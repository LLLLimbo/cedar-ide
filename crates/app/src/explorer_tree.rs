//! Session-local, explicitly loaded Explorer snapshots. The single outstanding
//! browser ticket also covers the original flat browser; mode changes never
//! create a second queued List request.
use crate::{chrome, model::parent_path, system_fonts, CedarApp, Job, MUTED, RED, TEXT};
use cedar_protocol::{Entry, Operation, Payload};
use eframe::egui::{self, NumExt, RichText};
use std::collections::{BTreeMap, HashSet};

#[path = "project_reveal.rs"]
mod project_reveal;
#[cfg(test)]
pub(crate) use project_reveal::{MAX_PROJECT_REVEAL_LISTS, PROJECT_REVEAL_DEADLINE};

pub(crate) const MAX_SNAPSHOTS: usize = 64;
pub(crate) const MAX_ROWS: usize = 4096;
pub(crate) const MAX_TEXT_BYTES: usize = 512 * 1024;
pub(crate) const MAX_DEPTH: usize = 32;
pub(crate) const MAX_PATH_BYTES: usize = 4096;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Mode {
    #[default]
    Flat,
    Tree,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Ticket {
    pub generation: u64,
    pub mode: Mode,
    pub mode_epoch: u64,
    pub epoch: u64,
    pub path: String,
    pub request: u64,
}

#[derive(Default)]
struct Branch {
    epoch: u64,
    // Separate from the request epoch: a failed refresh keeps this accepted
    // snapshot selectable, while replacing it invalidates captured choices.
    admission: u64,
    entries: Option<Vec<Entry>>,
    stale: bool,
    error: Option<String>,
}

#[derive(Default)]
pub(crate) struct Explorer {
    pub mode: Mode,
    pub(crate) mode_epoch: u64,
    pub(crate) epoch: u64,
    pub outstanding: Option<Ticket>,
    branches: BTreeMap<String, Branch>,
    pub selected: String,
    pub message: Option<String>,
    flat_loaded: bool,
    flat_admission: Option<(u64, String)>,
    flat_error: Option<String>,
    flat_failed_path: Option<String>,
    flat_row_scope: Option<egui::Id>,
    flat_input: FlatInput,
    name_search: NameSearch,
    key_intent: Option<KeyIntent>,
    pub(crate) key_reveal: Option<String>,
    pointer_blocked: bool,
    exhausted: bool,
    held_keys: HashSet<egui::Key>,
    claimed_keys: HashSet<egui::Key>,
    retry_path: Option<String>,
    reveal: Option<project_reveal::RevealIntent>,
    reveal_response: Option<(egui::Response, bool)>,
}

/// Original-batch admission survives another focused surface claiming held
/// keys. Keep only fixed metadata, never text/paste/IME bodies.
pub(crate) struct InputBatch {
    key: Option<egui::Key>,
    pointer_blocked: bool,
}

impl InputBatch {
    pub(crate) fn capture(input: &egui::RawInput) -> Self {
        let pointer_blocked = input
            .events
            .iter()
            .filter(|event| actionable(event))
            .any(|event| {
                !matches!(
                    event,
                    egui::Event::PointerButton {
                        button: egui::PointerButton::Primary,
                        ..
                    }
                )
            });
        let mut events = input.events.iter().filter(|event| actionable(event));
        let first = events.next();
        let key = if events.next().is_none() {
            match first {
                Some(egui::Event::Key {
                    key,
                    pressed: true,
                    repeat: false,
                    modifiers,
                    ..
                }) if modifiers.is_none() && is_tree_key(key) => Some(*key),
                _ => None,
            }
        } else {
            None
        };
        Self {
            key,
            pointer_blocked,
        }
    }
}

struct KeyIntent {
    key: egui::Key,
    generation: u64,
    mode_epoch: u64,
    focused: egui::Id,
    selected: String,
    epoch: u64,
    flat_identity: Option<FocusIdentity>,
}

/// Only input-lifetime metadata is retained, never composition or pasted text.
/// Flat movement requires a fresh, otherwise unoccupied keyboard gesture.
#[derive(Default)]
struct FlatInput {
    keys: HashSet<egui::Key>,
    pointers: [bool; 5],
    composing: bool,
}

impl FlatInput {
    fn capture(&mut self, input: &egui::RawInput) -> bool {
        if !input.focused
            || input
                .events
                .iter()
                .any(|event| matches!(event, egui::Event::WindowFocused(false)))
        {
            *self = Self::default();
            return false;
        }
        let clear = self.keys.is_empty()
            && !self.pointers.iter().any(|pressed| *pressed)
            && !self.composing;
        for event in &input.events {
            match event {
                egui::Event::Key { key, pressed, .. } => {
                    if *pressed {
                        self.keys.insert(*key);
                    } else {
                        self.keys.remove(key);
                    }
                }
                egui::Event::PointerButton {
                    button, pressed, ..
                } => {
                    self.pointers[*button as usize] = *pressed;
                }
                egui::Event::Ime(egui::ImeEvent::Preedit(_)) => self.composing = true,
                egui::Event::Ime(egui::ImeEvent::Commit(_) | egui::ImeEvent::Disabled) => {
                    self.composing = false;
                }
                _ => {}
            }
        }
        clear
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Row {
    pub entry: Entry,
    pub depth: usize,
    pub expanded: bool,
    pub loaded: bool,
    pub stale: bool,
    pub error: Option<String>,
    pub loading: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct FileAdmission(pub u64);

pub(crate) struct LoadedFile<'a> {
    pub path: &'a str,
    pub admission: FileAdmission,
}

#[derive(Clone, PartialEq, Eq)]
pub(super) struct FocusIdentity {
    mode: Mode,
    mode_epoch: u64,
    epoch: u64,
    selected: String,
    directory: String,
    flat_admission: Option<(u64, String)>,
}

// Project name search is a local, literal, case-sensitive view of admitted
// rows, not a provider query. This is the same scalar bound as loaded-file search.
const MAX_NAME_QUERY_CHARS: usize = 4096;

#[derive(Clone, PartialEq, Eq)]
struct NameContext {
    generation: u64,
    mode: Mode,
    mode_epoch: u64,
    epoch: u64,
    directory: String,
    directory_request: u64,
    flat_admission: Option<(u64, String)>,
    navigation: u64,
}

struct NameSession {
    context: NameContext,
    serial: u64,
    revision: u64,
    query: String,
    admitted: Option<String>,
    matches: Vec<String>,
    selected: Option<usize>,
    origin: Option<String>,
    composing: bool,
    paused: bool,
}
impl NameSession {
    fn id(&self) -> egui::Id {
        egui::Id::new(("project_name_query", self.context.generation, self.serial))
    }
}

#[derive(Default)]
struct NameSearch {
    session: Option<NameSession>,
    serial: u64,
    claimed: HashSet<egui::Key>,
    frame: Option<NameFrame>,
    press: Option<NameContext>,
    button_id: Option<egui::Id>,
    // A cancelled composition must not become a commit in a later query.
    ime_barrier: bool,
    rejected_pointer: bool,
}
struct NameFrame {
    context: NameContext,
    focused: Option<egui::Id>,
    owner: Option<(u64, u64)>,
    selected: Option<String>,
    seed: Option<String>,
    command: Option<egui::Key>,
    button_key: Option<egui::Key>,
    pointer_clean: bool,
    pointer_pressed: bool,
    query_owned: bool,
    edit_owned: bool,
    had_ime: bool,
    was_composing: bool,
    observed: bool,
}

fn name_query_valid(query: &str) -> bool {
    !query.is_empty()
        && !query.chars().any(char::is_control)
        && query.chars().take(MAX_NAME_QUERY_CHARS + 1).count() <= MAX_NAME_QUERY_CHARS
}

pub(crate) fn row_id(path: &str) -> egui::Id {
    egui::Id::new(("explorer_tree_row", path))
}

pub(crate) fn retry_id(kind: &str, path: &str) -> egui::Id {
    egui::Id::new(("explorer_retry", kind, path))
}

/// Explicit interaction identity is necessary: push_id scopes still inherit
/// the parent's positional auto counter for ordinary Button widgets.
pub(crate) fn retry_button(
    ui: &mut egui::Ui,
    kind: &str,
    path: &str,
    label: &str,
    enabled: bool,
) -> egui::Response {
    ui.add_enabled_ui(enabled, |ui| {
        let padding = ui.spacing().button_padding;
        let galley = egui::WidgetText::from(label).into_galley(
            ui,
            Some(egui::TextWrapMode::Wrap),
            (ui.available_width() - padding.x * 2.0).max(0.0),
            egui::TextStyle::Button,
        );
        let size = (galley.size() + padding * 2.0).at_least(ui.spacing().interact_size);
        let (_, rect) = ui.allocate_space(size);
        let response = ui.interact(rect, retry_id(kind, path), egui::Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
        });
        if ui.is_rect_visible(rect) {
            let visuals = ui.style().interact(&response);
            ui.painter().rect(
                rect.expand(visuals.expansion),
                visuals.corner_radius,
                visuals.weak_bg_fill,
                visuals.bg_stroke,
                egui::StrokeKind::Inside,
            );
            let text_pos = ui
                .layout()
                .align_size_within_rect(galley.size(), rect.shrink2(padding))
                .min;
            ui.painter().galley(text_pos, galley, visuals.text_color());
        }
        response
    })
    .inner
    .on_hover_text(format!("Retry directory: /{path}"))
}

fn descendant(path: &str, ancestor: &str) -> bool {
    path != ancestor
        && (ancestor.is_empty()
            || path
                .strip_prefix(ancestor)
                .is_some_and(|rest| rest.starts_with('/')))
}

fn depth(path: &str) -> usize {
    if path.is_empty() {
        0
    } else {
        path.split('/').count()
    }
}

fn valid_path(path: &str) -> bool {
    path.len() <= MAX_PATH_BYTES
        && !path.contains(['\0', '\\', ':'])
        && (path.is_empty()
            || path
                .split('/')
                .all(|part| !part.is_empty() && part != "." && part != ".."))
}

fn validate_entries(path: &str, entries: &[Entry], tree: bool) -> Result<(), String> {
    if !valid_path(path) || (tree && depth(path) > MAX_DEPTH) {
        return Err("Explorer directory path exceeds its path or depth limit".into());
    }
    let mut names = HashSet::new();
    let mut paths = HashSet::new();
    let mut bytes = 0usize;
    if entries.len() > MAX_ROWS {
        return Err("Explorer limit: a complete directory must fit within 4,096 rows".into());
    }
    for entry in entries {
        if entry.name.is_empty()
            || entry.name == "."
            || entry.name == ".."
            || entry.name.contains(['/', '\\', '\0', ':'])
            || !valid_path(&entry.path)
            || parent_path(&entry.path) != path
            || entry.path.rsplit('/').next() != Some(entry.name.as_str())
            || entry.path.is_empty()
            || (tree && depth(&entry.path) > MAX_DEPTH)
            || !names.insert(&entry.name)
            || !paths.insert(&entry.path)
        {
            return Err("Explorer rejected the whole directory: rows must be unique immediate children with matching names and valid paths (tree depth at most 32)".into());
        }
        bytes = bytes
            .checked_add(entry.name.len())
            .and_then(|bytes| bytes.checked_add(entry.path.len()))
            .ok_or("Explorer name/path byte count overflowed")?;
        if bytes > MAX_TEXT_BYTES {
            return Err(
                "Explorer limit: complete directory names and paths must fit within 512 KiB".into(),
            );
        }
    }
    Ok(())
}

fn sort_entries(entries: &mut [Entry]) {
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name))
    });
}

