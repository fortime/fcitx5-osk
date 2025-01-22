use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};

use anyhow::{Error, Result};
use dark_light::Mode;
use iced::{
    futures::{
        channel::mpsc::{self, UnboundedReceiver, UnboundedSender},
        Stream, StreamExt,
    },
    widget,
    window::{self, Event as IcedWindowEvent, Id, Settings},
    Color, Element, Event as IcedEvent, Size, Subscription, Task, Theme,
};
use iced_futures::event;
use pin_project::pin_project;
use xkeysym::Keysym;

use crate::{
    config::{ConfigManager, Placement},
    dbus::{client::InputMethodInfo, server::Fcitx5VirtualkeyboardImPanelEvent},
    state::{
        HideOpSource, KeyboardState, LayoutState, StartDbusServiceEvent, StartedEvent, State,
        WindowEvent, WindowStateSnapshot,
    },
    store::Store,
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
    KeyPressed(u8, String, Keysym),
    KeyReleased(u8, String, Keysym),
    Window(WindowEvent),
    UpdateKeyAreaLayout(String),
    Fcitx5VirtualkeyboardImPanel(Fcitx5VirtualkeyboardImPanelEvent),
    UpdateImList(Vec<InputMethodInfo>),
    UpdateCurrentIm(String),
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
    config_manager: ConfigManager,
    store: Store,
    state: State<WM>,
    error: Option<KeyboardError>,
}

impl<WM> Keyboard<WM>
where
    WM: Default,
{
    pub fn new(config_manager: ConfigManager) -> Result<Self> {
        let config = config_manager.as_ref();
        let store = Store::new(config)?;
        // key_area_layout will be updated when cur_im is updated.
        let key_area_layout = store.key_area_layout("");
        let state = State::new(
            KeyboardState::new(&key_area_layout, &store),
            LayoutState::new(config.width(), key_area_layout)?,
        );
        Ok(Self {
            config_manager,
            store,
            state,
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
        let base = self
            .state
            .layout()
            .to_element(
                self.state.im().candidate_area_state(),
                self.state.im().candidate_font(),
                self.state.keyboard(),
            )
            .into();
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
                    return self
                        .state
                        .im()
                        .sync_input_methods()
                        .chain(self.state.im().sync_current_input_method())
                        // TODO show after im is updated
                        .chain(self.state.window().show())
                        .map_task();
                }
            },
            Message::NewSubscription(tx) => {
                return self.state.keyboard_mut().start_dbus_service(tx).map_task();
            }
            Message::StartDbusService(event) => match event {
                StartDbusServiceEvent::Started(dbus_service_token, connection) => {
                    self.state
                        .keyboard_mut()
                        .set_dbus_service_connection(dbus_service_token, connection);
                    // make sure keyboard is started after dbus service is created.
                    return self.start().map_task();
                }
            },
            Message::KeyPressed(state_id, s, keysym) => {
                return self
                    .state
                    .keyboard_mut()
                    .press_key(state_id, &s, keysym)
                    .map_task();
            }
            Message::KeyReleased(state_id, s, keysym) => {
                return self
                    .state
                    .keyboard_mut()
                    .release_key(state_id, &s, keysym)
                    .map_task();
            }
            Message::Window(event) => match event {
                WindowEvent::Resize(id, scale_factor, width_p) => {
                    tracing::debug!("scale_factor: {}", scale_factor);
                    if self.state.layout_mut().update_width(width_p, scale_factor) {
                        if width_p != self.config_manager.as_ref().width() {
                            self.config_manager.as_mut().set_width(width_p);
                            self.config_manager.try_write();
                        }
                        let size = self.window_size();
                        if !self.state.window().wm_inited() {
                            self.state.window_mut().set_wm_inited(id)
                        }
                        return self.state.window_mut().resize(size);
                    }
                }
                WindowEvent::WmInited(id, size) => {
                    if has_fraction(size.width) || has_fraction(size.height) {
                        let width_p = self.config_manager.as_ref().width();
                        return window::get_scale_factor(id).map(move |scale_factor| {
                            // calculate a new size without fraction
                            Message::from(WindowEvent::Resize(id, scale_factor, width_p)).into()
                        });
                    }
                    self.state.window_mut().set_wm_inited(id)
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
            Message::Fcitx5VirtualkeyboardImPanel(event) => {
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
                    Fcitx5VirtualkeyboardImPanelEvent::NotifyImListChanged => {
                        return self.state.im().sync_input_methods().map_task();
                    }
                    Fcitx5VirtualkeyboardImPanelEvent::UpdateCandidateArea(state) => {
                        self.state.im_mut().set_candidate_area_state(state);
                    }
                    Fcitx5VirtualkeyboardImPanelEvent::NotifyImActivated(im) => {
                        self.state.update_cur_im(&im, &self.store);
                    }
                    Fcitx5VirtualkeyboardImPanelEvent::NotifyImDeactivated(_) => {
                        // TODO? other logic
                        self.state.im_mut().deactive();
                    }
                    _ => {
                        // TODO
                    }
                }
            }
            Message::UpdateImList(list) => {
                self.state.im_mut().update_ims(list);
            }
            Message::UpdateCurrentIm(im) => {
                self.state.update_cur_im(&im, &self.store);
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
        if let Some(t) = self.store.theme(&self.config_manager.as_ref().theme()) {
            t.clone()
        } else {
            match dark_light::detect() {
                Mode::Dark => Theme::Dark.clone(),
                Mode::Light | Mode::Default => Theme::Light.clone(),
            }
        }
    }

    fn show(&mut self) -> Task<WM::Message> {
        let settings =
            WindowSettings::new(self.window_size(), self.config_manager.as_ref().placement());
        return self.state.window_mut().show_local(settings);
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
