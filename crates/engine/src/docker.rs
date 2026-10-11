//! `docker-build-cache` (phase 6 items 7 and 8). Docker is asked, in order:
//! where it points (`DOCKER_HOST` or the active context: only a local
//! socket or named pipe is eligible), whether it is Podman, whether the
//! daemon answers, and what `docker system df` reports. Only the build
//! cache line can become a candidate; volumes are never offered. The clean
//! command is `docker builder prune -f` (dangling build cache, no `-a`).

use std::fs;
use std::path::PathBuf;

use serde_json::Value;

use crate::discover::CancelToken;
use crate::measure;
use crate::model::{DiskFile, DockerContainer, DockerImage, DockerVolume};
use crate::policy::Context;
use crate::tool::{self, Exit, Tool};

/// The fixed clean command.
pub(crate) const PRUNE: &[&str] = &["builder", "prune", "-f"];

/// What the checks found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Status {
    /// The build cache, with Docker's reclaimable bytes.
    BuildCache {
        reclaimable: u64,
        /// Reclaimable bytes on the volumes line, shown as a note only.
        volumes: Option<u64>,
    },
    /// A reason for one note, and no candidate.
    NotOffered(&'static str),
}

pub(crate) const REMOTE: &str =
    "Docker points to another machine, so its build cache is not offered.";
pub(crate) const PODMAN: &str = "This is Podman, not Docker, so its build cache is not offered.";
pub(crate) const SILENT: &str = "Docker did not respond.";
pub(crate) const UNREADABLE: &str = "Could not read Docker's report.";
pub(crate) const SETTINGS: &str =
    "Could not read Docker's settings, so its build cache is not offered.";

pub(crate) const DETAIL_SLOW: &str =
    "Docker took too long to list its images, containers, and volumes.";
pub(crate) const DETAIL_UNREADABLE: &str =
    "Could not read Docker's list of images, containers, and volumes.";

/// Docker's images, containers, and volumes, for information only.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Lists {
    pub images: Vec<DockerImage>,
    pub containers: Vec<DockerContainer>,
    pub volumes: Vec<DockerVolume>,
}

