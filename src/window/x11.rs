use iced::{
    daemon::{Appearance, DefaultStyle},
    window::{self as iced_window, Id, Settings},
    Color, Point, Size, Task, Theme,
};

use crate::{
    app::Message,
    config::Placement,
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

    fn nothing() -> Task<Self::Message> {
        Message::from_nothing()
    }

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

    fn opened(&mut self, _id: Id, _size: Size) -> Task<Self::Message> {
        todo!()
    }

    fn close(&mut self, id: Id) -> Task<Self::Message> {
        iced_window::close(id)
    }

    fn closed(&mut self, _id: Id) -> Task<Self::Message> {
        todo!()
    }

    fn resize(&mut self, id: Id, size: Size) -> Task<Self::Message> {
        iced_window::resize(id, size)
    }

    fn mv(&mut self, id: Id, position: Point) -> Task<Self::Message> {
        iced_window::move_to(id, position)
    }

    fn position(&self, _id: Id) -> Option<Point> {
        todo!()
    }

    fn placement(&self, _id: Id) -> Option<Placement> {
        todo!()
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
