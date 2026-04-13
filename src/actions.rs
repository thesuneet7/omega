//! Phase 5 — "Action Layer": parameterized LLM transforms that take bucket summaries
//! as structured input and produce specific output formats (reports, PRDs, emails, etc.).

use anyhow::{anyhow, Context, Result};
use reqwest::blocking::Response;
use serde::{Deserialize, Serialize};
use std::thread::sleep;
use std::time::Duration;

/// All supported action types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionType {
    Report,
    Prd,
    Email,
    Timeline,
}

impl ActionType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Report => "report",
            Self::Prd => "prd",
            Self::Email => "email",
            Self::Timeline => "timeline",
        }
    }

    pub fn from_str(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "report" => Ok(Self::Report),
            "prd" => Ok(Self::Prd),
            "email" => Ok(Self::Email),
            "timeline" => Ok(Self::Timeline),
            other => Err(anyhow!("unknown action type '{other}'")),
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Report => "Report",
            Self::Prd => "PRD",
            Self::Email => "Email draft",
            Self::Timeline => "Timeline",
        }
    }
}

/// Minimal bucket summary info passed as input to an action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionBucketInput {
    pub bucket_id: i64,
    pub title: String,
    pub one_liner: String,
    pub detailed_summary: String,
    pub tags: Vec<String>,
    pub primary_apps: Vec<String>,
    pub source_attribution: Vec<ActionSourceRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionSourceRef {
    pub app_name: String,
    pub window_title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionOutput {
    pub session_key: String,
    pub action_type: String,
    pub input_bucket_ids: Vec<i64>,
    pub output_body: String,
    pub model: String,
    pub generated_at_epoch_secs: u64,
}

fn system_prompt_for(action: ActionType) -> &'static str {
    match action {
        ActionType::Report => r#"You produce a well-structured professional report from activity session data. Write in second person ("you"). Use markdown formatting with clear headings, bullet points, and sections. The report should have:
- An executive summary (2-3 sentences)
- Key activities section with details
- Key findings / takeaways
- Recommended next steps (if inferable from context)
Be factual: only include information supported by the bucket summaries. Do not invent URLs, names, or data."#,

        ActionType::Prd => r#"You produce a Product Requirements Document (PRD) from activity session data. Infer the product/feature context from what was researched and worked on. Use markdown with clear sections:
- **Overview**: What product/feature this relates to (1-2 sentences)
- **Problem Statement**: What problem is being solved (inferred from activity)
- **Goals & Success Metrics**: Measurable objectives
- **Requirements**: Numbered list of functional requirements
- **Non-functional Requirements**: Performance, security, scalability considerations
- **Open Questions**: Unknowns or areas needing further research
Be factual: only include information supported by the bucket summaries. Mark inferred items clearly."#,

        ActionType::Email => r#"You draft a concise professional email summarizing the session activity. Write as if the user is sending an update to a colleague or stakeholder. Use a clear subject line and structured body:
- Subject: [descriptive subject]
- Brief context (1-2 sentences)
- Key points (bulleted)
- Next steps or ask (if applicable)
Keep it under 300 words. Be factual: only include information from the bucket summaries."#,

        ActionType::Timeline => r#"You produce a chronological timeline of activities from session data. Use markdown with a clear timeline format:
- Each entry should have a time indicator and description
- Group closely related activities
- Highlight key transitions between different tasks/contexts
- Add brief context for each activity
Be factual: only include information from the bucket summaries."#,
    }
}

