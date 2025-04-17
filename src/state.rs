use std::{future::Future, sync::Arc};

use anyhow::{Error, Result};
use dark_light::Mode;
use getset::{Getters, MutGetters};
use iced::{window::Id, Element, Task, Theme, Vector};
use zbus::Result as ZbusResult;

use crate::{
    app::{KeyboardError, Message},
    config::{Config, ConfigManager},
    dbus::client::Fcitx5Services,
    layout::ToElementCommonParams,
    store::Store,
    window::WindowManager,
};

mod config;
mod im;
mod keyboard;
mod layout;
mod window;

pub use config::{
    ConfigState, EnumDesc, Field, FieldType, OwnedEnumDesc, StepDesc, UpdateConfigEvent,
};
pub use im::{ImEvent, ImState};
pub use keyboard::{KeyEvent, KeyboardEvent, KeyboardState};
pub use layout::{LayoutEvent, LayoutState};
pub use window::{CloseOpSource, WindowEvent, WindowManagerEvent, WindowManagerState};

#[derive(Getters, MutGetters)]
pub struct State<WM> {
    config: ConfigState,
    #[getset(get_mut = "pub")]
    store: Store,
    #[getset(get_mut = "pub")]
    keyboard: KeyboardState,
    #[getset(get_mut = "pub")]
    im: ImState,
    #[getset(get = "pub", get_mut = "pub")]
    window_manager: WindowManagerState<WM>,
    theme: Theme,
    has_fcitx5_services: bool,
}

impl<WM> State<WM>
where
    WM: Default,
{
    pub fn new(config_manager: ConfigManager) -> Result<Self> {
        let config = config_manager.as_ref();
        let store = Store::new(config)?;
        // key_area_layout will be updated when cur_im is updated.
        let key_area_layout = store.key_area_layout("");
        let mut state = Self {
            keyboard: KeyboardState::new(config.holding_timeout(), &key_area_layout, &store),
            im: Default::default(),
            window_manager: WindowManagerState::new(config, key_area_layout)?,
            theme: Default::default(),
            has_fcitx5_services: false,
            config: ConfigState::new(config_manager),
            store,
        };
        state.sync_theme();
        Ok(state)
    }
}

impl<WM> State<WM> {
    pub fn start(&mut self) -> Task<Message> {
        if self.has_fcitx5_services {
            Task::done(Message::Nothing)
        } else {
            Task::perform(Fcitx5Services::new(), |res: ZbusResult<_>| match res {
                Ok(services) => StartEvent::StartedDbusClients(services).into(),
                Err(e) => fatal_with_context(e, "failed to create dbus clients"),
            })
        }
    }

    pub fn set_dbus_clients(&mut self, fcitx5_services: Fcitx5Services) -> bool {
        if self.has_fcitx5_services {
            false
        } else {
            self.keyboard.set_dbus_clients(fcitx5_services.clone());
            self.im.set_dbus_clients(fcitx5_services.clone());
            self.window_manager
                .set_dbus_clients(fcitx5_services.clone());
            self.has_fcitx5_services = true;
            true
        }
    }

    fn update_cur_im(&mut self, im_name: &str) -> bool {
        let key_area_layout = self.store.key_area_layout_by_im(im_name);
        let res = self
            .window_manager
            .update_key_area_layout(key_area_layout.clone());
        if res {
            self.keyboard
                .update_key_area_layout(&key_area_layout, &self.store);
            self.im.update_cur_im(im_name);
            self.window_manager
                .update_candidate_font(self.store.font_by_im(im_name));
        }
        res
    }

    pub fn on_im_event(&mut self, event: ImEvent) -> Task<Message> {
        match event {
            ImEvent::UpdateCurrentIm(im) => {
                self.update_cur_im(&im);
                Message::nothing()
            }
            // make sure virtual keyboard mode of fcitx5 is activated
            ImEvent::SelectIm(_) => self
                .keyboard_mut()
                .clear_fcitx5_hidden()
                .chain(self.im.on_event(event)),
            _ => self.im.on_event(event),
        }
    }

    fn is_auto_theme(&self) -> bool {
        self.config.config().theme().eq_ignore_ascii_case("auto")
    }

    pub fn on_theme_event(&mut self, event: ThemeEvent) {
        match event {
            ThemeEvent::Detect => {
                if self.is_auto_theme() {
                    self.sync_theme();
                }
            }
            ThemeEvent::Updated => {
                self.sync_theme();
            }
        }
    }

    fn sync_theme(&mut self) {
        let config = self.config.config();
        let mut default_theme = Default::default();
        let theme = if !self.is_auto_theme() {
            self.store.theme(config.theme())
        } else {
            match dark_light::detect() {
                Mode::Dark => {
                    default_theme = Theme::Dark;
                    config.dark_theme().and_then(|t| self.store.theme(t))
                }
                Mode::Light | Mode::Default => {
                    default_theme = Theme::Light;
                    config.light_theme().and_then(|t| self.store.theme(t))
                }
            }
        };
        self.theme = theme.cloned().unwrap_or(default_theme);
    }
}

