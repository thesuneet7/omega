# Omega — Feature Plan for ICP: Consultants, Analysts, PMs, UX Researchers

This document defines the feature set specifically for the target ICP established in [ICP.md](ICP.md). Every feature is evaluated against the core workflow of this user: **2–4 hour multi-source research sessions that must produce a structured, shareable deliverable**.

Cross-reference the build order with [ROADMAP.md](ROADMAP.md) — JTBD 1 (trust) must be solved before JTBD 3 (value) is credible.

---

## Current state (what is already built)


| Area                                                                | Status                                  |
| ------------------------------------------------------------------- | --------------------------------------- |
| Phase 1–4 pipeline (capture → embed → stitch → summarize)           | **Done**                                |
| Session list + summary editor + revision history (Rust API + React) | **Code exists, not wired into live UI** |
| Electron desktop shell                                              | **Done**                                |
| Export                                                              | **Not built**                           |
| App/URL exclusion list                                              | **Not built**                           |
| Cross-session search                                                | **Not built**                           |
| Project / client folders                                            | **Not built**                           |
| Source attribution in summaries                                     | **Not built**                           |
| Overlay / floating suggestion window                                | **Not built**                           |


The most important quick win: **SessionsPage already exists as dead code** — wiring it in (`main.tsx`) delivers sessions + editor + revisions with minimal new work.

---

## Feature themes

### Theme 1 — Privacy & trust

*Consultants handle client-confidential work. Without this layer, they will not run a screen-capture agent on their machine.*

### Theme 2 — Research session intelligence

*The core engine already captures and stitches — these features surface the results in ways directly useful to the ICP.*

### Theme 3 — Session organization

*Consultants think in engagements/projects. The flat session list today does not match their mental model.*

### Theme 4 — Output & export

*The deliverable moment: turning the session into something they can use in a memo, deck, or PRD.*

### Theme 5 — Active session assistance

*The differentiated feature: proactive, in-session nudges that no other tool offers.*

### Theme 6 — Integrations

*Later — only after single-player loop is proven.*

---

## Feature table


| #   | Feature                                     | Theme                 | Description                                                                                                                                                   | Why this ICP specifically needs it                                                                                                                                            | Priority | Sprint    | Est. effort |
| --- | ------------------------------------------- | --------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------- | --------- | ----------- |
| 1   | **Wire SessionsPage into live UI**          | Foundation            | Mount existing `SessionsPage.tsx` in `main.tsx` so sessions, editor, revisions, and pipeline buttons are live                                                 | Usable app shell; everything else depends on this                                                                                                                             | P0       | Sprint 1  | 0.5 days    |
| 2   | **App exclusion list**                      | Privacy               | User-configured list of apps (by name) that Omega will never capture (e.g. personal banking, internal confidential tools, Slack with clients)                 | Client confidentiality is non-negotiable; one screenshot of a confidential client doc = trust destroyed                                                                       | P0       | Sprint 1  | 2 days      |
| 3   | **Per-session privacy pause**               | Privacy               | System tray icon or keyboard shortcut to pause/resume capture instantly with visible status indicator                                                         | They frequently switch from research to confidential internal tools mid-session                                                                                               | P0       | Sprint 1  | 1 day       |
| 4   | **Data manifest + one-click delete**        | Privacy               | In-app view showing exactly what is stored (paths, sizes, retention), with delete-session and delete-all buttons                                              | Consultants need to prove to themselves (and potentially clients) that data stays local and is erasable                                                                       | P0       | Sprint 2  | 2 days      |
| 5   | **Source attribution in summaries**         | Research intelligence | Every summary includes which apps and window titles the insight came from (e.g. "from Chrome — Gartner report, Salesforce CRM docs, competitor pricing page") | Consultants must be able to back up every finding with a source; a summary without provenance is unusable in a client deliverable                                             | P0       | Sprint 2  | 3 days      |
| 6   | **Browser URL capture**                     | Research intelligence | Capture the active browser tab URL (not just OCR) as structured metadata alongside each screenshot                                                            | URLs are the ground truth of "where did this come from" — OCR alone gives text but not the source identity                                                                    | P0       | Sprint 3  | 3 days      |
| 7   | **Session naming + project/client folders** | Organization          | Auto-generate a human-readable session title from Phase 4 summary; allow user to rename and assign to a named project/client folder                           | Consultants manage multiple engagements; flat chronological list is unusable after week 2                                                                                     | P0       | Sprint 4  | 3 days      |
| 8   | **Export to Markdown**                      | Output                | One-click export of session summary (title, one-liner, findings, source list) as a clean `.md` file                                                           | First step to a shareable artifact; Markdown pastes cleanly into Notion, Confluence, GitHub, and most writing tools                                                           | P0       | Sprint 4  | 1.5 days    |
| 9   | **Quick summary trigger**                   | Research intelligence | Keyboard shortcut (e.g. ⌘⇧S) or always-visible button to generate "summarize what I've been researching for the last N hours" on demand                       | Consultants don't wait for a session to end; they want a mid-session checkpoint before a client call                                                                          | P1       | Sprint 5  | 2 days      |
| 10  | **Research brief output template**          | Output                | Structured summary output format optimized for ICP: Background → Key Findings → Open Questions → Sources → Suggested Next Steps                               | This maps directly to how consultants and PMs write memos and PRDs; generic summaries require heavy rewriting                                                                 | P1       | Sprint 5  | 2 days      |
| 11  | **Session tagging**                         | Organization          | User can tag sessions with type labels (e.g. Competitive Intel, User Research, Due Diligence, Market Sizing) and custom tags                                  | Enables filtering and retrieval by work type, not just by date or project                                                                                                     | P1       | Sprint 6  | 1.5 days    |
| 12  | **Cross-session search**                    | Organization          | Full-text and semantic search across all past sessions and summaries                                                                                          | "I researched this competitor three weeks ago — what did I find?" is a constant pain point                                                                                    | P1       | Sprint 6  | 4 days      |
| 13  | **Clipboard capture as intent signal**      | Research intelligence | Track what the user copies (URLs, text snippets, numbers) during a session as a high-value signal alongside OCR                                               | Copy behavior reveals what the user found important — it's the implicit highlight; improves Phase 3 stitching quality significantly for this ICP                              | P1       | Sprint 7  | 2 days      |
| 14  | **Export to PDF**                           | Output                | Generate a clean, print-ready PDF from the research brief output (with source list and session metadata)                                                      | Some stakeholders and clients require PDF; deck-builders need a formatted reference document                                                                                  | P1       | Sprint 7  | 2 days      |
| 15  | **"What I found" daily digest**             | Research intelligence | End-of-day (or on-demand) digest: a single structured document across all sessions that day, deduplicated and organized by topic                              | PMs and analysts frequently need to reconstruct "what did I do today" for standups, journals, or end-of-sprint retros                                                         | P1       | Sprint 8  | 3 days      |
| 16  | **Transparent overlay window**              | Active assistance     | Always-on-top semi-transparent floating panel (40% opacity, non-intrusive) showing real-time contextual suggestions while the user researches                 | **Core differentiator** — no other tool offers passive proactive nudges tied to your live session context; directly addresses the "I didn't know what to search next" problem | P2       | Sprint 9  | 5 days      |
| 17  | **"What to search next" nudges**            | Active assistance     | Overlay shows 2–3 contextual suggestions during active research: follow-up searches, related angles, gaps in current session context                          | Consultants and analysts often get stuck in one frame — this breaks the pattern and surfaces adjacent research directions they'd otherwise miss                               | P2       | Sprint 10 | 4 days      |
| 18  | **Circle / dead-end detection**             | Active assistance     | Detect when user is revisiting the same sources or queries repeatedly and show a gentle pivot suggestion in the overlay                                       | Saves significant time on rabbit holes; a strong signal for users doing deep research under time pressure                                                                     | P2       | Sprint 10 | 3 days      |
| 19  | **Notion / Confluence push**                | Integrations          | One-click push of research brief to a Notion page or Confluence article via API                                                                               | Analysts and PMs already live in these tools; removing the copy-paste step closes the loop to their existing workflow                                                         | P3       | Month 5+  | 5 days      |
| 20  | **Calendar context correlation**            | Integrations          | Show which calendar event (meeting, client call) a session preceded or followed, as context in the summary                                                    | Helps users reconstruct "this was my prep research before the Monday call with client X" — adds narrative context without manual annotation                                   | P3       | Month 6+  | 3 days      |


