//! Literal, active-buffer replacement. A preview never authorizes a later draft.
use crate::{editor_state, model, CedarApp, ConnectionState, WorkspaceKey, GREEN, MUTED, RED};
use cedar_protocol::MAX_FILE_BYTES;
use cedar_recovery::WorkspaceIdentity;
use eframe::egui::{self, RichText};
use std::{ops::Range, sync::Arc};

pub(super) const FIND_INPUT: &str = "literal_find_query";
const REPLACE_INPUT: &str = "literal_replace_text";
const PREVIEW_ROWS: usize = 3;
const PREVIEW_CHARS: usize = 120;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scope {
    One,
    All,
    Selection,
}

enum Action {
    Preview(Box<Preview>, Option<Activation>),
    Apply(Option<Activation>),
}

#[derive(Clone)]
struct Activation {
    pass: u64,
    owner: Arc<InputOwner>,
}

struct SelectionInput {
    pass: u64,
    focus: Option<egui::Id>,
    pointer: Option<PointerInput>,
    keyboard: bool,
    owner: Option<Arc<InputOwner>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PointerInput {
    Press,
    Release,
    Click,
}

#[derive(PartialEq, Eq)]
struct InputOwner {
    generation: u64,
    navigation: u64,
    document: u64,
    version: u64,
    path: String,
    source: String,
    selection: (usize, bool, usize, bool),
    query: String,
    replacement: String,
    workspace: Option<WorkspaceIdentity>,
    workspace_key: Option<WorkspaceKey>,
    root: String,
    connection: ConnectionState,
}

fn selection_identity(cursor: egui::text::CCursorRange) -> (usize, bool, usize, bool) {
    (
        cursor.primary.index,
        cursor.primary.prefer_next_row,
        cursor.secondary.index,
        cursor.secondary.prefer_next_row,
    )
}

impl InputOwner {
    fn capture(app: &CedarApp, ctx: &egui::Context) -> Option<Arc<Self>> {
        let doc = app.active()?;
        if doc.text.len() > MAX_FILE_BYTES
            || app.find_query.len() > MAX_FILE_BYTES
            || app.replace.replacement.len() > MAX_FILE_BYTES
        {
            return None;
        }
        Some(Arc::new(Self {
            generation: app.generation,
            navigation: app.navigation_epoch,
            document: doc.id,
            version: doc.edit_version,
            path: doc.path.clone(),
            source: doc.text.clone(),
            selection: selection_identity(stored_selection(ctx, doc.id)?),
            query: app.find_query.clone(),
            replacement: app.replace.replacement.clone(),
            workspace: app.recovery_workspace(),
            workspace_key: app.workspace_key.clone(),
            root: app.root.clone(),
            connection: app.state,
        }))
    }

