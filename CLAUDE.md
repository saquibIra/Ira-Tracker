# CLAUDE.md

Working notes for Claude Code on **Time Pilot**, a commercial time-tracking SaaS: a web platform (organizations, projects, members, reports) plus a desktop app that tracks time automatically. Read README.md for the overview and docs/STATUS.md for the compliance matrix: W1–W15 and R1–R6 come from the original requirements doc (Arvind, 31 Oct 2026); AT1–AT8 are the AI auto-tracking change (19 Nov 2026); TP1–TP13 are the Time Pilot product requirements (25 Nov 2026).

**Scope rule:** build what the requirements ask for (the original doc, the AI auto-tracking change, and the Time Pilot product brief of 25 Nov 2026); before adding anything else, check it against them and say so. Engineering quality (tests, security, CI, performance, migrations) is always in scope.

## What this is

- `widget/` — Time Pilot desktop app, Tauri 2 tray app. Rust core (`src-tauri/src`) owns all state; the TypeScript UI (`src/main.ts`) only renders windows and calls commands. Identifier `com.timepilot.desktop`.
- `server/web/` — the **Time Pilot web app: a fork of solidtime (AGPL-3.0)**, built as the official `solidtime/solidtime:<version>` image plus `overlay/` (added or replaced files) and `patches.mjs` (small text edits that must match exactly, or the build fails). `upstream/` holds build configs the published image leaves out, copied from the same tag. Updating solidtime = change `SOLIDTIME_VERSION` in the Dockerfile, build, fix what the build reports. See server/web/README.md.
- `server/autotrack/` — FastAPI service for AI auto-tracking (AI provider `TIMEPILOT_AI_PROVIDER`: `gemini` in production, or `anthropic`; structured logs with request ids): auth via the caller's token, policy, project keyword hints (settings page at `/admin`, browser sign-in), `POST /v1/classify`, usage/cost log, day submissions (W8), `GET /v1/client` (OAuth client ids for the desktop app and the settings page). Holds the only AI key. Reconciliation (R1–R6) goes here too.
- `server/local/` — local stack: the fork, Postgres, Mailpit (catches every email, http://localhost:8025) and Autotrack. `TRUSTED_HOSTS="app"` lets Autotrack call the web app inside Docker.
- `deploy/linux/` — **production** (https://tracker.aasnatech.com): `deploy.sh` (Mac → rsync over SSH; the server cannot reach GitHub but can reach Docker Hub and npm, so the web image is built there) runs `install.sh`: settings and secrets in `/opt/timepilot-data`, Compose project `timepilot`, migrations, the Super Admin (`ADMIN_EMAIL`), the OAuth clients (`timepilot:oauth-clients`), email (`MAIL=log|smtp`), nightly `backup.sh`. Guide: `docs/DEPLOY_LINUX.md`.
- `tests/e2e/` — Playwright test of the whole onboarding in a real browser against the local stack.
- `scripts/brand/make_brand.py` — generates every logo, favicon and app/tray icon from one drawing.

## Platform flows (Time Pilot)

- **Super Admin** (`SUPER_ADMINS`, the installer's `ADMIN_EMAIL`): `/admin` (Filament) → Organizations → **New organization** (`TimePilotOnboardingService`): creates the organization with its admin as **owner** (new user without password, or an existing account) and emails a set-password link (`TimePilotJoinController::welcomeUrl`: signed, 7 days, single use because its check value covers the password hash). **Resend admin email** sends a new one. The Super Admin never becomes a member of customer organizations.
- **Invitations:** the admin invites under Members (solidtime). The emailed link (`organization-invitations.accept`, signed) sends a person without an account to `/join/{invitation}` (signed, 2 h): they choose a name and password and the account is created **verified** (the link proves the address) with only that organization. Existing accounts sign in and are added.
- **Sign-up is invite-only** (`APP_ENABLE_REGISTRATION=off`; `/register` redirects to sign-in; the sign-in page says to ask the admin).
- **Desktop sign-in:** OAuth 2.0 authorization code + PKCE (`widget/src-tauri/src/oauth.rs`) with a loopback redirect `http://127.0.0.1:<random port>/callback` (RFC 8252; Passport ignores the port for loopback). Client id from `GET <server>/autotrack/v1/client`. First-party clients (`TIMEPILOT_FIRST_PARTY_CLIENTS`) skip the consent screen; any other client still sees it. Access tokens 12 h, refresh tokens 90 days (patched in `AuthServiceProvider`); the app renews an hour before expiry and asks to sign in again only when the refresh token stops working (nothing local is deleted). The session (JSON) lives only in the OS keychain (service `com.timepilot.desktop`, account `session`).
- **Settings page** (`/autotrack/admin/`): same browser sign-in with its own public client; the code exchange runs on the service (`POST /v1/admin/oauth/token`), only the access token reaches the tab (sessionStorage).
- New projects are **visible to the organization by default** (patched), so employees and auto-tracking can use them; admins can still make one private.

## Commands

```bash
# local stack (web fork + Postgres + Mailpit + Autotrack)
cd server/local && docker compose up -d --build              # http://localhost:8000, mail: http://localhost:8025
docker compose exec app php artisan migrate --force
docker compose exec app php artisan timepilot:oauth-clients --admin-redirect=http://localhost:8010/admin/
docker compose exec scheduler php artisan tinker --execute='...'   # inspect data

# end-to-end onboarding test (needs the local stack and its Super Admin)
cd tests/e2e && npm ci && npx playwright install chromium
TP_SUPERADMIN_EMAIL=... TP_SUPERADMIN_PASSWORD=... npx playwright test

# web fork only
docker build -t time-pilot/web:dev server/web                 # applies overlay + patches, type-checks, builds

# desktop app
cd widget && npm run tauri dev        # run (tray app; no Dock icon); signs in against http://localhost:8000
TIMEPILOT_IDLE_SECS=30 npm run tauri dev    # idle prompt after 30 s instead of 10 min (testing W6/W7)
TIMEPILOT_AUTO_TICK_SECS=5 TIMEPILOT_CAPTURE_SECS=30 npm run tauri dev   # faster auto-tracking checks / AI calls (testing)
cd widget && npm run build            # typecheck + build UI
cd widget && npm test                 # UI unit tests (vitest, src/*.test.ts)
cd widget/src-tauri && cargo test     # Rust unit tests
cd widget/src-tauri && cargo clippy --all-targets -- -D warnings
cd widget/src-tauri && cargo fmt      # format before committing
cd widget/src-tauri && cargo audit; cd widget && npm audit   # dependency vulnerabilities

# Autotrack service (needs server/autotrack/.env; see .env.example)
cd server/autotrack && .venv/bin/python -m pytest -q && .venv/bin/ruff check . && .venv/bin/ruff format --check .

# brand assets
python3 scripts/brand/make_brand.py   # then: cd widget && npx tauri icon src-tauri/icons/app-icon-source.png

# local desktop database
sqlite3 ~/Library/Application\ Support/com.timepilot.desktop/tracker.db
```

Rust is installed via rustup; run `source ~/.cargo/env` in a fresh shell if `cargo` is missing. Node is under nvm (`~/.nvm/versions/node/v22.14.0/bin`).

## Desktop app architecture

- `db.rs` — SQLite (WAL, synchronous=FULL). Schema changes go through `MIGRATIONS` (tracked in `PRAGMA user_version`): append a step, never edit a released one. Table `entry` is the offline queue and local source of truth. Every local change sets `dirty=1` and bumps `rev`; sync clears `dirty` only if `rev` is unchanged. Rows rejected by the server (4xx) get `error`/`error_rev` and are skipped until edited again. Starting a timer closes the running one in the same transaction (no overlaps); zero-length entries are dropped.
- Creates are duplicate-safe: `create_attempted` is set before POST; on retry the sync looks the entry up by exact start time before creating again.
- Migration 3 adds `entry.source` (`manual`/`auto`), `entry.context` (window fingerprint of an auto entry, local only) and `context_cache` (window fingerprint → project learned from the AI or the user's corrections; AI answers reused 4 h, user corrections 30 days; cleared on sign-out). An entry without a project is a "Needs review" block.
- `oauth.rs` — browser sign-in (see Platform flows). Pure parts (PKCE incl. the RFC 7636 vector, URLs, callback parsing, renewal timing) and the loopback listener are unit-tested.
- `auto.rs` — AI auto-tracking. Every 20 s while enabled and not paused/idle/away (any day, any time; no work hours since 24 Nov 2026): front window → learned cache → admin keyword hints → (at most once per capture interval) screenshot + title to the Autotrack service. `Engine` is a pure state machine (unit-tested): hysteresis (`min_switch_secs`), switches backdated to when the work started (never before the running entry), unresolved/private windows become review blocks after `unresolved_review_secs`. A manual start/stop pins auto-switching for `pin_secs` and teaches nothing. AI answers and review corrections are remembered only for specific window titles (`auto::specific`). Policy comes from the server (`GET /v1/config`), cached in `kv`.
- `context.rs` / `capture.rs` — front app + window title (CoreGraphics window list on macOS, foreground window on Windows) and the screenshot (ScreenCaptureKit on macOS 14+, GDI on Windows): one display, downscaled by the OS, grayscale JPEG in memory only. Pure helpers are unit-tested; `cargo test live_ -- --ignored --nocapture` checks a real machine.
- `ai.rs` — Autotrack service client (policy, hints with ETag, multipart classify, day submit). The service address is derived from the signed-in server (`ai::service_url`: `<server>/autotrack`, or `http://localhost:8010` for a local server); settings of another server's service are dropped (`db::forget_other_service`).
- `review.rs` — W8 end-of-day review (blocks with screenshots, reassign/describe/delete, edit times, split, Submit); reminder at `review_at` on days with tracked time.
- `shots.rs` — kept screenshots: files + `capture` table, ids validated, retention pruning, deleted on sign-out.
- `applog.rs` — app log file `<app data>/logs/time-pilot.log` (1 MB rotation); tray "Open logs folder". Never log titles, screenshots, descriptions or tokens.
- `idle.rs` — W6/W7 (`Detector` pure state machine; OS idle seconds; sleep = wall-clock jump).
- `input.rs` — validation at the command boundary (server URL HTTPS unless localhost; descriptions single-line, ≤ 500 chars).
- `src/lib.ts` — pure UI helpers (escaping, colour validation, fuzzy search, `aiName`, `afterTurnOn`, `MARK_SVG`), unit-tested; `src/main.ts` renders windows (welcome/sign-in, switcher, auto-tracking consent, idle, review, nudge).
- `api.rs` — web app REST client. `ApiError::Offline` (network/5xx → retry), `Auth` (401), `Rejected` (other 4xx).
- `lib.rs` — app state, tray menu, commands (`sign_in`, `choose_organization`, `cancel_sign_in`, `sign_out`, timers, `auto::*`, `review::*`), sync loop (≤ 1 sync/min, renews the session first), `session_keeper` (checks renewal every minute, no network otherwise), window management.
- Windows are created on demand and destroyed on close/blur. After any state change call `changed(app)`.

## Web app API facts (solidtime v0.23)

- Auth: `Authorization: Bearer <token>` (OAuth access token or personal access token). Base path `/api/v1`. Scopes: `create read update delete`.
- `GET /users/me/memberships` → organization id and **member id** (time entries need `member_id`).
- `GET /organizations/{org}/projects|tasks|clients` (paginated, 500 per page).
- `POST /organizations/{org}/time-entries` requires `member_id`, `start`, `billable`; running timer = `end: null`. Datetimes UTC `YYYY-MM-DDTHH:MM:SSZ`.
- The web app accepts only its own host (`APP_URL`) plus `TRUSTED_HOSTS`.
- `config/app.php` name comes from `APP_NAME` (patched).

## Rules that must hold

- **AGPL:** the web app is a modified solidtime. Its source must stay available to everyone who uses it: the `/source` link (sign-in pages and sidebar) points to `TIMEPILOT_SOURCE_URL`. Keep the overlay small and documented; never remove the source link or the AGPL notice.
- **Privacy (auto-tracking):** never capture keystrokes or mouse activity. Window titles and screenshots are used only for auto-tracking, only after the user accepted the consent screen (`CONSENT_VERSION`, now v4: any time, no work hours), and never while paused, idle, locked or in an excluded app. Screenshots: once per capture interval while tracking, one display, downscaled grayscale JPEG, kept **only on the person's computer** (`<app data>/captures/`, files 0600, folder 0700) for `keep_screenshots_days`; sent to the Autotrack service only when the AI must classify; the service never stores them. The AI key (Gemini or Anthropic) lives only on the Autotrack service.
- **Performance budget:** idle RSS < 50 MB, idle CPU < 0.5 %, startup < 2 s to tray, installer < 15 MB, at most one batched sync per minute while running and nothing while stopped, no focus stealing (only user-invoked windows take focus).
- **Offline-first:** user actions write to SQLite first and return instantly; never block UI on the network.
- **Security:** strict CSP in `tauri.conf.json` and on the settings page; HTML only with `esc()`/`safeColor()` (desktop) or `textContent` (settings page); SQL only with bound parameters; validate every command argument; secrets reach the server only on stdin (`deploy.sh`), never on a command line; signed, expiring links for onboarding; PKCE for every browser sign-in.
- **Testing:** pure logic gets unit tests (Rust `#[cfg(test)]`, `src/*.test.ts`, pytest); user-facing flows get a step in `tests/e2e/onboarding.spec.ts`. CI runs fmt, clippy, tests, audits (macOS and Windows) and builds the web fork.
- **Secrets:** this repo is public. Never commit `.env`, `laravel.env`, credentials, tokens or keys. Desktop tokens live only in the OS keychain.

## Gotchas

- solidtime `APP_ENV` must stay `production`; `local` crashes on missing Telescope.
- Email: Mailinator only receives mail and silently drops mail sent straight from the server (no SPF/reverse DNS). Production email needs an SMTP relay (Brevo etc.: `MAIL=smtp`); Mailinator addresses then work for testing.
- Browsers may report legacy time zones (`Asia/Calcutta`); the join pages map them like solidtime's sign-up (`TimezoneService`).
- In dev, every rebuild changes the binary signature, so macOS asks for keychain access again ("Always Allow").
- Auto-start registers only in release builds.
- Local release builds: sign with the local self-signed identity "Ira Tracker Local Signing" (its name predates the rename; it lives only in this Mac's login keychain): `APPLE_SIGNING_IDENTITY="Ira Tracker Local Signing" npm run tauri build -- --bundles app`. macOS ties Screen Recording to the signature; real releases need Apple Developer ID signing (W15).
- Screen Recording (macOS) is granted per binary and identifier; the rename to `com.timepilot.desktop` means everyone allows it once more.
- Only one desktop instance runs (single-instance plugin). Stop dev before testing a release build.

## Git workflow

- Branches: `main` (released) ← `staging` (pilot/release candidate) ← `develop` (integration) ← `feature/<name>`.
- Work on a `feature/*` branch from `develop`; merge into `develop`. Never commit directly to `main` or `staging`.
- Commit messages: imperative, prefixed with the area, e.g. `widget: ...`, `web: ...`, `autotrack: ...`, `deploy: ...`, `docs: ...`. Reference requirement IDs where relevant.
- Before committing: `npm run build`, `npm test`, `cargo fmt`, `cargo clippy -- -D warnings`, `cargo test`, the Autotrack tests, and for web changes `docker build server/web` plus the e2e test; update docs/STATUS.md when a requirement changes state.
