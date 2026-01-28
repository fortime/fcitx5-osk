use std::{
    cell::RefCell,
    future::{self, Future},
    os::fd::{AsRawFd, OwnedFd},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use anyhow::{Context, Error, Result};
use fcitx5_osk_common::signal::ShutdownFlag;
use iced::{
    futures::{
        channel::{
            mpsc::{self, UnboundedReceiver, UnboundedSender},
            oneshot::{self, Sender},
        },
        stream,
    },
    widget::{self, Column},
    window::{Event as IcedWindowEvent, Id},
    Color, Element, Event as IcedEvent, Subscription, Task, Theme,
};
use iced_futures::event;
use tokio::time;
use zbus::Connection;

use crate::{
    config::{Config, ConfigManager},
    dbus::{
        client::{Fcitx5Services, FdoPortalSettingsServiceProxy},
        server::{
            Fcitx5OskService, Fcitx5OskServiceClient, Fcitx5VirtualkeyboardImPanelEvent,
            Fcitx5VirtualkeyboardImPanelService, ImPanelEvent, SocketEnv,
        },
    },
    state::{
        CloseOpSource, ImEvent, KeyEvent, KeyboardEvent, LayoutEvent, State, StateExtractor,
        StoreEvent, ThemeEvent, UpdateConfigEvent, WindowEvent, WindowManagerEvent,
    },
    window::{self, WindowAppearance, WindowManager},
};

pub mod wayland;
pub mod x11;

#[derive(Clone, Debug)]
pub enum Message {
    AfterError,
    Error(KeyboardError),
    Fcitx5VirtualkeyboardImPanelEvent(Fcitx5VirtualkeyboardImPanelEvent),
    ImEvent(ImEvent),
    ImPanelEvent(ImPanelEvent),
    KeyEvent(KeyEvent),
    KeyboardEvent(KeyboardEvent),
    LayoutEvent(LayoutEvent),
    Nothing,
    StoreEvent(StoreEvent),
    ThemeEvent(ThemeEvent),
    UpdateConfigEvent(UpdateConfigEvent),
    UpdateFcitx5Services(Fcitx5Services),
    WindowEvent(WindowEvent),
    WindowManagerEvent(WindowManagerEvent),
}

impl Message {
    /// Chaining Task::none() will discard previous tasks, so we create a shortcut like
    /// Task::none() but it won't discard previous tasks.
    pub fn nothing() -> Task<Self> {
        Task::done(Message::Nothing)
    }

    pub fn from_nothing<T>() -> Task<T>
    where
        T: From<Message> + 'static + Send,
    {
        Task::done(Message::Nothing.into())
    }
}

pub(crate) trait MapTask<T> {
    fn map_task(self) -> Task<T>;
}

impl<T> MapTask<T> for Task<Message>
where
    T: From<Message> + 'static + Send,
{
    fn map_task(self) -> Task<T> {
        self.map(|t| t.into())
    }
}

trait ErrorDialogContent {
    fn err_msg(&self) -> String;

    fn button_text(&self) -> String;
}

#[derive(Clone, Debug)]
pub enum KeyboardError {
    Error(Arc<Error>),
    Fatal(Arc<Error>),
}

impl ErrorDialogContent for &KeyboardError {
    fn err_msg(&self) -> String {
        match self {
            KeyboardError::Error(e) => format!("Error: {e}"),
            KeyboardError::Fatal(e) => format!("Fatal error: {e}"),
        }
    }

    fn button_text(&self) -> String {
        match self {
            KeyboardError::Error(_) => "Close".to_string(),
            KeyboardError::Fatal(_) => "Exit".to_string(),
        }
    }
}

impl ErrorDialogContent for &str {
    fn err_msg(&self) -> String {
        self.to_string()
    }

    fn button_text(&self) -> String {
        "Close".to_string()
    }
}

impl From<KeyboardError> for Message {
    fn from(value: KeyboardError) -> Self {
        Self::Error(value)
    }
}

impl KeyboardError {
    fn is_priority_over(&self, other: &Self) -> bool {
        if let KeyboardError::Fatal(_) = self {
            return true;
        }
        if let KeyboardError::Fatal(_) = other {
            return false;
        }
        true
    }
}

/// State that should be initialized in a async runtime.
pub struct AsyncAppState {
    fcitx5_services: Fcitx5Services,
    fcitx5_osk_service_client: Fcitx5OskServiceClient,
    rx: RefCell<Option<UnboundedReceiver<Message>>>,
    display_socket: Option<OwnedFd>,
    detect_theme_enabled: Arc<AtomicBool>,
}

impl AsyncAppState {
    pub async fn new(
        config: &Config,
        wait_for_socket: bool,
        modifier_workaround: bool,
        shutdown_flag: ShutdownFlag,
    ) -> Result<Self> {
        let (tx, rx) = mpsc::unbounded();
        let (socket_env_tx, socket_env_rx) = if wait_for_socket {
            let (tx, rx) = oneshot::channel();
            (Some(tx), Some(rx))
        } else {
            (None, None)
        };
        let fcitx5_osk_service_client =
            start_dbus_services(tx.clone(), socket_env_tx, shutdown_flag.clone()).await?;
        let mut display_socket = None;
        if let Some(socket_env_rx) = socket_env_rx {
            unsafe {
                // Safety, there should be no other thread running yet
                match socket_env_rx
                    .await
                    .context("failed to receive socket env")?
                {
                    SocketEnv::WaylandSocket(s) => {
                        window::wayland::set_env(Some(&s.as_raw_fd().to_string()), None);
                        display_socket = Some(s);
                    }
                    SocketEnv::WaylandDisplay(s) => window::wayland::set_env(None, Some(&s)),
                    SocketEnv::X11Display(s) => window::x11::set_env(Some(&s)),
                }
            }
        }
        let fcitx5_services = Fcitx5Services::new(
            modifier_workaround,
            config.modifier_workaround_keycodes().clone(),
        )
        .await?;
        let detect_theme_enabled = Arc::new(AtomicBool::new(false));
        tokio::spawn(detect_theme(
            tx,
            shutdown_flag,
            detect_theme_enabled.clone(),
        ));
        Ok(Self {
            fcitx5_services,
            fcitx5_osk_service_client,
            rx: RefCell::new(Some(rx)),
            display_socket,
            detect_theme_enabled,
        })
    }
}

pub struct Keyboard<WM> {
    state: State<WM>,
    error: Option<KeyboardError>,
    shutdown_flag: ShutdownFlag,
    shutdown_sent: bool,
    fcitx5_osk_service_client: Fcitx5OskServiceClient,
    rx: RefCell<Option<UnboundedReceiver<Message>>>,
    // Hold the socket so that it won't be closed
    #[allow(unused)]
    display_socket: Option<OwnedFd>,
}

impl<WM> Keyboard<WM> {
    pub fn new(
        async_state: AsyncAppState,
        config_manager: ConfigManager,
        wm: WM,
        wait_for_socket: bool,
        shutdown_flag: ShutdownFlag,
    ) -> (Self, Task<Message>) {
        let AsyncAppState {
            fcitx5_services,
            fcitx5_osk_service_client,
            rx,
            display_socket,
            detect_theme_enabled,
        } = async_state;

        fcitx5_osk_service_client.set_manual_mode(config_manager.as_ref().manual_mode());
        let state = State::new(config_manager, wm, fcitx5_services, detect_theme_enabled);
        let mut init_task = Task::done(StoreEvent::Load.into());
        if !wait_for_socket {
            // open indicator if it is not waiting for a socket.
            init_task = Task::done(WindowManagerEvent::OpenIndicator.into());
        }
        (
            Self {
                state,
                error: None,
                shutdown_flag,
                shutdown_sent: false,
                fcitx5_osk_service_client,
                rx,
                display_socket,
            },
            init_task,
        )
    }

    pub fn handle_error_message(&mut self, e: KeyboardError) {
        match &e {
            KeyboardError::Error(e) => tracing::error!("Error: {e:#}"),
            KeyboardError::Fatal(e) => tracing::error!("Fatal error: {e:?}"),
        }
        if let Some(existing) = &self.error {
            if existing.is_priority_over(&e) {
                tracing::warn!("skip error, error is drop: {:?}", e);
            } else {
                tracing::warn!("overwrite existing error, error is drop: {:?}", existing);
                self.error = Some(e);
            }
        } else {
            self.error = Some(e);
        }
    }
}

impl<WM> Keyboard<WM>
where
    WM: WindowManager,
    WM::Message: From<Message> + 'static + Send + Sync,
    WM::Appearance: WindowAppearance + 'static + Send + Sync,
{
    fn error_dialog<T: ErrorDialogContent>(&self, e: T) -> Element<Message> {
        let err_msg = e.err_msg();
        let button_text = e.button_text();
        widget::container(
            widget::column![
                widget::text(err_msg),
                widget::button(widget::text(button_text)).on_press(Message::AfterError),
            ]
            .spacing(10)
            .padding(10),
        )
        .max_width(self.state.window_manager().size().width)
        .style(widget::container::rounded_box)
        .into()
    }

    pub fn view(&self, id: Id) -> Element<WM::Message> {
        let visible = self.fcitx5_osk_service_client.visible().unwrap_or(true);
        if visible && self.state.window_manager().is_keyboard(id) {
            let base = self.state.to_element(id);
            let res = if let Some(e) = &self.error {
                modal(base, self.error_dialog(e), Message::AfterError)
            } else {
                base
            };
            res.map(|m| m.into())
        } else if visible && self.state.window_manager().is_indicator(id) {
            self.state.to_element(id).map(|m| m.into())
        } else {
            Column::new().into()
        }
    }

    /// subscription will be called after each batch updates, iced will check if streams in it has
    /// been changed.
    pub fn subscription(&self) -> Subscription<WM::Message> {
        let mut subscriptions = vec![event::listen_with(|event, status, id| {
            tracing::trace!("event: {}, {:?}, {:?}", id, status, event);
            match event {
                IcedEvent::Window(IcedWindowEvent::Opened {
                    position: _position,
                    size,
                }) => {
                    // ignore position, position isn't supported in wayland
                    Some(Message::from(WindowEvent::Opened(id, size)))
                }
                IcedEvent::Window(IcedWindowEvent::Closed) => {
                    tracing::debug!("closed: {}", id);
                    Some(Message::from(WindowEvent::Closed(id)))
                }
                IcedEvent::Keyboard(event) => {
                    tracing::debug!("keyboard event: {:?}", event);
                    None
                }
                _ => None,
            }
        })];

        const EXTERNAL_SUBSCRIPTION_ID: &str = "external";
        if let Some(rx) = self.rx.borrow_mut().take() {
            subscriptions.push(Subscription::run_with_id(EXTERNAL_SUBSCRIPTION_ID, rx));
        } else {
            // should always return a subscription with the same id, otherwise, the first one will
            // be dropped.
            subscriptions.push(Subscription::run_with_id(
                EXTERNAL_SUBSCRIPTION_ID,
                stream::empty(),
            ));
        }

        Subscription::batch(subscriptions).map(|m| m.into())
    }

    pub fn update(&mut self, message: Message) -> Task<WM::Message> {
        if self.shutdown_flag.get() {
            tracing::warn!("message[{:?}] is ignored after shutdown", message);
            if !self.shutdown_sent {
                self.shutdown_sent = true;
                return self.state.window_manager_mut().shutdown();
            } else {
                return iced::exit();
            }
        }
        if let Message::Nothing = message {
            return Task::none();
        } else {
            tracing::debug!("Update with message: {message:?}");
        }
        let mut task = Task::done(Message::Nothing.into());
        match message {
            Message::Nothing => unreachable!("Nothing should be return before here"),
            Message::Error(e) => self.handle_error_message(e),
            Message::AfterError => {
                if let Some(KeyboardError::Fatal(_)) = self.error.take() {
                    task = task.chain(self.state.window_manager_mut().shutdown());
                }
            }
            Message::KeyboardEvent(event) => {
                task = task.chain(self.state.keyboard_mut().on_event(event).map_task());
            }
            Message::LayoutEvent(event) => {
                task = task.chain(self.state.on_layout_event(event));
            }
            Message::KeyEvent(event) => {
                task = task.chain(self.state.keyboard_mut().on_key_event(event).map_task());
            }
            Message::WindowEvent(event) => {
                task = task.chain(self.state.window_manager_mut().on_window_event(event));
            }
            Message::WindowManagerEvent(event) => {
                let is_update_mode = matches!(event, WindowManagerEvent::UpdateMode(_));
                task = task.chain(self.state.window_manager_mut().on_event(event));
                if is_update_mode {
                    let mode = self.state.window_manager().mode();
                    self.fcitx5_osk_service_client.set_mode(mode);
                }
            }
            Message::StoreEvent(event) => {
                task = task.chain(self.state.on_store_event(event));
            }
            Message::ThemeEvent(event) => {
                self.state.on_theme_event(event);
            }
            Message::UpdateConfigEvent(event) => {
                task = task.chain(self.state.on_update_config_event(event));
            }
            Message::Fcitx5VirtualkeyboardImPanelEvent(event) => {
                match event {
                    Fcitx5VirtualkeyboardImPanelEvent::ShowVirtualKeyboard => {
                        if !self.state.config().manual_mode() {
                            task = task.chain(self.state.window_manager_mut().open_keyboard());
                        }
                    }
                    Fcitx5VirtualkeyboardImPanelEvent::HideVirtualKeyboard => {
                        // Always set fcitx5 hidden, so we can make sure virtual keyboard mode of fcitx5 will be activated
                        self.state.keyboard_mut().set_fcitx5_hidden();
                        if !self.state.config().manual_mode() {
                            // Close keyboard only when setting isn't shown
                            if !self.state.window_manager().is_setting_shown() {
                                task = task.chain(
                                    self.state
                                        .window_manager_mut()
                                        .close_keyboard(CloseOpSource::Fcitx5),
                                );
                            }
                        }
                    }
                    Fcitx5VirtualkeyboardImPanelEvent::UpdateCandidateArea(state) => {
                        self.state.im_mut().update_candidate_area_state(state);
                    }
                    _ => {}
                }
            }
            Message::ImPanelEvent(event) => {
                match event {
                    ImPanelEvent::Show(force) => {
                        if force || !self.state.config().manual_mode() {
                            task = task.chain(self.state.window_manager_mut().open_keyboard());
                        }
                    }
                    ImPanelEvent::Hide(force) => {
                        // always set fcitx5 hidden, so we can make sure virtual keyboard mode of fcitx5 will be activated.
                        self.state.keyboard_mut().set_fcitx5_hidden();
                        if force || !self.state.config().manual_mode() {
                            // Unlike hiding request from Fcitx5, we always think that request from DbusController should be followed.
                            task = task.chain(
                                self.state
                                    .window_manager_mut()
                                    .close_keyboard(CloseOpSource::DbusController),
                            );
                        }
                    }
                    ImPanelEvent::NewVisibleRequest(visible) => {
                        self.fcitx5_osk_service_client.new_visible_request(visible)
                    }
                    ImPanelEvent::UpdateManualMode(manual_mode) => {
                        self.fcitx5_osk_service_client.set_manual_mode(manual_mode)
                    }
                    ImPanelEvent::ReopenIfOpened => {
                        if let Some(next_task) =
                            self.state.window_manager_mut().reopen_keyboard_if_opened()
                        {
                            task = task.chain(next_task)
                        }
                        if let Some(next_task) =
                            self.state.window_manager_mut().reopen_indicator_if_opened()
                        {
                            task = task.chain(next_task)
                        }
                    }
                }
            }
            Message::ImEvent(event) => {
                task = task.chain(self.state.on_im_event(event));
            }
            Message::UpdateFcitx5Services(fcitx5_services) => {
                tracing::debug!("update fcitx5_services");
                self.state.update_fcitx5_services(fcitx5_services);
            }
        };
        task
    }

    pub fn appearance(&self, theme: &Theme, id: Id) -> WM::Appearance {
        let mut appearance = self.state.window_manager().appearance(theme, id);
        if !self.fcitx5_osk_service_client.visible().unwrap_or(true) {
            appearance.set_background_color(Color::TRANSPARENT);
        }
        appearance
    }

    pub fn theme(&self, id: Id) -> Theme {
        let visible = self.fcitx5_osk_service_client.visible().unwrap_or(true);
        // in iced, style doesn't accept id, we should return a theme with transparent background.
        let theme = self.state.theme().clone();
        let appearance = self.state.window_manager().appearance(&theme, id);
        if !visible || appearance.background_color() == Color::TRANSPARENT {
            let mut palette = theme.palette();
            palette.background = Color::TRANSPARENT;
            Theme::custom(String::new(), palette)
        } else {
            theme
        }
    }
}

fn modal<'a>(
    base: Element<'a, Message>,
    content: Element<'a, Message>,
    on_blur: Message,
) -> Element<'a, Message> {
    widget::stack![
        base,
        widget::opaque(
            widget::mouse_area(widget::center(widget::opaque(content)).style(|_theme| {
                widget::container::Style {
                    background: Some(
                        Color {
                            a: 0.8,
                            ..Color::BLACK
                        }
                        .into(),
                    ),
                    ..widget::container::Style::default()
                }
            }))
            .on_press(on_blur)
        )
    ]
    .into()
}

