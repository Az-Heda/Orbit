use std::io::Write;
use std::path::PathBuf;

use tracing_subscriber::EnvFilter;

/// Initialise tracing. Never log secrets (API keys, tokens) or setting values.
pub fn init() {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,sqlx=warn"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
    install_crash_log(crash_log_path());
}

/// Where a fatal startup error is written. The release build has no console, so without this a
/// crash during startup is invisible.
pub fn crash_log_path() -> PathBuf {
    std::env::temp_dir().join("orbit-crash.log")
}

/// Append every panic (including a failed `setup`, which ends in one) to `path`, then run the
/// normal hook. Contains only the panic message and location, never settings or secrets.
pub fn install_crash_log(path: PathBuf) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let when = chrono::Utc::now().to_rfc3339();
        let line = format!(
            "[{when}] orbit {} panicked: {info}\n",
            env!("CARGO_PKG_VERSION")
        );
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            let _ = f.write_all(line.as_bytes());
        }
        previous(info);
    }));
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_panic_is_appended_to_the_crash_log() {
        let path = std::env::temp_dir().join(format!("orbit-crash-{}.log", uuid::Uuid::new_v4()));
        let prev = std::panic::take_hook();
        super::install_crash_log(path.clone());
        let _ = std::panic::catch_unwind(|| panic!("boom at startup"));
        std::panic::set_hook(prev);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.contains("boom at startup") && text.contains("panicked"),
            "{text}"
        );
        let _ = std::fs::remove_file(&path);
    }
}
