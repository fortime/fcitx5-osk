use iced::{window::Id, Size, Task};

use crate::config::Placement;

pub mod wayland;
pub mod x11;

pub trait WindowManager: Default {
    type Message;

    fn open(&mut self, settings: WindowSettings) -> (Id, Task<Self::Message>);

    fn close(&mut self, window_id: Id) -> Task<Self::Message>;

    fn resize(&mut self, window_id: Id, size: Size) -> Task<Self::Message>;
}

#[derive(Default)]
pub struct WindowSettings {
    size: Size,
    placement: Placement,
}

impl WindowSettings {
    pub fn new(size: Size, placement: Placement) -> Self {
        Self { size, placement }
    }

    pub fn application_id(&self) -> &'static str {
        // TODO no need?
        // set WM_CLASS (resourceClass) for different placement.
        // tested in kde6.
        match self.placement {
            Placement::Dock => "fcitx5-osk-dock",
            Placement::Float => "fcitx5-osk-float",
        }
    }
}