fn build_action_prompt(buckets: &[ActionBucketInput], action: ActionType) -> String {
    let mut parts = Vec::new();
    for b in buckets {
        let apps = b.primary_apps.join(", ");
        let tags = b.tags.join(", ");
        let sources: Vec<String> = b
            .source_attribution
            .iter()
            .map(|s| {
                if s.window_title.is_empty() {
                    s.app_name.clone()
                } else {
                    format!("{} — {}", s.app_name, s.window_title)
                }
            })
            .collect();
        let sources_str = if sources.is_empty() {
            "none".to_string()
        } else {
            sources.join("; ")
        };

        parts.push(format!(
            "### Bucket {}: {}\n\
             **Summary**: {}\n\
             **Details**: {}\n\
             **Apps**: {}\n\
             **Tags**: {}\n\
             **Sources**: {}",
            b.bucket_id,
            b.title,
            b.one_liner,
            b.detailed_summary,
            apps,
            tags,
            sources_str,
        ));
    }

    let bucket_count = buckets.len();
    format!(
        "Generate a {} from the following {bucket_count} activity bucket(s).\n\n\
         ---\n{}\n---\n\n\
         Output the result in markdown. Do not wrap in code fences.",
        action.label().to_lowercase(),
        parts.join("\n\n"),
    )
}

pub fn run_action(
    session_key: &str,
    action: ActionType,
    buckets: &[ActionBucketInput],
    bucket_ids: &[i64],
) -> Result<ActionOutput> {
    let api_key = std::env::var("OMEGA_GEMINI_API_KEY")
        .map_err(|_| anyhow!("OMEGA_GEMINI_API_KEY not set"))?;
    let base_url = std::env::var("OMEGA_GEMINI_BASE_URL")
        .unwrap_or_else(|_| "https://generativelanguage.googleapis.com".to_string());
    let model = std::env::var("OMEGA_PHASE4_MODEL")
        .unwrap_or_else(|_| "gemini-2.5-flash-lite".to_string());

    let system = system_prompt_for(action);
    let user_prompt = build_action_prompt(buckets, action);
    let full_text = format!("{system}\n\n{user_prompt}");

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(90))
        .build()
        .context("build http client")?;

    let endpoint = format!(
        "{}/v1beta/models/{}:generateContent",
        base_url.trim_end_matches('/'),
        model
    );

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
        temperature: f32,
        #[serde(rename = "maxOutputTokens")]
        max_output_tokens: u32,
    }

    let body = GenBody {
        contents: vec![GenContent {
            role: "user",
            parts: vec![GenPart { text: &full_text }],
        }],
        generation_config: GenConfig {
            temperature: 0.3,
            max_output_tokens: 8192,
        },
    };

    let output_body = with_retries(2, 1000, || {
        let response = client
            .post(&endpoint)
            .query(&[("key", &api_key)])
            .json(&body)
            .send()
            .context("action generateContent request failed")?;

        parse_gemini_text(response)
    })?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    Ok(ActionOutput {
        session_key: session_key.to_string(),
        action_type: action.as_str().to_string(),
        input_bucket_ids: bucket_ids.to_vec(),
        output_body,
        model,
        generated_at_epoch_secs: now,
    })
}

#[derive(Deserialize)]
struct GeminiGenerateResponse {
    candidates: Option<Vec<GeminiCandidate>>,
}
#[derive(Deserialize)]
struct GeminiCandidate {
    content: Option<GeminiContent>,
}
#[derive(Deserialize)]
struct GeminiContent {
    parts: Option<Vec<GeminiPart>>,
}
#[derive(Deserialize)]
struct GeminiPart {
    text: Option<String>,
}

fn parse_gemini_text(response: Response) -> Result<String> {
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().unwrap_or_default();
        let retryable = status.as_u16() == 429 || status.is_server_error();
        let prefix = if retryable { "[retryable] " } else { "" };
        return Err(anyhow!(
            "{prefix}action request failed status={status} body={body}"
        ));
    }
    let gen: GeminiGenerateResponse = response.json().context("parse Gemini response")?;
    gen.candidates
        .and_then(|c| c.into_iter().next())
        .and_then(|c| c.content)
        .and_then(|c| c.parts)
        .and_then(|p| p.into_iter().next())
        .and_then(|p| p.text)
        .ok_or_else(|| anyhow!("Gemini response missing candidate text"))
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
