//! Android emulators and SDK system images, for the Android tool
//! (`android-avd-snapshots`, `android-unreferenced-images`,
//! `android-emulators-report`). Nothing here deletes or starts a process:
//! `sdkmanager`, `avdmanager`, and `emulator` are never run.
//!
//! Emulators are the `<name>.avd` folders in `~/.android/avd`. Which system
//! image an emulator boots is read from its own `config.ini`
//! (`image.sysdir.1`), one emulator at a time, and compared with an image
//! folder by file identity. Nothing is inferred from the order of a
//! listing. When any emulator's settings cannot be read, the answer to
//! "is this image in use?" is "unknown", and unknown is never "no".
//!
//! Locations, for review against the Android Studio documentation
//! ("Android Studio configuration: emulator and SDK directories"):
//!
//! | | macOS | Ubuntu | Windows |
//! | --- | --- | --- | --- |
//! | Emulators | `~/.android/avd` | `~/.android/avd` | `%USERPROFILE%\.android\avd` |
//! | SDK | `~/Library/Android/sdk` | `~/Android/Sdk` | `%LOCALAPPDATA%\Android\Sdk` |
//!
//! `ANDROID_AVD_HOME`, `ANDROID_SDK_ROOT`, and the other Android variables
//! are not read: a GUI app does not see the shell's environment, and the
//! catalog does not trust variables for roots. Emulators or an SDK kept
//! elsewhere are not found.

use std::ffi::OsStr;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::measure;
use crate::model::{Platform, SkipCause};
use crate::policy::{self, Context, ValidRoot};
use crate::roots;

/// The Quick Boot snapshot's folder name inside `<name>.avd/snapshots`.
/// Other folders there are snapshots the person saved by hand.
pub(crate) const QUICK_BOOT: &str = "default_boot";

/// A system image as its settings name it: api level, tag, and ABI
/// (`android-35`, `google_apis`, `arm64-v8a`).
pub(crate) type ImageKey = [String; 3];

/// One emulator.
#[derive(Debug, Clone)]
pub(crate) struct Avd {
    pub dir: PathBuf,
    /// `avd.ini.displayname`, else the folder name without `.avd`.
    pub name: String,
    /// The images its settings name. None when they could not be read.
    pub images: Option<Vec<ImageKey>>,
}

/// The emulators in `~/.android/avd`.
#[derive(Debug, Clone, Default)]
pub(crate) struct Emulators {
    pub avds: Vec<Avd>,
    /// Every emulator's image is known. False when a `config.ini` is
    /// missing, unreadable, or names no SDK image, or when a `<name>.ini`
    /// has no `<name>.avd` folder here (an emulator kept somewhere else).
    pub complete: bool,
}

/// One system image folder in the SDK.
#[derive(Debug, Clone)]
pub(crate) struct Image {
    pub key: ImageKey,
    /// `<sdk>/system-images/<api>/<tag>`: the folder the image sits in.
    pub parent: PathBuf,
    /// `<parent>/<abi>`.
    pub dir: PathBuf,
}

impl Image {
    /// "android-35 — google_apis — arm64-v8a".
    pub fn label(&self) -> String {
        label(&self.key)
    }
}

pub(crate) fn label(key: &ImageKey) -> String {
    key.join(" — ")
}

pub(crate) fn avd_home(ctx: &Context) -> PathBuf {
    ctx.home().join(".android").join("avd")
}

pub(crate) fn sdk_root(ctx: &Context, platform: Platform) -> PathBuf {
    match platform {
        Platform::Macos => ctx.home().join("Library").join("Android").join("sdk"),
        Platform::Linux => ctx.home().join("Android").join("Sdk"),
        Platform::Windows => ctx.local_app_data().join("Android").join("Sdk"),
    }
}

/// How much of a `config.ini` is read. Real ones are a few kilobytes.
const CONFIG_LIMIT: u64 = 256 * 1024;

