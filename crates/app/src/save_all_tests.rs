//! Recording-worker coverage of the production queue, conditional save path,
//! response validation, and final-frame guards. No filesystem or agent is used.
use super::*;
use sha2::{Digest, Sha256};

fn revision(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

fn connected(count: usize) -> (CedarApp, Receiver<Command>) {
    let mut app = CedarApp::empty();
    let form = ConnectForm {
        local_root: "/synthetic-save-all".into(),
        allow_run: false,
        ..Default::default()
    };
    app.workspace_key = Some(form.key());
    app.active_form = Some(form);
    app.root = "/synthetic-save-all".into();
    app.state = ConnectionState::Ready;
    app.generation = 7;
    app.open_form = false;
    app.explorer.mode = explorer_tree::Mode::Tree;
    for index in 0..count {
        let id = index as u64 + 1;
        let original = format!("original {id}\n");
        let mut doc = Document::new(
            id,
            format!("file-{id}.txt"),
            original.clone(),
            revision(&original),
        );
        editor_state::commit(&app.editor_ctx, &mut doc, format!("draft {id} é 🐻\r\n"), 3);
        app.documents.push(doc);
    }
    app.active_document = (count > 0).then_some(1);
    app.next_document = count as u64 + 1;
    let (worker, commands) = Worker::recording();
    app.worker = Some(worker);
    (app, commands)
}

fn finish(app: &mut CedarApp) {
    let ctx = app.editor_ctx.clone();
    app.finish_save_all_frame(&ctx);
}

fn idle(commands: &Receiver<Command>) {
    assert!(matches!(
        commands.try_recv(),
        Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected)
    ));
}

fn write(app: &CedarApp, commands: &Receiver<Command>, document: u64) -> Command {
    let command = commands.try_recv().expect("one conditional Write expected");
    let doc = app.documents.iter().find(|doc| doc.id == document).unwrap();
    assert!(
        matches!(&command.op, Operation::Write { path, text, expected_revision }
        if *path == doc.path && *text == doc.text && *expected_revision == doc.revision)
    );
    assert!(
        matches!(app.pending.get(&command.id), Some(Job::Save { document: saved, snapshot, submission: Some(_) })
        if *saved == document && *snapshot == doc.text)
    );
    assert!(doc.saving);
    assert_eq!(
        app.pending
            .values()
            .filter(|job| matches!(job, Job::Save { .. }))
            .count(),
        1
    );
    idle(commands);
    command
}

fn acknowledge(app: &mut CedarApp, command: &Command) {
    let Operation::Write { text, .. } = &command.op else {
        panic!("expected Write")
    };
    app.apply_event(Event {
        generation: app.generation,
        id: command.id,
        connected: true,
        result: Ok(Payload::Written {
            revision: revision(text),
        }),
    });
}

fn summary(app: &CedarApp, acknowledged: usize, failed: usize, unknown: usize, unattempted: usize) {
    assert!(!app.save_all_busy());
    let message = app.save_all_message().expect("completed batch summary");
    for (count, label) in [
        (acknowledged, "acknowledged"),
        (failed, "failed"),
        (unknown, "unknown"),
        (unattempted, "unattempted"),
    ] {
        assert!(message.contains(&format!("{count} {label}")), "{message}");
    }
    assert!(message.len() < 512);
    assert!(!message.contains("file-"));
    assert!(!message.contains("draft 1"));
}

#[test]
fn cohort_of_32_dispatches_one_write_per_verified_reply_and_only_in_finisher() {
    let (mut app, commands) = connected(32);
    app.queue_save_all();
    assert!(app.save_all_busy());
    assert!(app.mutation_pending());
    idle(&commands);
    finish(&mut app);
    for id in 1..=32 {
        let command = write(&app, &commands, id);
        finish(&mut app);
        idle(&commands);
        acknowledge(&mut app, &command);
        idle(&commands);
        // A response alone never releases batch ownership or dispatches.
        assert!(app.save_all_busy());
        finish(&mut app);
    }
    idle(&commands);
    summary(&app, 32, 0, 0, 0);
    assert!(app.documents.iter().all(|doc| !doc.dirty()));
    assert!(!app.mutation_pending());
}

