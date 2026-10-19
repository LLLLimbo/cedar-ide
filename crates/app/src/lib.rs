//! Cedar IDE — a native Rust frontend for a local or SSH workspace agent.
mod agent_support;
mod block_indent;
#[cfg(test)]
mod block_indent_process_tests;
mod browse;
#[cfg(test)]
mod browse_tests;
#[cfg(test)]
mod browsing_shell_tests;
#[cfg(test)]
mod build_problem_process_tests;
#[cfg(test)]
mod build_problem_ui_tests;
mod build_problems;
#[cfg(test)]
mod case_search_open_tests;
#[cfg(test)]
mod case_search_tests;
mod chrome;
mod close_transport;
#[cfg(test)]
mod close_transport_tests;
#[cfg(test)]
mod column_navigation_process_tests;
pub mod completion;
#[cfg(test)]
mod connection_cancel_tests;
mod copy_draft;
#[cfg(test)]
mod copy_draft_process_tests;
mod disconnect;
#[cfg(test)]
mod disconnect_tests;
mod disk_merge;
#[cfg(test)]
mod disk_merge_process_tests;
mod disk_review;
mod duplicate_lines;
#[cfg(test)]
mod duplicate_lines_process_tests;
mod editor_state;
mod editor_tab;
#[cfg(test)]
mod editor_tab_entry_tests;
mod enter_indent;
#[cfg(test)]
mod enter_indent_process_tests;
#[cfg(test)]
mod explicit_disconnect_process_tests;
mod explorer_tree;
#[cfg(test)]
mod explorer_tree_process_tests;
#[cfg(test)]
mod explorer_tree_tests;
mod file_find;
mod git_ui;
#[cfg(test)]
mod idle_disconnect_process_tests;
#[cfg(test)]
mod idle_transport_tests;
#[cfg(test)]
mod ime_receipt_tests;
mod interrupted_save;
#[cfg(test)]
mod interrupted_save_process_tests;
mod java_language;
#[cfg(test)]
mod java_maven_metadata_tests;
#[cfg(test)]
mod java_parameter_info_acceptance_tests;
mod language_navigation_results;
mod language_results;
mod language_sync;
mod language_ui;
#[cfg(test)]
mod linux_local_tests;
#[cfg(test)]
mod loaded_file_process_tests;
mod location_history;
#[cfg(test)]
mod location_history_process_tests;
mod model;
mod navigation;
mod profile_ui;
#[cfg(test)]
mod project_context_tests;
#[cfg(test)]
mod project_text_preview_tests;
mod recent_files;
mod recovery;
mod recovery_actor;
#[cfg(test)]
mod recovery_lifecycle_tests;
#[cfg(test)]
mod recovery_tests;
mod recovery_ui;
mod replace;
mod report_keyboard;
#[cfg(test)]
mod report_keyboard_tests;
#[cfg(test)]
mod report_triage_process_tests;
mod run_ui;
#[cfg(test)]
mod save_ack_process_tests;
#[cfg(test)]
mod save_ack_tests;
mod save_all;
#[cfg(test)]
mod save_all_process_tests;
#[cfg(test)]
mod save_all_tests;
#[cfg(test)]
mod save_close_process_tests;
#[cfg(test)]
mod save_close_tests;
#[cfg(test)]
mod scoped_search_process_tests;
#[cfg(test)]
mod case_search_process_tests;
#[cfg(test)]
mod scoped_search_tests;
mod search_keyboard;
#[cfg(test)]
mod search_keyboard_tests;
#[cfg(test)]
mod search_navigation_process_tests;
#[cfg(test)]
mod selection_replace_process_tests;
#[cfg(test)]
mod sidebar_layout_tests;
mod sidebar_reveal;
#[cfg(test)]
mod ssh_preflight_process_tests;
mod syntax;
mod system_fonts;
mod tab_close;
#[cfg(test)]
mod tab_focus_tests;
pub mod task_profiles;
#[cfg(test)]
mod test_report_process_tests;
mod test_report_ui;
#[cfg(test)]
mod test_report_ui_tests;
mod test_reports;
pub mod text_edits;
mod tools_resize;
#[cfg(test)]
mod tools_resize_tests;
#[cfg(test)]
mod unavailable_recovery_process_tests;
#[cfg(test)]
mod welcome_layout_tests;
mod worker;
mod workspace_access;
#[cfg(test)]
mod workspace_access_tests;

use cedar_client::ConnectionSpec;
use cedar_protocol::{AgentInfo, Entry, Operation, Payload, SearchMatch};
use eframe::egui::{self, Color32, FontId, RichText, Stroke};
use model::{cursor_location, language, line_start, parent_path, Document};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender},
};
use worker::{Command, Event, Worker, WorkerEvent};

const BG: Color32 = chrome::SURFACE;
const PANEL: Color32 = chrome::SURFACE;
const BORDER: Color32 = chrome::BORDER;
const MUTED: Color32 = chrome::MUTED;
const TEXT: Color32 = chrome::TEXT;
const GREEN: Color32 = Color32::from_rgb(146, 215, 174);
const AMBER: Color32 = Color32::from_rgb(234, 185, 117);
const RED: Color32 = Color32::from_rgb(243, 150, 154);

#[derive(Clone, Copy, PartialEq, Eq)]
enum ConnectionState {
    Idle,
    Connecting,
    Ready,
    Disconnected,
    Disconnecting,
    ClosingTransport,
    CleanupUnverified,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    Search,
    Git,
    Run,
    Language,
    Tests,
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
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
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
            // Use the transport's pure argument validation before connect can
            // retire the current worker or reset any workspace state. Building
            // these arguments performs no SSH, configuration or filesystem I/O.
            cedar_client::ssh_arguments(
                self.host.trim(),
                port,
                self.remote_root.trim(),
                self.agent.trim(),
                self.allow_run,
            )?;
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
            #[cfg(target_os = "linux")]
            return Ok(ConnectionSpec::BundledLinux {
                root: PathBuf::from(self.local_root.trim()),
                allow_run: self.allow_run,
            });
            #[cfg(not(target_os = "linux"))]
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

// Project Text activation remains tied to one accepted snapshot and selected
// row. Its authority can retire while the physical Read remains drainable.
struct SearchOpenSource {
    snapshot: search_keyboard::Source,
    index: usize,
    text_identity: egui::Id,
    active: bool,
}

enum Job {
    BrowseFindFiles {
        ticket: browse::DiscoveryTicket,
    },
    BrowsePreviewRead {
        ticket: browse::PreviewTicket,
    },
    List {
        path: String,
    },
    TreeList {
        ticket: explorer_tree::Ticket,
    },
    Open {
        path: String,
        line: Option<usize>,
        navigation: u64,
    },
    SearchOpen {
        path: String,
        line: Option<usize>,
        navigation: u64,
        source: SearchOpenSource,
    },
    LanguageOpen {
        path: String,
        navigation: u64,
        session: u64,
        sequence: u64,
    },
    BuildProblemOpen {
        path: String,
        line: Option<usize>,
        navigation: u64,
        source: run_ui::BuildSource,
    },
    JavaImplementationOpen {
        path: String,
        navigation: u64,
        context: language_ui::JavaImplementationContext,
    },
    FileStructureOpen {
        path: String,
        navigation: u64,
        ticket: language_ui::FileStructureTicket,
    },
    JavaTypeOpen {
        path: String,
        navigation: u64,
        context: language_ui::JavaTypeContext,
    },
    Save {
        document: u64,
        snapshot: String,
        submission: Option<interrupted_save::InterruptedSave>,
    },
    Search {
        specification: search_keyboard::Specification,
    },
    Git,
    GitRead(git_ui::Action),
    ProfilesLoad {
        epoch: u64,
    },
    TestReportRead(test_report_ui::Load),
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

// Only the actual header render can identify its egui menu bar and roots.
// Keep that evidence in its Context, valid for the next pass only.
#[derive(Clone, Copy)]
struct HeaderMenus {
    pass: u64,
    viewport: egui::ViewportId,
    bar: egui::Id,
    roots: [egui::Id; 2],
}

impl HeaderMenus {
    fn key() -> egui::Id {
        egui::Id::new("cedar_header_menus")
    }
}

#[derive(Clone, Default)]
struct IslandMenus {
    pass: u64,
    viewport: egui::ViewportId,
    roots: Vec<(egui::Id, egui::Id)>,
}

// A Response owns a strong Context reference. Never put one inside that same
// Context's temporary data: the resulting cycle retains every test UI/font atlas.
#[cfg(test)]
#[derive(Clone, Copy, Debug)]
struct EditorScrollResponseSnapshot {
    id: egui::Id,
    interact_rect: egui::Rect,
    senses_drag: bool,
    dragged: bool,
}

#[cfg(test)]
impl EditorScrollResponseSnapshot {
    fn capture(response: egui::Response) -> Self {
        Self {
            id: response.id,
            interact_rect: response.interact_rect,
            senses_drag: response.sense.senses_drag(),
            dragged: response.dragged(),
        }
    }
}

fn register_island_menu(ui: &egui::Ui, root: egui::Id) {
    let ctx = ui.ctx();
    let pass = ctx.cumulative_pass_nr();
    let viewport = ctx.viewport_id();
    let bar = ui.id();
    ctx.data_mut(|data| {
        let menus =
            data.get_temp_mut_or_default::<IslandMenus>(egui::Id::new("cedar_island_menus"));
        if menus.pass != pass || menus.viewport != viewport {
            menus.roots.clear();
            menus.pass = pass;
            menus.viewport = viewport;
        }
        menus.roots.push((bar, root));
    });
}

pub struct CedarApp {
    editor_ctx: egui::Context,
    enter_input: Option<enter_indent::FrameInput>,
    duplicate_input: Option<duplicate_lines::FrameInput>,
    editor_tab_input: Option<editor_tab::FrameInput>,
    system_fonts: system_fonts::SystemFonts,
    cjk_seen: bool,
    state: ConnectionState,
    unverified_local_close: bool,
    // One bounded warning lineage; retired attempts cannot adopt workspace state.
    unverified_attempt_generation: Option<u64>,
    form: ConnectForm,
    active_form: Option<ConnectForm>,
    workspace_key: Option<WorkspaceKey>,
    connecting_form: Option<ConnectForm>,
    // Validated, immutable support claims for this accepted connection only.
    agent_info: Option<AgentInfo>,
    file_discovery: Option<cedar_protocol::FileDiscoveryDescriptor>,
    scoped_search: Option<cedar_protocol::ScopedSearchDescriptor>,
    case_search: Option<cedar_protocol::CaseSearchDescriptor>,
    root: String,
    generation: u64,
    next_request: u64,
    next_document: u64,
    navigation_epoch: u64,
    worker: Option<Worker>,
    owner_ledger: close_transport::OwnerLedger,
    active_owner: Option<close_transport::OwnerTicket>,
    transport_close: Option<close_transport::CloseIntent>,
    next_transport_close: u64,
    connection_generation_exhausted: bool,
    #[cfg(test)]
    close_frame_mutation: Option<fn(&mut CedarApp, &egui::Context)>,
    result_tx: Sender<WorkerEvent>,
    result_rx: Receiver<WorkerEvent>,
    pending: HashMap<u64, Job>,
    documents: Vec<Document>,
    active_document: Option<u64>,
    close_tab_requested: Option<u64>,
    tab_close: Option<tab_close::Intent>,
    tab_close_submission: Option<tab_close::Submission>,
    tab_close_focus: tab_close::FocusReturn,
    directory: String,
    entries: Vec<Entry>,
    directory_request: u64,
    explorer: explorer_tree::Explorer,
    open_form: bool,
    confirm: Option<Confirm>,
    allow_close: bool,
    close_after_language_stop: bool,
    close_snapshot: Option<Vec<(u64, u64)>>,
    error: Option<String>,
    notice: String,
    tool: Tool,
    tools_open: bool,
    // Runtime-only preference, established by a deliberate native-edge drag.
    tools_height: Option<f32>,
    tools_height_reset: tools_resize::ResetGuard,
    search_query: String,
    search_scope: String,
    search_masks: String,
    search_match_case: bool,
    search_results: Vec<SearchMatch>,
    search_truncated: bool,
    search_request: u64,
    search_state: search_keyboard::Search,
    git_output: String,
    git_state: git_ui::GitPanel,
    profiles: profile_ui::Profiles,
    run_state: run_ui::RunPanel,
    test_report: test_report_ui::TestReportPanel,
    language: language_ui::LanguagePanel,
    navigation: navigation::Navigation,
    browse: browse::Browse,
    recent_files: recent_files::RecentFiles,
    location_history: location_history::LocationHistory,
    workspace_access: workspace_access::Access,
    new_file: bool,
    new_path: String,
    copy_draft: copy_draft::CopyDraft,
    find_open: bool,
    find_query: String,
    file_find: file_find::State,
    find_focus: bool,
    replace: replace::Replace,
    disk_review: disk_review::DiskReview,
    interrupted_save_check: interrupted_save::Check,
    save_all: save_all::SaveAll,
    font_size: f32,
    recovery: recovery::Recovery,
}

impl CedarApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = chrome::FRAME;
        visuals.window_fill = PANEL;
        visuals.extreme_bg_color = BG;
        visuals.faint_bg_color = chrome::FRAME;
        visuals.override_text_color = Some(TEXT);
        visuals.selection.bg_fill = chrome::SELECTED;
        visuals.selection.stroke = Stroke::new(1.0_f32, chrome::FOCUS);
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER);
        visuals.widgets.inactive.bg_fill = chrome::FRAME;
        visuals.widgets.hovered.bg_fill = chrome::HOVER;
        visuals.widgets.active.bg_fill = chrome::SELECTED;
        cc.egui_ctx.set_visuals(visuals);
        cc.egui_ctx.set_fonts(system_fonts::base_fonts());
        cc.egui_ctx.style_mut(|style| {
            style.spacing.item_spacing = egui::vec2(6.0, 4.0);
            style.spacing.button_padding = egui::vec2(8.0, 4.0);
            style
                .text_styles
                .insert(egui::TextStyle::Body, FontId::proportional(13.0));
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
            enter_input: None,
            duplicate_input: None,
            editor_tab_input: None,
            system_fonts: system_fonts::SystemFonts::default(),
            cjk_seen: false,
            state: ConnectionState::Idle,
            unverified_local_close: false,
            unverified_attempt_generation: None,
            form: ConnectForm::default(),
            active_form: None,
            workspace_key: None,
            connecting_form: None,
            agent_info: None,
            file_discovery: None,
            scoped_search: None,
            case_search: None,
            root: String::new(),
            generation: 0,
            next_request: 1,
            next_document: 1,
            navigation_epoch: 0,
            worker: None,
            owner_ledger: close_transport::OwnerLedger::default(),
            active_owner: None,
            transport_close: None,
            next_transport_close: 0,
            connection_generation_exhausted: false,
            #[cfg(test)]
            close_frame_mutation: None,
            result_tx,
            result_rx,
            pending: HashMap::new(),
            documents: Vec::new(),
            active_document: None,
            close_tab_requested: None,
            tab_close: None,
            tab_close_submission: None,
            tab_close_focus: tab_close::FocusReturn::default(),
            directory: String::new(),
            entries: Vec::new(),
            directory_request: 0,
            explorer: explorer_tree::Explorer::default(),
            open_form: true,
            confirm: None,
            allow_close: false,
            close_after_language_stop: false,
            close_snapshot: None,
            error: None,
            notice: "Ready when you are".into(),
            tool: Tool::Search,
            tools_open: false,
            tools_height: None,
            tools_height_reset: tools_resize::ResetGuard::default(),
            search_query: String::new(),
            search_scope: String::new(),
            search_masks: String::new(),
            search_match_case: true,
            search_results: Vec::new(),
            search_truncated: false,
            search_request: 0,
            search_state: search_keyboard::Search::default(),
            git_output: "Refresh to read workspace Git status".into(),
            git_state: git_ui::GitPanel::default(),
            profiles: profile_ui::Profiles::default(),
            run_state: run_ui::RunPanel::default(),
            test_report: test_report_ui::TestReportPanel::default(),
            language: language_ui::LanguagePanel::default(),
            navigation: navigation::Navigation::default(),
            browse: browse::Browse::default(),
            recent_files: recent_files::RecentFiles::default(),
            location_history: location_history::LocationHistory::default(),
            workspace_access: workspace_access::Access::default(),
            new_file: false,
            new_path: String::new(),
            copy_draft: copy_draft::CopyDraft::default(),
            find_open: false,
            find_query: String::new(),
            file_find: file_find::State::default(),
            find_focus: false,
            replace: replace::Replace::default(),
            disk_review: disk_review::DiskReview::default(),
            interrupted_save_check: interrupted_save::Check::default(),
            save_all: save_all::SaveAll::default(),
            font_size: 14.0,
            recovery: recovery::Recovery::default(),
        }
    }

