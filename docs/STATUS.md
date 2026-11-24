# Requirements compliance

Source of truth: *Time Tracking Widget – Requirements & Solution Approach* (Arvind, 31 Oct 2026).
Scope rule: build what the document asks for; anything extra is checked against it first.
**Agreed change (19 Nov 2026, with Arvind):** AI auto-tracking from window context and screenshots (AT1–AT8 below). It replaces the doc's "no screenshots / window titles" privacy constraint; Arvind should update the document so it stays the source of truth.

Legend: ✅ done · 🟡 partial · ⬜ not started · ⏭ later phase

## Widget — functional (W1–W15)

| ID | Requirement | Pri | Status | Where / notes |
|----|-------------|-----|--------|---------------|
| W1 | Sign in once with personal API token; token in OS keychain | Must | ✅ | `lib.rs` `sign_in`; keychain service `com.ira.tracker`. HTTPS enforced except localhost (`input.rs`) |
| W2 | Tray icon shows running/stopped; project short name + elapsed on hover | Must | ✅ | `tray_icon`, `update_tooltip` |
| W3 | Tray lists 5 most recent project/task pairs; one click switches | Must | ✅ | `refresh_tray`, `db::recent` |
| W4 | Global hotkey (Cmd/Ctrl+Shift+T) opens fuzzy quick switcher over assigned projects/tasks | Must | ✅ | Lists what solidtime returns for the user: for employees, their projects (+ public ones); admins see all |
| W5 | Optional one-line description, editable while running | Must | ✅ | Single line, max 500 chars (`input::description`) |
| W6 | Idle 10 min (configurable) → Keep / Discard / Assign | Must | ✅ | `idle.rs`; threshold from `idle_minutes` setting (default 10). Needs manual test on device |
| W7 | Screen lock and sleep treated as idle; same prompt | Must | ✅ | Lock = no input; sleep = wall-clock jump. Needs manual test on device |
| W8 | End-of-day review at 18:30: blocks, gaps, edit, Submit | Must | ✅ | `review.rs`: day view (blocks + gaps ≥ 5 min, screenshots), reassign/describe/delete, edit times, split, Submit (blocked while "Needs review" blocks remain) stored on the Ira service; reminder toast at `review_at` (non-focus, 8 s). Needs manual test on device |
| W9 | Offline queue; no loss on crash/reboot | Must | ✅ | SQLite WAL + `synchronous=FULL`; duplicate-safe creates (`create_attempted` + lookup) |
| W10 | Start/stop/switch instant locally; async sync | Must | ✅ | All actions write SQLite first |
| W11 | Reminder if no timer for 30 min in work hours (max 3/day) | Should | ⬜ | After W8 |
| W12 | Repo/branch → project suggestion toast | Should | 🟡 | Superseded by AT3: repo names and ticket prefixes in window titles are matched against admin keywords and switch automatically (no toast) |
| W13 | Ticket keys from branch into description | Should | ⏭ | Phase 2 (AI notes already describe the activity) |
| W14 | Calendar pre-fill | Could | ⏭ | Phase 2 |
| W15 | Auto-start with OS; auto-update via signed releases | Must | 🟡 | Auto-start in release builds. Signing + updater need certificates (open question) |

## AI auto-tracking (agreed change, 19 Nov 2026)

| ID | Requirement | Status | Where / notes |
|----|-------------|--------|---------------|
| AT1 | Opt-in with a consent screen explaining what is captured, when, where it goes and what is kept | ✅ | `auto` window, `CONSENT_VERSION`; consent and learned data are cleared on sign-out |
| AT2 | Timer follows the work without manual switching; hysteresis, backdated switches; unsure time → "Needs review" | ✅ | `auto.rs` `Engine` (unit-tested). Defaults: switch after 2 min, review below 0.6 confidence |
| AT3 | Cheap signals first: front window → learned cache → admin keywords → AI | ✅ | Window checked every 20 s while active; AI at most once per capture interval (default 5 min) |
| AT4 | Screenshots: visible to the engineer, never stored on the server | ✅ | Changed 19 Nov 2026 (consent v2): one per capture interval while tracking, kept only on the engineer's computer for 3 days (`IRA_KEEP_SCREENSHOTS_DAYS`, 0 = none) and shown in the day review; sent to the service only for AI classification; the service stores no image, title or token |
| AT9 | Review shows the screen and the project; fix project and times | ✅ | Thumbnails per block (orange border = screenshot points to another project), full viewer with how the project was decided, split at a screenshot, edit start/end with the neighbouring block following |
| AT5 | Cheap but accurate, configurable model | ✅ | Claude Haiku 5.5, re-checked by Claude Opus 5.5 below 0.7 confidence; models, thresholds, effort via env (`server/ira-service/.env.example`) |
| AT6 | Private time: pause, excluded apps, work hours only | ✅ | Tray: Pause 30 min / 1 h / until tomorrow (stops the timer); excluded apps (password managers, personal messengers) never captured |
| AT7 | Manual override always wins; corrections teach the system | ✅ | Manual start/stop pins auto-switching for 30 min. Review corrections teach the cache (30 days), and AI answers are reused for 4 h, both only for specific window titles (generic ones like "Zoom Meeting" are re-checked) |
| AT8 | Admin: project descriptions/keywords, AI usage and cost, day submissions | ✅ | `/admin` on the Ira service (owner/admin role in solidtime) |
| AT10 | Production on Google Cloud, Gemini, logs, easy distribution | 🟡 | Deploy kit `deploy/gcp/` (one VM, HTTPS, Vertex AI Gemini via service account, Cloud Logging, daily snapshots). Structured service logs with request ids; app log file. Installers: `scripts/release-mac.sh`, Windows via Actions. Pending: running it (needs project, domain), Apple Developer ID for warning-free Mac install, auto-update (W15) |

