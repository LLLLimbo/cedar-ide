//! Bounded, explicitly submitted Project Search snapshots and native row input.
//! A row is historical evidence from one workspace/session/request/index. Its
//! line is an ordinary Open hint, never a claim about current disk or draft text.
use crate::{CedarApp, Tool, WorkspaceKey, AMBER, GREEN, MUTED};
use cedar_protocol::{Payload, ScopedSearchIncompleteReason, ScopedSearchRequest, SearchMatch};
use eframe::egui::{self, RichText};
use std::collections::HashSet;

pub(super) const MAX_ROWS: usize = 500;
pub(super) const MAX_TEXT_BYTES: usize = 512 * 1024;

pub(super) const SCOPED_UNAVAILABLE: &str = "Directory and file masks require scoped search support from this agent. Clear both filters for a workspace search, or upgrade the agent.";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Specification {
    pub request: ScopedSearchRequest,
    // None deliberately preserves the legacy operation for root/no-mask search.
    pub version: Option<u32>,
}
impl Specification {
    pub(super) fn legacy(query: String) -> Self {
        Self {
            request: ScopedSearchRequest {
                query,
                scope: String::new(),
                masks: Vec::new(),
                limit: MAX_ROWS,
            },
            version: None,
        }
    }
    pub(super) fn description(&self) -> String {
        let directory = if self.request.scope.is_empty() {
            "workspace root".into()
        } else {
            format!("directory {:?}", self.request.scope)
        };
        let masks = if self.request.masks.is_empty() {
            "all files".into()
        } else {
            format!("masks {}", self.request.masks.join(";"))
        };
        format!(
            "{:?} · {directory} · {masks} · limit {}",
            self.request.query, self.request.limit
        )
    }
}

#[derive(Clone, Debug)]
pub(super) struct ScanStatus {
    pub reasons: Vec<ScopedSearchIncompleteReason>,
    pub examined_entries: usize,
    pub read_bytes: usize,
}

