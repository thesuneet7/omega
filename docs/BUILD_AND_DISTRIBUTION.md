# Omega — Build, Packaging, and Distribution Plan

This document describes how the **shipped Omega desktop product** is built, what artifacts users receive, how they install and run the app, **where user data lives on disk**, and **what cloud services (if any)** you might use — including that **AWS or Azure are not required** for a local-first MVP. It assumes feature work is complete and the focus is **release engineering** and **distribution**.

It is aligned with the current codebase: a **Rust** backend (`omega-api` HTTP server + `sensor_layer` capture pipeline), a **React + Vite** UI in `ui/`, and an **Electron** shell that starts the API and loads the UI.

---

## 1. What “the actual app” is

Omega is a **native desktop application**, not a hosted web app users visit in a browser.

| Layer | Role |
| ----- | ---- |
| **Electron** | Windowing, system integration (tray, shortcuts later), packaged lifecycle, auto-update hook |
| **Embedded web UI** | Built static assets (`ui/dist/`) loaded from `file://` or equivalent in production |
| **Rust binaries** | `omega-api` (local API the UI calls), capture/sensor logic; run as **child processes** or sidecar binaries next to the app |
| **Local data** | SQLite, logs, session artifacts — stored on disk under a user data directory (not in the installer bundle) |

**Product shape:** one installable **per operating system** (macOS app bundle, Windows installer, optional Linux package). Users **download** an installer or archive from your website, GitHub Releases, or (later) app stores — same pattern as Slack, VS Code, Notion desktop, etc.

---

## 2. Tech stack (current + shipping additions)

### Already in the repo

| Area | Choice |
| ---- | ------ |
| Backend / capture | Rust 2021 (`sensor_layer` crate), `axum`, `tokio`, `rusqlite`, etc. |
| Desktop shell | Electron (see `ui/package.json`, `ui/electron/main.cjs`) |
| UI | React 18, TypeScript, Vite |
| Dev workflow | `cargo build`, `npm run build` in `ui/`, `electron:dev` spawning Vite + Electron |

### What you add to ship