impl Explorer {
    #[cfg(test)]
    pub(super) fn claims(&self, key: egui::Key) -> bool {
        self.claimed_keys.contains(&key)
    }

    pub(crate) fn usage(&self) -> Option<(usize, usize, usize)> {
        let mut rows = 0usize;
        let mut bytes = 0usize;
        for (path, branch) in &self.branches {
            bytes = bytes.checked_add(path.len())?;
            if let Some(entries) = &branch.entries {
                rows = rows.checked_add(entries.len())?;
                for entry in entries {
                    bytes = bytes
                        .checked_add(entry.name.len())?
                        .checked_add(entry.path.len())?;
                }
            }
        }
        Some((self.branches.len(), rows, bytes))
    }
    pub fn reset_connection(&mut self) {
        let mode = self.mode;
        let held_keys = std::mem::take(&mut self.held_keys);
        let claimed_keys = std::mem::take(&mut self.claimed_keys);
        let flat_input = std::mem::take(&mut self.flat_input);
        let mut name_search = std::mem::take(&mut self.name_search);
        name_search.cancel();
        // Connection reset also clears pending jobs. Generation + request IDs
        // exclude every prior connection reply, allowing fresh local epochs.
        *self = Self {
            mode,
            held_keys,
            claimed_keys,
            flat_input,
            name_search,
            ..Self::default()
        };
    }

    fn exhausted(&mut self) {
        self.exhausted = true;
        self.key_intent = None;
        self.message = Some(
            "Explorer request identity is exhausted. Restart Cedar before browsing again".into(),
        );
    }

    fn advance_epoch(&mut self) -> bool {
        if let Some(epoch) = self
            .epoch
            .checked_add(1)
            .filter(|epoch| !self.exhausted && *epoch < u64::MAX)
        {
            self.epoch = epoch;
            true
        } else {
            self.exhausted();
            false
        }
    }

    fn current(&self, ticket: &Ticket, generation: u64) -> bool {
        !self.exhausted
            && ticket.generation == generation
            && self.mode == ticket.mode
            && self.mode_epoch == ticket.mode_epoch
            && (ticket.mode == Mode::Flat
                || self
                    .branches
                    .get(&ticket.path)
                    .is_some_and(|branch| branch.epoch == ticket.epoch))
    }

    fn row(&self, entry: Entry) -> Row {
        let branch = self.branches.get(&entry.path);
        Row {
            depth: depth(&entry.path),
            expanded: entry.is_dir && branch.is_some(),
            loaded: branch.is_some_and(|branch| branch.entries.is_some()),
            stale: branch.is_some_and(|branch| branch.stale),
            error: branch.and_then(|branch| branch.error.clone()),
            loading: self.outstanding.as_ref().is_some_and(|ticket| {
                ticket.mode == Mode::Tree
                    && ticket.path == entry.path
                    && self.current(ticket, ticket.generation)
            }),
            entry,
        }
    }

    fn append_rows(&self, path: &str, rows: &mut Vec<Row>) {
        if let Some(entries) = self
            .branches
            .get(path)
            .and_then(|branch| branch.entries.as_ref())
        {
            for entry in entries {
                rows.push(self.row(entry.clone()));
                if entry.is_dir {
                    self.append_rows(&entry.path, rows);
                }
            }
        }
    }

    fn visible_rows(&self) -> Vec<Row> {
        let mut rows = vec![self.row(Entry {
            name: "/".into(),
            path: String::new(),
            is_dir: true,
        })];
        self.append_rows("", &mut rows);
        rows
    }

    fn admit(&mut self, path: &str, mut entries: Vec<Entry>) -> Result<(), String> {
        validate_entries(path, &entries, true)?;
        // Refresh removes only caches whose ancestor directory was removed or
        // became a file. Surviving expanded folders keep their loaded children.
        let directories: HashSet<&str> = entries
            .iter()
            .filter(|entry| entry.is_dir)
            .map(|entry| entry.path.as_str())
            .collect();
        let retained = |candidate: &str| {
            if candidate == path {
                return false;
            }
            if !descendant(candidate, path) {
                return true;
            }
            directories
                .iter()
                .any(|dir| candidate == *dir || descendant(candidate, dir))
        };
        let mut rows = entries.len();
        let mut bytes = path.len();
        let mut snapshots = 1usize;
        for entry in &entries {
            bytes = bytes
                .checked_add(entry.name.len())
                .and_then(|bytes| bytes.checked_add(entry.path.len()))
                .ok_or("Explorer name/path byte count overflowed")?;
        }
        for (candidate, branch) in &self.branches {
            if !retained(candidate) {
                continue;
            }
            snapshots = snapshots
                .checked_add(1)
                .ok_or("Explorer snapshot count overflowed")?;
            bytes = bytes
                .checked_add(candidate.len())
                .ok_or("Explorer path byte count overflowed")?;
            if let Some(entries) = &branch.entries {
                rows = rows
                    .checked_add(entries.len())
                    .ok_or("Explorer row count overflowed")?;
                for entry in entries {
                    bytes = bytes
                        .checked_add(entry.name.len())
                        .and_then(|bytes| bytes.checked_add(entry.path.len()))
                        .ok_or("Explorer name/path byte count overflowed")?;
                }
            }
        }
        if snapshots > MAX_SNAPSHOTS || rows > MAX_ROWS || bytes > MAX_TEXT_BYTES {
            return Err("Explorer cache limit: the whole branch was rejected (64 directories, 4,096 rows, 512 KiB of names/paths). Collapse a loaded branch or use Flat mode, then explicitly Retry".into());
        }
        let remove: Vec<_> = self
            .branches
            .keys()
            .filter(|candidate| candidate.as_str() != path && !retained(candidate))
            .cloned()
            .collect();
        drop(directories);
        for candidate in remove {
            self.branches.remove(&candidate);
        }
        sort_entries(&mut entries);
        let branch = self
            .branches
            .get_mut(path)
            .expect("current ticket has a branch");
        branch.entries = Some(entries);
        branch.admission = self.epoch;
        branch.stale = false;
        branch.error = None;
        if !self
            .visible_rows()
            .iter()
            .any(|row| row.entry.path == self.selected)
        {
            self.selected = path.into();
        }
        Ok(())
    }

    fn fail(&mut self, path: &str, message: String) {
        if let Some(branch) = self.branches.get_mut(path) {
            branch.stale = branch.entries.is_some();
            branch.error = Some(message.chars().take(2048).collect());
        }
    }
}

