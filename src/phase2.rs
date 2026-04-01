use crate::models::{Phase1Payload, VisualLogItem};
use anyhow::{anyhow, Context, Result};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Deserialize)]
struct SessionLogFile {
    payloads: Vec<Phase1Payload>,
}

#[derive(Debug, Serialize)]
pub struct IngestionSummary {
    pub source_log_path: String,
    pub generated_at_epoch_secs: u64,
    pub items_read: usize,
    pub items_embedded: usize,
    pub embedding_backend: String,
    pub embedding_model: String,
}

#[derive(Debug, Serialize)]
pub struct IngestedVisualItem {
    pub source_visual_id: u64,
    pub timestamp_epoch_secs: u64,
    pub timestamp_nanos: u32,
    pub app_name: String,
    pub window_title: String,
    pub event_type: String,
    pub ocr_engine_used: String,
    pub screen_width: u32,
    pub screen_height: u32,
    pub canonical_text: String,
    pub canonical_text_sha256: String,
    pub embedding: Vec<f32>,
    pub embedding_dim: usize,
}

#[derive(Debug, Serialize)]
pub struct IngestionOutput {
    pub summary: IngestionSummary,
    pub items: Vec<IngestedVisualItem>,
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

    fn as_str(&self) -> &'static str {
        match self {
            Self::Gemini => "gemini",
            Self::OpenAI => "openai",
            Self::Hash => "hash",
        }
    }
}

#[derive(Debug, Clone)]
pub struct IngestionConfig {
    pub input_log: Option<PathBuf>,
    pub output_path: Option<PathBuf>,
    pub backend: EmbeddingBackend,
    pub gemini_base_url: String,
    pub gemini_api_key: Option<String>,
    pub openai_base_url: String,
    pub openai_api_key: Option<String>,
    pub embed_model: String,
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

        Ok(Self {
            input_log,
            output_path,
            backend,
            gemini_base_url,
            gemini_api_key,
            openai_base_url,
            openai_api_key,
            embed_model,
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

pub fn run_ingestion(config: IngestionConfig) -> Result<PathBuf> {
    let source_log = match config.input_log {
        Some(path) => path,
        None => latest_capture_log_path(Path::new("logs"))?,
    };

    let content = fs::read_to_string(&source_log)
        .with_context(|| format!("failed to read session log '{}'", source_log.display()))?;
    let session: SessionLogFile = serde_json::from_str(&content)
        .with_context(|| format!("failed to parse JSON in '{}'", source_log.display()))?;

    let items = build_items(
        &session.payloads,
        &config.backend,
        &config.gemini_base_url,
        config.gemini_api_key.as_deref(),
        &config.openai_base_url,
        config.openai_api_key.as_deref(),
        &config.embed_model,
    )?;

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
            generated_at_epoch_secs: now_epoch,
            items_read: session.payloads.len(),
            items_embedded: items.len(),
            embedding_backend: config.backend.as_str().to_string(),
            embedding_model: config.embed_model,
        },
        items,
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

fn build_items(
    payloads: &[Phase1Payload],
    backend: &EmbeddingBackend,
    gemini_base_url: &str,
    gemini_api_key: Option<&str>,
    openai_base_url: &str,
    openai_api_key: Option<&str>,
    embed_model: &str,
) -> Result<Vec<IngestedVisualItem>> {
    let mut out: Vec<IngestedVisualItem> = Vec::with_capacity(payloads.len());
    let client = Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .context("failed to create HTTP client for embeddings")?;

    for payload in payloads {
        let Phase1Payload::Visual(visual) = payload;
        let canonical_text = to_canonical_text(visual);
        let canonical_text_sha256 = sha256_hex(&canonical_text);
        let embedding = match backend {
            EmbeddingBackend::Gemini => embed_with_gemini(
                &client,
                gemini_base_url,
                gemini_api_key,
                embed_model,
                &canonical_text,
            )?,
            EmbeddingBackend::OpenAI => embed_with_openai(
                &client,
                openai_base_url,
                openai_api_key,
                embed_model,
                &canonical_text,
            )?,
            EmbeddingBackend::Hash => embed_with_hash(&canonical_text, 384),
        };

        let (ts_secs, ts_nanos) = match visual.timestamp.duration_since(UNIX_EPOCH) {
            Ok(dur) => (dur.as_secs(), dur.subsec_nanos()),
            Err(_) => (0, 0),
        };

        out.push(IngestedVisualItem {
            source_visual_id: visual.id,
            timestamp_epoch_secs: ts_secs,
            timestamp_nanos: ts_nanos,
            app_name: visual.app_name.clone(),
            window_title: visual.window_title.clone(),
            event_type: visual.event_type.clone(),
            ocr_engine_used: visual.ocr_engine_used.clone(),
            screen_width: visual.width,
            screen_height: visual.height,
            canonical_text,
            canonical_text_sha256,
            embedding_dim: embedding.len(),
            embedding,
        });
    }

    Ok(out)
}

fn embed_with_gemini(
    client: &Client,
    base_url: &str,
    api_key: Option<&str>,
    model: &str,
    text: &str,
) -> Result<Vec<f32>> {
    let api_key = api_key
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("missing OMEGA_GEMINI_API_KEY (required when backend=gemini)"))?;

    let endpoint = format!(
        "{}/v1beta/models/{}:embedContent",
        base_url.trim_end_matches('/'),
        model
    );
    let body = GeminiEmbeddingRequest {
        content: GeminiContent {
            parts: vec![GeminiPart { text }],
        },
        task_type: "RETRIEVAL_DOCUMENT",
    };

    let response = client
        .post(&endpoint)
        .query(&[("key", api_key)])
        .json(&body)
        .send()
        .with_context(|| {
            format!("failed to connect to Gemini embeddings endpoint at {endpoint}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let response_body = response.text().unwrap_or_default();
        return Err(anyhow!(
            "gemini embedding request failed status={} body={}",
            status,
            response_body
        ));
    }

    let parsed: GeminiEmbeddingResponse = response
        .json()
        .context("failed to parse Gemini embedding response")?;
    if parsed.embedding.values.is_empty() {
        return Err(anyhow!("gemini returned an empty embedding vector"));
    }
    Ok(parsed.embedding.values)
}

fn to_canonical_text(item: &VisualLogItem) -> String {
    let ts = item
        .timestamp
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string());
    let ocr = normalize_text(&item.ocr_text);
    format!(
        "timestamp={ts}\napp_name={}\nwindow_title={}\nevent_type={}\nocr_engine={}\nresolution={}x{}\nocr_text:\n{}",
        normalize_text(&item.app_name),
        normalize_text(&item.window_title),
        normalize_text(&item.event_type),
        normalize_text(&item.ocr_engine_used),
        item.width,
        item.height,
        ocr
    )
}

fn normalize_text(input: &str) -> String {
    input
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn embed_with_openai(
    client: &Client,
    base_url: &str,
    api_key: Option<&str>,
    model: &str,
    text: &str,
) -> Result<Vec<f32>> {
    let api_key = api_key
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("missing OMEGA_OPENAI_API_KEY (required when backend=openai)"))?;

    let endpoint = format!("{}/v1/embeddings", base_url.trim_end_matches('/'));
    let body = OpenAIEmbeddingRequest { model, input: text };

    let response = client
        .post(&endpoint)
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .with_context(|| format!("failed to connect to embeddings endpoint at {endpoint}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let response_body = response.text().unwrap_or_default();
        return Err(anyhow!(
            "embedding request failed status={} body={}",
            status,
            response_body
        ));
    }

    let parsed: OpenAIEmbeddingResponse = response
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
