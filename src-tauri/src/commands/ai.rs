//! Thin IPC handlers for the assistant. The API key can be set and cleared but never read back.

use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};

use crate::domain::ai::{AiConfig, ChatContext, Conversation, Memory, Proposal, SharingSettings};
use crate::error::AppResult;
use crate::infrastructure::ai::secrets::KeyStatus;
use crate::infrastructure::database::UndoReport;
use crate::services::{ChatEvent, ConversationDetail, StreamSink};
use crate::state::AppState;

struct TauriSink(AppHandle);

impl StreamSink for TauriSink {
    fn emit(&self, event: ChatEvent) {
        let _ = self.0.emit("ai-stream", event);
    }
}

/// Runs one turn; progress arrives as `ai-stream` events. Returns the conversation id.
#[tauri::command]
pub async fn ai_send_message(
    app: AppHandle,
    state: State<'_, AppState>,
    conversation_id: Option<String>,
    text: String,
    context: ChatContext,
) -> AppResult<String> {
    let ai = Arc::clone(&state.ai);
    Ok(ai
        .send(conversation_id.as_deref(), &text, context, &TauriSink(app))
        .await?)
}

#[tauri::command]
pub fn ai_cancel(state: State<'_, AppState>, conversation_id: String) {
    state.ai.cancel(&conversation_id);
}

#[tauri::command]
pub async fn ai_list_conversations(state: State<'_, AppState>) -> AppResult<Vec<Conversation>> {
    Ok(state.ai.conversations().await?)
}

#[tauri::command]
pub async fn ai_get_conversation(
    state: State<'_, AppState>,
    id: String,
) -> AppResult<ConversationDetail> {
    Ok(state.ai.conversation(&id).await?)
}

#[tauri::command]
pub async fn ai_delete_conversation(state: State<'_, AppState>, id: String) -> AppResult<()> {
    Ok(state.ai.delete_conversation(&id).await?)
}

#[tauri::command]
pub async fn ai_apply_proposal(state: State<'_, AppState>, id: String) -> AppResult<Proposal> {
    Ok(state.ai.apply_proposal(&id).await?)
}

#[tauri::command]
pub async fn ai_reject_proposal(state: State<'_, AppState>, id: String) -> AppResult<Proposal> {
    Ok(state.ai.reject_proposal(&id).await?)
}

#[tauri::command]
pub async fn ai_undo_proposal(
    state: State<'_, AppState>,
    id: String,
) -> AppResult<(Proposal, UndoReport)> {
    Ok(state.ai.undo_proposal(&id).await?)
}

#[tauri::command]
pub fn ai_key_status(state: State<'_, AppState>) -> AppResult<KeyStatus> {
    Ok(state.ai.key_status()?)
}

#[tauri::command]
pub fn ai_set_key(state: State<'_, AppState>, key: String) -> AppResult<KeyStatus> {
    Ok(state.ai.set_key(&key)?)
}

#[tauri::command]
pub fn ai_clear_key(state: State<'_, AppState>) -> AppResult<KeyStatus> {
    Ok(state.ai.clear_key()?)
}

#[tauri::command]
pub async fn ai_get_config(state: State<'_, AppState>) -> AppResult<AiConfig> {
    Ok(state.ai.config().await)
}

#[tauri::command]
pub async fn ai_set_config(state: State<'_, AppState>, config: AiConfig) -> AppResult<AiConfig> {
    Ok(state.ai.set_config(config).await?)
}

#[tauri::command]
pub async fn ai_get_sharing(state: State<'_, AppState>) -> AppResult<SharingSettings> {
    Ok(state.ai.sharing().await)
}

#[tauri::command]
pub async fn ai_set_sharing(
    state: State<'_, AppState>,
    sharing: SharingSettings,
) -> AppResult<SharingSettings> {
    Ok(state.ai.set_sharing(sharing).await?)
}

#[tauri::command]
pub async fn ai_list_memories(state: State<'_, AppState>) -> AppResult<Vec<Memory>> {
    Ok(state.ai.memories().await?)
}

#[tauri::command]
pub async fn ai_add_memory(state: State<'_, AppState>, text: String) -> AppResult<Memory> {
    Ok(state.ai.add_memory(&text).await?)
}

#[tauri::command]
pub async fn ai_update_memory(
    state: State<'_, AppState>,
    id: String,
    text: String,
) -> AppResult<Memory> {
    Ok(state.ai.update_memory(&id, &text).await?)
}

#[tauri::command]
pub async fn ai_delete_memory(state: State<'_, AppState>, id: String) -> AppResult<()> {
    Ok(state.ai.delete_memory(&id).await?)
}
