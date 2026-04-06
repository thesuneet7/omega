use crate::app_db;
use crate::app_models::{PipelineRunRecord, SessionBucket, SessionListItem, SessionSummaryState};
use crate::usage;
use crate::{phase2, phase3, phase4};
use anyhow::{Context, Result};
use regex::Regex;
use rusqlite::Connection;
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct StoredSummaryV1 {
    version: u32,
    buckets: Vec<SessionBucket>,
}

fn encode_buckets_storage(buckets: &[SessionBucket]) -> Result<String> {
    serde_json::to_string(&StoredSummaryV1 {
        version: 1,
        buckets: buckets.to_vec(),
    })
    .context("encode summary storage")
}

fn parse_legacy_bucket_markdown(body: &str) -> Option<Vec<SessionBucket>> {
    if !body.contains("## Bucket ") {
        return None;
    }
    let re = Regex::new(r"(?m)^## Bucket (\d+): (.+)$").ok()?;
    let mut spans: Vec<(usize, usize, i64, String)> = Vec::new();
    for cap in re.captures_iter(body) {
        let whole = cap.get(0)?;
        let id: i64 = cap.get(1)?.as_str().parse().ok()?;
        let title = cap.get(2)?.as_str().to_string();
        spans.push((whole.start(), whole.end(), id, title));
    }
    if spans.is_empty() {
        return None;
    }
    let mut out = Vec::new();
    for i in 0..spans.len() {
        let (_, header_end, id, title) = &spans[i];
        let body_end = spans.get(i + 1).map(|s| s.0).unwrap_or(body.len());
        let body_text = body[*header_end..body_end].trim().to_string();
        out.push(SessionBucket {
            bucket_id: *id,
            title: title.clone(),
            body: body_text,
        });
    }
    Some(out)
}

fn buckets_from_body(body: &str) -> Vec<SessionBucket> {
    let t = body.trim();
    if t.starts_with('{') {
        if let Ok(v) = serde_json::from_str::<StoredSummaryV1>(t) {
            if v.version == 1 {
                return v.buckets;
            }
        }
    }
    parse_legacy_bucket_markdown(body).unwrap_or_default()
}

#[derive(Debug, Deserialize)]
struct CaptureSessionSummary {
    session_start_epoch_secs: u64,
    session_end_epoch_secs: u64,
    session_duration_secs: u64,
    total_events_seen: u64,
    accepted_captures: u64,
}

#[derive(Debug, Deserialize)]
struct CaptureSessionFile {
    session_summary: CaptureSessionSummary,
}

fn logs_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("OMEGA_APP_LOGS_DIR").unwrap_or_else(|_| "logs".to_string()),
    )
}

fn app_db_path() -> PathBuf {
    logs_dir().join("app_state.db")
}

fn phase_db_path() -> PathBuf {
    PathBuf::from(
        std::env::var("OMEGA_PHASE4_DB_PATH").unwrap_or_else(|_| "logs/phase2.db".to_string()),
    )
}

fn collect_session_files(base_dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    if !base_dir.exists() {
        return Ok(files);
    }
    for entry in fs::read_dir(base_dir).with_context(|| format!("read {}", base_dir.display()))? {
        let entry = entry.context("read logs dir entry")?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if name.starts_with("capture-session-") && name.ends_with(".json") {
            files.push(path);
        }
    }
    files.sort();
    files.reverse();
    Ok(files)
}

fn load_generated_summary(db_path: &Path) -> Result<(String, Vec<i64>, Vec<SessionBucket>)> {
    if !db_path.exists() {
        return Ok((
            "Run phase2 -> phase3 -> phase4 to generate summaries.".to_string(),
            Vec::new(),
            Vec::new(),
        ));
    }
    let conn = Connection::open(db_path)
        .with_context(|| format!("open phase db '{}'", db_path.display()))?;
    conn.busy_timeout(Duration::from_secs(5))
        .context("set sqlite busy timeout")?;
    // Table is created by phase4; ensure it exists before read (avoids "no such table" if the DB
    // was only ingested/stitched, or if reads race a fresh file).
    phase4::init_phase4_schema(&conn).context("ensure phase4 schema")?;
    let mut stmt = conn
        .prepare(
            "SELECT bucket_id, summary_json, generated_at_epoch_secs
             FROM task_bucket_summaries
             ORDER BY bucket_id ASC
             LIMIT 8",
        )
        .context("prepare task_bucket_summaries query")?;

    let rows = stmt
        .query_map([], |row| {
            let bucket_id: i64 = row.get(0)?;
            let summary_json: String = row.get(1)?;
            Ok((bucket_id, summary_json))
        })
        .context("query summaries")?;

    let mut buckets = Vec::new();
    let mut ids = Vec::new();
    for row in rows {
        let (bucket_id, summary_json) = row.context("decode summary row")?;
        ids.push(bucket_id);
        let parsed: serde_json::Value = serde_json::from_str(&summary_json).unwrap_or_default();
        let title = parsed
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("Untitled")
            .to_string();
        let one_liner = parsed
            .get("one_liner")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let detail = parsed
            .get("detailed_summary")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let body = format!("{one_liner}\n\n{detail}").trim().to_string();
        buckets.push(SessionBucket {
            bucket_id,
            title,
            body,
        });
    }

    if buckets.is_empty() {
        if let Some(fallback) = fallback_summary_from_ingested_chunks(db_path)? {
            return Ok((fallback, ids, Vec::new()));
        }
        return Ok((
            "No bucket summaries in phase DB yet. Run phase4 first.".to_string(),
            ids,
            Vec::new(),
        ));
    }
    let storage = encode_buckets_storage(&buckets)?;
    Ok((storage, ids, buckets))
}

