use std::{
    collections::HashMap,
    fmt::{Debug, Formatter, Result as FmtResult},
    mem::MaybeUninit,
    sync::Arc,
};

use anyhow::Result;
use fcitx5_osk_common::dbus::client::Fcitx5OskKeyHelperControllerServiceProxy;
use getset::Getters;
use iced::futures::lock::Mutex as IcedFuturesMutex;
use serde::Deserialize;
use zbus::{zvariant::OwnedValue, Connection, Result as ZbusResult};
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
    _unknown_field1: HashMap<String, OwnedValue>,
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
            _unknown_field1: Default::default(),
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
    _unknown_field1: HashMap<String, OwnedValue>,
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
            _unknown_field1: Default::default(),
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

#[zbus::proxy(
    default_service = "org.fcitx.Fcitx5",
    default_path = "/controller",
    interface = "org.fcitx.Fcitx.Controller1"
)]
trait Fcitx5ControllerService {
    #[tracing::instrument(level = "debug", skip(self), err, ret)]
    fn full_input_method_group_info(&self, name: &str) -> ZbusResult<InputMethodGroupInfo>;

    #[tracing::instrument(level = "debug", skip(self), err, ret)]
    fn current_input_method(&self) -> ZbusResult<String>;

    #[zbus(name = "SetCurrentIM")]
    #[tracing::instrument(level = "debug", skip(self), err, ret)]
    fn set_current_im(&self, im: &str) -> ZbusResult<()>;
}

#[async_trait::async_trait]
impl IFcitx5ControllerService for Fcitx5ControllerServiceProxy<'_> {
    async fn full_input_method_group_info(&self, name: &str) -> ZbusResult<InputMethodGroupInfo> {
        Fcitx5ControllerServiceProxy::full_input_method_group_info(self, name).await
    }

    async fn current_input_method(&self) -> ZbusResult<String> {
        Fcitx5ControllerServiceProxy::current_input_method(self).await
    }

    async fn set_current_im(&self, im: &str) -> ZbusResult<()> {
        Fcitx5ControllerServiceProxy::set_current_im(self, im).await
    }
}

/// make fcitx5 replaceable
#[async_trait::async_trait]
pub trait IFcitx5VirtualKeyboardService: Debug {
    async fn show_virtual_keyboard(&self) -> ZbusResult<()>;

    async fn hide_virtual_keyboard(&self) -> ZbusResult<()>;
}

#[zbus::proxy(
    default_service = "org.fcitx.Fcitx5",
    default_path = "/virtualkeyboard",
    interface = "org.fcitx.Fcitx.VirtualKeyboard1"
)]
trait Fcitx5VirtualKeyboardService {
    #[tracing::instrument(level = "debug", skip(self), err, ret)]
    fn show_virtual_keyboard(&self) -> ZbusResult<()>;

    #[tracing::instrument(level = "debug", skip(self), err, ret)]
    fn hide_virtual_keyboard(&self) -> ZbusResult<()>;
}

#[async_trait::async_trait]
impl IFcitx5VirtualKeyboardService for Fcitx5VirtualKeyboardServiceProxy<'_> {
    async fn show_virtual_keyboard(&self) -> ZbusResult<()> {
        Fcitx5VirtualKeyboardServiceProxy::show_virtual_keyboard(self).await
    }

    async fn hide_virtual_keyboard(&self) -> ZbusResult<()> {
        Fcitx5VirtualKeyboardServiceProxy::hide_virtual_keyboard(self).await
    }
}

/// make fcitx5 replaceable
#[async_trait::async_trait]
pub trait IFcitx5VirtualKeyboardBackendService: Debug {
    async fn process_key_event(
        &mut self,
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

#[zbus::proxy(
    default_service = "org.fcitx.Fcitx5.VirtualKeyboardBackend",
    default_path = "/virtualkeyboard",
    interface = "org.fcitx.Fcitx5.VirtualKeyboardBackend1"
)]
trait Fcitx5VirtualKeyboardBackendService {
    /// keyval(keysym), state: src/lib/fcitx-utils/keysym.h.
    #[tracing::instrument(level = "debug", skip(self), err, ret)]
    fn process_key_event(
        &self,
        keyval: u32,
        keycode: u32,
        state: u32,
        is_release: bool,
        time: u32,
    ) -> ZbusResult<()>;

