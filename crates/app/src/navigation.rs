//! Bounded, local-only navigation over buffers and the current directory listing.
use crate::{model::Document, CedarApp, MUTED, RED};
use cedar_protocol::Entry;
use eframe::egui::{self, RichText};
use std::collections::HashSet;

const MAX_RESULTS: usize = 64;
const MAX_QUERY_CHARS: usize = 4096;
const FILE_INPUT: &str = "navigation_file_query";
const LINE_INPUT: &str = "navigation_line_query";

#[derive(Default)]
pub(super) struct Navigation {
    dialog: Option<Dialog>,
    // Keep explicit typed-path input available when reopening, as before.
    query: String,
    pub restore_focus: bool,
    blocked_frame: bool,
}

struct Dialog {
    document: Option<u64>,
    generation: u64,
    focus: bool,
    deferred_input: Vec<egui::Event>,
    kind: Kind,
}

enum Kind {
    Files {
        selected: Option<String>,
    },
    Line {
        input: String,
        error: Option<String>,
    },
}

#[derive(Debug, PartialEq, Eq)]
struct Candidate {
    path: String,
    open: bool,
}

struct Candidates {
    items: Vec<Candidate>,
    truncated: bool,
}

impl Navigation {
    pub fn blocks_editor(&self) -> bool {
        self.blocked_frame || self.dialog.is_some()
    }

    pub fn has_cjk(&self) -> bool {
        crate::system_fonts::contains_cjk(&self.query)
    }
}

fn candidates(documents: &[Document], entries: &[Entry], query: &str) -> Candidates {
    let query = query.to_lowercase();
    let mut seen = HashSet::new();
    let mut items = Vec::new();
    let mut files: Vec<_> = entries
        .iter()
        .filter(|entry| !entry.is_dir)
        .map(|entry| (entry.path.to_lowercase(), entry))
        .collect();
    files.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.path.cmp(&b.1.path)));
    for (path, lower, open) in documents
        .iter()
        .map(|doc| (&doc.path, doc.path.to_lowercase(), true))
        .chain(
            files
                .into_iter()
                .map(|(lower, entry)| (&entry.path, lower, false)),
        )
    {
        if lower.contains(&query) && seen.insert(path) {
            if items.len() == MAX_RESULTS {
                return Candidates {
                    items,
                    truncated: true,
                };
            }
            items.push(Candidate {
                path: path.clone(),
                open,
            });
        }
    }
    Candidates {
        items,
        truncated: false,
    }
}

fn selected_index(items: &[Candidate], selected: &mut Option<String>) -> Option<usize> {
    let index = selected
        .as_ref()
        .and_then(|path| items.iter().position(|item| &item.path == path))
        .or_else(|| (!items.is_empty()).then_some(0));
    *selected = index.map(|index| items[index].path.clone());
    index
}

fn line_count(text: &str) -> usize {
    text.bytes().filter(|byte| *byte == b'\n').count() + 1
}

fn parse_line(input: &str, text: &str) -> Result<usize, String> {
    let count = line_count(text);
    let input = input.trim();
    if !input.is_empty() && input.bytes().all(|byte| byte.is_ascii_digit()) {
        if let Ok(line) = input.parse::<usize>() {
            if (1..=count).contains(&line) {
                return Ok(line);
            }
        }
    }
    Err(format!("Enter a whole line number from 1 to {count}"))
}

fn typed_path(input: &str) -> String {
    // This normalization belongs only to the explicitly typed-path action.
    input.trim().replace('\\', "/")
}

fn is_keyboard(event: &egui::Event) -> bool {
    matches!(
        event,
        egui::Event::Key { .. }
            | egui::Event::Text(_)
            | egui::Event::Paste(_)
            | egui::Event::Copy
            | egui::Event::Cut
            | egui::Event::Ime(_)
    )
}

fn discard_keyboard(ctx: &egui::Context) {
    ctx.input_mut(|input| {
        input.events.retain(|event| !is_keyboard(event));
    });
}

