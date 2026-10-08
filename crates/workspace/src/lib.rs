//! Workspace operations shared by the local client and the stdio agent.
//!
//! Paths are relative, validated, and confined to a canonical root. Symlinks are
//! deliberately rejected, including symlinks whose current target is in-root.
//! This is defense against accidental traversal, NOT an OS security sandbox:
//! another process with write access can rename directories between checks and
//! access. Revision checking also cannot provide atomic compare-and-swap against
//! arbitrary external writers. Use a trusted workspace and an OS sandbox/account
//! boundary for hostile filesystems. Enabled commands have the account's full
//! permissions, not merely workspace access.

mod capabilities;
mod git_read;
#[cfg(feature = "windows-java-gc-diagnostic")]
mod java_gc_diagnostic;
mod java_launch;
#[cfg(feature = "windows-language-validation")]
mod java_validation;
mod language;
mod tasks;

#[cfg(feature = "windows-java-gc-diagnostic")]
pub use java_gc_diagnostic::{
    WINDOWS_JAVA_GC_DIAGNOSTIC_DISTRIBUTION_MARKER,
    WINDOWS_JAVA_GC_DIAGNOSTIC_DISTRIBUTION_MARKER_CONTENTS, WINDOWS_JAVA_GC_DIAGNOSTIC_MARKER,
    WINDOWS_JAVA_GC_DIAGNOSTIC_MARKER_CONTENTS, WINDOWS_JAVA_GC_DIAGNOSTIC_OPTION,
};

#[cfg(feature = "windows-language-validation")]
pub use java_validation::{
    WINDOWS_JAVA_EVIDENCE_FILE, WINDOWS_JAVA_VALIDATION_MARKER,
    WINDOWS_JAVA_VALIDATION_MARKER_CONTENTS,
};

use cedar_protocol::{
    Entry, Operation, Payload, RemoteError, SearchMatch, MAX_FILE_BYTES, PROTOCOL_VERSION,
};
pub use cedar_tasks::BackendMode;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

pub const MAX_COMMAND_OUTPUT_BYTES: usize = 256 * 1024;
pub const MAX_COMMAND_TIMEOUT_SECS: u64 = 300;
pub const MAX_SEARCH_RESULTS: usize = 1000;
const MAX_SEARCH_ENTRIES: usize = 20_000;
const MAX_SEARCH_BYTES: usize = 32 * 1024 * 1024;
const MAX_SEARCH_LINE_BYTES: usize = 2048;
const MAX_SEARCH_RESPONSE_BYTES: usize = 512 * 1024;
const MAX_SEARCH_DURATION: Duration = Duration::from_secs(5);
const MAX_LIST_ENTRIES: usize = 4096;
const MAX_LIST_TEXT_BYTES: usize = 512 * 1024;
const MAX_PATH_BYTES: usize = 4096;
const MAX_SEARCH_DEPTH: usize = 64;

#[derive(Debug)]
pub struct Workspace {
    root: PathBuf,
    // A host construction choice, never a wire operation or execution grant.
    backend_mode: BackendMode,
    allow_run: bool,
    #[cfg(feature = "windows-language-validation")]
    windows_language_validation: bool,
    #[cfg(feature = "windows-language-validation")]
    windows_java_validation: Option<java_validation::JavaValidationProfile>,
    #[cfg(feature = "windows-java-gc-diagnostic")]
    windows_java_gc_diagnostic: Option<java_gc_diagnostic::JavaGcDiagnosticProfile>,
    language: Option<language::LanguageSession>,
    tasks: Option<cedar_tasks::TaskManager>,
}

impl Workspace {
    pub fn open(root: impl AsRef<Path>) -> Result<Self, RemoteError> {
        Self::with_backend_mode(root, BackendMode::InProcess)
    }

    /// Select the host's process-ownership implementation before serving peers.
    /// `IsolatedAgent` is only for a controlled-spawning agent host; it does not
    /// grant workspace execution trust, which must still be enabled separately.
    pub fn with_backend_mode(
        root: impl AsRef<Path>,
        backend_mode: BackendMode,
    ) -> Result<Self, RemoteError> {
        let root = fs::canonicalize(root).map_err(io_error)?;
        if !root.is_dir() {
            return Err(error("not_directory", "Workspace root must be a directory"));
        }
        Ok(Self {
            root,
            backend_mode,
            allow_run: false,
            #[cfg(feature = "windows-language-validation")]
            windows_language_validation: false,
            #[cfg(feature = "windows-language-validation")]
            windows_java_validation: None,
            #[cfg(feature = "windows-java-gc-diagnostic")]
            windows_java_gc_diagnostic: None,
            language: None,
            tasks: None,
        })
    }

