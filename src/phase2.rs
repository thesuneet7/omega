use crate::models::{Phase1Payload, VisualLogItem};
use anyhow::{anyhow, Context, Result};
use regex::Regex;
use reqwest::blocking::{Client, Response};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Deserialize)]
struct SessionLogFile {
    payloads: Vec<Phase1Payload>,
}

#[derive(Debug, Serialize)]
pub struct IngestionSummary {
    pub source_log_path: String,
    pub db_path: String,
    pub generated_at_epoch_secs: u64,
    pub items_read: usize,
    pub chunks_created: usize,
    pub chunks_embedded: usize,
    pub chunks_reused_from_db: usize,
    pub embedding_backend: String,
    pub embedding_model: String,
    pub pii_redaction_enabled: bool,
}

#[derive(Debug, Serialize)]
pub struct IngestedChunkItem {
    pub source_visual_id: u64,
    pub chunk_index: usize,
    pub chunk_hash: String,
    pub canonical_text_sha256: String,
    pub embedding_dim: usize,
    pub reused_existing_embedding: bool,
}

#[derive(Debug, Serialize)]
pub struct IngestionOutput {
    pub summary: IngestionSummary,
    pub chunks: Vec<IngestedChunkItem>,
}

#[derive(Debug, Clone)]
pub enum EmbeddingBackend {
    Gemini,
    OpenAI,
    Hash,
}

impl EmbeddingBackend {
    fn from_env() -> Result<Self> {
        let raw = std::env::var("OMEGA_EMBEDDING_BACKEND").unwrap_or_else(|_| "gemini".to_string());
        match raw.trim().to_ascii_lowercase().as_str() {
            "gemini" => Ok(Self::Gemini),
            "openai" => Ok(Self::OpenAI),
            "hash" => Ok(Self::Hash),
            other => Err(anyhow!(
                "unsupported OMEGA_EMBEDDING_BACKEND='{other}', expected 'gemini', 'openai', or 'hash'"
            )),
        }
    }
}

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
enum EmbedTaskType {
    RetrievalDocument,
    RetrievalQuery,
}

impl EmbedTaskType {
    fn as_str(self) -> &'static str {
        match self {
            Self::RetrievalDocument => "RETRIEVAL_DOCUMENT",
            Self::RetrievalQuery => "RETRIEVAL_QUERY",
        }
    }
}

#[derive(Debug, Clone)]
pub struct IngestionConfig {
    pub input_log: Option<PathBuf>,
    pub output_path: Option<PathBuf>,
    pub backend: EmbeddingBackend,
    pub db_path: PathBuf,
    pub gemini_base_url: String,
    pub gemini_api_key: Option<String>,
    pub openai_base_url: String,
    pub openai_api_key: Option<String>,
    pub embed_model: String,
    pub chunk_size_chars: usize,
    pub chunk_overlap_chars: usize,
    pub redact_pii: bool,
    pub max_retries: usize,
    pub retry_base_delay_ms: u64,
}

impl IngestionConfig {
    pub fn from_env_and_args(
        input_log: Option<PathBuf>,
        output_path: Option<PathBuf>,
    ) -> Result<Self> {
        let backend = EmbeddingBackend::from_env()?;
        let gemini_base_url = std::env::var("OMEGA_GEMINI_BASE_URL")
            .unwrap_or_else(|_| "https://generativelanguage.googleapis.com".to_string());
        let gemini_api_key = std::env::var("OMEGA_GEMINI_API_KEY").ok();
        let openai_base_url = std::env::var("OMEGA_OPENAI_BASE_URL")
            .unwrap_or_else(|_| "https://api.openai.com".to_string());
        let openai_api_key = std::env::var("OMEGA_OPENAI_API_KEY").ok();
        let embed_model =
            std::env::var("OMEGA_EMBED_MODEL").unwrap_or_else(|_| "text-embedding-004".to_string());
        let db_path = PathBuf::from(
            std::env::var("OMEGA_PHASE2_DB_PATH").unwrap_or_else(|_| "logs/phase2.db".to_string()),
        );
        let chunk_size_chars = std::env::var("OMEGA_CHUNK_SIZE_CHARS")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(1200);
        let chunk_overlap_chars = std::env::var("OMEGA_CHUNK_OVERLAP_CHARS")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(200);
        let redact_pii = std::env::var("OMEGA_REDACT_PII")
            .unwrap_or_else(|_| "true".to_string())
            .to_ascii_lowercase()
            != "false";
        let max_retries = std::env::var("OMEGA_EMBED_MAX_RETRIES")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(3);
        let retry_base_delay_ms = std::env::var("OMEGA_EMBED_RETRY_BASE_DELAY_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(500);

        Ok(Self {
            input_log,
            output_path,
            backend,
            db_path,
            gemini_base_url,
            gemini_api_key,
            openai_base_url,
            openai_api_key,
            embed_model,
            chunk_size_chars,
            chunk_overlap_chars,
            redact_pii,
            max_retries,
            retry_base_delay_ms,
        })
    }
}

#[derive(Debug, Serialize)]
struct GeminiEmbeddingRequest<'a> {
    content: GeminiContent<'a>,
    #[serde(rename = "taskType")]
    task_type: &'a str,
}

