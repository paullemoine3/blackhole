//! Memory and CPU used by the browser: its own process plus the web engine processes it owns.

use std::collections::HashMap;
use sysinfo::{CpuRefreshKind, Pid, ProcessRefreshKind, ProcessesToUpdate, System};

#[derive(Clone, Copy, PartialEq)]
pub struct Usage {
    pub memory_mb: u64,
    /// Share of the whole machine's CPU, in percent.
    pub cpu: f32,
}

pub struct Meter {
    system: System,
    me: Pid,
}

impl Meter {
    pub fn new() -> Self {
        let mut system = System::new();
        // Without it `cpus()` stays empty and the CPU share is never divided by the core count.
        system.refresh_cpu_list(CpuRefreshKind::nothing());
        Self { system, me: Pid::from_u32(std::process::id()) }
    }

    pub fn sample(&mut self) -> Usage {
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_cpu().with_memory(),
        );
        let parents: HashMap<Pid, Pid> = self
            .system
            .processes()
            .iter()
            .filter_map(|(pid, p)| p.parent().map(|parent| (*pid, parent)))
            .collect();
        let (mut bytes, mut cpu) = (0u64, 0f32);
        for (pid, process) in self.system.processes() {
            if self.owned(*pid, &parents) {
                bytes += process.memory();
                cpu += process.cpu_usage();
            }
        }
        let cores = self.system.cpus().len().max(1) as f32;
        Usage { memory_mb: bytes / (1024 * 1024), cpu: cpu / cores }
    }

    fn owned(&self, pid: Pid, parents: &HashMap<Pid, Pid>) -> bool {
        #[cfg(target_os = "macos")]
        if blackhole_macos_webext::responsible_pid(pid.as_u32()) == self.me.as_u32() {
            return true;
        }
        let mut current = pid;
        // Bounded walk: a stale process table must not loop forever.
        for _ in 0..16 {
            if current == self.me {
                return true;
            }
            match parents.get(&current) {
                Some(parent) if *parent != current => current = *parent,
                _ => return false,
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn counts_its_own_process() {
        let mut meter = super::Meter::new();
        assert!(meter.sample().memory_mb > 0);
    }

    #[test]
    fn knows_the_core_count() {
        assert!(!super::Meter::new().system.cpus().is_empty());
    }
}