    fn current(&self, app: &CedarApp, ctx: &egui::Context) -> bool {
        app.find_open
            && self.generation == app.generation
            && self.navigation == app.navigation_epoch
            && self.query == app.find_query
            && self.replacement == app.replace.replacement
            && self.workspace == app.recovery_workspace()
            && self.workspace_key == app.workspace_key
            && self.root == app.root
            && self.connection == app.state
            && app.active().is_some_and(|doc| {
                self.document == doc.id
                    && self.version == doc.edit_version
                    && self.path == doc.path
                    && self.source == doc.text
                    && doc.jump_to.is_none()
                    && stored_selection(ctx, doc.id)
                        .is_some_and(|cursor| self.selection == selection_identity(cursor))
            })
    }
}

impl Activation {
    fn current(&self, app: &CedarApp, ctx: &egui::Context) -> bool {
        self.pass == input_pass(ctx) && self.owner.current(app, ctx)
    }
}

fn input_pass(ctx: &egui::Context) -> u64 {
    // The hook precedes ctx.run; sizing retries still belong to that same run.
    ctx.cumulative_pass_nr()
        .saturating_sub(ctx.output(|output| output.num_completed_passes) as u64)
}

#[derive(Default)]
pub(super) struct Replace {
    pub replacement: String,
    preview: Option<Preview>,
    error: Option<String>,
    action: Option<Action>,
    selection_input: Option<SelectionInput>,
    selection_press: Option<(egui::Id, Arc<InputOwner>)>,
    selection_button: Option<egui::Id>,
    apply_button: Option<egui::Id>,
}

struct Preview {
    document: u64,
    path: String,
    generation: u64,
    workspace: Option<WorkspaceIdentity>,
    workspace_key: Option<WorkspaceKey>,
    root: String,
    connection: ConnectionState,
    navigation: u64,
    version: u64,
    source: String,
    selection: egui::text::CCursorRange,
    query: String,
    replacement: String,
    scope: Scope,
    plan: Plan,
}

#[derive(Debug, PartialEq, Eq)]
struct Plan {
    text: String,
    ranges: Vec<Range<usize>>,
    cursor_chars: usize,
}

fn stored_selection(ctx: &egui::Context, document: u64) -> Option<egui::text::CCursorRange> {
    egui::TextEdit::load_state(ctx, egui::Id::new(("editor", document)))
        .and_then(|state| state.cursor.char_range())
}

fn selection(ctx: &egui::Context, document: u64) -> egui::text::CCursorRange {
    stored_selection(ctx, document)
        .unwrap_or_else(|| egui::text::CCursorRange::one(egui::text::CCursor::new(0)))
}

// egui compares cursor indices only. Both affinities belong to the reviewed
// selection and must be checked even when its character endpoints are equal.
fn same_selection(a: egui::text::CCursorRange, b: egui::text::CCursorRange) -> bool {
    a == b
        && a.primary.prefer_next_row == b.primary.prefer_next_row
        && a.secondary.prefer_next_row == b.secondary.prefer_next_row
}

/// Match positions are Unicode scalar offsets, just like Find and egui. Byte
/// offsets used to assemble text come from the same non-overlapping matcher.
fn plan(
    source: &str,
    query: &str,
    replacement: &str,
    cursor: egui::text::CCursorRange,
    scope: Scope,
) -> Result<Plan, String> {
    if query.is_empty() {
        return Err("Enter nonempty case-sensitive text to find".into());
    }
    if source.len() > MAX_FILE_BYTES || replacement.len() > MAX_FILE_BYTES {
        return Err("Replace source and replacement must each fit within 1 MiB".into());
    }
    if replacement.contains('\0') {
        return Err("Replacement contains NUL".into());
    }
    let chars = source.chars().count();
    if cursor.primary.index > chars || cursor.secondary.index > chars {
        return Err("Replace selection is outside the document".into());
    }
    let (haystack, byte_offset, char_offset) = if scope == Scope::Selection {
        let [start, end] = cursor.sorted();
        if start.index == end.index {
            return Err("Select nonempty text in the active file before Preview selection".into());
        }
        let byte_at = |index| {
            source
                .char_indices()
                .map(|(byte, _)| byte)
                .chain(std::iter::once(source.len()))
                .nth(index)
                .expect("selection bounds checked above")
        };
        let start_byte = byte_at(start.index);
        let end_byte = byte_at(end.index);
        (&source[start_byte..end_byte], start_byte, start.index)
    } else {
        (source, 0, 0)
    };
    // find_ranges deliberately caps its list. Never use that capped list to
    // silently authorize a partial Replace all (or misidentify a later match).
    // Selection matches start at the slice boundary, so neither outside
    // matches nor whole-document overlap alignment can affect this action.
    if haystack
        .match_indices(query)
        .nth(model::MAX_FIND_MATCHES)
        .is_some()
    {
        return Err("Replace exceeds 10,000 matches. Narrow the Find text".into());
    }
    let mut ranges = model::find_ranges(haystack, query);
    if ranges.is_empty() {
        return Err(if scope == Scope::Selection {
            "No matching text in the selected text"
        } else {
            "No matching text in the active file"
        }
        .into());
    }
    for range in &mut ranges {
        range.start += char_offset;
        range.end += char_offset;
    }
    let chosen = if scope == Scope::One {
        let [start, end] = cursor.sorted();
        let selected = start.index..end.index;
        let index = ranges
            .iter()
            .position(|range| *range == selected)
            .or_else(|| {
                ranges
                    .iter()
                    .position(|range| range.start >= cursor.primary.index)
            })
            .unwrap_or(0);
        let chosen = ranges[index].clone();
        ranges.clear();
        ranges.push(chosen);
        Some(index)
    } else {
        None
    };
    let removed = query
        .len()
        .checked_mul(ranges.len())
        .ok_or("Replace size overflow")?;
    let inserted = replacement
        .len()
        .checked_mul(ranges.len())
        .ok_or("Replace size overflow")?;
    let bytes = source
        .len()
        .checked_sub(removed)
        .and_then(|bytes| bytes.checked_add(inserted))
        .filter(|bytes| *bytes <= MAX_FILE_BYTES)
        .ok_or("Replaced document would exceed 1 MiB")?;
    let mut text = String::new();
    text.try_reserve_exact(bytes)
        .map_err(|_| "Cannot allocate replaced document")?;
    let mut previous = 0;
    for (index, (start, _)) in haystack.match_indices(query).enumerate() {
        if chosen.is_some_and(|chosen| chosen != index) {
            continue;
        }
        let start = start + byte_offset;
        text.push_str(&source[previous..start]);
        text.push_str(replacement);
        previous = start + query.len();
        if chosen.is_some() {
            break;
        }
    }
    text.push_str(&source[previous..]);
    let mut cursor_chars = ranges[0].start + replacement.chars().count();
    // Literal CR/LF bytes stay exact. If a replacement joins a CR to an LF,
    // keep the resulting cursor outside the middle of the CRLF terminator.
    let byte = text
        .char_indices()
        .map(|(byte, _)| byte)
        .chain(std::iter::once(text.len()))
        .nth(cursor_chars)
        .unwrap_or(text.len());
    if byte > 0
        && text.as_bytes().get(byte - 1) == Some(&b'\r')
        && text.as_bytes().get(byte) == Some(&b'\n')
    {
        cursor_chars += 1;
    }
    Ok(Plan {
        text,
        ranges,
        cursor_chars,
    })
}

/// Escape controls and bound rendering without shortening the applied result.
fn excerpt(text: &str) -> String {
    let mut chars = text.chars();
    let mut output: String = chars
        .by_ref()
        .take(PREVIEW_CHARS)
        .flat_map(char::escape_debug)
        .collect();
    if chars.next().is_some() {
        output.push('…');
    }
    output
}

pub(super) fn discard_keyboard(ctx: &egui::Context) {
    ctx.input_mut(|input| {
        input.events.retain(|event| {
            !matches!(
                event,
                egui::Event::Key { .. }
                    | egui::Event::Text(_)
                    | egui::Event::Paste(_)
                    | egui::Event::Copy
                    | egui::Event::Cut
                    | egui::Event::Ime(_)
            )
        })
    });
}

impl Preview {
    fn current(&self, app: &CedarApp, ctx: &egui::Context) -> bool {
        app.find_open
            && self.generation == app.generation
            && self.workspace == app.recovery_workspace()
            && self.workspace_key == app.workspace_key
            && self.root == app.root
            && self.connection == app.state
            && self.navigation == app.navigation_epoch
            && self.query == app.find_query
            && self.replacement == app.replace.replacement
            && app.active().is_some_and(|doc| {
                self.document == doc.id
                    && self.path == doc.path
                    && self.version == doc.edit_version
                    && (self.scope != Scope::Selection || doc.edit_version != u64::MAX)
                    && self.source == doc.text
                    && doc.jump_to.is_none()
                    && if self.scope == Scope::Selection {
                        stored_selection(ctx, doc.id)
                            .is_some_and(|cursor| same_selection(self.selection, cursor))
                    } else {
                        same_selection(self.selection, selection(ctx, doc.id))
                    }
            })
    }
}

impl Replace {
    /// Classify the original native batch before any shortcut or field can
    /// consume input. Retain bounded ownership metadata, never raw event text.
    fn capture_selection_input(
        &mut self,
        ctx: &egui::Context,
        raw: &egui::RawInput,
        owner: Option<Arc<InputOwner>>,
    ) {
        let mut events = raw.events.iter().filter(|event| {
            !matches!(
                event,
                egui::Event::PointerMoved(_)
                    | egui::Event::MouseMoved(_)
                    | egui::Event::PointerGone
                    // IME lifecycle notifications accompany native field-focus
                    // changes without carrying composition. Keep every Preedit
                    // and Commit event (including empty text) competing: egui
                    // can modify a TextEdit selection for those events.
                    | egui::Event::Ime(egui::ImeEvent::Enabled | egui::ImeEvent::Disabled)
            )
        });
        let first = events.next();
        let second = events.next();
        let third = events.next();
        let mut pointer = if raw.focused && third.is_none() {
            match (first, second) {
                (
                    Some(egui::Event::PointerButton {
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers,
                        ..
                    }),
                    None,
                ) if modifiers.is_none() => Some(if *pressed {
                    PointerInput::Press
                } else {
                    PointerInput::Release
                }),
                (
                    Some(egui::Event::PointerButton {
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: down,
                        ..
                    }),
                    Some(egui::Event::PointerButton {
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: up,
                        ..
                    }),
                ) if down.is_none() && up.is_none() => Some(PointerInput::Click),
                _ => None,
            }
        } else {
            None
        };
        if ctx.input(|input| {
            !input.keys_down.is_empty()
                || [
                    egui::PointerButton::Secondary,
                    egui::PointerButton::Middle,
                    egui::PointerButton::Extra1,
                    egui::PointerButton::Extra2,
                ]
                .into_iter()
                .any(|button| input.pointer.button_down(button))
                || (matches!(pointer, Some(PointerInput::Press | PointerInput::Click))
                    && input.pointer.primary_down())
        }) {
            pointer = None;
        }
        let mut keyboard = false;
        if let Some(egui::Event::Key {
            key: key @ (egui::Key::Enter | egui::Key::Space),
            pressed: true,
            repeat: false,
            modifiers,
            ..
        }) = first
        {
            let mut release = false;
            let mut space = false;
            keyboard = raw.focused
                && modifiers.is_none()
                && !ctx.input(|input| input.pointer.any_down() || !input.keys_down.is_empty())
                && [second, third]
                    .into_iter()
                    .flatten()
                    .all(|event| match event {
                        egui::Event::Key {
                            key: up,
                            pressed: false,
                            repeat: false,
                            modifiers,
                            ..
                        } if up == key && modifiers.is_none() && !release => {
                            release = true;
                            true
                        }
                        egui::Event::Text(text)
                            if *key == egui::Key::Space && text == " " && !space =>
                        {
                            space = true;
                            true
                        }
                        _ => false,
                    })
                && events.next().is_none();
        }
        // A split click carries only its clean, actually owned press while the
        // same pointer gesture and draft owner remain live. Competing input,
        // another press, or an ended gesture destroys the ticket.
        if !raw.focused
            || matches!(pointer, Some(PointerInput::Press | PointerInput::Click))
            || (pointer.is_none() && first.is_some())
            || !ctx.input(|input| input.pointer.any_down())
        {
            self.selection_press = None;
        }
        self.selection_input = Some(SelectionInput {
            pass: ctx.cumulative_pass_nr(),
            focus: ctx.memory(|memory| memory.focused()),
            pointer,
            keyboard,
            owner,
        });
    }

