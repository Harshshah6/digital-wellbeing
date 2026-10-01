/// api_server.rs — DigitalWellbeing Enterprise Agent API
///
/// Exposes port 7842 (HTTP REST) and port 7843 (UDP discovery beacon).
/// The admin dashboard discovers agents via HTTP probing and queries them.

use axum::{
    extract::{Query, State},
    http::Method,
    response::Json,
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use std::{net::UdpSocket, sync::Arc, time::Duration};
use tokio::sync::Mutex;
use tower_http::cors::{Any, CorsLayer};

use crate::tracker::TrackerState;

pub const HTTP_PORT: u16 = 7842;
pub const UDP_BEACON_PORT: u16 = 7843;
pub const UDP_BROADCAST_ADDR: &str = "255.255.255.255";

// ─── Shared axum state ────────────────────────────────────────────────────────

/// Everything axum handlers need — all resolved at startup, no block_on needed.
#[derive(Clone)]
pub struct ApiState {
    pub tracker: Arc<Mutex<TrackerState>>,
    pub device_id: String,
    pub hostname: String,
    pub local_ip: String,
    pub peers: Arc<Mutex<std::collections::HashMap<String, serde_json::Value>>>,
}

// ─── API Response types ───────────────────────────────────────────────────────

#[allow(dead_code)]
#[derive(Serialize)]
pub struct AgentInfo {
    pub device_id: String,
    pub hostname: String,
    pub ip: String,
    pub version: String,
}

#[derive(Serialize)]
pub struct AppUsageApi {
    pub display_name: String,
    pub executable_name: String,
    pub category: String,
    pub total_seconds: i64,
    pub productivity_score: i32,
}

#[derive(Serialize)]
pub struct CategorySummaryApi {
    pub category: String,
    pub total_seconds: i64,
}

#[derive(Serialize)]
pub struct TimelineSegmentApi {
    pub hour: i32,
    pub total_seconds: i64,
}

#[derive(Serialize)]
pub struct HeatmapPointApi {
    pub date: String,
    pub count: i64,
}

#[allow(dead_code)]
#[derive(Serialize)]
pub struct YearlyDataApi {
    pub month: String,
    pub month_label: String,
    pub total_minutes: i64,
}

#[derive(Serialize)]
pub struct AvgStatsApi {
    pub daily_avg_seconds: i64,
    pub weekly_avg_seconds: i64,
    pub monthly_avg_seconds: i64,
    pub yearly_avg_seconds: i64,
    pub tracked_days: i64,
}

#[derive(Deserialize)]
pub struct DateQuery {
    pub date: Option<String>,
    pub limit: Option<i64>,
}

// ─── Helpers resolved at startup (synchronous) ───────────────────────────────

pub fn get_local_ip() -> String {
    match UdpSocket::bind("0.0.0.0:0") {
        Ok(socket) => {
            let _ = socket.connect("8.8.8.8:80");
            socket
                .local_addr()
                .map(|a| a.ip().to_string())
                .unwrap_or_else(|_| "127.0.0.1".to_string())
        }
        Err(_) => "127.0.0.1".to_string(),
    }
}

/// Must be called synchronously (from setup block, outside async runtime).
pub fn get_or_create_device_id_sync(pool: &SqlitePool) -> String {
    tauri::async_runtime::block_on(async {
        let existing: Option<(String,)> =
            sqlx::query_as("SELECT value FROM settings WHERE key = 'device_id'")
                .fetch_optional(pool)
                .await
                .unwrap_or(None);

        if let Some((id,)) = existing {
            return id;
        }

        let new_id = uuid::Uuid::new_v4().to_string();
        let _ = sqlx::query(
            "INSERT INTO settings (key, value) VALUES ('device_id', ?) \
             ON CONFLICT(key) DO UPDATE SET value = ?",
        )
        .bind(&new_id)
        .bind(&new_id)
        .execute(pool)
        .await;
        new_id
    })
}

// ─── Route handlers ───────────────────────────────────────────────────────────

async fn handle_info(State(state): State<ApiState>) -> Json<serde_json::Value> {
    let subnet = state.local_ip.split('.').take(3).collect::<Vec<_>>().join(".");
    Json(serde_json::json!({
        "app": "DigitalWellbeing",
        "device_id": state.device_id,
        "hostname": state.hostname,
        "ip": state.local_ip,
        "subnet": subnet,
        "port": HTTP_PORT,
        "version": env!("CARGO_PKG_VERSION"),
        "os": std::env::consts::OS,
    }))
}

async fn handle_peers(State(state): State<ApiState>) -> Json<Vec<serde_json::Value>> {
    let map = state.peers.lock().await;
    let list: Vec<serde_json::Value> = map.values().cloned().collect();
    Json(list)
}

async fn handle_top_apps(
    State(state): State<ApiState>,
    Query(q): Query<DateQuery>,
) -> Json<Vec<AppUsageApi>> {
    let tracker = state.tracker.lock().await;
    let pool = &tracker.db_pool;
    let date = q
        .date
        .unwrap_or_else(|| chrono::Local::now().format("%Y-%m-%d").to_string());
    let limit = q.limit.unwrap_or(10);

    let rows = sqlx::query(
        "SELECT a.display_name, a.executable_name, a.category, \
         SUM(act.duration_seconds) as total_seconds, a.productivity_score \
         FROM apps a JOIN activities act ON a.id = act.app_id \
         WHERE substr(act.start_time, 1, 10) = ? \
         GROUP BY a.id ORDER BY total_seconds DESC LIMIT ?",
    )
    .bind(&date)
    .bind(limit)
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    Json(
        rows.into_iter()
            .map(|r| AppUsageApi {
                display_name: r.get("display_name"),
                executable_name: r.get("executable_name"),
                category: r.get("category"),
                total_seconds: r.get("total_seconds"),
                productivity_score: r.get("productivity_score"),
            })
            .collect(),
    )
}

async fn handle_categories(
    State(state): State<ApiState>,
    Query(q): Query<DateQuery>,
) -> Json<Vec<CategorySummaryApi>> {
    let tracker = state.tracker.lock().await;
    let pool = &tracker.db_pool;
    let date = q
        .date
        .unwrap_or_else(|| chrono::Local::now().format("%Y-%m-%d").to_string());

    let rows = sqlx::query(
        "SELECT a.category, SUM(act.duration_seconds) as total_seconds \
         FROM apps a JOIN activities act ON a.id = act.app_id \
         WHERE substr(act.start_time, 1, 10) = ? \
         GROUP BY a.category ORDER BY total_seconds DESC",
    )
    .bind(&date)
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    Json(
        rows.into_iter()
            .map(|r| CategorySummaryApi {
                category: r.get("category"),
                total_seconds: r.get("total_seconds"),
            })
            .collect(),
    )
}

async fn handle_timeline(
    State(state): State<ApiState>,
    Query(q): Query<DateQuery>,
) -> Json<Vec<TimelineSegmentApi>> {
    let tracker = state.tracker.lock().await;
    let pool = &tracker.db_pool;
    let date = q
        .date
        .unwrap_or_else(|| chrono::Local::now().format("%Y-%m-%d").to_string());

    let rows = sqlx::query(
        "SELECT CAST(substr(act.start_time, 12, 2) AS INTEGER) as hour, \
         SUM(act.duration_seconds) as total_seconds \
         FROM activities act WHERE substr(act.start_time, 1, 10) = ? \
         GROUP BY hour ORDER BY hour",
    )
    .bind(&date)
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let mut timeline = vec![0i64; 24];
    for r in rows {
        let hr: i32 = r.get("hour");
        let secs: i64 = r.get("total_seconds");
        if hr >= 0 && hr < 24 {
            timeline[hr as usize] = secs;
        }
    }

    Json(
        timeline
            .into_iter()
            .enumerate()
            .map(|(h, s)| TimelineSegmentApi {
                hour: h as i32,
                total_seconds: s,
            })
            .collect(),
    )
}

async fn handle_heatmap(State(state): State<ApiState>) -> Json<Vec<HeatmapPointApi>> {
    let tracker = state.tracker.lock().await;
    let pool = &tracker.db_pool;

    let rows = sqlx::query(
        "SELECT substr(act.start_time, 1, 10) as act_date, \
         SUM(act.duration_seconds) as total \
         FROM activities act GROUP BY act_date ORDER BY act_date ASC",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    Json(
        rows.into_iter()
            .map(|r| HeatmapPointApi {
                date: r.get("act_date"),
                count: r.get("total"),
            })
            .collect(),
    )
}

async fn handle_avg_stats(State(state): State<ApiState>) -> Json<AvgStatsApi> {
    let tracker = state.tracker.lock().await;
    let pool = &tracker.db_pool;

    let day_rows = sqlx::query(
        "SELECT substr(start_time, 1, 10) as day, SUM(duration_seconds) as total \
         FROM activities GROUP BY day",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    if day_rows.is_empty() {
        return Json(AvgStatsApi {
            daily_avg_seconds: 0,
            weekly_avg_seconds: 0,
            monthly_avg_seconds: 0,
            yearly_avg_seconds: 0,
            tracked_days: 0,
        });
    }

    let mut day_totals: Vec<(String, i64)> = day_rows
        .iter()
        .map(|r| (r.get::<String, _>("day"), r.get::<i64, _>("total")))
        .collect();
    day_totals.sort_by(|a, b| a.0.cmp(&b.0));

    let tracked_days = day_totals.len() as i64;
    let total_all: i64 = day_totals.iter().map(|(_, s)| s).sum();
    let daily_avg = total_all / tracked_days;

    let mut week_map = std::collections::HashMap::<String, i64>::new();
    for (ds, secs) in &day_totals {
        if let Ok(date) = chrono::NaiveDate::parse_from_str(ds, "%Y-%m-%d") {
            use chrono::Datelike;
            let iso = date.iso_week();
            let key = format!("{:04}-W{:02}", iso.year(), iso.week());
            *week_map.entry(key).or_insert(0) += secs;
        }
    }
    let weekly_avg = week_map.values().sum::<i64>() / (week_map.len().max(1) as i64);

    let mut month_map = std::collections::HashMap::<String, i64>::new();
    for (ds, secs) in &day_totals {
        if ds.len() >= 7 {
            *month_map.entry(ds[..7].to_string()).or_insert(0) += secs;
        }
    }
    let monthly_avg = month_map.values().sum::<i64>() / (month_map.len().max(1) as i64);

    let mut year_map = std::collections::HashMap::<String, i64>::new();
    for (ds, secs) in &day_totals {
        if ds.len() >= 4 {
            *year_map.entry(ds[..4].to_string()).or_insert(0) += secs;
        }
    }
    let yearly_avg = year_map.values().sum::<i64>() / (year_map.len().max(1) as i64);

    Json(AvgStatsApi {
        daily_avg_seconds: daily_avg,
        weekly_avg_seconds: weekly_avg,
        monthly_avg_seconds: monthly_avg,
        yearly_avg_seconds: yearly_avg,
        tracked_days,
    })
}

// ─── Start enterprise services ────────────────────────────────────────────────

/// Called from lib.rs setup block (synchronous context).
/// device_id and hostname are resolved HERE (sync) before spawning async tasks.
pub fn start_enterprise_services(
    tracker_state: Arc<Mutex<TrackerState>>,
    pool: SqlitePool,
) {
    // Resolve these synchronously now — safe, we're in the setup block
    let device_id = get_or_create_device_id_sync(&pool);
    let local_ip = get_local_ip();
    let hostname = hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "Unknown".to_string());

    println!(
        "[DigitalWellbeing] Agent ID: {} | Host: {} | IP: {}",
        device_id, hostname, local_ip
    );

    let peers_map = Arc::new(Mutex::new(std::collections::HashMap::<String, serde_json::Value>::new()));

    let api_state = ApiState {
        tracker: tracker_state,
        device_id: device_id.clone(),
        hostname: hostname.clone(),
        local_ip: local_ip.clone(),
        peers: peers_map.clone(),
    };

    // ── HTTP REST server ──────────────────────────────────────────────────────
    let srv_state = api_state.clone();
    tauri::async_runtime::spawn(async move {
        let cors = CorsLayer::new()
            .allow_methods([Method::GET, Method::OPTIONS])
            .allow_origin(Any)
            .allow_headers(Any);

        let app = Router::new()
            .route("/api/info", get(handle_info))
            .route("/api/peers", get(handle_peers))
            .route("/api/top_apps", get(handle_top_apps))
            .route("/api/categories", get(handle_categories))
            .route("/api/timeline", get(handle_timeline))
            .route("/api/heatmap", get(handle_heatmap))
            .route("/api/avg_stats", get(handle_avg_stats))
            .layer(cors)
            .with_state(srv_state);

        let bind_addr = format!("0.0.0.0:{}", HTTP_PORT);
        match tokio::net::TcpListener::bind(&bind_addr).await {
            Ok(listener) => {
                println!("[DigitalWellbeing] HTTP REST API listening on http://{}", bind_addr);
                if let Err(e) = axum::serve(listener, app).await {
                    eprintln!("[DigitalWellbeing] HTTP server error: {}", e);
                }
            }
            Err(e) => {
                eprintln!("[DigitalWellbeing] Failed to bind HTTP on {}: {}", bind_addr, e);
            }
        }
    });

    // ── UDP discovery beacon broadcast ─────────────────────────────────────────
    let beacon = format!(
        r#"{{"app":"DigitalWellbeing","device_id":"{device_id}","hostname":"{hostname}","ip":"{local_ip}","port":{HTTP_PORT},"version":"{}"}}"#,
        env!("CARGO_PKG_VERSION")
    );

    tauri::async_runtime::spawn(async move {
        // Use std socket for UDP broadcast — no async needed here
        match UdpSocket::bind("0.0.0.0:0") {
            Ok(socket) => {
                let _ = socket.set_broadcast(true);
                let target = format!("{}:{}", UDP_BROADCAST_ADDR, UDP_BEACON_PORT);
                loop {
                    let _ = socket.send_to(beacon.as_bytes(), &target);
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
            Err(e) => eprintln!("[DigitalWellbeing] UDP beacon error: {}", e),
        }
    });

    // ── UDP discovery beacon listener ──────────────────────────────────────────
    let peers_clone = peers_map.clone();
    let my_device_id = device_id.clone();
    tauri::async_runtime::spawn(async move {
        let std_socket = match std::net::UdpSocket::bind(format!("0.0.0.0:{}", UDP_BEACON_PORT)) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[DigitalWellbeing] UDP beacon listener bind error: {}", e);
                return;
            }
        };
        let _ = std_socket.set_nonblocking(true);
        let socket = match tokio::net::UdpSocket::from_std(std_socket) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[DigitalWellbeing] UDP beacon listener socket error: {}", e);
                return;
            }
        };

        let mut buf = [0u8; 2048];
        loop {
            match socket.recv_from(&mut buf).await {
                Ok((len, peer_addr)) => {
                    if let Ok(mut val) = serde_json::from_slice::<serde_json::Value>(&buf[..len]) {
                        if val.get("app").and_then(|v| v.as_str()) == Some("DigitalWellbeing") {
                            let is_self = val.get("device_id").and_then(|v| v.as_str()) == Some(&my_device_id);
                            if !is_self {
                                let ip = val.get("ip").and_then(|v| v.as_str())
                                    .map(|s| s.to_string())
                                    .unwrap_or_else(|| peer_addr.ip().to_string());
                                if let Some(obj) = val.as_object_mut() {
                                    obj.insert("ip".to_string(), serde_json::Value::String(ip.clone()));
                                }
                                let mut map = peers_clone.lock().await;
                                map.insert(ip, val);
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("[DigitalWellbeing] UDP beacon listener recv error: {}", e);
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            }
        }
    });
}
