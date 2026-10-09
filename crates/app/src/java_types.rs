//! Explicit Java workspace-symbol searches. Results are unversioned index snapshots.
use super::*;
use crate::language_navigation_results::{self, WorkspaceSymbol};

const QUERY_INPUT: &str = "java_type_query";
const INDEX_NOTICE: &str =
    "Unversioned index snapshot; results may lag unsaved edits or project indexing.";

#[derive(Clone, Debug)]
pub(crate) struct TypeContext {
    generation: u64,
    session: u64,
    sequence: u64,
    query: String,
}

struct Snapshot {
    context: TypeContext,
    rows: Vec<WorkspaceSymbol>,
}

#[derive(Default)]
pub(super) struct TypeSearch {
    query: String,
    sequence: u64,
    pending: Option<TypeContext>,
    snapshot: Option<Snapshot>,
    error: Option<String>,
}

impl TypeSearch {
    pub(super) fn reset(&mut self) {
        self.sequence = self.sequence.wrapping_add(1);
        self.pending = None;
        self.snapshot = None;
        self.error = None;
    }
}

impl CedarApp {
    fn java_types_problem(&self) -> Option<String> {
        if !self.ready() || !self.language.running || self.language.diagnostics_exited {
            return Some("Start a Java / JDT LS session for this connection first".into());
        }
        if !self.execution_trusted() {
            return Some(
                "Find Java type requires trusted tool permission for this connection".into(),
            );
        }
        if self.language.mode != ServerMode::Java {
            return Some("Find Java type requires a typed Java / JDT LS session".into());
        }
        if !self.backend_supports("language_workspace_symbols") {
            return Some(self.unsupported_message("language_workspace_symbols"));
        }
        if !self.language.supports("workspaceSymbolProvider") {
            return Some(
                "The running Java server does not advertise workspace symbol search".into(),
            );
        }
        if self.close_after_language_stop || self.recovery.closing.is_some() {
            return Some("Finish closing the current session first".into());
        }
        None
    }

    pub(crate) fn java_types_operation_problem(&self, query: &str) -> Option<String> {
        self.java_types_problem()
            .or_else(|| language_navigation_results::validate_workspace_query(query).err())
    }

    pub(crate) fn java_type_context_current(&self, context: &TypeContext) -> bool {
        self.java_types_problem().is_none()
            && self.generation == context.generation
            && self.language.session == context.session
            && self.language.types.sequence == context.sequence
            && self.language.types.query == context.query
            && self.tools_open
            && self.tool == crate::Tool::Language
            && self.language.view == View::JavaTypes
    }

