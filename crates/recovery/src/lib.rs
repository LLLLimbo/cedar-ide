//! Private, bounded, frontend-local recovery of unsaved UTF-8 buffers.
//!
//! This crate never opens workspace files or restores execution trust. Callers
//! restore drafts into memory and retain the recorded base revision for ordinary
//! backend conflict checks. A successful [`Store::write`] acknowledges the disk
//! write, not a save to the workspace. See `docs/RECOVERY.md` for filesystem and
//! Windows durability limits.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fmt;
use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};

pub const FORMAT_VERSION: u32 = 1;
pub const MAX_TEXT_BYTES: usize = 1024 * 1024;
pub const MAX_PATH_BYTES: usize = 4096;
pub const MAX_HEADER_BYTES: usize = 64 * 1024;
const PREFIX_BYTES: usize = 44;
pub const MAX_RECORD_BYTES: u64 = (2 * MAX_TEXT_BYTES + MAX_HEADER_BYTES + PREFIX_BYTES) as u64;
pub const MAX_TOTAL_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_RECORDS: usize = 128;
pub const MAX_STORAGE_ENTRIES: usize = 256;
pub const MAX_OPERATION_KEYS: usize = 4096;
const MAX_HOST_BYTES: usize = 1024;
const MAX_REVISION_BYTES: usize = 256;
const MAGIC: &[u8; 8] = b"CEDARDR1";
const LOCK_NAME: &str = ".cedar-lock";
const EXTENSION: &str = ".draft";

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Recovery I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("Recovery store is already locked by another editor")]
    Locked,
    #[error("Invalid recovery data: {0}")]
    Invalid(String),
    #[error("Recovery limit exceeded: {0}; the draft has not been acknowledged as recoverable")]
    Limit(String),
    #[error("Unsafe recovery storage: {0}")]
    UnsafeStorage(String),
    #[error("Damaged recovery record: {0}; existing evidence was left in place")]
    Damaged(String),
}