    fn ready(&self) -> bool {
        self.state == ConnectionState::Ready && !self.connection_generation_exhausted
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
        self.save_all_busy()
            || self.save_close_tab_busy()
            || self.save_close_write_pending()
            || self.language.startup_active()
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
        if let Some(problem) = self.connection_admission_problem() {
            self.error = Some(problem.into());
            return;
        }
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
        // Reserve ownership before any Worker starts; admission failure does
        // not retire the current connection or invent a new process owner.
        let Some(next_generation) = self.generation.checked_add(1) else {
            self.error =
                Some("Connection generations are exhausted; no new connection was started".into());
            return;
        };
        let owner = match self.owner_ledger.register(next_generation) {
            Ok(owner) => owner,
            Err(error) => {
                self.error = Some(error);
                return;
            }
        };
        self.recovery.restoring_generation = None;
        self.reset_git(self.workspace_key.as_ref() != Some(&form.key()));
        self.dismiss_disk_review();
        self.run_state.reset();
        self.test_report.clear();
        self.profiles.disconnected();
        self.retire_current_worker();
        self.language.reset();
        self.search_state.retire();
        self.reset_recent_files();
        self.retire_ui_generation();
        self.location_history.clear();
        self.pending.clear();
        self.explorer.reset_connection();
        self.disk_review.outstanding = None;
        self.interrupted_save_check.reset();
        for doc in &mut self.documents {
            doc.saving = false;
        }
        self.agent_info = None;
        self.file_discovery = None;
        self.scoped_search = None;
        self.case_search = None;
        self.state = ConnectionState::Connecting;
        self.notice = format!("Connecting to {}...", form.label());
        self.error = None;
        self.connecting_form = Some(form);
        self.active_owner = Some(owner);
        match Worker::spawn(spec, owner, self.result_tx.clone(), ctx.clone()) {
            Ok(worker) => self.worker = Some(worker),
            Err(error) => {
                // The captured guard reports Unverified; do not invent a second receipt.
                self.retire_current_worker();
                self.connecting_form = None;
                self.state = if self.workspace_key.is_some() {
                    ConnectionState::Disconnected
                } else {
                    ConnectionState::Idle
                };
                self.error = Some(error);
            }
        }
    }

    fn cancel_connection(&mut self) {
        if self.state != ConnectionState::Connecting {
            return;
        }
        self.dismiss_disk_review();
        self.reset_git(false);
        self.test_report.clear();
        self.retire_current_worker();
        self.disk_review.outstanding = None;
        self.agent_info = None;
        self.file_discovery = None;
        self.scoped_search = None;
        self.case_search = None;
        self.search_state.retire();
        self.reset_recent_files();
        self.retire_ui_generation();
        self.location_history.clear();
        self.explorer.reset_connection();
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
        self.reset_recent_files();
        self.settle_tab_close_transport_loss();
        self.cancel_tab_close();
        self.location_history.cancel_pending();
        // Transport loss is never proof that language cleanup finished, even
        // when an ordinary file/task request reports the loss first.
        self.close_after_language_stop = false;
        self.close_snapshot = None;
        self.recovery.language_close_guard = None;
        self.retain_interrupted_saves();
        self.save_all_transport_lost();
        self.reset_git(false);
        self.dismiss_disk_review();
        self.run_state.disconnected();
        self.test_report.disconnected();
        self.search_state.retire();
        self.profiles.disconnected();
        self.recovery.restoring_generation = None;
        self.state = ConnectionState::Disconnected;
        self.connecting_form = None;
        self.agent_info = None;
        self.file_discovery = None;
        self.scoped_search = None;
        self.case_search = None;
        self.language.disconnected();
        self.retire_current_worker();
        self.pending.clear();
        self.explorer.reset_connection();
        self.disk_review.outstanding = None;
        self.interrupted_save_check.reset();
        for doc in &mut self.documents {
            doc.saving = false;
        }
        self.error = Some(message);
        self.notice = "Disconnected · drafts retained".into();
    }

    fn list(&mut self, path: String) {
        self.explorer_list_flat(path);
    }

