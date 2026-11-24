# Ira Tracker

A lightweight time-tracking system for Ira engineers: a menu-bar/tray widget records which project each engineer works on and for how long, and the server reconciles that time against projects and customers for billing.

- **Time server:** [solidtime](https://www.solidtime.io) (open source, self-hosted in Docker), the system of record.
- **Widget ("Ira Tracker"):** Tauri 2 tray app (Rust + TypeScript), built here.
- **Ira service:** Python FastAPI service (`server/ira-service`). It runs AI auto-tracking (Claude classifies which project the screen belongs to), the project hints admin page and day submissions. Reconciliation (allocation checks, gap detection, billing exports) will be added here.

Design principles: *confirm, don't type*; invisible when idle; the timer follows your work automatically; fix it later in an end-of-day review.

**Privacy.** Auto-tracking is opt-in, with a consent screen. It reads the front window's app and title locally. While tracking it takes a small grayscale screenshot every few minutes (work hours only). Screenshots stay on the engineer's own computer for a few days so they can check and fix their day in the review, and are then deleted. Only when the window title is not enough is a screenshot sent to the Ira service for classification; the server never stores it. No keystrokes or mouse activity are ever captured. Password managers and personal messengers are excluded, and engineers can pause it at any time for private time.

## Repository layout

```
.
├── widget/              Tauri tray app (Ira Tracker)
│   ├── src/             TypeScript UI (quick switcher, sign-in)
│   └── src-tauri/       Rust core: tray, SQLite queue, sync, keychain
├── deploy/linux/        Production on any Linux server with Docker (current: client server)
├── deploy/gcp/          Alternative: one Google Cloud VM (Docker, HTTPS, Gemini, logs, backups)
├── scripts/             Release scripts (Mac installer)
├── server/
│   ├── solidtime/       Docker Compose for self-hosted solidtime
│   ├── ira-service/     FastAPI: AI auto-tracking, hints admin, day submissions
│   └── seed/            Demo data loader
├── docs/                Requirements status and decisions
└── CLAUDE.md            Working notes for Claude Code
```

## Quick start (local development, macOS)

Prerequisites: Docker Desktop, Node 20+, Rust (`rustup`), Xcode command-line tools (macOS) or Visual Studio Build Tools + WebView2 (Windows).

1. **Server** — first-time setup steps are in [server/README.md](server/README.md). After that:
   ```bash
   cd server/solidtime && docker compose up -d     # http://localhost:8000
   ```
2. **Ira service** (AI auto-tracking): put your Anthropic API key in `server/ira-service/.env` (copy `.env.example`), then:
   ```bash
   cd server/ira-service && uv run --env-file .env uvicorn --factory ira_service.main:create_app --port 8010
   ```
   Admin page: http://localhost:8010/admin (project descriptions and keywords, AI cost, day submissions). Details: [server/ira-service/README.md](server/ira-service/README.md).
3. **Widget**
   ```bash
   cd widget && npm install && npm run tauri dev
   ```
   Sign in with a solidtime personal API token (solidtime → Profile → API Tokens). The auto-tracking window opens next: read it, enter the Ira service address (`http://localhost:8010`) and turn it on. On macOS, allow Ira Tracker under System Settings → Privacy & Security → Screen Recording, then restart the app. **⌘⇧T** (Cmd+Shift+T) still switches projects by hand.

Developer checklist: [widget/README.md](widget/README.md). Full manual test plan (engineer + admin): [docs/MANUAL_TEST_PLAN.md](docs/MANUAL_TEST_PLAN.md).

## Status

| Area | Status |
|------|--------|
| solidtime self-hosted locally + demo data | Done |
| Widget: sign-in, tray state, recent list, hotkey switcher, descriptions, offline queue + sync, idle/lock/sleep prompt (W1–W7, W9, W10) | Done (macOS tested in dev; Windows built in CI) |
| AI auto-tracking: timer follows the work (window context, keywords, Claude on screenshots), consent, pause, excluded apps (AT1–AT8) | Done (live Claude test pending API key) |
| Widget: end-of-day review with Submit (W8) | Done |
| Widget: reminders (W11) | Next |
| Widget: signed builds, auto-update (W15) | Needs code-signing certificates |
| Reconciliation (R1–R6, reports) in the Ira service | Planned |
| Ticket keys into descriptions, calendar (W13, W14) | Phase 2 |

Full requirement-by-requirement check: [docs/STATUS.md](docs/STATUS.md).

**Production:** [docs/DEPLOY_LINUX.md](docs/DEPLOY_LINUX.md) covers the current deployment on the client server (https://time.iratechnologies.com). [docs/DEPLOY_GCP.md](docs/DEPLOY_GCP.md) is the alternative Google Cloud setup. [docs/INSTALL.md](docs/INSTALL.md) is the install guide for employees.

## Tests

```bash
cd widget && npm test                      # UI unit tests
cd widget/src-tauri && cargo test          # Rust unit tests
cd server/ira-service && uv run pytest     # Ira service tests (Claude and solidtime are faked)
```

CI (`.github/workflows/ci.yml`) runs formatting, lint, tests and dependency audits on macOS and Windows for every push and pull request.

## Branches

| Branch | Purpose |
|--------|---------|
| `main` | Released, production-ready code. Only merged from `staging`. |
| `staging` | Release candidate, tested by pilot users. Merged from `develop`. |
| `develop` | Integration branch; all feature branches merge here. |
| `feature/<name>` | Day-to-day work, branched from `develop`, merged back via pull request. |

Flow: `feature/*` → `develop` → `staging` → `main`. Hotfixes: `hotfix/<name>` from `main`, merged into `main` and `develop`.
