//! Phase 4: Task-bucket summarization — turns stitched buckets into human-readable,
//! structured summaries with idempotent persistence (production-style: fingerprints,
//! retries, versioned prompts, JSON artifacts).

use anyhow::{anyhow, Context, Result};
use reqwest::blocking::Response;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;
use std::thread::sleep;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Bump when the prompt or output schema changes materially (stored in DB + JSON artifact).
pub const PROMPT_VERSION: &str = "phase4-v2";

/// Capture metadata pair stored on bucket summaries (same JSON shape as `app_models::SourceAttribution`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BucketSourceRef {
    pub app_name: String,
    pub window_title: String,
}

#[derive(Debug, Clone)]
pub struct SummarizeConfig {
    pub db_path: PathBuf,
    pub output_path: Option<PathBuf>,
    pub backend: SummarizeBackend,
    pub gemini_base_url: String,
    pub gemini_api_key: Option<String>,
    pub model: String,
    pub max_input_chars: usize,
    pub max_retries: usize,
    pub retry_base_delay_ms: u64,
    pub force: bool,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummarizeBackend {
    Gemini,
    /// Deterministic local summaries — no network; for pipeline tests and offline dev.
    Stub,
}

impl SummarizeBackend {
    fn from_env() -> Result<Self> {
        let raw = std::env::var("OMEGA_PHASE4_BACKEND").unwrap_or_else(|_| "gemini".to_string());
        match raw.trim().to_ascii_lowercase().as_str() {
            "gemini" => Ok(Self::Gemini),
            "stub" => Ok(Self::Stub),
            other => Err(anyhow!(
                "unsupported OMEGA_PHASE4_BACKEND='{other}', expected 'gemini' or 'stub'"
            )),
        }
    }
}

impl SummarizeConfig {
    pub fn from_env_and_args(
        db_path: Option<PathBuf>,
        output_path: Option<PathBuf>,
        force: bool,
        dry_run: bool,
    ) -> Result<Self> {
        let backend = SummarizeBackend::from_env()?;
        let db_path = db_path.unwrap_or_else(|| {
            PathBuf::from(
                std::env::var("OMEGA_PHASE4_DB_PATH")
                    .unwrap_or_else(|_| "logs/phase2.db".to_string()),
            )
        });
        let gemini_base_url = std::env::var("OMEGA_GEMINI_BASE_URL")
            .unwrap_or_else(|_| "https://generativelanguage.googleapis.com".to_string());
        let gemini_api_key = std::env::var("OMEGA_GEMINI_API_KEY").ok();
        let model = std::env::var("OMEGA_PHASE4_MODEL").unwrap_or_else(|_| {
            // Stable, lowest-cost Flash tier on the Gemini API (2.0 Flash is deprecated).
            "gemini-2.5-flash-lite".to_string()
        });
        let max_input_chars = std::env::var("OMEGA_PHASE4_MAX_INPUT_CHARS")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(48_000)
            .max(4_000);
        let max_retries = std::env::var("OMEGA_PHASE4_MAX_RETRIES")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(3);
        let retry_base_delay_ms = std::env::var("OMEGA_PHASE4_RETRY_BASE_DELAY_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(800);

        let force = force || env_flag_true("OMEGA_PHASE4_FORCE");
        let dry_run = dry_run || env_flag_true("OMEGA_PHASE4_DRY_RUN");

        Ok(Self {
            db_path,
            output_path,
            backend,
            gemini_base_url,
            gemini_api_key,
            model,
            max_input_chars,
            max_retries,
            retry_base_delay_ms,
            force,
            dry_run,
        })
    }
}

fn env_flag_true(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .map(|v| {
            matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes")
        })
        .unwrap_or(false)
}

#[derive(Debug, Serialize)]
pub struct Phase4Summary {
    pub db_path: String,
    pub generated_at_epoch_secs: u64,
    pub prompt_version: &'static str,
    pub backend: String,
    pub model: String,
    pub buckets_total: usize,
    pub summaries_written: usize,
    pub summaries_skipped_unchanged: usize,
    pub summaries_failed: usize,
    /// Chars sent to the LLM (Gemini only; excludes dry-run and stub).
    #[serde(default)]
    pub llm_input_chars: usize,
    /// Approximate response size (serialized summary JSON chars; Gemini only).
    #[serde(default)]
    pub llm_output_chars: usize,
    pub dry_run: bool,
    pub force: bool,
    pub max_input_chars: usize,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BucketSummaryRecord {
    pub bucket_id: i64,
    pub title: String,
    pub one_liner: String,
    pub detailed_summary: String,
    /// Distinct app + window title pairs from capture metadata (not inferred by the LLM).
    #[serde(default)]
    pub source_attribution: Vec<BucketSourceRef>,
    pub primary_apps: Vec<String>,
    pub tags: Vec<String>,
    pub confidence_0_1: f32,
    pub caveats: Option<String>,
    pub input_fingerprint: String,
    pub chunk_count: usize,
    pub input_chars: usize,
    pub input_truncated: bool,
    pub first_seen_epoch_secs: Option<u64>,
    pub last_seen_epoch_secs: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct Phase4Output {
    pub summary: Phase4Summary,
    pub summaries: Vec<BucketSummaryRecord>,
}

#[derive(Debug, Deserialize)]
struct GeminiGenerateResponse {
    candidates: Option<Vec<GeminiCandidate>>,
}

#[derive(Debug, Deserialize)]
struct GeminiCandidate {
    content: Option<GeminiContent>,
}

#[derive(Debug, Deserialize)]
struct GeminiContent {
    parts: Option<Vec<GeminiPart>>,
}

#[derive(Debug, Deserialize)]
struct GeminiPart {
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LlmJsonShape {
    title: Option<String>,
    one_liner: Option<String>,
    /// Short second-person framing (1–3 sentences). Optional if `key_points` is exhaustive.
    detailed_summary: Option<String>,
    /// One discrete takeaway per entry; merged into stored `detailed_summary` as markdown bullets.
    key_points: Option<Vec<String>>,
    primary_apps: Option<Vec<String>>,
    tags: Option<Vec<String>>,
    confidence_0_1: Option<f32>,
    caveats: Option<String>,
}

pub fn run_summarization(config: SummarizeConfig) -> Result<(PathBuf, Phase4Summary)> {
    let gemini_key: Option<String> = match config.backend {
        SummarizeBackend::Gemini => Some(
            config
                .gemini_api_key
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow!("missing OMEGA_GEMINI_API_KEY (required for phase4 backend=gemini)"))?
                .to_string(),
        ),
        SummarizeBackend::Stub => None,
    };
    run_summarization_inner(config, gemini_key)
}

fn run_summarization_inner(config: SummarizeConfig, gemini_key: Option<String>) -> Result<(PathBuf, Phase4Summary)> {
    let conn = Connection::open(&config.db_path)
        .with_context(|| format!("failed to open sqlite db '{}'", config.db_path.display()))?;
    init_phase4_schema(&conn)?;

    let buckets = list_buckets(&conn)?;
    let buckets_total = buckets.len();
    let mut summaries: Vec<BucketSummaryRecord> = Vec::new();
    let mut written = 0usize;
    let mut skipped = 0usize;
    let mut failed = 0usize;
    let mut llm_input_chars = 0usize;
    let mut llm_output_chars = 0usize;

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .context("failed to create HTTP client for phase4")?;

    for bucket in buckets {
        let agg = aggregate_bucket(&conn, bucket.bucket_id)?;
        if agg.chunk_hashes.is_empty() {
            eprintln!(
                "phase4: bucket {} has no assigned chunks; skipping",
                bucket.bucket_id
            );
            continue;
        }
        let fingerprint = fingerprint_for_bucket(&agg.chunk_hashes);
        let want_backend = backend_label(&config.backend);
        if !config.force {
            if let Some((existing_fp, stored_prompt_ver, stored_backend, stored_model)) =
                existing_summary_meta(&conn, bucket.bucket_id)?
            {
                // Must match backend/model too — otherwise switching stub→gemini (or changing
                // OMEGA_PHASE4_MODEL) would still skip and leave stale JSON in the artifact.
                if existing_fp == fingerprint
                    && stored_prompt_ver == PROMPT_VERSION
                    && stored_backend == want_backend
                    && stored_model == config.model
                {
                    skipped += 1;
                    if let Some(row) = load_summary_row(&conn, bucket.bucket_id)? {
                        summaries.push(row);
                    }
                    continue;
                }
            }
        }

        if config.dry_run {
            let (body, truncated) = build_prompt_body(&agg, config.max_input_chars);
            written += 1;
            summaries.push(BucketSummaryRecord {
                bucket_id: bucket.bucket_id,
                title: format!("[dry-run] bucket {}", bucket.bucket_id),
                one_liner: "dry run — no LLM call".to_string(),
                detailed_summary: String::new(),
                source_attribution: agg.source_attribution.clone(),
                primary_apps: agg.distinct_apps.clone(),
                tags: vec!["dry-run".to_string()],
                confidence_0_1: 0.0,
                caveats: Some("OMEGA_PHASE4_DRY_RUN or --dry-run".to_string()),
                input_fingerprint: fingerprint.clone(),
                chunk_count: agg.chunk_hashes.len(),
                input_chars: body.len(),
                input_truncated: truncated,
                first_seen_epoch_secs: agg.first_seen_epoch_secs,
                last_seen_epoch_secs: agg.last_seen_epoch_secs,
            });
            continue;
        }

        let (body, truncated) = build_prompt_body(&agg, config.max_input_chars);
        let user_prompt = build_user_prompt(&agg, &body, truncated);

        let result: Result<BucketSummaryRecord> = match config.backend {
            SummarizeBackend::Stub => Ok(stub_summary(
                bucket.bucket_id,
                &agg,
                &fingerprint,
                body.len(),
                truncated,
            )),
            SummarizeBackend::Gemini => {
                let key = gemini_key.as_ref().expect("gemini key checked");
                match gemini_summarize(
                    &client,
                    &config.gemini_base_url,
                    key,
                    &config.model,
                    &user_prompt,
                    config.max_retries,
                    config.retry_base_delay_ms,
                ) {
                    Ok(parsed) => Ok(llm_to_record(
                        bucket.bucket_id,
                        parsed,
                        &fingerprint,
                        agg.chunk_hashes.len(),
                        body.len(),
                        truncated,
                        &agg,
                    )),
                    Err(e) => Err(e),
                }
            }
        };

        match result {
            Ok(record) => {
                if matches!(config.backend, SummarizeBackend::Gemini) {
                    llm_input_chars += user_prompt.chars().count();
                    if let Ok(j) = serde_json::to_string(&record) {
                        llm_output_chars += j.len();
                    }
                }
                persist_summary(
                    &conn,
                    &record,
                    &config.model,
                    match config.backend {
                        SummarizeBackend::Gemini => "gemini",
                        SummarizeBackend::Stub => "stub",
                    },
                )?;
                written += 1;
                summaries.push(record);
            }
            Err(e) => {
                eprintln!(
                    "phase4: bucket {} failed: {:#}",
                    bucket.bucket_id, e
                );
                failed += 1;
            }
        }
    }

    let out_path = config
        .output_path
        .clone()
        .unwrap_or_else(|| default_output_path(now_epoch_secs()));
    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create output dir '{}'", parent.display()))?;
    }

    let output = Phase4Output {
        summary: Phase4Summary {
            db_path: config.db_path.display().to_string(),
            generated_at_epoch_secs: now_epoch_secs(),
            prompt_version: PROMPT_VERSION,
            backend: match config.backend {
                SummarizeBackend::Gemini => "gemini".to_string(),
                SummarizeBackend::Stub => "stub".to_string(),
            },
            model: config.model.clone(),
            buckets_total,
            summaries_written: written,
            summaries_skipped_unchanged: skipped,
            summaries_failed: failed,
            llm_input_chars,
            llm_output_chars,
            dry_run: config.dry_run,
            force: config.force,
            max_input_chars: config.max_input_chars,
        },
        summaries,
    };

    let json = serde_json::to_string_pretty(&output).context("failed to serialize phase4 output")?;
    fs::write(&out_path, json)
        .with_context(|| format!("failed to write phase4 output '{}'", out_path.display()))?;
    Ok((out_path, output.summary))
}

struct BucketRow {
    bucket_id: i64,
}

struct AggregatedBucket {
    chunk_hashes: Vec<String>,
    distinct_apps: Vec<String>,
    source_attribution: Vec<BucketSourceRef>,
    prompt_body: String,
    first_seen_epoch_secs: Option<u64>,
    last_seen_epoch_secs: Option<u64>,
}

/// Distinct (app, window title) pairs for chunks assigned to this bucket (capture metadata).
/// Called from `app_commands` in the API crate; the `sensor_layer` capture binary also compiles `phase4` without that module.
#[allow(dead_code)]
pub fn bucket_source_attribution(conn: &Connection, bucket_id: i64) -> Result<Vec<BucketSourceRef>> {
    let mut stmt = conn.prepare(
        r#"SELECT cap.app_name, cap.window_title
           FROM task_bucket_items tbi
           JOIN chunks c ON c.chunk_hash = tbi.chunk_hash
           JOIN captures cap ON cap.canonical_hash = c.canonical_hash
           WHERE tbi.bucket_id = ?1"#,
    )?;
    let rows = stmt.query_map(params![bucket_id], |row| {
        Ok(BucketSourceRef {
            app_name: row.get(0)?,
            window_title: row.get(1)?,
        })
    })?;
    let mut set: std::collections::BTreeSet<BucketSourceRef> =
        std::collections::BTreeSet::new();
    for r in rows {
        set.insert(r.context("source attribution row")?);
    }
    Ok(set.into_iter().collect())
}

fn list_buckets(conn: &Connection) -> Result<Vec<BucketRow>> {
    let mut stmt = conn.prepare(
        "SELECT bucket_id FROM task_buckets ORDER BY last_active_epoch_secs DESC, bucket_id DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(BucketRow {
            bucket_id: row.get(0)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.context("bucket row")?);
    }
    Ok(out)
}

fn aggregate_bucket(conn: &Connection, bucket_id: i64) -> Result<AggregatedBucket> {
    let mut chunk_hashes_stmt = conn.prepare(
        "SELECT chunk_hash FROM task_bucket_items WHERE bucket_id = ?1 ORDER BY source_timestamp_epoch_secs ASC",
    )?;
    let hash_rows =
        chunk_hashes_stmt.query_map(params![bucket_id], |row| row.get::<_, String>(0))?;
    let mut chunk_hashes: Vec<String> = Vec::new();
    for h in hash_rows {
        chunk_hashes.push(h.context("chunk_hash")?);
    }

    let mut stmt = conn.prepare(
        r#"SELECT c.chunk_text, c.chunk_index, cap.app_name, cap.window_title, cap.timestamp_epoch_secs
           FROM task_bucket_items tbi
           JOIN chunks c ON c.chunk_hash = tbi.chunk_hash
           JOIN captures cap ON cap.canonical_hash = c.canonical_hash
           WHERE tbi.bucket_id = ?1
           ORDER BY cap.timestamp_epoch_secs ASC, c.chunk_index ASC"#,
    )?;
    let rows = stmt.query_map(params![bucket_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, i64>(4)?,
        ))
    })?;

