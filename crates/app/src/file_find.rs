//! Bounded literal navigation in the active draft. No source text or backend work.
use crate::{browse, location_history, model, replace, CedarApp, ConnectionState, WorkspaceKey};
use eframe::egui::{self, text::CCursorRange};
use std::{collections::HashSet, ops::Range};

const SEED_BYTES: usize = 4096;

#[derive(Clone, PartialEq, Eq)]
struct Identity {
    generation: u64,
    document: u64,
    version: u64,
    workspace: Option<WorkspaceKey>,
    root: String,
    path: String,
    connection: ConnectionState,
}
struct Snapshot {
    identity: Identity,
    query: String,
    hits: Vec<Range<usize>>,
}
struct Journey {
    owner: Owner,
}
#[derive(Clone)]
struct Owner {
    identity: Identity,
    revision: u64,
    navigation: u64,
    selection: Option<CCursorRange>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Direction {
    Next,
    Previous,
}
#[derive(Clone, Copy)]
enum Command {
    Open,
    Step(Direction),
}
#[derive(Clone)]
struct Button {
    direction: Direction,
    id: egui::Id,
    rect: egui::Rect,
}
#[derive(Default)]
pub(super) struct State {
    snapshot: Option<Snapshot>,
    revision: u64,
    exhausted: bool,
    journey: Option<Journey>,
    pub message: Option<&'static str>,
    held: HashSet<egui::Key>,
    claimed: HashSet<egui::Key>,
    composing: bool,
    consumed_keys: HashSet<egui::Key>,
    consumed_space: bool,
    key: Option<(Command, Owner, Option<egui::Id>)>,
    buttons: Vec<Button>,
    press: Option<(Button, Owner)>,
    click: Option<(Button, Owner)>,
}
impl State {
    pub(super) fn cancel_loop(&mut self) {
        self.journey = None;
        self.press = None;
        self.click = None;
    }
}
fn selection(ctx: &egui::Context, document: u64) -> Option<CCursorRange> {
    egui::TextEdit::load_state(ctx, egui::Id::new(("editor", document)))?
        .cursor
        .char_range()
}
fn same_selection(a: Option<CCursorRange>, b: Option<CCursorRange>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => location_history::same_selection(a, b),
        (None, None) => true,
        _ => false,
    }
}
fn exact_hit(hits: &[Range<usize>], cursor: CCursorRange) -> Option<usize> {
    let [start, end] = cursor.sorted();
    hits.iter()
        .position(|hit| hit.start == start.index && hit.end == end.index)
}
fn target(hits: &[Range<usize>], cursor: CCursorRange, direction: Direction) -> Option<usize> {
    if hits.is_empty() {
        return None;
    }
    if let Some(current) = exact_hit(hits, cursor) {
        return Some(match direction {
            Direction::Next => (current + 1) % hits.len(),
            Direction::Previous => (current + hits.len() - 1) % hits.len(),
        });
    }
    let [start, end] = cursor.sorted();
    Some(match direction {
        Direction::Next => hits
            .iter()
            .position(|hit| hit.start >= end.index)
            .unwrap_or(0),
        Direction::Previous => hits
            .iter()
            .rposition(|hit| hit.end <= start.index)
            .unwrap_or(hits.len() - 1),
    })
}
fn shortcut(key: egui::Key, modifiers: egui::Modifiers) -> Option<Command> {
    if key == egui::Key::F
        && !(modifiers.ctrl && modifiers.mac_cmd)
        && modifiers.matches_exact(egui::Modifiers::COMMAND)
    {
        Some(Command::Open)
    } else if key == egui::Key::F3 && (modifiers.is_none() || modifiers == egui::Modifiers::SHIFT) {
        Some(Command::Step(if modifiers.shift {
            Direction::Previous
        } else {
            Direction::Next
        }))
    } else {
        None
    }
}
impl CedarApp {
    fn find_identity(&self) -> Option<Identity> {
        let doc = self.active()?;
        Some(Identity {
            generation: self.generation,
            document: doc.id,
            version: doc.edit_version,
            workspace: self.workspace_key.clone(),
            root: self.root.clone(),
            path: doc.path.clone(),
            connection: self.state,
        })
    }
    fn sync_file_find(&mut self, ctx: &egui::Context) {
        let identity = self.find_identity();
        let cacheable = self
            .active()
            .is_some_and(|doc| doc.edit_version != u64::MAX)
            && !self.file_find.exhausted;
        let current = self.file_find.snapshot.as_ref().is_some_and(|snapshot| {
            Some(&snapshot.identity) == identity.as_ref() && snapshot.query == self.find_query
        });
        if !current || !cacheable {
            self.file_find.cancel_loop();
            self.file_find.snapshot = None;
            if cacheable {
                if let Some(revision) = self.file_find.revision.checked_add(1) {
                    self.file_find.revision = revision;
                    let doc = self.active().expect("cacheable active document");
                    self.file_find.snapshot = Some(Snapshot {
                        identity: identity.expect("active identity"),
                        query: self.find_query.clone(),
                        hits: model::find_ranges(&doc.text, &self.find_query),
                    });
                } else {
                    self.file_find.exhausted = true;
                }
            }
        }
        if self
            .file_find
            .journey
            .as_ref()
            .is_some_and(|journey| !self.find_owner_current(&journey.owner, ctx))
        {
            self.file_find.cancel_loop();
        }
    }
    fn find_owner(&self, ctx: &egui::Context) -> Option<Owner> {
        let identity = self.find_identity()?;
        Some(Owner {
            selection: selection(ctx, identity.document),
            identity,
            revision: self.file_find.revision,
            navigation: self.navigation_epoch,
        })
    }
    fn find_owner_current(&self, owner: &Owner, ctx: &egui::Context) -> bool {
        self.find_identity().as_ref() == Some(&owner.identity)
            && owner.revision == self.file_find.revision
            && owner.navigation == self.navigation_epoch
            && same_selection(owner.selection, selection(ctx, owner.identity.document))
            && self.active().is_some_and(|doc| doc.jump_to.is_none())
    }
    fn file_find_allowed(&self, ctx: &egui::Context) -> bool {
        ctx == &self.editor_ctx
            && self.editor_indent_eligible(ctx)
            && !self.navigation.coordinate_jump_pending()
            && !ctx.memory(|memory| memory.any_popup_open())
            && !self.completion_popup_open()
            && self.active().is_some_and(|doc| doc.jump_to.is_none())
    }
    /// Classify all original edges before any other surface filters them. Only
    /// fixed ownership metadata survives; held and IME lifetimes span frames.
    pub(super) fn capture_file_find_input(&mut self, ctx: &egui::Context, raw: &egui::RawInput) {
        self.sync_file_find(ctx);
        let allowed = self.file_find_allowed(ctx);
        let focused = ctx.memory(|memory| memory.focused());
        let owns_field = self
            .active_document
            .is_some_and(|id| focused == Some(egui::Id::new(("editor", id))))
            || (self.find_open && focused == Some(egui::Id::new(replace::FIND_INPUT)));
        let owner = self.find_owner(ctx);
        let input = &mut self.file_find;
        input.key = None;
        input.click = None;
        input.consumed_keys.clear();
        input.consumed_space = false;
        if !raw.focused
            || raw
                .events
                .iter()
                .any(|event| matches!(event, egui::Event::WindowFocused(false)))
        {
            input.held.clear();
            input.claimed.clear();
            input.composing = false;
            input.cancel_loop();
            return;
        }
        let was_composing = input.composing;
        for event in &raw.events {
            match event {
                egui::Event::Ime(egui::ImeEvent::Preedit(_)) => input.composing = true,
                egui::Event::Ime(egui::ImeEvent::Commit(_) | egui::ImeEvent::Disabled) => {
                    input.composing = false
                }
                _ => {}
            }
        }
        let clean = browse::clean_press(&raw.events).filter(|_| {
            !raw.events
                .iter()
                .any(|event| matches!(event, egui::Event::Key { repeat: true, .. }))
        });
        let fresh_batch = !was_composing
            && !input.composing
            && input.held.is_empty()
            && !ctx.input(|state| state.pointer.any_down() || !state.keys_down.is_empty());
        let pointer_clean = allowed
            && self.find_open
            && !was_composing
            && !input.composing
            && input.held.is_empty()
            && raw.modifiers.is_none()
            && !ctx.input(|state| {
                !state.keys_down.is_empty()
                    || [
                        egui::PointerButton::Secondary,
                        egui::PointerButton::Middle,
                        egui::PointerButton::Extra1,
                        egui::PointerButton::Extra2,
                    ]
                    .iter()
                    .any(|button| state.pointer.button_down(*button))
            })
            && browse::pointer_only(&raw.events);
        let edges: Vec<_> = raw
            .events
            .iter()
            .filter_map(|event| {
                if let egui::Event::PointerButton { pressed, .. } = event {
                    Some(*pressed)
                } else {
                    None
                }
            })
            .take(3)
            .collect();
        if !pointer_clean
            || edges.len() > 2
            || (edges.len() == 2 && edges != [true, false])
            || (edges.first() == Some(&true)
                && (input.press.is_some() || ctx.input(|state| state.pointer.any_down())))
        {
            input.press = None;
        } else {
            for event in &raw.events {
                if let egui::Event::PointerButton {
                    pos,
                    pressed,
                    modifiers,
                    ..
                } = event
                {
                    if !modifiers.is_none() {
                        input.press = None;
                        continue;
                    }
                    if *pressed {
                        input.press = input
                            .buttons
                            .iter()
                            .find(|button| button.rect.contains(*pos))
                            .cloned()
                            .zip(owner.clone());
                    } else if let Some((button, captured)) = input.press.take() {
                        if button.rect.contains(*pos)
                            && input.buttons.iter().any(|live| {
                                live.id == button.id
                                    && live.direction == button.direction
                                    && live.rect.contains(*pos)
                            })
                        {
                            input.click = Some((button, captured));
                        }
                    }
                }
            }
        }
        for event in &raw.events {
            let egui::Event::Key {
                key,
                modifiers,
                pressed,
                repeat,
                ..
            } = event
            else {
                continue;
            };
            let claimed = input.claimed.contains(key);
            if !pressed {
                input.held.remove(key);
                input.claimed.remove(key);
                if claimed {
                    input.consumed_keys.insert(*key);
                }
                continue;
            }
            let fresh = input.held.insert(*key);
            let command = shortcut(*key, *modifiers)
                .filter(|command| allowed && (matches!(command, Command::Open) || owns_field))
                .or_else(|| {
                    (allowed
                        && self.find_open
                        && focused == Some(egui::Id::new(replace::FIND_INPUT))
                        && *key == egui::Key::Enter
                        && modifiers.is_none())
                    .then_some(Command::Step(Direction::Next))
                })
                .or_else(|| {
                    input
                        .buttons
                        .iter()
                        .find(|button| {
                            Some(button.id) == focused
                                && matches!(key, egui::Key::Enter | egui::Key::Space)
                                && modifiers.is_none()
                        })
                        .filter(|_| allowed && self.find_open)
                        .map(|button| Command::Step(button.direction))
                });
            // A rejected mixed Enter stays in its original field; no input is
            // redirected into the editor by a speculative focus transfer.
            if claimed
                || (command.is_some()
                    && fresh
                    && !repeat
                    && fresh_batch
                    && clean == Some((*key, *modifiers)))
            {
                input.consumed_keys.insert(*key);
                if fresh || claimed {
                    input.claimed.insert(*key);
                }
                if fresh && !repeat && fresh_batch && clean == Some((*key, *modifiers)) {
                    input.key = command
                        .zip(owner.clone())
                        .map(|(command, owner)| (command, owner, focused));
                }
            }
        }
        if input.key.is_some() && clean.is_some_and(|(key, _)| key == egui::Key::Space) {
            input.consumed_space = true;
        }
    }
    /// Keep original native edges visible through ready-navigation polling,
    /// then consume only this surface's captured key lifetimes even if a modal
    /// or an early-returning shortcut has acquired focus in the meantime.
    pub(super) fn consume_file_find_keys(&self, ctx: &egui::Context) {
        if !self.navigation.input_frame_current(ctx) {
            return;
        }
        ctx.input_mut(|input| {
            input.events.retain(|event| match event {
                egui::Event::Key { key, .. } => !self.file_find.consumed_keys.contains(key),
                egui::Event::Text(text) => !(self.file_find.consumed_space && text == " "),
                _ => true,
            })
        });
    }
    pub(super) fn file_find_shortcuts(&mut self, ctx: &egui::Context) -> bool {
        if !self.navigation.input_frame_current(ctx) {
            return false;
        }
        self.consume_file_find_keys(ctx);
        self.sync_file_find(ctx);
        let Some((command, owner, focus)) = self.file_find.key.take() else {
            return false;
        };
        if !self.file_find_allowed(ctx)
            || ctx.memory(|memory| memory.focused()) != focus
            || !self.find_owner_current(&owner, ctx)
        {
            return false;
        }
        match command {
            Command::Open => self.open_file_find(
                ctx,
                focus == Some(egui::Id::new(("editor", owner.identity.document))),
            ),
            Command::Step(direction) => self.step_file_find(ctx, direction),
        }
        true
    }
    /// At the existing ready-reply retirement gate, current Find input wins
    /// once under its original owner. No ticket is rebound to a newer epoch.
    pub(super) fn file_find_supersede_ready_navigation(&mut self, ctx: &egui::Context) -> bool {
        let navigation = self.navigation_epoch;
        if !self.file_find_shortcuts(ctx) {
            return false;
        }
        if self.navigation_epoch == navigation {
            self.navigation_changed();
        }
        true
    }
    fn open_file_find(&mut self, ctx: &egui::Context, editor_owned: bool) {
        self.file_find.message = None;
        if editor_owned {
            if let Some(doc) = self.active() {
                if let Some(cursor) = selection(ctx, doc.id) {
                    let [start, end] = cursor.sorted();
                    let chars = doc.text.chars().count();
                    if end.index > chars {
                        self.file_find.message =
                            Some("Selection is outside this file; kept the Find text.");
                    } else if start.index != end.index {
                        let byte_at = |index| {
                            doc.text
                                .char_indices()
                                .map(|(byte, _)| byte)
                                .chain(std::iter::once(doc.text.len()))
                                .nth(index)
                                .expect("validated scalar selection")
                        };
                        let seed = &doc.text[byte_at(start.index)..byte_at(end.index)];
                        if seed.len() > SEED_BYTES {
                            self.file_find.message =
                                Some("Selection exceeds 4,096 UTF-8 bytes; kept the Find text.");
                        } else if seed.contains(['\r', '\n', '\0']) {
                            self.file_find.message =
                                Some("Selection contains a newline or NUL; kept the Find text.");
                        } else {
                            self.find_query = seed.to_owned();
                            self.replace.invalidate();
                        }
                    }
                }
            }
        }
        self.find_open = true;
        self.find_focus = true;
        self.sync_file_find(ctx);
    }
    fn step_file_find(&mut self, ctx: &egui::Context, direction: Direction) {
        self.sync_file_find(ctx);
        if self
            .active()
            .is_some_and(|doc| doc.edit_version == u64::MAX)
            || self.file_find.exhausted
        {
            self.file_find.message =
                Some("Find navigation unavailable: this buffer or query version is exhausted.");
            return;
        }
        let Some(doc) = self.active() else {
            return;
        };
        let Some(cursor) = selection(ctx, doc.id) else {
            return;
        };
        if cursor.primary.index > doc.text.chars().count()
            || cursor.secondary.index > doc.text.chars().count()
        {
            self.file_find.message = Some("Selection is outside this file; Find did not move it.");
            return;
        }
        let Some(snapshot) = &self.file_find.snapshot else {
            return;
        };
        let Some(index) = target(&snapshot.hits, cursor, direction) else {
            return;
        };
        let range = snapshot.hits[index].clone();
        if exact_hit(&snapshot.hits, cursor) == Some(index) {
            return;
        }
        let document = doc.id;
        // Clean button activation can include native IME availability changes.
        // They belonged to the old focus and must not collapse the new range.
        replace::discard_keyboard(ctx);
        let continuing = self.file_find.journey.is_some();
        let departure = self.history_departure();
        self.navigation_changed();
        if !continuing {
            self.history_begin(departure, false);
        }
        let doc = self
            .documents
            .iter_mut()
            .find(|doc| doc.id == document)
            .expect("active document");
        let mut state = crate::editor_state::load(ctx, doc);
        let selected = CCursorRange::two(
            egui::text::CCursor::new(range.start),
            egui::text::CCursor::new(range.end),
        );
        state.cursor.set_char_range(Some(selected));
        let id = egui::Id::new(("editor", document));
        state.store(ctx, id);
        doc.cursor = model::cursor_location(&doc.text, selected.primary.index);
        doc.scroll_to = Some(range.start);
        ctx.memory_mut(|memory| memory.request_focus(id));
        if !continuing {
            let ticket = self.history_take_completion();
            self.history_commit(ticket);
        }
        self.file_find.journey = self.find_owner(ctx).map(|owner| Journey { owner });
        self.file_find.message = None;
    }
    pub(super) fn file_find_button(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        direction: Direction,
    ) {
        self.file_find
            .buttons
            .retain(|button| button.direction != direction);
        if response.enabled() {
            self.file_find.buttons.push(Button {
                direction,
                id: response.id,
                rect: response.interact_rect.intersect(ui.clip_rect()),
            });
        }
        if !response.clicked()
            || !self.navigation.input_frame_current(ui.ctx())
            || !self.file_find_allowed(ui.ctx())
        {
            return;
        }
        self.sync_file_find(ui.ctx());
        if self
            .file_find
            .click
            .as_ref()
            .is_some_and(|(button, owner)| {
                button.id == response.id
                    && button.direction == direction
                    && self.find_owner_current(owner, ui.ctx())
            })
        {
            self.file_find.click = None;
            self.step_file_find(ui.ctx(), direction);
        }
    }
    pub(super) fn file_find_count(&mut self, ctx: &egui::Context) -> String {
        self.sync_file_find(ctx);
        if self
            .active()
            .is_some_and(|doc| doc.edit_version == u64::MAX)
            || self.file_find.exhausted
        {
            return "Find navigation unavailable: version exhausted".into();
        }
        let Some(snapshot) = &self.file_find.snapshot else {
            return "0 matches".into();
        };
        let count = snapshot.hits.len();
        let current = selection(ctx, snapshot.identity.document)
            .and_then(|cursor| exact_hit(&snapshot.hits, cursor));
        if count == model::MAX_FIND_MATCHES {
            current.map_or_else(
                || format!("First {count} matches retained"),
                |index| format!("{} of first {count} matches retained", index + 1),
            )
        } else {
            current.map_or_else(
                || format!("{count} matches"),
                |index| format!("{} of {count} matches", index + 1),
            )
        }
    }
    pub(super) fn finish_file_find_frame(&mut self, ctx: &egui::Context) {
        self.sync_file_find(ctx);
        self.file_find.key = None;
        self.file_find.click = None;
        if !self.find_open {
            self.file_find.buttons.clear();
        }
    }
}

#[cfg(test)]
mod tests;
