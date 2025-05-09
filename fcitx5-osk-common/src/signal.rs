use std::{
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex,
    },
    task::{Context, Poll},
};

use anyhow::Result;
use tokio::{
    signal::unix::{signal, Signal, SignalKind},
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
        SignalKind::pipe(),
        SignalKind::quit(),
    ])
    .await
}

pub struct ShutdownFlagState {
    flag: AtomicBool,
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
                flag: AtomicBool::new(false),
                cvar: (Mutex::new(false), Condvar::new()),
                tx,
            }),
            rx,
        }
    }

    pub fn get(&self) -> bool {
        self.state.flag.load(Ordering::Relaxed)
    }

    pub fn shutdown(&self) {
        self.state.flag.store(true, Ordering::Relaxed);
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