/// When Phase 4 wrote nothing (no buckets, API failures, or skipped), still return useful text
/// from ingested chunks so the fetch UI is not empty.
fn fallback_summary_from_ingested_chunks(db_path: &Path) -> Result<Option<String>> {
    if !db_path.exists() {
        return Ok(None);
    }
    let conn = Connection::open(db_path)
        .with_context(|| format!("open phase db for chunk fallback '{}'", db_path.display()))?;
    let mut stmt = conn
        .prepare(
            "SELECT c.chunk_text, cap.app_name
             FROM chunks c
             JOIN captures cap ON cap.canonical_hash = c.canonical_hash
             ORDER BY cap.timestamp_epoch_secs ASC, c.chunk_index ASC",
        )
        .context("prepare chunk fallback query")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;

    let mut parts: Vec<String> = Vec::new();
    let mut total: usize = 0;
    const MAX_CHARS: usize = 48_000;
    for row in rows {
        let (chunk_text, app_name) = row.context("chunk fallback row")?;
        let block = format!("### {app_name}\n\n{}\n\n", chunk_text.trim());
        if total + block.len() > MAX_CHARS {
            break;
        }
        total += block.len();
        parts.push(block);
    }

    if parts.is_empty() {
        return Ok(None);
    }
    Ok(Some(format!(
        "## Summary (from ingested text)\n\n\
         Phase 4 did not persist bucket summaries (no buckets, API failures, or empty run). \
         Below is text recovered from Phase 2 chunks.\n\n\
         {}",
        parts.join("")
    )))
}

pub fn load_generated_summary_text() -> Result<String, String> {
    let (storage, _, buckets) = load_generated_summary(&phase_db_path()).map_err(|e| e.to_string())?;
    if buckets.is_empty() {
        return Ok(storage);
    }
    Ok(buckets
        .iter()
        .map(|b| format!("## {}\n\n{}", b.title, b.body.trim()))
        .collect::<Vec<_>>()
        .join("\n\n"))
}

fn load_summary_state_impl(session_key: &str) -> Result<SessionSummaryState> {
    let app_db = app_db::open_app_db(&app_db_path())?;
    if let Some((summary_id, title, body, source_bucket_ids)) =
        app_db::get_summary_row(&app_db, session_key)?
    {
        let revisions = app_db::list_revisions(&app_db, summary_id)?;
        let buckets = buckets_from_body(&body);
        return Ok(SessionSummaryState {
            session_key: session_key.to_string(),
            title,
            body,
            source_bucket_ids,
            revisions,
            buckets,
        });
    }

    let (generated_body, source_bucket_ids, buckets) = load_generated_summary(&phase_db_path())?;
    let generated_title = format!("Session Summary - {session_key}");
    let summary_id = app_db::upsert_current_summary(
        &app_db,
        session_key,
        &generated_title,
        &generated_body,
        &source_bucket_ids,
    )?;
    app_db::insert_revision(
        &app_db,
        summary_id,
        &generated_title,
        &generated_body,
        "system-generated",
    )?;
    let revisions = app_db::list_revisions(&app_db, summary_id)?;
    Ok(SessionSummaryState {
        session_key: session_key.to_string(),
        title: generated_title,
        body: generated_body,
        source_bucket_ids,
        revisions,
        buckets,
    })
}

pub fn list_sessions() -> Result<Vec<SessionListItem>, String> {
    let files = collect_session_files(&logs_dir()).map_err(|e| e.to_string())?;
    let mut sessions = Vec::new();
    for path in files {
        let raw = fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
        let parsed: CaptureSessionFile =
            serde_json::from_str(&raw).map_err(|e| format!("parse {}: {e}", path.display()))?;
        let session_key = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown-session")
            .to_string();
        sessions.push(SessionListItem {
            session_key,
            file_path: path.display().to_string(),
            started_at_epoch_secs: parsed.session_summary.session_start_epoch_secs,
            ended_at_epoch_secs: parsed.session_summary.session_end_epoch_secs,
            duration_secs: parsed.session_summary.session_duration_secs,
            accepted_captures: parsed.session_summary.accepted_captures,
            total_events_seen: parsed.session_summary.total_events_seen,
        });
    }
    Ok(sessions)
}

