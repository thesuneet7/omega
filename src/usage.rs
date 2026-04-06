//! Estimated Gemini API spend (list pricing; see docs/COST_ESTIMATION.md).

use crate::app_db;
use crate::phase2::IngestionSummary;
use crate::phase4::Phase4Summary;
use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::path::{Path, PathBuf};

const CHARS_PER_TOKEN_EST: f64 = 4.0;
const USD_PER_M_EMBED: f64 = 0.15;
const USD_PER_M_P4_IN: f64 = 0.10;
const USD_PER_M_P4_OUT: f64 = 0.40;

fn app_db_path() -> PathBuf {
    PathBuf::from(
        std::env::var("OMEGA_APP_LOGS_DIR").unwrap_or_else(|_| "logs".to_string()),
    )
    .join("app_state.db")
}

/// Soft cap for the usage bar (USD / calendar month, planning estimate).
pub fn monthly_limit_usd() -> f64 {
    std::env::var("OMEGA_USAGE_MONTHLY_LIMIT_USD")
        .ok()
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|v| *v > 0.0)
        .unwrap_or(3.0)
}

fn cost_delta_phase2(s: &IngestionSummary) -> f64 {
    if s.embedding_backend == "hash" {
        return 0.0;
    }
    let tokens = (s.embedded_input_chars as f64) / CHARS_PER_TOKEN_EST;
    tokens / 1_000_000.0 * USD_PER_M_EMBED
}

fn cost_delta_phase4(s: &Phase4Summary) -> f64 {
    if s.dry_run || s.backend == "stub" {
        return 0.0;
    }
    let in_t = (s.llm_input_chars as f64) / CHARS_PER_TOKEN_EST;
    let out_t = (s.llm_output_chars as f64) / CHARS_PER_TOKEN_EST;
    in_t / 1_000_000.0 * USD_PER_M_P4_IN + out_t / 1_000_000.0 * USD_PER_M_P4_OUT
}

pub fn record_phase2(summary: &IngestionSummary, session_key: Option<&str>) -> Result<()> {
    let conn = app_db::open_app_db(&app_db_path())?;
    let delta = cost_delta_phase2(summary);
    let ec = summary.embedded_input_chars as i64;
    let now = app_db::now_epoch_secs() as i64;
    conn.execute(
        "UPDATE api_usage_totals SET
           estimated_cost_usd = estimated_cost_usd + ?1,
           embedded_chars_total = embedded_chars_total + ?2,
           updated_at = ?3
         WHERE id = 1",
        params![delta, ec, now],
    )
    .context("update api_usage_totals phase2")?;
    if let Some(sk) = session_key {
        upsert_session_phase2(&conn, sk, delta, ec, now)?;
    }
    Ok(())
}

fn upsert_session_phase2(conn: &Connection, sk: &str, delta: f64, ec: i64, now: i64) -> Result<()> {
    conn.execute(
        "INSERT INTO api_usage_session (session_key, estimated_cost_usd, embedded_chars, phase4_input_chars, phase4_output_chars, updated_at)
         VALUES (?1, ?2, ?3, 0, 0, ?4)
         ON CONFLICT(session_key) DO UPDATE SET
           estimated_cost_usd = estimated_cost_usd + excluded.estimated_cost_usd,
           embedded_chars = embedded_chars + excluded.embedded_chars,
           updated_at = excluded.updated_at",
        params![sk, delta, ec, now],
    )
    .context("upsert api_usage_session phase2")?;
    Ok(())
}

pub fn record_phase4(summary: &Phase4Summary, session_key: Option<&str>) -> Result<()> {
    let conn = app_db::open_app_db(&app_db_path())?;
    let delta = cost_delta_phase4(summary);
    let ic = summary.llm_input_chars as i64;
    let oc = summary.llm_output_chars as i64;
    let now = app_db::now_epoch_secs() as i64;
    conn.execute(
        "UPDATE api_usage_totals SET
           estimated_cost_usd = estimated_cost_usd + ?1,
           phase4_input_chars_total = phase4_input_chars_total + ?2,
           phase4_output_chars_total = phase4_output_chars_total + ?3,
           updated_at = ?4
         WHERE id = 1",
        params![delta, ic, oc, now],
    )
    .context("update api_usage_totals phase4")?;
    if let Some(sk) = session_key {
        upsert_session_phase4(&conn, sk, delta, ic, oc, now)?;
    }
    Ok(())
}

