use iced::{
    daemon::{Appearance, DefaultStyle},
    window::{self as iced_window, Id, Settings},
    Color, Size, Task, Theme,
};

use crate::{
    app::Message,
    has_text_within_env,
    window::{WindowAppearance, WindowManager, WindowSettings},
};

pub fn is_available() -> bool {
    has_text_within_env("DISPLAY")
}

impl WindowAppearance for Appearance {
    fn default(theme: &Theme) -> Self {
        theme.default_style()
    }

    fn set_background_color(&mut self, background_color: iced::Color) {
        self.background_color = background_color;
    }
}

#[derive(Default)]
pub struct X11WindowManager;

impl WindowManager for X11WindowManager {
    type Message = Message;

    type Appearance = Appearance;

    fn open(&mut self, settings: WindowSettings) -> (Id, Task<Self::Message>) {
        let mut iced_settings = Settings::default();
        iced_settings.size = settings.size.unwrap();
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

    fn opened(&mut self, id: Id, size: Size) -> Task<Self::Message> {
        todo!()
    }

    fn close(&mut self, id: Id) -> Task<Self::Message> {
        iced_window::close(id)
    }

    fn closed(&mut self, id: Id) -> Task<Self::Message> {
        todo!()
    }

    fn resize(&mut self, id: Id, size: Size) -> Task<Self::Message> {
        iced_window::resize(id, size)
    }

    fn fetch_screen_info(&mut self) -> Task<Self::Message> {
        todo!()
    }

    fn appearance(&self, theme: &Theme, _id: Id) -> Self::Appearance {
        let mut appearance = Self::Appearance::default(theme);
        appearance.set_background_color(Color::TRANSPARENT);
        appearance
    }
}
