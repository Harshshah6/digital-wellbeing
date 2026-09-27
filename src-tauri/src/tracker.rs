use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use chrono::{Local, DateTime};
use sqlx::{SqlitePool, Row};
use tokio::sync::Mutex;
use crate::idle::is_user_idle;

#[derive(Debug, Clone)]
pub struct ActiveAppInfo {
    pub executable_name: String,
    pub window_title: String,
    pub start_time: DateTime<Local>,
    pub duration_seconds: u64,
}

pub struct TrackerState {
    pub current_app: Option<ActiveAppInfo>,
    pub db_pool: SqlitePool,
    pub is_tracking: bool,
    pub is_idle_monitoring: bool,
}

impl TrackerState {
    pub fn new(db_pool: SqlitePool) -> Self {
        let is_idle_monitoring = tauri::async_runtime::block_on(async {
            sqlx::query("SELECT value FROM settings WHERE key = 'idle_monitoring'")
                .fetch_one(&db_pool)
                .await
                .map(|r| r.try_get::<String, _>("value").unwrap_or_else(|_| "false".to_string()) == "true")
                .unwrap_or(false)
        });

        Self {
            current_app: None,
            db_pool,
            is_tracking: true,
            is_idle_monitoring,
        }
    }
}


#[cfg(target_os = "windows")]
pub fn get_active_window_info() -> Option<(String, String)> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId};
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW};
    use windows_sys::Win32::Foundation::MAX_PATH;

    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd == 0 {
            return None;
        }

        // 1. Get window title
        let mut title_buf = [0u16; 512];
        let len = GetWindowTextW(hwnd, title_buf.as_mut_ptr(), title_buf.len() as i32);
        let title = if len > 0 {
            String::from_utf16_lossy(&title_buf[..len as usize])
        } else {
            String::new()
        };

        // 2. Get process ID
        let mut process_id: u32 = 0;
        GetWindowThreadProcessId(hwnd, &mut process_id);
        if process_id == 0 {
            return None;
        }

        // 3. Get executable name
        let process_handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id);
        if process_handle == 0 {
            return None;
        }

        let mut path_buf = [0u16; MAX_PATH as usize];
        let mut size = path_buf.len() as u32;
        let success = QueryFullProcessImageNameW(process_handle, 0, path_buf.as_mut_ptr(), &mut size);
        
        // Close handle
        extern "system" {
            fn CloseHandle(handle: isize) -> i32;
        }
        CloseHandle(process_handle);

        if success != 0 {
            let full_path = String::from_utf16_lossy(&path_buf[..size as usize]);
            let exe_name = Path::new(&full_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("Unknown")
                .to_string();
            Some((exe_name, title))
        } else {
            None
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub fn get_active_window_info() -> Option<(String, String)> {
    // DigitalWellbeing is Windows-only
    None
}

// Write the accumulated tracking block to the DB
async fn persist_activity(pool: &SqlitePool, app_info: &ActiveAppInfo) -> Result<(), sqlx::Error> {
    if app_info.duration_seconds == 0 {
        return Ok(());
    }

    // 1. Get or create app ID in SQLite
    let display_name = app_info.executable_name
        .strip_suffix(".exe")
        .unwrap_or(&app_info.executable_name)
        .to_string();

    let mut transaction = pool.begin().await?;

    // Insert app if missing
    sqlx::query(
        "INSERT INTO apps (executable_name, display_name) 
         VALUES (?, ?) 
         ON CONFLICT(executable_name) DO UPDATE SET executable_name=executable_name"
    )
    .bind(&app_info.executable_name)
    .bind(&display_name)
    .execute(&mut *transaction)
    .await?;

    let app_row: (i64,) = sqlx::query_as(
        "SELECT id FROM apps WHERE executable_name = ?"
    )
    .bind(&app_info.executable_name)
    .fetch_one(&mut *transaction)
    .await?;
    
    let app_id = app_row.0;

    // 2. Insert the tracking chunk
    let end_time = app_info.start_time + chrono::Duration::seconds(app_info.duration_seconds as i64);
    sqlx::query(
        "INSERT INTO activities (app_id, window_title, start_time, end_time, duration_seconds) 
         VALUES (?, ?, ?, ?, ?)"
    )
    .bind(app_id)
    .bind(&app_info.window_title)
    .bind(app_info.start_time.to_rfc3339())
    .bind(end_time.to_rfc3339())
    .bind(app_info.duration_seconds as i64)
    .execute(&mut *transaction)
    .await?;

    // 3. Update daily summary
    let today = Local::now().format("%Y-%m-%d").to_string();
    sqlx::query(
        "INSERT INTO daily_summaries (date, total_screen_time) 
         VALUES (?, ?) 
         ON CONFLICT(date) DO UPDATE SET total_screen_time = total_screen_time + ?"
    )
    .bind(&today)
    .bind(app_info.duration_seconds as i64)
    .bind(app_info.duration_seconds as i64)
    .execute(&mut *transaction)
    .await?;

    transaction.commit().await?;
    Ok(())
}

pub fn start_tracker_loop(state: Arc<Mutex<TrackerState>>) {
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        
        loop {
            interval.tick().await;

            let mut tracker = state.lock().await;
            if !tracker.is_tracking {
                continue;
            }

            // check user idle status (e.g. 60 seconds threshold)
            if tracker.is_idle_monitoring && is_user_idle(60) {
                // If user becomes idle, persist whatever they were doing and clear active state
                if let Some(active) = tracker.current_app.take() {
                    let _ = persist_activity(&tracker.db_pool, &active).await;
                }
                continue;
            }

            match get_active_window_info() {
                Some((exe, title)) => {
                    let mut should_persist = false;
                    let mut old_app = None;

                    if let Some(ref mut current) = tracker.current_app {
                        if current.executable_name == exe && current.window_title == title {
                            current.duration_seconds += 1;
                            
                            // To prevent long loss of data on crashes, persist periodically (every 15 seconds)
                            if current.duration_seconds >= 15 {
                                should_persist = true;
                            }
                        } else {
                            // Active window has changed!
                            old_app = Some(current.clone());
                            
                            // Set new active app
                            *current = ActiveAppInfo {
                                executable_name: exe,
                                window_title: title,
                                start_time: Local::now(),
                                duration_seconds: 1,
                            };
                        }
                    } else {
                        // Start tracking first app
                        tracker.current_app = Some(ActiveAppInfo {
                            executable_name: exe,
                            window_title: title,
                            start_time: Local::now(),
                            duration_seconds: 1,
                        });
                    }

                    if let Some(old) = old_app {
                        let pool = tracker.db_pool.clone();
                        tauri::async_runtime::spawn(async move {
                            let _ = persist_activity(&pool, &old).await;
                        });
                    } else if should_persist {
                        if let Some(ref mut current) = tracker.current_app {
                            let old_copy = current.clone();
                            current.duration_seconds = 0;
                            current.start_time = Local::now();
                            let pool = tracker.db_pool.clone();
                            tauri::async_runtime::spawn(async move {
                                let _ = persist_activity(&pool, &old_copy).await;
                            });
                        }
                    }
                }
                None => {
                    // No active window (e.g. locked screen or empty space), clear current tracking
                    if let Some(active) = tracker.current_app.take() {
                        let pool = tracker.db_pool.clone();
                        tauri::async_runtime::spawn(async move {
                            let _ = persist_activity(&pool, &active).await;
                        });
                    }
                }
            }
        }
    });
}
