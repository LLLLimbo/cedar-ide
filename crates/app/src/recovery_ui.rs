//! Explicit recovery review and workspace identity verification.
#[cfg(test)]
#[path = "ssh_preflight_tests.rs"]
mod ssh_preflight_tests;
use crate::{
    model::Document,
    recovery::{CloseGuard, ClosePhase},
    CedarApp, ConnectForm, ConnectionState, AMBER, GREEN, MUTED, RED,
};
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
    pub(crate) fn recovery_close_guard(&self) -> CloseGuard {
        CloseGuard::new(
            &self.documents,
            self.recovery_workspace(),
            self.generation,
            self.profiles.epoch,
        )
    }
    pub(crate) fn keep_editing_recovery(&mut self, ctx: &egui::Context) {
        self.cancel_tab_close();
        self.recovery.keep_editing();
        self.allow_close = false;
        self.confirm = None;
        self.close_after_language_stop = false;
        self.close_snapshot = None;
        self.recovery.language_close_guard = None;
        ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
    }
    pub(crate) fn recovery_tick(&mut self, ctx: &egui::Context) {
        if let Some(draft) = self.recovery.poll() {
            self.recovery.pending_restore = Some(draft);
            self.recovery.visible = true;
        }
        if self
            .recovery
            .closing
            .as_ref()
            .is_some_and(|close| close.guard != self.recovery_close_guard())
        {
            self.keep_editing_recovery(ctx);
            self.confirm = Some(crate::Confirm::CloseWindow);
            self.notice = "A draft or workspace changed while recovery was finishing; confirm before quitting".into();
        }
        if let Some(workspace) = self.recovery_workspace() {
            for doc in &self.documents {
                self.recovery.observe(&workspace, doc);
            }
        }
        let live: Vec<_> = self.documents.iter().map(|doc| doc.id).collect();
        self.recovery.forget_completed(&live);
        if self.recovery.closing.is_some() || self.recovery.resuming() {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
    }
    pub(crate) fn finish_recovery_close(&mut self, ctx: &egui::Context) {
        if self.recovery.closing.is_some() {
            return;
        }
        if self.recovery.resuming() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.notice =
                "Recovery is still resuming. Keep editing and wait before trying to quit again"
                    .into();
            return;
        }
        if let Some(workspace) = self.recovery_workspace() {
            for doc in &self.documents {
                self.recovery.discard_owned(&workspace, doc);
            }
        }
        self.recovery.flush();
        self.recovery.begin_close(self.recovery_close_guard());
        ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        self.notice = "Finishing explicit recovery discards before quitting".into();
    }
    pub(crate) fn retain_recovery_and_quit(&mut self) {
        if self
            .recovery
            .closing
            .as_ref()
            .is_some_and(|close| matches!(close.phase, ClosePhase::NeedsDecision))
        {
            self.recovery.begin_quiescence(true);
        }
    }
    pub(crate) fn confirm_retained_recovery_close(&mut self) {
        let current = self.recovery_close_guard();
        if let Some(close) = &mut self.recovery.closing {
            if close.guard == current {
                if let ClosePhase::AwaitingConfirmation { ticket } = close.phase {
                    close.phase = ClosePhase::Confirmed { ticket };
                }
            }
        }
    }
    /// Commit only after every input handler; a proof does not authorize new text.
    pub(crate) fn finish_recovery_close_frame(&mut self, ctx: &egui::Context) {
        let Some(close) = &self.recovery.closing else {
            return;
        };
        if close.guard != self.recovery_close_guard()
            || self.mutation_pending()
            || self.language.running
            || self.state == ConnectionState::Disconnecting
            || !self.guard_run_transition(crate::run_ui::Transition::Close)
        {
            self.keep_editing_recovery(ctx);
            self.notice = "A draft, save, workspace or tool request changed while closing. Finish it and retry close; your drafts remain open".into();
            return;
        }
        self.recovery.observe_close_deadline();
        if matches!(
            self.recovery.closing.as_ref().map(|close| close.phase),
            Some(ClosePhase::Discarding { .. })
        ) && self.recovery.removals_finished()
        {
            self.recovery.begin_quiescence(false);
        }
        if matches!(
            self.recovery.closing.as_ref().map(|close| close.phase),
            Some(ClosePhase::Confirmed { .. })
        ) {
            self.allow_close = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
    pub(crate) fn install_recovered(&mut self, draft: Draft) -> Result<(), String> {
        if self.save_close_tab_busy() || self.save_close_write_pending() {
            return Err("Finish the tab close or Keep editing and wait for its save before restoring a draft".into());
        }
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
        let id = self.allocate_document_id()?;
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
        if self.save_close_tab_busy() || self.save_close_write_pending() {
            self.recovery.error = Some("Finish the tab close or Keep editing and wait for its save before restoring a draft".into());
            return;
        }
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
                if ui.add_enabled(self.recovery.can_retry(), egui::Button::new("Retry recovery")).clicked() { self.recovery.retry(ctx); }
                if self.recovery.loading || self.recovery.reading.is_some() { ui.spinner(); }
            });
            if let Some(error) = &self.recovery.error { ui.colored_label(RED, error); }
            for (path, error) in self.recovery.failures() { ui.colored_label(RED, format!("{path}: {error}")); }
            for (name, issue) in &self.recovery.issues { ui.colored_label(AMBER, format!("Unreadable copy {name}: {issue}. The file was retained; inspect the recovery folder.")); }
            if let Some(phase) = self.recovery.closing.as_ref().map(|close| close.phase) {
                match phase {
                    ClosePhase::Discarding { .. } => { ui.colored_label(AMBER, "Waiting for recovery discards (up to 5 seconds before review)."); }
                    ClosePhase::NeedsDecision => {
                        ui.colored_label(AMBER, self.recovery.close_decision_message());
                        if ui.button("Quit without deleting remaining recovery copies").clicked() { self.retain_recovery_and_quit(); }
                    }
                    ClosePhase::Draining { .. } => { ui.colored_label(AMBER, "Waiting up to 5 seconds for accepted recovery writes and any operation already running. Queued removals were canceled."); }
                    ClosePhase::AwaitingConfirmation { .. } => { ui.colored_label(AMBER, "Recovery has stopped making changes. Confirm quitting below."); }
                    ClosePhase::Blocked { .. } => { ui.colored_label(RED, "Recovery settlement is unverified. Quitting is blocked. Keep editing; Retry becomes available after the worker settles or has stopped."); }
                    ClosePhase::Confirmed { .. } => {},
                }
                if ui.button("Keep editing").clicked() { self.keep_editing_recovery(ctx); }
            } else if self.recovery.resuming() {
                ui.colored_label(AMBER, "You can keep editing and Save. Recovery admission will resume after the operation already running finishes.");
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
                            if ui.add_enabled(self.recovery.can_retry() && self.recovery.reading.is_none() && self.recovery.restoring_generation.is_none(), egui::Button::new("Review restore")).clicked() { restore = Some(draft.id.clone()); }
                            if ui.add_enabled(self.recovery.can_retry(), egui::Button::new("Remove copy")).clicked() { remove = Some(draft.id.clone()); }
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
        if !visible && self.recovery.closing.is_some() {
            self.keep_editing_recovery(ctx);
        }
        self.recovery.visible = visible;
        if matches!(
            self.recovery.closing.as_ref().map(|close| close.phase),
            Some(ClosePhase::AwaitingConfirmation { .. })
        ) {
            egui::Modal::new(egui::Id::new("retain_recovery_quit_confirmation")).show(ctx, |ui| {
                ui.set_max_width(480.0);
                ui.heading("Quit and discard current unsaved changes?");
                ui.label("Current unsaved editor text and profile form edits may be lost. Remaining recovery copies may be older or incomplete and are not proof that your current text is recoverable. Save or copy your draft before quitting if you need it.");
                ui.horizontal(|ui| {
                    if ui.button("Keep editing").clicked() { self.keep_editing_recovery(ctx); }
                    if ui.button(RichText::new("Quit without deleting remaining recovery copies").color(RED)).clicked() {
                        self.confirm_retained_recovery_close();
                    }
                });
            });
        }
        if let Some(id) = self.recovery.remove_confirmation.clone() {
            egui::Modal::new(egui::Id::new("remove_recovery_copy")).show(ctx, |ui| {
                ui.heading("Remove this recovery copy?");
                let draft = self.recovery.drafts.iter().find(|draft| draft.id == id);
                if let Some(draft) = draft { ui.label(format!("{}\n{}", draft.path, project(&draft.workspace))); }
                ui.label("This deletes only the private recovery copy. If an open draft is still dirty, a new copy may be created while recovery is on.");
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() { self.recovery.remove_confirmation = None; }
                    if ui.add_enabled(self.recovery.can_retry(), egui::Button::new(RichText::new("Remove recovery copy").color(RED))).clicked() {
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
    use crate::recovery_actor::Availability;
    use cedar_recovery::{record_id, DraftMetadata, Store};
    use std::time::{Duration, Instant};

    const STORAGE_UNVERIFIED: &str = "Recovery storage is unavailable or could not be inspected. Older copies may exist. Your editor text remains open.";
    const REMOVALS_UNVERIFIED: &str = "Requested recovery copy removals could not be verified. Copies may remain. Your editor text remains open.";
    const REVIEW_REQUIRED: &str = "Review is still required before quitting. Recovery copies may remain. Your editor text remains open.";
    const FINAL_WARNING: &str = "Current unsaved editor text and profile form edits may be lost. Remaining recovery copies may be older or incomplete and are not proof that your current text is recoverable. Save or copy your draft before quitting if you need it.";

    fn close_app() -> CedarApp {
        let mut app = CedarApp::empty();
        let form = ConnectForm {
            local_root: "/synthetic/project".into(),
            allow_run: false,
            ..Default::default()
        };
        app.workspace_key = Some(form.key());
        app.active_form = Some(form);
        app.root = "/synthetic/project".into();
        app.state = ConnectionState::Ready;
        app.open_form = false;
        let mut doc = Document::new(1, "main.rs".into(), "base".into(), "r0".into());
        doc.text = "current unsaved text".into();
        doc.edit_version = 1;
        app.documents.push(doc);
        app.active_document = Some(1);
        app
    }

    fn wait_recovery(app: &mut CedarApp, done: impl Fn(&CedarApp) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !done(app) {
            app.recovery.poll();
            assert!(Instant::now() < deadline, "recovery did not settle");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn recovery_frame(app: &mut CedarApp, time: f64) -> egui::FullOutput {
        app.editor_ctx.clone().run(
            egui::RawInput {
                time: Some(time),
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 960.0),
                )),
                ..Default::default()
            },
            |ctx| {
                app.recovery_window(ctx);
                app.finish_recovery_close_frame(ctx);
            },
        )
    }

    fn frame_has_text(output: &egui::FullOutput, expected: &str) -> bool {
        fn contains(shape: &egui::epaint::Shape, expected: &str) -> bool {
            match shape {
                egui::epaint::Shape::Text(text) => text.galley.job.text == expected,
                egui::epaint::Shape::Vec(shapes) => {
                    shapes.iter().any(|shape| contains(shape, expected))
                }
                _ => false,
            }
        }
        output
            .shapes
            .iter()
            .any(|shape| contains(&shape.shape, expected))
    }

    fn assert_close_frames(app: &mut CedarApp, time: f64, expected: &str) {
        let close = app.recovery.closing.as_ref().unwrap();
        let phase = close.phase;
        let guard = close.guard.clone();
        let snapshot = app.recovery.test_snapshot();
        let text = app.documents[0].text.clone();
        assert!(!app.allow_close);
        recovery_frame(app, time);
        let output = recovery_frame(app, time + 0.1);
        assert!(frame_has_text(&output, expected), "missing {expected}");
        assert!(!frame_has_text(
            &output,
            "Recovery could not finish the requested discards. Your editor text remains open."
        ));
        for reason in [STORAGE_UNVERIFIED, REMOVALS_UNVERIFIED, REVIEW_REQUIRED] {
            if reason != expected {
                assert!(!frame_has_text(&output, reason));
            }
        }
        if !app.recovery.initialized {
            assert!(!frame_has_text(&output, "No recovery copies available"));
        }
        assert_eq!(app.recovery.closing.as_ref().unwrap().phase, phase);
        assert_eq!(app.recovery.closing.as_ref().unwrap().guard, guard);
        assert_eq!(app.recovery_close_guard(), guard);
        assert_eq!(app.recovery.test_snapshot(), snapshot);
        assert_eq!(app.documents[0].text, text);
        assert!(!app.allow_close);
        assert!(!output.viewport_output.values().any(|viewport| viewport
            .commands
            .iter()
            .any(|command| matches!(command, egui::ViewportCommand::Close))));
    }

    fn assert_decision_and_final_warning(app: &mut CedarApp, expected: &str) {
        app.finish_recovery_close(&egui::Context::default());
        assert_eq!(
            app.recovery.closing.as_ref().unwrap().phase,
            ClosePhase::NeedsDecision
        );
        assert_close_frames(app, 0.0, expected);
        app.retain_recovery_and_quit();
        wait_recovery(app, |app| {
            matches!(
                app.recovery.closing.as_ref().unwrap().phase,
                ClosePhase::AwaitingConfirmation { .. }
            )
        });
        assert_close_frames(app, 1.0, FINAL_WARNING);
    }

    #[test]
    fn actual_frames_failed_open_without_owned_records_explains_uninspected_storage() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("recovery");
        std::fs::write(&path, "blocker").unwrap();
        let mut app = close_app();
        app.recovery.start(Ok(path.clone()), &app.editor_ctx);
        wait_recovery(&mut app, |app| app.recovery.error.is_some());
        assert!(!app.recovery.initialized);
        assert!(app.recovery.drafts.is_empty());
        assert_eq!(app.recovery.test_snapshot(), (0, 0, 0));
        assert_decision_and_final_warning(&mut app, STORAGE_UNVERIFIED);
        drop(app);
        assert_eq!(std::fs::read_to_string(path).unwrap(), "blocker");
    }

    #[test]
    fn actual_frames_requested_removal_failure_says_copies_may_remain() {
        let mut app = close_app();
        let workspace = app.recovery_workspace().unwrap();
        let doc = &app.documents[0];
        app.recovery.start(
            Err("Recovery location is unavailable".into()),
            &app.editor_ctx,
        );
        app.recovery.drafts.push(DraftMetadata {
            id: record_id(&workspace, &doc.path).unwrap(),
            workspace: workspace.clone(),
            path: doc.path.clone(),
            base_revision: doc.revision.clone(),
            modified_ms: 1,
            text_bytes: doc.text.len(),
            base_text_bytes: doc.saved_text.len(),
        });
        app.recovery.authorize(&workspace, doc);
        assert_decision_and_final_warning(&mut app, REMOVALS_UNVERIFIED);
        assert_eq!(app.recovery.drafts.len(), 1);
    }

    #[test]
    fn actual_frames_unknown_older_copy_never_claims_absence() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("recovery");
        let mut app = close_app();
        let draft = Draft {
            workspace: app.recovery_workspace().unwrap(),
            path: "main.rs".into(),
            text: "older uninspected copy".into(),
            base_text: "base".into(),
            base_revision: Some("r0".into()),
            modified_ms: 1,
        };
        let mut locked = Store::open(&path).unwrap();
        locked.write(1, &draft).unwrap();
        app.recovery.start(Ok(path), &app.editor_ctx);
        wait_recovery(&mut app, |app| app.recovery.error.is_some());
        assert!(matches!(
            app.recovery.availability(),
            Availability::Unavailable(_)
        ));
        assert!(!app.recovery.initialized);
        assert!(app.recovery.drafts.is_empty());
        assert_eq!(app.recovery.test_snapshot(), (0, 0, 0));
        assert_decision_and_final_warning(&mut app, STORAGE_UNVERIFIED);
        drop(app);
        assert_eq!(
            locked
                .read(&record_id(&draft.workspace, &draft.path).unwrap())
                .unwrap()
                .text,
            draft.text
        );
    }

    #[test]
    fn actual_frames_late_successful_remove_keeps_review_without_unavailable_warning() {
        let temp = tempfile::tempdir().unwrap();
        let mut app = close_app();
        app.recovery
            .start(Ok(temp.path().join("recovery")), &app.editor_ctx);
        wait_recovery(&mut app, |app| app.recovery.initialized);
        let workspace = app.recovery_workspace().unwrap();
        app.recovery.observe(&workspace, &app.documents[0]);
        app.recovery.flush();
        wait_recovery(&mut app, |app| {
            app.recovery.protected(&workspace, &app.documents[0])
        });

        let (entered, release) = app.recovery.hold_next_operation(false);
        app.finish_recovery_close(&egui::Context::default());
        entered.recv_timeout(Duration::from_secs(5)).unwrap();
        app.recovery.closing.as_mut().unwrap().phase = ClosePhase::Discarding {
            started: Instant::now() - crate::recovery::CLOSE_OBSERVATION,
        };
        app.recovery.observe_close_deadline();
        assert_eq!(
            app.recovery.closing.as_ref().unwrap().phase,
            ClosePhase::NeedsDecision
        );
        assert_close_frames(&mut app, 0.0, REMOVALS_UNVERIFIED);

        release.send(()).unwrap();
        wait_recovery(&mut app, |app| app.recovery.removals_finished());
        assert!(app.recovery.initialized);
        assert_eq!(app.recovery.availability(), Availability::Ready);
        assert_eq!(app.recovery.test_snapshot().2, 1);
        assert_eq!(
            app.recovery.closing.as_ref().unwrap().phase,
            ClosePhase::NeedsDecision
        );
        assert_close_frames(&mut app, 1.0, REVIEW_REQUIRED);

        app.retain_recovery_and_quit();
        wait_recovery(&mut app, |app| {
            matches!(
                app.recovery.closing.as_ref().unwrap().phase,
                ClosePhase::AwaitingConfirmation { .. }
            )
        });
        assert_close_frames(&mut app, 2.0, FINAL_WARNING);
    }

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
