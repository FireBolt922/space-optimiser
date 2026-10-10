//! Catalog ids, entry rules, protected folders, and temp-root validation
//! (phase 4). The catalog is the allowlist: a row that is not here cannot be
//! reported or acted on.

use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime};

use serde::Serialize;

use crate::measure::{self, Facts, FileId, Kind};
use crate::model::{Action, Risk};

/// First-release catalog rows. `fixtures/catalog-mvp-ids.txt` lists the same
/// ids; a test fails if they drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CatalogId {
    UserTemp,
    ThumbnailCache,
    WindowsThumbcache,
    FontCache,
    BrowserHttpCache,
    PkgNpmCache,
    PkgNpx,
    PkgPnpmStore,
    PkgYarnCache,
    PkgPipCache,
    PkgUvCache,
    PkgCargoCache,
    PkgGradleCaches,
    PkgNugetHttp,
    UserLogsOld,
    DockerBuildCache,
    LargeFilesReport,
}

impl CatalogId {
    pub const ALL: [CatalogId; 17] = [
        CatalogId::UserTemp,
        CatalogId::ThumbnailCache,
        CatalogId::WindowsThumbcache,
        CatalogId::FontCache,
        CatalogId::BrowserHttpCache,
        CatalogId::PkgNpmCache,
        CatalogId::PkgNpx,
        CatalogId::PkgPnpmStore,
        CatalogId::PkgYarnCache,
        CatalogId::PkgPipCache,
        CatalogId::PkgUvCache,
        CatalogId::PkgCargoCache,
        CatalogId::PkgGradleCaches,
        CatalogId::PkgNugetHttp,
        CatalogId::UserLogsOld,
        CatalogId::DockerBuildCache,
        CatalogId::LargeFilesReport,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            CatalogId::UserTemp => "user-temp",
            CatalogId::ThumbnailCache => "thumbnail-cache",
            CatalogId::WindowsThumbcache => "windows-thumbcache",
            CatalogId::FontCache => "font-cache",
            CatalogId::BrowserHttpCache => "browser-http-cache",
            CatalogId::PkgNpmCache => "pkg-npm-cache",
            CatalogId::PkgNpx => "pkg-npx",
            CatalogId::PkgPnpmStore => "pkg-pnpm-store",
            CatalogId::PkgYarnCache => "pkg-yarn-cache",
            CatalogId::PkgPipCache => "pkg-pip-cache",
            CatalogId::PkgUvCache => "pkg-uv-cache",
            CatalogId::PkgCargoCache => "pkg-cargo-cache",
            CatalogId::PkgGradleCaches => "pkg-gradle-caches",
            CatalogId::PkgNugetHttp => "pkg-nuget-http",
            CatalogId::UserLogsOld => "user-logs-old",
            CatalogId::DockerBuildCache => "docker-build-cache",
            CatalogId::LargeFilesReport => "large-files-report",
        }
    }

    pub const fn risk(self) -> Risk {
        match self {
            CatalogId::UserTemp
            | CatalogId::ThumbnailCache
            | CatalogId::FontCache
            | CatalogId::UserLogsOld => Risk::Low,
            CatalogId::WindowsThumbcache | CatalogId::LargeFilesReport => Risk::Info,
            _ => Risk::Medium,
        }
    }

    /// Whether "Select safe items" may include the row. Only low-risk rows.
    pub const fn bulk_safe(self) -> bool {
        matches!(self.risk(), Risk::Low)
    }

    pub const fn action(self) -> Action {
        match self {
            CatalogId::WindowsThumbcache | CatalogId::LargeFilesReport => Action::Report,
            CatalogId::PkgNpmCache
            | CatalogId::PkgPnpmStore
            | CatalogId::PkgYarnCache
            | CatalogId::PkgPipCache
            | CatalogId::PkgUvCache
            | CatalogId::DockerBuildCache => Action::RunTool,
            _ => Action::Delete,
        }
    }

    /// Static text keyed by id, so the UI never invents a reason from a file
    /// name (phase 4 design decisions).
    pub const fn explanation(self) -> &'static str {
        match self {
            CatalogId::UserTemp => {
                "Temporary files more than a day old. Apps recreate what they still need."
            }
            CatalogId::ThumbnailCache => {
                "Thumbnail images. They are rebuilt the next time you view a folder."
            }
            CatalogId::WindowsThumbcache => {
                "Explorer's thumbnail database. Explorer keeps it open, so it is shown for information only."
            }
            CatalogId::FontCache => "Font cache files. The next app start rebuilds them.",
            CatalogId::BrowserHttpCache => {
                "Copies of web pages and images. Pages load from the network again. Quit the browser first."
            }
            CatalogId::PkgNpmCache => {
                "Downloaded npm packages. They download again when a project needs them."
            }
            CatalogId::PkgNpx => {
                "Packages fetched by npx. They download again on the next npx run."
            }
            CatalogId::PkgPnpmStore => {
                "pnpm packages that no project references. Savings can be much smaller than the store size."
            }
            CatalogId::PkgYarnCache => {
                "Downloaded Yarn packages. They download again on the next install."
            }
            CatalogId::PkgPipCache => {
                "Downloaded Python wheels. They download again on the next install."
            }
            CatalogId::PkgUvCache => {
                "uv's package cache. The size is an estimate because files can be shared with environments."
            }
            CatalogId::PkgCargoCache => {
                "Downloaded Rust crates and git dependencies. They download again on the next build."
            }
            CatalogId::PkgGradleCaches => {
                "Gradle's dependency cache. The next build downloads dependencies again."
            }
            CatalogId::PkgNugetHttp => {
                "NuGet's download cache. Installed packages are not touched."
            }
            CatalogId::UserLogsOld => "Log files more than 30 days old.",
            CatalogId::DockerBuildCache => {
                "Docker build cache that no image uses. The next image build is slower."
            }
            CatalogId::LargeFilesReport => {
                "Large files in your home folder. Shown so you can decide; never removed."
            }
        }
    }

    /// The review-screen sentence for a planned item (phase 2 item 11).
    pub const fn action_text(self) -> &'static str {
        match self.action() {
            Action::Delete => match self {
                CatalogId::UserTemp => {
                    "Delete this item from your temporary folder. Everything in it is more than a day old."
                }
                CatalogId::UserLogsOld => "Delete log files more than 30 days old.",
                _ => "Delete these cache files.",
            },
            Action::RunTool => "Run the tool's own clean command.",
            Action::Report | Action::None => "Nothing. Shown for information only.",
        }
    }

    /// What to expect after a successful cleanup (phase 8 item 10).
    pub const fn recovery_text(self) -> Option<&'static str> {
        match self {
            CatalogId::UserTemp => {
                Some("Files older than a day were removed from the temporary folder.")
            }
            CatalogId::BrowserHttpCache => {
                Some("Those sites will load from the network next time.")
            }
            CatalogId::PkgNpmCache
            | CatalogId::PkgNpx
            | CatalogId::PkgPnpmStore
            | CatalogId::PkgYarnCache
            | CatalogId::PkgPipCache
            | CatalogId::PkgUvCache
            | CatalogId::PkgCargoCache
            | CatalogId::PkgGradleCaches
            | CatalogId::PkgNugetHttp => Some("The next install will download packages again."),
            CatalogId::DockerBuildCache => Some(
                "The next build can be slower. Docker may need a restart before the Mac or Windows disk file shrinks.",
            ),
            CatalogId::UserLogsOld => Some("Log files older than 30 days were removed."),
            CatalogId::ThumbnailCache
            | CatalogId::WindowsThumbcache
            | CatalogId::FontCache
            | CatalogId::LargeFilesReport => None,
        }
    }

    /// The age gate in the catalog entry rules. Cache rows have none: the
    /// whole folder is regenerable.
    pub const fn min_age(self) -> Option<Duration> {
        match self {
            CatalogId::UserTemp => Some(TEMP_MIN_AGE),
            CatalogId::UserLogsOld => Some(LOG_MIN_AGE),
            _ => None,
        }
    }

    pub const fn location_label(self) -> &'static str {
        match self {
            CatalogId::UserTemp => "Temporary files",
            CatalogId::ThumbnailCache | CatalogId::WindowsThumbcache => "Thumbnails",
            CatalogId::FontCache => "Font cache",
            CatalogId::BrowserHttpCache => "Browser cache",
            CatalogId::UserLogsOld => "Logs",
            CatalogId::DockerBuildCache => "Docker",
            CatalogId::LargeFilesReport => "Home folder",
            _ => "Developer caches",
        }
    }
}

