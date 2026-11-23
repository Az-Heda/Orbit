use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::domain::activity::Actor;
use crate::domain::automation::Rule;
use crate::domain::calendar::Event;
use crate::domain::inbox::InboxItem;
use crate::domain::kanban::Task;
use crate::error::AppResult;
use crate::services::RuleInput;
use crate::state::AppState;

const U: Actor = Actor::User;

#[derive(Serialize)]
pub struct TriagedTask {
    item: InboxItem,
    task: Task,
}
#[derive(Serialize)]
pub struct TriagedEvent {
    item: InboxItem,
    event: Event,
}

#[tauri::command]
pub async fn capture_inbox(
    app: AppHandle,
    s: State<'_, AppState>,
    text: String,
) -> AppResult<InboxItem> {
    let item = s.inbox.capture(&text, U).await?;
    let _ = app.emit("inbox-changed", ());
    Ok(item)
}

#[tauri::command]
pub async fn list_inbox(
    s: State<'_, AppState>,
    status: Option<String>,
    limit: Option<i64>,
) -> AppResult<Vec<InboxItem>> {
    Ok(s.inbox
        .list(status.as_deref(), limit.unwrap_or(200))
        .await?)
}

#[tauri::command]
pub async fn inbox_count(s: State<'_, AppState>) -> AppResult<i64> {
    Ok(s.inbox.count_new().await?)
}

#[tauri::command]
pub async fn triage_inbox_to_task(
    app: AppHandle,
    s: State<'_, AppState>,
    id: String,
    column_id: String,
    title: Option<String>,
    due_at: Option<String>,
) -> AppResult<TriagedTask> {
    let (item, task) = s
        .inbox
        .triage_to_task(&id, &column_id, title.as_deref(), due_at.as_deref(), U)
        .await?;
    let _ = app.emit("inbox-changed", ());
    Ok(TriagedTask { item, task })
}

#[tauri::command]
pub async fn triage_inbox_to_event(
    app: AppHandle,
    s: State<'_, AppState>,
    id: String,
    title: Option<String>,
    start_at: String,
    end_at: String,
    timezone: String,
) -> AppResult<TriagedEvent> {
    let (item, event) = s
        .inbox
        .triage_to_event(&id, title.as_deref(), &start_at, &end_at, &timezone, U)
        .await?;
    let _ = app.emit("inbox-changed", ());
    Ok(TriagedEvent { item, event })
}

#[tauri::command]
pub async fn dismiss_inbox(
    app: AppHandle,
    s: State<'_, AppState>,
    id: String,
) -> AppResult<InboxItem> {
    let item = s.inbox.dismiss(&id, U).await?;
    let _ = app.emit("inbox-changed", ());
    Ok(item)
}

// ----- automation rules -----

#[tauri::command]
pub async fn list_rules(s: State<'_, AppState>) -> AppResult<Vec<Rule>> {
    Ok(s.rules.list().await?)
}

#[tauri::command]
pub async fn create_rule(s: State<'_, AppState>, input: RuleInput) -> AppResult<Rule> {
    Ok(s.rules.create(input, U).await?)
}

#[tauri::command]
pub async fn update_rule(s: State<'_, AppState>, id: String, input: RuleInput) -> AppResult<Rule> {
    Ok(s.rules.update(&id, input, U).await?)
}

#[tauri::command]
pub async fn set_rule_enabled(
    s: State<'_, AppState>,
    id: String,
    enabled: bool,
) -> AppResult<Rule> {
    Ok(s.rules.set_enabled(&id, enabled, U).await?)
}

#[tauri::command]
pub async fn delete_rule(s: State<'_, AppState>, id: String) -> AppResult<()> {
    Ok(s.rules.delete(&id, U).await?)
}
