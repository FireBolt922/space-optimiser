//! The commands M6 and M7 add for the desktop shell: "Show in folder", the
//! report export, and the About data (phases 9, 11, and 12). Reveal runs
//! through a test hook here, so no file manager opens.

mod common;

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use common::Home;
use space_optimiser_engine::model::{
    CandidateId, PlanRef, RevealArgs, ScanMode, ScanView, StartScanArgs,
};
use space_optimiser_engine::platform::{self, RevealCommand};
use space_optimiser_engine::{Config, Engine, EngineError};

fn parse<T: serde::de::DeserializeOwned>(json: &str) -> T {
    serde_json::from_str(json).unwrap()
}

type Calls = Arc<Mutex<Vec<RevealCommand>>>;
/// Each scan's mode and the roots it visited.
type Visits = Arc<Mutex<Vec<(ScanMode, Vec<PathBuf>)>>>;

/// `config` with a reveal hook that records each command and succeeds.
fn recording(config: Config) -> (Engine, Calls) {
    let calls: Calls = Arc::default();
    let seen = calls.clone();
    let engine = Engine::new(config.with_reveal(move |cmd| {
        seen.lock().unwrap().push(cmd.clone());
        Ok(())
    }));
    (engine, calls)
}

fn reveal(engine: &Engine, id: CandidateId) -> Result<(), EngineError> {
    engine.reveal(RevealArgs { candidate_id: id })
}

/// Reveals the named candidate and checks the hook got this OS's command
/// for `path`. Without a file manager (Linux with no `xdg-open`) the
/// engine reports that it could not open the folder.
fn assert_reveals(engine: &Engine, calls: &Calls, view: &ScanView, name: &str, path: &Path) {
    let candidate = common::named(view, name);
    assert!(candidate.revealable, "{name}");
    let result = reveal(engine, candidate.id);
    match platform::reveal_command(path) {
        Some(expected) => {
            result.unwrap();
            assert_eq!(calls.lock().unwrap().last(), Some(&expected));
        }
        None => assert_eq!(result, Err(EngineError::RevealFailed)),
    }
}

// Phase 9 test 3: the reveal argv is the fixed OS command.
#[cfg(target_os = "macos")]
#[test]
fn reveal_uses_open_dash_r_on_macos() {
    let target = Path::new("/nowhere/Library/Caches/thing");
    let cmd = platform::reveal_command(target).unwrap();
    assert_eq!(cmd.program, PathBuf::from("/usr/bin/open"));
    assert_eq!(cmd.args, [OsString::from("-R"), OsString::from(target)]);
}

#[cfg(target_os = "linux")]
#[test]
fn reveal_opens_the_parent_folder_with_xdg_open_on_linux() {
    let target = Path::new("/nowhere/.cache/thing");
    let installed = ["/usr/bin/xdg-open", "/bin/xdg-open"]
        .into_iter()
        .map(PathBuf::from)
        .find(|p| fs::metadata(p).is_ok_and(|m| m.is_file()));
    match (platform::reveal_command(target), installed) {
        (Some(cmd), Some(program)) => {
            assert_eq!(cmd.program, program);
            assert_eq!(cmd.args, [OsString::from("/nowhere/.cache")]);
        }
        (None, None) => {}
        (cmd, installed) => panic!("{cmd:?} for {installed:?}"),
    }
}

