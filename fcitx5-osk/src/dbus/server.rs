use std::{
    os::fd::OwnedFd,
    sync::{Arc, Mutex, MutexGuard},
};

use fcitx5_osk_common::{
    dbus::{self, entity},
    signal::ShutdownFlag,
};
use getset::{CopyGetters, Getters};
use iced::{
    Font,
    futures::{
        StreamExt as _,
        channel::{
            mpsc::{self, UnboundedReceiver, UnboundedSender},
            oneshot::Sender,
        },
    },
};
use uuid::Uuid;
use zbus::{
    Connection,
    fdo::Error,
    object_server::{InterfaceRef, SignalEmitter},
};

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
            Error::Failed(
                "The internal channel of fcitx5-osk has been closed, unable to handle the request"
                    .to_string(),
            )
        })
    }
}

#[zbus::interface(name = "org.fcitx.Fcitx5.VirtualKeyboard1")]
impl Fcitx5VirtualkeyboardImPanelService {
    const SERVICE_NAME: &'static str = "org.fcitx.Fcitx5.VirtualKeyboard";
    const OBJECT_PATH: &'static str = "/org/fcitx/virtualkeyboard/impanel";

    #[tracing::instrument(level = "debug", skip(self), err, ret)]
    async fn show_virtual_keyboard(&self) -> Result<(), Error> {
        self.send(Fcitx5VirtualkeyboardImPanelEvent::ShowVirtualKeyboard)
    }

    #[tracing::instrument(level = "debug", skip(self), err, ret)]
    async fn hide_virtual_keyboard(&self) -> Result<(), Error> {
        self.send(Fcitx5VirtualkeyboardImPanelEvent::HideVirtualKeyboard)
    }

    #[tracing::instrument(level = "debug", skip(self), err, ret)]
    async fn update_preedit_caret(&self, preedit_cursor: i32) -> Result<(), Error> {
        self.send(Fcitx5VirtualkeyboardImPanelEvent::UpdatePreeditCaret(
            preedit_cursor,
        ))
    }

    #[tracing::instrument(level = "debug", skip(self), err, ret)]
    async fn update_preedit_area(&self, preedit_text: String) -> Result<(), Error> {
        self.send(Fcitx5VirtualkeyboardImPanelEvent::UpdatePreeditArea(
            preedit_text,
        ))
    }

    #[tracing::instrument(level = "debug", skip(self), err, ret)]
    async fn update_candidate_area(
        &self,
        candidate_text_list: Vec<String>,
        has_prev: bool,
        has_next: bool,
        page_index: i32,
        global_cursor_index: i32,
    ) -> Result<(), Error> {
        self.send(Fcitx5VirtualkeyboardImPanelEvent::UpdateCandidateArea(
            Arc::new(CandidateAreaState::new(
                candidate_text_list
                    .into_iter()
                    .map(|text| vec![(text, None)])
                    .collect(),
                has_prev,
                has_next,
                page_index,
                global_cursor_index,
            )),
        ))
    }

    #[zbus(name = "NotifyIMActivated")]
    #[tracing::instrument(level = "debug", skip(self), err, ret)]
    async fn notify_im_activated(&self, im: String) -> Result<(), Error> {
        self.send(ImEvent::UpdateCurrentIm(im))
    }

    #[zbus(name = "NotifyIMDeactivated")]
    #[tracing::instrument(level = "debug", skip(self), err, ret)]
    async fn notify_im_deactivated(&self, im: String) -> Result<(), Error> {
        self.send(ImEvent::DeactivateIm(im))
    }

    #[zbus(name = "NotifyIMListChanged")]
    #[tracing::instrument(level = "debug", skip(self), err, ret)]
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
    candidate_text_list: Vec<Vec<(String, Option<Font>)>>,
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

impl CandidateAreaState {
    pub fn new(
        candidate_text_list: Vec<Vec<(String, Option<Font>)>>,
        has_prev: bool,
        has_next: bool,
        page_index: i32,
        global_cursor_index: i32,
    ) -> Self {
        Self {
            candidate_text_list,
            has_prev,
            has_next,
            page_index,
            global_cursor_index,
        }
    }

