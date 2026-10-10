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
            generation: 0,
            kind: OperationKind::Write,
            sequence: write_sequence,
            effect: Effect::Applied
        }
    ));
    assert!(!recovery.tracked[&id].acknowledged);
    recovery.authorize(&workspace(), &doc);
    recovery.tracked.get_mut(&id).unwrap().stamp = Some(Stamp::of(&doc));
    assert!(!recovery.acknowledge(
        &id,
        Ack {
            generation: 0,
            kind: OperationKind::Write,
            sequence: remove_sequence,
            effect: Effect::Applied
        }
    ));
    assert!(!recovery.protected(&workspace(), &doc));
    let latest = recovery.tracked[&id].sequence;
    assert!(recovery.acknowledge(
        &id,
        Ack {
            generation: 0,
            kind: OperationKind::Write,
            sequence: latest,
            effect: Effect::Applied
        }
    ));
    assert!(recovery.protected(&workspace(), &doc));
    assert!(!recovery.acknowledge(
        &id,
        Ack {
            generation: 0,
            kind: OperationKind::Write,
            sequence: write_sequence,
            effect: Effect::NotInvoked("stale failure".into())
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
            generation: 0,
            kind: OperationKind::Write,
            sequence,
            effect: Effect::Applied,
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

fn ack(sequence: u64, kind: OperationKind, effect: Effect) -> Ack {
    Ack {
        generation: 0,
        sequence,
        kind,
        effect,
    }
}
fn tracked_recovery() -> (Recovery, Document, RecordId) {
    let mut recovery = Recovery::default();
    let doc = doc();
    recovery.authorize(&workspace(), &doc);
    let id = record_id(&workspace(), &doc.path).unwrap();
    (recovery, doc, id)
}
#[test]
fn rejected_newer_intent_preserves_older_applied_copy_without_certifying_text() {
    let (mut recovery, mut doc, id) = tracked_recovery();
    let previous = recovery.tracked[&id].sequence;
    recovery
        .tracked
        .get_mut(&id)
        .unwrap()
        .submitted
        .insert(previous, OperationKind::Write);
    doc.edit_version += 1;
    recovery.authorize(&workspace(), &doc);
    assert!(!recovery.acknowledge(&id, ack(previous, OperationKind::Write, Effect::Applied)));
    assert_eq!(recovery.tracked[&id].copy.state, CopyState::Present);
    assert!(recovery.owns(&workspace(), &doc));
    assert!(!recovery.protected(&workspace(), &doc));
    let latest = recovery.tracked[&id].sequence;
    recovery.acknowledge(
        &id,
        ack(
            latest,
            OperationKind::Write,
            Effect::NotInvoked("queue full".into()),
        ),
    );
    assert_eq!(recovery.tracked[&id].copy.state, CopyState::Present);
}
#[test]
fn never_invoked_intent_has_no_deletion_authority() {
    let (mut recovery, doc, id) = tracked_recovery();
    let sequence = recovery.tracked[&id].sequence;
    recovery.acknowledge(
        &id,
        ack(
            sequence,
            OperationKind::Write,
            Effect::NotInvoked("unavailable".into()),
        ),
    );
    assert!(!recovery.owns(&workspace(), &doc));
    recovery.discard_owned(&workspace(), &doc);
    assert!(!recovery.tracked[&id].removing);
    assert_eq!(recovery.test_snapshot(), (0, 0, 0));
}
#[test]
fn uncertain_write_and_remove_never_invent_presence_or_absence() {
    let (mut recovery, doc, id) = tracked_recovery();
    let first = recovery.tracked[&id].sequence;
    recovery.acknowledge(&id, ack(first, OperationKind::Write, Effect::Applied));
    recovery.authorize(&workspace(), &doc);
    let second = recovery.tracked[&id].sequence;
    recovery.acknowledge(
        &id,
        ack(
            second,
            OperationKind::Write,
            Effect::PossiblyApplied("sync failed after rename".into()),
        ),
    );
    assert_eq!(recovery.tracked[&id].copy.state, CopyState::Uncertain);
    assert_eq!(recovery.tracked[&id].copy.last_applied_write, Some(first));
    recovery.authorize(&workspace(), &doc);
    let third = recovery.tracked[&id].sequence;
    recovery.acknowledge(
        &id,
        ack(
            third,
            OperationKind::Remove,
            Effect::PossiblyApplied("sync failed after unlink".into()),
        ),
    );
    assert_eq!(recovery.tracked[&id].copy.state, CopyState::Uncertain);
    assert!(!recovery.protected(&workspace(), &doc));
}
#[test]
fn old_write_cannot_resurrect_copy_after_newer_applied_remove() {
    let (mut recovery, doc, id) = tracked_recovery();
    let first = recovery.tracked[&id].sequence;
    recovery.authorize(&workspace(), &doc);
    let second = recovery.tracked[&id].sequence;
    recovery.acknowledge(&id, ack(second, OperationKind::Remove, Effect::Applied));
    recovery.acknowledge(&id, ack(first, OperationKind::Write, Effect::Applied));
    assert_eq!(recovery.tracked[&id].copy.state, CopyState::NoOwnedCopy);
}
#[test]
fn retry_invalidates_old_acks_but_keeps_copy_evidence_and_retained_intent() {
    let (mut recovery, doc, id) = tracked_recovery();
    let first = recovery.tracked[&id].sequence;
    recovery.acknowledge(&id, ack(first, OperationKind::Write, Effect::Applied));
    recovery.tracked.get_mut(&id).unwrap().removing = true;
    let _ = doc;
    recovery.retain_copies();
    recovery.reset_intents();
    recovery.acknowledge(&id, ack(first, OperationKind::Write, Effect::Applied));
    assert_eq!(recovery.tracked[&id].copy.state, CopyState::Present);
    assert!(!recovery.tracked[&id].acknowledged);
    assert!(!recovery.tracked[&id].removing);
    assert!(recovery.tracked[&id].retained);
}
#[test]
fn stale_generation_cannot_change_protection_or_copy_knowledge() {
    let (mut recovery, _, id) = tracked_recovery();
    let sequence = recovery.tracked[&id].sequence;
    let mut stale = ack(sequence, OperationKind::Write, Effect::Applied);
    stale.generation = 9;
    recovery.acknowledge(&id, stale);
    assert_eq!(recovery.tracked[&id].copy.state, CopyState::NoOwnedCopy);
}

fn metadata(id: RecordId, doc: &Document) -> DraftMetadata {
    DraftMetadata {
        id,
        workspace: workspace(),
        path: doc.path.clone(),
        base_revision: doc.revision.clone(),
        modified_ms: 1,
        text_bytes: doc.text.len(),
        base_text_bytes: doc.saved_text.len(),
    }
}
#[test]
fn explicit_restore_adopts_a_listed_copy_after_rejected_only_tracking() {
    let (mut recovery, mut doc, id) = tracked_recovery();
    let sequence = recovery.tracked[&id].sequence;
    recovery.acknowledge(
        &id,
        ack(
            sequence,
            OperationKind::Write,
            Effect::NotInvoked("unavailable".into()),
        ),
    );
    assert!(!recovery.tracked[&id].may_own_copy());
    recovery.drafts.push(metadata(id.clone(), &doc));
    doc.id = 2;
    recovery.authorize(&workspace(), &doc);
    assert!(recovery.owns(&workspace(), &doc));
    assert_eq!(recovery.tracked[&id].copy.state, CopyState::Present);
    assert!(!recovery.protected(&workspace(), &doc));
}
#[test]
fn actor_ready_before_listing_is_consumed_cannot_admit_an_unreviewed_overwrite() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("recovery");
    let doc = doc();
    let draft = Draft {
        workspace: workspace(),
        path: doc.path.clone(),
        text: "older unreviewed".into(),
        base_text: doc.saved_text.clone(),
        base_revision: doc.revision.clone(),
        modified_ms: 1,
    };
    {
        let mut store = cedar_recovery::Store::open(&path).unwrap();
        store.write(1, &draft).unwrap();
    }
    let mut recovery = Recovery::default();
    recovery.start(Ok(path.clone()), &egui::Context::default());
    let deadline = Instant::now() + Duration::from_secs(5);
    while recovery.availability() != Availability::Ready {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(!recovery.initialized);
    recovery.observe(&workspace(), &doc);
    assert_eq!(recovery.test_snapshot(), (0, 0, 0));
    recovery.poll();
    recovery.observe(&workspace(), &doc);
    assert_eq!(recovery.test_snapshot(), (0, 0, 0));
    drop(recovery);
    assert_eq!(
        cedar_recovery::Store::open(path)
            .unwrap()
            .read(&record_id(&workspace(), &doc.path).unwrap())
            .unwrap()
            .text,
        "older unreviewed"
    );
}
#[test]
fn retry_dispatches_an_ownerless_failed_explicit_removal() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("recovery");
    let doc = doc();
    let draft = Draft {
        workspace: workspace(),
        path: doc.path.clone(),
        text: doc.text.clone(),
        base_text: doc.saved_text.clone(),
        base_revision: doc.revision.clone(),
        modified_ms: 1,
    };
    {
        let mut store = cedar_recovery::Store::open(&path).unwrap();
        store.write(1, &draft).unwrap();
    }
    let id = record_id(&workspace(), &doc.path).unwrap();
    let mut recovery = Recovery {
        path: Some(path.clone()),
        enabled: true,
        ..Default::default()
    };
    recovery.drafts.push(metadata(id.clone(), &doc));
    recovery.remove(workspace(), doc.path.clone());
    assert!(recovery.tracked[&id].owner.is_none());
    assert!(recovery.tracked[&id].failure.is_some());
    recovery.retry(&egui::Context::default());
    let deadline = Instant::now() + Duration::from_secs(5);
    while !recovery.removals_finished() {
        recovery.poll();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    drop(recovery);
    assert!(cedar_recovery::Store::open(path)
        .unwrap()
        .list()
        .unwrap()
        .drafts
        .is_empty());
}
#[test]
fn late_applied_write_history_survives_a_newer_uncertain_effect() {
    let (mut recovery, doc, id) = tracked_recovery();
    let first = recovery.tracked[&id].sequence;
    recovery.authorize(&workspace(), &doc);
    let second = recovery.tracked[&id].sequence;
    recovery.acknowledge(
        &id,
        ack(
            second,
            OperationKind::Remove,
            Effect::PossiblyApplied("unknown unlink".into()),
        ),
    );
    recovery.acknowledge(&id, ack(first, OperationKind::Write, Effect::Applied));
    assert_eq!(recovery.tracked[&id].copy.state, CopyState::Uncertain);
    assert_eq!(recovery.tracked[&id].copy.last_applied_write, Some(first));
}
