//! Plans, confirmation tokens, and the only delete path in the product
//! (phase 8). Every planned entry is re-checked here, immediately before
//! removal, against the root rules, its scan-time identity, and the catalog
//! entry rules. Deletion is handle-relative: the root is opened once without
//! following links and every entry below it is reached through a directory
//! handle, never through a full path.

use std::ffi::OsString;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use crate::audit::Audit;
use crate::clock::Stamp;
use crate::discover::CancelToken;
use crate::measure::FileId;
use crate::model::{
    Candidate, CandidateId, ConfirmationToken, FreeSpace, Outcome, PlanId, Report, ReportItem,
    ScanId, SkipCause,
};
use crate::policy::{self, CatalogId, Context, ValidRoot};

/// One planned entry: what the scan saw, kept in the engine.
#[derive(Debug, Clone)]
pub(crate) struct PlanEntry {
    pub candidate_id: CandidateId,
    pub catalog_id: CatalogId,
    pub display_name: String,
    root: ValidRoot,
    /// A single path component directly inside `root`.
    name: OsString,
    file_id: FileId,
    /// Read by the Unix executor; the Windows one lands in M7.
    #[cfg_attr(windows, allow(dead_code))]
    members: Arc<[FileId]>,
}

impl PlanEntry {
    pub fn from_candidate(candidate: &Candidate) -> Option<Self> {
        Some(Self {
            candidate_id: candidate.id,
            catalog_id: candidate.catalog_id,
            display_name: candidate.display_name.clone(),
            root: candidate.root.clone(),
            name: candidate.path.file_name()?.to_owned(),
            file_id: candidate.file_id,
            members: candidate.members.clone(),
        })
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
        (
            entry.catalog_id.as_str(),
            &entry.name,
            entry.file_id,
            entry.root.id(),
            entry.root.path(),
        )
            .hash(&mut hasher);
    }
    ConfirmationToken::new(format!("{:016x}", hasher.finish()))
}

/// How the executor measures free space and waits for it to settle.
pub(crate) struct Measure<'a> {
    pub free_space: &'a (dyn Fn(&Path) -> io::Result<u64> + Send + Sync),
    pub settle: Duration,
}