    pub fn empty() -> Self {
        Self {
            candidate_text_list: vec![],
            has_prev: false,
            has_next: false,
            page_index: 0,
            global_cursor_index: 0,
        }
    }
}

pub enum SocketEnv {
    WaylandSocket(OwnedFd),
    WaylandDisplay(String),
    X11Display(String),
}

struct InnerFcitx5OskServiceState {
    mode: entity::WindowManagerMode,
    manual_mode: bool,
    // (id, manual_mode, finished)
    session_manual_modes: Vec<(String, bool, bool)>,
    visible: bool,
    /// A visible changed request. The first part is the id of the request, the second part is to
    /// be visible or to be invisible.
    visible_request: (i64, bool),
}

pub struct Fcitx5OskServiceClient {
    state: Arc<Mutex<InnerFcitx5OskServiceState>>,
    tx: UnboundedSender<PropertyChangedSignal>,
}

impl Fcitx5OskServiceClient {
    fn state(&self) -> Option<MutexGuard<'_, InnerFcitx5OskServiceState>> {
        match self.state.lock() {
            Ok(s) => Some(s),
            Err(_) => {
                tracing::error!("The state of Fcitx5OskService is poisoned");
                None
            }
        }
    }

    pub fn set_manual_mode(&self, manual_mode: bool, clear_session: bool) {
        let Some(mut state) = self.state() else {
            return;
        };
        if clear_session {
            state.session_manual_modes = vec![];
        }
        state.manual_mode = manual_mode;
        self.send(PropertyChangedSignal::ManualMode);
    }

    pub fn visible(&self) -> Option<bool> {
        self.state().map(|s| s.visible)
    }

    #[allow(unused)]
    pub fn set_visible(&self, visible: bool) {
        let Some(mut state) = self.state() else {
            return;
        };
        state.visible = visible;
        self.send(PropertyChangedSignal::Visible);
    }

    pub fn set_mode(&self, mode: WindowManagerMode) {
        let Some(mut state) = self.state() else {
            return;
        };
        let mode = match mode {
            WindowManagerMode::Normal => entity::WindowManagerMode::Normal,
            WindowManagerMode::KwinLockScreen => entity::WindowManagerMode::KwinLockScreen,
        };
        state.mode = mode;
        self.send(PropertyChangedSignal::Mode);
    }

    pub fn new_visible_request(&self, visible: bool) {
        let Some(mut state) = self.state() else {
            return;
        };

        state.visible_request.0 += 1;
        state.visible_request.1 = visible;

        self.send(PropertyChangedSignal::VisibleRequest);
    }

    fn send(&self, signal: PropertyChangedSignal) {
        if self.tx.unbounded_send(signal).is_err() {
            tracing::error!(
                "The channel of fcitx5_osk_service_event_loop has been closed, unable to handle the request"
            )
        }
    }
}

pub struct Fcitx5OskService {
    state: Arc<Mutex<InnerFcitx5OskServiceState>>,
    tx: UnboundedSender<Message>,
    socket_env_tx: Option<Sender<SocketEnv>>,
    shutdown_flag: ShutdownFlag,
}

impl Fcitx5OskService {
    pub fn new(
        tx: UnboundedSender<Message>,
        socket_env_tx: Option<Sender<SocketEnv>>,
        shutdown_flag: ShutdownFlag,
    ) -> Self {
        Self {
            state: Arc::new(Mutex::new(InnerFcitx5OskServiceState {
                mode: entity::WindowManagerMode::Normal,
                manual_mode: false,
                session_manual_modes: vec![],
                visible: true,
                visible_request: (0, true),
            })),
            tx,
            socket_env_tx,
            shutdown_flag,
        }
    }

    pub async fn start(self, conn: &Connection) -> Result<Fcitx5OskServiceClient, Error> {
        let state = self.state.clone();

        conn.object_server()
            .at(dbus::CONTROLLER_OBJECT_PATH, self)
            .await?;
        conn.request_name(dbus::SERVICE_NAME).await?;

        let fcitx5_osk_service_ref = conn
            .object_server()
            .interface(dbus::CONTROLLER_OBJECT_PATH)
            .await?;

        let (tx, rx) = mpsc::unbounded();

        let client = Fcitx5OskServiceClient { state, tx };

        tokio::spawn(async move {
            if let Err(e) = fcitx5_osk_service_event_loop(rx, fcitx5_osk_service_ref).await {
                tracing::error!("fcitx5_osk_service_event_loop exits with error: {e:#?}");
            } else {
                tracing::warn!("fcitx5_osk_service_event_loop exit");
            }
        });

        Ok(client)
    }

