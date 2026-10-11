//! Executing Deep Scan plans (phase 6 with phase 8): package caches are
//! emptied through the same handle-relative executor on every OS, old logs
//! are removed one entry at a time, and tool rows run their fixed command
//! with the tool found again. Tool rows are Unix only here: the fake tools
//! are shell scripts.

mod common;

use std::fs;
#[cfg(target_os = "macos")]
use std::time::{Duration, SystemTime};

use common::basic_home;
use space_optimiser_engine::Engine;
use space_optimiser_engine::model::{Outcome, Report, ReportItem, SkipCause};

fn item<'a>(report: &'a Report, name: &str) -> &'a ReportItem {
    report
        .items
        .iter()
        .find(|i| i.display_name == name)
        .unwrap_or_else(|| panic!("no report item {name}"))
}

fn clean_deep(engine: &Engine, names: &[&str]) -> Report {
    let view = common::deep_scan(engine);
    let plan = common::plan(engine, &view, names);
    let token = common::confirm(engine, &plan);
    common::execute(engine, &plan, token)
}

#[test]
fn the_cargo_registry_cache_is_emptied_and_cargo_bin_is_kept() {
    let home = basic_home();
    let report = clean_deep(&home.engine(), &["~/.cargo/registry/cache"]);

    let done = item(&report, "~/.cargo/registry/cache");
    assert_eq!(done.outcome, Outcome::Removed, "{done:?}");
    assert!(done.bytes_removed > 0);
    let cache = home.path(".cargo/registry/cache");
    assert!(cache.is_dir());
    assert_eq!(fs::read_dir(&cache).unwrap().count(), 0);
    assert!(home.path(".cargo/bin/cargo-fixture").exists());
    assert!(home.path(".cargo/env").exists());
    assert!(
        home.path(".cargo/git/checkouts")
            .read_dir()
            .unwrap()
            .count()
            > 0
    );
}

// Packages carry project files by design; inside a fixed package root that
// does not stop the clean.
#[test]
fn git_checkouts_and_npx_packages_are_emptied_despite_project_files() {
    let home = basic_home();
    home.mkdir(".cargo/git/checkouts/fixture-0123456789abcdef/abc1234/.git");
    let (npx, npx_dir, npm_kept) = if cfg!(windows) {
        (
            "~/AppData/Local/npm-cache/_npx",
            "AppData/Local/npm-cache/_npx",
            "AppData/Local/npm-cache",
        )
    } else {
        ("~/.npm/_npx", ".npm/_npx", ".npm/_cacache")
    };
    let report = clean_deep(&home.engine(), &["~/.cargo/git/checkouts", npx]);

    for name in ["~/.cargo/git/checkouts", npx] {
        assert_eq!(item(&report, name).outcome, Outcome::Removed, "{name}");
    }
    assert_eq!(fs::read_dir(home.path(npx_dir)).unwrap().count(), 0);
    assert!(home.path(npm_kept).is_dir());
}

#[test]
fn the_nuget_download_cache_is_emptied() {
    let home = basic_home();
    let (name, folder) = if cfg!(windows) {
        (
            "~/AppData/Local/NuGet/v3-cache",
            "AppData/Local/NuGet/v3-cache",
        )
    } else {
        (
            "~/.local/share/NuGet/http-cache",
            ".local/share/NuGet/http-cache",
        )
    };
    let report = clean_deep(&home.engine(), &[name]);

    let done = item(&report, name);
    assert_eq!(done.outcome, Outcome::Removed, "{done:?}");
    assert!(done.bytes_removed > 0);
    let cache = home.path(folder);
    assert!(cache.is_dir(), "the cache folder itself stays");
    assert_eq!(fs::read_dir(&cache).unwrap().count(), 0);
}

