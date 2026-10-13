//! Presentation-only workspace access. Focus intent lives for one UI pass;
//! actual rendered responses supply every destination.
use crate::{explorer_tree, search_keyboard, CedarApp, Tool};
use eframe::egui;
use std::collections::HashSet;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Target {
    Editor(u64),
    Explorer,
    Find,
    Tool(Tool),
}

struct Intent {
    pass: u64,
    generation: u64,
    document: Option<(u64, u64)>,
    session: u64,
    navigation: u64,
    focused: Option<egui::Id>,
    pointer_surrender: bool,
    tool: Tool,
    tools_open: bool,
    target: Target,
    project: Option<explorer_tree::FocusIdentity>,
    find: Option<search_keyboard::FocusIdentity>,
}

#[derive(Default)]
pub(super) struct Access {
    intent: Option<Intent>,
    response: Option<egui::Response>,
    priority: u8,
}

impl Access {
    fn wants(&self, target: Target) -> bool {
        self.intent
            .as_ref()
            .is_some_and(|intent| intent.target == target)
    }

    pub fn record(&mut self, target: Target, response: &egui::Response) {
        self.record_ranked(target, response, 1);
    }

    fn record_ranked(&mut self, target: Target, response: &egui::Response, priority: u8) -> bool {
        if self
            .intent
            .as_ref()
            .is_some_and(|intent| intent.target == target)
            && response.enabled()
            && response.sense.is_focusable()
            && (self.response.is_none() || priority > self.priority)
        {
            self.response = Some(response.clone());
            self.priority = priority;
            return true;
        }
        false
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    Toggle,
    Explorer,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Activation {
    Pointer,
    Keyboard(egui::Key),
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Input {
    pass: u64,
    origin: Origin,
    shortcut: Option<Action>,
    activation: Option<Activation>,
    owned_activation: Option<egui::Key>,
    held_primary_pointer: bool,
    clean_primary_press: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Origin {
    generation: u64,
    document: Option<(u64, u64)>,
    session: u64,
    navigation: u64,
    tool: Tool,
    tools_open: bool,
    focused: Option<egui::Id>,
    project_mode: explorer_tree::Mode,
    project_mode_epoch: u64,
    project_epoch: u64,
    directory_request: u64,
    search_request: u64,
    query: egui::Id,
}

impl Origin {
    fn of(app: &CedarApp, ctx: &egui::Context) -> Self {
        Self {
            generation: app.generation,
            document: app.active().map(|doc| (doc.id, doc.edit_version)),
            session: app.language.session,
            navigation: app.navigation_epoch,
            tool: app.tool,
            tools_open: app.tools_open,
            focused: ctx.memory(|memory| memory.focused()),
            project_mode: app.explorer.mode,
            project_mode_epoch: app.explorer.mode_epoch,
            project_epoch: app.explorer.epoch,
            directory_request: app.directory_request,
            search_request: app.search_request,
            query: egui::Id::new((&app.search_query, &app.search_scope, &app.search_masks)),
        }
    }

    fn current(&self, app: &CedarApp, ctx: &egui::Context, pointer: bool) -> bool {
        let current = Self::of(app, ctx);
        self.generation == current.generation
            && self.document == current.document
            && self.document.is_none_or(|(_, version)| version != u64::MAX)
            && self.session == current.session
            && self.navigation == current.navigation
            && self.tool == current.tool
            && self.tools_open == current.tools_open
            && self.project_mode == current.project_mode
            && self.project_mode_epoch == current.project_mode_epoch
            && self.project_epoch == current.project_epoch
            && self.directory_request == current.directory_request
            && self.search_request == current.search_request
            && self.query == current.query
            && (self.focused == current.focused || (pointer && current.focused.is_none()))
    }
}

const INPUT: &str = "workspace_access_original_input";
const LIFETIME: &str = "workspace_access_input_lifetime";
const RETIREMENT: &str = "workspace_access_ready_retirement";

/// Constructed only at the connected/current, owned-ready-reply poll gate.
/// This is an admission stamp, never a copy of the reply's payload.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct ReadyNavigation {
    pub request: u64,
    pub generation: u64,
    pub navigation: u64,
}

#[derive(Clone, Copy)]
pub(super) struct Retirement {
    input: Input,
    ready: ReadyNavigation,
    navigation: u64,
}

#[derive(Clone, Copy)]
struct PrimaryRetirement {
    origin: Origin,
    ready: ReadyNavigation,
    navigation: u64,
}

#[derive(Clone, Default)]
struct Lifetime {
    keys: HashSet<egui::Key>,
    pointers: [bool; 5],
    composing: bool,
    primary_clean: bool,
    primary_origin: Option<Origin>,
    primary_retirement: Option<PrimaryRetirement>,
    owned_activation: Option<egui::Key>,
}

/// Keep only eligibility metadata from the immutable native batch. In
/// particular, another surface consuming its arrows must not turn mixed input
/// into a clean access shortcut or panel activation. No text/IME is retained.
pub(super) fn capture_input(app: &CedarApp, ctx: &egui::Context, raw: &egui::RawInput) {
    let mut lifetime = ctx
        .data(|data| data.get_temp::<Lifetime>(egui::Id::new(LIFETIME)))
        .unwrap_or_default();
    let owned_activation = lifetime.owned_activation;
    let focused = raw.focused
        && !raw
            .events
            .iter()
            .any(|event| matches!(event, egui::Event::WindowFocused(false)));
    let events: Vec<_> = raw
        .events
        .iter()
        .filter(|event| actionable(event))
        .collect();
    let fresh_key = match events.as_slice() {
        [event @ egui::Event::Key {
            pressed: true,
            repeat: false,
            ..
        }] => Some(*event),
        _ => None,
    };
    let keyboard_clear = lifetime.keys.is_empty()
        && !lifetime.pointers.iter().any(|pressed| *pressed)
        && !lifetime.composing;
    let held_primary_pointer =
        lifetime.pointers[0] && !lifetime.pointers[1..].iter().any(|pressed| *pressed);
    let native_close_clear =
        lifetime.keys.is_empty() && !lifetime.composing && held_primary_pointer;
    let pointer_clear = lifetime.keys.is_empty()
        && !lifetime.pointers[1..].iter().any(|pressed| *pressed)
        && !lifetime.composing
        && raw.modifiers.is_none();
    let shortcut = (focused && keyboard_clear)
        .then(|| fresh_key.and_then(shortcut))
        .flatten();
    let fresh_pointer = pointer_clear && !lifetime.pointers.iter().any(|pressed| *pressed);
    let release_pointer = pointer_clear
        && lifetime.pointers[0]
        && lifetime.primary_clean
        && lifetime.primary_origin.is_some_and(|mut origin| {
            if let Some(witness) = lifetime.primary_retirement.filter(|witness| {
                witness.origin == origin
                    && witness.ready.generation == app.generation
                    && witness.ready.navigation.checked_add(1) == Some(witness.navigation)
            }) {
                origin.navigation = witness.navigation;
            }
            origin.current(app, ctx, true)
        });
    let keyboard_activation = |event: &egui::Event, companion: bool| match event {
        egui::Event::Key {
            key: key @ (egui::Key::Enter | egui::Key::Space),
            pressed: true,
            repeat: false,
            modifiers,
            ..
        } if (keyboard_clear || native_close_clear)
            && raw.modifiers.is_none()
            && modifiers.is_none()
            && (!companion || *key == egui::Key::Space) =>
        {
            Some(Activation::Keyboard(*key))
        }
        _ => None,
    };
    let activation = if !focused {
        None
    } else {
        match events.as_slice() {
            [egui::Event::PointerButton {
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers,
                ..
            }] if release_pointer && modifiers.is_none() => Some(Activation::Pointer),
            [egui::Event::PointerButton {
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: down,
                ..
            }, egui::Event::PointerButton {
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: up,
                ..
            }] if fresh_pointer && down.is_none() && up.is_none() => Some(Activation::Pointer),
            [event] => keyboard_activation(event, false),
            [event, egui::Event::Text(text)] | [egui::Event::Text(text), event] if text == " " => {
                keyboard_activation(event, true)
            }
            _ => None,
        }
    };
    let clean_primary_press = focused
        && fresh_pointer
        && matches!(events.as_slice(),
        [egui::Event::PointerButton { button: egui::PointerButton::Primary, pressed: true, modifiers, .. }] if modifiers.is_none());
    if !focused {
        lifetime = Lifetime::default();
    } else {
        // A poisoned press cannot become a clean click merely because its later
        // release contains no competing input. Motion alone keeps capture.
        if lifetime.pointers[0]
            && (!pointer_clear || events.iter().any(|event| !matches!(event,
                egui::Event::PointerButton { button: egui::PointerButton::Primary, pressed: false, modifiers, .. } if modifiers.is_none()))) {
            lifetime.primary_clean = false;
            lifetime.primary_retirement = None;
        }
        for event in &raw.events {
            match event {
                egui::Event::Key {
                    key, pressed: true, ..
                } => {
                    lifetime.keys.insert(*key);
                }
                egui::Event::Key {
                    key,
                    pressed: false,
                    ..
                } => {
                    lifetime.keys.remove(key);
                    if lifetime.owned_activation == Some(*key) {
                        lifetime.owned_activation = None;
                    }
                }
                egui::Event::PointerButton {
                    button,
                    pressed: true,
                    ..
                } => {
                    lifetime.pointers[*button as usize] = true;
                    if *button == egui::PointerButton::Primary {
                        lifetime.primary_clean = clean_primary_press;
                        lifetime.primary_origin = clean_primary_press.then(|| Origin::of(app, ctx));
                        lifetime.primary_retirement = None;
                    }
                }
                egui::Event::PointerButton {
                    button,
                    pressed: false,
                    ..
                } => {
                    lifetime.pointers[*button as usize] = false;
                    if *button == egui::PointerButton::Primary {
                        lifetime.primary_clean = false;
                        lifetime.primary_origin = None;
                        lifetime.primary_retirement = None;
                    }
                }
                egui::Event::Ime(egui::ImeEvent::Preedit(_)) => lifetime.composing = true,
                egui::Event::Ime(egui::ImeEvent::Commit(_) | egui::ImeEvent::Disabled) => {
                    lifetime.composing = false
                }
                _ => {}
            }
        }
    }
    ctx.data_mut(|data| data.insert_temp(egui::Id::new(LIFETIME), lifetime));
    let input = Input {
        pass: ctx.cumulative_pass_nr(),
        origin: Origin::of(app, ctx),
        shortcut,
        activation,
        owned_activation,
        held_primary_pointer,
        clean_primary_press,
    };
    ctx.data_mut(|data| data.insert_temp(egui::Id::new(INPUT), input));
}

/// An accepted button activation can move focus to a TextEdit. Fence only its
/// still-held native key and one exact Space companion, after every surface
/// has classified the original sequence. Leave the real release in egui's
/// stream; a later release/repress in this same batch is genuine new input.
pub(super) fn finish_input_capture(
    ctx: &egui::Context,
    raw: &egui::RawInput,
    consumed: &mut [bool],
) {
    let Some(owned) = original_input(ctx).and_then(|input| input.owned_activation) else {
        return;
    };
    let mut space_companion = false;
    for (index, event) in raw.events.iter().enumerate() {
        match event {
            egui::Event::Key {
                key, pressed: true, ..
            } if *key == owned => {
                consumed[index] = true;
                space_companion = owned == egui::Key::Space;
            }
            egui::Event::Key {
                key,
                pressed: false,
                ..
            } if *key == owned => {
                // Release is not an activation. Keeping it also clears egui's
                // own keys_down state from the originally rendered button.
                break;
            }
            egui::Event::Text(text) if space_companion && text == " " => {
                consumed[index] = true;
                space_companion = false;
            }
            egui::Event::PointerMoved(_) | egui::Event::MouseMoved(_) => {}
            _ => space_companion = false,
        }
    }
}

fn claim_activation(ctx: &egui::Context) {
    let Some(Activation::Keyboard(key)) = original_input(ctx).and_then(|input| input.activation)
    else {
        return;
    };
    ctx.data_mut(|data| {
        let lifetime = data.get_temp_mut_or_default::<Lifetime>(egui::Id::new(LIFETIME));
        if lifetime.keys.contains(&key) {
            lifetime.owned_activation = Some(key);
        }
    });
}

fn original_input(ctx: &egui::Context) -> Option<Input> {
    ctx.data(|data| data.get_temp::<Input>(egui::Id::new(INPUT)))
        .filter(|input| input.pass == ctx.cumulative_pass_nr())
}

fn input_origin_current(app: &CedarApp, ctx: &egui::Context, input: Input, pointer: bool) -> bool {
    let mut origin = input.origin;
    if let Some(witness) = ctx
        .data(|data| data.get_temp::<Retirement>(egui::Id::new(RETIREMENT)))
        .filter(|witness| {
            witness.input == input
                && witness.ready.generation == app.generation
                && witness.ready.navigation.checked_add(1) == Some(witness.navigation)
        })
    {
        origin.navigation = witness.navigation;
    }
    origin.current(app, ctx, pointer)
}

fn clean_activation(
    app: &CedarApp,
    ctx: &egui::Context,
    response: &egui::Response,
    close: bool,
) -> Option<bool> {
    if !response.clicked() {
        return None;
    }
    let input = original_input(ctx)?;
    // Native focused Close already supported a separate, clean keyboard
    // activation while the primary pointer remained held. Preserve that exact
    // ownership boundary; shortcuts and Find entry still require no pointer.
    match input.activation? {
        Activation::Pointer if input_origin_current(app, ctx, input, true) && response.clicked_by(egui::PointerButton::Primary) => Some(true),
        Activation::Keyboard(key) if (close || !input.held_primary_pointer) && input_origin_current(app, ctx, input, false) && response.has_focus() && ctx.input(|input| input.raw.events.iter().any(|event| {
            matches!(event, egui::Event::Key { key: current, pressed: true, repeat: false, modifiers, .. }
                if *current == key && modifiers.is_none())
        })) => Some(false),
        _ => None,
    }
}

fn shortcut(event: &egui::Event) -> Option<Action> {
    let egui::Event::Key { key, modifiers, .. } = event else {
        return None;
    };
    if modifiers.ctrl && modifiers.mac_cmd {
        return None;
    }
    if *key == egui::Key::Num1
        && modifiers.alt
        && !modifiers.ctrl
        && !modifiers.shift
        && !modifiers.command
        && !modifiers.mac_cmd
    {
        Some(Action::Explorer)
    } else if *key == egui::Key::J && modifiers.matches_exact(egui::Modifiers::COMMAND) {
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
    pub(super) fn prepare_workspace_access_retirement(
        &self,
        ctx: &egui::Context,
        ready: ReadyNavigation,
    ) -> Option<Retirement> {
        let input = original_input(ctx)?;
        let pointer =
            matches!(input.activation, Some(Activation::Pointer)) || input.clean_primary_press;
        if self.workspace_access_blocked(ctx)
            || ready.generation != self.generation
            || ready.navigation != self.navigation_epoch
            || input.origin.navigation != ready.navigation
            || !self.pending.contains_key(&ready.request)
            || (input.shortcut.is_none()
                && input.activation.is_none()
                && !input.clean_primary_press)
            || !input.origin.current(self, ctx, pointer)
        {
            return None;
        }
        Some(Retirement {
            input,
            ready,
            navigation: ready.navigation.checked_add(1)?,
        })
    }

    pub(super) fn finish_workspace_access_retirement(
        &self,
        ctx: &egui::Context,
        witness: Retirement,
    ) {
        let pointer = matches!(witness.input.activation, Some(Activation::Pointer))
            || witness.input.clean_primary_press;
        let mut expected = witness.input.origin;
        expected.navigation = witness.navigation;
        if original_input(ctx) != Some(witness.input)
            || witness.ready.generation != self.generation
            || witness.ready.navigation.checked_add(1) != Some(self.navigation_epoch)
            || witness.navigation != self.navigation_epoch
            || !self.pending.contains_key(&witness.ready.request)
            || !expected.current(self, ctx, pointer)
            || self.workspace_access_blocked(ctx)
        {
            return;
        }
        // Capture remains immutable. Only this exact gesture's cancellation of
        // this exact owned ready reply attests the one-step epoch transition.
        ctx.data_mut(|data| {
            data.insert_temp(egui::Id::new(RETIREMENT), witness);
            if witness.input.clean_primary_press {
                let lifetime = data.get_temp_mut_or_default::<Lifetime>(egui::Id::new(LIFETIME));
                if lifetime.primary_clean
                    && lifetime.pointers[0]
                    && lifetime.primary_origin == Some(witness.input.origin)
                {
                    lifetime.primary_retirement = Some(PrimaryRetirement {
                        origin: witness.input.origin,
                        ready: witness.ready,
                        navigation: witness.navigation,
                    });
                }
            }
        });
    }

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
        let action = original_input(ctx)
            .filter(|original| input_origin_current(self, ctx, *original, false))
            .and_then(|original| {
                ctx.input(|input| {
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
                    shortcut(event).filter(|action| Some(*action) == original.shortcut)
                })
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
                    self.request_workspace_access(
                        ctx,
                        if self.tool == Tool::Search {
                            Target::Find
                        } else {
                            Target::Tool(self.tool)
                        },
                    );
                }
                Action::Toggle => self.hide_tools_and_return(ctx),
                Action::Explorer => {
                    self.request_workspace_access(ctx, Target::Explorer);
                }
            }
        }
    }

    fn request_workspace_access(&mut self, ctx: &egui::Context, target: Target) {
        self.workspace_access = Access {
            intent: Some(Intent {
                pass: ctx.cumulative_pass_nr(),
                generation: self.generation,
                document: self.active().map(|doc| (doc.id, doc.edit_version)),
                session: self.language.session,
                navigation: self.navigation_epoch,
                focused: ctx.memory(|memory| memory.focused()),
                pointer_surrender: false,
                tool: self.tool,
                tools_open: self.tools_open,
                target,
                project: (target == Target::Explorer).then(|| self.project_focus_identity()),
                find: (target == Target::Find).then(|| self.search_focus_identity()),
            }),
            response: None,
            priority: 0,
        };
    }

    pub(super) fn show_find_results(&mut self, ctx: &egui::Context, response: &egui::Response) {
        let Some(pointer) = clean_activation(self, ctx, response, false) else {
            return;
        };
        if self.workspace_access_blocked(ctx) {
            return;
        }
        claim_activation(ctx);
        self.tool = Tool::Search;
        self.tools_open = true;
        self.request_workspace_access(ctx, Target::Find);
        self.workspace_access
            .intent
            .as_mut()
            .unwrap()
            .pointer_surrender = pointer;
    }

    /// Project rows may be clipped inside a usable viewport: return a reveal
    /// request for the existing bounded nested-scroll path. This never expands
    /// a branch or loads a directory. A zero-height viewport fails closed.
    pub(super) fn record_project_access(
        &mut self,
        ui: &egui::Ui,
        path: &str,
        response: &egui::Response,
    ) -> bool {
        if !self.workspace_access.wants(Target::Explorer)
            || self.workspace_access.priority == 2
            || !ui
                .clip_rect()
                .intersect(ui.ctx().screen_rect())
                .is_positive()
            || !self.project_focus_row_current(path)
        {
            return false;
        }
        let selected = if self.explorer.mode == explorer_tree::Mode::Tree {
            self.explorer.selected == path
        } else {
            self.active().is_some_and(|doc| doc.path == path)
        };
        self.workspace_access.record_ranked(
            Target::Explorer,
            response,
            if selected { 2 } else { 1 },
        )
    }

    pub(super) fn record_project_access_fallback(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
    ) {
        if ui
            .clip_rect()
            .intersect(ui.ctx().screen_rect())
            .is_positive()
            && self
                .workspace_access
                .record_ranked(Target::Explorer, response, 0)
        {
            response.scroll_to_me(None);
        }
    }

    pub(super) fn record_find_access(
        &mut self,
        ui: &egui::Ui,
        index: Option<usize>,
        response: &egui::Response,
    ) {
        // Find has a stricter contract than Project: never reveal clipped hits,
        // activate history, or expand a group just to choose a focus target.
        if !self.workspace_access.wants(Target::Find)
            || !response
                .rect
                .intersect(ui.clip_rect())
                .intersect(ui.ctx().screen_rect())
                .is_positive()
        {
            return;
        }
        let priority = match index {
            Some(index)
                if self.search_rows_active()
                    && self.search_state.selected_index() == Some(index)
                    && self.search_state.visible(index) =>
            {
                2
            }
            Some(_) => return,
            None => 1,
        };
        self.workspace_access
            .record_ranked(Target::Find, response, priority);
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
        let Some(pointer) = clean_activation(self, ctx, response, true) else {
            return;
        };
        if !self.workspace_access_blocked(ctx) {
            claim_activation(ctx);
            self.hide_tools_and_return(ctx);
            if let Some(intent) = &mut self.workspace_access.intent {
                intent.pointer_surrender = pointer;
            }
        }
    }

    pub(super) fn finish_workspace_access_frame(&mut self, ctx: &egui::Context) {
        let Access {
            intent, response, ..
        } = std::mem::take(&mut self.workspace_access);
        let (Some(intent), Some(response)) = (intent, response) else {
            return;
        };
        if intent.pass == ctx.cumulative_pass_nr()
            && intent.generation == self.generation
            && intent.document == self.active().map(|doc| (doc.id, doc.edit_version))
            && intent
                .document
                .is_none_or(|(_, version)| version != u64::MAX)
            && intent.session == self.language.session
            && intent.navigation == self.navigation_epoch
            && intent.tool == self.tool
            && intent.tools_open == self.tools_open
            && intent
                .project
                .as_ref()
                .is_none_or(|project| *project == self.project_focus_identity())
            && intent
                .find
                .as_ref()
                .is_none_or(|find| *find == self.search_focus_identity())
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