#[test]
fn thirty_third_dirty_buffer_rejects_the_whole_cohort_without_write() {
    let (mut app, commands) = connected(33);
    app.queue_save_all();
    finish(&mut app);
    idle(&commands);
    assert!(!app.save_all_busy());
    assert!(app.error.as_deref().unwrap().contains("at most 32"));
    assert!(app.documents.iter().all(|doc| doc.dirty() && !doc.saving));
}

#[test]
fn full_32_mib_cohort_is_admitted_at_the_exact_per_file_and_total_limits() {
    let (mut app, commands) = connected(32);
    for doc in &mut app.documents {
        doc.text = "x".repeat(cedar_protocol::MAX_FILE_BYTES);
    }
    assert_eq!(
        app.documents
            .iter()
            .map(|doc| doc.text.len())
            .sum::<usize>(),
        32 * 1024 * 1024,
    );
    app.queue_save_all();
    assert!(app.save_all_busy());
    idle(&commands);
    finish(&mut app);
    let first = write(&app, &commands, 1);
    app.cancel_save_all();
    acknowledge(&mut app, &first);
    finish(&mut app);
    idle(&commands);
    summary(&app, 1, 0, 0, 31);
}

#[test]
fn invalid_last_candidate_never_partially_dispatches_earlier_valid_drafts() {
    for path in [
        "", ".", "/", "a//b", "a/./b", "a/../b", "a/", "/a", "a\\b", "a:b", "a\0b",
    ] {
        let (mut app, commands) = connected(3);
        app.documents[2].path = path.into();
        app.queue_save_all();
        finish(&mut app);
        idle(&commands);
        assert!(!app.save_all_busy(), "path: {path:?}");
        assert!(app.error.as_deref().unwrap().contains("No writes sent"));
    }
    for invalid in [
        "oversized draft",
        "oversized baseline",
        "NUL draft",
        "NUL baseline",
        "oversized path",
        "oversized revision",
        "saturated version",
    ] {
        let (mut app, commands) = connected(3);
        let doc = &mut app.documents[2];
        match invalid {
            "oversized draft" => doc.text = "x".repeat(cedar_protocol::MAX_FILE_BYTES + 1),
            "oversized baseline" => doc.saved_text = "x".repeat(cedar_protocol::MAX_FILE_BYTES + 1),
            "NUL draft" => doc.text.push('\0'),
            "NUL baseline" => doc.saved_text.push('\0'),
            "oversized path" => doc.path = "x".repeat(4097),
            "oversized revision" => doc.revision = Some("r".repeat(65)),
            "saturated version" => doc.edit_version = u64::MAX,
            _ => unreachable!(),
        }
        app.queue_save_all();
        finish(&mut app);
        idle(&commands);
        assert!(!app.save_all_busy(), "{invalid}");
        assert!(app.error.as_deref().unwrap().contains("No writes sent"));
    }
}

#[test]
fn queue_requires_ready_writer_identity_and_no_existing_or_uncertain_saves() {
    for invalid in [
        "not ready",
        "no worker",
        "no identity",
        "no write",
        "saving",
        "pending save",
        "unknown",
        "unverifiable",
        "checking",
        "request exhausted",
    ] {
        let (mut app, commands) = connected(2);
        match invalid {
            "not ready" => app.state = ConnectionState::Disconnected,
            "no worker" => app.worker = None,
            "no identity" => app.active_form = None,
            "no write" => {
                app.agent_info = Some(AgentInfo {
                    schema: 1,
                    version: "test".into(),
                    os: "linux".into(),
                    arch: "x86_64".into(),
                    capabilities: vec!["read".into(), "list".into()],
                    capability_groups: Vec::new(),
                })
            }
            "saving" => app.documents[1].saving = true,
            "pending save" => {
                app.pending.insert(
                    99,
                    Job::Save {
                        document: 2,
                        snapshot: String::new(),
                        submission: None,
                    },
                );
            }
            "unknown" => {
                app.documents[1].interrupted_save =
                    interrupted_save::InterruptedSave::capture(&app, &app.documents[1])
            }
            "unverifiable" => app.documents[1].save_outcome_unverifiable = true,
            "checking" => app.interrupted_save_check.outstanding = Some(99),
            "request exhausted" => app.next_request = u64::MAX,
            _ => unreachable!(),
        }
        app.queue_save_all();
        finish(&mut app);
        idle(&commands);
        assert!(!app.save_all_busy(), "{invalid}");
        assert!(
            app.error.as_deref().unwrap().contains("No writes sent"),
            "{invalid}"
        );
    }
}

