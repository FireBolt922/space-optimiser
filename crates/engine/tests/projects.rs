//! The `project-build-folders` row (the Projects tool): dependency and
//! build folders inside the person's projects, found by exact name and
//! marker, offered only inside source control, and labelled by how long ago
//! the project last changed. The project's own files are never candidates.

mod common;

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use common::{Home, basic_home, days_ahead, report_item, set_mtime};
use space_optimiser_engine::discover::{self, CancelToken, Scan, Stage};
use space_optimiser_engine::model::{
    Action, BuildPlanArgs, Candidate, Confidence, Label, Outcome, Risk, ScanMode, SkipCause,
};
use space_optimiser_engine::policy::{CatalogId, Context};
use space_optimiser_engine::{Engine, EngineError};

const ROW: CatalogId = CatalogId::ProjectBuildFolders;
/// The first lines of the file Cargo writes into every `target` folder.
const CACHEDIR_TAG: &[u8] =
    b"Signature: 8a477f597d28d172789f06886806bc55\n# This file is a cache directory tag created by cargo.\n";

fn scan(ctx: &Context) -> Scan {
    discover::scan(ctx, ScanMode::Projects, &CancelToken::new(), |_| {}).unwrap()
}

fn rows(scan: &Scan) -> Vec<&Candidate> {
    scan.caches.iter().filter(|c| c.catalog_id == ROW).collect()
}

fn names(scan: &Scan) -> BTreeSet<String> {
    rows(scan).iter().map(|c| c.display_name.clone()).collect()
}

fn is_old(c: &Candidate) -> bool {
    c.labels.contains(&Label::OldProject)
}

fn is_active(c: &Candidate) -> bool {
    c.labels.contains(&Label::ActiveProject)
}

fn row<'a>(scan: &'a Scan, name: &str) -> &'a Candidate {
    rows(scan)
        .into_iter()
        .find(|c| c.display_name == name)
        .unwrap_or_else(|| panic!("no row {name}; found {:?}", names(scan)))
}

/// A source-control folder with one source file.
fn repo(home: &Home, relative: &str) {
    home.write(&format!("{relative}/.git/HEAD"), b"ref: refs/heads/main\n");
    home.write(&format!("{relative}/src/main.txt"), b"source");
}

/// A Node project folder: `package.json` and a `node_modules` with a package.
fn node(home: &Home, relative: &str) {
    home.write(&format!("{relative}/package.json"), b"{}");
    home.write(
        &format!("{relative}/node_modules/left-pad/index.js"),
        &[b'x'; 4096],
    );
    home.write(
        &format!("{relative}/node_modules/left-pad/package.json"),
        b"{}",
    );
}

fn node_repo(home: &Home, relative: &str) {
    repo(home, relative);
    node(home, relative);
}

/// A context whose clock is `days` ahead: every file the test wrote is that
/// old.
fn ctx_in(home: &Home, days: u64) -> Context {
    Context::for_fixture(&home.root).with_now(days_ahead(days))
}

fn engine_in(home: &Home, days: u64) -> Engine {
    let now = days_ahead(days);
    Engine::new(home.config_with(move |ctx| ctx.with_now(now)))
}

