pub mod client {
    use anyhow::Result;
    use getset::Getters;
    use zbus::{zvariant::OwnedFd, Connection, Result as ZbusResult};

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
        #[zbus(property(emits_changed_signal = "false"), name = "visible")]
        fn visible(&self) -> ZbusResult<bool>;

        #[zbus(signal, name = "visibleChanged")]
        fn visible_changed(&self);

        /// don't wait for reply, so it won't freeze kwin.
        #[tracing::instrument(level = "debug", skip(self), err, ret)]
        #[zbus(property, name = "enabled")]
        fn set_enabled(&self, value: bool) -> ZbusResult<()>;
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
}