    let mut parts: Vec<String> = Vec::new();
    let mut apps: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut source_keys: std::collections::BTreeSet<BucketSourceRef> =
        std::collections::BTreeSet::new();
    let mut ts_min: Option<i64> = None;
    let mut ts_max: Option<i64> = None;

    for row in rows {
        let (text, _idx, app, window, ts) = row.context("aggregate row")?;
        apps.insert(app.clone());
        ts_min = Some(ts_min.map_or(ts, |m| m.min(ts)));
        ts_max = Some(ts_max.map_or(ts, |m| m.max(ts)));
        parts.push(format!(
            "### segment (app={app}, window={window}, ts={ts})\n{}",
            text.trim()
        ));
        source_keys.insert(BucketSourceRef {
            app_name: app,
            window_title: window,
        });
    }

    let prompt_body = parts.join("\n\n");
    let mut distinct_apps: Vec<String> = apps.into_iter().collect();
    distinct_apps.sort_unstable();
    let source_attribution: Vec<BucketSourceRef> = source_keys.into_iter().collect();
    Ok(AggregatedBucket {
        chunk_hashes,
        distinct_apps,
        source_attribution,
        prompt_body,
        first_seen_epoch_secs: ts_min.map(|t| t as u64),
        last_seen_epoch_secs: ts_max.map(|t| t as u64),
    })
}

