//! The Android tool: `android-avd-snapshots` (the Quick Boot snapshot),
//! `android-unreferenced-images` (system images no emulator's `config.ini`
//! names), and `android-emulators-report` (information). Emulator user data
//! is never a candidate. Images are matched to emulators by parsing each
//! emulator's own settings, never by the order of any listing: the cleanup
//! session that led to this row deleted the image of the emulator it kept.

mod common;

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use common::{Home, basic_home, report_item};
use space_optimiser_engine::discover::{self, CancelToken, Scan, Stage};
use space_optimiser_engine::model::{
    Action, BuildPlanArgs, Candidate, Confidence, Label, Outcome, Platform, Risk, ScanMode,
    SkipCause,
};
use space_optimiser_engine::policy::{CatalogId, Context};
use space_optimiser_engine::roots::{self, App};
use space_optimiser_engine::{Engine, EngineError};

const IMAGES: CatalogId = CatalogId::AndroidUnreferencedImages;
const SNAPSHOTS: CatalogId = CatalogId::AndroidAvdSnapshots;
const EMULATORS: CatalogId = CatalogId::AndroidEmulatorsReport;

const IMAGE_35: &str = "android-35 — google_apis — arm64-v8a";
const IMAGE_36: &str = "android-36.1 — google_apis_playstore — arm64-v8a";
const IMAGE_30: &str = "android-30 — default — x86_64";

/// The SDK folder in this OS's layout, relative to the home.
fn sdk() -> &'static str {
    if cfg!(target_os = "macos") {
        "Library/Android/sdk"
    } else if cfg!(windows) {
        "AppData/Local/Android/Sdk"
    } else {
        "Android/Sdk"
    }
}

/// A system image, `key` as `api/tag/abi`.
fn image(home: &Home, key: &str) -> PathBuf {
    let dir = format!("{}/system-images/{key}", sdk());
    home.write(&format!("{dir}/system.img"), &[9u8; 8192]);
    home.write(&format!("{dir}/source.properties"), b"Pkg.Revision=1\n");
    home.path(&dir)
}

/// An emulator whose settings name `sysdir`, with user data, a Quick Boot
/// snapshot, and a snapshot saved by hand.
fn avd(home: &Home, id: &str, display: &str, sysdir: Option<&str>) {
    let dir = format!(".android/avd/{id}.avd");
    home.write(
        &format!(".android/avd/{id}.ini"),
        format!("avd.ini.encoding=UTF-8\npath.rel=avd/{id}.avd\ntarget=android-35\n").as_bytes(),
    );
    let mut config = format!("AvdId={id}\navd.ini.displayname = {display}\nhw.ramSize=2048\n");
    if let Some(sysdir) = sysdir {
        config.push_str(&format!("image.sysdir.1={sysdir}\n"));
    }
    home.write(&format!("{dir}/config.ini"), config.as_bytes());
    home.write(&format!("{dir}/userdata-qemu.img.qcow2"), &[1u8; 8192]);
    home.write(
        &format!("{dir}/snapshots/default_boot/ram.bin"),
        &[2u8; 8192],
    );
    home.write(&format!("{dir}/snapshots/default_boot/snapshot.pb"), b"pb");
    home.write(
        &format!("{dir}/snapshots/before-login/ram.bin"),
        &[3u8; 8192],
    );
}

fn scan(ctx: &Context) -> Scan {
    discover::scan(ctx, ScanMode::Android, &CancelToken::new(), |_| {}).unwrap()
}

fn rows(scan: &Scan, row: CatalogId) -> Vec<&Candidate> {
    scan.caches.iter().filter(|c| c.catalog_id == row).collect()
}

fn row<'a>(scan: &'a Scan, name: &str) -> &'a Candidate {
    scan.caches
        .iter()
        .find(|c| c.display_name == name)
        .unwrap_or_else(|| {
            let names: Vec<_> = scan.caches.iter().map(|c| &c.display_name).collect();
            panic!("no row {name}; found {names:?}")
        })
}

/// The emulator's main process on this OS.
fn emulator_process() -> &'static str {
    if cfg!(target_os = "macos") {
        "qemu-system-aarch64"
    } else if cfg!(windows) {
        "qemu-system-x86_64.exe"
    } else {
        "qemu-system-x86"
    }
}

