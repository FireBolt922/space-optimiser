//! The local audit log (phase 8 item 7, phase 2 item 10): one JSON line per
//! action between a "run started" and a "run finished" line. Paths and
//! outcomes only, never file contents. The log stays on this computer.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::model::{AuditLogStatus, InterruptedAction, InterruptedRun, Outcome};
use crate::policy::CatalogId;

pub(crate) const FILE: &str = "audit.jsonl";

/// How much of the end of the log is read to find an unfinished run.
const TAIL_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
enum Line {
    RunStarted {
        time: u64,
        run: String,
        items: usize,
    },
    Action {
        time: u64,
        run: String,
        catalog_id: String,
        path: String,
        outcome: String,
        detail: Option<String>,
        bytes: u64,
    },
    RunFinished {
        time: u64,
        run: String,
        removed: usize,
        bytes: u64,
        stopped: bool,
    },
    /// Written by a later launch that found the run unfinished, so it is
    /// reported once.
    RunInterrupted { time: u64, run: String },
}

/// Why "run started" could not be written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Unavailable {
    pub disk_full: bool,
}

/// One run's log. Every line is also kept in memory, so the report can show
/// the log when the disk could not take it.
#[derive(Debug)]
pub(crate) struct Audit {
    run: String,
    file: Option<File>,
    lines: Vec<String>,
    complete_on_disk: bool,
}

impl Audit {
    /// Writes "run started" and flushes it to disk. Without
    /// `keep_in_memory`, a failure stops the run before anything is deleted.
    pub fn start(dir: &Path, items: usize, keep_in_memory: bool) -> Result<Self, Unavailable> {
        let run = format!(
            "{}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis(),
            std::process::id()
        );
        let line = encode(&Line::RunStarted {
            time: now(),
            run: run.clone(),
            items,
        });
        let file = open_and_write(dir, &line);
        let (file, complete_on_disk) = match file {
            Ok(file) => (Some(file), true),
            Err(_) if keep_in_memory => (None, false),
            Err(e) => {
                return Err(Unavailable {
                    disk_full: is_disk_full(&e),
                });
            }
        };
        Ok(Self {
            run,
            file,
            lines: vec![line],
            complete_on_disk,
        })
    }

    pub fn action(
        &mut self,
        catalog_id: CatalogId,
        path: &Path,
        outcome: Outcome,
        detail: Option<&str>,
        bytes: u64,
    ) {
        let outcome = match outcome {
            Outcome::Removed => "removed",
            Outcome::Skipped => "skipped",
            Outcome::Failed => "failed",
            Outcome::NotRun => "not_run",
        };
        self.record(
            Line::Action {
                time: now(),
                run: self.run.clone(),
                catalog_id: catalog_id.as_str().to_owned(),
                path: escaped(path),
                outcome: outcome.to_owned(),
                detail: detail.map(str::to_owned),
                bytes,
            },
            false,
        );
    }

    /// Writes "run finished". Returns where the log ended up and, when any
    /// line missed the disk, every line for the report.
    pub fn finish(
        mut self,
        removed: usize,
        bytes: u64,
        stopped: bool,
    ) -> (AuditLogStatus, Vec<String>) {
        self.record(
            Line::RunFinished {
                time: now(),
                run: self.run.clone(),
                removed,
                bytes,
                stopped,
            },
            true,
        );
        if self.complete_on_disk {
            (AuditLogStatus::Written, Vec::new())
        } else {
            (AuditLogStatus::MemoryOnly, self.lines)
        }
    }

    fn record(&mut self, line: Line, sync: bool) {
        let line = encode(&line);
        if let Some(file) = &mut self.file {
            let written = file.write_all(format!("{line}\n").as_bytes());
            let synced = if sync { file.sync_data() } else { Ok(()) };
            if written.is_err() || synced.is_err() {
                self.file = None;
                self.complete_on_disk = false;
            }
        }
        self.lines.push(line);
    }
}

