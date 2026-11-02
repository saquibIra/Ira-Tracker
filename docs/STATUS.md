# Requirements status

Source: *Time Tracking Widget – Requirements & Solution Approach* (Arvind, 31 Oct 2026).

## Widget

| ID | Requirement | Priority | Status |
|----|-------------|----------|--------|
| W1 | Sign in once with personal API token; token in OS keychain | Must | Done |
| W2 | Tray shows running (project short name + elapsed) or stopped | Must | Partial: macOS menu-bar title + tooltip; separate running/stopped icon TODO |
| W3 | Tray lists 5 most recent project/task pairs; one click switches | Must | Done |
| W4 | Global hotkey (Cmd/Ctrl+Shift+T) opens fuzzy quick switcher | Must | Done |
| W5 | Optional one-line description, editable while running | Must | Done |
| W6 | Idle 10 min → on return ask Keep / Discard / Assign | Must | TODO |
| W7 | Screen lock / sleep treated as idle | Must | TODO |
| W8 | End-of-day review (18:30): blocks, gaps, edit, Submit | Must | TODO |
| W9 | Offline queue, no data loss on crash/reboot | Must | Done (see known issues) |
| W10 | Start/stop/switch instant locally; sync async | Must | Done |
| W11 | Reminder if no timer for 30 min in work hours (max 3/day) | Should | TODO |
| W12 | Repo/branch → project suggestion toast | Should | Phase 2 |
| W13 | Ticket keys from branch into description | Should | Phase 2 |
| W14 | Calendar pre-fill | Could | Phase 2 |
| W15 | Auto-start with OS; signed auto-update | Must | Auto-start wired (release builds only); signing/update TODO |

## Reconciliation service

| ID | Check | Status |
|----|-------|--------|
| R1 | Entry has no project | TODO |
| R2 | Daily total < 85% or > 110% of expected | TODO |
| R3 | Time on a project the person is not allocated to | TODO |
| R4 | Overlapping entries | TODO (solidtime `prevent_overlapping_time_entries` enabled) |
| R5 | Edits after period lock | TODO — see decision below |
| R6 | Actual vs planned allocation differs > 20% | TODO |

## Decisions and open points

- **Server runs locally** on a developer Mac for now; move to an Azure VM before the pilot.
- **Linux:** recommended to drop from v1 (idle detection and global hotkeys are unreliable on Wayland).
- **R5 vs "reconciliation never edits entries":** solidtime has no period lock, so web edits cannot be truly blocked. Plan: snapshot entries at lock time, bill from the snapshot, flag later changes; the widget refuses edits to locked periods.
- **"Submit" a day (W8)** needs storage solidtime lacks: add a `day_submission` table to the reconciliation service.
- **Hotkey clash:** Cmd+Shift+T is also "reopen closed tab" in browsers. Make it configurable.
- **Code signing:** Apple Developer account and Windows certificate needed before rollout (longest lead time).

## Known issues

- If the app crashes after the server accepts a new entry but before the local row stores its remote id, the next sync creates a duplicate. Fix: send a client-generated id (e.g. in a tag or description marker) and de-duplicate on pull.