#[test]
fn a_file_added_to_a_package_cache_after_the_scan_keeps_the_cache() {
    let home = basic_home();
    let engine = home.engine();
    let view = common::deep_scan(&engine);
    let plan = common::plan(&engine, &view, &["~/.gradle/caches"]);
    let token = common::confirm(&engine, &plan);
    home.write(".gradle/caches/new-download.jar", b"arrived after the scan");
    let report = common::execute(&engine, &plan, token);

    let done = item(&report, "~/.gradle/caches");
    assert_eq!(done.reason, Some(SkipCause::ChangedSinceScan));
    assert!(home.path(".gradle/caches/new-download.jar").exists());
    assert!(home.path(".gradle/caches/modules-2").exists());
}

#[cfg(target_os = "macos")]
#[test]
fn old_logs_are_removed_and_other_files_stay() {
    let home = basic_home();
    let later = SystemTime::now() + Duration::from_secs(60 * 86_400);
    let engine = Engine::new(home.config_with(move |ctx| ctx.with_now(later)));
    let report = clean_deep(&engine, &["old.log", "FixtureApp"]);

    assert_eq!(item(&report, "old.log").outcome, Outcome::Removed);
    assert_eq!(item(&report, "FixtureApp").outcome, Outcome::Removed);
    assert!(!home.path("Library/Logs/old.log").exists());
    assert!(!home.path("Library/Logs/FixtureApp").exists());
    assert!(home.path("Library/Logs/settings.plist").exists());
    assert!(home.path("Library/Logs/Mixed/notes.txt").exists());
    assert!(home.path("Library/Logs/DiagnosticReports").exists());
    assert!(home.path("Library/Logs").is_dir());
}

#[cfg(target_os = "macos")]
#[test]
fn a_log_folder_that_gains_a_document_after_the_scan_is_kept() {
    let home = basic_home();
    let later = SystemTime::now() + Duration::from_secs(60 * 86_400);
    let engine = Engine::new(home.config_with(move |ctx| ctx.with_now(later)));
    let view = common::deep_scan(&engine);
    let plan = common::plan(&engine, &view, &["FixtureApp"]);
    let token = common::confirm(&engine, &plan);
    home.write("Library/Logs/FixtureApp/report.pdf", b"mine");
    let report = common::execute(&engine, &plan, token);

    assert_eq!(item(&report, "FixtureApp").outcome, Outcome::Skipped);
    assert!(home.path("Library/Logs/FixtureApp/report.pdf").exists());
    assert!(home.path("Library/Logs/FixtureApp/fixture.log").exists());
}

#[cfg(unix)]
mod tools {
    use super::*;
    use common::Home;
    use space_optimiser_engine::EngineError;
    use space_optimiser_engine::model::BuildPlanArgs;
    use std::time::Duration;

    fn audit_log(home: &Home) -> String {
        fs::read_to_string(home.audit_dir().join("audit.jsonl")).unwrap()
    }

