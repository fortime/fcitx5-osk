use std::{
    env,
    future::Future,
    path::PathBuf,
    pin::Pin,
    process,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    task::{Context, Poll},
};

use anyhow::Result;
use app::Message;
use clap::Parser;
use config::{Config, ConfigManager};
use iced::Task;
use tokio::signal::unix::{signal, Signal, SignalKind};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};
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
    #[arg(
        short,
        long,
        value_name = "PATH",
    )]
    config: PathBuf,

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

pub struct Signals(Vec<(SignalKind, Signal)>);

impl Signals {
    /// Should be called inside tokio runtime
    pub async fn try_new(signal_kinds: Vec<SignalKind>) -> Result<Self> {
        let mut signals = Vec::with_capacity(signal_kinds.len());
        for kind in signal_kinds {
            signals.push((kind, signal(kind)?));
        }
        Ok(Self(signals))
    }
}

impl Future for Signals {
    type Output = SignalKind;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        for (kind, signal) in self.0.iter_mut() {
            match signal.poll_recv(cx) {
                Poll::Pending => continue,
                Poll::Ready(_) => return Poll::Ready(*kind),
            }
        }
        Poll::Pending
    }
}

pub async fn try_default_signals() -> Result<Signals> {
    Signals::try_new(vec![
        SignalKind::interrupt(),
        SignalKind::terminate(),
        SignalKind::hangup(),
        SignalKind::pipe(),
        SignalKind::quit(),
    ])
    .await
}

fn init_log(config: &Config) -> Result<()> {
    let subscriber = tracing_subscriber::registry().with(EnvFilter::from_default_env());
    #[cfg(debug_assertions)]
    let subscriber = subscriber.with(
        console_subscriber::ConsoleLayer::builder()
            .with_default_env()
            .spawn(),
    );

    if config.log_timestamp().unwrap_or(true) {
        subscriber.with(fmt::layer()).try_init()?;
    } else {
        subscriber.with(fmt::layer().without_time()).try_init()?;
    }
    Ok(())
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
    let (config_manager, config_write_bg) = ConfigManager::new(&args.config)?;

    init_log(config_manager.as_ref())?;

    load_external_fonts(config_manager.as_ref())?;

    let shutdown_flag = Arc::new(AtomicBool::new(false));
    let signal_handle = {
        let shutdown_flag = shutdown_flag.clone();
        async move {
            match try_default_signals().await {
                Ok(signals) => {
                    let res = signals.await;
                    shutdown_flag.store(true, Ordering::Relaxed);
                    tracing::info!("stopping by signal: {:?}", res);
                }
                Err(e) => {
                    shutdown_flag.store(true, Ordering::Relaxed);
                    tracing::error!("failed to create signal handle: {:?}", e);
                }
            }
        }
    };

    // this task will be run before x11/wayland connection is created.
    let init_task = Task::future(async move {
        tokio::spawn(signal_handle);
        tokio::spawn(config_write_bg);

        Message::Nothing
    });

    if args.force_wayland || (!args.force_x11 && wayland::is_available()) {
        app::wayland::start(
            config_manager,
            init_task,
            args.wait_for_socket,
            shutdown_flag,
        )?;
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
            shutdown_flag,
        )?;
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
