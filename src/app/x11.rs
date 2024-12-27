use std::{
    future::Future,
    sync::{atomic::AtomicBool, Arc},
};

use anyhow::Result;
use iced::Task;

use crate::{
    app::{Keyboard, Message},
    config::ConfigManager,
    window::x11::X11WindowManager,
};

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
    let keyboard = Keyboard::<X11WindowManager>::new(config_manager, shutdown_flag)?;

    iced::daemon(clap::crate_name!(), Keyboard::update, Keyboard::view)
        .theme(Keyboard::theme)
        .subscription(Keyboard::subscription)
        .theme(Keyboard::theme)
        .run_with(move || {
            (
                keyboard,
                // calculate size
                Task::future(async move {
                    tokio::spawn(signal_handle);
                    tokio::spawn(config_write_bg);
                    Message::Nothing
                }),
            )
        })?;
    Ok(())
}
