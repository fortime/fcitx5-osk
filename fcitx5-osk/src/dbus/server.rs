use std::{os::fd::OwnedFd, sync::Arc};

use fcitx5_osk_common::{
    dbus::{self, entity},
    signal::ShutdownFlag,
};
use getset::{CopyGetters, Getters};
use iced::futures::channel::mpsc::UnboundedSender;
use tracing::instrument;
use zbus::{fdo::Error, Connection};

use crate::{
    app::Message,
    state::{ImEvent, UpdateConfigEvent, WindowManagerEvent},
    window::WindowManagerMode,
};

///According to codes in fcitx5:src/ui/virtualkeyboard/virtualkeyboard.cpp
///
/// auto msg = bus_->createMethodCall(
///     VirtualKeyboardName, "/org/fcitx/virtualkeyboard/impanel",
///     VirtualKeyboardInterfaceName, "ShowVirtualKeyboard");
///
/// auto msg = bus_->createMethodCall(
///     VirtualKeyboardName, "/org/fcitx/virtualkeyboard/impanel",
///     VirtualKeyboardInterfaceName, "HideVirtualKeyboard");
///
/// auto msg = bus_->createMethodCall(
///     VirtualKeyboardName, "/org/fcitx/virtualkeyboard/impanel",
///     VirtualKeyboardInterfaceName, "UpdatePreeditCaret");
///
/// auto msg = bus_->createMethodCall(
///     VirtualKeyboardName, "/org/fcitx/virtualkeyboard/impanel",
///     VirtualKeyboardInterfaceName, "UpdatePreeditArea");
///
/// auto msg = bus_->createMethodCall(
///     VirtualKeyboardName, "/org/fcitx/virtualkeyboard/impanel",
///     VirtualKeyboardInterfaceName, "UpdateCandidateArea");
///
/// auto msg = bus_->createMethodCall(
///     VirtualKeyboardName, "/org/fcitx/virtualkeyboard/impanel",
///     VirtualKeyboardInterfaceName, "NotifyIMActivated");
///
/// auto msg = bus_->createMethodCall(
///     VirtualKeyboardName, "/org/fcitx/virtualkeyboard/impanel",
///     VirtualKeyboardInterfaceName, "NotifyIMDeactivated");
///
/// auto msg = bus_->createMethodCall(
///     VirtualKeyboardName, "/org/fcitx/virtualkeyboard/impanel",
///     VirtualKeyboardInterfaceName, "NotifyIMListChanged");
#[derive(Clone)]
pub struct Fcitx5VirtualkeyboardImPanelService {
    tx: UnboundedSender<Message>,
}

impl Fcitx5VirtualkeyboardImPanelService {
    pub fn new(tx: UnboundedSender<Message>) -> Self {
        Self { tx }
    }

    pub async fn start(self, conn: &Connection) -> Result<(), Error> {
        conn.object_server().at(Self::OBJECT_PATH, self).await?;
        conn.request_name(Self::SERVICE_NAME).await?;
        Ok(())
    }

    fn send<Event>(&self, event: Event) -> Result<(), Error>
    where
        Event: Into<Message>,
    {
        self.tx.unbounded_send(event.into()).map_err(|_| {
            Error::Failed("the channel has been closed, unable to handle the request".to_string())
        })
    }
}

#[zbus::interface(name = "org.fcitx.Fcitx5.VirtualKeyboard1")]
impl Fcitx5VirtualkeyboardImPanelService {
    const SERVICE_NAME: &'static str = "org.fcitx.Fcitx5.VirtualKeyboard";
    const OBJECT_PATH: &'static str = "/org/fcitx/virtualkeyboard/impanel";

    #[instrument(level = "debug", skip(self), err, ret)]
    async fn show_virtual_keyboard(&self) -> Result<(), Error> {
        self.send(Fcitx5VirtualkeyboardImPanelEvent::ShowVirtualKeyboard)
    }

    #[instrument(level = "debug", skip(self), err, ret)]
    async fn hide_virtual_keyboard(&self) -> Result<(), Error> {
        self.send(Fcitx5VirtualkeyboardImPanelEvent::HideVirtualKeyboard)
    }

    #[instrument(level = "debug", skip(self), err, ret)]
    async fn update_preedit_caret(&self, preedit_cursor: i32) -> Result<(), Error> {
        self.send(Fcitx5VirtualkeyboardImPanelEvent::UpdatePreeditCaret(
            preedit_cursor,
        ))
    }

