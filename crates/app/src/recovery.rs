//! Recovery session bookkeeping. Storage acknowledgements never certify newer text.
use crate::{
    model::Document,
    recovery_actor::{Ack, Actor, Mutation},
};
use cedar_recovery::{record_id, Draft, DraftMetadata, RecordId, WorkspaceIdentity};
use eframe::egui;
use std::{
    collections::HashMap,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

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
struct Tracked {
    workspace: WorkspaceIdentity,
    path: String,
    stamp: Option<Stamp>,
    owner: Option<u64>,
    sequence: u64,
    acknowledged: bool,
    removing: bool,
    failure: Option<String>,
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
    pub closing: Option<Vec<(u64, u64)>>,
    pub reading: Option<RecordId>,
    actor: Option<Actor>,
    path: Option<PathBuf>,
    tracked: HashMap<RecordId, Tracked>,
    sequence: u64,
    initial_notice: bool,
}
impl Recovery {
    pub fn start(&mut self, path: Result<PathBuf, String>, ctx: &egui::Context) {
        self.enabled = true;
        self.initial_notice = true;
        match path {
            Ok(path) => {
                self.actor = Some(Actor::spawn(path.clone(), ctx.clone()));
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
    fn next_sequence(&mut self) -> u64 {
        self.sequence = self
            .sequence
            .checked_add(1)
            .expect("recovery sequence exhausted");
        self.sequence
    }
    pub fn poll(&mut self) -> Option<Draft> {
        let Some(actor) = &self.actor else {
            return None;
        };
        let results = actor.poll();
        if actor.finished() {
            self.error =
                Some("Recovery worker stopped. Choose Retry recovery to restart it".into());
        }
        if let Some(listing) = results.listing {
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
                    self.error = None;
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
        if refresh {
            self.refresh(false);
        }
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
        let Some(tracked) = self.tracked.get_mut(id) else {
            return false;
        };
        if tracked.sequence != ack.sequence {
            return false;
        }
        match ack.result {
            Ok(()) => {
                tracked.acknowledged = true;
                tracked.failure = None;
                if tracked.removing {
                    self.drafts.retain(|draft| &draft.id != id);
                }
                true
            }
            Err(error) => {
                tracked.acknowledged = false;
                tracked.failure = Some(error);
                self.visible = true;
                false
            }
        }
    }
    pub fn refresh(&mut self, retry: bool) {
        if let Some(actor) = &self.actor {
            actor.refresh(retry);
            self.loading = true;
        }
    }
    pub fn retry(&mut self, ctx: &egui::Context) {
        if self.actor.as_ref().is_none_or(Actor::finished) {
            // A stopped actor releases its file lock before a replacement starts.
            self.actor = None;
            if let Some(path) = self.path.clone() {
                self.actor = Some(Actor::spawn(path, ctx.clone()));
            } else {
                match cedar_recovery::default_store_path() {
                    Ok(path) => {
                        self.path = Some(path.clone());
                        self.actor = Some(Actor::spawn(path, ctx.clone()));
                    }
                    Err(error) => {
                        self.error = Some(error.to_string());
                        return;
                    }
                }
            }
            for tracked in self.tracked.values_mut() {
                tracked.acknowledged = false;
            }
        }
        self.error = None;
        self.refresh(true);
        let removals: Vec<_> = self
            .tracked
            .values()
            .filter(|item| item.removing && !item.acknowledged)
            .map(|item| (item.workspace.clone(), item.path.clone()))
            .collect();
        for item in self
            .tracked
            .values_mut()
            .filter(|item| !item.removing && !item.acknowledged)
        {
            item.stamp = None;
            item.failure = None;
        }
        for (workspace, path) in removals {
            self.remove(workspace, path);
        }
    }
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            if let Some(actor) = &self.actor {
                actor.cancel_writes();
            }
        } else {
            for tracked in self
                .tracked
                .values_mut()
                .filter(|item| !item.removing && !item.acknowledged)
            {
                tracked.stamp = None;
            }
        }
    }
    pub fn request_restore(&mut self, id: RecordId) {
        if self.reading.is_none() {
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
        self.tracked.insert(
            id,
            Tracked {
                workspace: workspace.clone(),
                path: doc.path.clone(),
                stamp: None,
                owner: Some(doc.id),
                sequence,
                acknowledged: false,
                removing: false,
                failure: None,
            },
        );
    }
    pub fn observe(&mut self, workspace: &WorkspaceIdentity, doc: &Document) {
        if !self.enabled || !self.initialized || self.closing.is_some() {
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
            if tracked.stamp.as_ref() == Some(&stamp) || tracked.failure.is_some() {
                return;
            }
        } else {
            if !doc.dirty() {
                return;
            }
            if self.drafts.iter().any(|draft| draft.id == id) {
                return;
            }
        }
        if !doc.dirty() {
            self.remove(workspace.clone(), doc.path.clone());
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
        self.tracked.insert(
            id,
            Tracked {
                workspace: workspace.clone(),
                path: doc.path.clone(),
                stamp: Some(stamp),
                owner: Some(doc.id),
                sequence,
                acknowledged: false,
                removing: false,
                failure,
            },
        );
    }
    pub fn remove(&mut self, workspace: WorkspaceIdentity, path: String) {
        let id = match record_id(&workspace, &path) {
            Ok(id) => id,
            Err(error) => {
                self.error = Some(error.to_string());
                return;
            }
        };
        let owner = self.tracked.get(&id).and_then(|tracked| tracked.owner);
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
        self.tracked.insert(
            id,
            Tracked {
                workspace,
                path,
                stamp: None,
                owner,
                sequence,
                acknowledged: false,
                removing: true,
                failure,
            },
        );
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
            .is_some_and(|item| item.owner == Some(doc.id))
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
        if let (Some(workspace), Some(doc)) = (workspace, doc) {
            if doc.dirty() {
                if self.protected(workspace, doc) {
                    return ("Draft backed up locally", true);
                }
                if record_id(workspace, &doc.path).ok().is_some_and(|id| {
                    self.drafts.iter().any(|draft| draft.id == id)
                        && !self.tracked.contains_key(&id)
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
            .filter(|item| item.removing)
            .all(|item| item.acknowledged)
    }
    pub fn has_store(&self) -> bool {
        self.actor.is_some()
    }
    pub fn flush(&self) {
        if let Some(actor) = &self.actor {
            actor.flush();
        }
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
mod tests {
    use super::*;
    fn workspace() -> WorkspaceIdentity {
        WorkspaceIdentity::Local {
            root: "/synthetic".into(),
        }
    }
    fn doc() -> Document {
        let mut doc = Document::new(1, "file.rs".into(), "base".into(), "r0".into());
        doc.text = "draft".into();
        doc.edit_version = 1;
        doc
    }
    #[test]
    fn stale_write_and_remove_acknowledgements_cannot_certify_newer_state() {
        let mut recovery = Recovery::default();
        let mut doc = doc();
        recovery.authorize(&workspace(), &doc);
        let id = record_id(&workspace(), &doc.path).unwrap();
        let write_sequence = recovery.tracked[&id].sequence;
        recovery.tracked.get_mut(&id).unwrap().stamp = Some(Stamp::of(&doc));
        doc.text.push_str(" newer");
        doc.edit_version += 1;
        recovery.authorize(&workspace(), &doc);
        let remove_sequence = recovery.tracked[&id].sequence;
        recovery.tracked.get_mut(&id).unwrap().removing = true;
        assert!(!recovery.acknowledge(
            &id,
            Ack {
                sequence: write_sequence,
                result: Ok(())
            }
        ));
        assert!(!recovery.tracked[&id].acknowledged);
        recovery.authorize(&workspace(), &doc);
        recovery.tracked.get_mut(&id).unwrap().stamp = Some(Stamp::of(&doc));
        assert!(!recovery.acknowledge(
            &id,
            Ack {
                sequence: remove_sequence,
                result: Ok(())
            }
        ));
        assert!(!recovery.protected(&workspace(), &doc));
        let latest = recovery.tracked[&id].sequence;
        assert!(recovery.acknowledge(
            &id,
            Ack {
                sequence: latest,
                result: Ok(())
            }
        ));
        assert!(recovery.protected(&workspace(), &doc));
        assert!(!recovery.acknowledge(
            &id,
            Ack {
                sequence: write_sequence,
                result: Err("stale failure".into())
            }
        ));
        assert!(recovery.failures().is_empty());
    }
    #[test]
    fn protection_never_follows_same_path_to_other_workspace_or_document() {
        let mut recovery = Recovery::default();
        let mut doc = doc();
        recovery.authorize(&workspace(), &doc);
        let id = record_id(&workspace(), &doc.path).unwrap();
        let sequence = recovery.tracked[&id].sequence;
        recovery.tracked.get_mut(&id).unwrap().stamp = Some(Stamp::of(&doc));
        recovery.acknowledge(
            &id,
            Ack {
                sequence,
                result: Ok(()),
            },
        );
        assert!(recovery.protected(&workspace(), &doc));
        assert!(!recovery.protected(
            &WorkspaceIdentity::Local {
                root: "/another".into()
            },
            &doc
        ));
        doc.id = 2;
        assert!(!recovery.protected(&workspace(), &doc));
    }
}