#[cfg(windows)]
#[test]
fn reveal_selects_the_item_in_explorer_on_windows() {
    let cmd = platform::reveal_command(Path::new(r"\\?\D:\nowhere\Local\Temp\x")).unwrap();
    assert!(
        cmd.program
            .to_string_lossy()
            .to_ascii_lowercase()
            .ends_with(r"\explorer.exe"),
        "{cmd:?}"
    );
    assert!(cmd.program.is_absolute());
    assert_eq!(
        cmd.args,
        [OsString::from(r#"/select,"D:\nowhere\Local\Temp\x""#)]
    );
    let unc = platform::reveal_command(Path::new(r"\\?\UNC\server\share\x")).unwrap();
    assert_eq!(unc.args, [OsString::from(r#"/select,"\\server\share\x""#)]);
}

#[test]
fn reveal_shows_a_temp_entry_and_a_cache_folder() {
    let home = common::basic_home();
    let (engine, calls) = recording(home.config());
    let view = common::scan(&engine);

    assert_reveals(
        &engine,
        &calls,
        &view,
        "old-download.bin",
        &home.path("tmp/old-download.bin"),
    );
    assert_reveals(
        &engine,
        &calls,
        &view,
        common::CHROME_CACHE,
        &home.chrome_cache(),
    );
    // Showing is not deleting.
    assert!(home.path("tmp/old-download.bin").exists());
    assert!(home.chrome_cache().is_dir());
}

#[test]
fn reveal_refuses_an_id_the_scan_did_not_return() {
    let home = common::basic_home();
    let (engine, calls) = recording(home.config());
    assert_eq!(
        reveal(&engine, parse("1")),
        Err(EngineError::UnknownCandidate),
        "no scan yet"
    );
    common::scan(&engine);
    assert_eq!(
        reveal(&engine, parse("999")),
        Err(EngineError::UnknownCandidate)
    );
    assert!(calls.lock().unwrap().is_empty());
}

#[test]
fn reveal_refuses_an_entry_replaced_after_the_scan() {
    let home = common::basic_home();
    let (engine, calls) = recording(home.config());
    let view = common::scan(&engine);
    let replaced = common::named(&view, "old-download.bin").id;
    let removed = common::named(&view, "old-build").id;

    fs::rename(home.path("tmp/old-download.bin"), home.path("moved.bin")).unwrap();
    home.write("tmp/old-download.bin", b"a different file");
    fs::rename(home.path("tmp/old-build"), home.path("moved-build")).unwrap();

    assert_eq!(reveal(&engine, replaced), Err(EngineError::CandidateGone));
    assert_eq!(reveal(&engine, removed), Err(EngineError::CandidateGone));
    assert!(calls.lock().unwrap().is_empty());
}

#[test]
fn a_failing_file_manager_is_reported_as_reveal_failed() {
    let home = common::basic_home();
    let engine = Engine::new(
        home.config()
            .with_reveal(|_| Err(std::io::Error::other("no file manager"))),
    );
    let view = common::scan(&engine);
    assert_eq!(
        reveal(&engine, common::named(&view, "old-build").id),
        Err(EngineError::RevealFailed)
    );
    assert!(home.path("tmp/old-build").is_dir());
}

#[cfg(unix)]
#[test]
fn docker_build_cache_is_not_revealable() {
    use space_optimiser_engine::policy::CatalogId;
    use std::time::Duration;

    let home = common::basic_home();
    home.fake_tool(
        "docker",
        r#"case "$1" in
  context) echo '[{"Name":"default","Endpoints":{"docker":{"Host":"unix:///run/docker.sock"}}}]' ;;
  version) echo 'Client: Docker Engine - Community' ;;
  info) echo 'Server: ok' ;;
  system) echo '{"Type":"Build Cache","Reclaimable":"1.5GB"}' ;;
  *) exit 2 ;;
esac"#,
    );
    let (bin, work) = (home.tool_dir(), home.work_dir());
    let (engine, calls) = recording(home.config_with(move |ctx| {
        ctx.with_tool_dirs(vec![bin.clone()], Vec::new())
            .with_work_dir(work.clone())
            .with_tool_timeouts(Duration::from_secs(5), Duration::from_secs(10))
    }));
    let view = common::deep_scan(&engine);

    let docker = view
        .candidates
        .iter()
        .find(|c| c.catalog_id == CatalogId::DockerBuildCache)
        .expect("the fake docker offers its build cache");
    assert!(!docker.revealable);
    assert_eq!(reveal(&engine, docker.id), Err(EngineError::NotActionable));
    assert!(calls.lock().unwrap().is_empty());
    assert!(
        view.candidates
            .iter()
            .filter(|c| c.catalog_id != CatalogId::DockerBuildCache)
            .all(|c| c.revealable)
    );
}