#[test]
fn all_later_members_are_revalidated_after_ack_before_the_next_write() {
    for changed in ["text", "version", "baseline", "revision", "path", "closed"] {
        let (mut app, commands) = connected(3);
        app.queue_save_all();
        finish(&mut app);
        let first = write(&app, &commands, 1);
        acknowledge(&mut app, &first);
        // C changes after A's reply; B must not be sent either. Direct text
        // replacement deliberately leaves edit_version unchanged.
        match changed {
            "text" => app.documents[2].text.push('x'),
            "version" => app.documents[2].edit_version += 1,
            "baseline" => app.documents[2].saved_text.push('x'),
            "revision" => app.documents[2].revision = Some(revision("different baseline")),
            "path" => app.documents[2].path = "renamed.txt".into(),
            "closed" => {
                app.documents.pop();
            }
            _ => unreachable!(),
        }
        finish(&mut app);
        idle(&commands);
        summary(&app, 1, 0, 0, 2);
        assert!(
            app.save_all_message()
                .unwrap()
                .contains("unattempted draft changed"),
            "{changed}"
        );
        assert!(app.documents[1].dirty());
    }
}

#[test]
fn changed_unsent_draft_before_first_finisher_stops_every_write() {
    let (mut app, commands) = connected(2);
    app.queue_save_all();
    editor_state::commit(
        &app.editor_ctx,
        &mut app.documents[1],
        "same-frame edit".into(),
        3,
    );
    finish(&mut app);
    idle(&commands);
    summary(&app, 0, 0, 0, 2);
}

#[test]
fn editing_then_undoing_an_unsent_member_still_invalidates_its_captured_version() {
    let (mut app, commands) = connected(3);
    let original = app.documents[2].text.clone();
    let version = app.documents[2].edit_version;
    app.queue_save_all();
    finish(&mut app);
    let first = write(&app, &commands, 1);
    acknowledge(&mut app, &first);
    editor_state::commit(
        &app.editor_ctx,
        &mut app.documents[2],
        "temporary edit".into(),
        3,
    );
    let ctx = app.editor_ctx.clone();
    let _ = ctx.run(
        egui::RawInput {
            events: vec![egui::Event::Key {
                key: egui::Key::Z,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::COMMAND,
            }],
            ..Default::default()
        },
        |ctx| {
            ctx.memory_mut(|memory| memory.request_focus(egui::Id::new(("editor", 3u64))));
            editor_state::history_shortcut(ctx, &mut app.documents[2]);
        },
    );
    assert_eq!(app.documents[2].text, original);
    assert!(app.documents[2].edit_version > version);
    finish(&mut app);
    idle(&commands);
    summary(&app, 1, 0, 0, 2);
}

#[test]
fn in_flight_newer_typing_survives_ack_and_does_not_join_the_cohort_again() {
    let (mut app, commands) = connected(2);
    let submitted = app.documents[0].text.clone();
    app.queue_save_all();
    finish(&mut app);
    let first = write(&app, &commands, 1);
    editor_state::commit(
        &app.editor_ctx,
        &mut app.documents[0],
        "newer typing\n".into(),
        5,
    );
    acknowledge(&mut app, &first);
    finish(&mut app);
    let second = write(&app, &commands, 2);
    acknowledge(&mut app, &second);
    finish(&mut app);
    idle(&commands);
    summary(&app, 2, 0, 0, 0);
    assert_eq!(app.documents[0].text, "newer typing\n");
    assert_eq!(app.documents[0].saved_text, submitted);
    assert!(app.documents[0].dirty());
    assert!(!app.documents[1].dirty());
}

#[test]
fn a_new_dirty_document_after_queue_does_not_expand_the_frozen_cohort() {
    let (mut app, commands) = connected(1);
    app.queue_save_all();
    let mut new = Document::new(2, "new.txt".into(), String::new(), String::new());
    new.revision = None;
    new.text = "new draft".into();
    app.documents.push(new);
    finish(&mut app);
    let first = write(&app, &commands, 1);
    acknowledge(&mut app, &first);
    finish(&mut app);
    idle(&commands);
    summary(&app, 1, 0, 0, 0);
    assert!(app.documents[1].dirty());
}

