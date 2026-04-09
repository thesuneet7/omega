# Payments & pricing — what it costs *you* to get paid

This doc covers **accepting money** from users (one-time, subscriptions, tiers) and the **ongoing fees** you pay processors — not your customers’ prices. For **hosting the app binary** and a minimal site, see [JUST_RELEASE_MINIMAL_COST.md](JUST_RELEASE_MINIMAL_COST.md) and [BUILD_AND_DISTRIBUTION.md](BUILD_AND_DISTRIBUTION.md).

**TL;DR:** Setting up payments is usually **$0 upfront** (no monthly fee on common plans). You mainly pay **a percentage of each sale** plus sometimes a **fixed per-transaction fee**. Optional tools (license server, email) can stay on free tiers at small scale.

---

## 1. What you need in practice

| Need | Purpose |
| ---- | ------- |
| **Payment processor** | Charges cards / PayPal; sends you payouts. |
| **Checkout** | Page or embed where the user picks a plan and pays. |
| **Entitlement** | Something the **app** checks: license key, signed JWT, or “active subscription” from an API — so only paying users unlock paid features. |
| **Payouts** | Money hits your **bank account** (or PayPal); processor does KYC/identity verification — usually **free** to set up. |

Omega is **local-first**; you do **not** need a big cloud stack to “host users.” You may add a **tiny** backend or use a platform that handles **license keys + webhooks** for you.

---

## 2. Two common approaches (cost vs. work)

### A) Merchant of Record (MoR) — simplest tax/compliance for global sales

**Examples:** [Lemon Squeezy](https://www.lemonsqueezy.com/pricing), [Paddle](https://www.paddle.com) (verify current pricing on their sites).

| Aspect | Typical pattern |
| ------ | ---------------- |
| **Your monthly fee** | Often **$0** for core e-commerce (you pay only when you sell). |
| **Per sale** | Lemon Squeezy publishes **5% + $0.50** per transaction (US; see their [pricing](https://www.lemonsqueezy.com/pricing) and [fee docs](https://docs.lemonsqueezy.com/help/getting-started/fees) for edge cases). Paddle uses a similar MoR model; **confirm their current rate**. |
| **What they handle** | VAT/sales tax collection and filing in many jurisdictions, checkout, often **license keys**, subscriptions, refunds. |
| **Best when** | You want **minimum** legal/tax homework and are okay with a **higher** fee per sale than raw Stripe. |

### B) Stripe (or similar) — lower % per charge, more is on you

**Example:** [Stripe](https://stripe.com/pricing) — standard US online card payments are commonly **2.9% + $0.30** per successful charge (international and extra products cost more; see their pricing page).

| Aspect | Typical pattern |
| ------ | ---------------- |
| **Your monthly fee** | **$0** for standard pay-as-you-go. |
| **Per sale** | Lower than most MoR **all-in** rates, **but** you must handle **sales tax / VAT** where required (or buy a tax tool like Stripe Tax with added cost). |
| **What you build** | Checkout (Stripe Checkout or Payment Links), **webhooks** to your server or serverless function to issue **license keys** or unlock accounts. |
| **Best when** | You’re mostly one region at first, or you’re ready to integrate tax tools / accountants later. |

**Rough comparison (illustrative only):** On a **$50** sale, **~$1.75** to Lemon Squeezy (5% + $0.50) vs **~$1.75** to Stripe (2.9% + $0.30) — close at that price point; at **$10**, fixed fees hurt more (MoR’s $0.50 is a larger slice). Model your tiers in a spreadsheet.

---

## 3. Other options (indie-friendly)

| Option | Fee idea | Notes |
| ------ | -------- | ----- |
| **Gumroad** | Percentage per sale + may have platform fee tiers | Very simple for digital goods; check [gumroad.com](https://gumroad.com). |
| **PayPal** (direct) | ~transaction fees | Possible for manual invoicing; worse UX for subscriptions and license automation unless paired with something else. |

Always read the **current** pricing page before you commit.

---

## 4. “Setting everything up” — fixed costs (your wallet)

| Item | Typical cost |
| ---- | ------------ |
| **Stripe / Lemon Squeezy / Paddle account** | **$0** to open; you complete KYC. |
| **Bank account** | **$0** (you already have one); payouts go there. |
| **Business entity** | **$0–500+** one-time depending on country (sole prop vs LLC, etc.) — **not** a processor fee; ask a local accountant if you’re unsure. |
| **Website with pricing** | **$0** (free static host) + optional **~$10–20/year** domain — same as [JUST_RELEASE_MINIMAL_COST.md](JUST_RELEASE_MINIMAL_COST.md). |
| **License validation in the app** | **$0** if you validate via **MoR license API** or a **small free-tier** serverless worker (e.g. Cloudflare Workers, Vercel, AWS Lambda) for webhooks — **only if** you don’t put secrets in the client. |
| **Email receipts** | Usually **included** in MoR/Stripe customer emails. Marketing email may be extra. |

There is **no** universal “$X/month just to turn on pricing” on standard Stripe or Lemon Squeezy e-commerce — you pay when money moves.

---

## 5. Ongoing costs you *can’t* avoid (per sale)

- **Processor / MoR take** — the main cost (percent + fixed).
- **Chargebacks** — Stripe often charges a **dispute fee** (e.g. **$15** if a chargeback occurs — refunded if you win; verify [Stripe disputes](https://stripe.com/docs/disputes)). MoR platforms may handle customer-facing billing differently.
- **Refunds** — usually you return the net to the customer; fees may be partially non-refundable depending on provider — read their docs.

---

## 6. Pricing models for users (product-side, not fee-side)

| Model | Fits desktop well? | Notes |
| ----- | ------------------- | ----- |
| **One-time purchase** | Yes | Simple: pay once → license key. |
| **Annual subscription** | Yes | MoR and Stripe Billing both support recurring; app checks subscription status or license period. |
| **Per-seat / team** | Later | Needs clearer account model; still doable with MoR variants or Stripe. |
| **Free tier + paid** | Yes | Free = no payment; paid = same stack as above. |

---

## 7. Minimal engineering path (aligned with low spend)

1. Pick **Lemon Squeezy or Paddle** if you want **tax + license keys** with less backend work **or** **Stripe + Stripe Tax** (or manual tax at first) if you optimize for lower %.
2. Create products: **Pro**, **Team**, etc., with prices and (if needed) **trial periods**.
3. Link checkout from your **website** (embed or redirect).
4. In the **Electron app**, add a **license field** (or login) that calls **your** validation (MoR API, or your small endpoint that checks Stripe subscription).
5. **Never** ship processor **secret keys** in the desktop app — only **publishable** keys or server-side validation.

---

## 8. Summary table

| Question | Answer |
| -------- | ------ |
| **Upfront cost to start accepting payments?** | Usually **$0** (account + KYC). |
| **Monthly platform fee?** | Often **$0** on common Stripe / Lemon Squeezy e-commerce tiers. |
| **Real cost** | **Per transaction** (e.g. **~2.9% + $0.30** Stripe US vs **5% + $0.50** Lemon Squeezy — illustrative; **verify live**). |
| **AWS / Azure required?** | **No** for basics; optional tiny serverless for webhooks. |
| **Need a lawyer/accountant?** | Recommended before high volume or multi-country; not a “Stripe line item.” |

---

*Fees and features change — confirm numbers on each provider’s site before you launch pricing pages.*