/// Stable location metadata only. No credentials, session IDs, or trust state.
/// Callers should use the canonical root returned by the workspace handshake.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkspaceIdentity {
    Local {
        root: String,
    },
    Ssh {
        host: String,
        port: u16,
        root: String,
        agent_path: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    pub workspace: WorkspaceIdentity,
    pub path: String,
    pub text: String,
    pub base_text: String,
    /// The original backend revision, never refreshed during recovery. None
    /// means a new file; the backend must still reject an existing destination.
    pub base_revision: Option<String>,
    pub modified_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RecordId(String);

impl RecordId {
    pub fn parse(value: &str) -> Result<Self> {
        if !valid_digest(value) {
            return Err(Error::Invalid(
                "record ID must be 64 lowercase hex digits".into(),
            ));
        }
        Ok(Self(value.into()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RecordId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftMetadata {
    pub id: RecordId,
    pub workspace: WorkspaceIdentity,
    pub path: String,
    pub base_revision: Option<String>,
    pub modified_ms: u64,
    pub text_bytes: usize,
    pub base_text_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordIssue {
    pub name: String,
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub struct Listing {
    pub drafts: Vec<DraftMetadata>,
    /// Damaged, unsafe, unknown, or overflow entries. They are never deleted.
    pub issues: Vec<RecordIssue>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationOutcome {
    Applied,
    IgnoredStale,
}

/// Lower quotas are useful for constrained frontends and deterministic tests.
/// Values may not exceed the exported hard bounds.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_text_bytes: usize,
    pub max_record_bytes: u64,
    pub max_total_bytes: u64,
    pub max_records: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_text_bytes: MAX_TEXT_BYTES,
            max_record_bytes: MAX_RECORD_BYTES,
            max_total_bytes: MAX_TOTAL_BYTES,
            max_records: MAX_RECORDS,
        }
    }
}

impl Limits {
    fn validate(self) -> Result<Self> {
        if self.max_text_bytes == 0
            || self.max_text_bytes > MAX_TEXT_BYTES
            || self.max_record_bytes == 0
            || self.max_record_bytes > MAX_RECORD_BYTES
            || self.max_total_bytes == 0
            || self.max_total_bytes > MAX_TOTAL_BYTES
            || self.max_records == 0
            || self.max_records > MAX_RECORDS
        {
            return Err(Error::Invalid(
                "limits must be positive and at most the hard bounds".into(),
            ));
        }
        Ok(self)
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    version: u32,
    workspace: WorkspaceIdentity,
    path: String,
    base_revision: Option<String>,
    modified_ms: u64,
    text_bytes: usize,
    base_text_bytes: usize,
    text_sha256: String,
    base_text_sha256: String,
}

impl Header {
    fn for_draft(draft: &Draft) -> Self {
        Self {
            version: FORMAT_VERSION,
            workspace: draft.workspace.clone(),
            path: draft.path.clone(),
            base_revision: draft.base_revision.clone(),
            modified_ms: draft.modified_ms,
            text_bytes: draft.text.len(),
            base_text_bytes: draft.base_text.len(),
            text_sha256: digest(draft.text.as_bytes()),
            base_text_sha256: digest(draft.base_text.as_bytes()),
        }
    }

    fn validate(&self, limits: Limits, id: &RecordId) -> Result<()> {
        if self.version != FORMAT_VERSION {
            return Err(Error::Damaged(format!(
                "unsupported format version {}",
                self.version
            )));
        }
        validate_identity(&self.workspace)?;
        validate_relative_path(&self.path)?;
        validate_revision(self.base_revision.as_deref())?;
        if self.text_bytes > limits.max_text_bytes || self.base_text_bytes > limits.max_text_bytes {
            return Err(Error::Limit(
                "draft or base text exceeds the text byte limit".into(),
            ));
        }
        if !valid_digest(&self.text_sha256) || !valid_digest(&self.base_text_sha256) {
            return Err(Error::Damaged("invalid payload digest".into()));
        }
        if record_id(&self.workspace, &self.path)? != *id {
            return Err(Error::Damaged(
                "record filename does not match its identity and path".into(),
            ));
        }
        Ok(())
    }

    fn metadata(self, id: RecordId) -> DraftMetadata {
        DraftMetadata {
            id,
            workspace: self.workspace,
            path: self.path,
            base_revision: self.base_revision,
            modified_ms: self.modified_ms,
            text_bytes: self.text_bytes,
            base_text_bytes: self.base_text_bytes,
        }
    }
}

/// One lock-owning writer per store. Keep this object in a frontend I/O worker.
/// Advisory locks only coordinate cooperating Cedar processes.
#[derive(Debug)]
pub struct Store {
    root: PathBuf,
    directory: File,
    _lock: File,
    limits: Limits,
    sequences: HashMap<RecordId, u64>,
}

impl Store {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_with_limits(path, Limits::default())
    }

    pub fn open_with_limits(path: impl AsRef<Path>, limits: Limits) -> Result<Self> {
        let limits = limits.validate()?;
        let root = create_private_directory(path.as_ref())?;
        #[cfg(unix)]
        sync_ancestor_directories(&root, |parent| {
            open_directory(parent)?.sync_all()?;
            Ok(())
        })?;
        let directory = open_directory(&root)?;
        let lock_path = root.join(LOCK_NAME);
        let lock = match open_private_new(&lock_path) {
            Ok(file) => file,
            Err(Error::Io(error)) if error.kind() == io::ErrorKind::AlreadyExists => {
                open_regular(&lock_path, true)?
            }
            Err(error) => return Err(error),
        };
        if lock.metadata()?.len() != 0 {
            return Err(Error::UnsafeStorage("lock file must be empty".into()));
        }
        match lock.try_lock() {
            Ok(()) => {}
            Err(std::fs::TryLockError::WouldBlock) => return Err(Error::Locked),
            Err(std::fs::TryLockError::Error(error)) => return Err(Error::Io(error)),
        }
        let store = Self {
            root,
            directory,
            _lock: lock,
            limits,
            sequences: HashMap::new(),
        };
        store.check_root()?;
        Ok(store)
    }

    /// Reads bounded metadata headers only, never all draft payloads. Payload
    /// integrity is checked when an individual draft is read for restoration.
    pub fn list(&self) -> Result<Listing> {
        self.check_root()?;
        let mut listing = Listing::default();
        let mut total = 0u64;
        let mut count = 0usize;
        for (index, item) in fs::read_dir(&self.root)?.enumerate() {
            if index >= MAX_STORAGE_ENTRIES {
                listing.issues.push(issue(
                    "(store)",
                    "Recovery directory entry limit exceeded; listing is incomplete",
                ));
                break;
            }
            let entry = match item {
                Ok(entry) => entry,
                Err(error) => {
                    listing.issues.push(issue("(entry)", error.to_string()));
                    continue;
                }
            };
            let name = entry.file_name();
            if name == LOCK_NAME {
                continue;
            }
            let label = entry_label(&name);
            let meta = match fs::symlink_metadata(entry.path()) {
                Ok(meta) => meta,
                Err(error) => {
                    listing.issues.push(issue(&label, error.to_string()));
                    continue;
                }
            };
            total = total.saturating_add(meta.len());
            let id = match record_name(&name) {
                Some(id) => id,
                None => {
                    listing.issues.push(issue(
                        &label,
                        "Unrecognized entry retained; it may be evidence from an interrupted write",
                    ));
                    continue;
                }
            };
            count += 1;
            if count > self.limits.max_records {
                listing.issues.push(issue(
                    &label,
                    "Record count limit exceeded; this record was not inspected",
                ));
                continue;
            }
            match self.read_header(&id) {
                Ok((_, header)) => listing.drafts.push(header.metadata(id)),
                Err(error) => listing.issues.push(issue(&label, error.to_string())),
            }
        }
        if total > self.limits.max_total_bytes {
            listing.issues.push(issue(
                "(store)",
                "Total storage limit exceeded; new durable writes are blocked",
            ));
        }
        listing.drafts.sort_by(|a, b| {
            b.modified_ms
                .cmp(&a.modified_ms)
                .then_with(|| a.id.cmp(&b.id))
        });
        Ok(listing)
    }

    /// Load one record, preserving text bytes and the original base revision.
    /// No workspace connection, file write, or execution authorization occurs.
    pub fn read(&self, id: &RecordId) -> Result<Draft> {
        let (mut file, header) = self.read_header(id)?;
        let mut text = vec![0; header.text_bytes];
        let mut base_text = vec![0; header.base_text_bytes];
        file.read_exact(&mut text)?;
        file.read_exact(&mut base_text)?;
        let mut extra = [0];
        if file.read(&mut extra)? != 0 {
            return Err(Error::Damaged("unexpected trailing bytes".into()));
        }
        if digest(&text) != header.text_sha256 || digest(&base_text) != header.base_text_sha256 {
            return Err(Error::Damaged("payload checksum mismatch".into()));
        }
        let text = String::from_utf8(text)
            .map_err(|_| Error::Damaged("draft is not valid UTF-8".into()))?;
        let base_text = String::from_utf8(base_text)
            .map_err(|_| Error::Damaged("base text is not valid UTF-8".into()))?;
        Ok(Draft {
            workspace: header.workspace,
            path: header.path,
            text,
            base_text,
            base_revision: header.base_revision,
            modified_ms: header.modified_ms,
        })
    }

    /// Atomically replace a private draft, then synchronize the file and (Unix)
    /// its directory. Only `Applied` is a durable acknowledgement. A stale
    /// sequence is ignored even after removal. Failed operations reserve their
    /// sequence too; retry with a newer sequence. No old draft is evicted.
    pub fn write(&mut self, sequence: u64, draft: &Draft) -> Result<MutationOutcome> {
        let id = record_id(&draft.workspace, &draft.path)?;
        if !self.accept_sequence(&id, sequence)? {
            return Ok(MutationOutcome::IgnoredStale);
        }
        self.check_root()?;
        validate_revision(draft.base_revision.as_deref())?;
        if draft.text.len() > self.limits.max_text_bytes
            || draft.base_text.len() > self.limits.max_text_bytes
        {
            return Err(Error::Limit(
                "draft or base text exceeds the text byte limit".into(),
            ));
        }
        let header = serde_json::to_vec(&Header::for_draft(draft))
            .map_err(|error| Error::Invalid(error.to_string()))?;
        if header.len() > MAX_HEADER_BYTES {
            return Err(Error::Limit(
                "record metadata exceeds the header byte limit".into(),
            ));
        }
        let size = PREFIX_BYTES as u64
            + header.len() as u64
            + draft.text.len() as u64
            + draft.base_text.len() as u64;
        if size > self.limits.max_record_bytes {
            return Err(Error::Limit(
                "serialized record exceeds the record byte limit".into(),
            ));
        }
        let destination = self.record_path(&id);
        let replacing = match fs::symlink_metadata(&destination) {
            Ok(_) => {
                // Never silently replace damaged records: preserve evidence.
                self.read(&id)?;
                true
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => false,
            Err(error) => return Err(error.into()),
        };
        self.check_capacity(size, replacing)?;
        let mut temporary = tempfile::Builder::new()
            .prefix(".cedar-write-")
            .tempfile_in(&self.root)?;
        check_private_regular(&temporary.as_file().metadata()?)?;
        temporary.write_all(MAGIC)?;
        temporary.write_all(&(header.len() as u32).to_le_bytes())?;
        temporary.write_all(&Sha256::digest(&header))?;
        temporary.write_all(&header)?;
        temporary.write_all(draft.text.as_bytes())?;
        temporary.write_all(draft.base_text.as_bytes())?;
        temporary.as_file().sync_all()?;
        self.check_root()?;
        // Detect ordinary post-check substitution; hostile rename races still
        // require an OS boundary, as documented.
        if replacing {
            check_private_regular(&fs::symlink_metadata(&destination)?)?;
        } else if fs::symlink_metadata(&destination).is_ok() {
            return Err(Error::UnsafeStorage(
                "draft destination appeared during write".into(),
            ));
        }
        temporary
            .persist(&destination)
            .map_err(|error| Error::Io(error.error))?;
        self.sync_directory()?;
        Ok(MutationOutcome::Applied)
    }

    /// Explicitly remove this identity/path's valid record. Damaged evidence is
    /// retained with an error. Even an absent record gets a process tombstone.
    pub fn remove(
        &mut self,
        sequence: u64,
        workspace: &WorkspaceIdentity,
        path: &str,
    ) -> Result<MutationOutcome> {
        let id = record_id(workspace, path)?;
        if !self.accept_sequence(&id, sequence)? {
            return Ok(MutationOutcome::IgnoredStale);
        }
        self.check_root()?;
        let destination = self.record_path(&id);
        match fs::symlink_metadata(&destination) {
            Ok(_) => {
                self.read(&id)?;
                self.check_root()?;
                check_private_regular(&fs::symlink_metadata(&destination)?)?;
                fs::remove_file(destination)?;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        self.sync_directory()?;
        Ok(MutationOutcome::Applied)
    }

    fn record_path(&self, id: &RecordId) -> PathBuf {
        self.root.join(format!("{}{EXTENSION}", id.as_str()))
    }

    fn accept_sequence(&mut self, id: &RecordId, sequence: u64) -> Result<bool> {
        if let Some(previous) = self.sequences.get_mut(id) {
            if sequence <= *previous {
                return Ok(false);
            }
            *previous = sequence;
        } else {
            if self.sequences.len() >= MAX_OPERATION_KEYS {
                return Err(Error::Limit("too many distinct recovery keys in one process; reopen the store to reset sequence tracking".into()));
            }
            self.sequences.insert(id.clone(), sequence);
        }
        Ok(true)
    }

    fn read_header(&self, id: &RecordId) -> Result<(File, Header)> {
        self.check_root()?;
        let mut file = open_regular(&self.record_path(id), false)?;
        let length = file.metadata()?.len();
        if length > self.limits.max_record_bytes {
            return Err(Error::Limit("record exceeds the record byte limit".into()));
        }
        let mut prefix = [0u8; PREFIX_BYTES];
        file.read_exact(&mut prefix)
            .map_err(|error| damaged_read(error, "record prefix"))?;
        if &prefix[..8] != MAGIC {
            return Err(Error::Damaged("wrong format marker".into()));
        }
        let header_size =
            u32::from_le_bytes(prefix[8..12].try_into().expect("four-byte slice")) as usize;
        if header_size == 0 || header_size > MAX_HEADER_BYTES {
            return Err(Error::Limit(
                "header exceeds the metadata byte limit".into(),
            ));
        }
        if PREFIX_BYTES as u64 + header_size as u64 > length {
            return Err(Error::Damaged("truncated metadata header".into()));
        }
        let mut bytes = vec![0; header_size];
        file.read_exact(&mut bytes)
            .map_err(|error| damaged_read(error, "metadata header"))?;
        if Sha256::digest(&bytes)[..] != prefix[12..PREFIX_BYTES] {
            return Err(Error::Damaged("metadata checksum mismatch".into()));
        }
        let header: Header = serde_json::from_slice(&bytes)
            .map_err(|error| Error::Damaged(format!("invalid metadata: {error}")))?;
        header.validate(self.limits, id)?;
        let expected_size = PREFIX_BYTES as u64
            + header_size as u64
            + header.text_bytes as u64
            + header.base_text_bytes as u64;
        if length != expected_size {
            return Err(Error::Damaged(
                "record length does not match metadata".into(),
            ));
        }
        Ok((file, header))
    }

    fn check_capacity(&self, additional_bytes: u64, replacing: bool) -> Result<()> {
        let mut total = 0u64;
        let mut records = 0usize;
        let mut entries = 0usize;
        for item in fs::read_dir(&self.root)? {
            entries += 1;
            // Reserve one slot for the same-directory atomic temporary file.
            if entries >= MAX_STORAGE_ENTRIES {
                return Err(Error::Limit(
                    "recovery directory entry limit reached".into(),
                ));
            }
            let entry = item?;
            let meta = fs::symlink_metadata(entry.path())?;
            check_private_regular(&meta)?;
            total = total
                .checked_add(meta.len())
                .ok_or_else(|| Error::Limit("storage byte counter overflow".into()))?;
            if record_name(&entry.file_name()).is_some() {
                records += 1;
            }
        }
        if records + usize::from(!replacing) > self.limits.max_records {
            return Err(Error::Limit(
                "record count limit reached; existing drafts were retained".into(),
            ));
        }
        // Includes the full transient temporary file, not just net growth.
        if total
            .checked_add(additional_bytes)
            .is_none_or(|size| size > self.limits.max_total_bytes)
        {
            return Err(Error::Limit("total storage limit reached, including atomic-write headroom; existing drafts were retained".into()));
        }
        Ok(())
    }

    fn check_root(&self) -> Result<()> {
        check_directory_chain(&self.root)?;
        let current = fs::symlink_metadata(&self.root)?;
        check_private_directory(&current)?;
        if !same_file(&current, &self.directory.metadata()?) {
            return Err(Error::UnsafeStorage(
                "recovery directory was replaced while open".into(),
            ));
        }
        let lock_meta = fs::symlink_metadata(self.root.join(LOCK_NAME))?;
        check_private_regular(&lock_meta)?;
        if !same_file(&lock_meta, &self._lock.metadata()?) {
            return Err(Error::UnsafeStorage(
                "recovery lock file was replaced while open".into(),
            ));
        }
        Ok(())
    }

    fn sync_directory(&self) -> Result<()> {
        #[cfg(unix)]
        self.directory.sync_all()?;
        // Windows has no portable std directory-fsync guarantee. The temporary
        // payload was flushed before atomic rename; see the documented limit.
        Ok(())
    }
}

/// Selects a frontend-local location, even for SSH projects. Does not create it.
pub fn default_store_path() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("CEDAR_RECOVERY_DIR") {
        return absolute_setting(PathBuf::from(path), "CEDAR_RECOVERY_DIR");
    }
    #[cfg(target_os = "windows")]
    {
        let base = std::env::var_os("LOCALAPPDATA").ok_or_else(|| {
            Error::Invalid(
                "LOCALAPPDATA is missing; set CEDAR_RECOVERY_DIR to a private absolute path".into(),
            )
        })?;
        absolute_setting(
            PathBuf::from(base).join("Cedar").join("recovery"),
            "LOCALAPPDATA",
        )
    }
    #[cfg(target_os = "macos")]
    {
        let base = std::env::var_os("HOME").ok_or_else(|| {
            Error::Invalid(
                "HOME is missing; set CEDAR_RECOVERY_DIR to a private absolute path".into(),
            )
        })?;
        absolute_setting(
            PathBuf::from(base).join("Library/Application Support/Cedar/recovery"),
            "HOME",
        )
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        if let Some(base) = std::env::var_os("XDG_DATA_HOME") {
            let base = PathBuf::from(base);
            if base.is_absolute() {
                return Ok(base.join("cedar/recovery"));
            }
        }
        let base = std::env::var_os("HOME").ok_or_else(|| {
            Error::Invalid(
                "HOME is missing; set CEDAR_RECOVERY_DIR to a private absolute path".into(),
            )
        })?;
        absolute_setting(
            PathBuf::from(base).join(".local/share/cedar/recovery"),
            "HOME",
        )
    }
}

fn absolute_setting(path: PathBuf, name: &str) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path)
    } else {
        Err(Error::Invalid(format!("{name} must name an absolute path")))
    }
}

/// Versioned, length-delimited canonical serialization avoids concatenation
/// collisions. Identity strings are not lowercased or Unicode-normalized.
pub fn record_id(workspace: &WorkspaceIdentity, path: &str) -> Result<RecordId> {
    validate_identity(workspace)?;
    validate_relative_path(path)?;
    let bytes = serde_json::to_vec(&("cedar-recovery-key-v1", workspace, path))
        .map_err(|error| Error::Invalid(error.to_string()))?;
    Ok(RecordId(digest(&bytes)))
}

/// Strict portable file paths: forward-slash components only; no traversal,
/// drive prefixes, alternate streams, Windows device aliases, or normalization.
pub fn validate_relative_path(path: &str) -> Result<()> {
    if path.is_empty() || path.len() > MAX_PATH_BYTES {
        return Err(Error::Invalid(
            "relative path must be 1..=4096 UTF-8 bytes".into(),
        ));
    }
    for component in path.split('/') {
        if component.is_empty()
            || matches!(component, "." | "..")
            || component.len() > 255
            || component.ends_with(['.', ' '])
            || component.chars().any(|c| {
                c.is_control() || matches!(c, '\\' | ':' | '<' | '>' | '"' | '|' | '?' | '*')
            })
        {
            return Err(Error::Invalid(
                "path is not a strict portable relative file path".into(),
            ));
        }
        let stem = component
            .split('.')
            .next()
            .unwrap_or_default()
            .trim_end_matches(['.', ' '])
            .to_ascii_uppercase();
        if matches!(
            stem.as_str(),
            "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
        ) || ["COM", "LPT"].iter().any(|prefix| {
            stem.strip_prefix(prefix).is_some_and(|number| {
                matches!(
                    number,
                    "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
            })
        }) {
            return Err(Error::Invalid(
                "Windows device paths are not supported".into(),
            ));
        }
    }
    Ok(())
}

fn validate_identity(workspace: &WorkspaceIdentity) -> Result<()> {
    match workspace {
        WorkspaceIdentity::Local { root } => bounded_label(root, MAX_PATH_BYTES, "local root"),
        WorkspaceIdentity::Ssh {
            host,
            port,
            root,
            agent_path,
        } => {
            bounded_label(host, MAX_HOST_BYTES, "SSH host")?;
            bounded_label(root, MAX_PATH_BYTES, "SSH root")?;
            bounded_label(agent_path, MAX_PATH_BYTES, "SSH agent path")?;
            if *port == 0 {
                return Err(Error::Invalid("SSH port must be nonzero".into()));
            }
            Ok(())
        }
    }
}

fn bounded_label(value: &str, max: usize, label: &str) -> Result<()> {
    if value.is_empty() || value.len() > max || value.chars().any(char::is_control) {
        return Err(Error::Invalid(format!(
            "{label} must be nonempty, contain no control characters, and fit {max} bytes"
        )));
    }
    Ok(())
}

fn validate_revision(revision: Option<&str>) -> Result<()> {
    if let Some(revision) = revision {
        bounded_label(revision, MAX_REVISION_BYTES, "base revision")?;
    }
    Ok(())
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn record_name(name: &std::ffi::OsStr) -> Option<RecordId> {
    RecordId::parse(name.to_str()?.strip_suffix(EXTENSION)?).ok()
}

fn entry_label(name: &std::ffi::OsStr) -> String {
    match name.to_str() {
        Some(name) if name.len() <= 512 => name.into(),
        _ => "(non-UTF-8 or oversized entry name)".into(),
    }
}

fn issue(name: &str, message: impl Into<String>) -> RecordIssue {
    RecordIssue {
        name: name.into(),
        message: message.into(),
    }
}

fn damaged_read(error: io::Error, part: &str) -> Error {
    if error.kind() == io::ErrorKind::UnexpectedEof {
        Error::Damaged(format!("truncated {part}"))
    } else {
        Error::Io(error)
    }
}

fn create_private_directory(path: &Path) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut current = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::ParentDir => {
                return Err(Error::Invalid(
                    "recovery directory cannot contain parent traversal".into(),
                ))
            }
            Component::CurDir => continue,
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                current.push(component)
            }
        }
        match fs::symlink_metadata(&current) {
            Ok(meta) => check_directory(&meta)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let builder = private_directory_builder();
                match builder.create(&current) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(error.into()),
                }
                check_private_directory(&fs::symlink_metadata(&current)?)?;
            }
            Err(error) => return Err(error.into()),
        }
    }
    check_private_directory(&fs::symlink_metadata(&current)?)?;
    Ok(current)
}

