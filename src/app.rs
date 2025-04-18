use std::{
    cell::RefCell,
    future::Future,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use anyhow::{Context, Error, Result};
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
use tokio::{sync::oneshot, time};
use zbus::Connection;

use crate::{
    config::{ConfigManager, IndicatorDisplay},
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
    window::{self, WindowAppearance, WindowManager},
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
    rx: RefCell<Option<UnboundedReceiver<Message>>>,
    error: Option<KeyboardError>,
    shutdown_flag: Arc<AtomicBool>,
    shutdown_sent: bool,
}

impl<WM> Keyboard<WM>
where
    WM: Default,
{
    pub fn new(
        config_manager: ConfigManager,
        fcitx5_services: Fcitx5Services,
        wait_for_socket: bool,
        shutdown_flag: Arc<AtomicBool>,
    ) -> Result<(Self, Task<Message>)> {
        let (tx, rx) = mpsc::unbounded();
        let state = State::new(config_manager, fcitx5_services)?;
        Ok((
            Self {
                state,
                rx: RefCell::new(Some(rx)),
                error: None,
                shutdown_flag: shutdown_flag.clone(),
                shutdown_sent: false,
            },
            Task::future(init(tx, wait_for_socket, shutdown_flag)),
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
        if self.state.window_manager().is_keyboard(id) {
            let base = self.state.to_element(id);
            let res = if let Some(e) = &self.error {
                modal(base, self.error_dialog(e), Message::AfterError)
            } else {
                base
            };
            res.map(|m| m.into())
        } else if self.state.window_manager().is_indicator(id) {
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
        if self.shutdown_flag.load(Ordering::Relaxed) {
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
                        task = task.chain(self.state.window_manager_mut().open_keyboard());
                    }
                    Fcitx5VirtualkeyboardImPanelEvent::HideVirtualKeyboard => {
                        // always set fcitx5 hidden, so we can make sure virtual keyboard mode of fcitx5 will be activated.
                        self.state.keyboard_mut().set_fcitx5_hidden();
                        if self.state.config().indicator_display() != IndicatorDisplay::AlwaysOff {
                            // if there is no indicator, we ignore hide call.
                            task = task.chain(
                                self.state
                                    .window_manager_mut()
                                    .close_keyboard(CloseOpSource::Fcitx5),
                            );
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
                    ImPanelEvent::Show => {
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
                }
            }
            Message::ImEvent(event) => {
                task = task.chain(self.state.on_im_event(event).map_task());
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
        if appearance.background_color() == Color::TRANSPARENT {
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
    wait_for_socket: bool,
    shutdown_flag: Arc<AtomicBool>,
) -> Message {
    let (socket_env_tx, socket_env_rx) = oneshot::channel();

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
                    shutdown_flag.store(true, Ordering::Relaxed);
                }
            }
        }
    });

    tokio::spawn(start_detect_theme(tx, shutdown_flag.clone()));

    if wait_for_socket {
        unsafe {
            // No Safety Guarantee, multi_thread runtime has been started.
            // It is high probability that data race of env won't be happened.
            match socket_env_rx.await {
                Ok(SocketEnv::WaylandSocket(s)) => window::wayland::set_env(Some(&s), None),
                Ok(SocketEnv::WaylandDisplay(s)) => window::wayland::set_env(None, Some(&s)),
                Ok(SocketEnv::X11Display(s)) => window::x11::set_env(Some(&s)),
                Err(_) => {
                    tracing::error!("failed to receive socket env");
                    shutdown_flag.store(true, Ordering::Relaxed);
                }
            }
        }
    }

    Message::from(WindowManagerEvent::OpenIndicator)
}

async fn start_detect_theme(tx: UnboundedSender<Message>, shutdown_flag: Arc<AtomicBool>) {
    while !shutdown_flag.load(Ordering::Relaxed) {
        if tx.unbounded_send(ThemeEvent::Detect.into()).is_err() {
            tracing::warn!("failed to send ThemeEvent::Check message, close the task");
            break;
        }
        time::sleep(Duration::from_millis(500)).await;
    }
}

async fn start_dbus_services(
    tx: UnboundedSender<Message>,
    socket_env_tx: oneshot::Sender<SocketEnv>,
    shutdown_flag: Arc<AtomicBool>,
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
    while !shutdown_flag.load(Ordering::Relaxed) {
        time::sleep(Duration::from_millis(500)).await;
    }
    // trigger shutdown check
    if tx.unbounded_send(Message::Nothing).is_err() {
        tracing::error!("failed to send message, shutdown may be stopped");
    }
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