/// `user-temp` entries must be older than this (catalog `user-temp`).
pub const TEMP_MIN_AGE: Duration = Duration::from_secs(24 * 60 * 60);

/// `user-logs-old` files must be older than this (catalog `user-logs-old`).
pub const LOG_MIN_AGE: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// Age is the newest of the two timestamps (`mtime`/`ctime`, or last-write/
/// creation). A timestamp in the future counts as fresh.
pub fn is_old(a: SystemTime, b: SystemTime, now: SystemTime, min_age: Duration) -> bool {
    match now.duration_since(a.max(b)) {
        Ok(age) => age >= min_age,
        Err(_) => false,
    }
}

/// Why an entry was left alone. A top-level entry is judged whole: one
/// failing descendant skips all of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, thiserror::Error)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    /// A symlink, junction, or other reparse point. Never followed.
    #[error("it is a link, and links are never followed")]
    Link,
    /// The entry or something inside it changed within the age limit.
    #[error("it changed within the last day")]
    Fresh,
    /// Contains a socket or FIFO (an agent, editor, or X11 connection).
    #[error("it holds a connection a running app may be using")]
    SocketOrPipe,
    /// A device file.
    #[error("it is a device file")]
    SpecialFile,
    /// Contains a project marker such as `.git`: looks like a project.
    #[error("it looks like a project folder")]
    Project,
    /// Something inside could not be read.
    #[error("part of it could not be read")]
    Unreadable,
    /// Something inside belongs to another user.
    #[error("part of it belongs to another user")]
    OtherOwner,
    /// Read-only (Windows) or immutable (macOS). Attributes are never cleared.
    #[error("it is locked")]
    Locked,
    /// Is or contains a protected folder.
    #[error("it is or contains a protected folder")]
    ProtectedFolder,
    /// Crosses into another filesystem.
    #[error("part of it is on another disk")]
    OtherFilesystem,
    /// A cache folder whose real location is not inside the home folder.
    #[error("it is outside your home folder")]
    OutsideHome,
}