#[test]
fn each_kind_is_found_only_with_its_marker() {
    let home = basic_home();
    node_repo(&home, "Projects/web");
    home.write("Projects/web/.next/cache/data.bin", &[1u8; 4096]);
    repo(&home, "Projects/rusty");
    home.write("Projects/rusty/Cargo.toml", b"[package]\n");
    home.write("Projects/rusty/target/CACHEDIR.TAG", CACHEDIR_TAG);
    home.write("Projects/rusty/target/debug/app", &[2u8; 4096]);
    repo(&home, "Projects/py");
    home.write("Projects/py/requirements.txt", b"requests\n");
    home.write("Projects/py/.venv/pyvenv.cfg", b"home = python\n");
    home.write("Projects/py/.venv/lib/site.py", &[3u8; 4096]);
    home.write("Projects/py/venv/pyvenv.cfg", b"home = python\n");
    home.write("Projects/py/venv/lib/site.py", &[3u8; 4096]);
    // The same names without their markers are not this row.
    repo(&home, "Projects/bare");
    home.write("Projects/bare/node_modules/pkg/index.js", b"x");
    home.write("Projects/bare/.next/cache/data.bin", b"x");
    home.write("Projects/bare/Cargo.toml", b"[package]\n");
    home.write("Projects/bare/target/debug/app", b"x");
    home.write("Projects/bare/.venv/lib/site.py", b"x");
    home.write("Projects/bare/build/out.bin", b"x");
    home.write("Projects/bare/dist/out.bin", b"x");

    let scan = scan(&home.old_context());

    let expected: BTreeSet<String> = [
        "~/Projects/web/node_modules",
        "~/Projects/web/.next",
        "~/Projects/rusty/target",
        "~/Projects/py/.venv",
        "~/Projects/py/venv",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    assert_eq!(names(&scan), expected);
    for c in rows(&scan) {
        assert_eq!(c.action, Action::Delete, "{}", c.display_name);
        assert_eq!(c.confidence, Confidence::High);
        assert!(!c.bulk_safe);
        assert!(c.estimated_bytes > 0, "{}", c.display_name);
        assert!(c.revealable);
        assert!(c.detail.is_some());
    }
}

// A `target` whose tag file was not written by Cargo's convention is not
// taken for a cache folder.
#[test]
fn a_target_folder_with_a_wrong_tag_is_not_listed() {
    let home = basic_home();
    repo(&home, "Projects/rusty");
    home.write("Projects/rusty/Cargo.toml", b"[package]\n");
    home.write("Projects/rusty/target/CACHEDIR.TAG", b"my notes\n");
    home.write("Projects/rusty/target/release/tool", &[2u8; 4096]);

    assert!(rows(&scan(&home.old_context())).is_empty());
}

// An installed app can ship `node_modules` next to a `package.json`. Only a
// folder inside source control is offered.
#[test]
fn a_folder_outside_source_control_is_shown_but_not_offered() {
    let home = basic_home();
    node(&home, "Tools/some-app/resources/app");
    let engine = home.engine();
    let view = common::scan_in(&engine, ScanMode::Projects);

    let c = common::named(&view, "~/Tools/some-app/resources/app/node_modules");
    assert_eq!(c.action, Action::None);
    assert_eq!(c.confidence, Confidence::Uncertain);
    assert!(!is_old(c) && !is_active(c), "{:?}", c.labels);
    assert!(
        c.detail.as_deref().unwrap().contains("source-control"),
        "{:?}",
        c.detail
    );
    let result = engine.build_plan(BuildPlanArgs {
        scan_id: view.scan_id,
        candidate_ids: vec![c.id],
    });
    assert_eq!(result.unwrap_err(), EngineError::NotActionable);
}

// A repository in the home folder itself (dotfiles) does not make every
// folder below it a project.
#[test]
fn a_repository_at_the_home_folder_does_not_count() {
    let home = basic_home();
    home.write(".git/HEAD", b"ref: refs/heads/main\n");
    node(&home, "Tools/app");
    home.write("package.json", b"{}");
    home.write("node_modules/pkg/index.js", b"x");

    let scan = scan(&home.old_context());

    assert_eq!(row(&scan, "~/Tools/app/node_modules").action, Action::None);
    assert!(!names(&scan).contains("~/node_modules"));
}

#[test]
fn a_virtual_environment_without_a_requirements_file_is_not_offered() {
    let home = basic_home();
    repo(&home, "Projects/py");
    home.write("Projects/py/.venv/pyvenv.cfg", b"home = python\n");
    home.write("Projects/py/.venv/lib/site.py", &[3u8; 4096]);
    repo(&home, "Projects/tracked");
    home.write("Projects/tracked/pyproject.toml", b"[project]\n");
    home.write("Projects/tracked/.venv/pyvenv.cfg", b"home = python\n");
    home.write("Projects/tracked/.venv/lib/site.py", &[3u8; 4096]);

    let scan = scan(&home.old_context());

    let loose = row(&scan, "~/Projects/py/.venv");
    assert_eq!(loose.action, Action::None);
    assert_eq!(loose.confidence, Confidence::Uncertain);
    assert!(loose.detail.as_deref().unwrap().contains("requirements"));
    assert_eq!(
        row(&scan, "~/Projects/tracked/.venv").action,
        Action::Delete
    );
}

/// Folders the walk must never look inside, each holding what would
/// otherwise be a match.
fn excluded(home: &Home) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    for relative in [
        ".hidden/app",
        "Library/Tool/app",
        "Applications/Tool.app/Contents/Resources/app",
        "Projects/Bundle.app/Contents/Resources/app",
        "AppData/Roaming/npm",
        "snap/tool/current",
        "Documents/app",
        "Desktop/app",
        "Downloads/app",
    ] {
        node_repo(home, relative);
        dirs.push(home.path(relative));
    }
    node_repo(home, "Projects/infra");
    node(home, "Projects/infra/cdk.out/asset.1");
    dirs.push(home.path("Projects/infra/cdk.out"));
    dirs
}

