//! Common Find and return entry. Provider adapters retain
//! authority; this view owns only selection, native input and one bounded preview.
use crate::{
    chrome, navigation, search_keyboard, CedarApp, ConnectionState, Job, Tool, WorkspaceKey, AMBER,
    MUTED,
};
use cedar_protocol::{
    FileDiscoveryIncompleteReason, Operation, Payload, DEFAULT_FILE_DISCOVERY_RESULTS,
    MAX_FILE_BYTES,
};
use eframe::egui::{self, emath::GuiRounding, RichText};
use std::{collections::HashSet, sync::Arc};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(super) enum Provider {
    #[default]
    Files,
    ProjectText,
}

#[derive(Clone, Copy)]
enum GlobalEntry {
    Browse(Provider),
    Recent,
}

#[derive(Clone, PartialEq, Eq)]
enum Row {
    Project(ProjectCandidate),
    File(navigation::LoadedCandidate),
    Text(search_keyboard::BrowseHit),
}
impl Row {
    fn path(&self) -> &str {
        match self {
            Self::Project(row) => &row.source.paths[row.index],
            Self::File(row) => row.path(),
            Self::Text(row) => row.path(),
        }
    }
    fn line(&self) -> Option<usize> {
        match self {
            Self::Project(_) | Self::File(_) => None,
            Self::Text(row) => Some(row.line()),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
struct Context {
    workspace: Option<WorkspaceKey>,
    root: String,
    generation: u64,
    navigation: u64,
    connection: ConnectionState,
    document: Option<(u64, u64)>,
}
impl Context {
    fn capture(app: &CedarApp) -> Self {
        Self {
            workspace: app.workspace_key.clone(),
            root: app.root.clone(),
            generation: app.generation,
            navigation: app.navigation_epoch,
            connection: app.state,
            document: app.active().map(|doc| (doc.id, doc.edit_version)),
        }
    }
    fn current(&self, app: &CedarApp) -> bool {
        *self == Self::capture(app) && self.document.is_none_or(|(_, version)| version != u64::MAX)
    }
}

// Discovery owns a separate bounded request ledger. Every candidate borrows
// its exact path from one immutable, validated result snapshot.
#[derive(Clone, PartialEq, Eq)]
pub(super) struct DiscoveryTicket {
    context: Context,
    epoch: u64,
    query: String,
    scope: String,
    version: u32,
}
#[derive(PartialEq, Eq)]
struct ProjectSource {
    ticket: DiscoveryTicket,
    request: u64,
    paths: Vec<String>,
    incomplete: bool,
    reasons: Vec<FileDiscoveryIncompleteReason>,
    examined: usize,
}
#[derive(Clone, PartialEq, Eq)]
struct ProjectCandidate {
    source: Arc<ProjectSource>,
    index: usize,
}
#[derive(Default)]
struct Discovery {
    desired: Option<DiscoveryTicket>,
    accepted: Option<Arc<ProjectSource>>,
    flight: Option<(u64, DiscoveryTicket)>,
    replacement: Option<DiscoveryTicket>,
    completed: Option<(u64, DiscoveryTicket, Result<Payload, String>)>,
}
impl Discovery {
    fn pending_current(&self) -> bool {
        self.desired.as_ref().is_some_and(|ticket| {
            self.replacement.as_ref() == Some(ticket)
                || self
                    .flight
                    .as_ref()
                    .is_some_and(|(_, pending)| pending == ticket)
        })
    }
    fn retire(&mut self) {
        self.desired = None;
        self.accepted = None;
        self.replacement = None;
    }
}

// One committed Files edit owns one deadline. Repaints never renew it, and
// an A → B → A edit gets a new epoch even when its strings match an old flight.
struct FilesSettlement {
    context: Context,
    epoch: u64,
    query: String,
    scope: String,
    version: Option<u32>,
    deadline: f64,
    last_seen: f64,
}
pub(super) const FILES_DEBOUNCE_SECONDS: f64 = 0.250;

#[derive(Clone, Copy, PartialEq, Eq)]
enum PreviewSurface {
    Browse,
    Search,
}

#[derive(Clone, PartialEq, Eq)]
pub(super) struct PreviewTicket {
    surface: PreviewSurface,
    serial: u64,
    context: Context,
    epoch: u64,
    provider: Provider,
    query: String,
    index: usize,
    row: Row,
    draft: Option<(u64, u64)>,
}

impl PreviewTicket {
    fn same_intent(&self, other: &Self) -> bool {
        self.surface == other.surface
            && self.context == other.context
            && self.epoch == other.epoch
            && self.provider == other.provider
            && self.query == other.query
            && self.index == other.index
            && self.row == other.row
            && self.draft == other.draft
    }
}

// An arm belongs to one accepted text view, independently of its selection.
// The per-selection ticket is reserved immediately; only fulfillment settles.
#[derive(Clone, PartialEq, Eq)]
struct ProjectTextReview {
    surface: PreviewSurface,
    context: Context,
    epoch: u64,
    source: search_keyboard::Source,
    view: search_keyboard::View,
}
struct PreviewReveal {
    ticket: PreviewTicket,
    focus: egui::Id,
}
struct PreviewSettlement {
    ticket: PreviewTicket,
    deadline: f64,
    last_seen: f64,
}
pub(super) const PROJECT_TEXT_REVIEW_DEBOUNCE_SECONDS: f64 = 0.150;
pub(super) const MAX_PROJECT_TEXT_CONTEXT_LINES: usize = 41;
pub(super) const MAX_PROJECT_TEXT_CONTEXT_BYTES: usize = 64 * 1024;

// Keep the original clock sample: (deadline - interval) is not a stable inverse
// in floating point and can falsely reject the selection's own frame.
pub(super) fn preview_settlement_remaining(now: f64, last_seen: f64, deadline: f64) -> Option<f64> {
    (now.is_finite() && now >= last_seen).then_some(deadline - now)
}

#[derive(Debug)]
pub(super) struct ProjectTextContext {
    pub(super) text: String,
    pub(super) note: String,
}

// Borrow only the requested window. Every line gets a bounded share, so a
// giant preceding line cannot hide the marked historical target.
pub(super) fn project_text_context(text: &str, requested: usize) -> ProjectTextContext {
    let first = requested
        .saturating_sub(MAX_PROJECT_TEXT_CONTEXT_LINES / 2)
        .max(1);
    let last = requested.saturating_add(MAX_PROJECT_TEXT_CONTEXT_LINES / 2);
    let mut lines = Vec::new();
    let mut total_seen = 0;
    let mut after = false;
    for (index, line) in text.split('\n').enumerate() {
        let number = index + 1;
        total_seen = number;
        if number > last {
            after = true;
            break;
        }
        if number >= first {
            lines.push((number, line.strip_suffix('\r').unwrap_or(line)));
        }
    }
    if requested == 0
        || requested > total_seen
        || !lines.iter().any(|(number, _)| *number == requested)
    {
        return ProjectTextContext { text: String::new(), note: format!("Context unavailable: historical line {requested} is beyond the current source ({} lines).", total_seen) };
    }
    let share = MAX_PROJECT_TEXT_CONTEXT_BYTES / lines.len();
    let marker = " … [line truncated]";
    let mut rendered = String::new();
    let mut truncated = first > 1 || after;
    for (number, line) in &lines {
        let prefix = format!(
            "{} {number} | ",
            if *number == requested { "▶" } else { " " }
        );
        let available = share.saturating_sub(prefix.len() + 1);
        rendered.push_str(&prefix);
        if line.len() > available {
            let mut end = available.saturating_sub(marker.len()).min(line.len());
            while !line.is_char_boundary(end) {
                end -= 1;
            }
            rendered.push_str(&line[..end]);
            rendered.push_str(marker);
            truncated = true;
        } else {
            rendered.push_str(line);
        }
        rendered.push('\n');
    }
    rendered.pop();
    ProjectTextContext {
        text: rendered,
        note: format!(
            "Historical requested line {requested} · lines {}–{}{} · coordinates may have changed",
            lines[0].0,
            lines.last().unwrap().0,
            if truncated {
                " · context truncated (41 lines / 64 KiB maximum)"
            } else {
                ""
            }
        ),
    }
}

enum Preview {
    Draft(PreviewTicket),
    Disk {
        ticket: PreviewTicket,
        text: String,
        revision: String,
    },
    Message {
        ticket: PreviewTicket,
        message: String,
    },
}
impl Preview {
    fn ticket(&self) -> &PreviewTicket {
        match self {
            Self::Draft(ticket) | Self::Disk { ticket, .. } | Self::Message { ticket, .. } => {
                ticket
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Target {
    Query,
    Scope,
    TextScope,
    Masks,
    MatchCase,
    Files,
    Text,
    Submit,
    Row(usize),
    Group(usize),
    Preview,
    HidePreview,
    Open,
    Close,
    PreviewPane,
}
#[derive(Clone, PartialEq, Eq)]
struct Token {
    epoch: u64,
    target: Target,
    row: Option<Row>,
}
#[derive(Clone)]
struct Presented {
    token: Token,
    // Only the shared case control carries search-draft authority. Files and
    // other Browse controls keep their established token identity unchanged.
    case_identity: Option<search_keyboard::FocusIdentity>,
    id: egui::Id,
    rect: egui::Rect,
    clip: egui::Rect,
}
// egui counts clicks globally. A row may use that timing only after a clean,
// completed click on this exact row, in the same Browse view and preview pane.
#[derive(Clone, PartialEq, Eq)]
struct RowClick {
    token: Token,
    id: egui::Id,
    context: Context,
    provider: Provider,
    query: String,
    scope: String,
    pane: Option<PreviewTicket>,
}

#[derive(Clone)]
struct KeyIntent {
    source: Presented,
    key: egui::Key,
}
const MAX_GLOBAL_EDGES: usize = 128;
#[derive(Clone, Copy)]
struct GlobalEdge {
    index: usize,
    key: egui::Key,
    physical_key: Option<egui::Key>,
    pressed: bool,
    repeat: bool,
    modifiers: egui::Modifiers,
}
impl GlobalEdge {
    fn matches(&self, event: &egui::Event) -> bool {
        matches!(event, egui::Event::Key { key, physical_key, pressed, repeat, modifiers }
            if *key == self.key && *physical_key == self.physical_key && *pressed == self.pressed
                && *repeat == self.repeat && *modifiers == self.modifiers)
    }
}

#[derive(Default)]
struct Input {
    presented: Vec<Presented>,
    held: HashSet<egui::Key>,
    claimed: HashSet<egui::Key>,
    // Checkbox key ownership survives view retirement until physical release.
    case_claimed: HashSet<egui::Key>,
    case_attempted: bool,
    case_composing: bool,
    case_ime_blocked: bool,
    composing: bool,
    files_ime_commit: bool,
    files_ime_cancel: bool,
    files_ime_owner_stable: bool,
    files_ime_batch: bool,
    // Raw ownership is an additional gate, never an event-index mapping.
    files_ime_token: Option<Token>,
    files_ime_observed: bool,
    files_native_commit: bool,
    files_plain_edit: bool,
    files_edit: Option<Target>,
    key: Option<KeyIntent>,
    pressed: Option<Presented>,
    pressed_row_eligible: bool,
    click: Option<Presented>,
    completed_row: Option<RowClick>,
    // Frozen before the original raw batch; never derived from a completion
    // accepted by an earlier layout pass of that same batch.
    prior_row: Option<RowClick>,
    accepted_row: Option<(Token, egui::Id)>,
    row_click_eligible: bool,
    row_click_committed: bool,
    shortcut: Option<(GlobalEntry, u64, Option<WorkspaceKey>)>,
    global_edges: Vec<GlobalEdge>,
    global_overflow: bool,
    retained_events: Option<usize>,
    pass: Option<u64>,
    competing: bool,
}
impl Input {
    fn retire(&mut self) {
        self.presented.clear();
        self.key = None;
        self.pressed = None;
        self.pressed_row_eligible = false;
        self.click = None;
        self.break_row_continuity();
        self.accepted_row = None;
    }
    fn break_row_continuity(&mut self) {
        self.completed_row = None;
        self.prior_row = None;
    }
}

// Ownership survives a transfer back to the original field. Only a fresh
// composition lifecycle can replace an invalidated owner.
struct FilesComposition {
    owner: Target,
    valid: bool,
}

#[derive(Default)]
pub(super) struct Browse {
    context: Option<Context>,
    provider: Provider,
    query: String,
    scope: String,
    submitted: String,
    discovery: Discovery,
    files_settlement: Option<FilesSettlement>,
    // egui writes preedit into its String. Keep native text intact and pause
    // automatic admission until a clear commit or a later explicit action.
    files_composition: Option<FilesComposition>,
    // A separated Disabled then late Commit must not relabel an interrupted
    // composition as a new direct edit. Fresh owned edits/lifecycles release it.
    files_handoff_barrier: bool,
    epoch: u64,
    exhausted: bool,
    snapshot: Option<navigation::LoadedSnapshot>,
    rows: Vec<Row>,
    selected: Option<usize>,
    truncated: bool,
    message: Option<String>,
    text_request: Option<u64>,
    search_completed: Option<(u64, search_keyboard::Specification, Result<Payload, String>)>,
    desired: Option<PreviewTicket>,
    review: Option<ProjectTextReview>,
    settlement: Option<PreviewSettlement>,
    context_reveal: Option<PreviewReveal>,
    // A fresh explicit intent never reuses an earlier in-flight Read ticket,
    // including A → B → A and collapse → expand → preview of the same row.
    preview_serial: u64,
    preview: Option<Preview>,
    // Kept across dismissal and reopening. Only its actual reply drains flight.
    flight: Option<(u64, PreviewTicket)>,
    replacement: Option<PreviewTicket>,
    completed: Option<(u64, PreviewTicket, Result<Payload, String>)>,
    focus: Option<Target>,
    restore: Option<Context>,
    input: Input,
}
impl Browse {
    #[cfg(test)]
    pub(super) fn ime_focus_observation(&self) -> serde_json::Value {
        serde_json::json!({
            "epoch": self.epoch,
            "composition": self.files_composition.is_some(),
            "composition_owner": self.files_composition.as_ref().map(|c| format!("{:?}", c.owner)),
            "composition_owner_valid": self.files_composition.as_ref().map(|c| c.valid),
            "owner_stable_this_batch": self.input.files_ime_owner_stable,
            "handoff_barrier": self.files_handoff_barrier,
            "raw_ime_batch": self.input.files_ime_batch,
            "raw_plain_edit": self.input.files_plain_edit,
            "input_composing": self.input.composing,
            "raw_commit": self.input.files_ime_commit,
            "raw_cancel": self.input.files_ime_cancel,
            "native_commit": self.input.files_native_commit,
            "native_observed": self.input.files_ime_observed,
            "settlement": self.files_settlement.as_ref().map(|s| serde_json::json!({
                "deadline": s.deadline, "last_seen": s.last_seen,
                "epoch": s.epoch, "query": s.query, "scope": s.scope,
                "generation": s.context.generation, "version": s.version,
            })),
            "flight": self.discovery.flight.as_ref().map(|(id, t)| serde_json::json!({
                "id": id, "epoch": t.epoch, "query": t.query, "scope": t.scope,
                "generation": t.context.generation, "version": t.version,
            })),
        })
    }
    #[cfg(test)]
    pub(super) fn scope(&self) -> &str {
        &self.scope
    }
    #[cfg(test)]
    pub(super) fn set_epoch_for_test(&mut self, epoch: u64) {
        self.epoch = epoch;
    }
    #[cfg(test)]
    pub(super) fn set_preview_serial_for_test(&mut self, serial: u64) {
        self.preview_serial = serial;
    }
    #[cfg(test)]
    pub(super) fn preview_serial(&self) -> u64 {
        self.preview_serial
    }
    #[cfg(test)]
    pub(super) fn query(&self) -> &str {
        &self.query
    }
    #[cfg(test)]
    pub(super) fn selected_index(&self) -> Option<usize> {
        self.selected
    }
    #[cfg(test)]
    pub(super) fn preview_visible(&self) -> bool {
        self.desired.is_some()
    }
    #[cfg(test)]
    pub(super) fn preview_message(&self) -> Option<&str> {
        match self.preview.as_ref() {
            Some(Preview::Message { message, .. }) => Some(message),
            _ => self.message.as_deref(),
        }
    }
    #[cfg(test)]
    pub(super) fn preview_text(&self) -> Option<&str> {
        match self.preview.as_ref() {
            Some(Preview::Disk { text, .. }) => Some(text),
            _ => None,
        }
    }
    #[cfg(test)]
    pub(super) fn busy(&self) -> bool {
        self.flight.is_some()
    }
    #[cfg(test)]
    pub(super) fn paths(&self) -> Vec<&str> {
        self.rows.iter().map(Row::path).collect()
    }
    fn retire_view(&mut self) {
        if let Some(epoch) = self.epoch.checked_add(1) {
            self.epoch = epoch;
        } else {
            self.exhausted = true;
        }
        self.search_completed = None;
        self.discovery.retire();
        self.files_settlement = None;
        self.rows.clear();
        self.selected = None;
        self.desired = None;
        self.review = None;
        self.settlement = None;
        self.context_reveal = None;
        self.preview = None;
        self.replacement = None;
        self.message = None;
        self.truncated = false;
        self.input.retire();
    }
    fn token(&self, target: Target) -> Token {
        let row = match target {
            Target::Row(index) => self.rows.get(index),
            Target::Preview | Target::HidePreview | Target::Open | Target::PreviewPane => {
                self.selected.and_then(|index| self.rows.get(index))
            }
            _ => None,
        }
        .cloned();
        Token {
            epoch: self.epoch,
            target,
            row,
        }
    }
    fn id(&self, target: &Target) -> egui::Id {
        egui::Id::new((
            "browse_control",
            if matches!(
                target,
                Target::Query
                    | Target::Scope
                    | Target::TextScope
                    | Target::Masks
                    | Target::MatchCase
                    | Target::Files
                    | Target::Text
                    | Target::Submit
                    | Target::Close
            ) {
                0
            } else {
                self.epoch
            },
            format!("{target:?}"),
        ))
    }
}

impl CedarApp {
    pub(super) fn show_browse(&mut self, provider: Provider) {
        if self.browse.exhausted
            || self.workspace_key.is_none()
            || self.foreign_modal_owns_input(&self.editor_ctx)
        {
            return;
        }
        if !self.browse_marker_open() {
            self.show_browse_marker();
            self.browse.context = Some(Context::capture(self));
        }
        self.browse_switch(provider);
    }
    fn browse_switch(&mut self, provider: Provider) {
        self.browse.files_composition = None;
        self.browse.files_handoff_barrier = false;
        if self.browse.text_request.take().is_some() {
            self.search_state.retire();
        }
        self.browse.retire_view();
        if self.browse_refuse_exhaustion() {
            return;
        }
        self.browse.provider = provider;
        self.browse.query = if provider == Provider::ProjectText {
            self.search_query.clone()
        } else {
            String::new()
        };
        self.browse.submitted.clear();
        self.browse.scope.clear();
        self.browse.focus = Some(Target::Query);
        self.browse.restore = None;
        self.browse.snapshot = None;
        if provider == Provider::ProjectText {
            self.restore_browse_text();
        }
        if provider == Provider::Files {
            match self.browse_loaded_snapshot() {
                Ok(snapshot) => self.browse.snapshot = Some(snapshot),
                Err(error) => self.browse.message = Some(error),
            }
            if self.browse.snapshot.is_some() {
                self.browse_submit();
            }
        }
    }
    fn restore_browse_text(&mut self) {
        self.browse.submitted = self
            .search_state
            .accepted
            .as_ref()
            .map(|source| source.query.clone())
            .unwrap_or_default();
        self.browse.rows = self
            .browse_search_hits()
            .into_iter()
            .map(Row::Text)
            .collect();
        self.browse.selected = self.search_state.selected_index();
        self.browse.truncated = self.search_truncated;
        if self.search_state.accepted.is_some() && !self.search_rows_active() {
            self.browse.message = Some(
                "Previous results are inactive. Run Find for the current query, filters and connection."
                    .into(),
            );
        }
    }
    fn browse_text_visible(&self, index: usize) -> bool {
        self.browse.provider != Provider::ProjectText || self.search_state.visible(index)
    }
    fn browse_toggle_text_group(&mut self, first: usize) {
        if self.browse.provider != Provider::ProjectText || !self.search_rows_active() {
            return;
        }
        self.search_state.toggle_group(first);
        self.browse.input.retire();
        self.browse.review = None;
        self.browse.settlement = None;
        self.browse.desired = None;
        self.browse.preview = None;
        self.browse.replacement = None;
        self.browse.focus = Some(Target::Group(first));
    }
    pub(super) fn browse_retire(&mut self) {
        self.browse.files_composition = None;
        self.browse.files_handoff_barrier = false;
        if self.browse.text_request.take().is_some() {
            self.search_state.retire();
        }
        self.browse.retire_view();
        self.browse.context = None;
        self.browse.snapshot = None;
        self.browse.restore = None;
        self.browse.focus = None;
    }
    fn browse_refuse_exhaustion(&mut self) -> bool {
        if !self.browse.exhausted {
            return false;
        }
        self.dismiss_navigation();
        self.error = Some("Find cannot continue because its session identity is exhausted. Reopen Cedar to start a new session.".into());
        true
    }
    fn browse_current(&self) -> bool {
        !self.browse.exhausted
            && self.browse_marker_open()
            && self
                .browse
                .context
                .as_ref()
                .is_some_and(|context| context.current(self))
            && !self.open_form
            && !self.new_file
            && self.confirm.is_none()
    }
    fn browse_row_current(&self, row: &Row) -> bool {
        match row {
            Row::Project(row) => {
                self.browse
                    .discovery
                    .accepted
                    .as_ref()
                    .is_some_and(|source| Arc::ptr_eq(source, &row.source))
                    && self.discovery_ticket_current(&row.source.ticket)
            }
            Row::File(row) => self.browse_loaded_current(row),
            Row::Text(row) => self.browse_search_current(row),
        }
    }
    fn browse_ticket_current(&self, ticket: &PreviewTicket) -> bool {
        if ticket.serial != self.browse.preview_serial {
            return false;
        }
        if ticket.surface == PreviewSurface::Search {
            return self.tools_open
                && self.tool == Tool::Search
                && !self.navigation.blocks_editor()
                && !self.open_form
                && !self.new_file
                && !self.foreign_modal_owns_input(&self.editor_ctx)
                && ticket.context.current(self)
                && self.search_state.selected_index() == Some(ticket.index)
                && self.search_state.visible(ticket.index)
                && self.search_query == ticket.query
                && matches!(&ticket.row, Row::Text(hit) if self.browse_search_current(hit))
                && self
                    .documents
                    .iter()
                    .find(|doc| doc.path == ticket.row.path())
                    .map(|doc| (doc.id, doc.edit_version))
                    == ticket.draft
                && ticket.draft.is_none_or(|(_, version)| version != u64::MAX);
        }
        self.browse_current()
            && ticket.context.current(self)
            && ticket.epoch == self.browse.epoch
            && ticket.provider == self.browse.provider
            && ticket.query == self.browse.query
            && ticket.query == self.browse.submitted
            && self.browse.selected == Some(ticket.index)
            && self.browse_text_visible(ticket.index)
            && self.browse.rows.get(ticket.index) == Some(&ticket.row)
            && self.browse_row_current(&ticket.row)
            && self
                .documents
                .iter()
                .find(|doc| doc.path == ticket.row.path())
                .map(|doc| (doc.id, doc.edit_version))
                == ticket.draft
            && ticket.draft.is_none_or(|(_, version)| version != u64::MAX)
    }
    pub(super) fn browse_submit(&mut self) {
        if !self.browse_current()
            || (self.browse.provider == Provider::ProjectText && self.search_busy())
        {
            return;
        }
        if self.browse.provider == Provider::Files
            && self.browse.snapshot.is_none()
            && !self.backend_file_discovery_supported()
        {
            return;
        }
        if self.browse.provider == Provider::Files {
            if self.browse.files_composition.is_some() || self.browse.input.composing {
                return;
            }
            self.browse.files_handoff_barrier = false;
            // Enter/Find flush a waiting edit, but never duplicate its already
            // admitted flight or latest replacement.
            if self.browse.discovery.pending_current()
                && self
                    .browse
                    .discovery
                    .desired
                    .as_ref()
                    .is_some_and(|ticket| self.discovery_ticket_current(ticket))
            {
                self.browse.files_settlement = None;
                return;
            }
        }
        self.retire_browse_view_keep_controls();
        if self.browse_refuse_exhaustion() {
            return;
        }
        self.browse.submitted = self.browse.query.clone();
        self.browse.focus = Some(Target::Query);
        if let Some(query) = self
            .browse
            .input
            .presented
            .iter()
            .find(|item| item.token.target == Target::Query)
        {
            if let Some(response) = self
                .editor_ctx
                .read_response(query.id)
                .filter(|response| response.enabled())
            {
                response.request_focus();
                self.browse.focus = None;
            }
        }
        match self.browse.provider {
            Provider::Files => self.browse_admit_files(),
            Provider::ProjectText => {
                self.search_state.deactivate();
                if self.next_request == 0 || self.next_request == u64::MAX {
                    self.browse.message = Some("Find cannot submit: the request identity counter is exhausted. Reopen Cedar to start a new session.".into());
                    return;
                }
                self.search_query = self.browse.query.clone();
                if !self.search_query.trim().is_empty() && self.search_submission_supported() {
                    self.search();
                    self.browse.text_request = self
                        .search_state
                        .pending
                        .as_ref()
                        .map(|source| source.request);
                    if self.browse.text_request.is_none() {
                        self.browse.message = self.error.clone();
                        self.restore_browse_text();
                    }
                } else {
                    self.browse.message = self.search_filter_explanation().or_else(|| {
                        Some(format!(
                            "Enter a {} literal query, then press Enter to search.",
                            search_keyboard::case_mode(self.search_match_case).to_lowercase()
                        ))
                    });
                    self.restore_browse_text();
                }
            }
        }
    }
    // Edits retire row gestures immediately, while the unchanged query/scope
    // controls remain usable in the next native batch (including IME commit).
    fn retire_browse_view_keep_controls(&mut self) {
        let controls: Vec<_> = self
            .browse
            .input
            .presented
            .iter()
            .filter(|item| {
                matches!(
                    item.token.target,
                    Target::Query
                        | Target::Scope
                        | Target::TextScope
                        | Target::Masks
                        | Target::MatchCase
                        | Target::Files
                        | Target::Text
                        | Target::Submit
                        | Target::Close
                )
            })
            .cloned()
            .collect();
        self.browse.retire_view();
        for mut control in controls {
            control.token = self.browse.token(control.token.target.clone());
            self.browse.input.presented.push(control);
        }
    }

    // Shared preparation only: automatic settlement must never run the
    // explicit submit wrapper's retirement or keyboard-focus behavior.
    fn browse_admit_files(&mut self) {
        self.browse.submitted = self.browse.query.clone();
        if self.backend_file_discovery_supported() && !self.browse.query.is_empty() {
            if let Err(error) = cedar_protocol::validate_file_discovery_request(
                &self.browse.query,
                &self.browse.scope,
                DEFAULT_FILE_DISCOVERY_RESULTS,
            ) {
                self.browse.message = Some(error.to_string());
                return;
            }
            let ticket = DiscoveryTicket {
                context: self.browse.context.as_ref().unwrap().clone(),
                epoch: self.browse.epoch,
                query: self.browse.query.clone(),
                scope: self.browse.scope.clone(),
                version: self.file_discovery.unwrap().version,
            };
            self.browse.discovery.desired = Some(ticket.clone());
            self.browse.discovery.replacement = Some(ticket);
        } else if self.browse.query.is_empty() && !self.browse.scope.is_empty() {
            self.browse.message =
                Some("Enter a filename query before searching this directory scope.".into());
        } else if let Some(snapshot) = &self.browse.snapshot {
            let (rows, truncated) = snapshot.matching(&self.browse.query);
            self.browse.rows = rows.into_iter().map(Row::File).collect();
            self.browse.truncated = truncated;
            self.browse.selected = (!self.browse.rows.is_empty()).then_some(0);
        }
    }

    fn arm_files_settlement(&mut self) {
        if !self.browse_current()
            || self.browse.files_composition.is_some()
            || self.browse.input.composing
        {
            return;
        }
        if self.browse.files_handoff_barrier {
            self.browse.message = Some("Automatic search paused after composition. Edit the query or press Find to search.".into());
            self.editor_ctx.request_repaint();
            return;
        }
        let now = self.editor_ctx.input(|input| input.time);
        if !now.is_finite() {
            return;
        }
        self.browse.files_settlement = Some(FilesSettlement {
            context: self.browse.context.as_ref().unwrap().clone(),
            epoch: self.browse.epoch,
            query: self.browse.query.clone(),
            scope: self.browse.scope.clone(),
            version: self.file_discovery.map(|descriptor| descriptor.version),
            deadline: now + FILES_DEBOUNCE_SECONDS,
            last_seen: now,
        });
        self.browse.message = Some("Waiting for typing to pause… Enter searches now.".into());
    }

    pub(super) fn finish_files_settlement(&mut self, ctx: &egui::Context, now: f64) {
        let Some(mut settlement) = self.browse.files_settlement.take() else {
            return;
        };
        if !self.browse_current()
            || self.foreign_modal_owns_input(ctx)
            || self.browse.provider != Provider::Files
            || self.browse.files_composition.is_some()
            || self.browse.files_handoff_barrier
            || self.browse.input.composing
            || !settlement.context.current(self)
            || settlement.epoch != self.browse.epoch
            || settlement.query != self.browse.query
            || settlement.scope != self.browse.scope
            || settlement.version != self.file_discovery.map(|descriptor| descriptor.version)
        {
            if self.browse_current() && self.browse.provider == Provider::Files {
                self.browse.message = Some("Automatic search paused because the search context changed. Edit the query or press Find to search.".into());
                ctx.request_repaint();
            }
            return;
        }
        match preview_settlement_remaining(now, settlement.last_seen, settlement.deadline) {
            None => {
                self.browse.message =
                    Some("Typing pause was interrupted. Press Enter to search.".into());
                ctx.request_repaint();
            }
            Some(remaining) if remaining <= 0.0 => {
                self.browse.message = None;
                self.browse_admit_files();
                ctx.request_repaint();
            }
            Some(remaining) => {
                settlement.last_seen = now;
                self.browse.files_settlement = Some(settlement);
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(remaining));
            }
        }
    }

    fn observe_files_composition_focus(&mut self, target: Target, response: &egui::Response) {
        let Some(composition) = &mut self.browse.files_composition else {
            return;
        };
        if composition.owner == target {
            let stable = response.has_focus() && !response.gained_focus() && !response.lost_focus();
            composition.valid &= stable;
            self.browse.input.files_ime_owner_stable |= stable && composition.valid;
        }
    }

    // Receipts belong only to this invocation's output and widget-local
    // InputState snapshot. Consume their actual processing order directly;
    // input_event_index is deliberately never interpreted as a raw-batch index.
    fn observe_files_ime_receipts(
        &mut self,
        target: Target,
        response: &egui::Response,
        receipts: &[egui::text_edit::TextEditImeReceipt],
    ) -> bool {
        if self.browse.input.files_ime_observed
            || !self.browse_input_current(&self.editor_ctx)
            || !self.browse_current()
            || self.browse.input.files_ime_token.as_ref()
                != Some(&self.browse.token(target.clone()))
            || response.id != self.browse.id(&target)
        {
            // Native text can change before an unowned/stale raw intent is
            // rejected here. Do not let those IME bytes re-enter admission via
            // the ordinary changed-bytes route. A separate owned Text/Paste
            // intent below may still release the existing barrier.
            if self.browse.files_composition.is_none()
                && receipts.iter().any(|receipt| {
                    matches!(
                        receipt.outcome,
                        egui::text_edit::ImeEventOutcome::CommitAccepted { .. }
                            | egui::text_edit::ImeEventOutcome::PreeditApplied
                    )
                })
            {
                self.browse.files_handoff_barrier = true;
            }
            return false;
        }
        // Even absence is consumed for this owner/batch. An extra layout pass
        // cannot reinterpret the original raw intent or reuse a past receipt.
        self.browse.input.files_ime_observed = true;
        let mut committed = false;
        let mut accepted_direct = false;
        for receipt in receipts {
            use egui::text_edit::ImeEventOutcome::*;
            match receipt.outcome {
                Enabled | PreeditApplied | CommitEmpty | CommitIgnoredLineBreak => {
                    committed = false
                }
                CommitAccepted { .. } => {
                    committed = true;
                    accepted_direct = true;
                }
                // A rejected duplicate is not a second successful completion,
                // nor does it erase an earlier success in the same lifecycle.
                CommitRejectedAnchor | Disabled | PreeditIgnoredLineBreak => {}
            }
        }
        self.browse.input.files_native_commit = committed;
        // Preserve the existing direct-edit contract: without a composition,
        // an actually accepted Commit is a fresh edit, including when a later
        // empty/rejected terminal does not insert. Composition cancellation
        // remains governed by the latest lifecycle's completion above.
        self.browse.files_composition.is_none() && accepted_direct
    }

    fn finish_files_composition(&mut self) {
        // A later-rendered control may have taken focus after the owner's
        // response was observed. Reconcile against the final native focus.
        if let Some(owner) = self
            .browse
            .files_composition
            .as_ref()
            .map(|c| c.owner.clone())
        {
            if self.editor_ctx.memory(|memory| memory.focused()) != Some(self.browse.id(&owner)) {
                self.browse.files_composition.as_mut().unwrap().valid = false;
            }
        }
        if self.browse.provider != Provider::Files || self.browse.input.composing {
            return;
        }
        if self.browse.files_composition.is_some()
            && (self.browse.input.files_ime_commit || self.browse.input.files_ime_cancel)
        {
            let composition = self.browse.files_composition.take().unwrap();
            if self.browse.input.files_ime_commit
                && self.browse.input.files_native_commit
                && composition.valid
                && self.browse.input.files_ime_owner_stable
            {
                // A terminal owned by a continuously focused original control
                // (even identical text) gets one full interval. Later focus
                // transfers cannot cancel this already completed composition.
                self.browse_query_changed();
            } else {
                self.browse.files_handoff_barrier = true;
                // Disabled, empty Commit and ambiguous endings do not prove
                // the final String committed. Preserve it exactly; only a new
                // committed edit or explicit Find may admit these bytes.
                self.retire_browse_view_keep_controls();
                if self.browse_refuse_exhaustion() {
                    return;
                }
                self.browse.submitted.clear();
                self.browse.message = Some("Automatic search paused after composition. Edit the query or press Find to search.".into());
                self.editor_ctx.request_repaint();
            }
        }
    }

    fn browse_query_changed(&mut self) {
        if self.browse.provider == Provider::ProjectText {
            self.search_query = self.browse.query.clone();
            self.search_filters_changed();
        }
        if self.browse.text_request.take().is_some() {
            self.search_state.retire();
        }
        self.retire_browse_view_keep_controls();
        if self.browse_refuse_exhaustion() {
            return;
        }
        if self.browse.provider == Provider::Files {
            self.browse.submitted.clear();
        } else {
            // Retain the accepted batch's actual list during this very frame;
            // an empty intermediate layout would clamp its scroll to zero.
            self.restore_browse_text();
        }
        if self.browse.provider == Provider::Files {
            self.arm_files_settlement();
        } else {
            self.browse.message = Some("Press Enter to submit this query.".into());
        }
    }
    pub(super) fn browse_select(&mut self, index: usize) {
        self.validate_project_text_review();
        if !self.browse_current()
            || !self.browse_text_visible(index)
            || !self
                .browse
                .rows
                .get(index)
                .is_some_and(|row| self.browse_row_current(row))
        {
            return;
        }
        if self.browse.selected != Some(index) {
            self.browse.settlement = None;
            self.browse.desired = None;
            self.browse.preview = None;
            self.browse.replacement = None;
        }
        self.browse.selected = Some(index);
        if self.browse.provider == Provider::ProjectText {
            self.select_search_row(index);
        }
        self.browse.focus = Some(Target::Row(index));
    }
    fn browse_preview_ticket(&self) -> Option<PreviewTicket> {
        let context = self.browse.context.clone()?;
        let index = self.browse.selected?;
        let row = self.browse.rows.get(index)?.clone();
        let draft = self
            .documents
            .iter()
            .find(|doc| doc.path == row.path())
            .map(|doc| (doc.id, doc.edit_version));
        let ticket = PreviewTicket {
            surface: PreviewSurface::Browse,
            serial: self.browse.preview_serial,
            context,
            epoch: self.browse.epoch,
            provider: self.browse.provider,
            query: self.browse.query.clone(),
            index,
            row,
            draft,
        };
        self.browse_ticket_current(&ticket).then_some(ticket)
    }
    fn search_preview_ticket(&self, index: usize) -> Option<PreviewTicket> {
        let hit = self
            .browse_search_hits()
            .into_iter()
            .find(|hit| hit.index() == index)?;
        let draft = self
            .documents
            .iter()
            .find(|doc| doc.path == hit.path())
            .map(|doc| (doc.id, doc.edit_version));
        let ticket = PreviewTicket {
            surface: PreviewSurface::Search,
            serial: self.browse.preview_serial,
            context: Context::capture(self),
            epoch: self.search_request,
            provider: Provider::ProjectText,
            query: self.search_query.clone(),
            index,
            row: Row::Text(hit),
            draft,
        };
        self.browse_ticket_current(&ticket).then_some(ticket)
    }
    pub(super) fn browse_preview(&mut self) {
        if let Some(ticket) = self.browse_preview_ticket() {
            self.request_find_preview(ticket);
            self.browse.focus = Some(if self.browse.provider == Provider::ProjectText {
                Target::Row(self.browse.selected.unwrap())
            } else {
                Target::PreviewPane
            });
        }
    }
    pub(super) fn search_preview(&mut self, index: usize) {
        if let Some(ticket) = self.search_preview_ticket(index) {
            self.request_find_preview(ticket);
        }
    }
    fn project_text_review(&self, surface: PreviewSurface) -> Option<ProjectTextReview> {
        let visible = match surface {
            PreviewSurface::Browse => {
                self.browse_current()
                    && self.browse.provider == Provider::ProjectText
                    && self.browse.query == self.browse.submitted
            }
            PreviewSurface::Search => {
                self.tools_open
                    && self.tool == Tool::Search
                    && !self.navigation.blocks_editor()
                    && !self.open_form
                    && !self.new_file
            }
        };
        if !visible || self.foreign_modal_owns_input(&self.editor_ctx) {
            return None;
        }
        Some(ProjectTextReview {
            surface,
            context: Context::capture(self),
            epoch: if surface == PreviewSurface::Browse {
                self.browse.epoch
            } else {
                self.search_request
            },
            source: self.search_state.accepted.clone()?,
            view: search_keyboard::View::of(self)?,
        })
    }
    fn review_current(&self) -> bool {
        self.browse.review.as_ref().is_some_and(|review| {
            self.project_text_review(review.surface).as_ref() == Some(review)
                && self
                    .browse
                    .desired
                    .as_ref()
                    .is_none_or(|ticket| self.browse_ticket_current(ticket))
        })
    }
    pub(super) fn validate_project_text_review(&mut self) {
        if self.browse.review.is_some() && !self.review_current() {
            self.retire_project_text_preview();
        }
    }
    #[cfg(test)]
    pub(super) fn project_text_review_armed(&self) -> bool {
        self.review_current()
    }
    #[cfg(test)]
    pub(super) fn project_text_review_pending(&self) -> bool {
        self.browse.settlement.is_some()
    }
    pub(super) fn search_review_armed(&self) -> bool {
        self.review_current()
            && self
                .browse
                .review
                .as_ref()
                .is_some_and(|review| review.surface == PreviewSurface::Search)
    }
    fn browse_select_owned(&mut self, index: usize) {
        let previous = self.browse.selected;
        self.browse_select(index);
        if self.browse.selected != previous && self.browse.selected == Some(index) {
            self.settle_project_text_selection(PreviewSurface::Browse, index);
        }
    }
    pub(super) fn search_selection_owned(&mut self, index: usize) {
        let previous = self.search_state.selected_index();
        self.select_search_row(index);
        if self.search_state.selected_index() != previous
            && self.search_state.selected_index() == Some(index)
        {
            self.settle_project_text_selection(PreviewSurface::Search, index);
        }
    }
    fn settle_project_text_selection(&mut self, surface: PreviewSurface, index: usize) {
        if !self.review_current()
            || !self
                .browse
                .review
                .as_ref()
                .is_some_and(|review| review.surface == surface)
        {
            return;
        }
        let ticket = match surface {
            PreviewSurface::Browse => self.browse_preview_ticket(),
            PreviewSurface::Search => self.search_preview_ticket(index),
        };
        if let Some(ticket) = ticket.and_then(|ticket| self.reserve_find_preview(ticket)) {
            let now = self.editor_ctx.input(|input| input.time);
            if !now.is_finite() {
                self.retire_project_text_preview();
                return;
            }
            self.browse.settlement = Some(PreviewSettlement {
                ticket,
                deadline: now + PROJECT_TEXT_REVIEW_DEBOUNCE_SECONDS,
                last_seen: now,
            });
            self.editor_ctx
                .request_repaint_after(std::time::Duration::from_secs_f64(
                    PROJECT_TEXT_REVIEW_DEBOUNCE_SECONDS,
                ));
        }
    }
    pub(super) fn clear_search_selection_preview(&mut self) {
        if self
            .browse
            .desired
            .as_ref()
            .is_some_and(|ticket| ticket.surface == PreviewSurface::Search)
        {
            self.browse.desired = None;
            self.browse.preview = None;
            self.browse.replacement = None;
            self.browse.settlement = None;
            self.browse.context_reveal = None;
        }
    }
    pub(super) fn retire_project_text_preview(&mut self) {
        self.browse.context_reveal = None;
        self.browse.review = None;
        self.browse.settlement = None;
        if self
            .browse
            .desired
            .as_ref()
            .is_some_and(|ticket| ticket.provider == Provider::ProjectText)
        {
            self.browse.desired = None;
            self.browse.preview = None;
            self.browse.replacement = None;
        }
    }
    pub(super) fn retire_search_preview(&mut self) {
        if self
            .browse
            .context_reveal
            .as_ref()
            .is_some_and(|reveal| reveal.ticket.surface == PreviewSurface::Search)
        {
            self.browse.context_reveal = None;
        }
        if self
            .browse
            .review
            .as_ref()
            .is_some_and(|review| review.surface == PreviewSurface::Search)
        {
            self.browse.review = None;
            self.browse.settlement = None;
        }
        if self
            .browse
            .desired
            .as_ref()
            .is_some_and(|ticket| ticket.surface == PreviewSurface::Search)
        {
            self.browse.desired = None;
            self.browse.preview = None;
            self.browse.replacement = None;
        }
    }
    pub(super) fn search_preview_current(&self) -> bool {
        self.browse.desired.as_ref().is_some_and(|ticket| {
            ticket.surface == PreviewSurface::Search && self.browse_ticket_current(ticket)
        })
    }
    #[cfg(test)]
    pub(super) fn has_search_preview(&self) -> bool {
        self.search_preview_current()
    }
    #[cfg(test)]
    pub(super) fn browse_preview_text(&self) -> Option<&str> {
        self.current_preview_text(PreviewSurface::Browse)
    }
    #[cfg(test)]
    pub(super) fn search_preview_text(&self) -> Option<&str> {
        self.current_preview_text(PreviewSurface::Search)
    }
    fn current_preview_text(&self, surface: PreviewSurface) -> Option<&str> {
        let ticket = self.browse.desired.as_ref()?;
        if ticket.surface != surface || !self.browse_ticket_current(ticket) {
            return None;
        }
        match self
            .browse
            .preview
            .as_ref()
            .filter(|preview| preview.ticket() == ticket)?
        {
            Preview::Draft(ticket) => self
                .documents
                .iter()
                .find(|doc| {
                    Some((doc.id, doc.edit_version)) == ticket.draft
                        && doc.path == ticket.row.path()
                })
                .map(|doc| doc.text.as_str()),
            Preview::Disk { text, .. } => Some(text),
            Preview::Message { .. } => None,
        }
    }
    fn request_context_reveal(&mut self, ticket: &PreviewTicket) {
        self.browse.context_reveal = if ticket.provider == Provider::ProjectText {
            let focus = match ticket.surface {
                PreviewSurface::Browse => self.browse.id(&Target::Row(ticket.index)),
                PreviewSurface::Search => search_keyboard::row_id(
                    self.search_state.accepted.as_ref().unwrap(),
                    ticket.index,
                ),
            };
            Some(PreviewReveal {
                ticket: ticket.clone(),
                focus,
            })
        } else {
            None
        };
    }
    fn capture_context_reveal_input(&mut self, ctx: &egui::Context, raw: &egui::RawInput) {
        let Some(reveal) = &self.browse.context_reveal else {
            return;
        };
        let pending_row_focus = reveal.ticket.surface == PreviewSurface::Browse
            && self.browse.focus == Some(Target::Row(reveal.ticket.index));
        if !raw.focused
            || (!pending_row_focus && ctx.memory(|memory| memory.focused()) != Some(reveal.focus))
            || raw.events.iter().any(|event| {
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
                        | egui::Event::Ime(egui::ImeEvent::Preedit(_) | egui::ImeEvent::Commit(_))
                        | egui::Event::Touch { .. }
                        | egui::Event::WindowFocused(false)
                )
            })
        {
            self.browse.context_reveal = None;
        }
    }
    fn take_context_reveal(&mut self, ui: &egui::Ui, surface: PreviewSurface) -> bool {
        if ui.is_sizing_pass() || self.current_preview_text(surface).is_none() {
            return false;
        }
        let Some(reveal) = self.browse.context_reveal.take() else {
            return false;
        };
        reveal.ticket.surface == surface
            && self.browse.desired.as_ref() == Some(&reveal.ticket)
            && self.browse_ticket_current(&reveal.ticket)
            && ui.memory(|memory| memory.focused()) == Some(reveal.focus)
    }
    fn current_project_text_context(&self, surface: PreviewSurface) -> Option<ProjectTextContext> {
        let ticket = self.browse.desired.as_ref()?;
        if ticket.provider != Provider::ProjectText || ticket.surface != surface {
            return None;
        }
        Some(project_text_context(
            self.current_preview_text(surface)?,
            ticket.row.line()?,
        ))
    }
    #[cfg(test)]
    pub(super) fn project_text_preview_context(&self) -> Option<ProjectTextContext> {
        self.current_project_text_context(self.browse.desired.as_ref()?.surface)
    }
    pub(super) fn search_preview_panel(&mut self, ui: &mut egui::Ui, height: f32) {
        let current = self.browse.desired.as_ref().is_some_and(|ticket| {
            ticket.surface == PreviewSurface::Search && self.browse_ticket_current(ticket)
        });
        let title = if !current {
            "Read-only preview · select a result and press Space".to_owned()
        } else {
            match &self.browse.preview {
                Some(Preview::Draft(_)) => "Current open draft · read-only preview".into(),
                Some(Preview::Disk { revision, .. }) => format!(
                    "Disk preview · revision {} · historical search coordinates",
                    &revision[..8]
                ),
                Some(Preview::Message { message, .. }) => message.clone(),
                None => "Preview requested…".into(),
            }
        };
        ui.label(RichText::new(title).small().color(MUTED));
        let context = self.current_project_text_context(PreviewSurface::Search);
        let reveal = self.take_context_reveal(ui, PreviewSurface::Search);
        if let Some(context) = &context {
            ui.label(RichText::new(&context.note).small().color(MUTED));
        }
        let scroll = egui::ScrollArea::both()
            .id_salt("search_preview_scroll")
            .animated(false)
            .min_scrolled_height(0.0)
            .max_height(search_keyboard::visible_height(ui).min(height))
            .show(ui, |ui| {
                let response = if let Some(context) = &context {
                    draw_project_text_context(ui, context, reveal)
                } else {
                    ui.add(
                        egui::Label::new(
                            RichText::new(
                                "41 lines / 64 KiB maximum · preview does not change the editor",
                            )
                            .monospace(),
                        )
                        .selectable(false),
                    )
                };
                #[cfg(test)]
                crate::workspace_access_tests::record(ui, "search_preview_content", &response);
                #[cfg(not(test))]
                let _ = response;
            });
        #[cfg(test)]
        record_context_scroll(
            ui.ctx(),
            scroll.id,
            scroll.inner_rect,
            PreviewSurface::Search,
        );
        #[cfg(not(test))]
        let _ = scroll;
    }

    fn reserve_find_preview(&mut self, mut ticket: PreviewTicket) -> Option<PreviewTicket> {
        if self.browse.desired.as_ref().is_some_and(|desired| {
            desired.same_intent(&ticket) && self.browse_ticket_current(desired)
        }) {
            return None;
        }
        let Some(serial) = self.browse.preview_serial.checked_add(1) else {
            self.browse.desired = None;
            self.browse.preview = None;
            self.browse.replacement = None;
            self.browse.review = None;
            self.browse.settlement = None;
            self.error = Some("Preview cannot continue: the preview identity counter is exhausted. Reopen Cedar to start a new session.".into());
            return None;
        };
        self.browse.preview_serial = serial;
        ticket.serial = serial;
        self.browse.desired = Some(ticket.clone());
        self.request_context_reveal(&ticket);
        self.browse.preview = None;
        self.browse.replacement = None;
        self.browse.settlement = None;
        Some(ticket)
    }
    fn request_find_preview(&mut self, ticket: PreviewTicket) {
        let surface = ticket.surface;
        let project_text = ticket.provider == Provider::ProjectText;
        if let Some(ticket) = self.reserve_find_preview(ticket) {
            self.fulfill_find_preview(ticket);
        } else if let Some(settlement) = self.browse.settlement.take() {
            // A fresh explicit Space need not wait for the automatic deadline.
            self.fulfill_find_preview(settlement.ticket);
        }
        if project_text && self.browse.desired.is_some() {
            self.browse.review = self.project_text_review(surface);
            // Only a fresh explicit preview may renew reveal after intervening
            // input. Fulfillment/repaint never changes this authority.
            if let Some(ticket) = self.browse.desired.clone() {
                self.request_context_reveal(&ticket);
            }
        }
    }
    fn fulfill_find_preview(&mut self, ticket: PreviewTicket) {
        if !valid_preview_path(ticket.row.path()) {
            self.browse.preview = Some(Preview::Message { ticket, message: "Preview is unavailable because this result is not a valid workspace-relative file path.".into() });
            self.browse.replacement = None;
        } else if ticket.draft.is_some() {
            self.browse.preview = Some(
                if self
                    .documents
                    .iter()
                    .find(|doc| doc.path == ticket.row.path())
                    .is_some_and(|doc| {
                        ticket.provider == Provider::Files && doc.text.len() > MAX_FILE_BYTES
                    })
                {
                    Preview::Message { ticket, message: "This open draft exceeds the 1 MiB preview limit. Open it in the editor.".into() }
                } else {
                    Preview::Draft(ticket)
                },
            );
            self.browse.replacement = None;
        } else if self.ready() {
            self.browse.replacement = Some(ticket);
        } else {
            self.browse.preview = Some(Preview::Message {
                ticket,
                message: "Reconnect before previewing an unopened file.".into(),
            });
        }
    }
    pub(super) fn browse_open(&mut self) {
        let Some(row) = self
            .browse
            .selected
            .and_then(|index| self.browse.rows.get(index))
            .cloned()
        else {
            return;
        };
        if !self.browse_current()
            || self.browse.query != self.browse.submitted
            || !self
                .browse
                .selected
                .is_some_and(|index| self.browse_text_visible(index))
            || !self.browse_row_current(&row)
        {
            return;
        }
        if !self.ready() && !self.documents.iter().any(|doc| doc.path == row.path()) {
            return;
        }
        // Ordinary Open owns reuse and history; text hits retain the accepted
        // specification through a pending Read rather than losing its authority.
        if let Row::Text(hit) = &row {
            self.open_search_result(hit.index());
        } else {
            self.open(row.path().to_owned(), row.line());
        }
    }
    pub(super) fn browse_dismiss(&mut self) {
        let restore = self
            .browse
            .context
            .clone()
            .filter(|context| context.current(self));
        self.dismiss_navigation();
        self.browse.restore = restore;
    }
    pub(super) fn apply_browse_preview(
        &mut self,
        request: u64,
        ticket: PreviewTicket,
        result: Result<Payload, String>,
        connected: bool,
    ) {
        if !connected {
            self.browse.flight = None;
            self.browse.completed = None;
            self.browse.replacement = None;
            self.disconnected(result.err().unwrap_or_else(|| {
                "The connection closed during preview. Your drafts are retained".into()
            }));
            return;
        }
        if self
            .browse
            .flight
            .as_ref()
            .is_some_and(|(id, current)| *id == request && current == &ticket)
        {
            self.browse.completed = Some((request, ticket, result));
        }
    }
    pub(super) fn browse_owns_search(&self, request: u64) -> bool {
        self.browse.text_request == Some(request)
    }
    pub(super) fn stage_browse_search(
        &mut self,
        request: u64,
        specification: search_keyboard::Specification,
        result: Result<Payload, String>,
    ) {
        self.browse.search_completed = Some((request, specification, result));
    }
    pub(super) fn browse_finish_frame(&mut self, ctx: &egui::Context) {
        if self.browse.review.is_some() && !self.review_current() {
            self.retire_project_text_preview();
        }
        if let Some(mut settlement) = self.browse.settlement.take() {
            if self.review_current()
                && self.browse.desired.as_ref() == Some(&settlement.ticket)
                && self.browse_ticket_current(&settlement.ticket)
            {
                let now = ctx.input(|input| input.time);
                match preview_settlement_remaining(now, settlement.last_seen, settlement.deadline) {
                    None => self.retire_project_text_preview(),
                    Some(remaining) if remaining <= 0.0 => {
                        self.fulfill_find_preview(settlement.ticket);
                        // The pane was already painted this frame. Local draft
                        // and refusal outcomes need a wake to display them.
                        if self.browse.preview.is_some() {
                            ctx.request_repaint();
                        }
                    }
                    Some(remaining) => {
                        ctx.request_repaint_after(std::time::Duration::from_secs_f64(remaining));
                        settlement.last_seen = now;
                        self.browse.settlement = Some(settlement);
                    }
                }
            }
        }
        self.finish_files_settlement(ctx, ctx.input(|input| input.time));
        self.finish_browse_discovery(ctx);
        let owns_view = self.browse_current() && !self.foreign_modal_owns_input(ctx);
        if let Some((request, specification, result)) = self.browse.search_completed.take() {
            if owns_view
                && self.browse.text_request == Some(request)
                && self.browse.query == specification.request.query
                && self.browse.submitted == specification.request.query
            {
                self.apply_search_response(request, specification, result);
                if self
                    .search_state
                    .accepted
                    .as_ref()
                    .is_some_and(|source| source.request == request)
                {
                    self.browse.message = None;
                } else {
                    self.browse.message = self.error.clone();
                }
                ctx.request_repaint();
            }
        }
        if let Some((request, ticket, result)) = self.browse.completed.take() {
            if self
                .browse
                .flight
                .as_ref()
                .is_some_and(|(id, _)| *id == request)
            {
                self.browse.flight = None;
            }
            if !self.foreign_modal_owns_input(ctx)
                && self.browse.desired.as_ref() == Some(&ticket)
                && self.browse_ticket_current(&ticket)
            {
                ctx.request_repaint();
                self.browse.preview = Some(match result {
                    Ok(Payload::File { path, text, revision }) if path == ticket.row.path() && valid_preview_path(&path) && text.len() <= MAX_FILE_BYTES
                        && !text.contains('\0') && revision.len() == 64 && revision.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)) => {
                            self.cjk_seen |= crate::system_fonts::contains_cjk(&text);
                            Preview::Disk { ticket, text, revision }
                        },
                    Ok(_) => Preview::Message { ticket, message: "Preview refused an invalid, binary, oversized or mismatched Read response.".into() },
                    Err(message) => Preview::Message { ticket, message },
                });
            }
        }
        if self
            .browse
            .desired
            .as_ref()
            .is_some_and(|ticket| !self.browse_ticket_current(ticket))
        {
            self.browse.desired = None;
            self.browse.preview = None;
            self.browse.replacement = None;
        }
        if self.browse.flight.as_ref().is_some_and(|(id, ticket)| {
            ticket.context.generation != self.generation || !self.pending.contains_key(id)
        }) {
            self.browse.flight = None;
        }
        if self.browse.flight.is_none() {
            if let Some(ticket) = self.browse.replacement.take() {
                if !self.foreign_modal_owns_input(ctx)
                    && self.browse.desired.as_ref() == Some(&ticket)
                    && self.browse_ticket_current(&ticket)
                    && self.ready()
                {
                    if self.next_request == 0 || self.next_request == u64::MAX {
                        self.browse.preview = Some(Preview::Message { ticket, message: "Preview cannot read: the request identity counter is exhausted. Reopen Cedar to start a new session.".into() });
                        ctx.request_repaint();
                        return;
                    }
                    let request = self.request(
                        Operation::Read {
                            path: ticket.row.path().to_owned(),
                        },
                        Job::BrowsePreviewRead {
                            ticket: ticket.clone(),
                        },
                    );
                    if request != 0 {
                        self.browse.flight = Some((request, ticket));
                    }
                }
            }
        }
    }
    pub(super) fn browse_begin_frame(&mut self, ctx: &egui::Context) {
        if self.browse_marker_open() && !self.browse_current() {
            self.dismiss_navigation();
        }
        if self.browse.restore.as_ref().is_some_and(|context| {
            !context.current(self)
                || self.browse.input.competing
                || self.foreign_modal_owns_input(ctx)
        }) {
            self.browse.restore = None;
        }
        if !self.navigation.blocks_editor() {
            if let Some(context) = self.browse.restore.take() {
                if let Some((document, _)) = context.document {
                    ctx.memory_mut(|memory| {
                        memory.request_focus(egui::Id::new(("editor", document)))
                    });
                }
            }
        } else if self.browse.restore.is_some() {
            ctx.request_repaint();
        }
    }

    pub(super) fn find_preview_identity(&self) -> Option<PreviewTicket> {
        self.browse.desired.clone()
    }

    fn browse_row_click(&self, token: Token, id: egui::Id) -> RowClick {
        RowClick {
            token,
            id,
            context: Context::capture(self),
            provider: self.browse.provider,
            query: self.browse.query.clone(),
            scope: self.browse.scope.clone(),
            pane: self.find_preview_identity(),
        }
    }

    // Capture from the same original native batch as the legacy surfaces. Never
    // infer clean ownership from events another component already removed.
    pub(super) fn capture_browse_input(
        &mut self,
        ctx: &egui::Context,
        raw: &egui::RawInput,
        consumed: &mut [bool],
    ) {
        self.capture_context_reveal_input(ctx, raw);
        let allowed = self.browse_current() && !self.foreign_modal_owns_input(ctx);
        let focused = ctx.memory(|memory| memory.focused());
        let prior_row = self.browse.input.completed_row.clone().filter(|prior| {
            // egui may temporarily clear keyboard focus during a real owned
            // pointer press. Only that exact clean carried gesture may bridge
            // None; idle focus loss or another focused widget still retires it.
            let focus_current = allowed
                && (focused == Some(prior.id)
                    || (focused.is_none()
                        && self.browse.input.pressed_row_eligible
                        && self.browse.input.pressed.as_ref().is_some_and(|press| {
                            press.token == prior.token && press.id == prior.id
                        })
                        && ctx.input(|input| input.pointer.primary_down())
                        && ctx
                            .read_response(prior.id)
                            .is_some_and(|response| response.is_pointer_button_down_on())));
            focus_current
                && prior.token == self.browse.token(prior.token.target.clone())
                && *prior == self.browse_row_click(prior.token.clone(), prior.id)
                && prior
                    .token
                    .row
                    .as_ref()
                    .is_some_and(|row| self.browse_row_current(row))
        });
        self.browse.input.files_ime_commit = false;
        self.browse.input.files_ime_cancel = false;
        self.browse.input.files_ime_owner_stable = false;
        self.browse.input.files_ime_token = None;
        self.browse.input.files_ime_observed = false;
        self.browse.input.files_native_commit = false;
        self.browse.input.files_ime_batch =
            raw.events.iter().any(|e| matches!(e, egui::Event::Ime(_)));
        self.browse.input.files_plain_edit = false;
        self.browse.input.files_edit = None;
        if let Some(owner) = self
            .browse
            .files_composition
            .as_ref()
            .map(|c| c.owner.clone())
        {
            if focused != Some(self.browse.id(&owner)) {
                self.browse.files_composition.as_mut().unwrap().valid = false;
            }
        }
        if allowed && self.browse.provider == Provider::Files {
            self.browse.input.files_plain_edit = raw
                .events
                .iter()
                .any(|event| matches!(event, egui::Event::Text(_) | egui::Event::Paste(_)));
            // Conservative edit intent from the original owned batch, never
            // replayed. Even a final-same Text/Paste is a fresh intent; a no-op
            // Delete/Backspace alone is not. Widgets consume this once.
            if raw.events.iter().any(|event| match event {
                egui::Event::Text(_) | egui::Event::Paste(_) => true,
                egui::Event::Ime(egui::ImeEvent::Commit(text)) => {
                    // Active composition commits belong only to their original
                    // control. Do not turn a late terminal into a destination edit.
                    self.browse.files_composition.is_none()
                        && !self.browse.files_handoff_barrier
                        && !text.is_empty()
                        && text != "\n"
                        && text != "\r"
                }
                _ => false,
            }) {
                self.browse.input.files_edit = self
                    .browse
                    .input
                    .presented
                    .iter()
                    .find(|item| {
                        Some(item.id) == focused
                            && matches!(item.token.target, Target::Query | Target::Scope)
                            && item.token == self.browse.token(item.token.target.clone())
                    })
                    .map(|item| item.token.target.clone());
            }
            let owner = [Target::Query, Target::Scope]
                .into_iter()
                .find(|target| focused == Some(self.browse.id(target)));
            let lifecycle = raw.events.iter().any(|event| {
                matches!(
                    event,
                    egui::Event::Ime(egui::ImeEvent::Enabled | egui::ImeEvent::Preedit(_))
                )
            });
            if let Some(owner) = owner.filter(|owner| {
                lifecycle
                    && self
                        .browse
                        .files_composition
                        .as_ref()
                        .is_none_or(|c| c.owner != *owner || !c.valid)
            }) {
                self.browse.files_composition = Some(FilesComposition { owner, valid: true });
                self.browse.files_handoff_barrier = false;
                self.browse_query_changed();
            }
            if self.browse.files_composition.is_some() {
                // Preserve order: a new preedit after a commit makes the
                // final buffer uncommitted again, even if Disabled follows it.
                for event in &raw.events {
                    match event {
                        egui::Event::Ime(egui::ImeEvent::Enabled | egui::ImeEvent::Preedit(_)) => {
                            self.browse.input.files_ime_commit = false;
                        }
                        egui::Event::Ime(egui::ImeEvent::Commit(text)) => {
                            self.browse.input.files_ime_commit =
                                !text.is_empty() && text != "\n" && text != "\r";
                            self.browse.input.files_ime_cancel = true;
                        }
                        egui::Event::Ime(egui::ImeEvent::Disabled) => {
                            self.browse.input.files_ime_cancel = true;
                        }
                        _ => {}
                    }
                }
                if !raw.focused
                    || raw
                        .events
                        .iter()
                        .any(|event| matches!(event, egui::Event::WindowFocused(false)))
                {
                    self.browse.input.files_ime_commit = false;
                    self.browse.input.files_ime_cancel = true;
                }
            }
        }
        if allowed && self.browse.provider == Provider::Files {
            self.browse.input.files_ime_token = [Target::Query, Target::Scope]
                .into_iter()
                .find(|target| focused == Some(self.browse.id(target)))
                .map(|target| self.browse.token(target));
        }
        let input = &mut self.browse.input;
        input.completed_row = prior_row.clone();
        input.prior_row = prior_row;
        input.accepted_row = None;
        input.row_click_eligible = true;
        input.row_click_committed = false;
        input.pass = Some(ctx.cumulative_pass_nr());
        input.key = None;
        input.case_attempted = false;
        input.click = None;
        input.shortcut = None;
        input.global_edges.clear();
        input.global_overflow = false;
        input.retained_events = None;
        input.competing = raw.events.iter().any(|event| {
            matches!(
                event,
                egui::Event::Key { pressed: true, .. }
                    | egui::Event::Text(_)
                    | egui::Event::Paste(_)
                    | egui::Event::PointerButton { .. }
                    | egui::Event::WindowFocused(false)
                    | egui::Event::Ime(egui::ImeEvent::Preedit(_) | egui::ImeEvent::Commit(_))
            )
        });
        if !raw.focused
            || raw
                .events
                .iter()
                .any(|event| matches!(event, egui::Event::WindowFocused(false)))
        {
            input.retire();
            input.held.clear();
            input.claimed.clear();
            input.case_claimed.clear();
            input.case_composing = false;
            input.case_ime_blocked = false;
            input.composing = false;
            return;
        }
        let was_composing = input.composing;
        let was_case_composing = input.case_composing;
        for event in &raw.events {
            // Preserve the established generic Browse composition policy.
            match event {
                egui::Event::Ime(egui::ImeEvent::Preedit(_)) => input.composing = true,
                egui::Event::Ime(egui::ImeEvent::Commit(_) | egui::ImeEvent::Disabled) => {
                    input.composing = false
                }
                _ => {}
            }
            // Match case owns a stricter fence from Enabled, even before the
            // first Preedit. This must not alter Files or other controls.
            match event {
                egui::Event::Ime(egui::ImeEvent::Enabled | egui::ImeEvent::Preedit(_)) => {
                    input.case_composing = true
                }
                egui::Event::Ime(egui::ImeEvent::Commit(_) | egui::ImeEvent::Disabled) => {
                    input.case_composing = false
                }
                _ => {}
            }
        }
        input.case_ime_blocked = was_case_composing
            || input.case_composing
            || raw.events.iter().any(|event| {
                matches!(
                    event,
                    egui::Event::Ime(
                        egui::ImeEvent::Enabled
                            | egui::ImeEvent::Preedit(_)
                            | egui::ImeEvent::Commit(_)
                    )
                )
            });
        if input.case_ime_blocked
            && input
                .pressed
                .as_ref()
                .is_some_and(|press| press.token.target == Target::MatchCase)
        {
            input.pressed = None;
            input.pressed_row_eligible = false;
        }
        let source = allowed
            .then(|| {
                input
                    .presented
                    .iter()
                    .find(|item| Some(item.id) == focused)
                    .cloned()
            })
            .flatten();
        let clean = clean_press(&raw.events);
        let fresh_batch = !was_composing
            && !input.composing
            && input.held.is_empty()
            && !ctx.input(|state| state.pointer.any_down() || !state.keys_down.is_empty());
        let pointer_only = pointer_only(&raw.events);
        if !allowed
            || !pointer_only
            || was_composing
            || input.composing
            || !input.held.is_empty()
            || !raw.modifiers.is_none()
            || ctx.input(|state| !state.keys_down.is_empty())
        {
            input.pressed = None;
            input.pressed_row_eligible = false;
            input.break_row_continuity();
            input.row_click_eligible = false;
        } else {
            // A carried release followed by a fresh press is a normal
            // coalesced double-click. Duplicate edges or more than one completed
            // gesture cannot prove clean continuity in this original batch.
            let edges: Vec<_> = raw
                .events
                .iter()
                .filter_map(|event| match event {
                    egui::Event::PointerButton { pressed, .. } => Some(*pressed),
                    _ => None,
                })
                .collect();
            if edges.len() > 2
                || (edges.len() == 2 && edges[0] == edges[1])
                || (edges.first() == Some(&true) && input.pressed.is_some())
                || (edges.first() == Some(&false) && input.pressed.is_none())
            {
                input.break_row_continuity();
                input.row_click_eligible = false;
            }
            for event in &raw.events {
                if let egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers,
                } = event
                {
                    if !modifiers.is_none() {
                        input.pressed = None;
                        input.pressed_row_eligible = false;
                        input.break_row_continuity();
                        input.row_click_eligible = false;
                        continue;
                    }
                    if *pressed {
                        // Preserve poison across frames until this gesture's
                        // release; a clean later batch cannot launder its press.
                        input.pressed_row_eligible = input.row_click_eligible;
                        input.pressed = input
                            .presented
                            .iter()
                            .find(|item| item.rect.intersect(item.clip).contains(*pos))
                            .filter(|item| {
                                item.token.target != Target::MatchCase || !input.case_ime_blocked
                            })
                            .cloned();
                        if !input.pressed.as_ref().is_some_and(|press| {
                            matches!(press.token.target, Target::Row(_))
                                && input.prior_row.as_ref().is_some_and(|prior| {
                                    prior.token == press.token && prior.id == press.id
                                })
                        }) {
                            input.break_row_continuity();
                        }
                    } else if let Some(press) = input.pressed.take() {
                        input.row_click_eligible &= input.pressed_row_eligible;
                        input.pressed_row_eligible = false;
                        // A rejected/disabled release must not leave an older
                        // accepted completion available for a subsequent click.
                        input.completed_row = None;
                        if press.rect.intersect(press.clip).contains(*pos)
                            && input.presented.iter().any(|item| {
                                item.token == press.token
                                    && item.id == press.id
                                    && item.rect.intersect(item.clip).contains(*pos)
                            })
                        {
                            input.click = Some(press);
                        } else {
                            input.break_row_continuity();
                            input.row_click_eligible = false;
                        }
                    } else {
                        input.break_row_continuity();
                        input.row_click_eligible = false;
                    }
                }
            }
        }
        let mut space_claimed = false;
        let mut composing = was_case_composing;
        for (index, event) in raw.events.iter().enumerate() {
            match event {
                egui::Event::Ime(egui::ImeEvent::Enabled | egui::ImeEvent::Preedit(_)) => {
                    composing = true
                }
                egui::Event::Ime(egui::ImeEvent::Commit(_) | egui::ImeEvent::Disabled) => {
                    composing = false
                }
                _ => {}
            }
            let egui::Event::Key {
                key,
                physical_key,
                pressed,
                repeat,
                modifiers,
                ..
            } = event
            else {
                continue;
            };
            let claimed = input.claimed.contains(key);
            // Global entry edges remain in the native raw batch so early
            // completion and other shortcut classifiers see original evidence.
            // Only their later dispatch copy is consumed by browse_shortcuts.
            let shortcut = shortcut_provider(*key, *modifiers);
            let global = shortcut.is_some()
                || (claimed && matches!(key, egui::Key::N | egui::Key::F | egui::Key::E));
            if global && !input.global_overflow {
                if input.global_edges.len() == MAX_GLOBAL_EDGES {
                    input.global_edges.clear();
                    input.global_overflow = true;
                } else {
                    input.global_edges.push(GlobalEdge {
                        index,
                        key: *key,
                        physical_key: *physical_key,
                        pressed: *pressed,
                        repeat: *repeat,
                        modifiers: *modifiers,
                    });
                }
            }
            if !pressed {
                input.held.remove(key);
                input.claimed.remove(key);
                input.case_claimed.remove(key);
                consumed[index] |= claimed && !global;
                continue;
            }
            let fresh = input.held.insert(*key);
            if matches!(key, egui::Key::Enter | egui::Key::Space)
                && (input.case_claimed.contains(key)
                    || source
                        .as_ref()
                        .is_some_and(|item| item.token.target == Target::MatchCase))
            {
                // Consult ownership at this exact ordered press, after any
                // preceding release. A new editor key cannot inherit old ownership.
                input.case_attempted = true;
            }
            let owns = source
                .as_ref()
                .is_some_and(|item| owns_key(&item.token.target, *key) && modifiers.is_none());
            if owns || shortcut.is_some() || claimed {
                // A repeat arriving after focus transfer never adopts another
                // surface's lifetime or turns into a new activation.
                if fresh || claimed {
                    input.claimed.insert(*key);
                }
                consumed[index] |= !global;
                let case_owns = owns
                    && source
                        .as_ref()
                        .is_some_and(|item| item.token.target == Target::MatchCase);
                if case_owns && (fresh || claimed) {
                    input.case_claimed.insert(*key);
                }
                // Consume only this owned Space's native companion. A release,
                // another key, IME event, or unrelated text ends adjacency.
                // Pointer input cannot transfer an already-owned physical key.
                if *key == egui::Key::Space && input.case_claimed.contains(key) && !composing {
                    for (consume, event) in consumed.iter_mut().zip(&raw.events).skip(index + 1) {
                        match event {
                            egui::Event::PointerMoved(_)
                            | egui::Event::MouseMoved(_)
                            | egui::Event::PointerButton { .. } => continue,
                            egui::Event::Text(text) if text == " " => *consume = true,
                            _ => {}
                        }
                        break;
                    }
                }
                space_claimed |= *key == egui::Key::Space && !input.case_claimed.contains(key);
                if fresh && !repeat && fresh_batch && clean == Some((*key, *modifiers)) {
                    if let Some(provider) = shortcut {
                        input.shortcut =
                            Some((provider, self.generation, self.workspace_key.clone()));
                    } else if let Some(source) = source.clone().filter(|_| owns) {
                        input.key = Some(KeyIntent { source, key: *key });
                    }
                }
            }
        }
        if input.global_overflow {
            input.shortcut = None;
        }
        if space_claimed && clean.is_some_and(|(key, _)| key == egui::Key::Space) {
            for (index, event) in raw.events.iter().enumerate() {
                consumed[index] |= matches!(event, egui::Event::Text(text) if text == " ");
            }
        }
    }
    pub(super) fn finish_browse_input_capture(
        &mut self,
        ctx: &egui::Context,
        events: &[egui::Event],
        consumed: &[bool],
    ) {
        // egui 0.31 derives repeat from its previous keys_down while beginning
        // the pass. Mirror that fixed metadata over the retained sequence.
        let mut keys_down = ctx.input(|input| input.keys_down.clone());
        let mut retained = 0;
        let mut next = 0;
        for (index, removed) in consumed.iter().enumerate() {
            let repeat = if !*removed {
                if let egui::Event::Key {
                    key,
                    pressed,
                    repeat,
                    ..
                } = &events[index]
                {
                    if *pressed {
                        !keys_down.insert(*key)
                    } else {
                        keys_down.remove(key);
                        *repeat
                    }
                } else {
                    false
                }
            } else {
                false
            };
            if self
                .browse
                .input
                .global_edges
                .get(next)
                .is_some_and(|edge| edge.index == index)
            {
                let edge = &mut self.browse.input.global_edges[next];
                edge.index = if *removed { usize::MAX } else { retained };
                edge.repeat = repeat;
                next += 1;
            }
            retained += usize::from(!*removed);
        }
        self.browse
            .input
            .global_edges
            .retain(|edge| edge.index != usize::MAX);
        self.browse.input.retained_events = Some(retained);
    }
    fn browse_input_current(&self, ctx: &egui::Context) -> bool {
        self.navigation.input_frame_current(ctx)
            && self.browse.input.pass
                == Some(
                    ctx.cumulative_pass_nr()
                        .saturating_sub(ctx.output(|output| output.num_completed_passes) as u64),
                )
            && ctx.input(|input| input.focused)
            && !self.foreign_modal_owns_input(ctx)
    }
    pub(super) fn browse_shortcuts(&mut self, ctx: &egui::Context) {
        let current = self.browse_input_current(ctx);
        let mut dispatch_current = false;
        if current {
            let edges = &self.browse.input.global_edges;
            let expected = self.browse.input.retained_events;
            let overflow = self.browse.input.global_overflow;
            dispatch_current = ctx.input_mut(|input| {
                // Earlier dispatch mutations invalidate positions. Never match
                // by key alone: release/repress and modifier changes are distinct.
                let valid = !overflow
                    && expected == Some(input.events.len())
                    && input.events.len() == input.raw.events.len()
                    && edges.iter().all(|edge| {
                        input
                            .events
                            .get(edge.index)
                            .is_some_and(|event| edge.matches(event))
                            && input
                                .raw
                                .events
                                .get(edge.index)
                                .is_some_and(|event| edge.matches(event))
                    });
                if !valid {
                    return false;
                }
                let mut index = 0;
                let mut next = 0;
                input.events.retain(|_| {
                    let owned = edges.get(next).is_some_and(|edge| edge.index == index);
                    if owned {
                        next += 1;
                    }
                    index += 1;
                    !owned
                });
                true
            });
        }
        #[cfg(test)]
        crate::browse_tests::record_dispatch_keys(ctx);
        if let Some((provider, generation, workspace)) = self.browse.input.shortcut.take() {
            if current
                && dispatch_current
                && generation == self.generation
                && workspace == self.workspace_key
                && !self.open_form
                && !self.new_file
            {
                match provider {
                    GlobalEntry::Browse(provider) => self.show_browse(provider),
                    GlobalEntry::Recent => self.show_recent_files(),
                }
            }
        }
    }
    fn browse_response(
        &mut self,
        ui: &egui::Ui,
        target: Target,
        response: &egui::Response,
    ) -> bool {
        let token = self.browse.token(target.clone());
        let case_identity = (target == Target::MatchCase).then(|| self.search_focus_identity());
        if !ui.is_sizing_pass() {
            self.browse.input.presented.push(Presented {
                token: token.clone(),
                case_identity: case_identity.clone(),
                id: response.id,
                rect: response.rect,
                clip: ui.clip_rect(),
            });
            if self.browse.focus.as_ref() == Some(&target) {
                response.request_focus();
                response
                    .scroll_to_me(matches!(target, Target::Row(_)).then_some(egui::Align::Center));
                self.browse.focus = None;
            }
        }
        #[cfg(test)]
        {
            let name = match &target {
                Target::Query => "browse_query".into(),
                Target::Scope => "browse_scope".into(),
                Target::TextScope => "browse_text_scope".into(),
                Target::Masks => "browse_masks".into(),
                Target::MatchCase => "browse_match_case".into(),
                Target::Files => "browse_files".into(),
                Target::Text => "browse_text".into(),
                Target::Submit => "browse_submit".into(),
                Target::Row(index) => format!("browse_row:{index}"),
                Target::Group(index) => format!("browse_group:{index}"),
                Target::Preview => "browse_preview".into(),
                Target::HidePreview => "browse_hide_preview".into(),
                Target::Open => "browse_open".into(),
                Target::Close => "browse_close".into(),
                Target::PreviewPane => "browse_preview_content".into(),
            };
            crate::workspace_access_tests::record(ui, &name, response);
            crate::browse_tests::record_row_click_response(ui, &name, response);
        }
        if !response.enabled()
            || ui.is_sizing_pass()
            || !self.browse_input_current(ui.ctx())
            || (target == Target::MatchCase && self.browse.input.case_ime_blocked)
        {
            return false;
        }
        let click = self.browse.input.click.as_ref().is_some_and(|click| {
            click.token == token && click.id == response.id && click.case_identity == case_identity
        }) && response.clicked_by(egui::PointerButton::Primary);
        let key = self.browse.input.key.as_ref().is_some_and(|intent| {
            intent.source.token == token
                && intent.source.case_identity == case_identity
                && intent.source.id == response.id
                && matches!(intent.key, egui::Key::Enter | egui::Key::Space)
                && ui.ctx().memory(|memory| memory.focused()) == Some(response.id)
        });
        if click {
            response.request_focus();
            if matches!(target, Target::Row(_))
                && self.browse.input.row_click_eligible
                && !self.browse.input.row_click_committed
            {
                self.browse.input.accepted_row = Some((token, response.id));
                self.browse.input.row_click_committed = true;
            }
        }
        click || key
    }
    fn browse_results_panel(
        &mut self,
        ui: &mut egui::Ui,
        max_height: f32,
        select: &mut Option<usize>,
        open: &mut bool,
        toggle_group: &mut Option<usize>,
    ) {
        let scroll = egui::ScrollArea::vertical().id_salt("browse_results");
        let scroll = if self.browse.provider == Provider::ProjectText {
            scroll.id_salt((
                "browse_text_results",
                self.search_state
                    .accepted
                    .as_ref()
                    .map(|source| search_keyboard::row_id(source, 0)),
            ))
        } else {
            scroll
        };
        let scroll = scroll.auto_shrink([false, false]);
        let line_width = text_result_line_width(
            ui,
            self.browse
                .rows
                .iter()
                .filter_map(|row| {
                    if let Row::Text(hit) = row {
                        Some(hit.line())
                    } else {
                        None
                    }
                })
                .max(),
        );
        let output = scroll
            .animated(false)
            .min_scrolled_height(max_height.min(64.0))
            .max_height(max_height)
            .show(ui, |ui| {
                // Group adapters preserve original accepted indexes; no snippets are copied.
                let groups = if self.browse.provider == Provider::ProjectText {
                    self.search_state.group_rows()
                } else {
                    vec![(0..self.browse.rows.len()).collect()]
                };
                for group in groups {
                    if self.browse.provider == Provider::ProjectText {
                        let Some(first) = group.first().copied() else {
                            continue;
                        };
                        let Some(row) = self.browse.rows.get(first) else {
                            continue;
                        };
                        let path = row.path().to_owned();
                        let expanded = self.search_state.group_expanded(first);
                        let response = ui
                            .add_enabled_ui(self.search_rows_active(), |ui| {
                                text_result_group(
                                    ui,
                                    self.browse.id(&Target::Group(first)),
                                    &path,
                                    group.len(),
                                    expanded,
                                )
                            })
                            .inner;
                        if self.browse_response(ui, Target::Group(first), &response) {
                            *toggle_group = Some(first);
                        }
                        if !self.search_state.group_expanded(first) {
                            continue;
                        }
                    }
                    for index in group {
                        let Some(row) = self.browse.rows.get(index).cloned() else {
                            continue;
                        };
                        let label = match &row {
                            Row::Project(candidate) => {
                                candidate.source.paths[candidate.index].clone()
                            }
                            Row::File(candidate) => self.browse_loaded_label(candidate),
                            Row::Text(_) => String::new(),
                        };
                        let target = Target::Row(index);
                        let id = self.browse.id(&target);
                        let current = self.browse_row_current(&row);
                        let response = ui
                            .add_enabled_ui(current, |ui| {
                                if self.browse.provider == Provider::Files {
                                    let annotation = match &row {
                                        Row::File(candidate) => {
                                            self.browse_loaded_annotation(candidate)
                                        }
                                        _ => "",
                                    };
                                    files_result_row(
                                        ui,
                                        id,
                                        self.browse.selected == Some(index),
                                        row.path(),
                                        annotation,
                                        &label,
                                    )
                                } else if let Row::Text(hit) = &row {
                                    text_result_hit(
                                        ui,
                                        id,
                                        self.browse.selected == Some(index),
                                        hit,
                                        self.browse_search_snippet(hit).unwrap_or(""),
                                        line_width,
                                        if current {
                                            self.search_highlight_ranges(hit.index())
                                        } else {
                                            &[]
                                        },
                                    )
                                } else {
                                    navigation::candidate_row(
                                        ui,
                                        id,
                                        self.browse.selected == Some(index),
                                        &label,
                                        true,
                                    )
                                }
                            })
                            .inner;
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
                        if self.browse_response(ui, target, &response) {
                            *select = Some(index);
                            if response.double_clicked_by(egui::PointerButton::Primary)
                                && self.browse.input.row_click_eligible
                                && self.browse.input.prior_row.as_ref().is_some_and(|prior| {
                                    *prior
                                        == self.browse_row_click(
                                            self.browse.token(Target::Row(index)),
                                            response.id,
                                        )
                                })
                            {
                                *open = true;
                            }
                        }
                    }
                }
                if self.browse.rows.is_empty()
                    && (self.browse.provider == Provider::ProjectText
                        || (!self.browse.discovery.pending_current()
                            && self.browse.files_settlement.is_none()
                            && self.browse.message.is_none()))
                {
                    let empty = if self.browse.provider == Provider::ProjectText {
                        if self.search_state.pending.is_some() || self.search_busy() {
                            "Waiting for search results…"
                        } else if self.search_state.accepted.is_some() && self.search_rows_active()
                        {
                            if self.search_truncated {
                                "No matching lines observed in an incomplete scan; more may exist."
                            } else {
                                "No matching lines in the searched scope."
                            }
                        } else {
                            "Submit a literal query to search files."
                        }
                    } else {
                        match &self.browse.discovery.accepted {
                            Some(source) if source.incomplete => {
                                "No matches observed in an incomplete scan; more files may exist."
                            }
                            Some(_) => {
                                "No matching project files in the searched scope and exclusions."
                            }
                            None => "No open or loaded files. Type a filename to search.",
                        }
                    };
                    ui.label(RichText::new(empty).color(MUTED));
                }
            });
        #[cfg(test)]
        crate::browse_tests::record_results_scroll(
            ui.ctx(),
            output.id,
            output.inner_rect,
            output.content_size,
        );
        #[cfg(not(test))]
        let _ = output;
    }
    fn browse_preview_title(&self, scrolling: bool) -> String {
        let mut title = match &self.browse.preview {
            Some(Preview::Draft(_)) => "Current open draft · read-only preview".to_owned(),
            Some(Preview::Disk { revision, .. }) => format!(
                "Disk preview · revision {} · search coordinates may be historical",
                &revision[..8]
            ),
            Some(Preview::Message { message, .. }) => message.clone(),
            None => "Preview requested…".to_owned(),
        };
        if scrolling {
            title.push_str(" · scroll for more");
        }
        title
    }

    fn text_action(
        &mut self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        target: Target,
        label: &str,
        enabled: bool,
    ) -> bool {
        let response = ui
            .add_enabled_ui(enabled, |ui| {
                if target == Target::Text {
                    text_tab_control(ui, rect, self.browse.id(&target), label)
                } else {
                    files_action_control(ui, rect, self.browse.id(&target), label, false)
                }
            })
            .inner;
        let response = if target == Target::Text {
            response.on_hover_text(format!(
                "Project Text: explicitly submitted, {} literal disk search",
                search_keyboard::case_mode(self.search_match_case).to_lowercase()
            ))
        } else {
            response
        };
        self.browse_response(ui, target, &response)
    }

    fn browse_text_count(&self) -> Option<String> {
        self.search_state.accepted.as_ref().map(|_| {
            let lines = self.search_results.len();
            let files = self.search_state.group_rows().len();
            text_matching_line_count(lines, files, self.search_truncated)
        })
    }

    fn browse_text_surface(&mut self, ui: &mut egui::Ui) -> TextActions {
        let mut actions = TextActions::default();
        let screen = ui.ctx().screen_rect();
        let width = (screen.width() - 64.0).clamp(1.0, 860.0);
        ui.set_width(width);
        let top = ui.next_widget_position();
        let gap = 4.0;
        let body_line = text_line_height(ui, "Ag", egui::TextStyle::Body);
        let button_line = text_line_height(ui, "Ag", egui::TextStyle::Button);
        let control_height = (button_line + 10.0)
            .max(30.0)
            .max(ui.spacing().interact_size.y);
        let field_height =
            (body_line + 2.0 * ui.spacing().button_padding.y).max(ui.spacing().interact_size.y);
        let filter_rows = if width >= search_keyboard::filter_controls_width(ui) {
            1.0
        } else {
            2.0
        };
        let filters_height = filter_rows * field_height + (filter_rows - 1.0) * gap;
        let status_height = 2.0 * body_line + gap;
        let standard_row =
            (button_line.max(text_line_height(ui, "Ag", egui::TextStyle::Monospace)) + 6.0)
                .max(ui.spacing().interact_size.y);
        let chrome_height = 3.0 * control_height + filters_height + status_height + 5.0 * gap;
        let normal_height = chrome_height + 14.0 * standard_row + 13.0 * gap;
        let bottom_margin = egui::Frame::popup(ui.style()).total_margin().bottom;
        let height = (screen.bottom() - top.y - 24.0 - bottom_margin).clamp(1.0, normal_height);
        let surface = egui::Rect::from_min_size(top, egui::vec2(width, height));
        ui.set_min_size(surface.size());
        let band = |y, h| egui::Rect::from_min_size(egui::pos2(top.x, y), egui::vec2(width, h));
        let header = band(top.y, control_height);
        let files_width = files_text_width(ui, "Files") + 20.0;
        let text_width = files_text_width(ui, "Find in Files") + 20.0;
        let close_width = files_text_width(ui, "Close · Esc") + 16.0;
        let files_rect =
            egui::Rect::from_min_size(header.min, egui::vec2(files_width, control_height));
        let text_rect = egui::Rect::from_min_size(
            egui::pos2(files_rect.right() + gap, top.y),
            egui::vec2(text_width, control_height),
        );
        let close_rect =
            egui::Rect::from_min_max(egui::pos2(header.right() - close_width, top.y), header.max);
        if self.text_action(ui, files_rect, Target::Files, "Files", true) {
            actions.switch = Some(Provider::Files);
        }
        if self.text_action(
            ui,
            text_rect,
            Target::Text,
            "Find in Files",
            self.backend_supports("search")
                || self.backend_scoped_search_supported()
                || self.backend_case_search_supported(),
        ) {
            actions.switch = Some(Provider::ProjectText);
        }
        actions.close = self.text_action(ui, close_rect, Target::Close, "Close · Esc", true);
        let count_rect = egui::Rect::from_min_max(
            egui::pos2(text_rect.right() + 12.0, header.top()),
            egui::pos2(
                (close_rect.left() - 12.0).max(text_rect.right() + 12.0),
                header.bottom(),
            ),
        );
        let query_band = band(header.bottom() + gap, control_height);
        let find_width = files_text_width(ui, "Find") + 24.0;
        let find_rect = egui::Rect::from_min_max(
            egui::pos2(query_band.right() - find_width, query_band.top()),
            query_band.max,
        );
        // Reserve measured horizontal space without stealing an extra row
        // from results or preview. Creation order below controls native Tab.
        let case_size = search_keyboard::match_case_size(ui);
        let case_rect = egui::Rect::from_min_size(
            egui::pos2(
                find_rect.left() - gap - case_size.x,
                query_band.center().y - case_size.y * 0.5,
            ),
            case_size,
        );
        let query_rect = egui::Rect::from_min_max(
            query_band.min,
            egui::pos2(
                (case_rect.left() - gap).max(query_band.left()),
                query_band.bottom(),
            ),
        );
        let query_id = self.browse.id(&Target::Query);
        if self.browse.focus == Some(Target::Query) && !ui.is_sizing_pass() {
            ui.memory_mut(|memory| memory.request_focus(query_id));
        }
        let query = ui.put(
            query_rect,
            egui::TextEdit::singleline(&mut self.browse.query)
                .id(query_id)
                .char_limit(4096)
                .desired_width(f32::INFINITY)
                .hint_text("Enter literal text, then press Enter"),
        );
        if query.changed() {
            self.browse_query_changed();
            actions.edited = true;
        }
        self.browse_response(ui, Target::Query, &query);
        let filters_rect = band(
            query_band.bottom().max(query.rect.bottom()) + gap,
            filters_height,
        );
        let mut filters_ui = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("text_filters")
                .max_rect(filters_rect),
        );
        filters_ui.set_clip_rect(ui.clip_rect().intersect(filters_rect));
        let (scope, masks) = self.search_compact_filter_controls(
            &mut filters_ui,
            self.browse.id(&Target::TextScope),
            self.browse.id(&Target::Masks),
        );
        if scope.changed() || masks.changed() {
            self.browse_query_changed();
            actions.edited = true;
        }
        self.browse_response(&filters_ui, Target::TextScope, &scope);
        self.browse_response(&filters_ui, Target::Masks, &masks);
        let mut match_case =
            search_keyboard::match_case_response(ui, case_rect, self.browse.id(&Target::MatchCase));
        if self.browse_response(ui, Target::MatchCase, &match_case)
            && self.search_case_draft_available()
        {
            self.search_match_case = !self.search_match_case;
            self.browse_query_changed();
            match_case.mark_changed();
            actions.case_changed = true;
            ui.ctx().request_repaint();
        }
        search_keyboard::paint_match_case(ui, &match_case, self.search_match_case);
        match_case.on_hover_text_at_pointer(search_keyboard::MATCH_CASE_HINT);
        // Native creation order remains query → directory → masks → Match case → submit.
        actions.submit = self.text_action(
            ui,
            find_rect,
            Target::Submit,
            "Find",
            self.search_submission_supported() && !self.search_busy(),
        );

        let count = self.browse_text_count();
        let active = self.search_rows_active();
        let header_count = count.as_ref().map(|count| {
            if active {
                count.clone()
            } else {
                format!("Previous · {count}")
            }
        });
        let count_in_header = header_count
            .as_ref()
            .is_some_and(|text| files_text_width(ui, text) <= count_rect.width());
        if count_in_header {
            files_label_at(
                ui,
                count_rect,
                egui::Label::new(
                    RichText::new(header_count.as_ref().unwrap()).color(chrome::MUTED),
                )
                .truncate(),
            )
            .on_hover_text(format!(
                "Project Text · {} literal disk search. Workspace: {}",
                search_keyboard::case_mode(
                    self.search_state
                        .accepted
                        .as_ref()
                        .map_or(self.search_match_case, |source| source
                            .specification
                            .case_sensitive)
                )
                .to_lowercase(),
                self.root
            ));
        } else {
            let workspace = self
                .root
                .trim_end_matches(['/', '\\'])
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or(&self.root);
            files_label_at(
                ui,
                count_rect,
                egui::Label::new(RichText::new(workspace).color(chrome::MUTED)).truncate(),
            )
            .on_hover_text(format!("Project Text · Workspace: {}", self.root));
        }
        let footer = band(surface.bottom() - control_height, control_height);
        let status_rect = band(filters_rect.bottom() + gap, status_height);
        let explanation = self.search_filter_explanation();
        let headline = if self.search_state.pending.is_some() {
            "Searching…".to_owned()
        } else if self.search_busy() {
            "Waiting for previous search to finish".to_owned()
        } else if let Some(explanation) = &explanation {
            format!("Invalid or unavailable filters: {explanation}")
        } else if self.search_state.accepted.is_some() && !active {
            format!(
                "Previous results are inactive. {}",
                self.browse
                    .message
                    .as_deref()
                    .unwrap_or("Run Find for the current query, filters and connection.")
            )
        } else if let Some(message) = &self.browse.message {
            message.clone()
        } else if self.search_truncated || !count_in_header {
            count
                .clone()
                .unwrap_or_else(|| "Enter literal text, then press Enter or Find".into())
        } else {
            format!(
                "{} · Literal · Disk search",
                search_keyboard::case_mode(self.search_match_case)
            )
        };
        // Keep the current conclusion outside the scrolling details. A wheel
        // gesture may still have smooth input after a query/reply changes; its
        // tail must never move Searching, Waiting or inactive/error headlines.
        let headline_rect = band(status_rect.top(), body_line);
        files_label_at(
            ui,
            headline_rect,
            egui::Label::new(RichText::new(&headline).color(
                if explanation.is_some()
                    || self.browse.message.is_some()
                    || self.search_truncated
                    || (self.search_state.accepted.is_some() && !active)
                {
                    AMBER
                } else {
                    chrome::MUTED
                },
            ))
            .truncate(),
        )
        .on_hover_text(&headline);
        // Reuse the inter-line gap as scroll viewport slack: a one-glyph-high
        // viewport clips both edges at fractional native wheel offsets. Keep
        // the same total status/body/footer budget while allowing a complete
        // details row to remain readable without exact pixel alignment.
        let details_rect = egui::Rect::from_min_max(
            egui::pos2(status_rect.left(), headline_rect.bottom()),
            status_rect.max,
        );
        let mut status_ui = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("text_status")
                .max_rect(details_rect),
        );
        status_ui.set_clip_rect(ui.clip_rect().intersect(details_rect));
        // Keep overflow discoverable even before pointer hover. This child-only
        // opacity change leaves the Files and bottom Find scrollbar styles alone.
        status_ui.spacing_mut().scroll.dormant_handle_opacity = 0.65;
        #[cfg(test)]
        ui.ctx().data_mut(|data| {
            data.insert_temp(egui::Id::new("text_status_rect_probe"), status_rect);
            data.insert_temp(egui::Id::new("text_status_headline_probe"), headline_rect);
        });
        // A scroll position belongs to the status currently being explained,
        // not to later edits or replies. Keep one bounded presentation key and
        // one stable scroll ID; result-list scroll identity remains untouched.
        let status_identity = egui::Id::new((
            &headline,
            &self.browse.query,
            &self.search_scope,
            &self.search_masks,
            self.search_match_case,
            &self.root,
            self.generation,
            self.search_state
                .accepted
                .as_ref()
                .map(|source| search_keyboard::row_id(source, 0)),
            self.search_state
                .pending
                .as_ref()
                .map(|source| search_keyboard::row_id(source, 0)),
        ));
        let status_key = egui::Id::new("text_status_content_identity");
        let reset_status = ui
            .ctx()
            .data(|data| data.get_temp::<egui::Id>(status_key) != Some(status_identity));
        let visible = !ui.is_sizing_pass() && ui.is_visible() && !ui.ctx().will_discard();
        if visible {
            ui.ctx()
                .data_mut(|data| data.insert_temp(status_key, status_identity));
        }
        let status_scroll = egui::ScrollArea::vertical()
            .id_salt("text_status_scroll")
            .animated(false)
            .auto_shrink([false, false])
            .min_scrolled_height(0.0)
            .max_height(details_rect.height());
        let status_scroll = if reset_status {
            status_scroll.vertical_scroll_offset(0.0)
        } else {
            status_scroll
        };
        let status_scroll = status_scroll.show(&mut status_ui, |ui| {
            ui.add_space(gap / 2.0);
            if let Some(source) = &self.search_state.accepted {
                let history = format!("Historical results for {} · {}. Lines may have changed; Open uses current source.", source.specification.description(), count.as_deref().unwrap_or(""));
                let label = ui.add(egui::Label::new(RichText::new(&history).color(chrome::MUTED)).truncate()).on_hover_text(history);
                #[cfg(test)] crate::workspace_access_tests::record(ui, "browse_history", &label);
                #[cfg(not(test))] let _ = label;
            } else {
                ui.label(RichText::new(format!("{} · Literal · Disk search", search_keyboard::case_mode(self.search_match_case))).color(chrome::MUTED));
            }
            // Long actionable details remain readable in the bounded scrolling
            // region; tooltips are supplementary, never the only error surface.
            if self.search_state.accepted.is_some() && !active { ui.colored_label(AMBER, "Previous results are inactive. Run Find for the current query, filters and connection."); }
            if let Some(explanation) = explanation { ui.colored_label(AMBER, explanation); }
            if let Some(message) = &self.browse.message { ui.colored_label(AMBER, message); }
            if let Some(description) = self.search_scan_description() {
                let prefix = if active { "Accepted scan" } else { "Previous accepted scan" };
                ui.label(RichText::new(format!("{prefix}: {description}")).color(chrome::MUTED));
            }
            if self.search_truncated { ui.colored_label(AMBER, "Search incomplete; more matching lines may exist."); }
        });
        #[cfg(test)]
        crate::browse_tests::record_text_status_scroll(
            ui.ctx(),
            status_scroll.id,
            status_scroll.inner_rect,
            status_scroll.content_size,
        );
        #[cfg(not(test))]
        let _ = status_scroll;
        let body = egui::Rect::from_min_max(
            egui::pos2(top.x, status_rect.bottom() + gap),
            egui::pos2(
                surface.right(),
                (footer.top() - gap).max(status_rect.bottom() + gap),
            ),
        )
        .round_to_pixels(ui.pixels_per_point());
        let preview_height = if self.browse.desired.is_some() {
            let row_height = self
                .browse
                .rows
                .iter()
                .map(|row| {
                    let path_height = text_line_height(ui, row.path(), egui::TextStyle::Button);
                    let snippet_height = if let Row::Text(hit) = row {
                        text_line_height(
                            ui,
                            self.browse_search_snippet(hit).unwrap_or(""),
                            egui::TextStyle::Monospace,
                        )
                    } else {
                        0.0
                    };
                    (path_height.max(snippet_height) + 6.0).max(ui.spacing().interact_size.y)
                })
                .fold(standard_row, f32::max);
            let code_line = self
                .current_project_text_context(PreviewSurface::Browse)
                .as_ref()
                .map(|context| {
                    context
                        .text
                        .lines()
                        .map(|line| text_line_height(ui, line, egui::TextStyle::Monospace))
                        .fold(0.0, f32::max)
                })
                .unwrap_or_else(|| text_line_height(ui, "Ag", egui::TextStyle::Monospace));
            let preview_chrome =
                control_height + 2.0 * body_line + gap + ui.spacing().scroll.allocated_width();
            // A centered reveal can leave partial lines at both edges. Reserve
            // one extra measured glyph row so three (or six when space allows)
            // remain complete after the native scroll offset settles.
            let roomy_results = 6.0 * row_height + 5.0 * gap;
            let roomy_preview = preview_chrome + 7.0 * code_line;
            let (results_min, preview_min) = if body.height() >= roomy_results + roomy_preview {
                (roomy_results, roomy_preview)
            } else {
                (
                    3.0 * row_height + 2.0 * gap,
                    preview_chrome + 4.0 * code_line,
                )
            };
            if body.height() >= results_min + preview_min {
                (body.height() * 0.45).clamp(preview_min, body.height() - results_min)
            } else {
                body.height() * 0.5
            }
        } else {
            0.0
        };
        let results_rect = egui::Rect::from_min_max(
            body.min,
            egui::pos2(
                body.right(),
                (body.bottom() - preview_height).round_to_pixels(ui.pixels_per_point()),
            ),
        );
        let mut results_ui = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("text_results_panel")
                .max_rect(results_rect),
        );
        results_ui.set_clip_rect(ui.clip_rect().intersect(results_rect));
        self.browse_results_panel(
            &mut results_ui,
            results_rect.height(),
            &mut actions.select,
            &mut actions.open,
            &mut actions.toggle_group,
        );
        let preview_rect = self.browse.desired.as_ref().map(|_| {
            egui::Rect::from_min_max(egui::pos2(body.left(), results_rect.bottom()), body.max)
        });
        if let Some(rect) = preview_rect {
            actions.hide_preview = self.browse_text_preview(ui, rect, control_height, body_line);
        }
        ui.painter().hline(
            surface.x_range(),
            footer.top() - gap / 2.0,
            egui::Stroke::new(1.0_f32, chrome::BORDER),
        );
        let selected = self
            .browse
            .selected
            .filter(|index| self.browse_text_visible(*index))
            .and_then(|index| self.browse.rows.get(index))
            .is_some_and(|row| self.browse_row_current(row));
        let preview_width = files_text_width(ui, "Preview · Space") + 16.0;
        let open_width = files_text_width(ui, "Open · Enter") + 16.0;
        let preview_button =
            egui::Rect::from_min_size(footer.min, egui::vec2(preview_width, control_height));
        let open_button = egui::Rect::from_min_size(
            egui::pos2(preview_button.right() + gap, footer.top()),
            egui::vec2(open_width, control_height),
        );
        actions.preview = self.text_action(
            ui,
            preview_button,
            Target::Preview,
            "Preview · Space",
            selected,
        );
        actions.open |= self.text_action(ui, open_button, Target::Open, "Open · Enter", selected);
        let hint_rect = egui::Rect::from_min_max(
            egui::pos2(
                (open_button.right() + 12.0).min(footer.right()),
                footer.top(),
            ),
            footer.max,
        );
        let hint = if self.browse.review.is_some() {
            "Follows selection · Esc stops"
        } else {
            "Space starts preview"
        };
        files_label_at(ui, hint_rect, egui::Label::new(RichText::new(hint).color(chrome::MUTED)).truncate()).on_hover_text("Read-only context. Space starts preview for the selected result; it then follows selection after 150 ms. Escape stops preview and returns to the same result; Escape again closes Find in Files.");
        #[cfg(test)]
        let visible = !ui.is_sizing_pass() && ui.is_visible();
        #[cfg(test)]
        ui.ctx().data_mut(|data| {
            data.insert_temp(egui::Id::new("text_surface_visible_probe"), visible);
            data.insert_temp(egui::Id::new("text_surface_rect_probe"), surface);
            data.insert_temp(egui::Id::new("text_body_rect_probe"), body);
            data.insert_temp(egui::Id::new("text_preview_rect_probe"), preview_rect);
        });
        actions
    }

    fn browse_text_preview(
        &mut self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        header_height: f32,
        body_line: f32,
    ) -> bool {
        ui.painter().hline(
            rect.x_range(),
            rect.top(),
            egui::Stroke::new(1.0_f32, chrome::BORDER),
        );
        let header = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), header_height));
        let hide_width = files_text_width(ui, "Hide preview") + 16.0;
        let hide_rect = egui::Rect::from_min_max(
            egui::pos2(header.right() - hide_width, header.top()),
            header.max,
        );
        let hide = self.text_action(ui, hide_rect, Target::HidePreview, "Hide preview", true);
        let title_rect = egui::Rect::from_min_max(
            header.min,
            egui::pos2((hide_rect.left() - 8.0).max(header.left()), header.bottom()),
        );
        let path = self
            .browse
            .desired
            .as_ref()
            .map(|ticket| ticket.row.path())
            .unwrap_or("");
        let source = match &self.browse.preview {
            Some(Preview::Draft(_)) => "Current open draft · read-only",
            Some(Preview::Disk { .. }) => "Disk · read-only",
            Some(Preview::Message { .. }) => "Preview unavailable",
            None => "Preview requested… · read-only",
        };
        files_label_at(
            ui,
            title_rect,
            egui::Label::new(RichText::new(format!("{source} · {path}")).color(chrome::MUTED))
                .truncate(),
        )
        .on_hover_text(format!("{path}\n{}", self.browse_preview_title(false)));
        let context = self.current_project_text_context(PreviewSurface::Browse);
        let note_rect = egui::Rect::from_min_max(
            egui::pos2(rect.left(), header.bottom()),
            egui::pos2(
                rect.right(),
                (header.bottom() + 2.0 * body_line).min(rect.bottom()),
            ),
        );
        let note = context
            .as_ref()
            .map(|context| context.note.as_str())
            .unwrap_or("Historical search coordinates may have changed; preview is read-only.");
        let mut note_ui = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("text_preview_note")
                .max_rect(note_rect),
        );
        note_ui.set_clip_rect(ui.clip_rect().intersect(note_rect));
        note_ui
            .label(RichText::new(note).color(chrome::MUTED))
            .on_hover_text(note);
        let content_rect = egui::Rect::from_min_max(
            egui::pos2(rect.left(), (note_rect.bottom() + 4.0).min(rect.bottom())),
            rect.max,
        )
        .round_to_pixels(ui.pixels_per_point());
        let mut content_ui = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("text_preview_panel")
                .max_rect(content_rect),
        );
        content_ui.set_clip_rect(ui.clip_rect().intersect(content_rect));
        let reveal = self.take_context_reveal(&content_ui, PreviewSurface::Browse);
        let id = self.browse.id(&Target::PreviewPane);
        let scroll = egui::ScrollArea::both()
            .id_salt("browse_preview_scroll")
            .animated(false)
            .auto_shrink([false, false])
            .min_scrolled_height(0.0)
            .max_height(content_rect.height())
            .show(&mut content_ui, |ui| {
                let content = if let Some(context) = &context {
                    draw_project_text_context(ui, context, reveal)
                } else if let Some(Preview::Message { message, .. }) = &self.browse.preview {
                    ui.label(RichText::new(message).color(AMBER))
                } else {
                    ui.label(RichText::new("Waiting for preview…").color(chrome::MUTED))
                };
                ui.interact(content.rect, id, egui::Sense::click())
            });
        #[cfg(test)]
        record_context_scroll(
            ui.ctx(),
            scroll.id,
            scroll.inner_rect,
            PreviewSurface::Browse,
        );
        self.browse_response(&content_ui, Target::PreviewPane, &scroll.inner);
        hide
    }
    // Both Escape and the explicit Files close control retire only the preview
    // intent. Its physical Read slot still belongs to its eventual reply.
    fn browse_hide_preview(&mut self) {
        self.browse.desired = None;
        self.browse.review = None;
        self.browse.settlement = None;
        self.browse.context_reveal = None;
        self.browse.preview = None;
        self.browse.replacement = None;
        self.browse.focus = self.browse.selected.map(Target::Row);
    }

    fn files_action(
        &mut self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        target: Target,
        label: &str,
        enabled: bool,
    ) -> bool {
        let selected = target == Target::Files;
        let response = ui
            .add_enabled_ui(enabled, |ui| {
                files_action_control(ui, rect, self.browse.id(&target), label, selected)
            })
            .inner;
        self.browse_response(ui, target, &response)
    }

    fn browse_files_surface(&mut self, ui: &mut egui::Ui) -> FilesActions {
        let mut actions = FilesActions::default();
        #[cfg(test)]
        {
            let visible = !ui.is_sizing_pass() && ui.is_visible();
            ui.ctx().data_mut(|data| {
                data.insert_temp(egui::Id::new("files_surface_visible_probe"), visible)
            });
        }
        let screen = ui.ctx().screen_rect();
        let width = (screen.width() - 64.0).clamp(1.0, 720.0);
        ui.set_width(width);
        let top = ui.next_widget_position();
        let bottom_margin = egui::Frame::popup(ui.style()).total_margin().bottom;
        let control_height = (egui::TextStyle::Button.resolve(ui.style()).size + 14.0).max(30.0);
        let gap = 4.0;
        let text_height = |style| {
            egui::WidgetText::from("Ag")
                .into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, style)
                .size()
                .y
        };
        let body_line_height = text_height(egui::TextStyle::Body);
        let standard_row_height =
            (text_height(egui::TextStyle::Button) + 8.0).max(chrome::ROW_HEIGHT);
        let status_height = 2.0 * body_line_height + gap;
        // Reserve twelve standard rows at the chosen UI font (about 508 points
        // at 13 pt), then bound by the viewport. This ASCII metric is stable
        // when optional CJK fonts arrive; no result count or state sizes it.
        let normal_height = 12.0 * standard_row_height
            + 11.0 * gap
            + 4.0 * control_height
            + 5.0 * gap
            + status_height;
        let height = (screen.bottom() - top.y - 24.0 - bottom_margin).clamp(1.0, normal_height);
        let surface = egui::Rect::from_min_size(top, egui::vec2(width, height));
        ui.set_min_size(surface.size());
        let band = |y, h| egui::Rect::from_min_size(egui::pos2(top.x, y), egui::vec2(width, h));
        let header = band(top.y, control_height);
        let close_width = files_text_width(ui, "Close · Esc") + 16.0;
        let files_width = files_text_width(ui, "Files") + 20.0;
        let text_width = files_text_width(ui, "Project Text") + 20.0;
        let close_rect = egui::Rect::from_min_max(
            egui::pos2(header.right() - close_width, header.top()),
            header.max,
        );
        let files_rect =
            egui::Rect::from_min_size(header.min, egui::vec2(files_width, control_height));
        let text_rect = files_rect.translate(egui::vec2(files_width + gap, 0.0));
        let text_rect =
            egui::Rect::from_min_size(text_rect.min, egui::vec2(text_width, control_height));
        if self.files_action(ui, files_rect, Target::Files, "Files", true) {
            actions.switch = Some(Provider::Files);
        }
        if self.files_action(
            ui,
            text_rect,
            Target::Text,
            "Project Text",
            self.backend_supports("search")
                || self.backend_scoped_search_supported()
                || self.backend_case_search_supported(),
        ) {
            actions.switch = Some(Provider::ProjectText);
        }
        actions.close = self.files_action(ui, close_rect, Target::Close, "Close · Esc", true);
        let workspace_rect = egui::Rect::from_min_max(
            egui::pos2(text_rect.right() + 12.0, header.top()),
            egui::pos2(
                (close_rect.left() - 12.0).max(text_rect.right() + 12.0),
                header.bottom(),
            ),
        );
        let workspace = self
            .root
            .trim_end_matches(['/', '\\'])
            .rsplit(['/', '\\'])
            .next()
            .filter(|name| !name.is_empty())
            .unwrap_or(&self.root);
        ui.put(
            workspace_rect,
            egui::Label::new(RichText::new(workspace).color(chrome::MUTED)).truncate(),
        )
        .on_hover_text(format!("Workspace: {}", self.root));

        let query_band = band(header.bottom() + gap, control_height);
        let find_width = files_text_width(ui, "Find") + 24.0;
        let find_rect = egui::Rect::from_min_max(
            egui::pos2(query_band.right() - find_width, query_band.top()),
            query_band.max,
        );
        let query_rect = egui::Rect::from_min_max(
            query_band.min,
            egui::pos2(find_rect.left() - gap, query_band.bottom()),
        );
        let query_id = self.browse.id(&Target::Query);
        if self.browse.focus == Some(Target::Query) && !ui.is_sizing_pass() {
            ui.memory_mut(|memory| memory.request_focus(query_id));
        }
        let previous_query = self.browse.query.clone();
        let (query, receipts) = put_files_text_edit(
            ui,
            query_rect,
            egui::TextEdit::singleline(&mut self.browse.query)
                .id(query_id)
                .char_limit(4096)
                .desired_width(f32::INFINITY)
                .hint_text("Type a filename"),
        );
        let native_commit = self.observe_files_ime_receipts(Target::Query, &query, &receipts);
        self.observe_files_composition_focus(Target::Query, &query);
        let edit_intent = self.browse.input.files_edit == Some(Target::Query)
            && (self.browse.input.files_plain_edit || native_commit);
        if edit_intent {
            self.browse.input.files_edit = None;
        }
        if (query.changed() && previous_query != self.browse.query)
            || (edit_intent && (query.changed() || query.has_focus()))
        {
            if !self.browse.input.files_ime_batch
                || (self.browse.input.files_plain_edit
                    && edit_intent
                    && (query.changed() || query.has_focus()))
            {
                self.browse.files_handoff_barrier = false;
            }
            self.browse_query_changed();
            actions.edited = true;
        }
        self.browse_response(ui, Target::Query, &query);
        let scope_band = band(
            query_band.bottom().max(query.rect.bottom()) + gap,
            control_height,
        );
        let discovery = self.backend_file_discovery_supported();
        if discovery {
            let label_width = files_text_width(ui, "Directory") + 12.0;
            let label_rect =
                egui::Rect::from_min_size(scope_band.min, egui::vec2(label_width, control_height));
            ui.put(
                label_rect,
                egui::Label::new(RichText::new("Directory").color(chrome::MUTED)),
            );
            let scope_rect = egui::Rect::from_min_max(
                egui::pos2(label_rect.right(), scope_band.top()),
                scope_band.max,
            );
            let id = self.browse.id(&Target::Scope);
            let previous_scope = self.browse.scope.clone();
            let (scope, receipts) = put_files_text_edit(
                ui,
                scope_rect,
                egui::TextEdit::singleline(&mut self.browse.scope)
                    .id(id)
                    .char_limit(4096)
                    .desired_width(f32::INFINITY)
                    .hint_text("Whole workspace"),
            );
            let scope = scope.on_hover_text(format!("Exact workspace-relative directory; empty searches workspace. Excluded directories: {}. Literal matching uses Unicode lowercase, without normalization.", cedar_protocol::FILE_DISCOVERY_EXCLUDED_DIRECTORIES.join(", ")));
            let native_commit = self.observe_files_ime_receipts(Target::Scope, &scope, &receipts);
            self.observe_files_composition_focus(Target::Scope, &scope);
            let edit_intent = self.browse.input.files_edit == Some(Target::Scope)
                && (self.browse.input.files_plain_edit || native_commit);
            if edit_intent {
                self.browse.input.files_edit = None;
            }
            if (scope.changed() && previous_scope != self.browse.scope)
                || (edit_intent && (scope.changed() || scope.has_focus()))
            {
                if !self.browse.input.files_ime_batch
                    || (self.browse.input.files_plain_edit
                        && edit_intent
                        && (scope.changed() || scope.has_focus()))
                {
                    self.browse.files_handoff_barrier = false;
                }
                self.browse_query_changed();
                actions.edited = true;
            }
            self.browse_response(ui, Target::Scope, &scope);
        } else {
            ui.put(
                scope_band,
                egui::Label::new(
                    RichText::new("Open and loaded files · project discovery unavailable")
                        .color(chrome::MUTED),
                )
                .truncate(),
            )
            .on_hover_text(
                "Directory scope is unavailable for Open and loaded files. No folders are scanned.",
            );
        }
        // Render in the established query → scope → submit keyboard order even
        // though Find is visually alongside the query.
        actions.submit = self.files_action(ui, find_rect, Target::Submit, "Find", true);

        let footer = band(surface.bottom() - control_height, control_height);
        let status_rect = band(scope_band.bottom() + gap, status_height);
        let mut status_ui = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("files_status")
                .max_rect(status_rect),
        );
        status_ui.set_clip_rect(ui.clip_rect().intersect(status_rect));
        let status_scroll = egui::ScrollArea::vertical().id_salt("files_status_scroll").auto_shrink([false, false]).min_scrolled_height(0.0).max_height(status_height).show(&mut status_ui, |ui| {
            let source = if let Some(source) = &self.browse.discovery.accepted {
                format!("Project files · {} results · {} entries examined", self.browse.rows.len(), source.examined)
            } else if discovery && self.browse.discovery.pending_current() {
                "Finding project files…".into()
            } else if discovery {
                if self.browse.query.is_empty() { "Project files · empty query shows open and loaded files".into() } else { "Project files".into() }
            } else {
                format!("Open and loaded files · {} results", self.browse.rows.len())
            };
            ui.add(egui::Label::new(RichText::new(source).color(chrome::MUTED)).truncate())
                .on_hover_text(format!("Project files search after a short typing pause (250 ms); Enter searches now. Workspace: {}. Directory scope: /{}.", self.root, self.browse.scope));
            if let Some(message) = &self.browse.message {
                ui.label(RichText::new(message).color(if self.browse.files_settlement.is_some() { chrome::MUTED } else { AMBER })).on_hover_text(message);
            } else if let Some(source) = self.browse.discovery.accepted.as_ref().filter(|source| source.incomplete) {
                ui.colored_label(AMBER, format!("Incomplete scan: {}. A narrower directory scope can help with traversal limits.", source.reasons.iter().map(discovery_reason).collect::<Vec<_>>().join(", ")));
            } else if self.browse.truncated {
                ui.colored_label(AMBER, "Result limit reached; more matches may exist. Narrow the query.");
            }
        });
        #[cfg(test)]
        crate::browse_tests::record_files_status_scroll(
            ui.ctx(),
            status_scroll.id,
            status_scroll.inner_rect,
            status_scroll.content_size,
        );
        #[cfg(not(test))]
        let _ = status_scroll;
        let body = egui::Rect::from_min_max(
            egui::pos2(top.x, status_rect.bottom() + gap),
            egui::pos2(
                surface.right(),
                (footer.top() - gap).max(status_rect.bottom() + gap),
            ),
        )
        .round_to_pixels(ui.pixels_per_point());
        let preview_height = if self.browse.desired.is_some() {
            // Font::row_height includes fallback-family metrics that may be
            // taller than the actual row galleys. Measure this accepted list,
            // without drawing or producing another set of native responses.
            let row_height = self
                .browse
                .rows
                .iter()
                .map(|row| {
                    egui::WidgetText::from(row.path())
                        .into_galley(
                            ui,
                            Some(egui::TextWrapMode::Extend),
                            f32::INFINITY,
                            egui::TextStyle::Button,
                        )
                        .size()
                        .y
                        + 8.0
                })
                .fold(chrome::ROW_HEIGHT, f32::max);
            let results_min = 3.0 * row_height + 2.0 * gap;
            let preview_min =
                (control_height + 3.0 * ui.text_style_height(&egui::TextStyle::Monospace)).ceil();
            if body.height() >= results_min + preview_min {
                (body.height() * 0.48).clamp(preview_min, body.height() - results_min)
            } else {
                // Below the supported budget, share the actual space. Never
                // push Close or the action footer outside the viewport.
                body.height() * 0.48
            }
        } else {
            0.0
        };
        let results_rect = egui::Rect::from_min_max(
            body.min,
            egui::pos2(
                body.right(),
                (body.bottom() - preview_height).round_to_pixels(ui.pixels_per_point()),
            ),
        );
        let mut results_ui = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("files_results_panel")
                .max_rect(results_rect),
        );
        results_ui.set_clip_rect(ui.clip_rect().intersect(results_rect));
        self.browse_results_panel(
            &mut results_ui,
            results_rect.height(),
            &mut actions.select,
            &mut actions.open,
            &mut None,
        );
        if self.browse.desired.is_some() {
            let preview_rect =
                egui::Rect::from_min_max(egui::pos2(body.left(), results_rect.bottom()), body.max);
            actions.hide_preview = self.browse_files_preview(ui, preview_rect, control_height);
        }
        ui.painter().hline(
            surface.x_range(),
            footer.top() - gap / 2.0,
            egui::Stroke::new(1.0_f32, chrome::BORDER),
        );
        let selected = self
            .browse
            .selected
            .and_then(|index| self.browse.rows.get(index))
            .is_some_and(|row| self.browse_row_current(row));
        let preview_width = files_text_width(ui, "Preview · Space") + 16.0;
        let open_width = files_text_width(ui, "Open · Enter") + 16.0;
        let preview_button =
            egui::Rect::from_min_size(footer.min, egui::vec2(preview_width, control_height));
        let open_button = egui::Rect::from_min_size(
            egui::pos2(preview_button.right() + gap, footer.top()),
            egui::vec2(open_width, control_height),
        );
        actions.preview = self.files_action(
            ui,
            preview_button,
            Target::Preview,
            "Preview · Space",
            selected,
        );
        actions.open |= self.files_action(ui, open_button, Target::Open, "Open · Enter", selected);
        let hint_rect = egui::Rect::from_min_max(
            egui::pos2(open_button.right() + 12.0, footer.top()),
            footer.max,
        );
        let hint = if self.browse.desired.is_some() {
            "Esc returns to results"
        } else if query.has_focus() {
            "Enter searches · ↓ selects"
        } else {
            "Read-only preview · 1 MiB max"
        };
        files_label_at(ui, hint_rect, egui::Label::new(RichText::new(hint).color(chrome::MUTED)).truncate().halign(egui::Align::Min))
            .on_hover_text("Enter in the query searches; Enter on a result opens it. Space explicitly previews. Preview is read-only, at most 1 MiB, and never creates an editable tab. Escape returns to the selected result, then closes Files.");
        actions
    }

    fn browse_files_preview(
        &mut self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        header_height: f32,
    ) -> bool {
        ui.painter().hline(
            rect.x_range(),
            rect.top(),
            egui::Stroke::new(1.0_f32, chrome::BORDER),
        );
        let hide_width = files_text_width(ui, "Hide preview") + 16.0;
        let header = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), header_height));
        let hide_rect = egui::Rect::from_min_max(
            egui::pos2(header.right() - hide_width, header.top()),
            header.max,
        );
        let hide = self.files_action(ui, hide_rect, Target::HidePreview, "Hide preview", true);
        let title_rect = egui::Rect::from_min_max(
            header.min,
            egui::pos2(hide_rect.left() - 8.0, header.bottom()),
        );
        let path = self
            .browse
            .desired
            .as_ref()
            .map(|ticket| ticket.row.path())
            .unwrap_or("");
        let source = match &self.browse.preview {
            Some(Preview::Draft(_)) => "Draft · read-only",
            Some(Preview::Disk { .. }) => "Disk · read-only",
            Some(Preview::Message { .. }) => "Preview unavailable",
            None => "Preview requested…",
        };
        files_label_at(
            ui,
            title_rect,
            egui::Label::new(RichText::new(format!("{source} · {path}")).color(chrome::MUTED))
                .truncate()
                .halign(egui::Align::Min),
        )
        .on_hover_text(format!("{path}\n{}", self.browse_preview_title(false)));
        let content_rect =
            egui::Rect::from_min_max(egui::pos2(rect.left(), header.bottom()), rect.max)
                .round_to_pixels(ui.pixels_per_point());
        let mut content_ui = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("files_preview_panel")
                .max_rect(content_rect),
        );
        content_ui.set_clip_rect(ui.clip_rect().intersect(content_rect));
        let id = self.browse.id(&Target::PreviewPane);
        let scroll = egui::ScrollArea::both()
            .id_salt("browse_preview_scroll")
            .animated(false)
            .auto_shrink([false, false])
            .min_scrolled_height(0.0)
            .max_height(content_rect.height())
            .show(&mut content_ui, |ui| {
                let content = if let Some(Preview::Message { message, .. }) = &self.browse.preview {
                    ui.label(RichText::new(message).color(AMBER))
                } else {
                    ui.add(
                        egui::Label::new(
                            RichText::new(
                                self.current_preview_text(PreviewSurface::Browse)
                                    .unwrap_or("Waiting for preview…"),
                            )
                            .monospace(),
                        )
                        .selectable(false),
                    )
                };
                ui.interact(content.rect, id, egui::Sense::click())
            });
        #[cfg(test)]
        record_context_scroll(
            ui.ctx(),
            scroll.id,
            scroll.inner_rect,
            PreviewSurface::Browse,
        );
        self.browse_response(&content_ui, Target::PreviewPane, &scroll.inner);
        hide
    }

    pub(super) fn browse_window(&mut self, ctx: &egui::Context) {
        self.validate_project_text_review();
        if !self.browse_current() {
            self.dismiss_navigation();
            return;
        }
        if self.browse.provider == Provider::ProjectText {
            let unchanged = self.browse.rows.first().is_some_and(
                |row| matches!(row, Row::Text(hit) if self.browse_search_snapshot_current(hit)),
            );
            if !unchanged {
                self.browse.rows = self
                    .browse_search_hits()
                    .into_iter()
                    .map(Row::Text)
                    .collect();
                self.browse.selected = self.search_state.selected_index();
                self.browse.truncated = self.search_truncated;
            }
        }
        if self
            .browse
            .desired
            .as_ref()
            .is_some_and(|ticket| !self.browse_ticket_current(ticket))
        {
            self.browse.desired = None;
            self.browse.preview = None;
            self.browse.replacement = None;
        }
        let key = self.browse.input.key.clone().filter(|intent| {
            self.browse_input_current(ctx)
                && intent.source.token == self.browse.token(intent.source.token.target.clone())
                && ctx.memory(|memory| memory.focused()) == Some(intent.source.id)
        });
        let mut submit = false;
        let mut select = None;
        let mut preview = false;
        let mut hide_preview = false;
        let mut open = false;
        let mut close = false;
        let mut switch = None;
        let mut toggle_group = None;
        if let Some(intent) = &key {
            match (&intent.source.token.target, intent.key) {
                (_, egui::Key::Escape) => {
                    if self.browse.desired.is_some() || self.browse.review.is_some() {
                        self.browse_hide_preview();
                    } else {
                        close = true;
                    }
                }
                (
                    Target::Query | Target::Scope | Target::TextScope | Target::Masks,
                    egui::Key::Enter,
                ) => submit = true,
                (
                    Target::Query | Target::Scope | Target::TextScope | Target::Masks,
                    egui::Key::ArrowDown,
                ) => {
                    select = if self.browse.provider == Provider::ProjectText {
                        self.search_state.visible_indices().first().copied()
                    } else {
                        (!self.browse.rows.is_empty()).then_some(0)
                    }
                }
                (Target::Row(index), egui::Key::Enter) => {
                    select = Some(*index);
                    open = true;
                }
                (Target::Row(index), egui::Key::Space) => {
                    select = Some(*index);
                    preview = true;
                }
                (Target::Row(index), key) => {
                    select = if self.browse.provider == Provider::ProjectText {
                        self.search_state.destination(*index, key)
                    } else {
                        search_keyboard::destination(self.browse.rows.len(), *index, key)
                    }
                }
                (Target::Group(first), egui::Key::Enter | egui::Key::Space) => {
                    toggle_group = Some(*first)
                }
                _ => {}
            }
        }
        if let Some(index) = select.take() {
            self.browse_select_owned(index);
        }
        self.browse.input.presented.clear();
        let modal_id = egui::Id::new("navigation_modal");
        egui::Modal::new(modal_id)
            .area(
                egui::Modal::default_area(modal_id)
                    .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 24.0)),
            )
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                ui.spacing_mut().button_padding = egui::vec2(6.0, 3.0);
                if self.browse.provider == Provider::Files {
                    let actions = self.browse_files_surface(ui);
                    if actions.edited {
                        submit = false;
                        select = None;
                        preview = false;
                        open = false;
                    }
                    submit |= actions.submit;
                    select = actions.select.or(select);
                    preview |= actions.preview;
                    hide_preview |= actions.hide_preview;
                    open |= actions.open;
                    close |= actions.close;
                    switch = actions.switch.or(switch);
                    return;
                }
                let actions = self.browse_text_surface(ui);
                if actions.edited {
                    submit = false;
                    select = None;
                    preview = false;
                    open = false;
                }
                submit |= actions.submit;
                select = actions.select.or(select);
                preview |= actions.preview;
                hide_preview |= actions.hide_preview;
                open |= actions.open;
                close |= actions.close;
                switch = actions.switch.or(switch);
                toggle_group = actions.toggle_group.or(toggle_group);
                // Case changes never share an activation frame with submission
                // or a result action. Preserve ordinary text-edit + Enter behavior.
                if actions.case_changed || self.browse.input.case_attempted {
                    submit = false;
                    select = None;
                    preview = false;
                    open = false;
                    toggle_group = None;
                }
            });
        self.finish_files_composition();
        // Same modal quarantine as the legacy chooser: entry/exit events cannot
        // become edits in the underlying editor, even before focus settles.
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
        if close {
            self.browse_dismiss();
        } else if let Some(provider) = switch {
            self.browse_switch(provider);
        } else if submit {
            self.browse_submit();
        } else if hide_preview {
            self.browse_hide_preview();
        } else if let Some(first) = toggle_group {
            self.browse_toggle_text_group(first);
        } else {
            if let Some(index) = select {
                self.browse_select_owned(index);
                if ctx.memory(|memory| memory.focused())
                    == Some(self.browse.id(&Target::Row(index)))
                {
                    self.browse.focus = None;
                }
            }
            if preview {
                self.browse_preview();
            }
            if open {
                self.browse_open();
            }
        }
        // Commit only an accepted non-sizing response, after its selection has
        // settled the preview pane. Taking the candidate makes this once-only
        // even if egui discards the pass and renders the same raw batch again.
        if let Some((token, id)) = self.browse.input.accepted_row.take() {
            if self.browse_current() && token == self.browse.token(token.target.clone()) {
                self.browse.input.completed_row = Some(self.browse_row_click(token, id));
            }
        }
        self.browse.input.key = None;
        self.browse.input.click = None;
        if self.browse.focus.is_some() {
            ctx.request_repaint();
        }
    }
}

