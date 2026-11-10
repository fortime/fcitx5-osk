use std::sync::{Arc, Mutex, MutexGuard};

use anyhow::Result;
use iced::{
    Subscription,
    futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender},
};
use tokio::task::JoinHandle;
use v1::Fcitx5ControllerServiceStub;

use crate::{app::wayland::WaylandMessage, misc::NamedSubscriptionData, state::KeyboardBackend};

use super::connection::WaylandConnection;

struct State {
    rx: Option<UnboundedReceiver<WaylandMessage>>,
    keyboard_backend: Option<KeyboardBackend>,
    bg_handle: Option<JoinHandle<()>>,
    closed: bool,
}

impl State {
    pub fn new(rx: UnboundedReceiver<WaylandMessage>) -> Self {
        Self {
            rx: Some(rx),
            keyboard_backend: None,
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
        fn wayland_input_method_subscription(
            data: &NamedSubscriptionData<Arc<Mutex<State>>>,
        ) -> UnboundedReceiver<WaylandMessage> {
            if let Some(rx) = data.data().lock().ok().and_then(|mut s| s.rx.take()) {
                rx
            } else {
                // should always return a subscription with the same id, otherwise, the first one will
                // be dropped.
                let (_, rx) = mpsc::unbounded();
                rx
            }
        }
        Subscription::run_with(
            NamedSubscriptionData::new("external::wayland_input_method", self.state.clone()),
            wayland_input_method_subscription,
        )
    }

    pub fn keyboard_backend(&self) -> Result<KeyboardBackend> {
        let mut guard = self.state();
        if let Some(keyboard_backend) = &guard.keyboard_backend {
            Ok(keyboard_backend.clone())
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
            let keyboard_backend = KeyboardBackend::new_with(stub.clone(), stub, client);
            guard.keyboard_backend = Some(keyboard_backend.clone());
            Ok(keyboard_backend)
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
        drop(guard.keyboard_backend.take());
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
        future::{self, Future},
        sync::{Arc, Mutex, MutexGuard},
        time::{Duration, Instant},
    };

    use anyhow::{Context, Result};
    use iced::futures::channel::mpsc::{self, UnboundedSender};
    use tokio::time;
    use wayland_client::{
        Connection, Dispatch, Proxy, QueueHandle, WEnum, event_created_child,
        protocol::wl_keyboard::{Event as WlKeyboardEvent, KeyState, WlKeyboard},
    };
    use wayland_protocols::wp::input_method::zv1::client::{
        zwp_input_method_context_v1::ZwpInputMethodContextV1,
        zwp_input_method_v1::{self, Event as ZwpInputMethodV1Event, ZwpInputMethodV1},
    };
    use zbus::{Error as ZbusError, Result as ZbusResult};

    use crate::{
        app::{
            Message,
            wayland::{WaylandMessage, connection::WaylandConnection},
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

    #[derive(Debug)]
    struct InputMethodContext {
        zwp_input_method_context: ZwpInputMethodContextV1,
        serial: u32,
        mods_depressed: u32,
        mods_latched: u32,
        mods_locked: u32,
        group: u32,
    }

    impl InputMethodContext {
        fn next_serial(&mut self) -> u32 {
            let serial = self.serial;
            self.serial = self.serial.wrapping_add(1);
            serial
        }
    }

    /// Among a sequence of Wayland messages, there are `Activate` and `Deactivate` events for the
    /// Wayland input method context. If we grab the keyboard when `activate` is received and
    /// release it when `deactivate` is received, a Wayland protocol error occurs, reporting that
    /// the keyboard object does not exist.
    ///
    /// This is because the context was already destroyed when the keyboard is grabbed. And the
    /// keyboard won't be created at the server side. I think this is a limitation of
    /// input-method-v1.
    ///
    /// Here is a not perfect workaround. It might cause memory leak. We will only release a
    /// keyboard which lasts longer than 100ms.
    #[derive(Debug)]
    struct Keyboard {
        kb: WlKeyboard,
        created_at: Instant,
    }

    impl Keyboard {
        fn new(kb: WlKeyboard) -> Self {
            Self {
                kb,
                created_at: Instant::now(),
            }
        }

        fn release(&mut self) {
            if self.created_at.elapsed() > Duration::from_millis(100) {
                self.kb.release();
            }
        }
    }

    /// WaylandInputMethodV1Client implements IFcitx5VirtualKeyboardBackendService only, so it
    /// doesn't need to be `Clone`.
    #[derive(Debug)]
    pub struct WaylandInputMethodV1Client {
        input_method_context: Arc<Mutex<Option<InputMethodContext>>>,
    }

    impl WaylandInputMethodV1Client {
        fn input_method_context(&self) -> MutexGuard<'_, Option<InputMethodContext>> {
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
        #[tracing::instrument(level = "debug", skip(self), err, ret)]
        async fn show_virtual_keyboard(&self) -> ZbusResult<()> {
            Ok(())
        }

        #[tracing::instrument(level = "debug", skip(self), err, ret)]
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
            let mut guard = self.input_method_context();
            let Some(input_method_context) = guard.as_mut() else {
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

            // Since KWin 6.8, we can only use `key` and `modifiers` after we grab the keyboard.
            let serial = input_method_context.next_serial();
            if let Some(modifier_mask) = modifier_mask {
                // Shift is pressed
                if input_method_context.mods_depressed & modifier_mask == 0 {
                    if is_release {
                        input_method_context.zwp_input_method_context.modifiers(
                            serial,
                            input_method_context.mods_depressed,
                            input_method_context.mods_latched,
                            input_method_context.mods_locked,
                            input_method_context.group,
                        );
                    } else {
                        input_method_context.zwp_input_method_context.modifiers(
                            serial,
                            input_method_context.mods_depressed | modifier_mask,
                            input_method_context.mods_latched,
                            input_method_context.mods_locked,
                            input_method_context.group,
                        );
                    }
                }
            } else {
                let key_state = if is_release { 0 } else { 1 };
                if keycode != 0 {
                    input_method_context.zwp_input_method_context.key(
                        serial,
                        time,
                        keycode - 8,
                        key_state,
                    );
                } else {
                    // After KWin 6.8, it finally can handle maybe all keysyms. But actually the
                    // state won't be used, and we can't use other modifier keys to generate key
                    // combo.
                    input_method_context
                        .zwp_input_method_context
                        .keysym(serial, time, keyval, key_state, state);
                }
            }
            Ok(())
        }

        #[tracing::instrument(level = "debug", skip(self), err, ret)]
        async fn select_candidate(&self, _index: i32) -> ZbusResult<()> {
            Ok(())
        }

        #[tracing::instrument(level = "debug", skip(self), err, ret)]
        async fn prev_page(&self, _index: i32) -> ZbusResult<()> {
            Ok(())
        }

        #[tracing::instrument(level = "debug", skip(self), err, ret)]
        async fn next_page(&self, _index: i32) -> ZbusResult<()> {
            Ok(())
        }

        #[tracing::instrument(level = "debug", skip(self), err, ret)]
        async fn reset_pressed_key_events(&mut self) -> ZbusResult<()> {
            // Since only shift in all modifier keys is handled, there is no need to reset
            Ok(())
        }
    }

    struct RepeatKey {
        time: u32,
        key: u32,
        next: Instant,
        interval: Duration,
        pressed: bool,
    }

    struct WaylandInputMethodV1Server {
        tx: UnboundedSender<WaylandMessage>,
        input_method_context: Arc<Mutex<Option<InputMethodContext>>>,
        // time, key, deplay, rate, repeat
        repeat_tx: UnboundedSender<Option<RepeatKey>>,
        /// grab the keyboard so that we can use `input_method_context.key` and
        /// `input_method_context.modifiers`
        keyboard: Option<Keyboard>,
        /// the rate of repeating keys in characters per second
        keyboard_repeat_rate: u32,
        /// delay in milliseconds since key down until repeating starts
        keyboard_repeat_delay: u32,
    }

    impl WaylandInputMethodV1Server {
        fn new(
            tx: UnboundedSender<WaylandMessage>,
            input_method_context: Arc<Mutex<Option<InputMethodContext>>>,
        ) -> Self {
            let (repeat_tx, mut repeat_rx) = mpsc::unbounded::<Option<RepeatKey>>();

            tokio::spawn({
                let input_method_context = input_method_context.clone();
                async move {
                    let mut cur_repeat_key: Option<RepeatKey> = None;
                    let mut next_repeat_time = None;
                    loop {
                        tokio::select! {
                            () = sleep_until(next_repeat_time) => {
                                if let Some(repeat_key) = &mut cur_repeat_key {
                                    let mut guard = input_method_context
                                        .lock()
                                        .expect("input method context is poisoned");
                                    let Some(input_method_context) = guard.as_mut() else {
                                        tracing::error!("No input context, skip repeating key[{}]", repeat_key.key);
                                        next_repeat_time = None;
                                        continue;
                                    };
                                    let serial = input_method_context.next_serial();
                                    input_method_context.zwp_input_method_context.key(
                                        serial,
                                        repeat_key.time,
                                        repeat_key.key,
                                        KeyState::Pressed as u32,
                                    );
                                    repeat_key.next += repeat_key.interval;
                                    repeat_key.time += repeat_key.interval.as_millis() as u32;
                                    next_repeat_time = Some(repeat_key.next);
                                }
                            },
                            repeat_key = repeat_rx.recv() => match repeat_key {
                                Ok(repeat_key) => match repeat_key {
                                    Some(repeat_key) => {
                                        if repeat_key.pressed {
                                            tracing::debug!(
                                                "repeat key, time[{:?}], key[{}], next[{:?}], interval[{:?}]",
                                                repeat_key.time,
                                                repeat_key.key,
                                                repeat_key.next,
                                                repeat_key.interval,
                                            );
                                            next_repeat_time = Some(repeat_key.next);
                                            cur_repeat_key = Some(repeat_key);
                                        } else {
                                            cur_repeat_key.take_if(|c| c.key == repeat_key.key);
                                            if cur_repeat_key.is_none() {
                                                next_repeat_time = None;
                                            }
                                        }
                                    }
                                    None => {
                                        cur_repeat_key.take();
                                        next_repeat_time = None;
                                    }
                                },
                                Err(_) => {
                                    tracing::info!("repeat_tx is dropped, quit background");
                                    break;
                                }
                            }
                        }
                    }
                }
            });

            Self {
                tx,
                input_method_context,
                repeat_tx,
                keyboard: None,
                keyboard_repeat_rate: 0,
                keyboard_repeat_delay: 0,
            }
        }

        fn input_method_context(&self) -> MutexGuard<'_, Option<InputMethodContext>> {
            self.input_method_context
                .lock()
                .expect("input method context is poisoned")
        }

        fn input_method_context_and_keyboard(
            &mut self,
        ) -> (
            MutexGuard<'_, Option<InputMethodContext>>,
            &'_ mut Option<Keyboard>,
        ) {
            (
                self.input_method_context
                    .lock()
                    .expect("input method context is poisoned"),
                &mut self.keyboard,
            )
        }
    }

    impl Drop for WaylandInputMethodV1Server {
        fn drop(&mut self) {
            if let Some(context) = self.input_method_context().take() {
                tracing::debug!("destroy zwp_input_method_context_v1 during drop");
                context.zwp_input_method_context.destroy();
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
            qhandle: &QueueHandle<Self>,
        ) {
            let res = match event {
                ZwpInputMethodV1Event::Activate { id } => {
                    tracing::debug!(
                        "wayland input method context[{:?}] activate",
                        id.id().protocol_id()
                    );
                    let (mut guard, keyboard) = state.input_method_context_and_keyboard();

                    // release the old keyboard then grab a new one
                    if let Some(mut keyboard) = keyboard.take() {
                        keyboard.release();
                    }
                    *keyboard = Some(Keyboard::new(id.grab_keyboard(qhandle, ())));

                    let old = guard.replace(InputMethodContext {
                        zwp_input_method_context: id,
                        serial: 0,
                        mods_depressed: 0,
                        mods_latched: 0,
                        mods_locked: 0,
                        group: 0,
                    });
                    if let Some(old) = old {
                        old.zwp_input_method_context.destroy();
                    }
                    drop(guard);

                    // reset repeat info
                    state.keyboard_repeat_rate = 0;
                    state.keyboard_repeat_delay = 0;
                    let _ = state.repeat_tx.unbounded_send(None);
                    state.tx.unbounded_send(WaylandMessage::from(Message::from(
                        ImPanelEvent::Show(true),
                    )))
                }
                ZwpInputMethodV1Event::Deactivate { context } => {
                    tracing::debug!(
                        "wayland input method context[{:?}] deactivate",
                        context.id().protocol_id()
                    );
                    let mut guard = state.input_method_context();
                    if guard
                        .as_ref()
                        .filter(|imc| imc.zwp_input_method_context == context)
                        .is_some()
                    {
                        tracing::debug!("remove wayland input method context");
                        guard.take();
                        drop(guard);
                        if let Some(mut keyboard) = state.keyboard.take() {
                            keyboard.release();
                        }
                    }
                    context.destroy();
                    tracing::debug!("wayland input method context is destroyed");
                    let _ = state.repeat_tx.unbounded_send(None);
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

    impl Dispatch<WlKeyboard, ()> for WaylandInputMethodV1Server {
        fn event(
            state: &mut Self,
            _proxy: &WlKeyboard,
            event: <WlKeyboard as Proxy>::Event,
            _data: &(),
            _conn: &Connection,
            _qhandle: &QueueHandle<Self>,
        ) {
            match event {
                WlKeyboardEvent::Key {
                    serial: _serial,
                    time,
                    key,
                    state: key_state,
                } => {
                    tracing::debug!("keyboard key event: {event:?}");
                    let mut guard = state.input_method_context();
                    let Some(input_method_context) = guard.as_mut() else {
                        tracing::error!("No input context, skip event[{event:?}]");
                        return;
                    };
                    let mut need_repeat = false;
                    let key_state = match key_state {
                        WEnum::Value(KeyState::Released) => {
                            need_repeat = state.keyboard_repeat_rate > 0;
                            KeyState::Released
                        }
                        WEnum::Value(KeyState::Pressed) => KeyState::Pressed,
                        WEnum::Value(KeyState::Repeated) => {
                            // stop repeating
                            let _ = state.repeat_tx.unbounded_send(None);
                            KeyState::Pressed
                        }
                        _ => {
                            tracing::error!("Invalid key state: {key_state:?}");
                            return;
                        }
                    };
                    let serial = input_method_context.next_serial();
                    input_method_context.zwp_input_method_context.key(
                        serial,
                        time,
                        key,
                        key_state as u32,
                    );
                    if state.keyboard_repeat_rate > 0 {
                        let _ = state.repeat_tx.unbounded_send(Some(RepeatKey {
                            time: time + state.keyboard_repeat_delay,
                            key,
                            next: Instant::now()
                                + Duration::from_millis(state.keyboard_repeat_delay as u64),
                            interval: Duration::from_millis(
                                (1000 / state.keyboard_repeat_rate as u64).min(1),
                            ),
                            pressed: need_repeat,
                        }));
                    }
                }
                WlKeyboardEvent::Modifiers {
                    serial: _serial,
                    mods_depressed,
                    mods_latched,
                    mods_locked,
                    group,
                } => {
                    tracing::debug!("keyboard modifiers event: {event:?}");
                    let mut guard = state.input_method_context();
                    let Some(input_method_context) = guard.as_mut() else {
                        tracing::error!("No input context, skip event[{event:?}]");
                        return;
                    };
                    let serial = input_method_context.next_serial();
                    input_method_context.mods_depressed = mods_depressed;
                    input_method_context.mods_latched = mods_latched;
                    input_method_context.mods_locked = mods_locked;
                    input_method_context.group = group;
                    input_method_context.zwp_input_method_context.modifiers(
                        serial,
                        mods_depressed,
                        mods_latched,
                        mods_locked,
                        group,
                    );
                }
                WlKeyboardEvent::RepeatInfo { rate, delay } => {
                    tracing::info!("Update keyboard repeat rate[{rate}], repeat delay[{delay}]");
                    if rate >= 0 && delay >= 0 {
                        let changed = state.keyboard_repeat_rate != rate as u32
                            || state.keyboard_repeat_delay != delay as u32;
                        state.keyboard_repeat_rate = rate as u32;
                        state.keyboard_repeat_delay = delay as u32;
                        if changed {
                            let _ = state.repeat_tx.unbounded_send(None);
                        }
                    }
                }
                _ => {
                    tracing::info!("Ignore wl_keyboard event: {event:?}");
                }
            }
        }
    }

    async fn sleep_until(t: Option<Instant>) {
        if let Some(t) = t {
            time::sleep(t.duration_since(Instant::now())).await;
        } else {
            future::pending().await
        }
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
            input_method_context: input_method_context.clone(),
        };
        let mut server = WaylandInputMethodV1Server::new(tx, input_method_context);
        Ok((client, async move {
            // we should not use blocking_dispatch here. because layershellev uses blocking_dispatch, if we use blocking_dispatch it will freeze the eventloop of layershellev
            loop {
                future::poll_fn(|cx| event_queue.poll_dispatch_pending(cx, &mut server)).await?;
            }
        }))
    }
}
