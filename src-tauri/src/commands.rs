use std::sync::Arc;
use serde::{Serialize, Deserialize};
use sqlx::Row;
use tokio::sync::Mutex;
use chrono::Local;
use crate::tracker::TrackerState;
use tauri::{AppHandle, Manager};

#[derive(Serialize, Deserialize, Debug)]
pub struct AppUsage {
    pub display_name: String,
    pub executable_name: String,
    pub category: String,
    pub total_seconds: i64,
    pub productivity_score: i32,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CategorySummary {
    pub category: String,
    pub total_seconds: i64,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TimelineSegment {
    pub hour: i32,
    pub total_seconds: i64,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct HeatmapDataPoint {
    pub date: String,
    pub count: i64, // total seconds tracked
}

#[derive(Serialize, Deserialize, Debug)]
pub struct YearlyDataPoint {
    pub month: String,       // "YYYY-MM"
    pub month_label: String, // "Jan", "Feb", ...
    pub total_minutes: i64,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AvgScreenTimeStats {
    pub daily_avg_seconds: i64,
    pub weekly_avg_seconds: i64,
    pub monthly_avg_seconds: i64,
    pub yearly_avg_seconds: i64,
    pub tracked_days: i64,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct FocusSessionInfo {
    pub id: i64,
    pub start_time: String,
    pub end_time: Option<String>,
    pub target_duration_seconds: i64,
    pub actual_duration_seconds: Option<i64>,
    pub completed: bool,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GoalInfo {
    pub id: i64,
    pub app_id: Option<i64>,
    pub executable_name: Option<String>,
    pub category: Option<String>,
    pub duration_limit_seconds: i64,
    pub period: String,
    pub is_active: bool,
    pub current_usage_seconds: i64,
}

// 1. Get Top Apps
#[tauri::command]
pub async fn get_top_apps(
    state: tauri::State<'_, Arc<Mutex<TrackerState>>>,
    date_str: String,
    limit: i64,
) -> Result<Vec<AppUsage>, String> {
    let tracker = state.lock().await;
    let pool = &tracker.db_pool;

    let rows = sqlx::query(
        "SELECT a.display_name, a.executable_name, a.category, SUM(act.duration_seconds) as total_seconds, a.productivity_score 
         FROM apps a 
         JOIN activities act ON a.id = act.app_id 
         WHERE substr(act.start_time, 1, 10) = ?
         GROUP BY a.id 
         ORDER BY total_seconds DESC 
         LIMIT ?"
    )
    .bind(&date_str)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    let usages = rows.into_iter().map(|r| AppUsage {
        display_name: r.get("display_name"),
        executable_name: r.get("executable_name"),
        category: r.get("category"),
        total_seconds: r.get("total_seconds"),
        productivity_score: r.get("productivity_score"),
    }).collect();

    Ok(usages)
}

// 2. Get Category Distribution
#[tauri::command]
pub async fn get_category_distribution(
    state: tauri::State<'_, Arc<Mutex<TrackerState>>>,
    date_str: String,
) -> Result<Vec<CategorySummary>, String> {
    let tracker = state.lock().await;
    let pool = &tracker.db_pool;

    let rows = sqlx::query(
        "SELECT a.category, SUM(act.duration_seconds) as total_seconds 
         FROM apps a 
         JOIN activities act ON a.id = act.app_id 
         WHERE substr(act.start_time, 1, 10) = ?
         GROUP BY a.category 
         ORDER BY total_seconds DESC"
    )
    .bind(&date_str)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    let summaries = rows.into_iter().map(|r| CategorySummary {
        category: r.get("category"),
        total_seconds: r.get("total_seconds"),
    }).collect();

    Ok(summaries)
}

// 3. Get Hourly Timeline for charts
#[tauri::command]
pub async fn get_hourly_timeline(
    state: tauri::State<'_, Arc<Mutex<TrackerState>>>,
    date_str: String,
) -> Result<Vec<TimelineSegment>, String> {
    let tracker = state.lock().await;
    let pool = &tracker.db_pool;

    let rows = sqlx::query(
        "SELECT CAST(substr(act.start_time, 12, 2) AS INTEGER) as hour, SUM(act.duration_seconds) as total_seconds 
         FROM activities act 
         WHERE substr(act.start_time, 1, 10) = ? 
         GROUP BY hour
         ORDER BY hour"
    )
    .bind(&date_str)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    // Create a dense list for all 24 hours
    let mut timeline = vec![0i64; 24];
    for r in rows {
        let hr: i32 = r.get("hour");
        let secs: i64 = r.get("total_seconds");
        if hr >= 0 && hr < 24 {
            timeline[hr as usize] = secs;
        }
    }

    let result = timeline.into_iter().enumerate().map(|(h, s)| TimelineSegment {
        hour: h as i32,
        total_seconds: s,
    }).collect();

    Ok(result)
}

// 4. Update an app's category and productivity rating
#[tauri::command]
pub async fn update_app_details(
    state: tauri::State<'_, Arc<Mutex<TrackerState>>>,
    executable_name: String,
    category: String,
    productivity_score: i32,
) -> Result<(), String> {
    let tracker = state.lock().await;
    let pool = &tracker.db_pool;

    sqlx::query(
        "UPDATE apps 
         SET category = ?, productivity_score = ? 
         WHERE executable_name = ?"
    )
    .bind(category)
    .bind(productivity_score)
    .bind(executable_name)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

// 5. Create a new screen time goal
#[tauri::command]
pub async fn create_goal(
    state: tauri::State<'_, Arc<Mutex<TrackerState>>>,
    app_id: Option<i64>,
    category: Option<String>,
    limit_seconds: i64,
    period: String,
) -> Result<(), String> {
    let tracker = state.lock().await;
    let pool = &tracker.db_pool;

    sqlx::query(
        "INSERT INTO goals (app_id, category, duration_limit_seconds, period) 
         VALUES (?, ?, ?, ?)"
    )
    .bind(app_id)
    .bind(category)
    .bind(limit_seconds)
    .bind(period)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

// 6. Fetch goals alongside their live tracking progress
#[tauri::command]
pub async fn get_goals(
    state: tauri::State<'_, Arc<Mutex<TrackerState>>>,
    date_str: String,
) -> Result<Vec<GoalInfo>, String> {
    let tracker = state.lock().await;
    let pool = &tracker.db_pool;

    let rows = sqlx::query(
        "SELECT g.id, g.app_id, a.executable_name, g.category, g.duration_limit_seconds, g.period, g.is_active 
         FROM goals g
         LEFT JOIN apps a ON g.app_id = a.id"
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    let mut goals_list = Vec::new();

    for r in rows {
        let id: i64 = r.get("id");
        let app_id: Option<i64> = r.get("app_id");
        let executable_name: Option<String> = r.get("executable_name");
        let category: Option<String> = r.get("category");
        let duration_limit_seconds: i64 = r.get("duration_limit_seconds");
        let period: String = r.get("period");
        let is_active: i32 = r.get("is_active");

        // Compute current usage for the goal period
        let mut current_usage_seconds = 0i64;
        if let Some(ref exe) = executable_name {
            let usage_row = sqlx::query(
                "SELECT SUM(act.duration_seconds) as total 
                 FROM activities act 
                 JOIN apps a ON act.app_id = a.id 
                 WHERE a.executable_name = ? AND substr(act.start_time, 1, 10) = ?"
            )
            .bind(exe)
            .bind(&date_str)
            .fetch_one(pool)
            .await;

            if let Ok(row) = usage_row {
                current_usage_seconds = row.try_get::<i64, _>("total").unwrap_or(0);
            }
        } else if let Some(ref cat) = category {
            let usage_row = sqlx::query(
                "SELECT SUM(act.duration_seconds) as total 
                 FROM activities act 
                 JOIN apps a ON act.app_id = a.id 
                 WHERE a.category = ? AND substr(act.start_time, 1, 10) = ?"
            )
            .bind(cat)
            .bind(&date_str)
            .fetch_one(pool)
            .await;

            if let Ok(row) = usage_row {
                current_usage_seconds = row.try_get::<i64, _>("total").unwrap_or(0);
            }
        }

        goals_list.push(GoalInfo {
            id,
            app_id,
            executable_name,
            category,
            duration_limit_seconds,
            period,
            is_active: is_active != 0,
            current_usage_seconds,
        });
    }

    Ok(goals_list)
}

// 7. Focus sessions management
#[tauri::command]
pub async fn start_focus_session(
    state: tauri::State<'_, Arc<Mutex<TrackerState>>>,
    target_seconds: i64,
) -> Result<i64, String> {
    let tracker = state.lock().await;
    let pool = &tracker.db_pool;
    let now = Local::now().to_rfc3339();

    let result = sqlx::query(
        "INSERT INTO focus_sessions (start_time, target_duration_seconds, completed) 
         VALUES (?, ?, 0)"
    )
    .bind(&now)
    .bind(target_seconds)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(result.last_insert_rowid())
}

#[tauri::command]
pub async fn end_focus_session(
    state: tauri::State<'_, Arc<Mutex<TrackerState>>>,
    session_id: i64,
    actual_seconds: i64,
    completed: bool,
) -> Result<(), String> {
    let tracker = state.lock().await;
    let pool = &tracker.db_pool;
    let now = Local::now().to_rfc3339();
    let completed_flag = if completed { 1 } else { 0 };

    sqlx::query(
        "UPDATE focus_sessions 
         SET end_time = ?, actual_duration_seconds = ?, completed = ? 
         WHERE id = ?"
    )
    .bind(&now)
    .bind(actual_seconds)
    .bind(completed_flag)
    .bind(session_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
pub async fn get_focus_sessions(
    state: tauri::State<'_, Arc<Mutex<TrackerState>>>,
) -> Result<Vec<FocusSessionInfo>, String> {
    let tracker = state.lock().await;
    let pool = &tracker.db_pool;

    let rows = sqlx::query(
        "SELECT id, start_time, end_time, target_duration_seconds, actual_duration_seconds, completed 
         FROM focus_sessions 
         ORDER BY id DESC"
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    let sessions = rows.into_iter().map(|r| {
        let completed_val: i32 = r.get("completed");
        FocusSessionInfo {
            id: r.get("id"),
            start_time: r.get("start_time"),
            end_time: r.get("end_time"),
            target_duration_seconds: r.get("target_duration_seconds"),
            actual_duration_seconds: r.get("actual_duration_seconds"),
            completed: completed_val != 0,
        }
    }).collect();

    Ok(sessions)
}

// 8. Fetch Heatmap Data (Yearly/Monthly usage for contribution grid)
#[tauri::command]
pub async fn get_heatmap_data(
    state: tauri::State<'_, Arc<Mutex<TrackerState>>>,
) -> Result<Vec<HeatmapDataPoint>, String> {
    let tracker = state.lock().await;
    let pool = &tracker.db_pool;

    let rows = sqlx::query(
        "SELECT substr(act.start_time, 1, 10) as act_date, SUM(act.duration_seconds) as total 
         FROM activities act 
         GROUP BY act_date 
         ORDER BY act_date ASC"
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    let points = rows.into_iter().map(|r| HeatmapDataPoint {
        date: r.get("act_date"),
        count: r.get("total"),
    }).collect();

    Ok(points)
}

// ─── Cross-platform Autostart Management ───

use std::sync::atomic::{AtomicBool, Ordering};

/// Cached background-mode state. Read on every close event to avoid
/// spawning a subprocess (which causes a visible freeze).
/// Initialized from the real autostart status on app startup,
/// and updated whenever the user toggles the setting.
pub static BACKGROUND_ENABLED: AtomicBool = AtomicBool::new(true);

/// Debug builds use a separate registry key / plist / desktop file
/// so that dev and production don't overwrite each other.
#[cfg(target_os = "windows")]
#[cfg(debug_assertions)]
const AUTOSTART_REG_NAME: &str = "DigitalWellbeing-Dev";

#[cfg(target_os = "windows")]
#[cfg(not(debug_assertions))]
const AUTOSTART_REG_NAME: &str = "DigitalWellbeing";

#[cfg(target_os = "windows")]
fn get_exe_path() -> Option<String> {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(|s| s.to_string()))
}



#[cfg(target_os = "windows")]
pub fn autostart_set(enabled: bool) -> Result<(), String> {
    use std::process::Command;
    use std::os::windows::process::CommandExt;
    if enabled {
        let exe = get_exe_path().ok_or("Cannot resolve executable path")?;
        let exe_with_flag = format!("\"{}\" --background", exe);
        let status = Command::new("reg")
            .args([
                "add",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                "/v", AUTOSTART_REG_NAME,
                "/t", "REG_SZ",
                "/d", &exe_with_flag,
                "/f",
            ])
            .creation_flags(0x08000000)
            .status()
            .map_err(|e| e.to_string())?;
        if status.success() { Ok(()) } else { Err("Failed to add registry entry".into()) }
    } else {
        let status = Command::new("reg")
            .args([
                "delete",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                "/v", AUTOSTART_REG_NAME,
                "/f",
            ])
            .creation_flags(0x08000000)
            .status()
            .map_err(|e| e.to_string())?;
        if status.success() { Ok(()) } else { Err("Failed to remove registry entry".into()) }
    }
}



// Windows-only autostart. This app is Windows-exclusive.
#[cfg(not(target_os = "windows"))]
pub fn autostart_set(_enabled: bool) -> Result<(), String> {
    Err("DigitalWellbeing only supports Windows".to_string())
}

#[tauri::command]
pub async fn get_autostart_enabled() -> Result<bool, String> {
    Ok(BACKGROUND_ENABLED.load(Ordering::Relaxed))
}

#[tauri::command]
pub async fn set_autostart_enabled(
    state: tauri::State<'_, Arc<Mutex<TrackerState>>>,
    enabled: bool,
) -> Result<(), String> {
    autostart_set(enabled)?;
    BACKGROUND_ENABLED.store(enabled, Ordering::Relaxed);

    let tracker = state.lock().await;
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES ('autostart', ?)
         ON CONFLICT(key) DO UPDATE SET value = ?"
    )
    .bind(if enabled { "true" } else { "false" })
    .bind(if enabled { "true" } else { "false" })
    .execute(&tracker.db_pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
pub async fn export_data(content: String, filename: String) -> Result<String, String> {
    let file_path = rfd::FileDialog::new()
        .set_file_name(&filename)
        .save_file();
    
    if let Some(path) = file_path {
        std::fs::write(&path, content).map_err(|e| e.to_string())?;
        Ok(path.to_string_lossy().to_string())
    } else {
        Err("Save cancelled".to_string())
    }
}

#[tauri::command]
pub async fn get_idle_monitoring(
    state: tauri::State<'_, Arc<Mutex<TrackerState>>>,
) -> Result<bool, String> {
    let tracker = state.lock().await;
    Ok(tracker.is_idle_monitoring)
}

#[tauri::command]
pub async fn set_idle_monitoring(
    state: tauri::State<'_, Arc<Mutex<TrackerState>>>,
    enabled: bool,
) -> Result<(), String> {
    let mut tracker = state.lock().await;
    tracker.is_idle_monitoring = enabled;
    
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES ('idle_monitoring', ?)
         ON CONFLICT(key) DO UPDATE SET value = ?"
    )
    .bind(if enabled { "true" } else { "false" })
    .bind(if enabled { "true" } else { "false" })
    .execute(&tracker.db_pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

// 9. Get Yearly Data (monthly aggregated for past 12 months)
#[tauri::command]
pub async fn get_yearly_data(
    state: tauri::State<'_, Arc<Mutex<TrackerState>>>,
    date_str: String,
) -> Result<Vec<YearlyDataPoint>, String> {
    use chrono::Datelike;

    let tracker = state.lock().await;
    let pool = &tracker.db_pool;

    let base = chrono::NaiveDate::parse_from_str(&date_str, "%Y-%m-%d")
        .map_err(|e| e.to_string())?;

    let rows = sqlx::query(
        "SELECT substr(act.start_time, 1, 7) as month, SUM(act.duration_seconds) as total \
         FROM activities act \
         GROUP BY month \
         ORDER BY month ASC"
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    let mut lookup = std::collections::HashMap::<String, i64>::new();
    for r in rows {
        let m: String = r.get("month");
        let t: i64 = r.get("total");
        lookup.insert(m, t);
    }

    let month_names = ["Jan","Feb","Mar","Apr","May","Jun",
                       "Jul","Aug","Sep","Oct","Nov","Dec"];

    let mut result = Vec::new();
    let base_year = base.year();
    let base_month = base.month() as i32; // 1..=12

    for i in (0i32..12).rev() {
        // offset from month 0 (Jan of base_year): base_month-1 - i
        let raw = base_month - 1 - i;
        let (y, m) = if raw >= 0 {
            (base_year, raw % 12 + 1)
        } else {
            let years_back = ((-raw - 1) / 12) + 1;
            let adj = raw + years_back * 12;
            (base_year - years_back, adj + 1)
        };
        let month_str = format!("{:04}-{:02}", y, m);
        let label = month_names[(m - 1) as usize].to_string();
        let total_secs = lookup.get(&month_str).copied().unwrap_or(0);
        result.push(YearlyDataPoint {
            month: month_str,
            month_label: label,
            total_minutes: total_secs / 60,
        });
    }

    Ok(result)
}

// 10. Get Average Screen Time Stats
#[tauri::command]
pub async fn get_avg_screen_time(
    state: tauri::State<'_, Arc<Mutex<TrackerState>>>,
) -> Result<AvgScreenTimeStats, String> {
    use chrono::Datelike;

    let tracker = state.lock().await;
    let pool = &tracker.db_pool;

    let day_rows = sqlx::query(
        "SELECT substr(start_time, 1, 10) as day, SUM(duration_seconds) as total \
         FROM activities \
         GROUP BY day"
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    if day_rows.is_empty() {
        return Ok(AvgScreenTimeStats {
            daily_avg_seconds: 0,
            weekly_avg_seconds: 0,
            monthly_avg_seconds: 0,
            yearly_avg_seconds: 0,
            tracked_days: 0,
        });
    }

    let mut day_totals: Vec<(String, i64)> = day_rows.iter().map(|r| {
        let day: String = r.get("day");
        let total: i64 = r.get("total");
        (day, total)
    }).collect();
    day_totals.sort_by(|a, b| a.0.cmp(&b.0));

    let tracked_days = day_totals.len() as i64;
    let total_all: i64 = day_totals.iter().map(|(_, s)| s).sum();
    let daily_avg_seconds = total_all / tracked_days;

    // Weekly avg
    let mut week_map = std::collections::HashMap::<String, i64>::new();
    for (day_str, secs) in &day_totals {
        if let Ok(date) = chrono::NaiveDate::parse_from_str(day_str, "%Y-%m-%d") {
            let iso = date.iso_week();
            let key = format!("{:04}-W{:02}", iso.year(), iso.week());
            *week_map.entry(key).or_insert(0) += secs;
        }
    }
    let num_weeks = week_map.len().max(1) as i64;
    let weekly_avg_seconds: i64 = week_map.values().sum::<i64>() / num_weeks;

    // Monthly avg
    let mut month_map = std::collections::HashMap::<String, i64>::new();
    for (day_str, secs) in &day_totals {
        if day_str.len() >= 7 {
            let key = day_str[..7].to_string();
            *month_map.entry(key).or_insert(0) += secs;
        }
    }
    let num_months = month_map.len().max(1) as i64;
    let monthly_avg_seconds: i64 = month_map.values().sum::<i64>() / num_months;

    // Yearly avg
    let mut year_map = std::collections::HashMap::<String, i64>::new();
    for (day_str, secs) in &day_totals {
        if day_str.len() >= 4 {
            let key = day_str[..4].to_string();
            *year_map.entry(key).or_insert(0) += secs;
        }
    }
    let num_years = year_map.len().max(1) as i64;
    let yearly_avg_seconds: i64 = year_map.values().sum::<i64>() / num_years;

    Ok(AvgScreenTimeStats {
        daily_avg_seconds,
        weekly_avg_seconds,
        monthly_avg_seconds,
        yearly_avg_seconds,
        tracked_days,
    })
}

// 11. Backup Database using VACUUM INTO (safe online backup)
#[tauri::command]
pub async fn backup_database(
    state: tauri::State<'_, Arc<Mutex<TrackerState>>>,
    _app: AppHandle,
) -> Result<String, String> {
    let file_path = rfd::FileDialog::new()
        .set_file_name("wellbeing_backup.db")
        .add_filter("SQLite Database", &["db"])
        .save_file();

    let dest = match file_path {
        Some(p) => p,
        None => return Err("Backup cancelled".to_string()),
    };

    let dest_str = dest.to_string_lossy().to_string();
    // Escape single quotes in path for SQLite
    let safe_dest = dest_str.replace('\'', "''");

    let tracker = state.lock().await;
    let pool = &tracker.db_pool;
    let query = format!("VACUUM INTO '{}'", safe_dest);
    sqlx::query(&query)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;

    Ok(dest_str)
}

// 12. Restore Database (validate, copy, restart)
#[tauri::command]
pub async fn restore_database(
    app: AppHandle,
) -> Result<(), String> {
    let file_path = rfd::FileDialog::new()
        .add_filter("SQLite Database", &["db"])
        .pick_file();

    let src = match file_path {
        Some(p) => p,
        None => return Err("Restore cancelled".to_string()),
    };

    // Validate SQLite magic header
    let header = std::fs::read(&src).map_err(|e| e.to_string())?;
    let magic = b"SQLite format 3\0";
    if header.len() < 16 || &header[..16] != magic {
        return Err("Invalid SQLite database file. Please select a valid .db backup.".to_string());
    }

    // Resolve destination path
    let app_data_dir = app.path().app_local_data_dir()
        .map_err(|e| e.to_string())?;
    let dest = app_data_dir.join("wellbeing.db");

    std::fs::copy(&src, &dest).map_err(|e| e.to_string())?;

    // Restart to reload with restored database
    app.restart();
}
