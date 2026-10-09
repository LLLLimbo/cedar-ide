//! Bounded history of exact locations in still-open buffers. No file data or
//! remote operations belong here: an entry is only an identity and selection.
use crate::{model::Document, CedarApp, MUTED};
use eframe::egui::{
    self,
    text::{CCursor, CCursorRange},
};

const LIMIT: usize = 64;

#[derive(Clone, Copy, Debug)]
pub(super) struct Location {
    pub generation: u64,
    pub document: u64,
    pub edit_version: u64,
    pub selection: CCursorRange,
}

// egui's CCursor equality intentionally ignores affinity. History must not.
pub(super) fn same_selection(a: CCursorRange, b: CCursorRange) -> bool {
    a.primary.index == b.primary.index
        && a.primary.prefer_next_row == b.primary.prefer_next_row
        && a.secondary.index == b.secondary.index
        && a.secondary.prefer_next_row == b.secondary.prefer_next_row
}

impl Location {
    fn same(self, other: Self) -> bool {
        self.generation == other.generation
            && self.document == other.document
            && self.edit_version == other.edit_version
            && same_selection(self.selection, other.selection)
    }
    fn valid(self, generation: u64, documents: &[Document]) -> bool {
        self.generation == generation
            && self.edit_version != u64::MAX
            && documents.iter().any(|doc| {
                doc.id == self.document
                    && doc.edit_version == self.edit_version
                    && self.selection.primary.index <= doc.text.chars().count()
                    && self.selection.secondary.index <= doc.text.chars().count()
            })
    }
}

pub(super) struct Admission {
    generation: u64,
    navigation: u64,
    departure: Option<Location>,
    // Language opens transfer this ticket through URI resolution and Read,
    // admitting only the final validated selection, never the provisional tab.
    pub language: bool,
    target: Option<(u64, u64)>,
}

