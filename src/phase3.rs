use crate::phase2::EmbeddingBackend;
use anyhow::{anyhow, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct StitchConfig {
    pub db_path: PathBuf,
    pub output_path: Option<PathBuf>,
    pub match_threshold: f32,
    pub decay_lambda: f64,
    pub active_window_mins: u64,
    /// Must match Phase 2 `embeddings.backend` rows to use (default `gemini`).
    pub embedding_backend: String,
    /// Must match Phase 2 `embeddings.model` rows to use (default aligns with Phase 2).
    pub embed_model: String,
    /// Cap the effective weight when updating a bucket centroid (prevents centroid drift).
    pub max_centroid_weight: usize,
    /// Number of k-means-style refinement rounds after the initial online pass.
    pub refinement_rounds: usize,
}

impl StitchConfig {
    pub fn from_env_and_args(db_path: Option<PathBuf>, output_path: Option<PathBuf>) -> Result<Self> {
        let db_path = db_path.unwrap_or_else(|| {
            PathBuf::from(
                std::env::var("OMEGA_PHASE3_DB_PATH")
                    .unwrap_or_else(|_| "logs/phase2.db".to_string()),
            )
        });
        let match_threshold = std::env::var("OMEGA_PHASE3_MATCH_THRESHOLD")
            .ok()
            .and_then(|v| v.parse::<f32>().ok())
            .unwrap_or(0.82);
        let decay_lambda = std::env::var("OMEGA_PHASE3_DECAY_LAMBDA")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.00002);
        let active_window_mins = std::env::var("OMEGA_PHASE3_ACTIVE_WINDOW_MINS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(15);
        let max_centroid_weight = std::env::var("OMEGA_PHASE3_MAX_CENTROID_WEIGHT")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(10)
            .max(1);
        let refinement_rounds = std::env::var("OMEGA_PHASE3_REFINEMENT_ROUNDS")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(3);
        let backend = EmbeddingBackend::from_env()?;
        let embedding_backend = backend.as_str().to_string();
        let embed_model = crate::phase2::resolve_embed_model_for_backend(&backend)?;
        Ok(Self {
            db_path,
            output_path,
            match_threshold,
            decay_lambda,
            active_window_mins,
            embedding_backend,
            embed_model,
            max_centroid_weight,
            refinement_rounds,
        })
    }
}

#[derive(Debug)]
struct CandidateChunk {
    chunk_hash: String,
    canonical_hash: String,
    timestamp_epoch_secs: i64,
    embedding: Vec<f32>,
    /// Git branch at the time of capture, if available.
    git_branch: Option<String>,
}

#[derive(Debug)]
struct BucketState {
    bucket_id: i64,
    centroid: Vec<f32>,
    item_count: i64,
    last_active_epoch_secs: i64,
    /// Known git branches for chunks already assigned to this bucket.
    /// Used to enforce hard separation between different branches.
    known_git_branches: std::collections::HashSet<String>,
}

#[derive(Debug, Serialize)]
pub struct StitchSummary {
    pub db_path: String,
    pub generated_at_epoch_secs: u64,
    pub embedding_backend: String,
    pub embed_model: String,
    pub candidates_seen: usize,
    pub chunks_assigned: usize,
    pub chunks_skipped_existing_assignment: usize,
    pub existing_buckets_loaded: usize,
    pub buckets_created: usize,
    pub match_threshold: f32,
    pub decay_lambda: f64,
    pub active_window_mins: u64,
    pub max_centroid_weight: usize,
    pub refinement_rounds_config: usize,
    pub refinement_rounds_run: usize,
    pub refinement_reassignments: usize,
}

#[derive(Debug, Serialize)]
pub struct BucketOutput {
    pub bucket_id: i64,
    pub item_count: usize,
    pub created_at_epoch_secs: u64,
    pub last_active_epoch_secs: u64,
    /// Earliest capture timestamp among chunks in this bucket (from `captures`).
    pub first_seen_epoch_secs: Option<u64>,
    /// Latest capture timestamp among chunks in this bucket.
    pub last_seen_epoch_secs: Option<u64>,
    /// Distinct app names seen in this bucket (from `captures`, unordered).
    pub distinct_apps: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct StitchOutput {
    pub summary: StitchSummary,
    pub buckets: Vec<BucketOutput>,
}

pub fn run_stitching(config: StitchConfig) -> Result<PathBuf> {
    let conn = Connection::open(&config.db_path)
        .with_context(|| format!("failed to open sqlite db '{}'", config.db_path.display()))?;
    init_phase3_schema(&conn)?;

    let mut buckets = load_bucket_states(&conn)?;
    let existing_buckets_loaded = buckets.len();
    let mut buckets_created = 0usize;
    let mut chunks_assigned = 0usize;
    let mut chunks_skipped_existing_assignment = 0usize;
    let mut embedding_backend = config.embedding_backend.clone();
    let mut embed_model = config.embed_model.clone();
    let mut candidates = load_candidate_chunks(&conn, &embedding_backend, &embed_model)?;
    if candidates.is_empty() {
        if let Some((b, m)) = dominant_embedding_backend_model(&conn)? {
            if b != embedding_backend || m != embed_model {
                eprintln!(
                    "phase3: no embedding rows for backend={embedding_backend} model={embed_model}; \
                     retrying with dominant DB pair backend={b} model={m}"
                );
                embedding_backend = b;
                embed_model = m;
                candidates = load_candidate_chunks(&conn, &embedding_backend, &embed_model)?;
            }
        }
    }

    for chunk in &candidates {
        if has_existing_assignment(&conn, &chunk.chunk_hash)? {
            chunks_skipped_existing_assignment += 1;
            continue;
        }

        let best = find_best_bucket_match(
            &buckets,
            &chunk.embedding,
            chunk.timestamp_epoch_secs,
            config.decay_lambda,
            config.active_window_mins,
            chunk.git_branch.as_deref(),
        );
        let assigned_bucket_id = match best {
            Some((idx, score)) if score >= config.match_threshold => {
                assign_to_bucket(
                    &conn,
                    &mut buckets[idx],
                    chunk,
                    score,
                    config.decay_lambda,
                    config.active_window_mins,
                    config.max_centroid_weight,
                )?
            }
            _ => {
                let new_bucket_id = create_bucket(&conn, chunk)?;
                let mut known_git_branches = std::collections::HashSet::new();
                if let Some(ref b) = chunk.git_branch {
                    known_git_branches.insert(b.clone());
                }
                let bucket = BucketState {
                    bucket_id: new_bucket_id,
                    centroid: chunk.embedding.clone(),
                    item_count: 1,
                    last_active_epoch_secs: chunk.timestamp_epoch_secs,
                    known_git_branches,
                };
                buckets.push(bucket);
                insert_assignment(&conn, new_bucket_id, chunk, 1.0, true)?;
                buckets_created += 1;
                new_bucket_id
            }
        };

        if assigned_bucket_id > 0 {
            chunks_assigned += 1;
        }
    }

    let (refinement_rounds_run, refinement_reassignments) = run_refinement(
        &conn,
        config.match_threshold,
        config.refinement_rounds,
        &embedding_backend,
        &embed_model,
    )?;

    let output_path = config
        .output_path
        .clone()
        .unwrap_or_else(|| default_output_path(now_epoch_secs()));
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create output dir '{}'", parent.display()))?;
    }

    let output = StitchOutput {
        summary: StitchSummary {
            db_path: config.db_path.display().to_string(),
            generated_at_epoch_secs: now_epoch_secs(),
            embedding_backend: embedding_backend.clone(),
            embed_model: embed_model.clone(),
            candidates_seen: candidates.len(),
            chunks_assigned,
            chunks_skipped_existing_assignment,
            existing_buckets_loaded,
            buckets_created,
            match_threshold: config.match_threshold,
            decay_lambda: config.decay_lambda,
            active_window_mins: config.active_window_mins,
            max_centroid_weight: config.max_centroid_weight,
            refinement_rounds_config: config.refinement_rounds,
            refinement_rounds_run,
            refinement_reassignments,
        },
        buckets: load_bucket_outputs(&conn)?,
    };
    let json = serde_json::to_string_pretty(&output).context("failed to serialize phase3 output")?;
    fs::write(&output_path, json)
        .with_context(|| format!("failed to write phase3 output '{}'", output_path.display()))?;
    Ok(output_path)
}

fn init_phase3_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
CREATE TABLE IF NOT EXISTS task_buckets (
    bucket_id INTEGER PRIMARY KEY AUTOINCREMENT,
    centroid_json TEXT NOT NULL,
    item_count INTEGER NOT NULL,
    created_at_epoch_secs INTEGER NOT NULL,
    last_active_epoch_secs INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS task_bucket_items (
    chunk_hash TEXT PRIMARY KEY,
    canonical_hash TEXT NOT NULL,
    bucket_id INTEGER NOT NULL,
    source_timestamp_epoch_secs INTEGER NOT NULL,
    match_score REAL NOT NULL,
    is_new_bucket INTEGER NOT NULL,
    assigned_at_epoch_secs INTEGER NOT NULL
);
"#,
    )
    .context("failed creating phase3 schema")?;
    Ok(())
}

