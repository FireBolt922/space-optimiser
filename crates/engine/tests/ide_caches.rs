//! The `ide-index-caches` row (Deep Scan): VS Code, Cursor, and JetBrains
//! cache folders, found by exact name, "quit first" while the editor runs,
//! and emptied through the same executor as every other cache row. Settings,
//! local history, and anything else next to a cache are never candidates.

mod common;

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use common::{Home, basic_home};
use space_optimiser_engine::Engine;
use space_optimiser_engine::discover::{self, CancelToken, Scan};
use space_optimiser_engine::model::{
    Action, Candidate, Label, Outcome, Platform, Report, Risk, SkipCause,
};
use space_optimiser_engine::policy::{CatalogId, Context};
use space_optimiser_engine::roots::{self, App};

fn deep(ctx: &Context) -> Scan {
    discover::scan_deep(ctx, &CancelToken::new(), |_| {}).unwrap()
}

fn rows(scan: &Scan) -> Vec<&Candidate> {
    scan.caches
        .iter()
        .filter(|c| c.catalog_id == CatalogId::IdeIndexCaches)
        .collect()
}

/// Every folder the row lists for `platform`, as paths.
fn root_paths(ctx: &Context, platform: Platform) -> BTreeSet<PathBuf> {
    roots::cache_roots(ctx, CatalogId::IdeIndexCaches, platform)
        .into_iter()
        .map(|r| {
            r.root
                .expect("fixture roots are valid")
                .path()
                .to_path_buf()
        })
        .collect()
}

/// The name VS Code's main process has on this OS.
fn vscode_process() -> &'static str {
    if cfg!(target_os = "macos") {
        "Code"
    } else if cfg!(windows) {
        "Code.exe"
    } else {
        "code"
    }
}

/// The main process of the JetBrains IDE in this OS's fixture layout.
fn jetbrains_process() -> &'static str {
    if cfg!(target_os = "macos") {
        "idea"
    } else if cfg!(windows) {
        "webstorm64.exe"
    } else {
        "pycharm"
    }
}

/// The JetBrains display name in this OS's fixture layout.
fn jetbrains_label() -> &'static str {
    if cfg!(target_os = "macos") {
        "IntelliJ IDEA 2024.3"
    } else if cfg!(windows) {
        "WebStorm 2025.1"
    } else {
        "PyCharm 2024.2"
    }
}

/// This OS's VS Code user-data folder in the fixture layout.
fn vscode_dir(home: &Home) -> PathBuf {
    if cfg!(target_os = "macos") {
        home.path("Library/Application Support/Code")
    } else if cfg!(windows) {
        home.path("AppData/Roaming/Code")
    } else {
        home.path(".config/Code")
    }
}

/// How many VS Code cache folders this OS's fixture layout holds.
fn vscode_folders() -> usize {
    if cfg!(target_os = "macos") { 5 } else { 2 }
}

/// This OS's JetBrains product folder in the fixture layout.
fn jetbrains_dir(home: &Home) -> PathBuf {
    if cfg!(target_os = "macos") {
        home.path("Library/Caches/JetBrains/IntelliJIdea2024.3")
    } else if cfg!(windows) {
        home.path("AppData/Local/JetBrains/WebStorm2025.1")
    } else {
        home.path(".cache/JetBrains/PyCharmCE2024.2")
    }
}

#[test]
fn only_the_named_editor_cache_folders_match_on_each_layout() {
    let home = basic_home();
    let ctx = home.old_context();
    let layouts = [
        (
            Platform::Macos,
            vec![
                "Library/Application Support/Code/Cache",
                "Library/Application Support/Code/CachedData",
                "Library/Application Support/Code/Code Cache",
                "Library/Application Support/Code/GPUCache",
                "Library/Application Support/Code/CachedExtensionVSIXs",
                "Library/Application Support/Cursor/CachedData",
                "Library/Caches/JetBrains/IntelliJIdea2024.3/caches",
                "Library/Caches/JetBrains/IntelliJIdea2024.3/index",
            ],
        ),
        (
            Platform::Linux,
            vec![
                ".config/Code/Cache",
                ".config/Code/CachedData",
                ".config/Cursor/CachedData",
                ".cache/JetBrains/PyCharmCE2024.2/caches",
                ".cache/JetBrains/PyCharmCE2024.2/index",
            ],
        ),
        (
            Platform::Windows,
            vec![
                "AppData/Roaming/Code/Cache",
                "AppData/Roaming/Code/CachedData",
                "AppData/Roaming/Cursor/CachedData",
                "AppData/Local/JetBrains/WebStorm2025.1/caches",
                "AppData/Local/JetBrains/WebStorm2025.1/index",
            ],
        ),
    ];
    for (platform, expected) in layouts {
        let expected: BTreeSet<_> = expected.iter().map(|p| home.path(p)).collect();
        assert_eq!(root_paths(&ctx, platform), expected, "{platform:?}");
    }
}

