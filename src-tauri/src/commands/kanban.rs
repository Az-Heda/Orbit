use tauri::State;

use crate::domain::activity::Actor;
use crate::domain::kanban::{
    Board, BoardView, ChecklistItem, Column, Project, Task, TaskTemplate, TemplateNode,
};
use crate::error::AppResult;
use crate::services::{CreateTaskInput, UpdateProjectInput, UpdateTaskInput};
use crate::state::AppState;

const U: Actor = Actor::User;

// Projects / boards / columns

#[tauri::command]
pub async fn list_projects(s: State<'_, AppState>) -> AppResult<Vec<Project>> {
    Ok(s.projects.list_projects().await?)
}

#[tauri::command]
pub async fn create_project(
    s: State<'_, AppState>,
    name: String,
    description: Option<String>,
    color: Option<String>,
) -> AppResult<Project> {
    Ok(s.projects
        .create_project(&name, description.as_deref(), color.as_deref(), U)
        .await?)
}

#[tauri::command]
pub async fn update_project(
    s: State<'_, AppState>,
    id: String,
    input: UpdateProjectInput,
) -> AppResult<Project> {
    Ok(s.projects.update_project(&id, input, U).await?)
}

#[tauri::command]
pub async fn delete_project(s: State<'_, AppState>, id: String) -> AppResult<()> {
    Ok(s.projects.delete_project(&id, U).await?)
}

#[tauri::command]
pub async fn list_boards(s: State<'_, AppState>, project_id: String) -> AppResult<Vec<Board>> {
    Ok(s.projects.list_boards(&project_id).await?)
}

#[tauri::command]
pub async fn create_board(
    s: State<'_, AppState>,
    project_id: String,
    name: String,
) -> AppResult<Board> {
    Ok(s.projects.create_board(&project_id, &name, U).await?)
}

#[tauri::command]
pub async fn rename_board(s: State<'_, AppState>, id: String, name: String) -> AppResult<Board> {
    Ok(s.projects.rename_board(&id, &name, U).await?)
}

#[tauri::command]
pub async fn delete_board(s: State<'_, AppState>, id: String) -> AppResult<()> {
    Ok(s.projects.delete_board(&id, U).await?)
}

#[tauri::command]
pub async fn create_column(
    s: State<'_, AppState>,
    board_id: String,
    name: String,
    kind: String,
) -> AppResult<Column> {
    Ok(s.projects.create_column(&board_id, &name, &kind, U).await?)
}

#[tauri::command]
pub async fn update_column(
    s: State<'_, AppState>,
    id: String,
    name: Option<String>,
    kind: Option<String>,
) -> AppResult<Column> {
    Ok(s.projects
        .update_column(&id, name.as_deref(), kind.as_deref(), U)
        .await?)
}

#[tauri::command]
pub async fn move_column(s: State<'_, AppState>, id: String, position: i64) -> AppResult<Column> {
    Ok(s.projects.move_column(&id, position, U).await?)
}

#[tauri::command]
pub async fn delete_column(s: State<'_, AppState>, id: String) -> AppResult<()> {
    Ok(s.projects.delete_column(&id, U).await?)
}

// Tasks

#[tauri::command]
pub async fn board_view(s: State<'_, AppState>, board_id: String) -> AppResult<BoardView> {
    Ok(s.tasks.board_view(&board_id).await?)
}

#[tauri::command]
pub async fn create_task(s: State<'_, AppState>, input: CreateTaskInput) -> AppResult<Task> {
    Ok(s.tasks.create_task(input, U).await?)
}

#[tauri::command]
pub async fn update_task(
    s: State<'_, AppState>,
    id: String,
    input: UpdateTaskInput,
) -> AppResult<Task> {
    Ok(s.tasks.update_task(&id, input, U).await?)
}

#[tauri::command]
pub async fn move_task(
    s: State<'_, AppState>,
    id: String,
    column_id: String,
    position: i64,
    force: Option<bool>,
) -> AppResult<Task> {
    Ok(s.tasks
        .move_task(&id, &column_id, position, force.unwrap_or(false), U)
        .await?)
}

#[tauri::command]
pub async fn set_task_completed(
    s: State<'_, AppState>,
    id: String,
    completed: bool,
    force: Option<bool>,
) -> AppResult<Task> {
    Ok(s.tasks
        .set_completed(&id, completed, force.unwrap_or(false), U)
        .await?)
}

#[tauri::command]
pub async fn delete_task(s: State<'_, AppState>, id: String) -> AppResult<()> {
    Ok(s.tasks.delete_task(&id, U).await?)
}

#[tauri::command]
pub async fn add_dependency(
    s: State<'_, AppState>,
    task_id: String,
    blocked_by_id: String,
) -> AppResult<()> {
    Ok(s.tasks.add_dependency(&task_id, &blocked_by_id, U).await?)
}

#[tauri::command]
pub async fn remove_dependency(
    s: State<'_, AppState>,
    task_id: String,
    blocked_by_id: String,
) -> AppResult<()> {
    Ok(s.tasks
        .remove_dependency(&task_id, &blocked_by_id, U)
        .await?)
}

#[tauri::command]
pub async fn add_checklist_item(
    s: State<'_, AppState>,
    task_id: String,
    text: String,
) -> AppResult<ChecklistItem> {
    Ok(s.tasks.add_checklist_item(&task_id, &text, U).await?)
}

#[tauri::command]
pub async fn update_checklist_item(
    s: State<'_, AppState>,
    id: String,
    text: Option<String>,
    done: Option<bool>,
) -> AppResult<ChecklistItem> {
    Ok(s.tasks
        .update_checklist_item(&id, text.as_deref(), done, U)
        .await?)
}

#[tauri::command]
pub async fn delete_checklist_item(s: State<'_, AppState>, id: String) -> AppResult<()> {
    Ok(s.tasks.delete_checklist_item(&id, U).await?)
}

// Templates

#[tauri::command]
pub async fn list_templates(s: State<'_, AppState>) -> AppResult<Vec<TaskTemplate>> {
    Ok(s.templates.list().await?)
}

#[tauri::command]
pub async fn create_template(
    s: State<'_, AppState>,
    name: String,
    description: Option<String>,
    tree: Vec<TemplateNode>,
) -> AppResult<TaskTemplate> {
    Ok(s.templates
        .create(&name, description.as_deref(), tree, U)
        .await?)
}

#[tauri::command]
pub async fn delete_template(s: State<'_, AppState>, id: String) -> AppResult<()> {
    Ok(s.templates.delete(&id, U).await?)
}

#[tauri::command]
pub async fn instantiate_template(
    s: State<'_, AppState>,
    template_id: String,
    column_id: String,
    parent_id: Option<String>,
) -> AppResult<Vec<Task>> {
    Ok(s.templates
        .instantiate(&template_id, &column_id, parent_id.as_deref(), U)
        .await?)
}

#[tauri::command]
pub async fn save_task_as_template(
    s: State<'_, AppState>,
    task_id: String,
    name: String,
) -> AppResult<TaskTemplate> {
    Ok(s.templates
        .save_task_as_template(&task_id, &name, U)
        .await?)
}

#[tauri::command]
pub async fn get_task(s: State<'_, AppState>, id: String) -> AppResult<Task> {
    Ok(s.tasks.get_task(&id).await?)
}
