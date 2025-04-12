use std::{
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    task::{Context, Poll},
    time::Duration,
};

use anyhow::{Error, Result};
use iced::{
    futures::{
        channel::mpsc::{self, UnboundedReceiver, UnboundedSender},
        Stream, StreamExt,
    },
    widget::{self, Column},
    window::{Event as IcedWindowEvent, Id},
    Color, Element, Event as IcedEvent, Subscription, Task, Theme,
};
use iced_futures::event;
use pin_project::pin_project;
use tokio::time;

use crate::{
    config::{ConfigManager, IndicatorDisplay},
    dbus::server::Fcitx5VirtualkeyboardImPanelEvent,
    state::{
        CloseOpSource, ImEvent, KeyEvent, KeyboardEvent, LayoutEvent, StartEvent, State,
        ThemeEvent, UpdateConfigEvent, WindowEvent, WindowManagerEvent,
    },
    window::{WindowAppearance, WindowManager},
};

pub mod wayland;
pub mod x11;

#[derive(Clone, Debug)]
pub enum Message {
    Nothing,
    StartEvent(StartEvent),
    NewSubscription(UnboundedSender<Message>),
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
    error: Option<KeyboardError>,
    shutdown_flag: Arc<AtomicBool>,
    shutdown_sent: bool,
}

impl<WM> Keyboard<WM>
where
    WM: Default,
{
    pub fn new(config_manager: ConfigManager, shutdown_flag: Arc<AtomicBool>) -> Result<Self> {
        Ok(Self {
            state: State::new(config_manager)?,
            error: None,
            shutdown_flag,
            shutdown_sent: false,
        })
    }
}

impl<WM> Keyboard<WM> {
    pub fn start(&mut self) -> Task<Message> {
        self.state.start()
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

    pub fn subscription(&self) -> Subscription<WM::Message> {
        Subscription::batch(vec![
            event::listen_with(|event, status, id| {
                tracing::trace!("event: {}, {:?}, {:?}", id, status, event);
                match event {
                    IcedEvent::Window(IcedWindowEvent::Opened {
                        position: _position,
                        size,
                    }) => {
                        // ignore position, position isn't supported in wayland
                        Some(WindowEvent::Opened(id, size).into())
                    }
                    IcedEvent::Window(IcedWindowEvent::Closed) => {
                        tracing::debug!("closed: {}", id);
                        Some(WindowEvent::Closed(id).into())
                    }
                    IcedEvent::Keyboard(event) => {
                        tracing::debug!("keyboard event: {:?}", event);
                        None
                    }
                    _ => None,
                }
            }),
            Subscription::run(move || {
                let (tx, rx) = mpsc::unbounded();

                MessageStream { tx: Some(tx), rx }
            }),
        ])
        .map(|m| m.into())
    }

    pub fn update(&mut self, message: Message) -> Task<WM::Message> {
        if self.shutdown_flag.load(Ordering::Relaxed) {
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
            Message::StartEvent(event) => match event {
                StartEvent::Start => task = task.chain(self.start().map_task()),
                StartEvent::StartedDbusClients(services) => {
                    self.state.set_dbus_clients(services);
                    task = task.chain(self.state.window_manager_mut().open_indicator());
                }
            },
            Message::NewSubscription(tx) => {
                {
                    let tx = tx.clone();
                    tokio::spawn(async move {
                        while !tx.is_closed() {
                            if tx.unbounded_send(ThemeEvent::Detect.into()).is_err() {
                                tracing::warn!(
                                    "failed to send ThemeEvent::Check message, close the task"
                                );
                                break;
                            }
                            time::sleep(Duration::from_secs(1)).await;
                        }
                    });
                }
                task = task.chain(self.state.keyboard_mut().start_dbus_service(tx).map_task());
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

#[pin_project]
struct MessageStream {
    tx: Option<UnboundedSender<Message>>,
    rx: UnboundedReceiver<Message>,
}

impl Stream for MessageStream {
    type Item = Message;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if let Some(tx) = self.tx.take() {
            return Poll::Ready(Some(Message::NewSubscription(tx)));
        }
        self.project().rx.poll_next_unpin(cx)
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
