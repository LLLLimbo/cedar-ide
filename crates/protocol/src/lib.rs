//! Bounded newline-delimited JSON protocol between native UI and workspace agent.
use serde::{Deserialize, Serialize};
use std::io::{self, BufRead, Write};
pub const PROTOCOL_VERSION: u32 = 4;
pub const MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_FILE_BYTES: usize = 1024 * 1024;
pub const AGENT_INFO_SCHEMA: u32 = 1;
pub const MAX_AGENT_VERSION_BYTES: usize = 64;
pub const MAX_AGENT_PLATFORM_BYTES: usize = 32;
pub const MAX_AGENT_CAPABILITIES: usize = 32;
pub const MAX_CAPABILITY_BYTES: usize = 64;
/// Minimum complete lifecycle required before starting a managed command task.
pub const RUN_TASK_CAPABILITIES: &[&str] = &["run_start", "run_poll", "run_cancel"];
/// Minimum generic session Cedar must be able to synchronize and shut down.
/// Queries, navigation, formatting and completion resolution remain optional.
pub const LANGUAGE_SESSION_CAPABILITIES: &[&str] = &[
    "language_start",
    "language_open",
    "language_change",
    "language_close",
    "language_events",
    "language_stop",
];
/// Scoped Java startup uses the same synchronization/shutdown lifecycle, without
/// implying that the backend supports arbitrary language-server commands.
pub const JAVA_LANGUAGE_SESSION_CAPABILITIES: &[&str] = &[
    "language_start_java",
    "language_open",
    "language_change",
    "language_close",
    "language_events",
    "language_stop",
];

/// Optional owned startup lifecycle. Legacy Java startup stays synchronous.
pub const JAVA_STARTUP_CAPABILITIES: &[&str] = &[
    "language_start_java_begin",
    "language_start_java_poll",
    "language_start_java_cancel",
];

/// Explicit opt-in Maven profile. A peer must implement the entire owned Java
/// lifecycle as well; this group never permits a generic executeCommand bridge.
pub const JAVA_MAVEN_CAPABILITIES: &[&str] =
    &["language_start_java_maven_begin", "language_maven_model"];

/// Unverified implementation information, never execution permission or identity.
/// Validate received information before retaining it as a connection snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentInfo {
    pub schema: u32,
    pub version: String,
    pub os: String,
    pub arch: String,
    pub capabilities: Vec<String>,
}

impl AgentInfo {
    pub fn validate(&self) -> Result<(), RemoteError> {
        let invalid = |message| RemoteError::new("invalid_agent_info", message);
        if self.schema != AGENT_INFO_SCHEMA {
            return Err(invalid("Unsupported agent metadata schema"));
        }
        if self.version.is_empty()
            || self.version.len() > MAX_AGENT_VERSION_BYTES
            || !self
                .version
                .bytes()
                .all(|b| b.is_ascii() && !b.is_ascii_control())
        {
            return Err(invalid("Agent version must be 1..64 printable ASCII bytes"));
        }
        for value in [&self.os, &self.arch] {
            if !valid_identifier(value, MAX_AGENT_PLATFORM_BYTES) {
                return Err(invalid(
                    "Agent platform identifiers must be 1..32 lowercase ASCII identifier bytes",
                ));
            }
        }
        if self.capabilities.len() > MAX_AGENT_CAPABILITIES {
            return Err(invalid("Agent metadata exceeds 32 capabilities"));
        }
        for (index, capability) in self.capabilities.iter().enumerate() {
            if !valid_identifier(capability, MAX_CAPABILITY_BYTES) {
                return Err(invalid(
                    "Agent capabilities must be 1..64 lowercase ASCII identifier bytes",
                ));
            }
            if self.capabilities[..index].contains(capability) {
                return Err(invalid("Agent capabilities must be unique"));
            }
        }
        Ok(())
    }