fn paint_project_row(
    ui: &egui::Ui,
    response: &egui::Response,
    entry: &Entry,
    galley: std::sync::Arc<egui::Galley>,
    selected: bool,
    depth: usize,
    expanded: Option<bool>,
) {
    let rect = response.rect;
    if !ui.is_rect_visible(rect) {
        return;
    }
    if selected || response.has_focus() || response.hovered() {
        ui.painter().rect_filled(
            rect,
            4,
            if selected {
                chrome::SELECTED
            } else {
                chrome::HOVER
            },
        );
    }
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect,
            4,
            egui::Stroke::new(1.0_f32, chrome::FOCUS),
            egui::StrokeKind::Inside,
        );
    }
    let indent = (depth as f32 * chrome::INDENT).min((rect.width() - 8.0).max(0.0) * 0.35);
    let x = rect.left() + 2.0 + indent;
    let icon_rect = |left| {
        egui::Rect::from_center_size(
            egui::pos2(left + 8.0, rect.center().y),
            egui::vec2(16.0, 16.0),
        )
    };
    if entry.is_dir {
        chrome::paint_icon(
            ui,
            icon_rect(x),
            if expanded == Some(true) {
                chrome::Icon::ChevronDown
            } else {
                chrome::Icon::ChevronRight
            },
            MUTED,
        );
    }
    chrome::paint_icon(
        ui,
        icon_rect(x + 17.0),
        chrome::file_icon(&entry.path, entry.is_dir),
        MUTED,
    );
    let painter = ui.painter().with_clip_rect(ui.clip_rect().intersect(rect));
    painter.galley(
        egui::pos2(x + 38.0, rect.center().y - galley.size().y / 2.0),
        galley,
        TEXT,
    );
}

impl CedarApp {
    pub(super) fn project_focus_identity(&self) -> FocusIdentity {
        FocusIdentity {
            mode: self.explorer.mode,
            mode_epoch: self.explorer.mode_epoch,
            epoch: self.explorer.epoch,
            selected: self.explorer.selected.clone(),
            directory: self.directory.clone(),
            flat_admission: self.explorer.flat_admission.clone(),
        }
    }

    pub(super) fn project_focus_row_current(&self, path: &str) -> bool {
        self.ready()
            && if self.explorer.mode == Mode::Flat {
                self.explorer
                    .flat_admission
                    .as_ref()
                    .is_some_and(|(_, directory)| directory == &self.directory)
            } else {
                // The caller supplies a row from this pass's visible_rows output.
                // Its parent snapshot and the final mode/epoch check prove its
                // currentness without rebuilding the whole tree for every row.
                path.is_empty()
                    || self
                        .explorer
                        .branches
                        .get(&parent_path(path))
                        .is_some_and(|branch| branch.entries.is_some())
            }
    }

    fn explorer_input_blocked(&self, ctx: &egui::Context) -> bool {
        self.explorer.mode != Mode::Tree || self.project_row_input_blocked(ctx)
    }

