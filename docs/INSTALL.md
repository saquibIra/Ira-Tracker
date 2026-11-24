# Installing Ira Tracker

Ira Tracker sits in your menu bar (Mac) or system tray (Windows). It records which project you work on, so you don't have to start and stop timers. At the end of the day you check your time and submit it.

## 1. Get your API token (once)

1. Create your account at **https://tracker.aasnatech.com/register**. If the page asks you to confirm your email, wait 15 seconds and reload it. No email is sent. If your admin already sent you a login, sign in at **https://tracker.aasnatech.com** instead.
2. Click your name → **Profile** → **API Tokens** → **Create API Token**.
3. Name it "Ira Tracker", then copy the token. You need it in step 3.

## 2. Install

**Mac**

1. Download **https://tracker.aasnatech.com/download/IraTracker-mac.dmg**.
2. Open it and drag **Ira Tracker** into **Applications**.
3. Open Ira Tracker from Applications. The first time, macOS says it can't verify the developer. Go to **System Settings → Privacy & Security**, scroll down, and click **Open Anyway** next to Ira Tracker. You only do this once.

**Windows**

1. Download the installer from **https://tracker.aasnatech.com/download/**.
2. Run it. If Windows SmartScreen appears, click **More info → Run anyway**.

## 3. Sign in and turn on auto-tracking

1. Paste your API token and click **Sign in**. The server address is already filled in.
2. Read the **Auto-tracking** screen, which explains what is captured. Then click **I agree, turn on auto-tracking**.
3. **Mac only:**
   1. macOS asks for **Screen Recording**. Click **Open System Settings** and turn on **Ira Tracker**.
   2. Click **Quit & Reopen**.

That's it. The tray icon turns green when a timer runs, and switching between projects happens automatically.

## Every day

- **Fix things:** tray → **Review today…** shows your blocks and screenshots. You can change a project, edit times, or split a block.
- **Private time:** tray → **Auto-tracking → Pause** stops tracking until you resume.
- **Pick by hand:** **⌘⇧T** (Mac) or **Ctrl+Shift+T** (Windows) picks a project yourself. Auto-tracking respects your choice for 30 minutes.
- **Submit:** at 18:30 a reminder asks you to check and **Submit** your day.

## Privacy

- **When:** a small grayscale screenshot every 5 minutes while tracking, whenever you work. Pause it for private time.
- **Where it's kept:** only on **your** computer, for 3 days. You can delete any of them in the review.
- **The AI:** when the window title isn't enough, the screenshot is sent to Ira's server, which asks the AI which project it is. The server doesn't keep it.
- **Never captured:** keystrokes, mouse activity, password managers, personal messengers, or anything while paused or locked.

## Problems?

Tray → **Open logs folder**, then send `ira-tracker.log` to your admin. It contains no screenshots or window titles.
