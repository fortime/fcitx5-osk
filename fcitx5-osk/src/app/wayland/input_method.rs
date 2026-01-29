use std::sync::{Arc, Mutex, MutexGuard};

use anyhow::Result;
use iced::{
    futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender},
    futures::stream,
    Subscription,
};
use tokio::task::JoinHandle;
use v1::Fcitx5ControllerServiceStub;

use crate::{app::wayland::WaylandMessage, dbus::client::Fcitx5Services};

use super::connection::WaylandConnection;

struct State {
    rx: Option<UnboundedReceiver<WaylandMessage>>,
    fcitx5_services: Option<Fcitx5Services>,
    bg_handle: Option<JoinHandle<()>>,
    closed: bool,
}

impl State {
    pub fn new(rx: UnboundedReceiver<WaylandMessage>) -> Self {
        Self {
            rx: Some(rx),
            fcitx5_services: None,
            bg_handle: None,
            closed: false,
        }
    }
}

#[derive(Clone)]
pub struct InputMethodContext {
    connection: WaylandConnection,
    tx: UnboundedSender<WaylandMessage>,
    state: Arc<Mutex<State>>,
}

impl InputMethodContext {
    pub fn new(connection: WaylandConnection) -> Self {
        let (tx, rx) = mpsc::unbounded();
        Self {
            connection,
            tx,
            state: Arc::new(Mutex::new(State::new(rx))),
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .expect("input method context state is poisoned")
    }

    pub fn subscription(&self) -> Subscription<WaylandMessage> {
        const EXTERNAL_SUBSCRIPTION_ID: &str = "external::wayland_input_method";
        if let Some(rx) = self.state.lock().ok().and_then(|mut s| s.rx.take()) {
            Subscription::run_with_id(EXTERNAL_SUBSCRIPTION_ID, rx)
        } else {
            // should always return a subscription with the same id, otherwise, the first one will
            // be dropped.
            Subscription::run_with_id(EXTERNAL_SUBSCRIPTION_ID, stream::empty())
        }
    }

    pub fn fcitx5_services(&self) -> Result<Fcitx5Services> {
        let mut guard = self.state();
        if let Some(fcitx5_services) = &guard.fcitx5_services {
            Ok(fcitx5_services.clone())
        } else {
            // create a mock fcitx5 backend using input-method-v1, so that the keyboard can be
            // shown and input in kscreenlocker.
            let (client, bg) = v1::new(&self.connection, self.tx.clone())?;
            let handle = tokio::spawn(async move {
                if let Err(e) = bg.await {
                    tracing::error!(
                        "wayland input-method-v1 event queue exit with error: {:?}",
                        e
                    );
                }
            });
            guard.bg_handle = Some(handle);
            let stub = Arc::new(Fcitx5ControllerServiceStub);
            let client = Arc::new(iced::futures::lock::Mutex::new(client));
            let fcitx5_services = Fcitx5Services::new_with(stub.clone(), stub, client);
            guard.fcitx5_services = Some(fcitx5_services.clone());
            Ok(fcitx5_services)
        }
    }

    pub fn close(&self) {
        let Some(mut guard) = self.state.lock().ok() else {
            tracing::debug!("closing InputMethodContext, but lock is poisoned");
            return;
        };
        if guard.closed {
            return;
        }
        tracing::debug!("close InputMethodContext");
        drop(guard.bg_handle.take());
        drop(guard.fcitx5_services.take());
        guard.closed = true;
    }
}

impl Drop for InputMethodContext {
    fn drop(&mut self) {
        self.close();
    }
}

mod v1 {
    use std::{
        future::Future,
        sync::{
            atomic::{AtomicU32, Ordering},
            Arc, Mutex, MutexGuard,
        },
    };

    use anyhow::{Context, Result};
    use iced::futures::channel::mpsc::UnboundedSender;
    use wayland_client::{event_created_child, Connection, Dispatch, Proxy, QueueHandle};
    use wayland_protocols::wp::input_method::zv1::client::{
        zwp_input_method_context_v1::ZwpInputMethodContextV1,
        zwp_input_method_v1::{self, Event as ZwpInputMethodV1Event, ZwpInputMethodV1},
    };
    use zbus::{Error as ZbusError, Result as ZbusResult};

