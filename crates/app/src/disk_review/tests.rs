use super::*;
use crate::{
    model::Document,
    worker::{Command, Event, Worker},
    ConnectForm, ConnectionState,
};
use std::sync::mpsc::Receiver;

fn revision(byte: char) -> String {
    byte.to_string().repeat(64)
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
        "before".into(),
        revision('a'),
    ));
    app.active_document = Some(1);
    app.next_document = 2;
    let (worker, rx) = Worker::recording();
    app.worker = Some(worker);
    (app, rx)
}

fn file(text: &str, rev: char) -> Payload {
    Payload::File {
        path: "file.rs".into(),
        text: text.into(),
        revision: revision(rev),
    }
}

fn reply(app: &mut CedarApp, command: Command, payload: Payload) {
    assert!(matches!(command.op, Operation::Read { .. }));
    app.apply_event(Event {
        generation: app.generation,
        id: command.id,
        connected: true,
        result: Ok(payload),
    });
}

fn review(app: &mut CedarApp, rx: &Receiver<Command>, text: &str, rev: char) {
    app.compare_with_disk();
    reply(app, rx.try_recv().unwrap(), file(text, rev));
}

fn stage(app: &mut CedarApp, rx: &Receiver<Command>, text: &str, rev: char) {
    review(app, rx, text, rev);
    app.reload_from_disk();
    reply(app, rx.try_recv().unwrap(), file(text, rev));
    assert!(app.disk_review.slot.as_ref().unwrap().staged);
}

fn finish(app: &mut CedarApp) {
    app.finish_disk_reload(&app.editor_ctx.clone());
}

fn history(app: &mut CedarApp, redo: bool) {
    let ctx = app.editor_ctx.clone();
    let _ = ctx.run(
        egui::RawInput {
            events: vec![egui::Event::Key {
                key: egui::Key::Z,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: if redo {
                    egui::Modifiers::COMMAND | egui::Modifiers::SHIFT
                } else {
                    egui::Modifiers::COMMAND
                },
            }],
            ..Default::default()
        },
        |ctx| {
            ctx.memory_mut(|memory| memory.request_focus(egui::Id::new(("editor", 1u64))));
            editor_state::history_shortcut(ctx, &mut app.documents[0]);
        },
    );
}

#[test]
fn dirty_and_never_saved_drafts_compare_without_any_document_mutation() {
    for never_saved in [false, true] {
        let (mut app, rx) = connected();
        editor_state::commit(&app.editor_ctx, &mut app.documents[0], "my 草稿".into(), 2);
        if never_saved {
            app.documents[0].revision = None;
            app.documents[0].saved_text.clear();
        }
        let before = format!("{:?}", app.documents[0]);
        review(&mut app, &rx, "racing file created on disk", 'b');
        app.reload_from_disk();
        finish(&mut app);
        assert_eq!(format!("{:?}", app.documents[0]), before);
        assert!(rx.try_recv().is_err());
        assert_eq!(
            app.disk_review
                .slot
                .as_ref()
                .unwrap()
                .snapshot
                .as_ref()
                .unwrap()
                .text,
            "racing file created on disk"
        );
        history(&mut app, false);
        assert_eq!(app.documents[0].text, "before");
        assert_eq!(
            app.documents[0].revision,
            (!never_saved).then(|| revision('a'))
        );
    }
}

#[test]
fn clean_reload_is_staged_clamps_unicode_cursor_and_undo_redo_use_new_baseline() {
    let (mut app, rx) = connected();
    let mut state = editor_state::load(&app.editor_ctx, &mut app.documents[0]);
    state
        .cursor
        .set_char_range(Some(egui::text::CCursorRange::one(
            egui::text::CCursor::new(6),
        )));
    state.store(&app.editor_ctx, egui::Id::new(("editor", 1u64)));
    app.documents[0].jump_to = Some(50);
    app.find_index = Some(3);
    stage(&mut app, &rx, "é🐻", 'b');
    assert_eq!(
        app.documents[0].text, "before",
        "apply_event only stages the candidate"
    );
    finish(&mut app);
    let doc = &app.documents[0];
    assert_eq!(doc.id, 1);
    assert_eq!(doc.text, "é🐻");
    assert_eq!(doc.saved_text, "é🐻");
    assert_eq!(doc.revision, Some(revision('b')));
    assert_eq!(doc.edit_version, 1);
    assert_eq!(doc.cursor, (1, 3));
    assert_eq!(doc.jump_to, None);
    assert_eq!(app.find_index, None);
    assert!(!doc.dirty());
    history(&mut app, false);
    assert_eq!(app.documents[0].text, "before");
    assert_eq!(app.documents[0].revision, Some(revision('b')));
    assert!(app.documents[0].dirty());
    history(&mut app, true);
    assert_eq!(app.documents[0].text, "é🐻");
    assert!(!app.documents[0].dirty());
    assert!(rx.try_recv().is_err());
    assert!(!app.language.running);
    assert!(!app.execution_trusted());
}

#[test]
fn exact_noop_and_revision_only_adoption_do_not_create_text_history_or_versions() {
    for rev in ['a', 'b'] {
        let (mut app, rx) = connected();
        app.language.sync.observe(1, 0, 0.0);
        app.language.sync.acknowledge(
            1,
            crate::language_sync::Acknowledged {
                version: 17,
                edit_version: 0,
                uri: "file:///project/file.rs".into(),
            },
        );
        stage(&mut app, &rx, "before", rev);
        finish(&mut app);
        assert!(!app.documents[0].undo_initialized);
        assert_eq!(app.documents[0].edit_version, 0);
        assert_eq!(app.documents[0].revision, Some(revision(rev)));
        assert!(!app.documents[0].dirty());
        assert!(app.language.sync.synced(1, 0));
        assert_eq!(app.language.sync.next_version(1), Some(18));
        assert!(rx.try_recv().is_err());
    }
}

#[test]
fn second_read_requires_exact_text_and_revision_and_another_click_on_change() {
    for (text, rev) in [
        ("disk changed again", 'b'),
        ("disk first", 'c'),
        ("disk changed again", 'c'),
    ] {
        let (mut app, rx) = connected();
        review(&mut app, &rx, "disk first", 'b');
        app.reload_from_disk();
        reply(&mut app, rx.try_recv().unwrap(), file(text, rev));
        finish(&mut app);
        assert_eq!(app.documents[0].text, "before");
        let review = app.disk_review.slot.as_ref().unwrap();
        assert!(!review.staged);
        assert!(review.message.as_ref().unwrap().contains("changed again"));
        assert_eq!(review.snapshot.as_ref().unwrap().text, text);
        app.reload_from_disk();
        reply(&mut app, rx.try_recv().unwrap(), file(text, rev));
        finish(&mut app);
        assert_eq!(app.documents[0].text, text);
        assert_eq!(app.documents[0].revision, Some(revision(rev)));
    }
}

#[test]
fn invalid_responses_never_adopt_or_keep_a_reloadable_snapshot() {
    let payloads = vec![
        Payload::File {
            path: "other.rs".into(),
            text: "wrong".into(),
            revision: revision('b'),
        },
        file(&"x".repeat(MAX_FILE_BYTES + 1), 'b'),
        file("binary\0text", 'b'),
        Payload::Written {
            revision: revision('b'),
        },
    ]
    .into_iter()
    .chain(
        [
            "".to_owned(),
            "a".repeat(65),
            "é".repeat(32),
            "F".repeat(64),
            "g".repeat(64),
        ]
        .into_iter()
        .map(|revision| Payload::File {
            path: "file.rs".into(),
            text: "text".into(),
            revision,
        }),
    );
    for payload in payloads {
        let (mut app, rx) = connected();
        review(&mut app, &rx, "disk", 'b');
        app.reload_from_disk();
        reply(&mut app, rx.try_recv().unwrap(), payload);
        finish(&mut app);
        assert_eq!(app.documents[0].text, "before");
        assert_eq!(app.documents[0].revision, Some(revision('a')));
        assert!(app.disk_review.slot.as_ref().unwrap().snapshot.is_none());
        app.reload_from_disk();
        assert!(rx.try_recv().is_err());
    }
}

