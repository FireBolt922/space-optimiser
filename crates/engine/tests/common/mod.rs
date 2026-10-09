//! Fixture homes for tests. Every test copies `fixtures/homes/basic` into its
//! own temp directory and changes only that copy (phase 4 test 8).

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use space_optimiser_engine::policy::{Context, OwnerRule};

pub struct Home {
    _dir: tempfile::TempDir,
    /// Canonical path of the copied home.
    pub root: PathBuf,
}

impl Home {
    pub fn path(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }

    pub fn write(&self, relative: &str, bytes: &[u8]) -> PathBuf {
        let path = self.path(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
        path
    }

    pub fn mkdir(&self, relative: &str) -> PathBuf {
        let path = self.path(relative);
        fs::create_dir_all(&path).unwrap();
        path
    }

    /// A context whose clock is a week ahead, so files created by the test
    /// count as old (their `ctime` cannot be set backwards).
    pub fn old_context(&self) -> Context {
        Context::for_fixture(&self.root)
            .with_now(a_week_ahead())
            .with_temp_root(self.path("tmp"), OwnerRule::Root)
    }
}

pub fn basic_home() -> Home {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("home");
    copy_tree(&fixture_source(), &root);
    let root = fs::canonicalize(&root).unwrap();
    Home { _dir: dir, root }
}

pub fn fixture_source() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/homes/basic")
}

/// `base` joined with `relative` (which may hold `..`) in a form the OS
/// resolves. Windows treats `..` literally after the `\\?\` prefix that
/// canonical paths carry, so the prefix is dropped there.
pub fn spelled(base: &Path, relative: &str) -> PathBuf {
    if cfg!(windows) {
        let base = base.to_str().unwrap().trim_start_matches(r"\\?\");
        PathBuf::from(format!("{base}\\{}", relative.replace('/', "\\")))
    } else {
        base.join(relative)
    }
}

pub fn a_week_ahead() -> SystemTime {
    SystemTime::now() + Duration::from_secs(7 * 24 * 60 * 60)
}

pub fn year_2019() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_550_000_000)
}

pub fn set_mtime(path: &Path, time: SystemTime) {
    fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(time)
        .unwrap();
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).unwrap();
        }
    }
}
