//! Bounded, explicitly submitted Project Search snapshots and native row input.
//! A row is historical evidence from one workspace/session/request/index. Its
//! line is an ordinary Open hint, never a claim about current disk or draft text.
use crate::{CedarApp, Tool, WorkspaceKey, AMBER, GREEN, MUTED};
use cedar_protocol::{Payload, SearchMatch};
use eframe::egui::{self, RichText};
use std::collections::HashSet;

pub(super) const MAX_ROWS: usize = 500;
pub(super) const MAX_TEXT_BYTES: usize = 512 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Batch {
    workspace: egui::Id,
    generation: u64,
    request: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Source {
    pub workspace: WorkspaceKey,
    pub root: String,
    pub generation: u64,
    pub request: u64,
    pub query: String,
}

impl Source {
    fn batch(&self) -> Batch {
        Batch {
            workspace: egui::Id::new((&self.workspace, &self.root)),
            generation: self.generation,
            request: self.request,
        }
    }
}

#[derive(Default)]
pub(super) struct Search {
    pub accepted: Option<Source>,
    pub pending: Option<Source>,
    keyboard: Keyboard,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PointerProbe {
    pub pressed: Option<(usize, egui::Id)>,
    pub click: Option<(usize, egui::Id)>,
}

impl Search {
    #[cfg(test)]
    pub(super) fn claims(&self, key: egui::Key) -> bool {
        self.keyboard.claimed.contains(&key)
    }

    #[cfg(test)]
    pub(super) fn pointer_probe(&self) -> PointerProbe {
        let row = |pointer: &Pointer| (pointer.source.index, pointer.source.id);
        PointerProbe {
            pressed: self.keyboard.pressed.as_ref().map(row),
            click: self.keyboard.click.as_ref().map(row),
        }
    }

    pub(super) fn clear_owned(&mut self) {
        self.accepted = None;
        self.retire();
    }

    pub(super) fn retire(&mut self) {
        self.pending = None;
        self.keyboard.invalidate();
    }

    pub(super) fn invalidate(&mut self) {
        self.keyboard.invalidate();
    }
}

pub(super) fn row_id(source: &Source, index: usize) -> egui::Id {
    batch_row_id(source.batch(), index)
}

fn batch_row_id(batch: Batch, index: usize) -> egui::Id {
    egui::Id::new(("project_search_row", batch, index))
}

pub(super) fn validate(matches: &[SearchMatch]) -> Result<(), &'static str> {
    if matches.len() > MAX_ROWS {
        return Err("Search response exceeds the 500-row display limit; no results were accepted");
    }
    let bytes = matches.iter().try_fold(0usize, |total, row| {
        total
            .checked_add(row.path.len())?
            .checked_add(row.text.len())
    });
    if bytes.is_none_or(|bytes| bytes > MAX_TEXT_BYTES) {
        return Err(
            "Search response exceeds the 512 KiB path and snippet limit; no results were accepted",
        );
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct View {
    batch: Batch,
    count: usize,
}

impl View {
    fn of(app: &CedarApp) -> Option<Self> {
        app.search_rows_active().then(|| Self {
            batch: app.search_state.accepted.as_ref().unwrap().batch(),
            count: app.search_results.len(),
        })
    }
}

#[derive(Clone)]
struct Presented {
    index: usize,
    id: egui::Id,
    rect: egui::Rect,
    clip: egui::Rect,
}

#[derive(Clone)]
struct Intent {
    view: View,
    source: Presented,
    key: egui::Key,
}

#[derive(Clone)]
struct Pointer {
    view: View,
    source: Presented,
}

#[derive(Default)]
struct Keyboard {
    view: Option<View>,
    presented: Vec<Presented>,
    held: HashSet<egui::Key>,
    claimed: HashSet<egui::Key>,
    composing: bool,
    input_pass: Option<u64>,
    key: Option<Intent>,
    pressed: Option<Pointer>,
    click: Option<Pointer>,
    render: Option<(View, usize, egui::Id)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Focus(usize),
    Open(usize),
}

pub(super) fn destination(count: usize, current: usize, key: egui::Key) -> Option<usize> {
    if current >= count {
        return None;
    }
    Some(match key {
        egui::Key::ArrowUp => current.saturating_sub(1),
        egui::Key::ArrowDown => (current + 1).min(count - 1),
        egui::Key::Home => 0,
        egui::Key::End => count - 1,
        _ => return None,
    })
}

fn row_key(key: egui::Key) -> bool {
    matches!(
        key,
        egui::Key::ArrowUp
            | egui::Key::ArrowDown
            | egui::Key::Home
            | egui::Key::End
            | egui::Key::Enter
            | egui::Key::Space
    )
}

// Attribute only one exact Space text companion to a single plain press. Any
// other text, paste, IME, pointer, modifier, key or repeated edge vetoes input.
pub(super) fn clean_key(events: &[egui::Event]) -> Option<egui::Key> {
    let mut key = None;
    let mut spaces = 0;
    let mut released = false;
    for event in events {
        match event {
            egui::Event::Key {
                key: candidate,
                pressed: true,
                modifiers,
                ..
            } if modifiers.is_none() && row_key(*candidate) && key.is_none() => {
                key = Some(*candidate)
            }
            egui::Event::Key {
                key: candidate,
                pressed: false,
                modifiers,
                ..
            } if modifiers.is_none() && key == Some(*candidate) && !released => released = true,
            egui::Event::Text(text) if text == " " && spaces == 0 => spaces += 1,
            _ => return None,
        }
    }
    key.filter(|key| spaces == 0 || *key == egui::Key::Space)
}

impl Keyboard {
    fn invalidate(&mut self) {
        self.view = None;
        self.presented.clear();
        self.key = None;
        self.pressed = None;
        self.click = None;
        self.render = None;
        // Lifetime ownership ends at physical release, not a focus/view change.
    }

    fn record(&mut self, ui: &egui::Ui, index: usize, response: &egui::Response) {
        self.presented.push(Presented {
            index,
            id: response.id,
            rect: response.rect,
            clip: ui.clip_rect(),
        });
    }
}

impl CedarApp {
    pub(super) fn search_rows_active(&self) -> bool {
        self.ready()
            && self.search_state.pending.is_none()
            && self.search_state.accepted.as_ref().is_some_and(|source| {
                self.workspace_key.as_ref() == Some(&source.workspace)
                    && self.root == source.root
                    && self.generation == source.generation
                    && self.search_request == source.request
                    && self.search_query == source.query
            })
    }

    fn search_input_allowed(&self, ctx: &egui::Context) -> bool {
        self.search_rows_active()
            && self.tools_open
            && self.tool == Tool::Search
            && !self.open_form
            && !self.new_file
            && !self.navigation.blocks_editor()
            && !self.navigation.restore_focus
            && !self.foreign_modal_owns_input(ctx)
            && !self.find_focus
            && self.close_tab_requested.is_none()
            && !self.close_after_language_stop
            && !self.allow_close
            && !self.active().is_some_and(|doc| doc.jump_to.is_some())
    }

    pub(super) fn apply_search_response(
        &mut self,
        request: u64,
        query: String,
        result: Result<Payload, String>,
    ) {
        let current = self.search_state.pending.as_ref().is_some_and(|source| {
            source.request == request
                && self.search_request == request
                && source.query == query
                && source.generation == self.generation
                && self.workspace_key.as_ref() == Some(&source.workspace)
                && self.root == source.root
        });
        if !current {
            return;
        }
        let source = self.search_state.pending.take().unwrap();
        self.search_state.invalidate();
        let (matches, truncated) = match result {
            Ok(Payload::Matches { matches, truncated }) => (matches, truncated),
            Ok(_) => {
                self.error = Some("Unexpected search response; no results were accepted".into());
                return;
            }
            Err(error) => {
                self.error = Some(error);
                return;
            }
        };
        if let Err(error) = validate(&matches) {
            self.error = Some(error.into());
            return;
        }
        self.notice = format!("{} matches for {:?}", matches.len(), source.query);
        self.search_results = matches;
        self.search_truncated = truncated;
        self.search_state.accepted = Some(source);
    }

    // Every surface sees the immutable original sequence. The raw hook applies
    // the union of per-event removals only after all lifetimes were observed.
    pub(super) fn capture_search_input(
        &mut self,
        ctx: &egui::Context,
        input: &egui::RawInput,
        consumed: &mut [bool],
    ) {
        let allowed = self.search_input_allowed(ctx);
        let current = View::of(self);
        let focused = ctx.memory(|memory| memory.focused());
        let keyboard = &mut self.search_state.keyboard;
        keyboard.input_pass = Some(ctx.cumulative_pass_nr());
        keyboard.render = None;
        keyboard.key = None;
        keyboard.click = None;
        if !input.focused
            || input
                .events
                .iter()
                .any(|event| matches!(event, egui::Event::WindowFocused(false)))
        {
            keyboard.invalidate();
            keyboard.held.clear();
            keyboard.claimed.clear();
            keyboard.composing = false;
            return;
        }
        let composition_was_active = keyboard.composing;
        for event in &input.events {
            match event {
                egui::Event::Ime(egui::ImeEvent::Preedit(_)) => keyboard.composing = true,
                egui::Event::Ime(egui::ImeEvent::Commit(_) | egui::ImeEvent::Disabled) => {
                    keyboard.composing = false
                }
                _ => {}
            }
        }
        let current_presentation = allowed && current.is_some() && current == keyboard.view;
        let source = if current_presentation {
            keyboard
                .presented
                .iter()
                .find(|row| Some(row.id) == focused)
                .cloned()
        } else {
            None
        };
        let companion = clean_key(&input.events);
        let candidate = if !composition_was_active
            && !keyboard.composing
            && keyboard.held.is_empty()
            && input.modifiers.is_none()
            && !ctx.input(|input| input.pointer.any_down() || !input.keys_down.is_empty())
        {
            companion
        } else {
            None
        };
        // Raw relative motion is an untouched companion. Only the absolute
        // primary-button positions below can create or resolve row ownership;
        // the current-frame egui response must still own the actual click.
        let pointer_only = input.events.iter().all(|event| {
            matches!(
                event,
                egui::Event::PointerMoved(_)
                    | egui::Event::MouseMoved(_)
                    | egui::Event::PointerButton {
                        button: egui::PointerButton::Primary,
                        ..
                    }
                    | egui::Event::Ime(egui::ImeEvent::Enabled | egui::ImeEvent::Disabled)
            )
        });
        if !current_presentation
            || !pointer_only
            || composition_was_active
            || keyboard.composing
            || !keyboard.held.is_empty()
            || !input.modifiers.is_none()
            || ctx.input(|input| !input.keys_down.is_empty())
        {
            keyboard.pressed = None;
        } else {
            for event in &input.events {
                if let egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers,
                } = event
                {
                    if !modifiers.is_none() {
                        keyboard.pressed = None;
                        continue;
                    }
                    if *pressed {
                        keyboard.pressed = keyboard
                            .presented
                            .iter()
                            .find(|row| row.rect.intersect(row.clip).contains(*pos))
                            .cloned()
                            .map(|source| Pointer {
                                view: current.unwrap(),
                                source,
                            });
                    } else if let Some(press) = keyboard.pressed.take() {
                        if Some(press.view) == current
                            && press
                                .source
                                .rect
                                .intersect(press.source.clip)
                                .contains(*pos)
                            && keyboard.presented.iter().any(|row| {
                                row.id == press.source.id
                                    && row.index == press.source.index
                                    && row.rect.intersect(row.clip).contains(*pos)
                            })
                        {
                            keyboard.click = Some(press);
                        }
                    }
                }
            }
        }
        let mut accepted = None;
        let mut claimed_space_press = false;
        for (index, event) in input.events.iter().enumerate() {
            let egui::Event::Key {
                key,
                pressed,
                repeat,
                modifiers,
                ..
            } = event
            else {
                continue;
            };
            if !row_key(*key) {
                continue;
            }
            let claimed = keyboard.claimed.contains(key);
            if !pressed {
                keyboard.held.remove(key);
                keyboard.claimed.remove(key);
                consumed[index] |= claimed;
                continue;
            }
            let fresh = keyboard.held.insert(*key);
            if source.is_some() && modifiers.is_none() {
                // A foreign-held key remains the original consumer's lifetime.
                if fresh || claimed {
                    keyboard.claimed.insert(*key);
                }
                claimed_space_press |= *key == egui::Key::Space;
                if fresh && !repeat && candidate == Some(*key) {
                    accepted = Some(*key);
                }
                consumed[index] = true;
            } else {
                claimed_space_press |= claimed && *key == egui::Key::Space;
                consumed[index] |= claimed;
            }
        }
        if claimed_space_press && companion == Some(egui::Key::Space) {
            for (index, event) in input.events.iter().enumerate() {
                consumed[index] |= matches!(event, egui::Event::Text(text) if text == " ");
            }
        }
        if let (Some(key), Some(source), Some(view)) = (accepted, source, current) {
            keyboard.key = Some(Intent { view, source, key });
        }
    }

    fn search_input_frame_current(&self, ctx: &egui::Context) -> bool {
        self.search_input_allowed(ctx)
            && self.navigation.input_frame_current(ctx)
            && self.search_state.keyboard.input_pass
                == Some(
                    ctx.cumulative_pass_nr()
                        .saturating_sub(ctx.output(|output| output.num_completed_passes) as u64),
                )
            && ctx.input(|input| input.focused)
    }

    fn take_search_click(&mut self, ctx: &egui::Context) -> Option<usize> {
        let click = self.search_state.keyboard.click.take()?;
        (self.search_input_frame_current(ctx) && Some(click.view) == View::of(self))
            .then_some(click.source.index)
    }

    fn take_search_action(&mut self, ctx: &egui::Context) -> Option<Action> {
        let key = self.search_state.keyboard.key.take();
        let current = View::of(self);
        if !self.search_input_frame_current(ctx) {
            self.search_state.invalidate();
            return None;
        }
        if key.is_none() && ctx.output(|output| output.num_completed_passes) > 0 {
            return self
                .search_state
                .keyboard
                .render
                .as_ref()
                .and_then(|(view, index, focus)| {
                    (current.as_ref() == Some(view)
                        && ctx.memory(|memory| memory.focused()) == Some(*focus))
                    .then_some(Action::Focus(*index))
                });
        }
        let intent = key?;
        if current != Some(intent.view)
            || ctx.memory(|memory| memory.focused()) != Some(intent.source.id)
        {
            return None;
        }
        let index = intent.source.index;
        if index >= intent.view.count {
            return None;
        }
        if matches!(intent.key, egui::Key::Enter | egui::Key::Space) {
            Some(Action::Open(index))
        } else {
            destination(intent.view.count, index, intent.key).map(Action::Focus)
        }
    }

    pub(super) fn search_panel(&mut self, ui: &mut egui::Ui) {
        if self.ready() && !self.backend_supports("search") {
            ui.colored_label(AMBER, self.unsupported_message("search"));
        }
        ui.horizontal(|ui| {
            let edit = ui.add(
                egui::TextEdit::singleline(&mut self.search_query)
                    .hint_text("Find text across the workspace...")
                    .desired_width((ui.available_width() - 175.0).max(180.0)),
            );
            #[cfg(test)]
            crate::workspace_access_tests::record(ui, "search_query", &edit);
            if edit.changed() {
                self.search_state.invalidate();
            }
            let enter = edit.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            let submit =
                ui.add_enabled(self.backend_supports("search"), egui::Button::new("Search"));
            #[cfg(test)]
            crate::workspace_access_tests::record(ui, "search_submit", &submit);
            if submit.clicked() || enter {
                self.search();
            }
            ui.label(
                RichText::new(format!("{} results", self.search_results.len()))
                    .small()
                    .color(MUTED),
            );
        });
        if let Some(source) = &self.search_state.accepted {
            let label = ui.label(RichText::new(format!("Historical results for {:?}. Lines may have changed; Open uses the current buffer.", source.query)).small().color(MUTED));
            #[cfg(test)]
            crate::workspace_access_tests::record(ui, "search_history", &label);
            #[cfg(not(test))]
            let _ = label;
        }
        let active = self.search_rows_active();
        if self.search_state.accepted.is_some() && !active {
            let label = ui.colored_label(
                AMBER,
                "Previous results are inactive. Run Search for the current query and connection.",
            );
            #[cfg(test)]
            crate::workspace_access_tests::record(ui, "search_inactive", &label);
            #[cfg(not(test))]
            let _ = label;
        }
        if self.ready() && self.search_state.pending.is_some() {
            ui.label(RichText::new("Searching...").small().color(MUTED));
        }
        if self.search_truncated {
            let label = ui.colored_label(
                AMBER,
                "Search stopped at an engine limit; more matches may exist. Narrow your query.",
            );
            #[cfg(test)]
            crate::workspace_access_tests::record(ui, "search_truncated", &label);
            #[cfg(not(test))]
            let _ = label;
        }
        let action = if ui.is_sizing_pass() {
            None
        } else {
            self.take_search_action(ui.ctx())
        };
        let click = if ui.is_sizing_pass() {
            None
        } else {
            self.take_search_click(ui.ctx())
        };
        self.search_state.keyboard.presented.clear();
        let source = self.search_state.accepted.as_ref();
        let keyboard = &mut self.search_state.keyboard;
        let mut open = match action {
            Some(Action::Open(index)) => Some(index),
            _ => None,
        };
        let mut revealed = false;
        let scroll = egui::ScrollArea::vertical()
            .id_salt("search_results")
            .animated(false)
            .show(ui, |ui| {
                ui.add_enabled_ui(active, |ui| {
                    for (index, result) in self.search_results.iter().enumerate() {
                        ui.horizontal(|ui| {
                            let id = source.map_or_else(
                                || egui::Id::new(("unowned_search_row", index)),
                                |source| row_id(source, index),
                            );
                            let response =
                                row_control(ui, id, &format!("{}:{}", result.path, result.line));
                            keyboard.record(ui, index, &response);
                            #[cfg(test)]
                            crate::workspace_access_tests::record(
                                ui,
                                &format!("search_row:{index}"),
                                &response,
                            );
                            #[cfg(test)]
                            crate::search_keyboard_tests::record_pointer_response(
                                ui, index, &response, click,
                            );
                            if click == Some(index)
                                && response.clicked_by(egui::PointerButton::Primary)
                            {
                                open = Some(index);
                            }
                            if !ui.is_sizing_pass() && action == Some(Action::Focus(index)) {
                                response.request_focus();
                                response.scroll_to_me(Some(egui::Align::Center));
                                revealed = true;
                            } else if !ui.is_sizing_pass()
                                && action.is_none()
                                && !revealed
                                && response.gained_focus()
                            {
                                response.scroll_to_me(None);
                                revealed = true;
                            }
                            ui.add(
                                egui::Label::new(
                                    RichText::new(result.text.trim())
                                        .monospace()
                                        .size(12.0)
                                        .color(MUTED),
                                )
                                .truncate(),
                            );
                        });
                    }
                });
                if self.search_results.is_empty() {
                    ui.label(
                        RichText::new(
                            "Case-sensitive text search. Open a result to jump to its line",
                        )
                        .small()
                        .color(MUTED),
                    );
                }
            });
        #[cfg(test)]
        crate::search_keyboard_tests::record_scroll(ui.ctx(), scroll.id);
        #[cfg(not(test))]
        let _ = scroll;
        let view = View::of(self);
        if let (Some(view), Some(Action::Focus(index))) = (view, action) {
            let focus = batch_row_id(view.batch, index);
            if ui.ctx().memory(|memory| memory.focused()) == Some(focus) {
                self.search_state.keyboard.render = Some((view, index, focus));
            }
        }
        self.search_state.keyboard.view = view;
        if let Some(index) = open.filter(|_| self.search_input_allowed(ui.ctx())) {
            if let Some(result) = self.search_results.get(index) {
                self.open(result.path.clone(), Some(result.line));
            }
        }
    }
}

// Give the actual native interaction its batch/index ID, preserving Tab order
// and accessibility even for duplicate path/line/snippet triples.
fn row_control(ui: &mut egui::Ui, id: egui::Id, label: &str) -> egui::Response {
    let padding = ui.spacing().button_padding;
    let galley = egui::WidgetText::from(RichText::new(label).color(GREEN)).into_galley(
        ui,
        Some(egui::TextWrapMode::Truncate),
        (ui.available_width() * 0.6 - 2.0 * padding.x).max(0.0),
        egui::TextStyle::Button,
    );
    let (_, rect) = ui.allocate_space(egui::vec2(
        galley.size().x + 2.0 * padding.x,
        (galley.size().y + 2.0 * padding.y).max(ui.spacing().interact_size.y),
    ));
    let response = ui.interact(rect, id, egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    if response.has_focus() {
        ui.memory_mut(|memory| {
            memory.set_focus_lock_filter(
                id,
                egui::EventFilter {
                    vertical_arrows: true,
                    ..Default::default()
                },
            )
        });
    }
    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact(&response);
        if response.hovered() || response.has_focus() {
            ui.painter().rect(
                rect,
                visuals.corner_radius,
                visuals.weak_bg_fill,
                visuals.bg_stroke,
                egui::StrokeKind::Inside,
            );
        }
        ui.painter().galley(rect.min + padding, galley, GREEN);
    }
    response
}
