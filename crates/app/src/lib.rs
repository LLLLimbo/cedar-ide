//! Cedar IDE — a native Rust frontend for a local or SSH workspace agent.
mod agent_support;
#[cfg(test)]
mod build_problem_process_tests;
#[cfg(test)]
mod build_problem_ui_tests;
mod build_problems;
pub mod completion;
#[cfg(test)]
mod connection_cancel_tests;
mod disk_review;
mod editor_state;
mod git_ui;
mod interrupted_save;
#[cfg(test)]
mod interrupted_save_process_tests;
mod java_language;
mod language_navigation_results;
mod language_results;
mod language_sync;
mod language_ui;
mod model;
mod navigation;
mod profile_ui;
mod recovery;
mod recovery_actor;
#[cfg(test)]
mod recovery_tests;
mod recovery_ui;
mod replace;
mod run_ui;
mod syntax;
mod system_fonts;
#[cfg(test)]
mod tab_focus_tests;
pub mod task_profiles;
pub mod text_edits;
mod worker;

use cedar_client::ConnectionSpec;
use cedar_protocol::{AgentInfo, Entry, Operation, Payload, SearchMatch};
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
// Keep endpoint fields separate: paths and SSH hosts can contain colons. Trust
// changes intentionally retain the workspace identity; Hello checks its root.
#[derive(Clone, Debug, PartialEq, Eq)]
enum WorkspaceKey {
    Local {
        root: String,
    },
    Ssh {
        host: String,
        port: String,
        root: String,
        agent_path: String,
    },
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
    fn key(&self) -> WorkspaceKey {
        if self.ssh {
            WorkspaceKey::Ssh {
                host: self.host.trim().into(),
                port: self.port.trim().into(),
                root: self.remote_root.trim().into(),
                agent_path: self.agent.trim().into(),
            }
        } else {
            WorkspaceKey::Local {
                root: self.local_root.trim().into(),
            }
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
    BuildProblemOpen {
        path: String,
        line: Option<usize>,
        navigation: u64,
        source: run_ui::BuildSource,
    },
    Save {
        document: u64,
        snapshot: String,
        submission: Option<interrupted_save::InterruptedSave>,
    },
    Search {
        query: String,
    },
    Git,
    GitRead(git_ui::Action),
    ProfilesLoad {
        epoch: u64,
    },
    Run(run_ui::Action),
    DiskReview {
        ticket: u64,
        purpose: disk_review::Purpose,
    },
    InterruptedSaveCheck {
        ticket: u64,
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
    workspace_key: Option<WorkspaceKey>,
    connecting_form: Option<ConnectForm>,
    // Validated, immutable support claims for this accepted connection only.
    agent_info: Option<AgentInfo>,
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
    close_tab_requested: Option<u64>,
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
    git_state: git_ui::GitPanel,
    profiles: profile_ui::Profiles,
    run_state: run_ui::RunPanel,
    language: language_ui::LanguagePanel,
    navigation: navigation::Navigation,
    new_file: bool,
    new_path: String,
    find_open: bool,
    find_query: String,
    find_index: Option<usize>,
    find_focus: bool,
    replace: replace::Replace,
    disk_review: disk_review::DiskReview,
    interrupted_save_check: interrupted_save::Check,
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
            agent_info: None,
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
            close_tab_requested: None,
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
            git_state: git_ui::GitPanel::default(),
            profiles: profile_ui::Profiles::default(),
            run_state: run_ui::RunPanel::default(),
            language: language_ui::LanguagePanel::default(),
            navigation: navigation::Navigation::default(),
            new_file: false,
            new_path: String::new(),
            find_open: false,
            find_query: String::new(),
            find_index: None,
            find_focus: false,
            replace: replace::Replace::default(),
            disk_review: disk_review::DiskReview::default(),
            interrupted_save_check: interrupted_save::Check::default(),
            font_size: 14.0,
            recovery: recovery::Recovery::default(),
        }
    }

    fn ready(&self) -> bool {
        self.state == ConnectionState::Ready
    }
    fn dirty(&self) -> bool {
        self.profiles.dirty() || self.documents.iter().any(Document::dirty)
    }
    fn draft_versions(&self) -> Vec<(u64, u64)> {
        let mut versions: Vec<_> = self
            .documents
            .iter()
            .map(|doc| (doc.id, doc.edit_version))
            .collect();
        versions.push((0, self.profiles.epoch));
        versions
    }
    fn mutation_pending(&self) -> bool {
        self.language.startup_active()
            || self.pending.values().any(|job| {
                matches!(
                    job,
                    Job::Save { .. } | Job::Git | Job::GitRead(_) | Job::Language(_)
                )
            })
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
        if self.dirty() && self.workspace_key.as_ref() != Some(&form.key()) {
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
        self.reset_git(self.workspace_key.as_ref() != Some(&form.key()));
        self.dismiss_disk_review();
        self.run_state.reset();
        self.profiles.disconnected();
        self.worker = None;
        self.language.reset();
        self.generation += 1;
        self.pending.clear();
        self.disk_review.outstanding = None;
        self.interrupted_save_check.reset();
        for doc in &mut self.documents {
            doc.saving = false;
        }
        self.agent_info = None;
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

    fn cancel_connection(&mut self) {
        if self.state != ConnectionState::Connecting {
            return;
        }
        self.dismiss_disk_review();
        self.reset_git(false);
        self.worker = None;
        self.disk_review.outstanding = None;
        self.agent_info = None;
        self.generation += 1;
        self.connecting_form = None;
        self.recovery.restoring_generation = None;
        self.state = if self.workspace_key.is_some() {
            ConnectionState::Disconnected
        } else {
            ConnectionState::Idle
        };
        self.notice = "Connection cancelled".into();
    }

    fn request(&mut self, op: Operation, job: Job) -> u64 {
        if !self.ready() {
            self.error =
                Some("Reconnect to the workspace first. Unsaved buffers are retained".into());
            return 0;
        }
        if let Some(problem) = self.operation_problem(&op) {
            self.error = Some(problem);
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
        // Transport loss is never proof that language cleanup finished, even
        // when an ordinary file/task request reports the loss first.
        self.close_after_language_stop = false;
        self.close_snapshot = None;
        self.retain_interrupted_saves();
        self.reset_git(false);
        self.dismiss_disk_review();
        self.run_state.disconnected();
        self.profiles.disconnected();
        self.recovery.restoring_generation = None;
        self.state = ConnectionState::Disconnected;
        self.connecting_form = None;
        self.agent_info = None;
        self.language.reset();
        self.worker = None;
        self.pending.clear();
        self.disk_review.outstanding = None;
        self.interrupted_save_check.reset();
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
        self.replace.invalidate();
        self.dismiss_navigation();
        self.dismiss_disk_review();
        self.interrupted_save_check.invalidate();
        self.navigation_epoch = self.navigation_epoch.wrapping_add(1);
        self.language.cancel_navigation();
    }

    fn open(&mut self, path: String, line: Option<usize>) {
        self.open_with_build_source(path, line, None);
    }

    fn open_with_build_source(
        &mut self,
        path: String,
        line: Option<usize>,
        source: Option<run_ui::BuildSource>,
    ) {
        if source.is_some_and(|source| !self.build_source_is_current(source)) {
            return;
        }
        if source.is_some() {
            if self.build_path_ambiguous(&path) {
                return;
            }
            if let Some(doc) = self.documents.iter().find(|doc| doc.path == path) {
                if !self.build_line_available(&doc.text.clone(), line) {
                    return;
                }
            }
        }
        self.navigation_changed();
        let navigation = self.navigation_epoch;
        if let Some(doc) = self.documents.iter_mut().find(|doc| doc.path == path) {
            self.active_document = Some(doc.id);
            if let Some(line) = line {
                doc.jump_to = Some(line_start(&doc.text, line));
            }
            self.navigation.restore_focus = true;
            return;
        }
        if self.documents.len() >= 32 {
            self.error = Some(
                "Close a tab before opening another. Cedar limits the workspace to 32 buffers"
                    .into(),
            );
            return;
        }
        let pending = self.pending.iter().find_map(|(id, job)| match job {
            Job::Open { path: pending, .. } | Job::BuildProblemOpen { path: pending, .. }
                if pending == &path =>
            {
                Some(*id)
            }
            _ => None,
        });
        let op = Operation::Read { path: path.clone() };
        let job = match source {
            Some(source) => Job::BuildProblemOpen {
                path,
                line,
                navigation,
                source,
            },
            None => Job::Open {
                path,
                line,
                navigation,
            },
        };
        if let Some(id) = pending {
            self.pending.insert(id, job);
        } else {
            self.request(op, job);
        }
    }

    fn save(&mut self) {
        if let Some(id) = self.active_document {
            self.save_document(id);
        }
    }

    fn save_document(&mut self, id: u64) {
        let Some(doc) = self.documents.iter().find(|doc| doc.id == id) else {
            return;
        };
        if !doc.dirty() || doc.saving {
            return;
        }
        if doc.interrupted_save.is_some() || self.interrupted_save_check.busy() {
            self.error = Some("Check the interrupted save after reconnecting before saving again. Your draft is retained".into());
            return;
        }
        if doc.text.len() > cedar_protocol::MAX_FILE_BYTES {
            self.error = Some("This draft exceeds the 1 MiB file limit. Your text is retained; shorten it or copy it before saving".into());
            return;
        }
        let submission = interrupted_save::InterruptedSave::capture(self, doc);
        let Some(submission) = submission else {
            self.error = Some("The save identity could not be captured. Reconnect to the original workspace before saving".into());
            return;
        };
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
                submission: Some(submission),
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
            // Every connection event, including errors, belongs to a single active
            // attempt. Duplicate handshakes must never revoke a Ready session.
            if self.state != ConnectionState::Connecting || self.connecting_form.is_none() {
                return;
            }
            match event.result {
                Ok(Payload::Hello {
                    protocol,
                    root,
                    agent,
                }) => {
                    if protocol != cedar_protocol::PROTOCOL_VERSION {
                        self.disconnected(
                            "Agent protocol version does not match this frontend".into(),
                        );
                        return;
                    }
                    if let Some(info) = &agent {
                        if let Err(error) = info.validate() {
                            self.disconnected(error.to_string());
                            return;
                        }
                    }
                    if !["list", "read"]
                        .iter()
                        .all(|name| cedar_protocol::supports_capability(agent.as_ref(), name))
                    {
                        self.disconnected(
                            "Agent must support list and read to open a workspace".into(),
                        );
                        return;
                    }
                    let Some(form) = self.connecting_form.take() else {
                        return;
                    };
                    let key = form.key();
                    if self.recovery.restoring_generation == Some(self.generation)
                        && self.recovery.pending_restore.as_ref().is_some_and(|draft| {
                            draft.workspace != recovery_ui::identity(&form, &root)
                        })
                    {
                        self.recovery.error = Some("The agent returned a different workspace root. Recovery was not restored; verify the intended workspace and retry".into());
                        self.disconnected("Recovery workspace identity did not match".into());
                        return;
                    }
                    if self.workspace_key.as_ref() == Some(&key)
                        && !self.root.is_empty()
                        && self.root != root
                        && self.dirty()
                    {
                        self.disconnected("The workspace root changed while reconnecting. Your drafts are retained; reconnect to their original root before saving".into());
                        return;
                    }
                    if self.workspace_key.as_ref() != Some(&key)
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
                        self.reset_git(true);
                        self.run_state.output = "Command output will appear here".into();
                    }
                    self.profiles.connected(recovery_ui::identity(&form, &root));
                    self.workspace_key = Some(key);
                    self.active_form = Some(form);
                    self.root = root;
                    self.agent_info = agent;
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
        let invalid_save_ack = self.pending.get(&event.id).is_some_and(|job| {
            matches!(job, Job::Save { .. } if match &event.result {
                Ok(Payload::Written { .. }) => false,
                Ok(_) => true,
                Err(_) => false,
            })
        });
        if !event.connected || invalid_save_ack {
            self.retain_interrupted_save(event.id);
        }
        let Some(job) = self.pending.remove(&event.id) else {
            return;
        };
        // A build location is historical output from one completed command.
        // Late reads must not open tabs after a new task/session/navigation.
        let job = if let Job::BuildProblemOpen {
            path,
            line,
            navigation,
            source,
        } = job
        {
            if !event.connected {
                self.disconnected(event.result.err().unwrap_or_else(|| {
                    "The connection closed while opening a build location".into()
                }));
                return;
            }
            if navigation != self.navigation_epoch || !self.build_source_is_current(source) {
                return;
            }
            if self.build_path_ambiguous(&path) {
                return;
            }
            if let Ok(Payload::File {
                path: returned,
                text,
                ..
            }) = &event.result
            {
                if returned == &path {
                    let current = self
                        .documents
                        .iter()
                        .find(|doc| doc.path == path)
                        .map_or(text, |doc| &doc.text)
                        .clone();
                    if !self.build_line_available(&current, line) {
                        return;
                    }
                }
            }
            Job::Open {
                path,
                line,
                navigation,
            }
        } else {
            job
        };
        if matches!(&job, Job::Language(action) if action.is_java_startup()) {
            let Job::Language(action) = job else {
                unreachable!()
            };
            self.apply_java_startup_event(action, event.result, event.connected);
            return;
        }
        if matches!(&job, Job::Language(action) if action.is_maven_model()) {
            let Job::Language(action) = job else {
                unreachable!()
            };
            self.apply_maven_model_event(action, event.result, event.connected);
            return;
        }
        if let Job::Language(language_ui::Action {
            kind: language_ui::ActionKind::RefreshJavaDiagnostics { context },
            ..
        }) = &job
        {
            // An obsolete snapshot cannot publish errors or unexpected payloads,
            // but it must not hide a failure of the current transport.
            if !event.connected {
                self.disconnected("The connection closed while requesting Java diagnostics. Your unsaved draft is retained.".into());
                return;
            }
            if !self.java_diagnostics_refresh_is_current(context) {
                return;
            }
        }
        if let Job::GitRead(action) = job {
            // A stale view must not hide a current transport failure. Conversely,
            // a late Git result/error cannot replace a newer selection or config.
            if !event.connected {
                self.disconnected(
                    event.result.err().unwrap_or_else(|| {
                        "The connection closed while reading Git changes".into()
                    }),
                );
            } else {
                self.apply_git_read(event.id, action, event.result);
            }
            return;
        }
        if let Job::InterruptedSaveCheck { ticket } = job {
            if self.interrupted_save_check.outstanding == Some(event.id) {
                self.interrupted_save_check.outstanding = None;
            }
            if !event.connected {
                self.disconnected(event.result.err().unwrap_or_else(|| {
                    "The connection closed while checking the interrupted save".into()
                }));
            } else {
                self.apply_interrupted_save_read(ticket, event.result);
            }
            return;
        }
        if let Job::DiskReview { ticket, purpose } = job {
            if self.disk_review.outstanding == Some(event.id) {
                self.disk_review.outstanding = None;
            }
            // Transport liveness belongs to the current connection, even when
            // its old file review has been dismissed. Old generations were
            // rejected before looking up this job.
            if !event.connected {
                self.disconnected(
                    event
                        .result
                        .err()
                        .unwrap_or_else(|| "The connection closed while reading disk".into()),
                );
                return;
            }
            self.apply_disk_read(ticket, purpose, event.result, event.connected);
            return;
        }
        if let Job::Save { document, .. } = &job {
            if let Some(doc) = self.documents.iter_mut().find(|doc| doc.id == *document) {
                doc.saving = false;
            }
        }
        if invalid_save_ack {
            self.error = Some("The save acknowledgement could not be verified. Use Check interrupted save; your draft is retained".into());
            if !event.connected {
                self.disconnected(self.error.clone().unwrap());
            }
            return;
        }
        let payload = match event.result {
            Ok(payload) => payload,
            Err(error) => {
                let error = if let Job::Language(action) = &job {
                    self.language_public_error(action, &error)
                } else {
                    error
                };
                if let Job::ProfilesLoad { epoch } = &job {
                    if self.profile_load_error(*epoch, event.connected, &error) {
                        return;
                    }
                }
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
                self.observe_maven_pom_acknowledgement(event.id, &path, &revision);
                if let Some(doc) = self.documents.iter_mut().find(|doc| doc.path == path) {
                    if navigation == self.navigation_epoch {
                        self.active_document = Some(doc.id);
                        self.navigation.restore_focus = true;
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
                    self.navigation.restore_focus = true;
                    self.open_form = false;
                }
                self.complete_language_navigation(&requested);
            }
            (
                Job::ProfilesLoad { epoch },
                Payload::File {
                    path,
                    text,
                    revision,
                },
            ) => {
                if path == profile_ui::PATH {
                    self.apply_profile_load(epoch, Some((text, revision)));
                } else {
                    self.profiles.message =
                        Some("Agent returned a different profile path; response ignored".into());
                }
            }
            (
                Job::Save {
                    document, snapshot, ..
                },
                Payload::Written { revision },
            ) => {
                if self
                    .documents
                    .iter()
                    .any(|doc| doc.id == document && doc.path == "pom.xml")
                {
                    self.observe_maven_pom_acknowledgement(event.id, "pom.xml", &revision);
                }
                let workspace = self.recovery_workspace();
                if let Some(doc) = self.documents.iter_mut().find(|doc| doc.id == document) {
                    doc.acknowledge_save(snapshot, revision);
                    if let Some(workspace) = &workspace {
                        self.recovery.saved(workspace, doc);
                    }
                    self.notice = format!("Saved {}", doc.path);
                }
                self.profile_saved(document);
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
            (Job::Language(action), _)
                if matches!(
                    action.kind,
                    language_ui::ActionKind::RefreshJavaDiagnostics { .. }
                ) =>
            {
                let error = "Unexpected Java diagnostic refresh response; diagnostic freshness is unchanged".to_owned();
                self.language_error(&action, &error);
                self.error = Some(error);
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
        if self.close_after_language_stop
            && !self.language.running
            && !self.language.startup_active()
            && !self.language_busy()
        {
            self.close_after_language_stop = false;
            let current = self.draft_versions();
            if self.close_snapshot.take().as_ref() != Some(&current) && self.dirty() {
                self.confirm = Some(Confirm::CloseWindow);
                self.notice = "A draft changed while the language server was stopping; confirm before quitting".into();
            } else {
                self.finish_recovery_close(ctx);
            }
        }
    }

    fn request_window_close(&mut self, ctx: &egui::Context) {
        if self.close_after_language_stop {
            // A repeated window-close event must not create a second discard
            // dialog while the original approval waits for verified cleanup.
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.notice = "Waiting for language cleanup before closing".into();
        } else if self.recovery.closing.is_some() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.recovery.visible = true;
        } else if !self.guard_run_transition(run_ui::Transition::Close) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        } else if self.pending.values().any(|job| match job {
            Job::Save { .. } | Job::Git | Job::GitRead(_) => true,
            Job::Language(action) => !action.is_java_startup() || !self.language.startup_active(),
            _ => false,
        }) {
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

    fn begin_close(&mut self, ctx: &egui::Context) {
        if !self.guard_run_transition(run_ui::Transition::Close) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            return;
        }
        self.dismiss_disk_review();
        if (self.language.running || self.language.startup_active()) && self.ready() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.close_after_language_stop = true;
            self.close_snapshot = Some(self.draft_versions());
            self.stop_language();
            self.notice = "Stopping language server before closing".into();
        } else {
            self.finish_recovery_close(ctx);
        }
    }

    fn finish_tab_close(&mut self) {
        if let Some(id) = self.close_tab_requested.take() {
            self.close_tab(id);
        }
    }

    fn close_tab(&mut self, id: u64) {
        if self.active_document == Some(id) {
            self.dismiss_disk_review();
        }
        if self.documents.iter().any(|doc| doc.id == id && doc.saving) {
            self.error =
                Some("This file is saving. Wait for the acknowledgement before closing it".into());
            return;
        }
        if self.documents.iter().any(|doc| doc.id == id && doc.dirty())
            || (self.profiles.owns_document(id) && self.profiles.dirty())
        {
            self.confirm = Some(Confirm::CloseTab(id));
        } else {
            self.remove_tab(id);
        }
    }
    fn remove_tab(&mut self, id: u64) {
        self.profiles.document_closed(id);
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
        if self.navigation_shortcuts(ctx) {
            return;
        }
        self.language_shortcuts(ctx);
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::S)) {
            self.save();
        }
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::F)) {
            self.find_open = true;
            self.find_focus = true;
        }
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::W)) {
            if let Some(id) = self.active_document {
                self.close_tab_requested = Some(id);
            }
        }
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            self.dismiss_disk_review();
            if self.find_open {
                self.close_find(ctx);
            }
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
                        let can_save = self.backend_supports("write")
                            && !self.interrupted_save_check.busy()
                            && self.active().is_some_and(|doc| {
                                doc.dirty() && !doc.saving && doc.interrupted_save.is_none()
                            });
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
                                .on_hover_text("Choose an open buffer or file · Ctrl/Cmd+P")
                                .clicked()
                        {
                            self.show_file_chooser();
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
                    if self.ready() {
                        let status = self.agent_status();
                        ui.add_sized([240.0, 18.0], egui::Label::new(RichText::new(&status).small().color(MUTED)).truncate())
                            .on_hover_text(format!("{status}\n{}", self.agent_details()));
                    }
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
                    ui.label(RichText::new("Ctrl/Cmd+P  Choose a file\nCtrl/Cmd+G  Go to line\nCtrl/Cmd+F  Find in file\nCtrl/Cmd+S  Save changes").size(11.0).color(MUTED));
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.selectable_label(self.tools_open && self.tool == Tool::Search, "Search").clicked() { self.tool = Tool::Search; self.tools_open = true; }
                        if ui.selectable_label(self.tools_open && self.tool == Tool::Git, "Git").clicked() { self.tool = Tool::Git; self.tools_open = true; }
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
                    if ui.selectable_label(self.tool == Tool::Git, "GIT CHANGES").clicked() { self.tool = Tool::Git; }
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
                        if self.ready() && !self.backend_supports("search") { ui.colored_label(AMBER, self.unsupported_message("search")); }
                        ui.horizontal(|ui| {
                            let edit = ui.add(egui::TextEdit::singleline(&mut self.search_query).hint_text("Find text across the workspace...").desired_width((ui.available_width() - 175.0).max(180.0)));
                            let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                            if ui.add_enabled(self.backend_supports("search"), egui::Button::new("Search")).clicked() || enter { self.search(); }
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
                    Tool::Git => self.git_panel(ui),
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
                    self.cancel_connection();
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
                                self.show_file_chooser();
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
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("FIND").small().color(MUTED));
            let input_id = egui::Id::new(replace::FIND_INPUT);
            if self.find_focus && !self.navigation.blocks_editor() && ui.is_enabled() {
                ui.ctx().memory_mut(|memory| memory.request_focus(input_id));
                self.find_focus = false;
            }
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.find_query)
                    .id(input_id)
                    .hint_text("Case-sensitive text")
                    .desired_width(230.0),
            );
            if response.changed() {
                self.find_index = None;
                self.replace.invalidate();
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
                self.close_find(ui.ctx());
            }
        });
        if next || previous {
            self.replace.invalidate();
            replace::discard_keyboard(ui.ctx());
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
        if self.find_open {
            self.replace_bar(ui);
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
        let activate = activate.filter(|_| {
            ui.is_enabled()
                && !self.navigation.blocks_editor()
                && !self.foreign_modal_owns_input(ui.ctx())
        });
        if let Some(id) = activate {
            self.navigation_changed();
            self.active_document = Some(id);
            self.find_index = None;
        }
        if let Some(id) = close {
            self.close_tab_requested = Some(id);
        }
        ui.separator();
        let mut compare = false;
        let mut check_save = false;
        let can_compare = self.backend_supports("read")
            && !self.disk_review.busy()
            && !self.interrupted_save_check.busy();
        let can_check_save = can_compare
            && !self.documents.iter().any(|doc| doc.saving)
            && !self
                .pending
                .values()
                .any(|job| matches!(job, Job::Save { .. }));
        if let Some(doc) = self.active() {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&doc.path).size(12.0).color(MUTED));
                if doc.interrupted_save.is_some() {
                    ui.label(RichText::new("SAVE OUTCOME UNKNOWN").size(10.0).color(AMBER));
                } else if doc.dirty() {
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
                        .add_enabled(can_compare, egui::Button::new("Compare with disk").small())
                        .on_hover_text("Read the current file without changing your draft")
                        .clicked()
                    {
                        compare = true;
                    }
                    if doc.interrupted_save.is_some() {
                        check_save = ui.add_enabled(can_check_save, egui::Button::new("Check interrupted save").small())
                            .on_hover_text("Read disk twice and compare with the submitted contents; never retries the write").clicked();
                    }
                    if ui.small_button("Copy draft").clicked() {
                        ui.ctx().copy_text(doc.text.clone());
                    }
                });
            });
        }
        if compare {
            self.compare_with_disk();
        }
        if check_save {
            self.check_interrupted_save();
        }
        if let Some(message) = self.interrupted_save_check.message() {
            ui.label(RichText::new(message).small().color(AMBER));
        }
        let navigation_blocked = self.navigation.blocks_editor();
        ui.add_enabled_ui(!navigation_blocked, |ui| self.find_bar(ui));
        let find_open = self.find_open;
        if let Some(doc) = self
            .documents
            .iter_mut()
            .find(|doc| Some(doc.id) == self.active_document)
        {
            let editor_id = egui::Id::new(("editor", doc.id));
            editor_state::load(ui.ctx(), doc);
            if !navigation_blocked {
                editor_state::history_shortcut(ui.ctx(), doc);
            }
            let jump_to = (!navigation_blocked).then(|| doc.jump_to.take()).flatten();
            let scroll_to = jump_to.or_else(|| {
                (!navigation_blocked)
                    .then(|| doc.scroll_to.take())
                    .flatten()
            });
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
            let cursor_history = if navigation_blocked {
                None
            } else {
                editor_state::before_cursor_interaction(ui.ctx(), doc, scroll_to.is_some())
            };
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
                // egui's background drag surface uses last frame's rect. The
                // expanding Find preview must not be covered by that old rect.
                .drag_to_scroll(!find_open)
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
                            .interactive(!navigation_blocked)
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
                        editor_state::after_cursor_interaction(
                            ui.ctx(),
                            doc,
                            &output,
                            cursor_history,
                        );
                        if let Some(range) = output.cursor_range {
                            doc.cursor = cursor_location(&doc.text, range.primary.ccursor.index);
                        }
                    });
                });
        }
        if let Some(id) = activate.filter(|id| {
            Some(*id) == self.active_document
                && ui.is_enabled()
                && !self.navigation.blocks_editor()
                && !self.foreign_modal_owns_input(ui.ctx())
        }) {
            // A native click can deliver press and release in one frame. Focus
            // after TextEdit handles that outside press, or it surrenders focus.
            // This local intent never survives into later Find/Replace input.
            ui.ctx()
                .memory_mut(|memory| memory.request_focus(egui::Id::new(("editor", id))));
            ui.ctx().request_repaint();
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
                _ => "all unsaved files and profile form edits".into(),
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
        self.disk_review_window(ctx);
    }
}

