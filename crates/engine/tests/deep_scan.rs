//! Deep Scan rows on fixture homes (phase 6): package caches, old logs,
//! tool rows with fake tools, Docker with a fake `docker`, the large-file
//! report, and totals that count each file once. Nothing here deletes.
//! Fake tools are shell scripts, so the tests that run one are Unix only.

mod common;

use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

use common::{Home, basic_home};
use space_optimiser_engine::discover::{self, CancelToken, Scan};
use space_optimiser_engine::model::{Action, BuildPlanArgs, Candidate, Risk};
use space_optimiser_engine::policy::{CatalogId, Context};

fn deep(ctx: &Context) -> Scan {
    discover::scan_deep(ctx, &CancelToken::new(), |_| {}).unwrap()
}

fn rows(scan: &Scan, row: CatalogId) -> Vec<&Candidate> {
    scan.caches.iter().filter(|c| c.catalog_id == row).collect()
}

#[cfg(unix)]
fn named<'a>(scan: &'a Scan, name: &str) -> &'a Candidate {
    scan.caches
        .iter()
        .find(|c| c.display_name == name)
        .unwrap_or_else(|| {
            let names: Vec<_> = scan.caches.iter().map(|c| &c.display_name).collect();
            panic!("no candidate {name} in {names:?}")
        })
}

#[cfg(unix)]
fn notes(scan: &Scan, row: CatalogId) -> Vec<&str> {
    scan.notes
        .iter()
        .filter(|n| n.catalog_id == row)
        .map(|n| n.text.as_str())
        .collect()
}

/// A context two months ahead, so fixture logs count as old.
fn logs_old(home: &Home) -> Context {
    home.old_context()
        .with_now(std::time::SystemTime::now() + std::time::Duration::from_secs(60 * 86_400))
}

// Test 1: Cargo's `bin` is not a candidate; the registry cache is, and so
// are git checkouts, although they hold `Cargo.toml` and `.git`.
#[test]
fn cargo_bin_is_never_a_candidate_and_the_download_caches_are() {
    let home = basic_home();
    home.mkdir(".cargo/git/checkouts/fixture-0123456789abcdef/abc1234/.git");
    home.write(
        ".cargo/git/checkouts/fixture-0123456789abcdef/abc1234/.git/HEAD",
        b"ref",
    );
    let scan = deep(&home.old_context());

    let cargo = rows(&scan, CatalogId::PkgCargoCache);
    let paths: Vec<&Path> = cargo.iter().map(|c| c.path()).collect();
    assert_eq!(
        paths,
        [
            home.path(".cargo/registry/cache"),
            home.path(".cargo/git/checkouts"),
        ]
    );
    for c in &cargo {
        assert_eq!(c.action, Action::Delete);
        assert_eq!(c.risk, Risk::Medium);
        assert!(!c.bulk_safe);
    }
    let bin = home.path(".cargo/bin");
    assert!(scan.caches.iter().all(|c| !c.path().starts_with(&bin)));
    assert!(
        scan.caches
            .iter()
            .all(|c| c.path() != home.path(".cargo") && c.path() != home.path(".cargo/env"))
    );
}

#[test]
fn the_gradle_wrapper_is_never_a_candidate() {
    let home = basic_home();
    let scan = deep(&home.old_context());
    let gradle = rows(&scan, CatalogId::PkgGradleCaches);
    assert_eq!(gradle.len(), 1);
    assert_eq!(gradle[0].path(), home.path(".gradle/caches"));
    let wrapper = home.path(".gradle/wrapper");
    assert!(scan.caches.iter().all(|c| !c.path().starts_with(&wrapper)));
}

#[test]
fn the_nuget_download_cache_is_a_candidate() {
    let home = basic_home();
    let scan = deep(&home.old_context());
    let nuget = rows(&scan, CatalogId::PkgNugetHttp);
    let expected = if cfg!(windows) {
        home.path("AppData/Local/NuGet/v3-cache")
    } else {
        home.path(".local/share/NuGet/http-cache")
    };
    assert_eq!(nuget.len(), 1);
    assert_eq!(nuget[0].path(), expected);
}