#[test]
fn an_image_in_use_is_shown_and_an_unused_one_is_offered() {
    let home = basic_home();
    image(&home, "android-35/google_apis/arm64-v8a");
    image(&home, "android-30/default/x86_64");
    avd(
        &home,
        "pixel",
        "Pixel 9",
        Some("system-images/android-35/google_apis/arm64-v8a/"),
    );

    let scan = scan(&home.old_context());

    let used = row(&scan, IMAGE_35);
    assert_eq!(used.catalog_id, IMAGES);
    assert_eq!(used.action, Action::Report);
    assert_eq!(used.risk, Risk::Info);
    assert_eq!(used.detail.as_deref(), Some("In use by Pixel 9."));

    let unused = row(&scan, IMAGE_30);
    assert_eq!(unused.catalog_id, IMAGES);
    assert_eq!(unused.action, Action::Delete);
    assert_eq!(unused.risk, Risk::High);
    assert_eq!(unused.confidence, Confidence::High);
    assert!(!unused.bulk_safe);
    assert!(unused.estimated_bytes >= 8192);
    assert_eq!(
        unused.detail.as_deref(),
        Some("No emulator uses this image.")
    );
}

// The session's mistake as a test: two emulators, each on its own image, and
// a third image nobody uses. Only the third may go.
#[test]
fn each_emulator_is_matched_to_its_own_image() {
    let home = basic_home();
    let kept_a = image(&home, "android-35/google_apis/arm64-v8a");
    let kept_b = image(&home, "android-36.1/google_apis_playstore/arm64-v8a");
    let unused = image(&home, "android-30/default/x86_64");
    // Listed in the opposite order to their images, with Windows separators
    // in one of them.
    avd(
        &home,
        "a_tablet",
        "Tablet",
        Some("system-images/android-36.1/google_apis_playstore/arm64-v8a/"),
    );
    avd(
        &home,
        "z_phone",
        "Phone",
        Some("system-images\\android-35\\google_apis\\arm64-v8a\\"),
    );
    let engine = home.engine();
    let view = common::scan_in(&engine, ScanMode::Android);

    assert_eq!(
        common::named(&view, IMAGE_35).detail.as_deref(),
        Some("In use by Phone.")
    );
    assert_eq!(
        common::named(&view, IMAGE_36).detail.as_deref(),
        Some("In use by Tablet.")
    );
    for name in [IMAGE_35, IMAGE_36] {
        let result = engine.build_plan(BuildPlanArgs {
            scan_id: view.scan_id,
            candidate_ids: vec![common::named(&view, name).id],
        });
        assert_eq!(result.unwrap_err(), EngineError::NotActionable, "{name}");
    }

    let plan = common::plan(&engine, &view, &[IMAGE_30]);
    let token = common::confirm(&engine, &plan);
    let report = common::execute(&engine, &plan, token);

    let done = report_item(&report, IMAGE_30);
    assert_eq!(done.outcome, Outcome::Removed, "{done:?}");
    assert!(done.bytes_removed >= 8192);
    assert!(!unused.exists());
    assert!(kept_a.join("system.img").is_file());
    assert!(kept_b.join("system.img").is_file());
}

#[test]
fn two_emulators_on_one_image_are_both_named() {
    let home = basic_home();
    image(&home, "android-35/google_apis/arm64-v8a");
    let sysdir = Some("system-images/android-35/google_apis/arm64-v8a/");
    avd(&home, "one", "One", sysdir);
    avd(&home, "two", "Two", sysdir);

    let scan = scan(&home.old_context());

    assert_eq!(
        row(&scan, IMAGE_35).detail.as_deref(),
        Some("In use by One and Two.")
    );
}

/// Every image row is "not sure" and cannot be planned.
fn assert_no_image_is_offered(home: &Home) {
    let engine = home.engine();
    let view = common::scan_in(&engine, ScanMode::Android);
    let images: Vec<_> = view
        .candidates
        .iter()
        .filter(|c| c.catalog_id == IMAGES)
        .collect();
    assert!(!images.is_empty());
    for c in images {
        assert_eq!(c.action, Action::None, "{}", c.display_name);
        assert_eq!(c.confidence, Confidence::Uncertain);
        let result = engine.build_plan(BuildPlanArgs {
            scan_id: view.scan_id,
            candidate_ids: vec![c.id],
        });
        assert_eq!(result.unwrap_err(), EngineError::NotActionable);
    }
    assert!(
        view.notes
            .iter()
            .any(|n| n.catalog_id == IMAGES && n.text.contains("could not read")),
        "{:?}",
        view.notes
    );
}

