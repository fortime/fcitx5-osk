use std::{
    collections::{HashMap, HashSet},
    mem,
};

use iced::{
    window::{self as iced_window, Id},
    Color, Size, Task, Theme,
};
use iced_layershell::{
    reexport::{Anchor, KeyboardInteractivity, Layer, NewLayerShellSettings},
    Appearance, DefaultStyle,
};

use crate::{
    app::{wayland::WaylandMessage, Message},
    config::Placement,
    has_text_within_env,
    state::WindowManagerEvent,
    window::{WindowAppearance, WindowManager, WindowSettings},
};

pub fn is_available() -> bool {
    has_text_within_env("WAYLAND_DISPLAY") || has_text_within_env("WAYLAND_SOCKET")
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
pub struct WaylandWindowManager {
    settings: HashMap<Id, WindowSettings>,
    internals: HashSet<Id>,
}

impl WaylandWindowManager {
    fn open_window(&mut self, settings: WindowSettings) -> (Id, Task<WaylandMessage>) {
        let size = settings.size.map(|s| (s.width as u32, s.height as u32));
        let (anchor, exclusive_zone) = match settings.placement {
            Placement::Dock => (Anchor::Bottom, size.map(|(_, h)| h as i32)),
            Placement::Float => (Anchor::Bottom | Anchor::Left | Anchor::Right, None),
        };
        let use_last_output = settings.use_last_output;
        let id = Id::unique();
        self.settings.insert(id, settings);
        (
            id,
            Task::done(WaylandMessage::NewLayerShell {
                settings: NewLayerShellSettings {
                    size,
                    exclusive_zone,
                    anchor,
                    layer: Layer::Top,
                    margin: None,
                    keyboard_interactivity: KeyboardInteractivity::None,
                    use_last_output,
                    ..Default::default()
                },
                id,
            }),
        )
    }
}

impl WindowManager for WaylandWindowManager {
    type Message = WaylandMessage;

    type Appearance = Appearance;

    fn open(&mut self, settings: WindowSettings) -> (Id, Task<Self::Message>) {
        self.open_window(settings)
    }

    fn opened(&mut self, id: Id, size: Size) -> Task<Self::Message> {
        if self.internals.contains(&id) {
            // We keep internal window opened until any other types of window is
            // opened. So they can be opened in the same screen.
            iced_window::get_scale_factor(id)
                .map(move |scale_factor| {
                    Message::from(WindowManagerEvent::ScreenInfo(size, scale_factor)).into()
                })
        } else {
            // close all internals
            let mut task = Task::none();
            for id in &self.internals {
                task = task.chain(iced_window::close(*id));
            }
            task
        }
    }

    fn close(&mut self, id: Id) -> Task<Self::Message> {
        iced_window::close(id)
    }

    fn closed(&mut self, id: Id) -> Task<Self::Message> {
        self.settings.remove(&id);
        self.internals.remove(&id);
        Task::none()
    }

    fn resize(&mut self, id: Id, size: Size) -> Task<Self::Message> {
        if let Some(settings) = self.settings.get_mut(&id) {
            let mut task = Task::done(Self::Message::SizeChange {
                id,
                size: (size.width as u32, size.height as u32),
            });
            if settings.size.map(|s| s.height) != Some(size.height)
                && settings.placement == Placement::Dock
            {
                task = task.chain(Task::done(Self::Message::ExclusiveZoneChange {
                    id,
                    zone_size: size.height as i32,
                }));
            }
            let _ = settings.size.replace(size);
            task
        } else {
            Task::none()
        }
    }

    fn fetch_screen_info(&mut self) -> Task<Self::Message> {
        let mut settings = WindowSettings::new(None, Placement::Float);
        settings.use_last_output = false;
        let (id, task) = self.open_window(settings);
        self.internals.insert(id);
        task
    }

    fn appearance(&self, theme: &Theme, _id: Id) -> Self::Appearance {
        let mut appearance = Self::Appearance::default(theme);
        appearance.set_background_color(Color::TRANSPARENT);
        appearance
    }
}
