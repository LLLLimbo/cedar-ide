//! Native language UI. Remote payloads remain inert; edits are snapshot-checked transactions.
#[path = "language_features.rs"]
mod features;
#[path = "java_diagnostics.rs"]
mod java_diagnostics;
#[path = "java_startup.rs"]
mod java_startup;

use crate::{
    completion::{self, Candidate, Position, Range},
    java_language::{JavaConfiguration, JavaStopOutcome, ServerMode, StopStatus},
    language_results::{self, Diagnostics, Location},
    language_sync::{Acknowledged, SyncTracker},
    model::Document,
    CedarApp, Job, AMBER, GREEN, MUTED, RED,
};
use cedar_protocol::{LanguageQueryKind, Operation, MAX_FILE_BYTES};
use eframe::egui::{self, RichText};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

#[derive(Clone)]
pub(super) struct QueryContext {
    session: u64,
    document: u64,
    edit_version: u64,
    source: String,
    cursor: Position,
}
#[derive(Clone)]
struct QueryIntent {
    context: QueryContext,
    kind: LanguageQueryKind,
}

pub(super) struct Action {
    pub session: u64,
    pub kind: ActionKind,
}
pub(super) enum ActionKind {
    Start,
    JavaStartBegin,
    JavaStartPoll {
        startup_id: u64,
    },
    JavaStartCancel {
        startup_id: u64,
    },
    Stop,
    Sync {
        document: u64,
        version: i32,
        edit_version: u64,
    },
    Close {
        document: u64,
    },
    Query {
        context: QueryContext,
        kind: LanguageQueryKind,
    },
    Events,
    RefreshJavaDiagnostics {
        context: java_diagnostics::RefreshContext,
    },
    Feature {
        request: features::FeatureRequest,
    },
    ResolveUri {
        sequence: u64,
        navigation: u64,
        location: Location,
    },
    ResolveCompletion {
        context: QueryContext,
        original: Value,
        acceptance: u64,
    },
}
impl Action {
    pub fn is_java_startup(&self) -> bool {
        matches!(
            self.kind,
            ActionKind::JavaStartBegin
                | ActionKind::JavaStartPoll { .. }
                | ActionKind::JavaStartCancel { .. }
        )
    }
    pub fn is_stop(&self) -> bool {
        matches!(self.kind, ActionKind::Stop)
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    Problems,
    Completion,
    Definitions,
    Format,
    References,
    Outline,
    Hover,
    Activity,
}
struct CompletionMenu {
    context: QueryContext,
    candidates: Vec<Candidate>,
    selected: usize,
    incomplete: bool,
    truncated: bool,
}
struct DeferredNavigation {
    session: u64,
    sequence: u64,
    navigation: u64,
    range: Range,
}

pub(super) struct LanguagePanel {
    pub running: bool,
    pub cjk_seen: bool,
    pub session: u64,
    pub sync: SyncTracker,
    features: features::FeatureState,
    next_version: i32,
    closed_uris: HashSet<String>,
    mode: ServerMode,
    java: JavaConfiguration,
    restart_blocked: bool,
    startup: Option<java_startup::Startup>,
    program: String,
    args: String,
    language_id: String,
    capabilities: Value,
    diagnostics: Diagnostics,
    diagnostics_exited: bool,
    java_diagnostics_refresh_supported: bool,
    diagnostic_refresh: Option<java_diagnostics::RefreshState>,
    diagnostic_refresh_sequence: u64,
    definitions: Vec<Location>,
    hover: String,
    output: String,
    view: View,
    automatic: bool,
    paused_reason: Option<String>,
    next_events: f64,
    intent: Option<QueryIntent>,
    completions: Option<CompletionMenu>,
    completion_popup: bool,
    acceptance_sequence: u64,
    navigation_sequence: u64,
    deferred_navigation: HashMap<String, DeferredNavigation>,
}
impl Default for LanguagePanel {
    fn default() -> Self {
        Self {
            running: false, cjk_seen: false, session: 0, sync: SyncTracker::default(), features: features::FeatureState::default(), next_version: 1, closed_uris: HashSet::new(),
            mode: ServerMode::Generic, java: JavaConfiguration::default(), restart_blocked: false, startup: None,
            program: String::new(), args: "[]".into(), language_id: "rust".into(), capabilities: Value::Null,
            diagnostics: Diagnostics::default(), diagnostics_exited: false, java_diagnostics_refresh_supported: false,
            diagnostic_refresh: None, diagnostic_refresh_sequence: 0, definitions: Vec::new(), hover: String::new(),
            output: "Start an installed stdio language server. Java/Kotlin servers and their JDK must be installed on the workspace host.".into(),
            view: View::Problems, automatic: true, paused_reason: None, next_events: 0.0,
            intent: None, completions: None, completion_popup: false, acceptance_sequence: 0, navigation_sequence: 0, deferred_navigation: HashMap::new(),
        }
    }
}
impl LanguagePanel {
    pub fn reset(&mut self) {
        self.features.reset();
        self.session = self.session.wrapping_add(1);
        self.running = false;
        self.restart_blocked = false;
        self.startup = None;
        self.sync.clear();
        self.next_version = 1;
        self.closed_uris.clear();
        self.diagnostics.clear();
        self.diagnostics_exited = false;
        self.java_diagnostics_refresh_supported = false;
        self.diagnostic_refresh = None;
        self.definitions.clear();
        self.hover.clear();
        self.intent = None;
        self.acceptance_sequence = self.acceptance_sequence.wrapping_add(1);
        self.completions = None;
        self.completion_popup = false;
        self.deferred_navigation.clear();
        self.automatic = true;
        self.paused_reason = None;
        self.capabilities = Value::Null;
        self.output = "Language session stopped. Start a server when needed.".into();
    }
    pub fn startup_active(&self) -> bool {
        self.startup
            .as_ref()
            .is_some_and(java_startup::Startup::active)
    }
    pub(super) fn cancel_navigation(&mut self) {
        self.features.cancel_pending();
        self.cancel_deferred_navigation();
    }
    fn cancel_deferred_navigation(&mut self) {
        self.navigation_sequence = self.navigation_sequence.wrapping_add(1);
        self.deferred_navigation.clear();
    }
    fn remember_closed_uri(&mut self, uri: String) {
        if self.closed_uris.len() >= 512 {
            if let Some(old) = self.closed_uris.iter().next().cloned() {
                self.closed_uris.remove(&old);
            }
        }
        self.closed_uris.insert(uri);
    }
    fn document_language_id(&self) -> &str {
        if self.mode == ServerMode::Java {
            "java"
        } else {
            self.language_id.trim()
        }
    }
    fn matches(&self, path: &str) -> bool {
        match self.document_language_id() {
            "java" => path.ends_with(".java"),
            "kotlin" => path.ends_with(".kt") || path.ends_with(".kts"),
            "rust" => path.ends_with(".rs"),
            "python" => path.ends_with(".py"),
            "typescript" => path.ends_with(".ts") || path.ends_with(".tsx"),
            "javascript" => path.ends_with(".js") || path.ends_with(".jsx"),
            _ => true,
        }
    }
    fn supports(&self, key: &str) -> bool {
        self.capabilities
            .get(key)
            .is_some_and(|value| value.as_bool() == Some(true) || value.is_object())
    }
    fn valid(&self, context: &QueryContext, documents: &[Document]) -> bool {
        self.running
            && self.session == context.session
            && documents.iter().any(|doc| {
                doc.id == context.document
                    && doc.edit_version == context.edit_version
                    && doc.text == context.source
            })
    }
    fn activity(&mut self, value: &Value) {
        self.output = language_results::bounded_json(value, 128 * 1024);
        self.cjk_seen |= crate::system_fonts::contains_cjk(&self.output);
    }
}

impl CedarApp {
    pub(super) fn language_busy(&self) -> bool {
        self.pending
            .values()
            .any(|job| matches!(job, Job::Language(_)))
    }
    fn language_request(&mut self, operation: Operation, kind: ActionKind) -> u64 {
        self.request(
            operation,
            Job::Language(Action {
                session: self.language.session,
                kind,
            }),
        )
    }
    fn start_language(&mut self) {
        if !self.ready()
            || self.language.running
            || self.language.startup_active()
            || self.language_busy()
            || self.recovery.closing.is_some()
            || self.close_after_language_stop
        {
            return;
        }
        if self.language.restart_blocked {
            self.error = Some(
                "Reconnect before starting another Java session; prior cleanup was not verified"
                    .into(),
            );
            return;
        }
        if !self.execution_trusted() {
            self.error =
                Some("Language servers require trusted tool permission for this connection".into());
            return;
        }
        if !self.backend_generic_language_supported() && self.backend_java_language_supported() {
            self.language.mode = ServerMode::Java;
        }
        let operation = match self.language.mode {
            ServerMode::Java => {
                if !self.backend_java_language_supported() {
                    self.error = Some(
                        self.unsupported_message("the complete Java language session lifecycle"),
                    );
                    return;
                }
                match self.language.java.operation() {
                    Ok(operation) => operation,
                    Err(error) => {
                        self.error = Some(error);
                        return;
                    }
                }
            }
            ServerMode::Generic => {
                if !self.backend_generic_language_supported() {
                    self.error =
                        Some(self.unsupported_message("the complete language session lifecycle"));
                    return;
                }
                let args: Vec<String> = match serde_json::from_str(&self.language.args) {
                    Ok(args) => args,
                    Err(_) => {
                        self.error = Some(
                            "Language server arguments must be a JSON array of strings".into(),
                        );
                        return;
                    }
                };
                let program = self.language.program.trim().to_owned();
                if program.is_empty() || self.language.language_id.trim().is_empty() {
                    self.error =
                        Some("Enter the installed server executable and language ID".into());
                    return;
                }
                Operation::LanguageStart { program, args }
            }
        };
        self.language.reset();
        if self.language.mode == ServerMode::Java {
            self.language.language_id = "java".into();
        }
        if self.language.mode == ServerMode::Java && self.backend_java_startup_supported() {
            let Operation::LanguageStartJava {
                java_executable,
                distribution,
                data_directory,
            } = operation
            else {
                unreachable!()
            };
            self.begin_java_startup(Operation::LanguageStartJavaBegin {
                java_executable,
                distribution,
                data_directory,
            });
        } else {
            self.language_request(operation, ActionKind::Start);
        }
    }

