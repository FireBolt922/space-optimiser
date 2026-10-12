//! Where each cache row looks (phase 5 items 1, 2, 3, and 6; phase 6).
//! Every location is a fixed suffix below the home, or below Local AppData
//! on Windows. Nothing here reads inside a cache: it lists profile folders,
//! picks the cache folders by exact name, and checks each one with
//! [`policy::validate_cache_root`]. The scanner lists roots with this module
//! and the executor lists them again right before deleting.
//!
//! Vendor locations, for review against current vendor documentation:
//!
//! | Browser | macOS (`~/Library/Caches`, `~/Library/Application Support`) | Ubuntu (`~/.cache`, `~/.config`) | Windows (Local AppData) |
//! | --- | --- | --- | --- |
//! | Chrome | `Google/Chrome`, `… Beta`, `… Dev`, `… Canary` | `google-chrome`, `-beta`, `-unstable` | `Google\Chrome`, `… Beta`, `… Dev`, `… SxS` + `\User Data` |
//! | Edge | `Microsoft Edge`, `… Beta`, `… Dev`, `… Canary` | `microsoft-edge`, `-beta`, `-dev` | `Microsoft\Edge`, `… Beta`, `… Dev`, `… SxS` + `\User Data` |
//! | Brave | `BraveSoftware/Brave-Browser`, `-Beta`, `-Nightly` | same | same + `\User Data` |
//! | Chromium | — | `chromium`; Snap `~/snap/chromium/common/{.cache,}`; Flatpak | — |
//! | Firefox | `~/Library/Caches/Firefox/Profiles` | `~/.cache/mozilla/firefox`; Snap; Flatpak | `Mozilla\Firefox\Profiles` |
//!
//! Chromium-family profiles are `Default` and `Profile <n>`; inside one,
//! only `Cache`, `Code Cache`, and `GPUCache` match (the catalog
//! carve-out). Firefox keeps its cache apart from the profile, so each
//! profile folder under the cache root contributes its `cache2` only.
//!
//! Editor locations (`ide-index-caches`), for review against the vendors'
//! documentation (VS Code "Portable mode", user data directory; JetBrains
//! "Directories used by the IDE", system directory):
//!
//! | Editor | macOS | Ubuntu | Windows |
//! | --- | --- | --- | --- |
//! | VS Code, VS Code Insiders, Cursor | `~/Library/Application Support/<Code \| Code - Insiders \| Cursor>` | `~/.config/<same>` | `%APPDATA%\<same>` (Roaming) |
//! | JetBrains IDEs | `~/Library/Caches/JetBrains/<Product><Version>` | `~/.cache/JetBrains/<Product><Version>` | `%LOCALAPPDATA%\JetBrains\<Product><Version>` |
//!
//! Inside a VS Code-family user-data folder only `Cache`, `CachedData`,
//! `Code Cache`, `GPUCache`, and `CachedExtensionVSIXs` match; `User`,
//! storage, backups, and logs stay. Inside a JetBrains product folder only
//! `caches` and `index` match; `LocalHistory`, `log`, `tmp`, and plugins
//! stay. A JetBrains folder counts only when its name is a known product
//! followed by a version (`IntelliJIdea2024.3`); `Toolbox` and unknown
//! folders are ignored.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::model::{Platform, SkipCause};
use crate::policy::{self, CatalogId, Context, SkipReason, ValidRoot};

/// An app that must be quit before its cache is cleaned: a browser for
/// `browser-http-cache`, an editor for `ide-index-caches`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum App {
    Chrome,
    Edge,
    Brave,
    Chromium,
    Firefox,
    VsCode,
    VsCodeInsiders,
    Cursor,
    IntelliJIdea,
    PyCharm,
    WebStorm,
    GoLand,
    CLion,
    Rider,
    RustRover,
    DataGrip,
    PhpStorm,
    RubyMine,
    DataSpell,
    Aqua,
}

impl App {
    const ALL: [App; 20] = [
        App::Chrome,
        App::Edge,
        App::Brave,
        App::Chromium,
        App::Firefox,
        App::VsCode,
        App::VsCodeInsiders,
        App::Cursor,
        App::IntelliJIdea,
        App::PyCharm,
        App::WebStorm,
        App::GoLand,
        App::CLion,
        App::Rider,
        App::RustRover,
        App::DataGrip,
        App::PhpStorm,
        App::RubyMine,
        App::DataSpell,
        App::Aqua,
    ];

