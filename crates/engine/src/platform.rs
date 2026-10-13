//! macOS, Windows, and Linux path adapters (phase 4 item 3, moved from
//! phase 9). Folders come from OS APIs, not from environment variables;
//! hardening continues in M7 (phase 9).

use std::ffi::OsString;
use std::fs::{self, Metadata};
use std::io;
use std::path::{Path, PathBuf};

use crate::policy::Context;
#[cfg(target_os = "linux")]
use crate::policy::OwnerRule;

/// The fixed command that shows an item in the OS file manager (phase 9
/// item 1). Built from a candidate's path, never from a path the UI sent.
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevealCommand {
    pub program: PathBuf,
    pub args: Vec<OsString>,
}

/// `open -R <target>` on macOS, `xdg-open <parent>` on Linux, and Explorer
/// `/select,"<target>"` on Windows. None when the file manager is missing.
pub fn reveal_command(target: &Path) -> Option<RevealCommand> {
    sys::reveal_command(target)
}

/// The person's Downloads folder, if it exists. Reports are saved there;
/// the folder is never created.
pub(crate) fn downloads_dir(ctx: &Context) -> Option<PathBuf> {
    sys::downloads_dir(ctx).filter(|dir| dir.is_dir())
}

/// Folders under a home that are never walked for deletion (catalog
/// denylist). The union over all platforms: a name that does not exist on
/// this OS protects nothing and costs nothing.
const PROTECTED_UNDER_HOME: &[&str] = &[
    "Documents",
    "Desktop",
    "Downloads",
    "Pictures",
    "Movies",
    "Music",
    "Videos",
    "Public",
    ".ssh",
    ".gnupg",
    ".aws",
    ".azure",
    ".kube",
    ".config/gcloud",
    "Library/Mobile Documents",
    "Library/CloudStorage",
    "Library/Containers",
    "Library/Group Containers",
    "Library/Mail",
    "Library/Messages",
    "Library/Safari",
    "Library/Keychains",
    "Dropbox",
    "Google Drive",
    // The Trash is never emptied and never searched (catalog denylist).
    ".Trash",
    ".local/share/Trash",
];

/// The standard protected folders under `home`, the XDG user folders from
/// `home/.config/user-dirs.dirs`, and any `OneDrive*` folder in `home`.
pub(crate) fn protected_under_home(home: &Path) -> Vec<PathBuf> {
    let mut protected: Vec<PathBuf> = PROTECTED_UNDER_HOME.iter().map(|p| home.join(p)).collect();
    if let Ok(contents) = fs::read_to_string(home.join(".config/user-dirs.dirs")) {
        protected.extend(parse_user_dirs(&contents, home));
    }
    if let Ok(entries) = fs::read_dir(home) {
        protected.extend(
            entries
                .flatten()
                .filter(|e| e.file_name().to_string_lossy().starts_with("OneDrive"))
                .map(|e| e.path()),
        );
    }
    protected
}

/// Parses `user-dirs.dirs` (`XDG_<NAME>_DIR="$HOME/<path>"` or an absolute
/// path). A value equal to the home itself means "unset" in the XDG spec and
/// is dropped, so the home never becomes a protected folder by accident.
pub fn parse_user_dirs(contents: &str, home: &Path) -> Vec<PathBuf> {
    user_dir_entries(contents, home)
        .map(|(_, path)| path)
        .collect()
}

