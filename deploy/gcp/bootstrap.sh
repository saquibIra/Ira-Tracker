#!/usr/bin/env bash
# VM startup script (runs as root on every boot; idempotent). Installs Docker, fetches
# the code, creates secrets and settings on first boot, and starts the stack.
set -euo pipefail

APP=/opt/ira-tracker
DATA=/opt/ira-data
META=http://metadata.google.internal/computeMetadata/v1
meta() { curl -fs -H "Metadata-Flavor: Google" "$META/instance/attributes/$1"; }
PROJECT=$(curl -fs -H "Metadata-Flavor: Google" "$META/project/project-id")
DOMAIN=$(meta domain)
ADMIN_EMAIL=$(meta admin-email)
REPO=$(meta repo)
BRANCH=$(meta branch || echo main)

log() { echo "[ira-bootstrap] $*"; }

# 1. Docker (official repository)
if ! command -v docker >/dev/null; then
  log "installing Docker"
  apt-get update -q
  apt-get install -yq ca-certificates curl git openssl
  install -m 0755 -d /etc/apt/keyrings
  curl -fsSL https://download.docker.com/linux/debian/gpg -o /etc/apt/keyrings/docker.asc
  echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.asc] https://download.docker.com/linux/debian $(. /etc/os-release && echo "$VERSION_CODENAME") stable" \
    >/etc/apt/sources.list.d/docker.list
  apt-get update -q
  apt-get install -yq docker-ce docker-ce-cli containerd.io docker-compose-plugin
fi

# 2. Code (first boot only; later updates: update.sh)
if [ ! -d "$APP/.git" ]; then
  log "cloning $REPO ($BRANCH)"
  git clone --branch "$BRANCH" "$REPO" "$APP"
fi

# 3. Secrets and settings (first boot only; never in the repository)
install -d -m 700 "$DATA"
install -d -m 755 "$DATA/downloads"
if [ ! -f "$DATA/compose.env" ]; then
  log "creating $DATA/compose.env"
  cat >"$DATA/compose.env" <<EOF
DOMAIN=$DOMAIN
ACME_EMAIL=$ADMIN_EMAIL
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

if [ ! -f "$DATA/laravel.env" ]; then
  log "creating $DATA/laravel.env (new application keys)"
  {
    cat <<EOF
APP_NAME="Ira Tracker"
VITE_APP_NAME="Ira Tracker"
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
MAIL_FROM_NAME="Ira Tracker"
QUEUE_CONNECTION="database"
FILESYSTEM_DISK="local"
PUBLIC_FILESYSTEM_DISK="public"
GOTENBERG_URL="http://gotenberg:3000"
EOF
    docker run --rm "solidtime/solidtime:${SOLIDTIME_IMAGE_TAG:-latest}" php artisan self-host:generate-keys
  } >"$DATA/laravel.env"
  chmod 600 "$DATA/laravel.env"
fi

if [ ! -f "$DATA/ira-service.env" ]; then
  log "creating $DATA/ira-service.env"
  sed "s/__PROJECT__/$PROJECT/" "$APP/deploy/gcp/ira-service.env.template" >"$DATA/ira-service.env"
  chmod 600 "$DATA/ira-service.env"
fi

# 4. Start (build the Ira service image from the repository)
cd "$APP/deploy/gcp"
compose() { docker compose --env-file "$DATA/compose.env" "$@"; }
log "starting containers"
compose up -d --build

# 5. Database setup on first run
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
  compose exec -T scheduler php artisan admin:user:create "Ira Admin" "$ADMIN_EMAIL" --verify-email \
    >"$DATA/admin-credentials.txt"
  chmod 600 "$DATA/admin-credentials.txt"
fi
log "ready: https://$DOMAIN"
