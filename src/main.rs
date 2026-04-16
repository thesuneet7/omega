mod models;
mod openai_compat_url;
mod phase2;
mod phase3;
mod phase4;
mod capture_live_status;
mod phash;
mod privacy_config;
mod sensor;

use crossbeam_channel::{unbounded, Receiver};
use rdev::{listen, Event, EventType};
use sensor::{SensorEngine, SensorEvent};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Serialize)]
struct SessionSummary {
    session_start_epoch_secs: u64,
    session_end_epoch_secs: u64,
    session_duration_secs: u64,
    total_events_seen: u64,
    accepted_captures: u64,
    dropped_by_phash: u64,
    dropped_by_throttle: u64,
    dropped_by_exclusion: u64,
    dropped_by_pause: u64,
    per_event_counts: BTreeMap<String, u64>,
    saved_payload_count: usize,
}

#[derive(Debug, Serialize)]
struct SessionLogFile {
    session_summary: SessionSummary,
    payloads: Vec<models::Phase1Payload>,
}

fn spawn_event_listener(tx: crossbeam_channel::Sender<SensorEvent>) {
    std::thread::spawn(move || {
        let callback = move |event: Event| {
            let sensor_event = match event.event_type {
                EventType::ButtonPress(_) => Some(SensorEvent::MouseClick),
                EventType::KeyPress(_) => Some(SensorEvent::KeyPress),
                EventType::Wheel { .. } => Some(SensorEvent::Scroll),
                _ => None,
            };

            if let Some(ev) = sensor_event {
                // Ignore send errors (receiver may have been dropped on shutdown).
                let _ = tx.send(ev);
            }
        };

        if let Err(e) = listen(callback) {
            eprintln!("error from global event listener: {:?}", e);
        }
    });
}

fn capture_logs_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("OMEGA_APP_LOGS_DIR").unwrap_or_else(|_| "logs".to_string()),
    )
}

fn save_logs_to_file(log_file: &SessionLogFile) -> Result<PathBuf, String> {
    let logs_dir = capture_logs_dir();
    fs::create_dir_all(&logs_dir).map_err(|e| format!("failed to create logs directory: {e}"))?;

    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| format!("failed to compute timestamp: {e}"))?
        .as_secs();
    let out_path = logs_dir.join(format!("capture-session-{ts}.json"));

    let json = serde_json::to_string_pretty(log_file)
        .map_err(|e| format!("failed to serialize log file: {e}"))?;
    fs::write(&out_path, json).map_err(|e| format!("failed to write log file: {e}"))?;

    Ok(out_path)
}

fn main() {
    // Load local .env if present so users don't need manual exports every run.
    let _ = dotenvy::dotenv();

    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("phase2") => {
            if let Err(err) = run_phase2_from_args(args.collect()) {
                eprintln!("Phase 2 ingestion failed: {err:#}");
                std::process::exit(1);
            }
        }
        Some("phase3") => {
            if let Err(err) = run_phase3_from_args(args.collect()) {
                eprintln!("Phase 3 stitching failed: {err:#}");
                std::process::exit(1);
            }
        }
        Some("phase4") => {
            if let Err(err) = run_phase4_from_args(args.collect()) {
                eprintln!("Phase 4 summarization failed: {err:#}");
                std::process::exit(1);
            }
        }
        Some("capture") | None => run_capture(),
        Some(other) => {
            eprintln!("Unknown command '{other}'");
            print_usage();
            std::process::exit(2);
        }
    }
}

