## Omega Sensor Layer (Phase 1)

This project implements **Phase 1: The Sensor Layer (Smart Capture)** of your system: a local‑first desktop agent that watches your activity, captures *meaningful* screens, extracts text, and writes structured logs for later phases.

The goal of Phase 1 is: **collect the maximum useful signal with the minimum CPU, memory, and disk**, while keeping everything on-device.

---

### High-Level Flow

1. **Global OS events (input)**
   - Listened via `rdev`:
     - `MouseClick`
     - `KeyPress`
     - `Scroll` (wheel events)
   - These are turned into internal `SensorEvent`s and sent over a channel to the sensor engine.

2. **Event → Capture decision**
   - For each event, `SensorEngine` decides whether to *attempt* a capture:
     - **MouseClick / KeyPress**
       - May trigger a capture *if* global cooldown allows.
     - **Scroll**
       - Does **not** capture immediately.
       - Marks `scroll_pending = true` and remembers the last scroll time.
       - A periodic `tick()` is called from the main loop; if scroll has been idle for `scroll_idle_delay` (default 1500 ms), one capture is attempted with `event_type = "ScrollStopped"`.

3. **Global throttling (cooldown)**
   - Before doing any screenshot work, the engine checks:
     - `last_capture_instant` and `capture_cooldown` (default 500 ms).
   - If the last capture was too recent, the attempt is **dropped by throttle**:
     - No screenshot
     - No pHash
     - No OCR
   - This protects you from high-frequency event storms.

4. **Screenshot capture (real screen)**
   - Implemented with the `screenshots` crate.
   - Takes a full screenshot of the primary display into RAM as `DynamicImage` (RGBA).
   - No raw screenshots are stored permanently; they live only in memory for this pipeline.

5. **pHash gatekeeper (visual deduplication)**
   - Computes a 64‑bit perceptual hash (pHash) for the screenshot.
   - Compares against the last accepted pHash:
     - Computes similarity in \[0, 1\].
     - If similarity ≥ 0.95 → **drop the frame** as visually too similar.
   - Only when similarity is below threshold is this frame considered *visually new* and allowed to proceed.

6. **Active app + window metadata**
   - On macOS, Phase 1 queries the frontmost app/window via `osascript`:
     - `app_name`: frontmost process name (e.g. `Safari`, `Google Chrome`).
     - `window_title`: title of the front window (fallback `"Unknown Window"`).
   - If the query fails (permissions, etc.), we fall back to:
     - `app_name = "unknown.app"`
     - `window_title = "Unknown Window"`.

7. **OCR extraction (Vision first, Tesseract fallback)**
   - **Runs only on accepted screenshots**, after the pHash gate.
   - Saves the screenshot to a temp PNG.
   - On macOS:
     - Uses a pre‑compiled **Vision Framework helper binary** (built once per run from embedded Swift code with `swiftc`).
     - If Vision succeeds:
       - Extracts line‑wise recognized text.
       - Sets `ocr_engine_used = "vision"`.
   - If Vision is unavailable or fails:
     - Falls back to the `tesseract` CLI:
       - `tesseract <temp.png> stdout --psm 6`
       - Sets `ocr_engine_used = "tesseract"`.
   - If OCR fails entirely, placeholder markers like `[ocr_error] ...` or `[ocr_empty]` are stored to make debugging explicit.
   - The temp image and helper script/binary are cleaned up from the temp directory.

8. **Phase 1 → Phase 2 payload**
   - For each accepted capture, we build a `VisualLogItem` and wrap it in `Phase1Payload::Visual`:
     - `id`: monotonically increasing numeric ID.
     - `timestamp`: `SystemTime` of capture.
     - `app_name`: frontmost app name (or fallback).
     - `window_title`: front window title (or fallback).
     - `event_type`: `"MouseClick"`, `"KeyPress"`, or `"ScrollStopped"`.
     - `width`, `height`: screenshot dimensions.
     - `ocr_engine_used`: `"vision"`, `"tesseract"`, or `"none"`.
     - `ocr_text`: extracted text block (or error marker).
   - These payloads go into an in‑memory queue, which the main loop periodically drains and appends to the session log.

9. **Session logging and shutdown**
   - The app runs until you press **Ctrl+C**.
   - A Ctrl+C handler flips a `running` flag; the main loop exits cleanly.
   - On exit:
     - `engine.flush_pending_scroll()` is called to try to emit any final `ScrollStopped` capture.
     - Remaining payloads in the engine queue are drained.
     - A `SessionLogFile` is written to `./logs/capture-session-<unix_ts>.json`.

