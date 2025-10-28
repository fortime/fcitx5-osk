use std::{
    collections::VecDeque,
    env,
    os::fd::{FromRawFd, OwnedFd},
    process,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use anyhow::Result;
use clap::Parser;
use dbus::client::KwinServices;
use fcitx5_osk_common::{
    dbus::{self as common_dbus, client::Fcitx5OskServices, entity::WindowManagerMode},
    signal::ShutdownFlag,
};
use futures_util::StreamExt;
use tokio::process::Command;
use zbus::{
    fdo::{DBusProxy, Result as ZbusFdoResult},
    names::{UniqueName, WellKnownName},
    Connection,
};

use crate::dbus::client::{Fcitx5ControllerServiceProxy, FdoServices};

mod dbus;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Force the program running on wayland.
    #[arg(long, default_missing_value = "true")]
    log_timestamp: bool,

    /// Use reopen api.
    #[arg(long, default_missing_value = "true")]
    fcitx5_reopen: bool,

    /// Start as worker.
    #[arg(long, default_missing_value = "true")]
    worker: bool,

    /// Start for sddm.
    #[arg(long, default_missing_value = "true")]
    sddm: bool,
}

async fn owner(
    proxy: &DBusProxy<'static>,
    service_name: WellKnownName<'static>,
) -> ZbusFdoResult<Option<UniqueName<'static>>> {
    match proxy.get_name_owner(service_name.into()).await {
        Ok(owner) => Ok(Some(owner.into())),
        Err(zbus::fdo::Error::NameHasNoOwner(_)) => Ok(None),
        Err(e) => Err(e),
    }
}

async fn watch_fcitx5_osk(
    connection: &Connection,
    mut socket: Option<OwnedFd>,
    display: String,
    shutdown_flag: ShutdownFlag,
) -> Result<()> {
    let service_name = WellKnownName::try_from(common_dbus::SERVICE_NAME)?;
    let dbus_proxy = DBusProxy::new(connection).await?;
    let mut stream = dbus_proxy
        .receive_name_owner_changed_with_args(&[(0, common_dbus::SERVICE_NAME)])
        .await?;

    let mut owner = owner(&dbus_proxy, service_name.clone()).await?;
    let mut started = owner.is_some();
    let has_socket = socket.is_some();
    loop {
        match owner {
            None => {
                if started || shutdown_flag.get() {
                    // exited and using WAYLAND_SOCKET, socket can't be reused, so launcher should
                    // be restarted.
                    break;
                } else {
                    // start a new one
                    let res = dbus_proxy
                        .start_service_by_name(service_name.clone(), 0)
                        .await?;
                    tracing::debug!("start dbus service[{:?}]: {}", service_name, res);
                    started = true;
                }
            }
            Some(addr) => {
                let proxy =
                    common_dbus::client::Fcitx5OskControllerServiceProxy::builder(connection)
                        .destination(addr)?
                        .build()
                        .await?;
                if let Some(socket) = socket.take() {
                    // change mode to WaylandInputPanel, if it is using WAYLAND_SOCKET
                    proxy.change_mode(WindowManagerMode::KwinLockScreen).await?;
                    proxy
                        .open_socket(common_dbus::entity::Socket::Wayland(socket.into()))
                        .await?;
                } else if has_socket {
                    // socket has been used, shutdown to get a new one.
                    tracing::warn!("socket is sent, restart to get a new one");
                    return Ok(());
                } else {
                    proxy
                        .open_display(common_dbus::entity::Display::Wayland(display.clone()))
                        .await?;
                }
            }
        }
        if let Some(changed) = stream.next().await {
            let mut changed_args = changed.args()?;
            tracing::debug!(
                "the owner of dbus service[{:?}] is changed: {:?}",
                service_name,
                changed_args
            );
            owner = changed_args.new_owner.take().map(|o| o.into_owned());
        } else {
            break;
        }
    }
    Ok(())
}

