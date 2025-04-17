use std::{marker::PhantomData, mem, rc::Rc, time::Duration};

use anyhow::Result;
use iced::{window::Id, Color, Element, Font, Point, Size, Task, Theme};
use tokio::time;

use crate::{
    app::{MapTask, Message},
    config::{Config, IndicatorDisplay, Placement},
    dbus::client::{Fcitx5Services, Fcitx5VirtualKeyboardServiceProxy},
    layout::{self, KeyAreaLayout, ToElementCommonParams},
    state::{LayoutEvent, LayoutState, UpdateConfigEvent},
    widget::{Movable, Toggle, ToggleCondition},
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
struct WindowState<WM> {
    id: Option<Id>,
    state: InnerWindowState,
    close_req_token: u16,
    positions: (Option<Point>, Option<Point>),
    movable: bool,
    phantom: PhantomData<WM>,
}

impl<WM> WindowState<WM> {
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

    fn close_with_delay(&mut self, delay: Duration, source: CloseOpSource) -> Task<Message> {
        if let Some(snapshot) = self.snapshot() {
            tracing::debug!("waiting to close window: {:?}", snapshot);
            Task::future(time::sleep(delay)).map(move |_| {
                WindowEvent::ClosingWindow(snapshot.id, Some(snapshot), source).into()
            })
        } else {
            tracing::debug!("window is already closed");
            Message::nothing()
        }
    }

    fn set_closed(&mut self) -> Option<CloseOpSource> {
        if let Some(id) = self.id.take() {
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

    fn movable(&self) -> bool {
        self.movable
    }
}

impl<WM> WindowState<WM>
where
    WM: WindowManager,
{
    fn set_opened(&mut self, wm: &mut WM, portrait: bool) {
        let Some(id) = self.id else {
            tracing::error!("window is closed, can't set_opened");
            return;
        };
        if let InnerWindowState::Init = self.state {
            self.state = InnerWindowState::Opened;
            if Some(Placement::Float) == wm.placement(id) {
                if portrait {
                    self.positions.1 = wm.position(id);
                } else {
                    self.positions.0 = wm.position(id);
                }
                tracing::debug!(
                    "update window[{}] positions: {:?}, portrait: {}",
                    id,
                    self.positions,
                    portrait
                );
            }
            return;
        }
        tracing::error!(
            "window[{}] is in a wrong state: {:?}, can't update to {:?}",
            id,
            self.state,
            InnerWindowState::Opened
        )
    }

    fn open(
        &mut self,
        wm: &mut WM,
        mut settings: WindowSettings,
        portrait: bool,
    ) -> Task<WM::Message> {
        if let Some(id) = self.id {
            tracing::warn!("window[{}] is already shown", id);
            // disable all pending close requests.
            self.inc_close_req_token();
            WM::nothing()
        } else {
            if settings.placement() == Placement::Float {
                let position = if portrait {
                    self.positions.1
                } else {
                    self.positions.0
                };
                tracing::debug!("opening window in position: {:?}", position);
                if let Some(position) = position {
                    settings = settings.set_position(position);
                }
            }
            let (id, task) = wm.open(settings);
            tracing::debug!("opening window: {}", id);
            self.id = Some(id);
            self.state = InnerWindowState::Init;
            self.close_req_token = 0;
            task
        }
    }

    fn close_checked(
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
            WM::nothing()
        }
    }

    fn close(&mut self, wm: &mut WM, source: CloseOpSource) -> Task<WM::Message> {
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
                tracing::debug!("window is already closed");
            }
        }
        WM::nothing()
    }

    fn resize(&mut self, wm: &mut WM, size: Size) -> Task<WM::Message> {
        if let Some(id) = self.id {
            tracing::debug!("resizing window: {}, size: {:?}", id, size);
            wm.resize(id, size)
        } else {
            tracing::debug!("window is closed, don't resize");
            WM::nothing()
        }
    }

    fn mv(&mut self, wm: &mut WM, position: Point, portrait: bool) -> Task<WM::Message> {
        let Some(id) = self.id else {
            tracing::debug!("window is closed, don't move");
            return WM::nothing();
        };
        let Some(cur_position) = self.position(portrait) else {
            // ignore
            tracing::debug!("no position info of window[{}]", id);
            return WM::nothing();
        };
        tracing::debug!(
            "moving {} from position[{:?}] to position[{:?}]",
            id,
            cur_position,
            position
        );
        let task = wm.mv(id, position);
        // use latest position
        if portrait {
            self.positions.1 = wm.position(id);
        } else {
            self.positions.0 = wm.position(id);
        }
        task
    }

    fn position(&self, portrait: bool) -> Option<Point> {
        self.id.and({
            if portrait {
                self.positions.1
            } else {
                self.positions.0
            }
        })
    }

    fn set_movable(&mut self, wm: &WM, movable: bool) {
        let Some(id) = self.id else {
            tracing::debug!("window is closed, don't set movable");
            return;
        };

        self.movable = movable && wm.placement(id) == Some(Placement::Float);
    }

    fn fix_position(&mut self, wm: &mut WM, portrait: bool) -> Option<Task<WM::Message>> {
        if self.id.and_then(|id| wm.placement(id)) == Some(Placement::Dock) {
            return None;
        }
        self.position(portrait)
            .map(|position| self.mv(wm, position, portrait))
    }
}