The session log file contains:

- `session_summary`:
  - `session_start_epoch_secs`
  - `session_end_epoch_secs`
  - `session_duration_secs`
  - `total_events_seen`
  - `accepted_captures`
  - `dropped_by_phash`
  - `dropped_by_throttle`
  - `per_event_counts` (counts of MouseClick / KeyPress / Scroll)
  - `saved_payload_count`
- `payloads`:
  - Array of `Phase1Payload::Visual` objects described above.

---

### Key Invariants / Design Principles

- **Local-first + privacy**: no images or text leave your machine. Screenshots are only temporary; logs are plain JSON on disk.
- **Event-driven, not polling**: work is tied to real user activity (clicks, keys, scrolls), not a fixed timer.
- **Two-stage filtering for cost control**:
  - Stage 1: **Throttle** at the event level (cooldown + scroll idle debounce).
  - Stage 2: **pHash dedupe** at the image level.
  - OCR runs *only* after both filters are passed.
- **Metadata completeness**: every capture has enough context (`app_name`, `window_title`, timestamps, OCR text) to be useful downstream.

---

### Running Phase 1

#### Prerequisites

- **Rust toolchain** with `cargo` installed.
- On macOS:
  - Xcode command-line tools (for `swiftc` and Vision).
  - `osascript` available (part of macOS).
- Optional but recommended:
  - `tesseract` installed via Homebrew as fallback:

```bash
brew install tesseract
```

#### Build and run

From the project root:

```bash
cd /Users/suneet/Desktop/Folders/omega
source "$HOME/.cargo/env"   # if using rustup in a new shell
cargo run
```

On first run you may need to:

- Grant **Input Monitoring / Accessibility** permission (for global key/mouse events).
- Grant **Screen Recording** permission (for screenshots).

Once running, the terminal will print:

- Console JSON dumps of newly accepted payload batches.
- A summary line on shutdown showing how many items were saved and the log path.

#### Stopping the app

- Press **Ctrl+C** once in the terminal.
- The app will:
  - Stop listening to events.
  - Try to flush any pending `ScrollStopped` capture.
  - Write a new file into `./logs/` named `capture-session-<unix_timestamp>.json`.

---

### Where to Look Next (Phase 2)

Phase 1 is now a reliable producer of **semantic-ready logs**:

- Clean, deduplicated visual snapshots.
- Rich OCR text from on-screen content.
- Accurate app/window and event metadata.

For **Phase 2: Ingestion Layer**, you will:

- Read `logs/capture-session-*.json`.
- For each `VisualLogItem`, build a text block using OCR + metadata.
- Generate vector embeddings (cloud by default; local fallback available).
- Store:
  - Raw text.
  - Metadata.
  - Vector embeddings.

Those embeddings then feed Phase 3 (semantic stitching) and Phase 4 (summaries).

---

## Phase 2 (Ingestion) — Run it

Phase 2 turns the Phase 1 session log into an ingestion artifact that contains:

- Canonical text per capture (metadata + OCR)
- A stable SHA-256 fingerprint of that canonical text
- An embedding vector per capture

### Run with cloud embeddings (recommended)

Uses the **Gemini embeddings API** by default.

```bash
export OMEGA_EMBEDDING_BACKEND=gemini
export OMEGA_GEMINI_API_KEY="..."
export OMEGA_EMBED_MODEL="text-embedding-004"   # optional

cargo run -- phase2 --input logs/capture-session-*.json
```

Optional (custom Gemini endpoint):

```bash
export OMEGA_GEMINI_BASE_URL="https://generativelanguage.googleapis.com"
```

### Run with OpenAI-compatible embeddings

If you prefer OpenAI (or a compatible gateway):

```bash
export OMEGA_EMBEDDING_BACKEND=openai
export OMEGA_OPENAI_API_KEY="..."
export OMEGA_OPENAI_BASE_URL="https://api.openai.com"
export OMEGA_EMBED_MODEL="text-embedding-3-small"

cargo run -- phase2 --input logs/capture-session-*.json
```

### Run without network (dev/test fallback)

This uses a deterministic hash-based embedding (not semantic; for pipeline validation only).

```bash
export OMEGA_EMBEDDING_BACKEND=hash
cargo run -- phase2 --input logs/capture-session-*.json
```

