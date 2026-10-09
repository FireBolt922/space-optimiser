//! File identity, allocated size, and volume free space (phase 4 items 5
//! and 6). Nothing here follows a link: identity and size describe the link
//! itself.

use std::fs::{self, Metadata};
use std::io;
use std::path::Path;
use std::time::SystemTime;

/// Device and inode on Unix; volume serial and file index on Windows.
/// Protected folders and hard links are compared by this, never by string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FileId {
    device: u64,
    index: u64,
}

/// Allocated bytes. `exact` is false when Windows could not report the
/// compressed size and the logical size stands in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    pub bytes: u64,
    pub exact: bool,
}

/// The identity of `path` itself, not of a link target.
pub fn file_id(path: &Path) -> io::Result<FileId> {
    Ok(entry_info(path, &fs::symlink_metadata(path)?)?.id)
}

pub fn link_count(path: &Path) -> io::Result<u64> {
    Ok(entry_info(path, &fs::symlink_metadata(path)?)?.links)
}

/// Bytes the entry occupies on disk. Directories count their own blocks on
/// Unix and zero on Windows.
pub fn allocated_size(path: &Path, metadata: &Metadata) -> Size {
    sys::allocated_size(path, metadata)
}

/// Bytes available to this user on the volume that holds `path`.
pub fn free_space(path: &Path) -> io::Result<u64> {
    sys::free_space(path)
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct EntryInfo {
    pub id: FileId,
    pub links: u64,
}

/// Identity and hard-link count. Free on Unix; one handle on Windows.
pub(crate) fn entry_info(path: &Path, metadata: &Metadata) -> io::Result<EntryInfo> {
    sys::entry_info(path, metadata)
}

/// The two timestamps whose newest is the entry's age: `mtime` and `ctime`
/// on Unix, last-write and creation time on Windows.
pub(crate) fn age_times(metadata: &Metadata) -> io::Result<(SystemTime, SystemTime)> {
    sys::age_times(metadata)
}

#[cfg(unix)]
mod sys {
    use super::{EntryInfo, FileId, Size};
    use std::fs::Metadata;
    use std::io;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;
    use std::path::Path;
    use std::time::{Duration, SystemTime};

    pub fn entry_info(_path: &Path, metadata: &Metadata) -> io::Result<EntryInfo> {
        Ok(EntryInfo {
            id: FileId {
                device: metadata.dev(),
                index: metadata.ino(),
            },
            links: metadata.nlink(),
        })
    }

    pub fn allocated_size(_path: &Path, metadata: &Metadata) -> Size {
        // `st_blocks` is in 512-byte units on macOS and Linux.
        Size {
            bytes: metadata.blocks().saturating_mul(512),
            exact: true,
        }
    }

    pub fn age_times(metadata: &Metadata) -> io::Result<(SystemTime, SystemTime)> {
        let ctime = unix_time(metadata.ctime(), metadata.ctime_nsec());
        Ok((metadata.modified()?, ctime))
    }

    fn unix_time(secs: i64, nanos: i64) -> SystemTime {
        let nanos = Duration::from_nanos(nanos.clamp(0, 999_999_999) as u64);
        if secs >= 0 {
            SystemTime::UNIX_EPOCH + Duration::from_secs(secs as u64) + nanos
        } else {
            SystemTime::UNIX_EPOCH - Duration::from_secs(secs.unsigned_abs()) + nanos
        }
    }

    #[allow(clippy::unnecessary_cast)] // field widths differ between macOS and Linux
    pub fn free_space(path: &Path) -> io::Result<u64> {
        let c_path = std::ffi::CString::new(path.as_os_str().as_bytes())
            .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
        // SAFETY: an all-zero statvfs is a valid out-parameter.
        let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
        // SAFETY: `c_path` is NUL-terminated and `stat` is writable.
        if unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok((stat.f_bavail as u64).saturating_mul(stat.f_frsize as u64))
    }
}

#[cfg(windows)]
mod sys {
    use super::{EntryInfo, FileId, Size};
    use crate::platform::wide;
    use std::fs::{Metadata, OpenOptions};
    use std::io;
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use std::path::Path;
    use std::time::SystemTime;
    use windows_sys::Win32::Foundation::{GetLastError, NO_ERROR};
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        GetCompressedFileSizeW, GetDiskFreeSpaceExW, GetFileInformationByHandle, INVALID_FILE_SIZE,
    };

    pub fn entry_info(path: &Path, _metadata: &Metadata) -> io::Result<EntryInfo> {
        // Attribute-only access with full sharing; the reparse point itself is
        // opened, never its target.
        let file = OpenOptions::new()
            .access_mode(0)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)?;
        // SAFETY: an all-zero struct is a valid out-parameter.
        let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        // SAFETY: the handle is open for the duration of the call.
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(EntryInfo {
            id: FileId {
                device: u64::from(info.dwVolumeSerialNumber),
                index: (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
            },
            links: u64::from(info.nNumberOfLinks),
        })
    }

    pub fn allocated_size(path: &Path, metadata: &Metadata) -> Size {
        if metadata.is_dir() {
            return Size {
                bytes: 0,
                exact: true,
            };
        }
        let name = wide(path);
        let mut high = 0u32;
        // SAFETY: `name` is NUL-terminated and `high` is writable.
        let low = unsafe { GetCompressedFileSizeW(name.as_ptr(), &mut high) };
        // SAFETY: reads this thread's last error only.
        if low == INVALID_FILE_SIZE && unsafe { GetLastError() } != NO_ERROR {
            return Size {
                bytes: metadata.len(),
                exact: false,
            };
        }
        Size {
            bytes: (u64::from(high) << 32) | u64::from(low),
            exact: true,
        }
    }

    pub fn age_times(metadata: &Metadata) -> io::Result<(SystemTime, SystemTime)> {
        Ok((metadata.modified()?, metadata.created()?))
    }

    pub fn free_space(path: &Path) -> io::Result<u64> {
        let name = wide(path);
        let mut available = 0u64;
        // SAFETY: `name` is NUL-terminated; unused outputs may be null.
        let ok = unsafe {
            GetDiskFreeSpaceExW(
                name.as_ptr(),
                &mut available,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(available)
    }
}
