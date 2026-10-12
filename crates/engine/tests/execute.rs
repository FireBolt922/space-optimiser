//! Safe execution (phase 8). Every test runs on a temp copy of the fixture
//! home; the executor never sees a real home or temp folder. Most tests run
//! on every OS; links are symlinks on Unix and junctions on Windows.

mod common;

use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use common::Home;
use space_optimiser_engine::model::{
    AuditLogStatus, ConfirmationToken, ExecutePlanArgs, ExecuteStart, Outcome, PlanRef, Report,
    ReportItem, SkipCause,
};
use space_optimiser_engine::policy::SkipReason;
use space_optimiser_engine::{Engine, EngineError};

fn item<'a>(report: &'a Report, name: &str) -> &'a ReportItem {
    report
        .items
        .iter()
        .find(|i| i.display_name == name)
        .unwrap_or_else(|| panic!("no report item {name}"))
}

fn skipped(report: &Report, name: &str) -> SkipCause {
    let item = item(report, name);
    assert_eq!(item.outcome, Outcome::Skipped, "{item:?}");
    item.reason.unwrap()
}

fn audit_log(home: &Home) -> String {
    fs::read_to_string(home.audit_dir().join("audit.jsonl")).unwrap()
}

fn bad_token() -> ConfirmationToken {
    serde_json::from_str("\"0000000000000000\"").unwrap()
}

/// A link to a folder, planted the way each OS does it without privileges:
/// a symlink on Unix, a junction on Windows.
fn dir_link(target: &Path, link: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).unwrap();
    #[cfg(windows)]
    windows::junction(link, target);
}

#[test]
fn a_confirmed_plan_removes_old_temp_entries_and_nothing_else() {
    let home = common::basic_home();
    // Modified "in the future" relative to the test clock: fresh.
    let fresh = home.write("tmp/new-download.part", b"still downloading");
    common::set_mtime(&fresh, common::a_week_ahead());
    let engine = home.engine();
    let view = common::scan(&engine);
    assert!(
        view.candidates
            .iter()
            .all(|c| c.display_name != "new-download.part")
    );

    let report = common::clean(&engine, &["old-download.bin", "old-build"]);

    assert!(!home.path("tmp/old-download.bin").exists());
    assert!(!home.path("tmp/old-build").exists());
    assert!(fresh.exists());
    assert!(home.path("Documents/notes.txt").exists());
    assert!(home.path(".cache/app/blob.bin").exists());
    assert!(
        home.path("Library/Application Support/Fixture Browser/Default/Cookies")
            .exists()
    );
    assert!(
        home.path("tmp").is_dir(),
        "the root itself is never removed"
    );
    for name in ["old-download.bin", "old-build"] {
        let item = item(&report, name);
        assert_eq!(item.outcome, Outcome::Removed);
        assert!(item.recovery_text.is_some());
    }
    assert!(report.bytes_removed > 0);
    assert_eq!(
        report.bytes_removed,
        report.items.iter().map(|i| i.bytes_removed).sum::<u64>()
    );
    assert!(!report.stopped);
}

// Test 1: an entry swapped for a link to Documents after planning.
#[test]
fn an_entry_swapped_to_documents_after_planning_deletes_nothing() {
    let home = common::basic_home();
    let engine = home.engine();
    let view = common::scan(&engine);
    let plan = common::plan(&engine, &view, &["old-build"]);
    let token = common::confirm(&engine, &plan);

    fs::rename(home.path("tmp/old-build"), home.path("moved-build")).unwrap();
    dir_link(&home.path("Documents"), &home.path("tmp/old-build"));

    let report = common::execute(&engine, &plan, token);
    assert_eq!(skipped(&report, "old-build"), SkipCause::ChangedSinceScan);
    assert!(home.path("Documents/notes.txt").exists());
    assert!(home.path("moved-build/output.txt").exists());
    assert_eq!(report.bytes_removed, 0);
}

// Test 1, root variant: the temp root itself now points at Documents.
#[test]
fn a_temp_root_swapped_to_documents_after_planning_deletes_nothing() {
    let home = common::basic_home();
    let engine = home.engine();
    let view = common::scan(&engine);
    let plan = common::plan(&engine, &view, &["old-download.bin"]);
    let token = common::confirm(&engine, &plan);

    home.write(
        "Documents/old-download.bin",
        b"a document with the same name",
    );
    fs::rename(home.path("tmp"), home.path("old-tmp")).unwrap();
    dir_link(&home.path("Documents"), &home.path("tmp"));

    let report = common::execute(&engine, &plan, token);
    assert!(matches!(
        skipped(&report, "old-download.bin"),
        SkipCause::Root(_) | SkipCause::ChangedSinceScan
    ));
    assert!(home.path("Documents/old-download.bin").exists());
    assert!(home.path("Documents/notes.txt").exists());
}

// Test 2: a link planted inside a planned folder is not followed.
#[test]
fn a_link_planted_after_planning_is_not_followed() {
    let home = common::basic_home();
    let engine = home.engine();
    let view = common::scan(&engine);
    let plan = common::plan(&engine, &view, &["old-build"]);
    let token = common::confirm(&engine, &plan);

    dir_link(&home.path("Documents"), &home.path("tmp/old-build/docs"));

    let report = common::execute(&engine, &plan, token);
    assert_eq!(skipped(&report, "old-build"), SkipCause::ChangedSinceScan);
    assert!(home.path("Documents/notes.txt").exists());
    assert!(home.path("tmp/old-build/output.txt").exists());
}