fn defer_initial_input(ctx: &egui::Context) -> Vec<egui::Event> {
    // Only the first, disabled egui Area sizing pass needs replay. Keep this
    // temporary queue finite, even for a very large paste or synthetic batch.
    let mut remaining_chars = MAX_QUERY_CHARS;
    ctx.input(|input| {
        input
            .events
            .iter()
            .filter(|event| is_keyboard(event))
            .take(128)
            .map(|event| match event {
                egui::Event::Text(text)
                | egui::Event::Paste(text)
                | egui::Event::Ime(egui::ImeEvent::Preedit(text))
                | egui::Event::Ime(egui::ImeEvent::Commit(text)) => {
                    let text: String = text.chars().take(remaining_chars).collect();
                    remaining_chars -= text.chars().count();
                    match event {
                        egui::Event::Paste(_) => egui::Event::Paste(text),
                        egui::Event::Ime(egui::ImeEvent::Preedit(_)) => {
                            egui::Event::Ime(egui::ImeEvent::Preedit(text))
                        }
                        egui::Event::Ime(egui::ImeEvent::Commit(_)) => {
                            egui::Event::Ime(egui::ImeEvent::Commit(text))
                        }
                        _ => egui::Event::Text(text),
                    }
                }
                _ => event.clone(),
            })
            .collect()
    })
}

impl CedarApp {
    fn foreign_modal_pending(&self) -> bool {
        self.confirm.is_some()
            || self.recovery.remove_confirmation.is_some()
            || self.run_state.transition_pending()
    }

    pub(super) fn foreign_modal_owns_input(&self, ctx: &egui::Context) -> bool {
        self.foreign_modal_pending()
            || ctx.memory(|memory| {
                memory.top_modal_layer().is_some_and(|layer| {
                    layer
                        != egui::LayerId::new(
                            egui::Order::Foreground,
                            egui::Id::new("navigation_modal"),
                        )
                })
            })
    }

    pub(super) fn dismiss_navigation(&mut self) {
        if self.navigation.dialog.take().is_some() {
            self.navigation.blocked_frame = true;
            self.navigation.restore_focus = true;
        }
    }

    pub(super) fn begin_navigation_frame(&mut self, ctx: &egui::Context) {
        // egui keeps the previous frame's modal layer through this frame. Wait
        // for its release before focusing the editor or consuming a cursor jump.
        let old_modal = ctx.memory(|memory| memory.top_modal_layer().is_some());
        self.navigation.blocked_frame = self.navigation.dialog.is_some() || old_modal;
        if self.navigation.dialog.as_ref().is_some_and(|dialog| {
            dialog.document != self.active_document || dialog.generation != self.generation
        }) {
            self.dismiss_navigation();
            // Input queued for the old document must not act on its replacement.
            discard_keyboard(ctx);
        }
        if self.navigation.restore_focus && !self.navigation.blocks_editor() {
            if let Some(document) = self.active_document {
                ctx.memory_mut(|memory| {
                    memory.request_focus(egui::Id::new(("editor", document)));
                });
            }
            self.navigation.restore_focus = false;
        } else if self.navigation.restore_focus
            && self.navigation.dialog.is_none()
            && !self.foreign_modal_pending()
        {
            ctx.request_repaint();
        }
    }

    pub(super) fn show_file_chooser(&mut self) {
        if let Some(Dialog {
            kind: Kind::Files { .. },
            focus,
            ..
        }) = self.navigation.dialog.as_mut()
        {
            *focus = true;
            return;
        }
        self.navigation_changed();
        self.navigation.restore_focus = false;
        self.navigation.dialog = Some(Dialog {
            document: self.active_document,
            generation: self.generation,
            focus: true,
            deferred_input: Vec::new(),
            kind: Kind::Files { selected: None },
        });
        self.navigation.blocked_frame = true;
    }

    fn show_line_chooser(&mut self) {
        if let Some(Dialog {
            kind: Kind::Line { .. },
            focus,
            ..
        }) = self.navigation.dialog.as_mut()
        {
            *focus = true;
            return;
        }
        let Some(document) = self.active() else {
            return;
        };
        let input = document.cursor.0.to_string();
        self.navigation_changed();
        self.navigation.restore_focus = false;
        self.navigation.dialog = Some(Dialog {
            document: self.active_document,
            generation: self.generation,
            focus: true,
            deferred_input: Vec::new(),
            kind: Kind::Line { input, error: None },
        });
        self.navigation.blocked_frame = true;
    }

