//! Space Optimiser engine.
//!
//! The engine is the only component that may delete files. The desktop shell
//! forwards typed commands to an [`Engine`]; the UI never sends a path. Scan,
//! policy, measurement, and execution live in separate modules (phase 3).
//!
//! Milestone M3: `user-temp` runs end to end. A scan finds candidates, a
//! confirmed plan is executed by [`execute`] (handle-relative, Unix first),
//! every action is written to the audit log, and the report measures free
//! space before and after.

mod audit;
pub mod clock;
pub mod discover;
mod execute;
pub mod guard;
pub mod measure;
pub mod model;
pub mod platform;
pub mod policy;
mod session;

pub use session::{Config, Engine};

use serde::ser::SerializeStruct;

/// Errors a command returns. The UI receives `{ kind, message }`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EngineError {
    #[error("not supported in this build")]
    Unsupported,
    #[error("that scan is no longer available. Scan again.")]
    UnknownScan,
    #[error("the scan has not finished")]
    ScanNotFinished,
    #[error("select at least one item")]
    EmptyPlan,
    #[error("an item in the selection is not in this scan. Scan again.")]
    UnknownCandidate,
    #[error("an item in the selection cannot be cleaned")]
    NotActionable,
    #[error("that plan is no longer available. Scan again.")]
    UnknownPlan,
    #[error(
        "the plan is more than 15 minutes old, or the computer slept since it was made. Scan again."
    )]
    PlanExpired,
    #[error("the confirmation does not match this plan")]
    WrongToken,
    #[error("a cleanup is already running")]
    RunInProgress,
    #[error("Space Optimiser does not run as root")]
    RunningAsRoot,
    #[error("could not read this computer's folders: {0}")]
    Context(String),
}

impl EngineError {
    pub fn kind(&self) -> &'static str {
        match self {
            EngineError::Unsupported => "unsupported",
            EngineError::UnknownScan => "unknown_scan",
            EngineError::ScanNotFinished => "scan_not_finished",
            EngineError::EmptyPlan => "empty_plan",
            EngineError::UnknownCandidate => "unknown_candidate",
            EngineError::NotActionable => "not_actionable",
            EngineError::UnknownPlan => "unknown_plan",
            EngineError::PlanExpired => "plan_expired",
            EngineError::WrongToken => "wrong_token",
            EngineError::RunInProgress => "run_in_progress",
            EngineError::RunningAsRoot => "running_as_root",
            EngineError::Context(_) => "context",
        }
    }
}

impl serde::Serialize for EngineError {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        let mut error = serializer.serialize_struct("EngineError", 2)?;
        error.serialize_field("kind", self.kind())?;
        error.serialize_field("message", &self.to_string())?;
        error.end()
    }
}

pub type Result<T> = std::result::Result<T, EngineError>;
