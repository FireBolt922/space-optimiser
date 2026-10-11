//! The `user-temp` walker on fixture homes (phase 4). Nothing here deletes.

mod common;

use std::fs;
use std::time::Duration;

use common::{a_week_ahead, basic_home, set_mtime, year_2019};
use space_optimiser_engine::discover::{
    self, CancelToken, RootOutcome, ScanError, SkipReason, TempScan,
};
use space_optimiser_engine::model::{Action, Candidate, Confidence, Label, Risk};
use space_optimiser_engine::policy::{CatalogId, Context, OwnerRule, RootRejection};

fn scan(ctx: &Context) -> TempScan {
    discover::scan_user_temp(ctx, &CancelToken::new(), |_| {}).unwrap()
}

/// The single scanned root of a fixture scan.
fn scanned(scan: &TempScan) -> (&[Candidate], &[discover::Skipped]) {
    match scan.roots.as_slice() {
        [
            RootOutcome::Scanned {
                candidates,
                skipped,
                ..
            },
        ] => (candidates, skipped),
        other => panic!("expected one scanned root, got {other:?}"),
    }
}

fn candidate<'a>(candidates: &'a [Candidate], name: &str) -> Option<&'a Candidate> {
    candidates.iter().find(|c| c.display_name == name)
}

fn skip_reason(skipped: &[discover::Skipped], name: &str) -> Option<SkipReason> {
    skipped
        .iter()
        .find(|s| s.display_name == name)
        .map(|s| s.reason)
}

// Acceptance 2: discovery returns temp candidates and nothing under Documents.
#[test]
fn the_basic_fixture_returns_old_temp_entries() {
    let home = basic_home();
    let result = scan(&home.old_context());
    let (candidates, skipped) = scanned(&result);

    assert!(skipped.is_empty(), "{skipped:?}");
    let file = candidate(candidates, "old-download.bin").unwrap();
    let dir = candidate(candidates, "old-build").unwrap();
    assert_eq!(candidates.len(), 2);
    assert_eq!(dir.file_count, 2);
    for c in [file, dir] {
        assert_eq!(c.catalog_id, CatalogId::UserTemp);
        assert_eq!(c.risk, Risk::Low);
        assert!(c.bulk_safe);
        assert_eq!(c.confidence, Confidence::High);
        assert_eq!(c.action, Action::Delete);
        assert!(c.estimated_bytes > 0);
        assert!(c.path().starts_with(home.path("tmp")));
    }
    assert_ne!(file.id, dir.id);
}

#[test]
fn candidates_do_not_serialize_their_path() {
    let home = basic_home();
    let result = scan(&home.old_context());
    let (candidates, _) = scanned(&result);
    let json = serde_json::to_string(candidates).unwrap();
    assert!(!json.contains(&*home.root.to_string_lossy()), "{json}");
    assert!(json.contains("\"catalog_id\":\"user-temp\""), "{json}");
}

// Test 1: nothing under Documents is ever a candidate.
#[test]
fn nothing_under_documents_is_a_candidate() {
    let home = basic_home();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(home.path("Documents"), home.path("tmp/docs")).unwrap();
        std::os::unix::fs::symlink(home.path("Documents"), home.path("tmp/old-build/docs"))
            .unwrap();
    }
    let documents = home.path("Documents");
    for ctx in [
        home.old_context(),
        home.old_context()
            .with_temp_root(documents.clone(), OwnerRule::Root),
        home.old_context()
            .with_temp_root(home.root.clone(), OwnerRule::Root),
    ] {
        for root in scan(&ctx).roots {
            if let RootOutcome::Scanned { candidates, .. } = root {
                for c in candidates {
                    let canonical = fs::canonicalize(c.path()).unwrap();
                    assert!(
                        !canonical.starts_with(&documents),
                        "{}",
                        canonical.display()
                    );
                }
            }
        }
    }
    assert!(documents.join("notes.txt").is_file());
}

// Test 9: a temp root at the home is skipped with a reason and no candidates.
#[test]
fn a_temp_root_at_the_home_is_skipped_with_a_reason() {
    let home = basic_home();
    let ctx = Context::for_fixture(&home.root)
        .with_now(a_week_ahead())
        .with_temp_root(home.root.clone(), OwnerRule::Root);
    match scan(&ctx).roots.as_slice() {
        [RootOutcome::Skipped { reason, .. }] => assert_eq!(*reason, RootRejection::HomeOrAncestor),
        other => panic!("expected a skipped root, got {other:?}"),
    }
}

