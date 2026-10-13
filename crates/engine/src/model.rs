//! Types shared by the engine and the desktop shell.
//!
//! Every command payload is a typed struct with `deny_unknown_fields`, so a
//! webview cannot add a `path` field (phase 3, review C10). Ids are opaque
//! newtypes with no conversion from strings or paths.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::discover::{Progress, Stage};
use crate::measure::FileId;
use crate::policy::{CatalogId, RootRejection, SkipReason, ValidRoot};
use crate::tool::Tool;

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
    /// The app that uses this cache is running (catalog "quit first" rows).
    /// The candidate's action is `none` until it quits.
    QuitFirst,
}

/// What a candidate's delete covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scope {
    /// One entry directly inside the root (`user-temp`).
    Entry,
    /// Everything inside the root; the root folder itself stays (cache rows).
    Contents,
    /// The row's tool runs its own clean command.
    Tool,
    /// Shown only.
    Report,
}

/// A tool row's clean command, fixed by the catalog. The executor checks
/// `args` against the catalog again before running.
#[derive(Debug, Clone)]
pub(crate) struct ToolAction {
    pub tool: Tool,
    pub args: &'static [&'static str],
}

/// What a top-level temp or log entry is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    File,
    Folder,
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
    /// Zero for tool rows, whose tool decides what to remove.
    pub estimated_bytes: u64,
    /// For tool rows: the most the tool could free (the cache's size, or
    /// the tool's own reclaimable number). Never added to the delete total.
    pub upper_bound_bytes: Option<u64>,
    /// How the location was found, for tool rows ("from `pnpm store path`").
    pub found_by: Option<String>,
    /// Other candidates this one removes too (a folder deleted whole that
    /// holds another row's folder). Selecting both counts them once.
    pub covers: Vec<CandidateId>,
    /// File or folder, for a top-level temp or log entry. None for cache,
    /// tool, Docker, and large-file rows.
    pub kind: Option<EntryKind>,
    /// Whole days since the entry or anything inside it last changed, for a
    /// top-level temp or log entry.
    pub age_days: Option<u32>,
    pub file_count: u64,
    pub links_not_followed: u64,
    pub labels: Vec<Label>,
    /// The app that must be quit before this cache is cleaned ("Chrome",
    /// "VS Code"), for "quit first" rows. None for every other row.
    pub quit_app: Option<&'static str>,
    pub risk: Risk,
    pub bulk_safe: bool,
    pub confidence: Confidence,
    pub action: Action,
    pub platform: Platform,
    /// "Show in folder" is offered. False only for Docker's build cache,
    /// whose path is the home folder rather than the cache.
    pub revealable: bool,
    #[serde(skip)]
    pub(crate) path: PathBuf,
    #[serde(skip)]
    pub(crate) file_id: FileId,
    /// The validated root: the folder the entry sits directly in, or the
    /// cache folder itself for a `Contents` or `Tool` candidate. None for
    /// Docker and the large-file report.
    #[serde(skip)]
    pub(crate) root: Option<ValidRoot>,
    #[serde(skip)]
    pub(crate) scope: Scope,
    #[serde(skip)]
    pub(crate) tool: Option<ToolAction>,
    /// Sorted identities of everything inside a directory entry, or inside
    /// the cache folder.
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

/// Something a row found that is not a candidate: a tool that was not run,
/// Docker pointing elsewhere, volumes that are never offered.
#[derive(Debug, Clone, Serialize)]
pub struct Note {
    pub catalog_id: CatalogId,
    pub location_label: &'static str,
    pub text: String,
    /// A size the text refers to, for the UI to format.
    pub bytes: Option<u64>,
}