    /// The name shown to the person ("Quit VS Code first").
    pub const fn label(self) -> &'static str {
        match self {
            App::Chrome => "Chrome",
            App::Edge => "Edge",
            App::Brave => "Brave",
            App::Chromium => "Chromium",
            App::Firefox => "Firefox",
            App::VsCode => "VS Code",
            App::VsCodeInsiders => "VS Code Insiders",
            App::Cursor => "Cursor",
            App::IntelliJIdea => "IntelliJ IDEA",
            App::PyCharm => "PyCharm",
            App::WebStorm => "WebStorm",
            App::GoLand => "GoLand",
            App::CLion => "CLion",
            App::Rider => "Rider",
            App::RustRover => "RustRover",
            App::DataGrip => "DataGrip",
            App::PhpStorm => "PhpStorm",
            App::RubyMine => "RubyMine",
            App::DataSpell => "DataSpell",
            App::Aqua => "Aqua",
        }
    }

    /// Main-process names for every channel. An app counts as running if
    /// any of them is. Linux names are `comm`, cut to 15 bytes. Editor
    /// helpers ("Code Helper (Renderer)") are not the editor. A JetBrains
    /// IDE started through an old `idea.sh` shows as `java` and is not
    /// seen; the catalog row says so.
    const fn processes(self, platform: Platform) -> &'static [&'static str] {
        match (self, platform) {
            (App::VsCode, Platform::Macos) => &["Code"],
            (App::VsCode, Platform::Linux) => &["code"],
            (App::VsCode, Platform::Windows) => &["Code.exe"],
            (App::VsCodeInsiders, Platform::Macos) => &["Code - Insiders"],
            (App::VsCodeInsiders, Platform::Linux) => &["code-insiders"],
            (App::VsCodeInsiders, Platform::Windows) => &["Code - Insiders.exe"],
            (App::Cursor, Platform::Macos) => &["Cursor"],
            (App::Cursor, Platform::Linux) => &["cursor"],
            (App::Cursor, Platform::Windows) => &["Cursor.exe"],
            (App::IntelliJIdea, Platform::Macos | Platform::Linux) => &["idea"],
            (App::IntelliJIdea, Platform::Windows) => &["idea64.exe"],
            (App::PyCharm, Platform::Macos | Platform::Linux) => &["pycharm"],
            (App::PyCharm, Platform::Windows) => &["pycharm64.exe"],
            (App::WebStorm, Platform::Macos | Platform::Linux) => &["webstorm"],
            (App::WebStorm, Platform::Windows) => &["webstorm64.exe"],
            (App::GoLand, Platform::Macos | Platform::Linux) => &["goland"],
            (App::GoLand, Platform::Windows) => &["goland64.exe"],
            (App::CLion, Platform::Macos | Platform::Linux) => &["clion"],
            (App::CLion, Platform::Windows) => &["clion64.exe"],
            (App::Rider, Platform::Macos | Platform::Linux) => &["rider"],
            (App::Rider, Platform::Windows) => &["rider64.exe"],
            (App::RustRover, Platform::Macos | Platform::Linux) => &["rustrover"],
            (App::RustRover, Platform::Windows) => &["rustrover64.exe"],
            (App::DataGrip, Platform::Macos | Platform::Linux) => &["datagrip"],
            (App::DataGrip, Platform::Windows) => &["datagrip64.exe"],
            (App::PhpStorm, Platform::Macos | Platform::Linux) => &["phpstorm"],
            (App::PhpStorm, Platform::Windows) => &["phpstorm64.exe"],
            (App::RubyMine, Platform::Macos | Platform::Linux) => &["rubymine"],
            (App::RubyMine, Platform::Windows) => &["rubymine64.exe"],
            (App::DataSpell, Platform::Macos | Platform::Linux) => &["dataspell"],
            (App::DataSpell, Platform::Windows) => &["dataspell64.exe"],
            (App::Aqua, Platform::Macos | Platform::Linux) => &["aqua"],
            (App::Aqua, Platform::Windows) => &["aqua64.exe"],
            (App::Chrome, Platform::Macos) => &[
                "Google Chrome",
                "Google Chrome Beta",
                "Google Chrome Dev",
                "Google Chrome Canary",
            ],
            (App::Chrome, Platform::Linux) => &["chrome"],
            (App::Chrome, Platform::Windows) => &["chrome.exe"],
            (App::Edge, Platform::Macos) => &[
                "Microsoft Edge",
                "Microsoft Edge Beta",
                "Microsoft Edge Dev",
                "Microsoft Edge Canary",
            ],
            (App::Edge, Platform::Linux) => &["msedge"],
            (App::Edge, Platform::Windows) => &["msedge.exe"],
            (App::Brave, Platform::Macos) => &[
                "Brave Browser",
                "Brave Browser Beta",
                "Brave Browser Nightly",
            ],
            (App::Brave, Platform::Linux) => &["brave"],
            (App::Brave, Platform::Windows) => &["brave.exe"],
            (App::Chromium, Platform::Macos) => &["Chromium"],
            // The Snap runs `…/chromium-browser/chrome`.
            (App::Chromium, Platform::Linux) => &["chromium", "chromium-browse", "chrome"],
            (App::Chromium, Platform::Windows) => &["chromium.exe"],
            (App::Firefox, Platform::Macos) => &["firefox"],
            (App::Firefox, Platform::Linux) => &["firefox", "firefox-bin", "firefox-esr"],
            (App::Firefox, Platform::Windows) => &["firefox.exe"],
        }
    }
}

