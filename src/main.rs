use std::{path::PathBuf, process};

use anyhow::Result;
use clap::Parser;
use config::{Config, ConfigManager};
use figment::{
    providers::{Format, Toml},
    Figment,
};
use iced::{window::Position, Size, Task, Theme};
use app::{Keyboard, Message};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

mod app;
mod config;
mod dbus;
mod key_set;
mod layout;
mod state;
mod store;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// The path of config file.
    #[arg(
        short,
        long,
        value_name = "PATH",
        default_value = "~/.config/fcitx5-osk/config.toml"
    )]
    config: PathBuf,
}

fn init_log(config: &Config) -> Result<()> {
    let subscriber = tracing_subscriber::registry().with(EnvFilter::from_default_env());
    if config.log_timestamp().unwrap_or(true) {
        subscriber.with(fmt::layer()).try_init()?;
    } else {
        subscriber.with(fmt::layer().without_time()).try_init()?;
    }
    Ok(())
}

fn run(args: Args) -> Result<()> {
    let (config_manager, config_write_bg) = ConfigManager::new(&args.config)?;

    init_log(config_manager.as_ref())?;

    let keyboard = Keyboard::new(config_manager)?;

    iced::daemon(clap::crate_name!(), Keyboard::update, Keyboard::view)
        .theme(Keyboard::theme)
        .subscription(Keyboard::subscription)
        .run_with(move || {
            (
                keyboard,
                // calculate size
                Task::future(async move {
                    tokio::spawn(config_write_bg);
                    Message::Nothing
                }),
            )
        })?;
    Ok(())
}

/// on:
/// 1. on when it is in tablet mode.
/// 2. manually with tray icon.
///
/// show & hide:
/// show when user focus on a input box and hide after the user left the input box.
pub fn main() {
    let args = Args::parse();
    if let Err(e) = run(args) {
        eprintln!("run command failed: {e}");
        process::exit(1);
    }
}