#[derive(Default)]
struct TextActions {
    edited: bool,
    case_changed: bool,
    submit: bool,
    select: Option<usize>,
    toggle_group: Option<usize>,
    preview: bool,
    hide_preview: bool,
    open: bool,
    close: bool,
    switch: Option<Provider>,
}

fn text_line_height(ui: &egui::Ui, text: &str, style: egui::TextStyle) -> f32 {
    egui::WidgetText::from(text)
        .into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, style)
        .size()
        .y
}

#[derive(Default)]
struct FilesActions {
    edited: bool,
    submit: bool,
    select: Option<usize>,
    preview: bool,
    hide_preview: bool,
    open: bool,
    close: bool,
    switch: Option<Provider>,
}

pub(super) fn text_matching_line_count(lines: usize, files: usize, incomplete: bool) -> String {
    let line_unit = if lines == 1 { "line" } else { "lines" };
    let file_unit = if files == 1 { "file" } else { "files" };
    if incomplete {
        format!("{lines} matching {line_unit} observed in {files} {file_unit} · incomplete")
    } else {
        format!("{lines} matching {line_unit} in {files} {file_unit}")
    }
}

fn files_label_at(ui: &mut egui::Ui, rect: egui::Rect, label: egui::Label) -> egui::Response {
    // Ui::put centers the entire label widget even when its text halign is Min.
    // Use a left-origin child so the actual painted title starts at the pane edge.
    let mut label_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    label_ui.set_clip_rect(ui.clip_rect().intersect(rect));
    label_ui.add(label)
}