// Test 3: an entry that became newer than the scan is kept.
#[test]
fn an_entry_modified_after_the_scan_is_kept() {
    let home = common::basic_home();
    let engine = home.engine();
    let view = common::scan(&engine);
    let plan = common::plan(&engine, &view, &["old-download.bin", "old-build"]);
    let token = common::confirm(&engine, &plan);

    common::set_mtime(&home.path("tmp/old-download.bin"), common::a_week_ahead());
    common::set_mtime(
        &home.path("tmp/old-build/nested/deep.txt"),
        common::a_week_ahead(),
    );

    let report = common::execute(&engine, &plan, token);
    assert_eq!(
        skipped(&report, "old-download.bin"),
        SkipCause::Entry(SkipReason::Fresh)
    );
    assert_eq!(
        skipped(&report, "old-build"),
        SkipCause::Entry(SkipReason::Fresh)
    );
    assert!(home.path("tmp/old-download.bin").exists());
    assert!(home.path("tmp/old-build/nested/deep.txt").exists());
    assert!(home.path("tmp/old-build/output.txt").exists());
}

// Test 10: same name, new file.
#[test]
fn an_entry_replaced_after_the_scan_is_skipped() {
    let home = common::basic_home();
    let engine = home.engine();
    let view = common::scan(&engine);
    let plan = common::plan(&engine, &view, &["old-download.bin"]);
    let token = common::confirm(&engine, &plan);

    let replacement = home.write("replacement.bin", b"a different file");
    fs::rename(&replacement, home.path("tmp/old-download.bin")).unwrap();

    let report = common::execute(&engine, &plan, token);
    assert_eq!(
        skipped(&report, "old-download.bin"),
        SkipCause::ChangedSinceScan
    );
    assert_eq!(
        fs::read(home.path("tmp/old-download.bin")).unwrap(),
        b"a different file"
    );
}

// Test 11: files that were not in the scan are never deleted, even when
// they are old enough by the time the plan runs.
#[test]
fn a_file_that_became_old_only_after_the_scan_is_not_deleted() {
    let home = common::basic_home();
    let now = Arc::new(Mutex::new(common::a_week_ahead()));
    let engine = Engine::new(home.config_at(now.clone()));
    let late = home.path("tmp/late.bin");
    let view = common::scan(&engine);
    let plan = common::plan(&engine, &view, &["old-download.bin", "old-build"]);
    let token = common::confirm(&engine, &plan);

    home.write("tmp/old-build/late.txt", b"written after the scan");
    home.write("tmp/late.bin", b"written after the scan");
    *now.lock().unwrap() += Duration::from_secs(3 * 24 * 60 * 60);

    let report = common::execute(&engine, &plan, token);
    assert_eq!(item(&report, "old-download.bin").outcome, Outcome::Removed);
    assert_eq!(skipped(&report, "old-build"), SkipCause::ChangedSinceScan);
    assert!(home.path("tmp/old-build/late.txt").exists());
    assert!(home.path("tmp/old-build/output.txt").exists());
    assert!(late.exists());
    assert!(report.items.iter().all(|i| i.display_name != "late.bin"));
}

// A project marker that appears after the scan.
#[test]
fn a_folder_that_became_a_project_after_the_scan_is_skipped() {
    let home = common::basic_home();
    let engine = home.engine();
    let view = common::scan(&engine);
    let plan = common::plan(&engine, &view, &["old-build"]);
    let token = common::confirm(&engine, &plan);

    let marker = home.write("replacement", b"[package]");
    fs::rename(&marker, home.path("tmp/old-build/nested/deep.txt")).unwrap();
    fs::rename(
        home.path("tmp/old-build/nested/deep.txt"),
        home.path("tmp/old-build/Cargo.toml"),
    )
    .unwrap();

    let report = common::execute(&engine, &plan, token);
    assert!(matches!(
        skipped(&report, "old-build"),
        SkipCause::Entry(SkipReason::Project) | SkipCause::ChangedSinceScan
    ));
    assert!(home.path("tmp/old-build/output.txt").exists());
}

// Test 5.
#[test]
fn a_wrong_confirmation_token_deletes_nothing() {
    let home = common::basic_home();
    let engine = home.engine();
    let view = common::scan(&engine);
    let plan = common::plan(&engine, &view, &["old-download.bin"]);
    let _ = common::confirm(&engine, &plan);

    let result = engine.execute_plan(ExecutePlanArgs {
        plan_id: plan.plan_id,
        confirmation_token: bad_token(),
        keep_log_in_memory: false,
    });
    assert_eq!(result, Err(EngineError::WrongToken));
    assert!(home.path("tmp/old-download.bin").exists());
    assert!(!home.audit_dir().join("audit.jsonl").exists());
}

#[test]
fn a_plan_runs_once() {
    let home = common::basic_home();
    let engine = home.engine();
    let view = common::scan(&engine);
    let plan = common::plan(&engine, &view, &["old-download.bin"]);
    let token = common::confirm(&engine, &plan);
    common::execute(&engine, &plan, token.clone());
    let again = engine.execute_plan(ExecutePlanArgs {
        plan_id: plan.plan_id,
        confirmation_token: token,
        keep_log_in_memory: false,
    });
    assert_eq!(again, Err(EngineError::UnknownPlan));
}