#[derive(Debug, Serialize)]
struct GeminiContent<'a> {
    parts: Vec<GeminiPart<'a>>,
}

#[derive(Debug, Serialize)]
struct GeminiPart<'a> {
    text: &'a str,
}

#[derive(Debug, Deserialize)]
struct GeminiEmbeddingResponse {
    embedding: GeminiEmbeddingData,
}

#[derive(Debug, Deserialize)]
struct GeminiEmbeddingData {
    values: Vec<f32>,
}

#[derive(Debug, Serialize)]
struct OpenAIEmbeddingRequest<'a> {
    model: &'a str,
    input: &'a str,
}

#[derive(Debug, Deserialize)]
struct OpenAIEmbeddingResponse {
    data: Vec<OpenAIEmbeddingData>,
}

#[derive(Debug, Deserialize)]
struct OpenAIEmbeddingData {
    embedding: Vec<f32>,
}

trait EmbeddingProvider {
    fn embed(&self, text: &str, task_type: EmbedTaskType) -> Result<Vec<f32>>;
    fn backend_name(&self) -> &'static str;
    fn model_name(&self) -> &str;
}

struct GeminiProvider {
    client: Client,
    base_url: String,
    api_key: String,
    model: String,
    max_retries: usize,
    retry_base_delay_ms: u64,
}

struct OpenAIProvider {
    client: Client,
    base_url: String,
    api_key: String,
    model: String,
    max_retries: usize,
    retry_base_delay_ms: u64,
}

struct HashProvider {
    model: String,
}

impl EmbeddingProvider for GeminiProvider {
    fn embed(&self, text: &str, task_type: EmbedTaskType) -> Result<Vec<f32>> {
        with_retries(self.max_retries, self.retry_base_delay_ms, || {
            let endpoint = format!(
                "{}/v1beta/models/{}:embedContent",
                self.base_url.trim_end_matches('/'),
                self.model
            );
            let body = GeminiEmbeddingRequest {
                content: GeminiContent {
                    parts: vec![GeminiPart { text }],
                },
                task_type: task_type.as_str(),
            };

            let response = self
                .client
                .post(&endpoint)
                .query(&[("key", self.api_key.as_str())])
                .json(&body)
                .send()
                .with_context(|| {
                    format!("failed to connect to Gemini embeddings endpoint at {endpoint}")
                })?;

            maybe_retryable_response(response, "gemini", |resp| {
                let parsed: GeminiEmbeddingResponse = resp
                    .json()
                    .context("failed to parse Gemini embedding response")?;
                if parsed.embedding.values.is_empty() {
                    return Err(anyhow!("gemini returned an empty embedding vector"));
                }
                Ok(parsed.embedding.values)
            })
        })
    }

    fn backend_name(&self) -> &'static str {
        "gemini"
    }

    fn model_name(&self) -> &str {
        &self.model
    }
}

impl EmbeddingProvider for OpenAIProvider {
    fn embed(&self, text: &str, _task_type: EmbedTaskType) -> Result<Vec<f32>> {
        with_retries(self.max_retries, self.retry_base_delay_ms, || {
            let endpoint = format!("{}/v1/embeddings", self.base_url.trim_end_matches('/'));
            let body = OpenAIEmbeddingRequest {
                model: &self.model,
                input: text,
            };
            let response = self
                .client
                .post(&endpoint)
                .bearer_auth(&self.api_key)
                .json(&body)
                .send()
                .with_context(|| {
                    format!("failed to connect to embeddings endpoint at {endpoint}")
                })?;

            maybe_retryable_response(response, "openai", |resp| {
                let parsed: OpenAIEmbeddingResponse = resp
                    .json()
                    .context("failed to parse OpenAI-compatible embedding response")?;
                let embedding = parsed
                    .data
                    .into_iter()
                    .next()
                    .ok_or_else(|| anyhow!("embedding response missing data[0]"))?
                    .embedding;
                if embedding.is_empty() {
                    return Err(anyhow!(
                        "embedding backend returned an empty embedding vector"
                    ));
                }
                Ok(embedding)
            })
        })
    }

