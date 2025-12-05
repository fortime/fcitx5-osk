use anyhow::Result;
use fcitx5_osk_common::signal::ShutdownFlag;
use iced::{futures::stream, window::Id, Element, Subscription, Task, Theme};
use x11rb::rust_connection::RustConnection;

pub use crate::app::x11::output::{OutputContext, OutputGeometry};
use crate::{
    app::{self, Keyboard, Message},
    config::ConfigManager,
    window::x11::X11WindowManager,
};

use super::AsyncAppState;

mod output;

pub struct X11Keyboard {
    output_context: OutputContext,
    shutdown_flag: ShutdownFlag,
    inner: Keyboard<X11WindowManager>,
}

impl X11Keyboard {
    pub fn new(
        config_manager: ConfigManager,
        output_context: OutputContext,
        wait_for_socket: bool,
        modifier_workaround: bool,
        shutdown_flag: ShutdownFlag,
    ) -> (Self, Task<Message>) {
        let x11_window_manager = X11WindowManager::new(
            xcb_connection,
            output_context.clone(),
            config_manager.as_ref().preferred_output_name().cloned(),
        );
        let (async_state, config_manager) = super::run_async({
            let shutdown_flag = shutdown_flag.clone();
            async move {
                let res = AsyncAppState::new(
                    config_manager.as_ref(),
                    wait_for_socket,
                    modifier_workaround,
                    shutdown_flag,
                )
                .await;
                res.map(|r| (r, config_manager))
            }
        })
        .expect("Unable to create `AsyncAppState`");
        let (inner, task) = Keyboard::new(
            async_state,
            config_manager,
            x11_window_manager,
            wait_for_socket,
            shutdown_flag.clone(),
        );
        (
            Self {
                output_context,
                shutdown_flag,
                inner,
            },
            task,
        )
    }
}

impl X11Keyboard {
    pub fn view(&self, id: Id) -> Element<Message> {
        self.inner.view(id)
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let mut subscriptions = vec![
            self.inner.subscription(),
            self.output_context.subscription(),
        ];

        // These messages only work in the first call
        let mut once_messages = vec![];
        //  connection environment variables will be set in the call of `self.inner.subscription()`.
        if let Err(e) = self.output_context.listen() {
            once_messages.push(app::error_with_context(
                e,
                "Unable to listen to the changes of x11 output",
            ));
        }
        subscriptions.push(Subscription::run_with_id(
            "external::wayland_once",
            stream::iter(once_messages),
        ));

        Subscription::batch(subscriptions)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        if self.shutdown_flag.get() {
            self.output_context.close();
        }

        self.inner.update(message)
    }

    pub fn theme(&self, id: Id) -> Theme {
        self.inner.theme(id)
    }
}

pub fn start(
    config_manager: ConfigManager,
    init_task: Task<Message>,
    wait_for_socket: bool,
    modifier_workaround: bool,
    shutdown_flag: ShutdownFlag,
) -> Result<()> {
    // each eventloop should has its own connection.
    let output_context = OutputContext::new(xcb_connection)?;

    iced::daemon(clap::crate_name!(), X11Keyboard::update, X11Keyboard::view)
        .theme(X11Keyboard::theme)
        .subscription(X11Keyboard::subscription)
        .run_with(move || {
            let (keyboard, task) = X11Keyboard::new(
                config_manager,
                output_context,
                wait_for_socket,
                modifier_workaround,
                shutdown_flag,
            );
            (keyboard, init_task.chain(task))
        })?;
    Ok(())
}

fn xcb_connection() -> Result<(RustConnection, usize)> {
    Ok(x11rb::connect(None)?)
}
