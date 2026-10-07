//! Cedar IDE — a native Rust frontend for a local or SSH workspace agent.
pub mod completion;
mod editor_state;
mod language_results;
mod language_sync;
mod language_ui;
mod model;
mod recovery;
mod recovery_actor;
#[cfg(test)]
mod recovery_tests;
mod recovery_ui;
mod run_ui;
mod syntax;
mod system_fonts;
mod worker;

use cedar_client::ConnectionSpec;
use cedar_protocol::{Entry, Operation, Payload, SearchMatch};
use eframe::egui::{self, Color32, FontId, RichText, Stroke};
use model::{cursor_location, find_ranges, language, line_start, parent_path, Document};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender},
};
use worker::{Command, Event, Worker};

const BG: Color32 = Color32::from_rgb(17, 21, 29);
const PANEL: Color32 = Color32::from_rgb(22, 28, 38);
const BORDER: Color32 = Color32::from_rgb(44, 53, 66);
const MUTED: Color32 = Color32::from_rgb(139, 155, 174);
const TEXT: Color32 = Color32::from_rgb(218, 227, 238);
const GREEN: Color32 = Color32::from_rgb(146, 215, 174);
const AMBER: Color32 = Color32::from_rgb(234, 185, 117);
const RED: Color32 = Color32::from_rgb(243, 150, 154);

#[derive(Clone, Copy, PartialEq, Eq)]
enum ConnectionState {
    Idle,
    Connecting,
    Ready,
    Disconnected,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    Search,
    Git,
    Run,
    Language,
}
#[derive(Clone)]
struct ConnectForm {
    ssh: bool,
    local_root: String,
    host: String,
    port: String,
    remote_root: String,
    agent: String,
    allow_run: bool,
}
impl Default for ConnectForm {
    fn default() -> Self {
        Self {
            ssh: false,
            local_root: std::env::args().nth(1).unwrap_or_else(|| {
                std::env::current_dir()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            }),
            host: String::new(),
            port: "22".into(),
            remote_root: String::new(),
            agent: "cedar-agent".into(),
            allow_run: false,
        }
    }
}
impl ConnectForm {
    fn key(&self) -> String {
        if self.ssh {
            format!(
                "ssh:{}:{}:{}:{}",
                self.host.trim(),
                self.port.trim(),
                self.remote_root.trim(),
                self.agent.trim()
            )
        } else {
            format!("local:{}", self.local_root.trim())
        }
    }
    fn spec(&self) -> Result<ConnectionSpec, String> {
        if self.ssh {
            let port = self
                .port
                .parse::<u16>()
                .map_err(|_| "Port must be between 1 and 65535")?;
            if port == 0
                || self.host.trim().is_empty()
                || self.remote_root.trim().is_empty()
                || self.agent.trim().is_empty()
            {
                return Err(
                    "Enter a host, remote workspace root, agent path, and valid port".into(),
                );
            }
            Ok(ConnectionSpec::Ssh {
                host: self.host.trim().into(),
                port,
                root: self.remote_root.trim().into(),
                agent_path: self.agent.trim().into(),
                allow_run: self.allow_run,
            })
        } else {
            if self.local_root.trim().is_empty() {
                return Err("Choose a local workspace directory".into());
            }
            Ok(ConnectionSpec::Local {
                root: PathBuf::from(self.local_root.trim()),
                allow_run: self.allow_run,
            })
        }
    }
    fn label(&self) -> String {
        if self.ssh {
            self.host.clone()
        } else {
            "Local workspace".into()
        }
    }
}

enum Job {
    List {
        path: String,
    },
    Open {
        path: String,
        line: Option<usize>,
        navigation: u64,
    },
    Save {
        document: u64,
        snapshot: String,
    },
    Search {
        query: String,
    },
    Git,
    Run(run_ui::Action),
    Inspect {
        path: String,
    },
    Language(language_ui::Action),
}

enum Confirm {
    CloseTab(u64),
    CloseWindow,
}

pub struct CedarApp {
    editor_ctx: egui::Context,
    system_fonts: system_fonts::SystemFonts,
    cjk_seen: bool,
    state: ConnectionState,
    form: ConnectForm,
    active_form: Option<ConnectForm>,
    workspace_key: Option<String>,
    connecting_form: Option<ConnectForm>,
    root: String,
    generation: u64,
    next_request: u64,
    next_document: u64,
    navigation_epoch: u64,
    worker: Option<Worker>,
    result_tx: Sender<Event>,
    result_rx: Receiver<Event>,
    pending: HashMap<u64, Job>,
    documents: Vec<Document>,
    active_document: Option<u64>,
    directory: String,
    entries: Vec<Entry>,
    directory_request: u64,
    open_form: bool,
    confirm: Option<Confirm>,
    allow_close: bool,
    close_after_language_stop: bool,
    close_snapshot: Option<Vec<(u64, u64)>>,
    error: Option<String>,
    notice: String,
    tool: Tool,
    tools_open: bool,
    search_query: String,
    search_results: Vec<SearchMatch>,
    search_truncated: bool,
    search_request: u64,
    git_output: String,
    run_program: String,
    run_args: String,
    run_timeout: u64,
    run_state: run_ui::RunPanel,
    language: language_ui::LanguagePanel,
    quick_open: bool,
    quick_path: String,
    quick_focus: bool,
    new_file: bool,
    new_path: String,
    find_open: bool,
    find_query: String,
    find_index: Option<usize>,
    find_focus: bool,
    disk_view: Option<(String, String)>,
    font_size: f32,
    recovery: recovery::Recovery,
}