fn build_prompt_body(agg: &AggregatedBucket, max_chars: usize) -> (String, bool) {
    if agg.prompt_body.len() <= max_chars {
        return (agg.prompt_body.clone(), false);
    }
    let head = max_chars.saturating_mul(2) / 3;
    let tail = max_chars.saturating_sub(head).saturating_sub(80);
    let p = &agg.prompt_body;
    let start = p.chars().take(head).collect::<String>();
    let end = p.chars().rev().take(tail).collect::<String>();
    let end = end.chars().rev().collect::<String>();
    (
        format!(
            "{start}\n\n...[middle truncated: {} total chars; {} kept]...\n\n{end}",
            p.len(),
            head + tail
        ),
        true,
    )
}

fn build_user_prompt(agg: &AggregatedBucket, body: &str, truncated: bool) -> String {
    let apps = agg.distinct_apps.join(", ");
    let windows_preview: String = agg
        .source_attribution
        .iter()
        .map(|s| {
            let w = s.window_title.trim();
            if w.is_empty() {
                s.app_name.clone()
            } else {
                format!("{} — {}", s.app_name, w)
            }
        })
        .collect::<Vec<_>>()
        .join("; ");
    let windows_preview = if windows_preview.is_empty() {
        "none".to_string()
    } else {
        windows_preview
    };
    let tw = match (agg.first_seen_epoch_secs, agg.last_seen_epoch_secs) {
        (Some(a), Some(b)) => format!("time range (epoch seconds, inclusive): {a} .. {b}"),
        _ => "time range: unknown".to_string(),
    };
    format!(
        r#"You summarize one "task bucket" from an on-device activity log (OCR from screenshots). The owner opted in; content may include work or personal context. Be faithful to the text: do not invent URLs, product names, numbers, or steps that are not supported by the OCR. Prefer completeness over brevity for important material.

{tw}
Apps seen in this bucket: {apps}
Distinct windows/titles (from capture metadata; use only to understand context — do not invent additional titles): {windows_preview}
Segments truncated for model limits: {trunc}

OCR segments (chronological):
---
{body}
---

Voice: In "one_liner" and "detailed_summary", address the bucket owner as "you" (second person). Do not write "the user", "they", or similar.

Coverage: The OCR may contain articles, guides, tables, numbered lists, phases, metrics, and links. Your job is to preserve the substance point-by-point:
- Include every major heading, phase, and numbered step that appears.
- For tables, include one key_point (or more) per row: metric name, what it measures, and any threshold or target given.
- Include concrete examples, definitions, and named frameworks when present (e.g. quoted terms like "workflow-value fit").
- Include link text and URLs only if they appear verbatim in the OCR (no guessed links).

Respond with a single JSON object ONLY (no markdown fences), keys:
- "title": short task title (max ~80 chars), neutral phrasing (not "you")
- "one_liner": exactly one sentence, second person ("you ..."), capturing the main focus of what you were reading or doing in this bucket
- "detailed_summary": 1-3 short sentences in second person that frame the bucket (optional if key_points already cover context)
- "key_points": array of strings; each string is ONE atomic takeaway (no leading "- "). Order should follow the OCR when possible. Aim to list every important claim, step, metric, and example—err on the side of too many points rather than merging distinct ideas into one vague line. Typical buckets need many entries (often 15-40+ when the source is long); short pages need fewer.
- "primary_apps": array of app names most relevant (subset of seen apps only)
- "tags": 3-8 lowercase snake_case tags
- "confidence_0_1": number 0-1 how confident you are
- "caveats": optional string if OCR was noisy, duplicated, or ambiguous"#,
        tw = tw,
        apps = apps,
        windows_preview = windows_preview,
        trunc = truncated,
        body = body
    )
}

