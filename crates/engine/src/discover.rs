//! Walkers that emit unconfirmed candidates from validated catalog roots.
//! Never deletes (phase 4). Normal Scan (phase 5) covers `user-temp` and
//! the cache rows that [`crate::roots`] lists, runs no tools, and does not
//! walk the home. Deep Scan (phase 6) adds the package caches, old logs,
//! the tool rows, Docker, and the large-file report.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime};

use serde::Serialize;

use crate::docker;
use crate::measure::{self, Facts, FileId, Kind};
use crate::model::{
    Action, Candidate, CandidateId, Confidence, DevAssets, DockerAssets, EntryKind, Label, Note,
    Platform, ScanMode, Scope, ToolAction,
};
pub use crate::policy::SkipReason;
use crate::policy::{self, CatalogId, Context, RootRejection, ValidRoot};
use crate::roots;
use crate::tool::{self, Exit, NotFound, Tool};

/// Shared cancel flag. Checked before every directory entry.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// Sent at least every 200 ms or 1,000 files, whichever comes first, and
/// whenever the scan moves to another stage or tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Progress {
    pub files_seen: u64,
    pub bytes_seen: u64,
    pub stage: Stage,
    /// The tool a [`Stage::Tools`] row is asking, such as `uv`.
    pub detail: Option<&'static str>,
}

impl Progress {
    pub const START: Progress = Progress {
        files_seen: 0,
        bytes_seen: 0,
        stage: Stage::Temp,
        detail: None,
    };
}

/// A part of a scan, in the order the scan runs them. The UI names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Temp,
    Caches,
    Logs,
    Tools,
    Docker,
    LargeFiles,
}

/// The stages `mode` runs, in order.
pub fn stages(mode: ScanMode) -> Vec<Stage> {
    match mode {
        ScanMode::Normal => vec![Stage::Temp, Stage::Caches],
        ScanMode::Deep => vec![
            Stage::Temp,
            Stage::Caches,
            Stage::Logs,
            Stage::Tools,
            Stage::Docker,
            Stage::LargeFiles,
        ],
    }
}

const PROGRESS_FILES: u64 = 1_000;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(200);

/// The large-file report lists at most this many files.
pub const LARGE_FILE_LIMIT: usize = 100;
/// The large-file walk does not go deeper than this below the home.
pub const LARGE_FILE_DEPTH: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ScanError {
    #[error("Space Optimiser does not scan as root")]
    RunningAsRoot,
    #[error("Space Optimiser does not scan as administrator")]
    RunningElevated,
    #[error("the scan was cancelled")]
    Cancelled,
}

#[derive(Debug, Clone, Serialize)]
pub struct Skipped {
    pub display_name: String,
    pub reason: SkipReason,
}

#[derive(Debug)]
pub enum RootOutcome {
    /// The row is skipped for this root, with the reason. No fallback guess.
    Skipped { root: String, reason: RootRejection },
    Scanned {
        root: String,
        /// The root is in memory (tmpfs): cleaning frees memory, not disk.
        frees_memory: bool,
        candidates: Vec<Candidate>,
        skipped: Vec<Skipped>,
    },
}

#[derive(Debug)]
pub struct TempScan {
    pub roots: Vec<RootOutcome>,
    /// The final counts, including files since the last progress event.
    pub progress: Progress,
}

/// Discovers `user-temp` candidates under the context's temp roots: top-level
/// entries older than [`policy::TEMP_MIN_AGE`], judged whole.
pub fn scan_user_temp(
    ctx: &Context,
    cancel: &CancelToken,
    on_progress: impl FnMut(Progress),
) -> std::result::Result<TempScan, ScanError> {
    refuse_root(ctx)?;
    let mut walk = Walk::new(ctx, cancel, on_progress);
    let roots = walk.temp_roots()?;
    Ok(TempScan {
        roots,
        progress: walk.progress,
    })
}

/// The Normal Scan rows that are cache folders, in display order.
const CACHE_ROWS: [CatalogId; 4] = [
    CatalogId::ThumbnailCache,
    CatalogId::FontCache,
    CatalogId::WindowsThumbcache,
    CatalogId::BrowserHttpCache,
];

/// Deep Scan's cache folders: the Normal Scan ones, then the fixed
/// package-cache folders, emptied in place like browser caches.
const DEEP_CACHE_ROWS: [CatalogId; 9] = [
    CatalogId::ThumbnailCache,
    CatalogId::FontCache,
    CatalogId::WindowsThumbcache,
    CatalogId::BrowserHttpCache,
    CatalogId::PkgNpx,
    CatalogId::PkgCargoCache,
    CatalogId::PkgGradleCaches,
    CatalogId::PkgNugetHttp,
    CatalogId::IdeIndexCaches,
];

/// A row whose tool says where its cache is and cleans it.
struct ToolRow {
    row: CatalogId,
    /// Executable names, tried in order.
    names: &'static [&'static str],
    /// Prints the cache folder.
    query: &'static [&'static str],
    /// The clean command.
    action: &'static [&'static str],
    stop_on: &'static [&'static str],
    /// Folders directly inside the cache that another row covers.
    skip_top: &'static [&'static str],
}

