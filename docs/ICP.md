# Omega — Ideal Customer Profile (ICP)

This document captures positioning analysis: what Omega is, how to evaluate ICP candidates, critiques of proposed segments, recommended primary ICP, and a one-sentence Phase 0 statement aligned with [ROADMAP.md](ROADMAP.md).

---

## What Omega actually is (the lens for evaluating ICPs)

Omega is a **passive screen-activity capture + semantic stitching + summarization engine**, running local-first. Its superpower is reconstructing what you were doing and thinking across a multi-app research session — automatically, without you annotating everything.

That means the ideal user has:

1. Long, multi-source research sessions with no single “home” app
2. A deliverable at the end of that session (a memo, a report, a decision)
3. A real felt pain of “I lost the thread of what I found”
4. Professional-grade willingness to pay
5. A reason to care about local-first privacy

---

## Critique of proposed ICPs

### 1. Researchers (academia, papers)

**What’s right:** The inter-paper context stitching idea is compelling, and the “what to do next” need is real.

**The problem:**

- Omega captures screen OCR from live activity. Researchers’ primary input is PDFs — not general screen browsing. OCR of a PDF viewer gives fragmented, noisy text, not a clean document.
- Their standard is exceptionally high. “Extremely accurate conclusions” from LLM summaries is a credibility landmine — if Omega hallucinates a citation or misattributes a finding, a researcher’s work is damaged. One bad output can destroy trust.
- Tools like NotebookLM, Elicit, and Consensus already attack this workflow with purpose-built document ingestion. Omega’s screen-capture approach is architecturally at a disadvantage here.
- Researchers often work on institutional machines with IT policies that make a screen-capture agent a hard sell.

**Verdict:** High standards, low forgiveness, architectural mismatch. Not the primary ICP.

---

### 2. Consultants (Junior/Mid)

**What’s right:** This is the strongest candidate by a wide margin.

- Consultants spend much of the day in the workflow Omega is built for: browser research across many tabs, PDFs, decks, competitor sites, client portals — all in one messy session.
- They have a hard deliverable (slide, memo, model) that forces synthesis.
- Local-first is a genuine selling point: client data confidentiality matters at serious firms.
- “Internet rabbit hole → neatly arranged context” maps closely to the pipeline: capture → stitch → structured summary.
- The transparent overlay suggestion idea (“search this next to know about this”) is differentiated — passive proactive nudges tied to session context.
- They have expense accounts or firm tool budgets. Willingness to pay is real.

**What to sharpen:**

- “Junior/Mid” matters: juniors do the most grinding research but have least budget authority. Mid-level (e.g. Manager/Associate) is often the sweet spot — still deep research, more budget, and sharper pain from longer, more complex sessions.
- The ICP is more precisely: **solo knowledge workers doing deep, session-based internet research with a client deliverable at the end** — including independent consultants, strategy analysts, and market research professionals, not only Big 4 employees.

**Verdict:** Keep this. It’s the wedge.

---

### 3. Journalists

**What’s right:** Similar workflow to consultants — lots of browser research and cross-referencing articles.

**The problem:**

- Journalists have a sharper privacy concern. A screen-capture agent while working on an embargoed story or sensitive source is a real professional risk. Even with local-first, optics are hard.
- The bottleneck for many journalists is less *research synthesis* and more *writing* — editorial tools, CMS, established workflows. Omega doesn’t touch the writing phase.
- Smaller addressable market than consultants, harder to reach (no centralized procurement).
- Investigative journalists (who might benefit most) are often the most privacy-paranoid.

**Verdict:** Secondary or later vertical. Not primary ICP.

---

### 4. Students

**What’s right:** High frequency of use, clear workflow alignment, good early-adopter pool for virality.

**The problem:** “Great user, terrible customer” risk.

- Students often won’t pay, or pay very little — optimizing for them can pull the product in the wrong direction.
- Workflows are episodic (semester-based). Retention can be structurally low.
- Obsidian, Notion, NotebookLM, and ChatGPT already occupy this mental model. Switching cost is low.

**Verdict:** Not a revenue ICP. Can be a growth/distribution channel (referrals, virality) but should not drive product decisions.

---

## Recommended primary ICP (additions)

Based on product architecture and the JTBD order in the roadmap:

**Primary ICP: The independent knowledge worker doing intensive session-based research.**

Two sub-profiles with nearly identical workflow:

**A. Mid-level strategy consultant / independent analyst**

- Spends 2–4 hour blocks researching across browser, PDFs, and data sites.
- Has a client deliverable (deck, memo, model) requiring synthesis of that session.
- Often on a MacBook in client-confidential contexts (local-first is a feature).
- Can pay personally or expense without heavy approval.
- Pain: “I spent hours researching and now I have to write it up, but I’ve already forgotten half of what I found.”

**B. Product manager / UX researcher**

- Constant context-switching: competitor analysis, user interviews, Jira, Figma, customer calls.
- Needs to reconstruct research for PRDs, stakeholder updates, retrospectives.
- Strong macOS user base, healthy tool budget.
- Pain: “I did hours of competitive research and now I face a blank doc.”

Both share the same core job: **turn a messy multi-hour research session into a structured, shareable artifact — automatically.**

---

## One-sentence ICP (Phase 0 — ROADMAP)

Roadmap Phase 0 asks for a one-sentence ICP. Candidate:

> **Omega is for mid-level knowledge workers — consultants, analysts, and PMs — who spend hours doing multi-source research and need to reconstruct what they found without manually taking notes.**

That sentence maps capture (multi-source session), stitching (reconstruct), summary (what they found), and passive use (without manual notes).

---

## Summary table

| ICP | Fit with architecture | Willingness to pay | Frequency of use | Priority |
|-----|------------------------|--------------------|------------------|----------|
| Researchers | Low (PDF-first, not screen-first) | Medium | High | No |
| Consultants / analysts | **High** | **High** | **High** | **Primary** |
| PMs / UX researchers | High | High | High | Primary (co-equal) |
| Journalists | Medium | Medium | Medium | Later vertical |
| Students | Medium | Low | High | Growth channel only |

The differentiated “transparent on-screen what to do next” idea is strongest for consultant/analyst workflows (clear investigative threads). It is weaker for students (more scattered) and researchers (need citation-level precision, not quick nudges).

---

*Living document — align with Phase 0 in ROADMAP and revise as the wedge is validated.*