#[test]
fn dismissed_reads_and_errors_cannot_reopen_or_replace_a_newer_review() {
    let (mut app, rx) = connected();
    app.compare_with_disk();
    let old = rx.try_recv().unwrap();
    let old_ticket = app.disk_review.epoch;
    app.dismiss_disk_review();
    assert_eq!(app.pending.len(), 1);
    reply(&mut app, old, file("old reply", 'b'));
    assert!(app.disk_review.slot.is_none());
    review(&mut app, &rx, "new review", 'c');
    app.error = Some("new status".into());
    // Exercise both event/job removal and the independent epoch guard.
    app.apply_disk_read(
        0,
        old_ticket,
        Purpose::Compare,
        Ok(file("old reply", 'b')),
        true,
    );
    app.apply_disk_read(
        0,
        old_ticket,
        Purpose::Compare,
        Err("old transport error".into()),
        false,
    );
    assert_eq!(app.error.as_deref(), Some("new status"));
    assert!(app.ready());
    assert_eq!(
        app.disk_review
            .slot
            .as_ref()
            .unwrap()
            .snapshot
            .as_ref()
            .unwrap()
            .text,
        "new review"
    );
    app.dismiss_disk_review();
    app.apply_disk_read(
        0,
        old_ticket,
        Purpose::Compare,
        Ok(file("old reply", 'b')),
        true,
    );
    assert!(app.disk_review.slot.is_none());
}

#[test]
fn stale_source_success_and_error_are_rejected_before_processing() {
    for change in 0..9 {
        for error in [false, true] {
            let (mut app, rx) = connected();
            app.compare_with_disk();
            let command = rx.try_recv().unwrap();
            let ticket = app.disk_review.epoch;
            match change {
                0 => {
                    app.documents[0].text.push('!');
                    app.documents[0].edit_version += 1;
                }
                1 => app.documents[0].revision = Some(revision('c')),
                2 => app.documents[0].path = "another.rs".into(),
                3 => {
                    app.documents[0].id = 2;
                    app.active_document = Some(2);
                }
                4 => app.active_document = None,
                5 => app.navigation_epoch += 1,
                6 => app.generation += 1,
                7 => app.state = ConnectionState::Disconnected,
                8 => {
                    let mut info = crate::agent_support::full_test_agent();
                    info.capabilities.retain(|name| name != "read");
                    app.agent_info = Some(info);
                }
                _ => unreachable!(),
            }
            let before = format!("{:?}", app.documents[0]);
            app.error = Some("current status".into());
            app.apply_disk_read(
                0,
                ticket,
                Purpose::Compare,
                if error {
                    Err("stale error".into())
                } else {
                    Ok(file("old disk", 'b'))
                },
                false,
            );
            assert_eq!(format!("{:?}", app.documents[0]), before);
            assert_eq!(app.error.as_deref(), Some("current status"));
            assert!(app
                .disk_review
                .slot
                .as_ref()
                .is_none_or(|review| review.snapshot.is_none()));
            app.pending.remove(&command.id);
        }
    }
}

#[test]
fn closing_reopening_and_workspace_navigation_release_review_state() {
    let (mut app, rx) = connected();
    app.compare_with_disk();
    let old = rx.try_recv().unwrap();
    app.remove_tab(1);
    app.documents.push(Document::new(
        2,
        "file.rs".into(),
        "reopened".into(),
        revision('d'),
    ));
    app.active_document = Some(2);
    reply(&mut app, old, file("old disk", 'b'));
    assert!(app.disk_review.slot.is_none());
    assert_eq!(app.documents[0].text, "reopened");
    app.compare_with_disk();
    let delayed = rx.try_recv().unwrap();
    for _ in 0..100 {
        app.navigation_changed();
        app.compare_with_disk();
        assert_eq!(app.pending.len(), 1);
        assert!(rx.try_recv().is_err());
        assert!(app.disk_review.slot.is_none());
    }
    reply(&mut app, delayed, file("delayed", 'b'));
    assert!(app.pending.is_empty());
    assert!(!app.disk_review.busy());
    app.compare_with_disk();
    rx.try_recv().unwrap();
    app.disconnected("connection lost".into());
    assert!(app.disk_review.slot.is_none());
}

#[test]
fn repeated_clicks_share_one_read_and_jobs_contain_no_text_or_path() {
    let (mut app, rx) = connected();
    app.compare_with_disk();
    for _ in 0..100 {
        app.compare_with_disk();
        app.reload_from_disk();
    }
    let first = rx.try_recv().unwrap();
    assert!(rx.try_recv().is_err());
    assert!(matches!(
        app.pending.get(&first.id),
        Some(Job::DiskReview {
            ticket: _,
            purpose: Purpose::Compare
        })
    ));
    reply(&mut app, first, file("disk", 'b'));
    app.reload_from_disk();
    for _ in 0..100 {
        app.compare_with_disk();
        app.reload_from_disk();
    }
    reply(&mut app, rx.try_recv().unwrap(), file("disk", 'b'));
    assert!(rx.try_recv().is_err());
    assert!(app.pending.is_empty());
}

#[test]
fn same_frame_edits_transactions_and_undo_prevent_staged_reload() {
    for edit in 0..3 {
        let (mut app, rx) = connected();
        if edit == 2 {
            editor_state::commit(
                &app.editor_ctx,
                &mut app.documents[0],
                "clean edit".into(),
                0,
            );
            app.documents[0].acknowledge_save("clean edit".into(), revision('a'));
        }
        stage(&mut app, &rx, "disk", 'b');
        match edit {
            0 => {
                app.documents[0].text.push_str(" typing");
                app.documents[0].edit_version += 1;
            }
            1 => editor_state::commit(
                &app.editor_ctx,
                &mut app.documents[0],
                "completion or formatting".into(),
                0,
            ),
            2 => history(&mut app, false),
            _ => unreachable!(),
        }
        let before = format!("{:?}", app.documents[0]);
        finish(&mut app);
        assert_eq!(format!("{:?}", app.documents[0]), before);
        assert!(!app.disk_review.slot.as_ref().unwrap().staged);
        assert_eq!(app.documents[0].revision, Some(revision('a')));
        assert!(rx.try_recv().is_err());
    }
}