async fn detect_theme(
    tx: UnboundedSender<Message>,
    shutdown_flag: ShutdownFlag,
    detect_theme_enabled: Arc<AtomicBool>,
) {
    async fn service() -> Result<(Connection, FdoPortalSettingsServiceProxy<'static>)> {
        let connection = Connection::session().await?;
        let fdo_portal_settings_service = FdoPortalSettingsServiceProxy::new(&connection).await?;
        Ok((connection, fdo_portal_settings_service))
    }

    let mut ctx = None;

    'outer: while !shutdown_flag.get() {
        'inner: {
            if !detect_theme_enabled.load(Ordering::SeqCst) {
                break 'inner;
            }
            if ctx.is_none() {
                match service().await {
                    Ok(c) => ctx = Some(c),
                    Err(e) => tracing::error!("Failed to get FdoPortalSettingsService: {e:#?}"),
                };
            }

            let Some(ctx) = &ctx else {
                break 'inner;
            };

            let color_scheme = match ctx
                .1
                .read_one("org.freedesktop.appearance", "color-scheme")
                .await
            {
                Ok(color_scheme) => color_scheme,
                Err(e) => {
                    tracing::warn!("Failed to get color theme: {e:#?}");
                    break 'inner;
                }
            };

            let color_scheme = match u32::try_from(color_scheme) {
                Ok(color_scheme) => color_scheme,
                Err(e) => {
                    tracing::warn!("Unknown type of color theme: {e:#?}");
                    break 'inner;
                }
            };

            if tx
                .unbounded_send(ThemeEvent::Detected(color_scheme).into())
                .is_err()
            {
                tracing::warn!("failed to send ThemeEvent::Check message, close the task");
                break 'outer;
            }
        }
        time::sleep(Duration::from_millis(500)).await;
    }
}

