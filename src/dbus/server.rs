use zbus::{fdo::Error, interface, Connection};

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
pub struct VirtualkeyboardImPanelService {}

impl VirtualkeyboardImPanelService {
    pub async fn start(self) -> Result<Connection, Error> {
        let conn = Connection::session().await?;
        conn.object_server().at(Self::OBJECT_PATH, self).await?;
        conn.request_name(Self::SERVICE_NAME).await?;
        Ok(conn)
    }
}

#[interface(name = "org.fcitx.Fcitx5.VirtualKeyboard1")]
impl VirtualkeyboardImPanelService {
    const SERVICE_NAME: &'static str = "org.fcitx.Fcitx5.VirtualKeyboard";
    const OBJECT_PATH: &'static str = "/org/fcitx/virtualkeyboard/impanel";

    async fn show_virtual_keyboard(&self) -> Result<(), Error> {
        tracing::error!("show");
        Ok(())
    }

    async fn hide_virtual_keyboard(&self) -> Result<(), Error> {
        tracing::error!("hide");
        Ok(())
    }

    async fn update_preedit_caret(&self, preedit_cursor: i32) -> Result<(), Error> {
        tracing::error!("cursor: {}", preedit_cursor);
        Ok(())
    }

    async fn update_preedit_area(&self, preedit_text: String) -> Result<(), Error> {
        tracing::error!("text: {}", preedit_text);
        Ok(())
    }

    async fn update_candidate_area(
        &self,
        candidate_text_list: Vec<String>,
        has_prev: bool,
        has_next: bool,
        page_index: i32,
        global_cursor_index: i32,
    ) -> Result<(), Error> {
        tracing::error!("text list: {:?}", candidate_text_list);
        Ok(())
    }

    async fn notify_im_activated(&self, im: String) -> Result<(), Error> {
        tracing::error!("activated im: {}", im);
        Ok(())
    }

    async fn notify_im_deactivated(&self, im: String) -> Result<(), Error> {
        tracing::error!("deactivated im: {}", im);
        Ok(())
    }

    async fn notify_im_list_changed(&self) -> Result<(), Error> {
        tracing::error!("im list changed");
        Ok(())
    }
}