Measured/verified: Python service 51 tests; Rust 54 unit tests; UI 15 tests; live macOS window read and capture path (permission-denied path returns cleanly); Windows capture/context code type-checked and clippy-clean for `x86_64-pc-windows-msvc`; end-to-end against local solidtime (auth, org isolation, role checks, project visibility, hints + ETag, classify, day submission). Live Anthropic API reached with the configured key (19 Nov): the account has no credit yet, so the service reports "credit balance too low" and pauses AI calls (verified). Not yet: a successful live classification and the accuracy check on labelled screenshots, on-device GUI run, Windows device run.

Platforms: the document requires **macOS and Windows** (acceptance criteria) and lists Linux as an open question. One Tauri codebase; CI builds and tests on macOS and Windows. Windows has not yet been run by hand.

## Widget — performance and privacy constraints

| Constraint | Target | Status |
|------------|--------|--------|
| Memory, idle | < 50 MB | To measure on release build (see below) |
| CPU, idle | < 0.5 % over 10 min | Design: no polling while stopped; 30 s checks while running |
| CPU, switch/sync | brief spike, idle within 1 s | Design: single SQLite write + small JSON call |
| Installer size | < 15 MB | To measure |
| Startup | < 2 s to tray | To measure |
| Network | batched, ≤ 1 sync/min while running; nothing while stopped | ✅ `sync_loop` enforces the 60 s minimum interval; no calls when nothing is queued. Auto-tracking adds at most one classify call (≈ 100 KB) per capture interval, only when cache and keywords cannot decide, plus an hourly policy/hints check (ETag) |
| Disk | SQLite < 10 MB | ✅ synced entries older than 7 days are pruned |
| Context polling (W12) | event-driven or ≤ 30 s, only while running | ✅ auto-tracking reads the front window every 20 s, only while enabled, in work hours, not paused and not idle |
| UI | no focus stealing; toasts non-modal, auto-dismiss 8 s | ✅ idle prompt opens without focus; only user-invoked windows take focus |
| Privacy | changed 19 Nov 2026: no keystrokes or mouse tracking; window titles and screenshots only for opted-in auto-tracking | ✅ see AT1, AT4, AT6. Idle detection still reads only "seconds since last input" |

## Server side

| Item | Status |
|------|--------|
| solidtime self-hosted in Docker (Phase 0) | ✅ local; bound to 127.0.0.1. Azure VM + HTTPS (Caddy/Nginx) + nightly backup ⬜ |
| Master data: customers → projects → tasks, billable flags, rates, internal projects | ✅ demo data via `server/seed/seed_demo.py`; real data ⬜ |
| Phase 0 pilot with official solidtime desktop app | ⬜ needs the desktop OAuth client (see server/README.md) |
| Ira service (FastAPI, `server/ira-service`): auto-tracking classify, policy, hints admin page, usage/cost log, day submissions | ✅ local (port 8010, bound to 127.0.0.1 in Docker). Azure + HTTPS ⬜ |
| Reconciliation jobs R1–R6, period lock, reports, notifications (to be added to the Ira service) | ⬜ |

## Reconciliation rules