fn files_text_width(ui: &egui::Ui, text: &str) -> f32 {
    egui::WidgetText::from(text)
        .into_galley(
            ui,
            Some(egui::TextWrapMode::Extend),
            f32::INFINITY,
            egui::TextStyle::Button,
        )
        .size()
        .x
}

fn files_action_control(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    id: egui::Id,
    label: &str,
    selected: bool,
) -> egui::Response {
    let response = ui.interact(rect, id, egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact_selectable(&response, selected);
        if selected || response.hovered() || response.has_focus() {
            ui.painter().rect(
                rect,
                3.0,
                if selected {
                    chrome::SELECTED
                } else {
                    chrome::HOVER
                },
                if response.has_focus() {
                    egui::Stroke::new(1.0_f32, chrome::FOCUS)
                } else {
                    egui::Stroke::NONE
                },
                egui::StrokeKind::Inside,
            );
        }
        let galley = egui::WidgetText::from(label).into_galley(
            ui,
            Some(egui::TextWrapMode::Truncate),
            (rect.width() - 12.0).max(0.0),
            egui::TextStyle::Button,
        );
        ui.painter().galley(
            rect.center() - galley.size() / 2.0,
            galley,
            visuals.text_color(),
        );
        if selected {
            ui.painter().hline(
                rect.x_range(),
                rect.bottom() - 1.0,
                egui::Stroke::new(2.0_f32, chrome::FOCUS),
            );
        }
    }
    response
}