    fn navigation_changed(&mut self) {
        self.cancel_project_reveal();
        self.file_find.cancel_loop();
        self.location_history.cancel_pending();
        self.navigation.cancel_coordinate_jump();
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

    pub(crate) fn open_search_result(&mut self, index: usize) {
        if !self.search_rows_active()
            || self.search_state.selected_index() != Some(index)
            || !self.search_state.visible(index)
        {
            return;
        }
        let (Some(snapshot), Some(row)) = (
            self.search_state.accepted.as_ref(),
            self.search_results.get(index),
        ) else {
            return;
        };
        let source = SearchOpenSource {
            snapshot: snapshot.clone(),
            index,
            text_identity: egui::Id::new(&row.text),
            active: true,
        };
        let path = row.path.clone();
        let line = Some(row.line);
        // Preserve ordinary dirty-tab reuse, history, and same-path physical
        // Read reuse. Only a pending Open gains this additional authority.
        self.open(path.clone(), line);
        let navigation = self.navigation_epoch;
        if let Some(job) = self.pending.values_mut().find(|job| {
            matches!(job, Job::Open { path: pending, line: hint, navigation: epoch }
                if pending == &path && *hint == line && *epoch == navigation)
        }) {
            *job = Job::SearchOpen {
                path,
                line,
                navigation,
                source,
            };
        }
    }

    fn search_open_source_current(
        &self,
        source: &SearchOpenSource,
        path: &str,
        line: Option<usize>,
    ) -> bool {
        source.active
            && self.search_rows_active()
            && self.search_state.selected_index() == Some(source.index)
            && self.search_state.accepted.as_ref() == Some(&source.snapshot)
            && self.search_results.get(source.index).is_some_and(|row| {
                row.path == path
                    && Some(row.line) == line
                    && egui::Id::new(&row.text) == source.text_identity
            })
    }

    pub(crate) fn retire_search_result_opens(&mut self) {
        let mut retired = Vec::new();
        for job in self.pending.values_mut() {
            if let Job::SearchOpen {
                source, navigation, ..
            } = job
            {
                if source.active {
                    source.active = false;
                    retired.push(*navigation);
                }
            }
        }
        for navigation in retired {
            // A newer ordinary Open may already own a different history ticket.
            self.history_cancel_ticket(navigation);
        }
    }

    fn cache_retired_search_open(
        &mut self,
        request: u64,
        path: &str,
        result: Result<Payload, String>,
    ) {
        let Ok(Payload::File {
            path: returned,
            text,
            revision,
        }) = result
        else {
            return;
        };
        if returned != path {
            return;
        }
        // A matching completed Read is disk evidence even after its activation
        // retires, and even if an existing draft or full cache prevents insertion.
        self.observe_maven_pom_acknowledgement(request, &returned, &revision);
        if self.documents.iter().any(|doc| doc.path == path) || self.documents.len() >= 32 {
            return;
        }
        let Ok(id) = self.allocate_document_id() else {
            return;
        };
        // Keep safe inactive caching without caret, focus, history, or error changes.
        self.documents
            .push(Document::new(id, returned, text, revision));
    }

    fn open_with_build_source(
        &mut self,
        path: String,
        line: Option<usize>,
        source: Option<run_ui::BuildSource>,
    ) {
        self.open_with_history(path, line, source, None, false);
    }

    fn open_with_history(
        &mut self,
        path: String,
        line: Option<usize>,
        source: Option<run_ui::BuildSource>,
        transfer: Option<location_history::Admission>,
        language_target: bool,
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
        let departure = (!language_target)
            .then(|| self.history_departure())
            .flatten();
        self.navigation_changed();
        if let Some(ticket) = transfer {
            self.history_resume(ticket);
        } else if !language_target {
            self.history_begin(departure, false);
        }
        let navigation = self.navigation_epoch;
        if let Some(doc) = self.documents.iter_mut().find(|doc| doc.path == path) {
            let document = doc.id;
            if !language_target {
                self.active_document = Some(document);
                if let Some(line) = line {
                    doc.jump_to = Some(line_start(&doc.text, line));
                }
                self.navigation.restore_focus = true;
                if doc.jump_to.is_some() {
                    self.history_wait_for_selection(document);
                } else {
                    self.history_complete_open();
                }
            }
            return;
        }
        if self.documents.len() >= 32 {
            self.history_cancel_ticket(navigation);
            self.error = Some(
                "Close a tab before opening another. Cedar limits the workspace to 32 buffers"
                    .into(),
            );
            return;
        }
        let pending = self.pending.iter().find_map(|(id, job)| match job {
            Job::Open { path: pending, .. }
            | Job::SearchOpen { path: pending, .. }
            | Job::LanguageOpen { path: pending, .. }
            | Job::BuildProblemOpen { path: pending, .. }
            | Job::FileStructureOpen { path: pending, .. }
            | Job::JavaTypeOpen { path: pending, .. }
            | Job::JavaImplementationOpen { path: pending, .. }
                if pending == &path =>
            {
                Some(*id)
            }
            _ => None,
        });
        let op = Operation::Read { path: path.clone() };
        let job = if language_target {
            self.language_open_job(path, navigation)
        } else {
            match source {
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
            }
        };
        if let Some(id) = pending {
            self.pending.insert(id, job);
        } else if self.request(op, job) == 0 {
            self.location_history.cancel_pending();
        }
    }

    fn save(&mut self) {
        if let Some(id) = self.active_document {
            self.save_document(id);
        }
    }

    fn save_document(&mut self, id: u64) {
        if self.save_close_tab_busy() || self.save_close_write_pending() {
            self.error = Some(
                "Finish Save and close, or Keep editing and wait for its save before saving again"
                    .into(),
            );
            return;
        }
        if self.save_all_busy() {
            self.error = Some("Save All owns the pending saves. Cancel remaining saves and wait for the in-flight save before saving another file".into());
            return;
        }
        self.save_document_now(id);
    }

    fn save_document_now(&mut self, id: u64) {
        let Some(doc) = self.documents.iter().find(|doc| doc.id == id) else {
            return;
        };
        if !doc.dirty() || doc.saving {
            return;
        }
        if doc.save_outcome_unverifiable {
            self.error = Some("The save outcome is unknown and its submission identity is unavailable. Keep or copy your draft and compare disk; further saves are blocked for this tab".into());
            return;
        }
        if doc.save_outcome_unknown() || self.interrupted_save_check.busy() {
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

    #[cfg(test)]
    fn poll(&mut self) {
        self.poll_ready_navigation(None);
    }

    fn poll_ready_navigation(&mut self, ctx: Option<&egui::Context>) {
        let newer_input = ctx.is_some_and(|ctx| {
            self.history_newer_completion_input(ctx) || self.java_type_newer_input(ctx)
        });
        loop {
            let event = match self.result_rx.try_recv() {
                Ok(event) => event,
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.owner_channel_lost();
                    break;
                }
            };
            // Keep worker response/transport-loss ordering intact. Only a ready,
            // connected reply that could still navigate is superseded by input
            // already queued for this frame; earlier caret/typing frames do not
            // cancel requests that have not replied yet.
            if newer_input && !self.ordinary_worker_ui_blocked() {
                if let WorkerEvent::Response(reply) = &event {
                    let navigation = self.pending.get(&reply.id).and_then(|job| match job {
                        Job::Open { navigation, .. } => Some(*navigation),
                        Job::SearchOpen {
                            path,
                            line,
                            navigation,
                            source,
                        } if self.search_open_source_current(source, path, *line) => {
                            Some(*navigation)
                        }
                        Job::LanguageOpen {
                            navigation,
                            session,
                            sequence,
                            ..
                        } if self.language_open_current(*session, *sequence) => Some(*navigation),
                        Job::BuildProblemOpen {
                            navigation, source, ..
                        } if self.build_source_is_current(*source) => Some(*navigation),
                        Job::JavaTypeOpen {
                            navigation,
                            context,
                            ..
                        } if self.java_type_context_current(context) => Some(*navigation),
                        Job::JavaImplementationOpen {
                            navigation,
                            context,
                            ..
                        } if self.java_implementation_context_current(context) => Some(*navigation),
                        Job::Language(action) => self.language_navigation_owner(action),
                        _ => None,
                    });
                    if reply.connected
                        && reply.generation == self.generation
                        && navigation == Some(self.navigation_epoch)
                    {
                        self.history_supersede_ready_navigation(
                            ctx,
                            workspace_access::ReadyNavigation {
                                request: reply.id,
                                generation: reply.generation,
                                navigation: self.navigation_epoch,
                            },
                        );
                    }
                }
            }
            self.apply_worker_event(event);
        }
    }

    fn apply_worker_event(&mut self, event: WorkerEvent) {
        // Owner evidence spans the App lifetime, including retired UI generations.
        let event = match event {
            WorkerEvent::OwnerSettled(receipt) => {
                self.apply_owner_receipt(receipt);
                return;
            }
            event => event,
        };
        if self.ordinary_worker_ui_blocked() {
            return;
        }
        match event {
            WorkerEvent::OwnerSettled(_) => unreachable!("owner receipt already handled"),
            WorkerEvent::AttemptCleanupUnverified { generation } => {
                if generation <= self.generation
                    && self
                        .unverified_attempt_generation
                        .is_none_or(|old| generation > old)
                {
                    self.unverified_attempt_generation = Some(generation);
                    self.unverified_local_close = true;
                }
            }
            WorkerEvent::Response(event) => self.apply_event(event),
            WorkerEvent::Closed { generation, result } => {
                self.connection_closed(generation, result)
            }
            WorkerEvent::TransportLost {
                generation,
                message,
            } if generation == self.generation
                && matches!(
                    self.state,
                    ConnectionState::Connecting | ConnectionState::Ready
                ) =>
            {
                self.disconnected(message);
            }
            WorkerEvent::TransportLost { .. } => {}
        }
    }

    fn apply_event(&mut self, event: Event) {
        self.apply_event_inner(event, true);
    }

    fn apply_event_inner(&mut self, event: Event, defer_implementation: bool) {
        if event.generation != self.generation
            || self.state == ConnectionState::Disconnecting
            || self.ordinary_worker_ui_blocked()
        {
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
                    file_discovery,
                    scoped_search,
                    case_search,
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
                    if let Some(descriptor) = &file_discovery {
                        if let Err(error) = descriptor.validate() {
                            self.disconnected(error.to_string());
                            return;
                        }
                    }
                    if let Some(descriptor) = &scoped_search {
                        if let Err(error) = descriptor.validate() {
                            self.disconnected(error.to_string());
                            return;
                        }
                    }
                    if let Some(descriptor) = &case_search {
                        if let Err(error) = descriptor.validate() {
                            self.disconnected(error.to_string());
                            return;
                        }
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
                        self.location_history.clear();
                        self.documents.clear();
                        self.active_document = None;
                        self.search_results.clear();
                        self.search_state.clear_owned();
                        self.search_truncated = false;
                        self.reset_git(true);
                        self.run_state.output = "Command output will appear here".into();
                    }
                    self.profiles.connected(recovery_ui::identity(&form, &root));
                    self.location_history.clear();
                    self.workspace_key = Some(key);
                    self.active_form = Some(form);
                    self.root = root;
                    self.agent_info = agent;
                    self.file_discovery = file_discovery;
                    self.scoped_search = scoped_search;
                    self.case_search = case_search;
                    self.state = ConnectionState::Ready;
                    self.recent_connected();
                    self.error = None;
                    self.notice = "Workspace connected".into();
                    self.open_form = false;
                    self.directory.clear();
                    self.entries.clear();
                    self.explorer.reset_connection();
                    if self.explorer.mode == explorer_tree::Mode::Flat {
                        self.list(String::new());
                    }
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
        if defer_implementation
            && event.connected
            && self.pending.get(&event.id).is_some_and(|job| {
                matches!(job, Job::FileStructureOpen { .. })
                    || matches!(job, Job::Language(action) if action.is_file_structure())
            })
        {
            self.defer_file_structure_reply(event);
            return;
        }
        // Parameter replies wait until every current-frame draft/navigation
        // finisher has run. Current transport loss is still handled immediately.
        if defer_implementation
            && event.connected
            && self.pending.get(&event.id).is_some_and(
                |job| matches!(job, Job::Language(action) if action.is_java_parameter_info()),
            )
        {
            self.defer_java_parameter_reply(event);
            return;
        }
        // Poll runs before this frame's editor and Cancel input. Keep completed
        // implementation replies pending until that input has been processed;
        // only this new route changes timing, and transport loss remains immediate.
        if defer_implementation
            && event.connected
            && self.pending.get(&event.id).is_some_and(|job| {
                matches!(job, Job::JavaImplementationOpen { .. })
                    || matches!(job, Job::Language(action) if action.is_java_implementation())
            })
        {
            self.defer_java_implementation_reply(event);
            return;
        }
        let invalid_save_ack = self.pending.get(&event.id).is_some_and(|job| {
            matches!(job, Job::Save { snapshot, .. } if match &event.result {
                Ok(Payload::Written { revision }) => !event.connected || !self.save_submission_is_current(event.id)
                    || !interrupted_save::acknowledgement_matches(snapshot, revision),
                Ok(_) => true,
                Err(_) => false,
            })
        });
        self.save_all_observe_reply(
            event.generation,
            event.id,
            !invalid_save_ack
                && event.connected
                && matches!(self.pending.get(&event.id), Some(Job::Save { .. }))
                && matches!(event.result, Ok(Payload::Written { .. })),
            invalid_save_ack
                || !event.connected
                || !matches!(self.pending.get(&event.id), Some(Job::Save { .. })),
        );
        self.save_close_observe_reply(
            event.generation,
            event.id,
            !invalid_save_ack
                && event.connected
                && matches!(self.pending.get(&event.id), Some(Job::Save { .. }))
                && matches!(event.result, Ok(Payload::Written { .. })),
        );
        if invalid_save_ack || !event.connected {
            self.retain_interrupted_save(event.id);
        }
        let Some(job) = self.pending.remove(&event.id) else {
            return;
        };
        if let Job::Language(language_ui::Action {
            kind: language_ui::ActionKind::JavaParameterInfo { context },
            ..
        }) = job
        {
            self.apply_java_parameter_event(context, event.result, event.connected);
            return;
        }
        if let Job::BrowseFindFiles { ticket } = job {
            self.apply_browse_discovery(event.id, ticket, event.result, event.connected);
            return;
        }
        if let Job::BrowsePreviewRead { ticket } = job {
            self.apply_browse_preview(event.id, ticket, event.result, event.connected);
            return;
        }
        if let Job::Search { specification } = job {
            // Retired search outcomes cannot replace the current view/error,
            // but a current transport failure still disconnects the workspace.
            if !event.connected {
                self.disconnected(event.result.err().unwrap_or_else(|| {
                    "The connection closed during project search. Your drafts are retained".into()
                }));
            } else {
                if self.browse_owns_search(event.id) {
                    self.stage_browse_search(event.id, specification, event.result);
                } else {
                    self.stage_search_response(event.id, specification, event.result);
                }
            }
            return;
        }
        let job = if let Job::SearchOpen {
            path,
            line,
            navigation,
            source,
        } = job
        {
            // Retired UI authority cannot conceal a current transport failure.
            if !event.connected {
                self.disconnected(event.result.err().unwrap_or_else(|| {
                    "The connection closed while opening a search result. Your drafts are retained".into()
                }));
                return;
            }
            if navigation != self.navigation_epoch
                || !self.search_open_source_current(&source, &path, line)
            {
                self.history_cancel_ticket(navigation);
                self.cache_retired_search_open(event.id, &path, event.result);
                return;
            }
            Job::Open {
                path,
                line,
                navigation,
            }
        } else {
            job
        };
        let job = match job {
            Job::List { path } => {
                self.explorer_apply_list(event, path, None);
                return;
            }
            Job::TreeList { ticket } => {
                self.explorer_apply_list(event, ticket.path.clone(), Some(ticket));
                return;
            }
            other => other,
        };
        if let Job::Language(action) = &job {
            if action.is_file_structure() && !self.file_structure_action_current(action) {
                if !event.connected {
                    self.disconnected(
                        "The connection closed during File Structure. Your drafts are retained"
                            .into(),
                    );
                }
                return;
            }
        }
        let job = if let Job::FileStructureOpen {
            path,
            navigation,
            ticket,
        } = job
        {
            if !event.connected {
                self.disconnected(
                    "The connection closed while opening a symbol. Your drafts are retained".into(),
                );
                return;
            }
            if navigation != self.navigation_epoch || !self.file_structure_ticket_current(&ticket) {
                return;
            }
            self.finish_file_structure_navigation();
            Job::Open {
                path,
                line: None,
                navigation,
            }
        } else {
            job
        };
        let job = if let Job::LanguageOpen {
            path,
            navigation,
            session,
            sequence,
        } = job
        {
            if !event.connected {
                self.disconnected("The connection closed while opening a language target. Your drafts are retained".into());
                return;
            }
            if navigation != self.navigation_epoch || !self.language_open_current(session, sequence)
            {
                self.history_cancel_ticket(navigation);
                return;
            }
            Job::Open {
                path,
                line: None,
                navigation,
            }
        } else {
            job
        };
        let job = if let Job::JavaImplementationOpen {
            path,
            navigation,
            context,
        } = job
        {
            if !event.connected {
                self.disconnected("The connection closed while opening an implementation. Your drafts are retained".into());
                return;
            }
            if navigation != self.navigation_epoch
                || !self.java_implementation_context_current(&context)
            {
                return;
            }
            if let Ok(Payload::File {
                path: returned,
                text,
                ..
            }) = &event.result
            {
                if returned == &path {
                    if let Some(error) = self.java_implementation_read_problem(&path, text) {
                        self.java_implementation_error(&context, &error);
                        self.error = Some(error);
                        return;
                    }
                }
            }
            self.java_implementation_read_finished();
            Job::Open {
                path,
                line: None,
                navigation,
            }
        } else {
            job
        };
        if let Job::Language(action) = &job {
            if !self.java_implementation_action_current(action) {
                if !event.connected {
                    self.disconnected("The connection closed during implementation navigation. Your drafts are retained".into());
                }
                return;
            }
        }
        let job = if let Job::JavaTypeOpen {
            path,
            navigation,
            context,
        } = job
        {
            // A dismissed/replaced index query cannot open a late file, change
            // selection, or surface its old error. Transport failure still wins.
            if !event.connected {
                self.disconnected(
                    "The connection closed while opening a Java type. Your drafts are retained"
                        .into(),
                );
                return;
            }
            if navigation != self.navigation_epoch || !self.java_type_context_current(&context) {
                return;
            }
            Job::Open {
                path,
                line: None,
                navigation,
            }
        } else {
            job
        };
        if let Job::Language(action) = &job {
            if matches!(
                action.kind,
                language_ui::ActionKind::WorkspaceSymbols { .. }
                    | language_ui::ActionKind::JavaTypeResolve { .. }
            ) {
                if !event.connected {
                    self.disconnected(
                        "The connection closed while finding a Java type. Your drafts are retained"
                            .into(),
                    );
                    return;
                }
                if !self.java_type_action_current(action) {
                    return;
                }
            }
        }
        if let Job::TestReportRead(load) = job {
            // A stale view cannot hide a failure of the current transport.
            if !event.connected {
                self.disconnected(
                    "The connection closed while reading the test report. Your drafts are retained"
                        .into(),
                );
            } else {
                self.apply_test_report_read(event.id, load, event.result);
            }
            return;
        }
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
        if matches!(&job, Job::Language(action) if action.is_maven_dependencies()) {
            let Job::Language(action) = job else {
                unreachable!()
            };
            self.apply_maven_dependencies_event(event.id, action, event.result, event.connected);
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
                self.apply_interrupted_save_read(event.id, ticket, event.result);
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
            self.apply_disk_read(event.id, ticket, purpose, event.result, event.connected);
            return;
        }
        if let Job::Save { document, .. } = &job {
            if let Some(doc) = self.documents.iter_mut().find(|doc| doc.id == *document) {
                doc.saving = false;
            }
        }
        if invalid_save_ack {
            if matches!(&job, Job::Save { document, .. } if !self.documents.iter().any(|doc| doc.id == *document))
            {
                // There is no current tab on which this response can act.
                if !event.connected {
                    self.disconnected(
                        "The connection closed after an unverifiable save response".into(),
                    );
                }
                return;
            }
            let unavailable = matches!(&job, Job::Save { document, .. } if self.documents.iter().any(|doc| doc.id == *document && doc.save_outcome_unverifiable));
            self.error = Some(if unavailable {
                "The save outcome is unknown and its submission identity is unavailable. Keep or copy your draft and compare disk; verification and further saves are blocked"
            } else {
                "The save acknowledgement could not be verified. Use Check interrupted save; your draft is retained"
            }.into());
            if !event.connected {
                self.disconnected(self.error.clone().unwrap());
            }
            return;
        }
        // A retired Stop cannot reset a newer language session or its pending
        // close intent. Transport closure still belongs to this connection.
        if matches!(&job, Job::Language(action) if action.is_stop() && action.session != self.language.session)
        {
            if !event.connected {
                self.disconnected("The connection closed while stopping a previous language session; drafts are retained.".into());
            }
            return;
        }
        let payload = match event.result {
            Ok(payload) => payload,
            Err(error) => {
                let navigation = match &job {
                    Job::Open { navigation, .. } => Some(*navigation),
                    Job::Language(action) => action.history_navigation(),
                    _ => None,
                };
                if let Some(navigation) = navigation {
                    self.history_cancel_ticket(navigation);
                }
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
                    self.recovery.language_close_guard = None;
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
                    self.history_cancel_ticket(navigation);
                    self.error = Some(
                        "Agent returned a different file path; the response was ignored".into(),
                    );
                    return;
                }
                self.observe_maven_pom_acknowledgement(event.id, &path, &revision);
                let language_target = self.language_navigation_pending(&requested);
                if let Some(doc) = self.documents.iter_mut().find(|doc| doc.path == path) {
                    let document = doc.id;
                    if navigation == self.navigation_epoch && !language_target {
                        self.active_document = Some(document);
                        self.navigation.restore_focus = true;
                        if let Some(line) = line {
                            doc.jump_to = Some(line_start(&doc.text, line));
                        }
                        if doc.jump_to.is_some() {
                            self.history_wait_for_selection(document);
                        } else {
                            self.history_complete_open();
                        }
                    }
                    self.complete_language_navigation(&requested);
                    return;
                }
                if self.documents.len() >= 32 {
                    self.history_cancel_ticket(navigation);
                    self.error = Some(
                        "The 32-buffer limit was reached; close a tab and open the file again"
                            .into(),
                    );
                    return;
                }
                let id = match self.allocate_document_id() {
                    Ok(id) => id,
                    Err(error) => {
                        self.history_cancel_ticket(navigation);
                        self.error = Some(error);
                        return;
                    }
                };
                let mut doc = Document::new(id, path, text, revision);
                if navigation == self.navigation_epoch && !language_target {
                    if let Some(line) = line {
                        doc.jump_to = Some(line_start(&doc.text, line));
                    }
                }
                self.documents.push(doc);
                if navigation == self.navigation_epoch && !language_target {
                    self.active_document = Some(id);
                    self.navigation.restore_focus = true;
                    self.open_form = false;
                    if line.is_some() {
                        self.history_wait_for_selection(id);
                    } else {
                        self.history_complete_open();
                    }
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
                if let Some(path) = self
                    .documents
                    .iter()
                    .find(|doc| doc.id == document)
                    .map(|doc| doc.path.clone())
                {
                    self.explorer_saved(&path);
                }
            }
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
            (Job::Language(action), _) if action.is_stop() => {
                let error = self.language_public_error(&action, "Unexpected Stop response");
                self.language_error(&action, &error);
                self.error = Some(error);
            }
            (Job::Language(action), _) if action.is_java_implementation() => {
                let error = "Unexpected implementation response; no locations were accepted";
                self.language_error(&action, error);
                self.error = Some(error.into());
            }
            (Job::Language(action), _)
                if matches!(
                    action.kind,
                    language_ui::ActionKind::RefreshJavaDiagnostics { .. }
                        | language_ui::ActionKind::WorkspaceSymbols { .. }
                ) =>
            {
                let error = if matches!(
                    action.kind,
                    language_ui::ActionKind::WorkspaceSymbols { .. }
                ) {
                    "Unexpected Java type search response; no results were accepted"
                } else {
                    "Unexpected Java diagnostic refresh response; diagnostic freshness is unchanged"
                }
                .to_owned();
                self.language_error(&action, &error);
                self.error = Some(error);
            }
            _ => {
                self.error =
                    Some("Unexpected agent response; no editor buffers were changed".into())
            }
        }
    }

    fn search_busy(&self) -> bool {
        self.pending
            .values()
            .any(|job| matches!(job, Job::Search { .. }))
    }

    fn search(&mut self) {
        if self.search_query.trim().is_empty() || self.search_busy() {
            return;
        }
        self.search_filters_changed();
        let specification = match self.search_specification() {
            Ok(specification) => specification,
            Err(error) => {
                self.error = Some(error);
                return;
            }
        };
        if self.next_request == 0 || self.next_request == u64::MAX {
            self.error = Some("Search cannot submit: the request identity counter is exhausted. Reopen Cedar to start a new session.".into());
            return;
        }
        let operation = match specification.route {
            search_keyboard::Route::Legacy => Operation::Search {
                query: specification.request.query.clone(),
                limit: specification.request.limit,
            },
            search_keyboard::Route::ScopedV1 => Operation::SearchIn {
                request: specification.request.clone(),
            },
            search_keyboard::Route::CaseV1 => Operation::SearchCase {
                request: specification.case_request(),
            },
        };
        self.search_request = self.request(
            operation,
            Job::Search {
                specification: specification.clone(),
            },
        );
        self.search_state.pending = if self.search_request == 0 {
            None
        } else {
            self.workspace_key
                .clone()
                .map(|workspace| search_keyboard::Source {
                    workspace,
                    root: self.root.clone(),
                    generation: self.generation,
                    request: self.search_request,
                    query: specification.request.query.clone(),
                    specification,
                })
        };
        self.begin_search_submission();
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
            let guard_matches = self
                .recovery
                .language_close_guard
                .take()
                .is_none_or(|guard| guard == self.recovery_close_guard());
            let versions_match = self.close_snapshot.take().as_ref() == Some(&current);
            if !guard_matches || (!versions_match && self.dirty()) {
                self.confirm = Some(Confirm::CloseWindow);
                self.notice = "A draft changed while the language server was stopping; confirm before quitting".into();
            } else {
                self.finish_recovery_close(ctx);
            }
        }
    }

    fn request_window_close(&mut self, ctx: &egui::Context) {
        if self.state == ConnectionState::ClosingTransport {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.recovery.visible = true;
            self.notice = self.transport_close_message();
            ctx.request_repaint();
            return;
        }
        if self.save_close_tab_busy() || self.save_close_write_pending() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.cancel_tab_close();
            self.notice = "Window close cancelled. Review the retained tab; a submitted save may still finish.".into();
        } else if self.save_all_busy() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.error = Some(
                "Cancel remaining Save All writes and wait for the in-flight save before closing"
                    .into(),
            );
        } else if self.state == ConnectionState::Disconnecting {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.notice = disconnect::WAITING.into();
        } else if self.close_after_language_stop {
            // A repeated window-close event must not create a second discard
            // dialog while the original approval waits for verified cleanup.
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.notice = "Waiting for language cleanup before closing".into();
        } else if self.recovery.resuming() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.notice =
                "Recovery is still resuming. Keep editing and wait before trying to quit again"
                    .into();
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
        if self.state == ConnectionState::ClosingTransport {
            self.request_window_close(ctx);
            return;
        }
        if self.save_close_tab_busy() || self.save_close_write_pending() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.cancel_tab_close();
            self.notice =
                "Wait for the submitted save and review the retained tab before quitting.".into();
            return;
        }
        if self.recovery.resuming() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.notice =
                "Recovery is still resuming. Keep editing and wait before trying to quit again"
                    .into();
            return;
        }
        if self.save_all_busy() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.error = Some(
                "Cancel remaining Save All writes and wait for the in-flight save before closing"
                    .into(),
            );
            return;
        }
        if self.state == ConnectionState::Disconnecting {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.notice = disconnect::WAITING.into();
            return;
        }
        if !self.guard_run_transition(run_ui::Transition::Close) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            return;
        }
        self.dismiss_disk_review();
        if (self.language.running || self.language.startup_active()) && self.ready() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.close_after_language_stop = true;
            self.close_snapshot = Some(self.draft_versions());
            self.recovery.language_close_guard = Some(self.recovery_close_guard());
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
        if self.save_close_tab_busy() || self.save_close_write_pending() {
            self.cancel_tab_close();
            self.error = Some("Finish the current tab close or choose Keep editing and wait for its save before closing another tab".into());
            return;
        }
        if self.save_all_busy() {
            self.error = Some("Cancel remaining Save All writes and wait for the in-flight save before closing a tab".into());
            return;
        }
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
            self.capture_tab_close(id);
        } else {
            self.remove_tab(id);
        }
    }
    fn remove_tab(&mut self, id: u64) {
        self.location_history.close(id);
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
        if self.file_structure_shortcuts(ctx) {
            return;
        }
        if self.navigation_shortcuts(ctx) {
            return;
        }
        self.history_shortcuts(ctx);
        self.language_shortcuts(ctx);
        self.workspace_access_shortcuts(ctx);
        self.explorer_tree_shortcuts(ctx);
        self.save_all_shortcut(ctx);
        let save_current = ctx.input_mut(|input| {
            let mut pressed = false;
            input.events.retain(|event| {
                if let egui::Event::Key {
                    key: egui::Key::S,
                    pressed: down,
                    repeat,
                    modifiers,
                    ..
                } = event
                {
                    if !modifiers.alt
                        && !modifiers.shift
                        && !(modifiers.ctrl && modifiers.mac_cmd)
                        && modifiers.matches_exact(egui::Modifiers::COMMAND)
                    {
                        pressed |= *down && !*repeat;
                        return false;
                    }
                }
                true
            });
            pressed
        });
        if save_current {
            self.save();
        }
        self.file_find_shortcuts(ctx);
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::W)) {
            if let Some(id) = self.active_document {
                self.close_tab_requested = Some(id);
            }
        }
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            if self.close_header_menu(ctx) {
                return;
            }
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

    fn close_header_menu(&self, ctx: &egui::Context) -> bool {
        if ctx != &self.editor_ctx {
            return false;
        }
        if let Some(menus) =
            ctx.data(|data| data.get_temp::<IslandMenus>(egui::Id::new("cedar_island_menus")))
        {
            if menus.viewport == ctx.viewport_id()
                && menus.pass.checked_add(1) == Some(ctx.cumulative_pass_nr())
            {
                for (id, root) in menus.roots {
                    let mut bar = egui::menu::BarState::load(ctx, id);
                    if bar.as_ref().is_some_and(|menu| menu.id == root) {
                        bar.take();
                        bar.store(ctx, id);
                        return true;
                    }
                }
            }
        }
        let Some(header) = ctx.data(|data| data.get_temp::<HeaderMenus>(HeaderMenus::key())) else {
            return false;
        };
        if header.viewport != ctx.viewport_id()
            || header.pass.checked_add(1) != Some(ctx.cumulative_pass_nr())
        {
            return false;
        }
        let mut bar = egui::menu::BarState::load(ctx, header.bar);
        if !bar
            .as_ref()
            .is_some_and(|root| header.roots.contains(&root.id))
        {
            return false;
        }
        bar.take();
        bar.store(ctx, header.bar);
        true
    }

    fn header(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("header")
            .exact_height(44.0)
            .frame(
                egui::Frame::new()
                    .fill(chrome::FRAME)
                    .inner_margin(egui::Margin::symmetric(8, 5)),
            )
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(6.0, 0.0);
                ui.horizontal(|ui| {
                    logo(ui, 22.0);
                    let name = self
                        .root
                        .rsplit(['/', '\\'])
                        .find(|part| !part.is_empty())
                        .unwrap_or("Workspace");
                    let title = egui::WidgetText::from(RichText::new(name).strong()).into_galley(
                        ui,
                        Some(egui::TextWrapMode::Truncate),
                        (ui.available_width() - 240.0).clamp(40.0, 220.0),
                        egui::TextStyle::Button,
                    );
                    let workspace =
                        chrome::menu_button(ui, title, |ui| self.workspace_menu(ui, ctx)).response;
                    let workspace_id = workspace.id;
                    #[cfg(test)]
                    workspace_access_tests::record(ui, "workspace_menu", &workspace);
                    workspace.on_hover_text(if self.root.is_empty() {
                        "Workspace connection, recovery and save actions"
                    } else {
                        &self.root
                    });
                    ui.add_space(6.0);
                    self.history_controls(ui);
                    let tools = chrome::menu_button(ui, "Tools", |ui| self.tools_menu(ui)).response;
                    #[cfg(test)]
                    workspace_access_tests::record(ui, "tools_menu", &tools);
                    let header = HeaderMenus {
                        pass: ctx.cumulative_pass_nr(),
                        viewport: ctx.viewport_id(),
                        bar: ui.id(),
                        roots: [workspace_id, tools.id],
                    };
                    ctx.data_mut(|data| data.insert_temp(HeaderMenus::key(), header));
                    tools.on_hover_text("File Structure, Git, commands, language and test results");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let types = self.java_type_entry(ui);
                        #[cfg(test)]
                        workspace_access_tests::record(ui, "java_type_entry", &types);
                        #[cfg(not(test))]
                        let _ = types;
                        let text = ui
                            .add_enabled(
                                self.workspace_key.is_some(),
                                chrome::IconButton::new(chrome::Icon::SearchText, "Project Text"),
                            )
                            .on_hover_text("Search project text · Ctrl/Cmd+Shift+F");
                        #[cfg(test)]
                        workspace_access_tests::record(ui, "project_text_entry", &text);
                        if text.clicked() {
                            self.show_browse(browse::Provider::ProjectText);
                        }
                        let files = ui
                            .add_enabled(
                                self.workspace_key.is_some(),
                                chrome::IconButton::new(chrome::Icon::Search, "Files"),
                            )
                            .on_hover_text("Find project files · Ctrl/Cmd+Shift+N");
                        #[cfg(test)]
                        workspace_access_tests::record(ui, "browse_entry", &files);
                        if files.clicked() {
                            self.show_browse(browse::Provider::Files);
                        }
                    });
                });
            });
    }

    fn tool_rail(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("tool_window_rail")
            .exact_width(44.0)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(chrome::FRAME)
                    .inner_margin(egui::Margin::symmetric(4, 0)),
            )
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                let project = ui
                    .add_enabled(
                        self.workspace_key.is_some(),
                        chrome::IconButton::new(chrome::Icon::Project, "Project").rail(true),
                    )
                    .on_hover_text("Focus Project · Alt+1 or Ctrl/Cmd+Shift+E");
                #[cfg(test)]
                workspace_access_tests::record(ui, "project_rail", &project);
                if project.clicked() {
                    self.show_project(ctx, &project);
                }
                #[cfg(test)]
                browsing_shell_tests::record_find_diagnostic(
                    self,
                    ctx,
                    "before-header-response",
                    None,
                );
                let results = ui
                    .add(
                        chrome::IconButton::new(chrome::Icon::Search, "Find Results")
                            .rail(self.tools_open && self.tool == Tool::Search),
                    )
                    .on_hover_text("Find Results · return to retained results or the search query");
                #[cfg(test)]
                {
                    workspace_access_tests::record(ui, "find_results_entry", &results);
                    browsing_shell_tests::record_find_diagnostic(
                        self,
                        ctx,
                        "after-header-response",
                        Some(&results),
                    );
                }
                if results.clicked() {
                    self.show_find_results(ctx, &results);
                }
                let git = ui
                    .add_enabled(
                        self.workspace_key.is_some(),
                        chrome::IconButton::new(chrome::Icon::Git, "Git Changes")
                            .rail(self.tools_open && self.tool == Tool::Git),
                    )
                    .on_hover_text("Git Changes");
                if git.clicked() {
                    self.show_git_tool(ctx, &git);
                }
            });
    }

    fn workspace_menu(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.set_max_width(360.0);
        egui::ScrollArea::vertical()
          .id_salt("workspace_actions")
          .max_height((ctx.screen_rect().height() - 96.0).max(80.0))
          .drag_to_scroll(false)
          .show(ui, |ui| {
        if !self.root.is_empty() {
            ui.add(egui::Label::new(RichText::new(&self.root).small().color(MUTED)).truncate()).on_hover_text(&self.root);
        }
        let open = ui.button("Open workspace…");
        #[cfg(test)]
        workspace_access_tests::record(ui, "workspace_open", &open);
        if open.gained_focus() { open.scroll_to_me(None); }
        if open.clicked() { self.open_form = true; ui.close_menu(); }
        let reconnect = ui.add_enabled(self.active_form.is_some() && self.state != ConnectionState::Connecting && self.connection_admission_problem().is_none() && !self.mutation_pending(), egui::Button::new("Reconnect"));
        #[cfg(test)]
        workspace_access_tests::record(ui, "workspace_reconnect", &reconnect);
        if reconnect.gained_focus() { reconnect.scroll_to_me(None); }
        if reconnect.clicked() {
            if let Some(form) = self.active_form.clone() { self.connect(ctx, form); }
            ui.close_menu();
        }
        let recovery = ui.button("Recovery…");
        #[cfg(test)]
        workspace_access_tests::record(ui, "workspace_recovery", &recovery);
        if recovery.gained_focus() { recovery.scroll_to_me(None); }
        if recovery.clicked() { self.recovery.visible = true; ui.close_menu(); }
        ui.separator();
        let can_save = self.backend_supports("write") && !self.save_all_busy() && !self.interrupted_save_check.busy()
            && self.active().is_some_and(|doc| doc.dirty() && !doc.saving && !doc.save_outcome_unknown());
        let save = ui.add_enabled(can_save, egui::Button::new("Save file · Ctrl/Cmd+S"));
        #[cfg(test)]
        workspace_access_tests::record(ui, "workspace_save", &save);
        if save.gained_focus() { save.scroll_to_me(None); }
        if save.clicked() { self.save(); ui.close_menu(); }
        let save_all = ui.add_enabled(!self.save_all_busy() && self.backend_supports("write"), egui::Button::new("Save all editor buffers · Ctrl/Cmd+Shift+S"));
        #[cfg(test)]
        workspace_access_tests::record(ui, "workspace_save_all", &save_all);
        if save_all.gained_focus() { save_all.scroll_to_me(None); }
        if save_all.clicked() { self.queue_save_all(); ui.close_menu(); }
        let cancel = ui.add_enabled(self.save_all_busy(), egui::Button::new("Cancel remaining saves"));
        #[cfg(test)]
        workspace_access_tests::record(ui, "workspace_cancel_saves", &cancel);
        if cancel.gained_focus() { cancel.scroll_to_me(None); }
        if cancel.clicked() { self.cancel_save_all(); ui.close_menu(); }
        let copy = ui.add_enabled(self.active().is_some() && !self.copy_draft.is_open(), egui::Button::new("Copy to new draft…"));
        #[cfg(test)]
        workspace_access_tests::record(ui, "workspace_copy_draft", &copy);
        if copy.gained_focus() { copy.scroll_to_me(None); }
        if copy.clicked() { self.begin_copy_draft(); ui.close_menu(); }
        if let Some(message) = self.save_all_message() { ui.label(message); }
        ui.label(RichText::new("Saves editor buffers sequentially. Earlier saves remain on disk if a later save stops. Profile form changes are saved separately.").small().color(MUTED));
        ui.separator();
        let loaded = ui.add_enabled(self.ready(), egui::Button::new("Open and loaded files · Ctrl/Cmd+P"));
        #[cfg(test)]
        workspace_access_tests::record(ui, "workspace_loaded_files", &loaded);
        if loaded.gained_focus() { loaded.scroll_to_me(None); }
        if loaded.clicked() { self.show_file_chooser(); ui.close_menu(); }
        let recent = ui.add_enabled(self.ready(), egui::Button::new("Recent files · Ctrl/Cmd+E"));
        #[cfg(test)]
        workspace_access_tests::record(ui, "workspace_recent_files", &recent);
        if recent.clicked() { self.show_recent_files(); ui.close_menu(); }
        });
    }

    fn tools_menu(&mut self, ui: &mut egui::Ui) {
        self.file_structure_menu(ui);
        ui.separator();
        for (tool, label) in [
            (Tool::Git, "Git Changes"),
            (Tool::Run, "Commands"),
            (Tool::Language, "Language"),
            (Tool::Tests, "Test Results"),
        ] {
            let response = ui.selectable_label(self.tools_open && self.tool == tool, label);
            #[cfg(test)]
            workspace_access_tests::record(ui, &format!("tool_menu_{label}"), &response);
            if response.clicked() {
                self.tool = tool;
                self.tools_open = true;
                ui.close_menu();
            }
        }
    }

    fn status_controls(
        &mut self,
        ui: &mut egui::Ui,
        recovery_status: &str,
        protected: bool,
        compact: bool,
    ) {
        let (color, label) = match self.state {
            ConnectionState::Idle => (MUTED, "Not connected"),
            ConnectionState::Connecting => (AMBER, "Connecting"),
            ConnectionState::Ready => (
                if compact { GREEN } else { MUTED },
                if compact { "●" } else { "Connected" },
            ),
            ConnectionState::Disconnected => (RED, "Disconnected"),
            ConnectionState::Disconnecting => (AMBER, "Disconnecting"),
            ConnectionState::ClosingTransport => (AMBER, "Closing connection"),
            ConnectionState::CleanupUnverified => (AMBER, "Cleanup unverified"),
        };
        let recovery = ui.small_button(RichText::new(if compact { "Recovery" } else { recovery_status }).size(12.0).color(if protected { MUTED } else { AMBER }))
            .on_hover_text(format!("{recovery_status} · Private recovery on this computer. Click to review copies and settings"));
        #[cfg(test)]
        workspace_access_tests::record(ui, "recovery_status", &recovery);
        if recovery.clicked() {
            self.recovery.visible = true;
        }
        let connection =
            ui.colored_label(color, label)
                .on_hover_text(if compact { "Connected" } else { label });
        #[cfg(test)]
        workspace_access_tests::record(ui, "connection_status", &connection);
        #[cfg(not(test))]
        let _ = connection;
    }

    fn footer(&mut self, ctx: &egui::Context) {
        let style = ctx.style();
        let available_width = (ctx.available_rect().width() - 20.0).max(0.0);
        let workspace = self.recovery_workspace();
        let (recovery_status, protected) = self.recovery.status(workspace.as_ref(), self.active());
        let compact =
            protected && self.state == ConnectionState::Ready && !self.unverified_local_close;
        let meta_width = if self.active().is_some() {
            218.0_f32
        } else {
            0.0
        };
        let recovery_width = ctx.fonts(|fonts| {
            fonts
                .layout_no_wrap(
                    if compact { "Recovery" } else { recovery_status }.into(),
                    egui::FontId::proportional(12.0),
                    MUTED,
                )
                .size()
                .x
        }) + 2.0 * style.spacing.button_padding.x;
        let connection_width = if compact { 14.0 } else { 132.0 };
        let controls_width = recovery_width + connection_width + 2.0 * style.spacing.item_spacing.x;
        let pending = (!self.pending.is_empty()).then(|| format!("{} pending", self.pending.len()));
        let pending_width = pending.as_ref().map_or(0.0, |label| {
            ctx.fonts(|fonts| {
                fonts
                    .layout_no_wrap(label.clone(), egui::FontId::proportional(12.0), MUTED)
                    .size()
                    .x
            }) + 12.0
                + 2.0 * style.spacing.item_spacing.x
        });
        // Reserve critical controls before the elided path. Only a genuinely
        // constrained width wraps status; Idle alone never adds another row.
        let controls_fit = meta_width + pending_width + controls_width + 8.0 <= available_width;
        let right_width =
            meta_width + pending_width + if controls_fit { controls_width } else { 0.0 };
        let left_width = (available_width - right_width - style.spacing.item_spacing.x).max(0.0);
        let show_notice = !self.notice.is_empty()
            && self.notice != "Workspace connected"
            && self.notice != "Ready when you are"
            && !self.notice.starts_with("Saved ")
            && !self.notice.starts_with("CJK fallback:");
        let saves_unavailable =
            self.state == ConnectionState::Ready && !self.backend_supports("write");
        let extra_risk = self.unverified_local_close || saves_unavailable;
        let risk_width = ctx.fonts(|fonts| {
            let font = egui::TextStyle::Body.resolve(&style);
            [
                (self.unverified_local_close, "Prior cleanup unverified"),
                (saves_unavailable, "Saves unavailable"),
            ]
            .into_iter()
            .filter(|(show, _)| *show)
            .map(|(_, label)| {
                fonts
                    .layout_no_wrap(label.into(), font.clone(), AMBER)
                    .size()
                    .x
                    + style.spacing.item_spacing.x
            })
            .sum::<f32>()
        });
        let risk_fits = risk_width <= available_width;
        let (body_height, notice_height) = ctx.fonts(|fonts| {
            let notice_height = if show_notice {
                let mut job = egui::text::LayoutJob::simple(
                    self.notice.clone(),
                    egui::TextStyle::Small.resolve(&style),
                    AMBER,
                    available_width,
                );
                job.wrap.max_rows = 1;
                fonts.layout_job(job).size().y
            } else {
                0.0
            };
            (
                fonts.row_height(&egui::TextStyle::Body.resolve(&style)),
                notice_height,
            )
        });
        let height = 8.0
            + body_height.max(20.0)
            + if extra_risk && risk_fits {
                style.spacing.item_spacing.y + body_height.max(style.spacing.interact_size.y)
            } else {
                0.0
            }
            + if show_notice {
                style.spacing.item_spacing.y + notice_height
            } else {
                0.0
            };
        let panel = egui::TopBottomPanel::bottom("status");
        // Bounded rows, including newly arrived risk labels, have a known height
        // before painting. Using the last frame's automatic height can clip a
        // new row below the window and leaves a gap when it clears.
        let panel = if controls_fit && risk_fits {
            panel.exact_height(height)
        } else {
            // Dedicated risk controls retain their full, wrapping presentation.
            panel.min_height(height)
        };
        panel
            .frame(
                egui::Frame::new()
                    .fill(chrome::FRAME)
                    .inner_margin(egui::Margin::symmetric(10, 4)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if left_width > 0.0 {
                        let path =
                            self.active()
                                .map(|doc| doc.path.as_str())
                                .unwrap_or_else(|| {
                                    self.root
                                        .rsplit(['/', '\\'])
                                        .find(|part| !part.is_empty())
                                        .unwrap_or("Cedar")
                                });
                        let status_path = ui
                            .allocate_ui_with_layout(
                                egui::vec2(left_width, 20.0),
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    // Reserve the whole path lane without centering the
                                    // label's galley (add_sized centers the widget itself).
                                    ui.set_min_width(left_width);
                                    ui.add(
                                        egui::Label::new(RichText::new(path).color(MUTED))
                                            .truncate(),
                                    )
                                },
                            )
                            .inner
                            .on_hover_text(format!(
                                "{}\n{}\n{}\n{}",
                                self.root,
                                self.agent_status(),
                                self.agent_details(),
                                self.notice
                            ));
                        #[cfg(test)]
                        workspace_access_tests::record(ui, "status_path", &status_path);
                        #[cfg(not(test))]
                        let _ = status_path;
                    }
                    ui.allocate_ui_with_layout(
                        egui::vec2(right_width, 20.0),
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            ui.set_min_width(right_width);
                            if controls_fit {
                                self.status_controls(ui, recovery_status, protected, compact);
                            }
                            if let Some(label) = &pending {
                                ui.label(RichText::new(label).size(12.0).color(MUTED));
                                ui.add(egui::Spinner::new().size(12.0));
                            }
                            if let Some(doc) = self.active() {
                                ui.label(RichText::new("UTF-8").size(12.0).color(MUTED));
                                ui.label(
                                    RichText::new(language(&doc.path)).size(12.0).color(MUTED),
                                );
                                ui.label(
                                    RichText::new(format!(
                                        "Ln {}, Col {}",
                                        doc.cursor.0, doc.cursor.1
                                    ))
                                    .size(12.0)
                                    .color(MUTED),
                                );
                            }
                        },
                    );
                });
                if !controls_fit {
                    ui.horizontal_wrapped(|ui| {
                        self.status_controls(ui, recovery_status, protected, compact)
                    });
                }
                if self.unverified_local_close
                    || (self.state == ConnectionState::Ready && !self.backend_supports("write"))
                {
                    ui.horizontal_wrapped(|ui| {
                        if self.unverified_local_close {
                            ui.colored_label(AMBER, "Prior cleanup unverified")
                                .on_hover_text(disconnect::UNVERIFIED);
                        }
                        if self.state == ConnectionState::Ready && !self.backend_supports("write") {
                            ui.colored_label(AMBER, "Saves unavailable")
                                .on_hover_text(self.agent_details());
                        }
                    });
                }
                if show_notice {
                    // Reports can contain a complete long search scope. Keep the
                    // inherited bounded status lane; the full text remains on hover.
                    ui.add(
                        egui::Label::new(RichText::new(&self.notice).small().color(AMBER))
                            .truncate(),
                    )
                    .on_hover_text(&self.notice);
                }
            });
    }

    fn notifications(&mut self, ctx: &egui::Context) {
        if let Some(doc) = self
            .documents
            .iter()
            .find(|doc| Some(doc.id) != self.active_document && doc.save_outcome_unknown())
        {
            egui::TopBottomPanel::top("inactive_unknown_save")
                .frame(egui::Frame::new().fill(PANEL).inner_margin(10.0))
                .show(ctx, |ui| {
                    ui.colored_label(AMBER, format!(
                        "Save outcome unknown: {}. This tab still needs review; copying a draft does not resolve an unknown save. Save All remains blocked.",
                        doc.path
                    ));
                });
        }
        if let Some(error) = self.error.clone() {
            // TopBottomPanel otherwise clips its first paint to a one-row
            // default before learning the wrapped label's actual height.
            // Measure the existing horizontal-wrapped label and small Dismiss
            // button without allocating another UI or changing their IDs.
            let style = ctx.style();
            let available_width = (ctx.available_rect().width() - 20.0).max(1.0);
            let height = ctx.fonts(|fonts| {
                let mut job = egui::WidgetText::from(error.clone()).into_layout_job(
                    &style,
                    egui::FontSelection::Default,
                    egui::Align::Center,
                );
                job.wrap.max_width = available_width;
                job.first_row_min_height = style.spacing.interact_size.y;
                let label = fonts.layout_job(job);
                let dismiss = fonts.layout_no_wrap(
                    "Dismiss".into(),
                    egui::TextStyle::Body.resolve(&style),
                    TEXT,
                );
                let button_width = dismiss.size().x
                    + if style.visuals.button_frame {
                        2.0 * style.spacing.button_padding.x
                    } else {
                        0.0
                    };
                let content_height = label.rows.last().map_or(label.size().y, |last| {
                    if last.rect.right() + style.spacing.item_spacing.x + button_width
                        > available_width
                    {
                        label.size().y
                            + style.spacing.item_spacing.y
                            + dismiss.size().y.max(last.rect.height())
                    } else {
                        label.size().y.max(last.rect.top() + dismiss.size().y)
                    }
                });
                (20.0 + content_height.max(style.spacing.interact_size.y)).ceil()
            });
            egui::TopBottomPanel::top("error")
                .min_height(height)
                .frame(
                    egui::Frame::new()
                        .fill(Color32::from_rgb(55, 32, 39))
                        .inner_margin(10.0),
                )
                .show(ctx, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        let message = ui.colored_label(RED, error);
                        #[cfg(test)]
                        workspace_access_tests::record(ui, "error_status", &message);
                        #[cfg(not(test))]
                        let _ = message;
                        let dismiss = ui.small_button("Dismiss");
                        #[cfg(test)]
                        workspace_access_tests::record(ui, "error_dismiss", &dismiss);
                        if dismiss.clicked() {
                            self.error = None;
                        }
                    });
                });
        }
    }

    fn project_context_label(ui: &mut egui::Ui, text: &str, ready: bool) -> egui::Response {
        let label = egui::Label::new(RichText::new(text).color(MUTED))
            .truncate()
            .selectable(false)
            .sense(egui::Sense::hover());
        if !ready {
            return ui.add(label);
        }
        // egui 0.31 adds an automatic tooltip for elided Labels. Keep its
        // native layout, widget info and non-selectable paint path, but let
        // this context's single workspace/scope tooltip supply the full text.
        let (galley_pos, galley, response) = label.layout_in_ui(ui);
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), galley.text())
        });
        if ui.is_rect_visible(response.rect) {
            let response_color = ui.style().visuals.text_color();
            let underline = if response.has_focus() || response.highlighted() {
                Stroke::new(1.0_f32, response_color)
            } else {
                Stroke::NONE
            };
            ui.painter().add(
                egui::epaint::TextShape::new(galley_pos, galley, response_color)
                    .with_underline(underline),
            );
        }
        response
    }

    fn sidebar(&mut self, ctx: &egui::Context) {
        // egui 0.31's panel minimum subtracts inner, but not outer, margins.
        // Compensate the 4pt island gap so the actual minimum remains 180pt.
        egui::SidePanel::left("explorer").default_width(246.0).width_range((180.0 - f32::from(chrome::GAP))..=460.0).resizable(true)
            .frame(chrome::island()).show(ctx, |ui| {
                let size_id = ui.make_persistent_id("sidebar_viewport_size");
                let size = ui.available_size();
                let resized = ctx.data_mut(|data| {
                    let previous = data.get_temp::<egui::Vec2>(size_id);
                    data.insert_temp(size_id, size);
                    previous.is_some_and(|previous| previous != size)
                });
                // Solid gutters appear only when needed. Content and reveal targets
                // use the actual viewport, including any in-progress gutter animation.
                ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
                #[cfg(test)]
                sidebar_layout_tests::begin(ctx);
                // A held egui scrollbar drag cannot remap a zero-length track.
                // Keep its stored state intact until positive space returns.
                if size.y <= 0.0 { return; }
                let reveal = |response: &egui::Response| {
                    #[cfg(test)]
                    sidebar_layout_tests::record_reveal(response, false, resized);
                    let requested = response.gained_focus() || (resized && response.has_focus());
                    if requested {
                        response.scroll_to_me(None);
                    }
                    requested
                };
                let outer = egui::ScrollArea::vertical().id_salt("sidebar_scroll")
                    // Background drag surfaces use the previous viewport rect;
                    // changing tool/mode height can cover newly positioned rows.
                    // Wheel, touchpad and scrollbar scrolling remain available.
                    .drag_to_scroll(false)
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
                    .min_scrolled_height(0.0).auto_shrink([false, false]).animated(false)
                    .show_viewport(ui, |ui, viewport| {
                        // The paint clip includes a margin outside the real viewport.
                        // Capture the viewport origin before content changes min_rect.
                        let actual_viewport = viewport.translate(ui.min_rect().min.to_vec2());
                        #[cfg(test)]
                        sidebar_layout_tests::record_scroll_start(ctx, true, viewport.min.to_vec2());
                        // Bound the content before creating the horizontal child: its
                        // initial minimum width otherwise retains the rounded-up edge.
                        let content_right = (actual_viewport.right() * 8.0).floor() / 8.0;
                        ui.set_max_width((content_right - ui.max_rect().left()).max(0.0));
                        let mut toolbar_reveal = None;
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            ui.spacing_mut().interact_size.y = chrome::HEADER_HEIGHT;
                            let heading_text = egui::WidgetText::from(RichText::new("Project ⌄").strong()).into_galley(ui, Some(egui::TextWrapMode::Truncate), (ui.available_width() - 108.0 - 2.0 * ui.spacing().button_padding.x).max(20.0), egui::TextStyle::Button);
                            let heading = chrome::menu_button(ui, heading_text, |ui| {
                                let scope = self.explorer_scope();
                                ui.add(egui::Label::new(format!("Scope: /{scope}")).truncate()).on_hover_text(&scope);
                                let up = ui.add_enabled(self.ready() && !self.explorer_busy() && self.explorer.mode == explorer_tree::Mode::Flat && !self.directory.is_empty(), egui::Button::new("Up"));
                                #[cfg(test)]
                                sidebar_layout_tests::record(ui, "Up", &up);
                                if up.clicked() { self.list(parent_path(&self.directory)); ui.close_menu(); }
                                ui.separator();
                                for (mode, label) in [(explorer_tree::Mode::Flat, "Flat"), (explorer_tree::Mode::Tree, "Tree")] {
                                    let response = ui.selectable_label(self.explorer.mode == mode, label);
                                    #[cfg(test)]
                                    {
                                        workspace_access_tests::record(ui, if mode == explorer_tree::Mode::Flat { "explorer_flat_mode" } else { "explorer_tree_mode" }, &response);
                                        sidebar_layout_tests::record(ui, label, &response);
                                    }
                                    if response.clicked() { self.explorer_set_mode(mode); ui.close_menu(); }
                                }
                                ui.separator();
                                let help = ui.add(egui::Label::new("Ctrl/Cmd+P  Choose a file\nCtrl/Cmd+G  Go to line\nCtrl/Cmd+F  Find in file\nCtrl/Cmd+S  Save changes\nAlt+1  Focus Project").sense(egui::Sense::focusable_noninteractive()));
                                #[cfg(test)]
                                {
                                    sidebar_layout_tests::record(ui, "help", &help);
                                    workspace_access_tests::record(ui, "project_shortcuts", &help);
                                }
                                #[cfg(not(test))]
                                let _ = help;
                            }).response;
                            register_island_menu(ui, heading.id);
                            self.record_project_access_fallback(ui, &heading);
                            if reveal(&heading) { toolbar_reveal = Some(heading.rect); }
                            #[cfg(test)]
                            workspace_access_tests::record(ui, "project_heading", &heading);
                            heading.on_hover_text("Project view, directory scope and keyboard shortcuts");
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                // Four independent, always allocated targets. Cancel never takes Reveal's slot.
                                ui.push_id("project_reveal_actions", |ui| {
                                    let cancel = ui.push_id("cancel", |ui| ui.add_enabled(self.project_reveal_pending(), chrome::IconButton::new(chrome::Icon::Close, "Cancel selecting current file"))).inner.on_hover_text("Cancel selecting current file");
                                    if reveal(&cancel) { toolbar_reveal = Some(cancel.rect); }
                                    #[cfg(test)]
                                    { workspace_access_tests::record(ui, "project_reveal_cancel", &cancel); sidebar_layout_tests::record(ui, "Cancel selecting current file", &cancel); }
                                    if cancel.clicked() { self.cancel_project_reveal(); }
                                    let select = ui.push_id("select", |ui| ui.add_enabled(self.ready() && self.active().is_some(), chrome::IconButton::new(chrome::Icon::Target, "Select Current File in Project"))).inner.on_hover_text("Select Current File in Project · load only its known parent folders");
                                    if reveal(&select) { toolbar_reveal = Some(select.rect); }
                                    #[cfg(test)]
                                    { workspace_access_tests::record(ui, "project_select_current", &select); sidebar_layout_tests::record(ui, "Select Current File in Project", &select); }
                                    self.select_current_file_in_project(ctx, &select);
                                });
                                let refresh = ui.push_id("refresh", |ui| ui.add_enabled(self.ready() && !self.explorer_busy(), chrome::IconButton::new(chrome::Icon::Refresh, "Refresh directory"))).inner.on_hover_text("Refresh directory · Alt+1 focuses Project without refreshing");
                                if reveal(&refresh) { toolbar_reveal = Some(refresh.rect); }
                                #[cfg(test)]
                                { workspace_access_tests::record(ui, "explorer_refresh", &refresh); sidebar_layout_tests::record(ui, "R", &refresh); }
                                if refresh.clicked() { self.explorer_refresh(&self.explorer_scope()); }
                                let new_file = ui.push_id("new_file", |ui| ui.add_enabled(self.ready(), chrome::IconButton::new(chrome::Icon::Add, "New file"))).inner.on_hover_text("New UTF-8 file");
                                if reveal(&new_file) { toolbar_reveal = Some(new_file.rect); }
                                #[cfg(test)]
                                sidebar_layout_tests::record(ui, "+", &new_file);
                                #[cfg(test)]
                                workspace_access_tests::record(ui, "project_new_file", &new_file);
                                if new_file.clicked() { self.explorer_new_file(); }
                            });
                        });
                        if let Some(row) = toolbar_reveal {
                            if let Some(target) = sidebar_reveal::footer_target(row, actual_viewport, ui.min_rect().top(), viewport.min.y, ui.spacing().item_spacing.y) {
                                ui.scroll_to_rect(target, Some(egui::Align::TOP));
                            }
                        }
                        // Project's directory scope is independent of the active editor.
                        // Keep its mode and exact scope visible without a navigation action.
                        ui.push_id("project_context", |ui| {
                            let mode = match self.explorer.mode {
                                explorer_tree::Mode::Flat => "Flat",
                                explorer_tree::Mode::Tree => "Tree",
                            };
                            let scope = self.explorer_scope();
                            let context = match self.state {
                                ConnectionState::Ready => format!("Scope: /{scope}"),
                                ConnectionState::Connecting => "Connecting…".into(),
                                ConnectionState::Disconnecting => "Disconnecting…".into(),
                                ConnectionState::ClosingTransport => "Waiting for local cleanup".into(),
                                ConnectionState::CleanupUnverified => "Connection cleanup unverified".into(),
                                ConnectionState::Idle | ConnectionState::Disconnected => "Not connected".into(),
                            };
                            let text = format!("{mode} · {context}");
                            let response = Self::project_context_label(ui, &text, self.ready());
                            #[cfg(test)]
                            project_context_tests::record(ui, &response, actual_viewport, &text);
                            if self.ready() {
                                response.on_hover_text(format!("Workspace: {}\nProject directory scope: /{}", self.root, scope));
                            }
                        });
                        self.project_name_ui(ui);
                        // Later status and tree requests retain native last-writer priority.
                        ui.push_id("explorer_status", |ui| self.explorer_status(ui, resized));
                        ui.add_space(4.0);
                        let footer_height = 0.0;
                        let file_text_width = (ui.available_width() - ui.spacing().scroll.allocated_width() - 2.0 * ui.spacing().button_padding.x).max(0.0);
                        let tree_rows = if self.explorer.mode == explorer_tree::Mode::Tree { self.explorer_visible_rows() } else { Vec::new() };
                        let display_entries = if self.explorer.mode == explorer_tree::Mode::Tree { tree_rows.iter().map(|row| row.entry.clone()).collect() } else { self.entries.clone() };
                        let file_galleys: Vec<_> = display_entries.iter().enumerate().map(|(index, entry)| {
                            let row = tree_rows.get(index);
                            let state = row.filter(|row| row.entry.is_dir).map_or("", |row| {
                                if row.loading { " · loading" } else if row.stale { " · stale" }
                                else if row.error.is_some() { " · error" } else { "" }
                            });
                            let indent = row.map_or(0.0, |row| (row.depth as f32 * chrome::INDENT).min(file_text_width * 0.35));
                            let name = if entry.path.is_empty() && entry.is_dir { self.root.rsplit(['/', '\\']).find(|part| !part.is_empty()).unwrap_or("Workspace") } else { &entry.name };
                            self.project_name_galley(ui, entry, name, state, (file_text_width - indent - 38.0).max(0.0))
                        }).collect();
                        let explorer_min_height = file_galleys.iter().fold(64.0_f32, |height, galley| {
                            height.max(galley.size().y + 2.0 * ui.spacing().button_padding.y + 2.0 * ui.spacing().item_spacing.y).max(ui.spacing().interact_size.y)
                        });
                        let explorer_height = (ui.available_height() - footer_height).max(explorer_min_height);
                        let pending_id = ui.make_persistent_id("explorer_focus_reveal");
                        let pending_focus = ctx.data_mut(|data| {
                            let pending = data.get_temp::<egui::Id>(pending_id);
                            data.remove::<egui::Id>(pending_id);
                            pending
                        });
                        let mut focused_entry = None;
                        let mut open = None;
                        let mut retry = None;
                        let explorer = egui::ScrollArea::vertical().id_salt("explorer_scroll").drag_to_scroll(false)
                            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
                            .min_scrolled_height(explorer_min_height).max_height(explorer_height)
                            .auto_shrink([false, false]).animated(false).show_viewport(ui, |ui, viewport| {
                                let actual_viewport = viewport.translate(ui.min_rect().min.to_vec2());
                                #[cfg(test)]
                                sidebar_layout_tests::record_scroll_start(ctx, false, viewport.min.to_vec2());
                                ui.spacing_mut().item_spacing.y = 0.0;
                                if self.explorer.mode == explorer_tree::Mode::Flat && self.entries.is_empty() { ui.label(RichText::new(if !self.ready() { "Connect a workspace to browse files" } else if self.explorer_scope_loaded() { "This directory is empty" } else { "Directory not loaded. Refresh explicitly to load it" }).small().color(MUTED)); }
                                for (index, (entry, galley)) in display_entries.iter().zip(&file_galleys).enumerate() {
                                    let selected = self.active().is_some_and(|doc| doc.path == entry.path);
                                    let response = if let Some(row) = tree_rows.get(index) {
                                        self.explorer_tree_row(ui, row, galley.clone())
                                    } else {
                                        self.explorer_flat_row(ui, entry, galley.clone(), selected)
                                    };
                                    let explicit_key_reveal = self.explorer.key_reveal.as_deref() == Some(entry.path.as_str()) && (response.has_focus() || self.project_name_selected(&entry.path));
                                    let access_reveal = self.record_project_access(ui, &entry.path, &response);
                                    let current_file_reveal = self.record_project_reveal_row(ui, &entry.path, &response);
                                    let newly_revealed = response.gained_focus() || (resized && response.has_focus()) || explicit_key_reveal || access_reveal || current_file_reveal;
                                    if newly_revealed || (pending_focus == Some(response.id) && response.has_focus()) {
                                        response.scroll_to_me(None);
                                        // Correct this request before any later row or label can
                                        // issue its own native request. The stack grows downward.
                                        if let Some(target) = sidebar_reveal::footer_target(response.rect, actual_viewport, ui.min_rect().top(), viewport.min.y, ui.spacing().item_spacing.y) {
                                            ui.scroll_to_rect(target, Some(egui::Align::TOP));
                                        }
                                        focused_entry = Some((response.id, newly_revealed, response.rect, viewport.min.y));
                                    }
                                    #[cfg(test)]
                                    sidebar_layout_tests::record(ui, &entry.name, &response);
                                    #[cfg(test)]
                                    workspace_access_tests::record(ui, &entry.name, &response);
                                    if let Some(row) = tree_rows.get(index) {
                                        if self.explorer_pointer_activation(ctx, &response) {
                                            response.request_focus();
                                            open = Some(entry.clone());
                                        }
                                        if let Some(error) = &row.error {
                                            ui.label(RichText::new(error).small().color(RED));
                                            let retry_response = explorer_tree::retry_button(ui, "branch", &entry.path, "Retry", self.ready() && !self.explorer_busy());
                                            let newly_revealed = retry_response.gained_focus() || (resized && retry_response.has_focus());
                                            if newly_revealed || (pending_focus == Some(retry_response.id) && retry_response.has_focus()) {
                                                retry_response.scroll_to_me(None);
                                                if let Some(target) = sidebar_reveal::footer_target(retry_response.rect, actual_viewport, ui.min_rect().top(), viewport.min.y, ui.spacing().item_spacing.y) {
                                                    ui.scroll_to_rect(target, Some(egui::Align::TOP));
                                                }
                                                focused_entry = Some((retry_response.id, newly_revealed, retry_response.rect, viewport.min.y));
                                            }
                                            #[cfg(test)]
                                            workspace_access_tests::record(ui, &format!("tree_retry:{}", entry.path), &retry_response);
                                            if retry_response.clicked() { retry = Some(entry.path.clone()); }
                                        } else if row.expanded && row.loaded && !row.loading && tree_rows.get(index + 1).is_none_or(|child| child.depth <= row.depth) {
                                            ui.label(RichText::new("Empty directory").small().color(MUTED));
                                        }
                                    } else if self.explorer_flat_activation(&response) { open = Some(entry.clone()); }
                                }
                            });
                        if let Some((id, newly_revealed, row_rect, old_offset)) = focused_entry {
                            // The inner area consumes its own target. Reveal its viewport
                            // through the outer area afterwards, then correct the inner
                            // target once if its previous clip was hidden by the outer area.
                            if newly_revealed && !actual_viewport.contains_rect(explorer.inner_rect) {
                                ctx.data_mut(|data| data.insert_temp(pending_id, id));
                            }
                            let target = if explorer.inner_rect.height() <= size.y {
                                explorer.inner_rect
                            } else {
                                // A small remaining viewport can still reveal one row.
                                // Convert the rendered row from the old inner offset to
                                // the offset that will be used by the next frame.
                                row_rect.translate(egui::vec2(0.0, old_offset - explorer.state.offset.y))
                            };
                            ui.scroll_to_rect(target, None);
                            if let Some(target) = sidebar_reveal::footer_target(target, actual_viewport, ui.min_rect().top(), viewport.min.y, ui.spacing().item_spacing.y) {
                                ui.scroll_to_rect(target, Some(egui::Align::TOP));
                            }
                        }
                        #[cfg(test)]
                        sidebar_layout_tests::record_scroll(ctx, false, &explorer);
                        if let Some(path) = retry { self.explorer_refresh(&path); }
                        if let Some(entry) = open {
                            self.cancel_project_name_search();
                            if self.explorer.mode == explorer_tree::Mode::Tree { self.explorer_activate(&entry.path); }
                            else if entry.is_dir { self.list(entry.path); } else { self.open(entry.path, None); }
                        }
                    });
                #[cfg(test)]
                sidebar_layout_tests::record_scroll(ctx, true, &outer);
                #[cfg(not(test))]
                let _ = outer;
            });
    }

    fn java_types_tools_auto_height(&self, ctx: &egui::Context) -> Option<f32> {
        if self.tools_height.is_some() || !self.focused_java_types() {
            return None;
        }
        let available = ctx.available_rect();
        let frame = chrome::island();
        let margins = frame.total_margin().sum();
        // This UI only measures shared galleys: it never creates widgets,
        // reads input, or changes a search/focus owner.
        let mut measure = egui::Ui::new(
            ctx.clone(),
            egui::Id::new("java_types_measure"),
            egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2((available.width() - margins.x).max(0.0), available.height()),
            )),
        );
        let body_height = self.java_types_automatic_body_height(&measure);
        let gap = measure.spacing().item_spacing.y;
        measure.spacing_mut().button_padding = egui::vec2(8.0, 3.0);
        measure.spacing_mut().item_spacing = egui::vec2(8.0, 4.0);
        let row_height = if egui::TextStyle::Button.resolve(measure.style()).size > 15.0 {
            34.0
        } else {
            28.0
        };
        let title_width = egui::WidgetText::from(RichText::new("LANGUAGE").strong())
            .into_galley(
                &measure,
                Some(egui::TextWrapMode::Wrap),
                measure.available_width(),
                egui::TextStyle::Body,
            )
            .size()
            .x;
        let controls_width = shell_button_width(&measure, "x", true)
            + shell_button_width(&measure, "Automatic height", true)
            + 8.0;
        let header_height = if title_width + 8.0 + controls_width > measure.available_width() {
            2.0 * row_height + 4.0
        } else {
            row_height
        };
        // 160 points remain for the editor whenever both it and the normal
        // tools minimum fit. On a shorter screen, the available space wins.
        let cap = (available.height() - 160.0)
            .max(150.0)
            .min(available.height())
            .clamp(0.0, 500.0);
        Some(
            (margins.y + header_height + gap + 6.0 + gap + body_height)
                .ceil()
                .max(150.0)
                .min(cap),
        )
    }

    fn tools(&mut self, ctx: &egui::Context) {
        if !self.tools_open {
            return;
        }
        let mut close_tools = None;
        let mut automatic_height = false;
        let auto_height = self.java_types_tools_auto_height(ctx);
        let available = ctx.available_rect();
        #[cfg(test)]
        if self.focused_java_types() {
            ctx.data_mut(|data| {
                data.insert_temp(
                    egui::Id::new("java_types_tools_allocation"),
                    (available, auto_height),
                )
            });
        }
        // Compute native drag ownership before supplying an automatic display
        // allocation. This must not masquerade as a manual preference.
        let resize = tools_resize::prepare(ctx, self.tools_height, &self.tools_height_reset);
        if let Some(height) = auto_height {
            let mut rect = available;
            rect.min.y = rect.max.y - height;
            // Same PanelState mechanism as tools_resize's manual allocation;
            // TopBottomPanel still applies its own native edge interaction.
            ctx.data_mut(|data| {
                data.insert_persisted(
                    egui::Id::new("tools"),
                    egui::containers::panel::PanelState { rect },
                )
            });
        }
        let frame = chrome::island();
        let minimum = if self.focused_java_types() {
            (150.0 - f32::from(chrome::GAP))
                .min(available.height() - f32::from(chrome::GAP))
                .max(0.0)
        } else {
            150.0 - f32::from(chrome::GAP)
        };
        egui::TopBottomPanel::bottom("tools")
            .default_height(245.0)
            .height_range(minimum..=500.0)
            .resizable(true)
            .frame(frame)
            .show(ctx, |ui| {
                if let Some(height) = resize.reserve_and_capture(ui, &frame) {
                    self.tools_height = Some(height);
                }
                if auto_height.is_some() && self.tools_height.is_none() {
                    ui.set_min_height(ui.max_rect().height());
                }
                ui.scope(|ui| {
                    ui.spacing_mut().button_padding = egui::vec2(8.0, 3.0);
                    ui.spacing_mut().item_spacing = egui::vec2(8.0, 4.0);
                    ui.horizontal_wrapped(|ui| {
                        let font = egui::TextStyle::Button.resolve(ui.style()).size;
                        ui.set_row_height(if font > 15.0 { 34.0 } else { 28.0 });
                        let label = match self.tool {
                            Tool::Search => "FIND RESULTS",
                            Tool::Git => "GIT CHANGES",
                            Tool::Run => "COMMANDS",
                            Tool::Language => "LANGUAGE",
                            Tool::Tests => "TEST RESULTS",
                        };
                        let response = ui
                            .add(
                                egui::Label::new(RichText::new(label).strong())
                                    .sense(egui::Sense::focusable_noninteractive()),
                            )
                            .on_hover_text(
                                "Ctrl/Cmd+J hides tools or returns to the selected tool",
                            );
                        self.workspace_access
                            .record(workspace_access::Target::Tool(self.tool), &response);
                        #[cfg(test)]
                        workspace_access_tests::record(ui, label, &response);
                        // A nested non-wrapping row does not wrap with its parent.
                        // Match small Button sizing: Body text, horizontal frame
                        // padding, and an inside stroke that adds no layout width.
                        let controls_width = ["x", "Automatic height"]
                            .into_iter()
                            .map(|label| shell_button_width(ui, label, true))
                            .sum::<f32>()
                            + ui.spacing().item_spacing.x;
                        if controls_width > ui.available_size_before_wrap().x {
                            ui.end_row();
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let close = ui
                                .small_button("x")
                                .on_hover_text("Hide tools and return to editor · Ctrl/Cmd+J");
                            #[cfg(test)]
                            workspace_access_tests::record(ui, "close_tools", &close);
                            if close.clicked() {
                                close_tools = Some(close);
                            }
                            let reset = ui
                                .push_id("tools_automatic_height", |ui| {
                                    ui.add_enabled(
                                        self.tools_height.is_some(),
                                        egui::Button::new("Automatic height").small(),
                                    )
                                })
                                .inner
                                .on_hover_text("Let tool content determine the panel height");
                            #[cfg(test)]
                            workspace_access_tests::record(ui, "tools_automatic_height", &reset);
                            automatic_height |= reset.clicked();
                        });
                    });
                });
                ui.separator();
                match self.tool {
                    Tool::Language => self.language_panel(ui),
                    Tool::Search => self.search_panel(ui),
                    Tool::Git => self.git_panel(ui),
                    Tool::Run => self.run_panel(ui),
                    Tool::Tests => self.test_report_panel(ui),
                }
            });
        if automatic_height {
            self.tools_height = None;
            self.tools_height_reset
                .reset(ctx.input(|input| input.pointer.primary_down()));
            ctx.request_repaint();
        }
        if let Some(response) = close_tools {
            self.close_tools(ctx, &response);
        }
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
                    ui.label(RichText::new("Explicit port").small().color(MUTED));
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
            ui.label(RichText::new("The explicit port overrides an SSH alias's configured port. Enter literal remote paths: ~ and $variables are not expanded. Prefer an absolute agent path; a bare name uses the remote command environment.").small().color(MUTED));
        } else {
            #[cfg(target_os = "linux")]
            ui.label(RichText::new("Local folders use the matching cedar-agent beside this application. Missing or incompatible bundles cannot fall back to an embedded workspace.").small().color(MUTED));
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
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    self.state != ConnectionState::Connecting && self.connection_admission_problem().is_none() && !self.mutation_pending(),
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
            if self.ready() || self.state == ConnectionState::Disconnecting {
                let problem = self.disconnect_problem();
                let response = ui.add_enabled(problem.is_none(), egui::Button::new("Disconnect (keep drafts)"))
                    .on_hover_text("Release an idle connection. Stop active tools first. Read-only requests may be cancelled; drafts remain open.")
                    .on_disabled_hover_text(problem.unwrap_or(""));
                #[cfg(test)]
                crate::workspace_access_tests::record(ui, "Disconnect", &response);
                if response.clicked() { self.disconnect_idle(); }
            }
            if self.state == ConnectionState::Disconnecting {
                ui.spinner();
                ui.label("Waiting for local cleanup");
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
        // Keep overflow discoverable even when the pointer is elsewhere. Apply
        // this only to welcome; the rest of the shell keeps its own scroll style.
        let scroll_style = ui.spacing().scroll;
        ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
        #[cfg(test)]
        let viewport = ui.available_rect_before_wrap();
        let scroll_area = egui::ScrollArea::vertical().min_scrolled_height(0.0);
        let scroll = scroll_area.show(ui, |ui| {
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
                        ui.horizontal_wrapped(|ui| {
                            let label = if self.backend_file_discovery_supported() { "Find project files · Ctrl/Cmd+Shift+N" } else { "Open and loaded files · Ctrl/Cmd+Shift+N" };
                            let files = ui.button(label);
                            #[cfg(test)]
                            workspace_access_tests::record(ui, "welcome_files", &files);
                            if files.gained_focus() { files.scroll_to_me(None); }
                            if files.clicked() {
                                self.show_browse(browse::Provider::Files);
                            }
                            let text = ui.button("Search project text");
                            #[cfg(test)]
                            workspace_access_tests::record(ui, "welcome_text", &text);
                            if text.gained_focus() { text.scroll_to_me(None); }
                            if text.clicked() {
                                self.show_browse(browse::Provider::ProjectText);
                            }
                            let new_file = ui.button("New file");
                            #[cfg(test)]
                            workspace_access_tests::record(ui, "welcome_new", &new_file);
                            if new_file.gained_focus() { new_file.scroll_to_me(None); }
                            if new_file.clicked() {
                                self.explorer_new_file();
                            }
                        });
                        ui.add_space(16.0);
                        ui.label(
                            RichText::new(
                                "Browse Project on the left. Files and Project Text open the shared search; Find Results returns to your retained results.",
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
                    let footer = ui.label(
                        RichText::new(
                            "Independent open-source prototype. Not affiliated with JetBrains.",
                        )
                        .size(11.0)
                        .color(MUTED),
                    );
                    #[cfg(test)]
                    workspace_access_tests::record(ui, "welcome_footer", &footer);
                    #[cfg(not(test))]
                    let _ = footer;
                });
            });
        });
        ui.spacing_mut().scroll = scroll_style;
        #[cfg(test)]
        welcome_layout_tests::record(ui, viewport, &scroll);
        #[cfg(not(test))]
        let _ = scroll;
    }

    fn find_bar(&mut self, ui: &mut egui::Ui) {
        if !self.find_open {
            return;
        }
        ui.horizontal_wrapped(|ui| {
            let label = ui.label(RichText::new("FIND").small().color(MUTED));
            #[cfg(test)]
            workspace_access_tests::record(ui, "find_label", &label);
            #[cfg(not(test))]
            let _ = label;
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
            #[cfg(test)]
            workspace_access_tests::record(ui, "find_query", &response);
            if response.changed() {
                self.file_find.cancel_loop();
                self.file_find.message = None;
                self.replace.invalidate();
            }
            let previous = ui
                .small_button("Previous")
                .on_hover_text("Previous match · Shift+F3");
            #[cfg(test)]
            workspace_access_tests::record(ui, "find_previous", &previous);
            self.file_find_button(ui, &previous, file_find::Direction::Previous);
            let next = ui.small_button("Next").on_hover_text("Next match · F3");
            #[cfg(test)]
            workspace_access_tests::record(ui, "find_next", &next);
            self.file_find_button(ui, &next, file_find::Direction::Next);
            let count = self.file_find_count(ui.ctx());
            let count_response = ui.label(RichText::new(count).small().color(MUTED));
            #[cfg(test)]
            workspace_access_tests::record(ui, "find_count", &count_response);
            let _ = count_response;
            let close = ui.small_button("x");
            #[cfg(test)]
            workspace_access_tests::record(ui, "find_close", &close);
            if close.clicked() {
                self.close_find(ui.ctx());
            }
        });
        if let Some(message) = self.file_find.message {
            ui.label(RichText::new(message).small().color(MUTED));
        }
        if self.find_open {
            self.replace_bar(ui);
        }
        ui.separator();
    }

    #[cfg(test)]
    fn editor(&mut self, ui: &mut egui::Ui) {
        self.editor_with_input(ui, None, None);
    }

    fn editor_indent_eligible(&self, ctx: &egui::Context) -> bool {
        !self.navigation.blocks_editor()
            && !self.file_structure_open()
            && !self.navigation.restore_focus
            && !self.foreign_modal_owns_input(ctx)
            && !self.open_form
            && !self.new_file
            && !self.find_focus
            && self.close_tab_requested.is_none()
            && !self.close_after_language_stop
            && !self.allow_close
    }

    fn editor_with_input(
        &mut self,
        ui: &mut egui::Ui,
        enter_input: Option<enter_indent::FrameInput>,
        duplicate_input: Option<duplicate_lines::FrameInput>,
    ) {
        let mut activate = None;
        let mut close = None;
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
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(4.0, 0.0);
            ui.spacing_mut().interact_size.y = chrome::HEADER_HEIGHT;
            let tabs_width = (ui.available_width() - 30.0).max(0.0);
            ui.allocate_ui_with_layout(
                egui::vec2(tabs_width, 30.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_min_size(egui::vec2(tabs_width, chrome::HEADER_HEIGHT));
                    egui::ScrollArea::horizontal()
                        .id_salt("tabs")
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().interact_size.y = 26.0;
                                for doc in &self.documents {
                                    let active = Some(doc.id) == self.active_document;
                                    ui.push_id(("editor_tab", doc.id), |ui| {
                                        egui::Frame::new()
                                            .fill(if active { chrome::SELECTED } else { BG })
                                            .stroke(Stroke::new(
                                                1.0_f32,
                                                if active {
                                                    Color32::from_rgb(53, 83, 135)
                                                } else {
                                                    BG
                                                },
                                            ))
                                            .corner_radius(6)
                                            .inner_margin(egui::Margin::symmetric(6, 1))
                                            .show(ui, |ui| {
                                                ui.horizontal(|ui| {
                                                    let (rect, _) = ui.allocate_exact_size(
                                                        egui::vec2(16.0, 16.0),
                                                        egui::Sense::hover(),
                                                    );
                                                    chrome::paint_icon(
                                                        ui,
                                                        rect,
                                                        chrome::file_icon(&doc.path, false),
                                                        MUTED,
                                                    );
                                                    let name = doc
                                                        .path
                                                        .rsplit('/')
                                                        .next()
                                                        .unwrap_or(&doc.path);
                                                    let suffix = if doc.saving {
                                                        "  ~"
                                                    } else if doc.dirty() {
                                                        "  •"
                                                    } else {
                                                        ""
                                                    };
                                                    let tab = ui
                                                        .add(
                                                            egui::Button::new(
                                                                RichText::new(format!(
                                                                    "{name}{suffix}"
                                                                ))
                                                                .color(if active {
                                                                    TEXT
                                                                } else {
                                                                    MUTED
                                                                }),
                                                            )
                                                            .frame(false),
                                                        )
                                                        .on_hover_text(&doc.path);
                                                    #[cfg(test)]
                                                    workspace_access_tests::record(
                                                        ui,
                                                        &format!("editor_tab:{}", doc.id),
                                                        &tab,
                                                    );
                                                    if tab.clicked() {
                                                        activate = Some(doc.id);
                                                    }
                                                    let close_tab = ui
                                                        .add(chrome::IconButton::new(
                                                            chrome::Icon::Close,
                                                            "Close tab",
                                                        ))
                                                        .on_hover_text("Close tab · Ctrl/Cmd+W");
                                                    if close_tab.clicked() {
                                                        close = Some(doc.id);
                                                    }
                                                });
                                            });
                                    });
                                }
                            });
                        });
                },
            );
            let menu = chrome::menu_button(ui, "", |ui| {
                if let Some(doc) = self.active() {
                    let copy = ui.button("Copy draft");
                    #[cfg(test)]
                    workspace_access_tests::record(ui, "editor_copy_draft", &copy);
                    if copy.clicked() {
                        ui.ctx().copy_text(doc.text.clone());
                        ui.close_menu();
                    }
                    let compare_action = ui
                        .add_enabled(can_compare, egui::Button::new("Compare with disk"))
                        .on_hover_text("Read the current file without changing your draft");
                    #[cfg(test)]
                    workspace_access_tests::record(ui, "editor_compare_with_disk", &compare_action);
                    if compare_action.clicked() {
                        compare = true;
                        ui.close_menu();
                    }
                }
            })
            .response;
            chrome::paint_icon(
                ui,
                egui::Rect::from_center_size(menu.rect.center(), egui::vec2(16.0, 16.0)),
                chrome::Icon::More,
                MUTED,
            );
            menu.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    ui.is_enabled(),
                    "Editor actions",
                )
            });
            register_island_menu(ui, menu.id);
            #[cfg(test)]
            workspace_access_tests::record(ui, "editor_overflow", &menu);
            menu.on_hover_text("Editor actions · Copy draft and Compare with disk");
        });
        self.history_message(ui);
        let activate = activate.filter(|_| {
            ui.is_enabled()
                && !self.navigation.blocks_editor()
                && !self.foreign_modal_owns_input(ui.ctx())
        });
        if let Some(id) = activate {
            self.activate_history_tab(id);
        }
        if let Some(id) = close {
            self.close_tab_requested = Some(id);
        }
        ui.separator();
        // Routine path/dirty state lives in status/tabs. Save risks remain explicit in the editor.
        if let Some(doc) = self.active() {
            if doc.save_outcome_unknown() || doc.text.len() > cedar_protocol::MAX_FILE_BYTES {
                ui.horizontal_wrapped(|ui| {
                    if doc.save_outcome_unknown() { ui.colored_label(AMBER, "SAVE OUTCOME UNKNOWN"); }
                    if doc.text.len() > cedar_protocol::MAX_FILE_BYTES { ui.colored_label(RED, "OVER 1 MiB · copy or shorten to save"); }
                    if doc.interrupted_save.is_some() && !doc.save_outcome_unverifiable {
                        check_save = ui.add_enabled(can_check_save, egui::Button::new("Check interrupted save").small())
                            .on_hover_text("Read disk twice and compare with submitted contents; never retries the write").clicked();
                    }
                    if ui.add_enabled(can_compare, egui::Button::new("Compare with disk").small()).clicked() { compare = true; }
                    if ui.small_button("Copy draft").clicked() { ui.ctx().copy_text(doc.text.clone()); }
                });
            }
        }
        if compare {
            self.compare_with_disk();
        }
        if check_save {
            self.check_interrupted_save();
        }
        if self
            .active()
            .is_some_and(|doc| doc.save_outcome_unverifiable)
        {
            ui.label(RichText::new("Save outcome unknown; submission identity unavailable. Keep or copy your draft and compare disk. Verification and further saves are blocked for this tab.").small().color(AMBER));
        }
        if let Some(message) = self.interrupted_save_check.message() {
            ui.label(RichText::new(message).small().color(AMBER));
        }
        if let Some(message) = self.navigation.message {
            ui.label(RichText::new(message).small().color(AMBER));
        }
        let navigation_blocked = self.navigation.blocks_editor()
            || self.copy_draft.is_open()
            || matches!(self.confirm, Some(Confirm::CloseTab(_)));
        ui.add_enabled_ui(!navigation_blocked, |ui| self.find_bar(ui));
        let indent_eligible = ui.is_enabled()
            && ui.ctx().input(|input| input.focused)
            && self.editor_indent_eligible(ui.ctx());
        self.duplicate_lines(
            ui.ctx(),
            ui.is_enabled() && !navigation_blocked,
            duplicate_input,
        );
        let find_open = self.find_open;
        let mut history_completion =
            if !navigation_blocked && self.active().is_some_and(|doc| doc.jump_to.is_some()) {
                self.history_take_jump_completion()
            } else {
                None
            };
        let tab_context_owned = self
            .editor_tab_input
            .as_ref()
            .is_some_and(|input| input.matches_context(self, ui.ctx()));
        let mut block_had_focus = false;
        if let Some(doc) = self
            .documents
            .iter_mut()
            .find(|doc| Some(doc.id) == self.active_document)
        {
            editor_state::load(ui.ctx(), doc);
            let indent_eligible = indent_eligible
                && doc.jump_to.is_none()
                && !self.navigation.coordinate_jump_pending();
            enter_indent::handle(ui.ctx(), doc, indent_eligible, enter_input);
            block_had_focus = ui
                .ctx()
                .memory(|memory| memory.has_focus(egui::Id::new(("editor", doc.id))));
            let tab_owned = tab_context_owned
                && block_had_focus
                && self
                    .editor_tab_input
                    .as_ref()
                    .is_some_and(|input| input.matches_document(doc));
            let block_eligible = indent_eligible
                && self
                    .editor_tab_input
                    .as_ref()
                    .is_some_and(|input| input.block_action && input.original_input_eligible());
            block_indent::handle(ui.ctx(), doc, block_eligible, tab_owned);
            if !navigation_blocked {
                editor_state::history_shortcut(ui.ctx(), doc);
            }
        }
        // Coordinate tickets are validated after editor undo handling and the
        // modal's release, immediately before the cursor can be changed. They
        // never enter the older unguarded Open/LSP/plain-line jump slot.
        let coordinate_jump = if navigation_blocked {
            None
        } else {
            self.take_coordinate_jump(ui.ctx())
        };
        if coordinate_jump.is_some() {
            history_completion = self.history_take_jump_completion();
        }
        let tab_context_owned = coordinate_jump.is_none()
            && self.editor_tab_input.as_ref().is_some_and(|input| {
                input.matches_context(self, ui.ctx()) && input.input_eligible(self, ui.ctx())
            });
        let composition_identity = coordinate_jump.is_none()
            && self.editor_tab_input.as_ref().is_some_and(|input| {
                input.matches_identity(self, ui.ctx()) && input.input_eligible(self, ui.ctx())
            });
        if let Some(doc) = self
            .documents
            .iter_mut()
            .find(|doc| Some(doc.id) == self.active_document)
        {
            let editor_id = egui::Id::new(("editor", doc.id));
            let jump_to = coordinate_jump
                .or_else(|| (!navigation_blocked).then(|| doc.jump_to.take()).flatten());
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
            let history_destination = if history_completion.is_some() {
                egui::TextEdit::load_state(ui.ctx(), editor_id)
                    .and_then(|state| state.cursor.char_range())
                    .map(|selection| location_history::Location {
                        generation: self.generation,
                        document: doc.id,
                        edit_version: doc.edit_version,
                        selection,
                    })
            } else {
                None
            };
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
            // The background drag surface is registered from the previous
            // viewport. Confine it to today's editor lane before registration,
            // so a newly arrived banner cannot leave it over the tab toolbar.
            // Keep the same UI/scroll IDs and native wheel, bar and drag state.
            let editor_clip = ui.clip_rect();
            ui.set_clip_rect(editor_clip.intersect(ui.available_rect_before_wrap()));
            let editor_scroll = egui::ScrollArea::both()
                .id_salt(("editor_scroll", doc.id))
                // egui's background drag surface uses last frame's rect. The
                // expanding Find preview must not be covered by that old rect.
                .drag_to_scroll(!find_open)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.horizontal_top(|ui| {
                        syntax::gutter(ui, &line_numbers, font_size);
                        let tab_owned = tab_context_owned
                            && jump_to.is_none()
                            && ui.ctx().memory(|memory| memory.has_focus(editor_id))
                            && self
                                .editor_tab_input
                                .as_ref()
                                .is_some_and(|input| input.matches_document(doc));
                        // Native traversal grants focus inside TextEdit's
                        // interaction step. Only the original owner may turn
                        // this same batch's Tab into an edit. Eventless sizing
                        // retries keep the normal code-editor focus filter.
                        let tab_fenced = editor_tab::has_tab(ui.ctx()) && !tab_owned;
                        let composition = (tab_fenced
                            && !block_had_focus
                            && composition_identity
                            && jump_to.is_none()
                            && self
                                .editor_tab_input
                                .as_ref()
                                .is_some_and(|input| input.matches_document(doc)))
                        .then(|| block_indent::composition_transition(ui.ctx()))
                        .flatten();
                        let output = egui::TextEdit::multiline(&mut doc.text)
                            .id(editor_id)
                            .interactive(!navigation_blocked)
                            .font(FontId::monospace(font_size))
                            .code_editor()
                            .lock_focus(!tab_fenced)
                            .frame(false)
                            .desired_width(ui.available_width().max(600.0))
                            .desired_rows(count.max(30))
                            .margin(egui::vec2(10.0, 0.0))
                            .layouter(&mut layouter)
                            .show(ui);
                        // IME can legitimately change text here, so recheck the
                        // document/lifecycle identity without its edit version.
                        let composition = composition.filter(|_| {
                            self.active_document == Some(doc.id)
                                && self.editor_tab_input.as_ref().is_some_and(|input| {
                                    input.matches_document_identity(doc)
                                        && input.matches_frame(ui.ctx())
                                        && input.matches_lifecycle(
                                            self.generation,
                                            &self.workspace_key,
                                            &self.root,
                                            self.state,
                                            self.language.session,
                                            self.navigation_epoch,
                                        )
                                })
                        });
                        block_indent::finish_focus_composition(
                            ui.ctx(),
                            editor_id,
                            &output.response,
                            composition,
                        );
                        self.workspace_access
                            .record(workspace_access::Target::Editor(doc.id), &output.response);
                        #[cfg(test)]
                        workspace_access_tests::record(ui, "editor", &output.response);
                        #[cfg(test)]
                        if let Some(range) = output.cursor_range {
                            let caret = output
                                .galley
                                .pos_from_ccursor(range.primary.ccursor)
                                .translate(output.galley_pos.to_vec2());
                            ui.ctx().data_mut(|data| {
                                data.insert_temp(
                                    egui::Id::new("compact_history_editor_caret"),
                                    caret,
                                );
                            });
                        }
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
            #[cfg(test)]
            {
                let probe = (
                    editor_scroll.id,
                    editor_scroll.inner_rect,
                    editor_scroll.state.offset,
                    editor_scroll.content_size,
                    ui.clip_rect(),
                    (!find_open)
                        .then(|| ui.ctx().read_response(editor_scroll.id.with("area")))
                        .flatten()
                        .map(EditorScrollResponseSnapshot::capture),
                    ui.ctx()
                        .read_response(editor_scroll.id.with(1usize))
                        .map(EditorScrollResponseSnapshot::capture),
                );
                ui.ctx().data_mut(|data| {
                    data.insert_temp(egui::Id::new("compact_history_editor_scroll"), probe);
                });
            }
            #[cfg(not(test))]
            let _ = editor_scroll;
            ui.set_clip_rect(editor_clip);
            if let Some(destination) = history_destination {
                self.history_commit_destination(history_completion.take(), destination);
            }
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
                if ui.add_enabled(self.ready(), egui::Button::new("Create draft")).clicked() {
                    self.create_new_file_draft();
                }
            });
            if !visible {
                self.new_file = false;
            }
        }
        self.tab_close_dialog(ctx);
        if matches!(self.confirm, Some(Confirm::CloseWindow)) {
            egui::Modal::new(egui::Id::new("discard_confirmation")).show(ctx, |ui| {
                ui.set_max_width(430.0);
                ui.heading("Discard unsaved changes?");
                ui.label("Your changes to all unsaved files and profile form edits and recovery copies owned by these tabs will be discarded. Save or copy the draft first if you need to keep it.");
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("Keep editing").clicked() { self.keep_editing_recovery(ctx); }
                    if ui.button(RichText::new("Discard and quit").color(RED)).clicked() {
                        self.confirm = None;
                        self.begin_close(ctx);
                    }
                });
            });
        }
        self.disk_review_window(ctx);
    }
}

