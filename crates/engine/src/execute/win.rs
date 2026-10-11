//! The handle-relative Windows delete (phase 8 item 3, phase 9). It mirrors
//! the Unix executor in `execute.rs` step for step: the root is opened once
//! without following a reparse point, every entry below it is opened with
//! `NtCreateFile` relative to its parent's handle (one name, never a path),
//! a verify pass applies the catalog rules and the scan snapshot to every
//! descendant, and a delete pass re-checks identity on the very handle it
//! then marks for deletion with `SetFileInformationByHandle`.
//!
//! Reparse points are opened as themselves (`FILE_OPEN_REPARSE_POINT`) and
//! are never listed: only a plain directory handle is ever opened with
//! `FILE_LIST_DIRECTORY`. Symbolic links and junctions are deleted as links;
//! every other reparse point is a special file and skips its entry whole
//! ([`measure::kind_from`]). Read-only files are skipped by the entry rules
//! and the read-only attribute is never ignored or cleared.
//!
//! Relative opens bypass Win32 name rules, so names Win32 cannot spell
//! (a trailing dot or space, `CON`) and paths longer than `MAX_PATH` are
//! handled like any other.

use super::{Done, PlanEntry, Work};
use crate::measure::{self, Facts, FileId, Kind};
use crate::model::SkipCause;
use crate::policy::{self, CatalogId, Context, SkipReason};
use std::ffi::{OsStr, OsString};
use std::fs::OpenOptions;
use std::io;
use std::mem::{offset_of, size_of};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::{AsHandle, AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::{Path, PathBuf};
use windows_sys::Wdk::Foundation::OBJECT_ATTRIBUTES;
use windows_sys::Wdk::Storage::FileSystem::{
    FILE_OPEN, FILE_OPEN_REPARSE_POINT, FILE_SYNCHRONOUS_IO_NONALERT, NtCreateFile,
};
use windows_sys::Win32::Foundation::{
    ERROR_FILE_NOT_FOUND, ERROR_INVALID_FUNCTION, ERROR_INVALID_PARAMETER, ERROR_NO_MORE_FILES,
    ERROR_NOT_SUPPORTED, ERROR_PATH_NOT_FOUND, ERROR_SHARING_VIOLATION, HANDLE,
    RtlNtStatusToDosError, UNICODE_STRING,
};
use windows_sys::Win32::Storage::FileSystem::{
    DELETE, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_DISPOSITION_FLAG_DELETE,
    FILE_DISPOSITION_FLAG_POSIX_SEMANTICS, FILE_DISPOSITION_INFO, FILE_DISPOSITION_INFO_EX,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_FULL_DIR_INFO,
    FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
    FILE_SHARE_WRITE, FileDispositionInfo, FileDispositionInfoEx, FileFullDirectoryInfo,
    FileFullDirectoryRestartInfo, GetFileInformationByHandleEx, SYNCHRONIZE,
    SetFileInformationByHandle,
};
use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;

/// Attribute access only. No sharing mode blocks it, so an entry another
/// program holds open can still be judged.
const STAT: u32 = FILE_READ_ATTRIBUTES | SYNCHRONIZE;
/// What a handle that may be marked for deletion is opened with.
const REMOVE: u32 = DELETE | STAT;
/// Others may read, write, and delete while this engine holds a handle.
const SHARE_ALL: u32 = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;

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
    // The root handle has no DELETE access: the root is never removed.
    let root_dir = match open_root(root.path()) {
        Ok(handle) => handle,
        Err(e) => return Done::failed(e, 0),
    };
    let root_facts = match facts(&root_dir) {
        Ok(facts) if facts.kind == Kind::Dir && facts.id == root.id() => facts,
        Ok(_) => return Done::skipped(SkipCause::ChangedSinceScan),
        Err(e) => return Done::failed(e, 0),
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
        if let Err(cause) = tree.verify(&root_dir) {
            return Done::skipped(cause);
        }
        tree.empty(&root_dir);
        return match tree.error {
            Some(error) => Done::failed(error, tree.bytes),
            None => Done::removed(tree.bytes),
        };
    };
    let top = match open_child(&root_dir, name, STAT).and_then(|h| facts(&h)) {
        Ok(facts) => facts,
        Err(e) if is_not_found(&e) => return Done::skipped(SkipCause::Gone),
        Err(e) => return Done::failed(e, 0),
    };
    if top.id != *file_id {
        return Done::skipped(SkipCause::ChangedSinceScan);
    }
    if let Some(reason) = top_rules(ctx, row, name, &top, device) {
        return Done::skipped(SkipCause::Entry(reason));
    }

    // The handle that is marked for deletion, checked again: same file,
    // same kind (a folder can become a junction in place), same rules.
    let access = if top.kind == Kind::Dir {
        REMOVE | FILE_LIST_DIRECTORY
    } else {
        REMOVE
    };
    let handle = match open_child(&root_dir, name, access) {
        Ok(handle) => handle,
        // Another program holds it open without sharing delete access.
        Err(e) if is_sharing_violation(&e) => {
            return Done::skipped(SkipCause::Entry(SkipReason::Locked));
        }
        Err(e) if is_not_found(&e) => return Done::skipped(SkipCause::Gone),
        Err(e) => return Done::failed(e, 0),
    };
    let held = match facts(&handle) {
        Ok(facts) if facts.id == top.id && facts.kind == top.kind => facts,
        Ok(_) => return Done::skipped(SkipCause::ChangedSinceScan),
        Err(e) => return Done::failed(e, 0),
    };
    if let Some(reason) = top_rules(ctx, row, name, &held, device) {
        return Done::skipped(SkipCause::Entry(reason));
    }
    if held.kind != Kind::Dir {
        let bytes = if held.links > 1 { 0 } else { held.size.bytes };
        return match dispose(&handle) {
            Ok(()) => Done::removed(bytes),
            Err(e) => Done::failed(e, 0),
        };
    }

    if let Err(cause) = tree.verify(&handle) {
        return Done::skipped(cause);
    }
    tree.empty(&handle);
    if let Some(error) = tree.error {
        return Done::failed(error, tree.bytes);
    }
    match dispose(&handle) {
        Ok(()) => Done::removed(tree.bytes + held.size.bytes),
        Err(e) => Done::failed(e, tree.bytes),
    }
    // `handle` closes here, which completes the delete.
}

