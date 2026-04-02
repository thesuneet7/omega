# Omega — Product roadmap & JTBD order

Use this document to **review manually**, **prioritize**, and **implement** in sequence. Check off sections as you ship.

---

## How to read this

- **JTBD (Jobs To Be Done)** — ordered list of outcomes users hire the product for. Implement **top to bottom** so each layer builds on the last.
- **Roadmap** — parallel tracks (product, UX, engineering, ops, business). Align milestones with JTBD.

---

## JTBD — recommended order

Complete earlier jobs before later ones; later jobs assume earlier foundations exist.

| Order | JTBD | Success looks like |
|------:|------|-------------------|
| **1** | **Trust the app on my machine** | I understand permissions, what is captured, what leaves the device, and how to stop or delete data. |
| **2** | **Capture meaningful activity without killing my machine** | Capture is stable, throttled, and resource usage is predictable; I rarely miss important context. |
| **3** | **Turn raw activity into usable memory** | Ingestion + stitching + summary produce coherent, scannable output I actually read. |
| **4** | **Get value on a predictable rhythm** | I can run or schedule “summarize this session” (or equivalent) with clear loading, errors, and completion. |
| **5** | **Find and revisit past work** | I can list sessions, open a summary, and search or filter without hunting files. |
| **6** | **Edit and own the narrative** | I can fix titles, merge noise, export, or share a version I’m willing to stand behind. |
| **7** | **Use it as a team (optional)** | Shared context, roles, and retention policies match how we work (only if single-player loop is proven). |
| **8** | **Pay for sustainable usage** | Pricing, quotas, and upgrades match cost; I’m not surprised by bills or outages. |

**Implementation hint:** Map every feature to **one primary JTBD**. If it serves none clearly, defer it.

---

## Roadmap — full stack

### Phase 0 — Lock the wedge (before heavy build)

- [ ] **ICP (ideal customer profile)** — one sentence: who is this *for* first?
- [ ] **Core loop** — 3 steps from “open app” to “I got value” (e.g. capture → fetch → read summary).
- [ ] **Non-goals** — what you will *not* do in the next 6–12 months.
- [ ] **Differentiation** — why local / why this pipeline vs. a generic recorder or LLM wrapper.

*Exit criteria:* You can pitch the product in **60 seconds** without mentioning implementation details.

---

### Phase 1 — Foundation (JTBD 1–2)

**Product & trust**

- [ ] First-run **permission explainers** (why screen / input / accessibility).
- [ ] In-app **data manifest**: what’s stored locally, paths, retention, delete-everything.
- [ ] **Privacy copy** aligned with reality (what hits Gemini/APIs vs. stays on disk).

**Engineering efficiency**

- [ ] **Resource budget**: CPU/disk targets; validate on a low-end laptop.
- [ ] **Capture policy** tuning (cooldowns, dedupe, scroll idle) documented and adjustable.
- [ ] **Structured logging** (levels, rotation) for support and debugging.

**UX**

- [ ] **Empty states** everywhere: first launch, no captures, no summaries.
- [ ] **Error states** with retry and “what happened” in human language.

---

### Phase 2 — Core pipeline reliability (JTBD 3–4)

**Product**

- [ ] Clear **pipeline states**: idle → capturing → processing → ready / failed.
- [ ] **Idempotent** re-runs where possible (don’t pay 2× for identical inputs).
- [ ] **Cost guardrails**: env or settings for model choice, caps, dry-run / stub for dev.

**Engineering**

- [ ] **Incremental processing** roadmap (only new chunks / changed sessions).
- [ ] **Embedding + model consistency** (same backend/model across phases; fail fast with a clear message).
- [ ] **Async jobs** if summarization is long-running (queue + status, optional cancel).

**UX**

- [ ] **Progress** for multi-step runs (not a silent spinner for minutes).
- [ ] **Toast / banner** pattern for success and failure.

---

### Phase 3 — Information architecture & UI system (JTBD 4–5)

**IA**

- [ ] **Home**: primary surface (e.g. current session or timeline) + **one** primary CTA.
- [ ] **History**: sessions / summaries list; secondary navigation.
- [ ] **Settings**: API keys, models, retention, advanced — calm and scannable.

**Design system**

- [ ] **Tokens**: spacing, type scale, radius, semantic colors (background / surface / border / accent / danger).
- [ ] **Components**: button, input, panel, list row, modal, toast — **reuse before new screens**.
- [ ] **Light + dark** parity (or explicit “dark only” with rationale).

**UX polish**

- [ ] **Keyboard shortcuts** for primary actions.
- [ ] **Focus states** and accessible contrast (WCAG-minded).
- [ ] **Motion**: subtle only (short durations); no distracting animation.

**Process**

- [ ] **Figma (or equivalent)** for key flows: empty, loading, error, success.
- [ ] **Design review** gate before net-new screens.

---

### Phase 4 — Narrative ownership & export (JTBD 6)

- [ ] **Edit** summaries with autosave or explicit save; revision history if valuable.
- [ ] **Export**: Markdown / PDF / copy — pick what your ICP actually uses.
- [ ] **Naming**: sessions and summaries human-readable by default.

---

### Phase 5 — Distribution & desktop quality

- [ ] **Signed** builds (macOS notarization path when ready).
- [ ] **Auto-update** channel (stable / beta).
- [ ] **Crash reporting** (opt-in) with privacy policy.
- [ ] **Version** and “copy debug info” for support.

---

### Phase 6 — Hosting, sync, teams (JTBD 7–8) — only when justified

**Prerequisites**

- [ ] Retention and engagement in **single-player** mode are healthy.
- [ ] Clear **paid** value (usage-based or seat-based story).

**Hosting (typical shape)**

- [ ] **Identity** (auth) — email/OAuth; sessions and tokens.
- [ ] **Optional cloud**: encrypted sync of *metadata* and pointers; heavy blobs stay local or encrypted client-side if that’s the promise.
- [ ] **Boring infra**: managed DB, object storage, one region first; observability from day one.

**Business**

- [ ] **Pricing** tied to usage (captures, summaries, retention) or seats for teams.
- [ ] **Billing** provider and webhook hardening.
- [ ] **SOC2** as a roadmap item if selling to SMB/enterprise.

---

### Phase 7 — Growth & narrative (ongoing)

- [ ] **Demo script** — 60s, one user, one wow.
- [ ] **Metrics**: activation (first successful summary), D/W retention, cost per active user.
- [ ] **Content**: docs, short videos, comparison pages (only when positioning is stable).

---

## Suggested manual workflow for you

1. **Read JTBD table** — agree on order or reorder *once* with rationale.
2. **Score Phase 0** — don’t start UI overhaul until wedge + loop are written down.
3. **Pick one vertical slice** per sprint: e.g. “JTBD 4 end-to-end: fetch → progress → summary → error.”
4. **Design before code** for anything user-facing: Figma → components → screens.
5. **Measure**: add one metric per milestone (even if manual at first).

---

## Appendix — efficiency checklist (engineering)

Use as a backlog; not all items apply on day one.

| Area | Direction |
|------|-----------|
| Embeddings | Skip re-embed when chunk hash unchanged; batch where API allows. |
| LLM | Smaller/faster models for drafts; larger only on explicit action; truncate with clear UI. |
| Storage | Compaction / retention jobs; optional “archive old sessions.” |
| DB | Indexes for hot queries; avoid N+1 in session list APIs. |
| Client | Don’t block UI thread; streaming responses where it helps perception. |

---

*Last updated: living document — edit dates and owners as you go.*