/// Sync existing ancestors too: a previous mkdir may have succeeded before its
/// parent sync failed or the process crashed. Bottom-up order preserves every
/// accepted directory link before a later record write can be acknowledged.
#[cfg(unix)]
fn sync_ancestor_directories(path: &Path, mut sync: impl FnMut(&Path) -> Result<()>) -> Result<()> {
    for ancestor in path.ancestors().skip(1) {
        sync(ancestor)?;
    }
    Ok(())
}

fn private_directory_builder() -> fs::DirBuilder {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = fs::DirBuilder::new();
        builder.mode(0o700);
        builder
    }
    #[cfg(not(unix))]
    fs::DirBuilder::new()
}

fn check_directory_chain(path: &Path) -> Result<()> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component);
        check_directory(&fs::symlink_metadata(&current)?)?;
    }
    Ok(())
}

fn is_link(meta: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        meta.file_attributes() & 0x400 != 0 // FILE_ATTRIBUTE_REPARSE_POINT
    }
    #[cfg(not(windows))]
    {
        meta.file_type().is_symlink()
    }
}

fn check_directory(meta: &Metadata) -> Result<()> {
    if is_link(meta) || !meta.is_dir() {
        return Err(Error::UnsafeStorage(
            "store path contains a symlink, reparse point, or non-directory".into(),
        ));
    }
    Ok(())
}