#[test]
fn excluded_and_protected_folders_are_never_read() {
    let home = basic_home();
    let excluded = excluded(&home);
    // A package's own nested dependencies are part of the folder above.
    home.write(
        "Projects/infra/node_modules/pkg/node_modules/dep/index.js",
        b"x",
    );
    home.write("Projects/infra/node_modules/pkg/package.json", b"{}");
    let visits: Arc<Mutex<Vec<PathBuf>>> = Arc::default();

    let scan = scan(&home.old_context().with_visit_log(visits.clone()));

    assert_eq!(
        names(&scan),
        BTreeSet::from(["~/Projects/infra/node_modules".to_owned()])
    );
    let visited = visits.lock().unwrap().clone();
    assert!(!visited.is_empty());
    for dir in &excluded {
        let top = home
            .root
            .join(dir.strip_prefix(&home.root).unwrap().iter().next().unwrap());
        // The folder that is excluded is the first excluded component.
        let fence = if dir.ends_with("cdk.out") {
            dir.clone()
        } else if top.ends_with("Projects") {
            home.path("Projects/Bundle.app")
        } else {
            top
        };
        for path in &visited {
            assert!(
                !(path.starts_with(&fence) && path != &fence),
                "the scan read {} inside {}",
                path.display(),
                fence.display()
            );
        }
    }
    for protected in ["Documents", "Desktop", "Downloads"] {
        assert!(
            !visited.contains(&home.path(protected)),
            "the scan looked at {protected}"
        );
    }
}

#[test]
fn an_old_project_and_an_active_project_are_told_apart() {
    let home = basic_home();
    node_repo(&home, "Projects/old");
    node_repo(&home, "Projects/active");
    // One file in the active project changed a day before the scan's clock.
    set_mtime(&home.path("Projects/active/src/main.txt"), days_ahead(39));

    let scan = scan(&ctx_in(&home, 40));

    let old = row(&scan, "~/Projects/old/node_modules");
    assert!(is_old(old) && !is_active(old), "{:?}", old.labels);
    assert_eq!(old.risk, Risk::Medium);
    assert_eq!(old.age_days, Some(40));
    assert_eq!(
        old.detail.as_deref(),
        Some("Old project: nothing in it changed for 40 days.")
    );
    assert_eq!(old.action, Action::Delete);
    assert!(!old.bulk_safe);

    let active = row(&scan, "~/Projects/active/node_modules");
    assert!(is_active(active) && !is_old(active), "{:?}", active.labels);
    assert_eq!(active.risk, Risk::High);
    assert_eq!(active.age_days, Some(1));
    assert_eq!(
        active.detail.as_deref(),
        Some("Active project: last changed 1 day ago.")
    );
    assert_eq!(active.action, Action::Delete);
    // Old projects are listed first.
    let order: Vec<&str> = rows(&scan)
        .iter()
        .map(|c| c.display_name.as_str())
        .collect();
    assert_eq!(
        order,
        [
            "~/Projects/old/node_modules",
            "~/Projects/active/node_modules"
        ]
    );
}

