//! Explicit JDT implementation locations, retained through resolve and ordinary Read.
use super::*;
use features::{FeatureKind, FeatureRequest};

const NOTICE: &str = "JDT implementation locations are an unversioned index snapshot and may lag unsaved edits or indexing. Type queries may include subtypes; method results identify declarations, not every inheriting class or a call graph. Hiding or leaving these results dismisses them; query again explicitly.";

#[derive(Default)]
pub(super) struct ImplementationSearch {
    context: Option<FeatureRequest>,
    rows: Option<Vec<Location>>,
    pending: bool,
    navigating: bool,
    navigation_pass: Option<u64>,
    error: Option<String>,
}
impl ImplementationSearch {
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }
    pub(super) fn begin(&mut self, context: FeatureRequest) {
        *self = Self {
            context: Some(context),
            pending: true,
            ..Self::default()
        };
    }
}
impl CedarApp {
    pub(crate) fn defer_java_implementation_reply(&mut self, event: crate::worker::Event) {
        self.language.implementation_replies.push(event);
    }

    pub(crate) fn finish_java_implementation_frame(&mut self, ctx: &egui::Context) {
        let state = &self.language.implementations;
        // A later focus/edit intent wins even if it leaves the source snapshot
        // unchanged and the server has not replied yet. Only the row activation
        // that began this navigation is exempt in its own UI pass.
        let newer_input = ctx.input(|input| input.raw.events.iter().any(navigation_input));
        if state.navigating
            && state.navigation_pass != Some(ctx.cumulative_pass_nr())
            && newer_input
        {
            self.cancel_java_implementations();
        }
        self.finish_java_implementation_replies();
    }

    pub(crate) fn finish_java_implementation_replies(&mut self) {
        for event in std::mem::take(&mut self.language.implementation_replies) {
            self.apply_event_inner(event, false);
        }
    }

    pub(super) fn java_implementations_problem(&self) -> Option<String> {
        if !self.ready() || !self.language.running || self.language.diagnostics_exited {
            return Some("Start a Java / JDT LS session for this connection first".into());
        }
        if !self.execution_trusted() {
            return Some(
                "Go to Implementations requires trusted tool permission for this connection".into(),
            );
        }
        if self.language.mode != ServerMode::Java {
            return Some("Go to Implementations requires a typed Java / JDT LS session".into());
        }
        if !self.backend_supports("language_java_implementations") {
            return Some(self.unsupported_message("language_java_implementations"));
        }
        if !self.language.supports("implementationProvider") {
            return Some(
                "The running Java server does not advertise implementation locations".into(),
            );
        }
        if !self.active().is_some_and(|doc| doc.path.ends_with(".java")) {
            return Some("Select a Java document and place the cursor on a type or method".into());
        }
        if self.close_after_language_stop || self.recovery.closing.is_some() {
            return Some("Finish closing the current session first".into());
        }
        None
    }

