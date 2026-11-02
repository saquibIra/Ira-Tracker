# Requirements compliance

Source of truth: *Time Tracking Widget – Requirements & Solution Approach* (Arvind, 31 Oct 2026).
Scope rule: build what the document asks for; anything extra is checked against it first.

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
| W8 | End-of-day review at 18:30: blocks, gaps, edit, Submit | Must | ⬜ | Next |
| W9 | Offline queue; no loss on crash/reboot | Must | ✅ | SQLite WAL + `synchronous=FULL`; duplicate-safe creates (`create_attempted` + lookup) |
| W10 | Start/stop/switch instant locally; async sync | Must | ✅ | All actions write SQLite first |
| W11 | Reminder if no timer for 30 min in work hours (max 3/day) | Should | ⬜ | After W8 |
| W12 | Repo/branch → project suggestion toast | Should | ⏭ | Phase 2 |
| W13 | Ticket keys from branch into description | Should | ⏭ | Phase 2 |
| W14 | Calendar pre-fill | Could | ⏭ | Phase 2 |
| W15 | Auto-start with OS; auto-update via signed releases | Must | 🟡 | Auto-start in release builds. Signing + updater need certificates (open question) |

Platforms: the document requires **macOS and Windows** (acceptance criteria) and lists Linux as an open question. One Tauri codebase; CI builds and tests on macOS and Windows. Windows has not yet been run by hand.

## Widget — performance and privacy constraints

| Constraint | Target | Status |
|------------|--------|--------|
| Memory, idle | < 50 MB | To measure on release build (see below) |
| CPU, idle | < 0.5 % over 10 min | Design: no polling while stopped; 30 s checks while running |
| CPU, switch/sync | brief spike, idle within 1 s | Design: single SQLite write + small JSON call |
| Installer size | < 15 MB | To measure |
| Startup | < 2 s to tray | To measure |
| Network | batched, ≤ 1 sync/min while running; nothing while stopped | ✅ `sync_loop` enforces the 60 s minimum interval; no calls when nothing is queued |
| Disk | SQLite < 10 MB | ✅ synced entries older than 7 days are pruned |
| Context polling (W12) | event-driven or ≤ 30 s, only while running | ⏭ Phase 2 (idle checks already follow this: 30 s, only while running) |
| UI | no focus stealing; toasts non-modal, auto-dismiss 8 s | ✅ idle prompt opens without focus; only user-invoked windows take focus |
| Privacy | no screenshots, keystrokes, mouse tracking, window titles, URLs | ✅ only "seconds since last input" is read for W6 |

## Server side

| Item | Status |
|------|--------|
| solidtime self-hosted in Docker (Phase 0) | ✅ local; bound to 127.0.0.1. Azure VM + HTTPS (Caddy/Nginx) + nightly backup ⬜ |
| Master data: customers → projects → tasks, billable flags, rates, internal projects | ✅ demo data via `server/seed/seed_demo.py`; real data ⬜ |
| Phase 0 pilot with official solidtime desktop app | ⬜ needs the desktop OAuth client (see server/README.md) |
| Reconciliation service (FastAPI), R1–R6, period lock, reports, admin UI, notifications | ⬜ |

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
