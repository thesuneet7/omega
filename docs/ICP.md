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

### 4. Tech corporate employee (individual contributor)

**What's right:**

- The daily context-switching profile is a near-perfect architectural match. An IC at a company like AmEx, JPMorgan, or Google bounces between GitHub, Jira, Confluence, Slack, email, and internal docs all day — multiple sources, no single "home" app, no natural moment of documentation.
- The pain is real and daily: "What did I actually do today?" is a question most knowledge workers dread at 5pm. Stand-ups, 1:1s, weekly status emails, and async handoffs all require reconstructing a messy day from memory.
- Use cases multiply quickly: end-of-day summary for next morning's stand-up, quick context-share with a teammate picking up a task, prep for a manager 1:1, input for a performance review cycle, or a personal log for when a past decision gets questioned in a meeting.
- Frequency of use is extremely high — this is a daily-driver workflow, not an episodic one.
- Local-first is a particularly strong selling point in regulated industries (finance, healthcare, insurance) where cloud tools that process internal work artifacts carry real compliance risk. A screen-capture agent that never sends data off-device is a harder blocker to object to than a SaaS upload flow.

**What to sharpen:**

- The primary output here is a **work log / daily activity summary**, not a research artifact — a different (though equally valid) JTBD from the consultant's "research session → memo" workflow. Omega should be explicitly tested for whether its summarization pipeline performs on execution-oriented activity (code reviews, ticket triage, Slack threads, PR diffs) as well as it does on browser research sessions.
- Individual willingness to pay is lower than for consultants. The more realistic monetization path is team or enterprise licensing — a manager buying for an entire eng team once they see the output quality.
- The "transparent overlay / what to do next" proactive nudge feature is less relevant here. The value proposition is simpler and purer: **passive capture → end-of-day structured summary you didn't have to write.**

**Verdict:** Strong secondary ICP. High frequency of use, strong architectural fit, clear daily pain, and a local-first angle that lands well in enterprise settings. Should be validated alongside the consultant/PM wedge rather than deprioritized.

---

### 5. Students

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

**C. Tech IC / corporate knowledge worker**

- Spends the day jumping across GitHub, Jira, Confluence, Slack, documentation, and internal tools — constant context-switching, rarely pausing to write anything down.
- Needs a quick daily summary for stand-ups, 1:1s, status updates, and async handoffs with teammates picking up their work.
- Often works in industries (finance, insurance, healthcare tech) where local-first is not a nice-to-have but a compliance advantage over cloud-based tools.
- May not pay personally but is a strong candidate for team or enterprise licensing — especially once a manager or tech lead sees the output quality and recognises it solves a team-wide problem, not just an individual one.
- Pain: "I worked all day and now I have to remember what I did for the stand-up tomorrow — and explain a decision I made three weeks ago in a meeting today."

All three share the same underlying engine: **passive capture of a multi-app session → structured, shareable summary you didn't have to write.** The difference is the session type — research for A and B, execution/work log for C.

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
| Tech IC / corporate employee | **High** | Medium (individual) / High (team/enterprise) | **Very high (daily)** | Strong secondary |
| Journalists | Medium | Medium | Medium | Later vertical |
| Students | Medium | Low | High | Growth channel only |

The differentiated “transparent on-screen what to do next” idea is strongest for consultant/analyst workflows (clear investigative threads). It is weaker for students (more scattered) and researchers (need citation-level precision, not quick nudges).

---

*Living document — align with Phase 0 in ROADMAP and revise as the wedge is validated.*

**See also:** [GTM — first region, channels, and how to approach customers](GTM_REGION_CHANNELS_AND_FIRST_CUSTOMERS.md) (operationalizes geography, buyer motion, and distribution vs. this ICP).

---

## PM ICP card (execution-ready)

Use this card to keep PM-focused discovery and roadmap decisions grounded in one concrete user, one recurring trigger, and one first deliverable.

### Persona

**Role:** Mid-level product manager (or PM-adjacent UXR/Product Ops) at a startup or mid-size product company.

**Environment:** macOS-heavy, remote-first or hybrid, works across Slack, Jira, Confluence/Notion, Figma, dashboards, docs, and call notes.

**Reality:** Runs 2-4 hours of fragmented research/alignment work, then has to produce a clear stakeholder narrative fast.

### Core job to be done

"Help me turn scattered product context into a send-ready stakeholder update or decision brief in about 10 minutes, without losing evidence."

### Daily problems (high-frequency pain)

- Context is split across too many tools and tabs.
- Decisions lose traceability ("why did we choose this?" becomes hard to answer later).
- Stakeholder updates require repeated manual reconstruction.
- Ambiguous inputs must be converted into crisp requirements and action owners.
- End-of-day/week reporting consumes high cognitive load.

### Recurring trigger events

- Weekly product status update due.
- Stakeholder asks "what changed and why?" before a review.
- PM needs to justify a prioritization decision.
- Team needs a pre-read before planning/retro.

### First deliverable to nail (P0)

**Deliverable:** Weekly stakeholder brief (send-ready).

**Why this first:** High frequency, obvious value, easy quality judgment, and directly tied to retention ("I use Omega before every update").

### Deliverable structure (default template)

1. **What changed this week** (3-5 bullets)
2. **Decisions made and rationale** (each claim citation-linked)
3. **Progress vs plan** (on track/off track with reasons)
4. **Top risks/blockers** (owner + mitigation)
5. **Next 7 days** (commitments and asks)
6. **Evidence appendix** (E1, E2, E3... with source metadata)

### Citation and trust rules for PM outputs

- Every non-obvious claim must have at least one evidence reference.
- Citation should include source app, exact origin (URL/ticket/message), timestamp, and supporting snippet.
- If evidence is weak/missing, mark item as **Assumption** instead of **Fact**.
- Show confidence level per claim (High/Medium/Low).

### Product scope for PM wedge (what to build now vs later)

**Now:**
- One-click "Stakeholder Brief" mode.
- Source-locked citations and evidence appendix.
- Strong filtering/deduping for noisy activity.
- Re-open context loop: click citation -> jump to original source.

**Later:**
- Broader role packs and team taxonomy layers.
- Advanced overlays and proactive suggestion systems.
- Expansion workflows for tech IC daily logs.

### Success metrics (pilot scorecard)

- Median time from "generate" to send-ready brief.
- Percent of output claims with evidence references.
- User trust score ("I would forward this without heavy edits").
- Weekly active usage tied to real update rituals.
- Edit distance from generated draft to final sent version.

### Interview and validation prompts

- "Walk me through the last weekly update you sent. How long did reconstruction take?"
- "Which parts of this generated brief would you delete or rewrite first?"
- "Would you forward this to leadership as-is? If not, what blocks trust?"
- "What missing evidence would you need to defend this in a review?"

### Anti-goals for this ICP phase

- Do not optimize for "generic personal summary" positioning.
- Do not broaden messaging to students/general productivity users.
- Do not prioritize team-wide rollout before single-user trust is stable.
