use std::{collections::HashMap, future::Future, rc::Rc, sync::Arc};

use anyhow::{Context, Error, Result};
use dark_light::Mode;
use iced::{
    alignment::Horizontal,
    widget::{Button, Column, Text},
    window::{self, Id},
    Size, Subscription, Task, Theme,
};
use zbus::{Connection, Result as ZbusResult};

use state::{KeyboardState, LayoutState, State};

use crate::{
    config::ConfigManager,
    dbus::client::{Fcitx5VirtualKeyboardBackendServiceProxy, Fcitx5VirtualKeyboardServiceProxy},
    layout::{KeyAreaLayout, KeyManager},
    store::Store,
};

mod state;

#[derive(Clone, Debug)]
pub enum Message {
    Nothing,
    Error(Arc<Error>),
    KeyPressed(String),
    KeyReleased(String),
    Resize(Option<(Id, u16)>),
    UpdateKeyAreaLayout(String),
    UpdateCandidateArea(Vec<String>),
}

pub struct Keyboard {
    input: String,
    config_manager: ConfigManager,
    store: Store,
    state: State,
    fcitx5_virtual_keyboard_service: Fcitx5VirtualKeyboardServiceProxy<'static>,
    fcitx5_virtual_keyboard_backend_service: Fcitx5VirtualKeyboardBackendServiceProxy<'static>,
}

impl Keyboard {
    pub async fn new(config_manager: ConfigManager) -> Result<Self> {
        let connection = Connection::session().await?;

        let config = config_manager.as_ref();
        let store = Store::new(config)?;
        let key_area_layout = store.key_area_layout("TODO");
        let state = State {
            keyboard: KeyboardState::new(&key_area_layout, &store),
            layout: LayoutState::new(config.width(), key_area_layout)?,
        };
        Ok(Self {
            input: String::new(),
            config_manager,
            store,
            state,
            fcitx5_virtual_keyboard_service: Fcitx5VirtualKeyboardServiceProxy::new(&connection)
                .await?,
            fcitx5_virtual_keyboard_backend_service: Fcitx5VirtualKeyboardBackendServiceProxy::new(
                &connection,
            )
            .await?,
        })
    }
}

impl Keyboard {
    pub fn view(&self) -> Column<Message> {
        self.state.layout.to_element(&self.state.keyboard.input(), &self.state.keyboard)
    }

    pub fn subscription(&self) -> Subscription<Message> {
        // TODO
        Subscription::none()
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::KeyPressed(s) => {
                self.state.keyboard.update_input(s);
            }
            Message::Resize(Some((id, width_p))) => {
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

fn call_fcitx5<S, M, FN, F>(service: &S, err_msg: M, f: FN) -> Task<Message>
where
    S: Clone,
    M: Into<String>,
    FN: FnOnce(S) -> F,
    F: Future<Output = ZbusResult<()>> + 'static + Send + Sync,
{
    let err_msg = err_msg.into();
    let service = service.clone();
    Task::perform(f(service), move |r| {
        if let Err(e) = r {
            Message::Error(Arc::new(Error::from(e).context(err_msg.clone())))
        } else {
            Message::Nothing
        }
    })
}
