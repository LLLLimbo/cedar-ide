//! Nonshipping, deterministic lost-reply peer for generated acceptance roots.
//! File operations and revisions use the actual Workspace implementation.
//! By default the first successful Write commits before this process exits
//! without its response. Explicit, bounded synthetic ack modes also exercise
//! malformed Written frames. No mode enables execution or request replay.
use cedar_protocol::{read_frame, write_frame, Operation, Payload, RemoteError, Request, Response};
use cedar_workspace::Workspace;
use std::{
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

const MARKER: &str = ".cedar-interrupted-save-validation";
const MARKER_TEXT: &[u8] = b"cedar-interrupted-save-validation-v1\nsynthetic-data-only\n";
const OPERATIONS: &str = ".cedar-interrupted-save-operations";
const COMMITTED: &str = ".cedar-interrupted-save-committed";
const READ_MODE: &str = ".cedar-interrupted-save-read-mode";
const SAVE_ACK_MODE: &str = ".cedar-synthetic-save-ack-mode";
const MAX_OPERATIONS: usize = 256;

#[derive(Clone, Copy, PartialEq, Eq)]
enum SaveAckMode {
    LostReply,
    Missing,
    Empty,
    Oversized,
    Noncanonical,
    WrongDigest,
    NotCommitted,
}

fn save_ack_mode(root: &Path) -> io::Result<SaveAckMode> {
    match bounded_file(&root.join(SAVE_ACK_MODE), 64)?.as_deref() {
        None => Ok(SaveAckMode::LostReply),
        Some(b"synthetic-missing\n") => Ok(SaveAckMode::Missing),
        Some(b"synthetic-empty\n") => Ok(SaveAckMode::Empty),
        Some(b"synthetic-oversized\n") => Ok(SaveAckMode::Oversized),
        Some(b"synthetic-noncanonical\n") => Ok(SaveAckMode::Noncanonical),
        Some(b"synthetic-wrong-digest\n") => Ok(SaveAckMode::WrongDigest),
        Some(b"synthetic-not-committed\n") => Ok(SaveAckMode::NotCommitted),
        _ => Err(io::Error::other(
            "unknown synthetic save acknowledgement mode",
        )),
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("cedar-agent-interrupted-save-validation: {error}");
        std::process::exit(1);
    }
}

fn bounded_file(path: &Path, limit: usize) -> io::Result<Option<Vec<u8>>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {}
        Ok(_) => return Err(io::Error::other("fixture control must be a regular file")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    }
    let mut contents = Vec::new();
    fs::File::open(path)?
        .take(limit as u64 + 1)
        .read_to_end(&mut contents)?;
    if contents.len() > limit {
        return Err(io::Error::other("fixture control exceeds its bound"));
    }
    Ok(Some(contents))
}