/// Information-only findings for developers (v0.3). None of it has a
/// candidate id, so nothing here can be planned, revealed, or removed.
#[derive(Debug, Clone, Default, Serialize)]
pub struct DevAssets {
    /// Present when Docker answered locally or its disk file exists.
    pub docker: Option<DockerAssets>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct DockerAssets {
    pub images: Vec<DockerImage>,
    pub containers: Vec<DockerContainer>,
    pub volumes: Vec<DockerVolume>,
    pub disk_file: Option<DiskFile>,
    /// Why the lists are empty or missing, in words.
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DockerImage {
    pub repository: String,
    pub tag: String,
    pub size_bytes: Option<u64>,
    /// Space only this image uses (not shared with other images).
    pub unique_bytes: Option<u64>,
    /// Containers created from it, running or not.
    pub containers: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DockerContainer {
    pub name: String,
    pub image: String,
    /// `running`, `exited`, ...
    pub state: String,
    /// Docker's words, such as "Up 25 minutes".
    pub status: String,
    /// The container's own writable layer.
    pub size_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DockerVolume {
    pub name: String,
    pub size_bytes: Option<u64>,
    /// Containers that use it, running or not.
    pub in_use_by: Option<u64>,
}

/// Docker Desktop's virtual disk, measured with one `stat`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiskFile {
    /// Where it is, relative to the home ("~/Library/...").
    pub location_label: String,
    /// Space it takes on disk.
    pub allocated_bytes: u64,
    /// The size it reports; can be far larger (a sparse file).
    pub logical_bytes: u64,
}

/// What `get_scan` returns. Candidates arrive when the scan finishes.
#[derive(Debug, Clone, Serialize)]
pub struct ScanView {
    pub scan_id: ScanId,
    pub status: ScanStatus,
    /// The stages this scan runs, in order; `progress.stage` is the current one.
    pub stages: Vec<Stage>,
    pub progress: Progress,
    /// How long the scan took, once it stops running (phase 5 item 9).
    pub duration_ms: u64,
    pub error: Option<String>,
    pub candidates: Vec<Candidate>,
    pub skipped: Vec<SkippedView>,
    pub roots: Vec<RootView>,
    pub notes: Vec<Note>,
    /// Deep Scan only: information for developers, never offered.
    pub dev_assets: Option<DevAssets>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlanItem {
    pub candidate_id: CandidateId,
    pub catalog_id: CatalogId,
    pub display_name: String,
    /// What will happen, in words (phase 2 item 11).
    pub action_text: &'static str,
    pub estimated_bytes: u64,
    pub upper_bound_bytes: Option<u64>,
    pub file_count: u64,
}

/// A frozen list of actions, shown on the review screen. The engine keeps
/// the paths and identities; this is the readable copy.
#[derive(Debug, Clone, Serialize)]
pub struct Plan {
    pub plan_id: PlanId,
    pub scan_id: ScanId,
    pub items: Vec<PlanItem>,
    /// Allocated bytes of the planned deletes, each file counted once.
    pub estimated_bytes: u64,
    /// The most the planned tool commands could free. Shown on its own
    /// line, never added to `estimated_bytes` (catalog dedup rule 5).
    pub tool_upper_bound_bytes: u64,
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
    /// A tool's clean command finished. It reports no byte count; the free
    /// space change is the measurement.
    Ran,
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
    /// A "quit first" row whose app is running now.
    #[error("the app is open. Quit it, then scan again")]
    AppRunning,
    /// The process list could not be read, so a running app cannot be
    /// ruled out.
    #[error("could not check whether the app is open")]
    AppCheckFailed,
    /// The tool is busy (a lock held by another process).
    #[error("the tool is busy with another process. Try again later")]
    ToolBusy,
    /// Docker no longer points at this computer.
    #[error("Docker now points to another machine")]
    DockerNotLocal,
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
    /// What to do about a busy tool, for an item skipped as
    /// [`SkipCause::ToolBusy`].
    pub hint: Option<&'static str>,
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

/// The item a run is working on now.
#[derive(Debug, Clone, Serialize)]
pub struct CurrentItem {
    /// Its position in the plan, from 0.
    pub index: usize,
    pub display_name: String,
    pub catalog_id: CatalogId,
    pub elapsed_ms: u64,
    /// The last lines its tool printed, oldest first, cleaned of terminal
    /// codes. A tool row's first line names the command. Empty for deletes.
    pub output: Vec<String>,
}

/// Finished items so far, by outcome.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct RunCounts {
    /// Removed, or a tool's clean command ran.
    pub removed: usize,
    pub skipped: usize,
    pub failed: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunView {
    pub plan_id: PlanId,
    pub status: RunStatus,
    pub items_done: usize,
    pub items_total: usize,
    /// Since the run started.
    pub elapsed_ms: u64,
    /// None between items and once the run is over.
    pub current: Option<CurrentItem>,
    /// The most recently finished items, newest last (at most
    /// [`RECENT_ITEMS`]).
    pub recent: Vec<ReportItem>,
    pub counts: RunCounts,
    pub report: Option<Report>,
}

/// How many finished items a [`RunView`] carries.
pub const RECENT_ITEMS: usize = 20;
/// How many output lines a [`CurrentItem`] keeps.
pub const OUTPUT_LINES: usize = 50;

#[derive(Debug, Clone, Serialize)]
pub struct InterruptedAction {
    pub display_name: String,
    pub outcome: String,
}

/// What the About screen shows (phase 12). Paths are for reading only; no
/// command accepts them back.
#[derive(Debug, Clone, Serialize)]
pub struct AppInfo {
    pub version: &'static str,
    pub platform: Platform,
    /// Where the audit log is written, so a person can delete it themselves.
    pub audit_log_path: String,
    /// The folder `export_report` saves into.
    pub export_folder: String,
}

/// A report saved by `export_report`.
#[derive(Debug, Clone, Serialize)]
pub struct ExportedReport {
    /// The full path, for display.
    pub path: String,
    pub file_name: String,
}

/// A run that wrote "run started" and never "run finished" (phase 7 item 8).
#[derive(Debug, Clone, Serialize)]
pub struct InterruptedRun {
    pub started_unix_secs: u64,
    pub items_planned: usize,
    pub actions: Vec<InterruptedAction>,
}