impl CedarApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = PANEL;
        visuals.window_fill = PANEL;
        visuals.extreme_bg_color = BG;
        visuals.faint_bg_color = Color32::from_rgb(27, 35, 46);
        visuals.override_text_color = Some(TEXT);
        visuals.selection.bg_fill = Color32::from_rgb(51, 88, 77);
        visuals.selection.stroke = Stroke::new(1.0_f32, GREEN);
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER);
        visuals.widgets.inactive.bg_fill = Color32::from_rgb(34, 44, 57);
        visuals.widgets.hovered.bg_fill = Color32::from_rgb(43, 61, 68);
        visuals.widgets.active.bg_fill = Color32::from_rgb(51, 76, 75);
        cc.egui_ctx.set_visuals(visuals);
        cc.egui_ctx.style_mut(|style| {
            style.spacing.item_spacing = egui::vec2(9.0, 8.0);
            style.spacing.button_padding = egui::vec2(10.0, 6.0);
            style
                .text_styles
                .insert(egui::TextStyle::Body, FontId::proportional(14.0));
            style
                .text_styles
                .insert(egui::TextStyle::Button, FontId::proportional(13.0));
            style
                .text_styles
                .insert(egui::TextStyle::Monospace, FontId::monospace(14.0));
        });
        let mut app = Self::empty();
        app.editor_ctx = cc.egui_ctx.clone();
        app.recovery.start(
            cedar_recovery::default_store_path().map_err(|error| error.to_string()),
            &cc.egui_ctx,
        );
        app
    }

    fn empty() -> Self {
        let (result_tx, result_rx) = mpsc::channel();
        Self {
            editor_ctx: egui::Context::default(),
            system_fonts: system_fonts::SystemFonts::default(),
            cjk_seen: false,
            state: ConnectionState::Idle,
            form: ConnectForm::default(),
            active_form: None,
            workspace_key: None,
            connecting_form: None,
            root: String::new(),
            generation: 0,
            next_request: 1,
            next_document: 1,
            navigation_epoch: 0,
            worker: None,
            result_tx,
            result_rx,
            pending: HashMap::new(),
            documents: Vec::new(),
            active_document: None,
            directory: String::new(),
            entries: Vec::new(),
            directory_request: 0,
            open_form: true,
            confirm: None,
            allow_close: false,
            close_after_language_stop: false,
            close_snapshot: None,
            error: None,
            notice: "Ready when you are".into(),
            tool: Tool::Search,
            tools_open: false,
            search_query: String::new(),
            search_results: Vec::new(),
            search_truncated: false,
            search_request: 0,
            git_output: "Refresh to read workspace Git status".into(),
            run_program: String::new(),
            run_args: "[]".into(),
            run_timeout: 30,
            run_state: run_ui::RunPanel::default(),
            language: language_ui::LanguagePanel::default(),
            quick_open: false,
            quick_path: String::new(),
            quick_focus: false,
            new_file: false,
            new_path: String::new(),
            find_open: false,
            find_query: String::new(),
            find_index: None,
            find_focus: false,
            disk_view: None,
            font_size: 14.0,
            recovery: recovery::Recovery::default(),
        }
    }

    fn ready(&self) -> bool {
        self.state == ConnectionState::Ready
    }
    fn dirty(&self) -> bool {
        self.documents.iter().any(Document::dirty)
    }
    fn mutation_pending(&self) -> bool {
        self.pending
            .values()
            .any(|job| matches!(job, Job::Save { .. } | Job::Git | Job::Language(_)))
    }
    fn active(&self) -> Option<&Document> {
        self.documents
            .iter()
            .find(|doc| Some(doc.id) == self.active_document)
    }

    fn connect(&mut self, ctx: &egui::Context, form: ConnectForm) {
        if !self.guard_run_transition(run_ui::Transition::Reconnect) {
            return;
        }
        if self.mutation_pending() {
            self.error =
                Some("Wait for the current save, Git, command, or language request to finish before reconnecting".into());
            return;
        }
        if self.dirty() && self.workspace_key.as_deref() != Some(form.key().as_str()) {
            self.error = Some("Save or explicitly close your unsaved tabs before switching workspaces. Your drafts are still here".into());
            return;
        }
        let spec = match form.spec() {
            Ok(spec) => spec,
            Err(error) => {
                self.error = Some(error);
                return;
            }
        };
        self.recovery.restoring_generation = None;
        self.run_state.reset();
        self.worker = None;
        self.language.reset();
        self.generation += 1;
        self.pending.clear();
        for doc in &mut self.documents {
            doc.saving = false;
        }
        self.state = ConnectionState::Connecting;
        self.notice = format!("Connecting to {}...", form.label());
        self.error = None;
        self.connecting_form = Some(form);
        self.worker = Some(Worker::spawn(
            spec,
            self.generation,
            self.result_tx.clone(),
            ctx.clone(),
        ));
    }

    fn request(&mut self, op: Operation, job: Job) -> u64 {
        if !self.ready() {
            self.error =
                Some("Reconnect to the workspace first. Unsaved buffers are retained".into());
            return 0;
        }
        let id = self.next_request;
        self.next_request += 1;
        if self
            .worker
            .as_ref()
            .is_some_and(|worker| worker.tx.send(Command { id, op }).is_ok())
        {
            self.pending.insert(id, job);
            id
        } else {
            self.disconnected(
                "The connection worker stopped. Your buffers are still available".into(),
            );
            0
        }
    }

    fn disconnected(&mut self, message: String) {
        self.run_state.disconnected();
        self.recovery.restoring_generation = None;
        self.state = ConnectionState::Disconnected;
        self.language.reset();
        self.worker = None;
        self.pending.clear();
        for doc in &mut self.documents {
            doc.saving = false;
        }
        self.error = Some(message);
        self.notice = "Disconnected · drafts retained".into();
    }

    fn list(&mut self, path: String) {
        self.directory_request =
            self.request(Operation::List { path: path.clone() }, Job::List { path });
    }

    fn navigation_changed(&mut self) {
        self.navigation_epoch = self.navigation_epoch.wrapping_add(1);
        self.language.cancel_navigation();
    }

    fn open(&mut self, path: String, line: Option<usize>) {
        self.navigation_changed();
        let navigation = self.navigation_epoch;
        if let Some(doc) = self.documents.iter_mut().find(|doc| doc.path == path) {
            self.active_document = Some(doc.id);
            if let Some(line) = line {
                doc.jump_to = Some(line_start(&doc.text, line));
            }
            self.quick_open = false;
            return;
        }
        if self.documents.len() >= 32 {
            self.error = Some(
                "Close a tab before opening another. Cedar limits the workspace to 32 buffers"
                    .into(),
            );
            return;
        }
        for job in self.pending.values_mut() {
            if let Job::Open {
                path: pending,
                line: pending_line,
                navigation: pending_navigation,
            } = job
            {
                if pending == &path {
                    *pending_line = line;
                    *pending_navigation = navigation;
                    return;
                }
            }
        }
        self.request(
            Operation::Read { path: path.clone() },
            Job::Open {
                path,
                line,
                navigation,
            },
        );
        self.quick_open = false;
    }

    fn save(&mut self) {
        let Some(doc) = self.active() else {
            return;
        };
        if !doc.dirty() || doc.saving {
            return;
        }
        if doc.text.len() > cedar_protocol::MAX_FILE_BYTES {
            self.error = Some("This draft exceeds the 1 MiB file limit. Your text is retained; shorten it or copy it before saving".into());
            return;
        }
        let (id, path, text, revision) = (
            doc.id,
            doc.path.clone(),
            doc.text.clone(),
            doc.revision.clone(),
        );
        let request = self.request(
            Operation::Write {
                path,
                text: text.clone(),
                expected_revision: revision,
            },
            Job::Save {
                document: id,
                snapshot: text,
            },
        );
        if request != 0 {
            if let Some(doc) = self.documents.iter_mut().find(|doc| doc.id == id) {
                doc.saving = true;
            }
        }
    }

    fn poll(&mut self) {
        while let Ok(event) = self.result_rx.try_recv() {
            self.apply_event(event);
        }
    }

    fn apply_event(&mut self, event: Event) {
        if event.generation != self.generation {
            return;
        }
        if event.id == 0 {
            match event.result {
                Ok(Payload::Hello { root, .. }) => {
                    let Some(form) = self.connecting_form.take() else {
                        return;
                    };
                    if self.recovery.restoring_generation == Some(self.generation)
                        && self.recovery.pending_restore.as_ref().is_some_and(|draft| {
                            draft.workspace != recovery_ui::identity(&form, &root)
                        })
                    {
                        self.recovery.error = Some("The agent returned a different workspace root. Recovery was not restored; verify the intended workspace and retry".into());
                        self.disconnected("Recovery workspace identity did not match".into());
                        return;
                    }
                    if self.workspace_key.as_deref() == Some(form.key().as_str())
                        && !self.root.is_empty()
                        && self.root != root
                        && self.dirty()
                    {
                        self.disconnected("The workspace root changed while reconnecting. Your drafts are retained; reconnect to their original root before saving".into());
                        return;
                    }
                    if self.workspace_key.as_deref() != Some(form.key().as_str())
                        || (!self.root.is_empty() && self.root != root)
                    {
                        if self.dirty() {
                            self.disconnected("Workspace switch cancelled because a draft changed while connecting. Your edits are retained; reconnect to the original workspace to save them".into());
                            return;
                        }
                        for doc in &self.documents {
                            editor_state::forget(&self.editor_ctx, doc.id);
                        }
                        self.documents.clear();
                        self.active_document = None;
                        self.search_results.clear();
                        self.git_output = "Refresh to read workspace Git status".into();
                        self.run_state.output = "Command output will appear here".into();
                    }
                    self.workspace_key = Some(form.key());
                    self.active_form = Some(form);
                    self.root = root;
                    self.state = ConnectionState::Ready;
                    self.error = None;
                    self.notice = "Workspace connected".into();
                    self.open_form = false;
                    self.directory.clear();
                    self.entries.clear();
                    self.list(String::new());
                    if self.recovery.restoring_generation.take() == Some(self.generation) {
                        if let Some(draft) = self.recovery.pending_restore.take() {
                            if let Err(error) = self.install_recovered(draft.clone()) {
                                self.recovery.error = Some(error);
                                self.recovery.pending_restore = Some(draft);
                            }
                        }
                    }
                }
                Ok(_) => self.disconnected("Unexpected connection handshake".into()),
                Err(error) => self.disconnected(error),
            }
            return;
        }
        let Some(job) = self.pending.remove(&event.id) else {
            return;
        };
        if let Job::Save { document, .. } = &job {
            if let Some(doc) = self.documents.iter_mut().find(|doc| doc.id == *document) {
                doc.saving = false;
            }
        }
        let payload = match event.result {
            Ok(payload) => payload,
            Err(error) => {
                if matches!(&job, Job::Language(action) if action.is_stop()) {
                    self.close_after_language_stop = false;
                    self.close_snapshot = None;
                }
                if let Job::Language(action) = &job {
                    self.language_error(action, &error);
                }
                if let Job::Run(action) = &job {
                    self.run_error(action, event.connected, &error);
                }
                if !event.connected {
                    self.disconnected(error);
                } else {
                    self.error = Some(error);
                }
                return;
            }
        };
        match (job, payload) {
            (Job::List { path }, Payload::Entries { mut entries })
                if event.id == self.directory_request =>
            {
                entries.sort_by(|a, b| {
                    b.is_dir
                        .cmp(&a.is_dir)
                        .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                });
                self.directory = path;
                self.cjk_seen |= entries
                    .iter()
                    .any(|entry| system_fonts::contains_cjk(&entry.name));
                self.entries = entries;
            }
            (Job::List { .. }, Payload::Entries { .. }) => {}
            (
                Job::Open {
                    path: requested,
                    line,
                    navigation,
                },
                Payload::File {
                    path,
                    text,
                    revision,
                },
            ) => {
                if path != requested {
                    self.error = Some(
                        "Agent returned a different file path; the response was ignored".into(),
                    );
                    return;
                }
                if let Some(doc) = self.documents.iter_mut().find(|doc| doc.path == path) {
                    if navigation == self.navigation_epoch {
                        self.active_document = Some(doc.id);
                        if let Some(line) = line {
                            doc.jump_to = Some(line_start(&doc.text, line));
                        }
                    }
                    self.complete_language_navigation(&requested);
                    return;
                }
                if self.documents.len() >= 32 {
                    self.error = Some(
                        "The 32-buffer limit was reached; close a tab and open the file again"
                            .into(),
                    );
                    return;
                }
                let id = self.next_document;
                self.next_document += 1;
                let mut doc = Document::new(id, path, text, revision);
                if let Some(line) = line {
                    doc.jump_to = Some(line_start(&doc.text, line));
                }
                self.documents.push(doc);
                if navigation == self.navigation_epoch {
                    self.active_document = Some(id);
                    self.open_form = false;
                }
                self.complete_language_navigation(&requested);
            }
            (Job::Save { document, snapshot }, Payload::Written { revision }) => {
                let workspace = self.recovery_workspace();
                if let Some(doc) = self.documents.iter_mut().find(|doc| doc.id == document) {
                    doc.acknowledge_save(snapshot, revision);
                    if let Some(workspace) = &workspace {
                        self.recovery.saved(workspace, doc);
                    }
                    self.notice = format!("Saved {}", doc.path);
                }
                self.list(self.directory.clone());
            }
            (Job::Search { query }, Payload::Matches { matches, truncated })
                if event.id == self.search_request =>
            {
                self.notice = format!("{} matches for {query:?}", matches.len());
                self.search_results = matches;
                self.search_truncated = truncated;
            }
            (Job::Search { .. }, Payload::Matches { .. }) => {}
            (Job::Git, Payload::GitStatus { text }) => {
                self.git_output = if text.trim().is_empty() {
                    "Working tree clean".into()
                } else {
                    text
                };
            }
            (Job::Run(action), Payload::RunTask { snapshot }) => self.apply_run(action, snapshot),
            (Job::Run(action), _) => self.run_error(&action, true, "Unexpected command response"),
            (Job::Language(action), Payload::Language { value }) => {
                self.apply_language_action(action, value)
            }
            (Job::Inspect { path }, Payload::File { text, .. }) => {
                self.disk_view = Some((path, text))
            }
            _ => {
                self.error =
                    Some("Unexpected agent response; no editor buffers were changed".into())
            }
        }
    }

    fn search(&mut self) {
        if self.search_query.trim().is_empty() {
            return;
        }
        let query = self.search_query.clone();
        self.search_request = self.request(
            Operation::Search {
                query: query.clone(),
                limit: 500,
            },
            Job::Search { query },
        );
    }
    fn git(&mut self) {
        if !self.active_form.as_ref().is_some_and(|form| form.allow_run) {
            self.error = Some("Git status requires trusted command execution because repository filters may run code".into());
            return;
        }
        self.request(Operation::GitStatus, Job::Git);
    }
    fn finish_pending_close(&mut self, ctx: &egui::Context) {
        if self.close_after_language_stop && !self.language.running && !self.language_busy() {
            self.close_after_language_stop = false;
            let current: Vec<_> = self
                .documents
                .iter()
                .map(|doc| (doc.id, doc.edit_version))
                .collect();
            if self.close_snapshot.take().as_ref() != Some(&current) && self.dirty() {
                self.confirm = Some(Confirm::CloseWindow);
                self.notice = "A draft changed while the language server was stopping; confirm before quitting".into();
            } else {
                self.finish_recovery_close(ctx);
            }
        }
    }

    fn begin_close(&mut self, ctx: &egui::Context) {
        if !self.guard_run_transition(run_ui::Transition::Close) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            return;
        }
        if self.language.running && self.ready() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.close_after_language_stop = true;
            self.close_snapshot = Some(
                self.documents
                    .iter()
                    .map(|doc| (doc.id, doc.edit_version))
                    .collect(),
            );
            self.stop_language();
            self.notice = "Stopping language server before closing".into();
        } else {
            self.finish_recovery_close(ctx);
        }
    }

    fn close_tab(&mut self, id: u64) {
        if self.documents.iter().any(|doc| doc.id == id && doc.saving) {
            self.error =
                Some("This file is saving. Wait for the acknowledgement before closing it".into());
            return;
        }
        if self.documents.iter().any(|doc| doc.id == id && doc.dirty()) {
            self.confirm = Some(Confirm::CloseTab(id));
        } else {
            self.remove_tab(id);
        }
    }
    fn remove_tab(&mut self, id: u64) {
        if let Some(workspace) = self.recovery_workspace() {
            if let Some(doc) = self.documents.iter().find(|doc| doc.id == id) {
                self.recovery.discard_owned(&workspace, doc);
            }
        }
        if self.active_document == Some(id) {
            self.navigation_changed();
        }
        self.close_language_document(id);
        editor_state::forget(&self.editor_ctx, id);
        let position = self
            .documents
            .iter()
            .position(|doc| doc.id == id)
            .unwrap_or(0);
        self.documents.retain(|doc| doc.id != id);
        if self.active_document == Some(id) {
            self.active_document = self
                .documents
                .get(position.min(self.documents.len().saturating_sub(1)))
                .map(|doc| doc.id);
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        self.language_shortcuts(ctx);
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::S)) {
            self.save();
        }
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::P)) {
            self.quick_open = true;
            self.quick_focus = true;
        }
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::F)) {
            self.find_open = true;
            self.find_focus = true;
        }
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::W)) {
            if let Some(id) = self.active_document {
                self.close_tab(id);
            }
        }
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            self.quick_open = false;
            self.find_open = false;
            self.confirm = None;
            self.new_file = false;
            if !self.documents.is_empty() {
                self.open_form = false;
            }
        }
    }

    fn header(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("header")
            .exact_height(54.0)
            .frame(egui::Frame::new().fill(PANEL).inner_margin(12.0))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    logo(ui, 27.0);
                    ui.label(RichText::new("CEDAR").strong().size(17.0).color(TEXT));
                    ui.label(RichText::new("IDE").size(12.0).color(MUTED));
                    ui.add_space(15.0);
                    ui.separator();
                    let name = if self.root.is_empty() {
                        "No workspace".to_owned()
                    } else {
                        self.root
                            .rsplit(['/', '\\'])
                            .find(|part| !part.is_empty())
                            .unwrap_or(&self.root)
                            .to_owned()
                    };
                    ui.label(RichText::new(name).color(MUTED))
                        .on_hover_text(&self.root);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let can_save = self.ready()
                            && self.active().is_some_and(|doc| doc.dirty() && !doc.saving);
                        if ui
                            .add_enabled(
                                can_save,
                                egui::Button::new(RichText::new("Save").color(GREEN)),
                            )
                            .on_hover_text("Save file · Ctrl/Cmd+S")
                            .clicked()
                        {
                            self.save();
                        }
                        if ui.button("Recovery").clicked() {
                            self.recovery.visible = true;
                        }
                        if ui.button("Open workspace").clicked() {
                            self.open_form = true;
                        }
                        if self.active_form.is_some()
                            && self.state != ConnectionState::Connecting
                            && ui
                                .add_enabled(
                                    !self.mutation_pending(),
                                    egui::Button::new("Reconnect"),
                                )
                                .clicked()
                        {
                            if let Some(form) = self.active_form.clone() {
                                self.connect(ctx, form);
                            }
                        }
                        if self.ready()
                            && ui
                                .button("Open file")
                                .on_hover_text("Open by path · Ctrl/Cmd+P")
                                .clicked()
                        {
                            self.quick_open = true;
                            self.quick_focus = true;
                        }
                    });
                });
            });
    }

    fn footer(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status")
            .exact_height(29.0)
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(25, 38, 38))
                    .inner_margin(egui::Margin::symmetric(12, 5)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let (color, label) = match self.state {
                        ConnectionState::Idle => (MUTED, "Not connected"),
                        ConnectionState::Connecting => (AMBER, "Connecting"),
                        ConnectionState::Ready => (GREEN, "Connected"),
                        ConnectionState::Disconnected => (RED, "Disconnected"),
                    };
                    ui.colored_label(color, label);
                    let workspace = self.recovery_workspace();
                    let (recovery_status, protected) = self.recovery.status(workspace.as_ref(), self.active());
                    if ui.small_button(RichText::new(recovery_status).color(if protected { GREEN } else { AMBER })).on_hover_text("Private recovery on this computer. Click to review copies and settings").clicked() { self.recovery.visible = true; }
                    if let Some(form) = &self.active_form {
                        ui.label(
                            RichText::new(if form.ssh {
                                format!("SSH · {}", form.host)
                            } else {
                                "Local".into()
                            })
                            .small()
                            .color(MUTED),
                        );
                    }
                    if !self.pending.is_empty() {
                        ui.spinner();
                        ui.label(RichText::new(format!("{} pending", self.pending.len())).small());
                    }
                    if ui.available_width() > 690.0 {
                        ui.add_sized(
                            [350.0, 18.0],
                            egui::Label::new(RichText::new(&self.notice).small().color(MUTED))
                                .truncate(),
                        );
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if let Some(doc) = self.active() {
                            ui.label(RichText::new("UTF-8").small().color(MUTED));
                            ui.label(RichText::new(language(&doc.path)).small().color(MUTED));
                            ui.label(
                                RichText::new(format!("Ln {}, Col {}", doc.cursor.0, doc.cursor.1))
                                    .small()
                                    .color(MUTED),
                            );
                        } else {
                            ui.label(
                                RichText::new("Native Rust · independent open-source prototype")
                                    .small()
                                    .color(MUTED),
                            );
                        }
                    });
                });
            });
    }

    fn notifications(&mut self, ctx: &egui::Context) {
        if let Some(error) = self.error.clone() {
            egui::TopBottomPanel::top("error")
                .frame(
                    egui::Frame::new()
                        .fill(Color32::from_rgb(55, 32, 39))
                        .inner_margin(10.0),
                )
                .show(ctx, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.colored_label(RED, error);
                        if ui.small_button("Dismiss").clicked() {
                            self.error = None;
                        }
                    });
                });
        }
    }

    fn sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("explorer").default_width(246.0).width_range(180.0..=460.0).resizable(true)
            .frame(egui::Frame::new().fill(PANEL).inner_margin(12.0)).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("EXPLORER").size(11.0).strong().color(MUTED));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add_enabled(self.ready(), egui::Button::new("R").small()).on_hover_text("Refresh directory").clicked() { self.list(self.directory.clone()); }
                        if ui.add_enabled(self.ready(), egui::Button::new("+").small()).on_hover_text("New UTF-8 file").clicked() {
                            self.new_file = true; self.new_path = if self.directory.is_empty() { String::new() } else { format!("{}/", self.directory) };
                        }
                    });
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.add_enabled(self.ready() && !self.directory.is_empty(), egui::Button::new("Up").small()).on_hover_text("Parent directory").clicked() { self.list(parent_path(&self.directory)); }
                    ui.add(egui::Label::new(RichText::new(if self.directory.is_empty() { "/" } else { &self.directory }).monospace().size(12.0)).truncate()).on_hover_text(&self.directory);
                });
                ui.separator();
                let mut open = None;
                egui::ScrollArea::vertical().id_salt("explorer_scroll").max_height((ui.available_height() - 120.0).max(40.0)).show(ui, |ui| {
                    if self.entries.is_empty() { ui.label(RichText::new(if self.ready() { "This directory is empty" } else { "Connect a workspace to browse files" }).small().color(MUTED)); }
                    for entry in &self.entries {
                        let selected = self.active().is_some_and(|doc| doc.path == entry.path);
                        let color = if entry.is_dir { AMBER } else if syntax::supports(&entry.path) { GREEN } else { TEXT };
                        let icon = if entry.is_dir { "+" } else { "·" };
                        let response = ui.add_enabled(self.ready(), egui::Button::new(RichText::new(format!("{icon}  {}", entry.name)).color(color)).selected(selected).frame(selected).min_size(egui::vec2(ui.available_width(), 27.0)));
                        if response.on_hover_text(&entry.path).clicked() { open = Some(entry.clone()); }
                    }
                });
                if let Some(entry) = open { if entry.is_dir { self.list(entry.path); } else { self.open(entry.path, None); } }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.label(RichText::new("Ctrl/Cmd+P  Open a path\nCtrl/Cmd+F  Find in file\nCtrl/Cmd+S  Save changes").size(11.0).color(MUTED));
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.selectable_label(self.tools_open && self.tool == Tool::Search, "Search").clicked() { self.tool = Tool::Search; self.tools_open = true; }
                        if ui.selectable_label(self.tools_open && self.tool == Tool::Git, "Git").clicked() { self.tool = Tool::Git; self.tools_open = true; if self.ready() && self.active_form.as_ref().is_some_and(|form| form.allow_run) { self.git(); } }
                        if ui.selectable_label(self.tools_open && self.tool == Tool::Run, "Run").clicked() { self.tool = Tool::Run; self.tools_open = true; }
                        if ui.selectable_label(self.tools_open && self.tool == Tool::Language, "LSP").clicked() { self.tool = Tool::Language; self.tools_open = true; }
                    });
                });
            });
    }

    fn tools(&mut self, ctx: &egui::Context) {
        if !self.tools_open {
            return;
        }
        egui::TopBottomPanel::bottom("tools").default_height(245.0).height_range(150.0..=500.0).resizable(true)
            .frame(egui::Frame::new().fill(PANEL).inner_margin(12.0)).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui.selectable_label(self.tool == Tool::Search, "PROJECT SEARCH").clicked() { self.tool = Tool::Search; }
                    if ui.selectable_label(self.tool == Tool::Git, "GIT STATUS").clicked() { self.tool = Tool::Git; }
                    if ui.selectable_label(self.tool == Tool::Run, "COMMANDS").clicked() { self.tool = Tool::Run; }
                    if ui.selectable_label(self.tool == Tool::Language, "LANGUAGE").clicked() { self.tool = Tool::Language; }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("x").on_hover_text("Hide tools").clicked() { self.tools_open = false; }
                    });
                });
                ui.separator();
                match self.tool {
                    Tool::Language => self.language_panel(ui),
                    Tool::Search => {
                        ui.horizontal(|ui| {
                            let edit = ui.add(egui::TextEdit::singleline(&mut self.search_query).hint_text("Find text across the workspace...").desired_width((ui.available_width() - 175.0).max(180.0)));
                            let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                            if ui.add_enabled(self.ready(), egui::Button::new("Search")).clicked() || enter { self.search(); }
                            ui.label(RichText::new(format!("{} results", self.search_results.len())).small().color(MUTED));
                        });
                        if self.search_truncated { ui.colored_label(AMBER, "Results capped at 500. Narrow your query to see more specific matches"); }
                        let mut open = None;
                        egui::ScrollArea::vertical().id_salt("search_results").show(ui, |ui| {
                            for result in &self.search_results {
                                ui.horizontal(|ui| {
                                    if ui.add(egui::Button::new(RichText::new(format!("{}:{}", result.path, result.line)).color(GREEN)).frame(false)).clicked() { open = Some((result.path.clone(), result.line)); }
                                    ui.add(egui::Label::new(RichText::new(result.text.trim()).monospace().size(12.0).color(MUTED)).truncate());
                                });
                            }
                            if self.search_results.is_empty() { ui.label(RichText::new("Case-sensitive text search. Open a result to jump to its line").small().color(MUTED)); }
                        });
                        if let Some((path, line)) = open { self.open(path, Some(line)); }
                    }
                    Tool::Git => {
                        let allowed = self.active_form.as_ref().is_some_and(|form| form.allow_run);
                        ui.horizontal(|ui| {
                            if ui.add_enabled(self.ready() && allowed, egui::Button::new("Refresh status")).clicked() { self.git(); }
                            ui.label(RichText::new("Porcelain status · requires trusted command permission").small().color(MUTED));
                        });
                        if !allowed { ui.colored_label(AMBER, "Enable trusted command execution and reconnect. Git may execute repository-configured filters."); }
                        egui::ScrollArea::both().id_salt("git_output").show(ui, |ui| { ui.add(egui::TextEdit::multiline(&mut self.git_output).font(egui::TextStyle::Monospace).desired_width(f32::INFINITY).interactive(false).frame(false)); });
                    }
                    Tool::Run => self.run_panel(ui),
                }
            });
    }

    fn connection_fields(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.form.ssh, false, "Local folder");
            ui.selectable_value(&mut self.form.ssh, true, "Remote over SSH");
        });
        ui.add_space(10.0);
        if self.form.ssh {
            field(
                ui,
                "SSH host",
                &mut self.form.host,
                "user@hostname or an SSH config alias",
            );
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new("Port").small().color(MUTED));
                    ui.add(egui::TextEdit::singleline(&mut self.form.port).desired_width(75.0));
                });
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new("Remote agent executable")
                            .small()
                            .color(MUTED),
                    );
                    ui.add(egui::TextEdit::singleline(&mut self.form.agent).desired_width(350.0));
                });
            });
            field(
                ui,
                "Remote workspace directory",
                &mut self.form.remote_root,
                "/home/you/project",
            );
            ui.label(RichText::new("Uses your system OpenSSH and existing key/config. The remote agent must be installed on a POSIX host. Authenticate and verify its host key in your terminal first.").small().color(MUTED));
        } else {
            field(
                ui,
                "Workspace directory",
                &mut self.form.local_root,
                "/path/to/project",
            );
        }
        ui.add_space(10.0);
        ui.checkbox(
            &mut self.form.allow_run,
            "Trust this workspace for Git, language servers, and commands",
        );
        ui.label(RichText::new("Git, language servers, builds, and commands can run repository code with your account’s permissions. Enable only for a workspace you trust.").small().color(if self.form.allow_run { AMBER } else { MUTED }));
        ui.add_space(15.0);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    self.state != ConnectionState::Connecting && !self.mutation_pending(),
                    egui::Button::new(
                        RichText::new(if self.state == ConnectionState::Connecting {
                            "Connecting..."
                        } else {
                            "Connect workspace"
                        })
                        .strong()
                        .color(BG),
                    )
                    .fill(GREEN)
                    .min_size(egui::vec2(195.0, 38.0)),
                )
                .clicked()
            {
                self.connect(ctx, self.form.clone());
            }
            if self.state == ConnectionState::Connecting {
                ui.spinner();
                if ui.button("Cancel").clicked() {
                    self.worker = None;
                    self.generation += 1;
                    self.connecting_form = None;
                    self.state = if self.workspace_key.is_some() {
                        ConnectionState::Disconnected
                    } else {
                        ConnectionState::Idle
                    };
                    self.notice = "Connection cancelled".into();
                }
            }
        });
    }

    fn welcome(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.add_space((ui.available_height() * 0.10).clamp(16.0, 70.0));
            let width = ui.available_width().min(600.0);
            ui.horizontal(|ui| {
                ui.add_space(((ui.available_width() - width) / 2.0).max(0.0));
                ui.vertical(|ui| {
                    ui.set_max_width(width);
                    logo(ui, 55.0);
                    ui.add_space(18.0);
                    ui.label(
                        RichText::new("A little closer to your code.")
                            .size(30.0)
                            .strong(),
                    );
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new("Native on your desktop. At home on your server.")
                            .size(16.0)
                            .color(MUTED),
                    );
                    ui.add_space(28.0);
                    if self.ready() && !self.open_form {
                        ui.colored_label(GREEN, "Your workspace is connected");
                        ui.label(&self.root);
                        ui.add_space(12.0);
                        ui.horizontal(|ui| {
                            if ui.button("Open a file  ·  Ctrl/Cmd+P").clicked() {
                                self.quick_open = true;
                                self.quick_focus = true;
                            }
                            if ui.button("New file").clicked() {
                                self.new_file = true;
                            }
                        });
                        ui.add_space(16.0);
                        ui.label(
                            RichText::new(
                                "Browse folders on the left, or search the whole project below.",
                            )
                            .color(MUTED),
                        );
                    } else {
                        egui::Frame::new()
                            .fill(PANEL)
                            .stroke(Stroke::new(1.0_f32, BORDER))
                            .corner_radius(10)
                            .inner_margin(22.0)
                            .show(ui, |ui| {
                                ui.set_width((width - 44.0).max(280.0));
                                self.connection_fields(ui, ctx);
                            });
                    }
                    ui.add_space(25.0);
                    ui.label(
                        RichText::new(
                            "Built in Rust · No browser engine · Private draft recovery on this computer",
                        )
                        .size(12.0)
                        .color(MUTED),
                    );
                    ui.label(
                        RichText::new(
                            "Independent open-source prototype. Not affiliated with JetBrains.",
                        )
                        .size(11.0)
                        .color(MUTED),
                    );
                });
            });
        });
    }

    fn find_bar(&mut self, ui: &mut egui::Ui) {
        if !self.find_open {
            return;
        }
        let mut next = false;
        let mut previous = false;
        ui.horizontal(|ui| {
            ui.label(RichText::new("FIND").small().color(MUTED));
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.find_query)
                    .hint_text("Case-sensitive text")
                    .desired_width(230.0),
            );
            if self.find_focus {
                response.request_focus();
                self.find_focus = false;
            }
            if response.changed() {
                self.find_index = None;
            }
            if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                next = true;
            }
            previous = ui.small_button("Previous").clicked();
            next |= ui.small_button("Next").clicked();
            let hits = self
                .active()
                .map(|doc| find_ranges(&doc.text, &self.find_query).len())
                .unwrap_or(0);
            ui.label(
                RichText::new(if hits >= model::MAX_FIND_MATCHES {
                    format!("First {hits} matches")
                } else {
                    format!("{hits} matches")
                })
                .small()
                .color(MUTED),
            );
            if ui.small_button("x").clicked() {
                self.find_open = false;
            }
        });
        if next || previous {
            if let Some(doc) = self
                .documents
                .iter_mut()
                .find(|doc| Some(doc.id) == self.active_document)
            {
                let hits = find_ranges(&doc.text, &self.find_query);
                if !hits.is_empty() {
                    let index = match self.find_index {
                        None => {
                            if previous {
                                hits.len() - 1
                            } else {
                                0
                            }
                        }
                        Some(current) => {
                            if previous {
                                (current + hits.len() - 1) % hits.len()
                            } else {
                                (current + 1) % hits.len()
                            }
                        }
                    };
                    self.find_index = Some(index);
                    let range = &hits[index];
                    doc.scroll_to = Some(range.start);
                    let id = egui::Id::new(("editor", doc.id));
                    let mut state = egui::TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
                    state
                        .cursor
                        .set_char_range(Some(egui::text::CCursorRange::two(
                            egui::text::CCursor::new(range.start),
                            egui::text::CCursor::new(range.end),
                        )));
                    state.store(ui.ctx(), id);
                    ui.ctx().memory_mut(|memory| memory.request_focus(id));
                }
            }
        }
        ui.separator();
    }

    fn editor(&mut self, ui: &mut egui::Ui) {
        let mut activate = None;
        let mut close = None;
        egui::ScrollArea::horizontal()
            .id_salt("tabs")
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for doc in &self.documents {
                        let active = Some(doc.id) == self.active_document;
                        egui::Frame::new()
                            .fill(if active {
                                Color32::from_rgb(35, 45, 57)
                            } else {
                                BG
                            })
                            .inner_margin(egui::Margin::symmetric(9, 5))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    let name = doc.path.rsplit('/').next().unwrap_or(&doc.path);
                                    let suffix = if doc.saving {
                                        "  ~"
                                    } else if doc.dirty() {
                                        "  *"
                                    } else {
                                        ""
                                    };
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                RichText::new(format!("{name}{suffix}"))
                                                    .color(if active { GREEN } else { MUTED }),
                                            )
                                            .frame(false),
                                        )
                                        .on_hover_text(&doc.path)
                                        .clicked()
                                    {
                                        activate = Some(doc.id);
                                    }
                                    if ui
                                        .add(
                                            egui::Button::new(RichText::new("x").color(MUTED))
                                                .small()
                                                .frame(false),
                                        )
                                        .on_hover_text("Close tab · Ctrl/Cmd+W")
                                        .clicked()
                                    {
                                        close = Some(doc.id);
                                    }
                                });
                            });
                    }
                });
            });
        if let Some(id) = activate {
            self.navigation_changed();
            self.active_document = Some(id);
            self.find_index = None;
        }
        if let Some(id) = close {
            self.close_tab(id);
        }
        ui.separator();
        let mut inspect = None;
        let ready = self.ready();
        if let Some(doc) = self.active() {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&doc.path).size(12.0).color(MUTED));
                if doc.dirty() {
                    ui.label(RichText::new("UNSAVED").size(10.0).color(AMBER));
                }
                if doc.text.len() > cedar_protocol::MAX_FILE_BYTES {
                    ui.label(
                        RichText::new("OVER 1 MiB · copy or shorten to save")
                            .size(11.0)
                            .color(RED),
                    );
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(
                            ready && doc.revision.is_some(),
                            egui::Button::new("View disk version").small(),
                        )
                        .on_hover_text("Read the current file without changing your draft")
                        .clicked()
                    {
                        inspect = Some(doc.path.clone());
                    }
                    if ui.small_button("Copy draft").clicked() {
                        ui.ctx().copy_text(doc.text.clone());
                    }
                });
            });
        }
        if let Some(path) = inspect {
            self.request(
                Operation::Read { path: path.clone() },
                Job::Inspect { path },
            );
        }
        self.find_bar(ui);
        if let Some(doc) = self
            .documents
            .iter_mut()
            .find(|doc| Some(doc.id) == self.active_document)
        {
            let editor_id = egui::Id::new(("editor", doc.id));
            editor_state::load(ui.ctx(), doc);
            let jump_to = doc.jump_to.take();
            let scroll_to = jump_to.or(doc.scroll_to.take());
            if let Some(index) = jump_to {
                let mut state = egui::TextEdit::load_state(ui.ctx(), editor_id).unwrap_or_default();
                state
                    .cursor
                    .set_char_range(Some(egui::text::CCursorRange::one(
                        egui::text::CCursor::new(index),
                    )));
                state.store(ui.ctx(), editor_id);
                ui.ctx()
                    .memory_mut(|memory| memory.request_focus(editor_id));
            }
            let enabled = syntax::supports(&doc.path);
            let font_size = self.font_size;
            let mut layouter = |ui: &egui::Ui, text: &str, _width: f32| {
                syntax::galley(ui, text, font_size, enabled)
            };
            let count = doc.text.lines().count() + usize::from(doc.text.ends_with('\n'));
            let count = count.max(1);
            let line_numbers = (1..=count)
                .map(|line| format!("{line:>width$}", width = count.to_string().len().max(3)))
                .collect::<Vec<_>>()
                .join("\n");
            egui::ScrollArea::both()
                .id_salt(("editor_scroll", doc.id))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.horizontal_top(|ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(line_numbers)
                                    .font(FontId::monospace(font_size))
                                    .color(Color32::from_rgb(88, 105, 124)),
                            )
                            .selectable(false),
                        );
                        let output = egui::TextEdit::multiline(&mut doc.text)
                            .id(editor_id)
                            .font(FontId::monospace(font_size))
                            .code_editor()
                            .frame(false)
                            .desired_width(ui.available_width().max(600.0))
                            .desired_rows(count.max(30))
                            .margin(egui::vec2(10.0, 0.0))
                            .layouter(&mut layouter)
                            .show(ui);
                        if let Some(index) = scroll_to {
                            let cursor_rect = output
                                .galley
                                .pos_from_ccursor(egui::text::CCursor::new(index))
                                .translate(output.galley_pos.to_vec2());
                            ui.scroll_to_rect(cursor_rect, Some(egui::Align::Center));
                        }
                        if output.response.changed() {
                            doc.edit_version = doc.edit_version.saturating_add(1);
                            doc.has_cjk |= system_fonts::contains_cjk(&doc.text);
                        }
                        if let Some(range) = output.cursor_range {
                            doc.cursor = cursor_location(&doc.text, range.primary.ccursor.index);
                        }
                    });
                });
        }
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        if self.open_form && !self.documents.is_empty() {
            let mut visible = true;
            egui::Window::new("Connect workspace")
                .id(egui::Id::new("connection_form"))
                .open(&mut visible)
                .collapsible(false)
                .resizable(false)
                .default_width(535.0)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(ctx, |ui| {
                    self.connection_fields(ui, ctx);
                });
            if !visible {
                self.open_form = false;
            }
        }
        if self.quick_open {
            let mut visible = true;
            egui::Window::new("Open file by path")
                .open(&mut visible)
                .collapsible(false)
                .resizable(false)
                .default_width(560.0)
                .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 90.0))
                .show(ctx, |ui| {
                    ui.label(
                        RichText::new("Enter a path relative to the workspace root")
                            .small()
                            .color(MUTED),
                    );
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.quick_path)
                            .hint_text("src/main.rs")
                            .desired_width(f32::INFINITY),
                    );
                    if self.quick_focus {
                        response.request_focus();
                        self.quick_focus = false;
                    }
                    let enter =
                        response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if ui
                        .add_enabled(
                            self.ready() && !self.quick_path.trim().is_empty(),
                            egui::Button::new("Open file"),
                        )
                        .clicked()
                        || enter
                    {
                        self.open(self.quick_path.trim().replace('\\', "/"), None);
                    }
                    ui.separator();
                    ui.label(
                        RichText::new("FILES IN CURRENT DIRECTORY")
                            .small()
                            .color(MUTED),
                    );
                    let mut selected = None;
                    egui::ScrollArea::vertical()
                        .max_height(250.0)
                        .show(ui, |ui| {
                            for entry in self
                                .entries
                                .iter()
                                .filter(|entry| {
                                    !entry.is_dir
                                        && (self.quick_path.is_empty()
                                            || entry
                                                .path
                                                .to_lowercase()
                                                .contains(&self.quick_path.to_lowercase()))
                                })
                                .take(20)
                            {
                                if ui.selectable_label(false, &entry.path).clicked() {
                                    selected = Some(entry.path.clone());
                                }
                            }
                        });
                    if let Some(path) = selected {
                        self.open(path, None);
                    }
                });
            if !visible {
                self.quick_open = false;
            }
        }
        if self.new_file {
            let mut visible = true;
            egui::Window::new("New file").open(&mut visible).collapsible(false).resizable(false).default_width(450.0).anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO).show(ctx, |ui| {
                ui.label("Relative path in the workspace");
                ui.add(egui::TextEdit::singleline(&mut self.new_path).hint_text("src/new_file.rs").desired_width(f32::INFINITY));
                ui.label(RichText::new("Created on Save. Existing files will never be overwritten by a new tab.").small().color(MUTED));
                if ui.add_enabled(self.ready() && !self.new_path.trim().is_empty(), egui::Button::new("Create draft")).clicked() {
                    let path = self.new_path.trim().replace('\\', "/");
                    if path.starts_with('/') || path.split('/').any(|part| part.is_empty() || part == "." || part == "..") || path.contains(':') {
                        self.error = Some("Use a relative file path without empty, dot, or parent directory components".into());
                    } else if let Some(doc) = self.documents.iter().find(|doc| doc.path == path) {
                        self.active_document = Some(doc.id); self.new_file = false;
                    } else if self.documents.len() >= 32 { self.error = Some("Close a tab before opening another. Cedar limits the workspace to 32 buffers".into()); }
                    else {
                        let id = self.next_document; self.next_document += 1;
                        let mut doc = Document::new(id, path, String::new(), String::new());
                        doc.revision = None;
                        self.documents.push(doc); self.navigation_changed(); self.active_document = Some(id); self.new_file = false;
                    }
                }
            });
            if !visible {
                self.new_file = false;
            }
        }
        if self.confirm.is_some() {
            let close_window = matches!(self.confirm, Some(Confirm::CloseWindow));
            let target = match self.confirm.as_ref() {
                Some(Confirm::CloseTab(id)) => self
                    .documents
                    .iter()
                    .find(|doc| doc.id == *id)
                    .map(|doc| doc.path.clone())
                    .unwrap_or_default(),
                _ => "all unsaved files".into(),
            };
            egui::Modal::new(egui::Id::new("discard_confirmation")).show(ctx, |ui| {
                ui.set_max_width(430.0);
                ui.heading("Discard unsaved changes?");
                ui.label(format!("Your changes to {target} and recovery copies owned by these tabs will be discarded. Save or copy the draft first if you need to keep it."));
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("Keep editing").clicked() { self.confirm = None; }
                    if ui.button(RichText::new(if close_window { "Discard and quit" } else { "Discard and close" }).color(RED)).clicked() {
                        match self.confirm.take() {
                            Some(Confirm::CloseTab(id)) => self.remove_tab(id),
                            Some(Confirm::CloseWindow) => self.begin_close(ctx),
                            None => {},
                        }
                    }
                });
            });
        }
        if self.disk_view.is_some() {
            let mut visible = true;
            egui::Window::new("Disk version · read-only").open(&mut visible).default_size([780.0, 500.0]).show(ctx, |ui| {
                if let Some((path, text)) = &mut self.disk_view {
                    ui.label(RichText::new(path.as_str()).color(GREEN));
                    ui.label(RichText::new("Your editable draft has not been changed. Copy either version to resolve a conflict; close and reopen the tab to load the latest revision.").small().color(MUTED));
                    if ui.button("Copy disk version").clicked() { ui.ctx().copy_text(text.clone()); }
                    egui::ScrollArea::both().show(ui, |ui| { ui.add(egui::TextEdit::multiline(text).font(egui::TextStyle::Monospace).interactive(false).desired_width(f32::INFINITY).frame(false)); });
                }
            });
            if !visible {
                self.disk_view = None;
            }
        }
    }
}

