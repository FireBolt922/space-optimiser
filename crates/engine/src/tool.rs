//! Finding and running package-manager and Docker tools (phase 6 items 1,
//! 2, and 5; phase 8 item 4). This is the only module that starts a
//! process, and [`command`] is the only place it does. Every tool run uses
//! a fixed argument list, no shell, a closed `stdin`, the app's empty work
//! folder as `cwd`, a minimal environment, and a timeout that kills the
//! tool and everything it started. The one other process is the OS file
//! manager for "Show in folder" ([`reveal`], phase 9 item 1).

use std::ffi::OsString;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use crate::measure::{self, FileId};
use crate::platform::RevealCommand;
use crate::policy::Context;

/// Read-only queries such as `pnpm store path` (phase 6 item 2).
pub const QUERY_TIMEOUT: Duration = Duration::from_secs(5);
/// Clean commands (phase 8 item 4).
pub const ACTION_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// How much of each output stream is kept.
const STDOUT_LIMIT: usize = 1024 * 1024;
const STDERR_TAIL: usize = 4 * 1024;
/// How long to wait for output after the tool is gone.
const DRAIN_WAIT: Duration = Duration::from_secs(1);
const POLL: Duration = Duration::from_millis(10);
/// How long "Show in folder" waits for the file manager's launcher to exit.
const REVEAL_WAIT: Duration = Duration::from_secs(3);

/// Where tools are looked for and how they run. Empty by default, so a
/// fixture context finds no tools unless a test adds fake ones.
#[derive(Debug, Clone)]
pub(crate) struct ToolEnv {
    /// The app's `PATH`, in order.
    pub path: Vec<PathBuf>,
    /// The catalog's fixed install folders, searched after `PATH`.
    pub install_dirs: Vec<PathBuf>,
    /// The empty folder every tool runs in. No folder, no tools.
    pub work_dir: Option<PathBuf>,
    pub query_timeout: Duration,
    pub action_timeout: Duration,
    /// Passed through unchanged (`DOCKER_HOST`, `DOCKER_CONTEXT`).
    pub pass_env: Vec<(OsString, OsString)>,
}

impl Default for ToolEnv {
    fn default() -> Self {
        Self {
            path: Vec::new(),
            install_dirs: Vec::new(),
            work_dir: None,
            query_timeout: QUERY_TIMEOUT,
            action_timeout: ACTION_TIMEOUT,
            pass_env: Vec::new(),
        }
    }
}

/// Set for every tool: no network for Corepack, no update checks, no
/// prompts, no progress bars.
const QUIET_ENV: &[(&str, &str)] = &[
    ("COREPACK_ENABLE_NETWORK", "0"),
    ("COREPACK_ENABLE_DOWNLOAD_PROMPT", "0"),
    ("npm_config_update_notifier", "false"),
    ("NO_UPDATE_NOTIFIER", "1"),
    ("PIP_DISABLE_PIP_VERSION_CHECK", "1"),
    ("UV_NO_PROGRESS", "1"),
    ("DOCKER_CLI_HINTS", "false"),
];

/// A tool found by [`find`]. The canonical path and identity are checked
/// again before a clean command runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Tool {
    pub name: &'static str,
    /// The file as found, which may be a link.
    pub path: PathBuf,
    pub canonical: PathBuf,
    pub id: FileId,
    /// The folder it was found in, and whether that folder came from `PATH`.
    pub dir: PathBuf,
    pub on_path: bool,
}

impl Tool {
    /// "found on PATH" or "found in ~/.cargo/bin", for the results row.
    pub fn found_text(&self, home: &Path) -> String {
        if self.on_path {
            "found on PATH".to_owned()
        } else {
            format!("found in {}", home_relative(home, &self.dir))
        }
    }
}

/// `~/rest` for a path inside `home`, otherwise the path itself.
pub(crate) fn home_relative(home: &Path, path: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
        Ok(rest) => format!("~/{}", rest.to_string_lossy().replace('\\', "/")),
        Err(_) => path.to_string_lossy().into_owned(),
    }
}

/// Why a tool was not run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum NotFound {
    #[error("it is not installed, or not where Space Optimiser looks")]
    Missing,
    /// A Corepack shim, which can download a package manager.
    #[error("it is managed by Corepack, which may download it, so it is not run")]
    Corepack,
    /// A Windows `.cmd` shim, which only runs through `cmd.exe`.
    #[error("on Windows it is a script that needs a shell, so it is not run in this version")]
    ScriptShim,
}

/// Looks for `name` on `PATH`, then in the fixed install folders. Never
/// runs a shell or reads shell start-up files.
pub(crate) fn find(ctx: &Context, name: &'static str) -> Result<Tool, NotFound> {
    let env = ctx.tools();
    let dirs = env
        .path
        .iter()
        .map(|d| (d, true))
        .chain(env.install_dirs.iter().map(|d| (d, false)));
    let mut shim = false;
    for (dir, on_path) in dirs {
        match sys::candidate(dir, name) {
            Lookup::Absent => {}
            Lookup::ScriptShim => shim = true,
            Lookup::Found(path) => {
                let Ok(canonical) = fs::canonicalize(&path) else {
                    continue;
                };
                if sys::is_developer_stub(dir, &canonical) {
                    continue;
                }
                let Ok(id) = measure::file_id(&canonical) else {
                    continue;
                };
                if matches!(name, "pnpm" | "yarn") && is_corepack(&canonical) {
                    return Err(NotFound::Corepack);
                }
                return Ok(Tool {
                    name,
                    path,
                    canonical,
                    id,
                    dir: dir.clone(),
                    on_path,
                });
            }
        }
    }
    Err(if shim {
        NotFound::ScriptShim
    } else {
        NotFound::Missing
    })
}

