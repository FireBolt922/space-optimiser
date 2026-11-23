//! Dependency and build folders inside projects, for the Projects tool
//! (`project-build-folders`). Nothing here deletes. The rules, all from the
//! catalog row:
//!
//! - A folder is this row only by exact name **and** marker ([`matches`]):
//!   `node_modules` or `.next` beside a `package.json`, `target` beside a
//!   `Cargo.toml` and holding Cargo's own `CACHEDIR.TAG`, `.venv` or `venv`
//!   holding a `pyvenv.cfg`.
//! - It is offered only inside source control ([`repo_root`]). An installed
//!   app can ship `node_modules` beside a `package.json` too; a `.git`,
//!   `.hg`, or `.svn` above it, below the home, is what marks a project.
//! - The walk never enters a folder [`is_excluded`] names.
//! - Activity is judged for the whole source-control root
//!   ([`last_activity`]), never for a subfolder on its own.
//!
//! No tool is run inside a project. `cargo clean` was considered and
//! rejected: run from the app's empty work folder it would ignore the
//! project's `.cargo/config.toml`, and a rustup proxy can download a
//! toolchain. The scanner and the executor both call [`eligible`], so a
//! rule cannot hold at scan time and be forgotten at delete time.

use std::ffi::OsStr;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::discover::{CancelToken, ScanError};
use crate::measure;
use crate::model::SkipCause;
use crate::policy::{self, Context, ValidRoot};

/// A project with no change for this long is old (catalog row).
pub const OLD_AGE: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// The walk does not go deeper than this below the home.
pub const MAX_DEPTH: usize = 16;

/// Which dependency or build folder a candidate is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    NodeModules,
    Next,
    CargoTarget,
    Venv,
}

/// The kind a folder called `name` could be, before its marker is checked.
pub(crate) fn kind_of(name: &OsStr) -> Option<Kind> {
    match name.to_str()? {
        "node_modules" => Some(Kind::NodeModules),
        ".next" => Some(Kind::Next),
        "target" => Some(Kind::CargoTarget),
        ".venv" | "venv" => Some(Kind::Venv),
        _ => None,
    }
}

/// Whether `dir/name` is one of the row's folders right now: the name and
/// its marker. `dir` is the project folder. Reads a few fixed file names
/// and follows nothing.
pub(crate) fn matches(dir: &Path, name: &OsStr) -> Option<Kind> {
    let kind = kind_of(name)?;
    let found = match kind {
        Kind::NodeModules | Kind::Next => is_file(&dir.join("package.json")),
        Kind::CargoTarget => {
            is_file(&dir.join("Cargo.toml")) && is_cache_tag(&dir.join(name).join("CACHEDIR.TAG"))
        }
        Kind::Venv => is_file(&dir.join(name).join("pyvenv.cfg")),
    };
    found.then_some(kind)
}

fn is_file(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_file())
}

/// The first line of every cache directory tag (bford.info/cachedir).
/// Cargo writes one into each `target` folder it creates.
const CACHE_TAG_SIGNATURE: &[u8] = b"Signature: 8a477f597d28d172789f06886806bc55";

fn is_cache_tag(path: &Path) -> bool {
    if !is_file(path) {
        return false;
    }
    let mut head = [0u8; CACHE_TAG_SIGNATURE.len()];
    fs::File::open(path)
        .and_then(|mut file| file.read_exact(&mut head))
        .is_ok()
        && head == CACHE_TAG_SIGNATURE
}

/// Files that say how a Python environment is recreated.
const PYTHON_RECIPES: &[&str] = &[
    "pyproject.toml",
    "Pipfile",
    "setup.py",
    "setup.cfg",
    "uv.lock",
    "poetry.lock",
];

/// Whether a requirements file sits in `dir`, next to a virtual
/// environment. Without one the environment may be the only record of what
/// was installed, so it is not offered.
pub(crate) fn has_python_recipe(dir: &Path) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            return false;
        };
        let listed = PYTHON_RECIPES.contains(&name)
            || (name.starts_with("requirements") && name.ends_with(".txt"));
        listed && is_file(&entry.path())
    })
}