#[test]
fn the_same_root_listed_twice_is_scanned_once() {
    let home = basic_home();
    let ctx = home.old_context().with_temp_root(
        common::spelled(&home.root, "tmp/old-build/.."),
        OwnerRule::Root,
    );
    let result = scan(&ctx);
    assert_eq!(result.roots.len(), 1);
}

// Test 2: a link that points outside is skipped and not sized as its target.
#[cfg(unix)]
#[test]
fn links_are_skipped_and_never_sized_as_their_target() {
    use std::os::unix::fs::symlink;

    let home = basic_home();
    let outside = tempfile::tempdir().unwrap();
    let big = outside.path().join("big.bin");
    fs::write(&big, vec![1u8; 4 * 1024 * 1024]).unwrap();
    symlink(&big, home.path("tmp/link-to-big")).unwrap();
    symlink(outside.path(), home.path("tmp/old-build/link-to-dir")).unwrap();

    let result = scan(&home.old_context());
    let (candidates, skipped) = scanned(&result);
    assert_eq!(skip_reason(skipped, "link-to-big"), Some(SkipReason::Link));
    let dir = candidate(candidates, "old-build").unwrap();
    assert_eq!(dir.links_not_followed, 1);
    assert!(dir.estimated_bytes < 1024 * 1024, "{}", dir.estimated_bytes);
    assert!(big.is_file());
}

// Test 6: permission denied in one entry does not fail the scan.
#[cfg(unix)]
#[test]
fn an_unreadable_directory_skips_only_its_entry() {
    use std::os::unix::fs::PermissionsExt;

    // SAFETY: geteuid has no preconditions.
    if unsafe { libc::geteuid() } == 0 {
        eprintln!("skipped: root ignores directory permissions");
        return;
    }
    let home = basic_home();
    let locked = home.mkdir("tmp/locked-parent/locked");
    home.write("tmp/locked-parent/locked/inside.bin", b"x");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();

    let result = scan(&home.old_context());
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();

    let (candidates, skipped) = scanned(&result);
    assert_eq!(
        skip_reason(skipped, "locked-parent"),
        Some(SkipReason::Unreadable)
    );
    assert!(candidate(candidates, "old-build").is_some());
    assert!(candidate(candidates, "old-download.bin").is_some());
}

// Test 7: cancel stops the walk.
#[test]
fn cancel_after_the_first_progress_event_stops_the_walk() {
    let home = basic_home();
    for d in 0..30 {
        let dir = home.mkdir(&format!("tmp/deep/{}", "d/".repeat(d)));
        for f in 0..100 {
            fs::write(dir.join(format!("f{f}")), b"x").unwrap();
        }
    }
    let cancel = CancelToken::new();
    let mut events = 0;
    let result = discover::scan_user_temp(&home.old_context(), &cancel, |progress| {
        // The stage announcement comes before any file is counted.
        if progress.files_seen == 0 {
            return;
        }
        events += 1;
        cancel.cancel();
    });
    assert!(matches!(result, Err(ScanError::Cancelled)), "{result:?}");
    assert_eq!(events, 1);
}

// Test 11: a socket or FIFO anywhere inside an entry skips the whole entry.
#[cfg(unix)]
#[test]
fn a_socket_inside_an_old_directory_skips_the_directory() {
    let home = basic_home();
    home.mkdir("tmp/agent/inner");
    let _listener = std::os::unix::net::UnixListener::bind(home.path("tmp/agent/inner/s")).unwrap();
    let result = scan(&home.old_context());
    let (candidates, skipped) = scanned(&result);
    assert_eq!(
        skip_reason(skipped, "agent"),
        Some(SkipReason::SocketOrPipe)
    );
    assert!(candidate(candidates, "agent").is_none());
}

#[cfg(unix)]
#[test]
fn a_fifo_inside_an_old_directory_skips_the_directory() {
    use std::os::unix::ffi::OsStrExt;

    let home = basic_home();
    home.mkdir("tmp/ipc");
    let path = std::ffi::CString::new(home.path("tmp/ipc/fifo").as_os_str().as_bytes()).unwrap();
    // SAFETY: `path` is a valid NUL-terminated string.
    assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
    let result = scan(&home.old_context());
    let (_, skipped) = scanned(&result);
    assert_eq!(skip_reason(skipped, "ipc"), Some(SkipReason::SocketOrPipe));
}