    fn selection_activation(
        &mut self,
        ctx: &egui::Context,
        response: &egui::Response,
    ) -> Option<Activation> {
        let input = self.selection_input.as_ref()?;
        let owner = input.owner.as_ref()?;
        if input.pass != input_pass(ctx) {
            return None;
        }
        if input.pointer == Some(PointerInput::Press) && response.is_pointer_button_down_on() {
            self.selection_press = Some((response.id, owner.clone()));
        }
        let pointer = response.clicked_by(egui::PointerButton::Primary)
            && (input.pointer == Some(PointerInput::Click)
                || (input.pointer == Some(PointerInput::Release)
                    && self
                        .selection_press
                        .as_ref()
                        .is_some_and(|(id, pressed_owner)| {
                            *id == response.id && pressed_owner == owner
                        })));
        let keyboard = input.keyboard
            && input.focus == Some(response.id)
            && response.clicked()
            && !ctx.input(|input| {
                input.raw.events.iter().any(|event| {
                    matches!(
                        event,
                        egui::Event::Key {
                            pressed: true,
                            repeat: true,
                            ..
                        }
                    )
                })
            });
        (pointer || keyboard).then(|| Activation {
            pass: input.pass,
            owner: owner.clone(),
        })
    }

    fn refuse_selection_input(&mut self) {
        self.clear();
        self.error = Some(
            "Selection action canceled. Use Preview selection or Apply without other input".into(),
        );
    }

