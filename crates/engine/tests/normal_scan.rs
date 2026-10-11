//! Normal Scan rows on fixture homes (phase 5): temp, thumbnails, fonts,
//! Explorer's thumbnail database, and browser caches with the profile
//! carve-out. Nothing here deletes.

mod common;

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use common::{Home, basic_home, chrome_process};
use space_optimiser_engine::discover::{self, CancelToken, NormalScan, RootOutcome};
use space_optimiser_engine::model::{Action, Candidate, Label, Platform, Risk};
use space_optimiser_engine::policy::{CatalogId, Context};
use space_optimiser_engine::roots::{self, Browser};

fn scan(ctx: &Context) -> NormalScan {
    discover::scan_normal(ctx, &CancelToken::new(), |_| {}).unwrap()
}

fn all_candidates(scan: &NormalScan) -> Vec<&Candidate> {
    let temp = scan.temp.iter().flat_map(|root| match root {
        RootOutcome::Scanned { candidates, .. } => candidates.as_slice(),
        RootOutcome::Skipped { .. } => &[],
    });
    temp.chain(scan.caches.iter()).collect()
}

/// The cache candidate for `path`, from a scan of the fixture.
#[cfg(any(target_os = "linux", windows))]
fn at(home: &Home, path: &std::path::Path) -> Option<Candidate> {
    scan(&home.old_context())
        .caches
        .into_iter()
        .find(|c| c.path() == path)
}

/// Every cache folder a row lists for `platform`, as paths.
fn root_paths(ctx: &Context, row: CatalogId, platform: Platform) -> Vec<PathBuf> {
    roots::cache_roots(ctx, row, platform)
        .into_iter()
        .map(|r| {
            r.root
                .expect("fixture roots are valid")
                .path()
                .to_path_buf()
        })
        .collect()
}

fn chrome_profile(home: &Home) -> PathBuf {
    home.chrome_profile()
}

fn chrome_cache(home: &Home) -> PathBuf {
    home.chrome_cache()
}

// Test 1: the 2-day-old temp file is a bulk-safe candidate; the 1-hour-old
// file is not a candidate.
#[test]
fn old_temp_files_are_bulk_safe_and_new_ones_are_not_candidates() {
    let home = basic_home();
    let fresh = home.write("tmp/new-download.part", b"partial");
    common::set_mtime(&fresh, common::a_week_ahead());
    let result = scan(&home.old_context());
    let candidates = all_candidates(&result);

    let old = candidates
        .iter()
        .find(|c| c.display_name == "old-download.bin")
        .unwrap();
    assert!(old.bulk_safe);
    assert_eq!(old.catalog_id, CatalogId::UserTemp);
    assert!(
        candidates
            .iter()
            .all(|c| c.display_name != "new-download.part")
    );
}

// Test 2: `Cookies` is never a candidate; the cache is medium and not
// bulk-safe.
#[test]
fn cookies_are_never_candidates_and_the_cache_is_medium() {
    let home = basic_home();
    let result = scan(&home.old_context());
    let candidates = all_candidates(&result);

    for c in &candidates {
        let name = c.path().file_name().unwrap();
        assert_ne!(name, "Cookies", "{}", c.path().display());
        assert_ne!(name, "CacheBackup", "{}", c.path().display());
        assert_ne!(
            c.path(),
            chrome_profile(&home),
            "a profile is never a target"
        );
    }
    let cache = candidates
        .iter()
        .find(|c| c.path() == chrome_cache(&home))
        .expect("the Chrome cache is a candidate");
    assert_eq!(cache.catalog_id, CatalogId::BrowserHttpCache);
    assert_eq!(cache.risk, Risk::Medium);
    assert!(!cache.bulk_safe);
    assert_eq!(cache.action, Action::Delete);
    assert!(cache.estimated_bytes > 0);
    assert_eq!(cache.file_count, 1);
    assert!(chrome_profile(&home).join("Cookies").is_file());
}

// Test 3: "Select safe items" takes temp, plus thumbnails and fonts on
// Ubuntu, and nothing else.
#[test]
fn only_temp_thumbnail_and_font_rows_are_bulk_safe() {
    let home = basic_home();
    let result = scan(&home.old_context());
    let safe: BTreeSet<_> = all_candidates(&result)
        .iter()
        .filter(|c| c.bulk_safe)
        .map(|c| c.catalog_id.as_str())
        .collect();
    let expected: BTreeSet<_> = if cfg!(target_os = "linux") {
        ["user-temp", "thumbnail-cache", "font-cache"].into()
    } else {
        ["user-temp"].into()
    };
    assert_eq!(safe, expected);
}