    fn project_row_input_blocked(&self, ctx: &egui::Context) -> bool {
        !self.ready()
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

    /// Called by eframe before egui begin_pass. Otherwise egui schedules spatial
    /// arrow focus before row handlers run, including the very first key after
    /// pointer/Tab focus. Only one clean row-key intent survives to this frame.
    pub(crate) fn explorer_tree_input(
        &mut self,
        ctx: &egui::Context,
        input: &egui::RawInput,
        original: InputBatch,
        consumed: &mut [bool],
    ) {
        self.explorer.key_intent = None;
        self.explorer.key_reveal = None;
        self.explorer.pointer_blocked = original.pointer_blocked;
        self.capture_project_name_input(ctx, input, consumed);
        let flat_clear = self.explorer.flat_input.capture(input);
        if !input.focused
            || input
                .events
                .iter()
                .any(|event| matches!(event, egui::Event::WindowFocused(false)))
        {
            self.explorer.held_keys.clear();
            self.explorer.claimed_keys.clear();
            return;
        }
        let focused = ctx.memory(|memory| memory.focused());
        let owns_tree_row = !self.explorer_input_blocked(ctx)
            && focused.is_some_and(|focused| {
                self.explorer
                    .visible_rows()
                    .iter()
                    .any(|row| row_id(&row.entry.path) == focused)
            });
        let owns_flat_row = self.explorer.mode == Mode::Flat
            && !self.project_row_input_blocked(ctx)
            && self.project_focus_row_current("")
            && self.explorer.flat_row_scope.is_some_and(|scope| {
                self.entries
                    .iter()
                    .any(|entry| Some(scope.with(("flat_entry", &entry.path))) == focused)
            });
        let candidate = if owns_tree_row {
            original.key
        } else if owns_flat_row && flat_clear {
            original.key.filter(is_flat_key)
        } else {
            None
        };
        // winit leaves repeat=false for egui to fill during begin_pass. Track
        // these seven keys before removing them. A tree-claimed key stays owned
        // until release even if Open moves focus to the editor in the meantime.
        let mut action = None;
        for (index, event) in input.events.iter().enumerate() {
            let keep = (|| {
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
                if !is_tree_key(key) {
                    return true;
                }
                let claimed = self.explorer.claimed_keys.contains(key);
                if !pressed {
                    self.explorer.held_keys.remove(key);
                    self.explorer.claimed_keys.remove(key);
                    return !claimed;
                }
                let fresh = self.explorer.held_keys.insert(*key);
                // Flat owns all variants of its four movement keys while a
                // row is focused. Modified/mixed gestures are inert rather
                // than falling through to egui's pre-layout spatial focus.
                if (owns_tree_row && modifiers.is_none()) || (owns_flat_row && is_flat_key(key)) {
                    if fresh || claimed {
                        self.explorer.claimed_keys.insert(*key);
                    }
                    if fresh && !repeat && candidate == Some(*key) {
                        action = Some(*key);
                    }
                    return false;
                }
                !claimed
            })();
            consumed[index] |= !keep;
        }
        self.explorer.key_intent = action.map(|key| KeyIntent {
            key,
            generation: self.generation,
            mode_epoch: self.explorer.mode_epoch,
            focused: focused.expect("a row action requires focus"),
            selected: self.explorer.selected.clone(),
            epoch: self.explorer.epoch,
            flat_identity: owns_flat_row.then(|| self.project_focus_identity()),
        });
    }

    pub(crate) fn explorer_tree_shortcuts(&mut self, ctx: &egui::Context) {
        self.project_name_shortcuts(ctx);
        let Some(intent) = self.explorer.key_intent.take() else {
            return;
        };
        if intent.flat_identity.is_some() {
            self.explorer_flat_shortcut(ctx, intent);
            return;
        }
        if self.explorer_input_blocked(ctx)
            || !ctx.input(|input| input.focused)
            || intent.generation != self.generation
            || intent.mode_epoch != self.explorer.mode_epoch
            || intent.epoch != self.explorer.epoch
            || intent.selected != self.explorer.selected
            || ctx.memory(|memory| memory.focused()) != Some(intent.focused)
        {
            return;
        }
        let rows = self.explorer.visible_rows();
        let Some(index) = rows
            .iter()
            .position(|row| row_id(&row.entry.path) == intent.focused)
        else {
            return;
        };
        let row = &rows[index];
        let path = row.entry.path.clone();
        let mut target = path.clone();
        match intent.key {
            egui::Key::ArrowUp => target = rows[index.saturating_sub(1)].entry.path.clone(),
            egui::Key::ArrowDown => {
                target = rows[(index + 1).min(rows.len() - 1)].entry.path.clone()
            }
            egui::Key::Home => target = rows[0].entry.path.clone(),
            egui::Key::End => target = rows[rows.len() - 1].entry.path.clone(),
            egui::Key::ArrowRight if row.entry.is_dir => {
                if !row.expanded {
                    self.explorer_expand(&path);
                } else if row.loaded {
                    if let Some(child) = rows
                        .get(index + 1)
                        .filter(|child| parent_path(&child.entry.path) == path)
                    {
                        target = child.entry.path.clone();
                    }
                }
            }
            egui::Key::ArrowLeft if row.expanded => self.explorer_collapse(&path),
            egui::Key::ArrowLeft => target = parent_path(&path),
            egui::Key::Enter => {
                self.explorer_activate(&path);
                if !row.entry.is_dir {
                    return;
                }
            }
            _ => {}
        }
        self.explorer_select(&target);
        self.explorer.key_reveal = Some(target.clone());
        ctx.memory_mut(|memory| memory.request_focus(row_id(&target)));
        ctx.request_repaint();
    }

    fn explorer_flat_shortcut(&mut self, ctx: &egui::Context, intent: KeyIntent) {
        if self.project_row_input_blocked(ctx)
            || !ctx.input(|input| input.focused)
            || intent.generation != self.generation
            || intent.flat_identity.as_ref() != Some(&self.project_focus_identity())
            || ctx.memory(|memory| memory.focused()) != Some(intent.focused)
            || !self.project_focus_row_current("")
        {
            return;
        }
        let Some(scope) = self.explorer.flat_row_scope else {
            return;
        };
        let Some(index) = self
            .entries
            .iter()
            .position(|entry| scope.with(("flat_entry", &entry.path)) == intent.focused)
        else {
            return;
        };
        let target = match intent.key {
            egui::Key::ArrowUp => index.saturating_sub(1),
            egui::Key::ArrowDown => (index + 1).min(self.entries.len() - 1),
            egui::Key::Home => 0,
            egui::Key::End => self.entries.len() - 1,
            _ => return,
        };
        let path = self.entries[target].path.clone();
        self.cancel_project_reveal();
        self.explorer.key_reveal = Some(path.clone());
        ctx.memory_mut(|memory| memory.request_focus(scope.with(("flat_entry", &path))));
        ctx.request_repaint();
    }

    pub(crate) fn explorer_tree_row(
        &mut self,
        ui: &mut egui::Ui,
        row: &Row,
        galley: std::sync::Arc<egui::Galley>,
    ) -> egui::Response {
        let selected = if self.explorer.name_search.session.is_some() {
            self.project_name_selected(&row.entry.path)
        } else {
            self.explorer.selected == row.entry.path
        };
        let height = chrome::ROW_HEIGHT.max(galley.size().y);
        let (_, rect) = ui.allocate_space(egui::vec2(ui.available_width(), height));
        let response = ui.interact(
            rect,
            row_id(&row.entry.path),
            if self.ready() {
                egui::Sense::click()
            } else {
                egui::Sense::hover()
            },
        );
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::Button,
                self.ready(),
                selected,
                &row.entry.name,
            )
        });
        if response.has_focus() {
            ui.memory_mut(|memory| {
                memory.set_focus_lock_filter(
                    response.id,
                    egui::EventFilter {
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        ..Default::default()
                    },
                )
            });
            self.explorer.selected = row.entry.path.clone();
        }
        paint_project_row(
            ui,
            &response,
            &row.entry,
            galley,
            selected,
            row.depth,
            Some(row.expanded),
        );
        response.on_hover_text(if row.entry.path.is_empty() {
            "Workspace root"
        } else {
            &row.entry.path
        })
    }

    pub(crate) fn explorer_flat_row(
        &mut self,
        ui: &mut egui::Ui,
        entry: &Entry,
        galley: std::sync::Arc<egui::Galley>,
        selected: bool,
    ) -> egui::Response {
        let selected = if self.explorer.name_search.session.is_some() {
            self.project_name_selected(&entry.path)
        } else {
            selected
        };
        // Preserve the existing scoped widget IDs; retain just their parent ID
        // so the raw-input hook can recognize the first key after Tab/click.
        self.explorer.flat_row_scope = Some(ui.id());
        let (_, rect) = ui.allocate_space(egui::vec2(
            ui.available_width(),
            chrome::ROW_HEIGHT.max(galley.size().y),
        ));
        let response = ui.interact(
            rect,
            ui.make_persistent_id(("flat_entry", &entry.path)),
            if self.ready() {
                egui::Sense::click()
            } else {
                egui::Sense::hover()
            },
        );
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::Button,
                self.ready(),
                selected,
                &entry.name,
            )
        });
        if response.has_focus() {
            ui.memory_mut(|memory| {
                memory.set_focus_lock_filter(
                    response.id,
                    egui::EventFilter {
                        vertical_arrows: true,
                        ..Default::default()
                    },
                )
            });
        }
        paint_project_row(ui, &response, entry, galley, selected, 0, None);
        response.on_hover_text(&entry.path)
    }

    pub(crate) fn explorer_flat_activation(&self, response: &egui::Response) -> bool {
        response.clicked()
            && (!response.clicked_by(egui::PointerButton::Primary)
                || !self.explorer.pointer_blocked)
    }

    pub(crate) fn explorer_pointer_activation(
        &self,
        ctx: &egui::Context,
        response: &egui::Response,
    ) -> bool {
        if self.explorer.pointer_blocked
            || self.explorer_input_blocked(ctx)
            || !response.clicked_by(egui::PointerButton::Primary)
        {
            return false;
        }
        ctx.input(|input| {
            input
                .raw
                .events
                .iter()
                .filter(|event| actionable(event))
                .all(|event| {
                    matches!(
                        event,
                        egui::Event::PointerButton {
                            button: egui::PointerButton::Primary,
                            ..
                        }
                    )
                })
        })
    }

    pub(crate) fn explorer_set_mode(&mut self, mode: Mode) {
        if self.explorer.mode == mode {
            return;
        }
        self.cancel_project_name_search();
        self.cancel_project_reveal();
        let Some(epoch) = self
            .explorer
            .mode_epoch
            .checked_add(1)
            .filter(|_| !self.explorer.exhausted)
        else {
            self.explorer.exhausted();
            return;
        };
        self.explorer.mode = mode;
        self.explorer.mode_epoch = epoch;
        self.explorer.branches.clear();
        self.explorer.flat_loaded = false;
        self.explorer.flat_admission = None;
        self.explorer.selected.clear();
        self.explorer.message = None;
        self.explorer.retry_path = None;
        self.explorer.key_intent = None;
        self.explorer.key_reveal = None;
        // Keep the outstanding slot until its actual response arrives. No
        // deferred expansion or refresh is queued by changing the mode.
    }

    pub(crate) fn explorer_busy(&self) -> bool {
        self.explorer.outstanding.is_some()
    }

    pub(crate) fn explorer_visible_rows(&self) -> Vec<Row> {
        self.explorer.visible_rows()
    }

    pub(crate) fn explorer_select(&mut self, path: &str) {
        self.cancel_project_reveal();
        if self
            .explorer
            .visible_rows()
            .iter()
            .any(|row| row.entry.path == path)
        {
            self.explorer.selected = path.into();
        }
    }

    pub(crate) fn explorer_scope(&self) -> String {
        if self.explorer.mode == Mode::Flat {
            return self.directory.clone();
        }
        self.explorer
            .visible_rows()
            .iter()
            .find(|row| row.entry.path == self.explorer.selected)
            .map_or_else(String::new, |row| {
                if row.entry.is_dir {
                    row.entry.path.clone()
                } else {
                    parent_path(&row.entry.path)
                }
            })
    }

    pub(crate) fn explorer_scope_entries(&self) -> &[Entry] {
        if self.explorer.mode == Mode::Flat {
            return &self.entries;
        }
        self.explorer
            .branches
            .get(&self.explorer_scope())
            .and_then(|branch| branch.entries.as_deref())
            .unwrap_or(&[])
    }

    pub(crate) fn explorer_scope_loaded(&self) -> bool {
        if self.explorer.mode == Mode::Flat {
            return self.explorer.flat_loaded || !self.entries.is_empty();
        }
        self.explorer
            .branches
            .get(&self.explorer_scope())
            .is_some_and(|branch| branch.entries.is_some())
    }

    /// Only accepted snapshots from this mode/session are sources. Retained
    /// flat display rows after a reset or mode switch are deliberately absent.
    pub(crate) fn explorer_loaded_files(&self) -> Result<Vec<LoadedFile<'_>>, String> {
        let mut files = Vec::new();
        if self.explorer.mode == Mode::Flat {
            let Some((admission, path)) = &self.explorer.flat_admission else {
                return Ok(files);
            };
            if path != &self.directory {
                return Err(
                    "Loaded Explorer files are unavailable: the accepted directory changed".into(),
                );
            }
            validate_entries(path, &self.entries, false)?;
            files.extend(
                self.entries
                    .iter()
                    .filter(|entry| !entry.is_dir)
                    .map(|entry| LoadedFile {
                        path: &entry.path,
                        admission: FileAdmission(*admission),
                    }),
            );
        } else {
            let Some((snapshots, rows, bytes)) = self.explorer.usage() else {
                return Err("Loaded Explorer files are unavailable: cache size overflowed".into());
            };
            if snapshots > MAX_SNAPSHOTS || rows > MAX_ROWS || bytes > MAX_TEXT_BYTES {
                return Err("Loaded Explorer files refused the whole snapshot: Explorer exceeds 64 directories, 4,096 rows, or 512 KiB".into());
            }
            for branch in self.explorer.branches.values() {
                if let Some(entries) = &branch.entries {
                    files.extend(entries.iter().filter(|entry| !entry.is_dir).map(|entry| {
                        LoadedFile {
                            path: &entry.path,
                            admission: FileAdmission(branch.admission),
                        }
                    }));
                }
            }
        }
        Ok(files)
    }

    pub(crate) fn explorer_loaded_directory_count(&self) -> usize {
        if self.explorer.mode == Mode::Flat {
            usize::from(
                self.explorer
                    .flat_admission
                    .as_ref()
                    .is_some_and(|(_, path)| path == &self.directory),
            )
        } else {
            self.explorer
                .branches
                .values()
                .filter(|branch| branch.entries.is_some())
                .count()
        }
    }

    pub(crate) fn explorer_loaded_file_current(
        &self,
        path: &str,
        admission: FileAdmission,
    ) -> bool {
        if self.explorer.mode == Mode::Flat {
            self.explorer
                .flat_admission
                .as_ref()
                .is_some_and(|(current, directory)| {
                    *current == admission.0
                        && directory == &self.directory
                        && self
                            .entries
                            .iter()
                            .any(|entry| entry.path == path && !entry.is_dir)
                })
        } else {
            self.explorer
                .branches
                .get(&parent_path(path))
                .is_some_and(|branch| {
                    branch.admission == admission.0
                        && branch.entries.as_ref().is_some_and(|entries| {
                            entries
                                .iter()
                                .any(|entry| entry.path == path && !entry.is_dir)
                        })
                })
        }
    }

    pub(crate) fn explorer_loaded_file_stale(
        &self,
        path: &str,
        admission: FileAdmission,
    ) -> Option<bool> {
        if !self.explorer_loaded_file_current(path, admission) {
            return None;
        }
        Some(if self.explorer.mode == Mode::Flat {
            self.explorer.flat_error.is_some()
        } else {
            self.explorer.branches.get(&parent_path(path))?.stale
        })
    }

    pub(crate) fn explorer_new_file(&mut self) {
        self.cancel_project_name_search();
        self.cancel_project_reveal();
        let scope = self.explorer_scope();
        self.new_file = true;
        self.new_path = if scope.is_empty() {
            String::new()
        } else {
            format!("{scope}/")
        };
    }

    pub(crate) fn explorer_expand(&mut self, path: &str) {
        self.cancel_project_name_search();
        self.cancel_project_reveal();
        if self.explorer.branches.contains_key(path) {
            return;
        }
        self.explorer_refresh(path);
    }

    pub(crate) fn explorer_refresh(&mut self, path: &str) {
        self.cancel_project_name_search();
        self.cancel_project_reveal();
        if self.explorer.mode == Mode::Flat {
            self.list(path.into());
            return;
        }
        if !self.ready() {
            return;
        }
        if self.explorer_busy() {
            self.explorer.message = Some("Explorer is busy with one directory request. Wait, then try again; no request was queued".into());
            return;
        }
        if self.next_request == 0 || self.next_request == u64::MAX || self.explorer.exhausted {
            self.explorer.exhausted();
            return;
        }
        if !self
            .explorer
            .visible_rows()
            .iter()
            .any(|row| row.entry.path == path && row.entry.is_dir)
        {
            return;
        }
        if !valid_path(path) || depth(path) > MAX_DEPTH {
            self.explorer.message = Some(
                "Explorer path/depth limit: use a path of at most 4,096 bytes and depth 32".into(),
            );
            return;
        }
        if !self.explorer.branches.contains_key(path) {
            let fits = self.explorer.usage().is_some_and(|(snapshots, _, bytes)| {
                snapshots < MAX_SNAPSHOTS
                    && bytes
                        .checked_add(path.len())
                        .is_some_and(|bytes| bytes <= MAX_TEXT_BYTES)
            });
            if !fits {
                self.explorer.message = Some("Explorer cache limit: 64 directories or 512 KiB of retained names/paths. Collapse a loaded branch or use Flat mode, then try again".into());
                self.explorer.retry_path = Some(path.into());
                return;
            }
        }
        if !self.explorer.advance_epoch() {
            return;
        }
        let epoch = self.explorer.epoch;
        self.explorer.branches.entry(path.into()).or_default().epoch = epoch;
        let ticket = Ticket {
            generation: self.generation,
            mode: Mode::Tree,
            mode_epoch: self.explorer.mode_epoch,
            epoch,
            path: path.into(),
            request: self.next_request,
        };
        let request = self.request(
            Operation::List { path: path.into() },
            Job::TreeList {
                ticket: ticket.clone(),
            },
        );
        if request != 0 {
            self.explorer.outstanding = Some(ticket);
            self.explorer.message = None;
            self.explorer.retry_path = None;
        }
    }

    pub(crate) fn explorer_collapse(&mut self, path: &str) {
        self.cancel_project_name_search();
        self.cancel_project_reveal();
        self.explorer.advance_epoch();
        self.explorer
            .branches
            .retain(|candidate, _| candidate != path && !descendant(candidate, path));
        if descendant(&self.explorer.selected, path) {
            self.explorer.selected = path.into();
        }
    }

    pub(crate) fn explorer_activate(&mut self, path: &str) {
        let Some(row) = self
            .explorer
            .visible_rows()
            .into_iter()
            .find(|row| row.entry.path == path)
        else {
            return;
        };
        self.explorer_select(path);
        if row.entry.is_dir {
            if row.expanded {
                self.explorer_collapse(path);
            } else {
                self.explorer_expand(path);
            }
        } else {
            self.open(path.into(), None);
        }
    }

    pub(crate) fn explorer_saved(&mut self, path: &str) {
        self.cancel_project_name_search();
        self.cancel_project_reveal();
        if self.explorer.mode == Mode::Tree {
            self.explorer.advance_epoch();
            if let Some(branch) = self.explorer.branches.get_mut(&parent_path(path)) {
                branch.stale = true;
            }
            self.explorer.message = Some(format!(
                "Saved file's directory is stale: /{}. Select it and Refresh explicitly",
                parent_path(path)
            ));
        } else {
            self.list(self.directory.clone());
        }
    }

    pub(crate) fn explorer_list_flat(&mut self, path: String) {
        self.cancel_project_name_search();
        self.cancel_project_reveal();
        if self.explorer.mode != Mode::Flat {
            return;
        }
        if !valid_path(&path) {
            self.explorer.message = Some(
                "Explorer requires a valid relative directory path of at most 4,096 bytes".into(),
            );
            return;
        }
        if self.explorer_busy() {
            self.explorer.message = Some("Explorer is busy with one directory request. Wait, then try again; no request was queued".into());
            return;
        }
        if self.next_request == 0 || self.next_request == u64::MAX || self.explorer.exhausted {
            self.explorer.exhausted();
            return;
        }
        let ticket = Ticket {
            generation: self.generation,
            mode: Mode::Flat,
            mode_epoch: self.explorer.mode_epoch,
            epoch: 0,
            path: path.clone(),
            request: self.next_request,
        };
        let request = self.request(Operation::List { path: path.clone() }, Job::List { path });
        if request != 0 {
            self.directory_request = request;
            self.explorer.outstanding = Some(ticket);
            self.explorer.message = None;
        }
    }

    pub(crate) fn explorer_apply_list(
        &mut self,
        event: crate::Event,
        path: String,
        tree_ticket: Option<Ticket>,
    ) {
        if !event.connected {
            self.disconnected(
                event
                    .result
                    .err()
                    .unwrap_or_else(|| "The connection closed while loading Explorer".into()),
            );
            return;
        }
        let Some(ticket) = self.explorer.outstanding.as_ref() else {
            return;
        };
        if ticket.request != event.id
            || ticket.generation != event.generation
            || ticket.path != path
        {
            return;
        }
        let ticket = self.explorer.outstanding.take().unwrap();
        if !self.explorer.current(&ticket, self.generation)
            || (ticket.mode == Mode::Tree && tree_ticket.as_ref() != Some(&ticket))
            || (ticket.mode == Mode::Flat
                && (tree_ticket.is_some() || event.id != self.directory_request))
        {
            return;
        }
        let result = match event.result {
            Ok(Payload::Entries { entries }) => Ok(entries),
            Ok(_) => Err("Explorer expected a complete directory listing; response ignored".into()),
            Err(error) => Err(error),
        };
        let admitted;
        if ticket.mode == Mode::Tree {
            if !self.explorer.advance_epoch() {
                return;
            }
            let result = result.and_then(|entries| self.explorer.admit(&path, entries));
            admitted = result.is_ok();
            if let Err(error) = result {
                self.explorer.fail(&path, error);
            }
            self.cjk_seen |= self
                .explorer
                .branches
                .get(&path)
                .and_then(|branch| branch.entries.as_ref())
                .is_some_and(|entries| {
                    entries
                        .iter()
                        .any(|entry| system_fonts::contains_cjk(&entry.name))
                });
        } else {
            let result = result.and_then(|entries| {
                validate_entries(&path, &entries, false)?;
                Ok(entries)
            });
            admitted = result.is_ok();
            match result {
                Ok(mut entries) => {
                    sort_entries(&mut entries);
                    self.cjk_seen |= entries
                        .iter()
                        .any(|entry| system_fonts::contains_cjk(&entry.name));
                    self.directory = path;
                    self.entries = entries;
                    self.explorer.flat_loaded = true;
                    self.explorer.flat_admission = Some((ticket.request, self.directory.clone()));
                    self.explorer.flat_error = None;
                    self.explorer.flat_failed_path = None;
                }
                Err(error) => {
                    self.explorer.flat_error = Some(error.chars().take(2048).collect());
                    self.explorer.flat_failed_path = Some(path);
                }
            }
        }
        self.project_reveal_list_settled(&ticket, admitted);
    }

    pub(crate) fn explorer_status(&mut self, ui: &mut egui::Ui, resized: bool) {
        if self.explorer_busy() {
            ui.label(RichText::new("Loading one directory…").small().color(MUTED));
        }
        if let Some(message) = &self.explorer.message {
            let label = egui::Label::new(RichText::new(message).small().color(RED));
            // Only the completed path-bearing result needs a bounded row.
            // Native Label truncation retains the full text on hover.
            if message.starts_with(project_reveal::PROJECT_SELECTION_COMPLETED) {
                ui.add(label.truncate());
            } else {
                ui.add(label);
            }
        }
        if self.explorer.mode == Mode::Tree {
            if let Some(path) = self.explorer.retry_path.clone() {
                ui.add(
                    egui::Label::new(
                        RichText::new(format!("Retry directory: /{path}"))
                            .small()
                            .color(MUTED),
                    )
                    .truncate(),
                )
                .on_hover_text(&path);
                let response = retry_button(
                    ui,
                    "tree_limit",
                    &path,
                    "Retry branch",
                    self.ready() && !self.explorer_busy(),
                );
                if response.gained_focus() || (resized && response.has_focus()) {
                    response.scroll_to_me(None);
                }
                #[cfg(test)]
                crate::workspace_access_tests::record(ui, "explorer_status_retry", &response);
                if response.clicked() {
                    self.explorer_refresh(&path);
                }
            }
        }
        if self.explorer.mode == Mode::Flat {
            if let Some(error) = self.explorer.flat_error.clone() {
                let path = self
                    .explorer
                    .flat_failed_path
                    .clone()
                    .unwrap_or_else(|| self.directory.clone());
                ui.label(
                    RichText::new(format!("Stale listing · {error}"))
                        .small()
                        .color(RED),
                );
                ui.add(
                    egui::Label::new(
                        RichText::new(format!("Retry directory: /{path}"))
                            .small()
                            .color(MUTED),
                    )
                    .truncate(),
                )
                .on_hover_text(&path);
                let response = retry_button(
                    ui,
                    "flat",
                    &path,
                    "Retry",
                    self.ready() && !self.explorer_busy(),
                );
                if response.gained_focus() || (resized && response.has_focus()) {
                    response.scroll_to_me(None);
                }
                #[cfg(test)]
                crate::workspace_access_tests::record(ui, "explorer_status_retry", &response);
                if response.clicked() {
                    self.list(path);
                }
            }
        }
    }
}