// The current Text tab has a richer accessible name than its painted caption.
// Report it once using the actual native enabled state; Files keeps its original
// helper and event behavior unchanged.
fn text_tab_control(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    id: egui::Id,
    label: &str,
) -> egui::Response {
    let response = ui.interact(rect, id, egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            ui.is_enabled(),
            "Find in Files · Project Text",
        )
    });
    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact_selectable(&response, true);
        ui.painter().rect(
            rect,
            3.0,
            chrome::SELECTED,
            if response.has_focus() {
                egui::Stroke::new(1.0_f32, chrome::FOCUS)
            } else {
                egui::Stroke::NONE
            },
            egui::StrokeKind::Inside,
        );
        let galley = egui::WidgetText::from(label).into_galley(
            ui,
            Some(egui::TextWrapMode::Truncate),
            (rect.width() - 12.0).max(0.0),
            egui::TextStyle::Button,
        );
        ui.painter().galley(
            rect.center() - galley.size() / 2.0,
            galley,
            visuals.text_color(),
        );
        ui.painter().hline(
            rect.x_range(),
            rect.bottom() - 1.0,
            egui::Stroke::new(2.0_f32, chrome::FOCUS),
        );
    }
    response
}

// Elide only a painted copy. Character boundaries keep UTF-8 intact, while
// retaining the end keeps extensions and the nearest parent directory useful.
// Neither this text nor the resulting galley ever supplies an action identity.
fn files_path_galley(
    ui: &egui::Ui,
    text: &str,
    width: f32,
    color: egui::Color32,
    tail: bool,
) -> Arc<egui::Galley> {
    // Keep non-ASCII input intact: scalar slicing could detach combining marks,
    // emoji modifiers, or ZWJ sequences. Native single-line elision retains the
    // original layout text; it does not promise grapheme-boundary rendering.
    if !text.is_ascii() {
        return egui::WidgetText::from(RichText::new(text).color(color)).into_galley(
            ui,
            Some(egui::TextWrapMode::Truncate),
            width.max(0.0),
            egui::TextStyle::Button,
        );
    }
    let font = egui::TextStyle::Button.resolve(ui.style());
    let layout = |text: String| ui.fonts(|fonts| fonts.layout_no_wrap(text, font.clone(), color));
    let full = layout(text.to_owned());
    if full.size().x <= width {
        return full;
    }
    let chars: Vec<char> = text.chars().collect();
    let mut low = 0;
    let mut high = chars.len();
    let mut best = layout("…".into());
    while low < high {
        let keep = (low + high).div_ceil(2);
        let suffix = if tail { keep } else { keep.div_ceil(2) };
        let prefix = keep - suffix;
        let text = chars[..prefix]
            .iter()
            .chain(std::iter::once(&'…'))
            .chain(chars[chars.len() - suffix..].iter())
            .collect();
        let candidate = layout(text);
        if candidate.size().x <= width {
            best = candidate;
            low = keep;
        } else {
            high = keep - 1;
        }
    }
    best
}

