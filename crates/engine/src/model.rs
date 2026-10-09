//! Types shared by the engine and the desktop shell.
//!
//! Every command payload is a typed struct with `deny_unknown_fields`, so a
//! webview cannot add a `path` field (phase 3, review C10). Ids are opaque
//! newtypes with no conversion from strings or paths.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ScanMode {
    Normal,
    Deep,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(transparent)]
pub struct ScanId(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(transparent)]
pub struct PlanId(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(transparent)]
pub struct CandidateId(u64);

/// Proves the plan being executed is the plan that was built. A stale-plan
/// guard, not a security boundary (phase 2 item 13).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(transparent)]
pub struct ConfirmationToken(String);

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartScanArgs {
    pub mode: ScanMode,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScanRef {
    pub scan_id: ScanId,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildPlanArgs {
    pub scan_id: ScanId,
    pub candidate_ids: Vec<CandidateId>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanRef {
    pub plan_id: PlanId,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutePlanArgs {
    pub plan_id: PlanId,
    pub confirmation_token: ConfirmationToken,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevealArgs {
    pub candidate_id: CandidateId,
}

/// A scan result. Fields arrive in M2 (phase 4 item 7).
#[derive(Debug, Serialize)]
pub struct Candidate {}

/// A frozen, confirmed-later list of actions. Fields arrive in M3 (phase 8).
#[derive(Debug, Serialize)]
pub struct Plan {}

/// The outcome of an executed plan. Fields arrive in M3 (phase 8).
#[derive(Debug, Serialize)]
pub struct Report {}
