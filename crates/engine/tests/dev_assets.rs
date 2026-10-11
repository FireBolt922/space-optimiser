//! Developer assets (v0.3 usability round): Docker's images, containers,
//! and volumes, and the size of Docker's disk file, for information only.
//! Nothing here has a candidate id, so none of it can be planned. Fake
//! `docker` scripts make the Docker tests Unix only.

mod common;

use space_optimiser_engine::discover::{self, CancelToken, Scan};
use space_optimiser_engine::model::ScanMode;

fn deep(ctx: &space_optimiser_engine::policy::Context) -> Scan {
    discover::scan(ctx, ScanMode::Deep, &CancelToken::new(), |_| {}).unwrap()
}

/// The Docker Desktop disk file, relative to the home, on this OS.
fn disk_file() -> &'static str {
    if cfg!(target_os = "macos") {
        "Library/Containers/com.docker.docker/Data/vms/0/data/Docker.raw"
    } else if cfg!(windows) {
        "AppData/Local/Docker/wsl/disk/docker_data.vhdx"
    } else {
        ".docker/desktop/vms/0/data/Docker.raw"
    }
}

#[test]
fn a_normal_scan_has_no_developer_assets() {
    let home = common::basic_home();
    let scan = discover::scan(
        &home.old_context(),
        ScanMode::Normal,
        &CancelToken::new(),
        |_| {},
    )
    .unwrap();
    assert!(scan.dev_assets.is_none());
}

#[test]
fn without_docker_or_its_disk_file_there_are_no_docker_assets() {
    let home = common::basic_home();
    let scan = deep(&home.with_fake_tools(home.old_context()));
    assert!(scan.dev_assets.unwrap().docker.is_none());
}

#[test]
fn the_disk_file_is_measured_from_its_fixed_path_without_reading_its_folders() {
    let home = common::basic_home();
    let file = home.write(disk_file(), &vec![7u8; 64 * 1024]);
    let visits = space_optimiser_engine::policy::VisitLog::default();
    let ctx = home
        .with_fake_tools(home.old_context())
        .with_visit_log(visits.clone());
    let scan = deep(&ctx);

    let docker = scan
        .dev_assets
        .unwrap()
        .docker
        .expect("the disk file is found");
    let disk = docker.disk_file.expect("the disk file is measured");
    assert_eq!(disk.logical_bytes, 64 * 1024);
    assert!(disk.allocated_bytes > 0);
    assert_eq!(disk.location_label, format!("~/{}", disk_file()));
    // On macOS the file sits in a private folder that is never listed.
    let private = home.path("Library/Containers");
    let visited = visits.lock().unwrap();
    assert!(
        visited.iter().all(|p| !p.starts_with(&private)),
        "a private folder was read: {visited:?}"
    );
    drop(file);
}

#[cfg(unix)]
#[test]
fn a_link_in_place_of_the_disk_file_is_not_measured() {
    let home = common::basic_home();
    let target = home.write("big.bin", &vec![1u8; 4096]);
    let link = home.path(disk_file());
    std::fs::create_dir_all(link.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let scan = deep(&home.with_fake_tools(home.old_context()));
    let docker = scan.dev_assets.unwrap().docker;
    assert!(docker.is_none_or(|d| d.disk_file.is_none()));
}

#[cfg(unix)]
mod docker {
    use super::*;
    use std::time::{Duration, Instant};

    fn fixture() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/docker/system-df-verbose.json")
            .canonicalize()
            .unwrap()
    }

    /// A local Docker whose verbose report is the fixture (or `verbose`),
    /// logging each call's arguments.
    fn docker(verbose: &str) -> String {
        format!(
            r#"echo "$@" >> "$HOME/../docker-args"
case "$1" in
  context) echo '[{{"Name":"default","Endpoints":{{"docker":{{"Host":"unix:///run/docker.sock"}}}}}}]' ;;
  version) echo 'Client: Docker Engine - Community' ;;
  info) echo 'Server: ok' ;;
  system)
    if [ "$3" = "-v" ]; then {verbose}
    else
      echo '{{"Type":"Local Volumes","Reclaimable":"482MB (3%)"}}'
      echo '{{"Type":"Build Cache","Reclaimable":"1.5GB"}}'
    fi ;;
  *) exit 2 ;;
