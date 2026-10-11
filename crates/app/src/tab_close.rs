//! One snapshot-bound tab close. Replies establish eligibility only; removing
//! editor state belongs to the final frame, after all input and draft mutations.
use super::*;
use sha2::{Digest, Sha256};

fn digest(text: &str) -> [u8; 32] {
    Sha256::digest(text.as_bytes()).into()
}

fn workspace(app: &CedarApp) -> Option<[u8; 32]> {
    app.recovery_workspace()
        .and_then(|value| serde_json::to_string(&value).ok())
        .map(|value| digest(&value))
}

#[derive(Clone)]
struct Source {
    document: u64,
    path: String,
    generation: u64,
    workspace: Option<[u8; 32]>,
    connection: ConnectionState,
    version: u64,
    text: [u8; 32],
    baseline: [u8; 32],
    revision: Option<String>,
    interrupted: Option<interrupted_save::InterruptedSave>,
    unverifiable: bool,
    saving: bool,
    profile_epoch: u64,
    profile_draft: [u8; 32],
    profile_owner: bool,
}

impl Source {
    fn capture(app: &CedarApp, doc: &Document) -> Self {
        Self {
            document: doc.id,
            path: doc.path.clone(),
            generation: app.generation,
            workspace: workspace(app),
            connection: app.state,
            version: doc.edit_version,
            text: digest(&doc.text),
            baseline: digest(&doc.saved_text),
            revision: doc.revision.clone(),
            interrupted: doc.interrupted_save.clone(),
            unverifiable: doc.save_outcome_unverifiable,
            saving: doc.saving,
            profile_epoch: app.profiles.epoch,
            profile_draft: digest(&serde_json::to_string(&app.profiles.draft).unwrap()),
            profile_owner: app.profiles.owns_document(doc.id),
        }
    }

    fn current(&self, app: &CedarApp, acknowledged: bool) -> bool {
        let Some(doc) = app.documents.iter().find(|doc| doc.id == self.document) else {
            return false;
        };
        self.path == doc.path
            && self.generation == app.generation
            && self.workspace == workspace(app)
            && self.connection == app.state
            && self.version == doc.edit_version
            && self.version != u64::MAX
            && self.text == digest(&doc.text)
            && self.profile_epoch == app.profiles.epoch
            && self.profile_owner == app.profiles.owns_document(doc.id)
            && self.profile_draft == digest(&serde_json::to_string(&app.profiles.draft).unwrap())
            && if acknowledged {
                !doc.saving
                    && !doc.save_outcome_unknown()
                    && digest(&doc.saved_text) == self.text
                    && doc.revision.as_ref().is_some_and(|revision| {
                        interrupted_save::acknowledgement_matches(&doc.saved_text, revision)
                    })
            } else {
                self.revision == doc.revision
                    && self.baseline == digest(&doc.saved_text)
                    && self.interrupted == doc.interrupted_save
                    && self.unverifiable == doc.save_outcome_unverifiable
            }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Prompt,
    SaveQueued,
    Waiting(u64),
    Acknowledged,
    DiscardQueued,
    Failed,
}

pub(super) struct Intent {
    source: Source,
    phase: Phase,
}

// This bounded owner survives Keep editing until the exact reply or transport
// loss. Losing/replacing a pending job must not erase evidence of a sent write.
pub(super) struct Submission {
    source: Source,
    request: u64,
    token: interrupted_save::InterruptedSave,
}

impl CedarApp {
    pub(super) fn save_close_write_pending(&self) -> bool {
        self.tab_close_submission.is_some()
    }

    fn tab_close_submission_current(&self, submission: &Submission) -> bool {
        matches!(self.pending.get(&submission.request), Some(Job::Save {
            document, snapshot, submission: Some(token),
        }) if *document == submission.source.document
            && *token == submission.token
            && digest(snapshot) == submission.source.text)
    }

    fn protect_lost_tab_close_submission(&mut self, submission: &Submission) {
        if submission.source.generation != self.generation
            || submission.source.workspace != workspace(self)
        {
            return;
        }
        let workspace = self.recovery_workspace();
        if let Some(doc) = self
            .documents
            .iter_mut()
            .find(|doc| doc.id == submission.source.document && doc.path == submission.source.path)
        {
            doc.save_outcome_unverifiable = true;
            doc.saving = false;
            if let Some(workspace) = &workspace {
                self.recovery.observe(workspace, doc);
            }
        }
    }

    pub(super) fn settle_tab_close_transport_loss(&mut self) {
        if let Some(submission) = self.tab_close_submission.take() {
            if !self.tab_close_submission_current(&submission) {
                self.protect_lost_tab_close_submission(&submission);
            }
            // A still-exact job is protected by retain_interrupted_saves in
            // the ordinary transport-loss path; no replacement token is minted.
        }
    }

    pub(super) fn save_close_tab_busy(&self) -> bool {
        self.tab_close.is_some()
    }

    pub(super) fn capture_tab_close(&mut self, id: u64) {
        if let Some(doc) = self.documents.iter().find(|doc| doc.id == id) {
            self.tab_close = Some(Intent {
                source: Source::capture(self, doc),
                phase: Phase::Prompt,
            });
            self.confirm = Some(Confirm::CloseTab(id));
        }
    }

