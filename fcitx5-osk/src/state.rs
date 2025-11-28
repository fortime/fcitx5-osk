use std::future::Future;

use anyhow::Result;
use dark_light::Mode;
use fcitx5_osk_common::dbus::client::Fcitx5OskServices;
use getset::{Getters, MutGetters};
use iced::{window::Id, Element, Task, Theme, Vector};

use crate::{
    app::{self, MapTask, Message},
    config::{Config, ConfigManager, IndicatorDisplay, Placement},
    dbus::client::Fcitx5Services,
    layout::ToElementCommonParams,
    store::Store,
    window::{WindowManager, WindowManagerMode},
};

mod config;
mod im;
mod keyboard;
mod layout;
mod window;

pub use config::{
    BoolDesc, ConfigState, DynamicEnumDesc, EnumDesc, Field, FieldType, OwnedEnumDesc, StepDesc,
    TextDesc, UpdateConfigEvent,
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
    theme_detecting: bool,
}

impl<WM> State<WM> {
    pub fn new(
        config_manager: ConfigManager,
        wm: WM,
        fcitx5_services: Fcitx5Services,
        fcitx5_osk_services: Fcitx5OskServices,
    ) -> Result<Self> {
        let config = config_manager.as_ref();
        let store = Store::new(config)?;
        let portrait = false;
        // key_area_layout will be updated when cur_im is updated.
        let key_area_layout = store.key_area_layout_by_im("", portrait);
        let mut state = Self {
            keyboard: KeyboardState::new(
                config.holding_timeout(),
                &key_area_layout,
                &store,
                fcitx5_services.clone(),
            ),
            im: ImState::new(fcitx5_services.clone()),
            window_manager: WindowManagerState::new(
                config,
                wm,
                portrait,
                key_area_layout,
                fcitx5_services,
                fcitx5_osk_services,
            )?,
            theme: Default::default(),
            theme_detecting: false,
            config: ConfigState::new(config_manager),
            store,
        };
        state.sync_theme(None);
        Ok(state)
    }

    fn is_auto_theme(&self) -> bool {
        self.config.config().theme().eq_ignore_ascii_case("auto")
    }

    pub fn on_theme_event(&mut self, event: ThemeEvent) -> Task<Message> {
        let mut task = Message::nothing();
        match event {
            ThemeEvent::Detect => {
                if self.is_auto_theme() && !self.theme_detecting {
                    self.theme_detecting = true;
                    // detect may block the eventloop, run it in a task.
                    task =
                        Task::future(async { ThemeEvent::Detected(dark_light::detect()).into() });
                }
            }
            ThemeEvent::Detected(mode) => {
                if self.is_auto_theme() {
                    self.sync_theme(Some(mode));
                }
                self.theme_detecting = false;
            }
            ThemeEvent::Updated => {
                self.sync_theme(None);
            }
        }
        task
    }