// Test 12: a plan older than 15 minutes is refused.
#[test]
fn a_plan_older_than_fifteen_minutes_is_refused() {
    let home = common::basic_home();
    let clock = common::manual_clock();
    let engine = Engine::new(home.config().with_clock(clock.clone()));
    let view = common::scan(&engine);
    let plan = common::plan(&engine, &view, &["old-download.bin"]);
    let token = common::confirm(&engine, &plan);

    clock.advance(Duration::from_secs(16 * 60));

    let result = engine.execute_plan(ExecutePlanArgs {
        plan_id: plan.plan_id,
        confirmation_token: token,
        keep_log_in_memory: false,
    });
    assert_eq!(result, Err(EngineError::PlanExpired));
    assert!(home.path("tmp/old-download.bin").exists());
}

#[test]
fn a_plan_within_fifteen_minutes_runs() {
    let home = common::basic_home();
    let clock = common::manual_clock();
    let engine = Engine::new(home.config().with_clock(clock.clone()));
    let view = common::scan(&engine);
    let plan = common::plan(&engine, &view, &["old-download.bin"]);
    clock.advance(Duration::from_secs(14 * 60));
    let token = common::confirm(&engine, &plan);
    let report = common::execute(&engine, &plan, token);
    assert_eq!(item(&report, "old-download.bin").outcome, Outcome::Removed);
}

// Test 12: a plan built before a sleep is refused, even a short one.
#[test]
fn a_plan_built_before_a_sleep_is_refused() {
    let home = common::basic_home();
    let clock = common::manual_clock();
    let engine = Engine::new(home.config().with_clock(clock.clone()));
    let view = common::scan(&engine);
    let plan = common::plan(&engine, &view, &["old-download.bin"]);

    clock.sleep(Duration::from_secs(60));

    assert_eq!(
        engine.confirm_plan(PlanRef {
            plan_id: plan.plan_id
        }),
        Err(EngineError::PlanExpired)
    );
    assert!(home.path("tmp/old-download.bin").exists());
}

// Test 8: the log names the path and outcome, never the bytes.
#[test]
fn the_audit_log_records_paths_and_outcomes_but_not_contents() {
    let home = common::basic_home();
    home.write("tmp/secret.bin", b"TOP-SECRET-CONTENTS");
    let engine = home.engine();

    let report = common::clean(&engine, &["secret.bin", "old-build"]);

    let log = audit_log(&home);
    assert!(log.contains("secret.bin"));
    assert!(log.contains("old-build"));
    assert!(log.contains("\"removed\""));
    assert!(!log.contains("TOP-SECRET"));
    let lines: Vec<serde_json::Value> = log
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.first().unwrap()["event"], "run_started");
    assert_eq!(lines.last().unwrap()["event"], "run_finished");
    let actions: Vec<_> = lines.iter().filter(|l| l["event"] == "action").collect();
    assert_eq!(actions.len(), report.items.len());
    let bytes: u64 = actions.iter().map(|a| a["bytes"].as_u64().unwrap()).sum();
    assert_eq!(bytes, report.bytes_removed);
    assert_eq!(report.audit_log, AuditLogStatus::Written);
    assert!(report.memory_log.is_empty());
}

// Test 9: free space is measured on the volume before and after.
#[test]
fn free_space_before_and_after_is_reported() {
    let home = common::basic_home();
    let readings = Arc::new(Mutex::new(vec![5_000u64, 1_000]));
    let source = readings.clone();
    let engine = Engine::new(
        home.config()
            .with_free_space(move |_| Ok(source.lock().unwrap().pop().unwrap())),
    );

    let report = common::clean(&engine, &["old-download.bin"]);

    assert_eq!(report.free_space.len(), 1);
    let volume = &report.free_space[0];
    assert_eq!(volume.before, Some(1_000));
    assert_eq!(volume.after, Some(5_000));
    assert!(readings.lock().unwrap().is_empty());
}

// Test 13: the audit log cannot be written. Nothing runs until a second
// confirm; then the log is kept in memory and shown in the report.
#[test]
fn an_unwritable_audit_log_needs_a_second_confirm() {
    let home = common::basic_home();
    fs::write(home.audit_dir(), b"a file where the log folder should be").unwrap();
    let engine = home.engine();
    let view = common::scan(&engine);
    let plan = common::plan(&engine, &view, &["old-download.bin"]);
    let token = common::confirm(&engine, &plan);
    let args = |keep| ExecutePlanArgs {
        plan_id: plan.plan_id,
        confirmation_token: token.clone(),
        keep_log_in_memory: keep,
    };

    let first = engine.execute_plan(args(false)).unwrap();
    assert!(matches!(first, ExecuteStart::AuditLogUnavailable { .. }));
    assert!(home.path("tmp/old-download.bin").exists());

    assert_eq!(engine.execute_plan(args(true)), Ok(ExecuteStart::Started));
    let report = engine
        .wait_for_run(PlanRef {
            plan_id: plan.plan_id,
        })
        .unwrap();
    assert!(!home.path("tmp/old-download.bin").exists());
    assert_eq!(report.audit_log, AuditLogStatus::MemoryOnly);
    assert!(
        report
            .memory_log
            .iter()
            .any(|l| l.contains("old-download.bin"))
    );
    assert!(report.memory_log.first().unwrap().contains("run_started"));
}

