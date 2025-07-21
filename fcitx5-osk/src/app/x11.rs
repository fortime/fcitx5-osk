use anyhow::Result;
use fcitx5_osk_common::{dbus::client::Fcitx5OskServices, signal::ShutdownFlag};
use iced::{futures::stream, window::Id, Element, Subscription, Task, Theme};
use x11rb::rust_connection::RustConnection;

pub use crate::app::x11::output::{OutputContext, OutputGeometry};
use crate::{
    app::{self, Keyboard, Message},
    config::ConfigManager,
    dbus::client::Fcitx5Services,
    window::x11::X11WindowManager,
};

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
        fcitx5_services: Fcitx5Services,
        fcitx5_osk_services: Fcitx5OskServices,
        wait_for_socket: bool,
        shutdown_flag: ShutdownFlag,
    ) -> Result<(Self, Task<Message>)> {
        let wm = X11WindowManager::new(
            xcb_connection,
            output_context.clone(),
            config_manager.as_ref().preferred_output_name().cloned(),
        );
        let (inner, task) = Keyboard::new(
            config_manager,
            wm,
            fcitx5_services,
            fcitx5_osk_services,
            wait_for_socket,
            shutdown_flag.clone(),
        )?;
        Ok((
            Self {
                inner,
                output_context,
                shutdown_flag,
            },
            task,
        ))
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
    let modifier_workaround_keycodes = config_manager
        .as_ref()
        .modifier_workaround_keycodes()
        .clone();

    // each eventloop should has its own connection.
    let output_context = OutputContext::new(xcb_connection)?;

    iced::daemon(clap::crate_name!(), X11Keyboard::update, X11Keyboard::view)
        .theme(X11Keyboard::theme)
        .subscription(X11Keyboard::subscription)
        .run_with(move || {
            let fcitx5_services = super::run_async(Fcitx5Services::new(
                modifier_workaround,
                modifier_workaround_keycodes,
            ))
            .expect("unable to create a fcitx5 service clients");
            let fcitx5_osk_services = super::run_async(Fcitx5OskServices::new())
                .expect("unable to create a fcitx5 osk service clients");
            let (keyboard, task) = X11Keyboard::new(
                config_manager,
                output_context,
                fcitx5_services,
                fcitx5_osk_services,
                wait_for_socket,
                shutdown_flag,
            )
            .expect("unable to create a X11Keyboard");
            (keyboard, init_task.chain(task))
        })?;
    Ok(())
}

fn xcb_connection() -> Result<(RustConnection, usize)> {
    Ok(x11rb::connect(None)?)
}