    pub(super) fn stop_language(&mut self) {
        if self.language.startup_active() {
            self.cancel_java_startup();
            return;
        }
        self.language.diagnostic_refresh = None;
        self.language.features.reset();
        self.language.intent = None;
        self.language.automatic = false;
        self.language_request(Operation::LanguageStop, ActionKind::Stop);
    }
    fn sync_document(&mut self, document: u64) {
        if !self.language.running {
            return;
        }
        let Some(doc) = self.documents.iter().find(|doc| doc.id == document) else {
            return;
        };
        if self.language.mode == ServerMode::Java && !self.language.matches(&doc.path) {
            return;
        }
        if doc.text.len() > MAX_FILE_BYTES {
            self.language.sync.fail(doc.id, doc.edit_version);
            self.language.intent = None;
            self.error = Some(
                "Language synchronization is limited to 1 MiB per document; your draft is retained"
                    .into(),
            );
            return;
        }
        let Some(document_version) = self.language.sync.next_version(document) else {
            self.error = Some("Language version limit reached; restart the server".into());
            return;
        };
        let version = document_version.max(self.language.next_version);
        let Some(next_version) = version.checked_add(1) else {
            self.error = Some("Language version limit reached; restart the server".into());
            self.language.intent = None;
            return;
        };
        self.language.next_version = next_version;
        let edit_version = doc.edit_version;
        let op = if self.language.sync.opened.contains_key(&document) {
            Operation::LanguageChange {
                path: doc.path.clone(),
                version,
                text: doc.text.clone(),
            }
        } else {
            Operation::LanguageOpen {
                path: doc.path.clone(),
                language_id: self.language.document_language_id().to_owned(),
                version,
                text: doc.text.clone(),
            }
        };
        self.language_request(
            op,
            ActionKind::Sync {
                document,
                version,
                edit_version,
            },
        );
    }
    fn sync_current_language(&mut self) {
        if let Some(document) = self.active_document {
            self.language.sync.retry(document);
            self.language.paused_reason = None;
            self.sync_document(document);
        }
    }
    pub(super) fn close_language_document(&mut self, document: u64) {
        if self
            .language
            .diagnostic_refresh
            .as_ref()
            .is_some_and(|refresh| refresh.context.document == document)
        {
            self.language.diagnostic_refresh = None;
        }
        self.language.features.cancel_pending();
        let opening = self.pending.values().any(|job| matches!(job, Job::Language(Action { kind: ActionKind::Sync { document: id, .. }, .. }) if *id == document));
        if self.ready()
            && self.language.running
            && (self.language.sync.opened.contains_key(&document) || opening)
        {
            if let Some(path) = self
                .documents
                .iter()
                .find(|doc| doc.id == document)
                .map(|doc| doc.path.clone())
            {
                self.language_request(
                    Operation::LanguageClose { path },
                    ActionKind::Close { document },
                );
            }
        }
        if let Some(ack) = self.language.sync.opened.get(&document).cloned() {
            self.language.remember_closed_uri(ack.uri.clone());
            self.language.diagnostics.files.remove(&ack.uri);
        }
        self.language.sync.close(document);
        if self
            .language
            .completions
            .as_ref()
            .is_some_and(|menu| menu.context.document == document)
        {
            self.language.completions = None;
            self.language.completion_popup = false;
        }
        if self
            .language
            .intent
            .as_ref()
            .is_some_and(|intent| intent.context.document == document)
        {
            self.language.intent = None;
        }
    }
    pub(super) fn language_public_error(&self, action: &Action, error: &str) -> String {
        if self.language.mode != ServerMode::Java {
            return error.into();
        }
        match action.kind {
            ActionKind::Start => "Java server startup failed. Check the Java executable, JDT distribution and data directory on the workspace host.".into(),
            ActionKind::Stop => "Java session closed; process cleanup could not be verified. Reconnect before starting another Java session.".into(),
            _ => "Java request failed. Your unsaved draft is retained; reconnect if the session is no longer available.".into(),
        }
    }
    pub(super) fn language_error(&mut self, action: &Action, error: &str) {
        if action.session != self.language.session {
            return;
        }
        match action.kind {
            ActionKind::Sync {
                document,
                edit_version,
                ..
            } => {
                self.language.sync.fail(document, edit_version);
                self.language.features.cancel_pending();
                self.language.intent = None;
                self.language.paused_reason = Some(error.into());
            }
            ActionKind::Events => {
                self.language.automatic = false;
                self.language.paused_reason = Some(error.into());
            }
            ActionKind::Start => self.language.running = false,
            ActionKind::RefreshJavaDiagnostics { ref context } => {
                if self.java_diagnostics_refresh_is_current(context) {
                    self.language.diagnostic_refresh = None;
                }
            }
            ActionKind::Stop if self.language.mode == ServerMode::Java => {
                self.language.reset();
                self.language.restart_blocked = true;
                self.language.output = error.into();
            }
            _ => {}
        }
    }
    pub(super) fn request_language_feature(&mut self, kind: LanguageQueryKind) {
        let Some(doc) = self.active() else {
            return;
        };
        if !self.ready() || !self.language.running {
            self.error = Some("Start a language server first".into());
            return;
        }
        if !self.language.matches(&doc.path) {
            self.error =
                Some("This file does not match the running server’s language profile".into());
            return;
        }
        if matches!(kind, LanguageQueryKind::Definition)
            && !self.backend_supports("language_resolve_uri")
        {
            self.error = Some(self.unsupported_message("language_resolve_uri"));
            return;
        }
        if !self.backend_supports("language_query") {
            self.error = Some(self.unsupported_message("language_query"));
            return;
        }
        let capability = match kind {
            LanguageQueryKind::Completion => "completionProvider",
            LanguageQueryKind::Definition => "definitionProvider",
            LanguageQueryKind::Hover => "hoverProvider",
        };
        if !self.language.supports(capability) {
            self.error = Some("The running language server does not advertise this feature".into());
            return;
        }
        if doc.text.len() > MAX_FILE_BYTES {
            self.error = Some(
                "Language features are limited to 1 MiB documents; your draft is retained".into(),
            );
            return;
        }
        let context = QueryContext {
            session: self.language.session,
            document: doc.id,
            edit_version: doc.edit_version,
            source: doc.text.clone(),
            cursor: match utf16_position(&doc.text, doc.cursor) {
                Ok(cursor) => cursor,
                Err(error) => {
                    self.error = Some(error);
                    return;
                }
            },
        };
        let document = doc.id;
        self.language.features.cancel_pending();
        self.language.intent = Some(QueryIntent { context, kind });
        self.language.sync.retry(document);
        self.language.paused_reason = None;
    }
    pub(super) fn language_tick(&mut self, ctx: &egui::Context) {
        if self.language.startup_active() {
            self.java_startup_tick(ctx);
            return;
        }
        self.invalidate_diagnostics_refresh();
        self.invalidate_language_features();
        if !self.ready() || !self.language.running || self.close_after_language_stop {
            return;
        }
        let now = ctx.input(|input| input.time);
        for doc in &self.documents {
            if self.language.matches(&doc.path) {
                self.language.sync.observe(doc.id, doc.edit_version, now);
            }
        }
        if self
            .language
            .completions
            .as_ref()
            .is_some_and(|menu| !self.language.valid(&menu.context, &self.documents))
        {
            self.language.completions = None;
            self.language.completion_popup = false;
        }
        if self
            .language
            .intent
            .as_ref()
            .is_some_and(|intent| !self.language.valid(&intent.context, &self.documents))
        {
            self.language.intent = None;
            self.notice = "Code changed while language work was pending; request it again".into();
        }
        if !self.pending.is_empty() {
            return;
        }
        if self.language_feature_tick() {
            return;
        }
        if let Some(intent) = self.language.intent.clone() {
            if self
                .language
                .sync
                .synced(intent.context.document, intent.context.edit_version)
            {
                if let Some(path) = self
                    .documents
                    .iter()
                    .find(|doc| doc.id == intent.context.document)
                    .map(|doc| doc.path.clone())
                {
                    self.language.intent = None;
                    self.language_request(
                        Operation::LanguageQuery {
                            path,
                            line: intent.context.cursor.line,
                            character: intent.context.cursor.character,
                            kind: intent.kind.clone(),
                        },
                        ActionKind::Query {
                            context: intent.context,
                            kind: intent.kind,
                        },
                    );
                }
            } else {
                self.sync_document(intent.context.document);
            }
            return;
        }
        if !self.language.automatic {
            return;
        }
        let due = self
            .documents
            .iter()
            .filter(|doc| self.language.matches(&doc.path))
            .filter_map(|doc| {
                self.language
                    .sync
                    .deadline(doc.id, doc.edit_version)
                    .map(|deadline| (doc.id, deadline))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((document, deadline)) = due {
            if now >= deadline {
                self.sync_document(document);
                return;
            }
        }
        if now >= self.language.next_events {
            self.language.next_events = now + 1.0;
            self.language_request(Operation::LanguageEvents, ActionKind::Events);
            return;
        }
        let next = due.map_or(self.language.next_events, |(_, deadline)| {
            deadline.min(self.language.next_events)
        });
        ctx.request_repaint_after(Duration::from_secs_f64((next - now).clamp(0.016, 1.0)));
    }

    pub(super) fn apply_language_action(&mut self, action: Action, value: Value) {
        if action.session != self.language.session {
            return;
        }
        if action.is_java_startup() {
            self.apply_java_startup_action(action, value);
            return;
        }
        if let ActionKind::RefreshJavaDiagnostics { context } = &action.kind {
            self.apply_java_diagnostics_refresh(context, &value);
            return;
        }
        if matches!(action.kind, ActionKind::Stop) {
            if self.language.mode == ServerMode::Java {
                let outcome = JavaStopOutcome::parse(&value);
                self.language.reset();
                match outcome {
                    Ok(outcome) => {
                        let message = outcome.message();
                        self.language.output = message.clone();
                        self.notice = message.clone();
                        if outcome.status == StopStatus::Error {
                            self.close_after_language_stop = false;
                            self.close_snapshot = None;
                            self.error = Some(message);
                        }
                    }
                    Err(_) => {
                        let message = "Java session closed; process cleanup could not be verified. Reconnect before starting another Java session.".to_owned();
                        self.language.restart_blocked = true;
                        self.close_after_language_stop = false;
                        self.close_snapshot = None;
                        self.language.output = message.clone();
                        self.error = Some(message);
                    }
                }
            } else {
                self.language.reset();
            }
            return;
        }
        if self.language.mode == ServerMode::Java {
            self.language.output = "Java session response received. Diagnostics and supported results are shown in their views.".into();
        } else {
            self.language.activity(&value);
        }
        match action.kind {
            ActionKind::Start => {
                self.language.running = true;
                self.language.capabilities = value
                    .get("initialize")
                    .and_then(|value| value.get("capabilities"))
                    .cloned()
                    .unwrap_or(Value::Null);
                self.language.java_diagnostics_refresh_supported = self.language.mode
                    == ServerMode::Java
                    && value
                        .get("initialize")
                        .and_then(|value| value.get("cedar_java_diagnostics_refresh"))
                        .and_then(Value::as_bool)
                        == Some(true);
                self.language.diagnostics_exited = false;
                self.language.next_events = 0.0;
                self.language.automatic = true;
                self.language.view = View::Problems;
                self.notice =
                    "Language server ready; matching open files synchronize automatically".into();
            }
            ActionKind::JavaStartBegin
            | ActionKind::JavaStartPoll { .. }
            | ActionKind::JavaStartCancel { .. } => {
                unreachable!("startup responses handled before language activation")
            }
            ActionKind::Stop => self.language.reset(),
            ActionKind::Sync {
                document,
                version,
                edit_version,
            } => {
                if self.documents.iter().any(|doc| doc.id == document) {
                    if let Some(uri) = value
                        .get("opened")
                        .or_else(|| value.get("changed"))
                        .and_then(Value::as_str)
                    {
                        self.language.closed_uris.remove(uri);
                        self.language.sync.acknowledge(
                            document,
                            Acknowledged {
                                version,
                                edit_version,
                                uri: uri.into(),
                            },
                        );
                    } else {
                        self.language.sync.fail(document, edit_version);
                        self.error =
                            Some("Language sync response did not identify its document".into());
                    }
                } else if let Some(uri) = value
                    .get("opened")
                    .or_else(|| value.get("changed"))
                    .and_then(Value::as_str)
                {
                    self.language.remember_closed_uri(uri.into());
                    self.language.diagnostics.files.remove(uri);
                }
            }
            ActionKind::Close { document } => {
                self.language.sync.close(document);
                if let Some(uri) = value.get("closed").and_then(Value::as_str) {
                    self.language.remember_closed_uri(uri.into());
                    self.language.diagnostics.files.remove(uri);
                }
            }
            ActionKind::Events => self.apply_language_events(&value),
            ActionKind::RefreshJavaDiagnostics { .. } => {
                unreachable!("refresh acknowledgements are handled before activity output")
            }
            ActionKind::Feature { request } => self.apply_language_feature(request, value),
            ActionKind::Query { context, kind } => {
                if self.language.features.has_request_or_preview()
                    || !self.query_is_current(&context)
                {
                    self.notice = "Stale language result ignored; your draft changed".into();
                    return;
                }
                self.tools_open = true;
                self.tool = crate::Tool::Language;
                match kind {
                    LanguageQueryKind::Completion => {
                        match completion::parse_completion_result(&value) {
                            Ok(results) => {
                                self.language.completions = Some(CompletionMenu {
                                    context,
                                    candidates: results.candidates,
                                    selected: 0,
                                    incomplete: results.is_incomplete,
                                    truncated: results.truncated,
                                });
                                self.language.completion_popup = true;
                                self.language.view = View::Completion;
                            }
                            Err(error) => self.error = Some(error),
                        }
                    }
                    LanguageQueryKind::Definition => {
                        match language_results::parse_definitions(&value) {
                            Ok(locations) => {
                                self.language.view = View::Definitions;
                                self.language.definitions = locations;
                                if self.language.definitions.len() == 1 {
                                    self.navigate_language(self.language.definitions[0].clone());
                                }
                            }
                            Err(error) => self.error = Some(error),
                        }
                    }
                    LanguageQueryKind::Hover => {
                        self.language.hover = language_results::hover_text(&value);
                        self.language.view = View::Hover;
                    }
                }
            }
            ActionKind::ResolveUri {
                sequence,
                navigation,
                location,
            } => {
                if sequence != self.language.navigation_sequence
                    || navigation != self.navigation_epoch
                {
                    return;
                }
                let Some(path) = value.get("path").and_then(Value::as_str) else {
                    self.error = Some("Agent did not return a workspace path".into());
                    return;
                };
                if !safe_relative_path(path) {
                    self.error = Some("Agent returned an unsafe navigation path; ignored".into());
                    return;
                }
                let path = path.to_owned();
                self.open(path.clone(), None);
                self.language.deferred_navigation.insert(
                    path.clone(),
                    DeferredNavigation {
                        session: self.language.session,
                        sequence: self.language.navigation_sequence,
                        navigation: self.navigation_epoch,
                        range: location.range,
                    },
                );
                self.complete_language_navigation(&path);
            }
            ActionKind::ResolveCompletion {
                context,
                original,
                acceptance,
            } => {
                if acceptance != self.language.acceptance_sequence {
                    self.notice = "Completion cancelled; nothing was applied".into();
                    return;
                }
                if let Err(error) = validate_resolved_identity(&original, &value) {
                    self.error = Some(error);
                    return;
                }
                self.apply_language_completion(context, value)
            }
        }
    }
    fn apply_language_events(&mut self, value: &Value) {
        let truncated = value.get("truncated").and_then(Value::as_bool) == Some(true);
        let Some(events) = value.get("events").and_then(Value::as_array) else {
            self.error = Some("Language event response has no event array".into());
            return;
        };
        if truncated
            || events
                .iter()
                .any(|event| event.get("type").and_then(Value::as_str) == Some("lagged"))
        {
            self.language.diagnostics.invalidate();
            self.language.diagnostic_refresh = None;
        }
        for event in events.iter().take(32) {
            match event.get("type").and_then(Value::as_str) {
                Some("diagnostics") => {
                    if let Some(value) = event.get("value") {
                        let normalized = self.normalize_known_java_diagnostic(value);
                        let value = normalized.as_ref();
                        if let Some(uri) = value.get("uri").and_then(Value::as_str) {
                            if self.language.closed_uris.contains(uri) {
                                continue;
                            }
                            if let Some(version) = value.get("version").and_then(Value::as_i64) {
                                if self
                                    .language
                                    .sync
                                    .opened
                                    .values()
                                    .any(|ack| ack.uri == uri && version < i64::from(ack.version))
                                {
                                    continue;
                                }
                            }
                        }
                        if let Err(error) = self.language.diagnostics.apply(value) {
                            self.reject_diagnostics_batch(Some(value));
                            self.language.paused_reason = Some(error);
                        } else {
                            self.observe_diagnostics_refresh(value);
                        }
                    } else {
                        self.reject_diagnostics_batch(None);
                        self.language.paused_reason = Some(
                            "Diagnostic publication has no batch; freshness could not be verified"
                                .into(),
                        );
                    }
                }
                Some("closed") => {
                    self.language.diagnostics_exited = true;
                    self.language.java_diagnostics_refresh_supported = false;
                    self.language.diagnostic_refresh = None;
                    self.language.features.reset();
                    self.language.automatic = false;
                    self.language.intent = None;
                    self.language.completions = None;
                    self.language.diagnostics.invalidate();
                    self.language.paused_reason =
                        Some("The language server exited. Stop this session and restart it".into());
                }
                _ => {}
            }
        }
    }
    fn document_query_is_current(&self, context: &QueryContext) -> bool {
        self.active_document == Some(context.document)
            && self.language.valid(context, &self.documents)
    }
    fn query_is_current(&self, context: &QueryContext) -> bool {
        self.document_query_is_current(context)
            && self
                .documents
                .iter()
                .find(|doc| doc.id == context.document)
                .is_some_and(|doc| {
                    utf16_position(&doc.text, doc.cursor).ok() == Some(context.cursor)
                })
    }
    fn navigate_language(&mut self, location: Location) {
        if !self.ready() || !self.language.running {
            return;
        }
        if !self.backend_supports("language_resolve_uri") {
            self.error = Some(self.unsupported_message("language_resolve_uri"));
            return;
        }
        // Even file:// strings are sent to the agent for remote-filesystem confinement.
        if !location.uri.starts_with("file:") {
            self.error = Some("This target is outside supported workspace files (for example a JDK archive). Cedar does not open external URLs or dependency archives".into());
            return;
        }
        self.navigation_changed();
        let sequence = self.language.navigation_sequence;
        let navigation = self.navigation_epoch;
        self.language_request(
            Operation::LanguageResolveUri {
                uri: location.uri.clone(),
            },
            ActionKind::ResolveUri {
                sequence,
                navigation,
                location,
            },
        );
    }
    pub(super) fn complete_language_navigation(&mut self, path: &str) {
        if !self.documents.iter().any(|doc| doc.path == path) {
            return;
        }
        let Some(navigation) = self.language.deferred_navigation.remove(path) else {
            return;
        };
        if navigation.session != self.language.session
            || navigation.sequence != self.language.navigation_sequence
            || navigation.navigation != self.navigation_epoch
        {
            return;
        }
        let Some(doc) = self.documents.iter_mut().find(|doc| doc.path == path) else {
            return;
        };
        let start = completion::position_to_offsets(&doc.text, navigation.range.start);
        let end = completion::position_to_offsets(&doc.text, navigation.range.end);
        match (start, end) {
            (Ok((_, start)), Ok((_, end))) if start <= end => {
                self.active_document = Some(doc.id); doc.scroll_to = Some(start);
                let id = egui::Id::new(("editor", doc.id));
                let mut state = egui::TextEdit::load_state(&self.editor_ctx, id).unwrap_or_default();
                state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(start), egui::text::CCursor::new(end))));
                state.store(&self.editor_ctx, id);
                self.editor_ctx.memory_mut(|memory| memory.request_focus(id));
            }
            _ => self.error = Some("The target range does not fit the current file. Its contents may have changed; no draft was modified".into()),
        }
    }
    fn completion_apply_supported(&self) -> bool {
        self.ready()
            && (self
                .language
                .capabilities
                .get("completionProvider")
                .and_then(|value| value.get("resolveProvider"))
                .and_then(Value::as_bool)
                != Some(true)
                || self.backend_supports("language_resolve_completion"))
    }
    fn accept_completion(&mut self, index: usize) {
        if self.language_busy() {
            return;
        }
        let Some(menu) = self.language.completions.as_ref() else {
            return;
        };
        if !self.query_is_current(&menu.context) {
            self.language.completions = None;
            self.language.completion_popup = false;
            self.error = Some(
                "Completion expired because the draft or cursor changed. Request completion again"
                    .into(),
            );
            return;
        }
        let Some(candidate) = menu.candidates.get(index) else {
            return;
        };
        if let Some(reason) = &candidate.disabled_reason {
            self.error = Some(reason.clone());
            return;
        }
        let context = menu.context.clone();
        let item = candidate.item.clone();
        self.language.acceptance_sequence = self.language.acceptance_sequence.wrapping_add(1);
        let acceptance = self.language.acceptance_sequence;
        if self
            .language
            .capabilities
            .get("completionProvider")
            .and_then(|value| value.get("resolveProvider"))
            .and_then(Value::as_bool)
            == Some(true)
        {
            if !self.backend_supports("language_resolve_completion") {
                self.error = Some(self.unsupported_message("language_resolve_completion"));
                return;
            }
            self.language_request(
                Operation::LanguageResolveCompletion { item: item.clone() },
                ActionKind::ResolveCompletion {
                    context,
                    original: item,
                    acceptance,
                },
            );
        } else {
            self.apply_language_completion(context, item);
        }
    }
    fn apply_language_completion(&mut self, context: QueryContext, item: Value) {
        if !self.query_is_current(&context) {
            self.error = Some(
                "The draft or cursor changed while completion was resolving. Nothing was applied"
                    .into(),
            );
            return;
        }
        let applied = match completion::apply_completion(&context.source, context.cursor, &item) {
            Ok(applied) => applied,
            Err(error) => {
                self.error = Some(format!("Completion not applied: {error}"));
                return;
            }
        };
        let Some(doc) = self
            .documents
            .iter_mut()
            .find(|doc| doc.id == context.document)
        else {
            return;
        };
        crate::editor_state::commit(&self.editor_ctx, doc, applied.text, applied.cursor_chars);
        self.language.completion_popup = false;
        self.language.completions = None;
        self.language.intent = None;
        self.language.view = View::Problems;
        self.notice = format!(
            "Applied {} completion edit{}; Ctrl/Cmd+Z undoes the whole change",
            applied.edit_count,
            if applied.edit_count == 1 { "" } else { "s" }
        );
        if applied.skipped_advisory {
            self.language.output = "Applied the validated Java text edits. Skipped java.completion.onDidSelect: selection-ranking feedback and automatic signature-help follow-up are unavailable. No server command was executed.".into();
        }
    }

