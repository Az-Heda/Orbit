pub mod commands;
pub mod domain;
pub mod error;
pub mod infrastructure;
pub mod logging;
pub mod services;
pub mod state;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    logging::init();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let pool = tauri::async_runtime::block_on(infrastructure::database::open(
                &dir.join("orbit.db"),
            ))?;
            tracing::info!("database ready");
            app.manage(state::AppState::new(pool));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::settings::get_setting,
            commands::settings::list_settings,
            commands::settings::set_setting,
            commands::activity::recent_activity,
            commands::calendar::list_calendar_range,
            commands::notes::create_note,
            commands::notes::get_note,
            commands::notes::update_note,
            commands::notes::delete_note,
            commands::notes::list_notes,
            commands::notes::open_journal,
            commands::notes::add_note_link,
            commands::notes::remove_note_link,
            commands::notes::search_all,
            commands::timers::start_timer,
            commands::timers::pause_timer,
            commands::timers::resume_timer,
            commands::timers::stop_timer,
            commands::timers::active_timers,
            commands::timers::timer_history,
            commands::timers::list_focus_sessions,
            commands::timers::set_session_task,
            commands::timers::get_stats,
            commands::calendar::get_event,
            commands::calendar::create_event,
            commands::calendar::update_event,
            commands::calendar::delete_event,
            commands::calendar::skip_occurrence,
            commands::calendar::override_occurrence,
            commands::calendar::restore_occurrence,
            commands::calendar::set_task_recurrence,
            commands::calendar::set_task_occurrence,
            commands::calendar::export_ics_file,
            commands::calendar::import_ics_file,
            commands::kanban::list_projects,
            commands::kanban::create_project,
            commands::kanban::update_project,
            commands::kanban::delete_project,
            commands::kanban::list_boards,
            commands::kanban::create_board,
            commands::kanban::rename_board,
            commands::kanban::delete_board,
            commands::kanban::create_column,
            commands::kanban::update_column,
            commands::kanban::move_column,
            commands::kanban::delete_column,
            commands::kanban::board_view,
            commands::kanban::create_task,
            commands::kanban::update_task,
            commands::kanban::move_task,
            commands::kanban::set_task_completed,
            commands::kanban::delete_task,
            commands::kanban::add_dependency,
            commands::kanban::remove_dependency,
            commands::kanban::add_checklist_item,
            commands::kanban::update_checklist_item,
            commands::kanban::delete_checklist_item,
            commands::kanban::list_templates,
            commands::kanban::create_template,
            commands::kanban::delete_template,
            commands::kanban::instantiate_template,
            commands::kanban::save_task_as_template,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
