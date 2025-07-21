use std::{path::PathBuf, process};

use anyhow::Result;
use clap::Parser;
use dbus::Fcitx5OskKeyHelperControllerService;
use zbus::Connection;

use crate::{config::Config, keyboard::Keyboard};

mod config;
mod dbus;
mod keyboard;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// The path of config file.
    #[arg(
        short,
        long,
        value_name = "PATH",
        default_value = "/etc/fcitx5-osk-key-helper/config.toml"
    )]
    config: PathBuf,
}

async fn run(args: Args) -> Result<()> {
    let config = Config::new(&args.config)?;

    let _log_guard = fcitx5_osk_common::log::init_log(
        config.log_directives(),
        config.log_timestamp().unwrap_or(false),
    )?;

    let keyboard = Keyboard::new(config.keycodes())?;

    let conn = Connection::system().await?;
    Fcitx5OskKeyHelperControllerService::new(keyboard)
        .start(&conn)
        .await?;

    let (_, signal_handle) = fcitx5_osk_common::signal::shutdown_flag();
    signal_handle.await;
    Ok(())
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    if let Err(e) = run(args).await {
        eprintln!("run command failed: {e:?}");
        process::exit(1);
    }
}