    #[instrument(level = "debug", skip(self), err, ret)]
    async fn update_preedit_area(&self, preedit_text: String) -> Result<(), Error> {
        self.send(Fcitx5VirtualkeyboardImPanelEvent::UpdatePreeditArea(
            preedit_text,
        ))
    }

    #[instrument(level = "debug", skip(self), err, ret)]
    async fn update_candidate_area(
        &self,
        candidate_text_list: Vec<String>,
        has_prev: bool,
        has_next: bool,
        page_index: i32,
        global_cursor_index: i32,
    ) -> Result<(), Error> {
        self.send(Fcitx5VirtualkeyboardImPanelEvent::UpdateCandidateArea(
            Arc::new(CandidateAreaState {
                candidate_text_list,
                has_prev,
                has_next,
                page_index,
                global_cursor_index,
            }),
        ))
    }

    #[zbus(name = "NotifyIMActivated")]
    #[instrument(level = "debug", skip(self), err, ret)]
    async fn notify_im_activated(&self, im: String) -> Result<(), Error> {
        self.send(ImEvent::UpdateCurrentIm(im))
    }

    #[zbus(name = "NotifyIMDeactivated")]
    #[instrument(level = "debug", skip(self), err, ret)]
    async fn notify_im_deactivated(&self, im: String) -> Result<(), Error> {
        self.send(ImEvent::DeactivateIm(im))
    }

    #[zbus(name = "NotifyIMListChanged")]
    #[instrument(level = "debug", skip(self), err, ret)]
    async fn notify_im_list_changed(&self) -> Result<(), Error> {
        self.send(ImEvent::SyncImList)
    }
}

#[derive(Clone, Debug)]
pub enum Fcitx5VirtualkeyboardImPanelEvent {
    ShowVirtualKeyboard,
    HideVirtualKeyboard,
    #[allow(unused)]
    UpdatePreeditCaret(i32),
    #[allow(unused)]
    UpdatePreeditArea(String),
    UpdateCandidateArea(Arc<CandidateAreaState>),
}

impl From<Fcitx5VirtualkeyboardImPanelEvent> for Message {
    fn from(value: Fcitx5VirtualkeyboardImPanelEvent) -> Self {
        Self::Fcitx5VirtualkeyboardImPanelEvent(value)
    }
}

#[derive(Debug, Getters, CopyGetters)]
pub struct CandidateAreaState {
    #[getset(get = "pub")]
    candidate_text_list: Vec<String>,
    #[getset(get_copy = "pub")]
    has_prev: bool,
    #[getset(get_copy = "pub")]
    has_next: bool,
    #[getset(get_copy = "pub")]
    page_index: i32,
    #[allow(unused)]
    #[getset(get_copy = "pub")]
    global_cursor_index: i32,
}

pub enum SocketEnv {
    WaylandSocket(OwnedFd),
    WaylandDisplay(String),
    X11Display(String),
}

pub struct Fcitx5OskService {
    mode: entity::WindowManagerMode,
    tx: UnboundedSender<Message>,
    socket_env_tx: Option<std::sync::mpsc::Sender<SocketEnv>>,
    shutdown_flag: ShutdownFlag,
}

impl Fcitx5OskService {
    pub fn new(
        tx: UnboundedSender<Message>,
        socket_env_tx: Option<std::sync::mpsc::Sender<SocketEnv>>,
        shutdown_flag: ShutdownFlag,
    ) -> Self {
        Self {
            mode: entity::WindowManagerMode::Normal,
            tx,
            socket_env_tx,
            shutdown_flag,
        }
    }

    pub async fn start(self, conn: &Connection) -> Result<(), Error> {
        conn.object_server()
            .at(dbus::CONTROLLER_OBJECT_PATH, self)
            .await?;
        conn.request_name(dbus::SERVICE_NAME).await?;
        Ok(())
    }

    fn send<Event>(&self, event: Event) -> Result<(), Error>
    where
        Event: Into<Message>,
    {
        self.tx.unbounded_send(event.into()).map_err(|_| {
            Error::Failed("the channel has been closed, unable to handle the request".to_string())
        })
    }
}