#[test]
fn complete_native_frame_applies_input_after_read_event_before_reload_commit() {
    for input in 0..5 {
        let (mut app, rx) = connected();
        app.open_form = false;
        let ctx = app.editor_ctx.clone();
        let mut native = eframe::Frame::_new_kittest();
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 900.0),
                )),
                time: Some(1.0),
                ..Default::default()
            },
            |ctx| eframe::App::update(&mut app, ctx, &mut native),
        );
        if input == 3 {
            editor_state::commit(&ctx, &mut app.documents[0], "saved edit".into(), 0);
            app.documents[0].acknowledge_save("saved edit".into(), revision('a'));
        }
        review(&mut app, &rx, "disk", 'b');
        app.reload_from_disk();
        let command = rx.try_recv().unwrap();
        app.result_tx
            .send(crate::worker::WorkerEvent::Response(Event {
                generation: app.generation,
                id: command.id,
                connected: true,
                result: Ok(file("disk", 'b')),
            }))
            .unwrap();
        ctx.memory_mut(|memory| memory.request_focus(egui::Id::new(("editor", 1u64))));
        let events = match input {
            0 => vec![],
            1 => vec![egui::Event::Text("typed now".into())],
            2 => vec![egui::Event::Paste("pasted now".into())],
            3 => vec![egui::Event::Key {
                key: egui::Key::Z,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::COMMAND,
            }],
            4 => vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            _ => unreachable!(),
        };
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 900.0),
                )),
                time: Some(2.0),
                events,
                ..Default::default()
            },
            |ctx| eframe::App::update(&mut app, ctx, &mut native),
        );
        assert_eq!(
            app.documents[0].revision,
            Some(revision(if input == 0 { 'b' } else { 'a' }))
        );
        match input {
            0 => assert_eq!(app.documents[0].text, "disk"),
            1 => assert!(app.documents[0].text.contains("typed now")),
            2 => assert!(app.documents[0].text.contains("pasted now")),
            3 => assert_eq!(app.documents[0].text, "before"),
            4 => {
                assert_eq!(app.documents[0].text, "before");
                assert!(app.disk_review.slot.is_none());
            }
            _ => unreachable!(),
        }
        assert!(rx.try_recv().is_err());
    }
}

fn native_frame(app: &mut CedarApp, time: f64, events: Vec<egui::Event>) {
    let ctx = app.editor_ctx.clone();
    let mut frame = eframe::Frame::_new_kittest();
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
        |ctx| eframe::App::update(app, ctx, &mut frame),
    );
}

fn shortcut(key: egui::Key, modifiers: egui::Modifiers) -> Vec<egui::Event> {
    [true, false]
        .into_iter()
        .map(|pressed| egui::Event::Key {
            key,
            physical_key: Some(key),
            pressed,
            repeat: false,
            modifiers,
        })
        .collect()
}

#[test]
fn native_reload_undo_review_close_and_refocus_preserves_redo() {
    for navigation in ["click", "arrow"] {
        let (mut app, rx) = connected();
        app.open_form = false;
        app.tools_open = false;
        native_frame(&mut app, 0.0, vec![]);
        stage(&mut app, &rx, "更新后的磁盘\nsecond line", 'b');
        finish(&mut app);
        app.dismiss_disk_review();
        native_frame(&mut app, 1.0, vec![]);
        native_frame(
            &mut app,
            2.0,
            shortcut(egui::Key::Z, egui::Modifiers::COMMAND),
        );
        assert_eq!(app.documents[0].text, "before");
        assert!(app.documents[0].dirty());
        review(&mut app, &rx, "更新后的磁盘\nsecond line", 'b');
        native_frame(&mut app, 3.0, vec![]);
        app.editor_ctx
            .memory_mut(|memory| memory.surrender_focus(egui::Id::new(("editor", 1u64))));
        native_frame(&mut app, 5.0, vec![]);
        app.dismiss_disk_review();
        native_frame(&mut app, 6.0, vec![]);
        if navigation == "click" {
            let response = app
                .editor_ctx
                .read_response(egui::Id::new(("editor", 1u64)))
                .unwrap();
            let pos = response.rect.min + egui::vec2(34.0, 8.0);
            native_frame(
                &mut app,
                7.0,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
            native_frame(
                &mut app,
                7.1,
                vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
        } else {
            app.editor_ctx
                .memory_mut(|memory| memory.request_focus(egui::Id::new(("editor", 1u64))));
            native_frame(
                &mut app,
                7.0,
                shortcut(egui::Key::ArrowRight, egui::Modifiers::SHIFT),
            );
        }
        assert!(
            app.documents[0].cursor.1 > 1,
            "navigation must move the cursor"
        );
        native_frame(&mut app, 9.0, vec![]);
        native_frame(&mut app, 11.0, vec![]);
        native_frame(
            &mut app,
            12.0,
            shortcut(
                egui::Key::Z,
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
            ),
        );
        assert_eq!(
            app.documents[0].text, "更新后的磁盘\nsecond line",
            "Redo lost after {navigation}"
        );
        assert!(!app.documents[0].dirty());
        assert_eq!(app.documents[0].revision, Some(revision('b')));
        assert!(rx.try_recv().is_err());
        native_frame(
            &mut app,
            13.0,
            shortcut(egui::Key::Z, egui::Modifiers::COMMAND),
        );
        native_frame(
            &mut app,
            14.0,
            shortcut(egui::Key::A, egui::Modifiers::COMMAND),
        );
        let mut edit = shortcut(egui::Key::ArrowRight, egui::Modifiers::NONE);
        edit.push(egui::Event::Text(" fresh edit".into()));
        native_frame(&mut app, 15.0, edit);
        let edited = app.documents[0].text.clone();
        assert!(edited.contains("fresh edit"));
        native_frame(
            &mut app,
            16.0,
            shortcut(egui::Key::Y, egui::Modifiers::COMMAND),
        );
        assert_eq!(
            app.documents[0].text, edited,
            "real editing must clear redo"
        );
        assert!(app.documents[0].dirty());
    }
}

#[test]
fn native_many_cursor_moves_do_not_evict_reload_undo_or_redo() {
    let (mut app, rx) = connected();
    app.open_form = false;
    app.tools_open = false;
    native_frame(&mut app, 0.0, vec![]);
    stage(&mut app, &rx, "disk text", 'b');
    finish(&mut app);
    app.dismiss_disk_review();
    for frame in 1..=40 {
        let key = if frame % 2 == 0 {
            egui::Key::ArrowLeft
        } else {
            egui::Key::ArrowRight
        };
        native_frame(
            &mut app,
            f64::from(frame),
            shortcut(key, egui::Modifiers::SHIFT),
        );
    }
    native_frame(
        &mut app,
        41.0,
        shortcut(egui::Key::Z, egui::Modifiers::COMMAND),
    );
    assert_eq!(app.documents[0].text, "before");
    assert!(app.documents[0].dirty());
    for frame in 42..=81 {
        let key = if frame % 2 == 0 {
            egui::Key::ArrowRight
        } else {
            egui::Key::ArrowLeft
        };
        native_frame(
            &mut app,
            f64::from(frame),
            shortcut(key, egui::Modifiers::SHIFT),
        );
    }
    native_frame(
        &mut app,
        82.0,
        shortcut(egui::Key::Y, egui::Modifiers::COMMAND),
    );
    assert_eq!(app.documents[0].text, "disk text");
    assert!(!app.documents[0].dirty());
    assert!(rx.try_recv().is_err());
}

#[test]
fn reload_rechecks_save_and_every_close_transition_at_both_barriers() {
    for at_commit in [false, true] {
        for race in 0..7 {
            let (mut app, rx) = connected();
            review(&mut app, &rx, "disk", 'b');
            app.reload_from_disk();
            let read = rx.try_recv().unwrap();
            if at_commit {
                reply(
                    &mut app,
                    Command {
                        id: read.id,
                        op: Operation::Read {
                            path: "file.rs".into(),
                        },
                    },
                    file("disk", 'b'),
                );
            }
            match race {
                0 => app.documents[0].saving = true,
                1 => {
                    app.pending.insert(
                        900,
                        Job::Save {
                            document: 1,
                            snapshot: "before".into(),
                            submission: None,
                        },
                    );
                }
                2 => app.close_tab_requested = Some(1),
                3 => app.confirm = Some(crate::Confirm::CloseWindow),
                4 => app.close_after_language_stop = true,
                5 => app.recovery.closing = Some(vec![(1, 0)]),
                6 => app.allow_close = true,
                _ => unreachable!(),
            }
            if !at_commit {
                reply(&mut app, read, file("disk", 'b'));
            }
            finish(&mut app);
            assert_eq!(app.documents[0].text, "before");
            assert_eq!(app.documents[0].revision, Some(revision('a')));
            assert!(!app.disk_review.slot.as_ref().unwrap().staged);
        }
    }
}

#[test]
fn oversized_draft_is_never_copied_into_review_and_invalid_source_tokens_are_rejected() {
    let (mut app, rx) = connected();
    app.documents[0].text = "草".repeat(MAX_FILE_BYTES / 3 + 1);
    let pointer = app.documents[0].text.as_ptr();
    review(&mut app, &rx, "disk", 'b');
    assert_eq!(app.documents[0].text.as_ptr(), pointer);
    let ctx = app.editor_ctx.clone();
    let _ = ctx.run(egui::RawInput::default(), |ctx| app.disk_review_window(ctx));
    assert!(app
        .disk_review
        .slot
        .as_ref()
        .unwrap()
        .draft_layout
        .galley
        .is_none());
    app.dismiss_disk_review();
    app.documents[0].revision = Some("a".repeat(MAX_FILE_BYTES));
    app.compare_with_disk();
    assert!(app.disk_review.slot.is_none());
    assert!(rx.try_recv().is_err());
    app.documents[0].revision = None;
    app.documents[0].path = "x".repeat(MAX_PATH_BYTES + 1);
    app.compare_with_disk();
    assert!(app.disk_review.slot.is_none());
    assert!(rx.try_recv().is_err());
}

#[test]
fn pane_layouts_are_reused_across_idle_frames_and_released_on_dismissal() {
    let (mut app, rx) = connected();
    review(&mut app, &rx, "disk", 'b');
    let ctx = app.editor_ctx.clone();
    let _ = ctx.run(egui::RawInput::default(), |ctx| app.disk_review_window(ctx));
    let review = app.disk_review.slot.as_ref().unwrap();
    let draft = Arc::downgrade(review.draft_layout.galley.as_ref().unwrap());
    let disk = Arc::downgrade(review.disk_layout.galley.as_ref().unwrap());
    for _ in 0..10 {
        let _ = ctx.run(egui::RawInput::default(), |ctx| app.disk_review_window(ctx));
        let review = app.disk_review.slot.as_ref().unwrap();
        assert!(Arc::ptr_eq(
            &draft.upgrade().unwrap(),
            review.draft_layout.galley.as_ref().unwrap()
        ));
        assert!(Arc::ptr_eq(
            &disk.upgrade().unwrap(),
            review.disk_layout.galley.as_ref().unwrap()
        ));
    }
    app.dismiss_disk_review();
    assert!(app.disk_review.slot.is_none());
    assert!(!app.documents[0].undo_initialized);
}

#[test]
fn cached_panes_rebuild_when_replacement_fonts_activate_next_pass() {
    let (mut app, rx) = connected();
    review(&mut app, &rx, "disk 中", 'b');
    let ctx = app.editor_ctx.clone();
    let _ = ctx.run(egui::RawInput::default(), |ctx| app.disk_review_window(ctx));
    let old_atlas = app
        .disk_review
        .slot
        .as_ref()
        .unwrap()
        .disk_layout
        .atlas
        .as_ref()
        .unwrap()
        .clone();
    let mut definitions = egui::FontDefinitions::default();
    definitions
        .families
        .get_mut(&egui::FontFamily::Monospace)
        .unwrap()
        .reverse();
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        ctx.set_fonts(definitions.clone());
        app.disk_review_window(ctx);
        assert!(Arc::ptr_eq(
            &old_atlas,
            app.disk_review
                .slot
                .as_ref()
                .unwrap()
                .disk_layout
                .atlas
                .as_ref()
                .unwrap()
        ));
    });
    let old_galley = app
        .disk_review
        .slot
        .as_ref()
        .unwrap()
        .disk_layout
        .galley
        .as_ref()
        .unwrap()
        .clone();
    let _ = ctx.run(egui::RawInput::default(), |ctx| app.disk_review_window(ctx));
    let review = app.disk_review.slot.as_ref().unwrap();
    let active_atlas = ctx.fonts(|fonts| fonts.texture_atlas());
    assert!(!Arc::ptr_eq(&old_atlas, &active_atlas));
    assert!(Arc::ptr_eq(
        review.disk_layout.atlas.as_ref().unwrap(),
        &active_atlas
    ));
    assert!(Arc::ptr_eq(
        review.draft_layout.atlas.as_ref().unwrap(),
        &active_atlas
    ));
    assert!(!Arc::ptr_eq(
        &old_galley,
        review.disk_layout.galley.as_ref().unwrap()
    ));
}

