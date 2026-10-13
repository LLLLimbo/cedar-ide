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
    alt_project_companion: bool,
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
    alt_project_companion: bool,
    owned_alt_project: bool,
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
    owned_alt_project: bool,
}

/// Keep only eligibility metadata from the immutable native batch. In
/// particular, another surface consuming its arrows must not turn mixed input
/// into a clean access shortcut or panel activation. No text/IME is retained.
pub(super) fn capture_input(app: &CedarApp, ctx: &egui::Context, raw: &egui::RawInput) {
    let mut lifetime = ctx
        .data(|data| data.get_temp::<Lifetime>(egui::Id::new(LIFETIME)))
        .unwrap_or_default();
    let owned_activation = lifetime.owned_activation;
    let owned_alt_project = lifetime.owned_alt_project
        && lifetime.keys.len() == 1
        && lifetime.keys.contains(&egui::Key::Num1)
        && !lifetime.pointers.iter().any(|pressed| *pressed)
        && !lifetime.composing;
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
    // Pointer ownership ignores IME lifecycle notifications only. Keep the
    // original view below for every keyboard shortcut/activation, and retain
    // all content-bearing IME events (including empty Preedit/Commit).
    let pointer_events: Vec<_> = events
        .iter()
        .copied()
        .filter(|event| {
            !matches!(
                event,
                egui::Event::Ime(egui::ImeEvent::Enabled | egui::ImeEvent::Disabled)
            )
        })
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
    let alt_project_companion =
        focused && keyboard_clear && alt_project_text(&raw.events, raw.modifiers, true).is_some();
    let shortcut = (focused && keyboard_clear)
        .then(|| {
            fresh_key
                .and_then(shortcut)
                .or(alt_project_companion.then_some(Action::Explorer))
        })
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
        match pointer_events.as_slice() {
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
            _ => match events.as_slice() {
                [event] => keyboard_activation(event, false),
                [event, egui::Event::Text(text)] | [egui::Event::Text(text), event]
                    if text == " " =>
                {
                    keyboard_activation(event, true)
                }
                _ => None,
            },
        }
    };
    let clean_primary_press = focused
        && fresh_pointer
        && matches!(pointer_events.as_slice(),
        [egui::Event::PointerButton { button: egui::PointerButton::Primary, pressed: true, modifiers, .. }] if modifiers.is_none());
    if !focused {
        lifetime = Lifetime::default();
    } else {
        // A poisoned press cannot become a clean click merely because its later
        // release contains no competing input. Motion and lifecycle-only IME
        // notifications keep an already clean capture; active composition was
        // checked before this batch can disable it.
        if lifetime.pointers[0]
            && (!pointer_clear || pointer_events.iter().any(|event| !matches!(event,
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
                    if *key == egui::Key::Num1 {
                        lifetime.owned_alt_project = false;
                    }
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
        alt_project_companion,
        owned_alt_project: focused && owned_alt_project,
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
    // Every surface has already classified the original batch. A completed
    // Project gesture owns only a still-held, exact Alt+1/Text("1") repeat pair.
    // A real release before a repress prevents the pair matcher from claiming
    // any of that new input; modifiers, composition and unrelated text survive.
    if original_input(ctx).is_some_and(|input| input.owned_alt_project) {
        if let Some(text) = alt_project_text(&raw.events, raw.modifiers, false) {
            consumed[text] = true;
            for (index, event) in raw.events[..text].iter().enumerate() {
                if matches!(
                    event,
                    egui::Event::Key {
                        key: egui::Key::Num1,
                        pressed: true,
                        ..
                    }
                ) {
                    consumed[index] = true;
                }
            }
        }
    }
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

/// Admit a rendered menu command through the original native gesture only.
/// Its caller owns the destination and modal policy, not workspace focus access.
pub(crate) fn claim_menu_activation(
    app: &CedarApp,
    ui: &egui::Ui,
    response: &egui::Response,
) -> bool {
    if !ui.is_enabled()
        || ui.is_sizing_pass()
        || !response.enabled()
        || clean_activation(app, ui.ctx(), response, false).is_none()
    {
        return false;
    }
    claim_activation(ui.ctx());
    true
}

/// winit emits the printable Alt+1 companion after its key press. Unlike
/// `actionable`, this matcher observes releases: text after release, a repress,
/// or a reversed/mixed batch is ordinary input, never a shortcut companion.
/// Return only an index; no text payload is retained across the native hook.
fn alt_project_text(
    events: &[egui::Event],
    modifiers: egui::Modifiers,
    fresh: bool,
) -> Option<usize> {
    if modifiers != egui::Modifiers::ALT {
        return None;
    }
    let mut ordered = events.iter().enumerate().filter(|(_, event)| {
        !matches!(
            event,
            egui::Event::PointerMoved(_) | egui::Event::MouseMoved(_)
        )
    });
    match ordered.next()?.1 {
        egui::Event::Key {
            key: egui::Key::Num1,
            pressed: true,
            repeat,
            modifiers,
            ..
        } if *modifiers == egui::Modifiers::ALT && (!fresh || !repeat) => {}
        _ => return None,
    }
    let (index, egui::Event::Text(text)) = ordered.next()? else {
        return None;
    };
    if text != "1" {
        return None;
    }
    if let Some((_, event)) = ordered.next() {
        if !matches!(
            event,
            egui::Event::Key {
                key: egui::Key::Num1,
                pressed: false,
                ..
            }
        ) || ordered.next().is_some()
        {
            return None;
        }
    }
    Some(index)
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
                    if original.alt_project_companion {
                        // Recheck both post-hook raw input and the current widget
                        // event stream. Earlier consumers cannot manufacture a
                        // clean original pair or make a stale index own new text.
                        alt_project_text(&input.raw.events, input.raw.modifiers, true)?;
                        let text = alt_project_text(&input.events, input.modifiers, true)?;
                        return Some((Action::Explorer, Some(text)));
                    }
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
                        .filter(|action| Some(*action) == original.shortcut)
                        .map(|action| (action, None))
                })
            });
        let blocked = self.workspace_access_blocked(ctx);
        let companion = action.filter(|_| !blocked).and_then(|(_, text)| text);
        // Consume repeats too, without toggling or extending a focus request.
        // The admitted initial companion must leave the editor's stream now,
        // even if a later context change prevents final focus. No input replay.
        ctx.input_mut(|input| {
            let mut index = 0;
            input.events.retain(|event| {
                let keep = Some(index) != companion && shortcut(event).is_none();
                index += 1;
                keep
            });
        });
        if blocked {
            return;
        }
        if let Some((action, _)) = action {
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
                    if let Some(intent) = &mut self.workspace_access.intent {
                        intent.alt_project_companion = companion.is_some();
                    }
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
                alt_project_companion: false,
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

    pub(super) fn select_current_file_in_project(
        &mut self,
        ctx: &egui::Context,
        response: &egui::Response,
    ) {
        let Some(pointer) = clean_activation(self, ctx, response, false) else {
            return;
        };
        if self.workspace_access_blocked(ctx) {
            return;
        }
        claim_activation(ctx);
        self.start_project_reveal(ctx);
        self.project_reveal_allow_pointer_surrender(pointer);
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
            if intent.alt_project_companion && response.has_focus() {
                ctx.data_mut(|data| {
                    let lifetime =
                        data.get_temp_mut_or_default::<Lifetime>(egui::Id::new(LIFETIME));
                    // The ordered initial batch admitted at most one press and
                    // optional trailing release. A released gesture owns nothing.
                    lifetime.owned_alt_project = lifetime.keys.contains(&egui::Key::Num1);
                });
            }
            ctx.request_repaint();
        }
    }
}

