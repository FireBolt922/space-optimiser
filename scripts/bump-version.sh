#!/bin/sh
# Sets the app version in every place that records it: the workspace
# Cargo.toml (Tauri and the About screen read it from there), Cargo.lock,
# the desktop package.json and package-lock.json, and CHANGELOG.md, where
# the Unreleased entries become the new version's section.
#
#   scripts/bump-version.sh 0.2.0
#
# The Release workflow runs this for you and opens the pull request. The
# version must be MAJOR.MINOR.PATCH with no suffix: the Windows MSI accepts
# only numbers. A test build is a plain version published as a pre-release.
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

# Only the workspace crates' own entries in Cargo.lock change.
(cd "$repo" && cargo update --workspace --quiet)
(cd "$repo/apps/desktop" && npm version "$new" --no-git-tag-version --allow-same-version >/dev/null)
"$repo/scripts/roll-changelog.sh" "$new" "$old"

echo "Version $old -> $new"
echo "Check with: cargo test -p space-optimiser-engine --test packaging"
