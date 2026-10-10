use super::*;
fn draft(text: &str) -> Draft {
    Draft {
        workspace: WorkspaceIdentity::Local {
            root: "/synthetic".into(),
        },
        path: "main.rs".into(),
        text: text.into(),
        base_text: "base".into(),
        base_revision: Some("r0".into()),
        modified_ms: 1,
    }
}
fn pending(sequence: u64, text: &str, due: u64) -> Pending {
    Pending {
        sequence,
        mutation: Mutation::Write(draft(text)),
        due: Duration::from_secs(due),
    }
}
#[test]
fn fake_clock_coalesces_latest_snapshot_and_debounces() {
    let mut queue = Queue::default();
    let id = record_id(&draft("").workspace, "main.rs").unwrap();
    queue.put(id.clone(), pending(1, "first", 1)).unwrap();
    queue.put(id.clone(), pending(2, "newer", 2)).unwrap();
    assert!(queue.put(id.clone(), pending(1, "stale", 0)).is_err());
    assert_eq!(queue.items.len(), 1);
    assert!(queue
        .take_ready(Duration::from_millis(1999), false)
        .is_none());
    let (_, item) = queue.take_ready(Duration::from_secs(2), false).unwrap();
    assert_eq!(item.sequence, 2);
    assert!(matches!(item.mutation, Mutation::Write(d) if d.text == "newer"));
    assert_eq!(queue.bytes, 0);
}
#[test]
fn removal_supersedes_delayed_write_and_flush_ignores_debounce() {
    let mut queue = Queue::default();
    let draft = draft("draft");
    let id = record_id(&draft.workspace, &draft.path).unwrap();
    queue.put(id.clone(), pending(1, "stale", 100)).unwrap();
    queue
        .put(
            id,
            Pending {
                sequence: 2,
                mutation: Mutation::Remove {
                    workspace: draft.workspace,
                    path: draft.path,
                },
                due: Duration::ZERO,
            },
        )
        .unwrap();
    assert!(matches!(
        queue.take_ready(Duration::ZERO, false).unwrap().1.mutation,
        Mutation::Remove { .. }
    ));
    let id = record_id(&self::draft("").workspace, "other.rs").unwrap();
    queue.put(id, pending(3, "new draft", 100)).unwrap();
    assert!(queue.take_ready(Duration::ZERO, true).is_some());
}
#[test]
fn shutdown_flushes_pending_actual_store_write() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("recovery");
    let actor = Actor::spawn(path.clone(), egui::Context::default(), 1);
    wait_ready(&actor);
    let draft = draft("pending shutdown text");
    actor.submit(1, Mutation::Write(draft.clone())).unwrap();
    drop(actor);
    let store = Store::open(&path).unwrap();
    let id = record_id(&draft.workspace, &draft.path).unwrap();
    assert_eq!(store.read(&id).unwrap().text, "pending shutdown text");
}

