//! Written acknowledges the captured bytes only when their exact SHA-256 and
//! submission ownership are verified. All requests are captured from the real
//! frontend save path; frames exercise the production worker-event ordering.
use super::*;
use cedar_recovery::{record_id, Store};
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};

const ROOT: &str = "/synthetic-save-ack-workspace";
const FILE: &str = "draft.txt";
const ORIGINAL: &str = "original generated text\n";
const SUBMITTED: &str = "submitted generated text é 🐻\r\n";
const NEWER: &str = "newer generated text Ω\n";

fn revision(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

fn connected() -> (CedarApp, Receiver<Command>) {
    let mut app = CedarApp::empty();
    let form = ConnectForm {
        local_root: ROOT.into(),
        allow_run: false,
        ..Default::default()
    };
    app.workspace_key = Some(form.key());
    app.active_form = Some(form);
    app.root = ROOT.into();
    app.state = ConnectionState::Ready;
    app.generation = 7;
    app.open_form = false;
    // Tree mode makes successful saves mark the directory stale without an
    // unrelated automatic List; unexpected requests remain visible below.
    app.explorer.mode = explorer_tree::Mode::Tree;
    app.documents.push(Document::new(
        1,
        FILE.into(),
        ORIGINAL.into(),
        revision(ORIGINAL),
    ));
    app.active_document = Some(1);
    app.next_document = 2;
    let (worker, commands) = Worker::recording();
    app.worker = Some(worker);
    (app, commands)
}

fn edit(app: &mut CedarApp, text: &str) {
    editor_state::commit(&app.editor_ctx, &mut app.documents[0], text.into(), 4);
}

fn save(
    app: &mut CedarApp,
    commands: &Receiver<Command>,
) -> (u64, interrupted_save::InterruptedSave) {
    app.save_document(1);
    let command = commands.try_recv().expect("save must send one Write");
    assert!(matches!(
        &command.op,
        Operation::Write { path, text, expected_revision }
            if path == FILE && text == SUBMITTED
                && expected_revision.as_deref() == Some(revision(ORIGINAL).as_str())
    ));
    let Some(Job::Save {
        document,
        snapshot,
        submission: Some(submission),
    }) = app.pending.get(&command.id)
    else {
        panic!("real saves must capture a bounded submission identity");
    };
    assert_eq!(*document, 1);
    assert_eq!(snapshot, SUBMITTED);
    assert!(app.documents[0].saving);
    let submission = submission.clone();
    idle(commands);
    (command.id, submission)
}

fn idle(commands: &Receiver<Command>) {
    assert!(matches!(
        commands.try_recv(),
        Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected)
    ));
}

fn written(app: &CedarApp, request: u64, value: String) -> Event {
    Event {
        generation: app.generation,
        id: request,
        connected: true,
        result: Ok(Payload::Written { revision: value }),
    }
}

fn acknowledge(app: &mut CedarApp, request: u64, value: String) {
    let event = written(app, request, value);
    app.apply_event(event);
}

fn bad_revisions() -> Vec<(&'static str, String)> {
    vec![
        ("empty", String::new()),
        ("oversized", "raw-ack-must-stay-private".repeat(8192)),
        ("uppercase", revision(SUBMITTED).to_uppercase()),
        ("nonhex", "g".repeat(64)),
        ("wrong content", revision(NEWER)),
        ("wrong byte count", "a".repeat(65)),
    ]
}

fn unresolved(app: &CedarApp, current: &str, submission: &interrupted_save::InterruptedSave) {
    let doc = &app.documents[0];
    assert_eq!(doc.text, current);
    assert_eq!(doc.saved_text, ORIGINAL);
    assert_eq!(doc.revision, Some(revision(ORIGINAL)));
    assert_eq!(doc.interrupted_save.as_ref(), Some(submission));
    assert!(!doc.save_outcome_unverifiable);
    assert!(doc.save_outcome_unknown());
    assert!(doc.dirty());
    assert!(!doc.saving);
}

