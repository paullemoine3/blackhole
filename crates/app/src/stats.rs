//! Memory and CPU used by the browser: its own process plus the web engine processes it owns.

use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;
use sysinfo::{CpuRefreshKind, Pid, ProcessRefreshKind, ProcessesToUpdate, System};

/// How often the meter is read when nothing asks for it sooner.
const EVERY: Duration = Duration::from_secs(3);
/// After a tab closes, the engine returns its memory a moment later.
const SETTLE: Duration = Duration::from_millis(800);

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

/// Reads the meter on a thread of its own: scanning every process of the machine takes tens of
/// milliseconds on Windows, a visible hitch if the UI thread did it every few seconds.
pub struct Sampler {
    poke: Sender<()>,
    results: Receiver<Usage>,
}

impl Sampler {
    pub fn start() -> Self {
        let (poke, wake) = mpsc::channel::<()>();
        let (out, results) = mpsc::channel();
        let _ = std::thread::Builder::new()
            .name("bh-stats".into())
            .spawn(move || {
                let mut meter = Meter::new();
                loop {
                    if out.send(meter.sample()).is_err() {
                        break;
                    }
                    match wake.recv_timeout(EVERY) {
                        Ok(()) => {
                            std::thread::sleep(SETTLE);
                            while wake.try_recv().is_ok() {}
                        }
                        Err(RecvTimeoutError::Timeout) => {}
                        // The window is gone.
                        Err(RecvTimeoutError::Disconnected) => break,
                    }
                }
            });
        Self { poke, results }
    }

    /// Asks for a new reading soon (the number of tabs changed).
    pub fn poke(&self) {
        let _ = self.poke.send(());
    }

    /// The newest reading that arrived since the last call, if any.
    pub fn latest(&self) -> Option<Usage> {
        self.results.try_iter().last()
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
    fn the_sampler_reads_on_its_own_thread_and_answers_a_poke() {
        let sampler = super::Sampler::start();
        let first = sampler.results.recv_timeout(super::EVERY * 2).unwrap();
        assert!(first.memory_mb > 0);
        sampler.poke();
        assert!(sampler.results.recv_timeout(super::EVERY).is_ok(), "a poke comes back before the period");
    }

    #[test]
    fn knows_the_core_count() {
        assert!(!super::Meter::new().system.cpus().is_empty());
    }
}
