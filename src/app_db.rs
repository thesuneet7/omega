use crate::app_models::{PipelineRunRecord, SummaryRevision};
use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn open_app_db(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)
        .with_context(|| format!("failed to open app db '{}'", path.display()))?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")
        .context("enable sqlite foreign keys")?;
    init_schema(&conn)?;
    Ok(conn)
}

fn init_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
CREATE TABLE IF NOT EXISTS session_summaries (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  session_key TEXT NOT NULL UNIQUE,
  current_title TEXT NOT NULL,
  current_body TEXT NOT NULL,
  source_bucket_ids TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS summary_revisions (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  summary_id INTEGER NOT NULL,
  title TEXT NOT NULL,
  body TEXT NOT NULL,
  edited_at INTEGER NOT NULL,
  editor_label TEXT NOT NULL,
  FOREIGN KEY(summary_id) REFERENCES session_summaries(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS pipeline_runs (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  stage TEXT NOT NULL,
  input_ref TEXT NOT NULL,
  status TEXT NOT NULL,
  started_at INTEGER NOT NULL,
  ended_at INTEGER,
  error_text TEXT
);

CREATE TABLE IF NOT EXISTS api_usage_totals (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  estimated_cost_usd REAL NOT NULL DEFAULT 0,
  embedded_chars_total INTEGER NOT NULL DEFAULT 0,
  phase4_input_chars_total INTEGER NOT NULL DEFAULT 0,
  phase4_output_chars_total INTEGER NOT NULL DEFAULT 0,
  updated_at INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS api_usage_session (
  session_key TEXT PRIMARY KEY,
  estimated_cost_usd REAL NOT NULL DEFAULT 0,
  embedded_chars INTEGER NOT NULL DEFAULT 0,
  phase4_input_chars INTEGER NOT NULL DEFAULT 0,
  phase4_output_chars INTEGER NOT NULL DEFAULT 0,
  updated_at INTEGER NOT NULL DEFAULT 0
);
"#,
    )
    .context("failed to create app db schema")?;

    conn.execute(
        "INSERT OR IGNORE INTO api_usage_totals (id, estimated_cost_usd, embedded_chars_total, phase4_input_chars_total, phase4_output_chars_total, updated_at)
         VALUES (1, 0, 0, 0, 0, 0)",
        [],
    )
    .context("seed api_usage_totals")?;
    Ok(())
}

pub fn upsert_current_summary(
    conn: &Connection,
    session_key: &str,
    title: &str,
    body: &str,
    source_bucket_ids: &[i64],
) -> Result<i64> {
    let source_json = serde_json::to_string(source_bucket_ids).context("serialize bucket ids")?;
    let now = now_epoch_secs() as i64;

    let existing_id: Option<i64> = conn
        .query_row(
            "SELECT id FROM session_summaries WHERE session_key = ?1",
            params![session_key],
            |row| row.get(0),
        )
        .optional()
        .context("load existing session summary id")?;

    match existing_id {
        Some(id) => {
            conn.execute(
                "UPDATE session_summaries
                 SET current_title = ?1, current_body = ?2, source_bucket_ids = ?3, updated_at = ?4
                 WHERE id = ?5",
                params![title, body, source_json, now, id],
            )
            .context("update session_summaries")?;
            Ok(id)
        }
        None => {
            conn.execute(
                "INSERT INTO session_summaries (session_key, current_title, current_body, source_bucket_ids, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
                params![session_key, title, body, source_json, now],
            )
            .context("insert session_summaries")?;
            Ok(conn.last_insert_rowid())
        }
    }
}

pub fn insert_revision(
    conn: &Connection,
    summary_id: i64,
    title: &str,
    body: &str,
    editor_label: &str,
) -> Result<i64> {
    let now = now_epoch_secs() as i64;
    conn.execute(
        "INSERT INTO summary_revisions (summary_id, title, body, edited_at, editor_label)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![summary_id, title, body, now, editor_label],
    )
    .context("insert summary revision")?;
    Ok(conn.last_insert_rowid())
}

pub fn list_revisions(conn: &Connection, summary_id: i64) -> Result<Vec<SummaryRevision>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, summary_id, title, body, edited_at, editor_label
             FROM summary_revisions
             WHERE summary_id = ?1
             ORDER BY edited_at DESC, id DESC",
        )
        .context("prepare list revisions")?;

    let rows = stmt
        .query_map(params![summary_id], |row| {
            Ok(SummaryRevision {
                id: row.get(0)?,
                summary_id: row.get(1)?,
                title: row.get(2)?,
                body: row.get(3)?,
                edited_at_epoch_secs: row.get::<_, i64>(4)? as u64,
                editor_label: row.get(5)?,
            })
        })
        .context("query list revisions")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("decode revision row")?);
    }
    Ok(out)
}