// Read-only witnesses for the isolated actual-frame Find Results diagnostic.
// Compiled out of every production build; no input or focus state is changed.
#[cfg(test)]
pub(super) fn find_activation_diagnostic(
    app: &CedarApp,
    ctx: &egui::Context,
    response: Option<&egui::Response>,
) -> String {
    let input = ctx.data(|data| data.get_temp::<Input>(egui::Id::new(INPUT)));
    let lifetime = ctx
        .data(|data| data.get_temp::<Lifetime>(egui::Id::new(LIFETIME)))
        .unwrap_or_default();
    let current = Origin::of(app, ctx);
    let origin = |value: Origin| {
        let mut without_focus = value;
        without_focus.focused = current.focused;
        format!(
            "focused={:?},current={},nonfocus_current={}",
            value.focused,
            value.current(app, ctx, true),
            without_focus.current(app, ctx, true),
        )
    };
    let activation = input.map(|input| match input.activation {
        None => "none".to_owned(),
        Some(Activation::Pointer) => "pointer".to_owned(),
        Some(Activation::Keyboard(key)) => format!("keyboard:{key:?}"),
    });
    format!(
        "pass={} input_pass={:?} current_focus={:?} input_origin={:?} press_origin={:?} activation={activation:?} primary_clean={} held_keys={:?} pointers={:?} composing={} response={:?} admission={:?} tools_open={} blocked={} navigation_blocks={} restore_focus={} foreign_modal={} find_focus={} jump_to={}",
        ctx.cumulative_pass_nr(),
        input.map(|input| input.pass),
        current.focused,
        input.map(|input| origin(input.origin)),
        lifetime.primary_origin.map(origin),
        lifetime.primary_clean,
        lifetime.keys,
        lifetime.pointers,
        lifetime.composing,
        response.map(|response| (response.id, response.clicked(), response.clicked_by(egui::PointerButton::Primary), response.has_focus(), response.is_pointer_button_down_on())),
        response.and_then(|response| clean_activation(app, ctx, response, false)),
        app.tools_open,
        app.workspace_access_blocked(ctx),
        app.navigation.blocks_editor(),
        app.navigation.restore_focus,
        app.foreign_modal_owns_input(ctx),
        app.find_focus,
        app.active().is_some_and(|doc| doc.jump_to.is_some()),
    )
}

#[cfg(test)]
pub(super) fn find_activation_diagnostic_admission(
    app: &CedarApp,
    ctx: &egui::Context,
    response: &egui::Response,
) -> Option<bool> {
    clean_activation(app, ctx, response, false)
}

