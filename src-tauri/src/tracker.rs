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

/// Detect the active window on Linux, handling both X11 and Wayland sessions.
///
/// Session detection:
///   - X11  (`XDG_SESSION_TYPE=x11` or no WAYLAND_DISPLAY): persistent x11rb connection
///   - Wayland + GNOME (`XDG_CURRENT_DESKTOP=GNOME`): gdbus call to GNOME Shell
///   - Wayland + KDE:  xdotool via XWayland (best-effort)
///
/// Why X11 alone fails on Wayland:
///   `_NET_ACTIVE_WINDOW` on the XWayland root is only updated when X11 apps get
///   focus. Native Wayland apps (Firefox, Files, Terminal) never trigger it, so the
///   property stays stuck on the last focused X11 app — making the tracker appear
///   to show only one app with endlessly growing time.
#[cfg(target_os = "linux")]
pub fn get_active_window_info() -> Option<(String, String)> {
    let is_wayland = std::env::var("WAYLAND_DISPLAY").is_ok()
        || std::env::var("XDG_SESSION_TYPE")
            .map(|v| v.to_lowercase() == "wayland")
            .unwrap_or(false);

    let result = if is_wayland {
        get_active_window_wayland()
    } else {
        get_active_window_x11()
    };

    // Never track the Digital Wellbeing app itself — it would dominate the list
    // whenever the user is looking at the dashboard.
    if let Some((ref exe, _)) = result {
        let lower = exe.to_lowercase();
        if lower.contains("digital-wellbeing") || lower.contains("digitalwellbeing") {
            return None;
        }
    }
    result
}

/// Wayland: query the focused window from the desktop environment.
#[cfg(target_os = "linux")]
fn get_active_window_wayland() -> Option<(String, String)> {
    // Hyprland: check env var that is always set inside a Hyprland session
    if std::env::var("HYPRLAND_INSTANCE_SIGNATURE").is_ok() {
        return get_active_window_hyprland();
    }

    let desktop = std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .to_uppercase();

    if desktop.contains("GNOME") || desktop.contains("UNITY") {
        get_active_window_gnome_shell()
    } else if desktop.contains("KDE") || desktop.contains("PLASMA") {
        // KDE Wayland: xdotool via XWayland bridge (best-effort)
        get_active_window_xdotool()
    } else {
        // Generic Wayland: best-effort via xdotool for X11 apps under XWayland
        get_active_window_xdotool()
    }
}

/// Hyprland: use `hyprctl activewindow -j` — native Hyprland IPC.
/// Returns WM class + title with zero X11 assumptions. The `class` field
/// is the app identifier (e.g. "firefox", "code", "kitty"), and `pid`
/// lets us resolve the exact binary name from /proc.
#[cfg(target_os = "linux")]
fn get_active_window_hyprland() -> Option<(String, String)> {
    use std::process::Command;

    let out = Command::new("hyprctl")
        .args(["activewindow", "-j"])
        .output()
        .ok()?;

    if !out.status.success() {
        return None;
    }

    // Parse JSON using serde_json (already a dependency)
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;

    let class = v["class"].as_str().unwrap_or("").trim().to_string();
    let title = v["title"].as_str().unwrap_or("").trim().to_string();

    if class.is_empty() {
        return None;
    }

    // If hyprctl gives us the PID, try to get a more precise binary name
    // (e.g. "python3" instead of "code" for VS Code's extension host)
    // but for most apps the `class` field is exactly what we want.
    let exe_name = if let Some(pid) = v["pid"].as_u64() {
        std::fs::read_link(format!("/proc/{}/exe", pid))
            .ok()
            .and_then(|p| p.file_name().and_then(|n| n.to_str()).map(|s| s.to_string()))
            .unwrap_or(class)
    } else {
        class
    };

    Some((exe_name, title))
}

