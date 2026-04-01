use crate::models::{Phase1Payload, VisualLogItem};
use crate::phash::{compute_phash, similarity};
use image::{DynamicImage, GenericImageView, ImageBuffer, Rgba};
use screenshots::Screen;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant, SystemTime};

/// Events that can trigger a capture in the sensor layer.
#[derive(Debug, Clone)]
pub enum SensorEvent {
    MouseClick,
    KeyPress,
    Scroll,
}

pub struct SensorEngine {
    next_id: u64,
    last_phash: Option<[u8; 8]>,
    phash_similarity_threshold: f32,
    phase2_queue: Vec<Phase1Payload>,
    total_events_seen: u64,
    accepted_captures: u64,
    dropped_by_phash: u64,
    dropped_by_throttle: u64,
    last_capture_instant: Option<Instant>,
    capture_cooldown: Duration,
    scroll_pending: bool,
    last_scroll_instant: Option<Instant>,
    scroll_idle_delay: Duration,
    #[cfg(target_os = "macos")]
    vision_helper_binary: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy)]
pub struct SensorStats {
    pub total_events_seen: u64,
    pub accepted_captures: u64,
    pub dropped_by_phash: u64,
    pub dropped_by_throttle: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CaptureAttempt {
    Accepted,
    DroppedPhash,
    DroppedThrottle,
    Failed,
}

impl SensorEngine {
    pub fn new() -> Self {
        #[cfg(target_os = "macos")]
        let vision_helper_binary = Self::prepare_vision_helper_binary();

        Self {
            next_id: 1,
            last_phash: None,
            phash_similarity_threshold: 0.95,
            phase2_queue: Vec::new(),
            total_events_seen: 0,
            accepted_captures: 0,
            dropped_by_phash: 0,
            dropped_by_throttle: 0,
            last_capture_instant: None,
            capture_cooldown: Duration::from_millis(500),
            scroll_pending: false,
            last_scroll_instant: None,
            scroll_idle_delay: Duration::from_millis(1000),
            #[cfg(target_os = "macos")]
            vision_helper_binary,
        }
    }

    fn next_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Entry point from OS hooks / global event listener.
    pub fn handle_event(&mut self, event: SensorEvent) {
        self.total_events_seen += 1;

        // Scroll is debounced: capture only once the scroll has gone idle.
        match event {
            SensorEvent::Scroll => {
                self.scroll_pending = true;
                self.last_scroll_instant = Some(Instant::now());
            }
            SensorEvent::MouseClick => {
                // Any non-scroll event cancels the "scroll stopped" pending window
                // (because user intent has changed).
                self.scroll_pending = false;
                self.last_scroll_instant = None;
                let _ = self.try_capture("MouseClick");
            }
            SensorEvent::KeyPress => {
                // Any non-scroll event cancels the "scroll stopped" pending window
                // (because user intent has changed).
                self.scroll_pending = false;
                self.last_scroll_instant = None;
                let _ = self.try_capture("KeyPress");
            }
        }
    }

    /// Called periodically (e.g. from a `recv_timeout` loop) so "ScrollStopped"
    /// can fire after the scroll goes idle.
    pub fn tick(&mut self) {
        if !self.scroll_pending {
            return;
        }

        let Some(last_scroll) = self.last_scroll_instant else {
            return;
        };

        if last_scroll.elapsed() < self.scroll_idle_delay {
            return;
        }

        // Try to capture once. If throttled, keep pending so it will be retried.
        let attempt = self.try_capture("ScrollStopped");
        if attempt == CaptureAttempt::Accepted || attempt == CaptureAttempt::DroppedPhash {
            self.scroll_pending = false;
            self.last_scroll_instant = None;
        }
    }

    /// On shutdown, attempt to flush any pending "ScrollStopped" capture if idle.
    pub fn flush_pending_scroll(&mut self) {
        if !self.scroll_pending {
            return;
        }
        self.tick();
    }