fn run_capture() {
    let session_start = SystemTime::now();
    // Channel from global OS events to the sensor engine.
    let (tx, rx): (
        crossbeam_channel::Sender<SensorEvent>,
        Receiver<SensorEvent>,
    ) = unbounded();

    // Start global input listener (mouse, keyboard, scroll).
    spawn_event_listener(tx);

    // Stop gracefully on Ctrl+C and flush logs to disk.
    let running = Arc::new(AtomicBool::new(true));
    let running_for_handler = Arc::clone(&running);
    ctrlc::set_handler(move || {
        running_for_handler.store(false, Ordering::SeqCst);
    })
    .expect("failed to install Ctrl+C handler");

    // Run the sensor engine loop until you stop the process (Ctrl+C).
    let mut engine = SensorEngine::new();
    let mut all_payloads: Vec<models::Phase1Payload> = Vec::new();
    let mut per_event_counts: BTreeMap<String, u64> = BTreeMap::new();
    println!("Listening for global input events. Move/click/scroll to generate captures.");
    println!("Press Ctrl+C to stop and save logs to ./logs/*.json");

    while running.load(Ordering::SeqCst) {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(event) => {
                let event_key = match &event {
                    SensorEvent::MouseClick => "MouseClick",
                    SensorEvent::KeyPress => "KeyPress",
                    SensorEvent::Scroll => "Scroll",
                };
                *per_event_counts.entry(event_key.to_string()).or_insert(0) += 1;

                engine.handle_event(event);

                let payloads = engine.drain_phase2_payloads();
                if !payloads.is_empty() {
                    all_payloads.extend(payloads.clone());
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&payloads).expect("serialize payloads")
                    );
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                engine.tick();
            }
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
        }
    }

    // If the user stops while we're waiting for "scroll idle", try to flush once.
    engine.flush_pending_scroll();

    // Flush any payloads still in the engine queue.
    let remaining = engine.drain_phase2_payloads();
    if !remaining.is_empty() {
        all_payloads.extend(remaining);
    }

    let session_end = SystemTime::now();
    let start_epoch = session_start
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let end_epoch = session_end
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let duration_secs = session_end
        .duration_since(session_start)
        .unwrap_or_default()
        .as_secs();
    let stats = engine.stats();

    let log_file = SessionLogFile {
        session_summary: SessionSummary {
            session_start_epoch_secs: start_epoch,
            session_end_epoch_secs: end_epoch,
            session_duration_secs: duration_secs,
            total_events_seen: stats.total_events_seen,
            accepted_captures: stats.accepted_captures,
            dropped_by_phash: stats.dropped_by_phash,
            dropped_by_throttle: stats.dropped_by_throttle,
            dropped_by_exclusion: stats.dropped_by_exclusion,
            dropped_by_pause: stats.dropped_by_pause,
            per_event_counts,
            saved_payload_count: all_payloads.len(),
        },
        payloads: all_payloads,
    };

    match save_logs_to_file(&log_file) {
        Ok(path) => println!(
            "Saved {} log items to {}",
            log_file.session_summary.saved_payload_count,
            path.display()
        ),
        Err(err) => eprintln!("Could not save logs: {err}"),
    }
}

fn run_phase2_from_args(args: Vec<String>) -> anyhow::Result<()> {
    let mut input_log: Option<PathBuf> = None;
    let mut output_path: Option<PathBuf> = None;
    let mut idx = 0usize;
    while idx < args.len() {
        match args[idx].as_str() {
            "--input" => {
                idx += 1;
                let Some(value) = args.get(idx) else {
                    anyhow::bail!("missing value for --input");
                };
                input_log = Some(PathBuf::from(value));
            }
            "--output" => {
                idx += 1;
                let Some(value) = args.get(idx) else {
                    anyhow::bail!("missing value for --output");
                };
                output_path = Some(PathBuf::from(value));
            }
            "--help" | "-h" => {
                print_usage();
                return Ok(());
            }
            unknown => anyhow::bail!("unknown phase2 argument '{unknown}'"),
        }
        idx += 1;
    }

    let config = phase2::IngestionConfig::from_env_and_args(input_log, output_path)?;
    let (output_file, _) = phase2::run_ingestion(config)?;
    println!("Phase 2 ingestion completed: {}", output_file.display());
    Ok(())
}

fn run_phase4_from_args(args: Vec<String>) -> anyhow::Result<()> {
    let mut output_path: Option<PathBuf> = None;
    let mut db_path: Option<PathBuf> = None;
    let mut force = false;
    let mut dry_run = false;
    let mut idx = 0usize;
    while idx < args.len() {
        match args[idx].as_str() {
            "--output" => {
                idx += 1;
                let Some(value) = args.get(idx) else {
                    anyhow::bail!("missing value for --output");
                };
                output_path = Some(PathBuf::from(value));
            }
            "--db" => {
                idx += 1;
                let Some(value) = args.get(idx) else {
                    anyhow::bail!("missing value for --db");
                };
                db_path = Some(PathBuf::from(value));
            }
            "--force" => {
                force = true;
            }
            "--dry-run" => {
                dry_run = true;
            }
            "--help" | "-h" => {
                print_usage();
                return Ok(());
            }
            unknown => anyhow::bail!("unknown phase4 argument '{unknown}'"),
        }
        idx += 1;
    }

    let config = phase4::SummarizeConfig::from_env_and_args(db_path, output_path, force, dry_run)?;
    let (output_file, _) = phase4::run_summarization(config)?;
    println!("Phase 4 summarization completed: {}", output_file.display());
    Ok(())
}

