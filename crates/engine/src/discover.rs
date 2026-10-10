//! Walkers that emit unconfirmed candidates from validated catalog roots.
//! Never deletes (phase 4). `user-temp` is the only row so far; others
//! arrive in M4 and M5.

use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::measure::{self, Facts, FileId, Kind};
use crate::model::{Candidate, CandidateId, Confidence, Label, Platform};
pub use crate::policy::SkipReason;
use crate::policy::{self, CatalogId, Context, RootRejection, ValidRoot};
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
    #[cfg(unix)]
    if crate::guard::should_refuse(ctx.euid()) {
        return Err(ScanError::RunningAsRoot);
    }
    let mut walk = Walk {
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
    };
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
                    roots.push(walk.root(&root)?);
                }
            }
        }
    }
    Ok(TempScan {
        roots,
        progress: walk.progress,
    })
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

impl<F: FnMut(Progress)> Walk<'_, F> {
    fn root(&mut self, root: &ValidRoot) -> std::result::Result<RootOutcome, ScanError> {
        let label = root.path().to_string_lossy().into_owned();
        let mut candidates = Vec::new();
        let mut skipped = Vec::new();
        let device = Some(root.id().device());
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
            match self.top_level(&path, device)? {
                Verdict::Skip(reason) => skipped.push(Skipped {
                    display_name,
                    reason,
                }),
                Verdict::Keep(facts, summary) => {
                    candidates.push(self.candidate(root, &path, display_name, facts.id, summary))
                }
            }
        }
        Ok(RootOutcome::Scanned {
            root: label,
            frees_memory: is_tmpfs(root.path()),
            candidates,
            skipped,
        })
    }

    fn candidate(
        &mut self,
        root: &ValidRoot,
        path: &Path,
        display_name: String,
        file_id: FileId,
        s: Summary,
    ) -> Candidate {
        let row = CatalogId::UserTemp;
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
            members: members.into(),
        }
    }

    /// Judges one top-level entry whole. A directory is read to unlimited
    /// depth on the same filesystem, without following links.
    fn top_level(
        &mut self,
        path: &Path,
        device: Option<u64>,
    ) -> std::result::Result<Verdict, ScanError> {
        let Some(facts) = read_facts(path) else {
            return Ok(Verdict::Skip(SkipReason::Unreadable));
        };
        if let Some(reason) = policy::temp_entry_rule(self.ctx, &facts, device) {
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
            bytes: facts.size.bytes,
            links: 0,
            shared: false,
            exact: facts.size.exact,
            members: Vec::new(),
        };
        let mut stack = vec![path.to_path_buf()];
        while let Some(dir) = stack.pop() {
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
                let Some(child_facts) = read_facts(&child) else {
                    return Ok(Verdict::Skip(SkipReason::Unreadable));
                };
                if child_facts.kind == Kind::Link {
                    // Counted, never followed or sized as its target.
                    summary.links += 1;
                    summary.members.push(child_facts.id);
                    continue;
                }
                if let Some(reason) = policy::temp_entry_rule(self.ctx, &child_facts, device) {
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

fn read_facts(path: &Path) -> Option<Facts> {
    let metadata = fs::symlink_metadata(path).ok()?;
    measure::facts(path, &metadata).ok()
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