#[test]
fn a_new_file_is_saved_with_absence_precondition_and_empty_draft_is_valid() {
    let (mut app, commands) = connected(1);
    app.documents[0].revision = None;
    app.documents[0].saved_text.clear();
    app.documents[0].text.clear();
    app.queue_save_all();
    finish(&mut app);
    let first = write(&app, &commands, 1);
    assert!(
        matches!(&first.op, Operation::Write { text, expected_revision: None, .. } if text.is_empty())
    );
    acknowledge(&mut app, &first);
    finish(&mut app);
    summary(&app, 1, 0, 0, 0);
}

#[test]
fn queued_or_in_flight_repeats_and_manual_saves_cannot_create_another_write() {
    let (mut app, commands) = connected(2);
    app.queue_save_all();
    app.queue_save_all();
    app.save_document(2);
    idle(&commands);
    finish(&mut app);
    let first = write(&app, &commands, 1);
    app.queue_save_all();
    app.save_document(2);
    finish(&mut app);
    idle(&commands);
    acknowledge(&mut app, &first);
    // Ownership also covers the boundary after the response and before finish.
    app.save_document(2);
    app.queue_save_all();
    idle(&commands);
    finish(&mut app);
    let second = write(&app, &commands, 2);
    acknowledge(&mut app, &second);
    finish(&mut app);
    summary(&app, 2, 0, 0, 0);
}

#[test]
fn cancel_before_dispatch_keeps_every_member_unattempted() {
    let (mut app, commands) = connected(3);
    app.queue_save_all();
    app.cancel_save_all();
    app.queue_save_all();
    finish(&mut app);
    idle(&commands);
    summary(&app, 0, 0, 0, 3);
}

#[test]
fn cancel_keeps_in_flight_job_and_manual_guard_until_its_outcome_settles() {
    let (mut app, commands) = connected(3);
    app.queue_save_all();
    finish(&mut app);
    let first = write(&app, &commands, 1);
    app.cancel_save_all();
    app.cancel_save_all();
    finish(&mut app);
    assert!(app.save_all_busy());
    assert!(app.pending.contains_key(&first.id));
    assert!(app.documents[0].saving);
    app.save_document(2);
    app.queue_save_all();
    idle(&commands);
    acknowledge(&mut app, &first);
    assert!(app.save_all_busy());
    finish(&mut app);
    idle(&commands);
    summary(&app, 1, 0, 0, 2);
    // A later, explicit batch is allowed only after the owner has settled.
    app.queue_save_all();
    finish(&mut app);
    write(&app, &commands, 2);
}

#[test]
fn connected_error_is_failed_while_invalid_acknowledgement_is_unknown() {
    for result in [
        Err("fixture revision conflict".into()),
        Ok(Payload::Written {
            revision: revision("wrong bytes"),
        }),
        Ok(Payload::File {
            path: "file-1.txt".into(),
            text: "private remote payload".into(),
            revision: revision("private remote payload"),
        }),
    ] {
        let failed = result.is_err();
        let (mut app, commands) = connected(3);
        app.queue_save_all();
        finish(&mut app);
        let first = write(&app, &commands, 1);
        app.apply_event(Event {
            generation: app.generation,
            id: first.id,
            connected: true,
            result,
        });
        idle(&commands);
        finish(&mut app);
        idle(&commands);
        summary(&app, 0, usize::from(failed), usize::from(!failed), 2);
        assert_eq!(app.documents[0].save_outcome_unknown(), !failed);
        assert!(app.documents.iter().all(|doc| doc.dirty()));
        assert!(!app.save_all_message().unwrap().contains("private"));
    }
}

#[test]
fn a_later_conflict_preserves_prior_acknowledgements_without_rollback() {
    let (mut app, commands) = connected(3);
    app.queue_save_all();
    finish(&mut app);
    let first = write(&app, &commands, 1);
    acknowledge(&mut app, &first);
    finish(&mut app);
    let second = write(&app, &commands, 2);
    app.apply_event(Event {
        generation: app.generation,
        id: second.id,
        connected: true,
        result: Err("revision conflict".into()),
    });
    finish(&mut app);
    idle(&commands);
    summary(&app, 1, 1, 0, 1);
    assert!(!app.documents[0].dirty());
    assert!(app.documents[1].dirty() && app.documents[2].dirty());
}

