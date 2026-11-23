use std::sync::Arc;

use tauri::State;

use crate::error::AppResult;
use crate::services::{BackupInfo, BackupService, BackupStatus, UpdateInfo, UpdateService};

type Svc<'a> = State<'a, Arc<BackupService>>;

#[tauri::command]
pub async fn backup_status(s: Svc<'_>) -> AppResult<BackupStatus> {
    Ok(s.status().await)
}

#[tauri::command]
pub async fn backup_list(s: Svc<'_>) -> AppResult<Vec<BackupInfo>> {
    Ok(s.list()?)
}

#[tauri::command]
pub async fn backup_now(s: Svc<'_>) -> AppResult<BackupInfo> {
    Ok(s.backup_now("manual").await?)
}

/// Write a copy of all data to a file chosen in the save dialog.
#[tauri::command]
pub async fn backup_export(s: Svc<'_>, path: String) -> AppResult<()> {
    Ok(s.export(&path).await?)
}

/// Verify a database file and stage it; it replaces the data at the next start.
#[tauri::command]
pub async fn backup_import(s: Svc<'_>, path: String) -> AppResult<()> {
    Ok(s.stage_import(&path).await?)
}

#[tauri::command]
pub async fn backup_restore(s: Svc<'_>, name: String) -> AppResult<()> {
    Ok(s.stage_restore(&name).await?)
}

#[tauri::command]
pub async fn backup_cancel_import(s: Svc<'_>) -> AppResult<()> {
    Ok(s.cancel_import()?)
}

#[tauri::command]
pub fn restart_app(app: tauri::AppHandle) {
    app.restart();
}

type Upd<'a> = State<'a, Arc<UpdateService>>;

/// `force` is true for the "Check now" button; otherwise nothing is contacted while checking is off.
#[tauri::command]
pub async fn update_check(s: Upd<'_>, force: bool) -> AppResult<UpdateInfo> {
    Ok(s.check(force).await?)
}

#[tauri::command]
pub async fn update_skip(s: Upd<'_>, version: String) -> AppResult<()> {
    Ok(s.skip(&version).await?)
}

/// Download, verify and install the update, then start the new executable and quit.
#[tauri::command]
pub async fn update_install(app: tauri::AppHandle, s: Upd<'_>) -> AppResult<()> {
    let exe = s.install().await?;
    std::process::Command::new(&exe).spawn().map_err(|e| {
        crate::error::DomainError::External(format!("could not start the new version: {e}"))
    })?;
    app.exit(0);
    Ok(())
}
