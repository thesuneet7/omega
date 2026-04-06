# Omega API Cost Estimation

This document estimates API cost per user per month for the current Omega pipeline and recommends product pricing plans.

Assumptions are based on the current implementation defaults in:
- `src/phase2.rs` (embeddings)
- `src/phase3.rs` (local stitching)
- `src/phase4.rs` (bucket summarization)

---

## 1) What creates API cost

### Paid API stages
- **Phase 2 (embeddings):** one embedding request per chunk.
- **Phase 4 (summarization):** one LLM generation request per bucket summary.

### No API cost stage
- **Phase 3 (stitching):** local SQLite/vector math only.

### Important scaling behavior
- Cost scales with **chunks** and **bucket summaries**, not directly with raw screenshot count.
- Dedup logic in Phase 2 prevents re-paying for identical chunk hashes.
- Phase 4 skips unchanged bucket summaries unless forced.

---

## 2) Pricing inputs used for estimation

Reference model prices (paid tier, standard):
- **Gemini Embedding (`gemini-embedding-001`)**: `$0.15 / 1M input tokens`
- **Gemini 2.5 Flash-Lite (`gemini-2.5-flash-lite`)**:
  - Input: `$0.10 / 1M tokens`
  - Output: `$0.40 / 1M tokens`

Token conversion assumption for OCR text:
- `~4 characters = 1 token` (rough planning approximation).

---

## 3) User behavior assumptions

For an average consulting/analysis user:
- `16 work days / month`
- `~3 hours of active research / day`
- `~48 research hours / month`

For a high-intensity power user:
- `22 work days / month`
- `~6 hours of active research / day`
- `~132 research hours / month`

---

## 4) Monthly API cost estimates

## 4.1 Average user estimate (recommended planning baseline)

Usage model:
- `~1,500 embedded chunks / month`
- `~400 input tokens / chunk` in Phase 2
- `~80 bucket summaries / month`
- Each summary call: `~15,000 input + 1,500 output tokens`

Cost math:
- **Phase 2 embeddings**
  - Tokens: `1,500 * 400 = 600,000`
  - Cost: `0.6M * $0.15 = $0.09`
- **Phase 4 summaries input**
  - Tokens: `80 * 15,000 = 1,200,000`
  - Cost: `1.2M * $0.10 = $0.12`
- **Phase 4 summaries output**
  - Tokens: `80 * 1,500 = 120,000`
  - Cost: `0.12M * $0.40 = $0.048`

**Average monthly API cost per user: `~$0.26` (round to `$0.30`)**

---

## 4.2 Maximum expected cost (heavy but realistic)

Usage model:
- `~10,000 embedded chunks / month`
- `~500 input tokens / chunk`
- `~500 bucket summaries / month`
- Each summary call: `~30,000 input + 3,000 output tokens`

Cost math:
- **Phase 2 embeddings**
  - Tokens: `10,000 * 500 = 5,000,000`
  - Cost: `5.0M * $0.15 = $0.75`
- **Phase 4 summaries input**
  - Tokens: `500 * 30,000 = 15,000,000`
  - Cost: `15.0M * $0.10 = $1.50`
- **Phase 4 summaries output**
  - Tokens: `500 * 3,000 = 1,500,000`
  - Cost: `1.5M * $0.40 = $0.60`

**Maximum expected monthly API cost per heavy user: `~$2.85` (round to `$3.00`)**

---

## 4.3 Stress/upper-bound scenario (rare)

Usage model (edge case):
- `~20,000 chunks / month`
- `~800 summaries / month`
- Summary prompts near max input window frequently

Estimated API cost range:
- **`~$5 to $9 / month / user`**

This should be treated as an operational edge case, not normal user behavior.

---

## 5) Other key statistics to track

To keep estimates accurate and avoid margin surprises, track these per user/month:
- `embedded_chunks_count`
- `embedded_input_tokens_total`
- `summary_calls_count`
- `summary_input_tokens_total`
- `summary_output_tokens_total`
- `summaries_skipped_unchanged_count`
- `bucket_input_truncated_count`

Suggested finance monitoring metrics:
- `p50_api_cost_per_user`
- `p90_api_cost_per_user`
- `p99_api_cost_per_user`
- `gross_margin_after_api`

---

## 6) Is this feasible?

Yes. At current architecture and model defaults, API cost is low enough for healthy SaaS margins:
- Average user: around `$0.30/month`
- Heavy user: around `$3/month`
- Rare stress cases: still usually below `$10/month`

The current design is cost-efficient because:
- embeddings are cheap,
- summarization happens per bucket (not per capture),
- unchanged summaries are skipped,
- and Phase 3 is local compute.

---

## 7) Recommended pricing plans

These plans are designed for strong margin while keeping UX simple.

## Starter (individual)
- **Price:** `$9/month`
- Includes:
  - Up to `120 research hours/month`
  - Standard summarization speed
  - Session history + editor
- Internal target API budget: `< $1/user/month`

## Pro (power user)
- **Price:** `$19/month`
- Includes:
  - Up to `300 research hours/month`
  - Priority summarization queue
  - Advanced exports/templates
- Internal target API budget: `< $3/user/month`

## Team
- **Price:** `$39/user/month`
- Includes:
  - Everything in Pro
  - Team admin + policy controls
  - Shared standards/integrations
- Internal target API budget: `< $4/user/month`

## Fair use policy (recommended)
- Soft limits on extreme automated/high-frequency usage.
- If usage repeatedly exceeds Pro envelope, move customer to custom enterprise metered plan.

## Optional enterprise metered add-on
- Base seat + metered overage.
- Example overage:
  - `$0.25 per additional 1M summary input tokens`
  - `$0.90 per additional 1M summary output tokens`
  - (Priced above raw API cost to preserve support and infra margin.)

---

## 8) Quick decision summary

- **Planning baseline:** use `~$0.30 API cost / avg user / month`.
- **Conservative heavy-user reserve:** use `$3.00 / user / month`.
- **Pricing recommendation:** launch at `$9 / $19 / $39` tiers.
- **Margin posture:** strong, if token telemetry and fair-use guardrails are in place.
