use std::future::Future;

use anyhow::Result;
use iced::{
    futures::channel::mpsc::UnboundedSender, window::Id, Element, Subscription, Task, Theme,
};
use iced_layershell::{
    build_pattern::{self, MainSettings},
    settings::{LayerShellSettings, StartMode},
    to_layer_message,
};

use crate::{config::ConfigManager, state::WindowEvent, window::wayland::WaylandWindowManager};

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
    // there is no way to run async function in remove_id, so we send a Hidden through
    // subscription.
    tx: Option<UnboundedSender<Message>>,
}

impl WaylandKeyboard {
    pub fn new(config_manager: ConfigManager) -> Result<Self> {
        let inner = Keyboard::new(config_manager)?;
        Ok(Self { inner, tx: None })
    }
}

impl WaylandKeyboard {
    pub fn view(&self, window_id: Id) -> Element<WaylandMessage> {
        self.inner.view(window_id)
    }

    pub fn subscription(&self) -> Subscription<WaylandMessage> {
        self.inner.subscription()
    }

    pub fn update(&mut self, message: WaylandMessage) -> Task<WaylandMessage> {
        match message {
            WaylandMessage::Inner(message) => {
                if let Message::NewSubscription(tx) = &message {
                    self.tx = Some(tx.clone());
                }
                self.inner.update(message)
            }
            _ => unreachable!("layershell message should be handled before calling this method"),
        }
    }

    pub fn theme(&self) -> Theme {
        self.inner.theme()
    }

    pub fn remove_id(&mut self, window_id: Id) {
        if let Some(tx) = &self.tx {
            if let Err(_) = tx.unbounded_send(WindowEvent::Hidden(window_id).into()) {
                tracing::error!("unable to send window[{}] hidden event", window_id);
            }
        } else {
            tracing::error!(
                "window[{}] is closed when there is no subscription",
                window_id
            );
        }
    }
}

pub fn start<BG>(config_manager: ConfigManager, config_write_bg: BG) -> Result<()>
where
    BG: Future<Output = ()> + 'static + Send + Sync,
{
    let keyboard = WaylandKeyboard::new(config_manager)?;

    build_pattern::daemon(
        clap::crate_name!(),
        WaylandKeyboard::update,
        WaylandKeyboard::view,
        WaylandKeyboard::remove_id,
    )
    .theme(WaylandKeyboard::theme)
    .subscription(WaylandKeyboard::subscription)
    .settings(MainSettings {
        layer_settings: LayerShellSettings {
            start_mode: StartMode::Background,
            ..Default::default()
        },
        ..Default::default()
    })
    .run_with(move || {
        (
            keyboard,
            Task::future(async move {
                tokio::spawn(config_write_bg);
                Message::Nothing.into()
            }),
        )
    })?;
    Ok(())
}
