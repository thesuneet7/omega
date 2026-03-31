# High level Blueprint

Created by: Vasudharaje Srivastava
Created time: March 30, 2026 7:00 PM
Category: Planning, Strategy doc
Last edited by: Suneet
Last updated time: March 31, 2026 3:13 PM

---

## Phase 1: The Sensor Layer (Smart Capture)

*Goal: Collect the maximum amount of signal with the minimum amount of computer resources.*

1. **The OS Listener (Event-Driven):** * Your desktop app (built in Tauri/Rust for low memory footprint) hooks into OS accessibility APIs.
    - It does *not* run a timer. Instead, it waits for triggers: `WindowFocusChanged`, `MouseClick`, `ScrollStopped(1.5s delay)`.
2. **The pHash Gatekeeper:**
    - When a trigger fires, the app takes a screenshot *into RAM* (memory), not the hard drive.
    - It generates a Perceptual Hash (pHash) of the image.
    - It compares the new pHash to the last saved pHash. If they are >95% similar, the frame is **dropped immediately**.
3. **The Audio Buffer:**
    - A continuous 30-second audio loop runs in the background. When speech is detected (using a lightweight Voice Activity Detector), it saves the audio chunk for transcription.

### Phase 2: The Ingestion Layer (Extraction & Embedding)

*Goal: Turn raw pixels and audio into mathematical meaning.*

1. **Fast OCR & Transcription:**
    - The approved, unique screenshot is passed to a local, optimized OCR engine (like Apple's Vision Framework on Mac, or Windows.Media.Ocr on PC).
    - Audio chunks are passed to `Whisper.cpp` for local transcription.
2. **Instant Image Deletion:**
    - Once the text and window metadata (timestamp, app name, window title) are extracted, the screenshot is **permanently deleted**. You only store text.
3. **Real-Time Vectorization:**
    - The extracted text block is passed to a tiny, lightning-fast local embedding model (e.g., `nomic-embed-text`).
    - This model converts the text into a Vector (a list of numbers representing its meaning).

### Phase 3: The Semantic Stitching Engine (The "Brain")

*Goal: Group related activities together, regardless of how far apart they happened in the day.*

1. **The Vector Database:**
    - You store the raw text, metadata, and the Vector in a local database like `sqlite-vec` or **ChromaDB**.
2. **The "Active Context" Whiteboard (Batch Process):**
    - Every 15 minutes, a background cron job wakes up to group the latest vectors into "Task Buckets."
3. **The Matching Logic (Similarity + Time Decay):**
    - The engine compares the newest vectors against existing Task Buckets. To prevent old, finished tasks from grabbing new data, you apply a time decay penalty. The formula looks something like this:
    
    $$
    \text{Match Score} = \text{Cosine Similarity}(V_{\text{new}}, V_{\text{bucket}}) \times e^{-\lambda \Delta t}
    $$
    
    - **(Where \delta is the time elapsed since the bucket was last active, and \lambda is your decay rate.)**
    - If the Match Score hits your threshold, the log goes into the existing bucket. If not, a new Task Bucket is created.

### Phase 4: The Synthesis Layer (Deliverables)

*Goal: Turn messy data buckets into clean, corporate-ready reports.*

1. **The Trigger:**
    - The user clicks "Generate EOD Report" in your UI, or it triggers automatically at 5:00 PM.
2. **The Heavy LLM:**
    - Now, and *only* now, you spin up a heavier local LLM (like **Llama 3 8B** or **Qwen** via Ollama).
3. **Prompting the Buckets:**
    - Your backend grabs the top 3-5 most active "Task Buckets" of the day.
    - It feeds the raw text of Bucket 1 to the LLM with a strict system prompt: *"Review these chronological logs for a single project. Summarize the main objective, the key decisions made, and write 3 actionable next steps for tomorrow."*
4. **The Final Output:**
    - The LLM generates a clean Markdown file.
    - Your app presents this to the user, allowing them to edit it, copy it to Slack, or sync it directly to Notion/Jira.

---

### The Architecture Summary

To visualize the tech stack required for this:

| **Component** | **Recommended Tech Stack (Local-First)** |
| --- | --- |
| **Desktop App Framework** | Tauri (Rust/React) |
| **OCR / Vision** | Native OS APIs (Apple Vision / Windows OCR) |
| **Embedding Model** | `nomic-embed-text` (via ONNX runtime) |
| **Database** | SQLite + `sqlite-vec` extension |
| **Heavy LLM (Summarization)** | Ollama running Llama 3/4 (8B) |

This architecture scales beautifully. By using semantic matching, your tool actually *understands* what the user is working on, rather than just guessing based on the clock.