// Test 4: rows return no roots on the platforms the catalog leaves out.
#[test]
fn rows_have_no_roots_where_the_catalog_disables_them() {
    let home = basic_home();
    let ctx = home.old_context();
    let current = Platform::CURRENT;
    if !cfg!(windows) {
        assert!(root_paths(&ctx, CatalogId::WindowsThumbcache, current).is_empty());
    }
    if !cfg!(target_os = "linux") {
        assert!(root_paths(&ctx, CatalogId::ThumbnailCache, current).is_empty());
        assert!(root_paths(&ctx, CatalogId::FontCache, current).is_empty());
    }
    for platform in [Platform::Macos, Platform::Windows] {
        assert!(root_paths(&ctx, CatalogId::ThumbnailCache, platform).is_empty());
        assert!(root_paths(&ctx, CatalogId::FontCache, platform).is_empty());
    }
    for platform in [Platform::Macos, Platform::Linux] {
        assert!(root_paths(&ctx, CatalogId::WindowsThumbcache, platform).is_empty());
    }
    // Temp, the tool rows, and the large-file report have no fixed cache
    // folders; old logs have one on macOS only (phase 6).
    for row in [
        CatalogId::UserTemp,
        CatalogId::PkgNpmCache,
        CatalogId::PkgPnpmStore,
        CatalogId::PkgYarnCache,
        CatalogId::PkgPipCache,
        CatalogId::PkgUvCache,
        CatalogId::DockerBuildCache,
        CatalogId::LargeFilesReport,
    ] {
        assert!(root_paths(&ctx, row, current).is_empty(), "{row:?}");
    }
    for platform in [Platform::Linux, Platform::Windows] {
        assert!(root_paths(&ctx, CatalogId::UserLogsOld, platform).is_empty());
    }
}

// Test 5: Normal Scan reads nothing inside `Documents`.
#[test]
fn normal_scan_reads_nothing_inside_documents() {
    let home = basic_home();
    // A folder in Documents that a careless matcher would take for a cache.
    home.write("Documents/Google/Chrome/Default/Cache/keep.txt", b"mine");
    let log = Arc::new(Mutex::new(Vec::new()));
    let ctx = home.old_context().with_visit_log(log.clone());

    let result = scan(&ctx);

    let documents = home.path("Documents");
    let visited = log.lock().unwrap();
    assert!(!visited.is_empty(), "the visit log recorded nothing");
    for path in visited.iter() {
        assert!(!path.starts_with(&documents), "read {}", path.display());
    }
    assert!(!all_candidates(&result).is_empty());
}

// Test 6: an entry the user cannot read skips the whole cache folder.
#[cfg(unix)]
#[test]
fn an_unreadable_folder_inside_a_cache_skips_the_cache() {
    use std::os::unix::fs::PermissionsExt;

    // SAFETY: geteuid has no preconditions.
    if unsafe { libc::geteuid() } == 0 {
        eprintln!("skipped: root ignores directory permissions");
        return;
    }
    let home = basic_home();
    let cache = chrome_cache(&home);
    let locked = cache.join("Cache_Data/locked");
    fs::create_dir_all(&locked).unwrap();
    fs::write(locked.join("entry"), b"x").unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();

    let result = scan(&home.old_context());
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();

    assert!(result.caches.iter().all(|c| c.path() != cache));
    let skip = result
        .cache_skipped
        .iter()
        .find(|s| s.display_name == common::CHROME_CACHE)
        .expect("the cache is reported as skipped");
    assert_eq!(skip.reason, discover::SkipReason::Unreadable);
}

