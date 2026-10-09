//! Plans, confirmation, and the only delete path. Implemented in M3 (phase 8).

use crate::model::{CandidateId, ConfirmationToken, Plan, PlanId, Report, ScanId};
use crate::{EngineError, Result};

pub(crate) fn build_plan(_scan: ScanId, _ids: &[CandidateId]) -> Result<Plan> {
    Err(EngineError::Unsupported)
}

pub(crate) fn confirm_plan(_plan: PlanId) -> Result<ConfirmationToken> {
    Err(EngineError::Unsupported)
}

pub(crate) fn execute_plan(_plan: PlanId, _token: &ConfirmationToken) -> Result<Report> {
    Err(EngineError::Unsupported)
}
