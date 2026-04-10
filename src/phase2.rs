use crate::models::{Phase1Payload, VisualLogItem};
use crate::openai_compat_url::{ensure_openai_v1_base, openai_embeddings_url};
use anyhow::{anyhow, Context, Result};
use regex::Regex;
use reqwest::blocking::{Client, Response};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
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
    /// Total UTF-8 characters embedded via the remote provider this run (new API calls only).
    #[serde(default)]
    pub embedded_input_chars: usize,
    pub chunks_reused_from_db: usize,
    pub embedding_backend: String,
    pub embedding_model: String,
    pub pii_redaction_enabled: bool,
    /// `semantic` (default): embed mostly cleaned OCR + coarse app context — best for Phase 3 clustering. `full`: legacy verbose canonical text.
    pub canonical_mode: String,
    pub ocr_clean_enabled: bool,
    pub ocr_line_score_ratio: f32,
    pub ocr_emphasize_top: bool,
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
    /// OpenAI-compatible `POST …/v1/embeddings` using `OMEGA_XAI_API_KEY` and `OMEGA_BASE_URL`.
    Xai,
    Hash,
}

impl EmbeddingBackend {
    pub fn from_env() -> Result<Self> {
        let raw = std::env::var("OMEGA_EMBEDDING_BACKEND").unwrap_or_else(|_| "gemini".to_string());
        match raw.trim().to_ascii_lowercase().as_str() {
            "gemini" => Ok(Self::Gemini),
            "openai" => Ok(Self::OpenAI),
            "xai" => Ok(Self::Xai),
            "hash" => Ok(Self::Hash),
            other => Err(anyhow!(
                "unsupported OMEGA_EMBEDDING_BACKEND='{other}', expected 'gemini', 'openai', 'xai', or 'hash'"
            )),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Gemini => "gemini",
            Self::OpenAI => "openai",
            Self::Xai => "xai",
            Self::Hash => "hash",
        }
    }
}

#[derive(Debug, Deserialize)]
struct XaiModelsResponse {
    data: Vec<XaiModelItem>,
}

#[derive(Debug, Deserialize)]
struct XaiModelItem {
    id: String,
}

/// If `OMEGA_EMBED_MODEL` is unset, call `GET /v1/models` and pick the first model id containing `embed`.
fn discover_xai_embedding_model(base_url: &str, api_key: &str) -> Result<String> {
    let client = Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .context("failed to create HTTP client for xAI model discovery")?;
    let url = format!("{}/models", ensure_openai_v1_base(base_url));
    let response = client
        .get(&url)
        .bearer_auth(api_key)
        .send()
        .with_context(|| format!("failed to GET {url}"))?;
    let status = response.status();
    let body = response.text().unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow!(
            "xAI list models failed status={status} body={body}"
        ));
    }
    let parsed: XaiModelsResponse =
        serde_json::from_str(&body).context("parse xAI GET /v1/models JSON")?;
    let ids: Vec<String> = parsed.data.into_iter().map(|m| m.id).collect();
    let pick = |needle: &str| {
        ids.iter()
            .find(|id| id.to_lowercase().contains(needle))
            .cloned()
    };
    if let Some(m) = pick("embed") {
        return Ok(m);
    }
    if let Some(m) = pick("embedding") {
        return Ok(m);
    }
    Err(anyhow!(
        "no embedding-capable model found in GET /v1/models (need an id containing 'embed'). \
         Set OMEGA_EMBED_MODEL to your team’s embedding model id. Available ids: {}",
        ids.join(", ")
    ))
}

