//! What a scan says it is doing (v0.3 usability round): each mode has a
//! fixed list of stages, progress names the stage it is in, and a tool row
//! names its tool before the engine waits on it.

mod common;

use space_optimiser_engine::discover::{self, CancelToken, Progress, Stage};
use space_optimiser_engine::model::{ScanMode, ScanRef, StartScanArgs};

/// The stages in the order progress first reported them.
fn reported(mode: ScanMode, ctx: &space_optimiser_engine::policy::Context) -> Vec<Progress> {
    let mut seen = Vec::new();
    discover::scan(ctx, mode, &CancelToken::new(), |p| seen.push(p)).unwrap();
    seen
}

fn first_seen(progress: &[Progress]) -> Vec<Stage> {
    let mut stages: Vec<Stage> = Vec::new();
    for p in progress {
        if stages.last() != Some(&p.stage) {
            stages.push(p.stage);
        }
    }
    stages
}

#[test]
fn each_mode_lists_its_stages_in_scan_order() {
    assert_eq!(
        discover::stages(ScanMode::Normal),
        [Stage::Temp, Stage::Caches]
    );
    assert_eq!(
        discover::stages(ScanMode::Deep),
        [
            Stage::Temp,
            Stage::Caches,
            Stage::Logs,
            Stage::Tools,
            Stage::Docker,
            Stage::LargeFiles,
        ]
    );
    assert_eq!(discover::stages(ScanMode::Projects), [Stage::Projects]);
    assert_eq!(discover::stages(ScanMode::Android), [Stage::Android]);
}

// A tool's scan starts in its own stage, so the window never says
// "Checking temporary files" for a scan that does not check them.
#[test]
fn a_tool_scan_starts_and_ends_in_its_own_stage() {
    let home = common::basic_home();
    let engine = home.tool_engine();
    for (mode, stage, name) in [
        (ScanMode::Projects, Stage::Projects, "projects"),
        (ScanMode::Android, Stage::Android, "android"),
    ] {
        assert_eq!(Progress::starting(mode).stage, stage);
        let scan_id = engine.start_scan(StartScanArgs { mode }).unwrap();
        let view = engine.wait_for_scan(ScanRef { scan_id }).unwrap();
        assert_eq!(view.stages, [stage]);
        assert_eq!(view.progress.stage, stage);
        assert!(view.roots.is_empty(), "{mode:?} read a temp root");
        let json = serde_json::to_value(&view).unwrap();
        assert_eq!(json["stages"][0], name);
    }
    assert_eq!(Progress::starting(ScanMode::Deep).stage, Stage::Temp);
}

#[test]
fn progress_walks_through_every_stage_even_when_a_stage_finds_nothing() {
    let home = common::basic_home();
    for mode in [
        ScanMode::Normal,
        ScanMode::Deep,
        ScanMode::Projects,
        ScanMode::Android,
    ] {
        let progress = reported(mode, &home.with_fake_tools(home.old_context()));
        assert_eq!(first_seen(&progress), discover::stages(mode), "{mode:?}");
    }
}

#[cfg(unix)]
#[test]
fn a_tool_row_names_its_tool_before_the_engine_waits_on_it() {
    let home = common::basic_home();
    home.fake_tool("uv", "sleep 1; echo \"$HOME/.cache/uv\"");
    let progress = reported(ScanMode::Deep, &home.with_fake_tools(home.old_context()));
    let uv = progress
        .iter()
        .position(|p| p.stage == Stage::Tools && p.detail == Some("uv"))
        .expect("the uv row was announced");
    // Docker comes after the uv row has finished.
    let docker = progress
        .iter()
        .position(|p| p.stage == Stage::Docker)
        .unwrap();
    assert!(uv < docker);
    // Other stages carry no tool name.
    assert!(
        progress
            .iter()
            .filter(|p| p.stage != Stage::Tools)
            .all(|p| p.detail.is_none())
    );
}

#[test]
fn the_scan_view_carries_the_stage_list_and_ends_on_the_last_stage() {
    let home = common::basic_home();
    let engine = home.tool_engine();
    let scan_id = engine
        .start_scan(StartScanArgs {
            mode: ScanMode::Deep,
        })
        .unwrap();
    let started = engine.get_scan(ScanRef { scan_id }).unwrap();
    assert_eq!(started.stages, discover::stages(ScanMode::Deep));
    let view = engine.wait_for_scan(ScanRef { scan_id }).unwrap();
    assert_eq!(view.stages, discover::stages(ScanMode::Deep));
    assert_eq!(view.progress.stage, Stage::LargeFiles);

    let json = serde_json::to_value(&view).unwrap();
    assert_eq!(json["stages"][0], "temp");
    assert_eq!(json["stages"][5], "large_files");
    assert_eq!(json["progress"]["stage"], "large_files");
}
