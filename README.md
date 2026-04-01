## Omega Sensor Layer (Phase 1)

This project implements **Phase 1: The Sensor Layer (Smart Capture)** of your system: a local‑first desktop agent that watches your activity, captures *meaningful* screens, extracts text, and writes structured logs for later phases.

The goal of Phase 1 is: **collect the maximum useful signal with the minimum CPU, memory, and disk**, while keeping everything on-device.

### Collaborator Quickstart (clone and run)

This repo is designed to run natively (host machine), including global input/screen capture.
Use `req.txt` to install and verify local prerequisites first.

```bash
git clone <your-repo-url>
cd omega
cat req.txt
cp .env.example .env
# Fill OMEGA_GEMINI_API_KEY in .env

# 1) Run Phase 1 natively
cargo run -- capture

# Stop with Ctrl+C after activity is captured.

# 2) Run Phase 2 natively
cargo run -- phase2
```

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

Phase 2 turns the Phase 1 session log into a production-ready ingestion pipeline:

- **Semantic canonical text (default)** for embeddings: coarse `app` + **cleaned OCR body** (no per-capture timestamp/window title in the embedded string — those change every frame and break clustering)
- **OCR cleanup (default)**: app-agnostic line scoring (length, words, sentence shape, symbol noise); drops URLs/paths; optional generic `title — short tail` strip; keeps lines within a **relative score band** so UI chrome is down-ranked without naming specific apps
- Optional PII redaction (email/phone/card-like patterns)
- Smart chunking with overlap for long OCR blocks
- Embeddings per chunk (document task type)
- SQLite persistence (`logs/phase2.db`) for captures/chunks/embeddings
- Deduplication by hash to avoid re-embedding repeated data
- Retry + exponential backoff for 429/5xx embedding API responses

### Run with cloud embeddings (recommended)

Uses the **Gemini embeddings API** by default. The default embedding model is **`gemini-embedding-001`** (stable, text-only, cost-efficient). You can still set `OMEGA_EMBED_MODEL=text-embedding-004` for legacy compatibility.

Create a local `.env` file once (already supported by the app):

```bash
cp .env.example .env
```

Then fill your key in `.env` and run normally:

```bash
cargo run -- phase2 --input logs/capture-session-*.json
```

By default this writes:

- JSON artifact: `logs/phase2-ingestion-<unix_ts>.json`
- SQLite DB: `logs/phase2.db`

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

### Phase 2 tuning knobs

```bash
OMEGA_PHASE2_DB_PATH=logs/phase2.db
OMEGA_EMBED_MODEL=gemini-embedding-001
OMEGA_CHUNK_SIZE_CHARS=1200
OMEGA_CHUNK_OVERLAP_CHARS=200
OMEGA_REDACT_PII=true
OMEGA_EMBED_MAX_RETRIES=3
OMEGA_EMBED_RETRY_BASE_DELAY_MS=500
```

**Canonical + OCR (clustering / Phase 3):**

```bash
# Default: semantic embeddings + cleaned OCR (recommended for Phase 3).
OMEGA_PHASE2_CANONICAL_MODE=semantic
OMEGA_PHASE2_OCR_CLEAN=true
# Keep lines with score >= this fraction of the strongest line in the same capture (lower = stricter).
OMEGA_PHASE2_OCR_LINE_SCORE_RATIO=0.12
# Duplicate the strongest line once at the end so the embedding emphasizes real body text.
OMEGA_PHASE2_OCR_EMPHASIS_TOP=true

# Legacy: verbose canonical string (timestamp, window_title, event, resolution in the embedded text).
OMEGA_PHASE2_CANONICAL_MODE=full
```

After changing canonical mode or OCR cleaning, **re-run Phase 2** so chunks and embeddings are rebuilt. For a clean Phase 3 pass, remove old bucket rows or use a fresh DB, e.g.:

```bash
sqlite3 logs/phase2.db "DELETE FROM task_bucket_items; DELETE FROM task_buckets;"
```

(or delete `logs/phase2.db` and re-ingest).

---

## Phase 3 (Semantic Stitching) - Build it

Phase 3 turns individual Phase 2 chunks into higher-level **Task Buckets** using
semantic similarity and recency weighting.

Exactly what this phase does:

- Reads chunk embeddings and metadata from `logs/phase2.db`
- Loads existing task buckets (if present) so stitching is incremental across runs
- For each unassigned chunk:
  - Computes cosine similarity against active bucket centroids
  - Applies a time-decay multiplier (`exp(-lambda * delta_t)`)
  - Assigns to the best existing bucket if score >= threshold
  - Otherwise creates a new bucket
