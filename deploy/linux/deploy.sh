#!/usr/bin/env bash
# Deploys (or updates) Time Pilot on a Linux server from this Mac, over SSH. The server
# does not need GitHub: the code is copied with rsync and the web app is built there.
#
#   HOST=ubuntu@1.2.3.4 KEY=~/.ssh/server.pem \
#     DOMAIN=time.example.com ADMIN_EMAIL=superadmin@example.com deploy/linux/deploy.sh
#
# DOMAIN and ADMIN_EMAIL (the Super Admin) are needed the first time only. Optional, and
# remembered on the server:
#   GEMINI_API_KEY_FILE=~/gemini.txt     the AI key (or GEMINI_API_KEY=...)
#   MAIL=smtp SMTP_HOST=smtp-relay.brevo.com SMTP_PORT=587 SMTP_USERNAME=... \
#     MAIL_FROM=no-reply@example.com SMTP_PASSWORD_FILE=~/smtp.txt
# Secrets travel on stdin, never on a command line (sudo logs those on the server).
set -euo pipefail

: "${HOST:?set HOST, e.g. ubuntu@1.2.3.4}"
: "${KEY:?set KEY, the SSH private key file}"

secret() { # $1 = value, $2 = file: the file wins; surrounding whitespace removed
  if [ -n "${2:-}" ]; then tr -d '[:space:]' <"$2"; else printf '%s' "${1:-}"; fi
}
AI_KEY="$(secret "${GEMINI_API_KEY:-}" "${GEMINI_API_KEY_FILE:-}")"
SMTP_PASS="$(secret "${SMTP_PASSWORD:-}" "${SMTP_PASSWORD_FILE:-}")"
[[ "$AI_KEY" =~ ^[A-Za-z0-9._-]*$ ]] || { echo "GEMINI_API_KEY: unexpected characters"; exit 1; }
[[ "$SMTP_PASS" =~ ^[A-Za-z0-9._~!@#%^*+=,:/-]*$ ]] || { echo "SMTP_PASSWORD: unexpected characters"; exit 1; }

# Non-secret settings end up on the remote command line, so only safe characters pass.
check() { [[ "${!1:-}" =~ $2 ]] || { echo "$1: unexpected value"; exit 1; }; }
check DOMAIN '^[A-Za-z0-9.-]*$'
check ADMIN_EMAIL '^([A-Za-z0-9._+-]+@[A-Za-z0-9.-]+)?$'
check MAIL '^(log|smtp)?$'
check SMTP_HOST '^[A-Za-z0-9.-]*$'
check SMTP_PORT '^[0-9]{0,5}$'
check SMTP_USERNAME '^[A-Za-z0-9._@+-]*$'
check MAIL_FROM '^([A-Za-z0-9._+-]+@[A-Za-z0-9.-]+)?$'

cd "$(dirname "$0")/../.."
SSH=(ssh -i "$KEY" -o BatchMode=yes)

echo "copying code to $HOST:/opt/time-pilot"
"${SSH[@]}" "$HOST" "sudo install -d -m 755 /opt/time-pilot"
rsync -azR --delete -e "ssh -i $KEY -o BatchMode=yes" --rsync-path="sudo rsync" \
  --exclude '.venv' --exclude '.env' --exclude '*.db' --exclude '*.db-*' --exclude '*.log' \
  --exclude '__pycache__' --exclude '.pytest_cache' --exclude '.ruff_cache' --exclude 'node_modules' \
  deploy/linux server/autotrack server/web "$HOST:/opt/time-pilot/"

echo "installing on the server"
{
  if [ -n "$AI_KEY" ]; then printf 'GEMINI_API_KEY=%s\n' "$AI_KEY"; fi
  if [ -n "$SMTP_PASS" ]; then printf 'SMTP_PASSWORD=%s\n' "$SMTP_PASS"; fi
} | "${SSH[@]}" "$HOST" "sudo DOMAIN='${DOMAIN:-}' ADMIN_EMAIL='${ADMIN_EMAIL:-}' MAIL='${MAIL:-}' \
  SMTP_HOST='${SMTP_HOST:-}' SMTP_PORT='${SMTP_PORT:-}' SMTP_USERNAME='${SMTP_USERNAME:-}' \
  MAIL_FROM='${MAIL_FROM:-}' SECRETS_STDIN=1 bash /opt/time-pilot/deploy/linux/install.sh"
