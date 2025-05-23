use iced::{window::Id, Color, Point, Size, Task, Theme};

use crate::config::Placement;

pub mod wayland;
pub mod x11;

pub trait WindowAppearance {
    fn default(theme: &Theme) -> Self;

    fn set_background_color(&mut self, background: Color);

    fn background_color(&self) -> Color;
}

pub trait WindowManager: Default {
    type Message;

    type Appearance;

    /// generate a do nothing task
    fn nothing() -> Task<Self::Message>;

    fn open(&mut self, settings: WindowSettings) -> (Id, Task<Self::Message>);

    fn opened(&mut self, id: Id, size: Size) -> Task<Self::Message>;

    fn close(&mut self, id: Id) -> Task<Self::Message>;

    fn closed(&mut self, id: Id) -> Task<Self::Message>;

    fn resize(&mut self, id: Id, size: Size) -> Task<Self::Message>;

    fn mv(&mut self, id: Id, position: Point) -> Task<Self::Message>;

    fn position(&self, id: Id) -> Option<Point>;

    fn placement(&self, id: Id) -> Option<Placement>;

    fn fetch_screen_info(&mut self) -> Task<Self::Message>;

    fn appearance(&self, theme: &Theme, id: Id) -> Self::Appearance;

    fn set_screen_size(&mut self, size: Size) -> bool;

    /// screen size with exclusive zone
    fn full_screen_size(&self) -> Size;

    fn set_mode(&mut self, mode: WindowManagerMode) -> bool;

    fn mode(&self) -> WindowManagerMode;
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum WindowManagerMode {
    #[default]
    Normal,
    KwinLockScreen,
}

pub struct WindowSettings {
    application_id: String,
    size: Option<Size>,
    placement: Placement,
    position: Point,
    internal: bool,
}

impl WindowSettings {
    pub fn new(size: Option<Size>, placement: Placement) -> Self {
        Self {
            // TODO don't hardcode
            application_id: "fcitx5-osk".to_string(),
            size,
            placement,
            position: Point::ORIGIN,
            internal: false,
        }
    }

    /// setting position will change placement to Float.
    pub fn set_position(mut self, position: Point) -> Self {
        self.placement = Placement::Float;
        self.position = position;
        self
    }

    pub fn placement(&self) -> Placement {
        self.placement
    }
}