fn run_phase3_from_args(args: Vec<String>) -> anyhow::Result<()> {
    let mut output_path: Option<PathBuf> = None;
    let mut db_path: Option<PathBuf> = None;
    let mut idx = 0usize;
    while idx < args.len() {
        match args[idx].as_str() {
            "--output" => {
                idx += 1;
                let Some(value) = args.get(idx) else {
                    anyhow::bail!("missing value for --output");
                };
                output_path = Some(PathBuf::from(value));
            }
            "--db" => {
                idx += 1;
                let Some(value) = args.get(idx) else {
                    anyhow::bail!("missing value for --db");
                };
                db_path = Some(PathBuf::from(value));
            }
            "--help" | "-h" => {
                print_usage();
                return Ok(());
            }
            unknown => anyhow::bail!("unknown phase3 argument '{unknown}'"),
        }
        idx += 1;
    }

    let config = phase3::StitchConfig::from_env_and_args(db_path, output_path)?;
    let output_file = phase3::run_stitching(config)?;
    println!("Phase 3 stitching completed: {}", output_file.display());
    Ok(())
}

fn print_usage() {
    eprintln!(
        "Usage:\n  cargo run -- capture\n  cargo run -- phase2 [--input <path>] [--output <path>]\n  cargo run -- phase3 [--db <path>] [--output <path>]\n  cargo run -- phase4 [--db <path>] [--output <path>] [--force] [--dry-run]\n\nDefaults:\n  - capture is the default when no command is provided\n  - phase2 uses the latest logs/capture-session-*.json if --input is omitted\n  - phase3 uses logs/phase2.db when --db is omitted\n  - phase4 uses logs/phase2.db when --db is omitted\n\nEnvironment (phase2):\n  OMEGA_EMBEDDING_BACKEND=gemini|openai|xai|hash (default: gemini)\n  OMEGA_EMBED_MODEL=gemini-embedding-001 (Gemini default)\n  OMEGA_PHASE2_DB_PATH=logs/phase2.db\n  OMEGA_CHUNK_SIZE_CHARS=1200\n  OMEGA_CHUNK_OVERLAP_CHARS=200\n  OMEGA_REDACT_PII=true\n  OMEGA_EMBED_MAX_RETRIES=3\n  OMEGA_EMBED_RETRY_BASE_DELAY_MS=500\n  OMEGA_PHASE2_CANONICAL_MODE=semantic|full (default: semantic)\n  OMEGA_PHASE2_OCR_CLEAN=true|false (default: true)\n  OMEGA_PHASE2_OCR_LINE_SCORE_RATIO=0.0-1.0 (default: 0.12)\n  OMEGA_PHASE2_OCR_EMPHASIS_TOP=true|false (default: true)\n\nEnvironment (phase3):\n  OMEGA_PHASE3_DB_PATH=logs/phase2.db\n  OMEGA_EMBEDDING_BACKEND=gemini|openai|xai|hash (match Phase 2)\n  OMEGA_EMBED_MODEL=gemini-embedding-001 (match Phase 2)\n  OMEGA_PHASE3_MATCH_THRESHOLD=0.82\n  OMEGA_PHASE3_DECAY_LAMBDA=0.00002\n  OMEGA_PHASE3_ACTIVE_WINDOW_MINS=15\n  OMEGA_PHASE3_MAX_CENTROID_WEIGHT=10\n  OMEGA_PHASE3_REFINEMENT_ROUNDS=3\n\nEnvironment (phase4):\n  OMEGA_PHASE4_DB_PATH=logs/phase2.db\n  OMEGA_PHASE4_BACKEND=gemini|stub (default: gemini)\n  OMEGA_PHASE4_MODEL=gemini-2.5-flash-lite\n  OMEGA_PHASE4_MAX_INPUT_CHARS=48000\n  OMEGA_PHASE4_MAX_RETRIES=3\n  OMEGA_PHASE4_RETRY_BASE_DELAY_MS=800\n  OMEGA_PHASE4_FORCE=true (re-summarize all buckets)\n  OMEGA_PHASE4_DRY_RUN=true (no API calls)\n\nGemini config:\n  OMEGA_GEMINI_API_KEY=... (required when backend=gemini)\n  OMEGA_GEMINI_BASE_URL=https://generativelanguage.googleapis.com\n\nOpenAI-compatible config:\n  OMEGA_OPENAI_API_KEY=... (required when backend=openai)\n  OMEGA_OPENAI_BASE_URL=https://api.openai.com"
    );
}