    fn backend_name(&self) -> &'static str {
        "openai"
    }

    fn model_name(&self) -> &str {
        &self.model
    }
}

impl EmbeddingProvider for HashProvider {
    fn embed(&self, text: &str, _task_type: EmbedTaskType) -> Result<Vec<f32>> {
        Ok(embed_with_hash(text, 384))
    }

    fn backend_name(&self) -> &'static str {
        "hash"
    }

    fn model_name(&self) -> &str {
        &self.model
    }
}

pub fn run_ingestion(config: IngestionConfig) -> Result<PathBuf> {
    let source_log = match &config.input_log {
        Some(path) => path.clone(),
        None => latest_capture_log_path(Path::new("logs"))?,
    };

    let content = fs::read_to_string(&source_log)
        .with_context(|| format!("failed to read session log '{}'", source_log.display()))?;
    let session: SessionLogFile = serde_json::from_str(&content)
        .with_context(|| format!("failed to parse JSON in '{}'", source_log.display()))?;

    let provider = create_provider(&config)?;
    let db = open_db(&config.db_path)?;
    init_schema(&db)?;
    let mut chunks_created = 0usize;
    let mut chunks_embedded = 0usize;
    let mut chunks_reused_from_db = 0usize;
    let mut output_chunks: Vec<IngestedChunkItem> = Vec::new();

    for payload in &session.payloads {
        let Phase1Payload::Visual(visual) = payload;
        let canonical_text = to_canonical_text(visual, config.redact_pii)?;
        let canonical_hash = sha256_hex(&canonical_text);
        persist_capture(&db, visual, &canonical_text, &canonical_hash)?;

        let chunks = chunk_text(
            &canonical_text,
            config.chunk_size_chars.max(100),
            config
                .chunk_overlap_chars
                .min(config.chunk_size_chars.saturating_sub(1)),
        );

        for (chunk_index, chunk_text) in chunks.into_iter().enumerate() {
            chunks_created += 1;
            let chunk_hash = sha256_hex(&format!("{canonical_hash}:{chunk_index}:{chunk_text}"));
            persist_chunk(&db, &chunk_hash, &canonical_hash, chunk_index, &chunk_text)?;

            let existing = read_existing_embedding(
                &db,
                &chunk_hash,
                provider.backend_name(),
                provider.model_name(),
            )?;
            let (embedding, reused_existing) = match existing {
                Some(vec) => {
                    chunks_reused_from_db += 1;
                    (vec, true)
                }
                None => {
                    let vec = provider.embed(&chunk_text, EmbedTaskType::RetrievalDocument)?;
                    persist_embedding(
                        &db,
                        &chunk_hash,
                        provider.backend_name(),
                        provider.model_name(),
                        EmbedTaskType::RetrievalDocument.as_str(),
                        &vec,
                    )?;
                    chunks_embedded += 1;
                    (vec, false)
                }
            };

            output_chunks.push(IngestedChunkItem {
                source_visual_id: visual.id,
                chunk_index,
                chunk_hash,
                canonical_text_sha256: canonical_hash.clone(),
                embedding_dim: embedding.len(),
                reused_existing_embedding: reused_existing,
            });
        }
    }

    let now_epoch = now_epoch_secs();
    let output_path = match config.output_path {
        Some(path) => path,
        None => default_output_path(now_epoch),
    };
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create output directory '{}'", parent.display()))?;
    }

    let output = IngestionOutput {
        summary: IngestionSummary {
            source_log_path: source_log.display().to_string(),
            db_path: config.db_path.display().to_string(),
            generated_at_epoch_secs: now_epoch,
            items_read: session.payloads.len(),
            chunks_created,
            chunks_embedded,
            chunks_reused_from_db,
            embedding_backend: provider.backend_name().to_string(),
            embedding_model: provider.model_name().to_string(),
            pii_redaction_enabled: config.redact_pii,
        },
        chunks: output_chunks,
    };

    let json =
        serde_json::to_string_pretty(&output).context("failed to serialize ingestion output")?;
    fs::write(&output_path, json).with_context(|| {
        format!(
            "failed to write ingestion output '{}'",
            output_path.display()
        )
    })?;

    Ok(output_path)
}