    fn sync_theme(&mut self, mode: Option<Mode>) {
        let mode = mode.unwrap_or_else(|| {
            if self.is_auto_theme() {
                // it may block the eventloop.
                dark_light::detect()
            } else {
                Mode::Default
            }
        });
        let config = self.config.config();
        let mut default_theme = Default::default();
        let theme = if !self.is_auto_theme() {
            self.store.theme(config.theme())
        } else {
            match mode {
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

    pub fn update_fcitx5_services(&mut self, fcitx5_services: Fcitx5Services) {
        self.im.update_fcitx5_services(fcitx5_services.clone());
        self.keyboard
            .update_fcitx5_services(fcitx5_services.clone());
        self.window_manager.update_fcitx5_services(fcitx5_services);
    }
}

impl<WM> State<WM>
where
    WM: WindowManager,
{
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
        self.config.on_update_event(event).map_task()
    }

    fn update_layout_by_im(&mut self, im_name: Option<&str>) -> Option<Task<WM::Message>> {
        let im_name = im_name
            .or_else(|| self.im.im_name().map(String::as_str))
            .unwrap_or_default();
        let portrait = self.window_manager.is_portrait();
        let key_area_layout = self
            .store
            .key_area_layout_by_im(im_name, self.window_manager.is_portrait());
        let max_width = if portrait {
            self.config().portrait_width()
        } else {
            self.config().landscape_width()
        };
        let task = self
            .window_manager
            .update_key_area_layout(max_width, key_area_layout.clone());
        if task.is_some() {
            self.keyboard
                .update_key_area_layout(&key_area_layout, &self.store);
        }
        task
    }

    fn update_cur_im(&mut self, im_name: &str) -> Task<WM::Message> {
        if self.im.im_name().filter(|n| *n == im_name).is_some() {
            // Don't update
            return Message::from_nothing();
        }
        if let Some(task) = self.update_layout_by_im(Some(im_name)) {
            self.im.update_cur_im(im_name);
            self.window_manager
                .update_candidate_font(self.store.font_by_im(im_name));
            task
        } else {
            Message::from_nothing()
        }
    }

    pub fn on_im_event(&mut self, event: ImEvent) -> Task<WM::Message> {
        match event {
            ImEvent::UpdateCurrentIm(im) => self.update_cur_im(&im),
            // make sure virtual keyboard mode of fcitx5 is activated
            ImEvent::SelectIm(_) => self
                .keyboard_mut()
                .clear_fcitx5_hidden()
                .chain(self.im.on_event(event))
                .map_task(),
            _ => self.im.on_event(event).map_task(),
        }
    }

    pub fn on_layout_event(&mut self, event: LayoutEvent) -> Task<WM::Message> {
        let task = if let LayoutEvent::SyncLayout = event {
            self.update_layout_by_im(None)
        } else {
            self.window_manager.on_layout_event(event);
            if self.window_manager.is_setting_shown() {
                self.config.refresh();
            }
            None
        };
        if let Some(task) = task {
            task
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

    fn theme_names(&self) -> &[String];

    fn config(&self) -> &Config;

    fn updatable_fields(&self) -> &[Field];

    fn available_candidate_width(&self) -> u16;

    fn movable(&self, window_id: Id) -> bool;

    fn scale_factor(&self) -> f32;

    fn unit(&self) -> u16;

    fn new_position_message(&self, id: Id, delta: Vector) -> Option<Message>;

    fn window_manager_mode(&self) -> WindowManagerMode;

    fn placement(&self) -> Placement;

    fn indicator_display(&self) -> IndicatorDisplay;

    fn outputs(&self) -> Vec<(String, String)>;

    /// Return the init value and cur value stored by ChangeTempText event
    fn config_temp_text(&self, key: &str) -> Option<(&str, &str)>;
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

    fn theme_names(&self) -> &[String] {
        self.store.theme_names()
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

    fn window_manager_mode(&self) -> WindowManagerMode {
        self.window_manager.mode()
    }

    fn placement(&self) -> Placement {
        self.window_manager.placement()
    }

    fn indicator_display(&self) -> IndicatorDisplay {
        self.window_manager.indicator_display()
    }

    fn outputs(&self) -> Vec<(String, String)> {
        self.window_manager.outputs()
    }

    /// Return the init value and cur value stored by ChangeTempText event
    fn config_temp_text(&self, key: &str) -> Option<(&str, &str)> {
        self.config.temp_text(key)
    }
}

#[derive(Clone, Debug)]
pub enum ThemeEvent {
    Detect,
    Detected(Mode),
    Updated,
}

impl From<ThemeEvent> for Message {
    fn from(value: ThemeEvent) -> Self {
        Self::ThemeEvent(value)
    }
}

fn call_dbus<S, M, FN, F>(service: &S, err_msg: M, f: FN) -> Task<Message>
where
    S: Clone,
    M: Into<String>,
    FN: FnOnce(S) -> F,
    F: Future<Output = Result<Message>> + 'static + Send,
{
    let err_msg = err_msg.into();
    let service = service.clone();
    Task::perform(f(service), move |r| match r {
        Err(e) => app::error_with_context(e, err_msg.clone()),
        Ok(t) => t,
    })
}