/// Each `XDG_<NAME>_DIR` key in `user-dirs.dirs` with its folder, under the
/// rules of [`parse_user_dirs`].
fn user_dir_entries<'a>(
    contents: &'a str,
    home: &'a Path,
) -> impl Iterator<Item = (&'a str, PathBuf)> + 'a {
    contents.lines().filter_map(move |line| {
        let (key, value) = line.trim().split_once('=')?;
        if !(key.starts_with("XDG_") && key.ends_with("_DIR")) {
            return None;
        }
        let value = value.strip_prefix('"')?.strip_suffix('"')?;
        let path = match value.strip_prefix("$HOME") {
            Some(rest) => home.join(rest.trim_start_matches('/')),
            None if value.starts_with('/') => PathBuf::from(value),
            None => return None,
        };
        (path != home).then_some((key, path))
    })
}

pub(crate) fn owned_by_user(ctx: &Context, path: &Path, metadata: &Metadata) -> bool {
    sys::owned_by_user(ctx, path, metadata)
}

/// Resolves this machine's home, protected folders, temp roots, and where
/// tools are looked for.
pub fn resolve_context() -> io::Result<Context> {
    Ok(with_tools(sys::resolve_context()?))
}

/// The app's `PATH` (absolute folders only), the catalog's fixed install
/// folders, and the Docker variables that decide where Docker points. Tools
/// get nothing else from this environment (phase 6 item 1).
fn with_tools(ctx: Context) -> Context {
    let path = std::env::var_os("PATH")
        .map(|p| {
            std::env::split_paths(&p)
                .filter(|d| d.is_absolute())
                .collect()
        })
        .unwrap_or_default();
    let install = crate::tool::install_dirs(ctx.home(), ctx.local_app_data());
    let mut ctx = ctx.with_tool_dirs(path, install);
    for key in ["DOCKER_HOST", "DOCKER_CONTEXT"] {
        if let Some(value) = std::env::var_os(key) {
            ctx = ctx.with_tool_env(key, value);
        }
    }
    ctx
}

#[cfg(unix)]
fn home_from_passwd() -> io::Result<PathBuf> {
    use std::ffi::{CStr, OsStr};
    use std::os::unix::ffi::OsStrExt;

    // `$HOME` can still name the invoking user under sudo; the password
    // database names the effective one.
    let mut buffer = vec![0 as libc::c_char; 16 * 1024];
    // SAFETY: an all-zero passwd is a valid out-parameter.
    let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
    let mut result = std::ptr::null_mut();
    // SAFETY: every pointer is valid and `buffer.len()` is its true length.
    let status = unsafe {
        libc::getpwuid_r(
            libc::geteuid(),
            &mut pwd,
            buffer.as_mut_ptr(),
            buffer.len(),
            &mut result,
        )
    };
    if status != 0 || result.is_null() || pwd.pw_dir.is_null() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "no home directory for this user",
        ));
    }
    // SAFETY: getpwuid_r succeeded, so `pw_dir` is NUL-terminated in `buffer`.
    let dir = unsafe { CStr::from_ptr(pwd.pw_dir) };
    Ok(PathBuf::from(OsStr::from_bytes(dir.to_bytes())))
}

#[cfg(target_os = "macos")]
mod sys {
    use super::*;
    use std::os::unix::ffi::OsStringExt;
    use std::os::unix::fs::MetadataExt;

    pub fn owned_by_user(ctx: &Context, _path: &Path, metadata: &Metadata) -> bool {
        metadata.uid() == ctx.euid()
    }

    /// `open -R` selects the item in a Finder window.
    pub fn reveal_command(target: &Path) -> Option<RevealCommand> {
        Some(RevealCommand {
            program: PathBuf::from("/usr/bin/open"),
            args: vec![OsString::from("-R"), target.as_os_str().to_owned()],
        })
    }

    pub fn downloads_dir(ctx: &Context) -> Option<PathBuf> {
        Some(ctx.home().join("Downloads"))
    }

    pub fn resolve_context() -> io::Result<Context> {
        let home = home_from_passwd()?;
        let temp = user_temp_dir()?;
        Ok(Context::new(
            home.clone(),
            protected_under_home(&home),
            vec![(temp, crate::policy::OwnerRule::Root)],
            Some(PathBuf::from("/private/var/folders")),
        ))
    }

