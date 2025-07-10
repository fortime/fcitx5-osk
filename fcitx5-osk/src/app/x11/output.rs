use std::{
    sync::{Arc, Mutex, MutexGuard},
    thread::{self, JoinHandle},
    time::Duration,
};

use anyhow::Result;
use iced::{
    futures::{
        channel::mpsc::{self, UnboundedReceiver, UnboundedSender},
        stream,
    },
    Size, Subscription, Vector,
};
use x11rb::{
    connection::Connection as _,
    protocol::{
        randr::{self, ConnectionExt, Notify, NotifyEvent, NotifyMask, Rotation},
        xproto::{
            Atom, AtomEnum, ChangeWindowAttributesAux, ConnectionExt as _, EventMask,
            PropertyNotifyEvent, Screen,
        },
        Event as X11Event,
    },
    resource_manager::{self, Database},
    rust_connection::RustConnection,
};

use crate::{app::Message, state::WindowManagerEvent};

x11rb::atom_manager! {
    /// A collection of Atoms.
    pub Atoms:
    /// A handle to a response from the X11 server.
    AtomsCookie {
        _XSETTINGS_S0,
        _XSETTINGS_SETTINGS,
        EDID,
        RESOURCE_MANAGER,
    }
}

#[derive(Clone, Copy, Debug)]
enum Endianness {
    B,
    L,
    N,
}

struct State {
    rx: Option<UnboundedReceiver<Message>>,
    bg_handle: Option<JoinHandle<()>>,
    output_infos: Vec<OutputInfo>,
    // In X11, there is no native “scale factor” protocol. It is read from XSettings or
    // XResources.
    scale_factor: f64,
    selected_output: Option<u32>,
    closed: bool,
}

impl State {
    pub fn close(&mut self) {
        if self.closed {
            return;
        }
        tracing::debug!("Close OutputContext State");
        drop(self.bg_handle.take());
        if let Some(bg_handle) = self.bg_handle.take() {
            // Wake the thread
            bg_handle.thread().unpark();
        }
        drop(self.output_infos.drain(..));
        self.closed = true;
    }
}

impl Drop for State {
    fn drop(&mut self) {
        self.close();
    }
}

#[derive(PartialEq, Debug)]
pub struct OutputInfo {
    output: u32,
    name: String,
    description: String,
    x: i16,
    y: i16,
    physical_width: u16,
    physical_height: u16,
    rotation: Rotation,
}

#[derive(Clone)]
pub struct OutputGeometry {
    pub output: u32,
    pub x: i16,
    pub y: i16,
    pub physical_width: u16,
    pub physical_height: u16,
    pub scale_factor: f64,
    #[allow(unused)]
    pub rotation: Rotation,
}

impl OutputGeometry {
    pub fn logical_size(&self) -> Size {
        (
            (self.physical_width as f64 / self.scale_factor) as f32,
            (self.physical_height as f64 / self.scale_factor) as f32,
        )
            .into()
    }

    pub fn logical_alignment(&self) -> Vector {
        Vector::new(
            (self.x as f64 / self.scale_factor) as f32,
            (self.y as f64 / self.scale_factor) as f32,
        )
    }
}

impl From<&OutputInfo> for OutputGeometry {
    fn from(value: &OutputInfo) -> Self {
        Self {
            output: value.output,
            x: value.x,
            y: value.y,
            physical_width: value.physical_width,
            physical_height: value.physical_height,
            scale_factor: 1.,
            rotation: value.rotation,
        }
    }
}

#[derive(Clone)]
pub struct OutputContext {
    connection_supplier: Arc<dyn Fn() -> Result<(RustConnection, usize)> + Send + Sync>,
    tx: UnboundedSender<Message>,
    state: Arc<Mutex<State>>,
}

impl OutputContext {
    pub fn new<F>(connection_supplier: F) -> Result<Self>
    where
        F: Fn() -> Result<(RustConnection, usize)> + 'static + Send + Sync,
    {
        let (tx, rx) = mpsc::unbounded();
        Ok(Self {
            connection_supplier: Arc::new(connection_supplier),
            tx,
            state: Arc::new(Mutex::new(State {
                rx: Some(rx),
                bg_handle: None,
                output_infos: vec![],
                scale_factor: 1.,
                selected_output: None,
                closed: false,
            })),
        })
    }

