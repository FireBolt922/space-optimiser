#!/bin/sh
# Prints the version that follows the one in Cargo.toml.
#
#   scripts/next-version.sh [auto|major|minor|patch]
#
# auto (the default) reads the Conventional Commit lines since the last
# v* tag: a breaking change (`!` after the type, or a BREAKING CHANGE
# footer) raises MAJOR (MINOR before 1.0.0), feat raises MINOR, and fix,
# perf, or revert raise PATCH. Other commits alone (docs, ci, build, ...)
# are not a release; pass the part to raise to release anyway.
set -eu

bump=${1:-auto}
repo=$(cd "$(dirname "$0")/.." && pwd)
current=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$repo/Cargo.toml" | head -n 1)
major=${current%%.*}
rest=${current#*.}
minor=${rest%%.*}
patch=${rest#*.}

if [ "$bump" = auto ]; then
  last=$(git -C "$repo" describe --tags --abbrev=0 --match 'v*' 2>/dev/null || true)
  range=${last:+$last..}HEAD
  # Subjects and bodies. A squash merge keeps the pull request title as
  # the subject, so "Feat/topic (#12)" also counts as a feature.
  log=$(git -C "$repo" log --format='%s%n%b' "$range")
  if printf '%s\n' "$log" | grep -Eq '^[a-z]+(\([a-z]+\))?!: |^BREAKING CHANGE:'; then
    if [ "$major" -eq 0 ]; then bump="minor"; else bump="major"; fi
  elif printf '%s\n' "$log" | grep -Eiq '^feat([(/]|!?:)'; then
    bump="minor"
  elif printf '%s\n' "$log" | grep -Eiq '^(fix|perf|revert)([(/]|!?:)'; then
    bump="patch"
  else
    echo "No feat, fix, perf, or revert commit since ${last:-the first commit}: nothing to release." >&2
    echo "Pass major, minor, or patch to release anyway." >&2
    exit 1
  fi
fi

case $bump in
  major) echo "$((major + 1)).0.0" ;;
  minor) echo "$major.$((minor + 1)).0" ;;
  patch) echo "$major.$minor.$((patch + 1))" ;;
  *)
    echo "usage: $0 [auto|major|minor|patch]" >&2
    exit 2
    ;;
esac