    use crate::{
        app::{
            wayland::{connection::WaylandConnection, WaylandMessage},
            Message,
        },
        dbus::{
            client::{
                IFcitx5ControllerService, IFcitx5VirtualKeyboardBackendService,
                IFcitx5VirtualKeyboardService, InputMethodGroupInfo, InputMethodInfo,
            },
            server::ImPanelEvent,
        },
    };

    #[derive(Debug)]
    pub struct Fcitx5ControllerServiceStub;

    impl Fcitx5ControllerServiceStub {
        const IM_NAME: &str = "wayland-im-v1";
    }

    /// WaylandInputMethodV1Client implements IFcitx5VirtualKeyboardBackendService only, so it
    /// doesn't need to be `Clone`.
    #[derive(Debug)]
    pub struct WaylandInputMethodV1Client {
        serial: AtomicU32,
        input_method_context: Arc<Mutex<Option<ZwpInputMethodContextV1>>>,
    }

    impl WaylandInputMethodV1Client {
        fn input_method_context(&self) -> MutexGuard<'_, Option<ZwpInputMethodContextV1>> {
            self.input_method_context
                .lock()
                .expect("wayland input method context v1 is poisoned")
        }
    }

    impl Drop for WaylandInputMethodV1Client {
        fn drop(&mut self) {
            tracing::debug!("drop WaylandInputMethodV1Client");
        }
    }

    #[async_trait::async_trait]
    impl IFcitx5ControllerService for Fcitx5ControllerServiceStub {
        #[tracing::instrument(level = "debug", skip(self), err, ret)]
        async fn full_input_method_group_info(
            &self,
            _name: &str,
        ) -> ZbusResult<InputMethodGroupInfo> {
            let input_methods = vec![InputMethodInfo::new(Self::IM_NAME)];
            InputMethodGroupInfo::new("", 0, "", input_methods)
                .map_err(|e| ZbusError::Failure(e.to_string()))
        }

        #[tracing::instrument(level = "debug", skip(self), err, ret)]
        async fn current_input_method(&self) -> ZbusResult<String> {
            Ok(Self::IM_NAME.to_string())
        }

        #[tracing::instrument(level = "debug", skip(self), err, ret)]
        async fn set_current_im(&self, _im: &str) -> ZbusResult<()> {
            Ok(())
        }
    }

    #[async_trait::async_trait]
    impl IFcitx5VirtualKeyboardService for Fcitx5ControllerServiceStub {
        async fn show_virtual_keyboard(&self) -> ZbusResult<()> {
            Ok(())
        }

        async fn hide_virtual_keyboard(&self) -> ZbusResult<()> {
            Ok(())
        }
    }

    #[async_trait::async_trait]
    impl IFcitx5VirtualKeyboardBackendService for WaylandInputMethodV1Client {
        #[tracing::instrument(level = "debug", skip(self), err, ret)]
        async fn process_key_event(
            &mut self,
            keyval: u32,
            keycode: u32,
            state: u32,
            is_release: bool,
            time: u32,
        ) -> ZbusResult<()> {
            let guard = self.input_method_context();
            let Some(input_method_context) = guard.as_ref() else {
                return Err(ZbusError::Failure(
                    "there is no wayland input method context v1 ".to_string(),
                ));
            };
            // currently, this method is only for inputting password in kscreenlocker, we don't
            // respect the rule of xkbcommon.

            // only handle shift for inputting captital letter.
            let modifier_mask = if keycode == 50 || keycode == 62 {
                // Shift mask is 0x1
                Some(1)
            } else {
                None
            };
            let serial = self.serial.fetch_add(1, Ordering::Relaxed);
            if let Some(modifier_mask) = modifier_mask {
                // this is a simple implementation, no key modifier combo
                if is_release {
                    input_method_context.modifiers(serial, 0, 0, 0, 0);
                } else {
                    input_method_context.modifiers(serial, modifier_mask, 0, 0, 0);
                }
            } else {
                let key_state = if is_release { 0 } else { 1 };
                if keycode != 0 {
                    input_method_context.key(serial, time, keycode - 8, key_state);
                } else {
                    input_method_context.keysym(serial, time, keyval, key_state, state);
                }
            }
            Ok(())
        }

        async fn select_candidate(&self, _index: i32) -> ZbusResult<()> {
            Ok(())
        }

        async fn prev_page(&self, _index: i32) -> ZbusResult<()> {
            Ok(())
        }