impl NameSearch {
    fn cancel(&mut self) {
        if self
            .session
            .take()
            .is_some_and(|session| session.composing || session.paused)
        {
            self.ime_barrier = true;
        }
        self.press = None;
    }
}

fn name_command(key: egui::Key) -> bool {
    matches!(
        key,
        egui::Key::ArrowUp | egui::Key::ArrowDown | egui::Key::Enter | egui::Key::Escape
    )
}
fn name_text_event(event: &egui::Event) -> bool {
    matches!(
        event,
        egui::Event::Text(_)
            | egui::Event::Paste(_)
            | egui::Event::Ime(_)
            | egui::Event::Copy
            | egui::Event::Cut
    )
}
fn name_printable_key(key: egui::Key) -> bool {
    // Explicit contiguous printable groups in pinned egui 0.31.1.
    key == egui::Key::Space
        || (egui::Key::Colon..=egui::Key::Quote).contains(&key)
        || (egui::Key::Num0..=egui::Key::Num9).contains(&key)
        || (egui::Key::A..=egui::Key::Z).contains(&key)
}

impl CedarApp {
    fn project_name_context(&self) -> NameContext {
        NameContext {
            generation: self.generation,
            mode: self.explorer.mode,
            mode_epoch: self.explorer.mode_epoch,
            epoch: self.explorer.epoch,
            directory: self.directory.clone(),
            directory_request: self.directory_request,
            flat_admission: self.explorer.flat_admission.clone(),
            navigation: self.navigation_epoch,
        }
    }

