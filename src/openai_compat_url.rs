//! Normalize OpenAI/xAI-style API roots so `/v1/...` paths are never doubled or omitted.

/// Ensures the URL ends with `/v1` (no trailing slash).
pub fn ensure_openai_v1_base(raw: &str) -> String {
    let t = raw.trim().trim_end_matches('/');
    if t.ends_with("/v1") {
        t.to_string()
    } else {
        format!("{}/v1", t)
    }
}

pub fn openai_embeddings_url(raw_base: &str) -> String {
    format!("{}/embeddings", ensure_openai_v1_base(raw_base))
}