fn log_operation(root: &Path, name: &str) -> io::Result<()> {
    let path = root.join(OPERATIONS);
    let previous = bounded_file(&path, MAX_OPERATIONS * 9)?.unwrap_or_default();
    if previous.split(|byte| *byte == b'\n').count() > MAX_OPERATIONS {
        return Err(io::Error::other("fixture operation limit reached"));
    }
    let mut log = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(log, "{name}")?;
    log.sync_all()
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new("--root")) {
        return Err("only --root PATH is accepted".into());
    }
    let root = PathBuf::from(args.next().ok_or("--root requires a directory")?);
    // In particular, reject --allow-run. This fixture never launches tools,
    // invokes a shell, connects to a network, or accepts arbitrary file paths.
    if args.next().is_some() {
        return Err("only --root PATH is accepted; execution is never enabled".into());
    }
    let root = fs::canonicalize(root)?;
    if bounded_file(&root.join(MARKER), MARKER_TEXT.len())?.as_deref() != Some(MARKER_TEXT) {
        return Err("an exactly marked, newly generated synthetic root is required".into());
    }
    let committed = match bounded_file(&root.join(COMMITTED), 2)?.as_deref() {
        None => false,
        Some(b"1\n") => true,
        _ => return Err("invalid fixture commit counter".into()),
    };
    let save_ack_mode = save_ack_mode(&root)?;
    let mut handled_write = committed;
    let mut workspace = Workspace::open(&root)?;
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader = stdin.lock();
    let mut writer = stdout.lock();
    while let Some(request) = read_frame::<_, Request>(&mut reader)? {
        let (name, allowed) = match &request.op {
            Operation::Hello => ("Hello", true),
            Operation::List { path } => ("List", path.is_empty()),
            Operation::Read { path } => ("Read", path == "draft.txt"),
            Operation::Write { path, .. } => ("Write", path == "draft.txt"),
            _ => ("Rejected", false),
        };
        log_operation(&root, name)?;
        let synthetic_noncommit = allowed
            && name == "Write"
            && !handled_write
            && save_ack_mode == SaveAckMode::NotCommitted;
        let mut result = if synthetic_noncommit {
            // Intentionally claim success without invoking Workspace::write.
            // This single controlled case leaves the original real bytes intact.
            Ok(Payload::Written {
                revision: String::new(),
            })
        } else if allowed {
            workspace.handle(request.op)
        } else {
            Err(RemoteError::new(
                "fixture_operation_disabled",
                "Only Hello, root List, and generated draft.txt Read/Write are accepted",
            ))
        };
        if let Ok(Payload::Hello {
            agent: Some(info), ..
        }) = &mut result
        {
            info.capabilities = ["list", "read", "write"]
                .into_iter()
                .map(str::to_owned)
                .collect();
        }
        if name == "Write" && matches!(result, Ok(Payload::Written { .. })) && !handled_write {
            handled_write = true;
            if !synthetic_noncommit {
                let mut counter = OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(root.join(COMMITTED))?;
                counter.write_all(b"1\n")?;
                counter.sync_all()?;
            }
            if let Ok(Payload::Written { revision }) = &mut result {
                match save_ack_mode {
                    // Normal Workspace Write has already replaced the real file.
                    SaveAckMode::LostReply => return Ok(()),
                    SaveAckMode::Missing => {
                        // The real Client decoder must reject the missing required
                        // field. Only a numeric request ID enters this fixed frame.
                        writeln!(
                            writer,
                            "{{\"id\":{},\"result\":{{\"Ok\":{{\"type\":\"written\"}}}}}}",
                            request.id
                        )?;
                        writer.flush()?;
                        return Ok(());
                    }
                    SaveAckMode::Empty | SaveAckMode::NotCommitted => revision.clear(),
                    SaveAckMode::Oversized => *revision = "0".repeat(65),
                    SaveAckMode::Noncanonical => {
                        revision.make_ascii_uppercase();
                        // Even the all-digit SHA-256 corner case must be an
                        // unambiguously noncanonical synthetic response.
                        if revision.bytes().all(|byte| byte.is_ascii_digit()) {
                            revision.replace_range(..1, "A");
                        }
                    }
                    SaveAckMode::WrongDigest => {
                        *revision = if revision.bytes().all(|byte| byte == b'0') {
                            "1".repeat(64)
                        } else {
                            "0".repeat(64)
                        };
                    }
                }
            }
        }
        if name == "Read" {
            let mode = bounded_file(&root.join(READ_MODE), 32)?.unwrap_or_default();
            if let Ok(Payload::File {
                path,
                text,
                revision,
            }) = &mut result
            {
                match mode.as_slice() {
                    b"" => {}
                    b"bad-revision\n" => *revision = "not-a-revision".into(),
                    b"wrong-digest\n" => *revision = "0".repeat(64),
                    b"nul-content\n" => *text = "generated\0text".into(),
                    b"oversize-content\n" => {
                        *text = "x".repeat(cedar_protocol::MAX_FILE_BYTES + 1);
                    }
                    b"wrong-path\n" => *path = "other.txt".into(),
                    _ => return Err("unknown synthetic read mode".into()),
                }
            }
        }
        write_frame(
            &mut writer,
            &Response {
                id: request.id,
                result,
            },
        )?;
    }
    Ok(())
}
