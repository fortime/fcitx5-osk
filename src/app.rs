use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};

use anyhow::{Error, Result};
use iced::{
    futures::{
        channel::mpsc::{self, UnboundedReceiver, UnboundedSender},
        Stream, StreamExt,
    },
    widget,
    window::{self, Event as IcedWindowEvent, Id},
    Color, Element, Event as IcedEvent, Size, Subscription, Task, Theme,
};
use iced_futures::event;
use pin_project::pin_project;
use tokio::time;

use crate::{
    config::ConfigManager,
    dbus::server::Fcitx5VirtualkeyboardImPanelEvent,
    state::{
        HideOpSource, ImEvent, KeyEvent, LayoutEvent, StartDbusServiceEvent, StartedEvent, State,
        ThemeEvent, WindowEvent,
    },
    window::{WindowManager, WindowSettings},
};

pub mod wayland;
pub mod x11;

#[derive(Clone, Debug)]
pub enum Message {
    Nothing,
    Started(StartedEvent),
    NewSubscription(UnboundedSender<Message>),
    StartDbusService(StartDbusServiceEvent),
    Error(KeyboardError),
    AfterError,
    ImEvent(ImEvent),
    LayoutEvent(LayoutEvent),
    KeyEvent(KeyEvent),
    WindowEvent(WindowEvent),
    ThemeEvent(ThemeEvent),
    Fcitx5VirtualkeyboardImPanelEvent(Fcitx5VirtualkeyboardImPanelEvent),
}

