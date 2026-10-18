//! Explicit Java workspace-symbol searches. Results are unversioned index snapshots.
use super::*;
use crate::language_navigation_results::{self, WorkspaceSymbol};

const QUERY_INPUT: &str = "java_type_query";
const INDEX_NOTICE: &str =
    "Unversioned index snapshot; results may lag unsaved edits or project indexing.";

#[derive(Clone, Debug, PartialEq, Eq)]
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

struct TypeKeyIntent {
    context: TypeContext,
    focused: egui::Id,
    row: Option<usize>,
    key: egui::Key,
    navigation: u64,
    document: Option<(u64, u64)>,
}

fn result_id(context: &TypeContext, index: usize) -> egui::Id {
    egui::Id::new((
        "java_type_result",
        context.generation,
        context.session,
        context.sequence,
        index,
    ))
}

fn result_key(key: egui::Key) -> bool {
    matches!(
        key,
        egui::Key::ArrowUp
            | egui::Key::ArrowDown
            | egui::Key::Home
            | egui::Key::End
            | egui::Key::Enter
    )
}

#[derive(Default)]
pub(super) struct TypeSearch {
    query: String,
    focus_query: bool,
    sequence: u64,
    pending: Option<TypeContext>,
    snapshot: Option<Snapshot>,
    error: Option<String>,
    selected: Option<usize>,
    focus_result: Option<usize>,
    key_intent: Option<TypeKeyIntent>,
    submit_query: bool,
    held_keys: HashSet<egui::Key>,
    owned_enter: bool,
    composing: bool,
}

impl TypeSearch {
    pub(super) fn reset(&mut self) {
        self.sequence = self.sequence.wrapping_add(1);
        self.focus_query = false;
        self.pending = None;
        self.snapshot = None;
        self.error = None;
        self.selected = None;
        self.focus_result = None;
        self.key_intent = None;
        self.submit_query = false;
        // Physical input lifetimes survive dismiss/reopen and query changes.
        // A held key or unfinished composition cannot become a fresh action.
    }
}