/// Finds `tool.name` again and checks it is the same file.
pub(crate) fn refind(ctx: &Context, tool: &Tool) -> bool {
    find(ctx, tool.name).is_ok_and(|now| now.canonical == tool.canonical && now.id == tool.id)
}

enum Lookup {
    Absent,
    Found(PathBuf),
    // Windows `.cmd` shims only.
    #[cfg_attr(unix, allow(dead_code))]
    ScriptShim,
}

/// Corepack installs `pnpm` and `yarn` as shims inside its own package.
fn is_corepack(canonical: &Path) -> bool {
    if canonical
        .components()
        .any(|c| c.as_os_str().eq_ignore_ascii_case("corepack"))
    {
        return true;
    }
    let mut head = Vec::with_capacity(4096);
    if let Ok(file) = fs::File::open(canonical) {
        let _ = file.take(4096).read_to_end(&mut head);
    }
    String::from_utf8_lossy(&head)
        .to_ascii_lowercase()
        .contains("corepack")
}

/// How a run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Exit {
    Success,
    /// A non-zero exit code, or a signal (`None`).
    Failed(Option<i32>),
    TimedOut,
    /// The tool printed one of the `stop_on` lines and was stopped.
    Stopped(&'static str),
}

#[derive(Debug)]
pub(crate) struct Output {
    pub exit: Exit,
    pub stdout: String,
    /// The last few KB of `stderr`, for the report and the audit log.
    pub stderr: String,
}

/// Receives each line a tool prints, cleaned by [`clean_line`], while the
/// tool runs. Called from the pipe reader threads.
pub(crate) type LineSink = Arc<dyn Fn(String) + Send + Sync>;

/// Runs `tool` with exactly `args`. `stop_on` lists output that means the
/// tool is waiting on something it should not (a lock, a download); the
/// tool is stopped as soon as one appears.
pub(crate) fn run(
    ctx: &Context,
    tool: &Tool,
    args: &[&str],
    timeout: Duration,
    stop_on: &'static [&'static str],
) -> io::Result<Output> {
    run_with_lines(ctx, tool, args, timeout, stop_on, None)
}

/// [`run`], also sending each printed line to `lines` as it arrives.
pub(crate) fn run_with_lines(
    ctx: &Context,
    tool: &Tool,
    args: &[&str],
    timeout: Duration,
    stop_on: &'static [&'static str],
    lines: Option<LineSink>,
) -> io::Result<Output> {
    let work = empty_work_dir(ctx)?;
    let mut command = command(&tool.path);
    command
        .args(args)
        .current_dir(&work)
        .env_clear()
        .envs(environment(ctx, tool))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut group = sys::Group::spawn(&mut command)?;

    let stopped: Arc<Mutex<Option<&'static str>>> = Arc::default();
    let stdout = reader(
        group.child().stdout.take(),
        STDOUT_LIMIT,
        stop_on,
        &stopped,
        lines.clone(),
    );
    let stderr = reader(
        group.child().stderr.take(),
        STDOUT_LIMIT,
        stop_on,
        &stopped,
        lines,
    );

    let deadline = Instant::now() + timeout;
    let exit = loop {
        if let Some(pattern) = *stopped.lock().unwrap_or_else(PoisonError::into_inner) {
            break Exit::Stopped(pattern);
        }
        match group.exited() {
            Ok(true) => break Exit::Success,
            Ok(false) => {}
            Err(e) => {
                group.kill();
                let _ = group.child().wait();
                return Err(e);
            }
        }
        if Instant::now() >= deadline {
            break Exit::TimedOut;
        }
        thread::sleep(POLL);
    };
    // Whatever the tool started dies with it, on success too.
    group.kill();
    let status = group.child().wait()?;
    let exit = match exit {
        Exit::Success if status.success() => Exit::Success,
        Exit::Success => Exit::Failed(status.code()),
        other => other,
    };
    let stdout = finish(stdout);
    let stderr = finish(stderr);
    let tail = stderr.len().saturating_sub(STDERR_TAIL);
    let tail = (tail..stderr.len())
        .find(|&i| stderr.is_char_boundary(i))
        .unwrap_or(stderr.len());
    Ok(Output {
        exit,
        stdout,
        stderr: stderr[tail..].trim().to_owned(),
    })
}

/// Every process this app starts is built here.
fn command(program: &Path) -> Command {
    Command::new(program)
}

/// Starts the OS file manager with `cmd`'s fixed arguments. It inherits the
/// app's environment (it is a desktop handler, not a cache tool) and is not
/// killed: the file manager window stays open. Returns the launcher's exit
/// status, or None when it is still running after [`REVEAL_WAIT`]; it is
/// then reaped on a background thread.
pub(crate) fn reveal(cmd: &RevealCommand) -> io::Result<Option<ExitStatus>> {
    let mut command = command(&cmd.program);
    sys::raw_args(&mut command, &cmd.args);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = command.spawn()?;
    let deadline = Instant::now() + REVEAL_WAIT;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        if Instant::now() >= deadline {
            thread::spawn(move || child.wait());
            return Ok(None);
        }
        thread::sleep(POLL);
    }
}