#[test]
fn an_emulator_without_an_image_setting_blocks_every_image_offer() {
    let home = basic_home();
    image(&home, "android-30/default/x86_64");
    avd(&home, "broken", "Broken", None);
    assert_no_image_is_offered(&home);
}

#[test]
fn an_emulator_whose_folder_is_missing_blocks_every_image_offer() {
    let home = basic_home();
    image(&home, "android-30/default/x86_64");
    avd(
        &home,
        "pixel",
        "Pixel",
        Some("system-images/android-35/google_apis/arm64-v8a/"),
    );
    // Its `.avd` folder lives somewhere this scan does not look.
    home.write(
        ".android/avd/elsewhere.ini",
        b"path.rel=avd/elsewhere.avd\n",
    );
    assert_no_image_is_offered(&home);
}

// Gradle Managed Devices keep their emulators one folder down, in
// `avd/gradle-managed`. A folder there that is not an emulator may hold
// emulators this scan does not read, so nothing can be called unused.
#[test]
fn a_folder_of_emulators_inside_the_emulator_folder_blocks_every_image_offer() {
    let home = basic_home();
    image(&home, "android-30/default/x86_64");
    home.write(
        ".android/avd/gradle-managed/dev30.avd/config.ini",
        b"image.sysdir.1=system-images/android-30/default/x86_64/\n",
    );
    assert_no_image_is_offered(&home);
}

// `~/.android` linked to a disk that is not plugged in looks like "no
// emulator folder". It must read as "unknown", never as "no emulators".
#[cfg(unix)]
#[test]
fn a_dangling_link_at_the_android_folder_blocks_every_image_offer() {
    let home = basic_home();
    image(&home, "android-30/default/x86_64");
    std::os::unix::fs::symlink(home.path("unplugged-disk/android"), home.path(".android")).unwrap();
    assert_no_image_is_offered(&home);
}

#[test]
fn an_image_setting_that_is_not_an_sdk_path_blocks_every_image_offer() {
    for (id, sysdir) in [
        (
            "absolute",
            "/opt/android/system-images/android-30/default/x86_64/",
        ),
        ("dots", "system-images/../system-images/android-30/default"),
        ("short", "system-images/android-30"),
    ] {
        let home = basic_home();
        image(&home, "android-30/default/x86_64");
        avd(&home, id, id, Some(sysdir));
        assert_no_image_is_offered(&home);
    }
}

#[test]
fn images_are_offered_when_there_is_no_emulator_at_all() {
    let home = basic_home();
    image(&home, "android-30/default/x86_64");

    let scan = scan(&home.old_context());

    assert_eq!(row(&scan, IMAGE_30).action, Action::Delete);
    assert!(rows(&scan, EMULATORS).is_empty());
}

#[test]
fn a_folder_without_a_system_image_file_is_not_an_image() {
    let home = basic_home();
    home.write(
        &format!(
            "{}/system-images/android-30/default/x86_64/notes.txt",
            sdk()
        ),
        b"mine",
    );
    home.write(
        &format!("{}/platforms/android-35/android.jar", sdk()),
        b"jar",
    );
    home.write(&format!("{}/ndk/27.1/source.properties", sdk()), b"ndk");

    let scan = scan(&home.old_context());

    assert!(rows(&scan, IMAGES).is_empty());
}

#[cfg(unix)]
#[test]
fn a_link_in_place_of_an_image_folder_is_left_alone() {
    let home = basic_home();
    image(&home, "android-35/google_apis/arm64-v8a");
    let tag = home.path(&format!("{}/system-images/android-30/default", sdk()));
    fs::create_dir_all(&tag).unwrap();
    home.write("Documents/system.img", b"mine");
    std::os::unix::fs::symlink(home.path("Documents"), tag.join("x86_64")).unwrap();

    let scan = scan(&home.old_context());

    let names: Vec<_> = rows(&scan, IMAGES)
        .iter()
        .map(|c| c.display_name.clone())
        .collect();
    assert_eq!(names, [IMAGE_35]);
    assert!(home.path("Documents/system.img").is_file());
}

/// Whether `path` is below `link` (the link itself does not count) or
/// inside the folder the link points into.
#[cfg(unix)]
fn went_through(path: &std::path::Path, link: &std::path::Path, target: &std::path::Path) -> bool {
    path.starts_with(target) || (path.starts_with(link) && path != link)
}

