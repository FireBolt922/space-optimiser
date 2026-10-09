//! Walkers that emit unconfirmed candidates from validated catalog roots.
//! Never deletes (phase 4). M2 discovers `user-temp`; other rows arrive in M4
//! and M5. `start_scan` stays unsupported until M3 wires it to the UI.

use std::collections::HashSet;
use std::fs::{self, Metadata};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::measure::{self, FileId};
use crate::model::{Candidate, CandidateId, Confidence, Label, Platform, ScanId, ScanMode};
use crate::policy::{self, CatalogId, Context, RootRejection, ValidRoot};
use crate::{EngineError, Result};

pub(crate) fn start(_mode: ScanMode) -> Result<ScanId> {
    Err(EngineError::Unsupported)
}

pub(crate) fn cancel(_scan: ScanId) -> Result<()> {
    Err(EngineError::Unsupported)
}

pub(crate) fn candidates(_scan: ScanId) -> Result<Vec<Candidate>> {
    Err(EngineError::Unsupported)
}

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

/// Why a top-level entry was left alone. The whole entry is skipped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    /// A symlink, junction, or other reparse point. Never followed.
    Link,
    /// The entry or something inside it changed within the age limit.
    Fresh,
    /// Contains a socket or FIFO (an agent, editor, or X11 connection).
    SocketOrPipe,
    /// A device file.
    SpecialFile,
    /// Contains a project marker such as `.git`: looks like a project.
    Project,
    /// Something inside could not be read.
    Unreadable,
    /// Something inside belongs to another user.
    OtherOwner,
    /// Read-only (Windows) or immutable (macOS). Attributes are never cleared.
    Locked,
    /// Is or contains a protected folder.
    ProtectedFolder,
    /// Crosses into another filesystem.
    OtherFilesystem,
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
    Ok(TempScan { roots })
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
}

enum Verdict {
    Keep(Summary),
    Skip(SkipReason),
}

