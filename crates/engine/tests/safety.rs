//! The safety suite (phase 10). One named test per rule in the plan's
//! "Safety suite" list. Every test runs on a temp copy of the fixture home;
//! keep this file red or green as a whole, and do not move a rule out of it.

mod common;

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use common::{Home, basic_home};
use space_optimiser_engine::EngineError;
use space_optimiser_engine::model::{
    Action, BuildPlanArgs, Candidate, CandidateId, ConfirmationToken, ExecutePlanArgs,
    ExecuteStart, PlanRef, Risk, ScanView,
};
use space_optimiser_engine::policy::CatalogId;

/// The rows the cleanup catalog marks bulk-safe. A catalog change that adds
/// or removes one must update this list in the same change.
const BULK_SAFE_ROWS: [CatalogId; 4] = [
    CatalogId::UserTemp,
    CatalogId::ThumbnailCache,
    CatalogId::FontCache,
    CatalogId::UserLogsOld,
];

/// What a file tree looks like from outside: every entry with its kind,
/// and each file's length and modification time.
fn snapshot(root: &Path) -> BTreeMap<PathBuf, (&'static str, u64, Option<SystemTime>)> {
    let mut out = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            let meta = fs::symlink_metadata(&path).unwrap();
            let kind = if meta.file_type().is_symlink() {
                "link"
            } else if meta.is_dir() {
                stack.push(path.clone());
                "dir"
            } else {
                "file"
            };
            let (len, modified) = if kind == "file" {
                (meta.len(), meta.modified().ok())
            } else {
                (0, None)
            };
            out.insert(
                path.strip_prefix(root).unwrap().to_path_buf(),
                (kind, len, modified),
            );
        }
    }
    out
}

fn bad_token() -> ConfirmationToken {
    serde_json::from_str("\"0000000000000000\"").unwrap()
}

fn audit_log(home: &Home) -> PathBuf {
    home.audit_dir().join("audit.jsonl")
}

fn ids(candidates: &[&Candidate]) -> Vec<CandidateId> {
    candidates.iter().map(|c| c.id).collect()
}

/// What "Select safe items" picks: the candidates the engine marks
/// bulk-safe.
fn select_safe(view: &ScanView) -> Vec<&Candidate> {
    view.candidates.iter().filter(|c| c.bulk_safe).collect()
}

/// Every candidate the scan offers that the catalog marks bulk-safe and
/// that can be deleted.
fn bulk_safe_set(view: &ScanView) -> Vec<&Candidate> {
    view.candidates
        .iter()
        .filter(|c| BULK_SAFE_ROWS.contains(&c.catalog_id) && c.action == Action::Delete)
        .collect()
}

// Rule 1: the scan code path has no delete.
#[test]
fn safety_1_scans_leave_the_fixture_unchanged() {
    let home = basic_home();
    home.write("tmp/fresh.part", b"still downloading");
    let before = snapshot(&home.root);
    assert!(before.len() > 20, "the fixture copy is too small");

    let engine = home.engine();
    let normal = common::scan(&engine);
    assert!(!normal.candidates.is_empty());
    assert_eq!(snapshot(&home.root), before, "Normal Scan changed the home");

    let deep = common::deep_scan(&engine);
    assert!(!deep.candidates.is_empty());
    assert_eq!(snapshot(&home.root), before, "Deep Scan changed the home");
    assert!(!audit_log(&home).exists(), "a scan wrote the audit log");
}

// Rule 2: execute without the confirm step's token deletes nothing.
#[test]
fn safety_2_execute_without_confirm_deletes_nothing() {
    let home = basic_home();
    let engine = home.engine();
    let view = common::scan(&engine);
    let plan = common::plan(&engine, &view, &["old-download.bin", "old-build"]);
    let before = snapshot(&home.root);

    // Never confirmed: the caller has no token, so any token it sends is
    // wrong.
    let result = engine.execute_plan(ExecutePlanArgs {
        plan_id: plan.plan_id,
        confirmation_token: bad_token(),
        keep_log_in_memory: true,
    });
    assert!(result.is_err(), "{result:?}");
    if cfg!(unix) {
        assert_eq!(result, Err(EngineError::WrongToken));
    }

    // Confirmed, but a different token is sent.
    let _ = common::confirm(&engine, &plan);
    let result = engine.execute_plan(ExecutePlanArgs {
        plan_id: plan.plan_id,
        confirmation_token: bad_token(),
        keep_log_in_memory: true,
    });
    assert!(result.is_err(), "{result:?}");

    assert_eq!(snapshot(&home.root), before);
    assert!(!audit_log(&home).exists());
}

