#!/bin/sh
# Deletes exist only in the executor and processes start only in the tool
# runner. tests/hygiene.rs names these calls in order to forbid them, so it
# is excluded. CI and the pre-push hook both run this script.
set -eu
cd "$(dirname "$0")/.."

status=0
if grep -rnE --include='*.rs' --exclude=hygiene.rs 'remove_file|remove_dir' crates apps/desktop/src-tauri/src; then
  echo "std::fs::remove_* is not allowed. Deletes go through the executor." >&2
  status=1
fi
if grep -rlE --include='*.rs' --exclude=hygiene.rs 'Command::new' crates apps/desktop/src-tauri/src | grep -vx 'crates/engine/src/tool.rs'; then
  echo "Processes start only in crates/engine/src/tool.rs." >&2
  status=1
fi
if grep -rnE --include='*.rs' --exclude=hygiene.rs 'DeleteFileW|RemoveDirectoryW|SHFileOperation|IFileOperation|MOVEFILE_DELAY_UNTIL_REBOOT|MoveFileEx|FILE_FLAG_DELETE_ON_CLOSE|FILE_DELETE_ON_CLOSE|NtSetInformationFile|libc::unlink|libc::rmdir' crates apps/desktop/src-tauri/src; then
  echo "Path-based Windows deletes are not allowed. Deletes go through the executor." >&2
  status=1
fi
if grep -rlE --include='*.rs' --exclude=hygiene.rs 'CreateProcessW|ShellExecute' crates apps/desktop/src-tauri/src; then
  echo "Processes start only in crates/engine/src/tool.rs." >&2
  status=1
fi
if grep -rlE --include='*.rs' --exclude=hygiene.rs 'unlinkat|REMOVEDIR|FileDispositionInfo|FILE_DISPOSITION_FLAG_DELETE' crates apps/desktop/src-tauri/src | grep -vxE 'crates/engine/src/execute.rs|crates/engine/src/execute/[^/]+\.rs'; then
  echo "Handle-relative deletes are allowed only in the executor (crates/engine/src/execute.rs and crates/engine/src/execute/)." >&2
  status=1
fi
exit $status
