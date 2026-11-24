# Time Pilot

Time tracking that keeps up with your team. Organizations manage projects and people on the Time Pilot web platform; each person runs the Time Pilot desktop app, which recognises which project they work on and tracks the time by itself. At the end of the day everyone checks their time and submits it.

- **Web platform** (`server/web`): organizations, projects, clients, members, time, reports, and the Super Admin panel. It is a fork of [solidtime](https://www.solidtime.io) (AGPL-3.0) with the Time Pilot brand and onboarding, built as the official image plus a small, checked set of changes.
- **Desktop app** (`widget`): Tauri 2 tray app for macOS and Windows (Rust + TypeScript). Sign-in happens in the browser (OAuth 2.0 with PKCE).
- **Autotrack service** (`server/autotrack`): FastAPI service for AI auto-tracking (Gemini or Claude decides which project a window belongs to when keywords and what the app has learned are not enough), the auto-tracking settings page and day submissions.

**Privacy.** Auto-tracking is opt-in, with a consent screen. The app reads the front window's app and title locally and takes a small grayscale screenshot every few minutes while tracking. Screenshots stay on the person's own computer for a few days for their end-of-day review. Only when the window title is not enough is a screenshot sent to the Autotrack service for classification; the server never stores it. No keystrokes or mouse activity are ever captured. Password managers and personal messengers are excluded, and people can pause at any time.

## How people get in

1. The **Super Admin** creates an organization and enters its admin's email (`/admin` → Organizations → **New organization**).
2. The **admin** gets an email, sets a password, adds projects and invites the team under **Members**.
3. **Employees** get an invitation email, set a password, install the desktop app from `/download/` and click **Sign in**: the browser opens, they sign in, and the app is ready.

Sign-up is by invitation only.

## Repository layout

```
.
├── widget/              Time Pilot desktop app (Tauri 2)
│   ├── src/             TypeScript UI (welcome/sign-in, switcher, consent, review)
│   └── src-tauri/       Rust core: tray, SQLite queue, sync, OAuth sign-in, auto-tracking
├── server/
│   ├── web/             Web platform: solidtime fork (Dockerfile, overlay/, patches.mjs)
│   ├── autotrack/       FastAPI: AI auto-tracking, settings page, day submissions
│   ├── local/           Local stack: web + Postgres + Mailpit + Autotrack
│   └── seed/            Demo data loader
├── deploy/linux/        Production on a Linux server with Docker
├── tests/e2e/           Playwright end-to-end test of the onboarding
├── scripts/             Mac release script, brand asset generator
├── docs/                Status, deployment, install guide, test plan
└── CLAUDE.md            Working notes for Claude Code
```

## Quick start (local development, macOS)

Prerequisites: Docker Desktop, Node 22, Rust (`rustup`), Xcode command-line tools (or Visual Studio Build Tools + WebView2 on Windows).

1. **Platform** — see [server/local/readme.md](server/local/readme.md) for the first run (keys, Super Admin, sign-in clients). Then:
   ```bash
   cd server/local && docker compose up -d --build   # web http://localhost:8000, mail http://localhost:8025
   ```
2. **Desktop app**
   ```bash
   cd widget && npm install && npm run tauri dev
   ```
   Click **Sign in**: the browser opens the local sign-in page; sign in with an account you created through an invitation (emails appear in Mailpit). On macOS, allow Screen Recording for the dev build when asked.

## Tests

```bash
cd widget && npm test                         # UI unit tests
cd widget/src-tauri && cargo test             # Rust unit tests (incl. OAuth/PKCE)
cd server/autotrack && .venv/bin/python -m pytest -q
docker build server/web                       # web fork: patches, type check, build
cd tests/e2e && npx playwright test           # whole onboarding against the local stack
```

CI (`.github/workflows/ci.yml`) runs formatting, lint, tests and dependency audits on macOS and Windows, and builds the web fork.

## Production

[docs/DEPLOY_LINUX.md](docs/DEPLOY_LINUX.md) covers the server (https://tracker.aasnatech.com): install, email, Super Admin, backups. [docs/INSTALL.md](docs/INSTALL.md) is the guide for employees. Requirement status: [docs/STATUS.md](docs/STATUS.md).

## Licence

The web platform in `server/web` is a modified version of solidtime and is licensed under the **GNU AGPL-3.0**, like solidtime. Everyone who uses the platform can get its source code from the **Source code** link on the sign-in page and in the app.

## Branches

`main` (released) ← `staging` (release candidate) ← `develop` (integration) ← `feature/*`.