impl eframe::App for CedarApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        // eframe expects gamma-encoded components, unlike egui::Rgba's linear values.
        chrome::FRAME.to_normalized_gamma_f32()
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        self.capture_project_reveal_input(raw_input);
        self.capture_file_structure_input(raw_input);
        workspace_access::capture_input(self, ctx, raw_input);
        self.editor_tab_input = editor_tab::FrameInput::capture(self, ctx, raw_input);
        self.duplicate_input = self.capture_duplicate_input(ctx, raw_input);
        self.tools_height_reset.begin_input_frame(raw_input);
        self.capture_replace_input(ctx, raw_input);
        self.navigation.begin_input_frame(ctx, raw_input);
        self.capture_tab_close_focus_input(ctx, raw_input);
        self.enter_input = enter_indent::capture(ctx, raw_input, self.active()).filter(|_| {
            self.editor_indent_eligible(ctx)
                && self.active().is_some_and(|doc| doc.jump_to.is_none())
        });
        let explorer_input = explorer_tree::InputBatch::capture(raw_input);
        // Classify every surface from the same ordered native batch. Filtering
        // sooner could create a new clean action or hide a held key's release.
        let mut consumed = vec![false; raw_input.events.len()];
        if self.capture_java_type_input(ctx, raw_input, &mut consumed) {
            // The result's held Enter cannot become editor indentation after
            // navigation moves focus; this grant saw the original raw batch.
            self.enter_input = None;
        }
        self.capture_file_find_input(ctx, raw_input);
        self.capture_recent_input(ctx, raw_input, &mut consumed);
        self.capture_browse_input(ctx, raw_input, &mut consumed);
        self.capture_test_report_input(ctx, raw_input, &mut consumed);
        self.capture_search_input(ctx, raw_input, &mut consumed);
        self.explorer_tree_input(ctx, raw_input, explorer_input, &mut consumed);
        workspace_access::finish_input_capture(ctx, raw_input, &mut consumed);
        self.finish_browse_input_capture(ctx, &raw_input.events, &consumed);
        let mut index = 0;
        raw_input.events.retain(|_| {
            let keep = !consumed[index];
            index += 1;
            keep
        });
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let duplicate_input = self.duplicate_input.take();
        self.begin_tab_close_focus_frame(ctx);
        let enter_input = self.enter_input.take();
        self.workspace_access = workspace_access::Access::default();
        #[cfg(test)]
        workspace_access_tests::begin(ctx);
        self.poll_ready_navigation(Some(ctx));
        self.guard_project_name_input(ctx);
        self.consume_file_find_keys(ctx);
        self.begin_navigation_frame(ctx);
        self.recovery_tick(ctx);
        let cjk = self.system_fonts.needs_probe()
            && (self.cjk_seen
                || self.language.cjk_seen
                || self.documents.iter().any(|doc| doc.has_cjk)
                || system_fonts::contains_cjk(&self.form.local_root)
                || system_fonts::contains_cjk(&self.form.remote_root)
                || system_fonts::contains_cjk(&self.search_query)
                || system_fonts::contains_cjk(&self.search_scope)
                || system_fonts::contains_cjk(&self.search_masks)
                || system_fonts::contains_cjk(&self.find_query)
                || system_fonts::contains_cjk(&self.replace.replacement)
                || self.navigation.has_cjk()
                || self.copy_draft.has_cjk()
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
        self.file_structure_window(ctx);
        self.header(ctx);
        self.footer(ctx);
        self.notifications(ctx);
        self.tool_rail(ctx);
        self.tools(ctx);
        if self.workspace_key.is_some() {
            self.sidebar(ctx);
        }
        // The query TextEdit can discover same-pass pointer focus loss during
        // sidebar layout. Fence its original native keys before a new editor.
        self.guard_project_name_input(ctx);
        egui::CentralPanel::default()
            .frame(chrome::island())
            .show(ctx, |ui| {
                if self.documents.is_empty() {
                    self.welcome(ui, ctx);
                } else {
                    self.editor_with_input(ui, enter_input, duplicate_input);
                }
            });
        self.dialogs(ctx);
        self.copy_draft_window(ctx);
        if !self.navigation.blocks_editor() {
            self.language_popups(ctx);
        }
        self.run_tick(ctx);
        self.recovery_window(ctx);
        self.run_dialog(ctx);
        self.finish_disk_reload(ctx);
        self.finish_disk_merge(ctx);
        self.finish_profile_actions();
        self.finish_interrupted_save_check();
        self.finish_tab_close();
        self.finish_replace_frame(ctx);
        self.finish_java_implementation_frame(ctx);
        self.language_tick(ctx);
        self.recovery_tick(ctx);
        self.finish_recovery_close_frame(ctx);
        self.finish_workspace_access_frame(ctx);
        self.finish_history_frame(ctx);
        // Acknowledgements are drained first, but a subsequent batch Write is
        // admitted only after every editor, cancel and draft-mutation finisher.
        self.finish_save_all_frame(ctx);
        self.finish_copy_draft_frame(ctx);
        self.finish_save_close_tab_frame(ctx);
        self.finish_tab_close_focus_frame(ctx);
        self.finish_java_parameter_info_frame();
        self.finish_search_frame(ctx);
        self.browse_finish_frame(ctx);
        self.finish_recent_frame(ctx);
        self.finish_project_reveal_frame(ctx);
        self.finish_file_structure_frame(ctx);
        self.finish_file_find_frame(ctx);
        #[cfg(test)]
        if let Some(mutate) = self.close_frame_mutation.take() {
            mutate(self, ctx);
        }
        self.finish_transport_close_frame(ctx);
    }
}

