pub mod entity {
    use serde::{Deserialize, Serialize};
    use zvariant::{OwnedFd, OwnedValue, Type, Value};

    #[derive(Clone, Copy, Deserialize, Serialize, Type, PartialEq, Debug, Value, OwnedValue)]
    pub enum WindowManagerMode {
        Normal = 0,
        KwinLockScreen = 1,
    }

    #[derive(Deserialize, Serialize, Type, PartialEq, Debug)]
    pub enum Socket {
        Wayland(OwnedFd),
    }

    #[derive(Deserialize, Serialize, Type, PartialEq, Debug)]
    pub enum Display {
        Wayland(String),
        X11(String),
    }
}

pub mod client {
    use anyhow::Result;
    use getset::Getters;
    use zbus::{Connection, Result as ZbusResult};

    use crate::dbus::entity::{Display, Socket, WindowManagerMode};

    #[zbus::proxy(
        default_service = "fyi.fortime.Fcitx5Osk",
        default_path = "/fyi/fortime/Fcitx5Osk/Controller",
        interface = "fyi.fortime.Fcitx5Osk.Controller1"
    )]
    pub trait Fcitx5OskControllerService {
        #[tracing::instrument(level = "debug", skip(self), err, ret)]
        fn force_show(&self) -> ZbusResult<()>;

        #[tracing::instrument(level = "debug", skip(self), err, ret)]
        fn show(&self) -> ZbusResult<()>;

        #[tracing::instrument(level = "debug", skip(self), err, ret)]
        fn force_hide(&self) -> ZbusResult<()>;

        #[tracing::instrument(level = "debug", skip(self), err, ret)]
        fn hide(&self) -> ZbusResult<()>;

        #[tracing::instrument(level = "debug", skip(self), err, ret)]
        #[zbus(property)]
        fn manual_mode(&self) -> ZbusResult<bool>;

        #[tracing::instrument(level = "debug", skip(self), err, ret)]
        fn change_manual_mode(&self, manual_mode: bool) -> ZbusResult<()>;

        #[tracing::instrument(level = "debug", skip(self), err, ret)]
        fn change_visible(&self, visible: bool) -> ZbusResult<()>;

        #[tracing::instrument(level = "debug", skip(self), err, ret)]
        #[zbus(property)]
        fn visible_request(&self) -> ZbusResult<(i64, bool)>;

        #[tracing::instrument(level = "debug", skip(self), err, ret)]
        /// tell the server to change mode.
        fn change_mode(&self, mode: WindowManagerMode) -> ZbusResult<()>;

        #[tracing::instrument(level = "debug", skip(self), err, ret)]
        #[zbus(property)]
        fn mode(&self) -> ZbusResult<WindowManagerMode>;

        #[tracing::instrument(level = "debug", skip(self), err, ret)]
        fn open_socket(&self, socket: Socket) -> ZbusResult<()>;

        #[tracing::instrument(level = "debug", skip(self), err, ret)]
        fn open_display(&self, d: Display) -> ZbusResult<()>;

        #[tracing::instrument(level = "debug", skip(self), err, ret)]
        fn reopen_if_opened(&self) -> ZbusResult<()>;

        #[tracing::instrument(level = "debug", skip(self), err, ret)]
        fn shutdown(&self) -> ZbusResult<()>;
    }

    #[zbus::proxy(
        default_service = "fyi.fortime.Fcitx5OskKeyHelper",
        default_path = "/fyi/fortime/Fcitx5OskKeyHelper/Controller",
        interface = "fyi.fortime.Fcitx5OskKeyHelper.Controller1"
    )]
    pub trait Fcitx5OskKeyHelperControllerService {
        #[tracing::instrument(level = "debug", skip(self), err, ret)]
        fn reset_serial(&self) -> ZbusResult<u64>;

        #[tracing::instrument(level = "debug", skip(self), err, ret)]
        fn process_key_event(&self, serial: u64, keycode: u16, is_release: bool)
            -> ZbusResult<u64>;
    }

    #[derive(Clone, Debug, Getters)]
    pub struct Fcitx5OskServices {
        #[getset(get = "pub")]
        controller: Fcitx5OskControllerServiceProxy<'static>,
    }

    impl Fcitx5OskServices {
        pub async fn new() -> Result<Self> {
            let connection = Connection::session().await?;
            Self::new_with(&connection).await
        }

        pub async fn new_with(connection: &Connection) -> Result<Self> {
            let controller = Fcitx5OskControllerServiceProxy::new(connection).await?;
            Ok(Self { controller })
        }
    }
}

pub const SERVICE_NAME: &str = "fyi.fortime.Fcitx5Osk";
pub const CONTROLLER_OBJECT_PATH: &str = "/fyi/fortime/Fcitx5Osk/Controller";
