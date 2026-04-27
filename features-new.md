# Omega Feature Priorities (PM/Consultant Wedge)

This document defines what to build now for Omega based on the PM/consultant ICP and the core promise: **send-ready stakeholder brief in 10 minutes, with receipts**.

---

## Product goal (right now)

Build one trusted workflow that turns fragmented work context into a stakeholder-ready brief quickly and reliably.

**Primary outcome metric:** median time from generate to send-ready brief.

---

## Priority 0 (must build first): trust + core outcome

### 1) Stakeholder Brief Mode (single golden workflow)
- Input: selected time window from captured context (for example, last 24 hours or this week).
- Output: fixed brief template:
  - What changed
  - Decisions and rationale
  - Progress vs plan
  - Risks/blockers
  - Next steps and asks
- Why first: this is the clearest recurring PM/consultant deliverable and easiest to evaluate.

### 2) Evidence-locked claims
- Every non-trivial claim must attach at least one citation marker (`[E1]`, `[E2]`).
- If no citation exists, the claim is marked **Assumption** (not Fact).
- Why first: prevents trust collapse from unsupported statements.

### 3) Evidence panel + traceability
- For each evidence item, show:
  - source app
  - exact origin (URL, ticket ID, permalink, file path)
  - timestamp
  - snippet excerpt
  - confidence level
- Clicking a citation opens source context.
- Why first: users must verify quickly before sharing.

### 4) Trust controls
- Confidence labels per claim: High/Medium/Low.
- Toggle to hide low-confidence claims.
- Toggle to show only directly evidenced claims.
- Why first: lets users calibrate risk before sending.

### 5) Noise filtering and deduplication
- Suppress low-signal capture noise (tab churn, repetitive switches, irrelevant windows).
- Merge duplicate evidence references.
- Why first: reduces edit burden and improves signal quality.

---

## Priority 1 (next): daily usability and retention

### 6) Pre-send QA check
- One-click validation for:
  - unsupported claims
  - missing owner/date in action items
  - stale or weak evidence
- Why: catches common quality issues right before distribution.

### 7) Reconstruction timeline
- Chronological strip of key events used in the brief.
- Why: speeds review and helps users rebuild narrative confidence.

### 8) Action extraction
- Auto-detect tasks, owners, due dates, blockers from captured content.
- Why: PM and consultant outputs are action-heavy; this increases practical utility.

### 9) Fast editing UX with citation preservation
- Inline edit for each section.
- Keep citation links intact after edits.
- Why: users will always edit; editing must be fast and safe.

### 10) Workflow-native export
- Export/copy formats for Slack, Notion/Confluence, and email.
- Preserve citation markers and evidence appendix in export.
- Why: output has value only if it ships cleanly into existing channels.

---

## Priority 2 (parallel to P0/P1): GTM-critical instrumentation

### 11) Outcome metrics instrumentation
- Track:
  - time-to-send-ready brief
  - percent of claims with evidence
  - edit distance from first draft to final output
  - weekly repeat usage
- Why: validates if product is truly reducing cognitive and reporting load.

### 12) Trust feedback capture
- After generation ask: "Would you send this as-is?"
- Capture reason when "no" (missing evidence, wrong emphasis, too noisy, etc.).
- Why: gives direct signal on trust blockers.

### 13) Use-case tagging
- Tag each output (weekly update, decision memo, roadmap check-in).
- Why: identifies strongest repeat workflow and informs focused GTM.

---

## Deprioritize for now

- Generic "AI summary for everyone" messaging and feature paths.
- Student/researcher-first workflows.
- Advanced proactive overlays before trust loop is stable.
- Team collaboration/taxonomy layers before single-user loop is strong.
- Broad multi-persona templates.

---

## 14-day build sequence

### Days 1-3
- Stakeholder Brief Mode with fixed template.

### Days 4-6
- Evidence-locked claims and evidence appendix.

### Days 7-8
- Confidence labels and Fact vs Assumption enforcement.

### Days 9-10
- Noise filtering/dedup and reconstruction timeline.

### Days 11-12
- Pre-send QA and action extraction.

### Days 13-14
- Export flows and outcome instrumentation.

---

## Release criteria (ship gate)

- Median draft-to-send-ready time is under 10 minutes.
- At least 80% of material claims have evidence references.
- At least 60% of pilot users report they would send with light/no edits.
- Weekly repeat usage is observed without manual prompting.

---

## Scope guardrails

- Build for one wedge first: PM/consultant deliverables.
- Optimize for trustworthy send-ready output, not generic summarization novelty.
- If a feature does not improve trust, speed to deliverable, or repeat usage, it is not a near-term priority.