struct Focus {
    pass: u64,
    generation: u64,
    navigation: u64,
    document: u64,
    focused: Option<egui::Id>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Press {
    generation: u64,
    navigation: u64,
    document: Option<u64>,
    versions: u128,
    revision: u64,
    direction: Direction,
    widget: egui::Id,
}

#[derive(Default)]
pub(super) struct LocationHistory {
    pub back: Vec<Location>,
    pub forward: Vec<Location>,
    pub pending: Option<Admission>,
    pub message: Option<String>,
    focus: Option<Focus>,
    press: Option<Press>,
    revision: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Direction {
    Back,
    Forward,
}

impl LocationHistory {
    pub fn clear(&mut self) {
        *self = Self::default();
    }
    pub fn cancel_pending(&mut self) {
        self.pending = None;
        self.press = None;
    }
    fn stack_changed(&mut self) {
        self.press = None;
        self.revision = self.revision.wrapping_add(1);
    }
    pub fn close(&mut self, document: u64) {
        self.stack_changed();
        self.back.retain(|entry| entry.document != document);
        self.forward.retain(|entry| entry.document != document);
        if self.pending.as_ref().is_some_and(|ticket| {
            ticket
                .departure
                .is_some_and(|entry| entry.document == document)
                || ticket.target.is_some_and(|(id, _)| id == document)
        }) {
            self.pending = None;
        }
    }
    fn stack_mut(&mut self, direction: Direction) -> &mut Vec<Location> {
        match direction {
            Direction::Back => &mut self.back,
            Direction::Forward => &mut self.forward,
        }
    }
}

fn push(stack: &mut Vec<Location>, entry: Location) {
    if stack.last().is_some_and(|last| last.same(entry)) {
        return;
    }
    if stack.len() == LIMIT {
        stack.remove(0);
    }
    stack.push(entry);
}

fn actionable(event: &egui::Event) -> bool {
    matches!(
        event,
        egui::Event::Key { pressed: true, .. }
            | egui::Event::PointerButton { .. }
            | egui::Event::MouseWheel { .. }
            | egui::Event::Zoom(_)
            | egui::Event::Text(_)
            | egui::Event::Paste(_)
            | egui::Event::Copy
            | egui::Event::Cut
            | egui::Event::Ime(_)
            | egui::Event::Touch { .. }
            | egui::Event::WindowFocused(false)
    )
}

fn shortcut(event: &egui::Event) -> Option<Direction> {
    let egui::Event::Key { key, modifiers, .. } = event else {
        return None;
    };
    if modifiers.alt
        || modifiers.shift
        || (modifiers.ctrl && modifiers.mac_cmd)
        || !modifiers.matches_exact(egui::Modifiers::COMMAND)
    {
        return None;
    }
    match key {
        egui::Key::OpenBracket => Some(Direction::Back),
        egui::Key::CloseBracket => Some(Direction::Forward),
        _ => None,
    }
}

#[derive(Clone, Copy)]
enum ButtonInput {
    Quiet,
    Press(egui::Pos2),
    Release,
    Click(egui::Pos2),
    Keyboard,
    Mixed,
}

fn button_input(input: &egui::InputState) -> ButtonInput {
    // Read only fixed metadata from at most three actionable events. Never
    // copy text, paste, or IME bodies into the navigation UI's own storage.
    let mut events = input.raw.events.iter().filter(|event| {
        // egui-winit can emit Disabled when TextEdit loses IME eligibility,
        // even without a preceding Enabled. Focus housekeeping must not erase
        // a captured button press; actual composition remains competing input.
        match event {
            egui::Event::Ime(egui::ImeEvent::Enabled | egui::ImeEvent::Disabled) => false,
            egui::Event::Ime(egui::ImeEvent::Preedit(text) | egui::ImeEvent::Commit(text)) => {
                !text.is_empty()
            }
            _ => actionable(event),
        }
    });
    let first = events.next();
    let second = events.next();
    if events.next().is_some() {
        return ButtonInput::Mixed;
    }
    match (first, second) {
        (None, None) => ButtonInput::Quiet,
        (
            Some(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                ..
            }),
            None,
        ) => ButtonInput::Press(*pos),
        (
            Some(egui::Event::PointerButton {
                button: egui::PointerButton::Primary,
                pressed: false,
                ..
            }),
            None,
        ) => ButtonInput::Release,
        (
            Some(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                ..
            }),
            Some(egui::Event::PointerButton {
                button: egui::PointerButton::Primary,
                pressed: false,
                ..
            }),
        ) => ButtonInput::Click(*pos),
        (
            Some(egui::Event::Key {
                key: egui::Key::Enter | egui::Key::Space,
                pressed: true,
                repeat: false,
                modifiers,
                ..
            }),
            None,
        ) if modifiers.is_none()
            && !input
                .events
                .iter()
                .any(|event| matches!(event, egui::Event::Key { repeat: true, .. })) =>
        {
            ButtonInput::Keyboard
        }
        _ => ButtonInput::Mixed,
    }
}

impl CedarApp {
    fn history_location(&self, document: u64) -> Option<Location> {
        let doc = self.documents.iter().find(|doc| doc.id == document)?;
        if doc.edit_version == u64::MAX {
            return None;
        }
        // TextEdit starts a never-rendered document at its end. Once rendered,
        // the stored full range is authoritative, including reversed ranges.
        let selection =
            egui::TextEdit::load_state(&self.editor_ctx, egui::Id::new(("editor", doc.id)))
                .and_then(|state| state.cursor.char_range())
                .unwrap_or_else(|| CCursorRange::one(CCursor::new(doc.text.chars().count())));
        let location = Location {
            generation: self.generation,
            document: doc.id,
            edit_version: doc.edit_version,
            selection,
        };
        location
            .valid(self.generation, &self.documents)
            .then_some(location)
    }

    pub(super) fn history_departure(&self) -> Option<Location> {
        self.history_location(self.active_document?)
    }

    pub(super) fn history_begin(&mut self, departure: Option<Location>, language: bool) {
        self.location_history.pending = Some(Admission {
            generation: self.generation,
            navigation: self.navigation_epoch,
            departure,
            language,
            target: None,
        });
    }

