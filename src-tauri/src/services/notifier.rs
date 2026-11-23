/// Sends a desktop notification. The real implementation lives in the Tauri shell; tests record.
pub trait Notifier: Send + Sync {
    fn notify(&self, title: &str, body: &str);
}

/// Discards notifications (used when no shell is attached).
pub struct NullNotifier;
impl Notifier for NullNotifier {
    fn notify(&self, _title: &str, _body: &str) {}
}
