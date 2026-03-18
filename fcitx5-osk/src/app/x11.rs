use std::cell::RefCell;

use anyhow::Result;
use fcitx5_osk_common::signal::ShutdownFlag;
use iced::{
    futures::{stream, Stream},
    theme::Style,
    window::Id,
    Element, Subscription, Task, Theme,
};
use x11rb::rust_connection::RustConnection;

pub use crate::app::x11::output::{OutputContext, OutputGeometry};
use crate::{
    app::{self, Keyboard, Message},
    config::ConfigManager,
    misc::NamedSubscriptionData,
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
        let (async_state, config_manager) = match super::run_async({
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
        }) {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("Unable to create AsyncAppState: {e:#?}");
                panic!("Unable to create AsyncAppState");
            }
        };
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
    pub fn view(&self, id: Id) -> Element<'_, Message> {
        self.inner.view(id)
    }

    pub fn subscription(&self) -> Subscription<Message> {
        fn output_context_listen(
            data: &NamedSubscriptionData<OutputContext>,
        ) -> impl Stream<Item = Message> {
            // These messages only work in the first call
            let mut once_messages = vec![];
            // Wayland connection environment variables will be set in the call of `self.inner.subscription()`.
            if let Err(e) = data.data().listen() {
                once_messages.push(app::error_with_context(
                    e,
                    "Unable to listen to the changes of wayland output",
                ));
            }
            stream::iter(once_messages)
        }

        let mut subscriptions = vec![
            self.inner.subscription(),
            self.output_context.subscription(),
        ];
        subscriptions.push(Subscription::run_with(
            NamedSubscriptionData::new("external::x11_once", self.output_context.clone()),
            output_context_listen,
        ));

        Subscription::batch(subscriptions)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        if self.shutdown_flag.get() {
            self.output_context.close();
        }

        self.inner.update(message)
    }

    pub fn style(&self, theme: &Theme) -> Style {
        self.inner.style(theme)
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

    let boot = RefCell::new(Some(move || {
        let (keyboard, task) = X11Keyboard::new(
            config_manager,
            output_context,
            wait_for_socket,
            modifier_workaround,
            shutdown_flag,
        );
        (keyboard, init_task.chain(task))
    }));

    iced::daemon(
        move || {
            (boot
                .borrow_mut()
                .take()
                .expect("boot can't be called twice"))()
        },
        X11Keyboard::update,
        X11Keyboard::view,
    )
    .title(clap::crate_name!())
    .theme(X11Keyboard::theme)
    .style(X11Keyboard::style)
    .subscription(X11Keyboard::subscription)
    .run()?;
    Ok(())
}

fn xcb_connection() -> Result<(RustConnection, usize)> {
    Ok(x11rb::connect(None)?)
}
