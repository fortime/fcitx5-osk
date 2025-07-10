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
use tokio::task::JoinHandle;
use wayland_client::{
    protocol::{
        wl_output::{Event as WlOutputEvent, Transform, WlOutput},
        wl_registry::{Event as WlRegistryEvent, WlRegistry},
    },
    Connection, Dispatch, Proxy, QueueHandle, WEnum,
};

use crate::{
    app::{
        wayland::{connection::WaylandConnection, WaylandMessage},
        Message,
    },
    dbus::server::ImPanelEvent,
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
            output_info.wl_output.release();
        }
        self.closed = true;
    }
}

impl Drop for State {
    fn drop(&mut self) {
        self.close();
    }
}

pub struct OutputInfo {
    wl_output_name: u32,
    wl_output: WlOutput,
    name: String,
    description: String,
    physical_width: i32,
    physical_height: i32,
    transform: Transform,
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

    pub fn select_output(&self, preferred_output_name: Option<&str>) -> Option<(String, WlOutput)> {
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
                let wl_output_name = output_info.wl_output_name;
                let wl_output = output_info.wl_output.clone();
                let name = output_info.name.clone();
                guard.selected_output = Some(wl_output_name);
                return Some((name, wl_output));
            }
            if let Some(selected_output) = guard.selected_output {
                if selected_output == output_info.wl_output_name {
                    selected_output_info = Some(output_info);
                }
            }
        }

        // preferred_output_name not found, use selected_output
        if let Some(selected_output_info) = selected_output_info {
            return Some((
                selected_output_info.name.clone(),
                selected_output_info.wl_output.clone(),
            ));
        }

        // Use the first one
        if let Some(output_info) = guard.output_infos.first() {
            tracing::debug!("Use first output: {}", output_info.name);
            // mark it as selected
            let wl_output_name = output_info.wl_output_name;
            let wl_output = output_info.wl_output.clone();
            let name = output_info.name.clone();
            guard.selected_output = Some(wl_output_name);
            Some((name, wl_output))
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
            let bg = listen(self)?;
            guard.bg_handle = Some(tokio::spawn(async move {
                if let Err(e) = bg.await {
                    tracing::error!("wayland wl_output event queue exit with error: {:?}", e);
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
}

struct OutputChangedRequest {
    wl_output_name: u32,
    wl_output: Option<WlOutput>,
    name: Option<String>,
    description: Option<String>,
    physical_width: Option<i32>,
    physical_height: Option<i32>,
    transform: Option<Transform>,
}

impl OutputChangedRequest {
    fn new(wl_output_name: u32) -> Self {
        Self {
            wl_output_name,
            wl_output: None,
            name: None,
            description: None,
            physical_width: None,
            physical_height: None,
            transform: None,
        }
    }
}

struct OutputListener {
    output_context: OutputContext,
    requests: Vec<OutputChangedRequest>,
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

    fn find_or_add_request(&mut self, wl_output_name: u32) -> &mut OutputChangedRequest {
        let pos = self
            .requests
            .iter()
            .position(|r| r.wl_output_name == wl_output_name);
        let pos = if let Some(pos) = pos {
            pos
        } else {
            let pos = self.requests.len();
            // Add new request
            self.requests
                .push(OutputChangedRequest::new(wl_output_name));
            pos
        };
        &mut self.requests[pos]
    }

    fn take_request(&mut self, wl_output_name: u32) -> Option<OutputChangedRequest> {
        let pos = self
            .requests
            .iter()
            .position(|r| r.wl_output_name == wl_output_name);
        pos.map(|pos| self.requests.swap_remove(pos))
    }
}

impl Drop for OutputListener {
    fn drop(&mut self) {
        for mut request in self.requests.drain(..) {
            if let Some(wl_output) = request.wl_output.take() {
                wl_output.release();
            }
        }
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
                    let wl_output = proxy.bind::<WlOutput, _, _>(name, *version, qh, name);
                    let mut request = OutputChangedRequest::new(name);
                    // Check if there is any request of the same name
                    state.requests.retain(|r| {
                        if r.wl_output_name != name {
                            true
                        } else {
                            if let Some(exited_wl_output) = &r.wl_output {
                                // release if it is not the same.
                                if exited_wl_output != &wl_output {
                                    tracing::warn!("Two WlOutputs have the same name but they aren't the same: {}", name);
                                    exited_wl_output.release();
                                }
                            }
                            tracing::warn!("There are two WlOutput requests with the same name");
                            false
                        }
                    });
                    request.wl_output = Some(wl_output);
                    state.requests.push(request);
                }
            }
            WlRegistryEvent::GlobalRemove { name } => {
                let name = *name;
                // Remove request of the output. The wayland server told us the output is being
                // removed, I think there is no need to call release.
                state.requests.retain(|r| r.wl_output_name != name);
                let Some(mut guard) = state.output_context_state() else {
                    // Do nothing if it is poisoned or closed
                    return;
                };
                guard.output_infos.retain(|o| {
                    let removed = o.wl_output_name == name;
                    if removed {
                        tracing::debug!("Remove output: {}", o.wl_output_name);
                    }
                    !removed
                });
                guard
                    .selected_output
                    .take_if(|wl_output_name| *wl_output_name == name);
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
        wl_output_name: &u32,
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        let wl_output_name = *wl_output_name;
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
                    state.find_or_add_request(wl_output_name).transform = Some(transform)
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
                let request = state.find_or_add_request(wl_output_name);
                request.physical_width = Some(width);
                request.physical_height = Some(height);
            }
            WlOutputEvent::Name { name } => {
                state.find_or_add_request(wl_output_name).name = Some(name)
            }
            WlOutputEvent::Description { description } => {
                state.find_or_add_request(wl_output_name).description = Some(description)
            }
            WlOutputEvent::Done => {
                let Some(mut request) = state.take_request(wl_output_name) else {
                    tracing::debug!("No request found");
                    return;
                };
                // Update OutputContext
                let Some(mut guard) = state.output_context_state() else {
                    // Do nothing if it is poisoned or closed
                    return;
                };
                if let Some(output_info) = guard
                    .output_infos
                    .iter_mut()
                    .find(|o| o.wl_output_name == wl_output_name)
                {
                    if let Some(wl_output) = request.wl_output.take() {
                        tracing::error!("WlOutput shouldn't be set in the request");
                        if wl_output != output_info.wl_output {
                            wl_output.release();
                        }
                    }
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
                        layout_changed =
                            layout_changed || physical_width != output_info.physical_width;
                        output_info.physical_width = physical_width;
                    }
                    if let Some(physical_height) = request.physical_height.take() {
                        layout_changed =
                            layout_changed || physical_height != output_info.physical_height;
                        output_info.physical_height = physical_height;
                    }
                    // Reopen the keyboard if the layout of selected output is changed
                    if guard.selected_output == Some(wl_output_name) && layout_changed && state
                            .output_context
                            .tx
                            .unbounded_send(Message::from(ImPanelEvent::ReopenIfOpened).into())
                            .is_err() {
                        tracing::error!("Unable to send ImPanelEvent::ReopenIfOpened event");
                    }
                } else {
                    let Some(wl_output) = request.wl_output.take() else {
                        tracing::error!("There is no WlOutput set in the request");
                        return;
                    };
                    let name = if let Some(name) = request.name.take() {
                        name
                    } else {
                        tracing::warn!("There is no name set in the request, use wl_output_name");
                        wl_output_name.to_string()
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
                    let physical_width = if let Some(physical_width) = request.physical_width.take()
                    {
                        physical_width
                    } else {
                        tracing::warn!("There is no physical_width set in the request");
                        i32::MAX
                    };
                    let physical_height =
                        if let Some(physical_height) = request.physical_height.take() {
                            physical_height
                        } else {
                            tracing::warn!("There is no physical_height set in the request");
                            i32::MAX
                        };
                    guard.output_infos.push(OutputInfo {
                        wl_output_name,
                        wl_output,
                        name,
                        description,
                        physical_width,
                        physical_height,
                        transform,
                    });
                }
            }
            _ => {}
        }
    }
}

fn listen(output_context: &OutputContext) -> Result<impl Future<Output = Result<()>> + 'static> {
    let state = output_context.connection.state()?;
    let connection = state.connection();

    let mut event_queue = connection.new_event_queue::<OutputListener>();

    //// bind WlRegistry to get event callback
    let qh = event_queue.handle();
    connection.display().get_registry(&qh, ());

    let mut output_listener = OutputListener {
        output_context: output_context.clone(),
        requests: vec![],
    };
    Ok(async move {
        // we should not use blocking_dispatch here. because layershellev uses blocking_dispatch, if we use blocking_dispatch it will freeze the eventloop of layershellev
        loop {
            std::future::poll_fn(|cx| event_queue.poll_dispatch_pending(cx, &mut output_listener))
                .await?;
        }
    })
}