/// How many numbered work folders are tried before tools stop running.
const WORK_DIRS: usize = 20;

/// An empty folder for a tool to run in: the context's work folder, or the
/// first empty `work-2`, `work-3`, ... beside it. The app never deletes, so
/// a file a tool leaves behind moves later runs to the next folder instead
/// of stopping every tool row.
fn empty_work_dir(ctx: &Context) -> io::Result<PathBuf> {
    let base = ctx
        .tools()
        .work_dir
        .as_deref()
        .ok_or_else(|| io::Error::other("no work folder for tools"))?;
    for n in 1..=WORK_DIRS {
        let dir = if n == 1 {
            base.to_path_buf()
        } else {
            let mut name = base.file_name().unwrap_or_default().to_owned();
            name.push(format!("-{n}"));
            base.with_file_name(name)
        };
        fs::create_dir_all(&dir)?;
        let is_dir = fs::symlink_metadata(&dir).is_ok_and(|m| m.is_dir());
        if is_dir && fs::read_dir(&dir)?.next().is_none() {
            return Ok(dir);
        }
    }
    Err(io::Error::other("the app's work folders are not empty"))
}

type Stream = (Arc<Mutex<Vec<u8>>>, mpsc::Receiver<()>);

/// Reads a pipe on its own thread, keeping at most `limit` bytes, flags
/// the first `stop_on` pattern it sees, and sends each line to `lines`.
fn reader(
    pipe: Option<impl Read + Send + 'static>,
    limit: usize,
    stop_on: &'static [&'static str],
    stopped: &Arc<Mutex<Option<&'static str>>>,
    lines: Option<LineSink>,
) -> Stream {
    let buffer: Arc<Mutex<Vec<u8>>> = Arc::default();
    let (tx, rx) = mpsc::channel();
    let Some(mut pipe) = pipe else {
        let _ = tx.send(());
        return (buffer, rx);
    };
    let shared = buffer.clone();
    let stopped = stopped.clone();
    thread::spawn(move || {
        let mut chunk = [0u8; 8192];
        let mut seen = String::new();
        let mut partial: Vec<u8> = Vec::new();
        while let Ok(n) = pipe.read(&mut chunk) {
            if n == 0 {
                break;
            }
            if let Some(lines) = &lines {
                send_lines(&mut partial, &chunk[..n], lines);
            }
            {
                let mut kept = shared.lock().unwrap_or_else(PoisonError::into_inner);
                let room = limit.saturating_sub(kept.len()).min(n);
                kept.extend_from_slice(&chunk[..room]);
            }
            if !stop_on.is_empty() {
                seen.push_str(&String::from_utf8_lossy(&chunk[..n]));
                let lower = seen.to_ascii_lowercase();
                if let Some(p) = stop_on
                    .iter()
                    .find(|p| lower.contains(&p.to_ascii_lowercase()))
                {
                    stopped
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .get_or_insert(p);
                }
                // Keep only enough to match a pattern split across reads.
                if seen.len() > 512 {
                    let cut = seen.len() - 256;
                    let cut = (cut..seen.len())
                        .find(|&i| seen.is_char_boundary(i))
                        .unwrap_or(seen.len());
                    seen.drain(..cut);
                }
            }
        }
        if let Some(lines) = &lines
            && let Some(line) = clean_line(&String::from_utf8_lossy(&partial))
        {
            lines(line);
        }
        let _ = tx.send(());
    });
    (buffer, rx)
}

/// The longest line passed to a [`LineSink`], in characters.
const LINE_LIMIT: usize = 300;

/// Adds `bytes` to `partial` and sends every complete line. A carriage
/// return ends a line too, so a progress counter shows its latest value.
fn send_lines(partial: &mut Vec<u8>, bytes: &[u8], lines: &LineSink) {
    for &byte in bytes {
        if byte == b'\n' || byte == b'\r' {
            if let Some(line) = clean_line(&String::from_utf8_lossy(partial)) {
                lines(line);
            }
            partial.clear();
        } else if partial.len() < LINE_LIMIT * 4 {
            partial.push(byte);
        }
    }
}

/// A printed line made safe to show: terminal escape sequences and other
/// control characters removed, tabs as spaces, trimmed, and cut to
/// [`LINE_LIMIT`] characters. None for a line with nothing left.
pub(crate) fn clean_line(raw: &str) -> Option<String> {
    let mut out = String::with_capacity(raw.len().min(LINE_LIMIT));
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\u{1b}' => match chars.peek() {
                // CSI: ESC [ parameters, then one final byte in @..~.
                Some('[') => {
                    chars.next();
                    for c in chars.by_ref() {
                        if ('@'..='~').contains(&c) {
                            break;
                        }
                    }
                }
                // OSC: ESC ] text, ended by BEL or ESC \.
                Some(']') => {
                    chars.next();
                    while let Some(c) = chars.next() {
                        if c == '\u{7}' {
                            break;
                        }
                        if c == '\u{1b}' {
                            chars.next_if_eq(&'\\');
                            break;
                        }
                    }
                }
                _ => {
                    chars.next();
                }
            },
            '\t' => out.push(' '),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    let trimmed = out.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut line: String = trimmed.chars().take(LINE_LIMIT).collect();
    if trimmed.chars().count() > LINE_LIMIT {
        line.push('…');
    }
    Some(line)
}

