use std::{collections::HashMap, fmt::Debug, sync::Arc};

use anyhow::Result;
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
    pub fn new(
        group_name: &str,
        default_input_method: usize,
        default_layout: &str,
        input_methods: Vec<InputMethodInfo>,
    ) -> Result<Self> {
        Ok(Self {
            group_name: group_name.to_string(),
            default_input_method: input_methods
                .get(default_input_method)
                .map(InputMethodInfo::unique_name)
                .cloned()
                .ok_or_else(|| {
                    anyhow::anyhow!("can find input method at: {}", default_input_method)
                })?,
            default_layout: default_layout.to_string(),
            _unknon_field1: Default::default(),
            input_methods,
        })
    }

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

impl InputMethodInfo {
    pub fn new(unique_name: &str) -> Self {
        Self {
            unique_name: unique_name.to_string(),
            name: unique_name.to_string(),
            native_name: unique_name.to_string(),
            icon: Default::default(),
            label: Default::default(),
            kanguage_code: Default::default(),
            addon: Default::default(),
            is_configurable: Default::default(),
            layout: Default::default(),
            _unknon_field1: Default::default(),
        }
    }
}

/// make fcitx5 replaceable
#[async_trait::async_trait]
pub trait IFcitx5ControllerService: Debug {
    async fn full_input_method_group_info(&self, name: &str) -> ZbusResult<InputMethodGroupInfo>;

    async fn current_input_method(&self) -> ZbusResult<String>;

    async fn set_current_im(&self, im: &str) -> ZbusResult<()>;
}

#[proxy(
    default_service = "org.fcitx.Fcitx5",
    default_path = "/controller",
    interface = "org.fcitx.Fcitx.Controller1"
)]
trait Fcitx5ControllerService {
    #[instrument(level = "debug", skip(self), err, ret)]
    fn full_input_method_group_info(&self, name: &str) -> ZbusResult<InputMethodGroupInfo>;

    #[instrument(level = "debug", skip(self), err, ret)]
    fn current_input_method(&self) -> ZbusResult<String>;

    #[zbus(name = "SetCurrentIM")]
    #[instrument(level = "debug", skip(self), err, ret)]
    fn set_current_im(&self, im: &str) -> ZbusResult<()>;
}

#[async_trait::async_trait]
impl IFcitx5ControllerService for Fcitx5ControllerServiceProxy<'_> {
    async fn full_input_method_group_info(&self, name: &str) -> ZbusResult<InputMethodGroupInfo> {
        self.full_input_method_group_info(name).await
    }

    async fn current_input_method(&self) -> ZbusResult<String> {
        self.current_input_method().await
    }

    async fn set_current_im(&self, im: &str) -> ZbusResult<()> {
        self.set_current_im(im).await
    }
}

/// make fcitx5 replaceable
#[async_trait::async_trait]
pub trait IFcitx5VirtualKeyboardService: Debug {
    async fn show_virtual_keyboard(&self) -> ZbusResult<()>;

    async fn hide_virtual_keyboard(&self) -> ZbusResult<()>;
}

#[proxy(
    default_service = "org.fcitx.Fcitx5",
    default_path = "/virtualkeyboard",
    interface = "org.fcitx.Fcitx.VirtualKeyboard1"
)]
trait Fcitx5VirtualKeyboardService {
    #[instrument(level = "debug", skip(self), err, ret)]
    fn show_virtual_keyboard(&self) -> ZbusResult<()>;

    #[instrument(level = "debug", skip(self), err, ret)]
    fn hide_virtual_keyboard(&self) -> ZbusResult<()>;
}

#[async_trait::async_trait]
impl IFcitx5VirtualKeyboardService for Fcitx5VirtualKeyboardServiceProxy<'_> {
    async fn show_virtual_keyboard(&self) -> ZbusResult<()> {
        self.show_virtual_keyboard().await
    }

    async fn hide_virtual_keyboard(&self) -> ZbusResult<()> {
        self.hide_virtual_keyboard().await
    }
}

