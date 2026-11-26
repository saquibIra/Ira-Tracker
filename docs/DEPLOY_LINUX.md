# Running Time Pilot on a Linux server

Production runs on one Ubuntu server with Docker at **https://tracker.aasnatech.com**. The server IP, SSH user and key are kept outside this public repository.

| Address | What |
|---|---|
| `https://<domain>/` | Time Pilot web platform and its API |
| `https://<domain>/admin` | Super Admin panel |
| `https://<domain>/autotrack/` | Autotrack service (AI auto-tracking); settings page at `/autotrack/admin/` |
| `https://<domain>/download/` | desktop app installers and instructions |

The code is copied from a Mac with rsync (the server cannot reach GitHub). The web platform image is built on the server from `server/web`, which needs Docker Hub and the npm registry.

## Requirements

- **Server:** 2 vCPU, 4 GB RAM minimum (8 GB recommended), 40 GB disk, Ubuntu 22.04+ or Debian 12, Docker with Compose v2.
- **Network:** inbound 80 and 443; outbound HTTPS to Docker Hub, npm, Let's Encrypt and the AI provider.
- **DNS:** an A record `<domain> → server IP` (for aasnatech.com: GoDaddy → My Products → DNS).

## Install or update (from the Mac, in the repository root)

```bash
HOST=<user>@<server-ip> KEY=<ssh-key.pem> \
  DOMAIN=tracker.aasnatech.com ADMIN_EMAIL=<super admin email> \
  GEMINI_API_KEY_FILE=<file with the Gemini key> deploy/linux/deploy.sh
```

`DOMAIN` and `ADMIN_EMAIL` are needed the first time only; later updates are `HOST=… KEY=… deploy/linux/deploy.sh`. `SOURCE_URL` (where the source code is published, required by the AGPL) defaults to this checkout's GitHub address.

**The first run creates an empty platform:**

1. `/opt/timepilot-data` (root only): database password, application keys and settings. None of it is in git.
2. The stack (Compose project `timepilot`): web platform, Postgres, PDF service, Autotrack, Caddy with automatic HTTPS.
3. The database tables.
4. **The Super Admin** (`ADMIN_EMAIL`), the only account. Password: `sudo cat /opt/timepilot-data/admin-credentials.txt`. Change it after the first sign-in (Profile).
5. The sign-in clients of the desktop app and the settings page, with their ids written into both services.
6. A nightly backup at 02:30.

Sign-up is by invitation only. Secrets (the AI key) travel to the server on SSH's input stream, never on a command line.

## Onboarding customers

1. Sign in at `https://<domain>` as the Super Admin, open `https://<domain>/admin` → **Organizations** → **New organization**: organization name, admin name and email, currency.
2. The admin's link to set a password opens right away (valid 7 days, works once): **Copy** it and send it to the admin (email, chat, …). The admin becomes the owner. Lost or expired: **Admin link**.
3. The admin adds projects and invites people under **Members**. After **Invite member** the invite link opens to copy and send; **Copy invite link** on a pending invitation shows it again. Each person sets a password on the link and installs the desktop app from `https://<domain>/download/`.
4. Forgotten password: the Super Admin opens `https://<domain>/admin` → **Users** → **Password link** and sends it.

The Super Admin does not become a member of customer organizations.

## Email

Email is off: messages are only written to the log, and the Super Admin and the organization admins copy and send the links themselves (see above). Real email comes later. Note that this server's network blocks outgoing SMTP (ports 587, 465 and 2525), so that needs the hosting provider to open the port, or an email service with an HTTPS API.

## AI key

Create a Gemini API key at https://aistudio.google.com in a Google project **with billing** (on the free tier, Google may use what is sent, including screenshots, to improve its products). Deploy with `GEMINI_API_KEY_FILE=<file>`. Without a key, auto-tracking works from project keywords and learned windows only. Claude instead: `TIMEPILOT_AI_PROVIDER=anthropic` and `ANTHROPIC_API_KEY` in `/opt/timepilot-data/autotrack.env`.

## Settings

`/opt/timepilot-data/autotrack.env`: screenshot interval (5 min), retention (3 days), review reminder (18:30), models and thresholds. Edit it, then re-run the deploy; desktop apps pick up changes within an hour.

## Logs

```bash
ssh -i KEY USER@SERVER
cd /opt/time-pilot/deploy/linux
sudo docker compose --env-file /opt/timepilot-data/compose.env logs -f autotrack   # or app, caddy, queue
```

The Autotrack service writes one JSON line per request (`request_id`, path, status, ms, member) and one per AI classification, never window titles, screenshots or tokens. Logs rotate at 10 MB × 5 per container. Each computer keeps its own log: tray → **Open logs folder**.

## Backups and restore

Nightly at 02:30 into `/opt/timepilot-data/backups/` (database dump and Autotrack data), kept 14 days. Copy them off the server regularly.

```bash
cd /opt/time-pilot/deploy/linux; C="docker compose --env-file /opt/timepilot-data/compose.env"
sudo $C stop app scheduler queue
gunzip -c /opt/timepilot-data/backups/solidtime-YYYYMMDD-HHMM.sql.gz | sudo $C exec -T database psql -U solidtime -d solidtime
sudo $C start app scheduler queue
```

## Desktop app installers

```bash
TIMEPILOT_SERVER_URL=https://tracker.aasnatech.com PUBLISH_HOST=<user>@<server-ip> \
  PUBLISH_KEY=<ssh-key.pem> scripts/release-mac.sh
```

**Windows:** pushing a tag `v*` builds the installer on GitHub Actions and attaches `TimePilot-windows.exe` to the release; copy it to `/opt/timepilot-data/downloads/`. People install from `https://<domain>/download/` ([INSTALL.md](INSTALL.md)).