/// The catalog entry rules for `row`, applied to an entry and to every
/// descendant. The scanner and the executor both call this, so a rule
/// cannot hold at scan time and be forgotten at delete time (phase 8 item 3).
/// Rows differ only in the age gate ([`CatalogId::min_age`]).
pub(crate) fn entry_rule(
    row: CatalogId,
    ctx: &Context,
    facts: &Facts,
    root_device: Option<u64>,
) -> Option<SkipReason> {
    match facts.kind {
        Kind::Link => return Some(SkipReason::Link),
        Kind::SocketOrPipe => return Some(SkipReason::SocketOrPipe),
        Kind::Device => return Some(SkipReason::SpecialFile),
        Kind::File | Kind::Dir => {}
    }
    if root_device.is_some_and(|d| d != facts.id.device()) {
        return Some(SkipReason::OtherFilesystem);
    }
    #[cfg(unix)]
    if facts.uid != ctx.euid() {
        return Some(SkipReason::OtherOwner);
    }
    if facts.locked {
        return Some(SkipReason::Locked);
    }
    if let Some(min_age) = row.min_age() {
        if !is_old(facts.times.0, facts.times.1, ctx.now(), min_age) {
            return Some(SkipReason::Fresh);
        }
    }
    if facts.kind == Kind::Dir && ctx.is_protected_id(facts.id) {
        return Some(SkipReason::ProtectedFolder);
    }
    None
}

const PROJECT_MARKERS: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    "package.json",
    "Cargo.toml",
    "pyproject.toml",
    "go.mod",
    "pom.xml",
    "build.gradle",
    "build.gradle.kts",
    "Gemfile",
    "composer.json",
    "Makefile",
];
const PROJECT_MARKER_EXTENSIONS: &[&str] = &["sln", "csproj"];