trait MapTask<T> {
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

impl<'a> ErrorDialogContent for &'a str {
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
}

impl<WM> Keyboard<WM>
where
    WM: Default,
{
    pub fn new(config_manager: ConfigManager) -> Result<Self> {
        Ok(Self {
            state: State::new(config_manager)?,
            error: None,
        })
    }
}

impl<WM> Keyboard<WM> {
    pub fn start(&mut self) -> Task<Message> {
        self.state.start()
    }

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
        .max_width(self.state.layout().size().width)
        .style(widget::container::rounded_box)
        .into()
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
{
    pub fn view(&self, _window_id: Id) -> Element<WM::Message> {
        let base = self.state.to_element().into();
        let res = if let Some(e) = &self.error {
            modal(base, self.error_dialog(e), Message::AfterError)
        } else if !self.state.window().wm_inited() {
            modal(
                base,
                self.error_dialog("Keyboard window is Initializing!"),
                Message::AfterError,
            )
        } else {
            base
        };
        res.map(|m| m.into())
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
                        Some(WindowEvent::WmInited(id, size).into())
                    }
                    IcedEvent::Window(IcedWindowEvent::Closed) => {
                        Some(WindowEvent::Hidden(id).into())
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
        match message {
            Message::Error(e) => self.handle_error_message(e),
            Message::AfterError => {
                if let Some(KeyboardError::Fatal(_)) = self.error.take() {
                    return window::get_latest().then(|id| {
                        window::close(id.expect("failed to get id to close in fatal error"))
                    });
                }
            }
            Message::Started(event) => match event {
                StartedEvent::StartedDbusClients(services) => {
                    self.state.set_dbus_clients(services);
                    return Task::future(async {
                        // wait a moment for fcitx5 starting the backend service.
                        let _ = time::sleep(Duration::from_secs(1));
                        Message::Nothing
                    })
                    .chain(self.state.window().show())
                    // .chain(self.state.keyboard().set_keyboard_function_mode())
                    .map_task();
                }
            },
            Message::NewSubscription(tx) => {
                {
                    let tx = tx.clone();
                    tokio::spawn(async move {
                        while !tx.is_closed() {
                            if let Err(_) = tx.unbounded_send(ThemeEvent::Detect.into()) {
                                tracing::warn!(
                                    "failed to send ThemeEvent::Check message, close the task"
                                );
                                break;
                            }
                            time::sleep(Duration::from_secs(1)).await;
                        }
                    });
                }
                return self.state.keyboard_mut().start_dbus_service(tx).map_task();
            }
            Message::StartDbusService(event) => match event {
                StartDbusServiceEvent::Started(dbus_service_token, connection) => {
                    let (replaced, old) = self
                        .state
                        .keyboard_mut()
                        .set_dbus_service_connection(dbus_service_token, connection);
                    let mut task = Task::none();
                    if replaced {
                        if let Some(old) = old {
                            task = task.chain(Task::future(async move {
                                if let Err(err) = old.close().await {
                                    tracing::warn!("error in closing dbus connection: {err:?}");
                                }
                                Message::Nothing
                            }));
                        }
                        // make sure keyboard is started after dbus service is created.
                        task = task.chain(self.start());
                    }
                    return task.map_task();
                }
            },
            Message::LayoutEvent(event) => {
                self.state.layout_mut().on_event(event);
            }
            Message::KeyEvent(event) => {
                return self.state.keyboard_mut().on_event(event).map_task();
            }
            Message::WindowEvent(event) => match event {
                WindowEvent::Resize(id, scale_factor, width_p) => {
                    tracing::debug!("scale_factor: {}", scale_factor);
                    return self.state.update_width(id, width_p, scale_factor);
                }
                WindowEvent::WmInited(id, size) => {
                    if has_fraction(size.width) || has_fraction(size.height) {
                        let width_p = self.state.config().width();
                        return window::get_scale_factor(id).map(move |scale_factor| {
                            // calculate a new size without fraction
                            Message::from(WindowEvent::Resize(id, scale_factor, width_p)).into()
                        });
                    }
                    self.state.window_mut().set_wm_inited(id);
                }
                WindowEvent::HideWindow(snapshot, source) => {
                    if let Some(snapshot) = snapshot {
                        return self.state.window_mut().hide_local_checked(snapshot, source);
                    } else {
                        return self.state.window_mut().hide_local(source);
                    }
                }
                WindowEvent::Hidden(id) => {
                    return self.set_hidden(id);
                }
            },
            Message::ThemeEvent(event) => {
                self.state.on_theme_event(event);
            }
            Message::Fcitx5VirtualkeyboardImPanelEvent(event) => {
                match event {
                    Fcitx5VirtualkeyboardImPanelEvent::ShowVirtualKeyboard => {
                        return self.show();
                    }
                    Fcitx5VirtualkeyboardImPanelEvent::HideVirtualKeyboard => {
                        return self
                            .state
                            .window_mut()
                            .hide_local_with_delay(
                                Duration::from_millis(1000),
                                HideOpSource::Fcitx5,
                            )
                            .map_task();
                    }
                    Fcitx5VirtualkeyboardImPanelEvent::UpdateCandidateArea(state) => {
                        self.state.im_mut().update_candidate_area_state(state);
                    }
                    _ => {
                        // TODO
                    }
                }
            }
            Message::ImEvent(event) => {
                return self.state.on_im_event(event).map_task();
            }
            _ => {}
        };
        Task::none()
    }

    pub fn window_size(&self) -> Size {
        self.state.layout().size()
    }

    pub fn theme_multi_dummy(&self, _window_id: Id) -> Theme {
        self.theme()
    }

    pub fn theme(&self) -> Theme {
        self.state.theme().clone()
    }

    fn show(&mut self) -> Task<WM::Message> {
        let settings = WindowSettings::new(self.window_size(), self.state.config().placement());
        self.state
            .on_im_event(ImEvent::SyncImList)
            .chain(self.state.on_im_event(ImEvent::SyncCurrentIm))
            .map_task()
            .chain(self.state.window_mut().show_local(settings))
    }

    fn set_hidden(&mut self, window_id: Id) -> Task<WM::Message> {
        match self.state.window_mut().set_hidden(window_id) {
            None | Some(HideOpSource::Fcitx5) => Task::none(),
            Some(HideOpSource::External) => {
                // call fcitx5 to hide if it is caused by external user action.
                self.state.window().hide().map_task()
            }
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

fn has_fraction(f: f32) -> bool {
    f != f.trunc()
}