async fn start_dbus_services(
    tx: UnboundedSender<Message>,
    socket_env_tx: Option<Sender<SocketEnv>>,
    mut shutdown_flag: ShutdownFlag,
) -> Result<Fcitx5OskServiceClient> {
    let conn = Connection::session().await?;
    let fcitx5_service = Fcitx5VirtualkeyboardImPanelService::new(tx.clone());
    fcitx5_service
        .start(&conn)
        .await
        .context("failed to start fcitx5 dbus service")?;
    let fcitx5_osk_service =
        Fcitx5OskService::new(tx.clone(), socket_env_tx, shutdown_flag.clone());
    let fcitx5_osk_service_client = fcitx5_osk_service
        .start(&conn)
        .await
        .context("failed to start fcitx5 osk dbus service")?;

    tokio::spawn(async move {
        shutdown_flag.wait_for_shutdown().await;
        tracing::info!("shutting down");
        // trigger shutdown check
        if tx.unbounded_send(Message::Nothing).is_err() {
            tracing::error!("failed to send message, shutdown may be stopped");
        }
        let mut count = 0;
        loop {
            let _ = future::poll_fn(|cx| tx.poll_ready(cx)).await;
            if tx.is_closed() {
                tracing::info!("the mainloop of keyboard has exited.");
                break;
            }
            count += 1;
            if count > 4 {
                break;
            }
            let _ = time::sleep(Duration::from_millis(500)).await;
        }
        // there is no way to have a graceful shutdown if the wayland socket is from kwin, kwin is
        // running in a signal thread, when input method is shutting down, kwin can't handle other
        // requests.
        tracing::info!("close the connection of dbus services");
        if let Err(e) = conn.close().await {
            tracing::warn!("failed to close the connection of dbus services: {:?}", e);
        }
    });
    Ok(fcitx5_osk_service_client)
}

/// this function should be run in multi_thread runtime, otherwise, it will be deadlocked.
fn run_async<T, F>(f: F) -> Result<T>
where
    T: Send + 'static,
    F: Future<Output = Result<T>> + 'static + Send,
{
    let (tx, rx) = std::sync::mpsc::channel();
    tokio::spawn(async move {
        tx.send(f.await).expect("unable to send the result");
    });
    rx.recv()?
}

pub fn error_with_context<E, M>(e: E, err_msg: M) -> Message
where
    E: Into<Error>,
    M: Into<String>,
{
    KeyboardError::Error(Arc::new(e.into().context(err_msg.into()))).into()
}

pub fn fatal_with_context<E, M>(e: E, err_msg: M) -> Message
where
    E: Into<Error>,
    M: Into<String>,
{
    KeyboardError::Fatal(Arc::new(e.into().context(err_msg.into()))).into()
}
