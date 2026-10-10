//! Fixed sibling Linux route. Preflight and metadata are compatibility checks,
//! not executable identity, a file-replacement defense, or execution permission.
use super::*;
use std::{
    fmt,
    fs::{self, File},
    os::unix::fs::{MetadataExt, PermissionsExt},
};

const CONNECT_CLEANUP_TIMEOUT: Duration = Duration::from_secs(3);

/// Direct-child ownership evidence for a failed bundled Linux connection.
/// This never attests to descendant cleanup, pipe-thread completion, or a
/// graceful language-server exit. Callers must not infer it from error text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionOwnership {
    NoChild,
    CleanupVerified,
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
    fn no_child(message: String) -> Self {
        Self {
            message,
            ownership: ConnectionOwnership::NoChild,
            cleanup_error: None,
        }
    }

    fn after_cleanup(message: String, cleanup: Result<(), String>) -> Self {
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

    fn setup_unverified(message: String) -> Self {
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
    /// Open only this executable's fixed Linux agent sibling, retaining typed
    /// failure ownership evidence. Local/SSH/Windows connection APIs are unchanged.
    pub fn connect_bundled_linux_detailed(
        root: PathBuf,
        allow_run: bool,
    ) -> Result<Self, ConnectionFailure> {
        Self::connect_bundled_linux_inner(root, allow_run, None)
    }

    /// Cancellation before spawn returns NoChild. After spawn, Hello or
    /// validation failure and observed cancellation close the owned process and
    /// wait at most three seconds for its reaper, using the existing two-second
    /// grace. OS process creation and kernel cleanup are not interruptible.
    /// Once the reaper is installed, Drop/panic cleanup is asynchronous; an
    /// unwinding panic never returns verified ownership evidence to the caller.
    pub fn connect_bundled_linux_with_cancellation_detailed(
        root: PathBuf,
        allow_run: bool,
        cancellation: ConnectionCancellation,
    ) -> Result<Self, ConnectionFailure> {
        Self::connect_bundled_linux_inner(root, allow_run, Some(cancellation))
    }

    pub(super) fn connect_bundled_linux_inner(
        root: PathBuf,
        allow_run: bool,
        cancellation: Option<ConnectionCancellation>,
    ) -> Result<Self, ConnectionFailure> {
        if cancellation_requested(&cancellation) {
            return Err(ConnectionFailure::no_child(cancellation_error()));
        }
        let executable = std::env::current_exe().map_err(|error| {
            ConnectionFailure::no_child(format!(
                "bundled_agent_missing: cannot locate this executable's bundled cedar-agent: {error}"
            ))
        })?;
        let agent = bundled_linux_agent_path(&executable).map_err(ConnectionFailure::no_child)?;
        validate_linux_agent_file(&agent).map_err(ConnectionFailure::no_child)?;
        let root = canonical_linux_root(&root).map_err(ConnectionFailure::no_child)?;
        let mut command = Command::new(&agent);
        command.arg("--root").arg(&root);
        if allow_run {
            command.arg("--allow-run");
        }
        let process = ProcessClient::spawn_with_grace_and_setup_error(
            command,
            CLOSE_GRACE,
            cancellation,
            || ConnectionFailure::no_child(cancellation_error()),
            |error| {
                ConnectionFailure::no_child(format!(
                    "bundled_agent_start_failed: could not start bundled cedar-agent at {}: {error}",
                    agent.display()
                ))
            },
            ConnectionFailure::setup_unverified,
        )?;
        Self::from_bundled_linux_process(process, &root)
    }

    fn from_bundled_linux_process(
        mut process: ProcessClient,
        root: &str,
    ) -> Result<Self, ConnectionFailure> {
        // Retain the ProcessClient until every Hello check and the final
        // cancellation observation succeeds. A transport error may already
        // initiate close, but the same owner still holds its reaper receiver.
        let result = process.request(Operation::Hello).and_then(|handshake| {
            validate_bundled_linux_handshake(&handshake, root)?;
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
                let cleanup = process.close_and_wait(CONNECT_CLEANUP_TIMEOUT);
                Err(ConnectionFailure::after_cleanup(message, cleanup))
            }
        }
    }
}

// Production calls this only with current_exe. Synthetic paths let unit tests
// check the rule without making arbitrary executable selection a public API.
fn bundled_linux_agent_path(executable: &Path) -> Result<PathBuf, String> {
    if !executable.is_absolute() {
        return Err("bundled_agent_missing: the current executable path must be absolute".into());
    }
    let directory = executable.parent().ok_or_else(|| {
        "bundled_agent_missing: the current executable has no bundle directory".to_owned()
    })?;
    Ok(directory.join("cedar-agent"))
}

fn canonical_linux_root(root: &Path) -> Result<String, String> {
    let root = fs::canonicalize(root)
        .map_err(|error| format!("invalid_root: cannot canonicalize workspace root: {error}"))?;
    if !root.is_dir() {
        return Err("invalid_root: workspace root must be a directory".into());
    }
    root.into_os_string()
        .into_string()
        .map_err(|_| "invalid_root: bundled Linux workspace root must be valid UTF-8".into())
}

fn validate_linux_agent_file(agent: &Path) -> Result<(), String> {
    let invalid = |reason: &str| {
        format!(
            "bundled_agent_invalid: bundled cedar-agent at {} {reason}; restore the native executable beside this executable",
            agent.display()
        )
    };
    let metadata = fs::symlink_metadata(agent).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            format!(
                "bundled_agent_missing: bundled cedar-agent is missing at {}; restore it beside this executable",
                agent.display()
            )
        } else {
            invalid("cannot be inspected")
        }
    })?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(invalid("must be a regular file, not a symlink"));
    }
    if metadata.permissions().mode() & 0o111 == 0 {
        return Err(invalid("must already have executable permission"));
    }
    let mut file = File::open(agent).map_err(|_| invalid("cannot be read"))?;
    let opened = file
        .metadata()
        .map_err(|_| invalid("cannot be inspected"))?;
    if !opened.is_file() || metadata.dev() != opened.dev() || metadata.ino() != opened.ino() {
        return Err(invalid("changed during preflight"));
    }
    let mut header = [0u8; 64];
    let header_len = if cfg!(target_pointer_width = "64") {
        64
    } else {
        52
    };
    file.read_exact(&mut header[..header_len])
        .map_err(|_| invalid("does not contain a complete native ELF header"))?;
    if !is_native_linux_elf(&header[..header_len]) {
        return Err(invalid(
            "must be a native Linux ELF executable for this architecture",
        ));
    }
    let final_metadata =
        fs::symlink_metadata(agent).map_err(|_| invalid("changed during preflight"))?;
    if !final_metadata.is_file()
        || final_metadata.file_type().is_symlink()
        || metadata.dev() != final_metadata.dev()
        || metadata.ino() != final_metadata.ino()
        || final_metadata.permissions().mode() & 0o111 == 0
    {
        return Err(invalid("changed during preflight"));
    }
    Ok(())
}