fn files_result_row(
    ui: &mut egui::Ui,
    id: egui::Id,
    selected: bool,
    path: &str,
    annotation: &str,
    accessible: &str,
) -> egui::Response {
    let (parent, name) = path.rsplit_once('/').unwrap_or(("", path));
    let width = ui.available_width();
    let padding = 6.0;
    let icon_width = 20.0;
    let status = egui::WidgetText::from(RichText::new(annotation)).into_galley(
        ui,
        Some(egui::TextWrapMode::Truncate),
        (width * 0.4).max(0.0),
        egui::TextStyle::Button,
    );
    let status_width = if annotation.is_empty() {
        0.0
    } else {
        status.size().x + 12.0
    };
    let text_width = (width - 2.0 * padding - icon_width - status_width).max(1.0);
    let name_width = if parent.is_empty() {
        text_width
    } else {
        (text_width * 0.6).min(files_text_width(ui, name))
    };
    let name = files_path_galley(ui, name, name_width, chrome::TEXT, false);
    let parent = files_path_galley(
        ui,
        parent,
        (text_width - name.size().x - 12.0).max(1.0),
        chrome::MUTED,
        true,
    );
    let height =
        (name.size().y.max(parent.size().y).max(status.size().y) + 8.0).max(chrome::ROW_HEIGHT);
    let (_, rect) = ui.allocate_space(egui::vec2(width, height));
    let response = ui.interact(rect, id, egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::SelectableLabel,
            ui.is_enabled(),
            accessible,
        )
    });
    if ui.is_rect_visible(rect) {
        let background = if selected {
            chrome::SELECTED
        } else if response.hovered() {
            chrome::HOVER
        } else {
            egui::Color32::TRANSPARENT
        };
        ui.painter().rect(
            rect,
            3.0,
            background,
            if response.has_focus() {
                egui::Stroke::new(1.0_f32, chrome::FOCUS)
            } else {
                egui::Stroke::NONE
            },
            egui::StrokeKind::Inside,
        );
        chrome::paint_icon(
            ui,
            egui::Rect::from_center_size(
                egui::pos2(rect.left() + padding + 8.0, rect.center().y),
                egui::vec2(16.0, 16.0),
            ),
            chrome::file_icon(path, false),
            chrome::MUTED,
        );
        let x = rect.left() + padding + icon_width;
        ui.painter().galley(
            egui::pos2(x, rect.center().y - name.size().y / 2.0),
            name.clone(),
            chrome::TEXT,
        );
        ui.painter().galley(
            egui::pos2(
                x + name.size().x + 12.0,
                rect.center().y - parent.size().y / 2.0,
            ),
            parent,
            chrome::MUTED,
        );
        if !annotation.is_empty() {
            ui.painter().galley(
                egui::pos2(
                    rect.right() - padding - status.size().x,
                    rect.center().y - status.size().y / 2.0,
                ),
                status,
                if annotation == "Open" {
                    chrome::MUTED
                } else {
                    AMBER
                },
            );
        }
    }
    response
        .on_hover_text(accessible)
        .on_disabled_hover_text(accessible)
}

