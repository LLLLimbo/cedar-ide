//! One explicitly launched language server per workspace, always on the agent side.
use super::{error, validate_command, Workspace};
use cedar_language::{ClientOptions, LspClient, LspEvent, Position, ProcessConfig};
use cedar_protocol::{LanguageQueryKind, Operation, Payload, RemoteError, MAX_FILE_BYTES};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::Duration;

pub(super) const fn platform_supported() -> bool {
    // Match this service's existing startup guard, independently of the more
    // restrictive Linux/macOS command-task and Git implementations.
    !cfg!(windows)
}

pub(super) const fn java_platform_supported(backend: cedar_tasks::BackendMode) -> bool {
    cfg!(windows) && matches!(backend, cedar_tasks::BackendMode::IsolatedAgent)
}

pub(super) struct LanguageSession {
    client: LspClient,
    production_java: bool,
    opened: HashMap<String, OpenLanguageDocument>,
    #[cfg(feature = "windows-language-validation")]
    java_validation: Option<super::java_validation::JavaValidationSession>,
}

#[derive(Debug, Clone, Copy)]
struct OpenLanguageDocument {
    version: i32,
    bytes: usize,
}

impl OpenLanguageDocument {
    fn require_version(&self, expected: i32) -> Result<(), RemoteError> {
        if self.version != expected {
            return Err(error(
                "language_stale_version",
                "Document changed since the formatting request; synchronize and try again",
            ));
        }
        Ok(())
    }
}

impl LanguageSession {
    fn open_document(&self, uri: &str) -> Result<&OpenLanguageDocument, RemoteError> {
        self.opened.get(uri).ok_or_else(|| {
            error(
                "language_document_closed",
                "Synchronize this document before querying",
            )
        })
    }
}
impl std::fmt::Debug for LanguageSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LanguageSession")
            .field("open_documents", &self.opened.len())
            .field(
                "open_document_bytes",
                &self
                    .opened
                    .values()
                    .map(|document| document.bytes)
                    .sum::<usize>(),
            )
            .finish()
    }
}
fn lsp_error(e: cedar_language::Error) -> RemoteError {
    error("language_error", e.to_string())
}
fn stop_production_java(client: LspClient) -> Result<Payload, RemoteError> {
    // Do not run a semantic/indexing query on user Stop. The typed transport
    // outcome distinguishes a natural exit from joined forced cleanup.
    let (_, outcome) = client.shutdown_with_outcome();
    drop(client);
    java_stop_payload(outcome)
}

fn java_stop_payload(outcome: cedar_language::ShutdownOutcome) -> Result<Payload, RemoteError> {
    use cedar_language::{WindowsCleanupStatus, WindowsRootExit, WindowsShutdownReason};
    let Some(windows) = outcome.windows else {
        return Err(error(
            "language_cleanup_unverified",
            "Java session closed; owned process cleanup could not be verified",
        ));
    };
    if windows.cleanup == WindowsCleanupStatus::Unverified {
        return Err(error(
            "language_cleanup_unverified",
            "Java session closed; owned process cleanup could not be verified",
        ));
    }
    let root_exit_code = match windows.root_exit {
        WindowsRootExit::BeforeTermination(code) | WindowsRootExit::AfterTermination(code) => code,
        WindowsRootExit::Unobserved => {
            return Err(error(
                "language_cleanup_unverified",
                "Java session closed; root process termination could not be verified",
            ))
        }
    };
    let status = if outcome.is_graceful() {
        "graceful"
    } else if windows.cleanup == WindowsCleanupStatus::JoinedWithErrors
        || windows.errors != cedar_language::WindowsCleanupErrors::default()
        || windows.transport_failure_observed
    {
        "error"
    } else if matches!(windows.root_exit, WindowsRootExit::AfterTermination(_)) {
        "forced"
    } else {
        "error"
    };
    let reason = match windows.reason {
        WindowsShutdownReason::RootExited => "root_exited",
        WindowsShutdownReason::GraceExpired => "grace_expired",
        WindowsShutdownReason::Aborted => "aborted",
        WindowsShutdownReason::TransportFailure => "transport_failure",
        WindowsShutdownReason::WorkerPanicked => "worker_panicked",
    };
    Ok(Payload::Language {
        value: json!({"stopped":true,"shutdown":{
            "status":status,"reason":reason,"root_exit_code":root_exit_code,"cleanup_joined":true,
            "shutdown_response_received":outcome.shutdown_response_received,
            "exit_frame_completed":outcome.exit_frame_completed,
        }}),
    })
}

impl Workspace {
    fn language_platform_supported(&self) -> bool {
        #[cfg(feature = "windows-language-validation")]
        if cfg!(windows)
            && self.windows_language_validation
            && self.backend_mode == cedar_tasks::BackendMode::IsolatedAgent
        {
            return true;
        }
        platform_supported()
    }

