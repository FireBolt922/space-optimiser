//! Memory, CPU, and the busiest processes, for the Overview (phase 14).
//!
//! Read only. This module reads figures through `sysinfo`; it never stops,
//! pauses, or reprioritises a process, and nothing else in the engine uses
//! `sysinfo` (a hygiene test checks both). Process entries carry a name and
//! numbers, never a path, command line, user, or id.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use sysinfo::{MemoryRefreshKind, ProcessRefreshKind, ProcessesToUpdate, System};

use crate::model::{MemoryUsage, ProcessUsage, SystemUsage};

/// How many process names each list shows.
pub const TOP_PROCESSES: usize = 5;

/// CPU use is the change between two samples; samples closer together than
/// this are not accurate, so a reading that soon repeats the last one.
pub const MINIMUM_INTERVAL: Duration = sysinfo::MINIMUM_CPU_UPDATE_INTERVAL;

/// Longest process name shown, in characters.
const NAME_LIMIT: usize = 60;

pub struct Monitor(Mutex<Inner>);

struct Inner {
    system: System,
    last: Option<(Instant, SystemUsage)>,
}

impl Default for Monitor {
    fn default() -> Self {
        Self::new()
    }
}

impl Monitor {
    pub fn new() -> Self {
        // Linux: do not keep `/proc/<pid>/stat` open between readings.
        sysinfo::set_open_files_limit(0);
        Self(Mutex::new(Inner {
            system: System::new(),
            last: None,
        }))
    }

    /// A fresh reading. The first has no CPU figures; a reading less than
    /// [`MINIMUM_INTERVAL`] after the last returns the last one again.
    pub fn usage(&self) -> SystemUsage {
        let mut inner = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let now = Instant::now();
        if let Some((at, usage)) = &inner.last
            && now.duration_since(*at) < MINIMUM_INTERVAL
        {
            return usage.clone();
        }
        let cpu_ready = inner.last.is_some();
        let system = &mut inner.system;
        system.refresh_memory_specifics(MemoryRefreshKind::everything());
        system.refresh_cpu_usage();
        system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_memory().with_cpu(),
        );

        let logical_cpus = system.cpus().len().max(1);
        let samples: Vec<Sample> = system
            .processes()
            .values()
            .map(|p| Sample {
                name: p.name().to_string_lossy().into_owned(),
                memory_bytes: p.memory(),
                cpu_per_core: p.cpu_usage(),
            })
            .collect();
        let (top_memory, top_cpu) = top_processes(&samples, logical_cpus, cpu_ready);
        let total = system.total_memory();
        let usage = SystemUsage {
            memory: MemoryUsage {
                total_bytes: total,
                used_bytes: total.saturating_sub(system.available_memory()),
            },
            swap: MemoryUsage {
                total_bytes: system.total_swap(),
                used_bytes: system.used_swap().min(system.total_swap()),
            },
            cpu_percent: cpu_ready.then(|| percent(system.global_cpu_usage())),
            logical_cpus,
            top_memory,
            top_cpu,
        };
        inner.last = Some((now, usage.clone()));
        usage
    }
}

/// One process as `sysinfo` reports it. `cpu_per_core` is a share of one
/// core, so it can exceed 100.
#[derive(Debug, Clone)]
struct Sample {
    name: String,
    memory_bytes: u64,
    cpu_per_core: f32,
}

/// Processes grouped by name, then the [`TOP_PROCESSES`] names using the
/// most memory and the most CPU. CPU is shown as a share of the whole
/// machine so it matches the CPU gauge. Without two samples there are no
/// CPU figures and the CPU list is empty.
fn top_processes(
    samples: &[Sample],
    logical_cpus: usize,
    cpu_ready: bool,
) -> (Vec<ProcessUsage>, Vec<ProcessUsage>) {
    let cores = logical_cpus.max(1) as f32;
    let mut groups: HashMap<String, (u64, f32, u32)> = HashMap::new();
    for sample in samples {
        let name = short_name(&sample.name);
        if name.is_empty() {
            continue;
        }
        let group = groups.entry(name).or_default();
        group.0 = group.0.saturating_add(sample.memory_bytes);
        group.1 += sample.cpu_per_core.max(0.0);
        group.2 += 1;
    }
    let all: Vec<ProcessUsage> = groups
        .into_iter()
        .map(|(name, (memory_bytes, cpu, count))| ProcessUsage {
            name,
            memory_bytes,
            cpu_percent: cpu_ready.then(|| percent(cpu / cores)),
            count,
        })
        .collect();

    let mut by_memory: Vec<ProcessUsage> =
        all.iter().filter(|p| p.memory_bytes > 0).cloned().collect();
    by_memory.sort_by(|a, b| {
        b.memory_bytes
            .cmp(&a.memory_bytes)
            .then_with(|| a.name.cmp(&b.name))
    });
    by_memory.truncate(TOP_PROCESSES);

    let mut by_cpu: Vec<ProcessUsage> = all
        .into_iter()
        .filter(|p| p.cpu_percent.is_some_and(|c| c > 0.0))
        .collect();
    by_cpu.sort_by(|a, b| {
        let (ca, cb) = (a.cpu_percent.unwrap_or(0.0), b.cpu_percent.unwrap_or(0.0));
        cb.total_cmp(&ca).then_with(|| a.name.cmp(&b.name))
    });
    by_cpu.truncate(TOP_PROCESSES);
    (by_memory, by_cpu)
}