/// When env defaults do not match phase2's stored `embeddings` rows (e.g. hash vs gemini), pick
/// the backend/model pair with the most rows so stitching still runs.
fn dominant_embedding_backend_model(conn: &Connection) -> Result<Option<(String, String)>> {
    let row: Option<(String, String)> = conn
        .query_row(
            r#"
            SELECT backend, model
            FROM embeddings
            WHERE task_type = 'RETRIEVAL_DOCUMENT'
            GROUP BY backend, model
            ORDER BY COUNT(*) DESC
            LIMIT 1
            "#,
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .context("read dominant embedding backend/model")?;
    Ok(row)
}

fn load_candidate_chunks(
    conn: &Connection,
    embedding_backend: &str,
    embed_model: &str,
) -> Result<Vec<CandidateChunk>> {
    let mut stmt = conn.prepare(
        "SELECT c.chunk_hash, c.canonical_hash, cap.timestamp_epoch_secs,
                e.embedding_json, cap.git_branch
         FROM chunks c
         JOIN captures cap ON cap.canonical_hash = c.canonical_hash
         JOIN embeddings e ON e.chunk_hash = c.chunk_hash
         WHERE e.task_type = 'RETRIEVAL_DOCUMENT'
           AND e.backend = ?1
           AND e.model = ?2
         ORDER BY cap.timestamp_epoch_secs ASC, c.chunk_index ASC",
    )?;
    let rows = stmt.query_map(params![embedding_backend, embed_model], |row| {
        let embedding_json: String = row.get(3)?;
        let embedding: Vec<f32> = serde_json::from_str(&embedding_json).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(
                3,
                rusqlite::types::Type::Text,
                Box::new(e),
            )
        })?;
        Ok(CandidateChunk {
            chunk_hash: row.get(0)?,
            canonical_hash: row.get(1)?,
            timestamp_epoch_secs: row.get(2)?,
            embedding,
            git_branch: row.get(4)?,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to parse candidate chunk row")?);
    }
    Ok(out)
}

fn has_existing_assignment(conn: &Connection, chunk_hash: &str) -> Result<bool> {
    let exists: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM task_bucket_items WHERE chunk_hash = ?1",
            params![chunk_hash],
            |row| row.get(0),
        )
        .optional()
        .context("failed checking existing task bucket assignment")?;
    Ok(exists.is_some())
}

fn load_bucket_states(conn: &Connection) -> Result<Vec<BucketState>> {
    let mut stmt = conn.prepare(
        "SELECT bucket_id, centroid_json, item_count, last_active_epoch_secs
         FROM task_buckets",
    )?;
    let rows = stmt.query_map([], |row| {
        let centroid_json: String = row.get(1)?;
        let centroid: Vec<f32> = serde_json::from_str(&centroid_json).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(
                1,
                rusqlite::types::Type::Text,
                Box::new(e),
            )
        })?;
        Ok(BucketState {
            bucket_id: row.get(0)?,
            centroid,
            item_count: row.get(2)?,
            last_active_epoch_secs: row.get(3)?,
            known_git_branches: std::collections::HashSet::new(),
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to parse task bucket row")?);
    }

    // Populate known_git_branches for each existing bucket from the DB.
    for bucket in &mut out {
        let mut bstmt = conn.prepare(
            "SELECT DISTINCT cap.git_branch
             FROM task_bucket_items tbi
             JOIN chunks c ON c.chunk_hash = tbi.chunk_hash
             JOIN captures cap ON cap.canonical_hash = c.canonical_hash
             WHERE tbi.bucket_id = ?1
               AND cap.git_branch IS NOT NULL",
        )?;
        let branch_rows = bstmt.query_map(params![bucket.bucket_id], |row| {
            row.get::<_, String>(0)
        })?;
        for b in branch_rows {
            bucket.known_git_branches.insert(b.context("git branch row")?);
        }
    }

    Ok(out)
}

fn find_best_bucket_match(
    buckets: &[BucketState],
    embedding: &[f32],
    chunk_ts: i64,
    decay_lambda: f64,
    active_window_mins: u64,
    chunk_git_branch: Option<&str>,
) -> Option<(usize, f32)> {
    let mut best: Option<(usize, f32)> = None;
    let max_age_secs = (active_window_mins.saturating_mul(60)) as i64;
    for (idx, bucket) in buckets.iter().enumerate() {
        let delta = (chunk_ts - bucket.last_active_epoch_secs).max(0);
        if max_age_secs > 0 && delta > max_age_secs {
            continue;
        }

        // Hard split: if both the chunk and the bucket have known git branches
        // and none of the bucket's branches match the chunk's branch, skip.
        if let Some(branch) = chunk_git_branch {
            if !bucket.known_git_branches.is_empty()
                && !bucket.known_git_branches.contains(branch)
            {
                continue;
            }
        }

        let sim = cosine_similarity(embedding, &bucket.centroid).unwrap_or(0.0);
        let decay = (-(decay_lambda * delta as f64)).exp() as f32;
        let score = sim * decay;
        match best {
            Some((_, current)) if score <= current => {}
            _ => best = Some((idx, score)),
        }
    }
    best
}

fn assign_to_bucket(
    conn: &Connection,
    bucket: &mut BucketState,
    chunk: &CandidateChunk,
    score: f32,
    _decay_lambda: f64,
    _active_window_mins: u64,
    max_centroid_weight: usize,
) -> Result<i64> {
    if let Some(ref b) = chunk.git_branch {
        bucket.known_git_branches.insert(b.clone());
    }
    if bucket.centroid.len() != chunk.embedding.len() {
        return Err(anyhow!(
            "embedding dim mismatch for bucket {}: {} vs {}",
            bucket.bucket_id,
            bucket.centroid.len(),
            chunk.embedding.len()
        ));
    }
    let old_count = bucket.item_count.max(1).min(max_centroid_weight as i64) as f32;
    let new_count = old_count + 1.0;
    for (idx, v) in bucket.centroid.iter_mut().enumerate() {
        *v = ((*v * old_count) + chunk.embedding[idx]) / new_count;
    }
    l2_normalize(&mut bucket.centroid);
    bucket.item_count += 1;
    bucket.last_active_epoch_secs = bucket.last_active_epoch_secs.max(chunk.timestamp_epoch_secs);
    let centroid_json =
        serde_json::to_string(&bucket.centroid).context("failed to serialize centroid")?;
    conn.execute(
        "UPDATE task_buckets
         SET centroid_json = ?1, item_count = ?2, last_active_epoch_secs = ?3
         WHERE bucket_id = ?4",
        params![
            centroid_json,
            bucket.item_count,
            bucket.last_active_epoch_secs,
            bucket.bucket_id
        ],
    )
    .context("failed updating task bucket")?;
    insert_assignment(conn, bucket.bucket_id, chunk, score, false)?;
    Ok(bucket.bucket_id)
}

fn create_bucket(conn: &Connection, chunk: &CandidateChunk) -> Result<i64> {
    let now = now_epoch_secs() as i64;
    let centroid_json = serde_json::to_string(&chunk.embedding).context("serialize centroid")?;
    conn.execute(
        "INSERT INTO task_buckets (centroid_json, item_count, created_at_epoch_secs, last_active_epoch_secs)
         VALUES (?1, 1, ?2, ?3)",
        params![centroid_json, now, chunk.timestamp_epoch_secs],
    )
    .context("failed to create task bucket")?;
    Ok(conn.last_insert_rowid())
}

fn insert_assignment(
    conn: &Connection,
    bucket_id: i64,
    chunk: &CandidateChunk,
    match_score: f32,
    is_new_bucket: bool,
) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO task_bucket_items
         (chunk_hash, canonical_hash, bucket_id, source_timestamp_epoch_secs, match_score, is_new_bucket, assigned_at_epoch_secs)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            chunk.chunk_hash,
            chunk.canonical_hash,
            bucket_id,
            chunk.timestamp_epoch_secs,
            match_score,
            if is_new_bucket { 1 } else { 0 },
            now_epoch_secs() as i64
        ],
    )
    .context("failed to insert task bucket assignment")?;
    Ok(())
}