/// Reads every emulator's settings. Only a folder that is plainly absent
/// (`~/.android` or `~/.android/avd` does not exist) means "no emulators".
/// Anything else that cannot be read as a real folder means "unknown",
/// because emulators kept somewhere else may use any image: a link at
/// either level (a dangling one too: the disk it points to may only be
/// unplugged), a folder that resolves outside the home or into a protected
/// folder, or one that cannot be listed.
pub(crate) fn emulators(ctx: &Context) -> Emulators {
    let home = avd_home(ctx);
    let dot_android = ctx.home().join(".android");
    for dir in [&dot_android, &home] {
        ctx.visit(dir);
        match fs::symlink_metadata(dir) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Emulators {
                    avds: Vec::new(),
                    complete: true,
                };
            }
            Ok(_) if roots::is_real_folder(dir) => {}
            _ => return Emulators::default(),
        }
    }
    if policy::validate_cache_root(ctx, &home).is_err() {
        return Emulators::default();
    }
    let Ok(entries) = fs::read_dir(&home) else {
        return Emulators::default();
    };
    let mut complete = true;
    let mut folders: Vec<String> = Vec::new();
    let mut inis: Vec<String> = Vec::new();
    for entry in entries {
        let Ok(entry) = entry else {
            complete = false;
            continue;
        };
        let Ok(name) = entry.file_name().into_string() else {
            complete = false;
            continue;
        };
        if let Some(id) = name.strip_suffix(".avd") {
            // A link or a file with an emulator's name: not read, and so
            // not known.
            match fs::symlink_metadata(entry.path()) {
                Ok(m) if m.is_dir() => folders.push(id.to_owned()),
                _ => complete = false,
            }
        } else if let Some(id) = name.strip_suffix(".ini") {
            inis.push(id.to_owned());
        } else if !fs::symlink_metadata(entry.path()).is_ok_and(|m| m.file_type().is_file()) {
            // A folder that is not an emulator, or a link. Gradle keeps
            // its managed devices in `gradle-managed` here; whatever it
            // is, it may hold emulators that are not read, so it counts
            // as unknown.
            complete = false;
        }
    }
    folders.sort();
    if inis.iter().any(|id| !folders.contains(id)) {
        complete = false;
    }
    let mut avds = Vec::with_capacity(folders.len());
    for id in folders {
        let dir = home.join(format!("{id}.avd"));
        let config = read_config(ctx, &dir.join("config.ini")).map(|text| parse_config(&text));
        let images = config.as_ref().and_then(|c| {
            let keys: Option<Vec<ImageKey>> = c.sysdirs.iter().map(|s| image_key(s)).collect();
            keys.filter(|k| !k.is_empty())
        });
        if images.is_none() {
            complete = false;
        }
        let name = config
            .and_then(|c| c.display_name)
            .filter(|n| !n.is_empty())
            .unwrap_or(id);
        avds.push(Avd { dir, name, images });
    }
    Emulators { avds, complete }
}

fn read_config(ctx: &Context, path: &Path) -> Option<String> {
    ctx.visit(path);
    if !fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_file()) {
        return None;
    }
    let mut text = String::new();
    fs::File::open(path)
        .ok()?
        .take(CONFIG_LIMIT)
        .read_to_string(&mut text)
        .ok()?;
    Some(text)
}

/// The two settings this module reads from a `config.ini`.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Config {
    pub display_name: Option<String>,
    /// `image.sysdir.<n>` values, in file order.
    pub sysdirs: Vec<String>,
}

/// `key=value` lines; spaces around either side are dropped, and `#` starts
/// a comment line.
pub(crate) fn parse_config(text: &str) -> Config {
    let mut config = Config::default();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        if key == "avd.ini.displayname" {
            config.display_name = Some(value.to_owned());
        } else if key
            .strip_prefix("image.sysdir.")
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        {
            config.sysdirs.push(value.to_owned());
        }
    }
    config
}

/// `system-images/<api>/<tag>/<abi>/`, with `/` or `\`, as the emulator
/// writes it relative to the SDK. Anything else (an absolute path, `..`, a
/// shorter path) is not a key, and its emulator's image is unknown.
pub(crate) fn image_key(sysdir: &str) -> Option<ImageKey> {
    let parts: Vec<&str> = sysdir
        .split(['/', '\\'])
        .filter(|p| !p.is_empty())
        .collect();
    let [first, api, tag, abi] = parts.as_slice() else {
        return None;
    };
    if *first != "system-images" || sysdir.starts_with(['/', '\\']) {
        return None;
    }
    if [api, tag, abi]
        .iter()
        .any(|p| **p == "." || **p == ".." || p.contains(':'))
    {
        return None;
    }
    Some([(*api).to_owned(), (*tag).to_owned(), (*abi).to_owned()])
}

/// The SDK's system images: `<sdk>/system-images/<api>/<tag>/<abi>` where
/// every level is a real folder and `<abi>` holds a `system.img` file. An
/// SDK reached through a link at any level, or one that resolves outside
/// the home or into a protected folder, is not read at all.
pub(crate) fn images(ctx: &Context, platform: Platform) -> Vec<Image> {
    let base = sdk_root(ctx, platform).join("system-images");
    let anchor = match platform {
        Platform::Windows => ctx.local_app_data(),
        Platform::Macos | Platform::Linux => ctx.home(),
    };
    let mut found = Vec::new();
    ctx.visit(&base);
    if !roots::real_folders_below(anchor, &base) || policy::validate_cache_root(ctx, &base).is_err()
    {
        return found;
    }
    for api in roots::folders(ctx, &base) {
        let api_dir = base.join(&api);
        for tag in roots::folders(ctx, &api_dir) {
            let parent = api_dir.join(&tag);
            for abi in roots::folders(ctx, &parent) {
                let dir = parent.join(&abi);
                let system = dir.join("system.img");
                ctx.visit(&system);
                if fs::symlink_metadata(&system).is_ok_and(|m| m.file_type().is_file()) {
                    found.push(Image {
                        key: [api.clone(), tag.clone(), abi],
                        parent: parent.clone(),
                        dir,
                    });
                }
            }
        }
    }
    found
}

