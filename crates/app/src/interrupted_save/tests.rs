use super::*;
use crate::{
    editor_state,
    worker::{Command, Event, Worker},
    ConnectForm, ConnectionState,
};
use eframe::egui;
use std::sync::mpsc::Receiver;

fn revision(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}
fn connected() -> (CedarApp, Receiver<Command>) {
    let mut app = CedarApp::empty();
    let form = ConnectForm {
        local_root: "/project".into(),
        allow_run: false,
        ..Default::default()
    };
    app.workspace_key = Some(form.key());
    app.active_form = Some(form);
    app.root = "/project".into();
    app.state = ConnectionState::Ready;
    app.generation = 7;
    app.documents.push(Document::new(
        1,
        "file.rs".into(),
        "A".into(),
        revision("A"),
    ));
    app.active_document = Some(1);
    app.next_document = 2;
    app.open_form = false;
    let (worker, rx) = Worker::recording();
    app.worker = Some(worker);
    (app, rx)
}
fn interrupted() -> (CedarApp, Receiver<Command>) {
    let (mut app, rx) = connected();
    editor_state::commit(&app.editor_ctx, &mut app.documents[0], "B".into(), 1);
    app.save();
    let write = rx.try_recv().unwrap();
    assert!(matches!(write.op, Operation::Write { .. }));
    app.apply_event(Event {
        generation: app.generation,
        id: write.id,
        connected: false,
        result: Err("Reply lost".into()),
    });
    assert!(app.documents[0].interrupted_save.is_some());
    app.generation += 1;
    app.state = ConnectionState::Ready;
    let (worker, rx) = Worker::recording();
    app.worker = Some(worker);
    (app, rx)
}
fn file(app: &CedarApp, text: &str) -> Payload {
    Payload::File {
        path: app.documents[0].path.clone(),
        text: text.into(),
        revision: revision(text),
    }
}
fn reply(app: &mut CedarApp, command: Command, text: &str) {
    assert!(matches!(command.op, Operation::Read { .. }));
    let payload = file(app, text);
    app.apply_event(Event {
        generation: app.generation,
        id: command.id,
        connected: true,
        result: Ok(payload),
    });
}
fn stage(app: &mut CedarApp, rx: &Receiver<Command>, text: &str) {
    app.check_interrupted_save();
    reply(app, rx.try_recv().unwrap(), text);
    reply(app, rx.try_recv().unwrap(), text);
    assert!(app.interrupted_save_check.slot.as_ref().unwrap().staged);
}

#[test]
fn token_keeps_only_bounded_original_submission_lineage() {
    let (mut app, _) = connected();
    app.documents[0].text = "B".repeat(MAX_FILE_BYTES);
    let token = InterruptedSave::capture(&app, &app.documents[0]).unwrap();
    assert_eq!(token.generation, 7);
    assert_eq!(token.request, app.next_request);
    assert_eq!(token.document, 1);
    assert_eq!(token.base_digest, digest("A"));
    assert_eq!(token.submitted_digest, digest(&app.documents[0].text));
    assert!(std::mem::size_of::<InterruptedSave>() < 256);
    app.documents[0].text.push('x');
    assert!(InterruptedSave::capture(&app, &app.documents[0]).is_none());
    app.documents[0].text = "B".into();
    for path in ["../file", "/file", "a\\b", "a:b", "", "a\0b"] {
        app.documents[0].path = path.into();
        assert!(InterruptedSave::capture(&app, &app.documents[0]).is_none());
    }
    app.documents[0].path = "x".repeat(MAX_PATH_BYTES);
    assert!(InterruptedSave::capture(&app, &app.documents[0]).is_some());
    app.documents[0].path.push('x');
    assert!(InterruptedSave::capture(&app, &app.documents[0]).is_none());
}

