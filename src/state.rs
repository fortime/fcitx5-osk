use std::{future::Future, sync::Arc};

use anyhow::{Error, Result};
use dark_light::Mode;
use getset::{Getters, MutGetters};
use iced::{window::Id, Element, Task, Theme};
use zbus::Result as ZbusResult;

use crate::{
    app::{KeyboardError, Message},
    config::{Config, ConfigManager},
    dbus::client::Fcitx5Services,
    layout::KeyboardManager,
    store::Store,
    window::WindowManager,
};

mod im;
mod keyboard;
mod layout;
mod window;

pub use im::{CandidateAreaState, ImEvent, ImState};
pub use keyboard::{KeyEvent, KeyboardState, StartDbusServiceEvent};
pub use layout::{LayoutEvent, LayoutState};
pub use window::{CloseOpSource, WindowEvent, WindowManagerEvent, WindowManagerState};

#[derive(Getters, MutGetters)]
pub struct State<WM> {
    #[getset(get = "pub", get_mut = "pub")]
    config_manager: ConfigManager,
    #[getset(get = "pub", get_mut = "pub")]
    store: Store,
    #[getset(get = "pub", get_mut = "pub")]
    keyboard: KeyboardState,
    #[getset(get = "pub", get_mut = "pub")]
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
            config_manager,
            store,
        };
        state.sync_theme();
        Ok(state)
    }
}

impl<WM> State<WM> {
    pub fn start(&mut self) -> Task<Message> {
        if self.has_fcitx5_services {
            Task::none()
        } else {
            Task::perform(Fcitx5Services::new(), |res: ZbusResult<_>| match res {
                Ok(services) => StartedEvent::StartedDbusClients(services).into(),
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
                Task::none()
            }
            _ => self.im.on_event(event),
        }
    }

    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    fn is_auto_theme(&self) -> bool {
        self.config().theme().eq_ignore_ascii_case("auto")
    }

    pub fn on_theme_event(&mut self, event: ThemeEvent) {
        match event {
            ThemeEvent::Detect => {
                if self.is_auto_theme() {
                    self.sync_theme();
                }
            }
            ThemeEvent::Update(theme) => {
                let config = self.config_manager.as_mut();
                if theme != *config.theme() {
                    config.set_theme(theme);
                    self.config_manager.try_write();
                    self.sync_theme();
                }
            }
        }
    }

    fn sync_theme(&mut self) {
        let config = self.config();
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

    pub fn config(&self) -> &Config {
        self.config_manager.as_ref()
    }
}

impl<WM> State<WM>
where
    WM: WindowManager,
    WM::Message: From<Message> + 'static + Send + Sync,
{
    //pub fn update_width(&mut self, id: Id, width_p: u16, scale_factor: f32) -> Task<WM::Message> {
    //    if self.layout.update_width(width_p, scale_factor) {
    //        if width_p != self.config_manager.as_ref().width() {
    //            self.config_manager.as_mut().set_width(width_p);
    //            self.config_manager.try_write();
    //        }
    //        let size = self.layout.size();
    //        if !self.window.wm_inited() {
    //            self.window.set_wm_inited(id)
    //        }
    //        // After width is changed, the pages of candidate area should be changed too. Here we
    //        // just reset it.
    //        self.im.reset_candidate_cursor();
    //        return self.window.resize(size);
    //    } else {
    //        Task::none()
    //    }
    //}

    pub fn to_element(&self, id: Id) -> Element<Message> {
        self.window_manager.to_element(
            id,
            self.im.candidate_area_state(),
            self,
            &self.keyboard,
            &self.theme,
        )
    }
}

impl<WM> KeyboardManager for State<WM> {
    type Message = Message;

    fn available_candidate_width_p(&self) -> u16 {
        self.window_manager.available_candidate_width_p()
    }

    fn themes(&self) -> &[String] {
        self.store.theme_names()
    }

    fn selected_theme(&self) -> &String {
        self.config().theme()
    }

    fn select_theme(&self, theme: &String) -> Self::Message {
        ThemeEvent::Update(theme.clone()).into()
    }

    fn ims(&self) -> &[String] {
        self.im.im_names()
    }

    fn selected_im(&self) -> Option<&String> {
        self.im.im_name()
    }

    fn select_im(&self, im: &String) -> Self::Message {
        ImEvent::SelectIm(im.clone()).into()
    }

    fn toggle_setting(&self) -> Self::Message {
        LayoutEvent::ToggleSetting.into()
    }

    fn prev_candidates_message(&self) -> Self::Message {
        ImEvent::PrevCandidates.into()
    }

    fn next_candidates_message(&self, cursor: usize) -> Self::Message {
        ImEvent::NextCandidates(cursor).into()
    }

    fn select_candidate_message(&self, index: usize) -> Self::Message {
        ImEvent::SelectCandidate(index).into()
    }

    fn open_keyboard(&self) -> Self::Message {
        WindowManagerEvent::OpenKeyboard.into()
    }

    fn close_keyboard(&self) -> Self::Message {
        WindowManagerEvent::CloseKeyboard(CloseOpSource::UserAction).into()
    }
}

#[derive(Clone, Debug)]
pub enum StartedEvent {
    StartedDbusClients(Fcitx5Services),
}

impl From<StartedEvent> for Message {
    fn from(value: StartedEvent) -> Self {
        Self::Started(value)
    }
}

#[derive(Clone, Debug)]
pub enum ThemeEvent {
    Detect,
    Update(String),
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