fn frame(app: &mut CedarApp, time: f64, events: Vec<egui::Event>) {
    let ctx = app.editor_ctx.clone();
    let mut native = eframe::Frame::_new_kittest();
    let _ = ctx.run(
        egui::RawInput {
            time: Some(time),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 900.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| eframe::App::update(app, ctx, &mut native),
    );
}

fn focus(app: &CedarApp) {
    app.editor_ctx
        .memory_mut(|memory| memory.request_focus(egui::Id::new(("editor", 1u64))));
}

fn history_key(redo: bool) -> Vec<egui::Event> {
    let modifiers = if redo {
        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT
    } else {
        egui::Modifiers::COMMAND
    };
    [true, false]
        .into_iter()
        .map(|pressed| egui::Event::Key {
            key: egui::Key::Z,
            physical_key: Some(egui::Key::Z),
            pressed,
            repeat: false,
            modifiers,
        })
        .collect()
}

#[test]
fn acknowledgement_digest_uses_exact_utf8_bytes_including_empty_and_line_endings() {
    assert!(interrupted_save::acknowledgement_matches(
        "",
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    ));
    let canonical = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    assert!(interrupted_save::acknowledgement_matches("abc", canonical));
    assert!(!interrupted_save::acknowledgement_matches(
        "abc",
        &canonical.to_uppercase(),
    ));
    for text in ["", "plain text", "é 🐻", "line\n", "line\r\n", "e\u{301}"] {
        assert!(interrupted_save::acknowledgement_matches(
            text,
            &revision(text)
        ));
        assert!(!interrupted_save::acknowledgement_matches(
            text,
            &revision(&format!("{text} ")),
        ));
    }
    assert!(!interrupted_save::acknowledgement_matches(
        "line\r\n",
        &revision("line\n")
    ));
    assert!(!interrupted_save::acknowledgement_matches(
        "é",
        &revision("e\u{301}")
    ));
    let boundary = "é🐻\r\n".repeat(cedar_protocol::MAX_FILE_BYTES / 8);
    assert_eq!(boundary.len(), cedar_protocol::MAX_FILE_BYTES);
    assert!(interrupted_save::acknowledgement_matches(
        &boundary,
        &revision(&boundary),
    ));
    for (case, value) in bad_revisions() {
        assert!(
            !interrupted_save::acknowledgement_matches(SUBMITTED, &value),
            "{case}"
        );
    }
}

#[test]
fn malformed_and_wrong_content_written_keep_the_original_submission_unknown() {
    for (case, value) in bad_revisions() {
        for newer in [false, true] {
            let (mut app, commands) = connected();
            edit(&mut app, SUBMITTED);
            let (request, submission) = save(&mut app, &commands);
            if newer {
                edit(&mut app, NEWER);
            }
            let edit_version = app.documents[0].edit_version;
            let explorer_epoch = app.explorer.epoch;
            acknowledge(&mut app, request, value.clone());
            unresolved(&app, if newer { NEWER } else { SUBMITTED }, &submission);
            assert_eq!(app.documents[0].edit_version, edit_version, "{case}");
            assert!(app.pending.is_empty(), "{case}");
            assert!(
                app.ready(),
                "a bad acknowledgement alone is not EOF: {case}"
            );
            assert_eq!(app.explorer.epoch, explorer_epoch, "{case}");
            assert!(app.explorer.message.is_none(), "{case}");
            let error = app.error.as_deref().expect("bounded public explanation");
            assert!(error.len() <= 256, "{case}");
            assert!(error.contains("acknowledgement"), "{case}: {error}");
            if !value.is_empty() {
                assert!(
                    !error.contains(&value),
                    "raw acknowledgement leaked: {case}"
                );
                assert!(
                    !app.notice.contains(&value),
                    "raw acknowledgement leaked: {case}"
                );
            }
            assert!(!error.contains("raw-ack-must-stay-private"));
            assert!(!app.notice.contains("raw-ack-must-stay-private"));
            assert!(!app.notice.starts_with("Saved "), "{case}");
            app.save_document(1);
            idle(&commands);
        }
    }
}

#[test]
fn exact_submitted_hash_acknowledges_only_that_snapshot_after_newer_typing() {
    assert_ne!(revision(SUBMITTED), revision(NEWER));
    for current in [SUBMITTED, NEWER, ORIGINAL] {
        let (mut app, commands) = connected();
        edit(&mut app, SUBMITTED);
        let (request, _) = save(&mut app, &commands);
        if current != SUBMITTED {
            edit(&mut app, current);
        }
        let version = app.documents[0].edit_version;
        let explorer_epoch = app.explorer.epoch;
        acknowledge(&mut app, request, revision(SUBMITTED));
        let doc = &app.documents[0];
        assert_eq!(doc.text, current);
        assert_eq!(doc.saved_text, SUBMITTED);
        assert_eq!(doc.revision, Some(revision(SUBMITTED)));
        assert_eq!(doc.edit_version, version);
        assert!(!doc.saving);
        assert!(!doc.save_outcome_unknown());
        assert_eq!(doc.dirty(), current != SUBMITTED);
        assert!(app.pending.is_empty());
        assert!(app.error.is_none());
        assert!(app.explorer.epoch > explorer_epoch);
        idle(&commands);
    }
}

#[test]
fn valid_written_for_an_inactive_tab_keeps_active_selection_and_both_newer_drafts() {
    let (mut app, commands) = connected();
    frame(&mut app, 0.0, vec![]);
    edit(&mut app, SUBMITTED);
    let (request, _) = save(&mut app, &commands);
    edit(&mut app, NEWER);
    app.documents.push(Document::new(
        2,
        "other.txt".into(),
        "other original baseline".into(),
        revision("other original baseline"),
    ));
    app.next_document = 3;
    app.navigation_changed();
    app.active_document = Some(2);
    editor_state::commit(
        &app.editor_ctx,
        &mut app.documents[1],
        "other newer unsaved draft".into(),
        6,
    );
    frame(&mut app, 1.0, vec![]);
    let id = egui::Id::new(("editor", 2u64));
    let selection =
        egui::text::CCursorRange::two(egui::text::CCursor::new(2), egui::text::CCursor::new(8));
    let mut state = egui::TextEdit::load_state(&app.editor_ctx, id).unwrap();
    state.cursor.set_char_range(Some(selection));
    state.store(&app.editor_ctx, id);
    app.editor_ctx.memory_mut(|memory| memory.request_focus(id));
    let versions = app.draft_versions();
    app.result_tx
        .send(WorkerEvent::Response(written(
            &app,
            request,
            revision(SUBMITTED),
        )))
        .unwrap();
    frame(&mut app, 2.0, vec![]);
    assert_eq!(app.active_document, Some(2));
    assert_eq!(app.draft_versions(), versions);
    let original = &app.documents[0];
    assert_eq!(original.text, NEWER);
    assert_eq!(original.saved_text, SUBMITTED);
    assert_eq!(original.revision, Some(revision(SUBMITTED)));
    assert!(!original.saving);
    assert!(!original.save_outcome_unknown());
    assert!(original.dirty());
    let active = &app.documents[1];
    assert_eq!(active.text, "other newer unsaved draft");
    assert_eq!(active.saved_text, "other original baseline");
    assert_eq!(active.revision, Some(revision("other original baseline")));
    assert!(active.dirty());
    assert!(!active.saving);
    assert!(!active.save_outcome_unknown());
    assert_eq!(
        egui::TextEdit::load_state(&app.editor_ctx, id)
            .unwrap()
            .cursor
            .char_range(),
        Some(selection)
    );
    assert!(app.editor_ctx.memory(|memory| memory.has_focus(id)));
    assert!(app.pending.is_empty());
    assert!(app.error.is_none());
    idle(&commands);
}

#[test]
fn wrong_payload_cannot_acknowledge_even_matching_submitted_file_contents() {
    let (mut app, commands) = connected();
    edit(&mut app, SUBMITTED);
    let (request, submission) = save(&mut app, &commands);
    app.apply_event(Event {
        generation: app.generation,
        id: request,
        connected: true,
        result: Ok(Payload::File {
            path: FILE.into(),
            text: SUBMITTED.into(),
            revision: revision(SUBMITTED),
        }),
    });
    unresolved(&app, SUBMITTED, &submission);
    idle(&commands);
}

#[test]
fn stale_generation_unrelated_request_and_duplicate_cannot_publish_save_success() {
    for valid in [false, true] {
        let (mut app, commands) = connected();
        edit(&mut app, SUBMITTED);
        let (request, submission) = save(&mut app, &commands);
        edit(&mut app, NEWER);
        let mut stale = written(&app, request, revision(SUBMITTED));
        stale.generation -= 1;
        app.apply_event(stale);
        acknowledge(&mut app, request + 100, revision(SUBMITTED));
        assert!(app.documents[0].saving);
        assert!(app.pending.contains_key(&request));
        assert_eq!(app.documents[0].saved_text, ORIGINAL);
        assert!(!app.documents[0].save_outcome_unknown());
        assert!(app.error.is_none());
        acknowledge(
            &mut app,
            request,
            if valid {
                revision(SUBMITTED)
            } else {
                revision(NEWER)
            },
        );
        if !valid {
            unresolved(&app, NEWER, &submission);
        }
        let before = format!("{:?}", app.documents[0]);
        let error = app.error.clone();
        let notice = app.notice.clone();
        let explorer_epoch = app.explorer.epoch;
        // A late valid duplicate cannot repair an invalid first reply; a bad
        // duplicate cannot undo a valid first acknowledgement either.
        acknowledge(&mut app, request, revision(SUBMITTED));
        acknowledge(&mut app, request, "duplicate-raw-ack".into());
        assert_eq!(format!("{:?}", app.documents[0]), before);
        assert_eq!(app.error, error);
        assert_eq!(app.notice, notice);
        assert_eq!(app.explorer.epoch, explorer_epoch);
        idle(&commands);
    }
}

#[test]
fn exact_hash_still_requires_original_submission_ownership() {
    for race in 0..10 {
        let (mut app, commands) = connected();
        edit(&mut app, SUBMITTED);
        let (mut request, _) = save(&mut app, &commands);
        match race {
            0 => app.generation += 1,
            1 => {
                let job = app.pending.remove(&request).unwrap();
                request += 100;
                app.pending.insert(request, job);
            }
            2 => {
                app.documents[0].id = 2;
                let Some(Job::Save { document, .. }) = app.pending.get_mut(&request) else {
                    unreachable!();
                };
                *document = 2;
            }
            3 => app.documents[0].path = "renamed.txt".into(),
            4 => app.root = "/different-workspace".into(),
            5 => app.documents[0].saved_text = "different base bytes".into(),
            6 => app.documents[0].revision = Some(revision("different base bytes")),
            7 => {
                let Some(Job::Save { snapshot, .. }) = app.pending.get_mut(&request) else {
                    unreachable!();
                };
                *snapshot = NEWER.into();
            }
            8 => {
                // A different token already owned by this tab cannot be
                // replaced by a later successful-looking response.
                app.documents[0].interrupted_save =
                    interrupted_save::InterruptedSave::capture(&app, &app.documents[0]);
            }
            9 => app.documents[0].save_outcome_unverifiable = true,
            _ => unreachable!(),
        }
        let base = app.documents[0].saved_text.clone();
        let base_revision = app.documents[0].revision.clone();
        let value = if race == 7 {
            revision(NEWER)
        } else {
            revision(SUBMITTED)
        };
        acknowledge(&mut app, request, value);
        let doc = &app.documents[0];
        assert_eq!(doc.text, SUBMITTED, "race {race}");
        assert_eq!(doc.saved_text, base, "race {race}");
        assert_eq!(doc.revision, base_revision, "race {race}");
        assert!(doc.save_outcome_unverifiable, "race {race}");
        assert!(doc.save_outcome_unknown(), "race {race}");
        assert!(doc.dirty(), "race {race}");
        assert!(!doc.saving, "race {race}");
        assert!(!app.notice.starts_with("Saved "), "race {race}");
        assert!(app.explorer.message.is_none(), "race {race}");
        idle(&commands);
    }
}

#[test]
fn disconnected_written_with_an_exact_hash_is_still_an_unknown_outcome() {
    let (mut app, commands) = connected();
    edit(&mut app, SUBMITTED);
    let (request, submission) = save(&mut app, &commands);
    let mut event = written(&app, request, revision(SUBMITTED));
    event.connected = false;
    app.apply_event(event);
    unresolved(&app, SUBMITTED, &submission);
    assert!(app.state == ConnectionState::Disconnected);
    assert!(app.pending.is_empty());
    assert!(app.worker.is_none());
    idle(&commands);
}

#[test]
fn missing_document_acknowledgements_do_not_offer_a_check_for_a_removed_tab() {
    for connected in [false, true] {
        let (mut app, commands) = self::connected();
        edit(&mut app, SUBMITTED);
        let (request, _) = save(&mut app, &commands);
        app.documents.clear();
        app.active_document = None;
        let mut event = written(&app, request, "removed-tab-raw-ack".into());
        event.connected = connected;
        app.apply_event(event);
        assert!(app.documents.is_empty());
        assert!(app.pending.is_empty());
        assert!(!app.notice.starts_with("Saved "));
        assert!(!app
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("Check interrupted save"));
        assert!(!app
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("removed-tab-raw-ack"));
        if connected {
            assert!(app.ready());
            assert!(app.error.is_none());
        } else {
            assert!(app.state == ConnectionState::Disconnected);
        }
        idle(&commands);
    }
}

#[test]
fn queued_written_and_eof_keep_their_order_in_a_real_native_frame() {
    for valid in [false, true] {
        for eof_first in [false, true] {
            let (mut app, commands) = connected();
            frame(&mut app, 0.0, vec![]);
            edit(&mut app, SUBMITTED);
            let (request, submission) = save(&mut app, &commands);
            edit(&mut app, NEWER);
            let reply = WorkerEvent::Response(written(
                &app,
                request,
                if valid {
                    revision(SUBMITTED)
                } else {
                    revision(NEWER)
                },
            ));
            let eof = WorkerEvent::TransportLost {
                generation: app.generation,
                message: "transport_closed: synthetic EOF".into(),
            };
            let events = if eof_first {
                [eof, reply]
            } else {
                [reply, eof]
            };
            for event in events {
                app.result_tx.send(event).unwrap();
            }
            frame(&mut app, 1.0, vec![]);
            assert!(app.state == ConnectionState::Disconnected);
            assert!(app.worker.is_none());
            assert!(app.pending.is_empty());
            assert_eq!(app.documents[0].text, NEWER);
            if valid && !eof_first {
                assert_eq!(app.documents[0].saved_text, SUBMITTED);
                assert_eq!(app.documents[0].revision, Some(revision(SUBMITTED)));
                assert!(!app.documents[0].save_outcome_unknown());
                assert!(app.documents[0].dirty());
            } else {
                unresolved(&app, NEWER, &submission);
            }
            idle(&commands);
        }
    }
}

#[test]
fn same_frame_typing_paste_and_undo_survive_written_validation() {
    for valid in [false, true] {
        for input in 0..3 {
            let (mut app, commands) = connected();
            frame(&mut app, 0.0, vec![]);
            edit(&mut app, SUBMITTED);
            frame(&mut app, 1.0, vec![]);
            let (request, submission) = save(&mut app, &commands);
            app.result_tx
                .send(WorkerEvent::Response(written(
                    &app,
                    request,
                    if valid {
                        revision(SUBMITTED)
                    } else {
                        revision(NEWER)
                    },
                )))
                .unwrap();
            focus(&app);
            let events = match input {
                0 => vec![egui::Event::Text("typed".into())],
                1 => vec![egui::Event::Paste("pasted".into())],
                2 => history_key(false),
                _ => unreachable!(),
            };
            frame(&mut app, 2.0, events);
            let doc = &app.documents[0];
            assert_ne!(doc.text, SUBMITTED, "input {input}");
            if input == 2 {
                assert_eq!(doc.text, ORIGINAL);
            }
            assert!(doc.dirty());
            assert!(!doc.saving);
            if valid {
                assert_eq!(doc.saved_text, SUBMITTED);
                assert_eq!(doc.revision, Some(revision(SUBMITTED)));
                assert!(!doc.save_outcome_unknown());
            } else {
                unresolved(&app, &doc.text, &submission);
            }
            idle(&commands);
        }
    }
}

#[test]
fn selection_and_native_undo_redo_survive_valid_and_invalid_written() {
    for valid in [false, true] {
        let (mut app, commands) = connected();
        frame(&mut app, 0.0, vec![]);
        edit(&mut app, SUBMITTED);
        let (request, submission) = save(&mut app, &commands);
        edit(&mut app, NEWER);
        frame(&mut app, 1.0, vec![]);
        let id = egui::Id::new(("editor", 1u64));
        let selection =
            egui::text::CCursorRange::two(egui::text::CCursor::new(2), egui::text::CCursor::new(9));
        let mut state = egui::TextEdit::load_state(&app.editor_ctx, id).unwrap();
        state.cursor.set_char_range(Some(selection));
        state.store(&app.editor_ctx, id);
        focus(&app);
        let version = app.documents[0].edit_version;
        app.result_tx
            .send(WorkerEvent::Response(written(
                &app,
                request,
                if valid {
                    revision(SUBMITTED)
                } else {
                    revision(NEWER)
                },
            )))
            .unwrap();
        frame(&mut app, 2.0, vec![]);
        assert_eq!(app.documents[0].edit_version, version);
        assert_eq!(app.documents[0].text, NEWER);
        assert_eq!(
            egui::TextEdit::load_state(&app.editor_ctx, id)
                .unwrap()
                .cursor
                .char_range(),
            Some(selection)
        );
        frame(&mut app, 3.0, history_key(false));
        assert_eq!(app.documents[0].text, SUBMITTED);
        frame(&mut app, 4.0, history_key(true));
        assert_eq!(app.documents[0].text, NEWER);
        if valid {
            assert_eq!(app.documents[0].saved_text, SUBMITTED);
            assert_eq!(app.documents[0].revision, Some(revision(SUBMITTED)));
        } else {
            unresolved(&app, NEWER, &submission);
        }
        idle(&commands);
    }
}

fn read_reply(app: &mut CedarApp, command: Command, text: &str) {
    assert!(matches!(&command.op, Operation::Read { path } if path == FILE));
    app.apply_event(Event {
        generation: app.generation,
        id: command.id,
        connected: true,
        result: Ok(Payload::File {
            path: FILE.into(),
            text: text.into(),
            revision: revision(text),
        }),
    });
}

#[test]
fn invalid_written_requires_an_explicit_two_read_content_check_without_any_write() {
    for disk in [ORIGINAL, SUBMITTED] {
        let (mut app, commands) = connected();
        edit(&mut app, SUBMITTED);
        let (request, submission) = save(&mut app, &commands);
        edit(&mut app, NEWER);
        acknowledge(&mut app, request, revision(NEWER));
        unresolved(&app, NEWER, &submission);
        idle(&commands);
        app.check_interrupted_save();
        app.check_interrupted_save();
        let first = commands.try_recv().expect("first explicit Read");
        idle(&commands);
        read_reply(&mut app, first, disk);
        unresolved(&app, NEWER, &submission);
        let second = commands.try_recv().expect("second verification Read");
        app.check_interrupted_save();
        idle(&commands);
        read_reply(&mut app, second, disk);
        unresolved(&app, NEWER, &submission);
        app.finish_interrupted_save_check();
        assert_eq!(app.documents[0].text, NEWER);
        assert_eq!(app.documents[0].saved_text, disk);
        assert_eq!(app.documents[0].revision, Some(revision(disk)));
        assert!(!app.documents[0].save_outcome_unknown());
        assert!(app.documents[0].dirty());
        assert!(app
            .interrupted_save_check
            .message()
            .unwrap()
            .contains("does not prove"));
        idle(&commands);
    }
}

#[test]
fn new_file_bad_written_then_two_absent_reads_retains_a_dirty_new_file_without_writing() {
    let (mut app, commands) = connected();
    app.documents[0].text.clear();
    app.documents[0].saved_text.clear();
    app.documents[0].revision = None;
    edit(&mut app, SUBMITTED);
    app.save_document(1);
    let write = commands.try_recv().expect("explicit new-file Write");
    assert!(matches!(
        &write.op,
        Operation::Write { path, text, expected_revision }
            if path == FILE && text == SUBMITTED && expected_revision.is_none()
    ));
    let Some(Job::Save {
        snapshot,
        submission: Some(submission),
        ..
    }) = app.pending.get(&write.id)
    else {
        panic!("new-file saves must capture their absent original baseline");
    };
    assert_eq!(snapshot, SUBMITTED);
    let submission = submission.clone();
    acknowledge(&mut app, write.id, revision(NEWER));
    assert_eq!(app.documents[0].text, SUBMITTED);
    assert_eq!(app.documents[0].saved_text, "");
    assert_eq!(app.documents[0].revision, None);
    assert_eq!(
        app.documents[0].interrupted_save.as_ref(),
        Some(&submission)
    );
    assert!(app.documents[0].dirty());
    assert!(!app.documents[0].saving);
    idle(&commands);
    app.check_interrupted_save();
    for _ in 0..2 {
        let read = commands
            .try_recv()
            .expect("one of exactly two absence Reads");
        assert!(matches!(&read.op, Operation::Read { path } if path == FILE));
        app.check_interrupted_save();
        app.save_document(1);
        idle(&commands);
        app.apply_event(Event {
            generation: app.generation,
            id: read.id,
            connected: true,
            result: Err("not_found: generated new-file path is absent".into()),
        });
        assert_eq!(
            app.documents[0].interrupted_save.as_ref(),
            Some(&submission)
        );
        assert_eq!(app.documents[0].saved_text, "");
        assert_eq!(app.documents[0].revision, None);
    }
    let version = app.documents[0].edit_version;
    idle(&commands);
    app.finish_interrupted_save_check();
    assert_eq!(app.documents[0].text, SUBMITTED);
    assert_eq!(app.documents[0].saved_text, "");
    assert_eq!(app.documents[0].revision, None);
    assert_eq!(app.documents[0].edit_version, version);
    assert!(app.documents[0].dirty());
    assert!(!app.documents[0].saving);
    assert!(!app.documents[0].save_outcome_unknown());
    assert!(app
        .interrupted_save_check
        .message()
        .unwrap()
        .contains("currently absent"));
    assert!(app.pending.is_empty());
    idle(&commands);
}

fn forget_submission(app: &mut CedarApp, request: u64) {
    let Some(Job::Save { submission, .. }) = app.pending.get_mut(&request) else {
        unreachable!();
    };
    *submission = None;
}

#[test]
fn missing_submission_stays_unknown_after_undo_close_cancel_and_reconnect() {
    for value in [String::new(), revision(SUBMITTED)] {
        let (mut app, commands) = connected();
        frame(&mut app, 0.0, vec![]);
        edit(&mut app, SUBMITTED);
        frame(&mut app, 1.0, vec![]);
        let (request, _) = save(&mut app, &commands);
        forget_submission(&mut app, request);
        acknowledge(&mut app, request, value);
        focus(&app);
        frame(&mut app, 2.0, history_key(false));
        let doc = &app.documents[0];
        assert_eq!(doc.text, ORIGINAL);
        assert_eq!(doc.saved_text, ORIGINAL);
        assert_eq!(doc.revision, Some(revision(ORIGINAL)));
        assert!(doc.interrupted_save.is_none());
        assert!(doc.save_outcome_unverifiable);
        assert!(doc.dirty());
        assert!(!doc.saving);
        app.save_document(1);
        app.check_interrupted_save();
        idle(&commands);
        app.close_tab(1);
        assert!(matches!(app.confirm, Some(Confirm::CloseTab(1))));
        app.confirm = None;
        assert_eq!(app.documents.len(), 1);
        app.disconnected("synthetic reconnect".into());
        app.generation += 1;
        app.state = ConnectionState::Ready;
        let (worker, reconnected) = Worker::recording();
        app.worker = Some(worker);
        assert!(app.documents[0].save_outcome_unverifiable);
        assert!(app.documents[0].dirty());
        app.save_document(1);
        app.check_interrupted_save();
        idle(&reconnected);
    }
}

#[test]
fn missing_submission_transport_loss_stays_unknown_after_native_undo() {
    for passive in [false, true] {
        let (mut app, commands) = connected();
        frame(&mut app, 0.0, vec![]);
        edit(&mut app, SUBMITTED);
        frame(&mut app, 1.0, vec![]);
        let (request, _) = save(&mut app, &commands);
        forget_submission(&mut app, request);
        let event = if passive {
            WorkerEvent::TransportLost {
                generation: app.generation,
                message: "transport_closed: synthetic EOF".into(),
            }
        } else {
            WorkerEvent::Response(Event {
                generation: app.generation,
                id: request,
                connected: false,
                result: Err("transport_closed: write reply lost".into()),
            })
        };
        app.result_tx.send(event).unwrap();
        focus(&app);
        frame(&mut app, 2.0, history_key(false));
        assert!(app.state == ConnectionState::Disconnected);
        assert!(app.pending.is_empty());
        assert_eq!(app.documents[0].text, ORIGINAL);
        assert_eq!(app.documents[0].saved_text, ORIGINAL);
        assert_eq!(app.documents[0].revision, Some(revision(ORIGINAL)));
        assert!(app.documents[0].save_outcome_unverifiable);
        assert!(app.documents[0].interrupted_save.is_none());
        assert!(app.documents[0].dirty());
        app.close_tab(1);
        assert!(matches!(app.confirm, Some(Confirm::CloseTab(1))));
        idle(&commands);
    }
}

fn start_recovery(app: &mut CedarApp, path: &std::path::Path) {
    app.recovery.start(Ok(path.into()), &app.editor_ctx);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !app.recovery.initialized {
        let _ = app.recovery.poll();
        assert!(app.recovery.error.is_none(), "{:?}", app.recovery.error);
        assert!(
            Instant::now() < deadline,
            "recovery initialization timed out"
        );
        std::thread::yield_now();
    }
}

#[test]
fn recovery_keeps_original_baseline_for_bad_or_missing_submission_acknowledgements() {
    for missing in [false, true] {
        for current in [SUBMITTED, NEWER, ORIGINAL] {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("recovery");
            let (mut app, commands) = connected();
            start_recovery(&mut app, &path);
            edit(&mut app, SUBMITTED);
            let workspace = app.recovery_workspace().unwrap();
            app.recovery_tick(&app.editor_ctx.clone());
            let (request, _) = save(&mut app, &commands);
            if missing {
                forget_submission(&mut app, request);
            }
            if current != SUBMITTED {
                edit(&mut app, current);
            }
            acknowledge(&mut app, request, revision(NEWER));
            assert!(app.documents[0].dirty());
            app.recovery_tick(&app.editor_ctx.clone());
            idle(&commands);
            // Recovery actor Drop flushes its final queued mutation and joins,
            // so the persisted snapshot check needs no timing or sleep loop.
            drop(app);
            let store = Store::open(&path).unwrap();
            let draft = store.read(&record_id(&workspace, FILE).unwrap()).unwrap();
            assert_eq!(draft.text, current);
            assert_eq!(draft.base_text, ORIGINAL);
            assert_eq!(draft.base_revision, Some(revision(ORIGINAL)));
        }
    }
}

#[test]
fn valid_ack_updates_only_the_recovery_baseline_for_a_newer_draft() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("recovery");
    let (mut app, commands) = connected();
    start_recovery(&mut app, &path);
    edit(&mut app, SUBMITTED);
    let workspace = app.recovery_workspace().unwrap();
    app.recovery_tick(&app.editor_ctx.clone());
    let (request, _) = save(&mut app, &commands);
    edit(&mut app, NEWER);
    acknowledge(&mut app, request, revision(SUBMITTED));
    app.recovery_tick(&app.editor_ctx.clone());
    idle(&commands);
    drop(app);
    let store = Store::open(&path).unwrap();
    let draft = store.read(&record_id(&workspace, FILE).unwrap()).unwrap();
    assert_eq!(draft.text, NEWER);
    assert_eq!(draft.base_text, SUBMITTED);
    assert_eq!(draft.base_revision, Some(revision(SUBMITTED)));
}