#[test]
fn failed_save_and_other_queued_saves_keep_original_tokens_before_pending_is_cleared() {
    let (mut app, rx) = connected();
    app.documents.push(Document::new(
        2,
        "second.rs".into(),
        "X".into(),
        revision("X"),
    ));
    app.documents[0].text = "B".into();
    app.documents[1].text = "Y".into();
    app.save_document(1);
    app.save_document(2);
    let first = rx.try_recv().unwrap();
    let second = rx.try_recv().unwrap();
    app.documents[0].text = "C".into();
    app.documents[1].text = "Z".into();
    app.apply_event(Event {
        generation: app.generation,
        id: first.id,
        connected: false,
        result: Err("Reply lost".into()),
    });
    assert!(app.pending.is_empty());
    assert_eq!(
        app.documents[0]
            .interrupted_save
            .as_ref()
            .unwrap()
            .submitted_digest,
        digest("B")
    );
    assert_eq!(
        app.documents[1]
            .interrupted_save
            .as_ref()
            .unwrap()
            .submitted_digest,
        digest("Y")
    );
    assert_eq!(
        app.documents[1].interrupted_save.as_ref().unwrap().request,
        second.id
    );
    assert!(!app.documents.iter().any(|doc| doc.saving));
}

#[test]
fn unexpected_save_payload_preserves_uncertainty() {
    let (mut app, rx) = connected();
    app.documents[0].text = "B".into();
    app.save();
    let command = rx.try_recv().unwrap();
    let payload = file(&app, "B");
    app.apply_event(Event {
        generation: app.generation,
        id: command.id,
        connected: true,
        result: Ok(payload),
    });
    assert!(app.documents[0].interrupted_save.is_some());
    assert_eq!(app.documents[0].saved_text, "A");
    assert!(!app.documents[0].saving);
}

#[test]
fn failed_enqueue_keeps_a_known_undelivered_save_as_an_ordinary_dirty_draft() {
    let (mut app, rx) = connected();
    app.documents[0].text = "B".into();
    drop(rx);
    app.save();
    assert!(app.documents[0].interrupted_save.is_none());
    assert!(app.documents[0].dirty());
    assert_eq!(app.documents[0].text, "B");
    assert_eq!(app.documents[0].saved_text, "A");
    assert_eq!(app.documents[0].revision, Some(revision("A")));
    assert!(!app.documents[0].saving);
    assert!(app.pending.is_empty());
    assert!(app.state == ConnectionState::Disconnected);
}

#[test]
fn failed_enqueue_still_preserves_previously_accepted_pending_saves() {
    let (mut app, rx) = connected();
    app.documents[0].text = "B".into();
    app.documents.push(Document::new(
        2,
        "second.rs".into(),
        "X".into(),
        revision("X"),
    ));
    app.documents[1].text = "Y".into();
    app.save_document(1);
    let accepted = rx.try_recv().unwrap();
    drop(rx);
    app.save_document(2);
    let token = app.documents[0].interrupted_save.as_ref().unwrap();
    assert_eq!(token.request, accepted.id);
    assert_eq!(token.submitted_digest, digest("B"));
    assert!(app.documents[1].interrupted_save.is_none());
    assert!(app.documents.iter().all(|doc| doc.dirty() && !doc.saving));
    assert_eq!(app.documents[1].text, "Y");
    assert_eq!(app.documents[1].saved_text, "X");
    assert_eq!(app.documents[1].revision, Some(revision("X")));
    assert!(app.pending.is_empty());
}

#[test]
fn undo_to_original_baseline_stays_protected_until_content_check() {
    let (mut app, rx) = interrupted();
    editor_state::commit(&app.editor_ctx, &mut app.documents[0], "A".into(), 1);
    assert!(app.documents[0].dirty());
    app.save();
    assert!(rx.try_recv().is_err());
    app.close_tab(1);
    assert!(matches!(app.confirm, Some(crate::Confirm::CloseTab(1))));
    app.confirm = None;
    stage(&mut app, &rx, "B");
    let version = app.documents[0].edit_version;
    app.finish_interrupted_save_check();
    assert_eq!(app.documents[0].text, "A");
    assert_eq!(app.documents[0].saved_text, "B");
    assert_eq!(app.documents[0].edit_version, version);
    assert!(app.documents[0].dirty());
    assert!(app.documents[0].interrupted_save.is_none());
    assert!(rx.try_recv().is_err());
}

