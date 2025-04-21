use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use getset::{CopyGetters, Getters};
use iced::futures::channel::mpsc::UnboundedSender;
use serde::Deserialize;
use tokio::sync::oneshot;
use tracing::instrument;
use zbus::{fdo::Error, interface, Connection};
use zvariant::Type;

use crate::{
    app::Message,
    state::{ImEvent, WindowManagerEvent},
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

#[interface(name = "org.fcitx.Fcitx5.VirtualKeyboard1")]
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

pub struct Fcitx5OskService {
    tx: UnboundedSender<Message>,
    socket_env_tx: Option<oneshot::Sender<SocketEnv>>,
    shutdown_flag: Arc<AtomicBool>,
}

impl Fcitx5OskService {
    const SERVICE_NAME: &'static str = "fyi.fortime.Fcitx5Osk";

    pub fn new(
        tx: UnboundedSender<Message>,
        socket_env_tx: oneshot::Sender<SocketEnv>,
        shutdown_flag: Arc<AtomicBool>,
    ) -> Self {
        Self {
            tx,
            socket_env_tx: Some(socket_env_tx),
            shutdown_flag,
        }
    }

    pub async fn start(self, conn: &Connection) -> Result<(), Error> {
        conn.object_server()
            .at(Self::CONTROLLER_OBJECT_PATH, self)
            .await?;
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

#[interface(name = "fyi.fortime.Fcitx5Osk.Controller")]
impl Fcitx5OskService {
    const CONTROLLER_OBJECT_PATH: &'static str = "/fyi/fortime/Fcitx5Osk/Controller";

    #[instrument(level = "debug", skip(self), err, ret)]
    async fn show(&self) -> Result<(), Error> {
        self.send(ImPanelEvent::Show)
    }

    #[instrument(level = "debug", skip(self), err, ret)]
    async fn hide(&self) -> Result<(), Error> {
        self.send(ImPanelEvent::Hide)
    }

    #[instrument(level = "debug", skip(self), err, ret)]
    async fn change_mode(&self, mode: Mode) -> Result<(), Error> {
        let mode = match mode {
            Mode::Normal => WindowManagerMode::Normal,
            Mode::ExternalDock => WindowManagerMode::ExternalDock,
        };
        self.send(WindowManagerEvent::UpdateMode(mode))
    }

    /// Socket can be open once only.
    #[instrument(level = "debug", skip(self), err, ret)]
    async fn open_socket(&mut self, socket_env: SocketEnv) -> Result<(), Error> {
        if let Some(socket_env_tx) = self.socket_env_tx.take() {
            if socket_env_tx.send(socket_env).is_ok() {
                return Ok(());
            }
        }
        // if socket_env can't be sent, it will trigger a shutdown. fcitx5-osk-wayland-launcher
        // will start a new one.
        self.shutdown_flag.store(true, Ordering::Relaxed);
        Ok(())
    }

    #[instrument(level = "debug", skip(self), err, ret)]
    async fn shutdown(&mut self) -> Result<(), Error> {
        self.shutdown_flag.store(true, Ordering::Relaxed);
        Ok(())
    }
}

#[derive(Deserialize, Type, PartialEq, Debug)]
enum Mode {
    Normal,
    ExternalDock,
}

#[derive(Clone, Debug)]
pub enum ImPanelEvent {
    Show,
    Hide,
}

impl From<ImPanelEvent> for Message {
    fn from(value: ImPanelEvent) -> Self {
        Self::ImPanelEvent(value)
    }
}

#[derive(Deserialize, Type, PartialEq, Debug)]
pub enum SocketEnv {
    WaylandSocket(String),
    WaylandDisplay(String),
    X11Display(String),
}
