use super::*;
use crate::{
    editor_state,
    worker::{Command, Event, Worker},
    ConnectForm, Job, Operation, Payload,
};
use std::sync::mpsc::{self, Receiver};

fn app() -> (CedarApp, Receiver<Command>) {
    let mut app = CedarApp::empty();
    let form = ConnectForm {
        local_root: "/copy-draft-tests".into(),
        ..Default::default()
    };
    app.workspace_key = Some(form.key());
    app.active_form = Some(form);
    app.root = "/copy-draft-tests".into();
    app.state = ConnectionState::Ready;
    app.generation = 7;
    app.open_form = false;
    app.agent_info = Some(crate::agent_support::full_test_agent());
    let mut doc = Document::new(
        1,
        "source.rs".into(),
        "saved\r\n".into(),
        revision("saved\r\n"),
    );
    editor_state::commit(&app.editor_ctx, &mut doc, "draft é 🐻\r\n".into(), 3);
    app.documents.push(doc);
    app.active_document = Some(1);
    app.next_document = 2;
    let (worker, commands) = Worker::recording();
    app.worker = Some(worker);
    (app, commands)
}

fn revision(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

fn idle(commands: &Receiver<Command>) {
    assert!(matches!(
        commands.try_recv(),
        Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected)
    ));
}

fn queue(app: &mut CedarApp, path: &str) {
    app.begin_copy_draft();
    assert!(app.copy_draft_open());
    app.copy_draft_set_path(path);
    app.confirm_copy_draft();
}

fn finish(app: &mut CedarApp) {
    app.finish_copy_draft_frame(&app.editor_ctx.clone());
}

