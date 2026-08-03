pub mod client {
    use anyhow::Result;
    use getset::Getters;
    use zbus::{Connection, Result as ZbusResult, zvariant::OwnedFd};

    #[zbus::proxy(
        default_service = "org.fcitx.Fcitx5",
        default_path = "/controller",
        interface = "org.fcitx.Fcitx.Controller1"
    )]
    pub trait Fcitx5ControllerService {
        #[tracing::instrument(level = "debug", skip(self), err, ret)]
        fn reopen_wayland_connection_socket(
            &self,
            wayland_display: &str,
            wayland_socket: OwnedFd,
        ) -> ZbusResult<()>;

        #[tracing::instrument(level = "debug", skip(self), err, ret)]
        fn open_wayland_connection_socket(&self, wayland_socket: OwnedFd) -> ZbusResult<()>;
    }

    #[zbus::proxy(
        default_service = "org.kde.keyboard",
        default_path = "/VirtualKeyboard",
        interface = "org.kde.kwin.VirtualKeyboard"
    )]
    pub trait KwinVirtualKeyboardService {
        #[tracing::instrument(level = "debug", skip(self), err, ret)]
        #[zbus(property(emits_changed_signal = "false"), name = "active")]
        fn active(&self) -> ZbusResult<bool>;

        #[zbus(signal, name = "activeChanged")]
        fn active_changed(&self);

        #[tracing::instrument(level = "debug", skip(self), err, ret)]
        #[zbus(property, name = "active")]
        fn set_active(&self, value: bool) -> ZbusResult<()>;

        #[tracing::instrument(level = "debug", skip(self), err, ret)]
        #[zbus(property(emits_changed_signal = "false"), name = "visible")]
        fn visible(&self) -> ZbusResult<bool>;

        #[zbus(signal, name = "visibleChanged")]
        fn visible_changed(&self);
    }

    // Path=/org/kde/KWin  Interface=org.kde.KWin.TabletModeManager  Member=tabletModeChanged
    #[zbus::proxy(
        default_service = "org.kde.KWin",
        default_path = "/org/kde/KWin",
        interface = "org.kde.KWin.TabletModeManager"
    )]
    pub trait KwinTabletModeService {
        #[zbus(property(emits_changed_signal = "false"), name = "tabletMode")]
        fn tablet_mode(&self) -> ZbusResult<bool>;
    }

    #[derive(Clone, Debug, Getters)]
    pub struct KwinServices {
        #[getset(get = "pub")]
        virtual_keyboard: KwinVirtualKeyboardServiceProxy<'static>,

        #[getset(get = "pub")]
        tablet_mode: KwinTabletModeServiceProxy<'static>,
    }

    impl KwinServices {
        #[allow(unused)]
        pub async fn new() -> Result<Self> {
            let connection = Connection::session().await?;
            Self::new_with(&connection).await
        }

        pub async fn new_with(connection: &Connection) -> Result<Self> {
            let virtual_keyboard = KwinVirtualKeyboardServiceProxy::new(connection).await?;
            let tablet_mode = KwinTabletModeServiceProxy::new(connection).await?;
            Ok(Self {
                virtual_keyboard,
                tablet_mode,
            })
        }
    }

    // org.freedesktop.ScreenSaver /ScreenSaver org.freedesktop.ScreenSaver GetActive
    #[zbus::proxy(
        default_service = "org.freedesktop.ScreenSaver",
        default_path = "/org/freedesktop/ScreenSaver",
        interface = "org.freedesktop.ScreenSaver"
    )]
    pub trait FdoScreenSaverService {
        #[tracing::instrument(level = "debug", skip(self), err, ret)]
        fn get_active(&self) -> ZbusResult<bool>;

        #[zbus(signal)]
        fn active_changed(&self, active: bool);
    }

    #[derive(Clone, Debug, Getters)]
    pub struct FdoServices {
        #[getset(get = "pub")]
        screen_saver: FdoScreenSaverServiceProxy<'static>,
    }

    impl FdoServices {
        #[allow(unused)]
        pub async fn new() -> Result<Self> {
            let connection = Connection::session().await?;
            Self::new_with(&connection).await
        }

        pub async fn new_with(connection: &Connection) -> Result<Self> {
            let screen_saver = FdoScreenSaverServiceProxy::new(connection).await?;
            Ok(Self { screen_saver })
        }
    }

    #[zbus::proxy(
        default_service = "fyi.fortime.Fcitx5Osk.KwinLauncher",
        default_path = "/fyi/fortime/Fcitx5Osk/KwinLauncher/Controller",
        interface = "fyi.fortime.Fcitx5Osk.KwinLauncher.Controller1"
    )]
    pub trait Fcitx5OskKwinLauncherControllerService {
        #[tracing::instrument(level = "debug", skip(self), err, ret)]
        async fn register_kwin_input_method(&self, kwin_input_method: &str) -> ZbusResult<()>;
    }
}

pub mod server {
    use anyhow::Result;
    use tokio::sync::mpsc::UnboundedSender;
    use zbus::{
        Connection,
        fdo::{Error as ZbusFdoError, Result as ZbusFdoResult},
    };

    use crate::Message;

    pub struct Fcitx5OskKwinLauncherService {
        tx: UnboundedSender<Message>,
    }

    impl Fcitx5OskKwinLauncherService {
        pub fn new(tx: UnboundedSender<Message>) -> Self {
            Self { tx }
        }

        fn send(&self, message: Message) -> ZbusFdoResult<()> {
            self.tx.send(message).map_err(|_| {
                ZbusFdoError::Failed(
                    "The internal channel of fcitx5-osk-kwin-launcher has been closed, unable to handle the request"
                    .to_string(),
                )
            })
        }

        pub async fn start(self, conn: &Connection) -> Result<()> {
            conn.object_server()
                .at(super::CONTROLLER_OBJECT_PATH, self)
                .await?;
            conn.request_name(super::SERVICE_NAME).await?;

            Ok(())
        }
    }

    #[zbus::interface(name = "fyi.fortime.Fcitx5Osk.KwinLauncher.Controller1")]
    impl Fcitx5OskKwinLauncherService {
        // zbus::interface doesn't support type alias in the response
        #[tracing::instrument(level = "debug", skip(self), err, ret)]
        async fn register_kwin_input_method(
            &self,
            kwin_input_method: String,
        ) -> zbus::fdo::Result<()> {
            self.send(Message::RegisterKwinInputMethod(kwin_input_method))
        }

        #[tracing::instrument(level = "debug", skip(self), err, ret)]
        async fn restart_kwin_input_method(&self) -> zbus::fdo::Result<()> {
            self.send(Message::RestartKwinInputMethod)
        }
    }
}

pub const SERVICE_NAME: &str = "fyi.fortime.Fcitx5Osk.KwinLauncher";
pub const CONTROLLER_OBJECT_PATH: &str = "/fyi/fortime/Fcitx5Osk/KwinLauncher/Controller";
