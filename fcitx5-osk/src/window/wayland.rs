use std::{
    collections::{HashMap, HashSet},
    env,
};

use iced::{
    window::{self as iced_window, Id},
    Color, Point, Size, Task, Theme,
};
use iced_layershell::{
    reexport::{
        Anchor, KeyboardInteractivity, Layer, NewInputPanelSettings, NewLayerShellSettings,
    },
    Appearance, DefaultStyle,
};

use crate::{
    app::{wayland::WaylandMessage, Message},
    config::Placement,
    has_text_within_env,
    state::WindowManagerEvent,
    window::{WindowAppearance, WindowManager, WindowManagerMode, WindowSettings},
};

pub fn is_available() -> bool {
    has_text_within_env("WAYLAND_DISPLAY") || has_text_within_env("WAYLAND_SOCKET")
}

pub unsafe fn set_env(socket: Option<&str>, display: Option<&str>) {
    env::set_var("WAYLAND_SOCKET", socket.unwrap_or(""));
    env::set_var("WAYLAND_DISPLAY", display.unwrap_or(""));
}

impl WindowAppearance for Appearance {
    fn default(theme: &Theme) -> Self {
        theme.default_style()
    }

    fn set_background_color(&mut self, background_color: Color) {
        self.background_color = background_color;
    }

    fn background_color(&self) -> Color {
        self.background_color
    }
}

#[derive(Default)]
pub struct WaylandWindowManager {
    settings: HashMap<Id, WindowSettings>,
    internals: HashSet<Id>,
    screen_size: Size,
    full_screen_size: Size,
    mode: WindowManagerMode,
}

type Margin = (i32, i32, i32, i32);

impl WaylandWindowManager {
    /// make sure size and position are valid.
    fn fix_settings(settings: &mut WindowSettings, screen_size: Size) {
        if let Some(size) = settings.size.as_mut() {
            // make sure size is less than the size of the screen
            if size.width > screen_size.width {
                size.width = screen_size.width;
            }
            if size.height > screen_size.height {
                size.height = screen_size.height;
            }
            if settings.position.x > screen_size.width - size.width {
                settings.position.x = screen_size.width - size.width;
            }
            if settings.position.x < 0. {
                settings.position.x = 0.;
            }
            if settings.position.y > screen_size.height - size.height {
                settings.position.y = screen_size.height - size.height;
            }
            if settings.position.y < 0. {
                settings.position.y = 0.;
            }
        }
    }

    fn open_window(&mut self, mut settings: WindowSettings) -> (Id, Task<WaylandMessage>) {
        Self::fix_settings(&mut settings, self.screen_size);
        let size = settings.size.map(|s| (s.width as u32, s.height as u32));
        let placement = settings.placement;
        let (anchor, exclusive_zone) = match placement {
            Placement::Dock => (Anchor::Bottom, size.map(|(_, h)| h as i32)),
            Placement::Float => {
                // In kwin, if you anchor all edges, and doesn't set size, the final window size
                // will be the size of the screen subtruct the size of margin. In the meantime, if
                // there is a exclusive zone the size of the screen will be smaller. And if the
                // size of margin is greater than the size of the screen, kwin will close the
                // window which makes functions, such as `set_margin`, unavailable.
                (Anchor::Top | Anchor::Left, None)
            }
        };
        let internal = settings.internal;
        let margin = self.margin(&settings);
        let id = Id::unique();
        self.settings.insert(id, settings);
        if self.mode == WindowManagerMode::ExternalDock && placement == Placement::Dock {
            // create input panel surface, so that it can be shown by kwin in lock screen
            (
                id,
                Task::done(WaylandMessage::NewInputPanel {
                    settings: NewInputPanelSettings {
                        size: size.expect("size should not be none in dock mode"),
                        use_last_output: !internal,
                    },
                    id,
                }),
            )
        } else {
            (
                id,
                Task::done(WaylandMessage::NewLayerShell {
                    settings: NewLayerShellSettings {
                        size,
                        exclusive_zone,
                        anchor,
                        layer: Layer::Top,
                        margin,
                        keyboard_interactivity: KeyboardInteractivity::None,
                        use_last_output: !internal,
                        events_transparent: internal,
                    },
                    id,
                }),
            )
        }
    }

    fn margin(&self, settings: &WindowSettings) -> Option<Margin> {
        settings.size.and_then(|size| {
            if settings.internal || settings.placement != Placement::Float {
                None
            } else {
                let position = settings.position;
                let margin = (
                    // top
                    position.y.floor() as i32,
                    // right
                    (self.screen_size.width - position.x - size.width).floor() as i32,
                    // bottom
                    (self.screen_size.height - position.y - size.height).floor() as i32,
                    // left
                    position.x.floor() as i32,
                );
                Some(margin)
            }
        })
    }

