//! Explicit typed Java refresh. A notification acknowledgement is never diagnostic evidence.
use super::*;
#[path = "java_diagnostic_uri.rs"]
mod uri;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RefreshContext {
    generation: u64,
    session: u64,
    sequence: u64,
    pub(super) document: u64,
    path: String,
    edit_version: u64,
    version: i32,
    uri: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Observation {
    Awaiting,
    MatchingVersion,
    Unversioned,
}

pub(super) struct RefreshState {
    pub(super) context: RefreshContext,
    acknowledged: bool,
    observation: Observation,
}

pub(super) struct DiagnosticStatus {
    pub current: bool,
    pub message: String,
}

impl CedarApp {
    fn java_diagnostics_refresh_problem(&self) -> Option<String> {
        if !self.ready() || !self.language.running || self.language.diagnostics_exited {
            return Some("Start a Java / JDT LS session for this connection first".into());
        }
        if !self.execution_trusted() {
            return Some(
                "Java diagnostic refresh requires trusted tool permission for this connection"
                    .into(),
            );
        }
        if self.language.mode != ServerMode::Java {
            return Some("Diagnostic refresh requires a typed Java / JDT LS session".into());
        }
        if !self.backend_supports("java_diagnostics_refresh") {
            return Some(self.unsupported_message("java_diagnostics_refresh"));
        }
        if !self.language.java_diagnostics_refresh_supported {
            return Some("Diagnostic refresh is unavailable for this JDT LS session; a supported Standard server version was not confirmed".into());
        }
        let Some(doc) = self.active() else {
            return Some("Open a Java document first".into());
        };
        if !self.language.matches(&doc.path) {
            return Some("Select a Java document to refresh its diagnostics".into());
        }
        if !self.language.sync.synced(doc.id, doc.edit_version) {
            return Some("Use Sync now first and wait for this draft to synchronize".into());
        }
        if self.close_after_language_stop || self.recovery.closing.is_some() {
            return Some("Finish closing the current session first".into());
        }
        if self.language_busy() {
            return Some("Wait for the current language request to finish".into());
        }
        None
    }

    pub(crate) fn java_diagnostics_operation_problem(
        &self,
        path: &str,
        version: i32,
    ) -> Option<String> {
        if let Some(problem) = self.java_diagnostics_refresh_problem() {
            return Some(problem);
        }
        let doc = self
            .active()
            .expect("refresh guard checked the active document");
        let ack = &self.language.sync.opened[&doc.id];
        if path != doc.path || version != ack.version || version <= 0 {
            return Some(
                "Java diagnostic refresh requires the active document's exact synchronized version"
                    .into(),
            );
        }
        None
    }

    pub(super) fn request_java_diagnostics_refresh(&mut self) {
        if let Some(problem) = self.java_diagnostics_refresh_problem() {
            self.error = Some(problem);
            return;
        }
        let doc = self
            .active()
            .expect("refresh guard checked the active document");
        let ack = &self.language.sync.opened[&doc.id];
        let context = RefreshContext {
            generation: self.generation,
            session: self.language.session,
            sequence: self.language.diagnostic_refresh_sequence.wrapping_add(1),
            document: doc.id,
            path: doc.path.clone(),
            edit_version: doc.edit_version,
            version: ack.version,
            uri: ack.uri.clone(),
        };
        let id = self.language_request(
            Operation::LanguageRefreshJavaDiagnostics {
                path: context.path.clone(),
                version: context.version,
            },
            ActionKind::RefreshJavaDiagnostics {
                context: context.clone(),
            },
        );
        if id != 0 {
            self.language.diagnostic_refresh_sequence = context.sequence;
            self.language.diagnostic_refresh = Some(RefreshState {
                context,
                acknowledged: false,
                observation: Observation::Awaiting,
            });
            self.language.view = View::Problems;
        }
    }

    pub(crate) fn java_diagnostics_refresh_is_current(&self, context: &RefreshContext) -> bool {
        self.ready()
            && self.language.running
            && !self.language.diagnostics_exited
            && self.generation == context.generation
            && self.language.session == context.session
            && self
                .language
                .diagnostic_refresh
                .as_ref()
                .is_some_and(|refresh| refresh.context == *context)
            && self.documents.iter().any(|doc| {
                doc.id == context.document
                    && doc.path == context.path
                    && doc.edit_version == context.edit_version
                    && self.language.sync.opened.get(&doc.id).is_some_and(|ack| {
                        ack.version == context.version
                            && ack.edit_version == context.edit_version
                            && ack.uri == context.uri
                    })
            })
    }

    pub(super) fn invalidate_diagnostics_refresh(&mut self) {
        if self
            .language
            .diagnostic_refresh
            .as_ref()
            .is_some_and(|refresh| !self.java_diagnostics_refresh_is_current(&refresh.context))
        {
            self.language.diagnostic_refresh = None;
        }
    }

    pub(super) fn apply_java_diagnostics_refresh(
        &mut self,
        context: &RefreshContext,
        value: &Value,
    ) {
        if !self.java_diagnostics_refresh_is_current(context) {
            return;
        }
        if value
            .get("diagnostics_refresh_requested")
            .and_then(Value::as_str)
            != Some(context.uri.as_str())
            || value.get("version").and_then(Value::as_i64) != Some(i64::from(context.version))
            || value.get("notification_only").and_then(Value::as_bool) != Some(true)
        {
            self.language.diagnostic_refresh = None;
            self.error = Some("The Java diagnostic refresh acknowledgement could not be verified; diagnostic freshness is unchanged".into());
            return;
        }
        self.language
            .diagnostic_refresh
            .as_mut()
            .unwrap()
            .acknowledged = true;
        self.notice = "Java diagnostic refresh request sent".into();
        self.language.output = "Java diagnostic refresh request sent. Only published diagnostics can establish a diagnostic snapshot; this acknowledgement cannot verify freshness.".into();
    }

