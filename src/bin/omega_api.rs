//! Local HTTP API for the Electron UI (replaces Tauri IPC).
//! Default bind: 127.0.0.1:17421 (override with OMEGA_API_PORT).

use axum::extract::Query;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{extract::State, Json, Router};
use serde::Deserialize;
use sensor_layer::app_commands;
use sensor_layer::app_models::{PipelineRunRecord, SessionListItem, SessionSummaryState, SummaryRevision};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tower_http::cors::{Any, CorsLayer};

#[derive(Debug, Deserialize)]
struct SessionKeyQuery {
    session_key: String,
}

#[derive(Debug, Deserialize)]
struct SaveSummaryBody {
    title: String,
    body: String,
    #[serde(default, rename = "editorLabel")]
    editor_label: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PipelineBody {
    stage: String,
    #[serde(default, rename = "inputRef")]
    input_ref: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RunsQuery {
    #[serde(default)]
    limit: Option<u32>,
}

#[derive(Clone)]
struct ApiState {
    runtime_start_epoch_secs: u64,
    phase1: Arc<Mutex<Phase1State>>,
}

struct Phase1State {
    child: Option<Child>,
}

#[derive(serde::Serialize)]
struct FetchSummaryResponse {
    summary: String,
}

#[derive(serde::Serialize)]
struct SessionStatusResponse {
    status: &'static str,
}

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn logs_dir() -> PathBuf {
    PathBuf::from(std::env::var("OMEGA_APP_LOGS_DIR").unwrap_or_else(|_| "logs".to_string()))
}

fn spawn_phase1_capture() -> Result<Child, String> {
    let current_exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let sibling = current_exe
        .parent()
        .map(|p| p.join("sensor_layer"))
        .ok_or_else(|| "cannot determine executable directory".to_string())?;
    let mut cmd = if sibling.exists() {
        let mut c = Command::new(sibling);
        c.arg("capture");
        c
    } else {
        let mut c = Command::new("cargo");
        c.args(["run", "--bin", "sensor_layer", "--", "capture"]);
        c
    };
    cmd.spawn()
        .map_err(|e| format!("failed to start phase1 capture: {e}"))
}

fn ensure_phase1_running(state: &mut Phase1State) -> Result<(), String> {
    if let Some(child) = state.child.as_mut() {
        if child.try_wait().map_err(|e| e.to_string())?.is_none() {
            return Ok(());
        }
    }
    state.child = Some(spawn_phase1_capture()?);
    Ok(())
}

fn stop_phase1_capture(state: &mut Phase1State) -> Result<(), String> {
    let Some(mut child) = state.child.take() else {
        return Ok(());
    };
    let pid = child.id().to_string();
    let _ = Command::new("kill").args(["-INT", &pid]).status();
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(8) {
        if child.try_wait().map_err(|e| e.to_string())?.is_some() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let _ = child.kill();
    let _ = child.wait();
    Ok(())
}

/// Lock phase1 state. If a previous holder panicked, recover the inner guard so fetch can continue.
fn lock_phase1(m: &Mutex<Phase1State>) -> MutexGuard<'_, Phase1State> {
    m.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn latest_runtime_capture_log(runtime_start_epoch_secs: u64) -> Result<Option<PathBuf>, String> {
    let mut latest: Option<(u64, PathBuf)> = None;
    let dir = logs_dir();
    if !dir.exists() {
        return Ok(None);
    }
    for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if !(name.starts_with("capture-session-") && name.ends_with(".json")) {
            continue;
        }
        let ts = name
            .trim_start_matches("capture-session-")
            .trim_end_matches(".json")
            .parse::<u64>()
            .ok();
        let Some(ts) = ts else { continue };
        if ts < runtime_start_epoch_secs {
            continue;
        }
        if latest.as_ref().map(|(best, _)| ts > *best).unwrap_or(true) {
            latest = Some((ts, path));
        }
    }
    Ok(latest.map(|(_, p)| p))
}

async fn health() -> &'static str {
    "ok"
}

async fn list_sessions() -> Result<Json<Vec<SessionListItem>>, (StatusCode, String)> {
    app_commands::list_sessions()
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))
}

async fn get_session_summary(
    Query(q): Query<SessionKeyQuery>,
) -> Result<Json<SessionSummaryState>, (StatusCode, String)> {
    app_commands::get_session_summary(q.session_key)
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))
}

async fn save_summary_revision(
    Query(q): Query<SessionKeyQuery>,
    Json(body): Json<SaveSummaryBody>,
) -> Result<Json<SessionSummaryState>, (StatusCode, String)> {
    app_commands::save_summary_revision(
        q.session_key,
        body.title,
        body.body,
        body.editor_label,
    )
    .map(Json)
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))
}

async fn list_summary_revisions(
    Query(q): Query<SessionKeyQuery>,
) -> Result<Json<Vec<SummaryRevision>>, (StatusCode, String)> {
    app_commands::list_summary_revisions(q.session_key)
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))
}