fn owns_key(target: &Target, key: egui::Key) -> bool {
    key == egui::Key::Escape
        || match target {
            Target::Query | Target::Scope | Target::TextScope | Target::Masks => {
                matches!(key, egui::Key::Enter | egui::Key::ArrowDown)
            }
            Target::Row(_) => matches!(
                key,
                egui::Key::Enter
                    | egui::Key::Space
                    | egui::Key::ArrowUp
                    | egui::Key::ArrowDown
                    | egui::Key::Home
                    | egui::Key::End
            ),
            Target::PreviewPane => false,
            _ => matches!(key, egui::Key::Enter | egui::Key::Space),
        }
}
// Measurement and decoration only. Each surface retains its own allocation,
// native response, accessibility, focus lock, tooltip and admission policy.
pub(super) struct TextResultRow {
    size: egui::Vec2,
    foreground: egui::Color32,
    content: TextResultContent,
}

enum TextResultContent {
    Group {
        name: Arc<egui::Galley>,
        parent: Arc<egui::Galley>,
        count: Arc<egui::Galley>,
        expanded: bool,
        icon: chrome::Icon,
    },
    Hit {
        number: Arc<egui::Galley>,
        snippet: Arc<egui::Galley>,
        line_width: f32,
        trim_start: usize,
    },
}