// The session's first script judged each folder's parent on its own and
// marked parts of an active monorepo as old.
#[test]
fn activity_is_judged_at_the_repository_root() {
    let home = basic_home();
    repo(&home, "Projects/mono");
    node(&home, "Projects/mono/apps/a");
    node(&home, "Projects/mono/apps/b");
    home.write("Projects/mono/apps/b/src/page.txt", b"edited");
    set_mtime(
        &home.path("Projects/mono/apps/b/src/page.txt"),
        days_ahead(38),
    );

    let scan = scan(&ctx_in(&home, 40));

    for name in [
        "~/Projects/mono/apps/a/node_modules",
        "~/Projects/mono/apps/b/node_modules",
    ] {
        let c = row(&scan, name);
        assert!(is_active(c), "{name}: {:?}", c.labels);
        assert_eq!(c.age_days, Some(2), "{name}");
    }
}

// A commit or checkout moves HEAD and writes the reflog. A background
// fetch or `git status` only refreshes `FETCH_HEAD` and `index`; on the Mac
// this was tried on, that alone made every project look active.
#[test]
fn a_commit_is_activity_and_a_background_fetch_or_finder_file_is_not() {
    let home = basic_home();
    node_repo(&home, "Projects/committed");
    home.write("Projects/committed/.git/logs/HEAD", b"0 1 commit\n");
    set_mtime(
        &home.path("Projects/committed/.git/logs/HEAD"),
        days_ahead(37),
    );
    node_repo(&home, "Projects/fetched");
    for file in ["FETCH_HEAD", "index", "ORIG_HEAD"] {
        let path = format!("Projects/fetched/.git/{file}");
        home.write(&path, b"abc\n");
        set_mtime(&home.path(&path), days_ahead(39));
    }
    node_repo(&home, "Projects/browsed");
    home.write("Projects/browsed/.DS_Store", b"finder");
    set_mtime(&home.path("Projects/browsed/.DS_Store"), days_ahead(39));
    // Files inside dependency folders and dot-folders do not count either.
    home.write("Projects/browsed/.idea/workspace.xml", b"ide");
    set_mtime(
        &home.path("Projects/browsed/.idea/workspace.xml"),
        days_ahead(39),
    );
    set_mtime(
        &home.path("Projects/browsed/node_modules/left-pad/index.js"),
        days_ahead(39),
    );

    let scan = scan(&ctx_in(&home, 40));

    let committed = row(&scan, "~/Projects/committed/node_modules");
    assert!(is_active(committed), "{:?}", committed.labels);
    assert_eq!(committed.age_days, Some(3));
    let fetched = row(&scan, "~/Projects/fetched/node_modules");
    assert!(is_old(fetched), "{:?}", fetched.labels);
    let browsed = row(&scan, "~/Projects/browsed/node_modules");
    assert!(is_old(browsed), "{:?}", browsed.labels);
}

// A protected folder can sit below a project (a redirected Documents
// folder, a custom user folder). Its files are not read, even for dates.
#[test]
fn a_protected_folder_inside_a_project_is_not_read_for_activity() {
    let home = basic_home();
    // The protected folder is beside the project folder, below the same
    // source-control root, which is where activity is read.
    repo(&home, "Projects/mono");
    node(&home, "Projects/mono/apps/a");
    home.write("Projects/mono/private/letter.txt", b"mine");
    set_mtime(
        &home.path("Projects/mono/private/letter.txt"),
        days_ahead(39),
    );
    let visits: Arc<Mutex<Vec<PathBuf>>> = Arc::default();
    let ctx = ctx_in(&home, 40)
        .with_protected(home.path("Projects/mono/private"))
        .with_visit_log(visits.clone());

    let scan = scan(&ctx);

    let c = row(&scan, "~/Projects/mono/apps/a/node_modules");
    assert!(is_old(c), "{:?}", c.labels);
    let private = home.path("Projects/mono/private");
    for path in visits.lock().unwrap().iter() {
        assert!(
            !path.starts_with(&private),
            "the scan read {}",
            path.display()
        );
    }
}