/// Output that means a package manager is about to download itself.
const COREPACK_STOP: &[&str] = &["corepack"];
/// `uv` waits for a lock held by another `uv` (phase 6 item 6).
const UV_STOP: &[&str] = &["currently in-use", "waiting for other uv"];

const TOOL_ROWS: [ToolRow; 5] = [
    ToolRow {
        row: CatalogId::PkgNpmCache,
        names: &["npm"],
        query: &["config", "get", "cache"],
        action: &["cache", "clean", "--force"],
        stop_on: COREPACK_STOP,
        // `npm cache clean` leaves `_npx`; it is its own row.
        skip_top: &["_npx"],
    },
    ToolRow {
        row: CatalogId::PkgPnpmStore,
        names: &["pnpm"],
        query: &["store", "path"],
        action: &["store", "prune"],
        stop_on: COREPACK_STOP,
        skip_top: &[],
    },
    ToolRow {
        row: CatalogId::PkgYarnCache,
        names: &["yarn"],
        query: &["cache", "dir"],
        action: &["cache", "clean"],
        stop_on: COREPACK_STOP,
        skip_top: &[],
    },
    ToolRow {
        row: CatalogId::PkgPipCache,
        names: &["pip", "pip3"],
        query: &["cache", "dir"],
        action: &["cache", "purge"],
        stop_on: &[],
        skip_top: &[],
    },
    ToolRow {
        row: CatalogId::PkgUvCache,
        names: &["uv"],
        query: &["cache", "dir"],
        action: &["cache", "clean"],
        stop_on: UV_STOP,
        skip_top: &[],
    },
];

/// The first line a read-only query prints, if it succeeded.
fn query(
    ctx: &Context,
    tool: &Tool,
    args: &[&str],
    stop_on: &'static [&'static str],
) -> Option<String> {
    let timeout = ctx.tools().query_timeout;
    let out = tool::run(ctx, tool, args, timeout, stop_on).ok()?;
    if out.exit != Exit::Success {
        return None;
    }
    out.stdout
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(str::to_owned)
}

/// The cache folder `row`'s tool reports now, validated as at scan time.
/// The executor asks again right before a clean command, so a cache moved
/// after the scan (a changed `.npmrc` or `uv.toml`) is never cleaned.
pub(crate) fn tool_cache_root(ctx: &Context, row: CatalogId, tool: &Tool) -> Option<ValidRoot> {
    let spec = TOOL_ROWS.iter().find(|t| t.row == row)?;
    let answer = query(ctx, tool, spec.query, spec.stop_on)?;
    let path = PathBuf::from(answer);
    if !path.is_absolute() {
        return None;
    }
    policy::validate_cache_root(ctx, &path).ok()
}

/// The arguments and stop lines of `row`'s clean command, for the executor.
pub(crate) fn tool_action(
    row: CatalogId,
) -> Option<(&'static [&'static str], &'static [&'static str])> {
    if row == CatalogId::DockerBuildCache {
        return Some((docker::PRUNE, &[]));
    }
    TOOL_ROWS
        .iter()
        .find(|t| t.row == row)
        .map(|t| (t.action, t.stop_on))
}

const YARN_BERRY: &str =
    "Yarn 2 and later keep caches per project, so the Yarn cache is not offered.";
const NETWORK_HOME: &str =
    "Your home folder is on a network volume, so the large-file list was skipped.";
const VOLUMES: &str =
    "Docker volumes could free this much, but they hold databases and are never offered.";

/// What a scan found.
#[derive(Debug)]
pub struct Scan {
    pub temp: Vec<RootOutcome>,
    /// One candidate per cache folder, tool row, or large file.
    pub caches: Vec<Candidate>,
    /// Cache folders and entries left alone, with the reason.
    pub cache_skipped: Vec<Skipped>,
    /// What rows found that is not a candidate.
    pub notes: Vec<Note>,
    /// Deep Scan only: information for developers, never offered.
    pub dev_assets: Option<DevAssets>,
    pub progress: Progress,
}

/// The Normal Scan result. Deep Scan returns the same shape.
pub type NormalScan = Scan;

/// Normal Scan (phase 5): `user-temp`, then each cache row's folders. A
/// browser that is running, or that cannot be ruled out, makes its rows
/// "quit first".
pub fn scan_normal(
    ctx: &Context,
    cancel: &CancelToken,
    on_progress: impl FnMut(Progress),
) -> std::result::Result<Scan, ScanError> {
    scan(ctx, ScanMode::Normal, cancel, on_progress)
}

/// Deep Scan (phase 6): every Normal Scan row, then the package caches,
/// old logs, the tool rows, Docker, and the large-file report.
pub fn scan_deep(
    ctx: &Context,
    cancel: &CancelToken,
    on_progress: impl FnMut(Progress),
) -> std::result::Result<Scan, ScanError> {
    scan(ctx, ScanMode::Deep, cancel, on_progress)
}

