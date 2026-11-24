#!/usr/bin/env bash
# Installs or updates Time Pilot on a Linux server with Docker. Run as root from the copied
# code; deploy/linux/deploy.sh does this for you from a Mac.
#
#   sudo DOMAIN=time.example.com ADMIN_EMAIL=you@example.com \
#        [MAIL=log|smtp SMTP_HOST=... SMTP_PORT=587 SMTP_USERNAME=... MAIL_FROM=...] \
#        [SECRETS_STDIN=1]   # GEMINI_API_KEY=... and SMTP_PASSWORD=... lines on stdin
#        /opt/time-pilot/deploy/linux/install.sh
#
# The first run creates an empty platform with one Super Admin (ADMIN_EMAIL; password in
# $DATA/admin-credentials.txt). Safe to re-run: secrets and settings in $DATA are created
# once and kept; settings passed again are remembered in compose.env.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
DATA="${DATA_DIR:-/opt/timepilot-data}"
log() { echo "[timepilot-install] $*"; }

# ---- secrets: on stdin (never on a command line, which the system log records) --------
GEMINI_API_KEY="${GEMINI_API_KEY:-}"
SMTP_PASSWORD="${SMTP_PASSWORD:-}"
if [ "${SECRETS_STDIN:-}" = 1 ]; then
  while IFS= read -r line || [ -n "$line" ]; do
    case "$line" in
      GEMINI_API_KEY=*) GEMINI_API_KEY="${line#GEMINI_API_KEY=}" ;;
      SMTP_PASSWORD=*) SMTP_PASSWORD="${line#SMTP_PASSWORD=}" ;;
    esac
  done