#[cfg(test)]
mod pointer_lifecycle_tests {
    use super::*;

    fn pointer(pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos: egui::pos2(10.0, 10.0),
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }

    fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    // These classifier checks complement the rendered dirty-editor journeys.
    // Feed the actual raw hook; never manufacture Input/Lifetime or focus tickets.
    fn capture(app: &mut CedarApp, events: Vec<egui::Event>, modifiers: egui::Modifiers) -> Input {
        let mut raw = egui::RawInput {
            events,
            modifiers,
            focused: true,
            ..Default::default()
        };
        let original = raw.clone();
        let ctx = app.editor_ctx.clone();
        eframe::App::raw_input_hook(app, &ctx, &mut raw);
        assert_eq!(
            raw.events, original.events,
            "classification must retain original native events"
        );
        assert_eq!(raw.modifiers, original.modifiers);
        original_input(&ctx).unwrap()
    }

    #[test]
    fn active_composition_disabled_before_press_cannot_authorize_that_gesture() {
        for preedit in ["", "composing"] {
            let mut app = CedarApp::empty();
            capture(
                &mut app,
                vec![egui::Event::Ime(egui::ImeEvent::Preedit(preedit.into()))],
                egui::Modifiers::NONE,
            );
            let press = capture(
                &mut app,
                vec![egui::Event::Ime(egui::ImeEvent::Disabled), pointer(true)],
                egui::Modifiers::NONE,
            );
            assert!(!press.clean_primary_press);
            let release = capture(&mut app, vec![pointer(false)], egui::Modifiers::NONE);
            assert!(release.activation.is_none());
            // A genuinely new gesture after composition ended is eligible again.
            let fresh = capture(
                &mut app,
                vec![pointer(true), pointer(false)],
                egui::Modifiers::NONE,
            );
            assert!(matches!(fresh.activation, Some(Activation::Pointer)));
        }
    }

    #[test]
    fn lifecycle_notifications_preserve_atomic_pointer_but_never_clean_keyboard_batches() {
        for lifecycle in [egui::ImeEvent::Enabled, egui::ImeEvent::Disabled] {
            let mut app = CedarApp::empty();
            let atomic = capture(
                &mut app,
                vec![
                    pointer(true),
                    egui::Event::Ime(lifecycle.clone()),
                    pointer(false),
                ],
                egui::Modifiers::NONE,
            );
            assert!(matches!(atomic.activation, Some(Activation::Pointer)));
            for (key_code, modifiers) in [
                (egui::Key::Enter, egui::Modifiers::NONE),
                (egui::Key::Space, egui::Modifiers::NONE),
                (egui::Key::J, egui::Modifiers::COMMAND),
                (
                    egui::Key::E,
                    egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                ),
                (egui::Key::Num1, egui::Modifiers::ALT),
            ] {
                for lifecycle_first in [false, true] {
                    let mut app = CedarApp::empty();
                    let mut events = vec![
                        key(key_code, modifiers),
                        egui::Event::Ime(lifecycle.clone()),
                    ];
                    if lifecycle_first {
                        events.reverse();
                    }
                    if key_code == egui::Key::Space {
                        events.push(egui::Event::Text(" ".into()));
                    }
                    let input = capture(&mut app, events, egui::Modifiers::NONE);
                    assert!(input.activation.is_none(), "mixed {key_code:?} activation");
                    assert!(input.shortcut.is_none(), "mixed {key_code:?} shortcut");
                }
            }
        }
    }

    #[test]
    fn lifecycle_cannot_clean_mixed_or_modifier_pointer_press_and_cannot_revive_it() {
        for mixed in [
            egui::Event::Text("newer".into()),
            egui::Event::Paste("newer".into()),
            key(egui::Key::A, egui::Modifiers::NONE),
            egui::Event::Ime(egui::ImeEvent::Preedit(String::new())),
            egui::Event::Ime(egui::ImeEvent::Commit(String::new())),
        ] {
            let mut app = CedarApp::empty();
            let press = capture(
                &mut app,
                vec![
                    pointer(true),
                    egui::Event::Ime(egui::ImeEvent::Enabled),
                    mixed,
                ],
                egui::Modifiers::NONE,
            );
            assert!(!press.clean_primary_press);
            capture(
                &mut app,
                vec![egui::Event::Ime(egui::ImeEvent::Disabled)],
                egui::Modifiers::NONE,
            );
            let release = capture(&mut app, vec![pointer(false)], egui::Modifiers::NONE);
            assert!(release.activation.is_none());
        }
        let mut app = CedarApp::empty();
        let press = capture(
            &mut app,
            vec![pointer(true), egui::Event::Ime(egui::ImeEvent::Disabled)],
            egui::Modifiers::SHIFT,
        );
        assert!(!press.clean_primary_press);
        let release = capture(&mut app, vec![pointer(false)], egui::Modifiers::NONE);
        assert!(release.activation.is_none());
    }
}

#[cfg(test)]
#[path = "workspace_access_alt_project_tests.rs"]
mod alt_project_tests;