/// The rules for a top-level entry: the same calls as the Unix executor.
fn top_rules(
    ctx: &Context,
    row: CatalogId,
    name: &OsStr,
    facts: &Facts,
    device: Option<u64>,
) -> Option<SkipReason> {
    policy::entry_rule(row, ctx, facts, device).or_else(|| policy::name_rule(row, name, facts.kind))
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

    /// The catalog rules on every descendant, immediately before the delete
    /// pass. Anything the scan did not see skips the whole entry.
    fn verify(&self, dir: &OwnedHandle) -> Result<(), SkipCause> {
        let unreadable = SkipCause::Entry(SkipReason::Unreadable);
        for (name, attributes) in list(dir).map_err(|_| unreadable)? {
            if self.row.checks_project_markers() && policy::is_project_marker(&name) {
                return Err(SkipCause::Entry(SkipReason::Project));
            }
            let listable = is_plain_dir(attributes);
            let access = if listable {
                STAT | FILE_LIST_DIRECTORY
            } else {
                STAT
            };
            let child = open_child(dir, &name, access).map_err(|_| unreadable)?;
            let facts = facts(&child).map_err(|_| unreadable)?;
            if facts.kind != Kind::Link {
                if let Some(reason) = policy::entry_rule(self.row, self.ctx, &facts, self.device)
                    .or_else(|| policy::name_rule(self.row, &name, facts.kind))
                {
                    return Err(SkipCause::Entry(reason));
                }
            }
            if !self.is_member(facts.id) {
                return Err(SkipCause::ChangedSinceScan);
            }
            if facts.kind == Kind::Dir {
                // Listed as something else a moment ago.
                if !listable {
                    return Err(SkipCause::ChangedSinceScan);
                }
                self.verify(&child)?;
            }
        }
        Ok(())
    }

    /// Removes everything below `dir`, deepest first. The verify pass has
    /// just applied the rules; this pass re-checks identity on the handle
    /// it deletes through, so nothing swapped in between is touched. Links
    /// are removed, never followed.
    fn empty(&mut self, dir: &OwnedHandle) {
        let names = match list(dir) {
            Ok(names) => names,
            Err(e) => return self.fail(&e),
        };
        for (name, attributes) in names {
            let listable = is_plain_dir(attributes);
            let access = if listable {
                REMOVE | FILE_LIST_DIRECTORY
            } else {
                REMOVE
            };
            let child = match open_child(dir, &name, access) {
                Ok(child) => child,
                Err(e) => {
                    self.fail(&e);
                    continue;
                }
            };
            let facts = match facts(&child) {
                Ok(facts) => facts,
                Err(e) => {
                    self.fail(&e);
                    continue;
                }
            };
            let allowed = matches!(facts.kind, Kind::File | Kind::Dir | Kind::Link);
            if !allowed || !self.is_member(facts.id) {
                self.fail_with("it changed during the cleanup");
                continue;
            }
            if facts.kind == Kind::Dir {
                if !listable {
                    self.fail_with("it changed during the cleanup");
                    continue;
                }
                self.empty(&child);
                match dispose(&child) {
                    Ok(()) => self.bytes += facts.size.bytes,
                    Err(e) => self.fail(&e),
                }
            } else {
                let bytes = if facts.links > 1 { 0 } else { facts.size.bytes };
                match dispose(&child) {
                    Ok(()) => self.bytes += bytes,
                    Err(e) => self.fail(&e),
                }
            }
            // `child` closes here, before its parent is deleted.
        }
    }

    fn fail(&mut self, error: &io::Error) {
        self.fail_with(&error.to_string());
    }

    fn fail_with(&mut self, error: &str) {
        self.error.get_or_insert_with(|| error.to_owned());
    }
}