// Normal Scan stays fast: no package caches, no tools, no home walk.
#[test]
fn normal_scan_has_no_deep_rows() {
    let home = basic_home();
    let scan = discover::scan_normal(&home.old_context(), &CancelToken::new(), |_| {}).unwrap();
    for c in &scan.caches {
        assert!(
            matches!(
                c.catalog_id,
                CatalogId::ThumbnailCache
                    | CatalogId::FontCache
                    | CatalogId::WindowsThumbcache
                    | CatalogId::BrowserHttpCache
            ),
            "{:?}",
            c.catalog_id
        );
    }
}

// Test 8: "Select safe items" can never pick a package cache, Docker, or a
// browser cache: only low-risk rows are bulk-safe.
#[test]
fn only_low_risk_rows_are_bulk_safe_in_deep_scan() {
    let home = basic_home();
    let scan = deep(&logs_old(&home));
    let temp = scan.temp.iter().flat_map(|root| match root {
        discover::RootOutcome::Scanned { candidates, .. } => candidates.as_slice(),
        discover::RootOutcome::Skipped { .. } => &[],
    });
    let all: Vec<&Candidate> = temp.chain(scan.caches.iter()).collect();
    assert!(all.iter().any(|c| c.catalog_id == CatalogId::PkgCargoCache));
    for c in all {
        let safe_row = matches!(
            c.catalog_id,
            CatalogId::UserTemp
                | CatalogId::UserLogsOld
                | CatalogId::ThumbnailCache
                | CatalogId::FontCache
        );
        assert_eq!(
            c.bulk_safe, safe_row,
            "{} {:?}",
            c.display_name, c.catalog_id
        );
    }
}

// Test 5: a big file in Documents is never listed; one in a neutral folder
// is listed for information, with no delete.
#[test]
fn large_files_skip_protected_folders_and_are_report_only() {
    let home = basic_home();
    let big = vec![7u8; 2_000_000];
    home.write("Documents/holiday.mov", &big);
    home.write("Projects/disk-image.iso", &big);
    home.write("Projects/small.txt", b"small");
    let ctx = home.old_context().with_large_file_min(1_000_000);
    let scan = deep(&ctx);

    let large = rows(&scan, CatalogId::LargeFilesReport);
    let names: Vec<&str> = large.iter().map(|c| c.display_name.as_str()).collect();
    assert_eq!(names, ["~/Projects/disk-image.iso"]);
    assert_eq!(large[0].action, Action::Report);
    assert_eq!(large[0].risk, Risk::Info);
    assert!(large[0].estimated_bytes >= 1_000_000);
}

// Test 15: the large-file walk never reads inside `~/Library/Containers` or
// a cloud-synced folder.
#[test]
fn the_large_file_walk_never_reads_private_or_cloud_folders() {
    let home = basic_home();
    let big = vec![7u8; 2_000_000];
    home.write("Library/Containers/com.example.app/Data/big.bin", &big);
    home.write("Dropbox/big.bin", &big);
    home.write("Library/CloudStorage/OneDrive/big.bin", &big);
    home.write("Projects/visible.bin", &big);
    let log = Arc::new(Mutex::new(Vec::new()));
    let ctx = home
        .old_context()
        .with_large_file_min(1_000_000)
        .with_visit_log(log.clone());
    let scan = deep(&ctx);

    let visited = log.lock().unwrap();
    for never in [
        "Library/Containers",
        "Dropbox",
        "Library/CloudStorage",
        "Documents",
    ] {
        let never = home.path(never);
        for path in visited.iter() {
            assert!(!path.starts_with(&never), "read {}", path.display());
        }
    }
    let large = rows(&scan, CatalogId::LargeFilesReport);
    assert_eq!(large.len(), 1);
    assert_eq!(large[0].display_name, "~/Projects/visible.bin");
}

#[test]
fn the_large_file_report_counts_a_hard_linked_file_once() {
    let home = basic_home();
    let file = home.write("Projects/a.bin", &vec![7u8; 2_000_000]);
    fs::hard_link(&file, home.path("Projects/b.bin")).unwrap();
    let ctx = home.old_context().with_large_file_min(1_000_000);
    let scan = deep(&ctx);
    assert_eq!(rows(&scan, CatalogId::LargeFilesReport).len(), 1);
}