/// The text a reader kept, once its pipe closed or [`DRAIN_WAIT`] passed.
/// A process that escaped the kill can hold a pipe open; it is not waited
/// for.
fn finish((buffer, finished): Stream) -> String {
    let _ = finished.recv_timeout(DRAIN_WAIT);
    let bytes = buffer.lock().unwrap_or_else(PoisonError::into_inner);
    String::from_utf8_lossy(&bytes).into_owned()
}

/// `PATH`, the home variables, the quiet flags, and the pass-through
/// variables. Nothing else from the app's environment.
fn environment(ctx: &Context, tool: &Tool) -> Vec<(OsString, OsString)> {
    let env = ctx.tools();
    let mut dirs = vec![tool.dir.clone()];
    if let Some(parent) = tool.canonical.parent() {
        dirs.push(parent.to_path_buf());
    }
    dirs.extend(env.path.iter().cloned());
    dirs.extend(env.install_dirs.iter().cloned());
    // Tools call system programs (`sh`, `env`); these folders are on the
    // tool's `PATH` but are not searched for tools.
    dirs.extend(sys::system_dirs());
    dirs.dedup();
    let mut vars = Vec::new();
    if let Ok(path) = std::env::join_paths(dirs.iter().filter(|d| !d.as_os_str().is_empty())) {
        vars.push((OsString::from("PATH"), path));
    }
    vars.extend(sys::home_vars(ctx));
    vars.extend(
        QUIET_ENV
            .iter()
            .map(|(k, v)| (OsString::from(k), OsString::from(v))),
    );
    vars.extend(env.pass_env.iter().cloned());
    vars
}

/// The catalog's fixed install folders for this OS (phase 6 item 1). Only
/// folders are listed; nothing in them is run unless a row asks for it by
/// name.
pub(crate) fn install_dirs(home: &Path, local_app_data: &Path) -> Vec<PathBuf> {
    sys::install_dirs(home, local_app_data)
}

/// The newest `~/.nvm/versions/node/<version>/bin`.
#[cfg(unix)]
fn newest_nvm(home: &Path) -> Option<PathBuf> {
    let versions = home.join(".nvm").join("versions").join("node");
    let mut found: Vec<(Vec<u64>, PathBuf)> = fs::read_dir(&versions)
        .ok()?
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let numbers = name
                .strip_prefix('v')?
                .split('.')
                .map(|n| n.parse().ok())
                .collect::<Option<Vec<u64>>>()?;
            Some((numbers, e.path().join("bin")))
        })
        .collect();
    found.sort();
    found.pop().map(|(_, bin)| bin)
}

#[cfg(unix)]
mod sys {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::process::CommandExt;

    pub fn raw_args(command: &mut Command, args: &[OsString]) {
        command.args(args);
    }

    pub fn candidate(dir: &Path, name: &str) -> Lookup {
        let path = dir.join(name);
        match fs::metadata(&path) {
            Ok(m) if m.is_file() && m.permissions().mode() & 0o111 != 0 => Lookup::Found(path),
            _ => Lookup::Absent,
        }
    }

    /// macOS `/usr/bin` holds developer-tool stubs (`python3`, `pip3`,
    /// `git`) that open an install dialog. Nothing there is run.
    pub fn is_developer_stub(dir: &Path, canonical: &Path) -> bool {
        cfg!(target_os = "macos")
            && (dir == Path::new("/usr/bin") || canonical.starts_with("/usr/bin"))
    }

    pub fn home_vars(ctx: &Context) -> Vec<(OsString, OsString)> {
        vec![(OsString::from("HOME"), ctx.home().as_os_str().to_owned())]
    }

    pub fn system_dirs() -> Vec<PathBuf> {
        ["/usr/bin", "/bin", "/usr/sbin", "/sbin"]
            .into_iter()
            .map(PathBuf::from)
            .collect()
    }

    pub fn install_dirs(home: &Path, _local_app_data: &Path) -> Vec<PathBuf> {
        let mut dirs = vec![
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/usr/local/bin"),
        ];
        for relative in [
            ".cargo/bin",
            ".local/bin",
            ".volta/bin",
            ".asdf/shims",
            ".local/share/mise/shims",
            ".docker/bin",
        ] {
            dirs.push(home.join(relative));
        }
        if cfg!(target_os = "macos") {
            dirs.push(home.join("Library/pnpm"));
            dirs.push(PathBuf::from(
                "/Applications/Docker.app/Contents/Resources/bin",
            ));
        } else {
            dirs.push(home.join(".local/share/pnpm"));
            dirs.push(PathBuf::from("/usr/bin"));
        }
        dirs.extend(newest_nvm(home));
        dirs
    }

    /// The tool leads its own process group, so a timeout reaches everything
    /// it started.
    pub struct Group {
        child: Child,
        pid: libc::pid_t,
    }

