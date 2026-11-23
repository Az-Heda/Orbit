use tauri::AppHandle;
use tauri::State;
use tauri_plugin_autostart::ManagerExt;

use crate::error::{AppError, AppResult, DomainError};
use crate::shell::{self, ShortcutStatus};

fn fail(msg: impl ToString) -> AppError {
    AppError(DomainError::Storage(msg.to_string()))
}

#[tauri::command]
pub fn toggle_widget(app: AppHandle) {
    shell::toggle_widget(&app);
}

#[tauri::command]
pub async fn set_widget_click_through(app: AppHandle, enabled: bool) -> AppResult<()> {
    shell::set_click_through(&app, enabled).await;
    Ok(())
}

#[tauri::command]
pub fn open_palette(app: AppHandle) {
    shell::open_palette(&app);
}

#[tauri::command]
pub fn open_capture(app: AppHandle) {
    shell::open_capture(&app);
}

#[tauri::command]
pub fn hide_window(app: AppHandle, label: String) -> AppResult<()> {
    if !matches!(
        label.as_str(),
        shell::WIDGET | shell::PALETTE | shell::CAPTURE
    ) {
        return Err(fail("unknown window"));
    }
    shell::hide_window(&app, &label);
    Ok(())
}

#[tauri::command]
pub fn open_main(app: AppHandle, path: Option<String>) {
    shell::open_main(&app, path);
}

#[tauri::command]
pub fn get_autostart(app: AppHandle) -> AppResult<bool> {
    app.autolaunch().is_enabled().map_err(fail)
}

#[tauri::command]
pub fn set_autostart(app: AppHandle, enabled: bool) -> AppResult<bool> {
    let m = app.autolaunch();
    if enabled { m.enable() } else { m.disable() }.map_err(fail)?;
    m.is_enabled().map_err(fail)
}

/// Shortcuts that could not be registered (e.g. taken by another app), with the reason.
#[tauri::command]
pub fn shortcut_failures(status: State<'_, ShortcutStatus>) -> Vec<String> {
    status.0.lock().unwrap().clone()
}

/// Re-read `shortcuts.*` settings and register them again.
#[tauri::command]
pub async fn apply_shortcuts(app: AppHandle) -> Vec<String> {
    shell::apply_shortcuts(&app).await
}
