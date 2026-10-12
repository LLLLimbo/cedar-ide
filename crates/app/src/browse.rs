//! Common, explicitly submitted Find and return entry. Provider adapters retain
//! authority; this view owns only selection, native input and one bounded preview.
use crate::{
    navigation, search_keyboard, CedarApp, ConnectionState, Job, WorkspaceKey, AMBER, MUTED,
};
use cedar_protocol::{
    FileDiscoveryIncompleteReason, Operation, Payload, DEFAULT_FILE_DISCOVERY_RESULTS,
    MAX_FILE_BYTES,
};
use eframe::egui::{self, RichText};
use std::{collections::HashSet, sync::Arc};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(super) enum Provider {
    #[default]
    Files,
    ProjectText,
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

#[derive(Clone, PartialEq, Eq)]
pub(super) struct PreviewTicket {
    context: Context,
    epoch: u64,
    provider: Provider,
    query: String,
    index: usize,
    row: Row,
    draft: Option<(u64, u64)>,
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
    Files,
    Text,
    Submit,
    Row(usize),
    Preview,
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
    id: egui::Id,
    rect: egui::Rect,
    clip: egui::Rect,
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
    composing: bool,
    key: Option<KeyIntent>,
    pressed: Option<Presented>,
    click: Option<Presented>,
    shortcut: Option<(Provider, u64, Option<WorkspaceKey>)>,
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
        self.click = None;
    }
}

