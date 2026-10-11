//! Pure recording-worker and real egui-frame coverage of one-tab close consent.
//! These tests do not start an agent, execute commands, or touch workspace files.
use super::*;
use sha2::{Digest, Sha256};

const BASE: &str = "original generated text\n";
const DRAFT: &str = "submitted generated text é 🐻\r\n";
const NEWER: &str = "newer generated text Ω\n";

fn revision(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

fn connected() -> (CedarApp, Receiver<Command>) {
    let mut app = CedarApp::empty();
    let form = ConnectForm {
        local_root: "/synthetic-save-close".into(),
        allow_run: false,
        ..Default::default()
    };
    app.workspace_key = Some(form.key());
    app.active_form = Some(form);
    app.root = "/synthetic-save-close".into();
    app.state = ConnectionState::Ready;
    app.generation = 7;
    app.open_form = false;
    app.explorer.mode = explorer_tree::Mode::Tree;
    app.documents.push(Document::new(
        1,
        "target.txt".into(),
        BASE.into(),
        revision(BASE),
    ));
    app.active_document = Some(1);
    app.next_document = 2;
    editor_state::commit(&app.editor_ctx, &mut app.documents[0], DRAFT.into(), 4);
    let (worker, commands) = Worker::recording();
    app.worker = Some(worker);
    (app, commands)
}

fn finish(app: &mut CedarApp) {
    app.finish_save_close_tab_frame(&app.editor_ctx.clone());
}

fn idle(commands: &Receiver<Command>) {
    assert!(matches!(
        commands.try_recv(),
        Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected)
    ));
}

fn submit(app: &mut CedarApp, commands: &Receiver<Command>) -> Command {
    app.close_tab(1);
    assert!(matches!(app.confirm, Some(Confirm::CloseTab(1))));
    app.queue_save_close_tab();
    idle(commands);
    finish(app);
    let command = commands.try_recv().expect("one conditional Write");
    let doc = app.documents.iter().find(|doc| doc.id == 1).unwrap();
    assert!(
        matches!(&command.op, Operation::Write { path, text, expected_revision }
        if *path == doc.path && *text == doc.text && *expected_revision == doc.revision)
    );
    assert!(app.save_close_write_pending());
    assert!(doc.saving);
    idle(commands);
    command
}

fn written(app: &CedarApp, command: &Command) -> Event {
    let Operation::Write { text, .. } = &command.op else {
        panic!("Write");
    };
    Event {
        generation: app.generation,
        id: command.id,
        connected: true,
        result: Ok(Payload::Written {
            revision: revision(text),
        }),
    }
}