/// An engine over `home` that saves reports into `exports`.
fn exporting(home: &Home, exports: &Path) -> Engine {
    Engine::new(home.config().with_export_dir(exports.to_path_buf()))
}

#[test]
fn export_writes_a_new_text_file_each_time() {
    let home = common::basic_home();
    let exports = home.root.parent().unwrap().join("exports");
    fs::create_dir(&exports).unwrap();
    let engine = exporting(&home, &exports);
    let report = common::clean(&engine, &["old-download.bin", "old-build"]);
    let args = PlanRef {
        plan_id: report.plan_id,
    };

    let first = engine.export_report(args).unwrap();
    let first_path = PathBuf::from(&first.path);
    assert_eq!(first_path.parent(), Some(exports.as_path()));
    assert_eq!(first_path.file_name().unwrap(), first.file_name.as_str());
    let name = first.file_name.as_str();
    let stamp = name
        .strip_prefix("space-optimiser-report-")
        .and_then(|n| n.strip_suffix(".txt"))
        .unwrap_or_else(|| panic!("{name}"));
    // YYYY-MM-DD-HHMMSS, possibly with a -N suffix.
    assert!(stamp.len() >= 17, "{name}");
    assert!(
        stamp[..17]
            .chars()
            .enumerate()
            .all(|(i, c)| if [4, 7, 10].contains(&i) {
                c == '-'
            } else {
                c.is_ascii_digit()
            }),
        "{name}"
    );

    let text = fs::read_to_string(&first_path).unwrap();
    for needle in [
        "old-download.bin",
        "old-build",
        "Removed",
        "cannot be undone",
        "does not upload a list of your files",
        env!("CARGO_PKG_VERSION"),
    ] {
        assert!(text.contains(needle), "missing {needle:?} in\n{text}");
    }
    assert!(!text.contains("will not delete personal data"));
    assert!(!text.contains(&home.root.to_string_lossy().into_owned()));

    let second = engine.export_report(args).unwrap();
    assert_ne!(second.path, first.path);
    assert_eq!(fs::read_to_string(&first_path).unwrap(), text);
    assert!(Path::new(&second.path).is_file());
}