/// GNOME Wayland: use the GNOME Shell D-Bus scripting API to get the focused
/// window's WM class and title.  This is the only reliable method on Wayland for
/// native GTK/GNOME apps.
#[cfg(target_os = "linux")]
fn get_active_window_gnome_shell() -> Option<(String, String)> {
    use std::process::Command;

    // Script returns "wm_class|title" or "" if no focused window
    let script = r#"(function(){
        var win = global.display.get_focus_window();
        if(!win) return '';
        return (win.wm_class||'') + '|' + (win.title||'');
    })()"#;

    let out = Command::new("gdbus")
        .args([
            "call", "--session",
            "--dest", "org.gnome.Shell",
            "--object-path", "/org/gnome/Shell",
            "--method", "org.gnome.Shell.Eval",
            script,
        ])
        .output()
        .ok()?;

    if !out.status.success() {
        return None;
    }

    // gdbus prints: (true, 'wm_class|title',)
    let raw = String::from_utf8_lossy(&out.stdout);
    // Extract the inner string between the first and last quote
    let inner = raw
        .split('\'')
        .nth(1)
        .unwrap_or("")
        .trim();

    if inner.is_empty() {
        return None;
    }

    let mut parts = inner.splitn(2, '|');
    let wm_class = parts.next().unwrap_or("").trim().to_lowercase();
    let title    = parts.next().unwrap_or("").trim().to_string();

    if wm_class.is_empty() {
        return None;
    }

    Some((wm_class, title))
}

/// X11 / XWayland fallback via xdotool (one subprocess per tick — acceptable
/// since we only call this on Wayland KDE/generic where no better API exists).
#[cfg(target_os = "linux")]
fn get_active_window_xdotool() -> Option<(String, String)> {
    use std::process::Command;

    // Single xdotool call that chains window-id → name
    let name_out = Command::new("xdotool")
        .args(["getactivewindow", "getwindowname"])
        .output()
        .ok()?;
    if !name_out.status.success() {
        return None;
    }
    let title = String::from_utf8_lossy(&name_out.stdout).trim().to_string();

    // Try to get PID and resolve comm; fall back to getwindowclassname
    let pid_out = Command::new("xdotool")
        .args(["getactivewindow", "getwindowpid"])
        .output();

    let exe_name = pid_out
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<u32>().ok())
        .and_then(|pid| {
            std::fs::read_to_string(format!("/proc/{}/comm", pid))
                .map(|s| s.trim().to_string())
                .ok()
        })
        .or_else(|| {
            Command::new("xdotool")
                .args(["getactivewindow", "getwindowclassname"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .filter(|s| !s.is_empty())
        })?;

    if exe_name.is_empty() || title.is_empty() {
        return None;
    }
    Some((exe_name, title))
}

/// X11: persistent connection via x11rb — zero subprocess overhead.
#[cfg(target_os = "linux")]
fn get_active_window_x11() -> Option<(String, String)> {
    use once_cell::sync::Lazy;
    use std::sync::Mutex;
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt};
    use x11rb::rust_connection::RustConnection;

    struct X11State {
        conn: RustConnection,
        root: u32,
        net_active_window: u32,
        net_wm_name: u32,
        net_wm_pid: u32,
        wm_name: u32,
        wm_class: u32,
        utf8_string: u32,
    }

    static X11: Lazy<Mutex<Option<X11State>>> = Lazy::new(|| {
        let state = (|| -> Option<X11State> {
            let (conn, screen_num) = RustConnection::connect(None).ok()?;
            let root = conn.setup().roots[screen_num].root;
            let intern = |name: &str| -> Option<u32> {
                conn.intern_atom(false, name.as_bytes()).ok()?.reply().ok().map(|r| r.atom)
            };
            Some(X11State {
                net_active_window: intern("_NET_ACTIVE_WINDOW")?,
                net_wm_name:       intern("_NET_WM_NAME")?,
                net_wm_pid:        intern("_NET_WM_PID")?,
                wm_name:           intern("WM_NAME")?,
                wm_class:          intern("WM_CLASS")?,
                utf8_string:       intern("UTF8_STRING")?,
                root,
                conn,
            })
        })();
        Mutex::new(state)
    });

    let mut guard = X11.lock().ok()?;
    let x11 = guard.as_mut()?;

    // Active window ID
    let aw = x11.conn
        .get_property(false, x11.root, x11.net_active_window, AtomEnum::WINDOW, 0, 1)
        .ok()?.reply().ok()?;
    let window: u32 = aw.value32()?.next()?;
    if window == 0 { return None; }

    // Window title
    let title = {
        let r = x11.conn
            .get_property(false, window, x11.net_wm_name, x11.utf8_string, 0, 512)
            .ok()?.reply().ok()?;
        if !r.value.is_empty() {
            String::from_utf8_lossy(&r.value).into_owned()
        } else {
            let r2 = x11.conn
                .get_property(false, window, x11.wm_name, AtomEnum::STRING, 0, 512)
                .ok()?.reply().ok()?;
            String::from_utf8_lossy(&r2.value).into_owned()
        }
    };
    if title.is_empty() { return None; }

    // PID → /proc/comm (fast path)
    let pid: Option<u32> = x11.conn
        .get_property(false, window, x11.net_wm_pid, AtomEnum::CARDINAL, 0, 1)
        .ok().and_then(|c| c.reply().ok()).and_then(|r| r.value32()?.next());

    let exe_name = if let Some(pid) = pid {
        std::fs::read_to_string(format!("/proc/{}/comm", pid))
            .map(|s| s.trim().to_string()).ok()
            .or_else(|| {
                std::fs::read_link(format!("/proc/{}/exe", pid)).ok()
                    .and_then(|p| p.file_name().and_then(|n| n.to_str()).map(|s| s.to_string()))
            })
    } else { None }
    .or_else(|| {
        x11.conn.get_property(false, window, x11.wm_class, AtomEnum::STRING, 0, 256)
            .ok().and_then(|c| c.reply().ok())
            .filter(|r| !r.value.is_empty())
            .and_then(|r| {
                String::from_utf8_lossy(&r.value).split('\0')
                    .next().map(|s| s.to_string()).filter(|s| !s.is_empty())
            })
    })
    .unwrap_or_else(|| "unknown".to_string());

    if exe_name.is_empty() { return None; }
    Some((exe_name, title))
}