#[derive(Clone, Debug)]
pub enum WindowEvent {
    Opened(Id, Size),
    ClosingWindow(Id, Option<WindowStateSnapshot>, CloseOpSource),
    Closed(Id),
    Move(Id, Point),
    SetMovable(Id, bool),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    ExternalDock,
}

pub struct WindowManagerState<WM> {
    scale_factor: f32,
    landscape_layout: LayoutState,
    portrait_layout: LayoutState,
    keyboard_window_state: WindowState<WM>,
    indicator_window_state: WindowState<WM>,
    mode: Mode,
    /// a value sync with config file
    placement: Placement,
    indicator_width: u16,
    /// a value sync with config file
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
            scale_factor: 1.,
            landscape_layout: LayoutState::new(config.landscape_width(), key_area_layout.clone())?,
            portrait_layout: LayoutState::new(config.portrait_width(), key_area_layout.clone())?,
            keyboard_window_state: Default::default(),
            indicator_window_state: Default::default(),
            mode: Mode::Normal,
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
    pub fn on_layout_event(&mut self, event: LayoutEvent) {
        self.landscape_layout.on_event(event.clone());
        self.portrait_layout.on_event(event);
    }

    pub fn scale_factor(&self) -> f32 {
        self.scale_factor
    }

    fn update_scale_factor(&mut self, scale_factor: f32) -> bool {
        if scale_factor == self.scale_factor {
            return false;
        }
        let landscape_res = self.landscape_layout.update_scale_factor(scale_factor);
        if landscape_res.is_err() {
            tracing::warn!(
                "unable to update scale factor of landscape layout: {}",
                scale_factor
            );
        }
        let portrait_res = self.portrait_layout.update_scale_factor(scale_factor);
        if portrait_res.is_err() {
            tracing::warn!(
                "unable to update scale factor of portrait layout: {}",
                scale_factor
            );
        }
        match (landscape_res, portrait_res) {
            (Ok(_), Ok(_)) => {
                self.scale_factor = scale_factor;
                true
            }
            (Ok(old), Err(_)) => {
                // reset landscape to old layout
                if self.landscape_layout.update_scale_factor(old).is_err() {
                    // should't be failed
                    unreachable!("reset landscape to old scale factor failed");
                }
                false
            }
            (Err(_), Ok(old)) => {
                // reset portrait to old layout
                if self.portrait_layout.update_scale_factor(old).is_err() {
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
                if self.landscape_layout.update_key_area_layout(old).is_err() {
                    // should't be failed
                    unreachable!("reset landscape to old layout failed");
                }
                false
            }
            (Err(_), Ok(old)) => {
                // reset portrait to old layout
                if self.portrait_layout.update_key_area_layout(old).is_err() {
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

    pub fn placement(&self) -> Placement {
        match self.mode {
            Mode::Normal => self.placement,
            Mode::ExternalDock => Placement::Dock,
        }
    }

    pub fn indicator_display(&self) -> IndicatorDisplay {
        match self.mode {
            Mode::Normal => self.indicator_display,
            Mode::ExternalDock => IndicatorDisplay::AlwaysOff,
        }
    }
}

impl<WM> WindowManagerState<WM>
where
    WM: WindowManager,
{
    pub fn is_portrait(&self) -> bool {
        let screen_size = self.wm.full_screen_size();
        screen_size.height > screen_size.width
    }

    pub fn available_candidate_width(&self) -> u16 {
        if self.is_portrait() {
            self.portrait_layout.available_candidate_width()
        } else {
            self.landscape_layout.available_candidate_width()
        }
    }

    pub fn is_setting_shown(&self) -> bool {
        if self.is_portrait() {
            self.portrait_layout.is_setting_shown()
        } else {
            self.landscape_layout.is_setting_shown()
        }
    }

    pub fn size(&self) -> Size {
        if self.is_portrait() {
            self.portrait_layout.size()
        } else {
            self.landscape_layout.size()
        }
    }

    pub fn unit(&self) -> u16 {
        if self.is_portrait() {
            self.portrait_layout.unit()
        } else {
            self.landscape_layout.unit()
        }
    }

    fn window_state(&self, id: Id) -> Option<&WindowState<WM>> {
        if self.is_keyboard(id) {
            Some(&self.keyboard_window_state)
        } else if self.is_indicator(id) {
            Some(&self.indicator_window_state)
        } else {
            None
        }
    }

    pub fn is_keyboard(&self, id: Id) -> bool {
        Some(id) == self.keyboard_window_state.id()
    }

    pub fn is_indicator(&self, id: Id) -> bool {
        Some(id) == self.indicator_window_state.id()
    }

    fn update_screen_size(&mut self, screen_size: Size) -> bool {
        let old_full_screen_size = self.wm.full_screen_size();
        let res = self.wm.set_screen_size(screen_size);
        let new_full_screen_size = self.wm.full_screen_size();
        tracing::debug!(
            "update screen size, old full size: {:?}, new full size: {:?}",
            old_full_screen_size,
            new_full_screen_size,
        );
        res
    }

    pub fn position(&self, id: Id) -> Option<Point> {
        self.window_state(id)
            .and_then(|s| s.position(self.is_portrait()))
    }

    pub fn movable(&self, id: Id) -> bool {
        self.window_state(id).map(|s| s.movable()).unwrap_or(false)
    }

    pub fn to_element<'b>(&self, params: ToElementCommonParams<'b>) -> Element<'b, Message> {
        let id = params.window_id;
        if self.is_keyboard(id) {
            if self.is_portrait() {
                self.portrait_layout.to_element(&params)
            } else {
                self.landscape_layout.to_element(&params)
            }
        } else {
            let state = params.state;
            let message = if self.keyboard_window_state.id().is_some() {
                WindowManagerEvent::CloseKeyboard(CloseOpSource::UserAction).into()
            } else {
                WindowManagerEvent::OpenKeyboard.into()
            };
            let movable = self.movable(id);
            Toggle::new(
                Movable::new(
                    layout::indicator_btn(self.indicator_width).on_press(message),
                    move |delta| {
                        state
                            .new_position_message(id, delta)
                            .unwrap_or(Message::Nothing)
                    },
                    movable,
                )
                .on_move_end(WindowEvent::SetMovable(id, false).into()),
                ToggleCondition::LongPress(Duration::from_millis(1000)),
            )
            .on_toggle(WindowEvent::SetMovable(id, !movable).into())
            .into()
        }
    }
}

impl<WM> WindowManagerState<WM>
where
    WM: WindowManager,
    WM::Message: From<Message> + 'static + Send + Sync,
{
    pub fn shutdown(&mut self) -> Task<WM::Message> {
        // in fcitx5, calling hideVirtualKeyboardForcibly doesn't set InputMethodMode::PhysicalKeyboard. it causes that if we doesn't press any physical keys fcitx5 will still kept in InputMethodMode::VirtualKeyboard and its icon in tray will be gone.
        // self.fcitx5_hide().chain(iced::exit()).map_task()
        let mut task = self
            .keyboard_window_state
            .close(&mut self.wm, CloseOpSource::UserAction);
        task = task.chain(
            self.indicator_window_state
                .close(&mut self.wm, CloseOpSource::UserAction),
        );
        task.chain(iced::exit())
    }

    pub fn open_indicator(&mut self) -> Task<WM::Message> {
        match self.indicator_display() {
            IndicatorDisplay::Auto | IndicatorDisplay::AlwaysOn => {
                if self.indicator_window_state.id().is_none() {
                    self.to_be_opened = Some(ToBeOpened::Indicator);
                    self.wm.fetch_screen_info()
                } else {
                    // manually increase close_req_token
                    self.indicator_window_state.inc_close_req_token();
                    Message::from_nothing()
                }
            }
            IndicatorDisplay::AlwaysOff => self.open_keyboard(),
        }
    }

    pub fn close_indicator(&mut self) -> Task<WM::Message> {
        self.indicator_window_state
            .close(&mut self.wm, CloseOpSource::UserAction)
    }

    pub fn open_keyboard(&mut self) -> Task<WM::Message> {
        let mut task = Task::done(Message::from(ImEvent::SyncImList).into())
            .chain(Task::done(Message::from(ImEvent::SyncCurrentIm).into()));
        if self.keyboard_window_state.id().is_none() {
            self.to_be_opened = Some(ToBeOpened::Keyboard);
            task = task.chain(self.wm.fetch_screen_info());
        } else {
            // manually increase close_req_token
            self.keyboard_window_state.inc_close_req_token();
        }
        task
    }

    pub fn close_keyboard(&mut self, source: CloseOpSource) -> Task<WM::Message> {
        match source {
            CloseOpSource::Fcitx5 => self
                .keyboard_window_state
                .close_with_delay(Duration::from_millis(1000), source)
                .map_task(),
            CloseOpSource::UserAction => {
                let mut task = self.keyboard_window_state.close(&mut self.wm, source);
                if (self.indicator_display() == IndicatorDisplay::Auto
                    || self.indicator_display() == IndicatorDisplay::AlwaysOn)
                    && self.indicator_window_state.id().is_none()
                {
                    task = self.open_indicator().chain(task);
                }
                task
            }
        }
    }

    fn update_mode(&mut self, mut mode: Mode) -> Task<WM::Message> {
        if self.mode == mode {
            return Message::from_nothing();
        }
        mem::swap(&mut self.mode, &mut mode);
        match self.mode {
            Mode::Normal => {
                let mut task = self.reset_indicator();
                if self.keyboard_window_state.id().is_some() {
                    task = task.chain(self.reopen_keyboard());
                }
                task
            }
            Mode::ExternalDock => self.open_keyboard().chain(self.close_indicator()),
        }
    }

    fn update_placement(&mut self, placement: Placement) -> Task<WM::Message> {
        if self.placement != placement {
            let mut task =
                Task::done(Message::from(UpdateConfigEvent::Placement(placement)).into());
            self.placement = placement;
            if self.keyboard_window_state.id().is_some() {
                task = task.chain(self.reopen_keyboard())
            }
            task
        } else {
            Message::from_nothing()
        }
    }

    fn update_indicator_display(
        &mut self,
        indicator_display: IndicatorDisplay,
    ) -> Task<WM::Message> {
        if self.indicator_display != indicator_display {
            let task = Task::done(
                Message::from(UpdateConfigEvent::IndicatorDisplay(indicator_display)).into(),
            );
            self.indicator_display = indicator_display;
            task.chain(self.reset_indicator())
        } else {
            Message::from_nothing()
        }
    }

    fn reset_indicator(&mut self) -> Task<WM::Message> {
        let mut task = Message::from_nothing();
        match self.indicator_display() {
            IndicatorDisplay::Auto => {
                if self.keyboard_window_state.id.is_none() {
                    task = self.open_indicator();
                } else {
                    task = self.close_indicator();
                }
            }
            IndicatorDisplay::AlwaysOn => {
                if self.indicator_window_state.id.is_none() {
                    task = self.open_indicator();
                }
            }
            IndicatorDisplay::AlwaysOff => {
                if self.indicator_window_state.id.is_some() {
                    task = self.close_indicator();
                }
            }
        }
        task
    }

    fn reopen_keyboard(&mut self) -> Task<WM::Message> {
        // If the keyboard is closed, it will trigger fetching screen info. After screen size is
        // fetched, it will open a new keyboard window.
        self.to_be_opened = Some(ToBeOpened::Keyboard);
        self.close_keyboard(CloseOpSource::UserAction)
    }

    fn update_unit(&mut self, unit: u16) -> Task<WM::Message> {
        let portrait = self.is_portrait();
        let res = if portrait {
            self.portrait_layout.update_unit(unit)
        } else {
            self.landscape_layout.update_unit(unit)
        };

        if res.is_ok() {
            let (event, size) = if portrait {
                let size = self.portrait_layout.size();
                (UpdateConfigEvent::PortraitWidth(size.width as u16), size)
            } else {
                let size = self.landscape_layout.size();
                (UpdateConfigEvent::LandscapeWidth(size.width as u16), size)
            };
            // resize and update config
            self.keyboard_window_state
                .resize(&mut self.wm, size)
                .chain(Task::done(Message::from(event).into()))
        } else {
            Message::from_nothing()
        }
    }

    pub fn on_window_event(&mut self, event: WindowEvent) -> Task<WM::Message> {
        let portrait = self.is_portrait();
        match event {
            WindowEvent::Opened(id, size) => {
                let mut task = self.wm.opened(id, size);
                if self.is_keyboard(id) {
                    self.keyboard_window_state
                        .set_opened(&mut self.wm, portrait);
                    task = task.chain(self.fcitx5_show().map_task());
                    if self.indicator_display() == IndicatorDisplay::Auto {
                        task = task.chain(self.close_indicator());
                    }
                    if self.placement() == Placement::Dock {
                        task = task.chain(self.wm.fetch_screen_info());
                    }
                } else if self.is_indicator(id) {
                    self.indicator_window_state
                        .set_opened(&mut self.wm, portrait);
                    if self.indicator_display() == IndicatorDisplay::Auto {
                        task = task.chain(self.close_keyboard(CloseOpSource::UserAction));
                    }
                }
                task
            }
            WindowEvent::ClosingWindow(id, snapshot, source) => {
                let mut task = Message::from_nothing();
                let window_state = if self.is_keyboard(id) {
                    if (self.indicator_display() == IndicatorDisplay::Auto
                        || self.indicator_display() == IndicatorDisplay::AlwaysOn)
                        && self.indicator_window_state.id().is_none()
                    {
                        task = self.open_indicator();
                    }
                    Some(&mut self.keyboard_window_state)
                } else if self.is_indicator(id) {
                    Some(&mut self.indicator_window_state)
                } else {
                    None
                };
                if let Some(window_state) = window_state {
                    if let Some(snapshot) = snapshot {
                        task =
                            task.chain(window_state.close_checked(&mut self.wm, snapshot, source));
                    } else {
                        task = task.chain(window_state.close(&mut self.wm, source));
                    }
                }
                task
            }
            WindowEvent::Closed(id) => {
                let mut task = self.wm.closed(id);
                if self.is_keyboard(id) {
                    if Some(CloseOpSource::UserAction) == self.keyboard_window_state.set_closed() {
                        task = task.chain(self.fcitx5_hide().map_task());
                    }
                    task = task.chain(self.wm.fetch_screen_info());
                } else if self.is_indicator(id) {
                    self.indicator_window_state.set_closed();
                }
                task
            }
            WindowEvent::Move(id, position) => {
                let mut task = Message::from_nothing();
                let window_state = if self.is_keyboard(id) {
                    Some(&mut self.keyboard_window_state)
                } else if self.is_indicator(id) {
                    Some(&mut self.indicator_window_state)
                } else {
                    None
                };
                if let Some(window_state) = window_state {
                    task = task.chain(window_state.mv(&mut self.wm, position, portrait));
                }
                task
            }
            WindowEvent::SetMovable(id, movable) => {
                let window_state = if self.is_keyboard(id) {
                    Some(&mut self.keyboard_window_state)
                } else if self.is_indicator(id) {
                    Some(&mut self.indicator_window_state)
                } else {
                    None
                };
                if let Some(window_state) = window_state {
                    window_state.set_movable(&self.wm, movable);
                }
                Message::from_nothing()
            }
        }
    }

    pub fn on_event(&mut self, event: WindowManagerEvent) -> Task<WM::Message> {
        match event {
            WindowManagerEvent::ScreenInfo(screen_size, scale_factor) => {
                let update1 = self.update_screen_size(screen_size);
                let update2 = self.update_scale_factor(scale_factor);
                let portrait = self.is_portrait();
                match self.to_be_opened.take() {
                    Some(ToBeOpened::Keyboard) => {
                        let task = if update1 || update2 {
                            Task::done(WM::Message::from(ImEvent::ResetCandidateCursor.into()))
                        } else {
                            Message::from_nothing()
                        };
                        let size = if self.is_portrait() {
                            self.portrait_layout.size()
                        } else {
                            self.landscape_layout.size()
                        };
                        let mut window_settings = WindowSettings::new(Some(size), self.placement());
                        // set default float position.
                        if self.placement() == Placement::Float {
                            window_settings = window_settings.set_position(
                                (
                                    (screen_size.width - size.width) / 2.,
                                    screen_size.height - size.height,
                                )
                                    .into(),
                            );
                        }
                        task.chain(self.keyboard_window_state.open(
                            &mut self.wm,
                            window_settings,
                            portrait,
                        ))
                    }
                    Some(ToBeOpened::Indicator) => {
                        let window_settings = WindowSettings::new(
                            Some(Size::new(
                                self.indicator_width as f32,
                                self.indicator_width as f32,
                            )),
                            Placement::Float,
                        );
                        self.indicator_window_state
                            .open(&mut self.wm, window_settings, portrait)
                    }
                    None => {
                        let mut task = Message::from_nothing();
                        if update1 {
                            if let Some(t) = self
                                .keyboard_window_state
                                .fix_position(&mut self.wm, portrait)
                            {
                                task = task.chain(t);
                            }
                            if let Some(t) = self
                                .indicator_window_state
                                .fix_position(&mut self.wm, portrait)
                            {
                                task = task.chain(t);
                            }
                        }
                        task
                    }
                }
            }
            WindowManagerEvent::OpenKeyboard => self.open_keyboard(),
            WindowManagerEvent::CloseKeyboard(source) => self.close_keyboard(source),
            WindowManagerEvent::OpenIndicator => self.open_indicator(),
            WindowManagerEvent::UpdateMode(mode) => self.update_mode(mode),
            WindowManagerEvent::UpdatePlacement(placement) => self.update_placement(placement),
            WindowManagerEvent::UpdateIndicatorDisplay(indicator_display) => {
                self.update_indicator_display(indicator_display)
            }
            WindowManagerEvent::UpdateUnit(unit) => self.update_unit(unit),
        }
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
            WM::Appearance::default(theme)
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
            "send toggle event failed".to_string(),
            |s| async move {
                s.toggle_virtual_keyboard().await?;
                Ok(Message::Nothing)
            },
        )
    }

    fn fcitx5_show(&self) -> Task<Message> {
        super::call_fcitx5(
            self.fcitx5_virtual_keyboard_service(),
            "send show event failed".to_string(),
            |s| async move {
                s.show_virtual_keyboard().await?;
                Ok(Message::Nothing)
            },
        )
    }

    fn fcitx5_hide(&self) -> Task<Message> {
        super::call_fcitx5(
            self.fcitx5_virtual_keyboard_service(),
            "send hide event failed".to_string(),
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
    ScreenInfo(Size, f32),
    UpdateMode(Mode),
    UpdatePlacement(Placement),
    UpdateIndicatorDisplay(IndicatorDisplay),
    UpdateUnit(u16),
}

impl From<WindowManagerEvent> for Message {
    fn from(value: WindowManagerEvent) -> Self {
        Self::WindowManagerEvent(value)
    }
}
