use std::{marker::PhantomData, rc::Rc, time::Duration};

use anyhow::Result;
use fcitx5_osk_common::dbus::{client::Fcitx5OskServices, entity};
use iced::{window::Id, Color, Element, Font, Point, Size, Task, Theme};
use tokio::time;

use crate::{
    app::{MapTask, Message},
    config::{Config, IndicatorDisplay, Placement},
    dbus::client::{
        Fcitx5Services, Fcitx5VirtualKeyboardServiceExt, IFcitx5VirtualKeyboardService,
    },
    layout::{self, KeyAreaLayout, ToElementCommonParams},
    state::{LayoutEvent, LayoutState, UpdateConfigEvent},
    widget::{Movable, Toggle, ToggleCondition},
    window::{
        SyncOutputResponse, WindowAppearance, WindowManager, WindowManagerMode, WindowSettings,
    },
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
    DbusController,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum InnerWindowState {
    Init,
    Opened,
    Closing(CloseOpSource),
    #[default]
    Closed,
}

struct WindowState<WM> {
    name: String,
    id: Option<Id>,
    state: InnerWindowState,
    close_req_token: u16,
    positions: (Option<Point>, Option<Point>),
    movable: bool,
    phantom: PhantomData<WM>,
}

impl<WM> WindowState<WM> {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            id: Default::default(),
            state: Default::default(),
            close_req_token: Default::default(),
            positions: Default::default(),
            movable: Default::default(),
            phantom: PhantomData,
        }
    }

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
            tracing::debug!("Waiting to close window[{}]: {:?}", self.name, snapshot);
            Task::future(time::sleep(delay)).map(move |_| {
                WindowEvent::ClosingWindow(snapshot.id, Some(snapshot), source).into()
            })
        } else {
            tracing::debug!("Window[{}] is already closed", self.name);
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
                tracing::debug!("Window[{}/{}] closed", self.name, id);
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

    fn closing(&self) -> bool {
        matches!(self.state, InnerWindowState::Closing(_))
    }
}