#[test]
fn ordered_success_then_eof_counts_acknowledgement_and_never_dispatches_next() {
    let (mut app, commands) = connected(3);
    app.queue_save_all();
    finish(&mut app);
    let first = write(&app, &commands, 1);
    let Operation::Write { text, .. } = &first.op else {
        unreachable!()
    };
    app.result_tx
        .send(WorkerEvent::Response(Event {
            generation: app.generation,
            id: first.id,
            connected: true,
            result: Ok(Payload::Written {
                revision: revision(text),
            }),
        }))
        .unwrap();
    app.result_tx
        .send(WorkerEvent::TransportLost {
            generation: app.generation,
            message: "fixture EOF".into(),
        })
        .unwrap();
    app.poll();
    idle(&commands);
    finish(&mut app);
    idle(&commands);
    summary(&app, 1, 0, 0, 2);
    assert!(!app.documents[0].dirty());
    assert!(!app.documents[0].save_outcome_unknown());
    assert!(app.documents[1].dirty());
}

#[test]
fn transport_loss_with_unanswered_write_releases_cancelled_owner_as_unknown() {
    let (mut app, commands) = connected(3);
    app.queue_save_all();
    finish(&mut app);
    let first = write(&app, &commands, 1);
    app.cancel_save_all();
    app.disconnected("fixture EOF".into());
    assert!(app.documents[0].save_outcome_unknown());
    assert!(!app.pending.contains_key(&first.id));
    finish(&mut app);
    idle(&commands);
    summary(&app, 0, 0, 1, 2);
}

#[test]
fn stale_generation_request_and_duplicate_outcome_cannot_advance_the_owner() {
    let (mut app, commands) = connected(2);
    app.queue_save_all();
    finish(&mut app);
    let first = write(&app, &commands, 1);
    app.save_all_observe_reply(app.generation - 1, first.id, true, false);
    app.save_all_observe_reply(app.generation, first.id + 100, true, false);
    finish(&mut app);
    idle(&commands);
    assert!(app.pending.contains_key(&first.id));
    acknowledge(&mut app, &first);
    // First verified result wins; a duplicate cannot turn it into a failure.
    app.save_all_observe_reply(app.generation, first.id, false, true);
    finish(&mut app);
    let second = write(&app, &commands, 2);
    acknowledge(&mut app, &first);
    finish(&mut app);
    idle(&commands);
    acknowledge(&mut app, &second);
    finish(&mut app);
    summary(&app, 2, 0, 0, 0);
}

#[test]
fn exact_written_after_pending_save_is_missing_or_replaced_is_counted_unknown() {
    // Internal ownership-fault robustness: peers cannot remove/replace pending
    // jobs or reuse request IDs. Never turn that missing evidence into a retry.
    for (replace, cancel, retain_token) in [
        (false, false, false),
        (true, false, false),
        (false, true, false),
        (false, false, true),
    ] {
        let (mut app, commands) = connected(2);
        let baselines: Vec<_> = app
            .documents
            .iter()
            .map(|doc| (doc.saved_text.clone(), doc.revision.clone()))
            .collect();
        app.queue_save_all();
        finish(&mut app);
        let first = write(&app, &commands, 1);
        if cancel {
            app.cancel_save_all();
        }
        if retain_token {
            app.retain_interrupted_save(first.id);
        }
        let token = app.documents[0].interrupted_save.clone();
        assert!(app.pending.remove(&first.id).is_some());
        if replace {
            app.pending.insert(
                first.id,
                Job::Search {
                    query: "unrelated replacement".into(),
                },
            );
        }
        acknowledge(&mut app, &first);
        idle(&commands);
        finish(&mut app);
        idle(&commands);
        summary(&app, 0, 0, 1, 1);
        for (doc, (text, revision)) in app.documents.iter().zip(&baselines) {
            assert_eq!(&doc.saved_text, text);
            assert_eq!(&doc.revision, revision);
            assert!(doc.dirty());
        }
        assert!(app.documents[0].save_outcome_unverifiable);
        assert_eq!(app.documents[0].interrupted_save, token);
        assert!(!app.documents[0].saving);
        assert!(!app.documents[1].save_outcome_unknown());
        let ctx = app.editor_ctx.clone();
        let _ = ctx.run(
            egui::RawInput {
                events: vec![egui::Event::Key {
                    key: egui::Key::Z,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::COMMAND,
                }],
                ..Default::default()
            },
            |ctx| {
                ctx.memory_mut(|memory| memory.request_focus(egui::Id::new(("editor", 1u64))));
                editor_state::history_shortcut(ctx, &mut app.documents[0]);
            },
        );
        assert_eq!(app.documents[0].text, app.documents[0].saved_text);
        assert!(
            app.documents[0].dirty(),
            "Undo to the old baseline cannot settle an unknown write"
        );
        assert!(app.documents[0].save_outcome_unverifiable);
        app.disconnected("fixture reconnect".into());
        let (worker, reconnected_commands) = Worker::recording();
        app.worker = Some(worker);
        app.generation += 1;
        app.state = ConnectionState::Connecting;
        app.connecting_form = app.active_form.clone();
        app.apply_event(Event {
            generation: app.generation,
            id: 0,
            connected: true,
            result: Ok(Payload::Hello {
                protocol: cedar_protocol::PROTOCOL_VERSION,
                root: app.root.clone(),
                agent: None,
            }),
        });
        assert!(app.ready());
        app.save_document(1);
        assert!(app
            .error
            .as_deref()
            .unwrap()
            .contains("identity is unavailable"));
        app.check_interrupted_save();
        assert!(!app.interrupted_save_check.busy());
        app.queue_save_all();
        assert!(!app.save_all_busy());
        idle(&reconnected_commands);
        assert!(app.documents[0].save_outcome_unverifiable);
    }
}

