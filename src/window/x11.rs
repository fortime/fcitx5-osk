use iced::{
    window::{self as iced_window, Id, Settings},
    Size, Task,
};

use crate::{
    app::Message,
    has_text_within_env,
    window::{WindowManager, WindowSettings},
};

pub fn is_available() -> bool {
    has_text_within_env("DISPLAY")
}

#[derive(Default)]
pub struct X11WindowManager;

impl WindowManager for X11WindowManager {
    type Message = Message;

    fn open(&mut self, settings: WindowSettings) -> (Id, Task<Self::Message>) {
        let mut iced_settings = Settings::default();
        iced_settings.size = settings.size;
        iced_settings.decorations = false;
        // TODO placement, and application_id
        let (id, task) = iced_window::open(iced_settings);
        (
            id,
            task.then(|id| iced_window::get_scale_factor(id))
                .map(|scale_factor| {
                    tracing::debug!("scale_factor of window: {}", scale_factor);
                    Self::Message::Nothing
                }),
        )
    }

    fn close(&mut self, window_id: Id) -> Task<Self::Message> {
        iced_window::close(window_id)
    }

    fn resize(&mut self, window_id: Id, size: Size) -> Task<Self::Message> {
        // TODO I think it will be a close and open combo
        iced_window::resize(window_id, size)
    }
}
