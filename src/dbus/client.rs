use std::collections::HashMap;

use getset::Getters;
use serde::Deserialize;
use tracing::instrument;
use zbus::{proxy, zvariant::OwnedValue, Connection, Result as ZbusResult};
use zvariant::Type;

/// "sssa{sv}a(sssssssbsa{sv})"
#[derive(Debug, Deserialize, Getters, Type)]
pub struct InputMethodGroupInfo {
    #[allow(unused)]
    #[getset(get = "pub")]
    group_name: String,
    #[allow(unused)]
    #[getset(get = "pub")]
    default_input_method: String,
    #[allow(unused)]
    #[getset(get = "pub")]
    default_layout: String,
    _unknon_field1: HashMap<String, OwnedValue>,
    #[getset(get = "pub")]
    input_methods: Vec<InputMethodInfo>,
}

impl InputMethodGroupInfo {
    pub fn into_input_methods(self) -> Vec<InputMethodInfo> {
        let Self { input_methods, .. } = self;
        input_methods
    }
}

/// sssssssbsa{sv}
#[derive(Clone, Debug, Deserialize, Getters, Type)]
pub struct InputMethodInfo {
    #[getset(get = "pub")]
    unique_name: String,
    #[allow(unused)]
    #[getset(get = "pub")]
    name: String,
    #[allow(unused)]
    #[getset(get = "pub")]
    native_name: String,
    #[allow(unused)]
    #[getset(get = "pub")]
    icon: String,
    #[allow(unused)]
    #[getset(get = "pub")]
    label: String,
    #[allow(unused)]
    #[getset(get = "pub")]
    kanguage_code: String,
    #[allow(unused)]
    #[getset(get = "pub")]
    addon: String,
    #[allow(unused)]
    #[getset(get = "pub")]
    is_configurable: bool,
    #[allow(unused)]
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
    fn full_input_method_group_info(&self, name: &str) -> ZbusResult<InputMethodGroupInfo>;

    #[instrument(level = "debug", skip(self), err, ret)]
    fn current_input_method(&self) -> ZbusResult<String>;

    #[zbus(name = "SetCurrentIM")]
    #[instrument(level = "debug", skip(self), err, ret)]
    fn set_current_im(&self, im: &str) -> ZbusResult<()>;
}

#[proxy(
    default_service = "org.fcitx.Fcitx5",
    default_path = "/virtualkeyboard",
    interface = "org.fcitx.Fcitx.VirtualKeyboard1"
)]
pub trait Fcitx5VirtualKeyboardService {
    #[instrument(level = "debug", skip(self), err, ret)]
    fn show_virtual_keyboard(&self) -> ZbusResult<()>;

    #[instrument(level = "debug", skip(self), err, ret)]
    fn hide_virtual_keyboard(&self) -> ZbusResult<()>;

    #[instrument(level = "debug", skip(self), err, ret)]
    fn toggle_virtual_keyboard(&self) -> ZbusResult<()>;
}

#[proxy(
    default_service = "org.fcitx.Fcitx5.VirtualKeyboardBackend",
    default_path = "/virtualkeyboard",
    interface = "org.fcitx.Fcitx5.VirtualKeyboardBackend1"
)]
pub trait Fcitx5VirtualKeyboardBackendService {
    /// keyval(keysym), state: src/lib/fcitx-utils/keysym.h.
    #[instrument(level = "debug", skip(self), err, ret)]
    fn process_key_event(
        &self,
        keyval: u32,
        keycode: u32,
        state: u32,
        is_release: bool,
        time: u32,
    ) -> ZbusResult<()>;

    #[instrument(level = "debug", skip(self), err, ret)]
    fn process_visibility_event(&self, visible: bool) -> ZbusResult<()>;

    #[instrument(level = "debug", skip(self), err, ret)]
    fn select_candidate(&self, index: i32) -> ZbusResult<()>;

    #[instrument(level = "debug", skip(self), err, ret)]
    fn prev_page(&self, index: i32) -> ZbusResult<()>;

    #[instrument(level = "debug", skip(self), err, ret)]
    fn next_page(&self, index: i32) -> ZbusResult<()>;
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
    pub async fn new() -> anyhow::Result<Self> {
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
