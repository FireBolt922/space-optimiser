//! Phase 15: what the engine does for "Check for updates". The updater
//! itself lives in the desktop shell; the engine opens the fixed download
//! page, says when it is busy, and says whether a folder is writable.

mod common;

use std::ffi::OsString;
use std::sync::{Arc, Mutex, OnceLock};

use space_optimiser_engine::platform::{self, DOWNLOAD_PAGE, RevealCommand};
use space_optimiser_engine::{Engine, EngineError};

#[test]
fn the_download_page_is_a_fixed_https_address() {
    assert_eq!(
        DOWNLOAD_PAGE,
        "https://firebolt922.github.io/space-optimiser/"
    );
}

#[test]
fn open_download_page_runs_the_os_command_with_only_the_fixed_address() {
    let home = common::basic_home();
    let calls: Arc<Mutex<Vec<RevealCommand>>> = Arc::default();
    let seen = calls.clone();
    let engine = Engine::new(home.config().with_reveal(move |cmd| {
        seen.lock().unwrap().push(cmd.clone());
        Ok(())
    }));
    match platform::open_url_command(DOWNLOAD_PAGE) {
        Some(expected) => {
            engine.open_download_page().unwrap();
            let calls = calls.lock().unwrap();
            assert_eq!(calls.as_slice(), [expected.clone()]);
            assert_eq!(expected.args, [OsString::from(DOWNLOAD_PAGE)]);
        }
        // Linux without xdg-open.
        None => assert_eq!(engine.open_download_page(), Err(EngineError::RevealFailed)),
    }
}

#[cfg(target_os = "macos")]
#[test]
fn the_download_page_opens_with_open_on_macos() {
    use std::path::PathBuf;
    let cmd = platform::open_url_command(DOWNLOAD_PAGE).unwrap();
    assert_eq!(cmd.program, PathBuf::from("/usr/bin/open"));
}

#[cfg(windows)]
#[test]
fn the_download_page_opens_with_explorer_on_windows() {
    let cmd = platform::open_url_command(DOWNLOAD_PAGE).unwrap();
    assert!(
        cmd.program
            .to_string_lossy()
            .to_ascii_lowercase()
            .ends_with(r"\explorer.exe"),
        "{cmd:?}"
    );
}

#[test]
fn an_idle_engine_and_a_finished_scan_are_not_busy() {
    let home = common::basic_home();
    let engine = home.engine();
    assert_eq!(engine.busy(), None);
    common::scan(&engine);
    assert_eq!(engine.busy(), None);
}

#[test]
fn a_running_cleanup_is_busy_and_a_finished_one_is_not() {
    let home = common::basic_home();
    let cell: Arc<OnceLock<Engine>> = Arc::new(OnceLock::new());
    let during = Arc::new(Mutex::new(None));
    let (engine_cell, out) = (cell.clone(), during.clone());
    let engine = Engine::new(home.config().with_after_item(move |_, _| {
        if let Some(engine) = engine_cell.get() {
            out.lock().unwrap().get_or_insert(engine.busy());
        }
    }));
    let _ = cell.set(engine.clone());
    let view = common::scan(&engine);
    let plan = common::plan(&engine, &view, &["old-download.bin"]);
    let token = common::confirm(&engine, &plan);
    common::execute(&engine, &plan, token);

    assert_eq!(*during.lock().unwrap(), Some(Some(EngineError::Busy)));
    assert_eq!(engine.busy(), None);
}

#[cfg(unix)]
#[test]
fn a_folder_is_writable_only_when_this_user_can_write_to_it() {
    use std::os::unix::fs::PermissionsExt;
    let home = common::basic_home();
    let open = home.mkdir("apps-open");
    let shut = home.mkdir("apps-shut");
    std::fs::set_permissions(&shut, std::fs::Permissions::from_mode(0o555)).unwrap();
    assert!(platform::is_writable_dir(&open));
    assert!(!platform::is_writable_dir(&shut));
    assert!(!platform::is_writable_dir(&home.path("missing")));
    std::fs::set_permissions(&shut, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn the_busy_error_reaches_the_ui_as_a_kind_and_a_message() {
    let json = serde_json::to_value(EngineError::Busy).unwrap();
    assert_eq!(json["kind"], "busy");
    assert!(!json["message"].as_str().unwrap().is_empty());
}
