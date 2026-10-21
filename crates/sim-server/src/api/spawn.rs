//! Injection seam over OS-thread creation.
//!
//! Every match runs on its own OS thread (never on the async
//! executor). The seam lets tests inject a spawn failure — the case
//! where the slot was taken but the thread never started — without
//! exhausting the OS's thread limit.

use std::fmt;

/// Why a simulation thread could not be started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnError {
    /// Human-readable reason (the OS error text, or a test's reason).
    pub message: String,
}

impl fmt::Display for SpawnError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for SpawnError {}

/// Injection seam over OS-thread creation.
///
/// The store takes a concurrency slot *before* it spawns the thread, so
/// a spawn failure must be reported back to the store: the slot is
/// released and the match is cached as failed.
pub trait ThreadSpawner: Send + Sync + fmt::Debug {
    /// Start `task` on a new OS thread.
    ///
    /// # Errors
    ///
    /// Returns [`SpawnError`] when the OS refuses to create the thread.
    /// The task must not have been started in that case.
    fn spawn(&self, name: String, task: Box<dyn FnOnce() + Send>) -> Result<(), SpawnError>;
}

/// Real adapter: one named OS thread per match.
#[derive(Debug, Default)]
pub struct OsThreadSpawner;

impl ThreadSpawner for OsThreadSpawner {
    fn spawn(&self, name: String, task: Box<dyn FnOnce() + Send>) -> Result<(), SpawnError> {
        // The join handle is dropped: the thread is detached and writes
        // its result back into the store.
        std::thread::Builder::new()
            .name(name)
            .spawn(task)
            .map(|_handle| ())
            .map_err(|error| SpawnError {
                message: error.to_string(),
            })
    }
}