pub fn scan(
    ctx: &Context,
    mode: ScanMode,
    cancel: &CancelToken,
    on_progress: impl FnMut(Progress),
) -> std::result::Result<Scan, ScanError> {
    refuse_root(ctx)?;
    let mut walk = Walk::new(ctx, cancel, on_progress);
    let temp = walk.temp_roots()?;
    let running = ctx
        .process_names()
        .map(|names| roots::running_apps(&names, Platform::CURRENT));
    let mut out = Scan {
        temp,
        caches: Vec::new(),
        cache_skipped: Vec::new(),
        notes: Vec::new(),
        dev_assets: None,
        progress: walk.progress,
    };
    let mut seen = HashSet::new();
    walk.stage(Stage::Caches, None);
    let rows: &[CatalogId] = match mode {
        ScanMode::Normal => &CACHE_ROWS,
        ScanMode::Deep => &DEEP_CACHE_ROWS,
    };
    for &row in rows {
        for found in roots::cache_roots(ctx, row, Platform::CURRENT) {
            walk.check_cancel()?;
            let root = match found.root {
                Ok(root) => root,
                Err(reason) => {
                    out.cache_skipped.push(Skipped {
                        display_name: found.display_name,
                        reason,
                    });
                    continue;
                }
            };
            if !seen.insert(root.id()) {
                continue;
            }
            let quit_first = found.app.is_some_and(|a| match &running {
                Ok(running) => running.contains(&a),
                Err(_) => true,
            });
            match walk.cache(row, &root, found.display_name, found.app, quit_first)? {
                Some(Ok(candidate)) => out.caches.push(candidate),
                Some(Err(skipped)) => out.cache_skipped.push(skipped),
                None => {}
            }
        }
    }
    if mode == ScanMode::Deep {
        walk.stage(Stage::Logs, None);
        walk.logs(&mut out)?;
        for spec in &TOOL_ROWS {
            walk.check_cancel()?;
            walk.stage(Stage::Tools, spec.names.first().copied());
            walk.tool_row(spec, &mut out)?;
        }
        walk.check_cancel()?;
        walk.stage(Stage::Docker, None);
        let docker = walk.docker(&mut out)?;
        out.dev_assets = Some(DevAssets { docker });
        walk.stage(Stage::LargeFiles, None);
        walk.large_files(&mut out)?;
    }
    out.progress = walk.progress;
    Ok(out)
}

fn refuse_root(ctx: &Context) -> std::result::Result<(), ScanError> {
    #[cfg(unix)]
    if crate::guard::should_refuse(ctx.euid()) {
        return Err(ScanError::RunningAsRoot);
    }
    #[cfg(windows)]
    if crate::guard::should_refuse_elevated(ctx.elevated()) {
        return Err(ScanError::RunningElevated);
    }
    Ok(())
}

struct Walk<'a, F> {
    ctx: &'a Context,
    cancel: &'a CancelToken,
    on_progress: F,
    progress: Progress,
    since_event: u64,
    last_event: Instant,
    next_id: u64,
}

/// What an entry adds up to.
struct Summary {
    files: u64,
    bytes: u64,
    links: u64,
    shared: bool,
    exact: bool,
    /// Everything inside the entry, links included. The executor deletes
    /// only these.
    members: Vec<FileId>,
    /// The newest age timestamp of the entry and everything inside it.
    newest: Option<SystemTime>,
}

impl Summary {
    fn empty(exact: bool) -> Self {
        Self {
            files: 0,
            bytes: 0,
            links: 0,
            shared: false,
            exact,
            members: Vec::new(),
            newest: None,
        }
    }
}

enum Verdict {
    Keep(Facts, Summary),
    Skip(SkipReason),
}

/// What a candidate is, beyond its row and summary.
struct Target<'r> {
    root: Option<&'r ValidRoot>,
    path: &'r Path,
    file_id: FileId,
    scope: Scope,
}

impl<'a, F: FnMut(Progress)> Walk<'a, F> {
    fn new(ctx: &'a Context, cancel: &'a CancelToken, on_progress: F) -> Self {
        Walk {
            ctx,
            cancel,
            on_progress,
            progress: Progress::START,
            since_event: 0,
            last_event: Instant::now(),
            next_id: 1,
        }
    }

    /// Moves to `stage` and says so at once: a tool or Docker can keep the
    /// scan waiting without a single file tick.
    fn stage(&mut self, stage: Stage, detail: Option<&'static str>) {
        self.progress.stage = stage;
        self.progress.detail = detail;
        (self.on_progress)(self.progress);
        self.since_event = 0;
        self.last_event = Instant::now();
    }

    fn temp_roots(&mut self) -> std::result::Result<Vec<RootOutcome>, ScanError> {
        self.stage(Stage::Temp, None);
        let ctx = self.ctx;
        let mut seen = HashSet::new();
        let mut roots = Vec::new();
        for (path, owner) in ctx.temp_roots() {
            let label = path.to_string_lossy().into_owned();
            match policy::validate_temp_root(ctx, path, *owner) {
                Err(reason) => roots.push(RootOutcome::Skipped {
                    root: label,
                    reason,
                }),
                Ok(root) => {
                    if seen.insert(root.id()) {
                        roots.push(self.root(&root)?);
                    }
                }
            }
        }
        Ok(roots)
    }

