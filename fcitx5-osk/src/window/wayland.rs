use std::{collections::HashMap, env, mem};

use iced::{
    window::{self as iced_window, Id},
    Color, Point, Size, Task, Theme,
};
use iced_layershell::{
    reexport::{
        Anchor, KeyboardInteractivity, Layer, NewInputPanelSettings, NewLayerShellSettings,
        OutputOption,
    },
    Appearance, DefaultStyle,
};

use crate::{
    app::{
        wayland::{OutputContext, OutputGeometry, WaylandMessage},
        Message,
    },
    config::Placement,
    has_text_within_env,
    window::{WindowAppearance, WindowManager, WindowManagerMode, WindowSettings},
};

use super::SyncOutputResponse;

pub fn is_available() -> bool {
    has_text_within_env("WAYLAND_DISPLAY") || has_text_within_env("WAYLAND_SOCKET")
}

pub unsafe fn set_env(socket: Option<&str>, display: Option<&str>) {
    if let Some(socket) = socket {
        env::set_var("WAYLAND_SOCKET", socket);
    } else {
        env::remove_var("WAYLAND_SOCKET");
    }
    if let Some(display) = display {
        env::set_var("WAYLAND_DISPLAY", display);
    } else {
        env::remove_var("WAYLAND_DISPLAY");
    }
    tracing::debug!(
        "socket: {:?}, display: {:?}",
        env::var("WAYLAND_SOCKET"),
        env::var("WAYLAND_DISPLAY")
    );
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

pub struct WaylandWindowManager {
    settings: HashMap<Id, WindowSettings>,
    screen_size: Size,
    exclusive_zone: Option<u32>,
    mode: WindowManagerMode,
    output_context: OutputContext,
    preferred_output_name: Option<String>,
    selected_output: Option<OutputGeometry>,
}

type Margin = (i32, i32, i32, i32);

impl WaylandWindowManager {
    pub fn new(output_context: OutputContext, preferred_output_name: Option<String>) -> Self {
        Self {
            settings: Default::default(),
            screen_size: Size::new(1024., 768.),
            exclusive_zone: Default::default(),
            mode: Default::default(),
            output_context,
            preferred_output_name,
            selected_output: Default::default(),
        }
    }

    /// make sure size and position are valid.
    fn fix_settings(settings: &mut WindowSettings, screen_size: Size) {
        let size = &mut settings.size;
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

    fn open_window(&mut self, mut settings: WindowSettings) -> (Option<Id>, Task<WaylandMessage>) {
        let Some(selected_output) = &self.selected_output else {
            tracing::warn!("Can't find an output");
            return (None, Message::from_nothing());
        };

        Self::fix_settings(&mut settings, self.movable_screen_size());
        let size = (settings.size.width as u32, settings.size.height as u32);
        if size.0 == 0 || size.1 == 0 {
            // Setting width or height to 0 and without correct anchor will cause protocol
            // error in Kwin6
            tracing::warn!("Invalid size: {:?}", size);
            return (None, Message::from_nothing());
        }
        let placement = settings.placement;
        let (anchor, exclusive_zone) = match placement {
            Placement::Dock => {
                if let Some(exclusive_zone) = self.exclusive_zone {
                    tracing::error!(
                        "Multiple dock windows, there is already one dock with height: {}",
                        exclusive_zone
                    );
                    return (None, Message::from_nothing());
                }
                self.exclusive_zone = Some(size.1);
                (Anchor::Bottom, self.exclusive_zone.map(|n| n as i32))
            }
            Placement::Float => {
                // In kwin, if you anchor all edges, and doesn't set size, the final window size
                // will be the size of the screen subtract the size of margin. In the meantime, if
                // there is a exclusive zone the size of the screen will be smaller. And if the
                // size of margin is greater than the size of the screen, kwin will close the
                // window which makes functions, such as `set_margin`, unavailable.
                (Anchor::Top | Anchor::Left, None)
            }
        };
        let margin = self.margin(&settings);
        let id = Id::unique();

        let output_option = OutputOption::Output(selected_output.output.clone());

        self.settings.insert(id, settings);

        let task = if self.mode == WindowManagerMode::KwinLockScreen && placement == Placement::Dock
        {
            tracing::debug!("Open window[{id}] as input panel surface");
            // create input panel surface, so that it can be shown by kwin in lock screen
            Task::done(WaylandMessage::NewInputPanel {
                settings: NewInputPanelSettings {
                    size,
                    keyboard: true,
                    output_option,
                },
                id,
            })
        } else {
            tracing::debug!("Open window[{id}] as layer shell surface");
            Task::done(WaylandMessage::NewLayerShell {
                settings: NewLayerShellSettings {
                    size: Some(size),
                    exclusive_zone,
                    anchor,
                    layer: Layer::Overlay,
                    margin,
                    keyboard_interactivity: KeyboardInteractivity::None,
                    output_option,
                    events_transparent: false,
                },
                id,
            })
        };

        (Some(id), task)
    }

    fn margin(&self, settings: &WindowSettings) -> Option<Margin> {
        let size = settings.size;
        if settings.placement != Placement::Float {
            None
        } else {
            let position = settings.position;
            let movable_screen_size = self.movable_screen_size();
            let margin = (
                // top
                position.y.floor() as i32,
                // right
                (movable_screen_size.width - position.x - size.width).floor() as i32,
                // bottom
                (movable_screen_size.height - position.y - size.height).floor() as i32,
                // left
                position.x.floor() as i32,
            );
            Some(margin)
        }
    }

    fn set_margin(&mut self, id: Id, margin: Margin) -> Task<WaylandMessage> {
        tracing::debug!("set margin of window[{}]: {:?}", id, margin);
        Task::done(WaylandMessage::MarginChange { id, margin })
    }

    fn movable_screen_size(&self) -> Size {
        movable_screen_size(&self.screen_size, &self.exclusive_zone)
    }
}

impl WindowManager for WaylandWindowManager {
    type Message = WaylandMessage;

    type Appearance = Appearance;

    fn nothing() -> Task<Self::Message> {
        Message::from_nothing()
    }

    fn open(&mut self, settings: WindowSettings) -> (Option<Id>, Task<Self::Message>) {
        self.open_window(settings)
    }

    fn opened(&mut self, _id: Id, _size: Size) -> Task<Self::Message> {
        Message::from_nothing()
    }

    fn close(&mut self, id: Id) -> Task<Self::Message> {
        iced_window::close(id)
    }

    fn closed(&mut self, id: Id) -> Task<Self::Message> {
        if let Some(settings) = self.settings.remove(&id) {
            if settings.placement == Placement::Dock {
                // In Kwin6, the output change event is before the closed event, so we should reset
                // exclusive_zone at the beginning of closing the keyboard to have a correct output
                // logical_height
                self.exclusive_zone.take();
            }
        }
        Message::from_nothing()
    }

    fn resize(&mut self, id: Id, size: Size) -> Task<Self::Message> {
        let old_size = if let Some(settings) = self.settings.get_mut(&id) {
            let old_size = mem::replace(&mut settings.size, size);
            if settings.placement == Placement::Dock {
                // it should use screen_size to fix the settings in Dock mode
                Self::fix_settings(settings, self.screen_size);
            } else {
                Self::fix_settings(
                    settings,
                    movable_screen_size(&self.screen_size, &self.exclusive_zone),
                );
            }
            Some(old_size)
        } else {
            None
        };
        tracing::debug!("resize window[{}] from[{:?}] to [{:?}]", id, old_size, size);
        if let Some(settings) = self.settings.get(&id) {
            // use the size after fixing
            let size = settings.size;
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
                self.exclusive_zone = Some(size.height as u32);
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
            Self::fix_settings(
                settings,
                movable_screen_size(&self.screen_size, &self.exclusive_zone),
            );
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
                    let movable_screen_size = self.movable_screen_size();
                    let size = settings.size;
                    (
                        (movable_screen_size.width - size.width) / 2.,
                        movable_screen_size.height - size.height,
                    )
                        .into()
                }
                Placement::Float => settings.position,
            })
    }

    fn placement(&self, id: Id) -> Option<Placement> {
        self.settings.get(&id).map(WindowSettings::placement)
    }

    fn appearance(&self, theme: &Theme, _id: Id) -> Self::Appearance {
        let mut appearance = Self::Appearance::default(theme);
        appearance.set_background_color(Color::TRANSPARENT);
        appearance
    }

    fn screen_size(&self) -> Size {
        self.screen_size
    }

    fn set_mode(&mut self, mode: WindowManagerMode) -> bool {
        // can't change to other mode, once it is KwinLockScreen
        if self.mode != WindowManagerMode::KwinLockScreen && self.mode != mode {
            self.mode = mode;
            true
        } else {
            false
        }
    }

    fn mode(&self) -> WindowManagerMode {
        self.mode
    }

    fn set_preferred_output_name(&mut self, preferred_output_name: &str) {
        self.preferred_output_name = Some(preferred_output_name.to_string());
    }

    fn outputs(&self) -> Vec<(String, String)> {
        self.output_context.outputs()
    }

    fn sync_output(&mut self) -> Vec<SyncOutputResponse> {
        let preferred_output_name = self.preferred_output_name.as_deref();
        let output = self.output_context.select_output(preferred_output_name);
        let mut res = vec![];
        match (output, &mut self.selected_output) {
            (Some(mut output), Some(selected_output)) => {
                if output.output != selected_output.output {
                    res.push(SyncOutputResponse::OutputChanged);
                } else if let Some(exclusive_zone) = self.exclusive_zone {
                    if output.logical_width == selected_output.logical_width
                        && output.logical_height + exclusive_zone == selected_output.logical_height
                    {
                        // If there is no new_exclusive_zone event, check if the exclusive_zone
                        // should be added
                        output.logical_height = selected_output.logical_height;
                    } else if output.transform != selected_output.transform {
                        // If the screen is rotated, add the exclusive_zone
                        output.logical_height += exclusive_zone;
                    }
                }
                if output.logical_width != selected_output.logical_width
                    || output.logical_height != selected_output.logical_height
                {
                    let old_screen_size = self.screen_size;
                    self.screen_size =
                        Size::new(output.logical_width as f32, output.logical_height as f32);
                    tracing::debug!(
                        "Screen size is changed from {:?} to {:?}",
                        old_screen_size,
                        self.screen_size
                    );
                    res.push(SyncOutputResponse::SizeChanged);
                }
                if output.scale_factor != selected_output.scale_factor {
                    res.push(SyncOutputResponse::ScaleFactorChanged(output.scale_factor));
                }
                if output.transform != selected_output.transform {
                    res.push(SyncOutputResponse::RotationChanged);
                }
                *selected_output = output;
            }
            (Some(output), None) => {
                res.push(SyncOutputResponse::OutputChanged);
                res.push(SyncOutputResponse::SizeChanged);
                res.push(SyncOutputResponse::ScaleFactorChanged(output.scale_factor));
                self.screen_size =
                    Size::new(output.logical_width as f32, output.logical_height as f32);
                self.selected_output = Some(output);
            }
            (None, Some(_)) => {
                res.push(SyncOutputResponse::OutputChanged);
                self.selected_output = None;
            }
            (None, None) => {}
        }
        res
    }
}

fn movable_screen_size(screen_size: &Size, exclusive_zone: &Option<u32>) -> Size {
    if let Some(exclusive_zone) = exclusive_zone {
        Size::new(
            screen_size.width,
            screen_size.height - *exclusive_zone as f32,
        )
    } else {
        *screen_size
    }
}
