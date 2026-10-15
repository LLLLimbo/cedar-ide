//! Locally produced connection-failure provenance, independent of peer text.
use super::*;
use std::fmt;

/// Direct-child ownership evidence for a failed connection.
/// This never attests to descendant cleanup, pipe-thread completion, remote
/// SSH cleanup, or a graceful language-server exit. Do not infer it from text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionOwnership {
    /// This failed construction has no outstanding direct-child ownership.
    /// This does not claim the OS never briefly created a process on spawn Err.
    NoChild,
    /// The same process owner's cleanup observation returned success.
    CleanupVerified,
    /// Setup or cleanup ended without a successful direct-child receipt.
    CleanupUnverified,
}

/// The original connection error and separate direct-child cleanup evidence.
/// An unwinding panic does not return this value; an owner catching one must
/// conservatively retain uncertainty about an attempted process connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionFailure {
    pub message: String,
    pub ownership: ConnectionOwnership,
    pub cleanup_error: Option<String>,
}

impl ConnectionFailure {
    // Call only at a locally proven no-child boundary, never for an unknown
    // String-returning constructor or a setup error after successful spawn.
    pub(crate) fn no_child(message: String) -> Self {
        Self {
            message,
            ownership: ConnectionOwnership::NoChild,
            cleanup_error: None,
        }
    }

    pub(crate) fn after_cleanup(message: String, cleanup: Result<(), String>) -> Self {
        match cleanup {
            Ok(()) => Self {
                message,
                ownership: ConnectionOwnership::CleanupVerified,
                cleanup_error: None,
            },
            Err(error) => Self {
                message,
                ownership: ConnectionOwnership::CleanupUnverified,
                cleanup_error: Some(error),
            },
        }
    }

    pub(crate) fn setup_unverified(message: String) -> Self {
        Self::after_cleanup(
            message,
            Err("transport_cleanup_unverified: connection setup ended without a direct-child reaper acknowledgement".into()),
        )
    }
}

impl fmt::Display for ConnectionFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)?;
        if let Some(error) = &self.cleanup_error {
            write!(formatter, "; {error}")?;
        }
        Ok(())
    }
}

impl std::error::Error for ConnectionFailure {}

impl Client {
    /// Connect with locally produced ownership evidence on ordinary failure.
    /// After a process is spawned and its reaper is installed, a failed Hello,
    /// validation, or cancellation consumes that owner's cleanup observation,
    /// waiting at most three additional seconds with the existing two-second
    /// grace. Preflight/spawn errors have no outstanding direct child; setup
    /// failures without a reaper acknowledgement remain unverified. Embedded
    /// Local starts no transport child. OS creation/kernel cleanup are not
    /// interruptible, and unwinding panics do not return a typed failure.
    /// Legacy String-returning APIs retain their historical error timing.
    pub fn connect_with_cancellation_detailed(
        spec: ConnectionSpec,
        cancellation: ConnectionCancellation,
    ) -> Result<Self, ConnectionFailure> {
        let cancellation = Some(cancellation);
        if cancellation_requested(&cancellation) {
            return Err(ConnectionFailure::no_child(cancellation_error()));
        }
        match spec {
            #[cfg(target_os = "linux")]
            ConnectionSpec::BundledLinux { root, allow_run } => {
                Self::connect_bundled_linux_inner(root, allow_run, cancellation)
            }
            ConnectionSpec::Local { root, allow_run } => {
                #[cfg(windows)]
                {
                    Self::connect_bundled_windows_detailed(&root, allow_run, cancellation)
                }
                #[cfg(not(windows))]
                {
                    // This shared embedded constructor never acquires a child,
                    // including when opening or validating its workspace fails.
                    Self::connect_embedded(root, allow_run, cancellation)
                        .map_err(ConnectionFailure::no_child)
                }
            }
            ConnectionSpec::Ssh {
                host,
                port,
                root,
                agent_path,
                allow_run,
            } => {
                let command = ssh_command(&host, port, &root, &agent_path, allow_run)
                    .map_err(ConnectionFailure::no_child)?;
                Self::from_command_detailed(command, cancellation)
            }
        }
    }

