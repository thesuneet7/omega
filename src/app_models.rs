use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SessionListItem {
    pub session_key: String,
    pub file_path: String,
    pub started_at_epoch_secs: u64,
    pub ended_at_epoch_secs: u64,
    pub duration_secs: u64,
    pub accepted_captures: u64,
    pub total_events_seen: u64,
    /// Current summary title from the app database, if the session has been opened or saved.
    pub summary_title: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SummaryRevision {
    pub id: i64,
    pub summary_id: i64,
    pub title: String,
    pub body: String,
    pub edited_at_epoch_secs: u64,
    pub editor_label: String,
}

/// One distinct (app, window) pair observed in captures that fed a bucket summary.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct SourceAttribution {
    pub app_name: String,
    pub window_title: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SessionBucket {
    pub bucket_id: i64,
    pub title: String,
    pub body: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_attribution: Vec<SourceAttribution>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SessionSummaryState {
    pub session_key: String,
    pub title: String,
    pub body: String,
    pub source_bucket_ids: Vec<i64>,
    pub revisions: Vec<SummaryRevision>,
    #[serde(default)]
    pub buckets: Vec<SessionBucket>,
}

/// App names (exact match, case-insensitive) for which Phase 1 must not capture screenshots.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CaptureExclusionsState {
    #[serde(rename = "excludedAppNames")]
    pub excluded_app_names: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StorageManifestEntry {
    pub category: String,
    pub path: String,
    pub absolute_path: String,
    pub bytes: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StorageManifest {
    pub logs_root_absolute: String,
    pub retention_note: String,
    pub entries: Vec<StorageManifestEntry>,
    pub total_bytes: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DeleteLocalDataResponse {
    pub ok: bool,
    pub restart_recommended: bool,
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PipelineRunRecord {
    pub id: i64,
    pub stage: String,
    pub input_ref: String,
    pub status: String,
    pub started_at_epoch_secs: u64,
    pub ended_at_epoch_secs: Option<u64>,
    pub error_text: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ActionOutputRecord {
    pub id: i64,
    pub session_key: String,
    pub action_type: String,
    pub input_bucket_ids: Vec<i64>,
    pub output_body: String,
    pub model: String,
    pub generated_at_epoch_secs: u64,
}