impl<WM> State<WM>
where
    WM: WindowManager,
{
    pub fn on_layout_event(&mut self, event: LayoutEvent) {
        self.window_manager.on_layout_event(event);
        if self.window_manager.is_setting_shown() {
            self.config.refresh();
        }
    }

    pub fn to_element(&self, id: Id) -> Element<Message> {
        self.window_manager.to_element(ToElementCommonParams {
            state: self,
            window_id: id,
        })
    }
}

impl<WM> State<WM>
where
    WM: WindowManager,
    WM::Message: From<Message> + 'static + Send + Sync,
{
    pub fn on_update_config_event(&mut self, event: UpdateConfigEvent) -> Task<WM::Message> {
        if self.config.on_update_event(event.clone()) {
            match event {
                UpdateConfigEvent::Theme(_) => {
                    Task::done(Message::from(ThemeEvent::Updated).into())
                }
                UpdateConfigEvent::LandscapeWidth(_) | UpdateConfigEvent::PortraitWidth(_) => {
                    // After width is changed, the pages of candidate area should be changed too. Here we
                    // just reset it.
                    Task::done(Message::from(ImEvent::ResetCandidateCursor).into())
                }
                _ => Message::from_nothing(),
            }
        } else {
            Message::from_nothing()
        }
    }
}

/// Use dyn for reducing the need for writing generic type.
pub trait StateExtractor {
    fn store(&self) -> &Store;

    fn keyboard(&self) -> &KeyboardState;

    fn im(&self) -> &ImState;

    fn theme(&self) -> &Theme;

    fn config(&self) -> &Config;

    fn updatable_fields(&self) -> &[Field];

    fn available_candidate_width(&self) -> u16;

    fn movable(&self, window_id: Id) -> bool;

    fn scale_factor(&self) -> f32;

    fn unit(&self) -> u16;

    fn new_position_message(&self, id: Id, delta: Vector) -> Option<Message>;
}

impl<WM> StateExtractor for State<WM>
where
    WM: WindowManager,
{
    fn store(&self) -> &Store {
        &self.store
    }

    fn keyboard(&self) -> &KeyboardState {
        &self.keyboard
    }

    fn im(&self) -> &ImState {
        &self.im
    }

    fn theme(&self) -> &Theme {
        &self.theme
    }

    fn config(&self) -> &Config {
        self.config.config()
    }

    fn updatable_fields(&self) -> &[Field] {
        self.config.updatable_fields()
    }

    fn available_candidate_width(&self) -> u16 {
        self.window_manager.available_candidate_width()
    }

    fn movable(&self, window_id: Id) -> bool {
        self.window_manager.movable(window_id)
    }

    fn scale_factor(&self) -> f32 {
        self.window_manager.scale_factor()
    }

    fn unit(&self) -> u16 {
        self.window_manager.unit()
    }

    fn new_position_message(&self, id: Id, delta: Vector) -> Option<Message> {
        self.window_manager
            .position(id)
            .map(|p| Message::from(WindowEvent::Move(id, p + delta)))
    }
}

#[derive(Clone, Debug)]
pub enum StartEvent {
    Start,
    StartedDbusClients(Fcitx5Services),
}

impl From<StartEvent> for Message {
    fn from(value: StartEvent) -> Self {
        Self::StartEvent(value)
    }
}

#[derive(Clone, Debug)]
pub enum ThemeEvent {
    Detect,
    Updated,
}

impl From<ThemeEvent> for Message {
    fn from(value: ThemeEvent) -> Self {
        Self::ThemeEvent(value)
    }
}

fn call_fcitx5<S, M, FN, F>(service: Option<&S>, err_msg: M, f: FN) -> Task<Message>
where
    S: Clone,
    M: Into<String>,
    FN: FnOnce(S) -> F,
    F: Future<Output = Result<Message>> + 'static + Send,
{
    let err_msg = err_msg.into();
    if let Some(service) = service {
        let service = service.clone();
        Task::perform(f(service), move |r| match r {
            Err(e) => error_with_context(e, err_msg.clone()),
            Ok(t) => t,
        })
    } else {
        Task::done(fatal(anyhow::anyhow!(
            "dbus client hasn't been initialized"
        )))
    }
}

fn _error<E>(e: E) -> Message
where
    E: Into<Error>,
{
    KeyboardError::Error(Arc::new(e.into())).into()
}

fn error_with_context<E, M>(e: E, err_msg: M) -> Message
where
    E: Into<Error>,
    M: Into<String>,
{
    KeyboardError::Error(Arc::new(e.into().context(err_msg.into()))).into()
}

fn fatal<E>(e: E) -> Message
where
    E: Into<Error>,
{
    KeyboardError::Fatal(Arc::new(e.into())).into()
}

fn fatal_with_context<E, M>(e: E, err_msg: M) -> Message
where
    E: Into<Error>,
    M: Into<String>,
{
    KeyboardError::Fatal(Arc::new(e.into().context(err_msg.into()))).into()
}
