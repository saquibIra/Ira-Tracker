#!/usr/bin/env bash
# Installs or updates Time Pilot on a Linux server with Docker. Run as root from the
# copied code (deploy.sh does this for you):
#
#   sudo DOMAIN=time.example.com ADMIN_EMAIL=you@example.com \
#        [GEMINI_API_KEY_STDIN=1 (key on the first line of stdin)] [REGISTRATION=on|off|invite-only] \
#        [REGISTRATION_DOMAINS='example.com,other.com' or '*'] \
#        /opt/time-pilot/deploy/linux/install.sh
#
# Safe to re-run: secrets and settings in /opt/timepilot-data are created once and kept.
# REGISTRATION and REGISTRATION_DOMAINS are remembered in compose.env (default: off).
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
DATA="${DATA_DIR:-/opt/timepilot-data}"
REQUESTED_DOMAIN="${DOMAIN:-}"
REQUESTED_REGISTRATION="${REGISTRATION:-}"
REQUESTED_REGISTRATION_DOMAINS="${REGISTRATION_DOMAINS:-}"
log() { echo "[timepilot-install] $*"; }

# The AI key comes on stdin (deploy.sh), never as GEMINI_API_KEY=... on the sudo command
# line, which the system log records. Read it before anything else can consume stdin.
if [ "${GEMINI_API_KEY_STDIN:-}" = 1 ]; then IFS= read -r GEMINI_API_KEY || true; fi
[[ "${GEMINI_API_KEY:-}" =~ ^[A-Za-z0-9._-]*$ ]] || { echo "GEMINI_API_KEY: unexpected characters"; exit 1; }

case "$REQUESTED_REGISTRATION" in "" | on | off | invite-only) ;; *) echo "REGISTRATION must be on, off or invite-only"; exit 1 ;; esac
[[ "$REQUESTED_REGISTRATION_DOMAINS" =~ ^(\*|[A-Za-z0-9.,-]*)$ ]] || { echo "REGISTRATION_DOMAINS: domains separated by commas, or *"; exit 1; }
command -v docker >/dev/null || { echo "Docker is required"; exit 1; }
docker compose version >/dev/null || { echo "Docker Compose v2 is required"; exit 1; }

install -d -m 700 "$DATA"
install -d -m 700 "$DATA/backups"
install -d -m 755 "$DATA/downloads"

# 1. Secrets and settings (first run only; never in the repository)
if [ ! -f "$DATA/compose.env" ]; then
  : "${DOMAIN:?set DOMAIN on the first install}"
  : "${ADMIN_EMAIL:?set ADMIN_EMAIL on the first install}"
  log "creating $DATA/compose.env"
  cat >"$DATA/compose.env" <<EOF
DOMAIN=$DOMAIN
ACME_EMAIL=$ADMIN_EMAIL
ADMIN_EMAIL=$ADMIN_EMAIL
DATA_DIR=$DATA
DB_DATABASE=solidtime
DB_USERNAME=solidtime
DB_PASSWORD=$(openssl rand -hex 24)
SOLIDTIME_IMAGE_TAG=latest
EOF
  chmod 600 "$DATA/compose.env"
fi
# shellcheck disable=SC1091
. "$DATA/compose.env"

# Domain change on an existing installation (DOMAIN=new.example.com install.sh).
RECREATE=""
if [ -n "$REQUESTED_DOMAIN" ] && [ "$REQUESTED_DOMAIN" != "$DOMAIN" ]; then
  log "changing domain $DOMAIN -> $REQUESTED_DOMAIN"
  sed -i "s|^DOMAIN=.*|DOMAIN=$REQUESTED_DOMAIN|" "$DATA/compose.env"
  if [ -f "$DATA/laravel.env" ]; then
    sed -i -e "s|^APP_URL=.*|APP_URL=\"https://$REQUESTED_DOMAIN\"|" \
      -e "s|^MAIL_FROM_ADDRESS=.*|MAIL_FROM_ADDRESS=\"no-reply@$REQUESTED_DOMAIN\"|" "$DATA/laravel.env"
  fi
  DOMAIN="$REQUESTED_DOMAIN"
  RECREATE="--force-recreate"
fi

# Remembered settings: add or replace KEY=value in compose.env.
remember() {
  if grep -q "^$1=" "$DATA/compose.env"; then
    sed -i "s|^$1=.*|$1=$2|" "$DATA/compose.env"
  else
    echo "$1=$2" >>"$DATA/compose.env"
  fi
}
if [ -n "$REQUESTED_REGISTRATION" ]; then
  remember REGISTRATION "$REQUESTED_REGISTRATION"
  REGISTRATION="$REQUESTED_REGISTRATION"
fi
if [ -n "$REQUESTED_REGISTRATION_DOMAINS" ]; then
  remember REGISTRATION_DOMAINS "$REQUESTED_REGISTRATION_DOMAINS"
  REGISTRATION_DOMAINS="$REQUESTED_REGISTRATION_DOMAINS"
fi
REGISTRATION="${REGISTRATION:-off}"