/// A directory that is not a reparse point: the only kind ever listed.
fn is_plain_dir(attributes: u32) -> bool {
    attributes & FILE_ATTRIBUTE_DIRECTORY != 0 && attributes & FILE_ATTRIBUTE_REPARSE_POINT == 0
}

fn facts(handle: &OwnedHandle) -> io::Result<Facts> {
    measure::facts_from_handle(handle.as_handle())
}

fn is_not_found(error: &io::Error) -> bool {
    matches!(
        error.raw_os_error().map(|code| code as u32),
        Some(ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND)
    )
}

fn is_sharing_violation(error: &io::Error) -> bool {
    error.raw_os_error() == Some(ERROR_SHARING_VIOLATION as i32)
}

/// Opens the root folder itself, never a reparse point's target, for
/// listing and attributes only. Its identity is checked by the caller.
fn open_root(path: &Path) -> io::Result<OwnedHandle> {
    let file = OpenOptions::new()
        .access_mode(FILE_LIST_DIRECTORY | STAT)
        .share_mode(SHARE_ALL)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(verbatim(path))?;
    Ok(OwnedHandle::from(file))
}

/// `path` with the `\\?\` prefix, so Win32 does not rewrite it. Canonical
/// paths already carry it.
fn verbatim(path: &Path) -> PathBuf {
    use std::path::{Component, Prefix};
    let wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    let plain = path
        .components()
        .all(|c| !matches!(c, Component::CurDir | Component::ParentDir))
        && !wide.contains(&u16::from(b'/'));
    let prefix: &str = match path.components().next() {
        Some(Component::Prefix(p)) if plain => match p.kind() {
            Prefix::Disk(_) => r"\\?\",
            Prefix::UNC(..) => r"\\?\UNC",
            _ => return path.to_path_buf(),
        },
        _ => return path.to_path_buf(),
    };
    // A UNC path keeps one of its two leading backslashes.
    let rest = if prefix.ends_with("UNC") {
        &wide[1..]
    } else {
        &wide[..]
    };
    let mut out: Vec<u16> = prefix.encode_utf16().collect();
    out.extend_from_slice(rest);
    PathBuf::from(OsString::from_wide(&out))
}

