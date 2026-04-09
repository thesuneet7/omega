# Just release — minimal cost (cut everything optional)

This is a **tight “ship it” budget**: you want **downloads for users**, a **simple website**, and **many people able to use the product without you paying for servers** — while **spending as little as possible**. Numbers are **USD**, order-of-magnitude, early 2026.

For full context (signing, notarization, cloud options), see [BUILD_AND_DISTRIBUTION.md](BUILD_AND_DISTRIBUTION.md). To **charge users** (tiers, subscriptions) and what that costs *you*, see [PAYMENTS_AND_PRICING_SETUP.md](PAYMENTS_AND_PRICING_SETUP.md).

---

## What “smooth for multiple users” means here

| Piece | How you keep it cheap |
| ----- | --------------------- |
| **App usage** | Omega is **local-first**: each user runs the app on their machine; you **do not** need a backend or database in the cloud for them to use the product. |
| **Downloads** | Host installers on **GitHub Releases** — free for public repos, fine for many users downloading. |
| **Website** | Static site on **GitHub Pages**, **Cloudflare Pages**, or **Netlify** free tier — enough for “what is this + download + docs”. |
| **Support** | A **mailto**, **GitHub Issues**, or a free Discord — no paid helpdesk required to start. |

**Smoothness tradeoff:** The **lowest** cash cost usually means **no** paid **Apple** ($99) or **Windows code signing** (hundreds/year). Users still get the app; on first open they may need **Control‑click → Open** (Mac) or similar (Windows). Document that in your README — that is the main “non-smooth” part, not your server bill (there is none).

---

## Target: $0 / year (cash to third parties)

You can realistically launch at **zero recurring vendor spend** if you accept:

- **GitHub** public repo + **GitHub Releases** + **GitHub Pages** (or only Releases + a minimal README landing).
- **No custom domain** — URL looks like `https://<user>.github.io/<repo>/` or you link straight to `github.com/<org>/<repo>/releases`.
- **macOS:** **unsigned / not notarized** builds (see §12.7 in BUILD_AND_DISTRIBUTION.md).
- **Windows (if you ship it):** unsigned or self-signed — more SmartScreen friction; many indies ship Mac-only first to avoid cert cost.
- **No** paid analytics, **no** paid crash tool (optional later: Sentry free tier).

**Your time** is not $0 — but **cash out the door** can be **$0**.

---

## Target: ~$10–20 / year (one small upgrade)

| Item | Cost | Why bother |
| ---- | ---- | ---------- |
| **Custom domain** | **~$10–20 / year** | `yourproduct.com` looks credible on a landing page and in email; point DNS to GitHub Pages / Netlify / Cloudflare. |

Everything else can stay on free tiers above.

---

## Optional: smoother installs (later, when revenue allows)

These are **not** required to “just release,” but they reduce support and IT friction:

| Item | ~Cost | When |
| ---- | ----- | ---- |
| **Apple Developer Program** | **$99 / year** | Mac installs without Gatekeeper drama; notarization. |
| **Windows code signing (OV or EV)** | **~$200–600 / year** | If you ship Windows to non-technical buyers. |

---

## One-page budget summary

| Scenario | Annual cash (approx.) | What you get |
| -------- | ---------------------- | -------------- |
| **Bare minimum** | **$0** | GitHub Releases + free static site + unsigned Mac (and/or unsigned Win) + local-first app; many users, no hosting bill. |
| **Credible web presence** | **~$10–20** | Custom domain + same stack. |
| **Polished desktop trust** | **+$99** (Mac) and/or **+$200–600** (Win) | Signed/notarized Mac; signed Windows — add when B2B or scale warrants it. |

---

## Checklist (minimal launch)

- [ ] Build release binaries + package (e.g. electron-builder when ready).
- [ ] Upload artifacts to **GitHub Releases** (tag + release notes).
- [ ] One-page site: what it does, download link, how to open on Mac if unsigned, privacy stance, contact / Issues.
- [ ] No cloud **required** for user data — aligns with local-first architecture in BUILD_AND_DISTRIBUTION.md.

---

*This doc is only about **your** spend to distribute and present the product — not end-user API bills (Gemini/OpenAI BYO keys remain user or separate policy).*