#[derive(Default)]
pub(super) struct Browse {
    context: Option<Context>,
    provider: Provider,
    query: String,
    scope: String,
    submitted: String,
    discovery: Discovery,
    epoch: u64,
    exhausted: bool,
    snapshot: Option<navigation::LoadedSnapshot>,
    rows: Vec<Row>,
    selected: Option<usize>,
    truncated: bool,
    message: Option<String>,
    text_request: Option<u64>,
    search_completed: Option<(u64, String, Result<Payload, String>)>,
    desired: Option<PreviewTicket>,
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
    pub(super) fn scope(&self) -> &str {
        &self.scope
    }
    #[cfg(test)]
    pub(super) fn set_epoch_for_test(&mut self, epoch: u64) {
        self.epoch = epoch;
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
        self.rows.clear();
        self.selected = None;
        self.desired = None;
        self.preview = None;
        self.replacement = None;
        self.message = None;
        self.truncated = false;
        self.input.retire();
    }
    fn token(&self, target: Target) -> Token {
        let row = match target {
            Target::Row(index) => self.rows.get(index),
            Target::Preview | Target::Open | Target::PreviewPane => {
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
    pub(super) fn browse_retire(&mut self) {
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
        self.browse_current()
            && ticket.context.current(self)
            && ticket.epoch == self.browse.epoch
            && ticket.provider == self.browse.provider
            && ticket.query == self.browse.query
            && ticket.query == self.browse.submitted
            && self.browse.selected == Some(ticket.index)
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
        if !self.browse_current() {
            return;
        }
        if self.browse.provider == Provider::Files
            && self.browse.snapshot.is_none()
            && !self.backend_file_discovery_supported()
        {
            return;
        }
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
                        | Target::Files
                        | Target::Text
                        | Target::Submit
                        | Target::Close
                )
            })
            .cloned()
            .collect();
        self.browse.retire_view();
        if self.browse_refuse_exhaustion() {
            return;
        }
        for mut control in controls {
            control.token = self.browse.token(control.token.target.clone());
            self.browse.input.presented.push(control);
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
            Provider::Files => {
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
                } else if self.backend_file_discovery_supported() && !self.browse.scope.is_empty() {
                    self.browse.message = Some(
                        "Enter a filename query before searching this directory scope.".into(),
                    );
                } else if let Some(snapshot) = &self.browse.snapshot {
                    let (rows, truncated) = snapshot.matching(&self.browse.query);
                    self.browse.rows = rows.into_iter().map(Row::File).collect();
                    self.browse.truncated = truncated;
                    self.browse.selected = (!self.browse.rows.is_empty()).then_some(0);
                }
            }
            Provider::ProjectText => {
                self.search_state.retire();
                if self.next_request == 0 || self.next_request == u64::MAX {
                    self.browse.message = Some("Find cannot submit: the request identity counter is exhausted. Reopen Cedar to start a new session.".into());
                    return;
                }
                self.search_query = self.browse.query.clone();
                if !self.search_query.trim().is_empty() && self.backend_supports("search") {
                    self.search();
                    self.browse.text_request = self
                        .search_state
                        .pending
                        .as_ref()
                        .map(|source| source.request);
                } else {
                    self.browse.message = Some(
                        "Enter a case-sensitive literal query, then press Enter to search.".into(),
                    );
                }
            }
        }
    }
    fn browse_query_changed(&mut self) {
        if self.browse.text_request.take().is_some() {
            self.search_state.retire();
        }
        self.browse.retire_view();
        if self.browse_refuse_exhaustion() {
            return;
        }
        self.browse.submitted.clear();
        self.browse.message = Some("Press Enter to submit this query.".into());
    }
    pub(super) fn browse_select(&mut self, index: usize) {
        if !self.browse_current()
            || !self
                .browse
                .rows
                .get(index)
                .is_some_and(|row| self.browse_row_current(row))
        {
            return;
        }
        if self.browse.selected != Some(index) {
            self.browse.desired = None;
            self.browse.preview = None;
            self.browse.replacement = None;
        }
        self.browse.selected = Some(index);
        self.browse.focus = Some(Target::Row(index));
    }
    pub(super) fn browse_preview(&mut self) {
        let Some(context) = self.browse.context.clone() else {
            return;
        };
        let Some(index) = self.browse.selected else {
            return;
        };
        let Some(row) = self.browse.rows.get(index).cloned() else {
            return;
        };
        let draft = self
            .documents
            .iter()
            .find(|doc| doc.path == row.path())
            .map(|doc| (doc.id, doc.edit_version));
        let ticket = PreviewTicket {
            context,
            epoch: self.browse.epoch,
            provider: self.browse.provider,
            query: self.browse.query.clone(),
            index,
            row,
            draft,
        };
        if !self.browse_ticket_current(&ticket) {
            return;
        }
        if self.browse.desired.as_ref() == Some(&ticket) {
            self.browse.focus = Some(Target::PreviewPane);
            return;
        }
        self.browse.desired = Some(ticket.clone());
        self.browse.preview = None;
        self.browse.focus = Some(Target::PreviewPane);
        if !valid_preview_path(ticket.row.path()) {
            self.browse.preview = Some(Preview::Message { ticket, message: "Preview is unavailable because this result is not a valid workspace-relative file path.".into() });
            self.browse.replacement = None;
        } else if ticket.draft.is_some() {
            self.browse.preview = Some(
                if self
                    .documents
                    .iter()
                    .find(|doc| doc.path == ticket.row.path())
                    .is_some_and(|doc| doc.text.len() > MAX_FILE_BYTES)
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
            || !self.browse_row_current(&row)
        {
            return;
        }
        if !self.ready() && !self.documents.iter().any(|doc| doc.path == row.path()) {
            return;
        }
        // Ordinary Open alone owns draft reuse, text-hit navigation and history.
        self.open(row.path().to_owned(), row.line());
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
        query: String,
        result: Result<Payload, String>,
    ) {
        self.browse.search_completed = Some((request, query, result));
    }
    pub(super) fn browse_finish_frame(&mut self, ctx: &egui::Context) {
        self.finish_browse_discovery(ctx);
        let owns_view = self.browse_current() && !self.foreign_modal_owns_input(ctx);
        if let Some((request, query, result)) = self.browse.search_completed.take() {
            if owns_view
                && self.browse.text_request == Some(request)
                && self.browse.query == query
                && self.browse.submitted == query
            {
                self.apply_search_response(request, query, result);
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
            if owns_view
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
        if self.browse.flight.as_ref().is_some_and(|(id, ticket)| {
            ticket.context.generation != self.generation || !self.pending.contains_key(id)
        }) {
            self.browse.flight = None;
        }
        if self.browse.flight.is_none() {
            if let Some(ticket) = self.browse.replacement.take() {
                if owns_view
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

    // Capture from the same original native batch as the legacy surfaces. Never
    // infer clean ownership from events another component already removed.
    pub(super) fn capture_browse_input(
        &mut self,
        ctx: &egui::Context,
        raw: &egui::RawInput,
        consumed: &mut [bool],
    ) {
        let allowed = self.browse_current() && !self.foreign_modal_owns_input(ctx);
        let focused = ctx.memory(|memory| memory.focused());
        let input = &mut self.browse.input;
        input.pass = Some(ctx.cumulative_pass_nr());
        input.key = None;
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
            input.composing = false;
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
        let pointer_only = raw.events.iter().all(|event| {
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
        if !allowed
            || !pointer_only
            || was_composing
            || input.composing
            || !input.held.is_empty()
            || !raw.modifiers.is_none()
            || ctx.input(|state| !state.keys_down.is_empty())
        {
            input.pressed = None;
        } else {
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
                        continue;
                    }
                    if *pressed {
                        input.pressed = input
                            .presented
                            .iter()
                            .find(|item| item.rect.intersect(item.clip).contains(*pos))
                            .cloned();
                    } else if let Some(press) = input.pressed.take() {
                        if press.rect.intersect(press.clip).contains(*pos)
                            && input.presented.iter().any(|item| {
                                item.token == press.token
                                    && item.id == press.id
                                    && item.rect.intersect(item.clip).contains(*pos)
                            })
                        {
                            input.click = Some(press);
                        }
                    }
                }
            }
        }
        let mut space_claimed = false;
        for (index, event) in raw.events.iter().enumerate() {
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
            let global =
                shortcut.is_some() || (claimed && matches!(key, egui::Key::N | egui::Key::F));
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
                consumed[index] |= claimed && !global;
                continue;
            }
            let fresh = input.held.insert(*key);
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
                space_claimed |= *key == egui::Key::Space;
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
                self.show_browse(provider);
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
        if !ui.is_sizing_pass() {
            self.browse.input.presented.push(Presented {
                token: token.clone(),
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
                Target::Files => "browse_files".into(),
                Target::Text => "browse_text".into(),
                Target::Submit => "browse_submit".into(),
                Target::Row(index) => format!("browse_row:{index}"),
                Target::Preview => "browse_preview".into(),
                Target::Open => "browse_open".into(),
                Target::Close => "browse_close".into(),
                Target::PreviewPane => "browse_preview_content".into(),
            };
            crate::workspace_access_tests::record(ui, &name, response);
        }
        if !response.enabled() || ui.is_sizing_pass() || !self.browse_input_current(ui.ctx()) {
            return false;
        }
        let click = self
            .browse
            .input
            .click
            .as_ref()
            .is_some_and(|click| click.token == token && click.id == response.id)
            && response.clicked_by(egui::PointerButton::Primary);
        let key = self.browse.input.key.as_ref().is_some_and(|intent| {
            intent.source.token == token
                && intent.source.id == response.id
                && matches!(intent.key, egui::Key::Enter | egui::Key::Space)
                && ui.ctx().memory(|memory| memory.focused()) == Some(response.id)
        });
        if click {
            response.request_focus();
        }
        click || key
    }
    fn browse_button(
        &mut self,
        ui: &mut egui::Ui,
        target: Target,
        label: &str,
        enabled: bool,
    ) -> bool {
        let id = self.browse.id(&target);
        let response = ui
            .add_enabled_ui(enabled, |ui| {
                navigation::candidate_row(ui, id, false, label, false)
            })
            .inner;
        self.browse_response(ui, target, &response)
    }
    pub(super) fn browse_window(&mut self, ctx: &egui::Context) {
        if !self.browse_current() {
            self.dismiss_navigation();
            return;
        }
        if self.browse.provider == Provider::ProjectText
            && self.browse.rows.is_empty()
            && self.browse.text_request.is_some_and(|request| {
                self.search_state.accepted.as_ref().is_some_and(|source| {
                    source.request == request && source.query == self.browse.submitted
                })
            })
        {
            self.browse.rows = self
                .browse_search_hits()
                .into_iter()
                .map(Row::Text)
                .collect();
            self.browse.selected = (!self.browse.rows.is_empty()).then_some(0);
            self.browse.truncated = self.search_truncated;
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
        let mut open = false;
        let mut close = false;
        let mut switch = None;
        if let Some(intent) = &key {
            match (&intent.source.token.target, intent.key) {
                (_, egui::Key::Escape) => {
                    if self.browse.desired.take().is_some() {
                        self.browse.preview = None;
                        self.browse.replacement = None;
                        self.browse.focus = self.browse.selected.map(Target::Row);
                    } else {
                        close = true;
                    }
                }
                (Target::Query | Target::Scope, egui::Key::Enter) => submit = true,
                (Target::Query | Target::Scope, egui::Key::ArrowDown) => {
                    select = (!self.browse.rows.is_empty()).then_some(0)
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
                    select = search_keyboard::destination(self.browse.rows.len(), *index, key)
                }
                _ => {}
            }
        }
        if let Some(index) = select.take() {
            self.browse_select(index);
        }
        self.browse.input.presented.clear();
        let modal_id = egui::Id::new("navigation_modal");
        egui::Modal::new(modal_id).area(egui::Modal::default_area(modal_id).anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 24.0))).show(ctx, |ui| {
            ui.set_width((ctx.screen_rect().width() - 64.0).clamp(240.0, 860.0));
            ui.heading("Find and return");
            ui.horizontal(|ui| {
                if self.browse_button(ui, Target::Files, "Files", true) { switch = Some(Provider::Files); }
                if self.browse_button(ui, Target::Text, "Project Text", self.backend_supports("search")) { switch = Some(Provider::ProjectText); }
                if self.browse_button(ui, Target::Close, "Close · Esc", true) { close = true; }
            });
            let discovery_available = self.backend_file_discovery_supported();
            ui.label(RichText::new(match self.browse.provider {
                Provider::Files if discovery_available => "Project files · explicit filename search · empty query shows open and loaded files",
                Provider::Files => "Open and loaded files · project discovery unavailable · no folders are scanned",
                Provider::ProjectText => "Project Text · case-sensitive literal disk search",
            }).small().color(MUTED));
            ui.label(RichText::new(format!("Workspace: {}", self.root)).small().color(MUTED));
            let query_id = self.browse.id(&Target::Query);
            if self.browse.focus == Some(Target::Query) && !ui.is_sizing_pass() { ui.memory_mut(|memory| memory.request_focus(query_id)); }
            let query = ui.add(egui::TextEdit::singleline(&mut self.browse.query).id(query_id).char_limit(4096).desired_width(f32::INFINITY).hint_text("Enter query, then press Enter"));
            let changed = query.changed();
            if changed { self.browse_query_changed(); submit = false; select = None; preview = false; open = false; }
            self.browse_response(ui, Target::Query, &query);
            if self.browse.provider == Provider::Files {
                if discovery_available {
                    ui.horizontal(|ui| {
                        ui.label("Directory scope");
                        let id = self.browse.id(&Target::Scope);
                        let response = ui.add(egui::TextEdit::singleline(&mut self.browse.scope).id(id).char_limit(4096).desired_width(ui.available_width()).hint_text("Workspace-relative directory; empty searches workspace"));
                        if response.changed() { self.browse_query_changed(); submit = false; select = None; preview = false; open = false; }
                        self.browse_response(ui, Target::Scope, &response);
                    });
                    ui.label(RichText::new(format!("Excluded directories: {}. Literal matching uses Unicode lowercase, without normalization.", cedar_protocol::FILE_DISCOVERY_EXCLUDED_DIRECTORIES.join(", "))).small().color(MUTED));
                } else {
                    ui.label(RichText::new("Directory scope is unavailable for Open and loaded files.").small().color(MUTED));
                }
            }
            ui.horizontal(|ui| {
                submit |= self.browse_button(ui, Target::Submit, "Find", self.browse.provider == Provider::Files || self.backend_supports("search"));
                ui.label(RichText::new("Enter submits · ↓ selects · Space previews · Enter opens").small().color(MUTED));
            });
            if self.browse.provider == Provider::ProjectText && !self.browse.submitted.is_empty() {
                ui.label(RichText::new(format!("Historical results for {:?}. Lines may have changed; Open uses current source.", self.browse.submitted)).small().color(MUTED));
            }
            if self.browse.provider == Provider::ProjectText && self.browse.text_request.is_some() && self.search_state.pending.is_some() { ui.label("Searching…"); }
            if self.browse.provider == Provider::Files {
                if let Some(source) = &self.browse.discovery.accepted {
                    ui.label(RichText::new(format!("Project files for {:?} · scope /{} · {} entries examined", source.ticket.query, source.ticket.scope, source.examined)).small().color(MUTED));
                    if source.incomplete {
                        ui.colored_label(AMBER, format!("Incomplete scan: {}. A narrower directory scope can help with traversal limits.", source.reasons.iter().map(discovery_reason).collect::<Vec<_>>().join(", ")));
                    }
                } else if self.browse.discovery.pending_current() {
                    ui.label("Finding project files…");
                }
            }
            if let Some(message) = &self.browse.message { ui.colored_label(AMBER, message); }
            if self.browse.truncated { ui.colored_label(AMBER, if self.browse.provider == Provider::ProjectText { "Search incomplete; more matches may exist." } else { "Result limit reached; more matches may exist. Narrow the query." }); }
            let max_height = (ctx.screen_rect().height() - 400.0).clamp(70.0, 280.0);
            egui::ScrollArea::vertical().id_salt("browse_results").animated(false).max_height(max_height).show(ui, |ui| {
                // Candidate wrappers carry authority, not duplicated snippets.
                for index in 0..self.browse.rows.len() {
                    let row = self.browse.rows[index].clone();
                    let label = match &row {
                        Row::Project(candidate) => candidate.source.paths[candidate.index].clone(),
                        Row::File(candidate) => self.browse_loaded_label(candidate),
                        Row::Text(hit) => format!("{}:{}  {}", hit.path(), hit.line(), self.browse_search_snippet(hit).unwrap_or("").trim()),
                    };
                    let target = Target::Row(index); let id = self.browse.id(&target);
                    let current = self.browse_row_current(&row);
                    let response = ui.add_enabled_ui(current, |ui| navigation::candidate_row(ui, id, self.browse.selected == Some(index), &label, true)).inner;
                    if response.has_focus() { ui.memory_mut(|memory| memory.set_focus_lock_filter(id, egui::EventFilter { vertical_arrows: true, ..Default::default() })); }
                    if self.browse_response(ui, target, &response) {
                        select = Some(index);
                        if response.double_clicked_by(egui::PointerButton::Primary) { open = true; }
                    }
                }
                if self.browse.rows.is_empty() {
                    let empty = match &self.browse.discovery.accepted {
                        Some(source) if source.incomplete => "No matches observed in an incomplete scan; more files may exist.",
                        Some(_) => "No matching project files in the searched scope and exclusions.",
                        None => "No active results. Submit a query to find a file or text.",
                    };
                    ui.label(RichText::new(empty).color(MUTED));
                }
            });
            let selected = self.browse.selected.and_then(|index| self.browse.rows.get(index)).is_some_and(|row| self.browse_row_current(row));
            ui.horizontal(|ui| {
                preview |= self.browse_button(ui, Target::Preview, "Preview · Space", selected);
                open |= self.browse_button(ui, Target::Open, "Open · Enter", selected);
                ui.label(RichText::new("Explicit read-only preview · 1 MiB maximum").small().color(MUTED));
            });
            if self.browse.desired.is_some() {
                ui.separator();
                let text = self.browse.preview.as_ref().filter(|preview| self.browse.desired.as_ref() == Some(preview.ticket())).and_then(|preview| match preview {
                    Preview::Draft(ticket) => self.documents.iter().find(|doc| Some((doc.id, doc.edit_version)) == ticket.draft && doc.path == ticket.row.path()).map(|doc| doc.text.as_str()),
                    Preview::Disk { text, .. } => Some(text.as_str()), _ => None,
                });
                let title = match &self.browse.preview {
                    Some(Preview::Draft(_)) => "Current open draft · read-only preview".to_owned(),
                    Some(Preview::Disk { revision, .. }) => format!("Disk preview · revision {} · search coordinates may be historical", &revision[..8]),
                    Some(Preview::Message { message, .. }) => message.clone(),
                    None => "Preview requested…".to_owned(),
                };
                ui.label(RichText::new(title).small().color(MUTED));
                let id = self.browse.id(&Target::PreviewPane);
                let response = egui::ScrollArea::both().id_salt("browse_preview_scroll").max_height((ctx.screen_rect().height() - 400.0).clamp(45.0, 180.0)).show(ui, |ui| {
                    let content = ui.add(egui::Label::new(RichText::new(text.unwrap_or("No preview bytes available")).monospace()).selectable(false));
                    ui.interact(content.rect, id, egui::Sense::click())
                }).inner;
                self.browse_response(ui, Target::PreviewPane, &response);
                ui.label(RichText::new("Escape returns to the selected result").small().color(MUTED));
            }
        });
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
        } else {
            if let Some(index) = select {
                self.browse_select(index);
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
        self.browse.input.key = None;
        self.browse.input.click = None;
        if self.browse.focus.is_some() {
            ctx.request_repaint();
        }
    }
}

fn owns_key(target: &Target, key: egui::Key) -> bool {
    key == egui::Key::Escape
        || match target {
            Target::Query | Target::Scope => matches!(key, egui::Key::Enter | egui::Key::ArrowDown),
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
fn shortcut_provider(key: egui::Key, modifiers: egui::Modifiers) -> Option<Provider> {
    (modifiers.ctrl && modifiers.shift && !modifiers.alt && !modifiers.mac_cmd)
        .then_some(match key {
            egui::Key::N => Some(Provider::Files),
            egui::Key::F => Some(Provider::ProjectText),
            _ => None,
        })
        .flatten()
}
// One exact press, optional matching release and the native Space companion.
// Mixed text, paste, IME, pointer, repeated or additional edges never activate.
fn clean_press(events: &[egui::Event]) -> Option<(egui::Key, egui::Modifiers)> {
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

fn valid_preview_path(path: &str) -> bool {
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