    /// One cache folder, judged whole: a candidate for everything inside it,
    /// a skip with the reason, or nothing when it is empty.
    fn cache(
        &mut self,
        row: CatalogId,
        root: &ValidRoot,
        display_name: String,
        app: Option<roots::App>,
        quit_first: bool,
    ) -> std::result::Result<Option<std::result::Result<Candidate, Skipped>>, ScanError> {
        if row == CatalogId::WindowsThumbcache {
            return Ok(self.thumbcache(root, display_name).map(Ok));
        }
        let device = Some(root.id().device());
        let (facts, summary) = match self.judge(row, root.path(), device, false)? {
            Verdict::Skip(reason) => {
                return Ok(Some(Err(Skipped {
                    display_name,
                    reason,
                })));
            }
            Verdict::Keep(facts, summary) => (facts, summary),
        };
        if summary.members.is_empty() {
            return Ok(None);
        }
        let target = Target {
            root: Some(root),
            path: root.path(),
            file_id: facts.id,
            scope: Scope::Contents,
        };
        let mut candidate = self.candidate(row, target, display_name, summary);
        candidate.quit_app = app.map(roots::App::label);
        if quit_first {
            candidate.action = Action::None;
            candidate.labels.push(Label::QuitFirst);
        }
        Ok(Some(Ok(candidate)))
    }

