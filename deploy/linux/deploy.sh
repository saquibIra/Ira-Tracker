#!/usr/bin/env bash
# Deploys (or updates) Ira Tracker on a Linux server from this Mac, over SSH. The server
# does not need GitHub access: the code is copied with rsync.
#
#   HOST=ubuntu@1.2.3.4 KEY=~/.ssh/server.pem \
#     DOMAIN=time.example.com ADMIN_EMAIL=you@example.com deploy/linux/deploy.sh
#
# Later updates: the same command (DOMAIN/ADMIN_EMAIL are only needed the first time).
# Optional: GEMINI_API_KEY=... to set or change the AI key.
# Optional: REGISTRATION=on|off|invite-only and REGISTRATION_DOMAINS='a.com,b.com' or '*'
# (sign-up on the website; see onboard.sh). Remembered on the server.
set -euo pipefail

: "${HOST:?set HOST, e.g. ubuntu@1.2.3.4}"
: "${KEY:?set KEY, the SSH private key file}"
[[ "${REGISTRATION:-}" =~ ^(on|off|invite-only)?$ ]] || { echo "REGISTRATION must be on, off or invite-only"; exit 1; }
[[ "${REGISTRATION_DOMAINS:-}" =~ ^(\*|[A-Za-z0-9.,-]*)$ ]] || { echo "REGISTRATION_DOMAINS: domains separated by commas, or *"; exit 1; }
cd "$(dirname "$0")/../.."
SSH=(ssh -i "$KEY" -o BatchMode=yes)

echo "copying code to $HOST:/opt/ira-tracker"
"${SSH[@]}" "$HOST" "sudo install -d -m 755 /opt/ira-tracker"
rsync -azR --delete -e "ssh -i $KEY -o BatchMode=yes" --rsync-path="sudo rsync" \
  --exclude '.venv' --exclude '.env' --exclude '*.db' --exclude '*.db-*' --exclude '*.log' \
  --exclude '__pycache__' --exclude '.pytest_cache' --exclude '.ruff_cache' \
  deploy/linux server/ira-service "$HOST:/opt/ira-tracker/"

echo "installing on the server"
"${SSH[@]}" "$HOST" "sudo DOMAIN='${DOMAIN:-}' ADMIN_EMAIL='${ADMIN_EMAIL:-}' GEMINI_API_KEY='${GEMINI_API_KEY:-}' \
  REGISTRATION='${REGISTRATION:-}' REGISTRATION_DOMAINS='${REGISTRATION_DOMAINS:-}' bash /opt/ira-tracker/deploy/linux/install.sh"