    #[tracing::instrument(level = "debug", skip(self), err, ret)]
    fn select_candidate(&self, index: i32) -> ZbusResult<()>;

    #[tracing::instrument(level = "debug", skip(self), err, ret)]
    fn prev_page(&self, index: i32) -> ZbusResult<()>;

    #[tracing::instrument(level = "debug", skip(self), err, ret)]
    fn next_page(&self, index: i32) -> ZbusResult<()>;
}

#[async_trait::async_trait]
impl IFcitx5VirtualKeyboardBackendService for Fcitx5VirtualKeyboardBackendServiceProxy<'_> {
    async fn process_key_event(
        &mut self,
        keyval: u32,
        keycode: u32,
        state: u32,
        is_release: bool,
        time: u32,
    ) -> ZbusResult<()> {
        Fcitx5VirtualKeyboardBackendServiceProxy::process_key_event(
            self, keyval, keycode, state, is_release, time,
        )
        .await
    }

    async fn select_candidate(&self, index: i32) -> ZbusResult<()> {
        Fcitx5VirtualKeyboardBackendServiceProxy::select_candidate(self, index).await
    }

    async fn prev_page(&self, index: i32) -> ZbusResult<()> {
        Fcitx5VirtualKeyboardBackendServiceProxy::prev_page(self, index).await
    }

    async fn next_page(&self, index: i32) -> ZbusResult<()> {
        Fcitx5VirtualKeyboardBackendServiceProxy::next_page(self, index).await
    }
}

struct FusedFcitx5VirtualKeyboardBackendService<'a> {
    virtual_keyboard: Arc<dyn IFcitx5VirtualKeyboardService + Send + Sync>,
    virtual_keyboard_backend: Fcitx5VirtualKeyboardBackendServiceProxy<'a>,
    key_helper_serial: Option<u64>,
    key_helper_created: bool,
    /// Avoid created key helper in the login screen
    fcitx5_osk_key_helper_controller: MaybeUninit<Fcitx5OskKeyHelperControllerServiceProxy<'a>>,
    modifier_workaround_keycodes: Vec<u16>,
}

impl Debug for FusedFcitx5VirtualKeyboardBackendService<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.write_fmt(
            format_args!(
                "FusedFcitx5VirtualKeyboardBackendService {{ key_helper_created: {:?}, modifier_workaround_keycodes: {:?} }}",
                self.key_helper_created,
                self.modifier_workaround_keycodes
            )
        )
    }
}

impl Drop for FusedFcitx5VirtualKeyboardBackendService<'_> {
    fn drop(&mut self) {
        if self.key_helper_created {
            // SAFETY `self.key_helper_created` will be true only if
            // fcitx5_osk_key_helper_controller is inited
            unsafe {
                self.fcitx5_osk_key_helper_controller.assume_init_drop();
            }
        }
    }
}

#[async_trait::async_trait]
impl IFcitx5VirtualKeyboardBackendService for FusedFcitx5VirtualKeyboardBackendService<'_> {
    #[tracing::instrument(level = "debug", skip(self), err, ret)]
    async fn process_key_event(
        &mut self,
        keyval: u32,
        keycode: u32,
        state: u32,
        is_release: bool,
        time: u32,
    ) -> ZbusResult<()> {
        let code = keycode as u16;
        if self.modifier_workaround_keycodes.contains(&code) {
            let created = self.key_helper_created;
            if !self.key_helper_created {
                let sys_connection = Connection::system().await?;
                let fcitx5_osk_key_helper_controller =
                    Fcitx5OskKeyHelperControllerServiceProxy::new(&sys_connection).await?;
                self.fcitx5_osk_key_helper_controller
                    .write(fcitx5_osk_key_helper_controller);
                self.key_helper_created = true;
            }
            // SAFETY `self.fcitx5_osk_key_helper_controller` will be initialized if not
            let fcitx5_osk_key_helper_controller =
                unsafe { self.fcitx5_osk_key_helper_controller.assume_init_ref() };
            let key_helper_serial = if let Some(serial) = self.key_helper_serial.take() {
                serial
            } else {
                if created {
                    tracing::error!(
                        "fcitx5_osk_key_helper_controller serial is missing after created"
                    );
                }
                fcitx5_osk_key_helper_controller.reset_serial().await?
            };
            self.key_helper_serial = Some(
                fcitx5_osk_key_helper_controller
                    .process_key_event(key_helper_serial, code, is_release)
                    .await?,
            );
            // fcitx5 will hide virtual keyboard, send a show request, otherwise, other key events
            // will be ignored
            self.virtual_keyboard.show_virtual_keyboard().await?;
            Ok(())
        } else {
            self.virtual_keyboard_backend
                .process_key_event(keyval, keycode, state, is_release, time)
                .await
        }
    }

    async fn select_candidate(&self, index: i32) -> ZbusResult<()> {
        self.virtual_keyboard_backend.select_candidate(index).await
    }

    async fn prev_page(&self, index: i32) -> ZbusResult<()> {
        self.virtual_keyboard_backend.prev_page(index).await
    }

    async fn next_page(&self, index: i32) -> ZbusResult<()> {
        self.virtual_keyboard_backend.next_page(index).await
    }
}