fn fingerprint_for_bucket(chunk_hashes: &[String]) -> String {
    let mut sorted: Vec<&str> = chunk_hashes.iter().map(|s| s.as_str()).collect();
    sorted.sort_unstable();
    let joined = sorted.join("\n");
    let mut hasher = Sha256::new();
    hasher.update(joined.as_bytes());
    hex::encode(hasher.finalize())
}

/// Ensures `task_bucket_summaries` exists (safe to call before reads or after phase2-only DB use).
pub fn init_phase4_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
CREATE TABLE IF NOT EXISTS task_bucket_summaries (
    bucket_id INTEGER PRIMARY KEY,
    input_fingerprint TEXT NOT NULL,
    summary_json TEXT NOT NULL,
    model TEXT NOT NULL,
    backend TEXT NOT NULL,
    prompt_version TEXT NOT NULL,
    generated_at_epoch_secs INTEGER NOT NULL
);
"#,
    )
    .context("failed creating phase4 schema")?;
    Ok(())
}

fn backend_label(b: &SummarizeBackend) -> &'static str {
    match b {
        SummarizeBackend::Gemini => "gemini",
        SummarizeBackend::Stub => "stub",
    }
}

fn existing_summary_meta(
    conn: &Connection,
    bucket_id: i64,
) -> Result<Option<(String, String, String, String)>> {
    let row: Option<(String, String, String, String)> = conn
        .query_row(
            "SELECT input_fingerprint, prompt_version, backend, model FROM task_bucket_summaries WHERE bucket_id = ?1",
            params![bucket_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()
        .context("read summary meta")?;
    Ok(row)
}

fn load_summary_row(conn: &Connection, bucket_id: i64) -> Result<Option<BucketSummaryRecord>> {
    let json: Option<String> = conn
        .query_row(
            "SELECT summary_json FROM task_bucket_summaries WHERE bucket_id = ?1",
            params![bucket_id],
            |row| row.get(0),
        )
        .optional()
        .context("read summary_json")?;
    let Some(j) = json else {
        return Ok(None);
    };
    serde_json::from_str(&j).context("parse stored summary")
}

fn persist_summary(
    conn: &Connection,
    record: &BucketSummaryRecord,
    model: &str,
    backend: &str,
) -> Result<()> {
    let summary_json =
        serde_json::to_string(record).context("serialize BucketSummaryRecord")?;
    conn.execute(
        "INSERT OR REPLACE INTO task_bucket_summaries (bucket_id, input_fingerprint, summary_json, model, backend, prompt_version, generated_at_epoch_secs)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            record.bucket_id,
            record.input_fingerprint,
            summary_json,
            model,
            backend,
            PROMPT_VERSION,
            now_epoch_secs() as i64
        ],
    )
    .context("persist task_bucket_summaries")?;
    Ok(())
}

fn gemini_summarize(
    client: &reqwest::blocking::Client,
    base_url: &str,
    api_key: &str,
    model: &str,
    user_text: &str,
    max_retries: usize,
    base_delay_ms: u64,
) -> Result<LlmJsonShape> {
    #[derive(Serialize)]
    struct GenBody<'a> {
        contents: Vec<GenContent<'a>>,
        #[serde(rename = "generationConfig")]
        generation_config: GenConfig,
    }
    #[derive(Serialize)]
    struct GenContent<'a> {
        role: &'static str,
        parts: Vec<GenPart<'a>>,
    }
    #[derive(Serialize)]
    struct GenPart<'a> {
        text: &'a str,
    }
    #[derive(Serialize)]
    struct GenConfig {
        #[serde(rename = "responseMimeType")]
        response_mime_type: &'static str,
        temperature: f32,
        /// Long bucket summaries need room for many `key_points` without truncating JSON.
        #[serde(rename = "maxOutputTokens")]
        max_output_tokens: u32,
    }

    let system_preamble = "You are a precise analyst for on-device activity logs (OCR). Output a single JSON object only. Write for the person whose screen was captured: use second person (you/your) in one_liner and detailed_summary. Never refer to them as \"the user\", \"they\", or \"the reader\".";

    let full_text = format!("{system_preamble}\n\n{user_text}");
    let body = GenBody {
        contents: vec![GenContent {
            role: "user",
            parts: vec![GenPart { text: &full_text }],
        }],
        generation_config: GenConfig {
            response_mime_type: "application/json",
            temperature: 0.2,
            max_output_tokens: 8192,
        },
    };

    let endpoint = format!(
        "{}/v1beta/models/{}:generateContent",
        base_url.trim_end_matches('/'),
        model
    );

    let parsed: LlmJsonShape = with_retries(max_retries, base_delay_ms, || {
        let response = client
            .post(&endpoint)
            .query(&[("key", api_key)])
            .json(&body)
            .send()
            .with_context(|| format!("generateContent request failed ({endpoint})"))?;

        maybe_retryable_response(response, "gemini", |resp| {
            let gen: GeminiGenerateResponse = resp
                .json()
                .context("parse Gemini generateContent JSON")?;
            let text = gen
                .candidates
                .and_then(|c| c.into_iter().next())
                .and_then(|c| c.content)
                .and_then(|c| c.parts)
                .and_then(|p| p.into_iter().next())
                .and_then(|part| part.text)
                .ok_or_else(|| anyhow!("Gemini response missing candidate text"))?;
            let shape: LlmJsonShape = serde_json::from_str(text.trim())
                .or_else(|_| {
                    // Some models wrap JSON in whitespace or minor noise
                    serde_json::from_str(
                        text.trim()
                            .trim_start_matches("```json")
                            .trim_start_matches("```")
                            .trim_end_matches("```")
                            .trim(),
                    )
                })
                .context("parse LLM JSON body")?;
            Ok(shape)
        })
    })?;

    Ok(parsed)
}

