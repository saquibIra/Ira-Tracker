#!/usr/bin/env bash
# Creates the Google Cloud resources for Ira Tracker (run once, from your Mac, in the
# repository root). Safe to re-run: existing resources are kept.
#
#   PROJECT=my-project DOMAIN=time.example.com ADMIN_EMAIL=you@example.com \
#     deploy/gcp/setup.sh
#
# Optional: REGION (asia-south1), ZONE (asia-south1-a), MACHINE (e2-standard-2),
#           REPO (this GitHub repo), BRANCH (main).
set -euo pipefail

: "${PROJECT:?set PROJECT (Google Cloud project id)}"
: "${DOMAIN:?set DOMAIN (e.g. time.example.com)}"
: "${ADMIN_EMAIL:?set ADMIN_EMAIL (solidtime super admin, HTTPS certificate contact)}"
REGION="${REGION:-asia-south1}"
ZONE="${ZONE:-${REGION}-a}"
MACHINE="${MACHINE:-e2-standard-2}"
REPO="${REPO:-https://github.com/saquibIra/Ira-Tracker.git}"
BRANCH="${BRANCH:-main}"
VM=ira-tracker
SA_NAME=ira-tracker-vm
SA="${SA_NAME}@${PROJECT}.iam.gserviceaccount.com"

cd "$(dirname "$0")/../.."
run() { echo "+ $*"; "$@"; }
exists() { "$@" >/dev/null 2>&1; }

run gcloud config set project "$PROJECT"
run gcloud services enable compute.googleapis.com aiplatform.googleapis.com logging.googleapis.com

# Service account for the VM: Gemini on Vertex AI, logs and metrics. Nothing else.
exists gcloud iam service-accounts describe "$SA" ||
  run gcloud iam service-accounts create "$SA_NAME" --display-name "Ira Tracker VM"
for role in roles/aiplatform.user roles/logging.logWriter roles/monitoring.metricWriter; do
  run gcloud projects add-iam-policy-binding "$PROJECT" --member "serviceAccount:$SA" --role "$role" \
    --condition=None --quiet >/dev/null
done

# Fixed public address (point your DNS record here).
exists gcloud compute addresses describe ira-tracker-ip --region "$REGION" ||
  run gcloud compute addresses create ira-tracker-ip --region "$REGION"
IP=$(gcloud compute addresses describe ira-tracker-ip --region "$REGION" --format 'value(address)')

# Only HTTP(S) from the internet (HTTP is needed for the certificate and redirects).
exists gcloud compute firewall-rules describe ira-tracker-web ||
  run gcloud compute firewall-rules create ira-tracker-web --direction INGRESS \
    --allow tcp:80,tcp:443,udp:443 --target-tags ira-tracker

# Daily disk snapshots, kept 14 days (database and settings are on the boot disk).
exists gcloud compute resource-policies describe ira-tracker-daily --region "$REGION" ||
  run gcloud compute resource-policies create snapshot-schedule ira-tracker-daily --region "$REGION" \
    --daily-schedule --start-time 20:30 --max-retention-days 14 --on-source-disk-delete keep-auto-snapshots

if ! exists gcloud compute instances describe "$VM" --zone "$ZONE"; then
  run gcloud compute instances create "$VM" --zone "$ZONE" --machine-type "$MACHINE" \
    --image-family debian-12 --image-project debian-cloud \
    --boot-disk-size 30GB --boot-disk-type pd-balanced \
    --service-account "$SA" --scopes cloud-platform \
    --tags ira-tracker --address "$IP" --shielded-secure-boot \
    --metadata "domain=${DOMAIN},admin-email=${ADMIN_EMAIL},repo=${REPO},branch=${BRANCH}" \
    --metadata-from-file startup-script=deploy/gcp/bootstrap.sh
  run gcloud compute disks add-resource-policies "$VM" --zone "$ZONE" --resource-policies ira-tracker-daily
fi

cat <<EOF

Done. Next:
  1. DNS: create an A record   ${DOMAIN}  ->  ${IP}
  2. The VM installs everything by itself (5-10 minutes). Follow it with:
       gcloud compute ssh ${VM} --zone ${ZONE} -- sudo journalctl -u google-startup-scripts -f
  3. Then open https://${DOMAIN} (certificate appears once DNS points here).
     Admin login: gcloud compute ssh ${VM} --zone ${ZONE} -- sudo cat /opt/ira-data/admin-credentials.txt
EOF
