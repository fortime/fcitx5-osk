use std::sync::{atomic::AtomicBool, Arc};

use anyhow::Result;
use iced::{window::Id, Element, Subscription, Task, Theme};
use iced_layershell::{
    build_pattern::{self, MainSettings},
    settings::{LayerShellSettings, StartMode},
    to_layer_message, Appearance,
};

use crate::{
    app::{Keyboard, MapTask, Message},
    config::ConfigManager,
    dbus::client::Fcitx5Services,
    font,
    window::wayland::WaylandWindowManager,
};

#[to_layer_message(multi)]
#[derive(Clone, Debug)]
pub enum WaylandMessage {
    Inner(Message),
}

impl From<Message> for WaylandMessage {
    fn from(value: Message) -> Self {
        Self::Inner(value)
    }
}

struct WaylandKeyboard {
    inner: Keyboard<WaylandWindowManager>,
}

impl WaylandKeyboard {
    pub fn new(
        config_manager: ConfigManager,
        fcitx5_services: Fcitx5Services,
        shutdown_flag: Arc<AtomicBool>,
    ) -> Result<(Self, Task<Message>)> {
        let (inner, task) = Keyboard::new(config_manager, fcitx5_services, shutdown_flag)?;
        Ok((Self { inner }, task))
    }
}

impl WaylandKeyboard {
    pub fn view(&self, id: Id) -> Element<WaylandMessage> {
        self.inner.view(id)
    }

    pub fn subscription(&self) -> Subscription<WaylandMessage> {
        self.inner.subscription()
    }

    pub fn update(&mut self, message: WaylandMessage) -> Task<WaylandMessage> {
        if let WaylandMessage::Inner(message) = message {
            self.inner.update(message)
        } else {
            Message::from_nothing()
        }
    }

    pub fn appearance(&self, theme: &Theme, id: Id) -> Appearance {
        self.inner.appearance(theme, id)
    }

    pub fn theme(&self, id: Id) -> Theme {
        self.inner.theme(id)
    }

    pub fn remove_id(&mut self, _id: Id) {}
}

pub fn start(
    config_manager: ConfigManager,
    init_task: Task<Message>,
    shutdown_flag: Arc<AtomicBool>,
) -> Result<()> {
    let default_font = if let Some(font) = config_manager.as_ref().default_font() {
        font::load(font)
    } else {
        Default::default()
    };

    build_pattern::daemon(
        clap::crate_name!(),
        WaylandKeyboard::update,
        WaylandKeyboard::view,
        WaylandKeyboard::remove_id,
    )
    .style(WaylandKeyboard::appearance)
    .theme(WaylandKeyboard::theme)
    .subscription(WaylandKeyboard::subscription)
    .settings(MainSettings {
        layer_settings: LayerShellSettings {
            start_mode: StartMode::Background,
            ..Default::default()
        },
        default_font,
        ..Default::default()
    })
    .run_with(move || {
        let fcitx5_services = super::run_async(Fcitx5Services::new())
            .expect("unable to create a fcitx5 service clients");
        let (keyboard, task) = WaylandKeyboard::new(config_manager, fcitx5_services, shutdown_flag)
            .expect("unable to create a WaylandKeyboard");
        (keyboard, init_task.chain(task).map_task())
    })?;
    Ok(())
}