#[cfg(unix)]
#[test]
fn a_top_level_socket_is_skipped() {
    let home = basic_home();
    let _listener = std::os::unix::net::UnixListener::bind(home.path("tmp/s")).unwrap();
    let result = scan(&home.old_context());
    let (_, skipped) = scanned(&result);
    assert_eq!(skip_reason(skipped, "s"), Some(SkipReason::SocketOrPipe));
}

// Test 12: a project marker inside an old directory skips it as a project.
#[test]
fn a_project_marker_skips_the_directory_as_a_project() {
    let home = basic_home();
    home.mkdir("tmp/checkout/.git");
    home.write("tmp/nested/sub/Cargo.toml", b"[package]");
    home.write("tmp/dotnet/App.sln", b"");
    let result = scan(&home.old_context());
    let (candidates, skipped) = scanned(&result);
    for name in ["checkout", "nested", "dotnet"] {
        assert_eq!(
            skip_reason(skipped, name),
            Some(SkipReason::Project),
            "{name}"
        );
        assert!(candidate(candidates, name).is_none());
    }
}

// Test 13: a 2019 mtime with a fresh ctime is not eligible.
#[test]
fn a_2019_mtime_with_a_fresh_change_time_is_not_eligible() {
    let home = basic_home();
    let file = home.write("tmp/backdated.bin", b"x");
    set_mtime(&file, year_2019());
    let ctx = Context::for_fixture(&home.root).with_temp_root(home.path("tmp"), OwnerRule::Root);
    let result = scan(&ctx);
    let (candidates, skipped) = scanned(&result);
    assert_eq!(
        skip_reason(skipped, "backdated.bin"),
        Some(SkipReason::Fresh)
    );
    assert!(candidates.is_empty(), "{candidates:?}");
}

#[test]
fn a_future_mtime_is_fresh() {
    let home = basic_home();
    let file = home.write("tmp/future.bin", b"x");
    set_mtime(&file, a_week_ahead() + Duration::from_secs(24 * 60 * 60));
    let result = scan(&home.old_context());
    let (_, skipped) = scanned(&result);
    assert_eq!(skip_reason(skipped, "future.bin"), Some(SkipReason::Fresh));
}

// Test 14: one fresh descendant keeps the whole directory.
#[test]
fn one_fresh_descendant_keeps_the_directory() {
    let home = basic_home();
    let fresh = home.write("tmp/old-build/nested/new.txt", b"x");
    set_mtime(&fresh, a_week_ahead() - Duration::from_secs(60 * 60));
    let result = scan(&home.old_context());
    let (candidates, skipped) = scanned(&result);
    assert_eq!(skip_reason(skipped, "old-build"), Some(SkipReason::Fresh));
    assert!(candidate(candidates, "old-download.bin").is_some());
}

// Test 18: a file with two links counts zero recoverable bytes.
#[test]
fn a_hard_linked_file_counts_zero_bytes_and_is_shared() {
    let home = basic_home();
    let file = home.write("tmp/linked.bin", &vec![1u8; 64 * 1024]);
    fs::hard_link(&file, home.path(".cache/linked.bin")).unwrap();
    let result = scan(&home.old_context());
    let (candidates, _) = scanned(&result);
    let linked = candidate(candidates, "linked.bin").unwrap();
    assert_eq!(linked.estimated_bytes, 0);
    assert!(linked.labels.contains(&Label::Shared));
}

#[test]
fn a_hard_linked_file_inside_a_directory_is_not_counted() {
    let home = basic_home();
    let file = home.write("tmp/old-build/linked.bin", &vec![1u8; 512 * 1024]);
    fs::hard_link(&file, home.path(".cache/linked.bin")).unwrap();
    let result = scan(&home.old_context());
    let (candidates, _) = scanned(&result);
    let dir = candidate(candidates, "old-build").unwrap();
    assert!(dir.estimated_bytes < 256 * 1024, "{}", dir.estimated_bytes);
    assert_eq!(dir.file_count, 3);
    assert!(dir.labels.contains(&Label::Shared));
}

