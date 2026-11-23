//! Desktop shell: tray, floating windows, global shortcuts, notifications and background tasks.
//!
//! Everything here is thin glue over services that are unit-tested; it can only be fully verified
//! on Windows (tray, transparency, click-through, global shortcuts, toasts, autostart).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{
    AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent,
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use tauri_plugin_notification::NotificationExt;

use crate::domain::activity::Actor;
use crate::services::Notifier;
use crate::state::AppState;

pub const WIDGET: &str = "widget";
pub const PALETTE: &str = "palette";
pub const CAPTURE: &str = "capture";

/// Default global shortcuts (overridable via the `shortcuts.*` settings). `Alt+Space` is avoided
/// on purpose: Windows and PowerToys Run use it.
pub const SHORTCUTS: [(&str, &str); 3] = [
    ("shortcuts.palette", "Ctrl+Alt+Space"),
    ("shortcuts.capture", "Ctrl+Alt+N"),
    ("shortcuts.click_through", "Ctrl+Alt+W"),
];

/// Sends desktop notifications through the notification plugin.
pub struct TauriNotifier(pub AppHandle);

impl Notifier for TauriNotifier {
    fn notify(&self, title: &str, body: &str) {
        if let Err(e) = self
            .0
            .notification()
            .builder()
            .title(title)
            .body(body)
            .show()
        {
            tracing::warn!("notification failed: {e}");
        }
    }
}

/// Which global shortcuts failed to register (shown in Settings instead of crashing).
#[derive(Default)]
pub struct ShortcutStatus(pub Mutex<Vec<String>>);

// ---------- windows ----------

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// Show the main window and ask its frontend to navigate.
pub fn open_main(app: &AppHandle, path: Option<String>) {
    show_main(app);
    if let Some(p) = path {
        let _ = app.emit_to("main", "navigate", p);
    }
}

fn floating(
    app: &AppHandle,
    label: &str,
    title: &str,
    w: f64,
    h: f64,
    route: &str,
) -> tauri::Result<WebviewWindow> {
    WebviewWindowBuilder::new(app, label, WebviewUrl::App(route.into()))
        .title(title)
        .inner_size(w, h)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        .build()
}

/// A summoned window (palette/capture): created once, then shown/hidden; hides when it loses focus.
fn summon(app: &AppHandle, label: &str, title: &str, w: f64, h: f64, route: &str) {
    let win = match app.get_webview_window(label) {
        Some(win) => win,
        None => match floating(app, label, title, w, h, route) {
            Ok(win) => {
                let _ = win.set_resizable(false);
                let _ = win.center();
                let handle = win.clone();
                win.on_window_event(move |e| {
                    if let WindowEvent::Focused(false) = e {
                        let _ = handle.hide();
                    }
                });
                win
            }
            Err(e) => return tracing::warn!("cannot create {label} window: {e}"),
        },
    };
    let _ = win.center();
    let _ = win.show();
    let _ = win.set_focus();
    let _ = win.emit("summoned", ());
}

pub fn open_palette(app: &AppHandle) {
    summon(app, PALETTE, "Orbit palette", 680.0, 440.0, "palette");
}

pub fn open_capture(app: &AppHandle) {
    summon(app, CAPTURE, "Quick capture", 520.0, 150.0, "capture");
}

pub fn hide_window(app: &AppHandle, label: &str) {
    if let Some(w) = app.get_webview_window(label) {
        let _ = w.hide();
    }
}

pub fn toggle_widget(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(WIDGET) {
        if w.is_visible().unwrap_or(false) {
            let _ = w.hide();
        } else {
            let _ = w.show();
        }
        return;
    }
    match floating(app, WIDGET, "Orbit widget", 300.0, 380.0, "widget") {
        Ok(w) => {
            let _ = w.set_resizable(true);
            let _ = w.show();
            let app = app.clone();
            // Restore saved click-through after the window exists.
            tauri::async_runtime::spawn(async move {
                if widget_config(&app)
                    .await
                    .get("clickThrough")
                    .and_then(|v| v.as_bool())
                    == Some(true)
                {
                    apply_click_through(&app, true);
                }
            });
        }
        Err(e) => tracing::warn!("cannot create widget: {e}"),
    }
}

fn apply_click_through(app: &AppHandle, on: bool) {
    if let Some(w) = app.get_webview_window(WIDGET) {
        let _ = w.set_ignore_cursor_events(on);
    }
}

async fn widget_config(app: &AppHandle) -> serde_json::Value {
    let state = app.state::<AppState>();
    match state.settings.get("widget").await {
        Ok(Some(s)) => serde_json::from_str(&s.value).unwrap_or_else(|_| serde_json::json!({})),
        _ => serde_json::json!({}),
    }
}