    pub fn invalidate(&mut self) {
        if self.preview.take().is_some() {
            self.error =
                Some("Preview expired. Preview again after changing text or selection".into());
        }
        self.action = None;
        self.selection_press = None;
    }

    fn clear(&mut self) {
        self.preview = None;
        self.action = None;
        self.error = None;
        self.selection_press = None;
    }
}

impl CedarApp {
    pub(super) fn capture_replace_input(&mut self, ctx: &egui::Context, raw: &egui::RawInput) {
        // Capture full data only for a possible native button activation; idle,
        // typing and pointer-motion frames never clone the active draft.
        let buttons = [
            self.replace.selection_button,
            self.replace
                .preview
                .as_ref()
                .filter(|preview| preview.scope == Scope::Selection)
                .and(self.replace.apply_button),
        ];
        let focus = ctx.memory(|memory| memory.focused());
        let candidate = self.find_open
            && raw.events.iter().any(|event| match event {
                egui::Event::PointerButton {
                    button: egui::PointerButton::Primary,
                    pos,
                    pressed,
                    ..
                } => {
                    (!*pressed && self.replace.selection_press.is_some())
                        || buttons.into_iter().flatten().any(|id| {
                            ctx.read_response(id).is_some_and(|response| {
                                response.enabled()
                                    && response
                                        .interact_rect
                                        .expand(ctx.style().interaction.interact_radius)
                                        .contains(*pos)
                            })
                        })
                }
                egui::Event::Key {
                    key: egui::Key::Enter | egui::Key::Space,
                    pressed: true,
                    ..
                } => focus.is_some() && buttons.contains(&focus),
                _ => false,
            });
        let owner = candidate.then(|| InputOwner::capture(self, ctx)).flatten();
        if self
            .replace
            .selection_press
            .as_ref()
            .is_some_and(|(_, owner)| !owner.current(self, ctx))
        {
            self.replace.selection_press = None;
        }
        self.replace.capture_selection_input(ctx, raw, owner);
    }

