//! Startup guards: refuse to run as root or elevated, and allow one instance
//! (phase 2 items 14 and 15).

use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

const LOCK_FILE: &str = "instance.lock";

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
    // The Windows elevated-token check lands with windows-sys in M7 (phase 9).
    None
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
