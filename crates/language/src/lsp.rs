use crate::{ClientOptions, Error, ProcessConfig, RpcEvent, ShutdownOutcome, StdioRpc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

/// Zero-based UTF-16 code-unit coordinates (not UTF-8 bytes or Rust chars).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

impl Position {
    /// Position immediately after the text; supports LF, CRLF and bare CR lines.
    pub fn end_of(text: &str) -> Self {
        let mut position = Self::default();
        let mut chars = text.chars().peekable();
        while let Some(ch) = chars.next() {
            match ch {
                '\r' => {
                    if chars.peek() == Some(&'\n') {
                        chars.next();
                    }
                    position.line = position.line.saturating_add(1);
                    position.character = 0;
                }
                '\n' => {
                    position.line = position.line.saturating_add(1);
                    position.character = 0;
                }
                _ => position.character = position.character.saturating_add(ch.len_utf16() as u32),
            }
        }
        position
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub range: Range,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    pub message: String,
    /// Preserve relatedInformation, tags, data and future fields without
    /// interpreting or executing content from a language server.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishDiagnostics {
    pub uri: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<i32>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone)]
pub enum LspEvent {
    Diagnostics(PublishDiagnostics),
    Notification { method: String, params: Value },
    UnsupportedServerRequest { method: String, id: Value },
    Lagged { dropped: usize },
    Closed(Error),
}

#[derive(Debug, Clone, Copy)]
struct SyncCapabilities {
    open_close: bool,
    change: u64,
}

#[derive(Debug, Clone)]
enum Lifecycle {
    Created,
    Initializing,
    Ready {
        capabilities: Arc<Value>,
        sync: SyncCapabilities,
    },
    ShuttingDown,
    Stopped {
        result: Result<(), Error>,
        outcome: ShutdownOutcome,
    },
}

#[derive(Debug, Clone, Copy)]
struct OpenDocument {
    version: i32,
    end: Position,
}

/// Cloneable cancellation signal below the lifecycle gate and outbound queue.
/// It retains no process or worker ownership. Signaling never joins or reports
/// cleanup; the owning worker must still call [`LspClient::abort_and_join`].
#[derive(Clone)]
pub struct LspAbortHandle(crate::transport::AbortHandle);

impl LspAbortHandle {
    pub fn signal(&self) {
        self.0.signal();
    }
}

/// LSP 3.17 subset with lifecycle, capability negotiation and document tracking.
/// Wrap in `Arc` to issue concurrent feature requests while a separate consumer
/// polls [`Self::next_event`]. Do not hold the GUI thread during blocking calls.
pub struct LspClient {
    rpc: StdioRpc,
    gate: RwLock<()>,
    lifecycle: Mutex<Lifecycle>,
    documents: Mutex<HashMap<String, OpenDocument>>,
}

impl LspClient {
    pub fn spawn(config: ProcessConfig, options: ClientOptions) -> Result<Self, Error> {
        Ok(Self {
            rpc: StdioRpc::spawn(config, options)?,
            gate: RwLock::new(()),
            lifecycle: Mutex::new(Lifecycle::Created),
            documents: Mutex::new(HashMap::new()),
        })
    }

    pub fn process_id(&self) -> u32 {
        self.rpc.process_id()
    }

    pub fn abort_handle(&self) -> LspAbortHandle {
        LspAbortHandle(self.rpc.abort_handle())
    }

    /// Consume the owner, abort and wait for its cleanup. Windows returns the
    /// recorded observation only after worker join and process/I/O destruction.
    /// Kernel cancellation may delay this call; the signal has no such wait.
    /// Linux observes cleanup for three seconds after its first cleanup trigger;
    /// timeout returns an unverified report while its owner retains eventual wait
    /// responsibility. Other portable hosts retain legacy direct-child cleanup.
    pub fn abort_and_join(self) -> ShutdownOutcome {
        self.rpc
            .abort(Error::Closed("language client aborted".into()));
        let mut outcome = match &*self.lifecycle.lock().unwrap() {
            Lifecycle::Stopped { outcome, .. } => *outcome,
            _ => ShutdownOutcome::default(),
        };
        outcome.windows = self.rpc.windows_shutdown_outcome();
        outcome.linux = self.rpc.linux_shutdown_outcome();
        outcome
    }

    /// Return the complete InitializeResult. Capabilities are retained and checked
    /// by the convenience methods. Failure closes the session; create a new client
    /// rather than retrying initialize on an uncertain server state.
    pub fn initialize(
        &self,
        root_uri: Option<&str>,
        initialization_options: Value,
    ) -> Result<Value, Error> {
        self.initialize_impl(root_uri, initialization_options, None, None)
    }

    /// Use a separate cold-start deadline without lengthening normal feature
    /// request deadlines. This overrides only the initialize request; writing the
    /// subsequent initialized notification retains the normal write timeout.
    pub fn initialize_with_timeout(
        &self,
        root_uri: Option<&str>,
        initialization_options: Value,
        timeout: Duration,
    ) -> Result<Value, Error> {
        self.initialize_impl(root_uri, initialization_options, Some(timeout), None)
    }

    /// Use one absolute startup deadline supplied by the startup owner, measured
    /// from accepted begin. The initialize response is additionally bounded by
    /// `initialize_timeout`; the initialized write uses only the remaining total
    /// budget, capped by the ordinary write timeout. Cleanup may outlast the
    /// deadline because live kernel I/O is always canceled and joined safely.
    pub fn initialize_with_deadline(
        &self,
        root_uri: Option<&str>,
        initialization_options: Value,
        initialize_timeout: Duration,
        startup_deadline: Instant,
    ) -> Result<Value, Error> {
        self.initialize_impl(
            root_uri,
            initialization_options,
            Some(initialize_timeout),
            Some(startup_deadline),
        )
    }

    fn initialize_impl(
        &self,
        root_uri: Option<&str>,
        initialization_options: Value,
        timeout: Option<Duration>,
        startup_deadline: Option<Instant>,
    ) -> Result<Value, Error> {
        let _gate = self.gate.write().unwrap();
        {
            let mut state = self.lifecycle.lock().unwrap();
            if !matches!(*state, Lifecycle::Created) {
                return Err(Error::InvalidState(
                    "initialize must be called exactly once".into(),
                ));
            }
            *state = Lifecycle::Initializing;
        }
        let result = (|| {
            let params = json!({
                "processId": std::process::id(),
                "clientInfo": {"name":"Cedar", "version":env!("CARGO_PKG_VERSION")},
                "rootUri": root_uri,
                "workspaceFolders": null,
                "initializationOptions": initialization_options,
                "capabilities": {
                    "general": {"positionEncodings":["utf-16"]},
                    "workspace": {"applyEdit":false, "configuration":false, "workspaceFolders":false,
                        "symbol":{"dynamicRegistration":false}},
                    "window": {"workDoneProgress":false},
                    "textDocument": {
                        "synchronization": {"dynamicRegistration":false,"willSave":false,"willSaveWaitUntil":false,"didSave":false},
                        "completion": {"dynamicRegistration":false,"completionItem":{"snippetSupport":false,"documentationFormat":["plaintext","markdown"],"resolveSupport":{"properties":["documentation","detail","additionalTextEdits"]}}},
                        "hover": {"dynamicRegistration":false,"contentFormat":["plaintext","markdown"]},
                        "definition": {"dynamicRegistration":false,"linkSupport":true},
                        "implementation": {"dynamicRegistration":false,"linkSupport":false},
                        "formatting": {"dynamicRegistration":false},
                        "references": {"dynamicRegistration":false},
                        "documentSymbol": {"dynamicRegistration":false,"hierarchicalDocumentSymbolSupport":true},
                        "publishDiagnostics": {"relatedInformation":true,"versionSupport":true,"codeDescriptionSupport":true,"dataSupport":true}
                    }
                }
            });
            let result = match (timeout, startup_deadline) {
                (Some(timeout), Some(deadline)) => self.rpc.request_with_deadline(
                    "initialize",
                    params,
                    crate::transport::clipped_deadline(Instant::now(), timeout, deadline)?,
                )?,
                (Some(timeout), None) => {
                    self.rpc
                        .request_with_timeout("initialize", params, timeout)?
                }
                _ => self.rpc.request("initialize", params)?,
            };
            let capabilities = result
                .get("capabilities")
                .filter(|c| c.is_object())
                .ok_or_else(|| {
                    Error::Protocol("initialize result must contain capabilities".into())
                })?
                .clone();
            if let Some(encoding) = capabilities.get("positionEncoding") {
                if encoding.as_str() != Some("utf-16") {
                    return Err(Error::Unsupported(format!(
                        "server chose unadvertised position encoding {encoding}"
                    )));
                }
            }
            let sync = synchronization(&capabilities)?;
            if let Some(deadline) = startup_deadline {
                self.rpc.notify_before("initialized", json!({}), deadline)?;
            } else {
                self.rpc.notify("initialized", json!({}))?;
            }
            *self.lifecycle.lock().unwrap() = Lifecycle::Ready {
                capabilities: Arc::new(capabilities),
                sync,
            };
            Ok(result)
        })();
        if let Err(error) = &result {
            self.rpc.abort_after_failure(error.clone());
            *self.lifecycle.lock().unwrap() = Lifecycle::Stopped {
                result: Err(error.clone()),
                outcome: ShutdownOutcome {
                    windows: self.rpc.windows_shutdown_outcome(),
                    linux: self.rpc.linux_shutdown_outcome(),
                    ..ShutdownOutcome::default()
                },
            };
        }
        result
    }

    /// Open one document. URIs must be properly encoded and refer to the server's
    /// filesystem when used remotely. The client never reads a local path here.
    pub fn did_open(
        &self,
        uri: &str,
        language_id: &str,
        version: i32,
        text: &str,
    ) -> Result<(), Error> {
        let _gate = self.gate.read().unwrap();
        let (_, sync) = self.ready()?;
        if !sync.open_close {
            return Err(Error::Unsupported(
                "server does not accept didOpen/didClose".into(),
            ));
        }
        if uri.len() > 16 * 1024 || language_id.len() > 256 {
            return Err(Error::InvalidState(
                "document URI or language ID exceeds limit".into(),
            ));
        }
        let mut documents = self.documents.lock().unwrap();
        if documents.contains_key(uri) {
            return Err(Error::InvalidState("document already open".into()));
        }
        if documents.len() >= 4096 {
            return Err(Error::InvalidState(
                "open document limit reached (4096)".into(),
            ));
        }
        self.rpc.notify("textDocument/didOpen", json!({"textDocument":{"uri":uri,"languageId":language_id,"version":version,"text":text}}))?;
        documents.insert(
            uri.into(),
            OpenDocument {
                version,
                end: Position::end_of(text),
            },
        );
        Ok(())
    }

    /// Replace the whole editor buffer. Full-sync servers receive `{text}`;
    /// incremental servers receive a single edit spanning the previous document,
    /// computed in UTF-16. This honors their negotiated synchronization mode.
    pub fn did_change(&self, uri: &str, version: i32, text: &str) -> Result<(), Error> {
        let _gate = self.gate.read().unwrap();
        let (_, sync) = self.ready()?;
        if sync.change == 0 {
            return Err(Error::Unsupported(
                "server does not accept didChange".into(),
            ));
        }
        let mut documents = self.documents.lock().unwrap();
        let previous = documents
            .get(uri)
            .ok_or_else(|| Error::InvalidState("document is not open".into()))?;
        if version <= previous.version {
            return Err(Error::InvalidState("document version must increase".into()));
        }
        let change = if sync.change == 2 {
            json!({"range":{"start":Position::default(),"end":previous.end}, "text":text})
        } else {
            json!({"text":text})
        };
        self.rpc.notify(
            "textDocument/didChange",
            json!({"textDocument":{"uri":uri,"version":version},"contentChanges":[change]}),
        )?;
        documents.insert(
            uri.into(),
            OpenDocument {
                version,
                end: Position::end_of(text),
            },
        );
        Ok(())
    }

    pub fn did_close(&self, uri: &str) -> Result<(), Error> {
        let _gate = self.gate.read().unwrap();
        self.ready()?;
        let mut documents = self.documents.lock().unwrap();
        if !documents.contains_key(uri) {
            return Err(Error::InvalidState("document is not open".into()));
        }
        self.rpc
            .notify("textDocument/didClose", json!({"textDocument":{"uri":uri}}))?;
        documents.remove(uri);
        Ok(())
    }

    /// Return null, a CompletionItem array, or a CompletionList unchanged.
    pub fn completion(&self, uri: &str, position: Position) -> Result<Value, Error> {
        self.feature(
            "completionProvider",
            "textDocument/completion",
            uri,
            position,
        )
    }

    /// Resolve one original completion item, preserving its opaque `data` and
    /// extension fields. Requires the server's static `resolveProvider` capability.
    /// The returned item may contain lazy documentation, detail and import edits.
    /// This call does not execute commands or apply any text edits. The UI must
    /// validate its captured document/version before applying the returned edits.
    pub fn resolve_completion(&self, item: Value) -> Result<Value, Error> {
        let _gate = self.gate.read().unwrap();
        let (capabilities, _) = self.ready()?;
        if capabilities
            .pointer("/completionProvider/resolveProvider")
            .and_then(Value::as_bool)
            != Some(true)
        {
            return Err(Error::Unsupported("completionItem/resolve".into()));
        }
        if !item.is_object() || item.get("label").and_then(Value::as_str).is_none() {
            return Err(Error::InvalidState(
                "completion item must be an object with a string label".into(),
            ));
        }
        let resolved = self.rpc.request("completionItem/resolve", item)?;
        if !resolved.is_object() || resolved.get("label").and_then(Value::as_str).is_none() {
            return Err(Error::Protocol(
                "completion resolve result must be an item with a string label".into(),
            ));
        }
        Ok(resolved)
    }

    /// Return null, Location(s), or LocationLink(s) unchanged.
    pub fn definition(&self, uri: &str, position: Position) -> Result<Value, Error> {
        self.feature(
            "definitionProvider",
            "textDocument/definition",
            uri,
            position,
        )
    }

    /// Return null or a Hover object unchanged. Render server markup safely.
    pub fn hover(&self, uri: &str, position: Position) -> Result<Value, Error> {
        self.feature("hoverProvider", "textDocument/hover", uri, position)
    }

    /// Return plain TextEdit[] or null unchanged. The caller must validate the
    /// entire result against its captured document version before applying it.
    /// This never applies edits or executes server commands. Cedar limits tab
    /// size to 1..=16 rather than accepting arbitrary LSP unsigned integers.
    pub fn formatting(
        &self,
        uri: &str,
        tab_size: u32,
        insert_spaces: bool,
    ) -> Result<Value, Error> {
        let _gate = self.gate.read().unwrap();
        self.ready_document("documentFormattingProvider", "textDocument/formatting", uri)?;
        if !(1..=16).contains(&tab_size) {
            return Err(Error::InvalidState(
                "formatting tab size must be between 1 and 16".into(),
            ));
        }
        self.rpc.request(
            "textDocument/formatting",
            json!({"textDocument":{"uri":uri},"options":{"tabSize":tab_size,"insertSpaces":insert_spaces}}),
        )
    }

    /// Return Location[] or null unchanged. References do not use LocationLink
    /// or carry target document versions; callers must confine navigation URIs.
    pub fn references(
        &self,
        uri: &str,
        position: Position,
        include_declaration: bool,
    ) -> Result<Value, Error> {
        let _gate = self.gate.read().unwrap();
        self.ready_document("referencesProvider", "textDocument/references", uri)?;
        validate_position(position)?;
        self.rpc.request(
            "textDocument/references",
            json!({"textDocument":{"uri":uri},"position":position,"context":{"includeDeclaration":include_declaration}}),
        )
    }

    /// One standard implementation request on an acknowledged open document.
    /// Null/single Location/Location[] normalize to a strict bounded array. The
    /// results carry no target versions and may lag the server's indexing state;
    /// every navigation still requires independent workspace URI confinement.
    pub fn implementations(&self, uri: &str, position: Position) -> Result<Value, Error> {
        let _gate = self.gate.read().unwrap();
        self.ready_document("implementationProvider", "textDocument/implementation", uri)?;
        validate_position(position)?;
        let result = self.rpc.request(
            "textDocument/implementation",
            json!({"textDocument":{"uri":uri},"position":position}),
        )?;
        crate::implementations::normalize_result(result)
    }

    /// Return DocumentSymbol[], SymbolInformation[], or null unchanged.
    /// Hierarchical results refer to this document; flat results still require
    /// URI confinement. Consumers must not infer a hierarchy from flat results.
    pub fn document_symbols(&self, uri: &str) -> Result<Value, Error> {
        let _gate = self.gate.read().unwrap();
        self.ready_document("documentSymbolProvider", "textDocument/documentSymbol", uri)?;
        self.rpc.request(
            "textDocument/documentSymbol",
            json!({"textDocument":{"uri":uri}}),
        )
    }

    /// Standard workspace/symbol with a literal, bounded query. Requires an
    /// initialized, advertising server, but no open document or cursor. Only
    /// complete flat SymbolInformation locations are supported; returned URIs
    /// still require independent workspace confinement before navigation.
    pub fn workspace_symbols(&self, query: &str) -> Result<Value, Error> {
        let _gate = self.gate.read().unwrap();
        let (capabilities, _) = self.ready()?;
        if !capabilities
            .get("workspaceSymbolProvider")
            .is_some_and(|value| value == &Value::Bool(true) || value.is_object())
        {
            return Err(Error::Unsupported("workspace/symbol".into()));
        }
        crate::workspace_symbols::validate_query(query)?;
        let result = self
            .rpc
            .request("workspace/symbol", json!({"query":query}))?;
        crate::workspace_symbols::normalize_result(result)
    }

    /// Escape hatch for extensions after initialization. Caller owns capability
    /// checks and decoding. Do not use this to bypass the lifecycle/document APIs.
    pub fn request(&self, method: &str, params: Value) -> Result<Value, Error> {
        let _gate = self.gate.read().unwrap();
        self.ready()?;
        if matches!(method, "initialize" | "shutdown") {
            return Err(Error::InvalidState("use the lifecycle methods".into()));
        }
        self.rpc.request(method, params)
    }

    pub fn request_with_timeout(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, Error> {
        let _gate = self.gate.read().unwrap();
        self.ready()?;
        if matches!(method, "initialize" | "shutdown") {
            return Err(Error::InvalidState("use the lifecycle methods".into()));
        }
        self.rpc.request_with_timeout(method, params, timeout)
    }

    /// Send an extension notification after initialization. Lifecycle and tracked
    /// document notifications must go through their dedicated methods.
    pub fn notify(&self, method: &str, params: Value) -> Result<(), Error> {
        let _gate = self.gate.read().unwrap();
        self.ready()?;
        if matches!(
            method,
            "initialized"
                | "exit"
                | "textDocument/didOpen"
                | "textDocument/didChange"
                | "textDocument/didClose"
        ) {
            return Err(Error::InvalidState(
                "use the lifecycle/document methods".into(),
            ));
        }
        self.rpc.notify(method, params)
    }

    pub fn next_event(&self, timeout: Duration) -> Result<Option<LspEvent>, Error> {
        self.rpc
            .next_event(timeout)?
            .map(|event| match event {
                RpcEvent::Notification { method, params }
                    if method == "textDocument/publishDiagnostics" =>
                {
                    let diagnostics = serde_json::from_value(params)
                        .map_err(|e| Error::Protocol(format!("invalid diagnostics: {e}")))?;
                    Ok(LspEvent::Diagnostics(diagnostics))
                }
                RpcEvent::Notification { method, params } => {
                    Ok(LspEvent::Notification { method, params })
                }
                RpcEvent::UnsupportedServerRequest { method, id } => {
                    Ok(LspEvent::UnsupportedServerRequest { method, id })
                }
                RpcEvent::Lagged { dropped } => Ok(LspEvent::Lagged { dropped }),
                RpcEvent::Closed(error) => Ok(LspEvent::Closed(error)),
            })
            .transpose()
    }

    /// Perform shutdown -> response -> exit, then reap the process. This legacy
    /// result may be Ok after forced cleanup; use shutdown_with_outcome to tell
    /// whether owned Windows/Linux shutdown was graceful. Repeated calls return the
    /// original result, including failures, without sending another request.
    pub fn shutdown(&self) -> Result<(), Error> {
        self.shutdown_with_outcome().0
    }

    /// Stop once and return the original legacy result separately from a bounded
    /// ownership report. Even protocol failure returns the cleanup observation;
    /// no diagnostic text is copied into the report. Windows reports appear only
    /// after process/I/O owner destruction and worker join. Kernel cancellation
    /// can delay that join; this API does not promise a hard cleanup deadline.
    /// Linux reports a durable unverified result after its fixed cleanup
    /// observation budget; the same owner may still be waiting for its root.
    /// Calling before initialization returns InvalidState without stopping.
    pub fn shutdown_with_outcome(&self) -> (Result<(), Error>, ShutdownOutcome) {
        let _gate = self.gate.write().unwrap();
        {
            let mut state = self.lifecycle.lock().unwrap();
            if let Lifecycle::Stopped { result, outcome } = &*state {
                return (result.clone(), *outcome);
            }
            if !matches!(*state, Lifecycle::Ready { .. }) {
                return (
                    Err(Error::InvalidState(
                        "shutdown requires an initialized client".into(),
                    )),
                    ShutdownOutcome::default(),
                );
            }
            *state = Lifecycle::ShuttingDown;
        }
        let mut outcome = ShutdownOutcome::default();
        let result = (|| {
            let shutdown = self.rpc.request("shutdown", Value::Null);
            outcome.shutdown_response_received =
                matches!(shutdown, Ok(_) | Err(Error::Remote { .. }));
            shutdown?;
            self.rpc.send_exit()?;
            outcome.exit_frame_completed = true;
            self.rpc.finish_process()
        })();
        if let Err(error) = &result {
            self.rpc.abort_after_failure(error.clone());
        }
        outcome.windows = self.rpc.windows_shutdown_outcome();
        outcome.linux = self.rpc.linux_shutdown_outcome();
        *self.lifecycle.lock().unwrap() = Lifecycle::Stopped {
            result: result.clone(),
            outcome,
        };
        self.documents.lock().unwrap().clear();
        (result, outcome)
    }

    fn ready(&self) -> Result<(Arc<Value>, SyncCapabilities), Error> {
        match &*self.lifecycle.lock().unwrap() {
            Lifecycle::Ready { capabilities, sync } => Ok((capabilities.clone(), *sync)),
            _ => Err(Error::InvalidState(
                "language client is not initialized or has stopped".into(),
            )),
        }
    }

    fn feature(
        &self,
        capability: &str,
        method: &str,
        uri: &str,
        position: Position,
    ) -> Result<Value, Error> {
        let _gate = self.gate.read().unwrap();
        self.ready_document(capability, method, uri)?;
        validate_position(position)?;
        self.rpc.request(
            method,
            json!({"textDocument":{"uri":uri},"position":position}),
        )
    }

    /// The caller holds the lifecycle gate for the entire operation.
    fn ready_document(&self, capability: &str, method: &str, uri: &str) -> Result<(), Error> {
        let (capabilities, _) = self.ready()?;
        if !capabilities
            .get(capability)
            .is_some_and(|v| v == &Value::Bool(true) || v.is_object())
        {
            return Err(Error::Unsupported(method.into()));
        }
        if !self.documents.lock().unwrap().contains_key(uri) {
            return Err(Error::InvalidState("document is not open".into()));
        }
        Ok(())
    }
}

fn validate_position(position: Position) -> Result<(), Error> {
    if position.line > i32::MAX as u32 || position.character > i32::MAX as u32 {
        return Err(Error::InvalidState(
            "LSP positions must fit unsigned 31-bit integers".into(),
        ));
    }
    Ok(())
}

fn synchronization(capabilities: &Value) -> Result<SyncCapabilities, Error> {
    let sync = match capabilities.get("textDocumentSync") {
        None => SyncCapabilities {
            open_close: false,
            change: 0,
        },
        Some(Value::Number(number)) => {
            let kind = number
                .as_u64()
                .ok_or_else(|| Error::Protocol("invalid textDocumentSync".into()))?;
            SyncCapabilities {
                open_close: kind != 0,
                change: kind,
            }
        }
        Some(Value::Object(options)) => SyncCapabilities {
            open_close: match options.get("openClose") {
                None => false,
                Some(value) => value
                    .as_bool()
                    .ok_or_else(|| Error::Protocol("invalid textDocumentSync.openClose".into()))?,
            },
            change: match options.get("change") {
                None => 0,
                Some(value) => value
                    .as_u64()
                    .ok_or_else(|| Error::Protocol("invalid textDocumentSync.change".into()))?,
            },
        },
        _ => return Err(Error::Protocol("invalid textDocumentSync".into())),
    };
    if sync.change > 2 {
        return Err(Error::Unsupported(
            "unknown document synchronization kind".into(),
        ));
    }
    Ok(sync)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn end_position_uses_utf16_and_all_line_endings() {
        assert_eq!(
            Position::end_of("a🦀"),
            Position {
                line: 0,
                character: 3
            }
        );
        assert_eq!(
            Position::end_of("a\r\n🦀\rb\nλ🦀"),
            Position {
                line: 3,
                character: 3
            }
        );
        assert_eq!(
            Position::end_of("x\n"),
            Position {
                line: 1,
                character: 0
            }
        );
    }

    #[test]
    fn synchronization_negotiation() {
        assert_eq!(
            synchronization(&json!({"textDocumentSync":2}))
                .unwrap()
                .change,
            2
        );
        assert!(!synchronization(&json!({})).unwrap().open_close);
        assert!(
            synchronization(&json!({"textDocumentSync":{"openClose":true,"change":1}}))
                .unwrap()
                .open_close
        );
        assert!(synchronization(&json!({"textDocumentSync":99})).is_err());
        assert!(synchronization(&json!({"textDocumentSync":{"change":"2"}})).is_err());
        assert!(synchronization(&json!({"textDocumentSync":{"openClose":"true"}})).is_err());
    }
}