pub(super) fn text_result_line_width(ui: &egui::Ui, maximum: Option<usize>) -> f32 {
    maximum.map_or(0.0, |line| {
        egui::WidgetText::from(RichText::new(line.to_string()).monospace())
            .into_galley(
                ui,
                Some(egui::TextWrapMode::Extend),
                f32::INFINITY,
                egui::TextStyle::Monospace,
            )
            .size()
            .x
    })
}

impl TextResultRow {
    pub(super) fn group(ui: &egui::Ui, path: &str, count: usize, expanded: bool) -> Self {
        let (parent, basename) = path.rsplit_once('/').unwrap_or(("", path));
        let width = ui.available_width();
        let foreground = if ui.is_enabled() {
            chrome::TEXT
        } else {
            chrome::MUTED
        };
        let galley = |text: &str, width: f32, color| {
            egui::WidgetText::from(RichText::new(text).color(color)).into_galley(
                ui,
                Some(egui::TextWrapMode::Truncate),
                width.max(0.0),
                egui::TextStyle::Button,
            )
        };
        let count = galley(&count.to_string(), width * 0.2, chrome::MUTED);
        let text_width = (width - 48.0 - count.size().x - 18.0).max(0.0);
        let name_width = if parent.is_empty() {
            text_width
        } else {
            files_text_width(ui, basename).min(text_width * 0.6)
        };
        let name = galley(basename, name_width, foreground);
        let parent = galley(
            parent,
            (text_width - name.size().x - 12.0).max(0.0),
            chrome::MUTED,
        );
        let height = (name.size().y.max(parent.size().y).max(count.size().y) + 6.0)
            .max(ui.spacing().interact_size.y);
        Self {
            size: egui::vec2(width, height),
            foreground,
            content: TextResultContent::Group {
                name,
                parent,
                count,
                expanded,
                icon: chrome::file_icon(path, false),
            },
        }
    }

    pub(super) fn hit(ui: &egui::Ui, line: usize, snippet: &str, line_width: f32) -> Self {
        let width = ui.available_width();
        let foreground = if ui.is_enabled() {
            chrome::TEXT
        } else {
            chrome::MUTED
        };
        let number = egui::WidgetText::from(
            RichText::new(line.to_string())
                .monospace()
                .color(chrome::MUTED),
        )
        .into_galley(
            ui,
            Some(egui::TextWrapMode::Extend),
            f32::INFINITY,
            egui::TextStyle::Monospace,
        );
        let trim_start = snippet.len() - snippet.trim_start().len();
        let snippet =
            egui::WidgetText::from(RichText::new(snippet.trim()).monospace().color(foreground))
                .into_galley(
                    ui,
                    Some(egui::TextWrapMode::Truncate),
                    (width - 46.0 - line_width - 12.0 - 6.0).max(0.0),
                    egui::TextStyle::Monospace,
                );
        let height =
            (number.size().y.max(snippet.size().y) + 6.0).max(ui.spacing().interact_size.y);
        Self {
            size: egui::vec2(width, height),
            foreground,
            content: TextResultContent::Hit {
                number,
                snippet,
                line_width,
                trim_start,
            },
        }
    }

    pub(super) fn size(&self) -> egui::Vec2 {
        self.size
    }

    pub(super) fn paint(
        &self,
        ui: &egui::Ui,
        rect: egui::Rect,
        response: &egui::Response,
        selected: bool,
        highlights: &[std::ops::Range<usize>],
    ) {
        #[cfg(test)]
        {
            let observation = (
                response.enabled(),
                response.sense,
                ui.is_sizing_pass(),
                rect,
                ui.ctx().cumulative_pass_nr(),
            );
            ui.ctx().data_mut(|data| {
                data.insert_temp(
                    response.id.with("snippet_highlight_current_paint_response"),
                    observation,
                )
            });
        }
        if !ui.is_rect_visible(rect) {
            return;
        }
        text_row_background(ui, rect, response, selected);
        match &self.content {
            TextResultContent::Group {
                name,
                parent,
                count,
                expanded,
                icon,
            } => {
                let center = egui::pos2(rect.left() + 12.0, rect.center().y);
                let points = if *expanded {
                    vec![
                        center + egui::vec2(-3.0, -2.0),
                        center + egui::vec2(3.0, -2.0),
                        center + egui::vec2(0.0, 2.0),
                    ]
                } else {
                    vec![
                        center + egui::vec2(-2.0, -3.0),
                        center + egui::vec2(2.0, 0.0),
                        center + egui::vec2(-2.0, 3.0),
                    ]
                };
                ui.painter().add(egui::Shape::convex_polygon(
                    points,
                    chrome::MUTED,
                    egui::Stroke::NONE,
                ));
                chrome::paint_icon(
                    ui,
                    egui::Rect::from_center_size(
                        egui::pos2(rect.left() + 32.0, rect.center().y),
                        egui::vec2(16.0, 16.0),
                    ),
                    *icon,
                    chrome::MUTED,
                );
                ui.painter().galley(
                    egui::pos2(rect.left() + 46.0, rect.center().y - name.size().y / 2.0),
                    name.clone(),
                    self.foreground,
                );
                ui.painter().galley(
                    egui::pos2(
                        rect.left() + 46.0 + name.size().x + 12.0,
                        rect.center().y - parent.size().y / 2.0,
                    ),
                    parent.clone(),
                    chrome::MUTED,
                );
                ui.painter().galley(
                    egui::pos2(
                        rect.right() - count.size().x - 6.0,
                        rect.center().y - count.size().y / 2.0,
                    ),
                    count.clone(),
                    chrome::MUTED,
                );
            }
            TextResultContent::Hit {
                number,
                snippet,
                line_width,
                trim_start,
            } => {
                ui.painter().galley(
                    egui::pos2(
                        rect.left() + 46.0 + line_width - number.size().x,
                        rect.center().y - number.size().y / 2.0,
                    ),
                    number.clone(),
                    chrome::MUTED,
                );
                let origin = egui::pos2(
                    rect.left() + 46.0 + line_width + 12.0,
                    rect.center().y - snippet.size().y / 2.0,
                );
                if ui.is_enabled() && !highlights.is_empty() {
                    let snippet_clip = egui::Rect::from_min_max(
                        egui::pos2(origin.x, rect.top()),
                        egui::pos2(rect.right() - 6.0, rect.bottom()),
                    );
                    let clip = ui.clip_rect().intersect(rect).intersect(snippet_clip);
                    for highlight in
                        search_keyboard::highlights::rectangles(snippet, *trim_start, highlights)
                    {
                        let highlight = highlight.translate(origin.to_vec2());
                        if clip.contains_rect(highlight) {
                            ui.painter().rect_filled(
                                highlight,
                                0.0,
                                search_keyboard::highlights::BACKGROUND,
                            );
                        }
                    }
                }
                ui.painter()
                    .galley(origin, snippet.clone(), self.foreground);
            }
        }
    }
}

// Response's tooltip constructor does not expose Area::default_height. Bound
// its first sizing rectangle only for this synchronous construction, then
// restore the one context-style field even during unwinding or nested calls.
pub(super) fn with_text_tooltip_area_height<R>(
    ctx: &egui::Context,
    height: f32,
    show: impl FnOnce() -> R,
) -> R {
    struct Restore {
        ctx: egui::Context,
        theme: egui::Theme,
        height: f32,
    }
    impl Drop for Restore {
        fn drop(&mut self) {
            self.ctx.style_mut_of(self.theme, |style| {
                style.spacing.default_area_size.y = self.height;
            });
        }
    }
    let restore = Restore {
        ctx: ctx.clone(),
        theme: ctx.theme(),
        height: ctx.style().spacing.default_area_size.y,
    };
    ctx.style_mut_of(restore.theme, |style| {
        style.spacing.default_area_size.y = height;
    });
    let result = show();
    drop(restore);
    result
}

// Keep the original native hover lifecycle and row anchor. A large tooltip
// must fit the side egui will choose using its cached (initially 64x32) size;
// merely limiting it to the larger side can still clamp it over the origin.
pub(super) fn text_result_tooltip(response: egui::Response, text: &str) -> egui::Response {
    let ctx = &response.ctx;
    let screen = ctx.screen_rect();
    let mut origin = response.rect;
    if let Some(transform) = ctx.layer_transform_to_global(response.layer_id) {
        origin = transform * origin;
    }
    let expected = egui::AreaState::load(
        ctx,
        egui::containers::popup::next_tooltip_id(ctx, response.id),
    )
    .and_then(|state| state.size)
    .unwrap_or(egui::vec2(64.0, 32.0));
    let below = (screen.bottom() - origin.bottom() - 4.0).max(0.0);
    let above = (origin.top() - screen.top() - 4.0).max(0.0);
    let space = if !ctx.input(|input| input.any_touches()) && expected.y <= below {
        below
    } else if expected.y <= above {
        above
    } else {
        // Below the native minimum window size, or after an extreme resize,
        // do not use egui's fallback at screen.left_top: it may cover the row.
        // The original response and its full accessibility text remain intact.
        return response;
    };
    let margin = egui::Frame::popup(&ctx.style()).total_margin().sum();
    let height = space.min(160.0).min(screen.height() * 0.25) - margin.y - 2.0;
    let width = (screen.width() - margin.x - 16.0).min(ctx.style().spacing.tooltip_width);
    if height <= 0.0 || width <= 0.0 {
        return response;
    }
    let id = response.id;
    let contents = |ui: &mut egui::Ui| {
        ui.set_max_width(width);
        let scroll = egui::ScrollArea::vertical()
            .id_salt(("project_text_tooltip", id))
            .auto_shrink([false, true])
            .min_scrolled_height(0.0)
            .max_height(height)
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
            .show(ui, |ui| {
                #[cfg(test)]
                ui.ctx().data_mut(|data| {
                    data.insert_temp(
                        egui::Id::new(("text_result_tooltip_clip_probe", id)),
                        ui.clip_rect(),
                    );
                });
                ui.add(egui::Label::new(text).wrap());
            });
        #[cfg(test)]
        ui.ctx().data_mut(|data| {
            data.insert_temp(
                egui::Id::new(("text_result_tooltip_geometry_probe", id)),
                (origin, expected, above, below, space, width, height),
            );
            data.insert_temp(
                egui::Id::new(("text_result_tooltip_probe", id)),
                (scroll.id, scroll.inner_rect, scroll.content_size),
            );
        });
        #[cfg(not(test))]
        let _ = scroll;
    };
    let ctx = ctx.clone();
    with_text_tooltip_area_height(&ctx, height + margin.y + 2.0, || {
        response
            .on_hover_ui(contents)
            .on_disabled_hover_ui(contents)
    })
}

// Text rows keep their accepted index IDs. All painted pieces belong to one
// native response, so decoration cannot become another navigation target.
fn text_result_group(
    ui: &mut egui::Ui,
    id: egui::Id,
    path: &str,
    count: usize,
    expanded: bool,
) -> egui::Response {
    let row = TextResultRow::group(ui, path, count, expanded);
    let (_, rect) = ui.allocate_space(row.size());
    let response = ui.interact(rect, id, egui::Sense::click());
    let accessible = format!(
        "{} {path} ({count} matching lines)",
        if expanded { "Expanded" } else { "Collapsed" }
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::SelectableLabel,
            ui.is_enabled(),
            &accessible,
        )
    });
    row.paint(ui, rect, &response, false, &[]);
    text_result_tooltip(response, &accessible)
}

fn text_row_background(ui: &egui::Ui, rect: egui::Rect, response: &egui::Response, selected: bool) {
    ui.painter().rect(
        rect,
        3.0,
        if selected {
            chrome::SELECTED
        } else if response.hovered() {
            chrome::HOVER
        } else {
            egui::Color32::TRANSPARENT
        },
        if response.has_focus() {
            egui::Stroke::new(1.0_f32, chrome::FOCUS)
        } else {
            egui::Stroke::NONE
        },
        egui::StrokeKind::Inside,
    );
}

fn text_result_hit(
    ui: &mut egui::Ui,
    id: egui::Id,
    selected: bool,
    hit: &search_keyboard::BrowseHit,
    snippet: &str,
    line_width: f32,
    highlights: &[std::ops::Range<usize>],
) -> egui::Response {
    let row = TextResultRow::hit(ui, hit.line(), snippet, line_width);
    let (_, rect) = ui.allocate_space(row.size());
    let response = ui.interact(rect, id, egui::Sense::click());
    let accessible = format!("{}:{}\n{snippet}", hit.path(), hit.line());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::SelectableLabel,
            ui.is_enabled(),
            &accessible,
        )
    });
    row.paint(ui, rect, &response, selected, highlights);
    text_result_tooltip(response, &accessible)
}

fn shortcut_provider(key: egui::Key, modifiers: egui::Modifiers) -> Option<GlobalEntry> {
    if crate::recent_files::shortcut(key, modifiers) {
        return Some(GlobalEntry::Recent);
    }
    (modifiers.ctrl && modifiers.shift && !modifiers.alt && !modifiers.mac_cmd)
        .then_some(match key {
            egui::Key::N => Some(GlobalEntry::Browse(Provider::Files)),
            egui::Key::F => Some(GlobalEntry::Browse(Provider::ProjectText)),
            _ => None,
        })
        .flatten()
}
// One exact press, optional matching release and the native Space companion.
// Mixed text, paste, IME, pointer, repeated or additional edges never activate.
pub(super) fn pointer_only(events: &[egui::Event]) -> bool {
    events.iter().all(|event| {
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
    })
}

