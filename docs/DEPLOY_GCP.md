# Deploying to Google Cloud

Everything runs on **one Compute Engine VM** with Docker:

| Address | What |
|---|---|
| `https://<domain>/` | solidtime (web app and the API the widget uses) |
| `https://<domain>/ira/` | Ira service (AI auto-tracking); admin page at `/ira/admin/` |
| `https://<domain>/download/` | installers for employees |

- **HTTPS:** Caddy obtains the certificate automatically.
- **AI:** Gemini on Vertex AI, using the VM's service account, so there is no API key to manage.
- **Logs:** go to Cloud Logging.
- **Backups:** daily disk snapshots are kept for 14 days.

**Cost:** about $50 a month for the VM (e2-standard-2) plus a few dollars for disk and snapshots. Gemini costs roughly $1 per engineer per month.

## What you need

- A Google Cloud project with billing enabled. Your account needs the Owner role, or Editor plus Security Admin.
- A domain name for the server, e.g. `tracker.aasnatech.com`, and access to its DNS settings.
- The `gcloud` CLI on your Mac, signed in: `brew install --cask google-cloud-sdk && gcloud auth login`.

## 1. Create the server (once, about 10 minutes)

```bash
PROJECT=<project-id> DOMAIN=time.example.com ADMIN_EMAIL=you@example.com deploy/gcp/setup.sh
```

**What the script creates:**

- **Service account** `ira-tracker-vm`, with only three roles: Vertex AI user, log writer and metric writer.
- **Network:** a fixed IP address, and a firewall rule that allows HTTP and HTTPS only.
- **Backups:** a daily snapshot schedule, kept 14 days.
- **The VM itself**, in `asia-south1` (Mumbai) by default.

**What the VM does on first boot** (`deploy/gcp/bootstrap.sh`):

1. Installs Docker.
2. Clones this repository.
3. Generates all secrets in `/opt/ira-data` (database password, solidtime keys). These are never stored in git.
4. Starts the stack.
5. Runs the database migrations.
6. Creates the admin user.

Then:

1. **DNS:** create an A record `time.example.com → <IP printed by the script>`.
2. **Watch the setup:**
   ```bash
   gcloud compute ssh ira-tracker --zone asia-south1-a -- sudo journalctl -u google-startup-scripts -f
   ```
3. **Admin password:**
   ```bash
   gcloud compute ssh ira-tracker --zone asia-south1-a -- sudo cat /opt/ira-data/admin-credentials.txt
   ```
4. **Admin setup:** sign in at `https://time.example.com`, create the organization, projects and clients, and invite employees.
5. **Project hints:** open `https://time.example.com/ira/admin/` and add keywords for each project, such as repository names and ticket prefixes.

## 2. Settings

Production settings are in `/opt/ira-data/ira-service.env` on the VM. The template is `deploy/gcp/ira-service.env.template`.

**Defaults:**

- **Screenshots:** one every 5 minutes while tracking, kept 3 days on each laptop.
- **Work hours:** none; people track whenever they work.
- **Day review reminder:** 18:30.
- **AI:** Gemini `gemini-3.5-flash-lite`, re-checked by `gemini-3.8-flash` when unsure.

To change a setting, edit the file and run the update (step 4). Widgets pick up new settings within an hour.

## 3. Logs

All containers log to **Cloud Logging**: Console → Logging → Logs Explorer → resource "VM Instance" `ira-tracker`.

The Ira service writes one JSON line per request (`request_id`, path, status, ms, member) and one line per classification:

- **Included:** model, outcome, confidence, tokens, latency.
- **Never included:** window titles, screenshots, tokens.

Useful queries:

```
jsonPayload.logger="ira" severity>=WARNING
jsonPayload.message="classified"
jsonPayload.request_id="<id from the widget log>"
```

On the VM: `cd /opt/ira-tracker/deploy/gcp && sudo docker compose --env-file /opt/ira-data/compose.env logs -f ira-service`.

Each engineer's app keeps its own log. Tray → **Open logs folder**: `ira-tracker.log`, rotated at 1 MB.

## 4. Updating

After changes are merged into `main`:

```bash
gcloud compute ssh ira-tracker --zone asia-south1-a -- sudo /opt/ira-tracker/deploy/gcp/update.sh
```

## 5. Installers for employees

```bash
IRA_SERVER_URL=https://time.example.com PUBLISH=1 scripts/release-mac.sh
```

This builds the universal Mac app with the server address built in, signed with the local signing identity, and uploads it to `https://<domain>/download/IraTracker-mac.dmg`.

**Windows:**

1. Set the GitHub repository variable `IRA_SERVER_URL`.
2. Run Actions → **Release (Windows)**.
3. Download the installer artifact and copy it to `/opt/ira-data/downloads/` on the VM.

Employees follow [INSTALL.md](INSTALL.md).

## Restore from a snapshot

Console → Compute Engine → Snapshots → choose a date → **Create disk**, then create a VM from that disk with the same service account and network tag (`ira-tracker`). Finally, point the static IP at the new VM.
