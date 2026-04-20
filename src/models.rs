use serde::{Deserialize, Serialize};
use std::time::SystemTime;

/// Structured context extracted when the user is working in a code editor.
/// Populated via window-title parsing + git CLI, or pushed by the Omega IDE extension.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IdeContext {
    /// Active file name, e.g. `"payment.ts"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_file: Option<String>,
    /// Workspace / project name as shown in the editor title bar.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    /// Language inferred from the file extension, e.g. `"TypeScript"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// Current git branch at the time of capture.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
    /// Absolute path to the workspace root used to run git commands.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_path: Option<String>,
}

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
    /// Populated when the frontmost app is a recognised code editor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ide_context: Option<IdeContext>,
}

/// Unified payload that Phase 1 emits for Phase 2 ingestion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Phase1Payload {
    Visual(VisualLogItem),
}
