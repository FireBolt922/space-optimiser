//! Startup guards: refuse to run as root or elevated, and allow one instance
//! (phase 2 items 14 and 15, phase 9). Also the check that keeps the debug
//! fixture window away from a real home.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

const LOCK_FILE: &str = "instance.lock";
/// The file that marks a folder as a copy of a fixture home.
const FIXTURE_MARKER: &str = ".space-optimiser-fixture";

#[derive(Debug, thiserror::Error)]
pub enum GuardError {
    #[error("Space Optimiser is already running")]
    AlreadyRunning,
    #[error("could not create the instance lock: {0}")]
    Io(#[from] io::Error),
}

/// Root may not scan or delete: the `/tmp` owner check would match every
/// user's files, and `$HOME` may still point at the invoking user.
pub fn should_refuse(euid: u32) -> bool {
    euid == 0
}

/// An elevated Windows token may not scan or delete, for the same reason:
/// its new files belong to Administrators, and it can write where the
/// person cannot.
pub fn should_refuse_elevated(elevated: bool) -> bool {
    elevated
}

/// Whether this process runs with an elevated token. A token that cannot be
/// read counts as not elevated; the executor's ownership checks still apply.
#[cfg(windows)]
pub fn is_elevated() -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::{
        GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: the pseudo-handle needs no closing; `token` is an out-param.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return false;
    }
    let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
    let mut len = 0u32;
    // SAFETY: `elevation` is writable for the size passed.
    let ok = unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            (&raw mut elevation).cast(),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        )
    };
    // SAFETY: `token` was opened above.
    unsafe { CloseHandle(token) };
    ok != 0 && elevation.TokenIsElevated != 0
}

/// Returns why this process must not run, if it must not.
pub fn refusal_reason() -> Option<&'static str> {
    #[cfg(unix)]
    {
        // SAFETY: geteuid has no preconditions and cannot fail.
        let euid = unsafe { libc::geteuid() };
        if should_refuse(euid) {
            return Some("Space Optimiser does not run as root. Start it as your own user.");
        }
    }
    #[cfg(windows)]
    if should_refuse_elevated(is_elevated()) {
        return Some("Space Optimiser does not run as administrator. Start it as your own user.");
    }
    None
}

/// Whether `home` is a copy of a fixture home: it holds the marker file, a
/// regular file and not a link or a folder.
pub fn is_fixture_home(home: &Path) -> bool {
    std::fs::symlink_metadata(home.join(FIXTURE_MARKER)).is_ok_and(|m| m.file_type().is_file())
}

/// Held for the app's lifetime. Released when dropped.
#[derive(Debug)]
pub struct InstanceLock {
    _file: File,
}

impl InstanceLock {
    pub fn acquire(dir: &Path) -> Result<Self, GuardError> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join(LOCK_FILE);
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(0);
        }
        let file = match options.open(&path) {
            Ok(file) => file,
            #[cfg(windows)]
            Err(e) if e.raw_os_error() == Some(32) => return Err(GuardError::AlreadyRunning), // ERROR_SHARING_VIOLATION
            Err(e) => return Err(e.into()),
        };
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            // SAFETY: the descriptor is valid for the lifetime of `file`.
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
                let err = io::Error::last_os_error();
                return Err(if err.kind() == io::ErrorKind::WouldBlock {
                    GuardError::AlreadyRunning
                } else {
                    err.into()
                });
            }
        }
        Ok(Self { _file: file })
    }
}