#[test]
fn submitted_baseline_adoption_keeps_native_cursor_history_and_redo() {
    let (mut app, rx) = interrupted();
    editor_state::commit(&app.editor_ctx, &mut app.documents[0], "C".into(), 1);
    let state = editor_state::load(&app.editor_ctx, &mut app.documents[0]);
    let cursor = state.cursor.char_range().unwrap();
    let mut undo = state.undoer();
    let previous = undo.undo(&(cursor, "C".into())).unwrap().clone();
    let version = app.documents[0].edit_version;
    stage(&mut app, &rx, "B");
    app.finish_interrupted_save_check();
    let state = editor_state::load(&app.editor_ctx, &mut app.documents[0]);
    assert_eq!(state.cursor.char_range().unwrap(), cursor);
    assert_eq!(
        state.undoer().undo(&(cursor, "C".into())).unwrap(),
        &previous
    );
    assert_eq!(app.documents[0].edit_version, version);
    assert_eq!(app.documents[0].text, "C");
    assert_eq!(app.documents[0].saved_text, "B");
}

#[test]
fn every_check_phase_rejects_changed_document_baseline_workspace_and_token() {
    for phase in 0..3 {
        for race in 0..13 {
            let (mut app, rx) = interrupted();
            app.check_interrupted_save();
            let mut command = Some(rx.try_recv().unwrap());
            if phase >= 1 {
                reply(&mut app, command.take().unwrap(), "B");
                command = Some(rx.try_recv().unwrap());
            }
            if phase == 2 {
                reply(&mut app, command.take().unwrap(), "B");
            }
            match race {
                0 => app.documents[0].edit_version += 1,
                1 => app.documents[0].text.push('C'), // also guard text without a version bump
                2 => app.navigation_changed(),
                3 => app.generation += 1,
                4 => app.documents[0].path = "other.rs".into(),
                5 => app.documents[0].id = 99,
                6 => app.documents[0].saved_text = "other base".into(),
                7 => app.documents[0].revision = Some(revision("other base")),
                8 => app.root = "/different-root".into(),
                9 => app.documents[0].interrupted_save.as_mut().unwrap().request += 1,
                10 => app.close_tab_requested = Some(1),
                11 => app.confirm = Some(crate::Confirm::CloseWindow),
                12 => app.allow_close = true,
                _ => unreachable!(),
            }
            let before = format!("{:?}", app.documents[0]);
            if let Some(command) = command {
                reply(&mut app, command, "B");
            }
            app.finish_interrupted_save_check();
            assert_eq!(
                format!("{:?}", app.documents[0]),
                before,
                "phase {phase}, race {race}"
            );
            assert!(rx.try_recv().is_err());
        }
    }
}

#[test]
fn ordinary_dirty_disk_comparison_cannot_resolve_an_interrupted_save() {
    let (mut app, rx) = interrupted();
    app.documents[0].text = "A".into();
    app.compare_with_disk();
    reply(&mut app, rx.try_recv().unwrap(), "B");
    app.reload_from_disk();
    app.finish_disk_reload(&app.editor_ctx.clone());
    assert_eq!(app.documents[0].text, "A");
    assert_eq!(app.documents[0].saved_text, "A");
    assert!(app.documents[0].interrupted_save.is_some());
    assert!(rx.try_recv().is_err());
}

#[test]
fn check_and_save_dispatch_never_overlap_and_navigation_keeps_one_outstanding_read() {
    let (mut app, rx) = interrupted();
    app.documents.push(Document::new(
        2,
        "second.rs".into(),
        "X".into(),
        revision("X"),
    ));
    app.documents[1].text = "Y".into();
    app.save_document(2);
    let save = rx.try_recv().unwrap();
    app.check_interrupted_save();
    assert!(rx.try_recv().is_err());
    app.apply_event(Event {
        generation: app.generation,
        id: save.id,
        connected: true,
        result: Err("conflict".into()),
    });
    app.check_interrupted_save();
    let read = rx.try_recv().unwrap();
    app.save_document(2);
    app.check_interrupted_save();
    app.compare_with_disk();
    app.navigation_changed();
    app.check_interrupted_save();
    assert!(rx.try_recv().is_err());
    reply(&mut app, read, "B");
    app.check_interrupted_save();
    assert!(matches!(rx.try_recv().unwrap().op, Operation::Read { .. }));
}