/// macOS / other non-Windows, non-Linux: not yet implemented.
#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub fn get_active_window_info() -> Option<(String, String)> {
    None
}



// Write the accumulated tracking block to the DB
async fn persist_activity(pool: &SqlitePool, app_info: &ActiveAppInfo) -> Result<(), sqlx::Error> {
    if app_info.duration_seconds == 0 {
        return Ok(());
    }

    // 1. Get or create app ID in SQLite
    #[cfg(target_os = "windows")]
    let display_name = app_info.executable_name
        .strip_suffix(".exe")
        .unwrap_or(&app_info.executable_name)
        .to_string();
    #[cfg(not(target_os = "windows"))]
    let display_name = app_info.executable_name.clone();

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

            // ── Snapshot settings with a BRIEF lock, then release immediately ──
            // This prevents the long mutex hold that caused UI freezes when
            // commands (e.g. get_top_apps on scroll) tried to acquire the lock
            // while we were inside blocking X11 calls.
            let (is_tracking, is_idle_monitoring) = {
                let t = state.lock().await;
                (t.is_tracking, t.is_idle_monitoring)
            };

            if !is_tracking {
                continue;
            }

            // ── All blocking work runs in spawn_blocking (thread-pool), NOT in
            //    the async Tokio worker thread. This is what eliminates the freeze:
            //    Tokio workers are free to process UI commands while X11/idle
            //    queries happen concurrently in a dedicated blocking thread.

            let window_info = tokio::task::spawn_blocking(get_active_window_info)
                .await
                .unwrap_or(None);

            let is_idle = if is_idle_monitoring {
                tokio::task::spawn_blocking(|| is_user_idle(60))
                    .await
                    .unwrap_or(false)
            } else {
                false
            };

            // ── Re-acquire the mutex for the fast, non-blocking state update ──
            let mut tracker = state.lock().await;

            if is_idle {
                if let Some(active) = tracker.current_app.take() {
                    let pool = tracker.db_pool.clone();
                    tauri::async_runtime::spawn(async move {
                        let _ = persist_activity(&pool, &active).await;
                    });
                }
                continue;
            }

            match window_info {
                Some((exe, title)) => {
                    let mut should_persist = false;
                    let mut old_app = None;

                    if let Some(ref mut current) = tracker.current_app {
                        if current.executable_name == exe {
                            // Same application – accumulate time.
                            // Only update the stored title (for granular DB records)
                            // but do NOT treat a title change as an app switch.
                            // On Linux, titles change constantly (browser tabs,
                            // editor filenames) so title-based switching caused every
                            // session to be 1 second long.
                            current.window_title = title;
                            current.duration_seconds += 1;

                            // Persist periodically to limit data loss on crashes
                            if current.duration_seconds >= 15 {
                                should_persist = true;
                            }
                        } else {
                            // Application actually changed → flush old session
                            old_app = Some(current.clone());
                            *current = ActiveAppInfo {
                                executable_name: exe,
                                window_title: title,
                                start_time: Local::now(),
                                duration_seconds: 1,
                            };
                        }
                    } else {
                        // First window seen after startup / idle period
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
                    // No active window (locked screen / desktop focus) → flush
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

