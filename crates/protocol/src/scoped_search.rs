//! Independently negotiated, bounded literal text search in an exact scope.
use crate::{
    valid_file_discovery_path, RemoteError, SearchMatch, FILE_DISCOVERY_EXCLUDED_DIRECTORIES,
    MAX_FILE_BYTES,
};
use serde::{
    de::{value::MapAccessDeserializer, MapAccess, Visitor},
    Deserialize, Deserializer, Serialize,
};
use std::io::{self, Write};
use std::marker::PhantomData;

pub const SCOPED_SEARCH_VERSION: u32 = 1;
pub const MAX_SCOPED_SEARCH_DESCRIPTOR_BYTES: usize = 22;
pub const MAX_SCOPED_SEARCH_QUERY_BYTES: usize = 4096;
pub const MAX_SCOPED_SEARCH_MASKS: usize = 8;
pub const MAX_SCOPED_SEARCH_MASK_BYTES: usize = 256;
pub const MAX_SCOPED_SEARCH_RESULTS: usize = 1000;
pub const MAX_SCOPED_SEARCH_ENTRIES: usize = 20_000;
pub const MAX_SCOPED_SEARCH_DEPTH: usize = 64;
pub const MAX_SCOPED_SEARCH_PENDING_DIRS: usize = 4096;
pub const MAX_SCOPED_SEARCH_PENDING_BYTES: usize = 512 * 1024;
pub const MAX_SCOPED_SEARCH_READ_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_SCOPED_SEARCH_RESULT_BYTES: usize = 512 * 1024;
/// The legacy 2048-byte preview window plus two UTF-8 ellipses.
pub const MAX_SCOPED_SEARCH_SNIPPET_BYTES: usize = 2054;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ScopedSearchDescriptor {
    pub version: u32,
}

fn positive_version<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    let version = u32::deserialize(deserializer)?;
    if version == 0 {
        return Err(serde::de::Error::custom(
            "scoped_search version must be positive",
        ));
    }
    Ok(version)
}

impl ScopedSearchDescriptor {
    pub const V1: Self = Self {
        version: SCOPED_SEARCH_VERSION,
    };

    pub fn validate(&self) -> Result<(), RemoteError> {
        if self.version == 0
            || serde_json::to_vec(self).map_or(true, |bytes| {
                bytes.len() > MAX_SCOPED_SEARCH_DESCRIPTOR_BYTES
            })
        {
            return Err(RemoteError::new("invalid_scoped_search", "Scoped search descriptor must have a positive version and fit its compact JSON bound"));
        }
        Ok(())
    }

