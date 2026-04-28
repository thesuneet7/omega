//! Phase 5 — "Action Layer": parameterized LLM transforms that take bucket summaries
//! as structured input and produce specific output formats (reports, PRDs, emails, etc.).

use anyhow::{anyhow, Context, Result};
use reqwest::blocking::Response;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::thread::sleep;
use std::time::Duration;

/// All supported action types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionType {
    StakeholderBrief,
    Report,
    Prd,
    Email,
    Timeline,
    Custom,
}

impl ActionType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::StakeholderBrief => "stakeholder_brief",
            Self::Report => "report",
            Self::Prd => "prd",
            Self::Email => "email",
            Self::Timeline => "timeline",
            Self::Custom => "custom",
        }
    }

    pub fn from_str(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "stakeholder_brief" | "stakeholder-brief" | "brief" => Ok(Self::StakeholderBrief),
            "report" => Ok(Self::Report),
            "prd" => Ok(Self::Prd),
            "email" => Ok(Self::Email),
            "timeline" => Ok(Self::Timeline),
            "custom" => Ok(Self::Custom),
            other => Err(anyhow!("unknown action type '{other}'")),
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::StakeholderBrief => "Stakeholder brief",
            Self::Report => "Report",
            Self::Prd => "PRD",
            Self::Email => "Email draft",
            Self::Timeline => "Timeline",
            Self::Custom => "Custom",
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
    pub origin: String,
    pub timestamp_epoch_secs: Option<i64>,
    pub snippet: String,
    pub confidence: String,
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
        ActionType::StakeholderBrief => r#"You produce a send-ready stakeholder brief with strict trust controls.
Always output these sections in this exact order:
## What changed
## Decisions and rationale
## Progress vs plan
## Risks/blockers
## Next steps and asks

Rules:
- Every non-trivial claim must include at least one citation marker like [E1], [E2].
- If a claim has no citation, explicitly label it as Assumption.
- Prefix each claim line with a confidence label: [High], [Medium], or [Low].
- Keep claims concise and verifiable.
- Never invent URLs, ticket IDs, or source details."#,
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

        ActionType::Custom => r#"You are a helpful assistant that processes activity session data according to the user's instructions. Use markdown formatting in your output. Be factual: only include information supported by the bucket summaries. Do not invent URLs, names, or data."#,
    }
}