    pub(super) fn history_transfer(&mut self) -> Option<Admission> {
        self.location_history.pending.take().filter(|ticket| {
            ticket.generation == self.generation && ticket.navigation == self.navigation_epoch
        })
    }

    pub(super) fn history_resume(&mut self, mut ticket: Admission) {
        ticket.navigation = self.navigation_epoch;
        self.location_history.pending = Some(ticket);
    }

    pub(super) fn history_wait_for_selection(&mut self, document: u64) {
        let version = self
            .documents
            .iter()
            .find(|doc| doc.id == document)
            .map(|doc| doc.edit_version);
        if let Some(ticket) = &mut self.location_history.pending {
            ticket.target = version.map(|version| (document, version));
        }
    }

    // The captured range is a historical location, unaffected by later caret
    // movement. Edits invalidate its exact version; never rebind it to a newer
    // version or replace its original selection.
    pub(super) fn history_take_completion(&mut self) -> Option<Admission> {
        let mut ticket = self.history_transfer()?;
        if ticket.target.is_some_and(|(id, version)| {
            self.active_document != Some(id)
                || !self
                    .documents
                    .iter()
                    .any(|doc| doc.id == id && doc.edit_version == version)
        }) {
            return None;
        }
        ticket.departure = ticket
            .departure
            .filter(|entry| entry.valid(self.generation, &self.documents));
        Some(ticket)
    }

    pub(super) fn history_take_jump_completion(&mut self) -> Option<Admission> {
        if !self
            .location_history
            .pending
            .as_ref()
            .is_some_and(|ticket| {
                !ticket.language
                    && ticket.target.is_some_and(|(id, version)| {
                        self.active_document == Some(id)
                            && self
                                .documents
                                .iter()
                                .any(|doc| doc.id == id && doc.edit_version == version)
                    })
            })
        {
            return None;
        }
        self.history_take_completion()
    }

    pub(super) fn history_cancel_language(&mut self) {
        if self
            .location_history
            .pending
            .as_ref()
            .is_some_and(|ticket| ticket.language)
        {
            self.location_history.pending = None;
        }
    }
    pub(super) fn history_cancel_ticket(&mut self, navigation: u64) {
        if self
            .location_history
            .pending
            .as_ref()
            .is_some_and(|ticket| ticket.navigation == navigation)
        {
            self.location_history.pending = None;
        }
    }

    pub(super) fn history_commit(&mut self, ticket: Option<Admission>) {
        let Some(ticket) = ticket else {
            return;
        };
        if ticket.generation != self.generation || ticket.navigation != self.navigation_epoch {
            return;
        }
        let Some(destination) = self.history_departure() else {
            if self
                .active()
                .is_some_and(|doc| doc.edit_version == u64::MAX)
            {
                self.location_history.message =
                    Some("History unavailable: this buffer's edit version is saturated".into());
            }
            return;
        };
        self.history_commit_destination(Some(ticket), destination);
    }

    pub(super) fn history_commit_destination(
        &mut self,
        ticket: Option<Admission>,
        destination: Location,
    ) {
        let Some(ticket) = ticket else {
            return;
        };
        if ticket.generation != self.generation || ticket.navigation != self.navigation_epoch {
            return;
        }
        if destination.edit_version == u64::MAX {
            self.location_history.message =
                Some("History unavailable: this buffer's edit version is saturated".into());
            return;
        }
        if !destination.valid(self.generation, &self.documents) {
            return;
        }
        if ticket
            .departure
            .is_some_and(|departure| departure.same(destination))
        {
            return;
        }
        if let Some(departure) = ticket.departure {
            push(&mut self.location_history.back, departure);
        }
        self.location_history.forward.clear();
        self.location_history.stack_changed();
        self.location_history.message = None;
    }

    pub(super) fn history_complete_open(&mut self) {
        if self
            .location_history
            .pending
            .as_ref()
            .is_some_and(|ticket| ticket.language)
        {
            return;
        }
        let ticket = self.history_take_completion();
        self.history_commit(ticket);
    }

