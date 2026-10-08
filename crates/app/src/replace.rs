//! Literal, active-buffer replacement. A preview never authorizes a later draft.
use crate::{editor_state, model, CedarApp, GREEN, MUTED, RED};
use cedar_protocol::MAX_FILE_BYTES;
use eframe::egui::{self, RichText};
use std::ops::Range;

pub(super) const FIND_INPUT: &str = "literal_find_query";
const REPLACE_INPUT: &str = "literal_replace_text";
const PREVIEW_ROWS: usize = 3;
const PREVIEW_CHARS: usize = 120;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scope {
    One,
    All,
}

enum Action {
    Preview(Box<Preview>),
    Apply,
}

#[derive(Default)]
pub(super) struct Replace {
    pub replacement: String,
    preview: Option<Preview>,
    error: Option<String>,
    action: Option<Action>,
}

struct Preview {
    document: u64,
    path: String,
    generation: u64,
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

fn selection(ctx: &egui::Context, document: u64) -> egui::text::CCursorRange {
    egui::TextEdit::load_state(ctx, egui::Id::new(("editor", document)))
        .and_then(|state| state.cursor.char_range())
        .unwrap_or_else(|| egui::text::CCursorRange::one(egui::text::CCursor::new(0)))
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
    // find_ranges deliberately caps its list. Never use that capped list to
    // silently authorize a partial Replace all (or misidentify a later match).
    if source
        .match_indices(query)
        .nth(model::MAX_FIND_MATCHES)
        .is_some()
    {
        return Err("Replace exceeds 10,000 matches. Narrow the Find text".into());
    }
    let mut ranges = model::find_ranges(source, query);
    if ranges.is_empty() {
        return Err("No matching text in the active file".into());
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
    for (index, (start, _)) in source.match_indices(query).enumerate() {
        if chosen.is_some_and(|chosen| chosen != index) {
            continue;
        }
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
            && self.navigation == app.navigation_epoch
            && self.query == app.find_query
            && self.replacement == app.replace.replacement
            && app.active().is_some_and(|doc| {
                self.document == doc.id
                    && self.path == doc.path
                    && self.version == doc.edit_version
                    && self.source == doc.text
                    && doc.jump_to.is_none()
                    && self.selection == selection(ctx, doc.id)
            })
    }
}

impl Replace {
    pub fn invalidate(&mut self) {
        if self.preview.take().is_some() {
            self.error =
                Some("Preview expired. Preview again after changing text or selection".into());
        }
        self.action = None;
    }

    fn clear(&mut self) {
        self.preview = None;
        self.action = None;
        self.error = None;
    }
}

impl CedarApp {
    pub(super) fn close_find(&mut self, ctx: &egui::Context) {
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
                self.request_replace_preview(ui.ctx(), Scope::One);
            }
            if ui.small_button("Preview all").clicked() {
                self.request_replace_preview(ui.ctx(), Scope::All);
            }
        });
        if let Some(error) = &self.replace.error {
            ui.label(RichText::new(error).small().color(RED));
        }
        let mut apply = false;
        let mut cancel = false;
        if let Some(preview) = &self.replace.preview {
            ui.group(|ui| {
                ui.label(RichText::new(format!("{} · {} replacement(s)",
                    match preview.scope { Scope::One => "Replace one", Scope::All => "Replace all" },
                    preview.plan.ranges.len())).strong());
                ui.label(RichText::new("Active draft only · one Undo · Save separately").small().color(MUTED));
                // Keep the explicit actions reachable at the minimum window
                // size, even when escaped controls expand an excerpt heavily.
                egui::ScrollArea::vertical().id_salt("literal_replace_preview")
                    .max_height(120.0).drag_to_scroll(false).auto_shrink([false, true]).show(ui, |ui| {
                ui.label(&preview.path);
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
                    let apply_response = ui.button(RichText::new("Apply").color(GREEN));
                    let cancel_response = ui.button("Cancel");
                    apply = apply_response.clicked();
                    cancel = cancel_response.clicked();
                });
            });
        }
        if cancel {
            self.replace.clear();
        } else if apply {
            self.replace.action = Some(Action::Apply);
        }
    }

    fn request_replace_preview(&mut self, ctx: &egui::Context, scope: Scope) {
        self.replace.clear();
        let Some(doc) = self.active() else {
            return;
        };
        if doc.jump_to.is_some() {
            return;
        }
        let cursor = selection(ctx, doc.id);
        match plan(
            &doc.text,
            &self.find_query,
            &self.replace.replacement,
            cursor,
            scope,
        ) {
            Ok(plan) => {
                self.replace.action = Some(Action::Preview(Box::new(Preview {
                    document: doc.id,
                    path: doc.path.clone(),
                    generation: self.generation,
                    navigation: self.navigation_epoch,
                    version: doc.edit_version,
                    source: doc.text.clone(),
                    selection: cursor,
                    query: self.find_query.clone(),
                    replacement: self.replace.replacement.clone(),
                    scope,
                    plan,
                })))
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
            Some(Action::Preview(preview)) => {
                if preview.current(self, ctx) {
                    self.replace.preview = Some(*preview);
                    ctx.request_repaint();
                } else {
                    self.replace.error = Some(
                        "Preview expired. Preview again after changing text or selection".into(),
                    );
                }
            }
            Some(Action::Apply) => {
                let Some(preview) = self.replace.preview.take() else {
                    return;
                };
                ctx.request_repaint();
                // Keep the check adjacent to commit, even if callers change the
                // invalidation/UI order in a future version.
                if !preview.current(self, ctx) {
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
                editor_state::commit(ctx, doc, preview.plan.text, preview.plan.cursor_chars);
                self.find_index = None;
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