#[test]
fn missing_submission_repair_does_not_touch_changed_path_workspace_or_session() {
    // Internal ownership-fault robustness, not a remotely constructible reply.
    for change in ["path", "workspace", "session"] {
        let (mut app, commands) = connected(2);
        app.queue_save_all();
        finish(&mut app);
        let first = write(&app, &commands, 1);
        app.pending.remove(&first.id);
        acknowledge(&mut app, &first);
        // The outcome is recorded first, then ownership changes before the
        // final-frame repair. Neither the replacement nor another tab is ours.
        match change {
            "path" => app.documents[0].path = "replacement.txt".into(),
            "workspace" => app.root = "/different-workspace".into(),
            "session" => app.generation += 1,
            _ => unreachable!(),
        }
        let text = app.documents[0].text.clone();
        finish(&mut app);
        idle(&commands);
        summary(&app, 0, 0, 1, 1);
        assert!(
            !app.documents.iter().any(|doc| doc.save_outcome_unknown()),
            "{change}"
        );
        assert!(
            app.documents[0].saving,
            "repair must not mutate a different owner"
        );
        assert_eq!(app.documents[0].text, text);
    }
}

#[test]
fn workspace_generation_or_recovery_identity_change_before_dispatch_stops_cohort() {
    for change_generation in [true, false] {
        let (mut app, commands) = connected(2);
        app.queue_save_all();
        if change_generation {
            app.generation += 1;
        } else {
            app.root = "/different-workspace".into();
        }
        finish(&mut app);
        idle(&commands);
        summary(&app, 0, 0, 0, 2);
    }
}

#[test]
fn end_frame_modal_stops_unsent_work_without_dropping_in_flight_ownership() {
    let (mut app, commands) = connected(2);
    app.queue_save_all();
    finish(&mut app);
    let first = write(&app, &commands, 1);
    app.confirm = Some(Confirm::CloseWindow);
    finish(&mut app);
    assert!(app.save_all_busy());
    assert!(app.pending.contains_key(&first.id));
    idle(&commands);
    app.confirm = None;
    acknowledge(&mut app, &first);
    finish(&mut app);
    idle(&commands);
    summary(&app, 1, 0, 0, 1);
}

#[test]
fn disconnected_written_response_is_unknown_even_when_digest_matches() {
    let (mut app, commands) = connected(2);
    app.queue_save_all();
    finish(&mut app);
    let first = write(&app, &commands, 1);
    let Operation::Write { text, .. } = &first.op else {
        unreachable!()
    };
    app.apply_event(Event {
        generation: app.generation,
        id: first.id,
        connected: false,
        result: Ok(Payload::Written {
            revision: revision(text),
        }),
    });
    finish(&mut app);
    idle(&commands);
    summary(&app, 0, 0, 1, 1);
    assert!(app.documents[0].save_outcome_unknown());
}

#[test]
fn known_enqueue_failure_leaves_all_members_unattempted() {
    let (mut app, commands) = connected(2);
    app.queue_save_all();
    drop(commands);
    finish(&mut app);
    summary(&app, 0, 0, 0, 2);
    assert!(app
        .documents
        .iter()
        .all(|doc| doc.dirty() && !doc.save_outcome_unknown() && !doc.saving));
}
