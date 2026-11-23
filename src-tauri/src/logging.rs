use tracing_subscriber::EnvFilter;

/// Initialise tracing. Never log secrets (API keys, tokens) or setting values.
pub fn init() {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,sqlx=warn"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}
