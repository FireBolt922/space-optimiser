#!/bin/sh
# Moves everything under "## [Unreleased]" in CHANGELOG.md into a new
# "## [NEW] - DATE" section and points the comparison links at the new tag.
#
#   scripts/roll-changelog.sh NEW PREVIOUS [DATE]
#
# When nothing is listed under Unreleased, the commit subjects since
# vPREVIOUS are listed instead, so a release never stops for an empty
# changelog.
set -eu

new=${1:-}
prev=${2:-}
date=${3:-$(date -u +%Y-%m-%d)}
if [ -z "$new" ] || [ -z "$prev" ]; then
  echo "usage: $0 NEW PREVIOUS [DATE]" >&2
  exit 2
fi

repo=$(cd "$(dirname "$0")/.." && pwd)
file=$repo/CHANGELOG.md
tmp=$(mktemp)

# Unreleased is empty: fill it from the commits ("fix(ui): stop a crash
# (#12)" becomes "- Stop a crash (#12)"), leaving out release commits.
empty=$(awk '
  /^## \[Unreleased\]/ { inside = 1; next }
  inside && /^## / { exit }
  inside && !/^$/ { found = 1 }
  END { print (found ? "no" : "yes") }
' "$file")
if [ "$empty" = yes ]; then
  entries=$(git -C "$repo" log --format=%s "v${prev}..HEAD" 2>/dev/null \
    | grep -v '^chore(release)' \
    | sed -E 's/^[a-zA-Z]+(\([^)]*\))?!?: *//' \
    | awk '{ print "- " toupper(substr($0, 1, 1)) substr($0, 2) }' || true)
  [ -n "$entries" ] || entries="- Maintenance release; nothing changes for users."
  ENTRIES=$entries awk '
    { print }
    /^## \[Unreleased\]/ { print ""; print "### Changed"; print ""; print ENVIRON["ENTRIES"] }
  ' "$file" > "$tmp"
  mv "$tmp" "$file"
  tmp=$(mktemp)
fi
awk -v new="$new" -v prev="$prev" -v date="$date" '
  # 0: before Unreleased, 1: inside it, 2: after it.
  BEGIN { state = 0 }
  state == 0 && /^## \[Unreleased\]/ {
    print; print ""; print "## [" new "] - " date
    state = 1; next
  }
  state == 1 && /^## / { state = 2 }
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
  }
' "$file" > "$tmp"
mv "$tmp" "$file"
echo "CHANGELOG.md: Unreleased -> $new ($date)"
