//! Phase 10 acceptance 3: the whole workflow on a fixture copy, through the
//! same `Engine` calls the desktop shell makes: Normal Scan, "Select safe
//! items", plan, confirm, execute, report, audit log. The stale temp files
//! go, the fresh one stays, and a link's target outside the home stays.

mod common;

use std::collections::HashMap;
use std::fs;
use std::time::{Duration, Instant};

use space_optimiser_engine::Engine;
use space_optimiser_engine::model::{
    AuditLogStatus, BuildPlanArgs, ExecutePlanArgs, ExecuteStart, Outcome, PlanRef, Report,
    RunStatus, ScanMode, ScanRef, ScanStatus, ScanView, StartScanArgs,
};

const TIMEOUT: Duration = Duration::from_secs(120);

/// Polls `get_scan` as the UI does.
fn scan_like_the_ui(engine: &Engine) -> ScanView {
    let scan_id = engine
        .start_scan(StartScanArgs {
            mode: ScanMode::Normal,
        })
        .unwrap();
    let start = Instant::now();
    loop {
        let view = engine.get_scan(ScanRef { scan_id }).unwrap();
        if view.status != ScanStatus::Running {
            assert_eq!(view.status, ScanStatus::Finished, "{:?}", view.error);
            return view;
        }
        assert!(start.elapsed() < TIMEOUT, "the scan did not finish");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Polls `get_run` as the UI does.
fn wait_like_the_ui(engine: &Engine, plan: PlanRef) -> Report {
    let start = Instant::now();
    loop {
        let run = engine.get_run(plan).unwrap();
        if run.status == RunStatus::Finished {
            return run.report.expect("a finished run has a report");
        }
        assert!(start.elapsed() < TIMEOUT, "the run did not finish");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn outcome_word(outcome: Outcome) -> String {
    serde_json::to_value(outcome)
        .unwrap()
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn select_safe_items_on_a_fixture_home_removes_only_stale_temp_and_safe_caches() {
    let home = common::basic_home();
    // Stale: the fixture's temp entries. The engine's clock is a week ahead,
    // so files created now are old to it.
    assert!(home.path("tmp/old-download.bin").exists());
    assert!(home.path("tmp/old-build/nested/deep.txt").exists());
    // Fresh: modified "now" by the engine's clock.
    let fresh = home.write("tmp/fresh-download.part", b"still downloading");
    common::set_mtime(&fresh, common::a_week_ahead());

    // A folder outside the home, reached through links from a temp entry
    // and from a bulk-safe cache.
    let outside = home.root.parent().unwrap().join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("keep.txt"), b"not the app's to delete").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        symlink(&outside, home.path("tmp/old-build/escape-dir")).unwrap();
        symlink(outside.join("keep.txt"), home.path("tmp/escape-file")).unwrap();
        if cfg!(target_os = "linux") {
            symlink(&outside, home.path(".cache/thumbnails/normal/escape")).unwrap();
        }
    }

    let engine = home.engine();
    let view = scan_like_the_ui(&engine);

    // "Select safe items": the bulk-safe candidates, and nothing else.
    let selected: Vec<_> = view.candidates.iter().filter(|c| c.bulk_safe).collect();
    let names: Vec<&str> = selected.iter().map(|c| c.display_name.as_str()).collect();
    assert!(names.contains(&"old-download.bin"), "{names:?}");
    assert!(names.contains(&"old-build"), "{names:?}");
    assert!(!names.contains(&"fresh-download.part"), "{names:?}");

    let plan = engine
        .build_plan(BuildPlanArgs {
            scan_id: view.scan_id,
            candidate_ids: selected.iter().map(|c| c.id).collect(),
        })
        .unwrap();
    assert!(plan.items.iter().all(|i| i.catalog_id.bulk_safe()));
    let plan_ref = PlanRef {
        plan_id: plan.plan_id,
    };
    let token = engine.confirm_plan(plan_ref).unwrap();
    let start = engine
        .execute_plan(ExecutePlanArgs {
            plan_id: plan.plan_id,
            confirmation_token: token,
            keep_log_in_memory: false,
        })
        .unwrap();
    assert_eq!(start, ExecuteStart::Started);
    let report = wait_like_the_ui(&engine, plan_ref);

    // The stale temp entries are gone; the fresh one and the root stay.
    assert!(!home.path("tmp/old-download.bin").exists());
    assert!(!home.path("tmp/old-build").exists());
    assert!(fresh.exists());
    assert!(home.path("tmp").is_dir());
    // Links were removed as links; what they pointed at stays.
    assert_eq!(
        fs::read(outside.join("keep.txt")).unwrap(),
        b"not the app's to delete"
    );
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 1);
    // Nothing outside the selection was touched.
    assert!(home.path("Documents/notes.txt").exists());
    assert!(home.chrome_cache().is_dir());
    assert!(fs::read_dir(home.chrome_cache()).unwrap().count() > 0);
    assert!(home.path(".cargo/bin/cargo-fixture").exists());
    assert!(
        home.path("Library/Application Support/Fixture Browser/Default/Cookies")
            .exists()
    );

    // The report covers each planned item once.
    assert!(!report.stopped);
    assert_eq!(report.items.len(), plan.items.len());
    for name in ["old-download.bin", "old-build"] {
        let item = report
            .items
            .iter()
            .find(|i| i.display_name == name)
            .unwrap();
        assert_eq!(item.outcome, Outcome::Removed, "{item:?}");
    }
    assert_eq!(
        report.bytes_removed,
        report.items.iter().map(|i| i.bytes_removed).sum::<u64>()
    );
    assert!(report.bytes_removed > 0);
    assert_eq!(report.audit_log, AuditLogStatus::Written);

    // The audit log says the same as the report.
    let log = fs::read_to_string(home.audit_dir().join("audit.jsonl")).unwrap();
    let lines: Vec<serde_json::Value> = log
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let first = lines.first().unwrap();
    assert_eq!(first["event"], "run_started");
    assert_eq!(first["items"], plan.items.len());
    let last = lines.last().unwrap();
    assert_eq!(last["event"], "run_finished");
    assert_eq!(last["bytes"], report.bytes_removed);
    let removed = report
        .items
        .iter()
        .filter(|i| i.outcome == Outcome::Removed)
        .count();
    assert_eq!(last["removed"], removed);

    let mut logged: HashMap<(String, String, u64), usize> = HashMap::new();
    for action in lines.iter().filter(|l| l["event"] == "action") {
        let key = (
            action["catalog_id"].as_str().unwrap().to_owned(),
            action["outcome"].as_str().unwrap().to_owned(),
            action["bytes"].as_u64().unwrap(),
        );
        *logged.entry(key).or_default() += 1;
    }
    let mut reported: HashMap<(String, String, u64), usize> = HashMap::new();
    for item in &report.items {
        let key = (
            item.catalog_id.as_str().to_owned(),
            outcome_word(item.outcome),
            item.bytes_removed,
        );
        *reported.entry(key).or_default() += 1;
    }
    assert_eq!(logged, reported);
    for name in ["old-download.bin", "old-build"] {
        assert!(
            lines
                .iter()
                .any(|l| l["event"] == "action" && l["path"].as_str().unwrap().ends_with(name)),
            "{name} is not in the audit log"
        );
    }
    assert!(!log.contains("fresh-download.part"));
}
