//! Bounded stdio language-service building blocks for the Cedar desktop client.
//!
//! [`LspClient`] launches an explicitly configured executable, exchanges LSP
//! messages, and delivers events independently of request responses. It does not
//! install servers or execute code returned by one. See `docs/LANGUAGE_SERVICES.md`
//! for UI integration and the deliberately limited debugger foundation.

pub mod dap;
pub mod framing;
mod implementations;
mod lsp;
mod shutdown;
mod transport;
mod workspace_symbols;

pub use lsp::{
    Diagnostic, LspAbortHandle, LspClient, LspEvent, Position, PublishDiagnostics, Range,
};
pub use shutdown::{
    ShutdownOutcome, WindowsCleanupErrors, WindowsCleanupStatus, WindowsRootExit,
    WindowsShutdownOutcome, WindowsShutdownReason,
};
pub use transport::{ClientOptions, Error, ProcessConfig, RpcEvent, StdioRpc};
