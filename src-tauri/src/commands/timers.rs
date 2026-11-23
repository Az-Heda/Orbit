use tauri::{AppHandle, Emitter, State};

use crate::domain::activity::Actor;
use crate::domain::stats::Stats;
use crate::domain::timers::{FocusSession, TimerView};
use crate::error::AppResult;
use crate::services::StartTimerInput;
use crate::state::AppState;

const U: Actor = Actor::User;

#[tauri::command]
pub async fn start_timer(
    app: AppHandle,
    s: State<'_, AppState>,
    input: StartTimerInput,
) -> AppResult<TimerView> {
    let view = s.timers.start(input, U).await?;
    let _ = app.emit("timer-changed", ());
    Ok(view)
}

#[tauri::command]
pub async fn pause_timer(
    app: AppHandle,
    s: State<'_, AppState>,
    id: String,
) -> AppResult<TimerView> {
    let view = s.timers.pause(&id).await?;
    let _ = app.emit("timer-changed", ());
    Ok(view)
}

#[tauri::command]
pub async fn resume_timer(
    app: AppHandle,
    s: State<'_, AppState>,
    id: String,
) -> AppResult<TimerView> {
    let view = s.timers.resume(&id).await?;
    let _ = app.emit("timer-changed", ());
    Ok(view)
}

#[tauri::command]
pub async fn stop_timer(
    app: AppHandle,
    s: State<'_, AppState>,
    id: String,
) -> AppResult<TimerView> {
    let view = s.timers.stop(&id).await?;
    let _ = app.emit("timer-changed", ());
    Ok(view)
}

#[tauri::command]
pub async fn active_timers(s: State<'_, AppState>) -> AppResult<Vec<TimerView>> {
    Ok(s.timers.active().await?)
}

#[tauri::command]
pub async fn timer_history(
    s: State<'_, AppState>,
    limit: Option<i64>,
) -> AppResult<Vec<TimerView>> {
    Ok(s.timers.history(limit.unwrap_or(30)).await?)
}

#[tauri::command]
pub async fn list_focus_sessions(
    s: State<'_, AppState>,
    from: String,
    to: String,
    task_id: Option<String>,
) -> AppResult<Vec<FocusSession>> {
    Ok(s.timers.sessions(&from, &to, task_id.as_deref()).await?)
}

#[tauri::command]
pub async fn set_session_task(
    s: State<'_, AppState>,
    session_id: String,
    task_id: Option<String>,
) -> AppResult<FocusSession> {
    Ok(s.timers
        .set_session_task(&session_id, task_id.as_deref(), U)
        .await?)
}

#[tauri::command]
pub async fn get_stats(s: State<'_, AppState>, timezone: String, days: i64) -> AppResult<Stats> {
    Ok(s.stats.overview(&timezone, days).await?)
}
