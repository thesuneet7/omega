use serde::{Deserialize, Serialize};
use std::time::SystemTime;

/// Visual log item created when the pHash gatekeeper admits a new screenshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisualLogItem {
    pub id: u64,
    pub timestamp: SystemTime,
    pub app_name: String,
    pub window_title: String,
    pub event_type: String,
    /// In a real app this would be a handle to the in-RAM image buffer.
    /// For now we just store dimensions to keep the model lightweight.
    pub width: u32,
    pub height: u32,
    pub ocr_engine_used: String,
    pub ocr_text: String,
}

/// Unified payload that Phase 1 emits for Phase 2 ingestion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Phase1Payload {
    Visual(VisualLogItem),
}