    /// Explorer's `thumbcache_*.db` files, summed. Shown, never cleaned.
    fn thumbcache(&mut self, root: &ValidRoot, display_name: String) -> Option<Candidate> {
        self.ctx.visit(root.path());
        let mut summary = Summary::empty(true);
        for entry in fs::read_dir(root.path()).ok()?.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !(name.starts_with("thumbcache_") && name.ends_with(".db")) {
                continue;
            }
            let Some(facts) = self.read_facts(&entry.path()) else {
                continue;
            };
            if facts.kind == Kind::File {
                summary.files += 1;
                summary.bytes += facts.size.bytes;
                summary.exact &= facts.size.exact;
                self.tick(facts.size.bytes);
            }
        }
        if summary.files == 0 {
            return None;
        }
        let target = Target {
            root: Some(root),
            path: root.path(),
            file_id: root.id(),
            scope: Scope::Contents,
        };
        Some(self.candidate(CatalogId::WindowsThumbcache, target, display_name, summary))
    }

    fn root(&mut self, root: &ValidRoot) -> std::result::Result<RootOutcome, ScanError> {
        let label = root.path().to_string_lossy().into_owned();
        match self.entries(CatalogId::UserTemp, root, &[])? {
            Some((candidates, skipped)) => Ok(RootOutcome::Scanned {
                root: label,
                frees_memory: is_tmpfs(root.path()),
                candidates,
                skipped,
            }),
            None => Ok(RootOutcome::Skipped {
                root: label,
                reason: RootRejection::Unreadable,
            }),
        }
    }

    /// Each entry directly inside `root`, judged whole under `row`. `None`
    /// when the root cannot be listed. Names in `never_read` are not even
    /// looked at.
    #[allow(clippy::type_complexity)]
    fn entries(
        &mut self,
        row: CatalogId,
        root: &ValidRoot,
        never_read: &[&str],
    ) -> std::result::Result<Option<(Vec<Candidate>, Vec<Skipped>)>, ScanError> {
        let mut candidates = Vec::new();
        let mut skipped = Vec::new();
        let device = Some(root.id().device());
        self.ctx.visit(root.path());
        let Ok(entries) = fs::read_dir(root.path()) else {
            return Ok(None);
        };
        for entry in entries {
            self.check_cancel()?;
            let Ok(entry) = entry else { continue };
            let name = entry.file_name();
            if never_read.iter().any(|n| name == *n) {
                continue;
            }
            let path = entry.path();
            let display_name = name.to_string_lossy().into_owned();
            match self.judge(row, &path, device, true)? {
                Verdict::Skip(reason) => skipped.push(Skipped {
                    display_name,
                    reason,
                }),
                Verdict::Keep(facts, summary) => {
                    let target = Target {
                        root: Some(root),
                        path: &path,
                        file_id: facts.id,
                        scope: Scope::Entry,
                    };
                    let age_days = summary.newest.and_then(|t| age_days(self.ctx.now(), t));
                    let mut candidate = self.candidate(row, target, display_name, summary);
                    candidate.kind = Some(if facts.kind == Kind::Dir {
                        EntryKind::Folder
                    } else {
                        EntryKind::File
                    });
                    candidate.age_days = age_days;
                    candidates.push(candidate);
                }
            }
        }
        Ok(Some((candidates, skipped)))
    }

    /// `user-logs-old`: each entry in the log folder, judged like temp, with
    /// the log-name rule. Crash reports are another row and are never read.
    fn logs(&mut self, out: &mut Scan) -> std::result::Result<(), ScanError> {
        let row = CatalogId::UserLogsOld;
        for found in roots::cache_roots(self.ctx, row, Platform::CURRENT) {
            let root = match found.root {
                Ok(root) => root,
                Err(reason) => {
                    out.cache_skipped.push(Skipped {
                        display_name: found.display_name,
                        reason,
                    });
                    continue;
                }
            };
            if let Some((candidates, skipped)) = self.entries(row, &root, &["DiagnosticReports"])? {
                out.caches.extend(candidates);
                out.cache_skipped.extend(skipped);
            }
        }
        Ok(())
    }

    /// A tool row: ask the tool where its cache is, size that folder as an
    /// upper bound, and offer the tool's own clean command. Without a tool,
    /// a default folder that exists is shown as "not sure".
    fn tool_row(&mut self, spec: &ToolRow, out: &mut Scan) -> std::result::Result<(), ScanError> {
        let ctx = self.ctx;
        let found = spec
            .names
            .iter()
            .map(|n| tool::find(ctx, n))
            .reduce(|a, b| match (&a, &b) {
                (Ok(_), _) => a,
                (_, Ok(_)) => b,
                (Err(NotFound::Missing), _) => b,
                _ => a,
            })
            .unwrap_or(Err(NotFound::Missing));
        let name = spec.names[0];
        let tool = match found {
            Ok(tool) => tool,
            Err(reason) => {
                if reason != NotFound::Missing {
                    note(out, spec.row, format!("{name}: {reason}."), None);
                }
                return self.fallback(spec, &format!("{name} {}", short(reason)), out);
            }
        };
        if spec.row == CatalogId::PkgYarnCache {
            match self.query(&tool, &["--version"], spec.stop_on) {
                Some(version) if version.starts_with("1.") => {}
                Some(_) => {
                    note(out, spec.row, YARN_BERRY.to_owned(), None);
                    return Ok(());
                }
                None => return self.fallback(spec, "yarn did not answer", out),
            }
        }
        let Some(answer) = self.query(&tool, spec.query, spec.stop_on) else {
            let asked = format!("{} {}", tool.name, spec.query.join(" "));
            note(out, spec.row, format!("`{asked}` did not answer."), None);
            return self.fallback(spec, &format!("`{asked}` did not answer"), out);
        };
        let path = PathBuf::from(answer);
        let display_name = tool::home_relative(ctx.home(), &path);
        if !path.is_absolute() {
            return self.fallback(spec, "the tool gave no folder", out);
        }
        let root = match policy::validate_cache_root(ctx, &path) {
            Ok(root) => root,
            Err(SkipReason::Unreadable) => return Ok(()),
            Err(reason) => {
                out.cache_skipped.push(Skipped {
                    display_name,
                    reason,
                });
                return Ok(());
            }
        };
        let found_by = format!(
            "Folder from `{} {}`; {} {}.",
            tool.name,
            spec.query.join(" "),
            tool.name,
            tool.found_text(ctx.home())
        );
        let action = ToolAction {
            tool,
            args: spec.action,
        };
        self.sized_tool_row(spec, &root, display_name, found_by, Some(action), out)
    }

    /// The tool's default folder, shown as "not sure — not offered".
    fn fallback(
        &mut self,
        spec: &ToolRow,
        why: &str,
        out: &mut Scan,
    ) -> std::result::Result<(), ScanError> {
        let Some(path) = roots::default_tool_cache(self.ctx, spec.row, Platform::CURRENT) else {
            return Ok(());
        };
        if fs::symlink_metadata(&path).is_err() {
            return Ok(());
        }
        let display_name = tool::home_relative(self.ctx.home(), &path);
        let root = match policy::validate_cache_root(self.ctx, &path) {
            Ok(root) => root,
            Err(reason) => {
                out.cache_skipped.push(Skipped {
                    display_name,
                    reason,
                });
                return Ok(());
            }
        };
        let found_by = format!("Default folder; {why}.");
        self.sized_tool_row(spec, &root, display_name, found_by, None, out)
    }

    /// Sizes a tool row's folder and adds the candidate. `action` is the
    /// clean command, or `None` for a default folder nothing can clean.
    fn sized_tool_row(
        &mut self,
        spec: &ToolRow,
        root: &ValidRoot,
        display_name: String,
        found_by: String,
        action: Option<ToolAction>,
        out: &mut Scan,
    ) -> std::result::Result<(), ScanError> {
        if out
            .caches
            .iter()
            .any(|c| c.root.as_ref().is_some_and(|r| r.id() == root.id()))
        {
            return Ok(());
        }
        let summary = self.size(root, spec.skip_top)?;
        if summary.files == 0 {
            return Ok(());
        }
        let upper = summary.bytes;
        let target = Target {
            root: Some(root),
            path: root.path(),
            file_id: root.id(),
            scope: Scope::Tool,
        };
        let mut candidate = self.candidate(spec.row, target, display_name, summary);
        candidate.estimated_bytes = 0;
        candidate.upper_bound_bytes = Some(upper);
        candidate.found_by = Some(found_by);
        match action {
            Some(action) => candidate.tool = Some(action),
            None => {
                candidate.action = Action::None;
                candidate.confidence = Confidence::Uncertain;
            }
        }
        out.caches.push(candidate);
        Ok(())
    }

    fn query(
        &self,
        tool: &Tool,
        args: &[&str],
        stop_on: &'static [&'static str],
    ) -> Option<String> {
        query(self.ctx, tool, args, stop_on)
    }

    /// `docker-build-cache`: one candidate for the build cache Docker
    /// reports, or one note saying why not.
    /// `docker-build-cache`, then the information-only Docker assets: the
    /// lists from `docker system df -v` when the build-cache checks passed,
    /// and Docker Desktop's disk file when it exists.
    fn docker(&mut self, out: &mut Scan) -> std::result::Result<Option<DockerAssets>, ScanError> {
        let row = CatalogId::DockerBuildCache;
        let disk_file = docker::disk_file(self.ctx);
        let mut assets = disk_file.map(|disk| DockerAssets {
            disk_file: Some(disk),
            ..DockerAssets::default()
        });
        let tool = match tool::find(self.ctx, "docker") {
            Ok(tool) => tool,
            Err(NotFound::Missing) => return Ok(assets),
            Err(reason) => {
                note(out, row, format!("docker: {reason}."), None);
                return Ok(assets);
            }
        };
        match docker::status(self.ctx, &tool) {
            docker::Status::NotOffered(reason) => note(out, row, reason.to_owned(), None),
            docker::Status::BuildCache {
                reclaimable,
                volumes,
            } => {
                if let Some(bytes) = volumes.filter(|b| *b > 0) {
                    note(out, row, VOLUMES.to_owned(), Some(bytes));
                }
                self.build_cache(out, reclaimable, &tool);
                let found = assets.get_or_insert_with(DockerAssets::default);
                match docker::details(self.ctx, &tool, self.cancel) {
                    docker::Details::Listed(lists) => {
                        found.images = lists.images;
                        found.containers = lists.containers;
                        found.volumes = lists.volumes;
                    }
                    docker::Details::Note(text) => found.note = Some(text.to_owned()),
                    docker::Details::Cancelled => return Err(ScanError::Cancelled),
                }
            }
        }
        Ok(assets)
    }

    /// The build-cache candidate, with Docker's reclaimable number.
    fn build_cache(&mut self, out: &mut Scan, reclaimable: u64, tool: &Tool) {
        let row = CatalogId::DockerBuildCache;
        if reclaimable == 0 {
            return;
        }
        let home = self.ctx.home();
        let Ok(home_id) = measure::file_id(home) else {
            return;
        };
        let target = Target {
            root: None,
            path: home,
            file_id: home_id,
            scope: Scope::Tool,
        };
        let found_by = format!(
            "Size from `docker system df`; docker {}.",
            tool.found_text(home)
        );
        let mut candidate = self.candidate(
            row,
            target,
            "Docker build cache".to_owned(),
            Summary::empty(true),
        );
        candidate.upper_bound_bytes = Some(reclaimable);
        candidate.found_by = Some(found_by);
        candidate.tool = Some(ToolAction {
            tool: tool.clone(),
            args: docker::PRUNE,
        });
        out.caches.push(candidate);
    }

    /// `large-files-report` (phase 6 item 9): the largest files in the
    /// home. Protected and cloud folders, `~/Library` on macOS, links, and
    /// other filesystems are never read. Shown only.
    fn large_files(&mut self, out: &mut Scan) -> std::result::Result<(), ScanError> {
        let ctx = self.ctx;
        let row = CatalogId::LargeFilesReport;
        let home = ctx.home().to_path_buf();
        if measure::is_network_volume(&home) {
            note(out, row, NETWORK_HOME.to_owned(), None);
            return Ok(());
        }
        let Some(home_facts) = self.read_facts(&home) else {
            return Ok(());
        };
        let device = home_facts.id.device();
        let mut never: HashSet<PathBuf> = ctx.protected_paths().map(Path::to_path_buf).collect();
        if Platform::CURRENT == Platform::Macos {
            never.insert(home.join("Library"));
        }
        let min = ctx.large_file_min();
        let mut found: Vec<(u64, PathBuf, Facts)> = Vec::new();
        let mut seen = HashSet::new();
        let mut stack = vec![(home.clone(), 0usize)];
        while let Some((dir, depth)) = stack.pop() {
            self.ctx.visit(&dir);
            let Ok(entries) = fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries {
                self.check_cancel()?;
                let Ok(entry) = entry else { continue };
                let path = entry.path();
                if never.contains(&path) {
                    continue;
                }
                let Some(facts) = self.read_facts(&path) else {
                    continue;
                };
                match facts.kind {
                    Kind::Dir => {
                        let other_disk = facts.id.device() != device;
                        if !other_disk
                            && !ctx.is_protected_id(facts.id)
                            && depth + 1 < LARGE_FILE_DEPTH
                        {
                            stack.push((path, depth + 1));
                        }
                    }
                    Kind::File => {
                        self.tick(facts.size.bytes);
                        if facts.size.bytes >= min && seen.insert(facts.id) {
                            found.push((facts.size.bytes, path, facts));
                        }
                    }
                    Kind::Link | Kind::SocketOrPipe | Kind::Device => {}
                }
            }
        }
        found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        found.truncate(LARGE_FILE_LIMIT);
        for (bytes, path, facts) in found {
            let display_name = tool::home_relative(&home, &path);
            let target = Target {
                root: None,
                path: &path,
                file_id: facts.id,
                scope: Scope::Report,
            };
            let mut summary = Summary::empty(facts.size.exact);
            summary.files = 1;
            summary.bytes = bytes;
            let candidate = self.candidate(row, target, display_name, summary);
            out.caches.push(candidate);
        }
        Ok(())
    }

    /// The allocated size of everything in `root` (each file once), for a
    /// tool row's upper bound. No rules: the tool decides what to remove.
    fn size(
        &mut self,
        root: &ValidRoot,
        skip_top: &[&str],
    ) -> std::result::Result<Summary, ScanError> {
        let device = root.id().device();
        let mut summary = Summary::empty(true);
        let mut seen = HashSet::new();
        let mut stack = vec![root.path().to_path_buf()];
        while let Some(dir) = stack.pop() {
            self.ctx.visit(&dir);
            let Ok(entries) = fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries {
                self.check_cancel()?;
                let Ok(entry) = entry else { continue };
                if dir == root.path() && skip_top.iter().any(|n| entry.file_name() == *n) {
                    continue;
                }
                let path = entry.path();
                let Some(facts) = self.read_facts(&path) else {
                    continue;
                };
                if facts.id.device() != device || !seen.insert(facts.id) {
                    continue;
                }
                summary.exact &= facts.size.exact;
                match facts.kind {
                    Kind::Dir => {
                        summary.bytes += facts.size.bytes;
                        stack.push(path);
                    }
                    Kind::File => {
                        summary.files += 1;
                        summary.bytes += facts.size.bytes;
                        self.tick(facts.size.bytes);
                    }
                    Kind::Link | Kind::SocketOrPipe | Kind::Device => {}
                }
            }
        }
        Ok(summary)
    }

    fn candidate(
        &mut self,
        row: CatalogId,
        target: Target<'_>,
        display_name: String,
        s: Summary,
    ) -> Candidate {
        let id = CandidateId::new(self.next_id);
        self.next_id += 1;
        let mut labels = Vec::new();
        if s.shared {
            labels.push(Label::Shared);
        }
        if !s.exact {
            labels.push(Label::LogicalSize);
        }
        let mut members = s.members;
        members.sort_unstable();
        members.dedup();
        Candidate {
            id,
            catalog_id: row,
            display_name,
            explanation: row.explanation(),
            location_label: row.location_label(),
            estimated_bytes: s.bytes,
            upper_bound_bytes: None,
            found_by: None,
            covers: Vec::new(),
            kind: None,
            age_days: None,
            file_count: s.files,
            links_not_followed: s.links,
            labels,
            quit_app: None,
            risk: row.risk(),
            bulk_safe: row.bulk_safe(),
            confidence: Confidence::High,
            action: row.action(),
            platform: Platform::CURRENT,
            revealable: row != CatalogId::DockerBuildCache,
            path: target.path.to_path_buf(),
            file_id: target.file_id,
            root: target.root.cloned(),
            scope: target.scope,
            tool: None,
            members: members.into(),
        }
    }

    /// Judges one entry whole under `row`'s rules: a top-level temp or log
    /// entry, or a cache folder. A directory is read to unlimited depth on
    /// the same filesystem, without following links. `count_self` adds the
    /// entry's own blocks, which a cache folder keeps. A file with several
    /// links counts once when every link is inside the entry, and 0
    /// (shared) otherwise.
    fn judge(
        &mut self,
        row: CatalogId,
        path: &Path,
        device: Option<u64>,
        count_self: bool,
    ) -> std::result::Result<Verdict, ScanError> {
        let Some(facts) = self.read_facts(path) else {
            return Ok(Verdict::Skip(SkipReason::Unreadable));
        };
        if let Some(reason) = policy::entry_rule(row, self.ctx, &facts, device) {
            return Ok(Verdict::Skip(reason));
        }
        if count_self {
            let name = path.file_name().unwrap_or_default();
            if let Some(reason) = policy::name_rule(row, name, facts.kind) {
                return Ok(Verdict::Skip(reason));
            }
        }
        if facts.kind != Kind::Dir {
            let shared = facts.links > 1;
            let bytes = if shared { 0 } else { facts.size.bytes };
            self.tick(bytes);
            let mut summary = Summary::empty(facts.size.exact);
            summary.newest = Some(newest(&facts));
            summary.files = 1;
            summary.bytes = bytes;
            summary.shared = shared;
            return Ok(Verdict::Keep(facts, summary));
        }
        let mut summary = Summary::empty(facts.size.exact);
        summary.newest = Some(newest(&facts));
        if count_self {
            summary.bytes = facts.size.bytes;
        }
        // Files with more than one link: names seen here, links, bytes.
        let mut linked: HashMap<FileId, (u64, u64, u64)> = HashMap::new();
        let mut stack = vec![path.to_path_buf()];
        while let Some(dir) = stack.pop() {
            self.ctx.visit(&dir);
            let Ok(entries) = fs::read_dir(&dir) else {
                return Ok(Verdict::Skip(SkipReason::Unreadable));
            };
            for entry in entries {
                self.check_cancel()?;
                let Ok(entry) = entry else {
                    return Ok(Verdict::Skip(SkipReason::Unreadable));
                };
                let name = entry.file_name();
                if row.checks_project_markers() && policy::is_project_marker(&name) {
                    return Ok(Verdict::Skip(SkipReason::Project));
                }
                let child = entry.path();
                let Some(child_facts) = self.read_facts(&child) else {
                    return Ok(Verdict::Skip(SkipReason::Unreadable));
                };
                if child_facts.kind == Kind::Link {
                    // Counted, never followed or sized as its target.
                    summary.links += 1;
                    summary.members.push(child_facts.id);
                    continue;
                }
                if let Some(reason) = policy::entry_rule(row, self.ctx, &child_facts, device)
                    .or_else(|| policy::name_rule(row, &name, child_facts.kind))
                {
                    return Ok(Verdict::Skip(reason));
                }
                summary.members.push(child_facts.id);
                summary.exact &= child_facts.size.exact;
                summary.newest = summary.newest.max(Some(newest(&child_facts)));
                if child_facts.kind == Kind::Dir {
                    summary.bytes += child_facts.size.bytes;
                    stack.push(child);
                    continue;
                }
                summary.files += 1;
                if child_facts.links > 1 {
                    let seen = linked.entry(child_facts.id).or_insert((
                        0,
                        child_facts.links,
                        child_facts.size.bytes,
                    ));
                    seen.0 += 1;
                    self.tick(0);
                } else {
                    summary.bytes += child_facts.size.bytes;
                    self.tick(child_facts.size.bytes);
                }
            }
        }
        for (names, links, bytes) in linked.into_values() {
            if names >= links {
                summary.bytes += bytes;
            } else {
                summary.shared = true;
            }
        }
        Ok(Verdict::Keep(facts, summary))
    }

    fn read_facts(&self, path: &Path) -> Option<Facts> {
        self.ctx.visit(path);
        let metadata = fs::symlink_metadata(path).ok()?;
        measure::facts(path, &metadata).ok()
    }

    fn check_cancel(&self) -> std::result::Result<(), ScanError> {
        if self.cancel.is_cancelled() {
            Err(ScanError::Cancelled)
        } else {
            Ok(())
        }
    }

    fn tick(&mut self, bytes: u64) {
        self.progress.files_seen += 1;
        self.progress.bytes_seen += bytes;
        self.since_event += 1;
        if self.since_event >= PROGRESS_FILES || self.last_event.elapsed() >= PROGRESS_INTERVAL {
            (self.on_progress)(self.progress);
            self.since_event = 0;
            self.last_event = Instant::now();
        }
    }
}