    pub(super) fn navigation_shortcuts(&mut self, ctx: &egui::Context) -> bool {
        if self.foreign_modal_owns_input(ctx) {
            self.dismiss_navigation();
            self.navigation.blocked_frame = true;
            return true;
        }
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::P)) {
            self.show_file_chooser();
        }
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::G)) {
            self.show_line_chooser();
        }
        self.navigation.blocks_editor()
    }

    pub(super) fn navigation_window(&mut self, ctx: &egui::Context) {
        if self.foreign_modal_owns_input(ctx) {
            self.dismiss_navigation();
            self.navigation.blocked_frame = true;
            return;
        }
        let Some(mut dialog) = self.navigation.dialog.take() else {
            return;
        };
        self.navigation.blocked_frame = true;
        if !dialog.deferred_input.is_empty() {
            ctx.input_mut(|input| {
                let mut events = std::mem::take(&mut dialog.deferred_input);
                events.append(&mut input.events);
                input.events = events;
            });
        }
        let sizing = egui::AreaState::load(ctx, egui::Id::new("navigation_modal"))
            .is_none_or(|state| state.size.is_none());
        if sizing {
            dialog.deferred_input = defer_initial_input(ctx);
            discard_keyboard(ctx);
        }
        let mut escape = false;
        let mut explicit = false;
        let mut enter = false;
        let mut arrows = Vec::new();
        ctx.input_mut(|input| {
            let mut finished = false;
            input.events.retain(|event| {
                if finished && is_keyboard(event) {
                    return false;
                }
                if let egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } = event
                {
                    if *key == egui::Key::Escape && modifiers.is_none() {
                        escape = true;
                        finished = true;
                        return false;
                    }
                    if *key == egui::Key::Enter
                        && (modifiers.is_none()
                            || modifiers.matches_logically(egui::Modifiers::COMMAND))
                    {
                        explicit = modifiers.matches_logically(egui::Modifiers::COMMAND);
                        enter = !explicit;
                        finished = true;
                        return false;
                    }
                    if modifiers.is_none()
                        && matches!(key, egui::Key::ArrowUp | egui::Key::ArrowDown)
                    {
                        arrows.push(*key);
                        return false;
                    }
                }
                true
            });
        });
        if !arrows.is_empty() {
            // egui's spatial focus navigation sees arrow keys at begin_pass,
            // before we consume them. Keep filter typing in the query field.
            dialog.focus = true;
        }
        let mut open_path = None;
        let mut jump_line = None;
        let mut dismiss = escape;
        let modal = egui::Modal::new(egui::Id::new("navigation_modal")).show(ctx, |ui| {
            ui.set_width((ctx.screen_rect().width() - 64.0).clamp(240.0, 560.0));
            match &mut dialog.kind {
                Kind::Files { selected } => {
                    ui.heading("Open file");
                    ui.label(RichText::new(format!("Open buffers and files in directory scope: /{}", self.explorer_scope())).small().color(MUTED));
                    if !self.explorer_scope_loaded() {
                        ui.label(RichText::new("This directory is not loaded. Expand it or Refresh in Explorer to include its files; open buffers are still available.").small().color(MUTED));
                    }
                    let id = egui::Id::new(FILE_INPUT);
                    if dialog.focus && !ui.is_sizing_pass() {
                        ui.ctx().memory_mut(|memory| memory.request_focus(id));
                        dialog.focus = false;
                    }
                    let response = ui.add(egui::TextEdit::singleline(&mut self.navigation.query)
                        .id(id).hint_text("Filter files, or type a workspace-relative path")
                        .char_limit(MAX_QUERY_CHARS)
                        .desired_width(f32::INFINITY));
                    let query_changed = response.changed();
                    if query_changed {
                        *selected = None;
                    }
                    let result = candidates(&self.documents, self.explorer_scope_entries(), &self.navigation.query);
                    let mut index = selected_index(&result.items, selected);
                    for key in &arrows {
                        if let Some(current) = index {
                            index = Some(if *key == egui::Key::ArrowUp {
                                current.saturating_sub(1)
                            } else {
                                (current + 1).min(result.items.len() - 1)
                            });
                        }
                    }
                    *selected = index.map(|index| result.items[index].path.clone());
                    ui.label(RichText::new("↑/↓ choose · Enter opens selection · Ctrl/Cmd+Enter opens typed path").small().color(MUTED));
                    egui::ScrollArea::vertical().id_salt("navigation_files").max_height((ctx.screen_rect().height() - 300.0).clamp(80.0, 280.0)).show(ui, |ui| {
                        for (row, item) in result.items.iter().enumerate() {
                            let label = if item.open { format!("{}  ·  open", item.path) } else { item.path.clone() };
                            let response = ui.selectable_label(index == Some(row), label);
                            if index == Some(row) && (!arrows.is_empty() || query_changed) {
                                response.scroll_to_me(Some(egui::Align::Center));
                            }
                            if response.clicked() {
                                open_path = Some(item.path.clone());
                            }
                        }
                        if result.items.is_empty() {
                            ui.label(RichText::new("No matching buffers or current-directory files").color(MUTED));
                        }
                    });
                    ui.label(RichText::new(if result.truncated {
                        "Showing the first 64 matches; type more to narrow the list. No subfolders are scanned."
                    } else {
                        "Up to 64 matches, open buffers first. No subfolders are scanned."
                    }).small().color(MUTED));
                    ui.horizontal(|ui| {
                        let path = typed_path(&self.navigation.query);
                        let can_open = !path.is_empty() && (self.ready() || self.documents.iter().any(|doc| doc.path == path));
                        let typed = ui.add_enabled(can_open, egui::Button::new("Open typed path")).clicked();
                        if can_open && (typed || explicit || (enter && index.is_none())) {
                            open_path = Some(path);
                        } else if enter && !explicit {
                            open_path = index.map(|index| result.items[index].path.clone());
                        }
                        dismiss |= ui.button("Cancel").clicked();
                    });
                }
                Kind::Line { input, error } => {
                    ui.heading("Go to line");
                    if let Some(doc) = self.active() {
                        ui.label(&doc.path);
                        ui.label(RichText::new(format!("Line 1–{} · Ctrl/Cmd+G", line_count(&doc.text))).small().color(MUTED));
                    }
                    let id = egui::Id::new(LINE_INPUT);
                    if dialog.focus && !ui.is_sizing_pass() {
                        ui.ctx().memory_mut(|memory| memory.request_focus(id));
                        let mut state = egui::TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
                        state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(input.chars().count()))));
                        state.store(ui.ctx(), id);
                        dialog.focus = false;
                    }
                    if ui.add(egui::TextEdit::singleline(input).id(id).char_limit(20).desired_width(f32::INFINITY)).changed() {
                        *error = None;
                    }
                    ui.horizontal(|ui| {
                        if ui.button("Go").clicked() || enter || explicit {
                            if let Some(doc) = self.active() {
                                match parse_line(input, &doc.text) {
                                    Ok(line) => jump_line = Some(line),
                                    Err(message) => *error = Some(message),
                                }
                            }
                        }
                        dismiss |= ui.button("Cancel").clicked();
                    });
                    if let Some(error) = error {
                        ui.colored_label(RED, error.as_str());
                    }
                }
            }
        });
        dismiss |= modal.backdrop_response.clicked();
        // TextEdit focus alone is insufficient on the opening/closing frame:
        // all modal keyboard input must be gone before the editor is rendered.
        discard_keyboard(ctx);
        if !arrows.is_empty() {
            // Native spatial navigation can move focus again at end_pass. A
            // scheduled frame restores the query before any subsequent typing.
            dialog.focus = true;
            ctx.request_repaint();
        }
        let input_id = match &dialog.kind {
            Kind::Files { .. } => FILE_INPUT,
            Kind::Line { .. } => LINE_INPUT,
        };
        self.navigation.dialog = Some(dialog);
        if sizing {
            // The disabled sizing widget surrendered focus. Establish it now
            // so the real TextEdit can install its arrow-key focus filter.
            ctx.memory_mut(|memory| memory.request_focus(egui::Id::new(input_id)));
            ctx.request_discard("Initialize navigation dialog before handling its keyboard input");
            ctx.request_repaint();
            return;
        }
        if dismiss {
            self.dismiss_navigation();
        } else if let Some(path) = open_path {
            self.dismiss_navigation();
            self.open(path, None);
        } else if let Some(line) = jump_line {
            self.navigation_changed();
            if let Some(doc) = self
                .documents
                .iter_mut()
                .find(|doc| Some(doc.id) == self.active_document)
            {
                doc.jump_to = Some(crate::model::line_start(&doc.text, line));
            }
        }
        if self.navigation.restore_focus
            && self.navigation.dialog.is_none()
            && !self.foreign_modal_pending()
        {
            ctx.request_repaint();
        }
    }
}

#[cfg(test)]
mod tests;
