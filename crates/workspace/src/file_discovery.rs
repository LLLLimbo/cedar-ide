//! Query-scoped metadata traversal. No content reads, execution or persistent state.
use crate::{error, io_error, Workspace};
use cedar_protocol::{
    file_discovery_rank, valid_file_discovery_path, validate_file_discovery_request,
    FileDiscoveryIncompleteReason as Reason, FileDiscoveryResult, Payload, RemoteError,
    FILE_DISCOVERY_EXCLUDED_DIRECTORIES, MAX_FILE_DISCOVERY_DEPTH, MAX_FILE_DISCOVERY_ENTRIES,
    MAX_FILE_DISCOVERY_PENDING_BYTES, MAX_FILE_DISCOVERY_PENDING_DIRS,
    MAX_FILE_DISCOVERY_RESULT_BYTES,
};
use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const MAX_DURATION: Duration = Duration::from_secs(2);

#[derive(Clone, Copy)]
struct Budget {
    entries: usize,
    depth: usize,
    pending_dirs: usize,
    pending_bytes: usize,
    result_bytes: usize,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            entries: MAX_FILE_DISCOVERY_ENTRIES,
            depth: MAX_FILE_DISCOVERY_DEPTH,
            pending_dirs: MAX_FILE_DISCOVERY_PENDING_DIRS,
            pending_bytes: MAX_FILE_DISCOVERY_PENDING_BYTES,
            result_bytes: MAX_FILE_DISCOVERY_RESULT_BYTES,
        }
    }
}

impl Workspace {
    pub(super) fn find_files(
        &self,
        query: &str,
        scope: &str,
        limit: usize,
    ) -> Result<Payload, RemoteError> {
        let started = Instant::now();
        let result = self.find_files_with_budget(
            query,
            scope,
            limit,
            Budget::default(),
            || started.elapsed() >= MAX_DURATION,
            |_| {},
        )?;
        Ok(Payload::FilePaths { result })
    }