    fn send<Event>(&self, event: Event) -> Result<(), Error>
    where
        Event: Into<Message>,
    {
        self.tx.unbounded_send(event.into()).map_err(|_| {
            Error::Failed("the channel has been closed, unable to handle the request".to_string())
        })
    }

    fn state(&self) -> Result<MutexGuard<'_, InnerFcitx5OskServiceState>, Error> {
        self.state.lock().map_err(|_| {
            tracing::error!("The state of Fcitx5OskService is poisoned");
            Error::Failed("The state of Fcitx5OskService is poisoned".to_string())
        })
    }
}

#[zbus::interface(name = "fyi.fortime.Fcitx5Osk.Controller1")]
impl Fcitx5OskService {
    #[tracing::instrument(level = "debug", skip(self), err, ret)]
    async fn force_show(&self) -> Result<(), Error> {
        self.send(ImPanelEvent::Show(true))
    }

    #[tracing::instrument(level = "debug", skip(self), err, ret)]
    async fn show(&self) -> Result<(), Error> {
        self.send(ImPanelEvent::Show(false))
    }

    #[tracing::instrument(level = "debug", skip(self), err, ret)]
    async fn force_hide(&self) -> Result<(), Error> {
        self.send(ImPanelEvent::Hide(true))
    }

    #[tracing::instrument(level = "debug", skip(self), err, ret)]
    async fn hide(&self) -> Result<(), Error> {
        self.send(ImPanelEvent::Hide(false))
    }

    #[tracing::instrument(level = "debug", skip(self), err, ret)]
    #[zbus(property)]
    async fn manual_mode(&self) -> Result<bool, Error> {
        Ok(self.state()?.manual_mode)
    }

    #[tracing::instrument(level = "debug", skip(self), err, ret)]
    async fn change_manual_mode(&self, manual_mode: bool) -> Result<(), Error> {
        self.send(UpdateConfigEvent::ManualMode(manual_mode))
    }

    /// Start a session with the manual mode.
    #[tracing::instrument(level = "debug", skip(self), err, ret)]
    async fn start_manual_mode_session(&self, manual_mode: bool) -> Result<String, Error> {
        let mut state = self.state()?;
        let session_id = Uuid::new_v4().to_string();
        state
            .session_manual_modes
            .push((session_id.clone(), manual_mode, false));
        self.send(ImPanelEvent::UpdateSessionManualMode(Some(manual_mode)))?;
        Ok(session_id)
    }

    /// End a session, all its nested sessions are clear. And it will reset the session manual mode
    /// to its parent session or unset it.
    #[tracing::instrument(level = "debug", skip(self), err, ret)]
    async fn stop_manual_mode_session(&self, session_id: String) -> Result<(), Error> {
        let mut state = self.state()?;
        if let Some(session) = state
            .session_manual_modes
            .iter_mut()
            .rfind(|session| session.0 == session_id)
        {
            session.2 = true;
            let _ = session;
            // remove finished session from the end
            let mut new_manual_mode = None;
            while let Some(last) = state.session_manual_modes.last() {
                if last.2 {
                    state.session_manual_modes.pop();
                } else {
                    new_manual_mode = Some(last.1);
                    break;
                }
            }
            if let Some(new_manual_mode) = new_manual_mode {
                self.send(ImPanelEvent::UpdateSessionManualMode(Some(new_manual_mode)))?;
            } else if state.session_manual_modes.is_empty() {
                // No session, clear
                self.send(ImPanelEvent::UpdateSessionManualMode(None))?;
            }
        }
        Ok(())
    }

    #[tracing::instrument(level = "debug", skip(self), err, ret)]
    #[zbus(property)]
    async fn visible(&self) -> Result<bool, Error> {
        Ok(self.state()?.visible)
    }