pub fn get_summary_row(
    conn: &Connection,
    session_key: &str,
) -> Result<Option<(i64, String, String, Vec<i64>)>> {
    let row: Option<(i64, String, String, String)> = conn
        .query_row(
            "SELECT id, current_title, current_body, source_bucket_ids
             FROM session_summaries
             WHERE session_key = ?1",
            params![session_key],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()
        .context("load session summary row")?;

    let Some((id, title, body, source_bucket_ids_json)) = row else {
        return Ok(None);
    };
    let source_bucket_ids: Vec<i64> =
        serde_json::from_str(&source_bucket_ids_json).unwrap_or_else(|_| Vec::new());
    Ok(Some((id, title, body, source_bucket_ids)))
}

pub fn start_pipeline_run(conn: &Connection, stage: &str, input_ref: &str) -> Result<i64> {
    let now = now_epoch_secs() as i64;
    conn.execute(
        "INSERT INTO pipeline_runs (stage, input_ref, status, started_at, ended_at, error_text)
         VALUES (?1, ?2, 'running', ?3, NULL, NULL)",
        params![stage, input_ref, now],
    )
    .context("insert pipeline run")?;
    Ok(conn.last_insert_rowid())
}

pub fn finish_pipeline_run(
    conn: &Connection,
    run_id: i64,
    status: &str,
    error_text: Option<&str>,
) -> Result<()> {
    let now = now_epoch_secs() as i64;
    conn.execute(
        "UPDATE pipeline_runs SET status = ?1, ended_at = ?2, error_text = ?3 WHERE id = ?4",
        params![status, now, error_text, run_id],
    )
    .context("update pipeline run status")?;
    Ok(())
}

/// Remove saved summary, revisions (via FK cascade), and per-session usage for one capture session.
pub fn delete_session_records(conn: &Connection, session_key: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM api_usage_session WHERE session_key = ?1",
        params![session_key],
    )
    .context("delete api_usage_session row")?;
    conn.execute(
        "DELETE FROM session_summaries WHERE session_key = ?1",
        params![session_key],
    )
    .context("delete session_summaries row")?;
    Ok(())
}

pub fn list_pipeline_runs(conn: &Connection, limit: usize) -> Result<Vec<PipelineRunRecord>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, stage, input_ref, status, started_at, ended_at, error_text
             FROM pipeline_runs
             ORDER BY started_at DESC, id DESC
             LIMIT ?1",
        )
        .context("prepare list pipeline runs")?;
    let rows = stmt
        .query_map(params![limit as i64], |row| {
            Ok(PipelineRunRecord {
                id: row.get(0)?,
                stage: row.get(1)?,
                input_ref: row.get(2)?,
                status: row.get(3)?,
                started_at_epoch_secs: row.get::<_, i64>(4)? as u64,
                ended_at_epoch_secs: row.get::<_, Option<i64>>(5)?.map(|v| v as u64),
                error_text: row.get(6)?,
            })
        })
        .context("query pipeline runs")?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("decode pipeline run row")?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_db_path(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        let suffix = format!(
            "{}-{}-{}.db",
            name,
            std::process::id(),
            now_epoch_secs()
        );
        p.push(suffix);
        p
    }

    #[test]
    fn summary_revision_roundtrip() {
        let path = temp_db_path("omega-app-state");
        let conn = open_app_db(&path).expect("open app db");
        let summary_id = upsert_current_summary(&conn, "session-a", "T1", "Body1", &[1, 2])
            .expect("upsert");
        insert_revision(&conn, summary_id, "T1", "Body1", "test").expect("insert revision");

        let row = get_summary_row(&conn, "session-a").expect("get row").expect("row exists");
        assert_eq!(row.0, summary_id);
        assert_eq!(row.1, "T1");
        assert_eq!(row.2, "Body1");
        assert_eq!(row.3, vec![1, 2]);

        let revisions = list_revisions(&conn, summary_id).expect("list revisions");
        assert_eq!(revisions.len(), 1);
        assert_eq!(revisions[0].title, "T1");
        assert_eq!(revisions[0].editor_label, "test");

        let _ = fs::remove_file(path);
    }
}
