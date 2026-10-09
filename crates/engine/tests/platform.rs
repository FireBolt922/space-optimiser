//! Protected folders and temp-root validation (phase 4 items 3 and 4).

mod common;

use std::path::Path;

use space_optimiser_engine::platform;
use space_optimiser_engine::policy::{Context, OwnerRule, RootRejection, validate_temp_root};

fn rejection(ctx: &Context, root: &Path) -> RootRejection {
    validate_temp_root(ctx, root, OwnerRule::Root).unwrap_err()
}

#[test]
fn documents_and_its_contents_are_protected() {
    let home = common::basic_home();
    let ctx = Context::for_fixture(&home.root);
    assert!(ctx.is_protected(&home.path("Documents")));
    assert!(ctx.is_protected(&home.path("Documents/notes.txt")));
    assert!(!ctx.is_protected(&home.path("tmp/old-download.bin")));
}

#[test]
fn the_fixture_temp_folder_is_a_valid_root() {
    let home = common::basic_home();
    let ctx = Context::for_fixture(&home.root);
    let root = validate_temp_root(&ctx, &home.path("tmp"), OwnerRule::Root).unwrap();
    assert_eq!(root.path(), home.path("tmp"));
}

// Test 9: a temp variable pointing at the home is rejected, not trusted.
#[test]
fn a_temp_root_at_the_home_is_rejected() {
    let home = common::basic_home();
    let ctx = Context::for_fixture(&home.root);
    assert_eq!(rejection(&ctx, &home.root), RootRejection::HomeOrAncestor);
    assert_eq!(
        rejection(&ctx, home.root.parent().unwrap()),
        RootRejection::HomeOrAncestor
    );
}

// Test 10: `/`, a drive root, or an ancestor of a protected folder.
#[test]
fn a_filesystem_root_is_rejected() {
    let home = common::basic_home();
    let ctx = Context::for_fixture(&home.root);
    let root = home.root.ancestors().last().unwrap();
    assert_eq!(rejection(&ctx, root), RootRejection::FilesystemRoot);
}

#[test]
fn an_ancestor_of_a_protected_folder_is_rejected() {
    let home = common::basic_home();
    home.mkdir(".cache/vault");
    let ctx = Context::for_fixture(&home.root).with_protected(home.path(".cache/vault"));
    assert_eq!(
        rejection(&ctx, &home.path(".cache")),
        RootRejection::ContainsProtectedFolder
    );
}

#[test]
fn a_root_inside_a_protected_folder_is_rejected() {
    let home = common::basic_home();
    let inside = home.mkdir("Documents/tmp");
    let ctx = Context::for_fixture(&home.root);
    assert_eq!(
        rejection(&ctx, &inside),
        RootRejection::InsideProtectedFolder
    );
    assert_eq!(
        rejection(&ctx, &home.path("Documents")),
        RootRejection::InsideProtectedFolder
    );
}

#[test]
fn a_missing_root_is_rejected() {
    let home = common::basic_home();
    let ctx = Context::for_fixture(&home.root);
    assert_eq!(
        rejection(&ctx, &home.path("no-such-dir")),
        RootRejection::Missing
    );
    assert_eq!(
        rejection(&ctx, &home.path("tmp/old-download.bin")),
        RootRejection::NotADirectory
    );
}

#[test]
fn a_root_outside_the_required_area_is_rejected() {
    let home = common::basic_home();
    let ctx = Context::for_fixture(&home.root).with_required_temp_area(home.path(".cache"));
    assert_eq!(
        rejection(&ctx, &home.path("tmp")),
        RootRejection::OutsideSystemTempArea
    );
}

// Test 15: `~/documents` on a case-insensitive volume is the same folder.
#[test]
fn a_lowercase_documents_path_is_protected_on_a_case_insensitive_volume() {
    let home = common::basic_home();
    let lower = home.path("documents");
    if !lower.exists() {
        eprintln!("skipped: the test volume is case-sensitive");
        return;
    }
    let ctx = Context::for_fixture(&home.root);
    assert!(ctx.is_protected(&lower));
    assert!(ctx.is_protected(&lower.join("NOTES.TXT")));
    assert_eq!(
        rejection(&ctx, &lower),
        RootRejection::InsideProtectedFolder
    );
}

// Test 16: localized XDG folders are protected.
#[test]
fn a_localized_xdg_documents_folder_is_protected() {
    let home = common::basic_home();
    home.mkdir("Dokumente");
    home.mkdir("Bilder/Urlaub");
    home.write(
        ".config/user-dirs.dirs",
        b"# written by xdg-user-dirs-update\nXDG_DOCUMENTS_DIR=\"$HOME/Dokumente\"\nXDG_PICTURES_DIR=\"$HOME/Bilder\"\nXDG_DESKTOP_DIR=\"$HOME/\"\n",
    );
    let ctx = Context::for_fixture(&home.root);
    assert!(ctx.is_protected(&home.path("Dokumente")));
    assert!(ctx.is_protected(&home.path("Bilder/Urlaub")));
    // `XDG_DESKTOP_DIR="$HOME/"` means "no desktop folder", not "protect home".
    assert!(!ctx.is_protected(&home.path("tmp")));
}

#[test]
fn user_dirs_parsing_follows_the_xdg_format() {
    let home = Path::new("/home/ana"); // hygiene: data only
    let parsed = platform::parse_user_dirs(
        "# comment\nXDG_DOCUMENTS_DIR=\"$HOME/Dokumente\"\nXDG_MUSIC_DIR=\"/srv/musik\"\nXDG_DESKTOP_DIR=\"$HOME/\"\nXDG_VIDEOS_DIR=$HOME/unquoted\nNOT_XDG=\"$HOME/x\"\n",
        home,
    );
    assert_eq!(
        parsed,
        vec![
            home.join("Dokumente"),
            Path::new("/srv/musik").to_path_buf()
        ]
    );
}

// Resolves this machine's folders without walking them.
#[test]
fn this_machine_resolves_a_home_and_temp_roots() {
    let ctx = platform::resolve_context().unwrap();
    assert!(ctx.home().is_dir());
    assert!(!ctx.temp_roots().is_empty());
    if cfg!(target_os = "macos") {
        let area = "/private/var/folders"; // hygiene: data only
        let temp = std::fs::canonicalize(&ctx.temp_roots()[0].0).unwrap();
        assert!(temp.starts_with(area), "{}", temp.display());
    }
}