impl eframe::App for CedarApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll();
        self.recovery_tick(ctx);
        let cjk = self.system_fonts.needs_probe()
            && (self.cjk_seen
                || self.language.cjk_seen
                || self.documents.iter().any(|doc| doc.has_cjk)
                || system_fonts::contains_cjk(&self.form.local_root)
                || system_fonts::contains_cjk(&self.form.remote_root)
                || system_fonts::contains_cjk(&self.search_query)
                || system_fonts::contains_cjk(&self.find_query));
        if let Some(result) = self.system_fonts.tick(ctx, cjk) {
            match result {
                Ok(message) => self.notice = message,
                Err(error) => self.error = Some(error),
            }
        }
        self.finish_pending_close(ctx);
        if ctx.input(|input| input.viewport().close_requested()) && !self.allow_close {
            if self.recovery.closing.is_some() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.recovery.visible = true;
            } else if !self.guard_run_transition(run_ui::Transition::Close) {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            } else if self.mutation_pending() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.error = Some(
                    "A save, Git, command, or language request is still running. Wait for it to finish before quitting"
                        .into(),
                );
            } else if self.dirty() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.confirm = Some(Confirm::CloseWindow);
            } else {
                self.begin_close(ctx);
            }
        }
        if self.confirm.is_none() {
            self.shortcuts(ctx);
        }
        self.header(ctx);
        self.footer(ctx);
        self.notifications(ctx);
        self.tools(ctx);
        if self.workspace_key.is_some() {
            self.sidebar(ctx);
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG).inner_margin(12.0))
            .show(ctx, |ui| {
                if self.documents.is_empty() {
                    self.welcome(ui, ctx);
                } else {
                    self.editor(ui);
                }
            });
        self.dialogs(ctx);
        self.language_popups(ctx);
        self.language_tick(ctx);
        self.run_tick(ctx);
        self.recovery_window(ctx);
        self.run_dialog(ctx);
        self.recovery_tick(ctx);
        self.finish_recovery_close_frame(ctx);
    }
}

