#!/bin/sh
# Sets the app version in every place that records it: the workspace
# Cargo.toml (Tauri and the About screen read it from there), Cargo.lock,
# and the desktop package.json and package-lock.json.
#
#   scripts/bump-version.sh 0.2.0
#
# The version must be MAJOR.MINOR.PATCH with no suffix: the Windows MSI
# accepts only numbers. Mark a test build as a pre-release when running the
# Release workflow instead. Commit the change through a pull request, then
# run the workflow from main.
set -eu

new=${1:-}
if ! printf '%s' "$new" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$'; then
  echo "usage: $0 MAJOR.MINOR.PATCH (for example 0.2.0)" >&2
  exit 2
fi

repo=$(cd "$(dirname "$0")/.." && pwd)
old=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$repo/Cargo.toml" | head -n 1)
if [ "$old" = "$new" ]; then
  echo "The version is already $new." >&2
  exit 1
fi

# Only the [workspace.package] version line, the first `version =` in the file.
tmp=$(mktemp)
awk -v new="$new" '!done && /^version = "/ { print "version = \"" new "\""; done = 1; next } { print }' \
  "$repo/Cargo.toml" > "$tmp"
mv "$tmp" "$repo/Cargo.toml"

(cd "$repo" && cargo update --workspace --offline --quiet)
(cd "$repo/apps/desktop" && npm version "$new" --no-git-tag-version --allow-same-version >/dev/null)

echo "Version $old -> $new"
echo "Check with: cargo test -p space-optimiser-engine --test packaging"