#[test]
fn current_transport_failure_disconnects_even_after_dismissal_but_old_generation_cannot() {
    for stale in [false, true] {
        let (mut app, rx) = connected();
        app.compare_with_disk();
        let command = rx.try_recv().unwrap();
        let generation = app.generation;
        app.dismiss_disk_review();
        if stale {
            app.generation += 1;
        }
        app.apply_event(Event {
            generation,
            id: command.id,
            connected: false,
            result: Err("transport ended".into()),
        });
        assert_eq!(app.ready(), stale);
        assert!(app.disk_review.slot.is_none());
        if !stale {
            assert!(app.pending.is_empty());
            assert!(!app.disk_review.busy());
        }
    }
}

#[test]
fn read_only_backend_needs_only_read_capability_and_never_writes() {
    let (mut app, rx) = connected();
    let mut info = crate::agent_support::full_test_agent();
    info.capabilities = vec!["list".into(), "read".into()];
    app.agent_info = Some(info);
    stage(&mut app, &rx, "disk", 'b');
    finish(&mut app);
    assert_eq!(app.documents[0].text, "disk");
    assert!(rx.try_recv().is_err());
    assert!(!app.backend_supports("write"));
    assert!(!app.execution_trusted());
}

#[test]
fn actual_backend_missing_binary_oversized_and_directory_reads_never_create_or_overwrite() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("binary"), b"binary\0data").unwrap();
    std::fs::write(
        directory.path().join("large"),
        vec![b'x'; MAX_FILE_BYTES + 1],
    )
    .unwrap();
    std::fs::create_dir(directory.path().join("folder")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink("binary", directory.path().join("link")).unwrap();
    let mut backend = cedar_workspace::Workspace::open(directory.path()).unwrap();
    for path in ["missing", "binary", "large", "folder"]
        .into_iter()
        .chain(if cfg!(unix) { Some("link") } else { None })
    {
        let (mut app, rx) = connected();
        app.documents[0].path = path.into();
        app.documents[0].revision = None;
        app.documents[0].text = "unsaved draft".into();
        app.compare_with_disk();
        let command = rx.try_recv().unwrap();
        assert!(matches!(command.op, Operation::Read { .. }));
        let error = backend.handle(command.op).unwrap_err();
        app.apply_event(Event {
            generation: app.generation,
            id: command.id,
            connected: true,
            result: Err(format!("{error:?}")),
        });
        app.reload_from_disk();
        assert_eq!(app.documents[0].text, "unsaved draft");
        assert!(app.disk_review.slot.as_ref().unwrap().snapshot.is_none());
        assert!(rx.try_recv().is_err());
    }
    assert!(!directory.path().join("missing").exists());
    assert_eq!(
        std::fs::read(directory.path().join("binary")).unwrap(),
        b"binary\0data"
    );
    assert_eq!(
        std::fs::metadata(directory.path().join("large"))
            .unwrap()
            .len(),
        (MAX_FILE_BYTES + 1) as u64
    );
    assert!(directory.path().join("folder").is_dir());
}