fn shell_button_width(ui: &egui::Ui, text: &str, small: bool) -> f32 {
    let text = egui::WidgetText::from(text);
    let text = if small {
        text.text_style(egui::TextStyle::Body)
    } else {
        text
    };
    text.into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        egui::TextStyle::Button,
    )
    .size()
    .x + if ui.visuals().button_frame {
        2.0 * ui.spacing().button_padding.x
    } else {
        0.0
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
                file_discovery: None,
                scoped_search: None,
                case_search: None,
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
                file_discovery: None,
                scoped_search: None,
                case_search: None,
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
                file_discovery: None,
                scoped_search: None,
                case_search: None,
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
    fn status_test_app() -> CedarApp {
        let app = CedarApp::empty();
        app.editor_ctx.set_fonts(system_fonts::base_fonts());
        app.editor_ctx.style_mut(|style| {
            style.spacing.item_spacing = egui::vec2(6.0, 4.0);
            style.spacing.button_padding = egui::vec2(8.0, 4.0);
            style
                .text_styles
                .insert(egui::TextStyle::Body, FontId::proportional(13.0));
            style
                .text_styles
                .insert(egui::TextStyle::Button, FontId::proportional(13.0));
            style.interaction.tooltip_delay = 0.0;
            style.interaction.tooltip_grace_time = 0.0;
            style.interaction.show_tooltips_only_when_still = false;
        });
        app
    }

    fn status_test_frame(
        app: &mut CedarApp,
        size: [f32; 2],
        time: f64,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        let ctx = app.editor_ctx.clone();
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size.into())),
                time: Some(time),
                events,
                ..Default::default()
            },
            |ctx| {
                workspace_access_tests::begin(ctx);
                app.footer(ctx);
            },
        )
    }

    fn status_test_text(output: &egui::FullOutput, screen: egui::Rect, prefix: &str) -> egui::Rect {
        let (rect, clip) = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::epaint::Shape::Text(text) if text.galley.text().starts_with(prefix) => {
                    Some((text.visual_bounding_rect(), shape.clip_rect))
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("Missing status text: {prefix}"));
        assert!(
            screen.contains_rect(rect),
            "{prefix} outside screen: {rect:?}"
        );
        assert!(
            clip.contains_rect(rect),
            "{prefix} clipped: {rect:?} / {clip:?}"
        );
        rect
    }

    fn status_test_response(app: &CedarApp, screen: egui::Rect, name: &str) -> egui::Response {
        let response = workspace_access_tests::recorded_response(app, name);
        assert!(
            response.rect.is_positive() && screen.contains_rect(response.rect),
            "{name} outside screen: {:?}",
            response.rect
        );
        assert!(
            workspace_access_tests::recorded_clip(app, name).contains_rect(response.rect),
            "{name} hit target clipped"
        );
        response
    }

    #[test]
    fn maximum_agent_metadata_preserves_recovery_visibility_at_minimum_window_size() {
        let mut app = status_test_app();
        let mut info = agent_support::full_test_agent();
        info.version = "v".repeat(cedar_protocol::MAX_AGENT_VERSION_BYTES);
        info.os = "o".repeat(cedar_protocol::MAX_AGENT_PLATFORM_BYTES);
        info.arch = "a".repeat(cedar_protocol::MAX_AGENT_PLATFORM_BYTES);
        info.validate().unwrap();
        app.agent_info = Some(info);
        app.state = ConnectionState::Ready;
        app.root = "status-metadata-fixture".into();
        app.active_form = Some(ConnectForm::default());
        app.recovery.enabled = true;
        app.recovery.error = Some("Backup needs review".into());
        let size = [780.0, 540.0];
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size.into());
        for time in [0.0, 0.1, 0.2] {
            status_test_frame(&mut app, size, time, vec![]);
        }
        let output = status_test_frame(&mut app, size, 0.3, vec![]);
        status_test_text(&output, screen, "Recovery needs attention");
        let recovery = status_test_response(&app, screen, "recovery_status");
        assert!(recovery.enabled() && recovery.sense.senses_click());
        assert!(app.agent_status().len() > 128);
        assert!(app.agent_details().contains("not verified identity"));
        // Routine agent metadata belongs to the real path tooltip. Observe its
        // painted contents after a native hover, not private tooltip storage.
        let hover_at = status_test_text(&output, screen, &app.root).center();
        status_test_frame(
            &mut app,
            size,
            1.0,
            vec![egui::Event::PointerMoved(hover_at)],
        );
        status_test_frame(&mut app, size, 2.0, vec![]);
        let tooltip = status_test_frame(&mut app, size, 3.0, vec![]);
        let metadata = tooltip.shapes.iter().find_map(|shape| match &shape.shape {
            egui::epaint::Shape::Text(text)
                if text.galley.text().contains(&app.agent_status())
                    && text.galley.text().contains(&app.agent_details()) => Some((text, shape.clip_rect)),
            _ => None,
        }).expect("actual hovered path tooltip must retain full reported metadata and identity caveat");
        assert!(screen.contains_rect(metadata.0.visual_bounding_rect()));
        assert!(metadata.1.contains_rect(metadata.0.visual_bounding_rect()));
        // The warning remains a real reachable Recovery action.
        for (time, pressed) in [(4.0, true), (4.1, false)] {
            status_test_frame(
                &mut app,
                size,
                time,
                vec![
                    egui::Event::PointerMoved(recovery.rect.center()),
                    egui::Event::PointerButton {
                        pos: recovery.rect.center(),
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        assert!(app.recovery.visible);
        assert_eq!(app.recovery.error.as_deref(), Some("Backup needs review"));
    }

    #[test]
    fn ordinary_idle_and_healthy_status_fit_one_standard_height_line() {
        let size = [780.0, 540.0];
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size.into());
        for ready in [false, true] {
            let mut app = status_test_app();
            if ready {
                app.state = ConnectionState::Ready;
                app.root = "/workspace".into();
                app.recovery.enabled = true;
                app.recovery.initialized = true;
                let mut doc = Document::new(
                    1,
                    "src/Main.java".into(),
                    "class Main {}".into(),
                    "r1".into(),
                );
                doc.cursor = (200, 42);
                app.documents.push(doc);
                app.active_document = Some(1);
            }
            for time in [0.0, 0.1, 0.2] {
                status_test_frame(&mut app, size, time, vec![]);
            }
            let output = status_test_frame(&mut app, size, 0.3, vec![]);
            let panel =
                egui::containers::panel::PanelState::load(&app.editor_ctx, egui::Id::new("status"))
                    .unwrap()
                    .rect;
            assert!(
                panel.height() >= 24.0 && panel.height() <= 28.0,
                "ordinary status used extra rows: ready={ready}, panel={panel:?}"
            );
            for name in ["connection_status", "recovery_status"] {
                assert!(panel.contains_rect(status_test_response(&app, screen, name).rect));
            }
            let path = if ready { "src/Main.java" } else { "Cedar" };
            let glyphs = status_test_text(&output, screen, path);
            let hit = status_test_response(&app, screen, "status_path").rect;
            assert!((hit.left() - (panel.left() + 10.0)).abs() <= 0.125);
            assert!((glyphs.left() - hit.left()).abs() <= 1.0,
                "path glyphs must paint at the left status margin, not the lane center: {glyphs:?} / {hit:?}");
            if ready {
                for label in ["src/Main.java", "Ln 200, Col 42", "UTF-8"] {
                    status_test_text(&output, screen, label);
                }
            } else {
                status_test_text(&output, screen, "Not connected");
                status_test_text(&output, screen, "Recovery off");
            }
        }
    }

    #[test]
    fn status_pending_and_long_notices_cannot_displace_connected_content() {
        let size = [780.0, 540.0];
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size.into());
        let mut app = status_test_app();
        app.state = ConnectionState::Ready;
        app.recovery.enabled = true;
        app.recovery.initialized = true;
        app.documents.push(Document::new(
            1,
            "src/Main.java".into(),
            "draft".into(),
            "r1".into(),
        ));
        app.active_document = Some(1);
        let frame = |app: &mut CedarApp, time: f64| {
            let ctx = app.editor_ctx.clone();
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    time: Some(time),
                    ..Default::default()
                },
                |ctx| {
                    workspace_access_tests::begin(ctx);
                    app.footer(ctx);
                    egui::CentralPanel::default().show(ctx, |ui| app.editor(ui));
                },
            )
        };
        let panel = |app: &CedarApp| {
            egui::containers::panel::PanelState::load(&app.editor_ctx, egui::Id::new("status"))
                .unwrap()
                .rect
        };
        for time in [0.0, 0.1, 0.2] {
            frame(&mut app, time);
        }
        let idle = panel(&app);
        app.pending.insert(
            1,
            Job::List {
                path: String::new(),
            },
        );
        let output = frame(&mut app, 0.3);
        assert_eq!(
            panel(&app),
            idle,
            "ordinary Read/List progress must not move the tools or editor vertically"
        );
        status_test_text(&output, screen, "1 pending");
        for (time, scope) in [(0.4, "a".repeat(4000)), (0.5, "路径/".repeat(400))] {
            app.notice = format!("1 matches for directory {scope}");
            let output = frame(&mut app, time);
            assert!(
                panel(&app).height() <= 48.0,
                "long notice consumed editor space: {:?}",
                panel(&app)
            );
            let notice = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::epaint::Shape::Text(text) if text.galley.text() == app.notice => {
                        Some(text)
                    }
                    _ => None,
                })
                .expect("full notice must remain in the elided galley for its hover disclosure");
            assert_eq!(notice.galley.rows.len(), 1);
            status_test_text(&output, screen, "1 matches for directory");
            status_test_text(&output, screen, "UTF-8");
            status_test_response(&app, screen, "recovery_status");
            // The scrollable TextEdit canvas can exceed its clip; require its
            // real visible viewport and the actual document glyphs, not the
            // entire canvas, to remain available above the status panel.
            let editor = workspace_access_tests::recorded_response(&app, "editor");
            let visible_editor = editor
                .rect
                .intersect(workspace_access_tests::recorded_clip(&app, "editor"))
                .intersect(screen);
            assert!(
                visible_editor.is_positive()
                    && visible_editor.height() > 100.0
                    && visible_editor.bottom() <= panel(&app).top()
            );
            status_test_text(&output, screen, "draft");
        }
        app.notice.clear();
        app.pending.clear();
        frame(&mut app, 0.6);
        assert_eq!(panel(&app), idle);
        assert_eq!(app.documents[0].text, "draft");
    }

    #[test]
    fn status_first_risk_frame_is_visible_with_pending_and_notice_at_both_fonts() {
        let size = [780.0, 540.0];
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size.into());
        for font in [13.0, 18.0] {
            for notice_before in [false, true] {
                let mut app = status_test_app();
                app.editor_ctx.style_mut(|style| {
                    style
                        .text_styles
                        .insert(egui::TextStyle::Body, FontId::proportional(font));
                    style
                        .text_styles
                        .insert(egui::TextStyle::Button, FontId::proportional(font));
                });
                app.state = ConnectionState::Ready;
                app.agent_info = Some(agent_support::full_test_agent());
                app.recovery.enabled = true;
                app.recovery.initialized = true;
                app.documents.push(Document::new(
                    1,
                    "Main.java".into(),
                    "draft".into(),
                    "r1".into(),
                ));
                app.active_document = Some(1);
                if notice_before {
                    app.notice = "Read completed".into();
                }
                for time in [0.0, 0.1, 0.2] {
                    status_test_frame(&mut app, size, time, vec![]);
                }
                let before = egui::containers::panel::PanelState::load(
                    &app.editor_ctx,
                    egui::Id::new("status"),
                )
                .unwrap()
                .rect;
                app.unverified_local_close = true;
                app.agent_info
                    .as_mut()
                    .unwrap()
                    .capabilities
                    .retain(|name| name != "write");
                app.pending.insert(
                    1,
                    Job::List {
                        path: String::new(),
                    },
                );
                app.notice = "Read completed".into();
                let output = status_test_frame(&mut app, size, 0.3, vec![]);
                for label in [
                    "Prior cleanup unverified",
                    "Saves unavailable",
                    "Read completed",
                    "1 pending",
                    "UTF-8",
                ] {
                    status_test_text(&output, screen, label);
                }
                status_test_response(&app, screen, "recovery_status");
                status_test_response(&app, screen, "connection_status");
                app.unverified_local_close = false;
                app.agent_info = Some(agent_support::full_test_agent());
                app.pending.clear();
                if !notice_before {
                    app.notice.clear();
                }
                let output = status_test_frame(&mut app, size, 0.4, vec![]);
                assert_eq!(
                    egui::containers::panel::PanelState::load(
                        &app.editor_ctx,
                        egui::Id::new("status")
                    )
                    .unwrap()
                    .rect,
                    before
                );
                status_test_text(&output, screen, "UTF-8");
                assert_eq!(app.documents[0].text, "draft");
            }
        }
    }

    #[test]
    fn status_keeps_connection_recovery_and_cleanup_risks_visible_with_long_paths() {
        let size = [780.0, 540.0];
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size.into());
        for (state, label) in [
            (ConnectionState::Idle, "Not connected"),
            (ConnectionState::Connecting, "Connecting"),
            (ConnectionState::Disconnected, "Disconnected"),
            (ConnectionState::Disconnecting, "Disconnecting"),
            (ConnectionState::CleanupUnverified, "Cleanup unverified"),
        ] {
            let mut app = status_test_app();
            app.state = state;
            app.root = format!("/workspace/{}", "long-directory/".repeat(64));
            app.unverified_local_close = true;
            app.recovery.enabled = true;
            app.recovery.error = Some("Retained backup failure".into());
            let mut doc = Document::new(
                1,
                format!("{}Main.java", "long-directory/".repeat(64)),
                "saved".into(),
                "r1".into(),
            );
            doc.text = "dirty draft".into();
            doc.cursor = (200, 42);
            app.documents.push(doc);
            app.active_document = Some(1);
            let before = (
                app.documents[0].text.clone(),
                app.documents[0].saved_text.clone(),
                app.documents[0].edit_version,
            );
            for time in [0.0, 0.1, 0.2] {
                status_test_frame(&mut app, size, time, vec![]);
            }
            let output = status_test_frame(&mut app, size, 0.3, vec![]);
            for text in [
                label,
                "Recovery needs attention",
                "Prior cleanup unverified",
                "Ln 200, Col 42",
                "UTF-8",
            ] {
                status_test_text(&output, screen, text);
            }
            for name in ["connection_status", "recovery_status"] {
                status_test_response(&app, screen, name);
            }
            assert_eq!(
                (
                    app.documents[0].text.clone(),
                    app.documents[0].saved_text.clone(),
                    app.documents[0].edit_version
                ),
                before
            );
            assert_eq!(app.active_document, Some(1));
            assert!(app.documents[0].dirty());
            assert!(app.pending.is_empty());
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