    pub(crate) fn java_implementations_operation_problem(
        &self,
        path: &str,
        version: i32,
        line: u32,
        character: u32,
    ) -> Option<String> {
        if let Some(problem) = self.java_implementations_problem() {
            return Some(problem);
        }
        let doc = self.active().expect("implementation guard checked source");
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
            return Some("Go to Implementations requires the active Java draft's exact synchronized version and UTF-16 cursor".into());
        }
        None
    }

    pub(crate) fn request_java_implementations(&mut self) {
        self.request_language_navigation_feature(FeatureKind::JavaImplementations);
    }

    pub(crate) fn java_implementation_context_current(&self, context: &FeatureRequest) -> bool {
        matches!(context.kind, FeatureKind::JavaImplementations)
            && self
                .language
                .implementations
                .context
                .as_ref()
                .is_some_and(|live| {
                    live.sequence == context.sequence && live.navigation == context.navigation
                })
            && !self.foreign_modal_owns_input(&self.editor_ctx)
            && !self.navigation.blocks_editor()
            && self.feature_request_current(context)
    }

    pub(crate) fn java_implementation_action_current(&self, action: &Action) -> bool {
        match &action.kind {
            ActionKind::Feature { request }
                if matches!(request.kind, FeatureKind::JavaImplementations) =>
            {
                self.java_implementation_context_current(request)
            }
            ActionKind::JavaImplementationResolve {
                context,
                sequence,
                navigation,
                ..
            } => {
                self.java_implementation_context_current(context)
                    && *sequence == self.language.navigation_sequence
                    && *navigation == self.navigation_epoch
            }
            _ => true,
        }
    }

    pub(super) fn invalidate_java_implementations(&mut self) {
        if self
            .language
            .implementations
            .context
            .as_ref()
            .is_some_and(|context| !self.java_implementation_context_current(context))
        {
            if self
                .language
                .implementations
                .context
                .as_ref()
                .is_some_and(|context| context.sequence == self.language.features.sequence())
            {
                self.language.features.cancel_pending();
                self.language.cancel_deferred_navigation();
            }
            self.language.implementations.reset();
        }
    }

    pub(crate) fn cancel_java_implementations(&mut self) {
        self.language.features.cancel_pending();
        self.language.implementations.reset();
        self.language.cancel_deferred_navigation();
        self.notice = "Implementation locations dismissed; late replies will be ignored".into();
    }

    pub(super) fn apply_java_implementations(&mut self, context: FeatureRequest, value: Value) {
        if !self.java_implementation_context_current(&context) {
            return;
        }
        self.language.implementations.pending = false;
        match crate::language_navigation_results::parse_java_implementations(&value) {
            Ok(rows) => {
                self.language.cjk_seen |= rows
                    .iter()
                    .any(|row| crate::system_fonts::contains_cjk(&row.uri));
                self.language.implementations = ImplementationSearch {
                    context: Some(context),
                    rows: Some(rows),
                    ..Default::default()
                };
            }
            Err(error) => self.java_implementation_error(&context, &error),
        }
    }

    pub(crate) fn java_implementation_error(&mut self, context: &FeatureRequest, error: &str) {
        if self.java_implementation_context_current(context) {
            self.language.implementations = ImplementationSearch {
                context: Some(context.clone()),
                error: Some(error.into()),
                ..Default::default()
            };
        }
    }

    #[cfg(test)]
    pub(crate) fn select_java_implementation(&mut self, index: usize) {
        self.select_java_implementation_in_pass(index, None);
    }

    fn select_java_implementation_in_pass(&mut self, index: usize, navigation_pass: Option<u64>) {
        let state = &self.language.implementations;
        let Some(context) = state.context.as_ref() else {
            return;
        };
        if !self.java_implementation_context_current(context) || self.language_busy() {
            return;
        }
        let Some(location) = state
            .rows
            .as_ref()
            .and_then(|rows| rows.get(index))
            .cloned()
        else {
            return;
        };
        for capability in ["language_resolve_uri", "read"] {
            if !self.backend_supports(capability) {
                self.error = Some(self.unsupported_message(capability));
                return;
            }
        }
        if !location.uri.starts_with("file:") {
            self.error = Some("Dependency archives and external URLs cannot be opened".into());
            return;
        }
        // Own navigation advances ordinary navigation guards, then retains this
        // exact source/participant snapshot under the new sequence. It never
        // recaptures changed text, a cursor, or an acknowledged LSP version.
        let mut state = std::mem::take(&mut self.language.implementations);
        self.navigation_changed();
        let context = state.context.as_mut().expect("checked context");
        context.sequence = self.language.features.sequence();
        context.navigation = self.navigation_epoch;
        let context = context.clone();
        state.navigating = true;
        state.navigation_pass = navigation_pass;
        self.language.implementations = state;
        let sequence = self.language.navigation_sequence;
        let navigation = self.navigation_epoch;
        let id = self.language_request(
            Operation::LanguageResolveUri {
                uri: location.uri.clone(),
            },
            ActionKind::JavaImplementationResolve {
                context: context.clone(),
                sequence,
                navigation,
                location,
            },
        );
        if id == 0 {
            self.java_implementation_error(&context, "Implementation target could not be resolved");
        }
    }

    pub(super) fn apply_java_implementation_resolve(
        &mut self,
        mut context: FeatureRequest,
        sequence: u64,
        navigation: u64,
        location: Location,
        value: Value,
    ) {
        if !self.java_implementation_context_current(&context) {
            return;
        }
        if let Some(doc) = value
            .get("path")
            .and_then(Value::as_str)
            .and_then(|path| self.documents.iter().find(|doc| doc.path == path))
        {
            if let Err(error) = implementation_range_fits(&doc.text, location.range) {
                self.java_implementation_error(&context, &error);
                self.error = Some(error);
                return;
            }
        }
        let mut state = std::mem::take(&mut self.language.implementations);
        let Some(path) =
            self.apply_resolved_language_location(sequence, navigation, location, value)
        else {
            state.navigating = false;
            self.language.implementations = state;
            return;
        };
        context.sequence = self.language.features.sequence();
        context.navigation = self.navigation_epoch;
        let navigation = self.navigation_epoch;
        let mut reading = false;
        for job in self.pending.values_mut() {
            if matches!(job, Job::Open { path: pending, navigation: epoch, .. } if pending == &path && *epoch == navigation)
            {
                *job = Job::JavaImplementationOpen {
                    path: path.clone(),
                    navigation,
                    context: context.clone(),
                };
                reading = true;
            }
        }
        if reading {
            state.context = Some(context);
            self.language.implementations = state;
        }
    }

    pub(crate) fn java_implementation_read_problem(
        &self,
        path: &str,
        text: &str,
    ) -> Option<String> {
        let Some(navigation) = self.language.deferred_navigation.get(path) else {
            return Some(
                "The selected implementation location expired before Read completed".into(),
            );
        };
        implementation_range_fits(text, navigation.range).err()
    }

    pub(crate) fn java_implementation_read_finished(&mut self) {
        self.language.implementations.navigating = false;
    }

    pub(super) fn java_implementations_control(&mut self, ui: &mut egui::Ui) {
        let problem = self.java_implementations_problem();
        let response = ui.add_enabled(
            problem.is_none() && !self.language_busy(),
            egui::Button::new("Go to Implementations"),
        );
        let response = if let Some(problem) = problem {
            response.on_disabled_hover_text(problem)
        } else {
            response.on_hover_text("Synchronize open Java drafts and request JDT implementation locations at the cursor")
        };
        if response.clicked() {
            self.request_java_implementations();
        }
    }

    pub(super) fn java_implementations_view(&mut self, ui: &mut egui::Ui) {
        self.invalidate_java_implementations();
        ui.label(RichText::new("JDT implementation locations").strong());
        ui.label(RichText::new(NOTICE).small().color(AMBER));
        if let Some(problem) = self.java_implementations_problem() {
            ui.colored_label(AMBER, problem);
        }
        let state = &self.language.implementations;
        let pending = state.pending || state.navigating;
        if ui
            .button(if pending { "Cancel" } else { "Dismiss" })
            .clicked()
        {
            self.cancel_java_implementations();
            self.language.view = View::Problems;
            return;
        }
        let state = &self.language.implementations;
        if state.pending {
            ui.label("Synchronizing Java drafts and requesting implementation locations…");
        }
        if state.navigating {
            ui.label("Opening the selected implementation location…");
        }
        if let Some(error) = &state.error {
            ui.colored_label(AMBER, error);
        }
        let mut selected = None;
        if let Some(rows) = &state.rows {
            ui.label(format!("{} JDT implementation locations", rows.len()));
            if rows.is_empty() {
                ui.label("No implementation locations returned. Indexing may still be in progress; retry explicitly when ready.");
            }
            let enabled = !self.language_busy()
                && self.backend_supports("language_resolve_uri")
                && self.backend_supports("read");
            egui::ScrollArea::vertical()
                .id_salt("java_implementation_results")
                .show(ui, |ui| {
                    for (index, row) in rows.iter().enumerate() {
                        let supported = row.uri.starts_with("file:");
                        let label = format!(
                            "{}:{}:{}",
                            row.uri,
                            u64::from(row.range.start.line) + 1,
                            u64::from(row.range.start.character) + 1
                        );
                        if ui
                            .add_enabled(enabled && supported, egui::Button::new(label))
                            .clicked()
                        {
                            selected = Some(index);
                        }
                        if !supported {
                            ui.label(
                                RichText::new(
                                    "Dependency archives and external URLs cannot be opened",
                                )
                                .small()
                                .color(MUTED),
                            );
                        }
                    }
                });
        } else if !pending && state.error.is_none() {
            ui.label(
                "Place the cursor on a Java type or method, then choose Go to Implementations.",
            );
        }
        if let Some(index) = selected {
            let initiating_pass =
                isolated_row_activation(ui.ctx()).then(|| ui.ctx().cumulative_pass_nr());
            self.select_java_implementation_in_pass(index, initiating_pass);
        }
    }
}