// Test 9: when a folder deleted whole holds another row's folder, the plan
// counts only the outer one.
#[test]
fn an_overlapping_parent_and_child_count_once() {
    let home = basic_home();
    let cargo = home.path(".cargo");
    let engine = space_optimiser_engine::Engine::new(home.config_with(move |ctx| {
        ctx.with_temp_root(
            cargo.clone(),
            space_optimiser_engine::policy::OwnerRule::Root,
        )
    }));
    let view = common::deep_scan(&engine);

    let parent = common::named(&view, "registry");
    assert_eq!(parent.catalog_id, CatalogId::UserTemp);
    let child = common::named(&view, "~/.cargo/registry/cache");
    assert_eq!(child.catalog_id, CatalogId::PkgCargoCache);
    assert!(parent.covers.contains(&child.id));
    assert!(!child.covers.contains(&parent.id));

    let plan = engine
        .build_plan(BuildPlanArgs {
            scan_id: view.scan_id,
            candidate_ids: vec![child.id, parent.id],
        })
        .unwrap();
    assert_eq!(plan.items.len(), 1);
    assert_eq!(plan.items[0].candidate_id, parent.id);
    assert_eq!(plan.estimated_bytes, parent.estimated_bytes);
}

#[cfg(target_os = "macos")]
mod logs {
    use super::*;

    #[test]
    fn old_logs_are_bulk_safe_and_other_files_are_left_alone() {
        let home = basic_home();
        let scan = deep(&logs_old(&home));

        let old = named(&scan, "old.log");
        assert_eq!(old.catalog_id, CatalogId::UserLogsOld);
        assert!(old.bulk_safe);
        assert_eq!(named(&scan, "FixtureApp").file_count, 3);
        let skipped: Vec<_> = scan
            .cache_skipped
            .iter()
            .map(|s| (s.display_name.as_str(), s.reason))
            .collect();
        assert!(skipped.contains(&("settings.plist", discover::SkipReason::NotALog)));
        assert!(skipped.contains(&("Mixed", discover::SkipReason::NotALog)));
    }

    #[test]
    fn logs_newer_than_30_days_are_not_candidates() {
        let home = basic_home();
        let scan = deep(&home.old_context());
        assert!(rows(&scan, CatalogId::UserLogsOld).is_empty());
    }

    #[test]
    fn crash_reports_are_never_read() {
        let home = basic_home();
        let log = Arc::new(Mutex::new(Vec::new()));
        let scan = deep(&logs_old(&home).with_visit_log(log.clone()));
        let reports = home.path("Library/Logs/DiagnosticReports");
        for path in log.lock().unwrap().iter() {
            assert!(!path.starts_with(&reports), "read {}", path.display());
        }
        assert!(!rows(&scan, CatalogId::UserLogsOld).is_empty());
    }

    // Test 4: two hard links to one log, both inside the entry, count once.
    #[test]
    fn two_hard_linked_logs_count_once() {
        let home = basic_home();
        let a = home.write("Library/Logs/Linked/a.log", &vec![1u8; 64 * 1024]);
        fs::hard_link(&a, home.path("Library/Logs/Linked/b.log")).unwrap();
        let single = {
            use std::os::unix::fs::MetadataExt;
            fs::metadata(&a).unwrap().blocks() * 512
        };
        let scan = deep(&logs_old(&home));
        let linked = named(&scan, "Linked");
        assert_eq!(linked.file_count, 2);
        assert_eq!(
            linked.estimated_bytes,
            single + dir_blocks(&home.path("Library/Logs/Linked"))
        );
        assert!(linked.labels.is_empty(), "{:?}", linked.labels);
    }

    fn dir_blocks(path: &Path) -> u64 {
        use std::os::unix::fs::MetadataExt;
        fs::metadata(path).unwrap().blocks() * 512
    }
}

#[cfg(unix)]
mod tools {
    use super::*;
    use space_optimiser_engine::model::{Confidence, Label};

    const NPM: &str = r#"case "$1 $2" in
  "config get") echo "$HOME/.npm" ;;
  *) exit 2 ;;