pub(crate) enum Details {
    Listed(Lists),
    /// Why there are no lists, in words.
    Note(&'static str),
    Cancelled,
}

/// `docker system df -v`, read-only, after [`status`] found a local Docker
/// that answers. It can take long (Docker sizes every volume), so it has
/// its own timeout and stops at once when the scan is cancelled.
pub(crate) fn details(ctx: &Context, docker: &Tool, cancel: &CancelToken) -> Details {
    let args = ["system", "df", "-v", "--format", "json"];
    let timeout = ctx.tools().detail_timeout;
    match tool::run_with_lines(ctx, docker, &args, timeout, &[], None, Some(cancel)) {
        Ok(out) => match out.exit {
            Exit::Success => match parse_verbose(&out.stdout) {
                Some(lists) => Details::Listed(lists),
                None => Details::Note(DETAIL_UNREADABLE),
            },
            Exit::Cancelled => Details::Cancelled,
            Exit::TimedOut => Details::Note(DETAIL_SLOW),
            Exit::Failed(_) | Exit::Stopped(_) => Details::Note(DETAIL_UNREADABLE),
        },
        Err(_) => Details::Note(DETAIL_UNREADABLE),
    }
}

/// One JSON object with `Images`, `Containers`, and `Volumes` arrays, whose
/// numbers are strings. A missing array is empty; anything that is not
/// such an object is `None`.
pub(crate) fn parse_verbose(output: &str) -> Option<Lists> {
    let value: Value = serde_json::from_str(output.trim()).ok()?;
    let object = value.as_object()?;
    if !["Images", "Containers", "Volumes"]
        .iter()
        .any(|k| object.contains_key(*k))
    {
        return None;
    }
    let rows = |key: &str| -> Vec<&Value> {
        object
            .get(key)
            .and_then(Value::as_array)
            .map(|a| a.iter().collect())
            .unwrap_or_default()
    };
    Some(Lists {
        images: rows("Images")
            .into_iter()
            .map(|r| DockerImage {
                repository: text(r, "Repository"),
                tag: text(r, "Tag"),
                size_bytes: size(r, "Size"),
                unique_bytes: size(r, "UniqueSize"),
                containers: count(r, "Containers"),
            })
            .collect(),
        containers: rows("Containers")
            .into_iter()
            .map(|r| DockerContainer {
                name: text(r, "Names"),
                image: text(r, "Image"),
                state: text(r, "State"),
                status: text(r, "Status"),
                size_bytes: size(r, "Size"),
            })
            .collect(),
        volumes: rows("Volumes")
            .into_iter()
            .map(|r| DockerVolume {
                name: text(r, "Name"),
                size_bytes: size(r, "Size"),
                in_use_by: count(r, "Links"),
            })
            .collect(),
    })
}

fn text(row: &Value, key: &str) -> String {
    row.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn size(row: &Value, key: &str) -> Option<u64> {
    row.get(key).and_then(Value::as_str).and_then(parse_size)
}

/// A count Docker prints as a string ("1") or a number; "N/A" is none.
fn count(row: &Value, key: &str) -> Option<u64> {
    match row.get(key)? {
        Value::String(s) => s.trim().parse().ok(),
        Value::Number(n) => n.as_u64(),
        _ => None,
    }
}

/// Where Docker Desktop keeps its virtual disk on this OS, newest layout
/// first.
fn disk_file_paths(ctx: &Context) -> Vec<PathBuf> {
    if cfg!(target_os = "macos") {
        vec![
            ctx.home()
                .join("Library/Containers/com.docker.docker/Data/vms/0/data/Docker.raw"),
        ]
    } else if cfg!(windows) {
        let docker = ctx.local_app_data().join("Docker").join("wsl");
        vec![
            docker.join("disk").join("docker_data.vhdx"),
            docker.join("data").join("ext4.vhdx"),
        ]
    } else {
        vec![ctx.home().join(".docker/desktop/vms/0/data/Docker.raw")]
    }
}

/// Docker Desktop's disk file, measured with one `stat` of its fixed path.
/// No folder on the way is listed, and a link is not followed or measured.
pub(crate) fn disk_file(ctx: &Context) -> Option<DiskFile> {
    disk_file_paths(ctx).into_iter().find_map(|path| {
        let metadata = fs::symlink_metadata(&path).ok()?;
        if !metadata.file_type().is_file() {
            return None;
        }
        let facts = measure::facts(&path, &metadata).ok()?;
        Some(DiskFile {
            location_label: tool::home_relative(ctx.home(), &path),
            allocated_bytes: facts.size.bytes,
            logical_bytes: metadata.len(),
        })
    })
}

/// The checks before anything is offered.
pub(crate) fn status(ctx: &Context, docker: &Tool) -> Status {
    if let Err(reason) = check_local(ctx, docker) {
        return Status::NotOffered(reason);
    }
    let timeout = ctx.tools().query_timeout;
    match tool::run(ctx, docker, &["version"], timeout, &[]) {
        Ok(out) if out.exit == Exit::TimedOut => return Status::NotOffered(SILENT),
        Ok(out) => {
            let text = format!("{}\n{}", out.stdout, out.stderr).to_ascii_lowercase();
            if text.contains("podman") {
                return Status::NotOffered(PODMAN);
            }
        }
        Err(_) => return Status::NotOffered(SILENT),
    }
    match tool::run(ctx, docker, &["info"], timeout, &[]) {
        Ok(out) if out.exit == Exit::Success => {}
        _ => return Status::NotOffered(SILENT),
    }
    let df = ["system", "df", "--format", "json"];
    match tool::run(ctx, docker, &df, timeout, &[]) {
        Ok(out) if out.exit == Exit::Success => match parse_df(&out.stdout) {
            Some(report) => match report.build_cache {
                Some(reclaimable) => Status::BuildCache {
                    reclaimable,
                    volumes: report.volumes,
                },
                None => Status::NotOffered(UNREADABLE),
            },
            None => Status::NotOffered(UNREADABLE),
        },
        Ok(out) if out.exit == Exit::TimedOut => Status::NotOffered(SILENT),
        _ => Status::NotOffered(UNREADABLE),
    }
}

/// `DOCKER_HOST` if set, else the active context's endpoint, must be this
/// computer. Run again right before the prune.
pub(crate) fn check_local(ctx: &Context, docker: &Tool) -> Result<(), &'static str> {
    if let Some(host) = ctx.tool_env("DOCKER_HOST") {
        return if is_local(&host.to_string_lossy()) {
            Ok(())
        } else {
            Err(REMOTE)
        };
    }
    let timeout = ctx.tools().query_timeout;
    let out = match tool::run(ctx, docker, &["context", "inspect"], timeout, &[]) {
        Ok(out) if out.exit == Exit::Success => out,
        Ok(out) if out.exit == Exit::TimedOut => return Err(SILENT),
        _ => return Err(SETTINGS),
    };
    match context_host(&out.stdout) {
        Some(host) if is_local(&host) => Ok(()),
        Some(_) => Err(REMOTE),
        None => Err(SETTINGS),
    }
}

/// A Unix socket, or a named pipe on this computer (`npipe:////./pipe/…`).
pub(crate) fn is_local(host: &str) -> bool {
    let host = host.trim();
    if let Some(path) = host.strip_prefix("unix://") {
        return !path.is_empty();
    }
    if let Some(pipe) = host.strip_prefix("npipe:") {
        let pipe = pipe.replace('\\', "/");
        return pipe.trim_start_matches('/').starts_with("./pipe/");
    }
    false
}

/// `Endpoints.docker.Host` of the first context `docker context inspect`
/// prints.
pub(crate) fn context_host(json: &str) -> Option<String> {
    let value: Value = serde_json::from_str(json.trim()).ok()?;
    let first = match &value {
        Value::Array(items) => items.first()?,
        other => other,
    };
    Some(
        first
            .get("Endpoints")?
            .get("docker")?
            .get("Host")?
            .as_str()?
            .to_owned(),
    )
}

/// The lines of `docker system df` this row reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Df {
    pub build_cache: Option<u64>,
    pub volumes: Option<u64>,
}