    /// Nonshipping Windows language acceptance host, never a user workspace API.
    ///
    /// Requires an explicitly marked synthetic root and fixes process ownership
    /// to IsolatedAgent. It does not grant execution trust or change Hello's
    /// production capability claims. The marker is an opt-in, not a sandbox.
    #[cfg(feature = "windows-language-validation")]
    pub fn for_windows_language_validation(root: impl AsRef<Path>) -> Result<Self, RemoteError> {
        let mut workspace = Self::with_backend_mode(root, BackendMode::IsolatedAgent)?;
        let marker = workspace.resolve(".cedar-windows-language-validation", false)?;
        if !fs::metadata(&marker).map_err(io_error)?.is_file() {
            return Err(error(
                "invalid_validation_root",
                "Validation marker must be a regular file",
            ));
        }
        let expected = b"cedar-windows-language-validation-v1\n";
        let mut contents = Vec::new();
        File::open(marker)
            .map_err(io_error)?
            .take(expected.len() as u64 + 1)
            .read_to_end(&mut contents)
            .map_err(io_error)?;
        if contents != expected {
            return Err(error(
                "invalid_validation_root",
                "Expected a marked synthetic validation root",
            ));
        }
        workspace.windows_language_validation = true;
        Ok(workspace)
    }

    /// Nonshipping, marked-root Java fixture. Does not grant execution trust,
    /// change Hello's production support claims, or change ordinary constructors.
    #[cfg(feature = "windows-language-validation")]
    pub fn for_windows_java_validation(
        root: impl AsRef<Path>,
        distribution: impl AsRef<Path>,
    ) -> Result<Self, RemoteError> {
        let mut workspace = Self::for_windows_language_validation(root)?;
        java_validation::require_marker(&workspace)?;
        workspace.windows_java_validation = Some(java_validation::JavaValidationProfile::new(
            &workspace,
            distribution.as_ref(),
        )?);
        Ok(workspace)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Nonshipping fixed GC diagnostic host for a newly marked synthetic root.
    ///
    /// Uses the ordinary Java operation and production recipe, with exactly one
    /// bounded GC log option. The selected distribution must separately opt in
    /// before the single launch attempt. This grants no execution trust and does
    /// not enable the generic language or Java validation fixture paths.
    #[cfg(feature = "windows-java-gc-diagnostic")]
    pub fn for_windows_java_gc_diagnostic(root: impl AsRef<Path>) -> Result<Self, RemoteError> {
        let mut workspace = Self::with_backend_mode(root, BackendMode::IsolatedAgent)?;
        workspace.windows_java_gc_diagnostic = Some(
            java_gc_diagnostic::JavaGcDiagnosticProfile::new(&workspace.root)?,
        );
        Ok(workspace)
    }

    /// Explicitly allow arbitrary programs with the current account's authority.
    pub fn set_allow_run(&mut self, allow: bool) {
        self.allow_run = allow;
    }

    pub fn handle(&mut self, op: Operation) -> Result<Payload, RemoteError> {
        match op {
            Operation::Hello => Ok(Payload::Hello {
                protocol: PROTOCOL_VERSION,
                root: self.root.to_string_lossy().into_owned(),
                agent: Some(capabilities::agent_info(self.backend_mode)),
            }),
            Operation::List { path } => self.list(&path),
            Operation::Read { path } => self.read(&path),
            Operation::Write {
                path,
                text,
                expected_revision,
            } => self.write(&path, &text, expected_revision.as_deref()),
            Operation::Search { query, limit } => self.search(&query, limit),
            Operation::GitStatus => {
                // Git can run repository-configured clean/process filters even
                // during status. Disabling fsmonitor alone is not a trust boundary.
                if !self.allow_run {
                    return Err(error("run_disabled", "Git status can execute repository-configured programs; enable workspace execution trust (--allow-run) first"));
                }
                self.git_status()
            }
            op @ (Operation::GitChanges { .. } | Operation::GitDiff { .. }) => {
                self.handle_git_read(op)
            }
            op @ (Operation::LanguageStart { .. }
            | Operation::LanguageStartJava { .. }
            | Operation::LanguageOpen { .. }
            | Operation::LanguageChange { .. }
            | Operation::LanguageClose { .. }
            | Operation::LanguageQuery { .. }
            | Operation::LanguageFormat { .. }
            | Operation::LanguageReferences { .. }
            | Operation::LanguageDocumentSymbols { .. }
            | Operation::LanguageResolveUri { .. }
            | Operation::LanguageResolveCompletion { .. }
            | Operation::LanguageEvents
            | Operation::LanguageStop) => self.handle_language(op),
            op @ (Operation::RunStart { .. }
            | Operation::RunPoll { .. }
            | Operation::RunCancel { .. }) => self.handle_task(op),
            Operation::Run {
                program,
                args,
                timeout_secs,
            } => {
                if !self.allow_run {
                    return Err(error("run_disabled", "Command execution is disabled; start the agent with --allow-run to enable it"));
                }
                validate_command(&program, &args, timeout_secs)?;
                let mut command = Command::new(program);
                command.args(args).current_dir(&self.root);
                let result = run_bounded(command, Duration::from_secs(timeout_secs))?;
                Ok(Payload::Run {
                    stdout: result.stdout,
                    stderr: result.stderr,
                    exit_code: result.exit_code,
                    timed_out: result.timed_out,
                    truncated: result.truncated,
                })
            }
        }
    }

    fn resolve(&self, path: &str, allow_missing_leaf: bool) -> Result<PathBuf, RemoteError> {
        let relative = validate_path(path)?;
        let mut current = self.root.clone();
        let components: Vec<_> = relative.components().collect();
        for (i, part) in components.iter().enumerate() {
            if let Component::Normal(name) = part {
                current.push(name);
            } else {
                continue;
            }
            match fs::symlink_metadata(&current) {
                Ok(meta) => {
                    if meta.file_type().is_symlink() {
                        return Err(error("invalid_path", "Symlink paths are not supported"));
                    }
                    let canonical = fs::canonicalize(&current).map_err(io_error)?;
                    if !canonical.starts_with(&self.root) {
                        return Err(error("invalid_path", "Path escapes the workspace root"));
                    }
                    if i + 1 != components.len() && !meta.is_dir() {
                        return Err(error("not_directory", "A parent path is not a directory"));
                    }
                }
                Err(e)
                    if e.kind() == io::ErrorKind::NotFound
                        && allow_missing_leaf
                        && i + 1 == components.len() => {}
                Err(e) => return Err(io_error(e)),
            }
        }
        Ok(current)
    }

    fn list(&self, path: &str) -> Result<Payload, RemoteError> {
        let dir = self.resolve(path, false)?;
        if !dir.is_dir() {
            return Err(error("not_directory", "Path is not a directory"));
        }
        let mut entries = Vec::new();
        let mut text_bytes = 0usize;
        for entry in fs::read_dir(&dir).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            let kind = entry.file_type().map_err(io_error)?;
            // Never advertise links or special files as safe, editable entries.
            if kind.is_symlink() || !(kind.is_file() || kind.is_dir()) {
                continue;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let relative = self.relative_text(&entry.path())?;
            if validate_path(&relative).is_err() {
                continue;
            }
            text_bytes += name.len() + relative.len();
            if entries.len() == MAX_LIST_ENTRIES || text_bytes > MAX_LIST_TEXT_BYTES {
                return Err(error(
                    "directory_too_large",
                    "Directory listing exceeds the bounded response limit",
                ));
            }
            entries.push(Entry {
                path: relative,
                name,
                is_dir: kind.is_dir(),
            });
        }
        entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.cmp(&b.name)));
        Ok(Payload::Entries { entries })
    }

    fn read(&self, path: &str) -> Result<Payload, RemoteError> {
        let full = self.resolve(path, false)?;
        let text = read_text(&full)?;
        Ok(Payload::File {
            path: self.relative_text(&full)?,
            revision: revision(&text),
            text,
        })
    }

    fn write(
        &self,
        path: &str,
        text: &str,
        expected: Option<&str>,
    ) -> Result<Payload, RemoteError> {
        self.write_with_preparation_hook(path, text, expected, || {})
    }

    // A private, per-call seam lets tests change the destination after all
    // temporary-file preparation, without sleeps or a process-global hook.
    fn write_with_preparation_hook(
        &self,
        path: &str,
        text: &str,
        expected: Option<&str>,
        after_preparation: impl FnOnce(),
    ) -> Result<Payload, RemoteError> {
        if text.len() > MAX_FILE_BYTES {
            return Err(error(
                "file_too_large",
                "Files are limited to 1 MiB of UTF-8 text",
            ));
        }
        if text.contains('\0') {
            return Err(error(
                "binary_file",
                "Text files must not contain NUL bytes",
            ));
        }
        let full = self.resolve(path, true)?;
        if full == self.root {
            return Err(error("invalid_path", "A file path is required"));
        }
        let existing = match fs::symlink_metadata(&full) {
            Ok(metadata) => Some(metadata),
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(io_error(e)),
        };
        match (&existing, expected) {
            (Some(_), None) => {
                return Err(error(
                    "conflict",
                    "File already exists; read it before replacing it",
                ))
            }
            (None, Some(_)) => {
                return Err(error(
                    "conflict",
                    "The file was removed or moved after it was read",
                ))
            }
            (Some(meta), Some(wanted)) => {
                if !meta.is_file() || meta.file_type().is_symlink() {
                    return Err(error("invalid_path", "Only regular files can be replaced"));
                }
                if revision(&read_text(&full)?) != wanted {
                    return Err(error(
                        "conflict",
                        "File changed on disk; reload before saving",
                    ));
                }
                if meta.permissions().readonly() {
                    return Err(error("permission_denied", "File is read-only"));
                }
            }
            (None, None) => {}
        }
        let parent = full
            .parent()
            .ok_or_else(|| error("invalid_path", "A file parent is required"))?;
        let mut temporary = tempfile::Builder::new()
            .prefix(".cedar-save-")
            .tempfile_in(parent)
            .map_err(io_error)?;
        temporary.write_all(text.as_bytes()).map_err(io_error)?;
        temporary.flush().map_err(io_error)?;
        if let Some(meta) = &existing {
            temporary
                .as_file()
                .set_permissions(meta.permissions())
                .map_err(io_error)?;
        }
        if let Some(wanted) = expected {
            let temporary = PreparedReplacement::new(temporary).map_err(io_error)?;
            after_preparation();
            // All attribute changes and flushing precede these final checks.
            // Commit immediately afterward: this narrows races, but is not an
            // atomic filesystem CAS against non-cooperating external writers.
            self.resolve(path, true)?;
            let latest = read_text(&full).map_err(|e| {
                if e.code == "not_found" {
                    error("conflict", "The file was removed while saving")
                } else {
                    e
                }
            })?;
            if revision(&latest) != wanted {
                return Err(error(
                    "conflict",
                    "File changed while saving; reload before saving",
                ));
            }
            temporary.persist(&full).map_err(io_error)?;
        } else {
            temporary.as_file().sync_all().map_err(io_error)?;
            after_preparation();
            self.resolve(path, true)?;
            temporary.persist_noclobber(&full).map_err(|e| {
                if e.error.kind() == io::ErrorKind::AlreadyExists {
                    error("conflict", "File was created while saving")
                } else {
                    io_error(e.error)
                }
            })?;
        }
        // Directory sync improves rename durability on Unix; failure does not
        // turn a committed save into an ambiguous error response.
        #[cfg(unix)]
        if let Ok(directory) = File::open(parent) {
            let _ = directory.sync_all();
        }
        Ok(Payload::Written {
            revision: revision(text),
        })
    }

    fn search(&self, query: &str, requested_limit: usize) -> Result<Payload, RemoteError> {
        if query.is_empty() || query.len() > 4096 || query.contains(['\n', '\r', '\0']) {
            return Err(error(
                "invalid_query",
                "Search requires a nonempty, single-line query of at most 4096 bytes",
            ));
        }
        if requested_limit == 0 {
            return Err(error(
                "invalid_limit",
                "Search result limit must be positive",
            ));
        }
        let limit = requested_limit.min(MAX_SEARCH_RESULTS);
        let started = Instant::now();
        let mut matches = Vec::new();
        let mut stack = vec![(self.root.clone(), 0usize)];
        let mut scanned_entries = 0;
        let mut scanned_bytes = 0;
        let mut response_bytes = 0;
        let mut truncated = false;
        'walk: while let Some((dir, depth)) = stack.pop() {
            if started.elapsed() > MAX_SEARCH_DURATION {
                truncated = true;
                break;
            }
            let entries = match fs::read_dir(&dir) {
                Ok(entries) => entries,
                Err(_) => {
                    truncated = true;
                    continue;
                }
            };
            // read_dir is streamed so one huge directory cannot allocate freely.
            for entry in entries {
                scanned_entries += 1;
                if scanned_entries > MAX_SEARCH_ENTRIES || started.elapsed() > MAX_SEARCH_DURATION {
                    truncated = true;
                    break 'walk;
                }
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(_) => {
                        truncated = true;
                        continue;
                    }
                };
                let kind = match entry.file_type() {
                    Ok(kind) => kind,
                    Err(_) => {
                        truncated = true;
                        continue;
                    }
                };
                if kind.is_symlink() {
                    continue;
                }
                if kind.is_dir() {
                    if skipped_search_dir(&entry.file_name().to_string_lossy()) {
                        continue;
                    }
                    if depth >= MAX_SEARCH_DEPTH {
                        truncated = true;
                        continue;
                    }
                    if let Ok(relative) = self.relative_text(&entry.path()) {
                        if let Ok(safe) = self.resolve(&relative, false) {
                            stack.push((safe, depth + 1));
                        }
                    }
                    continue;
                }
                if !kind.is_file() {
                    continue;
                }
                let path = match self.relative_text(&entry.path()) {
                    Ok(path) => path,
                    Err(_) => continue,
                };
                let full = match self.resolve(&path, false) {
                    Ok(full) => full,
                    Err(_) => continue,
                };
                let meta = match fs::metadata(&full) {
                    Ok(meta) => meta,
                    Err(_) => {
                        truncated = true;
                        continue;
                    }
                };
                if meta.len() > MAX_FILE_BYTES as u64 {
                    continue;
                }
                if scanned_bytes + meta.len() as usize > MAX_SEARCH_BYTES {
                    truncated = true;
                    break 'walk;
                }
                let text = match read_text(&full) {
                    Ok(text) => text,
                    Err(e)
                        if e.code == "invalid_utf8"
                            || e.code == "binary_file"
                            || e.code == "file_too_large" =>
                    {
                        continue
                    }
                    Err(_) => {
                        truncated = true;
                        continue;
                    }
                };
                scanned_bytes += text.len();
                if scanned_bytes > MAX_SEARCH_BYTES {
                    truncated = true;
                    break 'walk;
                }
                for (line, value) in text.lines().enumerate() {
                    if !value.contains(query) {
                        continue;
                    }
                    if matches.len() == limit {
                        truncated = true;
                        break 'walk;
                    }
                    let preview = search_preview(value, query);
                    response_bytes += path.len() + preview.len();
                    if response_bytes > MAX_SEARCH_RESPONSE_BYTES {
                        truncated = true;
                        break 'walk;
                    }
                    matches.push(SearchMatch {
                        path: path.clone(),
                        line: line + 1,
                        text: preview,
                    });
                }
            }
        }
        matches.sort_by(|a, b| a.path.cmp(&b.path).then_with(|| a.line.cmp(&b.line)));
        Ok(Payload::Matches { matches, truncated })
    }

    fn git_status(&self) -> Result<Payload, RemoteError> {
        let mut command = Command::new("git");
        command
            .current_dir(&self.root)
            .args([
                "--no-pager",
                "--no-optional-locks",
                "-c",
                "core.fsmonitor=false",
                "-c",
                "core.untrackedCache=false",
                "status",
                "--porcelain=v1",
                "--branch",
                "--ignore-submodules=all",
                "--untracked-files=normal",
                "--",
                ".",
            ])
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_COUNT", "0")
            .env(
                "GIT_CONFIG_GLOBAL",
                if cfg!(windows) { "NUL" } else { "/dev/null" },
            );
        for key in [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_INDEX_FILE",
            "GIT_COMMON_DIR",
            "GIT_OBJECT_DIRECTORY",
            "GIT_ALTERNATE_OBJECT_DIRECTORIES",
            "GIT_CONFIG",
            "GIT_CONFIG_PARAMETERS",
        ] {
            command.env_remove(key);
        }
        let result = run_bounded(command, Duration::from_secs(10))?;
        if result.timed_out {
            return Err(error(
                "command_timeout",
                "Git status exceeded its 10-second limit",
            ));
        }
        if result.truncated {
            return Err(error(
                "output_limit",
                "Git status exceeded the output limit",
            ));
        }
        if result.exit_code != Some(0) {
            return Err(error("git_error", result.stderr.trim()));
        }
        Ok(Payload::GitStatus {
            text: result.stdout,
        })
    }

    fn relative_text(&self, path: &Path) -> Result<String, RemoteError> {
        let relative = path
            .strip_prefix(&self.root)
            .map_err(|_| error("invalid_path", "Path escapes workspace"))?;
        let text = relative
            .to_str()
            .ok_or_else(|| error("invalid_path", "Paths must be UTF-8"))?;
        Ok(if cfg!(windows) {
            text.replace('\\', "/")
        } else {
            text.to_owned()
        })
    }
}