esac"#;

    // Test 2: `_npx` is its own candidate even when the npm cache is one,
    // and the npm upper bound leaves `_npx` out.
    #[test]
    fn npx_is_its_own_candidate_next_to_the_npm_cache() {
        let home = basic_home();
        home.fake_tool("npm", NPM);
        let scan = deep(&home.with_fake_tools(home.old_context()));

        let npx = named(&scan, "~/.npm/_npx");
        assert_eq!(npx.catalog_id, CatalogId::PkgNpx);
        assert_eq!(npx.action, Action::Delete);
        let npm = named(&scan, "~/.npm");
        assert_eq!(npm.catalog_id, CatalogId::PkgNpmCache);
        assert_eq!(npm.action, Action::RunTool);
        assert_eq!(npm.estimated_bytes, 0);
        assert_eq!(npm.file_count, 2, "only the two _cacache files");
        assert!(npm.upper_bound_bytes.unwrap() > 0);
        assert_eq!(
            npm.found_by.as_deref(),
            Some("Folder from `npm config get cache`; npm found on PATH.")
        );
    }

    // Test 3: pnpm's action is `store prune`; the store's size is an upper
    // bound and is never in the delete total.
    #[test]
    fn pnpm_prunes_and_its_store_size_is_only_an_upper_bound() {
        let home = basic_home();
        let store = home.path("pnpm-store/v3");
        home.write("pnpm-store/v3/files/00/abc", b"package file");
        home.fake_tool(
            "pnpm",
            &format!("[ \"$1 $2\" = 'store path' ] && echo '{}'", store.display()),
        );
        let engine = home.tool_engine();
        let view = common::deep_scan(&engine);
        let pnpm = common::named(&view, "~/pnpm-store/v3");
        assert_eq!(pnpm.action, Action::RunTool);
        assert_eq!(pnpm.estimated_bytes, 0);
        let upper = pnpm.upper_bound_bytes.unwrap();
        assert!(upper > 0);

        let plan = engine
            .build_plan(BuildPlanArgs {
                scan_id: view.scan_id,
                candidate_ids: vec![pnpm.id],
            })
            .unwrap();
        assert_eq!(plan.estimated_bytes, 0);
        assert_eq!(plan.tool_upper_bound_bytes, upper);
        assert_eq!(
            plan.items[0].action_text,
            "Run the tool's own clean command."
        );
    }

    #[test]
    fn a_tool_cache_outside_the_home_is_left_alone() {
        let home = basic_home();
        let outside = home.root.parent().unwrap().join("elsewhere");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("wheel.whl"), b"wheel").unwrap();
        home.fake_tool("pip", &format!("echo '{}'", outside.display()));
        let scan = deep(&home.with_fake_tools(home.old_context()));
        assert!(rows(&scan, CatalogId::PkgPipCache).is_empty());
        assert!(
            scan.cache_skipped
                .iter()
                .any(|s| s.reason == discover::SkipReason::OutsideHome)
        );
    }

    #[test]
    fn a_tool_cache_in_a_protected_folder_is_left_alone() {
        let home = basic_home();
        home.write("Documents/uv-cache/archive", b"mine");
        let cache = home.path("Documents/uv-cache");
        home.fake_tool("uv", &format!("echo '{}'", cache.display()));
        let scan = deep(&home.with_fake_tools(home.old_context()));
        assert!(rows(&scan, CatalogId::PkgUvCache).is_empty());
        assert!(
            scan.cache_skipped
                .iter()
                .any(|s| s.reason == discover::SkipReason::ProtectedFolder)
        );
    }

    // Phase 6 item 2: without the tool, the default folder is shown but
    // nothing is offered.
    #[test]
    fn without_the_tool_a_default_cache_is_shown_but_not_offered() {
        let home = basic_home();
        let default = if cfg!(target_os = "macos") {
            "Library/Caches/pip/http/fixture"
        } else {
            ".cache/pip/http/fixture"
        };
        home.write(default, b"wheel");
        let scan = deep(&home.with_fake_tools(home.old_context()));
        let pip = rows(&scan, CatalogId::PkgPipCache);
        assert_eq!(pip.len(), 1);
        assert_eq!(pip[0].action, Action::None);
        assert_eq!(pip[0].confidence, Confidence::Uncertain);
        assert!(
            pip[0]
                .found_by
                .as_deref()
                .unwrap()
                .starts_with("Default folder; pip was not found")
        );
        assert!(!pip[0].labels.contains(&Label::QuitFirst));
    }

    // Test 13: a tool missing from PATH but in a listed install folder is
    // found, and the row says where.
    #[test]
    fn a_tool_in_an_install_folder_is_found_and_the_row_says_where() {
        let home = basic_home();
        home.write(".cache/uv/archive-v0/fixture", b"wheel");
        let local_bin = home.mkdir(".local/bin");
        let uv = home.fake_tool("uv", "echo \"$HOME/.cache/uv\"");
        fs::rename(&uv, local_bin.join("uv")).unwrap();
        let ctx = home
            .with_fake_tools(home.old_context())
            .with_tool_dirs(vec![home.tool_dir()], vec![local_bin]);
        let scan = deep(&ctx);
        let uv = named(&scan, "~/.cache/uv");
        assert_eq!(uv.action, Action::RunTool);
        assert_eq!(
            uv.found_by.as_deref(),
            Some("Folder from `uv cache dir`; uv found in ~/.local/bin.")
        );
    }

    // Test 12: a pnpm that is a Corepack shim, or one that starts a Corepack
    // download, is a failure with no action.
    #[test]
    fn a_corepack_pnpm_is_never_offered() {
        let home = basic_home();
        home.write("pnpm-store/v3/files/00/abc", b"package file");
        let shims = home
            .root
            .parent()
            .unwrap()
            .join("lib/node_modules/corepack/shims");
        fs::create_dir_all(&shims).unwrap();
        let shim = home.fake_tool("pnpm", "echo should-not-run");
        fs::rename(&shim, shims.join("pnpm")).unwrap();
        std::os::unix::fs::symlink(shims.join("pnpm"), home.tool_dir().join("pnpm")).unwrap();
        let scan = deep(&home.with_fake_tools(home.old_context()));
        assert!(
            rows(&scan, CatalogId::PkgPnpmStore)
                .iter()
                .all(|c| c.action != Action::RunTool)
        );
        assert!(notes(&scan, CatalogId::PkgPnpmStore)[0].contains("Corepack"));
    }

    #[test]
    fn a_pnpm_that_starts_a_download_is_stopped_and_not_offered() {
        let home = basic_home();
        home.fake_tool(
            "pnpm",
            "echo '! Corepack is about to download https://registry.npmjs.org/pnpm' >&2; sleep 30",
        );
        let started = std::time::Instant::now();
        let scan = deep(&home.with_fake_tools(home.old_context()));
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
        assert!(
            rows(&scan, CatalogId::PkgPnpmStore)
                .iter()
                .all(|c| c.action != Action::RunTool)
        );
        assert!(!notes(&scan, CatalogId::PkgPnpmStore).is_empty());
    }

    #[test]
    fn yarn_2_is_not_offered() {
        let home = basic_home();
        home.write(".cache/yarn/v6/fixture", b"package");
        home.fake_tool(
            "yarn",
            r#"case "$1" in --version) echo 4.1.0 ;; *) echo "$HOME/.cache/yarn" ;; esac"#,
        );
        let scan = deep(&home.with_fake_tools(home.old_context()));
        assert!(rows(&scan, CatalogId::PkgYarnCache).is_empty());
        assert!(notes(&scan, CatalogId::PkgYarnCache)[0].contains("Yarn 2"));
    }

    #[test]
    fn yarn_1_cleans_its_cache() {
        let home = basic_home();
        home.write(".cache/yarn/v6/fixture", b"package");
        home.fake_tool(
            "yarn",
            r#"case "$1" in --version) echo 1.22.22 ;; *) echo "$HOME/.cache/yarn" ;; esac"#,
        );
        let scan = deep(&home.with_fake_tools(home.old_context()));
        assert_eq!(named(&scan, "~/.cache/yarn").action, Action::RunTool);
    }

    const DOCKER_LOCAL: &str = r#"case "$1" in
  context) echo '[{"Name":"default","Endpoints":{"docker":{"Host":"unix:///run/docker.sock"}}}]' ;;
  version) echo 'Client: Docker Engine - Community' ;;
  info) echo 'Server: ok' ;;
  system) echo '{"Type":"Images","Reclaimable":"1GB (50%)"}'
          echo '{"Type":"Local Volumes","Reclaimable":"2GB (40%)"}'
          echo '{"Type":"Build Cache","Reclaimable":"1.5GB"}' ;;
  *) exit 2 ;;