pub(super) fn parse_masks(raw: &str) -> Result<Vec<String>, String> {
    if raw.is_empty() {
        return Ok(Vec::new());
    }
    let mut masks = Vec::new();
    let mut bytes = 0usize;
    for token in raw.split(';') {
        let mask = token.trim();
        if mask.is_empty() {
            return Err("File masks must be nonempty patterns separated by semicolons (for example *.rs;*.toml).".into());
        }
        bytes = bytes.saturating_add(mask.len());
        if masks.len() == 8 || bytes > 256 {
            return Err("Use at most 8 file masks and 256 UTF-8 bytes in total.".into());
        }
        masks.push(mask.to_owned());
    }
    Ok(masks)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Batch {
    workspace: egui::Id,
    generation: u64,
    request: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Source {
    pub workspace: WorkspaceKey,
    pub root: String,
    pub generation: u64,
    pub request: u64,
    pub query: String,
    pub specification: Specification,
}

impl Source {
    fn batch(&self) -> Batch {
        Batch {
            workspace: egui::Id::new((&self.workspace, &self.root)),
            generation: self.generation,
            request: self.request,
        }
    }
}

#[derive(Default)]
pub(super) struct Search {
    pub accepted: Option<Source>,
    pub pending: Option<Source>,
    completed: Option<(u64, Specification, Result<Payload, String>)>,
    pub status: Option<ScanStatus>,
    submission: Option<(u64, Option<(u64, u64)>)>,
    keyboard: Keyboard,
    // Only indexes are grouped; snippets live once in CedarApp::search_results.
    groups: Vec<Vec<usize>>,
    collapsed: HashSet<usize>,
    selected: Option<usize>,
    active: bool,
    layout: u64,
}

#[derive(Clone, PartialEq, Eq)]
pub(super) struct FocusIdentity {
    accepted: Option<Source>,
    pending: Option<Source>,
    layout: u64,
    selected: Option<usize>,
    active: bool,
    request: u64,
    query: String,
    scope: String,
    masks: String,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PointerProbe {
    pub pressed: Option<(usize, egui::Id)>,
    pub click: Option<(usize, egui::Id)>,
}

impl Search {
    #[cfg(test)]
    pub(super) fn claims(&self, key: egui::Key) -> bool {
        self.keyboard.claimed.contains(&key)
    }

    #[cfg(test)]
    pub(super) fn pointer_probe(&self) -> PointerProbe {
        let row = |pointer: &Pointer| (pointer.source.index, pointer.source.id);
        PointerProbe {
            pressed: self.keyboard.pressed.as_ref().map(row),
            click: self.keyboard.click.as_ref().map(row),
        }
    }

    pub(super) fn selected_index(&self) -> Option<usize> {
        self.selected
    }
    #[cfg(test)]
    pub(super) fn accepted_snapshot_id(&self) -> Option<egui::Id> {
        self.accepted
            .as_ref()
            .map(|source| egui::Id::new(source.batch()))
    }
    pub(super) fn group_rows(&self) -> Vec<Vec<usize>> {
        self.groups.clone()
    }
    pub(super) fn group_expanded(&self, first: usize) -> bool {
        !self.collapsed.contains(&first)
    }
    pub(super) fn visible_indices(&self) -> Vec<usize> {
        self.groups
            .iter()
            .filter(|group| self.group_expanded(group[0]))
            .flatten()
            .copied()
            .collect()
    }
    pub(super) fn visible(&self, index: usize) -> bool {
        self.groups
            .iter()
            .any(|group| self.group_expanded(group[0]) && group.contains(&index))
    }
    pub(super) fn destination(&self, current: usize, key: egui::Key) -> Option<usize> {
        let visible = self.visible_indices();
        let position = visible.iter().position(|index| *index == current)?;
        destination(visible.len(), position, key).map(|position| visible[position])
    }
    pub(super) fn deactivate(&mut self) {
        self.active = false;
        self.retire();
    }
    fn install(&mut self, rows: &[SearchMatch]) {
        self.groups.clear();
        for (index, row) in rows.iter().enumerate() {
            if let Some(group) = self
                .groups
                .iter_mut()
                .find(|group| rows[group[0]].path == row.path)
            {
                group.push(index);
            } else {
                self.groups.push(vec![index]);
            }
        }
        self.collapsed.clear();
        self.selected = (!rows.is_empty()).then_some(0);
        self.active = true;
        self.layout = 0;
    }
    pub(super) fn toggle_group(&mut self, first: usize) {
        if !self.groups.iter().any(|group| group[0] == first) {
            return;
        }
        if !self.collapsed.remove(&first) {
            self.collapsed.insert(first);
        }
        if let Some(layout) = self.layout.checked_add(1) {
            self.layout = layout;
        } else {
            self.active = false;
        }
        self.invalidate();
    }

    pub(super) fn clear_owned(&mut self) {
        self.accepted = None;
        self.status = None;
        self.groups.clear();
        self.collapsed.clear();
        self.selected = None;
        self.active = false;
        self.retire();
    }

    pub(super) fn retire(&mut self) {
        self.pending = None;
        self.submission = None;
        self.completed = None;
        self.keyboard.invalidate();
    }

    pub(super) fn invalidate(&mut self) {
        self.keyboard.invalidate();
    }
}

fn scoped_reason(reason: ScopedSearchIncompleteReason) -> &'static str {
    match reason {
        ScopedSearchIncompleteReason::PermissionDenied => "permission denied",
        ScopedSearchIncompleteReason::IoError => "I/O error",
        ScopedSearchIncompleteReason::DepthLimit => "depth limit",
        ScopedSearchIncompleteReason::TimeLimit => "time limit",
        ScopedSearchIncompleteReason::EntryLimit => "entry limit",
        ScopedSearchIncompleteReason::StackLimit => "pending directory limit",
        ScopedSearchIncompleteReason::ResponseLimit => "response size limit",
        ScopedSearchIncompleteReason::ResultLimit => "result limit",
        ScopedSearchIncompleteReason::UnrepresentablePath => "unrepresentable path",
        ScopedSearchIncompleteReason::UnsafePath => "unsafe path",
        ScopedSearchIncompleteReason::ReadByteLimit => "read byte limit",
    }
}

pub(super) fn row_id(source: &Source, index: usize) -> egui::Id {
    batch_row_id(source.batch(), index)
}

fn batch_row_id(batch: Batch, index: usize) -> egui::Id {
    egui::Id::new(("project_search_row", batch, index))
}

pub(super) fn validate(matches: &[SearchMatch]) -> Result<(), &'static str> {
    if matches.len() > MAX_ROWS {
        return Err("Search response exceeds the 500-row display limit; no results were accepted");
    }
    let bytes = matches.iter().try_fold(0usize, |total, row| {
        total
            .checked_add(row.path.len())?
            .checked_add(row.text.len())
    });
    if bytes.is_none_or(|bytes| bytes > MAX_TEXT_BYTES) {
        return Err(
            "Search response exceeds the 512 KiB path and snippet limit; no results were accepted",
        );
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct View {
    batch: Batch,
    count: usize,
    layout: u64,
    navigation: u64,
    document: Option<(u64, u64)>,
    screen: [u32; 4],
}

impl View {
    fn of(app: &CedarApp) -> Option<Self> {
        (app.search_rows_active() && app.active().is_none_or(|doc| doc.edit_version != u64::MAX))
            .then(|| Self {
                batch: app.search_state.accepted.as_ref().unwrap().batch(),
                count: app.search_results.len(),
                layout: app.search_state.layout,
                navigation: app.navigation_epoch,
                document: app.active().map(|doc| (doc.id, doc.edit_version)),
                screen: {
                    let rect = app.editor_ctx.screen_rect();
                    [
                        rect.min.x.to_bits(),
                        rect.min.y.to_bits(),
                        rect.max.x.to_bits(),
                        rect.max.y.to_bits(),
                    ]
                },
            })
    }
}

#[derive(Clone)]
struct Presented {
    index: usize,
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

// egui's click count is global. Only a prior accepted completion on this
// exact bottom-Find row and pane may supply identity for its double timing.
#[derive(Clone, PartialEq, Eq)]
struct RowClick {
    view: View,
    hit: BrowseHit,
    id: egui::Id,
    pane: Option<crate::browse::PreviewTicket>,
}

#[derive(Default)]
struct Keyboard {
    view: Option<View>,
    presented: Vec<Presented>,
    held: HashSet<egui::Key>,
    claimed: HashSet<egui::Key>,
    composing: bool,
    input_pass: Option<u64>,
    key: Option<Intent>,
    pressed: Option<Pointer>,
    click: Option<Pointer>,
    completed_row: Option<RowClick>,
    prior_row: Option<RowClick>,
    pressed_row_eligible: bool,
    row_click_eligible: bool,
    row_click_committed: bool,
    render: Option<(View, usize, egui::Id)>,
    preview_reveal: Option<(View, usize, u64)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Focus(usize),
    Open(usize),
    Preview(usize),
    Toggle(usize),
    Reveal(usize),
}

pub(super) fn destination(count: usize, current: usize, key: egui::Key) -> Option<usize> {
    if current >= count {
        return None;
    }
    Some(match key {
        egui::Key::ArrowUp => current.saturating_sub(1),
        egui::Key::ArrowDown => (current + 1).min(count - 1),
        egui::Key::Home => 0,
        egui::Key::End => count - 1,
        _ => return None,
    })
}

fn row_key(key: egui::Key) -> bool {
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

// Attribute only one exact Space text companion to a single plain press. Any
// other text, paste, IME, pointer, modifier, key or repeated edge vetoes input.
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
            } if modifiers.is_none() && row_key(*candidate) && key.is_none() => {
                key = Some(*candidate)
            }
            egui::Event::Key {
                key: candidate,
                pressed: false,
                modifiers,
                ..
            } if modifiers.is_none() && key == Some(*candidate) && !released => released = true,
            egui::Event::Text(text) if text == " " && spaces == 0 => spaces += 1,
            _ => return None,
        }
    }
    key.filter(|key| spaces == 0 || *key == egui::Key::Space)
}

impl Keyboard {
    fn break_row_continuity(&mut self) {
        self.completed_row = None;
        self.prior_row = None;
    }

    fn invalidate(&mut self) {
        self.view = None;
        self.presented.clear();
        self.key = None;
        self.pressed = None;
        self.click = None;
        self.break_row_continuity();
        self.pressed_row_eligible = false;
        self.render = None;
        self.preview_reveal = None;
        // Lifetime ownership ends at physical release, not a focus/view change.
    }

    fn record(&mut self, ui: &egui::Ui, index: usize, response: &egui::Response) {
        self.presented.push(Presented {
            index,
            id: response.id,
            rect: response.rect,
            clip: ui.clip_rect(),
        });
    }
}

impl CedarApp {
    fn search_row_click(&self, index: usize, id: egui::Id) -> Option<RowClick> {
        let row = self.search_results.get(index)?;
        Some(RowClick {
            view: View::of(self)?,
            hit: BrowseHit {
                source: std::sync::Arc::new(self.search_state.accepted.clone()?),
                index,
                path: row.path.clone(),
                line: row.line,
                text_identity: egui::Id::new(&row.text),
            },
            id,
            pane: self.find_preview_identity(),
        })
    }

    pub(super) fn search_specification(&self) -> Result<Specification, String> {
        let masks = parse_masks(&self.search_masks)?;
        if self.search_scope.is_empty() && masks.is_empty() {
            return Ok(Specification::legacy(self.search_query.clone()));
        }
        let request = ScopedSearchRequest {
            query: self.search_query.clone(),
            scope: self.search_scope.clone(),
            masks,
            limit: MAX_ROWS,
        };
        request.validate().map_err(|error| error.to_string())?;
        if !self.backend_scoped_search_supported() {
            return Err(SCOPED_UNAVAILABLE.into());
        }
        Ok(Specification {
            request,
            version: self.scoped_search.map(|descriptor| descriptor.version),
        })
    }

    pub(super) fn search_filters_changed(&mut self) {
        self.search_state.deactivate();
        self.retire_project_text_preview();
    }

    pub(super) fn search_submission_supported(&self) -> bool {
        if self.search_scope.is_empty() && self.search_masks.is_empty() {
            self.backend_supports("search")
        } else {
            self.backend_scoped_search_supported()
        }
    }

    // Both text-search surfaces render these exact drafts. Width is measured in
    // the current body font so larger text naturally switches to two rows.
    pub(super) fn search_filter_controls(
        &mut self,
        ui: &mut egui::Ui,
        scope_id: egui::Id,
        masks_id: egui::Id,
    ) -> (egui::Response, egui::Response) {
        let font = egui::TextStyle::Body.resolve(ui.style());
        let required = ui.fonts(|fonts| {
            fonts
                .layout_no_wrap(
                    "Directory  workspace/root    File masks  *.rs;*.toml".into(),
                    font,
                    MUTED,
                )
                .size()
                .x
        }) + ui.spacing().item_spacing.x * 6.0
            + ui.spacing().button_padding.x * 4.0;
        let field = |ui: &mut egui::Ui, label: &str, value: &mut String, id, hint: &str| {
            ui.horizontal(|ui| {
                ui.label(label);
                ui.add(
                    egui::TextEdit::singleline(value)
                        .id(id)
                        .desired_width(ui.available_width())
                        .hint_text(hint),
                )
            })
            .inner
        };
        let responses = if ui.available_width() >= required {
            ui.columns(2, |columns| {
                let scope = field(
                    &mut columns[0],
                    "Directory",
                    &mut self.search_scope,
                    scope_id,
                    "Exact relative directory; blank = root",
                );
                let masks = field(
                    &mut columns[1],
                    "File masks",
                    &mut self.search_masks,
                    masks_id,
                    "*.rs;*.toml",
                );
                (scope, masks)
            })
        } else {
            let scope = field(
                ui,
                "Directory",
                &mut self.search_scope,
                scope_id,
                "Exact relative directory; blank = root",
            );
            let masks = field(
                ui,
                "File masks",
                &mut self.search_masks,
                masks_id,
                "*.rs;*.toml",
            );
            (scope, masks)
        };
        ui.label(
            RichText::new(format!(
                "Excluded directories: {}",
                cedar_protocol::FILE_DISCOVERY_EXCLUDED_DIRECTORIES.join(", ")
            ))
            .small()
            .color(MUTED),
        );
        (
            responses.0.on_hover_text("Exact workspace-relative directory. Blank searches the workspace root. The path is not trimmed or normalized."),
            responses.1.on_hover_text("Whole filename masks, case-sensitive. Separate alternatives with semicolons. * matches zero or more characters; ? matches one Unicode character. Blank includes all filenames."),
        )
    }

    pub(super) fn search_filter_explanation(&self) -> Option<String> {
        if self.search_scope.is_empty() && self.search_masks.is_empty() {
            return None;
        }
        if !self.backend_scoped_search_supported() {
            return Some(SCOPED_UNAVAILABLE.into());
        }
        // A placeholder query validates filters before a query has been entered.
        let masks = match parse_masks(&self.search_masks) {
            Ok(masks) => masks,
            Err(error) => return Some(error),
        };
        ScopedSearchRequest {
            query: "x".into(),
            scope: self.search_scope.clone(),
            masks,
            limit: MAX_ROWS,
        }
        .validate()
        .err()
        .map(|error| error.to_string())
    }

    pub(super) fn search_scan_description(&self) -> Option<String> {
        self.search_state.status.as_ref().map(|status| {
            let counts = format!(
                "{} entries examined · {} bytes read",
                status.examined_entries, status.read_bytes
            );
            if status.reasons.is_empty() {
                counts
            } else {
                format!(
                    "{counts} · Incomplete: {}",
                    status
                        .reasons
                        .iter()
                        .map(|reason| scoped_reason(*reason))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        })
    }

    pub(super) fn search_rows_active(&self) -> bool {
        self.ready()
            && self.search_state.active
            && self.search_state.pending.is_none()
            && self.search_state.accepted.as_ref().is_some_and(|source| {
                self.workspace_key.as_ref() == Some(&source.workspace)
                    && self.root == source.root
                    && self.generation == source.generation
                    && self.search_request == source.request
                    && self.search_query == source.query
                    && self.search_specification().as_ref() == Ok(&source.specification)
            })
    }

    pub(super) fn search_focus_identity(&self) -> FocusIdentity {
        FocusIdentity {
            accepted: self.search_state.accepted.clone(),
            pending: self.search_state.pending.clone(),
            layout: self.search_state.layout,
            selected: self.search_state.selected,
            active: self.search_rows_active(),
            request: self.search_request,
            query: self.search_query.clone(),
            scope: self.search_scope.clone(),
            masks: self.search_masks.clone(),
        }
    }

    fn search_input_allowed(&self, ctx: &egui::Context) -> bool {
        self.search_rows_active()
            && self.tools_open
            && self.tool == Tool::Search
            && !self.open_form
            && !self.new_file
            && !self.navigation.blocks_editor()
            && !self.navigation.restore_focus
            && !self.foreign_modal_owns_input(ctx)
            && !self.find_focus
            && self.close_tab_requested.is_none()
            && !self.close_after_language_stop
            && !self.allow_close
            && !self
                .active()
                .is_some_and(|doc| doc.jump_to.is_some() || doc.edit_version == u64::MAX)
    }

    pub(super) fn begin_search_submission(&mut self) {
        self.search_state.submission = self.search_state.pending.as_ref().map(|_| {
            (
                self.navigation_epoch,
                self.active().map(|doc| (doc.id, doc.edit_version)),
            )
        });
    }
    pub(super) fn stage_search_response(
        &mut self,
        request: u64,
        specification: Specification,
        result: Result<Payload, String>,
    ) {
        self.search_state.completed = Some((request, specification, result));
    }
    pub(super) fn finish_search_frame(&mut self, ctx: &egui::Context) {
        let visible = self.tools_open
            && self.tool == Tool::Search
            && !self.open_form
            && !self.new_file
            && !self.navigation.blocks_editor()
            && !self.foreign_modal_owns_input(ctx);
        let bottom_pending = self
            .search_state
            .pending
            .as_ref()
            .is_some_and(|source| !self.browse_owns_search(source.request));
        let submission = (
            self.navigation_epoch,
            self.active().map(|doc| (doc.id, doc.edit_version)),
        );
        if bottom_pending
            && (!visible
                || self.search_state.submission != Some(submission)
                || submission.1.is_some_and(|(_, version)| version == u64::MAX))
        {
            // Retire intent, never the physical Job::Search slot. Reopening is not a rerun.
            self.search_state.retire();
        }
        if let Some((request, specification, result)) = self.search_state.completed.take() {
            if visible {
                self.apply_search_response(request, specification, result);
                ctx.request_repaint();
            } else if self
                .search_state
                .pending
                .as_ref()
                .is_some_and(|source| source.request == request)
            {
                self.search_state.retire();
            }
        }
        if !visible {
            self.search_state.invalidate();
            self.retire_search_preview();
        }
    }

    pub(super) fn apply_search_response(
        &mut self,
        request: u64,
        specification: Specification,
        result: Result<Payload, String>,
    ) {
        let current = self.search_state.pending.as_ref().is_some_and(|source| {
            source.request == request
                && self.search_request == request
                && source.specification == specification
                && self.search_specification().as_ref() == Ok(&specification)
                && source.generation == self.generation
                && self.workspace_key.as_ref() == Some(&source.workspace)
                && self.root == source.root
        });
        if !current {
            return;
        }
        let source = self.search_state.pending.take().unwrap();
        self.search_state.submission = None;
        self.search_state.invalidate();
        let (matches, truncated, status) = match result {
            Ok(Payload::Matches { matches, truncated }) if specification.version.is_none() => {
                (matches, truncated, None)
            }
            Ok(Payload::ScopedMatches { result }) if specification.version.is_some() => {
                if let Err(error) = result.validate_for(&specification.request) {
                    self.error = Some(error.to_string());
                    return;
                }
                let status = ScanStatus {
                    reasons: result.reasons,
                    examined_entries: result.examined_entries,
                    read_bytes: result.read_bytes,
                };
                (result.matches, result.incomplete, Some(status))
            }
            Ok(_) => {
                self.error = Some("Unexpected search response; no results were accepted".into());
                return;
            }
            Err(error) => {
                self.error = Some(error);
                return;
            }
        };
        if let Err(error) = validate(&matches) {
            self.error = Some(error.into());
            return;
        }
        self.cjk_seen |= matches.iter().any(|row| {
            crate::system_fonts::contains_cjk(&row.path)
                || crate::system_fonts::contains_cjk(&row.text)
        });
        self.notice = format!(
            "{} matches for {}",
            matches.len(),
            source.specification.description()
        );
        self.search_state.status = status;
        self.search_state.install(&matches);
        self.search_results = matches;
        self.search_truncated = truncated;
        self.search_state.accepted = Some(source);
    }

    // Every surface sees the immutable original sequence. The raw hook applies
    // the union of per-event removals only after all lifetimes were observed.
    pub(super) fn capture_search_input(
        &mut self,
        ctx: &egui::Context,
        input: &egui::RawInput,
        consumed: &mut [bool],
    ) {
        let allowed = self.search_input_allowed(ctx);
        let current = View::of(self);
        let focused = ctx.memory(|memory| memory.focused());
        let prior_row = self
            .search_state
            .keyboard
            .completed_row
            .clone()
            .filter(|prior| {
                allowed
                    && (focused == Some(prior.id)
                        // egui may surrender keyboard focus during an owned
                        // press. Preserve only that exact clean held gesture;
                        // another focused control still ends continuity.
                        || (focused.is_none()
                            && self.search_state.keyboard.pressed_row_eligible
                            && self.search_state.keyboard.pressed.as_ref().is_some_and(|press| {
                                press.view == prior.view
                                    && press.source.index == prior.hit.index
                                    && press.source.id == prior.id
                            })
                            && ctx.input(|input| input.pointer.primary_down())
                            && ctx.read_response(prior.id).is_some_and(|response| response.is_pointer_button_down_on())))
                    && Some(prior) == self.search_row_click(prior.hit.index, prior.id).as_ref()
                    && self.search_state.visible(prior.hit.index)
            });
        let keyboard = &mut self.search_state.keyboard;
        keyboard.completed_row = prior_row.clone();
        // Freeze evidence before this original batch. A repeated render pass
        // must never use the completion that this batch is about to produce.
        keyboard.prior_row = prior_row;
        keyboard.row_click_eligible = true;
        keyboard.row_click_committed = false;
        // This continuation may scroll only after the user's existing Space.
        // Inspect the original native batch, before any surface removes events.
        if !input.modifiers.is_none() || input.events.iter().any(|event| !matches!(event,
            egui::Event::Key { key: egui::Key::Space, pressed: false, modifiers, .. } if modifiers.is_none()
        ) && !matches!(event, egui::Event::PointerMoved(_) | egui::Event::MouseMoved(_))) {
            keyboard.preview_reveal = None;
        }
        keyboard.input_pass = Some(ctx.cumulative_pass_nr());
        keyboard.render = None;
        keyboard.key = None;
        keyboard.click = None;
        if !input.focused
            || input
                .events
                .iter()
                .any(|event| matches!(event, egui::Event::WindowFocused(false)))
        {
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
        let screen = input.screen_rect.unwrap_or_else(|| ctx.screen_rect());
        let activation_visible = source
            .as_ref()
            .is_some_and(|row| row.rect.intersect(row.clip).intersect(screen).is_positive());
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
        // Raw relative motion is an untouched companion. Only the absolute
        // primary-button positions below can create or resolve row ownership;
        // the current-frame egui response must still own the actual click.
        let pointer_only = input.events.iter().all(|event| {
            matches!(
                event,
                egui::Event::PointerMoved(_)
                    | egui::Event::MouseMoved(_)
                    | egui::Event::PointerButton {
                        button: egui::PointerButton::Primary,
                        ..
                    }
                    | egui::Event::Ime(egui::ImeEvent::Enabled | egui::ImeEvent::Disabled)
            )
        });
        if !current_presentation
            || !pointer_only
            || composition_was_active
            || keyboard.composing
            || !keyboard.held.is_empty()
            || !input.modifiers.is_none()
            || ctx.input(|input| !input.keys_down.is_empty())
        {
            keyboard.pressed = None;
            keyboard.pressed_row_eligible = false;
            keyboard.break_row_continuity();
            keyboard.row_click_eligible = false;
        } else {
            let edges: Vec<_> = input
                .events
                .iter()
                .filter_map(|event| match event {
                    egui::Event::PointerButton { pressed, .. } => Some(*pressed),
                    _ => None,
                })
                .collect();
            // Keep clean carried release→press pairs. Duplicate presses and
            // multiple completions cannot establish one clean row gesture.
            if edges.len() > 2
                || (edges.len() == 2 && edges[0] == edges[1])
                || (edges.first() == Some(&true) && keyboard.pressed.is_some())
                || (edges.first() == Some(&false) && keyboard.pressed.is_none())
            {
                keyboard.break_row_continuity();
                keyboard.row_click_eligible = false;
            }
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
                        keyboard.pressed_row_eligible = false;
                        keyboard.break_row_continuity();
                        keyboard.row_click_eligible = false;
                        continue;
                    }
                    if *pressed {
                        // Poison belongs to the held gesture, not only this
                        // frame: a later clean release cannot repair its press.
                        keyboard.pressed_row_eligible = keyboard.row_click_eligible;
                        keyboard.pressed = keyboard
                            .presented
                            .iter()
                            .find(|row| row.rect.intersect(row.clip).contains(*pos))
                            .cloned()
                            .map(|source| Pointer {
                                view: current.unwrap(),
                                source,
                            });
                        if !keyboard.pressed.as_ref().is_some_and(|press| {
                            press.source.index < MAX_ROWS
                                && keyboard.prior_row.as_ref().is_some_and(|prior| {
                                    prior.view == press.view
                                        && prior.hit.index == press.source.index
                                        && prior.id == press.source.id
                                })
                        }) {
                            keyboard.break_row_continuity();
                        }
                    } else if let Some(press) = keyboard.pressed.take() {
                        keyboard.row_click_eligible &= keyboard.pressed_row_eligible;
                        keyboard.pressed_row_eligible = false;
                        keyboard.completed_row = None;
                        if Some(press.view) == current
                            && press
                                .source
                                .rect
                                .intersect(press.source.clip)
                                .contains(*pos)
                            && keyboard.presented.iter().any(|row| {
                                row.id == press.source.id
                                    && row.index == press.source.index
                                    && row.rect.intersect(row.clip).contains(*pos)
                            })
                        {
                            keyboard.click = Some(press);
                        } else {
                            keyboard.break_row_continuity();
                            keyboard.row_click_eligible = false;
                        }
                    } else {
                        keyboard.break_row_continuity();
                        keyboard.row_click_eligible = false;
                    }
                }
            }
        }
        let mut accepted = None;
        let mut claimed_space_press = false;
        for (index, event) in input.events.iter().enumerate() {
            let egui::Event::Key {
                key,
                pressed,
                repeat,
                modifiers,
                ..
            } = event
            else {
                continue;
            };
            if !row_key(*key) {
                continue;
            }
            let claimed = keyboard.claimed.contains(key);
            if !pressed {
                keyboard.held.remove(key);
                keyboard.claimed.remove(key);
                consumed[index] |= claimed;
                continue;
            }
            let fresh = keyboard.held.insert(*key);
            if source.is_some() && modifiers.is_none() {
                // A foreign-held key remains the original consumer's lifetime.
                if fresh || claimed {
                    keyboard.claimed.insert(*key);
                }
                claimed_space_press |= *key == egui::Key::Space;
                if fresh
                    && !repeat
                    && candidate == Some(*key)
                    && (!matches!(key, egui::Key::Enter | egui::Key::Space) || activation_visible)
                {
                    accepted = Some(*key);
                }
                consumed[index] = true;
            } else {
                claimed_space_press |= claimed && *key == egui::Key::Space;
                consumed[index] |= claimed;
            }
        }
        if claimed_space_press && companion == Some(egui::Key::Space) {
            for (index, event) in input.events.iter().enumerate() {
                consumed[index] |= matches!(event, egui::Event::Text(text) if text == " ");
            }
        }
        if let (Some(key), Some(source), Some(view)) = (accepted, source, current) {
            keyboard.key = Some(Intent { view, source, key });
        }
    }

    fn search_input_frame_current(&self, ctx: &egui::Context) -> bool {
        self.search_input_allowed(ctx)
            && self.navigation.input_frame_current(ctx)
            && self.search_state.keyboard.input_pass
                == Some(
                    ctx.cumulative_pass_nr()
                        .saturating_sub(ctx.output(|output| output.num_completed_passes) as u64),
                )
            && ctx.input(|input| input.focused)
    }

    fn take_search_click(&mut self, ctx: &egui::Context) -> Option<usize> {
        let click = self.search_state.keyboard.click.take()?;
        (self.search_input_frame_current(ctx) && Some(click.view) == View::of(self))
            .then_some(click.source.index)
    }

    fn take_search_action(&mut self, ctx: &egui::Context) -> Option<Action> {
        let key = self.search_state.keyboard.key.take();
        let current = View::of(self);
        if !self.search_input_frame_current(ctx) {
            self.search_state.invalidate();
            return None;
        }
        if key.is_none() && ctx.output(|output| output.num_completed_passes) > 0 {
            return self
                .search_state
                .keyboard
                .render
                .as_ref()
                .and_then(|(view, index, focus)| {
                    (current.as_ref() == Some(view)
                        && ctx.memory(|memory| memory.focused()) == Some(*focus))
                    .then_some(Action::Focus(*index))
                });
        }
        let intent = key?;
        if current != Some(intent.view)
            || ctx.memory(|memory| memory.focused()) != Some(intent.source.id)
        {
            return None;
        }
        let index = intent.source.index;
        if index >= MAX_ROWS {
            return matches!(intent.key, egui::Key::Enter | egui::Key::Space)
                .then_some(Action::Toggle(index - MAX_ROWS));
        }
        if index >= intent.view.count || !self.search_state.visible(index) {
            return None;
        }
        match intent.key {
            egui::Key::Enter => Some(Action::Open(index)),
            egui::Key::Space => Some(Action::Preview(index)),
            key => self.search_state.destination(index, key).map(Action::Focus),
        }
    }

    pub(super) fn select_search_row(&mut self, index: usize) {
        if !self.search_rows_active() || !self.search_state.visible(index) {
            return;
        }
        if self.search_state.selected != Some(index) {
            self.retire_search_preview();
        }
        self.search_state.selected = Some(index);
    }

    fn search_rendered_row_visible(&self, index: usize, ctx: &egui::Context) -> bool {
        self.search_state.keyboard.presented.iter().any(|row| {
            row.index == index
                && row
                    .rect
                    .intersect(row.clip)
                    .intersect(ctx.screen_rect())
                    .is_positive()
        })
    }

    pub(super) fn search_panel(&mut self, ui: &mut egui::Ui) {
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.spacing_mut().button_padding = egui::vec2(6.0, 3.0);
            self.find_results_panel(ui);
        });
    }

    fn find_results_panel(&mut self, ui: &mut egui::Ui) {
        if self.ready()
            && !self.backend_supports("search")
            && !self.backend_scoped_search_supported()
        {
            ui.colored_label(AMBER, self.unsupported_message("search"));
        }
        let mut submit_requested = false;
        ui.horizontal(|ui| {
            let button_font = egui::TextStyle::Button.resolve(ui.style());
            let button_width = ui.fonts(|fonts| {
                fonts
                    .layout_no_wrap("Search".into(), button_font, MUTED)
                    .size()
                    .x
            }) + ui.spacing().button_padding.x * 2.0;
            let edit = ui.add(
                egui::TextEdit::singleline(&mut self.search_query)
                    .hint_text("Case-sensitive literal text")
                    .desired_width(
                        (ui.available_width() - button_width - ui.spacing().item_spacing.x)
                            .max(1.0),
                    ),
            );
            #[cfg(test)]
            crate::workspace_access_tests::record(ui, "search_query", &edit);
            self.record_find_access(ui, None, &edit);
            if edit.changed() {
                self.search_filters_changed();
            }
            let enter = edit.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            let enabled = self.search_submission_supported() && !self.search_busy();
            let submit = ui.add_enabled(enabled, egui::Button::new("Search"));
            #[cfg(test)]
            crate::workspace_access_tests::record(ui, "search_submit", &submit);
            submit_requested = enabled && (submit.clicked() || enter);
        });
        let scope_id = ui.id().with("search_scope");
        let masks_id = ui.id().with("search_masks");
        let (scope, masks) = self.search_filter_controls(ui, scope_id, masks_id);
        #[cfg(test)]
        {
            crate::workspace_access_tests::record(ui, "search_scope", &scope);
            crate::workspace_access_tests::record(ui, "search_masks", &masks);
        }
        if scope.changed() || masks.changed() {
            self.search_filters_changed();
            submit_requested = false;
        } else if (scope.lost_focus() || masks.lost_focus())
            && ui.input(|input| input.key_pressed(egui::Key::Enter))
        {
            submit_requested = self.search_submission_supported() && !self.search_busy();
        }
        if let Some(explanation) = self.search_filter_explanation() {
            ui.colored_label(AMBER, explanation);
        }
        if submit_requested {
            self.search();
        }
        if let Some(source) = &self.search_state.accepted {
            let history = format!("Historical results for {} · {} results. Lines may have changed; Open uses the current buffer.", source.specification.description(), self.search_results.len());
            let label = ui
                .add(egui::Label::new(RichText::new(&history).small().color(MUTED)).truncate())
                .on_hover_text(history);
            #[cfg(test)]
            crate::workspace_access_tests::record(ui, "search_history", &label);
            #[cfg(not(test))]
            let _ = label;
        }
        let active = self.search_rows_active();
        if self.search_state.accepted.is_some() && !active {
            let label = ui.colored_label(
                AMBER,
                "Previous results are inactive. Run Search for the current query, filters and connection.",
            );
            #[cfg(test)]
            crate::workspace_access_tests::record(ui, "search_inactive", &label);
            #[cfg(not(test))]
            let _ = label;
        }
        if self.search_busy() {
            ui.label(RichText::new("Searching...").small().color(MUTED));
        }
        if let Some(description) = self.search_scan_description() {
            ui.label(RichText::new(description).small().color(MUTED));
        }
        if self.search_truncated {
            let label = ui.colored_label(AMBER, "Search incomplete; more matches may exist.");
            #[cfg(test)]
            crate::workspace_access_tests::record(ui, "search_truncated", &label);
            #[cfg(not(test))]
            let _ = label;
        }
        ui.label(
            RichText::new("Click selects · Space previews · Enter or double-click opens")
                .small()
                .color(MUTED),
        );
        let mut action = if ui.is_sizing_pass() {
            None
        } else {
            self.take_search_action(ui.ctx())
        };
        if !ui.is_sizing_pass() {
            if let Some((view, index, pass)) = self.search_state.keyboard.preview_reveal {
                if self.search_state.keyboard.input_pass != Some(pass) {
                    self.search_state.keyboard.preview_reveal = None;
                    if action.is_none()
                        && View::of(self) == Some(view)
                        && self.search_preview_current()
                        && ui.memory(|memory| memory.focused())
                            == Some(batch_row_id(view.batch, index))
                    {
                        action = Some(Action::Reveal(index));
                    }
                }
            }
        }
        let click = if ui.is_sizing_pass() {
            None
        } else {
            self.take_search_click(ui.ctx())
        };
        let toggle = match action {
            Some(Action::Toggle(first)) => Some(first),
            _ => click
                .filter(|index| *index >= MAX_ROWS)
                .map(|index| index - MAX_ROWS),
        };
        // A structure change retires native presentation before another row may act.
        let mut toggle_after = None;
        let mut open = None;
        let mut preview = None;
        match action {
            Some(Action::Focus(index)) => self.select_search_row(index),
            Some(Action::Open(index)) => {
                self.select_search_row(index);
                open = Some(index);
            }
            Some(Action::Preview(index)) => {
                self.select_search_row(index);
                preview = Some(index);
            }
            _ => {}
        }
        self.search_state.keyboard.presented.clear();
        let wide = ui.available_width() >= 760.0;
        // A small Tools preference may leave only part of the logical Ui on
        // screen. Scroll within that actual clip so End cannot reveal a row
        // into an invisible extension of the viewport.
        let showing_preview = self.search_preview_current();
        if self.tools_height.is_none() {
            // Auto needs a bounded content request independent of its previous
            // clipped viewport. Explicit user heights remain entirely unchanged.
            let spacing = ui.spacing().item_spacing.y;
            let row_height = (ui.text_style_height(&egui::TextStyle::Button)
                + 2.0 * ui.spacing().button_padding.y)
                .max(ui.spacing().interact_size.y);
            let controls = (self.search_state.groups.len()
                + self.search_state.visible_indices().len())
            .clamp(1, 5) as f32;
            let results = controls * row_height + (controls - 1.0) * spacing;
            let preview = if showing_preview {
                ui.text_style_height(&egui::TextStyle::Small)
                    + spacing
                    + 2.0 * ui.text_style_height(&egui::TextStyle::Monospace)
            } else {
                0.0
            };
            let body_height = if wide {
                results.max(preview)
            } else {
                results
                    + if showing_preview {
                        spacing + preview
                    } else {
                        0.0
                    }
            };
            ui.expand_to_include_rect(egui::Rect::from_min_size(
                ui.available_rect_before_wrap().min,
                egui::vec2(0.0, body_height),
            ));
        }
        let available = visible_height(ui);
        let results_height = if wide || !showing_preview {
            available
        } else {
            available * 0.4
        };
        let scroll_source = self
            .search_state
            .accepted
            .as_ref()
            .map(|source| row_id(source, 0));
        let mut draw_results = |app: &mut Self, ui: &mut egui::Ui| {
            let scroll = egui::ScrollArea::vertical()
                .id_salt(("search_results", scroll_source))
                .animated(false)
                .min_scrolled_height(0.0)
                .max_height(results_height)
                .show(ui, |ui| {
                    let mut revealed = false;
                    for group in app.search_state.group_rows() {
                        let first = group[0];
                        let expanded = app.search_state.group_expanded(first);
                        let Some(result) = app.search_results.get(first) else {
                            continue;
                        };
                        let id = app.search_state.accepted.as_ref().map_or_else(
                            || egui::Id::new(("unowned_search_group", first)),
                            |source| egui::Id::new(("find_group", source.batch(), first)),
                        );
                        let label = format!(
                            "{} {}  ({})",
                            if expanded { "▾" } else { "▸" },
                            result.path,
                            group.len()
                        );
                        let response = ui
                            .add_enabled_ui(active, |ui| row_control(ui, id, &label, false))
                            .inner
                            .on_hover_text(&result.path)
                            .on_disabled_hover_text(&result.path);
                        app.search_state
                            .keyboard
                            .record(ui, MAX_ROWS + first, &response);
                        #[cfg(test)]
                        crate::workspace_access_tests::record(
                            ui,
                            &format!("search_group:{first}"),
                            &response,
                        );
                        #[cfg(test)]
                        crate::search_keyboard_tests::record_pointer_response(
                            ui, MAX_ROWS + first, &response, click,
                        );
                        if toggle == Some(first)
                            && (action == Some(Action::Toggle(first))
                                || response.clicked_by(egui::PointerButton::Primary))
                        {
                            toggle_after = Some((first, id));
                        }
                        if !expanded {
                            continue;
                        }
                        for index in group {
                            let result = &app.search_results[index];
                            let id = app.search_state.accepted.as_ref().map_or_else(
                                || egui::Id::new(("unowned_search_row", index)),
                                |source| row_id(source, index),
                            );
                            let label = format!("    {}:  {}", result.line, result.text.trim());
                            let response = ui
                                .add_enabled_ui(active, |ui| {
                                    row_control(
                                        ui,
                                        id,
                                        &label,
                                        app.search_state.selected == Some(index),
                                    )
                                })
                                .inner;
                            app.search_state.keyboard.record(ui, index, &response);
                            app.record_find_access(ui, Some(index), &response);
                            #[cfg(test)]
                            crate::workspace_access_tests::record(
                                ui,
                                &format!("search_row:{index}"),
                                &response,
                            );
                            #[cfg(test)]
                            crate::search_keyboard_tests::record_pointer_response(
                                ui, index, &response, click,
                            );
                            if click == Some(index)
                                && response.clicked_by(egui::PointerButton::Primary)
                            {
                                let same_row = app.search_state.keyboard.row_click_eligible
                                    && app.search_state.keyboard.prior_row.as_ref().is_some_and(|prior| {
                                        Some(prior) == app.search_row_click(index, response.id).as_ref()
                                    });
                                app.select_search_row(index);
                                response.request_focus();
                                if same_row && response.double_clicked_by(egui::PointerButton::Primary) {
                                    open = Some(index);
                                }
                                if !ui.is_sizing_pass()
                                    && response.enabled()
                                    && app.search_state.keyboard.row_click_eligible
                                    && !app.search_state.keyboard.row_click_committed
                                {
                                    app.search_state.keyboard.completed_row = app.search_row_click(index, response.id);
                                    app.search_state.keyboard.row_click_committed = true;
                                }
                            }
                            if !ui.is_sizing_pass() && matches!(action, Some(Action::Focus(target) | Action::Preview(target) | Action::Reveal(target)) if target == index) {
                                if action != Some(Action::Reveal(index)) { response.request_focus(); }
                                response.scroll_to_me(Some(egui::Align::Center));
                                revealed = true;
                            } else if !ui.is_sizing_pass()
                                && action.is_none()
                                && !revealed
                                && response.gained_focus()
                            {
                                response.scroll_to_me(None);
                                revealed = true;
                            }
                        }
                    }
                    if app.search_results.is_empty() {
                        ui.label(
                            RichText::new(
                                "Case-sensitive literal disk search. Submit a query to find text.",
                            )
                            .small()
                            .color(MUTED),
                        );
                    }
                });
            #[cfg(test)]
            crate::search_keyboard_tests::record_scroll(ui.ctx(), scroll.id);
            #[cfg(test)]
            crate::browse_tests::record_bottom_results_scroll(
                ui.ctx(),
                scroll.id,
                scroll.inner_rect,
                scroll.content_size,
            );
            #[cfg(not(test))]
            let _ = scroll;
        };
        if wide {
            ui.columns(2, |columns| {
                draw_results(self, &mut columns[0]);
                self.search_preview_panel(&mut columns[1], available);
            });
        } else {
            draw_results(self, ui);
            if showing_preview {
                self.search_preview_panel(ui, visible_height(ui));
            }
        }
        if let Some((first, id)) = toggle_after {
            self.search_state.toggle_group(first);
            self.retire_search_preview();
            ui.memory_mut(|memory| memory.request_focus(id));
            open = None;
            preview = None;
            ui.ctx().request_repaint();
        }
        let view = View::of(self);
        if let (Some(view), Some(Action::Focus(index))) = (view, action) {
            let focus = batch_row_id(view.batch, index);
            if ui.ctx().memory(|memory| memory.focused()) == Some(focus) {
                self.search_state.keyboard.render = Some((view, index, focus));
            }
        }
        self.search_state.keyboard.view = view;
        if self.search_input_allowed(ui.ctx()) {
            if let Some(index) =
                preview.filter(|index| self.search_rendered_row_visible(*index, ui.ctx()))
            {
                self.search_preview(index);
                self.search_state.keyboard.preview_reveal = View::of(self)
                    .zip(self.search_state.keyboard.input_pass)
                    .map(|(view, pass)| (view, index, pass));
                ui.ctx().request_repaint();
            }
            if let Some(index) =
                open.filter(|index| self.search_rendered_row_visible(*index, ui.ctx()))
            {
                if let Some(result) = self.search_results.get(index) {
                    self.open(result.path.clone(), Some(result.line));
                }
            }
        }
    }
}