fn check_private_directory(meta: &Metadata) -> Result<()> {
    check_directory(meta)?;
    check_private_permissions(meta)
}

fn check_private_regular(meta: &Metadata) -> Result<()> {
    if is_link(meta) || !meta.is_file() {
        return Err(Error::UnsafeStorage(
            "only regular, non-symlink recovery files are allowed".into(),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if meta.nlink() != 1 {
            return Err(Error::UnsafeStorage(
                "hard-linked recovery files are not supported".into(),
            ));
        }
    }
    check_private_permissions(meta)
}

fn check_private_permissions(meta: &Metadata) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if meta.mode() & 0o077 != 0 || meta.uid() != unsafe { libc::geteuid() } {
            return Err(Error::UnsafeStorage("recovery storage must be owned by the current account and private (directories 0700, files 0600)".into()));
        }
    }
    #[cfg(not(unix))]
    let _ = meta;
    Ok(())
}

fn nofollow_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .mode(0o600);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x00200000); // FILE_FLAG_OPEN_REPARSE_POINT
    }
    options
}

fn open_private_new(path: &Path) -> Result<File> {
    let file = nofollow_options()
        .read(true)
        .write(true)
        .create_new(true)
        .open(path)?;
    check_private_regular(&file.metadata()?)?;
    Ok(file)
}