/// Folder names the walk never enters, at any depth: hidden folders, app
/// and system folders, bundles, and folders that hold copies of
/// dependencies rather than a project's own.
pub(crate) fn is_excluded(name: &OsStr) -> bool {
    let Some(name) = name.to_str() else {
        // A name that is not text cannot be shown or matched; leave it.
        return true;
    };
    name.starts_with('.')
        || name.ends_with(".app")
        || matches!(
            name,
            "Library" | "Applications" | "AppData" | "snap" | "node_modules" | "cdk.out"
        )
}

const VCS: [&str; 3] = [".git", ".hg", ".svn"];

/// The nearest folder from `dir` upwards, below `home`, that directly holds
/// `.git`, `.hg`, or `.svn`. The home itself never counts: a dotfiles
/// repository there would make everything below it a project.
pub(crate) fn repo_root(home: &Path, dir: &Path) -> Option<PathBuf> {
    for folder in dir.ancestors() {
        if folder == home || !folder.starts_with(home) {
            return None;
        }
        if VCS
            .iter()
            .any(|v| fs::symlink_metadata(folder.join(v)).is_ok())
        {
            return Some(folder.to_path_buf());
        }
    }
    None
}

/// Why a matched folder is shown but not offered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NotOffered {
    /// Not inside source control.
    NoRepository,
    /// A virtual environment with no requirements file beside it.
    NoRecipe,
}

impl NotOffered {
    pub const fn text(self) -> &'static str {
        match self {
            NotOffered::NoRepository => {
                "Not inside a source-control folder (.git), so Space Optimiser cannot tell that it belongs to a project of yours. Not offered."
            }
            NotOffered::NoRecipe => {
                "No requirements file is next to it, so there may be no record of what to install again. Not offered."
            }
        }
    }
}

/// Whether the matched folder `dir/name` may be offered, and the
/// source-control root its activity is judged at.
pub(crate) fn eligible(home: &Path, dir: &Path, kind: Kind) -> Result<PathBuf, NotOffered> {
    let repo = repo_root(home, dir).ok_or(NotOffered::NoRepository)?;
    if kind == Kind::Venv && !has_python_recipe(dir) {
        return Err(NotOffered::NoRecipe);
    }
    Ok(repo)
}

/// Checks a planned project folder again, immediately before it is
/// removed: the project folder is still the validated one the scan saw,
/// below the home with no excluded folder on the way, and `name` still
/// matches its marker and is still offered.
pub(crate) fn recheck(ctx: &Context, planned: &ValidRoot, name: &OsStr) -> Result<(), SkipCause> {
    let root = policy::validate_cache_root(ctx, planned.path()).map_err(SkipCause::Entry)?;
    if root.id() != planned.id() || root.path() != planned.path() {
        return Err(SkipCause::ChangedSinceScan);
    }
    let changed = SkipCause::ChangedSinceScan;
    let below_home = root.path().strip_prefix(ctx.home()).map_err(|_| changed)?;
    if below_home.components().count() >= MAX_DEPTH
        || below_home.components().any(|c| is_excluded(c.as_os_str()))
    {
        return Err(changed);
    }
    let kind = matches(root.path(), name).ok_or(changed)?;
    eligible(ctx.home(), root.path(), kind).map_err(|_| changed)?;
    Ok(())
}

/// Finder and Explorer rewrite these when a folder is only looked at.
const VIEW_FILES: [&str; 3] = [".DS_Store", "Thumbs.db", "desktop.ini"];

/// Files git rewrites when `HEAD` moves: a commit, checkout, merge, pull
/// that brought something in, rebase, or reset. `index`, `FETCH_HEAD`, and
/// `ORIG_HEAD` are left out on purpose: editors and background tools
/// refresh them (`git status`, automatic fetch) in a project nobody is
/// working on, which made every project on the test Mac look active.
const GIT_SIGNALS: [&str; 2] = ["HEAD", "logs/HEAD"];

/// How deep below a project's root the activity walk goes.
const ACTIVITY_DEPTH: usize = 32;

