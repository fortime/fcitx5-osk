use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicU8, Ordering},
    },
    task::{Context, Poll},
};

use anyhow::Result;
use tokio::{
    signal::unix::{Signal, SignalKind, signal},
    sync::watch::{self, Receiver, Sender},
};

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
        //SignalKind::pipe(),
        SignalKind::quit(),
    ])
    .await
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ShutdownKind {
    NOT = 0,
    NORMAL = 1,
    /// Only make sense when it is running under wayland
    FORCE = 2,

    UNKNOWN = 255,
}

impl ShutdownKind {
    pub fn is_shutting_down(&self) -> bool {
        match self {
            ShutdownKind::NOT => false,
            ShutdownKind::NORMAL => true,
            ShutdownKind::FORCE => true,
            ShutdownKind::UNKNOWN => false,
        }
    }
}

impl From<u8> for ShutdownKind {
    fn from(value: u8) -> Self {
        match value {
            0 => ShutdownKind::NOT,
            1 => ShutdownKind::NORMAL,
            2 => ShutdownKind::FORCE,
            _ => ShutdownKind::UNKNOWN,
        }
    }
}

pub struct ShutdownFlagState {
    flag: AtomicU8,
    cvar: (Mutex<bool>, Condvar),
    tx: Sender<bool>,
}

#[derive(Clone)]
pub struct ShutdownFlag {
    state: Arc<ShutdownFlagState>,
    rx: Receiver<bool>,
}

impl ShutdownFlag {
    fn new() -> Self {
        let (tx, rx) = watch::channel(false);
        Self {
            state: Arc::new(ShutdownFlagState {
                flag: AtomicU8::new(ShutdownKind::NOT as u8),
                cvar: (Mutex::new(false), Condvar::new()),
                tx,
            }),
            rx,
        }
    }

    pub fn get(&self) -> ShutdownKind {
        self.state.flag.load(Ordering::Relaxed).into()
    }

    fn shutdown_with_kind(&self, kind: ShutdownKind) {
        if self
            .state
            .flag
            .compare_exchange(
                ShutdownKind::NOT as u8,
                kind as u8,
                Ordering::Acquire,
                Ordering::Relaxed,
            )
            .is_ok()
        {
            if self.state.tx.send(true).is_err() {
                tracing::error!("failed to send shutdown flag");
            }
            let mut guard = self
                .state
                .cvar
                .0
                .lock()
                .expect("shutdown flag lock is poisoned");
            *guard = true;
            self.state.cvar.1.notify_all();
        }
    }

    pub fn shutdown(&self) {
        self.shutdown_with_kind(ShutdownKind::NORMAL);
    }

    pub fn force_shutdown(&self) {
        self.shutdown_with_kind(ShutdownKind::FORCE);
    }

    pub async fn wait_for_shutdown(&mut self) {
        loop {
            if *self.rx.borrow() {
                break;
            }
            if self.rx.changed().await.is_err() {
                tracing::error!("failed to waiting the change of shutdown flag");
                break;
            }
        }
    }

    pub fn wait_for_shutdown_blocking(&mut self) {
        loop {
            let guard = self
                .state
                .cvar
                .0
                .lock()
                .expect("shutdown flag lock is poisoned");
            if *self
                .state
                .cvar
                .1
                .wait(guard)
                .expect("shutdown flag lock is poisoned")
            {
                break;
            }
        }
    }
}

pub fn shutdown_flag() -> (ShutdownFlag, impl Future<Output = ()>) {
    let shutdown_flag = ShutdownFlag::new();
    let signal_handle = {
        let shutdown_flag = shutdown_flag.clone();
        async move {
            match try_default_signals().await {
                Ok(signals) => {
                    let res = signals.await;
                    shutdown_flag.shutdown();
                    tracing::info!("stopping by signal: {:?}", res);
                }
                Err(e) => {
                    shutdown_flag.shutdown();
                    tracing::error!("failed to create signal handle: {:?}", e);
                }
            }
        }
    };
    (shutdown_flag, signal_handle)
}
