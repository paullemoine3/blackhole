//! Disk writes happen on one background thread so the UI thread never waits for the filesystem.
//! Writes to the same file are coalesced (only the latest content matters) and applied in order.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Mutex, OnceLock};

enum Job {
    Write(PathBuf, String),
    Barrier(Sender<()>),
}

fn queue() -> &'static Mutex<Sender<Job>> {
    static QUEUE: OnceLock<Mutex<Sender<Job>>> = OnceLock::new();
    QUEUE.get_or_init(|| {
        let (tx, rx) = channel::<Job>();
        std::thread::Builder::new()
            .name("bh-writer".into())
            .spawn(move || {
                while let Ok(first) = rx.recv() {
                    let mut latest: HashMap<PathBuf, String> = HashMap::new();
                    let mut order: Vec<PathBuf> = Vec::new();
                    let mut waiters = Vec::new();
                    let mut absorb = |job: Job| match job {
                        Job::Write(path, text) => {
                            if latest.insert(path.clone(), text).is_none() {
                                order.push(path);
                            }
                        }
                        Job::Barrier(done) => waiters.push(done),
                    };
                    absorb(first);
                    while let Ok(next) = rx.try_recv() {
                        absorb(next);
                    }
                    for path in order {
                        if let Some(text) = latest.remove(&path) {
                            let _ = crate::write_atomic_now(&path, &text);
                        }
                    }
                    for done in waiters {
                        let _ = done.send(());
                    }
                }
            })
            .expect("writer thread");
        Mutex::new(tx)
    })
}

/// Queues `contents` to be written atomically to `path`.
pub fn write_later(path: PathBuf, contents: String) {
    if let Ok(tx) = queue().lock() {
        let _ = tx.send(Job::Write(path, contents));
    }
}

/// Blocks until everything queued so far is on disk (before reading it back, and at exit).
pub fn flush_writes() {
    let (done, wait) = channel();
    let sent = queue()
        .lock()
        .is_ok_and(|tx| tx.send(Job::Barrier(done)).is_ok());
    if sent {
        let _ = wait.recv();
    }
}
