//! Test 8: tests touch only `fixtures/` copies and test temp directories.
//! Every fixture is a `tempfile` copy (see `common`); this check keeps real
//! home and system paths out of test sources that run. A line that only builds data
//! (a JSON payload, a parser input) may end with `// hygiene: data only`.

use std::fs;
use std::path::Path;

const FORBIDDEN: &[&str] = &[
    "\"/Users/",
    "\"/home/",
    "\"/root",
    "\"/tmp",
    "\"/private/",
    "\"/var/",
    "C:\\\\Users",
    "home_dir(",
    "env::var(\"HOME",
    "env::var(\"TMP",
    "env::var(\"TEMP",
    "resolve_context().unwrap().home().join(",
];

#[test]
fn test_sources_do_not_name_real_home_or_temp_paths() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut checked = 0;
    let mut stack = vec![dir];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            // `tests/ui` holds compile-fail cases, which never run.
            if path.is_dir() && path.file_name().unwrap() != "ui" {
                stack.push(path);
                continue;
            }
            if path.file_name().unwrap() == "hygiene.rs"
                || path.extension().is_none_or(|e| e != "rs")
            {
                continue;
            }
            let text = fs::read_to_string(&path).unwrap();
            for (n, line) in text.lines().enumerate() {
                if line.ends_with("// hygiene: data only") {
                    continue;
                }
                for needle in FORBIDDEN {
                    assert!(
                        !line.contains(needle),
                        "{}:{} contains {needle}",
                        path.display(),
                        n + 1
                    );
                }
            }
            checked += 1;
        }
    }
    assert!(checked >= 5);
}

// Phase 8 test 19: delete calls live only in the executor, and nothing uses
// the path-based `std::fs::remove_*` family (CVE-2022-21658, junctions) or
// the path-based Windows delete APIs. The executor is `src/execute.rs` and
// its child modules in `src/execute/`. Processes start only in the tool
// runner (phase 6 item 1).
#[test]
fn delete_calls_live_only_in_the_executor() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let sources = [
        manifest.join("src"),
        manifest.join("tests"),
        manifest.join("../../apps/desktop/src-tauri/src"),
    ];
    let never = [
        "remove_dir_all",
        "remove_file",
        "remove_dir",
        "DeleteFileW",
        "RemoveDirectoryW",
        "SHFileOperation",
        "IFileOperation",
        "MOVEFILE_DELAY_UNTIL_REBOOT",
        "MoveFileEx",
        "FILE_FLAG_DELETE_ON_CLOSE",
        "FILE_DELETE_ON_CLOSE",
        "NtSetInformationFile",
        "libc::unlink",
        "libc::rmdir",
    ];
    let executor_only = [
        "unlinkat",
        "AtFlags::REMOVEDIR",
        "FileDispositionInfo",
        "FILE_DISPOSITION_FLAG_DELETE",
    ];
    let runner_only = ["Command::new", "CreateProcessW", "ShellExecute"];
    let mut executor_deletes = 0;
    let mut runner_spawns = 0;
    let mut stack: Vec<_> = sources.into_iter().collect();
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().is_none_or(|e| e != "rs") || path.ends_with("tests/hygiene.rs") {
                continue;
            }
            let text = fs::read_to_string(&path).unwrap();
            let is_executor = path.ends_with("src/execute.rs")
                || path.parent().is_some_and(|p| p.ends_with("src/execute"));
            let is_runner = path.ends_with("src/tool.rs");
            for needle in never {
                assert!(!text.contains(needle), "{} uses {needle}", path.display());
            }
            for needle in executor_only {
                if is_executor {
                    executor_deletes += text.matches(needle).count();
                } else {
                    assert!(!text.contains(needle), "{} uses {needle}", path.display());
                }
            }
            for needle in runner_only {
                if is_runner {
                    runner_spawns += text.matches(needle).count();
                } else {
                    assert!(!text.contains(needle), "{} uses {needle}", path.display());
                }
            }
        }
    }
    assert!(
        executor_deletes > 0,
        "the executor's delete calls were not found"
    );
    assert_eq!(
        runner_spawns, 1,
        "the tool runner should start processes in one place"
    );
}