impl eframe::App for CedarApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll();
        self.begin_navigation_frame(ctx);
        self.recovery_tick(ctx);
        let cjk = self.system_fonts.needs_probe()
            && (self.cjk_seen
                || self.language.cjk_seen
                || self.documents.iter().any(|doc| doc.has_cjk)
                || system_fonts::contains_cjk(&self.form.local_root)
                || system_fonts::contains_cjk(&self.form.remote_root)
                || system_fonts::contains_cjk(&self.search_query)
                || system_fonts::contains_cjk(&self.find_query)
                || system_fonts::contains_cjk(&self.replace.replacement)
                || self.navigation.has_cjk()
                || system_fonts::contains_cjk(&self.profiles.draft.name)
                || system_fonts::contains_cjk(&self.profiles.draft.program)
                || self
                    .profiles
                    .draft
                    .args
                    .iter()
                    .any(|arg| system_fonts::contains_cjk(arg)));
        if let Some(result) = self.system_fonts.tick(ctx, cjk) {
            match result {
                Ok(message) => self.notice = message,
                Err(error) => self.error = Some(error),
            }
        }
        self.finish_pending_close(ctx);
        if ctx.input(|input| input.viewport().close_requested()) && !self.allow_close {
            self.request_window_close(ctx);
        }
        if self.confirm.is_none() {
            self.shortcuts(ctx);
        }
        self.navigation_window(ctx);
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
        if !self.navigation.blocks_editor() {
            self.language_popups(ctx);
        }
        self.run_tick(ctx);
        self.recovery_window(ctx);
        self.run_dialog(ctx);
        self.finish_disk_reload(ctx);
        self.finish_profile_actions();
        self.finish_interrupted_save_check();
        self.finish_tab_close();
        self.finish_replace_frame(ctx);
        self.language_tick(ctx);
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

    fn colliding_ssh_forms() -> (ConnectForm, ConnectForm) {
        let first = ConnectForm {
            ssh: true,
            host: "user@[2001:db8::1]".into(),
            remote_root: "/work:bin".into(),
            agent: "cedar".into(),
            ..Default::default()
        };
        let second = ConnectForm {
            remote_root: "/work".into(),
            agent: "bin:cedar".into(),
            ..first.clone()
        };
        (first, second)
    }

    fn workspace_app(form: &ConnectForm, dirty: bool) -> (CedarApp, Receiver<Command>) {
        let mut app = CedarApp::empty();
        let (worker, commands) = Worker::recording();
        app.worker = Some(worker);
        app.workspace_key = Some(form.key());
        app.active_form = Some(form.clone());
        app.root = "/canonical/workspace".into();
        app.profiles
            .connected(recovery_ui::identity(form, &app.root));
        app.state = ConnectionState::Ready;
        let mut doc = Document::new(1, "main.rs".into(), "disk".into(), "r0".into());
        if dirty {
            doc.text = "precious draft".into();
        }
        app.documents.push(doc);
        app.active_document = Some(1);
        (app, commands)
    }

    fn apply_hello(app: &mut CedarApp, form: ConnectForm, root: &str) {
        app.connecting_form = Some(form);
        app.state = ConnectionState::Connecting;
        app.apply_event(Event {
            generation: app.generation,
            id: 0,
            connected: true,
            result: Ok(Payload::Hello {
                protocol: cedar_protocol::PROTOCOL_VERSION,
                agent: None,
                root: root.into(),
            }),
        });
    }

    #[test]
    fn workspace_keys_keep_ssh_fields_separate() {
        let (first, second) = colliding_ssh_forms();
        let legacy_key = |form: &ConnectForm| {
            format!(
                "ssh:{}:{}:{}:{}",
                form.host, form.port, form.remote_root, form.agent
            )
        };
        assert_eq!(legacy_key(&first), legacy_key(&second));
        assert_ne!(first.key(), second.key());
        assert!(first.spec().is_ok());
        assert!(second.spec().is_ok());
        for field in ["host", "port", "root", "agent"] {
            let mut different = first.clone();
            match field {
                "host" => different.host.push_str(":2"),
                "port" => different.port = "2222".into(),
                "root" => different.remote_root.push_str(":more"),
                "agent" => different.agent.push_str(":more"),
                _ => unreachable!(),
            }
            assert_ne!(first.key(), different.key(), "changed {field}");
        }
        let local = ConnectForm {
            ssh: false,
            local_root: first.remote_root.clone(),
            ..first.clone()
        };
        assert_ne!(first.key(), local.key());
    }

    #[test]
    fn workspace_keys_ignore_trust_inactive_fields_and_outer_whitespace() {
        for ssh in [false, true] {
            let form = ConnectForm {
                ssh,
                local_root: "/local:workspace".into(),
                host: "user@[2001:db8::1]".into(),
                remote_root: "/remote:workspace".into(),
                agent: "bin:cedar".into(),
                ..Default::default()
            };
            let mut changed = form.clone();
            changed.allow_run = !form.allow_run;
            if ssh {
                changed.local_root = "/ignored".into();
                changed.host = format!(" {} ", form.host);
                changed.port = format!(" {} ", form.port);
                changed.remote_root = format!(" {} ", form.remote_root);
                changed.agent = format!(" {} ", form.agent);
            } else {
                changed.local_root = format!(" {} ", form.local_root);
                changed.host = "ignored".into();
                changed.port = "2222".into();
                changed.remote_root = "/ignored".into();
                changed.agent = "ignored".into();
            }
            assert_eq!(form.key(), changed.key());
        }
    }

    #[test]
    fn dirty_workspace_rejects_delimiter_collision_before_connect() {
        let (first, second) = colliding_ssh_forms();
        // Fail before calling connect if the key regresses, so this test can
        // never launch SSH even when the delimiter-collision bug is restored.
        assert_ne!(first.key(), second.key());
        let (mut app, commands) = workspace_app(&first, true);
        app.connect(&egui::Context::default(), second);
        assert!(app.state == ConnectionState::Ready);
        assert_eq!(app.generation, 0);
        assert!(app.connecting_form.is_none());
        assert_eq!(app.workspace_key, Some(first.key()));
        assert_eq!(app.documents[0].text, "precious draft");
        assert!(app
            .error
            .as_deref()
            .unwrap()
            .contains("switching workspaces"));
        assert!(matches!(
            commands.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
    }

    #[test]
    fn delimiter_collision_handshake_cannot_adopt_dirty_workspace() {
        let (first, second) = colliding_ssh_forms();
        let (mut app, commands) = workspace_app(&first, true);
        // Distinct paths/agents can report the same canonical root. The Hello
        // root alone therefore cannot establish the retained draft's identity.
        apply_hello(&mut app, second, "/canonical/workspace");
        assert!(app.state == ConnectionState::Disconnected);
        assert_eq!(app.workspace_key, Some(first.key()));
        assert_eq!(app.active_form.as_ref().unwrap().key(), first.key());
        assert_eq!(app.root, "/canonical/workspace");
        assert_eq!(app.documents[0].text, "precious draft");
        assert_eq!(app.documents[0].revision.as_deref(), Some("r0"));
        assert!(app
            .error
            .as_deref()
            .unwrap()
            .contains("Workspace switch cancelled"));
        app.save_document(1);
        assert!(!app.documents[0].saving);
        assert!(app.pending.is_empty());
        assert!(matches!(
            commands.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
    }

    #[test]
    fn delimiter_collision_handshake_clears_clean_workspace() {
        let (first, second) = colliding_ssh_forms();
        let (mut app, commands) = workspace_app(&first, false);
        let next_key = second.key();
        apply_hello(&mut app, second, "/canonical/workspace");
        assert!(app.ready());
        assert_eq!(app.workspace_key, Some(next_key));
        assert!(app.documents.is_empty());
        assert!(app.active_document.is_none());
        assert!(matches!(
            commands.try_recv().unwrap().op,
            Operation::List { .. }
        ));
    }

    #[test]
    fn canonical_root_change_rejects_same_endpoint_dirty_reconnect() {
        let (form, _) = colliding_ssh_forms();
        let (mut app, commands) = workspace_app(&form, true);
        apply_hello(&mut app, form.clone(), "/different/canonical/root");
        assert!(app.state == ConnectionState::Disconnected);
        assert_eq!(app.workspace_key, Some(form.key()));
        assert_eq!(app.root, "/canonical/workspace");
        assert_eq!(app.documents[0].text, "precious draft");
        assert_eq!(app.documents[0].revision.as_deref(), Some("r0"));
        assert!(app
            .error
            .as_deref()
            .unwrap()
            .contains("workspace root changed"));
        assert!(matches!(
            commands.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
    }

    #[test]
    fn trust_only_handshake_retains_same_workspace_drafts() {
        for ssh in [false, true] {
            for allow_run in [false, true] {
                let (mut form, _) = colliding_ssh_forms();
                form.ssh = ssh;
                form.allow_run = allow_run;
                let (mut app, commands) = workspace_app(&form, true);
                let original_key = form.key();
                form.allow_run = !allow_run;
                apply_hello(&mut app, form, "/canonical/workspace");
                assert!(app.ready());
                assert!(app.error.is_none());
                assert_eq!(app.workspace_key, Some(original_key));
                assert_eq!(app.active_form.as_ref().unwrap().allow_run, !allow_run);
                assert_eq!(app.documents[0].text, "precious draft");
                assert_eq!(app.documents[0].revision.as_deref(), Some("r0"));
                assert!(app.documents[0].dirty());
                assert!(matches!(
                    commands.try_recv().unwrap().op,
                    Operation::List { .. }
                ));
            }
        }
    }

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
                agent: None,
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
                submission: None,
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
                submission: None,
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
                agent: None,
                root: "/new".into(),
            }),
        });
        assert_eq!(app.documents[0].text, "edited during connection");
        assert_eq!(
            app.workspace_key,
            Some(WorkspaceKey::Local {
                root: "/old".into()
            })
        );
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
                    let form = ConnectForm::default();
                    app.workspace_key = Some(form.key());
                    app.active_form = Some(form);
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
                    app.show_file_chooser();
                }
                if stage == 4 {
                    app.dismiss_navigation();
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
                        app.navigation_window(ctx);
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
    fn maximum_agent_metadata_preserves_recovery_visibility_at_minimum_window_size() {
        let ctx = egui::Context::default();
        let mut app = CedarApp::empty();
        let mut info = agent_support::full_test_agent();
        info.version = "v".repeat(cedar_protocol::MAX_AGENT_VERSION_BYTES);
        info.os = "o".repeat(cedar_protocol::MAX_AGENT_PLATFORM_BYTES);
        info.arch = "a".repeat(cedar_protocol::MAX_AGENT_PLATFORM_BYTES);
        info.validate().unwrap();
        app.agent_info = Some(info);
        app.state = ConnectionState::Ready;
        app.active_form = Some(ConnectForm::default());
        app.recovery.enabled = true;
        app.recovery.error = Some("Backup needs review".into());
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(780.0, 540.0));
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ctx| app.footer(ctx),
        );
        let text = |prefix: &str| {
            output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::epaint::Shape::Text(text) if text.galley.text().starts_with(prefix) => {
                        Some((text.visual_bounding_rect(), shape.clip_rect))
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("Missing status text: {prefix}"))
        };
        let (recovery, recovery_clip) = text("Recovery needs attention");
        let (reported, agent_clip) = text("Reported agent");
        assert!(screen.contains_rect(recovery));
        assert!(recovery_clip.contains_rect(recovery));
        assert!(screen.contains_rect(reported));
        assert!(agent_clip.contains_rect(reported));
        assert!(reported.width() <= 242.0);
        assert!(recovery.right() < reported.left());
        assert!(app.agent_status().len() > 128);
        assert!(app.agent_details().contains("not verified identity"));
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
