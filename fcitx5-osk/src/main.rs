use std::{
    env,
    path::{Path, PathBuf},
    process, thread,
    time::Duration,
};

use anyhow::Result;
use app::Message;
use clap::Parser;
use config::{Config, ConfigManager};
use iced::Task;
use window::{wayland, x11};

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

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// The path of config file.
    #[arg(short, long, value_name = "PATH")]
    config: Option<PathBuf>,

    /// Force the program running on wayland.
    #[arg(long, default_missing_value = "true")]
    force_wayland: bool,

    /// Force the program running on x11.
    #[arg(long, default_missing_value = "true")]
    force_x11: bool,

    /// Waiting DISPLAY, WAYLAND_DISPLAY, WAYLAND_SOCKET from dbus.
    #[arg(long, default_missing_value = "true")]
    wait_for_socket: bool,
}

pub fn has_text_within_env(k: &str) -> bool {
    env::var(k).ok().filter(|v| !v.is_empty()).is_some()
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
    let config_path = if let Ok(path) = env::var("FCITX5_OSK_CONFIG") {
        Path::new(&path).to_path_buf()
    } else if let Some(path) = &args.config {
        path.clone()
    } else if let Ok(home_path) = env::var("HOME") {
        let mut buf = PathBuf::new();
        buf.push(home_path);
        buf.push(".config/fcitx5-osk/config.toml");
        buf
    } else {
        anyhow::bail!("can't get the path of config file, specify it by -c or FCITX5_OSK_CONFIG");
    };
    let (config_manager, config_write_bg) = ConfigManager::new(&config_path)?;

    let _log_guard = fcitx5_osk_common::log::init_log(
        config_manager.as_ref().log_directives(),
        config_manager.as_ref().log_timestamp().unwrap_or(false),
    )?;

    load_external_fonts(config_manager.as_ref())?;

    let (mut shutdown_flag, signal_handle) = fcitx5_osk_common::signal::shutdown_flag();

    // this task will be run before x11/wayland connection is created.
    let init_task = Task::future(async move {
        tokio::spawn(signal_handle);
        tokio::spawn(config_write_bg);

        Message::Nothing
    });

    let handle = thread::spawn({
        let shutdown_flag = shutdown_flag.clone();
        move || {
            let res = if args.force_wayland || (!args.force_x11 && wayland::is_available()) {
                app::wayland::start(
                    config_manager,
                    init_task,
                    args.wait_for_socket,
                    shutdown_flag.clone(),
                )
            } else if args.force_x11 || x11::is_available() {
                if args.force_x11 {
                    // unset wayland env, otherwise, winit will use wayland to open windows.
                    unsafe {
                        // Safety, currently there is no other thread running, except console_subscriber.
                        wayland::set_env(None, None);
                    }
                }
                app::x11::start(
                    config_manager,
                    init_task,
                    args.wait_for_socket,
                    shutdown_flag.clone(),
                )
            } else {
                Err(anyhow::anyhow!("No Wayland or X11 Environment"))
            };
            // make sure shutdown
            shutdown_flag.shutdown();
            res
        }
    });

    if !handle.is_finished() {
        shutdown_flag.wait_for_shutdown_blocking();
    }

    let mut count = 0;
    while !handle.is_finished() {
        if count >= 10 {
            anyhow::bail!("timeout of waiting the keyboard to quit");
        }
        thread::sleep(Duration::from_millis(300));
        count += 1;
    }
    match handle.join() {
        Ok(res) => res,
        Err(e) => anyhow::bail!("join error: {e:?}"),
    }
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