/// Runs every entry in order. Stop finishes the current entry and marks the
/// rest "not run". Errors are recorded and the run continues.
pub(crate) fn run(
    ctx: &Context,
    plan: PlanId,
    entries: &[PlanEntry],
    mut audit: Audit,
    stop: &CancelToken,
    measure: Measure<'_>,
    mut after_item: impl FnMut(usize),
) -> Report {
    let mut volumes: Vec<(u64, &Path, &'static str, Option<u64>)> = Vec::new();
    for entry in entries {
        let device = entry.root.id().device();
        if volumes.iter().all(|v| v.0 != device) {
            let before = (measure.free_space)(entry.root.path()).ok();
            volumes.push((
                device,
                entry.root.path(),
                entry.catalog_id.location_label(),
                before,
            ));
        }
    }

    let mut items = Vec::with_capacity(entries.len());
    for (index, entry) in entries.iter().enumerate() {
        let done = if stop.is_cancelled() {
            Done::not_run()
        } else {
            delete_entry(ctx, entry)
        };
        audit.action(
            entry.catalog_id,
            &entry.root.path().join(&entry.name),
            done.outcome,
            done.detail.as_deref(),
            done.bytes,
        );
        items.push(ReportItem {
            candidate_id: entry.candidate_id,
            catalog_id: entry.catalog_id,
            display_name: entry.display_name.clone(),
            outcome: done.outcome,
            reason: done.reason,
            detail: done.detail,
            bytes_removed: done.bytes,
            recovery_text: (done.outcome == Outcome::Removed)
                .then(|| entry.catalog_id.recovery_text())
                .flatten(),
        });
        after_item(index + 1);
    }

    let removed = items
        .iter()
        .filter(|i| i.outcome == Outcome::Removed)
        .count();
    let bytes_removed = items.iter().map(|i| i.bytes_removed).sum();
    let stopped = items.iter().any(|i| i.outcome == Outcome::NotRun);
    // APFS and other filesystems free space asynchronously.
    let delay = if bytes_removed > 0 {
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

// `removed` and `failed` come from the Unix executor until M7 adds Windows.
#[cfg_attr(windows, allow(dead_code))]
impl Done {
    fn removed(bytes: u64) -> Self {
        Self {
            outcome: Outcome::Removed,
            reason: None,
            detail: None,
            bytes,
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

fn delete_entry(ctx: &Context, entry: &PlanEntry) -> Done {
    // Only rows with an executor here can run, whatever the plan says.
    if entry.catalog_id != CatalogId::UserTemp {
        return Done::skipped(SkipCause::NotInThisBuild);
    }
    let root = match policy::validate_temp_root(ctx, entry.root.path(), entry.root.owner_rule()) {
        Ok(root) => root,
        Err(reason) => return Done::skipped(SkipCause::Root(reason)),
    };
    if root.id() != entry.root.id() || root.path() != entry.root.path() {
        return Done::skipped(SkipCause::ChangedSinceScan);
    }
    sys::delete(ctx, entry)
}

#[cfg(unix)]
mod sys {
    use super::{Done, PlanEntry};
    use crate::measure::{self, Facts, FileId, Kind, RawStat};
    use crate::model::SkipCause;
    use crate::policy::{self, Context, SkipReason};
    use rustix::fd::{AsFd, OwnedFd};
    use rustix::fs::{AtFlags, CWD, Dir, Mode, OFlags};
    use rustix::io::Errno;
    use std::ffi::{OsStr, OsString};
    use std::io;
    use std::os::unix::ffi::OsStrExt;

    pub fn delete(ctx: &Context, entry: &PlanEntry) -> Done {
        let root_fd = match open_dir(CWD, entry.root.path().as_os_str()) {
            Ok(fd) => fd,
            Err(Errno::LOOP | Errno::NOTDIR) => return Done::skipped(SkipCause::ChangedSinceScan),
            Err(e) => return Done::failed(io::Error::from(e), 0),
        };
        match fstat(&root_fd) {
            Ok(facts) if facts.id == entry.root.id() => {}
            Ok(_) => return Done::skipped(SkipCause::ChangedSinceScan),
            Err(e) => return Done::failed(io::Error::from(e), 0),
        }
        let device = Some(entry.root.id().device());
        let top = match stat_at(&root_fd, &entry.name) {
            Ok(facts) => facts,
            Err(Errno::NOENT) => return Done::skipped(SkipCause::Gone),
            Err(e) => return Done::failed(io::Error::from(e), 0),
        };
        if top.id != entry.file_id {
            return Done::skipped(SkipCause::ChangedSinceScan);
        }
        if let Some(reason) = policy::temp_entry_rule(ctx, &top, device) {
            return Done::skipped(SkipCause::Entry(reason));
        }
        if top.kind != Kind::Dir {
            let bytes = if top.links > 1 { 0 } else { top.size.bytes };
            return match rustix::fs::unlinkat(&root_fd, &entry.name, AtFlags::empty()) {
                Ok(()) => Done::removed(bytes),
                Err(e) => Done::failed(io::Error::from(e), 0),
            };
        }

        let dir_fd = match open_child(&root_fd, &entry.name, top.id) {
            Ok(fd) => fd,
            Err(cause) => return Done::skipped(cause),
        };
        let mut tree = Tree {
            ctx,
            members: &entry.members,
            device,
            bytes: 0,
            error: None,
        };
        if let Err(cause) = tree.verify(&dir_fd) {
            return Done::skipped(cause);
        }
        tree.empty(&dir_fd);
        drop(dir_fd);
        if let Some(error) = tree.error {
            return Done::failed(error, tree.bytes);
        }
        match rustix::fs::unlinkat(&root_fd, &entry.name, AtFlags::REMOVEDIR) {
            Ok(()) => Done::removed(tree.bytes + top.size.bytes),
            Err(e) => Done::failed(io::Error::from(e), tree.bytes),
        }
    }

    /// A planned directory, walked through handles only.
    struct Tree<'a> {
        ctx: &'a Context,
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
                if policy::is_project_marker(&name) {
                    return Err(SkipCause::Entry(SkipReason::Project));
                }
                let facts = stat_at(dir, &name).map_err(|_| unreadable)?;
                if facts.kind != Kind::Link {
                    if let Some(reason) = policy::temp_entry_rule(self.ctx, &facts, self.device) {
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

/// The handle-relative Windows delete lands in M7 (phase 9). The engine
/// refuses to execute on Windows before reaching this.
#[cfg(windows)]
mod sys {
    use super::{Done, PlanEntry};
    use crate::model::SkipCause;
    use crate::policy::Context;

    pub fn delete(_ctx: &Context, _entry: &PlanEntry) -> Done {
        Done::skipped(SkipCause::NotInThisBuild)
    }
}

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
            catalog_id: CatalogId::BrowserHttpCache,
            display_name: "cache.bin".into(),
            root,
            name: "cache.bin".into(),
            file_id: crate::measure::file_id(&file).unwrap(),
            members: Arc::from([]),
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
        );
        assert_eq!(report.items[0].outcome, Outcome::Skipped);
        assert_eq!(report.items[0].reason, Some(SkipCause::NotInThisBuild));
        assert!(file.exists());
    }
}