// Test 14: a run that started and never finished is reported once.
#[test]
fn an_interrupted_run_is_reported_on_the_next_launch() {
    let home = common::basic_home();
    let engine = home.engine();
    assert!(engine.interrupted_run().is_none());
    common::clean(&engine, &["old-download.bin", "old-build"]);
    drop(engine);

    // Simulate a crash before the "run finished" line was written.
    let log = audit_log(&home);
    let crashed: Vec<&str> = log.lines().take(2).collect();
    fs::write(
        home.audit_dir().join("audit.jsonl"),
        crashed.join("\n") + "\n",
    )
    .unwrap();

    let next = home.engine();
    let interrupted = next.interrupted_run().unwrap();
    assert_eq!(interrupted.actions.len(), 1);
    assert!(interrupted.started_unix_secs > 0);

    assert!(home.engine().interrupted_run().is_none());
}

// Test 15: Stop finishes the current item and halts.
#[test]
fn stop_after_the_first_item_leaves_the_rest_not_run() {
    let home = common::basic_home();
    home.write("tmp/third.bin", b"third");
    let engine = Engine::new(home.config().with_after_item(|done, stop| {
        if done == 1 {
            stop.cancel();
        }
    }));

    let report = common::clean(&engine, &["old-download.bin", "old-build", "third.bin"]);

    assert!(report.stopped);
    assert_eq!(report.items[0].outcome, Outcome::Removed);
    assert_eq!(report.items[1].outcome, Outcome::NotRun);
    assert_eq!(report.items[2].outcome, Outcome::NotRun);
    let gone = ["tmp/old-download.bin", "tmp/old-build", "tmp/third.bin"]
        .iter()
        .filter(|p| !home.path(p).exists())
        .count();
    assert_eq!(gone, 1);
    assert!(audit_log(&home).contains("\"not_run\""));
}

#[test]
fn only_one_plan_runs_at_a_time() {
    let home = common::basic_home();
    let engine_cell: Arc<OnceLock<(Engine, ExecutePlanArgs)>> = Arc::new(OnceLock::new());
    let seen = Arc::new(Mutex::new(None));
    let (cell, out) = (engine_cell.clone(), seen.clone());
    let engine = Engine::new(home.config().with_after_item(move |_, _| {
        if let Some((engine, args)) = cell.get() {
            let args = ExecutePlanArgs {
                plan_id: args.plan_id,
                confirmation_token: args.confirmation_token.clone(),
                keep_log_in_memory: false,
            };
            out.lock().unwrap().get_or_insert(engine.execute_plan(args));
        }
    }));
    let view = common::scan(&engine);
    let first = common::plan(&engine, &view, &["old-download.bin"]);
    let second = common::plan(&engine, &view, &["old-build"]);
    let first_token = common::confirm(&engine, &first);
    let second_token = common::confirm(&engine, &second);
    let _ = engine_cell.set((
        engine.clone(),
        ExecutePlanArgs {
            plan_id: second.plan_id,
            confirmation_token: second_token,
            keep_log_in_memory: false,
        },
    ));

    common::execute(&engine, &first, first_token);

    assert_eq!(
        seen.lock().unwrap().clone().unwrap(),
        Err(EngineError::RunInProgress)
    );
    assert!(home.path("tmp/old-build").exists());
}

#[test]
fn a_plan_from_an_older_scan_is_refused() {
    let home = common::basic_home();
    let engine = home.engine();
    let view = common::scan(&engine);
    let plan = common::plan(&engine, &view, &["old-download.bin"]);
    let token = common::confirm(&engine, &plan);
    common::scan(&engine);
    let result = engine.execute_plan(ExecutePlanArgs {
        plan_id: plan.plan_id,
        confirmation_token: token,
        keep_log_in_memory: false,
    });
    assert_eq!(result, Err(EngineError::UnknownPlan));
    assert!(home.path("tmp/old-download.bin").exists());
}

// Phase 5 and phase 8 item 3 for cache rows: the folder is emptied, the
// folder itself and every profile file next to it stay.
#[test]
fn a_confirmed_browser_cache_is_emptied_and_the_profile_kept() {
    let home = common::basic_home();
    let engine = home.engine();
    let report = common::clean(&engine, &[common::CHROME_CACHE]);

    let done = item(&report, common::CHROME_CACHE);
    assert_eq!(done.outcome, Outcome::Removed, "{done:?}");
    assert!(done.bytes_removed > 0);
    assert_eq!(
        done.recovery_text,
        Some("Those sites will load from the network next time.")
    );
    let cache = home.chrome_cache();
    assert!(cache.is_dir(), "the cache folder itself stays");
    assert_eq!(fs::read_dir(&cache).unwrap().count(), 0);
    assert!(home.chrome_profile().join("Cookies").is_file());
    assert!(
        cache
            .with_file_name("Code Cache")
            .join("js/index")
            .is_file()
    );
    let log = audit_log(&home);
    assert!(
        log.contains("\"catalog_id\":\"browser-http-cache\""),
        "{log}"
    );
    assert!(log.contains("\"outcome\":\"removed\""), "{log}");
}

#[test]
fn a_running_browser_cannot_be_planned() {
    let home = common::basic_home();
    let engine = Engine::new(
        home.config_with(|ctx| ctx.with_running_processes(&[common::chrome_process()])),
    );
    let view = common::scan(&engine);
    let result = engine.build_plan(space_optimiser_engine::model::BuildPlanArgs {
        scan_id: view.scan_id,
        candidate_ids: vec![common::named(&view, common::CHROME_CACHE).id],
    });
    assert_eq!(result.unwrap_err(), EngineError::NotActionable);
}

