//! True scoped text traversal. Ancestor checks narrow filesystem races; this is
//! not a sandbox against a process concurrently replacing filesystem names.
use crate::case_matcher::CaseMatcher;
use crate::file_discovery::is_link_or_reparse;
use crate::{error, search_preview, search_preview_at, Workspace};
use cedar_protocol::{
    scoped_search_masks_match, valid_file_discovery_path, CaseSearchMatch, CaseSearchRequest,
    CaseSearchResult, Payload, RemoteError, ScopedSearchIncompleteReason as Reason,
    ScopedSearchRequest, ScopedSearchResult, SearchMatch, FILE_DISCOVERY_EXCLUDED_DIRECTORIES,
    MAX_FILE_BYTES, MAX_SCOPED_SEARCH_DEPTH, MAX_SCOPED_SEARCH_ENTRIES,
    MAX_SCOPED_SEARCH_PENDING_BYTES, MAX_SCOPED_SEARCH_PENDING_DIRS, MAX_SCOPED_SEARCH_READ_BYTES,
    MAX_SCOPED_SEARCH_RESULT_BYTES,
};
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{self, Read};
use std::time::{Duration, Instant};

const MAX_DURATION: Duration = Duration::from_secs(5);

#[derive(Clone, Copy)]
struct Budget {
    entries: usize,
    depth: usize,
    pending_dirs: usize,
    pending_bytes: usize,
    read_bytes: usize,
    result_bytes: usize,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            entries: MAX_SCOPED_SEARCH_ENTRIES,
            depth: MAX_SCOPED_SEARCH_DEPTH,
            pending_dirs: MAX_SCOPED_SEARCH_PENDING_DIRS,
            pending_bytes: MAX_SCOPED_SEARCH_PENDING_BYTES,
            read_bytes: MAX_SCOPED_SEARCH_READ_BYTES,
            result_bytes: MAX_SCOPED_SEARCH_RESULT_BYTES,
        }
    }
}

// Instrumentation exists only in unit-test builds, not a protocol/host option.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TestEvent {
    Compile,
    Compiled,
    Match,
    Directory,
    FileCheck,
    Open,
}

impl Workspace {
    pub(super) fn search_in(&self, request: ScopedSearchRequest) -> Result<Payload, RemoteError> {
        let started = Instant::now();
        let result = self.search_in_with_budget(
            &request,
            Budget::default(),
            || started.elapsed() >= MAX_DURATION,
            #[cfg(test)]
            |_, _| {},
        )?;
        Ok(Payload::ScopedMatches { result })
    }

    fn search_in_with_budget(
        &self,
        request: &ScopedSearchRequest,
        budget: Budget,
        expired: impl FnMut() -> bool,
        #[cfg(test)] event: impl FnMut(TestEvent, &str),
    ) -> Result<ScopedSearchResult, RemoteError> {
        self.search_with_matcher(
            request,
            budget,
            None,
            expired,
            #[cfg(test)]
            event,
        )
    }

    pub(super) fn search_case(&self, request: CaseSearchRequest) -> Result<Payload, RemoteError> {
        // One cooperative deadline includes compilation; no forced interruption
        // of a compile, match, filesystem call or finalization is promised.
        let started = Instant::now();
        let result = self.search_case_with_budget(
            &request,
            Budget::default(),
            || started.elapsed() >= MAX_DURATION,
            #[cfg(test)]
            |_, _| {},
            #[cfg(test)]
            None,
        )?;
        Ok(Payload::CaseMatches { result })
    }

    fn search_case_with_budget(
        &self,
        request: &CaseSearchRequest,
        mut budget: Budget,
        mut expired: impl FnMut() -> bool,
        #[cfg(test)] mut event: impl FnMut(TestEvent, &str),
        #[cfg(test)] nfa_limit: Option<usize>,
    ) -> Result<CaseSearchResult, RemoteError> {
        request.validate()?;
        let mut empty = CaseSearchResult {
            request: request.clone(),
            matches: Vec::new(),
            incomplete: false,
            reasons: Vec::new(),
            examined_entries: 0,
            read_bytes: 0,
        };
        if expired() {
            empty.incomplete = true;
            empty.reasons.push(Reason::TimeLimit);
            empty.validate_for(request)?;
            return Ok(empty);
        }
        #[cfg(test)]
        event(TestEvent::Compile, "");
        #[cfg(test)]
        let matcher = match nfa_limit {
            Some(limit) => {
                CaseMatcher::with_test_nfa_limit(&request.query, request.case_sensitive, limit)
            }
            None => CaseMatcher::new(&request.query, request.case_sensitive),
        }?;
        #[cfg(not(test))]
        let matcher = CaseMatcher::new(&request.query, request.case_sensitive)?;
        #[cfg(test)]
        event(TestEvent::Compiled, "");
        // A build error above wins over the post-build elapsed observation.
        if expired() {
            empty.incomplete = true;
            empty.reasons.push(Reason::TimeLimit);
            empty.validate_for(request)?;
            return Ok(empty);
        }
        let scoped = ScopedSearchRequest {
            query: request.query.clone(),
            scope: request.scope.clone(),
            masks: request.masks.clone(),
            limit: request.limit,
        };
        // Rows/counters/reasons have identical JSON shapes. Derive the exact
        // request-envelope overhead from serialization (including true/false),
        // and reserve it before the unchanged scoped response prefix crop.
        let scoped_empty = ScopedSearchResult {
            request: scoped.clone(),
            matches: Vec::new(),
            incomplete: false,
            reasons: Vec::new(),
            examined_entries: 0,
            read_bytes: 0,
        };
        let overhead = empty
            .encoded_bytes()?
            .checked_sub(scoped_empty.encoded_bytes()?)
            .ok_or_else(case_metadata_error)?;
        budget.result_bytes = budget
            .result_bytes
            .checked_sub(overhead)
            .ok_or_else(case_metadata_error)?;
        let result = self.search_with_matcher(
            &scoped,
            budget,
            Some(&matcher),
            expired,
            #[cfg(test)]
            event,
        )?;
        let result = CaseSearchResult {
            request: request.clone(),
            matches: result
                .matches
                .into_iter()
                .map(|row| CaseSearchMatch {
                    path: row.path,
                    line: row.line,
                    text: row.text,
                })
                .collect(),
            incomplete: result.incomplete,
            reasons: result.reasons,
            examined_entries: result.examined_entries,
            read_bytes: result.read_bytes,
        };
        result.validate_for(request)?;
        Ok(result)
    }

