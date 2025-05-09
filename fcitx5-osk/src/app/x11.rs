use anyhow::Result;
use fcitx5_osk_common::{dbus::client::Fcitx5OskServices, signal::ShutdownFlag};
use iced::Task;

use crate::{
    app::{Keyboard, Message},
    config::ConfigManager,
    dbus::client::Fcitx5Services,
    window::x11::X11WindowManager,
};

pub fn start(
    config_manager: ConfigManager,
    init_task: Task<Message>,
    wait_for_socket: bool,
    shutdown_flag: ShutdownFlag,
) -> Result<()> {
    iced::daemon(clap::crate_name!(), Keyboard::update, Keyboard::view)
        .theme(Keyboard::theme)
        .subscription(Keyboard::subscription)
        .run_with(move || {
            let fcitx5_services = super::run_async(Fcitx5Services::new())
                .expect("unable to create a fcitx5 service clients");
            let fcitx5_osk_services = super::run_async(Fcitx5OskServices::new())
                .expect("unable to create a fcitx5 osk service clients");
            let (keyboard, task) = Keyboard::<X11WindowManager>::new(
                config_manager,
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
