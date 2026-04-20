//! Local HTTP API for the Electron UI (replaces Tauri IPC).
//! Default bind: 127.0.0.1:17421 (override with OMEGA_API_PORT).

use axum::extract::Query;
use axum::http::StatusCode;
use axum::routing::{delete, get, post, put};
use axum::{extract::State, Json, Router};
use serde::{Deserialize, Serialize};
use sensor_layer::app_commands;
use sensor_layer::app_models::{
    ActionOutputRecord, CaptureExclusionsState, DeleteLocalDataResponse, PipelineRunRecord,
    SessionListItem, SessionSummaryState, StorageManifest, SummaryRevision,
};
use sensor_layer::capture_live_status::Phase1LiveStatus;
use sensor_layer::ide_context;
use sensor_layer::models::{IdeContext, Phase1Payload, VisualLogItem};
use sensor_layer::usage::ApiUsageResponse;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
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

#[derive(Debug, Deserialize)]
struct UsageQuery {
    #[serde(default)]
    session_key: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CapturePauseBody {
    paused: bool,
}

#[derive(Debug, Deserialize)]
struct DeleteAllLocalBody {
    confirm: bool,
}

#[derive(Debug, Deserialize)]
struct DeleteSessionBody {
    #[serde(rename = "sessionKey")]
    session_key: String,
}

#[derive(Debug, Deserialize)]
struct RunActionBody {
    #[serde(rename = "sessionKey")]
    session_key: String,
    #[serde(rename = "actionType")]
    action_type: String,
    #[serde(default, rename = "bucketIds")]
    bucket_ids: Option<Vec<i64>>,
    #[serde(default, rename = "customPrompt")]
    custom_prompt: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ActionOutputsQuery {
    session_key: String,
}

/// Payload pushed by the Omega VS Code / JetBrains extension (or any IDE plugin).
#[derive(Debug, Deserialize)]
struct IdeContextPushBody {
    /// Process/app name as it appears in the OS, e.g. `"Code"`, `"PyCharm"`.
    #[serde(default)]
    app_name: String,
    /// Human-readable IDE name, e.g. `"VS Code"`.
    #[serde(default)]
    ide_label: String,
    #[serde(default)]
    workspace: Option<String>,
    #[serde(default)]
    active_file: Option<String>,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    git_branch: Option<String>,
    /// Visible code text in the editor viewport (optional; may be empty).
    #[serde(default)]
    visible_code: Option<String>,
    /// Current diagnostics / errors from the language server.
    #[serde(default)]
    diagnostics: Option<Vec<String>>,
}

#[derive(Serialize)]
struct IdeContextPushResponse {
    queued: bool,
}

/// Background phase2+phase3 while capture is running (`stop` is shared with the worker thread).
struct IngestWorkerState {
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Default for IngestWorkerState {
    fn default() -> Self {
        Self {
            stop: Arc::new(AtomicBool::new(true)),
            handle: None,
        }
    }
}

fn stop_and_join_ingest_worker(state: &mut IngestWorkerState) {
    state.stop.store(true, Ordering::SeqCst);
    if let Some(h) = state.handle.take() {
        let _ = h.join();
    }
}

fn spawn_ingest_worker(state: &mut IngestWorkerState, runtime_start_epoch_secs: u64) {
    stop_and_join_ingest_worker(state);
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = Arc::clone(&stop);
    let handle = std::thread::Builder::new()
        .name("omega-ingest".to_string())
        .spawn(move || ingest_worker_loop(runtime_start_epoch_secs, stop_thread))
        .expect("spawn omega-ingest worker");
    state.stop = stop;
    state.handle = Some(handle);
}

fn incremental_ingest_interval_ms() -> u64 {
    std::env::var("OMEGA_INCREMENTAL_INGEST_INTERVAL_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8_000u64)
        .clamp(2_000, 120_000)
}

fn ingest_worker_loop(runtime_start_epoch_secs: u64, stop: Arc<AtomicBool>) {
    let interval_ms = incremental_ingest_interval_ms();
    while !stop.load(Ordering::SeqCst) {
        match latest_runtime_capture_log(runtime_start_epoch_secs) {
            Ok(Some(path)) => {
                if let Err(e) = app_commands::run_phase2_phase3_ingest_only(path) {
                    eprintln!("omega-api incremental ingest: {e}");
                }
            }
            Ok(None) => {}
            Err(e) => eprintln!("omega-api incremental ingest: {e}"),
        }
        let step = 250u64;
        let mut waited = 0u64;
        while waited < interval_ms && !stop.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(step));
            waited += step;
        }
    }
}

#[derive(Clone)]
struct ApiState {
    runtime_start_epoch_secs: u64,
    phase1: Arc<Mutex<Phase1State>>,
    ingest: Arc<Mutex<IngestWorkerState>>,
    /// IDE context items pushed by the extension; injected into the session file at end_session.
    ide_queue: Arc<Mutex<Vec<VisualLogItem>>>,
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

/// Reads the JSON session log at `log_path`, appends `items` as `Phase1Payload::Visual`
/// entries, and re-writes the file. Called at session end to include extension-pushed
/// IDE context in the phase2 pipeline.
fn inject_ide_items_into_log(log_path: &PathBuf, items: Vec<VisualLogItem>) -> Result<(), String> {
    #[derive(serde::Deserialize, serde::Serialize)]
    struct SessionLogFile {
        session_summary: serde_json::Value,
        payloads: Vec<Phase1Payload>,
    }

    let raw = std::fs::read_to_string(log_path)
        .map_err(|e| format!("ide inject: read log: {e}"))?;
    let mut log: SessionLogFile = serde_json::from_str(&raw)
        .map_err(|e| format!("ide inject: parse log: {e}"))?;
    for item in items {
        log.payloads.push(Phase1Payload::Visual(item));
    }
    let json = serde_json::to_string_pretty(&log)
        .map_err(|e| format!("ide inject: serialize: {e}"))?;
    std::fs::write(log_path, json)
        .map_err(|e| format!("ide inject: write log: {e}"))?;
    Ok(())
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

fn lock_ingest(m: &Mutex<IngestWorkerState>) -> MutexGuard<'_, IngestWorkerState> {
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

async fn get_phase1_live_status() -> Result<Json<Phase1LiveStatus>, (StatusCode, String)> {
    app_commands::get_phase1_live_status()
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))
}

async fn put_capture_pause(
    Json(body): Json<CapturePauseBody>,
) -> Result<Json<Phase1LiveStatus>, (StatusCode, String)> {
    app_commands::set_capture_paused(body.paused)
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))
}