// Test 8 at execute time: the browser opened between confirm and run.
#[test]
fn a_browser_opened_after_the_scan_keeps_its_cache() {
    let home = common::basic_home();
    let running: Arc<Mutex<Vec<String>>> = Arc::default();
    let shared = running.clone();
    let engine = Engine::new(
        home.config_with(move |ctx| ctx.with_running_processes(&shared.lock().unwrap())),
    );
    let view = common::scan(&engine);
    let plan = common::plan(&engine, &view, &[common::CHROME_CACHE]);
    let token = common::confirm(&engine, &plan);
    running
        .lock()
        .unwrap()
        .push(common::chrome_process().to_owned());

    let report = common::execute(&engine, &plan, token);

    assert_eq!(
        skipped(&report, common::CHROME_CACHE),
        SkipCause::AppRunning
    );
    assert!(home.chrome_cache().join("Cache_Data/f_000001").is_file());
}

#[test]
fn a_browser_cache_is_kept_when_processes_cannot_be_listed() {
    let home = common::basic_home();
    let broken = Arc::new(Mutex::new(false));
    let flag = broken.clone();
    let engine = Engine::new(home.config_with(move |ctx| {
        if *flag.lock().unwrap() {
            ctx.with_process_list_error()
        } else {
            ctx
        }
    }));
    let view = common::scan(&engine);
    let plan = common::plan(&engine, &view, &[common::CHROME_CACHE]);
    let token = common::confirm(&engine, &plan);
    *broken.lock().unwrap() = true;

    let report = common::execute(&engine, &plan, token);

    assert_eq!(
        skipped(&report, common::CHROME_CACHE),
        SkipCause::AppCheckFailed
    );
    assert!(home.chrome_cache().join("Cache_Data/f_000001").is_file());
}

// Only entries in the scan snapshot are eligible (phase 8 item 3).
#[test]
fn a_file_added_to_a_cache_after_the_scan_skips_the_cache() {
    let home = common::basic_home();
    let engine = home.engine();
    let view = common::scan(&engine);
    let plan = common::plan(&engine, &view, &[common::CHROME_CACHE]);
    let token = common::confirm(&engine, &plan);
    let added = home.chrome_cache().join("Cache_Data/f_000099");
    fs::write(&added, b"new").unwrap();

    let report = common::execute(&engine, &plan, token);

    assert_eq!(
        skipped(&report, common::CHROME_CACHE),
        SkipCause::ChangedSinceScan
    );
    assert!(added.is_file());
    assert!(home.chrome_cache().join("Cache_Data/f_000001").is_file());
}

// Test 1 for cache rows: the folder swapped for a link to Documents.
#[test]
fn a_cache_swapped_for_a_link_after_planning_deletes_nothing() {
    let home = common::basic_home();
    let engine = home.engine();
    let view = common::scan(&engine);
    let plan = common::plan(&engine, &view, &[common::CHROME_CACHE]);
    let token = common::confirm(&engine, &plan);
    let cache = home.chrome_cache();
    let moved = cache.with_file_name("Cache-moved");
    fs::rename(&cache, &moved).unwrap();
    dir_link(&home.path("Documents"), &cache);

    let report = common::execute(&engine, &plan, token);

    assert_eq!(
        skipped(&report, common::CHROME_CACHE),
        SkipCause::ChangedSinceScan
    );
    assert!(home.path("Documents/notes.txt").is_file());
    assert!(moved.join("Cache_Data/f_000001").is_file());
}

#[cfg(unix)]
mod unix {
    use super::*;

    // Phase 8 risk: a folder that held a link at scan time. The link is
    // removed; its target is not.
    #[test]
    fn a_link_inside_a_planned_folder_is_removed_and_its_target_kept() {
        let home = common::basic_home();
        std::os::unix::fs::symlink(
            home.path("Documents/notes.txt"),
            home.path("tmp/old-build/notes-link"),
        )
        .unwrap();
        std::os::unix::fs::symlink(home.path("Documents"), home.path("tmp/old-build/docs"))
            .unwrap();
        let engine = home.engine();

        let report = common::clean(&engine, &["old-build"]);

        assert_eq!(item(&report, "old-build").outcome, Outcome::Removed);
        assert!(!home.path("tmp/old-build").exists());
        assert_eq!(
            fs::read(home.path("Documents/notes.txt")).unwrap(),
            fs::read(common::fixture_source().join("Documents/notes.txt")).unwrap()
        );
    }

    // Test 17: a socket created inside a planned folder after the scan.
    #[test]
    fn a_folder_with_a_socket_created_after_the_scan_is_skipped_whole() {
        let home = common::basic_home();
        let engine = home.engine();
        let view = common::scan(&engine);
        let plan = common::plan(&engine, &view, &["old-build"]);
        let token = common::confirm(&engine, &plan);

        let _listener =
            std::os::unix::net::UnixListener::bind(home.path("tmp/old-build/agent.sock")).unwrap();

        let report = common::execute(&engine, &plan, token);
        assert!(matches!(
            skipped(&report, "old-build"),
            SkipCause::Entry(SkipReason::SocketOrPipe) | SkipCause::ChangedSinceScan
        ));
        assert!(home.path("tmp/old-build/output.txt").exists());
        assert!(home.path("tmp/old-build/nested/deep.txt").exists());
    }

    // Tests 4 and 16: a locked file is skipped and not modified; the rest of
    // the batch is removed and the run finishes normally.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_locked_file_is_skipped_and_the_rest_of_the_batch_is_removed() {
        let home = common::basic_home();
        let engine = home.engine();
        let view = common::scan(&engine);
        let plan = common::plan(&engine, &view, &["old-download.bin", "old-build"]);
        let token = common::confirm(&engine, &plan);