    /// `confstr(_CS_DARWIN_USER_TEMP_DIR)`: the per-user temp directory,
    /// whatever `TMPDIR` says. Never `/private/tmp`.
    fn user_temp_dir() -> io::Result<PathBuf> {
        let mut buffer = vec![0u8; 1024];
        // SAFETY: `buffer` is writable for its full length.
        let needed = unsafe {
            libc::confstr(
                libc::_CS_DARWIN_USER_TEMP_DIR,
                buffer.as_mut_ptr().cast(),
                buffer.len(),
            )
        };
        if needed == 0 || needed > buffer.len() {
            return Err(io::Error::last_os_error());
        }
        buffer.truncate(needed - 1);
        Ok(PathBuf::from(std::ffi::OsString::from_vec(buffer)))
    }
}

#[cfg(target_os = "linux")]
mod sys {
    use super::*;
    use std::os::unix::fs::MetadataExt;

    pub fn owned_by_user(ctx: &Context, _path: &Path, metadata: &Metadata) -> bool {
        metadata.uid() == ctx.euid()
    }

    /// `xdg-open` cannot select an item, so it opens the folder holding it.
    /// Only the system copies are used, never one found on `PATH`.
    pub fn reveal_command(target: &Path) -> Option<RevealCommand> {
        let program = ["/usr/bin/xdg-open", "/bin/xdg-open"]
            .into_iter()
            .map(PathBuf::from)
            .find(|p| fs::metadata(p).is_ok_and(|m| m.is_file()))?;
        let folder = target.parent()?;
        Some(RevealCommand {
            program,
            args: vec![folder.as_os_str().to_owned()],
        })
    }

    /// `XDG_DOWNLOAD_DIR` from `user-dirs.dirs` (it may be localised),
    /// else `~/Downloads`.
    pub fn downloads_dir(ctx: &Context) -> Option<PathBuf> {
        let home = ctx.home();
        let configured = fs::read_to_string(home.join(".config/user-dirs.dirs"))
            .ok()
            .and_then(|contents| {
                user_dir_entries(&contents, home)
                    .find(|(key, _)| *key == "XDG_DOWNLOAD_DIR")
                    .map(|(_, path)| path)
            });
        Some(configured.unwrap_or_else(|| home.join("Downloads")))
    }

    pub fn resolve_context() -> io::Result<Context> {
        let home = home_from_passwd()?;
        // `/tmp` is shared, so each entry's owner is checked. `/var/tmp` is
        // never emptied.
        let mut temp_roots = vec![(PathBuf::from("/tmp"), OwnerRule::PerEntry)];
        let cache_tmp = home.join(".cache/tmp");
        if cache_tmp.is_dir() {
            temp_roots.push((cache_tmp, OwnerRule::Root));
        }
        Ok(Context::new(
            home.clone(),
            protected_under_home(&home),
            temp_roots,
            None,
        ))
    }
}

#[cfg(all(unix, not(any(target_os = "macos", target_os = "linux"))))]
compile_error!("Space Optimiser supports macOS, Linux, and Windows only.");