    pub(super) fn cancel_tab_close(&mut self) {
        if let Some(intent) = self.tab_close.take() {
            self.notice = if matches!(intent.phase, Phase::Waiting(_)) {
                "Close cancelled. The submitted save may still finish; your tab stays open."
            } else {
                "Close cancelled. Your tab and editor history are retained."
            }
            .into();
        }
        if matches!(self.confirm, Some(Confirm::CloseTab(_))) {
            self.confirm = None;
        }
    }

    pub(super) fn save_close_problem(&self) -> Option<&'static str> {
        let Some(intent) = &self.tab_close else {
            return Some("This close confirmation is no longer current. Close the tab again.");
        };
        if intent.phase != Phase::Prompt && intent.phase != Phase::SaveQueued {
            return Some(
                "Waiting for this save. Keep editing cancels closing; the save may still finish.",
            );
        }
        if !intent.source.current(self, false) {
            return Some("The draft or workspace changed. Close the tab again to review its current contents.");
        }
        if self.profiles.dirty() && self.profiles.owns_document(intent.source.document) {
            return Some("The profile form has unsaved edits. Keep editing, explicitly Save profile, then close this tab again.");
        }
        if !self.ready() || self.worker.is_none() || !self.backend_supports("write") {
            return Some(
                "Reconnect to the original workspace with writing available before Save and close.",
            );
        }
        if self.save_all_busy()
            || self.save_close_write_pending()
            || self.documents.iter().any(|doc| doc.saving)
            || self
                .pending
                .values()
                .any(|job| matches!(job, Job::Save { .. }))
        {
            return Some("Wait for the current save or Save All before Save and close.");
        }
        if self.interrupted_save_check.busy() {
            return Some("Finish the interrupted-save check before Save and close.");
        }
        if self.next_request == 0 || self.next_request == u64::MAX {
            return Some("Reconnect before saving; request identities are exhausted.");
        }
        let doc = self
            .documents
            .iter()
            .find(|doc| doc.id == intent.source.document)?;
        if doc.save_outcome_unknown() {
            return Some(
                "Check the interrupted save before Save and close. Your draft is retained.",
            );
        }
        if doc.text.len() > cedar_protocol::MAX_FILE_BYTES {
            return Some(
                "This draft exceeds the 1 MiB save limit. Keep or copy it before closing.",
            );
        }
        if !doc.dirty() {
            return Some("The draft changed. Close the tab again to review its current contents.");
        }
        None
    }

    pub(super) fn queue_save_close_tab(&mut self) {
        if let Some(problem) = self.save_close_problem() {
            self.error = Some(problem.into());
            return;
        }
        self.tab_close.as_mut().unwrap().phase = Phase::SaveQueued;
    }

    pub(super) fn queue_discard_close_tab(&mut self) {
        if let Some(intent) = &mut self.tab_close {
            if intent.phase == Phase::Prompt {
                intent.phase = Phase::DiscardQueued;
            }
        }
    }

    /// The caller uses the ordinary exact-submission and SHA acknowledgement
    /// checks before removing the pending job. This hook never closes a tab.
    pub(super) fn save_close_observe_reply(&mut self, generation: u64, request: u64, valid: bool) {
        let matches = self
            .tab_close_submission
            .as_ref()
            .is_some_and(|submission| {
                submission.source.generation == generation && submission.request == request
            });
        if !matches {
            return;
        }
        let submission = self.tab_close_submission.take().unwrap();
        let current = self.tab_close_submission_current(&submission);
        if !current {
            self.protect_lost_tab_close_submission(&submission);
        }
        let consent_current = self
            .tab_close
            .as_ref()
            .is_some_and(|intent| intent.source.current(self, false));
        if let Some(intent) = &mut self.tab_close {
            if intent.source.generation == generation && intent.phase == Phase::Waiting(request) {
                intent.phase = if valid && current && consent_current {
                    Phase::Acknowledged
                } else {
                    Phase::Failed
                };
            }
        }
    }

    fn tab_close_competing(&self, ctx: &egui::Context, id: u64) -> bool {
        !matches!(self.confirm, Some(Confirm::CloseTab(target)) if target == id)
            || self.save_all_busy()
            || self.open_form
            || self.new_file
            || self.copy_draft.is_open()
            // blocks_editor also remembers our own modal from the preceding
            // frame. Only a real navigation dialog competes with this consent;
            // foreign modal layers are checked separately below.
            || self.navigation.dialog_open()
            || self.close_tab_requested.is_some()
            || self.close_after_language_stop
            || self.close_snapshot.is_some()
            || self.allow_close
            || self.run_state.transition_pending()
            || self.recovery.closing.is_some()
            || self.recovery.resuming()
            || self.recovery.restoring_generation.is_some()
            || self.recovery.pending_restore.is_some()
            || self.recovery.remove_confirmation.is_some()
            || ctx.memory(|memory| {
                memory.top_modal_layer().is_some_and(|layer| {
                    layer
                        != egui::LayerId::new(
                            egui::Order::Foreground,
                            egui::Id::new("discard_confirmation"),
                        )
                })
            })
            || ctx.input(|input| input.viewport().close_requested())
    }