fn upsert_session_phase4(
    conn: &Connection,
    sk: &str,
    delta: f64,
    ic: i64,
    oc: i64,
    now: i64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO api_usage_session (session_key, estimated_cost_usd, embedded_chars, phase4_input_chars, phase4_output_chars, updated_at)
         VALUES (?1, ?2, 0, ?3, ?4, ?5)
         ON CONFLICT(session_key) DO UPDATE SET
           estimated_cost_usd = estimated_cost_usd + excluded.estimated_cost_usd,
           phase4_input_chars = phase4_input_chars + excluded.phase4_input_chars,
           phase4_output_chars = phase4_output_chars + excluded.phase4_output_chars,
           updated_at = excluded.updated_at",
        params![sk, delta, ic, oc, now],
    )
    .context("upsert api_usage_session phase4")?;
    Ok(())
}

#[derive(Debug, Serialize, Clone)]
pub struct ApiUsageSnapshot {
    pub estimated_cost_usd_total: f64,
    pub monthly_limit_usd: f64,
    pub usage_percent_of_limit: f64,
    pub embedded_chars_total: u64,
    pub phase4_input_chars_total: u64,
    pub phase4_output_chars_total: u64,
    pub estimated_embed_tokens: u64,
    pub estimated_phase4_input_tokens: u64,
    pub estimated_phase4_output_tokens: u64,
    pub pricing_note: &'static str,
}

#[derive(Debug, Serialize, Clone)]
pub struct SessionUsage {
    pub session_key: String,
    pub estimated_cost_usd: f64,
    pub embedded_chars: u64,
    pub phase4_input_chars: u64,
    pub phase4_output_chars: u64,
}

#[derive(Debug, Serialize, Clone)]
pub struct ApiUsageResponse {
    pub overall: ApiUsageSnapshot,
    pub session: Option<SessionUsage>,
}

pub fn get_usage_snapshot() -> Result<ApiUsageSnapshot> {
    let conn = app_db::open_app_db(&app_db_path())?;
    let row: (f64, i64, i64, i64) = conn
        .query_row(
            "SELECT estimated_cost_usd, embedded_chars_total, phase4_input_chars_total, phase4_output_chars_total
             FROM api_usage_totals WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .context("read api_usage_totals")?;
    let (cost, ec, p4i, p4o) = row;
    let limit = monthly_limit_usd();
    let pct = if limit > 0.0 {
        (cost / limit * 100.0).min(100.0)
    } else {
        0.0
    };
    let et = (ec as f64 / CHARS_PER_TOKEN_EST).max(0.0) as u64;
    let p4it = (p4i as f64 / CHARS_PER_TOKEN_EST).max(0.0) as u64;
    let p4ot = (p4o as f64 / CHARS_PER_TOKEN_EST).max(0.0) as u64;
    Ok(ApiUsageSnapshot {
        estimated_cost_usd_total: cost,
        monthly_limit_usd: limit,
        usage_percent_of_limit: pct,
        embedded_chars_total: ec as u64,
        phase4_input_chars_total: p4i as u64,
        phase4_output_chars_total: p4o as u64,
        estimated_embed_tokens: et,
        estimated_phase4_input_tokens: p4it,
        estimated_phase4_output_tokens: p4ot,
        pricing_note: "Estimates use gemini-embedding-001 + gemini-2.5-flash-lite list pricing (~4 chars/token).",
    })
}

pub fn get_session_usage(session_key: &str) -> Result<Option<SessionUsage>> {
    let conn = app_db::open_app_db(&app_db_path())?;
    let row: Option<(f64, i64, i64, i64)> = conn
        .query_row(
            "SELECT estimated_cost_usd, embedded_chars, phase4_input_chars, phase4_output_chars
             FROM api_usage_session WHERE session_key = ?1",
            params![session_key],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()
        .context("read api_usage_session")?;
    let Some((cost, ec, p4i, p4o)) = row else {
        return Ok(None);
    };
    Ok(Some(SessionUsage {
        session_key: session_key.to_string(),
        estimated_cost_usd: cost,
        embedded_chars: ec as u64,
        phase4_input_chars: p4i as u64,
        phase4_output_chars: p4o as u64,
    }))
}

pub fn get_api_usage_response(session_key: Option<&str>) -> Result<ApiUsageResponse> {
    let overall = get_usage_snapshot()?;
    let session = if let Some(sk) = session_key.filter(|s| !s.is_empty()) {
        get_session_usage(sk)?
    } else {
        None
    };
    Ok(ApiUsageResponse { overall, session })
}

/// Derive `capture-session-*` stem from a capture log path.
pub fn session_key_from_input_ref(input: &str) -> Option<String> {
    if input.is_empty() {
        return None;
    }
    Path::new(input)
        .file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
}