    fn language_uri(&self, path: &str) -> Result<String, RemoteError> {
        let full = self.resolve(path, true)?;
        if full == self.root || full.exists() && !full.is_file() {
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
                if !self.language_platform_supported() {
                    return Err(error("unsupported_platform", "Generic language startup is unavailable on Windows; use the Java/JDT start operation in an isolated agent."));
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
                let initialization_options = Value::Null;
                #[cfg(feature = "windows-language-validation")]
                let initialization_options = match &self.windows_java_validation {
                    Some(profile) => profile.configure(&mut config, &mut options)?,
                    None => initialization_options,
                };
                self.start_language_session(config, options, initialization_options, false)
            }
            Operation::LanguageStartJava {
                java_executable,
                distribution,
                data_directory,
            } => {
                if !java_platform_supported(self.backend_mode) {
                    return Err(error(
                        "unsupported_platform",
                        "Java startup requires an isolated Windows agent",
                    ));
                }
                if self.language.is_some() {
                    return Err(error(
                        "language_running",
                        "Stop the current language server before starting another",
                    ));
                }
                let launch = super::java_launch::production(
                    &self.root,
                    &java_executable,
                    &distribution,
                    &data_directory,
                )?;
                self.start_language_session(
                    launch.config,
                    launch.options,
                    launch.initialization_options,
                    true,
                )
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
                session.opened.insert(
                    uri.clone(),
                    OpenLanguageDocument {
                        version,
                        bytes: text.len(),
                    },
                );
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
                session.opened.insert(
                    uri.clone(),
                    OpenLanguageDocument {
                        version,
                        bytes: text.len(),
                    },
                );
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
                session.open_document(&uri)?;
                let position = Position { line, character };
                let value = match kind {
                    LanguageQueryKind::Completion => session.client.completion(&uri, position),
                    LanguageQueryKind::Definition => session.client.definition(&uri, position),
                    LanguageQueryKind::Hover => session.client.hover(&uri, position),
                }
                .map_err(lsp_error)?;
                Ok(Payload::Language { value })
            }
            Operation::LanguageFormat {
                path,
                version,
                tab_size,
                insert_spaces,
            } => {
                let uri = self.language_uri(&path)?;
                let session = self.language.as_ref().ok_or_else(|| {
                    error("language_not_running", "Start a language server first")
                })?;
                // LSP formatting has no version field. Reject both older and
                // unsynchronized future drafts before anything reaches the peer.
                session.open_document(&uri)?.require_version(version)?;
                let value = session
                    .client
                    .formatting(&uri, tab_size, insert_spaces)
                    .map_err(lsp_error)?;
                Ok(Payload::Language { value })
            }
            Operation::LanguageReferences {
                path,
                line,
                character,
                include_declaration,
            } => {
                let uri = self.language_uri(&path)?;
                let session = self.language.as_ref().ok_or_else(|| {
                    error("language_not_running", "Start a language server first")
                })?;
                session.open_document(&uri)?;
                let value = session
                    .client
                    .references(&uri, Position { line, character }, include_declaration)
                    .map_err(lsp_error)?;
                Ok(Payload::Language { value })
            }
            Operation::LanguageDocumentSymbols { path } => {
                let uri = self.language_uri(&path)?;
                let session = self.language.as_ref().ok_or_else(|| {
                    error("language_not_running", "Start a language server first")
                })?;
                session.open_document(&uri)?;
                let value = session.client.document_symbols(&uri).map_err(lsp_error)?;
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
                    if session.production_java {
                        return stop_production_java(session.client);
                    }
                    #[cfg(feature = "windows-language-validation")]
                    if let Some(validation) = session.java_validation {
                        self.windows_java_validation
                            .as_mut()
                            .expect("validation session has a profile")
                            .finish(session.client, validation, None)?;
                    } else {
                        session.client.shutdown().map_err(lsp_error)?;
                    }
                    #[cfg(not(feature = "windows-language-validation"))]
                    session.client.shutdown().map_err(lsp_error)?;
                }
                Ok(Payload::Language {
                    value: json!({"stopped":true}),
                })
            }
            _ => Err(error("invalid_operation", "Not a language operation")),
        }
    }
    fn start_language_session(
        &mut self,
        config: ProcessConfig,
        options: ClientOptions,
        initialization_options: Value,
        production_java: bool,
    ) -> Result<Payload, RemoteError> {
        let uri = url::Url::from_directory_path(&self.root)
            .map_err(|_| error("invalid_path", "Cannot create root URI"))?;
        let client = LspClient::spawn(config, options).map_err(lsp_error)?;
        #[cfg(feature = "windows-language-validation")]
        let java_validation = match self.windows_java_validation.as_mut() {
            Some(profile) if !production_java => match profile.begin(&client) {
                Ok(session) => Some(session),
                Err(error) => {
                    let _ = client.shutdown();
                    drop(client);
                    return Err(error);
                }
            },
            _ => None,
        };
        let result = client
            .initialize_with_timeout(
                Some(uri.as_str()),
                initialization_options,
                Duration::from_secs(60),
            )
            .map_err(lsp_error);
        #[cfg(feature = "windows-language-validation")]
        let java_validation = if let Some(mut validation) = java_validation {
            let initialized = match &result {
                Ok(value) => validation.initialized(value),
                Err(error) => Err(error.clone()),
            };
            if let Err(error) = initialized {
                self.windows_java_validation
                    .as_mut()
                    .expect("validation session has a profile")
                    .finish(client, validation, Some(error))?;
                unreachable!("failed initialization cannot pass Java validation");
            }
            Some(validation)
        } else {
            None
        };
        let result = result?;
        let process_id = client.process_id();
        self.language = Some(LanguageSession {
            client,
            production_java,
            opened: HashMap::new(),
            #[cfg(feature = "windows-language-validation")]
            java_validation,
        });
        let mut value = json!({"started":true,"initialize":result,"root_uri":uri.as_str()});
        if production_java {
            value["process_id"] = json!(process_id);
        }
        Ok(Payload::Language { value })
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
    fn navigation_operations(path: &str) -> [Operation; 3] {
        [
            Operation::LanguageFormat {
                path: path.into(),
                version: 1,
                tab_size: 4,
                insert_spaces: true,
            },
            Operation::LanguageReferences {
                path: path.into(),
                line: 0,
                character: 0,
                include_declaration: true,
            },
            Operation::LanguageDocumentSymbols { path: path.into() },
        ]
    }
    #[test]
    fn new_language_operations_require_execution_trust_and_running_session() {
        let dir = tempfile::tempdir().unwrap();
        let mut ws = Workspace::open(dir.path()).unwrap();
        for operation in navigation_operations("Hello.java") {
            assert_eq!(ws.handle(operation).unwrap_err().code, "run_disabled");
        }
        ws.set_allow_run(true);
        for operation in navigation_operations("Hello.java") {
            assert_eq!(
                ws.handle(operation).unwrap_err().code,
                "language_not_running"
            );
        }
    }
    #[test]
    fn new_language_paths_are_confined_before_accessing_the_server() {
        let dir = tempfile::tempdir().unwrap();
        let mut ws = Workspace::open(dir.path()).unwrap();
        ws.set_allow_run(true);
        std::fs::create_dir(dir.path().join("directory")).unwrap();
        for path in [
            "../outside.java",
            "/absolute.java",
            "",
            ".",
            "directory",
            "a\\b.java",
            "nul\0.java",
        ] {
            for operation in navigation_operations(path) {
                assert_eq!(
                    ws.handle(operation).unwrap_err().code,
                    "invalid_path",
                    "{path:?}"
                );
            }
        }
    }
    #[cfg(unix)]
    #[test]
    fn new_language_queries_reject_symlinks_before_accessing_the_server() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Hello.java"), "class Hello {}").unwrap();
        std::os::unix::fs::symlink("Hello.java", dir.path().join("alias.java")).unwrap();
        let mut ws = Workspace::open(dir.path()).unwrap();
        ws.set_allow_run(true);
        for operation in navigation_operations("alias.java") {
            assert_eq!(ws.handle(operation).unwrap_err().code, "invalid_path");
        }
    }
    #[test]
    fn formatting_requires_exact_synced_version_including_signed_boundaries() {
        for version in [i32::MIN, -1, 0, 1, i32::MAX] {
            let document = OpenLanguageDocument { version, bytes: 42 };
            document.require_version(version).unwrap();
            for stale in [version.wrapping_sub(1), version.wrapping_add(1)] {
                assert_eq!(
                    document.require_version(stale).unwrap_err().code,
                    "language_stale_version"
                );
            }
        }
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

#[cfg(test)]
mod java_production_tests {
    use super::*;
    use cedar_language::{
        ShutdownOutcome, WindowsCleanupErrors, WindowsCleanupStatus, WindowsRootExit,
        WindowsShutdownOutcome, WindowsShutdownReason,
    };

    fn start_java() -> Operation {
        Operation::LanguageStartJava {
            java_executable: "must-not-be-inspected\0".into(),
            distribution: "must-not-be-inspected\0".into(),
            data_directory: "must-not-be-inspected\0".into(),
        }
    }

    #[test]
    fn java_start_checks_trust_and_owned_host_before_any_path_validation() {
        let root = tempfile::tempdir().unwrap();
        for backend in [
            cedar_tasks::BackendMode::InProcess,
            cedar_tasks::BackendMode::IsolatedAgent,
        ] {
            let mut workspace = Workspace::with_backend_mode(root.path(), backend).unwrap();
            assert_eq!(
                workspace.handle(start_java()).unwrap_err().code,
                "run_disabled"
            );
            workspace.set_allow_run(true);
            let error = workspace.handle(start_java()).unwrap_err();
            if java_platform_supported(backend) {
                assert_eq!(error.code, "invalid_java_launch");
            } else {
                assert_eq!(error.code, "unsupported_platform");
            }
            assert!(workspace.language.is_none());
            assert!(workspace.tasks.is_none());
        }
        assert!(std::fs::read_dir(root.path()).unwrap().next().is_none());
    }

    fn outcome(
        root_exit: WindowsRootExit,
        reason: WindowsShutdownReason,
        cleanup: WindowsCleanupStatus,
    ) -> ShutdownOutcome {
        ShutdownOutcome {
            shutdown_response_received: true,
            exit_frame_completed: true,
            windows: Some(WindowsShutdownOutcome {
                reason,
                root_exit,
                cleanup,
                errors: WindowsCleanupErrors::default(),
                transport_failure_observed: false,
            }),
        }
    }

    fn stop_status(outcome: ShutdownOutcome) -> Value {
        let Payload::Language { value } = java_stop_payload(outcome).unwrap() else {
            panic!("language response")
        };
        value
    }

    #[test]
    fn typed_java_stop_distinguishes_natural_forced_and_unverified_cleanup() {
        let natural = outcome(
            WindowsRootExit::BeforeTermination(0),
            WindowsShutdownReason::RootExited,
            WindowsCleanupStatus::Joined,
        );
        let value = stop_status(natural);
        assert_eq!(value["stopped"], true);
        assert_eq!(value["shutdown"]["status"], "graceful");
        assert_eq!(value["shutdown"]["root_exit_code"], 0);
        for code in [0, 259, 1067, u32::MAX] {
            let forced = outcome(
                WindowsRootExit::AfterTermination(code),
                WindowsShutdownReason::GraceExpired,
                WindowsCleanupStatus::Joined,
            );
            let value = stop_status(forced);
            assert_eq!(value["shutdown"]["status"], "forced");
            assert_eq!(value["shutdown"]["root_exit_code"], code);
            assert_eq!(value["shutdown"]["reason"], "grace_expired");
            assert_eq!(value["shutdown"].as_object().unwrap().len(), 6);
            assert!(value.to_string().len() < 512);
        }
        for result in [
            ShutdownOutcome::default(),
            outcome(
                WindowsRootExit::Unobserved,
                WindowsShutdownReason::Aborted,
                WindowsCleanupStatus::Joined,
            ),
            outcome(
                WindowsRootExit::BeforeTermination(0),
                WindowsShutdownReason::WorkerPanicked,
                WindowsCleanupStatus::Unverified,
            ),
        ] {
            let error = java_stop_payload(result).unwrap_err();
            assert_eq!(error.code, "language_cleanup_unverified");
            assert!(error.message.len() < 256);
        }
        for result in [
            outcome(
                WindowsRootExit::BeforeTermination(1),
                WindowsShutdownReason::RootExited,
                WindowsCleanupStatus::Joined,
            ),
            outcome(
                WindowsRootExit::BeforeTermination(0),
                WindowsShutdownReason::TransportFailure,
                WindowsCleanupStatus::Joined,
            ),
            outcome(
                WindowsRootExit::AfterTermination(1067),
                WindowsShutdownReason::GraceExpired,
                WindowsCleanupStatus::JoinedWithErrors,
            ),
            ShutdownOutcome {
                shutdown_response_received: false,
                ..natural
            },
            ShutdownOutcome {
                exit_frame_completed: false,
                ..natural
            },
        ] {
            assert_eq!(stop_status(result)["shutdown"]["status"], "error");
        }
    }

    #[test]
    fn sticky_transport_failure_prevents_graceful_or_forced_success_classification() {
        for (root, reason) in [
            (
                WindowsRootExit::BeforeTermination(0),
                WindowsShutdownReason::RootExited,
            ),
            (
                WindowsRootExit::AfterTermination(1067),
                WindowsShutdownReason::GraceExpired,
            ),
        ] {
            let mut result = outcome(root, reason, WindowsCleanupStatus::Joined);
            result.windows.as_mut().unwrap().transport_failure_observed = true;
            let value = stop_status(result);
            assert_eq!(value["shutdown"]["status"], "error");
            assert_eq!(value["stopped"], true);
            assert_eq!(value["shutdown"].as_object().unwrap().len(), 6);
            assert!(value.to_string().len() < 512);
        }
    }
}
