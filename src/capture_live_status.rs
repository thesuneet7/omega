//! Live Phase 1 UI/sensor state: exclusion blocks this session and optional privacy pause. Reset on session start/end.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Phase1LiveStatus {
    /// Distinct app names (as reported by the OS) that caused a block, first-seen order.
    #[serde(default)]
    pub blocked_app_names: Vec<String>,
    /// User toggled privacy pause: sensor skips captures while true.
    #[serde(default)]
    pub capture_paused: bool,
}

pub fn status_path() -> PathBuf {
    PathBuf::from(
        std::env::var("OMEGA_APP_LOGS_DIR").unwrap_or_else(|_| "logs".to_string()),
    )
    .join("phase1_live_status.json")
}

/// Current `{ "blockedAppNames", "capturePaused" }` JSON, or legacy `{ "lastBlockedAppName" }` / snake_case variants.
pub fn read_or_default() -> anyhow::Result<Phase1LiveStatus> {
    let path = status_path();
    if !path.exists() {
        return Ok(Phase1LiveStatus::default());
    }
    let raw = fs::read_to_string(&path)?;
    if let Ok(current) = serde_json::from_str::<Phase1LiveStatus>(&raw) {
        return Ok(current);
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct LegacyFile {
        #[serde(default)]
        blocked_app_names: Vec<String>,
        #[serde(default)]
        last_blocked_app_name: Option<String>,
    }
    if let Ok(legacy) = serde_json::from_str::<LegacyFile>(&raw) {
        if !legacy.blocked_app_names.is_empty() {
            return Ok(Phase1LiveStatus {
                blocked_app_names: legacy.blocked_app_names,
                ..Default::default()
            });
        }
        if let Some(one) = legacy.last_blocked_app_name {
            let t = one.trim();
            if !t.is_empty() {
                return Ok(Phase1LiveStatus {
                    blocked_app_names: vec![t.to_string()],
                    ..Default::default()
                });
            }
        }
    }
    Ok(Phase1LiveStatus::default())
}

fn write_atomic(status: &Phase1LiveStatus) -> anyhow::Result<()> {
    let path = status_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(status)?;
    let mut tmp = path.clone();
    tmp.set_extension("json.tmp");
    let mut file = fs::File::create(&tmp)?;
    file.write_all(json.as_bytes())?;
    file.sync_all()?;
    drop(file);
    fs::rename(&tmp, &path)?;
    Ok(())
}

// Used from omega-api via `app_commands`; unused in the `sensor_layer` capture binary.
#[allow(dead_code)]
pub fn reset() -> anyhow::Result<()> {
    write_atomic(&Phase1LiveStatus::default())
}

// Used from the `omega-api` binary via `app_commands`; not referenced by the `sensor_layer` capture bin.
#[allow(dead_code)]
pub fn set_capture_paused(paused: bool) -> anyhow::Result<()> {
    let mut s = read_or_default()?;
    s.capture_paused = paused;
    write_atomic(&s)
}

/// Best-effort: never panics the sensor loop. Appends the name once per distinct app (case-insensitive).
pub fn record_exclusion_block(app_name: &str) {
    let mut s = read_or_default().unwrap_or_default();
    let trimmed = app_name.trim();
    if trimmed.is_empty() {
        return;
    }
    let key = trimmed.to_lowercase();
    let exists = s
        .blocked_app_names
        .iter()
        .any(|n| n.to_lowercase() == key);
    if !exists {
        s.blocked_app_names.push(trimmed.to_string());
    }
    if let Err(e) = write_atomic(&s) {
        eprintln!("omega: failed to write phase1 live status: {e}");
    }
}