/// An existing-file replacement that needs no more preparation before commit.
/// Kept private to workspace saves; new files still use persist_noclobber.
struct PreparedReplacement {
    temporary: tempfile::NamedTempFile,
}

impl PreparedReplacement {
    fn new(temporary: tempfile::NamedTempFile) -> io::Result<Self> {
        #[cfg(windows)]
        let temporary = {
            // keep clears FILE_ATTRIBUTE_TEMPORARY. Re-arm cleanup immediately
            // so a failed flush, late validation, or rename removes our source.
            // The path is absolute (the workspace root is canonical), so
            // try_from_path needs no current-directory lookup or filesystem I/O.
            let (file, path) = temporary.keep().map_err(|error| error.error)?;
            let cleanup = tempfile::TempPath::try_from_path(path)?;
            tempfile::NamedTempFile::from_parts(file, cleanup)
        };
        temporary.as_file().sync_all()?;
        Ok(Self { temporary })
    }

    fn persist(self, destination: &Path) -> io::Result<()> {
        #[cfg(windows)]
        {
            // tempfile 3.27 uses legacy MoveFileExW. Rust 1.99's maintained
            // rename has a POSIX-semantics fallback for delete-sharing readers;
            // it does not bypass read-only attributes or sharing restrictions.
            fs::rename(self.temporary.path(), destination)?;
            let mut temporary = self.temporary;
            temporary.disable_cleanup(true);
        }
        #[cfg(not(windows))]
        self.temporary
            .persist(destination)
            .map_err(|error| error.error)?;
        Ok(())
    }
}

