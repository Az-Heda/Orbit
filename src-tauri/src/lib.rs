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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