---

## Priority definitions


| Priority | Meaning                                                                        |
| -------- | ------------------------------------------------------------------------------ |
| **P0**   | Blocking — without this, the ICP will not adopt or trust the product           |
| **P1**   | Core value — differentiates Omega for this ICP vs. generic tools               |
| **P2**   | Differentiator — the unique capability that drives word-of-mouth and retention |
| **P3**   | Later — only after single-player loop is healthy and retention is proven       |


---

## Build timeline

```
Sprint 1  (Week 1–2)   Wire SessionsPage · App exclusion list · Privacy pause
Sprint 2  (Week 3–4)   Data manifest + delete · Source attribution in summaries
Sprint 3  (Week 5–6)   Browser URL capture · Summary UX polish
Sprint 4  (Week 7–8)   Session naming · Project/client folders · Export to Markdown
Sprint 5  (Week 9–10)  Quick summary trigger · Research brief output template
Sprint 6  (Week 11–12) Session tagging · Cross-session search (basic full-text)
Sprint 7  (Month 4)    Clipboard capture · Export to PDF
Sprint 8  (Month 4)    Daily digest
Sprint 9  (Month 5)    Overlay window foundation (always-on-top Electron shell)
Sprint 10 (Month 5)    Overlay content — "what to search next" nudges · Dead-end detection
Month 6+               Integrations (Notion, Confluence, Calendar)
```

---

## What NOT to build (non-goals for this ICP, next 6–12 months)

- Team sharing / multi-user sessions (single-player loop must be proven first)
- Mobile app (this ICP researches on a desktop/laptop, always)
- Voice / meeting transcription (separate product surface; adds trust complexity)
- Citation management for academic papers (wrong ICP — see researcher critique in ICP.md)
- Browser extension (useful but not differentiated; focus on the native desktop advantage)
- Real-time collaboration (premature; adds infra complexity with no validated demand)

---

## Metric per milestone (to validate each sprint pays off)


| Sprint  | Success signal                                                                                |
| ------- | --------------------------------------------------------------------------------------------- |
| 1–2     | User runs the app for a full research session without disabling it mid-way (trust proxy)      |
| 3–4     | User exports at least one summary and uses it in a deliverable (activation)                   |
| 5–6     | User returns the next day and opens a past session (D1 retention signal)                      |
| 7–8     | User has sessions organized across 2+ projects (depth of use)                                 |
| 9–10    | User mentions overlay nudge in feedback as useful (qualitative signal for the differentiator) |
| Month 6 | Weekly active use and at least one referral to a colleague                                    |


---

*Living document — align with ROADMAP.md phases and ICP.md positioning. Revisit after each sprint.*