fn navigation_input(event: &egui::Event) -> bool {
    matches!(
        event,
        egui::Event::PointerButton { .. }
            | egui::Event::Touch { .. }
            | egui::Event::Key { pressed: true, .. }
            | egui::Event::Text(_)
            | egui::Event::Paste(_)
            | egui::Event::Cut
            | egui::Event::Ime(_)
            | egui::Event::WindowFocused(false)
    )
}

fn isolated_row_activation(ctx: &egui::Context) -> bool {
    ctx.input(|input| {
        let events: Vec<_> = input
            .raw
            .events
            .iter()
            .filter(|event| navigation_input(event))
            .collect();
        matches!(
            events.as_slice(),
            [egui::Event::PointerButton {
                button: egui::PointerButton::Primary,
                pressed: false,
                ..
            }] | [
                egui::Event::PointerButton {
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    ..
                },
                egui::Event::PointerButton {
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    ..
                }
            ]
        ) || matches!(events.as_slice(), [egui::Event::Key {
            key: egui::Key::Enter | egui::Key::Space, repeat: false, modifiers, ..
        }] if modifiers.is_none())
    })
}

fn implementation_range_fits(text: &str, range: Range) -> Result<(), String> {
    let start = completion::position_to_offsets(text, range.start);
    let end = completion::position_to_offsets(text, range.end);
    match (start, end) {
        (Ok((_, start)), Ok((_, end))) if start <= end => Ok(()),
        _ => Err("The implementation range does not fit the current file. Request fresh locations; no document was focused or changed".into()),
    }
}

#[cfg(test)]
#[path = "java_implementations_tests.rs"]
mod tests;
