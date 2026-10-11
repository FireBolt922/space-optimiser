//! Thin Tauri shell. Each command takes one typed payload and forwards it to
//! the engine. No command accepts a path, and this crate holds no file logic
//! (phase 3). Scans and runs happen on engine threads; the window polls
//! `get_scan` and `get_run`. Commands that start a process or write a file
//! (`reveal`, `export_report`) run off the main thread.

use std::path::PathBuf;

use space_optimiser_engine::{
    Config, Engine, EngineError,
    guard::{self, GuardError, InstanceLock},
    model::{
        AppInfo, BuildPlanArgs, ConfirmationToken, ExecutePlanArgs, ExecuteStart, ExportedReport,
        InterruptedRun, Plan, PlanRef, RevealArgs, RunView, ScanId, ScanRef, ScanView,
        StartScanArgs,
    },
    platform,
};
use tauri::{Manager, State};

type CommandResult<T> = Result<T, EngineError>;

#[tauri::command]
fn start_scan(engine: State<'_, Engine>, args: StartScanArgs) -> CommandResult<ScanId> {
    engine.start_scan(args)
}

#[tauri::command]
fn cancel_scan(engine: State<'_, Engine>, args: ScanRef) -> CommandResult<()> {
    engine.cancel_scan(args)
}

#[tauri::command]
fn get_scan(engine: State<'_, Engine>, args: ScanRef) -> CommandResult<ScanView> {
    engine.get_scan(args)
}

#[tauri::command]
fn build_plan(engine: State<'_, Engine>, args: BuildPlanArgs) -> CommandResult<Plan> {
    engine.build_plan(args)
}

#[tauri::command]
fn confirm_plan(engine: State<'_, Engine>, args: PlanRef) -> CommandResult<ConfirmationToken> {
    engine.confirm_plan(args)
}

#[tauri::command]
fn execute_plan(engine: State<'_, Engine>, args: ExecutePlanArgs) -> CommandResult<ExecuteStart> {
    engine.execute_plan(args)
}

#[tauri::command]
fn get_run(engine: State<'_, Engine>, args: PlanRef) -> CommandResult<RunView> {
    engine.get_run(args)
}

#[tauri::command]
fn stop_run(engine: State<'_, Engine>, args: PlanRef) -> CommandResult<()> {
    engine.stop_run(args)
}

#[tauri::command]
fn interrupted_run(engine: State<'_, Engine>) -> Option<InterruptedRun> {
    engine.interrupted_run()
}

/// Waits up to a few seconds for the file manager, so it runs off the main
/// thread.
#[tauri::command(async)]
fn reveal(engine: State<'_, Engine>, args: RevealArgs) -> CommandResult<()> {
    engine.reveal(args)
}

#[tauri::command(async)]
fn export_report(engine: State<'_, Engine>, args: PlanRef) -> CommandResult<ExportedReport> {
    engine.export_report(args)
}

#[tauri::command]
fn app_info(engine: State<'_, Engine>) -> AppInfo {
    engine.app_info()
}

/// This computer's folders. Debug builds can point the engine at a copy of
/// the fixture home instead (`scripts/fixture-home.sh`); release builds
/// cannot.
fn engine(audit_dir: PathBuf) -> Engine {
    #[cfg(debug_assertions)]
    if let Some(home) = std::env::var_os("SPACE_OPTIMISER_FIXTURE_HOME") {
        return Engine::new(debug_roots(fixture_config(PathBuf::from(home))));
    }
    let config = Config::new(platform::resolve_context, audit_dir);
    #[cfg(debug_assertions)]
    let config = debug_roots(config);
    Engine::new(config)
}

/// With `SPACE_OPTIMISER_DEBUG_ROOTS=1`, a debug build prints the
/// outermost folders each scan read (phase 11).
#[cfg(debug_assertions)]
fn debug_roots(config: Config) -> Config {
    use space_optimiser_engine::model::ScanMode;

    if std::env::var_os("SPACE_OPTIMISER_DEBUG_ROOTS").is_none_or(|v| v != "1") {
        return config;
    }
    config.with_visited_roots(|mode, roots| {
        let mode = match mode {
            ScanMode::Normal => "normal",
            ScanMode::Deep => "deep",
        };
        for root in roots {
            eprintln!(
                "space-optimiser debug: {mode} scan visited {}",
                root.display()
            );
        }
    })
}

/// The fixture clock runs two days ahead, so files the script just copied
/// count as old and a file it dated two days ahead counts as new. Running
/// browsers are read from this computer, so an open browser shows its
/// fixture cache as "quit first". No tools are found, so nothing runs
/// against this computer's npm, pip, or Docker: tool rows show only their
/// fixture default folders, as "not sure". The audit log and exported
/// reports go next to the fixture home, not into the real app data or
/// Downloads. A folder without the fixture marker is refused, so this mode
/// cannot be pointed at a real home.
#[cfg(debug_assertions)]
fn fixture_config(home: PathBuf) -> Config {
    use space_optimiser_engine::policy::{Context, OwnerRule};
    use std::time::{Duration, SystemTime};

    if !guard::is_fixture_home(&home) {
        eprintln!(
            "{} is not a fixture home copy (no .space-optimiser-fixture file). \
             Make one with scripts/fixture-home.sh.",
            home.display()
        );
        std::process::exit(1);
    }
    eprintln!("Fixture mode: scanning {}", home.display());
    let audit_dir = home.with_file_name("audit");
    let export_dir = home.with_file_name("exports");
    if let Err(e) = std::fs::create_dir_all(&export_dir) {
        eprintln!("Could not create {}: {e}", export_dir.display());
    }
    Config::new(
        move || {
            Ok(Context::for_fixture(&home)
                .with_now(SystemTime::now() + Duration::from_secs(2 * 24 * 60 * 60))
                .with_temp_root(home.join("tmp"), OwnerRule::Root)
                .with_system_processes())
        },
        audit_dir,
    )
    .with_export_dir(export_dir)
}

pub fn run() {
    if let Some(reason) = guard::refusal_reason() {
        eprintln!("{reason}");
        std::process::exit(1);
    }

    tauri::Builder::default()
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            match InstanceLock::acquire(&dir) {
                Ok(lock) => {
                    app.manage(lock);
                }
                Err(GuardError::AlreadyRunning) => {
                    eprintln!("Space Optimiser is already running.");
                    std::process::exit(1);
                }
                Err(e) => return Err(e.into()),
            }
            app.manage(engine(dir.join("audit")));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            start_scan,
            cancel_scan,
            get_scan,
            build_plan,
            confirm_plan,
            execute_plan,
            get_run,
            stop_run,
            interrupted_run,
            reveal,
            export_report,
            app_info
        ])
        .run(tauri::generate_context!())
        .expect("error while running Space Optimiser");
}
