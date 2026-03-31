use crate::models::{Phase1Payload, VisualLogItem};
use crate::phash::{compute_phash, similarity};
use image::{DynamicImage, RgbaImage};
use std::time::{Duration, SystemTime};

/// Events that can trigger a capture in the sensor layer.
/// For now we only expose a simulated event; later you can add:
/// - WindowFocusChanged
/// - MouseClick
/// - ScrollStopped
#[derive(Debug, Clone)]
pub enum SensorEvent {
    SimulatedActivity,
}

pub struct SensorEngine {
    next_id: u64,
    last_phash: Option<[u8; 8]>,
    phash_similarity_threshold: f32,
    phase2_queue: Vec<Phase1Payload>,
}

impl SensorEngine {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            last_phash: None,
            phash_similarity_threshold: 0.95,
            phase2_queue: Vec::new(),
        }
    }

    fn next_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Entry point from OS hooks / Tauri side.
    pub fn handle_event(&mut self, _event: SensorEvent) {
        // 1) Capture screenshot into RAM (simulated for now).
        let screenshot = self.capture_screenshot_simulated();

        // 2) Compute pHash and check similarity with last frame.
        let phash = compute_phash(&screenshot);

        if let Some(last) = &self.last_phash {
            let sim = similarity(&phash, last);
            if sim >= self.phash_similarity_threshold {
                // Highly similar, drop this frame.
                return;
            }
        }
        self.last_phash = Some(phash);

        // 3) Create visual log item and enqueue for Phase 2.
        let (w, h) = screenshot.dimensions();
        let visual = VisualLogItem {
            id: self.next_id(),
            timestamp: SystemTime::now(),
            app_name: "simulated.app".to_string(),
            window_title: "Simulated Window".to_string(),
            event_type: "SimulatedActivity".to_string(),
            width: w,
            height: h,
        };
        self.phase2_queue.push(Phase1Payload::Visual(visual));

        // Audio is intentionally omitted in this version; only visual payloads are emitted.
    }

    /// Simulated "screenshot into RAM".
    ///
    /// Replace this with a platform-specific implementation that uses native
    /// APIs to grab the frontmost window or display.
    fn capture_screenshot_simulated(&self) -> DynamicImage {
        let img: RgbaImage = RgbaImage::from_fn(800, 600, |x, y| {
            let r = (x % 256) as u8;
            let g = (y % 256) as u8;
            let b = ((x + y) % 256) as u8;
            image::Rgba([r, g, b, 255])
        });
        DynamicImage::ImageRgba8(img)
    }

    /// Drain all Phase 1 payloads intended for Phase 2 ingestion.
    pub fn drain_phase2_payloads(&mut self) -> Vec<Phase1Payload> {
        std::mem::take(&mut self.phase2_queue)
    }
}

