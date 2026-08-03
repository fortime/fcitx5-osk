use std::{
    env,
    os::fd::{FromRawFd, OwnedFd},
    path::PathBuf,
    process,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
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
use futures_util::{FutureExt as _, StreamExt};
use tokio::{
    sync::mpsc::{self, UnboundedReceiver, UnboundedSender},
    time,
};
use zbus::{
    Connection,
    fdo::{DBusProxy, Result as ZbusFdoResult},
    names::{UniqueName, WellKnownName},
};

use crate::dbus::{
    client::{
        Fcitx5ControllerServiceProxy, Fcitx5OskKwinLauncherControllerServiceProxy, FdoServices,
    },
    server::Fcitx5OskKwinLauncherService,
};

mod dbus;
mod kwin;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Force the program running on wayland.
    #[arg(long, default_missing_value = "true")]
    log_timestamp: bool,

    /// Use reopen api.
    #[arg(long, default_missing_value = "true")]
    fcitx5_reopen: bool,

    /// Start for sddm.
    #[arg(long, default_missing_value = "true")]
    sddm: bool,

    /// Start as dbus server.
    #[arg(long, default_missing_value = "true")]
    dbus_server: bool,
}

enum Message {
    RegisterKwinInputMethod(String),
    RestartKwinInputMethod,
    HealthCheck,
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
        time::sleep(Duration::from_millis(200)).await;
    }
    if in_lockscreen {
        // The keyboard will be open through activate signal of wayland input-method, there is no
        // need to open it manually. Otherwise, the state of kwin virtual keyboard in lockscreen
        // will be wrong (The virtual keyboard won't block auto hide of lockscreen).
        // fcitx5_osk_services.controller().force_show().await?;
        let mut last_visible_request_id = None;
        let mut last_visible = false;
        let mut visible_request_stream = fcitx5_osk_services
            .controller()
            .receive_visible_request_changed()
            .await;
        let mut visible_changed_stream = kwin_services
            .virtual_keyboard()
            .receive_visible_changed()
            .await?;
        let mut visible_changed_future = visible_changed_stream.next().fuse();
        let mut visible_request_future = visible_request_stream.next().fuse();
        loop {
            let visible;
            futures_util::select! {
                visible_changed_res = visible_changed_future => {
                    visible_changed_future = visible_changed_stream.next().fuse();
                    if visible_changed_res.is_some() {
                        visible = kwin_services.virtual_keyboard().visible().await?;
                        tracing::debug!("kwin virtual keyboard visible: {visible}");
                    } else {
                        continue;
                    }
                },
                visible_request_res = visible_request_future => {
                    visible_request_future = visible_request_stream.next().fuse();
                    if let Some(changed) = visible_request_res {
                        let req = changed.get().await?;
                        let req_id = req.0;
                        visible = req.1;
                        tracing::debug!("fcitx5-osk visible request: ({req_id}, {visible})");
                        if last_visible_request_id == Some(req_id) {
                            // Ignore if the id is the same
                            continue;
                        }
                        last_visible_request_id = Some(req_id);
                    } else {
                        continue;
                    }
                },
            }
            if visible != last_visible && !visible {
                // Deactivate the input method, so the virtual keyboard will be closed.
                kwin_services.virtual_keyboard().set_active(false).await?;
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
            let tablet_mode = kwin_services.tablet_mode().tablet_mode().await?;
            // check tablet mode, show only if it is in tablet mode.
            tracing::debug!("kwin virtual keyboard active: {active}, tablet mode: {tablet_mode}");
            if active
                && tablet_mode
                && let Err(e) = fcitx5_osk_services.controller().show().await
            {
                // allow error
                tracing::error!("Unable to call `show` of fcitx5 osk: {e:#?}");
            }
        }
    }
    Ok(())
}

