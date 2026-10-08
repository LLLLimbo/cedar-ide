//! Session-local, content-only reconciliation of one interrupted save per tab.
//! Retain a bounded digest token, never a second full submitted draft. Checking
//! sends two Reads and never retries a Write or proves physical file identity.
use crate::{model::Document, CedarApp, Job, Operation, Payload};
use cedar_protocol::MAX_FILE_BYTES;
use sha2::{Digest, Sha256};

const MAX_PATH_BYTES: usize = 4096;

fn digest(text: &str) -> [u8; 32] {
    Sha256::digest(text.as_bytes()).into()
}

fn workspace_digest(app: &CedarApp) -> Option<[u8; 32]> {
    let workspace = app.recovery_workspace()?;
    let encoded = serde_json::to_string(&workspace).ok()?;
    Some(digest(&encoded))
}

fn valid_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= MAX_PATH_BYTES
        && !path.contains(['\0', '\\', ':'])
        && !path.starts_with('/')
        && !path.split('/').any(|part| part == "..")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct InterruptedSave {
    generation: u64,
    request: u64,
    document: u64,
    workspace: [u8; 32],
    path: String,
    base_revision: Option<String>,
    base_digest: [u8; 32],
    submitted_digest: [u8; 32],
}

impl InterruptedSave {
    pub fn capture(app: &CedarApp, doc: &Document) -> Option<Self> {
        if !valid_path(&doc.path)
            || doc.text.len() > MAX_FILE_BYTES
            || doc.saved_text.len() > MAX_FILE_BYTES
            || doc
                .revision
                .as_ref()
                .is_some_and(|revision| revision.len() > 64)
        {
            return None;
        }
        Some(Self {
            generation: app.generation,
            request: app.next_request,
            document: doc.id,
            workspace: workspace_digest(app)?,
            path: doc.path.clone(),
            base_revision: doc.revision.clone(),
            base_digest: digest(&doc.saved_text),
            submitted_digest: digest(&doc.text),
        })
    }

    fn matches_baseline(&self, app: &CedarApp, doc: &Document) -> bool {
        self.document == doc.id
            && self.path == doc.path
            && Some(self.workspace) == workspace_digest(app)
            && self.base_revision == doc.revision
            && doc.saved_text.len() <= MAX_FILE_BYTES
            && self.base_digest == digest(&doc.saved_text)
    }
}

struct Source {
    generation: u64,
    navigation: u64,
    document: u64,
    edit_version: u64,
    draft_digest: [u8; 32],
    profile_epoch: u64,
    submission_generation: u64,
    submission_request: u64,
}

#[derive(PartialEq, Eq)]
enum Snapshot {
    File {
        text: String,
        revision: String,
        submitted: bool,
    },
    Absent,
}

struct PendingCheck {
    source: Source,
    snapshot: Option<Snapshot>,
    staged: bool,
}

#[derive(Default)]
pub(super) struct Check {
    epoch: u64,
    slot: Option<PendingCheck>,
    message: Option<String>,
    // Invalidating a check cannot cancel a wire Read. Drain that one ticket
    // before accepting another click, keeping both memory and queue bounded.
    pub outstanding: Option<u64>,
}

impl Check {
    pub fn busy(&self) -> bool {
        self.outstanding.is_some() || self.slot.is_some()
    }
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }
    pub fn invalidate(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.slot = None;
        self.message = None;
    }
    pub fn reset(&mut self) {
        self.invalidate();
        self.outstanding = None;
    }
}

impl CedarApp {
    pub(super) fn retain_interrupted_save(&mut self, request: u64) {
        let Some(Job::Save {
            document,
            submission: Some(submission),
            ..
        }) = self.pending.get(&request)
        else {
            return;
        };
        if submission.generation != self.generation || submission.request != request {
            return;
        }
        if let Some(doc) = self.documents.iter_mut().find(|doc| {
            doc.id == *document && doc.id == submission.document && doc.path == submission.path
        }) {
            // A queued request is not proof that the agent received or committed
            // it. Either way its outcome stays unknown until explicit checking.
            if doc.interrupted_save.is_none() {
                doc.interrupted_save = Some(submission.clone());
            }
        }
    }