async fn get_capture_exclusions() -> Result<Json<CaptureExclusionsState>, (StatusCode, String)> {
    app_commands::get_capture_exclusions()
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))
}

async fn put_capture_exclusions(
    Json(body): Json<CaptureExclusionsState>,
) -> Result<Json<CaptureExclusionsState>, (StatusCode, String)> {
    app_commands::set_capture_exclusions(body.excluded_app_names)
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))
}

async fn storage_manifest() -> Result<Json<StorageManifest>, (StatusCode, String)> {
    app_commands::get_storage_manifest()
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))
}

async fn delete_session_privacy_impl(
    session_key: String,
) -> Result<Json<DeleteLocalDataResponse>, (StatusCode, String)> {
    tokio::task::spawn_blocking(move || app_commands::delete_session_data(session_key))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map(Json)
        .map_err(|e| {
            let code = if e.contains("invalid") {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            (code, e)
        })
}

async fn delete_privacy_session(
    Query(q): Query<SessionKeyQuery>,
) -> Result<Json<DeleteLocalDataResponse>, (StatusCode, String)> {
    delete_session_privacy_impl(q.session_key).await
}

async fn post_delete_session(
    Json(body): Json<DeleteSessionBody>,
) -> Result<Json<DeleteLocalDataResponse>, (StatusCode, String)> {
    delete_session_privacy_impl(body.session_key).await
}

async fn post_delete_all_local(
    Json(body): Json<DeleteAllLocalBody>,
) -> Result<Json<DeleteLocalDataResponse>, (StatusCode, String)> {
    if !body.confirm {
        return Err((
            StatusCode::BAD_REQUEST,
            "Set confirm to true to erase all local session data.".to_string(),
        ));
    }
    tokio::task::spawn_blocking(app_commands::delete_all_local_session_data)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))
}