fn frame(app: &mut CedarApp, time: f64, events: Vec<egui::Event>) {
    let ctx = app.editor_ctx.clone();
    let mut native = eframe::Frame::_new_kittest();
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 900.0),
            )),
            time: Some(time),
            events,
            ..Default::default()
        },
        |ctx| eframe::App::update(app, ctx, &mut native),
    );
}

#[test]
fn same_native_frame_text_paste_and_undo_run_before_baseline_adoption() {
    for input in 0..4 {
        let (mut app, rx) = interrupted();
        frame(&mut app, 1.0, vec![]);
        app.check_interrupted_save();
        reply(&mut app, rx.try_recv().unwrap(), "B");
        let command = rx.try_recv().unwrap();
        app.result_tx
            .send(crate::worker::WorkerEvent::Response(Event {
                generation: app.generation,
                id: command.id,
                connected: true,
                result: Ok(file(&app, "B")),
            }))
            .unwrap();
        app.editor_ctx
            .memory_mut(|memory| memory.request_focus(egui::Id::new(("editor", 1u64))));
        let events = match input {
            0 => vec![],
            1 => vec![egui::Event::Text("typed".into())],
            2 => vec![egui::Event::Paste("pasted".into())],
            3 => vec![egui::Event::Key {
                key: egui::Key::Z,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::COMMAND,
            }],
            _ => unreachable!(),
        };
        frame(&mut app, 2.0, events);
        assert_eq!(
            app.documents[0].interrupted_save.is_some(),
            input != 0,
            "input {input}"
        );
        assert_eq!(
            app.documents[0].saved_text,
            if input == 0 { "B" } else { "A" }
        );
        assert!(rx.try_recv().is_err());
    }
}

fn error_reply(app: &mut CedarApp, command: Command, error: &str) {
    assert!(matches!(command.op, Operation::Read { .. }));
    app.apply_event(Event {
        generation: app.generation,
        id: command.id,
        connected: true,
        result: Err(error.into()),
    });
}

fn interrupted_new_file() -> (CedarApp, Receiver<Command>) {
    let (mut app, rx) = connected();
    app.documents[0].revision = None;
    app.documents[0].saved_text.clear();
    app.documents[0].text = "B".into();
    app.save();
    let command = rx.try_recv().unwrap();
    app.apply_event(Event {
        generation: app.generation,
        id: command.id,
        connected: false,
        result: Err("Reply lost".into()),
    });
    app.generation += 1;
    app.state = ConnectionState::Ready;
    let (worker, rx) = Worker::recording();
    app.worker = Some(worker);
    (app, rx)
}

#[test]
fn twice_confirmed_new_file_absence_only_allows_a_later_explicit_create() {
    let (mut app, rx) = interrupted_new_file();
    app.check_interrupted_save();
    error_reply(&mut app, rx.try_recv().unwrap(), "not_found: file absent");
    assert!(app.documents[0].interrupted_save.is_some());
    error_reply(&mut app, rx.try_recv().unwrap(), "not_found: file absent");
    assert!(app.documents[0].interrupted_save.is_some());
    app.finish_interrupted_save_check();
    assert!(app.documents[0].interrupted_save.is_none());
    assert!(app.documents[0].dirty());
    assert_eq!(app.documents[0].revision, None);
    assert_eq!(app.documents[0].text, "B");
    assert_eq!(app.documents[0].saved_text, "");
    assert!(app
        .interrupted_save_check
        .message()
        .unwrap()
        .contains("currently absent"));
    assert!(rx.try_recv().is_err());
    app.save();
    assert!(matches!(
        rx.try_recv().unwrap().op,
        Operation::Write {
            expected_revision: None,
            ..
        }
    ));
}