// The walk reports what it has looked at, so a home with no project folder
// does not sit at zero.
#[test]
fn the_walk_counts_the_files_it_looks_at_even_when_nothing_matches() {
    let home = basic_home();
    home.write("Projects/notes/readme.txt", b"text");
    home.write("Projects/notes/todo.txt", b"text");

    let scan = scan(&home.old_context());

    assert!(rows(&scan).is_empty());
    assert!(scan.progress.files_seen >= 2, "{:?}", scan.progress);
}

#[test]
fn a_timestamp_in_the_future_counts_as_active() {
    let home = basic_home();
    node_repo(&home, "Projects/clock");
    set_mtime(&home.path("Projects/clock/src/main.txt"), days_ahead(400));

    let scan = scan(&ctx_in(&home, 40));

    let c = row(&scan, "~/Projects/clock/node_modules");
    assert!(is_active(c), "{:?}", c.labels);
    assert_eq!(c.age_days, Some(0));
    assert_eq!(
        c.detail.as_deref(),
        Some("Active project: last changed today.")
    );
}

#[cfg(unix)]
#[test]
fn a_link_named_node_modules_is_never_followed_or_offered() {
    let home = basic_home();
    repo(&home, "Projects/linked");
    home.write("Projects/linked/package.json", b"{}");
    std::os::unix::fs::symlink(
        home.path("Documents"),
        home.path("Projects/linked/node_modules"),
    )
    .unwrap();

    let scan = scan(&home.old_context());

    assert!(rows(&scan).is_empty());
    assert!(
        scan.cache_skipped.iter().any(|s| {
            s.display_name == "~/Projects/linked/node_modules"
                && s.reason == space_optimiser_engine::policy::SkipReason::Link
        }),
        "{:?}",
        scan.cache_skipped
    );
    assert!(home.path("Documents/notes.txt").is_file());
}

#[test]
fn the_projects_tool_runs_one_stage_and_finds_only_its_own_row() {
    let home = basic_home();
    node_repo(&home, "Projects/web");
    let mut stages = Vec::new();
    let scan = discover::scan(
        &home.old_context(),
        ScanMode::Projects,
        &CancelToken::new(),
        |p| stages.push(p.stage),
    )
    .unwrap();

    assert_eq!(discover::stages(ScanMode::Projects), [Stage::Projects]);
    assert!(stages.iter().all(|s| *s == Stage::Projects), "{stages:?}");
    assert!(scan.temp.is_empty());
    assert!(scan.caches.iter().all(|c| c.catalog_id == ROW));
    assert!(scan.dev_assets.is_none());
}

// --- Execution -----------------------------------------------------------

const WEB: &str = "~/Projects/web/node_modules";

#[test]
fn a_confirmed_folder_is_removed_and_the_project_is_kept() {
    let home = basic_home();
    node_repo(&home, "Projects/web");
    home.write("Projects/web/.next/cache/data.bin", &[1u8; 4096]);
    home.write("Projects/web/.env", b"SECRET=1\n");

    let report = common::clean_in(&engine_in(&home, 40), ScanMode::Projects, &[WEB]);

    let done = report_item(&report, WEB);
    assert_eq!(done.outcome, Outcome::Removed, "{done:?}");
    assert!(done.bytes_removed >= 4096);
    assert_eq!(
        done.recovery_text,
        Some("Run the project's install or build command to recreate the folder.")
    );
    assert!(!home.path("Projects/web/node_modules").exists());
    for kept in [
        "Projects/web/package.json",
        "Projects/web/.git/HEAD",
        "Projects/web/src/main.txt",
        "Projects/web/.env",
        "Projects/web/.next/cache/data.bin",
    ] {
        assert!(home.path(kept).is_file(), "{kept} was removed");
    }
}

