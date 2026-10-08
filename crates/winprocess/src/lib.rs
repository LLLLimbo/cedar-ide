//! Owned Windows process primitives for an isolated, controlled-spawning host.
//!
//! Isolated-agent tasks already use the NUL-stdin constructor. This piped-stdin
//! addition does not enable Windows LSP. Callers must separately authorize tools
//! and must not host concurrent broad-inheritance
//! process creation: HANDLE_LIST constrains this child, not unrelated spawns.
//! No shell, account grants, network listener or global security setting is used.
use std::path::PathBuf;

pub mod command_line;

#[derive(Debug, Clone)]
pub struct LaunchSpec {
    /// Fully qualified UTF-8 path to a native .exe; no PATH lookup in this layer.
    pub executable: PathBuf,
    pub arguments: Vec<String>,
    /// Existing absolute workspace directory; process-wide cwd is never changed.
    pub cwd: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessExit {
    /// Preserve all native Windows exit-code bits, including 259 and high bits.
    pub code: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CaptureProgress {
    pub bytes: usize,
    pub stdout_eof: bool,
    pub stderr_eof: bool,
}

/// Maximum bytes copied into the one owned pending stdin operation.
/// Larger protocol frames must be submitted as successive chunks.
pub const MAX_STDIN_WRITE_BYTES: usize = 64 * 1024;

/// Crate policy bound for an explicit child environment, including all entry
/// terminators and the final NUL (two NULs for an empty block). This is 2 MiB
/// of UTF-16 storage, not an operating-system limit or a limit on inheritance.
pub const MAX_ENVIRONMENT_UTF16_UNITS: usize = 1024 * 1024;

/// Completion is reported exactly once, either by begin or by a later poll.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use = "Account for every completed byte before sending another chunk"]
pub enum StdinWriteProgress {
    Idle,
    Pending,
    /// Bytes accepted by the transport, not proof the child consumed them.
    /// A short result is valid; the caller still owns the unwritten suffix.
    Written(usize),
    Closed,
}

/// The stdin endpoint is closed after cancellation and completion are joined.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use = "Cancellation can leave a partially transmitted protocol frame"]
pub enum StdinCancelOutcome {
    /// No outstanding write remained when cancellation was requested.
    Idle,
    /// Completion won the cancellation race, with this actual byte count.
    Written(usize),
    /// Cancellation completed. Some bytes may already have reached the child;
    /// do not replay this write or reuse the connection for framed messages.
    Cancelled,
}

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::WindowsCommand;
