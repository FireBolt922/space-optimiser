//! The Overview (phase 14): memory, CPU, the busiest processes, and the
//! disk that holds the home folder. Read only.

mod common;

use space_optimiser_engine::model::DiskUsage;
use space_optimiser_engine::monitor::{Monitor, TOP_PROCESSES};

#[test]
fn a_reading_has_memory_and_at_most_five_processes_per_list() {
    let monitor = Monitor::new();
    let usage = monitor.usage();
    assert!(usage.memory.total_bytes > 0);
    assert!(usage.memory.used_bytes <= usage.memory.total_bytes);
    assert!(usage.swap.used_bytes <= usage.swap.total_bytes);
    assert!(usage.logical_cpus > 0);
    assert!(usage.top_memory.len() <= TOP_PROCESSES);
    assert!(usage.top_cpu.len() <= TOP_PROCESSES);
    // This test's own process uses memory, so the list is never empty.
    assert!(!usage.top_memory.is_empty());
}

#[test]
fn the_first_reading_has_no_cpu_figure_and_a_later_one_does() {
    let monitor = Monitor::new();
    let first = monitor.usage();
    assert_eq!(first.cpu_percent, None);
    assert!(first.top_cpu.is_empty());

    std::thread::sleep(sysinfo_interval());
    let second = monitor.usage();
    let cpu = second.cpu_percent.expect("a CPU figure after two samples");
    assert!((0.0..=100.0).contains(&cpu));
    for p in &second.top_cpu {
        assert!(p.cpu_percent.is_some_and(|c| c > 0.0 && c <= 100.0));
    }
}

#[test]
fn a_reading_taken_too_soon_repeats_the_last_one() {
    let monitor = Monitor::new();
    monitor.usage();
    std::thread::sleep(sysinfo_interval());
    let start = std::time::Instant::now();
    let a = monitor.usage();
    let b = monitor.usage();
    // A slow runner can take longer than the interval for one reading; then
    // the second is a fresh one and there is nothing to compare.
    if start.elapsed() < space_optimiser_engine::monitor::MINIMUM_INTERVAL {
        assert_eq!(a, b);
    }
}

#[test]
fn a_process_entry_carries_a_name_and_numbers_only() {
    let monitor = Monitor::new();
    let usage = monitor.usage();
    let json = serde_json::to_value(&usage).unwrap();
    let entry = json["top_memory"][0].as_object().unwrap();
    let mut keys: Vec<_> = entry.keys().map(String::as_str).collect();
    keys.sort();
    assert_eq!(keys, ["count", "cpu_percent", "memory_bytes", "name"]);
}

#[test]
fn disk_usage_is_for_the_volume_that_holds_the_home_folder() {
    let home = common::basic_home();
    let engine = home.engine();
    let DiskUsage {
        total_bytes,
        free_bytes,
        used_bytes,
        ..
    } = engine.disk_usage().unwrap();
    assert!(total_bytes > 0);
    assert!(free_bytes <= total_bytes);
    assert_eq!(used_bytes, total_bytes - free_bytes);
}

fn sysinfo_interval() -> std::time::Duration {
    space_optimiser_engine::monitor::MINIMUM_INTERVAL + std::time::Duration::from_millis(50)
}