    impl Group {
        pub fn spawn(command: &mut Command) -> io::Result<Self> {
            command.process_group(0);
            let child = command.spawn()?;
            let pid = libc::pid_t::try_from(child.id()).map_err(io::Error::other)?;
            Ok(Self { child, pid })
        }

        pub fn child(&mut self) -> &mut Child {
            &mut self.child
        }

        /// Whether the tool has exited, without reaping it: the process
        /// group id cannot be reused while the zombie remains.
        pub fn exited(&self) -> io::Result<bool> {
            // SAFETY: an all-zero siginfo_t is a valid out-parameter.
            let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
            // SAFETY: `info` is writable; WNOWAIT leaves the child waitable.
            let status = unsafe {
                libc::waitid(
                    libc::P_PID,
                    self.pid as libc::id_t,
                    &mut info,
                    libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                )
            };
            if status != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(siginfo_pid(&info) != 0)
        }

        /// `SIGKILL` to the whole group. The tool is not reaped yet, so the
        /// group id still names this group.
        pub fn kill(&self) {
            // SAFETY: plain syscall; a group that is already gone is ESRCH.
            unsafe { libc::killpg(self.pid, libc::SIGKILL) };
        }
    }

    #[cfg(target_os = "macos")]
    fn siginfo_pid(info: &libc::siginfo_t) -> libc::pid_t {
        info.si_pid
    }

    #[cfg(target_os = "linux")]
    fn siginfo_pid(info: &libc::siginfo_t) -> libc::pid_t {
        // SAFETY: waitid with WEXITED fills the child fields.
        unsafe { info.si_pid() }
    }
}