    fn project_name_entries(&self) -> Vec<Entry> {
        if !self.ready() {
            return Vec::new();
        }
        if self.explorer.mode == Mode::Flat {
            if !self.project_focus_row_current("") {
                return Vec::new();
            }
            self.entries.clone()
        } else {
            self.explorer
                .visible_rows()
                .into_iter()
                .map(|row| row.entry)
                .collect()
        }
    }

    fn project_name_row_id(&self, path: &str) -> Option<egui::Id> {
        if self.explorer.mode == Mode::Tree {
            Some(row_id(path))
        } else {
            self.explorer
                .flat_row_scope
                .map(|scope| scope.with(("flat_entry", path)))
        }
    }

    pub(crate) fn cancel_project_name_search(&mut self) {
        self.explorer.name_search.cancel();
    }

    fn capture_project_name_input(
        &mut self,
        ctx: &egui::Context,
        raw: &egui::RawInput,
        consumed: &mut [bool],
    ) {
        let context = self.project_name_context();
        let focused = ctx.memory(|memory| memory.focused());
        let window = raw.focused
            && !raw
                .events
                .iter()
                .any(|event| matches!(event, egui::Event::WindowFocused(false)));
        let allowed = !self.project_row_input_blocked(ctx)
            && !self.file_structure_open()
            && !ctx.memory(|memory| memory.any_popup_open());
        let query_owned = self
            .explorer
            .name_search
            .session
            .as_ref()
            .is_some_and(|session| Some(session.id()) == focused);
        let owner = self
            .explorer
            .name_search
            .session
            .as_ref()
            .map(|session| (session.serial, session.revision));
        let selected = self
            .explorer
            .name_search
            .session
            .as_ref()
            .and_then(|session| {
                session
                    .selected
                    .and_then(|index| session.matches.get(index).cloned())
            });
        let row_owned = allowed
            && window
            && self
                .project_name_entries()
                .iter()
                .any(|entry| self.project_name_row_id(&entry.path) == focused);
        let clear = self.explorer.flat_input.keys.is_empty()
            && !self
                .explorer
                .flat_input
                .pointers
                .iter()
                .any(|pressed| *pressed)
            && !self.explorer.flat_input.composing;
        let was_composing = self.explorer.flat_input.composing
            || self
                .explorer
                .name_search
                .session
                .as_ref()
                .is_some_and(|session| session.composing);
        let had_ime = raw
            .events
            .iter()
            .any(|event| matches!(event, egui::Event::Ime(_)));
        if had_ime && !query_owned {
            self.explorer.name_search.ime_barrier = true;
        }
        let actions: Vec<_> = raw
            .events
            .iter()
            .filter(|event| actionable(event))
            .collect();
        let clean_key = if let [egui::Event::Key {
            key,
            pressed: true,
            repeat: false,
            modifiers,
            ..
        }] = actions.as_slice()
        {
            modifiers.is_none().then_some(*key)
        } else {
            None
        };
        let pointer_clean = actions.iter().all(|event| {
            matches!(
                event,
                egui::Event::PointerButton {
                    button: egui::PointerButton::Primary,
                    ..
                }
            )
        });
        let pointer_pressed = actions.iter().any(|event| {
            matches!(
                event,
                egui::Event::PointerButton {
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    ..
                }
            )
        });
        let button_owned = allowed
            && window
            && focused.is_some()
            && focused == self.explorer.name_search.button_id;
        let button_key = if button_owned && clear {
            match actions.as_slice() {
                [egui::Event::Key {
                    key: egui::Key::Enter | egui::Key::Space,
                    pressed: true,
                    repeat: false,
                    modifiers,
                    ..
                }] if modifiers.is_none() => clean_key,
                [egui::Event::Key {
                    key: egui::Key::Space,
                    pressed: true,
                    repeat: false,
                    modifiers,
                    ..
                }, egui::Event::Text(text)]
                    if modifiers.is_none() && text.as_str() == " " =>
                {
                    Some(egui::Key::Space)
                }
                _ => None,
            }
        } else {
            None
        };
        let seed = if row_owned && clear {
            match actions.as_slice() {
                [egui::Event::Key {
                    key,
                    pressed: true,
                    repeat: false,
                    modifiers,
                    ..
                }, egui::Event::Text(text)]
                    if name_printable_key(*key)
                        && !modifiers.ctrl
                        && !modifiers.alt
                        && !modifiers.command
                        && !modifiers.mac_cmd =>
                {
                    let bounded: String = text.chars().take(MAX_NAME_QUERY_CHARS).collect();
                    name_query_valid(&bounded).then_some(bounded)
                }
                _ => None,
            }
        } else {
            None
        };
        // Do not enumerate TextEdit's editing shortcuts here. Current-owner
        // key/text batches reach native editing; ordinary global shortcuts keep
        // their existing route. Our four commands and Tab are separate gestures.
        let edit_owned = query_owned
            && allowed
            && window
            && actions.iter().all(|event| match event {
                egui::Event::Key { key, modifiers, .. } => {
                    (!name_command(*key) && *key != egui::Key::Tab)
                        || (was_composing && had_ime && name_command(*key) && modifiers.is_none())
                }
                event => name_text_event(event),
            });
        let row_text_batch = row_owned && raw.events.iter().any(name_text_event);
        // Keep egui's native press/release lifetime intact. A rejected mixed
        // press also owns its later release, even if that release is clean.
        self.explorer.pointer_blocked |= self.explorer.name_search.rejected_pointer;
        for event in &raw.events {
            if let egui::Event::PointerButton {
                button: egui::PointerButton::Primary,
                pressed,
                ..
            } = event
            {
                if !pressed {
                    self.explorer.name_search.rejected_pointer = false;
                } else if row_text_batch {
                    self.explorer.name_search.rejected_pointer = true;
                }
            }
        }
        let mut command = None;
        for (index, event) in raw.events.iter().enumerate() {
            if (row_owned || (query_owned && !edit_owned) || button_key.is_some())
                && name_text_event(event)
            {
                consumed[index] = true;
            }
            if let egui::Event::Key {
                key,
                pressed,
                repeat,
                ..
            } = event
            {
                let claimed = self.explorer.name_search.claimed.contains(key);
                if !pressed {
                    consumed[index] |= self.explorer.name_search.claimed.remove(key);
                } else if claimed
                    || row_text_batch
                    || button_key == Some(*key)
                    || (query_owned && name_command(*key))
                {
                    consumed[index] = true;
                    self.explorer.name_search.claimed.insert(*key);
                    if query_owned
                        && allowed
                        && window
                        && clear
                        && !claimed
                        && !repeat
                        && clean_key == Some(*key)
                    {
                        command = Some(*key);
                    }
                }
            }
        }
        if !window {
            self.cancel_project_name_search();
            self.explorer.name_search.claimed.clear();
            self.explorer.name_search.rejected_pointer = false;
        }
        self.explorer.name_search.frame = Some(NameFrame {
            context,
            focused,
            owner,
            selected,
            seed,
            command,
            button_key,
            pointer_clean,
            pointer_pressed,
            query_owned,
            edit_owned,
            had_ime,
            was_composing,
            observed: false,
        });
    }