/// Resolves `OMEGA_EMBED_MODEL` per backend; for xAI with no env var, discovers via GET /v1/models.
pub fn resolve_embed_model_for_backend(backend: &EmbeddingBackend) -> Result<String> {
    let explicit = std::env::var("OMEGA_EMBED_MODEL")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    match backend {
        EmbeddingBackend::Xai => {
            if let Some(m) = explicit {
                return Ok(m);
            }
            let base_url = std::env::var("OMEGA_BASE_URL")
                .unwrap_or_else(|_| "https://api.x.ai/v1".to_string());
            let api_key = std::env::var("OMEGA_XAI_API_KEY")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    anyhow!("missing OMEGA_XAI_API_KEY (required when OMEGA_EMBEDDING_BACKEND=xai)")
                })?;
            discover_xai_embedding_model(&base_url, &api_key)
        }
        EmbeddingBackend::Gemini => Ok(explicit.unwrap_or_else(|| "gemini-embedding-001".to_string())),
        EmbeddingBackend::OpenAI => Ok(explicit.unwrap_or_else(|| "text-embedding-3-small".to_string())),
        EmbeddingBackend::Hash => Ok(explicit.unwrap_or_else(|| "hash".to_string())),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanonicalMode {
    /// Best for semantic stitching: stable across scrolls/tabs (excludes volatile window title, timestamp from embedded text).
    Semantic,
    /// Original format with full metadata in the embedded string (more unique per capture).
    Full,
}