/// A directory that directly contains one of these is a project (catalog
/// denylist) and is never acted on.
pub fn is_project_marker(name: &OsStr) -> bool {
    if PROJECT_MARKERS.iter().any(|m| name == OsStr::new(m)) {
        return true;
    }
    let path = Path::new(name);
    path.file_stem().is_some_and(|stem| !stem.is_empty())
        && path.extension().is_some_and(|ext| {
            PROJECT_MARKER_EXTENSIONS
                .iter()
                .any(|m| ext == OsStr::new(m))
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PathRejection {
    #[error("the path does not exist")]
    Missing,
    #[error("the path resolves outside its allowed root")]
    Escapes,
}

/// Resolves `..` and links in the parent of `path` (not in its last
/// component, which is never followed) and checks that the result is strictly
/// inside `root`.
pub fn contained_in(root: &Path, path: &Path) -> Result<PathBuf, PathRejection> {
    let root = fs::canonicalize(root).map_err(|_| PathRejection::Missing)?;
    let resolved = match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => fs::canonicalize(parent)
            .map_err(|_| PathRejection::Missing)?
            .join(name),
        // Ends in `..` or is a root: resolve it whole.
        _ => fs::canonicalize(path).map_err(|_| PathRejection::Missing)?,
    };
    if resolved != root && resolved.starts_with(&root) {
        Ok(resolved)
    } else {
        Err(PathRejection::Escapes)
    }
}

/// Who must own the entries under a temp root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerRule {
    /// The root itself must belong to the user (macOS, Windows, `~/.cache/tmp`).
    Root,
    /// A shared root such as Ubuntu `/tmp`: each entry's owner is checked.
    PerEntry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, thiserror::Error)]
#[serde(rename_all = "snake_case")]
pub enum RootRejection {
    #[error("the temporary folder does not exist")]
    Missing,
    #[error("the temporary folder could not be read")]
    Unreadable,
    #[error("the temporary folder is not a folder")]
    NotADirectory,
    #[error("the temporary folder is the top of a disk")]
    FilesystemRoot,
    #[error("the temporary folder is outside the system's per-user temporary area")]
    OutsideSystemTempArea,
    #[error("the temporary folder is your home folder or contains it")]
    HomeOrAncestor,
    #[error("the temporary folder is inside a protected folder")]
    InsideProtectedFolder,
    #[error("the temporary folder contains a protected folder")]
    ContainsProtectedFolder,
    #[error("the temporary folder belongs to another user")]
    NotOwnedByUser,
}

/// A root that passed validation. Only [`validate_temp_root`] and
/// [`validate_cache_root`] make one, so the walker cannot be pointed at an
/// arbitrary path.
#[derive(Debug, Clone)]
pub struct ValidRoot {
    path: PathBuf,
    id: FileId,
    owner: OwnerRule,
}

impl ValidRoot {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn id(&self) -> FileId {
        self.id
    }

    pub fn owner_rule(&self) -> OwnerRule {
        self.owner
    }
}

#[derive(Debug, Clone)]
struct Protected {
    path: PathBuf,
    id: Option<FileId>,
}

/// Every path a scan reads or lists, for tests that prove a folder was not
/// walked (phase 5 test 5).
pub type VisitLog = Arc<Mutex<Vec<PathBuf>>>;

/// Where the names of running processes come from.
#[derive(Debug, Clone)]
enum Processes {
    /// This computer's process list.
    System,
    /// A fixed list, for fixtures.
    Fixed(Arc<[String]>),
    /// The list cannot be read.
    Unavailable,
}

/// What a scan may and may not touch: the home, protected folders, temp
/// roots, the clock, and the user. Built by [`crate::platform::resolve_context`].
#[derive(Debug, Clone)]
pub struct Context {
    home: PathBuf,
    /// Windows Local AppData. Defaults to `home\AppData\Local`.
    local_app_data: PathBuf,
    protected: Vec<Protected>,
    temp_roots: Vec<(PathBuf, OwnerRule)>,
    required_temp_area: Option<PathBuf>,
    now: SystemTime,
    processes: Processes,
    visits: Option<VisitLog>,
    #[cfg(unix)]
    euid: u32,
}

impl Context {
    pub(crate) fn new(
        home: PathBuf,
        protected: impl IntoIterator<Item = PathBuf>,
        temp_roots: Vec<(PathBuf, OwnerRule)>,
        required_temp_area: Option<PathBuf>,
    ) -> Self {
        let home = fs::canonicalize(&home).unwrap_or(home);
        let mut ctx = Self {
            local_app_data: home.join("AppData").join("Local"),
            home,
            protected: Vec::new(),
            temp_roots,
            required_temp_area,
            now: SystemTime::now(),
            processes: Processes::System,
            visits: None,
            #[cfg(unix)]
            // SAFETY: geteuid has no preconditions and cannot fail.
            euid: unsafe { libc::geteuid() },
        };
        for path in protected {
            ctx.add_protected(path);
        }
        ctx
    }

