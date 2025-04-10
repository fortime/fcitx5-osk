use std::{
    future::Future,
    sync::{atomic::AtomicBool, Arc},
};

use anyhow::Result;
use iced::{window::Id, Element, Subscription, Task, Theme};
use iced_layershell::{
    build_pattern::{self, MainSettings},
    settings::{LayerShellSettings, StartMode},
    to_layer_message, Appearance,
};

use crate::{config::ConfigManager, font, window::wayland::WaylandWindowManager};

use super::{Keyboard, Message};

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
    pub fn new(config_manager: ConfigManager, shutdown_flag: Arc<AtomicBool>) -> Result<Self> {
        let inner = Keyboard::new(config_manager, shutdown_flag)?;
        Ok(Self { inner })
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

pub fn start<BG, SH>(
    config_manager: ConfigManager,
    config_write_bg: BG,
    signal_handle: SH,
    shutdown_flag: Arc<AtomicBool>,
) -> Result<()>
where
    BG: Future<Output = ()> + 'static + Send + Sync,
    SH: Future<Output = ()> + 'static + Send + Sync,
{
    let default_font = if let Some(font) = config_manager.as_ref().default_font() {
        font::load(&font)
    } else {
        Default::default()
    };

    let keyboard = WaylandKeyboard::new(config_manager, shutdown_flag)?;

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
        (
            keyboard,
            Task::future(async move {
                tokio::spawn(signal_handle);
                tokio::spawn(config_write_bg);
                Message::Nothing.into()
            }),
        )
    })?;
    Ok(())
}
