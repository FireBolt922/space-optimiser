//! Phase 10 `large` fixture and phase 11 acceptance 2: a 1,000,000-file
//! tree generated at test time (never checked in), and cancel meets the
//! phase 2 targets on it: Normal Scan stops within 1 second, Deep Scan
//! within 2. Ignored by default; CI runs it on Linux with `--ignored`.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use common::Home;
use space_optimiser_engine::Engine;
use space_optimiser_engine::model::{ScanMode, ScanRef, ScanStatus, StartScanArgs};

const DIRS: usize = 1000;
const FILES_PER_DIR: usize = 1000;
/// Cancel once the scan has seen this many files, so it is well inside the
/// big tree.
const CANCEL_AFTER: u64 = 10_000;

/// `DIRS` folders of `FILES_PER_DIR` empty files under `root`, written by
/// several threads.
fn big_tree(root: &Path) {
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(2, 16);
    std::thread::scope(|scope| {
        for t in 0..threads {
            scope.spawn(move || {
                for d in (t..DIRS).step_by(threads) {
                    let dir = root.join(format!("d{d:04}"));
                    fs::create_dir_all(&dir).unwrap();
                    for f in 0..FILES_PER_DIR {
                        fs::File::create(dir.join(format!("f{f:04}"))).unwrap();
                    }
                }
            });
        }
    });
}

/// Starts a scan, cancels it once it is inside the big tree, and returns
/// how long the engine took to stop after the cancel.
fn time_cancel(engine: &Engine, mode: ScanMode) -> Duration {
    let scan_id = engine.start_scan(StartScanArgs { mode }).unwrap();
    let scan = ScanRef { scan_id };
    let started = Instant::now();
    loop {
        let view = engine.get_scan(scan).unwrap();
        assert_eq!(
            view.status,
            ScanStatus::Running,
            "the scan stopped before it reached {CANCEL_AFTER} files ({} seen)",
            view.progress.files_seen
        );
        if view.progress.files_seen > CANCEL_AFTER {
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(600),
            "the scan never reached the big tree"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    let cancelled = Instant::now();
    engine.cancel_scan(scan).unwrap();
    loop {
        let view = engine.get_scan(scan).unwrap();
        if view.status != ScanStatus::Running {
            let took = cancelled.elapsed();
            assert_eq!(view.status, ScanStatus::Cancelled, "{:?}", view.error);
            eprintln!(
                "{mode:?} Scan: cancelled after {} files in {took:?}",
                view.progress.files_seen
            );
            return took;
        }
        assert!(
            cancelled.elapsed() < Duration::from_secs(60),
            "the scan ignored cancel"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn home_with_big_tree(relative: &str) -> (Home, PathBuf) {
    let home = common::basic_home();
    let root = home.mkdir(relative);
    let made = Instant::now();
    big_tree(&root);
    eprintln!(
        "created {} files under {relative} in {:?}",
        DIRS * FILES_PER_DIR,
        made.elapsed()
    );
    (home, root)
}

#[test]
#[ignore = "creates 1,000,000 files; CI runs it on Linux"]
fn normal_scan_cancels_within_one_second_on_a_million_temp_files() {
    let (home, _) = home_with_big_tree("tmp/big");
    let engine = home.engine();
    let took = time_cancel(&engine, ScanMode::Normal);
    assert!(took < Duration::from_secs(1), "Normal Scan took {took:?}");
}

#[test]
#[ignore = "creates 1,000,000 files; CI runs it on Linux"]
fn deep_scan_cancels_within_two_seconds_on_a_million_home_files() {
    let (home, _) = home_with_big_tree("projects/big");
    let engine = home.engine();
    let took = time_cancel(&engine, ScanMode::Deep);
    assert!(took < Duration::from_secs(2), "Deep Scan took {took:?}");
}