fn profile_app() -> (CedarApp, Receiver<Command>, String) {
    let (mut app, rx) = connected();
    let text = r#"{"version":1,"profiles":[{"name":"Build","program":"cargo","args":["build"],"timeout_secs":30}]}"#.to_owned();
    app.documents[0] = Document::new(
        1,
        crate::profile_ui::PATH.into(),
        text.clone(),
        revision('a'),
    );
    app.profiles.connected(app.recovery_workspace().unwrap());
    app.load_profiles();
    app.select_profile(Some(0));
    (app, rx, text)
}

fn profile_stage(app: &mut CedarApp, rx: &Receiver<Command>, text: &str) {
    app.compare_with_disk();
    let payload = || Payload::File {
        path: crate::profile_ui::PATH.into(),
        text: text.into(),
        revision: revision('b'),
    };
    reply(app, rx.try_recv().unwrap(), payload());
    app.reload_from_disk();
    reply(app, rx.try_recv().unwrap(), payload());
}

#[test]
fn profile_drafts_are_retained_and_queued_save_run_and_load_are_invalidated() {
    for action in [
        crate::profile_ui::Action::Save,
        crate::profile_ui::Action::Run,
        crate::profile_ui::Action::Load,
    ] {
        let (mut app, rx, text) = profile_app();
        app.profiles.draft.args.push("form-only draft".into());
        app.profiles.changed();
        let draft = app.profiles.draft.clone();
        profile_stage(&mut app, &rx, &text.replace("Build", "Disk profile"));
        app.queue_profile_action(action);
        finish(&mut app);
        app.finish_profile_actions();
        assert_eq!(app.profiles.draft, draft);
        assert!(app.profiles.dirty());
        assert!(app.profile_run_problem().is_some());
        assert!(app.documents[0].text.contains("Disk profile"));
        assert!(!app.documents[0].dirty());
        app.save_profile();
        assert!(rx.try_recv().is_err());
    }
}

#[test]
fn profile_revision_only_reload_invalidates_source_and_same_frame_form_input_blocks_reload() {
    for input in [false, true] {
        let (mut app, rx, text) = profile_app();
        profile_stage(&mut app, &rx, &text);
        if input {
            app.profiles.draft.name = "New form input".into();
            app.profiles.changed();
        }
        finish(&mut app);
        assert_eq!(app.documents[0].edit_version, 0);
        assert_eq!(
            app.documents[0].revision,
            Some(revision(if input { 'a' } else { 'b' }))
        );
        if input {
            assert_eq!(app.profiles.draft.name, "New form input");
        } else {
            assert!(app.profile_run_problem().is_some());
        }
        assert!(rx.try_recv().is_err());
    }
}

const MERGE_BASE: &str = "old α\r\nanchor one\nanchor two\r\nold ω\n";
const MERGE_DRAFT: &str = "draft 🦀\r\nanchor one\nanchor two\r\nold ω\n";
const MERGE_DISK: &str = "old α\r\nanchor one\nanchor two\r\ndisk 🐻";
const MERGE_RESULT: &str = "draft 🦀\r\nanchor one\nanchor two\r\ndisk 🐻";

fn merge_app() -> (CedarApp, Receiver<Command>) {
    let (mut app, rx) = connected();
    app.open_form = false;
    app.tools_open = false;
    app.documents[0] = Document::new(
        1,
        "file.rs".into(),
        MERGE_BASE.into(),
        content_revision(MERGE_BASE),
    );
    app.documents[0].text = MERGE_DRAFT.into();
    app.documents[0].edit_version = 1;
    (app, rx)
}

fn merge_file(path: &str, text: &str) -> Payload {
    Payload::File {
        path: path.into(),
        text: text.into(),
        revision: content_revision(text),
    }
}

fn merge_review(app: &mut CedarApp, rx: &Receiver<Command>, disk: &str) {
    let path = app.active().unwrap().path.clone();
    app.compare_with_disk();
    reply(app, rx.try_recv().unwrap(), merge_file(&path, disk));
}

fn merge_preview(app: &mut CedarApp, rx: &Receiver<Command>) {
    merge_review(app, rx, MERGE_DISK);
    app.preview_disk_merge(&app.editor_ctx.clone());
    assert!(
        app.disk_review.slot.as_ref().unwrap().merge.is_some(),
        "{:?}",
        app.disk_review.slot.as_ref().unwrap().message
    );
    assert!(rx.try_recv().is_err(), "Preview cannot read or write");
}

fn merge_stage(app: &mut CedarApp, rx: &Receiver<Command>) {
    merge_preview(app, rx);
    app.apply_disk_merge(&app.editor_ctx.clone());
    reply(
        app,
        rx.try_recv().unwrap(),
        merge_file("file.rs", MERGE_DISK),
    );
    assert!(app.disk_review.slot.as_ref().unwrap().merge_staged);
}

fn set_merge_selection(app: &mut CedarApp, range: egui::text::CCursorRange) {
    let mut state = editor_state::load(&app.editor_ctx, &mut app.documents[0]);
    state.cursor.set_char_range(Some(range));
    state.store(&app.editor_ctx, egui::Id::new(("editor", 1u64)));
}

fn merge_range() -> egui::text::CCursorRange {
    egui::text::CCursorRange {
        primary: egui::text::CCursor {
            index: 2,
            prefer_next_row: true,
        },
        secondary: egui::text::CCursor {
            index: 8,
            prefer_next_row: false,
        },
    }
}