    /// Capture the primary screen into RAM as a DynamicImage.
    fn capture_screenshot(&self) -> anyhow::Result<DynamicImage> {
        let screens = Screen::all()?;
        let screen = screens
            .get(0)
            .ok_or_else(|| anyhow::anyhow!("no screens available"))?;
        let image = screen.capture()?;

        let (w, h) = (image.width(), image.height());
        let buffer = image.to_vec(); // BGRA

        let mut img_buf: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let idx = ((y * w + x) * 4) as usize;
                let b = buffer[idx];
                let g = buffer[idx + 1];
                let r = buffer[idx + 2];
                let a = buffer[idx + 3];
                img_buf.put_pixel(x, y, Rgba([r, g, b, a]));
            }
        }

        Ok(DynamicImage::ImageRgba8(img_buf))
    }

    fn try_capture(&mut self, event_type: &str) -> CaptureAttempt {
        // Coalesce/throttle expensive capture work.
        let now = Instant::now();
        if let Some(last) = self.last_capture_instant {
            if last.elapsed() < self.capture_cooldown {
                self.dropped_by_throttle += 1;
                return CaptureAttempt::DroppedThrottle;
            }
        }
        self.last_capture_instant = Some(now);

        let screenshot = match self.capture_screenshot() {
            Ok(img) => img,
            Err(e) => {
                eprintln!("failed to capture screenshot: {e}");
                return CaptureAttempt::Failed;
            }
        };

        let phash = compute_phash(&screenshot);

        if let Some(last) = &self.last_phash {
            let sim = similarity(&phash, last);
            if sim >= self.phash_similarity_threshold {
                self.dropped_by_phash += 1;
                return CaptureAttempt::DroppedPhash;
            }
        }
        self.last_phash = Some(phash);

        let (app_name, window_title) = self.get_active_app_and_window();

        let (w, h) = screenshot.dimensions();
        let (ocr_engine_used, ocr_text) = self.extract_ocr_text_with_engine(&screenshot);
        let visual = VisualLogItem {
            id: self.next_id(),
            timestamp: SystemTime::now(),
            app_name,
            window_title,
            event_type: event_type.to_string(),
            width: w,
            height: h,
            ocr_engine_used,
            ocr_text,
        };

        self.phase2_queue.push(Phase1Payload::Visual(visual));
        self.accepted_captures += 1;
        CaptureAttempt::Accepted
    }

    fn get_active_app_and_window(&self) -> (String, String) {
        #[cfg(target_os = "macos")]
        {
            let script = r#"
            tell application "System Events"
              set frontApp to first application process whose frontmost is true
              set appName to name of frontApp
              set winTitle to ""
              try
                set winTitle to name of front window of frontApp
              end try
              return appName & "|" & winTitle
            end tell
            "#;

            let out = Command::new("osascript").arg("-e").arg(script).output();
            if let Ok(out) = out {
                if out.status.success() {
                    let s = String::from_utf8_lossy(&out.stdout);
                    let mut parts = s.trim().splitn(2, '|');
                    let app = parts.next().unwrap_or("").trim();
                    let title = parts.next().unwrap_or("").trim();
                    if !app.is_empty() {
                        let app_name = app.to_string();
                        let window_title = if !title.is_empty() {
                            title.to_string()
                        } else {
                            "Unknown Window".to_string()
                        };
                        return (app_name, window_title);
                    }
                }
            }
        }

        ("unknown.app".to_string(), "Unknown Window".to_string())
    }

    fn extract_ocr_text_with_engine(&self, screenshot: &DynamicImage) -> (String, String) {
        let mut temp_path: PathBuf = std::env::temp_dir();
        let ts = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        temp_path.push(format!("sensor_layer_ocr_{ts}.png"));

        if let Err(err) = screenshot.save(&temp_path) {
            return (
                "none".to_string(),
                format!("[ocr_error] failed_to_write_temp_image: {err}"),
            );
        }

        #[cfg(target_os = "macos")]
        {
            if let Ok(text) = self.run_macos_vision_ocr_fast(&temp_path) {
                let _ = fs::remove_file(&temp_path);
                if text.trim().is_empty() {
                    return ("vision".to_string(), "[ocr_empty]".to_string());
                }
                return ("vision".to_string(), text);
            }
        }

        let output = Command::new("tesseract")
            .arg(&temp_path)
            .arg("stdout")
            .arg("--psm")
            .arg("6")
            .output();
        let _ = fs::remove_file(&temp_path);

        match output {
            Ok(out) => {
                if !out.status.success() {
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    return (
                        "tesseract".to_string(),
                        format!(
                            "[ocr_error] tesseract_failed_status={} stderr={}",
                            out.status, stderr
                        ),
                    );
                }
                let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if text.is_empty() {
                    ("tesseract".to_string(), "[ocr_empty]".to_string())
                } else {
                    ("tesseract".to_string(), text)
                }
            }
            Err(err) => (
                "tesseract".to_string(),
                format!("[ocr_error] tesseract_not_available_or_failed_to_start: {err}"),
            ),
        }
    }

    #[cfg(target_os = "macos")]
    fn prepare_vision_helper_binary() -> Option<PathBuf> {
        let mut helper_dir = std::env::temp_dir();
        helper_dir.push("sensor_layer_cache");
        if fs::create_dir_all(&helper_dir).is_err() {
            return None;
        }

        let mut swift_path = helper_dir.clone();
        swift_path.push("vision_ocr_helper.swift");
        let mut binary_path = helper_dir;
        binary_path.push("vision_ocr_helper_bin");

        let swift_code = Self::vision_swift_source();
        if let Ok(mut file) = fs::File::create(&swift_path) {
            if file.write_all(swift_code.as_bytes()).is_err() {
                return None;
            }
        } else {
            return None;
        }

        // Compile once and reuse the binary for all OCR calls in this run.
        let compile = Command::new("swiftc")
            .arg(&swift_path)
            .arg("-O")
            .arg("-o")
            .arg(&binary_path)
            .output();

        match compile {
            Ok(out) if out.status.success() => Some(binary_path),
            _ => None,
        }
    }

    #[cfg(target_os = "macos")]
    fn run_macos_vision_ocr_fast(&self, image_path: &PathBuf) -> Result<String, String> {
        let helper = self
            .vision_helper_binary
            .as_ref()
            .ok_or_else(|| "vision_helper_not_available".to_string())?;

        let output = Command::new(helper).arg(image_path).output();
        match output {
            Ok(out) => {
                if !out.status.success() {
                    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                    return Err(format!(
                        "vision_helper_failed status={} stderr={stderr}",
                        out.status
                    ));
                }
                Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
            }
            Err(err) => Err(format!("vision_helper_failed_to_start: {err}")),
        }
    }

    #[cfg(target_os = "macos")]
    fn vision_swift_source() -> &'static str {
        r#"import Foundation