esac"#;

    // Test 7: the volume number never becomes a candidate; the build cache
    // is a tool row with Docker's own number.
    #[test]
    fn docker_offers_the_build_cache_and_never_volumes() {
        let home = basic_home();
        home.fake_tool("docker", DOCKER_LOCAL);
        let scan = deep(&home.with_fake_tools(home.old_context()));

        let docker = rows(&scan, CatalogId::DockerBuildCache);
        assert_eq!(docker.len(), 1);
        assert_eq!(docker[0].action, Action::RunTool);
        assert_eq!(docker[0].estimated_bytes, 0);
        assert_eq!(docker[0].upper_bound_bytes, Some(1_500_000_000));
        let volumes = scan
            .notes
            .iter()
            .find(|n| n.text.contains("volumes"))
            .unwrap();
        assert_eq!(volumes.bytes, Some(2_000_000_000));
        assert!(
            scan.caches
                .iter()
                .all(|c| c.upper_bound_bytes != Some(2_000_000_000))
        );
    }

    // Test 6: a Docker that hangs is "did not respond" and the scan ends.
    #[test]
    fn a_silent_docker_does_not_hang_the_scan() {
        let home = basic_home();
        home.fake_tool(
            "docker",
            &DOCKER_LOCAL.replace(
                "version) echo 'Client: Docker Engine - Community' ;;",
                "version) sleep 30 ;;",
            ),
        );
        let ctx = home.with_fake_tools(home.old_context()).with_tool_timeouts(
            std::time::Duration::from_secs(3),
            std::time::Duration::from_secs(10),
        );
        let started = std::time::Instant::now();
        let scan = deep(&ctx);
        assert!(started.elapsed() < std::time::Duration::from_secs(20));
        assert!(rows(&scan, CatalogId::DockerBuildCache).is_empty());
        assert_eq!(
            notes(&scan, CatalogId::DockerBuildCache),
            ["Docker did not respond."]
        );
    }

    // Test 10: a context that points at another machine offers nothing.
    #[test]
    fn docker_pointing_at_another_machine_is_not_offered() {
        let home = basic_home();
        home.fake_tool(
            "docker",
            &DOCKER_LOCAL.replace("unix:///run/docker.sock", "tcp://10.0.0.5:2376"),
        );
        let scan = deep(&home.with_fake_tools(home.old_context()));
        assert!(rows(&scan, CatalogId::DockerBuildCache).is_empty());
        assert!(notes(&scan, CatalogId::DockerBuildCache)[0].contains("another machine"));
    }

    #[test]
    fn a_remote_docker_host_variable_wins_over_a_local_context() {
        let home = basic_home();
        home.fake_tool("docker", DOCKER_LOCAL);
        let ctx = home
            .with_fake_tools(home.old_context())
            .with_tool_env("DOCKER_HOST", "ssh://builder@ci.example");
        let scan = deep(&ctx);
        assert!(rows(&scan, CatalogId::DockerBuildCache).is_empty());
        assert!(notes(&scan, CatalogId::DockerBuildCache)[0].contains("another machine"));
    }

    // Test 11: Podman answering as `docker` is skipped with the reason.
    #[test]
    fn podman_is_skipped_with_a_reason() {
        let home = basic_home();
        home.fake_tool(
            "docker",
            &DOCKER_LOCAL.replace("Client: Docker Engine - Community", "Client: Podman Engine"),
        );
        let scan = deep(&home.with_fake_tools(home.old_context()));
        assert!(rows(&scan, CatalogId::DockerBuildCache).is_empty());
        assert!(notes(&scan, CatalogId::DockerBuildCache)[0].contains("Podman"));
    }

    #[test]
    fn an_unreadable_docker_report_is_a_note_not_zero() {
        let home = basic_home();
        home.fake_tool(
            "docker",
            &DOCKER_LOCAL.replace(
                "echo '{\"Type\":\"Build Cache\",\"Reclaimable\":\"1.5GB\"}'",
                "echo 'TYPE TOTAL ACTIVE SIZE RECLAIMABLE'",
            ),
        );
        let scan = deep(&home.with_fake_tools(home.old_context()));
        assert!(rows(&scan, CatalogId::DockerBuildCache).is_empty());
        assert_eq!(
            notes(&scan, CatalogId::DockerBuildCache),
            ["Could not read Docker's report."]
        );
    }

    #[test]
    fn a_tool_never_runs_when_scanning_in_normal_mode() {
        let home = basic_home();
        let ran = home.root.parent().unwrap().join("ran");
        home.fake_tool("npm", &format!("touch '{}'", ran.display()));
        home.fake_tool("docker", &format!("touch '{}'", ran.display()));
        let ctx = home.with_fake_tools(home.old_context());
        discover::scan_normal(&ctx, &CancelToken::new(), |_| {}).unwrap();
        assert!(!ran.exists());
    }
}