// Emulators kept somewhere else through a link are not read, so which
// images they use is unknown, and unknown is never "unused".
#[cfg(unix)]
#[test]
fn an_emulator_folder_that_is_a_link_is_not_followed_and_blocks_image_offers() {
    let home = basic_home();
    image(&home, "android-30/default/x86_64");
    home.write(
        "Documents/avd/kept.avd/config.ini",
        b"image.sysdir.1=system-images/android-30/default/x86_64/\n",
    );
    home.write(
        "Documents/avd/kept.avd/snapshots/default_boot/ram.bin",
        &[2u8; 8192],
    );
    home.mkdir(".android");
    std::os::unix::fs::symlink(home.path("Documents/avd"), home.path(".android/avd")).unwrap();
    let visits: Arc<Mutex<Vec<PathBuf>>> = Arc::default();

    let scan = scan(&home.old_context().with_visit_log(visits.clone()));

    let image = row(&scan, IMAGE_30);
    assert_eq!(image.action, Action::None);
    assert_eq!(image.confidence, Confidence::Uncertain);
    assert!(rows(&scan, SNAPSHOTS).is_empty());
    assert!(rows(&scan, EMULATORS).is_empty());
    // Nothing was read through the link: no path below it, by either name.
    let link = home.path(".android/avd");
    for path in visits.lock().unwrap().iter() {
        assert!(
            !went_through(path, &link, &home.path("Documents")),
            "the scan read {}",
            path.display()
        );
    }
    assert!(
        home.path("Documents/avd/kept.avd/snapshots/default_boot/ram.bin")
            .is_file()
    );
}

#[cfg(unix)]
#[test]
fn an_sdk_image_folder_that_is_a_link_is_not_followed() {
    let home = basic_home();
    home.write(
        "Documents/images/android-30/default/x86_64/system.img",
        b"mine",
    );
    let sdk_dir = home.mkdir(sdk());
    std::os::unix::fs::symlink(home.path("Documents/images"), sdk_dir.join("system-images"))
        .unwrap();
    let visits: Arc<Mutex<Vec<PathBuf>>> = Arc::default();

    let scan = scan(&home.old_context().with_visit_log(visits.clone()));

    assert!(rows(&scan, IMAGES).is_empty());
    assert!(scan.cache_skipped.is_empty(), "{:?}", scan.cache_skipped);
    // Nothing was read through the link: no path below it, by either name.
    let link = sdk_dir.join("system-images");
    for path in visits.lock().unwrap().iter() {
        assert!(
            !went_through(path, &link, &home.path("Documents")),
            "the scan read {}",
            path.display()
        );
    }
    assert!(
        home.path("Documents/images/android-30/default/x86_64/system.img")
            .is_file()
    );
}

#[test]
fn each_emulator_is_listed_for_information_with_the_image_it_uses() {
    let home = basic_home();
    image(&home, "android-35/google_apis/arm64-v8a");
    avd(
        &home,
        "pixel",
        "Pixel 9",
        Some("system-images/android-35/google_apis/arm64-v8a/"),
    );
    let engine = home.engine();
    let view = common::scan_in(&engine, ScanMode::Android);

    let emulator = common::named(&view, "Pixel 9");
    assert_eq!(emulator.catalog_id, EMULATORS);
    assert_eq!(emulator.action, Action::Report);
    assert_eq!(emulator.risk, Risk::Info);
    assert!(emulator.estimated_bytes >= 3 * 8192);
    let detail = emulator.detail.as_deref().unwrap();
    assert!(detail.contains(IMAGE_35), "{detail}");
    assert!(detail.contains("never removed"), "{detail}");
    let result = engine.build_plan(BuildPlanArgs {
        scan_id: view.scan_id,
        candidate_ids: vec![emulator.id],
    });
    assert_eq!(result.unwrap_err(), EngineError::NotActionable);
}

// --- Quick Boot snapshots ------------------------------------------------

const SNAPSHOT: &str = "Pixel 9 — Quick Boot snapshot";

fn pixel(home: &Home) {
    image(home, "android-35/google_apis/arm64-v8a");
    avd(
        home,
        "pixel",
        "Pixel 9",
        Some("system-images/android-35/google_apis/arm64-v8a/"),
    );
}

