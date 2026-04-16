use crate::actions::{self, ActionBucketInput, ActionSourceRef, ActionType};
use crate::app_db;
use crate::app_models::{
    ActionOutputRecord, CaptureExclusionsState, DeleteLocalDataResponse, PipelineRunRecord,
    SessionBucket, SessionListItem, SessionSummaryState, SourceAttribution, StorageManifest,
    StorageManifestEntry,
};
use crate::capture_live_status::Phase1LiveStatus;
use crate::privacy_config;
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
            source_attribution: Vec::new(),
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
    #[serde(default)]
    #[allow(dead_code)]
    dropped_by_pause: u64,
}

#[derive(Debug, Deserialize)]
struct CaptureSessionFile {
    session_summary: CaptureSessionSummary,
}

fn format_sources_markdown(sources: &[SourceAttribution]) -> String {
    if sources.is_empty() {
        return String::new();
    }
    let parts: Vec<String> = sources
        .iter()
        .map(|s| {
            let w = s.window_title.trim();
            if w.is_empty() {
                s.app_name.clone()
            } else {
                format!("{} — {}", s.app_name, w)
            }
        })
        .collect();
    format!(
        "**Sources (from capture metadata):** {}",
        parts.join("; ")
    )
}

fn logs_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("OMEGA_APP_LOGS_DIR").unwrap_or_else(|_| "logs".to_string()),
    )
}

const STORAGE_RETENTION_NOTE: &str = "All capture logs, edited summaries, pipeline history, and local research databases live in the folder above. \
Remote APIs receive text only when you run embedding or summarization with keys configured. \
Deleting one session removes its capture JSON and saved edits for that session; embeddings and chunks can remain inside phase SQLite files until you delete those databases or use delete all. \
Delete all removes capture logs, app state, and phase databases in this folder but keeps your app exclusion list.";

fn storage_entry_category(name: &str) -> Option<&'static str> {
    if name.starts_with("capture-session-") && name.ends_with(".json") {
        Some("capture_session")
    } else if name == "app_state.db" {
        Some("app_database")
    } else if name.ends_with(".db") || name.ends_with(".db-wal") || name.ends_with(".db-shm") {
        Some("phase_database")
    } else if name == "capture_exclusions.json" {
        Some("privacy_config")
    } else if name == "phase1_live_status.json" {
        Some("live_status")
    } else {
        None
    }
}

/// List on-disk artifacts under the logs directory (paths, sizes) for transparency and erasure.
pub fn get_storage_manifest() -> Result<StorageManifest, String> {
    let root = logs_dir();
    let root_abs = fs::canonicalize(&root).unwrap_or_else(|_| {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(&root)
    });
    let mut entries: Vec<StorageManifestEntry> = Vec::new();
    let mut total: u64 = 0;
    if root_abs.is_dir() {
        for entry in fs::read_dir(&root_abs).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            if name.ends_with(".tmp") || name.starts_with('.') {
                continue;
            }
            if !path.is_file() {
                continue;
            }
            let Some(cat) = storage_entry_category(name) else {
                continue;
            };
            let bytes = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            let rel = path
                .strip_prefix(&root_abs)
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| name.to_string());
            total += bytes;
            entries.push(StorageManifestEntry {
                category: cat.to_string(),
                path: rel,
                absolute_path: path.display().to_string(),
                bytes,
            });
        }
    }
    entries.sort_by(|a, b| a.category.cmp(&b.category).then_with(|| a.path.cmp(&b.path)));
    Ok(StorageManifest {
        logs_root_absolute: root_abs.display().to_string(),
        retention_note: STORAGE_RETENTION_NOTE.to_string(),
        entries,
        total_bytes: total,
    })
}

pub fn delete_session_data(session_key: String) -> Result<DeleteLocalDataResponse, String> {
    if !session_key.starts_with("capture-session-")
        || session_key.contains('/')
        || session_key.contains('\\')
        || session_key.contains("..")
    {
        return Err("invalid session key".to_string());
    }
    let json_path = logs_dir().join(format!("{session_key}.json"));
    let conn = app_db::open_app_db(&app_db_path()).map_err(|e| e.to_string())?;
    app_db::delete_session_records(&conn, &session_key).map_err(|e| e.to_string())?;
    if json_path.is_file() {
        fs::remove_file(&json_path).map_err(|e| e.to_string())?;
    }
    Ok(DeleteLocalDataResponse {
        ok: true,
        restart_recommended: false,
        message: "Removed this session’s capture log and saved summary data.".to_string(),
    })
}

