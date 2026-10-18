//! Independently negotiated case-aware literal search. Stage 1 defines the
//! contract only; no workspace advertises or implements this operation yet.
use crate::{
    RemoteError, ScopedSearchIncompleteReason, ScopedSearchRequest, ScopedSearchResult,
    SearchMatch, MAX_SCOPED_SEARCH_RESULT_BYTES,
};
use serde::{
    de::{value::MapAccessDeserializer, MapAccess, Visitor},
    Deserialize, Deserializer, Serialize,
};
use std::io::{self, Write};
use std::marker::PhantomData;

pub const CASE_SEARCH_VERSION: u32 = 1;
pub const MAX_CASE_SEARCH_DESCRIPTOR_BYTES: usize = 22;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct CaseSearchDescriptor {
    pub version: u32,
}

impl CaseSearchDescriptor {
    pub const V1: Self = Self {
        version: CASE_SEARCH_VERSION,
    };

    pub fn validate(&self) -> Result<(), RemoteError> {
        if self.version == 0
            || serde_json::to_vec(self)
                .map_or(true, |bytes| bytes.len() > MAX_CASE_SEARCH_DESCRIPTOR_BYTES)
        {
            return Err(RemoteError::new(
                "invalid_case_search",
                "Case search descriptor must have a positive version and fit its compact JSON bound",
            ));
        }
        Ok(())
    }

    /// Well-formed future versions remain valid but never enable v1 requests.
    pub fn supported(&self) -> bool {
        self.version == CASE_SEARCH_VERSION
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CaseSearchRequest {
    pub query: String,
    pub scope: String,
    pub masks: Vec<String>,
    pub limit: usize,
    pub case_sensitive: bool,
}

impl CaseSearchRequest {
    /// Reuse the unchanged scoped-v1 bounds and path/mask semantics. The exact
    /// requested limit, mask order, spelling and boolean remain in the echo.
    pub fn validate(&self) -> Result<usize, RemoteError> {
        self.scoped_request().validate()
    }

    fn scoped_request(&self) -> ScopedSearchRequest {
        ScopedSearchRequest {
            query: self.query.clone(),
            scope: self.scope.clone(),
            masks: self.masks.clone(),
            limit: self.limit,
        }
    }
}

/// Strict new row decoding must not tighten the existing SearchMatch contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CaseSearchMatch {
    pub path: String,
    pub line: usize,
    pub text: String,
}

impl From<CaseSearchMatch> for SearchMatch {
    fn from(row: CaseSearchMatch) -> Self {
        Self {
            path: row.path,
            line: row.line,
            text: row.text,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CaseSearchResult {
    pub request: CaseSearchRequest,
    pub matches: Vec<CaseSearchMatch>,
    pub incomplete: bool,
    pub reasons: Vec<ScopedSearchIncompleteReason>,
    pub examined_entries: usize,
    pub read_bytes: usize,
}

impl CaseSearchResult {
    /// Count the actual case-search encoding, including the required boolean.
    pub fn encoded_bytes(&self) -> Result<usize, RemoteError> {
        struct Counter(usize);
        impl Write for Counter {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                self.0 = self
                    .0
                    .checked_add(bytes.len())
                    .filter(|n| *n <= MAX_SCOPED_SEARCH_RESULT_BYTES)
                    .ok_or_else(|| io::Error::other("case search result exceeds JSON budget"))?;
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

    pub fn validate_for(&self, request: &CaseSearchRequest) -> Result<(), RemoteError> {
        let limit = request.validate()?;
        if &self.request != request || self.matches.len() > limit {
            return Err(invalid_result());
        }
        // Bound serialized data before copying into the existing validator.
        // This keeps one source of truth for all scoped membership, row,
        // counter and incompleteness semantics without changing old decoding.
        self.encoded_bytes()?;
        let scoped = request.scoped_request();
        let result = ScopedSearchResult {
            request: scoped.clone(),
            matches: self
                .matches
                .iter()
                .cloned()
                .map(SearchMatch::from)
                .collect(),
            incomplete: self.incomplete,
            reasons: self.reasons.clone(),
            examined_entries: self.examined_entries,
            read_bytes: self.read_bytes,
        };
        result.validate_for(&scoped).map_err(|_| invalid_result())
    }
}

fn invalid_result() -> RemoteError {
    RemoteError::new(
        "invalid_case_search",
        "Case search result is malformed, inconsistent, or oversized",
    )
}

// Derived structs also accept positional arrays. Forward maps only, preserving
// derived missing/duplicate/unknown-field checks, including escaped keys.
fn deserialize_object<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct Object<T>(PhantomData<T>);
    impl<'de, T: Deserialize<'de>> Visitor<'de> for Object<T> {
        type Value = T;
        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a case-search JSON object")
        }
        fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<T, A::Error> {
            T::deserialize(MapAccessDeserializer::new(map))
        }
    }
    deserializer.deserialize_map(Object(PhantomData))
}

impl<'de> Deserialize<'de> for CaseSearchDescriptor {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Fields {
            version: u32,
        }
        let fields: Fields = deserialize_object(deserializer)?;
        if fields.version == 0 {
            return Err(serde::de::Error::custom(
                "case_search version must be positive",
            ));
        }
        Ok(Self {
            version: fields.version,
        })
    }
}

impl<'de> Deserialize<'de> for CaseSearchRequest {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Fields {
            query: String,
            scope: String,
            masks: Vec<String>,
            limit: usize,
            case_sensitive: bool,
        }
        let fields: Fields = deserialize_object(deserializer)?;
        Ok(Self {
            query: fields.query,
            scope: fields.scope,
            masks: fields.masks,
            limit: fields.limit,
            case_sensitive: fields.case_sensitive,
        })
    }
}

impl<'de> Deserialize<'de> for CaseSearchMatch {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Fields {
            path: String,
            line: usize,
            text: String,
        }
        let fields: Fields = deserialize_object(deserializer)?;
        Ok(Self {
            path: fields.path,
            line: fields.line,
            text: fields.text,
        })
    }
}

impl<'de> Deserialize<'de> for CaseSearchResult {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Fields {
            request: CaseSearchRequest,
            matches: Vec<CaseSearchMatch>,
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