/// Rounded to one decimal place and kept within 0–100.
fn percent(value: f32) -> f32 {
    if !value.is_finite() {
        return 0.0;
    }
    (value.clamp(0.0, 100.0) * 10.0).round() / 10.0
}

fn short_name(name: &str) -> String {
    name.trim().chars().take(NAME_LIMIT).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(name: &str, memory_bytes: u64, cpu_per_core: f32) -> Sample {
        Sample {
            name: name.to_string(),
            memory_bytes,
            cpu_per_core,
        }
    }

    #[test]
    fn processes_with_one_name_are_summed_and_counted() {
        let samples = [
            sample("Browser Helper", 100, 10.0),
            sample("Browser Helper", 300, 30.0),
            sample("editor", 200, 5.0),
        ];
        let (memory, cpu) = top_processes(&samples, 4, true);
        assert_eq!(memory[0].name, "Browser Helper");
        assert_eq!(memory[0].memory_bytes, 400);
        assert_eq!(memory[0].count, 2);
        assert_eq!(memory[0].cpu_percent, Some(10.0));
        assert_eq!(cpu[0].name, "Browser Helper");
        assert_eq!(cpu[1].name, "editor");
    }

    #[test]
    fn each_list_is_the_top_five_largest_first_ties_by_name() {
        let samples: Vec<_> = (0..8)
            .map(|i| sample(&format!("p{i}"), 1000 - (i / 2) * 100, (i % 3) as f32))
            .collect();
        let (memory, cpu) = top_processes(&samples, 1, true);
        let names: Vec<_> = memory.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["p0", "p1", "p2", "p3", "p4"]);
        assert_eq!(cpu.len(), TOP_PROCESSES);
        let names: Vec<_> = cpu.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["p2", "p5", "p1", "p4", "p7"]);
    }

    #[test]
    fn cpu_is_a_share_of_the_whole_machine_within_0_to_100() {
        let samples = [
            sample("busy", 1, 350.0),
            sample("runaway", 1, 5000.0),
            sample("odd", 1, f32::NAN),
        ];
        let (memory, cpu) = top_processes(&samples, 8, true);
        let busy = memory.iter().find(|p| p.name == "busy").unwrap();
        assert_eq!(busy.cpu_percent, Some(43.8));
        let runaway = memory.iter().find(|p| p.name == "runaway").unwrap();
        assert_eq!(runaway.cpu_percent, Some(100.0));
        assert!(cpu.iter().all(|p| p.name != "odd"));
    }

    #[test]
    fn an_idle_process_is_not_in_the_cpu_list() {
        let samples = [sample("idle", 500, 0.0), sample("busy", 100, 2.0)];
        let (memory, cpu) = top_processes(&samples, 2, true);
        assert_eq!(memory.len(), 2);
        let names: Vec<_> = cpu.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["busy"]);
    }

    #[test]
    fn without_two_samples_there_are_no_cpu_figures() {
        let samples = [sample("busy", 100, 80.0)];
        let (memory, cpu) = top_processes(&samples, 2, false);
        assert_eq!(memory[0].cpu_percent, None);
        assert!(cpu.is_empty());
    }

    #[test]
    fn a_long_name_is_cut_and_an_empty_one_is_left_out() {
        let long = "x".repeat(200);
        let samples = [sample(&long, 10, 0.0), sample("  ", 10, 0.0)];
        let (memory, _) = top_processes(&samples, 1, true);
        assert_eq!(memory.len(), 1);
        assert_eq!(memory[0].name.chars().count(), NAME_LIMIT);
    }

    #[test]
    fn a_process_without_memory_is_not_in_the_memory_list() {
        let samples = [sample("other-user", 0, 0.0), sample("mine", 10, 0.0)];
        let (memory, _) = top_processes(&samples, 1, true);
        let names: Vec<_> = memory.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["mine"]);
    }
}
