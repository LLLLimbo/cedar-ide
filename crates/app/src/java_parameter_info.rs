//! Explicit, inert Java signature help. Only the final frame admits sync/query.
use super::*;
use cedar_language::signature_help::{self, ParameterLabel, SignatureHelp};

const SYNC_NOTICE: &str = "Parameter info uses the exact synchronized draft and UTF-16 cursor. LSP synchronization does not guarantee that JDT has finished indexing or analyzing the draft. Move or edit the source, dismiss, or leave this view to expire the result; request it again explicitly.";

#[derive(Clone, Debug)]
pub(crate) struct ParameterContext {
    generation: u64,
    workspace: Option<crate::WorkspaceKey>,
    root: String,
    session: u64,
    navigation: u64,
    sequence: u64,
    feature_sequence: u64,
    document: u64,
    path: String,
    edit_version: u64,
    text: String,
    cursor: Position,
    version: Option<i32>,
}

#[derive(Default)]
pub(super) struct ParameterInfo {
    sequence: u64,
    context: Option<ParameterContext>,
    waiting: bool,
    sync_attempted: bool,
    pending: bool,
    result: Option<SignatureHelp>,
    selected: Option<usize>,
    error: Option<String>,
    reply: Option<crate::worker::Event>,
}
impl ParameterInfo {
    pub(super) fn reset(&mut self) {
        self.sequence = self.sequence.wrapping_add(1);
        self.context = None;
        self.waiting = false;
        self.sync_attempted = false;
        self.pending = false;
        self.result = None;
        self.selected = None;
        self.error = None;
        // A deferred reply still owns its pending worker job. Drain it after the frame,
        // where the retired sequence rejects both successes and errors.
    }
    pub(super) fn waiting_to_dispatch(&self) -> bool {
        self.waiting
    }
}

impl CedarApp {
    pub(super) fn java_parameter_info_problem(&self) -> Option<String> {
        if !self.ready() || !self.language.running || self.language.diagnostics_exited {
            return Some("Start a Java / JDT LS session for this connection first".into());
        }
        if !self.execution_trusted() {
            return Some(
                "Parameter info requires trusted tool permission for this connection".into(),
            );
        }
        if self.language.mode != ServerMode::Java {
            return Some("Parameter info requires a typed Java / JDT LS session".into());
        }
        if !self.backend_supports("language_java_signature_help") {
            return Some(self.unsupported_message("language_java_signature_help"));
        }
        if !self.language.supports("signatureHelpProvider") {
            return Some("The running Java server does not advertise parameter info".into());
        }
        if !self.active().is_some_and(|doc| doc.path.ends_with(".java")) {
            return Some("Select a Java document and place the cursor inside a call".into());
        }
        if self.close_after_language_stop || self.recovery.closing.is_some() {
            return Some("Finish closing the current session first".into());
        }
        None
    }

    pub(crate) fn java_parameter_info_operation_problem(
        &self,
        path: &str,
        version: i32,
        line: u32,
        character: u32,
    ) -> Option<String> {
        if let Some(problem) = self.java_parameter_info_problem() {
            return Some(problem);
        }
        let doc = self.active().expect("parameter info guard checked source");
        if path != doc.path
            || version <= 0
            || !self
                .language
                .sync
                .opened
                .get(&doc.id)
                .is_some_and(|ack| ack.version == version && ack.edit_version == doc.edit_version)
            || utf16_position(&doc.text, doc.cursor).ok() != Some(Position { line, character })
        {
            return Some("Parameter info requires the active Java draft's exact synchronized version and UTF-16 cursor".into());
        }
        None
    }

    pub(crate) fn request_java_parameter_info(&mut self) {
        if let Some(problem) = self.java_parameter_info_problem() {
            self.error = Some(problem);
            return;
        }
        let doc = self.active().expect("checked source");
        if doc.text.len() > MAX_FILE_BYTES {
            self.error =
                Some("Parameter info requires a draft within the 1 MiB language limit".into());
            return;
        }
        let cursor = match utf16_position(&doc.text, doc.cursor) {
            Ok(cursor) => cursor,
            Err(error) => {
                self.error = Some(error);
                return;
            }
        };
        let mut context = ParameterContext {
            generation: self.generation,
            workspace: self.workspace_key.clone(),
            root: self.root.clone(),
            session: self.language.session,
            navigation: self.navigation_epoch,
            sequence: 0,
            feature_sequence: 0,
            document: doc.id,
            path: doc.path.clone(),
            edit_version: doc.edit_version,
            text: doc.text.clone(),
            cursor,
            version: None,
        };
        self.language.parameter_info.reset();
        self.language.features.cancel_pending();
        self.language.intent = None;
        self.language.completions = None;
        self.language.completion_popup = false;
        self.language.acceptance_sequence = self.language.acceptance_sequence.wrapping_add(1);
        context.sequence = self.language.parameter_info.sequence;
        context.feature_sequence = self.language.features.sequence();
        self.language.sync.retry(context.document);
        self.language.parameter_info.context = Some(context);
        self.language.parameter_info.waiting = true;
        self.language.parameter_info.pending = true;
        self.tools_open = true;
        self.tool = crate::Tool::Language;
        self.language.view = View::JavaParameterInfo;
        self.language.paused_reason = None;
        // Capturing this button's intent does not send anything. The editor and
        // every draft/navigation finisher get their turn before final admission.
    }

