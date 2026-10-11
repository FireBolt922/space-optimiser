#!/bin/sh
# Moves everything under "## [Unreleased]" in CHANGELOG.md into a new
# "## [NEW] - DATE" section and points the comparison links at the new tag.
#
#   scripts/roll-changelog.sh NEW PREVIOUS [DATE]
#
# Fails when nothing is listed under Unreleased: a release with no entry
# in the changelog is a mistake, not a release.
set -eu

new=${1:-}
prev=${2:-}
date=${3:-$(date -u +%Y-%m-%d)}
if [ -z "$new" ] || [ -z "$prev" ]; then
  echo "usage: $0 NEW PREVIOUS [DATE]" >&2
  exit 2
fi

file=$(cd "$(dirname "$0")/.." && pwd)/CHANGELOG.md
tmp=$(mktemp)
awk -v new="$new" -v prev="$prev" -v date="$date" '
  # 0: before Unreleased, 1: inside it, 2: after it.
  BEGIN { state = 0; entries = 0 }
  state == 0 && /^## \[Unreleased\]/ {
    print; print ""; print "## [" new "] - " date
    state = 1; next
  }
  state == 1 && /^## / { state = 2 }
  state == 1 && !/^$/ { entries++ }
  /^\[Unreleased\]: / {
    url = $0
    sub(/^\[Unreleased\]: /, "", url)
    sub(/\/compare\/.*$/, "", url)
    print "[Unreleased]: " url "/compare/v" new "...HEAD"
    print "[" new "]: " url "/compare/v" prev "...v" new
    next
  }
  { print }
  END {
    if (state == 0) { print "CHANGELOG.md has no \"## [Unreleased]\" heading." > "/dev/stderr"; exit 1 }
    if (entries == 0) { print "CHANGELOG.md lists nothing under Unreleased." > "/dev/stderr"; exit 1 }
  }
' "$file" > "$tmp"
mv "$tmp" "$file"
echo "CHANGELOG.md: Unreleased -> $new ($date)"
