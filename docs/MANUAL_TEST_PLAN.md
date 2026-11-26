# Manual test plan

What a person checks by hand before a release, on the local stack (`server/local`). The onboarding itself (Super Admin, admin, invitations, browser sign-in) is covered automatically by the end-to-end test; run it first:

```bash
cd tests/e2e && TP_SUPERADMIN_EMAIL=superadmin@timepilot.local TP_SUPERADMIN_PASSWORD=... npx playwright test
```

## Part A — Set up a test organization (browser)

| # | Who | Do this | Expect |
|---|-----|---------|--------|
| A1 | Super Admin | http://localhost:8000 → sign in → http://localhost:8000/admin → **Organizations → New organization**: "Acme Studio", admin "Ada Admin", `ada@example.com` | "Organization created" and the admin's password link with a copy button; you are **not** a member of Acme Studio |
| A2 | — | Open the copied link in a private window | Welcome page; set a password → dashboard with "Your organization is ready" |
| A3 | Admin | **Projects → Create Project**: "Acme – POS Revamp" (tasks Backend, Frontend), "Acme – Mobile App", "Internal" (tasks Meetings, non-billable) | Projects listed, visible to the organization |
| A4 | Admin | **Members → Invite member**: `eng@example.com`, Employee | The invite link opens with **Copy link**; the invitation is listed as pending; its menu has **Copy invite link** |
| A5 | Engineer | Open the copied invite link (in a private window) | "Join Acme Studio" page; choose name and password → dashboard; the engineer belongs only to Acme Studio |
| A6 | Engineer | Open the same invitation link again | Signs in or says the account exists; no second account |

## Part B — Desktop app as the engineer

### B0. Start clean

1. Quit any running Time Pilot (tray menu → **Quit Time Pilot**), including the copy in `/Applications`; only one instance can run.
2. Start the dev build with a short idle threshold:
   ```bash
   cd widget
   TIMEPILOT_IDLE_SECS=30 npm run tauri dev
   ```
3. If a previous account is signed in: tray → **Sign out**.

### B1. Browser sign-in (TP7, TP8)