async fn run_pipeline(Json(body): Json<PipelineBody>) -> Result<Json<PipelineRunRecord>, (StatusCode, String)> {
    let stage = body.stage.clone();
    let input = body.input_ref.clone();
    tokio::task::spawn_blocking(move || app_commands::run_pipeline_stage(stage, input))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))
}

async fn list_pipeline_runs(Query(q): Query<RunsQuery>) -> Result<Json<Vec<PipelineRunRecord>>, (StatusCode, String)> {
    app_commands::list_pipeline_runs(q.limit)
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))
}

async fn fetch_summary(
    State(state): State<ApiState>,
) -> Result<Json<FetchSummaryResponse>, (StatusCode, String)> {
    // Do not hold `phase1` while running phases 2–4: a panic there would poison the mutex.
    {
        let mut phase1 = lock_phase1(&state.phase1);
        stop_phase1_capture(&mut phase1).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    }

    let latest_log = latest_runtime_capture_log(state.runtime_start_epoch_secs)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                "No runtime capture log found yet. Let phase1 run and interact, then fetch again."
                    .to_string(),
            )
        })?;

    // Phase 2–4 use blocking I/O (`reqwest::blocking`, sqlite, etc.). Running that on the
    // async runtime worker triggers: "Cannot drop a runtime in a context where blocking is not allowed."
    let input_ref = latest_log.display().to_string();
    let pipeline_result = tokio::task::spawn_blocking(move || -> Result<String, String> {
        app_commands::run_pipeline_stage("phase2".to_string(), Some(input_ref))?;
        app_commands::run_pipeline_stage("phase3".to_string(), None)?;
        app_commands::run_pipeline_stage("phase4".to_string(), None)?;
        app_commands::load_generated_summary_text()
    })
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("pipeline task join failed: {e}"),
        )
    })?;

    // Always resume Phase 1 capture after the pipeline attempt (success or failure).
    {
        let mut phase1 = lock_phase1(&state.phase1);
        ensure_phase1_running(&mut phase1).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    }

    let summary = pipeline_result.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(FetchSummaryResponse { summary }))
}

async fn start_session(
    State(state): State<ApiState>,
) -> Result<Json<SessionStatusResponse>, (StatusCode, String)> {
    let mut phase1 = lock_phase1(&state.phase1);
    ensure_phase1_running(&mut phase1).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(SessionStatusResponse { status: "running" }))
}

async fn end_session(
    State(state): State<ApiState>,
) -> Result<Json<FetchSummaryResponse>, (StatusCode, String)> {
    {
        let mut phase1 = lock_phase1(&state.phase1);
        stop_phase1_capture(&mut phase1).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    }

    let latest_log = latest_runtime_capture_log(state.runtime_start_epoch_secs)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                "No runtime capture log found yet. Let phase1 run and interact, then end the session."
                    .to_string(),
            )
        })?;

    let input_ref = latest_log.display().to_string();
    let summary = tokio::task::spawn_blocking(move || -> Result<String, String> {
        app_commands::run_pipeline_stage("phase2".to_string(), Some(input_ref))?;
        app_commands::run_pipeline_stage("phase3".to_string(), None)?;
        app_commands::run_pipeline_stage("phase4".to_string(), None)?;
        app_commands::load_generated_summary_text()
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("pipeline task join failed: {e}")))?
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    Ok(Json(FetchSummaryResponse { summary }))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Match `sensor_layer` / CLI: load `.env` so OMEGA_EMBEDDING_BACKEND, keys, etc. apply to
    // phase2–4 inside fetch (Electron does not inject `.env` automatically).
    let _ = dotenvy::dotenv();

    let runtime_start_epoch_secs = now_epoch_secs();
    let runtime_db = logs_dir().join(format!("runtime-phase2-{runtime_start_epoch_secs}.db"));
    std::env::set_var("OMEGA_PHASE2_DB_PATH", runtime_db.display().to_string());
    std::env::set_var("OMEGA_PHASE3_DB_PATH", runtime_db.display().to_string());
    std::env::set_var("OMEGA_PHASE4_DB_PATH", runtime_db.display().to_string());

    let port: u16 = std::env::var("OMEGA_API_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(17_421);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));

    // Phase 1 starts only when the client calls POST /api/session/start (idle by default).
    let phase1_state = Phase1State { child: None };
    let shared_state = ApiState {
        runtime_start_epoch_secs,
        phase1: Arc::new(Mutex::new(phase1_state)),
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/api/sessions", get(list_sessions))
        .route("/api/session-summary", get(get_session_summary))
        .route("/api/session-summary", post(save_summary_revision))
        .route("/api/summary-revisions", get(list_summary_revisions))
        .route("/api/pipeline/run", post(run_pipeline))
        .route("/api/pipeline/runs", get(list_pipeline_runs))
        .route("/api/session/start", post(start_session))
        .route("/api/session/end", post(end_session))
        .route("/api/fetch-summary", post(fetch_summary))
        .with_state(shared_state)
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        );

    println!("omega-api listening on http://127.0.0.1:{port}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