fn validate_path(path: &str) -> Result<&Path, RemoteError> {
    if path.len() > MAX_PATH_BYTES
        || path.contains(['\0', '\\', ':'])
        || path.starts_with('/')
        || path.split('/').any(|s| s == "..")
    {
        return Err(error(
            "invalid_path",
            "Use a relative workspace path without traversal, drive prefixes, or backslashes",
        ));
    }
    let path = Path::new(path);
    if path
        .components()
        .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        return Err(error(
            "invalid_path",
            "Path must be relative to the workspace",
        ));
    }
    Ok(path)
}

fn read_text(path: &Path) -> Result<String, RemoteError> {
    let metadata = fs::symlink_metadata(path).map_err(io_error)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(error("invalid_path", "Only regular files can be read"));
    }
    if metadata.len() > MAX_FILE_BYTES as u64 {
        return Err(error("file_too_large", "Files are limited to 1 MiB"));
    }
    let file = File::open(path).map_err(io_error)?;
    let mut bytes = Vec::new();
    file.take((MAX_FILE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() > MAX_FILE_BYTES {
        return Err(error("file_too_large", "Files are limited to 1 MiB"));
    }
    let text =
        String::from_utf8(bytes).map_err(|_| error("invalid_utf8", "File is not valid UTF-8"))?;
    if text.contains('\0') {
        return Err(error(
            "binary_file",
            "Binary files containing NUL bytes cannot be edited",
        ));
    }
    Ok(text)
}

fn revision(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}
fn error(code: &str, message: impl Into<String>) -> RemoteError {
    RemoteError::new(code, message)
}
fn io_error(e: io::Error) -> RemoteError {
    let code = match e.kind() {
        io::ErrorKind::NotFound => "not_found",
        io::ErrorKind::PermissionDenied => "permission_denied",
        io::ErrorKind::AlreadyExists => "conflict",
        _ => "io_error",
    };
    error(code, e.to_string())
}
fn skipped_search_dir(name: &str) -> bool {
    matches!(
        name,
        ".git"
            | "target"
            | "build"
            | "dist"
            | "node_modules"
            | ".idea"
            | ".venv"
            | "venv"
            | "__pycache__"
    )
}
fn search_preview(line: &str, query: &str) -> String {
    if line.len() <= MAX_SEARCH_LINE_BYTES {
        return line.to_owned();
    }
    let found = line.find(query).unwrap_or(0);
    let mut start = found.saturating_sub(MAX_SEARCH_LINE_BYTES / 4);
    while !line.is_char_boundary(start) {
        start += 1;
    }
    let mut end = (start + MAX_SEARCH_LINE_BYTES).min(line.len());
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    format!(
        "{}{}{}",
        if start > 0 { "…" } else { "" },
        &line[start..end],
        if end < line.len() { "…" } else { "" }
    )
}
fn validate_command(program: &str, args: &[String], timeout_secs: u64) -> Result<(), RemoteError> {
    if program.is_empty()
        || program.len() > 4096
        || program.contains('\0')
        || args.len() > 256
        || args.iter().any(|s| s.contains('\0'))
        || args.iter().map(String::len).sum::<usize>() > 64 * 1024
    {
        return Err(error(
            "invalid_command",
            "Program or arguments exceed supported bounds",
        ));
    }
    if timeout_secs == 0 || timeout_secs > MAX_COMMAND_TIMEOUT_SECS {
        return Err(error(
            "invalid_timeout",
            format!("Command timeout must be 1–{MAX_COMMAND_TIMEOUT_SECS} seconds"),
        ));
    }
    Ok(())
}

struct CommandResult {
    stdout: String,
    stderr: String,
    exit_code: Option<i32>,
    timed_out: bool,
    truncated: bool,
}
struct RawCommandResult {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    exit_code: Option<i32>,
    timed_out: bool,
    truncated: bool,
}
#[derive(Default)]
struct Capture {
    bytes: Vec<u8>,
    truncated: bool,
    read_error: bool,
}

/// Captures both pipes concurrently with bounded RAM. On Unix a private process
/// group is killed at timeout, output cap, and when its leader exits. Deliberate
/// setsid()/daemonization can escape a process group. Windows execution is
/// disabled until job-object containment and cancellable pipe readers exist.
/// Only Linux and macOS provide the currently verified wait/kill implementation.
fn run_bounded(command: Command, timeout: Duration) -> Result<CommandResult, RemoteError> {
    let result = run_bounded_raw(command, timeout)?;
    Ok(CommandResult {
        stdout: String::from_utf8_lossy(&result.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&result.stderr).into_owned(),
        exit_code: result.exit_code,
        timed_out: result.timed_out,
        truncated: result.truncated,
    })
}

// Strict identity protocols consume bytes; legacy command callers retain their
// existing lossy display behavior through the wrapper above.
fn run_bounded_raw(
    mut command: Command,
    timeout: Duration,
) -> Result<RawCommandResult, RemoteError> {
    if !cfg!(any(target_os = "linux", target_os = "macos")) {
        return Err(error("unsupported_platform", "Local command execution and Git status require the Linux/macOS process-containment implementation; use a Linux or macOS workspace agent over SSH"));
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command
        .spawn()
        .map_err(|e| error("command_failed", e.to_string()))?;
    let stdout = Arc::new(Mutex::new(Capture::default()));
    let stderr = Arc::new(Mutex::new(Capture::default()));
    let (done_tx, done_rx) = mpsc::channel();
    let out_pipe = child.stdout.take().expect("piped stdout");
    let err_pipe = child.stderr.take().expect("piped stderr");
    #[cfg(unix)]
    {
        if let Err(e) = nonblocking(&out_pipe).and_then(|_| nonblocking(&err_pipe)) {
            terminate(&mut child);
            let _ = child.wait();
            return Err(io_error(e));
        }
    }
    let stopped = Arc::new(AtomicBool::new(false));
    let out_thread = capture_pipe(out_pipe, stdout.clone(), stopped.clone(), done_tx.clone());
    let err_thread = capture_pipe(err_pipe, stderr.clone(), stopped.clone(), done_tx);
    let started = Instant::now();
    let mut timed_out = false;
    let mut truncated = false;
    let status_result = loop {
        if stdout.lock().unwrap().truncated || stderr.lock().unwrap().truncated {
            truncated = true;
            terminate(&mut child);
            break child.wait();
        }
        // Observe without reaping: the zombie leader reserves its PID until
        // group cleanup finishes, preventing a recycled PID from being signaled.
        match exited_without_reaping(&child) {
            Ok(true) => {
                terminate_descendants(&child);
                break child.wait();
            }
            Ok(false) => {}
            Err(e) => {
                // A missing waitable child (for example, an external SIGCHLD
                // reaper) makes group identity uncertain. Never signal an old
                // PID on this error path. Readers are cancelled below as usual.
                break Err(e);
            }
        }
        if started.elapsed() >= timeout {
            timed_out = true;
            terminate(&mut child);
            break child.wait();
        }
        thread::sleep(Duration::from_millis(10));
    };
    // An escaped descendant holding a pipe must not block the request forever.
    // Capture threads are nonblocking on Unix and stop on this bounded deadline.
    let drain_deadline = Instant::now() + Duration::from_millis(250);
    for _ in 0..2 {
        if done_rx
            .recv_timeout(drain_deadline.saturating_duration_since(Instant::now()))
            .is_err()
        {
            truncated = true;
            break;
        }
    }
    stopped.store(true, Ordering::Release);
    // On Unix pipes are nonblocking, so these readers always observe cancellation.
    #[cfg(unix)]
    {
        let _ = out_thread.join();
        let _ = err_thread.join();
    }
    #[cfg(not(unix))]
    {
        drop(out_thread);
        drop(err_thread);
    }
    let status = status_result.map_err(io_error)?;
    let out = stdout.lock().unwrap();
    let err = stderr.lock().unwrap();
    Ok(RawCommandResult {
        stdout: out.bytes.clone(),
        stderr: err.bytes.clone(),
        exit_code: status.code(),
        timed_out,
        truncated: truncated || out.truncated || err.truncated || out.read_error || err.read_error,
    })
}

fn capture_pipe<R: Read + Send + 'static>(
    mut pipe: R,
    capture: Arc<Mutex<Capture>>,
    stopped: Arc<AtomicBool>,
    done: mpsc::Sender<()>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut buffer = [0u8; 8192];
        while !stopped.load(Ordering::Acquire) {
            match pipe.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => {
                    let mut target = capture.lock().unwrap();
                    let available = MAX_COMMAND_OUTPUT_BYTES.saturating_sub(target.bytes.len());
                    target.bytes.extend_from_slice(&buffer[..n.min(available)]);
                    if n > available {
                        target.truncated = true;
                        break;
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5))
                }
                Err(_) => {
                    capture.lock().unwrap().read_error = true;
                    break;
                }
            }
        }
        let _ = done.send(());
    })
}