async fn watch_fcitx5(
    connection: &Connection,
    mut socket: Option<OwnedFd>,
    display: String,
    reopen: bool,
    shutdown_flag: ShutdownFlag,
) -> Result<()> {
    const FCITX5_SERVICE_NAME: &str = "org.fcitx.Fcitx5";
    let service_name = WellKnownName::try_from(FCITX5_SERVICE_NAME)?;
    let dbus_proxy = DBusProxy::new(connection).await?;
    let mut stream = dbus_proxy
        .receive_name_owner_changed_with_args(&[(0, FCITX5_SERVICE_NAME)])
        .await?;

    let mut owner = owner(&dbus_proxy, service_name.clone()).await?;
    let mut started = owner.is_some();
    let has_socket = socket.is_some();
    loop {
        match owner {
            None => {
                if started {
                    // fcitx5 exits
                    return Ok(());
                } else if !shutdown_flag.get() {
                    // Start service
                    let res = dbus_proxy
                        .start_service_by_name(service_name.clone(), 0)
                        .await?;
                    tracing::debug!("start dbus service[{:?}]: {}", service_name, res);
                    started = true;
                }
            }
            Some(addr) => {
                let proxy = Fcitx5ControllerServiceProxy::builder(connection)
                    .destination(addr)?
                    .build()
                    .await?;
                if let Some(socket) = socket.take() {
                    if reopen {
                        proxy
                            .reopen_wayland_connection_socket(&display, socket.into())
                            .await?;
                    } else {
                        proxy.open_wayland_connection_socket(socket.into()).await?;
                    }
                } else if has_socket {
                    // socket has been used, shutdown to get a new one.
                    tracing::warn!("socket is sent, restart to get a new one");
                    return Ok(());
                }
            }
        }
        if let Some(changed) = stream.next().await {
            let mut changed_args = changed.args()?;
            tracing::debug!(
                "the owner of dbus service[{:?}] is changed: {:?}",
                service_name,
                changed_args
            );
            owner = changed_args.new_owner.take().map(|o| o.into_owned());
        } else {
            break;
        }
    }
    Ok(())
}

