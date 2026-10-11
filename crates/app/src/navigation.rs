//! Bounded, local-only navigation over buffers and explicitly loaded listings.
use crate::{
    explorer_tree::{FileAdmission, LoadedFile, Mode},
    model::Document,
    CedarApp, MUTED, RED,
};
use cedar_protocol::Entry;
use eframe::egui::{self, RichText};
use std::{collections::HashSet, sync::Arc};

const MAX_RESULTS: usize = 64;
const MAX_QUERY_CHARS: usize = 4096;
const MAX_CACHED_PATHS: usize = 4096;
const MAX_BUFFERS: usize = 32;
const MAX_SNAPSHOT_PATH_BYTES: usize = 1024 * 1024;
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
    mode_epoch: u64,
    mode: Mode,
    serial: u64,
    focus: bool,
    deferred_input: Vec<egui::Event>,
    kind: Kind,
}

enum Kind {
    Files {
        selected: Option<Arc<str>>,
        scope: FileScope,
        snapshot: Vec<Candidate>,
        loaded_directories: usize,
        error: Option<String>,
        refused: bool,
        scope_epoch: u64,
        presented: Vec<Presented>,
        pressed: Option<Presented>,
    },
    Line {
        input: String,
        error: Option<String>,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
enum FileScope {
    #[default]
    Current,
    Loaded,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Source {
    Buffer(u64),
    Current,
    Loaded(FileAdmission),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Candidate {
    path: Arc<str>,
    open: bool,
    source: Source,
}

#[derive(Clone)]
struct Presented {
    candidate: Candidate,
    id: egui::Id,
}

struct Candidates {
    items: Vec<Candidate>,
    truncated: bool,
}

impl Navigation {
    pub fn dialog_open(&self) -> bool {
        self.dialog.is_some()
    }

    pub fn blocks_editor(&self) -> bool {
        self.blocked_frame || self.dialog.is_some()
    }

    pub fn has_cjk(&self) -> bool {
        crate::system_fonts::contains_cjk(&self.query)
    }

    #[cfg(test)]
    pub(crate) fn loaded_scope(&self) -> bool {
        matches!(
            self.dialog.as_ref().map(|dialog| &dialog.kind),
            Some(Kind::Files {
                scope: FileScope::Loaded,
                ..
            })
        )
    }

    #[cfg(test)]
    pub(crate) fn query_text(&self) -> &str {
        &self.query
    }

    #[cfg(test)]
    pub(crate) fn selected_file_path(&self) -> Option<&str> {
        match self.dialog.as_ref().map(|dialog| &dialog.kind) {
            Some(Kind::Files { selected, .. }) => selected.as_deref(),
            _ => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn visible_paths(&self) -> Vec<&str> {
        match self.dialog.as_ref().map(|dialog| &dialog.kind) {
            Some(Kind::Files { presented, .. }) => presented
                .iter()
                .map(|row| row.candidate.path.as_ref())
                .collect(),
            _ => Vec::new(),
        }
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
    for (path, lower, source) in documents
        .iter()
        .map(|doc| (&doc.path, doc.path.to_lowercase(), Source::Buffer(doc.id)))
        .chain(
            files
                .into_iter()
                .map(|(lower, entry)| (&entry.path, lower, Source::Current)),
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
                path: path.as_str().into(),
                open: matches!(source, Source::Buffer(_)),
                source,
            });
        }
    }
    Candidates {
        items,
        truncated: false,
    }
}

fn loaded_snapshot(
    documents: &[Document],
    files: &[LoadedFile<'_>],
) -> Result<Vec<Candidate>, String> {
    let refusal = "Loaded Explorer files refused the whole snapshot: at most 4,096 cached paths, 32 open buffers, and 1 MiB of paths. Use Current directory or Open typed path.";
    if documents.len() > MAX_BUFFERS || files.len() > MAX_CACHED_PATHS {
        return Err(refusal.into());
    }
    // Check the entire source before allocating retained path strings. Count
    // exact UTF-8 bytes, including duplicates, conservatively and with overflow
    // checks; never retain a seemingly useful prefix of an oversized source.
    let mut bytes = 0usize;
    for path in documents
        .iter()
        .map(|doc| doc.path.as_str())
        .chain(files.iter().map(|file| file.path))
    {
        bytes = bytes
            .checked_add(path.len())
            .filter(|bytes| *bytes <= MAX_SNAPSHOT_PATH_BYTES)
            .ok_or(refusal)?;
    }
    let mut seen = HashSet::new();
    let mut items = Vec::new();
    for doc in documents {
        if seen.insert(doc.path.as_str()) {
            items.push(Candidate {
                path: doc.path.as_str().into(),
                open: true,
                source: Source::Buffer(doc.id),
            });
        }
    }
    let mut files: Vec<_> = files.iter().collect();
    files.sort_by(|a, b| {
        a.path
            .to_lowercase()
            .cmp(&b.path.to_lowercase())
            .then_with(|| a.path.cmp(b.path))
    });
    for file in files {
        if seen.insert(file.path) {
            items.push(Candidate {
                path: file.path.into(),
                open: false,
                source: Source::Loaded(file.admission),
            });
        }
    }
    Ok(items)
}

fn snapshot_matches(snapshot: &[Candidate], query: &str) -> Candidates {
    let query = query.to_lowercase();
    let mut matches = snapshot
        .iter()
        .filter(|item| item.path.to_lowercase().contains(&query));
    let items = matches.by_ref().take(MAX_RESULTS).cloned().collect();
    Candidates {
        items,
        truncated: matches.next().is_some(),
    }
}

fn candidate_row(
    ui: &mut egui::Ui,
    id: egui::Id,
    selected: bool,
    label: &str,
    full_width: bool,
) -> egui::Response {
    let padding = ui.spacing().button_padding;
    let galley = egui::WidgetText::from(label).into_galley(
        ui,
        Some(egui::TextWrapMode::Wrap),
        (ui.available_width() - 2.0 * padding.x).max(0.0),
        egui::TextStyle::Button,
    );
    let size = egui::vec2(
        if full_width {
            ui.available_width()
        } else {
            galley.size().x + 2.0 * padding.x
        },
        (galley.size().y + 2.0 * padding.y).max(ui.spacing().interact_size.y),
    );
    let (_, rect) = ui.allocate_space(size);
    let response = ui.interact(rect, id, egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::SelectableLabel, ui.is_enabled(), label)
    });
    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact_selectable(&response, selected);
        if selected || response.hovered() || response.has_focus() {
            ui.painter().rect(
                rect,
                visuals.corner_radius,
                visuals.weak_bg_fill,
                visuals.bg_stroke,
                egui::StrokeKind::Inside,
            );
        }
        ui.painter()
            .galley(rect.min + padding, galley, visuals.text_color());
    }
    response
}

fn selected_index(items: &[Candidate], selected: &mut Option<Arc<str>>) -> Option<usize> {
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
            || self.copy_draft.is_open()
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
            dialog.document != self.active_document
                || dialog.generation != self.generation
                || (matches!(dialog.kind, Kind::Files { .. })
                    && (dialog.mode != self.explorer.mode
                        || dialog.mode_epoch != self.explorer.mode_epoch))
        }) {
            if self
                .navigation
                .dialog
                .as_ref()
                .is_some_and(|dialog| matches!(dialog.kind, Kind::Files { .. }))
            {
                self.error = Some("File chooser canceled because its document, connection, or Explorer mode changed. Open it again to choose a current file.".into());
            }
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
            mode_epoch: self.explorer.mode_epoch,
            mode: self.explorer.mode,
            serial: self.navigation_epoch,
            focus: true,
            deferred_input: Vec::new(),
            kind: Kind::Files {
                selected: None,
                scope: FileScope::Current,
                snapshot: Vec::new(),
                loaded_directories: 0,
                error: None,
                refused: false,
                scope_epoch: 0,
                presented: Vec::new(),
                pressed: None,
            },
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
            mode_epoch: self.explorer.mode_epoch,
            mode: self.explorer.mode,
            serial: self.navigation_epoch,
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

    fn navigation_candidate_current(&self, candidate: &Candidate) -> bool {
        match candidate.source {
            Source::Buffer(id) => self
                .documents
                .iter()
                .any(|doc| doc.id == id && doc.path == candidate.path.as_ref()),
            Source::Current => self
                .explorer_scope_entries()
                .iter()
                .any(|entry| entry.path == candidate.path.as_ref() && !entry.is_dir),
            Source::Loaded(admission) => {
                self.explorer_loaded_file_current(&candidate.path, admission)
            }
        }
    }

    fn navigation_candidate_label(&self, candidate: &Candidate) -> String {
        match candidate.source {
            Source::Buffer(_) => format!("{}  ·  open", candidate.path),
            Source::Loaded(admission) => {
                match self.explorer_loaded_file_stale(&candidate.path, admission) {
                    Some(true) => format!(
                        "{}  ·  stale listing; disk state unverified",
                        candidate.path
                    ),
                    Some(false) => candidate.path.to_string(),
                    None => format!("{}  ·  listing changed; re-enter scope", candidate.path),
                }
            }
            Source::Current => candidate.path.to_string(),
        }
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
        let scope_focused = ctx.memory(|memory| {
            memory.focused().is_some_and(|id| {
                id == egui::Id::new("navigation_scope_current")
                    || id == egui::Id::new("navigation_scope_loaded")
            })
        });
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
                        && (!scope_focused || modifiers.matches_logically(egui::Modifiers::COMMAND))
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
        let mut open_candidate = None;
        let mut jump_line = None;
        let mut dismiss = escape;
        let modal = egui::Modal::new(egui::Id::new("navigation_modal")).show(ctx, |ui| {
            ui.set_width((ctx.screen_rect().width() - 64.0).clamp(240.0, 560.0));
            match &mut dialog.kind {
                Kind::Files { selected, scope, snapshot, loaded_directories, error, refused, scope_epoch, presented, pressed } => {
                    // Preserve the identity that was actually displayed before a
                    // List reply or buffer update in this frame can move rows.
                    let captured_enter = selected.as_ref().and_then(|path| presented.iter().find(|row| &row.candidate.path == path)).map(|row| row.candidate.clone());
                    let mut captured_click = None;
                    let (primary_press, primary_release, competing) = ctx.input(|input| {
                        let press = input.raw.events.iter().any(|event| matches!(event, egui::Event::PointerButton { button: egui::PointerButton::Primary, pressed: true, .. }));
                        let release = input.raw.events.iter().any(|event| matches!(event, egui::Event::PointerButton { button: egui::PointerButton::Primary, pressed: false, .. }));
                        let competing = !input.focused || input.raw.events.iter().any(|event| is_keyboard(event) || matches!(event, egui::Event::WindowFocused(false) | egui::Event::MouseWheel { .. } | egui::Event::Zoom(_) | egui::Event::Touch { .. }));
                        (press, release, competing)
                    });
                    if competing {
                        *pressed = None;
                    } else {
                        let clicked = ctx.interaction_snapshot(|interaction| interaction.clicked);
                        if primary_press {
                            *pressed = presented.iter().find(|row| ctx.read_response(row.id).is_some_and(|response| response.enabled() && (response.is_pointer_button_down_on() || (clicked == Some(row.id) && response.clicked_by(egui::PointerButton::Primary))))).cloned();
                        }
                        if primary_release {
                            if let Some(row) = pressed.take() {
                                if clicked == Some(row.id) && ctx.read_response(row.id).is_some_and(|response| response.enabled() && response.clicked_by(egui::PointerButton::Primary)) {
                                    captured_click = Some(row.candidate);
                                } else if !self.navigation_candidate_current(&row.candidate) {
                                    *error = Some("Selection canceled: the pressed file or buffer changed. Choose again.".into());
                                }
                            }
                        }
                    }
                    ui.heading("Open file");
                    let previous_scope = *scope;
                    ui.horizontal(|ui| {
                        for (choice, label, name) in [(FileScope::Current, "Current directory", "navigation_scope_current"), (FileScope::Loaded, "Loaded Explorer files", "navigation_scope_loaded")] {
                            let response = candidate_row(ui, egui::Id::new(name), *scope == choice, label, false);
                            #[cfg(test)]
                            crate::workspace_access_tests::record(ui, name, &response);
                            if response.clicked() {
                                *scope = choice;
                            }
                        }
                    });
                    let scope_changed = previous_scope != *scope;
                    if scope_changed {
                        *scope_epoch = scope_epoch.saturating_add(1);
                        *selected = None;
                        *pressed = None;
                        captured_click = None;
                        *error = None;
                        *refused = false;
                        snapshot.clear();
                        presented.clear();
                        if *scope == FileScope::Loaded {
                            *loaded_directories = self.explorer_loaded_directory_count();
                            match self.explorer_loaded_files().and_then(|files| loaded_snapshot(&self.documents, &files)) {
                                Ok(items) => *snapshot = items,
                                Err(message) => {
                                    *error = Some(message);
                                    *refused = true;
                                }
                            }
                        }
                        // Native Space activation can also emit Text(" ").
                        // The control owns this batch; it must not become a
                        // query edit when we return focus to the TextEdit.
                        discard_keyboard(ctx);
                        arrows.clear();
                        dialog.focus = true;
                    }
                    if *scope == FileScope::Current {
                        ui.label(RichText::new(format!("Open buffers and files in directory scope: /{}", self.explorer_scope())).small().color(MUTED));
                        if !self.explorer_scope_loaded() {
                            ui.label(RichText::new("This directory is not loaded. Expand it or Refresh in Explorer to include its files; open buffers are still available.").small().color(MUTED));
                        }
                    } else {
                        ui.label(RichText::new(if *loaded_directories == 0 { "No directories are loaded in this Explorer mode. This snapshot contains open buffers only; expand or Refresh in Explorer, then re-enter this scope.".to_string() } else { format!("Snapshot of open buffers and {loaded_directories} loaded directories in this Explorer mode. No folders are scanned. Re-enter this scope to update it.") }).small().color(MUTED));
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
                        if !*refused { *error = None; }
                    }
                    let result = if *scope == FileScope::Loaded { snapshot_matches(snapshot, &self.navigation.query) } else { candidates(&self.documents, self.explorer_scope_entries(), &self.navigation.query) };
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
                    ui.label(RichText::new("Tab / Shift+Tab changes control · Space chooses scope · ↑/↓ choose file · Enter opens selection · Ctrl/Cmd+Enter opens typed path").small().color(MUTED));
                    presented.clear();
                    egui::ScrollArea::vertical().id_salt(("navigation_files", *scope_epoch)).max_height((ctx.screen_rect().height() - 330.0).clamp(80.0, 250.0)).show(ui, |ui| {
                        for (row, item) in result.items.iter().enumerate() {
                            let label = self.navigation_candidate_label(item);
                            let id = egui::Id::new(("navigation_file_row", dialog.serial, dialog.generation, dialog.mode_epoch, *scope_epoch, &item.path, &item.source));
                            let response = candidate_row(ui, id, index == Some(row), &label, true);
                            #[cfg(test)]
                            crate::workspace_access_tests::record(ui, &format!("navigation_file:{}", item.path), &response);
                            presented.push(Presented { candidate: item.clone(), id: response.id });
                            if index == Some(row) && (!arrows.is_empty() || query_changed) {
                                response.scroll_to_me(Some(egui::Align::Center));
                            }
                            // Pointer actions use the prior displayed row above,
                            // including a row removed before its release frame.
                            if response.clicked() && !response.clicked_by(egui::PointerButton::Primary) {
                                open_candidate = Some(item.clone());
                            }
                        }
                        if result.items.is_empty() {
                            ui.label(RichText::new(if *scope == FileScope::Loaded { "No matching buffers or loaded Explorer files" } else { "No matching buffers or current-directory files" }).color(MUTED));
                        }
                    });
                    if let Some(candidate) = captured_click {
                        open_candidate = Some(candidate);
                    }
                    ui.label(RichText::new(if result.truncated {
                        "Showing the first 64 matches; type more to narrow the list. No subfolders are scanned."
                    } else {
                        "Up to 64 matches, open buffers first. No subfolders are scanned."
                    }).small().color(MUTED));
                    ui.horizontal(|ui| {
                        let path = typed_path(&self.navigation.query);
                        let can_open = !path.is_empty() && (self.ready() || self.documents.iter().any(|doc| doc.path == path));
                        let typed = ui.add_enabled(can_open, egui::Button::new("Open typed path")).clicked();
                        if can_open && (typed || explicit || (enter && index.is_none() && captured_enter.is_none() && *scope == FileScope::Current)) {
                            open_path = Some(path);
                        } else if enter && !explicit && !scope_changed {
                            open_candidate = if query_changed || !arrows.is_empty() {
                                index.map(|index| result.items[index].clone())
                            } else {
                                captured_enter.or_else(|| index.map(|index| result.items[index].clone()))
                            };
                        }
                        dismiss |= ui.button("Cancel").clicked();
                    });
                    if let Some(error) = error {
                        ui.colored_label(RED, error.as_str());
                    }
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
        } else if let Some(candidate) = open_candidate {
            if !self.navigation_candidate_current(&candidate) {
                if let Some(Dialog {
                    kind: Kind::Files { error, .. },
                    ..
                }) = self.navigation.dialog.as_mut()
                {
                    *error = Some("Selection canceled: this file or open buffer changed since it was displayed. Choose again, or re-enter Loaded Explorer files to update its snapshot.".into());
                }
            } else if !candidate.open
                && !self.ready()
                && !self
                    .documents
                    .iter()
                    .any(|doc| doc.path == candidate.path.as_ref())
            {
                if let Some(Dialog {
                    kind: Kind::Files { error, .. },
                    ..
                }) = self.navigation.dialog.as_mut()
                {
                    *error = Some("Connect the workspace before opening an unopened file. Open buffers are still available.".into());
                }
            } else {
                self.dismiss_navigation();
                self.open(candidate.path.to_string(), None);
            }
        } else if let Some(line) = jump_line {
            self.history_go_to_line(line);
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