// Test 7 and the carve-out, for every OS's layout: only `Cache`,
// `Code Cache`, and `GPUCache` one level below a profile, and Firefox
// `cache2`. `Cookies` and the `CacheBackup` sibling never match.
#[test]
fn only_the_named_cache_folders_match_on_each_layout() {
    let home = basic_home();
    let ctx = home.old_context();
    let layouts = [
        (
            Platform::Windows,
            vec![
                "AppData/Local/Google/Chrome/User Data/Default/Cache",
                "AppData/Local/Google/Chrome/User Data/Default/Code Cache",
                "AppData/Local/Mozilla/Firefox/Profiles/abcd1234.default-release/cache2",
            ],
        ),
        (
            Platform::Macos,
            vec![
                "Library/Caches/Google/Chrome/Default/Cache",
                "Library/Caches/Google/Chrome/Default/Code Cache",
                "Library/Caches/Google/Chrome/Profile 1/Cache",
                "Library/Application Support/Google/Chrome/Default/GPUCache",
                "Library/Caches/Firefox/Profiles/abcd1234.default-release/cache2",
            ],
        ),
        (
            Platform::Linux,
            vec![
                ".cache/google-chrome/Default/Cache",
                ".cache/google-chrome/Default/Code Cache",
                ".config/google-chrome/Default/GPUCache",
                "snap/firefox/common/.cache/mozilla/firefox/abcd1234.default/cache2",
                ".var/app/org.chromium.Chromium/cache/chromium/Default/Cache",
            ],
        ),
    ];
    for (platform, expected) in layouts {
        let found: BTreeSet<_> = root_paths(&ctx, CatalogId::BrowserHttpCache, platform)
            .into_iter()
            .collect();
        let expected: BTreeSet<_> = expected.iter().map(|p| home.path(p)).collect();
        assert_eq!(found, expected, "{platform:?}");
    }
}

// Test 8: a running browser makes its rows "quit first", not actionable.
// Other browsers stay actionable.
#[test]
fn a_running_browser_makes_its_rows_not_actionable() {
    let home = basic_home();
    let ctx = home
        .old_context()
        .with_running_processes(&[chrome_process()]);
    let result = scan(&ctx);

    let browser: Vec<_> = result
        .caches
        .iter()
        .filter(|c| c.catalog_id == CatalogId::BrowserHttpCache)
        .collect();
    let chrome: Vec<_> = browser
        .iter()
        .filter(|c| c.display_name.starts_with("Google Chrome"))
        .collect();
    assert!(!chrome.is_empty());
    for c in &chrome {
        assert_eq!(c.action, Action::None, "{}", c.display_name);
        assert!(c.labels.contains(&Label::QuitFirst));
    }
    let firefox = browser
        .iter()
        .find(|c| c.display_name.starts_with("Firefox"))
        .expect("a Firefox cache in this OS's fixture layout");
    assert_eq!(firefox.action, Action::Delete);
    assert!(!firefox.labels.contains(&Label::QuitFirst));
}

#[test]
fn every_browser_row_is_blocked_when_processes_cannot_be_listed() {
    let home = basic_home();
    let ctx = home.old_context().with_process_list_error();
    let result = scan(&ctx);
    let browser: Vec<_> = result
        .caches
        .iter()
        .filter(|c| c.catalog_id == CatalogId::BrowserHttpCache)
        .collect();
    assert!(!browser.is_empty());
    assert!(browser.iter().all(|c| c.action == Action::None));
}

#[test]
fn process_names_map_to_browsers_per_os() {
    let running = |names: &[&str], platform| {
        let names: Vec<String> = names.iter().map(|n| n.to_string()).collect();
        roots::running_browsers(&names, platform)
    };
    assert_eq!(
        running(&["Google Chrome Canary"], Platform::Macos),
        [Browser::Chrome]
    );
    assert_eq!(running(&["MSEDGE.EXE"], Platform::Windows), [Browser::Edge]);
    assert_eq!(running(&["firefox"], Platform::Linux), [Browser::Firefox]);
    assert_eq!(
        running(&["chromium-browse"], Platform::Linux),
        [Browser::Chromium]
    );
    // A helper or an unrelated process is not the browser.
    assert!(running(&["Google Chrome Helper (Renderer)"], Platform::Macos).is_empty());
    assert!(running(&["chrome"], Platform::Windows).is_empty());
    assert!(running(&["bash", "Finder"], Platform::Macos).is_empty());
}

// Test 9: the Snap Firefox cache is found.
#[test]
fn the_snap_firefox_cache_is_found() {
    let home = basic_home();
    let found = roots::cache_roots(
        &home.old_context(),
        CatalogId::BrowserHttpCache,
        Platform::Linux,
    );
    let snap = home.path("snap/firefox/common/.cache/mozilla/firefox/abcd1234.default/cache2");
    let root = found
        .iter()
        .find(|r| r.root.as_ref().is_ok_and(|v| v.path() == snap))
        .expect("snap Firefox cache");
    assert_eq!(root.browser, Some(Browser::Firefox));
    assert_eq!(root.display_name, "Firefox (Snap) — abcd1234.default");
}

