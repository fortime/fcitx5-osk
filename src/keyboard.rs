use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use anyhow::{Error, Result};
use dark_light::Mode;
use iced::{
    futures::{
        channel::mpsc::{self, UnboundedReceiver, UnboundedSender},
        Stream, StreamExt,
    },
    widget::{button, center, column, container, mouse_area, opaque, stack, text, Column},
    window::{self, Id},
    Color, Element, Size, Subscription, Task, Theme,
};
use pin_project::pin_project;
use xkeysym::Keysym;

use crate::{
    config::ConfigManager,
    dbus::server::Fcitx5VirtualkeyboardImPanelState,
    keyboard::state::{KeyboardState, LayoutState, StartDbusServiceState, StartedState, State},
    store::Store,
};

mod state;

#[derive(Clone, Debug)]
pub enum Message {
    Nothing,
    Started(StartedState),
    StartDbusService(StartDbusServiceState),
    Error(KeyboardError),
    AfterError,
    KeyPressed(u8, String, Keysym),
    KeyReleased(u8, String, Keysym),
    Resize(Id, u16),
    UpdateKeyAreaLayout(String),
    UpdateCandidateArea(Vec<String>),
    Fcitx5VirtualkeyboardImPanel(Fcitx5VirtualkeyboardImPanelState),
}

#[derive(Clone, Debug)]
pub enum KeyboardError {
    Error(Arc<Error>),
    Fatal(Arc<Error>),
}

impl From<KeyboardError> for Message {
    fn from(value: KeyboardError) -> Self {
        Self::Error(value)
    }
}

impl KeyboardError {
    fn is_priority_over(&self, other: &Self) -> bool {
        match (self, other) {
            (KeyboardError::Error(_), KeyboardError::Error(_)) => true,
            (KeyboardError::Error(_), KeyboardError::Fatal(_)) => false,
            (KeyboardError::Fatal(_), KeyboardError::Error(_)) => true,
            (KeyboardError::Fatal(_), KeyboardError::Fatal(_)) => true,
        }
    }
}

pub struct Keyboard {
    config_manager: ConfigManager,
    store: Store,
    state: State,
    error: Option<KeyboardError>,
}

impl Keyboard {
    pub fn new(config_manager: ConfigManager) -> Result<Self> {
        let config = config_manager.as_ref();
        let store = Store::new(config)?;
        let key_area_layout = store.key_area_layout("TODO");
        let state = State {
            keyboard: KeyboardState::new(&key_area_layout, &store),
            layout: LayoutState::new(config.width(), key_area_layout)?,
        };
        Ok(Self {
            config_manager,
            store,
            state,
            error: None,
        })
    }
}

impl Keyboard {
    pub fn start(&mut self) -> Task<Message> {
        self.state.start()
    }

    pub fn error_dialog(&self, e: &KeyboardError) -> Element<Message> {
        let (err_msg, button_text) = match e {
            KeyboardError::Error(e) => (format!("Error: {e}"), "Close"),
            KeyboardError::Fatal(e) => (format!("Fatal error: {e}"), "Exit"),
        };
        container(
            column![
                text(err_msg),
                button(text(button_text)).on_press(Message::AfterError),
            ]
            .spacing(10)
            .padding(10),
        )
        .max_width(self.state.layout.size().width)
        .style(container::rounded_box)
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

impl Keyboard {
    pub fn view(&self) -> Element<Message> {
        let base = self
            .state
            .layout
            .to_element(&self.state.keyboard.input(), &self.state.keyboard)
            .into();
        if let Some(e) = &self.error {
            modal(base, self.error_dialog(e), Message::AfterError)
        } else {
            base
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        Subscription::run(move || {
            let (tx, rx) = mpsc::unbounded();

            MessageStream { tx: Some(tx), rx }
        })
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Error(e) => self.handle_error_message(e),
            Message::AfterError => {
                if let Some(KeyboardError::Fatal(_)) = self.error.take() {
                    return window::get_latest().then(|id| {
                        window::close(id.expect("failed to get id to close in fatal error"))
                    });
                }
            }
            Message::Started(state) => match state {
                StartedState::StartedDbusClients(s1, s2) => {
                    self.state.keyboard.set_dbus_clients(s1, s2);
                    return self.state.keyboard.show();
                }
            },
            Message::StartDbusService(state) => match state {
                StartDbusServiceState::New(tx) => {
                    return self.state.keyboard.start_dbus_service(tx);
                }
                StartDbusServiceState::Started(dbus_service_id, connection) => {
                    self.state
                        .keyboard
                        .set_dbus_service_connection(dbus_service_id, connection);
                    // make sure keyboard is started after dbus service is created.
                    return self.start();
                }
            },
            Message::KeyPressed(state_id, s, keysym) => {
                return self.state.keyboard.press_key(state_id, &s, keysym);
            }
            Message::KeyReleased(state_id, s, keysym) => {
                return self.state.keyboard.release_key(state_id, &s, keysym);
            }
            Message::Resize(id, width_p) => {
                // window::get_latest().map
                if self.state.layout.update_width(width_p) {
                    self.config_manager.as_mut().set_width(width_p);
                    self.config_manager.try_write();
                    return window::resize(id, self.window_size());
                }
            }
            _ => {}
        };
        Task::none()
    }

    pub fn window_size(&self) -> Size {
        self.state.layout.size()
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
            return Poll::Ready(Some(StartDbusServiceState::New(tx).into()));
        }
        self.project().rx.poll_next_unpin(cx)
    }
}

fn modal<'a>(
    base: Element<'a, Message>,
    content: Element<'a, Message>,
    on_blur: Message,
) -> Element<'a, Message> {
    stack![
        base,
        opaque(
            mouse_area(center(opaque(content)).style(|_theme| {
                container::Style {
                    background: Some(
                        Color {
                            a: 0.8,
                            ..Color::BLACK
                        }
                        .into(),
                    ),
                    ..container::Style::default()
                }
            }))
            .on_press(on_blur)
        )
    ]
    .into()
}
