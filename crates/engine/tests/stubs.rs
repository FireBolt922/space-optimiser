//! Until M3, every command is wired and returns Unsupported. Nothing can
//! delete yet.

use space_optimiser_engine::{
    EngineError,
    model::{BuildPlanArgs, ExecutePlanArgs, PlanRef, RevealArgs, ScanRef, StartScanArgs},
};

fn parse<T: serde::de::DeserializeOwned>(json: &str) -> T {
    serde_json::from_str(json).unwrap()
}

#[test]
fn every_command_is_unsupported_until_m3() {
    let scan: ScanRef = parse(r#"{"scan_id":1}"#);
    let plan: PlanRef = parse(r#"{"plan_id":1}"#);
    let unsupported = |r: Result<(), EngineError>| matches!(r, Err(EngineError::Unsupported));

    assert!(unsupported(
        space_optimiser_engine::start_scan(parse::<StartScanArgs>(r#"{"mode":"normal"}"#))
            .map(drop)
    ));
    assert!(unsupported(space_optimiser_engine::cancel_scan(scan)));
    assert!(unsupported(
        space_optimiser_engine::get_scan(scan).map(drop)
    ));
    assert!(unsupported(
        space_optimiser_engine::build_plan(parse::<BuildPlanArgs>(
            r#"{"scan_id":1,"candidate_ids":[1,2]}"#
        ))
        .map(drop)
    ));
    assert!(unsupported(
        space_optimiser_engine::confirm_plan(plan).map(drop)
    ));
    assert!(unsupported(
        space_optimiser_engine::execute_plan(parse::<ExecutePlanArgs>(
            r#"{"plan_id":1,"confirmation_token":"t"}"#
        ))
        .map(drop)
    ));
    assert!(unsupported(space_optimiser_engine::reveal(parse::<
        RevealArgs,
    >(
        r#"{"candidate_id":1}"#
    ))));
}
