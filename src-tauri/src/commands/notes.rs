use tauri::State;

use crate::domain::activity::Actor;
use crate::domain::notes::Note;
use crate::domain::search::SearchHit;
use crate::error::AppResult;
use crate::services::NoteDetail;
use crate::state::AppState;

const U: Actor = Actor::User;

#[tauri::command]
pub async fn create_note(
    s: State<'_, AppState>,
    title: String,
    body: Option<String>,
) -> AppResult<Note> {
    Ok(s.notes
        .create(&title, body.as_deref().unwrap_or(""), U)
        .await?)
}

#[tauri::command]
pub async fn get_note(s: State<'_, AppState>, id: String) -> AppResult<NoteDetail> {
    Ok(s.notes.get(&id).await?)
}

#[tauri::command]
pub async fn update_note(
    s: State<'_, AppState>,
    id: String,
    title: Option<String>,
    body: Option<String>,
) -> AppResult<Note> {
    Ok(s.notes
        .update(&id, title.as_deref(), body.as_deref(), U)
        .await?)
}

#[tauri::command]
pub async fn delete_note(s: State<'_, AppState>, id: String) -> AppResult<()> {
    Ok(s.notes.delete(&id, U).await?)
}

#[tauri::command]
pub async fn list_notes(
    s: State<'_, AppState>,
    kind: Option<String>,
    entity_type: Option<String>,
    entity_id: Option<String>,
    limit: Option<i64>,
) -> AppResult<Vec<Note>> {
    let entity = match (&entity_type, &entity_id) {
        (Some(t), Some(i)) => Some((t.as_str(), i.as_str())),
        _ => None,
    };
    Ok(s.notes
        .list(kind.as_deref(), entity, limit.unwrap_or(100))
        .await?)
}

#[tauri::command]
pub async fn open_journal(s: State<'_, AppState>, date: String) -> AppResult<Note> {
    Ok(s.notes.journal(&date, U).await?)
}

#[tauri::command]
pub async fn add_note_link(
    s: State<'_, AppState>,
    note_id: String,
    entity_type: String,
    entity_id: String,
) -> AppResult<()> {
    Ok(s.notes
        .add_link(&note_id, &entity_type, &entity_id, U)
        .await?)
}

#[tauri::command]
pub async fn remove_note_link(
    s: State<'_, AppState>,
    note_id: String,
    entity_type: String,
    entity_id: String,
) -> AppResult<()> {
    Ok(s.notes
        .remove_link(&note_id, &entity_type, &entity_id, U)
        .await?)
}

#[tauri::command]
pub async fn search_all(
    s: State<'_, AppState>,
    query: String,
    kinds: Option<Vec<String>>,
    limit: Option<i64>,
) -> AppResult<Vec<SearchHit>> {
    Ok(s.search.search(&query, kinds, limit).await?)
}