#[test]
fn a_folder_in_an_active_project_can_be_removed_when_chosen() {
    let home = basic_home();
    repo(&home, "Projects/rusty");
    home.write("Projects/rusty/Cargo.toml", b"[package]\n");
    home.write("Projects/rusty/target/CACHEDIR.TAG", CACHEDIR_TAG);
    home.write("Projects/rusty/target/debug/app", &[2u8; 4096]);

    let report = common::clean_in(
        &home.engine(),
        ScanMode::Projects,
        &["~/Projects/rusty/target"],
    );

    assert_eq!(
        report_item(&report, "~/Projects/rusty/target").outcome,
        Outcome::Removed
    );
    assert!(!home.path("Projects/rusty/target").exists());
    assert!(home.path("Projects/rusty/Cargo.toml").is_file());
}

/// Scans, plans `WEB`, confirms, lets `change` alter the home, then runs.
fn clean_after(home: &Home, change: impl FnOnce(&Home)) -> SkipCause {
    let engine = home.engine();
    let view = common::scan_in(&engine, ScanMode::Projects);
    let plan = common::plan(&engine, &view, &[WEB]);
    let token = common::confirm(&engine, &plan);
    change(home);
    let report = common::execute(&engine, &plan, token);
    let item = report_item(&report, WEB);
    assert_eq!(item.outcome, Outcome::Skipped, "{item:?}");
    assert_eq!(item.bytes_removed, 0);
    item.reason.unwrap()
}

#[test]
fn a_marker_that_went_away_after_the_scan_skips_the_folder() {
    let home = basic_home();
    node_repo(&home, "Projects/web");

    let cause = clean_after(&home, |home| {
        fs::rename(
            home.path("Projects/web/package.json"),
            home.path("Projects/web/package.json.bak"),
        )
        .unwrap();
    });

    assert_eq!(cause, SkipCause::ChangedSinceScan);
    assert!(
        home.path("Projects/web/node_modules/left-pad/index.js")
            .is_file()
    );
}

#[test]
fn a_project_that_left_source_control_after_the_scan_is_skipped() {
    let home = basic_home();
    node_repo(&home, "Projects/web");

    let cause = clean_after(&home, |home| {
        fs::rename(
            home.path("Projects/web/.git"),
            home.path("Projects/web/git-backup"),
        )
        .unwrap();
    });

    assert_eq!(cause, SkipCause::ChangedSinceScan);
    assert!(home.path("Projects/web/node_modules").is_dir());
}

#[test]
fn a_file_added_after_the_scan_skips_the_folder() {
    let home = basic_home();
    node_repo(&home, "Projects/web");

    let cause = clean_after(&home, |home| {
        home.write("Projects/web/node_modules/new-package/index.js", b"new");
    });

    assert_eq!(cause, SkipCause::ChangedSinceScan);
    assert!(
        home.path("Projects/web/node_modules/new-package/index.js")
            .is_file()
    );
    assert!(
        home.path("Projects/web/node_modules/left-pad/index.js")
            .is_file()
    );
}

#[cfg(unix)]
#[test]
fn a_folder_swapped_for_a_link_after_the_scan_is_skipped() {
    let home = basic_home();
    node_repo(&home, "Projects/web");

    let cause = clean_after(&home, |home| {
        fs::rename(
            home.path("Projects/web/node_modules"),
            home.path("Projects/web/node_modules.real"),
        )
        .unwrap();
        std::os::unix::fs::symlink(
            home.path("Documents"),
            home.path("Projects/web/node_modules"),
        )
        .unwrap();
    });

    assert_eq!(cause, SkipCause::ChangedSinceScan);
    assert!(home.path("Documents/notes.txt").is_file());
}

// "Select safe items" in the UI reads `bulk_safe`; no project row may set it.
#[test]
fn no_project_row_is_bulk_safe() {
    let home = basic_home();
    node_repo(&home, "Projects/old");
    node(&home, "Tools/app");
    let scan = scan(&ctx_in(&home, 400));
    assert_eq!(rows(&scan).len(), 2);
    assert!(rows(&scan).iter().all(|c| !c.bulk_safe));
    assert!(!ROW.bulk_safe());
}