async fn watch_kwin_virtual_keyboard(
    fcitx5_osk_services: &Fcitx5OskServices,
    kwin_services: &KwinServices,
    in_lockscreen: bool,
    tablet_mode_check: bool,
) -> Result<()> {
    let expected_mode = if in_lockscreen {
        WindowManagerMode::KwinLockScreen
    } else {
        WindowManagerMode::Normal
    };
    loop {
        let mode = fcitx5_osk_services.controller().mode().await;
        if (in_lockscreen && mode == Ok(expected_mode))
            || (!in_lockscreen && mode == Ok(expected_mode))
        {
            break;
        } else {
            // make sure mode is set
            fcitx5_osk_services
                .controller()
                .change_mode(expected_mode)
                .await?;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    // in lockscreen, if the keyboard is set hidden, then trigger a hide-and-show of the keyboard to reset the input panel surface.
    if in_lockscreen {
        const TIMEOUT: Duration = Duration::from_secs(4);
        const MAX_AMOUNT: usize = 5;
        let mut visible_changes = VecDeque::with_capacity(MAX_AMOUNT);
        let mut last_visible = true;
        let mut stream = kwin_services
            .virtual_keyboard()
            .receive_visible_changed()
            .await?;
        while stream.next().await.is_some() {
            let visible = kwin_services.virtual_keyboard().visible().await?;
            tracing::debug!("kwin virtual keyboard visible: {visible}");
            let now = Instant::now();
            visible_changes.push_back(now);
            while visible_changes.len() >= MAX_AMOUNT {
                // In some cases, keyboard will be lost for ever in lockscreen. in these
                // cases, we should replace the input panel surface to let the keyboard
                // show again. If the user clicks the virtual keyboard button more than n
                // times during m seconds, we will replace the surface.
                if let Some(oldest) = visible_changes.pop_front() {
                    if now.saturating_duration_since(oldest) < TIMEOUT {
                        // show and hide will cause visible changed signal, so we remove
                        // previous one in case it enters a infinite loop.
                        visible_changes.clear();
                        fcitx5_osk_services.controller().hide().await?;
                        fcitx5_osk_services.controller().show().await?;
                        continue;
                    }
                }
            }
            if visible != last_visible {
                // only run if visible is different from the last one.
                fcitx5_osk_services
                    .controller()
                    .change_visible(visible)
                    .await?;
            }
            last_visible = visible;
        }
    } else {
        let mut stream = kwin_services
            .virtual_keyboard()
            .receive_active_changed()
            .await?;
        while stream.next().await.is_some() {
            let active = kwin_services.virtual_keyboard().active().await?;
            let tablet_mode = if tablet_mode_check {
                kwin_services.tablet_mode().tablet_mode().await?
            } else {
                true
            };
            // check tablet mode, show only if it is in tablet mode.
            tracing::debug!("kwin virtual keyboard active: {active}, tablet_mode_check: {tablet_mode_check}, tablet mode: {tablet_mode}");
            if active && tablet_mode {
                fcitx5_osk_services.controller().show().await?;
            }
        }
    }
    Ok(())
}

async fn watch_lockscreen_state(fdo_services: &FdoServices, in_lockscreen: bool) -> Result<()> {
    let mut stream = fdo_services.screen_saver().receive_active_changed().await?;
    while let Some(changed) = stream.next().await {
        let active = changed.args()?.active;
        tracing::debug!("lockscreen active changed, new: {active}");
        if active != in_lockscreen {
            // exit
            break;
        }
    }
    Ok(())
}

async fn run(args: &Args) -> Result<()> {
    let _log_guard = fcitx5_osk_common::log::init_log(&[], args.log_timestamp)?;
    let tablet_mode_check = env::var("FCITX5_OSK_KWIN_LAUNCHER_TABLET_MODE_CHECK")
        .map(|s| !s.eq_ignore_ascii_case("off"))
        .unwrap_or(true);

    let (mut shutdown_flag, signal_handle) = fcitx5_osk_common::signal::shutdown_flag();
    tokio::spawn(signal_handle);

    let socket = match env::var("WAYLAND_SOCKET")
        .unwrap_or_default()
        .parse::<i32>()
    {
        Ok(socket) => {
            let socket = unsafe { OwnedFd::from_raw_fd(socket) };
            Some(socket)
        }
        Err(_) => None,
    };
    let wayland_display = env::var("WAYLAND_DISPLAY").unwrap_or_default();

    // tracing can format argument whose name is display.
    tracing::debug!(
        "wayland socket: {:?}, wayland display: {}",
        socket,
        wayland_display
    );

    let connection = Connection::session().await?;

    let services = Fcitx5OskServices::new().await?;
    let kwin_services = KwinServices::new_with(&connection).await?;
    let fdo_services = FdoServices::new_with(&connection).await?;
    let lockscreen_active = fdo_services.screen_saver().get_active().await?;
    tracing::debug!("first check of lockscreen active: {lockscreen_active}");
    let (fcitx5_socket, fcitx5_osk_socket) = if lockscreen_active {
        (None, socket)
    } else {
        (socket, None)
    };

    let fcitx5_osk_exited = Arc::new(AtomicBool::new(false));
    let fcitx5_osk_handler = tokio::spawn({
        let connection = connection.clone();
        let wayland_display = wayland_display.clone();
        let shutdown_flag = shutdown_flag.clone();
        let fcitx5_osk_exited = fcitx5_osk_exited.clone();
        async move {
            if let Err(e) = watch_fcitx5_osk(
                &connection,
                fcitx5_osk_socket,
                wayland_display,
                shutdown_flag.clone(),
            )
            .await
            {
                tracing::error!("watch_fcitx5_osk exits abnormally: {e:?}");
            } else {
                tracing::info!("watch_fcitx5_osk exits");
            }
            // set fcitx5_osk_exited to true before shutting down.
            fcitx5_osk_exited.store(true, Ordering::Relaxed);
            // tell the main loop to exit
            shutdown_flag.shutdown();
        }
    });

    let watch_fcitx5_fut = watch_fcitx5(
        &connection,
        fcitx5_socket,
        wayland_display.clone(),
        args.fcitx5_reopen,
        shutdown_flag.clone(),
    );

    // only the latest match rule will work in zbus::receive_signal. so I create two connections.
    tokio::select! {
        res = {
            let mut shutdown_flag = shutdown_flag.clone();
            async move {
                if lockscreen_active {
                    // there is no need to watch_fcitx5 in lockscreen mode, wait shutting down.
                    shutdown_flag.wait_for_shutdown().await;
                    Ok(())
                } else {
                    watch_fcitx5_fut.await
                }
            }
        } => {
            if let Err(e) = res {
                tracing::error!("watch_fcitx5 exits abnormally: {e:?}");
            } else {
                tracing::info!("watch_fcitx5 exits");
            }
        }
        res = watch_lockscreen_state(&fdo_services, lockscreen_active) => {
            if let Err(e) = res {
                tracing::error!("watch_lockscreen_state exits abnormally: {e:?}");
            } else {
                tracing::info!("the state of lockscreen is changed");
            }
        }
        res = watch_kwin_virtual_keyboard(&services, &kwin_services, lockscreen_active, tablet_mode_check) => {
            if let Err(e) = res {
                tracing::error!("watch_kwin_virtual_keyboard exits abnormally: {e:?}");
            } else {
                tracing::info!("watch_kwin_virtual_keyboard exits");
            }
        }
        _ = shutdown_flag.wait_for_shutdown() => {
        }
    }

    let mut shutdown_res = None;
    if !fcitx5_osk_exited.load(Ordering::Relaxed) {
        // shutdown fcitx5-osk
        shutdown_res = Some(services.controller().shutdown().await);
        // wait fcitx5-osk to shutdown
        let _ = fcitx5_osk_handler.await;
    }

    // disable and enable virtual keyboard to restart launcher
    let disable_res = kwin_services.virtual_keyboard().set_enabled(false).await;
    let enable_res = kwin_services.virtual_keyboard().set_enabled(true).await;
    tracing::info!(
        "shutdown fcitx5-osk result: {:?}, disable virtual keyboard result: {:?}, enable virtual keyboard result: {:?}",
        shutdown_res,
        disable_res,
        enable_res
    );

    // wait a moment for letting fcitx5-osk to shutdown gracefully.
    tokio::time::sleep(Duration::from_secs(1)).await;

    Ok(())
}

async fn daemon() -> Result<()> {
    let exec = env::current_exe()?;
    let mut args: Vec<_> = env::args().collect();
    if let Some(arg) = args.get_mut(0) {
        *arg = "--worker".to_string();
    } else {
        args.push("--worker".to_string());
    }

    let (mut shutdown_flag, signal_handle) = fcitx5_osk_common::signal::shutdown_flag();
    tokio::spawn(signal_handle);

    let mut child = Command::new(exec).args(args).spawn()?;
    drop(child.stdin.take());
    tokio::select! {
        res = child.wait() => {
            match res {
                Ok(code) => {
                    tracing::warn!("worker exit with code: {code}");
                },
                Err(e) => {
                    tracing::warn!("failed to worker to exit: {e:?}");
                }
            }
        }
        _ = shutdown_flag.wait_for_shutdown() => {
            if let Some(pid) = child.id() {
                cvt::cvt(unsafe { libc::kill(pid as i32, libc::SIGTERM) }).map(drop)?;
            } else {
                child.kill().await?;
            }
        }
    }
    Ok(())
}

async fn run_in_sddm(args: &Args) -> Result<()> {
    let _log_guard = fcitx5_osk_common::log::init_log(&[], args.log_timestamp)?;

    let (mut shutdown_flag, signal_handle) = fcitx5_osk_common::signal::shutdown_flag();
    tokio::spawn(signal_handle);

    let socket = match env::var("WAYLAND_SOCKET")
        .unwrap_or_default()
        .parse::<i32>()
    {
        Ok(socket) => {
            let socket = unsafe { OwnedFd::from_raw_fd(socket) };
            Some(socket)
        }
        Err(_) => None,
    };
    let wayland_display = env::var("WAYLAND_DISPLAY").unwrap_or_default();

    // tracing can format argument whose name is display.
    tracing::debug!(
        "wayland socket: {:?}, wayland display: {}",
        socket,
        wayland_display
    );

    let connection = Connection::session().await?;

    let services = Fcitx5OskServices::new().await?;
    let kwin_services = KwinServices::new_with(&connection).await?;

    let fcitx5_osk_exited = Arc::new(AtomicBool::new(false));
    let fcitx5_osk_handler = tokio::spawn({
        let connection = connection.clone();
        let wayland_display = wayland_display.clone();
        let shutdown_flag = shutdown_flag.clone();
        let fcitx5_osk_exited = fcitx5_osk_exited.clone();
        async move {
            if let Err(e) =
                watch_fcitx5_osk(&connection, socket, wayland_display, shutdown_flag.clone()).await
            {
                tracing::error!("watch_fcitx5_osk exits abnormally: {e:?}");
            } else {
                tracing::info!("watch_fcitx5_osk exits");
            }
            // set fcitx5_osk_exited to true before shutting down.
            fcitx5_osk_exited.store(true, Ordering::Relaxed);
            // tell the main loop to exit
            shutdown_flag.shutdown();
        }
    });

    // only the latest match rule will work in zbus::receive_signal. so I create two connections.
    tokio::select! {
        res = watch_kwin_virtual_keyboard(&services, &kwin_services, true, true) => {
            if let Err(e) = res {
                tracing::error!("watch_kwin_virtual_keyboard exits abnormally: {e:?}");
            } else {
                tracing::info!("watch_kwin_virtual_keyboard exits");
            }
        }
        _ = shutdown_flag.wait_for_shutdown() => {
        }
    }

    let mut shutdown_res = None;
    if !fcitx5_osk_exited.load(Ordering::Relaxed) {
        // shutdown fcitx5-osk
        shutdown_res = Some(services.controller().shutdown().await);
        // wait fcitx5-osk to shutdown
        let _ = fcitx5_osk_handler.await;
    }

    // disable and enable virtual keyboard to restart launcher
    let disable_res = kwin_services.virtual_keyboard().set_enabled(false).await;
    let enable_res = kwin_services.virtual_keyboard().set_enabled(true).await;
    tracing::info!(
        "shutdown fcitx5-osk result: {:?}, disable virtual keyboard result: {:?}, enable virtual keyboard result: {:?}",
        shutdown_res,
        disable_res,
        enable_res
    );

    // wait a moment for letting fcitx5-osk to shutdown gracefully.
    tokio::time::sleep(Duration::from_secs(1)).await;

    Ok(())
}

#[tokio::main]
pub async fn main() {
    let args = Args::parse();
    let res = if args.worker {
        if args.sddm {
            run_in_sddm(&args).await
        } else {
            run(&args).await
        }
    } else {
        daemon().await
    };
    if let Err(e) = res {
        eprintln!("worker[{}] run command failed: {e:?}", args.worker);
        process::exit(1);
    }
}
