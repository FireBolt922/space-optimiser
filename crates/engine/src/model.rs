//! Types shared by the engine and the desktop shell.
//!
//! Every command payload is a typed struct with `deny_unknown_fields`, so a
//! webview cannot add a `path` field (phase 3, review C10). Ids are opaque
//! newtypes with no conversion from strings or paths.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::discover::Progress;
use crate::measure::FileId;
use crate::policy::{CatalogId, RootRejection, SkipReason, ValidRoot};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ScanMode {
    Normal,
    Deep,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(transparent)]
pub struct ScanId(u64);

impl ScanId {
    pub(crate) fn new(value: u64) -> Self {
        Self(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(transparent)]
pub struct PlanId(u64);

impl PlanId {
    pub(crate) fn new(value: u64) -> Self {
        Self(value)
    }
}

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

impl ConfirmationToken {
    pub(crate) fn new(value: String) -> Self {
        Self(value)
    }
}

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
    /// The second confirm after [`ExecuteStart::AuditLogUnavailable`]: run
    /// with the audit log kept in memory (phase 2 item 10).
    #[serde(default)]
    pub keep_log_in_memory: bool,
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
    /// The validated root the entry sits directly in.
    #[serde(skip)]
    pub(crate) root: ValidRoot,
    /// Sorted identities of everything inside a directory entry.
    #[serde(skip)]
    pub(crate) members: Arc<[FileId]>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanStatus {
    Running,
    Finished,
    Cancelled,
    Failed,
}

/// An entry the scan left alone, with the reason in words.
#[derive(Debug, Clone, Serialize)]
pub struct SkippedView {
    pub display_name: String,
    pub reason: SkipReason,
    pub detail: String,
}

/// One temp root: scanned, or skipped with the reason.
#[derive(Debug, Clone, Serialize)]
pub struct RootView {
    pub location_label: &'static str,
    /// Why the whole root was skipped, in words.
    pub skipped: Option<String>,
    /// The root is in memory (tmpfs): cleaning frees memory, not disk.
    pub frees_memory: bool,
}

/// What `get_scan` returns. Candidates arrive when the scan finishes.
#[derive(Debug, Clone, Serialize)]
pub struct ScanView {
    pub scan_id: ScanId,
    pub status: ScanStatus,
    pub progress: Progress,
    pub error: Option<String>,
    pub candidates: Vec<Candidate>,
    pub skipped: Vec<SkippedView>,
    pub roots: Vec<RootView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlanItem {
    pub candidate_id: CandidateId,
    pub catalog_id: CatalogId,
    pub display_name: String,
    /// What will happen, in words (phase 2 item 11).
    pub action_text: &'static str,
    pub estimated_bytes: u64,
    pub file_count: u64,
}

/// A frozen list of actions, shown on the review screen. The engine keeps
/// the paths and identities; this is the readable copy.
#[derive(Debug, Clone, Serialize)]
pub struct Plan {
    pub plan_id: PlanId,
    pub scan_id: ScanId,
    pub items: Vec<PlanItem>,
    pub estimated_bytes: u64,
    /// Seconds until the plan expires if the computer stays awake.
    pub expires_in_secs: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ExecuteStart {
    Started,
    /// Nothing ran: the "run started" line could not be written. Ask for a
    /// second confirm, then call again with `keep_log_in_memory`.
    AuditLogUnavailable {
        disk_full: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Removed,
    Skipped,
    Failed,
    /// Stop was pressed before this item.
    NotRun,
}

/// Why the executor left a planned item alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, thiserror::Error)]
#[serde(rename_all = "snake_case")]
pub enum SkipCause {
    /// The entry, or something inside it, is not what the scan saw.
    #[error("it changed since the scan")]
    ChangedSinceScan,
    #[error("it was already gone")]
    Gone,
    #[error("this cleanup is not in this build")]
    NotInThisBuild,
    #[error("{0}")]
    Entry(SkipReason),
    #[error("{0}")]
    Root(RootRejection),
}

#[derive(Debug, Clone, Serialize)]
pub struct ReportItem {
    pub candidate_id: CandidateId,
    pub catalog_id: CatalogId,
    pub display_name: String,
    pub outcome: Outcome,
    pub reason: Option<SkipCause>,
    /// The skip reason or the OS error, in words.
    pub detail: Option<String>,
    /// Allocated bytes measured just before each unlink, summed.
    pub bytes_removed: u64,
    /// What to expect now (phase 8 item 10). Set for removed items.
    pub recovery_text: Option<&'static str>,
}

/// Free space on one volume, as the system reports it.
#[derive(Debug, Clone, Serialize)]
pub struct FreeSpace {
    pub location_label: &'static str,
    pub before: Option<u64>,
    pub after: Option<u64>,
    /// How long after the last action the "after" reading was taken.
    pub after_delay_secs: u64,
    pub source: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditLogStatus {
    Written,
    /// The log could not be written; `Report::memory_log` holds it.
    MemoryOnly,
}

/// The outcome of an executed plan. A partial run is still a finished run.
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub plan_id: PlanId,
    pub items: Vec<ReportItem>,
    pub bytes_removed: u64,
    pub stopped: bool,
    pub free_space: Vec<FreeSpace>,
    pub audit_log: AuditLogStatus,
    /// The audit lines, when they could not all be written to disk.
    pub memory_log: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Running,
    Finished,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunView {
    pub plan_id: PlanId,
    pub status: RunStatus,
    pub items_done: usize,
    pub items_total: usize,
    pub report: Option<Report>,
}

#[derive(Debug, Clone, Serialize)]
pub struct InterruptedAction {
    pub display_name: String,
    pub outcome: String,
}

/// A run that wrote "run started" and never "run finished" (phase 7 item 8).
#[derive(Debug, Clone, Serialize)]
pub struct InterruptedRun {
    pub started_unix_secs: u64,
    pub items_planned: usize,
    pub actions: Vec<InterruptedAction>,
}