    fn search_with_matcher(
        &self,
        request: &ScopedSearchRequest,
        budget: Budget,
        matcher: Option<&CaseMatcher>,
        mut expired: impl FnMut() -> bool,
        #[cfg(test)] mut event: impl FnMut(TestEvent, &str),
    ) -> Result<ScopedSearchResult, RemoteError> {
        let limit = request.validate()?;
        let mut result = ScopedSearchResult {
            request: request.clone(),
            matches: Vec::new(),
            incomplete: false,
            reasons: Vec::new(),
            examined_entries: 0,
            read_bytes: 0,
        };
        let mut reasons = BTreeSet::new();
        let mut pending = vec![(request.scope.clone(), 0usize)];
        let mut pending_bytes = request.scope.len();
        if pending.len() > budget.pending_dirs || pending_bytes > budget.pending_bytes {
            pending.clear();
            reasons.insert(Reason::StackLimit);
        }
        'walk: while let Some((relative, depth)) = pending.pop() {
            pending_bytes = pending_bytes
                .checked_sub(relative.len())
                .expect("accounted stack path");
            if expired() {
                reasons.insert(Reason::TimeLimit);
                break;
            }
            #[cfg(test)]
            event(TestEvent::Directory, &relative);
            let (directory, metadata) = match self.resolve_discovery_path(&relative, &mut expired) {
                Ok(value) => value,
                Err(failure) if failure.code == "discovery_time_limit" => {
                    reasons.insert(Reason::TimeLimit);
                    break;
                }
                Err(failure) if relative == request.scope && failure.code == "invalid_path" => {
                    return Err(failure)
                }
                Err(failure) => {
                    reasons.insert(reason_for_error(&failure));
                    continue;
                }
            };
            if !metadata.is_dir() {
                if relative == request.scope {
                    return Err(error("not_directory", "Search scope must be a directory"));
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
                // Never consume a 20,001st entry. At an exact boundary, report
                // conservative incompleteness rather than probe one entry more.
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
                // Declared exclusions: links/reparse points, special files,
                // fixed skip directories, binary, invalid UTF-8 and oversized files.
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
                // Scope is enforced by where traversal starts, and masks before
                // any content open. Unmatched files still consume entry budget.
                if !scoped_search_masks_match(&request.masks, name) {
                    continue;
                }
                if expired() {
                    reasons.insert(Reason::TimeLimit);
                    break 'walk;
                }
                #[cfg(test)]
                event(TestEvent::FileCheck, &path);
                let (full, metadata) = match self.resolve_discovery_path(&path, &mut expired) {
                    Ok(value) if value.1.is_file() => value,
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
                };
                if metadata.len() > MAX_FILE_BYTES as u64 {
                    continue;
                }
                if expired() {
                    reasons.insert(Reason::TimeLimit);
                    break 'walk;
                }
                if result.read_bytes >= budget.read_bytes {
                    reasons.insert(Reason::ReadByteLimit);
                    break 'walk;
                }
                #[cfg(test)]
                event(TestEvent::Open, &path);
                let mut file = match File::open(&full) {
                    Ok(file) => file,
                    Err(failure) => {
                        reasons.insert(reason_for_io(&failure));
                        continue;
                    }
                };
                // Recheck the named ancestors and leaf after opening, before
                // reading any bytes. This catches a persistent replacement in
                // the preceding gap, but cannot undo open or defeat swap-back.
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
                let opened_metadata = match file.metadata() {
                    Ok(metadata) => metadata,
                    Err(failure) => {
                        reasons.insert(reason_for_io(&failure));
                        continue;
                    }
                };
                if !opened_metadata.is_file() || is_link_or_reparse(&opened_metadata) {
                    reasons.insert(Reason::UnsafePath);
                    continue;
                }
                if opened_metadata.len() > MAX_FILE_BYTES as u64 {
                    continue;
                }
                let text = match read_scoped_text(
                    &mut file,
                    &mut result.read_bytes,
                    budget.read_bytes,
                    &mut expired,
                ) {
                    FileRead::Text(text) => text,
                    FileRead::Excluded => continue,
                    FileRead::Failure(reason) => {
                        reasons.insert(reason);
                        continue;
                    }
                    FileRead::Stopped(reason) => {
                        reasons.insert(reason);
                        break 'walk;
                    }
                };
                for (line, value) in text.lines().enumerate() {
                    if expired() {
                        reasons.insert(Reason::TimeLimit);
                        break 'walk;
                    }
                    let span = if let Some(matcher) = matcher {
                        let span = matcher.find(value);
                        #[cfg(test)]
                        event(TestEvent::Match, &path);
                        // Include nonmatches and the final line in cooperative
                        // elapsed checks; never expose a row from an overrun.
                        if expired() {
                            reasons.insert(Reason::TimeLimit);
                            break 'walk;
                        }
                        let Some(span) = span else { continue };
                        if value.get(span.clone()).is_none() {
                            return Err(error(
                                "case_search_matcher_error",
                                "Invalid literal match span",
                            ));
                        }
                        Some(span)
                    } else {
                        // Keep old SearchIn matching, preview lookup and timing
                        // checks unchanged. Only SearchCase adds post-match checks.
                        if !value.contains(&request.query) {
                            continue;
                        }
                        None
                    };
                    if result.matches.len() == limit {
                        reasons.insert(Reason::ResultLimit);
                        break 'walk;
                    }
                    result.matches.push(SearchMatch {
                        path: path.clone(),
                        line: line + 1,
                        text: match span {
                            Some(span) => search_preview_at(value, span.start),
                            None => search_preview(value, &request.query),
                        },
                    });
                }
            }
        }
        result
            .matches
            .sort_by(|a, b| a.path.cmp(&b.path).then_with(|| a.line.cmp(&b.line)));
        // Concurrent namespace changes can expose an already-observed path.
        result
            .matches
            .dedup_by(|a, b| a.path == b.path && a.line == b.line);
        result.reasons = reasons.iter().copied().collect();
        result.incomplete = !result.reasons.is_empty();
        if result
            .encoded_bytes()
            .map_or(true, |bytes| bytes > budget.result_bytes)
        {
            reasons.insert(Reason::ResponseLimit);
            result.reasons = reasons.into_iter().collect();
            result.incomplete = true;
            // Determine the retained prefix in one pass, including escaping,
            // brackets, separators and the newly added reason. Avoid repeatedly
            // serializing a large shrinking vector after the elapsed deadline.
            let rows = std::mem::take(&mut result.matches);
            let mut bytes = result.encoded_bytes()?;
            if bytes > budget.result_bytes {
                return Err(error(
                    "invalid_scoped_search",
                    "Scoped search result metadata exceeds its byte budget",
                ));
            }
            for row in rows {
                let row_bytes = serde_json::to_vec(&row)
                    .map_err(|_| error("invalid_scoped_search", "Cannot encode search row"))?
                    .len();
                let next = bytes.checked_add(row_bytes + usize::from(!result.matches.is_empty()));
                if next.is_none_or(|next| next > budget.result_bytes) {
                    break;
                }
                bytes = next.expect("checked response bytes");
                result.matches.push(row);
            }
        }
        result.validate_for(request)?;
        Ok(result)
    }
}