// Settings, site storage, backups, local history, scratch files, and a
// JetBrains folder that is not a product (Toolbox) are never candidates,
// even when they hold a folder called `caches`.
#[test]
fn editor_settings_history_and_unknown_jetbrains_folders_are_never_candidates() {
    let home = basic_home();
    let scan = deep(&home.old_context());
    let code = vscode_dir(&home);
    let jetbrains = jetbrains_dir(&home);
    let mut kept = vec![
        code.join("User"),
        code.join("Local Storage"),
        code.join("Backups"),
        jetbrains.join("LocalHistory"),
        jetbrains.join("tmp"),
        home.path("Library/Caches/JetBrains/Toolbox"),
    ];
    if cfg!(target_os = "macos") {
        kept.push(home.path("Library/Caches/JetBrains/Toolbox/caches"));
    }
    for c in &scan.caches {
        for sentinel in &kept {
            assert!(
                !c.path().starts_with(sentinel) && !sentinel.starts_with(c.path()),
                "{} covers {}",
                c.display_name,
                sentinel.display()
            );
        }
    }
    // The product folder itself is never a candidate: only its two subfolders.
    assert!(scan.caches.iter().all(|c| c.path() != jetbrains));
    assert!(scan.caches.iter().all(|c| c.path() != code));
}

#[test]
fn editor_rows_are_medium_risk_deletes_that_are_not_bulk_safe() {
    let home = basic_home();
    let scan = deep(&home.old_context());
    let rows = rows(&scan);
    assert_eq!(rows.len(), vscode_folders() + 3, "{:?}", names(&rows));
    for c in &rows {
        assert_eq!(c.action, Action::Delete, "{}", c.display_name);
        assert_eq!(c.risk, Risk::Medium);
        assert!(!c.bulk_safe);
        assert!(c.estimated_bytes > 0);
        assert!(c.quit_app.is_some(), "{}", c.display_name);
    }
}

#[test]
fn display_names_say_which_editor_and_which_folder() {
    let home = basic_home();
    let scan = deep(&home.old_context());
    let names: BTreeSet<_> = rows(&scan).iter().map(|c| c.display_name.clone()).collect();
    assert!(names.contains("VS Code — CachedData"), "{names:?}");
    assert!(names.contains("Cursor — CachedData"), "{names:?}");
    assert!(
        names.contains(&format!("{} — caches", jetbrains_label())),
        "{names:?}"
    );
    let code = rows(&scan)
        .into_iter()
        .find(|c| c.display_name == "VS Code — CachedData")
        .unwrap();
    assert_eq!(code.quit_app, Some("VS Code"));
}

// A running editor makes only its own rows "quit first".
#[test]
fn a_running_editor_blocks_only_its_own_rows() {
    let home = basic_home();
    let ctx = home
        .old_context()
        .with_running_processes(&[vscode_process()]);
    let scan = deep(&ctx);
    let rows = rows(&scan);
    let (code, others): (Vec<_>, Vec<_>) = rows
        .iter()
        .partition(|c| c.display_name.starts_with("VS Code"));
    assert_eq!(code.len(), vscode_folders(), "{:?}", names(&code));
    for c in code {
        assert_eq!(c.action, Action::None, "{}", c.display_name);
        assert!(c.labels.contains(&Label::QuitFirst));
    }
    assert_eq!(others.len(), 3, "{:?}", names(&others));
    for c in others {
        assert_eq!(c.action, Action::Delete, "{}", c.display_name);
        assert!(!c.labels.contains(&Label::QuitFirst));
    }
}

#[test]
fn a_running_jetbrains_ide_blocks_only_that_product() {
    let home = basic_home();
    let ctx = home
        .old_context()
        .with_running_processes(&[jetbrains_process()]);
    let scan = deep(&ctx);
    for c in rows(&scan) {
        let blocked = c.display_name.starts_with(jetbrains_label());
        assert_eq!(c.action == Action::None, blocked, "{}", c.display_name);
        assert_eq!(c.labels.contains(&Label::QuitFirst), blocked);
    }
}

#[test]
fn every_editor_row_is_blocked_when_processes_cannot_be_listed() {
    let home = basic_home();
    let scan = deep(&home.old_context().with_process_list_error());
    let rows = rows(&scan);
    assert!(!rows.is_empty());
    assert!(rows.iter().all(|c| c.action == Action::None));
}

#[test]
fn process_names_map_to_editors_per_os() {
    let running = |names: &[&str], platform| {
        let names: Vec<String> = names.iter().map(|n| n.to_string()).collect();
        roots::running_apps(&names, platform)
    };
    assert_eq!(running(&["Code"], Platform::Macos), [App::VsCode]);
    assert_eq!(running(&["code"], Platform::Linux), [App::VsCode]);
    assert_eq!(running(&["CODE.EXE"], Platform::Windows), [App::VsCode]);
    assert_eq!(running(&["Cursor"], Platform::Macos), [App::Cursor]);
    assert_eq!(running(&["idea"], Platform::Macos), [App::IntelliJIdea]);
    assert_eq!(running(&["pycharm"], Platform::Linux), [App::PyCharm]);
    assert_eq!(
        running(&["webstorm64.exe"], Platform::Windows),
        [App::WebStorm]
    );
    // A helper, a shell, or a browser is not an editor; browsers still map.
    assert!(running(&["Code Helper (Renderer)"], Platform::Macos).is_empty());
    assert!(running(&["Cursor Helper (Plugin)"], Platform::Macos).is_empty());
    assert!(running(&["code"], Platform::Macos).is_empty());
    assert_eq!(running(&["Google Chrome"], Platform::Macos), [App::Chrome]);
}