    fn replace_selection_activation(
        &mut self,
        ctx: &egui::Context,
        response: &egui::Response,
    ) -> Option<Activation> {
        // Replies/navigation can run after the hook but before this widget.
        let owner = self.replace.selection_input.as_ref()?.owner.as_ref()?;
        if !owner.current(self, ctx) {
            self.replace.selection_press = None;
            return None;
        }
        self.replace.selection_activation(ctx, response)
    }

    pub(super) fn close_find(&mut self, ctx: &egui::Context) {
        self.file_find.cancel_loop();
        self.find_open = false;
        self.find_focus = false;
        self.replace.clear();
        // Closing Find and pasting/typing in one input batch must not deliver
        // the remaining field input to the newly focused source editor.
        discard_keyboard(ctx);
        self.navigation.restore_focus = true;
    }

    pub(super) fn replace_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("REPLACE").small().color(MUTED));
            let changed = ui
                .add(
                    egui::TextEdit::singleline(&mut self.replace.replacement)
                        .id(egui::Id::new(REPLACE_INPUT))
                        .char_limit(MAX_FILE_BYTES)
                        .hint_text("Literal text · empty deletes")
                        .desired_width(230.0),
                )
                .changed();
            if changed {
                self.replace.invalidate();
            }
            if ui.small_button("Preview one").clicked() {
                self.request_replace_preview(ui.ctx(), Scope::One, None);
            }
            if ui.small_button("Preview all").clicked() {
                self.request_replace_preview(ui.ctx(), Scope::All, None);
            }
            let response = ui.small_button("Preview selection");
            self.replace.selection_button = Some(response.id);
            #[cfg(test)]
            crate::workspace_access_tests::record(ui, "replace_preview_selection", &response);
            let activation = self.replace_selection_activation(ui.ctx(), &response);
            if response.clicked() {
                if let Some(activation) = activation {
                    self.request_replace_preview(ui.ctx(), Scope::Selection, Some(activation));
                } else {
                    self.replace.refuse_selection_input();
                }
            }
        });
        if let Some(error) = &self.replace.error {
            ui.label(RichText::new(error).small().color(RED));
        }
        let mut apply = false;
        let mut activation = None;
        let mut apply_response = None;
        let mut selection_scope = false;
        let mut cancel = false;
        if let Some(preview) = &self.replace.preview {
            selection_scope = preview.scope == Scope::Selection;
            ui.group(|ui| {
                ui.label(RichText::new(format!("{} · {} replacement(s)",
                    match preview.scope {
                        Scope::One => "Replace one",
                        Scope::All => "Replace all",
                        Scope::Selection => "Replace selection",
                    },
                    preview.plan.ranges.len())).strong());
                ui.label(RichText::new("Active draft only · one Undo · Save separately").small().color(MUTED));
                // Keep the explicit actions reachable at the minimum window
                // size, even when escaped controls expand an excerpt heavily.
                egui::ScrollArea::vertical().id_salt("literal_replace_preview")
                    .max_height(120.0).drag_to_scroll(false).auto_shrink([false, true]).show(ui, |ui| {
                ui.label(&preview.path);
                if preview.scope == Scope::Selection {
                    let [start, end] = preview.selection.sorted();
                    let (start_line, start_column) = model::cursor_location(&preview.source, start.index);
                    let (end_line, end_column) = model::cursor_location(&preview.source, end.index);
                    ui.label(format!("Selected range: line {start_line}, column {start_column} to line {end_line}, column {end_column} (end exclusive)"));
                }
                ui.label(format!("Before: {}", excerpt(&preview.query)));
                ui.label(format!("After: {}", if preview.replacement.is_empty() {
                    "(empty)".into()
                } else { excerpt(&preview.replacement) }));
                for range in preview.plan.ranges.iter().take(PREVIEW_ROWS) {
                    let (line, column) = model::cursor_location(&preview.source, range.start);
                    let context: String = preview.source.chars()
                        .skip(range.start.saturating_sub(20)).take(PREVIEW_CHARS).collect();
                    ui.label(RichText::new(format!("Line {line}, column {column}: {}", excerpt(&context)))
                        .monospace().small());
                }
                ui.label(RichText::new(format!("Showing {} of {} locations; text excerpts are limited to {PREVIEW_CHARS} characters. Apply uses the full replacement.",
                    preview.plan.ranges.len().min(PREVIEW_ROWS), preview.plan.ranges.len())).small().color(MUTED));
                if preview.source == preview.plan.text {
                    ui.label("No text changes; Apply keeps the existing Undo history.");
                }
                });
                ui.horizontal(|ui| {
                    let response = ui.button(RichText::new("Apply").color(GREEN));
                    #[cfg(test)]
                    crate::workspace_access_tests::record(ui, "replace_apply", &response);
                    let cancel_response = ui.button("Cancel");
                    apply = response.clicked();
                    apply_response = Some(response);
                    cancel = cancel_response.clicked();
                });
            });
        }
        if let Some(response) = apply_response {
            self.replace.apply_button = Some(response.id);
            activation = self.replace_selection_activation(ui.ctx(), &response);
        }
        if cancel {
            self.replace.clear();
        } else if apply {
            if selection_scope && activation.is_none() {
                self.replace.refuse_selection_input();
            } else {
                self.replace.action = Some(Action::Apply(activation));
            }
        }
    }

    fn request_replace_preview(
        &mut self,
        ctx: &egui::Context,
        scope: Scope,
        activation: Option<Activation>,
    ) {
        self.replace.clear();
        let Some(doc) = self.active() else {
            return;
        };
        if doc.jump_to.is_some() {
            return;
        }
        if scope == Scope::Selection && doc.edit_version == u64::MAX {
            self.replace.error =
                Some("Document version is exhausted; selection was not replaced".into());
            return;
        }
        let cursor = if scope == Scope::Selection {
            let Some(cursor) = stored_selection(ctx, doc.id) else {
                self.replace.error =
                    Some("Select nonempty text in the active file before Preview selection".into());
                return;
            };
            cursor
        } else {
            selection(ctx, doc.id)
        };
        match plan(
            &doc.text,
            &self.find_query,
            &self.replace.replacement,
            cursor,
            scope,
        ) {
            Ok(plan) => {
                self.replace.action = Some(Action::Preview(
                    Box::new(Preview {
                        document: doc.id,
                        path: doc.path.clone(),
                        generation: self.generation,
                        workspace: self.recovery_workspace(),
                        workspace_key: self.workspace_key.clone(),
                        root: self.root.clone(),
                        connection: self.state,
                        navigation: self.navigation_epoch,
                        version: doc.edit_version,
                        source: doc.text.clone(),
                        selection: cursor,
                        query: self.find_query.clone(),
                        replacement: self.replace.replacement.clone(),
                        scope,
                        plan,
                    }),
                    activation,
                ))
            }
            Err(error) => self.replace.error = Some(error),
        }
    }

    /// Run after this frame's editor, navigation and other draft transactions.
    /// A queued Apply can never overwrite input processed later in the frame.
    pub(super) fn finish_replace_frame(&mut self, ctx: &egui::Context) {
        let action = self.replace.action.take();
        if self
            .replace
            .preview
            .as_ref()
            .is_some_and(|preview| !preview.current(self, ctx))
        {
            self.replace.invalidate();
            ctx.request_repaint();
        }
        if self.navigation.blocks_editor() || self.foreign_modal_owns_input(ctx) || !self.find_open
        {
            self.replace.invalidate();
            return;
        }
        match action {
            Some(Action::Preview(preview, activation)) => {
                if preview.scope == Scope::Selection
                    && !activation.is_some_and(|activation| activation.current(self, ctx))
                {
                    self.replace.refuse_selection_input();
                    return;
                }
                if preview.current(self, ctx) {
                    self.replace.preview = Some(*preview);
                    ctx.request_repaint();
                } else {
                    self.replace.error = Some(
                        "Preview expired. Preview again after changing text or selection".into(),
                    );
                }
            }
            Some(Action::Apply(activation)) => {
                let Some(preview) = self.replace.preview.take() else {
                    return;
                };
                ctx.request_repaint();
                // Keep the check adjacent to commit, even if callers change the
                // invalidation/UI order in a future version.
                if !preview.current(self, ctx)
                    || (preview.scope == Scope::Selection
                        && !activation.is_some_and(|activation| activation.current(self, ctx)))
                {
                    return;
                }
                if preview.source == preview.plan.text {
                    self.notice = "Replace made no text changes".into();
                    return;
                }
                let doc = self
                    .documents
                    .iter_mut()
                    .find(|doc| doc.id == preview.document)
                    .unwrap();
                doc.jump_to = None;
                if preview.scope == Scope::Selection {
                    // Match Enter's established transaction pattern. Refresh
                    // the before-selection (including both affinities) without
                    // introducing a same-text Undo checkpoint.
                    editor_state::move_selection(ctx, doc, preview.selection);
                }
                editor_state::commit(ctx, doc, preview.plan.text, preview.plan.cursor_chars);
                self.file_find.cancel_loop();
                self.notice = format!(
                    "Replaced {} match(es) in draft · Save separately",
                    preview.plan.ranges.len()
                );
            }
            None => {}
        }
    }
}

#[cfg(test)]
mod tests;
