use std::{
    result::Result,
    sync::{Arc, LazyLock},
};

use wayland_client::{
    globals::{self, GlobalError, GlobalList, GlobalListContents},
    protocol::wl_registry::WlRegistry,
    ConnectError, Connection, Dispatch, Proxy, QueueHandle,
};

#[derive(Debug)]
pub struct WaylandConnectionState {
    connection: Connection,
    global_list: Result<GlobalList, GlobalError>,
}

impl WaylandConnectionState {
    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    pub fn global_list(&self) -> Result<&GlobalList, GlobalError> {
        match &self.global_list {
            Ok(l) => Ok(l),
            Err(e) => Err(match e {
                GlobalError::Backend(wayland_error) => GlobalError::Backend(wayland_error.clone()),
                GlobalError::InvalidId(invalid_id) => GlobalError::InvalidId(invalid_id.clone()),
            }),
        }
    }
}

#[derive(Debug, Clone)]
pub struct WaylandConnection {
    state: Arc<LazyLock<Result<WaylandConnectionState, ConnectError>>>,
}

impl Dispatch<WlRegistry, GlobalListContents> for WaylandConnection {
    fn event(
        _state: &mut Self,
        _proxy: &WlRegistry,
        _event: <WlRegistry as Proxy>::Event,
        _data: &GlobalListContents,
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
    }
}

impl WaylandConnection {
    pub fn new() -> Self {
        Self {
            state: Arc::new(LazyLock::new(|| {
                let connection = Connection::connect_to_env()?;

                let global_list = globals::registry_queue_init::<WaylandConnection>(&connection)
                    .map(|(global_list, _)| {
                        tracing::debug!("global_list: {:?}", global_list.contents(),);
                        global_list
                    });

                Ok(WaylandConnectionState {
                    connection,
                    global_list,
                })
            })),
        }
    }

    pub fn state(&self) -> Result<&WaylandConnectionState, ConnectError> {
        match &**self.state {
            Ok(conn) => Ok(conn),
            Err(e) => Err(match e {
                ConnectError::NoWaylandLib => ConnectError::NoWaylandLib,
                ConnectError::NoCompositor => ConnectError::NoCompositor,
                ConnectError::InvalidFd => ConnectError::InvalidFd,
            }),
        }
    }
}