#[test]
fn export_needs_a_known_finished_run() {
    let home = common::basic_home();
    let engine = home.engine();
    assert_eq!(
        engine
            .export_report(parse(r#"{"plan_id":42}"#))
            .unwrap_err(),
        EngineError::UnknownPlan
    );
}

#[test]
fn export_waits_for_the_run_to_finish() {
    use std::sync::mpsc;

    let home = common::basic_home();
    let exports = home.root.parent().unwrap().join("exports");
    fs::create_dir(&exports).unwrap();
    let (release, gate) = mpsc::channel::<()>();
    let gate = Mutex::new(gate);
    let engine = Engine::new(
        home.config()
            .with_export_dir(exports.clone())
            .with_after_item(move |_, _| {
                let _ = gate.lock().unwrap().recv();
            }),
    );
    let view = common::scan(&engine);
    let plan = common::plan(&engine, &view, &["old-download.bin"]);
    let token = common::confirm(&engine, &plan);
    let args = PlanRef {
        plan_id: plan.plan_id,
    };
    engine
        .execute_plan(space_optimiser_engine::model::ExecutePlanArgs {
            plan_id: plan.plan_id,
            confirmation_token: token,
            keep_log_in_memory: false,
        })
        .unwrap();

    assert_eq!(
        engine.export_report(args).unwrap_err(),
        EngineError::RunNotFinished
    );
    release.send(()).unwrap();
    engine.wait_for_run(args).unwrap();
    assert!(engine.export_report(args).is_ok());
    assert_eq!(fs::read_dir(&exports).unwrap().count(), 1);
}

// Without a Downloads folder the report goes next to the audit folder; the
// engine never creates Downloads.
#[cfg(unix)]
#[test]
fn export_falls_back_to_the_app_folder_without_downloads() {
    let home = common::basic_home();
    assert!(!home.path("Downloads").exists());
    let engine = home.engine();
    let fallback = home.audit_dir().with_file_name("exports");
    assert_eq!(
        engine.app_info().export_folder,
        fallback.to_string_lossy().into_owned()
    );

    let report = common::clean(&engine, &["old-build"]);
    let saved = engine
        .export_report(PlanRef {
            plan_id: report.plan_id,
        })
        .unwrap();
    assert_eq!(Path::new(&saved.path).parent(), Some(fallback.as_path()));
    assert!(!home.path("Downloads").exists());
}

#[cfg(unix)]
#[test]
fn export_uses_the_downloads_folder_when_there_is_one() {
    let home = common::basic_home();
    let downloads = home.mkdir("Downloads");
    let engine = home.engine();
    assert_eq!(
        engine.app_info().export_folder,
        downloads.to_string_lossy().into_owned()
    );
}

#[test]
fn app_info_names_the_version_and_the_audit_log() {
    let home = common::basic_home();
    let exports = home.root.parent().unwrap().join("exports");
    let info = exporting(&home, &exports).app_info();
    assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
    assert_eq!(
        info.platform,
        space_optimiser_engine::model::Platform::CURRENT
    );
    assert_eq!(
        PathBuf::from(&info.audit_log_path),
        home.audit_dir().join("audit.jsonl")
    );
    assert_eq!(PathBuf::from(&info.export_folder), exports);
    let json = serde_json::to_value(&info).unwrap();
    for key in ["version", "platform", "audit_log_path", "export_folder"] {
        assert!(json.get(key).is_some(), "{key}");
    }
}

// Phase 11: the debug log of what a scan walked.
#[test]
fn the_visited_roots_hook_lists_the_outermost_folders_a_scan_read() {
    let home = common::basic_home();
    let seen: Visits = Arc::default();
    let out = seen.clone();
    let engine = Engine::new(home.config().with_visited_roots(move |mode, roots| {
        out.lock().unwrap().push((mode, roots.to_vec()));
    }));

    common::scan(&engine);

    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    let (mode, roots) = &seen[0];
    assert_eq!(*mode, ScanMode::Normal);
    assert!(roots.contains(&home.path("tmp")), "{roots:#?}");
    assert!(
        roots.iter().all(|r| !r.starts_with(home.path("Documents"))),
        "{roots:#?}"
    );
    for (i, a) in roots.iter().enumerate() {
        for (j, b) in roots.iter().enumerate() {
            assert!(i == j || !b.starts_with(a), "{a:?} contains {b:?}");
        }
    }
}

#[test]
fn a_scan_without_the_hook_still_runs() {
    let home = common::basic_home();
    let engine = home.engine();
    let scan_id = engine
        .start_scan(StartScanArgs {
            mode: ScanMode::Normal,
        })
        .unwrap();
    assert!(
        engine
            .wait_for_scan(space_optimiser_engine::model::ScanRef { scan_id })
            .is_ok()
    );
}

#[test]
fn new_payloads_reject_unknown_fields() {
    assert!(serde_json::from_str::<PlanRef>(r#"{"plan_id":1,"path":"/"}"#).is_err());
    assert!(serde_json::from_str::<RevealArgs>(r#"{"candidate_id":1,"path":"/"}"#).is_err());
    assert!(serde_json::from_str::<RevealArgs>(r#"{"path":"/"}"#).is_err());
}

#[test]
fn new_errors_reach_the_ui_as_a_kind_and_a_message() {
    for (error, kind) in [
        (EngineError::RevealFailed, "reveal_failed"),
        (EngineError::CandidateGone, "candidate_gone"),
        (EngineError::RunNotFinished, "run_not_finished"),
        (
            EngineError::ExportFailed("disk full".into()),
            "export_failed",
        ),
        (EngineError::RunningElevated, "running_elevated"),
    ] {
        let json = serde_json::to_value(&error).unwrap();
        assert_eq!(json["kind"], kind);
        assert!(!json["message"].as_str().unwrap().is_empty());
    }
}
