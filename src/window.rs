use iced::{window::{Id, Position}, Color, Size, Task, Theme};

use crate::config::Placement;

pub mod wayland;
pub mod x11;

pub trait WindowAppearance {
    fn default(theme: &Theme) -> Self;

    fn set_background_color(&mut self, background: Color);
}

pub trait WindowManager: Default {
    type Message;

    type Appearance;

    fn open(&mut self, settings: WindowSettings) -> (Id, Task<Self::Message>);

    fn opened(&mut self, id: Id, size: Size) -> Task<Self::Message>;

    fn close(&mut self, id: Id) -> Task<Self::Message>;

    fn closed(&mut self, id: Id) -> Task<Self::Message>;

    fn resize(&mut self, id: Id, size: Size) -> Task<Self::Message>;

    // fn move(&mut self, id: Id, position: Position) -> Task<Self::Message>;

    fn fetch_screen_info(&mut self) -> Task<Self::Message>;

    fn appearance(&self, theme: &Theme, id: Id) -> Self::Appearance;
}

#[derive(Default)]
pub struct WindowSettings {
    size: Option<Size>,
    placement: Placement,
    position: Option<Position>,
    use_last_output: bool,
}

impl WindowSettings {
    pub fn new(size: Option<Size>, placement: Placement) -> Self {
        Self {
            size,
            placement,
            position: None,
            use_last_output: true,
        }
    }
}