impl CedarApp {
    pub(crate) fn capture_java_type_input(
        &mut self,
        ctx: &egui::Context,
        raw: &egui::RawInput,
        consumed: &mut [bool],
    ) -> bool {
        let mut claimed_enter = false;
        let state = &mut self.language.types;
        state.key_intent = None;
        state.submit_query = false;
        let was_composing = state.composing;
        let had_keys = !state.held_keys.is_empty();
        for (index, event) in raw.events.iter().enumerate() {
            match event {
                egui::Event::Key { key, pressed, .. } => {
                    // An accepted result Enter owns its physical lifetime even
                    // after opening an editor or leaving the results view.
                    // Preserve keyup, including a release before a fresh press.
                    if *key == egui::Key::Enter {
                        if *pressed && state.owned_enter {
                            consumed[index] = true;
                            claimed_enter = true;
                        } else if !*pressed {
                            state.owned_enter = false;
                        }
                    }
                    if *pressed {
                        state.held_keys.insert(*key);
                    } else {
                        state.held_keys.remove(key);
                    }
                }
                egui::Event::Ime(egui::ImeEvent::Preedit(_)) => state.composing = true,
                egui::Event::Ime(egui::ImeEvent::Commit(_) | egui::ImeEvent::Disabled) => {
                    state.composing = false;
                }
                egui::Event::WindowFocused(false) => {
                    state.held_keys.clear();
                    state.composing = false;
                }
                _ => {}
            }
        }
        if !raw.focused || self.foreign_modal_owns_input(ctx) || self.navigation.blocks_editor() {
            return claimed_enter;
        }
        let context = TypeContext {
            generation: self.generation,
            session: self.language.session,
            sequence: self.language.types.sequence,
            query: self.language.types.query.clone(),
        };
        if !self.java_type_context_current(&context) {
            return claimed_enter;
        }
        let Some(focused) = ctx.memory(|memory| memory.focused()) else {
            return claimed_enter;
        };
        let row = self.language.types.snapshot.as_ref().and_then(|snapshot| {
            snapshot.rows.iter().enumerate().find_map(|(index, _)| {
                (result_id(&snapshot.context, index) == focused).then_some(index)
            })
        });
        let query = focused == egui::Id::new(QUERY_INPUT);
        if !query && row.is_none() {
            return claimed_enter;
        }
        let key = crate::browse::clean_press(&raw.events)
            .filter(|(_, modifiers)| modifiers.is_none() && raw.modifiers.is_none())
            .map(|(key, _)| key)
            .filter(|key| {
                result_key(*key)
                    && (!query || matches!(key, egui::Key::ArrowDown | egui::Key::Enter))
            })
            .filter(|_| {
                !claimed_enter
                    && !had_keys
                    && !was_composing
                    && !self.language.types.composing
                    && !ctx.input(|input| input.pointer.any_down())
                    && !self.language_busy()
            })
            .filter(|key| {
                (query && *key == egui::Key::Enter)
                    || (self.backend_supports("language_resolve_uri")
                        && self.backend_supports("read"))
            });
        // Only Down explicitly transfers from the query. Home/End/Up and all
        // composition input retain the query's native caret behavior. Result
        // navigation presses are shielded before egui spatial focus handling.
        // The ready-reply guard separately sees only newly admitted intent.
        // Keep all releases so egui's held-key state can clear.
        let suppress_query_enter = query
            && !was_composing
            && !self.language.types.composing
            && !raw
                .events
                .iter()
                .any(|event| matches!(event, egui::Event::Ime(_)))
            && (had_keys
                || self.language_busy()
                || raw.events.iter().any(|event| {
                    matches!(
                        event,
                        egui::Event::Key {
                            key: egui::Key::Enter,
                            pressed: true,
                            repeat: true,
                            ..
                        }
                    )
                }));
        let query_down = query
            && !was_composing
            && !self.language.types.composing
            && !raw
                .events
                .iter()
                .any(|event| matches!(event, egui::Event::Ime(_)));
        for (index, event) in raw.events.iter().enumerate() {
            if matches!(event, egui::Event::Key { key: event_key, pressed: true, modifiers, .. }
                if (row.is_some() && result_key(*event_key))
                    || (query_down && modifiers.is_none() && *event_key == egui::Key::ArrowDown)
                    || (suppress_query_enter && *event_key == egui::Key::Enter))
            {
                consumed[index] = true;
            }
        }
        if let Some(key) = key {
            if row.is_some() && key == egui::Key::Enter {
                claimed_enter = true;
                self.language.types.owned_enter =
                    self.language.types.held_keys.contains(&egui::Key::Enter);
            }
            self.language.types.key_intent = Some(TypeKeyIntent {
                context,
                focused,
                row,
                key,
                navigation: self.navigation_epoch,
                document: self.active().map(|doc| (doc.id, doc.edit_version)),
            });
        }
        claimed_enter
    }

    pub(crate) fn java_type_newer_input(&self, ctx: &egui::Context) -> bool {
        self.navigation.input_frame_current(ctx)
            && self
                .language
                .types
                .key_intent
                .as_ref()
                .is_some_and(|intent| {
                    self.java_type_context_current(&intent.context)
                        && self.navigation_epoch == intent.navigation
                        && self.active().map(|doc| (doc.id, doc.edit_version)) == intent.document
                        && ctx.memory(|memory| memory.focused()) == Some(intent.focused)
                        && !self.language_busy()
                })
    }

    fn java_types_problem(&self) -> Option<String> {
        if !self.ready() {
            return Some("Find Java type requires a ready workspace connection".into());
        }
        if self.close_after_language_stop || self.recovery.closing.is_some() {
            return Some("Finish closing the current session first".into());
        }
        if !self.execution_trusted() {
            return Some(
                "Find Java type requires trusted tool permission for this connection".into(),
            );
        }
        if !self.language.running || self.language.diagnostics_exited {
            return Some("Start a Java / JDT LS session for this connection first".into());
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
        if (state.focus_query
            && (!self.tools_open
                || self.tool != crate::Tool::Language
                || self.language.view != View::JavaTypes))
            || state
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
            if matches!(job, Job::Open { path: pending, navigation: epoch, .. } | Job::LanguageOpen { path: pending, navigation: epoch, .. } if pending == &path && *epoch == navigation)
            {
                *job = Job::JavaTypeOpen {
                    path: path.clone(),
                    navigation,
                    context: context.clone(),
                };
            }
        }
    }

    fn show_java_types(&mut self, ctx: &egui::Context) {
        if self.java_types_problem().is_some() || self.foreign_modal_owns_input(ctx) {
            return;
        }
        // Retire a hidden view before making it current again. Reopening must
        // not revive an outstanding query or a previously invalid snapshot.
        self.invalidate_java_types();
        self.dismiss_navigation();
        self.navigation.restore_focus = false;
        self.tools_open = true;
        self.tool = crate::Tool::Language;
        self.language.view = View::JavaTypes;
        self.language.completion_popup = false;
        // The native button's Enter/Space (and Space text companion) still
        // belongs to that button. Transfer focus only after the query widget
        // processes this pass, so opening cannot submit or edit the query.
        self.language.types.focus_query = true;
        ctx.request_repaint();
    }