    fn java_parameter_context_current(&self, context: &ParameterContext) -> bool {
        self.java_parameter_info_problem().is_none()
            && self.generation == context.generation
            && self.workspace_key == context.workspace
            && self.root == context.root
            && self.language.session == context.session
            && self.navigation_epoch == context.navigation
            && self.language.parameter_info.sequence == context.sequence
            && self.language.features.sequence() == context.feature_sequence
            && self.active_document == Some(context.document)
            && self.tools_open
            && self.tool == crate::Tool::Language
            && self.language.view == View::JavaParameterInfo
            && !self.foreign_modal_owns_input(&self.editor_ctx)
            && !self.navigation.blocks_editor()
            && self.active().is_some_and(|doc| {
                doc.id == context.document
                    && doc.path == context.path
                    && doc.edit_version == context.edit_version
                    && doc.text == context.text
                    && utf16_position(&doc.text, doc.cursor).ok() == Some(context.cursor)
            })
            && context.version.is_none_or(|version| {
                self.language
                    .sync
                    .opened
                    .get(&context.document)
                    .is_some_and(|ack| {
                        ack.version == version && ack.edit_version == context.edit_version
                    })
            })
    }

    fn invalidate_java_parameter_info(&mut self) {
        if self
            .language
            .parameter_info
            .context
            .as_ref()
            .is_some_and(|context| !self.java_parameter_context_current(context))
        {
            self.language.parameter_info.reset();
            self.editor_ctx.request_repaint();
        }
    }

    pub(crate) fn defer_java_parameter_reply(&mut self, event: crate::worker::Event) {
        // Final admission permits one query only when all prior jobs drain.
        // Retain at most its first response; duplicate delivery cannot grow a
        // queue or replace the first result while the frame processes input.
        if self.language.parameter_info.reply.is_none() {
            self.language.parameter_info.reply = Some(event);
        }
    }

    pub(crate) fn finish_java_parameter_info_frame(&mut self) {
        self.invalidate_java_parameter_info();
        if let Some(event) = self.language.parameter_info.reply.take() {
            self.apply_event_inner(event, false);
        }
        if !self.language.parameter_info.waiting || !self.pending.is_empty() {
            return;
        }
        let Some(mut context) = self.language.parameter_info.context.clone() else {
            return;
        };
        if !self.java_parameter_context_current(&context) {
            self.language.parameter_info.reset();
            return;
        }
        if !self
            .language
            .sync
            .synced(context.document, context.edit_version)
        {
            if self.language.parameter_info.sync_attempted {
                self.java_parameter_error(
                    &context,
                    "Java draft synchronization failed; request parameter info again explicitly",
                );
            } else {
                self.language.parameter_info.sync_attempted = true;
                self.sync_document(context.document);
            }
            return;
        }
        context.version = self
            .language
            .sync
            .opened
            .get(&context.document)
            .map(|ack| ack.version);
        let Some(version) = context.version else {
            return;
        };
        self.language.parameter_info.context = Some(context.clone());
        self.language.parameter_info.waiting = false;
        let id = self.language_request(
            Operation::LanguageJavaSignatureHelp {
                path: context.path.clone(),
                version,
                line: context.cursor.line,
                character: context.cursor.character,
            },
            ActionKind::JavaParameterInfo {
                context: context.clone(),
            },
        );
        if id == 0 {
            self.java_parameter_error(&context, "Parameter info could not be requested; retry explicitly when the Java session is ready");
        }
    }

    pub(crate) fn apply_java_parameter_event(
        &mut self,
        context: ParameterContext,
        result: Result<crate::Payload, String>,
        connected: bool,
    ) {
        if !connected {
            self.disconnected(
                "The connection closed while requesting parameter info. Your drafts are retained"
                    .into(),
            );
            return;
        }
        if !self.java_parameter_context_current(&context) {
            return;
        }
        match result {
            Ok(crate::Payload::Language { value }) => match signature_help::parse(&value) {
                Ok(result) => {
                    self.language.cjk_seen |= result.signatures.iter().any(|signature| {
                        crate::system_fonts::contains_cjk(&signature.label)
                    });
                    let state = &mut self.language.parameter_info;
                    state.pending = false;
                    state.selected = result.initial_signature();
                    state.result = Some(result);
                    state.error = None;
                    // Replies are admitted after the tools render. Show the
                    // accepted state even when automatic polling is disabled.
                    self.editor_ctx.request_repaint();
                }
                Err(_) => self.java_parameter_error(&context, "The Java server returned invalid or over-limit parameter info; no signatures were accepted"),
            },
            Ok(_) => self.java_parameter_error(&context, "Unexpected parameter info response; no signatures were accepted"),
            Err(_) => self.java_parameter_error(&context, "Parameter info failed. Retry explicitly when the Java session is ready"),
        }
    }