fn acknowledge(app: &mut CedarApp, command: &Command) {
    app.apply_event(written(app, command));
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

fn selection(app: &CedarApp, document: u64) -> Option<egui::text::CCursorRange> {
    egui::TextEdit::load_state(&app.editor_ctx, egui::Id::new(("editor", document)))
        .and_then(|state| state.cursor.char_range())
}

#[test]
fn exact_ack_closes_only_captured_existing_or_new_inactive_tab_at_final_frame() {
    for new_file in [false, true] {
        let (mut app, commands) = connected();
        if new_file {
            app.documents[0].revision = None;
            app.documents[0].saved_text.clear();
        }
        app.documents.push(Document::new(
            2,
            "other.txt".into(),
            BASE.into(),
            revision(BASE),
        ));
        editor_state::commit(&app.editor_ctx, &mut app.documents[1], NEWER.into(), 3);
        app.active_document = Some(2);
        app.next_document = 3;
        let other = format!("{:?}", app.documents[1]);
        let other_selection = selection(&app, 2);
        let command = submit(&mut app, &commands);
        app.queue_save_close_tab();
        finish(&mut app);
        idle(&commands);
        acknowledge(&mut app, &command);
        assert_eq!(app.documents.len(), 2, "reply must not remove any tab");
        assert!(!app.save_close_write_pending());
        finish(&mut app);
        assert_eq!(app.documents.len(), 1);
        assert_eq!(app.documents[0].id, 2);
        assert_eq!(format!("{:?}", app.documents[0]), other);
        assert_eq!(selection(&app, 2), other_selection);
        assert_eq!(app.active_document, Some(2));
        assert_eq!(app.next_document, 3);
        assert!(!app.save_close_tab_busy());
        finish(&mut app);
        idle(&commands); // also proves Trust OFF emitted no Run or LSP.
    }
}

#[test]
fn stale_generation_and_wrong_request_cannot_make_close_eligible() {
    let (mut app, commands) = connected();
    let command = submit(&mut app, &commands);
    let mut event = written(&app, &command);
    event.generation -= 1;
    app.apply_event(event);
    let mut event = written(&app, &command);
    event.id += 10;
    app.apply_event(event);
    finish(&mut app);
    assert_eq!(app.documents.len(), 1);
    assert!(app.documents[0].saving);
    assert!(app.save_close_write_pending());
    acknowledge(&mut app, &command);
    finish(&mut app);
    assert!(app.documents.is_empty());
    idle(&commands);
}

#[test]
fn malformed_payload_hash_token_conflict_and_disconnected_ack_retain_draft() {
    for case in 0..6 {
        let (mut app, commands) = connected();
        let before_selection = selection(&app, 1);
        let command = submit(&mut app, &commands);
        let mut event = written(&app, &command);
        match case {
            0 => {
                event.result = Ok(Payload::Written {
                    revision: revision(NEWER),
                })
            }
            1 => {
                event.result = Ok(Payload::Written {
                    revision: revision(DRAFT).to_uppercase(),
                })
            }
            2 => {
                event.result = Ok(Payload::File {
                    path: "target.txt".into(),
                    text: DRAFT.into(),
                    revision: revision(DRAFT),
                })
            }
            3 => {
                let Some(Job::Save { submission, .. }) = app.pending.get_mut(&command.id) else {
                    panic!("Save");
                };
                *submission = None;
            }
            4 => event.result = Err("conflict: generated file changed on disk".into()),
            5 => event.connected = false,
            _ => unreachable!(),
        }
        app.apply_event(event);
        finish(&mut app);
        assert_eq!(app.documents.len(), 1, "case {case}");
        assert_eq!(app.documents[0].text, DRAFT);
        assert_eq!(app.documents[0].saved_text, BASE);
        assert_eq!(app.documents[0].revision, Some(revision(BASE)));
        assert_eq!(selection(&app, 1), before_selection);
        assert_eq!(app.documents[0].save_outcome_unknown(), case != 4);
        assert!(!app.save_close_tab_busy());
        assert!(!app.save_close_write_pending());
        idle(&commands);
    }
}

#[test]
fn newer_text_paste_undo_and_profile_changes_after_ack_prevent_removal() {
    for case in 0..5 {
        let (mut app, commands) = connected();
        let command = submit(&mut app, &commands);
        acknowledge(&mut app, &command);
        match case {
            0 => editor_state::commit(&app.editor_ctx, &mut app.documents[0], NEWER.into(), 2),
            1 => app.documents[0].text.push_str(" pasted"), // digest catches missing version increment.
            2 => {
                editor_state::commit(&app.editor_ctx, &mut app.documents[0], NEWER.into(), 2);
                editor_state::commit(&app.editor_ctx, &mut app.documents[0], DRAFT.into(), 4);
            }
            3 => app.profiles.changed(),
            4 => app.profiles.draft.program = "new uncommitted form input".into(),
            _ => unreachable!(),
        }
        let selection = selection(&app, 1);
        let newer_text = app.documents[0].text.clone();
        finish(&mut app);
        assert_eq!(app.documents.len(), 1, "case {case}");
        assert_eq!(app.documents[0].text, newer_text);
        assert_eq!(app.documents[0].saved_text, DRAFT);
        assert_eq!(app.documents[0].revision, Some(revision(DRAFT)));
        assert_eq!(super::save_close_tests::selection(&app, 1), selection);
        assert!(!app.documents[0].save_outcome_unknown());
        idle(&commands);
    }
}

#[test]
fn cancelling_close_does_not_cancel_write_and_preserves_undo_after_ack() {
    let (mut app, commands) = connected();
    frame(&mut app, 0.0, vec![]);
    let command = submit(&mut app, &commands);
    app.cancel_tab_close();
    assert!(app.notice.contains("may still finish"));
    assert!(app.documents[0].saving);
    assert!(app.pending.contains_key(&command.id));
    assert!(app.save_close_write_pending());
    acknowledge(&mut app, &command);
    finish(&mut app);
    assert_eq!(app.documents.len(), 1);
    assert_eq!(app.documents[0].saved_text, DRAFT);
    app.editor_ctx
        .memory_mut(|memory| memory.request_focus(egui::Id::new(("editor", 1u64))));
    frame(&mut app, 2.0, key(egui::Key::Z, egui::Modifiers::COMMAND));
    assert_eq!(app.documents[0].text, BASE);
    assert!(app.documents[0].dirty());
    idle(&commands);
}

#[test]
fn same_frame_ack_and_keep_editing_click_retains_tab_and_saved_fact() {
    let (mut app, commands) = connected();
    let command = submit(&mut app, &commands);
    frame(&mut app, 0.0, vec![]);
    frame(&mut app, 0.1, vec![]);
    let point = workspace_access_tests::recorded_rect(&app, "save_close_tab_cancel").center();
    app.result_tx
        .send(WorkerEvent::Response(written(&app, &command)))
        .unwrap();
    frame(
        &mut app,
        1.0,
        vec![
            egui::Event::PointerMoved(point),
            egui::Event::PointerButton {
                pos: point,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::PointerButton {
                pos: point,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    assert_eq!(app.documents.len(), 1);
    assert_eq!(app.documents[0].saved_text, DRAFT);
    assert!(!app.save_close_tab_busy());
    idle(&commands);
}

#[test]
fn production_modal_survives_idle_frames_then_save_click_and_ack_close() {
    let (mut app, commands) = connected();
    app.close_tab(1);
    frame(&mut app, 0.0, vec![]);
    frame(&mut app, 0.1, vec![]);
    frame(&mut app, 0.2, vec![]);
    assert!(app.save_close_tab_busy());
    assert!(matches!(app.confirm, Some(Confirm::CloseTab(1))));
    assert!(
        app.navigation.blocks_editor(),
        "own modal keeps editor input blocked"
    );
    assert!(!app.navigation.dialog_open());
    idle(&commands);
    let point = workspace_access_tests::recorded_rect(&app, "save_close_tab_save").center();
    frame(
        &mut app,
        0.3,
        vec![
            egui::Event::PointerMoved(point),
            egui::Event::PointerButton {
                pos: point,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::PointerButton {
                pos: point,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    let command = commands
        .try_recv()
        .expect("Save and close click dispatches one Write");
    assert!(
        matches!(&command.op, Operation::Write { path, text, expected_revision }
        if path == "target.txt" && text == DRAFT && *expected_revision == Some(revision(BASE)))
    );
    assert!(app.documents[0].saving);
    frame(&mut app, 0.4, vec![]);
    assert!(app.save_close_tab_busy());
    idle(&commands);
    app.result_tx
        .send(WorkerEvent::Response(written(&app, &command)))
        .unwrap();
    frame(&mut app, 1.0, vec![]);
    assert!(app.documents.is_empty());
    assert!(!app.save_close_tab_busy());
    idle(&commands);
}

#[test]
fn same_frame_ack_and_newer_other_tab_close_cancels_old_consent() {
    let (mut app, commands) = connected();
    app.documents.push(Document::new(
        2,
        "other.txt".into(),
        BASE.into(),
        revision(BASE),
    ));
    let command = submit(&mut app, &commands);
    app.close_tab_requested = Some(2);
    app.result_tx
        .send(WorkerEvent::Response(written(&app, &command)))
        .unwrap();
    frame(&mut app, 1.0, vec![]);
    assert_eq!(app.documents.len(), 2);
    assert_eq!(app.documents[0].saved_text, DRAFT);
    assert!(!app.save_close_tab_busy());
    idle(&commands);
}

#[test]
fn same_frame_ack_and_queued_profile_mutation_retains_tab() {
    let (mut app, commands) = connected();
    let command = submit(&mut app, &commands);
    app.queue_profile_action(profile_ui::Action::Discard);
    app.result_tx
        .send(WorkerEvent::Response(written(&app, &command)))
        .unwrap();
    frame(&mut app, 1.0, vec![]);
    assert_eq!(app.documents.len(), 1);
    assert_eq!(app.documents[0].saved_text, DRAFT);
    assert!(!app.save_close_tab_busy());
    idle(&commands);
}

#[test]
fn eof_order_retains_acknowledged_fact_but_never_closes_or_replays() {
    for ack_first in [false, true] {
        let (mut app, commands) = connected();
        let command = submit(&mut app, &commands);
        let reply = written(&app, &command);
        if ack_first {
            app.apply_event(reply);
        }
        app.connection_closed(app.generation, Ok(()));
        if !ack_first {
            app.apply_event(written(&app, &command));
        }
        finish(&mut app);
        assert_eq!(app.documents.len(), 1);
        assert_eq!(app.documents[0].text, DRAFT);
        assert_eq!(
            app.documents[0].saved_text,
            if ack_first { DRAFT } else { BASE }
        );
        assert_eq!(app.documents[0].save_outcome_unknown(), !ack_first);
        assert!(!app.save_close_tab_busy());
        idle(&commands);
    }
}

#[test]
fn lost_or_replaced_job_after_cancel_marks_only_original_owner_unknown() {
    for replacement in [false, true] {
        let (mut app, commands) = connected();
        let command = submit(&mut app, &commands);
        app.cancel_tab_close();
        app.pending.remove(&command.id);
        if replacement {
            app.pending.insert(
                command.id,
                Job::Search {
                    query: "other job".into(),
                },
            );
        }
        app.documents.push(Document::new(
            2,
            "other.txt".into(),
            BASE.into(),
            revision(BASE),
        ));
        let other = format!("{:?}", app.documents[1]);
        assert!(app.mutation_pending());
        assert!(app.disconnect_problem().is_some());
        finish(&mut app);
        assert!(app.documents[0].save_outcome_unverifiable);
        assert!(app.documents[0].interrupted_save.is_none());
        assert_eq!(app.documents[0].text, DRAFT);
        assert_eq!(app.documents[0].saved_text, BASE);
        assert_eq!(format!("{:?}", app.documents[1]), other);
        assert!(!app.save_close_write_pending());
        app.save_document(1);
        app.queue_save_all();
        idle(&commands);
    }
}

#[test]
fn cancelled_close_with_lost_job_is_protected_on_direct_reply_or_eof() {
    for eof in [false, true] {
        let (mut app, commands) = connected();
        let command = submit(&mut app, &commands);
        app.cancel_tab_close();
        app.pending.remove(&command.id);
        if eof {
            app.connection_closed(app.generation, Ok(()));
        } else {
            app.apply_event(written(&app, &command));
        }
        assert!(!app.save_close_write_pending());
        assert!(app.documents[0].save_outcome_unverifiable);
        assert!(app.documents[0].interrupted_save.is_none());
        assert!(!app.documents[0].saving);
        assert_eq!(app.documents[0].text, DRAFT);
        assert_eq!(app.documents[0].saved_text, BASE);
        finish(&mut app);
        assert_eq!(app.documents.len(), 1);
        idle(&commands);
    }
}

#[test]
fn lost_job_fallback_never_marks_reopened_id_or_reconnected_workspace() {
    for replacement in 0..3 {
        let (mut app, commands) = connected();
        let command = submit(&mut app, &commands);
        app.pending.remove(&command.id);
        match replacement {
            0 => app.documents[0].id = 2,
            1 => app.generation += 1,
            2 => app.root = "/different-workspace".into(),
            _ => unreachable!(),
        }
        finish(&mut app);
        assert_eq!(app.documents.len(), 1);
        assert!(!app.documents[0].save_outcome_unknown());
        assert_eq!(app.documents[0].text, DRAFT);
        idle(&commands);
    }
}

#[test]
fn discard_consent_is_snapshot_bound_and_only_removes_after_final_guard() {
    for mutation in 0..10 {
        let (mut app, commands) = connected();
        app.close_tab(1);
        app.queue_discard_close_tab();
        assert_eq!(app.documents.len(), 1);
        match mutation {
            0 => {}
            1 => app.documents[0].text.push_str("newer"),
            2 => app.documents[0].edit_version += 1,
            3 => app.documents[0].saved_text.push_str("new baseline"),
            4 => app.documents[0].revision = Some(revision(NEWER)),
            5 => app.documents[0].save_outcome_unverifiable = true,
            6 => app.profiles.changed(),
            7 => app.generation += 1,
            8 => app.documents[0].path = "renamed.txt".into(),
            9 => app.documents[0].saving = true,
            _ => unreachable!(),
        }
        let before = format!("{:?}", app.documents[0]);
        finish(&mut app);
        if mutation == 0 {
            assert!(app.documents.is_empty());
        } else {
            assert_eq!(app.documents.len(), 1, "mutation {mutation}");
            assert_eq!(format!("{:?}", app.documents[0]), before);
            assert!(app.notice.contains("kept open"));
        }
        idle(&commands);
    }
}

#[test]
fn changed_unknown_token_and_exhausted_version_invalidate_discard_consent() {
    for changed_token in [false, true] {
        let (mut app, commands) = connected();
        if changed_token {
            app.documents[0].interrupted_save =
                interrupted_save::InterruptedSave::capture(&app, &app.documents[0]);
        }
        app.close_tab(1);
        app.queue_discard_close_tab();
        if changed_token {
            app.next_request += 1;
            app.documents[0].interrupted_save =
                interrupted_save::InterruptedSave::capture(&app, &app.documents[0]);
        } else {
            app.documents[0].edit_version = u64::MAX;
        }
        finish(&mut app);
        assert_eq!(app.documents.len(), 1);
        assert_eq!(app.documents[0].text, DRAFT);
        idle(&commands);
    }
}

#[test]
fn admission_exhaustion_and_save_all_competition_emit_no_extra_write() {
    for exhaustion in [0, u64::MAX] {
        let (mut app, commands) = connected();
        app.next_request = exhaustion;
        app.close_tab(1);
        app.queue_save_close_tab();
        finish(&mut app);
        assert_eq!(app.documents.len(), 1);
        assert!(!app.documents[0].saving);
        idle(&commands);
    }
    let (mut app, commands) = connected();
    app.close_tab(1);
    app.queue_save_all();
    assert!(!app.save_all_busy());
    app.queue_save_close_tab();
    finish(&mut app);
    let command = commands.try_recv().unwrap();
    app.cancel_tab_close();
    app.save_document(1);
    app.queue_save_all();
    app.close_tab(1);
    assert!(!app.save_all_busy());
    acknowledge(&mut app, &command);
    finish(&mut app);
    assert_eq!(app.documents.len(), 1);
    idle(&commands);
}

#[test]
fn dirty_owned_profile_form_disables_save_close_without_serialization() {
    let (mut app, commands) = connected();
    let config = r#"{"version":1,"profiles":[{"name":"check","program":"tool","args":[],"timeout_secs":30}]}"#;
    app.documents[0] = Document::new(1, profile_ui::PATH.into(), config.into(), revision(config));
    app.load_profiles();
    app.select_profile(Some(0));
    app.profiles.draft.program = "edited-form-program".into();
    app.profiles.changed();
    assert!(app.profiles.dirty());
    assert!(app.profiles.owns_document(1));
    let raw = app.documents[0].text.clone();
    app.close_tab(1);
    assert!(app.save_close_problem().unwrap().contains("Save profile"));
    app.queue_save_close_tab();
    finish(&mut app);
    assert_eq!(app.documents[0].text, raw);
    assert_eq!(app.profiles.draft.program, "edited-form-program");
    assert!(app.profiles.dirty());
    idle(&commands);
    app.queue_discard_close_tab();
    app.profiles.draft.args.push("later form edit".into());
    app.profiles.changed();
    finish(&mut app);
    assert_eq!(app.documents.len(), 1);
    assert!(app.profiles.dirty());
}

#[test]
fn modal_owns_save_all_save_find_close_undo_typing_and_paste() {
    for events in [
        key(egui::Key::S, egui::Modifiers::COMMAND),
        key(
            egui::Key::S,
            egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
        ),
        key(egui::Key::W, egui::Modifiers::COMMAND),
        key(egui::Key::F, egui::Modifiers::COMMAND),
        key(egui::Key::Z, egui::Modifiers::COMMAND),
        vec![egui::Event::Text("new input".into())],
        vec![egui::Event::Paste("new paste".into())],
    ] {
        let (mut app, commands) = connected();
        frame(&mut app, 0.0, vec![]);
        app.close_tab(1);
        frame(&mut app, 1.0, vec![]);
        let before = format!("{:?}", app.documents[0]);
        let before_selection = selection(&app, 1);
        frame(&mut app, 2.0, events);
        assert_eq!(format!("{:?}", app.documents[0]), before);
        assert_eq!(selection(&app, 1), before_selection);
        assert!(!app.save_all_busy());
        assert!(!app.find_open);
        assert!(app.save_close_tab_busy());
        idle(&commands);
    }
}

#[test]
fn competing_window_restore_and_recovery_modal_cannot_close_acknowledged_tab() {
    for competition in 0..4 {
        let (mut app, commands) = connected();
        let command = submit(&mut app, &commands);
        acknowledge(&mut app, &command);
        match competition {
            0 => app.request_window_close(&app.editor_ctx.clone()),
            1 => app.recovery.restoring_generation = Some(app.generation),
            2 => {
                app.recovery.remove_confirmation = Some(
                    cedar_recovery::record_id(&app.recovery_workspace().unwrap(), "target.txt")
                        .unwrap(),
                )
            }
            3 => app.open_form = true,
            _ => unreachable!(),
        }
        finish(&mut app);
        assert_eq!(app.documents.len(), 1);
        assert!(!app.allow_close);
        assert_eq!(app.documents[0].saved_text, DRAFT);
        idle(&commands);
    }
}

#[test]
fn recovery_removal_failure_survives_successful_save_and_close() {
    let (mut app, commands) = connected();
    let workspace = app.recovery_workspace().unwrap();
    let id = cedar_recovery::record_id(&workspace, "target.txt").unwrap();
    app.recovery.drafts.push(cedar_recovery::DraftMetadata {
        id: id.clone(),
        workspace: workspace.clone(),
        path: "target.txt".into(),
        base_revision: Some(revision(BASE)),
        modified_ms: 1,
        text_bytes: DRAFT.len(),
        base_text_bytes: BASE.len(),
    });
    app.recovery.authorize(&workspace, &app.documents[0]);
    let command = submit(&mut app, &commands);
    acknowledge(&mut app, &command);
    finish(&mut app);
    assert!(app.documents.is_empty());
    assert!(!app.recovery.removals_finished());
    assert!(!app.recovery.failures().is_empty());
    assert!(app.recovery.visible);
    assert_eq!(app.recovery.drafts[0].id, id);
    assert!(app.notice.contains("pending or failed"));
    idle(&commands);
}