- Updates bucket centroid as a running average after each assignment
- Persists bucket state + item assignments in SQLite
- Writes an artifact JSON: `logs/phase3-stitching-<unix_ts>.json` (each bucket includes **first/last capture time** and **distinct app names** for quick sanity checks before Phase 4)

### Run Phase 3

```bash
cargo run -- phase3
```

Optional arguments:

```bash
cargo run -- phase3 --db logs/phase2.db --output logs/phase3-stitching-custom.json
```

### Phase 3 tuning knobs

Use the **same** `OMEGA_EMBEDDING_BACKEND` and `OMEGA_EMBED_MODEL` as Phase 2 so stitching reads the intended embedding rows (avoids duplicates when multiple models exist in `embeddings`).

```bash
OMEGA_PHASE3_DB_PATH=logs/phase2.db
OMEGA_EMBEDDING_BACKEND=gemini
OMEGA_EMBED_MODEL=gemini-embedding-001
OMEGA_PHASE3_MATCH_THRESHOLD=0.60
OMEGA_PHASE3_DECAY_LAMBDA=0.00002
OMEGA_PHASE3_ACTIVE_WINDOW_MINS=15
```

### SQLite tables used by Phase 3

- `task_buckets`
  - bucket centroid vector, item count, created/last-active timestamps
- `task_bucket_items`
  - chunk -> bucket assignment, source timestamp, match score, new-bucket flag

This gives you a local semantic context layer that Phase 4 can summarize directly.

---

## Phase 4 (Summaries) — Run it

Phase 4 turns each **task bucket** into a **structured, human-readable summary** (title, one-liner, detailed summary, tags, confidence, caveats) using an LLM, with **production-style** behavior:

**By default, summaries come from the Gemini API** (`OMEGA_PHASE4_BACKEND=gemini`, same key as Phase 2). The only alternative is **`stub`**, which does **not** call an LLM — it writes deterministic placeholder text for tests or air‑gapped runs. There is no other summarization path in this repo.

- **Idempotency**: Skips a bucket when the **input fingerprint** (SHA-256 of sorted chunk hashes) is unchanged **and** the stored **`prompt_version`**, **`backend`**, and **`model`** match the current run (so switching `OMEGA_PHASE4_BACKEND` or `OMEGA_PHASE4_MODEL` does not leave stale stub text while “skipping”).
- **Prompt upgrades**: When you bump `PROMPT_VERSION` in code, existing rows are **re-summarized** automatically unless you rely on unchanged fingerprints only — the version check forces refresh when the prompt/schema changes.
- **Retries**: Same exponential backoff pattern as Phase 2 for 429/5xx on Gemini.
- **Large buckets**: Long OCR is **truncated** head/tail with an explicit marker (`OMEGA_PHASE4_MAX_INPUT_CHARS`, default 48k).
- **Artifacts + DB**: Writes `logs/phase4-summaries-<unix_ts>.json` and persists rows in SQLite table `task_bucket_summaries`.

### Run Phase 4 (Gemini, default)

Requires `OMEGA_GEMINI_API_KEY` (same as Phase 2).

```bash
cargo run -- phase4
```

Optional:

```bash
cargo run -- phase4 --db logs/phase2.db --output logs/my-phase4.json
cargo run -- phase4 --force          # re-summarize every bucket
cargo run -- phase4 --dry-run        # no API calls; inspect sizing/fingerprints
```

### Run Phase 4 without network (stub)

Deterministic placeholder summaries — useful for CI or validating the pipeline:

```bash
export OMEGA_PHASE4_BACKEND=stub
cargo run -- phase4
```

### Phase 4 tuning knobs

```bash
OMEGA_PHASE4_DB_PATH=logs/phase2.db
OMEGA_PHASE4_BACKEND=gemini|stub
OMEGA_PHASE4_MODEL=gemini-2.5-flash-lite
OMEGA_PHASE4_MAX_INPUT_CHARS=48000
OMEGA_PHASE4_MAX_RETRIES=3
OMEGA_PHASE4_RETRY_BASE_DELAY_MS=800
OMEGA_PHASE4_FORCE=true
OMEGA_PHASE4_DRY_RUN=true
```

### SQLite tables used by Phase 4

- `task_bucket_summaries`
  - `bucket_id`, `input_fingerprint`, `summary_json` (full structured record), `model`, `backend`, `prompt_version`, `generated_at_epoch_secs`