fn case_metadata_error() -> RemoteError {
    error(
        "invalid_case_search",
        "Case search result metadata exceeds its byte budget",
    )
}

enum FileRead {
    Text(String),
    Excluded,
    Failure(Reason),
    Stopped(Reason),
}

fn read_scoped_text(
    reader: &mut impl Read,
    read_bytes: &mut usize,
    byte_limit: usize,
    expired: &mut impl FnMut() -> bool,
) -> FileRead {
    let mut contents = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        if expired() {
            return FileRead::Stopped(Reason::TimeLimit);
        }
        let remaining = byte_limit.saturating_sub(*read_bytes);
        if remaining == 0 {
            return FileRead::Stopped(Reason::ReadByteLimit);
        }
        let count = chunk
            .len()
            .min(remaining)
            .min(MAX_FILE_BYTES + 1 - contents.len());
        match reader.read(&mut chunk[..count]) {
            Ok(0) => break,
            Ok(count) => {
                *read_bytes += count; // read size was capped by the remaining global budget
                contents.extend_from_slice(&chunk[..count]);
                if contents.len() > MAX_FILE_BYTES || chunk[..count].contains(&0) {
                    return FileRead::Excluded;
                }
            }
            Err(failure) if failure.kind() == io::ErrorKind::Interrupted => continue,
            Err(failure) => return FileRead::Failure(reason_for_io(&failure)),
        }
    }
    match String::from_utf8(contents) {
        Ok(text) => FileRead::Text(text),
        Err(_) => FileRead::Excluded,
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
mod case_tests;
#[cfg(test)]
mod tests;