impl<WM> WindowState<WM>
where
    WM: WindowManager,
{
    fn set_opened(&mut self, wm: &mut WM, portrait: bool) {
        let Some(id) = self.id else {
            tracing::error!("Window[{}] is closed, can't set_opened", self.name);
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
                    "Update window[{}/{}] positions: {:?}, portrait: {}",
                    self.name,
                    id,
                    self.positions,
                    portrait
                );
            }
            return;
        }
        tracing::error!(
            "Window[{}/{}] is in a wrong state: {:?}, can't update to {:?}",
            self.name,
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
            tracing::warn!("Window[{}/{}] is already shown", self.name, id);
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
                if let Some(position) = position {
                    settings = settings.set_position(position);
                }
            }
            tracing::debug!(
                "Opening window[{}] with settings: {:?}",
                self.name,
                settings
            );
            let (id, task) = wm.open(settings);
            if id.is_some() {
                tracing::debug!("Opening window[{}]: {:?}", self.name, id);
                self.id = id;
                self.state = InnerWindowState::Init;
                self.close_req_token = 0;
            }
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
                "Window[{}] state snapshot doesn't match, last: {:?}, current: {:?}",
                self.name,
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
                tracing::debug!("Closing window: {}/{}", self.name, id);
                return wm.close(id);
            }
            (Some(id), InnerWindowState::Closing(_)) => {
                if source == CloseOpSource::Fcitx5 {
                    tracing::debug!(
                        "Update close op source: {:?}, window: {}/{:?}",
                        source,
                        self.name,
                        id
                    );
                    self.state = InnerWindowState::Closing(source);
                }
            }
            (None, _) | (_, InnerWindowState::Closed) => {
                tracing::debug!("Window[{}] is already closed", self.name);
            }
        }
        WM::nothing()
    }

    fn resize(&mut self, wm: &mut WM, size: Size) -> Task<WM::Message> {
        // id exists and not closing
        if let Some(id) = self.opened_id() {
            tracing::debug!("Resizing window: {}/{}, size: {:?}", self.name, id, size);
            wm.resize(id, size)
        } else {
            tracing::debug!("Window[{}] is closed, don't resize", self.name);
            WM::nothing()
        }
    }

    fn mv(&mut self, wm: &mut WM, position: Point, portrait: bool) -> Task<WM::Message> {
        let Some(id) = self.opened_id() else {
            tracing::debug!("Window[{}] is closed, don't move", self.name);
            return WM::nothing();
        };
        let Some(cur_position) = self.position(portrait) else {
            // ignore
            tracing::debug!("No position info of window[{}/{}]", self.name, id);
            return WM::nothing();
        };
        tracing::debug!(
            "Moving window[{}/{}] from position[{:?}] to position[{:?}]",
            self.name,
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
        // Not closing or closed
        self.id.filter(|_| !self.closing()).and({
            if portrait {
                self.positions.1
            } else {
                self.positions.0
            }
        })
    }

    fn set_movable(&mut self, wm: &WM, movable: bool) {
        let Some(id) = self.opened_id() else {
            tracing::debug!("Window[{}] is closed, don't set movable", self.name);
            return;
        };

        self.movable = movable && wm.placement(id) == Some(Placement::Float);
    }

    fn fix_position(&mut self, wm: &mut WM, portrait: bool) -> Option<Task<WM::Message>> {
        if self.opened_id().and_then(|id| wm.placement(id)) == Some(Placement::Dock) {
            return None;
        }
        tracing::debug!("Fix position of window[{}]", self.name);
        self.position(portrait)
            .map(|position| self.mv(wm, position, portrait))
    }

    /// Return id only if the window is opened
    fn opened_id(&self) -> Option<Id> {
        self.id.filter(|_| self.state == InnerWindowState::Opened)
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
#[repr(u16)]
enum WindowMask {
    Keyboard = 0x0001,
    Indicator = 0x0002,
}

pub struct WindowManagerState<WM> {
    scale_factor: f32,
    portrait: bool,
    layout: LayoutState,
    keyboard_window_state: WindowState<WM>,
    indicator_window_state: WindowState<WM>,
    /// a value sync with config file
    placement: Placement,
    indicator_width: u16,
    /// a value sync with config file
    indicator_display: IndicatorDisplay,
    to_be_opened_flag: u16,
    fcitx5_services: Fcitx5Services,
    fcitx5_osk_services: Fcitx5OskServices,
    wm: WM,
    hide_delay: Duration,
}

impl<WM> WindowManagerState<WM> {
    pub fn new(
        config: &Config,
        wm: WM,
        portrait: bool,
        key_area_layout: Rc<KeyAreaLayout>,
        fcitx5_services: Fcitx5Services,
        fcitx5_osk_services: Fcitx5OskServices,
    ) -> Result<Self> {
        let max_width = if portrait {
            config.portrait_width()
        } else {
            config.landscape_width()
        };
        Ok(Self {
            scale_factor: 1.,
            portrait,
            layout: LayoutState::new(max_width, key_area_layout)?,
            keyboard_window_state: WindowState::new("keyboard"),
            indicator_window_state: WindowState::new("indicator"),
            placement: config.placement(),
            indicator_width: config.indicator_width(),
            indicator_display: config.indicator_display(),
            to_be_opened_flag: 0,
            fcitx5_services,
            fcitx5_osk_services,
            wm,
            hide_delay: *config.hide_delay(),
        })
    }

    pub fn on_layout_event(&mut self, event: LayoutEvent) {
        self.layout.on_event(event);
    }

    pub fn scale_factor(&self) -> f32 {
        self.scale_factor
    }

    fn update_scale_factor(&mut self, scale_factor: f32) -> bool {
        if scale_factor == self.scale_factor {
            return false;
        }
        let res = self.layout.update_scale_factor(scale_factor);
        if res.is_err() {
            tracing::warn!("unable to update scale factor of layout: {}", scale_factor);
            false
        } else {
            self.scale_factor = scale_factor;
            true
        }
    }

    pub fn update_candidate_font(&mut self, font: Font) {
        self.layout.update_candidate_font(font);
    }
}

impl<WM> WindowManagerState<WM>
where
    WM: WindowManager,
{
    pub fn is_portrait(&self) -> bool {
        self.portrait
    }

    pub fn available_candidate_width(&self) -> u16 {
        self.layout.available_candidate_width()
    }

    pub fn is_setting_shown(&self) -> bool {
        self.layout.is_setting_shown()
    }

    pub fn size(&self) -> Size {
        self.layout.size()
    }

    pub fn unit(&self) -> u16 {
        self.layout.unit()
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

    pub fn position(&self, id: Id) -> Option<Point> {
        self.window_state(id)
            .and_then(|s| s.position(self.is_portrait()))
    }

    pub fn movable(&self, id: Id) -> bool {
        self.window_state(id).map(|s| s.movable()).unwrap_or(false)
    }

    pub fn placement(&self) -> Placement {
        match self.mode() {
            WindowManagerMode::Normal => self.placement,
            WindowManagerMode::KwinLockScreen => Placement::Dock,
        }
    }

    pub fn indicator_display(&self) -> IndicatorDisplay {
        match self.mode() {
            WindowManagerMode::Normal => self.indicator_display,
            WindowManagerMode::KwinLockScreen => IndicatorDisplay::AlwaysOff,
        }
    }

    pub fn mode(&self) -> WindowManagerMode {
        self.wm.mode()
    }

    pub fn outputs(&self) -> Vec<(String, String)> {
        self.wm.outputs()
    }

    pub fn to_element<'b>(&self, params: ToElementCommonParams<'b>) -> Element<'b, Message> {
        let id = params.window_id;
        if self.is_keyboard(id) {
            self.layout.to_element(&params)
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
        //let mut task = self
        //    .keyboard_window_state
        //    .close(&mut self.wm, CloseOpSource::UserAction);
        //task = task.chain(
        //    self.indicator_window_state
        //        .close(&mut self.wm, CloseOpSource::UserAction),
        //);
        //task.chain(iced::exit())
        iced::exit()
    }

    pub fn open_indicator(&mut self) -> Task<WM::Message> {
        // Reset the flag
        self.unset_to_be_opened(WindowMask::Indicator);
        match self.indicator_display() {
            IndicatorDisplay::Auto | IndicatorDisplay::AlwaysOn => {
                if self.indicator_window_state.id().is_none() {
                    let portrait = self.is_portrait();
                    let window_settings = WindowSettings::new(
                        Size::new(self.indicator_width as f32, self.indicator_width as f32),
                        Placement::Float,
                    );
                    let task =
                        self.indicator_window_state
                            .open(&mut self.wm, window_settings, portrait);
                    // window is not opened, mark it to_be_opened
                    self.set_to_be_opened(WindowMask::Indicator, false);
                    task
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
        // Reset the flag
        self.unset_to_be_opened(WindowMask::Keyboard);
        let mut task = Task::done(Message::from(ImEvent::SyncImList).into())
            .chain(Task::done(Message::from(ImEvent::SyncCurrentIm).into()));
        if self.keyboard_window_state.id().is_none() {
            let portrait = self.is_portrait();
            task = task.chain(Task::done(WM::Message::from(
                ImEvent::ResetCandidateCursor.into(),
            )));
            let mut size = self.size();
            let screen_size = self.wm.screen_size();
            // update unit if width is too large
            if size.width > screen_size.width {
                // update unit
                let unit = self.layout.unit_within(screen_size.width as u16);
                if self
                    .layout
                    .update_unit(unit, screen_size.width as u16)
                    .is_ok()
                {
                    size = self.size();
                }
            }
            let mut window_settings = WindowSettings::new(size, self.placement());
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
            task = task.chain(self.keyboard_window_state.open(
                &mut self.wm,
                window_settings,
                portrait,
            ));
            // window is not opened, mark it to_be_opened
            self.set_to_be_opened(WindowMask::Keyboard, false);
        } else if self.keyboard_window_state.closing() {
            // don't chain fetch_screen_info, otherwise, to_be_opened will be consumed before the
            // keyboard is closed.
            //self.to_be_opened_flag |= WindowMask::Keyboard as u16;
            // TODO I have forgotten why this case is needed
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
                .close_with_delay(self.hide_delay, source)
                .map_task(),
            CloseOpSource::UserAction | CloseOpSource::DbusController => {
                let task = self.keyboard_window_state.close(&mut self.wm, source);
                // If Keyboard is reopening, don't open indicator
                if ((self.indicator_display() == IndicatorDisplay::Auto
                    && !self.need_opened(WindowMask::Keyboard))
                    || self.indicator_display() == IndicatorDisplay::AlwaysOn)
                    && self.indicator_window_state.id().is_none()
                {
                    self.set_to_be_opened(WindowMask::Indicator, false);
                }
                task
            }
        }
    }

    fn update_mode(&mut self, mode: WindowManagerMode) -> Task<WM::Message> {
        let res = self.wm.set_mode(mode);

        // use current mode.
        let mode = self.wm.mode();
        let mut task = super::call_dbus(
            self.fcitx5_osk_services.controller(),
            "setting fcitx5 osk mode failed".to_string(),
            |s| async move {
                let mode = match mode {
                    WindowManagerMode::Normal => entity::WindowManagerMode::Normal,
                    WindowManagerMode::KwinLockScreen => entity::WindowManagerMode::KwinLockScreen,
                };
                s.set_mode(mode).await?;
                Ok(Message::Nothing)
            },
        )
        .map_task();

        if !res {
            return task;
        }

        match mode {
            WindowManagerMode::Normal => {
                task = task.chain(self.reset_indicator());
                if let Some(next_task) = self.reopen_keyboard_if_opened() {
                    task = task.chain(next_task)
                }
                task
            }
            WindowManagerMode::KwinLockScreen => task
                .chain(self.close_indicator())
                // open keyboard in input-method activation
                .chain(self.close_keyboard(CloseOpSource::UserAction)),
        }
    }

    fn update_placement(&mut self, placement: Placement) -> Task<WM::Message> {
        if self.placement != placement {
            let mut task =
                Task::done(Message::from(UpdateConfigEvent::Placement(placement)).into());
            self.placement = placement;
            if let Some(next_task) = self.reopen_keyboard_if_opened() {
                task = task.chain(next_task)
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

    pub fn reopen_keyboard_if_opened(&mut self) -> Option<Task<WM::Message>> {
        if self.keyboard_window_state.id().is_some() {
            self.set_to_be_opened(WindowMask::Keyboard, true);
            Some(self.close_keyboard(CloseOpSource::UserAction))
        } else {
            None
        }
    }

    pub fn reopen_indicator_if_opened(&mut self) -> Option<Task<WM::Message>> {
        if self.indicator_window_state.id().is_some() {
            self.set_to_be_opened(WindowMask::Indicator, true);
            Some(self.close_indicator())
        } else {
            None
        }
    }

    fn update_unit(&mut self, unit: u16) -> Task<WM::Message> {
        let max_width = self.wm.screen_size().width as u16;
        let portrait = self.is_portrait();
        if self.layout.update_unit(unit, max_width).is_ok() {
            let (max_width, size) = (self.layout.max_width(), self.layout.size());
            let event = if portrait {
                UpdateConfigEvent::PortraitWidth(max_width)
            } else {
                UpdateConfigEvent::LandscapeWidth(max_width)
            };
            // resize and update config
            self.keyboard_window_state
                .resize(&mut self.wm, size)
                .chain(Task::done(Message::from(event).into()))
        } else {
            Message::from_nothing()
        }
    }

    pub fn update_key_area_layout(
        &mut self,
        max_width: u16,
        key_area_layout: Rc<KeyAreaLayout>,
    ) -> Option<Task<WM::Message>> {
        let old_size = self.size();
        let max_width = max_width.min(self.wm.screen_size().width as u16);
        let res = self
            .layout
            .update_key_area_layout(max_width, key_area_layout);
        if res.is_ok() {
            // resize if the size is changed
            let new_size = self.size();
            if new_size != old_size {
                Some(self.keyboard_window_state.resize(&mut self.wm, new_size))
            } else {
                Some(Message::from_nothing())
            }
        } else {
            None
        }
    }

    fn sync_output(&mut self) -> Task<WM::Message> {
        let res = self.wm.sync_output();

        let mut tasks = vec![];

        let screen_size = self.wm.screen_size();
        let mut reopen = false;
        let old_unit = self.unit();

        let portrait = screen_size.height > screen_size.width;
        if portrait != self.portrait {
            self.portrait = portrait;
            tasks.push(Task::done(Message::from(LayoutEvent::SyncLayout).into()));
        }

        let scale_factor = res
            .iter()
            .filter_map(|r| {
                if let SyncOutputResponse::ScaleFactorChanged(f) = r {
                    Some(*f as f32)
                } else {
                    None
                }
            })
            .next_back();
        if let Some(scale_factor) = scale_factor {
            self.update_scale_factor(scale_factor);
            reopen = reopen || old_unit != self.unit();
        }

        if res.contains(&SyncOutputResponse::SizeChanged) {
            let screen_size = self.wm.screen_size();
            reopen = self.size().width > screen_size.width;
        }

        reopen = reopen || res.contains(&SyncOutputResponse::RotationChanged);
        reopen = reopen || res.contains(&SyncOutputResponse::OutputChanged);

        if reopen {
            if let Some(task) = self.reopen_indicator_if_opened() {
                tasks.push(task);
            } else if let Some(task) = self.open_to_be_opened(WindowMask::Indicator) {
                // Check if window is need to be opened
                tasks.push(task);
            }
            if let Some(task) = self.reopen_keyboard_if_opened() {
                tasks.push(task);
            } else if let Some(task) = self.open_to_be_opened(WindowMask::Keyboard) {
                // Check if window is need to be opened
                tasks.push(task);
            }
        } else {
            if let Some(task) = self.open_to_be_opened(WindowMask::Indicator) {
                tasks.push(task);
            }
            if let Some(task) = self.open_to_be_opened(WindowMask::Keyboard) {
                tasks.push(task);
            }
        }
        if tasks.is_empty() {
            Message::from_nothing()
        } else {
            Task::batch(tasks)
        }
    }

    fn need_opened(&self, window_mask: WindowMask) -> bool {
        (self.to_be_opened_flag & window_mask as u16) != 0
    }

    fn open_to_be_opened(&mut self, window_mask: WindowMask) -> Option<Task<WM::Message>> {
        let to_be_opened = self.need_opened(window_mask);
        if !to_be_opened {
            None
        } else {
            Some(match window_mask {
                WindowMask::Keyboard => {
                    tracing::debug!("Open to be opened keyboard");
                    self.open_keyboard()
                }
                WindowMask::Indicator => {
                    tracing::debug!("Open to be opened indicator");
                    self.open_indicator()
                }
            })
        }
    }

    fn set_to_be_opened(&mut self, window_mask: WindowMask, force: bool) {
        let closed = match window_mask {
            WindowMask::Keyboard => self.keyboard_window_state.id().is_none(),
            WindowMask::Indicator => self.indicator_window_state.id().is_none(),
        };
        // set only if the window is closed
        if closed || force {
            self.to_be_opened_flag |= window_mask as u16;
        }
    }

    fn unset_to_be_opened(&mut self, window_mask: WindowMask) {
        self.to_be_opened_flag &= 0xffff ^ window_mask as u16;
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
                    match self.indicator_display() {
                        IndicatorDisplay::Auto | IndicatorDisplay::AlwaysOff => {
                            if self.indicator_window_state.id().is_some() {
                                task = task.chain(self.close_indicator())
                            }
                        }
                        IndicatorDisplay::AlwaysOn => {
                            if self.indicator_window_state.id().is_none() {
                                task = task.chain(self.open_indicator())
                            }
                        }
                    }
                    // The indicator might need to move up after a docked keyboard is opened
                    if let Some(t) = self
                        .indicator_window_state
                        .fix_position(&mut self.wm, portrait)
                    {
                        task = task.chain(t);
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
                tracing::debug!("Window is to be closed: {id}");
                let window_state = if self.is_keyboard(id) {
                    // If Keyboard is reopening, don't open indicator
                    if ((self.indicator_display() == IndicatorDisplay::Auto
                        && !self.need_opened(WindowMask::Keyboard))
                        || self.indicator_display() == IndicatorDisplay::AlwaysOn)
                        && self.indicator_window_state.id().is_none()
                    {
                        self.set_to_be_opened(WindowMask::Indicator, false);
                    }
                    Some(&mut self.keyboard_window_state)
                } else if self.is_indicator(id) {
                    Some(&mut self.indicator_window_state)
                } else {
                    None
                };
                if let Some(window_state) = window_state {
                    if let Some(snapshot) = snapshot {
                        window_state.close_checked(&mut self.wm, snapshot, source)
                    } else {
                        window_state.close(&mut self.wm, source)
                    }
                } else {
                    Message::from_nothing()
                }
            }
            WindowEvent::Closed(id) => {
                let mut task = self.wm.closed(id);
                if self.is_keyboard(id) {
                    if Some(CloseOpSource::UserAction) == self.keyboard_window_state.set_closed() {
                        task = task.chain(self.fcitx5_hide().map_task());
                    }
                    // check if the keyboard needs to open again
                    if let Some(t) = self.open_to_be_opened(WindowMask::Keyboard) {
                        task = task.chain(t);
                    }
                    // check if the indicator needs to open again
                    if let Some(t) = self.open_to_be_opened(WindowMask::Indicator) {
                        task = task.chain(t);
                    }
                } else if self.is_indicator(id) {
                    self.indicator_window_state.set_closed();
                    // check if the indicator needs to open again
                    if let Some(t) = self.open_to_be_opened(WindowMask::Indicator) {
                        task = task.chain(t);
                    }
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
            WindowManagerEvent::OutputChanged => self.sync_output(),
            WindowManagerEvent::OpenKeyboard => self.open_keyboard(),
            WindowManagerEvent::CloseKeyboard(source) => self.close_keyboard(source),
            WindowManagerEvent::OpenIndicator => self.open_indicator(),
            WindowManagerEvent::UpdateMode(mode) => self.update_mode(mode),
            WindowManagerEvent::UpdatePlacement(placement) => self.update_placement(placement),
            WindowManagerEvent::UpdateIndicatorDisplay(indicator_display) => {
                self.update_indicator_display(indicator_display)
            }
            WindowManagerEvent::UpdateUnit(unit) => self.update_unit(unit),
            WindowManagerEvent::UpdatePreferredOutputName(name) => {
                tracing::debug!("Set preferred name: {name}");
                self.wm.set_preferred_output_name(&name);
                self.sync_output()
            }
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
    pub(super) fn update_fcitx5_services(&mut self, fcitx5_services: Fcitx5Services) {
        self.fcitx5_services = fcitx5_services;
    }

    fn fcitx5_virtual_keyboard_service(&self) -> &Fcitx5VirtualKeyboardServiceExt {
        self.fcitx5_services.virtual_keyboard()
    }

    fn fcitx5_show(&self) -> Task<Message> {
        super::call_dbus(
            self.fcitx5_virtual_keyboard_service(),
            "send show event failed".to_string(),
            |s| async move {
                s.show_virtual_keyboard().await?;
                Ok(Message::Nothing)
            },
        )
    }

    fn fcitx5_hide(&self) -> Task<Message> {
        super::call_dbus(
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
    UpdateMode(WindowManagerMode),
    UpdatePlacement(Placement),
    UpdateIndicatorDisplay(IndicatorDisplay),
    UpdateUnit(u16),
    UpdatePreferredOutputName(String),
    OutputChanged,
}

impl From<WindowManagerEvent> for Message {
    fn from(value: WindowManagerEvent) -> Self {
        Self::WindowManagerEvent(value)
    }
}