fn native_elf_machine() -> Option<u16> {
    match std::env::consts::ARCH {
        "x86" => Some(3),
        "mips" | "mips32r6" | "mips64" | "mips64r6" => Some(8),
        "powerpc" => Some(20),
        "powerpc64" => Some(21),
        "s390x" => Some(22),
        "arm" => Some(40),
        "sparc64" => Some(43),
        "x86_64" => Some(62),
        "aarch64" => Some(183),
        "riscv32" | "riscv64" => Some(243),
        "loongarch64" => Some(258),
        _ => None,
    }
}

fn is_native_linux_elf(header: &[u8]) -> bool {
    let class = if cfg!(target_pointer_width = "64") {
        2
    } else {
        1
    };
    let endian = if cfg!(target_endian = "little") { 1 } else { 2 };
    let expected_len = if class == 2 { 64 } else { 52 };
    if header.len() < expected_len
        || &header[..4] != b"\x7fELF"
        || header[4] != class
        || header[5] != endian
        || header[6] != 1
        || !matches!(header[7], 0 | 3)
    {
        return false;
    }
    let kind = u16::from_ne_bytes([header[16], header[17]]);
    let machine = u16::from_ne_bytes([header[18], header[19]]);
    let version = u32::from_ne_bytes([header[20], header[21], header[22], header[23]]);
    matches!(kind, 2 | 3) && Some(machine) == native_elf_machine() && version == 1
}

fn validate_bundled_linux_handshake(handshake: &Payload, root: &str) -> Result<(), String> {
    validate_handshake(handshake)?;
    let Payload::Hello {
        root: reported_root,
        agent: Some(agent),
        ..
    } = handshake
    else {
        return Err("invalid_agent_info: bundled Linux agent must advertise metadata".into());
    };
    if agent.os != "linux"
        || agent.arch != std::env::consts::ARCH
        || agent.version != env!("CARGO_PKG_VERSION")
    {
        return Err("bundled_agent_mismatch: bundled agent must match Linux, this client's architecture, and its exact package version".into());
    }
    if reported_root != root {
        return Err(
            "bundled_agent_root_mismatch: agent reported a different workspace root".into(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests;