fn click_merge_button(app: &mut CedarApp, time: f64, name: &str) {
    let rect = app
        .editor_ctx
        .data(|data| data.get_temp::<egui::Rect>(egui::Id::new(name)))
        .unwrap();
    let pos = rect.center();
    native_frame(
        app,
        time,
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    native_frame(
        app,
        time + 0.01,
        vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
}

#[test]
fn merge_preview_cancel_is_inert_even_without_native_undo_initialization() {
    let (mut app, rx) = merge_app();
    let before = format!("{:?}", app.documents[0]);
    merge_preview(&mut app, &rx);
    assert_eq!(
        app.disk_review
            .slot
            .as_ref()
            .unwrap()
            .merge
            .as_ref()
            .unwrap()
            .text,
        MERGE_RESULT
    );
    assert_eq!(format!("{:?}", app.documents[0]), before);
    assert!(native_selection(&app.editor_ctx, 1).is_none());
    app.cancel_disk_merge();
    assert!(app.disk_review.slot.as_ref().unwrap().merge.is_none());
    assert_eq!(format!("{:?}", app.documents[0]), before);
    assert!(rx.try_recv().is_err());
}

#[test]
fn native_merge_preview_cancel_apply_and_undo_redo_restore_full_selection() {
    let (mut app, rx) = merge_app();
    native_frame(&mut app, 0.0, vec![]);
    set_merge_selection(&mut app, merge_range());
    merge_review(&mut app, &rx, MERGE_DISK);
    native_frame(&mut app, 1.0, vec![]);
    native_frame(&mut app, 1.1, vec![]);
    let before_selection = native_selection(&app.editor_ctx, 1);
    let version = app.documents[0].edit_version;
    click_merge_button(&mut app, 2.0, "disk_merge_preview_button");
    assert!(app.disk_review.slot.as_ref().unwrap().merge.is_some());
    assert_eq!(app.documents[0].text, MERGE_DRAFT);
    assert_eq!(app.documents[0].saved_text, MERGE_BASE);
    assert!(same_selection(
        before_selection,
        native_selection(&app.editor_ctx, 1)
    ));
    native_frame(&mut app, 2.2, vec![]);
    click_merge_button(&mut app, 3.0, "disk_merge_cancel_button");
    assert!(app.disk_review.slot.as_ref().unwrap().merge.is_none());
    assert_eq!(app.documents[0].edit_version, version);
    assert!(same_selection(
        before_selection,
        native_selection(&app.editor_ctx, 1)
    ));
    assert!(rx.try_recv().is_err());
    native_frame(&mut app, 3.2, vec![]);
    click_merge_button(&mut app, 4.0, "disk_merge_preview_button");
    native_frame(&mut app, 4.2, vec![]);
    click_merge_button(&mut app, 5.0, "disk_merge_apply_button");
    let read = rx.try_recv().unwrap();
    assert!(rx.try_recv().is_err());
    reply(&mut app, read, merge_file("file.rs", MERGE_DISK));
    assert_eq!(app.documents[0].text, MERGE_DRAFT, "response only stages");
    native_frame(&mut app, 6.0, vec![]);
    assert_eq!(app.documents[0].text, MERGE_RESULT);
    assert_eq!(app.documents[0].saved_text, MERGE_DISK);
    assert_eq!(
        app.documents[0].revision,
        Some(content_revision(MERGE_DISK))
    );
    assert_eq!(app.documents[0].edit_version, version + 1);
    assert!(app.documents[0].dirty());
    app.dismiss_disk_review();
    native_frame(
        &mut app,
        7.0,
        shortcut(egui::Key::Z, egui::Modifiers::COMMAND),
    );
    assert_eq!(app.documents[0].text, MERGE_DRAFT);
    assert!(same_selection(
        before_selection,
        native_selection(&app.editor_ctx, 1)
    ));
    assert_eq!(app.documents[0].saved_text, MERGE_DISK);
    assert!(app.documents[0].dirty());
    native_frame(
        &mut app,
        8.0,
        shortcut(egui::Key::Y, egui::Modifiers::COMMAND),
    );
    assert_eq!(app.documents[0].text, MERGE_RESULT);
    assert!(app.documents[0].dirty());
    assert!(rx.try_recv().is_err());
    assert!(!app.execution_trusted());
}

#[test]
fn merge_rechecks_exact_text_base_and_full_selection_at_apply_response_and_final_frame() {
    for barrier in 0..3 {
        for change in 0..7 {
            let (mut app, rx) = merge_app();
            set_merge_selection(&mut app, merge_range());
            merge_preview(&mut app, &rx);
            let read = if barrier > 0 {
                app.apply_disk_merge(&app.editor_ctx.clone());
                Some(rx.try_recv().unwrap())
            } else {
                None
            };
            if barrier == 2 {
                let read = read.as_ref().unwrap();
                reply(
                    &mut app,
                    Command {
                        id: read.id,
                        op: Operation::Read {
                            path: "file.rs".into(),
                        },
                    },
                    merge_file("file.rs", MERGE_DISK),
                );
            }
            match change {
                0 => app.documents[0].text.push_str("new input"), // even without version increment
                1 => app.documents[0].saved_text.push_str("changed base"),
                2 => app.documents[0].edit_version += 1,
                3 => {
                    let mut range = merge_range();
                    range.secondary.index += 1;
                    set_merge_selection(&mut app, range);
                }
                4 => {
                    let mut range = merge_range();
                    range.primary.prefer_next_row = false;
                    set_merge_selection(&mut app, range);
                }
                5 => {
                    let mut range = merge_range();
                    range.secondary.prefer_next_row = true;
                    set_merge_selection(&mut app, range);
                }
                6 => app.documents[0].jump_to = Some(1),
                _ => unreachable!(),
            }
            let before = format!("{:?}", app.documents[0]);
            if barrier == 0 {
                app.apply_disk_merge(&app.editor_ctx.clone());
            }
            if barrier == 1 {
                reply(&mut app, read.unwrap(), merge_file("file.rs", MERGE_DISK));
            }
            app.finish_disk_merge(&app.editor_ctx.clone());
            assert_eq!(
                format!("{:?}", app.documents[0]),
                before,
                "barrier {barrier}, change {change}"
            );
            assert!(app.disk_review.slot.as_ref().unwrap().merge.is_none());
            assert!(rx.try_recv().is_err());
        }
    }
}

#[test]
fn merge_cannot_compose_with_same_frame_typing_paste_undo_selection_or_escape() {
    for change in 0..5 {
        let (mut app, rx) = merge_app();
        native_frame(&mut app, 0.0, vec![]);
        editor_state::commit(
            &app.editor_ctx,
            &mut app.documents[0],
            MERGE_DRAFT.replace("draft", "draft edited"),
            3,
        );
        merge_stage(&mut app, &rx);
        let events = match change {
            0 => vec![egui::Event::Text("input".into())],
            1 => vec![egui::Event::Paste("paste".into())],
            2 => shortcut(egui::Key::Z, egui::Modifiers::COMMAND),
            3 => shortcut(egui::Key::ArrowRight, egui::Modifiers::SHIFT),
            4 => shortcut(egui::Key::Escape, egui::Modifiers::NONE),
            _ => unreachable!(),
        };
        native_frame(&mut app, 1.0, events);
        assert_eq!(app.documents[0].saved_text, MERGE_BASE);
        assert_eq!(
            app.documents[0].revision,
            Some(content_revision(MERGE_BASE))
        );
        assert!(app
            .disk_review
            .slot
            .as_ref()
            .is_none_or(|review| review.merge.is_none()));
        assert!(rx.try_recv().is_err());
    }
}

#[test]
fn changed_or_invalid_verification_clears_merge_and_requires_explicit_new_preview() {
    for case in 0..6 {
        let (mut app, rx) = merge_app();
        merge_preview(&mut app, &rx);
        app.apply_disk_merge(&app.editor_ctx.clone());
        let read = rx.try_recv().unwrap();
        let result = match case {
            0 => Ok(merge_file(
                "file.rs",
                &MERGE_DISK.replace("disk", "new disk"),
            )),
            1 => Ok(Payload::File {
                path: "file.rs".into(),
                text: MERGE_DISK.into(),
                revision: revision('a'),
            }),
            2 => Ok(merge_file("other.rs", MERGE_DISK)),
            3 => Err("not_found: absent".into()),
            4 => Ok(merge_file("file.rs", "binary\0contents")),
            5 => Ok(merge_file("file.rs", &"x".repeat(MAX_FILE_BYTES + 1))),
            _ => unreachable!(),
        };
        app.apply_event(Event {
            generation: app.generation,
            id: read.id,
            connected: true,
            result,
        });
        app.finish_disk_merge(&app.editor_ctx.clone());
        assert_eq!(app.documents[0].text, MERGE_DRAFT);
        assert_eq!(app.documents[0].saved_text, MERGE_BASE);
        assert!(app.disk_review.slot.as_ref().unwrap().merge.is_none());
        app.apply_disk_merge(&app.editor_ctx.clone());
        assert!(rx.try_recv().is_err());
        if case == 0 {
            app.preview_disk_merge(&app.editor_ctx.clone());
            assert!(app.disk_review.slot.as_ref().unwrap().merge.is_some());
            app.apply_disk_merge(&app.editor_ctx.clone());
            reply(
                &mut app,
                rx.try_recv().unwrap(),
                merge_file("file.rs", &MERGE_DISK.replace("disk", "new disk")),
            );
            app.finish_disk_merge(&app.editor_ctx.clone());
            assert_eq!(
                app.documents[0].text,
                MERGE_RESULT.replace("disk", "new disk")
            );
        }
    }
}

#[test]
fn merge_preview_refuses_unknown_baseline_unchanged_disk_ambiguity_touching_and_caps() {
    for case in 0..10 {
        let (mut app, rx) = merge_app();
        let mut disk = MERGE_DISK.to_owned();
        match case {
            0 => app.documents[0].revision = None,
            1 => app.documents[0].revision = Some(revision('a')),
            2 => disk = MERGE_BASE.into(),
            3 => {
                app.documents[0].saved_text = "a\na\ngap\nz\n".into();
                app.documents[0].text = "a\na\na\ngap\nz\n".into();
                disk = "a\na\ngap\nZ\n".into();
                app.documents[0].revision = Some(content_revision(&app.documents[0].saved_text));
            }
            4 => {
                app.documents[0].text = MERGE_BASE.replace("anchor one", "draft anchor");
                disk = MERGE_BASE.replace("anchor two", "disk anchor");
            }
            5 => app.documents[0].text = "x".repeat(MAX_FILE_BYTES + 1),
            6 => app.documents[0].saved_text = "x".repeat(MAX_FILE_BYTES + 1),
            7 => app.documents[0].text = "\n".repeat(65_537),
            8 => disk = "\n".repeat(65_537),
            9 => {
                app.documents[0].text = MERGE_BASE.replace("anchor one", "inserted\nanchor one");
                disk = MERGE_BASE.replace("anchor one", "also inserted\nanchor one");
            }
            _ => unreachable!(),
        }
        merge_review(&mut app, &rx, &disk);
        let before = format!("{:?}", app.documents[0]);
        app.preview_disk_merge(&app.editor_ctx.clone());
        assert!(
            app.disk_review.slot.as_ref().unwrap().merge.is_none(),
            "case {case}"
        );
        assert_eq!(format!("{:?}", app.documents[0]), before);
        assert!(rx.try_recv().is_err());
    }
    assert!(bounded_merge_text(&"\n".repeat(65_536)));
    assert!(!bounded_merge_text(&format!("{}x", "\n".repeat(65_536))));
}

#[test]
fn merge_rechecks_saves_unknown_outcomes_close_and_dialogs_before_preview_and_commit() {
    for barrier in 0..3 {
        for change in 0..11 {
            let (mut app, rx) = merge_app();
            merge_review(&mut app, &rx, MERGE_DISK);
            if barrier > 0 {
                app.preview_disk_merge(&app.editor_ctx.clone());
                app.apply_disk_merge(&app.editor_ctx.clone());
            }
            let read = if barrier > 0 {
                Some(rx.try_recv().unwrap())
            } else {
                None
            };
            if barrier == 2 {
                let read = read.as_ref().unwrap();
                reply(
                    &mut app,
                    Command {
                        id: read.id,
                        op: Operation::Read {
                            path: "file.rs".into(),
                        },
                    },
                    merge_file("file.rs", MERGE_DISK),
                );
            }
            match change {
                0 => app.documents[0].saving = true,
                1 => {
                    app.pending.insert(
                        900,
                        Job::Save {
                            document: 1,
                            snapshot: MERGE_DRAFT.into(),
                            submission: None,
                        },
                    );
                }
                2 => app.close_tab_requested = Some(1),
                3 => app.confirm = Some(crate::Confirm::CloseWindow),
                4 => app.close_after_language_stop = true,
                5 => app.recovery.closing = Some(vec![(1, 1)]),
                6 => app.allow_close = true,
                7 => app.open_form = true,
                8 => app.new_file = true,
                9 => app.interrupted_save_check.outstanding = Some(900),
                10 => {
                    app.documents[0].interrupted_save =
                        crate::interrupted_save::InterruptedSave::capture(&app, &app.documents[0])
                }
                _ => unreachable!(),
            }
            let before = format!("{:?}", app.documents[0]);
            if barrier == 0 {
                app.preview_disk_merge(&app.editor_ctx.clone());
            }
            if barrier == 1 {
                reply(&mut app, read.unwrap(), merge_file("file.rs", MERGE_DISK));
            }
            app.finish_disk_merge(&app.editor_ctx.clone());
            assert_eq!(
                format!("{:?}", app.documents[0]),
                before,
                "barrier {barrier}, change {change}"
            );
            assert!(app.disk_review.slot.as_ref().unwrap().merge.is_none());
            assert!(rx.try_recv().is_err());
        }
    }
}

#[test]
fn stale_merge_verification_dismissal_cancel_navigation_and_generation_never_apply() {
    for change in 0..5 {
        let (mut app, rx) = merge_app();
        merge_preview(&mut app, &rx);
        app.apply_disk_merge(&app.editor_ctx.clone());
        let read = rx.try_recv().unwrap();
        let generation = app.generation;
        match change {
            0 => app.dismiss_disk_review(),
            1 => app.cancel_disk_merge(),
            2 => app.navigation_changed(),
            3 => {
                app.disconnected("connection lost".into());
                app.generation += 1;
            }
            4 => app.documents[0].path = "different.rs".into(),
            _ => unreachable!(),
        }
        app.apply_event(Event {
            generation,
            id: read.id,
            connected: true,
            result: Ok(merge_file("file.rs", MERGE_DISK)),
        });
        app.finish_disk_merge(&app.editor_ctx.clone());
        assert_eq!(app.documents[0].text, MERGE_DRAFT);
        assert_eq!(app.documents[0].saved_text, MERGE_BASE);
        assert!(app
            .disk_review
            .slot
            .as_ref()
            .is_none_or(|review| review.merge.is_none()));
        assert!(rx.try_recv().is_err());
    }
}

#[test]
fn cancelled_merge_keeps_one_wire_read_until_drain_and_late_reply_cannot_revive_it() {
    let (mut app, rx) = merge_app();
    merge_preview(&mut app, &rx);
    app.apply_disk_merge(&app.editor_ctx.clone());
    let read = rx.try_recv().unwrap();
    app.cancel_disk_merge();
    for _ in 0..30 {
        app.preview_disk_merge(&app.editor_ctx.clone());
        app.compare_with_disk();
    }
    assert!(rx.try_recv().is_err());
    assert!(app.disk_review.busy());
    reply(&mut app, read, merge_file("file.rs", MERGE_DISK));
    assert!(!app.disk_review.busy());
    assert!(app.disk_review.slot.as_ref().unwrap().merge.is_none());
    assert_eq!(app.documents[0].text, MERGE_DRAFT);
    app.preview_disk_merge(&app.editor_ctx.clone());
    assert!(app.disk_review.slot.as_ref().unwrap().merge.is_some());
    assert!(rx.try_recv().is_err());
}

const PROFILE_MERGE_BASE: &str = "{\n  \"version\": 1,\n  \"profiles\": [{\n    \"name\": \"Build\",\n    \"program\": \"cargo\",\n    \"args\": [\"build\"],\n    \"timeout_secs\": 30\n  }]\n}\n";

fn profile_merge_app() -> (CedarApp, Receiver<Command>, String, String) {
    let (mut app, rx) = merge_app();
    app.documents[0] = Document::new(
        1,
        crate::profile_ui::PATH.into(),
        PROFILE_MERGE_BASE.into(),
        content_revision(PROFILE_MERGE_BASE),
    );
    app.profiles.connected(app.recovery_workspace().unwrap());
    app.load_profiles();
    app.select_profile(Some(0));
    app.profiles.draft.args.push("form-only".into());
    app.profiles.changed();
    app.documents[0].text = PROFILE_MERGE_BASE.replace("Build", "Merged name");
    app.documents[0].edit_version += 1;
    let disk = PROFILE_MERGE_BASE.replace("30", "60");
    let merged = disk.replace("Build", "Merged name");
    merge_review(&mut app, &rx, &disk);
    app.preview_disk_merge(&app.editor_ctx.clone());
    assert!(app.disk_review.slot.as_ref().unwrap().merge.is_some());
    (app, rx, disk, merged)
}

#[test]
fn profile_merge_retains_form_and_old_source_cancels_queued_actions_and_late_load() {
    for action in [
        crate::profile_ui::Action::Save,
        crate::profile_ui::Action::Run,
        crate::profile_ui::Action::Load,
    ] {
        let (mut app, rx, disk, merged) = profile_merge_app();
        let draft = app.profiles.draft.clone();
        let epoch = app.profiles.epoch;
        app.apply_disk_merge(&app.editor_ctx.clone());
        reply(
            &mut app,
            rx.try_recv().unwrap(),
            merge_file(crate::profile_ui::PATH, &disk),
        );
        app.queue_profile_action(action);
        app.finish_disk_merge(&app.editor_ctx.clone());
        app.finish_profile_actions();
        assert_eq!(app.documents[0].text, merged);
        assert_eq!(app.documents[0].saved_text, disk);
        assert!(app.documents[0].dirty());
        assert_eq!(app.profiles.draft, draft);
        assert!(app.profiles.dirty());
        assert!(app.profiles.owns_document(1));
        assert_eq!(app.profiles.epoch, epoch + 1);
        assert!(app.profiles.message.as_ref().unwrap().contains("merged"));
        assert!(app.profile_run_problem().is_some());
        app.apply_profile_load(
            epoch,
            Some((
                PROFILE_MERGE_BASE.into(),
                content_revision(PROFILE_MERGE_BASE),
            )),
        );
        assert_eq!(app.profiles.draft, draft);
        assert_eq!(app.documents[0].text, merged);
        app.save_profile();
        assert_eq!(app.documents[0].text, merged);
        assert!(rx.try_recv().is_err());
        app.discard_profile_form();
        assert_eq!(
            app.profiles.draft.name, "Build",
            "the old form baseline is retained"
        );
        assert_eq!(app.profiles.draft.args, vec!["build"]);
    }
}

#[test]
fn profile_form_change_during_verification_or_final_input_cancels_merge() {
    for final_frame in [false, true] {
        let (mut app, rx, disk, _) = profile_merge_app();
        app.apply_disk_merge(&app.editor_ctx.clone());
        let read = rx.try_recv().unwrap();
        if final_frame {
            reply(
                &mut app,
                Command {
                    id: read.id,
                    op: Operation::Read {
                        path: crate::profile_ui::PATH.into(),
                    },
                },
                merge_file(crate::profile_ui::PATH, &disk),
            );
        }
        app.profiles.draft.name = "new form input".into();
        app.profiles.changed();
        if !final_frame {
            reply(&mut app, read, merge_file(crate::profile_ui::PATH, &disk));
        }
        app.finish_disk_merge(&app.editor_ctx.clone());
        assert_eq!(app.documents[0].saved_text, PROFILE_MERGE_BASE);
        assert_eq!(
            app.documents[0].text,
            PROFILE_MERGE_BASE.replace("Build", "Merged name")
        );
        assert_eq!(app.profiles.draft.name, "new form input");
        assert!(app.disk_review.slot.as_ref().unwrap().merge.is_none());
        assert!(rx.try_recv().is_err());
    }
}

fn wait_merge_recovery(app: &mut CedarApp, done: impl Fn(&CedarApp) -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        app.recovery.poll();
        if done(app) {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "recovery timed out: {:?}",
            app.recovery.error
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[test]
fn merge_recovery_preserves_older_unowned_records_and_persists_owned_new_tuple() {
    use cedar_recovery::{record_id, Draft, Store};
    for older_unowned in [false, true] {
        let (mut app, rx) = merge_app();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("private-recovery");
        let workspace = app.recovery_workspace().unwrap();
        let id = record_id(&workspace, "file.rs").unwrap();
        if older_unowned {
            Store::open(&path)
                .unwrap()
                .write(
                    1,
                    &Draft {
                        workspace: workspace.clone(),
                        path: "file.rs".into(),
                        text: "older private draft".into(),
                        base_text: "older base".into(),
                        base_revision: Some("older revision".into()),
                        modified_ms: 1,
                    },
                )
                .unwrap();
        }
        app.recovery.start(Ok(path.clone()), &app.editor_ctx);
        wait_merge_recovery(&mut app, |app| app.recovery.initialized);
        app.recovery.observe(&workspace, &app.documents[0]);
        app.recovery.flush();
        if !older_unowned {
            wait_merge_recovery(&mut app, |app| {
                app.recovery.protected(&workspace, &app.documents[0])
            });
        }
        merge_preview(&mut app, &rx);
        assert_eq!(
            app.recovery.protected(&workspace, &app.documents[0]),
            !older_unowned
        );
        app.apply_disk_merge(&app.editor_ctx.clone());
        reply(
            &mut app,
            rx.try_recv().unwrap(),
            merge_file("file.rs", MERGE_DISK),
        );
        app.finish_disk_merge(&app.editor_ctx.clone());
        assert!(
            !app.recovery.protected(&workspace, &app.documents[0]),
            "old acknowledgement cannot certify new edit/base tuple"
        );
        app.recovery.poll();
        assert!(!app.recovery.protected(&workspace, &app.documents[0]));
        app.recovery.observe(&workspace, &app.documents[0]);
        app.recovery.flush();
        if older_unowned {
            app.recovery.discard_owned(&workspace, &app.documents[0]);
        } else {
            wait_merge_recovery(&mut app, |app| {
                app.recovery.protected(&workspace, &app.documents[0])
            });
        }
        assert!(rx.try_recv().is_err());
        drop(app);
        let persisted = Store::open(path).unwrap().read(&id).unwrap();
        if older_unowned {
            assert_eq!(persisted.text, "older private draft");
            assert_eq!(persisted.base_text, "older base");
            assert_eq!(persisted.base_revision.as_deref(), Some("older revision"));
        } else {
            assert_eq!(persisted.text, MERGE_RESULT);
            assert_eq!(persisted.base_text, MERGE_DISK);
            assert_eq!(persisted.base_revision, Some(content_revision(MERGE_DISK)));
        }
    }
}

#[test]
fn current_merge_transport_loss_after_cancel_disconnects_without_applying_or_replaying() {
    let (mut app, rx) = merge_app();
    merge_preview(&mut app, &rx);
    app.apply_disk_merge(&app.editor_ctx.clone());
    let read = rx.try_recv().unwrap();
    app.cancel_disk_merge();
    app.apply_event(Event {
        generation: app.generation,
        id: read.id,
        connected: false,
        result: Err("transport ended".into()),
    });
    assert!(!app.ready());
    assert!(app.disk_review.slot.is_none());
    assert_eq!(app.documents[0].text, MERGE_DRAFT);
    assert_eq!(app.documents[0].saved_text, MERGE_BASE);
    assert!(!app.disk_review.busy());
    assert!(rx.try_recv().is_err());
}