    pub(super) fn language_panel(&mut self, ui: &mut egui::Ui) {
        let trusted = self.execution_trusted();
        let busy = self.language_busy();
        let starting = self.language.startup_active();
        let supported = self.backend_language_supported();
        if !supported {
            ui.colored_label(
                AMBER,
                self.unsupported_message("the complete language session lifecycle"),
            );
        }
        if !trusted {
            ui.colored_label(AMBER, "Language servers require trusted tool permission. Enable it in Open workspace and reconnect.");
        }
        let generic_supported = self.backend_generic_language_supported();
        let java_supported = self.backend_java_language_supported();
        if !self.language.running && !starting && !busy && !generic_supported && java_supported {
            self.language.mode = ServerMode::Java;
        }
        egui::CollapsingHeader::new("Server configuration").default_open(!self.language.running).show(ui, |ui| {
            ui.add_enabled_ui(trusted && supported && self.ready() && !self.language.running && !starting && !busy && !self.language.restart_blocked, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if generic_supported { ui.selectable_value(&mut self.language.mode, ServerMode::Generic, "Installed stdio server"); }
                    if java_supported { ui.selectable_value(&mut self.language.mode, ServerMode::Java, "Java / JDT LS"); }
                });
                match self.language.mode {
                    ServerMode::Generic => ui.horizontal_wrapped(|ui| {
                        ui.label("Executable"); ui.add(egui::TextEdit::singleline(&mut self.language.program).hint_text("kotlin-lsp / rust-analyzer").desired_width(245.0));
                        ui.label("Arguments (JSON)"); ui.add(egui::TextEdit::singleline(&mut self.language.args).desired_width(210.0));
                        ui.label("Language ID"); ui.add(egui::TextEdit::singleline(&mut self.language.language_id).desired_width(80.0));
                    }),
                    ServerMode::Java => ui.vertical(|ui| {
                        ui.label("Use existing paths on the workspace host:");
                        ui.horizontal(|ui| { ui.label("Java executable"); ui.add(egui::TextEdit::singleline(&mut self.language.java.executable).hint_text("Absolute ASCII path to java.exe").desired_width(390.0)); });
                        ui.horizontal(|ui| { ui.label("JDT distribution"); ui.add(egui::TextEdit::singleline(&mut self.language.java.distribution).hint_text("Existing Eclipse JDT LS directory").desired_width(390.0)); });
                        ui.horizontal(|ui| { ui.label("JDT data directory"); ui.add(egui::TextEdit::singleline(&mut self.language.java.data_directory).hint_text("Existing directory outside the workspace").desired_width(390.0)); });
                        ui.label("Language: Java. Maven and Gradle project imports are disabled; JDK class-file viewing is unavailable.");
                    }),
                };
                if ui.button("Start server").clicked() { self.start_language(); }
            });
            ui.label(RichText::new("One explicitly started server per workspace. The server and JDK must be installed on the workspace host.").small().color(MUTED));
        });
        self.java_startup_controls(ui);
        if busy && !starting {
            ui.label(RichText::new("This synchronous language request blocks queued workspace requests until it finishes. Stop becomes available afterward.").small().color(MUTED));
        }
        if self.language.mode == ServerMode::Java
            && !self.backend_java_startup_supported()
            && !self.language.running
        {
            ui.label(RichText::new("This agent uses blocking Java startup. Stop becomes available after startup finishes.").small().color(MUTED));
        }
        if self.language.restart_blocked {
            ui.colored_label(
                AMBER,
                "Reconnect before starting another Java session; prior cleanup was not verified.",
            );
        }
        if self.language.running {
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(
                    GREEN,
                    format!("{} server", self.language.document_language_id()),
                );
                ui.checkbox(&mut self.language.automatic, "Automatic sync + diagnostics");
                if ui
                    .add_enabled(!busy, egui::Button::new("Stop server"))
                    .clicked()
                {
                    self.stop_language();
                }
                let matching = self
                    .active()
                    .is_some_and(|doc| self.language.matches(&doc.path));
                if ui
                    .add_enabled(!busy && matching, egui::Button::new("Sync now"))
                    .clicked()
                {
                    self.sync_current_language();
                }
                let can_query = matching && self.backend_supports("language_query");
                if ui
                    .add_enabled(
                        can_query && self.language.supports("completionProvider"),
                        egui::Button::new("Complete"),
                    )
                    .on_hover_text("Ctrl+Space")
                    .clicked()
                {
                    self.request_language_feature(LanguageQueryKind::Completion);
                }
                if ui
                    .add_enabled(
                        can_query
                            && self.backend_supports("language_resolve_uri")
                            && self.language.supports("definitionProvider"),
                        egui::Button::new("Definition"),
                    )
                    .on_hover_text("F12")
                    .clicked()
                {
                    self.request_language_feature(LanguageQueryKind::Definition);
                }
                if ui
                    .add_enabled(
                        can_query && self.language.supports("hoverProvider"),
                        egui::Button::new("Hover"),
                    )
                    .on_hover_text("Ctrl/Cmd+K")
                    .clicked()
                {
                    self.request_language_feature(LanguageQueryKind::Hover);
                }
                if ui
                    .add_enabled(
                        !busy && self.backend_supports("language_events"),
                        egui::Button::new("Refresh events"),
                    )
                    .clicked()
                {
                    self.language_request(Operation::LanguageEvents, ActionKind::Events);
                }
            });
        }
        self.language_feature_controls(ui);
        self.java_diagnostics_refresh_controls(ui);
        if let Some(reason) = &self.language.paused_reason {
            ui.colored_label(AMBER, reason);
        }
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(
                &mut self.language.view,
                View::Problems,
                format!("Problems ({})", self.language.diagnostics.len()),
            );
            ui.selectable_value(&mut self.language.view, View::Completion, "Completion");
            ui.selectable_value(&mut self.language.view, View::Definitions, "Definitions");
            ui.selectable_value(&mut self.language.view, View::Hover, "Hover");
            ui.selectable_value(&mut self.language.view, View::Format, "Format");
            ui.selectable_value(&mut self.language.view, View::References, "References");
            ui.selectable_value(&mut self.language.view, View::Outline, "Outline");
            let activity_label = if self.language.mode == ServerMode::Java {
                "Session activity"
            } else {
                "Protocol details"
            };
            ui.selectable_value(&mut self.language.view, View::Activity, activity_label);
            if busy {
                ui.spinner();
            }
        });
        ui.separator();
        match self.language.view {
            View::Problems => self.problems_view(ui),
            View::Format => self.format_view(ui),
            View::References => self.references_view(ui),
            View::Outline => self.outline_view(ui),
            View::Definitions => {
                let mut selected = None;
                egui::ScrollArea::vertical()
                    .id_salt("definitions")
                    .show(ui, |ui| {
                        if self.language.definitions.is_empty() {
                            ui.label(
                                RichText::new("Place the cursor on a symbol, then press F12")
                                    .color(MUTED),
                            );
                        }
                        for location in &self.language.definitions {
                            if ui
                                .add_enabled(
                                    self.backend_supports("language_resolve_uri"),
                                    egui::Button::new(format!(
                                        "{}:{}:{}",
                                        location.uri,
                                        u64::from(location.range.start.line) + 1,
                                        u64::from(location.range.start.character) + 1
                                    )),
                                )
                                .clicked()
                            {
                                selected = Some(location.clone());
                            }
                        }
                    });
                if let Some(location) = selected {
                    self.navigate_language(location);
                }
            }
            View::Completion => {
                let enabled = !busy && self.completion_apply_supported();
                if !self.completion_apply_supported() {
                    ui.colored_label(
                        AMBER,
                        self.unsupported_message("language_resolve_completion"),
                    );
                }
                let selected =
                    completion_rows(ui, self.language.completions.as_mut(), enabled, 150.0);
                if let Some(index) = selected {
                    self.accept_completion(index);
                }
            }
            View::Hover => {
                egui::ScrollArea::both().id_salt("hover").show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut self.language.hover)
                            .interactive(false)
                            .frame(false)
                            .desired_width(f32::INFINITY),
                    );
                });
            }
            View::Activity => {
                egui::ScrollArea::both()
                    .id_salt("language_output")
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut self.language.output)
                                .font(egui::TextStyle::Monospace)
                                .interactive(false)
                                .frame(false)
                                .desired_width(f32::INFINITY),
                        );
                    });
            }
        }
    }
    fn problems_view(&mut self, ui: &mut egui::Ui) {
        let status = self.active_diagnostics_status();
        ui.colored_label(if status.current { GREEN } else { AMBER }, status.message);
        if self.language.diagnostics.incomplete {
            ui.colored_label(AMBER, "Some language events were lost or exceeded limits. This problem list may be incomplete; resync or restart to refresh it.");
        }
        let mut selected = None;
        egui::ScrollArea::vertical()
            .id_salt("diagnostics")
            .show(ui, |ui| {
                if self.language.diagnostics.len() == 0 {
                    ui.label(
                        RichText::new(if self.language.running {
                            "No problem rows to display. See the active document's diagnostic status above."
                        } else {
                            "Start a language server to see project diagnostics"
                        })
                        .color(MUTED),
                    );
                }
                for (uri, batch) in &self.language.diagnostics.files {
                    let known = self
                        .language
                        .sync
                        .opened
                        .iter()
                        .find(|(_, ack)| ack.uri == *uri);
                    let doc =
                        known.and_then(|(id, _)| self.documents.iter().find(|doc| doc.id == *id));
                    let current = self.ready() && self.language.running && !self.language.diagnostics_exited && known.zip(doc).is_some_and(|((_, ack), doc)| {
                        batch.version == Some(ack.version) && doc.edit_version == ack.edit_version
                    });
                    let freshness = if batch.version.is_none() {
                        "unversioned server result"
                    } else if current {
                        "current draft"
                    } else {
                        "older or unopened snapshot"
                    };
                    let name = doc.map_or(uri.as_str(), |doc| doc.path.as_str());
                    for diagnostic in &batch.items {
                        let (severity, color) = match diagnostic.severity {
                            1 => ("Error", RED),
                            2 => ("Warning", AMBER),
                            4 => ("Hint", MUTED),
                            _ => ("Info", GREEN),
                        };
                        ui.horizontal(|ui| {
                            ui.colored_label(color, severity);
                            let response =
                                ui.add_enabled(
                                    self.backend_supports("language_resolve_uri"),
                                    egui::Button::new(
                                        RichText::new(format!(
                                            "{name}:{}",
                                            u64::from(diagnostic.range.start.line) + 1
                                        ))
                                        .color(if current { GREEN } else { MUTED }),
                                    )
                                    .frame(false),
                                );
                            if response
                                .on_hover_text(format!("{freshness}\n{}", diagnostic.source))
                                .clicked()
                            {
                                selected = Some(Location {
                                    uri: uri.clone(),
                                    range: diagnostic.range,
                                });
                            }
                            ui.add(egui::Label::new(&diagnostic.message).truncate())
                                .on_hover_text(format!("{}\n{freshness}", diagnostic.message));
                            if !current {
                                ui.label(
                                    RichText::new(if batch.version.is_none() {
                                        "unversioned"
                                    } else {
                                        "stale"
                                    })
                                    .small()
                                    .color(MUTED),
                                );
                            }
                        });
                    }
                }
            });
        if let Some(location) = selected {
            self.navigate_language(location);
        }
    }
    pub(super) fn language_popups(&mut self, ctx: &egui::Context) {
        self.format_preview_window(ctx);
        if !self.language.completion_popup {
            return;
        }
        if self
            .language
            .completions
            .as_ref()
            .is_none_or(|menu| !self.query_is_current(&menu.context))
        {
            self.language.completion_popup = false;
            self.language.completions = None;
            return;
        }
        let busy = self.language_busy();
        let mut visible = true;
        let enabled = !busy && self.completion_apply_supported();
        let mut selected = None;
        egui::Window::new("Completion")
            .id(egui::Id::new("completion_menu"))
            .open(&mut visible)
            .collapsible(false)
            .resizable(false)
            .default_width(620.0)
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 105.0))
            .show(ctx, |ui| {
                selected = completion_rows(ui, self.language.completions.as_mut(), enabled, 290.0);
                if !self.completion_apply_supported() {
                    ui.colored_label(
                        AMBER,
                        self.unsupported_message("language_resolve_completion"),
                    );
                }
                if busy {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Resolving imports and validating the complete change...");
                    });
                }
            });
        if !visible {
            self.language.completion_popup = false;
            self.language.acceptance_sequence = self.language.acceptance_sequence.wrapping_add(1);
        }
        if let Some(index) = selected {
            self.accept_completion(index);
        }
    }
    pub(super) fn language_shortcuts(&mut self, ctx: &egui::Context) {
        // Consume modal Escape before the app-wide Escape handler.
        if self.format_preview_shortcut(ctx) {
            return;
        }
        if self.language.completion_popup {
            if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
                self.language.completion_popup = false;
                self.language.acceptance_sequence =
                    self.language.acceptance_sequence.wrapping_add(1);
                return;
            }
            if let Some(menu) = self.language.completions.as_mut() {
                let count = menu.candidates.len();
                if count > 0 {
                    if ctx.input_mut(|input| {
                        input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)
                    }) {
                        menu.selected = (menu.selected + 1) % count;
                    }
                    if ctx.input_mut(|input| {
                        input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp)
                    }) {
                        menu.selected = (menu.selected + count - 1) % count;
                    }
                    if ctx.input_mut(|input| {
                        input.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                    }) {
                        let index = menu.selected;
                        self.accept_completion(index);
                    }
                }
            }
        }
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::Space)) {
            self.request_language_feature(LanguageQueryKind::Completion);
        }
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F12)) {
            self.request_language_feature(LanguageQueryKind::Definition);
        }
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::K)) {
            self.request_language_feature(LanguageQueryKind::Hover);
        }
    }
}

