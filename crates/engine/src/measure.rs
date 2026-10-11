//! File identity, allocated size, and volume free space (phase 4 items 5
//! and 6). Nothing here follows a link: identity and size describe the link
//! itself.

use std::fs::{self, Metadata};
use std::io;
use std::path::Path;
use std::time::SystemTime;

/// Device and inode on Unix; volume serial and file id on Windows.
/// Protected folders and hard links are compared by this, never by string.
/// The index is 128 bits because ReFS (Dev Drive) file ids are: its 64-bit
/// `nFileIndex` is documented as not unique. A Unix inode widens losslessly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FileId {
    device: u64,
    index: u128,
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

/// Whether `path` is on a network volume (NFS, SMB, AFP, a mapped drive).
/// The large-file report skips a home there (phase 6 item 9).
pub fn is_network_volume(path: &Path) -> bool {
    sys::is_network_volume(path)
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
    /// A symlink, or on Windows a symbolic link or junction (mount point).
    Link,
    /// A socket or FIFO; on Windows an AF_UNIX socket reparse point.
    SocketOrPipe,
    /// A device file; on Windows any other reparse point (cloud placeholder,
    /// app execution alias, deduplicated file, ...). Skipped, never deleted.
    Device,
}

/// `FILE_ATTRIBUTE_DIRECTORY`, `FILE_ATTRIBUTE_REPARSE_POINT`, and the
/// reparse tags [`kind_from`] tells apart. The tags come from a
/// `windows-sys` feature this crate does not otherwise need.
const ATTRIBUTE_DIRECTORY: u32 = 0x10;
const ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
const TAG_MOUNT_POINT: u32 = 0xA000_0003;
const TAG_SYMLINK: u32 = 0xA000_000C;
const TAG_AF_UNIX: u32 = 0x8000_0023;

#[cfg(windows)]
const _: () = {
    use windows_sys::Win32::Storage::FileSystem as fs;
    assert!(ATTRIBUTE_DIRECTORY == fs::FILE_ATTRIBUTE_DIRECTORY);
    assert!(ATTRIBUTE_REPARSE_POINT == fs::FILE_ATTRIBUTE_REPARSE_POINT);
};