| # | Do this | Expect |
|---|---------|--------|
| 1 | Look at the welcome window | Time Pilot mark, name, one-line promise, three points, one **Sign in** button (dev builds also show a Server field with `http://localhost:8000`) |
| 2 | **Sign in** | The default browser opens the Time Pilot sign-in page; the app shows "Waiting for your browser…" with **Cancel** |
| 3 | **Cancel** in the app | Back to **Sign in**; nothing saved |
| 4 | **Sign in** again; sign in as the engineer in the browser | The browser shows "You're signed in"; the app window closes and the auto-tracking window opens (no consent screen in the browser: Time Pilot's own app) |
| 5 | Quit and restart the app | Still signed in (session in the macOS keychain; allow keychain access if asked) |
| 6 | Sign in as an account that belongs to two organizations | The app asks which organization to track in |
| 7 | Server field `http://192.168.1.10:8000` (dev build) → **Sign in** | Refused: the server must use https:// (only localhost may use http) |

### B2. Switching and the tray (W2, W3, W4, W5, W10)

| # | Do this | Expect |
|---|---------|--------|
| 6 | Press **⌘⇧T** (Command+Shift+T) | Quick switcher opens and lists Acme Studio's projects (+ their tasks); private projects the engineer is not a member of are not shown (W4) |
| 7 | Type `pos back`, press **Enter** | Switcher closes instantly; tray icon turns **teal** (W2, W10) |
| 8 | Hover the tray icon | Tooltip `POS Revamp — 0:00` (short name + elapsed, W2) |
| 9 | ⌘⇧T → **Tab** to the description field → type `ABC-12 login fix` → Enter on `internal meet` | Timer switches to Internal · Meetings with that description; the POS entry ends at the same second (W5) |
| 10 | ⌘⇧T → click the description under the running timer, change it, press Enter | Description updated while running (W5) |
| 11 | Paste a multi-line text into a description | Saved as one line (max 500 characters) |
| 12 | Click the tray icon | Menu: status, **Stop timer**, up to 5 recent project/task pairs, Quick switch, Sign out, Quit (W3) |
| 13 | Click a recent item | Timer switches to it in one click (W3) |
| 14 | Menu → **Stop timer** | Icon turns grey; tooltip "Time Pilot — stopped" |

### B3. Server sync (W10, and what managers see)

| # | Do this | Expect |
|---|---------|--------|
| 15 | Wait up to 1 minute, then refresh **Time** in the engineer's browser | All entries from B2 appear with the right project, task, description and times (sync is batched at most once a minute) |
| 16 | Admin browser → **Time** or **Reports**, filter the engineer | Admin sees the engineer's entries; nothing else (no activity data) |
| 17 | Check the entries' fields | Only project, task, description, start, end and billable — no window titles, URLs or activity (privacy rules) |

### B4. Offline, crash and reboot (W9)

| # | Do this | Expect |
|---|---------|--------|
| 18 | `cd server/local && docker compose stop app` | Server down |
| 19 | Switch projects 3 times with ⌘⇧T | Still instant; tooltip shows "Offline · N waiting to sync" |
| 20 | Quit the widget (simulates crash/reboot), start `npm run tauri dev` again | Running timer and recent list intact |
| 21 | Tray → **Sign out** | Refused: "N entries not synced yet" (no data can be lost by signing out) |
| 22 | `docker compose start app`, wait ~1–2 min | All offline entries appear on the platform **once** — no gaps, no duplicates |

### B5. Idle, screen lock and sleep (W6, W7)

Threshold is 30 s because of `TIMEPILOT_IDLE_SECS=30` (default 10 min).

| # | Do this | Expect |
|---|---------|--------|
| 23 | Start a timer; don't touch keyboard or mouse for 40 s; then move the mouse | Within ~3 s a prompt appears top-right: "You were away for 0 min … the timer kept running on …". It does **not** take focus (typing continues in your current app) |
| 24 | Click **Keep** | Prompt closes; one continuous entry |
| 25 | Repeat 23, click **Discard** | The entry ends when you left; a new entry for the same project starts when you came back (a gap on the platform) |
| 26 | Repeat 23, click **Assign to…**, pick another project | The away time becomes its own entry on that project; the original timer continues |
| 27 | Lock the screen (**Ctrl+Cmd+Q**) for 40 s, unlock | Same prompt (W7) |
| 28 | Sleep the Mac (Apple menu → Sleep) for 1 min, wake | Same prompt (W7) |
| 29 | Stop the timer, stay idle 40 s, return | **No** prompt (only tracked time is checked) |
| 30 | Start a timer, idle 40 s, don't answer the prompt; click the tray icon | Menu shows "Away N min — Keep / Discard / Assign…"; clicking it brings the prompt back |

### B6. AI auto-tracking (AT1–AT8)

**Setup**

1. The local stack's Autotrack service needs `GEMINI_API_KEY` in `server/autotrack/.env` for live AI; see [server/autotrack/README.md](../server/autotrack/README.md).
2. As admin, open http://localhost:8010/admin/, **Sign in with Time Pilot**, and add keywords to two of the engineer's projects:
   - **Acme – POS Revamp:** `acme-pos, POS-`
   - **Acme – Mobile App:** `acme-mobile, MOB-`
3. Start the widget with fast timings:

   ```bash
   TIMEPILOT_AUTO_TICK_SECS=5 TIMEPILOT_CAPTURE_SECS=30 npm run tauri dev
   ```

   On the service, also set `TIMEPILOT_PIN_SECS=60`, so a manual start or stop pauses auto-switching for 1 minute instead of 30. Restart the service after changing these.

| # | Do this | Expect |
|---|---------|--------|
| 40 | Sign in (or tray → Auto-tracking → Set up…) | Auto-tracking window explains what is captured, when, where it goes, what is kept, and the excluded apps |
| 41 | Click **I agree, turn on auto-tracking** | Status "On"; macOS asks for Screen Recording. Allow Time Pilot in System Settings, quit and reopen the widget; the window now shows "allowed" |
| 42 | Stop any timer; open a Finder or editor window whose title contains `acme-pos` (e.g. a folder named `acme-pos`) | Within ~10 s a timer starts on POS Revamp; tray shows "· auto" |
| 43 | Switch to a window titled with `acme-mobile` and stay > 2 min | Timer switches to Mobile App, backdated to when you switched (check the start time in the review) |
| 44 | Switch to `acme-mobile` for only 30 s, then back | **No** switch (hysteresis) |
| 45 | Open a browser on a page about one of your projects without keywords in the title; wait ≥ 30 s | After the next allowed AI call the timer switches to that project (or a "Needs review" block if the AI is unsure). On the settings page → AI usage, a Flash-Lite call appears (and a Flash call if it was unsure) |
| 46 | Open Slack or Mail for 3 min | Timer stays on the current project (communication is neutral) |
| 47 | Open 1Password (or another excluded app) | No AI call in the usage log while it is in front; after 15 min a "Private app" review block |
| 48 | Pick a different project with ⌘⇧T | Timer switches; auto-tracking does not override it for the pin time (30 min by default, 1 min with `TIMEPILOT_PIN_SECS=60`; status "following your manual pick") |
| 49 | Tray → Auto-tracking → **Pause 30 minutes** | Timer stops; nothing is captured; status "Paused until …"; **Resume** restarts tracking |
| 50 | Stop the Autotrack service; keep working on a window without keywords for 15 min | Status shows "Autotrack service unreachable"; after 15 min the period becomes a "Needs review" block; no crash, no screenshot written anywhere (`ls ~/Library/Application\ Support/com.timepilot.desktop/`) |
| 51 | Sign out, sign in as another user | Auto-tracking is off again and asks for consent (consent and learned windows are per person) |

### B7. End-of-day review (W8)

| # | Do this | Expect |
|---|---------|--------|
| 52 | Tray → **Review today…** | Today's blocks with times, project, description, duration, auto/manual tag; gaps ≥ 5 min shown in italics; "Needs review" blocks highlighted |
| 53 | Click a "Needs review" block's name, pick a project | Block gets the project; within a minute it syncs to the platform. Next time the same window appears, auto-tracking picks that project without AI |
| 54 | Edit a description, press Enter | Saved and synced |
| 55 | Click × then **Delete?** on a block | Block removed (and deleted on the platform) |
| 56 | **Submit day** while a review block remains | Button disabled with a hint |
| 57 | Fix all, **Submit day** | "Submitted at HH:MM"; settings page → Day submissions shows the engineer as submitted |
| 58 | Set `TIMEPILOT_REVIEW_AT` to a minute from now on the service, restart widget | A small reminder appears top-right without taking focus and disappears after 8 s (or opens the review) |
| 59 | ‹ / › in the review | Previous days (up to 6 back) |

## Part C — Admin checks on the platform

| # | Do this | Expect |
|---|---------|--------|
| 31 | **Reports**, this week, group by member then project | Engineer's hours split by project; billable vs non-billable (Internal is non-billable) |
| 32 | **Projects → Acme – POS Revamp** | Spent time includes the engineer's entries |
| 33 | Try to remove the engineer from POS Revamp while they have a running timer on it, then press ⌘⇧T in the widget | After the next catalog refresh (≤ 10 min) POS Revamp disappears from the switcher |

## Not testable yet

Reminders (W11), ticket keys and calendar suggestions (W13–W14), auto-tracking on Windows devices, signed auto-update (W15), and the reconciliation service (R1–R6, billing and utilization exports). See [STATUS.md](STATUS.md).

## Reporting a problem

Note the step number, what you expected and what happened. Useful extra detail:

```bash
# desktop app's local queue (unsynced rows have dirty=1; rejected rows have an error)
sqlite3 ~/Library/Application\ Support/com.timepilot.desktop/tracker.db \
  "select start, end, description, dirty, error from entry order by start desc limit 10;"
# server logs
cd server/local && docker compose logs --tail 50 app
```
