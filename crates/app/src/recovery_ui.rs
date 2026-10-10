//! Explicit recovery review and workspace identity verification.
#[cfg(test)]
#[path = "ssh_preflight_tests.rs"]
mod ssh_preflight_tests;
use crate::{model::Document, CedarApp, ConnectForm, ConnectionState, AMBER, GREEN, MUTED, RED};
use cedar_recovery::{Draft, WorkspaceIdentity};
use eframe::egui::{self, RichText};

pub fn identity(form: &ConnectForm, root: &str) -> WorkspaceIdentity {
    if form.ssh {
        WorkspaceIdentity::Ssh {
            host: form.host.trim().into(),
            port: form.port.trim().parse().unwrap_or(22),
            root: root.into(),
            agent_path: form.agent.trim().into(),
        }
    } else {
        WorkspaceIdentity::Local { root: root.into() }
    }
}
fn restore_form(workspace: &WorkspaceIdentity) -> ConnectForm {
    match workspace {
        WorkspaceIdentity::Local { root } => ConnectForm {
            local_root: root.clone(),
            allow_run: false,
            ..Default::default()
        },
        WorkspaceIdentity::Ssh {
            host,
            port,
            root,
            agent_path,
        } => ConnectForm {
            ssh: true,
            host: host.clone(),
            port: port.to_string(),
            remote_root: root.clone(),
            agent: agent_path.clone(),
            allow_run: false,
            ..Default::default()
        },
    }
}
fn project(workspace: &WorkspaceIdentity) -> String {
    match workspace {
        WorkspaceIdentity::Local { root } => format!("Local · {root}"),
        WorkspaceIdentity::Ssh {
            host,
            port,
            root,
            agent_path,
        } => format!("SSH · {host}:{port} · {root}\nAgent: {agent_path}"),
    }
}
// UTC calendar conversion keeps timestamps readable without runtime dependencies.
fn time_label(ms: u64) -> String {
    let seconds = ms / 1000;
    let days = (seconds / 86400) as i64;
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02} UTC",
        seconds % 86400 / 3600,
        seconds % 3600 / 60,
        seconds % 60
    )
}

