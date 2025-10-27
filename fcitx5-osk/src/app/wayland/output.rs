use std::{
    future::Future,
    sync::{Arc, Mutex, MutexGuard},
};

use anyhow::Result;
use iced::{
    futures::{
        channel::mpsc::{self, UnboundedReceiver, UnboundedSender},
        stream,
    },
    Subscription,
};
use iced_layershell::reexport::{Anchor, Layer, WlRegion};
use tokio::task::JoinHandle;
use wayland_client::{
    delegate_noop,
    protocol::{
        wl_compositor::WlCompositor,
        wl_output::{Event as WlOutputEvent, Transform, WlOutput},
        wl_registry::{Event as WlRegistryEvent, WlRegistry},
        wl_surface::WlSurface,
    },
    Connection, Dispatch, Proxy, QueueHandle, WEnum,
};
use wayland_protocols::wp::fractional_scale::v1::client::{
    wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1,
    wp_fractional_scale_v1::{Event as FractionalScaleEvent, WpFractionalScaleV1},
};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::ZwlrLayerShellV1,
    zwlr_layer_surface_v1::{Event as LayerShellSurfaceEvent, ZwlrLayerSurfaceV1},
};

use crate::{
    app::{
        wayland::{connection::WaylandConnection, WaylandMessage},
        Message,
    },
    state::WindowManagerEvent,
};

struct State {
    rx: Option<UnboundedReceiver<WaylandMessage>>,
    bg_handle: Option<JoinHandle<()>>,
    output_infos: Vec<OutputInfo>,
    selected_output: Option<u32>,
    closed: bool,
}

impl State {
    fn close(&mut self) {
        if self.closed {
            return;
        }
        tracing::debug!("Close OutputContext State");
        drop(self.bg_handle.take());
        for output_info in self.output_infos.drain(..) {
            output_info.output.release();
        }
        self.closed = true;
    }
}

impl Drop for State {
    fn drop(&mut self) {
        self.close();
    }
}

#[derive(Debug)]
struct OutputInfo {
    output_name: u32,
    output: WlOutput,
    name: String,
    description: String,
    physical_width: i32,
    physical_height: i32,
    // We need the actual size(subtract the size of panels) which our window can be drawn.
    logical_width: u32,
    logical_height: u32,
    scale_factor: f64,
    transform: Transform,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OutputGeometry {
    pub output_name: u32,
    pub output: WlOutput,
    pub logical_width: u32,
    pub logical_height: u32,
    pub scale_factor: f64,
    pub transform: Transform,
}

impl From<&OutputInfo> for OutputGeometry {
    fn from(value: &OutputInfo) -> Self {
        Self {
            output_name: value.output_name,
            output: value.output.clone(),
            logical_width: value.logical_width,
            logical_height: value.logical_height,
            scale_factor: value.scale_factor,
            transform: value.transform,
        }
    }
}

#[derive(Clone)]
pub struct OutputContext {
    connection: WaylandConnection,
    tx: UnboundedSender<WaylandMessage>,
    state: Arc<Mutex<State>>,
}

impl OutputContext {
    pub fn new(connection: WaylandConnection) -> Self {
        let (tx, rx) = mpsc::unbounded();
        Self {
            connection,
            tx,
            state: Arc::new(Mutex::new(State {
                rx: Some(rx),
                bg_handle: None,
                output_infos: vec![],
                selected_output: None,
                closed: false,
            })),
        }
    }

    /// Currently, it is ok not getting state from OutputContext
    fn state(&self) -> Option<MutexGuard<'_, State>> {
        self.state.lock().ok()
    }

