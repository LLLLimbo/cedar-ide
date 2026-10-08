//! Nonshipping, deterministic lost-reply peer for generated acceptance roots.
//! File operations and revisions use the actual Workspace implementation.
//! The first successful Write commits before this process exits without its
//! response. A new process reads those same bytes; no request is replayed.
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
const MAX_OPERATIONS: usize = 256;

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
        let mut result = if allowed {
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
        if name == "Write" && matches!(result, Ok(Payload::Written { .. })) && !committed {
            let mut counter = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(root.join(COMMITTED))?;
            counter.write_all(b"1\n")?;
            counter.sync_all()?;
            // Normal Workspace Write has already completed and replaced the
            // real file. Intentionally omit the successful Written frame.
            return Ok(());
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
