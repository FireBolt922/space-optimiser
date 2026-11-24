//! "Check for updates" (phase 15) with Tauri's signed updater, used from
//! Rust only: the webview has no updater permission and calls these
//! commands instead.
//!
//! - Nothing is checked until the person asks (or turned on the check at
//!   start), and nothing is downloaded until they press Install.
//! - Every download is verified against the public key in tauri.conf.json
//!   before it replaces anything.
//! - The app is never replaced while a scan or a cleanup runs.
//! - It never asks for an administrator password: on Linux (a .deb needs
//!   one) and on a Mac whose app folder this user cannot write, the person
//!   gets the download page instead.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use serde::Serialize;
use space_optimiser_engine::{Engine, EngineError};
use tauri::{AppHandle, Runtime, State};
use tauri_plugin_updater::{Update, UpdaterExt};

/// How long a check or a download may wait on the network before failing.
const NETWORK_TIMEOUT: Duration = Duration::from_secs(60);

/// How often a finished download looks again for a scan or cleanup to end.
const BUSY_RETRY: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Os {
    Macos,
    Windows,
    Linux,
}

impl Os {
    const CURRENT: Os = if cfg!(target_os = "macos") {
        Os::Macos
    } else if cfg!(windows) {
        Os::Windows
    } else {
        Os::Linux
    };
}

/// How a found update gets onto this computer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallRoute {
    /// Downloaded, verified, and installed by the app.
    InApp,
    /// Linux: installing a .deb needs an administrator password.
    LinuxPackage,
    /// macOS: this user cannot write the folder that holds the app.
    AppFolderLocked,
    /// macOS: not running from an installed .app (a development build).
    NotInstalled,
}

/// What the Settings screen shows. Notes are plain text from the release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum UpdateView {
    #[default]
    Idle,
    Checking,
    UpToDate {
        current_version: String,
    },
    Available {
        version: String,
        current_version: String,
        notes: Option<String>,
        date: Option<String>,
        route: InstallRoute,
    },
    Downloading {
        version: String,
        downloaded: u64,
        total: Option<u64>,
    },
    /// Downloaded and verified; waiting for a scan or cleanup to finish.
    Waiting {
        version: String,
    },
    /// Installed; takes effect when the app restarts (macOS).
    Installed {
        version: String,
    },
    Failed {
        message: String,
    },
}

#[derive(Default)]
struct Inner {
    view: UpdateView,
    pending: Option<Update>,
}

/// The updater's state, shared with the download task.
#[derive(Clone, Default)]
pub struct Updates(Arc<Mutex<Inner>>);

impl Updates {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn set(&self, view: UpdateView) {
        self.lock().view = view;
    }

    fn view(&self) -> UpdateView {
        self.lock().view.clone()
    }
}

/// Linux never installs in the app; Windows always runs the installer the
/// person chose; macOS replaces the .app only when its folder is writable.
pub(crate) fn install_route(os: Os, app_folder_writable: Option<bool>) -> InstallRoute {
    match os {
        Os::Linux => InstallRoute::LinuxPackage,
        Os::Windows => InstallRoute::InApp,
        Os::Macos => match app_folder_writable {
            Some(true) => InstallRoute::InApp,
            Some(false) => InstallRoute::AppFolderLocked,
            None => InstallRoute::NotInstalled,
        },
    }
}

/// The folder that holds the `.app` this executable is inside, if any.
pub(crate) fn app_bundle_folder(exe: &Path) -> Option<PathBuf> {
    exe.ancestors()
        .find(|p| p.extension().is_some_and(|e| e == "app"))
        .and_then(Path::parent)
        .map(Path::to_path_buf)
}

fn this_route() -> InstallRoute {
    let writable = std::env::current_exe()
        .ok()
        .and_then(|exe| app_bundle_folder(&exe))
        .map(|folder| space_optimiser_engine::platform::is_writable_dir(&folder));
    install_route(Os::CURRENT, writable)
}

/// Asks the update server for the newest version. Returns the new state;
/// a check during a download or after an install changes nothing.
#[tauri::command]
pub async fn check_for_update<R: Runtime>(
    app: AppHandle<R>,
    updates: State<'_, Updates>,
) -> Result<UpdateView, EngineError> {
    {
        let mut inner = updates.lock();
        if matches!(
            inner.view,
            UpdateView::Checking
                | UpdateView::Downloading { .. }
                | UpdateView::Waiting { .. }
                | UpdateView::Installed { .. }
        ) {
            return Ok(inner.view.clone());
        }
        inner.view = UpdateView::Checking;
        inner.pending = None;
    }
    let current_version = app.package_info().version.to_string();
    let result = match app.updater_builder().timeout(NETWORK_TIMEOUT).build() {
        Ok(updater) => updater.check().await,
        Err(e) => Err(e),
    };
    let mut inner = updates.lock();
    inner.view = match result {
        Ok(Some(update)) => {
            let view = UpdateView::Available {
                version: update.version.clone(),
                current_version,
                notes: update.body.clone().filter(|n| !n.trim().is_empty()),
                date: update.date.map(|d| d.date().to_string()),
                route: this_route(),
            };
            inner.pending = Some(update);
            view
        }
        Ok(None) => UpdateView::UpToDate { current_version },
        Err(e) => UpdateView::Failed {
            message: format!("could not check for updates: {e}"),
        },
    };
    Ok(inner.view.clone())
}

