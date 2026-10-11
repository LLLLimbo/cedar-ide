//! Report-only input ownership. Native events are captured before egui can move
//! focus; no event bodies, editor actions, or backend requests are retained.
use crate::{
    test_report_ui::{Filter, Load, TestReportPanel},
    CedarApp, Tool,
};
use eframe::egui;
use std::collections::HashSet;

#[derive(Clone, Debug, PartialEq, Eq)]
struct View {
    source: Load,
    revision: String,
    filter: Filter,
    query: String,
    visible: Vec<usize>,
    selected: Option<usize>,
}

impl View {
    fn of(panel: &TestReportPanel) -> Option<Self> {
        let snapshot = panel.snapshot.as_ref()?;
        if panel.loading.is_some() || panel.path != snapshot.source.path {
            return None;
        }
        Some(Self {
            source: snapshot.source.clone(),
            revision: snapshot.revision.clone(),
            filter: panel.filter,
            query: panel.name_filter.clone(),
            visible: panel.visible_cases(),
            selected: panel.selected,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Target {
    Case(usize),
    Back(usize),
}

#[derive(Clone)]
struct Presented {
    target: Target,
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

#[derive(Clone)]
struct Return {
    view: View,
    focus: egui::Id,
    index: usize,
}

#[derive(Default)]
pub(super) struct Keyboard {
    view: Option<View>,
    presented: Vec<Presented>,
    held: HashSet<egui::Key>,
    claimed: HashSet<egui::Key>,
    composing: bool,
    input_pass: Option<u64>,
    key: Option<Intent>,
    pressed: Option<Pointer>,
    click: Option<Pointer>,
    // Only a pointer Back after rows have rendered needs a next-frame return.
    // It is taken at the very next raw hook, never retained past that frame.
    next_return: Option<Return>,
    returning: Option<Return>,
    render: Option<(View, Action, egui::Id)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Action {
    Focus(usize),
    Details(usize),
}

pub(super) fn row_id(load: &Load, index: usize) -> egui::Id {
    egui::Id::new(("test_report_case", load.generation, load.id, index))
}

pub(super) fn back_id(load: &Load, index: usize) -> egui::Id {
    egui::Id::new(("test_report_back", load.generation, load.id, index))
}

pub(super) fn destination(visible: &[usize], current: usize, key: egui::Key) -> Option<usize> {
    let index = visible.iter().position(|item| *item == current)?;
    let destination = match key {
        egui::Key::ArrowUp => index.saturating_sub(1),
        egui::Key::ArrowDown => (index + 1).min(visible.len() - 1),
        egui::Key::Home => 0,
        egui::Key::End => visible.len() - 1,
        _ => return None,
    };
    Some(visible[destination])
}

fn report_key(key: egui::Key) -> bool {
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

// A normal Space key can have one text companion. Attribute only the exact
// single-space token to one clean Space press; paste, IME and mixed keys veto.
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
            } if modifiers.is_none() && report_key(*candidate) && key.is_none() => {
                key = Some(*candidate)
            }
            egui::Event::Key {
                key: released_key,
                pressed: false,
                modifiers,
                ..
            } if modifiers.is_none() && key == Some(*released_key) && !released => {
                released = true;
            }
            egui::Event::Text(text) if text == " " && spaces == 0 => spaces += 1,
            _ => return None,
        }
    }
    key.filter(|key| spaces == 0 || *key == egui::Key::Space)
}

impl Keyboard {
    pub(super) fn invalidate(&mut self) {
        self.view = None;
        self.presented.clear();
        self.key = None;
        self.pressed = None;
        self.click = None;
        self.next_return = None;
        self.returning = None;
        self.render = None;
        // Held/claimed keys survive state changes until their physical release.
    }

    pub(super) fn start_presenting(&mut self) {
        self.presented.clear();
    }

    pub(super) fn record(&mut self, ui: &egui::Ui, target: Target, response: &egui::Response) {
        self.presented.push(Presented {
            target,
            id: response.id,
            rect: response.rect,
            clip: ui.clip_rect(),
        });
    }

    pub(super) fn finish_presenting(
        panel: &mut TestReportPanel,
        ctx: &egui::Context,
        action: Option<Action>,
    ) {
        let view = View::of(panel);
        if let (Some(view), Some(action)) = (&view, action) {
            let focus = match action {
                Action::Focus(index) => row_id(&view.source, index),
                Action::Details(index) => back_id(&view.source, index),
            };
            if ctx.memory(|memory| memory.focused()) == Some(focus) {
                // Geometry may need another egui sizing/discard pass. This is
                // the same reveal within one native frame, never input replay.
                panel.keyboard.render = Some((view.clone(), action, focus));
            }
        }
        panel.keyboard.view = view;
    }

    pub(super) fn return_next_frame(panel: &mut TestReportPanel, index: usize, focus: egui::Id) {
        panel.keyboard.next_return = View::of(panel).map(|view| Return { view, focus, index });
    }
}

impl CedarApp {
    fn report_input_allowed(&self, ctx: &egui::Context) -> bool {
        self.ready()
            && self.tools_open
            && self.tool == Tool::Tests
            && !self.open_form
            && !self.new_file
            && !self.navigation.dialog_open()
            && !self.navigation.blocks_editor()
            && !self.foreign_modal_owns_input(ctx)
            && self
                .test_report
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.source.generation == self.generation)
    }

    pub(super) fn capture_test_report_input(
        &mut self,
        ctx: &egui::Context,
        input: &mut egui::RawInput,
    ) {
        let allowed = self.report_input_allowed(ctx);
        let current = View::of(&self.test_report);
        let focused = ctx.memory(|memory| memory.focused());
        let keyboard = &mut self.test_report.keyboard;
        keyboard.input_pass = Some(ctx.cumulative_pass_nr());
        keyboard.render = None;
        keyboard.key = None;
        keyboard.click = None;
        keyboard.returning = keyboard.next_return.take().filter(|ticket| {
            allowed
                && input.focused
                && input.events.is_empty()
                && current.as_ref() == Some(&ticket.view)
                && focused == Some(ticket.focus)
        });
        let lost_focus = !input.focused
            || input
                .events
                .iter()
                .any(|event| matches!(event, egui::Event::WindowFocused(false)));
        if lost_focus {
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
        let candidate = candidate.filter(|key| {
            source.as_ref().is_some_and(|row| {
                matches!(row.target, Target::Case(_))
                    || matches!(key, egui::Key::Enter | egui::Key::Space)
            })
        });

        // Pointer selection must finish on the same displayed identity, view,
        // and hit rectangle as its press. A stale release cannot hit a new row.
        let pointer_only = input.events.iter().all(|event| {
            matches!(
                event,
                egui::Event::PointerMoved(_)
                    | egui::Event::PointerButton {
                        button: egui::PointerButton::Primary,
                        ..
                    }
            )
        });
        if !current_presentation || !pointer_only || composition_was_active || keyboard.composing {
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
                                view: current.clone().unwrap(),
                                source,
                            });
                    } else if let Some(press) = keyboard.pressed.take() {
                        if Some(&press.view) == current.as_ref()
                            && press
                                .source
                                .rect
                                .intersect(press.source.clip)
                                .contains(*pos)
                            && keyboard.presented.iter().any(|row| {
                                row.id == press.source.id
                                    && row.target == press.source.target
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
        input.events.retain(|event| {
            let egui::Event::Key {
                key,
                pressed,
                repeat,
                modifiers,
                ..
            } = event
            else {
                return true;
            };
            if !report_key(*key) {
                return true;
            }
            let claimed = keyboard.claimed.contains(key);
            if !pressed {
                keyboard.held.remove(key);
                keyboard.claimed.remove(key);
                return !claimed;
            }
            let fresh = keyboard.held.insert(*key);
            if source.is_some() && modifiers.is_none() {
                // A key already held before report focus belongs to its
                // original consumer. Suppress the local native repeat without
                // taking its lifetime; its release must reach that consumer.
                if fresh || claimed {
                    keyboard.claimed.insert(*key);
                }
                claimed_space_press |= *key == egui::Key::Space;
                if fresh && !repeat && candidate == Some(*key) {
                    accepted = Some(*key);
                }
                return false;
            }
            claimed_space_press |= claimed && *key == egui::Key::Space;
            !claimed
        });
        // Remove the companion only for a clean matching intent, including a
        // held repeat. Mixed text stays untouched and cannot activate a row.
        if claimed_space_press && companion == Some(egui::Key::Space) {
            input
                .events
                .retain(|event| !matches!(event, egui::Event::Text(text) if text == " "));
        }
        if let (Some(key), Some(source), Some(view)) = (accepted, source, current) {
            keyboard.key = Some(Intent { view, source, key });
        }
    }

    pub(super) fn take_test_report_click(&mut self, ctx: &egui::Context) -> Option<Target> {
        let click = self.test_report.keyboard.click.take()?;
        (self.report_input_allowed(ctx)
            && self.navigation.input_frame_current(ctx)
            && Some(&click.view) == View::of(&self.test_report).as_ref())
        .then_some(click.source.target)
    }

    pub(super) fn take_test_report_action(&mut self, ctx: &egui::Context) -> Option<Action> {
        let key = self.test_report.keyboard.key.take();
        let returning = self.test_report.keyboard.returning.take();
        let current = View::of(&self.test_report);
        if !self.report_input_allowed(ctx)
            || !self.navigation.input_frame_current(ctx)
            || self.test_report.keyboard.input_pass
                != Some(
                    ctx.cumulative_pass_nr()
                        .saturating_sub(ctx.output(|output| output.num_completed_passes) as u64),
                )
            || !ctx.input(|input| input.focused)
        {
            self.test_report.keyboard.invalidate();
            return None;
        }
        if let Some(ticket) = returning {
            return (current.as_ref() == Some(&ticket.view)
                && ctx.memory(|memory| memory.focused()) == Some(ticket.focus)
                && ticket.view.visible.contains(&ticket.index))
            .then_some(Action::Focus(ticket.index));
        }
        if key.is_none() && ctx.output(|output| output.num_completed_passes) > 0 {
            return self
                .test_report
                .keyboard
                .render
                .as_ref()
                .and_then(|(view, action, focus)| {
                    (current.as_ref() == Some(view)
                        && ctx.memory(|memory| memory.focused()) == Some(*focus))
                    .then_some(*action)
                });
        }
        let intent = key?;
        if current.as_ref() != Some(&intent.view)
            || ctx.memory(|memory| memory.focused()) != Some(intent.source.id)
        {
            return None;
        }
        match intent.source.target {
            Target::Case(index) if intent.view.visible.contains(&index) => {
                if matches!(intent.key, egui::Key::Enter | egui::Key::Space) {
                    Some(Action::Details(index))
                } else {
                    destination(&intent.view.visible, index, intent.key).map(Action::Focus)
                }
            }
            Target::Back(index)
                if intent.view.selected == Some(index) && intent.view.visible.contains(&index) =>
            {
                Some(Action::Focus(index))
            }
            _ => None,
        }
    }
}
