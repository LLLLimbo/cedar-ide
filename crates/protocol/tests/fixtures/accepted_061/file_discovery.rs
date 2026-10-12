//! Versioned, path-only discovery. Independent of the frozen capability lists.
use crate::RemoteError;
use serde::{Deserialize, Deserializer, Serialize};
use std::io::{self, Write};

pub const FILE_DISCOVERY_VERSION: u32 = 1;
pub const MAX_FILE_DISCOVERY_DESCRIPTOR_BYTES: usize = 22;
pub const MAX_FILE_DISCOVERY_QUERY_BYTES: usize = 256;
pub const MAX_FILE_DISCOVERY_PATH_BYTES: usize = 4096;
pub const DEFAULT_FILE_DISCOVERY_RESULTS: usize = 100;
pub const MAX_FILE_DISCOVERY_RESULTS: usize = 200;
pub const MAX_FILE_DISCOVERY_ENTRIES: usize = 20_000;
pub const MAX_FILE_DISCOVERY_DEPTH: usize = 64;
pub const MAX_FILE_DISCOVERY_PENDING_DIRS: usize = 4096;
pub const MAX_FILE_DISCOVERY_PENDING_BYTES: usize = 512 * 1024;
/// Compact FileDiscoveryResult JSON, including query, scope and reasons.
/// Raw wire spellings and the enclosing frame still have the 8 MiB frame cap.
pub const MAX_FILE_DISCOVERY_RESULT_BYTES: usize = 512 * 1024;
pub const FILE_DISCOVERY_EXCLUDED_DIRECTORIES: &[&str] = &[
    ".git",
    "target",
    "build",
    "dist",
    "node_modules",
    ".idea",
    ".venv",
    "venv",
    "__pycache__",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileDiscoveryDescriptor {
    #[serde(deserialize_with = "positive_version")]
    pub version: u32,
}

fn positive_version<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    let version = u32::deserialize(deserializer)?;
    if version == 0 {
        return Err(serde::de::Error::custom(
            "file_discovery version must be positive",
        ));
    }
    Ok(version)
}

impl FileDiscoveryDescriptor {
    pub const V1: Self = Self {
        version: FILE_DISCOVERY_VERSION,
    };

    pub fn validate(&self) -> Result<(), RemoteError> {
        if self.version == 0
            || serde_json::to_vec(self).map_or(true, |bytes| {
                bytes.len() > MAX_FILE_DISCOVERY_DESCRIPTOR_BYTES
            })
        {
            return Err(RemoteError::new(
                "invalid_file_discovery",
                "Discovery descriptor must have a positive version and fit its compact JSON bound",
            ));
        }
        Ok(())
    }

    pub fn supported(&self) -> bool {
        self.version == FILE_DISCOVERY_VERSION
    }
}

pub fn default_file_discovery_limit() -> usize {
    DEFAULT_FILE_DISCOVERY_RESULTS
}

pub fn validate_file_discovery_request(
    query: &str,
    scope: &str,
    limit: usize,
) -> Result<usize, RemoteError> {
    if query.is_empty()
        || query.len() > MAX_FILE_DISCOVERY_QUERY_BYTES
        || query
            .chars()
            .any(|c| c.is_control() || matches!(c, '\u{2028}' | '\u{2029}'))
    {
        return Err(RemoteError::new("invalid_query", "FindFiles requires a nonempty single-line query of at most 256 UTF-8 bytes, without controls"));
    }
    if !scope.is_empty() && !valid_file_discovery_path(scope) {
        return Err(RemoteError::new(
            "invalid_path",
            "Discovery scope must be an exact relative directory path, or empty for the root",
        ));
    }
    if scope
        .split('/')
        .any(|part| FILE_DISCOVERY_EXCLUDED_DIRECTORIES.contains(&part))
    {
        return Err(RemoteError::new(
            "excluded_scope",
            "Discovery scope is inside an excluded directory",
        ));
    }
    if limit == 0 {
        return Err(RemoteError::new(
            "invalid_limit",
            "FindFiles result limit must be positive",
        ));
    }
    Ok(limit.min(MAX_FILE_DISCOVERY_RESULTS))
}