    const UV: &str = r#"case "$1 $2" in
      "cache dir") echo "$HOME/.cache/uv" ;;
      "cache clean") printf '%s\n' "$*" > "$HOME/../uv-args"; pwd >> "$HOME/../uv-args" ;;
      *) exit 2 ;;
    esac"#;

    fn uv_home() -> Home {
        let home = basic_home();
        home.write(".cache/uv/archive-v0/fixture/file", b"wheel");
        home.fake_tool("uv", UV);
        home
    }

    // Phase 8 test 7 and phase 6 test 16: the clean command is exactly the
    // fixed argv, run in the app's empty work folder.
    #[test]
    fn uv_runs_exactly_cache_clean_in_the_work_folder() {
        let home = uv_home();
        let report = clean_deep(&home.tool_engine(), &["~/.cache/uv"]);

        let done = item(&report, "~/.cache/uv");
        assert_eq!(done.outcome, Outcome::Ran, "{done:?}");
        assert_eq!(done.bytes_removed, 0);
        assert_eq!(
            done.recovery_text,
            Some("The next install will download packages again.")
        );
        let ran = fs::read_to_string(home.root.parent().unwrap().join("uv-args")).unwrap();
        let mut lines = ran.lines();
        assert_eq!(lines.next(), Some("cache clean"));
        assert_eq!(
            fs::canonicalize(lines.next().unwrap()).unwrap(),
            fs::canonicalize(home.work_dir()).unwrap()
        );
        let log = audit_log(&home);
        assert!(log.contains("uv cache clean\""), "{log}");
        assert!(log.contains("\"outcome\":\"ran\""), "{log}");
    }

    // A cache folder the tool reports differently at clean time (a changed
    // `uv.toml`) is never cleaned: the folder is asked for again.
    #[test]
    fn a_tool_whose_cache_moved_after_the_scan_is_not_run() {
        let home = basic_home();
        home.write(".cache/uv/archive-v0/fixture/file", b"wheel");
        home.write("Projects/uv-elsewhere/file", b"mine");
        let parent = home.root.parent().unwrap();
        fs::write(
            parent.join("uv-dir"),
            home.path(".cache/uv").to_str().unwrap(),
        )
        .unwrap();
        home.fake_tool(
            "uv",
            r#"case "$1 $2" in
  "cache dir") cat "$HOME/../uv-dir" ;;
  "cache clean") touch "$HOME/../uv-cleaned" ;;
  *) exit 2 ;;