        let locked = home.path("tmp/old-download.bin");
        let inner = home.path("tmp/old-build/nested/deep.txt");
        let before = fs::read(&locked).unwrap();
        set_immutable(&locked, true);
        set_immutable(&inner, true);

        let report = common::execute(&engine, &plan, token);
        let still_there = locked.exists() && inner.exists();
        let unchanged = fs::read(&locked).ok() == Some(before);
        set_immutable(&locked, false);
        set_immutable(&inner, false);

        assert!(still_there && unchanged);
        assert_eq!(
            skipped(&report, "old-download.bin"),
            SkipCause::Entry(SkipReason::Locked)
        );
        assert_eq!(
            skipped(&report, "old-build"),
            SkipCause::Entry(SkipReason::Locked)
        );
        assert!(home.path("tmp/old-build/output.txt").exists());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_locked_file_does_not_stop_the_other_items() {
        let home = common::basic_home();
        home.write("tmp/second.bin", b"second");
        let engine = home.engine();
        let view = common::scan(&engine);
        let plan = common::plan(&engine, &view, &["old-download.bin", "second.bin"]);
        let token = common::confirm(&engine, &plan);
        let locked = home.path("tmp/old-download.bin");
        set_immutable(&locked, true);

        let report = common::execute(&engine, &plan, token);
        set_immutable(&locked, false);

        assert_eq!(
            skipped(&report, "old-download.bin"),
            SkipCause::Entry(SkipReason::Locked)
        );
        assert_eq!(item(&report, "second.bin").outcome, Outcome::Removed);
        assert!(!home.path("tmp/second.bin").exists());
    }

    #[cfg(target_os = "macos")]
    fn set_immutable(path: &std::path::Path, on: bool) {
        use std::os::unix::ffi::OsStrExt;
        let c_path = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        let flags = if on { libc::UF_IMMUTABLE } else { 0 };
        // SAFETY: `c_path` is NUL-terminated.
        assert_eq!(unsafe { libc::chflags(c_path.as_ptr(), flags) }, 0);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_non_utf8_name_is_logged_losslessly() {
        use std::os::unix::ffi::OsStrExt;
        let home = common::basic_home();
        let name = std::ffi::OsStr::from_bytes(b"bad-\xff-name");
        fs::write(home.path("tmp").join(name), b"x").unwrap();
        let engine = home.engine();
        let view = common::scan(&engine);
        let ids = view.candidates.iter().map(|c| c.id).collect();
        let plan = engine
            .build_plan(space_optimiser_engine::model::BuildPlanArgs {
                scan_id: view.scan_id,
                candidate_ids: ids,
            })
            .unwrap();
        let token = common::confirm(&engine, &plan);
        common::execute(&engine, &plan, token);
        assert!(audit_log(&home).contains(r"bad-\\xff-name"));
    }

    // Test 13 on a real full disk: `/dev/full` fails every write with ENOSPC.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_full_disk_is_named_in_the_warning() {
        let home = common::basic_home();
        fs::create_dir_all(home.audit_dir()).unwrap();
        std::os::unix::fs::symlink("/dev/full", home.audit_dir().join("audit.jsonl")).unwrap(); // hygiene: data only
        let engine = home.engine();
        let view = common::scan(&engine);
        let plan = common::plan(&engine, &view, &["old-download.bin"]);
        let token = common::confirm(&engine, &plan);
        let first = engine
            .execute_plan(ExecutePlanArgs {
                plan_id: plan.plan_id,
                confirmation_token: token,
                keep_log_in_memory: false,
            })
            .unwrap();
        assert_eq!(first, ExecuteStart::AuditLogUnavailable { disk_full: true });
        assert!(home.path("tmp/old-download.bin").exists());
    }

    #[test]
    fn execution_is_refused_as_root() {
        let home = common::basic_home();
        let root = Arc::new(Mutex::new(false));
        let flag = root.clone();
        let base = home.root.clone();
        let config = space_optimiser_engine::Config::new(
            move || {
                let ctx = space_optimiser_engine::policy::Context::for_fixture(&base)
                    .with_now(common::a_week_ahead())
                    .with_temp_root(
                        base.join("tmp"),
                        space_optimiser_engine::policy::OwnerRule::Root,
                    );
                Ok(if *flag.lock().unwrap() {
                    ctx.with_euid(0)
                } else {
                    ctx
                })
            },
            home.audit_dir(),
        );
        let engine = Engine::new(config);
        let view = common::scan(&engine);
        let plan = common::plan(&engine, &view, &["old-download.bin"]);
        let token = common::confirm(&engine, &plan);
        *root.lock().unwrap() = true;
        let result = engine.execute_plan(ExecutePlanArgs {
            plan_id: plan.plan_id,
            confirmation_token: token,
            keep_log_in_memory: false,
        });
        assert_eq!(result, Err(EngineError::RunningAsRoot));
        assert!(home.path("tmp/old-download.bin").exists());
    }