async fn get_usage(
    Query(q): Query<UsageQuery>,
) -> Result<Json<ApiUsageResponse>, (StatusCode, String)> {
    let session_key = q.session_key.clone();
    tokio::task::spawn_blocking(move || app_commands::get_api_usage(session_key))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("usage task join failed: {e}")))?
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
    let session_key = latest_log
        .file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string());
    let input_ref = latest_log.display().to_string();
    let ingest = Arc::clone(&state.ingest);
    let runtime_secs = state.runtime_start_epoch_secs;
    let pipeline_result = tokio::task::spawn_blocking(move || -> Result<String, String> {
        {
            let mut w = ingest
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            stop_and_join_ingest_worker(&mut *w);
        }
        if let Some(ref sk) = session_key {
            std::env::set_var("OMEGA_USAGE_SESSION_KEY", sk);
        }
        let r = (|| -> Result<String, String> {
            app_commands::run_pipeline_stage("phase2".to_string(), Some(input_ref))?;
            app_commands::run_pipeline_stage("phase3".to_string(), None)?;
            app_commands::run_pipeline_stage("phase4".to_string(), None)?;
            app_commands::load_generated_summary_text()
        })();
        let _ = std::env::remove_var("OMEGA_USAGE_SESSION_KEY");
        r
    })
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("pipeline task join failed: {e}"),
        )
    })?;

    // Resume incremental ingest + Phase 1 capture after the pipeline attempt (success or failure).
    {
        let mut ingest = lock_ingest(&state.ingest);
        spawn_ingest_worker(&mut *ingest, runtime_secs);
    }
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
    app_commands::reset_phase1_live_status().map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    {
        let mut ingest = lock_ingest(&state.ingest);
        spawn_ingest_worker(&mut *ingest, state.runtime_start_epoch_secs);
    }
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
    let _ = app_commands::reset_phase1_live_status();

    // Drain IDE context items before handing off to the blocking pipeline.
    let ide_items: Vec<VisualLogItem> = state
        .ide_queue
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .drain(..)
        .collect();

    let ingest = Arc::clone(&state.ingest);
    let runtime_secs = state.runtime_start_epoch_secs;
    let summary = tokio::task::spawn_blocking(move || -> Result<String, String> {
        {
            let mut w = ingest
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            stop_and_join_ingest_worker(&mut *w);
        }
        let latest_log = latest_runtime_capture_log(runtime_secs).map_err(|e| e.to_string())?;
        let latest_log = latest_log.ok_or_else(|| {
            "No runtime capture log found yet. Let phase1 run and interact, then end the session."
                .to_string()
        })?;

        // Merge any queued IDE context items into the capture file so phase2 sees them.
        if !ide_items.is_empty() {
            inject_ide_items_into_log(&latest_log, ide_items)?;
        }

        let session_key = latest_log
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string());
        // Final ingest + stitch (no pipeline_runs rows): catches captures since the last background tick.
        // Record only phase4 as the explicit "end session" summarize step.
        let r = (|| -> Result<String, String> {
            if let Some(ref sk) = session_key {
                std::env::set_var("OMEGA_USAGE_SESSION_KEY", sk);
            }
            app_commands::run_phase2_phase3_ingest_only(latest_log.clone())
                .map_err(|e| format!("final ingest/stitch: {e}"))?;
            if let Some(ref sk) = session_key {
                std::env::set_var("OMEGA_USAGE_SESSION_KEY", sk);
            }
            app_commands::run_pipeline_stage("phase4".to_string(), None)?;
            app_commands::load_generated_summary_text()
        })();
        let _ = std::env::remove_var("OMEGA_USAGE_SESSION_KEY");
        r
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("pipeline task join failed: {e}")))?
    .map_err(|e| {
        let code = if e.contains("No runtime capture log") {
            StatusCode::BAD_REQUEST
        } else {
            StatusCode::INTERNAL_SERVER_ERROR
        };
        (code, e)
    })?;

    Ok(Json(FetchSummaryResponse { summary }))
}