fn field(ui: &mut egui::Ui, label: &str, value: &mut String, hint: &str) {
    ui.label(RichText::new(label).small().color(MUTED));
    ui.add(
        egui::TextEdit::singleline(value)
            .hint_text(hint)
            .desired_width(f32::INFINITY),
    );
}

fn logo(ui: &mut egui::Ui, size: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let painter = ui.painter();
    let x = rect.center().x;
    let y = rect.top();
    for (top, bottom, width) in [(0.05, 0.51, 0.29), (0.30, 0.78, 0.42)] {
        painter.add(egui::Shape::convex_polygon(
            vec![
                egui::pos2(x, y + size * top),
                egui::pos2(x - size * width, y + size * bottom),
                egui::pos2(x + size * width, y + size * bottom),
            ],
            GREEN,
            Stroke::NONE,
        ));
    }
    painter.rect_filled(
        egui::Rect::from_min_size(
            egui::pos2(x - size * 0.055, y + size * 0.75),
            egui::vec2(size * 0.11, size * 0.22),
        ),
        1.0,
        GREEN,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stale_generation_cannot_replace_workspace() {
        let mut app = CedarApp::empty();
        app.generation = 2;
        app.apply_event(Event {
            generation: 1,
            id: 0,
            connected: true,
            result: Ok(Payload::Hello {
                protocol: cedar_protocol::PROTOCOL_VERSION,
                root: "wrong".into(),
            }),
        });
        assert!(app.root.is_empty());
        assert!(app.state == ConnectionState::Idle);
    }
    #[test]
    fn transport_failure_retains_draft_and_revision() {
        let mut app = CedarApp::empty();
        let mut doc = Document::new(7, "main.rs".into(), "disk".into(), "r0".into());
        doc.text = "precious draft".into();
        doc.saving = true;
        app.documents.push(doc);
        app.pending.insert(
            1,
            Job::Save {
                document: 7,
                snapshot: "precious draft".into(),
            },
        );
        app.apply_event(Event {
            generation: 0,
            id: 1,
            connected: false,
            result: Err("SSH disconnected".into()),
        });
        assert_eq!(app.documents[0].text, "precious draft");
        assert_eq!(app.documents[0].revision.as_deref(), Some("r0"));
        assert!(!app.documents[0].saving);
        assert!(app.state == ConnectionState::Disconnected);
    }
    #[test]
    fn conflict_keeps_session_and_draft() {
        let mut app = CedarApp::empty();
        app.state = ConnectionState::Ready;
        let mut doc = Document::new(7, "main.rs".into(), "disk".into(), "r0".into());
        doc.text = "draft".into();
        doc.saving = true;
        app.documents.push(doc);
        app.pending.insert(
            1,
            Job::Save {
                document: 7,
                snapshot: "draft".into(),
            },
        );
        app.apply_event(Event {
            generation: 0,
            id: 1,
            connected: true,
            result: Err("conflict: changed externally".into()),
        });
        assert_eq!(app.documents[0].text, "draft");
        assert_eq!(app.documents[0].revision.as_deref(), Some("r0"));
        assert!(app.state == ConnectionState::Ready);
        assert!(app.documents[0].dirty());
    }
    #[test]
    fn dirty_tab_requires_explicit_close() {
        let mut app = CedarApp::empty();
        let mut doc = Document::new(1, "f".into(), "saved".into(), "r".into());
        doc.text = "unsaved".into();
        app.documents.push(doc);
        app.active_document = Some(1);
        app.close_tab(1);
        assert_eq!(app.documents.len(), 1);
        assert!(matches!(app.confirm, Some(Confirm::CloseTab(1))));
    }
    #[test]
    fn editing_during_workspace_switch_never_discards_draft() {
        let mut app = CedarApp::empty();
        let old_form = ConnectForm {
            local_root: "/old".into(),
            ..Default::default()
        };
        let new_form = ConnectForm {
            local_root: "/new".into(),
            ..Default::default()
        };
        app.workspace_key = Some(old_form.key());
        app.active_form = Some(old_form);
        app.connecting_form = Some(new_form);
        app.state = ConnectionState::Connecting;
        let mut doc = Document::new(1, "f.rs".into(), "disk".into(), "r0".into());
        doc.text = "edited during connection".into();
        app.documents.push(doc);
        app.apply_event(Event {
            generation: 0,
            id: 0,
            connected: true,
            result: Ok(Payload::Hello {
                protocol: cedar_protocol::PROTOCOL_VERSION,
                root: "/new".into(),
            }),
        });
        assert_eq!(app.documents[0].text, "edited during connection");
        assert_eq!(app.workspace_key.as_deref(), Some("local:/old"));
        assert!(app.state == ConnectionState::Disconnected);
    }
    #[test]
    fn typing_during_language_shutdown_requires_fresh_discard_confirmation() {
        let mut app = CedarApp::empty();
        let mut doc = Document::new(1, "Main.java".into(), "saved".into(), "r".into());
        app.close_after_language_stop = true;
        app.close_snapshot = Some(vec![(1, 0)]);
        doc.text = "typed during shutdown".into();
        doc.edit_version = 1;
        app.documents.push(doc);
        app.finish_pending_close(&egui::Context::default());
        assert!(!app.allow_close);
        assert!(matches!(app.confirm, Some(Confirm::CloseWindow)));
        assert_eq!(app.documents[0].text, "typed during shutdown");
    }
    #[test]
    fn slow_open_results_cannot_exceed_tab_limit() {
        let mut app = CedarApp::empty();
        for id in 0..32 {
            app.documents
                .push(Document::new(id, format!("{id}.rs"), "".into(), "r".into()));
        }
        app.pending.insert(
            1,
            Job::Open {
                path: "extra.rs".into(),
                line: None,
                navigation: 0,
            },
        );
        app.apply_event(Event {
            generation: 0,
            id: 1,
            connected: true,
            result: Ok(Payload::File {
                path: "extra.rs".into(),
                text: "extra".into(),
                revision: "r".into(),
            }),
        });
        assert_eq!(app.documents.len(), 32);
        assert!(app.error.is_some());
    }
    #[test]
    fn headless_layout_covers_launcher_editor_tools_and_modals() {
        for size in [[780.0, 540.0], [1320.0, 880.0]] {
            let ctx = egui::Context::default();
            let mut app = CedarApp::empty();
            for stage in 0..6 {
                if stage == 1 {
                    app.state = ConnectionState::Ready;
                    app.workspace_key = Some("test".into());
                    app.active_form = Some(ConnectForm::default());
                    app.open_form = false;
                    app.documents.push(Document::new(
                        1,
                        "hello.rs".into(),
                        "fn main() {\n    println!(\"Hello, 世界!\");\n}\n".into(),
                        "r".into(),
                    ));
                    app.active_document = Some(1);
                }
                if stage == 2 {
                    app.tools_open = true;
                    app.find_open = true;
                    app.find_query = "main".into();
                }
                if stage == 3 {
                    app.tool = Tool::Run;
                    app.quick_open = true;
                }
                if stage == 4 {
                    app.quick_open = false;
                    app.tool = Tool::Language;
                    app.confirm = Some(Confirm::CloseTab(1));
                }
                if stage == 5 {
                    app.confirm = None;
                    app.open_form = true;
                    app.form.ssh = true;
                }
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(size[0], size[1]),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        app.header(ctx);
                        app.footer(ctx);
                        app.notifications(ctx);
                        app.tools(ctx);
                        if app.workspace_key.is_some() {
                            app.sidebar(ctx);
                        }
                        egui::CentralPanel::default().show(ctx, |ui| {
                            if app.documents.is_empty() {
                                app.welcome(ui, ctx);
                            } else {
                                app.editor(ui);
                            }
                        });
                        app.dialogs(ctx);
                    },
                );
                assert!(!output.shapes.is_empty());
                assert!(output
                    .shapes
                    .iter()
                    .all(|shape| shape.clip_rect.is_finite()));
            }
        }
    }
    #[test]
    fn slow_file_open_does_not_steal_focus_from_newer_navigation() {
        let mut app = CedarApp::empty();
        app.documents.push(Document::new(
            1,
            "current.rs".into(),
            "draft".into(),
            "r".into(),
        ));
        app.active_document = Some(1);
        app.next_document = 2;
        app.navigation_epoch = 2;
        app.pending.insert(
            7,
            Job::Open {
                path: "older.rs".into(),
                line: None,
                navigation: 1,
            },
        );
        app.apply_event(Event {
            generation: 0,
            id: 7,
            connected: true,
            result: Ok(Payload::File {
                path: "older.rs".into(),
                text: "older".into(),
                revision: "r".into(),
            }),
        });
        assert_eq!(app.active_document, Some(1));
        assert_eq!(app.documents.len(), 2);
    }
    #[test]
    fn newer_directory_request_wins() {
        let mut app = CedarApp::empty();
        app.directory_request = 2;
        app.pending.insert(1, Job::List { path: "old".into() });
        app.apply_event(Event {
            generation: 0,
            id: 1,
            connected: true,
            result: Ok(Payload::Entries {
                entries: Vec::new(),
            }),
        });
        assert_eq!(app.directory, "");
    }
}