        async fn next_page(&self, _index: i32) -> ZbusResult<()> {
            Ok(())
        }

        async fn reset_pressed_key_events(&mut self) -> ZbusResult<()> {
            // Since only shift in all modifier keys is handled, there is no need to reset
            Ok(())
        }
    }

    struct WaylandInputMethodV1Server {
        #[allow(unused)]
        tx: UnboundedSender<WaylandMessage>,
        input_method_context: Arc<Mutex<Option<ZwpInputMethodContextV1>>>,
    }

    impl WaylandInputMethodV1Server {
        fn input_method_context(&self) -> MutexGuard<'_, Option<ZwpInputMethodContextV1>> {
            self.input_method_context
                .lock()
                .expect("wayland input method context v1 is poisoned")
        }
    }

    impl Drop for WaylandInputMethodV1Server {
        fn drop(&mut self) {
            if let Some(context) = self.input_method_context().take() {
                tracing::debug!("destroy zwp_input_method_context_v1 during drop");
                context.destroy();
            }
        }
    }

    impl Dispatch<ZwpInputMethodContextV1, ()> for WaylandInputMethodV1Server {
        fn event(
            _state: &mut Self,
            _proxy: &ZwpInputMethodContextV1,
            _event: <ZwpInputMethodContextV1 as Proxy>::Event,
            _data: &(),
            _conn: &Connection,
            _qhandle: &QueueHandle<Self>,
        ) {
        }
    }

    impl Dispatch<ZwpInputMethodV1, ()> for WaylandInputMethodV1Server {
        fn event(
            state: &mut Self,
            _proxy: &ZwpInputMethodV1,
            event: <ZwpInputMethodV1 as Proxy>::Event,
            _data: &(),
            _conn: &Connection,
            _qhandle: &QueueHandle<Self>,
        ) {
            let res = match event {
                ZwpInputMethodV1Event::Activate { id } => {
                    tracing::debug!("wayland input method v1 activate");
                    let mut guard = state.input_method_context();
                    let old = guard.replace(id);
                    if let Some(old) = old {
                        old.destroy();
                    }
                    state.tx.unbounded_send(WaylandMessage::from(Message::from(
                        ImPanelEvent::Show(true),
                    )))
                }
                ZwpInputMethodV1Event::Deactivate { context } => {
                    tracing::debug!("wayland input method v1 deactivate");
                    let mut guard = state.input_method_context();
                    if guard.as_ref() == Some(&context) {
                        guard.take();
                    }
                    context.destroy();
                    // Hide the window. In Kwin, if the virtual keyboard button is clicked in
                    // kscreenlock, a activate signal will be sent, the window will show again.
                    state
                        .tx
                        .unbounded_send(Message::from(ImPanelEvent::Hide(true)).into())
                }
                _ => Ok(()),
            };
            if res.is_err() {
                tracing::error!("unable to send wayland input-method-v1 event");
            }
        }

        event_created_child!(WaylandInputMethodV1Server, ZwpInputMethodV1, [
            zwp_input_method_v1::EVT_ACTIVATE_OPCODE => (ZwpInputMethodContextV1, ()),
        ]);
    }

    pub fn new(
        connection: &WaylandConnection,
        tx: UnboundedSender<WaylandMessage>,
    ) -> Result<(
        WaylandInputMethodV1Client,
        impl Future<Output = Result<()>> + 'static,
    )> {
        let state = connection.state()?;
        let connection = state.connection();
        let global_list = state.global_list()?;

        let mut event_queue = connection.new_event_queue::<WaylandInputMethodV1Server>();
        let qh = event_queue.handle();
        let input_method_context = Arc::new(Mutex::new(None));

        global_list
            .bind::<ZwpInputMethodV1, _, _>(&qh, 1..=1, ())
            .context("failed to bind ZwpInputMethodV1")?;

        let client = WaylandInputMethodV1Client {
            serial: AtomicU32::new(0),
            input_method_context: input_method_context.clone(),
        };
        let mut server = WaylandInputMethodV1Server {
            tx,
            input_method_context,
        };
        Ok((client, async move {
            // we should not use blocking_dispatch here. because layershellev uses blocking_dispatch, if we use blocking_dispatch it will freeze the eventloop of layershellev
            loop {
                std::future::poll_fn(|cx| event_queue.poll_dispatch_pending(cx, &mut server))
                    .await?;
            }
        }))
    }
}