fn load_bucket_outputs(conn: &Connection) -> Result<Vec<BucketOutput>> {
    let mut stmt = conn.prepare(
        r#"SELECT
              tb.bucket_id,
              tb.item_count,
              tb.created_at_epoch_secs,
              tb.last_active_epoch_secs,
              MIN(cap.timestamp_epoch_secs),
              MAX(cap.timestamp_epoch_secs),
              GROUP_CONCAT(DISTINCT cap.app_name)
           FROM task_buckets tb
           LEFT JOIN task_bucket_items tbi ON tbi.bucket_id = tb.bucket_id
           LEFT JOIN chunks c ON c.chunk_hash = tbi.chunk_hash
           LEFT JOIN captures cap ON cap.canonical_hash = c.canonical_hash
           GROUP BY tb.bucket_id
           ORDER BY tb.last_active_epoch_secs DESC, tb.bucket_id DESC"#,
    )?;
    let rows = stmt.query_map([], |row| {
        let apps_csv: Option<String> = row.get(6)?;
        let distinct_apps: Vec<String> = apps_csv
            .as_deref()
            .map(|s| {
                s.split(',')
                    .map(|a| a.trim().to_string())
                    .filter(|a| !a.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        Ok(BucketOutput {
            bucket_id: row.get(0)?,
            item_count: row.get::<_, i64>(1)? as usize,
            created_at_epoch_secs: row.get::<_, i64>(2)? as u64,
            last_active_epoch_secs: row.get::<_, i64>(3)? as u64,
            first_seen_epoch_secs: row
                .get::<_, Option<i64>>(4)?
                .map(|v| v as u64),
            last_seen_epoch_secs: row
                .get::<_, Option<i64>>(5)?
                .map(|v| v as u64),
            distinct_apps,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed parsing bucket output row")?);
    }
    Ok(out)
}

// ── Post-assignment refinement ────────────────────────────────────────

#[derive(Debug)]
struct RefinementItem {
    chunk_hash: String,
    canonical_hash: String,
    timestamp_epoch_secs: i64,
    embedding: Vec<f32>,
    bucket_id: i64,
    git_branch: Option<String>,
}

fn load_refinement_items(
    conn: &Connection,
    embedding_backend: &str,
    embed_model: &str,
) -> Result<Vec<RefinementItem>> {
    let mut stmt = conn.prepare(
        "SELECT tbi.chunk_hash, tbi.canonical_hash, tbi.source_timestamp_epoch_secs,
                e.embedding_json, tbi.bucket_id, cap.git_branch
         FROM task_bucket_items tbi
         JOIN embeddings e ON e.chunk_hash = tbi.chunk_hash
         JOIN captures cap ON cap.canonical_hash = tbi.canonical_hash
         WHERE e.task_type = 'RETRIEVAL_DOCUMENT'
           AND e.backend = ?1
           AND e.model = ?2
         ORDER BY tbi.source_timestamp_epoch_secs ASC",
    )?;
    let rows = stmt.query_map(params![embedding_backend, embed_model], |row| {
        let embedding_json: String = row.get(3)?;
        let embedding: Vec<f32> = serde_json::from_str(&embedding_json).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(e))
        })?;
        Ok(RefinementItem {
            chunk_hash: row.get(0)?,
            canonical_hash: row.get(1)?,
            timestamp_epoch_secs: row.get(2)?,
            embedding,
            bucket_id: row.get(4)?,
            git_branch: row.get(5)?,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("refinement item row")?);
    }
    Ok(out)
}

/// Split a bucket only when it is genuinely bimodal — i.e. the **median** pairwise
/// similarity is below `match_threshold` AND a substantial fraction of pairs are
/// below threshold. Using the median (rather than the min) prevents a single noisy
/// OCR chunk from shattering an otherwise coherent bucket into many small ones.
/// When a split is warranted, the two most dissimilar chunks seed the sub-buckets.
fn split_incoherent_buckets(items: &mut [RefinementItem], match_threshold: f32) -> usize {
    // Require this fraction of pairwise similarities to be below threshold before
    // splitting. Low fractions (single outlier pairs) are ignored.
    const FRACTION_BELOW_CUTOFF: f32 = 0.30;
    // Require at least this many items in a bucket before it is eligible to split.
    // 3 is too small — 1 outlier in a 3-item bucket flips the split trigger.
    const MIN_ITEMS_FOR_SPLIT: usize = 5;
    // Splitting only triggers when the median sim is below this margin from threshold,
    // so clusters that are only marginally below the bar are left intact.
    const MEDIAN_MARGIN: f32 = 0.04;

    let mut total_changes = 0usize;
    let mut max_bucket_id = items.iter().map(|i| i.bucket_id).max().unwrap_or(0);

    loop {
        let mut bucket_indices: HashMap<i64, Vec<usize>> = HashMap::new();
        for (idx, item) in items.iter().enumerate() {
            bucket_indices.entry(item.bucket_id).or_default().push(idx);
        }

        let mut did_split = false;

        for (&_bid, indices) in &bucket_indices {
            if indices.len() < MIN_ITEMS_FOR_SPLIT {
                continue;
            }

            let mut sims: Vec<f32> = Vec::with_capacity(indices.len() * (indices.len() - 1) / 2);
            let mut min_sim = f32::MAX;
            let mut seed_a = indices[0];
            let mut seed_b = indices[1];

            for i in 0..indices.len() {
                for j in (i + 1)..indices.len() {
                    let sim = cosine_similarity(
                        &items[indices[i]].embedding,
                        &items[indices[j]].embedding,
                    )
                    .unwrap_or(0.0);
                    sims.push(sim);
                    if sim < min_sim {
                        min_sim = sim;
                        seed_a = indices[i];
                        seed_b = indices[j];
                    }
                }
            }

            // Compute median (O(n log n) sort — n is quadratic in items, but buckets are small).
            sims.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
            let median_sim = sims[sims.len() / 2];

            let below_count = sims.iter().filter(|&&s| s < match_threshold).count();
            let fraction_below = below_count as f32 / sims.len() as f32;

            // Only split if the bucket is genuinely fractured, not just noisy.
            if median_sim >= match_threshold - MEDIAN_MARGIN
                || fraction_below < FRACTION_BELOW_CUTOFF
            {
                continue;
            }

            max_bucket_id += 1;
            let new_id = max_bucket_id;
            let keep_id = items[seed_a].bucket_id;

            eprintln!(
                "phase3: splitting bucket (median sim {:.4}, min sim {:.4}, {:.0}% pairs below threshold {:.4}, {} items) → 2 sub-buckets",
                median_sim,
                min_sim,
                fraction_below * 100.0,
                match_threshold,
                indices.len()
            );

            items[seed_b].bucket_id = new_id;
            total_changes += 1;

            for &idx in indices {
                if idx == seed_a || idx == seed_b {
                    continue;
                }
                let sim_a = cosine_similarity(&items[idx].embedding, &items[seed_a].embedding)
                    .unwrap_or(0.0);
                let sim_b = cosine_similarity(&items[idx].embedding, &items[seed_b].embedding)
                    .unwrap_or(0.0);
                if sim_b > sim_a {
                    items[idx].bucket_id = new_id;
                    total_changes += 1;
                } else if items[idx].bucket_id != keep_id {
                    items[idx].bucket_id = keep_id;
                    total_changes += 1;
                }
            }

            did_split = true;
            break;
        }

        if !did_split {
            break;
        }
    }

    total_changes
}

/// Merge any pair of buckets whose centroids are at least `match_threshold` similar.
/// Respects git-branch hard splits: two buckets with disjoint non-empty branch sets
/// are never merged, even if their centroids match. Runs iteratively until no more
/// merges are possible. Returns the number of (item-level) bucket-id changes.
fn merge_near_duplicate_buckets(items: &mut [RefinementItem], match_threshold: f32) -> usize {
    let mut total_changes = 0usize;

    loop {
        // Group items by current bucket id and compute each bucket's centroid + branch set.
        let mut grouped: HashMap<i64, Vec<usize>> = HashMap::new();
        for (idx, item) in items.iter().enumerate() {
            grouped.entry(item.bucket_id).or_default().push(idx);
        }
        if grouped.len() < 2 {
            return total_changes;
        }

        let dim = items[0].embedding.len();
        let mut bucket_ids: Vec<i64> = grouped.keys().copied().collect();
        bucket_ids.sort_unstable();

        let mut centroids: HashMap<i64, Vec<f32>> = HashMap::new();
        let mut branch_sets: HashMap<i64, std::collections::HashSet<String>> = HashMap::new();
        for &bid in &bucket_ids {
            let indices = &grouped[&bid];
            let mut c = vec![0.0f32; dim];
            let mut branches = std::collections::HashSet::new();
            for &i in indices {
                for (k, v) in items[i].embedding.iter().enumerate() {
                    c[k] += v;
                }
                if let Some(ref b) = items[i].git_branch {
                    branches.insert(b.clone());
                }
            }
            let n = indices.len() as f32;
            for v in c.iter_mut() { *v /= n; }
            l2_normalize(&mut c);
            centroids.insert(bid, c);
            branch_sets.insert(bid, branches);
        }

        // Find the single best merge candidate (highest centroid similarity above threshold)
        // and merge it in this iteration. Repeat until no candidates remain.
        let mut best: Option<(i64, i64, f32)> = None;
        for i in 0..bucket_ids.len() {
            for j in (i + 1)..bucket_ids.len() {
                let a = bucket_ids[i];
                let b = bucket_ids[j];

                // Git-branch hard split: if both sides have branches and none overlap, skip.
                let ba = &branch_sets[&a];
                let bb = &branch_sets[&b];
                if !ba.is_empty() && !bb.is_empty() && ba.is_disjoint(bb) {
                    continue;
                }

                let sim = cosine_similarity(&centroids[&a], &centroids[&b]).unwrap_or(0.0);
                if sim < match_threshold {
                    continue;
                }
                match best {
                    Some((_, _, s)) if sim <= s => {}
                    _ => best = Some((a, b, sim)),
                }
            }
        }

        match best {
            Some((keep, drop, sim)) => {
                eprintln!(
                    "phase3: merging bucket {} into {} (centroid sim {:.4} ≥ threshold {:.4})",
                    drop, keep, sim, match_threshold
                );
                for item in items.iter_mut() {
                    if item.bucket_id == drop {
                        item.bucket_id = keep;
                        total_changes += 1;
                    }
                }
            }
            None => return total_changes,
        }
    }
}

/// Two-phase refinement:
/// 1. **Cohesion split**: any bucket with min pairwise similarity below threshold gets divided
///    using the two most dissimilar chunks as seeds (handles N topics, not just 2).
/// 2. **K-means reassignment**: recompute centroids, then reassign each chunk to its best
///    centroid. Chunks below threshold spawn new buckets.
/// Returns `(rounds_actually_run, total_reassignments)`.
fn run_refinement(
    conn: &Connection,
    match_threshold: f32,
    max_rounds: usize,
    embedding_backend: &str,
    embed_model: &str,
) -> Result<(usize, usize)> {
    if max_rounds == 0 {
        return Ok((0, 0));
    }

    let mut items = load_refinement_items(conn, embedding_backend, embed_model)?;
    if items.len() < 2 {
        return Ok((0, 0));
    }

    let split_changes = split_incoherent_buckets(&mut items, match_threshold);

    let dim = items[0].embedding.len();
    let mut total_reassignments = split_changes;
    let mut rounds_run = 0usize;

    // Pre-merge: if the online-assignment phase (or a previous refinement) left behind
    // near-duplicate buckets, collapse them before k-means refinement so the centroids
    // we iterate on are not redundant.
    let pre_merge_changes = merge_near_duplicate_buckets(&mut items, match_threshold);
    total_reassignments += pre_merge_changes;

    for round in 0..max_rounds {
        rounds_run += 1;

        let mut centroid_map: HashMap<i64, (Vec<f32>, usize)> = HashMap::new();
        for item in &items {
            let entry = centroid_map
                .entry(item.bucket_id)
                .or_insert_with(|| (vec![0.0f32; dim], 0));
            for (i, v) in item.embedding.iter().enumerate() {
                entry.0[i] += v;
            }
            entry.1 += 1;
        }
        for (centroid, count) in centroid_map.values_mut() {
            let n = *count as f32;
            for v in centroid.iter_mut() {
                *v /= n;
            }
            l2_normalize(centroid);
        }

        let mut changes = 0usize;
        let mut max_id = centroid_map.keys().max().copied().unwrap_or(0);

        for item in items.iter_mut() {
            let mut best: Option<(i64, f32)> = None;
            for (&bid, (centroid, _)) in &centroid_map {
                let sim = cosine_similarity(&item.embedding, centroid).unwrap_or(0.0);
                match best {
                    Some((_, s)) if sim <= s => {}
                    _ => best = Some((bid, sim)),
                }
            }

            match best {
                Some((best_bid, best_sim)) if best_sim >= match_threshold => {
                    if best_bid != item.bucket_id {
                        item.bucket_id = best_bid;
                        changes += 1;
                    }
                }
                _ => {
                    max_id += 1;
                    let mut new_centroid = item.embedding.clone();
                    l2_normalize(&mut new_centroid);
                    centroid_map.insert(max_id, (new_centroid, 1));
                    item.bucket_id = max_id;
                    changes += 1;
                }
            }
        }

        total_reassignments += changes;

        eprintln!(
            "phase3: refinement round {} — {} reassignment(s), {} bucket(s)",
            round + 1,
            changes,
            {
                let mut ids: Vec<i64> = items.iter().map(|i| i.bucket_id).collect();
                ids.sort_unstable();
                ids.dedup();
                ids.len()
            }
        );

        if changes == 0 {
            break;
        }
    }

    // Post-merge: after k-means has stabilized, some buckets may have centroids that
    // converged close to each other (e.g. near-duplicate OCR captures that the splitter
    // separated on noise). Collapse those now — this is the fix for "two buckets with
    // exactly the same content" showing up in the summary.
    let post_merge_changes = merge_near_duplicate_buckets(&mut items, match_threshold);
    if post_merge_changes > 0 {
        eprintln!(
            "phase3: post-refinement merge collapsed {} item assignment(s)",
            post_merge_changes
        );
    }
    total_reassignments += post_merge_changes;

    if total_reassignments == 0 {
        return Ok((rounds_run, 0));
    }

    persist_refined_state(conn, &items)?;
    Ok((rounds_run, total_reassignments))
}

/// Wipe `task_buckets` / `task_bucket_items` and rebuild from the refined in-memory assignments.
fn persist_refined_state(conn: &Connection, items: &[RefinementItem]) -> Result<()> {
    let mut bucket_groups: HashMap<i64, Vec<usize>> = HashMap::new();
    for (idx, item) in items.iter().enumerate() {
        bucket_groups.entry(item.bucket_id).or_default().push(idx);
    }

    conn.execute_batch("BEGIN IMMEDIATE")
        .context("begin refinement transaction")?;

    let result = (|| -> Result<()> {
        conn.execute("DELETE FROM task_bucket_items", [])
            .context("clear task_bucket_items")?;
        conn.execute("DELETE FROM task_buckets", [])
            .context("clear task_buckets")?;

        let now = now_epoch_secs() as i64;

        for indices in bucket_groups.values() {
            if indices.is_empty() {
                continue;
            }
            let dim = items[indices[0]].embedding.len();
            let mut centroid = vec![0.0f32; dim];
            for &idx in indices {
                for (i, v) in items[idx].embedding.iter().enumerate() {
                    centroid[i] += v;
                }
            }
            let n = indices.len() as f32;
            for v in centroid.iter_mut() {
                *v /= n;
            }
            l2_normalize(&mut centroid);

            let centroid_json =
                serde_json::to_string(&centroid).context("serialize refined centroid")?;
            let item_count = indices.len() as i64;
            let last_active = indices
                .iter()
                .map(|&i| items[i].timestamp_epoch_secs)
                .max()
                .unwrap_or(now);

            conn.execute(
                "INSERT INTO task_buckets (centroid_json, item_count, created_at_epoch_secs, last_active_epoch_secs)
                 VALUES (?1, ?2, ?3, ?4)",
                params![centroid_json, item_count, now, last_active],
            )
            .context("insert refined bucket")?;
            let real_bucket_id = conn.last_insert_rowid();

            for &idx in indices {
                let item = &items[idx];
                conn.execute(
                    "INSERT OR REPLACE INTO task_bucket_items
                     (chunk_hash, canonical_hash, bucket_id, source_timestamp_epoch_secs,
                      match_score, is_new_bucket, assigned_at_epoch_secs)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        item.chunk_hash,
                        item.canonical_hash,
                        real_bucket_id,
                        item.timestamp_epoch_secs,
                        1.0f32,
                        0,
                        now
                    ],
                )
                .context("insert refined assignment")?;
            }
        }
        Ok(())
    })();

    match result {
        Ok(()) => {
            conn.execute_batch("COMMIT")
                .context("commit refinement")?;
            eprintln!(
                "phase3: refinement persisted {} bucket(s) from {} chunk(s)",
                bucket_groups.len(),
                items.len()
            );
            Ok(())
        }
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(e)
        }
    }
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> Option<f32> {
    if a.len() != b.len() || a.is_empty() {
        return None;
    }
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    let denom = na.sqrt() * nb.sqrt();
    if denom.partial_cmp(&0.0) != Some(Ordering::Greater) {
        return None;
    }
    Some(dot / denom)
}

fn l2_normalize(vec: &mut [f32]) {
    let norm = vec.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm.partial_cmp(&0.0) == Some(Ordering::Greater) {
        for v in vec.iter_mut() {
            *v /= norm;
        }
    }
}

fn default_output_path(now_epoch: u64) -> PathBuf {
    PathBuf::from("logs").join(format!("phase3-stitching-{now_epoch}.json"))
}

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