    /// Returns only a support claim. Callers must separately enforce trust,
    /// operation-family prerequisites, and current connection/session state.
    pub fn supports(&self, name: &str) -> bool {
        self.capabilities
            .iter()
            .any(|capability| capability == name)
    }
}

fn valid_identifier(value: &str, max_bytes: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_bytes
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
}

/// Legacy protocol-4 peers retain basic editing; execution support requires
/// explicit metadata. An empty declared list never falls back to legacy support.
pub fn supports_capability(agent: Option<&AgentInfo>, name: &str) -> bool {
    match agent {
        Some(agent) => agent.supports(name),
        None => matches!(name, "list" | "read" | "write" | "search"),
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub id: u64,
    pub op: Operation,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Operation {
    Hello,
    List {
        path: String,
    },
    Read {
        path: String,
    },
    Write {
        path: String,
        text: String,
        expected_revision: Option<String>,
    },
    Search {
        query: String,
        limit: usize,
    },
    GitStatus,
    /// Explicit trusted read view. The program is on the workspace host.
    GitChanges {
        git_executable: String,
    },
    GitDiff {
        git_executable: String,
        path: String,
        kind: GitDiffKind,
    },
    LanguageStart {
        program: String,
        args: Vec<String>,
    },
    /// Explicit Java/JDT recipe; no shell, PATH lookup or caller-supplied argv.
    /// Like other execution operations, support does not grant workspace trust.
    LanguageStartJava {
        java_executable: String,
        distribution: String,
        data_directory: String,
    },
    LanguageStartJavaBegin {
        java_executable: String,
        distribution: String,
        data_directory: String,
    },
    /// Bounded offline leaf-POM import using explicit installed/cache locations.
    LanguageStartJavaMavenBegin {
        java_executable: String,
        distribution: String,
        data_directory: String,
        local_repository: String,
    },
    /// Read the fixed root POM's imported model in the current typed session.
    LanguageMavenModel,
    LanguageStartJavaPoll {
        startup_id: u64,
    },
    LanguageStartJavaCancel {
        startup_id: u64,
    },
    LanguageOpen {
        path: String,
        language_id: String,
        version: i32,
        text: String,
    },
    LanguageChange {
        path: String,
        version: i32,
        text: String,
    },
    LanguageClose {
        path: String,
    },
    LanguageQuery {
        path: String,
        line: u32,
        character: u32,
        kind: LanguageQueryKind,
    },
    /// Request formatting for exactly the synchronized document version.
    /// The version is checked by the workspace agent, not sent as an LSP field.
    LanguageFormat {
        path: String,
        version: i32,
        tab_size: u32,
        insert_spaces: bool,
    },
    /// Explicit Java-only validation notification for the current open version.
    /// A successful response acknowledges the write, never diagnostic completion.
    LanguageRefreshJavaDiagnostics {
        path: String,
        version: i32,
    },
    /// Preview imports for exactly the acknowledged Java document version.
    /// The agent owns the fixed command and its single document URI argument.
    LanguageOrganizeJavaImports {
        path: String,
        version: i32,
    },
    LanguageReferences {
        path: String,
        line: u32,
        character: u32,
        include_declaration: bool,
    },
    LanguageDocumentSymbols {
        path: String,
    },
    /// Standard workspace/symbol query; no document, command or resolve bridge.
    LanguageWorkspaceSymbols {
        query: String,
    },
    LanguageResolveUri {
        uri: String,
    },
    LanguageResolveCompletion {
        item: serde_json::Value,
    },
    LanguageEvents,
    LanguageStop,
    RunStart {
        program: String,
        args: Vec<String>,
        timeout_secs: u64,
    },
    RunPoll {
        task_id: u64,
    },
    RunCancel {
        task_id: u64,
    },
    Run {
        program: String,
        args: Vec<String>,
        timeout_secs: u64,
    },
}

impl Operation {
    /// Protocol-4 capability names. The Java diagnostic refresh bridge has a
    /// separate capability from its operation discriminant. Hello is always
    /// available so support discovery cannot depend on its own result.
    pub fn capability_name(&self) -> Option<&'static str> {
        Some(match self {
            Self::Hello => return None,
            Self::List { .. } => "list",
            Self::Read { .. } => "read",
            Self::Write { .. } => "write",
            Self::Search { .. } => "search",
            Self::GitStatus => "git_status",
            Self::GitChanges { .. } => "git_changes",
            Self::GitDiff { .. } => "git_diff",
            Self::LanguageStart { .. } => "language_start",
            Self::LanguageStartJava { .. } => "language_start_java",
            Self::LanguageStartJavaBegin { .. } => "language_start_java_begin",
            Self::LanguageStartJavaMavenBegin { .. } => "language_start_java_maven_begin",
            Self::LanguageMavenModel => "language_maven_model",
            Self::LanguageStartJavaPoll { .. } => "language_start_java_poll",
            Self::LanguageStartJavaCancel { .. } => "language_start_java_cancel",
            Self::LanguageOpen { .. } => "language_open",
            Self::LanguageChange { .. } => "language_change",
            Self::LanguageClose { .. } => "language_close",
            Self::LanguageQuery { .. } => "language_query",
            Self::LanguageFormat { .. } => "language_format",
            Self::LanguageRefreshJavaDiagnostics { .. } => "java_diagnostics_refresh",
            Self::LanguageOrganizeJavaImports { .. } => "language_organize_java_imports",
            Self::LanguageReferences { .. } => "language_references",
            Self::LanguageDocumentSymbols { .. } => "language_document_symbols",
            Self::LanguageWorkspaceSymbols { .. } => "language_workspace_symbols",
            Self::LanguageResolveUri { .. } => "language_resolve_uri",
            Self::LanguageResolveCompletion { .. } => "language_resolve_completion",
            Self::LanguageEvents => "language_events",
            Self::LanguageStop => "language_stop",
            Self::RunStart { .. } => "run_start",
            Self::RunPoll { .. } => "run_poll",
            Self::RunCancel { .. } => "run_cancel",
            Self::Run { .. } => "run",
        })
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LanguageQueryKind {
    Completion,
    Definition,
    Hover,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub id: u64,
    pub result: Result<Payload, RemoteError>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Payload {
    Hello {
        protocol: u32,
        root: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent: Option<AgentInfo>,
    },
    Entries {
        entries: Vec<Entry>,
    },
    File {
        path: String,
        text: String,
        revision: String,
    },
    Written {
        revision: String,
    },
    Matches {
        matches: Vec<SearchMatch>,
        truncated: bool,
    },
    GitStatus {
        text: String,
    },
    GitChanges {
        entries: Vec<GitChange>,
    },
    GitDiff {
        path: String,
        kind: GitDiffKind,
        text: String,
    },
    Language {
        value: serde_json::Value,
    },
    RunTask {
        snapshot: serde_json::Value,
    },
    Run {
        stdout: String,
        stderr: String,
        exit_code: Option<i32>,
        timed_out: bool,
        truncated: bool,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitDiffKind {
    Staged,
    Unstaged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitChangeKind {
    File,
    Untracked,
    Conflict,
    Unsupported,
}

/// Exact repository-relative path, separate from any escaped display label.
/// Capability flags describe a bounded read action, never execution permission.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitChange {
    pub path: String,
    pub index: char,
    pub worktree: char,
    pub kind: GitChangeKind,
    pub can_diff_staged: bool,
    pub can_diff_unstaged: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchMatch {
    pub path: String,
    pub line: usize,
    pub text: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, thiserror::Error)]
#[error("{code}: {message}")]
pub struct RemoteError {
    pub code: String,
    pub message: String,
}
impl RemoteError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}
/// Fail closed on oversized frames, including missing newline.
pub fn read_frame<R: BufRead, T: serde::de::DeserializeOwned>(
    reader: &mut R,
) -> io::Result<Option<T>> {
    let mut bytes = Vec::new();
    loop {
        let buf = reader.fill_buf()?;
        if buf.is_empty() {
            if bytes.is_empty() {
                return Ok(None);
            }
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "incomplete frame",
            ));
        }
        let end = buf
            .iter()
            .position(|b| *b == b'\n')
            .map(|x| x + 1)
            .unwrap_or(buf.len());
        if bytes.len() + end > MAX_FRAME_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "frame too large",
            ));
        }
        bytes.extend_from_slice(&buf[..end]);
        reader.consume(end);
        if bytes.last() == Some(&b'\n') {
            return serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e));
        }
    }
}
pub fn write_frame<W: Write, T: Serialize>(writer: &mut W, value: &T) -> io::Result<()> {
    let bytes = serde_json::to_vec(value).map_err(io::Error::other)?;
    if bytes.len() + 1 > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame too large",
        ));
    }
    writer.write_all(&bytes)?;
    writer.write_all(b"\n")?;
    writer.flush()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn typed_git_requests_preserve_literal_paths_and_require_new_capabilities() {
        let path = "src/-雪 [literal]\nfile.txt";
        let op = Operation::GitDiff {
            git_executable: "C:\\Program Files\\Git\\cmd\\git.exe".into(),
            path: path.into(),
            kind: GitDiffKind::Staged,
        };
        let value = serde_json::to_value(&op).unwrap();
        assert_eq!(value["type"], "git_diff");
        assert_eq!(value["kind"], "staged");
        let decoded: Operation = serde_json::from_value(value).unwrap();
        assert!(
            matches!(decoded, Operation::GitDiff { path: actual, kind: GitDiffKind::Staged, .. } if actual == path)
        );
        assert_eq!(op.capability_name(), Some("git_diff"));
        assert_eq!(
            Operation::GitChanges {
                git_executable: "/usr/bin/git".into()
            }
            .capability_name(),
            Some("git_changes")
        );
        assert!(!supports_capability(None, "git_changes"));
        assert!(!supports_capability(None, "git_diff"));
        for kind in ["commit", "reset", "HEAD~1", "--cached", ""] {
            assert!(serde_json::from_value::<Operation>(serde_json::json!({
                "type": "git_diff", "git_executable": "/usr/bin/git",
                "path": "file.txt", "kind": kind
            }))
            .is_err());
        }
    }

    #[test]
    fn protocol_round_trip() {
        let req = Request {
            id: 3,
            op: Operation::Read {
                path: "src/你好.rs".into(),
            },
        };
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &req).unwrap();
        let decoded: Request = read_frame(&mut &bytes[..]).unwrap().unwrap();
        assert_eq!(decoded.id, 3);
    }
    #[test]
    fn language_navigation_requests_preserve_exact_wire_fields() {
        for op in [
            serde_json::json!({"type":"language_format","path":"src/你好.java","version":7,"tab_size":4,"insert_spaces":true}),
            serde_json::json!({"type":"language_references","path":"src/你好.java","line":2,"character":3,"include_declaration":false}),
            serde_json::json!({"type":"language_document_symbols","path":"src/你好.java"}),
            serde_json::json!({"type":"language_workspace_symbols","query":" 你好*Type "}),
        ] {
            let request = serde_json::json!({"id":42,"op":op});
            let decoded: Request = serde_json::from_value(request.clone()).unwrap();
            let mut bytes = Vec::new();
            write_frame(&mut bytes, &decoded).unwrap();
            let round_trip: serde_json::Value = read_frame(&mut &bytes[..]).unwrap().unwrap();
            assert_eq!(round_trip, request);
        }
        assert_eq!(PROTOCOL_VERSION, 4);
    }
    #[test]
    fn formatting_requires_version_and_typed_options() {
        for op in [
            serde_json::json!({"type":"language_format","path":"a.java","tab_size":4,"insert_spaces":true}),
            serde_json::json!({"type":"language_format","path":"a.java","version":1,"tab_size":-1,"insert_spaces":true}),
            serde_json::json!({"type":"language_format","path":"a.java","version":1,"tab_size":4,"insert_spaces":"true"}),
            serde_json::json!({"type":"language_references","path":"a.java","line":-1,"character":0,"include_declaration":true}),
            serde_json::json!({"type":"language_references","path":"a.java","line":0,"character":0}),
        ] {
            assert!(serde_json::from_value::<Operation>(op).is_err());
        }
    }
    #[test]
    fn java_diagnostics_refresh_is_a_typed_additive_protocol_four_operation() {
        let wire = serde_json::json!({
            "type":"language_refresh_java_diagnostics", "path":"src/你好 #.java", "version":7
        });
        let operation: Operation = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(
            operation.capability_name(),
            Some("java_diagnostics_refresh")
        );
        assert_eq!(serde_json::to_value(operation).unwrap(), wire);
        assert!(!supports_capability(None, "java_diagnostics_refresh"));
        assert!(!JAVA_LANGUAGE_SESSION_CAPABILITIES.contains(&"java_diagnostics_refresh"));
        assert_eq!(PROTOCOL_VERSION, 4);
        for invalid in [
            serde_json::json!({"type":"language_refresh_java_diagnostics","path":"a.java"}),
            serde_json::json!({"type":"language_refresh_java_diagnostics","path":"a.java","version":"7"}),
            serde_json::json!({"type":"language_refresh_java_diagnostics","path":"a.java","version":2147483648_i64}),
            serde_json::json!({"type":"language_notify","method":"java/validateDocument","params":{}}),
        ] {
            assert!(serde_json::from_value::<Operation>(invalid).is_err());
        }
    }
    #[test]
    fn java_imports_is_a_typed_optional_protocol_four_operation() {
        let wire = serde_json::json!({
            "type":"language_organize_java_imports", "path":"src/你好 #.java", "version":7
        });
        let operation: Operation = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(
            operation.capability_name(),
            Some("language_organize_java_imports")
        );
        assert_eq!(serde_json::to_value(operation).unwrap(), wire);
        assert!(!supports_capability(None, "language_organize_java_imports"));
        assert!(!JAVA_LANGUAGE_SESSION_CAPABILITIES.contains(&"language_organize_java_imports"));
        assert_eq!(PROTOCOL_VERSION, 4);
        for invalid in [
            serde_json::json!({"type":"language_organize_java_imports","path":"a.java"}),
            serde_json::json!({"type":"language_organize_java_imports","path":"a.java","version":"7"}),
            serde_json::json!({"type":"language_organize_java_imports","path":"a.java","version":2147483648_i64}),
            serde_json::json!({"type":"language_execute_command","command":"java.edit.organizeImports","arguments":[]}),
        ] {
            assert!(serde_json::from_value::<Operation>(invalid).is_err());
        }
    }
    #[test]
    fn language_payload_preserves_raw_feature_results() {
        for value in [
            serde_json::Value::Null,
            serde_json::json!([]),
            serde_json::json!([{"name":"类","children":[]}]),
        ] {
            let response = Response {
                id: 42,
                result: Ok(Payload::Language {
                    value: value.clone(),
                }),
            };
            let mut bytes = Vec::new();
            write_frame(&mut bytes, &response).unwrap();
            let decoded: Response = read_frame(&mut &bytes[..]).unwrap().unwrap();
            assert!(
                matches!(decoded.result, Ok(Payload::Language { value: actual }) if actual == value)
            );
        }
    }
    #[test]
    fn rejects_truncated_frame() {
        assert!(read_frame::<_, Request>(&mut &b"{\"id\":1}"[..]).is_err());
    }
    #[test]
    fn rejects_oversized_frame() {
        let data = vec![b'x'; MAX_FRAME_BYTES + 1];
        assert!(read_frame::<_, Request>(&mut &data[..]).is_err());
    }
}