pub fn get_session_summary(session_key: String) -> Result<SessionSummaryState, String> {
    load_summary_state_impl(&session_key).map_err(|e| e.to_string())
}

pub fn save_summary_revision(
    session_key: String,
    title: String,
    body: String,
    editor_label: Option<String>,
) -> Result<SessionSummaryState, String> {
    let conn = app_db::open_app_db(&app_db_path()).map_err(|e| e.to_string())?;
    let existing = app_db::get_summary_row(&conn, &session_key).map_err(|e| e.to_string())?;
    let source_bucket_ids = existing
        .as_ref()
        .map(|(_, _, _, ids)| ids.clone())
        .unwrap_or_default();
    let summary_id = app_db::upsert_current_summary(&conn, &session_key, &title, &body, &source_bucket_ids)
        .map_err(|e| e.to_string())?;
    app_db::insert_revision(
        &conn,
        summary_id,
        &title,
        &body,
        editor_label.as_deref().unwrap_or("local-user"),
    )
    .map_err(|e| e.to_string())?;
    let revisions = app_db::list_revisions(&conn, summary_id).map_err(|e| e.to_string())?;
    let buckets = buckets_from_body(&body);
    Ok(SessionSummaryState {
        session_key,
        title,
        body,
        source_bucket_ids,
        revisions,
        buckets,
    })
}

pub fn list_summary_revisions(session_key: String) -> Result<Vec<crate::app_models::SummaryRevision>, String> {
    let conn = app_db::open_app_db(&app_db_path()).map_err(|e| e.to_string())?;
    let Some((summary_id, _, _, _)) =
        app_db::get_summary_row(&conn, &session_key).map_err(|e| e.to_string())?
    else {
        return Ok(Vec::new());
    };
    app_db::list_revisions(&conn, summary_id).map_err(|e| e.to_string())
}

fn run_pipeline_stage_impl(stage: &str, input_ref: Option<String>) -> Result<()> {
    let session_key = std::env::var("OMEGA_USAGE_SESSION_KEY").ok().or_else(|| {
        input_ref
            .as_ref()
            .and_then(|p| usage::session_key_from_input_ref(p))
    });
    let sk = session_key.as_deref();

    match stage {
        "phase2" => {
            let cfg = phase2::IngestionConfig::from_env_and_args(
                input_ref.map(PathBuf::from),
                None,
            )?;
            let (_path, summary) = phase2::run_ingestion(cfg)?;
            usage::record_phase2(&summary, sk)?;
            Ok(())
        }
        "phase3" => {
            let cfg = phase3::StitchConfig::from_env_and_args(None, None)?;
            phase3::run_stitching(cfg)?;
            Ok(())
        }
        "phase4" => {
            let cfg = phase4::SummarizeConfig::from_env_and_args(None, None, false, false)?;
            let (_path, summary) = phase4::run_summarization(cfg)?;
            usage::record_phase4(&summary, sk)?;
            Ok(())
        }
        _ => anyhow::bail!("unknown stage '{stage}'"),
    }
}

pub fn run_pipeline_stage(stage: String, input_ref: Option<String>) -> Result<PipelineRunRecord, String> {
    let conn = app_db::open_app_db(&app_db_path()).map_err(|e| e.to_string())?;
    let input = input_ref.clone().unwrap_or_default();
    let run_id = app_db::start_pipeline_run(&conn, &stage, &input).map_err(|e| e.to_string())?;
    let started_at = app_db::now_epoch_secs();

    let run_res = run_pipeline_stage_impl(&stage, input_ref);
    match run_res {
        Ok(()) => {
            app_db::finish_pipeline_run(&conn, run_id, "succeeded", None).map_err(|e| e.to_string())?;
            Ok(PipelineRunRecord {
                id: run_id,
                stage,
                input_ref: input,
                status: "succeeded".to_string(),
                started_at_epoch_secs: started_at,
                ended_at_epoch_secs: Some(app_db::now_epoch_secs()),
                error_text: None,
            })
        }
        Err(e) => {
            let msg = e.to_string();
            app_db::finish_pipeline_run(&conn, run_id, "failed", Some(&msg)).map_err(|x| x.to_string())?;
            Ok(PipelineRunRecord {
                id: run_id,
                stage,
                input_ref: input,
                status: "failed".to_string(),
                started_at_epoch_secs: started_at,
                ended_at_epoch_secs: Some(app_db::now_epoch_secs()),
                error_text: Some(msg),
            })
        }
    }
}

pub fn list_pipeline_runs(limit: Option<u32>) -> Result<Vec<PipelineRunRecord>, String> {
    let conn = app_db::open_app_db(&app_db_path()).map_err(|e| e.to_string())?;
    app_db::list_pipeline_runs(&conn, limit.unwrap_or(25) as usize).map_err(|e| e.to_string())
}

pub fn get_api_usage(session_key: Option<String>) -> Result<usage::ApiUsageResponse, String> {
    usage::get_api_usage_response(session_key.as_deref()).map_err(|e| e.to_string())
}