    // Run immediately after navigation replies, and before query/editor render.
    // Every key from a stale query loses its authority, independent of native
    // editing admission. In particular Ctrl+H/K/U and modified cursor movement
    // must never replay into a newly focused dirty editor.
    pub(crate) fn guard_project_name_input(&mut self, ctx: &egui::Context) {
        let context = self.project_name_context();
        let focused = ctx.memory(|memory| memory.focused());
        let current = self
            .explorer
            .name_search
            .session
            .as_ref()
            .is_some_and(|session| {
                session.context == context
                    && focused == Some(session.id())
                    && !self.project_row_input_blocked(ctx)
                    && !self.file_structure_open()
                    && ctx.input(|input| input.focused)
                    && !ctx.memory(|memory| memory.any_popup_open())
            });
        let stale_owned = self
            .explorer
            .name_search
            .frame
            .as_ref()
            .is_some_and(|frame| {
                frame.query_owned
                    && (!current || frame.context != context || frame.focused != focused)
            });
        if stale_owned {
            ctx.input_mut(|input| {
                input.events.retain(|event| {
                    !name_text_event(event) && !matches!(event, egui::Event::Key { .. })
                })
            });
        }
        if self.explorer.name_search.session.is_some() && !current {
            self.cancel_project_name_search();
        }
    }
}

impl CedarApp {
    fn open_project_name_search(&mut self, ctx: &egui::Context, query: String) {
        if self.project_row_input_blocked(ctx) || self.file_structure_open() {
            return;
        }
        let Some(serial) = self.explorer.name_search.serial.checked_add(1) else {
            return;
        };
        let focused = ctx.memory(|memory| memory.focused());
        let entries = self.project_name_entries();
        let origin = entries
            .iter()
            .find(|entry| self.project_name_row_id(&entry.path) == focused)
            .or_else(|| {
                entries
                    .iter()
                    .find(|entry| entry.path == self.explorer.selected)
            })
            .or_else(|| entries.first())
            .map(|entry| entry.path.clone());
        self.cancel_project_reveal();
        if !query.is_empty() {
            self.explorer.name_search.ime_barrier = false;
        }
        self.explorer.name_search.serial = serial;
        let session = NameSession {
            context: self.project_name_context(),
            serial,
            revision: 0,
            query,
            admitted: None,
            matches: Vec::new(),
            selected: None,
            origin,
            composing: false,
            paused: false,
        };
        ctx.memory_mut(|memory| memory.request_focus(session.id()));
        self.explorer.name_search.session = Some(session);
        self.update_project_name_matches();
    }

    fn update_project_name_matches(&mut self) {
        let entries = self.project_name_entries();
        let context = self.project_name_context();
        let Some(session) = &mut self.explorer.name_search.session else {
            return;
        };
        session.matches.clear();
        session.selected = None;
        session.admitted = None;
        if session.context != context
            || session.composing
            || session.paused
            || !name_query_valid(&session.query)
        {
            return;
        }
        session.matches = entries
            .into_iter()
            .filter(|entry| !entry.path.is_empty() && entry.name.contains(&session.query))
            .map(|entry| entry.path)
            .collect();
        session.admitted = Some(session.query.clone());
        if let Some(path) = session.matches.first() {
            session.selected = Some(0);
            self.explorer.key_reveal = Some(path.clone());
            if self.explorer.mode == Mode::Tree {
                self.explorer.selected = path.clone();
            }
        }
    }

    pub(crate) fn project_name_selected(&self, path: &str) -> bool {
        self.explorer
            .name_search
            .session
            .as_ref()
            .is_some_and(|session| {
                session
                    .selected
                    .and_then(|index| session.matches.get(index))
                    .is_some_and(|selected| selected == path)
            })
    }