    fn find_files_with_budget(
        &self,
        query: &str,
        scope: &str,
        limit: usize,
        budget: Budget,
        mut expired: impl FnMut() -> bool,
        mut before_directory: impl FnMut(&str),
    ) -> Result<FileDiscoveryResult, RemoteError> {
        let limit = validate_file_discovery_request(query, scope, limit)?;
        let mut result = FileDiscoveryResult {
            query: query.into(),
            scope: scope.into(),
            paths: Vec::new(),
            incomplete: false,
            reasons: Vec::new(),
            examined_entries: 0,
        };
        // Depth is relative to the selected scope: it starts at zero. Retained
        // stack bytes count exact relative UTF-8 strings, not repeated roots.
        let mut pending = vec![(scope.to_owned(), 0usize)];
        let mut pending_bytes = scope.len();
        let mut candidates = BTreeSet::new();
        let mut reasons = BTreeSet::new();
        'walk: while !pending.is_empty() {
            if expired() {
                reasons.insert(Reason::TimeLimit);
                break;
            }
            let (relative, depth) = pending.pop().expect("nonempty pending stack");
            pending_bytes = pending_bytes
                .checked_sub(relative.len())
                .expect("accounted stack path");
            before_directory(&relative);
            // Recheck every ancestor when popping. The check narrows races; it
            // is not a sandbox against hostile concurrent namespace changes.
            let (directory, metadata) = match self.resolve_discovery_path(&relative, &mut expired) {
                Ok(value) => value,
                Err(failure) if failure.code == "discovery_time_limit" => {
                    reasons.insert(Reason::TimeLimit);
                    break;
                }
                Err(failure) if relative == scope && failure.code == "invalid_path" => {
                    return Err(failure)
                }
                Err(failure) => {
                    reasons.insert(reason_for_error(&failure));
                    continue;
                }
            };
            if !metadata.is_dir() {
                if relative == scope {
                    return Err(error(
                        "not_directory",
                        "Discovery scope must be a directory",
                    ));
                }
                reasons.insert(Reason::UnsafePath);
                continue;
            }
            if expired() {
                reasons.insert(Reason::TimeLimit);
                break;
            }
            let mut entries = match fs::read_dir(&directory) {
                Ok(entries) => entries,
                Err(failure) => {
                    reasons.insert(reason_for_io(&failure));
                    continue;
                }
            };
            loop {
                // Do not call next() for a 20,001st entry. Exhaustion is
                // conservatively incomplete, even when the last entry was last.
                if result.examined_entries >= budget.entries {
                    reasons.insert(Reason::EntryLimit);
                    break 'walk;
                }
                if expired() {
                    reasons.insert(Reason::TimeLimit);
                    break 'walk;
                }
                let Some(entry) = entries.next() else {
                    break;
                };
                result.examined_entries += 1;
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(failure) => {
                        reasons.insert(reason_for_io(&failure));
                        continue;
                    }
                };
                if expired() {
                    reasons.insert(Reason::TimeLimit);
                    break 'walk;
                }
                let metadata = match fs::symlink_metadata(entry.path()) {
                    Ok(metadata) => metadata,
                    Err(failure) => {
                        reasons.insert(reason_for_io(&failure));
                        continue;
                    }
                };
                // Links/reparse points and special files are excluded by policy.
                if is_link_or_reparse(&metadata) || !(metadata.is_dir() || metadata.is_file()) {
                    continue;
                }
                let name = entry.file_name();
                let Some(name) = name.to_str() else {
                    reasons.insert(Reason::UnrepresentablePath);
                    continue;
                };
                if metadata.is_dir() && FILE_DISCOVERY_EXCLUDED_DIRECTORIES.contains(&name) {
                    continue;
                }
                let path = if relative.is_empty() {
                    name.to_owned()
                } else {
                    format!("{relative}/{name}")
                };
                if !valid_file_discovery_path(&path) {
                    reasons.insert(Reason::UnrepresentablePath);
                    continue;
                }
                if metadata.is_dir() {
                    if depth >= budget.depth {
                        reasons.insert(Reason::DepthLimit);
                        continue;
                    }
                    let bytes = pending_bytes.checked_add(path.len());
                    if pending.len() >= budget.pending_dirs
                        || bytes.is_none_or(|bytes| bytes > budget.pending_bytes)
                    {
                        reasons.insert(Reason::StackLimit);
                        continue;
                    }
                    pending_bytes = bytes.expect("checked pending bytes");
                    pending.push((path, depth + 1));
                    continue;
                }
                let Some(rank) = file_discovery_rank(&path, query) else {
                    continue;
                };
                // Validate all ancestors and the leaf immediately before admitting
                // a regular-file candidate. Never open or inspect its contents.
                match self.resolve_discovery_path(&path, &mut expired) {
                    Ok((_, metadata)) if metadata.is_file() => {}
                    Ok(_) => {
                        reasons.insert(Reason::UnsafePath);
                        continue;
                    }
                    Err(failure) if failure.code == "discovery_time_limit" => {
                        reasons.insert(Reason::TimeLimit);
                        break 'walk;
                    }
                    Err(failure) => {
                        reasons.insert(reason_for_error(&failure));
                        continue;
                    }
                }
                if expired() {
                    reasons.insert(Reason::TimeLimit);
                    break 'walk;
                }
                let candidate = (rank, path);
                if candidates.contains(&candidate) {
                    continue;
                }
                if candidates.len() == limit {
                    reasons.insert(Reason::ResultLimit);
                    if candidates.last().is_some_and(|last| last <= &candidate) {
                        continue;
                    }
                    candidates.pop_last();
                }
                candidates.insert(candidate);
            }
        }
        result.paths = candidates.into_iter().map(|(_, path)| path).collect();
        result.reasons = reasons.iter().copied().collect();
        result.incomplete = !result.reasons.is_empty();
        // Include the added reason itself in final JSON accounting. Dropping
        // worst-ranked paths keeps the observed ordering without a second walk.
        if result
            .encoded_bytes()
            .map_or(true, |bytes| bytes > budget.result_bytes)
        {
            reasons.insert(Reason::ResponseLimit);
            result.reasons = reasons.into_iter().collect();
            result.incomplete = true;
            while result
                .encoded_bytes()
                .map_or(true, |bytes| bytes > budget.result_bytes)
            {
                if result.paths.pop().is_none() {
                    return Err(error(
                        "invalid_file_discovery",
                        "Discovery result metadata exceeds its byte budget",
                    ));
                }
            }
        }
        result.validate_for(query, scope, limit)?;
        Ok(result)
    }

    pub(super) fn resolve_discovery_path(
        &self,
        relative: &str,
        expired: &mut impl FnMut() -> bool,
    ) -> Result<(PathBuf, fs::Metadata), RemoteError> {
        if !relative.is_empty() && !valid_file_discovery_path(relative) {
            return Err(error("invalid_path", "Invalid discovery path"));
        }
        let mut current = self.root.clone();
        check_deadline(expired)?;
        let mut metadata = fs::symlink_metadata(&current).map_err(io_error)?;
        if is_link_or_reparse(&metadata) {
            return Err(error(
                "invalid_path",
                "Discovery does not follow links or reparse points",
            ));
        }
        if !metadata.is_dir() {
            return Err(error("not_directory", "Workspace root is not a directory"));
        }
        let components: Vec<_> = relative
            .split('/')
            .filter(|part| !part.is_empty())
            .collect();
        for (index, part) in components.iter().enumerate() {
            current.push(part);
            check_deadline(expired)?;
            metadata = fs::symlink_metadata(&current).map_err(io_error)?;
            if is_link_or_reparse(&metadata) {
                return Err(error(
                    "invalid_path",
                    "Discovery does not follow links or reparse points",
                ));
            }
            check_deadline(expired)?;
            if !fs::canonicalize(&current)
                .map_err(io_error)?
                .starts_with(&self.root)
            {
                return Err(error(
                    "invalid_path",
                    "Discovery path escapes the workspace root",
                ));
            }
            if index + 1 != components.len() && !metadata.is_dir() {
                return Err(error(
                    "not_directory",
                    "Discovery parent is not a directory",
                ));
            }
        }
        check_deadline(expired)?;
        Ok((current, metadata))
    }
}

fn check_deadline(expired: &mut impl FnMut() -> bool) -> Result<(), RemoteError> {
    if expired() {
        Err(error(
            "discovery_time_limit",
            "Discovery elapsed budget exhausted",
        ))
    } else {
        Ok(())
    }
}

pub(super) fn is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn reason_for_io(failure: &io::Error) -> Reason {
    if failure.kind() == io::ErrorKind::PermissionDenied {
        Reason::PermissionDenied
    } else {
        Reason::IoError
    }
}

fn reason_for_error(failure: &RemoteError) -> Reason {
    match failure.code.as_str() {
        "permission_denied" => Reason::PermissionDenied,
        "invalid_path" | "not_directory" => Reason::UnsafePath,
        _ => Reason::IoError,
    }
}

#[cfg(test)]
mod tests;
