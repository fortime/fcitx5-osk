use anyhow::Result;
use connection::WaylandConnection;
use fcitx5_osk_common::{dbus::client::Fcitx5OskServices, signal::ShutdownFlag};
use iced::{futures::stream, window::Id, Element, Subscription, Task, Theme};
use iced_layershell::{
    build_pattern::{self, MainSettings},
    settings::{LayerShellSettings, StartMode},
    to_layer_message, Appearance,
};

pub use crate::app::wayland::output::{OutputContext, OutputGeometry};
use crate::{
    app::{self, wayland::input_method::InputMethodContext, Keyboard, MapTask, Message},
    config::ConfigManager,
    dbus::client::Fcitx5Services,
    font,
    state::WindowManagerEvent,
    window::{wayland::WaylandWindowManager, WindowManagerMode},
};

mod connection;
mod input_method;
mod output;

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
    input_method_context: InputMethodContext,
    output_context: OutputContext,
    shutdown_flag: ShutdownFlag,
    inner: Keyboard<WaylandWindowManager>,
}

impl WaylandKeyboard {
    pub fn new(
        config_manager: ConfigManager,
        input_method_context: InputMethodContext,
        output_context: OutputContext,
        fcitx5_services: Fcitx5Services,
        fcitx5_osk_services: Fcitx5OskServices,
        wait_for_socket: bool,
        shutdown_flag: ShutdownFlag,
    ) -> Result<(Self, Task<Message>)> {
        let wayland_window_manager = WaylandWindowManager::new(
            output_context.clone(),
            config_manager.as_ref().preferred_output_name().cloned(),
        );
        let (inner, task) = Keyboard::new(
            config_manager,
            wayland_window_manager,
            fcitx5_services,
            fcitx5_osk_services,
            wait_for_socket,
            shutdown_flag.clone(),
        )?;
        Ok((
            Self {
                inner,
                input_method_context,
                output_context,
                shutdown_flag,
            },
            task,
        ))
    }
}

impl WaylandKeyboard {
    pub fn view(&self, id: Id) -> Element<WaylandMessage> {
        self.inner.view(id)
    }

    pub fn subscription(&self) -> Subscription<WaylandMessage> {
        let mut subscriptions = vec![
            self.inner.subscription(),
            self.input_method_context.subscription(),
            self.output_context.subscription(),
        ];

        // These messages only work in the first call
        let mut once_messages = vec![];
        // Wayland connection environment variables will be set in the call of `self.inner.subscription()`.
        if let Err(e) = self.output_context.listen() {
            once_messages.push(
                app::error_with_context(e, "Unable to listen to the changes of wayland output")
                    .into(),
            );
        }
        subscriptions.push(Subscription::run_with_id(
            "external::wayland_once",
            stream::iter(once_messages),
        ));

        Subscription::batch(subscriptions)
    }

    pub fn update(&mut self, message: WaylandMessage) -> Task<WaylandMessage> {
        if self.shutdown_flag.get() {
            self.input_method_context.close();
            self.output_context.close();
        }

        if let WaylandMessage::Inner(message) = message {
            let input_panel = matches!(
                &message,
                Message::WindowManagerEvent(WindowManagerEvent::UpdateMode(
                    WindowManagerMode::KwinLockScreen,
                ))
            );
            let mut task = self.inner.update(message);
            if input_panel {
                match self.input_method_context.fcitx5_services() {
                    Ok(fcitx5_services) => {
                        // switch to our input-method-v1 implementation.
                        task = task.chain(
                            self.inner
                                .update(Message::UpdateFcitx5Services(fcitx5_services)),
                        );
                    }
                    Err(e) => {
                        task = Task::done(
                            app::fatal_with_context(
                                e,
                                "failed to create Fcitx5Services for input_method_v1",
                            )
                            .into(),
                        );
                    }
                }
            }
            task
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
}

pub fn start(
    config_manager: ConfigManager,
    init_task: Task<Message>,
    wait_for_socket: bool,
    shutdown_flag: ShutdownFlag,
) -> Result<()> {
    let default_font = if let Some(font) = config_manager.as_ref().default_font() {
        font::load(font)
    } else {
        Default::default()
    };

    let connection = WaylandConnection::new();
    let input_method_context = InputMethodContext::new(connection.clone());
    let output_context = OutputContext::new(connection.clone());

    build_pattern::daemon(
        clap::crate_name!(),
        WaylandKeyboard::update,
        WaylandKeyboard::view,
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
        with_connection: Some(
            (move || {
                connection
                    .state()
                    .inspect_err(|e| tracing::error!("failed to get a connection: {:?}", e))
                    .map(|s| s.connection().clone())
            })
            .into(),
        ),
        ..Default::default()
    })
    .run_with(move || {
        let fcitx5_services = super::run_async(Fcitx5Services::new())
            .expect("unable to create a fcitx5 service clients");
        let fcitx5_osk_services = super::run_async(Fcitx5OskServices::new())
            .expect("unable to create a fcitx5 osk service clients");
        let (keyboard, task) = WaylandKeyboard::new(
            config_manager,
            input_method_context,
            output_context,
            fcitx5_services,
            fcitx5_osk_services,
            wait_for_socket,
            shutdown_flag,
        )
        .expect("unable to create a WaylandKeyboard");
        (keyboard, init_task.chain(task).map_task())
    })?;
    Ok(())
}
