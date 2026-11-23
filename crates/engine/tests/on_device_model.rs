//! The `chrome-on-device-model` row: the AI model Chrome keeps in its
//! user-data folder is shown with its size in Normal Scan and Deep Scan.
//! It is a report row. Nothing can plan it, and nothing removes it: the
//! Chrome setting is what removes the model.

mod common;

use std::fs;
use std::path::PathBuf;

use common::{Home, basic_home};
use space_optimiser_engine::discover::{self, CancelToken, Scan};
use space_optimiser_engine::model::{Action, BuildPlanArgs, Candidate, Risk, ScanMode};
use space_optimiser_engine::policy::CatalogId;
use space_optimiser_engine::{Engine, EngineError};

const NAME: &str = "Google Chrome — on-device AI model";

/// Chrome's user-data folder in this OS's fixture layout: the folder that
/// holds the profiles.
fn user_data(home: &Home) -> PathBuf {
    home.chrome_profile().parent().unwrap().to_path_buf()
}

fn model_dir(home: &Home) -> PathBuf {
    user_data(home).join("OptGuideOnDeviceModel")
}

/// Another Chromium browser's user-data folder on this OS.
fn edge_user_data(home: &Home) -> PathBuf {
    if cfg!(target_os = "macos") {
        home.path("Library/Application Support/Microsoft Edge")
    } else if cfg!(windows) {
        home.path("AppData/Local/Microsoft/Edge/User Data")
    } else {
        home.path(".config/microsoft-edge")
    }
}

fn write_model(dir: &std::path::Path) {
    let version = dir.join("2025.8.21.1028");
    fs::create_dir_all(&version).unwrap();
    fs::write(version.join("weights.bin"), vec![7u8; 16 * 1024]).unwrap();
    fs::write(version.join("manifest.json"), b"{}").unwrap();
}

fn scan(home: &Home, mode: ScanMode) -> Scan {
    discover::scan(&home.old_context(), mode, &CancelToken::new(), |_| {}).unwrap()
}

fn model_rows(scan: &Scan) -> Vec<&Candidate> {
    scan.caches
        .iter()
        .filter(|c| c.catalog_id == CatalogId::ChromeOnDeviceModel)
        .collect()
}

#[test]
fn the_model_is_shown_with_its_size_in_normal_and_deep_scan() {
    let home = basic_home();
    write_model(&model_dir(&home));
    for mode in [ScanMode::Normal, ScanMode::Deep] {
        let scan = scan(&home, mode);
        let rows = model_rows(&scan);
        assert_eq!(rows.len(), 1, "{mode:?}");
        let row = rows[0];
        assert_eq!(row.display_name, NAME);
        assert_eq!(row.action, Action::Report);
        assert_eq!(row.risk, Risk::Info);
        assert!(!row.bulk_safe);
        assert!(row.revealable);
        assert_eq!(row.file_count, 2);
        assert!(row.estimated_bytes >= 16 * 1024, "{}", row.estimated_bytes);
        assert_eq!(row.path(), model_dir(&home));
        assert!(row.explanation.contains("On-device AI"));
    }
}

#[test]
fn a_computer_without_the_model_has_no_row() {
    let home = basic_home();
    assert!(model_rows(&scan(&home, ScanMode::Deep)).is_empty());
}

#[test]
fn the_model_cannot_be_planned_and_stays_on_disk() {
    let home = basic_home();
    write_model(&model_dir(&home));
    let engine: Engine = home.engine();
    let view = common::scan(&engine);
    let id = common::named(&view, NAME).id;

    let result = engine.build_plan(BuildPlanArgs {
        scan_id: view.scan_id,
        candidate_ids: vec![id],
    });

    assert_eq!(result.unwrap_err(), EngineError::NotActionable);
    assert!(
        model_dir(&home)
            .join("2025.8.21.1028/weights.bin")
            .is_file()
    );
}

// Only Chrome's own user-data folder counts. A folder of the same name in
// another browser, inside a profile, or in Documents is not this row.
#[test]
fn a_folder_of_the_same_name_anywhere_else_is_not_listed() {
    let home = basic_home();
    write_model(&edge_user_data(&home).join("OptGuideOnDeviceModel"));
    write_model(&home.chrome_profile().join("OptGuideOnDeviceModel"));
    write_model(&home.path("Documents/OptGuideOnDeviceModel"));

    let scan = scan(&home, ScanMode::Deep);

    assert!(model_rows(&scan).is_empty());
}

#[cfg(unix)]
#[test]
fn a_link_in_place_of_the_model_is_left_alone() {
    let home = basic_home();
    std::os::unix::fs::symlink(home.path("Documents"), model_dir(&home)).unwrap();

    let scan = scan(&home, ScanMode::Normal);

    assert!(model_rows(&scan).is_empty());
    assert!(
        scan.cache_skipped.iter().any(|s| s.display_name == NAME
            && s.reason == space_optimiser_engine::policy::SkipReason::Link),
        "{:?}",
        scan.cache_skipped
    );
    assert!(home.path("Documents/notes.txt").is_file());
}

// The Projects and Android tools look only at their own rows.
#[test]
fn the_other_tools_do_not_report_the_model() {
    let home = basic_home();
    write_model(&model_dir(&home));
    for mode in [ScanMode::Projects, ScanMode::Android] {
        assert!(model_rows(&scan(&home, mode)).is_empty(), "{mode:?}");
    }
}