// Rule 3: a planned path swapped for a link to Documents deletes nothing,
// and the link's target stays.
#[cfg(unix)]
#[test]
fn safety_3_a_mutated_path_deletes_nothing_and_keeps_the_target() {
    use space_optimiser_engine::model::{Outcome, SkipCause};

    let home = basic_home();
    let engine = home.engine();
    let view = common::scan(&engine);
    let plan = common::plan(&engine, &view, &["old-build"]);
    let token = common::confirm(&engine, &plan);

    fs::rename(home.path("tmp/old-build"), home.path("moved-build")).unwrap();
    std::os::unix::fs::symlink(home.path("Documents"), home.path("tmp/old-build")).unwrap();
    let documents = snapshot(&home.path("Documents"));

    let report = common::execute(&engine, &plan, token);
    let item = &report.items[0];
    assert_eq!(item.outcome, Outcome::Skipped, "{item:?}");
    assert_eq!(item.reason, Some(SkipCause::ChangedSinceScan));
    assert_eq!(report.bytes_removed, 0);
    assert!(home.path("Documents/notes.txt").exists());
    assert_eq!(snapshot(&home.path("Documents")), documents);
    assert!(home.path("moved-build/output.txt").exists());
    assert!(
        fs::symlink_metadata(home.path("tmp/old-build"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

// Rule 4: "Select safe items" picks exactly the bulk-safe set.
#[test]
fn safety_4_select_safe_items_equals_the_bulk_safe_set() {
    let rows: Vec<CatalogId> = CatalogId::ALL
        .into_iter()
        .filter(|r| r.bulk_safe())
        .collect();
    assert_eq!(rows, BULK_SAFE_ROWS, "the catalog's bulk-safe rows changed");
    for row in rows {
        assert_eq!(row.risk(), Risk::Low, "{row:?}");
        assert_eq!(row.action(), Action::Delete, "{row:?}");
    }

    let home = basic_home();
    let engine = home.engine();
    for view in [common::scan(&engine), common::deep_scan(&engine)] {
        let selected = select_safe(&view);
        assert!(!selected.is_empty(), "the fixture has no bulk-safe items");
        assert_eq!(ids(&selected), ids(&bulk_safe_set(&view)));
        for c in &selected {
            assert_eq!(c.risk, Risk::Low, "{}", c.display_name);
            assert_eq!(c.action, Action::Delete, "{}", c.display_name);
        }
        for c in view.candidates.iter().filter(|c| !c.bulk_safe) {
            assert!(
                !BULK_SAFE_ROWS.contains(&c.catalog_id) || c.action != Action::Delete,
                "{} should be bulk-safe",
                c.display_name
            );
        }
        // Browser and package caches are never in the set.
        assert!(selected.iter().all(|c| !matches!(
            c.catalog_id,
            CatalogId::BrowserHttpCache
                | CatalogId::PkgNpmCache
                | CatalogId::PkgNpx
                | CatalogId::PkgCargoCache
                | CatalogId::PkgGradleCaches
                | CatalogId::PkgNugetHttp
                | CatalogId::DockerBuildCache
        )));
    }
}

// Rule 5: the audit log appears only once a confirmed plan executes.
#[test]
fn safety_5_the_audit_log_is_written_only_by_execute() {
    let home = basic_home();
    let engine = home.engine();
    let view = common::scan(&engine);
    let plan = common::plan(&engine, &view, &["old-download.bin"]);
    assert!(!audit_log(&home).exists(), "written by scan or plan");
    let token = common::confirm(&engine, &plan);
    assert!(!audit_log(&home).exists(), "written by confirm");

    let result = engine.execute_plan(ExecutePlanArgs {
        plan_id: plan.plan_id,
        confirmation_token: token,
        keep_log_in_memory: false,
    });
    match result {
        Ok(ExecuteStart::Started) => {
            engine
                .wait_for_run(PlanRef {
                    plan_id: plan.plan_id,
                })
                .unwrap();
            let log = fs::read_to_string(audit_log(&home)).unwrap();
            assert!(log.contains("\"run_started\""));
            assert!(log.contains("\"run_finished\""));
            assert!(log.contains("old-download.bin"));
        }
        // Windows, until its executor is enabled: nothing ran.
        Err(EngineError::Unsupported) if cfg!(windows) => {
            assert!(!audit_log(&home).exists());
            assert!(home.path("tmp/old-download.bin").exists());
        }
        other => panic!("unexpected {other:?}"),
    }
}

// Rule 6: extra ids from the UI never grow the plan beyond the deletable
// set it asked for.
#[test]
fn safety_6_extra_ids_never_grow_the_plan() {
    let home = basic_home();
    let engine = home.engine();
    let first = common::scan(&engine);
    let stale_id = first.candidates[0].id;

    let engine = space_optimiser_engine::Engine::new(home.config_with(|ctx| {
        ctx.with_running_processes(&[common::chrome_process()])
            .with_large_file_min(1)
    }));
    let view = common::deep_scan(&engine);
    let safe = select_safe(&view);
    let safe_ids = ids(&safe);
    let not_actionable: Vec<&Candidate> = view
        .candidates
        .iter()
        .filter(|c| matches!(c.action, Action::Report | Action::None))
        .collect();
    assert!(
        not_actionable.iter().any(|c| c.action == Action::Report),
        "the fixture should list an information-only item"
    );
    assert!(
        not_actionable.iter().any(|c| c.action == Action::None),
        "the running browser's cache should be not actionable"
    );

    let build = |extra: Vec<CandidateId>| {
        let mut candidate_ids = safe_ids.clone();
        candidate_ids.extend(extra);
        engine.build_plan(BuildPlanArgs {
            scan_id: view.scan_id,
            candidate_ids,
        })
    };

    // The safe set alone, sent twice: each item once, nothing added.
    let plan = build(safe_ids.clone()).unwrap();
    let planned: HashSet<CandidateId> = plan.items.iter().map(|i| i.candidate_id).collect();
    let requested: HashSet<CandidateId> = safe_ids.iter().copied().collect();
    assert_eq!(plan.items.len(), planned.len(), "an item was planned twice");
    assert!(planned.is_subset(&requested));
    assert!(
        plan.items
            .iter()
            .all(|i| BULK_SAFE_ROWS.contains(&i.catalog_id))
    );

    // Adding an information-only or "quit first" item refuses the plan.
    for extra in &not_actionable {
        assert_eq!(
            build(vec![extra.id]).unwrap_err(),
            EngineError::NotActionable,
            "{}",
            extra.display_name
        );
    }
    // An id from an earlier scan, or one never issued, is refused.
    if !view.candidates.iter().any(|c| c.id == stale_id) {
        assert_eq!(
            build(vec![stale_id]).unwrap_err(),
            EngineError::UnknownCandidate
        );
    }
    let never_issued: CandidateId = serde_json::from_str("999999999").unwrap();
    assert_eq!(
        build(vec![never_issued]).unwrap_err(),
        EngineError::UnknownCandidate
    );
}

// Rule 7: nothing in the engine, the shell, or the UI talks to the network.
#[test]
fn safety_7_no_network_clients_and_no_granted_capabilities() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let desktop = manifest.join("../../apps/desktop");

    let rust_forbidden = [
        "std::net",
        "TcpStream",
        "TcpListener",
        "UdpSocket",
        "reqwest",
        "ureq",
        "hyper",
    ];
    let rust = files_with_extension(
        &[manifest.join("src"), desktop.join("src-tauri/src")],
        &["rs"],
    );
    assert!(rust.len() >= 10, "found only {} Rust sources", rust.len());
    for path in &rust {
        let text = fs::read_to_string(path).unwrap();
        for needle in rust_forbidden {
            assert!(!text.contains(needle), "{} uses {needle}", path.display());
        }
    }

    let ui_forbidden = [
        "fetch(",
        "XMLHttpRequest",
        "WebSocket",
        "EventSource",
        "sendBeacon",
    ];
    let ui = files_with_extension(&[desktop.join("src")], &["ts", "tsx"]);
    assert!(ui.len() >= 3, "found only {} UI sources", ui.len());
    for path in &ui {
        let text = fs::read_to_string(path).unwrap();
        for needle in ui_forbidden {
            assert!(!text.contains(needle), "{} uses {needle}", path.display());
        }
    }

    // The engine's dependencies are exactly the reviewed set.
    let engine_toml = fs::read_to_string(manifest.join("Cargo.toml")).unwrap();
    let deps = dependency_names(&engine_toml, false);
    let expected: BTreeSet<String> = [
        "serde",
        "serde_json",
        "thiserror",
        "libc",
        "rustix",
        "windows-sys",
    ]
    .map(String::from)
    .into();
    assert_eq!(deps, expected, "engine dependencies changed");
    let dev: BTreeSet<String> = ["tempfile", "trybuild", "windows-sys"]
        .map(String::from)
        .into();
    assert_eq!(
        dependency_names(&engine_toml, true),
        dev,
        "engine dev-dependencies changed"
    );

    // The shell installs no plugin and grants no permission.
    let shell_toml = fs::read_to_string(desktop.join("src-tauri/Cargo.toml")).unwrap();
    assert!(
        !shell_toml.contains("tauri-plugin"),
        "a Tauri plugin was added"
    );
    let capability =
        fs::read_to_string(desktop.join("src-tauri/capabilities/default.toml")).unwrap();
    let permission_lines: Vec<&str> = capability
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("permissions"))
        .collect();
    assert_eq!(permission_lines, ["permissions = []"]);
    let capability_files = fs::read_dir(desktop.join("src-tauri/capabilities"))
        .unwrap()
        .count();
    assert_eq!(capability_files, 1, "only the default capability may exist");
}

fn files_with_extension(roots: &[PathBuf], extensions: &[&str]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = roots.to_vec();
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| extensions.contains(&e))
            {
                out.push(path);
            }
        }
    }
    out
}

