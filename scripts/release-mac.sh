#!/usr/bin/env bash
# Builds the Mac installer for employees and (optionally) publishes it on the server.
#
#   IRA_SERVER_URL=https://time.example.com scripts/release-mac.sh            # build only
#   IRA_SERVER_URL=https://time.example.com PUBLISH=1 scripts/release-mac.sh  # build + upload (GCP VM)
#   IRA_SERVER_URL=... PUBLISH_HOST=ubuntu@1.2.3.4 PUBLISH_KEY=~/key.pem scripts/release-mac.sh  # Linux server
#
# - The server URL is baked in, so engineers only paste their API token.
# - Signed with the "Ira Tracker Local Signing" identity from this Mac's keychain, so the
#   Screen Recording permission survives updates. (With an Apple Developer ID, set
#   SIGNING_IDENTITY to it and macOS shows no warning on first open.)
# - Universal binary: Apple silicon and Intel.
set -euo pipefail

: "${IRA_SERVER_URL:?set IRA_SERVER_URL, e.g. https://time.example.com}"
SIGNING_IDENTITY="${SIGNING_IDENTITY:-Ira Tracker Local Signing}"
VM="${VM:-ira-tracker}"
ZONE="${ZONE:-asia-south1-a}"

# Tools: Node from nvm and Rust from rustup when they are not on PATH.
if ! command -v npm >/dev/null && [ -d "$HOME/.nvm/versions/node" ]; then
  PATH="$(ls -d "$HOME"/.nvm/versions/node/*/bin | sort -V | tail -1):$PATH"
fi
# shellcheck disable=SC1091
command -v cargo >/dev/null || . "$HOME/.cargo/env"

cd "$(dirname "$0")/../widget"
VERSION=$(sed -n 's/.*"version": *"\([^"]*\)".*/\1/p' src-tauri/tauri.conf.json | head -1)
OUT="$(pwd)/src-tauri/target/release-dist"
mkdir -p "$OUT"

rustup target add aarch64-apple-darwin x86_64-apple-darwin >/dev/null
VITE_IRA_SERVER_URL="$IRA_SERVER_URL" APPLE_SIGNING_IDENTITY="$SIGNING_IDENTITY" \
  npm run tauri build -- --target universal-apple-darwin --bundles app

APP="src-tauri/target/universal-apple-darwin/release/bundle/macos/Ira Tracker.app"
codesign --verify --deep --strict "$APP"

# Drag-to-Applications disk image.
STAGE=$(mktemp -d)
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
DMG="$OUT/IraTracker-$VERSION-mac.dmg"
rm -f "$DMG"
hdiutil create -volname "Ira Tracker" -srcfolder "$STAGE" -ov -format UDZO "$DMG" >/dev/null
rm -rf "$STAGE"
cp "$DMG" "$OUT/IraTracker-mac.dmg" # stable "latest" name for the download link
echo "built $DMG"

if [ -n "${PUBLISH_HOST:-}" ]; then
  : "${PUBLISH_KEY:?set PUBLISH_KEY, the SSH key for PUBLISH_HOST}"
  # Only the linked "latest" file goes up; older versions are not kept on the server.
  scp -i "$PUBLISH_KEY" -o BatchMode=yes "$OUT/IraTracker-mac.dmg" "$PUBLISH_HOST:/tmp/"
  ssh -i "$PUBLISH_KEY" -o BatchMode=yes "$PUBLISH_HOST" \
    "sudo install -m 644 /tmp/IraTracker-mac.dmg /opt/ira-data/downloads/ && rm -f /tmp/IraTracker-mac.dmg"
  echo "published: $IRA_SERVER_URL/download/IraTracker-mac.dmg"
elif [ "${PUBLISH:-0}" = "1" ]; then
  gcloud compute scp "$OUT/IraTracker-mac.dmg" "$VM:/tmp/" --zone "$ZONE"
  gcloud compute ssh "$VM" --zone "$ZONE" -- \
    "sudo install -m 644 /tmp/IraTracker-mac.dmg /opt/ira-data/downloads/ && rm -f /tmp/IraTracker-mac.dmg"
  echo "published: $IRA_SERVER_URL/download/IraTracker-mac.dmg"
fi