#[cfg(windows)]
mod sys {
    use super::*;
    use std::os::windows::io::AsRawHandle;
    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
    };
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject, TerminateJobObject,
    };
    use windows_sys::Win32::System::Threading::{
        CREATE_NO_WINDOW, CREATE_SUSPENDED, OpenThread, ResumeThread, THREAD_SUSPEND_RESUME,
    };

    /// Passed as written: Explorer parses its own command line, and
    /// `/select,"<path>"` must not be quoted again.
    pub fn raw_args(command: &mut Command, args: &[OsString]) {
        for arg in args {
            command.raw_arg(arg);
        }
    }

    /// Only `.exe` files run. `npm`, `pnpm`, and `yarn` install `.cmd`
    /// shims, which need `cmd.exe`.
    pub fn candidate(dir: &Path, name: &str) -> Lookup {
        let exe = dir.join(format!("{name}.exe"));
        if fs::metadata(&exe).is_ok_and(|m| m.is_file()) {
            return Lookup::Found(exe);
        }
        let shims = ["cmd", "bat", "ps1"];
        if shims
            .iter()
            .any(|ext| fs::metadata(dir.join(format!("{name}.{ext}"))).is_ok_and(|m| m.is_file()))
        {
            return Lookup::ScriptShim;
        }
        Lookup::Absent
    }

    pub fn is_developer_stub(_dir: &Path, _canonical: &Path) -> bool {
        false
    }

    pub fn system_dirs() -> Vec<PathBuf> {
        std::env::var_os("SystemRoot")
            .map(|root| vec![PathBuf::from(root).join("System32")])
            .unwrap_or_default()
    }

    pub fn home_vars(ctx: &Context) -> Vec<(OsString, OsString)> {
        let mut vars = vec![
            (
                OsString::from("USERPROFILE"),
                ctx.home().as_os_str().to_owned(),
            ),
            (
                OsString::from("LOCALAPPDATA"),
                ctx.local_app_data().as_os_str().to_owned(),
            ),
        ];
        for name in [
            "APPDATA",
            "SystemRoot",
            "windir",
            "SystemDrive",
            "ProgramData",
        ] {
            if let Some(value) = std::env::var_os(name) {
                vars.push((OsString::from(name), value));
            }
        }
        // Without these, Windows tools put temporary files in the home.
        if let Some((temp, _)) = ctx.temp_roots().first() {
            for name in ["TEMP", "TMP"] {
                vars.push((OsString::from(name), temp.as_os_str().to_owned()));
            }
        }
        vars
    }

    pub fn install_dirs(home: &Path, local_app_data: &Path) -> Vec<PathBuf> {
        let mut dirs = vec![
            home.join(".cargo").join("bin"),
            home.join(".local").join("bin"),
            local_app_data
                .join("Microsoft")
                .join("WinGet")
                .join("Links"),
        ];
        let python = local_app_data.join("Programs").join("Python");
        if let Ok(entries) = fs::read_dir(&python) {
            let mut versions: Vec<PathBuf> = entries
                .flatten()
                .filter(|e| e.file_name().to_string_lossy().starts_with("Python3"))
                .map(|e| e.path().join("Scripts"))
                .collect();
            versions.sort();
            dirs.extend(versions.into_iter().rev());
        }
        if let Some(programs) = std::env::var_os("ProgramFiles") {
            dirs.push(
                PathBuf::from(programs)
                    .join("Docker")
                    .join("Docker")
                    .join("resources")
                    .join("bin"),
            );
        }
        dirs
    }

    /// The tool runs in a job object that kills every process in it when
    /// the job closes. It starts suspended so it cannot start anything
    /// before it is in the job.
    pub struct Group {
        child: Child,
        job: HANDLE,
    }

    // SAFETY: the job handle is owned by this value and used from one thread.
    unsafe impl Send for Group {}

    impl Group {
        pub fn spawn(command: &mut Command) -> io::Result<Self> {
            // SAFETY: null attributes and name create an anonymous job.
            let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if job.is_null() {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: an all-zero struct is valid; only the flag is set.
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            // SAFETY: `limits` is the struct this information class expects.
            let ok = unsafe {
                SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            };
            if ok == 0 {
                let error = io::Error::last_os_error();
                // SAFETY: `job` was created above.
                unsafe { CloseHandle(job) };
                return Err(error);
            }
            command.creation_flags(CREATE_SUSPENDED | CREATE_NO_WINDOW);
            let mut child = match command.spawn() {
                Ok(child) => child,
                Err(e) => {
                    // SAFETY: `job` was created above.
                    unsafe { CloseHandle(job) };
                    return Err(e);
                }
            };
            let group = |child| Self { child, job };
            // SAFETY: both handles are open.
            if unsafe { AssignProcessToJobObject(job, child.as_raw_handle() as HANDLE) } == 0 {
                let error = io::Error::last_os_error();
                let _ = child.kill();
                let _ = child.wait();
                drop(group(child));
                return Err(error);
            }
            let group = group(child);
            resume(group.child.id())?;
            Ok(group)
        }

        pub fn child(&mut self) -> &mut Child {
            &mut self.child
        }

        pub fn exited(&mut self) -> io::Result<bool> {
            Ok(self.child.try_wait()?.is_some())
        }

        pub fn kill(&self) {
            // SAFETY: `job` is open until drop.
            unsafe { TerminateJobObject(self.job, 1) };
        }
    }

    impl Drop for Group {
        fn drop(&mut self) {
            // SAFETY: `job` was created in `spawn` and is closed once.
            unsafe { CloseHandle(self.job) };
        }
    }

    /// Resumes every thread of a process started suspended.
    fn resume(pid: u32) -> io::Result<()> {
        // SAFETY: no pointers; 0 means every process.
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: an all-zero THREADENTRY32 is valid once dwSize is set.
        let mut entry: THREADENTRY32 = unsafe { std::mem::zeroed() };
        entry.dwSize = size_of::<THREADENTRY32>() as u32;
        let mut resumed = 0;
        // SAFETY: `snapshot` is open and `entry` is writable.
        let mut more = unsafe { Thread32First(snapshot, &mut entry) } != 0;
        while more {
            if entry.th32OwnerProcessID == pid {
                // SAFETY: plain call; a null handle means the thread is gone.
                let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
                if !thread.is_null() {
                    // SAFETY: `thread` is open.
                    let previous = unsafe { ResumeThread(thread) };
                    // SAFETY: `thread` is open and closed once.
                    unsafe { CloseHandle(thread) };
                    if previous != u32::MAX {
                        resumed += 1;
                    }
                }
            }
            // SAFETY: as above.
            more = unsafe { Thread32Next(snapshot, &mut entry) } != 0;
        }
        // SAFETY: `snapshot` was opened above.
        unsafe { CloseHandle(snapshot) };
        if resumed == 0 {
            return Err(io::Error::other("could not start the tool"));
        }
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    struct Fake {
        _dir: tempfile::TempDir,
        root: PathBuf,
    }

    impl Fake {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let root = fs::canonicalize(dir.path()).unwrap();
            fs::create_dir_all(root.join("home")).unwrap();
            fs::create_dir_all(root.join("bin")).unwrap();
            Self { _dir: dir, root }
        }

        fn tool(&self, name: &str, script: &str) -> PathBuf {
            let path = self.root.join("bin").join(name);
            fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
            path
        }

        fn ctx(&self) -> Context {
            Context::for_fixture(&self.root.join("home"))
                .with_tool_dirs(vec![self.root.join("bin")], Vec::new())
                .with_work_dir(self.root.join("work"))
                .with_tool_timeouts(Duration::from_secs(1), Duration::from_secs(1))
        }
    }

    #[test]
    fn a_tool_prints_its_output() {
        let fake = Fake::new();
        fake.tool("npm", "echo \"$@\"");
        let ctx = fake.ctx();
        let tool = find(&ctx, "npm").unwrap();
        let out = run(&ctx, &tool, &["config", "get", "cache"], QUERY_TIMEOUT, &[]).unwrap();
        assert_eq!(out.exit, Exit::Success);
        assert_eq!(out.stdout.trim(), "config get cache");
    }

    #[test]
    fn a_failing_tool_reports_its_exit_code_and_error() {
        let fake = Fake::new();
        fake.tool("npm", "echo broken >&2; exit 3");
        let ctx = fake.ctx();
        let tool = find(&ctx, "npm").unwrap();
        let out = run(&ctx, &tool, &[], QUERY_TIMEOUT, &[]).unwrap();
        assert_eq!(out.exit, Exit::Failed(Some(3)));
        assert_eq!(out.stderr, "broken");
    }

    // Phase 6 test 16: the tool runs in the app's empty work folder, with
    // only the minimal environment.
    #[test]
    fn a_tool_runs_in_the_empty_work_folder_with_a_minimal_environment() {
        let fake = Fake::new();
        fake.tool("uv", "pwd; env");
        let ctx = fake
            .ctx()
            .with_tool_env("DOCKER_HOST", "unix:///fixture.sock");
        let tool = find(&ctx, "uv").unwrap();
        let out = run(&ctx, &tool, &[], QUERY_TIMEOUT, &[]).unwrap();
        let mut lines = out.stdout.lines();
        assert_eq!(Path::new(lines.next().unwrap()), fake.root.join("work"));
        let names: Vec<&str> = lines.filter_map(|l| l.split('=').next()).collect();
        assert!(names.contains(&"HOME"));
        assert!(names.contains(&"COREPACK_ENABLE_NETWORK"));
        assert!(names.contains(&"DOCKER_HOST"));
        for leaked in [
            "USER",
            "TMPDIR",
            "npm_config_cache",
            "UV_CACHE_DIR",
            "SHELL",
        ] {
            assert!(!names.contains(&leaked), "{leaked} leaked into the tool");
        }
    }

    #[test]
    fn a_tool_never_runs_in_a_work_folder_that_is_not_empty() {
        let fake = Fake::new();
        fake.tool("uv", "pwd");
        fs::create_dir_all(fake.root.join("work")).unwrap();
        fs::write(fake.root.join("work/package.json"), b"{}").unwrap();
        let ctx = fake.ctx();
        let tool = find(&ctx, "uv").unwrap();
        let out = run(&ctx, &tool, &[], QUERY_TIMEOUT, &[]).unwrap();
        assert_eq!(Path::new(out.stdout.trim()), fake.root.join("work-2"));

        for n in 2..=WORK_DIRS {
            let dir = fake.root.join(format!("work-{n}"));
            fs::write(dir.join("left-behind"), b"x").unwrap_or_else(|_| {
                fs::create_dir_all(&dir).unwrap();
                fs::write(dir.join("left-behind"), b"x").unwrap();
            });
        }
        assert!(run(&ctx, &tool, &[], QUERY_TIMEOUT, &[]).is_err());
    }

    // Phase 6 test 14: a tool that starts a child and hangs is killed with
    // the child when the timeout passes.
    #[test]
    fn a_timeout_kills_the_tool_and_its_children() {
        let fake = Fake::new();
        let pids = fake.root.join("pids");
        fake.tool(
            "pnpm",
            &format!(
                "sleep 30 & echo $! > '{p}'; echo $$ >> '{p}'; sleep 30",
                p = pids.display()
            ),
        );
        let ctx = fake.ctx();
        let tool = find(&ctx, "pnpm").unwrap();
        let started = Instant::now();
        let out = run(&ctx, &tool, &[], Duration::from_secs(2), &[]).unwrap();
        assert_eq!(out.exit, Exit::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(10));
        let pids = fs::read_to_string(&pids).unwrap();
        for pid in pids.lines() {
            let pid: libc::pid_t = pid.trim().parse().unwrap();
            let deadline = Instant::now() + Duration::from_secs(5);
            // SAFETY: signal 0 only checks that the process exists.
            while unsafe { libc::kill(pid, 0) } == 0 && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(20));
            }
            // SAFETY: as above.
            assert_ne!(unsafe { libc::kill(pid, 0) }, 0, "process {pid} survived");
        }
    }

    #[test]
    fn each_printed_line_reaches_the_sink_while_the_tool_runs() {
        let fake = Fake::new();
        fake.tool(
            "uv",
            r"printf 'Clearing cache at: /x\n'; printf 'Removed 3 files\r'; printf '\033[1mdone\033[0m\n' >&2; printf 'no newline'",
        );
        let ctx = fake.ctx();
        let tool = find(&ctx, "uv").unwrap();
        let seen: Arc<Mutex<Vec<String>>> = Arc::default();
        let sink: LineSink = {
            let seen = seen.clone();
            Arc::new(move |line| seen.lock().unwrap().push(line))
        };
        let out = run_with_lines(&ctx, &tool, &[], QUERY_TIMEOUT, &[], Some(sink)).unwrap();
        assert_eq!(out.exit, Exit::Success);
        let mut lines = seen.lock().unwrap().clone();
        lines.sort();
        assert_eq!(
            lines,
            [
                "Clearing cache at: /x",
                "Removed 3 files",
                "done",
                "no newline"
            ]
        );
    }

    #[test]
    fn a_stop_line_still_stops_the_tool_when_lines_are_sent() {
        let fake = Fake::new();
        fake.tool(
            "uv",
            "echo 'Cache is currently in-use, waiting for other uv processes' >&2; sleep 30",
        );
        let ctx = fake.ctx();
        let tool = find(&ctx, "uv").unwrap();
        let seen: Arc<Mutex<Vec<String>>> = Arc::default();
        let sink: LineSink = {
            let seen = seen.clone();
            Arc::new(move |line| seen.lock().unwrap().push(line))
        };
        let out = run_with_lines(
            &ctx,
            &tool,
            &[],
            Duration::from_secs(20),
            &["in-use"],
            Some(sink),
        )
        .unwrap();
        assert_eq!(out.exit, Exit::Stopped("in-use"));
        assert_eq!(
            seen.lock().unwrap().as_slice(),
            ["Cache is currently in-use, waiting for other uv processes"]
        );
    }

    #[test]
    fn a_tool_that_prints_a_stop_line_is_stopped_at_once() {
        let fake = Fake::new();
        fake.tool(
            "uv",
            "echo 'Cache is currently in-use, waiting for other uv processes' >&2; sleep 30",
        );
        let ctx = fake.ctx();
        let tool = find(&ctx, "uv").unwrap();
        let started = Instant::now();
        let out = run(&ctx, &tool, &[], Duration::from_secs(20), &["in-use"]).unwrap();
        assert_eq!(out.exit, Exit::Stopped("in-use"));
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    // Phase 6 test 13: a tool missing from PATH is found in a listed install
    // folder, and the row can say where.
    #[test]
    fn a_tool_in_an_install_folder_is_found_and_says_where() {
        let fake = Fake::new();
        let cargo_bin = fake.root.join("home/.cargo/bin");
        fs::create_dir_all(&cargo_bin).unwrap();
        let uv = cargo_bin.join("uv");
        fs::write(&uv, "#!/bin/sh\n").unwrap();
        fs::set_permissions(&uv, fs::Permissions::from_mode(0o755)).unwrap();
        let ctx = Context::for_fixture(&fake.root.join("home"))
            .with_tool_dirs(vec![fake.root.join("bin")], vec![cargo_bin]);
        let tool = find(&ctx, "uv").unwrap();
        assert!(!tool.on_path);
        assert_eq!(tool.found_text(ctx.home()), "found in ~/.cargo/bin");
    }

    #[test]
    fn a_file_that_is_not_executable_is_not_a_tool() {
        let fake = Fake::new();
        fs::write(fake.root.join("bin/uv"), "#!/bin/sh\n").unwrap();
        assert_eq!(find(&fake.ctx(), "uv"), Err(NotFound::Missing));
    }

    // Phase 6 test 12: a Corepack shim is never run.
    #[test]
    fn a_corepack_shim_is_not_run() {
        let fake = Fake::new();
        let shims = fake.root.join("lib/node_modules/corepack/shims");
        fs::create_dir_all(&shims).unwrap();
        let target = shims.join("pnpm");
        fs::write(&target, "#!/usr/bin/env node\n").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o755)).unwrap();
        std::os::unix::fs::symlink(&target, fake.root.join("bin/pnpm")).unwrap();
        assert_eq!(find(&fake.ctx(), "pnpm"), Err(NotFound::Corepack));
        fake.tool("yarn", "# installed by corepack enable");
        assert_eq!(find(&fake.ctx(), "yarn"), Err(NotFound::Corepack));
    }

    #[test]
    fn a_replaced_tool_is_not_the_same_tool() {
        let fake = Fake::new();
        let path = fake.tool("pnpm", "exit 0");
        let ctx = fake.ctx();
        let tool = find(&ctx, "pnpm").unwrap();
        assert!(refind(&ctx, &tool));
        fs::rename(&path, fake.root.join("old-pnpm")).unwrap();
        fake.tool("pnpm", "exit 0");
        assert!(!refind(&ctx, &tool));
    }
}

