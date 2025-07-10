use iced::{window::Id, Color, Point, Size, Task, Theme};

use crate::config::Placement;

pub mod wayland;
pub mod x11;

pub trait WindowAppearance {
    fn default(theme: &Theme) -> Self;

    fn set_background_color(&mut self, background: Color);

    fn background_color(&self) -> Color;
}

#[allow(clippy::enum_variant_names)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SyncOutputResponse {
    OutputChanged,
    SizeChanged,
    ScaleFactorChanged(f64),
    RotationChanged,
}

pub trait WindowManager {
    type Message;

    type Appearance;

    /// generate a do nothing task
    fn nothing() -> Task<Self::Message>;

    fn open(&mut self, settings: WindowSettings) -> (Option<Id>, Task<Self::Message>);

    fn opened(&mut self, id: Id, size: Size) -> Task<Self::Message>;

    fn close(&mut self, id: Id) -> Task<Self::Message>;

    fn closed(&mut self, id: Id) -> Task<Self::Message>;

    fn resize(&mut self, id: Id, size: Size) -> Task<Self::Message>;

    fn mv(&mut self, id: Id, position: Point) -> Task<Self::Message>;

    fn position(&self, id: Id) -> Option<Point>;

    fn placement(&self, id: Id) -> Option<Placement>;

    fn appearance(&self, theme: &Theme, id: Id) -> Self::Appearance;

    /// screen size with exclusive zone
    fn screen_size(&self) -> Size;

    fn set_mode(&mut self, mode: WindowManagerMode) -> bool;

    fn mode(&self) -> WindowManagerMode;

    fn set_preferred_output_name(&mut self, preferred_output_name: &str);

    /// Return a list of output's name and its description
    fn outputs(&self) -> Vec<(String, String)>;

    fn sync_output(&mut self) -> Vec<SyncOutputResponse>;
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum WindowManagerMode {
    #[default]
    Normal,
    KwinLockScreen,
}

#[derive(Debug)]
pub struct WindowSettings {
    application_id: String,
    size: Size,
    placement: Placement,
    position: Point,
}

impl WindowSettings {
    pub fn new(size: Size, placement: Placement) -> Self {
        Self {
            // TODO don't hardcode
            application_id: "fcitx5-osk".to_string(),
            size,
            placement,
            position: Point::ORIGIN,
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
