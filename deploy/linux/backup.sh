#!/usr/bin/env bash
# Nightly backup of the solidtime database and the Autotrack service data (cron, see install.sh).
# Files: /opt/timepilot-data/backups/{solidtime,autotrack}-YYYYMMDD-HHMM.*, kept 14 days.
# Restore: docs/DEPLOY_LINUX.md.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
DATA="${DATA_DIR:-/opt/timepilot-data}"
OUT="$DATA/backups"
STAMP=$(date +%Y%m%d-%H%M)
# shellcheck disable=SC1091
. "$DATA/compose.env"
cd "$HERE"
compose() { docker compose --env-file "$DATA/compose.env" "$@"; }

compose exec -T database pg_dump -U "$DB_USERNAME" -d "$DB_DATABASE" --no-owner | gzip >"$OUT/solidtime-$STAMP.sql.gz"
compose exec -T autotrack /app/.venv/bin/python -c "
import sqlite3
src = sqlite3.connect('/data/autotrack.db'); dst = sqlite3.connect('/data/backup.db')
src.backup(dst); dst.close()"
compose cp autotrack:/data/backup.db "$OUT/autotrack-$STAMP.db"
chmod 600 "$OUT"/*-"$STAMP".*
find "$OUT" -name 'solidtime-*.sql.gz' -mtime +14 -delete
find "$OUT" -name 'autotrack-*.db' -mtime +14 -delete
echo "$(date -Is) backup ok: $STAMP"