    pub fn supported(&self) -> bool {
        self.version == SCOPED_SEARCH_VERSION
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScopedSearchRequest {
    pub query: String,
    pub scope: String,
    pub masks: Vec<String>,
    pub limit: usize,
}

impl ScopedSearchRequest {
    /// Returns the effective result limit; the wire echo retains the exact request.
    pub fn validate(&self) -> Result<usize, RemoteError> {
        if self.query.is_empty()
            || self.query.len() > MAX_SCOPED_SEARCH_QUERY_BYTES
            || self.query.contains(['\n', '\r', '\0'])
        {
            return Err(RemoteError::new(
                "invalid_query",
                "Search requires a nonempty, single-line query of at most 4096 bytes",
            ));
        }
        if !self.scope.is_empty() && !valid_file_discovery_path(&self.scope) {
            return Err(RemoteError::new(
                "invalid_path",
                "Search scope must be an exact relative directory path, or empty for the root",
            ));
        }
        if self
            .scope
            .split('/')
            .any(|part| FILE_DISCOVERY_EXCLUDED_DIRECTORIES.contains(&part))
        {
            return Err(RemoteError::new(
                "excluded_scope",
                "Search scope is inside an excluded directory",
            ));
        }
        let mask_bytes = self
            .masks
            .iter()
            .try_fold(0usize, |sum, mask| sum.checked_add(mask.len()));
        if self.masks.len() > MAX_SCOPED_SEARCH_MASKS
            || mask_bytes.is_none_or(|bytes| bytes > MAX_SCOPED_SEARCH_MASK_BYTES)
            || self.masks.iter().any(|mask| {
                mask.is_empty()
                    || mask.chars().any(|ch| {
                        ch.is_control()
                            || matches!(ch, '/' | '\\' | ';' | '[' | ']' | '{' | '}' | '!')
                    })
            })
        {
            return Err(RemoteError::new("invalid_masks", "Search accepts at most 8 nonempty basename masks totaling 256 UTF-8 bytes; only * and ? are wildcard syntax"));
        }
        if self.limit == 0 {
            return Err(RemoteError::new(
                "invalid_limit",
                "Search result limit must be positive",
            ));
        }
        Ok(self.limit.min(MAX_SCOPED_SEARCH_RESULTS))
    }
}

/// Whole-basename, case-sensitive Unicode-scalar matching. No normalization,
/// regex, separators, classes or recursive backtracking. Inputs are bounded by
/// the validated request and portable path limits at each caller.
pub fn scoped_search_mask_matches(mask: &str, basename: &str) -> bool {
    let pattern: Vec<char> = mask.chars().collect();
    let name: Vec<char> = basename.chars().collect();
    let (mut p, mut n) = (0, 0);
    let mut star = None;
    let mut retry = 0;
    while n < name.len() {
        if p < pattern.len() && (pattern[p] == '?' || (pattern[p] != '*' && pattern[p] == name[n]))
        {
            p += 1;
            n += 1;
        } else if p < pattern.len() && pattern[p] == '*' {
            star = Some(p);
            p += 1;
            retry = n;
        } else if let Some(last_star) = star {
            retry += 1;
            n = retry;
            p = last_star + 1;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == '*' {
        p += 1;
    }
    p == pattern.len()
}

pub fn scoped_search_masks_match(masks: &[String], basename: &str) -> bool {
    masks.is_empty()
        || masks
            .iter()
            .any(|mask| scoped_search_mask_matches(mask, basename))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopedSearchIncompleteReason {
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
    ReadByteLimit,
}

impl ScopedSearchIncompleteReason {
    pub const COUNT: usize = 11;
}

#[derive(Debug, Clone, Serialize)]
pub struct ScopedSearchResult {
    pub request: ScopedSearchRequest,
    pub matches: Vec<SearchMatch>,
    pub incomplete: bool,
    pub reasons: Vec<ScopedSearchIncompleteReason>,
    pub examined_entries: usize,
    /// All bytes actually returned by file reads, including rejected text and
    /// the extra byte used to detect a file growing past its accepted size.
    pub read_bytes: usize,
}

impl ScopedSearchResult {
    pub fn encoded_bytes(&self) -> Result<usize, RemoteError> {
        struct Counter(usize);
        impl Write for Counter {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                self.0 = self
                    .0
                    .checked_add(bytes.len())
                    .filter(|n| *n <= MAX_SCOPED_SEARCH_RESULT_BYTES)
                    .ok_or_else(|| io::Error::other("scoped search result exceeds JSON budget"))?;
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

    pub fn validate_for(&self, request: &ScopedSearchRequest) -> Result<(), RemoteError> {
        let limit = request.validate()?;
        if &self.request != request
            || self.matches.len() > limit
            || self.examined_entries > MAX_SCOPED_SEARCH_ENTRIES
            || self.read_bytes > MAX_SCOPED_SEARCH_READ_BYTES
            || (!self.matches.is_empty() && (self.examined_entries == 0 || self.read_bytes == 0))
            || self.reasons.len() > ScopedSearchIncompleteReason::COUNT
            || self.incomplete != !self.reasons.is_empty()
            || self
                .reasons
                .iter()
                .enumerate()
                .any(|(i, reason)| self.reasons[..i].contains(reason))
        {
            return Err(invalid_result());
        }
        let mut previous = None;
        for row in &self.matches {
            let scoped_path = if request.scope.is_empty() {
                Some(row.path.as_str())
            } else {
                row.path
                    .strip_prefix(&request.scope)
                    .and_then(|tail| tail.strip_prefix('/'))
            };
            let basename = row.path.rsplit('/').next().unwrap_or("");
            if !valid_file_discovery_path(&row.path)
                || scoped_path.is_none_or(|path| {
                    path.bytes().filter(|b| *b == b'/').count() > MAX_SCOPED_SEARCH_DEPTH
                })
                || row
                    .path
                    .split('/')
                    .rev()
                    .skip(1)
                    .any(|part| FILE_DISCOVERY_EXCLUDED_DIRECTORIES.contains(&part))
                || !scoped_search_masks_match(&request.masks, basename)
                || row.line == 0
                || row.line > MAX_FILE_BYTES
                || row.text.len() > MAX_SCOPED_SEARCH_SNIPPET_BYTES
                || row.text.contains(['\n', '\0'])
            {
                return Err(invalid_result());
            }
            let key = (row.path.as_str(), row.line);
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
        "invalid_scoped_search",
        "Scoped search result is malformed, inconsistent, or oversized",
    )
}

// Derive alone also accepts positional sequences for structs. These extension
// types have an object-only JSON contract, while legacy wire decoding remains
// untouched. Forwarding the map preserves derived duplicate and unknown checks.
fn deserialize_object<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct Object<T>(PhantomData<T>);
    impl<'de, T: Deserialize<'de>> Visitor<'de> for Object<T> {
        type Value = T;
        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a scoped-search JSON object")
        }
        fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<T, A::Error> {
            T::deserialize(MapAccessDeserializer::new(map))
        }
    }
    deserializer.deserialize_map(Object(PhantomData))
}

impl<'de> Deserialize<'de> for ScopedSearchDescriptor {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Fields {
            #[serde(deserialize_with = "positive_version")]
            version: u32,
        }
        let fields: Fields = deserialize_object(deserializer)?;
        Ok(Self {
            version: fields.version,
        })
    }
}

impl<'de> Deserialize<'de> for ScopedSearchRequest {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Fields {
            query: String,
            scope: String,
            masks: Vec<String>,
            limit: usize,
        }
        let fields: Fields = deserialize_object(deserializer)?;
        Ok(Self {
            query: fields.query,
            scope: fields.scope,
            masks: fields.masks,
            limit: fields.limit,
        })
    }
}

impl<'de> Deserialize<'de> for ScopedSearchResult {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Fields {
            request: ScopedSearchRequest,
            matches: Vec<SearchMatch>,
            incomplete: bool,
            reasons: Vec<ScopedSearchIncompleteReason>,
            examined_entries: usize,
            read_bytes: usize,
        }
        let fields: Fields = deserialize_object(deserializer)?;
        Ok(Self {
            request: fields.request,
            matches: fields.matches,
            incomplete: fields.incomplete,
            reasons: fields.reasons,
            examined_entries: fields.examined_entries,
            read_bytes: fields.read_bytes,
        })
    }
}