// Test 19: root may not scan.
#[cfg(unix)]
#[test]
fn a_scan_as_root_is_refused() {
    let home = basic_home();
    let ctx = home.old_context().with_euid(0);
    let result = discover::scan_user_temp(&ctx, &CancelToken::new(), |_| {});
    assert!(matches!(result, Err(ScanError::RunningAsRoot)));
}

// Test 20: a non-UTF-8 name survives as an OsString.
#[cfg(target_os = "linux")]
#[test]
fn a_non_utf8_name_survives_the_round_trip() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let home = basic_home();
    let name = OsStr::from_bytes(b"caf\xe9.bin");
    fs::write(home.path("tmp").join(name), b"x").unwrap();
    let result = scan(&home.old_context());
    let (candidates, _) = scanned(&result);
    let c = candidates
        .iter()
        .find(|c| c.path().file_name() == Some(name))
        .unwrap();
    assert_eq!(c.display_name, "caf\u{fffd}.bin");
    assert!(c.path().exists());
}

// Test 17: a junction inside a cache that points at Documents is not followed.
#[cfg(windows)]
#[test]
fn a_junction_to_documents_is_not_followed() {
    let home = basic_home();
    home.write("Documents/big.bin", &vec![1u8; 4 * 1024 * 1024]);
    home.mkdir("tmp/cache-dir");
    junction(&home.path("tmp/cache-dir/docs"), &home.path("Documents"));
    junction(&home.path("tmp/docs"), &home.path("Documents"));

    let result = scan(&home.old_context());
    let (candidates, skipped) = scanned(&result);
    assert_eq!(skip_reason(skipped, "docs"), Some(SkipReason::Link));
    let dir = candidate(candidates, "cache-dir").unwrap();
    assert_eq!(dir.links_not_followed, 1);
    assert!(dir.estimated_bytes < 1024 * 1024, "{}", dir.estimated_bytes);
    assert!(home.path("Documents/big.bin").is_file());
    assert!(home.path("Documents/notes.txt").is_file());
}

/// Creates a directory junction with `FSCTL_SET_REPARSE_POINT`, without
/// spawning `mklink`.
#[cfg(windows)]
fn junction(link: &std::path::Path, target: &std::path::Path) {
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    };
    use windows_sys::Win32::System::IO::DeviceIoControl;
    use windows_sys::Win32::System::Ioctl::FSCTL_SET_REPARSE_POINT;

    const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA000_0003;

    fs::create_dir(link).unwrap();
    let target = target.as_os_str().to_string_lossy();
    let print = target.trim_start_matches(r"\\?\");
    let substitute: Vec<u16> = format!(r"\??\{print}").encode_utf16().collect();
    let print: Vec<u16> = std::ffi::OsStr::new(print).encode_wide().collect();

    let mut path_buffer = Vec::new();
    for unit in substitute
        .iter()
        .chain([&0])
        .chain(print.iter())
        .chain([&0])
    {
        path_buffer.extend_from_slice(&unit.to_le_bytes());
    }
    let data_len = (8 + path_buffer.len()) as u16;
    let mut buffer = Vec::new();
    buffer.extend_from_slice(&IO_REPARSE_TAG_MOUNT_POINT.to_le_bytes());
    buffer.extend_from_slice(&data_len.to_le_bytes());
    buffer.extend_from_slice(&0u16.to_le_bytes());
    buffer.extend_from_slice(&0u16.to_le_bytes());
    buffer.extend_from_slice(&((substitute.len() * 2) as u16).to_le_bytes());
    buffer.extend_from_slice(&(((substitute.len() + 1) * 2) as u16).to_le_bytes());
    buffer.extend_from_slice(&((print.len() * 2) as u16).to_le_bytes());
    buffer.extend_from_slice(&path_buffer);

    let handle = fs::OpenOptions::new()
        .write(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(link)
        .unwrap();
    let mut returned = 0u32;
    // SAFETY: the handle is open for writing and `buffer` holds a complete
    // mount-point reparse buffer of the length passed.
    let ok = unsafe {
        DeviceIoControl(
            handle.as_raw_handle(),
            FSCTL_SET_REPARSE_POINT,
            buffer.as_ptr().cast(),
            buffer.len() as u32,
            std::ptr::null_mut(),
            0,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    assert_ne!(ok, 0, "{}", std::io::Error::last_os_error());
}
