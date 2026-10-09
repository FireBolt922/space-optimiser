//! Types shared by the engine and the desktop shell.
//!
//! Every command payload is a typed struct with `deny_unknown_fields`, so a
//! webview cannot add a `path` field (phase 3, review C10). Ids are opaque
//! newtypes with no conversion from strings or paths.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::measure::FileId;
use crate::policy::CatalogId;

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

impl CandidateId {
    pub(crate) fn new(value: u64) -> Self {
        Self(value)
    }
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Risk {
    Low,
    Medium,
    High,
    Info,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// Delete the entry. Only the executor (M3) can.
    Delete,
    /// Run the row's tool with a fixed argument list (M5).
    RunTool,
    /// Show size and location only.
    Report,
    /// Nothing is offered (an uncertain candidate, phase 2 item 12).
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    High,
    Uncertain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Macos,
    Windows,
    Linux,
}

impl Platform {
    pub const CURRENT: Platform = if cfg!(target_os = "macos") {
        Platform::Macos
    } else if cfg!(windows) {
        Platform::Windows
    } else {
        Platform::Linux
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Label {
    /// A file with more than one hard link: deleting this name frees nothing.
    Shared,
    /// Windows could not report the compressed size; this is the logical size.
    LogicalSize,
}

/// A scan result (phase 4 item 7). The path and identity stay in the engine;
/// the UI gets a lossy display name and an opaque id.
#[derive(Debug, Clone, Serialize)]
pub struct Candidate {
    pub id: CandidateId,
    pub catalog_id: CatalogId,
    pub display_name: String,
    pub explanation: &'static str,
    pub location_label: &'static str,
    /// Allocated bytes, deduplicated by hard link (shared files count 0).
    pub estimated_bytes: u64,
    pub file_count: u64,
    pub links_not_followed: u64,
    pub labels: Vec<Label>,
    pub risk: Risk,
    pub bulk_safe: bool,
    pub confidence: Confidence,
    pub action: Action,
    pub platform: Platform,
    #[serde(skip)]
    pub(crate) path: PathBuf,
    #[serde(skip)]
    pub(crate) file_id: FileId,
}

impl Candidate {
    /// The entry this candidate names. Never sent to the UI.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The identity recorded at scan time; the executor re-checks it (M3).
    pub fn file_id(&self) -> FileId {
        self.file_id
    }
}

/// A frozen, confirmed-later list of actions. Fields arrive in M3 (phase 8).
#[derive(Debug, Serialize)]
pub struct Plan {}

/// The outcome of an executed plan. Fields arrive in M3 (phase 8).
#[derive(Debug, Serialize)]
pub struct Report {}
