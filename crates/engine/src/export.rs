//! The plain-text copy of a cleanup report that "Export report" saves
//! (phase 7, phase 12). Items are named by their display names. Paths
//! appear only in the audit lines included when the log could not be saved
//! to disk, as on the report screen. The text never promises more than the
//! report measured.

use std::fmt::Write as _;
use std::fs::OpenOptions;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::model::{AuditLogStatus, FreeSpace, Outcome, Report, ReportItem};

/// How many `-N` suffixes are tried when a report with this name exists.
const MAX_SUFFIX: u32 = 99;

const EXPLANATION: &str = "Free space can rise by less than the removed total, or later. \
Local snapshots (Time Machine on macOS, Btrfs or ZFS snapshots on Linux, restore points \
on Windows) can keep removed files, macOS counts some space as purgeable, the system can \
free space after a delay, and Docker's disk file does not shrink on its own.";

/// The report as text, saved at `saved_at`.
pub(crate) fn render(report: &Report, saved_at: SystemTime) -> String {
    let mut text = String::new();
    let _ = writeln!(text, "Space Optimiser cleanup report");
    let _ = writeln!(text, "Version {}", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(text, "Saved {} UTC", Civil::from(saved_at).readable());
    if report.stopped {
        let _ = writeln!(text, "\nThe cleanup was stopped before the end.");
    }

    let sections: [(Outcome, &str); 5] = [
        (Outcome::Removed, "Removed"),
        (Outcome::Ran, "Ran"),
        (Outcome::Skipped, "Skipped"),
        (Outcome::Failed, "Failed"),
        (Outcome::NotRun, "Not run"),
    ];
    for (outcome, heading) in sections {
        let items: Vec<&ReportItem> = report
            .items
            .iter()
            .filter(|i| i.outcome == outcome)
            .collect();
        if items.is_empty() {
            continue;
        }
        let _ = writeln!(text, "\n{heading} ({})", items.len());
        for item in items {
            let _ = writeln!(text, "- {}", item_line(item));
        }
    }
    let _ = writeln!(
        text,
        "\nRemoved in total: {}",
        format_bytes(report.bytes_removed)
    );

    if !report.free_space.is_empty() {
        let _ = writeln!(text, "\nFree space");
        for volume in &report.free_space {
            let _ = writeln!(text, "- {}", free_space_line(volume));
        }
    }
    let _ = writeln!(text, "\n{EXPLANATION}");
    let _ = writeln!(
        text,
        "\nCleanups cannot be undone; files do not go to the Trash."
    );
    let _ = writeln!(
        text,
        "Space Optimiser does not upload a list of your files."
    );

    if report.audit_log == AuditLogStatus::MemoryOnly {
        let _ = writeln!(
            text,
            "\nAudit log (it could not be written to disk, so it is kept here)"
        );
        for line in &report.memory_log {
            let _ = writeln!(text, "{line}");
        }
    }
    text
}

fn item_line(item: &ReportItem) -> String {
    let mut line = item.display_name.clone();
    if item.outcome == Outcome::Removed {
        let _ = write!(line, ": {}", format_bytes(item.bytes_removed));
    }
    let detail = item
        .detail
        .clone()
        .or_else(|| item.reason.map(|r| r.to_string()));
    if let Some(detail) = detail.filter(|d| !d.is_empty()) {
        let _ = write!(line, " ({detail})");
    }
    line
}

fn free_space_line(volume: &FreeSpace) -> String {
    let reading = |bytes: Option<u64>| bytes.map_or("not available".to_owned(), format_bytes);
    format!(
        "{}: {} before, {} after (read {} s after the last action; {})",
        volume.location_label,
        reading(volume.before),
        reading(volume.after),
        volume.after_delay_secs,
        volume.source,
    )
}

/// Decimal units, as the results screen shows them.
fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["bytes", "KB", "MB", "GB", "TB"];
    if bytes < 1000 {
        return format!("{bytes} bytes");
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    let digits = if value >= 100.0 { 0 } else { 1 };
    format!("{value:.digits$} {}", UNITS[unit])
}

/// Saves `text` in `dir` as `space-optimiser-report-YYYY-MM-DD-HHMMSS.txt`
/// (UTC), adding `-2` to `-99` when that name is taken. An existing file is
/// never opened or replaced.
pub(crate) fn save(dir: &Path, saved_at: SystemTime, text: &str) -> io::Result<PathBuf> {
    let stem = format!(
        "space-optimiser-report-{}",
        Civil::from(saved_at).file_stamp()
    );
    let names = std::iter::once(format!("{stem}.txt"))
        .chain((2..=MAX_SUFFIX).map(|n| format!("{stem}-{n}.txt")));
    for name in names {
        let path = dir.join(name);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                file.write_all(text.as_bytes())?;
                file.sync_all()?;
                return Ok(path);
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "too many reports were saved this second",
    ))
}