/// The apps among `names`. Windows compares without case.
pub fn running_apps(names: &[String], platform: Platform) -> Vec<App> {
    App::ALL
        .into_iter()
        .filter(|b| {
            b.processes(platform).iter().any(|p| {
                names.iter().any(|n| match platform {
                    Platform::Windows => n.eq_ignore_ascii_case(p),
                    _ => n == p,
                })
            })
        })
        .collect()
}

/// One cache folder a row found, valid or with the reason it is not.
#[derive(Debug, Clone)]
pub struct CacheRoot {
    pub display_name: String,
    /// The app that must be quit first (browser rows and editor rows).
    pub app: Option<App>,
    pub root: Result<ValidRoot, SkipReason>,
}

/// The cache folders `row` covers on `platform`. Only folders that exist
/// are listed: a missing browser or editor is a quiet absence. Rows that
/// run a tool, and the large-file report, have none.
pub fn cache_roots(ctx: &Context, row: CatalogId, platform: Platform) -> Vec<CacheRoot> {
    let home = ctx.home();
    let mut found = Vec::new();
    match (row, platform) {
        (CatalogId::ThumbnailCache, Platform::Linux) => {
            fixed(
                ctx,
                &join(home, ".cache/thumbnails"),
                "Thumbnails",
                &mut found,
            );
        }
        (CatalogId::FontCache, Platform::Linux) => {
            fixed(
                ctx,
                &join(home, ".cache/fontconfig"),
                "Font cache",
                &mut found,
            );
        }
        (CatalogId::WindowsThumbcache, Platform::Windows) => {
            let explorer = join(ctx.local_app_data(), "Microsoft/Windows/Explorer");
            fixed(ctx, &explorer, "Explorer thumbnails", &mut found);
        }
        (CatalogId::BrowserHttpCache, platform) => browsers(ctx, platform, &mut found),
        (CatalogId::IdeIndexCaches, platform) => editors(ctx, platform, &mut found),
        (CatalogId::PkgNpx, Platform::Windows) => {
            let npx = join(ctx.local_app_data(), "npm-cache/_npx");
            fixed(ctx, &npx, "~/AppData/Local/npm-cache/_npx", &mut found);
        }
        (CatalogId::PkgNpx, _) => {
            fixed(ctx, &join(home, ".npm/_npx"), "~/.npm/_npx", &mut found);
        }
        (CatalogId::PkgCargoCache, _) => {
            for relative in [".cargo/registry/cache", ".cargo/git/checkouts"] {
                fixed(
                    ctx,
                    &join(home, relative),
                    &format!("~/{relative}"),
                    &mut found,
                );
            }
        }
        (CatalogId::PkgGradleCaches, _) => {
            fixed(
                ctx,
                &join(home, ".gradle/caches"),
                "~/.gradle/caches",
                &mut found,
            );
        }
        (CatalogId::PkgNugetHttp, Platform::Windows) => {
            for relative in ["NuGet/v3-cache", "NuGet/http-cache"] {
                let path = join(ctx.local_app_data(), relative);
                let name = format!("~/AppData/Local/{relative}");
                fixed(ctx, &path, &name, &mut found);
            }
        }
        (CatalogId::PkgNugetHttp, _) => {
            for relative in [
                ".local/share/NuGet/http-cache",
                ".local/share/NuGet/v3-cache",
            ] {
                fixed(
                    ctx,
                    &join(home, relative),
                    &format!("~/{relative}"),
                    &mut found,
                );
            }
        }
        (CatalogId::UserLogsOld, Platform::Macos) => {
            fixed(
                ctx,
                &join(home, "Library/Logs"),
                "~/Library/Logs",
                &mut found,
            );
        }
        _ => {}
    }
    found
}