    /// A context for a fixture home: the standard protected folders under
    /// `home` and its `user-dirs.dirs`, no temp roots, the real clock, and
    /// no running browsers.
    #[doc(hidden)]
    pub fn for_fixture(home: &Path) -> Self {
        let protected = crate::platform::protected_under_home(home);
        Self::new(home.to_path_buf(), protected, Vec::new(), None)
            .with_running_processes::<&str>(&[])
    }

    // Set by the Windows adapter from the Local AppData Known Folder.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) fn with_local_app_data(mut self, path: PathBuf) -> Self {
        self.local_app_data = fs::canonicalize(&path).unwrap_or(path);
        self
    }

    /// Pretends exactly these processes are running.
    #[doc(hidden)]
    pub fn with_running_processes<S: AsRef<str>>(mut self, names: &[S]) -> Self {
        self.processes = Processes::Fixed(names.iter().map(|n| n.as_ref().to_owned()).collect());
        self
    }

    /// Pretends the process list cannot be read.
    #[doc(hidden)]
    pub fn with_process_list_error(mut self) -> Self {
        self.processes = Processes::Unavailable;
        self
    }

    /// Reads this computer's process list, as production does. For the
    /// debug fixture window.
    #[doc(hidden)]
    pub fn with_system_processes(mut self) -> Self {
        self.processes = Processes::System;
        self
    }

    #[doc(hidden)]
    pub fn with_visit_log(mut self, log: VisitLog) -> Self {
        self.visits = Some(log);
        self
    }

    #[doc(hidden)]
    pub fn with_now(mut self, now: SystemTime) -> Self {
        self.now = now;
        self
    }

    #[doc(hidden)]
    pub fn with_temp_root(mut self, path: PathBuf, owner: OwnerRule) -> Self {
        self.temp_roots.push((path, owner));
        self
    }

    #[doc(hidden)]
    pub fn with_protected(mut self, path: PathBuf) -> Self {
        self.add_protected(path);
        self
    }

    #[doc(hidden)]
    pub fn with_required_temp_area(mut self, path: PathBuf) -> Self {
        self.required_temp_area = Some(fs::canonicalize(&path).unwrap_or(path));
        self
    }

    #[cfg(unix)]
    #[doc(hidden)]
    pub fn with_euid(mut self, euid: u32) -> Self {
        self.euid = euid;
        self
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    pub fn local_app_data(&self) -> &Path {
        &self.local_app_data
    }

    /// Names of the processes running now.
    pub(crate) fn process_names(&self) -> io::Result<Vec<String>> {
        match &self.processes {
            Processes::System => crate::process::names(),
            Processes::Fixed(names) => Ok(names.to_vec()),
            Processes::Unavailable => Err(io::Error::other("process list unavailable")),
        }
    }

    /// Records that the scan read or listed `path`.
    pub(crate) fn visit(&self, path: &Path) {
        if let Some(log) = &self.visits {
            log.lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(path.to_path_buf());
        }
    }

    pub fn temp_roots(&self) -> &[(PathBuf, OwnerRule)] {
        &self.temp_roots
    }

    pub fn now(&self) -> SystemTime {
        self.now
    }

    #[cfg(unix)]
    pub(crate) fn euid(&self) -> u32 {
        self.euid
    }

    fn add_protected(&mut self, path: PathBuf) {
        let path = fs::canonicalize(&path).unwrap_or(path);
        let id = measure::file_id(&path).ok();
        self.protected.push(Protected { path, id });
    }

    pub(crate) fn is_protected_id(&self, id: FileId) -> bool {
        self.protected.iter().any(|p| p.id == Some(id))
    }

    /// True if `path`, or any folder above it, is a protected folder. Compared
    /// by file identity, so `~/documents` matches `~/Documents` on a
    /// case-insensitive volume.
    pub fn is_protected(&self, path: &Path) -> bool {
        let resolved = resolve_existing(path);
        if resolved
            .ancestors()
            .any(|a| measure::file_id(a).is_ok_and(|id| self.is_protected_id(id)))
        {
            return true;
        }
        // Protected folders that do not exist yet are matched by path.
        self.protected
            .iter()
            .any(|p| p.id.is_none() && resolved.starts_with(&p.path))
    }
}