/// Exact relative spelling, with no normalization into a different target.
pub fn valid_file_discovery_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= MAX_FILE_DISCOVERY_PATH_BYTES
        && !path.contains(['\\', ':'])
        && !path.chars().any(char::is_control)
        && path
            .split('/')
            .all(|part| !part.is_empty() && !matches!(part, "." | ".."))
}

/// Rust Unicode lowercase on each operand, with no normalization, full case
/// folding, regex, or typo correction. Returned paths retain original spelling.
pub fn file_discovery_rank(path: &str, query: &str) -> Option<u8> {
    let path = path.to_lowercase();
    let query = query.to_lowercase();
    let basename = path.rsplit('/').next().unwrap_or("");
    if basename == query {
        Some(0)
    } else if basename.starts_with(&query) {
        Some(1)
    } else if basename.contains(&query) {
        Some(2)
    } else if path.contains(&query) {
        Some(3)
    } else {
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileDiscoveryIncompleteReason {
    PermissionDenied,
    IoError,
    DepthLimit,
    TimeLimit,
    EntryLimit,
    StackLimit,
    ResponseLimit,
    ResultLimit,
    UnrepresentablePath,
    UnsafePath,
}

impl FileDiscoveryIncompleteReason {
    pub const COUNT: usize = 10;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileDiscoveryResult {
    pub query: String,
    pub scope: String,
    pub paths: Vec<String>,
    pub incomplete: bool,
    pub reasons: Vec<FileDiscoveryIncompleteReason>,
    pub examined_entries: usize,
}

impl FileDiscoveryResult {
    /// Stream the exact serialization into a counter; do not allocate a second
    /// unbounded JSON buffer simply to validate an untrusted result.
    pub fn encoded_bytes(&self) -> Result<usize, RemoteError> {
        struct Counter(usize);
        impl Write for Counter {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                self.0 = self
                    .0
                    .checked_add(bytes.len())
                    .filter(|n| *n <= MAX_FILE_DISCOVERY_RESULT_BYTES)
                    .ok_or_else(|| io::Error::other("discovery result exceeds JSON budget"))?;
                Ok(bytes.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let mut counter = Counter(0);
        serde_json::to_writer(&mut counter, self).map_err(|_| invalid_result())?;
        Ok(counter.0)
    }

    pub fn validate_for(&self, query: &str, scope: &str, limit: usize) -> Result<(), RemoteError> {
        let limit = validate_file_discovery_request(query, scope, limit)?;
        if self.query != query
            || self.scope != scope
            || self.paths.len() > limit
            || self.examined_entries > MAX_FILE_DISCOVERY_ENTRIES
            || self.paths.len() > self.examined_entries
            || self.reasons.len() > FileDiscoveryIncompleteReason::COUNT
            || self.incomplete != !self.reasons.is_empty()
            || self
                .reasons
                .iter()
                .enumerate()
                .any(|(i, reason)| self.reasons[..i].contains(reason))
        {
            return Err(invalid_result());
        }
        let mut previous: Option<(u8, &str)> = None;
        for path in &self.paths {
            let scoped_path = if scope.is_empty() {
                Some(path.as_str())
            } else {
                path.strip_prefix(scope)
                    .and_then(|tail| tail.strip_prefix('/'))
            };
            if !valid_file_discovery_path(path)
                || scoped_path.is_none_or(|path| {
                    path.bytes().filter(|b| *b == b'/').count() > MAX_FILE_DISCOVERY_DEPTH
                })
                || path
                    .split('/')
                    .rev()
                    .skip(1)
                    .any(|part| FILE_DISCOVERY_EXCLUDED_DIRECTORIES.contains(&part))
            {
                return Err(invalid_result());
            }
            let rank = file_discovery_rank(path, query).ok_or_else(invalid_result)?;
            let key = (rank, path.as_str());
            if previous.is_some_and(|previous| previous >= key) {
                return Err(invalid_result());
            }
            previous = Some(key);
        }
        self.encoded_bytes()?;
        Ok(())
    }
}

fn invalid_result() -> RemoteError {
    RemoteError::new(
        "invalid_file_discovery",
        "Discovery result is malformed, inconsistent, or oversized",
    )
}