fi
[[ "$GEMINI_API_KEY" =~ ^[A-Za-z0-9._-]*$ ]] || { echo "GEMINI_API_KEY: unexpected characters"; exit 1; }
[[ "$SMTP_PASSWORD" =~ ^[A-Za-z0-9._~!@#%^*+=,:/-]*$ ]] || { echo "SMTP_PASSWORD: use letters, digits and ._~!@#%^*+=,:/- only"; exit 1; }

# ---- other settings (not secret) -------------------------------------------------------
REQ_DOMAIN="${DOMAIN:-}"
REQ_MAIL="${MAIL:-}"
REQ_SMTP_HOST="${SMTP_HOST:-}"
REQ_SMTP_PORT="${SMTP_PORT:-}"
REQ_SMTP_USERNAME="${SMTP_USERNAME:-}"
REQ_MAIL_FROM="${MAIL_FROM:-}"
case "$REQ_MAIL" in "" | log | smtp) ;; *) echo "MAIL must be log or smtp"; exit 1 ;; esac
[[ "$REQ_SMTP_HOST" =~ ^[A-Za-z0-9.-]*$ ]] || { echo "SMTP_HOST: a host name"; exit 1; }
[[ "$REQ_SMTP_PORT" =~ ^[0-9]{0,5}$ ]] || { echo "SMTP_PORT: a number"; exit 1; }
[[ "$REQ_SMTP_USERNAME" =~ ^[A-Za-z0-9._@+-]*$ ]] || { echo "SMTP_USERNAME: unexpected characters"; exit 1; }
[[ "$REQ_MAIL_FROM" =~ ^([A-Za-z0-9._+-]+@[A-Za-z0-9.-]+)?$ ]] || { echo "MAIL_FROM: an email address"; exit 1; }
command -v docker >/dev/null || { echo "Docker is required"; exit 1; }
docker compose version >/dev/null || { echo "Docker Compose v2 is required"; exit 1; }

install -d -m 700 "$DATA"
install -d -m 700 "$DATA/backups"
install -d -m 755 "$DATA/downloads"

# ---- 1. compose.env: domain, database password, remembered settings (first run) -------
if [ ! -f "$DATA/compose.env" ]; then
  : "${DOMAIN:?set DOMAIN on the first install}"
  : "${ADMIN_EMAIL:?set ADMIN_EMAIL (the Super Admin) on the first install}"
  log "creating $DATA/compose.env"
  (umask 077 && cat >"$DATA/compose.env" <<EOF
DOMAIN=$DOMAIN
ACME_EMAIL=$ADMIN_EMAIL
ADMIN_EMAIL=$ADMIN_EMAIL
DATA_DIR=$DATA
DB_DATABASE=solidtime
DB_USERNAME=solidtime
DB_PASSWORD=$(openssl rand -hex 24)
MAIL=log
EOF
  )
fi
# shellcheck disable=SC1091
. "$DATA/compose.env"

remember() { # KEY value: add or replace in compose.env
  if grep -q "^$1=" "$DATA/compose.env"; then
    sed -i "s|^$1=.*|$1=$2|" "$DATA/compose.env"
  else
    echo "$1=$2" >>"$DATA/compose.env"
  fi
}
RECREATE_WEB=""
if [ -n "$REQ_DOMAIN" ] && [ "$REQ_DOMAIN" != "$DOMAIN" ]; then
  log "changing domain $DOMAIN -> $REQ_DOMAIN"
  remember DOMAIN "$REQ_DOMAIN"
  DOMAIN="$REQ_DOMAIN"
  RECREATE_WEB=1
fi
if [ -n "$REQ_MAIL" ]; then remember MAIL "$REQ_MAIL"; MAIL="$REQ_MAIL"; fi
if [ -n "$REQ_SMTP_HOST" ]; then remember SMTP_HOST "$REQ_SMTP_HOST"; SMTP_HOST="$REQ_SMTP_HOST"; fi
if [ -n "$REQ_SMTP_PORT" ]; then remember SMTP_PORT "$REQ_SMTP_PORT"; SMTP_PORT="$REQ_SMTP_PORT"; fi
if [ -n "$REQ_SMTP_USERNAME" ]; then remember SMTP_USERNAME "$REQ_SMTP_USERNAME"; SMTP_USERNAME="$REQ_SMTP_USERNAME"; fi
if [ -n "$REQ_MAIL_FROM" ]; then remember MAIL_FROM "$REQ_MAIL_FROM"; MAIL_FROM="$REQ_MAIL_FROM"; fi
MAIL="${MAIL:-log}"
MAIL_FROM="${MAIL_FROM:-no-reply@$DOMAIN}"

# set_env FILE KEY VALUE: replace or append KEY=VALUE (value written as given); prints
# "changed" when the file changed.
set_env() {
  local file="$1" key="$2" value="$3"
  if grep -qxF "$key=$value" "$file"; then return 0; fi
  (umask 077 && K="$key" V="$value" awk '
    BEGIN { done = 0 }
    index($0, ENVIRON["K"] "=") == 1 { if (!done) { print ENVIRON["K"] "=" ENVIRON["V"]; done = 1 }; next }
    { print }
    END { if (!done) print ENVIRON["K"] "=" ENVIRON["V"] }' "$file" >"$file.new")
  mv "$file.new" "$file"
  echo changed
}

# ---- 2. laravel.env: web app settings and keys (keys created once) --------------------
SOLIDTIME_VERSION="$(sed -n 's/^ARG SOLIDTIME_VERSION=//p' "$HERE/../../server/web/Dockerfile")"
if [ ! -f "$DATA/laravel.env" ]; then
  log "creating $DATA/laravel.env (new application keys)"
  docker pull -q "solidtime/solidtime:$SOLIDTIME_VERSION" >/dev/null
  (umask 077 && {
    cat <<EOF
APP_ENV="production"
APP_DEBUG="false"
APP_FORCE_HTTPS="true"
TRUSTED_PROXIES="0.0.0.0/0,2000:0:0:0:0:0:0:0/3"
LOG_CHANNEL="stderr"
LOG_LEVEL="info"
DB_CONNECTION="pgsql"
DB_HOST="database"
DB_PORT="5432"
DB_SSL_MODE="prefer"
DB_DATABASE="$DB_DATABASE"
DB_USERNAME="$DB_USERNAME"
DB_PASSWORD="$DB_PASSWORD"
QUEUE_CONNECTION="database"
FILESYSTEM_DISK="local"
PUBLIC_FILESYSTEM_DISK="public"
GOTENBERG_URL="http://gotenberg:3000"
EOF
    docker run --rm "solidtime/solidtime:$SOLIDTIME_VERSION" php artisan self-host:generate-keys
  } >"$DATA/laravel.env")
fi
L="$DATA/laravel.env"
changes=""
changes+=$(set_env "$L" APP_NAME '"Time Pilot"')
changes+=$(set_env "$L" APP_URL "\"https://$DOMAIN\"")
changes+=$(set_env "$L" SUPER_ADMINS "\"$ADMIN_EMAIL\"")
# People join by invitation only (the Super Admin creates organizations; admins invite).
changes+=$(set_env "$L" APP_ENABLE_REGISTRATION '"off"')
changes+=$(set_env "$L" TIMEPILOT_SOURCE_URL "\"${TIMEPILOT_SOURCE_URL:-https://github.com/saquibIra/Ira-Tracker}\"")
changes+=$(set_env "$L" MAIL_FROM_ADDRESS "\"$MAIL_FROM\"")
changes+=$(set_env "$L" MAIL_FROM_NAME '"Time Pilot"')
if [ "$MAIL" = smtp ]; then
  : "${SMTP_HOST:?MAIL=smtp needs SMTP_HOST}"
  port="${SMTP_PORT:-587}"
  changes+=$(set_env "$L" MAIL_MAILER '"smtp"')
  changes+=$(set_env "$L" MAIL_HOST "\"$SMTP_HOST\"")
  changes+=$(set_env "$L" MAIL_PORT "\"$port\"")
  changes+=$(set_env "$L" MAIL_USERNAME "\"${SMTP_USERNAME:-}\"")
  if [ "$port" = 465 ]; then scheme=smtps; else scheme=smtp; fi
  changes+=$(set_env "$L" MAIL_SCHEME "\"$scheme\"")
  if [ -n "$SMTP_PASSWORD" ]; then
    log "updating the SMTP password"
    changes+=$(set_env "$L" MAIL_PASSWORD "'$SMTP_PASSWORD'")
  fi
else
  changes+=$(set_env "$L" MAIL_MAILER '"log"')
fi
if [ -n "$changes" ]; then RECREATE_WEB=1; fi

# ---- 3. autotrack.env: AI settings (created once) -------------------------------------
write_key() { # GEMINI_API_KEY into $1; the key reaches awk through its environment only
  (umask 077 && K="$GEMINI_API_KEY" awk \
    '/^GEMINI_API_KEY=/ { print "GEMINI_API_KEY=" ENVIRON["K"]; next } { print }' "$1" >"$1.new")
  mv "$1.new" "$1"
}
A="$DATA/autotrack.env"
RECREATE_AUTOTRACK=""
if [ ! -f "$A" ]; then
  log "creating $A"
  (umask 077 && cp "$HERE/autotrack.env.template" "$A")
  write_key "$A"
elif [ -n "$GEMINI_API_KEY" ]; then
  log "updating GEMINI_API_KEY"
  write_key "$A"
  RECREATE_AUTOTRACK=1
fi
if [ -n "$(set_env "$A" TIMEPILOT_PUBLIC_URL "https://$DOMAIN")" ]; then RECREATE_AUTOTRACK=1; fi

sed "s|__DOMAIN__|$DOMAIN|g" "$HERE/downloads-index.html" >"$DATA/downloads/index.html"

# ---- 4. start or update ---------------------------------------------------------------
cd "$HERE"
compose() { docker compose --env-file "$DATA/compose.env" "$@"; }
log "building and starting (the first run downloads and builds for a few minutes)"
compose pull --quiet --ignore-buildable
compose build --pull --quiet
compose up -d --remove-orphans
if [ -n "$RECREATE_WEB" ]; then compose up -d --force-recreate app scheduler queue; fi
if [ -n "$RECREATE_AUTOTRACK" ]; then compose up -d --force-recreate autotrack; fi

for _ in $(seq 1 60); do
  compose exec -T database pg_isready -q -U "$DB_USERNAME" && break
  sleep 2
done
compose exec -T scheduler php artisan migrate --force
if [ ! -f "$DATA/.passport-client" ]; then
  compose exec -T scheduler php artisan passport:client --personal --name="API" -n >/dev/null
  touch "$DATA/.passport-client"
fi

# The Super Admin: the only account of a new platform.
if [ ! -f "$DATA/admin-credentials.txt" ]; then
  (umask 077 && compose exec -T scheduler php artisan admin:user:create "Time Pilot Admin" "$ADMIN_EMAIL" --verify-email \
    >"$DATA/admin-credentials.txt")
  log "Super Admin created: $ADMIN_EMAIL (password in $DATA/admin-credentials.txt)"
fi

# Sign-in clients of the desktop app and the auto-tracking settings page.
clients="$(compose exec -T scheduler php artisan timepilot:oauth-clients \
  --admin-redirect="https://$DOMAIN/autotrack/admin/" | tail -n 1)"
desktop="$(printf '%s' "$clients" | sed -n 's/.*"desktop":"\([A-Za-z0-9-]*\)".*/\1/p')"
admin="$(printf '%s' "$clients" | sed -n 's/.*"admin":"\([A-Za-z0-9-]*\)".*/\1/p')"
[ -n "$desktop" ] && [ -n "$admin" ] || { echo "could not create the OAuth clients: $clients"; exit 1; }
if [ -n "$(set_env "$L" TIMEPILOT_FIRST_PARTY_CLIENTS "\"$desktop,$admin\"")" ]; then
  compose up -d --force-recreate app scheduler queue
fi
a1="$(set_env "$A" TIMEPILOT_DESKTOP_CLIENT_ID "$desktop")"
a2="$(set_env "$A" TIMEPILOT_ADMIN_CLIENT_ID "$admin")"
if [ -n "$a1$a2" ]; then
  compose up -d --force-recreate autotrack
fi

# ---- 5. nightly backup at 02:30 (kept 14 days in $DATA/backups) -----------------------
cat >/etc/cron.d/time-pilot-backup <<EOF
30 2 * * * root $HERE/backup.sh >>$DATA/backups/backup.log 2>&1
EOF
chmod 644 /etc/cron.d/time-pilot-backup
rm -f /etc/cron.d/time-pilot-onboard /etc/cron.d/ira-tracker-onboard # self-registration is gone

docker image prune -f >/dev/null
log "ready: https://$DOMAIN  (Super Admin panel: https://$DOMAIN/admin, email: $MAIL)"
