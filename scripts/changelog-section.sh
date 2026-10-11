#!/bin/sh
# Prints the body of one version's section of CHANGELOG.md, for release
# notes and the release pull request.
#
#   scripts/changelog-section.sh 0.2.0
set -eu

version=${1:-}
[ -n "$version" ] || { echo "usage: $0 VERSION" >&2; exit 2; }
file=$(cd "$(dirname "$0")/.." && pwd)/CHANGELOG.md
awk -v version="$version" '
  /^## \[/ { inside = ($0 ~ "^## \\[" version "\\]"); if (inside) { found = 1; next } }
  inside { lines[n++] = $0 }
  END {
    if (!found) { print "CHANGELOG.md has no section for " version "." > "/dev/stderr"; exit 1 }
    # Without the blank lines before and after the body.
    first = 0; last = n - 1
    while (first <= last && lines[first] == "") first++
    while (last >= first && lines[last] == "") last--
    for (i = first; i <= last; i++) print lines[i]
  }
' "$file"