    pub(super) fn invalidate_java_types(&mut self) {
        let state = &self.language.types;
        if state
            .pending
            .as_ref()
            .is_some_and(|context| !self.java_type_context_current(context))
            || state
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| !self.java_type_context_current(&snapshot.context))
        {
            self.language.types.reset();
        }
    }

    pub(crate) fn java_type_action_current(&self, action: &Action) -> bool {
        match &action.kind {
            ActionKind::WorkspaceSymbols { context } => self.java_type_context_current(context),
            ActionKind::JavaTypeResolve {
                context,
                sequence,
                navigation,
                ..
            } => {
                self.java_type_context_current(context)
                    && *sequence == self.language.navigation_sequence
                    && *navigation == self.navigation_epoch
            }
            _ => true,
        }
    }

    fn request_java_types(&mut self) {
        if let Some(problem) = self.java_types_operation_problem(&self.language.types.query) {
            self.error = Some(problem);
            return;
        }
        if self.language_busy() {
            return;
        }
        self.language.types.reset();
        self.language.completion_popup = false;
        self.tools_open = true;
        self.tool = crate::Tool::Language;
        self.language.view = View::JavaTypes;
        let context = TypeContext {
            generation: self.generation,
            session: self.language.session,
            sequence: self.language.types.sequence,
            query: self.language.types.query.clone(),
        };
        self.language.types.pending = Some(context.clone());
        // This is an indexed lookup, not a promise to synchronize drafts or wait
        // for indexing. Preserve the user's automatic-sync preference unchanged.
        let id = self.language_request(
            Operation::LanguageWorkspaceSymbols {
                query: context.query.clone(),
            },
            ActionKind::WorkspaceSymbols { context },
        );
        if id == 0 {
            self.language.types.pending = None;
        }
    }

    pub(super) fn apply_java_types(&mut self, context: TypeContext, value: Value) {
        if !self.java_type_context_current(&context) {
            return;
        }
        self.language.types.pending = None;
        match language_navigation_results::parse_workspace_symbols(&value) {
            Ok(rows) => {
                self.language.cjk_seen |= rows.iter().any(|row| {
                    crate::system_fonts::contains_cjk(&row.name)
                        || crate::system_fonts::contains_cjk(&row.container)
                        || crate::system_fonts::contains_cjk(&row.location.uri)
                });
                self.language.types.snapshot = Some(Snapshot { context, rows });
                self.language.types.error = None;
            }
            Err(error) => {
                self.language.types.snapshot = None;
                self.language.types.error = Some(error);
            }
        }
    }

    pub(super) fn java_type_error(&mut self, context: &TypeContext, error: &str) {
        if self.java_type_context_current(context) {
            self.language.types.pending = None;
            self.language.types.snapshot = None;
            self.language.types.error = Some(error.into());
        }
    }

    fn select_java_type(&mut self, index: usize) {
        let Some(snapshot) = &self.language.types.snapshot else {
            return;
        };
        if !self.java_type_context_current(&snapshot.context) || self.language_busy() {
            return;
        }
        let Some(row) = snapshot.rows.get(index) else {
            return;
        };
        let context = snapshot.context.clone();
        let location = row.location.clone();
        self.navigate_language_with_type(location, Some(context));
    }

    pub(super) fn apply_java_type_resolve(
        &mut self,
        context: TypeContext,
        sequence: u64,
        navigation: u64,
        location: Location,
        value: Value,
    ) {
        if !self.java_type_context_current(&context) {
            return;
        }
        let Some(path) =
            self.apply_resolved_language_location(sequence, navigation, location, value)
        else {
            return;
        };
        // Reuse the ordinary read/open path, adding only the index-query lifetime
        // guard. Existing dirty buffers are focused without issuing a Read.
        let navigation = self.navigation_epoch;
        for job in self.pending.values_mut() {
            if matches!(job, Job::Open { path: pending, navigation: epoch, .. } if pending == &path && *epoch == navigation)
            {
                *job = Job::JavaTypeOpen {
                    path: path.clone(),
                    navigation,
                    context: context.clone(),
                };
            }
        }
    }

    pub(super) fn java_type_controls(&mut self, ui: &mut egui::Ui) {
        if self.language.mode != ServerMode::Java {
            return;
        }
        let problem = self.java_types_problem();
        let response = ui.add_enabled(
            problem.is_none(),
            egui::Button::new("Find Java type").selected(self.language.view == View::JavaTypes),
        );
        let response = if let Some(problem) = problem {
            response.on_hover_text(problem)
        } else {
            response
        };
        if response.clicked() {
            self.language.view = View::JavaTypes;
            self.language.completion_popup = false;
            ui.memory_mut(|memory| memory.request_focus(egui::Id::new(QUERY_INPUT)));
        }
    }

    pub(super) fn java_type_shortcuts(&mut self, ctx: &egui::Context) -> bool {
        if !self.tools_open
            || self.tool != crate::Tool::Language
            || self.language.view != View::JavaTypes
            || self.foreign_modal_owns_input(ctx)
        {
            return false;
        }
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            self.language.types.reset();
            self.language.view = View::Problems;
            return true;
        }
        // Let the query's TextEdit receive Enter and any text in this frame.
        // Editor completion shortcuts must not consume query-field input.
        ctx.memory(|memory| memory.has_focus(egui::Id::new(QUERY_INPUT)))
    }

    pub(super) fn java_types_view(&mut self, ui: &mut egui::Ui) {
        self.invalidate_java_types();
        ui.label(RichText::new(INDEX_NOTICE).small().color(AMBER));
        let problem = self.java_types_problem();
        if let Some(problem) = &problem {
            ui.colored_label(AMBER, problem);
        }
        let mut search = false;
        let mut dismiss = false;
        ui.horizontal_wrapped(|ui| {
            let input = ui.add(
                egui::TextEdit::singleline(&mut self.language.types.query)
                    .id(egui::Id::new(QUERY_INPUT))
                    .char_limit(256)
                    .hint_text("Type name (case follows the Java server)")
                    .desired_width(260.0),
            );
            if input.changed() {
                self.language.types.reset();
                self.language.cjk_seen |=
                    crate::system_fonts::contains_cjk(&self.language.types.query);
            }
            let valid =
                language_navigation_results::validate_workspace_query(&self.language.types.query)
                    .is_ok();
            let enabled = problem.is_none() && valid && !self.language_busy();
            search = ui
                .add_enabled(enabled, egui::Button::new("Search"))
                .clicked()
                || (enabled
                    && input.lost_focus()
                    && ui.input(|input| input.key_pressed(egui::Key::Enter)));
            dismiss = ui.button("Dismiss").clicked();
        });
        if dismiss {
            self.language.types.reset();
            self.language.view = View::Problems;
            return;
        }
        if search {
            self.request_java_types();
        }
        if !self.language.types.query.is_empty() {
            if let Err(error) =
                language_navigation_results::validate_workspace_query(&self.language.types.query)
            {
                ui.colored_label(AMBER, error);
            }
        }
        if let Some(error) = &self.language.types.error {
            ui.colored_label(AMBER, error);
        }
        if self.language.types.pending.is_some() {
            ui.label("Searching the Java index…");
        }
        let mut selected = None;
        if let Some(snapshot) = &self.language.types.snapshot {
            ui.label(format!(
                "{} results for {:?}",
                snapshot.rows.len(),
                snapshot.context.query
            ));
            if snapshot.rows.is_empty() {
                ui.label("No indexed matches. Try a different query, or retry after indexing.");
            }
            let enabled = self.java_type_context_current(&snapshot.context)
                && !self.language_busy()
                && self.backend_supports("language_resolve_uri")
                && self.backend_supports("read");
            egui::ScrollArea::vertical()
                .id_salt("java_type_results")
                .show(ui, |ui| {
                    for (index, row) in snapshot.rows.iter().enumerate() {
                        let suffix = if row.deprecated { " · deprecated" } else { "" };
                        let label = format!("{} · {}{}", row.name, symbol_kind(row.kind), suffix);
                        let supported = row.location.uri.starts_with("file:");
                        let response =
                            ui.add_enabled(enabled && supported, egui::Button::new(label));
                        if response.clicked() {
                            selected = Some(index);
                        }
                        if !row.container.is_empty() {
                            ui.label(RichText::new(&row.container).small().color(MUTED));
                        }
                        ui.label(
                            RichText::new(format!(
                                "{}:{}:{}",
                                row.location.uri,
                                u64::from(row.location.range.start.line) + 1,
                                u64::from(row.location.range.start.character) + 1
                            ))
                            .small()
                            .color(MUTED),
                        );
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
        } else if self.language.types.pending.is_none() && self.language.types.error.is_none() {
            ui.label("Enter a type name, then press Search or Enter.");
        }
        if let Some(index) = selected {
            self.select_java_type(index);
        }
    }
}

fn symbol_kind(kind: u32) -> &'static str {
    match kind {
        5 => "Class",
        10 => "Enum",
        11 => "Interface",
        23 => "Struct",
        26 => "Type parameter",
        1 => "File",
        2 => "Module",
        3 => "Namespace",
        4 => "Package",
        6 => "Method",
        7 => "Property",
        8 => "Field",
        9 => "Constructor",
        12 => "Function",
        13 => "Variable",
        14 => "Constant",
        15 => "String",
        16 => "Number",
        17 => "Boolean",
        18 => "Array",
        19 => "Object",
        20 => "Key",
        21 => "Null",
        22 => "Enum member",
        24 => "Event",
        25 => "Operator",
        _ => "Symbol",
    }
}

#[cfg(test)]
#[path = "java_types_tests.rs"]
mod tests;
