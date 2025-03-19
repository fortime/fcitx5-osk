use std::{rc::Rc, time::Duration};

use anyhow::Result;
use iced::{
    advanced::svg::Handle as SvgHandle,
    widget::svg::Svg,
    window::{Id, Position},
    Color, Element, Font, Size, Task, Theme,
};
use tokio::time;

use crate::{
    app::{MapTask, Message},
    config::{Config, IndicatorDisplay, Placement},
    dbus::client::{Fcitx5Services, Fcitx5VirtualKeyboardServiceProxy},
    layout::{self, KeyAreaLayout, KeyManager, KeyboardManager},
    state::{CandidateAreaState, LayoutEvent, LayoutState},
    window::{WindowAppearance, WindowManager, WindowSettings},
};

use super::ImEvent;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WindowStateSnapshot {
    id: Id,
    close_req_token: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseOpSource {
    Fcitx5,
    UserAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum InnerWindowState {
    Init,
    Opened,
    Closing(CloseOpSource),
    #[default]
    Closed,
}

#[derive(Default)]
struct WindowState {
    id: Option<Id>,
    state: InnerWindowState,
    close_req_token: u16,
    fcitx5_services: Option<Fcitx5Services>,
    positions: (Option<Position>, Option<Position>),
}

impl WindowState {
    fn id(&self) -> Option<Id> {
        self.id
    }

    fn snapshot(&self) -> Option<WindowStateSnapshot> {
        self.id.map(|id| WindowStateSnapshot {
            id,
            close_req_token: self.close_req_token,
        })
    }

    fn inc_close_req_token(&mut self) {
        self.close_req_token = self.close_req_token.wrapping_add(1);
    }

    fn set_opened(&mut self) {
        if let InnerWindowState::Init = self.state {
            self.state = InnerWindowState::Opened;
            return;
        }
        tracing::error!(
            "window[{:?}] is in a wrong state: {:?}, can't update to {:?}",
            self.id,
            self.state,
            InnerWindowState::Opened
        )
    }

    fn open<WM: WindowManager>(
        &mut self,
        wm: &mut WM,
        settings: WindowSettings,
    ) -> Task<WM::Message> {
        if let Some(id) = self.id {
            tracing::warn!("window[{}] is already shown", id);
            // disable all pending close requests    .
            self.inc_close_req_token();
            Task::none()
        } else {
            let (id, task) = wm.open(settings);
            tracing::debug!("opening window: {}", id);
            self.id = Some(id);
            self.state = InnerWindowState::Init;
            self.close_req_token = 0;
            task
        }
    }

    fn close_with_delay(&mut self, delay: Duration, source: CloseOpSource) -> Task<Message> {
        if let Some(snapshot) = self.snapshot() {
            tracing::debug!("waiting to close window: {:?}", snapshot);
            Task::future(time::sleep(delay)).map(move |_| {
                WindowEvent::ClosingWindow(snapshot.id, Some(snapshot), source).into()
            })
        } else {
            tracing::debug!("window is already hidden");
            Task::none()
        }
    }

    fn close_checked<WM: WindowManager>(
        &mut self,
        wm: &mut WM,
        last: WindowStateSnapshot,
        source: CloseOpSource,
    ) -> Task<WM::Message> {
        let snapshot = self.snapshot();
        if snapshot == Some(last) {
            self.close(wm, source)
        } else {
            tracing::debug!(
                "window state snapshot doesn't match, last: {:?}, current: {:?}",
                snapshot,
                last
            );
            Task::none()
        }
    }

    fn close<WM: WindowManager>(
        &mut self,
        wm: &mut WM,
        source: CloseOpSource,
    ) -> Task<WM::Message> {
        match (self.id, self.state) {
            (Some(id), InnerWindowState::Init) | (Some(id), InnerWindowState::Opened) => {
                self.state = InnerWindowState::Closing(source);
                tracing::debug!("closing window: {}", id);
                return wm.close(id);
            }
            (Some(id), InnerWindowState::Closing(_)) => {
                if source == CloseOpSource::Fcitx5 {
                    tracing::debug!("update close op source: {:?}, window: {:?}", source, id);
                    self.state = InnerWindowState::Closing(source);
                }
            }
            (None, _) | (_, InnerWindowState::Closed) => {
                tracing::debug!("window is already hidden");
            }
        }
        Task::none()
    }

    fn set_closed(&mut self) -> Option<CloseOpSource> {
        if let Some(id) = self.id {
            let source = match self.state {
                InnerWindowState::Closing(source) => Some(source),
                InnerWindowState::Closed => None,
                _ => Some(CloseOpSource::UserAction),
            };
            if source.is_some() {
                tracing::debug!("window[{}] closed", id);
                self.state = InnerWindowState::Closed;
            }
            source
        } else {
            None
        }
    }

    fn resize<WM: WindowManager>(&mut self, wm: &mut WM, size: Size) -> Task<WM::Message> {
        if let Some(id) = self.id {
            tracing::debug!("resizing window: {}", id);
            wm.resize(id, size)
        } else {
            tracing::debug!("window is hidden, don't resize");
            Task::none()
        }
    }

    fn reset_positions(&mut self) {
        self.positions = (None, None);
    }
}

#[derive(Clone, Debug)]
pub enum WindowEvent {
    // Resize(Id, f32, u16),
    SyncSize(Id),
    Opened(Id, Size),
    ClosingWindow(Id, Option<WindowStateSnapshot>, CloseOpSource),
    Closed(Id),
}

impl From<WindowEvent> for Message {
    fn from(value: WindowEvent) -> Self {
        Self::WindowEvent(value)
    }
}

#[derive(Clone, Copy)]
enum ToBeOpened {
    Keyboard,
    Indicator,
}

pub struct WindowManagerState<WM> {
    screen_size: Size,
    scale_factor: f32,
    landscape_layout: LayoutState,
    portrait_layout: LayoutState,
    keyboard_window_state: WindowState,
    indicator_window_state: WindowState,
    placement: Placement,
    indicator_width: u16,
    indicator_display: IndicatorDisplay,
    to_be_opened: Option<ToBeOpened>,
    fcitx5_services: Option<Fcitx5Services>,
    wm: WM,
}

impl<WM> WindowManagerState<WM>
where
    WM: Default,
{
    pub fn new(config: &Config, key_area_layout: Rc<KeyAreaLayout>) -> Result<Self> {
        Ok(Self {
            screen_size: Size::new(0., 0.),
            scale_factor: 1.,
            landscape_layout: LayoutState::new(config.landscape_width(), key_area_layout.clone())?,
            portrait_layout: LayoutState::new(config.portrait_width(), key_area_layout.clone())?,
            keyboard_window_state: Default::default(),
            indicator_window_state: Default::default(),
            placement: config.placement(),
            indicator_width: config.indicator_width(),
            indicator_display: config.indicator_display(),
            to_be_opened: None,
            fcitx5_services: None,
            wm: Default::default(),
        })
    }
}

impl<WM> WindowManagerState<WM> {
    pub fn is_portrait(&self) -> bool {
        self.screen_size.height > self.screen_size.width
    }

    pub fn available_candidate_width_p(&self) -> u16 {
        if self.is_portrait() {
            self.portrait_layout.available_candidate_width_p()
        } else {
            self.landscape_layout.available_candidate_width_p()
        }
    }

    pub fn on_layout_event(&mut self, event: LayoutEvent) {
        self.landscape_layout.on_event(event.clone());
        self.portrait_layout.on_event(event);
    }

    pub fn size(&self) -> Size {
        if self.is_portrait() {
            self.portrait_layout.size()
        } else {
            self.landscape_layout.size()
        }
    }

    pub fn update_width(&mut self, width: u16, is_portrait: bool) -> bool {
        if is_portrait {
            self.portrait_layout.update_width(width).is_ok()
        } else {
            self.landscape_layout.update_width(width).is_ok()
        }
    }

    fn update_scale_factor(&mut self, scale_factor: f32) -> bool {
        if scale_factor == self.scale_factor {
            return false;
        }
        let landscape_res = self.landscape_layout.update_scale_factor(scale_factor);
        let portrait_res = self.portrait_layout.update_scale_factor(scale_factor);
        match (landscape_res, portrait_res) {
            (Ok(_), Ok(_)) => {
                self.scale_factor = scale_factor;
                true
            }
            (Ok(old), Err(_)) => {
                // reset landscape to old layout
                if let Err(_) = self.landscape_layout.update_scale_factor(old) {
                    // should't be failed
                    unreachable!("reset landscape to old scale factor failed");
                }
                false
            }
            (Err(_), Ok(old)) => {
                // reset portrait to old layout
                if let Err(_) = self.portrait_layout.update_scale_factor(old) {
                    // should't be failed
                    unreachable!("reset portrait to old scale factor failed");
                }
                false
            }
            _ => false,
        }
    }

    pub fn update_key_area_layout(&mut self, key_area_layout: Rc<KeyAreaLayout>) -> bool {
        let landscape_res = self
            .landscape_layout
            .update_key_area_layout(key_area_layout.clone());
        let portrait_res = self.portrait_layout.update_key_area_layout(key_area_layout);
        match (landscape_res, portrait_res) {
            (Ok(_), Ok(_)) => true,
            (Ok(old), Err(_)) => {
                // reset landscape to old layout
                if let Err(_) = self.landscape_layout.update_key_area_layout(old) {
                    // should't be failed
                    unreachable!("reset landscape to old layout failed");
                }
                false
            }
            (Err(_), Ok(old)) => {
                // reset portrait to old layout
                if let Err(_) = self.portrait_layout.update_key_area_layout(old) {
                    // should't be failed
                    unreachable!("reset portrait to old layout failed");
                }
                false
            }
            _ => false,
        }
    }

    pub fn update_candidate_font(&mut self, font: Font) {
        self.landscape_layout.update_candidate_font(font);
        self.portrait_layout.update_candidate_font(font);
    }
}

impl<WM> WindowManagerState<WM>
where
    WM: WindowManager,
    WM::Message: From<Message> + 'static + Send + Sync,
{
    pub fn is_keyboard(&self, id: Id) -> bool {
        Some(id) == self.keyboard_window_state.id()
    }

    pub fn is_indicator(&self, id: Id) -> bool {
        Some(id) == self.indicator_window_state.id()
    }

    pub fn open_indicator(&mut self) -> Task<WM::Message> {
        if self.indicator_window_state.id().is_none() {
            self.to_be_opened = Some(ToBeOpened::Indicator);
            self.wm.fetch_screen_info()
        } else {
            // manually increase close_req_token
            self.indicator_window_state.inc_close_req_token();
            Task::none()
        }
    }

    pub fn close_indicator(&mut self) -> Task<WM::Message> {
        todo!()
    }

    pub fn open_keyboard(&mut self) -> Task<WM::Message> {
        if self.keyboard_window_state.id().is_none() {
            self.to_be_opened = Some(ToBeOpened::Keyboard);
            self.wm.fetch_screen_info()
        } else {
            // manually increase close_req_token
            self.keyboard_window_state.inc_close_req_token();
            Task::none()
        }
    }

    pub fn to_element<'a, 'b, KbdM, KM, M>(
        &'a self,
        id: Id,
        candidate_area_state: &'b CandidateAreaState,
        keyboard_manager: &'b KbdM,
        key_manager: &'b KM,
        theme: &'a Theme,
    ) -> Element<'b, M>
    where
        KbdM: KeyboardManager<Message = M>,
        KM: KeyManager<Message = M>,
        M: 'static + Clone,
    {
        if self.is_keyboard(id) {
            if self.is_portrait() {
                self.portrait_layout.to_element(
                    candidate_area_state,
                    keyboard_manager,
                    key_manager,
                    theme,
                )
            } else {
                self.landscape_layout.to_element(
                    candidate_area_state,
                    keyboard_manager,
                    key_manager,
                    theme,
                )
            }
        } else {
            if self.keyboard_window_state.id().is_some() {
                layout::indicator_btn(self.indicator_width)
                    .on_press(keyboard_manager.close_keyboard())
                    .into()
            } else {
                layout::indicator_btn(self.indicator_width)
                    .on_press(keyboard_manager.open_keyboard())
                    .into()
            }
        }
    }

    pub fn close_keyboard(&mut self, source: CloseOpSource) -> Task<WM::Message> {
        match source {
            CloseOpSource::Fcitx5 => self
                .keyboard_window_state
                .close_with_delay(Duration::from_millis(1000), source)
                .map_task(),
            CloseOpSource::UserAction => self.keyboard_window_state.close(&mut self.wm, source),
        }
    }

    pub fn on_window_event(&mut self, event: WindowEvent) -> Task<WM::Message> {
        match event {
            //WindowEvent::Resize(id, scale_factor, width_p) => {
            //    tracing::debug!("scale_factor: {}", scale_factor);
            //    return self.update_width(id, width_p, scale_factor);
            //}
            WindowEvent::Opened(id, size) => {
                let mut task = self.wm.opened(id, size);
                if self.is_keyboard(id) {
                    self.keyboard_window_state.set_opened();
                    task = task.chain(self.fcitx5_show().map_task());
                } else if self.is_indicator(id) {
                    self.indicator_window_state.set_opened();
                }
                task
            }
            WindowEvent::ClosingWindow(id, snapshot, source) => {
                let window_state = if self.is_keyboard(id) {
                    &mut self.keyboard_window_state
                } else if self.is_indicator(id) {
                    &mut self.indicator_window_state
                } else {
                    return Task::none();
                };
                if let Some(snapshot) = snapshot {
                    return window_state.close_checked(&mut self.wm, snapshot, source);
                } else {
                    return window_state.close(&mut self.wm, source);
                }
            }
            WindowEvent::Closed(id) => {
                let mut task = self.wm.closed(id);
                if self.is_keyboard(id) {
                    if Some(CloseOpSource::UserAction) == self.keyboard_window_state.set_closed() {
                        task = task.chain(self.fcitx5_hide().map_task());
                    }
                } else if self.is_indicator(id) {
                    self.indicator_window_state.set_closed();
                }
                task
            }
            //WindowEvent::SyncSize(id) => {
            //    let size = self.window_size();
            //    return self.state.window_mut().resize(size);
            //}
            _ => Task::none(),
        }
    }

    fn update_screen_size(&mut self, screen_size: Size) -> bool {
        if screen_size != self.screen_size {
            if screen_size.width != self.screen_size.height
                || screen_size.height != self.screen_size.width
            {
                // not rotated, reset position
                self.keyboard_window_state.reset_positions();
                self.indicator_window_state.reset_positions();
            }
            self.screen_size = screen_size;
            true
        } else {
            false
        }
    }

    pub fn on_event(&mut self, event: WindowManagerEvent) -> Task<WM::Message> {
        match event {
            WindowManagerEvent::ScreenInfo(screen_size, scale_factor) => {
                let update1 = self.update_screen_size(screen_size);
                let update2 = self.update_scale_factor(scale_factor);
                if !update2 {
                    tracing::warn!("unable to update scale factor: {}", scale_factor);
                }
                match self.to_be_opened {
                    Some(ToBeOpened::Keyboard) => {
                        let task = if update1 || update2 {
                            Task::done(WM::Message::from(ImEvent::ResetCandidateCursor.into()))
                        } else {
                            Task::none()
                        };
                        let size = if self.is_portrait() {
                            self.portrait_layout.size()
                        } else {
                            self.landscape_layout.size()
                        };
                        let window_settings = WindowSettings::new(Some(size), self.placement);
                        return task.chain(
                            self.keyboard_window_state
                                .open(&mut self.wm, window_settings),
                        );
                    }
                    Some(ToBeOpened::Indicator) => {
                        let window_settings = WindowSettings::new(
                            Some(Size::new(
                                self.indicator_width as f32,
                                self.indicator_width as f32,
                            )),
                            Placement::Float,
                        );
                        return self
                            .indicator_window_state
                            .open(&mut self.wm, window_settings);
                    }
                    None => {}
                }
            }
            WindowManagerEvent::OpenKeyboard => return self.open_keyboard(),
            WindowManagerEvent::CloseKeyboard(source) => return self.close_keyboard(source),
            WindowManagerEvent::OpenIndicator => return self.open_indicator(),
            WindowManagerEvent::CloseIndicator => return self.close_indicator(),
        }
        Task::none()
    }
}

impl<WM> WindowManagerState<WM>
where
    WM: WindowManager,
    WM::Message: From<Message> + 'static + Send + Sync,
    WM::Appearance: WindowAppearance + 'static + Send + Sync,
{
    pub fn appearance(&self, theme: &Theme, id: Id) -> WM::Appearance {
        if self.is_keyboard(id) {
            let appearance = WM::Appearance::default(theme);
            appearance
        } else if self.is_indicator(id) {
            let mut appearance = WM::Appearance::default(theme);
            appearance.set_background_color(Color::TRANSPARENT);
            appearance
        } else {
            self.wm.appearance(theme, id)
        }
    }
}

// call fcitx5
impl<WM> WindowManagerState<WM> {
    pub(super) fn set_dbus_clients(&mut self, fcitx5_services: Fcitx5Services) {
        self.fcitx5_services = Some(fcitx5_services);
    }

    fn fcitx5_virtual_keyboard_service(
        &self,
    ) -> Option<&Fcitx5VirtualKeyboardServiceProxy<'static>> {
        self.fcitx5_services
            .as_ref()
            .map(Fcitx5Services::virtual_keyboard)
    }

    fn _fcitx5_toggle(&self) -> Task<Message> {
        super::call_fcitx5(
            self.fcitx5_virtual_keyboard_service(),
            format!("send toggle event failed"),
            |s| async move {
                s.toggle_virtual_keyboard().await?;
                Ok(Message::Nothing)
            },
        )
    }

    fn fcitx5_show(&self) -> Task<Message> {
        super::call_fcitx5(
            self.fcitx5_virtual_keyboard_service(),
            format!("send show event failed"),
            |s| async move {
                s.show_virtual_keyboard().await?;
                Ok(Message::Nothing)
            },
        )
    }

    fn fcitx5_hide(&self) -> Task<Message> {
        super::call_fcitx5(
            self.fcitx5_virtual_keyboard_service(),
            format!("send hide event failed"),
            |s| async move {
                s.hide_virtual_keyboard().await?;
                Ok(Message::Nothing)
            },
        )
    }
}

#[derive(Clone, Debug)]
pub enum WindowManagerEvent {
    OpenKeyboard,
    CloseKeyboard(CloseOpSource),
    OpenIndicator,
    CloseIndicator,
    ScreenInfo(Size, f32),
    // Resize(Id, f32, u16),
}

impl From<WindowManagerEvent> for Message {
    fn from(value: WindowManagerEvent) -> Self {
        Self::WindowManagerEvent(value)
    }
}
