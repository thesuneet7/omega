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

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SessionSummaryState {
    pub session_key: String,
    pub title: String,
    pub body: String,
    pub source_bucket_ids: Vec<i64>,
    pub revisions: Vec<SummaryRevision>,
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