/// Where a tool row's cache is when the tool does not say (phase 6 item
/// 2). Used only to show the folder as "not sure"; nothing is offered.
pub(crate) fn default_tool_cache(
    ctx: &Context,
    row: CatalogId,
    platform: Platform,
) -> Option<PathBuf> {
    let home = ctx.home();
    let local = ctx.local_app_data();
    let path = match (row, platform) {
        (CatalogId::PkgNpmCache, Platform::Windows) => join(local, "npm-cache"),
        (CatalogId::PkgNpmCache, _) => join(home, ".npm"),
        (CatalogId::PkgPnpmStore, Platform::Macos) => join(home, "Library/pnpm/store"),
        (CatalogId::PkgPnpmStore, Platform::Linux) => join(home, ".local/share/pnpm/store"),
        (CatalogId::PkgPnpmStore, Platform::Windows) => join(local, "pnpm/store"),
        (CatalogId::PkgYarnCache, Platform::Macos) => join(home, "Library/Caches/Yarn"),
        (CatalogId::PkgYarnCache, Platform::Linux) => join(home, ".cache/yarn"),
        (CatalogId::PkgYarnCache, Platform::Windows) => join(local, "Yarn/Cache"),
        (CatalogId::PkgPipCache, Platform::Macos) => join(home, "Library/Caches/pip"),
        (CatalogId::PkgPipCache, Platform::Linux) => join(home, ".cache/pip"),
        (CatalogId::PkgPipCache, Platform::Windows) => join(local, "pip/cache"),
        (CatalogId::PkgUvCache, Platform::Windows) => join(local, "uv/cache"),
        (CatalogId::PkgUvCache, _) => join(home, ".cache/uv"),
        _ => return None,
    };
    Some(path)
}

/// Checks a planned cache folder again, immediately before deleting it:
/// the row must still list it, with the identity the scan recorded.
/// Returns the app to check for.
pub(crate) fn recheck(
    ctx: &Context,
    row: CatalogId,
    planned: &ValidRoot,
) -> Result<Option<App>, SkipCause> {
    let found = cache_roots(ctx, row, Platform::CURRENT)
        .into_iter()
        .find_map(|r| match r.root {
            Ok(root) if root.path() == planned.path() => Some((root, r.app)),
            _ => None,
        });
    match found {
        Some((root, app)) if root.id() == planned.id() => Ok(app),
        Some(_) => Err(SkipCause::ChangedSinceScan),
        None if fs::symlink_metadata(planned.path()).is_err() => Err(SkipCause::Gone),
        None => Err(SkipCause::ChangedSinceScan),
    }
}

/// `relative` uses `/`; joined one component at a time so it also works
/// below a Windows `\\?\` path.
fn join(base: &Path, relative: &str) -> PathBuf {
    relative
        .split('/')
        .fold(base.to_path_buf(), |p, c| p.join(c))
}