// Acceptance 3: a missing browser is a quiet absence, not an error.
#[test]
fn a_home_without_browsers_has_no_cache_rows_and_no_errors() {
    let dir = tempfile::tempdir().unwrap();
    let home = fs::canonicalize(dir.path()).unwrap();
    let ctx = Context::for_fixture(&home);
    let result = scan(&ctx);
    assert!(result.caches.is_empty());
    assert!(result.cache_skipped.is_empty());
}

#[test]
fn thumbnails_and_fonts_list_only_their_cache_folders() {
    let home = basic_home();
    let ctx = home.old_context();
    assert_eq!(
        root_paths(&ctx, CatalogId::ThumbnailCache, Platform::Linux),
        [home.path(".cache/thumbnails")]
    );
    // Never `~/.local/share/fonts`, where the fonts themselves live.
    assert_eq!(
        root_paths(&ctx, CatalogId::FontCache, Platform::Linux),
        [home.path(".cache/fontconfig")]
    );
    assert_eq!(
        root_paths(&ctx, CatalogId::WindowsThumbcache, Platform::Windows),
        [home.path("AppData/Local/Microsoft/Windows/Explorer")]
    );
}

#[cfg(target_os = "linux")]
#[test]
fn thumbnails_and_fonts_are_bulk_safe_candidates_on_ubuntu() {
    let home = basic_home();
    let thumbnails = at(&home, &home.path(".cache/thumbnails")).unwrap();
    let fonts = at(&home, &home.path(".cache/fontconfig")).unwrap();
    for c in [&thumbnails, &fonts] {
        assert!(c.bulk_safe);
        assert_eq!(c.action, Action::Delete);
        assert_eq!(c.file_count, 1);
    }
    assert_eq!(thumbnails.display_name, "Thumbnails");
    assert_eq!(fonts.display_name, "Font cache");
}

#[cfg(windows)]
#[test]
fn explorer_thumbnails_are_shown_for_information_only() {
    let home = basic_home();
    let explorer = at(
        &home,
        &home.path("AppData/Local/Microsoft/Windows/Explorer"),
    )
    .unwrap();
    assert_eq!(explorer.catalog_id, CatalogId::WindowsThumbcache);
    assert_eq!(explorer.action, Action::Report);
    assert!(!explorer.bulk_safe);
}

// A cache folder that is a link is recorded and not followed.
#[cfg(unix)]
#[test]
fn a_cache_folder_that_is_a_link_is_not_followed() {
    let home = basic_home();
    let cache = chrome_cache(&home);
    fs::rename(&cache, cache.with_file_name("Cache-moved")).unwrap();
    std::os::unix::fs::symlink(home.path("Documents"), &cache).unwrap();

    let result = scan(&home.old_context());

    assert!(result.caches.iter().all(|c| c.path() != cache));
    let skip = result
        .cache_skipped
        .iter()
        .find(|s| s.display_name == common::CHROME_CACHE)
        .expect("the link is reported");
    assert_eq!(skip.reason, discover::SkipReason::Link);
    assert!(home.path("Documents/notes.txt").is_file());
}

// A cache whose real location is outside the home is not offered.
#[cfg(unix)]
#[test]
fn a_cache_that_resolves_outside_the_home_is_rejected() {
    let home = basic_home();
    let outside = tempfile::tempdir().unwrap();
    let elsewhere = outside.path().join("mozilla/firefox/p.default/cache2");
    fs::create_dir_all(&elsewhere).unwrap();
    fs::write(elsewhere.join("entry"), b"x").unwrap();
    let snap_cache = home.path("snap/firefox/common/.cache");
    fs::rename(&snap_cache, snap_cache.with_file_name("cache-moved")).unwrap();
    std::os::unix::fs::symlink(outside.path(), &snap_cache).unwrap();

    let found = roots::cache_roots(
        &home.old_context(),
        CatalogId::BrowserHttpCache,
        Platform::Linux,
    );
    let snap = found
        .iter()
        .find(|r| r.display_name == "Firefox (Snap) — p.default")
        .expect("the redirected cache is reported");
    assert_eq!(
        snap.root.as_ref().unwrap_err(),
        &discover::SkipReason::OutsideHome
    );
    assert!(elsewhere.join("entry").is_file());
}

#[test]
fn cache_candidates_never_serialize_a_path() {
    let home = basic_home();
    let result = scan(&home.old_context());
    let json = serde_json::to_string(&result.caches).unwrap();
    assert!(!json.contains(&*home.root.to_string_lossy()), "{json}");
}