if [ ! -f "$DATA/laravel.env" ]; then
  log "creating $DATA/laravel.env (new application keys)"
  docker pull -q "solidtime/solidtime:${SOLIDTIME_IMAGE_TAG:-latest}" >/dev/null
  {
    cat <<EOF
APP_NAME="Time Pilot"
VITE_APP_NAME="Time Pilot"
APP_ENV="production"
APP_DEBUG="false"
APP_URL="https://$DOMAIN"
APP_FORCE_HTTPS="true"
TRUSTED_PROXIES="0.0.0.0/0,2000:0:0:0:0:0:0:0/3"
SUPER_ADMINS="$ADMIN_EMAIL"
LOG_CHANNEL="stderr"
LOG_LEVEL="info"
DB_CONNECTION="pgsql"
DB_HOST="database"
DB_PORT="5432"
DB_SSL_MODE="prefer"
DB_DATABASE="$DB_DATABASE"
DB_USERNAME="$DB_USERNAME"
DB_PASSWORD="$DB_PASSWORD"
MAIL_MAILER="log"
MAIL_FROM_ADDRESS="no-reply@$DOMAIN"
MAIL_FROM_NAME="Time Pilot"
QUEUE_CONNECTION="database"
FILESYSTEM_DISK="local"
PUBLIC_FILESYSTEM_DISK="public"
GOTENBERG_URL="http://gotenberg:3000"
EOF
    docker run --rm "solidtime/solidtime:${SOLIDTIME_IMAGE_TAG:-latest}" php artisan self-host:generate-keys
  } >"$DATA/laravel.env"
  chmod 600 "$DATA/laravel.env"
fi

# Registration on the sign-in page (solidtime reads its settings when a container starts).
if ! grep -qx "APP_ENABLE_REGISTRATION=\"$REGISTRATION\"" "$DATA/laravel.env"; then
  log "registration: $REGISTRATION"
  sed -i '/^APP_ENABLE_REGISTRATION=/d' "$DATA/laravel.env"
  echo "APP_ENABLE_REGISTRATION=\"$REGISTRATION\"" >>"$DATA/laravel.env"
  RECREATE="--force-recreate"
fi

# Writes GEMINI_API_KEY into $1 (mode 600). The key reaches awk through its environment,
# never through process arguments (visible in `ps`).
write_key() {
  (umask 077 && K="${GEMINI_API_KEY:-}" awk \
    '/^GEMINI_API_KEY=/ { print "GEMINI_API_KEY=" ENVIRON["K"]; next } { print }' "$1" >"$1.new")
  mv "$1.new" "$1"
}
if [ ! -f "$DATA/autotrack.env" ]; then
  log "creating $DATA/autotrack.env"
  cp "$HERE/autotrack.env.template" "$DATA/autotrack.env"
  write_key "$DATA/autotrack.env"
elif [ -n "${GEMINI_API_KEY:-}" ]; then
  log "updating GEMINI_API_KEY"
  write_key "$DATA/autotrack.env"
fi

cp "$HERE/downloads-index.html" "$DATA/downloads/index.html"
sed -i "s|__DOMAIN__|$DOMAIN|g" "$DATA/downloads/index.html"

# 2. Start or update
cd "$HERE"
compose() { docker compose --env-file "$DATA/compose.env" "$@"; }
log "starting containers (first run downloads ~5 GB of images)"
compose pull --quiet --ignore-buildable
compose up -d --build --remove-orphans $RECREATE

# 3. Database
for _ in $(seq 1 60); do
  compose exec -T database pg_isready -q -U "$DB_USERNAME" && break
  sleep 2
done
compose exec -T scheduler php artisan migrate --force
if [ ! -f "$DATA/.passport-client" ]; then
  compose exec -T scheduler php artisan passport:client --personal --name="API" -n
  touch "$DATA/.passport-client"
fi
if [ ! -f "$DATA/admin-credentials.txt" ]; then
  compose exec -T scheduler php artisan admin:user:create "Time Pilot Admin" "$ADMIN_EMAIL" --verify-email \
    >"$DATA/admin-credentials.txt"
  chmod 600 "$DATA/admin-credentials.txt"
fi

# 4. Nightly backup at 02:30 (kept 14 days in $DATA/backups)
cat >/etc/cron.d/time-pilot-backup <<EOF
30 2 * * * root $HERE/backup.sh >>$DATA/backups/backup.log 2>&1
EOF
chmod 644 /etc/cron.d/time-pilot-backup

# 5. Self-registration without email: activate new accounts and add them to the company
#    (onboard.sh; only while REGISTRATION=on).
if [ "$REGISTRATION" = on ]; then
  log "self-registration on for: ${REGISTRATION_DOMAINS:-${ADMIN_EMAIL#*@}} (log: $DATA/onboard.log)"
  echo "* * * * * root $HERE/onboard.sh >>$DATA/onboard.log 2>&1" >/etc/cron.d/time-pilot-onboard
  chmod 644 /etc/cron.d/time-pilot-onboard
else
  rm -f /etc/cron.d/time-pilot-onboard
fi

docker image prune -f >/dev/null
log "ready: https://$DOMAIN (admin login in $DATA/admin-credentials.txt)"
