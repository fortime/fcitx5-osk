use iced::futures::channel::mpsc::UnboundedSender;
use tracing::instrument;
use zbus::{fdo::Error, interface, Connection};

use crate::app::Message;

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

    pub async fn start(self) -> Result<Connection, Error> {
        let conn = Connection::session().await?;
        conn.object_server().at(Self::OBJECT_PATH, self).await?;
        conn.request_name(Self::SERVICE_NAME).await?;
        Ok(conn)
    }

    fn send(&self, state: Fcitx5VirtualkeyboardImPanelState) -> Result<(), Error> {
        self.tx.unbounded_send(state.into()).map_err(|_| {
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
        self.send(Fcitx5VirtualkeyboardImPanelState::ShowVirtualKeyboard)
    }

    #[instrument(level = "debug", skip(self), err, ret)]
    async fn hide_virtual_keyboard(&self) -> Result<(), Error> {
        self.send(Fcitx5VirtualkeyboardImPanelState::HideVirtualKeyboard)
    }

    #[instrument(level = "debug", skip(self), err, ret)]
    async fn update_preedit_caret(&self, preedit_cursor: i32) -> Result<(), Error> {
        self.send(Fcitx5VirtualkeyboardImPanelState::UpdatePreeditCaret(
            preedit_cursor,
        ))
    }

    #[instrument(level = "debug", skip(self), err, ret)]
    async fn update_preedit_area(&self, preedit_text: String) -> Result<(), Error> {
        self.send(Fcitx5VirtualkeyboardImPanelState::UpdatePreeditArea(
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
        self.send(Fcitx5VirtualkeyboardImPanelState::UpdateCandidateArea {
            candidate_text_list,
            has_prev,
            has_next,
            page_index,
            global_cursor_index,
        })
    }

    #[instrument(level = "debug", skip(self), err, ret)]
    async fn notify_im_activated(&self, im: String) -> Result<(), Error> {
        self.send(Fcitx5VirtualkeyboardImPanelState::NotifyImActivated(im))
    }

    #[instrument(level = "debug", skip(self), err, ret)]
    async fn notify_im_deactivated(&self, im: String) -> Result<(), Error> {
        self.send(Fcitx5VirtualkeyboardImPanelState::NotifyImDeactivated(im))
    }

    #[instrument(level = "debug", skip(self), err, ret)]
    async fn notify_im_list_changed(&self) -> Result<(), Error> {
        self.send(Fcitx5VirtualkeyboardImPanelState::NotifyImListChanged)
    }
}

#[derive(Clone, Debug)]
pub enum Fcitx5VirtualkeyboardImPanelState {
    ShowVirtualKeyboard,
    HideVirtualKeyboard,
    UpdatePreeditCaret(i32),
    UpdatePreeditArea(String),
    UpdateCandidateArea {
        candidate_text_list: Vec<String>,
        has_prev: bool,
        has_next: bool,
        page_index: i32,
        global_cursor_index: i32,
    },
    NotifyImActivated(String),
    NotifyImDeactivated(String),
    NotifyImListChanged,
}

impl From<Fcitx5VirtualkeyboardImPanelState> for Message {
    fn from(value: Fcitx5VirtualkeyboardImPanelState) -> Self {
        Self::Fcitx5VirtualkeyboardImPanel(value)
    }
}