/// Erase capture JSON files, SQLite databases (app + phase), and live status. Preserves `capture_exclusions.json`.
pub fn delete_all_local_session_data() -> Result<DeleteLocalDataResponse, String> {
    let root = logs_dir();
    let root_path = fs::canonicalize(&root).unwrap_or_else(|_| {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(&root)
    });
    if root_path.is_dir() {
        for entry in fs::read_dir(&root_path).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            if !path.is_file() {
                continue;
            }
            let wipe = (name.starts_with("capture-session-") && name.ends_with(".json"))
                || name.ends_with(".db")
                || name.ends_with(".db-wal")
                || name.ends_with(".db-shm")
                || name == "phase1_live_status.json";
            if wipe {
                let _ = fs::remove_file(&path);
            }
        }
    }
    crate::capture_live_status::reset().map_err(|e| e.to_string())?;
    let app_path = app_db_path();
    if !app_path.exists() {
        let _ = app_db::open_app_db(&app_path).map_err(|e| e.to_string())?;
    }
    Ok(DeleteLocalDataResponse {
        ok: true,
        restart_recommended: true,
        message: "Removed local capture logs, summaries, pipeline history, and research databases. App exclusions were kept. Restart Omega (quit and reopen) before running ingest or summarize again."
            .to_string(),
    })
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
            "SELECT s.bucket_id, s.summary_json, s.generated_at_epoch_secs
             FROM task_bucket_summaries s
             JOIN task_buckets b ON b.bucket_id = s.bucket_id
             ORDER BY s.bucket_id ASC
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
        let mut source_attribution: Vec<SourceAttribution> = parsed
            .get("source_attribution")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        if source_attribution.is_empty() {
            source_attribution = phase4::bucket_source_attribution(&conn, bucket_id)
                .unwrap_or_default()
                .into_iter()
                .map(|r| SourceAttribution {
                    app_name: r.app_name,
                    window_title: r.window_title,
                })
                .collect();
        }
        let sources_md = format_sources_markdown(&source_attribution);
        let core_body = format!("{one_liner}\n\n{detail}").trim().to_string();
        let body = if sources_md.is_empty() {
            core_body
        } else {
            format!("{core_body}\n\n{sources_md}")
        };
        buckets.push(SessionBucket {
            bucket_id,
            title,
            body,
            source_attribution,
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
            "SELECT c.chunk_text, cap.app_name, cap.window_title
             FROM chunks c
             JOIN captures cap ON cap.canonical_hash = c.canonical_hash
             ORDER BY cap.timestamp_epoch_secs ASC, c.chunk_index ASC",
        )
        .context("prepare chunk fallback query")?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;

    let mut parts: Vec<String> = Vec::new();
    let mut total: usize = 0;
    const MAX_CHARS: usize = 48_000;
    for row in rows {
        let (chunk_text, app_name, window_title) = row.context("chunk fallback row")?;
        let head = {
            let w = window_title.trim();
            if w.is_empty() {
                app_name.clone()
            } else {
                format!("{app_name} — {w}")
            }
        };
        let block = format!("### {head}\n\n{}\n\n", chunk_text.trim());
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

const FALLBACK_MARKER: &str = "## Summary (from ingested text)";

fn body_is_fallback(body: &str) -> bool {
    body.trim().starts_with(FALLBACK_MARKER)
}

fn load_summary_state_impl(session_key: &str) -> Result<SessionSummaryState> {
    let app_db = app_db::open_app_db(&app_db_path())?;
    if let Some((summary_id, title, body, source_bucket_ids)) =
        app_db::get_summary_row(&app_db, session_key)?
    {
        // Re-check the phase DB when:
        //  a) the cached body is a Phase-4-failure fallback, OR
        //  b) the phase DB now has different bucket IDs (pipeline re-ran with refinement/splitting).
        let should_refresh = body_is_fallback(&body) || {
            let db = phase_db_path();
            if db.exists() {
                load_generated_summary(&db)
                    .map(|(_, fresh_ids, fresh_buckets)| {
                        !fresh_buckets.is_empty() && fresh_ids != source_bucket_ids
                    })
                    .unwrap_or(false)
            } else {
                false
            }
        };

        if should_refresh {
            let (fresh_body, fresh_ids, fresh_buckets) =
                load_generated_summary(&phase_db_path())?;
            if !fresh_buckets.is_empty() {
                app_db::upsert_current_summary(
                    &app_db,
                    session_key,
                    &title,
                    &fresh_body,
                    &fresh_ids,
                )?;
                app_db::insert_revision(
                    &app_db,
                    summary_id,
                    &title,
                    &fresh_body,
                    "system-regenerated",
                )?;
                let revisions = app_db::list_revisions(&app_db, summary_id)?;
                return Ok(SessionSummaryState {
                    session_key: session_key.to_string(),
                    title,
                    body: fresh_body,
                    source_bucket_ids: fresh_ids,
                    revisions,
                    buckets: fresh_buckets,
                });
            }
        }

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
    let generated_title = "Untitled session".to_string();
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
    let app_db = app_db::open_app_db(&app_db_path()).map_err(|e| e.to_string())?;
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
        let summary_title = app_db::get_summary_row(&app_db, &session_key)
            .map_err(|e| e.to_string())?
            .map(|(_, title, _, _)| title);
        sessions.push(SessionListItem {
            session_key,
            file_path: path.display().to_string(),
            started_at_epoch_secs: parsed.session_summary.session_start_epoch_secs,
            ended_at_epoch_secs: parsed.session_summary.session_end_epoch_secs,
            duration_secs: parsed.session_summary.session_duration_secs,
            accepted_captures: parsed.session_summary.accepted_captures,
            total_events_seen: parsed.session_summary.total_events_seen,
            summary_title,
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

/// Run phase 2 (embed new captures) and phase 3 (bucket assignment) without writing `pipeline_runs` rows.
/// Used for incremental processing while a capture session is active; end-of-session still uses
/// `run_pipeline_stage` so the UI shows explicit completion runs.
pub fn run_phase2_phase3_ingest_only(input_log_path: PathBuf) -> Result<(), String> {
    if let Some(sk) = input_log_path.file_stem().and_then(|s| s.to_str()) {
        std::env::set_var("OMEGA_USAGE_SESSION_KEY", sk);
    }
    let input_str = input_log_path.display().to_string();
    let res = run_pipeline_stage_impl("phase2", Some(input_str))
        .map_err(|e| e.to_string())
        .and_then(|_| run_pipeline_stage_impl("phase3", None).map_err(|e| e.to_string()));
    let _ = std::env::remove_var("OMEGA_USAGE_SESSION_KEY");
    res
}

/// Run Phase 4 with one automatic retry (with --force) when all bucket summaries fail on the
/// first attempt (typically a transient Gemini API error).
fn run_phase4_with_retry(sk: Option<&str>) -> Result<()> {
    let cfg = phase4::SummarizeConfig::from_env_and_args(None, None, false, false)?;
    let (_path, summary) = phase4::run_summarization(cfg)?;
    usage::record_phase4(&summary, sk)?;
    if summary.summaries_failed > 0 && summary.summaries_written == 0 && summary.buckets_total > 0
    {
        eprintln!(
            "omega: phase4 all {} bucket(s) failed; retrying once with --force",
            summary.buckets_total
        );
        let cfg2 = phase4::SummarizeConfig::from_env_and_args(None, None, true, false)?;
        let (_path2, summary2) = phase4::run_summarization(cfg2)?;
        usage::record_phase4(&summary2, sk)?;
        if summary2.summaries_written == 0 && summary2.buckets_total > 0 {
            anyhow::bail!(
                "phase4 failed for all {} bucket(s) after retry",
                summary2.buckets_total
            );
        }
    }
    Ok(())
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
            run_phase4_with_retry(sk)?;
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

pub fn get_capture_exclusions() -> Result<CaptureExclusionsState, String> {
    let c = privacy_config::load_capture_exclusions().map_err(|e| e.to_string())?;
    Ok(CaptureExclusionsState {
        excluded_app_names: c.excluded_app_names,
    })
}

pub fn set_capture_exclusions(names: Vec<String>) -> Result<CaptureExclusionsState, String> {
    let cfg = privacy_config::CaptureExclusionsConfig {
        excluded_app_names: names,
    };
    privacy_config::save_capture_exclusions(&cfg).map_err(|e| e.to_string())?;
    let loaded = privacy_config::load_capture_exclusions().map_err(|e| e.to_string())?;
    Ok(CaptureExclusionsState {
        excluded_app_names: loaded.excluded_app_names,
    })
}

pub fn get_phase1_live_status() -> Result<Phase1LiveStatus, String> {
    crate::capture_live_status::read_or_default().map_err(|e| e.to_string())
}

pub fn reset_phase1_live_status() -> Result<(), String> {
    crate::capture_live_status::reset().map_err(|e| e.to_string())
}

pub fn set_capture_paused(paused: bool) -> Result<Phase1LiveStatus, String> {
    crate::capture_live_status::set_capture_paused(paused).map_err(|e| e.to_string())?;
    get_phase1_live_status()
}

// ── Actions (Phase 5) ──────────────────────────────────────────────

pub fn run_action_command(
    session_key: String,
    action_type_str: String,
    bucket_ids: Option<Vec<i64>>,
    custom_prompt: Option<String>,
) -> Result<ActionOutputRecord, String> {
    let action = ActionType::from_str(&action_type_str).map_err(|e| e.to_string())?;
    let app_db_conn = app_db::open_app_db(&app_db_path()).map_err(|e| e.to_string())?;

    let (_, _, body, _) = app_db::get_summary_row(&app_db_conn, &session_key)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("no summary found for session '{session_key}'"))?;

    let all_buckets = buckets_from_body(&body);
    if all_buckets.is_empty() {
        return Err("no bucket summaries available for this session".to_string());
    }

    let selected: Vec<&SessionBucket> = if let Some(ref ids) = bucket_ids {
        all_buckets
            .iter()
            .filter(|b| ids.contains(&b.bucket_id))
            .collect()
    } else {
        all_buckets.iter().collect()
    };

    if selected.is_empty() {
        return Err("no matching buckets found for the given IDs".to_string());
    }

    let action_inputs: Vec<ActionBucketInput> = selected
        .iter()
        .map(|b| {
            let phase_db = phase_db_path();
            let (tags, primary_apps, one_liner, detailed_summary) =
                load_bucket_detail_from_phase_db(&phase_db, b.bucket_id);
            ActionBucketInput {
                bucket_id: b.bucket_id,
                title: b.title.clone(),
                one_liner,
                detailed_summary,
                tags,
                primary_apps,
                source_attribution: b
                    .source_attribution
                    .iter()
                    .map(|s| ActionSourceRef {
                        app_name: s.app_name.clone(),
                        window_title: s.window_title.clone(),
                    })
                    .collect(),
            }
        })
        .collect();

    let used_ids: Vec<i64> = selected.iter().map(|b| b.bucket_id).collect();

    let output =
        actions::run_action(&session_key, action, &action_inputs, &used_ids, custom_prompt.as_deref())
            .map_err(|e| format!("action failed: {e:#}"))?;

    let id = app_db::insert_action_output(
        &app_db_conn,
        &session_key,
        action.as_str(),
        &output.input_bucket_ids,
        &output.output_body,
        &output.model,
    )
    .map_err(|e| e.to_string())?;

    Ok(ActionOutputRecord {
        id,
        session_key: output.session_key,
        action_type: output.action_type,
        input_bucket_ids: output.input_bucket_ids,
        output_body: output.output_body,
        model: output.model,
        generated_at_epoch_secs: output.generated_at_epoch_secs,
    })
}

pub fn list_action_outputs(session_key: String) -> Result<Vec<ActionOutputRecord>, String> {
    let conn = app_db::open_app_db(&app_db_path()).map_err(|e| e.to_string())?;
    app_db::list_action_outputs(&conn, &session_key).map_err(|e| e.to_string())
}

/// Best-effort extraction of structured fields from the phase DB's `task_bucket_summaries`.
fn load_bucket_detail_from_phase_db(
    db_path: &std::path::Path,
    bucket_id: i64,
) -> (Vec<String>, Vec<String>, String, String) {
    let empty = (Vec::new(), Vec::new(), String::new(), String::new());
    let Ok(conn) = rusqlite::Connection::open(db_path) else {
        return empty;
    };
    let row: Option<String> = conn
        .query_row(
            "SELECT summary_json FROM task_bucket_summaries WHERE bucket_id = ?1",
            rusqlite::params![bucket_id],
            |row| row.get(0),
        )
        .ok();
    let Some(json_str) = row else { return empty };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&json_str) else {
        return empty;
    };
    let tags = v
        .get("tags")
        .and_then(|t| t.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let primary_apps = v
        .get("primary_apps")
        .and_then(|t| t.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let one_liner = v
        .get("one_liner")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let detailed_summary = v
        .get("detailed_summary")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    (tags, primary_apps, one_liner, detailed_summary)
}