impl CedarApp {
    pub(crate) fn recovery_workspace(&self) -> Option<WorkspaceIdentity> {
        self.active_form
            .as_ref()
            .filter(|_| !self.root.is_empty())
            .map(|form| identity(form, &self.root))
    }
    pub(crate) fn recovery_tick(&mut self, ctx: &egui::Context) {
        if let Some(draft) = self.recovery.poll() {
            self.recovery.pending_restore = Some(draft);
            self.recovery.visible = true;
        }
        if let Some(snapshot) = &self.recovery.closing {
            let current = self.draft_versions();
            if *snapshot != current {
                self.recovery.closing = None;
                self.allow_close = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.confirm = Some(crate::Confirm::CloseWindow);
                self.notice =
                    "A draft changed while recovery was finishing; confirm before quitting".into();
            }
        }
        if let Some(workspace) = self.recovery_workspace() {
            for doc in &self.documents {
                self.recovery.observe(&workspace, doc);
            }
        }
        let live: Vec<_> = self.documents.iter().map(|doc| doc.id).collect();
        self.recovery.forget_completed(&live);
    }
    pub(crate) fn finish_recovery_close(&mut self, ctx: &egui::Context) {
        if self.recovery.has_store() {
            if let Some(workspace) = self.recovery_workspace() {
                for doc in &self.documents {
                    self.recovery.discard_owned(&workspace, doc);
                }
            }
            self.recovery.flush();
        }
        self.recovery.closing = Some(self.draft_versions());
        ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        self.notice = "Finishing explicit recovery discards before quitting".into();
    }
    /// Commit Close only after every UI input handler and final draft observation.
    pub(crate) fn finish_recovery_close_frame(&mut self, ctx: &egui::Context) {
        if self.recovery.closing.is_none() {
            return;
        }
        if self.mutation_pending()
            || self.language.running
            || !self.guard_run_transition(crate::run_ui::Transition::Close)
        {
            self.recovery.closing = None;
            self.allow_close = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.notice = "A save or tool request started while closing. Finish it and retry close; your drafts remain open".into();
            return;
        }
        if self.recovery.removals_finished() {
            self.allow_close = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
    pub(crate) fn install_recovered(&mut self, draft: Draft) -> Result<(), String> {
        if self.save_all_busy() {
            return Err("Cancel remaining Save All writes and wait for the in-flight save before restoring a draft".into());
        }
        if self.recovery_workspace().as_ref() != Some(&draft.workspace) || !self.ready() {
            return Err("The connected workspace does not match this recovery. The saved copy has been retained".into());
        }
        if self.active_form.as_ref().is_some_and(|form| form.allow_run) {
            return Err("Reconnect with workspace trust off before restoring this draft".into());
        }
        if self.documents.iter().any(|doc| doc.path == draft.path) {
            return Err("This file already has an open tab. Save or copy its work, then explicitly close it before restoring".into());
        }
        if self.documents.len() >= 32 {
            return Err("Close a tab before restoring another draft (32-buffer limit)".into());
        }
        let id = self.next_document;
        self.next_document += 1;
        let mut doc = Document::new(
            id,
            draft.path.clone(),
            draft.base_text,
            draft.base_revision.clone().unwrap_or_default(),
        );
        doc.revision = draft.base_revision;
        doc.has_cjk |= crate::system_fonts::contains_cjk(&draft.text);
        doc.text = draft.text;
        self.recovery.authorize(&draft.workspace, &doc);
        self.recovery.observe(&draft.workspace, &doc);
        self.documents.push(doc);
        self.navigation_changed();
        self.active_document = Some(id);
        self.open_form = false;
        self.recovery.visible = false;
        self.notice = format!(
            "Restored {} locally. Review it before saving; the original base revision is retained",
            draft.path
        );
        Ok(())
    }
    fn begin_restore(&mut self, ctx: &egui::Context) {
        if self.save_all_busy() {
            self.recovery.error = Some("Cancel remaining Save All writes and wait for the in-flight save before restoring a draft".into());
            return;
        }
        if self.state == ConnectionState::Disconnecting {
            self.recovery.error = Some(crate::disconnect::WAITING.into());
            return;
        }
        let Some(draft) = &self.recovery.pending_restore else {
            return;
        };
        if self.documents.iter().any(|doc| doc.path == draft.path)
            && self.recovery_workspace().as_ref() == Some(&draft.workspace)
        {
            self.recovery.error = Some("Close the existing tab before restoring. Current work will never be replaced by recovery".into());
            return;
        }
        if self.ready()
            && self.recovery_workspace().as_ref() == Some(&draft.workspace)
            && self
                .active_form
                .as_ref()
                .is_some_and(|form| !form.allow_run)
        {
            let draft = self.recovery.pending_restore.take().unwrap();
            if let Err(error) = self.install_recovered(draft.clone()) {
                self.recovery.error = Some(error);
                self.recovery.pending_restore = Some(draft);
            }
            return;
        }
        let form = restore_form(&draft.workspace);
        self.connect(ctx, form);
        if self.state == ConnectionState::Connecting {
            self.recovery.restoring_generation = Some(self.generation);
        }
    }
    pub(crate) fn recovery_window(&mut self, ctx: &egui::Context) {
        if !self.recovery.visible {
            return;
        }
        let mut visible = true;
        egui::Window::new("Private draft recovery").open(&mut visible).default_width(700.0).default_height(430.0).resizable(true).show(ctx, |ui| {
            ui.label("Unsaved text, including SSH drafts, is copied only to this frontend computer. Workspace files change only when you choose Save.");
            ui.label(RichText::new(if cfg!(windows) {
                "Local recovery is on by default. Windows folder permissions are inherited: protect the recovery folder's access. Copies are not encrypted; draft text may contain secrets. No login credentials or tool-trust settings are saved."
            } else {
                "Local recovery is on by default. Copies use private owner-only permissions and are not encrypted. Draft text may itself contain secrets. No login credentials or tool-trust settings are saved."
            }).small().color(MUTED));
            let mut enabled = self.recovery.enabled;
            if ui.checkbox(&mut enabled, "Create local recovery copies for this session").changed() { self.recovery.set_enabled(enabled); }
            if !enabled { ui.colored_label(AMBER, "New recovery copies are off for this session. Existing copies remain; a write already in progress may finish."); }
            ui.label(RichText::new(format!("Location: {}", self.recovery.location())).small().color(MUTED));
            ui.horizontal(|ui| {
                if ui.button("Refresh").clicked() { self.recovery.refresh(false); }
                if ui.button("Retry recovery").clicked() { self.recovery.retry(ctx); }
                if self.recovery.loading || self.recovery.reading.is_some() { ui.spinner(); }
            });
            if let Some(error) = &self.recovery.error { ui.colored_label(RED, error); }
            for (path, error) in self.recovery.failures() { ui.colored_label(RED, format!("{path}: {error}")); }
            for (name, issue) in &self.recovery.issues { ui.colored_label(AMBER, format!("Unreadable copy {name}: {issue}. The file was retained; inspect the recovery folder.")); }
            if self.recovery.closing.is_some() {
                ui.colored_label(AMBER, "Waiting for recovery discards. Retry errors or keep editing.");
                if ui.button("Keep editing").clicked() { self.recovery.closing = None; }
            }
            ui.separator();
            ui.label(RichText::new("AVAILABLE COPIES").strong().color(GREEN));
            ui.label(RichText::new("Restore and Remove are explicit choices. An older copy blocks new recovery for that same file until reviewed. Copies are never pruned to make space.").small().color(MUTED));
            let mut restore = None;
            let mut remove = None;
            egui::ScrollArea::vertical().id_salt("recovery_copies").max_height(280.0).show(ui, |ui| {
                if self.recovery.drafts.is_empty() && self.recovery.initialized { ui.label("No recovery copies available"); }
                for draft in &self.recovery.drafts {
                    ui.group(|ui| {
                        ui.label(RichText::new(&draft.path).strong());
                        ui.label(project(&draft.workspace));
                        ui.label(RichText::new(format!("{} · {} bytes", time_label(draft.modified_ms), draft.text_bytes)).small().color(MUTED));
                        ui.horizontal(|ui| {
                            if ui.add_enabled(self.recovery.reading.is_none() && self.recovery.restoring_generation.is_none(), egui::Button::new("Review restore")).clicked() { restore = Some(draft.id.clone()); }
                            if ui.button("Remove copy").clicked() { remove = Some(draft.id.clone()); }
                        });
                    });
                }
            });
            if let Some(id) = restore { self.recovery.request_restore(id); }
            if let Some(id) = remove { self.recovery.remove_confirmation = Some(id); }
            if let Some(draft) = &self.recovery.pending_restore {
                ui.separator();
                ui.label(RichText::new(format!("Restore {}", draft.path)).strong());
                ui.label(project(&draft.workspace));
                ui.label("This explicitly connects to the displayed workspace if needed, with trust OFF. The agent root must match. The draft will stay unsaved with its original base revision; external changes produce a normal save conflict.");
                ui.horizontal(|ui| {
                    if ui.add_enabled(!matches!(self.state, ConnectionState::Connecting | ConnectionState::Disconnecting) && !self.mutation_pending(), egui::Button::new("Connect with trust off and restore")).clicked() { self.begin_restore(ctx); }
                    if ui.button("Cancel restore").clicked() {
                        if self.recovery.restoring_generation == Some(self.generation) {
                            self.worker = None; self.generation += 1; self.connecting_form = None;
                            self.state = if self.workspace_key.is_some() { ConnectionState::Disconnected } else { ConnectionState::Idle };
                        }
                        self.recovery.pending_restore = None;
                        self.recovery.restoring_generation = None;
                    }
                });
            }
        });
        self.recovery.visible = visible;
        if let Some(id) = self.recovery.remove_confirmation.clone() {
            egui::Modal::new(egui::Id::new("remove_recovery_copy")).show(ctx, |ui| {
                ui.heading("Remove this recovery copy?");
                let draft = self.recovery.drafts.iter().find(|draft| draft.id == id);
                if let Some(draft) = draft { ui.label(format!("{}\n{}", draft.path, project(&draft.workspace))); }
                ui.label("This deletes only the private recovery copy. If an open draft is still dirty, a new copy may be created while recovery is on.");
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() { self.recovery.remove_confirmation = None; }
                    if ui.button(RichText::new("Remove recovery copy").color(RED)).clicked() {
                        if let Some(draft) = self.recovery.drafts.iter().find(|draft| draft.id == id) {
                            let (workspace, path) = (draft.workspace.clone(), draft.path.clone());
                            self.recovery.remove(workspace, path);
                        }
                        self.recovery.remove_confirmation = None;
                    }
                });
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_disconnect_wait_blocks_restore_before_taking_the_copy() {
        let mut app = CedarApp::empty();
        app.state = ConnectionState::Disconnecting;
        app.generation = 7;
        let draft = Draft {
            workspace: WorkspaceIdentity::Local {
                root: "/synthetic/project".into(),
            },
            path: "draft.txt".into(),
            text: "retained draft".into(),
            base_text: "saved".into(),
            base_revision: Some("revision".into()),
            modified_ms: 1,
        };
        app.recovery.pending_restore = Some(draft.clone());
        app.begin_restore(&egui::Context::default());
        assert!(app.state == ConnectionState::Disconnecting);
        assert_eq!(app.generation, 7);
        assert!(app.worker.is_none());
        assert_eq!(
            app.recovery.pending_restore.as_ref().unwrap().text,
            draft.text
        );
        assert!(app.recovery.restoring_generation.is_none());
        assert_eq!(
            app.recovery.error.as_deref(),
            Some(crate::disconnect::WAITING)
        );
    }

    #[test]
    fn metadata_cannot_inherit_execution_trust() {
        for workspace in [
            WorkspaceIdentity::Local {
                root: "/sample".into(),
            },
            WorkspaceIdentity::Ssh {
                host: "sample.invalid".into(),
                port: 22,
                root: "/project".into(),
                agent_path: "cedar-agent".into(),
            },
        ] {
            let form = restore_form(&workspace);
            assert!(!form.allow_run);
            assert_eq!(
                identity(
                    &form,
                    match &workspace {
                        WorkspaceIdentity::Local { root } | WorkspaceIdentity::Ssh { root, .. } =>
                            root,
                    }
                ),
                workspace
            );
        }
    }
    #[test]
    fn timestamp_labels_use_utc() {
        assert_eq!(time_label(0), "1970-01-01 00:00:00 UTC");
        assert_eq!(time_label(1709164800000), "2024-02-29 00:00:00 UTC");
    }
}