/// One JSON object per line (Docker 23 and later), or one array. Unknown
/// shapes are `None`, never zero.
pub(crate) fn parse_df(output: &str) -> Option<Df> {
    let trimmed = output.trim();
    let rows: Vec<Value> = if trimmed.starts_with('[') {
        serde_json::from_str::<Vec<Value>>(trimmed).ok()?
    } else {
        trimmed
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(l).ok())
            .collect::<Option<_>>()?
    };
    let mut df = Df {
        build_cache: None,
        volumes: None,
    };
    for row in &rows {
        let kind = row.get("Type").and_then(Value::as_str).unwrap_or_default();
        let reclaimable = row
            .get("Reclaimable")
            .and_then(Value::as_str)
            .and_then(parse_size);
        match kind {
            "Build Cache" => df.build_cache = Some(reclaimable?),
            "Local Volumes" => df.volumes = reclaimable,
            _ => {}
        }
    }
    (!rows.is_empty()).then_some(df)
}

/// Docker's sizes: `"1.234GB (50%)"`, decimal units (`kB`, `MB`, `GB`)
/// or binary ones (`KiB`, `MiB`).
pub(crate) fn parse_size(text: &str) -> Option<u64> {
    let text = text.split_whitespace().next()?;
    let split = text
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(text.len());
    let (number, unit) = text.split_at(split);
    let number: f64 = number.parse().ok()?;
    let scale: f64 = match unit {
        "B" | "" => 1.0,
        "kB" | "KB" => 1e3,
        "MB" => 1e6,
        "GB" => 1e9,
        "TB" => 1e12,
        "PB" => 1e15,
        "KiB" => 1024.0,
        "MiB" => 1024.0 * 1024.0,
        "GiB" => 1024.0 * 1024.0 * 1024.0,
        "TiB" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => return None,
    };
    let bytes = number * scale;
    (bytes.is_finite() && bytes >= 0.0).then(|| bytes.round() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_verbose_report_reads_strings_as_numbers_and_tolerates_gaps() {
        let lists = parse_verbose(
            r#"{"Images":[{"Repository":"a","Tag":"1","Size":"1.4GB","UniqueSize":"N/A","Containers":"2"}],
                "Volumes":[{"Name":"db","Size":"N/A","Links":"N/A"},{"Name":"x","Size":"0B","Links":0}],
                "Unknown":[1]}"#,
        )
        .unwrap();
        assert_eq!(lists.images[0].size_bytes, Some(1_400_000_000));
        assert_eq!(lists.images[0].unique_bytes, None);
        assert_eq!(lists.images[0].containers, Some(2));
        assert!(lists.containers.is_empty());
        assert_eq!(lists.volumes[0].size_bytes, None);
        assert_eq!(lists.volumes[0].in_use_by, None);
        assert_eq!(lists.volumes[1].size_bytes, Some(0));
        assert_eq!(lists.volumes[1].in_use_by, Some(0));
    }

    #[test]
    fn anything_but_the_verbose_object_is_unreadable() {
        for output in ["", "not json", "[]", "{}", r#"{"Type":"Images"}"#, "42"] {
            assert_eq!(parse_verbose(output), None, "{output}");
        }
    }

    #[test]
    fn only_a_local_socket_or_pipe_is_local() {
        for host in [
            "unix:///var/run/docker.sock",
            "unix:///Users/a/.docker/run/docker.sock",
            "npipe:////./pipe/docker_engine",
            "npipe://./pipe/dockerDesktopLinuxEngine",
        ] {
            assert!(is_local(host), "{host}");
        }
        for host in [
            "tcp://10.0.0.5:2376",
            "ssh://builder@ci.example",
            "npipe:////server/pipe/docker_engine",
            "unix://",
            "",
            "http://localhost:2375",
        ] {
            assert!(!is_local(host), "{host}");
        }
    }

    #[test]
    fn the_context_endpoint_is_read_from_inspect_output() {
        let json = r#"[{"Name":"desktop-linux","Endpoints":{"docker":{"Host":"unix:///Users/a/.docker/run/docker.sock","SkipTLSVerify":false}}}]"#; // hygiene: data only
        assert_eq!(
            context_host(json).as_deref(),
            Some("unix:///Users/a/.docker/run/docker.sock") // hygiene: data only
        );
        assert_eq!(context_host("not json"), None);
        assert_eq!(context_host("[]"), None);
    }

    #[test]
    fn sizes_use_dockers_units() {
        assert_eq!(parse_size("1.5GB (40%)"), Some(1_500_000_000));
        assert_eq!(parse_size("512kB"), Some(512_000));
        assert_eq!(parse_size("0B"), Some(0));
        assert_eq!(parse_size("2MiB"), Some(2 * 1024 * 1024));
        assert_eq!(parse_size("lots"), None);
        assert_eq!(parse_size("3 parsecs"), Some(3));
        assert_eq!(parse_size("3XB"), None);
    }

    #[test]
    fn the_build_cache_and_volume_lines_are_read_and_others_ignored() {
        let lines = concat!(
            r#"{"Active":"2","Reclaimable":"1.2GB (60%)","Size":"2GB","TotalCount":"5","Type":"Images"}"#,
            "\n",
            r#"{"Active":"1","Reclaimable":"3.4GB (90%)","Size":"3.8GB","TotalCount":"4","Type":"Local Volumes"}"#,
            "\n",
            r#"{"Active":"0","Reclaimable":"812.5MB","Size":"812.5MB","TotalCount":"30","Type":"Build Cache"}"#,
        );
        assert_eq!(
            parse_df(lines),
            Some(Df {
                build_cache: Some(812_500_000),
                volumes: Some(3_400_000_000),
            })
        );
    }

    #[test]
    fn an_unreadable_report_is_none_not_zero() {
        assert_eq!(parse_df(""), None);
        assert_eq!(parse_df("TYPE TOTAL ACTIVE SIZE RECLAIMABLE"), None);
        let bad = r#"{"Type":"Build Cache","Reclaimable":"???"}"#;
        assert_eq!(parse_df(bad).and_then(|d| d.build_cache), None);
    }
}
