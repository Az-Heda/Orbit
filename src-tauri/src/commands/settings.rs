use tauri::State;

use crate::domain::activity::Actor;
use crate::domain::settings::Setting;
use crate::error::AppResult;
use crate::state::AppState;

#[tauri::command]
pub async fn get_setting(state: State<'_, AppState>, key: String) -> AppResult<Option<Setting>> {
    Ok(state.settings.get(&key).await?)
}

#[tauri::command]
pub async fn list_settings(state: State<'_, AppState>) -> AppResult<Vec<Setting>> {
    Ok(state.settings.list().await?)
}

#[tauri::command]
pub async fn set_setting(
    state: State<'_, AppState>,
    key: String,
    value: String,
) -> AppResult<Setting> {
    Ok(state.settings.set(&key, &value, Actor::User).await?)
}
