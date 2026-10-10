//! Fixture homes for tests. Every test copies `fixtures/homes/basic` into its
//! own temp directory and changes only that copy (phase 4 test 8).

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use space_optimiser_engine::clock::ManualClock;
use space_optimiser_engine::model::{
    BuildPlanArgs, Candidate, ConfirmationToken, ExecutePlanArgs, ExecuteStart, Plan, PlanRef,
    Report, ScanMode, ScanRef, ScanStatus, ScanView, StartScanArgs,
};
use space_optimiser_engine::policy::{Context, OwnerRule};
use space_optimiser_engine::{Config, Engine};

pub struct Home {
    _dir: tempfile::TempDir,
    /// Canonical path of the copied home.
    pub root: PathBuf,
}

impl Home {
    /// `relative` uses `/`. It is joined one component at a time, because
    /// Windows reads `/` literally after the `\\?\` prefix of a canonical path.
    pub fn path(&self, relative: &str) -> PathBuf {
        relative
            .split('/')
            .fold(self.root.clone(), |p, c| p.join(c))
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

impl Home {
    /// Outside the fixture home, inside the test's temp directory.
    pub fn audit_dir(&self) -> PathBuf {
        self.root.parent().unwrap().join("audit")
    }

    /// An engine over this home whose context clock reads `now`.
    pub fn config_at(&self, now: Arc<Mutex<SystemTime>>) -> Config {
        self.config_with(move |ctx| ctx.with_now(*now.lock().unwrap()))
    }

    /// An engine over this home, files old, with `change` applied to every
    /// context the engine resolves.
    pub fn config_with(
        &self,
        change: impl Fn(Context) -> Context + Send + Sync + 'static,
    ) -> Config {
        let root = self.root.clone();
        Config::new(
            move || {
                Ok(change(
                    Context::for_fixture(&root)
                        .with_now(a_week_ahead())
                        .with_temp_root(root.join("tmp"), OwnerRule::Root),
                ))
            },
            self.audit_dir(),
        )
        .with_settle(Duration::ZERO)
    }

    /// This OS's Chrome `Default` HTTP cache in the fixture layout.
    pub fn chrome_cache(&self) -> PathBuf {
        if cfg!(target_os = "macos") {
            self.path("Library/Caches/Google/Chrome/Default/Cache")
        } else if cfg!(windows) {
            self.path("AppData/Local/Google/Chrome/User Data/Default/Cache")
        } else {
            self.path(".cache/google-chrome/Default/Cache")
        }
    }

    /// Where this OS keeps the fixture's Chrome `Default` profile files.
    pub fn chrome_profile(&self) -> PathBuf {
        if cfg!(target_os = "macos") {
            self.path("Library/Application Support/Google/Chrome/Default")
        } else if cfg!(windows) {
            self.path("AppData/Local/Google/Chrome/User Data/Default")
        } else {
            self.path(".config/google-chrome/Default")
        }
    }

    /// Files created by the test count as old (see [`Home::old_context`]).
    pub fn config(&self) -> Config {
        self.config_at(Arc::new(Mutex::new(a_week_ahead())))
    }

    pub fn engine(&self) -> Engine {
        Engine::new(self.config())
    }
}

pub fn scan(engine: &Engine) -> ScanView {
    let scan_id = engine
        .start_scan(StartScanArgs {
            mode: ScanMode::Normal,
        })
        .unwrap();
    let view = engine.wait_for_scan(ScanRef { scan_id }).unwrap();
    assert_eq!(view.status, ScanStatus::Finished, "{:?}", view.error);
    view
}

pub fn named<'a>(view: &'a ScanView, name: &str) -> &'a Candidate {
    view.candidates
        .iter()
        .find(|c| c.display_name == name)
        .unwrap_or_else(|| panic!("no candidate {name}"))
}

pub fn plan(engine: &Engine, view: &ScanView, names: &[&str]) -> Plan {
    engine
        .build_plan(BuildPlanArgs {
            scan_id: view.scan_id,
            candidate_ids: names.iter().map(|n| named(view, n).id).collect(),
        })
        .unwrap()
}

pub fn confirm(engine: &Engine, plan: &Plan) -> ConfirmationToken {
    engine
        .confirm_plan(PlanRef {
            plan_id: plan.plan_id,
        })
        .unwrap()
}

pub fn execute(engine: &Engine, plan: &Plan, token: ConfirmationToken) -> Report {
    let start = engine
        .execute_plan(ExecutePlanArgs {
            plan_id: plan.plan_id,
            confirmation_token: token,
            keep_log_in_memory: false,
        })
        .unwrap();
    assert_eq!(start, ExecuteStart::Started);
    engine
        .wait_for_run(PlanRef {
            plan_id: plan.plan_id,
        })
        .unwrap()
}

/// Scan, plan the named entries, confirm, execute.
pub fn clean(engine: &Engine, names: &[&str]) -> Report {
    let view = scan(engine);
    let plan = plan(engine, &view, names);
    let token = confirm(engine, &plan);
    execute(engine, &plan, token)
}

/// The display name of the fixture's Chrome `Default` HTTP cache.
pub const CHROME_CACHE: &str = "Google Chrome — Default — Cache";

/// The name Chrome's main process has on this OS.
pub fn chrome_process() -> &'static str {
    if cfg!(target_os = "macos") {
        "Google Chrome"
    } else if cfg!(windows) {
        "chrome.exe"
    } else {
        "chrome"
    }
}

pub fn manual_clock() -> ManualClock {
    ManualClock::default()
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
