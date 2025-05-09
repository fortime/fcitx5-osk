use anyhow::Result;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

pub fn init_log(directives: &[String], log_timestamp: bool) -> Result<()> {
    let mut env_filter = EnvFilter::from_default_env();
    for directive in directives {
        env_filter = env_filter.add_directive(directive.parse()?);
    }
    let subscriber = tracing_subscriber::registry().with(env_filter);
    #[cfg(debug_assertions)]
    let subscriber = subscriber.with(
        console_subscriber::ConsoleLayer::builder()
            .with_default_env()
            .spawn(),
    );

    if log_timestamp {
        subscriber.with(fmt::layer()).try_init()?;
    } else {
        subscriber.with(fmt::layer().without_time()).try_init()?;
    }
    Ok(())
}