esac"#
        )
    }

    fn calls(home: &common::Home) -> String {
        std::fs::read_to_string(home.root.parent().unwrap().join("docker-args")).unwrap_or_default()
    }

    #[test]
    fn images_containers_and_volumes_are_listed_for_information_only() {
        let home = common::basic_home();
        home.fake_tool("docker", &docker(&format!("cat '{}'", fixture().display())));
        let scan = deep(&home.with_fake_tools(home.old_context()));
        assert!(calls(&home).contains("system df -v --format json"));

        let docker = scan.dev_assets.unwrap().docker.unwrap();
        assert_eq!(docker.note, None);
        assert_eq!(docker.images.len(), 3);
        let backend = &docker.images[0];
        assert_eq!(backend.repository, "example/backend");
        assert_eq!(backend.tag, "latest");
        assert_eq!(backend.size_bytes, Some(1_400_000_000));
        assert_eq!(backend.unique_bytes, Some(1_291_000_000));
        assert_eq!(backend.containers, Some(1));

        assert_eq!(docker.containers.len(), 2);
        let running = &docker.containers[0];
        assert_eq!(running.name, "example-backend-1");
        assert_eq!(running.image, "example/backend:latest");
        assert_eq!(running.state, "running");
        assert_eq!(running.size_bytes, Some(57_300));

        assert_eq!(docker.volumes.len(), 3);
        let db = &docker.volumes[0];
        assert_eq!(db.name, "example_db");
        assert_eq!(db.size_bytes, Some(14_200_000_000));
        assert_eq!(db.in_use_by, Some(1));
        assert_eq!(docker.volumes[1].in_use_by, Some(0));

        // Only the build cache is a candidate; no listed size became one.
        for c in &scan.caches {
            assert_ne!(c.upper_bound_bytes, Some(14_200_000_000));
            assert_ne!(c.estimated_bytes, 14_200_000_000);
        }
        let json = serde_json::to_value(&docker).unwrap();
        assert!(json["volumes"][0].get("id").is_none());
        assert!(json["volumes"][0].get("path").is_none());
    }

    #[test]
    fn an_unreadable_detail_report_is_a_note_and_the_build_cache_stays() {
        let home = common::basic_home();
        home.fake_tool("docker", &docker("echo 'not json'"));
        let scan = deep(&home.with_fake_tools(home.old_context()));
        let docker = scan.dev_assets.unwrap().docker.unwrap();
        assert_eq!(
            docker.note.as_deref(),
            Some("Could not read Docker's list of images, containers, and volumes.")
        );
        assert!(docker.images.is_empty() && docker.volumes.is_empty());
        assert!(
            scan.caches
                .iter()
                .any(|c| c.display_name == "Docker build cache")
        );
    }

    #[test]
    fn a_slow_detail_report_becomes_a_note_and_the_scan_finishes() {
        let home = common::basic_home();
        home.fake_tool("docker", &docker("sleep 30"));
        let ctx = home
            .with_fake_tools(home.old_context())
            .with_detail_timeout(Duration::from_secs(2));
        let started = Instant::now();
        let scan = deep(&ctx);
        assert!(started.elapsed() < Duration::from_secs(15));
        let docker = scan.dev_assets.unwrap().docker.unwrap();
        assert_eq!(
            docker.note.as_deref(),
            Some("Docker took too long to list its images, containers, and volumes.")
        );
    }

    #[test]
    fn cancel_stops_waiting_for_the_detail_report() {
        let home = common::basic_home();
        home.fake_tool("docker", &docker("sleep 30"));
        let ctx = home
            .with_fake_tools(home.old_context())
            .with_detail_timeout(Duration::from_secs(30));
        let cancel = CancelToken::new();
        let waiting = cancel.clone();
        let started = Instant::now();
        let scan = std::thread::spawn(move || {
            discover::scan(&ctx, ScanMode::Deep, &waiting, |p| {
                if p.stage == discover::Stage::Docker {
                    let waiting = waiting.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_secs(4));
                        waiting.cancel();
                    });
                }
            })
            .map(|_| ())
        });
        let result = scan.join().unwrap();
        assert_eq!(result, Err(discover::ScanError::Cancelled));
        assert!(started.elapsed() < Duration::from_secs(20));
        drop(cancel);
    }

    #[test]
    fn a_remote_docker_is_never_asked_for_details() {
        let home = common::basic_home();
        home.fake_tool(
            "docker",
            &docker(&format!("cat '{}'", fixture().display()))
                .replace("unix:///run/docker.sock", "tcp://10.0.0.5:2376"),
        );
        let scan = deep(&home.with_fake_tools(home.old_context()));
        assert!(!calls(&home).contains("-v"));
        assert!(scan.dev_assets.unwrap().docker.is_none());
    }
}