impl<F: FnMut(Progress)> Walk<'_, F> {
    fn root(&mut self, root: &ValidRoot) -> std::result::Result<RootOutcome, ScanError> {
        let label = root.path().to_string_lossy().into_owned();
        let mut candidates = Vec::new();
        let mut skipped = Vec::new();
        let device = fs::metadata(root.path()).ok().map(|m| device_of(&m));
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
                Verdict::Keep(summary) => match measure::file_id(&path) {
                    Ok(file_id) => {
                        candidates.push(self.candidate(&path, display_name, file_id, summary))
                    }
                    Err(_) => skipped.push(Skipped {
                        display_name,
                        reason: SkipReason::Unreadable,
                    }),
                },
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
        }
    }

    /// Judges one top-level entry whole. A directory is read to unlimited
    /// depth on the same filesystem, without following links.
    fn top_level(
        &mut self,
        path: &Path,
        device: Option<u64>,
    ) -> std::result::Result<Verdict, ScanError> {
        let Ok(metadata) = fs::symlink_metadata(path) else {
            return Ok(Verdict::Skip(SkipReason::Unreadable));
        };
        if is_link(&metadata) {
            return Ok(Verdict::Skip(SkipReason::Link));
        }
        if let Some(reason) = self.entry_rule(path, &metadata, device) {
            return Ok(Verdict::Skip(reason));
        }
        let size = measure::allocated_size(path, &metadata);
        if !metadata.is_dir() {
            let Ok(info) = measure::entry_info(path, &metadata) else {
                return Ok(Verdict::Skip(SkipReason::Unreadable));
            };
            let shared = info.links > 1;
            let bytes = if shared { 0 } else { size.bytes };
            self.tick(bytes);
            return Ok(Verdict::Keep(Summary {
                files: 1,
                bytes,
                links: 0,
                shared,
                exact: size.exact,
            }));
        }
        let mut summary = Summary {
            files: 0,
            bytes: size.bytes,
            links: 0,
            shared: false,
            exact: size.exact,
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
                let Ok(metadata) = fs::symlink_metadata(&child) else {
                    return Ok(Verdict::Skip(SkipReason::Unreadable));
                };
                if is_link(&metadata) {
                    // Counted, never followed or sized as its target.
                    summary.links += 1;
                    continue;
                }
                if let Some(reason) = self.entry_rule(&child, &metadata, device) {
                    return Ok(Verdict::Skip(reason));
                }
                let size = measure::allocated_size(&child, &metadata);
                summary.exact &= size.exact;
                if metadata.is_dir() {
                    summary.bytes += size.bytes;
                    stack.push(child);
                    continue;
                }
                let Ok(info) = measure::entry_info(&child, &metadata) else {
                    return Ok(Verdict::Skip(SkipReason::Unreadable));
                };
                summary.files += 1;
                if info.links > 1 {
                    summary.shared = true;
                    self.tick(0);
                } else {
                    summary.bytes += size.bytes;
                    self.tick(size.bytes);
                }
            }
        }
        Ok(Verdict::Keep(summary))
    }

    /// The catalog entry rules, checked on the entry and every descendant.
    fn entry_rule(
        &self,
        path: &Path,
        metadata: &Metadata,
        device: Option<u64>,
    ) -> Option<SkipReason> {
        if let Some(reason) = special_kind(metadata) {
            return Some(reason);
        }
        if device.is_some_and(|d| d != device_of(metadata)) {
            return Some(SkipReason::OtherFilesystem);
        }
        if !owned_by_user(self.ctx, metadata) {
            return Some(SkipReason::OtherOwner);
        }
        if is_locked(metadata) {
            return Some(SkipReason::Locked);
        }
        match measure::age_times(metadata) {
            Ok((a, b)) if policy::is_old(a, b, self.ctx.now(), policy::TEMP_MIN_AGE) => {}
            Ok(_) => return Some(SkipReason::Fresh),
            Err(_) => return Some(SkipReason::Unreadable),
        }
        if metadata.is_dir() {
            match measure::entry_info(path, metadata) {
                Ok(info) if self.ctx.is_protected_id(info.id) => {
                    return Some(SkipReason::ProtectedFolder);
                }
                Ok(_) => {}
                Err(_) => return Some(SkipReason::Unreadable),
            }
        }
        None
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

#[cfg(unix)]
fn is_link(metadata: &Metadata) -> bool {
    metadata.file_type().is_symlink()
}

/// Symlinks, junctions, mount points, cloud placeholders, and app execution
/// aliases are all reparse points.
#[cfg(windows)]
fn is_link(metadata: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
    metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(unix)]
fn special_kind(metadata: &Metadata) -> Option<SkipReason> {
    use std::os::unix::fs::FileTypeExt;
    let kind = metadata.file_type();
    if kind.is_socket() || kind.is_fifo() {
        Some(SkipReason::SocketOrPipe)
    } else if kind.is_block_device() || kind.is_char_device() {
        Some(SkipReason::SpecialFile)
    } else {
        None
    }
}

#[cfg(windows)]
fn special_kind(_metadata: &Metadata) -> Option<SkipReason> {
    None
}

#[cfg(unix)]
fn device_of(metadata: &Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    metadata.dev()
}

/// Windows mount points are reparse points, which are never entered.
#[cfg(windows)]
fn device_of(_metadata: &Metadata) -> u64 {
    0
}

#[cfg(unix)]
fn owned_by_user(ctx: &Context, metadata: &Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    metadata.uid() == ctx.euid()
}

/// Windows checks the owner of the temp root only (catalog `user-temp`).
#[cfg(windows)]
fn owned_by_user(_ctx: &Context, _metadata: &Metadata) -> bool {
    true
}

#[cfg(target_os = "macos")]
fn is_locked(metadata: &Metadata) -> bool {
    use std::os::macos::fs::MetadataExt;
    metadata.st_flags() & (libc::UF_IMMUTABLE | libc::SF_IMMUTABLE) != 0
}

#[cfg(target_os = "linux")]
fn is_locked(_metadata: &Metadata) -> bool {
    false
}

#[cfg(windows)]
fn is_locked(metadata: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_READONLY;
    // On folders the read-only bit is a shell hint, not a protection.
    !metadata.is_dir() && metadata.file_attributes() & FILE_ATTRIBUTE_READONLY != 0
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