    pub(super) fn activate_history_tab(&mut self, document: u64) {
        if !self.documents.iter().any(|doc| doc.id == document) {
            return;
        }
        if self.active_document == Some(document) {
            self.navigation_changed();
            return;
        }
        let departure = self.history_departure();
        self.navigation_changed();
        self.history_begin(departure, false);
        self.active_document = Some(document);
        self.find_index = None;
        if self.active().is_some_and(|doc| doc.jump_to.is_some()) {
            self.history_wait_for_selection(document);
        } else {
            self.history_complete_open();
        }
    }

    pub(super) fn history_go_to_line(&mut self, line: usize) {
        let departure = self.history_departure();
        self.navigation_changed();
        self.history_begin(departure, false);
        if let Some(doc) = self
            .documents
            .iter_mut()
            .find(|doc| Some(doc.id) == self.active_document)
        {
            doc.jump_to = Some(crate::model::line_start(&doc.text, line));
            let document = doc.id;
            self.history_wait_for_selection(document);
        }
    }

    fn history_blocked(&self, ctx: &egui::Context) -> bool {
        !ctx.input(|input| input.focused)
            || self.navigation.blocks_editor()
            || self.navigation.restore_focus
            || self.foreign_modal_owns_input(ctx)
            || self.open_form
            || self.new_file
            || self.find_focus
            || self.close_tab_requested.is_some()
            || self.close_after_language_stop
            || self.allow_close
            || self.active().is_some_and(|doc| doc.jump_to.is_some())
    }

    pub(super) fn history_step(&mut self, direction: Direction, ctx: &egui::Context) -> bool {
        if self.history_blocked(ctx) {
            return false;
        }
        let current = self.history_departure();
        // Even a boundary/all-stale/all-equal gesture is newer explicit
        // navigation. It cancels older asynchronous focus intent while preserving
        // the current location and opposite stack unless a restore succeeds.
        self.navigation_changed();
        let mut skipped = 0;
        let target = loop {
            let Some(entry) = self.location_history.stack_mut(direction).last().copied() else {
                break None;
            };
            let valid = entry.valid(self.generation, &self.documents);
            if valid && !current.is_some_and(|current| current.same(entry)) {
                break Some(entry);
            }
            // Ordinary caret movement can return to a recorded location. Remove
            // these redundant entries too, without counting them as stale.
            self.location_history.stack_mut(direction).pop();
            if !valid {
                skipped += 1;
            }
            self.location_history.stack_changed();
        };
        if skipped > 0 {
            self.location_history.message = Some(format!(
                "Skipped {skipped} stale editor location{}",
                if skipped == 1 { "" } else { "s" }
            ));
        }
        let Some(target) = target else {
            return false;
        };
        // Recheck before touching the active tab or editor state. Pruning alone
        // never creates an opposite entry, changes focus, or changes selection.
        if !target.valid(self.generation, &self.documents) {
            return false;
        }
        let Some(doc) = self
            .documents
            .iter_mut()
            .find(|doc| doc.id == target.document)
        else {
            return false;
        };
        let id = egui::Id::new(("editor", target.document));
        let mut state = egui::TextEdit::load_state(ctx, id).unwrap_or_default();
        state.cursor.set_char_range(Some(target.selection));
        state.store(ctx, id);
        doc.cursor = crate::model::cursor_location(&doc.text, target.selection.primary.index);
        doc.jump_to = None;
        doc.scroll_to = Some(target.selection.primary.index);
        self.active_document = Some(target.document);
        self.find_index = None;
        self.location_history.stack_mut(direction).pop();
        self.location_history.stack_changed();
        if let Some(current) = current {
            let opposite = match direction {
                Direction::Back => Direction::Forward,
                Direction::Forward => Direction::Back,
            };
            push(self.location_history.stack_mut(opposite), current);
        }
        if skipped == 0 {
            self.location_history.message = None;
        }
        self.location_history.focus = Some(Focus {
            pass: ctx.cumulative_pass_nr(),
            generation: self.generation,
            navigation: self.navigation_epoch,
            document: target.document,
            focused: ctx.memory(|memory| memory.focused()),
        });
        true
    }

