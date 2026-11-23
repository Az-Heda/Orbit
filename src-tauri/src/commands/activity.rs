use tauri::State;

use crate::domain::activity::ActivityEntry;
use crate::error::AppResult;
use crate::state::AppState;

#[tauri::command]
pub async fn recent_activity(
    state: State<'_, AppState>,
    limit: Option<i64>,
) -> AppResult<Vec<ActivityEntry>> {
    Ok(state.activity.recent(limit.unwrap_or(50)).await?)
}