| ID | Check | Status |
|----|-------|--------|
| R1 | Entry has no project | ⬜ |
| R2 | Daily total < 85 % or > 110 % of expected | ⬜ |
| R3 | Time on a project the person is not allocated to | ⬜ |
| R4 | Overlapping entries | ⬜ (solidtime `prevent_overlapping_time_entries` on; widget never overlaps) |
| R5 | Edits after period lock | ⬜ see open point below |
| R6 | Actual vs planned allocation > 20 % | ⬜ |

## Acceptance criteria (MVP)

| Criterion | Status |
|-----------|--------|
| Switch projects in one click/hotkey, < 5 s | ✅ manual check done on macOS dev build |
| Every performance target met on Windows and macOS | ⬜ macOS measurement pending; Windows pending |
| 8 h offline then reconnect: no loss or duplicates | 🟡 logic + unit tests done; long offline run not yet performed |
| Idle and lock trigger Keep / Discard / Assign | 🟡 implemented + unit tests; device test pending |
| Nightly job raises R1–R6 on seeded data | ⬜ |
| Billing export matches hand-calculated total | ⬜ |
| ≥ 90 % of working days submitted after 2 weeks | ⬜ rollout |

## Open questions (from the document)

- Notification channel for leads: email, Slack or Teams?
- Is Linux needed in v1, or only Windows and macOS?
- Does Ira already hold Windows and Apple code-signing certificates?

## Points to confirm with Arvind (found while building)

- **Auto-tracking consent and data protection:** engineers see a consent screen, but employees should also get a written notice (and HR/legal sign-off) before rollout. Screenshots and window titles are sent to Anthropic's API for classification (not used for training by default); ask Anthropic about zero-data-retention if required.
- **Open self-registration (testing only, 24 Nov 2026):** at the team's request, the server allows anyone to sign up without email confirmation (`REGISTRATION=on`, `REGISTRATION_DOMAINS='*'`). New accounts join the company organization as employees (`deploy/linux/onboard.sh`). This is not in the requirements document. Before rollout: add SMTP and switch to `invite-only`, or at least limit it to company domains.
- **Auto entries in solidtime are not tagged:** solidtime tags would need tag-create permission for employees. The widget records `source=auto` locally and the service logs AI usage; tag syncing can follow if admins need it in solidtime.

- **R5 vs "reconciliation never edits entries":** solidtime has no period lock, so edits in its web UI cannot be blocked. Proposal: snapshot entries at lock time, bill from the snapshot, flag later changes.
- **"Submit" (W8) and the compliance report** need a `day_submission` table in the reconciliation service (not in the data model table).
- **Hotkey:** Cmd+Shift+T is also "reopen closed tab" in browsers; the document makes it the default, so it stays, but it should become configurable.

## Security review (2 Nov 2026)

- Token only in OS keychain; never in SQLite, logs or the repo. HTTPS required for non-local servers.
- Webview: strict Content-Security-Policy; all server text HTML-escaped; colours validated.
- SQL: parameterised queries only.
- Input validation at command boundary: project/task must exist; descriptions single-line and capped.
- Local server bound to 127.0.0.1; secrets files git-ignored (repo is public).
- Dependencies: `npm audit` 0 vulnerabilities; `cargo audit` 0 vulnerabilities, 5 warnings (unmaintained/unsound crates in Linux-only GTK/D-Bus dependencies of Tauri; not shipped in macOS/Windows builds).

## Security review — auto-tracking (19 Nov 2026)

- Anthropic key only on the Ira service; widgets authenticate with their own solidtime token, verified against solidtime (cached 10 min by token hash; never stored or logged).
- Every request is scoped to the caller's organization membership; project lists come from solidtime with the caller's token, so an engineer can only be classified onto projects they can see. Admin endpoints require `owner`/`admin`.
- Model output is untrusted: only project references we offered are accepted (prompt injection via on-screen text cannot pick another project); confidence clamped; activity notes sanitized; non-work results never carry details.
- Screenshots: grayscale, one display, ≤ 900 KB, JPEG magic checked; requests that could spill to a temp file are rejected (411/413); per-user rate limit (429). No image, title or token in any table (tested).
- Widget: new commands validate URLs (`input::server_url`), project/task ids against the catalog and descriptions (`input::description`); new windows build HTML only with `esc()`/`safeColor()`; CSP unchanged.
- Dependencies: `pip-audit` 0 known vulnerabilities; `npm audit` 0; `cargo audit` 0 vulnerabilities, 7 warnings: the 5 Linux-only GTK/D-Bus ones above, plus two advisories published since: `chrono` marked unmaintained (direct dependency, no vulnerability; candidate replacement `jiff`) and `async-executor` yanked (Linux D-Bus path). None come from the new auto-tracking dependencies.