// Windows has no shell scripts to fake a tool with, so these use two
// programs every Windows has. The job-object kill of grandchildren is
// checked by hand in M7 (phase 9).
#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;

    fn system32() -> PathBuf {
        PathBuf::from(std::env::var_os("SystemRoot").expect("SystemRoot")).join("System32")
    }

    fn ctx(dir: &Path) -> Context {
        Context::for_fixture(dir)
            .with_tool_dirs(vec![system32()], Vec::new())
            .with_work_dir(dir.join("work"))
    }

    #[test]
    fn a_windows_tool_runs_and_prints() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());
        let tool = find(&ctx, "hostname").unwrap();
        let out = run(&ctx, &tool, &[], QUERY_TIMEOUT, &[]).unwrap();
        assert_eq!(out.exit, Exit::Success);
        assert!(!out.stdout.trim().is_empty());
    }

    #[test]
    fn a_windows_tool_that_hangs_is_stopped_at_the_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());
        let tool = find(&ctx, "ping").unwrap();
        let started = Instant::now();
        let args = ["-n", "60", "127.0.0.1"];
        let out = run(&ctx, &tool, &args, Duration::from_secs(2), &[]).unwrap();
        assert_eq!(out.exit, Exit::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(20));
    }
}

#[cfg(test)]
mod line_tests {
    use super::{LINE_LIMIT, clean_line};

    #[test]
    fn colour_and_title_sequences_are_removed() {
        assert_eq!(clean_line("\u{1b}[1;32mok\u{1b}[0m").as_deref(), Some("ok"));
        assert_eq!(
            clean_line("\u{1b}]0;window title\u{7}rest").as_deref(),
            Some("rest")
        );
        assert_eq!(
            clean_line("\u{1b}]8;;http://x\u{1b}\\link").as_deref(),
            Some("link")
        );
    }

    #[test]
    fn control_characters_go_and_tabs_become_spaces() {
        assert_eq!(clean_line("a\u{0}b\u{7}c\td").as_deref(), Some("abc d"));
        assert_eq!(clean_line("  \u{1b}[0m \t ").as_deref(), None);
    }

    #[test]
    fn a_long_line_is_cut() {
        let line = clean_line(&"é".repeat(1000)).unwrap();
        assert_eq!(line.chars().count(), LINE_LIMIT + 1);
        assert!(line.ends_with('…'));
    }
}
