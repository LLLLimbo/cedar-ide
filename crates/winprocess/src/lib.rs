//! Owned Windows process primitives for an isolated, controlled-spawning host.
//!
//! This checkpoint does not enable Windows IDE task execution. A caller must
//! separately authorize tools and must not host concurrent broad-inheritance
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

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::WindowsCommand;
