# Priority 0 Implementation Plan (PM/Consultant Wedge)

This plan operationalizes `features-new.md` Priority 0 into concrete product slices and maps to the implemented code paths.

## Scope

- Stakeholder Brief Mode (single golden workflow)
- Evidence-locked claims
- Evidence panel + traceability
- Trust controls
- Noise filtering + deduplication

## Build Plan and Status

### 1) Stakeholder Brief Mode

- **Plan**
  - Add a first-class `stakeholder_brief` action type.
  - Force fixed output template: What changed, Decisions and rationale, Progress vs plan, Risks/blockers, Next steps and asks.
  - Add time-window selector in action UI (`Selected session`, `Last 24 hours`, `This week`) and pass it as generation guidance.
- **Status**: Implemented.

### 2) Evidence-locked Claims

- **Plan**
  - Enforce citation markers (`[E#]`) post-generation for stakeholder briefs.
  - Add explicit `(Assumption)` marker when a claim lacks citation.
  - Add confidence labels (`[High]`, `[Medium]`, `[Low]`) per claim.
- **Status**: Implemented via action post-processor.

### 3) Evidence Panel + Traceability

- **Plan**
  - Enrich source refs with app, origin, timestamp, snippet, confidence.
  - Append an Evidence Appendix for stakeholder briefs.
  - Render evidence in a dedicated panel and make `[E#]` clickable in output.
- **Status**: Implemented in backend generation + frontend evidence panel.

### 4) Trust Controls

- **Plan**
  - Add output toggles to hide low-confidence claims.
  - Add output toggle to show only directly evidenced claims.
- **Status**: Implemented in action output viewer.

### 5) Noise Filtering + Deduplication

- **Plan**
  - Filter low-signal source entries before evidence construction.
  - Deduplicate source/evidence references before appendix generation.
- **Status**: Implemented in source extraction and evidence assembly.

## Validation

- Rust build: `cargo check` passes.
- UI build: `npm run build` passes.

