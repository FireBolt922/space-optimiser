//! Thin Tauri shell. Each command takes one typed payload and forwards it to
//! the engine. No command accepts a path, and this crate holds no file logic
//! (phase 3). Scans and runs happen on engine threads; the window polls
//! `get_scan` and `get_run`.

use std::path::PathBuf;

use space_optimiser_engine::{
    Engine, EngineError,
    guard::{self, GuardError, InstanceLock},
    model::{
        BuildPlanArgs, ConfirmationToken, ExecutePlanArgs, ExecuteStart, InterruptedRun, Plan,
        PlanRef, RevealArgs, RunView, ScanId, ScanRef, ScanView, StartScanArgs,
    },
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

#[tauri::command]
fn reveal(engine: State<'_, Engine>, args: RevealArgs) -> CommandResult<()> {
    engine.reveal(args)
}

/// This computer's folders. Debug builds can point the engine at a copy of
/// the fixture home instead (`scripts/fixture-home.sh`); release builds
/// cannot.
fn engine(audit_dir: PathBuf) -> Engine {
    #[cfg(debug_assertions)]
    if let Some(home) = std::env::var_os("SPACE_OPTIMISER_FIXTURE_HOME") {
        return fixture_engine(PathBuf::from(home));
    }
    Engine::for_this_machine(audit_dir)
}

/// The fixture clock runs two days ahead, so files the script just copied
/// count as old and a file it dated two days ahead counts as new. The audit
/// log goes next to the fixture home, not into the real app data.
#[cfg(debug_assertions)]
fn fixture_engine(home: PathBuf) -> Engine {
    use space_optimiser_engine::Config;
    use space_optimiser_engine::policy::{Context, OwnerRule};
    use std::time::{Duration, SystemTime};

    eprintln!("Fixture mode: scanning {}", home.display());
    let audit_dir = home.with_file_name("audit");
    Engine::new(Config::new(
        move || {
            Ok(Context::for_fixture(&home)
                .with_now(SystemTime::now() + Duration::from_secs(2 * 24 * 60 * 60))
                .with_temp_root(home.join("tmp"), OwnerRule::Root))
        },
        audit_dir,
    ))
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
            reveal
        ])
        .run(tauri::generate_context!())
        .expect("error while running Space Optimiser");
}