    pub(super) fn finish_save_close_tab_frame(&mut self, ctx: &egui::Context) {
        if self
            .tab_close_submission
            .as_ref()
            .is_some_and(|submission| !self.tab_close_submission_current(submission))
        {
            let submission = self.tab_close_submission.take().unwrap();
            self.protect_lost_tab_close_submission(&submission);
            ctx.request_repaint();
        }
        let Some(intent) = &self.tab_close else {
            return;
        };
        let id = intent.source.document;
        let phase = intent.phase;
        let saving_matches = matches!(phase, Phase::Waiting(_) | Phase::Acknowledged)
            || self
                .documents
                .iter()
                .find(|doc| doc.id == id)
                .is_some_and(|doc| doc.saving == intent.source.saving);
        let valid = saving_matches && intent.source.current(self, phase == Phase::Acknowledged);
        let lost_request =
            matches!(phase, Phase::Waiting(request) if !self.pending.contains_key(&request));
        if phase == Phase::Failed || !valid || lost_request || self.tab_close_competing(ctx, id) {
            self.cancel_tab_close();
            self.notice = "Tab kept open: the draft, workspace, save outcome or close intent changed. Review it before closing again; a submitted save may still finish.".into();
            return;
        }
        match phase {
            Phase::SaveQueued => {
                if let Some(problem) = self.save_close_problem() {
                    self.error = Some(problem.into());
                    self.cancel_tab_close();
                    return;
                }
                let request = self.next_request;
                self.save_document_now(id);
                if let Some(Job::Save {
                    document,
                    submission: Some(token),
                    ..
                }) = self
                    .pending
                    .get(&request)
                    .filter(|job| matches!(job, Job::Save { document, .. } if *document == id))
                {
                    debug_assert_eq!(*document, id);
                    self.tab_close_submission = Some(Submission {
                        source: self.tab_close.as_ref().unwrap().source.clone(),
                        request,
                        token: token.clone(),
                    });
                    self.tab_close.as_mut().unwrap().phase = Phase::Waiting(request);
                    self.notice = "Saving before closing this tab. Keep editing cancels closing; the submitted save may still finish.".into();
                } else {
                    self.cancel_tab_close();
                }
            }
            Phase::Acknowledged | Phase::DiscardQueued => {
                // No write or inferred recovery deletion is hidden here. The
                // ordinary close path tracks owned recovery cleanup separately.
                self.tab_close = None;
                self.confirm = None;
                self.remove_tab(id);
                self.notice = if phase == Phase::Acknowledged {
                    "Saved and closed the tab. Recovery cleanup is tracked separately."
                } else {
                    "Closed the reviewed draft. Recovery cleanup is tracked separately."
                }
                .into();
                if !self.recovery.removals_finished() || !self.recovery.failures().is_empty() {
                    self.recovery.visible = true;
                    self.notice
                        .push_str(" Review Recovery for pending or failed copy removal.");
                }
            }
            _ => {}
        }
    }

    pub(super) fn tab_close_dialog(&mut self, ctx: &egui::Context) {
        let Some(Confirm::CloseTab(id)) = self.confirm else {
            return;
        };
        let target = self
            .documents
            .iter()
            .find(|doc| doc.id == id)
            .map(|doc| doc.path.clone())
            .unwrap_or_default();
        let waiting = self
            .tab_close
            .as_ref()
            .is_some_and(|intent| intent.phase != Phase::Prompt);
        let problem = self.save_close_problem();
        let mut save = false;
        let mut discard = false;
        let mut cancel = false;
        let modal = egui::Modal::new(egui::Id::new("discard_confirmation")).show(ctx, |ui| {
            ui.set_max_width(465.0);
            ui.heading(if waiting { "Saving before closing" } else { "Close unsaved tab?" });
            ui.label(&target);
            ui.label("Save and close waits for a verified acknowledgement. Discard and close discards this reviewed draft and requests removal of recovery copies owned by this tab.");
            if let Some(problem) = problem { ui.colored_label(AMBER, problem); }
            ui.horizontal(|ui| {
                let response = ui.add_enabled(problem.is_none(), egui::Button::new("Save and close"));
                #[cfg(test)]
                workspace_access_tests::record(ui, "save_close_tab_save", &response);
                save = response.clicked();
                let response = ui.button("Keep editing");
                #[cfg(test)]
                workspace_access_tests::record(ui, "save_close_tab_cancel", &response);
                cancel = response.clicked();
                let response = ui.add_enabled(!waiting && self.tab_close.is_some(), egui::Button::new(RichText::new("Discard and close").color(RED)));
                #[cfg(test)]
                workspace_access_tests::record(ui, "save_close_tab_discard", &response);
                discard = response.clicked();
            });
        });
        cancel |= modal.should_close();
        crate::replace::discard_keyboard(ctx);
        if cancel {
            self.cancel_tab_close();
        } else if save {
            self.queue_save_close_tab();
        } else if discard {
            self.queue_discard_close_tab();
        }
    }
}
