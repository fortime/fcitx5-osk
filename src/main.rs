use std::{env, fs, path::PathBuf, process};

use anyhow::Result;
use app::{Keyboard, Message};
use clap::Parser;
use config::{Config, ConfigManager};
use iced::Task;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};
use window::{
    wayland,
    x11::{self, X11WindowManager},
};

mod app;
mod config;
mod dbus;
mod font;
mod key_set;
mod layout;
mod state;
mod store;
mod widget;
mod window;

pub fn has_text_within_env(k: &str) -> bool {
    env::var(k).ok().filter(|v| !v.is_empty()).is_some()
}

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

fn load_external_fonts(config: &Config) -> Result<()> {
    let mut font_system = iced_graphics::text::font_system()
        .write()
        .map_err(|e| anyhow::anyhow!("unable to get font system: {:?}", e))?;
    tracing::debug!("fonts before loading: {}", font_system.raw().db_mut().len());
    for font_path in config.external_font_paths() {
        tracing::debug!("adding external font path: {:?}", font_path);
        font_system.raw().db_mut().load_font_file(font_path)?;
    }
    tracing::debug!("fonts after loaded: {}", font_system.raw().db_mut().len());
    Ok(())
}

fn run(args: Args) -> Result<()> {
    let (config_manager, config_write_bg) = ConfigManager::new(&args.config)?;

    init_log(config_manager.as_ref())?;

    load_external_fonts(config_manager.as_ref())?;

    if wayland::is_available() {
        app::wayland::start(config_manager, config_write_bg)?;
    } else if x11::is_available() {
        let keyboard = Keyboard::<X11WindowManager>::new(config_manager)?;

        iced::daemon(clap::crate_name!(), Keyboard::update, Keyboard::view)
            .theme(Keyboard::theme_multi_dummy)
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
    } else {
        anyhow::bail!("No Wayland or X11 Environment");
    }
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