pub(super) fn visible_height(ui: &egui::Ui) -> f32 {
    (ui.clip_rect().max.y.min(ui.ctx().screen_rect().max.y) - ui.available_rect_before_wrap().min.y)
        .max(0.0)
}

// Give the actual native interaction its batch/index ID, preserving Tab order
// and accessibility even for duplicate path/line/snippet triples.
fn row_control(ui: &mut egui::Ui, id: egui::Id, label: &str, selected: bool) -> egui::Response {
    let padding = ui.spacing().button_padding;
    let galley = egui::WidgetText::from(RichText::new(label).color(GREEN)).into_galley(
        ui,
        Some(egui::TextWrapMode::Truncate),
        (ui.available_width() - 2.0 * padding.x).max(0.0),
        egui::TextStyle::Button,
    );
    let (_, rect) = ui.allocate_space(egui::vec2(
        ui.available_width(),
        (galley.size().y + 2.0 * padding.y).max(ui.spacing().interact_size.y),
    ));
    let response = ui.interact(rect, id, egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    if response.has_focus() {
        ui.memory_mut(|memory| {
            memory.set_focus_lock_filter(
                id,
                egui::EventFilter {
                    vertical_arrows: true,
                    ..Default::default()
                },
            )
        });
    }
    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact_selectable(&response, selected);
        if selected || response.hovered() || response.has_focus() {
            ui.painter().rect(
                rect,
                visuals.corner_radius,
                visuals.weak_bg_fill,
                visuals.bg_stroke,
                egui::StrokeKind::Inside,
            );
        }
        ui.painter().galley(rect.min + padding, galley, GREEN);
    }
    response
}

