//! Read-only captured POM declarations, separate from bounded JDT observations.
use crate::RemoteError;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::{self, Write};

pub const MAVEN_DEPENDENCIES_SCHEMA: u32 = 1;
pub const MAX_MAVEN_DEPENDENCIES: usize = 256;
pub const MAX_MAVEN_DEPENDENCIES_BYTES: usize = 128 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MavenDependenciesSnapshot {
    pub schema: u32,
    pub profile: String,
    pub startup_id: u64,
    pub pom_path: String,
    pub pom_sha256: String,
    pub declarations: Vec<MavenDependencyDeclaration>,
    pub observation: MavenDependencyObservation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MavenDependencyDeclaration {
    pub group_id: String,
    pub artifact_id: String,
    pub version: String,
    pub classifier: Option<String>,
    pub scope: MavenDependencyScope,
    pub scope_explicit: bool,
    pub optional: bool,
    pub optional_explicit: bool,
    pub expected_jar_path: String,
    /// A checked regular file existed at observation time. No integrity claim.
    pub regular_file_present: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MavenDependencyScope {
    Compile,
    Provided,
    Runtime,
    Test,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum MavenDependencyObservation {
    Available {
        libraries: Vec<MavenObservedLibrary>,
    },
    Unavailable {
        reason: MavenDependencyUnavailableReason,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MavenDependencyUnavailableReason {
    ModelUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MavenObservedLibrary {
    pub root: MavenLibraryRoot,
    pub relative_path: String,
    pub regular_file_present: bool,
    /// All exact normalized path matches, in captured declaration order.
    /// More than one index is ambiguous; consumers must not select the first.
    pub declaration_indices: Vec<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MavenLibraryRoot {
    Workspace,
    LocalRepository,
}

fn invalid() -> RemoteError {
    RemoteError::new(
        "language_maven_invalid_dependencies",
        "Maven dependency metadata is malformed, inconsistent, or oversized",
    )
}

fn bounded_json(value: &impl Serialize) -> Result<(), RemoteError> {
    struct Budget(usize);
    impl Write for Budget {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0 = self
                .0
                .checked_sub(bytes.len())
                .ok_or_else(|| io::Error::other("JSON limit"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(Budget(MAX_MAVEN_DEPENDENCIES_BYTES), value).map_err(|_| invalid())
}

fn safe_component(value: &str) -> bool {
    if value.is_empty()
        || matches!(value, "." | "..")
        || value.ends_with(['.', ' '])
        || value.chars().any(|c| {
            c.is_control() || matches!(c, '/' | '\\' | ':' | '<' | '>' | '"' | '|' | '?' | '*')
        })
    {
        return false;
    }
    let stem = value.split('.').next().unwrap_or("").to_ascii_uppercase();
    !matches!(
        stem.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "CONIN$"
            | "CONOUT$"
            | "COM¹"
            | "COM²"
            | "COM³"
            | "LPT¹"
            | "LPT²"
            | "LPT³"
    ) && !(stem.len() == 4
        && (stem.starts_with("COM") || stem.starts_with("LPT"))
        && matches!(stem.as_bytes()[3], b'1'..=b'9'))
}

fn coordinate(value: &str, group: bool) -> bool {
    value.len() <= 256
        && safe_component(value)
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
        && (!group || value.split('.').all(safe_component))
        && !value.eq_ignore_ascii_case("LATEST")
        && !value.eq_ignore_ascii_case("RELEASE")
}

fn relative_path(value: &str, windows: bool) -> bool {
    !value.is_empty()
        && value.len() <= 4096
        && !value.contains(['\\', ':'])
        && !value.chars().any(char::is_control)
        && value.split('/').all(|part| {
            !part.is_empty() && !matches!(part, "." | "..") && (!windows || safe_component(part))
        })
}

fn path_key(value: &str, windows: bool) -> String {
    if windows {
        value.to_ascii_lowercase()
    } else {
        value.to_owned()
    }
}

impl MavenDependencyDeclaration {
    pub fn repository_jar_path(&self) -> String {
        let classifier = self
            .classifier
            .as_ref()
            .map(|value| format!("-{value}"))
            .unwrap_or_default();
        format!(
            "{}/{}/{}/{}-{}{}.jar",
            self.group_id.replace('.', "/"),
            self.artifact_id,
            self.version,
            self.artifact_id,
            self.version,
            classifier
        )
    }
}

impl MavenDependenciesSnapshot {
    /// Validate untrusted JSON before making an owned frontend snapshot copy.
    pub fn parse_for(
        value: &serde_json::Value,
        startup_id: u64,
        pom_sha256: &str,
        windows_paths: bool,
    ) -> Result<Self, RemoteError> {
        bounded_json(value)?;
        let snapshot: Self = serde_json::from_value(value.clone()).map_err(|_| invalid())?;
        snapshot.validate_for(startup_id, pom_sha256, windows_paths)?;
        Ok(snapshot)
    }

    /// The caller supplies the actual owned session identity and host path rules.
    pub fn validate_for(
        &self,
        startup_id: u64,
        pom_sha256: &str,
        windows_paths: bool,
    ) -> Result<(), RemoteError> {
        if self.schema != MAVEN_DEPENDENCIES_SCHEMA
            || self.profile != "maven_leaf"
            || self.startup_id == 0
            || self.startup_id != startup_id
            || self.pom_path != "pom.xml"
            || self.pom_sha256 != pom_sha256
            || self.pom_sha256.len() != 64
            || !self
                .pom_sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || self.declarations.len() > MAX_MAVEN_DEPENDENCIES
        {
            return Err(invalid());
        }
        let mut coordinates = HashSet::new();
        for declaration in &self.declarations {
            if !coordinate(&declaration.group_id, true)
                || !coordinate(&declaration.artifact_id, false)
                || !coordinate(&declaration.version, false)
                || declaration
                    .classifier
                    .as_ref()
                    .is_some_and(|value| !coordinate(value, false))
                || (!declaration.scope_explicit
                    && declaration.scope != MavenDependencyScope::Compile)
                || (!declaration.optional_explicit && declaration.optional)
                || declaration.expected_jar_path != declaration.repository_jar_path()
                || !relative_path(&declaration.expected_jar_path, windows_paths)
                || !coordinates.insert((
                    &declaration.group_id,
                    &declaration.artifact_id,
                    &declaration.classifier,
                ))
            {
                return Err(invalid());
            }
        }
        if let MavenDependencyObservation::Available { libraries } = &self.observation {
            if libraries.len() > MAX_MAVEN_DEPENDENCIES {
                return Err(invalid());
            }
            let mut paths = HashSet::new();
            let mut links = 0usize;
            for library in libraries {
                let key = path_key(&library.relative_path, windows_paths);
                if !relative_path(&library.relative_path, windows_paths)
                    || !paths.insert((library.root, key.clone()))
                    || library.declaration_indices.len() > MAX_MAVEN_DEPENDENCIES
                {
                    return Err(invalid());
                }
                links = links
                    .checked_add(library.declaration_indices.len())
                    .ok_or_else(invalid)?;
                if links > MAX_MAVEN_DEPENDENCIES {
                    return Err(invalid());
                }
                let expected: Vec<u16> = self
                    .declarations
                    .iter()
                    .enumerate()
                    .filter(|(_, declaration)| {
                        library.root == MavenLibraryRoot::LocalRepository
                            && path_key(&declaration.expected_jar_path, windows_paths) == key
                    })
                    .map(|(index, _)| index as u16)
                    .collect();
                // Equality also rejects duplicate, unsorted, out-of-bounds, missing,
                // or foreign links, including a first-match-only case collision.
                if library.declaration_indices != expected {
                    return Err(invalid());
                }
            }
        }
        // Include the typed payload wrapper in the final wire budget.
        #[derive(Serialize)]
        struct Envelope<'a> {
            r#type: &'static str,
            snapshot: &'a MavenDependenciesSnapshot,
        }
        bounded_json(&Envelope {
            r#type: "maven_dependencies",
            snapshot: self,
        })
    }
}