#[cfg(unix)]
fn nonblocking(pipe: &impl std::os::fd::AsRawFd) -> io::Result<()> {
    let fd = pipe.as_raw_fd();
    // SAFETY: the live pipe owns this descriptor; fcntl only updates its flags.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        if flags < 0 || libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

fn terminate(child: &mut Child) {
    terminate_descendants(child);
    let _ = child.kill();
}
fn terminate_descendants(child: &Child) {
    #[cfg(unix)]
    {
        // SAFETY: the child was created as leader of its own process group and
        // has not been reaped. Its PID therefore cannot identify a new process.
        // Negative PID targets that group; kill does not access pointers.
        unsafe {
            libc::kill(-(child.id() as i32), libc::SIGKILL);
        }
    }
    #[cfg(not(unix))]
    let _ = child;
}

/// The caller exclusively owns this child's wait state. Do not install a global
/// SIGCHLD reaper or SA_NOCLDWAIT while using workspace command execution.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn exited_without_reaping(child: &Child) -> io::Result<bool> {
    loop {
        // SAFETY: zero initializes siginfo_t's integer/pointer storage. waitid
        // receives a writable siginfo_t and selects only our owned child. The
        // WNOWAIT flag leaves its status and PID reserved for the later wait.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                child.id() as libc::id_t,
                &mut info,
                libc::WEXITED | libc::WNOWAIT | libc::WNOHANG,
            )
        };
        if result == 0 {
            // SAFETY: successful waitid initialized this status field; the
            // explicit initial zero also handles portable WNOHANG behavior.
            return Ok(unsafe { info.si_pid() } != 0);
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn exited_without_reaping(_child: &Child) -> io::Result<bool> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Safe wait observation is unavailable on this platform",
    ))
}

