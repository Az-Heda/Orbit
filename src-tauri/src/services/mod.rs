//! Business logic. Used by both UI commands and (later) AI tools. Depends on repository traits only.

mod activity_service;
mod settings_service;

pub use activity_service::ActivityService;
pub use settings_service::SettingsService;