fn completion_rows(
    ui: &mut egui::Ui,
    menu: Option<&mut CompletionMenu>,
    enabled: bool,
    height: f32,
) -> Option<usize> {
    let Some(menu) = menu else {
        ui.label(
            RichText::new("Press Ctrl+Space in a synchronized file for completions").color(MUTED),
        );
        return None;
    };
    let mut selected = None;
    if menu.incomplete || menu.truncated {
        ui.colored_label(
            AMBER,
            "Results are incomplete or capped. Type more, then request completion again",
        );
    }
    if menu.candidates.is_empty() {
        ui.label("No completion candidates at this position");
    }
    egui::ScrollArea::vertical()
        .id_salt("completion_rows")
        .max_height(height)
        .show(ui, |ui| {
            for (index, candidate) in menu.candidates.iter().enumerate() {
                ui.horizontal(|ui| {
                    let allowed = enabled && candidate.disabled_reason.is_none();
                    let response = ui.add_enabled(
                        allowed,
                        egui::Button::new(RichText::new(&candidate.label).monospace())
                            .selected(menu.selected == index)
                            .min_size(egui::vec2(210.0, 24.0)),
                    );
                    if response.clicked() {
                        menu.selected = index;
                    }
                    if response.double_clicked() {
                        selected = Some(index);
                    }
                    if let Some(reason) = &candidate.disabled_reason {
                        response.on_hover_text(reason);
                        ui.add(
                            egui::Label::new(RichText::new(reason).small().color(AMBER)).truncate(),
                        );
                    } else if let Some(detail) = &candidate.detail {
                        ui.add(
                            egui::Label::new(RichText::new(detail).small().color(MUTED)).truncate(),
                        );
                    }
                });
            }
        });
    ui.horizontal(|ui| {
        let allowed = enabled
            && menu
                .candidates
                .get(menu.selected)
                .is_some_and(|candidate| candidate.disabled_reason.is_none());
        if ui
            .add_enabled(allowed, egui::Button::new("Apply selected  ·  Enter"))
            .clicked()
        {
            selected = Some(menu.selected);
        }
        ui.label(
            RichText::new("One undo step · changes stay unsaved")
                .small()
                .color(MUTED),
        );
    });
    ui.label(RichText::new("Commands never run. Java’s selection callback is skipped; ranking feedback and automatic signature-help follow-up are unavailable.").small().color(MUTED));
    selected
}