    /// Currently, it is ok not getting state from OutputContext
    fn state(&self) -> Option<MutexGuard<'_, State>> {
        self.state.lock().ok()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        const EXTERNAL_SUBSCRIPTION_ID: &str = "external::x11_output";
        if let Some(rx) = self.state().and_then(|mut s| s.rx.take()) {
            Subscription::run_with_id(EXTERNAL_SUBSCRIPTION_ID, rx)
        } else {
            // should always return a subscription with the same id, otherwise, the first one will
            // be dropped.
            Subscription::run_with_id(EXTERNAL_SUBSCRIPTION_ID, stream::empty())
        }
    }

    pub fn select_output(&self, preferred_output_name: Option<&str>) -> Option<OutputGeometry> {
        let Some(mut guard) = self.state() else {
            tracing::debug!("Unable to select a output, the state of OutputContext is poisoned");
            return None;
        };
        tracing::debug!(
            "Try to select preferred_output_name: {:?}",
            preferred_output_name
        );
        let scale_factor = guard.scale_factor;
        let mut selected_output_info = None;
        for output_info in &guard.output_infos {
            if preferred_output_name
                .filter(|n| output_info.name == *n)
                .is_some()
            {
                tracing::debug!("Found preferred output: {}", output_info.name);
                // mark it as selected
                let mut output_geometry = OutputGeometry::from(output_info);
                guard.selected_output = Some(output_geometry.output);
                output_geometry.scale_factor = scale_factor;
                return Some(output_geometry);
            }
            if let Some(selected_output) = guard.selected_output {
                if selected_output == output_info.output {
                    selected_output_info = Some(output_info);
                }
            }
        }

        // preferred_output_name not found, use selected_output
        if let Some(selected_output_info) = selected_output_info {
            let mut output_geometry = OutputGeometry::from(selected_output_info);
            output_geometry.scale_factor = scale_factor;
            return Some(output_geometry);
        }

        // Use the first one
        if let Some(output_info) = guard.output_infos.first() {
            tracing::debug!("Use first output: {}", output_info.name);
            // mark it as selected
            let mut output_geometry = OutputGeometry::from(output_info);
            guard.selected_output = Some(output_geometry.output);
            output_geometry.scale_factor = scale_factor;
            Some(output_geometry)
        } else {
            None
        }
    }

    pub fn outputs(&self) -> Vec<(String, String)> {
        self.state()
            .map(|g| {
                g.output_infos
                    .iter()
                    .map(|o| (o.name.clone(), o.description.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn listen(&self) -> Result<()> {
        let Some(mut guard) = self.state() else {
            anyhow::bail!("Unable to listen, the state of OutputContext is poisoned");
        };
        if guard.closed {
            return Ok(());
        }
        if guard.bg_handle.is_none() {
            let bg = listen(self)?;
            guard.bg_handle = Some(thread::spawn(move || {
                if let Err(e) = bg() {
                    tracing::error!("x11 output eventloop exit with error: {:?}", e);
                }
            }));
        }
        Ok(())
    }

    pub fn close(&mut self) {
        let Some(mut guard) = self.state() else {
            tracing::debug!("Closing OutputContext, but lock is poisoned");
            return;
        };
        guard.close();
    }

    fn send_output_changed_event(&self) {
        if self
            .tx
            .unbounded_send(Message::from(WindowManagerEvent::OutputChanged))
            .is_err()
        {
            tracing::error!("Unable to send WindowManagerEvent::OutputChanged event");
        }
    }

    fn add_or_update_output_info(&self, output_info: OutputInfo) {
        tracing::debug!("Add output: {output_info:?}");
        let Some(mut guard) = self.state() else {
            tracing::warn!("Unable to add an output info, the state of OutputContext is poisoned");
            return;
        };
        if guard.closed {
            return;
        }

        let selected_changed = guard.selected_output == Some(output_info.output);
        let existed = guard
            .output_infos
            .iter()
            .position(|o| o.output == output_info.output);
        if let Some(existed) = existed {
            if guard.output_infos[existed] == output_info {
                // Do nothing if they are the same.
                return;
            }
            guard.output_infos.swap_remove(existed);
        }
        tracing::debug!(
            "Output added: {}, selected changed: {}",
            output_info.output,
            selected_changed
        );
        guard.output_infos.push(output_info);
        if selected_changed {
            self.send_output_changed_event();
        }
    }

    fn remove_output_info(&self, output: u32) {
        tracing::debug!("Remove output: {output}");
        let Some(mut guard) = self.state() else {
            tracing::warn!(
                "Unable to remove an output info, the state of OutputContext is poisoned"
            );
            return;
        };
        if guard.closed {
            return;
        }

        let selected_changed = guard.selected_output.take_if(|s| *s == output).is_some();
        let existed = guard.output_infos.iter().position(|o| o.output == output);
        tracing::debug!(
            "Output removed: {}, selected changed: {}",
            existed.is_some(),
            selected_changed
        );
        if let Some(existed) = existed {
            guard.output_infos.swap_remove(existed);
        }
        if selected_changed {
            self.send_output_changed_event();
        }
    }

    fn update_scale_factor(&self, scale_factor: f64) {
        tracing::debug!("Update scale factor: {scale_factor}");
        let Some(mut guard) = self.state() else {
            tracing::warn!("Unable to update scale factor, the state of OutputContext is poisoned");
            return;
        };
        if guard.closed {
            return;
        }

        if guard.scale_factor != scale_factor {
            tracing::debug!(
                "Scale factor is changed from {} to {}",
                guard.scale_factor,
                scale_factor,
            );
            guard.scale_factor = scale_factor;
            self.send_output_changed_event();
        }
    }
}

fn listen(output_context: &OutputContext) -> Result<impl FnOnce() -> Result<()>> {
    let (conn, default_screen) = (output_context.connection_supplier)()?;

    let atoms = Atoms::new(&conn)?.reply()?;

    let setup = conn.setup();
    let Some(screen) = setup.roots.get(default_screen) else {
        anyhow::bail!(
            "Unable to get the default screen[{}], size: {}",
            default_screen,
            setup.roots_len()
        );
    };

    let mut database = resource_manager::new_from_default(&conn)?;

    // Monitor property change event
    conn.change_window_attributes(
        screen.root,
        &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE),
    )?;

    // Make sure randr extension exits and has the minimum version
    conn.randr_query_version(1, 5)?.reply()?;

    // Select RandR events on the root window
    conn.randr_select_input(
        screen.root,
        NotifyMask::OUTPUT_CHANGE | NotifyMask::CRTC_CHANGE,
    )?;

    conn.flush()?;

    let output_context = output_context.clone();
    let screen = screen.clone();
    let bg = move || {
        // access state in another thread, otherwise, it will deadlock
        if let Some(mut guard) = output_context.state() {
            guard.output_infos = get_all_outputs(&conn, &screen, &atoms)?;
            guard.scale_factor = get_scale_factor(&conn, default_screen, &database, &atoms)?;
            // Trigger the first call of sync_output
            output_context.send_output_changed_event();
        } else {
            anyhow::bail!("Unable to get the state of OutputContext");
        }
        loop {
            if output_context.state().filter(|g| !g.closed).is_none() {
                break;
            }

            let Some(event) = conn.poll_for_event()? else {
                // wait until timeout or unpark
                thread::park_timeout(Duration::from_millis(50));
                continue;
            };

            match event {
                X11Event::RandrNotify(NotifyEvent {
                    response_type: _,
                    sub_code,
                    sequence: _,
                    u,
                }) => {
                    tracing::debug!("RandrNotify, sub_code: {sub_code:?}");
                    // EDID shouldn't be changed, so we don't monitor the change of description
                    if sub_code == Notify::CRTC_CHANGE || sub_code == Notify::RESOURCE_CHANGE {
                        let output_infos = match get_all_outputs(&conn, &screen, &atoms) {
                            Ok(output_infos) => output_infos,
                            Err(_) => {
                                // in some cases, the output isn't stable, wait a little time
                                thread::sleep(Duration::from_secs(2));
                                match get_all_outputs(&conn, &screen, &atoms) {
                                    Ok(output_infos) => output_infos,
                                    Err(e) => {
                                        tracing::error!("Getting output infos error again, {e:?}");
                                        vec![]
                                    }
                                }
                            }
                        };
                        for output_info in output_infos {
                            output_context.add_or_update_output_info(output_info);
                        }
                    } else if sub_code == Notify::OUTPUT_CHANGE {
                        let oc = u.as_oc();
                        if oc.connection == randr::Connection::CONNECTED {
                            let output_info = match get_output_info(&conn, &atoms, oc.output) {
                                Ok(output_info) => output_info,
                                Err(_) => {
                                    // in some cases, the output isn't stable, wait a little time
                                    thread::sleep(Duration::from_secs(2));
                                    match get_output_info(&conn, &atoms, oc.output) {
                                        Ok(output_info) => output_info,
                                        Err(e) => {
                                            tracing::error!(
                                                "Getting output info error again, {e:?}"
                                            );
                                            None
                                        }
                                    }
                                }
                            };
                            if let Some(output_info) = output_info {
                                output_context.add_or_update_output_info(output_info);
                            }
                        } else {
                            output_context.remove_output_info(oc.output);
                        }
                    }
                }
                X11Event::PropertyNotify(PropertyNotifyEvent {
                    response_type: _,
                    sequence: _,
                    window: _,
                    atom,
                    time: _,
                    state,
                }) => {
                    if atom == atoms.RESOURCE_MANAGER {
                        tracing::debug!("Property[RESOURCE_MANAGER] changed, state: {:?}", state,);
                        // reload database
                        database = resource_manager::new_from_default(&conn)?;
                        let scale_factor =
                            get_scale_factor(&conn, default_screen, &database, &atoms)?;
                        output_context.update_scale_factor(scale_factor);
                    }
                }
                _ => {}
            }
        }
        Ok(())
    };
    Ok(bg)
}

fn get_all_outputs(
    conn: &RustConnection,
    screen: &Screen,
    atoms: &Atoms,
) -> Result<Vec<OutputInfo>> {
    let resources = conn.randr_get_screen_resources(screen.root)?.reply()?;

    let mut output_infos = vec![];

    // Iterate over CRTCs and get output info
    for crtc in resources.crtcs {
        let crtc_info = conn
            .randr_get_crtc_info(crtc, x11rb::CURRENT_TIME)?
            .reply()?;

        for output in crtc_info.outputs {
            if let Some(output_info) = get_output_info(conn, atoms, output)? {
                output_infos.push(output_info);
            }
        }
    }
    Ok(output_infos)
}

fn get_output_info(
    conn: &RustConnection,
    atoms: &Atoms,
    output: u32,
) -> Result<Option<OutputInfo>> {
    let output_info = conn
        .randr_get_output_info(output, x11rb::CURRENT_TIME)?
        .reply()?;
    if output_info.connection != randr::Connection::CONNECTED {
        return Ok(None);
    }

    let crtc_info = conn
        .randr_get_crtc_info(output_info.crtc, x11rb::CURRENT_TIME)?
        .reply()?;

    let output_properties = conn.randr_list_output_properties(output)?.reply()?;

    let name = String::from_utf8_lossy(&output_info.name);
    tracing::debug!(
        "Found an output[{name}], x: {}, y: {}, width: {}, height: {}, rotation: {:?}",
        crtc_info.x,
        crtc_info.y,
        crtc_info.width,
        crtc_info.height,
        crtc_info.rotation
    );

    let mut description = None;
    for prop_atom in output_properties.atoms {
        if prop_atom == atoms.EDID {
            description = Some(get_description(conn, output, prop_atom)?);
        }
    }

    Ok(Some(OutputInfo {
        output,
        name: name.to_string(),
        description: description.unwrap_or_default(),
        x: crtc_info.x,
        y: crtc_info.y,
        physical_width: crtc_info.width,
        physical_height: crtc_info.height,
        rotation: crtc_info.rotation,
    }))
}

fn get_scale_factor(
    conn: &RustConnection,
    default_screen: usize,
    database: &Database,
    atoms: &Atoms,
) -> Result<f64> {
    let mut scale_factor = xsettings_scale_factor(conn, default_screen, atoms)?;
    if scale_factor.is_none() {
        let dpi = database.get_string("Xft.dpi", "");
        tracing::debug!("Xft.dpi from database: {:?}", dpi);
        scale_factor = dpi.and_then(|s| s.parse::<f64>().ok()).map(|s| s / 96.)
    }
    tracing::debug!("scale factor: {:?}", scale_factor);
    Ok(scale_factor.filter(validate_scale_factor).unwrap_or(1.))
}

fn get_description(conn: &RustConnection, output: u32, atom: Atom) -> Result<String> {
    let prop = conn
        .randr_get_output_property(output, atom, AtomEnum::NONE, 0, 256, false, false)?
        .reply()?;

    let res = if prop.format == 8 {
        parse_edid_monitor_name(&prop.data)
    } else {
        None
    };
    Ok(res.unwrap_or_default())
}

/// Parses a human-readable monitor name from EDID bytes.
fn parse_edid_monitor_name(edid: &[u8]) -> Option<String> {
    // Detailed timing descriptors start at byte 54 (0x36), four blocks of 18 bytes each
    for block in 0..4 {
        let i = 54 + block * 18;
        if i + 18 > edid.len() {
            break;
        }

        // Descriptor type at offset 3–4
        if edid[i] == 0x00 && edid[i + 1] == 0x00 {
            let descriptor_type = edid[i + 3];
            if descriptor_type == 0xfc {
                // Model name descriptor
                let name_bytes = &edid[i + 5..i + 18];
                let name = name_bytes
                    .iter()
                    .take_while(|&&c| c != 0x0A && c != 0x00)
                    .map(|&c| c as char)
                    .collect::<String>();
                return Some(name.trim().to_string());
            }
        }
    }
    None
}

// Copy from winit
fn validate_scale_factor(scale_factor: &f64) -> bool {
    scale_factor.is_sign_positive() && scale_factor.is_normal()
}

fn xsettings_scale_factor(
    conn: &RustConnection,
    default_screen: usize,
    atoms: &Atoms,
) -> Result<Option<f64>> {
    let xsettings_screen = if default_screen == 0 {
        atoms._XSETTINGS_S0
    } else {
        conn.intern_atom(false, format!("_XSETTINGS_S{default_screen}").as_bytes())?
            .reply()?
            .atom
    };
    // Get the window that owns this selection
    let selection_owner = conn.get_selection_owner(xsettings_screen)?.reply()?.owner;
    if selection_owner == 0 {
        tracing::debug!("No XSETTINGS selection owner found");
        return Ok(None);
    }

    let prop = conn
        .get_property(
            false,
            selection_owner,
            atoms._XSETTINGS_SETTINGS,
            atoms._XSETTINGS_SETTINGS,
            0,
            10000,
        )?
        .reply()?;

    if prop.format != 8 {
        tracing::debug!("Unexpected property format of xsettings: {}", prop.format);
        return Ok(None);
    }

    let data = &prop.value;

    // Parse XSETTINGS binary format
    let mut offset = 0;

    // 1. byte: endian (B or l)
    let endianness = match data[offset] {
        b'B' => Endianness::B,
        b'l' => Endianness::L,
        _ => Endianness::N,
    };
    tracing::debug!("Endianness: {:?}/{}", endianness, data[offset]);
    offset += 1;

    // 2. padding
    offset += 3;

    // 3. serial (i32)
    offset += 4;

    // 4. N_SETTINGS (i32)
    let n_settings = read_i32(&data[offset..], endianness)?;
    offset += 4;

    tracing::debug!("Number of xsettings: {n_settings}");
    for _ in 0..n_settings {
        // Type (CARD8): 0=integer, 1=string, 2=color
        let setting_type = data[offset];
        offset += 1;

        // Read another byte of padding.
        offset += 1;

        // Name length (i16)
        let name_len = read_i16(&data[offset..], endianness)? as usize;
        offset += 2;

        // Name
        let name = String::from_utf8_lossy(read_exact(&data[offset..], name_len)?);
        offset += name_len;
        tracing::trace!("Reading xsettings[{name:?}], type: {setting_type}");

        // Pad to 4-byte alignment
        offset = (offset + 3) & !3;

        // Last-change serial
        offset += 4;

        // Value
        if name == "Xft/DPI" && setting_type == 0 {
            let value = read_i32(&data[offset..], endianness)?;
            let dpi = value as f64 / 1024.0;
            tracing::debug!("Xft/DPI (from XSETTINGS): {:.2}", dpi);
            return Ok(Some(dpi / 96.));
        } else {
            // Skip the value
            offset = match setting_type {
                // integer
                0 => offset + 4,
                // string
                1 => {
                    let len = read_i32(&data[offset..], endianness)? as usize;
                    offset += 4 + len;
                    (offset + 3) & !3 // align
                }
                // color
                2 => offset + 8,
                _ => anyhow::bail!("Unsupported xsetting type: {setting_type}"),
            }
        }
    }
    Ok(None)
}

fn read_i32(data: &[u8], endianness: Endianness) -> Result<i32> {
    match endianness {
        Endianness::B => read(data, i32::from_be_bytes),
        Endianness::L => read(data, i32::from_le_bytes),
        Endianness::N => read(data, i32::from_ne_bytes),
    }
}

fn read_i16(data: &[u8], endianness: Endianness) -> Result<i16> {
    match endianness {
        Endianness::B => read(data, i16::from_be_bytes),
        Endianness::L => read(data, i16::from_le_bytes),
        Endianness::N => read(data, i16::from_ne_bytes),
    }
}

fn read<const N: usize, T>(data: &[u8], f: fn([u8; N]) -> T) -> Result<T> {
    let mut buf = [0; N];
    buf.clone_from_slice(read_exact(data, N)?);
    Ok(f(buf))
}

fn read_exact(data: &[u8], len: usize) -> Result<&[u8]> {
    if data.len() < len {
        anyhow::bail!(
            "The size of data is too small, expect {}, but got {}",
            len,
            data.len()
        );
    }
    Ok(&data[..len])
}
