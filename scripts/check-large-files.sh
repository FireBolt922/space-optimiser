#!/bin/sh
# Refuses to commit a file over the limit. Installers, audit logs, fixture
# copies, and coverage output never belong in the repository.
set -eu

limit=1048576 # 1 MiB
status=0
for file in "$@"; do
  [ -f "$file" ] || continue
  size=$(wc -c < "$file" | tr -d ' ')
  if [ "$size" -gt "$limit" ]; then
    echo "$file is $size bytes; the limit is $limit. Large binaries do not belong in git." >&2
    status=1
  fi
done
exit $status