/// A UTC date and time of day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Civil {
    year: i64,
    month: u32,
    day: u32,
    hour: u64,
    minute: u64,
    second: u64,
}

impl From<SystemTime> for Civil {
    /// Times before 1970 read as 1970-01-01.
    fn from(time: SystemTime) -> Self {
        let secs = time.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let (year, month, day) = civil_from_days((secs / 86_400) as i64);
        let of_day = secs % 86_400;
        Self {
            year,
            month,
            day,
            hour: of_day / 3600,
            minute: of_day / 60 % 60,
            second: of_day % 60,
        }
    }
}

impl Civil {
    fn readable(self) -> String {
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        )
    }

    fn file_stamp(self) -> String {
        format!(
            "{:04}-{:02}-{:02}-{:02}{:02}{:02}",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        )
    }
}

/// The proleptic Gregorian date `days` after 1970-01-01 (Howard Hinnant's
/// `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{FreeSpace, PlanId};
    use std::time::Duration;

    fn at(secs: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(secs)
    }

    #[test]
    fn dates_are_utc_civil_dates() {
        assert_eq!(Civil::from(at(0)).file_stamp(), "1970-01-01-000000");
        assert_eq!(
            Civil::from(at(951_782_400)).readable(),
            "2000-02-29 00:00:00"
        );
        assert_eq!(
            Civil::from(at(1_700_000_000)).file_stamp(),
            "2023-11-14-221320"
        );
        assert_eq!(
            Civil::from(at(4_102_444_799)).readable(),
            "2099-12-31 23:59:59"
        );
    }

    #[test]
    fn sizes_use_decimal_units() {
        assert_eq!(format_bytes(999), "999 bytes");
        assert_eq!(format_bytes(1_500), "1.5 KB");
        assert_eq!(format_bytes(123_456_789), "123 MB");
        assert_eq!(format_bytes(2_000_000_000), "2.0 GB");
    }

    fn report(audit_log: AuditLogStatus) -> Report {
        Report {
            plan_id: PlanId::new(7),
            items: Vec::new(),
            bytes_removed: 0,
            stopped: false,
            free_space: vec![FreeSpace {
                location_label: "Main disk",
                before: Some(10_000_000_000),
                after: None,
                after_delay_secs: 3,
                source: "free space reported by the system",
            }],
            audit_log,
            memory_log: vec!["{\"event\":\"run_started\"}".to_owned()],
        }
    }

    #[test]
    fn the_memory_log_is_included_only_when_it_was_not_written() {
        let written = render(&report(AuditLogStatus::Written), at(0));
        assert!(!written.contains("run_started"));
        let memory = render(&report(AuditLogStatus::MemoryOnly), at(0));
        assert!(memory.contains("run_started"));
        for text in [written, memory] {
            assert!(text.contains("Main disk: 10.0 GB before, not available after"));
            assert!(text.contains("Time Machine"));
            assert!(text.contains("cannot be undone"));
            assert!(text.contains("does not upload a list of your files"));
            assert!(!text.contains("will not delete personal data"));
        }
    }

    #[test]
    fn a_taken_name_gets_a_suffix_and_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let first = save(dir.path(), at(0), "one").unwrap();
        let second = save(dir.path(), at(0), "two").unwrap();
        assert_eq!(
            first.file_name().unwrap(),
            "space-optimiser-report-1970-01-01-000000.txt"
        );
        assert_eq!(
            second.file_name().unwrap(),
            "space-optimiser-report-1970-01-01-000000-2.txt"
        );
        assert_eq!(std::fs::read_to_string(first).unwrap(), "one");
        for _ in 3..=MAX_SUFFIX {
            save(dir.path(), at(0), "more").unwrap();
        }
        assert_eq!(
            save(dir.path(), at(0), "too many").unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
    }
}
