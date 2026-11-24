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

## Employee accounts

The server has no email (SMTP) yet, so solidtime's own email confirmation can't work. There are two ways to give people an account.

**Self-registration (currently on, for testing).** People sign up at `https://<domain>/register`.

- Every minute, `onboard.sh` (cron) activates new accounts whose address matches `REGISTRATION_DOMAINS`. It checks four times a minute, so a new account works about 15 seconds after sign-up; reload the page if it still asks to confirm the email.
- It adds each new account to the company organization as an employee and removes the empty personal organization solidtime made for them. The app then signs them straight into the company.
- Log: `/opt/ira-data/onboard.log`.
- `REGISTRATION_DOMAINS` is a comma-separated list of domains (for example `iratechnologies.com,aasnatech.com`), or `*` for any address. The default is the admin's domain.

```bash
# on, for any address (current testing setup)
HOST=… KEY=… REGISTRATION=on REGISTRATION_DOMAINS='*' deploy/linux/deploy.sh
# only company addresses
HOST=… KEY=… REGISTRATION=on REGISTRATION_DOMAINS=iratechnologies.com deploy/linux/deploy.sh
# off
HOST=… KEY=… REGISTRATION=off deploy/linux/deploy.sh
```

Without email nobody proves they own the address. With `*`, anyone who finds the site can sign up and becomes an employee who sees project names. Check **Members** regularly and remove strangers.

After SMTP is set up (the `MAIL_*` settings in `/opt/ira-data/laravel.env`), switch to `REGISTRATION=invite-only`. Admins then invite people under **Members**, people confirm their email, and `onboard.sh` stops.

**Admin adds a person (no sign-up needed).** One command prints a login to send privately:

```bash
ssh -i <ssh-key.pem> <user>@<server-ip> \
  'sudo /opt/ira-tracker/deploy/linux/add-user.sh "Full Name" name@company.com employee'
```

Roles: `employee`, `manager` or `admin`.

## AI key

Create a Gemini API key at https://aistudio.google.com (Get API key). Save it to a file on the Mac, then run the deploy again with that file:

```bash
pbpaste > ~/ira-gemini-key.txt && chmod 600 ~/ira-gemini-key.txt   # after copying the key
HOST=… KEY=… GEMINI_API_KEY_FILE=~/ira-gemini-key.txt deploy/linux/deploy.sh
```

The key is sent over SSH on stdin and written only to `/opt/ira-data/ira-service.env`. It never appears on a command line, where the server's sudo log would record it.

Until a key is set, auto-tracking works from project keywords and learned windows only.

To use Claude instead, set `IRA_AI_PROVIDER=anthropic` and `ANTHROPIC_API_KEY=…` in `/opt/ira-data/ira-service.env`.

## Settings

`/opt/ira-data/ira-service.env` holds the screenshot interval (5 min), retention (3 days), the review reminder (18:30), models and thresholds. Edit it, then re-run the deploy. Widgets pick up changes within an hour.

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