impl CanonicalMode {
    fn from_env() -> Result<Self> {
        let raw =
            std::env::var("OMEGA_PHASE2_CANONICAL_MODE").unwrap_or_else(|_| "semantic".to_string());
        match raw.trim().to_ascii_lowercase().as_str() {
            "semantic" => Ok(Self::Semantic),
            "full" => Ok(Self::Full),
            other => Err(anyhow!(
                "unsupported OMEGA_PHASE2_CANONICAL_MODE='{other}', expected 'semantic' or 'full'"
            )),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Semantic => "semantic",
            Self::Full => "full",
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
    pub canonical_mode: CanonicalMode,
    pub ocr_clean_enabled: bool,
    /// Keep lines whose content score is at least this fraction of the best line in the same capture (0.0–1.0).
    pub ocr_line_score_ratio: f32,
    /// Duplicate the top-scoring line(s) once at the end so embeddings emphasize real body text over stray UI lines.
    pub ocr_emphasize_top: bool,
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
        let embed_model = resolve_embed_model_for_backend(&backend)?;
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
        let canonical_mode = CanonicalMode::from_env()?;
        let ocr_clean_enabled = std::env::var("OMEGA_PHASE2_OCR_CLEAN")
            .unwrap_or_else(|_| "true".to_string())
            .to_ascii_lowercase()
            != "false";
        let ocr_line_score_ratio = std::env::var("OMEGA_PHASE2_OCR_LINE_SCORE_RATIO")
            .ok()
            .and_then(|v| v.parse::<f32>().ok())
            .filter(|&r| r >= 0.0 && r <= 1.0)
            .unwrap_or(0.12);
        let ocr_emphasize_top = std::env::var("OMEGA_PHASE2_OCR_EMPHASIS_TOP")
            .unwrap_or_else(|_| "true".to_string())
            .to_ascii_lowercase()
            != "false";

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
            canonical_mode,
            ocr_clean_enabled,
            ocr_line_score_ratio,
            ocr_emphasize_top,
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

/// OpenAI- or xAI-compatible `POST /v1/embeddings`.
struct OpenAiCompatEmbeddingProvider {
    client: Client,
    base_url: String,
    api_key: String,
    model: String,
    max_retries: usize,
    retry_base_delay_ms: u64,
    backend_name: &'static str,
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

impl EmbeddingProvider for OpenAiCompatEmbeddingProvider {
    fn embed(&self, text: &str, _task_type: EmbedTaskType) -> Result<Vec<f32>> {
        let source = self.backend_name;
        with_retries(self.max_retries, self.retry_base_delay_ms, || {
            let endpoint = openai_embeddings_url(&self.base_url);
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

            maybe_retryable_response(response, source, |resp| {
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
        self.backend_name
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

pub fn run_ingestion(config: IngestionConfig) -> Result<(PathBuf, IngestionSummary)> {
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
    let mut embedded_input_chars = 0usize;
    let mut chunks_reused_from_db = 0usize;
    let mut output_chunks: Vec<IngestedChunkItem> = Vec::new();

    for payload in &session.payloads {
        let Phase1Payload::Visual(visual) = payload;
        let canonical_text = to_canonical_text(visual, &config)?;
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
                    embedded_input_chars += chunk_text.chars().count();
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
            embedded_input_chars,
            chunks_reused_from_db,
            embedding_backend: provider.backend_name().to_string(),
            embedding_model: provider.model_name().to_string(),
            pii_redaction_enabled: config.redact_pii,
            canonical_mode: config.canonical_mode.as_str().to_string(),
            ocr_clean_enabled: config.ocr_clean_enabled,
            ocr_line_score_ratio: config.ocr_line_score_ratio,
            ocr_emphasize_top: config.ocr_emphasize_top,
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

    Ok((output_path, output.summary))
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
            Ok(Box::new(OpenAiCompatEmbeddingProvider {
                client,
                base_url: config.openai_base_url.clone(),
                api_key,
                model: config.embed_model.clone(),
                max_retries: config.max_retries,
                retry_base_delay_ms: config.retry_base_delay_ms,
                backend_name: "openai",
            }))
        }
        EmbeddingBackend::Xai => {
            let base_url = std::env::var("OMEGA_BASE_URL")
                .unwrap_or_else(|_| "https://api.x.ai/v1".to_string());
            let api_key = std::env::var("OMEGA_XAI_API_KEY")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    anyhow!("missing OMEGA_XAI_API_KEY (required when backend=xai)")
                })?;
            Ok(Box::new(OpenAiCompatEmbeddingProvider {
                client,
                base_url,
                api_key,
                model: config.embed_model.clone(),
                max_retries: config.max_retries,
                retry_base_delay_ms: config.retry_base_delay_ms,
                backend_name: "xai",
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

fn to_canonical_text(item: &VisualLogItem, config: &IngestionConfig) -> Result<String> {
    let ocr_raw = normalize_text(&item.ocr_text);
    let ocr_after_pii = if config.redact_pii {
        redact_sensitive_text(&ocr_raw)?
    } else {
        ocr_raw
    };
    let ocr = if config.ocr_clean_enabled {
        clean_ocr_for_semantics(&ocr_after_pii, config)
    } else {
        ocr_after_pii
    };

    match config.canonical_mode {
        CanonicalMode::Semantic => {
            let app = normalize_text(&item.app_name);
            if app.is_empty() && ocr.is_empty() {
                Ok("content:\n".to_string())
            } else if app.is_empty() {
                Ok(format!("content:\n{ocr}"))
            } else {
                Ok(format!("app: {app}\ncontent:\n{ocr}"))
            }
        }
        CanonicalMode::Full => {
            let ts = item
                .timestamp
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs().to_string())
                .unwrap_or_else(|_| "0".to_string());
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
    }
}

/// App-agnostic OCR cleanup: score lines by length / wordiness / sentence shape, drop obvious
/// chrome (URLs, paths), strip generic "long title — short app" tails, then keep lines within a
/// relative score band so embeddings down-rank UI noise without hardcoding specific apps.
fn clean_ocr_for_semantics(input: &str, config: &IngestionConfig) -> String {
    fn url_regex() -> &'static Regex {
        static RE: OnceLock<Regex> = OnceLock::new();
        RE.get_or_init(|| {
            Regex::new(r"(?i)https?://[^\s]+|www\.[^\s]+").expect("url regex")
        })
    }
    fn host_only_line() -> &'static Regex {
        static RE: OnceLock<Regex> = OnceLock::new();
        RE.get_or_init(|| {
            Regex::new(r"(?i)^[a-z0-9][a-z0-9.-]*\.[a-z]{2,}(/\S*)?$").expect("host line regex")
        })
    }
    fn path_only_line() -> &'static Regex {
        static RE: OnceLock<Regex> = OnceLock::new();
        RE.get_or_init(|| Regex::new(r"^/[A-Za-z0-9._~/-]+$").expect("path-only regex"))
    }

    let without_urls = url_regex().replace_all(input, " ");
    let collapsed = without_urls
        .chars()
        .map(|c| if c.is_control() && c != '\n' { ' ' } else { c })
        .collect::<String>();

    let mut scored: Vec<(String, f32)> = Vec::new();
    for raw_line in collapsed.lines() {
        let line = strip_short_window_style_suffix(raw_line.trim());
        if line.is_empty() {
            continue;
        }
        if should_hard_drop_line(line) {
            continue;
        }
        if host_only_line().is_match(line) {
            continue;
        }
        if path_only_line().is_match(line) {
            continue;
        }
        let score = line_content_score(line);
        if score <= 0.0 {
            continue;
        }
        scored.push((line.to_string(), score));
    }

    if scored.is_empty() {
        return String::new();
    }

    let max_s = scored
        .iter()
        .map(|(_, s)| *s)
        .fold(0.0f32, |a, b| a.max(b))
        .max(1e-6);

    let ratio = config.ocr_line_score_ratio.clamp(0.0, 1.0);
    let threshold = max_s * ratio;

    let mut kept: Vec<String> = Vec::new();
    for (line, score) in &scored {
        if *score >= threshold {
            kept.push(line.clone());
        }
    }

    if kept.is_empty() {
        // Fallback: keep the strongest few lines so we never embed an empty capture.
        let mut order: Vec<usize> = (0..scored.len()).collect();
        order.sort_by(|&a, &b| {
            scored[b]
                .1
                .partial_cmp(&scored[a].1)
                .unwrap_or(Ordering::Equal)
        });
        let take = order.len().min(3);
        let mut pick = order.into_iter().take(take).collect::<Vec<_>>();
        pick.sort();
        for i in pick {
            kept.push(scored[i].0.clone());
        }
    }

    let mut joined = kept.join("\n");
    joined = normalize_text(&joined);

    if config.ocr_emphasize_top && !scored.is_empty() && !joined.is_empty() {
        let best_idx = scored
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.1.partial_cmp(&b.1).unwrap_or(Ordering::Equal))
            .map(|(i, _)| i)
            .unwrap_or(0);
        let best = &scored[best_idx].0;
        if best.len() >= 24 {
            joined.push_str("\n\n");
            joined.push_str(best);
        }
    }

    normalize_text(&joined)
}

/// Many UIs render "Document title — Product" on one OCR line; strip only when the right side looks
/// like a short window/app name (few words, no sentence punctuation), not a subtitle.
fn strip_short_window_style_suffix(line: &str) -> &str {
    for sep in [" — ", " – ", " - "] {
        if let Some(pos) = line.rfind(sep) {
            let left = line[..pos].trim();
            let right = line[pos + sep.len()..].trim();
            let lc = left.chars().count();
            let rc = right.chars().count();
            let rw = right.split_whitespace().count();
            if right.contains('.') || right.contains('?') || right.contains('!') {
                continue;
            }
            if lc >= 20 && rc <= 28 && rw <= 3 && rc <= lc {
                return left;
            }
        }
    }
    line
}

fn should_hard_drop_line(line: &str) -> bool {
    let t = line.trim();
    if t.len() <= 2 {
        return true;
    }
    if t.chars().all(|c| c.is_ascii_digit() || c.is_whitespace()) {
        return true;
    }
    let alnum = t.chars().filter(|c| c.is_alphanumeric()).count();
    if alnum < 2 && !t.chars().any(|c| c.is_alphabetic()) {
        return true;
    }
    false
}

/// Higher score ≈ more likely narrative body; lower ≈ labels, chrome, stray tokens (any app).
fn line_content_score(line: &str) -> f32 {
    let len = line.chars().count();
    let words: Vec<&str> = line.split_whitespace().collect();
    let wc = words.len().max(1);

    let mut s = (len as f32).ln_1p() * (wc as f32).ln_1p();

    if len >= 80 {
        s *= 1.15;
    }
    if line.contains('.') || line.contains('?') || line.contains('!') {
        if len > 25 {
            s *= 1.3;
        }
    }

    let letters = line.chars().filter(|c| c.is_alphabetic()).count();
    let non_alnum = line
        .chars()
        .filter(|c| !c.is_alphanumeric() && !c.is_whitespace())
        .count();
    if letters + non_alnum > 0 {
        let noise = non_alnum as f32 / (letters + non_alnum) as f32;
        if noise > 0.38 {
            s *= 0.2;
        } else if noise > 0.22 {
            s *= 0.55;
        }
    }

    if len < 14 {
        s *= 0.35;
    }
    if wc == 1 && len < 28 {
        s *= 0.3;
    }

    if wc >= 2 && wc <= 5 && len < 48 {
        let caps = words
            .iter()
            .filter(|w| {
                w.chars()
                    .next()
                    .map(|c| c.is_uppercase())
                    .unwrap_or(false)
            })
            .count();
        if caps == wc {
            s *= 0.5;
        }
    }

    s.max(0.0)
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