| Area | Typical choice | Purpose |
| ---- | -------------- | ------- |
| **Packaging** | [electron-builder](https://www.electron.build/) or [Electron Forge](https://www.electronforge.io/) | Produce `.dmg` / `.pkg`, `.exe` (NSIS/Squirrel), `.AppImage`/`.deb`, embed `dist/` + Rust binaries |
| **Rust in release** | `cargo build --release` for each target triple | Small, fast `omega-api` (and any other bins you ship) |
| **Signing (macOS)** | Apple Developer ID Application certificate + **notarization** + (optional) stapling | Gatekeeper acceptance on end-user Macs |
| **Signing (Windows)** | Authenticode (EV cert recommended for SmartScreen reputation) | Fewer “unknown publisher” warnings |
| **Updates** | [electron-updater](https://www.electron.build/auto-update) (pairs well with electron-builder) | Background updates from your update server or GitHub Releases |
| **CI** | GitHub Actions, GitLab CI, or similar | Matrix builds: `macos-latest`, `windows-latest`, optional `ubuntu-latest` |
| **Secrets** | CI secret store | Certificates, API keys for notary, not for embedding Gemini keys in the binary — user/env still preferred for AI keys |

**Embedding AI keys in the client is discouraged** for a consumer product; keep keys in user config or OS keychain and document that in the installer README.

---

## 3. Artifact formats (what people download)

Rough standard by platform:

| Platform | Common formats | Notes |
| -------- | --------------- | ----- |
| **macOS** | `.dmg` (drag-to-Applications) or `.zip` containing `.app` | Prefer signed + notarized `.dmg` for simplest UX |
| **Windows** | `.exe` (NSIS or Squirrel) or **MSIX** | NSIS is common for Electron; MSIX if you need Store or enterprise policies |
| **Linux** | `.AppImage`, `.deb`, or Flatpak | Pick one primary; AppImage is portable, deb fits Ubuntu/Debian |

**Architecture:** ship **arm64** and **x64** where relevant (Apple Silicon vs Intel Mac; Windows arm64 optional). Electron-builder can produce universal macOS builds or separate artifacts per arch.

**Inside the package (conceptually):**

- `Omega.app` / `Omega.exe` + Electron resources
- `resources/` (or equivalent): **release** `omega-api` binary for that OS/arch (and `sensor_layer` if you ship it separately)
- `ui/dist/` static files

Today, `ui/electron/main.cjs` resolves `omega-api` from `target/debug/` — **for production you must resolve the binary from the packaged app path** (for example `process.resourcesPath` when `app.isPackaged` is true) and ship **release** builds.

---

## 4. How users get and use Omega

1. **Download** — From a marketing site or GitHub Releases: choose OS + arch, download the installer.
2. **Install** — Run the installer or drag the app to Applications (macOS). First launch may trigger **Screen Recording** and **Accessibility** prompts on macOS (and similar on other OSes) because capture requires OS permissions.
3. **Configure** — Optional: paste API key, set exclusions, data location (if you expose it).
4. **Run** — Electron starts, spawns `omega-api`, UI loads; user works as today in development, but with no manual `cargo`/`npm` steps.

**Updates:** With `electron-updater`, the app checks a URL you control (or GitHub Releases); users get delta or full updates with minimal friction after you publish a new version.

---

## 5. User data storage (local-first)

Omega’s default architecture is **local-first**: session content, embeddings, and UI state live on the user’s machine. Nothing in the core design **requires** you to run databases or object storage on AWS, Azure, or any other cloud for the product to work.

### 5.1 What gets stored today (aligned with the codebase)

| Kind of data | Typical location | Mechanism |
| ------------ | ---------------- | --------- |
| **Capture logs (JSON)** | Under `OMEGA_APP_LOGS_DIR` (defaults to `logs/`) | Files like `capture-session-<unix_ts>.json` |
| **App / UI state** | `<logs_dir>/app_state.db` | SQLite (`rusqlite`) — sessions list, summaries, revisions, exclusions state, etc. |
| **Pipeline / embeddings DB** | Path from `OMEGA_PHASE2_DB_PATH` / `OMEGA_PHASE3_DB_PATH` / `OMEGA_PHASE4_DB_PATH` (defaults often `logs/phase2.db`) | SQLite — chunks, embeddings, stitching, Phase 4 outputs |
| **API runtime session** | `omega-api` can create a per-run DB such as `runtime-phase2-<epoch>.db` under the logs dir | Isolates one “runtime” from older files |
| **Usage metering (local)** | Same tree as `app_state` / usage module | Stored locally for session-scoped usage tracking |

**Secrets (API keys)** — Gemini/OpenAI keys are expected via **environment** or `.env` on the machine, not shipped inside your installer. For production, plan to store keys in the **OS keychain** or a small encrypted preferences file and inject them into the process environment when `omega-api` starts (Electron can set `env` for the child process, as it already does for `OMEGA_API_PORT`).

### 5.2 Where the folder should live in production

During development, `logs/` next to the repo is fine. For a **shipped** app, point **`OMEGA_APP_LOGS_DIR`** (and derived paths) at the OS **application data** directory so data survives updates and is not written next to the read-only `.app` bundle:

| OS | Conventional root (Electron) | Example |
| -- | ---------------------------- | ------- |
| **macOS** | `app.getPath('userData')` | `~/Library/Application Support/<YourAppName>/` |
| **Windows** | `app.getPath('userData')` | `%APPDATA%\<YourAppName>\` |
| **Linux** | `app.getPath('userData')` | `~/.config/<YourAppName>/` |

**Implementation note:** Set `OMEGA_APP_LOGS_DIR` (and matching `OMEGA_PHASE*_DB_PATH` if you want a single tree) when spawning `omega-api` from Electron — same pattern as port injection in `ui/electron/main.cjs`.

### 5.3 Backups, deletion, and encryption

| Topic | Recommendation |
| ----- | ---------------- |
| **User backups** | Document that data lives under userData; users can rely on **Time Machine**, **File History**, or full-disk backup. Optional: in-app “Export data folder” zip. |
| **Delete all** | You already have product direction for “delete session / delete all” — implement as **delete SQLite files + JSON logs** under the configured directory. |
| **Encryption at rest** | Optional: OS full-disk encryption (FileVault, BitLocker) is usually enough for a desktop tool. **SQLCipher** or app-level encryption adds complexity; only consider if enterprise customers require it. |
| **Syncing across devices** | Not required for MVP. If you add it later, that becomes a **sync/backup product decision** (see §6) with clear privacy review. |

---

## 6. External services and cloud (AWS, Azure, and what is actually required)

### 6.1 Required for the core product (no AWS/Azure needed)

| Need | How it is satisfied without your cloud |
| ---- | ---------------------------------------- |
| **LLM / embeddings** | User’s keys → **HTTPS directly to Google (Gemini)** or **OpenAI** (already in code paths). Traffic goes from the user’s machine to the vendor API, not through your servers. |
| **Desktop app delivery** | **GitHub Releases** or any static file host; no Azure/AWS **required**. |
| **Local API** | `omega-api` binds to **127.0.0.1** — no public server. |

So: **you can ship v1 with zero AWS or Azure accounts** if you are comfortable using GitHub (or another simple host) for downloads and optional update metadata.

### 6.2 Optional cloud — when you might add AWS, Azure, or GCP

Use cloud **only when you add a feature that needs centralized infrastructure**. None of these are mandatory to “finish the app” as a local desktop product.

| Capability | Role of cloud | Typical AWS examples | Typical Azure examples |
| ---------- | --------------- | --------------------- | --------------------- |
| **Hosting installers + update manifests** | Store `.dmg`/`.exe` and `latest.yml` / `app-update.yml` | **S3 + CloudFront** | **Blob Storage + CDN** |
| **Auto-update backend** | Same as above; electron-updater reads static files | S3 static website or minimal API | Static website on Storage, or Azure Functions |
| **Website / docs** | Marketing site, docs | S3/CloudFront, Amplify | Static Web Apps, App Service |
| **Crash / error reporting** | Optional telemetry | Third-party (**Sentry**) or CloudWatch via SDK | Application Insights, Sentry |
| **CI/CD** | Build signing, releases | CodeBuild, GitHub Actions runners | Azure Pipelines |
| **Account system (future)** | Login, license keys, team features | Cognito + API Gateway + DynamoDB | Entra ID + App Service + SQL/Cosmos |
| **Central sync / backup (future)** | Encrypted blob per user | S3 with KMS, optional presigned URLs | Blob + Key Vault |

**Choosing AWS vs Azure:** treat it as an **organizational and pricing** choice unless you depend on a specific service. For static hosting + CDN + release artifacts, either ecosystem is fine. Many small teams use **GitHub Releases only** for years before adding S3/Azure Blob.

### 6.3 What you still should pay for outside “cloud”

| Item | Purpose |
| ---- | ------- |
| **Domain + DNS** | Website and update URLs (`download.yourproduct.com`). |
| **Apple Developer Program** | Signing and notarization (macOS). |
| **Code signing cert (Windows)** | Authenticode EV or standard. |
| **Email** (optional) | Support and transactional mail if you add accounts later. |

**Typical dollar ranges** for the above (and for optional cloud/CI) are in **§12**.

### 6.4 Privacy stance

If user screen content and summaries stay **only on disk + outbound to the LLM provider** the user configured, your **privacy story stays simple**: you are not running a data lake of captures on AWS/Azure unless you choose to build that. If you later add cloud backup, add explicit **opt-in**, encryption, and a data processing agreement as needed.

---

## 7. End-to-end release pipeline (high level)

```
┌─────────────────┐     ┌──────────────────┐     ┌─────────────────┐
│  Version bump   │────▶│  CI: build UI    │────▶│  CI: cargo      │
│  (app + Cargo)  │     │  (vite build)    │     │  --release bins │
└─────────────────┘     └──────────────────┘     └────────┬────────┘
                                                          │
                        ┌───────────────────────────────────┘
                        ▼
              ┌─────────────────────┐     ┌──────────────────────┐
              │  electron-builder   │────▶│  Sign + notarize     │
              │  (assemble package) │     │  (macOS/Windows)     │
              └─────────────────────┘     └──────────┬───────────┘
                                                     │
                        ┌────────────────────────────┘
                        ▼
              ┌─────────────────────┐     ┌──────────────────────┐
              │  Upload artifacts   │────▶│  Publish release     │
              │  (S3/GitHub/etc.)   │     │  + optional auto-    │
              └─────────────────────┘     │    update manifest   │
                                          └──────────────────────┘
```

---

## 8. What you need to set up (checklist)

### Developer machines

- Rust toolchain (`rustup`), targets for cross-compilation if you build Windows from Mac (often easier to build Windows **on** Windows in CI).
- Node.js LTS, `npm`/`pnpm` as you standardize.
- For local packaging tests: same OS as target, or CI-only builds.

### Apple (macOS distribution)

- **Apple Developer Program** membership.
- **Developer ID Application** certificate + exported `.p12` for CI.
- **App-specific password** or API key for **notarytool** (notarization).
- Entitlements plist if you need hardened runtime exceptions (screen capture APIs often need careful entitlement review).

### Microsoft (Windows distribution)

- **Code signing certificate** (Standard or EV). EV speeds up SmartScreen trust accumulation.
- Signing tool in CI (`signtool` or electron-builder’s built-in hooks).

### Hosting

- **Download hosting:** GitHub Releases (simplest), or S3/CloudFront, or both.
- **Update server:** static JSON + artifacts compatible with electron-updater, or a dedicated update service.

### Legal / product

- **Privacy policy** and **terms** (screen capture is sensitive).
- In-app disclosure of what is captured and where data lives (local-first story).

---

## 9. Gaps between today’s repo and a shippable build

| Gap | Action |
| --- | ------ |
| Debug-only binary path in Electron | Use `app.isPackaged` + `process.resourcesPath` (or `app.getPath('exe')` sibling) to locate `omega-api` in production |
| No `electron-builder` (or Forge) config | Add `electron-builder.yml` / `forge.config.js`, define `files`, `extraResources` for Rust binaries |
| Rust built as debug in scripts | CI and local `npm run release` should use `--release` and copy artifacts into the Electron bundle |
| No code signing | Integrate signing in CI; document secrets |
| No auto-update | Add `electron-updater` and publish URLs |
| API keys in `.env` | Document first-run setup; consider keychain storage for production |
| Data in `logs/` next to repo | Production Electron should set `OMEGA_APP_LOGS_DIR` to `app.getPath('userData')` (or subfolder) |

---

## 10. Optional directions (later)

- **Microsoft Store / Mac App Store** — stricter review, sandbox limits; often at odds with unrestricted screen capture; many tools distribute **outside** the stores first.
- **Enterprise:** MSI, managed deployment, offline installers, MDM-delivered configs.
- **Telemetry:** opt-in crash reporting (e.g. Sentry) with explicit consent.

---

## 11. Summary

| Question | Answer |
| -------- | ------ |
| **Format** | Per-OS installable: `.dmg`/`.app`, `.exe`/MSIX, Linux package — **not** a single “web download” of the whole product as a website-only app. |
| **How users download** | Website or GitHub Releases → installer → first-run permissions → use. |
| **Core stack** | **Rust** (backend/capture) + **Electron** + **React/Vite**; **electron-builder** (or Forge) + **signing** + **notarization** (macOS) + optional **electron-updater**. |
| **User data** | **Local-first:** SQLite under `OMEGA_APP_LOGS_DIR` (e.g. `app_state.db`, phase DBs, capture JSON). In production, point that env var at Electron **userData**. No AWS/Azure **required** for storage. |
| **Cloud (AWS / Azure)** | **Not required** for MVP. Optional for CDN + hosting installers/update feeds, CI, future accounts/sync — pick either cloud or stay on GitHub Releases + static hosting until you need more. |
| **What to set up** | Apple + Microsoft signing assets, CI matrix builds, hosting for binaries and update metadata (can be GitHub alone), privacy/compliance pages, packaging glue so `omega-api` ships as a **release** binary beside the Electron app, and production **userData** paths for logs/DBs. |
| **Rough cost** | See **§12** — **~$0–20/year** possible for Mac **without** Apple’s fee (**§12.7**); **~$110–130/year** once you add Apple + domain; more with Windows signing or paid services. |

---

## 12. Indicative costs (USD, order-of-magnitude)

**Disclaimer:** Figures are **typical retail-style ranges** for a solo developer or small team as of early 2026. Vendors change prices by region, tax, and promotions. Treat this as **budget planning**, not a quote. Verify on each vendor’s site before you buy.

### 12.1 One-time and annual — shipping the desktop app

| Item | Cost | Notes |
| ---- | ---- | ----- |
| **Rust, Node.js, Electron, Vite, React, electron-builder / Electron Forge** | **$0** | Open-source tooling; no license fee for building the app. |
| **Apple Developer Program** | **~$99 / year** | Required for **Developer ID** signing + **notarization** (smooth installs for most users). **Not required** to *distribute* a `.app`/`.dmg` for early testers — see **§12.7**. |
| **Windows code signing (Standard / OV)** | **~$200–400 / year** | Often cheaper than EV; may show more SmartScreen friction early in the cert’s life. Shop Sectigo, DigiCert resellers, etc. |
| **Windows code signing (EV)** | **~$300–600+ / year** | Typical EV list/discounted pricing varies by CA and term; often **hardware token/USB or HSM** required (one-time **~$50–150** or included). Helps **SmartScreen** reputation faster. |
| **Domain name** | **~$10–20 / year** | `.com` / `.app` etc.; transfers and premium names cost more. |
| **DNS** | **$0** | Often included with registrar or Cloudflare free tier. |

**Rough fixed “get serious about Mac + Windows desktop” budget:** about **$400–800 / year** (Apple + one Windows cert + domain), before cloud or CI paid tiers — or **~$110–130 / year** if you ship **macOS only** with Apple + domain — or **~$10–20 / year** (domain only) if you skip Apple for now (**§12.7**).

### 12.2 CI/CD and source hosting

| Item | Cost | Notes |
| ---- | ---- | ----- |
| **GitHub — public repo + Actions** | **$0** for many small projects | Free minutes on hosted runners; large matrix builds or private repos may need a paid plan. |
| **GitHub Team / Enterprise** | **From ~$4 / user / month** (Team) | Private repos, more Actions minutes, org features — check current [GitHub pricing](https://github.com/pricing). |
| **GitLab / Bitbucket / Azure DevOps** | **$0–20+ / user / month** | Depends on tier; comparable to GitHub for small teams. |
| **Self-hosted runner** | **Hardware + power** | One-time Mac mini for notarization/signing if you avoid cloud Mac minutes. |

### 12.3 Hosting downloads and updates (optional)

| Item | Cost | Notes |
| ---- | ---- | ----- |
| **GitHub Releases** (installers hosted as release assets) | **$0** | Fits many indie apps until traffic is huge. |
| **AWS S3 + CloudFront** | **~$1–50+ / month** | Pennies to a few dollars at low traffic; scales with egress and requests. **Free tier** may cover early experiments. |
| **Azure Blob Storage + CDN** | **Similar to S3** | Same ballpark for static files and installers. |
| **Marketing site only** (static) | **~$0–20 / month** | GitHub Pages, Netlify, Vercel hobby tiers, or S3 static hosting. |

### 12.4 Observability, email, and “later” cloud (optional)

| Item | Cost | Notes |
| ---- | ---- | ----- |
| **Sentry** (crash reporting) | **$0** on free tier; **~$26+ / month** team plans | Scales with events; verify [sentry.io pricing](https://sentry.io/pricing/). |
| **Application Insights / CloudWatch** | **Low $ or free tier** | Pay per ingest if you wire AWS/Azure telemetry. |
| **Transactional email** (SendGrid, Postmark, SES) | **~$0–15 / month** at low volume | Free tiers exist; needed if you add accounts or magic links later. |
| **Google Workspace / Microsoft 365** (support@ address) | **~$6–15 / user / month** | Optional; not required for the app binary. |
| **Future: auth + API + DB** (Cognito, API Gateway, DynamoDB, App Service, Cosmos, etc.) | **~$10–500+ / month** | Highly variable; **$0** until you build those features. |

### 12.5 APIs used by the app (usually not “your” fixed cost)

| Item | Who pays | Notes |
| ---- | -------- | ----- |
| **Gemini / OpenAI** (embeddings, summaries) | **End user** (BYO key) or **you** if you subsidize | Usage-based; **not** a flat platform fee. Your doc already assumes keys in `.env` / user config. |
| **Your own backend** (if you add one later) | **You** | Only if you proxy or centralize API calls. |

### 12.6 Example annual totals (illustrative)

| Scenario | What’s included | Ballpark / year |
| -------- | ----------------- | ----------------- |
| **Minimal indie** | macOS + Windows signing, domain, GitHub free, GitHub Releases | **~$500–900** |
| **macOS only (signed + notarized)** | Apple + domain | **~$110–130** |
| **macOS early access, no Apple fee yet** | Unsigned/ad-hoc build on GitHub Releases + domain (optional) | **~$0–20** |
| **Indie + paid GitHub + Sentry + small AWS** | Above + Team, light CDN, crash reporting | **~$1,000–2,500+** |
| **Heavy cloud product** | Auth, database, sync, high egress | **Thousands / month** — only if you build those features |

### 12.7 macOS downloads without the $99 Apple Developer Program (for now)

You **can** put a `.dmg`, `.zip`, or `.app` on **GitHub Releases**, your website, or a shared drive **without** joining the Apple Developer Program. **Cost: $0** from Apple for that path.

What you **do not** get without the paid program:

| You skip | Effect |
| -------- | ------ |
| **Developer ID Application signing** | The app is **not** signed with a cert that macOS trusts for “identified developer” flows. |
| **Notarization** | Apple does not scan and stamp the build; **Gatekeeper** is stricter on first open. |

What users see (varies by macOS version):

- A warning such as **“can’t be opened because it is from an unidentified developer”** or **“Apple cannot verify…”**.
- They can usually still run it by **Control‑click → Open** (or **Open** in **System Settings → Privacy & Security** after a block). This is normal for early betas; document it in your README.

**Practical guidance:**

- Fine for **friends, design partners, and many B2B pilots** if you give them a one-line “how to open” note and they trust you.
- **Not** ideal for mass consumer distribution: friction, support burden, and some orgs block unsigned software.
- When you close B2B deals or go wider, budget the **$99/year** for Developer ID + notarization — that is when installs feel “normal” and IT departments are happier.

**Do not** tell users to turn Gatekeeper off globally; per-app first open is the right pattern.

**Auto-update:** `electron-updater` can still serve updates over HTTPS from GitHub Releases without Apple’s fee; the **downloaded update binaries** will have the same signed/unsigned behavior as your first install.

---

This plan should be revisited when you add new native binaries, change the API port strategy, introduce auto-update, or add cloud sync — each affects entitlements, firewall prompts, data residency, CI caching, and **ongoing cost**.

---

*Living document — adjust as packaging tooling, vendor pricing, and store policies evolve.*