    // Shared by the common browsing header and the existing Language panel.
    // Opening the view is local: Search/Enter retains sole query authority.
    pub(crate) fn java_type_entry(&mut self, ui: &mut egui::Ui) -> egui::Response {
        let problem = self.java_types_problem();
        let response = ui.add_enabled(
            problem.is_none(),
            egui::Button::new("Find Java type").selected(
                self.tools_open
                    && self.tool == crate::Tool::Language
                    && self.language.view == View::JavaTypes,
            ),
        );
        let response = if let Some(problem) = problem {
            response.on_disabled_hover_text(problem)
        } else {
            response.on_hover_text("Open Java type search in the running Java / JDT LS session")
        };
        if response.clicked() {
            self.show_java_types(ui.ctx());
        }
        response
    }

    pub(super) fn java_type_controls(&mut self, ui: &mut egui::Ui) {
        if self.language.mode == ServerMode::Java {
            let response = self.java_type_entry(ui);
            #[cfg(test)]
            crate::workspace_access_tests::record(ui, "java_type_language_entry", &response);
            #[cfg(not(test))]
            let _ = response;
        }
    }

    pub(super) fn java_type_shortcuts(&mut self, ctx: &egui::Context) -> bool {
        let intent = self.language.types.key_intent.take();
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
        if let Some(intent) = intent.filter(|intent| {
            self.navigation.input_frame_current(ctx)
                && self.java_type_context_current(&intent.context)
                && self.navigation_epoch == intent.navigation
                && self.active().map(|doc| (doc.id, doc.edit_version)) == intent.document
                && ctx.memory(|memory| memory.focused()) == Some(intent.focused)
                && !self.language_busy()
        }) {
            if intent.row.is_none() && intent.key == egui::Key::Enter {
                self.language.types.submit_query = !self.language_busy();
                return true;
            }
            let rows = self
                .language
                .types
                .snapshot
                .as_ref()
                .filter(|snapshot| snapshot.context == intent.context)
                .filter(|_| {
                    self.backend_supports("language_resolve_uri") && self.backend_supports("read")
                })
                .map(|snapshot| {
                    snapshot
                        .rows
                        .iter()
                        .enumerate()
                        .filter_map(|(index, row)| {
                            row.location.uri.starts_with("file:").then_some(index)
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if intent.key == egui::Key::Enter {
                if let Some(row) = intent.row.filter(|row| {
                    rows.contains(row)
                        && !self.language_busy()
                        && self.backend_supports("language_resolve_uri")
                        && self.backend_supports("read")
                }) {
                    self.language.types.selected = Some(row);
                    self.select_java_type(row);
                }
            } else {
                let next = if let Some(row) = intent.row {
                    rows.iter()
                        .position(|index| *index == row)
                        .and_then(|position| {
                            crate::search_keyboard::destination(rows.len(), position, intent.key)
                                .map(|next| rows[next])
                        })
                } else {
                    rows.first().copied()
                };
                if let Some(next) = next {
                    let pending_type_open = self.pending.values().any(|job| match job {
                        Job::JavaTypeOpen {
                            context,
                            navigation,
                            ..
                        } => *context == intent.context && *navigation == self.navigation_epoch,
                        Job::Language(Action {
                            kind:
                                ActionKind::JavaTypeResolve {
                                    context,
                                    navigation,
                                    ..
                                },
                            ..
                        }) => *context == intent.context && *navigation == self.navigation_epoch,
                        _ => false,
                    });
                    if pending_type_open {
                        self.location_history.cancel_pending();
                        self.navigation_epoch = self.navigation_epoch.wrapping_add(1);
                        self.cancel_ready_language_navigation();
                    }
                    self.language.types.selected = Some(next);
                    self.language.types.focus_result = Some(next);
                    ctx.request_repaint();
                }
            }
        }
        // Let the query's TextEdit receive Enter and any text in this frame.
        // Editor completion shortcuts must not consume query or result input.
        ctx.memory(|memory| {
            memory.has_focus(egui::Id::new(QUERY_INPUT))
                || self
                    .language
                    .types
                    .snapshot
                    .as_ref()
                    .is_some_and(|snapshot| {
                        snapshot
                            .rows
                            .iter()
                            .enumerate()
                            .any(|(index, _)| memory.has_focus(result_id(&snapshot.context, index)))
                    })
        })
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
        let mut query_response = None;
        let focus_query = self.language.types.focus_query;
        let submit_query = std::mem::take(&mut self.language.types.submit_query);
        ui.horizontal_wrapped(|ui| {
            let input = ui.add(
                egui::TextEdit::singleline(&mut self.language.types.query)
                    .id(egui::Id::new(QUERY_INPUT))
                    .char_limit(256)
                    .hint_text("Type name (case follows the Java server)")
                    .desired_width(260.0),
            );
            if focus_query && !ui.is_sizing_pass() {
                input.request_focus();
                self.language.types.focus_query = false;
            }
            if input.changed() {
                self.language.types.reset();
                self.language.cjk_seen |=
                    crate::system_fonts::contains_cjk(&self.language.types.query);
            }
            query_response = Some(input.clone());
            let valid =
                language_navigation_results::validate_workspace_query(&self.language.types.query)
                    .is_ok();
            let enabled = problem.is_none() && valid && !self.language_busy();
            search = ui
                .add_enabled(enabled, egui::Button::new("Search"))
                .clicked()
                || (!focus_query
                    && submit_query
                    && enabled
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
            // Single-line Enter surrenders TextEdit focus. Restore it on the
            // rendered query after its input processing; Down is a separate act.
            if let Some(response) = query_response {
                response.request_focus();
            }
            ui.ctx().request_repaint();
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
        let focus_result = if ui.is_sizing_pass() {
            None
        } else {
            self.language.types.focus_result.take()
        };
        let mut focused_result = None;
        if let Some(snapshot) = &self.language.types.snapshot {
            ui.label(format!(
                "{} results for {:?}",
                snapshot.rows.len(),
                snapshot.context.query
            ));
            if snapshot.rows.is_empty() {
                ui.label("No indexed matches. Try a different query, or retry after indexing.");
            } else {
                ui.label(RichText::new("Down from query selects a result · Up/Down/Home/End moves · Enter opens · Tab changes control").small().color(MUTED));
            }
            let enabled = self.java_type_context_current(&snapshot.context)
                && self.backend_supports("language_resolve_uri")
                && self.backend_supports("read");
            let busy = self.language_busy();
            egui::ScrollArea::vertical()
                .id_salt("java_type_results")
                .show(ui, |ui| {
                    for (index, row) in snapshot.rows.iter().enumerate() {
                        let suffix = if row.deprecated { " · deprecated" } else { "" };
                        let label = format!("{} · {}{}", row.name, symbol_kind(row.kind), suffix);
                        let supported = row.location.uri.starts_with("file:");
                        let id = result_id(&snapshot.context, index);
                        // Preserve the existing focus owner while resolving;
                        // other rows stay inert until the request settles.
                        let row_enabled = enabled
                            && supported
                            && (!busy
                                || (self.language.types.selected == Some(index)
                                    && ui.memory(|memory| memory.has_focus(id))));
                        let response = ui
                            .add_enabled_ui(row_enabled, |ui| {
                                crate::navigation::candidate_row(
                                    ui,
                                    id,
                                    self.language.types.selected == Some(index),
                                    &label,
                                    true,
                                )
                            })
                            .inner;
                        #[cfg(test)]
                        crate::workspace_access_tests::record(
                            ui,
                            &format!("java_type_row_{index}"),
                            &response,
                        );
                        // Keyboard activation is admitted only from the original
                        // raw batch and its exact snapshot/focus ticket above.
                        if !busy && response.clicked_by(egui::PointerButton::Primary) {
                            selected = Some(index);
                        }
                        if response.has_focus() && enabled && supported {
                            focused_result = Some(index);
                        }
                        if focus_result == Some(index) && enabled && supported {
                            response.scroll_to_me(Some(egui::Align::Center));
                            response.request_focus();
                        }
                        if response.has_focus() {
                            ui.memory_mut(|memory| {
                                memory.set_focus_lock_filter(
                                    response.id,
                                    egui::EventFilter {
                                        vertical_arrows: true,
                                        ..Default::default()
                                    },
                                )
                            });
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
        if let Some(index) = focus_result.or(focused_result) {
            self.language.types.selected = Some(index);
        }
        if let Some(index) = selected {
            self.language.types.selected = Some(index);
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