fn open_regular(path: &Path, writable: bool) -> Result<File> {
    let before = fs::symlink_metadata(path)?;
    check_private_regular(&before)?;
    let file = nofollow_options().read(true).write(writable).open(path)?;
    let after = file.metadata()?;
    check_private_regular(&after)?;
    if !same_file(&before, &after) {
        return Err(Error::UnsafeStorage(
            "file changed during recovery open".into(),
        ));
    }
    Ok(file)
}

fn open_directory(path: &Path) -> Result<File> {
    let mut options = nofollow_options();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x00200000 | 0x02000000); // OPEN_REPARSE_POINT | BACKUP_SEMANTICS
    }
    let file = options.read(true).open(path)?;
    check_directory(&file.metadata()?)?;
    Ok(file)
}

fn same_file(left: &Metadata, right: &Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        left.dev() == right.dev() && left.ino() == right.ino()
    }
    #[cfg(not(unix))]
    {
        // Stable std Metadata exposes no portable Windows file identity. This
        // compares kind only; directory-rename races are documented, not solved.
        left.is_file() == right.is_file() && left.is_dir() == right.is_dir()
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn reopening_retries_every_ancestor_barrier_after_sync_failure() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("new/cedar/recovery");
        let root = create_private_directory(&root).unwrap();
        let expected: Vec<_> = root.ancestors().skip(1).map(Path::to_owned).collect();
        let mut first_attempt = Vec::new();
        let error = sync_ancestor_directories(&root, |path| {
            first_attempt.push(path.to_owned());
            Err(Error::Io(io::Error::other("injected parent fsync failure")))
        });
        assert!(error.is_err());
        assert_eq!(first_attempt, expected[..1]);
        // All components now already exist, exactly as after an interrupted open.
        let reopened_root = create_private_directory(&root).unwrap();
        let mut retry = Vec::new();
        sync_ancestor_directories(&reopened_root, |path| {
            retry.push(path.to_owned());
            open_directory(path)?.sync_all()?;
            Ok(())
        })
        .unwrap();
        assert_eq!(retry, expected);
        let _store = Store::open(root).unwrap();
    }
}
