//! Phase 11 test 2: Normal Scan visits only the catalog's roots for this OS.
//! The expected roots below are written from the cleanup catalog, not read
//! from the engine, so a new location in the engine fails here until the
//! catalog and this list agree.

mod common;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use common::{Home, basic_home};
use space_optimiser_engine::discover::{self, CancelToken};

/// Chromium-family vendor folders, as the catalog names them per OS.
#[cfg(target_os = "macos")]
const CHROMIUM: &[&str] = &[
    "Google/Chrome",
    "Google/Chrome Beta",
    "Google/Chrome Dev",
    "Google/Chrome Canary",
    "Microsoft Edge",
    "Microsoft Edge Beta",
    "Microsoft Edge Dev",
    "Microsoft Edge Canary",
    "BraveSoftware/Brave-Browser",
    "BraveSoftware/Brave-Browser-Beta",
    "BraveSoftware/Brave-Browser-Nightly",
];

#[cfg(all(unix, not(target_os = "macos")))]
const CHROMIUM: &[&str] = &[
    "google-chrome",
    "google-chrome-beta",
    "google-chrome-unstable",
    "microsoft-edge",
    "microsoft-edge-beta",
    "microsoft-edge-dev",
    "BraveSoftware/Brave-Browser",
    "BraveSoftware/Brave-Browser-Beta",
    "BraveSoftware/Brave-Browser-Nightly",
    "chromium",
];

#[cfg(windows)]
const CHROMIUM: &[&str] = &[
    "Google/Chrome",
    "Google/Chrome Beta",
    "Google/Chrome Dev",
    "Google/Chrome SxS",
    "Microsoft/Edge",
    "Microsoft/Edge Beta",
    "Microsoft/Edge Dev",
    "Microsoft/Edge SxS",
    "BraveSoftware/Brave-Browser",
    "BraveSoftware/Brave-Browser-Beta",
    "BraveSoftware/Brave-Browser-Nightly",
];

/// Every folder Normal Scan may read, relative to the fixture home: the
/// temp root, the fixed cache folders, and the browser folders whose
/// profiles it lists.
fn catalog_roots() -> Vec<String> {
    let mut roots = vec!["tmp".to_owned()];
    if cfg!(target_os = "macos") {
        for base in ["Library/Caches", "Library/Application Support"] {
            roots.extend(CHROMIUM.iter().map(|v| format!("{base}/{v}")));
        }
        roots.push("Library/Caches/Firefox/Profiles".into());
    } else if cfg!(windows) {
        roots.extend(
            CHROMIUM
                .iter()
                .map(|v| format!("AppData/Local/{v}/User Data")),
        );
        roots.push("AppData/Local/Mozilla/Firefox/Profiles".into());
        roots.push("AppData/Local/Microsoft/Windows/Explorer".into());
    } else {
        roots.push(".cache/thumbnails".into());
        roots.push(".cache/fontconfig".into());
        for base in [".cache", ".config"] {
            roots.extend(CHROMIUM.iter().map(|v| format!("{base}/{v}")));
        }
        roots.push("snap/chromium/common/.cache/chromium".into());
        roots.push("snap/chromium/common/chromium".into());
        for (id, folder) in [
            ("com.google.Chrome", "google-chrome"),
            ("com.microsoft.Edge", "microsoft-edge"),
            ("com.brave.Browser", "BraveSoftware/Brave-Browser"),
            ("org.chromium.Chromium", "chromium"),
        ] {
            roots.push(format!(".var/app/{id}/cache/{folder}"));
            roots.push(format!(".var/app/{id}/config/{folder}"));
        }
        roots.push(".cache/mozilla/firefox".into());
        roots.push("snap/firefox/common/.cache/mozilla/firefox".into());
        roots.push(".var/app/org.mozilla.firefox/cache/mozilla/firefox".into());
    }
    roots
}

fn visited_by_normal_scan(home: &Home) -> Vec<PathBuf> {
    let log = Arc::new(Mutex::new(Vec::new()));
    let ctx = home.old_context().with_visit_log(log.clone());
    let scan = discover::scan_normal(&ctx, &CancelToken::new(), |_| {}).unwrap();
    assert!(!scan.caches.is_empty() || !scan.temp.is_empty());
    let visited = log.lock().unwrap().clone();
    assert!(!visited.is_empty(), "the visit log recorded nothing");
    visited
}

#[test]
fn normal_scan_visits_only_catalog_roots() {
    let home = basic_home();
    let roots: Vec<PathBuf> = catalog_roots().iter().map(|r| home.path(r)).collect();

    let visited = visited_by_normal_scan(&home);

    for path in &visited {
        assert!(
            roots.iter().any(|root| path.starts_with(root)),
            "Normal Scan read {}, which is under no catalog root",
            path.display()
        );
    }
}

#[test]
fn normal_scan_visits_every_catalog_root_the_fixture_has() {
    let home = basic_home();
    let visited = visited_by_normal_scan(&home);

    let mut present = 0;
    for root in catalog_roots() {
        let path = home.path(&root);
        if !path.is_dir() {
            continue;
        }
        present += 1;
        assert!(
            visited.contains(&path),
            "Normal Scan did not read the catalog root {root}"
        );
    }
    assert!(present >= 2, "the fixture has too few roots for this OS");
}

#[test]
fn normal_scan_reads_nothing_outside_the_catalog_even_with_lookalikes() {
    let home = basic_home();
    // Folders a careless matcher would take for caches, outside every root.
    home.write("Documents/Google/Chrome/Default/Cache/keep.txt", b"mine");
    home.write("projects/app/.cache/thumbnails/keep.png", b"mine");
    home.write("Downloads/tmp/old.bin", b"mine");
    let roots: Vec<PathBuf> = catalog_roots().iter().map(|r| home.path(r)).collect();

    for path in visited_by_normal_scan(&home) {
        assert!(
            roots.iter().any(|root| path.starts_with(root)),
            "Normal Scan read {}",
            path.display()
        );
        for outside in ["Documents", "projects", "Downloads"] {
            assert!(!path.starts_with(home.path(outside)), "{}", path.display());
        }
    }
}
