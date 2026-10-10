//! One explicit, bounded cohort of conditional saves. The cohort retains only
//! metadata; the existing single-save path owns the one submitted draft/token.
//! Responses record outcomes here, but only the final frame guard sends Writes.
use crate::{interrupted_save::InterruptedSave, model::Document, CedarApp, Job};
use cedar_protocol::MAX_FILE_BYTES;
use eframe::egui;
use sha2::{Digest, Sha256};

const MAX_MEMBERS: usize = 32;
const MAX_TOTAL_BYTES: usize = 32 * MAX_FILE_BYTES;

fn digest(text: &str) -> [u8; 32] {
    Sha256::digest(text.as_bytes()).into()
}

fn workspace_digest(app: &CedarApp) -> Option<[u8; 32]> {
    let workspace = app.recovery_workspace()?;
    let encoded = serde_json::to_string(&workspace).ok()?;
    Some(digest(&encoded))
}

struct Member {
    document: u64,
    path: String,
    version: u64,
    text: [u8; 32],
    baseline: [u8; 32],
    revision: Option<String>,
}

impl Member {
    fn capture(app: &CedarApp, doc: &Document) -> Option<Self> {
        // Share submission eligibility with single Save, then discard this
        // temporary validation token. A queued member never owns a request ID.
        if doc.edit_version == u64::MAX
            || doc.text.contains('\0')
            || doc.saved_text.contains('\0')
            || doc
                .path
                .split('/')
                .any(|part| part.is_empty() || part == ".")
            || InterruptedSave::capture(app, doc).is_none()
        {
            return None;
        }
        Some(Self {
            document: doc.id,
            path: doc.path.clone(),
            version: doc.edit_version,
            text: digest(&doc.text),
            baseline: digest(&doc.saved_text),
            revision: doc.revision.clone(),
        })
    }

    fn current(&self, app: &CedarApp) -> bool {
        let Some(doc) = app.documents.iter().find(|doc| doc.id == self.document) else {
            return false;
        };
        doc.dirty()
            && !doc.saving
            && !doc.save_outcome_unknown()
            && self.path == doc.path
            && self.version == doc.edit_version
            && self.revision == doc.revision
            && doc.text.len() <= MAX_FILE_BYTES
            && doc.saved_text.len() <= MAX_FILE_BYTES
            && self.text == digest(&doc.text)
            && self.baseline == digest(&doc.saved_text)
    }
}

#[derive(Clone, Copy)]
enum Outcome {
    Acknowledged,
    Failed,
    Unknown,
}

struct Inflight {
    generation: u64,
    request: u64,
    outcome: Option<Outcome>,
    missing_submission: bool,
}

struct Batch {
    generation: u64,
    workspace: [u8; 32],
    members: Vec<Member>,
    next: usize,
    inflight: Option<Inflight>,
    acknowledged: usize,
    failed: usize,
    unknown: usize,
    stopped: Option<&'static str>,
}

impl Batch {
    fn stop(&mut self, reason: &'static str) {
        self.stopped.get_or_insert(reason);
    }

    fn summary(&self) -> String {
        let reason = self
            .stopped
            .unwrap_or("Finished; newer edits may remain unsaved");
        format!(
            "Save All: {} acknowledged · {} failed · {} unknown · {} unattempted. {}.",
            self.acknowledged,
            self.failed,
            self.unknown,
            self.members.len() - self.next,
            reason,
        )
    }
}

#[derive(Default)]
pub(super) struct SaveAll {
    batch: Option<Batch>,
    summary: Option<String>,
}

impl CedarApp {
    pub(super) fn save_all_busy(&self) -> bool {
        self.save_all.batch.is_some()
    }

