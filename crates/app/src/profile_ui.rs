//! Explicit project profiles. The ordinary editor document owns all disk state.
use crate::{
    editor_state, model::Document, recovery_ui, task_profiles::*, CedarApp, Job, AMBER, MUTED,
};
use cedar_protocol::Operation;
use cedar_recovery::WorkspaceIdentity;
use eframe::egui::{self, RichText};

pub(super) const PATH: &str = "cedar.tasks.json";
#[derive(Clone, Debug)]
pub(super) struct Source {
    workspace: WorkspaceIdentity,
    generation: u64,
    document: u64,
    edit_version: u64,
    revision: Option<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mode {
    Manual,
    Saved(usize),
    New,
}
#[derive(Clone, Copy)]
pub(super) enum Action {
    Load,
    Select(Option<usize>),
    New,
    Save,
    Discard,
    Review,
    Run,
    OpenRaw,
}
pub(super) struct Profiles {
    queued: Option<Action>,
    load_navigation: Option<u64>,
    pub draft: TaskProfile,
    baseline: TaskProfile,
    mode: Mode,
    file: Option<TaskFile>,
    source: Option<Source>,
    owner: Option<WorkspaceIdentity>,
    pub epoch: u64,
    review_required: bool,
    pub message: Option<String>,
}
fn empty_command() -> TaskProfile {
    TaskProfile {
        name: String::new(),
        program: String::new(),
        args: Vec::new(),
        timeout_secs: 30,
    }
}
impl Default for Profiles {
    fn default() -> Self {
        Self {
            queued: None,
            load_navigation: None,
            draft: empty_command(),
            baseline: empty_command(),
            mode: Mode::Manual,
            file: None,
            source: None,
            owner: None,
            epoch: 0,
            review_required: false,
            message: None,
        }
    }
}
impl Profiles {
    pub fn dirty(&self) -> bool {
        self.mode == Mode::New || (self.mode != Mode::Manual && self.draft != self.baseline)
    }
    pub fn changed(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
    }
    pub fn disconnected(&mut self) {
        self.queued = None;
        self.load_navigation = None;
        self.changed();
        self.review_required = self.source.is_some() || self.mode != Mode::Manual;
    }
    pub fn connected(&mut self, workspace: WorkspaceIdentity) {
        if self.owner.as_ref().is_some_and(|old| old != &workspace) {
            *self = Self::default();
        }
        self.owner = Some(workspace);
    }
    pub fn owns_document(&self, id: u64) -> bool {
        self.source
            .as_ref()
            .is_some_and(|source| source.document == id)
    }
    pub fn document_closed(&mut self, id: u64) {
        if !self.owns_document(id) {
            return;
        }
        let owner = self.owner.clone();
        if self.mode == Mode::Manual {
            self.file = None;
            self.source = None;
            self.changed();
        } else {
            *self = Self::default();
            self.owner = owner;
        }
    }
}
impl CedarApp {
    pub(super) fn profile_disk_reloaded(&mut self) {
        // Retain the form, its baseline and its old source. Changed source
        // version/revision makes Save/Run fail until the user explicitly loads.
        // Also invalidate same-frame actions and outstanding profile reads.
        self.profiles.queued = None;
        self.profiles.load_navigation = None;
        self.profiles.changed();
        self.profiles.message = Some("The configuration tab was reloaded. Your profile form is retained; discard form changes if needed, then explicitly Load the editor version before Save or Run".into());
    }
    pub(super) fn queue_profile_action(&mut self, action: Action) {
        // Run only after this frame's form and raw-editor input has been applied.
        self.profiles.queued = Some(action);
    }
    pub(super) fn finish_profile_actions(&mut self) {
        match self.profiles.queued.take() {
            Some(Action::Load) => self.load_profiles(),
            Some(Action::Select(index)) => self.select_profile(index),
            Some(Action::New) => self.new_profile(),
            Some(Action::Save) => self.save_profile(),
            Some(Action::Discard) => self.discard_profile_form(),
            Some(Action::Review) => self.review_profile_connection(),
            Some(Action::Run) => self.run(),
            Some(Action::OpenRaw) => {
                self.navigation_changed();
                self.active_document = self.profiles.source.as_ref().map(|source| source.document);
            }
            None => {}
        }
    }
    pub(super) fn profile_load_pending(&self) -> bool {
        self.pending
            .values()
            .any(|job| matches!(job, Job::ProfilesLoad { .. }))
    }
    fn profile_source(&self, document: u64) -> Option<Source> {
        let doc = self.documents.iter().find(|doc| doc.id == document)?;
        Some(Source {
            workspace: self.recovery_workspace()?,
            generation: self.generation,
            document,
            edit_version: doc.edit_version,
            revision: doc.revision.clone(),
        })
    }
    fn profile_source_problem(&self, allow_review: bool) -> Option<&'static str> {
        let Some(source) = &self.profiles.source else {
            return Some("Load cedar.tasks.json before saving a profile");
        };
        if self.recovery_workspace().as_ref() != Some(&source.workspace) {
            return Some("The profile belongs to a different workspace; reconnect to its original host and root");
        }
        if !allow_review && (source.generation != self.generation || self.profiles.review_required)
        {
            return Some("Connection changed. Review the retained draft or explicitly Load profiles before running or saving");
        }
        let Some(doc) = self
            .documents
            .iter()
            .find(|doc| doc.id == source.document && doc.path == PATH)
        else {
            return Some("The configuration tab was closed; Load profiles again");
        };
        if doc.edit_version != source.edit_version || doc.revision != source.revision {
            return Some("The raw editor and profile form diverged. Both drafts are retained. Copy the form if needed, then Discard form changes and Load the editor version");
        }
        if doc.saving {
            return Some("The configuration is saving; wait for its acknowledgement");
        }
        None
    }
    pub(super) fn profile_run_problem(&self) -> Option<&'static str> {
        if self.profiles.mode == Mode::Manual {
            None
        } else {
            self.profile_source_problem(false)
        }
    }
    pub(super) fn load_profiles(&mut self) {
        if !self.ready() || self.profile_load_pending() {
            return;
        }
        if self.profiles.dirty() {
            self.profiles.message = Some("Save or Discard form changes before loading. Both drafts are retained; Cancel a running command separately".into());
            return;
        }
        if let Some(doc) = self.documents.iter().find(|doc| doc.path == PATH) {
            if doc.saving {
                self.profiles.message =
                    Some("Wait for the configuration save before loading".into());
                return;
            }
            let id = doc.id;
            if self.active_document.is_none() {
                self.active_document = Some(id);
            }
            self.load_profile_document(id, true);
            return;
        }
        if self.documents.len() >= 32 {
            self.profiles.message =
                Some("Close a tab before loading profiles (32-buffer limit)".into());
            return;
        }
        // A single ordinary Read; no file watching, discovery command, or implicit run.
        self.profiles.changed();
        self.profiles.load_navigation = Some(self.navigation_epoch);
        self.request(
            Operation::Read { path: PATH.into() },
            Job::ProfilesLoad {
                epoch: self.profiles.epoch,
            },
        );
    }
    fn load_profile_document(&mut self, id: u64, focus_invalid: bool) {
        let Some(doc) = self.documents.iter().find(|doc| doc.id == id) else {
            return;
        };
        let file = match parse_task_file(&doc.text) {
            Ok(file) => file,
            Err(error) => {
                self.profiles.message = Some(format!(
                    "Invalid profiles: {error}. The original text remains in the configuration tab"
                ));
                if focus_invalid {
                    self.active_document = Some(id);
                }
                return;
            }
        };
        let old_name =
            (self.profiles.mode != Mode::Manual).then(|| self.profiles.draft.name.clone());
        self.profiles.source = self.profile_source(id);
        self.profiles.owner = self.recovery_workspace();
        self.profiles.file = Some(file);
        self.profiles.review_required = false;
        self.profiles.message = Some(
            "Loaded the configuration editor buffer. Loading and selecting never execute commands"
                .into(),
        );
        self.profiles.changed();
        if let Some(name) = old_name {
            let index = self
                .profiles
                .file
                .as_ref()
                .unwrap()
                .profiles
                .iter()
                .position(|profile| profile.name == name);
            self.select_profile(index);
        }
    }
    pub(super) fn apply_profile_load(&mut self, epoch: u64, file: Option<(String, String)>) {
        let focus_invalid = self.profiles.load_navigation.take() == Some(self.navigation_epoch);
        // The response may arrive after a raw-editor open or a form edit. Preserve both.
        let id = if let Some(doc) = self.documents.iter().find(|doc| doc.path == PATH) {
            doc.id
        } else {
            if self.documents.len() >= 32 {
                self.profiles.message = Some(
                    "The 32-buffer limit was reached. Load profiles again after closing a tab"
                        .into(),
                );
                return;
            }
            let missing = file.is_none();
            let (text, revision) =
                file.unwrap_or_else(|| ("{\"version\":1,\"profiles\":[]}".into(), String::new()));
            let id = self.next_document;
            self.next_document += 1;
            let mut doc = Document::new(id, PATH.into(), text, revision);
            if missing {
                doc.revision = None;
            }
            self.documents.push(doc);
            id
        };
        // A first Load must leave a usable editor selection. Do not steal a
        // newer selection, including a deliberately emptied editor after navigation.
        if focus_invalid && self.active_document.is_none() {
            self.active_document = Some(id);
        }
        if epoch != self.profiles.epoch || self.profiles.dirty() {
            self.profiles.message = Some("Configuration read finished, but the command form changed. Your edits are retained; choose Load again when ready".into());
            return;
        }
        self.load_profile_document(id, focus_invalid);
        if self
            .documents
            .iter()
            .any(|doc| doc.id == id && doc.revision.is_none())
        {
            self.profiles.message = Some("No cedar.tasks.json exists. This is an unsaved configuration tab; New profile then Save creates it with an exclusive creation check".into());
        }
    }
    pub(super) fn profile_load_error(&mut self, epoch: u64, connected: bool, error: &str) -> bool {
        if connected && error.starts_with("not_found:") {
            self.apply_profile_load(epoch, None);
            true
        } else {
            self.profiles.load_navigation = None;
            self.profiles.message = Some(format!(
                "Could not load profiles: {error}. No default file was created"
            ));
            false
        }
    }
    pub(super) fn select_profile(&mut self, index: Option<usize>) {
        if self.profiles.dirty() {
            self.profiles.message =
                Some("Save or Discard form changes before selecting another profile".into());
            return;
        }
        let draft = match index {
            Some(index) => match self
                .profiles
                .file
                .as_ref()
                .and_then(|file| file.profiles.get(index))
            {
                Some(profile) => profile.clone(),
                None => return,
            },
            None => empty_command(),
        };
        self.profiles.draft = draft.clone();
        self.profiles.baseline = draft;
        self.profiles.mode = index.map_or(Mode::Manual, Mode::Saved);
        self.profiles.changed();
    }
    pub(super) fn new_profile(&mut self) {
        if self.profiles.dirty() {
            self.profiles.message =
                Some("Save or Discard form changes before creating another profile".into());
            return;
        }
        if self.profiles.file.is_none() {
            self.profiles.message = Some("Choose Load first to read or create cedar.tasks.json. Manual commands do not need a profile file".into());
            return;
        }
        if self.profiles.file.as_ref().unwrap().profiles.len() >= MAX_PROFILES {
            self.profiles.message = Some("The file already contains 32 profiles. Edit one or use the raw editor to remove one".into());
            return;
        }
        self.profiles.mode = Mode::New;
        self.profiles.draft.name.clear();
        self.profiles.baseline = empty_command();
        self.profiles.changed();
        self.profiles.message = Some("New editable profile draft. Enter a name and review the exact executable and arguments before Save or Run".into());
    }
    pub(super) fn discard_profile_form(&mut self) {
        if self.profiles.mode == Mode::New {
            self.profiles.mode = Mode::Manual;
            self.profiles.draft = empty_command();
            self.profiles.baseline = empty_command();
        } else {
            self.profiles.draft = self.profiles.baseline.clone();
        }
        self.profiles.changed();
        self.profiles.message = Some("Discarded only the form edits. The configuration editor buffer and disk file are unchanged".into());
    }
    pub(super) fn review_profile_connection(&mut self) {
        if !self.ready() {
            return;
        }
        if let Some(problem) = self.profile_source_problem(true) {
            self.profiles.message = Some(problem.into());
            return;
        }
        if let Some(source) = &mut self.profiles.source {
            source.generation = self.generation;
        }
        self.profiles.review_required = false;
        self.profiles.message = Some("Retained draft reviewed for this host and root. Save still checks its original disk revision; no command was started".into());
    }
    pub(super) fn save_profile(&mut self) {
        if !self.ready() || self.profiles.mode == Mode::Manual {
            return;
        }
        // Check before encoding or mutating either editor state or form baselines.
        if !self.backend_supports("write") {
            self.profiles.message = Some(self.unsupported_message("write"));
            return;
        }
        if let Some(problem) = self.profile_source_problem(false) {
            self.profiles.message = Some(problem.into());
            return;
        }
        if let Err(error) = validate_profile(&self.profiles.draft) {
            self.profiles.message = Some(error);
            return;
        }
        let Some(mut file) = self.profiles.file.clone() else {
            return;
        };
        let index = match self.profiles.mode {
            Mode::Saved(index) => {
                file.profiles[index] = self.profiles.draft.clone();
                index
            }
            Mode::New => {
                file.profiles.push(self.profiles.draft.clone());
                file.profiles.len() - 1
            }
            Mode::Manual => return,
        };
        let text = match encode_task_file(&file) {
            Ok(text) => text,
            Err(error) => {
                self.profiles.message = Some(error);
                return;
            }
        };
        let id = self.profiles.source.as_ref().unwrap().document;
        let doc = self.documents.iter_mut().find(|doc| doc.id == id).unwrap();
        if doc.text != text {
            editor_state::commit(&self.editor_ctx, doc, text, 0);
        }
        self.profiles.file = Some(file);
        self.profiles.mode = Mode::Saved(index);
        self.profiles.baseline = self.profiles.draft.clone();
        self.profiles.source = self.profile_source(id);
        self.profiles.changed();
        self.profiles.message = Some(
            "Profile committed to the configuration editor. Disk saves use its captured revision"
                .into(),
        );
        self.save_document(id);
    }
    pub(super) fn profile_saved(&mut self, document: u64) {
        if let Some(source) = &mut self.profiles.source {
            if source.document == document {
                if let Some(doc) = self.documents.iter().find(|doc| doc.id == document) {
                    // Only the SHA baseline advances. Never replace a newer form or raw draft.
                    source.revision = doc.revision.clone();
                    self.profiles.message = Some("Saved the submitted configuration. Any newer form or editor changes remain unsaved".into());
                }
            }
        }
    }
    pub(super) fn profile_fields(&mut self, ui: &mut egui::Ui) {
        if self.ready() && !self.backend_supports("write") {
            ui.colored_label(AMBER, self.unsupported_message("write"));
        }
        ui.horizontal_wrapped(|ui| {
            let selected = match self.profiles.mode {
                Mode::Manual => "Manual command".to_owned(),
                Mode::New => "New profile (unsaved)".to_owned(),
                Mode::Saved(_) => self.profiles.baseline.name.clone(),
            };
            let mut pick = None;
            egui::ComboBox::from_id_salt("task_profile_picker")
                .selected_text(selected)
                .width(180.0)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(self.profiles.mode == Mode::Manual, "Manual command")
                        .clicked()
                    {
                        pick = Some(None);
                    }
                    if let Some(file) = &self.profiles.file {
                        for (index, profile) in file.profiles.iter().enumerate() {
                            if ui
                                .selectable_label(
                                    self.profiles.mode == Mode::Saved(index),
                                    &profile.name,
                                )
                                .clicked()
                            {
                                pick = Some(Some(index));
                            }
                        }
                    }
                });
            if let Some(index) = pick {
                self.queue_profile_action(Action::Select(index));
            }
            if ui
                .add_enabled(
                    self.ready() && !self.profile_load_pending(),
                    egui::Button::new("Load"),
                )
                .on_hover_text("Read cedar.tasks.json, or load its current open editor buffer")
                .clicked()
            {
                self.queue_profile_action(Action::Load);
            }
            if ui
                .add_enabled(
                    self.profiles.file.is_some(),
                    egui::Button::new("New profile"),
                )
                .clicked()
            {
                self.queue_profile_action(Action::New);
            }
            if ui
                .add_enabled(
                    self.backend_supports("write")
                        && self.profiles.mode != Mode::Manual
                        && self.profile_source_problem(false).is_none(),
                    egui::Button::new("Save profile"),
                )
                .clicked()
            {
                self.queue_profile_action(Action::Save);
            }
            if ui
                .add_enabled(
                    self.profiles.dirty(),
                    egui::Button::new("Discard form changes"),
                )
                .clicked()
            {
                self.queue_profile_action(Action::Discard);
            }
            if ui
                .add_enabled(
                    self.profiles.source.is_some(),
                    egui::Button::new("Raw editor"),
                )
                .clicked()
            {
                self.queue_profile_action(Action::OpenRaw);
            }
        });
        let mut changed = false;
        if self.profiles.mode != Mode::Manual {
            ui.horizontal(|ui| {
                ui.label("Name");
                changed |= ui
                    .add(
                        egui::TextEdit::singleline(&mut self.profiles.draft.name)
                            .id(egui::Id::new("task_profile_name"))
                            .desired_width(f32::INFINITY),
                    )
                    .changed();
            });
        }
        let windows_agent = self
            .agent_info
            .as_ref()
            .is_some_and(|agent| agent.os == "windows");
        ui.horizontal(|ui| {
            ui.label("Executable");
            changed |= ui
                .add(
                    egui::TextEdit::singleline(&mut self.profiles.draft.program)
                        .id(egui::Id::new("task_program"))
                        .hint_text(if windows_agent {
                            r"C:\Tools\cargo.exe"
                        } else {
                            "cargo"
                        })
                        .desired_width((ui.available_width() - 110.0).max(100.0)),
                )
                .changed();
            changed |= ui
                .add(
                    egui::DragValue::new(&mut self.profiles.draft.timeout_secs)
                        .range(1..=300)
                        .suffix(" s"),
                )
                .changed();
        });
        if windows_agent {
            ui.label(
                RichText::new("Windows: use an absolute path to a native .exe; PATH lookup and batch files are unavailable")
                    .small()
                    .color(MUTED),
            );
        }
        ui.label(
            RichText::new("Literal arguments, in order (empty rows are empty arguments)")
                .small()
                .color(MUTED),
        );
        let mut remove = None;
        for (index, arg) in self.profiles.draft.args.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.label(format!("{}", index + 1));
                changed |= ui
                    .add(
                        egui::TextEdit::singleline(arg)
                            .id_salt(("task_argument", index))
                            .desired_width((ui.available_width() - 78.0).max(80.0)),
                    )
                    .changed();
                if ui.small_button("Remove").clicked() {
                    remove = Some(index);
                }
            });
        }
        if let Some(index) = remove {
            self.profiles.draft.args.remove(index);
            changed = true;
        }
        if ui
            .add_enabled(
                self.profiles.draft.args.len() < cedar_tasks::MAX_ARGUMENTS,
                egui::Button::new("Add argument"),
            )
            .clicked()
        {
            self.profiles.draft.args.push(String::new());
            changed = true;
        }
        if changed {
            self.profiles.changed();
        }
        let workspace = self
            .active_form
            .as_ref()
            .map(|form| recovery_ui::identity(form, &self.root));
        ui.label(
            RichText::new(format!(
                "Workspace: {}",
                match workspace {
                    Some(WorkspaceIdentity::Local { root }) => format!("Local · {root}"),
                    Some(WorkspaceIdentity::Ssh {
                        host, port, root, ..
                    }) => format!("SSH · {host}:{port} · {root}"),
                    None => "not connected".into(),
                }
            ))
            .small(),
        );
        let saved_state = if self.profiles.dirty() {
            "Unsaved form changes"
        } else if let Some(source) = &self.profiles.source {
            match self.documents.iter().find(|doc| doc.id == source.document) {
                Some(doc) if doc.saving => "Saving configuration…",
                Some(doc) if doc.dirty() => "Unsaved configuration editor buffer",
                Some(_) => "Configuration saved",
                None => "Configuration tab closed",
            }
        } else {
            "Manual command; no profile file required"
        };
        ui.label(RichText::new(saved_state).small().color(MUTED));
        // Debug escaping exposes whitespace/control characters and empty strings without shell parsing.
        ui.label(
            RichText::new(format!(
                "Executable: {:?}\nargv: {:?}",
                self.profiles.draft.program, self.profiles.draft.args
            ))
            .monospace()
            .small(),
        );
        ui.label(RichText::new("Project profiles are plaintext. Do not put secrets in arguments. Commands can run repository code and download dependencies.").small().color(AMBER));
        if self.profiles.review_required {
            ui.colored_label(
                AMBER,
                "Connection changed. Review this retained command and workspace before using it",
            );
            if ui
                .add_enabled(
                    self.ready(),
                    egui::Button::new("Review retained draft for this connection"),
                )
                .clicked()
            {
                self.queue_profile_action(Action::Review);
            }
        }
        if self.profiles.mode != Mode::Manual {
            if let Some(problem) = self.profile_source_problem(false) {
                ui.colored_label(AMBER, problem);
            }
        }
        if let Some(message) = &self.profiles.message {
            ui.label(RichText::new(message).small().color(MUTED));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{worker, ConnectForm, ConnectionState, Event};
    use cedar_protocol::Payload;
    use std::sync::mpsc::Receiver;

    fn sample() -> TaskFile {
        TaskFile {
            version: 1,
            profiles: vec![TaskProfile {
                name: "Build".into(),
                program: " cargo ".into(),
                args: vec![
                    "".into(),
                    "  literal space  ".into(),
                    "你好".into(),
                    "$(touch sentinel); * | >".into(),
                ],
                timeout_secs: 30,
            }],
        }
    }
    fn connected() -> (CedarApp, Receiver<worker::Command>) {
        let mut app = CedarApp::empty();
        let form = ConnectForm {
            local_root: "/project".into(),
            allow_run: true,
            ..Default::default()
        };
        app.workspace_key = Some(form.key());
        app.root = "/project".into();
        app.active_form = Some(form);
        app.state = ConnectionState::Ready;
        app.agent_info = Some(crate::agent_support::full_test_agent());
        app.generation = 7;
        app.profiles.connected(app.recovery_workspace().unwrap());
        let (worker, rx) = worker::Worker::recording();
        app.worker = Some(worker);
        (app, rx)
    }
    fn respond(app: &mut CedarApp, command: &worker::Command, payload: Payload) {
        app.apply_event(Event {
            generation: app.generation,
            id: command.id,
            connected: true,
            result: Ok(payload),
        });
    }
    fn loaded() -> (CedarApp, Receiver<worker::Command>) {
        let (mut app, rx) = connected();
        app.load_profiles();
        let command = rx.try_recv().unwrap();
        assert!(matches!(command.op, Operation::Read { .. }));
        respond(
            &mut app,
            &command,
            Payload::File {
                path: PATH.into(),
                text: encode_task_file(&sample()).unwrap(),
                revision: "sha0".into(),
            },
        );
        app.select_profile(Some(0));
        assert!(rx.try_recv().is_err());
        (app, rx)
    }
    #[test]
    fn read_only_profile_save_and_queued_save_have_zero_editor_or_form_mutation() {
        for queued in [false, true] {
            let (mut app, rx) = loaded();
            app.profiles.draft.args.push("unsaved form change".into());
            app.profiles.changed();
            let text = app.documents[0].text.clone();
            let saved_text = app.documents[0].saved_text.clone();
            let revision = app.documents[0].revision.clone();
            let edit_version = app.documents[0].edit_version;
            let file = encode_task_file(app.profiles.file.as_ref().unwrap()).unwrap();
            let baseline = app.profiles.baseline.clone();
            let mode = app.profiles.mode;
            let epoch = app.profiles.epoch;
            let source = format!("{:?}", app.profiles.source);
            let next_request = app.next_request;
            if queued {
                app.queue_profile_action(Action::Save);
            }
            app.agent_info
                .as_mut()
                .unwrap()
                .capabilities
                .retain(|name| name != "write");
            if queued {
                app.finish_profile_actions();
            } else {
                app.save_profile();
            }
            assert_eq!(app.documents[0].text, text);
            assert_eq!(app.documents[0].saved_text, saved_text);
            assert_eq!(app.documents[0].revision, revision);
            assert_eq!(app.documents[0].edit_version, edit_version);
            assert!(!app.documents[0].saving);
            assert_eq!(
                encode_task_file(app.profiles.file.as_ref().unwrap()).unwrap(),
                file
            );
            assert_eq!(app.profiles.baseline, baseline);
            assert_eq!(app.profiles.mode, mode);
            assert_eq!(app.profiles.epoch, epoch);
            assert_eq!(format!("{:?}", app.profiles.source), source);
            assert!(app.profiles.dirty());
            assert_eq!(app.next_request, next_request);
            assert!(rx.try_recv().is_err());
            assert!(app.profiles.message.as_ref().unwrap().contains("write"));
        }
    }
    #[test]
    fn queued_run_checks_capabilities_and_trust_after_form_frame() {
        let (mut app, rx) = loaded();
        app.queue_profile_action(Action::Run);
        app.agent_info
            .as_mut()
            .unwrap()
            .capabilities
            .retain(|name| name != "run_cancel");
        app.finish_profile_actions();
        assert!(rx.try_recv().is_err());
        assert!(app.run_state.snapshot.is_none());
        assert!(app.run_state.output.is_empty());
    }
    #[test]
    fn load_select_new_and_save_never_run_and_save_uses_document_id() {
        let (mut app, rx) = loaded();
        app.documents.push(Document::new(
            99,
            "other.rs".into(),
            "untouched".into(),
            "other-sha".into(),
        ));
        app.active_document = Some(99);
        app.new_profile();
        app.profiles.draft.name = "Test".into();
        app.profiles.draft.program = "cargo".into();
        app.profiles.draft.args = vec!["test".into()];
        app.profiles.changed();
        assert!(app.dirty());
        app.save_profile();
        let command = rx.try_recv().unwrap();
        let Operation::Write {
            path,
            text,
            expected_revision,
        } = &command.op
        else {
            panic!("Only a save is allowed")
        };
        assert_eq!(path, PATH);
        assert_eq!(expected_revision.as_deref(), Some("sha0"));
        let file = parse_task_file(text).unwrap();
        assert_eq!(file.profiles.len(), 2);
        assert_eq!(file.profiles[0], sample().profiles[0]);
        assert_eq!(app.active_document, Some(99));
        assert_eq!(app.documents[1].text, "untouched");
        assert!(rx.try_recv().is_err());
        let ctx = app.editor_ctx.clone();
        let doc = &mut app.documents[0];
        let state = editor_state::load(&ctx, doc);
        let previous = state
            .undoer()
            .undo(&(state.cursor.char_range().unwrap(), doc.text.clone()))
            .unwrap()
            .clone();
        assert_eq!(
            previous.1,
            encode_task_file(&sample()).unwrap(),
            "one undo restores whole config"
        );
    }
    #[test]
    fn source_conflict_preserves_raw_and_form_and_blocks_run_save() {
        let (mut app, rx) = loaded();
        app.profiles.draft.args.push("form draft".into());
        app.profiles.changed();
        app.documents[0].text.push_str(" \n");
        app.documents[0].edit_version += 1;
        let raw = app.documents[0].text.clone();
        let form = app.profiles.draft.clone();
        app.save_profile();
        app.run();
        app.load_profiles();
        assert_eq!(app.documents[0].text, raw);
        assert_eq!(app.profiles.draft, form);
        assert!(rx.try_recv().is_err());
        app.discard_profile_form();
        app.load_profiles();
        assert!(app.profile_run_problem().is_none());
        assert_eq!(app.documents[0].text, raw);
    }
    #[test]
    fn delayed_save_preserves_new_form_and_raw_typing_and_original_submitted_baseline() {
        let (mut app, rx) = loaded();
        app.profiles.draft.args.push("submitted".into());
        app.profiles.changed();
        app.save_profile();
        let command = rx.try_recv().unwrap();
        let submitted = app.documents[0].text.clone();
        app.profiles.draft.args.push("newer form".into());
        app.profiles.changed();
        app.documents[0].text.push_str(" \n");
        app.documents[0].edit_version += 1;
        let raw = app.documents[0].text.clone();
        respond(
            &mut app,
            &command,
            Payload::Written {
                revision: "sha1".into(),
            },
        );
        assert_eq!(app.documents[0].text, raw);
        assert_eq!(app.documents[0].saved_text, submitted);
        assert_eq!(app.documents[0].revision.as_deref(), Some("sha1"));
        assert_eq!(app.profiles.draft.args.last().unwrap(), "newer form");
        assert!(app.profiles.dirty());
        assert!(app.documents[0].dirty());
        assert!(app
            .profile_source_problem(false)
            .unwrap()
            .contains("diverged"));
        assert!(matches!(rx.try_recv().unwrap().op, Operation::List { .. }));
        assert!(rx.try_recv().is_err());
    }
    #[test]
    fn delayed_save_with_form_typing_can_save_again_against_new_revision() {
        let (mut app, rx) = loaded();
        app.profiles.draft.args.push("submitted".into());
        app.profiles.changed();
        app.save_profile();
        let command = rx.try_recv().unwrap();
        app.profiles.draft.args.push("later".into());
        app.profiles.changed();
        respond(
            &mut app,
            &command,
            Payload::Written {
                revision: "sha1".into(),
            },
        );
        rx.try_recv().unwrap();
        app.save_profile();
        let command = rx.try_recv().unwrap();
        let Operation::Write {
            text,
            expected_revision,
            ..
        } = command.op
        else {
            panic!()
        };
        assert_eq!(expected_revision.as_deref(), Some("sha1"));
        assert_eq!(
            parse_task_file(&text).unwrap().profiles[0]
                .args
                .last()
                .unwrap(),
            "later"
        );
    }
    #[test]
    fn late_load_preserves_typing_and_uses_concurrently_opened_raw_buffer() {
        let (mut app, rx) = connected();
        app.load_profiles();
        let command = rx.try_recv().unwrap();
        app.profiles.draft.program = "my newer manual command".into();
        app.profiles.changed();
        let mut raw = Document::new(
            40,
            PATH.into(),
            encode_task_file(&sample()).unwrap(),
            "newsha".into(),
        );
        raw.text.push_str(" \n");
        raw.edit_version += 1;
        let expected = raw.text.clone();
        app.documents.push(raw);
        respond(
            &mut app,
            &command,
            Payload::File {
                path: PATH.into(),
                text: "stale".into(),
                revision: "oldsha".into(),
            },
        );
        assert_eq!(app.documents.len(), 1);
        assert_eq!(app.documents[0].text, expected);
        assert_eq!(app.profiles.draft.program, "my newer manual command");
        assert!(app.profiles.file.is_none());
        app.load_profiles();
        assert_eq!(app.profiles.source.as_ref().unwrap().document, 40);
        assert_eq!(
            app.profiles.source.as_ref().unwrap().revision.as_deref(),
            Some("newsha")
        );
    }
    #[test]
    fn only_not_found_creates_unsaved_config_and_new_file_uses_no_revision() {
        for error in [
            "permission_denied: no",
            "file_too_large: no",
            "invalid_utf8: no",
            "io_error: no",
        ] {
            let (mut app, rx) = connected();
            app.load_profiles();
            let command = rx.try_recv().unwrap();
            app.apply_event(Event {
                generation: app.generation,
                id: command.id,
                connected: true,
                result: Err(error.into()),
            });
            assert!(app.documents.is_empty());
            assert!(app.profiles.file.is_none());
        }
        let (mut app, rx) = connected();
        app.load_profiles();
        let command = rx.try_recv().unwrap();
        app.apply_event(Event {
            generation: app.generation,
            id: command.id,
            connected: true,
            result: Err("not_found: absent".into()),
        });
        assert!(app.documents[0].revision.is_none());
        assert!(app.documents[0].dirty());
        app.new_profile();
        app.profiles.draft = sample().profiles.remove(0);
        app.profiles.changed();
        app.save_profile();
        assert!(matches!(
            rx.try_recv().unwrap().op,
            Operation::Write {
                expected_revision: None,
                ..
            }
        ));
    }
    #[test]
    fn initial_profile_load_selects_configuration_with_no_existing_editor() {
        for missing in [false, true] {
            let (mut app, rx) = connected();
            assert!(app.documents.is_empty());
            assert!(app.active_document.is_none());
            app.load_profiles();
            let command = rx.try_recv().unwrap();
            if missing {
                app.apply_event(Event {
                    generation: app.generation,
                    id: command.id,
                    connected: true,
                    result: Err("not_found: absent".into()),
                });
            } else {
                respond(
                    &mut app,
                    &command,
                    Payload::File {
                        path: PATH.into(),
                        text: encode_task_file(&sample()).unwrap(),
                        revision: "sha0".into(),
                    },
                );
            }
            assert_eq!(app.active_document, Some(app.documents[0].id));
            assert_eq!(app.active().unwrap().path, PATH);
            assert!(app.profiles.file.is_some());
            assert!(rx.try_recv().is_err());
        }
    }
    #[test]
    fn delayed_valid_load_never_selects_after_newer_navigation_even_if_editor_is_empty() {
        for keep_other_open in [false, true] {
            let (mut app, rx) = connected();
            app.load_profiles();
            let command = rx.try_recv().unwrap();
            app.navigation_changed();
            if keep_other_open {
                app.documents.push(Document::new(
                    90,
                    "other.rs".into(),
                    "keep editing".into(),
                    "other-sha".into(),
                ));
                app.active_document = Some(90);
            }
            let active_before = app.active_document;
            respond(
                &mut app,
                &command,
                Payload::File {
                    path: PATH.into(),
                    text: encode_task_file(&sample()).unwrap(),
                    revision: "sha0".into(),
                },
            );
            assert_eq!(app.active_document, active_before);
            assert!(app.documents.iter().any(|doc| doc.path == PATH));
            assert!(app.profiles.file.is_some());
            assert!(rx.try_recv().is_err());
        }
    }
    #[test]
    fn delayed_invalid_load_does_not_steal_focus_from_newer_navigation() {
        let (mut app, rx) = connected();
        app.load_profiles();
        let command = rx.try_recv().unwrap();
        app.documents.push(Document::new(
            90,
            "other.rs".into(),
            "keep editing".into(),
            "other-sha".into(),
        ));
        app.active_document = Some(90);
        app.navigation_changed();
        respond(
            &mut app,
            &command,
            Payload::File {
                path: PATH.into(),
                text: "invalid".into(),
                revision: "sha0".into(),
            },
        );
        assert_eq!(app.active_document, Some(90));
        assert_eq!(app.documents[0].text, "keep editing");
        assert_eq!(app.documents[1].text, "invalid");
        assert!(app.profiles.message.as_ref().unwrap().contains("Invalid"));
    }
    #[test]
    fn malformed_oversized_and_unsupported_configs_remain_inspectable() {
        for text in [
            "{malformed".into(),
            " ".repeat(MAX_TASK_FILE_BYTES + 1),
            "{\"version\":2,\"profiles\":[]}".into(),
        ] {
            let (mut app, rx) = connected();
            app.load_profiles();
            let command = rx.try_recv().unwrap();
            respond(
                &mut app,
                &command,
                Payload::File {
                    path: PATH.into(),
                    text: text.clone(),
                    revision: "sha0".into(),
                },
            );
            assert_eq!(app.documents[0].text, text);
            assert_eq!(app.active_document, Some(app.documents[0].id));
            assert!(app.profiles.file.is_none());
            assert!(app.profiles.message.as_ref().unwrap().contains("Invalid"));
            assert!(rx.try_recv().is_err());
        }
    }
    #[test]
    fn reconnect_requires_review_and_different_identity_never_inherits_profiles() {
        let (mut app, rx) = loaded();
        app.profiles.draft.args.push("retained unsaved".into());
        app.profiles.changed();
        let draft = app.profiles.draft.clone();
        app.profiles.disconnected();
        app.generation += 1;
        app.profiles.connected(app.recovery_workspace().unwrap());
        assert_eq!(app.profiles.draft, draft);
        assert!(app.profile_run_problem().is_some());
        app.run();
        assert!(rx.try_recv().is_err());
        app.review_profile_connection();
        assert!(app.profile_run_problem().is_none());
        assert_eq!(app.profiles.draft, draft);
        app.profiles.connected(WorkspaceIdentity::Ssh {
            host: "other".into(),
            port: 22,
            root: "/project".into(),
            agent_path: "agent".into(),
        });
        assert!(app.profiles.file.is_none());
        assert!(app.profiles.draft.program.is_empty());
    }
    #[test]
    fn form_edits_guard_workspace_switch_tab_close_and_final_close_snapshot() {
        let (mut app, _rx) = loaded();
        app.profiles.draft.args.push("unsaved".into());
        app.profiles.changed();
        assert!(!app.documents[0].dirty());
        assert!(app.dirty());
        let generation = app.generation;
        app.connect(
            &egui::Context::default(),
            ConnectForm {
                local_root: "/different".into(),
                ..Default::default()
            },
        );
        assert_eq!(app.generation, generation);
        app.close_tab(app.documents[0].id);
        assert!(matches!(app.confirm, Some(crate::Confirm::CloseTab(_))));
        app.finish_recovery_close(&egui::Context::default());
        app.profiles.draft.args.push("newer".into());
        app.profiles.changed();
        app.recovery_tick(&egui::Context::default());
        assert!(app.recovery.closing.is_none());
        assert!(!app.allow_close);
        assert!(matches!(app.confirm, Some(crate::Confirm::CloseWindow)));
    }
    #[test]
    fn closed_documents_and_old_generations_cannot_receive_profile_save_or_load() {
        let (mut app, rx) = loaded();
        app.profiles.draft.args.push("submitted".into());
        app.save_profile();
        let command = rx.try_recv().unwrap();
        app.remove_tab(app.documents[0].id);
        respond(
            &mut app,
            &command,
            Payload::Written {
                revision: "ignored".into(),
            },
        );
        assert!(app.profiles.source.is_none());
        assert!(app.documents.is_empty());
        let (mut app, rx) = connected();
        app.load_profiles();
        let command = rx.try_recv().unwrap();
        app.apply_event(Event {
            generation: app.generation - 1,
            id: command.id,
            connected: true,
            result: Ok(Payload::File {
                path: PATH.into(),
                text: encode_task_file(&sample()).unwrap(),
                revision: "old".into(),
            }),
        });
        assert!(app.documents.is_empty());
        assert!(app.profiles.source.is_none());
    }
    #[test]
    fn disconnect_during_load_or_save_retains_drafts_and_ignores_late_results() {
        let (mut app, rx) = connected();
        app.load_profiles();
        let command = rx.try_recv().unwrap();
        app.disconnected("connection lost".into());
        respond(
            &mut app,
            &command,
            Payload::File {
                path: PATH.into(),
                text: encode_task_file(&sample()).unwrap(),
                revision: "late".into(),
            },
        );
        assert!(app.documents.is_empty());
        assert!(app.profiles.file.is_none());
        assert!(app.pending.is_empty());

        let (mut app, rx) = loaded();
        app.profiles.draft.args.push("submitted draft".into());
        app.profiles.changed();
        app.save_profile();
        let command = rx.try_recv().unwrap();
        let document = app.documents[0].text.clone();
        app.profiles.draft.args.push("unsent form edit".into());
        app.profiles.changed();
        app.disconnected("connection lost".into());
        respond(
            &mut app,
            &command,
            Payload::Written {
                revision: "unknown-result-sha".into(),
            },
        );
        assert_eq!(app.documents[0].text, document);
        assert_eq!(app.documents[0].revision.as_deref(), Some("sha0"));
        assert!(!app.documents[0].saving);
        assert!(app.documents[0].dirty());
        assert!(app.profiles.dirty());
        assert_eq!(app.profiles.draft.args.last().unwrap(), "unsent form edit");
        assert!(app.profiles.review_required);
        assert!(app.pending.is_empty());
    }
    #[test]
    fn profile_edit_arriving_during_workspace_handshake_prevents_replacement() {
        let (mut app, _rx) = loaded();
        let original = app.recovery_workspace().unwrap();
        app.connecting_form = Some(ConnectForm {
            local_root: "/different".into(),
            ..Default::default()
        });
        app.state = ConnectionState::Connecting;
        app.profiles.draft.args.push("late edit".into());
        app.profiles.changed();
        app.apply_event(Event {
            generation: app.generation,
            id: 0,
            connected: true,
            result: Ok(Payload::Hello {
                protocol: cedar_protocol::PROTOCOL_VERSION,
                agent: None,
                root: "/different".into(),
            }),
        });
        assert!(app.state == ConnectionState::Disconnected);
        assert_eq!(app.recovery_workspace().unwrap(), original);
        assert_eq!(app.profiles.draft.args.last().unwrap(), "late edit");
        assert_eq!(app.documents.len(), 1);
    }
    #[test]
    fn pretty_print_overflow_preserves_compact_document_form_revision_and_undo_state() {
        let mut file = TaskFile {
            version: 1,
            profiles: (0..4)
                .map(|index| TaskProfile {
                    name: format!("Build {index}"),
                    program: "cargo".into(),
                    args: vec![String::new()],
                    timeout_secs: 30,
                })
                .collect(),
        };
        let overhead = serde_json::to_string(&file).unwrap().len();
        for profile in &mut file.profiles[..3] {
            profile.args[0] = "a".repeat(cedar_tasks::MAX_ARGUMENT_BYTES);
        }
        file.profiles[3].args[0] = "a".repeat(cedar_tasks::MAX_ARGUMENT_BYTES - overhead);
        let compact = serde_json::to_string(&file).unwrap();
        assert_eq!(compact.len(), MAX_TASK_FILE_BYTES);
        assert!(parse_task_file(&compact).is_ok());
        assert!(encode_task_file(&file).is_err());
        let (mut app, rx) = connected();
        app.load_profiles();
        let command = rx.try_recv().unwrap();
        respond(
            &mut app,
            &command,
            Payload::File {
                path: PATH.into(),
                text: compact.clone(),
                revision: "sha0".into(),
            },
        );
        app.select_profile(Some(0));
        app.profiles.draft.program = "rustc".into();
        app.profiles.changed();
        let draft = app.profiles.draft.clone();
        app.save_profile();
        assert_eq!(app.documents[0].text, compact);
        assert_eq!(app.documents[0].saved_text, compact);
        assert_eq!(app.documents[0].revision.as_deref(), Some("sha0"));
        assert_eq!(app.documents[0].edit_version, 0);
        assert!(!app.documents[0].undo_initialized);
        assert!(!app.documents[0].dirty());
        assert_eq!(app.profiles.draft, draft);
        assert_eq!(app.profiles.baseline.program, "cargo");
        assert!(app.profiles.dirty());
        assert!(app.profiles.message.as_ref().unwrap().contains("262144"));
        assert!(
            rx.try_recv().is_err(),
            "formatting failure must not dispatch a write or command"
        );
    }
    #[test]
    fn validation_failure_changes_neither_document_nor_saved_form_baseline() {
        let (mut app, rx) = loaded();
        let raw = app.documents[0].text.clone();
        app.profiles.draft.args = vec!["\0".into()];
        app.profiles.changed();
        app.save_profile();
        assert_eq!(app.documents[0].text, raw);
        assert_eq!(app.profiles.baseline, sample().profiles[0]);
        assert!(app.profiles.dirty());
        assert!(rx.try_recv().is_err());
    }
    #[test]
    fn recovered_configuration_keeps_revision_and_trust_off_without_resuming() {
        let (mut app, rx) = connected();
        app.active_form.as_mut().unwrap().allow_run = false;
        let text = encode_task_file(&sample()).unwrap();
        app.install_recovered(cedar_recovery::Draft {
            workspace: app.recovery_workspace().unwrap(),
            path: PATH.into(),
            text: text.clone(),
            base_text: String::new(),
            base_revision: Some("recovered-sha".into()),
            modified_ms: 1,
        })
        .unwrap();
        app.load_profiles();
        app.select_profile(Some(0));
        app.run();
        assert!(rx.try_recv().is_err());
        assert!(!app.active_form.as_ref().unwrap().allow_run);
        assert!(app.run_state.snapshot.is_none());
        assert!(app.run_state.output.is_empty());
        assert_eq!(app.documents[0].text, text);
        assert_eq!(app.documents[0].revision.as_deref(), Some("recovered-sha"));
    }
    #[test]
    fn run_dispatch_preserves_executable_and_literal_argv_and_repeated_click_is_one_task() {
        let (mut app, rx) = loaded();
        // A frontend on every platform can send to a Linux SSH backend.
        app.active_form.as_mut().unwrap().ssh = true;
        app.profiles.source.as_mut().unwrap().workspace = app.recovery_workspace().unwrap();
        app.run();
        app.run();
        let command = rx.try_recv().unwrap();
        let Operation::RunStart {
            program,
            args,
            timeout_secs,
        } = command.op
        else {
            panic!()
        };
        assert_eq!(program, " cargo ");
        assert_eq!(args, sample().profiles[0].args);
        assert_eq!(timeout_secs, 30);
        assert!(rx.try_recv().is_err());
        assert!(app.run_state.output.contains("\" cargo \""));
    }
    #[test]
    fn command_support_uses_remote_capabilities_independent_of_frontend_and_transport() {
        for ssh in [false, true] {
            let (mut app, rx) = connected();
            app.active_form.as_mut().unwrap().ssh = ssh;
            app.profiles.draft.program = "cargo".into();
            app.agent_info.as_mut().unwrap().os = "windows".into();
            app.agent_info
                .as_mut()
                .unwrap()
                .capabilities
                .retain(|name| name != "run_cancel");
            app.run();
            assert!(rx.try_recv().is_err());
            app.agent_info.as_mut().unwrap().os = "linux".into();
            app.agent_info
                .as_mut()
                .unwrap()
                .capabilities
                .push("run_cancel".into());
            app.run();
            assert!(matches!(
                rx.try_recv().unwrap().op,
                Operation::RunStart { .. }
            ));
        }
    }
    #[test]
    fn manual_command_does_not_need_loading_or_saving_a_file() {
        let (mut app, rx) = connected();
        app.active_form.as_mut().unwrap().ssh = true;
        app.profiles.draft.program = "cargo".into();
        app.profiles.draft.args = vec!["--version".into()];
        assert!(app.documents.is_empty());
        app.run();
        assert!(matches!(
            rx.try_recv().unwrap().op,
            Operation::RunStart { .. }
        ));
        assert!(app.documents.is_empty());
    }
    fn frame(app: &mut CedarApp, ctx: &egui::Context, time: f64, events: Vec<egui::Event>) {
        let mut native = eframe::Frame::_new_kittest();
        let _ = ctx.run(
            egui::RawInput {
                time: Some(time),
                events,
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1320.0, 880.0),
                )),
                ..Default::default()
            },
            |ctx| eframe::App::update(app, ctx, &mut native),
        );
    }
    fn prepare_editor_frame(app: &mut CedarApp, ctx: &egui::Context) {
        app.editor_ctx = ctx.clone();
        app.open_form = false;
        app.active_document = Some(app.documents[0].id);
        app.tool = crate::Tool::Run;
        app.tools_open = true;
        frame(app, ctx, 0.0, vec![]);
        let doc = &mut app.documents[0];
        doc.jump_to = Some(doc.text.chars().count());
        frame(app, ctx, 1.0, vec![]);
    }
    #[test]
    fn complete_frames_apply_raw_text_and_paste_before_queued_profile_save_or_run() {
        for action in [Action::Save, Action::Run] {
            for paste in [false, true] {
                let (mut app, rx) = loaded();
                let ctx = egui::Context::default();
                prepare_editor_frame(&mut app, &ctx);
                app.profiles.draft.args.push("form stays".into());
                app.profiles.changed();
                let form = app.profiles.draft.clone();
                let raw = app.documents[0].text.clone();
                app.queue_profile_action(action);
                frame(
                    &mut app,
                    &ctx,
                    1.1,
                    vec![if paste {
                        egui::Event::Paste("X".into())
                    } else {
                        egui::Event::Text("X".into())
                    }],
                );
                assert_eq!(app.documents[0].text, format!("{raw}X"));
                assert_eq!(app.profiles.draft, form);
                assert!(app.documents[0].dirty());
                assert!(
                    rx.try_recv().is_err(),
                    "same-frame diverged commands must not save or run"
                );
                assert!(app
                    .profile_source_problem(false)
                    .unwrap()
                    .contains("diverged"));
            }
        }
    }
    #[test]
    fn complete_frame_discard_changes_only_form_and_raw_typing_cancels_close() {
        let (mut app, rx) = loaded();
        let ctx = egui::Context::default();
        prepare_editor_frame(&mut app, &ctx);
        app.profiles.draft.args.push("form discard".into());
        app.profiles.changed();
        let raw = app.documents[0].text.clone();
        app.finish_recovery_close(&ctx);
        app.queue_profile_action(Action::Discard);
        frame(&mut app, &ctx, 1.1, vec![egui::Event::Paste("X".into())]);
        assert_eq!(app.documents[0].text, format!("{raw}X"));
        assert_eq!(app.profiles.draft, sample().profiles[0]);
        assert!(app.recovery.closing.is_none());
        assert!(!app.allow_close);
        assert!(matches!(app.confirm, Some(crate::Confirm::CloseWindow)));
        assert!(rx.try_recv().is_err());
    }
    #[test]
    fn complete_frame_raw_typing_before_close_shortcut_keeps_configuration_open() {
        for paste in [false, true] {
            let (mut app, rx) = loaded();
            let ctx = egui::Context::default();
            prepare_editor_frame(&mut app, &ctx);
            let raw = app.documents[0].text.clone();
            frame(
                &mut app,
                &ctx,
                1.1,
                vec![
                    if paste {
                        egui::Event::Paste("X".into())
                    } else {
                        egui::Event::Text("X".into())
                    },
                    egui::Event::Key {
                        key: egui::Key::W,
                        physical_key: Some(egui::Key::W),
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::COMMAND,
                    },
                    egui::Event::Key {
                        key: egui::Key::W,
                        physical_key: Some(egui::Key::W),
                        pressed: false,
                        repeat: false,
                        modifiers: egui::Modifiers::COMMAND,
                    },
                ],
            );
            assert_eq!(app.documents.len(), 1);
            assert_eq!(app.documents[0].text, format!("{raw}X"));
            assert!(matches!(app.confirm, Some(crate::Confirm::CloseTab(_))));
            assert!(rx.try_recv().is_err());
        }
    }
    #[test]
    fn complete_frame_profile_typing_before_close_shortcut_gets_dirty_confirmation() {
        for paste in [false, true] {
            let (mut app, rx) = loaded();
            let ctx = egui::Context::default();
            prepare_editor_frame(&mut app, &ctx);
            let id = egui::Id::new("task_program");
            let mut state = egui::TextEdit::load_state(&ctx, id).unwrap();
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::one(
                    egui::text::CCursor::new(app.profiles.draft.program.chars().count()),
                )));
            state.store(&ctx, id);
            ctx.memory_mut(|memory| memory.request_focus(id));
            let program = app.profiles.draft.program.clone();
            frame(
                &mut app,
                &ctx,
                1.1,
                vec![
                    if paste {
                        egui::Event::Paste("X".into())
                    } else {
                        egui::Event::Text("X".into())
                    },
                    egui::Event::Key {
                        key: egui::Key::W,
                        physical_key: Some(egui::Key::W),
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::COMMAND,
                    },
                    egui::Event::Key {
                        key: egui::Key::W,
                        physical_key: Some(egui::Key::W),
                        pressed: false,
                        repeat: false,
                        modifiers: egui::Modifiers::COMMAND,
                    },
                ],
            );
            assert_eq!(app.profiles.draft.program, format!("{program}X"));
            assert_eq!(app.documents.len(), 1);
            assert!(app.profiles.dirty());
            assert!(matches!(app.confirm, Some(crate::Confirm::CloseTab(_))));
            assert!(rx.try_recv().is_err());
        }
    }
    #[test]
    fn complete_frame_save_ack_and_new_input_preserve_submitted_baseline() {
        let (mut app, rx) = loaded();
        let ctx = egui::Context::default();
        prepare_editor_frame(&mut app, &ctx);
        app.profiles.draft.args.push("submitted".into());
        app.profiles.changed();
        app.queue_profile_action(Action::Save);
        frame(&mut app, &ctx, 1.1, vec![]);
        let command = rx.try_recv().unwrap();
        let submitted = app.documents[0].text.clone();
        app.documents[0].jump_to = Some(submitted.chars().count());
        frame(&mut app, &ctx, 1.2, vec![]);
        app.result_tx
            .send(Event {
                generation: app.generation,
                id: command.id,
                connected: true,
                result: Ok(Payload::Written {
                    revision: "sha1".into(),
                }),
            })
            .unwrap();
        frame(&mut app, &ctx, 1.3, vec![egui::Event::Text("Y".into())]);
        assert_eq!(app.documents[0].saved_text, submitted);
        assert_eq!(app.documents[0].text, format!("{submitted}Y"));
        assert_eq!(app.documents[0].revision.as_deref(), Some("sha1"));
        assert!(app.documents[0].dirty());
        assert!(app
            .profile_source_problem(false)
            .unwrap()
            .contains("diverged"));
    }
    #[test]
    fn profile_panel_layout_handles_minimum_default_and_many_literal_rows() {
        let (mut app, _rx) = loaded();
        app.profiles.draft.args = vec![" 你好 \n\t $(literal); ".into(); 256];
        let ctx = egui::Context::default();
        for size in [[780.0, 150.0], [1320.0, 500.0]] {
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(size[0], size[1]),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| app.run_panel(ui));
                },
            );
            assert!(!output.shapes.is_empty());
        }
        assert_eq!(app.profiles.draft.args.len(), 256);
        assert_eq!(app.profiles.draft.args[0], " 你好 \n\t $(literal); ");
    }
}
