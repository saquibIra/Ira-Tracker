# Deploying to a Linux server (client-provided)

This guide is for any Ubuntu or Debian server with Docker. Production runs on the client's server (Ubuntu 24.04, 2 vCPU, 8 GB RAM, 96 GB disk) at **https://tracker.aasnatech.com**. The server IP, SSH user and key are kept outside this public repository.

| Address | What |
|---|---|
| `https://<domain>/` | solidtime (web app and the API the widget uses) |
| `https://<domain>/ira/` | Ira service (AI auto-tracking); admin page at `/ira/admin/` |
| `https://<domain>/download/` | installers and instructions for employees |

The server cannot reach GitHub, so the code is copied from a Mac with rsync over SSH.

## Requirements

- **Server:** 2 vCPU, 4 GB RAM minimum (8 GB recommended), 40 GB disk, Ubuntu 22.04+/Debian 12, Docker with Compose v2.
- **Network:**
  - Inbound: ports 80 and 443.
  - Outbound HTTPS: Docker Hub, PyPI, Let's Encrypt and the AI provider.
- **Access:** SSH with a sudo user.
- **DNS:** an A record `<domain> → server IP`. For aasnatech.com this is GoDaddy → My Products → DNS.

## Deploy or update (from the Mac, in the repository root)

```bash
HOST=<user>@<server-ip> KEY=<ssh-key.pem> \
  DOMAIN=tracker.aasnatech.com ADMIN_EMAIL=<admin email> deploy/linux/deploy.sh
```

`DOMAIN` and `ADMIN_EMAIL` are only needed the first time. Later updates are `HOST=… KEY=… deploy/linux/deploy.sh`.

**On the first run, `install.sh` does the following:**

1. Creates `/opt/ira-data` (root-only): the database password, solidtime keys and settings. None of these are in git.
2. Starts the stack: solidtime, Postgres, the PDF service, the Ira service, and Caddy (automatic HTTPS).
3. Runs the migrations.
4. Creates the admin user. The password is in `/opt/ira-data/admin-credentials.txt`.
5. Installs a nightly backup (cron, 02:30).

**Admin password:**

```bash
ssh -i <ssh-key.pem> <user>@<server-ip> sudo cat /opt/ira-data/admin-credentials.txt
```

## AI key

Create a Gemini API key at https://aistudio.google.com (Get API key), then run the deploy again with `GEMINI_API_KEY=…`.

Until a key is set, auto-tracking works from project keywords and learned windows only.

To use Claude instead, set `IRA_AI_PROVIDER=anthropic` and `ANTHROPIC_API_KEY=…` in `/opt/ira-data/ira-service.env`.

## Settings

`/opt/ira-data/ira-service.env` holds the screenshot interval (5 min), retention (3 days), work hours (Mon–Fri 09:00–19:00), the review reminder (18:30), models and thresholds. Edit it, then re-run the deploy. Widgets pick up changes within an hour.

## Logs

```bash
ssh -i KEY ubuntu@SERVER
cd /opt/ira-tracker/deploy/linux
sudo docker compose --env-file /opt/ira-data/compose.env logs -f ira-service   # or app, caddy, queue
```

**What the Ira service logs:**

- One JSON line per request: `request_id`, path, status, ms, member.
- One line per AI classification.
- Never window titles, screenshots or tokens.

Logs rotate at 10 MB × 5 per container. Each laptop keeps its own log: tray → **Open logs folder**.

## Backups and restore

**Where:** nightly at 02:30, into `/opt/ira-data/backups/`, kept 14 days. Log: `backup.log`. Copy them off the server regularly, or ask the client for server snapshots.

**Restore solidtime:**

```bash
cd /opt/ira-tracker/deploy/linux; C="docker compose --env-file /opt/ira-data/compose.env"
sudo $C stop app scheduler queue
gunzip -c /opt/ira-data/backups/solidtime-YYYYMMDD-HHMM.sql.gz | sudo $C exec -T database psql -U solidtime -d solidtime
sudo $C start app scheduler queue
```

## Installers for employees

```bash
IRA_SERVER_URL=https://tracker.aasnatech.com PUBLISH_HOST=<user>@<server-ip> \
  PUBLISH_KEY=<ssh-key.pem> scripts/release-mac.sh
```

- **Windows:** pushing a tag `v*` builds the installer on GitHub Actions and attaches it to the GitHub release. Download it, then copy it to `/opt/ira-data/downloads/IraTracker-windows.exe`.
- **Employees:** open `https://<domain>/download/` and follow the steps there (also in [INSTALL.md](INSTALL.md)).