/// What a Windows entry is, from its attributes and (for a reparse point)
/// its reparse tag. The scanner and the executor both call this. Only
/// symbolic links and junctions count as links; an AF_UNIX socket is a
/// socket; any other reparse point is a special file, so the entry holding
/// it is skipped whole rather than deleted or followed.
#[cfg_attr(unix, allow(dead_code))]
pub(crate) fn kind_from(attributes: u32, reparse_tag: u32) -> Kind {
    if attributes & ATTRIBUTE_REPARSE_POINT != 0 {
        return match reparse_tag {
            TAG_SYMLINK | TAG_MOUNT_POINT => Kind::Link,
            TAG_AF_UNIX => Kind::SocketOrPipe,
            _ => Kind::Device,
        };
    }
    if attributes & ATTRIBUTE_DIRECTORY != 0 {
        Kind::Dir
    } else {
        Kind::File
    }
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
    /// The two age timestamps: `mtime` and `ctime` on Unix, last write and
    /// creation on Windows. See [`crate::policy::is_old`].
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

/// The facts of an open handle: the scanner's handle on a path, or the
/// executor's handle opened relative to its parent. The handle must have
/// been opened with `FILE_FLAG_OPEN_REPARSE_POINT` (or
/// `FILE_OPEN_REPARSE_POINT`) and `FILE_READ_ATTRIBUTES`.
#[cfg(windows)]
pub(crate) fn facts_from_handle(
    handle: std::os::windows::io::BorrowedHandle<'_>,
) -> io::Result<Facts> {
    sys::facts_from_handle(handle)
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
                index: u128::from(metadata.ino()),
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
                index: u128::from(raw.inode),
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

    /// macOS: the mount is not `MNT_LOCAL`.
    #[cfg(target_os = "macos")]
    pub fn is_network_volume(path: &Path) -> bool {
        let Ok(c_path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
            return false;
        };
        // SAFETY: an all-zero statfs is a valid out-parameter.
        let mut stat: libc::statfs = unsafe { std::mem::zeroed() };
        // SAFETY: `c_path` is NUL-terminated and `stat` is writable.
        let ok = unsafe { libc::statfs(c_path.as_ptr(), &mut stat) } == 0;
        ok && stat.f_flags & libc::MNT_LOCAL as u32 == 0
    }

    /// Linux: the filesystem type is a network one.
    #[cfg(target_os = "linux")]
    #[allow(clippy::unnecessary_cast)] // `f_type` width differs between Linux targets
    pub fn is_network_volume(path: &Path) -> bool {
        const NETWORK: [i64; 7] = [
            0x6969,      // NFS
            0x517B,      // SMB
            0xFF53_4D42, // CIFS
            0xFE53_4D42, // SMB2
            0x7375_7245, // Coda
            0x5346_414F, // AFS
            0x0BD0_0BD0, // Lustre
        ];
        let Ok(c_path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
            return false;
        };
        // SAFETY: an all-zero statfs is a valid out-parameter.
        let mut stat: libc::statfs = unsafe { std::mem::zeroed() };
        // SAFETY: `c_path` is NUL-terminated and `stat` is writable.
        let ok = unsafe { libc::statfs(c_path.as_ptr(), &mut stat) } == 0;
        ok && NETWORK.contains(&(stat.f_type as i64))
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
    use std::os::windows::io::{AsHandle, AsRawHandle, BorrowedHandle};
    use std::path::Path;
    use std::time::{Duration, SystemTime};
    use windows_sys::Win32::Foundation::{FILETIME, GetLastError, NO_ERROR};
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_READONLY,
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO, FILE_COMPRESSION_INFO,
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_ID_INFO,
        FILE_READ_ATTRIBUTES, FileAttributeTagInfo, FileCompressionInfo, FileIdInfo,
        GetCompressedFileSizeW, GetDiskFreeSpaceExW, GetFileInformationByHandle,
        GetFileInformationByHandleEx, INVALID_FILE_SIZE, SYNCHRONIZE,
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
            id: file_id_of(file.as_handle(), &info),
            links: u64::from(info.nNumberOfLinks),
        })
    }

    /// The identity of an open handle. Both the scanner (`entry_info`) and
    /// the executor (`facts_from_handle`) read it here, so they agree.
    /// `FileIdInfo` gives the 64-bit volume serial and the 128-bit file id
    /// (unique on ReFS, where `nFileIndex` is not). If the file system does
    /// not support it (FAT, some network redirectors) the 32-bit serial and
    /// 64-bit index from `info` stand in. Support is a property of the file
    /// system, so every handle on one volume takes the same branch.
    fn file_id_of(handle: BorrowedHandle<'_>, info: &BY_HANDLE_FILE_INFORMATION) -> FileId {
        let mut id = FILE_ID_INFO::default();
        // SAFETY: `id` is a writable FILE_ID_INFO of the size passed, and
        // `handle` is open for the duration of the call.
        let ok = unsafe {
            GetFileInformationByHandleEx(
                handle.as_raw_handle(),
                FileIdInfo,
                (&raw mut id).cast(),
                std::mem::size_of::<FILE_ID_INFO>() as u32,
            )
        };
        if ok != 0 {
            FileId {
                device: id.VolumeSerialNumber,
                index: u128::from_le_bytes(id.FileId.Identifier),
            }
        } else {
            FileId {
                device: u64::from(info.dwVolumeSerialNumber),
                index: u128::from(
                    (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
                ),
            }
        }
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

    /// The entry itself is opened (never a reparse point's target), with
    /// attribute access only, which no sharing mode blocks. Windows checks
    /// the owner of the temp root only (catalog `user-temp`).
    pub fn facts(path: &Path, _metadata: &Metadata) -> io::Result<Facts> {
        let file = OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES | SYNCHRONIZE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)?;
        facts_from_handle(file.as_handle())
    }

    /// Identity and times from `GetFileInformationByHandle`, the reparse tag
    /// from `FileAttributeTagInfo`, and the size from `FileCompressionInfo`
    /// (what `GetCompressedFileSizeW` reports), all on the one handle.
    pub fn facts_from_handle(handle: BorrowedHandle<'_>) -> io::Result<Facts> {
        let raw = handle.as_raw_handle();
        // SAFETY: an all-zero struct is a valid out-parameter.
        let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        // SAFETY: `handle` is open for the duration of the call and `info`
        // is writable.
        if unsafe { GetFileInformationByHandle(raw, &mut info) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let attributes = info.dwFileAttributes;
        let tag = if attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            let mut tag = FILE_ATTRIBUTE_TAG_INFO::default();
            // SAFETY: `tag` is a writable FILE_ATTRIBUTE_TAG_INFO of the size
            // passed, and `handle` is open for the duration of the call.
            let ok = unsafe {
                GetFileInformationByHandleEx(
                    raw,
                    FileAttributeTagInfo,
                    (&raw mut tag).cast(),
                    std::mem::size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
                )
            };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            tag.ReparseTag
        } else {
            0
        };
        let kind = super::kind_from(attributes, tag);
        let size = if attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
            Size {
                bytes: 0,
                exact: true,
            }
        } else {
            compressed_size(handle).unwrap_or(Size {
                bytes: (u64::from(info.nFileSizeHigh) << 32) | u64::from(info.nFileSizeLow),
                exact: false,
            })
        };
        Ok(Facts {
            kind,
            id: file_id_of(handle, &info),
            links: u64::from(info.nNumberOfLinks),
            // On folders the read-only bit is a shell hint, not a protection.
            locked: kind == Kind::File && attributes & FILE_ATTRIBUTE_READONLY != 0,
            times: (
                file_time(info.ftLastWriteTime),
                file_time(info.ftCreationTime),
            ),
            size,
        })
    }

    fn compressed_size(handle: BorrowedHandle<'_>) -> Option<Size> {
        let mut info = FILE_COMPRESSION_INFO::default();
        // SAFETY: `info` is a writable FILE_COMPRESSION_INFO of the size
        // passed, and `handle` is open for the duration of the call.
        let ok = unsafe {
            GetFileInformationByHandleEx(
                handle.as_raw_handle(),
                FileCompressionInfo,
                (&raw mut info).cast(),
                std::mem::size_of::<FILE_COMPRESSION_INFO>() as u32,
            )
        };
        (ok != 0).then(|| Size {
            bytes: u64::try_from(info.CompressedFileSize).unwrap_or(0),
            exact: true,
        })
    }

    /// A `FILETIME` (100 ns ticks since 1601) as the `SystemTime` that
    /// `std` metadata reports for it.
    fn file_time(time: FILETIME) -> SystemTime {
        const TICKS_TO_UNIX_EPOCH: u64 = 116_444_736_000_000_000;
        let ticks = (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime);
        let span = |ticks: u64| {
            Duration::from_secs(ticks / 10_000_000) + Duration::from_nanos(ticks % 10_000_000 * 100)
        };
        if ticks >= TICKS_TO_UNIX_EPOCH {
            SystemTime::UNIX_EPOCH + span(ticks - TICKS_TO_UNIX_EPOCH)
        } else {
            SystemTime::UNIX_EPOCH
                .checked_sub(span(TICKS_TO_UNIX_EPOCH - ticks))
                .unwrap_or(SystemTime::UNIX_EPOCH)
        }
    }

    /// A mapped network drive or a UNC path.
    pub fn is_network_volume(path: &Path) -> bool {
        use std::path::{Component, Prefix};
        use windows_sys::Win32::Storage::FileSystem::GetDriveTypeW;
        // `DRIVE_REMOTE`, from a feature this crate does not otherwise need.
        const DRIVE_REMOTE: u32 = 4;
        let root = match path.components().next() {
            Some(Component::Prefix(prefix)) => match prefix.kind() {
                Prefix::UNC(..) | Prefix::VerbatimUNC(..) => return true,
                Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
                    format!("{}:\\", letter as char)
                }
                _ => return false,
            },
            _ => return false,
        };
        let name: Vec<u16> = root.encode_utf16().chain([0]).collect();
        // SAFETY: `name` is NUL-terminated.
        unsafe { GetDriveTypeW(name.as_ptr()) == DRIVE_REMOTE }
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

