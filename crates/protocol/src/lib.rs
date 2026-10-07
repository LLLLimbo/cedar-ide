//! Bounded newline-delimited JSON protocol between native UI and workspace agent.
use serde::{Deserialize, Serialize};
use std::io::{self, BufRead, Write};
pub const PROTOCOL_VERSION: u32 = 4;
pub const MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_FILE_BYTES: usize = 1024 * 1024;
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
    LanguageStart {
        program: String,
        args: Vec<String>,
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
    LanguageReferences {
        path: String,
        line: u32,
        character: u32,
        include_declaration: bool,
    },
    LanguageDocumentSymbols {
        path: String,
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
