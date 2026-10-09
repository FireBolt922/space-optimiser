//! Catalog ids, entry rules, and path containment (phase 4).

mod common;

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::time::{Duration, SystemTime};

use space_optimiser_engine::model::{Action, Risk};
use space_optimiser_engine::policy::{self, CatalogId, PathRejection, contained_in};

const DAY: Duration = Duration::from_secs(24 * 60 * 60);

#[test]
fn rust_catalog_ids_match_the_checked_in_list() {
    let list = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/catalog-mvp-ids.txt"),
    )
    .unwrap();
    let listed: BTreeSet<String> = list
        .lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .map(|line| line.trim().trim_matches('"').to_owned())
        .collect();
    let rust: BTreeSet<String> = CatalogId::ALL
        .iter()
        .map(|id| id.as_str().to_owned())
        .collect();
    assert_eq!(listed, rust);
}

#[test]
fn catalog_ids_serialize_as_their_catalog_names() {
    for id in CatalogId::ALL {
        assert_eq!(serde_json::to_value(id).unwrap(), id.as_str());
    }
}

#[test]
fn only_low_risk_rows_are_bulk_safe() {
    for id in CatalogId::ALL {
        if id.bulk_safe() {
            assert_eq!(id.risk(), Risk::Low, "{}", id.as_str());
        }
        assert!(!id.explanation().is_empty(), "{}", id.as_str());
    }
    assert!(CatalogId::UserTemp.bulk_safe());
    assert!(!CatalogId::BrowserHttpCache.bulk_safe());
    assert_eq!(CatalogId::LargeFilesReport.action(), Action::Report);
    assert_eq!(CatalogId::WindowsThumbcache.action(), Action::Report);
}

// Test 13: age is the newest of mtime and ctime; the future counts as fresh.
#[test]
fn an_old_mtime_with_a_fresh_ctime_is_not_old() {
    let now = SystemTime::now();
    let old = now - 400 * DAY;
    assert!(!policy::is_old(old, now - DAY / 2, now, DAY));
    assert!(policy::is_old(old, now - 2 * DAY, now, DAY));
}

#[test]
fn a_future_timestamp_is_fresh() {
    let now = SystemTime::now();
    assert!(!policy::is_old(now + DAY, now - 400 * DAY, now, DAY));
    assert!(!policy::is_old(now - 400 * DAY, now + 400 * DAY, now, DAY));
}

#[test]
fn project_markers_are_recognised_by_name() {
    for name in [
        ".git",
        ".hg",
        ".svn",
        "package.json",
        "Cargo.toml",
        "go.mod",
        "app.sln",
        "App.csproj",
        "Makefile",
    ] {
        assert!(policy::is_project_marker(OsStr::new(name)), "{name}");
    }
    for name in ["notes.txt", "sln", "cargo.lock", ".gitignore"] {
        assert!(!policy::is_project_marker(OsStr::new(name)), "{name}");
    }
}

// Test 4: a path with `..` that escapes the root is rejected.
#[test]
fn a_dotdot_path_that_escapes_the_root_is_rejected() {
    let home = common::basic_home();
    let tmp = home.path("tmp");
    let spell = |relative| common::spelled(&tmp, relative);
    assert_eq!(
        contained_in(&tmp, &spell("../Documents/notes.txt")),
        Err(PathRejection::Escapes)
    );
    assert_eq!(
        contained_in(&tmp, &spell("old-build/../../Documents")),
        Err(PathRejection::Escapes)
    );
    assert_eq!(
        contained_in(&tmp, &spell("..")),
        Err(PathRejection::Escapes)
    );
    let inside = contained_in(&tmp, &spell("old-build/../old-download.bin")).unwrap();
    assert_eq!(inside, tmp.join("old-download.bin"));
}

#[cfg(unix)]
#[test]
fn a_symlinked_parent_that_escapes_the_root_is_rejected() {
    let home = common::basic_home();
    let tmp = home.path("tmp");
    std::os::unix::fs::symlink(home.path("Documents"), tmp.join("docs")).unwrap();
    assert!(contained_in(&tmp, &tmp.join("docs/notes.txt")).is_err());
    // The link itself is inside the root; its target is not followed.
    assert_eq!(
        contained_in(&tmp, &tmp.join("docs")).unwrap(),
        tmp.join("docs")
    );
}
