use tauri::State;

use crate::domain::activity::Actor;
use crate::domain::channels::{Channel, ChannelInput};
use crate::error::AppResult;
use crate::state::AppState;

const U: Actor = Actor::User;

#[tauri::command]
pub async fn list_channels(s: State<'_, AppState>) -> AppResult<Vec<Channel>> {
    Ok(s.channels.list().await?)
}

/// `input.secret` (bot token / webhook URL) goes to the credential store and is never returned.
#[tauri::command]
pub async fn create_channel(s: State<'_, AppState>, input: ChannelInput) -> AppResult<Channel> {
    Ok(s.channels.create(input, U).await?)
}

#[tauri::command]
pub async fn update_channel(
    s: State<'_, AppState>,
    id: String,
    input: ChannelInput,
) -> AppResult<Channel> {
    Ok(s.channels.update(&id, input, U).await?)
}

#[tauri::command]
pub async fn delete_channel(s: State<'_, AppState>, id: String) -> AppResult<()> {
    Ok(s.channels.delete(&id, U).await?)
}

#[tauri::command]
pub async fn send_test_notification(s: State<'_, AppState>, id: String) -> AppResult<()> {
    Ok(s.channels.send_test(&id).await?)
}
