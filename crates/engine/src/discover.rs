//! Walkers that emit unconfirmed candidates from validated catalog roots.
//! Never deletes (phase 4). Normal Scan (phase 5) covers `user-temp` and
//! the cache rows that [`crate::roots`] lists; Deep Scan rows arrive in M5.
//! Normal Scan runs no tools and does not walk the home.

use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::measure::{self, Facts, FileId, Kind};
use crate::model::{Action, Candidate, CandidateId, Confidence, Label, Platform, Scope};
pub use crate::policy::SkipReason;
use crate::policy::{self, CatalogId, Context, RootRejection, ValidRoot};
use crate::roots;

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

/// Sent at least every 200 ms or 1,000 files, whichever comes first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Progress {
    pub files_seen: u64,
    pub bytes_seen: u64,
}

const PROGRESS_FILES: u64 = 1_000;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(200);

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ScanError {
    #[error("Space Optimiser does not scan as root")]
    RunningAsRoot,
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

#[derive(Debug)]
pub struct NormalScan {
    pub temp: Vec<RootOutcome>,
    /// One candidate per cache folder that has something in it.
    pub caches: Vec<Candidate>,
    /// Cache folders left alone, with the reason.
    pub cache_skipped: Vec<Skipped>,
    pub progress: Progress,
}

/// Normal Scan (phase 5): `user-temp`, then each cache row's folders. A
/// browser that is running, or that cannot be ruled out, makes its rows
/// "quit first".
pub fn scan_normal(
    ctx: &Context,
    cancel: &CancelToken,
    on_progress: impl FnMut(Progress),
) -> std::result::Result<NormalScan, ScanError> {
    refuse_root(ctx)?;
    let mut walk = Walk::new(ctx, cancel, on_progress);
    let temp = walk.temp_roots()?;
    let running = ctx
        .process_names()
        .map(|names| roots::running_browsers(&names, Platform::CURRENT));
    let mut caches = Vec::new();
    let mut cache_skipped = Vec::new();
    let mut seen = HashSet::new();
    for row in CACHE_ROWS {
        for found in roots::cache_roots(ctx, row, Platform::CURRENT) {
            walk.check_cancel()?;
            let root = match found.root {
                Ok(root) => root,
                Err(reason) => {
                    cache_skipped.push(Skipped {
                        display_name: found.display_name,
                        reason,
                    });
                    continue;
                }
            };
            if !seen.insert(root.id()) {
                continue;
            }
            let quit_first = found.browser.is_some_and(|b| match &running {
                Ok(running) => running.contains(&b),
                Err(_) => true,
            });
            match walk.cache(row, &root, found.display_name, quit_first)? {
                Some(Ok(candidate)) => caches.push(candidate),
                Some(Err(skipped)) => cache_skipped.push(skipped),
                None => {}
            }
        }
    }
    Ok(NormalScan {
        temp,
        caches,
        cache_skipped,
        progress: walk.progress,
    })
}

fn refuse_root(ctx: &Context) -> std::result::Result<(), ScanError> {
    #[cfg(unix)]
    if crate::guard::should_refuse(ctx.euid()) {
        return Err(ScanError::RunningAsRoot);
    }
    #[cfg(not(unix))]
    let _ = ctx;
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

/// What a top-level entry adds up to.
struct Summary {
    files: u64,
    bytes: u64,
    links: u64,
    shared: bool,
    exact: bool,
    /// Everything inside the entry, links included. The executor deletes
    /// only these.
    members: Vec<FileId>,
}

enum Verdict {
    Keep(Facts, Summary),
    Skip(SkipReason),
}

impl<'a, F: FnMut(Progress)> Walk<'a, F> {
    fn new(ctx: &'a Context, cancel: &'a CancelToken, on_progress: F) -> Self {
        Walk {
            ctx,
            cancel,
            on_progress,
            progress: Progress {
                files_seen: 0,
                bytes_seen: 0,
            },
            since_event: 0,
            last_event: Instant::now(),
            next_id: 1,
        }
    }

    fn temp_roots(&mut self) -> std::result::Result<Vec<RootOutcome>, ScanError> {
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
        let mut candidate = self.candidate(
            row,
            root,
            root.path(),
            display_name,
            facts.id,
            summary,
            Scope::Contents,
        );
        if quit_first {
            candidate.action = Action::None;
            candidate.labels.push(Label::QuitFirst);
        }
        Ok(Some(Ok(candidate)))
    }

    /// Explorer's `thumbcache_*.db` files, summed. Shown, never cleaned.
    fn thumbcache(&mut self, root: &ValidRoot, display_name: String) -> Option<Candidate> {
        self.ctx.visit(root.path());
        let mut summary = Summary {
            files: 0,
            bytes: 0,
            links: 0,
            shared: false,
            exact: true,
            members: Vec::new(),
        };
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
        Some(self.candidate(
            CatalogId::WindowsThumbcache,
            root,
            root.path(),
            display_name,
            root.id(),
            summary,
            Scope::Contents,
        ))
    }

    fn root(&mut self, root: &ValidRoot) -> std::result::Result<RootOutcome, ScanError> {
        let label = root.path().to_string_lossy().into_owned();
        let mut candidates = Vec::new();
        let mut skipped = Vec::new();
        let device = Some(root.id().device());
        self.ctx.visit(root.path());
        let entries = match fs::read_dir(root.path()) {
            Ok(entries) => entries,
            Err(_) => {
                return Ok(RootOutcome::Skipped {
                    root: label,
                    reason: RootRejection::Unreadable,
                });
            }
        };
        for entry in entries {
            self.check_cancel()?;
            let Ok(entry) = entry else { continue };
            let path = entry.path();
            let display_name = entry.file_name().to_string_lossy().into_owned();
            match self.judge(CatalogId::UserTemp, &path, device, true)? {
                Verdict::Skip(reason) => skipped.push(Skipped {
                    display_name,
                    reason,
                }),
                Verdict::Keep(facts, summary) => candidates.push(self.candidate(
                    CatalogId::UserTemp,
                    root,
                    &path,
                    display_name,
                    facts.id,
                    summary,
                    Scope::Entry,
                )),
            }
        }
        Ok(RootOutcome::Scanned {
            root: label,
            frees_memory: is_tmpfs(root.path()),
            candidates,
            skipped,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn candidate(
        &mut self,
        row: CatalogId,
        root: &ValidRoot,
        path: &Path,
        display_name: String,
        file_id: FileId,
        s: Summary,
        scope: Scope,
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
            file_count: s.files,
            links_not_followed: s.links,
            labels,
            risk: row.risk(),
            bulk_safe: row.bulk_safe(),
            confidence: Confidence::High,
            action: row.action(),
            platform: Platform::CURRENT,
            path: path.to_path_buf(),
            file_id,
            root: root.clone(),
            scope,
            members: members.into(),
        }
    }

    /// Judges one entry whole under `row`'s rules: a top-level temp entry,
    /// or a cache folder. A directory is read to unlimited depth on the same
    /// filesystem, without following links. `count_self` adds the entry's
    /// own blocks, which a cache folder keeps.
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
        if facts.kind != Kind::Dir {
            let shared = facts.links > 1;
            let bytes = if shared { 0 } else { facts.size.bytes };
            self.tick(bytes);
            return Ok(Verdict::Keep(
                facts,
                Summary {
                    files: 1,
                    bytes,
                    links: 0,
                    shared,
                    exact: facts.size.exact,
                    members: Vec::new(),
                },
            ));
        }
        let mut summary = Summary {
            files: 0,
            bytes: if count_self { facts.size.bytes } else { 0 },
            links: 0,
            shared: false,
            exact: facts.size.exact,
            members: Vec::new(),
        };
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
                if policy::is_project_marker(&entry.file_name()) {
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
                if let Some(reason) = policy::entry_rule(row, self.ctx, &child_facts, device) {
                    return Ok(Verdict::Skip(reason));
                }
                summary.members.push(child_facts.id);
                summary.exact &= child_facts.size.exact;
                if child_facts.kind == Kind::Dir {
                    summary.bytes += child_facts.size.bytes;
                    stack.push(child);
                    continue;
                }
                summary.files += 1;
                if child_facts.links > 1 {
                    summary.shared = true;
                    self.tick(0);
                } else {
                    summary.bytes += child_facts.size.bytes;
                    self.tick(child_facts.size.bytes);
                }
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