/// The newer of an entry's two age timestamps (see [`policy::is_old`]).
fn newest(facts: &Facts) -> SystemTime {
    facts.times.0.max(facts.times.1)
}

/// Whole days from `newest` to `now`; none for a timestamp in the future.
fn age_days(now: SystemTime, newest: SystemTime) -> Option<u32> {
    let age = now.duration_since(newest).ok()?;
    u32::try_from(age.as_secs() / 86_400).ok()
}

fn note(out: &mut Scan, row: CatalogId, text: String, bytes: Option<u64>) {
    out.notes.push(Note {
        catalog_id: row,
        location_label: row.location_label(),
        text,
        bytes,
    });
}

/// The reason a tool was not run, as a clause after its name.
fn short(reason: NotFound) -> &'static str {
    match reason {
        NotFound::Missing => "was not found",
        NotFound::Corepack => "is managed by Corepack",
        NotFound::ScriptShim => "is not run on Windows in this version",
    }
}

#[cfg(target_os = "linux")]
#[allow(clippy::unnecessary_cast)] // `f_type` width differs between Linux targets
fn is_tmpfs(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    const TMPFS_MAGIC: i64 = 0x0102_1994;
    let Ok(c_path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: an all-zero statfs is a valid out-parameter.
    let mut stat: libc::statfs = unsafe { std::mem::zeroed() };
    // SAFETY: `c_path` is NUL-terminated and `stat` is writable.
    unsafe { libc::statfs(c_path.as_ptr(), &mut stat) == 0 && stat.f_type as i64 == TMPFS_MAGIC }
}

#[cfg(not(target_os = "linux"))]
fn is_tmpfs(_path: &Path) -> bool {
    false
}
