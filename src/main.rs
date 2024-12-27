use std::{path::PathBuf, process};

use anyhow::Result;
use clap::Parser;
use config::{Config, ConfigManager};
use figment::{
    providers::{Format, Toml},
    Figment,
};
use iced::{window::Position, Size, Theme};
use keyboard::Keyboard;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

mod config;
mod dbus;
mod key_set;
mod keyboard;
mod layout;
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

async fn run(args: Args) -> Result<()> {
    let (config_manager, write_bg) = ConfigManager::new(&args.config)?;

    init_log(config_manager.as_ref())?;

    let s = dbus::server::VirtualkeyboardImPanelService {};
    let _conn = s.start().await?;

    let keyboard = Keyboard::new(config_manager).await?;

    iced::application("Keyboard", Keyboard::update, Keyboard::view)
        .decorations(false)
        .position(Position::Specific((10.0, 10.0).into()))
        .window_size(keyboard.window_size())
        .theme(Keyboard::theme)
        .subscription(Keyboard::subscription)
        .run_with(move || {
            (
                keyboard,
                // calculate size
                ().into(),
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
#[tokio::main]
async fn main() {
    let args = Args::parse();
    if let Err(e) = run(args).await {
        eprintln!("run command failed: {e}");
        process::exit(1);
    }
}