    fn set_margin(&mut self, id: Id, margin: Margin) -> Task<WaylandMessage> {
        tracing::debug!("set margin of window[{}]: {:?}", id, margin);
        Task::done(WaylandMessage::MarginChange { id, margin })
    }
}

impl WindowManager for WaylandWindowManager {
    type Message = WaylandMessage;

    type Appearance = Appearance;

    fn nothing() -> Task<Self::Message> {
        Message::from_nothing()
    }

    fn open(&mut self, settings: WindowSettings) -> (Id, Task<Self::Message>) {
        self.open_window(settings)
    }

    fn opened(&mut self, id: Id, size: Size) -> Task<Self::Message> {
        if self.internals.contains(&id) {
            // We keep internal window opened until any other types of window is
            // opened. So they can be opened in the same screen.
            iced_window::get_scale_factor(id).map(move |scale_factor| {
                Message::from(WindowManagerEvent::ScreenInfo(size, scale_factor)).into()
            })
        } else {
            // close all internals
            let mut task = Message::from_nothing();
            for id in &self.internals {
                tracing::debug!("closing internal window: {}", id);
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
        Message::from_nothing()
    }

    fn resize(&mut self, id: Id, size: Size) -> Task<Self::Message> {
        tracing::debug!("resize window[{}] to size: {:?}", id, size);
        let old_size = if let Some(settings) = self.settings.get_mut(&id) {
            let old_size = settings.size.replace(size);
            Self::fix_settings(settings, self.screen_size);
            old_size
        } else {
            None
        };
        if let Some(settings) = self.settings.get(&id) {
            // use the size after fixing
            let Some(size) = settings.size else {
                unreachable!("size shouldn't be none");
            };
            let mut task = Task::done(Self::Message::SizeChange {
                id,
                size: (size.width as u32, size.height as u32),
            });
            if let Some(margin) = self.margin(settings) {
                task = task.chain(self.set_margin(id, margin));
            } else if old_size.map(|s| s.height) != Some(size.height)
                && settings.placement == Placement::Dock
            {
                tracing::debug!("changing exclusive zone to: {}", size.height);
                task = task.chain(Task::done(Self::Message::ExclusiveZoneChange {
                    id,
                    zone_size: size.height as i32,
                }));
            }
            return task;
        }
        Message::from_nothing()
    }

    fn mv(&mut self, id: Id, position: Point) -> Task<Self::Message> {
        if let Some(settings) = self.settings.get_mut(&id) {
            settings.position = position;
            Self::fix_settings(settings, self.screen_size);
        }
        if let Some(settings) = self.settings.get(&id) {
            if let Some(margin) = self.margin(settings) {
                return self.set_margin(id, margin);
            }
        }
        Message::from_nothing()
    }

    fn position(&self, id: Id) -> Option<Point> {
        self.settings
            .get(&id)
            .map(|settings| match settings.placement {
                Placement::Dock => {
                    let Some(size) = settings.size else {
                        unreachable!("size should be set in Dock mode");
                    };
                    (
                        (self.screen_size.width - size.width) / 2.,
                        self.screen_size.height - size.height,
                    )
                        .into()
                }
                Placement::Float => settings.position,
            })
    }

    fn placement(&self, id: Id) -> Option<Placement> {
        self.settings.get(&id).map(WindowSettings::placement)
    }

    fn fetch_screen_info(&mut self) -> Task<Self::Message> {
        let mut settings = WindowSettings::new(None, Placement::Float);
        settings.internal = true;
        let (id, task) = self.open_window(settings);
        self.internals.insert(id);
        task
    }

    fn appearance(&self, theme: &Theme, _id: Id) -> Self::Appearance {
        let mut appearance = Self::Appearance::default(theme);
        appearance.set_background_color(Color::TRANSPARENT);
        appearance
    }

    fn set_screen_size(&mut self, size: Size) -> bool {
        let res = self.screen_size != size;
        self.screen_size = size;
        let exclusive_zone = self
            .settings
            .values()
            .flat_map(|s| {
                s.size
                    // max can't apply to f32, the size of all window should be in integer.
                    .map(|size| size.height as u32)
                    .filter(|_| s.placement == Placement::Dock)
            })
            .max()
            .unwrap_or(0);
        self.full_screen_size = Size::new(
            self.screen_size.width,
            self.screen_size.height + exclusive_zone as f32,
        );
        res
    }

    fn full_screen_size(&self) -> Size {
        self.full_screen_size
    }

    fn set_mode(&mut self, mode: WindowManagerMode) -> bool {
        if self.mode != mode {
            self.mode = mode;
            true
        } else {
            false
        }
    }

    fn mode(&self) -> WindowManagerMode {
        self.mode
    }
}
