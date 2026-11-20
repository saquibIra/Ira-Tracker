#!/usr/bin/env bash
# Deploys the latest code on the VM: run on the VM as root, or from your Mac:
#   gcloud compute ssh ira-tracker --zone asia-south1-a -- sudo /opt/ira-tracker/deploy/gcp/update.sh
set -euo pipefail

APP=/opt/ira-tracker
DATA=/opt/ira-data
BRANCH="${BRANCH:-main}"

git -C "$APP" fetch origin "$BRANCH"
git -C "$APP" checkout -q "$BRANCH"
git -C "$APP" reset -q --hard "origin/$BRANCH"

cd "$APP/deploy/gcp"
compose() { docker compose --env-file "$DATA/compose.env" "$@"; }
compose pull --quiet
compose up -d --build --remove-orphans
compose exec -T scheduler php artisan migrate --force
docker image prune -f >/dev/null
echo "updated to $(git -C "$APP" log -1 --format='%h %s')"
