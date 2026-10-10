//! The engine's command surface: scan, plan, confirm. Ids go in, views come
//! out, and no view carries a path (phase 3, phase 7).

mod common;

use space_optimiser_engine::model::{
    BuildPlanArgs, CandidateId, PlanRef, RevealArgs, ScanMode, ScanRef, ScanStatus, StartScanArgs,
};
use space_optimiser_engine::policy::CatalogId;
use space_optimiser_engine::{Config, Engine, EngineError};

fn parse<T: serde::de::DeserializeOwned>(json: &str) -> T {
    serde_json::from_str(json).unwrap()
}

#[test]
fn a_scan_finds_the_fixture_candidates_and_explains_skips() {
    let home = common::basic_home();
    let fresh = home.write("tmp/fresh.part", b"new");
    common::set_mtime(&fresh, common::a_week_ahead());
    let engine = home.engine();

    let view = common::scan(&engine);

    let mut names: Vec<_> = view
        .candidates
        .iter()
        .filter(|c| c.catalog_id == CatalogId::UserTemp)
        .map(|c| c.display_name.as_str())
        .collect();
    names.sort();
    assert_eq!(names, ["old-build", "old-download.bin"]);
    // The fixture's browser cache for this OS is found too (phase 5).
    assert!(
        view.candidates
            .iter()
            .any(|c| c.catalog_id == CatalogId::BrowserHttpCache)
    );
    let skip = view
        .skipped
        .iter()
        .find(|s| s.display_name == "fresh.part")
        .unwrap();
    assert_eq!(skip.detail, "it changed within the last day");
    assert_eq!(view.roots.len(), 1);
    assert!(view.roots[0].skipped.is_none());
    assert!(view.progress.files_seen >= 3);
}

#[test]
fn no_view_sent_to_the_ui_contains_a_path() {
    let home = common::basic_home();
    let engine = home.engine();
    let view = common::scan(&engine);
    let plan = common::plan(&engine, &view, &["old-download.bin", "old-build"]);
    let root = home.root.to_string_lossy().into_owned();
    let tmp_name = home.root.parent().unwrap().to_string_lossy().into_owned();
    for json in [
        serde_json::to_string(&view).unwrap(),
        serde_json::to_string(&plan).unwrap(),
    ] {
        assert!(!json.contains(&root), "{json}");
        assert!(!json.contains(&tmp_name), "{json}");
    }
}

#[test]
fn a_plan_lists_each_action_and_the_estimated_total() {
    let home = common::basic_home();
    let engine = home.engine();
    let view = common::scan(&engine);
    let plan = common::plan(&engine, &view, &["old-download.bin", "old-build"]);
    assert_eq!(plan.items.len(), 2);
    assert_eq!(
        plan.estimated_bytes,
        view.candidates
            .iter()
            .filter(|c| c.catalog_id == CatalogId::UserTemp)
            .map(|c| c.estimated_bytes)
            .sum::<u64>()
    );
    assert!(plan.items.iter().all(|i| !i.action_text.is_empty()));
    assert_eq!(plan.expires_in_secs, 15 * 60);
}

#[test]
fn a_duplicate_id_is_planned_once() {
    let home = common::basic_home();
    let engine = home.engine();
    let view = common::scan(&engine);
    let id = common::named(&view, "old-download.bin").id;
    let plan = engine
        .build_plan(BuildPlanArgs {
            scan_id: view.scan_id,
            candidate_ids: vec![id, id],
        })
        .unwrap();
    assert_eq!(plan.items.len(), 1);
}

#[test]
fn an_empty_or_unknown_selection_is_refused() {
    let home = common::basic_home();
    let engine = home.engine();
    let view = common::scan(&engine);
    let build = |ids: Vec<CandidateId>| {
        engine.build_plan(BuildPlanArgs {
            scan_id: view.scan_id,
            candidate_ids: ids,
        })
    };
    assert_eq!(build(vec![]).unwrap_err(), EngineError::EmptyPlan);
    assert_eq!(
        build(vec![parse("999")]).unwrap_err(),
        EngineError::UnknownCandidate
    );
    assert_eq!(
        engine
            .build_plan(BuildPlanArgs {
                scan_id: parse("999"),
                candidate_ids: vec![common::named(&view, "old-build").id],
            })
            .unwrap_err(),
        EngineError::UnknownScan
    );
}

#[test]
fn unknown_ids_are_refused() {
    let home = common::basic_home();
    let engine = home.engine();
    let scan: ScanRef = parse(r#"{"scan_id":42}"#);
    let plan: PlanRef = parse(r#"{"plan_id":42}"#);
    assert_eq!(engine.get_scan(scan).unwrap_err(), EngineError::UnknownScan);
    assert_eq!(
        engine.cancel_scan(scan).unwrap_err(),
        EngineError::UnknownScan
    );
    assert_eq!(
        engine.confirm_plan(plan).unwrap_err(),
        EngineError::UnknownPlan
    );
    assert_eq!(engine.get_run(plan).unwrap_err(), EngineError::UnknownPlan);
    assert_eq!(engine.stop_run(plan).unwrap_err(), EngineError::UnknownPlan);
}

#[test]
fn cancel_ends_a_scan() {
    let home = common::basic_home();
    let engine = home.engine();
    let scan_id = engine
        .start_scan(StartScanArgs {
            mode: ScanMode::Deep,
        })
        .unwrap();
    engine.cancel_scan(ScanRef { scan_id }).unwrap();
    let view = engine.wait_for_scan(ScanRef { scan_id }).unwrap();
    // The fixture is tiny; the scan may finish before the flag is seen.
    assert!(matches!(
        view.status,
        ScanStatus::Cancelled | ScanStatus::Finished
    ));
}

#[test]
fn a_context_that_cannot_be_resolved_fails_the_scan() {
    let home = common::basic_home();
    let engine = Engine::new(Config::new(
        || Err(std::io::Error::other("no home")),
        home.audit_dir(),
    ));
    let scan_id = engine
        .start_scan(StartScanArgs {
            mode: ScanMode::Normal,
        })
        .unwrap();
    let view = engine.wait_for_scan(ScanRef { scan_id }).unwrap();
    assert_eq!(view.status, ScanStatus::Failed);
    assert!(view.error.unwrap().contains("no home"));
    assert!(view.candidates.is_empty());
}

#[test]
fn engine_errors_reach_the_ui_as_a_kind_and_a_message() {
    let json = serde_json::to_value(EngineError::PlanExpired).unwrap();
    assert_eq!(json["kind"], "plan_expired");
    assert!(json["message"].as_str().unwrap().contains("Scan again"));
}

#[test]
fn reveal_is_not_in_this_build() {
    let home = common::basic_home();
    let engine = home.engine();
    assert_eq!(
        engine.reveal(parse::<RevealArgs>(r#"{"candidate_id":1}"#)),
        Err(EngineError::Unsupported)
    );
}
