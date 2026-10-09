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