    fn project_name_shortcuts(&mut self, ctx: &egui::Context) {
        self.guard_project_name_input(ctx);
        let Some(mut frame) = self.explorer.name_search.frame.take() else {
            return;
        };
        let current = frame.context == self.project_name_context()
            && frame.focused == ctx.memory(|memory| memory.focused())
            && !self.project_row_input_blocked(ctx)
            && !self.file_structure_open()
            && ctx.input(|input| input.focused)
            && self.navigation.input_frame_current(ctx);
        if current {
            if let Some(seed) = frame.seed.take() {
                if self.explorer.name_search.session.is_none() {
                    self.open_project_name_search(ctx, seed);
                }
            } else if let Some(key) = frame.command.take() {
                let valid = self
                    .explorer
                    .name_search
                    .session
                    .as_ref()
                    .is_some_and(|session| {
                        frame.owner == Some((session.serial, session.revision))
                            && frame.focused == Some(session.id())
                            && frame.selected
                                == session
                                    .selected
                                    .and_then(|index| session.matches.get(index).cloned())
                    });
                if valid {
                    let session = self.explorer.name_search.session.as_mut().unwrap();
                    if key == egui::Key::Escape {
                        let target = session
                            .selected
                            .and_then(|index| session.matches.get(index).cloned())
                            .or_else(|| session.origin.clone());
                        self.cancel_project_name_search();
                        if let Some(path) = target {
                            if self
                                .project_name_entries()
                                .iter()
                                .any(|entry| entry.path == path)
                            {
                                if let Some(id) = self.project_name_row_id(&path) {
                                    ctx.memory_mut(|memory| memory.request_focus(id));
                                }
                                self.explorer.key_reveal = Some(path);
                            }
                        }
                    } else if !session.composing
                        && !session.paused
                        && session.admitted.as_ref() == Some(&session.query)
                    {
                        if let Some(index) = session.selected {
                            match key {
                                egui::Key::ArrowUp | egui::Key::ArrowDown => {
                                    let next = if key == egui::Key::ArrowUp {
                                        index.saturating_sub(1)
                                    } else {
                                        (index + 1).min(session.matches.len() - 1)
                                    };
                                    session.selected = Some(next);
                                    let path = session.matches[next].clone();
                                    self.explorer.key_reveal = Some(path.clone());
                                    if self.explorer.mode == Mode::Tree {
                                        self.explorer.selected = path;
                                    }
                                }
                                egui::Key::Enter => {
                                    let path = session.matches[index].clone();
                                    let target = self
                                        .project_name_entries()
                                        .into_iter()
                                        .find(|entry| entry.path == path);
                                    self.cancel_project_name_search();
                                    if let Some(entry) = target {
                                        if self.explorer.mode == Mode::Tree {
                                            self.explorer_activate(&entry.path);
                                        } else if entry.is_dir {
                                            self.list(entry.path);
                                        } else {
                                            self.open(entry.path, None);
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
        self.explorer.name_search.frame = Some(frame);
    }

    pub(crate) fn project_name_galley(
        &self,
        ui: &egui::Ui,
        entry: &Entry,
        name: &str,
        state: &str,
        width: f32,
    ) -> std::sync::Arc<egui::Galley> {
        let query = self
            .explorer
            .name_search
            .session
            .as_ref()
            .and_then(|session| session.admitted.as_deref());
        let format = egui::TextFormat {
            font_id: egui::TextStyle::Body.resolve(ui.style()),
            color: TEXT,
            ..Default::default()
        };
        let mut job = egui::text::LayoutJob::default();
        if let Some((query, start)) = query
            .filter(|query| !entry.path.is_empty() && !query.is_empty())
            .and_then(|query| name.find(query).map(|start| (query, start)))
        {
            job.append(&name[..start], 0.0, format.clone());
            let mut highlight = format.clone();
            highlight.background = chrome::SELECTED;
            highlight.color = egui::Color32::WHITE;
            job.append(&name[start..start + query.len()], 0.0, highlight);
            job.append(&name[start + query.len()..], 0.0, format.clone());
        } else {
            job.append(name, 0.0, format.clone());
        }
        job.append(state, 0.0, format);
        egui::WidgetText::from(job).into_galley(
            ui,
            Some(egui::TextWrapMode::Truncate),
            width,
            egui::TextStyle::Body,
        )
    }

    pub(crate) fn project_name_ui(&mut self, ui: &mut egui::Ui) {
        self.guard_project_name_input(ui.ctx());
        let context = self.project_name_context();
        if self.explorer.name_search.session.is_none() {
            let button = ui.add_enabled(
                self.ready() && !self.project_row_input_blocked(ui.ctx()),
                egui::Button::new("Search names…"),
            );
            self.explorer.name_search.button_id = Some(button.id);
            #[cfg(test)]
            crate::workspace_access_tests::record(ui, "project_name_search", &button);
            if let Some(frame) = &mut self.explorer.name_search.frame {
                if frame.pointer_clean
                    && frame.pointer_pressed
                    && button.is_pointer_button_down_on()
                {
                    self.explorer.name_search.press = Some(frame.context.clone());
                }
                let pointer = frame.pointer_clean
                    && !frame.was_composing
                    && self.explorer.name_search.press.as_ref() == Some(&context);
                let keyboard = frame.focused == Some(button.id)
                    && button.has_focus()
                    && frame.button_key.take().is_some()
                    && !frame.was_composing;
                let open = frame.context == context && ((button.clicked() && pointer) || keyboard);
                if !frame.pointer_clean {
                    self.explorer.name_search.press = None;
                }
                if open {
                    self.open_project_name_search(ui.ctx(), String::new());
                }
            }
        }
        let Some(mut session) = self.explorer.name_search.session.take() else {
            return;
        };
        let id = session.id();
        let before = session.query.clone();
        let before_flags = (session.composing, session.paused);
        let mut close = false;
        let output = ui
            .horizontal(|ui| {
                // desired_width is INNER width. Preserve the actual native margins,
                // inter-item spacing and 26px IconButton inside the sidebar bounds.
                let margin = egui::Margin::symmetric(4, 2);
                let width =
                    ui.available_width() - margin.sum().x - ui.spacing().item_spacing.x - 26.0;
                let output = egui::TextEdit::singleline(&mut session.query)
                    .id(id)
                    .margin(margin)
                    .desired_width(width.max(24.0))
                    .char_limit(MAX_NAME_QUERY_CHARS)
                    .hint_text("Name contains (case-sensitive)")
                    .show(ui);
                ui.memory_mut(|memory| {
                    memory.set_focus_lock_filter(
                        id,
                        egui::EventFilter {
                            horizontal_arrows: true,
                            vertical_arrows: true,
                            escape: true,
                            tab: false,
                        },
                    )
                });
                #[cfg(test)]
                {
                    crate::workspace_access_tests::record_text_edit_receipts(ui, &output);
                    crate::workspace_access_tests::record(
                        ui,
                        "project_name_query",
                        &output.response,
                    );
                }
                let button = ui
                    .push_id("project_name_close", |ui| {
                        ui.add(chrome::IconButton::new(
                            chrome::Icon::Close,
                            "Close name search",
                        ))
                    })
                    .inner;
                #[cfg(test)]
                crate::workspace_access_tests::record(ui, "project_name_close", &button);
                close = button.clicked();
                output
            })
            .inner;
        let current = session.context == self.project_name_context()
            && output.response.has_focus()
            && !output.response.lost_focus();
        let owner = self
            .explorer
            .name_search
            .frame
            .as_ref()
            .is_some_and(|frame| {
                !frame.observed
                    && frame.query_owned
                    && frame.edit_owned
                    && frame.context == session.context
                    && frame.focused == Some(id)
                    && frame.owner == Some((session.serial, session.revision))
                    && !output.response.gained_focus()
                    && self.navigation.input_frame_current(ui.ctx())
            });
        let had_ime = self
            .explorer
            .name_search
            .frame
            .as_ref()
            .is_some_and(|frame| frame.had_ime);
        let was_composing = self
            .explorer
            .name_search
            .frame
            .as_ref()
            .is_some_and(|frame| frame.was_composing);
        let mut admitted_edit = false;
        if owner && current {
            for receipt in &output.ime_receipts {
                use egui::text_edit::ImeEventOutcome::*;
                match receipt.outcome {
                    Enabled => {}
                    PreeditApplied => {
                        self.explorer.name_search.ime_barrier = false;
                        session.composing = true;
                        session.paused = true;
                        admitted_edit = false;
                    }
                    CommitAccepted { .. } => {
                        session.composing = false;
                        session.paused = self.explorer.name_search.ime_barrier;
                        admitted_edit = !session.paused;
                    }
                    CommitEmpty
                    | CommitRejectedAnchor
                    | CommitIgnoredLineBreak
                    | PreeditIgnoredLineBreak => {
                        self.explorer.name_search.ime_barrier = true;
                        session.composing = false;
                        session.paused = true;
                        admitted_edit = false;
                    }
                    Disabled => {
                        if session.composing {
                            self.explorer.name_search.ime_barrier = true;
                            session.paused = true;
                            admitted_edit = false;
                        }
                        session.composing = false;
                    }
                }
            }
            if session.query != before
                && !had_ime
                && output.ime_receipts.is_empty()
                && !was_composing
                && !session.composing
            {
                self.explorer.name_search.ime_barrier = false;
                session.paused = false;
                admitted_edit = true;
            }
        } else if session.query != before || !output.ime_receipts.is_empty() {
            self.explorer.name_search.ime_barrier = true;
            session.paused = true;
            session.composing = false;
        }
        if let Some(frame) = &mut self.explorer.name_search.frame {
            frame.observed = true;
        }
        let ime_changed = output.ime_receipts.iter().any(|receipt| {
            !matches!(
                receipt.outcome,
                egui::text_edit::ImeEventOutcome::Enabled
                    | egui::text_edit::ImeEventOutcome::Disabled
            )
        });
        if session.query != before
            || before_flags != (session.composing, session.paused)
            || ime_changed
        {
            if let Some(revision) = session.revision.checked_add(1) {
                session.revision = revision;
            } else {
                close = true;
            }
            session.admitted = None;
            session.matches.clear();
            session.selected = None;
        }
        if close || !current {
            if session.composing || session.paused {
                self.explorer.name_search.ime_barrier = true;
            }
            return;
        }
        self.cjk_seen |= system_fonts::contains_cjk(&session.query);
        self.explorer.name_search.session = Some(session);
        if admitted_edit {
            self.update_project_name_matches();
        }
        let session = self.explorer.name_search.session.as_ref().unwrap();
        let status = if session.composing {
            "Composing · loaded rows only".into()
        } else if session.paused {
            "Search paused · edit the query to resume".into()
        } else if session.query.is_empty() {
            "Type a name · loaded rows only".into()
        } else if !name_query_valid(&session.query) {
            "Use a single-line name without control characters".into()
        } else {
            format!(
                "{} {} · loaded rows only",
                session.matches.len(),
                if session.matches.len() == 1 {
                    "match"
                } else {
                    "matches"
                }
            )
        };
        ui.label(RichText::new(status).small().color(MUTED)).on_hover_text("Literal, case-sensitive names in this Flat directory or expanded Tree rows. Collapsed/unloaded directories are not searched. Typing sends no requests. Enter opens the selected exact path.");
    }

    #[cfg(test)]
    pub(crate) fn project_name_observation(&self) -> Option<(String, Vec<String>, bool, bool)> {
        self.explorer.name_search.session.as_ref().map(|session| {
            (
                session.query.clone(),
                session.matches.clone(),
                session.composing,
                session.paused,
            )
        })
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

fn is_tree_key(key: &egui::Key) -> bool {
    matches!(
        key,
        egui::Key::ArrowUp
            | egui::Key::ArrowDown
            | egui::Key::ArrowLeft
            | egui::Key::ArrowRight
            | egui::Key::Home
            | egui::Key::End
            | egui::Key::Enter
    )
}

fn is_flat_key(key: &egui::Key) -> bool {
    matches!(
        key,
        egui::Key::ArrowUp | egui::Key::ArrowDown | egui::Key::Home | egui::Key::End
    )
}