/// Adds `path` if it exists: validated if it is a folder, rejected if it
/// is a link. Anything else (a file with a cache's name) is not a cache.
fn fixed(ctx: &Context, path: &Path, display_name: &str, found: &mut Vec<CacheRoot>) {
    add(ctx, path, display_name.to_owned(), None, found);
}

fn add(
    ctx: &Context,
    path: &Path,
    display_name: String,
    app: Option<App>,
    found: &mut Vec<CacheRoot>,
) {
    ctx.visit(path);
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return;
    };
    if !(metadata.is_dir() || metadata.file_type().is_symlink() || is_reparse_point(&metadata)) {
        return;
    }
    found.push(CacheRoot {
        display_name,
        app,
        root: policy::validate_cache_root(ctx, path),
    });
}

#[cfg(windows)]
fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_reparse_point(_metadata: &fs::Metadata) -> bool {
    false
}

/// The names of the folders directly inside `dir`, sorted. Links are left
/// out: a profile is never reached through one.
fn folders(ctx: &Context, dir: &Path) -> Vec<String> {
    ctx.visit(dir);
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|e| {
            fs::symlink_metadata(e.path()).is_ok_and(|m| m.is_dir() && !is_reparse_point(&m))
        })
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    names.sort();
    names
}

/// `Default` or `Profile <n>`.
fn is_chromium_profile(name: &str) -> bool {
    name == "Default"
        || name
            .strip_prefix("Profile ")
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

const CHROMIUM_CACHE_FOLDERS: [&str; 3] = ["Cache", "Code Cache", "GPUCache"];

/// One Chromium-family install: the folders that hold its profiles.
struct Chromium {
    browser: App,
    label: &'static str,
    /// Each holds profile folders, for example `~/.cache/google-chrome`.
    bases: Vec<PathBuf>,
}

/// One Firefox install: the folder whose profile folders hold `cache2`.
struct Firefox {
    label: &'static str,
    profiles: PathBuf,
}

fn browsers(ctx: &Context, platform: Platform, found: &mut Vec<CacheRoot>) {
    let (chromium, firefox) = layout(ctx, platform);
    for install in chromium {
        for base in &install.bases {
            for profile in folders(ctx, base).iter().filter(|p| is_chromium_profile(p)) {
                for folder in CHROMIUM_CACHE_FOLDERS {
                    let path = base.join(profile).join(folder);
                    let name = format!("{} — {profile} — {folder}", install.label);
                    add(ctx, &path, name, Some(install.browser), found);
                }
            }
        }
    }
    for install in firefox {
        for profile in folders(ctx, &install.profiles) {
            let path = install.profiles.join(&profile).join("cache2");
            let name = format!("{} — {profile}", install.label);
            add(ctx, &path, name, Some(App::Firefox), found);
        }
    }
}

/// The VS Code-family cache folders, by exact name, directly inside the
/// user-data folder (the catalog row).
const EDITOR_CACHE_FOLDERS: [&str; 5] = [
    "Cache",
    "CachedData",
    "Code Cache",
    "GPUCache",
    "CachedExtensionVSIXs",
];

/// VS Code-family editors: (app, user-data folder name). The name is the
/// same on every OS, below the platform's config root.
const ELECTRON_EDITORS: [(App, &str); 3] = [
    (App::VsCode, "Code"),
    (App::VsCodeInsiders, "Code - Insiders"),
    (App::Cursor, "Cursor"),
];

/// Inside a JetBrains product folder, only these two (the catalog row).
const JETBRAINS_CACHE_FOLDERS: [&str; 2] = ["caches", "index"];

/// JetBrains products: (app, folder-name prefixes). The folder is the
/// prefix followed by a version (`IdeaIC2024.3`).
const JETBRAINS: [(App, &[&str]); 12] = [
    (App::IntelliJIdea, &["IntelliJIdea", "IdeaIC"]),
    (App::PyCharm, &["PyCharm", "PyCharmCE"]),
    (App::WebStorm, &["WebStorm"]),
    (App::GoLand, &["GoLand"]),
    (App::CLion, &["CLion"]),
    (App::Rider, &["Rider"]),
    (App::RustRover, &["RustRover"]),
    (App::DataGrip, &["DataGrip"]),
    (App::PhpStorm, &["PhpStorm"]),
    (App::RubyMine, &["RubyMine"]),
    (App::DataSpell, &["DataSpell"]),
    (App::Aqua, &["Aqua"]),
];

fn editors(ctx: &Context, platform: Platform, found: &mut Vec<CacheRoot>) {
    let home = ctx.home();
    let (config, jetbrains) = match platform {
        Platform::Macos => (
            join(home, "Library/Application Support"),
            join(home, "Library/Caches/JetBrains"),
        ),
        Platform::Linux => (join(home, ".config"), join(home, ".cache/JetBrains")),
        Platform::Windows => (
            ctx.roaming_app_data().to_path_buf(),
            join(ctx.local_app_data(), "JetBrains"),
        ),
    };
    for (app, dir) in ELECTRON_EDITORS {
        let base = config.join(dir);
        for folder in EDITOR_CACHE_FOLDERS {
            let name = format!("{} — {folder}", app.label());
            add(ctx, &base.join(folder), name, Some(app), found);
        }
    }
    for product in folders(ctx, &jetbrains) {
        let Some((app, version)) = jetbrains_product(&product) else {
            continue;
        };
        let base = jetbrains.join(&product);
        for folder in JETBRAINS_CACHE_FOLDERS {
            let name = format!("{} {version} — {folder}", app.label());
            add(ctx, &base.join(folder), name, Some(app), found);
        }
    }
}

/// `IntelliJIdea2024.3` → (IntelliJ IDEA, "2024.3"). A folder that is not
/// a known product followed by a version (`Toolbox`, `jcef_cache`) is None.
fn jetbrains_product(folder: &str) -> Option<(App, &str)> {
    for (app, prefixes) in JETBRAINS {
        for prefix in prefixes {
            if let Some(version) = folder.strip_prefix(prefix) {
                if is_version(version) {
                    return Some((app, version));
                }
            }
        }
    }
    None
}

/// Digits and dots, starting and ending with a digit: `2024.3`, `2024.3.1`.
fn is_version(s: &str) -> bool {
    !s.is_empty()
        && s.bytes().all(|b| b.is_ascii_digit() || b == b'.')
        && !s.starts_with('.')
        && !s.ends_with('.')
        && !s.contains("..")
}

/// Chromium-family vendor folders: (browser, label, macOS, Linux, Windows).
/// The Windows path is below Local AppData and gains `User Data`.
const VENDORS: &[(App, &str, &str, &str, &str)] = &[
    (
        App::Chrome,
        "Google Chrome",
        "Google/Chrome",
        "google-chrome",
        "Google/Chrome",
    ),
    (
        App::Chrome,
        "Google Chrome Beta",
        "Google/Chrome Beta",
        "google-chrome-beta",
        "Google/Chrome Beta",
    ),
    (
        App::Chrome,
        "Google Chrome Dev",
        "Google/Chrome Dev",
        "google-chrome-unstable",
        "Google/Chrome Dev",
    ),
    (
        App::Chrome,
        "Google Chrome Canary",
        "Google/Chrome Canary",
        "",
        "Google/Chrome SxS",
    ),
    (
        App::Edge,
        "Microsoft Edge",
        "Microsoft Edge",
        "microsoft-edge",
        "Microsoft/Edge",
    ),
    (
        App::Edge,
        "Microsoft Edge Beta",
        "Microsoft Edge Beta",
        "microsoft-edge-beta",
        "Microsoft/Edge Beta",
    ),
    (
        App::Edge,
        "Microsoft Edge Dev",
        "Microsoft Edge Dev",
        "microsoft-edge-dev",
        "Microsoft/Edge Dev",
    ),
    (
        App::Edge,
        "Microsoft Edge Canary",
        "Microsoft Edge Canary",
        "",
        "Microsoft/Edge SxS",
    ),
    (
        App::Brave,
        "Brave",
        "BraveSoftware/Brave-Browser",
        "BraveSoftware/Brave-Browser",
        "BraveSoftware/Brave-Browser",
    ),
    (
        App::Brave,
        "Brave Beta",
        "BraveSoftware/Brave-Browser-Beta",
        "BraveSoftware/Brave-Browser-Beta",
        "BraveSoftware/Brave-Browser-Beta",
    ),
    (
        App::Brave,
        "Brave Nightly",
        "BraveSoftware/Brave-Browser-Nightly",
        "BraveSoftware/Brave-Browser-Nightly",
        "BraveSoftware/Brave-Browser-Nightly",
    ),
    (App::Chromium, "Chromium", "", "chromium", ""),
];

/// Flatpak apps keep their caches in `~/.var/app/<id>/{cache,config}`:
/// (browser, label, app id, folder inside).
const FLATPAKS: &[(App, &str, &str, &str)] = &[
    (
        App::Chrome,
        "Google Chrome (Flatpak)",
        "com.google.Chrome",
        "google-chrome",
    ),
    (
        App::Edge,
        "Microsoft Edge (Flatpak)",
        "com.microsoft.Edge",
        "microsoft-edge",
    ),
    (
        App::Brave,
        "Brave (Flatpak)",
        "com.brave.Browser",
        "BraveSoftware/Brave-Browser",
    ),
    (
        App::Chromium,
        "Chromium (Flatpak)",
        "org.chromium.Chromium",
        "chromium",
    ),
];

fn layout(ctx: &Context, platform: Platform) -> (Vec<Chromium>, Vec<Firefox>) {
    let home = ctx.home();
    let mut chromium = Vec::new();
    let mut firefox = Vec::new();
    match platform {
        Platform::Macos => {
            let caches = join(home, "Library/Caches");
            let support = join(home, "Library/Application Support");
            for &(browser, label, dir, _, _) in VENDORS {
                if !dir.is_empty() {
                    chromium.push(Chromium {
                        browser,
                        label,
                        bases: vec![join(&caches, dir), join(&support, dir)],
                    });
                }
            }
            firefox.push(Firefox {
                label: "Firefox",
                profiles: join(&caches, "Firefox/Profiles"),
            });
        }
        Platform::Linux => {
            let cache = join(home, ".cache");
            let config = join(home, ".config");
            for &(browser, label, _, dir, _) in VENDORS {
                if !dir.is_empty() {
                    chromium.push(Chromium {
                        browser,
                        label,
                        bases: vec![join(&cache, dir), join(&config, dir)],
                    });
                }
            }
            chromium.push(Chromium {
                browser: App::Chromium,
                label: "Chromium (Snap)",
                bases: vec![
                    join(home, "snap/chromium/common/.cache/chromium"),
                    join(home, "snap/chromium/common/chromium"),
                ],
            });
            for &(browser, label, id, dir) in FLATPAKS {
                let app = join(home, ".var/app").join(id);
                chromium.push(Chromium {
                    browser,
                    label,
                    bases: vec![
                        join(&app.join("cache"), dir),
                        join(&app.join("config"), dir),
                    ],
                });
            }
            firefox.push(Firefox {
                label: "Firefox",
                profiles: join(&cache, "mozilla/firefox"),
            });
            firefox.push(Firefox {
                label: "Firefox (Snap)",
                profiles: join(home, "snap/firefox/common/.cache/mozilla/firefox"),
            });
            firefox.push(Firefox {
                label: "Firefox (Flatpak)",
                profiles: join(home, ".var/app/org.mozilla.firefox/cache/mozilla/firefox"),
            });
        }
        Platform::Windows => {
            let local = ctx.local_app_data();
            for &(browser, label, _, _, dir) in VENDORS {
                if !dir.is_empty() {
                    chromium.push(Chromium {
                        browser,
                        label,
                        bases: vec![join(local, dir).join("User Data")],
                    });
                }
            }
            firefox.push(Firefox {
                label: "Firefox",
                profiles: join(local, "Mozilla/Firefox/Profiles"),
            });
        }
    }
    (chromium, firefox)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chromium_profiles_are_default_and_numbered_profiles_only() {
        for name in ["Default", "Profile 1", "Profile 12"] {
            assert!(is_chromium_profile(name), "{name}");
        }
        for name in [
            "Profile",
            "Profile ",
            "Profile x",
            "Guest Profile",
            "System Profile",
            "default",
        ] {
            assert!(!is_chromium_profile(name), "{name}");
        }
    }
}