impl Emulators {
    /// The emulators whose settings name `image`, by name or by the
    /// identity of the folder the name resolves to (so a differently-cased
    /// name on a case-insensitive disk still matches).
    pub fn users<'a>(&'a self, ctx: &Context, platform: Platform, image: &Image) -> Vec<&'a Avd> {
        let base = sdk_root(ctx, platform).join("system-images");
        let image_id = measure::file_id(&image.dir).ok();
        self.avds
            .iter()
            .filter(|avd| {
                avd.images.iter().flatten().any(|key| {
                    if *key == image.key {
                        return true;
                    }
                    let named = key.iter().fold(base.clone(), |p, part| p.join(part));
                    image_id.is_some() && measure::file_id(&named).ok() == image_id
                })
            })
            .collect()
    }
}

/// Checks a planned system image again, immediately before it is removed:
/// its folder is still the validated one the scan saw, it is still an
/// image, every emulator's settings can still be read, and none of them
/// names it.
pub(crate) fn recheck_image(
    ctx: &Context,
    planned: &ValidRoot,
    name: &OsStr,
) -> Result<(), SkipCause> {
    let platform = Platform::CURRENT;
    let root = policy::validate_cache_root(ctx, planned.path()).map_err(SkipCause::Entry)?;
    if root.id() != planned.id() || root.path() != planned.path() {
        return Err(SkipCause::ChangedSinceScan);
    }
    let image = images(ctx, platform)
        .into_iter()
        .find(|image| {
            OsStr::new(&image.key[2]) == name
                && measure::file_id(&image.parent).is_ok_and(|id| id == planned.id())
        })
        .ok_or_else(|| {
            if fs::symlink_metadata(planned.path().join(name)).is_err() {
                SkipCause::Gone
            } else {
                SkipCause::ChangedSinceScan
            }
        })?;
    let emulators = emulators(ctx);
    if !emulators.complete {
        return Err(SkipCause::EmulatorsUnknown);
    }
    if !emulators.users(ctx, platform, &image).is_empty() {
        return Err(SkipCause::ImageInUse);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(api: &str, tag: &str, abi: &str) -> Option<ImageKey> {
        Some([api.to_owned(), tag.to_owned(), abi.to_owned()])
    }

    #[test]
    fn a_sysdir_is_read_with_either_separator() {
        assert_eq!(
            image_key("system-images/android-36.1/google_apis_playstore/arm64-v8a/"),
            key("android-36.1", "google_apis_playstore", "arm64-v8a")
        );
        assert_eq!(
            image_key("system-images\\android-34\\google_apis\\x86_64\\"),
            key("android-34", "google_apis", "x86_64")
        );
        assert_eq!(
            image_key("system-images/android-30/default/x86"),
            key("android-30", "default", "x86")
        );
    }

    #[test]
    fn anything_that_is_not_an_sdk_relative_image_path_is_unknown() {
        for sysdir in [
            "",
            "system-images",
            "system-images/android-30/default",
            "system-images/android-30/default/x86/extra",
            "/system-images/android-30/default/x86/",
            "\\system-images\\android-30\\default\\x86",
            "images/android-30/default/x86",
            "system-images/../default/x86",
            "system-images/android-30/./x86",
            "system-images/C:/default/x86",
        ] {
            assert_eq!(image_key(sysdir), None, "{sysdir}");
        }
    }

    #[test]
    fn the_config_parser_reads_the_name_and_every_sysdir() {
        let config = parse_config(
            "# comment\nAvdId=Medium_Phone\n avd.ini.displayname = Medium Phone \n\
             image.sysdir.1=system-images/android-36.1/google_apis_playstore/arm64-v8a/\n\
             image.sysdir.2 = system-images/android-36/default/arm64-v8a/\n\
             image.sysdir.x=ignored\nimage.sysdir.=ignored\nnot a setting\n",
        );
        assert_eq!(config.display_name.as_deref(), Some("Medium Phone"));
        assert_eq!(
            config.sysdirs,
            [
                "system-images/android-36.1/google_apis_playstore/arm64-v8a/",
                "system-images/android-36/default/arm64-v8a/",
            ]
        );
    }

    #[test]
    fn a_config_without_an_image_setting_has_none() {
        assert_eq!(parse_config("hw.ramSize=2048\n"), Config::default());
    }
}
