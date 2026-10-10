#!/bin/sh
# Copies the basic fixture home into a new temporary directory for a manual
# run of the window (phase 7 and phase 8 acceptance). Only debug builds read
# SPACE_OPTIMISER_FIXTURE_HOME; release builds always use this computer's
# folders.
#
# The fixture clock runs two days ahead, so the copied files count as old.
# tmp/new-download.part is dated two days ahead, so it counts as new.
set -eu

repo=$(cd "$(dirname "$0")/.." && pwd)
dest="$(mktemp -d)/home"
cp -R "$repo/fixtures/homes/basic" "$dest"
printf 'partial download\n' > "$dest/tmp/new-download.part"
if stamp=$(date -v+2d +%Y%m%d%H%M 2>/dev/null); then :; else
  stamp=$(date -d '+2 days' +%Y%m%d%H%M)
fi
touch -t "$stamp" "$dest/tmp/new-download.part"

echo "Fixture home: $dest"
echo "Run the window against it with:"
echo "  SPACE_OPTIMISER_FIXTURE_HOME=\"$dest\" npm --prefix apps/desktop run tauri dev"