    pub(super) fn history_supersede_ready_navigation(&mut self, ctx: Option<&egui::Context>) {
        // The guard is cancelling an older reply on behalf of current input.
        // Consume only this exact, already-owned history release here, after
        // preceding worker events and before this reply. Other invalidated
        // presses remain cancelled; no-ready frames use normal widget handling.
        let owned_press = ctx.and_then(|ctx| {
            let press = self.location_history.press?;
            if self.history_blocked(ctx)
                || !matches!(ctx.input(button_input), ButtonInput::Release)
                || self.history_press_stamp(press.direction, press.widget) != Some(press)
            {
                return None;
            }
            let clicked = ctx.interaction_snapshot(|snapshot| snapshot.clicked);
            if clicked != Some(press.widget) {
                return None;
            }
            ctx.read_response(press.widget)
                .filter(|response| {
                    response.enabled() && response.clicked_by(egui::PointerButton::Primary)
                })
                .map(|_| press)
        });
        if let (Some(press), Some(ctx)) = (owned_press, ctx) {
            // A boundary/all-stale click is consumed too: history_step advances
            // ownership once even when it does not restore a location.
            self.history_step(press.direction, ctx);
            return;
        }
        self.location_history.cancel_pending();
        self.navigation_epoch = self.navigation_epoch.wrapping_add(1);
        self.cancel_ready_language_navigation();
    }

    pub(super) fn history_newer_completion_input(&self, ctx: &egui::Context) -> bool {
        // egui resolves a captured click from the previous frame's widget
        // before update(). Read it outside Context locks; an orphan release or
        // disabled widget is not a new intent and must not cancel a reply.
        let primary_release = ctx.input(|input| {
            input.raw.events.iter().any(|event| {
                matches!(
                    event,
                    egui::Event::PointerButton {
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        ..
                    }
                )
            })
        });
        let clicked = ctx.interaction_snapshot(|snapshot| snapshot.clicked);
        let owned_release = primary_release
            && clicked
                .and_then(|id| ctx.read_response(id))
                .is_some_and(|response| {
                    response.enabled() && response.clicked_by(egui::PointerButton::Primary)
                });
        self.foreign_modal_owns_input(ctx)
            || self.navigation.dialog_open()
            || self.open_form
            || self.new_file
            || ctx.input(|input| {
                !input.focused
                    || input.raw.events.iter().any(|event| match event {
                        egui::Event::Key {
                            pressed: true,
                            repeat,
                            ..
                        } => !(shortcut(event).is_some() && *repeat),
                        egui::Event::PointerButton { pressed: true, .. } => true,
                        egui::Event::PointerButton {
                            button: egui::PointerButton::Primary,
                            pressed: false,
                            ..
                        } => owned_release,
                        egui::Event::Text(text) | egui::Event::Paste(text) => !text.is_empty(),
                        egui::Event::Ime(
                            egui::ImeEvent::Preedit(text) | egui::ImeEvent::Commit(text),
                        ) => !text.is_empty(),
                        egui::Event::Copy
                        | egui::Event::Cut
                        | egui::Event::WindowFocused(false) => true,
                        egui::Event::Touch {
                            phase: egui::TouchPhase::Start,
                            ..
                        } => true,
                        egui::Event::MouseWheel { delta, .. } => *delta != egui::Vec2::ZERO,
                        egui::Event::Zoom(factor) => *factor != 1.0,
                        _ => false,
                    })
            })
    }

