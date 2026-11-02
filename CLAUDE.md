# CLAUDE.md

Working notes for Claude Code on the Ira Tracker project. Read README.md for the overview and docs/STATUS.md for the compliance matrix (IDs W1–W15, R1–R6 come from the requirements doc).

**Scope rule:** the requirements document (Arvind, 31 Oct 2026) is the source of truth. Build what it asks for; before adding anything else, check it against the document and say so. Engineering quality (tests, security, CI, performance, migrations) is always in scope.

## What this is

- `widget/` — Tauri 2 tray app. Rust core (`src-tauri/src`) owns all state; the TypeScript UI (`src/main.ts`) only renders windows and calls commands.
- `server/solidtime/` — unmodified solidtime in Docker. We never fork or patch it (AGPL); integrate only via its REST API.
- `server/seed/` — demo data loader (stdlib-only Python).
- Reconciliation service (FastAPI) — not started; will live in `server/reconciliation/`.

## Commands

```bash
# server
cd server/solidtime && docker compose up -d                 # http://localhost:8000
docker compose exec scheduler php artisan tinker --execute='...'   # inspect data

# widget
cd widget && npm run tauri dev        # run (tray app; no Dock icon)
IRA_IDLE_SECS=30 npm run tauri dev    # idle prompt after 30 s instead of 10 min (testing W6/W7)
cd widget && npm run build            # typecheck + build UI
cd widget && npm test                 # UI unit tests (vitest, src/*.test.ts)
cd widget/src-tauri && cargo test     # Rust unit tests
cd widget/src-tauri && cargo clippy --all-targets -- -D warnings
cd widget/src-tauri && cargo fmt      # format before committing
cd widget/src-tauri && cargo audit; cd widget && npm audit   # dependency vulnerabilities

# local widget DB
sqlite3 ~/Library/Application\ Support/com.ira.tracker/tracker.db
```

Rust is installed via rustup; run `source ~/.cargo/env` in a fresh shell if `cargo` is missing.

## Widget architecture

- `db.rs` — SQLite (WAL, synchronous=FULL). Schema changes go through `MIGRATIONS` (tracked in `PRAGMA user_version`): append a step, never edit a released one. Table `entry` is the offline queue and local source of truth. Every local change sets `dirty=1` and bumps `rev`; sync clears `dirty` only if `rev` is unchanged. Rows rejected by the server (4xx) get `error`/`error_rev` and are skipped until edited again. Starting a timer closes the running one in the same transaction (no overlaps); zero-length entries are dropped.
- Creates are duplicate-safe: `create_attempted` is set before POST; on retry the sync looks the entry up by exact start time before creating again.
- `idle.rs` — W6/W7. `Detector` is a pure state machine (unit-tested); `monitor` feeds it the clock and the OS idle seconds (CoreGraphics on macOS, `GetLastInputInfo` on Windows). Sleep = wall-clock jump between checks.
- `input.rs` — validation at the command boundary (server URL must be HTTPS unless localhost; descriptions single-line, ≤ 500 chars).
- `src/lib.ts` — pure UI helpers (escaping, colour validation, fuzzy search), unit-tested; `src/main.ts` renders windows.
- `api.rs` — solidtime client. `ApiError::Offline` (network/5xx → retry), `Auth` (401), `Rejected` (other 4xx → don't retry unchanged).
- `lib.rs` — app state, tray menu, commands (`get_state`, `start_timer`, `stop_timer`, `set_description`, `sign_in`, `sign_out`, `get_away`, `resolve_away`), sync loop (≤ 1 sync/min), window management.
- Windows (`switcher`, `login`, `idle`) are created on demand and destroyed on close/blur to keep idle memory low. The window label selects the view in `main.ts`.
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

- **Privacy:** never capture or send screenshots, keystrokes, mouse activity, window titles or URLs. Repo/branch detection (Phase 2) stays local; only the accepted project + description go to the server.
- **Performance budget** (acceptance criteria): idle RSS < 50 MB, idle CPU < 0.5 %, startup < 2 s to tray, installer < 15 MB, network at most one batched sync per minute while running and nothing while stopped, no focus stealing (only user-invoked windows take focus).
- **Offline-first:** user actions write to SQLite first and return instantly; never block UI on the network.
- **Security:** keep the CSP in `tauri.conf.json` strict; build HTML only with `esc()`/`safeColor()` from `lib.ts`; SQL only with bound parameters; validate every command argument (`input.rs`, catalog checks).
- **Testing:** pure logic gets unit tests (Rust `#[cfg(test)]`, `src/*.test.ts`); keep I/O (OS, network, windows) in thin wrappers around testable functions. CI runs fmt, clippy, tests and audits on macOS and Windows.
- **Secrets:** this repo is public. Never commit `.env`, `laravel.env`, `.admin-credentials`, tokens or keys. Tokens live only in the OS keychain (service `com.ira.tracker`, account `solidtime-token`).

## Gotchas

- solidtime `APP_ENV` must stay `production`; `local` crashes on missing Telescope.
- In dev, every rebuild changes the binary signature, so macOS asks for keychain access again ("Always Allow").
- Auto-start registers only in release builds (`#[cfg(not(debug_assertions))]`) so dev runs don't install a LaunchAgent.
- Only one widget instance runs (single-instance plugin); a second launch opens the switcher of the first. Stop dev before testing a release build.

## Git workflow

- Branches: `main` (released) ← `staging` (pilot/release candidate) ← `develop` (integration) ← `feature/<name>`.
- Work on a `feature/*` branch from `develop`; open a PR into `develop`. Never commit directly to `main` or `staging`.
- Commit messages: imperative, prefixed with the area, e.g. `widget: add idle detection (W6)`, `server: ...`, `docs: ...`. Reference requirement IDs where relevant.
- Before committing: `npm run build`, `npm test`, `cargo fmt`, `cargo clippy -- -D warnings` and `cargo test` must pass; update docs/STATUS.md when a requirement changes state.
