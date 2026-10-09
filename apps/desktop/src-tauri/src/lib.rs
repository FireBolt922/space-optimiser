//! Thin Tauri shell. Each command takes one typed payload and forwards it to
//! the engine. No command accepts a path, and this crate holds no file logic
//! (phase 3).

use space_optimiser_engine::{
    self as engine,
    guard::{self, GuardError, InstanceLock},
    model::{
        BuildPlanArgs, Candidate, ConfirmationToken, ExecutePlanArgs, Plan, PlanRef, Report,
        RevealArgs, ScanId, ScanRef, StartScanArgs,
    },
};
use tauri::Manager;

type CommandResult<T> = Result<T, String>;

fn forward<T>(result: engine::Result<T>) -> CommandResult<T> {
    result.map_err(|e| e.to_string())
}

#[tauri::command]
fn start_scan(args: StartScanArgs) -> CommandResult<ScanId> {
    forward(engine::start_scan(args))
}

#[tauri::command]
fn cancel_scan(args: ScanRef) -> CommandResult<()> {
    forward(engine::cancel_scan(args))
}

#[tauri::command]
fn get_scan(args: ScanRef) -> CommandResult<Vec<Candidate>> {
    forward(engine::get_scan(args))
}

#[tauri::command]
fn build_plan(args: BuildPlanArgs) -> CommandResult<Plan> {
    forward(engine::build_plan(args))
}

#[tauri::command]
fn confirm_plan(args: PlanRef) -> CommandResult<ConfirmationToken> {
    forward(engine::confirm_plan(args))
}

#[tauri::command]
fn execute_plan(args: ExecutePlanArgs) -> CommandResult<Report> {
    forward(engine::execute_plan(args))
}

#[tauri::command]
fn reveal(args: RevealArgs) -> CommandResult<()> {
    forward(engine::reveal(args))
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
                    Ok(())
                }
                Err(GuardError::AlreadyRunning) => {
                    eprintln!("Space Optimiser is already running.");
                    std::process::exit(1);
                }
                Err(e) => Err(e.into()),
            }
        })
        .invoke_handler(tauri::generate_handler![
            start_scan,
            cancel_scan,
            get_scan,
            build_plan,
            confirm_plan,
            execute_plan,
            reveal
        ])
        .run(tauri::generate_context!())
        .expect("error while running Space Optimiser");
}
