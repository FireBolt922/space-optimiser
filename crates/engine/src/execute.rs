//! Plans, confirmation tokens, and the only delete path in the product
//! (phase 8). Every planned entry is re-checked here, immediately before
//! removal, against the root rules, its scan-time identity, and the catalog
//! entry rules. Deletion is handle-relative: the root is opened once without
//! following links and every entry below it is reached through a directory
//! handle, never through a full path.
//!
//! Three shapes of plan entry exist. A `user-temp` or `user-logs-old` entry
//! is one name directly inside its root, removed whole. A cache entry is the
//! cache folder itself: everything inside it is removed and the folder
//! stays. A tool entry runs the row's fixed clean command through
//! [`crate::tool`], after finding the same tool again (phase 8 item 4).

use std::ffi::OsString;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::audit::{self, Audit};
use crate::clock::Stamp;
use crate::discover::CancelToken;
use crate::docker;
use crate::measure::{self, FileId};
use crate::model::{
    Candidate, CandidateId, ConfirmationToken, FreeSpace, Outcome, PlanId, Platform, Report,
    ReportItem, ScanId, Scope, SkipCause, ToolAction,
};
use crate::policy::{self, CatalogId, Context, ValidRoot};
use crate::roots;
use crate::tool::{self, Exit, LineSink};

/// Whether this build has a handle-relative delete executor for the target
/// platform. The engine refuses to execute a plan when it is false. Unix
/// uses `rustix`; Windows uses `NtCreateFile` handles (`execute/win.rs`).
pub(crate) const SUPPORTED: bool = cfg!(any(unix, windows));

/// One planned entry: what the scan saw, kept in the engine.
#[derive(Debug, Clone)]
pub(crate) struct PlanEntry {
    pub candidate_id: CandidateId,
    pub catalog_id: CatalogId,
    pub display_name: String,
    work: Work,
}

#[derive(Debug, Clone)]
enum Work {
    Delete {
        root: ValidRoot,
        /// A single path component directly inside `root`, or `None` for
        /// the contents of `root` itself.
        name: Option<OsString>,
        file_id: FileId,
        /// Sorted identities of everything inside, as the scan saw them.
        members: Arc<[FileId]>,
    },
    Tool {
        action: ToolAction,
        /// Where free space is measured: the cache folder, or the home.
        measure_at: PathBuf,
        /// The cache folder the tool reported at scan time. None for Docker,
        /// whose cache is inside its own disk.
        cache: Option<FileId>,
    },
}

impl PlanEntry {
    pub fn from_candidate(candidate: &Candidate) -> Option<Self> {
        let work = match candidate.scope {
            Scope::Entry | Scope::Contents => Work::Delete {
                root: candidate.root.clone()?,
                name: match candidate.scope {
                    Scope::Entry => Some(candidate.path.file_name()?.to_owned()),
                    _ => None,
                },
                file_id: candidate.file_id,
                members: candidate.members.clone(),
            },
            Scope::Tool => Work::Tool {
                action: candidate.tool.clone()?,
                cache: candidate.root.as_ref().map(ValidRoot::id),
                measure_at: candidate
                    .root
                    .as_ref()
                    .map_or_else(|| candidate.path.clone(), |r| r.path().to_path_buf()),
            },
            Scope::Report => return None,
        };
        Some(Self {
            candidate_id: candidate.id,
            catalog_id: candidate.catalog_id,
            display_name: candidate.display_name.clone(),
            work,
        })
    }

    /// The full path, or the tool's command line, for the audit log only.
    fn target(&self) -> String {
        match &self.work {
            Work::Delete {
                root,
                name: Some(name),
                ..
            } => audit::escaped(&root.path().join(name)),
            Work::Delete { root, .. } => audit::escaped(root.path()),
            Work::Tool { action, .. } => {
                let mut line = audit::escaped(&action.tool.path);
                for arg in action.args {
                    line.push(' ');
                    line.push_str(arg);
                }
                line
            }
        }
    }