esac"#,
        );
        let engine = home.tool_engine();
        let view = common::deep_scan(&engine);
        let plan = common::plan(&engine, &view, &["~/.cache/uv"]);
        let token = common::confirm(&engine, &plan);
        let moved = home.path("Projects/uv-elsewhere");
        fs::write(parent.join("uv-dir"), moved.to_str().unwrap()).unwrap();
        let report = common::execute(&engine, &plan, token);

        assert_eq!(
            item(&report, "~/.cache/uv").reason,
            Some(SkipCause::ChangedSinceScan)
        );
        assert!(!parent.join("uv-cleaned").exists());
    }

    #[test]
    fn a_tool_replaced_after_the_scan_is_not_run() {
        let home = uv_home();
        let engine = home.tool_engine();
        let view = common::deep_scan(&engine);
        let plan = common::plan(&engine, &view, &["~/.cache/uv"]);
        let token = common::confirm(&engine, &plan);
        let uv = home.tool_dir().join("uv");
        fs::rename(&uv, home.root.parent().unwrap().join("old-uv")).unwrap();
        home.fake_tool("uv", UV);
        let report = common::execute(&engine, &plan, token);

        assert_eq!(
            item(&report, "~/.cache/uv").reason,
            Some(SkipCause::ChangedSinceScan)
        );
        assert!(!home.root.parent().unwrap().join("uv-args").exists());
    }

    // Phase 6 item 6: a uv waiting for its lock is reported busy, never forced.
    #[test]
    fn a_busy_uv_is_skipped_not_forced() {
        let home = basic_home();
        home.write(".cache/uv/archive-v0/fixture/file", b"wheel");
        home.fake_tool(
            "uv",
            r#"case "$1 $2" in
      "cache dir") echo "$HOME/.cache/uv" ;;
      *) echo 'Cache is currently in-use, waiting for other uv processes to finish' >&2; sleep 30 ;;
    esac"#,
        );
        let started = std::time::Instant::now();
        let report = clean_deep(&home.tool_engine(), &["~/.cache/uv"]);
        assert!(started.elapsed() < Duration::from_secs(20));
        assert_eq!(
            item(&report, "~/.cache/uv").reason,
            Some(SkipCause::ToolBusy)
        );
    }

    #[test]
    fn a_failing_tool_is_reported_with_its_error() {
        let home = basic_home();
        home.write(".cache/uv/archive-v0/fixture/file", b"wheel");
        home.fake_tool(
            "uv",
            r#"case "$1 $2" in
      "cache dir") echo "$HOME/.cache/uv" ;;
      *) echo 'error: permission denied' >&2; exit 2 ;;
    esac"#,
        );
        let report = clean_deep(&home.tool_engine(), &["~/.cache/uv"]);
        let done = item(&report, "~/.cache/uv");
        assert_eq!(done.outcome, Outcome::Failed);
        assert_eq!(done.detail.as_deref(), Some("error: permission denied"));
    }

    /// A fake `docker` whose endpoint is read from a file, so a test can move
    /// it after the scan.
    fn docker_home() -> Home {
        let home = basic_home();
        let parent = home.root.parent().unwrap();
        fs::write(parent.join("docker-host"), "unix:///run/docker.sock").unwrap();
        home.fake_tool(
            "docker",
            r#"case "$1" in
      context) printf '[{"Name":"default","Endpoints":{"docker":{"Host":"%s"}}}]' "$(cat "$HOME/../docker-host")" ;;
      version) echo 'Client: Docker Engine - Community' ;;
      info) echo 'Server: ok' ;;
      system) echo '{"Type":"Build Cache","Reclaimable":"1.5GB"}' ;;
      builder) printf '%s\n' "$*" > "$HOME/../docker-args" ;;
      *) exit 2 ;;
    esac"#,
        );
        home
    }

    #[test]
    fn docker_prunes_the_build_cache_with_the_fixed_argv() {
        let home = docker_home();
        let report = clean_deep(&home.tool_engine(), &["Docker build cache"]);
        assert_eq!(item(&report, "Docker build cache").outcome, Outcome::Ran);
        let args = fs::read_to_string(home.root.parent().unwrap().join("docker-args")).unwrap();
        assert_eq!(args.trim(), "builder prune -f");
    }

    // Phase 8 item 4: the context is checked again right before the prune.
    #[test]
    fn docker_pointing_elsewhere_after_the_scan_is_not_pruned() {
        let home = docker_home();
        let engine = home.tool_engine();
        let view = common::deep_scan(&engine);
        let plan = common::plan(&engine, &view, &["Docker build cache"]);
        let token = common::confirm(&engine, &plan);
        let parent = home.root.parent().unwrap();
        fs::write(parent.join("docker-host"), "tcp://10.0.0.5:2376").unwrap();
        let report = common::execute(&engine, &plan, token);

        assert_eq!(
            item(&report, "Docker build cache").reason,
            Some(SkipCause::DockerNotLocal)
        );
        assert!(!parent.join("docker-args").exists());
    }

    #[test]
    fn a_default_cache_shown_without_its_tool_cannot_be_planned() {
        let home = basic_home();
        let default = if cfg!(target_os = "macos") {
            "Library/Caches/pip/http/fixture"
        } else {
            ".cache/pip/http/fixture"
        };
        home.write(default, b"wheel");
        let engine = home.tool_engine();
        let view = common::deep_scan(&engine);
        let pip = view
            .candidates
            .iter()
            .find(|c| c.catalog_id == space_optimiser_engine::policy::CatalogId::PkgPipCache)
            .unwrap();
        let result = engine.build_plan(BuildPlanArgs {
            scan_id: view.scan_id,
            candidate_ids: vec![pip.id],
        });
        assert_eq!(result.unwrap_err(), EngineError::NotActionable);
    }

    #[test]
    fn a_large_file_cannot_be_planned() {
        let home = basic_home();
        home.write("Projects/disk-image.iso", &vec![7u8; 2_000_000]);
        let engine = Engine::new(home.config_with(|ctx| ctx.with_large_file_min(1_000_000)));
        let view = common::deep_scan(&engine);
        let large = common::named(&view, "~/Projects/disk-image.iso");
        let result = engine.build_plan(BuildPlanArgs {
            scan_id: view.scan_id,
            candidate_ids: vec![large.id],
        });
        assert_eq!(result.unwrap_err(), EngineError::NotActionable);
        assert!(home.path("Projects/disk-image.iso").exists());
    }
}
