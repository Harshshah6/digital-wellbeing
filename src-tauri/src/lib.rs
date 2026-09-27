use std::sync::Arc;
use std::sync::atomic::Ordering;
use tauri::Manager;
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::menu::{Menu, MenuItem};
use tokio::sync::Mutex;

mod db;
mod idle;
mod tracker;
mod commands;
mod api_server;

use tracker::TrackerState;

/// Check if the app was launched with --background flag (autostart scenario)
fn is_background_launch() -> bool {
    std::env::args().any(|a| a == "--background")
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let start_hidden = is_background_launch();

    // Initialize background-mode state. It will be loaded from DB in setup block.
    commands::BACKGROUND_ENABLED.store(true, Ordering::Relaxed);

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .setup(move |app| {
            // ─── Database & Tracker ───
            let app_data_dir = app.path().app_local_data_dir()
                .expect("Failed to resolve app local data directory");

            let pool = tauri::async_runtime::block_on(async {
                db::init_db(app_data_dir).await.expect("Failed to initialize SQLite database")
            });

            // Handle autostart by default on startup
            let autostart_val: Option<String> = tauri::async_runtime::block_on(async {
                let row: Option<(String,)> = sqlx::query_as("SELECT value FROM settings WHERE key = 'autostart'")
                    .fetch_optional(&pool)
                    .await
                    .unwrap_or(None);
                row.map(|r| r.0)
            });

            let autostart_enabled = match autostart_val {
                Some(val) => val == "true",
                None => {
                    // First run: save setting as true in database and register in system
                    let _ = commands::autostart_set(true);
                    let _ = tauri::async_runtime::block_on(async {
                        sqlx::query("INSERT INTO settings (key, value) VALUES ('autostart', 'true')")
                            .execute(&pool)
                            .await
                    });
                    true
                }
            };

            // Sync with current status
            if autostart_enabled {
                let _ = commands::autostart_set(true);
            } else {
                let _ = commands::autostart_set(false);
            }
            commands::BACKGROUND_ENABLED.store(autostart_enabled, Ordering::Relaxed);

            let tracker_state = Arc::new(Mutex::new(TrackerState::new(pool.clone())));
            app.manage(tracker_state.clone());
            tracker::start_tracker_loop(tracker_state.clone());

            // ─── Enterprise: Start HTTP REST API + UDP Beacon ───
            api_server::start_enterprise_services(tracker_state, pool);

            // ─── System Tray ───
            let show_item = MenuItem::with_id(app, "show", "Show Digital Wellbeing", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit Agent", true, None::<&str>)?;
            let tray_menu = Menu::with_items(app, &[&show_item, &quit_item])?;

            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("Digital Wellbeing Agent")
                .menu(&tray_menu)
                .on_menu_event(|app, event| {
                    match event.id.as_ref() {
                        "show" => {
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.unminimize();
                                let _ = window.set_focus();
                            }
                        }
                        "quit" => {
                            app.exit(0);
                        }
                        _ => {}
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.unminimize();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app)?;

            // ─── Window Visibility ───
            if let Some(window) = app.get_webview_window("main") {
                if start_hidden {
                    let _ = window.hide();
                } else {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            // Intercept close: hide to tray instead of quitting
            // Reads cached AtomicBool — zero subprocess overhead, no freeze.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if commands::BACKGROUND_ENABLED.load(Ordering::Relaxed) {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_top_apps,
            commands::get_category_distribution,
            commands::get_hourly_timeline,
            commands::update_app_details,
            commands::create_goal,
            commands::get_goals,
            commands::start_focus_session,
            commands::end_focus_session,
            commands::get_focus_sessions,
            commands::get_heatmap_data,
            commands::get_autostart_enabled,
            commands::set_autostart_enabled,
            commands::export_data,
            commands::get_idle_monitoring,
            commands::set_idle_monitoring,
            commands::get_yearly_data,
            commands::get_avg_screen_time,
            commands::backup_database,
            commands::restore_database
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
