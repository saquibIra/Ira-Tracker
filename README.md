# Ira Tracker

A lightweight time-tracking system for Ira engineers: a menu-bar/tray widget records which project each engineer works on and for how long, and the server reconciles that time against projects and customers for billing.

- **Time server:** [solidtime](https://www.solidtime.io) (open source, self-hosted in Docker), the system of record.
- **Widget ("Ira Tracker"):** Tauri 2 tray app (Rust + TypeScript), built here.
- **Reconciliation service:** Python FastAPI service for allocation checks, gap detection and billing exports (planned).

Design principles: *confirm, don't type*; invisible when idle; running timer by default; fix it later in an end-of-day review. **No surveillance:** no screenshots, keystrokes, mouse tracking, window titles or URLs are captured.

## Repository layout

```
.
├── widget/              Tauri tray app (Ira Tracker)
│   ├── src/             TypeScript UI (quick switcher, sign-in)
│   └── src-tauri/       Rust core: tray, SQLite queue, sync, keychain
├── server/
│   ├── solidtime/       Docker Compose for self-hosted solidtime
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
2. **Widget**
   ```bash
   cd widget && npm install && npm run tauri dev
   ```
   Sign in with a solidtime personal API token (solidtime → Profile → API Tokens). Then press **⌘⇧T** (Cmd+Shift+T) to switch projects.

Developer checklist: [widget/README.md](widget/README.md). Full manual test plan (engineer + admin): [docs/MANUAL_TEST_PLAN.md](docs/MANUAL_TEST_PLAN.md).

## Status

| Area | Status |
|------|--------|
| solidtime self-hosted locally + demo data | Done |
| Widget: sign-in, tray state, recent list, hotkey switcher, descriptions, offline queue + sync, idle/lock/sleep prompt (W1–W7, W9, W10) | Done (macOS tested in dev; Windows built in CI) |
| Widget: end-of-day review, reminders (W8, W11) | Next |
| Widget: signed builds, auto-update (W15) | Needs code-signing certificates |
| Reconciliation service (R1–R6, reports) | Planned |
| Smart suggestions: repo/branch, ticket keys, calendar (W12–W14) | Phase 2 |

Full requirement-by-requirement check: [docs/STATUS.md](docs/STATUS.md).

## Tests

```bash
cd widget && npm test                      # UI unit tests
cd widget/src-tauri && cargo test          # Rust unit tests
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
