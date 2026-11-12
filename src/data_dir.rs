//! One stereo at a time per data directory. Two backends on the same collections would race each
//! other's database writes and git syncs, which the per-repo locks in `git.rs` can't prevent:
//! they're process-local.
//!
//! The owner holds an exclusive OS lock on `LOCK_FILE` in the data directory from before it touches
//! a repository until it exits (the OS releases it even in a crash), and once it listens it writes
//! its port to `PORT_FILE`, so that the desktop app can wait for a backend that is still starting
//! and open it instead of starting its own. The port is a file of its own because on Windows a
//! locked file can't be read by anyone else; it's only meaningful while the lock is held.

use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const LOCK_FILE: &str = "stereo.lock";
pub const PORT_FILE: &str = "stereo.port";

/// How long acquiring the lock retries before giving up: someone checking whether the directory is
/// in use (the desktop app) holds the lock for a moment too.
const ACQUIRE_TIMEOUT: Duration = Duration::from_secs(2);

/// The data directory's lock, held while this value lives.
pub struct DataDirLock {
    _file: File,
    port_file: PathBuf,
}

impl DataDirLock {
    /// Locks `home` (an existing directory) for this process, or fails if another stereo uses it.
    pub fn acquire(home: &Path) -> anyhow::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(home.join(LOCK_FILE))?;
        let deadline = Instant::now() + ACQUIRE_TIMEOUT;
        loop {
            match file.try_lock() {
                Ok(()) => break,
                Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(TryLockError::WouldBlock) => {
                    let port = std::fs::read_to_string(home.join(PORT_FILE))
                        .ok()
                        .and_then(|port| port.trim().parse::<u16>().ok());
                    let at = port.map_or(String::new(), |port| format!(" (on port {port})"));
                    anyhow::bail!(
                        "another stereo{at} is using the data directory {}: stop it, or give this \
                         one another --home",
                        home.display()
                    );
                }
                Err(TryLockError::Error(error)) => {
                    return Err(anyhow::anyhow!(error).context(format!(
                        "could not lock the data directory {}",
                        home.display()
                    )));
                }
            }
        }
        let lock = Self {
            _file: file,
            port_file: home.join(PORT_FILE),
        };
        // A previous owner's port, until this one listens.
        match std::fs::remove_file(&lock.port_file) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                tracing::warn!("could not remove {}: {error}", lock.port_file.display());
            }
            _ => {}
        }
        Ok(lock)
    }

    /// Records the port this stereo listens on, for the desktop app to find it.
    pub fn record_port(&self, port: u16) {
        if let Err(error) = std::fs::write(&self.port_file, format!("{port}\n")) {
            tracing::warn!("could not write {}: {error}", self.port_file.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_owner_is_refused_until_the_first_is_gone() {
        let dir = tempfile::tempdir().unwrap();
        let first = DataDirLock::acquire(dir.path()).unwrap();
        first.record_port(8123);
        let error = DataDirLock::acquire(dir.path()).err().unwrap().to_string();
        assert!(error.contains("on port 8123"), "{error}");

        drop(first);
        let second = DataDirLock::acquire(dir.path()).unwrap();
        // The first owner's port is gone with it.
        assert!(!dir.path().join(PORT_FILE).exists());
        drop(second);
    }
}