/// Canonicalizes the longest existing prefix of `path` and appends the rest.
fn resolve_existing(path: &Path) -> PathBuf {
    let mut missing = Vec::new();
    let mut current = path;
    loop {
        if let Ok(found) = fs::canonicalize(current) {
            return missing.iter().rev().fold(found, |acc, part| acc.join(part));
        }
        match (current.parent(), current.file_name()) {
            (Some(parent), Some(name)) => {
                missing.push(name.to_owned());
                current = parent;
            }
            _ => return path.to_path_buf(),
        }
    }
}

/// Checks a temp root against the catalog `user-temp` root rules. Failure
/// skips the row with the reason; there is no fallback guess.
pub fn validate_temp_root(
    ctx: &Context,
    path: &Path,
    owner: OwnerRule,
) -> Result<ValidRoot, RootRejection> {
    let canonical = fs::canonicalize(path).map_err(|e| match e.kind() {
        io::ErrorKind::NotFound => RootRejection::Missing,
        _ => RootRejection::Unreadable,
    })?;
    let metadata = fs::metadata(&canonical).map_err(|_| RootRejection::Unreadable)?;
    if !metadata.is_dir() {
        return Err(RootRejection::NotADirectory);
    }
    if canonical.parent().is_none() {
        return Err(RootRejection::FilesystemRoot);
    }
    if let Some(area) = &ctx.required_temp_area {
        if !canonical.starts_with(area) {
            return Err(RootRejection::OutsideSystemTempArea);
        }
    }
    let id = measure::file_id(&canonical).map_err(|_| RootRejection::Unreadable)?;
    let same = |a: &Path| measure::file_id(a).is_ok_and(|other| other == id);
    if ctx.home.ancestors().any(same) {
        return Err(RootRejection::HomeOrAncestor);
    }
    if ctx.is_protected(&canonical) {
        return Err(RootRejection::InsideProtectedFolder);
    }
    if ctx
        .protected
        .iter()
        .filter(|p| p.id.is_some())
        .any(|p| p.path.ancestors().any(same))
    {
        return Err(RootRejection::ContainsProtectedFolder);
    }
    if owner == OwnerRule::Root && !crate::platform::owned_by_user(ctx, &canonical, &metadata) {
        return Err(RootRejection::NotOwnedByUser);
    }
    Ok(ValidRoot {
        path: canonical,
        id,
        owner,
    })
}

/// Checks a cache folder found by [`crate::roots`]: a real folder, not a
/// link; inside the home; not a protected folder, inside one, or above one;
/// and owned by the user. Failure skips that folder with the reason.
pub fn validate_cache_root(ctx: &Context, path: &Path) -> Result<ValidRoot, SkipReason> {
    let link = fs::symlink_metadata(path).map_err(|_| SkipReason::Unreadable)?;
    let facts = measure::facts(path, &link).map_err(|_| SkipReason::Unreadable)?;
    match facts.kind {
        Kind::Dir => {}
        Kind::Link => return Err(SkipReason::Link),
        Kind::SocketOrPipe => return Err(SkipReason::SocketOrPipe),
        Kind::File | Kind::Device => return Err(SkipReason::SpecialFile),
    }
    let canonical = fs::canonicalize(path).map_err(|_| SkipReason::Unreadable)?;
    if canonical == ctx.home || !canonical.starts_with(&ctx.home) {
        return Err(SkipReason::OutsideHome);
    }
    if ctx.is_protected(&canonical) {
        return Err(SkipReason::ProtectedFolder);
    }
    let id = facts.id;
    let same = |a: &Path| measure::file_id(a).is_ok_and(|other| other == id);
    if ctx
        .protected
        .iter()
        .filter(|p| p.id.is_some())
        .any(|p| p.path.ancestors().any(same))
    {
        return Err(SkipReason::ProtectedFolder);
    }
    let metadata = fs::metadata(&canonical).map_err(|_| SkipReason::Unreadable)?;
    if !crate::platform::owned_by_user(ctx, &canonical, &metadata) {
        return Err(SkipReason::OtherOwner);
    }
    Ok(ValidRoot {
        path: canonical,
        id,
        owner: OwnerRule::Root,
    })
}
