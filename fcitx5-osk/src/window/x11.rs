use std::{
    collections::{HashMap, HashSet},
    env,
    sync::Arc,
};

use anyhow::Result;
use iced::{
    daemon::{Appearance, DefaultStyle},
    window::{self as iced_window, settings::PlatformSpecific, Id, Level, Position, Settings},
    Color, Point, Size, Task, Theme,
};
use x11rb::{
    connection::Connection,
    properties::WmHints,
    protocol::xproto::{self, ConnectionExt},
    rust_connection::RustConnection,
    wrapper::ConnectionExt as WrapperConnectionExt,
};

use crate::{
    app::Message,
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

pub struct X11WindowManager {
    settings: HashMap<Id, WindowSettings>,
    internals: HashSet<Id>,
    screen_size: Size,
    conn: Arc<RustConnection>,
    default_screen: usize,
    atoms: Arc<Atoms>,
}

impl Default for X11WindowManager {
    fn default() -> Self {
        let (conn, default_screen) = xcb_connection().expect("unable to get x11 connection");
        let atoms = atoms(&conn)
            .map(Arc::new)
            .expect("unable to get atoms from x11");
        Self {
            settings: Default::default(),
            internals: Default::default(),
            screen_size: Default::default(),
            conn: Arc::new(conn),
            default_screen,
            atoms,
        }
    }
}

impl X11WindowManager {
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

    fn open_window(&mut self, mut settings: WindowSettings) -> (Id, Task<Message>) {
        let position = if settings.internal {
            Position::Centered
        } else if settings.placement == Placement::Dock {
            Position::SpecificWith(|size, screen_size| {
                (
                    (screen_size.width - size.width) / 2.,
                    screen_size.height - size.height,
                )
                    .into()
            })
        } else {
            Position::Specific(settings.position)
        };
        Self::fix_settings(&mut settings, self.screen_size);
        let iced_settings = Settings {
            size: settings.size.unwrap_or_else(|| Size::new(1., 1.)),
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
        if settings.internal {
            self.internals.insert(id);
        }
        self.settings.insert(id, settings);
        (id, task.map(|_| Message::Nothing))
    }
}

impl WindowManager for X11WindowManager {
    type Message = Message;

    type Appearance = Appearance;

    fn nothing() -> Task<Self::Message> {
        Message::from_nothing()
    }

    fn open(&mut self, settings: WindowSettings) -> (Id, Task<Self::Message>) {
        self.open_window(settings)
    }

    fn opened(&mut self, id: Id, _size: Size) -> Task<Self::Message> {
        if self.internals.contains(&id) {
            iced_window::get_position(id).then(move |position| {
                let position = position.expect("unable to get the position of the window");
                let size = Size::new(position.x * 2., position.y * 2.);
                iced_window::get_scale_factor(id)
                    .map(move |scale_factor| {
                        Message::from(WindowManagerEvent::ScreenInfo(size, scale_factor))
                    })
                    .chain(iced_window::close(id))
            })
        } else {
            // close all internals
            let mut task = Message::nothing();
            for id in &self.internals {
                tracing::debug!("closing internal window: {}", id);
                task = task.chain(iced_window::close(*id));
            }
            let exclusive_zone = self.settings.get(&id).and_then(|s| {
                s.size
                    .filter(|_| s.placement == Placement::Dock)
                    .map(|size| size.height)
            });
            let conn = self.conn.clone();
            let default_screen = self.default_screen;
            let atoms = self.atoms.clone();
            task = task.chain(
                iced_window::get_raw_id::<Self::Message>(id)
                    .map(move |raw_id| {
                        let x_window_id = raw_id as xproto::Window;
                        if let Err(err) =
                            init_window(&conn, default_screen, x_window_id, &atoms, exclusive_zone)
                        {
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
    }

    fn close(&mut self, id: Id) -> Task<Self::Message> {
        iced_window::close(id)
    }

    fn closed(&mut self, id: Id) -> Task<Self::Message> {
        self.settings.remove(&id);
        self.internals.remove(&id);
        Self::Message::nothing()
    }

    fn resize(&mut self, id: Id, size: Size) -> Task<Self::Message> {
        let mut task = Self::Message::nothing();
        if let Some(settings) = self.settings.get_mut(&id) {
            settings.size.replace(size);
            Self::fix_settings(settings, self.screen_size);
            if let Some(size) = settings.size {
                task = iced_window::resize(id, size);
                if settings.placement == Placement::Dock {
                    let conn = self.conn.clone();
                    let atoms = self.atoms.clone();
                    settings.position = (
                        (self.screen_size.width - size.width) / 2.,
                        self.screen_size.height - size.height,
                    )
                        .into();
                    task = task.chain(iced_window::get_raw_id::<Self::Message>(id).map(
                        move |raw_id| {
                            let x_window_id = raw_id as xproto::Window;
                            if let Err(err) =
                                set_exclusive_zone(&conn, x_window_id, &atoms, size.height as u32)
                            {
                                tracing::error!("failed to set exclusive zone: {:?}", err);
                            }
                            Message::Nothing
                        },
                    ))
                }
            }
            // position may be changed after resized.
            let new_position = settings.position;
            let _ = settings;
            task = task.chain(self.mv(id, new_position));
        };
        task
    }

    fn mv(&mut self, id: Id, position: Point) -> Task<Self::Message> {
        let mut task = Self::Message::nothing();
        if let Some(settings) = self.settings.get_mut(&id) {
            settings.position = position;
            Self::fix_settings(settings, self.screen_size);
            task = iced_window::move_to(id, settings.position);
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

fn xcb_connection() -> Result<(RustConnection, usize)> {
    let (conn, default_screen) = x11rb::connect(None)?;
    Ok((conn, default_screen))
}

fn atoms(conn: &RustConnection) -> Result<Atoms> {
    Ok(Atoms::new(conn)?.reply()?)
}

fn send_event<T>(
    conn: &RustConnection,
    screen: &xproto::Screen,
    x_window_id: xproto::Window,
    typ: xproto::Atom,
    data: T,
) -> Result<()>
where
    T: Into<xproto::ClientMessageData>,
{
    let client_message_event = xproto::ClientMessageEvent::new(32, x_window_id, typ, data.into());

    conn.send_event(
        false,
        screen.root,
        xproto::EventMask::SUBSTRUCTURE_REDIRECT | xproto::EventMask::SUBSTRUCTURE_NOTIFY,
        client_message_event,
    )?;
    Ok(())
}

#[allow(unused)]
fn send_wm_state_event(
    conn: &RustConnection,
    screen: &xproto::Screen,
    x_window_id: xproto::Window,
    atoms: &Atoms,
    property: xproto::Atom,
    op: u32,
) -> Result<()> {
    // to get screen
    //let Some(screen) = conn.setup().roots.get(default_screen) else {
    //    anyhow::bail!("no screen of index: {}", default_screen);
    //};
    send_event(
        conn,
        screen,
        x_window_id,
        atoms._NET_WM_STATE,
        [op, property, 0, 0, 0],
    )
}

fn init_window(
    conn: &RustConnection,
    _default_screen: usize,
    x_window_id: xproto::Window,
    atoms: &Atoms,
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
        set_exclusive_zone(conn, x_window_id, atoms, exclusive_zone as u32)?;
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
    exclusive_zone: u32,
) -> Result<()> {
    conn.change_property32(
        xproto::PropMode::REPLACE,
        x_window_id,
        atoms._NET_WM_STRUT,
        xproto::AtomEnum::CARDINAL,
        &[0, 0, 0, exclusive_zone],
    )?
    .check()?;
    Ok(())
}
