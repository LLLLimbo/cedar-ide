//! Session-local, explicitly loaded Explorer snapshots. The single outstanding
//! browser ticket also covers the original flat browser; mode changes never
//! create a second queued List request.
use crate::{model::parent_path, system_fonts, CedarApp, Job, GREEN, MUTED, RED, TEXT};
use cedar_protocol::{Entry, Operation, Payload};
use eframe::egui::{self, NumExt, RichText};
use std::collections::{BTreeMap, HashSet};

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
    key_intent: Option<KeyIntent>,
    pub(crate) key_reveal: Option<String>,
    pointer_blocked: bool,
    exhausted: bool,
    held_keys: HashSet<egui::Key>,
    claimed_keys: HashSet<egui::Key>,
    retry_path: Option<String>,
}

struct KeyIntent {
    key: egui::Key,
    generation: u64,
    mode_epoch: u64,
    focused: egui::Id,
    selected: String,
    epoch: u64,
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
        // Connection reset also clears pending jobs. Generation + request IDs
        // exclude every prior connection reply, allowing fresh local epochs.
        *self = Self {
            mode,
            held_keys,
            claimed_keys,
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

impl CedarApp {
    fn explorer_input_blocked(&self, ctx: &egui::Context) -> bool {
        !self.ready()
            || self.explorer.mode != Mode::Tree
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
    pub(crate) fn explorer_tree_input(&mut self, ctx: &egui::Context, input: &mut egui::RawInput) {
        self.explorer.key_intent = None;
        self.explorer.key_reveal = None;
        self.explorer.pointer_blocked =
            input
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
        let owns_row = !self.explorer_input_blocked(ctx)
            && focused.is_some_and(|focused| {
                self.explorer
                    .visible_rows()
                    .iter()
                    .any(|row| row_id(&row.entry.path) == focused)
            });
        let mut events = input.events.iter().filter(|event| actionable(event));
        let first = events.next();
        let candidate = if owns_row && events.next().is_none() {
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
        // winit leaves repeat=false for egui to fill during begin_pass. Track
        // these seven keys before removing them. A tree-claimed key stays owned
        // until release even if Open moves focus to the editor in the meantime.
        let mut action = None;
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
            if owns_row && modifiers.is_none() {
                self.explorer.claimed_keys.insert(*key);
                if fresh && !repeat && candidate == Some(*key) {
                    action = Some(*key);
                }
                return false;
            }
            !claimed
        });
        self.explorer.key_intent = action.map(|key| KeyIntent {
            key,
            generation: self.generation,
            mode_epoch: self.explorer.mode_epoch,
            focused: focused.expect("a row action requires focus"),
            selected: self.explorer.selected.clone(),
            epoch: self.explorer.epoch,
        });
    }

    pub(crate) fn explorer_tree_shortcuts(&mut self, ctx: &egui::Context) {
        let Some(intent) = self.explorer.key_intent.take() else {
            return;
        };
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

    pub(crate) fn explorer_tree_row(
        &mut self,
        ui: &mut egui::Ui,
        row: &Row,
        galley: std::sync::Arc<egui::Galley>,
    ) -> egui::Response {
        let selected = self.explorer.selected == row.entry.path;
        let padding = ui.spacing().button_padding;
        let height = (galley.size().y + 2.0 * padding.y)
            .max(27.0)
            .max(ui.spacing().interact_size.y);
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
        let visuals = ui.style().interact(&response);
        if selected || response.hovered() || response.has_focus() {
            ui.painter().rect_filled(
                rect,
                visuals.corner_radius,
                if selected {
                    ui.visuals().selection.bg_fill
                } else {
                    visuals.bg_fill
                },
            );
        }
        if response.has_focus() {
            ui.painter().rect_stroke(
                rect,
                visuals.corner_radius,
                egui::Stroke::new(1.0_f32, GREEN),
                egui::StrokeKind::Inside,
            );
        }
        let indent =
            (row.depth as f32 * 10.0).min((rect.width() - 2.0 * padding.x).max(0.0) * 0.35);
        ui.painter().galley(
            rect.min + egui::vec2(padding.x + indent, padding.y),
            galley,
            TEXT,
        );
        response.on_hover_text(if row.entry.path.is_empty() {
            "Workspace root"
        } else {
            &row.entry.path
        })
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
        let scope = self.explorer_scope();
        self.new_file = true;
        self.new_path = if scope.is_empty() {
            String::new()
        } else {
            format!("{scope}/")
        };
    }

    pub(crate) fn explorer_expand(&mut self, path: &str) {
        if self.explorer.branches.contains_key(path) {
            return;
        }
        self.explorer_refresh(path);
    }

    pub(crate) fn explorer_refresh(&mut self, path: &str) {
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
        if ticket.mode == Mode::Tree {
            if !self.explorer.advance_epoch() {
                return;
            }
            let result = result.and_then(|entries| self.explorer.admit(&path, entries));
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
    }

    pub(crate) fn explorer_status(&mut self, ui: &mut egui::Ui, resized: bool) {
        if self.explorer_busy() {
            ui.label(RichText::new("Loading one directory…").small().color(MUTED));
        }
        if let Some(message) = &self.explorer.message {
            ui.label(RichText::new(message).small().color(RED));
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
