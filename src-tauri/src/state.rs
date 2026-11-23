use std::sync::Arc;

use sqlx::SqlitePool;

use crate::infrastructure::database::{SqliteActivityRepository, SqliteSettingsRepository};
use crate::services::{ActivityService, SettingsService};

/// Application state managed by Tauri. Wires repositories into services.
pub struct AppState {
    pub settings: SettingsService,
    pub activity: ActivityService,
}

impl AppState {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            settings: SettingsService::new(Arc::new(SqliteSettingsRepository::new(pool.clone()))),
            activity: ActivityService::new(Arc::new(SqliteActivityRepository::new(pool))),
        }
    }
}