/// Explorer does not understand `\\?\` paths: `\\?\C:\x` becomes
/// `C:\x` and `\\?\UNC\server\x` becomes `\\server\x`.
#[cfg(any(windows, test))]
fn without_verbatim_prefix(units: &[u16]) -> Vec<u16> {
    let utf16 = |s: &str| s.encode_utf16().collect::<Vec<u16>>();
    let unc = utf16(r"\\?\UNC\");
    let verbatim = utf16(r"\\?\");
    if units.starts_with(&unc) {
        utf16(r"\\")
            .into_iter()
            .chain(units[unc.len()..].iter().copied())
            .collect()
    } else if units.starts_with(&verbatim) {
        units[verbatim.len()..].to_vec()
    } else {
        units.to_vec()
    }
}

#[cfg(windows)]
pub(crate) fn wide(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().chain([0]).collect()
}

#[cfg(windows)]
mod sys {
    use super::*;
    use crate::policy::OwnerRule;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_SUCCESS, HANDLE, LocalFree};
    use windows_sys::Win32::Security::Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT};
    use windows_sys::Win32::Security::{
        EqualSid, GetTokenInformation, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID,
        TOKEN_OWNER, TOKEN_QUERY, TOKEN_USER, TokenOwner, TokenUser,
    };
    use windows_sys::Win32::Storage::FileSystem::GetTempPathW;
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    use windows_sys::Win32::UI::Shell::{
        FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads, FOLDERID_LocalAppData,
        FOLDERID_Music, FOLDERID_Pictures, FOLDERID_Profile, FOLDERID_RoamingAppData,
        FOLDERID_SkyDrive, FOLDERID_Videos, FOLDERID_Windows, SHGetKnownFolderPath,
    };
    use windows_sys::core::{GUID, PWSTR};

    /// `explorer.exe /select,"<path>"` from the Windows folder. The single
    /// argument is passed as written (`raw_arg`), because Explorer parses
    /// its own command line.
    pub fn reveal_command(target: &Path) -> Option<RevealCommand> {
        let program = known_folder(&FOLDERID_Windows).ok()?.join("explorer.exe");
        let units = without_verbatim_prefix(&target.as_os_str().encode_wide().collect::<Vec<_>>());
        let mut arg = OsString::from("/select,\"");
        arg.push(OsString::from_wide(&units));
        arg.push("\"");
        Some(RevealCommand {
            program,
            args: vec![arg],
        })
    }

    /// The Downloads Known Folder, which follows redirection.
    pub fn downloads_dir(_ctx: &Context) -> Option<PathBuf> {
        known_folder(&FOLDERID_Downloads).ok()
    }

    pub fn resolve_context() -> io::Result<Context> {
        let home = known_folder(&FOLDERID_Profile)?;
        let mut protected = protected_under_home(&home);
        // Known Folders follow redirection (for example Documents moved to
        // another drive or into OneDrive).
        for id in [
            &FOLDERID_Documents,
            &FOLDERID_Desktop,
            &FOLDERID_Downloads,
            &FOLDERID_Pictures,
            &FOLDERID_Music,
            &FOLDERID_Videos,
            &FOLDERID_SkyDrive,
        ] {
            if let Ok(path) = known_folder(id) {
                protected.push(path);
            }
        }
        let mut temp_roots = vec![(temp_path()?, OwnerRule::Root)];
        let local = known_folder(&FOLDERID_LocalAppData).ok();
        if let Some(local) = &local {
            // The scan deduplicates the two roots by file identity.
            temp_roots.push((local.join("Temp"), OwnerRule::Root));
        }
        // `TEMP` can point anywhere, so a root outside Local AppData is
        // rejected, as a macOS root outside `/private/var/folders` is.
        let ctx = Context::new(home, protected, temp_roots, local.clone());
        // Browser caches and Explorer's thumbnails live here (phase 5).
        let ctx = match local {
            Some(local) => ctx.with_local_app_data(local),
            None => ctx,
        };
        // VS Code and Cursor keep their user data under Roaming AppData.
        Ok(match known_folder(&FOLDERID_RoamingAppData) {
            Ok(roaming) => ctx.with_roaming_app_data(roaming),
            Err(_) => ctx,
        })
    }

    fn known_folder(id: &GUID) -> io::Result<PathBuf> {
        let mut raw: PWSTR = std::ptr::null_mut();
        // SAFETY: `raw` is a valid out-pointer; a null token means this user.
        let hr = unsafe { SHGetKnownFolderPath(id, 0, std::ptr::null_mut(), &mut raw) };
        let result = if hr >= 0 && !raw.is_null() {
            // SAFETY: on success `raw` is a NUL-terminated string.
            let len = unsafe { (0..).take_while(|&i| *raw.add(i) != 0).count() };
            // SAFETY: `len` units were just read from `raw`.
            let units = unsafe { std::slice::from_raw_parts(raw, len) };
            Ok(PathBuf::from(OsString::from_wide(units)))
        } else {
            Err(io::Error::from_raw_os_error(hr))
        };
        // SAFETY: the shell allocated `raw` (or it is null, which is allowed).
        unsafe { CoTaskMemFree(raw as *const _) };
        result
    }

    /// `GetTempPathW` honours `TMP`/`TEMP`; validation rejects a hostile value.
    fn temp_path() -> io::Result<PathBuf> {
        let mut buffer = vec![0u16; 1024];
        // SAFETY: `buffer` is writable for its full length.
        let len = unsafe { GetTempPathW(buffer.len() as u32, buffer.as_mut_ptr()) } as usize;
        if len == 0 || len > buffer.len() {
            return Err(io::Error::last_os_error());
        }
        buffer.truncate(len);
        Ok(PathBuf::from(OsString::from_wide(&buffer)))
    }

    /// The folder's owner is this user, or this token's default owner for new
    /// objects (an elevated token's files belong to Administrators).
    pub fn owned_by_user(_ctx: &Context, path: &Path, _metadata: &Metadata) -> bool {
        let name = wide(path);
        let mut owner: PSID = std::ptr::null_mut();
        let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        // SAFETY: `name` is NUL-terminated; unused outputs are null.
        let status = unsafe {
            GetNamedSecurityInfoW(
                name.as_ptr(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION,
                &mut owner,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut descriptor,
            )
        };
        if status != ERROR_SUCCESS {
            return false;
        }
        let owned = token_sids().iter().any(|(buffer, offset)| {
            // SAFETY: the SID sits at `offset` inside its live, unmoved
            // buffer, and `owner` lives until `descriptor` is freed.
            unsafe { EqualSid(owner, buffer.as_ptr().add(*offset) as PSID) != 0 }
        });
        // SAFETY: `descriptor` was allocated by GetNamedSecurityInfoW.
        unsafe { LocalFree(descriptor) };
        owned
    }

    /// The token's user SID and default-owner SID: each buffer with the
    /// offset of its SID.
    fn token_sids() -> Vec<(Vec<u8>, usize)> {
        let mut token: HANDLE = std::ptr::null_mut();
        // SAFETY: the pseudo-handle needs no closing; `token` is an out-param.
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
            return Vec::new();
        }
        let mut sids = Vec::new();
        for class in [TokenUser, TokenOwner] {
            let mut buffer = vec![0u8; 256];
            let mut len = 0u32;
            // SAFETY: `buffer` is writable for `buffer.len()` bytes.
            let ok = unsafe {
                GetTokenInformation(
                    token,
                    class,
                    buffer.as_mut_ptr().cast(),
                    buffer.len() as u32,
                    &mut len,
                )
            };
            if ok == 0 {
                continue;
            }
            // Both TOKEN_USER and TOKEN_OWNER start with a SID pointer into
            // `buffer`, which stays alive inside `sids`.
            // SAFETY: GetTokenInformation filled `buffer` with that struct.
            let sid = unsafe {
                if class == TokenUser {
                    (*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid
                } else {
                    (*buffer.as_ptr().cast::<TOKEN_OWNER>()).Owner
                }
            };
            let offset = sid as usize - buffer.as_ptr() as usize;
            sids.push((buffer, offset));
        }
        // SAFETY: `token` was opened above.
        unsafe { CloseHandle(token) };
        sids
    }
}

#[cfg(test)]
mod tests {
    use super::without_verbatim_prefix;

    fn strip(path: &str) -> String {
        String::from_utf16(&without_verbatim_prefix(
            &path.encode_utf16().collect::<Vec<_>>(),
        ))
        .unwrap()
    }

    #[test]
    fn explorer_gets_paths_without_the_verbatim_prefix() {
        assert_eq!(strip(r"\\?\C:\Users\a b\x."), r"C:\Users\a b\x.");
        assert_eq!(strip(r"\\?\UNC\server\share\x"), r"\\server\share\x");
        assert_eq!(strip(r"C:\plain"), r"C:\plain");
        assert_eq!(strip(r"\\server\share"), r"\\server\share");
    }
}