fn create_provider(config: &IngestionConfig) -> Result<Box<dyn EmbeddingProvider>> {
    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .context("failed to create HTTP client for embeddings")?;
    match config.backend {
        EmbeddingBackend::Gemini => {
            let api_key = config
                .gemini_api_key
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    anyhow!("missing OMEGA_GEMINI_API_KEY (required when backend=gemini)")
                })?
                .to_string();
            Ok(Box::new(GeminiProvider {
                client,
                base_url: config.gemini_base_url.clone(),
                api_key,
                model: config.embed_model.clone(),
                max_retries: config.max_retries,
                retry_base_delay_ms: config.retry_base_delay_ms,
            }))
        }
        EmbeddingBackend::OpenAI => {
            let api_key = config
                .openai_api_key
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    anyhow!("missing OMEGA_OPENAI_API_KEY (required when backend=openai)")
                })?
                .to_string();
            Ok(Box::new(OpenAIProvider {
                client,
                base_url: config.openai_base_url.clone(),
                api_key,
                model: config.embed_model.clone(),
                max_retries: config.max_retries,
                retry_base_delay_ms: config.retry_base_delay_ms,
            }))
        }
        EmbeddingBackend::Hash => Ok(Box::new(HashProvider {
            model: config.embed_model.clone(),
        })),
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
            "[retryable] {} embedding request failed status={} body={}",
            source,
            status,
            response_body
        ))
    } else {
        Err(anyhow!(
            "{} embedding request failed status={} body={}",
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

fn open_db(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create db parent directory '{}'",
                parent.display()
            )
        })?;
    }
    Connection::open(path).with_context(|| format!("failed to open sqlite db '{}'", path.display()))
}

fn init_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
CREATE TABLE IF NOT EXISTS captures (
    canonical_hash TEXT PRIMARY KEY,
    source_visual_id INTEGER NOT NULL,
    timestamp_epoch_secs INTEGER NOT NULL,
    app_name TEXT NOT NULL,
    window_title TEXT NOT NULL,
    event_type TEXT NOT NULL,
    ocr_engine_used TEXT NOT NULL,
    canonical_text TEXT NOT NULL,
    created_at_epoch_secs INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS chunks (
    chunk_hash TEXT PRIMARY KEY,
    canonical_hash TEXT NOT NULL,
    chunk_index INTEGER NOT NULL,
    chunk_text TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS embeddings (
    chunk_hash TEXT NOT NULL,
    backend TEXT NOT NULL,
    model TEXT NOT NULL,
    task_type TEXT NOT NULL,
    embedding_json TEXT NOT NULL,
    embedding_dim INTEGER NOT NULL,
    created_at_epoch_secs INTEGER NOT NULL,
    PRIMARY KEY (chunk_hash, backend, model, task_type)
);
"#,
    )
    .context("failed creating sqlite schema")?;
    Ok(())
}

fn persist_capture(
    conn: &Connection,
    visual: &VisualLogItem,
    canonical_text: &str,
    canonical_hash: &str,
) -> Result<()> {
    let ts_secs = visual
        .timestamp
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    conn.execute(
        "INSERT OR IGNORE INTO captures (canonical_hash, source_visual_id, timestamp_epoch_secs, app_name, window_title, event_type, ocr_engine_used, canonical_text, created_at_epoch_secs)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            canonical_hash,
            visual.id as i64,
            ts_secs,
            visual.app_name,
            visual.window_title,
            visual.event_type,
            visual.ocr_engine_used,
            canonical_text,
            now_epoch_secs() as i64
        ],
    )
    .context("failed to persist capture")?;
    Ok(())
}

fn persist_chunk(
    conn: &Connection,
    chunk_hash: &str,
    canonical_hash: &str,
    chunk_index: usize,
    chunk_text: &str,
) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO chunks (chunk_hash, canonical_hash, chunk_index, chunk_text)
         VALUES (?1, ?2, ?3, ?4)",
        params![chunk_hash, canonical_hash, chunk_index as i64, chunk_text],
    )
    .context("failed to persist chunk")?;
    Ok(())
}

fn persist_embedding(
    conn: &Connection,
    chunk_hash: &str,
    backend: &str,
    model: &str,
    task_type: &str,
    embedding: &[f32],
) -> Result<()> {
    let embedding_json =
        serde_json::to_string(embedding).context("failed to serialize embedding vector")?;
    conn.execute(
        "INSERT OR REPLACE INTO embeddings (chunk_hash, backend, model, task_type, embedding_json, embedding_dim, created_at_epoch_secs)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            chunk_hash,
            backend,
            model,
            task_type,
            embedding_json,
            embedding.len() as i64,
            now_epoch_secs() as i64
        ],
    )
    .context("failed to persist embedding")?;
    Ok(())
}

