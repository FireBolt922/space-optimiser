//! Names of running processes, for "quit first" rows (catalog entry rules,
//! phase 5 item 5). Read only: the app never signals or stops a process.
//! macOS uses libproc, Linux `/proc`, Windows a ToolHelp snapshot.

use std::io;

/// The name of every process this user can see. A process that exits while
/// the list is read is left out.
pub(crate) fn names() -> io::Result<Vec<String>> {
    sys::names()
}

#[cfg(target_os = "macos")]
mod sys {
    use std::io;

    pub fn names() -> io::Result<Vec<String>> {
        // SAFETY: a null buffer asks only for the number of processes.
        let count = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
        if count <= 0 {
            return Err(io::Error::last_os_error());
        }
        // Room for processes started between the two calls.
        let mut pids = vec![0 as libc::pid_t; count as usize + 64];
        let bytes = (pids.len() * size_of::<libc::pid_t>()) as libc::c_int;
        // SAFETY: `pids` is writable for `bytes` bytes.
        let found = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), bytes) };
        if found <= 0 {
            return Err(io::Error::last_os_error());
        }
        pids.truncate(found as usize);
        let mut names = Vec::with_capacity(pids.len());
        let mut buffer = [0u8; 256];
        for pid in pids {
            // SAFETY: `buffer` is writable for its full length.
            let len =
                unsafe { libc::proc_name(pid, buffer.as_mut_ptr().cast(), buffer.len() as u32) };
            if len > 0 {
                names.push(String::from_utf8_lossy(&buffer[..len as usize]).into_owned());
            }
        }
        Ok(names)
    }
}

#[cfg(target_os = "linux")]
mod sys {
    use std::fs;
    use std::io;

    /// `/proc/<pid>/comm`: the executable name, cut to 15 bytes.
    pub fn names() -> io::Result<Vec<String>> {
        let mut names = Vec::new();
        for entry in fs::read_dir("/proc")? {
            let entry = entry?;
            let file_name = entry.file_name();
            let Some(pid) = file_name.to_str() else {
                continue;
            };
            if !pid.bytes().all(|b| b.is_ascii_digit()) {
                continue;
            }
            if let Ok(comm) = fs::read(entry.path().join("comm")) {
                let comm = String::from_utf8_lossy(&comm);
                names.push(comm.trim_end_matches('\n').to_owned());
            }
        }
        Ok(names)
    }
}

#[cfg(windows)]
mod sys {
    use std::io;
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };

    /// The executable file name of each process, such as `chrome.exe`.
    pub fn names() -> io::Result<Vec<String>> {
        // SAFETY: no pointers; a process id of 0 means every process.
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let mut names = Vec::new();
        // SAFETY: an all-zero PROCESSENTRY32W is valid once dwSize is set.
        let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
        entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        // SAFETY: `snapshot` is open and `entry` is writable with dwSize set.
        let mut more = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
        while more {
            let len = entry
                .szExeFile
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(entry.szExeFile.len());
            names.push(String::from_utf16_lossy(&entry.szExeFile[..len]));
            // SAFETY: as above.
            more = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
        }
        // SAFETY: `snapshot` was opened above.
        unsafe { CloseHandle(snapshot) };
        Ok(names)
    }
}

#[cfg(test)]
mod tests {
    // The process list is read only; this checks it can be read at all.
    #[test]
    fn this_machine_lists_at_least_this_process() {
        let names = super::names().unwrap();
        assert!(!names.is_empty());
    }
}