async fn watch_lockscreen_state(
    fdo_services: &FdoServices,
    tx: UnboundedSender<Message>,
) -> Result<()> {
    let mut stream = fdo_services.screen_saver().receive_active_changed().await?;
    let mut last = None;
    while let Some(changed) = stream.next().await {
        let active = changed.args()?.active;
        tracing::debug!("lockscreen active changed, new: {active}");
        if last.filter(|l| *l == active).is_none() {
            // raise the error
            tx.send(Message::RestartKwinInputMethod)?;
        }
        last = Some(active);
    }
    Ok(())
}

fn kreadconfig() -> Option<PathBuf> {
    env::var("FCITX5_OSK_KWIN_LAUNCHER_KREADCONFIG_PATH")
        .ok()
        .map(PathBuf::from)
}

fn kwriteconfig() -> Option<PathBuf> {
    env::var("FCITX5_OSK_KWIN_LAUNCHER_KWRITECONFIG_PATH")
        .ok()
        .map(PathBuf::from)
}

fn health_check_seconds() -> u64 {
    env::var("FCITX5_OSK_KWIN_LAUNCHER_HEALTH_CHECK_SECONDS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(20)
}

async fn dbus_eventloop(mut rx: UnboundedReceiver<Message>) {
    let kreadconfig = kreadconfig();
    let kwriteconfig = kwriteconfig();
    let health_check_enabled = env::var("FCITX5_OSK_KWIN_LAUNCHER_HEALTH_CHECK_ENABLED")
        .map(|s| !s.eq_ignore_ascii_case("off"))
        .unwrap_or(true);
    // restart the kwin input method if there is no register event within 3 health check intervals
    let health_check_timeout_duration = Duration::from_secs(health_check_seconds() * 3);

    let mut kwin_input_method = Default::default();
    let mut last_register = Instant::now();
    while let Some(message) = rx.recv().await {
        let mut need_restart = false;
        match message {
            Message::RegisterKwinInputMethod(kim) => {
                if kim != kwin_input_method {
                    tracing::info!(
                        "kwin input method is changed from [{kwin_input_method}] to [{kim}]"
                    );
                    kwin_input_method = kim;
                }
                last_register = Instant::now();
            }
            Message::RestartKwinInputMethod => {
                need_restart = true;
            }
            Message::HealthCheck => {
                if health_check_enabled && last_register.elapsed() > health_check_timeout_duration {
                    need_restart = true;
                }
            }
        }
        if need_restart
            && !kwin_input_method.is_empty()
            && let Err(e) = kwin::restart_input_method(
                kreadconfig.as_ref(),
                kwriteconfig.as_ref(),
                Some(&kwin_input_method),
                None,
            )
            .await
        {
            tracing::error!("restart input method error: {e:#?}");
        }
    }
}

async fn run_dbus_server(args: &Args) -> Result<()> {
    async fn health_check(tx: UnboundedSender<Message>) -> Result<()> {
        let health_check_duration = Duration::from_secs(health_check_seconds());
        let mut tick = time::interval(health_check_duration);
        loop {
            tick.tick().await;
            tx.send(Message::HealthCheck)?;
        }
    }

    let _log_guard = fcitx5_osk_common::log::init_log(&[], args.log_timestamp)?;

    let (mut shutdown_flag, signal_handle) = fcitx5_osk_common::signal::shutdown_flag();
    tokio::spawn(signal_handle);

    let connection = Connection::session().await?;

    let (tx, rx) = mpsc::unbounded_channel();

    let fcitx5_osk_kwin_launcher_service = Fcitx5OskKwinLauncherService::new(tx.clone());
    let fdo_services = FdoServices::new_with(&connection).await?;

    fcitx5_osk_kwin_launcher_service.start(&connection).await?;

    tokio::select! {
        _ = dbus_eventloop(rx) => {
            tracing::error!("dbus_eventloop returns abnormally");
        }
        res = health_check(tx.clone()) => {
            tracing::error!("health_check returns abnormally: {res:#?}");
        }
        res = watch_lockscreen_state(&fdo_services, tx.clone()) => {
            tracing::error!("watch_lockscreen_state exits abnormally: {res:#?}");
        }
        _ = shutdown_flag.wait_for_shutdown() => {
            tracing::info!("dbus server is shutting down");
        }
    }

    Ok(())
}

async fn run(args: &Args) -> Result<()> {
    async fn health_check(
        proxy: Fcitx5OskKwinLauncherControllerServiceProxy<'_>,
        kwin_input_method: &str,
    ) {
        let health_check_duration = Duration::from_secs(health_check_seconds());
        let mut tick = time::interval(health_check_duration);
        loop {
            tick.tick().await;
            if let Err(e) = proxy.register_kwin_input_method(kwin_input_method).await {
                tracing::error!("failed to register kwin input method: {e:#?}");
            }
        }
    }

    let kreadconfig = kreadconfig();
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

    let cur_input_method = match kwin::cur_input_method(kreadconfig.as_ref()).await {
        Ok(p) => p,
        Err(e) => {
            tracing::error!(
                "Unable to get current input method, a empty string will be used: {e:#?}"
            );
            String::new()
        }
    };

    // tracing can format argument whose name is display.
    tracing::debug!(
        "wayland socket: {:?}, wayland display: {}, input method: {}",
        socket,
        wayland_display,
        cur_input_method,
    );

    let connection = Connection::session().await?;

    let services = Fcitx5OskServices::new().await?;
    let fcitx5_osk_kwin_launcher_controller_service =
        Fcitx5OskKwinLauncherControllerServiceProxy::new(&connection).await?;
    let kwin_services = KwinServices::new_with(&connection).await?;
    let fdo_services = FdoServices::new_with(&connection).await?;
    let lockscreen_active = fdo_services.screen_saver().get_active().await?;
    tracing::debug!("lockscreen active: {lockscreen_active}");
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
                tracing::error!("watch_fcitx5_osk exits abnormally: {e:#?}");
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
        _ = health_check(fcitx5_osk_kwin_launcher_controller_service, &cur_input_method) => {
            tracing::error!("health_check returns abnormally");
        }
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
                tracing::error!("watch_fcitx5 exits abnormally: {e:#?}");
            } else {
                tracing::info!("watch_fcitx5 exits");
            }
        }
        res = watch_kwin_virtual_keyboard(&services, &kwin_services, lockscreen_active) => {
            if let Err(e) = res {
                tracing::error!("watch_kwin_virtual_keyboard exits abnormally: {e:#?}");
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

    tracing::info!("shutdown fcitx5-osk result: {:?}", shutdown_res,);

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
                tracing::error!("watch_fcitx5_osk exits abnormally: {e:#?}");
            } else {
                tracing::error!("watch_fcitx5_osk exits");
            }
            // set fcitx5_osk_exited to true before shutting down.
            fcitx5_osk_exited.store(true, Ordering::Relaxed);
            // tell the main loop to exit
            shutdown_flag.shutdown();
        }
    });

    // only the latest match rule will work in zbus::receive_signal. so I create two connections.
    tokio::select! {
        res = watch_kwin_virtual_keyboard(&services, &kwin_services, true) => {
            if let Err(e) = res {
                tracing::error!("watch_kwin_virtual_keyboard exits abnormally: {e:#?}");
            } else {
                tracing::error!("watch_kwin_virtual_keyboard exits");
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

    // There is no need to restart in sddm
    tracing::info!("shutdown fcitx5-osk result: {:?}", shutdown_res,);

    Ok(())
}

#[tokio::main]
pub async fn main() {
    let args = Args::parse();
    let res = if args.sddm {
        run_in_sddm(&args).await
    } else if args.dbus_server {
        run_dbus_server(&args).await
    } else {
        run(&args).await
    };
    if let Err(e) = res {
        eprintln!("run command failed: {e:#?}");
        process::exit(1);
    }
}