fn build_bucket_block(buckets: &[ActionBucketInput]) -> String {
    let mut parts = Vec::new();
    for b in buckets {
        let apps = b.primary_apps.join(", ");
        let tags = b.tags.join(", ");
        let sources: Vec<String> = b
            .source_attribution
            .iter()
            .map(|s| {
                let window = if s.window_title.is_empty() {
                    String::new()
                } else {
                    format!(" — {}", s.window_title)
                };
                let origin = if s.origin.is_empty() {
                    "unknown-origin".to_string()
                } else {
                    s.origin.clone()
                };
                let snippet = if s.snippet.is_empty() {
                    "n/a".to_string()
                } else {
                    s.snippet.clone()
                };
                let confidence = if s.confidence.is_empty() {
                    "Medium".to_string()
                } else {
                    s.confidence.clone()
                };
                format!(
                    "{}{} | origin={} | confidence={} | snippet={}",
                    s.app_name, window, origin, confidence, snippet
                )
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
    parts.join("\n\n")
}

fn build_action_prompt(buckets: &[ActionBucketInput], action: ActionType, custom_prompt: Option<&str>) -> String {
    let bucket_block = build_bucket_block(buckets);
    let bucket_count = buckets.len();
    if action == ActionType::StakeholderBrief {
        let window_note = custom_prompt.unwrap_or("Use full selected session window.");
        return format!(
            "Create a stakeholder brief from the following {bucket_count} activity bucket(s).\n\
             Time window preference: {window_note}\n\n\
             ---\n{bucket_block}\n---\n\n\
             Output the result in markdown. Do not wrap in code fences.",
        );
    }

    if action == ActionType::Custom {
        let user_instruction = custom_prompt.unwrap_or("Summarize the following session data.");
        return format!(
            "User instruction: {user_instruction}\n\n\
             Here are the {bucket_count} activity bucket(s) to work with:\n\n\
             ---\n{bucket_block}\n---\n\n\
             Output the result in markdown. Do not wrap in code fences.",
        );
    }

    format!(
        "Generate a {} from the following {bucket_count} activity bucket(s).\n\n\
         ---\n{bucket_block}\n---\n\n\
         Output the result in markdown. Do not wrap in code fences.",
        action.label().to_lowercase(),
    )
}

fn evidence_regex() -> Regex {
    Regex::new(r"\[E\d+\]").expect("compile evidence regex")
}

fn confidence_regex() -> Regex {
    Regex::new(r"^\s*(?:[-*]\s+)?\[(High|Medium|Low)\]\s+").expect("compile confidence regex")
}

fn whitespace_regex() -> Regex {
    Regex::new(r"\s+").expect("compile whitespace regex")
}

fn is_structural_line(t: &str) -> bool {
    t.is_empty()
        || t.starts_with('#')
        || t.starts_with("---")
        || t == "Evidence Appendix"
        || t == "## Evidence Appendix"
}

fn should_treat_as_claim(t: &str) -> bool {
    if is_structural_line(t) {
        return false;
    }
    let raw = t.trim_start_matches(['-', '*', ' ']).trim();
    raw.len() >= 18
}

fn normalize_confidence(s: &str) -> &'static str {
    let t = s.trim().to_ascii_lowercase();
    if t == "high" {
        "High"
    } else if t == "low" {
        "Low"
    } else {
        "Medium"
    }
}

fn normalize_text_fragment(s: &str, max_len: usize) -> String {
    let compact = whitespace_regex().replace_all(s.trim(), " ").to_string();
    if compact.chars().count() <= max_len {
        compact
    } else {
        compact.chars().take(max_len).collect::<String>()
    }
}

fn extract_ref_ids(line: &str) -> Vec<usize> {
    evidence_regex()
        .find_iter(line)
        .filter_map(|m| {
            m.as_str()
                .trim_start_matches("[E")
                .trim_end_matches(']')
                .parse::<usize>()
                .ok()
        })
        .collect()
}

fn remap_ref_ids(raw_ids: &[usize], max_id: usize) -> Vec<usize> {
    if max_id == 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for id in raw_ids {
        let normalized = if *id == 0 {
            1
        } else if *id > max_id {
            ((*id - 1) % max_id) + 1
        } else {
            *id
        };
        if seen.insert(normalized) {
            out.push(normalized);
        }
        if out.len() >= 3 {
            break;
        }
    }
    out
}

fn confidence_for_refs(ref_ids: &[usize], by_id: &HashMap<usize, ActionSourceRef>) -> &'static str {
    if ref_ids.iter().any(|id| {
        by_id.get(id)
            .map(|r| r.confidence.eq_ignore_ascii_case("high"))
            .unwrap_or(false)
    }) {
        "High"
    } else if ref_ids.iter().any(|id| {
        by_id.get(id)
            .map(|r| r.confidence.eq_ignore_ascii_case("low"))
            .unwrap_or(false)
    }) {
        "Low"
    } else {
        "Medium"
    }
}

fn dedupe_sources(buckets: &[ActionBucketInput]) -> Vec<ActionSourceRef> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<ActionSourceRef> = Vec::new();
    for b in buckets {
        for s in &b.source_attribution {
            let app_name = normalize_text_fragment(&s.app_name, 80);
            let window_title = normalize_text_fragment(&s.window_title, 120);
            let origin = normalize_text_fragment(&s.origin, 180);
            let snippet = normalize_text_fragment(&s.snippet, 260);
            if app_name.is_empty() || snippet.len() < 12 {
                continue;
            }
            let key = format!(
                "{}|{}|{}|{}",
                app_name.to_lowercase(),
                origin.to_lowercase(),
                s.timestamp_epoch_secs.unwrap_or_default(),
                snippet.to_lowercase()
            );
            if seen.insert(key) {
                out.push(ActionSourceRef {
                    app_name,
                    window_title,
                    origin,
                    timestamp_epoch_secs: s.timestamp_epoch_secs,
                    snippet,
                    confidence: normalize_confidence(&s.confidence).to_string(),
                });
            }
        }
    }
    out.sort_by_key(|s| s.timestamp_epoch_secs.unwrap_or(i64::MAX));
    out
}