    pub(super) fn observe_diagnostics_refresh(&mut self, value: &Value) {
        let Some(refresh) = &self.language.diagnostic_refresh else {
            return;
        };
        if !self.java_diagnostics_refresh_is_current(&refresh.context)
            || value.get("uri").and_then(Value::as_str) != Some(refresh.context.uri.as_str())
        {
            return;
        }
        let Some(batch) = self.language.diagnostics.files.get(&refresh.context.uri) else {
            return;
        };
        let observation = match value.get("version") {
            None | Some(Value::Null) if batch.version.is_none() => Observation::Unversioned,
            Some(version)
                if version.as_i64() == Some(i64::from(refresh.context.version))
                    && batch.version == Some(refresh.context.version) =>
            {
                Observation::MatchingVersion
            }
            _ => return,
        };
        self.language
            .diagnostic_refresh
            .as_mut()
            .unwrap()
            .observation = observation;
    }

    pub(super) fn reject_diagnostics_batch(&mut self, value: Option<&Value>) {
        self.language.diagnostics.incomplete = true;
        if let Some(uri) = value
            .and_then(|value| value.get("uri"))
            .and_then(Value::as_str)
            .filter(|uri| uri.len() <= language_results::MAX_DIAGNOSTIC_URI_BYTES)
        {
            self.language.diagnostics.files.remove(uri);
            if self
                .language
                .diagnostic_refresh
                .as_ref()
                .is_some_and(|refresh| refresh.context.uri == uri)
            {
                self.language.diagnostic_refresh = None;
            }
        } else {
            // A malformed publication with no usable target cannot preserve a
            // claim that the currently displayed set is complete and current.
            self.language.diagnostics.invalidate();
            self.language.diagnostic_refresh = None;
        }
    }

    pub(super) fn active_diagnostics_status(&self) -> DiagnosticStatus {
        let unverified = |message| DiagnosticStatus {
            current: false,
            message,
        };
        if !self.ready() || !self.language.running {
            return unverified(
                "Diagnostics unavailable: start a language server for this connection.".into(),
            );
        }
        if self.language.diagnostics_exited {
            return unverified("Diagnostics unavailable: the language server exited. Stop and restart the session.".into());
        }
        let Some(doc) = self.active() else {
            return unverified("Select an open document to see its diagnostic freshness.".into());
        };
        if !self.language.matches(&doc.path) {
            return unverified(format!(
                "{}: diagnostics unavailable for this language profile.",
                doc.path
            ));
        }
        let Some(ack) = self.language.sync.opened.get(&doc.id) else {
            return unverified(format!(
                "{}: diagnostics pending; this document has not synchronized. Use Sync now first.",
                doc.path
            ));
        };
        let synced = doc.edit_version == ack.edit_version;
        let Some(batch) = self.language.diagnostics.files.get(&ack.uri) else {
            return unverified(if synced {
                format!("{}: diagnostics pending; no batch received for synchronized version {}. Missing events do not mean no problems.", doc.path, ack.version)
            } else {
                format!("{}: diagnostics pending; the current draft has not synchronized. Use Sync now first.", doc.path)
            });
        };
        let count = batch.items.len();
        let Some(version) = batch.version else {
            return unverified(format!("{}: unversioned diagnostics, {count} reported problems; source version not provided. Current draft unverified.{}", doc.path, if synced { "" } else { " Use Sync now for the newer draft." }));
        };
        if synced && version == ack.version {
            return DiagnosticStatus {
                current: true,
                message: format!("{}: current diagnostic snapshot, version {version}; {count} reported problems.", doc.path),
            };
        }
        unverified(format!(
            "{}: stale diagnostic snapshot, version {version}; {count} reported problems. {}",
            doc.path,
            if synced {
                "Awaiting a batch matching the synchronized draft."
            } else {
                "Use Sync now for the newer draft."
            }
        ))
    }

    pub(super) fn java_diagnostics_refresh_controls(&mut self, ui: &mut egui::Ui) {
        if self.language.mode != ServerMode::Java || !self.language.running {
            return;
        }
        let problem = self.java_diagnostics_refresh_problem();
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(problem.is_none(), egui::Button::new("Refresh Java diagnostics"))
                .on_hover_text("Request diagnostics once for the synchronized Java draft. This does not sync, save, or retry edits.")
                .clicked()
            {
                self.request_java_diagnostics_refresh();
            }
            if let Some(problem) = problem {
                ui.label(RichText::new(problem).small().color(MUTED));
            }
        });
        if let Some(refresh) = &self.language.diagnostic_refresh {
            if self.java_diagnostics_refresh_is_current(&refresh.context) {
                let message = match (refresh.acknowledged, refresh.observation) {
                    (false, _) => "Sending Java diagnostic refresh request.",
                    (true, Observation::Awaiting) => "Request sent; awaiting diagnostics. Sending a request does not verify freshness.",
                    (true, Observation::Unversioned) => "New unversioned diagnostics received; source version not provided. This cannot verify the current draft or link the batch to the refresh request.",
                    (true, Observation::MatchingVersion) => "A matching versioned batch was received. It verifies the snapshot, but cannot prove it came from the refresh request.",
                };
                ui.label(
                    RichText::new(format!("{}: {message}", refresh.context.path))
                        .small()
                        .color(MUTED),
                );
            }
        }
    }
}

#[cfg(test)]
#[path = "java_diagnostics_tests.rs"]
mod tests;