fn merge_detailed_summary(j: &LlmJsonShape) -> String {
    let overview = j.detailed_summary.as_deref().unwrap_or("").trim();
    let points: Vec<String> = j
        .key_points
        .as_ref()
        .map(|v| {
            v.iter()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();

    if points.is_empty() {
        return overview.to_string();
    }

    let bullets: String = points
        .into_iter()
        .map(|p| {
            let t = p.trim();
            if t.starts_with("- ") || t.starts_with("* ") || t.starts_with("• ") {
                t.to_string()
            } else {
                format!("- {t}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    if overview.is_empty() {
        bullets
    } else {
        format!("{overview}\n\n{bullets}")
    }
}

fn llm_to_record(
    bucket_id: i64,
    j: LlmJsonShape,
    fingerprint: &str,
    chunk_count: usize,
    input_chars: usize,
    truncated: bool,
    agg: &AggregatedBucket,
) -> BucketSummaryRecord {
    let detailed_summary = merge_detailed_summary(&j);
    BucketSummaryRecord {
        bucket_id,
        title: j.title.unwrap_or_else(|| "Untitled task".to_string()),
        one_liner: j.one_liner.unwrap_or_default(),
        detailed_summary,
        source_attribution: agg.source_attribution.clone(),
        primary_apps: j.primary_apps.unwrap_or_default(),
        tags: j.tags.unwrap_or_default(),
        confidence_0_1: j.confidence_0_1.unwrap_or(0.5).clamp(0.0, 1.0),
        caveats: j.caveats,
        input_fingerprint: fingerprint.to_string(),
        chunk_count,
        input_chars,
        input_truncated: truncated,
        first_seen_epoch_secs: agg.first_seen_epoch_secs,
        last_seen_epoch_secs: agg.last_seen_epoch_secs,
    }
}

fn stub_summary(
    bucket_id: i64,
    agg: &AggregatedBucket,
    fingerprint: &str,
    input_chars: usize,
    truncated: bool,
) -> BucketSummaryRecord {
    let preview: String = agg
        .prompt_body
        .chars()
        .take(200)
        .collect::<String>()
        .replace('\n', " ");
    BucketSummaryRecord {
        bucket_id,
        title: format!("Bucket {bucket_id} (stub)"),
        one_liner: "Stub backend — set OMEGA_PHASE4_BACKEND=gemini for real summaries.".to_string(),
        detailed_summary: format!(
            "This is a deterministic offline summary. Content preview: {preview}..."
        ),
        source_attribution: agg.source_attribution.clone(),
        primary_apps: agg.distinct_apps.clone(),
        tags: vec!["stub".to_string(), "offline".to_string()],
        confidence_0_1: 0.1,
        caveats: Some("OMEGA_PHASE4_BACKEND=stub".to_string()),
        input_fingerprint: fingerprint.to_string(),
        chunk_count: agg.chunk_hashes.len(),
        input_chars,
        input_truncated: truncated,
        first_seen_epoch_secs: agg.first_seen_epoch_secs,
        last_seen_epoch_secs: agg.last_seen_epoch_secs,
    }
}

fn maybe_retryable_response<T, F>(response: Response, source: &str, parse: F) -> Result<T>
where
    F: FnOnce(Response) -> Result<T>,
{
    if response.status().is_success() {
        return parse(response);
    }
    let status = response.status();
    let response_body = response.text().unwrap_or_default();
    let is_retryable = status.as_u16() == 429 || status.is_server_error();
    if is_retryable {
        Err(anyhow!(
            "[retryable] {} request failed status={} body={}",
            source,
            status,
            response_body
        ))
    } else {
        Err(anyhow!(
            "{} request failed status={} body={}",
            source,
            status,
            response_body
        ))
    }
}

fn with_retries<T, F>(max_retries: usize, base_delay_ms: u64, mut op: F) -> Result<T>
where
    F: FnMut() -> Result<T>,
{
    let mut attempt = 0usize;
    loop {
        match op() {
            Ok(v) => return Ok(v),
            Err(err) => {
                let retryable = err.to_string().contains("[retryable]");
                if !retryable || attempt >= max_retries {
                    return Err(err);
                }
                let backoff = base_delay_ms.saturating_mul(2u64.saturating_pow(attempt as u32));
                sleep(Duration::from_millis(backoff.min(8_000)));
                attempt += 1;
            }
        }
    }
}

fn default_output_path(now_epoch: u64) -> PathBuf {
    PathBuf::from("logs").join(format!("phase4-summaries-{now_epoch}.json"))
}

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