#[derive(Clone, Debug, Getters)]
pub struct Fcitx5Services {
    #[getset(get = "pub")]
    controller: Arc<dyn IFcitx5ControllerService + Send + Sync>,
    #[getset(get = "pub")]
    virtual_keyboard: Arc<dyn IFcitx5VirtualKeyboardService + Send + Sync>,
    // Ensure the usage of virtual_keyboard_backend are serialized so that key events from
    // different messages do not interfere with each other
    #[getset(get = "pub")]
    virtual_keyboard_backend:
        Arc<IcedFuturesMutex<dyn IFcitx5VirtualKeyboardBackendService + Send + Sync>>,
}

impl Fcitx5Services {
    pub async fn new(
        modifier_workaround: bool,
        modifier_workaround_keycodes: Vec<u16>,
    ) -> Result<Self> {
        let connection = Connection::session().await?;
        let controller = Fcitx5ControllerServiceProxy::new(&connection).await?;
        let virtual_keyboard = Fcitx5VirtualKeyboardServiceProxy::new(&connection).await?;
        let virtual_keyboard_backend =
            Fcitx5VirtualKeyboardBackendServiceProxy::new(&connection).await?;
        // only enable fused backend, if `--modifier-workaround` is set and keycodes isn't empty
        if modifier_workaround && !modifier_workaround_keycodes.is_empty() {
            tracing::debug!(
                "Work in modifier workaround mode, keycodes: {:?}",
                modifier_workaround_keycodes
            );
            let virtual_keyboard = Arc::new(virtual_keyboard);
            Ok(Self {
                controller: Arc::new(controller),
                virtual_keyboard: virtual_keyboard.clone(),
                virtual_keyboard_backend: Arc::new(IcedFuturesMutex::new(
                    FusedFcitx5VirtualKeyboardBackendService {
                        virtual_keyboard,
                        virtual_keyboard_backend,
                        key_helper_serial: None,
                        key_helper_created: false,
                        fcitx5_osk_key_helper_controller: MaybeUninit::uninit(),
                        modifier_workaround_keycodes,
                    },
                )),
            })
        } else {
            tracing::debug!(
                "Work in normal mode: {}/{:?}",
                modifier_workaround,
                modifier_workaround_keycodes
            );
            Ok(Self {
                controller: Arc::new(controller),
                virtual_keyboard: Arc::new(virtual_keyboard),
                virtual_keyboard_backend: Arc::new(IcedFuturesMutex::new(virtual_keyboard_backend)),
            })
        }
    }

    pub fn new_with(
        controller: Arc<dyn IFcitx5ControllerService + Send + Sync>,
        virtual_keyboard: Arc<dyn IFcitx5VirtualKeyboardService + Send + Sync>,
        virtual_keyboard_backend: Arc<
            IcedFuturesMutex<dyn IFcitx5VirtualKeyboardBackendService + Send + Sync>,
        >,
    ) -> Self {
        Self {
            controller,
            virtual_keyboard,
            virtual_keyboard_backend,
        }
    }
}