fn frame(app: &mut CedarApp, time: f64, events: Vec<egui::Event>) -> egui::FullOutput {
    let ctx = app.editor_ctx.clone();
    let mut native = eframe::Frame::_new_kittest();
    ctx.run(
        egui::RawInput {
            time: Some(time),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(780.0, 540.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| eframe::App::update(app, ctx, &mut native),
    )
}

fn key(key: egui::Key, modifiers: egui::Modifiers) -> Vec<egui::Event> {
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

fn selection(app: &CedarApp, id: u64) -> egui::text::CCursorRange {
    egui::TextEdit::load_state(&app.editor_ctx, egui::Id::new(("editor", id)))
        .unwrap()
        .cursor
        .char_range()
        .unwrap()
}

fn assert_selection(actual: egui::text::CCursorRange, expected: egui::text::CCursorRange) {
    assert_eq!(actual, expected);
    assert_eq!(
        actual.primary.prefer_next_row,
        expected.primary.prefer_next_row
    );
    assert_eq!(
        actual.secondary.prefer_next_row,
        expected.secondary.prefer_next_row
    );
}

#[test]
fn copy_creates_only_fresh_unsaved_document_and_preserves_source_history_and_selection() {
    let (mut app, commands) = app();
    let source = &app.documents[0];
    let source_before = format!("{source:?}");
    let mut state =
        egui::TextEdit::load_state(&app.editor_ctx, egui::Id::new(("editor", 1u64))).unwrap();
    let range = egui::text::CCursorRange {
        primary: egui::text::CCursor {
            index: 8,
            prefer_next_row: true,
        },
        secondary: egui::text::CCursor {
            index: 2,
            prefer_next_row: false,
        },
    };
    state.cursor.set_char_range(Some(range));
    state.store(&app.editor_ctx, egui::Id::new(("editor", 1u64)));
    let before_undo = egui::TextEdit::load_state(&app.editor_ctx, egui::Id::new(("editor", 1u64)))
        .unwrap()
        .undoer()
        .undo(&(range, app.documents[0].text.clone()))
        .cloned();
    queue(&mut app, "nested/拷贝.rs");
    assert_eq!(app.documents.len(), 1, "confirmation is only an intent");
    finish(&mut app);
    assert!(!app.copy_draft_open());
    assert_eq!(app.documents.len(), 2);
    assert_eq!(format!("{:?}", app.documents[0]), source_before);
    assert_selection(selection(&app, 1), range);
    let after_undo = egui::TextEdit::load_state(&app.editor_ctx, egui::Id::new(("editor", 1u64)))
        .unwrap()
        .undoer()
        .undo(&(range, app.documents[0].text.clone()))
        .cloned();
    assert_eq!(before_undo, after_undo);
    let new = &app.documents[1];
    assert_eq!(new.id, 2);
    assert_eq!(app.active_document, Some(2));
    assert_eq!(new.path, "nested/拷贝.rs");
    assert_eq!(new.text, app.documents[0].text);
    assert!(new.saved_text.is_empty());
    assert!(new.revision.is_none() && new.dirty() && !new.saving && !new.undo_initialized);
    assert!(!new.save_outcome_unknown());
    assert_eq!(new.edit_version, 0);
    assert!(
        egui::TextEdit::load_state(&app.editor_ctx, egui::Id::new(("editor", new.id))).is_none()
    );
    assert!(app.pending.is_empty());
    idle(&commands);
    app.save();
    let command = commands.try_recv().unwrap();
    assert!(
        matches!(command.op, Operation::Write { path, expected_revision: None, .. } if path == "nested/拷贝.rs")
    );
    idle(&commands);
}

#[test]
fn unknown_source_keeps_token_and_baseline_and_continues_blocking_save_all() {
    for unverifiable in [false, true] {
        let (mut app, commands) = app();
        if unverifiable {
            app.documents[0].save_outcome_unverifiable = true;
        } else {
            app.documents[0].interrupted_save =
                crate::interrupted_save::InterruptedSave::capture(&app, &app.documents[0]);
        }
        let source = format!("{:?}", app.documents[0]);
        queue(&mut app, "copy.rs");
        finish(&mut app);
        assert_eq!(app.documents.len(), 2);
        assert_eq!(format!("{:?}", app.documents[0]), source);
        assert!(!app.documents[1].save_outcome_unknown());
        app.queue_save_all();
        assert!(!app.save_all_busy());
        assert!(app.error.as_deref().unwrap().contains("unknown"));
        let output = frame(&mut app, 0.0, vec![]);
        assert!(output
            .shapes
            .iter()
            .any(|shape| shape_text_contains(&shape.shape, "Save outcome unknown: source.rs")));
        idle(&commands);
    }
}

fn shape_text_contains(shape: &egui::epaint::Shape, needle: &str) -> bool {
    match shape {
        egui::epaint::Shape::Text(text) => text.galley.job.text.contains(needle),
        egui::epaint::Shape::Vec(shapes) => shapes
            .iter()
            .any(|shape| shape_text_contains(shape, needle)),
        _ => false,
    }
}

#[test]
fn literal_portable_validation_is_shared_with_new_file_and_keeps_rejected_input() {
    let mut invalid: Vec<String> = [
        "",
        "/absolute",
        "a//b",
        "a/./b",
        "a/../b",
        "C:/file",
        "a\\b",
        "file.",
        "file ",
        "a\0b",
        "a\nb",
        "a<b",
        "a>b",
        "a\"b",
        "a|b",
        "a?b",
        "a*b",
        "CON",
        "con.txt",
        "NUL",
        "COM1.rs",
        "lpt9",
        "COM¹",
        "CONIN$",
        "CONOUT$",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    invalid.push("x".repeat(256));
    invalid.push(format!("{}/x", "a/".repeat(2048)));
    for path in invalid {
        let (mut app, commands) = app();
        let source = format!("{:?}", app.documents[0]);
        queue(&mut app, &path);
        finish(&mut app);
        assert_eq!(app.documents.len(), 1, "{path:?}");
        assert!(app.copy_draft_error().is_some(), "{path:?}");
        assert_eq!(app.copy_draft_path(), Some(path.as_str()));
        assert_eq!(format!("{:?}", app.documents[0]), source);
        assert_eq!(app.next_document, 2);
        app.dismiss_copy_draft();
        app.new_file = true;
        app.new_path = path.clone();
        app.create_new_file_draft();
        assert!(app.new_file);
        assert_eq!(app.new_path, path);
        assert_eq!(app.documents.len(), 1);
        assert!(app.error.is_some());
        idle(&commands);
    }
    for path in [" folder/é.txt", "folder/e\u{301}.txt", "Case/File.rs"] {
        let (mut app, commands) = app();
        queue(&mut app, path);
        finish(&mut app);
        assert_eq!(app.documents[1].path, path);
        idle(&commands);
    }
}

#[test]
fn source_size_bound_is_utf8_bytes_and_empty_copy_is_still_unsaved() {
    for bytes in [0, MAX_FILE_BYTES, MAX_FILE_BYTES + 1] {
        let (mut app, commands) = app();
        app.documents[0].text = "é".repeat(bytes / 2);
        if bytes % 2 != 0 {
            app.documents[0].text.push('x');
        }
        app.begin_copy_draft();
        if bytes > MAX_FILE_BYTES {
            assert!(!app.copy_draft_open());
            assert!(app.error.as_deref().unwrap().contains("1 MiB"));
        } else {
            app.copy_draft_set_path("copy.rs");
            app.confirm_copy_draft();
            finish(&mut app);
            assert_eq!(app.documents[1].text.len(), bytes);
            assert!(app.documents[1].dirty());
            assert!(app.documents[1].saved_text.is_empty());
            assert_eq!(app.documents[1].revision, None);
        }
        idle(&commands);
    }
}

#[test]
fn changed_source_or_session_never_installs_a_partial_copy() {
    for change in 0..13 {
        let (mut app, commands) = app();
        queue(&mut app, "copy.rs");
        match change {
            0 => app.documents[0].text.push('!'),
            1 => app.documents[0].edit_version += 1,
            2 => app.documents[0].path = "renamed.rs".into(),
            3 => app.generation += 1,
            4 => app.navigation_epoch += 1,
            5 => app.root = "/another".into(),
            6 => app.active_document = None,
            7 => app.documents[0].id = 9,
            8 => app.state = ConnectionState::Disconnected,
            9 => app.documents[0].saved_text.push('!'),
            10 => app.documents[0].revision = None,
            11 => app.documents[0].save_outcome_unverifiable = true,
            12 => app.workspace_key = None,
            _ => unreachable!(),
        }
        let source = format!("{:?}", app.documents[0]);
        let active = app.active_document;
        finish(&mut app);
        assert_eq!(app.documents.len(), 1, "change {change}");
        assert_eq!(app.next_document, 2);
        assert_eq!(app.active_document, active);
        assert_eq!(format!("{:?}", app.documents[0]), source);
        assert!(app.copy_draft_error().unwrap().contains("changed"));
        assert_eq!(app.copy_draft_path(), Some("copy.rs"));
        idle(&commands);
    }
}

#[test]
fn stable_disconnected_draft_can_be_copied_but_reconnect_invalidates_snapshot() {
    let (mut app, commands) = app();
    app.state = ConnectionState::Disconnected;
    queue(&mut app, "offline.rs");
    finish(&mut app);
    assert_eq!(app.documents.len(), 2);
    queue(&mut app, "stale.rs");
    app.generation += 1;
    app.state = ConnectionState::Ready;
    finish(&mut app);
    assert_eq!(app.documents.len(), 2);
    assert!(app.copy_draft_error().is_some());
    idle(&commands);
}

#[test]
fn destination_open_race_capacity_and_id_exhaustion_keep_dialog_source_and_counter() {
    for refusal in 0..5 {
        let (mut app, commands) = app();
        queue(&mut app, "copy.rs");
        match refusal {
            0 => app.documents.push(Document::new(
                3,
                "copy.rs".into(),
                "occupied".into(),
                revision("occupied"),
            )),
            1 => {
                for id in 2..=32 {
                    app.documents.push(Document::new(
                        id,
                        format!("file-{id}"),
                        String::new(),
                        String::new(),
                    ));
                }
            }
            2 => app.next_document = u64::MAX,
            3 => app.next_document = 0,
            4 => app.next_document = 1,
            _ => unreachable!(),
        }
        let before: Vec<_> = app.documents.iter().map(|doc| format!("{doc:?}")).collect();
        let next = app.next_document;
        finish(&mut app);
        assert_eq!(
            app.documents
                .iter()
                .map(|doc| format!("{doc:?}"))
                .collect::<Vec<_>>(),
            before
        );
        assert_eq!(app.next_document, next);
        assert_eq!(app.active_document, Some(1));
        assert!(app.copy_draft_error().is_some());
        assert_eq!(app.copy_draft_path(), Some("copy.rs"));
        idle(&commands);
    }
}

#[test]
fn listed_recovery_blocks_copy_without_adoption_and_unavailable_store_allows_editing() {
    let (mut app, commands) = app();
    let workspace = app.recovery_workspace().unwrap();
    let id = cedar_recovery::record_id(&workspace, "copy.rs").unwrap();
    app.recovery.drafts.push(cedar_recovery::DraftMetadata {
        id,
        workspace,
        path: "copy.rs".into(),
        base_revision: None,
        modified_ms: 1,
        text_bytes: 5,
        base_text_bytes: 0,
    });
    let records = app.recovery.drafts.clone();
    queue(&mut app, "copy.rs");
    finish(&mut app);
    assert!(app.copy_draft_error().unwrap().contains("recovery"));
    assert_eq!(app.recovery.drafts, records);
    assert_eq!(app.documents.len(), 1);
    app.copy_draft_set_path("different.rs");
    app.recovery.enabled = true;
    app.recovery.initialized = false;
    app.recovery.error = Some("Storage unavailable".into());
    app.confirm_copy_draft();
    finish(&mut app);
    assert_eq!(app.documents.len(), 2);
    assert_eq!(app.recovery.error.as_deref(), Some("Storage unavailable"));
    assert_eq!(app.recovery.drafts, records);
    assert_eq!(
        app.recovery
            .status(app.recovery_workspace().as_ref(), app.active()),
        ("Recovery needs attention", false)
    );
    idle(&commands);
}

#[test]
fn new_file_also_keeps_occupied_and_recovery_destinations_open_for_correction() {
    let (mut app, commands) = app();
    app.new_file = true;
    app.new_path = "source.rs".into();
    app.create_new_file_draft();
    assert!(app.new_file);
    assert_eq!(app.new_path, "source.rs");
    assert!(app.error.as_deref().unwrap().contains("open tab"));
    let workspace = app.recovery_workspace().unwrap();
    app.recovery.drafts.push(cedar_recovery::DraftMetadata {
        id: cedar_recovery::record_id(&workspace, "recover.rs").unwrap(),
        workspace,
        path: "recover.rs".into(),
        base_revision: None,
        modified_ms: 1,
        text_bytes: 5,
        base_text_bytes: 0,
    });
    app.new_path = "recover.rs".into();
    app.create_new_file_draft();
    assert!(app.new_file);
    assert_eq!(app.new_path, "recover.rs");
    assert!(app.error.as_deref().unwrap().contains("recovery"));
    assert_eq!(app.documents.len(), 1);
    assert_eq!(app.active_document, Some(1));
    assert_eq!(app.next_document, 2);
    idle(&commands);
}

#[test]
fn checked_allocator_is_shared_by_open_restore_profile_and_new_file() {
    for next in [0, 1, u64::MAX] {
        for caller in 0..4 {
            let (mut app, commands) = app();
            app.next_document = next;
            let source = format!("{:?}", app.documents[0]);
            match caller {
                0 => {
                    app.pending.insert(
                        99,
                        Job::Open {
                            path: "other.rs".into(),
                            line: None,
                            navigation: app.navigation_epoch,
                        },
                    );
                    app.apply_event(Event {
                        generation: app.generation,
                        id: 99,
                        connected: true,
                        result: Ok(Payload::File {
                            path: "other.rs".into(),
                            text: "disk".into(),
                            revision: revision("disk"),
                        }),
                    });
                    assert!(app.error.is_some());
                }
                1 => {
                    let draft = cedar_recovery::Draft {
                        workspace: app.recovery_workspace().unwrap(),
                        path: "other.rs".into(),
                        text: "recovered".into(),
                        base_text: "base".into(),
                        base_revision: Some(revision("base")),
                        modified_ms: 1,
                    };
                    assert!(app.install_recovered(draft).is_err());
                }
                2 => {
                    app.apply_profile_load(app.profiles.epoch, None);
                    assert!(app
                        .profiles
                        .message
                        .as_ref()
                        .is_some_and(|message| message.contains("identit")));
                }
                3 => {
                    app.new_file = true;
                    app.new_path = "other.rs".into();
                    app.create_new_file_draft();
                    assert!(app.new_file && app.error.is_some());
                }
                _ => unreachable!(),
            }
            assert_eq!(app.documents.len(), 1, "caller {caller}, next {next}");
            assert_eq!(app.active_document, Some(1));
            assert_eq!(app.next_document, next);
            assert_eq!(format!("{:?}", app.documents[0]), source);
            idle(&commands);
        }
    }
}

#[test]
fn modal_owns_save_find_close_navigation_undo_and_enter_indentation_shortcuts() {
    for (key_value, modifiers) in [
        (egui::Key::S, egui::Modifiers::COMMAND),
        (
            egui::Key::S,
            egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
        ),
        (egui::Key::W, egui::Modifiers::COMMAND),
        (egui::Key::F, egui::Modifiers::COMMAND),
        (egui::Key::P, egui::Modifiers::COMMAND),
        (egui::Key::G, egui::Modifiers::COMMAND),
        (egui::Key::J, egui::Modifiers::COMMAND),
        (
            egui::Key::E,
            egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
        ),
        (egui::Key::Z, egui::Modifiers::COMMAND),
        (egui::Key::Tab, egui::Modifiers::NONE),
        (egui::Key::Enter, egui::Modifiers::NONE),
    ] {
        let (mut app, commands) = app();
        frame(&mut app, 0.0, vec![]);
        let source = format!("{:?}", app.documents[0]);
        let source_selection = selection(&app, 1);
        app.begin_copy_draft();
        frame(&mut app, 1.0, vec![]);
        frame(&mut app, 1.1, vec![]);
        frame(&mut app, 2.0, key(key_value, modifiers));
        assert_eq!(format!("{:?}", app.documents[0]), source, "{key_value:?}");
        assert_selection(selection(&app, 1), source_selection);
        assert_eq!(app.documents.len(), 1);
        assert!(!app.save_all_busy() && !app.find_open && !app.navigation.dialog_open());
        assert!(app.close_tab_requested.is_none());
        assert!(!app.tools_open);
        idle(&commands);
    }
}

#[test]
fn canceled_modal_and_repeated_confirm_never_duplicate_or_leak_typing() {
    let (mut app, commands) = app();
    frame(&mut app, 0.0, vec![]);
    let source = app.documents[0].text.clone();
    app.begin_copy_draft();
    frame(&mut app, 1.0, vec![]);
    frame(&mut app, 1.1, vec![]);
    let mut events = key(egui::Key::Escape, egui::Modifiers::NONE);
    events.push(egui::Event::Text("must not reach editor".into()));
    frame(&mut app, 2.0, events);
    assert!(!app.copy_draft_open());
    assert_eq!(app.documents[0].text, source);
    queue(&mut app, "copy.rs");
    app.confirm_copy_draft();
    finish(&mut app);
    app.confirm_copy_draft();
    finish(&mut app);
    assert_eq!(app.documents.len(), 2);
    idle(&commands);
}

#[test]
fn same_frame_profile_serialization_precedes_copy_final_guard() {
    let (mut app, commands) = app();
    app.documents[0] = Document::new(
        1,
        crate::profile_ui::PATH.into(),
        "{\"version\":1,\"profiles\":[]}".into(),
        revision("profiles"),
    );
    app.load_profiles();
    app.new_profile();
    app.profiles.draft.name = "test".into();
    app.profiles.draft.program = "true".into();
    queue(&mut app, "profiles-copy.json");
    app.queue_profile_action(crate::profile_ui::Action::Save);
    frame(&mut app, 1.0, vec![]);
    assert_eq!(app.documents.len(), 1);
    assert!(app.documents[0].text.contains("true"));
    assert!(app.copy_draft_error().unwrap().contains("changed"));
    // The separately queued, explicit profile save retains its normal action;
    // copying did not add any operation of its own.
    assert!(
        matches!(commands.try_recv().unwrap().op, Operation::Write { path, .. } if path == crate::profile_ui::PATH)
    );
    idle(&commands);
}

#[test]
fn same_frame_reconciliation_rejects_captured_copy_even_when_text_is_unchanged() {
    let (mut app, commands) = app();
    app.documents[0].interrupted_save =
        crate::interrupted_save::InterruptedSave::capture(&app, &app.documents[0]);
    app.check_interrupted_save();
    for _ in 0..2 {
        let command = commands.try_recv().unwrap();
        assert!(matches!(command.op, Operation::Read { .. }));
        let text = app.documents[0].text.clone();
        app.apply_event(Event {
            generation: app.generation,
            id: command.id,
            connected: true,
            result: Ok(Payload::File {
                path: "source.rs".into(),
                revision: revision(&text),
                text,
            }),
        });
    }
    assert!(app.documents[0].save_outcome_unknown());
    queue(&mut app, "copy.rs");
    frame(&mut app, 1.0, vec![]);
    assert!(!app.documents[0].save_outcome_unknown());
    assert_eq!(app.documents[0].saved_text, app.documents[0].text);
    assert_eq!(app.documents.len(), 1);
    assert!(app.copy_draft_error().unwrap().contains("changed"));
    idle(&commands);
}

#[test]
fn checked_allocator_keeps_successful_open_restore_profile_and_new_file_semantics() {
    for caller in 0..4 {
        let (mut app, commands) = app();
        let source = format!("{:?}", app.documents[0]);
        match caller {
            0 => {
                app.pending.insert(
                    99,
                    Job::Open {
                        path: "other.rs".into(),
                        line: Some(2),
                        navigation: app.navigation_epoch,
                    },
                );
                app.apply_event(Event {
                    generation: app.generation,
                    id: 99,
                    connected: true,
                    result: Ok(Payload::File {
                        path: "other.rs".into(),
                        text: "disk\nsecond".into(),
                        revision: revision("disk\nsecond"),
                    }),
                });
                assert_eq!(app.active_document, Some(2));
                assert_eq!(app.documents[1].jump_to, Some(5));
                assert!(!app.documents[1].dirty());
            }
            1 => {
                app.install_recovered(cedar_recovery::Draft {
                    workspace: app.recovery_workspace().unwrap(),
                    path: "other.rs".into(),
                    text: "recovered".into(),
                    base_text: "base".into(),
                    base_revision: Some(revision("base")),
                    modified_ms: 1,
                })
                .unwrap();
                assert_eq!(app.active_document, Some(2));
                assert_eq!(app.documents[1].text, "recovered");
                assert_eq!(app.documents[1].saved_text, "base");
                assert_eq!(
                    app.documents[1].revision.as_deref(),
                    Some(revision("base").as_str())
                );
            }
            2 => {
                app.apply_profile_load(app.profiles.epoch, None);
                assert_eq!(app.active_document, Some(1));
                assert_eq!(app.documents[1].path, crate::profile_ui::PATH);
                assert!(app.documents[1].revision.is_none());
                assert_eq!(app.documents[1].text, "{\"version\":1,\"profiles\":[]}");
            }
            3 => {
                app.new_file = true;
                app.new_path = "other.rs".into();
                app.create_new_file_draft();
                assert_eq!(app.active_document, Some(2));
                assert!(!app.new_file);
                assert!(app.documents[1].text.is_empty());
                assert!(app.documents[1].saved_text.is_empty());
                assert!(app.documents[1].revision.is_none());
            }
            _ => unreachable!(),
        }
        assert_eq!(app.documents.len(), 2);
        assert_eq!(app.documents[1].id, 2);
        assert_eq!(app.next_document, 3);
        assert_eq!(format!("{:?}", app.documents[0]), source);
        idle(&commands);
    }
}

fn click_recorded(app: &mut CedarApp, time: f64, name: &str) {
    let pos = crate::workspace_access_tests::recorded_rect(app, name).center();
    frame(app, time, vec![egui::Event::PointerMoved(pos)]);
    for (index, pressed) in [true, false].into_iter().enumerate() {
        frame(
            app,
            time + 0.1 + index as f64 * 0.1,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            }],
        );
    }
}

#[test]
fn native_modal_copies_literal_input_with_independent_undo_and_preserved_source_selection() {
    let (mut app, commands) = app();
    frame(&mut app, 0.0, vec![]);
    let range = egui::text::CCursorRange {
        primary: egui::text::CCursor {
            index: 8,
            prefer_next_row: true,
        },
        secondary: egui::text::CCursor {
            index: 2,
            prefer_next_row: false,
        },
    };
    editor_state::move_selection(&app.editor_ctx, &mut app.documents[0], range);
    let original = app.documents[0].text.clone();
    app.begin_copy_draft();
    frame(&mut app, 1.0, vec![]);
    frame(&mut app, 1.1, vec![]);
    click_recorded(&mut app, 2.0, "copy_draft_path");
    frame(
        &mut app,
        3.0,
        vec![egui::Event::Text(" literal/é.rs".into())],
    );
    assert_eq!(app.copy_draft_path(), Some(" literal/é.rs"));
    assert_eq!(app.documents[0].text, original);
    assert_selection(selection(&app, 1), range);
    click_recorded(&mut app, 4.0, "copy_draft_confirm");
    assert!(!app.copy_draft_open());
    assert_eq!(app.documents[1].path, " literal/é.rs");
    assert_eq!(app.documents[1].text, original);
    assert_selection(selection(&app, 1), range);
    for time in [5.0, 5.1, 5.2] {
        frame(&mut app, time, vec![]);
    }
    frame(&mut app, 6.0, key(egui::Key::Z, egui::Modifiers::COMMAND));
    assert_eq!(
        app.documents[1].text, original,
        "copied text is the initial Undo state"
    );
    frame(&mut app, 7.0, vec![egui::Event::Text("!".into())]);
    assert_ne!(app.documents[1].text, original);
    frame(&mut app, 8.0, key(egui::Key::Z, egui::Modifiers::COMMAND));
    assert_eq!(app.documents[1].text, original);
    assert_eq!(app.documents[0].text, original);
    assert_selection(selection(&app, 1), range);
    idle(&commands);
}

#[test]
fn allocator_uses_last_representable_successor_then_refuses_without_wrapping() {
    let (mut app, commands) = app();
    app.next_document = u64::MAX - 1;
    queue(&mut app, "last.rs");
    finish(&mut app);
    assert_eq!(app.documents[1].id, u64::MAX - 1);
    assert_eq!(app.next_document, u64::MAX);
    queue(&mut app, "exhausted.rs");
    finish(&mut app);
    assert_eq!(app.documents.len(), 2);
    assert_eq!(app.next_document, u64::MAX);
    assert_eq!(app.active_document, Some(u64::MAX - 1));
    assert!(app.copy_draft_error().unwrap().contains("exhausted"));
    idle(&commands);
}

#[test]
fn late_async_open_preserves_modal_source_and_invalidates_copy_before_finishing() {
    for path in ["source.rs", "copy.rs"] {
        for exhausted in [false, true] {
            let (mut app, commands) = app();
            frame(&mut app, 0.0, vec![]);
            let source = format!("{:?}", app.documents[0]);
            let source_selection = selection(&app, 1);
            app.pending.insert(
                99,
                Job::Open {
                    path: path.into(),
                    line: Some(2),
                    navigation: app.navigation_epoch,
                },
            );
            queue(&mut app, "copy.rs");
            if exhausted {
                app.next_document = u64::MAX;
            }
            app.result_tx
                .send(crate::worker::WorkerEvent::Response(Event {
                    generation: app.generation,
                    id: 99,
                    connected: true,
                    result: Ok(Payload::File {
                        path: path.into(),
                        text: "received\nsecond".into(),
                        revision: revision("received\nsecond"),
                    }),
                }))
                .unwrap();
            frame(&mut app, 1.0, vec![]);
            assert_eq!(format!("{:?}", app.documents[0]), source);
            assert_selection(selection(&app, 1), source_selection);
            assert_eq!(app.active_document, Some(1));
            assert_eq!(app.copy_draft_path(), Some("copy.rs"));
            assert!(app.copy_draft_error().unwrap().contains("changed"));
            if path == "copy.rs" && !exhausted {
                assert_eq!(app.documents.len(), 2);
                assert_eq!(app.documents[1].text, "received\nsecond");
                assert!(!app.documents[1].dirty());
                assert_eq!(app.next_document, 3);
            } else {
                assert_eq!(app.documents.len(), 1);
                assert_eq!(app.next_document, if exhausted { u64::MAX } else { 2 });
            }
            idle(&commands);
        }
    }
}

#[test]
fn non_ascii_destination_participates_in_existing_cjk_font_probe() {
    let (mut app, commands) = app();
    app.begin_copy_draft();
    app.copy_draft_set_path("src/你好.rs");
    assert!(app.copy_draft.has_cjk());
    app.copy_draft_set_path("src/ascii.rs");
    assert!(!app.copy_draft.has_cjk());
    idle(&commands);
}