import Vision
import CoreGraphics
import ImageIO

if CommandLine.arguments.count < 2 {
    fputs("missing image path\n", stderr)
    exit(2)
}

let imagePath = CommandLine.arguments[1]
let url = URL(fileURLWithPath: imagePath)
guard let source = CGImageSourceCreateWithURL(url as CFURL, nil),
      let cgImage = CGImageSourceCreateImageAtIndex(source, 0, nil) else {
    fputs("failed to load image\n", stderr)
    exit(3)
}

let request = VNRecognizeTextRequest()
request.recognitionLevel = .accurate
request.usesLanguageCorrection = true

let handler = VNImageRequestHandler(cgImage: cgImage, options: [:])
do {
    try handler.perform([request])
    let observations = request.results ?? []
    let lines = observations.compactMap { $0.topCandidates(1).first?.string }
    print(lines.joined(separator: "\n"))
} catch {
    fputs("vision request failed: \(error)\n", stderr)
    exit(4)
}
"#
    }

    /// Drain all Phase 1 payloads intended for Phase 2 ingestion.
    pub fn drain_phase2_payloads(&mut self) -> Vec<Phase1Payload> {
        std::mem::take(&mut self.phase2_queue)
    }

    pub fn stats(&self) -> SensorStats {
        SensorStats {
            total_events_seen: self.total_events_seen,
            accepted_captures: self.accepted_captures,
            dropped_by_phash: self.dropped_by_phash,
            dropped_by_throttle: self.dropped_by_throttle,
        }
    }
}