    pub(super) fn history_shortcuts(&mut self, ctx: &egui::Context) {
        let direction = ctx.input(|input| {
            let mut events = input.raw.events.iter().filter(|event| actionable(event));
            let event = events.next()?;
            if events.next().is_some()
                || !matches!(
                    event,
                    egui::Event::Key {
                        pressed: true,
                        repeat: false,
                        ..
                    }
                )
            {
                return None;
            }
            if input.pointer.any_down()
                || input.events.iter().any(|event| {
                    shortcut(event).is_some()
                        && matches!(event, egui::Event::Key { repeat: true, .. })
                })
            {
                return None;
            }
            shortcut(event)
        });
        ctx.input_mut(|input| input.events.retain(|event| shortcut(event).is_none()));
        if let Some(direction) = direction {
            self.history_step(direction, ctx);
        }
    }

    fn history_press_stamp(&self, direction: Direction, widget: egui::Id) -> Option<Press> {
        if self
            .documents
            .iter()
            .any(|doc| doc.edit_version == u64::MAX)
        {
            return None;
        }
        Some(Press {
            generation: self.generation,
            navigation: self.navigation_epoch,
            document: self.active_document,
            // Versions only increase for an open incarnation. At most 32 u64
            // versions fit exactly in u128; any source/target edit cancels a
            // held press without retaining another document or stack snapshot.
            versions: self
                .documents
                .iter()
                .map(|doc| u128::from(doc.edit_version))
                .sum(),
            revision: self.location_history.revision,
            direction,
            widget,
        })
    }

    pub(super) fn history_controls(&mut self, ui: &mut egui::Ui) {
        let input = ui.input(button_input);
        if !matches!(
            input,
            ButtonInput::Quiet
                | ButtonInput::Press(_)
                | ButtonInput::Release
                | ButtonInput::Click(_)
        ) || self.history_blocked(ui.ctx())
        {
            self.location_history.press = None;
        }
        ui.horizontal_wrapped(|ui| {
            let enabled = !self.history_blocked(ui.ctx()) && ui.is_enabled();
            for (direction, label, available, help) in [
                (Direction::Back, "Back", !self.location_history.back.is_empty(), "Back to an unchanged open-buffer location · Ctrl/Cmd+["),
                (Direction::Forward, "Forward", !self.location_history.forward.is_empty(), "Forward to an unchanged open-buffer location · Ctrl/Cmd+]"),
            ] {
                let response = ui.push_id(("location_history", label), |ui| ui.add_enabled(enabled && available, egui::Button::new(label).small())).inner.on_hover_text(help);
                #[cfg(test)]
                crate::workspace_access_tests::record(ui, label, &response);
                let clean_press = matches!(input, ButtonInput::Press(pos) | ButtonInput::Click(pos) if response.rect.contains(pos));
                if enabled && available && clean_press {
                    self.location_history.press = self.history_press_stamp(direction, response.id);
                }
                if response.clicked() {
                    let pointer = matches!(input, ButtonInput::Release | ButtonInput::Click(_));
                    let keyboard = matches!(input, ButtonInput::Keyboard) && response.has_focus();
                    let owned = self.history_press_stamp(direction, response.id).is_some_and(|stamp| self.location_history.press == Some(stamp));
                    if (pointer && owned) || keyboard { self.history_step(direction, ui.ctx()); }
                }
            }
            if let Some(message) = &self.location_history.message { ui.label(egui::RichText::new(message).small().color(MUTED)); }
        });
        if matches!(input, ButtonInput::Release | ButtonInput::Click(_)) {
            self.location_history.press = None;
        }
    }

    pub(super) fn finish_history_frame(&mut self, ctx: &egui::Context) {
        let Some(focus) = self.location_history.focus.take() else {
            return;
        };
        if focus.pass == ctx.cumulative_pass_nr()
            && focus.generation == self.generation
            && focus.navigation == self.navigation_epoch
            && self.active_document == Some(focus.document)
            && !self.history_blocked(ctx)
            && ctx.memory(|memory| memory.focused() == focus.focused || memory.focused().is_none())
        {
            ctx.memory_mut(|memory| {
                memory.request_focus(egui::Id::new(("editor", focus.document)))
            });
            ctx.request_repaint();
        }
    }
}

#[cfg(test)]
mod tests;
