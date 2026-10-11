#!/bin/sh
# Branch names are `<type>/<topic>`: the Conventional Commit types, a slash,
# then lower-case letters, digits, dots, dashes, or underscores. `main` is
# allowed for the maintainer's direct fixes; dependabot branches are allowed.
set -eu

branch=$(git symbolic-ref --quiet --short HEAD 2>/dev/null || echo "")
# A detached HEAD (rebase, bisect) has no name to check.
[ -n "$branch" ] || exit 0

case "$branch" in
  main) exit 0 ;;
  dependabot/*) exit 0 ;;
esac

if printf '%s' "$branch" | grep -Eq '^(feat|fix|docs|test|refactor|perf|build|ci|chore|revert)/[a-z0-9][a-z0-9._-]*$'; then
  exit 0
fi

cat >&2 <<MSG
Branch name '$branch' is not allowed.
Use <type>/<topic>, for example fix/temp-root-project-marker or docs/contributing.
Types: feat fix docs test refactor perf build ci chore revert.
Rename with: git branch -m <type>/<topic>
MSG
exit 1
