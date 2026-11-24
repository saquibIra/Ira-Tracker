# CLAUDE.md

Working notes for Claude Code on the Ira Tracker project. Read README.md for the overview and docs/STATUS.md for the compliance matrix (IDs W1–W15, R1–R6 come from the requirements doc; AT1–AT8 are the AI auto-tracking change agreed with Arvind on 19 Nov 2026).

**Scope rule:** the requirements document (Arvind, 31 Oct 2026) is the source of truth, with one agreed change: AI auto-tracking from window context and screenshots (19 Nov 2026), which replaces the doc's "no screenshots / window titles" constraint. Build what they ask for; before adding anything else, check it against the document and say so. Engineering quality (tests, security, CI, performance, migrations) is always in scope.

## What this is

- `widget/` — Tauri 2 tray app. Rust core (`src-tauri/src`) owns all state; the TypeScript UI (`src/main.ts`) only renders windows and calls commands.
- `server/solidtime/` — unmodified solidtime in Docker. We never fork or patch it (AGPL); integrate only via its REST API.
- `server/seed/` — demo data loader (stdlib-only Python).
- `deploy/linux/` — **current production** (client server, https://time.iratechnologies.com): `deploy.sh` (Mac → rsync over SSH, the server cannot reach GitHub) runs `install.sh` (secrets in `/opt/ira-data`, stack, migrations, admin, nightly `backup.sh`). Gemini via `GEMINI_API_KEY`. Guide: `docs/DEPLOY_LINUX.md`.
- `deploy/gcp/` — alternative: production on one Compute Engine VM: `setup.sh` (gcloud: service account, IP, firewall, snapshots, VM), `bootstrap.sh` (VM startup: Docker, secrets in `/opt/ira-data`, stack, migrations, admin), `update.sh`, `docker-compose.yml` (solidtime + Postgres + Ira service + Caddy HTTPS; `/ira/*` → Ira service, `/download/*` → installers; logs via gcplogs). Guide: `docs/DEPLOY_GCP.md`.
- `server/ira-service/` — FastAPI service for AI auto-tracking (AI provider `IRA_AI_PROVIDER`: `gemini` on Vertex AI in production, or `anthropic`; structured logs with request ids, `logs.py`): auth via the caller's solidtime token, policy, project keyword hints (admin page at `/admin`), `POST /v1/classify` (Claude), usage/cost log, day submissions (W8). Holds the only Anthropic key. The reconciliation jobs (R1–R6) will be added here too.

## Commands

```bash
# server
cd server/solidtime && docker compose up -d                 # http://localhost:8000
docker compose exec scheduler php artisan tinker --execute='...'   # inspect data

# widget
cd widget && npm run tauri dev        # run (tray app; no Dock icon)
IRA_IDLE_SECS=30 npm run tauri dev    # idle prompt after 30 s instead of 10 min (testing W6/W7)
IRA_AUTO_TICK_SECS=5 IRA_CAPTURE_SECS=30 npm run tauri dev   # faster auto-tracking checks / AI calls (testing)
cd widget && npm run build            # typecheck + build UI
cd widget && npm test                 # UI unit tests (vitest, src/*.test.ts)
cd widget/src-tauri && cargo test     # Rust unit tests
cd widget/src-tauri && cargo clippy --all-targets -- -D warnings
cd widget/src-tauri && cargo fmt      # format before committing
cd widget/src-tauri && cargo audit; cd widget && npm audit   # dependency vulnerabilities

# ira-service (needs server/ira-service/.env; see .env.example)
cd server/ira-service && uv run --env-file .env uvicorn --factory ira_service.main:create_app --port 8010
cd server/ira-service && uv run pytest -q && uv run ruff check . && uv run ruff format --check . && uv run pip-audit

# local widget DB
sqlite3 ~/Library/Application\ Support/com.ira.tracker/tracker.db
```

Rust is installed via rustup; run `source ~/.cargo/env` in a fresh shell if `cargo` is missing.

## Widget architecture

- `db.rs` — SQLite (WAL, synchronous=FULL). Schema changes go through `MIGRATIONS` (tracked in `PRAGMA user_version`): append a step, never edit a released one. Table `entry` is the offline queue and local source of truth. Every local change sets `dirty=1` and bumps `rev`; sync clears `dirty` only if `rev` is unchanged. Rows rejected by the server (4xx) get `error`/`error_rev` and are skipped until edited again. Starting a timer closes the running one in the same transaction (no overlaps); zero-length entries are dropped.
- Creates are duplicate-safe: `create_attempted` is set before POST; on retry the sync looks the entry up by exact start time before creating again.
- Migration 3 adds `entry.source` (`manual`/`auto`), `entry.context` (window fingerprint of an auto entry, local only) and `context_cache` (window fingerprint → project learned from the AI or the user's corrections; AI answers reused 4 h, user corrections 30 days; cleared on sign-out). An entry without a project is a "Needs review" block.
- `auto.rs` — AI auto-tracking. Every 20 s while enabled, in work hours, not paused/idle/away: front window → learned cache → admin keyword hints → (at most once per capture interval) screenshot + title to the Ira service. `Engine` is a pure state machine (unit-tested): hysteresis (`min_switch_secs`), switches backdated to when the work started (never before the running entry), unresolved/private windows become review blocks after `unresolved_review_secs`. A manual start/stop pins auto-switching for `pin_secs` and teaches nothing (people pick before switching windows). AI answers and review corrections are remembered only for specific window titles (`auto::specific`); generic ones such as "Zoom Meeting" are classified afresh. Policy comes from the server (`GET /v1/config`), cached in `kv`.
- `context.rs` / `capture.rs` — front app + window title (CoreGraphics window list on macOS, foreground window on Windows) and the screenshot (ScreenCaptureKit on macOS 14+, GDI on Windows): one display, downscaled by the OS, grayscale JPEG in memory only. Pure helpers (`target_size`, `bgra_to_luma`, `encode_jpeg`) are unit-tested; `cargo test live_ -- --ignored --nocapture` checks a real machine.
- `ai.rs` — Ira service client (policy, hints with ETag, multipart classify, day submit). `cargo test e2e_ -- --ignored` runs it against a live service.
- `review.rs` — W8 end-of-day review: day view with each block's screenshots (thumbnails, full viewer, how the project was decided), reassign/describe/delete, edit start/end (`db::set_entry_times` moves the adjacent block's boundary and refuses overlaps), split (`db::split_entry`), Submit to the Ira service; review reminder at `review_at`. `open -a "Ira Tracker" --args --review` opens it.
- `shots.rs` — kept screenshots: files + `capture` table (migration 4), ids validated (no path tricks), retention pruning, deleted on sign-out.
- Sync pushes rows refused by the server once more after the rest of the batch: a moved boundary changes two entries and solidtime refuses the temporary overlap.
- `applog.rs` — app log file `<app data>/logs/ira-tracker.log` (1 MB rotation) via the `log` crate; tray "Open logs folder". Never log titles, screenshots, descriptions or tokens.
- Build-time `VITE_IRA_SERVER_URL` pre-fills the server; the Ira service defaults to `<server>/ira` (`lib.ts` `defaultIraUrl`).
- `idle.rs` — W6/W7. `Detector` is a pure state machine (unit-tested); `monitor` feeds it the clock and the OS idle seconds (CoreGraphics on macOS, `GetLastInputInfo` on Windows). Sleep = wall-clock jump between checks.
- `input.rs` — validation at the command boundary (server URL must be HTTPS unless localhost; descriptions single-line, ≤ 500 chars).
- `src/lib.ts` — pure UI helpers (escaping, colour validation, fuzzy search), unit-tested; `src/main.ts` renders windows.
- `api.rs` — solidtime client. `ApiError::Offline` (network/5xx → retry), `Auth` (401), `Rejected` (other 4xx → don't retry unchanged).
- `lib.rs` — app state, tray menu (incl. Auto-tracking submenu and "Review today…"), commands (`get_state`, `start_timer`, `stop_timer`, `set_description`, `sign_in`, `sign_out`, `get_away`, `resolve_away`, plus `auto::*` and `review::*`), sync loop (≤ 1 sync/min), window management.
- Windows (`switcher`, `login`, `idle`, `auto`, `review`, `nudge`) are created on demand and destroyed on close/blur to keep idle memory low. The window label selects the view in `main.ts`.
- After any state change call `changed(app)`: refreshes tray, emits `state-changed` to open windows, kicks the sync loop.

## solidtime API facts (verified against v1 source)

- Auth: `Authorization: Bearer <personal access token>`. Base path `/api/v1`.
- `GET /users/me/memberships` → organization id and **member id** (time entries need `member_id`, not user id).
- `GET /organizations/{org}/projects|tasks|clients` (paginated, 500 per page; tasks accept `done=false|all`).
- `POST /organizations/{org}/time-entries` requires `member_id`, `start`, `billable`; optional `project_id`, `task_id`, `end`, `description`. A running timer is an entry with `end: null`.
- Datetimes must be UTC in exactly `YYYY-MM-DDTHH:MM:SSZ` (no milliseconds).
- `billable_rate` is an integer in cents.
- `GET /users/me/time-entries/active` returns the running entry.
- Org setting `prevent_overlapping_time_entries` is enabled by the seed script.

## Rules that must hold

- **Privacy (auto-tracking, agreed 19 Nov 2026):** never capture keystrokes or mouse activity. Window titles and screenshots are used only for auto-tracking, only after the user accepted the consent screen (`CONSENT_VERSION`), only in work hours and never while paused, idle, locked or in an excluded app. Screenshots (consent v2, 19 Nov 2026): once per capture interval while tracking, one display, downscaled grayscale JPEG. Kept **only on the engineer's computer** (`<app data>/captures/`, files 0600, folder 0700) for `keep_screenshots_days` (server policy, default 3, 0 = none) so the engineer can check their day in the review; deleted after that, on sign-out, or by the user. Sent to the Ira service only when the AI must classify; the service never stores them (it rejects requests that could spill to a temp file). Only the project and a short neutral activity note are kept; "non_work" results never put details into descriptions. The Anthropic key lives only on the Ira service.
- **Performance budget** (acceptance criteria): idle RSS < 50 MB, idle CPU < 0.5 %, startup < 2 s to tray, installer < 15 MB, network at most one batched sync per minute while running and nothing while stopped, no focus stealing (only user-invoked windows take focus).
- **Offline-first:** user actions write to SQLite first and return instantly; never block UI on the network.
- **Security:** keep the CSP in `tauri.conf.json` strict; build HTML only with `esc()`/`safeColor()` from `lib.ts`; SQL only with bound parameters; validate every command argument (`input.rs`, catalog checks).
- **Testing:** pure logic gets unit tests (Rust `#[cfg(test)]`, `src/*.test.ts`); keep I/O (OS, network, windows) in thin wrappers around testable functions. CI runs fmt, clippy, tests and audits on macOS and Windows.
- **Secrets:** this repo is public. Never commit `.env` (incl. `server/ira-service/.env` with `ANTHROPIC_API_KEY`), `laravel.env`, `.admin-credentials`, tokens or keys. Tokens live only in the OS keychain (service `com.ira.tracker`, account `solidtime-token`).

## Gotchas

- solidtime `APP_ENV` must stay `production`; `local` crashes on missing Telescope.
- In dev, every rebuild changes the binary signature, so macOS asks for keychain access again ("Always Allow").
- Auto-start registers only in release builds (`#[cfg(not(debug_assertions))]`) so dev runs don't install a LaunchAgent.
- Local release builds: sign with the local self-signed identity "Ira Tracker Local Signing" (`APPLE_SIGNING_IDENTITY="Ira Tracker Local Signing" npm run tauri build -- --bundles app`). macOS ties Screen Recording to the signature; ad-hoc builds change it every time and silently lose the permission. The identity lives only in this Mac's login keychain (not in the repo); real releases need Apple Developer ID signing (W15).
- Screen Recording (macOS) is granted per binary: in dev, grant it to the dev build (or the terminal/VS Code that runs it) and restart the app; until then titles are empty and screenshots fail (auto-tracking falls back to app names). Screenshots need macOS 14+; the bundle requires macOS 13+ (ScreenCaptureKit linkage).
- Only one widget instance runs (single-instance plugin); a second launch opens the switcher of the first. Stop dev before testing a release build.

## Git workflow

- Branches: `main` (released) ← `staging` (pilot/release candidate) ← `develop` (integration) ← `feature/<name>`.
- Work on a `feature/*` branch from `develop`; open a PR into `develop`. Never commit directly to `main` or `staging`.
- Commit messages: imperative, prefixed with the area, e.g. `widget: add idle detection (W6)`, `server: ...`, `docs: ...`. Reference requirement IDs where relevant.
- Before committing: `npm run build`, `npm test`, `cargo fmt`, `cargo clippy -- -D warnings` and `cargo test` must pass; update docs/STATUS.md when a requirement changes state.