    /// Where free space is measured for this entry, and its volume.
    fn volume(&self) -> (&Path, Option<u64>) {
        match &self.work {
            Work::Delete { root, .. } => (root.path(), Some(root.id().device())),
            Work::Tool { measure_at, .. } => (
                measure_at,
                measure::file_id(measure_at).ok().map(|id| id.device()),
            ),
        }
    }
}

/// The stale-plan guard (phase 2 item 13): a digest of exactly what was
/// planned. It is not a security boundary; re-validation below is.
pub(crate) fn token(
    plan: PlanId,
    scan: ScanId,
    entries: &[PlanEntry],
    built: Stamp,
) -> ConfirmationToken {
    let mut hasher = DefaultHasher::new();
    (plan, scan, built.awake, built.total).hash(&mut hasher);
    for entry in entries {
        entry.catalog_id.as_str().hash(&mut hasher);
        match &entry.work {
            Work::Delete {
                root,
                name,
                file_id,
                ..
            } => (name, file_id, root.id(), root.path()).hash(&mut hasher),
            Work::Tool { action, .. } => {
                (&action.tool.canonical, action.tool.id, action.args).hash(&mut hasher)
            }
        }
    }
    ConfirmationToken::new(format!("{:016x}", hasher.finish()))
}

/// How the executor measures free space and waits for it to settle.
pub(crate) struct Measure<'a> {
    pub free_space: &'a (dyn Fn(&Path) -> io::Result<u64> + Send + Sync),
    pub settle: Duration,
}

