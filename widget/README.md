# Ira Tracker (widget) — dev testing guide

## 1. Start the server (solidtime)

```bash
cd ~/Desktop/ira_tracker/server/solidtime
docker compose up -d
```

Check http://localhost:8000 loads. Login and API token are in `server/solidtime/.admin-credentials`.

## 2. Start the widget in dev mode

```bash
cd ~/Desktop/ira_tracker/widget
npm run tauri dev
```

The first run compiles for a few minutes; later runs take seconds. Keep this terminal open: the widget runs as long as it does, and logs appear here. `Ctrl+C` stops it.

The widget has no main window and no Dock icon. It lives in the **menu bar** (top right). If you can't see it, the menu bar is probably full and it is hidden behind the notch.

## 3. Sign in (first run only)

A sign-in window opens. Paste the API token from `.admin-credentials` and click **Sign in**. The window closes on success. If macOS asks to access the keychain, click **Always Allow** (it asks again after code changes in dev mode).

## 4. Test checklist

| # | Do this | Expect |
|---|---------|--------|
| 1 | Press **⌘⇧T** | Quick switcher opens with Recent and All projects |
| 2 | Type `pos back`, press **Enter** | Switcher closes; menu bar shows `POS Revamp 0:00` |
| 3 | Refresh solidtime in the browser | Running timer on Acme – POS Revamp · Backend within ~5 s |
| 4 | ⌘⇧T, click the description under the running timer, type text, **Enter** | Description appears in solidtime |
| 5 | ⌘⇧T, pick another project, **Tab**, type a description, **Enter** | First entry stops, second starts; both in solidtime, no overlap |
| 6 | Click the menu bar icon | Menu shows status, Stop, your recent projects |
| 7 | Click a recent project in the menu | Timer switches to it |
| 8 | Menu → **Stop timer** | Menu bar text disappears; entry ends in solidtime |
| 9 | **Offline:** `docker compose stop app` (in `server/solidtime`), switch projects 2–3 times | Switching still instant; tooltip says "Offline · N waiting to sync" |
| 10 | `docker compose start app`, wait ~1 min | All offline entries appear in solidtime, no duplicates |
| 11 | Quit (menu → Quit), run `npm run tauri dev` again | Still signed in; running timer and recent list intact |

## 5. Useful commands

```bash
# Local queue (what the widget has stored / not yet synced)
sqlite3 ~/Library/Application\ Support/com.ira.tracker/tracker.db \
  "select project_id, description, start, end, dirty, error from entry order by start desc limit 10;"

# Reset the widget completely (sign-out + clear local data)
rm -rf ~/Library/Application\ Support/com.ira.tracker
security delete-generic-password -s com.ira.tracker -a solidtime-token

# Re-seed demo customers/projects (safe to repeat)
SOLIDTIME_TOKEN=<token> python3 ../server/seed/seed_demo.py
```

## Not in this build yet

Idle / screen-lock prompt (W6/W7), end-of-day review (W8), reminders (W11), separate running/stopped tray icon, signed release build.
