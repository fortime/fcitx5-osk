use std::fmt::{Display, Formatter, Result as FmtResult};

use anyhow::Result;
use tempfile::{NamedTempFile, TempPath};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

pub struct LogGuard {
    #[cfg(debug_assertions)]
    socket: TempPath,
}

impl LogGuard {
    pub fn new() -> Result<Self> {
        Ok(Self {
            #[cfg(debug_assertions)]
            socket: NamedTempFile::with_prefix("fcitx5-osk-tokio-console-")?.into_temp_path(),
        })
    }
}

#[cfg(debug_assertions)]
impl Display for LogGuard {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.write_fmt(format_args!("console_subscriber addr: {:?}", self.socket))?;
        Ok(())
    }
}

pub fn init_log(directives: &[String], log_timestamp: bool) -> Result<LogGuard> {
    let mut env_filter = EnvFilter::from_default_env();
    for directive in directives {
        env_filter = env_filter.add_directive(directive.parse()?);
    }
    let log_guard = LogGuard::new()?;
    let subscriber = tracing_subscriber::registry().with(env_filter);
    #[cfg(debug_assertions)]
    let subscriber = {
        // use a tempfile as addr of console_subscriber. So it won't panic in the restart of the
        // fcitx5-osk-kwin-launcher.
        let socket_file_path = log_guard.socket.to_path_buf();
        // delete the file, so console_subscriber can create it.
        std::fs::remove_file(&socket_file_path)?;
        subscriber.with(
            console_subscriber::ConsoleLayer::builder()
                .with_default_env()
                .server_addr(socket_file_path)
                .spawn(),
        )
    };

    if log_timestamp {
        subscriber.with(fmt::layer()).try_init()?;
    } else {
        subscriber.with(fmt::layer().without_time()).try_init()?;
    }
    #[cfg(debug_assertions)]
    tracing::debug!("{}", log_guard);
    Ok(log_guard)
}
