//! What a temp or log row says about itself (v0.3 usability round): whether
//! it is a file or a folder, and how old its newest part is. Cache and tool
//! rows say neither.

mod common;

use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use space_optimiser_engine::Engine;
use space_optimiser_engine::model::EntryKind;
use space_optimiser_engine::policy::CatalogId;

#[test]
fn a_temp_row_says_file_or_folder_and_its_age_in_days() {
    let home = common::basic_home();
    let view = common::scan(&home.engine());

    let folder = common::named(&view, "old-build");
    assert_eq!(folder.kind, Some(EntryKind::Folder));
    assert!(folder.file_count > 0);
    let file = common::named(&view, "old-download.bin");
    assert_eq!(file.kind, Some(EntryKind::File));
    // The engine's clock is a week ahead of the files' timestamps.
    for c in [folder, file] {
        let age = c.age_days.expect("a temp row has an age");
        assert!((6..=7).contains(&age), "{age}");
    }

    let json = serde_json::to_value(folder).unwrap();
    assert_eq!(json["kind"], "folder");
    assert!(json["age_days"].is_u64());
}

#[test]
fn the_age_follows_the_engines_clock() {
    let home = common::basic_home();
    home.write("tmp/aged/inner.txt", b"x");
    let three_days_ahead = SystemTime::now() + Duration::from_secs(3 * 86_400 + 60);
    let engine = Engine::new(home.config_at(Arc::new(Mutex::new(three_days_ahead))));
    let view = common::scan(&engine);
    assert_eq!(common::named(&view, "aged").age_days, Some(3));
}

#[test]
fn cache_rows_have_no_kind_or_age() {
    let home = common::basic_home();
    let view = common::scan(&home.engine());
    let cache = view
        .candidates
        .iter()
        .find(|c| c.catalog_id == CatalogId::BrowserHttpCache)
        .expect("the fixture has a browser cache");
    assert_eq!(cache.kind, None);
    assert_eq!(cache.age_days, None);
}
