use tauri::State;

use crate::domain::activity::Actor;
use crate::domain::calendar::{CalendarItem, Event, ImportSummary};
use crate::error::{AppError, AppResult, DomainError};
use crate::services::{CreateEventInput, OverrideInput, UpdateEventInput};
use crate::state::AppState;

const U: Actor = Actor::User;

fn io_err(e: std::io::Error) -> AppError {
    AppError(DomainError::Storage(format!("file error: {e}")))
}

#[tauri::command]
pub async fn list_calendar_range(
    s: State<'_, AppState>,
    from: String,
    to: String,
) -> AppResult<Vec<CalendarItem>> {
    Ok(s.calendar.list_range(&from, &to).await?)
}

#[tauri::command]
pub async fn get_event(s: State<'_, AppState>, id: String) -> AppResult<Event> {
    Ok(s.calendar.get_event(&id).await?)
}

#[tauri::command]
pub async fn create_event(s: State<'_, AppState>, input: CreateEventInput) -> AppResult<Event> {
    Ok(s.calendar.create_event(input, U).await?)
}

#[tauri::command]
pub async fn update_event(
    s: State<'_, AppState>,
    id: String,
    input: UpdateEventInput,
) -> AppResult<Event> {
    Ok(s.calendar.update_event(&id, input, U).await?)
}

#[tauri::command]
pub async fn delete_event(s: State<'_, AppState>, id: String) -> AppResult<()> {
    Ok(s.calendar.delete_event(&id, U).await?)
}

#[tauri::command]
pub async fn skip_occurrence(
    s: State<'_, AppState>,
    event_id: String,
    original_start: String,
) -> AppResult<()> {
    Ok(s.calendar
        .skip_occurrence(&event_id, &original_start, U)
        .await?)
}

#[tauri::command]
pub async fn override_occurrence(
    s: State<'_, AppState>,
    event_id: String,
    original_start: String,
    input: OverrideInput,
) -> AppResult<()> {
    Ok(s.calendar
        .override_occurrence(&event_id, &original_start, input, U)
        .await?)
}

#[tauri::command]
pub async fn restore_occurrence(
    s: State<'_, AppState>,
    event_id: String,
    original_start: String,
) -> AppResult<()> {
    Ok(s.calendar
        .restore_occurrence(&event_id, &original_start, U)
        .await?)
}

#[tauri::command]
pub async fn set_task_recurrence(
    s: State<'_, AppState>,
    task_id: String,
    rule: Option<String>,
    timezone: String,
) -> AppResult<()> {
    Ok(s.calendar
        .set_task_recurrence(&task_id, rule.as_deref(), &timezone, U)
        .await?)
}

#[tauri::command]
pub async fn set_task_occurrence(
    s: State<'_, AppState>,
    task_id: String,
    original_due: String,
    status: String,
) -> AppResult<()> {
    Ok(s.calendar
        .set_task_occurrence(&task_id, &original_due, &status, U)
        .await?)
}

/// `path` comes from the native save dialog.
#[tauri::command]
pub async fn export_ics_file(s: State<'_, AppState>, path: String) -> AppResult<()> {
    let text = s.calendar.export_ics().await?;
    std::fs::write(path, text).map_err(io_err)?;
    Ok(())
}

/// `path` comes from the native open dialog.
#[tauri::command]
pub async fn import_ics_file(
    s: State<'_, AppState>,
    path: String,
    timezone: String,
) -> AppResult<ImportSummary> {
    let text = std::fs::read_to_string(path).map_err(io_err)?;
    Ok(s.calendar.import_ics(&text, &timezone, U).await?)
}

// ---------- subscribed (read-only) calendars ----------

#[tauri::command]
pub async fn list_external_calendars(
    s: State<'_, AppState>,
) -> AppResult<Vec<crate::domain::external::ExternalCalendar>> {
    Ok(s.external.list().await?)
}

#[tauri::command]
pub async fn add_external_calendar(
    s: State<'_, AppState>,
    name: String,
    url: String,
) -> AppResult<crate::domain::external::ExternalCalendar> {
    Ok(s.external.add(&name, &url).await?)
}

#[tauri::command]
pub async fn remove_external_calendar(s: State<'_, AppState>, id: String) -> AppResult<()> {
    Ok(s.external.remove(&id).await?)
}

#[tauri::command]
pub async fn sync_external_calendar(
    s: State<'_, AppState>,
    id: String,
) -> AppResult<crate::domain::external::SyncReport> {
    Ok(s.external.sync(&id).await?)
}
