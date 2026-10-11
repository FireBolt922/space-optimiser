//! Startup guards: refuse root or elevated, and allow one instance (phase 2
//! items 14 and 15, review C11).

mod common;

use std::fs;

use space_optimiser_engine::guard::{
    GuardError, InstanceLock, is_fixture_home, should_refuse, should_refuse_elevated,
};

#[test]
fn root_is_refused() {
    assert!(should_refuse(0));
}

#[test]
fn a_normal_user_is_not_refused() {
    assert!(!should_refuse(501));
    assert!(!should_refuse(1000));
}

#[test]
fn a_second_instance_cannot_take_the_lock() {
    let dir = tempfile::tempdir().unwrap();
    let first = InstanceLock::acquire(dir.path()).unwrap();
    assert!(matches!(
        InstanceLock::acquire(dir.path()),
        Err(GuardError::AlreadyRunning)
    ));
    drop(first);
    assert!(InstanceLock::acquire(dir.path()).is_ok());
}

#[test]
fn an_elevated_process_is_refused() {
    assert!(should_refuse_elevated(true));
    assert!(!should_refuse_elevated(false));
}

// The debug fixture window only scans a folder that carries the marker.
#[test]
fn a_fixture_home_carries_the_marker_file() {
    let home = common::basic_home();
    assert!(is_fixture_home(&home.root));

    let marker = home.path(".space-optimiser-fixture");
    fs::rename(&marker, home.root.parent().unwrap().join("marker")).unwrap();
    assert!(!is_fixture_home(&home.root));

    fs::create_dir(&marker).unwrap();
    assert!(!is_fixture_home(&home.root), "a folder is not the marker");

    let empty = tempfile::tempdir().unwrap();
    assert!(!is_fixture_home(empty.path()));
}

#[cfg(unix)]
#[test]
fn a_linked_marker_does_not_count() {
    let home = common::basic_home();
    let marker = home.path(".space-optimiser-fixture");
    let elsewhere = home.root.parent().unwrap().join("marker");
    fs::rename(&marker, &elsewhere).unwrap();
    std::os::unix::fs::symlink(&elsewhere, &marker).unwrap();
    assert!(!is_fixture_home(&home.root));
}

// An elevated token is refused before anything runs (phase 3, phase 9).
#[cfg(windows)]
#[test]
fn an_elevated_process_executes_nothing() {
    use space_optimiser_engine::model::{ExecutePlanArgs, PlanRef};
    use space_optimiser_engine::{Engine, EngineError};

    let home = common::basic_home();
    let engine = Engine::new(home.config_with(|ctx| ctx.with_elevated(true)));
    let view = common::scan(&engine);
    let plan = common::plan(&engine, &view, &["old-download.bin"]);
    let token = engine
        .confirm_plan(PlanRef {
            plan_id: plan.plan_id,
        })
        .unwrap();
    let result = engine.execute_plan(ExecutePlanArgs {
        plan_id: plan.plan_id,
        confirmation_token: token,
        keep_log_in_memory: false,
    });
    assert_eq!(result.unwrap_err(), EngineError::RunningElevated);
    assert!(home.path("tmp/old-download.bin").exists());
}