    /// Use a separately deployed agent with typed direct-child evidence.
    /// Ordinary postspawn failures wait up to three additional seconds for the
    /// same owner's cleanup receipt, using the existing two-second grace.
    /// Setup failures without a receipt are unverified; an unwinding panic
    /// returns no evidence. The legacy String API keeps asynchronous failure
    /// cleanup and its historical timing.
    pub fn spawn_agent_with_cancellation_detailed(
        agent: &Path,
        root: &Path,
        allow_run: bool,
        cancellation: ConnectionCancellation,
    ) -> Result<Self, ConnectionFailure> {
        Self::from_command_detailed(agent_command(agent, root, allow_run), Some(cancellation))
    }

    fn from_command_detailed(
        command: Command,
        cancellation: Option<ConnectionCancellation>,
    ) -> Result<Self, ConnectionFailure> {
        let process = ProcessClient::spawn_with_grace_and_setup_error(
            command,
            CLOSE_GRACE,
            cancellation,
            || ConnectionFailure::no_child(cancellation_error()),
            |error| ConnectionFailure::no_child(process_spawn_error(error)),
            ConnectionFailure::setup_unverified,
        )?;
        Self::from_process_detailed(process, validate_handshake)
    }

    #[cfg(windows)]
    fn connect_bundled_windows_detailed(
        root: &Path,
        allow_run: bool,
        cancellation: Option<ConnectionCancellation>,
    ) -> Result<Self, ConnectionFailure> {
        // This preparation only resolves/checks the fixed executable sibling
        // and constructs its command. Windows root validation remains in Hello.
        let (command, agent) =
            bundled_windows_agent_command(root, allow_run).map_err(ConnectionFailure::no_child)?;
        let start_error = |error| {
            format!("bundled_agent_start_failed: could not start bundled cedar-agent.exe: {error}")
        };
        let process = ProcessClient::spawn_with_grace_and_setup_error(
            command,
            CLOSE_GRACE,
            cancellation,
            || ConnectionFailure::no_child(cancellation_error()),
            |error| ConnectionFailure::no_child(start_error(error.to_string())),
            |error| ConnectionFailure::setup_unverified(start_error(error)),
        )?;
        Self::from_process_detailed(process, validate_handshake).map_err(|mut failure| {
            // Preserve the Windows user-facing prefix without deriving any
            // ownership evidence from that prefix or from the peer's message.
            failure.message = bundled_windows_connection_error(&agent, failure.message);
            failure
        })
    }

    pub(super) fn from_process_detailed(
        process: ProcessClient,
        validate: impl FnOnce(&Payload) -> Result<(), String>,
    ) -> Result<Self, ConnectionFailure> {
        Self::from_process_detailed_with_timeout(process, validate, CONNECT_CLEANUP_TIMEOUT)
    }

    // The timeout argument is private so channel-only tests can distinguish a
    // real observation timeout from a dropped sender without changing grace.
    fn from_process_detailed_with_timeout(
        mut process: ProcessClient,
        validate: impl FnOnce(&Payload) -> Result<(), String>,
        cleanup_timeout: Duration,
    ) -> Result<Self, ConnectionFailure> {
        // Retain this exact owner through Hello, validation and cancellation.
        // request() may already close it, but never consumes its reaper receiver.
        let result = process.request(Operation::Hello).and_then(|handshake| {
            validate(&handshake)?;
            if cancellation_requested(&process.cancellation) {
                return Err(cancellation_error());
            }
            Ok(handshake)
        });
        match result {
            Ok(handshake) => Ok(Self {
                cancellation: process.cancellation.clone(),
                backend: Backend::Process(process),
                handshake,
            }),
            Err(message) => {
                let cleanup = process.close_and_wait(cleanup_timeout);
                Err(ConnectionFailure::after_cleanup(message, cleanup))
            }
        }
    }
}

#[cfg(test)]
mod tests;
