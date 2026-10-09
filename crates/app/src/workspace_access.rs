//! Presentation-only workspace access. Focus intent lives for one UI pass;
//! actual rendered responses supply every destination.
use crate::{CedarApp, Tool};
use eframe::egui;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Target {
    Editor(u64),
    Refresh,
    Explorer,
    Tool(Tool),
}

struct Intent {
    pass: u64,
    generation: u64,
    document: Option<u64>,
    navigation: u64,
    focused: Option<egui::Id>,
    pointer_surrender: bool,
    tool: Tool,
    tools_open: bool,
    target: Target,
}

#[derive(Default)]
pub(super) struct Access {
    intent: Option<Intent>,
    response: Option<egui::Response>,
}

impl Access {
    pub fn record(&mut self, target: Target, response: &egui::Response) {
        if self
            .intent
            .as_ref()
            .is_some_and(|intent| intent.target == target)
            && response.enabled()
            && self.response.is_none()
        {
            if matches!(target, Target::Explorer | Target::Refresh) {
                response.scroll_to_me(None);
            }
            self.response = Some(response.clone());
        }
    }

    pub fn cancel_sidebar_reveal(&self) -> bool {
        self.intent.is_some()
    }
}

#[derive(Clone, Copy)]
enum Action {
    Toggle,
    Explorer,
}

fn shortcut(event: &egui::Event) -> Option<Action> {
    let egui::Event::Key { key, modifiers, .. } = event else {
        return None;
    };
    if modifiers.ctrl && modifiers.mac_cmd {
        return None;
    }
    if *key == egui::Key::J && modifiers.matches_exact(egui::Modifiers::COMMAND) {
        Some(Action::Toggle)
    } else if *key == egui::Key::E
        && modifiers.matches_exact(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT)
    {
        Some(Action::Explorer)
    } else {
        None
    }
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

impl CedarApp {
    fn workspace_access_blocked(&self, ctx: &egui::Context) -> bool {
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

    pub(super) fn workspace_access_shortcuts(&mut self, ctx: &egui::Context) {
        // Inspect the raw batch: previous shortcut handlers can consume a newer
        // action, but it must still win over an access shortcut. Mixed batches
        // keep their original widgets and every non-access event untouched.
        let action = ctx.input(|input| {
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
            shortcut(event)
        });
        // Consume repeats too, without toggling or extending a focus request.
        ctx.input_mut(|input| input.events.retain(|event| shortcut(event).is_none()));
        if self.workspace_access_blocked(ctx) {
            return;
        }
        if let Some(action) = action {
            match action {
                Action::Toggle if !self.tools_open => {
                    self.tools_open = true;
                    self.request_workspace_access(ctx, Target::Tool(self.tool));
                }
                Action::Toggle => self.hide_tools_and_return(ctx),
                Action::Explorer => {
                    self.tools_open = false;
                    self.request_workspace_access(ctx, Target::Refresh);
                }
            }
        }
    }

    fn request_workspace_access(&mut self, ctx: &egui::Context, target: Target) {
        self.workspace_access = Access {
            intent: Some(Intent {
                pass: ctx.cumulative_pass_nr(),
                generation: self.generation,
                document: self.active_document,
                navigation: self.navigation_epoch,
                focused: ctx.memory(|memory| memory.focused()),
                pointer_surrender: false,
                tool: self.tool,
                tools_open: self.tools_open,
                target,
            }),
            response: None,
        };
    }

    fn hide_tools_and_return(&mut self, ctx: &egui::Context) {
        self.tools_open = false;
        let target = self
            .active()
            .map_or(Target::Explorer, |doc| Target::Editor(doc.id));
        self.request_workspace_access(ctx, target);
    }

    pub(super) fn close_tools(&mut self, ctx: &egui::Context, response: &egui::Response) {
        // A pointer click or an Enter/Space activation can close the panel.
        // Competing field input remains with the still-rendered tool body.
        let focused = response.has_focus();
        let (clean, pointer) = ctx.input(|input| {
            let events: Vec<_> = input
                .raw
                .events
                .iter()
                .filter(|event| actionable(event))
                .collect();
            let pointer = !events.is_empty()
                && events.iter().all(|event| {
                    matches!(event,
                        egui::Event::PointerButton { button: egui::PointerButton::Primary, pos, .. }
                            if response.rect.contains(*pos)
                    )
                });
            let keyboard = matches!(events.as_slice(), [egui::Event::Key {
                key: egui::Key::Enter | egui::Key::Space, repeat: false, modifiers, ..
            }] if modifiers.is_none() && focused);
            (pointer || keyboard, pointer)
        });
        if clean && !self.workspace_access_blocked(ctx) {
            self.hide_tools_and_return(ctx);
            if let Some(intent) = &mut self.workspace_access.intent {
                intent.pointer_surrender = pointer;
            }
        }
    }

    pub(super) fn finish_workspace_access_frame(&mut self, ctx: &egui::Context) {
        let Access { intent, response } = std::mem::take(&mut self.workspace_access);
        let (Some(intent), Some(response)) = (intent, response) else {
            return;
        };
        if intent.pass == ctx.cumulative_pass_nr()
            && intent.generation == self.generation
            && intent.document == self.active_document
            && intent.navigation == self.navigation_epoch
            && intent.tool == self.tool
            && intent.tools_open == self.tools_open
            && !self.workspace_access_blocked(ctx)
            && ctx.memory(|memory| {
                memory.focused() == intent.focused
                    || (intent.pointer_surrender && memory.focused().is_none())
            })
        {
            // Even when closing a tall tool leaves no center clip in this pass,
            // the editor emitted this response. Focus after its input processing;
            // the repaint renders the expanded editor without a deferred intent.
            response.request_focus();
            ctx.request_repaint();
        }
    }
}
