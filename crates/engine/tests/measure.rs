//! File identity, allocated size, and free space (phase 4 items 5 and 6).

mod common;

use std::fs;

use space_optimiser_engine::measure;

// Test 3: two hard links are one file identity.
#[test]
fn two_hard_links_are_one_identity() {
    let home = common::basic_home();
    let a = home.write("tmp/a.bin", b"shared bytes");
    let b = home.path("tmp/b.bin");
    fs::hard_link(&a, &b).unwrap();
    let other = home.path("tmp/old-download.bin");

    assert_eq!(measure::file_id(&a).unwrap(), measure::file_id(&b).unwrap());
    assert_ne!(
        measure::file_id(&a).unwrap(),
        measure::file_id(&other).unwrap()
    );
    assert_eq!(measure::link_count(&a).unwrap(), 2);
    assert_eq!(measure::link_count(&other).unwrap(), 1);
}

#[test]
fn a_directory_has_an_identity() {
    let home = common::basic_home();
    let tmp = home.path("tmp");
    assert_eq!(
        measure::file_id(&tmp).unwrap(),
        measure::file_id(&common::spelled(&tmp, "old-build/..")).unwrap()
    );
}

#[test]
fn allocated_size_of_a_written_file_is_measured() {
    let home = common::basic_home();
    let path = home.write("tmp/full.bin", &vec![7u8; 256 * 1024]);
    let size = measure::allocated_size(&path, &fs::symlink_metadata(&path).unwrap());
    assert!(size.exact);
    assert!(size.bytes >= 256 * 1024, "{}", size.bytes);
}

// Test 5: allocated size of a sparse file is smaller than its logical size.
// APFS (macOS runners) and ext4 (Ubuntu runners) both support sparse files.
#[cfg(unix)]
#[test]
fn a_sparse_file_is_smaller_than_its_logical_size() {
    use std::io::{Seek, SeekFrom, Write};

    let home = common::basic_home();
    let path = home.path("tmp/sparse.bin");
    let mut file = fs::File::create(&path).unwrap();
    file.seek(SeekFrom::Start(64 * 1024 * 1024)).unwrap();
    file.write_all(b"end").unwrap();
    drop(file);

    let metadata = fs::symlink_metadata(&path).unwrap();
    let size = measure::allocated_size(&path, &metadata);
    assert!(metadata.len() > 64 * 1024 * 1024);
    assert!(size.bytes < 1024 * 1024, "allocated {} bytes", size.bytes);
}

#[test]
fn free_space_is_reported_for_the_volume_of_a_path() {
    let home = common::basic_home();
    let free = measure::free_space(&home.path("tmp")).unwrap();
    assert!(free > 0);
}
