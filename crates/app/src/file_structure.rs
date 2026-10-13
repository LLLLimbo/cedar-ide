//! Current-file modal over the existing bounded outline. Tickets contain no source text.
use super::*;

const MODAL: &str = "file_structure_modal";
const FILTER: &str = "file_structure_filter";
const MAX_FILTER_CHARS: usize = 256;
const FLAT_NOTICE: &str = "Flat symbol targets are unversioned and may have changed";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Ticket {
    popup: u64,
    generation: u64,
    session: u64,
    document: u64,
    edit_version: u64,
    snapshot: u64,
    filter: u64,
    row: usize,
}

#[derive(Clone, Copy, Default)]
struct Input {
    key: Option<egui::Key>,
    focused: Option<egui::Id>,
    invoke: bool,
    pointer_clean: bool,
    press: bool,
    release: bool,
    newer: bool,
}
impl Input {
    fn capture(input: &egui::RawInput) -> Self {
        let mut state = Self {
            pointer_clean: input.focused,
            newer: !input.focused,
            ..Self::default()
        };
        let mut keys = 0;
        let mut key_clean = input.focused;
        for event in &input.events {
            match event {
                egui::Event::Key {
                    key,
                    pressed,
                    repeat,
                    modifiers,
                    ..
                } => {
                    state.pointer_clean = false;
                    if *pressed {
                        keys += 1;
                        state.newer = true;
                        let invoke = *key == egui::Key::F12
                            && !modifiers.alt
                            && !modifiers.shift
                            && !(modifiers.ctrl && modifiers.mac_cmd)
                            && modifiers.matches_exact(egui::Modifiers::COMMAND);
                        if !*repeat && (modifiers.is_none() || invoke) {
                            state.key = Some(*key);
                            state.invoke = invoke;
                        } else {
                            key_clean = false;
                        }
                    }
                }
                egui::Event::PointerButton {
                    button,
                    pressed,
                    modifiers,
                    ..
                } => {
                    key_clean = false;
                    state.newer = true;
                    state.pointer_clean &=
                        *button == egui::PointerButton::Primary && modifiers.is_none();
                    if *pressed {
                        state.press = true;
                    } else {
                        state.release = true;
                    }
                }
                egui::Event::PointerMoved(_) | egui::Event::MouseMoved(_) => {}
                egui::Event::WindowFocused(true)
                | egui::Event::Ime(egui::ImeEvent::Enabled | egui::ImeEvent::Disabled) => {}
                _ => {
                    key_clean = false;
                    state.pointer_clean = false;
                    state.newer = true;
                }
            }
        }
        if !key_clean || keys != 1 {
            state.key = None;
            state.invoke = false;
        }
        state
    }
}

struct Popup {
    serial: u64,
    generation: u64,
    session: u64,
    document: Option<(u64, u64)>,
    visible: bool,
    focus: bool,
    query: String,
    filter: u64,
    selected: Option<usize>,
    presented: Vec<(Ticket, egui::Id)>,
    pressed: Option<(Ticket, egui::Id)>,
    pending: Option<u64>,
    activation: Option<Ticket>,
    remote: Option<(u64, u64)>,
    activation_frame: u64,
    activation_navigation: u64,
    error: Option<String>,
}
#[derive(Default)]
pub(super) struct State {
    serial: u64,
    exhausted: bool,
    popup: Option<Popup>,
    input: Input,
    frame: u64,
    composing: bool,
    shortcut_frame: Option<u64>,
    window_frame: Option<u64>,
    replies: Vec<crate::worker::Event>,
}
impl State {
    pub(super) fn retire(&mut self) {
        match self.serial.checked_add(1) {
            Some(serial) => self.serial = serial,
            None => self.exhausted = true,
        }
        self.popup = None;
    }
}

