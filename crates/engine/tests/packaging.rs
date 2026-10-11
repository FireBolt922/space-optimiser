//! Phase 12: the bundle configuration keeps the release rules. One version
//! source, hardened runtime without entitlements, the WebKitGTK package the
//! `.deb` needs, the Inter licence shipped, and a CSP with no remote source.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

fn desktop() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/desktop")
}

fn tauri_conf() -> Value {
    let text = fs::read_to_string(desktop().join("src-tauri/tauri.conf.json")).unwrap();
    serde_json::from_str(&text).unwrap()
}

#[test]
fn the_version_has_one_source() {
    let conf = tauri_conf();
    // Without a version, Tauri uses the shell crate's, which is the
    // workspace version the engine reports.
    assert!(
        conf.get("version").is_none(),
        "tauri.conf.json must not set its own version"
    );
    let package: Value =
        serde_json::from_str(&fs::read_to_string(desktop().join("package.json")).unwrap()).unwrap();
    assert_eq!(package["version"], env!("CARGO_PKG_VERSION"));
}

#[test]
fn the_identifier_is_unchanged() {
    assert_eq!(tauri_conf()["identifier"], "com.spaceoptimiser.desktop");
}

#[test]
fn bundles_are_built_for_each_os() {
    let bundle = &tauri_conf()["bundle"];
    assert_eq!(bundle["active"], true);
    let targets: Vec<&str> = bundle["targets"]
        .as_array()
        .expect("bundle targets are listed")
        .iter()
        .map(|t| t.as_str().unwrap())
        .collect();
    for target in ["app", "dmg", "deb", "nsis", "msi"] {
        assert!(targets.contains(&target), "{target} is not bundled");
    }
    // No updater: the first release does not update itself.
    assert!(!targets.contains(&"updater"));
    assert_ne!(bundle["createUpdaterArtifacts"], true);
    assert!(tauri_conf()["plugins"].get("updater").is_none());
}

#[test]
fn macos_uses_the_hardened_runtime_without_entitlements() {
    let macos = &tauri_conf()["bundle"]["macOS"];
    assert_eq!(macos["minimumSystemVersion"], "14.0");
    assert_eq!(macos["hardenedRuntime"], true);
    // No App Sandbox, and no entitlement that would widen the runtime.
    assert!(macos.get("entitlements").is_none_or(Value::is_null));
    // Signing identities come from the release machine, never the repo.
    assert!(macos.get("signingIdentity").is_none_or(Value::is_null));
}

#[test]
fn the_deb_depends_on_webkitgtk() {
    let conf = tauri_conf();
    let depends: Vec<&str> = conf["bundle"]["linux"]["deb"]["depends"]
        .as_array()
        .expect("deb dependencies are listed")
        .iter()
        .map(|d| d.as_str().unwrap())
        .collect();
    assert!(depends.contains(&"libwebkit2gtk-4.1-0"), "{depends:?}");
}

#[test]
fn windows_uses_the_webview2_bootstrapper_and_no_signing_secrets() {
    let windows = &tauri_conf()["bundle"]["windows"];
    assert_eq!(
        windows["webviewInstallMode"]["type"],
        "downloadBootstrapper"
    );
    assert!(
        windows
            .get("certificateThumbprint")
            .is_none_or(Value::is_null)
    );
    assert!(windows.get("signCommand").is_none_or(Value::is_null));
}

#[test]
fn the_inter_licence_ships_with_the_app() {
    let resources = &tauri_conf()["bundle"]["resources"];
    let source = "../src/assets/fonts/OFL.txt";
    assert_eq!(resources[source], "licences/Inter-OFL-1.1.txt");
    let text = fs::read_to_string(desktop().join("src-tauri").join(source)).unwrap();
    assert!(text.contains("The Inter Project Authors"));
    assert!(text.contains("SIL OPEN FONT LICENSE Version 1.1 - 26 February 2007"));
    assert!(text.contains("THE FONT SOFTWARE IS PROVIDED \"AS IS\""));
}

/// The sources a CSP directive allows, for every directive.
fn csp_sources(csp: &str) -> Vec<&str> {
    csp.split(';')
        .flat_map(|directive| directive.split_whitespace().skip(1))
        .collect()
}

#[test]
fn the_csp_allows_no_remote_source() {
    let security = &tauri_conf()["app"]["security"];
    let csp = security["csp"].as_str().expect("a release CSP is set");
    for source in csp_sources(csp) {
        let lower = source.to_ascii_lowercase();
        assert_ne!(lower, "*", "wildcard source");
        assert!(!lower.contains("unsafe-inline"), "{source}");
        assert!(!lower.contains("unsafe-eval"), "{source}");
        if lower.starts_with("http:") || lower.starts_with("https:") {
            // Tauri's own IPC endpoint on Windows; not a network host.
            assert_eq!(lower, "http://ipc.localhost", "remote source {source}");
        }
        assert!(
            !lower.starts_with("ws:") && !lower.starts_with("wss:"),
            "{source}"
        );
    }
    // The dev CSP may reach the local dev server, nothing remote.
    if let Some(dev) = security["devCsp"].as_str() {
        for source in csp_sources(dev) {
            let lower = source.to_ascii_lowercase();
            if lower.contains("://") {
                let host = lower.split("://").nth(1).unwrap();
                assert!(
                    host.starts_with("localhost") || host.starts_with("ipc.localhost"),
                    "remote dev source {source}"
                );
            }
        }
    }
    assert_eq!(tauri_conf()["app"]["withGlobalTauri"], false);
}