#[zbus::interface(name = "fyi.fortime.Fcitx5Osk.Controller1")]
impl Fcitx5OskService {
    #[instrument(level = "debug", skip(self), err, ret)]
    async fn force_show(&self) -> Result<(), Error> {
        self.send(ImPanelEvent::Show(true))
    }

    #[instrument(level = "debug", skip(self), err, ret)]
    async fn show(&self) -> Result<(), Error> {
        self.send(ImPanelEvent::Show(false))
    }

    #[instrument(level = "debug", skip(self), err, ret)]
    async fn force_hide(&self) -> Result<(), Error> {
        self.send(ImPanelEvent::Hide(true))
    }

    #[instrument(level = "debug", skip(self), err, ret)]
    async fn hide(&self) -> Result<(), Error> {
        self.send(ImPanelEvent::Hide(false))
    }

    #[instrument(level = "debug", skip(self), err, ret)]
    async fn change_manual_mode(&self, manual_mode: bool) -> Result<(), Error> {
        self.send(UpdateConfigEvent::ManualMode(manual_mode))
    }

    /// Unlike show/hide, setting visible to false will cause the program generating a transparent
    /// view, it won't open/close the window.
    #[instrument(level = "debug", skip(self), err, ret)]
    async fn change_visible(&self, visible: bool) -> Result<(), Error> {
        self.send(ImPanelEvent::UpdateVisible(visible))
    }

    #[instrument(level = "debug", skip(self), err, ret)]
    async fn change_mode(&self, mode: entity::WindowManagerMode) -> Result<(), Error> {
        let mode = match mode {
            entity::WindowManagerMode::Normal => WindowManagerMode::Normal,
            entity::WindowManagerMode::KwinLockScreen => WindowManagerMode::KwinLockScreen,
        };
        self.send(WindowManagerEvent::UpdateMode(mode))
    }

    #[instrument(level = "debug", skip(self), ret)]
    #[zbus(property(emits_changed_signal = "false"))]
    async fn mode(&self) -> entity::WindowManagerMode {
        self.mode
    }

    #[instrument(level = "debug", skip(self))]
    #[zbus(property)]
    async fn set_mode(&mut self, mode: entity::WindowManagerMode) {
        self.mode = mode;
    }

    /// Socket can be open once only.
    #[instrument(level = "debug", skip(self), err, ret)]
    async fn open_socket(&mut self, socket: entity::Socket) -> Result<(), Error> {
        if let Some(socket_env_tx) = self.socket_env_tx.take() {
            match socket {
                entity::Socket::Wayland(fd) => {
                    if socket_env_tx
                        .send(SocketEnv::WaylandSocket(fd.into()))
                        .is_ok()
                    {
                        return Ok(());
                    }
                }
            }
        }
        // if socket_env can't be sent, it will trigger a shutdown. fcitx5-osk-wayland-launcher
        // will start a new one.
        self.shutdown_flag.shutdown();
        Ok(())
    }

    /// Socket can be open once only.
    #[instrument(level = "debug", skip(self), err, ret)]
    async fn open_display(&mut self, d: entity::Display) -> Result<(), Error> {
        if let Some(socket_env_tx) = self.socket_env_tx.take() {
            let socket_env = match d {
                entity::Display::Wayland(s) => SocketEnv::WaylandDisplay(s),
                entity::Display::X11(s) => SocketEnv::X11Display(s),
            };
            if socket_env_tx.send(socket_env).is_ok() {
                return Ok(());
            }
        }
        // if socket_env can't be sent, it will trigger a shutdown. fcitx5-osk-wayland-launcher
        // will start a new one.
        self.shutdown_flag.shutdown();
        Ok(())
    }

    #[instrument(level = "debug", skip(self), err, ret)]
    async fn reopen_if_opened(&mut self) -> Result<(), Error> {
        self.send(ImPanelEvent::ReopenIfOpened)
    }

    #[instrument(level = "debug", skip(self), err, ret)]
    async fn shutdown(&mut self) -> Result<(), Error> {
        self.shutdown_flag.shutdown();
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub enum ImPanelEvent {
    Show(bool),
    Hide(bool),
    UpdateVisible(bool),
    ReopenIfOpened,
    UpdateManualMode(bool),
}

impl From<ImPanelEvent> for Message {
    fn from(value: ImPanelEvent) -> Self {
        Self::ImPanelEvent(value)
    }
}
