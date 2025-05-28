use std::{
    cell::RefCell,
    future::{self, Future},
    os::fd::{AsRawFd, OwnedFd},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Error, Result};
use fcitx5_osk_common::{dbus::client::Fcitx5OskServices, signal::ShutdownFlag};
use iced::{
    futures::{
        channel::mpsc::{self, UnboundedReceiver, UnboundedSender},
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
    config::ConfigManager,
    dbus::{
        client::Fcitx5Services,
        server::{
            Fcitx5OskService, Fcitx5VirtualkeyboardImPanelEvent,
            Fcitx5VirtualkeyboardImPanelService, ImPanelEvent, SocketEnv,
        },
    },
    state::{
        CloseOpSource, ImEvent, KeyEvent, KeyboardEvent, LayoutEvent, State, StateExtractor,
        ThemeEvent, UpdateConfigEvent, WindowEvent, WindowManagerEvent,
    },
    window::{self, WindowAppearance, WindowManager, WindowManagerMode},
};

pub mod wayland;
pub mod x11;

#[derive(Clone, Debug)]
pub enum Message {
    Nothing,
    KeyboardEvent(KeyboardEvent),
    Error(KeyboardError),
    AfterError,
    ImEvent(ImEvent),
    LayoutEvent(LayoutEvent),
    KeyEvent(KeyEvent),
    WindowEvent(WindowEvent),
    WindowManagerEvent(WindowManagerEvent),
    ThemeEvent(ThemeEvent),
    UpdateConfigEvent(UpdateConfigEvent),
    Fcitx5VirtualkeyboardImPanelEvent(Fcitx5VirtualkeyboardImPanelEvent),
    ImPanelEvent(ImPanelEvent),
    UpdateFcitx5Services(Fcitx5Services),
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

pub struct Keyboard<WM> {
    state: State<WM>,
    visible: bool,
    rx: RefCell<Option<UnboundedReceiver<Message>>>,
    socket_env_context: RefCell<(
        Option<std::sync::mpsc::Receiver<SocketEnv>>,
        Option<OwnedFd>,
    )>,
    error: Option<KeyboardError>,
    shutdown_flag: ShutdownFlag,
    shutdown_sent: bool,
}

impl<WM> Keyboard<WM>
where
    WM: Default,
{
    pub fn new(
        config_manager: ConfigManager,
        fcitx5_services: Fcitx5Services,
        fcitx5_osk_services: Fcitx5OskServices,
        wait_for_socket: bool,
        shutdown_flag: ShutdownFlag,
    ) -> Result<(Self, Task<Message>)> {
        let (tx, rx) = mpsc::unbounded();
        let (socket_env_tx, socket_env_rx) = if wait_for_socket {
            let (tx, rx) = std::sync::mpsc::channel();
            (Some(tx), Some(rx))
        } else {
            (None, None)
        };
        let state = State::new(config_manager, fcitx5_services, fcitx5_osk_services)?;
        let mut init_task = Task::future(init(tx, socket_env_tx, shutdown_flag.clone()));
        if !wait_for_socket {
            // open indicator if it is not waiting for a socket.
            init_task =
                init_task.chain(Task::done(Message::from(WindowManagerEvent::OpenIndicator)));
        }
        Ok((
            Self {
                state,
                visible: true,
                rx: RefCell::new(Some(rx)),
                socket_env_context: RefCell::new((socket_env_rx, None)),
                error: None,
                shutdown_flag,
                shutdown_sent: false,
            },
            init_task,
        ))
    }
}

impl<WM> Keyboard<WM> {
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
        if self.visible && self.state.window_manager().is_keyboard(id) {
            let base = self.state.to_element(id);
            let res = if let Some(e) = &self.error {
                modal(base, self.error_dialog(e), Message::AfterError)
            } else {
                base
            };
            res.map(|m| m.into())
        } else if self.visible && self.state.window_manager().is_indicator(id) {
            self.state.to_element(id).map(|m| m.into())
        } else {
            Column::new().into()
        }
    }

    /// subscription will be called after each batch updates, iced will check if streams in it has
    /// been changed.
    pub fn subscription(&self) -> Subscription<WM::Message> {
        // subscription will be called before creating a wayland connection, so we wait socket
        // here.
        let mut socket_env_context = self.socket_env_context.borrow_mut();
        if let Some(socket_env_rx) = socket_env_context.0.take() {
            unsafe {
                // No Safety Guarantee, multi_thread runtime has been started.
                // It is high probability that data race of env won't be happened.
                match socket_env_rx.recv() {
                    Ok(SocketEnv::WaylandSocket(s)) => {
                        window::wayland::set_env(Some(&s.as_raw_fd().to_string()), None);
                        socket_env_context.1 = Some(s);
                    }
                    Ok(SocketEnv::WaylandDisplay(s)) => window::wayland::set_env(None, Some(&s)),
                    Ok(SocketEnv::X11Display(s)) => window::x11::set_env(Some(&s)),
                    Err(_) => {
                        tracing::error!("failed to receive socket env");
                        self.shutdown_flag.shutdown();
                    }
                }
            }
        }

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
        tracing::debug!("update with message: {message:?}");
        if self.shutdown_flag.get() {
            tracing::warn!("message[{:?}] is ignored after shutdown", message);
            if !self.shutdown_sent {
                self.shutdown_sent = true;
                return self.state.window_manager_mut().shutdown();
            } else {
                return iced::exit();
            }
        }
        let mut task = Task::done(Message::Nothing.into());
        match message {
            Message::Nothing => task = Task::none(),
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
                self.state.on_layout_event(event);
            }
            Message::KeyEvent(event) => {
                task = task.chain(self.state.keyboard_mut().on_key_event(event).map_task());
            }
            Message::WindowEvent(event) => {
                task = task.chain(self.state.window_manager_mut().on_window_event(event));
            }
            Message::WindowManagerEvent(event) => {
                task = task.chain(self.state.window_manager_mut().on_event(event));
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
                        self.visible = true;
                        task = task.chain(self.state.window_manager_mut().open_keyboard());
                    }
                    Fcitx5VirtualkeyboardImPanelEvent::HideVirtualKeyboard => {
                        // always set fcitx5 hidden, so we can make sure virtual keyboard mode of fcitx5 will be activated.
                        self.state.keyboard_mut().set_fcitx5_hidden();
                        task = task.chain(
                            self.state
                                .window_manager_mut()
                                .close_keyboard(CloseOpSource::Fcitx5),
                        );
                    }
                    Fcitx5VirtualkeyboardImPanelEvent::UpdateCandidateArea(state) => {
                        self.state.im_mut().update_candidate_area_state(state);
                    }
                    _ => {}
                }
            }
            Message::ImPanelEvent(event) => {
                match event {
                    ImPanelEvent::Show => {
                        self.visible = true;
                        task = task.chain(self.state.window_manager_mut().open_keyboard());
                    }
                    ImPanelEvent::Hide => {
                        // always set fcitx5 hidden, so we can make sure virtual keyboard mode of fcitx5 will be activated.
                        self.state.keyboard_mut().set_fcitx5_hidden();
                        // Unlike hiding request from Fcitx5, we always think that request from DbusController should be followed.
                        task = task.chain(
                            self.state
                                .window_manager_mut()
                                .close_keyboard(CloseOpSource::DbusController),
                        );
                    }
                    ImPanelEvent::UpdateVisible(visible) => self.visible = visible,
                }
            }
            Message::ImEvent(event) => {
                task = task.chain(self.state.on_im_event(event).map_task());
            }
            Message::UpdateFcitx5Services(fcitx5_services) => {
                tracing::debug!("update fcitx5_services");
                self.state.update_fcitx5_services(fcitx5_services);
            }
        };
        task
    }

    pub fn appearance(&self, theme: &Theme, id: Id) -> WM::Appearance {
        self.state.window_manager().appearance(theme, id)
    }

    pub fn theme(&self, id: Id) -> Theme {
        // in iced, style doesn't accept id, we should return a theme with transparent background.
        let theme = self.state.theme().clone();
        let appearance = self.state.window_manager().appearance(&theme, id);
        if !self.visible || appearance.background_color() == Color::TRANSPARENT {
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

async fn init(
    tx: UnboundedSender<Message>,
    socket_env_tx: Option<std::sync::mpsc::Sender<SocketEnv>>,
    shutdown_flag: ShutdownFlag,
) -> Message {
    tokio::spawn({
        let tx = tx.clone();
        let shutdown_flag = shutdown_flag.clone();
        async move {
            if let Err(e) =
                start_dbus_services(tx.clone(), socket_env_tx, shutdown_flag.clone()).await
            {
                tracing::error!("failed to start dbus services: {:?}, exit", e);
                if tx
                    .unbounded_send(KeyboardError::Fatal(Arc::new(e)).into())
                    .is_err()
                {
                    tracing::error!("failed to send fatal message");
                    shutdown_flag.shutdown();
                }
            }
        }
    });

    tokio::spawn(start_detect_theme(tx, shutdown_flag.clone()));

    // trigger to set mode property
    Message::from(WindowManagerEvent::UpdateMode(WindowManagerMode::Normal))
}

async fn start_detect_theme(tx: UnboundedSender<Message>, shutdown_flag: ShutdownFlag) {
    while !shutdown_flag.get() {
        if tx.unbounded_send(ThemeEvent::Detect.into()).is_err() {
            tracing::warn!("failed to send ThemeEvent::Check message, close the task");
            break;
        }
        time::sleep(Duration::from_millis(500)).await;
    }
}

async fn start_dbus_services(
    tx: UnboundedSender<Message>,
    socket_env_tx: Option<std::sync::mpsc::Sender<SocketEnv>>,
    mut shutdown_flag: ShutdownFlag,
) -> Result<()> {
    let conn = Connection::session().await?;
    let fcitx5_service = Fcitx5VirtualkeyboardImPanelService::new(tx.clone());
    fcitx5_service
        .start(&conn)
        .await
        .context("failed to start fcitx5 dbus service")?;
    let fcitx5_osk_service =
        Fcitx5OskService::new(tx.clone(), socket_env_tx, shutdown_flag.clone());
    fcitx5_osk_service
        .start(&conn)
        .await
        .context("failed to start fcitx5 osk dbus service")?;
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
        let _ = tokio::time::sleep(Duration::from_millis(500)).await;
    }
    // there is no way to have a graceful shutdown if the wayland socket is from kwin, kwin is
    // running in a signal thread, when input method is shutting down, kwin can't handle other
    // requests.
    tracing::info!("close the connection of dbus services");
    if let Err(e) = conn.close().await {
        tracing::warn!("failed to close the connection of dbus services: {:?}", e);
    }
    Ok(())
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

fn fatal_with_context<E, M>(e: E, err_msg: M) -> Message
where
    E: Into<Error>,
    M: Into<String>,
{
    KeyboardError::Fatal(Arc::new(e.into().context(err_msg.into()))).into()
}