/// When anything in the project at `root` last changed: the newest
/// modified time of a file below it, not counting the folders
/// [`is_excluded`] names (hidden folders, `node_modules`, ...), and of the
/// files git rewrites when `HEAD` moves. Build output such as `target` or
/// `dist` counts on purpose: a project built last week is in use. Links
/// are not followed, and a protected folder or another disk below the
/// project is not read, not even for its dates. `None` when no file was
/// found.
pub(crate) fn last_activity(
    ctx: &Context,
    root: &Path,
    cancel: &CancelToken,
) -> Result<Option<SystemTime>, ScanError> {
    let mut newest: Option<SystemTime> = None;
    let mut see = |time: std::io::Result<SystemTime>| {
        if let Ok(time) = time {
            newest = newest.max(Some(time));
        }
    };
    let git = root.join(".git");
    for signal in GIT_SIGNALS {
        let path = signal.split('/').fold(git.clone(), |p, part| p.join(part));
        let file = fs::symlink_metadata(&path)
            .ok()
            .filter(|m| m.file_type().is_file());
        if let Some(metadata) = file {
            see(metadata.modified());
        }
    }
    let device = measure::file_id(root).ok().map(|id| id.device());
    let protected: Vec<&Path> = ctx.protected_paths().collect();
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        ctx.visit(&dir);
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries {
            if cancel.is_cancelled() {
                return Err(ScanError::Cancelled);
            }
            let Ok(entry) = entry else { continue };
            let name = entry.file_name();
            // `DirEntry::metadata` describes a link, not its target.
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            let kind = metadata.file_type();
            if kind.is_dir() {
                if is_excluded(&name) || depth + 1 >= ACTIVITY_DEPTH {
                    continue;
                }
                let path = entry.path();
                if protected.contains(&path.as_path()) {
                    continue;
                }
                let Ok(id) = measure::file_id(&path) else {
                    continue;
                };
                if ctx.is_protected_id(id) || device.is_some_and(|d| d != id.device()) {
                    continue;
                }
                stack.push((path, depth + 1));
            } else if kind.is_file() && !VIEW_FILES.iter().any(|v| name == *v) {
                see(metadata.modified());
            }
        }
    }
    Ok(newest)
}

/// "Old project: nothing in it changed for 94 days." or "Active project:
/// last changed 2 days ago." `age_days` is whole days since the last
/// change; a change dated in the future is 0.
pub(crate) fn activity_sentence(age_days: u32, old: bool) -> String {
    if old {
        return format!("Old project: nothing in it changed for {age_days} days.");
    }
    match age_days {
        0 => "Active project: last changed today.".to_owned(),
        1 => "Active project: last changed 1 day ago.".to_owned(),
        n => format!("Active project: last changed {n} days ago."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_four_folder_names_are_kinds() {
        for (name, kind) in [
            ("node_modules", Kind::NodeModules),
            (".next", Kind::Next),
            ("target", Kind::CargoTarget),
            (".venv", Kind::Venv),
            ("venv", Kind::Venv),
        ] {
            assert_eq!(kind_of(OsStr::new(name)), Some(kind), "{name}");
        }
        for name in [
            "build",
            "dist",
            "out",
            "vendor",
            "Pods",
            ".gradle",
            "Node_Modules",
            "node_modules ",
            "targets",
            "env",
        ] {
            assert_eq!(kind_of(OsStr::new(name)), None, "{name}");
        }
    }

    #[test]
    fn hidden_system_and_bundle_folders_are_excluded() {
        for name in [
            ".git",
            ".cache",
            ".vscode",
            "Library",
            "Applications",
            "AppData",
            "snap",
            "node_modules",
            "cdk.out",
            "Tool.app",
        ] {
            assert!(is_excluded(OsStr::new(name)), "{name}");
        }
        for name in ["Projects", "src", "apps", "library", "application", "app"] {
            assert!(!is_excluded(OsStr::new(name)), "{name}");
        }
    }

    #[test]
    fn the_sentence_counts_days_in_words() {
        assert_eq!(
            activity_sentence(94, true),
            "Old project: nothing in it changed for 94 days."
        );
        assert_eq!(
            activity_sentence(0, false),
            "Active project: last changed today."
        );
        assert_eq!(
            activity_sentence(1, false),
            "Active project: last changed 1 day ago."
        );
        assert_eq!(
            activity_sentence(12, false),
            "Active project: last changed 12 days ago."
        );
    }
}