/// Receives structured IDE context from the Omega extension.
/// Builds a synthetic `VisualLogItem` and queues it for injection into the
/// session file at `end_session` time, so it participates in phase2-4 like any
/// other capture.
async fn post_ide_context(
    State(state): State<ApiState>,
    Json(body): Json<IdeContextPushBody>,
) -> Result<Json<IdeContextPushResponse>, (StatusCode, String)> {
    // Build a descriptive OCR-like text from whatever the extension provided.
    let mut text_parts: Vec<String> = Vec::new();
    if let Some(ref file) = body.active_file {
        text_parts.push(format!("File: {file}"));
    }
    if let Some(ref lang) = body.language {
        text_parts.push(format!("Language: {lang}"));
    }
    if let Some(ref branch) = body.git_branch {
        text_parts.push(format!("Branch: {branch}"));
    }
    if let Some(ref code) = body.visible_code {
        if !code.trim().is_empty() {
            text_parts.push(format!("---\n{}", code.trim()));
        }
    }
    if let Some(ref diags) = body.diagnostics {
        if !diags.is_empty() {
            text_parts.push(format!("Diagnostics:\n{}", diags.join("\n")));
        }
    }
    let ocr_text = if text_parts.is_empty() {
        "[ide-context: no content]".to_string()
    } else {
        text_parts.join("\n")
    };

    let app_name = if body.app_name.is_empty() {
        body.ide_label.clone()
    } else {
        body.app_name.clone()
    };
    let window_title = body
        .active_file
        .as_deref()
        .map(|f| {
            if let Some(ref ws) = body.workspace {
                format!("{f} \u{2014} {ws}")
            } else {
                f.to_string()
            }
        })
        .unwrap_or_default();

    // Resolve git branch: use the pushed value if present, else try to detect it.
    let git_branch = body.git_branch.clone().or_else(|| {
        body.workspace.as_deref().and_then(ide_context::get_git_branch)
    });

    let ide_ctx = IdeContext {
        active_file: body.active_file.clone(),
        workspace: body.workspace.clone(),
        language: body.language.clone(),
        git_branch,
        workspace_path: body.workspace.clone(),
    };

    let item = VisualLogItem {
        id: now_epoch_secs(),
        timestamp: SystemTime::now(),
        app_name,
        window_title,
        event_type: "ide-context-push".to_string(),
        width: 0,
        height: 0,
        ocr_engine_used: "ide-extension".to_string(),
        ocr_text,
        ide_context: Some(ide_ctx),
    };

    state
        .ide_queue
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .push(item);

    Ok(Json(IdeContextPushResponse { queued: true }))
}

async fn run_action(
    Json(body): Json<RunActionBody>,
) -> Result<Json<ActionOutputRecord>, (StatusCode, String)> {
    let session_key = body.session_key;
    let action_type = body.action_type;
    let bucket_ids = body.bucket_ids;
    let custom_prompt = body.custom_prompt;
    tokio::task::spawn_blocking(move || {
        app_commands::run_action_command(session_key, action_type, bucket_ids, custom_prompt)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .map(Json)
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))
}

async fn list_action_outputs(
    Query(q): Query<ActionOutputsQuery>,
) -> Result<Json<Vec<ActionOutputRecord>>, (StatusCode, String)> {
    app_commands::list_action_outputs(q.session_key)
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))
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
        ingest: Arc::new(Mutex::new(IngestWorkerState::default())),
        ide_queue: Arc::new(Mutex::new(Vec::new())),
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/api/sessions", get(list_sessions))
        .route("/api/session-summary", get(get_session_summary))
        .route("/api/session-summary", post(save_summary_revision))
        .route("/api/summary-revisions", get(list_summary_revisions))
        .route("/api/pipeline/run", post(run_pipeline))
        .route("/api/pipeline/runs", get(list_pipeline_runs))
        .route("/api/usage", get(get_usage))
        .route("/api/capture/live-status", get(get_phase1_live_status))
        .route("/api/capture/pause", put(put_capture_pause))
        .route("/api/privacy/capture-exclusions", get(get_capture_exclusions))
        .route("/api/privacy/capture-exclusions", put(put_capture_exclusions))
        .route("/api/privacy/storage-manifest", get(storage_manifest))
        .route("/api/privacy/session-data", delete(delete_privacy_session))
        .route("/api/privacy/delete-session", post(post_delete_session))
        .route("/api/privacy/delete-all-local", post(post_delete_all_local))
        .route("/api/session/start", post(start_session))
        .route("/api/session/end", post(end_session))
        .route("/api/fetch-summary", post(fetch_summary))
        .route("/api/action/run", post(run_action))
        .route("/api/action/outputs", get(list_action_outputs))
        .route("/api/ide-context", post(post_ide_context))
        .with_state(shared_state)
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        );

    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("omega-api listening on http://127.0.0.1:{port}");
    axum::serve(listener, app).await?;
    Ok(())
}