fn read_existing_embedding(
    conn: &Connection,
    chunk_hash: &str,
    backend: &str,
    model: &str,
) -> Result<Option<Vec<f32>>> {
    let maybe_json: Option<String> = conn
        .query_row(
            "SELECT embedding_json FROM embeddings WHERE chunk_hash = ?1 AND backend = ?2 AND model = ?3 AND task_type = ?4",
            params![chunk_hash, backend, model, EmbedTaskType::RetrievalDocument.as_str()],
            |row| row.get(0),
        )
        .optional()
        .context("failed to query existing embedding")?;

    maybe_json
        .map(|json| {
            serde_json::from_str::<Vec<f32>>(&json).context("failed to parse stored embedding_json")
        })
        .transpose()
}

fn latest_capture_log_path(logs_dir: &Path) -> Result<PathBuf> {
    let entries = fs::read_dir(logs_dir)
        .with_context(|| format!("failed to read logs directory '{}'", logs_dir.display()))?;

    let mut candidates: Vec<PathBuf> = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with("capture-session-") && name.ends_with(".json") {
            candidates.push(path);
        }
    }

    candidates.sort_by(|a, b| {
        let an = a.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        let bn = b.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        an.cmp(bn)
    });
    candidates.pop().ok_or_else(|| {
        anyhow!(
            "no capture-session-*.json files found in '{}'",
            logs_dir.display()
        )
    })
}

fn to_canonical_text(item: &VisualLogItem, redact_pii: bool) -> Result<String> {
    let ts = item
        .timestamp
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string());
    let ocr_raw = normalize_text(&item.ocr_text);
    let ocr = if redact_pii {
        redact_sensitive_text(&ocr_raw)?
    } else {
        ocr_raw
    };
    Ok(format!(
        "timestamp={ts}\napp_name={}\nwindow_title={}\nevent_type={}\nocr_engine={}\nresolution={}x{}\nocr_text:\n{}",
        normalize_text(&item.app_name),
        normalize_text(&item.window_title),
        normalize_text(&item.event_type),
        normalize_text(&item.ocr_engine_used),
        item.width,
        item.height,
        ocr
    ))
}

fn chunk_text(input: &str, chunk_size: usize, overlap: usize) -> Vec<String> {
    if input.len() <= chunk_size {
        return vec![input.to_string()];
    }
    let chars: Vec<char> = input.chars().collect();
    let mut out = Vec::new();
    let mut start = 0usize;
    while start < chars.len() {
        let end = (start + chunk_size).min(chars.len());
        let chunk: String = chars[start..end].iter().collect();
        out.push(chunk);
        if end == chars.len() {
            break;
        }
        start = end.saturating_sub(overlap);
    }
    out
}

fn normalize_text(input: &str) -> String {
    input
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn redact_sensitive_text(input: &str) -> Result<String> {
    let email = Regex::new(r"(?i)\b[A-Z0-9._%+\-]+@[A-Z0-9.\-]+\.[A-Z]{2,}\b")
        .context("failed to build email regex")?;
    let phone = Regex::new(r"\b(?:\+?\d{1,3}[-.\s]?)?(?:\d[-.\s]?){9,12}\b")
        .context("failed to build phone regex")?;
    let cc = Regex::new(r"\b(?:\d[ -]*?){13,19}\b").context("failed to build card regex")?;

    let s = email.replace_all(input, "[redacted_email]");
    let s = phone.replace_all(&s, "[redacted_phone]");
    let s = cc.replace_all(&s, "[redacted_card]");
    Ok(s.into_owned())
}

fn embed_with_hash(text: &str, dim: usize) -> Vec<f32> {
    let mut out = Vec::with_capacity(dim);
    let mut seed = sha256_hex(text);
    while out.len() < dim {
        let mut hasher = Sha256::new();
        hasher.update(seed.as_bytes());
        let bytes = hasher.finalize();
        seed = hex::encode(bytes);
        for chunk in seed.as_bytes().chunks(4) {
            if out.len() >= dim {
                break;
            }
            let mut value = 0u32;
            for b in chunk {
                value = value.wrapping_mul(37).wrapping_add(*b as u32);
            }
            let normalized = ((value % 2000) as f32 / 1000.0) - 1.0;
            out.push(normalized);
        }
    }
    l2_normalize(&mut out);
    out
}

fn l2_normalize(vec: &mut [f32]) {
    let norm = vec.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm.partial_cmp(&0.0) == Some(Ordering::Greater) {
        for v in vec.iter_mut() {
            *v /= norm;
        }
    }
}

fn sha256_hex(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    hex::encode(hasher.finalize())
}

fn default_output_path(now_epoch: u64) -> PathBuf {
    PathBuf::from("logs").join(format!("phase2-ingestion-{now_epoch}.json"))
}

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