/// Finds runs that started and never finished, marks them so they are
/// reported once, and returns the latest.
pub(crate) fn take_interrupted(dir: &Path) -> Option<InterruptedRun> {
    let text = read_tail(&dir.join(FILE))?;
    let mut open: BTreeMap<String, InterruptedRun> = BTreeMap::new();
    let mut order = Vec::new();
    for line in text.lines() {
        let Ok(line) = serde_json::from_str::<Line>(line) else {
            continue;
        };
        match line {
            Line::RunStarted { time, run, items } => {
                order.push(run.clone());
                open.insert(
                    run,
                    InterruptedRun {
                        started_unix_secs: time,
                        items_planned: items,
                        actions: Vec::new(),
                    },
                );
            }
            Line::Action {
                run, path, outcome, ..
            } => {
                if let Some(entry) = open.get_mut(&run) {
                    let display_name = Path::new(&path)
                        .file_name()
                        .map_or(path.clone(), |n| n.to_string_lossy().into_owned());
                    entry.actions.push(InterruptedAction {
                        display_name,
                        outcome,
                    });
                }
            }
            Line::RunFinished { run, .. } | Line::RunInterrupted { run, .. } => {
                open.remove(&run);
            }
        }
    }
    let latest = order
        .iter()
        .rev()
        .find(|run| open.contains_key(*run))?
        .clone();
    let marks: String = open
        .keys()
        .map(|run| {
            encode(&Line::RunInterrupted {
                time: now(),
                run: run.clone(),
            }) + "\n"
        })
        .collect();
    let _ = OpenOptions::new()
        .append(true)
        .open(dir.join(FILE))
        .and_then(|mut file| file.write_all(marks.as_bytes()));
    open.remove(&latest)
}

/// The last [`TAIL_BYTES`] of a regular file, from the first whole line.
/// Anything else (a device, a pipe) is not a log and is not read.
fn read_tail(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let length = file.metadata().ok().filter(|m| m.is_file())?.len();
    let start = length.saturating_sub(TAIL_BYTES);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::new();
    file.take(TAIL_BYTES).read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let text = if start > 0 {
        text.split_once('\n').map_or("", |(_, rest)| rest)
    } else {
        &text
    };
    Some(text.to_owned())
}

fn open_and_write(dir: &Path, line: &str) -> io::Result<File> {
    fs::create_dir_all(dir)?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(FILE))?;
    file.write_all(format!("{line}\n").as_bytes())?;
    file.sync_data()?;
    Ok(file)
}

fn is_disk_full(error: &io::Error) -> bool {
    #[cfg(unix)]
    if error.raw_os_error() == Some(libc::ENOSPC) {
        return true;
    }
    error.kind() == io::ErrorKind::StorageFull
}

fn encode(line: &Line) -> String {
    serde_json::to_string(line).expect("audit lines always serialize")
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// A lossless text form of a path. Non-UTF-8 bytes become `\xNN` and a
/// backslash becomes `\\`, so two different names never log the same.
#[cfg(unix)]
pub(crate) fn escaped(path: &Path) -> String {
    use std::fmt::Write as _;
    use std::os::unix::ffi::OsStrExt;
    let mut out = String::new();
    for chunk in path.as_os_str().as_bytes().utf8_chunks() {
        out.push_str(&chunk.valid().replace('\\', "\\\\"));
        for byte in chunk.invalid() {
            let _ = write!(out, "\\x{byte:02x}");
        }
    }
    out
}

/// Unpaired UTF-16 surrogates become `\u{XXXX}`.
#[cfg(windows)]
pub(crate) fn escaped(path: &Path) -> String {
    use std::os::windows::ffi::OsStrExt;
    char::decode_utf16(path.as_os_str().encode_wide())
        .map(|c| match c {
            Ok(c) => c.to_string(),
            Err(e) => format!("\\u{{{:04x}}}", e.unpaired_surrogate()),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn non_utf8_bytes_and_backslashes_are_escaped_losslessly() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        let path = Path::new(OsStr::from_bytes(b"tmp/a\\b-\xff"));
        assert_eq!(escaped(path), r"tmp/a\\b-\xff");
        let other = Path::new(OsStr::from_bytes(br"tmp/a\\b-\xff"));
        assert_ne!(escaped(path), escaped(other));
    }

    #[test]
    fn only_the_tail_of_a_long_log_is_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        let filler = "x".repeat(1024) + "\n";
        let mut text = filler.repeat((TAIL_BYTES / 1024) as usize + 8);
        text.push_str("last line\n");
        fs::write(&path, text).unwrap();
        let tail = read_tail(&path).unwrap();
        assert!(tail.len() as u64 <= TAIL_BYTES);
        assert!(tail.starts_with('x') && tail.ends_with("last line\n"));
    }

    #[test]
    fn a_finished_run_is_not_interrupted() {
        let dir = tempfile::tempdir().unwrap();
        let audit = Audit::start(dir.path(), 0, false).unwrap();
        audit.finish(0, 0, false);
        assert!(take_interrupted(dir.path()).is_none());
    }
}
