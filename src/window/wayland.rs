use std::mem;

use iced::{
    window::{self as iced_window, Id},
    Size, Task,
};
use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer, NewLayerShellSettings};

use crate::{
    app::wayland::WaylandMessage,
    has_text_within_env,
    window::{WindowManager, WindowSettings},
};

pub fn is_available() -> bool {
    has_text_within_env("WAYLAND_DISPLAY") || has_text_within_env("WAYLAND_SOCKET")
}

#[derive(Default)]
pub struct WaylandWindowManager {
    settings: WindowSettings,
}

impl WaylandWindowManager {
    fn open_window(&self) -> (Id, Task<WaylandMessage>) {
        let size = (
            self.settings.size.width as u32,
            self.settings.size.height as u32,
        );
        let id = Id::unique();
        (
            id,
            Task::done(WaylandMessage::NewLayerShell {
                settings: NewLayerShellSettings {
                    size: Some(size),
                    exclusive_zone: Some(size.1 as i32),
                    anchor: Anchor::Bottom,
                    layer: Layer::Overlay,
                    margin: None,
                    keyboard_interactivity: KeyboardInteractivity::None,
                    use_last_output: false,
                    ..Default::default()
                },
                id,
            }),
        )
    }
}

impl WindowManager for WaylandWindowManager {
    type Message = WaylandMessage;

    fn open(&mut self, settings: WindowSettings) -> (Id, Task<Self::Message>) {
        self.settings = settings;
        self.open_window()
    }

    fn close(&mut self, window_id: Id) -> Task<Self::Message> {
        iced_window::close(window_id)
    }

    fn resize(&mut self, window_id: Id, mut size: Size) -> Task<Self::Message> {
        mem::swap(&mut self.settings.size, &mut size);
        let mut task = Task::done(Self::Message::SizeChange {
            id: window_id,
            size: (
                self.settings.size.width as u32,
                self.settings.size.height as u32,
            ),
        });
        if self.settings.size.height != size.height {
            task = task.chain(Task::done(Self::Message::ExclusiveZoneChange {
                id: window_id,
                zone_size: self.settings.size.height as i32,
            }));
        }
        task
    }
}