#[tauri::command]
pub fn update_status(updates: State<'_, Updates>) -> UpdateView {
    updates.view()
}

/// Downloads, verifies, and installs the update the last check found.
/// Refused while a scan or a cleanup runs. On Windows the installer closes
/// the app; on macOS the new version starts on the next launch.
#[tauri::command]
pub fn install_update(
    updates: State<'_, Updates>,
    engine: State<'_, Engine>,
) -> Result<UpdateView, EngineError> {
    if let Some(busy) = engine.busy() {
        return Err(busy);
    }
    let update = {
        let mut inner = updates.lock();
        let route_ok = matches!(
            inner.view,
            UpdateView::Available {
                route: InstallRoute::InApp,
                ..
            }
        );
        let Some(update) = inner.pending.clone().filter(|_| route_ok) else {
            return Ok(inner.view.clone());
        };
        inner.view = UpdateView::Downloading {
            version: update.version.clone(),
            downloaded: 0,
            total: None,
        };
        update
    };
    let (state, engine) = (updates.inner().clone(), engine.inner().clone());
    tauri::async_runtime::spawn(async move {
        let version = update.version.clone();
        let progress = state.clone();
        let mut downloaded = 0u64;
        let bytes = update
            .download(
                |chunk, total| {
                    downloaded += chunk as u64;
                    progress.set(UpdateView::Downloading {
                        version: version.clone(),
                        downloaded,
                        total,
                    });
                },
                || {},
            )
            .await;
        let bytes = match bytes {
            Ok(bytes) => bytes,
            Err(e) => {
                state.set(UpdateView::Failed {
                    message: format!("could not download the update: {e}"),
                });
                return;
            }
        };
        // A scan or cleanup started during the download finishes first.
        while engine.busy().is_some() {
            state.set(UpdateView::Waiting {
                version: version.clone(),
            });
            pause(BUSY_RETRY).await;
        }
        state.set(match update.install(bytes) {
            Ok(()) => UpdateView::Installed { version },
            Err(e) => UpdateView::Failed {
                message: format!("could not install the update: {e}"),
            },
        });
    });
    Ok(updates.view())
}

/// Restarts into the installed version. Refused while a scan or a cleanup
/// runs.
#[tauri::command]
pub fn restart_app<R: Runtime>(
    app: AppHandle<R>,
    engine: State<'_, Engine>,
) -> Result<(), EngineError> {
    if let Some(busy) = engine.busy() {
        return Err(busy);
    }
    app.restart()
}

/// A one-second wait on a blocking thread, so the updater needs no timer
/// dependency of its own.
async fn pause(duration: Duration) {
    let _ = tauri::async_runtime::spawn_blocking(move || std::thread::sleep(duration)).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_always_gets_the_download_page() {
        for writable in [Some(true), Some(false), None] {
            assert_eq!(
                install_route(Os::Linux, writable),
                InstallRoute::LinuxPackage
            );
        }
    }

    #[test]
    fn windows_installs_in_the_app() {
        assert_eq!(install_route(Os::Windows, None), InstallRoute::InApp);
    }

    #[test]
    fn macos_installs_only_into_a_writable_app_folder() {
        assert_eq!(install_route(Os::Macos, Some(true)), InstallRoute::InApp);
        assert_eq!(
            install_route(Os::Macos, Some(false)),
            InstallRoute::AppFolderLocked
        );
        assert_eq!(install_route(Os::Macos, None), InstallRoute::NotInstalled);
    }

    #[test]
    fn the_app_folder_is_the_parent_of_the_app_bundle() {
        let exe = Path::new("/Applications/Space Optimiser.app/Contents/MacOS/space-optimiser");
        assert_eq!(app_bundle_folder(exe), Some(PathBuf::from("/Applications")));
        let dev = Path::new("/work/target/debug/space-optimiser");
        assert_eq!(app_bundle_folder(dev), None);
    }

    #[test]
    fn views_reach_the_ui_tagged_by_state() {
        let view = UpdateView::Available {
            version: "0.6.0".into(),
            current_version: "0.5.0".into(),
            notes: None,
            date: None,
            route: InstallRoute::LinuxPackage,
        };
        let json = serde_json::to_value(&view).unwrap();
        assert_eq!(json["state"], "available");
        assert_eq!(json["route"], "linux_package");
        let json = serde_json::to_value(UpdateView::Idle).unwrap();
        assert_eq!(json, serde_json::json!({ "state": "idle" }));
    }
}