pub(super) fn clean_press(events: &[egui::Event]) -> Option<(egui::Key, egui::Modifiers)> {
    let mut press = None;
    let mut release = false;
    let mut space = false;
    for event in events {
        match event {
            egui::Event::Key {
                key,
                pressed: true,
                repeat: false,
                modifiers,
                ..
            } if press.is_none() => press = Some((*key, *modifiers)),
            egui::Event::Key {
                key,
                pressed: false,
                modifiers,
                ..
            } if press == Some((*key, *modifiers)) && !release => release = true,
            egui::Event::Text(text) if text == " " && !space => space = true,
            _ => return None,
        }
    }
    press.filter(|(key, _)| !space || *key == egui::Key::Space)
}

pub(super) fn valid_preview_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 4096
        && !path.contains(['\0', '\\', ':'])
        && !path.starts_with('/')
        && !path.split('/').any(|part| part == "..")
}

impl CedarApp {
    fn discovery_ticket_current(&self, ticket: &DiscoveryTicket) -> bool {
        self.browse_current()
            && self.backend_file_discovery_supported()
            && self.browse.provider == Provider::Files
            && ticket.context.current(self)
            && ticket.epoch == self.browse.epoch
            && ticket.query == self.browse.query
            && ticket.query == self.browse.submitted
            && ticket.scope == self.browse.scope
            && self
                .file_discovery
                .is_some_and(|descriptor| descriptor.version == ticket.version)
            && self.browse.discovery.desired.as_ref() == Some(ticket)
    }

    pub(super) fn apply_browse_discovery(
        &mut self,
        request: u64,
        ticket: DiscoveryTicket,
        result: Result<Payload, String>,
        connected: bool,
    ) {
        if !connected {
            self.browse.discovery.flight = None;
            self.browse.discovery.completed = None;
            self.browse.discovery.replacement = None;
            self.disconnected(result.err().unwrap_or_else(|| {
                "The connection closed during project file discovery. Your drafts are retained"
                    .into()
            }));
            return;
        }
        if self
            .browse
            .discovery
            .flight
            .as_ref()
            .is_some_and(|(id, current)| *id == request && current == &ticket)
        {
            self.browse.discovery.completed = Some((request, ticket, result));
        }
    }

    fn finish_browse_discovery(&mut self, ctx: &egui::Context) {
        let owns_view = self.browse_current() && !self.foreign_modal_owns_input(ctx);
        if let Some((request, ticket, result)) = self.browse.discovery.completed.take() {
            if self
                .browse
                .discovery
                .flight
                .as_ref()
                .is_some_and(|(id, _)| *id == request)
            {
                self.browse.discovery.flight = None;
            }
            if owns_view && self.discovery_ticket_current(&ticket) {
                let validated = match result {
                    Ok(Payload::FilePaths { result }) => result
                        .validate_for(&ticket.query, &ticket.scope, DEFAULT_FILE_DISCOVERY_RESULTS)
                        .map(|_| result)
                        .map_err(|error| error.to_string()),
                    Ok(_) => {
                        Err("Unexpected project file response; no results were accepted.".into())
                    }
                    Err(error) => Err(error),
                };
                match validated {
                    Ok(result) => {
                        self.cjk_seen |= result
                            .paths
                            .iter()
                            .any(|path| crate::system_fonts::contains_cjk(path));
                        let source = Arc::new(ProjectSource {
                            ticket,
                            request,
                            paths: result.paths,
                            incomplete: result.incomplete,
                            reasons: result.reasons,
                            examined: result.examined_entries,
                        });
                        self.browse.rows = (0..source.paths.len())
                            .map(|index| {
                                Row::Project(ProjectCandidate {
                                    source: source.clone(),
                                    index,
                                })
                            })
                            .collect();
                        self.browse.selected = (!self.browse.rows.is_empty()).then_some(0);
                        self.browse.discovery.accepted = Some(source);
                        self.browse.message = None;
                    }
                    Err(message) => {
                        self.browse.message = Some(message);
                    }
                }
                ctx.request_repaint();
            }
        }
        // A new connection owns a different transport. Ordinary query retirement
        // never releases this slot or closes the worker connection.
        if self
            .browse
            .discovery
            .flight
            .as_ref()
            .is_some_and(|(id, ticket)| {
                ticket.context.generation != self.generation || !self.pending.contains_key(id)
            })
        {
            self.browse.discovery.flight = None;
        }
        if self.browse.discovery.flight.is_none() {
            if let Some(ticket) = self.browse.discovery.replacement.take() {
                if owns_view && self.discovery_ticket_current(&ticket) {
                    if self.next_request == 0 || self.next_request == u64::MAX {
                        self.browse.message = Some("Project files cannot submit because the request identity is exhausted. Reopen Cedar to start a new session.".into());
                        ctx.request_repaint();
                        return;
                    }
                    let request = self.request(
                        Operation::FindFiles {
                            query: ticket.query.clone(),
                            scope: ticket.scope.clone(),
                            limit: DEFAULT_FILE_DISCOVERY_RESULTS,
                        },
                        Job::BrowseFindFiles {
                            ticket: ticket.clone(),
                        },
                    );
                    if request != 0 {
                        self.browse.discovery.flight = Some((request, ticket));
                    }
                }
            }
        }
    }
}

fn discovery_reason(reason: &FileDiscoveryIncompleteReason) -> &'static str {
    use FileDiscoveryIncompleteReason as Reason;
    match reason {
        Reason::PermissionDenied => "permission denied",
        Reason::IoError => "unreadable directory",
        Reason::DepthLimit => "directory depth limit",
        Reason::TimeLimit => "time budget",
        Reason::EntryLimit => "entry limit",
        Reason::StackLimit => "pending directory limit",
        Reason::ResponseLimit => "response size limit",
        Reason::ResultLimit => "result limit",
        Reason::UnrepresentablePath => "unrepresentable path",
        Reason::UnsafePath => "unsafe path skipped",
    }
}

// The formatter remains the sole owner of context text and byte limits. Split
// only its leading metadata; source code after the delimiter is never parsed.
struct ContextLine<'a> {
    full: &'a str,
    number: &'a str,
    code: &'a str,
    target: bool,
}

fn context_lines(context: &ProjectTextContext) -> Option<Vec<ContextLine<'_>>> {
    if context.text.len() > MAX_PROJECT_TEXT_CONTEXT_BYTES {
        return None;
    }
    let mut rows = Vec::new();
    let mut previous = None;
    let mut targets = 0;
    for full in context.text.lines() {
        if rows.len() == MAX_PROJECT_TEXT_CONTEXT_LINES {
            return None;
        }
        let (target, rest) = if let Some(rest) = full.strip_prefix("▶ ") {
            (true, rest)
        } else {
            (false, full.strip_prefix("  ")?)
        };
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        let number = &rest[..digits];
        if number.is_empty() || number.starts_with('0') {
            return None;
        }
        let value = number.parse::<usize>().ok()?;
        if previous.is_some_and(|previous: usize| previous.checked_add(1) != Some(value)) {
            return None;
        }
        previous = Some(value);
        let code = rest[digits..].strip_prefix(" | ")?;
        targets += usize::from(target);
        rows.push(ContextLine {
            full,
            number,
            code,
            target,
        });
    }
    (rows.is_empty() || targets == 1).then_some(rows)
}

struct ContextRowLayout<'a> {
    line: ContextLine<'a>,
    height: f32,
    marker: Arc<egui::Galley>,
    number: Arc<egui::Galley>,
    separator: Arc<egui::Galley>,
    code: Arc<egui::Galley>,
    offsets_y: [f32; 4],
}

struct ContextLayout<'a> {
    rows: Vec<ContextRowLayout<'a>>,
    number_right: f32,
    separator_x: f32,
    code_x: f32,
    width: f32,
}

fn context_galley(ui: &egui::Ui, text: &str, color: egui::Color32) -> Arc<egui::Galley> {
    let mut job = egui::WidgetText::from(RichText::new(text).monospace().color(color))
        .into_layout_job(ui.style(), egui::FontSelection::Default, ui.text_valign());
    job.wrap.max_width = f32::INFINITY;
    job.halign = ui.layout().horizontal_placement();
    job.justify = ui.layout().horizontal_justify();
    ui.fonts(|fonts| fonts.layout_job(job))
}

// Fallback faces need not share the arrow's baseline. Every piece must match
// its own reference glyph slice with one finite vertical translation.
fn context_piece_offset(
    reference: &egui::Galley,
    piece: &egui::Galley,
    start: usize,
) -> Option<f32> {
    if reference.rows.len() != 1 || piece.rows.len() != 1 {
        return None;
    }
    let glyphs = &piece.rows[0].glyphs;
    let originals = reference.rows[0]
        .glyphs
        .get(start..start.checked_add(glyphs.len())?)?;
    let offset = originals
        .first()
        .zip(glyphs.first())
        .map_or(0.0, |(original, glyph)| original.pos.y - glyph.pos.y);
    (offset.is_finite()
        && glyphs.iter().zip(originals).all(|(glyph, original)| {
            glyph.chr == original.chr
                && glyph.pos.is_finite()
                && original.pos.is_finite()
                && glyph.pos.y + offset == original.pos.y
        }))
    .then_some(offset)
}

fn context_layout<'a>(ui: &egui::Ui, context: &'a ProjectTextContext) -> Option<ContextLayout<'a>> {
    let lines = context_lines(context)?;
    let color = ui.visuals().text_color();
    let muted = ui.visuals().weak_text_color();
    let space = context_galley(ui, " ", muted).size().x;
    let marker = context_galley(ui, "▶", muted);
    let blank = context_galley(ui, " ", muted);
    let separator = context_galley(ui, "|", muted);
    let mut number_width = 0.0_f32;
    let mut code_width = 0.0_f32;
    let mut rows = Vec::new();
    for line in lines {
        let reference = context_galley(ui, line.full, color);
        if reference.rows.len() != 1 || reference.rows[0].glyphs.len() != line.full.chars().count()
        {
            return None;
        }
        let height = reference.size().y;
        let number = context_galley(ui, line.number, muted);
        let code = context_galley(ui, line.code, color);
        let marker = if line.target {
            marker.clone()
        } else {
            blank.clone()
        };
        let offsets_y = [
            context_piece_offset(&reference, &marker, 0)?,
            context_piece_offset(&reference, &number, 2)?,
            context_piece_offset(&reference, &separator, line.number.len() + 3)?,
            context_piece_offset(&reference, &code, line.number.len() + 5)?,
        ];
        if !height.is_finite()
            || height <= 0.0
            || !number.size().is_finite()
            || !code.size().is_finite()
        {
            return None;
        }
        number_width = number_width.max(number.size().x);
        code_width = code_width.max(code.size().x);
        rows.push(ContextRowLayout {
            line,
            height,
            marker,
            number,
            separator: separator.clone(),
            code,
            offsets_y,
        });
    }
    let number_right = marker.size().x.max(blank.size().x) + space + number_width;
    let separator_x = number_right + space;
    let code_x = separator_x + separator.size().x + space;
    let width = (code_x + code_width).max(ui.available_width());
    [number_right, separator_x, code_x, width]
        .into_iter()
        .all(|value| value.is_finite() && value >= 0.0)
        .then_some(ContextLayout {
            rows,
            number_right,
            separator_x,
            code_x,
            width,
        })
}

fn draw_project_text_context(
    ui: &mut egui::Ui,
    context: &ProjectTextContext,
    reveal: bool,
) -> egui::Response {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let Some(layout) = context_layout(ui, context) else {
            // Preserve the old complete plain presentation on any malformed
            // metadata or untrustworthy geometry; never drop individual rows.
            for line in context.text.lines() {
                let response = ui.add(
                    egui::Label::new(RichText::new(line).monospace())
                        .selectable(false)
                        .wrap_mode(egui::TextWrapMode::Extend),
                );
                if line.starts_with('▶') {
                    context_target(ui, &response, reveal);
                }
            }
            return;
        };
        for row in layout.rows {
            // This is exactly the nonselectable Label's existing sense policy.
            let sense = if ui.memory(|memory| memory.options.screen_reader) {
                egui::Sense::focusable_noninteractive()
            } else {
                egui::Sense::hover()
            };
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(layout.width, row.height), sense);
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), row.line.full)
            });
            if ui.is_rect_visible(rect) {
                if row.line.target {
                    ui.painter().rect_filled(
                        rect,
                        0.0,
                        ui.visuals().selection.bg_fill.gamma_multiply(0.15),
                    );
                }
                let underline = if response.has_focus() || response.highlighted() {
                    egui::Stroke::new(1.0_f32, ui.visuals().text_color())
                } else {
                    egui::Stroke::NONE
                };
                let number_x = layout.number_right - row.number.size().x;
                for (galley, x, y) in [
                    (row.marker, 0.0, row.offsets_y[0]),
                    (row.number, number_x, row.offsets_y[1]),
                    (row.separator, layout.separator_x, row.offsets_y[2]),
                    (row.code, layout.code_x, row.offsets_y[3]),
                ] {
                    ui.painter().add(
                        egui::epaint::TextShape::new(
                            rect.min + egui::vec2(x, y),
                            galley,
                            ui.visuals().text_color(),
                        )
                        .with_underline(underline),
                    );
                }
            }
            #[cfg(test)]
            crate::workspace_access_tests::record(
                ui,
                &format!("project_text_context_row:{}", row.line.number),
                &response,
            );
            if row.line.target {
                context_target(ui, &response, reveal);
            }
        }
    })
    .response
}

fn context_target(ui: &mut egui::Ui, response: &egui::Response, reveal: bool) {
    #[cfg(test)]
    crate::workspace_access_tests::record(ui, "project_text_context_target", response);
    if reveal {
        let marker = egui::Rect::from_min_max(
            response.rect.min,
            egui::pos2(response.rect.min.x + 1.0, response.rect.max.y),
        );
        ui.scroll_to_rect(marker, Some(egui::Align::Center));
    }
}
#[cfg(test)]
fn record_context_scroll(
    ctx: &egui::Context,
    id: egui::Id,
    rect: egui::Rect,
    surface: PreviewSurface,
) {
    ctx.data_mut(|data| {
        data.insert_temp(
            egui::Id::new((
                "project_text_context_scroll",
                surface == PreviewSurface::Search,
            )),
            (id, rect),
        )
    });
}

// TextEdit's Widget implementation is show(ui).response. Keep the existing
// ui.put allocation/layout and return that very Response; only retain the
// local observational receipts alongside it.
fn put_files_text_edit(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    edit: egui::TextEdit<'_>,
) -> (egui::Response, Vec<egui::text_edit::TextEditImeReceipt>) {
    let mut receipts = Vec::new();
    let response = ui.put(rect, |ui: &mut egui::Ui| {
        let output = edit.show(ui);
        #[cfg(test)]
        crate::workspace_access_tests::record_text_edit_receipts(ui, &output);
        receipts = output.ime_receipts;
        output.response
    });
    (response, receipts)
}

#[cfg(test)]
#[path = "files_ime_widget_tests.rs"]
mod files_ime_widget_tests;

#[cfg(test)]
mod preview_gutter_tests {
    use super::*;
    use crate::workspace_access_tests::{begin, record, recorded_response};

    #[test]
    fn preview_gutter_parser_preserves_code_and_formatter_limits() {
        let context = project_text_context(" \t▶ 10 | 界😀K\r\n\r\nlast\n", 2);
        let original = context.text.clone();
        let rows = context_lines(&context).unwrap();
        assert_eq!(
            rows.iter()
                .map(|row| (row.number, row.code, row.target))
                .collect::<Vec<_>>(),
            [
                ("1", " \t▶ 10 | 界😀K", false),
                ("2", "", true),
                ("3", "last", false),
                ("4", "", false)
            ]
        );
        assert_eq!(context.text, original);
        let context = project_text_context(&"界".repeat(100_000), 1);
        assert!(context.text.len() <= MAX_PROJECT_TEXT_CONTEXT_BYTES);
        let rows = context_lines(&context).unwrap();
        assert!(rows[0].code.ends_with(" … [line truncated]"));
        assert_eq!(
            rows[0].code.as_bytes(),
            context.text.strip_prefix("▶ 1 | ").unwrap().as_bytes()
        );
        let text = (1..=200)
            .map(|line| format!("row {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        let context = project_text_context(&text, 100);
        let rows = context_lines(&context).unwrap();
        assert_eq!(rows.len(), 41);
        assert_eq!(rows.first().unwrap().number, "80");
        assert_eq!(rows.last().unwrap().number, "120");
        assert_eq!(rows.iter().filter(|row| row.target).count(), 1);
        assert!(context_lines(&project_text_context("one", 2))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn preview_gutter_parser_rejects_whole_malformed_context() {
        let huge_number = format!("▶ {}0 | code", usize::MAX);
        for text in [
            "▶ 0 | code",
            "▶ 01 | code",
            "▶ +1 | code",
            "▶ １ | code",
            "▶ 1| code",
            "▶ 1 |code",
            " ▶ 1 | code",
            "  1 | no marker",
            "▶ 1 | a\n▶ 2 | b",
            "▶ 1 | a\n  3 | b",
            "▶ 2 | a\n  1 | b",
            huge_number.as_str(),
        ] {
            let context = ProjectTextContext {
                text: text.into(),
                note: String::new(),
            };
            assert!(context_lines(&context).is_none(), "{text:?}");
        }
        let text = (1..=42)
            .map(|line| format!("{} {line} | code", if line == 1 { "▶" } else { " " }))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(context_lines(&ProjectTextContext {
            text,
            note: String::new()
        })
        .is_none());
        assert!(context_lines(&ProjectTextContext {
            text: format!("▶ 1 | {}", "x".repeat(MAX_PROJECT_TEXT_CONTEXT_BYTES)),
            note: String::new()
        })
        .is_none());
    }

    fn draw(app: &CedarApp, context: &ProjectTextContext, legacy: bool) -> egui::FullOutput {
        draw_events(app, context, legacy, Vec::new())
    }

    fn draw_events(
        app: &CedarApp,
        context: &ProjectTextContext,
        legacy: bool,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        let ctx = &app.editor_ctx;
        begin(ctx);
        ctx.run(
            egui::RawInput {
                events,
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(780.0, 540.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    if legacy {
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            for row in context_lines(context).unwrap() {
                                let response = ui.add(
                                    egui::Label::new(RichText::new(row.full).monospace())
                                        .selectable(false)
                                        .wrap_mode(egui::TextWrapMode::Extend),
                                );
                                record(
                                    ui,
                                    &format!("project_text_context_row:{}", row.number),
                                    &response,
                                );
                            }
                        });
                    } else {
                        draw_project_text_context(ui, context, false);
                    }
                });
            },
        )
    }

    #[test]
    fn preview_gutter_retains_label_ids_sense_height_and_accessibility() {
        let context = project_text_context("😀 K first\n\n▶ 10 | code", 2);
        for screen_reader in [false, true] {
            let legacy = CedarApp::empty();
            let candidate = CedarApp::empty();
            for app in [&legacy, &candidate] {
                app.editor_ctx.set_fonts(crate::system_fonts::base_fonts());
                app.editor_ctx
                    .memory_mut(|memory| memory.options.screen_reader = screen_reader);
            }
            for _ in 0..2 {
                draw(&legacy, &context, true);
                draw(&candidate, &context, false);
            }
            for line in 1..=3 {
                let probe = format!("project_text_context_row:{line}");
                let before = recorded_response(&legacy, &probe);
                let after = recorded_response(&candidate, &probe);
                assert_eq!(before.id, after.id, "one unchanged auto-ID per row");
                assert_eq!(before.sense, after.sense);
                assert_eq!(
                    after.sense,
                    if screen_reader {
                        egui::Sense::focusable_noninteractive()
                    } else {
                        egui::Sense::hover()
                    }
                );
                assert_eq!(before.rect.height(), after.rect.height());
            }
            if screen_reader {
                for line in 1..=3 {
                    let event = |pressed| egui::Event::Key {
                        key: egui::Key::Tab,
                        physical_key: Some(egui::Key::Tab),
                        pressed,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    };
                    let old_output = draw_events(&legacy, &context, true, vec![event(true)]);
                    let output = draw_events(&candidate, &context, false, vec![event(true)]);
                    assert_eq!(
                        output.platform_output.events,
                        old_output.platform_output.events
                    );
                    let infos = output
                        .platform_output
                        .events
                        .iter()
                        .filter_map(|event| {
                            if let egui::output::OutputEvent::FocusGained(info) = event {
                                Some(info)
                            } else {
                                None
                            }
                        })
                        .collect::<Vec<_>>();
                    assert_eq!(infos.len(), 1, "native Tab focus event for line {line}");
                    assert_eq!(infos[0].typ, egui::WidgetType::Label);
                    assert_eq!(
                        infos[0].label.as_deref(),
                        context.text.lines().nth(line - 1)
                    );
                    let probe = format!("project_text_context_row:{line}");
                    assert!(recorded_response(&candidate, &probe).has_focus());
                    assert_eq!(
                        legacy.editor_ctx.memory(|m| m.focused()),
                        candidate.editor_ctx.memory(|m| m.focused())
                    );
                    let old_release = draw_events(&legacy, &context, true, vec![event(false)]);
                    let release = draw_events(&candidate, &context, false, vec![event(false)]);
                    assert_eq!(
                        release.platform_output.events,
                        old_release.platform_output.events
                    );
                    assert!(release.platform_output.events.is_empty());
                }
            }
        }
    }

    #[test]
    fn preview_gutter_malformed_context_paints_all_original_lines() {
        let app = CedarApp::empty();
        let context = ProjectTextContext {
            text: "▶ 1 | Ω first\n  3 | ▶ 10 | second".into(),
            note: String::new(),
        };
        let output = draw(&app, &context, false);
        fn visit(shape: &egui::epaint::Shape, text: &mut Vec<String>) {
            match shape {
                egui::epaint::Shape::Text(draw) => text.push(draw.galley.text().into()),
                egui::epaint::Shape::Vec(shapes) => {
                    for shape in shapes {
                        visit(shape, text);
                    }
                }
                _ => {}
            }
        }
        let mut painted = Vec::new();
        for shape in &output.shapes {
            visit(&shape.shape, &mut painted);
        }
        assert_eq!(
            painted,
            context.text.lines().map(str::to_owned).collect::<Vec<_>>()
        );
    }
}