#[test]
fn only_the_quick_boot_snapshot_is_offered() {
    let home = basic_home();
    pixel(&home);

    let scan = scan(&home.old_context());

    let snapshots = rows(&scan, SNAPSHOTS);
    assert_eq!(snapshots.len(), 1);
    let c = snapshots[0];
    assert_eq!(c.display_name, SNAPSHOT);
    assert_eq!(c.action, Action::Delete);
    assert_eq!(c.risk, Risk::Medium);
    assert!(!c.bulk_safe);
    assert_eq!(c.quit_app, Some("Android Emulator"));
    assert_eq!(c.file_count, 2);
    assert_eq!(
        c.path(),
        home.path(".android/avd/pixel.avd/snapshots/default_boot")
    );
    // Nothing else in the emulator's folder is a delete candidate.
    let avd = home.path(".android/avd/pixel.avd");
    for other in &scan.caches {
        if other.action == Action::Delete && other.path().starts_with(&avd) {
            assert_eq!(other.display_name, SNAPSHOT);
        }
    }
}

#[test]
fn cleaning_the_snapshot_keeps_user_data_and_saved_snapshots() {
    let home = basic_home();
    pixel(&home);

    let report = common::clean_in(&home.engine(), ScanMode::Android, &[SNAPSHOT]);

    let done = report_item(&report, SNAPSHOT);
    assert_eq!(done.outcome, Outcome::Removed, "{done:?}");
    assert!(done.bytes_removed >= 8192);
    assert_eq!(
        done.recovery_text,
        Some("That emulator does a full boot the next time it starts.")
    );
    let avd = home.path(".android/avd/pixel.avd");
    let boot = avd.join("snapshots/default_boot");
    assert!(boot.is_dir(), "the snapshot folder itself stays");
    assert_eq!(fs::read_dir(&boot).unwrap().count(), 0);
    for kept in [
        "userdata-qemu.img.qcow2",
        "config.ini",
        "snapshots/before-login/ram.bin",
    ] {
        assert!(avd.join(kept).is_file(), "{kept} was removed");
    }
    assert!(home.path(".android/avd/pixel.ini").is_file());
}

// The row is exactly `<name>.avd/snapshots/default_boot`. A `snapshots`
// link would make that name mean some other folder.
#[cfg(unix)]
#[test]
fn a_snapshots_folder_that_is_a_link_is_left_alone() {
    let home = basic_home();
    image(&home, "android-35/google_apis/arm64-v8a");
    home.write(
        ".android/avd/pixel.avd/config.ini",
        b"avd.ini.displayname=Pixel 9\nimage.sysdir.1=system-images/android-35/google_apis/arm64-v8a/\n",
    );
    home.write("elsewhere/default_boot/keep.bin", &[5u8; 8192]);
    std::os::unix::fs::symlink(
        home.path("elsewhere"),
        home.path(".android/avd/pixel.avd/snapshots"),
    )
    .unwrap();

    let scan = scan(&home.old_context());

    assert!(rows(&scan, SNAPSHOTS).is_empty());
    assert!(
        scan.cache_skipped.iter().any(|s| s.display_name == SNAPSHOT
            && s.reason == space_optimiser_engine::policy::SkipReason::Link),
        "{:?}",
        scan.cache_skipped
    );
    assert!(home.path("elsewhere/default_boot/keep.bin").is_file());
}

#[test]
fn a_running_emulator_blocks_the_snapshot_row() {
    let home = basic_home();
    pixel(&home);
    let ctx = home
        .old_context()
        .with_running_processes(&[emulator_process()]);

    let scan = scan(&ctx);

    let c = row(&scan, SNAPSHOT);
    assert_eq!(c.action, Action::None);
    assert!(c.labels.contains(&Label::QuitFirst));
    let blind = self::scan(&home.old_context().with_process_list_error());
    assert_eq!(row(&blind, SNAPSHOT).action, Action::None);
}

#[test]
fn an_emulator_started_after_the_scan_keeps_its_snapshot() {
    let home = basic_home();
    pixel(&home);
    let running: Arc<Mutex<Vec<String>>> = Arc::default();
    let shared = running.clone();
    let engine = Engine::new(
        home.config_with(move |ctx| ctx.with_running_processes(&shared.lock().unwrap())),
    );
    let view = common::scan_in(&engine, ScanMode::Android);
    let plan = common::plan(&engine, &view, &[SNAPSHOT]);
    let token = common::confirm(&engine, &plan);
    running.lock().unwrap().push(emulator_process().to_owned());

    let report = common::execute(&engine, &plan, token);

    let item = report_item(&report, SNAPSHOT);
    assert_eq!(item.outcome, Outcome::Skipped, "{item:?}");
    assert_eq!(item.reason, Some(SkipCause::AppRunning));
    assert!(
        home.path(".android/avd/pixel.avd/snapshots/default_boot/ram.bin")
            .is_file()
    );
}

