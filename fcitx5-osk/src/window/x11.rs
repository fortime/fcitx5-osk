use std::{collections::HashMap, env, sync::Arc};

use anyhow::Result;
use iced::{
    daemon::{Appearance, DefaultStyle},
    window::{self as iced_window, settings::PlatformSpecific, Id, Level, Position, Settings},
    Color, Point, Size, Task, Theme,
};
use x11rb::{
    connection::Connection, properties::WmHints, protocol::xproto, rust_connection::RustConnection,
    wrapper::ConnectionExt as WrapperConnectionExt,
};

use crate::{
    app::{
        x11::{OutputContext, OutputGeometry},
        Message,
    },
    config::Placement,
    has_text_within_env,
    state::WindowManagerEvent,
    window::{WindowAppearance, WindowManager, WindowManagerMode, WindowSettings},
};

x11rb::atom_manager! {
    /// A collection of Atoms.
    pub Atoms:
    /// A handle to a response from the X11 server.
    AtomsCookie {
        _NET_WM_WINDOW_TYPE,
        _NET_WM_WINDOW_TYPE_DOCK,
        _NET_WM_WINDOW_TYPE_UTILITY,
        _NET_WM_STATE,
        _NET_WM_STATE_SKIP_TASKBAR,
        _NET_WM_STATE_SKIP_PAGER,
        _NET_WM_STRUT,
        _NET_WM_STRUT_PARTIAL,
    }
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

struct X11State {
    conn: RustConnection,
    atoms: Atoms,
}

pub struct X11WindowManager {
    settings: HashMap<Id, WindowSettings>,
    screen_size: Size,
    x11_state: Option<Arc<X11State>>,
    connection_supplier: Box<dyn Fn() -> Result<(RustConnection, usize)>>,
    output_context: OutputContext,
    preferred_output_name: Option<String>,
    selected_output_geometry: Option<OutputGeometry>,
}

impl X11WindowManager {
    pub fn new<F>(
        connection_supplier: F,
        output_context: OutputContext,
        preferred_output_name: Option<String>,
    ) -> Self
    where
        F: Fn() -> Result<(RustConnection, usize)> + 'static,
    {
        Self {
            settings: Default::default(),
            screen_size: Default::default(),
            x11_state: Default::default(),
            connection_supplier: Box::new(connection_supplier),
            output_context,
            preferred_output_name,
            selected_output_geometry: None,
        }
    }

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

    fn select_output(&mut self) -> Option<Task<Message>> {
        // Use preferred_output_name to select output
        let preferred_output_name = self.preferred_output_name.as_deref();
        let x11_output_res = self.output_context.select_output(preferred_output_name);
        let mut task: Option<Task<Message>> = None;
        if let Some(output_geometry) = x11_output_res {
            let close_window = self
                .selected_output_geometry
                .as_ref()
                .filter(|o| o.x11_output == output_geometry.x11_output)
                .is_none();
            // Update selected_output_name
            self.screen_size = output_geometry.logical_size();
            self.selected_output_geometry = Some(output_geometry);
            if close_window {
                let ids: Vec<_> = self.settings.keys().copied().collect();
                for id in ids {
                    tracing::debug!("Close window[{}] because selected output is changed", id);
                    if let Some(t) = task {
                        task = Some(t.chain(self.close(id)));
                    } else {
                        task = Some(self.close(id));
                    }
                }
            }
        } else {
            // no output
            self.selected_output_geometry = None;
        }
        task
    }

    fn open_window(&mut self, mut settings: WindowSettings) -> (Option<Id>, Task<Message>) {
        let closing_window_task = self.select_output();
        let Some(output_geometry) = &self.selected_output_geometry else {
            tracing::error!("Can't find an output");
            return (None, Message::nothing());
        };
        Self::fix_settings(&mut settings, self.screen_size);
        let size = settings.size.unwrap_or_else(|| Size::new(1., 1.));
        let position =
            global_position(output_geometry, settings.position, size, settings.placement);
        let iced_settings = Settings {
            size,
            position,
            // invisible until `init_window` is called, so it won't steal focus.
            visible: false,
            resizable: true,
            decorations: false,
            transparent: true,
            level: Level::AlwaysOnTop,
            platform_specific: PlatformSpecific {
                application_id: settings.application_id.clone(),
                override_redirect: false,
            },
            ..Default::default()
        };
        let (id, task) = iced_window::open(iced_settings);
        tracing::debug!("Open window[{}] in position: {:?}", id, position);
        self.settings.insert(id, settings);
        if let Some(closing_window_task) = closing_window_task {
            (
                Some(id),
                closing_window_task.chain(task.map(|_| Message::Nothing)),
            )
        } else {
            (Some(id), task.map(|_| Message::Nothing))
        }
    }

    fn x11_state(&mut self) -> Result<Arc<X11State>> {
        if let Some(x11_state) = self.x11_state.clone() {
            Ok(x11_state)
        } else {
            let conn = (self.connection_supplier)()?.0;
            let atoms = Atoms::new(&conn)?.reply()?;
            let x11_state = Arc::new(X11State { conn, atoms });
            self.x11_state = Some(x11_state.clone());
            Ok(x11_state)
        }
    }
}

impl WindowManager for X11WindowManager {
    type Message = Message;

    type Appearance = Appearance;

    fn nothing() -> Task<Self::Message> {
        Message::from_nothing()
    }

    fn open(&mut self, settings: WindowSettings) -> (Option<Id>, Task<Self::Message>) {
        self.open_window(settings)
    }

    fn opened(&mut self, id: Id, _size: Size) -> Task<Self::Message> {
        let x11_state = self.x11_state().expect("Unable to to generate X11State");
        let mut task = Message::nothing();
        let exclusive_zone = self.settings.get(&id).and_then(|s| {
            s.size
                .filter(|_| s.placement == Placement::Dock)
                .map(|size| size.height)
        });
        let full_screen_size = self.full_screen_size();
        task = task.chain(
            iced_window::get_raw_id::<Self::Message>(id)
                .map(move |raw_id| {
                    let x_window_id = raw_id as xproto::Window;
                    if let Err(err) = init_window(
                        &x11_state.conn,
                        x_window_id,
                        &x11_state.atoms,
                        full_screen_size,
                        exclusive_zone,
                    ) {
                        tracing::error!("failed to init a x11 window: {:?}", err);
                    }
                })
                .then(move |_| iced_window::change_mode(id, iced_window::Mode::Windowed))
                .chain(iced_window::change_level(
                    id,
                    iced_window::Level::AlwaysOnTop,
                )),
        );
        task
    }

    fn close(&mut self, id: Id) -> Task<Self::Message> {
        iced_window::close(id)
    }

    fn closed(&mut self, id: Id) -> Task<Self::Message> {
        self.settings.remove(&id);
        Self::Message::nothing()
    }

    fn resize(&mut self, id: Id, size: Size) -> Task<Self::Message> {
        let mut task = Self::Message::nothing();
        if let Some(settings) = self.settings.get_mut(&id) {
            settings.size.replace(size);
            Self::fix_settings(settings, self.screen_size);
            let mut new_position = settings.position;
            if let Some(size) = settings.size {
                task = iced_window::resize(id, size);
                if settings.placement == Placement::Dock {
                    settings.position = (
                        (self.screen_size.width - size.width) / 2.,
                        self.screen_size.height - size.height,
                    )
                        .into();
                    // position may be changed after resized.
                    new_position = settings.position;
                    let x11_state = self.x11_state().expect("Unable to to generate X11State");
                    let full_screen_size = self.full_screen_size();
                    task = task.chain(iced_window::get_raw_id::<Self::Message>(id).map(
                        move |raw_id| {
                            let x_window_id = raw_id as xproto::Window;
                            if let Err(err) = set_exclusive_zone(
                                &x11_state.conn,
                                x_window_id,
                                &x11_state.atoms,
                                full_screen_size,
                                size.height as u32,
                            ) {
                                tracing::error!("failed to set exclusive zone: {:?}", err);
                            }
                            Message::Nothing
                        },
                    ))
                }
            }
            task = task.chain(self.mv(id, new_position));
        };
        task
    }

    fn mv(&mut self, id: Id, position: Point) -> Task<Self::Message> {
        let mut task = Self::Message::nothing();
        if let Some(settings) = self.settings.get_mut(&id) {
            settings.position = position;
            Self::fix_settings(settings, self.screen_size);
            let position = if let Some(output_geometry) = &self.selected_output_geometry {
                settings.position + output_geometry.logical_alignment()
            } else {
                settings.position
            };
            task = iced_window::move_to(id, position);
        };
        task
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
        let task = self.select_output();
        if let Some(output_geometry) = &self.selected_output_geometry {
            let screen_info_task = Task::done(Message::from(WindowManagerEvent::ScreenInfo(
                output_geometry.logical_size(),
                output_geometry.scale_factor as f32,
            )));
            if let Some(task) = task {
                task.chain(screen_info_task)
            } else {
                screen_info_task
            }
        } else {
            Message::nothing()
        }
    }

    fn appearance(&self, theme: &Theme, _id: Id) -> Self::Appearance {
        let mut appearance = Self::Appearance::default(theme);
        appearance.set_background_color(Color::TRANSPARENT);
        appearance
    }

    fn set_screen_size(&mut self, size: Size) -> bool {
        let res = self.screen_size != size;
        self.screen_size = size;
        res
    }

    fn full_screen_size(&self) -> Size {
        self.screen_size
    }

    /// ignore mode in x11
    fn set_mode(&mut self, _: WindowManagerMode) -> bool {
        false
    }

    /// always returns WindowManagerMode::Normal
    fn mode(&self) -> WindowManagerMode {
        Default::default()
    }

    fn set_preferred_output_name(&mut self, preferred_output_name: &str) {
        self.preferred_output_name = Some(preferred_output_name.to_string());
    }

    fn outputs(&self) -> Vec<(String, String)> {
        self.output_context.outputs()
    }
}

pub fn is_available() -> bool {
    has_text_within_env("DISPLAY")
}

pub unsafe fn set_env(display: Option<&str>) {
    if let Some(display) = display {
        env::set_var("Display", display);
    } else {
        env::remove_var("Display");
    }
}

fn init_window(
    conn: &RustConnection,
    x_window_id: xproto::Window,
    atoms: &Atoms,
    full_screen_size: Size,
    exclusive_zone: Option<f32>,
) -> Result<()> {
    // not accept focus
    let mut wm_hints = WmHints::get(&conn, x_window_id)?
        .reply()?
        .unwrap_or_else(WmHints::new);
    wm_hints.input = Some(false);
    wm_hints.set(&conn, x_window_id)?.check()?;

    if let Some(exclusive_zone) = exclusive_zone {
        // change to a dock
        conn.change_property32(
            xproto::PropMode::REPLACE,
            x_window_id,
            atoms._NET_WM_WINDOW_TYPE,
            xproto::AtomEnum::ATOM,
            &[atoms._NET_WM_WINDOW_TYPE_DOCK],
        )?
        .check()?;

        // reserve space
        set_exclusive_zone(
            conn,
            x_window_id,
            atoms,
            full_screen_size,
            exclusive_zone as u32,
        )?;
    } else {
        conn.change_property32(
            xproto::PropMode::REPLACE,
            x_window_id,
            atoms._NET_WM_WINDOW_TYPE,
            xproto::AtomEnum::ATOM,
            &[atoms._NET_WM_WINDOW_TYPE_UTILITY],
        )?;
    }
    // SKIP only works in specific window types.
    conn.change_property32(
        xproto::PropMode::REPLACE,
        x_window_id,
        atoms._NET_WM_STATE,
        xproto::AtomEnum::ATOM,
        &[
            atoms._NET_WM_STATE_SKIP_PAGER,
            atoms._NET_WM_STATE_SKIP_TASKBAR,
        ],
    )?;
    conn.flush()?;

    Ok(())
}

fn set_exclusive_zone(
    conn: &RustConnection,
    x_window_id: xproto::Window,
    atoms: &Atoms,
    full_screen_size: Size,
    exclusive_zone: u32,
) -> Result<()> {
    tracing::debug!("Set exclusive zone for window[{x_window_id}]: {exclusive_zone}");
    conn.change_property32(
        xproto::PropMode::REPLACE,
        x_window_id,
        atoms._NET_WM_STRUT,
        xproto::AtomEnum::CARDINAL,
        &[0, 0, 0, exclusive_zone],
    )?
    .check()?;
    conn.change_property32(
        xproto::PropMode::REPLACE,
        x_window_id,
        atoms._NET_WM_STRUT_PARTIAL,
        xproto::AtomEnum::CARDINAL,
        &[
            0,
            0,
            0,
            exclusive_zone,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            full_screen_size.width as u32,
        ],
    )?
    .check()?;
    Ok(())
}

fn global_position(
    output_geometry: &OutputGeometry,
    position: Point,
    size: Size,
    placement: Placement,
) -> Position {
    let screen_size = output_geometry.logical_size();
    let screen_alignment = output_geometry.logical_alignment();
    let position_in_screen = if placement == Placement::Dock {
        (
            (screen_size.width - size.width) / 2.,
            screen_size.height - size.height,
        )
            .into()
    } else {
        position
    };
    Position::Specific(position_in_screen + screen_alignment)
}