    pub(super) fn retain_interrupted_saves(&mut self) {
        let requests: Vec<_> = self
            .pending
            .iter()
            .filter_map(|(id, job)| matches!(job, Job::Save { .. }).then_some(*id))
            .collect();
        for request in requests {
            self.retain_interrupted_save(request);
        }
    }

    fn interrupted_check_problem(&self, source: &Source) -> Option<&'static str> {
        if !self.backend_supports("read")
            || source.generation != self.generation
            || source.navigation != self.navigation_epoch
        {
            return Some("The connection or active tab changed. Check the interrupted save again");
        }
        let Some(doc) = self.active().filter(|doc| doc.id == source.document) else {
            return Some("The tab changed. Check the interrupted save again");
        };
        let Some(token) = &doc.interrupted_save else {
            return Some("This interrupted save is no longer current");
        };
        if token.generation != source.submission_generation
            || token.request != source.submission_request
            || !token.matches_baseline(self, doc)
        {
            return Some("The save identity or baseline changed. Keep your draft and review disk");
        }
        if doc.edit_version != source.edit_version || digest(&doc.text) != source.draft_digest {
            return Some("The draft changed during the check. Check the interrupted save again");
        }
        if doc.path == crate::profile_ui::PATH && source.profile_epoch != self.profiles.epoch {
            return Some(
                "The profile form changed during the check. Check the interrupted save again",
            );
        }
        if self.documents.iter().any(|doc| doc.saving)
            || self
                .pending
                .values()
                .any(|job| matches!(job, Job::Save { .. }))
        {
            return Some("Wait for this file's save before checking its interrupted save");
        }
        if self.close_tab_requested.is_some()
            || self.confirm.is_some()
            || self.close_after_language_stop
            || self.close_snapshot.is_some()
            || self.recovery.closing.is_some()
            || self.allow_close
        {
            return Some("Finish or cancel the close action before checking the interrupted save");
        }
        None
    }

    pub(super) fn check_interrupted_save(&mut self) {
        if self.interrupted_save_check.busy() || self.disk_review.busy() {
            return;
        }
        let Some(doc) = self.active() else { return };
        let Some(token) = &doc.interrupted_save else {
            return;
        };
        let source = Source {
            generation: self.generation,
            navigation: self.navigation_epoch,
            document: doc.id,
            edit_version: doc.edit_version,
            draft_digest: digest(&doc.text),
            profile_epoch: self.profiles.epoch,
            submission_generation: token.generation,
            submission_request: token.request,
        };
        if let Some(problem) = self.interrupted_check_problem(&source) {
            self.interrupted_save_check.message = Some(problem.into());
            return;
        }
        let path = token.path.clone();
        self.dismiss_disk_review();
        self.interrupted_save_check.invalidate();
        self.interrupted_save_check.slot = Some(PendingCheck {
            source,
            snapshot: None,
            staged: false,
        });
        self.interrupted_save_check.message =
            Some("Checking interrupted save · reading disk twice; no write is sent".into());
        self.request_interrupted_save_read(path);
    }

    fn request_interrupted_save_read(&mut self, path: String) {
        let ticket = self.interrupted_save_check.epoch;
        let id = self.request(
            Operation::Read { path },
            Job::InterruptedSaveCheck { ticket },
        );
        if id == 0 {
            self.fail_interrupted_save_check(
                "The disk check could not start. Reconnect and try again".into(),
            );
        } else {
            self.interrupted_save_check.outstanding = Some(id);
        }
    }

    fn fail_interrupted_save_check(&mut self, message: String) {
        self.interrupted_save_check.slot = None;
        self.interrupted_save_check.message = Some(message);
    }

    fn interrupted_snapshot(&self, payload: Payload) -> Result<Snapshot, &'static str> {
        let doc = self.active().unwrap();
        let token = doc.interrupted_save.as_ref().unwrap();
        let Payload::File {
            path,
            text,
            revision,
        } = payload
        else {
            return Err("Unexpected disk response. The interrupted save is unresolved; your draft is retained");
        };
        if path != token.path || !valid_path(&path) {
            return Err(
                "Disk returned a different or invalid path. The interrupted save is unresolved",
            );
        }
        if text.len() > MAX_FILE_BYTES || text.contains('\0') {
            return Err(
                "Disk returned oversized text or NUL bytes. The interrupted save is unresolved",
            );
        }
        let hash = digest(&text);
        // Unlike ordinary compare, baseline adoption must establish the actual
        // content-to-revision relationship, not just canonical token syntax.
        if revision != format!("{:x}", Sha256::digest(text.as_bytes())) {
            return Err("Disk contents do not match their SHA-256 revision. The interrupted save is unresolved");
        }
        let submitted = hash == token.submitted_digest;
        if !submitted
            && !(token.base_revision.as_ref() == Some(&revision) && hash == token.base_digest)
        {
            return Err("Disk differs from both the submitted contents and original baseline. Use Compare with disk and Copy draft to review; the interrupted save remains unresolved");
        }
        Ok(Snapshot::File {
            text,
            revision,
            submitted,
        })
    }

    pub(super) fn apply_interrupted_save_read(
        &mut self,
        ticket: u64,
        result: Result<Payload, String>,
    ) {
        let check = &self.interrupted_save_check;
        let Some(slot) = &check.slot else { return };
        if ticket != check.epoch || slot.staged {
            return;
        }
        if let Some(problem) = self.interrupted_check_problem(&slot.source) {
            self.fail_interrupted_save_check(problem.into());
            return;
        }
        let snapshot = match result {
            Ok(payload) => self.interrupted_snapshot(payload).map_err(str::to_owned),
            Err(error) if error.starts_with("not_found:") && self.active().unwrap().interrupted_save.as_ref().unwrap().base_revision.is_none() => Ok(Snapshot::Absent),
            Err(error) => Err(format!("Disk could not be read: {}. The interrupted save remains unresolved; reconnect or use Compare with disk. Your draft is retained", error.chars().take(1024).collect::<String>())),
        };
        let snapshot = match snapshot {
            Ok(snapshot) => snapshot,
            Err(message) => {
                self.fail_interrupted_save_check(message);
                return;
            }
        };
        let slot = self.interrupted_save_check.slot.as_mut().unwrap();
        if let Some(first) = &slot.snapshot {
            if first == &snapshot {
                slot.staged = true;
            } else {
                self.fail_interrupted_save_check("Disk changed between reads. Check again or use Compare with disk; the interrupted save remains unresolved".into());
            }
        } else {
            slot.snapshot = Some(snapshot);
            let path = self.active().unwrap().path.clone();
            self.request_interrupted_save_read(path);
        }
    }

    pub(super) fn finish_interrupted_save_check(&mut self) {
        let Some(slot) = &self.interrupted_save_check.slot else {
            return;
        };
        if !slot.staged {
            return;
        }
        if let Some(problem) = self.interrupted_check_problem(&slot.source) {
            self.fail_interrupted_save_check(problem.into());
            return;
        }
        let slot = self.interrupted_save_check.slot.take().unwrap();
        let snapshot = slot.snapshot.unwrap();
        let workspace = self.recovery_workspace();
        let doc = self
            .documents
            .iter_mut()
            .find(|doc| doc.id == slot.source.document)
            .unwrap();
        // The current draft, cursor, edit version and native Undo/Redo are never
        // replaced. Only the submitted snapshot becomes the confirmed baseline.
        let (submitted, absent) = match snapshot {
            Snapshot::File {
                text,
                revision,
                submitted: true,
            } => {
                doc.saved_text = text;
                doc.revision = Some(revision);
                (true, false)
            }
            Snapshot::File { .. } => (false, false),
            Snapshot::Absent => (false, true),
        };
        doc.interrupted_save = None;
        if let Some(workspace) = workspace {
            self.recovery.saved(&workspace, doc);
        }
        let profile = doc.path == crate::profile_ui::PATH;
        let message = if submitted {
            "Disk matches submitted contents. The baseline is updated; any newer draft remains unsaved. This does not prove the interrupted request committed or identify the physical file"
        } else if absent {
            "The new-file path is currently absent, matching its original baseline. Use Save explicitly to create it. This does not prove whether the interrupted request committed before the path became absent"
        } else {
            "Disk currently matches the original baseline. Use Save explicitly if you want to write your draft. This does not prove whether the interrupted request committed or identify the physical file"
        };
        if profile {
            self.profile_interrupted_save_checked(slot.source.document);
        }
        self.interrupted_save_check.message = Some(message.into());
        self.notice = message.into();
    }
}

#[cfg(test)]
mod tests;
