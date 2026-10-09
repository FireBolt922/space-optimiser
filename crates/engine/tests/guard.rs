//! Startup guards: refuse root or elevated, and allow one instance (phase 2
//! items 14 and 15, review C11).

use space_optimiser_engine::guard::{GuardError, InstanceLock, should_refuse};

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
