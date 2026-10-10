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

impl FileId {
    pub(crate) fn device(self) -> u64 {
        self.device
    }
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

/// What an entry is, as far as the catalog entry rules care.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    File,
    Dir,
    /// A symlink, junction, or other reparse point.
    Link,
    // Windows reports neither through `std` metadata; its AF_UNIX sockets
    // are reparse points, which are links here.
    #[cfg_attr(windows, allow(dead_code))]
    SocketOrPipe,
    #[cfg_attr(windows, allow(dead_code))]
    Device,
}

/// One `lstat` of an entry: everything the entry rules read. The scanner
/// builds it from `std` metadata and the executor from `fstatat`, so both
/// apply the same rules to the same fields.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Facts {
    pub kind: Kind,
    pub id: FileId,
    pub links: u64,
    /// The owner. Windows checks the owner of the temp root only.
    #[cfg(unix)]
    pub uid: u32,
    /// Immutable (macOS) or read-only (Windows files).
    pub locked: bool,
    /// The two age timestamps; see [`age_times`].
    pub times: (SystemTime, SystemTime),
    pub size: Size,
}

/// The facts of `path` itself, from its `symlink_metadata`.
pub(crate) fn facts(path: &Path, metadata: &Metadata) -> io::Result<Facts> {
    sys::facts(path, metadata)
}

/// The `stat` fields the entry rules read, from `std` or from `rustix`.
#[cfg(unix)]
pub(crate) struct RawStat {
    pub mode: u32,
    pub device: u64,
    pub inode: u64,
    pub links: u64,
    pub uid: u32,
    pub blocks: u64,
    pub mtime: (i64, i64),
    pub ctime: (i64, i64),
    /// `st_flags`: the immutable bits.
    #[cfg(target_os = "macos")]
    pub flags: u32,
}

#[cfg(unix)]
pub(crate) fn facts_from_stat(raw: &RawStat) -> Facts {
    sys::facts_from_stat(raw)
}

#[cfg(unix)]
mod sys {
    use super::{EntryInfo, Facts, FileId, Kind, RawStat, Size};
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

    pub fn facts(_path: &Path, metadata: &Metadata) -> io::Result<Facts> {
        Ok(facts_from_stat(&RawStat {
            mode: metadata.mode(),
            device: metadata.dev(),
            inode: metadata.ino(),
            links: metadata.nlink(),
            uid: metadata.uid(),
            blocks: metadata.blocks(),
            mtime: (metadata.mtime(), metadata.mtime_nsec()),
            ctime: (metadata.ctime(), metadata.ctime_nsec()),
            #[cfg(target_os = "macos")]
            flags: std::os::macos::fs::MetadataExt::st_flags(metadata),
        }))
    }

    #[allow(clippy::unnecessary_cast)] // `mode_t` is u16 on macOS and u32 on Linux
    pub fn facts_from_stat(raw: &RawStat) -> Facts {
        let kind = match raw.mode & libc::S_IFMT as u32 {
            m if m == libc::S_IFDIR as u32 => Kind::Dir,
            m if m == libc::S_IFLNK as u32 => Kind::Link,
            m if m == libc::S_IFSOCK as u32 || m == libc::S_IFIFO as u32 => Kind::SocketOrPipe,
            m if m == libc::S_IFBLK as u32 || m == libc::S_IFCHR as u32 => Kind::Device,
            _ => Kind::File,
        };
        #[cfg(target_os = "macos")]
        let locked = raw.flags & (libc::UF_IMMUTABLE | libc::SF_IMMUTABLE) != 0;
        #[cfg(not(target_os = "macos"))]
        let locked = false;
        Facts {
            kind,
            id: FileId {
                device: raw.device,
                index: raw.inode,
            },
            links: raw.links,
            uid: raw.uid,
            locked,
            times: (
                unix_time(raw.mtime.0, raw.mtime.1),
                unix_time(raw.ctime.0, raw.ctime.1),
            ),
            // `st_blocks` is in 512-byte units on macOS and Linux.
            size: Size {
                bytes: raw.blocks.saturating_mul(512),
                exact: true,
            },
        }
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
    use super::{EntryInfo, Facts, FileId, Kind, Size};
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

    /// Symlinks, junctions, mount points, cloud placeholders, and app
    /// execution aliases are all reparse points. Windows checks the owner of
    /// the temp root only (catalog `user-temp`), so `uid` is `None`.
    pub fn facts(path: &Path, metadata: &Metadata) -> io::Result<Facts> {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_ATTRIBUTE_READONLY, FILE_ATTRIBUTE_REPARSE_POINT,
        };
        let attributes = metadata.file_attributes();
        let kind = if metadata.file_type().is_symlink()
            || attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        {
            Kind::Link
        } else if metadata.is_dir() {
            Kind::Dir
        } else {
            Kind::File
        };
        let info = entry_info(path, metadata)?;
        Ok(Facts {
            kind,
            id: info.id,
            links: info.links,
            // On folders the read-only bit is a shell hint, not a protection.
            locked: kind == Kind::File && attributes & FILE_ATTRIBUTE_READONLY != 0,
            times: age_times(metadata)?,
            size: allocated_size(path, metadata),
        })
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
