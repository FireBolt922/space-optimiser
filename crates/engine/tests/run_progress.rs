//! What a running cleanup shows while it runs (v0.3 usability round): the
//! item in progress, how long it has run, the lines a tool prints, the items
//! already finished, and counts. Fake tools are shell scripts, so the tool
//! tests are Unix only.

mod common;

use std::time::{Duration, Instant};

use space_optimiser_engine::model::{ExecutePlanArgs, ExecuteStart, PlanRef, RunView};

fn start(engine: &space_optimiser_engine::Engine, names: &[&str], deep: bool) -> PlanRef {
    let view = if deep {
        common::deep_scan(engine)
    } else {
        common::scan(engine)
    };
    let plan = common::plan(engine, &view, names);
    let token = common::confirm(engine, &plan);
    let started = engine
        .execute_plan(ExecutePlanArgs {
            plan_id: plan.plan_id,
            confirmation_token: token,
            keep_log_in_memory: false,
        })
        .unwrap();
    assert_eq!(started, ExecuteStart::Started);
    PlanRef {
        plan_id: plan.plan_id,
    }
}

/// Polls until `done` holds, or panics after 20 s.
fn poll_until(
    engine: &space_optimiser_engine::Engine,
    plan: PlanRef,
    done: impl Fn(&RunView) -> bool,
) -> RunView {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let view = engine.get_run(plan).unwrap();
        if done(&view) {
            return view;
        }
        assert!(Instant::now() < deadline, "gave up waiting: {view:?}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_finished_run_lists_each_item_with_counts_and_no_current_item() {
    let home = common::basic_home();
    let engine = home.engine();
    let plan = start(&engine, &["old-build", "old-download.bin"], false);
    let report = engine.wait_for_run(plan).unwrap();
    assert_eq!(report.items.len(), 2);
    let run = engine.get_run(plan).unwrap();
    assert!(run.current.is_none());
    assert_eq!(run.counts.removed, 2);
    assert_eq!(run.counts.skipped, 0);
    assert_eq!(run.counts.failed, 0);
    let mut names: Vec<_> = run.recent.iter().map(|i| i.display_name.as_str()).collect();
    names.sort();
    assert_eq!(names, ["old-build", "old-download.bin"]);

    let json = serde_json::to_value(&run).unwrap();
    assert!(json["elapsed_ms"].is_u64());
    assert!(json["current"].is_null());
    assert_eq!(json["counts"]["removed"], 2);
}

#[cfg(unix)]
mod tools {
    use super::*;
    use space_optimiser_engine::model::SkipCause;

    #[test]
    fn a_running_tool_shows_the_command_and_what_it_prints() {
        let home = common::basic_home();
        home.write(".cache/uv/archive-v0/fixture/file", b"wheel");
        home.fake_tool(
            "uv",
            r#"case "$1 $2" in
  "cache dir") echo "$HOME/.cache/uv" ;;
  *) printf 'Clearing cache at: %s\n' "$HOME/.cache/uv"; sleep 2; printf '\033[32mRemoved 3 files\033[0m\n' ;;
esac"#,
        );
        let engine = home.tool_engine();
        let plan = start(&engine, &["~/.cache/uv"], true);

        let running = poll_until(&engine, plan, |v| {
            v.current
                .as_ref()
                .is_some_and(|c| c.output.iter().any(|l| l.starts_with("Clearing cache at:")))
        });
        let current = running.current.unwrap();
        assert_eq!(current.display_name, "~/.cache/uv");
        assert_eq!(current.index, 0);
        assert!(
            current.output[0].starts_with("Running ")
                && current.output[0].ends_with("uv cache clean"),
            "{:?}",
            current.output
        );
        assert!(running.recent.is_empty());

        let finished = poll_until(&engine, plan, |v| v.report.is_some());
        let item = &finished.recent[0];
        assert_eq!(item.display_name, "~/.cache/uv");
        assert!(finished.current.is_none());
        assert_eq!(finished.counts.removed, 1);
    }

    #[test]
    fn a_busy_uv_ends_skipped_with_a_hint_about_uvx_and_uv_run() {
        let home = common::basic_home();
        home.write(".cache/uv/archive-v0/fixture/file", b"wheel");
        home.fake_tool(
            "uv",
            r#"case "$1 $2" in
  "cache dir") echo "$HOME/.cache/uv" ;;
  *) echo 'Cache is currently in-use, waiting for other uv processes to finish' >&2; sleep 30 ;;
esac"#,
        );
        let engine = home.tool_engine();
        let plan = start(&engine, &["~/.cache/uv"], true);
        let finished = poll_until(&engine, plan, |v| v.report.is_some());
        let item = &finished.recent[0];
        assert_eq!(item.reason, Some(SkipCause::ToolBusy));
        let hint = item.hint.expect("a busy uv has a hint");
        assert!(hint.contains("uvx") && hint.contains("uv run"), "{hint}");
        assert_eq!(finished.counts.skipped, 1);
    }
}