#[cfg(test)]
mod save_preparation_tests {
    use super::*;

    fn prepared_temporary(root: &Path) {
        let temporaries: Vec<_> = fs::read_dir(root)
            .unwrap()
            .map(|entry| entry.unwrap())
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".cedar-save-")
            })
            .collect();
        assert_eq!(temporaries.len(), 1);
        assert_eq!(fs::read(temporaries[0].path()).unwrap(), b"saved");
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            assert_eq!(
                temporaries[0].metadata().unwrap().file_attributes() & 0x100,
                0,
                "Windows attributes must be prepared before final validation"
            );
        }
    }

    #[test]
    fn external_edit_after_preparation_is_detected_and_temporary_is_removed() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("file");
        fs::write(&path, "before").unwrap();
        let workspace = Workspace::open(root.path()).unwrap();
        let failure = workspace
            .write_with_preparation_hook("file", "saved", Some(&revision("before")), || {
                prepared_temporary(root.path());
                fs::write(&path, "external edit").unwrap();
            })
            .unwrap_err();
        assert_eq!(failure.code, "conflict");
        assert_eq!(fs::read(&path).unwrap(), b"external edit");
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }

    #[test]
    fn external_removal_after_preparation_is_not_recreated() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("file");
        fs::write(&path, "before").unwrap();
        let workspace = Workspace::open(root.path()).unwrap();
        let failure = workspace
            .write_with_preparation_hook("file", "saved", Some(&revision("before")), || {
                prepared_temporary(root.path());
                fs::remove_file(&path).unwrap();
            })
            .unwrap_err();
        assert_eq!(failure.code, "conflict");
        assert!(!path.exists());
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    }

    #[test]
    fn stale_revision_is_rejected_before_preparation() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("file");
        fs::write(&path, "external edit").unwrap();
        let workspace = Workspace::open(root.path()).unwrap();
        let failure = workspace
            .write_with_preparation_hook("file", "saved", Some(&revision("before")), || {
                panic!("a stale revision must fail the early check");
            })
            .unwrap_err();
        assert_eq!(failure.code, "conflict");
        assert_eq!(fs::read(&path).unwrap(), b"external edit");
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }

    #[test]
    fn external_creation_after_preparation_is_not_clobbered() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("file");
        let workspace = Workspace::open(root.path()).unwrap();
        let failure = workspace
            .write_with_preparation_hook("file", "saved", None, || {
                fs::write(&path, "external creation").unwrap();
            })
            .unwrap_err();
        assert_eq!(failure.code, "conflict");
        assert_eq!(fs::read(&path).unwrap(), b"external creation");
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_added_after_preparation_is_rejected_without_touching_its_target() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let path = root.path().join("file");
        let target = outside.path().join("file");
        // Identical revisions cannot excuse a failed late path validation.
        fs::write(&path, "before").unwrap();
        fs::write(&target, "before").unwrap();
        let workspace = Workspace::open(root.path()).unwrap();
        let failure = workspace
            .write_with_preparation_hook("file", "saved", Some(&revision("before")), || {
                prepared_temporary(root.path());
                fs::remove_file(&path).unwrap();
                symlink(&target, &path).unwrap();
            })
            .unwrap_err();
        assert_eq!(failure.code, "invalid_path");
        assert!(fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(fs::read(&target).unwrap(), b"before");
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }

    #[cfg(windows)]
    #[test]
    fn read_only_attribute_added_after_preparation_is_not_bypassed_by_rename() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("file");
        fs::write(&path, "before").unwrap();
        let original_permissions = fs::metadata(&path).unwrap().permissions();
        let workspace = Workspace::open(root.path()).unwrap();
        let result = workspace.write_with_preparation_hook(
            "file",
            "saved",
            Some(&revision("before")),
            || {
                prepared_temporary(root.path());
                let mut permissions = original_permissions.clone();
                permissions.set_readonly(true);
                fs::set_permissions(&path, permissions).unwrap();
            },
        );
        let actual = fs::read(&path).unwrap();
        let permissions = fs::metadata(&path).unwrap().permissions();
        fs::set_permissions(&path, original_permissions).unwrap();
        assert_eq!(result.unwrap_err().code, "permission_denied");
        assert_eq!(actual, b"before");
        assert!(permissions.readonly());
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod process_wait_tests {
    use super::*;

    #[test]
    fn exit_observation_does_not_reap_or_release_child_pid() {
        let mut child = Command::new("/bin/sh")
            .args(["-c", "exit 7"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while !exited_without_reaping(&child).unwrap() {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(5));
        }
        // Repeated WNOWAIT observation still succeeds, and std retains its
        // actual exit code rather than receiving ECHILD from an earlier reap.
        assert!(exited_without_reaping(&child).unwrap());
        assert_eq!(child.wait().unwrap().code(), Some(7));
        assert_eq!(
            exited_without_reaping(&child).unwrap_err().raw_os_error(),
            Some(libc::ECHILD)
        );
    }
}

#[cfg(all(test, feature = "windows-language-validation"))]
mod validation_tests;