#[cfg(test)]
mod tests {
    use super::*;

    // Only symbolic links and junctions are links. Every other reparse
    // point is skipped whole, never followed and never deleted.
    #[test]
    fn windows_reparse_points_are_told_apart_by_tag() {
        let reparse = ATTRIBUTE_REPARSE_POINT;
        let dir = ATTRIBUTE_DIRECTORY;
        assert_eq!(kind_from(0, 0), Kind::File);
        assert_eq!(kind_from(dir, 0), Kind::Dir);
        assert_eq!(kind_from(reparse, TAG_SYMLINK), Kind::Link);
        assert_eq!(kind_from(reparse | dir, TAG_SYMLINK), Kind::Link);
        assert_eq!(kind_from(reparse | dir, TAG_MOUNT_POINT), Kind::Link);
        assert_eq!(kind_from(reparse, TAG_AF_UNIX), Kind::SocketOrPipe);
        // A OneDrive placeholder, an app execution alias, a deduplicated
        // file, and a tag the check cannot read.
        for tag in [0x9000_601A, 0x8000_001B, 0x8000_0013, 0] {
            assert_eq!(kind_from(reparse, tag), Kind::Device, "{tag:#x}");
            assert_eq!(kind_from(reparse | dir, tag), Kind::Device, "{tag:#x}");
        }
    }

    // The scanner's identity (`file_id`, the protected-folder and root
    // checks) and the executor's (`facts`, `facts_from_handle`) must be
    // read the same way, or a re-check before removal would see "changed".
    #[test]
    fn scanner_and_executor_read_the_same_identity() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.bin");
        fs::write(&file, b"bytes").unwrap();
        for path in [dir.path(), file.as_path()] {
            let metadata = fs::symlink_metadata(path).unwrap();
            let id = file_id(path).unwrap();
            assert_eq!(facts(path, &metadata).unwrap().id, id, "{path:?}");
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                // The inode widens into the 128-bit index without loss.
                let wide: u128 = id.index;
                assert_eq!(wide, u128::from(metadata.ino()));
                assert_eq!(id.device(), metadata.dev());
            }
        }
    }
}
