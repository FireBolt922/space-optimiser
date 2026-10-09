//! Command payloads are typed and reject unknown fields, so a webview cannot
//! smuggle a path into a command (phase 3, review C10).

use space_optimiser_engine::model::{
    BuildPlanArgs, ExecutePlanArgs, PlanRef, RevealArgs, ScanMode, ScanRef, StartScanArgs,
};

#[test]
fn start_scan_accepts_a_mode() {
    let args: StartScanArgs = serde_json::from_str(r#"{"mode":"deep"}"#).unwrap();
    assert_eq!(args.mode, ScanMode::Deep);
}

#[test]
fn start_scan_rejects_extra_roots() {
    let err = serde_json::from_str::<StartScanArgs>(r#"{"mode":"normal","roots":["/"]}"#);
    assert!(err.is_err());
}

#[test]
fn start_scan_rejects_an_unknown_mode() {
    assert!(serde_json::from_str::<StartScanArgs>(r#"{"mode":"everything"}"#).is_err());
}

#[test]
fn execute_plan_rejects_a_path_field() {
    let json = r#"{"plan_id":1,"confirmation_token":"t","path":"/Users/me/Documents"}"#;
    assert!(serde_json::from_str::<ExecutePlanArgs>(json).is_err());
}

#[test]
fn execute_plan_accepts_id_and_token_only() {
    let json = r#"{"plan_id":1,"confirmation_token":"t"}"#;
    assert!(serde_json::from_str::<ExecutePlanArgs>(json).is_ok());
}

#[test]
fn build_plan_rejects_paths_in_place_of_ids() {
    let json = r#"{"scan_id":1,"candidate_ids":["/tmp/x"]}"#;
    assert!(serde_json::from_str::<BuildPlanArgs>(json).is_err());
}

#[test]
fn every_payload_rejects_unknown_fields() {
    assert!(serde_json::from_str::<ScanRef>(r#"{"scan_id":1,"path":"/"}"#).is_err());
    assert!(serde_json::from_str::<PlanRef>(r#"{"plan_id":1,"path":"/"}"#).is_err());
    assert!(serde_json::from_str::<RevealArgs>(r#"{"candidate_id":1,"path":"/"}"#).is_err());
    assert!(
        serde_json::from_str::<BuildPlanArgs>(r#"{"scan_id":1,"candidate_ids":[],"path":"/"}"#)
            .is_err()
    );
}