#[test]
fn missing_existing_file_unreadable_new_file_and_absence_races_stay_unresolved() {
    for new_file in [false, true] {
        for first_missing in [false, true] {
            let (mut app, rx) = if new_file {
                interrupted_new_file()
            } else {
                interrupted()
            };
            app.check_interrupted_save();
            let first = rx.try_recv().unwrap();
            if first_missing {
                error_reply(&mut app, first, "not_found: file absent");
            } else {
                reply(&mut app, first, "B");
            }
            if new_file || !first_missing {
                let second = rx.try_recv().unwrap();
                if first_missing {
                    reply(&mut app, second, "B");
                } else {
                    error_reply(&mut app, second, "not_found: file absent");
                }
            }
            app.finish_interrupted_save_check();
            assert!(app.documents[0].interrupted_save.is_some());
            assert_eq!(
                app.documents[0].revision,
                (!new_file).then(|| revision("A"))
            );
            assert!(rx.try_recv().is_err());
        }
    }
    for error in [
        "permission_denied: cannot read",
        "io: absent",
        "not_found",
        "unexpected: not_found: absent",
    ] {
        let (mut app, rx) = interrupted_new_file();
        app.check_interrupted_save();
        error_reply(&mut app, rx.try_recv().unwrap(), error);
        app.finish_interrupted_save_check();
        assert!(app.documents[0].interrupted_save.is_some());
        assert!(rx.try_recv().is_err());
    }
}

#[test]
fn new_file_absence_confirmation_is_invalidated_by_same_frame_edits() {
    let (mut app, rx) = interrupted_new_file();
    app.check_interrupted_save();
    error_reply(&mut app, rx.try_recv().unwrap(), "not_found: file absent");
    error_reply(&mut app, rx.try_recv().unwrap(), "not_found: file absent");
    app.documents[0].text = "C".into();
    app.documents[0].edit_version += 1;
    app.finish_interrupted_save_check();
    assert!(app.documents[0].interrupted_save.is_some());
    assert_eq!(app.documents[0].text, "C");
    assert!(rx.try_recv().is_err());
}

#[test]
fn original_baseline_and_new_file_submitted_contents_are_separate_confirmations() {
    let (mut app, rx) = interrupted();
    stage(&mut app, &rx, "A");
    app.finish_interrupted_save_check();
    assert!(app.documents[0].interrupted_save.is_none());
    assert_eq!(app.documents[0].text, "B");
    assert_eq!(app.documents[0].saved_text, "A");
    assert_eq!(app.documents[0].revision, Some(revision("A")));
    assert!(rx.try_recv().is_err());
    app.save();
    assert!(
        matches!(rx.try_recv().unwrap().op, Operation::Write { expected_revision: Some(rev), .. } if rev == revision("A"))
    );

    let (mut app, rx) = interrupted_new_file();
    stage(&mut app, &rx, "B");
    app.finish_interrupted_save_check();
    assert!(app.documents[0].interrupted_save.is_none());
    assert!(!app.documents[0].dirty());
    assert_eq!(app.documents[0].text, "B");
    assert_eq!(app.documents[0].saved_text, "B");
    assert_eq!(app.documents[0].revision, Some(revision("B")));
    assert!(rx.try_recv().is_err());
}

#[test]
fn one_mib_readback_is_supported_but_oversized_and_nul_snapshots_are_rejected() {
    for invalid in [false, true] {
        let (mut app, rx) = connected();
        let submitted = "B".repeat(MAX_FILE_BYTES);
        app.documents[0].text = submitted.clone();
        app.save();
        let command = rx.try_recv().unwrap();
        app.apply_event(Event {
            generation: app.generation,
            id: command.id,
            connected: false,
            result: Err("lost reply".into()),
        });
        app.generation += 1;
        app.state = ConnectionState::Ready;
        let (worker, rx) = Worker::recording();
        app.worker = Some(worker);
        if invalid {
            for invalid_text in [format!("{submitted}x"), "B\0".into()] {
                app.check_interrupted_save();
                reply(&mut app, rx.try_recv().unwrap(), &invalid_text);
                app.finish_interrupted_save_check();
                assert!(app.documents[0].interrupted_save.is_some());
                assert_eq!(app.documents[0].saved_text, "A");
                assert!(rx.try_recv().is_err());
            }
        } else {
            stage(&mut app, &rx, &submitted);
            app.finish_interrupted_save_check();
            assert!(!app.documents[0].dirty());
            assert_eq!(app.documents[0].saved_text.len(), MAX_FILE_BYTES);
            assert!(rx.try_recv().is_err());
        }
    }
}
