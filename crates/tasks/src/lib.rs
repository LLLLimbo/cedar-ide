//! Bounded, explicitly invoked command tasks for a trusted workspace.
//!
//! This is **not a sandbox**: a command has all permissions of the running
//! account. The caller must enforce execution trust before [`TaskManager::start`].
//! An executable and literal arguments are used directly; a shell is run only
//! when the caller explicitly chooses one. No command is silently retried.
//!
//! Linux and macOS use one supervisor thread, nonblocking pipes, and an owned
//! process group. Linux is runtime-tested; macOS uses the same available APIs but
//! needs platform runtime validation. Windows remains unsupported pending Job
//! Object containment and cancellable pipe handling. Process groups cannot
//! contain a deliberately daemonized or otherwise escaped descendant.
//!
//! Do not install a competing SIGCHLD reaper or `SA_NOCLDWAIT`: this library must
//! exclusively own its children's wait state. It observes exit with `WNOWAIT`,
//! cleans up the process group, and only then reaps the leader. Task IDs are
//! lookup keys, never process IDs or authority to signal an arbitrary process.

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;
use thiserror::Error;

pub const MAX_OUTPUT_BYTES_PER_STREAM: usize = 256 * 1024;
pub const MAX_COMPLETED_TASKS: usize = 8;
pub const MAX_TIMEOUT: Duration = Duration::from_secs(300);
pub const MAX_PROGRAM_BYTES: usize = 4096;
pub const MAX_ARGUMENTS: usize = 256;
pub const MAX_ARGUMENT_BYTES: usize = 64 * 1024;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) const MAX_ERROR_BYTES: usize = 4096;

/// Monotonically allocated across all managers in this process; never reused.
/// IDs expire when evicted or when the owning manager is dropped. They must not
/// be carried across a new agent connection as a substitute for session identity.
pub type TaskId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Starting,
    Running,
    /// Cancellation has been requested; natural exit may still win the race.
    Cancelling,
    Succeeded,
    /// Nonzero/signal exit, or an execution/capture failure (see `error`).
    Failed,
    Cancelled,
    TimedOut,
    OutputLimit,
    SpawnFailed,
}

impl TaskState {
    pub fn is_terminal(self) -> bool {
        !matches!(self, Self::Starting | Self::Running | Self::Cancelling)
    }
}

/// A bounded full snapshot, not an append-only event or output chunk.
///
/// Replace displayed output on each poll. Bytes are retained until presentation,
/// so a UTF-8 sequence split between reads becomes intact on subsequent polls.
/// A live snapshot can temporarily end with a replacement character. Each
/// lossy string is at most three times its raw stream cap; no snapshots are
/// retained by the manager beyond one record per task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskSnapshot {
    pub id: TaskId,
    pub state: TaskState,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    /// Bytes were discarded, pipe capture failed, or final drain was cut short.
    pub truncated: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TaskError {
    #[error("Command tasks require Linux or macOS; Windows needs verified Job Object and cancellable pipe support")]
    UnsupportedPlatform,
    #[error("Program or arguments exceed supported bounds or contain NUL")]
    InvalidCommand,
    #[error("Timeout must be greater than zero and at most 300 seconds")]
    InvalidTimeout,
    #[error("Workspace root is unavailable or not a directory: {0}")]
    InvalidRoot(String),
    #[error("Command task {id} is still active")]
    Busy { id: TaskId },
    #[error("Unknown or expired command task {id}")]
    UnknownTask { id: TaskId },
    #[error("Command task IDs are exhausted")]
    Capacity,
    #[error("Command task supervisor is unavailable: {0}")]
    WorkerUnavailable(String),
}

impl TaskError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::UnsupportedPlatform => "unsupported_platform",
            Self::InvalidCommand => "invalid_command",
            Self::InvalidTimeout => "invalid_timeout",
            Self::InvalidRoot(_) => "invalid_root",
            Self::Busy { .. } => "task_busy",
            Self::UnknownTask { .. } => "unknown_task",
            Self::Capacity => "task_capacity",
            Self::WorkerUnavailable(_) => "task_worker_unavailable",
        }
    }
}

/// Owns at most one active command and eight completed command records.
///
/// Start/poll/cancel never wait for command exit. Dropping this manager cancels
/// and joins its supervisor, cleaning up and reaping normal owned children. An
/// OS process stuck indefinitely in an uninterruptible kernel operation can
/// still delay drop; no userspace implementation promises otherwise.
#[derive(Debug)]
pub struct TaskManager {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    inner: native::Manager,
}

impl TaskManager {
    /// Canonicalizes the trusted workspace root and starts one idle supervisor.
    pub fn new(root: impl AsRef<Path>) -> Result<Self, TaskError> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            Ok(Self {
                inner: native::Manager::new(root.as_ref())?,
            })
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = root;
            Err(TaskError::UnsupportedPlatform)
        }
    }

    /// Explicitly invoke arbitrary account-level code after caller trust checks.
    ///
    /// A successful return means the bounded request was accepted. Launch runs
    /// asynchronously and a failed spawn is observable as `SpawnFailed` in poll.
    /// Timeout includes time since acceptance; command outcome is never retried.
    pub fn start(
        &self,
        program: String,
        args: Vec<String>,
        timeout: Duration,
    ) -> Result<TaskId, TaskError> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            self.inner.start(program, args, timeout)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = (program, args, timeout);
            Err(TaskError::UnsupportedPlatform)
        }
    }

    /// Return a bounded snapshot; does not consume output or allocate history.
    pub fn poll(&self, id: TaskId) -> Result<TaskSnapshot, TaskError> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            self.inner.poll(id)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = id;
            Err(TaskError::UnsupportedPlatform)
        }
    }

    /// Request cancellation without waiting. Repeated cancellation is idempotent
    /// for a retained task, including an already completed one. No PID is accepted.
    pub fn cancel(&self, id: TaskId) -> Result<TaskSnapshot, TaskError> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            self.inner.cancel(id)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = id;
            Err(TaskError::UnsupportedPlatform)
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod native;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_state_and_snapshot_serialization_are_stable() {
        let snapshot = TaskSnapshot {
            id: 5,
            state: TaskState::OutputLimit,
            stdout: "hello".into(),
            stderr: String::new(),
            exit_code: None,
            truncated: true,
            error: None,
        };
        let json = serde_json::to_value(&snapshot).unwrap();
        assert_eq!(json["state"], "output_limit");
        assert_eq!(
            serde_json::from_value::<TaskSnapshot>(json).unwrap(),
            snapshot
        );
        assert!(snapshot.state.is_terminal());
        assert!(!TaskState::Cancelling.is_terminal());
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    #[test]
    fn local_execution_is_explicitly_unsupported() {
        assert!(matches!(
            TaskManager::new("."),
            Err(TaskError::UnsupportedPlatform)
        ));
    }
}