    fn java_parameter_error(&mut self, context: &ParameterContext, error: &str) {
        if self.java_parameter_context_current(context) {
            let state = &mut self.language.parameter_info;
            state.waiting = false;
            state.pending = false;
            state.result = None;
            state.selected = None;
            state.error = Some(error.into());
            self.editor_ctx.request_repaint();
        }
    }

    pub(super) fn java_parameter_info_control(&mut self, ui: &mut egui::Ui) {
        let problem = self.java_parameter_info_problem();
        let response = ui.add_enabled(
            problem.is_none(),
            egui::Button::new("Parameter info")
                .selected(self.language.view == View::JavaParameterInfo),
        );
        #[cfg(test)]
        crate::workspace_access_tests::record(ui, "java_parameter_info", &response);
        let response = if let Some(problem) = problem {
            response.on_disabled_hover_text(problem)
        } else {
            response.on_hover_text("Request Java parameter info at the current cursor")
        };
        if response.clicked() {
            self.request_java_parameter_info();
        }
    }

    pub(super) fn java_parameter_info_view(&mut self, ui: &mut egui::Ui) {
        self.invalidate_java_parameter_info();
        ui.label(RichText::new(SYNC_NOTICE).small().color(MUTED));
        let dismiss = ui.button("Dismiss parameter info");
        #[cfg(test)]
        crate::workspace_access_tests::record(ui, "dismiss_parameter_info", &dismiss);
        if dismiss.clicked() {
            self.language.parameter_info.reset();
            self.language.view = View::Problems;
            return;
        }
        let state = &mut self.language.parameter_info;
        if state.pending {
            ui.label("Synchronizing the Java draft and requesting parameter info…");
        }
        if let Some(error) = &state.error {
            ui.colored_label(AMBER, error);
        }
        let Some(result) = &state.result else {
            return;
        };
        if result.signatures.is_empty() {
            ui.label(
                "No signatures returned. Move the cursor inside a call or retry after indexing.",
            );
            return;
        }
        let selected_row = &mut state.selected;
        egui::ScrollArea::vertical()
            .id_salt("java_parameter_info")
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for index in 0..result.signatures.len() {
                        let response = ui.selectable_value(
                            selected_row,
                            Some(index),
                            format!("Overload {}", index + 1),
                        );
                        #[cfg(test)]
                        crate::workspace_access_tests::record(
                            ui,
                            &format!("java_parameter_overload_{index}"),
                            &response,
                        );
                        #[cfg(not(test))]
                        let _ = response;
                    }
                });
                let Some(selected) = *selected_row else {
                    return;
                };
                let Some(signature) = result.signatures.get(selected) else {
                    return;
                };
                if result.server_selected_signature() == Some(selected) {
                    ui.label(
                        RichText::new("Server-selected overload")
                            .small()
                            .color(MUTED),
                    );
                } else if result.server_selected_signature().is_none()
                    && result.initial_signature() == Some(selected)
                {
                    ui.label(
                        RichText::new(
                            "First overload shown; the server did not select a valid overload",
                        )
                        .small()
                        .color(MUTED),
                    );
                }
                // The parser has validated byte boundaries derived from LSP UTF-16.
                // Server strings are labels only: never markup, links, commands or edits.
                let mut job = egui::text::LayoutJob::default();
                let ordinary = egui::TextFormat {
                    font_id: egui::FontId::monospace(14.0),
                    color: ui.visuals().text_color(),
                    ..Default::default()
                };
                if let Some(range) = result.highlight(selected) {
                    job.append(&signature.label[..range.start], 0.0, ordinary.clone());
                    let emphasis = egui::TextFormat {
                        color: GREEN,
                        background: ui.visuals().selection.bg_fill,
                        ..ordinary.clone()
                    };
                    job.append(&signature.label[range.clone()], 0.0, emphasis);
                    job.append(&signature.label[range.end..], 0.0, ordinary);
                } else {
                    job.append(&signature.label, 0.0, ordinary);
                }
                ui.label(job);
                for (index, parameter) in signature.parameters.iter().enumerate() {
                    let label = match &parameter.label {
                        ParameterLabel::Text(text) => text.as_str(),
                        ParameterLabel::Offsets(_) => parameter
                            .highlight
                            .as_ref()
                            .map(|range| &signature.label[range.clone()])
                            .unwrap_or("(empty parameter label)"),
                    };
                    let text = RichText::new(format!("{}: {label}", index + 1));
                    ui.label(if result.active_parameter(selected) == Some(index) {
                        text.color(GREEN).strong()
                    } else {
                        text
                    });
                }
            });
    }
}

#[cfg(test)]
#[path = "java_parameter_info_tests.rs"]
mod tests;