/// Set click-through on the widget, persist it, and tell the widget UI. This is the escape hatch
/// used by the tray menu and the global shortcut as well as the settings UI.
pub async fn set_click_through(app: &AppHandle, on: bool) {
    apply_click_through(app, on);
    let mut cfg = widget_config(app).await;
    cfg["clickThrough"] = serde_json::Value::Bool(on);
    let state = app.state::<AppState>();
    if let Err(e) = state
        .settings
        .set("widget", &cfg.to_string(), Actor::User)
        .await
    {
        tracing::warn!("cannot save widget config: {e}");
    }
    let _ = app.emit("widget-config-changed", cfg);
}

// ---------- shortcuts ----------

/// (Re)register the global shortcuts from settings. Failures are recorded, never fatal.
pub async fn apply_shortcuts(app: &AppHandle) -> Vec<String> {
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    let state = app.state::<AppState>();
    let mut failures = vec![];
    for (key, default) in SHORTCUTS {
        let combo = match state.settings.get(key).await {
            Ok(Some(s)) => {
                serde_json::from_str::<String>(&s.value).unwrap_or_else(|_| default.to_string())
            }
            _ => default.to_string(),
        };
        if combo.trim().is_empty() {
            continue; // disabled by the user
        }
        let which = key.to_string();
        let result = gs.on_shortcut(combo.as_str(), move |app, _shortcut, event| {
            if event.state != ShortcutState::Pressed {
                return;
            }
            match which.as_str() {
                "shortcuts.palette" => open_palette(app),
                "shortcuts.capture" => open_capture(app),
                _ => {
                    let app = app.clone();
                    tauri::async_runtime::spawn(async move {
                        let on = widget_config(&app)
                            .await
                            .get("clickThrough")
                            .and_then(|v| v.as_bool())
                            == Some(true);
                        set_click_through(&app, !on).await;
                    });
                }
            }
        });
        if let Err(e) = result {
            failures.push(format!("{combo}: {e}"));
        }
    }
    *app.state::<ShortcutStatus>().0.lock().unwrap() = failures.clone();
    failures
}

// ---------- tray ----------

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let menu = Menu::with_items(
        app,
        &[
            &MenuItem::with_id(app, "show", "Show Orbit", true, None::<&str>)?,
            &MenuItem::with_id(app, "capture", "Quick capture", true, None::<&str>)?,
            &MenuItem::with_id(app, "palette", "Command palette", true, None::<&str>)?,
            &MenuItem::with_id(app, "widget", "Show / hide widget", true, None::<&str>)?,
            &MenuItem::with_id(
                app,
                "click_off",
                "Disable widget click-through",
                true,
                None::<&str>,
            )?,
            &MenuItem::with_id(app, "quit", "Quit Orbit", true, None::<&str>)?,
        ],
    )?;
    let mut tray = TrayIconBuilder::with_id("orbit")
        .tooltip("Orbit")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main(app),
            "capture" => open_capture(app),
            "palette" => open_palette(app),
            "widget" => toggle_widget(app),
            "click_off" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move { set_click_through(&app, false).await });
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

// ---------- setup ----------

/// Wire up everything that needs a running app: tray, close-to-tray, shortcuts, background loops.
pub fn setup(app: &AppHandle, hidden: bool) -> tauri::Result<()> {
    app.manage(ShortcutStatus::default());
    build_tray(app)?;

    if let Some(main) = app.get_webview_window("main") {
        let w = main.clone();
        // Closing the window keeps Orbit running in the tray; "Quit" in the tray exits.
        main.on_window_event(move |e| {
            if let WindowEvent::CloseRequested { api, .. } = e {
                api.prevent_close();
                let _ = w.hide();
            }
        });
        // The window starts invisible so a `--hidden` autostart never flashes it.
        if !hidden {
            let _ = main.show();
        }
    }

    let state = app.state::<AppState>();
    // Automation engine: consumes domain events.
    tauri::async_runtime::spawn(state.engine.clone().run(state.bus.subscribe()));
    // Scheduler: timers, reminders and time-based rules. A tick after a long sleep catches up once.
    tauri::async_runtime::spawn(state.scheduler.clone().run(Duration::from_secs(20)));
    // Refresh subscribed calendars at startup and then hourly (read-only, opt-in: no-op without any).
    let external = state.external.clone();
    tauri::async_runtime::spawn(async move {
        let mut every = tokio::time::interval(Duration::from_secs(3600));
        every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            every.tick().await;
            external.sync_all().await;
        }
    });
    // Relay domain events to the UI so open windows can refresh.
    let relay_app = app.clone();
    let mut rx = state.bus.subscribe();
    tauri::async_runtime::spawn(async move {
        while let Ok(env) = rx.recv().await {
            let _ = relay_app.emit(
                "domain-event",
                format!("{:?}", env.event)
                    .split([' ', '{'])
                    .next()
                    .unwrap_or("")
                    .to_string(),
            );
        }
    });

    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        let failures = apply_shortcuts(&app2).await;
        if !failures.is_empty() {
            tracing::warn!("global shortcuts not registered: {failures:?}");
        }
    });
    Ok(())
}

pub fn shared_notifier(app: &AppHandle) -> Arc<dyn Notifier> {
    Arc::new(TauriNotifier(app.clone()))
}