fn append_evidence_appendix(markdown: &str, evidence: &[ActionSourceRef]) -> String {
    let base = if let Some((head, _)) = markdown.split_once("\n## Evidence Appendix\n") {
        head.trim_end().to_string()
    } else {
        markdown.trim_end().to_string()
    };
    let mut out = String::new();
    out.push_str(&base);
    out.push_str("\n\n## Evidence Appendix\n");
    if evidence.is_empty() {
        out.push_str("\n- [E1] source app: Unknown\n  - origin: unknown-origin\n  - timestamp: unknown\n  - snippet: No source metadata available\n  - confidence: Low\n");
        return out;
    }
    for (idx, ev) in evidence.iter().enumerate() {
        let label = idx + 1;
        let ts = ev
            .timestamp_epoch_secs
            .map(|v| v.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let origin = if ev.origin.is_empty() {
            "unknown-origin"
        } else {
            ev.origin.as_str()
        };
        let source_label = if ev.window_title.is_empty() {
            ev.app_name.clone()
        } else {
            format!("{} — {}", ev.app_name, ev.window_title)
        };
        let snippet = if ev.snippet.is_empty() {
            "n/a"
        } else {
            ev.snippet.as_str()
        };
        let confidence = normalize_confidence(&ev.confidence);
        out.push_str(&format!(
            "\n- [E{label}] source app: {}\n  - origin: {origin}\n  - timestamp: {ts}\n  - snippet: {snippet}\n  - confidence: {confidence}\n",
            source_label
        ));
    }
    out
}

fn enforce_evidence_lock(markdown: &str, buckets: &[ActionBucketInput]) -> String {
    let refs = dedupe_sources(buckets);
    let evidence_re = evidence_regex();
    let confidence_re = confidence_regex();
    let by_id: HashMap<usize, ActionSourceRef> = refs
        .iter()
        .enumerate()
        .map(|(i, r)| (i + 1, r.clone()))
        .collect();
    let default_ref = 1usize;
    let mut next_ref: usize = 1;
    let max_ref = refs.len().max(1);
    let mut claim_count: usize = 0;
    let mut claims_with_refs: usize = 0;
    let mut injected_refs: usize = 0;
    let mut remapped_refs: usize = 0;

    let mut lines_out: Vec<String> = Vec::new();
    for line in markdown.lines() {
        let trimmed = line.trim();
        if is_structural_line(trimmed) {
            lines_out.push(line.to_string());
            continue;
        }
        if !should_treat_as_claim(trimmed) {
            lines_out.push(line.to_string());
            continue;
        }
        claim_count += 1;
        let original_ids = extract_ref_ids(line);
        let ids = remap_ref_ids(&original_ids, max_ref);
        if !original_ids.is_empty() && ids != original_ids {
            remapped_refs += 1;
        }

        let mut updated = line.to_string();
        if ids.is_empty() {
            let ref_id = if refs.is_empty() {
                default_ref
            } else {
                let id = next_ref.min(refs.len());
                next_ref += 1;
                id
            };
            updated = format!("{updated} [E{ref_id}] (Assumption)");
            injected_refs += 1;
        } else {
            claims_with_refs += 1;
            let canonical = ids
                .iter()
                .map(|id| format!("[E{id}]"))
                .collect::<Vec<_>>()
                .join(" ");
            updated = evidence_re.replace_all(&updated, "").to_string();
            updated = format!("{} {}", updated.trim_end(), canonical).trim().to_string();
        }
        if !confidence_re.is_match(&updated) {
            let ids2 = extract_ref_ids(&updated);
            let conf = confidence_for_refs(&ids2, &by_id);
            let prefix = if updated.trim_start().starts_with("- ") {
                "- "
            } else {
                ""
            };
            let content = updated.trim_start_matches("- ").trim_start();
            updated = format!("{prefix}[{conf}] {content}");
        }
        lines_out.push(updated);
    }

    let coverage_pct = if claim_count == 0 {
        100.0
    } else {
        (claims_with_refs as f64 / claim_count as f64) * 100.0
    };
    let mut with_integrity = lines_out.join("\n");
    with_integrity.push_str(&format!(
        "\n\n## Citation Integrity\n- Claims scanned: {claim_count}\n- Claims with original citations: {claims_with_refs}\n- Citation coverage before repair: {:.1}%\n- Citations injected: {injected_refs}\n- Citations remapped: {remapped_refs}\n",
        coverage_pct
    ));
    append_evidence_appendix(&with_integrity, &refs)
}

pub fn run_action(
    session_key: &str,
    action: ActionType,
    buckets: &[ActionBucketInput],
    bucket_ids: &[i64],
    custom_prompt: Option<&str>,
) -> Result<ActionOutput> {
    let api_key = std::env::var("OMEGA_GEMINI_API_KEY")
        .map_err(|_| anyhow!("OMEGA_GEMINI_API_KEY not set"))?;
    let base_url = std::env::var("OMEGA_GEMINI_BASE_URL")
        .unwrap_or_else(|_| "https://generativelanguage.googleapis.com".to_string());
    let model = std::env::var("OMEGA_PHASE4_MODEL")
        .unwrap_or_else(|_| "gemini-2.5-flash-lite".to_string());

    let system = system_prompt_for(action);
    let user_prompt = build_action_prompt(buckets, action, custom_prompt);
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

    let output_body = if action != ActionType::Custom {
        enforce_evidence_lock(&output_body, buckets)
    } else {
        output_body
    };

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
