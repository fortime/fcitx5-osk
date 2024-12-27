use std::collections::HashMap;

use getset::Getters;
use serde::Deserialize;
use tracing::instrument;
use zbus::{proxy, zvariant::OwnedValue, Connection, Result};
use zvariant::Type;

/// "sssa{sv}a(sssssssbsa{sv})"
#[derive(Debug, Deserialize, Getters, Type)]
pub struct InputMethodGroupInfo {
    #[getset(get = "pub")]
    group_name: String,
    #[getset(get = "pub")]
    default_input_method: String,
    #[getset(get = "pub")]
    default_layout: String,
    _unknon_field1: HashMap<String, OwnedValue>,
    #[getset(get = "pub")]
    input_methods: Vec<InputMethodInfo>,
}

impl InputMethodGroupInfo {
    pub fn into_input_methods(self) -> Vec<InputMethodInfo> {
        let Self {
            input_methods,
            ..
        } = self;
        input_methods
    }
}

/// sssssssbsa{sv}
#[derive(Clone, Debug, Deserialize, Getters, Type)]
pub struct InputMethodInfo {
    #[getset(get = "pub")]
    unique_name: String,
    #[getset(get = "pub")]
    name: String,
    #[getset(get = "pub")]
    native_name: String,
    #[getset(get = "pub")]
    icon: String,
    #[getset(get = "pub")]
    label: String,
    #[getset(get = "pub")]
    language_code: String,
    #[getset(get = "pub")]
    addon: String,
    #[getset(get = "pub")]
    is_configurable: bool,
    #[getset(get = "pub")]
    layout: String,
    _unknon_field1: HashMap<String, OwnedValue>,
}

#[proxy(
    default_service = "org.fcitx.Fcitx5",
    default_path = "/controller",
    interface = "org.fcitx.Fcitx.Controller1"
)]
pub trait Fcitx5ControllerService {
    #[instrument(level = "debug", skip(self), err, ret)]
    fn full_input_method_group_info(&self, name: &str) -> Result<InputMethodGroupInfo>;

    #[instrument(level = "debug", skip(self), err, ret)]
    fn current_input_method(&self) -> Result<String>;
}

#[proxy(
    default_service = "org.fcitx.Fcitx5",
    default_path = "/virtualkeyboard",
    interface = "org.fcitx.Fcitx.VirtualKeyboard1"
)]
pub trait Fcitx5VirtualKeyboardService {
    #[instrument(level = "debug", skip(self), err, ret)]
    fn show_virtual_keyboard(&self) -> Result<()>;

    #[instrument(level = "debug", skip(self), err, ret)]
    fn hide_virtual_keyboard(&self) -> Result<()>;

    #[instrument(level = "debug", skip(self), err, ret)]
    fn toggle_virtual_keyboard(&self) -> Result<()>;
}

#[proxy(
    default_service = "org.fcitx.Fcitx5.VirtualKeyboardBackend",
    default_path = "/virtualkeyboard",
    interface = "org.fcitx.Fcitx5.VirtualKeyboardBackend1"
)]
pub trait Fcitx5VirtualKeyboardBackendService {
    #[instrument(level = "debug", skip(self), err, ret)]
    fn set_virtual_keyboard_function_mode(&self, mode: u32) -> Result<()>;

    /// keyval(keysym), state: src/lib/fcitx-utils/keysym.h.
    /// use keyval + state or keycode.
    #[instrument(level = "debug", skip(self), err, ret)]
    fn process_key_event(
        &self,
        keyval: u32,
        keycode: u32,
        state: u32,
        is_release: bool,
        time: u32,
    ) -> Result<()>;

    #[instrument(level = "debug", skip(self), err, ret)]
    fn process_visibility_event(&self, visible: bool) -> Result<()>;

    #[instrument(level = "debug", skip(self), err, ret)]
    fn select_candidate(&self, index: i32) -> Result<()>;

    #[instrument(level = "debug", skip(self), err, ret)]
    fn prev_page(&self, index: i32) -> Result<()>;

    #[instrument(level = "debug", skip(self), err, ret)]
    fn next_page(&self, index: i32) -> Result<()>;
}

#[derive(Clone, Debug, Getters)]
pub struct Fcitx5Services {
    #[getset(get = "pub")]
    controller: Fcitx5ControllerServiceProxy<'static>,
    #[getset(get = "pub")]
    virtual_keyboard: Fcitx5VirtualKeyboardServiceProxy<'static>,
    #[getset(get = "pub")]
    virtual_keyboard_backend: Fcitx5VirtualKeyboardBackendServiceProxy<'static>,
}

impl Fcitx5Services {
    pub async fn new() -> Result<Self> {
        let connection = Connection::session().await?;
        let controller = Fcitx5ControllerServiceProxy::new(&connection).await?;
        let virtual_keyboard = Fcitx5VirtualKeyboardServiceProxy::new(&connection).await?;
        let virtual_keyboard_backend =
            Fcitx5VirtualKeyboardBackendServiceProxy::new(&connection).await?;
        Ok(Self {
            controller,
            virtual_keyboard,
            virtual_keyboard_backend,
        })
    }
}
