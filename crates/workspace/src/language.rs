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
            Operation::LanguageResolveUri { uri } => {
                if uri.len() > 16 * 1024 {
                    return Err(error("invalid_uri", "Language URI exceeds limit"));
                }
                let parsed = url::Url::parse(&uri)
                    .map_err(|_| error("invalid_uri", "Malformed language URI"))?;
                if parsed.scheme() != "file"
                    || parsed.fragment().is_some()
                    || parsed.query().is_some()
                    || parsed.host_str().is_some_and(|host| host != "localhost")
                {
                    return Err(error(
                        "unsupported_uri",
                        "Only plain local file URIs inside this workspace can be opened",
                    ));
                }
                let full = parsed.to_file_path().map_err(|_| {
                    error(
                        "unsupported_uri",
                        "Cannot convert language URI to a local path",
                    )
                })?;
                let normalized_root = url::Url::from_directory_path(&self.root)
                    .map_err(|_| error("invalid_path", "Cannot normalize workspace root URI"))?
                    .to_file_path()
                    .map_err(|_| error("invalid_path", "Cannot normalize workspace root path"))?;
                let relative = full
                    .strip_prefix(&normalized_root)
                    .map_err(|_| error("invalid_path", "Language URI is outside the workspace"))?;
                let raw_path = relative
                    .to_str()
                    .ok_or_else(|| error("invalid_path", "Language URI path is not UTF-8"))?;
                #[cfg(windows)]
                let path = raw_path.replace('\\', "/");
                #[cfg(not(windows))]
                let path = raw_path.to_owned();
                let safe = self.resolve(&path, false)?;
                if !safe.is_file() {
                    return Err(error(
                        "invalid_path",
                        "Language URI must refer to a regular workspace file",
                    ));
                }
                Ok(Payload::Language {
                    value: json!({"path": path}),
                })
            }
            Operation::LanguageResolveCompletion { item } => {
                if item.to_string().len() > MAX_FILE_BYTES {
                    return Err(error("language_limit", "Completion item exceeds limit"));
                }
                let session = self.language.as_ref().ok_or_else(|| {
                    error("language_not_running", "Start a language server first")
                })?;
                let value = session.client.resolve_completion(item).map_err(lsp_error)?;
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

#[cfg(test)]
mod navigation_tests {
    use super::*;
    #[test]
    fn resolves_only_regular_files_inside_workspace() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("space # λ.java"), "class A {}").unwrap();
        let mut ws = Workspace::open(dir.path()).unwrap();
        ws.set_allow_run(true);
        let uri = url::Url::from_file_path(ws.root.join("space # λ.java"))
            .unwrap()
            .to_string();
        let answer = ws.handle(Operation::LanguageResolveUri { uri }).unwrap();
        assert!(matches!(answer,Payload::Language{value} if value["path"]=="space # λ.java"));
        for uri in [
            "jdt://contents/java/lang/String.class",
            "https://example.org/evil",
            "file://external-host/tmp/file",
            "file:///etc/passwd",
            "file:///tmp/file?execute=yes",
        ] {
            assert!(ws
                .handle(Operation::LanguageResolveUri { uri: uri.into() })
                .is_err());
        }
        let uri = url::Url::from_directory_path(&ws.root).unwrap().to_string();
        assert!(ws.handle(Operation::LanguageResolveUri { uri }).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn navigation_rejects_symlinks_even_inside_workspace() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("real.java"), "x").unwrap();
        std::os::unix::fs::symlink("real.java", dir.path().join("link.java")).unwrap();
        let mut ws = Workspace::open(dir.path()).unwrap();
        ws.set_allow_run(true);
        let uri = url::Url::from_file_path(ws.root.join("link.java"))
            .unwrap()
            .to_string();
        assert_eq!(
            ws.handle(Operation::LanguageResolveUri { uri })
                .unwrap_err()
                .code,
            "invalid_path"
        );
    }
}

#[cfg(all(test, unix))]
mod unix_uri_edge_tests {
    use super::*;
    #[test]
    fn literal_backslash_does_not_alias_a_different_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("a")).unwrap();
        std::fs::write(dir.path().join("a/b.java"), "normal").unwrap();
        std::fs::write(dir.path().join("a\\b.java"), "different").unwrap();
        let mut ws = Workspace::open(dir.path()).unwrap();
        ws.set_allow_run(true);
        let uri = url::Url::from_file_path(ws.root.join("a\\b.java"))
            .unwrap()
            .to_string();
        assert_eq!(
            ws.handle(Operation::LanguageResolveUri { uri })
                .unwrap_err()
                .code,
            "invalid_path"
        );
    }
}