impl CedarApp {
    pub(crate) fn capture_file_structure_input(&mut self, input: &egui::RawInput) {
        let prior_keys = self.editor_ctx.input(|state| !state.keys_down.is_empty());
        let prior_pointer = self.editor_ctx.input(|state| state.pointer.any_down());
        let state = &mut self.language.features.file_structure;
        let was_composing = state.composing;
        for event in &input.events {
            match event {
                egui::Event::Ime(egui::ImeEvent::Preedit(_)) => state.composing = true,
                egui::Event::Ime(egui::ImeEvent::Commit(_) | egui::ImeEvent::Disabled) => {
                    state.composing = false
                }
                egui::Event::WindowFocused(false) => state.composing = false,
                _ => {}
            }
        }
        state.input = Input::capture(input);
        state.input.focused = self.editor_ctx.memory(|memory| memory.focused());
        let mut edges = input.events.iter().filter_map(|event| {
            if let egui::Event::PointerButton { pressed, .. } = event {
                Some(*pressed)
            } else {
                None
            }
        });
        let first = edges.next();
        let second = edges.next();
        let edges_clean = edges.next().is_none()
            && !(second.is_some() && (first != Some(true) || second != Some(false)))
            && !(first == Some(true)
                && state
                    .popup
                    .as_ref()
                    .is_some_and(|popup| popup.pressed.is_some()));
        state.input.pointer_clean &= !prior_keys
            && input.modifiers.is_none()
            && edges_clean
            && crate::browse::pointer_only(&input.events);
        if prior_keys || prior_pointer {
            state.input.key = None;
            state.input.invoke = false;
        }
        if was_composing || state.composing {
            state.input.key = None;
            state.input.invoke = false;
            state.input.pointer_clean = false;
        }
        match state.frame.checked_add(1) {
            Some(frame) => state.frame = frame,
            None => {
                state.exhausted = true;
                state.popup = None;
                state.input = Input::default();
            }
        }
    }
    #[cfg(test)]
    pub(crate) fn file_structure_activation_pending(&self) -> bool {
        self.language
            .features
            .file_structure
            .popup
            .as_ref()
            .is_some_and(|popup| popup.activation.is_some())
    }
    #[cfg(test)]
    pub(crate) fn file_structure_query(&self) -> Option<&str> {
        self.language
            .features
            .file_structure
            .popup
            .as_ref()
            .map(|popup| popup.query.as_str())
    }
    #[cfg(test)]
    pub(crate) fn file_structure_selected(&self) -> Option<usize> {
        self.language
            .features
            .file_structure
            .popup
            .as_ref()
            .and_then(|popup| popup.selected)
    }
    #[cfg(test)]
    pub(crate) fn file_structure_rows(&self) -> Vec<(usize, &str)> {
        let Some(popup) = self.language.features.file_structure.popup.as_ref() else {
            return vec![];
        };
        self.language
            .features
            .outline
            .as_ref()
            .into_iter()
            .flat_map(|outline| outline.items.iter().enumerate())
            .filter_map(|(index, row)| {
                row.name
                    .contains(&popup.query)
                    .then_some((index, row.name.as_str()))
            })
            .collect()
    }
    pub(crate) fn file_structure_open(&self) -> bool {
        self.language
            .features
            .file_structure
            .popup
            .as_ref()
            .is_some_and(|popup| popup.visible)
    }
    fn file_structure_problem(&self) -> Option<String> {
        if !self.ready() {
            return Some("Connect a workspace to view File Structure".into());
        }
        if !self.language.running || self.language.diagnostics_exited {
            return Some(
                "File Structure needs an already running language server for this file".into(),
            );
        }
        if !self.backend_supports("language_document_symbols") {
            return Some(self.unsupported_message("language_document_symbols"));
        }
        if !self.language.supports("documentSymbolProvider") {
            return Some("The running language server does not advertise document symbols".into());
        }
        let Some(doc) = self.active() else {
            return Some("Open a document to view File Structure".into());
        };
        if !self.language.matches(&doc.path) {
            return Some("This file does not match the running server’s language profile".into());
        }
        if doc.text.len() > MAX_FILE_BYTES || doc.edit_version == u64::MAX {
            return Some("File Structure requires a draft within the 1 MiB language limit and a current edit version".into());
        }
        if !self.language.sync.synced(doc.id, doc.edit_version) {
            if self
                .language
                .sync
                .next_version(doc.id)
                .is_none_or(|version| version.max(self.language.next_version) == i32::MAX)
            {
                return Some("Language version limit reached; restart the server".into());
            }
            let capability = if self.language.sync.opened.contains_key(&doc.id) {
                "language_change"
            } else {
                "language_open"
            };
            if !self.backend_supports(capability) {
                return Some(self.unsupported_message(capability));
            }
        }
        if self.close_after_language_stop || self.recovery.closing.is_some() {
            return Some("Finish closing the current session first".into());
        }
        None
    }
    fn file_structure_foreign_modal(&self) -> bool {
        self.confirm.is_some()
            || self.copy_draft.is_open()
            || self.recovery.remove_confirmation.is_some()
            || self.run_state.transition_pending()
            || self.language.features.preview_open
            || self.editor_ctx.memory(|memory| {
                memory.top_modal_layer().is_some_and(|layer| {
                    layer != egui::LayerId::new(egui::Order::Foreground, egui::Id::new(MODAL))
                })
            })
    }
    fn file_structure_context_current(&self) -> bool {
        self.language
            .features
            .file_structure
            .popup
            .as_ref()
            .is_some_and(|popup| {
                popup.generation == self.generation
                    && popup.session == self.language.session
                    && popup.document == self.active().map(|doc| (doc.id, doc.edit_version))
            })
    }
    pub(super) fn file_structure_request_current(&self, request: &FeatureRequest) -> bool {
        match request.destination {
            FeatureDestination::Tool => true,
            FeatureDestination::FileStructure(serial) => {
                !self.language.features.file_structure.exhausted
                    && !self.file_structure_foreign_modal()
                    && self.file_structure_context_current()
                    && self.file_structure_problem().is_none()
                    && self
                        .language
                        .features
                        .file_structure
                        .popup
                        .as_ref()
                        .is_some_and(|popup| {
                            popup.visible
                                && popup.serial == serial
                                && popup.pending == Some(request.sequence)
                        })
            }
        }
    }
    pub(crate) fn file_structure_ticket_current(&self, ticket: &Ticket) -> bool {
        !self.language.features.file_structure.exhausted
            && self.file_structure_context_current()
            && !self.file_structure_foreign_modal()
            && self.file_structure_problem().is_none()
            && self
                .language
                .features
                .file_structure
                .popup
                .as_ref()
                .is_some_and(|popup| {
                    popup.serial == ticket.popup
                        && popup.generation == ticket.generation
                        && popup.session == ticket.session
                        && popup.document == Some((ticket.document, ticket.edit_version))
                        && popup.filter == ticket.filter
                        && popup.pending.is_none()
                })
            && self
                .language
                .features
                .outline
                .as_ref()
                .is_some_and(|outline| {
                    outline.generation == ticket.snapshot
                        && self.feature_document_current(&outline.request)
                        && outline.items.get(ticket.row).is_some_and(|row| {
                            self.language
                                .features
                                .file_structure
                                .popup
                                .as_ref()
                                .is_some_and(|popup| row.name.contains(&popup.query))
                        })
                })
    }
    pub(crate) fn dismiss_file_structure(&mut self) {
        let state = &mut self.language.features.file_structure;
        if state.popup.is_none() {
            return;
        }
        let remote = state.popup.as_ref().and_then(|popup| popup.remote);
        let pending = state.popup.as_ref().and_then(|popup| popup.pending);
        state.retire();
        if pending == Some(self.language.features.sequence) {
            // Retire only this feature intent. An admitted Sync remains ordinary document work.
            self.language.features.cancel_pending();
        }
        if let Some((_, navigation)) = remote {
            self.history_cancel_ticket(navigation);
            self.language.cancel_deferred_navigation();
        }
    }
    fn return_from_file_structure(&mut self) {
        let restore = self.file_structure_context_current() && !self.file_structure_foreign_modal();
        self.dismiss_file_structure();
        if restore {
            self.navigation.restore_focus = true;
        }
    }
    pub(crate) fn show_file_structure(&mut self) {
        self.dismiss_file_structure();
        if self.language.features.file_structure.exhausted {
            self.error = Some("File Structure identity limit reached; restart Cedar".into());
            return;
        }
        self.invalidate_language_features();
        self.dismiss_navigation();
        self.language.completion_popup = false;
        let problem = self.file_structure_problem();
        let document = self.active().map(|doc| (doc.id, doc.edit_version));
        let state = &mut self.language.features.file_structure;
        state.popup = Some(Popup {
            serial: state.serial,
            generation: self.generation,
            session: self.language.session,
            document,
            visible: true,
            focus: true,
            query: String::new(),
            filter: 0,
            selected: None,
            presented: vec![],
            pressed: None,
            pending: None,
            activation: None,
            remote: None,
            activation_frame: 0,
            activation_navigation: self.navigation_epoch,
            error: problem.clone(),
        });
        // A fresh explicit invocation owns focus now, including when egui still
        // retains the layer from the just-dismissed File Structure modal.
        self.navigation.restore_focus = false;
        if problem.is_none() && self.language.features.outline.is_none() {
            self.refresh_file_structure();
        }
    }
    fn refresh_file_structure(&mut self) {
        if let Some(problem) = self.file_structure_problem() {
            self.file_structure_error(&problem);
            return;
        }
        let Some(popup) = self.language.features.file_structure.popup.as_mut() else {
            return;
        };
        let Some(serial) = popup.serial.checked_add(1) else {
            self.language.features.file_structure.exhausted = true;
            self.dismiss_file_structure();
            self.error = Some("File Structure identity limit reached; restart Cedar".into());
            return;
        };
        popup.serial = serial;
        self.language.features.file_structure.serial = popup.serial;
        popup.selected = None;
        popup.presented.clear();
        popup.pressed = None;
        popup.activation = None;
        let remote = popup.remote.take();
        popup.error = None;
        let serial = popup.serial;
        if let Some((_, navigation)) = remote {
            self.history_cancel_ticket(navigation);
            self.language.cancel_deferred_navigation();
        }
        self.request_language_feature_at(
            FeatureKind::Outline,
            FeatureDestination::FileStructure(serial),
        );
        let sequence = self.language.features.sequence;
        if let Some(popup) = self.language.features.file_structure.popup.as_mut() {
            popup.pending = Some(sequence);
        }
    }
    pub(super) fn file_structure_received(&mut self) {
        if let Some(popup) = self.language.features.file_structure.popup.as_mut() {
            popup.pending = None;
            popup.selected = None;
            popup.presented.clear();
            popup.pressed = None;
            popup.error = None;
        }
    }
    pub(crate) fn file_structure_error(&mut self, error: &str) {
        if let Some(popup) = self.language.features.file_structure.popup.as_mut() {
            popup.pending = None;
            popup.remote = None;
            popup.activation = None;
            popup.error = Some(error.into());
        }
    }
    pub(crate) fn file_structure_shortcuts(&mut self, ctx: &egui::Context) -> bool {
        let native_frame = self.navigation.input_frame_current(ctx);
        let state = &mut self.language.features.file_structure;
        let input = if !native_frame || state.shortcut_frame == Some(state.frame) {
            Input::default()
        } else {
            state.shortcut_frame = Some(state.frame);
            state.input
        };
        if input.key == Some(egui::Key::Escape)
            && self
                .language
                .features
                .file_structure
                .popup
                .as_ref()
                .is_some_and(|popup| !popup.visible)
        {
            self.return_from_file_structure();
            crate::navigation::discard_keyboard(ctx);
            return true;
        }
        if input.invoke
            && self.navigation.input_frame_current(ctx)
            && !self.navigation.dialog_open()
            && !self.file_structure_foreign_modal()
        {
            ctx.input_mut(|input| {
                input.events.retain(|event| {
                    !matches!(
                        event,
                        egui::Event::Key {
                            key: egui::Key::F12,
                            ..
                        }
                    )
                })
            });
            self.show_file_structure();
            return true;
        }
        self.file_structure_open()
            || self
                .language
                .features
                .file_structure
                .popup
                .as_ref()
                .is_some_and(|popup| popup.activation.is_some())
    }
    pub(crate) fn file_structure_window(&mut self, ctx: &egui::Context) {
        if !self.file_structure_context_current() || self.file_structure_foreign_modal() {
            self.dismiss_file_structure();
            return;
        }
        let Some(mut popup) = self.language.features.file_structure.popup.take() else {
            return;
        };
        if !popup.visible {
            self.language.features.file_structure.popup = Some(popup);
            return;
        }
        let native_frame = self.navigation.input_frame_current(ctx);
        let state = &mut self.language.features.file_structure;
        let fresh = native_frame && state.window_frame != Some(state.frame);
        let input = if !fresh {
            Input {
                pointer_clean: true,
                ..Input::default()
            }
        } else {
            state.window_frame = Some(state.frame);
            state.input
        };
        let mut dismiss = input.key == Some(egui::Key::Escape);
        let mut refresh = false;
        let mut activate = None;
        let mut exhausted = false;
        let mut query_cjk = false;
        let focused = input.focused;
        let query_focused = focused == Some(egui::Id::new(FILTER));
        let row_focused = popup.presented.iter().any(|(_, id)| Some(*id) == focused);
        let captured_enter =
            if input.key == Some(egui::Key::Enter) && (query_focused || row_focused) {
                popup.selected.and_then(|selected| {
                    popup
                        .presented
                        .iter()
                        .find(|(ticket, id)| {
                            ticket.row == selected && (!row_focused || Some(*id) == focused)
                        })
                        .map(|(ticket, _)| *ticket)
                })
            } else {
                None
            };
        if fresh && !input.pointer_clean {
            popup.pressed = None;
        } else if fresh {
            let clicked = ctx.interaction_snapshot(|interaction| interaction.clicked);
            if input.press {
                popup.pressed = popup
                    .presented
                    .iter()
                    .find(|(_, id)| {
                        ctx.read_response(*id).is_some_and(|response| {
                            response.enabled()
                                && (response.is_pointer_button_down_on()
                                    || (clicked == Some(*id)
                                        && response.clicked_by(egui::PointerButton::Primary)))
                        })
                    })
                    .copied();
            }
            if input.release {
                if let Some((ticket, id)) = popup.pressed.take() {
                    if clicked == Some(id)
                        && ctx.read_response(id).is_some_and(|response| {
                            response.enabled() && response.clicked_by(egui::PointerButton::Primary)
                        })
                    {
                        activate = Some(ticket);
                    }
                }
            }
        }
        let move_key = input.key.filter(|key| {
            matches!(
                key,
                egui::Key::ArrowUp | egui::Key::ArrowDown | egui::Key::Home | egui::Key::End
            )
        });
        // These keys belong to the symbol list; Tab remains native egui focus traversal.
        if move_key.is_some() || captured_enter.is_some() || dismiss {
            ctx.input_mut(|input| input.events.retain(|event| !matches!(event, egui::Event::Key { key, .. } if Some(*key) == move_key || *key == egui::Key::Enter || *key == egui::Key::Escape)));
        }
        let problem = self.file_structure_problem();
        let snapshot = self
            .language
            .features
            .outline
            .as_ref()
            .filter(|outline| self.feature_document_current(&outline.request));
        let mut presented = Vec::new();
        let modal = egui::Modal::new(egui::Id::new(MODAL)).show(ctx, |ui| {
            ui.set_width((ctx.screen_rect().width() - 64.0).clamp(240.0, 620.0));
            ui.heading("File Structure");
            if let Some(doc) = self.active() {
                ui.label(RichText::new(&doc.path).small().color(MUTED));
            }
            let before = popup.query.clone();
            let response = ui.add(
                egui::TextEdit::singleline(&mut popup.query)
                    .id(egui::Id::new(FILTER))
                    .char_limit(MAX_FILTER_CHARS)
                    .hint_text("Filter symbol names (literal, case-sensitive)"),
            );
            #[cfg(test)]
            crate::workspace_access_tests::record(ui, FILTER, &response);
            if popup.focus || move_key.is_some() {
                response.request_focus();
                popup.focus = false;
            }
            ui.memory_mut(|memory| {
                memory.set_focus_lock_filter(
                    response.id,
                    egui::EventFilter {
                        tab: false,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        escape: true,
                    },
                )
            });
            if popup.query != before {
                if let Some(filter) = popup.filter.checked_add(1) {
                    popup.filter = filter;
                } else {
                    exhausted = true;
                    dismiss = true;
                }
                query_cjk = crate::system_fonts::contains_cjk(&popup.query);
                popup.selected = None;
                popup.pressed = None;
                popup.activation = None;
                activate = None;
            }
            let enabled = problem.is_none() && popup.pending.is_none() && popup.remote.is_none();
            if let Some(error) = popup.error.as_ref().or(problem.as_ref()) {
                ui.colored_label(AMBER, error);
            }
            if popup.pending.is_some() {
                ui.label("Synchronizing this draft and requesting symbols…");
            }
            if popup.remote.is_some() {
                ui.label("Resolving the selected workspace symbol…");
            }
            if let Some(outline) = snapshot.filter(|_| popup.pending.is_none()) {
                if outline
                    .items
                    .iter()
                    .any(|row| matches!(row.location, OutlineLocation::Remote(_)))
                {
                    ui.colored_label(AMBER, FLAT_NOTICE);
                }
                let rows: Vec<usize> = outline
                    .items
                    .iter()
                    .enumerate()
                    .filter_map(|(index, item)| item.name.contains(&popup.query).then_some(index))
                    .collect();
                if !popup
                    .selected
                    .is_some_and(|selected| rows.contains(&selected))
                {
                    popup.selected = rows.first().copied();
                }
                if let Some(key) = move_key {
                    let current = popup
                        .selected
                        .and_then(|selected| rows.iter().position(|index| *index == selected))
                        .unwrap_or(0);
                    let next = match key {
                        egui::Key::Home => 0,
                        egui::Key::End => rows.len().saturating_sub(1),
                        egui::Key::ArrowUp => current.saturating_sub(1),
                        _ => (current + 1).min(rows.len().saturating_sub(1)),
                    };
                    popup.selected = rows.get(next).copied();
                }
                ui.label(format!("{} symbols · source order", rows.len()));
                egui::ScrollArea::vertical()
                    .id_salt("file_structure_rows")
                    .max_height(360.0)
                    .show(ui, |ui| {
                        for index in rows {
                            let row = &outline.items[index];
                            let ticket = Ticket {
                                popup: popup.serial,
                                generation: popup.generation,
                                session: popup.session,
                                document: outline.request.source.id,
                                edit_version: outline.request.source.edit_version,
                                snapshot: outline.generation,
                                filter: popup.filter,
                                row: index,
                            };
                            let id = egui::Id::new((
                                "file_structure_row",
                                popup.serial,
                                outline.generation,
                                popup.filter,
                                index,
                            ));
                            let allowed = enabled
                                && (!matches!(row.location, OutlineLocation::Remote(_))
                                    || self.backend_supports("language_resolve_uri"));
                            ui.horizontal(|ui| {
                                ui.add_space((row.depth.min(32) * 10) as f32);
                                let label = format!(
                                    "{}  {}{}",
                                    symbol_kind(row.kind),
                                    row.name,
                                    if row.detail.is_empty() {
                                        String::new()
                                    } else {
                                        format!("  ·  {}", row.detail)
                                    }
                                );
                                let response = ui
                                    .add_enabled_ui(allowed, |ui| {
                                        crate::navigation::candidate_row(
                                            ui,
                                            id,
                                            popup.selected == Some(index),
                                            &label,
                                            true,
                                        )
                                    })
                                    .inner;
                                #[cfg(test)]
                                crate::workspace_access_tests::record(
                                    ui,
                                    &format!("file_structure_row_{index}"),
                                    &response,
                                );
                                if response.has_focus() {
                                    popup.selected = Some(index);
                                }
                                if move_key.is_some() && popup.selected == Some(index) {
                                    response.scroll_to_me(Some(egui::Align::Center));
                                }
                                presented.push((ticket, id));
                            });
                        }
                        if presented.is_empty() {
                            ui.label(if popup.query.is_empty() {
                                "No document symbols returned"
                            } else {
                                "No matching symbols"
                            });
                        }
                    });
            }
            ui.horizontal(|ui| {
                let response = ui.add_enabled(problem.is_none(), egui::Button::new("Refresh"));
                #[cfg(test)]
                crate::workspace_access_tests::record(ui, "file_structure_refresh", &response);
                if fresh
                    && response.clicked()
                    && (input.pointer_clean
                        || input.key == Some(egui::Key::Enter)
                        || input.key == Some(egui::Key::Space))
                {
                    refresh = true;
                }
                let close = ui.button("Close");
                #[cfg(test)]
                crate::workspace_access_tests::record(ui, "file_structure_close", &close);
                if fresh
                    && close.clicked()
                    && (input.pointer_clean
                        || input.key == Some(egui::Key::Enter)
                        || input.key == Some(egui::Key::Space))
                {
                    dismiss = true;
                }
                ui.label(
                    RichText::new("↑ ↓ Home End · Enter to go · Escape to return")
                        .small()
                        .color(MUTED),
                );
            });
        });
        dismiss |= fresh && modal.backdrop_response.clicked() && input.pointer_clean;
        if activate.is_none() {
            activate = captured_enter;
        }
        self.language.cjk_seen |= query_cjk;
        self.language.features.file_structure.exhausted |= exhausted;
        popup.presented = presented;
        self.language.features.file_structure.popup = Some(popup);
        if dismiss {
            self.return_from_file_structure();
        } else if refresh {
            self.refresh_file_structure();
        } else if let Some(ticket) = activate {
            self.activate_file_structure(ticket);
        }
    }
    fn activate_file_structure(&mut self, ticket: Ticket) {
        if !self.file_structure_ticket_current(&ticket) {
            return;
        }
        let location = self.language.features.outline.as_ref().unwrap().items[ticket.row]
            .location
            .clone();
        let frame = self.language.features.file_structure.frame;
        if let Some(popup) = self.language.features.file_structure.popup.as_mut() {
            popup.activation = Some(ticket);
            popup.activation_frame = frame;
            popup.activation_navigation = self.navigation_epoch;
            popup.visible = false;
        }
        if let OutlineLocation::Remote(location) = location {
            self.navigate_language(location);
            let sequence = self.language.navigation_sequence;
            let navigation = self.navigation_epoch;
            for job in self.pending.values_mut() {
                if let Job::Language(action) = job {
                    if let ActionKind::ResolveUri {
                        sequence: old_sequence,
                        navigation: old_navigation,
                        location,
                    } = &action.kind
                    {
                        if *old_sequence == sequence && *old_navigation == navigation {
                            action.kind = ActionKind::FileStructureResolve {
                                ticket,
                                sequence,
                                navigation,
                                location: location.clone(),
                            };
                            if let Some(popup) =
                                self.language.features.file_structure.popup.as_mut()
                            {
                                popup.remote = Some((sequence, navigation));
                            }
                        }
                    }
                }
            }
            if self
                .language
                .features
                .file_structure
                .popup
                .as_ref()
                .is_some_and(|popup| popup.remote.is_none())
            {
                self.file_structure_error("This symbol could not be resolved within the workspace");
            }
        }
    }
    pub(crate) fn file_structure_action_current(&self, action: &Action) -> bool {
        match &action.kind {
            ActionKind::Feature { request }
                if matches!(request.destination, FeatureDestination::FileStructure(_)) =>
            {
                self.feature_request_current(request)
            }
            ActionKind::FileStructureResolve {
                ticket,
                sequence,
                navigation,
                ..
            } => {
                self.file_structure_ticket_current(ticket)
                    && *sequence == self.language.navigation_sequence
                    && *navigation == self.navigation_epoch
            }
            _ => true,
        }
    }
    pub(crate) fn apply_file_structure_resolve(
        &mut self,
        ticket: Ticket,
        sequence: u64,
        navigation: u64,
        location: Location,
        value: Value,
    ) {
        if !self.file_structure_ticket_current(&ticket) {
            return;
        }
        let Some(path) =
            self.apply_resolved_language_location(sequence, navigation, location, value)
        else {
            self.file_structure_error("The selected symbol could not be resolved");
            return;
        };
        let navigation = self.navigation_epoch;
        let mut waiting = false;
        for job in self.pending.values_mut() {
            if matches!(job, Job::Open { path: pending, navigation: epoch, .. } | Job::LanguageOpen { path: pending, navigation: epoch, .. } if pending == &path && *epoch == navigation)
            {
                *job = Job::FileStructureOpen {
                    path: path.clone(),
                    navigation,
                    ticket,
                };
                waiting = true;
            }
        }
        if waiting {
            if let Some(popup) = self.language.features.file_structure.popup.as_mut() {
                popup.remote = Some((self.language.navigation_sequence, navigation));
            }
        } else {
            self.finish_file_structure_navigation();
        }
    }
    pub(crate) fn finish_file_structure_navigation(&mut self) {
        self.language.features.file_structure.retire();
        self.navigation.restore_focus = true;
    }
    pub(crate) fn defer_file_structure_reply(&mut self, event: crate::worker::Event) {
        self.language.features.file_structure.replies.push(event);
    }
    pub(crate) fn finish_file_structure_frame(&mut self, ctx: &egui::Context) {
        if !self.navigation.input_frame_current(ctx) {
            self.dismiss_file_structure();
        }
        let state = &self.language.features.file_structure;
        if state.popup.as_ref().is_some_and(|popup| {
            !self.file_structure_context_current()
                || self.file_structure_foreign_modal()
                || (popup.activation.is_some()
                    && popup.activation_frame != state.frame
                    && state.input.newer)
                || (popup.activation.is_some()
                    && popup.remote.is_none()
                    && popup.activation_navigation != self.navigation_epoch)
                || (popup.remote.is_some() && self.file_structure_problem().is_some())
        }) {
            self.dismiss_file_structure();
        }
        if self
            .language
            .features
            .file_structure
            .popup
            .as_ref()
            .is_some_and(|popup| {
                popup
                    .pending
                    .is_some_and(|sequence| sequence != self.language.features.sequence)
            })
        {
            let error = self.language.paused_reason.clone().unwrap_or_else(|| {
                "The symbol request was retired; choose Refresh to try again".into()
            });
            self.file_structure_error(&error);
        }
        let local = self
            .language
            .features
            .file_structure
            .popup
            .as_ref()
            .and_then(|popup| {
                (!popup.visible && popup.remote.is_none())
                    .then_some(popup.activation)
                    .flatten()
            });
        if let Some(ticket) = local {
            if !ctx.memory(|memory| memory.top_modal_layer().is_some()) {
                if self.file_structure_ticket_current(&ticket) {
                    let location = self.language.features.outline.as_ref().unwrap().items
                        [ticket.row]
                        .location
                        .clone();
                    self.navigate_outline_location(location);
                    self.finish_file_structure_navigation();
                } else {
                    self.dismiss_file_structure();
                }
            } else {
                ctx.request_repaint();
            }
        }
        for event in std::mem::take(&mut self.language.features.file_structure.replies) {
            self.apply_event_inner(event, false);
        }
    }
}

#[cfg(test)]
#[path = "file_structure_tests.rs"]
mod tests;
