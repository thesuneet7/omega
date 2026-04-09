//! User-configured app names to skip for screen capture (Phase 1).
//! Stored as JSON next to other app logs so the capture process and API share one source of truth.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const MAX_APP_NAMES: usize = 128;
const MAX_NAME_LEN: usize = 256;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CaptureExclusionsConfig {
    #[serde(default)]
    pub excluded_app_names: Vec<String>,
}

pub fn exclusions_path() -> PathBuf {
    PathBuf::from(
        std::env::var("OMEGA_APP_LOGS_DIR").unwrap_or_else(|_| "logs".to_string()),
    )
    .join("capture_exclusions.json")
}

pub fn load_capture_exclusions() -> anyhow::Result<CaptureExclusionsConfig> {
    let path = exclusions_path();
    if !path.exists() {
        return Ok(CaptureExclusionsConfig::default());
    }
    let raw = fs::read_to_string(&path)?;
    let mut parsed: CaptureExclusionsConfig = serde_json::from_str(&raw)?;
    parsed.excluded_app_names = sanitize_app_names(parsed.excluded_app_names);
    Ok(parsed)
}

/// Normalize and dedupe names for storage. Returns an error if the result would be empty but input had junk only.
pub fn sanitize_app_names(names: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::<String>::new();
    let mut out = Vec::new();
    for n in names {
        let t = n.trim();
        if t.is_empty() {
            continue;
        }
        let clipped = if t.len() > MAX_NAME_LEN {
            &t[..MAX_NAME_LEN]
        } else {
            t
        };
        let key = clipped.to_lowercase();
        if seen.insert(key) {
            out.push(clipped.to_string());
        }
        if out.len() >= MAX_APP_NAMES {
            break;
        }
    }
    out.sort();
    out
}

// Called from the HTTP API (`lib`); the `sensor_layer` bin only reads this file.
#[allow(dead_code)]
pub fn save_capture_exclusions(config: &CaptureExclusionsConfig) -> anyhow::Result<()> {
    let path = exclusions_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let to_write = CaptureExclusionsConfig {
        excluded_app_names: sanitize_app_names(config.excluded_app_names.clone()),
    };
    let raw = serde_json::to_string_pretty(&to_write)?;
    fs::write(path, raw)?;
    Ok(())
}

pub fn is_app_excluded(frontmost_app: &str, excluded_lowercase: &[String]) -> bool {
    let n = frontmost_app.trim().to_lowercase();
    if n.is_empty() {
        return false;
    }
    excluded_lowercase.iter().any(|e| n == *e)
}