/// What the executor says while it runs, for the Cleaning screen.
pub(crate) enum RunEvent<'a> {
    /// Entry `index` is starting.
    Started { index: usize, entry: &'a PlanEntry },
    /// Entry `index` is done; `item` is its report line.
    Finished { index: usize, item: &'a ReportItem },
}

/// Receives each line a tool prints, with the index of its entry. Called
/// from the tool's pipe reader threads.
pub(crate) type OutputSink = Arc<dyn Fn(usize, String) + Send + Sync>;

/// Runs every entry in order. Stop finishes the current entry and marks the
/// rest "not run". Errors are recorded and the run continues.
#[allow(clippy::too_many_arguments)]
pub(crate) fn run(
    ctx: &Context,
    plan: PlanId,
    entries: &[PlanEntry],
    mut audit: Audit,
    stop: &CancelToken,
    measure: Measure<'_>,
    mut on_event: impl FnMut(RunEvent<'_>),
    output: Option<OutputSink>,
) -> Report {
    let mut volumes: Vec<(Option<u64>, &Path, &'static str, Option<u64>)> = Vec::new();
    for entry in entries {
        let (path, device) = entry.volume();
        if volumes.iter().all(|v| device.is_none() || v.0 != device) {
            let before = (measure.free_space)(path).ok();
            volumes.push((device, path, entry.catalog_id.location_label(), before));
        }
    }

    let mut items = Vec::with_capacity(entries.len());
    for (index, entry) in entries.iter().enumerate() {
        let done = if stop.is_cancelled() {
            Done::not_run()
        } else {
            on_event(RunEvent::Started { index, entry });
            match &entry.work {
                Work::Delete { .. } => delete_entry(ctx, entry),
                Work::Tool { action, cache, .. } => {
                    let lines = output
                        .clone()
                        .map(|output| -> LineSink { Arc::new(move |line| output(index, line)) });
                    run_tool(ctx, entry.catalog_id, action, *cache, lines)
                }
            }
        };
        audit.action(
            entry.catalog_id,
            &entry.target(),
            done.outcome,
            done.detail.as_deref(),
            done.bytes,
        );
        let finished = matches!(done.outcome, Outcome::Removed | Outcome::Ran);
        let hint = (done.reason == Some(SkipCause::ToolBusy))
            .then(|| entry.catalog_id.busy_hint())
            .flatten();
        items.push(ReportItem {
            candidate_id: entry.candidate_id,
            catalog_id: entry.catalog_id,
            display_name: entry.display_name.clone(),
            outcome: done.outcome,
            reason: done.reason,
            detail: done.detail,
            bytes_removed: done.bytes,
            recovery_text: finished.then(|| entry.catalog_id.recovery_text()).flatten(),
            hint,
        });
        on_event(RunEvent::Finished {
            index,
            item: &items[index],
        });
    }

    let removed = items
        .iter()
        .filter(|i| matches!(i.outcome, Outcome::Removed | Outcome::Ran))
        .count();
    let bytes_removed = items.iter().map(|i| i.bytes_removed).sum();
    let ran_tool = items.iter().any(|i| i.outcome == Outcome::Ran);
    let stopped = items.iter().any(|i| i.outcome == Outcome::NotRun);
    // APFS and other filesystems free space asynchronously.
    let delay = if bytes_removed > 0 || ran_tool {
        std::thread::sleep(measure.settle);
        measure.settle
    } else {
        Duration::ZERO
    };
    let free_space = volumes
        .into_iter()
        .map(|(_, root, label, before)| FreeSpace {
            location_label: label,
            before,
            after: (measure.free_space)(root).ok(),
            after_delay_secs: delay.as_secs(),
            source: "free space reported by the system",
        })
        .collect();
    let (audit_log, memory_log) = audit.finish(removed, bytes_removed, stopped);
    Report {
        plan_id: plan,
        items,
        bytes_removed,
        stopped,
        free_space,
        audit_log,
        memory_log,
    }
}

/// The outcome of one planned entry.
#[derive(Debug)]
struct Done {
    outcome: Outcome,
    reason: Option<SkipCause>,
    detail: Option<String>,
    bytes: u64,
}

impl Done {
    fn removed(bytes: u64) -> Self {
        Self {
            outcome: Outcome::Removed,
            reason: None,
            detail: None,
            bytes,
        }
    }

    fn ran() -> Self {
        Self {
            outcome: Outcome::Ran,
            reason: None,
            detail: None,
            bytes: 0,
        }
    }

    fn skipped(cause: SkipCause) -> Self {
        Self {
            outcome: Outcome::Skipped,
            reason: Some(cause),
            detail: Some(cause.to_string()),
            bytes: 0,
        }
    }

    fn failed(error: impl ToString, bytes: u64) -> Self {
        Self {
            outcome: Outcome::Failed,
            reason: None,
            detail: Some(error.to_string()),
            bytes,
        }
    }

    fn not_run() -> Self {
        Self {
            outcome: Outcome::NotRun,
            reason: None,
            detail: None,
            bytes: 0,
        }
    }
}

/// A tool row's clean command, only as the catalog fixes it: the row's own
/// arguments, the same tool file the scan found, the same cache folder the
/// tool reported at scan time (asked again now), and for Docker a local
/// endpoint checked again now.
fn run_tool(
    ctx: &Context,
    row: CatalogId,
    action: &ToolAction,
    cache: Option<FileId>,
    lines: Option<LineSink>,
) -> Done {
    let Some((args, stop_on)) = crate::discover::tool_action(row) else {
        return Done::skipped(SkipCause::NotInThisBuild);
    };
    if action.args != args || !tool::refind(ctx, &action.tool) {
        return Done::skipped(SkipCause::ChangedSinceScan);
    }
    if let Some(planned) = cache {
        let now = crate::discover::tool_cache_root(ctx, row, &action.tool);
        if now.is_none_or(|root| root.id() != planned) {
            return Done::skipped(SkipCause::ChangedSinceScan);
        }
    }
    if row == CatalogId::DockerBuildCache && docker::check_local(ctx, &action.tool).is_err() {
        return Done::skipped(SkipCause::DockerNotLocal);
    }
    let timeout = ctx.tools().action_timeout;
    if let Some(lines) = &lines {
        lines(format!(
            "Running {} {}",
            action.tool.path.display(),
            args.join(" ")
        ));
    }
    match tool::run_with_lines(ctx, &action.tool, args, timeout, stop_on, lines, None) {
        Ok(out) => match out.exit {
            Exit::Success => Done::ran(),
            Exit::Stopped(_) => Done::skipped(SkipCause::ToolBusy),
            // The executor passes no cancel token; Stop waits for the item.
            Exit::TimedOut | Exit::Cancelled => Done::failed(
                format!(
                    "{} did not finish in time and was stopped",
                    action.tool.name
                ),
                0,
            ),
            Exit::Failed(code) if out.stderr.is_empty() => Done::failed(
                match code {
                    Some(code) => format!("{} failed with exit code {code}", action.tool.name),
                    None => format!("{} was stopped by the system", action.tool.name),
                },
                0,
            ),
            Exit::Failed(_) => Done::failed(out.stderr, 0),
        },
        Err(e) => Done::failed(e, 0),
    }
}

fn delete_entry(ctx: &Context, entry: &PlanEntry) -> Done {
    let Work::Delete {
        root: planned,
        name,
        ..
    } = &entry.work
    else {
        return Done::skipped(SkipCause::NotInThisBuild);
    };
    // Only rows with an executor here can run, whatever the plan says, and
    // only in the shape the row allows.
    match (entry.catalog_id, name) {
        (CatalogId::UserTemp, Some(_)) => {
            let path = planned.path();
            let root = match policy::validate_temp_root(ctx, path, planned.owner_rule()) {
                Ok(root) => root,
                Err(reason) => return Done::skipped(SkipCause::Root(reason)),
            };
            if root.id() != planned.id() || root.path() != planned.path() {
                return Done::skipped(SkipCause::ChangedSinceScan);
            }
        }
        (CatalogId::UserLogsOld, Some(_))
        | (
            CatalogId::PkgNpx
            | CatalogId::PkgCargoCache
            | CatalogId::PkgGradleCaches
            | CatalogId::PkgNugetHttp,
            None,
        ) => {
            if let Err(cause) = roots::recheck(ctx, entry.catalog_id, planned) {
                return Done::skipped(cause);
            }
        }
        (
            CatalogId::ThumbnailCache
            | CatalogId::FontCache
            | CatalogId::BrowserHttpCache
            | CatalogId::IdeIndexCaches,
            None,
        ) => {
            let app = match roots::recheck(ctx, entry.catalog_id, planned) {
                Ok(app) => app,
                Err(cause) => return Done::skipped(cause),
            };
            if let Some(app) = app {
                match ctx.process_names() {
                    Err(_) => return Done::skipped(SkipCause::AppCheckFailed),
                    Ok(names) => {
                        if roots::running_apps(&names, Platform::CURRENT).contains(&app) {
                            return Done::skipped(SkipCause::AppRunning);
                        }
                    }
                }
            }
        }
        _ => return Done::skipped(SkipCause::NotInThisBuild),
    }
    sys::delete(ctx, entry)
}

#[cfg(unix)]
mod sys {
    use super::{Done, PlanEntry, Work};
    use crate::measure::{self, Facts, FileId, Kind, RawStat};
    use crate::model::SkipCause;
    use crate::policy::{self, CatalogId, Context, SkipReason};
    use rustix::fd::{AsFd, OwnedFd};
    use rustix::fs::{AtFlags, CWD, Dir, Mode, OFlags};
    use rustix::io::Errno;
    use std::ffi::{OsStr, OsString};
    use std::io;
    use std::os::unix::ffi::OsStrExt;

    pub fn delete(ctx: &Context, entry: &PlanEntry) -> Done {
        let row = entry.catalog_id;
        let Work::Delete {
            root,
            name,
            file_id,
            members,
        } = &entry.work
        else {
            return Done::skipped(SkipCause::NotInThisBuild);
        };
        let root_fd = match open_dir(CWD, root.path().as_os_str()) {
            Ok(fd) => fd,
            Err(Errno::LOOP | Errno::NOTDIR) => return Done::skipped(SkipCause::ChangedSinceScan),
            Err(e) => return Done::failed(io::Error::from(e), 0),
        };
        let root_facts = match fstat(&root_fd) {
            Ok(facts) if facts.id == root.id() => facts,
            Ok(_) => return Done::skipped(SkipCause::ChangedSinceScan),
            Err(e) => return Done::failed(io::Error::from(e), 0),
        };
        let device = Some(root.id().device());
        let mut tree = Tree {
            ctx,
            row,
            members,
            device,
            bytes: 0,
            error: None,
        };
        let Some(name) = name else {
            // A cache folder: empty it, keep it.
            if *file_id != root_facts.id {
                return Done::skipped(SkipCause::ChangedSinceScan);
            }
            if let Some(reason) = policy::entry_rule(row, ctx, &root_facts, device) {
                return Done::skipped(SkipCause::Entry(reason));
            }
            if let Err(cause) = tree.verify(&root_fd) {
                return Done::skipped(cause);
            }
            tree.empty(&root_fd);
            return match tree.error {
                Some(error) => Done::failed(error, tree.bytes),
                None => Done::removed(tree.bytes),
            };
        };
        let top = match stat_at(&root_fd, name) {
            Ok(facts) => facts,
            Err(Errno::NOENT) => return Done::skipped(SkipCause::Gone),
            Err(e) => return Done::failed(io::Error::from(e), 0),
        };
        if top.id != *file_id {
            return Done::skipped(SkipCause::ChangedSinceScan);
        }
        if let Some(reason) = policy::entry_rule(row, ctx, &top, device)
            .or_else(|| policy::name_rule(row, name, top.kind))
        {
            return Done::skipped(SkipCause::Entry(reason));
        }
        if top.kind != Kind::Dir {
            let bytes = if top.links > 1 { 0 } else { top.size.bytes };
            return match rustix::fs::unlinkat(&root_fd, name, AtFlags::empty()) {
                Ok(()) => Done::removed(bytes),
                Err(e) => Done::failed(io::Error::from(e), 0),
            };
        }

        let dir_fd = match open_child(&root_fd, name, top.id) {
            Ok(fd) => fd,
            Err(cause) => return Done::skipped(cause),
        };
        if let Err(cause) = tree.verify(&dir_fd) {
            return Done::skipped(cause);
        }
        tree.empty(&dir_fd);
        drop(dir_fd);
        if let Some(error) = tree.error {
            return Done::failed(error, tree.bytes);
        }
        match rustix::fs::unlinkat(&root_fd, name, AtFlags::REMOVEDIR) {
            Ok(()) => Done::removed(tree.bytes + top.size.bytes),
            Err(e) => Done::failed(io::Error::from(e), tree.bytes),
        }
    }

    /// A planned directory, walked through handles only.
    struct Tree<'a> {
        ctx: &'a Context,
        /// Whose entry rules apply.
        row: CatalogId,
        /// Sorted identities recorded by the scan.
        members: &'a [FileId],
        device: Option<u64>,
        bytes: u64,
        /// The first error; the walk continues past it.
        error: Option<String>,
    }

    impl Tree<'_> {
        fn is_member(&self, id: FileId) -> bool {
            self.members.binary_search(&id).is_ok()
        }

        /// The catalog rules on every descendant, immediately before the
        /// delete pass. Anything the scan did not see skips the whole entry.
        fn verify(&self, dir: &OwnedFd) -> Result<(), SkipCause> {
            let unreadable = SkipCause::Entry(SkipReason::Unreadable);
            for name in list(dir).map_err(|_| unreadable)? {
                if self.row.checks_project_markers() && policy::is_project_marker(&name) {
                    return Err(SkipCause::Entry(SkipReason::Project));
                }
                let facts = stat_at(dir, &name).map_err(|_| unreadable)?;
                if facts.kind != Kind::Link {
                    if let Some(reason) =
                        policy::entry_rule(self.row, self.ctx, &facts, self.device)
                            .or_else(|| policy::name_rule(self.row, &name, facts.kind))
                    {
                        return Err(SkipCause::Entry(reason));
                    }
                }
                if !self.is_member(facts.id) {
                    return Err(SkipCause::ChangedSinceScan);
                }
                if facts.kind == Kind::Dir {
                    self.verify(&open_child(dir, &name, facts.id)?)?;
                }
            }
            Ok(())
        }

        /// Removes everything below `dir`, deepest first. The verify pass has
        /// just applied the rules; this pass re-checks identity before every
        /// unlink, so nothing swapped in between is touched. Links are
        /// removed, never followed.
        fn empty(&mut self, dir: &OwnedFd) {
            let names = match list(dir) {
                Ok(names) => names,
                Err(e) => return self.fail(e),
            };
            for name in names {
                let facts = match stat_at(dir, &name) {
                    Ok(facts) => facts,
                    Err(e) => {
                        self.fail(e);
                        continue;
                    }
                };
                let allowed = matches!(facts.kind, Kind::File | Kind::Dir | Kind::Link);
                if !allowed || !self.is_member(facts.id) {
                    self.fail_with("it changed during the cleanup");
                    continue;
                }
                if facts.kind == Kind::Dir {
                    match open_child(dir, &name, facts.id) {
                        Ok(child) => self.empty(&child),
                        Err(_) => {
                            self.fail_with("it changed during the cleanup");
                            continue;
                        }
                    }
                    match rustix::fs::unlinkat(dir, &name, AtFlags::REMOVEDIR) {
                        Ok(()) => self.bytes += facts.size.bytes,
                        Err(e) => self.fail(e),
                    }
                } else {
                    let bytes = if facts.links > 1 { 0 } else { facts.size.bytes };
                    match rustix::fs::unlinkat(dir, &name, AtFlags::empty()) {
                        Ok(()) => self.bytes += bytes,
                        Err(e) => self.fail(e),
                    }
                }
            }
        }

        fn fail(&mut self, error: Errno) {
            self.fail_with(&io::Error::from(error).to_string());
        }

        fn fail_with(&mut self, error: &str) {
            self.error.get_or_insert_with(|| error.to_owned());
        }
    }

    fn open_dir(dir: impl AsFd, name: &OsStr) -> Result<OwnedFd, Errno> {
        rustix::fs::openat(
            dir,
            name,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
    }

    /// Opens a child directory and checks it is the one just `stat`ed.
    fn open_child(dir: &OwnedFd, name: &OsStr, id: FileId) -> Result<OwnedFd, SkipCause> {
        let fd = open_dir(dir, name).map_err(|_| SkipCause::ChangedSinceScan)?;
        match fstat(&fd) {
            Ok(facts) if facts.id == id => Ok(fd),
            _ => Err(SkipCause::ChangedSinceScan),
        }
    }

    fn list(dir: &OwnedFd) -> Result<Vec<OsString>, Errno> {
        let mut names = Vec::new();
        for entry in Dir::read_from(dir)? {
            let entry = entry?;
            let name = entry.file_name().to_bytes();
            if name != b"." && name != b".." {
                names.push(OsStr::from_bytes(name).to_owned());
            }
        }
        Ok(names)
    }

    fn stat_at(dir: impl AsFd, name: &OsStr) -> Result<Facts, Errno> {
        rustix::fs::statat(dir, name, AtFlags::SYMLINK_NOFOLLOW).map(|st| facts(&st))
    }

    fn fstat(fd: &OwnedFd) -> Result<Facts, Errno> {
        rustix::fs::fstat(fd).map(|st| facts(&st))
    }

    #[allow(clippy::unnecessary_cast)] // `stat` field widths differ between targets
    fn facts(st: &rustix::fs::Stat) -> Facts {
        measure::facts_from_stat(&RawStat {
            mode: st.st_mode as u32,
            device: st.st_dev as u64,
            inode: st.st_ino as u64,
            links: st.st_nlink as u64,
            uid: st.st_uid as u32,
            blocks: st.st_blocks as u64,
            mtime: (st.st_mtime as i64, st.st_mtime_nsec as i64),
            ctime: (st.st_ctime as i64, st.st_ctime_nsec as i64),
            #[cfg(target_os = "macos")]
            flags: st.st_flags as u32,
        })
    }
}

/// The same walk on Windows, through `NtCreateFile` handles opened relative
/// to their parent and `SetFileInformationByHandle` deletes (phase 9).
#[cfg(windows)]
mod win;
#[cfg(windows)]
use win as sys;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::Audit;
    use crate::policy::OwnerRule;

    // Test 6: a row without an executor deletes nothing, whatever the plan.
    #[test]
    fn a_row_without_an_executor_deletes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let home = std::fs::canonicalize(dir.path()).unwrap().join("home");
        let tmp = home.join("tmp");
        std::fs::create_dir_all(&tmp).unwrap();
        let file = tmp.join("cache.bin");
        std::fs::write(&file, b"cache").unwrap();
        let ctx = Context::for_fixture(&home)
            .with_now(std::time::SystemTime::now() + Duration::from_secs(7 * 86_400))
            .with_temp_root(tmp.clone(), OwnerRule::Root);
        let root = policy::validate_temp_root(&ctx, &tmp, OwnerRule::Root).unwrap();
        let entry = PlanEntry {
            candidate_id: CandidateId::new(1),
            catalog_id: CatalogId::LargeFilesReport,
            display_name: "cache.bin".into(),
            work: Work::Delete {
                root,
                name: Some("cache.bin".into()),
                file_id: crate::measure::file_id(&file).unwrap(),
                members: Arc::from([]),
            },
        };
        let audit = Audit::start(&dir.path().join("audit"), 1, false).unwrap();
        let report = run(
            &ctx,
            PlanId::new(1),
            &[entry],
            audit,
            &CancelToken::new(),
            Measure {
                free_space: &|_| Ok(0),
                settle: Duration::ZERO,
            },
            |_| {},
            None,
        );
        assert_eq!(report.items[0].outcome, Outcome::Skipped);
        assert_eq!(report.items[0].reason, Some(SkipCause::NotInThisBuild));
        assert!(file.exists());
    }

    // A row runs only in its own shape: a cache row never removes a single
    // named entry, and `user-temp` never empties a whole root.
    #[test]
    fn a_plan_entry_in_the_wrong_shape_deletes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let home = std::fs::canonicalize(dir.path()).unwrap().join("home");
        let tmp = home.join("tmp");
        std::fs::create_dir_all(&tmp).unwrap();
        let file = tmp.join("cache.bin");
        std::fs::write(&file, b"cache").unwrap();
        let ctx = Context::for_fixture(&home)
            .with_now(std::time::SystemTime::now() + Duration::from_secs(7 * 86_400))
            .with_temp_root(tmp.clone(), OwnerRule::Root);
        let root = policy::validate_temp_root(&ctx, &tmp, OwnerRule::Root).unwrap();
        let entry = |catalog_id, name: Option<OsString>| PlanEntry {
            candidate_id: CandidateId::new(1),
            catalog_id,
            display_name: "cache.bin".into(),
            work: Work::Delete {
                root: root.clone(),
                name,
                file_id: crate::measure::file_id(&file).unwrap(),
                members: Arc::from([]),
            },
        };
        let entries = [
            entry(CatalogId::BrowserHttpCache, Some("cache.bin".into())),
            entry(CatalogId::UserTemp, None),
        ];
        let audit = Audit::start(&dir.path().join("audit"), 2, false).unwrap();
        let report = run(
            &ctx,
            PlanId::new(1),
            &entries,
            audit,
            &CancelToken::new(),
            Measure {
                free_space: &|_| Ok(0),
                settle: Duration::ZERO,
            },
            |_| {},
            None,
        );
        for item in &report.items {
            assert_eq!(item.reason, Some(SkipCause::NotInThisBuild));
        }
        assert!(file.exists());
    }
}