    pub fn subscription(&self) -> Subscription<WaylandMessage> {
        const EXTERNAL_SUBSCRIPTION_ID: &str = "external::wayland_output";
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
        let mut selected_output_info = None;
        for output_info in &guard.output_infos {
            if preferred_output_name
                .filter(|n| output_info.name == *n)
                .is_some()
            {
                tracing::debug!("Found preferred output: {}", output_info.name);
                // mark it as selected
                let output_geometry = OutputGeometry::from(output_info);
                guard.selected_output = Some(output_geometry.output_name);
                return Some(output_geometry);
            }
            if let Some(selected_output) = guard.selected_output {
                if selected_output == output_info.output_name {
                    selected_output_info = Some(output_info);
                }
            }
        }

        // preferred_output_name not found, use selected_output
        if let Some(selected_output_info) = selected_output_info {
            let output_geometry = OutputGeometry::from(selected_output_info);
            return Some(output_geometry);
        }

        // Use the first one
        if let Some(output_info) = guard.output_infos.first() {
            tracing::debug!("Use first output: {}", output_info.name);
            let output_geometry = OutputGeometry::from(output_info);
            // mark it as selected
            guard.selected_output = Some(output_geometry.output_name);
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
        if guard.bg_handle.is_none() {
            let output_context = self.clone();
            let bg = listen(self)?;
            guard.bg_handle = Some(tokio::spawn(async move {
                if let Err(e) = bg.await {
                    tracing::error!("wayland WlOutput event queue exit with error: {:?}", e);
                    if let Ok(state) = output_context.connection.state() {
                        if let Some(error) = state.connection().protocol_error() {
                            tracing::error!(
                                "Wayland protocol error: object[{}], code[{}], message[{}]",
                                error.object_id,
                                error.code,
                                error.message
                            );
                            panic!(
                                "Wayland protocol error: object[{}], code[{}], message[{}]",
                                error.object_id, error.code, error.message
                            );
                        }
                    }
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

    pub fn send_output_changed_event(&self) {
        if self
            .tx
            .unbounded_send(Message::from(WindowManagerEvent::OutputChanged).into())
            .is_err()
        {
            tracing::error!("Unable to send WindowManagerEvent::OutputChanged event");
        }
    }
}

#[derive(Debug)]
struct OutputChangedRequest {
    output_name: u32,
    output: Option<WlOutput>,
    name: Option<String>,
    description: Option<String>,
    physical_width: Option<i32>,
    physical_height: Option<i32>,
    logical_width: Option<u32>,
    logical_height: Option<u32>,
    scale_factor: Option<f64>,
    transform: Option<Transform>,
}

impl OutputChangedRequest {
    fn new(output_name: u32) -> Self {
        Self {
            output_name,
            output: None,
            name: None,
            description: None,
            physical_width: None,
            physical_height: None,
            logical_width: None,
            logical_height: None,
            scale_factor: None,
            transform: None,
        }
    }
}

/// Create a transparent layer shell surface to monitor the change of geometry of the output
struct GeometryState {
    output_name: u32,
    surface: WlSurface,
    layer_shell_surface: ZwlrLayerSurfaceV1,
    fractional_scale: WpFractionalScaleV1,
}

impl Drop for GeometryState {
    fn drop(&mut self) {
        self.fractional_scale.destroy();
        self.layer_shell_surface.destroy();
        self.surface.destroy();
    }
}

struct OutputListener {
    output_context: OutputContext,
    requests: Vec<OutputChangedRequest>,
    compositer: WlCompositor,
    layer_shell: ZwlrLayerShellV1,
    fractional_scale_manager: WpFractionalScaleManagerV1,
    geometry_states: Vec<GeometryState>,
}

impl OutputListener {
    fn output_context_state(&self) -> Option<MutexGuard<'_, State>> {
        self.output_context
            .state()
            .or_else(|| {
                tracing::error!("Unable to get the state of OutputContext");
                None
            })
            .filter(|g| !g.closed)
    }

    fn find_or_add_request_mut(&mut self, output_name: u32) -> &mut OutputChangedRequest {
        let pos = self
            .requests
            .iter()
            .position(|r| r.output_name == output_name);
        let pos = if let Some(pos) = pos {
            pos
        } else {
            let pos = self.requests.len();
            // Add new request
            self.requests.push(OutputChangedRequest::new(output_name));
            pos
        };
        &mut self.requests[pos]
    }

    fn find_request_mut(&mut self, output_name: u32) -> Option<&mut OutputChangedRequest> {
        self.requests
            .iter_mut()
            .find(|r| r.output_name == output_name)
    }

    fn take_request(&mut self, output_name: u32) -> Option<OutputChangedRequest> {
        let pos = self
            .requests
            .iter()
            .position(|r| r.output_name == output_name);
        pos.map(|pos| self.requests.swap_remove(pos))
    }

    fn handle_output_changed(&self, mut request: OutputChangedRequest) {
        let output_name = request.output_name;
        // Update OutputContext
        let Some(mut guard) = self.output_context_state() else {
            // Do nothing if it is poisoned or closed
            return;
        };
        if let Some(output_info) = guard
            .output_infos
            .iter_mut()
            .find(|o| o.output_name == output_name)
        {
            if let Some(output) = request.output.take() {
                tracing::error!("WlOutput shouldn't be set in the request");
                if output != output_info.output {
                    output.release();
                }
            }
            tracing::debug!("Output changed: {:?}", request);
            if let Some(name) = request.name.take() {
                output_info.name = name;
            }
            if let Some(description) = request.description.take() {
                output_info.description = description;
            }
            let mut layout_changed = false;
            if let Some(transform) = request.transform.take() {
                layout_changed = layout_changed || transform != output_info.transform;
                output_info.transform = transform;
            }
            if let Some(physical_width) = request.physical_width.take() {
                layout_changed = layout_changed || physical_width != output_info.physical_width;
                output_info.physical_width = physical_width;
            }
            if let Some(physical_height) = request.physical_height.take() {
                layout_changed = layout_changed || physical_height != output_info.physical_height;
                output_info.physical_height = physical_height;
            }
            if let Some(logical_width) = request.logical_width.take() {
                layout_changed = layout_changed || logical_width != output_info.logical_width;
                output_info.logical_width = logical_width;
            }
            if let Some(logical_height) = request.logical_height.take() {
                layout_changed = layout_changed || logical_height != output_info.logical_height;
                output_info.logical_height = logical_height;
            }
            if let Some(scale_factor) = request.scale_factor.take() {
                layout_changed = layout_changed || scale_factor != output_info.scale_factor;
                output_info.scale_factor = scale_factor;
            }
            // Reopen the keyboard if the layout of selected output is changed
            if guard.selected_output == Some(output_name) && layout_changed {
                self.output_context.send_output_changed_event();
            }
        } else {
            let Some(output) = request.output.take() else {
                tracing::error!("There is no WlOutput set in the request");
                return;
            };
            let name = if let Some(name) = request.name.take() {
                name
            } else {
                tracing::warn!("There is no name set in the request, use output_name");
                output_name.to_string()
            };
            let description = if let Some(description) = request.description.take() {
                description
            } else {
                tracing::warn!("There is no description set in the request");
                "".to_string()
            };
            let transform = if let Some(transform) = request.transform.take() {
                transform
            } else {
                tracing::warn!("There is no transform set in the request");
                Transform::Normal
            };
            let physical_width = if let Some(physical_width) = request.physical_width.take() {
                physical_width
            } else {
                tracing::warn!("There is no physical_width set in the request");
                i32::MAX
            };
            let physical_height = if let Some(physical_height) = request.physical_height.take() {
                physical_height
            } else {
                tracing::warn!("There is no physical_height set in the request");
                i32::MAX
            };
            // logical_width might not exist at this time, so we don't warn.
            let logical_width = request.logical_width.take().unwrap_or(u32::MAX);
            // logical_height might not exist at this time, so we don't warn.
            let logical_height = request.logical_height.take().unwrap_or(u32::MAX);
            // scale_factor might not exist at this time, so we don't warn.
            let scale_factor = request.scale_factor.take().unwrap_or(1.);
            let output_info = OutputInfo {
                output_name,
                output,
                name,
                description,
                physical_width,
                physical_height,
                logical_width,
                logical_height,
                scale_factor,
                transform,
            };
            tracing::debug!("Add new output: {:?}", output_info);
            guard.output_infos.push(output_info);
            if guard.selected_output.is_none() {
                // send event if there is no selected output
                self.output_context.send_output_changed_event();
            }
        }
    }
}

impl Drop for OutputListener {
    fn drop(&mut self) {
        for mut request in self.requests.drain(..) {
            if let Some(output) = request.output.take() {
                output.release();
            }
        }
        drop(self.geometry_states.drain(..));
        self.fractional_scale_manager.destroy();
    }
}

impl Dispatch<WlRegistry, ()> for OutputListener {
    fn event(
        state: &mut Self,
        proxy: &WlRegistry,
        event: <WlRegistry as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match &event {
            WlRegistryEvent::Global {
                name,
                interface,
                version,
            } => {
                let name = *name;
                if interface == WlOutput::interface().name {
                    // minimum version
                    if *version < 4 {
                        tracing::warn!("The version of wl_output is less than 4, geometry can't be handled correctly");
                        return;
                    }
                    tracing::debug!("Add output {}", name);
                    let output = proxy.bind::<WlOutput, _, _>(name, *version, qh, name);

                    // Create a transparent layer shell surface for monitoring the scale factor
                    let surface = state.compositer.create_surface(qh, ());
                    let layer_shell_surface = state.layer_shell.get_layer_surface(
                        &surface,
                        Some(&output),
                        Layer::Background,
                        "TRANSPARENT".to_string(),
                        qh,
                        name,
                    );
                    tracing::debug!(
                        "Create a layer shell surface[{:?}] to monitor the change of geometry",
                        layer_shell_surface
                    );
                    layer_shell_surface.set_anchor(Anchor::Top | Anchor::Left);
                    // Make events pass through the window
                    let region = state.compositer.create_region(qh, ());
                    surface.set_input_region(Some(&region));
                    region.destroy();

                    surface.commit();
                    let fractional_scale = state
                        .fractional_scale_manager
                        .get_fractional_scale(&surface, qh, name);

                    let mut request = OutputChangedRequest::new(name);
                    // Check if there is any request of the same name
                    state.requests.retain(|r| {
                        if r.output_name != name {
                            true
                        } else {
                            if let Some(exited_output) = &r.output {
                                // release if it is not the same.
                                if exited_output != &output {
                                    tracing::warn!("Two WlOutputs have the same name but they aren't the same: {}", name);
                                    exited_output.release();
                                }
                            }
                            tracing::warn!("There are two WlOutput requests with the same name");
                            false
                        }
                    });
                    request.output = Some(output);
                    state.requests.push(request);
                    state.geometry_states.push(GeometryState {
                        output_name: name,
                        surface,
                        layer_shell_surface,
                        fractional_scale,
                    });
                }
            }
            WlRegistryEvent::GlobalRemove { name } => {
                let name = *name;
                // Remove request of the output. The wayland server told us the output is being
                // removed, I think there is no need to call release.
                state.requests.retain(|r| r.output_name != name);
                state.geometry_states.retain(|s| s.output_name != name);
                let Some(mut guard) = state.output_context_state() else {
                    // Do nothing if it is poisoned or closed
                    return;
                };
                guard.output_infos.retain(|o| {
                    let removed = o.output_name == name;
                    if removed {
                        tracing::debug!("Remove output: {}", o.output_name);
                    }
                    !removed
                });
                // If selected_output is removed send event
                if guard
                    .selected_output
                    .take_if(|output_name| *output_name == name)
                    .is_some()
                {
                    state.output_context.send_output_changed_event();
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<WlOutput, u32> for OutputListener {
    fn event(
        state: &mut Self,
        _proxy: &WlOutput,
        event: <WlOutput as Proxy>::Event,
        output_name: &u32,
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        let output_name = *output_name;
        tracing::debug!("Output changed event {:?}", event);
        // The value of scale event is integer, so we don't use the scale of WlOutput
        match event {
            WlOutputEvent::Geometry {
                x: _,
                y: _,
                physical_width: _,
                physical_height: _,
                subpixel: _,
                make: _,
                model: _,
                transform,
            } => match transform {
                WEnum::Value(transform) => {
                    state.find_or_add_request_mut(output_name).transform = Some(transform)
                }
                WEnum::Unknown(v) => {
                    tracing::warn!("Unknown transform value: {}", v);
                }
            },
            WlOutputEvent::Mode {
                flags: _,
                width,
                height,
                refresh: _,
            } => {
                let request = state.find_or_add_request_mut(output_name);
                request.physical_width = Some(width);
                request.physical_height = Some(height);
            }
            WlOutputEvent::Name { name } => {
                state.find_or_add_request_mut(output_name).name = Some(name)
            }
            WlOutputEvent::Description { description } => {
                state.find_or_add_request_mut(output_name).description = Some(description)
            }
            WlOutputEvent::Done => {
                let Some(request) = state.take_request(output_name) else {
                    tracing::debug!("No request found");
                    return;
                };
                state.handle_output_changed(request);
            }
            _ => {}
        }
    }
}

impl Dispatch<ZwlrLayerSurfaceV1, u32> for OutputListener {
    fn event(
        state: &mut Self,
        proxy: &ZwlrLayerSurfaceV1,
        event: <ZwlrLayerSurfaceV1 as Proxy>::Event,
        output_name: &u32,
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        let output_name = *output_name;
        match event {
            LayerShellSurfaceEvent::Configure {
                serial,
                width,
                height,
            } => {
                match state.find_request_mut(output_name) {
                    Some(r) => {
                        // Output has been added to OutputContext state
                        r.logical_width = Some(width);
                        r.logical_height = Some(height);
                    }
                    None => {
                        let mut request = OutputChangedRequest::new(output_name);
                        request.logical_width = Some(width);
                        request.logical_height = Some(height);
                        state.handle_output_changed(request);
                    }
                }
                proxy.ack_configure(serial);
            }
            LayerShellSurfaceEvent::Closed => {
                if state
                    .geometry_states
                    .iter()
                    .any(|s| s.output_name == output_name && s.layer_shell_surface == *proxy)
                {
                    // Client will only destroy layer shell surface after it is removed from state. So
                    // this is closed from the server side.
                    tracing::error!("The layer shell surface of output[{}] is closed from the server side, the change of geometry on this output won't work", output_name);
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<WpFractionalScaleV1, u32> for OutputListener {
    fn event(
        state: &mut Self,
        _proxy: &WpFractionalScaleV1,
        event: <WpFractionalScaleV1 as Proxy>::Event,
        output_name: &u32,
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        let output_name = *output_name;
        if let FractionalScaleEvent::PreferredScale { scale } = event {
            let scale_factor = Some(scale as f64 / 120.);
            match state.find_request_mut(output_name) {
                Some(r) => r.scale_factor = scale_factor,
                None => {
                    // Output has been added to OutputContext state
                    let mut request = OutputChangedRequest::new(output_name);
                    request.scale_factor = scale_factor;
                    state.handle_output_changed(request);
                }
            }
        }
    }
}

delegate_noop!(OutputListener: ignore WlCompositor);
delegate_noop!(OutputListener: ignore WpFractionalScaleManagerV1);
delegate_noop!(OutputListener: ignore WlSurface);
delegate_noop!(OutputListener: ignore ZwlrLayerShellV1);
delegate_noop!(OutputListener: ignore WlRegion);

fn listen(output_context: &OutputContext) -> Result<impl Future<Output = Result<()>> + 'static> {
    let state = output_context.connection.state()?;
    let connection = state.connection();
    let global_list = state.global_list()?;

    let mut event_queue = connection.new_event_queue::<OutputListener>();

    // bind WlRegistry to get event callback
    let qh = event_queue.handle();
    connection.display().get_registry(&qh, ());

    let compositer = global_list.bind::<WlCompositor, _, _>(&qh, 1..=5, ())?;
    let layer_shell = global_list.bind::<ZwlrLayerShellV1, _, _>(&qh, 3..=4, ())?;
    let fractional_scale_manager =
        global_list.bind::<WpFractionalScaleManagerV1, _, _>(&qh, 1..=1, ())?;

    let mut output_listener = OutputListener {
        output_context: output_context.clone(),
        requests: vec![],
        compositer,
        layer_shell,
        fractional_scale_manager,
        geometry_states: vec![],
    };
    Ok(async move {
        // we should not use blocking_dispatch here. because layershellev uses blocking_dispatch, if we use blocking_dispatch it will freeze the eventloop of layershellev
        loop {
            std::future::poll_fn(|cx| event_queue.poll_dispatch_pending(cx, &mut output_listener))
                .await?;
        }
    })
}