// Normal Scan never looks at editor folders.
#[test]
fn normal_scan_does_not_visit_editor_folders() {
    let home = basic_home();
    let visits: Arc<Mutex<Vec<PathBuf>>> = Arc::default();
    let ctx = home.old_context().with_visit_log(visits.clone());
    discover::scan_normal(&ctx, &CancelToken::new(), |_| {}).unwrap();
    let code = vscode_dir(&home);
    let jetbrains = jetbrains_dir(&home);
    for path in visits.lock().unwrap().iter() {
        assert!(
            !path.starts_with(&code) && !path.starts_with(&jetbrains),
            "Normal Scan visited {}",
            path.display()
        );
    }
}

fn names(rows: &[&Candidate]) -> Vec<String> {
    rows.iter().map(|c| c.display_name.clone()).collect()
}

// --- Execution -----------------------------------------------------------

fn item<'a>(report: &'a Report, name: &str) -> &'a space_optimiser_engine::model::ReportItem {
    report
        .items
        .iter()
        .find(|i| i.display_name == name)
        .unwrap_or_else(|| panic!("no report item {name}"))
}

fn clean_deep(engine: &Engine, names: &[&str]) -> Report {
    let view = common::deep_scan(engine);
    let plan = common::plan(engine, &view, names);
    let token = common::confirm(engine, &plan);
    common::execute(engine, &plan, token)
}

fn skipped(report: &Report, name: &str) -> SkipCause {
    let item = item(report, name);
    assert_eq!(item.outcome, Outcome::Skipped, "{item:?}");
    item.reason.unwrap()
}

#[test]
fn a_confirmed_editor_cache_is_emptied_and_settings_are_kept() {
    let home = basic_home();
    let report = clean_deep(&home.engine(), &["VS Code — CachedData"]);

    let done = item(&report, "VS Code — CachedData");
    assert_eq!(done.outcome, Outcome::Removed, "{done:?}");
    assert!(done.bytes_removed > 0);
    assert_eq!(
        done.recovery_text,
        Some("The editor rebuilds its caches the next time it starts.")
    );
    let cache = vscode_dir(&home).join("CachedData");
    assert!(cache.is_dir(), "the cache folder itself stays");
    assert_eq!(fs::read_dir(&cache).unwrap().count(), 0);
    assert!(vscode_dir(&home).join("User/settings.json").is_file());
    assert!(
        vscode_dir(&home)
            .join("Cache/Cache_Data/f_000001")
            .is_file()
    );
}

#[test]
fn a_confirmed_jetbrains_index_is_emptied_and_local_history_is_kept() {
    let home = basic_home();
    let name = format!("{} — index", jetbrains_label());
    let report = clean_deep(&home.engine(), &[&name]);

    assert_eq!(item(&report, &name).outcome, Outcome::Removed);
    let index = jetbrains_dir(&home).join("index");
    assert!(index.is_dir());
    assert_eq!(fs::read_dir(&index).unwrap().count(), 0);
    assert!(
        jetbrains_dir(&home)
            .join("LocalHistory/changes.storageData")
            .is_file()
    );
    assert!(jetbrains_dir(&home).join("caches/content.dat").is_file());
}

#[test]
fn an_editor_opened_after_the_scan_keeps_its_cache() {
    let home = basic_home();
    let running: Arc<Mutex<Vec<String>>> = Arc::default();
    let shared = running.clone();
    let engine = Engine::new(
        home.config_with(move |ctx| ctx.with_running_processes(&shared.lock().unwrap())),
    );
    let view = common::deep_scan(&engine);
    let plan = common::plan(&engine, &view, &["VS Code — CachedData"]);
    let token = common::confirm(&engine, &plan);
    running.lock().unwrap().push(vscode_process().to_owned());

    let report = common::execute(&engine, &plan, token);

    assert_eq!(
        skipped(&report, "VS Code — CachedData"),
        SkipCause::AppRunning
    );
    assert!(
        vscode_dir(&home)
            .join("CachedData/abc123/main.js.code")
            .is_file()
    );
}

#[test]
fn a_running_editor_cannot_be_planned() {
    let home = basic_home();
    let engine =
        Engine::new(home.config_with(|ctx| ctx.with_running_processes(&[vscode_process()])));
    let view = common::deep_scan(&engine);
    let result = engine.build_plan(space_optimiser_engine::model::BuildPlanArgs {
        scan_id: view.scan_id,
        candidate_ids: vec![common::named(&view, "VS Code — CachedData").id],
    });
    assert_eq!(
        result.unwrap_err(),
        space_optimiser_engine::EngineError::NotActionable
    );
}