// Immutable adapter over the one accepted historical payload. Both Find
// surfaces share query and selection; adapters never duplicate snippets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct BrowseHit {
    source: std::sync::Arc<Source>,
    index: usize,
    path: String,
    line: usize,
    text_identity: egui::Id,
}
impl BrowseHit {
    pub(super) fn path(&self) -> &str {
        &self.path
    }
    pub(super) fn line(&self) -> usize {
        self.line
    }
    pub(super) fn index(&self) -> usize {
        self.index
    }
}
impl CedarApp {
    pub(super) fn browse_search_hits(&self) -> Vec<BrowseHit> {
        let Some(accepted) = self.search_state.accepted.as_ref() else {
            return Vec::new();
        };
        let source = std::sync::Arc::new(accepted.clone());
        self.search_results
            .iter()
            .enumerate()
            .map(|(index, row)| BrowseHit {
                source: source.clone(),
                index,
                path: row.path.clone(),
                line: row.line,
                text_identity: egui::Id::new(&row.text),
            })
            .collect()
    }
    pub(super) fn browse_search_current(&self, hit: &BrowseHit) -> bool {
        self.search_rows_active() && self.browse_search_snapshot_current(hit)
    }
    pub(super) fn browse_search_snapshot_current(&self, hit: &BrowseHit) -> bool {
        self.search_state.accepted.as_ref() == Some(hit.source.as_ref())
            && self.search_results.get(hit.index).is_some_and(|row| {
                row.path == hit.path
                    && row.line == hit.line
                    && egui::Id::new(&row.text) == hit.text_identity
            })
    }
}

impl CedarApp {
    pub(super) fn browse_search_snippet(&self, hit: &BrowseHit) -> Option<&str> {
        self.browse_search_snapshot_current(hit)
            .then(|| self.search_results[hit.index].text.as_str())
    }
}
