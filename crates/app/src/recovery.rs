//! Recovery bookkeeping. Latest intent, copy evidence and close authority are separate.
use crate::{
    model::Document,
    recovery_actor::{
        Ack, Actor, Availability, Effect, Mutation, OperationKind, Settlement, Ticket,
    },
};
use cedar_recovery::{record_id, Draft, DraftMetadata, RecordId, WorkspaceIdentity};
use eframe::egui;
use std::{
    collections::HashMap,
    path::PathBuf,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub const CLOSE_OBSERVATION: Duration = Duration::from_secs(5);
#[derive(Clone, Debug, PartialEq, Eq)]
struct Stamp {
    document: u64,
    edit: u64,
    revision: Option<String>,
    dirty: bool,
}
impl Stamp {
    fn of(doc: &Document) -> Self {
        Self {
            document: doc.id,
            edit: doc.edit_version,
            revision: doc.revision.clone(),
            dirty: doc.dirty(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CloseGuard {
    documents: Vec<(String, Stamp)>,
    workspace: Option<WorkspaceIdentity>,
    session: u64,
    profile_epoch: u64,
}
impl CloseGuard {
    pub fn new(
        documents: &[Document],
        workspace: Option<WorkspaceIdentity>,
        session: u64,
        profile_epoch: u64,
    ) -> Self {
        Self {
            documents: documents
                .iter()
                .map(|doc| (doc.path.clone(), Stamp::of(doc)))
                .collect(),
            workspace,
            session,
            profile_epoch,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClosePhase {
    Discarding {
        started: Instant,
    },
    NeedsDecision,
    Draining {
        ticket: Ticket,
        retain: bool,
        started: Instant,
    },
    AwaitingConfirmation {
        ticket: Ticket,
    },
    Confirmed {
        ticket: Ticket,
    },
    Blocked {
        ticket: Option<Ticket>,
    },
}
pub struct CloseTransaction {
    pub guard: CloseGuard,
    pub phase: ClosePhase,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum CopyState {
    /// No owned copy is known. This makes no claim about other files in the directory.
    #[default]
    NoOwnedCopy,
    Present,
    Uncertain,
}
#[derive(Default)]
struct CopyKnowledge {
    sequence: u64,
    state: CopyState,
    last_applied_write: Option<u64>,
}
struct Tracked {
    workspace: WorkspaceIdentity,
    path: String,
    stamp: Option<Stamp>,
    owner: Option<u64>,
    sequence: u64,
    acknowledged: bool,
    removing: bool,
    failure: Option<String>,
    copy: CopyKnowledge,
    submitted: HashMap<u64, OperationKind>,
    retained: bool,
}
impl Tracked {
    fn may_own_copy(&self) -> bool {
        self.copy.state != CopyState::NoOwnedCopy
            || self
                .submitted
                .values()
                .any(|kind| *kind == OperationKind::Write)
    }
}
#[derive(Default)]
pub struct Recovery {
    pub enabled: bool,
    pub visible: bool,
    pub initialized: bool,
    pub loading: bool,
    pub error: Option<String>,
    pub issues: Vec<(String, String)>,
    pub drafts: Vec<DraftMetadata>,
    pub pending_restore: Option<Draft>,
    pub restoring_generation: Option<u64>,
    pub remove_confirmation: Option<RecordId>,
    pub closing: Option<CloseTransaction>,
    pub language_close_guard: Option<CloseGuard>,
    pub reading: Option<RecordId>,
    actor: Option<Actor>,
    path: Option<PathBuf>,
    tracked: HashMap<RecordId, Tracked>,
    sequence: u64,
    generation: u64,
    listing_epoch: u64,
    ticket: u64,
    resuming: Option<Ticket>,
    retry_removals: bool,
    initial_notice: bool,
}
impl Recovery {
    /// Read only bookkeeping: this reports known collisions, never filesystem
    /// absence, and never adopts or authorizes a retained record for a new tab.
    pub(crate) fn known_path_collision(&self, workspace: &WorkspaceIdentity, path: &str) -> bool {
        let Ok(id) = record_id(workspace, path) else {
            return true;
        };
        self.drafts.iter().any(|draft| draft.id == id)
            // Store listings retain exact `<record-id>.draft` names for
            // damaged/unsafe records whose metadata could not be inspected.
            || self.issues.iter().any(|(name, _)| name == &format!("{id}.draft"))
            || self
                .pending_restore
                .as_ref()
                .is_some_and(|draft| &draft.workspace == workspace && draft.path == path)
            || self.reading.as_ref() == Some(&id)
            || self.remove_confirmation.as_ref() == Some(&id)
            || self.tracked.get(&id).is_some_and(|tracked| {
                tracked.may_own_copy()
                    || tracked.removing
                    || tracked.retained
                    || !tracked.submitted.is_empty()
            })
    }

    pub fn start(&mut self, path: Result<PathBuf, String>, ctx: &egui::Context) {
        self.enabled = true;
        self.initial_notice = true;
        match path {
            Ok(path) => {
                self.generation += 1;
                self.listing_epoch = 1;
                self.initialized = false;
                self.actor = Some(Actor::spawn(path.clone(), ctx.clone(), self.generation));
                self.path = Some(path);
                self.loading = true;
            }
            Err(error) => {
                self.error = Some(error);
                self.visible = true;
            }
        }
    }
    pub fn location(&self) -> String {
        self.path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "Unavailable".into())
    }
    pub fn availability(&self) -> Availability {
        self.actor.as_ref().map_or_else(
            || {
                Availability::Unavailable(
                    self.error
                        .clone()
                        .unwrap_or_else(|| "Recovery storage is unavailable".into()),
                )
            },
            Actor::availability,
        )
    }
    fn next_sequence(&mut self) -> u64 {
        self.sequence = self
            .sequence
            .checked_add(1)
            .expect("recovery sequence exhausted");
        self.sequence
    }
    fn empty_tracking(&self, workspace: WorkspaceIdentity, path: String) -> Tracked {
        Tracked {
            workspace,
            path,
            stamp: None,
            owner: None,
            sequence: 0,
            acknowledged: false,
            removing: false,
            failure: None,
            copy: CopyKnowledge::default(),
            submitted: HashMap::new(),
            retained: false,
        }
    }
    pub fn poll(&mut self) -> Option<Draft> {
        let actor = self.actor.as_ref()?;
        let results = actor.poll();
        let finished = actor.finished();
        if finished {
            self.error = Some("Recovery worker stopped without a close proof. Keep editing and choose Retry recovery".into());
            self.loading = false;
            self.resuming = None;
            if let Some(close) = &mut self.closing {
                if !matches!(
                    close.phase,
                    ClosePhase::Confirmed { .. } | ClosePhase::AwaitingConfirmation { .. }
                ) {
                    close.phase = ClosePhase::Blocked { ticket: None };
                }
            }
            // A panic after mutation but before its acknowledgement is uncertain.
            for tracked in self.tracked.values_mut() {
                if !tracked.submitted.is_empty() {
                    tracked.copy.state = CopyState::Uncertain;
                }
            }
        }
        if let Some((epoch, listing)) = results
            .listing
            .filter(|(epoch, _)| *epoch == self.listing_epoch)
        {
            let _ = epoch;
            self.loading = false;
            match listing {
                Ok(listing) => {
                    self.drafts = listing.drafts;
                    self.issues = listing
                        .issues
                        .into_iter()
                        .map(|issue| (issue.name, issue.message))
                        .collect();
                    self.initialized = true;
                    if !finished {
                        self.error = None;
                    }
                    if self.initial_notice && (!self.drafts.is_empty() || !self.issues.is_empty()) {
                        self.visible = true;
                    }
                    self.initial_notice = false;
                }
                Err(error) => {
                    self.error = Some(error);
                    self.visible = true;
                }
            }
        }
        let mut refresh = false;
        for (id, ack) in results.acks {
            refresh |= self.acknowledge(&id, ack);
        }
        if refresh && self.closing.is_none() && self.resuming.is_none() {
            self.refresh(false);
        }
        if let Some(settlement) = results.settlement {
            match settlement {
                Settlement::Quiescent(ticket) => {
                    if let Some(close) = &mut self.closing {
                        if let ClosePhase::Draining {
                            ticket: expected,
                            retain,
                            started,
                        } = close.phase
                        {
                            // A late proof cannot re-arm an expired close request.
                            if ticket == expected && started.elapsed() < CLOSE_OBSERVATION {
                                close.phase = if retain {
                                    ClosePhase::AwaitingConfirmation { ticket }
                                } else {
                                    ClosePhase::Confirmed { ticket }
                                };
                            }
                        }
                    }
                }
                Settlement::Resumed(ticket) if self.resuming == Some(ticket) => {
                    self.resuming = None;
                    for item in self.tracked.values_mut().filter(|item| !item.removing) {
                        item.stamp = None;
                        item.failure = None;
                    }
                    // A queued initial/retry listing may have been canceled by
                    // the fence. Restart observation only after this ticket settles.
                    if !self.initialized && !finished {
                        self.refresh(false);
                    }
                }
                Settlement::Resumed(_) => {}
            }
        }
        if let Availability::Unavailable(error) = self.availability() {
            self.error = Some(error);
        }
        if self.retry_removals
            && self.availability() == Availability::Ready
            && self.closing.is_none()
            && self.resuming.is_none()
        {
            self.retry_removals = false;
            let removals: Vec<_> = self
                .tracked
                .values()
                .filter(|item| item.removing && !item.retained)
                .map(|item| (item.workspace.clone(), item.path.clone()))
                .collect();
            for (workspace, path) in removals {
                self.remove(workspace, path);
            }
        }
        self.observe_close_deadline();
        if let Some((id, result)) = results.read {
            if self.reading.as_ref() != Some(&id) {
                return None;
            }
            self.reading = None;
            match result {
                Ok(draft) => return Some(draft),
                Err(error) => {
                    self.error = Some(error);
                    self.visible = true;
                }
            }
        }
        None
    }
    fn acknowledge(&mut self, id: &RecordId, ack: Ack) -> bool {
        if ack.generation != self.generation {
            return false;
        }
        let Some(tracked) = self.tracked.get_mut(id) else {
            return false;
        };
        tracked.submitted.remove(&ack.sequence);
        if ack.kind == OperationKind::Write && ack.effect == Effect::Applied {
            tracked.copy.last_applied_write = Some(
                tracked
                    .copy
                    .last_applied_write
                    .map_or(ack.sequence, |previous| previous.max(ack.sequence)),
            );
        }
        // Stale acknowledgements still describe real disk effects. Newer rejected
        // intent never erases them; only a later invoked effect can supersede them.
        if ack.sequence >= tracked.copy.sequence {
            match &ack.effect {
                Effect::Applied => {
                    tracked.copy.sequence = ack.sequence;
                    tracked.copy.state = if ack.kind == OperationKind::Write {
                        CopyState::Present
                    } else {
                        CopyState::NoOwnedCopy
                    };
                    if ack.kind == OperationKind::Remove {
                        // This is a disk fact, even when Keep editing has already
                        // canceled the intent represented by this acknowledgement.
                        self.drafts.retain(|draft| &draft.id != id);
                    }
                }
                Effect::PossiblyApplied(_) => {
                    tracked.copy.sequence = ack.sequence;
                    tracked.copy.state = CopyState::Uncertain;
                }
                Effect::NotInvoked(_) => {}
            }
        }
        if tracked.sequence != ack.sequence {
            return false;
        }
        match ack.effect {
            Effect::Applied => {
                tracked.acknowledged = true;
                tracked.failure = None;
                if tracked.removing {
                    self.drafts.retain(|draft| &draft.id != id);
                }
                true
            }
            Effect::NotInvoked(error) | Effect::PossiblyApplied(error) => {
                tracked.acknowledged = false;
                tracked.failure = Some(error);
                self.visible = true;
                false
            }
        }
    }
    pub fn refresh(&mut self, retry: bool) {
        if self.closing.is_none() && self.resuming.is_none() {
            if let Some(actor) = &self.actor {
                if let Some(epoch) = actor.refresh(retry) {
                    self.listing_epoch = epoch;
                    self.loading = true;
                    if retry {
                        self.initialized = false;
                    }
                }
            }
        }
    }
    fn reset_intents(&mut self) {
        // Invalidate every old acknowledgement without discarding copy knowledge.
        let ids: Vec<_> = self.tracked.keys().cloned().collect();
        for id in ids {
            let sequence = self.next_sequence();
            let item = self.tracked.get_mut(&id).unwrap();
            item.sequence = sequence;
            item.acknowledged = false;
            item.stamp = None;
            item.failure = None;
            if item.retained {
                item.removing = false;
            }
        }
    }
    pub fn retry(&mut self, ctx: &egui::Context) {
        if !self.can_retry() {
            return;
        }
        if self.actor.as_ref().is_none_or(Actor::finished) {
            // Observe any terminal effects before replacing this generation.
            let _ = self.poll();
            self.actor = None;
            let path = match self.path.clone().map(Ok).unwrap_or_else(|| {
                cedar_recovery::default_store_path().map_err(|error| error.to_string())
            }) {
                Ok(path) => path,
                Err(error) => {
                    self.error = Some(error);
                    return;
                }
            };
            self.path = Some(path.clone());
            self.generation += 1;
            self.listing_epoch = 1;
            self.initialized = false;
            self.actor = Some(Actor::spawn(path, ctx.clone(), self.generation));
            self.resuming = None;
            for item in self.tracked.values_mut() {
                item.submitted.clear();
            }
        }
        self.reset_intents();
        self.error = None;
        self.retry_removals = true;
        self.refresh(true);
        // Failed explicit removals are retried only after storage is ready, and
        // only if the user has not chosen to retain those copies.
    }
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            if let Some(actor) = &self.actor {
                actor.cancel_writes();
            }
        } else {
            for item in self
                .tracked
                .values_mut()
                .filter(|item| !item.removing && !item.acknowledged)
            {
                item.stamp = None;
                item.failure = None;
            }
        }
    }
    pub fn request_restore(&mut self, id: RecordId) {
        if self.reading.is_none() && self.closing.is_none() && self.resuming.is_none() {
            if let Some(actor) = &self.actor {
                actor.read(id.clone());
                self.reading = Some(id);
            }
        }
    }
    pub fn authorize(&mut self, workspace: &WorkspaceIdentity, doc: &Document) {
        let Ok(id) = record_id(workspace, &doc.path) else {
            return;
        };
        let sequence = self.next_sequence();
        let mut tracked = self.tracked.remove(&id).unwrap_or_else(|| {
            let mut tracked = self.empty_tracking(workspace.clone(), doc.path.clone());
            if self.drafts.iter().any(|draft| draft.id == id) {
                tracked.copy.state = CopyState::Present;
            }
            tracked
        });
        // Explicit restore adopts the reviewed copy even if an earlier rejected
        // intent already created bookkeeping for this path.
        if self.drafts.iter().any(|draft| draft.id == id)
            && tracked.copy.state == CopyState::NoOwnedCopy
        {
            tracked.copy.state = CopyState::Present;
        }
        tracked.owner = Some(doc.id);
        tracked.sequence = sequence;
        tracked.stamp = None;
        tracked.removing = false;
        tracked.failure = None;
        tracked.acknowledged = false;
        self.tracked.insert(id, tracked);
    }
    pub fn observe(&mut self, workspace: &WorkspaceIdentity, doc: &Document) {
        if !self.enabled
            || !self.initialized
            || self.closing.is_some()
            || self.resuming.is_some()
            || matches!(self.availability(), Availability::Starting)
        {
            return;
        }
        let id = match record_id(workspace, &doc.path) {
            Ok(id) => id,
            Err(error) => {
                self.error = Some(error.to_string());
                return;
            }
        };
        if self.tracked.len() >= 256 && !self.tracked.contains_key(&id) {
            self.error = Some("Too many pending recovery records. Retry existing failures before creating more copies".into());
            return;
        }
        let stamp = Stamp::of(doc);
        if let Some(tracked) = self.tracked.get(&id) {
            // A retained/uncertain copy belongs to its exact document, not a
            // later tab that happens to open the same workspace path.
            if tracked.owner != Some(doc.id) && tracked.may_own_copy() {
                return;
            }
            if tracked.stamp.as_ref() == Some(&stamp) || tracked.failure.is_some() {
                return;
            }
            if !tracked.may_own_copy() && self.drafts.iter().any(|draft| draft.id == id) {
                return;
            }
        } else if !doc.dirty() || self.drafts.iter().any(|draft| draft.id == id) {
            return;
        }
        if !doc.dirty() {
            self.discard_owned(workspace, doc);
            if let Some(tracked) = self.tracked.get_mut(&id) {
                tracked.stamp = Some(stamp);
            }
            return;
        }
        if doc.text.len() > cedar_recovery::MAX_TEXT_BYTES
            || doc.saved_text.len() > cedar_recovery::MAX_TEXT_BYTES
        {
            self.error = Some("A draft exceeds the 1 MiB recovery limit. Shorten it or copy it somewhere safe; editing is still available".into());
            return;
        }
        let draft = Draft {
            workspace: workspace.clone(),
            path: doc.path.clone(),
            text: doc.text.clone(),
            base_text: doc.saved_text.clone(),
            base_revision: doc.revision.clone(),
            modified_ms: timestamp_ms(),
        };
        let sequence = self.next_sequence();
        let failure = self
            .actor
            .as_ref()
            .ok_or_else(|| "Recovery storage is unavailable. Choose Retry recovery".into())
            .and_then(|actor| actor.submit(sequence, Mutation::Write(draft)).map(|_| ()))
            .err();
        let mut tracked = self
            .tracked
            .remove(&id)
            .unwrap_or_else(|| self.empty_tracking(workspace.clone(), doc.path.clone()));
        tracked.stamp = Some(stamp);
        tracked.owner = Some(doc.id);
        tracked.sequence = sequence;
        tracked.acknowledged = false;
        tracked.removing = false;
        if failure.is_none() {
            tracked.submitted.insert(sequence, OperationKind::Write);
            // After Keep editing, a dirty document resumes ownership through
            // a newly admitted backup. Clean/closed retained copies stay pinned.
            tracked.retained = false;
        }
        tracked.failure = failure;
        self.tracked.insert(id, tracked);
    }
    pub fn remove(&mut self, workspace: WorkspaceIdentity, path: String) {
        let id = match record_id(&workspace, &path) {
            Ok(id) => id,
            Err(error) => {
                self.error = Some(error.to_string());
                return;
            }
        };
        let known = self.drafts.iter().any(|draft| draft.id == id);
        if !known && !self.tracked.get(&id).is_some_and(Tracked::may_own_copy) {
            return;
        }
        let sequence = self.next_sequence();
        let failure = self
            .actor
            .as_ref()
            .ok_or_else(|| "Recovery storage is unavailable. Retry to remove this copy".into())
            .and_then(|actor| {
                actor
                    .submit(
                        sequence,
                        Mutation::Remove {
                            workspace: workspace.clone(),
                            path: path.clone(),
                        },
                    )
                    .map(|_| ())
            })
            .err();
        let mut tracked = self
            .tracked
            .remove(&id)
            .unwrap_or_else(|| self.empty_tracking(workspace, path));
        if known && tracked.copy.sequence == 0 {
            tracked.copy.state = CopyState::Present;
        }
        tracked.sequence = sequence;
        tracked.stamp = None;
        tracked.acknowledged = false;
        tracked.removing = true;
        tracked.retained = false;
        if failure.is_none() {
            tracked.submitted.insert(sequence, OperationKind::Remove);
        }
        tracked.failure = failure;
        self.tracked.insert(id, tracked);
    }
    pub fn discard_owned(&mut self, workspace: &WorkspaceIdentity, doc: &Document) {
        if self.owns(workspace, doc) {
            self.remove(workspace.clone(), doc.path.clone());
        }
    }
    fn owns(&self, workspace: &WorkspaceIdentity, doc: &Document) -> bool {
        record_id(workspace, &doc.path)
            .ok()
            .and_then(|id| self.tracked.get(&id))
            .is_some_and(|item| item.owner == Some(doc.id) && !item.retained && item.may_own_copy())
    }
    pub fn saved(&mut self, workspace: &WorkspaceIdentity, doc: &Document) {
        if !self.owns(workspace, doc) {
            return;
        }
        if doc.dirty() {
            if self.enabled {
                self.authorize(workspace, doc);
                self.observe(workspace, doc);
            }
        } else {
            self.remove(workspace.clone(), doc.path.clone());
        }
    }
    pub fn forget_completed(&mut self, live: &[u64]) {
        if self.closing.is_none() {
            self.tracked.retain(|_, item| {
                !(item.removing
                    && item.acknowledged
                    && item.submitted.is_empty()
                    && item.owner.is_none_or(|id| !live.contains(&id)))
            });
        }
    }
    pub fn protected(&self, workspace: &WorkspaceIdentity, doc: &Document) -> bool {
        record_id(workspace, &doc.path)
            .ok()
            .and_then(|id| self.tracked.get(&id))
            .is_some_and(|item| {
                item.acknowledged && !item.removing && item.stamp.as_ref() == Some(&Stamp::of(doc))
            })
    }
    pub fn status(
        &self,
        workspace: Option<&WorkspaceIdentity>,
        doc: Option<&Document>,
    ) -> (&'static str, bool) {
        if !self.enabled {
            return ("Recovery off", false);
        }
        if self.error.is_some()
            || !self.issues.is_empty()
            || self.tracked.values().any(|item| item.failure.is_some())
        {
            return ("Recovery needs attention", false);
        }
        if self.resuming.is_some() {
            return ("Recovery resuming", false);
        }
        if let (Some(workspace), Some(doc)) = (workspace, doc) {
            if doc.dirty() {
                if self.protected(workspace, doc) {
                    return ("Draft backed up locally", true);
                }
                if record_id(workspace, &doc.path).ok().is_some_and(|id| {
                    self.drafts.iter().any(|draft| draft.id == id)
                        && !self
                            .tracked
                            .get(&id)
                            .is_some_and(|item| item.owner == Some(doc.id) && item.may_own_copy())
                }) {
                    return ("Older recovery waiting", false);
                }
                return ("Recovery pending", false);
            }
        }
        if !self.initialized {
            ("Recovery starting", false)
        } else {
            ("Local recovery on", true)
        }
    }
    pub fn failures(&self) -> Vec<(String, String)> {
        self.tracked
            .values()
            .filter_map(|item| {
                item.failure
                    .as_ref()
                    .map(|error| (item.path.clone(), error.clone()))
            })
            .collect()
    }
    pub fn removals_finished(&self) -> bool {
        self.tracked
            .values()
            .filter(|item| item.removing && !item.retained)
            .all(|item| item.acknowledged)
    }
    pub fn close_decision_message(&self) -> &'static str {
        // Display only: no owned removal intent is not proof that storage is empty.
        if !self.removals_finished() {
            "Requested recovery copy removals could not be verified. Copies may remain. Your editor text remains open."
        } else if !self.initialized || self.availability() != Availability::Ready {
            "Recovery storage is unavailable or could not be inspected. Older copies may exist. Your editor text remains open."
        } else {
            "Review is still required before quitting. Recovery copies may remain. Your editor text remains open."
        }
    }
    pub fn begin_close(&mut self, guard: CloseGuard) {
        let phase = if self.resuming.is_some()
            || (self.enabled && self.availability() != Availability::Ready)
        {
            self.visible = true;
            ClosePhase::NeedsDecision
        } else {
            ClosePhase::Discarding {
                started: Instant::now(),
            }
        };
        self.closing = Some(CloseTransaction { guard, phase });
    }
    pub fn observe_close_deadline(&mut self) {
        let Some(close) = &mut self.closing else {
            return;
        };
        match close.phase {
            ClosePhase::Discarding { started }
                if started.elapsed() >= CLOSE_OBSERVATION
                    || self
                        .tracked
                        .values()
                        .any(|item| item.removing && item.failure.is_some()) =>
            {
                close.phase = ClosePhase::NeedsDecision;
                self.visible = true;
            }
            ClosePhase::Draining {
                ticket, started, ..
            } if started.elapsed() >= CLOSE_OBSERVATION => {
                close.phase = ClosePhase::Blocked {
                    ticket: Some(ticket),
                };
                self.error = Some("Recovery has not settled within 5 seconds. Quitting is blocked; keep editing or retry after the operation finishes. Current unsaved text is still here".into());
                self.visible = true;
            }
            _ => {}
        }
    }
    pub fn begin_quiescence(&mut self, retain: bool) {
        if self.closing.is_none() {
            return;
        }
        if self.resuming.is_some() {
            self.error = Some("An earlier recovery operation is still settling. Keep editing and wait before trying to quit again".into());
            self.closing.as_mut().unwrap().phase = ClosePhase::Blocked { ticket: None };
            return;
        }
        self.ticket += 1;
        let ticket = Ticket {
            generation: self.generation,
            serial: self.ticket,
        };
        if retain {
            self.retain_copies();
        }
        let phase = match &self.actor {
            Some(actor) => match actor.quiesce(ticket) {
                Ok(()) => ClosePhase::Draining {
                    ticket,
                    retain,
                    started: Instant::now(),
                },
                Err(error) => {
                    self.error = Some(error);
                    ClosePhase::Blocked { ticket: None }
                }
            },
            None => {
                if retain {
                    ClosePhase::AwaitingConfirmation { ticket }
                } else {
                    ClosePhase::Confirmed { ticket }
                }
            }
        };
        self.closing.as_mut().unwrap().phase = phase;
        self.visible = retain;
        self.loading = false;
        self.reading = None;
    }
    fn retain_copies(&mut self) {
        for item in self.tracked.values_mut() {
            item.retained = true;
            item.removing = false;
            item.acknowledged = false;
            item.stamp = None;
            item.failure = None;
        }
    }
    pub fn keep_editing(&mut self) {
        let Some(close) = self.closing.take() else {
            return;
        };
        // The fence cancels queued read/list intents. Clear only the intents
        // belonging to this canceled close, never a later request after resume.
        self.reading = None;
        self.loading = false;
        self.retain_copies();
        let ticket = match close.phase {
            ClosePhase::Draining { ticket, .. }
            | ClosePhase::AwaitingConfirmation { ticket }
            | ClosePhase::Confirmed { ticket }
            | ClosePhase::Blocked {
                ticket: Some(ticket),
            } => Some(ticket),
            _ => {
                self.ticket += 1;
                let ticket = Ticket {
                    generation: self.generation,
                    serial: self.ticket,
                };
                self.actor
                    .as_ref()
                    .filter(|actor| actor.quiesce(ticket).is_ok())
                    .map(|_| ticket)
            }
        };
        if let (Some(actor), Some(ticket)) = (&self.actor, ticket) {
            if !actor.finished() {
                actor.resume(ticket);
                self.resuming = Some(ticket);
            }
        }
    }
    pub fn can_retry(&self) -> bool {
        self.closing.is_none() && self.resuming.is_none()
    }
    pub fn resuming(&self) -> bool {
        self.resuming.is_some()
    }
    pub fn flush(&self) {
        if let Some(actor) = &self.actor {
            actor.flush();
        }
    }
    #[cfg(test)]
    pub fn has_actor(&self) -> bool {
        self.actor.is_some()
    }
    #[cfg(test)]
    pub fn hold_next_operation(
        &self,
        panic_after: bool,
    ) -> (
        std::sync::mpsc::Receiver<()>,
        std::sync::mpsc::SyncSender<()>,
    ) {
        self.actor
            .as_ref()
            .unwrap()
            .hold_next_operation(panic_after)
    }
    #[cfg(test)]
    pub fn hold_next_read(
        &self,
    ) -> (
        std::sync::mpsc::Receiver<()>,
        std::sync::mpsc::SyncSender<()>,
    ) {
        self.actor.as_ref().unwrap().hold_next_read()
    }
    #[cfg(test)]
    pub fn test_snapshot(&self) -> (usize, usize, usize) {
        (
            self.actor.as_ref().map_or(0, Actor::queued),
            self.tracked
                .values()
                .filter(|item| item.may_own_copy())
                .count(),
            self.tracked
                .values()
                .filter(|item| item.removing && item.acknowledged)
                .count(),
        )
    }
}
fn timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}
#[cfg(test)]
#[path = "recovery_state_tests.rs"]
mod tests;
