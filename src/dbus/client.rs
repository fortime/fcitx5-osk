use tracing::instrument;
use zbus::{proxy, Result};

#[proxy(
    default_service = "org.fcitx.Fcitx5",
    default_path = "/virtualkeyboard",
    interface = "org.fcitx.Fcitx.VirtualKeyboard1"
)]
pub trait Fcitx5VirtualKeyboardService {
    #[instrument(level = "debug", skip(self), err, ret)]
    fn show_virtual_keyboard(&self) -> Result<()>;

    #[instrument(level = "debug", skip(self), err, ret)]
    fn hide_virtual_keyboard(&self) -> Result<()>;

    #[instrument(level = "debug", skip(self), err, ret)]
    fn toggle_virtual_keyboard(&self) -> Result<()>;
}

#[proxy(
    default_service = "org.fcitx.Fcitx5.VirtualKeyboardBackend",
    default_path = "/virtualkeyboard",
    interface = "org.fcitx.Fcitx5.VirtualKeyboardBackend1"
)]
pub trait Fcitx5VirtualKeyboardBackendService {
    #[instrument(level = "debug", skip(self), err, ret)]
    fn set_virtual_keyboard_function_mode(&self, mode: u32) -> Result<()>;

    /// keyval(keysym), state: src/lib/fcitx-utils/keysym.h.
    /// use keyval + state or keycode.
    #[instrument(level = "debug", skip(self), err, ret)]
    fn process_key_event(
        &self,
        keyval: u32,
        keycode: u32,
        state: u32,
        is_release: bool,
        time: u32,
    ) -> Result<()>;

    #[instrument(level = "debug", skip(self), err, ret)]
    fn process_visibility_event(&self, visible: bool) -> Result<()>;

    #[instrument(level = "debug", skip(self), err, ret)]
    fn select_candidate(&self, index: i32) -> Result<()>;

    #[instrument(level = "debug", skip(self), err, ret)]
    fn prev_page(&self, index: i32) -> Result<()>;

    #[instrument(level = "debug", skip(self), err, ret)]
    fn next_page(&self, index: i32) -> Result<()>;
}