/// Crate names declared in `[dependencies]` sections (including
/// target-specific ones), or in `[dev-dependencies]` sections when `dev`.
fn dependency_names(toml: &str, dev: bool) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut in_section = false;
    for line in toml.lines().map(str::trim) {
        if line.starts_with('[') {
            let header = line.trim_matches(|c| c == '[' || c == ']');
            in_section = if dev {
                header.ends_with("dev-dependencies")
            } else {
                header.ends_with("dependencies")
                    && !header.ends_with("dev-dependencies")
                    && !header.ends_with("build-dependencies")
            };
            continue;
        }
        if !in_section || line.is_empty() || line.starts_with('#') {
            continue;
        }
        // `name = …`, `name.workspace = true`; skip continuation lines of a
        // multi-line table such as a feature list.
        let Some((key, _)) = line.split_once('=') else {
            continue;
        };
        let name = key.trim().split('.').next().unwrap().trim();
        if !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            out.insert(name.to_owned());
        }
    }
    out
}

// Phase 10 risk: test setup refuses a home without the fixture marker, so a
// test cannot be pointed at a real home by mistake.
#[test]
fn the_fixture_guard_refuses_a_home_without_its_marker() {
    let home = basic_home();
    common::assert_fixture_home(&home.root);

    let dir = tempfile::tempdir().unwrap();
    let unmarked = dir.path().join("unmarked");
    fs::create_dir_all(unmarked.join("Documents")).unwrap();
    let refused = std::panic::catch_unwind(|| common::assert_fixture_home(&unmarked));
    assert!(refused.is_err(), "a home without the marker was accepted");

    // A folder with the marker's name is not the marker.
    let folder_marker = dir.path().join("folder-marker");
    fs::create_dir_all(folder_marker.join(common::FIXTURE_MARKER)).unwrap();
    let refused = std::panic::catch_unwind(|| common::assert_fixture_home(&folder_marker));
    assert!(refused.is_err(), "a marker folder was accepted");
}