    // A link inside a cache is removed, never followed.
    #[test]
    fn a_link_inside_a_cache_is_removed_and_its_target_kept() {
        let home = common::basic_home();
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("keep.txt");
        fs::write(&target, b"keep").unwrap();
        std::os::unix::fs::symlink(&target, home.chrome_cache().join("link")).unwrap();
        let engine = home.engine();

        let report = common::clean(&engine, &[common::CHROME_CACHE]);

        assert_eq!(
            item(&report, common::CHROME_CACHE).outcome,
            Outcome::Removed
        );
        assert!(fs::symlink_metadata(home.chrome_cache().join("link")).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"keep");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn thumbnails_and_fonts_are_emptied_and_font_files_kept() {
        let home = common::basic_home();
        let engine = home.engine();
        let report = common::clean(&engine, &["Thumbnails", "Font cache"]);

        for name in ["Thumbnails", "Font cache"] {
            assert_eq!(item(&report, name).outcome, Outcome::Removed, "{name}");
        }
        for folder in [".cache/thumbnails", ".cache/fontconfig"] {
            let path = home.path(folder);
            assert!(path.is_dir());
            assert_eq!(fs::read_dir(&path).unwrap().count(), 0, "{folder}");
        }
        assert!(home.path(".local/share/fonts/FixtureSans.ttf").is_file());
    }
}

#[cfg(windows)]
mod windows {
    use super::*;

    // Test 18: a junction inside a cache that points at Documents. The
    // junction is removed as a link; nothing in Documents is touched.
    #[test]
    fn a_junction_in_a_cache_is_removed_and_documents_kept() {
        let home = common::basic_home();
        junction(&home.chrome_cache().join("docs"), &home.path("Documents"));
        let engine = home.engine();

        let report = common::clean(&engine, &[common::CHROME_CACHE]);

        let done = item(&report, common::CHROME_CACHE);
        assert_eq!(done.outcome, Outcome::Removed, "{done:?}");
        let cache = home.chrome_cache();
        assert!(cache.is_dir(), "the cache folder itself stays");
        assert_eq!(fs::read_dir(&cache).unwrap().count(), 0);
        assert_eq!(
            fs::read(home.path("Documents/notes.txt")).unwrap(),
            fs::read(common::fixture_source().join("Documents/notes.txt")).unwrap()
        );
    }

    // Phase 8 risk, temp variant: a planned folder that held a junction at
    // scan time.
    #[test]
    fn a_junction_inside_a_planned_folder_is_removed_and_its_target_kept() {
        let home = common::basic_home();
        junction(&home.path("tmp/old-build/docs"), &home.path("Documents"));
        let engine = home.engine();

        let report = common::clean(&engine, &["old-build"]);

        assert_eq!(item(&report, "old-build").outcome, Outcome::Removed);
        assert!(!home.path("tmp/old-build").exists());
        assert!(home.path("Documents/notes.txt").is_file());
    }

    // Phase 9 Windows item 4: a symbolic link is removed, never followed.
    // Creating one needs Developer Mode or a privilege; without it the
    // test has nothing to check and says so.
    #[test]
    fn a_symbolic_link_inside_a_planned_folder_is_not_followed() {
        let home = common::basic_home();
        let made = std::os::windows::fs::symlink_dir(
            home.path("Documents"),
            home.path("tmp/old-build/docs-link"),
        )
        .and_then(|()| {
            std::os::windows::fs::symlink_file(
                home.path("Documents/notes.txt"),
                home.path("tmp/old-build/notes-link"),
            )
        });
        if let Err(e) = made {
            eprintln!("not checked: this account cannot create symbolic links ({e})");
            return;
        }
        let engine = home.engine();

        let report = common::clean(&engine, &["old-build"]);

        assert_eq!(item(&report, "old-build").outcome, Outcome::Removed);
        assert!(!home.path("tmp/old-build").exists());
        assert_eq!(
            fs::read(home.path("Documents/notes.txt")).unwrap(),
            fs::read(common::fixture_source().join("Documents/notes.txt")).unwrap()
        );
    }

    fn set_read_only(path: &Path, on: bool) {
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_readonly(on);
        fs::set_permissions(path, permissions).unwrap();
    }

    // Test 16: a read-only file is skipped and not modified; the attribute
    // is never cleared.
    #[test]
    fn a_read_only_file_is_skipped_and_not_modified() {
        let home = common::basic_home();
        let engine = home.engine();
        let view = common::scan(&engine);
        let plan = common::plan(&engine, &view, &["old-download.bin", "old-build"]);
        let token = common::confirm(&engine, &plan);

        let locked = home.path("tmp/old-download.bin");
        let inner = home.path("tmp/old-build/nested/deep.txt");
        let before = fs::read(&locked).unwrap();
        set_read_only(&locked, true);
        set_read_only(&inner, true);

        let report = common::execute(&engine, &plan, token);
        let still_read_only = fs::metadata(&locked).unwrap().permissions().readonly()
            && fs::metadata(&inner).unwrap().permissions().readonly();
        let unchanged = fs::read(&locked).ok() == Some(before);
        set_read_only(&locked, false);
        set_read_only(&inner, false);

        assert!(still_read_only && unchanged);
        assert_eq!(
            skipped(&report, "old-download.bin"),
            SkipCause::Entry(SkipReason::Locked)
        );
        assert_eq!(
            skipped(&report, "old-build"),
            SkipCause::Entry(SkipReason::Locked)
        );
        assert!(home.path("tmp/old-build/output.txt").exists());
    }

    // Test 4: a file another program holds open without sharing delete
    // access. It is skipped, the rest of the batch is removed, and the run
    // finishes normally.
    #[test]
    fn a_file_held_open_is_skipped_and_the_others_are_removed() {
        use std::os::windows::fs::OpenOptionsExt;
        let home = common::basic_home();
        home.write("tmp/second.bin", b"second");
        let engine = home.engine();
        let view = common::scan(&engine);
        let plan = common::plan(&engine, &view, &["old-download.bin", "second.bin"]);
        let token = common::confirm(&engine, &plan);
        let held = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(home.path("tmp/old-download.bin"))
            .unwrap();

        let report = common::execute(&engine, &plan, token);
        drop(held);

        assert_eq!(
            skipped(&report, "old-download.bin"),
            SkipCause::Entry(SkipReason::Locked)
        );
        assert!(home.path("tmp/old-download.bin").exists());
        assert_eq!(item(&report, "second.bin").outcome, Outcome::Removed);
        assert!(!home.path("tmp/second.bin").exists());
        assert!(!report.stopped);
    }

