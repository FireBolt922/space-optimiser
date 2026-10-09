//! Space Optimiser engine.
//!
//! The engine is the only component that may delete files. The desktop shell
//! forwards typed commands here; the UI never sends a path. Scan, policy,
//! measurement, and execution live in separate modules (phase 3).
//!
//! Milestone M2: [`discover`] finds `user-temp` candidates under validated
//! roots and never deletes. Every command still returns
//! [`EngineError::Unsupported`] until M3 wires the scan to the UI.

pub mod discover;
pub mod execute;
pub mod guard;
pub mod measure;
pub mod model;
pub mod platform;
pub mod policy;

use model::{
    BuildPlanArgs, Candidate, ConfirmationToken, ExecutePlanArgs, Plan, PlanRef, Report,
    RevealArgs, ScanId, ScanRef, StartScanArgs,
};

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("not supported in this build")]
    Unsupported,
}

pub type Result<T> = std::result::Result<T, EngineError>;

pub fn start_scan(args: StartScanArgs) -> Result<ScanId> {
    discover::start(args.mode)
}

pub fn cancel_scan(args: ScanRef) -> Result<()> {
    discover::cancel(args.scan_id)
}

pub fn get_scan(args: ScanRef) -> Result<Vec<Candidate>> {
    discover::candidates(args.scan_id)
}

pub fn build_plan(args: BuildPlanArgs) -> Result<Plan> {
    execute::build_plan(args.scan_id, &args.candidate_ids)
}

pub fn confirm_plan(args: PlanRef) -> Result<ConfirmationToken> {
    execute::confirm_plan(args.plan_id)
}

/// Runs a confirmed plan. Takes a plan id and a token, never a path.
pub fn execute_plan(args: ExecutePlanArgs) -> Result<Report> {
    execute::execute_plan(args.plan_id, &args.confirmation_token)
}

/// Shows a scanned candidate in the OS file manager. Not a delete.
pub fn reveal(args: RevealArgs) -> Result<()> {
    platform::reveal(args.candidate_id)
}
