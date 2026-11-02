# Manual test plan

Tests every built feature as a real engineer (employee role) and as an admin. Each step names the requirement it checks (W = widget, from the requirements doc). Mark each step pass or fail; for a fail, note what you saw.

Accounts (local only) are in `server/solidtime/.admin-credentials`:

| Account | Role in "Ira Technologies" | Used for |
|---------|---------------------------|----------|
| `mhd.saquib@iratechnologies.com` | Owner (admin) | Master data, invitations, seeing everyone's time |
| `engineer@ira.local` | Employee (after accepting the invite) | Using the widget like an engineer |

Use two browser windows so both can be logged in at once: a normal window for the admin and a **private/incognito** window for the engineer.

---

## Part A — Set up the engineer (browser, ~5 min)

| # | Who | Do this | Expect |
|---|-----|---------|--------|
| A1 | Admin | Log in at http://localhost:8000 → **Members** | `engineer@ira.local` listed under pending invitations (role Employee) |
| A2 | — | In a terminal: `grep -ohE "http://localhost:8000/organization-invitations/[^ \"'<>]*" server/solidtime/logs/*.log \| grep -v amp \| tail -1` | Prints the invitation link (emails go to the log locally, not to an inbox) |
| A3 | Engineer | Private window: log in as `engineer@ira.local`, then open the link from A2 and accept | Engineer now sees organization **Ira Technologies** (switcher top-left) |
| A4 | Admin | **Projects → Acme – POS Revamp → Members → Add member** → Test Engineer. Repeat for **Internal** | Engineer is assigned to exactly these two projects |
| A5 | Engineer | Switch to **Ira Technologies**; **Profile → API Tokens → Create**, name it `Ira Tracker`, copy the token | Token shown once; keep it for B2 |

To add more test engineers later: `docker compose exec scheduler php artisan admin:user:create "Name" "name@ira.local" --verify-email` (in `server/solidtime`), then invite them from **Members** and repeat A2–A5.

## Part B — Widget as the engineer

### B0. Start clean

1. Quit any running Ira Tracker (tray menu → **Quit Ira Tracker**). This includes the copy in `/Applications`; only one instance can run.
2. Start the dev build with a short idle threshold:
   ```bash
   cd ~/Desktop/ira_tracker/widget
   IRA_IDLE_SECS=30 npm run tauri dev
   ```
3. If a previous account is signed in: tray → **Sign out**.

### B1. Sign-in and organizations (W1)

| # | Do this | Expect |
|---|---------|--------|
| 1 | Server URL `http://192.168.1.10:8000`, any token → **Sign in** | Refused: "Server URL must start with https://" (token never sent over plain HTTP to another machine) |
| 2 | Server URL `http://localhost:8000`, token `abc` → **Sign in** | Refused: token rejected |
| 3 | Paste the engineer token from A5 → **Sign in** | An **Organization** picker appears listing "Test's Organization (owner)" and "Ira Technologies (employee)" |
| 4 | Choose **Ira Technologies** → **Sign in** | Window closes; grey tray icon appears |
| 5 | Quit and restart `npm run tauri dev` | Still signed in (token is in the macOS keychain; allow keychain access if asked) |

### B2. Switching and the tray (W2, W3, W4, W5, W10)

| # | Do this | Expect |
|---|---------|--------|
| 6 | Press **⌘⇧T** (Command+Shift+T) | Quick switcher opens; lists **only** Acme – POS Revamp (+ its tasks) and Internal (+ its tasks) — not Northwind or Globex (W4: assigned projects only) |
| 7 | Type `pos back`, press **Enter** | Switcher closes instantly; tray icon turns **green** (W2, W10) |
| 8 | Hover the tray icon | Tooltip `POS Revamp — 0:00` (short name + elapsed, W2) |
| 9 | ⌘⇧T → **Tab** to the description field → type `ABC-12 login fix` → Enter on `internal meet` | Timer switches to Internal · Meetings with that description; the POS entry ends at the same second (W5) |
| 10 | ⌘⇧T → click the description under the running timer, change it, press Enter | Description updated while running (W5) |
| 11 | Paste a multi-line text into a description | Saved as one line (max 500 characters) |
| 12 | Click the tray icon | Menu: status, **Stop timer**, up to 5 recent project/task pairs, Quick switch, Sign out, Quit (W3) |
| 13 | Click a recent item | Timer switches to it in one click (W3) |
| 14 | Menu → **Stop timer** | Icon turns grey; tooltip "Ira Tracker — stopped" |