fn utf16_position(text: &str, cursor: (usize, usize)) -> Result<Position, String> {
    if cursor.0 == 0 || cursor.1 == 0 {
        return Err("Invalid editor cursor".into());
    }
    let scalar = crate::model::line_start(text, cursor.0)
        .checked_add(cursor.1 - 1)
        .ok_or("Editor cursor overflow")?;
    completion::chars_to_position(text, scalar)
}
fn validate_resolved_identity(original: &Value, resolved: &Value) -> Result<(), String> {
    if !resolved.is_object() {
        return Err(
            "The server returned an invalid resolved completion; nothing was applied".into(),
        );
    }
    for key in [
        "label",
        "sortText",
        "filterText",
        "insertText",
        "insertTextFormat",
        "insertTextMode",
        "textEdit",
        "commitCharacters",
    ] {
        if let Some(expected) = original.get(key) {
            if resolved.get(key) != Some(expected) {
                return Err(format!(
                    "The server changed completion {key} during resolution; nothing was applied"
                ));
            }
        }
    }
    Ok(())
}

fn safe_relative_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains(['\\', ':', '\0'])
        && path
            .split('/')
            .all(|component| !component.is_empty() && component != "." && component != "..")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn file_chooser_keys_take_priority_over_populated_completion_popup() {
        let (mut app, commands) = capability_app();
        app.open_form = false;
        app.language.running = true;
        app.language.automatic = false;
        app.language.capabilities = serde_json::json!({"completionProvider": {}});
        app.documents.push(Document::new(
            1,
            "first.rs".into(),
            "draft".into(),
            "r0".into(),
        ));
        app.documents.push(Document::new(
            2,
            "second.rs".into(),
            "other".into(),
            "r1".into(),
        ));
        app.active_document = Some(1);
        app.language.completions = Some(CompletionMenu {
            context: QueryContext {
                session: app.language.session,
                document: 1,
                edit_version: 0,
                source: "draft".into(),
                cursor: Position::default(),
            },
            candidates: completion::parse_completion_result(&serde_json::json!([
                {"label": "one", "insertText": "one"},
                {"label": "two", "insertText": "two"}
            ]))
            .unwrap()
            .candidates,
            selected: 0,
            incomplete: false,
            truncated: false,
        });
        app.language.completion_popup = true;
        let mut events = Vec::new();
        for (key, modifiers) in [
            (egui::Key::P, egui::Modifiers::COMMAND),
            (egui::Key::ArrowDown, egui::Modifiers::NONE),
            (egui::Key::Enter, egui::Modifiers::NONE),
        ] {
            for pressed in [true, false] {
                events.push(egui::Event::Key {
                    key,
                    physical_key: Some(key),
                    pressed,
                    repeat: false,
                    modifiers,
                });
            }
        }
        let ctx = app.editor_ctx.clone();
        let mut frame = eframe::Frame::_new_kittest();
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(780.0, 540.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| eframe::App::update(&mut app, ctx, &mut frame),
        );
        assert_eq!(app.active_document, Some(2));
        assert_eq!(app.documents[0].text, "draft");
        assert_eq!(app.documents[1].text, "other");
        assert_eq!(app.language.completions.as_ref().unwrap().selected, 0);
        assert!(app.documents.iter().all(|doc| doc.edit_version == 0));
        assert!(commands.try_recv().is_err());
    }

    #[test]
    fn disk_reload_uses_existing_language_debounce_and_preserves_sync_mode() {
        for automatic in [false, true] {
            let mut app = CedarApp::empty();
            app.state = crate::ConnectionState::Ready;
            app.agent_info = Some(crate::agent_support::full_test_agent());
            app.documents.push(Document::new(
                1,
                "file.rs".into(),
                "before".into(),
                "a".repeat(64),
            ));
            app.active_document = Some(1);
            let (worker, rx) = crate::worker::Worker::recording();
            app.worker = Some(worker);
            app.language.running = true;
            app.language.session = 4;
            app.language.automatic = automatic;
            app.language.next_events = f64::INFINITY;
            app.language.sync.observe(1, 0, 0.0);
            app.language.sync.acknowledge(
                1,
                Acknowledged {
                    version: 17,
                    edit_version: 0,
                    uri: "file:///project/file.rs".into(),
                },
            );
            app.language.completions = Some(CompletionMenu {
                context: QueryContext {
                    session: 4,
                    document: 1,
                    edit_version: 0,
                    source: "before".into(),
                    cursor: Position {
                        line: 0,
                        character: 0,
                    },
                },
                candidates: vec![],
                selected: 0,
                incomplete: false,
                truncated: false,
            });
            app.language.completion_popup = true;
            for verify in [false, true] {
                if verify {
                    app.reload_from_disk();
                } else {
                    app.compare_with_disk();
                }
                let command = rx.try_recv().unwrap();
                assert!(matches!(command.op, Operation::Read { .. }));
                app.apply_event(crate::Event {
                    generation: app.generation,
                    id: command.id,
                    connected: true,
                    result: Ok(crate::Payload::File {
                        path: "file.rs".into(),
                        text: "after".into(),
                        revision: "b".repeat(64),
                    }),
                });
            }
            let ctx = app.editor_ctx.clone();
            let _ = ctx.run(
                egui::RawInput {
                    time: Some(10.0),
                    ..Default::default()
                },
                |ctx| {
                    app.finish_disk_reload(ctx);
                    app.language_tick(ctx);
                },
            );
            assert_eq!(app.language.automatic, automatic);
            assert_eq!(app.language.session, 4);
            assert!(app.language.running);
            assert_eq!(app.documents[0].edit_version, 1);
            assert!(app.language.completions.is_none());
            assert!(!app.language.completion_popup);
            assert_eq!(app.language.sync.next_version(1), Some(18));
            assert!(!app.language.sync.synced(1, 1));
            assert!((app.language.sync.deadline(1, 1).unwrap() - 10.35).abs() < 0.00001);
            assert!(rx.try_recv().is_err());
            let _ = ctx.run(
                egui::RawInput {
                    time: Some(10.34),
                    ..Default::default()
                },
                |ctx| app.language_tick(ctx),
            );
            assert!(rx.try_recv().is_err());
            let _ = ctx.run(
                egui::RawInput {
                    time: Some(10.36),
                    ..Default::default()
                },
                |ctx| app.language_tick(ctx),
            );
            if automatic {
                let command = rx.try_recv().unwrap();
                assert!(
                    matches!(command.op, Operation::LanguageChange { version: 18, text, .. } if text == "after")
                );
            } else {
                assert!(rx.try_recv().is_err());
            }
        }
    }
    #[test]
    fn converts_cursor_to_utf16() {
        assert_eq!(
            utf16_position("first\nA🐻é.end", (2, 4)).unwrap(),
            Position {
                line: 1,
                character: 4
            }
        );
    }
    #[test]
    fn invalid_crlf_cursor_does_not_fall_back_to_document_start() {
        assert!(utf16_position("abc\r\n", (1, 5)).is_err());
        assert!(utf16_position("abc", (1, 99)).is_err());
    }
    #[test]
    fn snapshot_check_rejects_newer_text_and_restart() {
        let mut panel = LanguagePanel {
            running: true,
            session: 5,
            ..Default::default()
        };
        let mut doc = Document::new(1, "A.java".into(), "class A {}".into(), "r".into());
        let context = QueryContext {
            session: 5,
            document: 1,
            edit_version: 0,
            source: doc.text.clone(),
            cursor: Position {
                line: 0,
                character: 0,
            },
        };
        assert!(panel.valid(&context, std::slice::from_ref(&doc)));
        doc.edit_version += 1;
        assert!(!panel.valid(&context, std::slice::from_ref(&doc)));
        doc.edit_version = 0;
        panel.reset();
        assert!(!panel.valid(&context, &[doc]));
    }
    #[test]
    fn language_profiles_do_not_cross_streams() {
        let panel = LanguagePanel {
            language_id: "java".into(),
            ..Default::default()
        };
        assert!(panel.matches("src/A.java"));
        assert!(!panel.matches("src/lib.rs"));
        assert!(!panel.matches("Main.kt"));
    }
    #[test]
    fn completion_is_exactly_one_undo_and_redo_step() {
        let ctx = egui::Context::default();
        let mut doc = Document::new(4, "A.java".into(), "old draft".into(), "r".into());
        crate::editor_state::commit(&ctx, &mut doc, "import X;\nnew draft".into(), 13);
        let state = egui::TextEdit::load_state(&ctx, egui::Id::new(("editor", 4u64))).unwrap();
        let after = (state.cursor.char_range().unwrap(), doc.text.clone());
        let mut undo = state.undoer();
        let before = undo.undo(&after).unwrap().clone();
        assert_eq!(before.1, "old draft");
        assert_eq!(undo.redo(&before).unwrap().1, "import X;\nnew draft");
        assert!(doc.dirty());
        assert_eq!(doc.edit_version, 1);
    }
    #[test]
    fn resolve_can_add_imports_and_remove_data_but_cannot_substitute_item() {
        let original = serde_json::json!({"label":"Thing","insertText":"Thing","data":{"token":1}});
        let resolved =
            serde_json::json!({"label":"Thing","insertText":"Thing","additionalTextEdits":[]});
        assert!(validate_resolved_identity(&original, &resolved).is_ok());
        let replacement = serde_json::json!({"label":"Other","insertText":"Thing"});
        assert!(validate_resolved_identity(&original, &replacement).is_err());
    }
    fn capability_app() -> (CedarApp, std::sync::mpsc::Receiver<crate::worker::Command>) {
        let mut app = CedarApp::empty();
        app.state = crate::ConnectionState::Ready;
        app.agent_info = Some(crate::agent_support::full_test_agent());
        app.active_form = Some(crate::ConnectForm {
            allow_run: true,
            ..Default::default()
        });
        app.language.program = "language-server".into();
        let (worker, rx) = crate::worker::Worker::recording();
        app.worker = Some(worker);
        (app, rx)
    }
    #[test]
    fn language_start_requires_core_lifecycle_and_trust_without_resetting_rejected_session() {
        for missing in cedar_protocol::LANGUAGE_SESSION_CAPABILITIES {
            let (mut app, rx) = capability_app();
            app.agent_info
                .as_mut()
                .unwrap()
                .capabilities
                .retain(|name| name != missing);
            let session = app.language.session;
            app.start_language();
            assert_eq!(app.language.session, session);
            assert!(rx.try_recv().is_err());
        }
        let (mut app, rx) = capability_app();
        app.active_form.as_mut().unwrap().allow_run = false;
        app.start_language();
        assert_eq!(app.language.session, 0);
        assert!(rx.try_recv().is_err());
    }
    #[test]
    fn missing_optional_query_or_navigation_does_not_disable_core_language_session() {
        let (mut app, rx) = capability_app();
        app.agent_info
            .as_mut()
            .unwrap()
            .capabilities
            .retain(|name| {
                !matches!(
                    name.as_str(),
                    "language_query" | "language_resolve_uri" | "language_resolve_completion"
                )
            });
        assert!(app.backend_language_supported());
        app.start_language();
        assert!(matches!(
            rx.try_recv().unwrap().op,
            Operation::LanguageStart { .. }
        ));
        app.pending.clear();
        app.language.running = true;
        app.language.capabilities =
            serde_json::json!({"completionProvider":{},"definitionProvider":true});
        app.documents.push(Document::new(
            1,
            "main.rs".into(),
            "text".into(),
            "r".into(),
        ));
        app.active_document = Some(1);
        app.request_language_feature(LanguageQueryKind::Completion);
        assert!(app.language.intent.is_none());
        let navigation = app.navigation_epoch;
        app.navigate_language(Location {
            uri: "file:///workspace/main.rs".into(),
            range: Range::default(),
        });
        assert_eq!(app.navigation_epoch, navigation);
        assert!(rx.try_recv().is_err());
        app.active_form.as_mut().unwrap().allow_run = false;
        app.stop_language();
        assert!(matches!(rx.try_recv().unwrap().op, Operation::LanguageStop));
    }
    #[test]
    fn missing_completion_resolve_never_falls_back_to_direct_apply() {
        let (mut app, rx) = capability_app();
        app.agent_info
            .as_mut()
            .unwrap()
            .capabilities
            .retain(|name| name != "language_resolve_completion");
        app.language.running = true;
        app.language.capabilities =
            serde_json::json!({"completionProvider":{"resolveProvider":true}});
        app.documents
            .push(Document::new(1, "main.rs".into(), "x".into(), "r".into()));
        app.active_document = Some(1);
        let context = QueryContext {
            session: app.language.session,
            document: 1,
            edit_version: 0,
            source: "x".into(),
            cursor: Position::default(),
        };
        app.language.completions = Some(CompletionMenu {
            context,
            candidates: completion::parse_completion_result(
                &serde_json::json!([{"label":"replacement","insertText":"replacement"}]),
            )
            .unwrap()
            .candidates,
            selected: 0,
            incomplete: false,
            truncated: false,
        });
        assert!(!app.completion_apply_supported());
        app.accept_completion(0);
        assert_eq!(app.documents[0].text, "x");
        assert_eq!(app.documents[0].edit_version, 0);
        assert!(rx.try_recv().is_err());
        assert!(app
            .error
            .as_ref()
            .unwrap()
            .contains("language_resolve_completion"));
    }
    #[test]
    fn explicit_queries_preserve_disabled_automatic_updates() {
        let mut app = CedarApp::empty();
        app.state = crate::ConnectionState::Ready;
        app.agent_info = Some(crate::agent_support::full_test_agent());
        app.language.running = true;
        app.language.automatic = false;
        app.language.capabilities = serde_json::json!({"completionProvider":{}});
        app.documents
            .push(Document::new(1, "main.rs".into(), "x".into(), "r".into()));
        app.active_document = Some(1);
        app.request_language_feature(LanguageQueryKind::Completion);
        assert!(app.language.intent.is_some());
        assert!(!app.language.automatic);
    }
    #[test]
    fn cancelled_completion_resolve_does_not_apply() {
        let mut app = CedarApp::empty();
        app.language.running = true;
        app.language.session = 4;
        app.language.acceptance_sequence = 2;
        let mut doc = Document::new(1, "Main.java".into(), "ab".into(), "r".into());
        doc.cursor = (1, 2);
        app.documents.push(doc);
        app.active_document = Some(1);
        let item = serde_json::json!({"label":"abc", "insertText":"abc"});
        let context = QueryContext {
            session: 4,
            document: 1,
            edit_version: 0,
            source: "ab".into(),
            cursor: Position {
                line: 0,
                character: 1,
            },
        };
        app.apply_language_action(
            Action {
                session: 4,
                kind: ActionKind::ResolveCompletion {
                    context,
                    original: item.clone(),
                    acceptance: 1,
                },
            },
            item,
        );
        assert_eq!(app.documents[0].text, "ab");
        assert_eq!(app.documents[0].edit_version, 0);
    }
    #[test]
    fn closed_and_older_incarnation_diagnostics_cannot_poison_reopened_file() {
        let uri = "file:///workspace/Main.java";
        let mut app = CedarApp::empty();
        app.language.running = true;
        app.documents
            .push(Document::new(1, "Main.java".into(), "x".into(), "r".into()));
        app.language.sync.acknowledge(
            1,
            Acknowledged {
                version: 10,
                edit_version: 0,
                uri: uri.into(),
            },
        );
        let event = |version| serde_json::json!({"events":[{"type":"diagnostics","value":{"uri":uri,"version":version,"diagnostics":[{"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":1}},"message":"problem"}]}}],"truncated":false});
        app.apply_language_events(&event(10));
        assert_eq!(app.language.diagnostics.len(), 1);
        app.close_language_document(1);
        app.documents.clear();
        app.apply_language_events(&event(10));
        assert_eq!(app.language.diagnostics.len(), 0);
        app.documents
            .push(Document::new(2, "Main.java".into(), "x".into(), "r".into()));
        app.apply_language_action(
            Action {
                session: 0,
                kind: ActionKind::Sync {
                    document: 2,
                    version: 11,
                    edit_version: 0,
                },
            },
            serde_json::json!({"opened":uri,"version":11}),
        );
        app.apply_language_events(&event(10));
        assert_eq!(app.language.diagnostics.len(), 0);
        app.apply_language_events(&event(11));
        assert_eq!(app.language.diagnostics.len(), 1);
        assert_eq!(app.language.diagnostics.files[uri].version, Some(11));
    }
    #[test]
    fn unsafe_navigation_paths_are_rejected() {
        for path in [
            "/etc/passwd",
            "../secret",
            "a/../b",
            "a//b",
            "C:/file",
            "file:///x",
            "a\\b",
        ] {
            assert!(!safe_relative_path(path));
        }
        assert!(safe_relative_path("src/你好.java"));
    }
    fn java_app() -> (CedarApp, std::sync::mpsc::Receiver<crate::worker::Command>) {
        let (mut app, rx) = capability_app();
        let info = app.agent_info.as_mut().unwrap();
        info.os = "windows".into();
        info.capabilities.retain(|name| name != "language_start");
        info.capabilities.push("language_start_java".into());
        app.language.mode = ServerMode::Java;
        app.language.java = JavaConfiguration {
            executable: r"C:\Java\bin\java.exe".into(),
            distribution: r"D:\JDT 雪".into(),
            data_directory: r"D:\Java data 雪".into(),
        };
        (app, rx)
    }
    #[test]
    fn java_start_requires_the_scoped_lifecycle_and_unchanged_execution_trust() {
        for missing in cedar_protocol::JAVA_LANGUAGE_SESSION_CAPABILITIES {
            let (mut app, rx) = java_app();
            app.agent_info
                .as_mut()
                .unwrap()
                .capabilities
                .retain(|name| name != missing);
            app.start_language();
            assert!(rx.try_recv().is_err());
            assert_eq!(app.language.session, 0);
        }
        let (mut app, rx) = java_app();
        app.active_form.as_mut().unwrap().allow_run = false;
        app.start_language();
        assert!(!app.execution_trusted());
        assert!(rx.try_recv().is_err());
        assert_eq!(app.language.session, 0);
        let (mut app, rx) = java_app();
        assert!(app.backend_java_language_supported());
        assert!(!app.backend_generic_language_supported());
        assert!(app
            .operation_problem(&Operation::LanguageStart {
                program: "must-not-run".into(),
                args: vec![]
            })
            .is_some());
        app.start_language();
        assert!(
            matches!(rx.try_recv().unwrap().op, Operation::LanguageStartJava { java_executable,distribution,data_directory } if java_executable == r"C:\Java\bin\java.exe" && distribution == r"D:\JDT 雪" && data_directory == r"D:\Java data 雪")
        );
    }
    #[test]
    fn java_mode_uses_only_java_documents_even_for_explicit_sync() {
        let (mut app, rx) = java_app();
        app.language.running = true;
        app.language.language_id = "rust".into();
        app.documents.push(Document::new(
            1,
            "other.rs".into(),
            "fn main() {}".into(),
            "r".into(),
        ));
        app.documents.push(Document::new(
            2,
            "Main.java".into(),
            "class Main {}".into(),
            "r".into(),
        ));
        app.sync_document(1);
        assert!(rx.try_recv().is_err());
        app.sync_document(2);
        assert!(
            matches!(rx.try_recv().unwrap().op, Operation::LanguageOpen { path,language_id,.. } if path == "Main.java" && language_id == "java")
        );
    }
    #[test]
    fn java_stop_shows_bounded_outcome_and_unknown_cleanup_blocks_restart() {
        let (mut app, _) = java_app();
        app.language.running = true;
        let stop = serde_json::json!({"stopped":true,"shutdown":{"status":"forced","reason":"grace_expired","root_exit_code":1,"cleanup_joined":true,"shutdown_response_received":true,"exit_frame_completed":true},"private":"private stderr"});
        app.apply_language_action(
            Action {
                session: 0,
                kind: ActionKind::Stop,
            },
            stop,
        );
        assert!(!app.language.running);
        assert!(app.notice.contains("forced cleanup"));
        assert!(!app.language.output.contains("private"));
        assert!(!app.language.restart_blocked);
        let session = app.language.session;
        app.language.running = true;
        app.close_after_language_stop = true;
        app.close_snapshot = Some(vec![]);
        app.apply_language_action(
            Action {
                session,
                kind: ActionKind::Stop,
            },
            serde_json::json!({"stopped":true}),
        );
        assert!(!app.language.running);
        assert!(app.language.restart_blocked);
        assert!(!app.close_after_language_stop);
        assert!(app.close_snapshot.is_none());
        app.finish_pending_close(&egui::Context::default());
        assert!(!app.allow_close);
        assert!(app
            .error
            .as_ref()
            .unwrap()
            .contains("could not be verified"));
    }
    #[test]
    fn java_activity_and_errors_never_render_arbitrary_server_payloads() {
        let (mut app, _) = java_app();
        app.apply_language_action(
            Action {
                session: 0,
                kind: ActionKind::Start,
            },
            serde_json::json!({"initialize":{"capabilities":{}},"private":"private payload"}),
        );
        assert!(!app.language.output.contains("private"));
        let error = app.language_public_error(
            &Action {
                session: 0,
                kind: ActionKind::Stop,
            },
            "raw private stderr",
        );
        assert!(!error.contains("private"));
        assert!(error.contains("could not be verified"));
    }
}

#[cfg(test)]
#[path = "real_java_tests.rs"]
mod real_java_tests;