    pub(super) fn save_all_message(&self) -> Option<String> {
        if let Some(batch) = &self.save_all.batch {
            Some(format!(
                "Save All: {} acknowledged · {} awaiting reply · {} unattempted{}",
                batch.acknowledged,
                usize::from(batch.inflight.is_some()),
                batch.members.len() - batch.next,
                if batch.stopped.is_some() {
                    " · stopping"
                } else {
                    ""
                },
            ))
        } else {
            self.save_all.summary.clone()
        }
    }

    fn save_all_problem(&self) -> Option<&'static str> {
        if !self.ready() || self.worker.is_none() {
            Some("Reconnect to the original workspace before Save All")
        } else if !self.backend_supports("write") {
            Some("The workspace agent does not advertise writing")
        } else if self.interrupted_save_check.busy() {
            Some("Finish the interrupted-save check before Save All")
        } else if self.documents.iter().any(|doc| doc.save_outcome_unknown()) {
            Some("Resolve unknown save outcomes before Save All")
        } else if self.documents.iter().any(|doc| doc.saving)
            || self
                .pending
                .values()
                .any(|job| matches!(job, Job::Save { .. }))
        {
            Some("Wait for the current save before Save All")
        } else if self.next_request == 0 || self.next_request == u64::MAX {
            Some("Reconnect before saving; request identities are exhausted")
        } else {
            None
        }
    }

    fn save_all_frame_blocked(&self, ctx: &egui::Context) -> bool {
        self.navigation.blocks_editor()
            || self.foreign_modal_owns_input(ctx)
            || self.open_form
            || self.new_file
            || self.confirm.is_some()
            || self.close_tab_requested.is_some()
            || self.close_after_language_stop
            || self.close_snapshot.is_some()
            || self.allow_close
            || self.recovery.closing.is_some()
            || self.recovery.restoring_generation.is_some()
            || self.recovery.pending_restore.is_some()
            || ctx.input(|input| input.viewport().close_requested())
    }

    pub(super) fn queue_save_all(&mut self) {
        // Repeated clicks and shortcuts cannot replace the frozen cohort.
        if self.save_all_busy() {
            return;
        }
        self.save_all.summary = None;
        if let Some(problem) = self.save_all_problem() {
            self.error = Some(format!(
                "Save All did not start: {problem}. No writes sent."
            ));
            return;
        }
        let Some(workspace) = workspace_digest(self) else {
            self.error = Some(
                "Save All did not start: workspace identity is unavailable. No writes sent.".into(),
            );
            return;
        };
        let mut members = Vec::new();
        let mut total = 0usize;
        for doc in self.documents.iter().filter(|doc| doc.dirty()) {
            let bytes = total.checked_add(doc.text.len());
            if members.len() == MAX_MEMBERS || bytes.is_none_or(|bytes| bytes > MAX_TOTAL_BYTES) {
                self.error = Some("Save All did not start: at most 32 dirty buffers and 32 MiB of draft text are allowed. No writes sent.".into());
                return;
            }
            let Some(member) = Member::capture(self, doc) else {
                self.error = Some("Save All did not start: a draft has an invalid path, unbounded contents, or unavailable save identity. No writes sent.".into());
                return;
            };
            if members.iter().any(|prior: &Member| {
                prior.document == member.document || prior.path == member.path
            }) {
                self.error = Some(
                    "Save All did not start: draft identities are ambiguous. No writes sent."
                        .into(),
                );
                return;
            }
            total = bytes.unwrap();
            members.push(member);
        }
        if members.is_empty() {
            self.save_all.summary = Some("Save All: no dirty buffers.".into());
            self.notice = self.save_all_message().unwrap();
            return;
        }
        self.save_all.batch = Some(Batch {
            generation: self.generation,
            workspace,
            members,
            next: 0,
            inflight: None,
            acknowledged: 0,
            failed: 0,
            unknown: 0,
            stopped: None,
        });
        self.notice = self.save_all_message().unwrap();
    }

    pub(super) fn cancel_save_all(&mut self) {
        if let Some(batch) = &mut self.save_all.batch {
            batch.stop("Cancelled; an already submitted write is not undone");
            self.notice = self.save_all_message().unwrap();
        }
    }

    /// Called only after the ordinary save handler has validated its current
    /// submission. Recording a reply must never enqueue another operation.
    pub(super) fn save_all_observe_reply(
        &mut self,
        generation: u64,
        request: u64,
        success: bool,
        unknown: bool,
    ) {
        let Some(inflight) = self
            .save_all
            .batch
            .as_mut()
            .and_then(|batch| batch.inflight.as_mut())
        else {
            return;
        };
        if inflight.generation != generation
            || inflight.request != request
            || inflight.outcome.is_some()
        {
            return;
        }
        // The normal handler cannot retain a submission when its Save job has
        // disappeared. Record that loss without reconstructing a token from
        // the current draft or touching a replacement job's target.
        inflight.missing_submission = !matches!(self.pending.get(&request), Some(Job::Save { .. }));
        inflight.outcome = Some(if unknown || inflight.missing_submission {
            Outcome::Unknown
        } else if success {
            Outcome::Acknowledged
        } else {
            Outcome::Failed
        });
    }

    pub(super) fn save_all_transport_lost(&mut self) {
        if let Some(batch) = &mut self.save_all.batch {
            batch.stop("Connection lost; no further writes were sent");
            if let Some(inflight) = &mut batch.inflight {
                // A validated acknowledgement followed by EOF stays counted.
                if inflight.outcome.is_none() {
                    // As with a reply, the ordinary interrupted-save handler
                    // cannot retain an absent submission. Let final settlement
                    // protect only this batch's exact original document owner.
                    inflight.missing_submission =
                        !matches!(self.pending.get(&inflight.request), Some(Job::Save { .. }));
                    inflight.outcome = Some(Outcome::Unknown);
                }
            }
            self.notice = self.save_all_message().unwrap();
        }
    }

    fn finish_stopped_save_all(&mut self) {
        if self.save_all.batch.as_ref().is_some_and(|batch| {
            batch.inflight.is_none()
                && (batch.stopped.is_some() || batch.next == batch.members.len())
        }) {
            let batch = self.save_all.batch.take().unwrap();
            self.save_all.summary = Some(batch.summary());
            self.notice = self.save_all_message().unwrap();
            self.editor_ctx.request_repaint();
        }
    }

    pub(super) fn finish_save_all_frame(&mut self, ctx: &egui::Context) {
        if !self.save_all_busy() {
            return;
        }
        if self.save_all_frame_blocked(ctx) {
            self.save_all
                .batch
                .as_mut()
                .unwrap()
                .stop("Stopped for a dialog or close transition");
        }
        let unprotected_owner = self.save_all.batch.as_ref().and_then(|batch| {
            let inflight = batch.inflight.as_ref()?;
            if !inflight.missing_submission
                || inflight.outcome.is_none()
                || batch.generation != self.generation
                || Some(batch.workspace) != workspace_digest(self)
            {
                return None;
            }
            let member = batch.members.get(batch.next.checked_sub(1)?)?;
            Some((member.document, member.path.clone()))
        });
        if let Some((document, path)) = unprotected_owner {
            let workspace = self.recovery_workspace();
            if let Some(doc) = self
                .documents
                .iter_mut()
                .find(|doc| doc.id == document && doc.path == path)
            {
                // Session-local uncertainty survives reconnect's saving reset.
                // Preserve any existing token, draft, baseline and editor state;
                // no trustworthy submission exists from which to invent one.
                doc.save_outcome_unverifiable = true;
                doc.saving = false;
                // The ordinary final recovery observation ran before this
                // settlement. Replace any same-frame Undo-to-clean removal
                // now; another frame is not required to retain this copy.
                if let Some(workspace) = &workspace {
                    self.recovery.observe(workspace, doc);
                }
                ctx.request_repaint();
            }
        }
        let unverifiable = self.save_all.batch.as_ref().is_some_and(|batch| {
            batch
                .next
                .checked_sub(1)
                .and_then(|index| batch.members.get(index))
                .is_some_and(|member| {
                    self.documents.iter().any(|doc| {
                        doc.id == member.document
                            && doc.path == member.path
                            && doc.save_outcome_unverifiable
                    })
                })
        });
        let batch = self.save_all.batch.as_mut().unwrap();
        if let Some(outcome) = batch
            .inflight
            .as_ref()
            .and_then(|inflight| inflight.outcome)
        {
            batch.inflight = None;
            match outcome {
                Outcome::Acknowledged => batch.acknowledged += 1,
                Outcome::Failed => {
                    batch.failed += 1;
                    batch.stop("A write failed; earlier acknowledged writes remain applied");
                }
                Outcome::Unknown => {
                    batch.unknown += 1;
                    batch.stop(if unverifiable {
                        "Save identity unavailable; keep or copy the draft and compare disk. Interrupted-save Check is unavailable"
                    } else {
                        "A write outcome is unknown; check the interrupted save before saving again"
                    });
                }
            }
        }
        self.finish_stopped_save_all();
        let Some(batch) = &self.save_all.batch else {
            return;
        };
        if batch.inflight.is_some() {
            return;
        }
        let problem = self.save_all_problem().or_else(|| {
            if batch.generation != self.generation
                || Some(batch.workspace) != workspace_digest(self)
            {
                Some("Workspace identity changed; no further writes were sent")
            } else if !batch.members[batch.next..]
                .iter()
                .all(|member| member.current(self))
            {
                Some("An unattempted draft changed; start Save All again to capture current drafts")
            } else {
                None
            }
        });
        if let Some(problem) = problem {
            self.save_all.batch.as_mut().unwrap().stop(problem);
            self.finish_stopped_save_all();
            return;
        }
        let document = batch.members[batch.next].document;
        let request = self.next_request;
        self.save_document_now(document);
        if matches!(self.pending.get(&request), Some(Job::Save { document: sent, .. }) if *sent == document)
        {
            let batch = self.save_all.batch.as_mut().unwrap();
            batch.next += 1;
            batch.inflight = Some(Inflight {
                generation: self.generation,
                request,
                outcome: None,
                missing_submission: false,
            });
            self.notice = self.save_all_message().unwrap();
        } else {
            self.save_all
                .batch
                .as_mut()
                .unwrap()
                .stop("The next write could not be submitted; no retry was made");
            self.finish_stopped_save_all();
        }
    }

    pub(super) fn save_all_shortcut(&mut self, ctx: &egui::Context) {
        fn shortcut(event: &egui::Event) -> bool {
            matches!(event, egui::Event::Key { key: egui::Key::S, modifiers, .. }
                if !(modifiers.ctrl && modifiers.mac_cmd)
                    && modifiers.matches_exact(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT))
        }
        let requested = ctx.input(|input| {
            let mut events = input.raw.events.iter().filter(|event| {
                matches!(
                    event,
                    egui::Event::Key { pressed: true, .. }
                        | egui::Event::Text(_)
                        | egui::Event::Paste(_)
                        | egui::Event::Cut
                        | egui::Event::Copy
                        | egui::Event::Ime(_)
                        | egui::Event::PointerButton { .. }
                        | egui::Event::Touch { .. }
                        | egui::Event::MouseWheel { .. }
                        | egui::Event::Zoom(_)
                        | egui::Event::WindowFocused(false)
                )
            });
            events.next().is_some_and(|event| {
                shortcut(event) && matches!(event, egui::Event::Key { repeat: false, .. })
            }) && events.next().is_none()
                && input.focused
        });
        // Consume press/repeat/release so the single-save handler cannot treat
        // any part of this chord as an ordinary Save or replace an active batch.
        ctx.input_mut(|input| input.events.retain(|event| !shortcut(event)));
        if requested && !self.save_all_frame_blocked(ctx) {
            self.queue_save_all();
        }
    }
}