    /// Unlike show/hide, setting visible to false will cause the program generating a transparent
    /// view, it won't open/close the window.
    #[tracing::instrument(level = "debug", skip(self), err, ret)]
    async fn change_visible(
        &self,
        #[zbus(signal_emitter)] signal_emitter: SignalEmitter<'_>,
        visible: bool,
    ) -> Result<(), Error> {
        self.state()?.visible = visible;
        self.visible_changed(&signal_emitter).await?;
        Ok(())
    }

    #[tracing::instrument(level = "debug", skip(self), err, ret)]
    #[zbus(property)]
    async fn visible_request(&self) -> Result<(i64, bool), Error> {
        Ok(self.state()?.visible_request)
    }

    #[tracing::instrument(level = "debug", skip(self), ret)]
    #[zbus(property)]
    async fn mode(&self) -> Result<entity::WindowManagerMode, Error> {
        Ok(self.state()?.mode)
    }

    #[tracing::instrument(level = "debug", skip(self), err, ret)]
    async fn change_mode(&self, mode: entity::WindowManagerMode) -> Result<(), Error> {
        let mode = match mode {
            entity::WindowManagerMode::Normal => WindowManagerMode::Normal,
            entity::WindowManagerMode::KwinLockScreen => WindowManagerMode::KwinLockScreen,
        };
        self.send(WindowManagerEvent::UpdateMode(mode))
    }

    /// Socket can be open once only.
    #[tracing::instrument(level = "debug", skip(self), err, ret)]
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
        tracing::error!("An unexpected `open_socket` calling causes the process to exit");
        // if socket_env can't be sent, it will trigger a shutdown. fcitx5-osk-wayland-launcher
        // will start a new one.
        self.shutdown_flag.shutdown();
        Ok(())
    }

    /// Socket can be open once only.
    #[tracing::instrument(level = "debug", skip(self), err, ret)]
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
        tracing::error!("An unexpected open_display call");
        // if socket_env can't be sent, it will trigger a shutdown. fcitx5-osk-wayland-launcher
        // will start a new one.
        self.shutdown_flag.shutdown();
        Ok(())
    }

    #[tracing::instrument(level = "debug", skip(self), err, ret)]
    async fn reopen_if_opened(&mut self) -> Result<(), Error> {
        self.send(ImPanelEvent::ReopenIfOpened)
    }

    #[tracing::instrument(level = "debug", skip(self), err, ret)]
    async fn shutdown(&mut self) -> Result<(), Error> {
        self.shutdown_flag.shutdown();
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub enum ImPanelEvent {
    Show(bool),
    Hide(bool),
    NewVisibleRequest(bool),
    ReopenIfOpened,
    UpdateManualMode(bool),
    UpdateSessionManualMode(Option<bool>),
}

impl From<ImPanelEvent> for Message {
    fn from(value: ImPanelEvent) -> Self {
        Self::ImPanelEvent(value)
    }
}

#[derive(Clone, Debug)]
pub enum PropertyChangedSignal {
    ManualMode,
    Mode,
    Visible,
    VisibleRequest,
}

async fn fcitx5_osk_service_event_loop(
    mut rx: UnboundedReceiver<PropertyChangedSignal>,
    fcitx5_osk_service_ref: InterfaceRef<Fcitx5OskService>,
) -> anyhow::Result<()> {
    while let Some(signal) = rx.next().await {
        tracing::debug!("Receive signal: {signal:?}");
        match signal {
            PropertyChangedSignal::ManualMode => {
                fcitx5_osk_service_ref
                    .get()
                    .await
                    .manual_mode_changed(fcitx5_osk_service_ref.signal_emitter())
                    .await?
            }
            PropertyChangedSignal::Mode => {
                fcitx5_osk_service_ref
                    .get()
                    .await
                    .mode_changed(fcitx5_osk_service_ref.signal_emitter())
                    .await?
            }
            PropertyChangedSignal::Visible => {
                fcitx5_osk_service_ref
                    .get()
                    .await
                    .visible_changed(fcitx5_osk_service_ref.signal_emitter())
                    .await?
            }
            PropertyChangedSignal::VisibleRequest => {
                fcitx5_osk_service_ref
                    .get()
                    .await
                    .visible_request_changed(fcitx5_osk_service_ref.signal_emitter())
                    .await?
            }
        }
    }
    Ok(())
}