fn wait_ready(actor: &Actor) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while actor.availability() != Availability::Ready {
        assert!(
            Instant::now() < deadline,
            "actor did not become ready: {:?}",
            actor.availability()
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn wait_settled(actor: &Actor, expected: Settlement) -> Vec<(RecordId, Ack)> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut acks = Vec::new();
    loop {
        let results = actor.poll();
        acks.extend(results.acks);
        if results.settlement == Some(expected) {
            return acks;
        }
        assert!(Instant::now() < deadline, "actor did not settle");
        std::thread::sleep(Duration::from_millis(1));
    }
}
#[test]
fn failed_open_rejects_admission_without_invoking_a_mutation() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("not-a-directory");
    std::fs::write(&path, "blocker").unwrap();
    let actor = Actor::spawn(path.clone(), egui::Context::default(), 7);
    let deadline = Instant::now() + Duration::from_secs(5);
    while actor.availability() == Availability::Starting {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(matches!(actor.availability(), Availability::Unavailable(_)));
    for sequence in 1..200 {
        assert!(actor
            .submit(sequence, Mutation::Write(draft("unprotected")))
            .is_err());
    }
    assert_eq!(actor.queued(), 0);
    assert!(actor.poll().acks.is_empty());
    let ticket = Ticket {
        generation: 7,
        serial: 1,
    };
    actor.quiesce(ticket).unwrap();
    wait_settled(&actor, Settlement::Quiescent(ticket));
    drop(actor);
    assert_eq!(std::fs::read_to_string(path).unwrap(), "blocker");
}
#[test]
fn freeze_cancels_only_queued_removes_and_keeps_accepted_writes() {
    let mut mailbox = Mailbox::default();
    let id = record_id(&draft("").workspace, "main.rs").unwrap();
    let other = record_id(&draft("").workspace, "other.rs").unwrap();
    mailbox
        .queue
        .put(id.clone(), pending(1, "accepted", 100))
        .unwrap();
    mailbox
        .queue
        .put(
            other.clone(),
            Pending {
                sequence: 2,
                mutation: Mutation::Remove {
                    workspace: draft("").workspace,
                    path: "other.rs".into(),
                },
                due: Duration::ZERO,
            },
        )
        .unwrap();
    let ticket = Ticket {
        generation: 1,
        serial: 9,
    };
    mailbox.freeze(ticket).unwrap();
    assert_eq!(mailbox.queue.items.len(), 1);
    assert!(mailbox.queue.items.contains_key(&id));
    assert!(!mailbox.settle());
    assert_eq!(mailbox.results.acks.len(), 1);
    assert_eq!(mailbox.results.acks[0].0, other);
    assert!(matches!(
        mailbox.results.acks[0].1.effect,
        Effect::NotInvoked(_)
    ));
    mailbox.queue.take_ready(Duration::ZERO, true).unwrap();
    mailbox.settle();
    assert_eq!(
        mailbox.results.settlement,
        Some(Settlement::Quiescent(ticket))
    );
}
#[test]
fn retained_barrier_drains_in_flight_and_queued_writes_before_proof_and_drop() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("recovery");
    let actor = Actor::spawn(path.clone(), egui::Context::default(), 1);
    wait_ready(&actor);
    let (entered, release) = actor.hold_next_operation(false);
    actor
        .submit(1, Mutation::Write(draft("in flight")))
        .unwrap();
    actor.flush();
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    let mut second = draft("queued write survives");
    second.path = "second.rs".into();
    actor.submit(2, Mutation::Write(second.clone())).unwrap();
    actor
        .submit(
            3,
            Mutation::Remove {
                workspace: draft("").workspace,
                path: "main.rs".into(),
            },
        )
        .unwrap();
    let ticket = Ticket {
        generation: 1,
        serial: 1,
    };
    actor.quiesce(ticket).unwrap();
    assert!(actor
        .submit(4, Mutation::Write(draft("must not run")))
        .is_err());
    let canceled = actor.poll();
    assert_eq!(canceled.settlement, None);
    assert!(canceled
        .acks
        .iter()
        .any(|(_, ack)| ack.sequence == 3 && matches!(ack.effect, Effect::NotInvoked(_))));
    release.send(()).unwrap();
    let acks = wait_settled(&actor, Settlement::Quiescent(ticket));
    assert_eq!(
        acks.iter()
            .filter(|(_, ack)| ack.effect == Effect::Applied)
            .count(),
        2
    );
    drop(actor);
    let store = Store::open(path).unwrap();
    assert_eq!(
        store
            .read(&record_id(&draft("").workspace, "main.rs").unwrap())
            .unwrap()
            .text,
        "in flight"
    );
    assert_eq!(
        store
            .read(&record_id(&second.workspace, &second.path).unwrap())
            .unwrap()
            .text,
        "queued write survives"
    );
}
#[test]
fn resume_requested_before_settlement_never_unfences_in_flight_work() {
    let temp = tempfile::tempdir().unwrap();
    let actor = Actor::spawn(temp.path().join("recovery"), egui::Context::default(), 4);
    wait_ready(&actor);
    let (entered, release) = actor.hold_next_operation(false);
    actor.submit(1, Mutation::Write(draft("accepted"))).unwrap();
    actor.flush();
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    let ticket = Ticket {
        generation: 4,
        serial: 8,
    };
    actor.quiesce(ticket).unwrap();
    actor.resume(ticket);
    assert!(actor
        .submit(2, Mutation::Write(draft("too early")))
        .is_err());
    assert!(actor.poll().settlement.is_none());
    release.send(()).unwrap();
    wait_settled(&actor, Settlement::Resumed(ticket));
    actor.submit(3, Mutation::Write(draft("resumed"))).unwrap();
}
#[test]
fn repeated_submissions_without_poll_have_bounded_effect_storage() {
    let temp = tempfile::tempdir().unwrap();
    let actor = Actor::spawn(temp.path().join("recovery"), egui::Context::default(), 1);
    wait_ready(&actor);
    let (entered, release) = actor.hold_next_operation(false);
    actor.submit(1, Mutation::Write(draft("blocked"))).unwrap();
    actor.flush();
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    let mut rejected = 0;
    for sequence in 2..1000 {
        if actor
            .submit(sequence, Mutation::Write(draft("replacement")))
            .is_err()
        {
            rejected += 1;
        }
    }
    assert!(rejected > 0);
    let mailbox = actor.shared.mailbox.lock().unwrap();
    assert!(mailbox.results.acks.len() + mailbox.queue.items.len() < MAX_UNOBSERVED);
    drop(mailbox);
    release.send(()).unwrap();
}
