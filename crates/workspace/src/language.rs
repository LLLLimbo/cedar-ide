//! One explicitly launched language server per workspace, always on the agent side.
use super::{error, validate_command, Workspace};
use cedar_language::{ClientOptions, LspClient, LspEvent, Position, ProcessConfig};
use cedar_protocol::{LanguageQueryKind, Operation, Payload, RemoteError, MAX_FILE_BYTES};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::Duration;

pub(super) struct LanguageSession {
    client: LspClient,
    opened: HashMap<String, usize>,
}
impl std::fmt::Debug for LanguageSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LanguageSession")
            .field("open_documents", &self.opened.len())
            .finish()
    }
}
fn lsp_error(e: cedar_language::Error) -> RemoteError {
    error("language_error", e.to_string())
}
impl Workspace {
    fn language_uri(&self, path: &str) -> Result<String, RemoteError> {
        let full = self.resolve(path, true)?;
        if full == self.root {
            return Err(error(
                "invalid_path",
                "Language document requires a file path",
            ));
        }
        url::Url::from_file_path(full)
            .map(String::from)
            .map_err(|_| error("invalid_path", "Cannot represent path as a file URI"))
    }
    pub(super) fn handle_language(&mut self, op: Operation) -> Result<Payload, RemoteError> {
        if !self.allow_run {
            return Err(error("run_disabled","Language servers execute code. Enable trusted tool execution before starting a server."));
        }
        match op {
            Operation::LanguageStart { program, args } => {
                if cfg!(windows) {
                    return Err(error("unsupported_platform", "Local Windows tool processes are disabled until Job Object cleanup is implemented; use a Linux SSH workspace."));
                }
                validate_command(&program, &args, 10)?;
                if self.language.is_some() {
                    return Err(error(
                        "language_running",
                        "Stop the current language server before starting another",
                    ));
                }
                let mut options = ClientOptions::default();
                options.frame_limits.max_content_bytes = MAX_FILE_BYTES;
                options.event_capacity = 32;
                options.outbound_capacity = 8;
                options.max_pending_requests = 8;
                let mut config = ProcessConfig::new(program);
                config.args = args.into_iter().map(Into::into).collect();
                config.working_directory = Some(self.root.clone());
                let client = LspClient::spawn(config, options).map_err(lsp_error)?;
                let uri = url::Url::from_directory_path(&self.root)
                    .map_err(|_| error("invalid_path", "Cannot create root URI"))?;
                let result = client
                    .initialize_with_timeout(
                        Some(uri.as_str()),
                        Value::Null,
                        Duration::from_secs(60),
                    )
                    .map_err(lsp_error)?;
                self.language = Some(LanguageSession {
                    client,
                    opened: HashMap::new(),
                });
                Ok(Payload::Language {
                    value: json!({"started":true,"initialize":result,"root_uri":uri.as_str()}),
                })
            }
            Operation::LanguageOpen {
                path,
                language_id,
                version,
                text,
            } => {
                self.check_language_text(&text)?;
                if language_id.is_empty() || language_id.len() > 128 {
                    return Err(error(
                        "invalid_language",
                        "Language ID must contain 1..128 bytes",
                    ));
                }
                let uri = self.language_uri(&path)?;
                let session = self.language.as_mut().ok_or_else(|| {
                    error("language_not_running", "Start a language server first")
                })?;
                if session.opened.len() >= 32 && !session.opened.contains_key(&uri) {
                    return Err(error(
                        "language_limit",
                        "At most 32 language documents may be open",
                    ));
                }
                session
                    .client
                    .did_open(&uri, &language_id, version, &text)
                    .map_err(lsp_error)?;
                session.opened.insert(uri.clone(), text.len());
                Ok(Payload::Language {
                    value: json!({"opened":uri,"version":version}),
                })
            }
            Operation::LanguageChange {
                path,
                version,
                text,
            } => {
                self.check_language_text(&text)?;
                let uri = self.language_uri(&path)?;
                let session = self.language.as_mut().ok_or_else(|| {
                    error("language_not_running", "Start a language server first")
                })?;
                session
                    .client
                    .did_change(&uri, version, &text)
                    .map_err(lsp_error)?;
                session.opened.insert(uri.clone(), text.len());
                Ok(Payload::Language {
                    value: json!({"changed":uri,"version":version}),
                })
            }
            Operation::LanguageClose { path } => {
                let uri = self.language_uri(&path)?;
                let session = self.language.as_mut().ok_or_else(|| {
                    error("language_not_running", "Start a language server first")
                })?;
                session.client.did_close(&uri).map_err(lsp_error)?;
                session.opened.remove(&uri);
                Ok(Payload::Language {
                    value: json!({"closed":uri}),
                })
            }
            Operation::LanguageQuery {
                path,
                line,
                character,
                kind,
            } => {
                let uri = self.language_uri(&path)?;
                let session = self.language.as_ref().ok_or_else(|| {
                    error("language_not_running", "Start a language server first")
                })?;
                if !session.opened.contains_key(&uri) {
                    return Err(error(
                        "language_document_closed",
                        "Synchronize this document before querying",
                    ));
                }
                let position = Position { line, character };
                let value = match kind {
                    LanguageQueryKind::Completion => session.client.completion(&uri, position),
                    LanguageQueryKind::Definition => session.client.definition(&uri, position),
                    LanguageQueryKind::Hover => session.client.hover(&uri, position),
                }
                .map_err(lsp_error)?;
                Ok(Payload::Language { value })
            }
            Operation::LanguageEvents => {
                let session = self.language.as_ref().ok_or_else(|| {
                    error("language_not_running", "Start a language server first")
                })?;
                let mut events = Vec::new();
                let mut bytes = 0;
                let mut truncated = false;
                for _ in 0..32 {
                    let Some(event) = session
                        .client
                        .next_event(Duration::ZERO)
                        .map_err(lsp_error)?
                    else {
                        break;
                    };
                    let value = match event {
                        LspEvent::Diagnostics(d) => json!({"type":"diagnostics","value":d}),
                        LspEvent::Notification { method, params } => {
                            json!({"type":"notification","method":method,"params":params})
                        }
                        LspEvent::UnsupportedServerRequest { method, id } => {
                            json!({"type":"unsupported_server_request","method":method,"id":id})
                        }
                        LspEvent::Lagged { dropped } => json!({"type":"lagged","dropped":dropped}),
                        LspEvent::Closed(e) => json!({"type":"closed","message":e.to_string()}),
                    };
                    bytes += value.to_string().len();
                    if bytes > MAX_FILE_BYTES {
                        truncated = true;
                        break;
                    }
                    events.push(value);
                }
                Ok(Payload::Language {
                    value: json!({"events":events,"truncated":truncated}),
                })
            }
            Operation::LanguageStop => {
                if let Some(session) = self.language.take() {
                    session.client.shutdown().map_err(lsp_error)?;
                }
                Ok(Payload::Language {
                    value: json!({"stopped":true}),
                })
            }
            _ => Err(error("invalid_operation", "Not a language operation")),
        }
    }
    fn check_language_text(&self, text: &str) -> Result<(), RemoteError> {
        if text.len() > MAX_FILE_BYTES {
            Err(error(
                "file_too_large",
                "Language documents are limited to 1 MiB",
            ))
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn language_requires_explicit_trust() {
        let dir = tempfile::tempdir().unwrap();
        let mut ws = Workspace::open(dir.path()).unwrap();
        let e = ws
            .handle(Operation::LanguageStart {
                program: "nonexistent".into(),
                args: vec![],
            })
            .unwrap_err();
        assert_eq!(e.code, "run_disabled");
    }
    #[test]
    fn language_paths_confined_before_process_access() {
        let dir = tempfile::tempdir().unwrap();
        let mut ws = Workspace::open(dir.path()).unwrap();
        ws.set_allow_run(true);
        let e = ws
            .handle(Operation::LanguageOpen {
                path: "../outside".into(),
                language_id: "java".into(),
                version: 1,
                text: String::new(),
            })
            .unwrap_err();
        assert_eq!(e.code, "invalid_path");
    }
    #[test]
    fn file_uri_escapes_spaces_hash_and_unicode() {
        let dir = tempfile::tempdir().unwrap();
        let ws = Workspace::open(dir.path()).unwrap();
        let uri = ws.language_uri("你好 #.java").unwrap();
        assert!(uri.starts_with("file:///"));
        assert!(uri.contains("%20%23.java"));
        assert!(!uri.contains("你好"));
    }
}