#[test]
fn process_names_map_to_the_emulator_per_os() {
    let running = |names: &[&str], platform| {
        let names: Vec<String> = names.iter().map(|n| n.to_string()).collect();
        roots::running_apps(&names, platform)
    };
    let emulator = [App::AndroidEmulator];
    assert_eq!(running(&["qemu-system-aarch64"], Platform::Macos), emulator);
    assert_eq!(
        running(&["qemu-system-x86_64-headless"], Platform::Macos),
        emulator
    );
    // Linux `comm` is cut to 15 bytes.
    assert_eq!(running(&["qemu-system-x86"], Platform::Linux), emulator);
    assert_eq!(running(&["qemu-system-aar"], Platform::Linux), emulator);
    assert_eq!(
        running(&["QEMU-SYSTEM-X86_64.EXE"], Platform::Windows),
        emulator
    );
    assert_eq!(running(&["emulator.exe"], Platform::Windows), emulator);
    assert_eq!(running(&["qemu-system-armel"], Platform::Macos), emulator);
    assert_eq!(
        running(&["qemu-system-i386-headless.exe"], Platform::Windows),
        emulator
    );
    assert!(running(&["adb"], Platform::Macos).is_empty());
    assert!(running(&["studio"], Platform::Macos).is_empty());
}

// --- Re-checks before an image is removed --------------------------------

/// Scans, plans the unused image, confirms, lets `change` alter the home,
/// then runs. Returns why the image was skipped.
fn remove_image_after(change: impl FnOnce(&Home)) -> (Home, SkipCause) {
    let home = basic_home();
    pixel(&home);
    image(&home, "android-30/default/x86_64");
    let engine = home.engine();
    let view = common::scan_in(&engine, ScanMode::Android);
    let plan = common::plan(&engine, &view, &[IMAGE_30]);
    let token = common::confirm(&engine, &plan);
    change(&home);
    let report = common::execute(&engine, &plan, token);
    let item = report_item(&report, IMAGE_30);
    assert_eq!(item.outcome, Outcome::Skipped, "{item:?}");
    let cause = item.reason.unwrap();
    let kept = home.path(&format!(
        "{}/system-images/android-30/default/x86_64/system.img",
        sdk()
    ));
    assert!(kept.is_file(), "the image was removed");
    (home, cause)
}

#[test]
fn an_image_an_emulator_started_using_after_the_scan_is_skipped() {
    let (_home, cause) = remove_image_after(|home| {
        avd(
            home,
            "new",
            "New",
            Some("system-images/android-30/default/x86_64/"),
        );
    });
    assert_eq!(cause, SkipCause::ImageInUse);
}

#[test]
fn an_image_is_skipped_when_emulators_can_no_longer_be_read() {
    let (_home, cause) = remove_image_after(|home| {
        home.write(".android/avd/pixel.avd/config.ini", b"hw.ramSize=2048\n");
    });
    assert_eq!(cause, SkipCause::EmulatorsUnknown);
}

// --- The tool as a whole -------------------------------------------------

#[test]
fn a_computer_without_android_tools_gets_one_note_and_no_rows() {
    let home = basic_home();

    let scan = scan(&home.old_context());

    assert!(scan.caches.is_empty());
    assert_eq!(scan.notes.len(), 1);
    assert_eq!(scan.notes[0].catalog_id, EMULATORS);
    assert_eq!(
        scan.notes[0].text,
        "No Android emulators or system images were found."
    );
}

#[test]
fn the_android_tool_runs_one_stage_and_never_reads_documents_or_temp() {
    let home = basic_home();
    pixel(&home);
    let visits: Arc<Mutex<Vec<PathBuf>>> = Arc::default();
    let mut stages = Vec::new();
    let scan = discover::scan(
        &home.old_context().with_visit_log(visits.clone()),
        ScanMode::Android,
        &CancelToken::new(),
        |p| stages.push(p.stage),
    )
    .unwrap();

    assert_eq!(discover::stages(ScanMode::Android), [Stage::Android]);
    assert!(stages.iter().all(|s| *s == Stage::Android), "{stages:?}");
    assert!(scan.temp.is_empty());
    for path in visits.lock().unwrap().iter() {
        for outside in ["Documents", "Desktop", "Downloads", "tmp"] {
            assert!(!path.starts_with(home.path(outside)), "{}", path.display());
        }
    }
}