    // Inside a planned folder the walk records the error and goes on, as
    // on Unix: the rest of the folder goes, the held file and its folder
    // stay, and the item is reported failed with the system's error.
    #[test]
    fn a_file_held_open_inside_a_folder_fails_that_folder_only() {
        use std::os::windows::fs::OpenOptionsExt;
        let home = common::basic_home();
        let engine = home.engine();
        let view = common::scan(&engine);
        let plan = common::plan(&engine, &view, &["old-build", "old-download.bin"]);
        let token = common::confirm(&engine, &plan);
        let held = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(home.path("tmp/old-build/output.txt"))
            .unwrap();

        let report = common::execute(&engine, &plan, token);
        drop(held);

        let done = item(&report, "old-build");
        assert_eq!(done.outcome, Outcome::Failed, "{done:?}");
        assert!(done.detail.is_some());
        assert!(home.path("tmp/old-build/output.txt").exists());
        assert!(!home.path("tmp/old-build/nested").exists());
        assert_eq!(item(&report, "old-download.bin").outcome, Outcome::Removed);
    }

    // Phase 9 Windows item 5: names Win32 cannot spell and paths longer
    // than MAX_PATH. The fixture path is canonical, so it carries `\\?\`
    // and the test can create them.
    #[test]
    fn names_win32_cannot_spell_and_long_paths_are_removed() {
        let home = common::basic_home();
        let tmp = home.path("tmp");
        assert!(tmp.to_str().unwrap().starts_with(r"\\?\"), "{tmp:?}");
        let names = ["trailing-dot.", "trailing-space ", "CON"];
        for name in names {
            fs::write(tmp.join(name), b"odd name").unwrap();
        }
        let mut deep = tmp.join("long-tree");
        for part in ["a", "b", "c"] {
            deep.push(part.repeat(100));
        }
        fs::create_dir_all(&deep).unwrap();
        let file = deep.join("file.bin");
        fs::write(&file, b"deep").unwrap();
        assert!(file.as_os_str().len() > 260);
        let engine = home.engine();

        let report = common::clean(
            &engine,
            &["trailing-dot.", "trailing-space ", "CON", "long-tree"],
        );

        for name in names.into_iter().chain(["long-tree"]) {
            let done = item(&report, name);
            assert_eq!(done.outcome, Outcome::Removed, "{name}: {done:?}");
            assert!(fs::symlink_metadata(tmp.join(name)).is_err(), "{name}");
        }
        assert!(tmp.is_dir());
        assert!(home.path("tmp/old-download.bin").exists());
    }

    /// Creates a directory junction with `FSCTL_SET_REPARSE_POINT`, without
    /// spawning `mklink`.
    pub(super) fn junction(link: &Path, target: &Path) {
        use std::os::windows::ffi::OsStrExt;
        use std::os::windows::fs::OpenOptionsExt;
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        };
        use windows_sys::Win32::System::IO::DeviceIoControl;
        use windows_sys::Win32::System::Ioctl::FSCTL_SET_REPARSE_POINT;

        const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA000_0003;

        fs::create_dir(link).unwrap();
        let target = target.as_os_str().to_string_lossy();
        let print = target.trim_start_matches(r"\\?\");
        let substitute: Vec<u16> = format!(r"\??\{print}").encode_utf16().collect();
        let print: Vec<u16> = std::ffi::OsStr::new(print).encode_wide().collect();

        let mut path_buffer = Vec::new();
        for unit in substitute
            .iter()
            .chain([&0])
            .chain(print.iter())
            .chain([&0])
        {
            path_buffer.extend_from_slice(&unit.to_le_bytes());
        }
        let data_len = (8 + path_buffer.len()) as u16;
        let mut buffer = Vec::new();
        buffer.extend_from_slice(&IO_REPARSE_TAG_MOUNT_POINT.to_le_bytes());
        buffer.extend_from_slice(&data_len.to_le_bytes());
        buffer.extend_from_slice(&0u16.to_le_bytes());
        buffer.extend_from_slice(&0u16.to_le_bytes());
        buffer.extend_from_slice(&((substitute.len() * 2) as u16).to_le_bytes());
        buffer.extend_from_slice(&(((substitute.len() + 1) * 2) as u16).to_le_bytes());
        buffer.extend_from_slice(&((print.len() * 2) as u16).to_le_bytes());
        buffer.extend_from_slice(&path_buffer);

        let handle = fs::OpenOptions::new()
            .write(true)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(link)
            .unwrap();
        let mut returned = 0u32;
        // SAFETY: the handle is open for writing and `buffer` holds a
        // complete mount-point reparse buffer of the length passed.
        let ok = unsafe {
            DeviceIoControl(
                handle.as_raw_handle(),
                FSCTL_SET_REPARSE_POINT,
                buffer.as_ptr().cast(),
                buffer.len() as u32,
                std::ptr::null_mut(),
                0,
                &mut returned,
                std::ptr::null_mut(),
            )
        };
        assert_ne!(ok, 0, "{}", std::io::Error::last_os_error());
    }
}