/// Opens one name inside `parent` with `NtCreateFile`, relative to the
/// parent's handle. The final component is opened as itself
/// (`FILE_OPEN_REPARSE_POINT`), so a link or junction is never followed.
fn open_child(parent: &OwnedHandle, name: &OsStr, access: u32) -> io::Result<OwnedHandle> {
    let units: Vec<u16> = name.encode_wide().collect();
    let is = |unit: u16, c: u8| unit == u16::from(c);
    let single_component = !units.is_empty()
        && units != [u16::from(b'.')]
        && units != [u16::from(b'.'), u16::from(b'.')]
        && !units
            .iter()
            .any(|&u| u == 0 || is(u, b'\\') || is(u, b'/') || is(u, b':'));
    if !single_component {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    }
    // UNICODE_STRING lengths are in bytes, without a terminating NUL.
    let bytes =
        u16::try_from(units.len() * 2).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
    let object_name = UNICODE_STRING {
        Length: bytes,
        MaximumLength: bytes,
        // NtCreateFile only reads the name.
        Buffer: units.as_ptr().cast_mut(),
    };
    let attributes = OBJECT_ATTRIBUTES {
        Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: parent.as_raw_handle(),
        ObjectName: &object_name,
        Attributes: 0,
        SecurityDescriptor: std::ptr::null(),
        SecurityQualityOfService: std::ptr::null(),
    };
    let mut handle: HANDLE = std::ptr::null_mut();
    let mut status_block = IO_STATUS_BLOCK::default();
    // SAFETY: `handle` and `status_block` are writable; `attributes` points
    // at `object_name`, whose buffer `units` holds `bytes` bytes, and at the
    // open `parent` handle, all of which outlive the call. No allocation
    // size or extended attributes are passed (null, 0).
    let status = unsafe {
        NtCreateFile(
            &mut handle,
            access,
            &attributes,
            &mut status_block,
            std::ptr::null(),
            0,
            SHARE_ALL,
            FILE_OPEN,
            // No FILE_OPEN_FOR_BACKUP_INTENT: the native call opens folders
            // without it (only CreateFileW needs BACKUP_SEMANTICS for that),
            // and with SeBackupPrivilege it would bypass the ACLs.
            FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
            std::ptr::null(),
            0,
        )
    };
    if status < 0 {
        // SAFETY: converts a status code; no memory is touched.
        let code = unsafe { RtlNtStatusToDosError(status) };
        return Err(io::Error::from_raw_os_error(code as i32));
    }
    // SAFETY: NtCreateFile succeeded, so `handle` is a new open handle that
    // nothing else owns; `OwnedHandle` closes it exactly once.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

/// Marks the open entry for deletion; it goes when the handle closes.
/// POSIX semantics remove the name at once even if another program still
/// has the file open (as `unlink` does). The read-only attribute is never
/// ignored: a read-only file fails here and stays.
fn dispose(handle: &OwnedHandle) -> io::Result<()> {
    let raw = handle.as_raw_handle();
    let info = FILE_DISPOSITION_INFO_EX {
        Flags: FILE_DISPOSITION_FLAG_DELETE | FILE_DISPOSITION_FLAG_POSIX_SEMANTICS,
    };
    // SAFETY: `info` is a FILE_DISPOSITION_INFO_EX of the size passed and
    // `handle` is open for the duration of the call.
    let ok = unsafe {
        SetFileInformationByHandle(
            raw,
            FileDispositionInfoEx,
            (&raw const info).cast(),
            size_of::<FILE_DISPOSITION_INFO_EX>() as u32,
        )
    };
    if ok != 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    // Older Windows versions and some filesystems (FAT) lack the extended
    // class. Any other error is final.
    if !matches!(
        error.raw_os_error().map(|code| code as u32),
        Some(ERROR_INVALID_PARAMETER | ERROR_NOT_SUPPORTED | ERROR_INVALID_FUNCTION)
    ) {
        return Err(error);
    }
    let info = FILE_DISPOSITION_INFO { DeleteFile: true };
    // SAFETY: as above, for a FILE_DISPOSITION_INFO.
    let ok = unsafe {
        SetFileInformationByHandle(
            raw,
            FileDispositionInfo,
            (&raw const info).cast(),
            size_of::<FILE_DISPOSITION_INFO>() as u32,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// The names in `dir` with their attributes, from the start of the folder
/// every time (the verify and delete passes list the same handle).
fn list(dir: &OwnedHandle) -> io::Result<Vec<(OsString, u32)>> {
    // 64 KiB with the 8-byte alignment directory queries require.
    let mut buffer = vec![0u64; 8 * 1024];
    let size = buffer.len() * size_of::<u64>();
    let mut names = Vec::new();
    let mut class = FileFullDirectoryRestartInfo;
    loop {
        // SAFETY: `buffer` is writable for `size` bytes and `dir` is open
        // for the duration of the call.
        let ok = unsafe {
            GetFileInformationByHandleEx(
                dir.as_raw_handle(),
                class,
                buffer.as_mut_ptr().cast(),
                size as u32,
            )
        };
        if ok == 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(ERROR_NO_MORE_FILES as i32) {
                return Ok(names);
            }
            return Err(error);
        }
        class = FileFullDirectoryInfo;
        // SAFETY: the `u64` buffer is initialised and `size` bytes long;
        // `u8` has no alignment or validity requirements.
        let bytes = unsafe { std::slice::from_raw_parts(buffer.as_ptr().cast::<u8>(), size) };
        parse_entries(bytes, &mut names)?;
    }
}

/// Walks one buffer of FILE_FULL_DIR_INFO records, checking every offset
/// against the buffer.
fn parse_entries(bytes: &[u8], names: &mut Vec<(OsString, u32)>) -> io::Result<()> {
    let bad = || io::Error::from(io::ErrorKind::InvalidData);
    let read_u32 = |at: usize| -> io::Result<u32> {
        let field = bytes.get(at..at + 4).ok_or_else(bad)?;
        Ok(u32::from_ne_bytes([field[0], field[1], field[2], field[3]]))
    };
    let mut offset = 0usize;
    loop {
        let next = read_u32(offset + offset_of!(FILE_FULL_DIR_INFO, NextEntryOffset))?;
        let attributes = read_u32(offset + offset_of!(FILE_FULL_DIR_INFO, FileAttributes))?;
        let length = read_u32(offset + offset_of!(FILE_FULL_DIR_INFO, FileNameLength))? as usize;
        let start = offset + offset_of!(FILE_FULL_DIR_INFO, FileName);
        let name = bytes.get(start..start + length).ok_or_else(bad)?;
        if length % 2 != 0 {
            return Err(bad());
        }
        let units: Vec<u16> = name
            .chunks_exact(2)
            .map(|unit| u16::from_ne_bytes([unit[0], unit[1]]))
            .collect();
        let dot = u16::from(b'.');
        if units != [dot] && units != [dot, dot] {
            names.push((OsString::from_wide(&units), attributes));
        }
        if next == 0 {
            return Ok(());
        }
        offset = offset.checked_add(next as usize).ok_or_else(bad)?;
    }
}
