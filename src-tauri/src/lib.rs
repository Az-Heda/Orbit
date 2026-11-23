pub mod commands;
pub mod domain;
pub mod error;
pub mod infrastructure;
pub mod logging;
pub mod services;
pub mod shell;
pub mod state;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    logging::init();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--hidden"]),
        ))
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            // A staged import/restore replaces the database before anything opens it.
            match tauri::async_runtime::block_on(services::apply_pending_import(&dir)) {
                Ok(true) => tracing::info!("imported database applied"),
                Ok(false) => {}
                Err(e) => tracing::warn!("could not apply the staged import: {e}"),
            }
            let pool = tauri::async_runtime::block_on(infrastructure::database::open(
                &dir.join("orbit.db"),
            ))?;
            tracing::info!("database ready");
            let state = state::AppState::new(pool.clone(), shell::shared_notifier(app.handle()));
            let backups = std::sync::Arc::new(services::BackupService::new(
                pool,
                dir.clone(),
                state.settings.clone(),
                std::sync::Arc::new(chrono::Utc::now),
            ));
            // Automatic local backups: checked at startup and hourly, taken at most daily.
            let due = backups.clone();
            tauri::async_runtime::spawn(async move {
                let mut every = tokio::time::interval(std::time::Duration::from_secs(3600));
                every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                loop {
                    every.tick().await;
                    if let Err(e) = due.run_due().await {
                        tracing::warn!("automatic backup failed: {e}");
                    }
                }
            });
            // Self-update (plain executable): remove the previous version left by an update.
            if let Ok(exe) = std::env::current_exe() {
                services::cleanup_old_exe(&exe);
                app.manage(std::sync::Arc::new(services::UpdateService::new(
                    std::sync::Arc::new(infrastructure::updates::HttpUpdateSource::new()),
                    state.settings.clone(),
                    backups.clone(),
                    exe,
                )));
            }
            app.manage(backups);
            app.manage(state);
            let hidden = std::env::args().any(|a| a == "--hidden");
            shell::setup(app.handle(), hidden)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::settings::get_setting,
            commands::settings::list_settings,
            commands::settings::set_setting,
            commands::activity::recent_activity,
            commands::ai::ai_send_message,
            commands::ai::ai_cancel,
            commands::ai::ai_list_conversations,
            commands::ai::ai_get_conversation,
            commands::ai::ai_delete_conversation,
            commands::ai::ai_apply_proposal,
            commands::ai::ai_reject_proposal,
            commands::ai::ai_undo_proposal,
            commands::ai::ai_key_status,
            commands::ai::ai_set_key,
            commands::ai::ai_clear_key,
            commands::ai::ai_get_config,
            commands::ai::ai_set_config,
            commands::ai::ai_get_sharing,
            commands::ai::ai_set_sharing,
            commands::ai::ai_list_memories,
            commands::ai::ai_add_memory,
            commands::ai::ai_update_memory,
            commands::ai::ai_delete_memory,
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
            commands::inbox::capture_inbox,
            commands::inbox::list_inbox,
            commands::inbox::inbox_count,
            commands::inbox::triage_inbox_to_task,
            commands::inbox::triage_inbox_to_event,
            commands::inbox::dismiss_inbox,
            commands::inbox::list_rules,
            commands::inbox::create_rule,
            commands::inbox::update_rule,
            commands::inbox::set_rule_enabled,
            commands::inbox::delete_rule,
            commands::desktop::toggle_widget,
            commands::desktop::set_widget_click_through,
            commands::desktop::open_palette,
            commands::desktop::open_capture,
            commands::desktop::hide_window,
            commands::desktop::open_main,
            commands::desktop::get_autostart,
            commands::desktop::set_autostart,
            commands::desktop::shortcut_failures,
            commands::desktop::apply_shortcuts,
            commands::kanban::get_task,
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
            commands::calendar::list_external_calendars,
            commands::calendar::add_external_calendar,
            commands::calendar::remove_external_calendar,
            commands::calendar::sync_external_calendar,
            commands::data::backup_status,
            commands::data::backup_list,
            commands::data::backup_now,
            commands::data::backup_export,
            commands::data::backup_import,
            commands::data::backup_restore,
            commands::data::backup_cancel_import,
            commands::data::restart_app,
            commands::data::update_check,
            commands::data::update_skip,
            commands::data::update_install,
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
        .unwrap_or_else(|e| {
            // Reaches the crash log through the panic hook (the release build has no console).
            panic!("error while running Orbit: {e}")
        });
}