### B3. Server sync (W10, and what managers see)

| # | Do this | Expect |
|---|---------|--------|
| 15 | Wait up to 1 minute, then refresh **Time** in the engineer's browser | All entries from B2 appear with the right project, task, description and times (sync is batched at most once a minute) |
| 16 | Admin browser → **Time** or **Reports**, filter member Test Engineer | Admin sees the engineer's entries; nothing else (no activity data) |
| 17 | Check the entries' fields | Only project, task, description, start, end and billable — no window titles, URLs or activity (privacy rules) |

### B4. Offline, crash and reboot (W9)

| # | Do this | Expect |
|---|---------|--------|
| 18 | `cd server/solidtime && docker compose stop app` | Server down |
| 19 | Switch projects 3 times with ⌘⇧T | Still instant; tooltip shows "Offline · N waiting to sync" |
| 20 | Quit the widget (simulates crash/reboot), start `npm run tauri dev` again | Running timer and recent list intact |
| 21 | Tray → **Sign out** | Refused: "N entries not synced yet" (no data can be lost by signing out) |
| 22 | `docker compose start app`, wait ~1–2 min | All offline entries appear in solidtime **once** — no gaps, no duplicates |

### B5. Idle, screen lock and sleep (W6, W7)

Threshold is 30 s because of `IRA_IDLE_SECS=30` (default 10 min).

| # | Do this | Expect |
|---|---------|--------|
| 23 | Start a timer; don't touch keyboard or mouse for 40 s; then move the mouse | Within ~3 s a prompt appears top-right: "You were away for 0 min … the timer kept running on …". It does **not** take focus (typing continues in your current app) |
| 24 | Click **Keep** | Prompt closes; one continuous entry |
| 25 | Repeat 23, click **Discard** | The entry ends when you left; a new entry for the same project starts when you came back (gap in solidtime) |
| 26 | Repeat 23, click **Assign to…**, pick another project | The away time becomes its own entry on that project; the original timer continues |
| 27 | Lock the screen (**Ctrl+Cmd+Q**) for 40 s, unlock | Same prompt (W7) |
| 28 | Sleep the Mac (Apple menu → Sleep) for 1 min, wake | Same prompt (W7) |
| 29 | Stop the timer, stay idle 40 s, return | **No** prompt (only tracked time is checked) |
| 30 | Start a timer, idle 40 s, don't answer the prompt; click the tray icon | Menu shows "Away N min — Keep / Discard / Assign…"; clicking it brings the prompt back |

## Part C — Admin checks in solidtime

| # | Do this | Expect |
|---|---------|--------|
| 31 | **Reports**, this week, group by member then project | Engineer's hours split by project; billable vs non-billable (Internal is non-billable) |
| 32 | **Projects → Acme – POS Revamp** | Spent time includes the engineer's entries |
| 33 | Try to remove the engineer from POS Revamp while they have a running timer on it, then press ⌘⇧T in the widget | After the next catalog refresh (≤ 10 min) POS Revamp disappears from the switcher |

## Not testable yet

End-of-day review (W8), reminders (W11), repo/branch and calendar suggestions (W12–W14), signed auto-update (W15), and the reconciliation service (R1–R6, billing and utilization exports). See [STATUS.md](STATUS.md).

## Reporting a problem

Note the step number, what you expected and what happened. Useful extra detail:

```bash
# widget's local queue (unsynced rows have dirty=1; rejected rows have an error)
sqlite3 ~/Library/Application\ Support/com.ira.tracker/tracker.db \
  "select start, end, description, dirty, error from entry order by start desc limit 10;"
# server logs
cd server/solidtime && docker compose logs --tail 50 app
```