/// make fcitx5 replaceable
#[async_trait::async_trait]
pub trait IFcitx5VirtualKeyboardBackendService: Debug {
    async fn process_key_event(
        &self,
        keyval: u32,
        keycode: u32,
        state: u32,
        is_release: bool,
        time: u32,
    ) -> ZbusResult<()>;

    async fn select_candidate(&self, index: i32) -> ZbusResult<()>;

    async fn prev_page(&self, index: i32) -> ZbusResult<()>;

    async fn next_page(&self, index: i32) -> ZbusResult<()>;
}

#[proxy(
    default_service = "org.fcitx.Fcitx5.VirtualKeyboardBackend",
    default_path = "/virtualkeyboard",
    interface = "org.fcitx.Fcitx5.VirtualKeyboardBackend1"
)]
trait Fcitx5VirtualKeyboardBackendService {
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
    fn select_candidate(&self, index: i32) -> ZbusResult<()>;

    #[instrument(level = "debug", skip(self), err, ret)]
    fn prev_page(&self, index: i32) -> ZbusResult<()>;

    #[instrument(level = "debug", skip(self), err, ret)]
    fn next_page(&self, index: i32) -> ZbusResult<()>;
}

#[async_trait::async_trait]
impl IFcitx5VirtualKeyboardBackendService for Fcitx5VirtualKeyboardBackendServiceProxy<'_> {
    async fn process_key_event(
        &self,
        keyval: u32,
        keycode: u32,
        state: u32,
        is_release: bool,
        time: u32,
    ) -> ZbusResult<()> {
        self.process_key_event(keyval, keycode, state, is_release, time)
            .await
    }

    async fn select_candidate(&self, index: i32) -> ZbusResult<()> {
        self.select_candidate(index).await
    }

    async fn prev_page(&self, index: i32) -> ZbusResult<()> {
        self.prev_page(index).await
    }

    async fn next_page(&self, index: i32) -> ZbusResult<()> {
        self.next_page(index).await
    }
}

#[derive(Clone, Debug, Getters)]
pub struct Fcitx5Services {
    #[getset(get = "pub")]
    controller: Arc<dyn IFcitx5ControllerService + Send + Sync>,
    #[getset(get = "pub")]
    virtual_keyboard: Arc<dyn IFcitx5VirtualKeyboardService + Send + Sync>,
    #[getset(get = "pub")]
    virtual_keyboard_backend: Arc<dyn IFcitx5VirtualKeyboardBackendService + Send + Sync>,
}

impl Fcitx5Services {
    pub async fn new() -> Result<Self> {
        let connection = Connection::session().await?;
        let controller = Fcitx5ControllerServiceProxy::new(&connection).await?;
        let virtual_keyboard = Fcitx5VirtualKeyboardServiceProxy::new(&connection).await?;
        let virtual_keyboard_backend =
            Fcitx5VirtualKeyboardBackendServiceProxy::new(&connection).await?;
        Ok(Self {
            controller: Arc::new(controller),
            virtual_keyboard: Arc::new(virtual_keyboard),
            virtual_keyboard_backend: Arc::new(virtual_keyboard_backend),
        })
    }

    pub fn new_with(
        controller: Arc<dyn IFcitx5ControllerService + Send + Sync>,
        virtual_keyboard: Arc<dyn IFcitx5VirtualKeyboardService + Send + Sync>,
        virtual_keyboard_backend: Arc<dyn IFcitx5VirtualKeyboardBackendService + Send + Sync>,
    ) -> Self {
        Self {
            controller,
            virtual_keyboard,
            virtual_keyboard_backend,
        }
    }

    //pub fn new_with<C, VK, VKB>(
    //    controller: C,
    //    virtual_keyboard: VK,
    //    virtual_keyboard_backend: VKB,
    //) -> Self
    //where
    //    C: 'static + IFcitx5ControllerService + Send + Sync,
    //    VK: 'static + IFcitx5VirtualKeyboardService + Send + Sync,
    //    VKB: 'static + IFcitx5VirtualKeyboardBackendService + Send + Sync,
    //{
    //    Self {
    //        controller: Arc::new(controller),
    //        virtual_keyboard: Arc::new(virtual_keyboard),
    //        virtual_keyboard_backend: Arc::new(virtual_keyboard_backend),
    //    }
    //}
}
