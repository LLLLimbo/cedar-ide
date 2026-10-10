//! Native frames cover the places where Save All meets editor input, menus,
//! profile serialization, connection ownership and ordinary save side effects.
//! This module is a child of language_ui so its synthetic Maven session can use
//! the same private setup as the existing Maven regressions.
use crate::*;
use sha2::{Digest, Sha256};

const ROOT: &str = "/project";
const BASE: &str = "saved text\n";
const MENU_SAVE_ALL: &str = "Save all editor buffers · Ctrl/Cmd+Shift+S";
const MENU_CANCEL: &str = "Cancel remaining saves";

fn revision(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

fn app() -> (CedarApp, Receiver<Command>) {
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
    app.agent_info = Some(agent_support::full_test_agent());
    app.generation = 7;
    app.open_form = false;
    app.explorer.mode = explorer_tree::Mode::Tree;
    // Exercise the shipped desktop spacing, not egui's narrower defaults.
    app.editor_ctx.style_mut(|style| {
        style.spacing.item_spacing = egui::vec2(9.0, 8.0);
        style.spacing.button_padding = egui::vec2(10.0, 6.0);
        style
            .text_styles
            .insert(egui::TextStyle::Body, FontId::proportional(14.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, FontId::proportional(14.0));
        style
            .text_styles
            .insert(egui::TextStyle::Monospace, FontId::monospace(14.0));
    });
    for id in 1..=3 {
        let mut doc = Document::new(id, format!("file-{id}.txt"), BASE.into(), revision(BASE));
        editor_state::commit(&app.editor_ctx, &mut doc, format!("draft {id}\n"), 0);
        app.documents.push(doc);
    }
    app.active_document = Some(1);
    app.next_document = 4;
    let (worker, commands) = Worker::recording();
    app.worker = Some(worker);
    (app, commands)
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

fn focus(app: &CedarApp, document: u64) {
    app.editor_ctx.memory_mut(|memory| {
        memory.request_focus(egui::Id::new(("editor", document)));
    });
}

fn idle(commands: &Receiver<Command>) {
    assert!(
        matches!(
            commands.try_recv(),
            Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected)
        ),
        "unexpected operation after the asserted save boundary"
    );
}

fn write(commands: &Receiver<Command>, expected_path: &str) -> Command {
    let command = commands.try_recv().expect("one explicit conditional Write");
    assert!(matches!(&command.op, Operation::Write { path, .. } if path == expected_path));
    idle(commands);
    command
}

fn enqueue_ack(app: &CedarApp, command: &Command, exact: bool) {
    let Operation::Write { text, .. } = &command.op else {
        panic!("expected Write")
    };
    app.result_tx
        .send(WorkerEvent::Response(Event {
            generation: app.generation,
            id: command.id,
            connected: true,
            result: Ok(Payload::Written {
                revision: revision(if exact {
                    text
                } else {
                    "wrong acknowledged bytes"
                }),
            }),
        }))
        .unwrap();
}

fn start(app: &mut CedarApp, commands: &Receiver<Command>) -> Command {
    frame(app, 0.0, vec![]);
    app.queue_save_all();
    idle(commands);
    frame(app, 0.1, vec![]);
    assert!(app.save_all_busy());
    write(commands, "file-1.txt")
}

fn labels(output: &egui::FullOutput) -> Vec<(&egui::epaint::TextShape, egui::Rect)> {
    fn collect<'a>(
        shape: &'a egui::epaint::Shape,
        clip: egui::Rect,
        found: &mut Vec<(&'a egui::epaint::TextShape, egui::Rect)>,
    ) {
        match shape {
            egui::epaint::Shape::Text(text) => found.push((text, clip)),
            egui::epaint::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect(shape, clip, found);
                }
            }
            _ => {}
        }
    }
    let mut found = Vec::new();
    for shape in &output.shapes {
        collect(&shape.shape, shape.clip_rect, &mut found);
    }
    found
}

fn label_rect(output: &egui::FullOutput, label: &str) -> egui::Rect {
    labels(output)
        .into_iter()
        .find(|(text, _)| text.galley.job.text == label)
        .unwrap_or_else(|| panic!("missing visible control: {label}"))
        .0
        .visual_bounding_rect()
}

fn pointer(at: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(at),
        egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        },
    ]
}

fn click(app: &mut CedarApp, time: f64, at: egui::Pos2) -> egui::FullOutput {
    frame(app, time, pointer(at, true));
    frame(app, time + 0.01, pointer(at, false))
}

fn open_save_menu(app: &mut CedarApp, time: f64) -> egui::FullOutput {
    let output = frame(app, time, vec![]);
    let at = label_rect(&output, "▾").center();
    click(app, time + 0.01, at);
    frame(app, time + 0.03, vec![])
}

#[test]
fn queued_intent_rechecks_same_frame_native_typing_and_undo_before_first_write() {
    for undo in [false, true] {
        let (mut app, commands) = app();
        frame(&mut app, 0.0, vec![]);
        focus(&app, 1);
        let before = app.documents[0].text.clone();
        let version = app.documents[0].edit_version;
        app.queue_save_all();
        frame(
            &mut app,
            1.0,
            if undo {
                key(egui::Key::Z, egui::Modifiers::COMMAND)
            } else {
                vec![egui::Event::Text("newer native input".into())]
            },
        );
        assert_ne!(app.documents[0].text, before);
        assert!(app.documents[0].edit_version > version);
        assert_eq!(app.documents[0].saved_text, BASE);
        assert!(!app.save_all_busy());
        idle(&commands);
    }
}

#[test]
fn acknowledged_first_write_waits_for_same_frame_typing_in_later_unsent_tab() {
    let (mut app, commands) = app();
    let first = start(&mut app, &commands);
    app.active_document = Some(3);
    frame(&mut app, 1.0, vec![]);
    focus(&app, 3);
    let before = app.documents[2].text.clone();
    enqueue_ack(&app, &first, true);
    frame(&mut app, 2.0, vec![egui::Event::Text(" later".into())]);
    assert_ne!(app.documents[2].text, before);
    assert!(!app.documents[0].dirty());
    assert_eq!(app.documents[1].saved_text, BASE);
    assert_eq!(app.documents[2].saved_text, BASE);
    assert!(!app.save_all_busy());
    idle(&commands);
}

#[test]
fn native_undo_on_acknowledgement_frame_preserves_newer_inflight_draft_and_advances() {
    let (mut app, commands) = app();
    let first = start(&mut app, &commands);
    focus(&app, 1);
    enqueue_ack(&app, &first, true);
    frame(&mut app, 1.0, key(egui::Key::Z, egui::Modifiers::COMMAND));
    assert_eq!(app.documents[0].text, BASE);
    assert_eq!(app.documents[0].saved_text, "draft 1\n");
    assert!(app.documents[0].dirty());
    assert!(!app.documents[0].save_outcome_unknown());
    write(&commands, "file-2.txt");
    assert!(app.save_all_busy());
}

#[test]
fn menu_cancel_on_acknowledgement_frame_stops_before_next_write_even_with_following_eof() {
    for eof in [false, true] {
        let (mut app, commands) = app();
        let first = start(&mut app, &commands);
        let output = open_save_menu(&mut app, 1.0);
        let at = label_rect(&output, MENU_CANCEL).center();
        enqueue_ack(&app, &first, true);
        if eof {
            app.result_tx
                .send(WorkerEvent::TransportLost {
                    generation: app.generation,
                    message: "transport_closed: generated EOF".into(),
                })
                .unwrap();
        }
        let mut events = pointer(at, true);
        events.extend(pointer(at, false));
        frame(&mut app, 2.0, events);
        assert!(!app.save_all_busy());
        assert_eq!(app.documents[0].saved_text, "draft 1\n");
        assert!(!app.documents[0].save_outcome_unknown());
        assert_eq!(app.documents[1].saved_text, BASE);
        assert!(app.documents[1].dirty());
        assert_eq!(app.ready(), !eof);
        assert!(app.save_all_message().unwrap().contains("1 acknowledged"));
        idle(&commands);
    }
}

#[test]
fn eof_before_reply_marks_only_submitted_file_unknown_and_cannot_replay_late_ack() {
    let (mut app, commands) = app();
    let first = start(&mut app, &commands);
    app.result_tx
        .send(WorkerEvent::TransportLost {
            generation: app.generation,
            message: "transport_closed: generated EOF".into(),
        })
        .unwrap();
    enqueue_ack(&app, &first, true);
    frame(&mut app, 1.0, vec![]);
    assert!(!app.save_all_busy());
    assert!(app.documents[0].save_outcome_unknown());
    assert!(app.documents.iter().all(|doc| doc.saved_text == BASE));
    assert!(app.documents[1..]
        .iter()
        .all(|doc| !doc.save_outcome_unknown()));
    assert!(app.save_all_message().unwrap().contains("1 unknown"));
    idle(&commands);
}

#[test]
fn missing_save_owner_and_same_frame_undo_retain_owned_recovery_without_another_frame() {
    use cedar_recovery::{record_id, Store};
    use std::time::{Duration, Instant};

    for cause in ["written", "missing_eof", "replaced_eof"] {
        let temp = tempfile::tempdir().unwrap();
        let recovery_path = temp.path().join("recovery");
        let (mut app, commands) = app();
        app.recovery
            .start(Ok(recovery_path.clone()), &app.editor_ctx);
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
        let workspace = app.recovery_workspace().unwrap();
        let first = start(&mut app, &commands);
        assert!(matches!(
            app.pending.remove(&first.id),
            Some(Job::Save { .. })
        ));
        if cause == "written" {
            enqueue_ack(&app, &first, true);
            app.cancel_save_all();
        } else {
            if cause == "replaced_eof" {
                app.pending.insert(
                    first.id,
                    Job::Search {
                        query: "unrelated replacement job".into(),
                    },
                );
            }
            app.result_tx
                .send(WorkerEvent::TransportLost {
                    generation: app.generation,
                    message: "transport_closed: generated orphaned-save EOF".into(),
                })
                .unwrap();
        }
        focus(&app, 1);
        // Poll sees an orphaned response or passive loss, then native Undo makes
        // the text match its baseline. The final guard must retain uncertainty and
        // renew owned recovery within this frame, after that last editor input.
        frame(&mut app, 1.0, key(egui::Key::Z, egui::Modifiers::COMMAND));
        let doc = &app.documents[0];
        assert_eq!(doc.text, BASE);
        assert_eq!(doc.saved_text, BASE);
        assert_eq!(doc.revision, Some(revision(BASE)));
        assert!(doc.save_outcome_unverifiable);
        assert!(doc.interrupted_save.is_none());
        assert!(doc.dirty());
        assert!(!doc.saving);
        assert!(!app.save_all_busy());
        assert_eq!(app.ready(), cause == "written");
        assert!(app.documents[1..]
            .iter()
            .all(|doc| !doc.save_outcome_unknown()));
        assert!(app.save_all_message().unwrap().contains("1 unknown"));
        app.save_document(1);
        app.check_interrupted_save();
        assert!(app.documents[0].save_outcome_unverifiable);
        idle(&commands);

        // No extra frame or recovery_tick is allowed here: either could mask a
        // removal queued before the final fallback made this document dirty.
        // Recovery actor Drop flushes and joins its outstanding owned mutations.
        drop(app);
        let store = Store::open(&recovery_path).unwrap();
        let recovered = store
            .read(&record_id(&workspace, "file-1.txt").unwrap())
            .expect("the unknown save must retain its owned recovery record");
        assert_eq!(recovered.text, BASE);
        assert_eq!(recovered.base_text, BASE);
        assert_eq!(recovered.base_revision, Some(revision(BASE)));
        assert_eq!(store.list().unwrap().drafts.len(), 3);
    }
}

#[test]
fn orphaned_save_loss_ignores_stale_events_and_does_not_mark_replacement_owners() {
    for change in ["stale_event", "document", "path", "workspace", "session"] {
        let (mut app, commands) = app();
        let first = start(&mut app, &commands);
        assert!(matches!(
            app.pending.remove(&first.id),
            Some(Job::Save { .. })
        ));
        let original_generation = app.generation;
        let mut replacement_commands = None;
        app.apply_worker_event(WorkerEvent::TransportLost {
            generation: if change == "stale_event" {
                original_generation - 1
            } else {
                original_generation
            },
            message: "transport_closed: generated identity-isolation EOF".into(),
        });
        match change {
            "stale_event" => {
                assert!(app.ready());
                assert!(app.worker.is_some());
            }
            "document" => {
                app.documents[0] = Document::new(
                    99,
                    "file-1.txt".into(),
                    "replacement document".into(),
                    revision("replacement document"),
                );
                app.active_document = Some(99);
            }
            "path" => app.documents[0].path = "replacement-path.txt".into(),
            "workspace" => app.root = "/replacement-workspace".into(),
            "session" => {
                let (worker, reconnected) = Worker::recording();
                app.generation += 1;
                app.state = ConnectionState::Ready;
                app.worker = Some(worker);
                app.agent_info = Some(agent_support::full_test_agent());
                replacement_commands = Some(reconnected);
            }
            _ => unreachable!(),
        }
        let before: Vec<_> = app
            .documents
            .iter()
            .map(|doc| {
                (
                    doc.id,
                    doc.path.clone(),
                    doc.text.clone(),
                    doc.saved_text.clone(),
                    doc.revision.clone(),
                    doc.edit_version,
                    doc.saving,
                )
            })
            .collect();
        frame(&mut app, 1.0, vec![]);
        assert!(
            app.documents.iter().all(|doc| !doc.save_outcome_unknown()),
            "{change}"
        );
        let after: Vec<_> = app
            .documents
            .iter()
            .map(|doc| {
                (
                    doc.id,
                    doc.path.clone(),
                    doc.text.clone(),
                    doc.saved_text.clone(),
                    doc.revision.clone(),
                    doc.edit_version,
                    doc.saving,
                )
            })
            .collect();
        assert_eq!(
            after, before,
            "loss settlement mutated a replacement owner: {change}"
        );
        idle(&commands);
        if let Some(reconnected) = &replacement_commands {
            assert!(app.ready(), "the replacement connection must remain usable");
            idle(reconnected);
        }
        if change == "stale_event" {
            assert!(
                app.save_all_busy(),
                "a stale EOF must not settle the live write"
            );
            // Only the current connection's later loss may release this batch.
            app.apply_worker_event(WorkerEvent::TransportLost {
                generation: original_generation,
                message: "transport_closed: current EOF after stale notification".into(),
            });
            frame(&mut app, 2.0, vec![]);
            assert!(app.documents[0].save_outcome_unverifiable);
        } else {
            assert!(!app.save_all_busy());
        }
        idle(&commands);
    }
}

#[test]
fn lifecycle_guards_own_the_gap_after_acknowledgement_before_frame_dispatch() {
    for transition in [
        "disconnect",
        "reconnect",
        "tab",
        "window",
        "begin_close",
        "restore",
    ] {
        let (mut app, commands) = app();
        let first = start(&mut app, &commands);
        enqueue_ack(&app, &first, true);
        app.poll();
        assert!(app.pending.is_empty());
        assert!(app.save_all_busy());
        assert!(app.mutation_pending());
        let ctx = app.editor_ctx.clone();
        let generation = app.generation;
        let count = app.documents.len();
        match transition {
            "disconnect" => app.disconnect_idle(),
            "reconnect" => {
                let form = app.active_form.clone().unwrap();
                app.connect(&ctx, form);
            }
            "tab" => app.close_tab(2),
            "window" => app.request_window_close(&ctx),
            "begin_close" => app.begin_close(&ctx),
            "restore" => {
                let restored = app.install_recovered(cedar_recovery::Draft {
                    workspace: app.recovery_workspace().unwrap(),
                    path: "restored.txt".into(),
                    text: "recovered draft".into(),
                    base_text: BASE.into(),
                    base_revision: Some(revision(BASE)),
                    modified_ms: 1,
                });
                assert!(restored.is_err(), "restore must respect Save All ownership");
            }
            _ => unreachable!(),
        }
        assert!(
            app.ready(),
            "{transition} retired the connection between writes"
        );
        assert!(app.worker.is_some());
        assert_eq!(app.generation, generation);
        assert_eq!(app.documents.len(), count);
        assert!(app.confirm.is_none());
        assert!(app.recovery.closing.is_none());
        assert!(!app.allow_close);
        idle(&commands);
        app.cancel_save_all();
        frame(&mut app, 1.0, vec![]);
        assert!(!app.save_all_busy());
        idle(&commands);
    }
}

fn load_profile(app: &mut CedarApp, commands: &Receiver<Command>) -> u64 {
    use task_profiles::{encode_task_file, TaskFile, TaskProfile};
    app.profiles.connected(app.recovery_workspace().unwrap());
    app.load_profiles();
    let command = commands.try_recv().unwrap();
    assert!(matches!(command.op, Operation::Read { .. }));
    let text = encode_task_file(&TaskFile {
        version: 1,
        profiles: vec![TaskProfile {
            name: "Build".into(),
            program: "cargo".into(),
            args: vec!["check".into()],
            timeout_secs: 30,
        }],
    })
    .unwrap();
    app.apply_event(Event {
        generation: app.generation,
        id: command.id,
        connected: true,
        result: Ok(Payload::File {
            path: profile_ui::PATH.into(),
            revision: revision(&text),
            text,
        }),
    });
    app.select_profile(Some(0));
    idle(commands);
    app.documents
        .iter()
        .find(|doc| doc.path == profile_ui::PATH)
        .unwrap()
        .id
}

#[test]
fn manual_and_queued_profile_save_are_blocked_before_editor_or_form_serialization() {
    for acknowledge_first in [false, true] {
        let (mut app, commands) = app();
        let profile = load_profile(&mut app, &commands);
        app.profiles
            .draft
            .args
            .push("retain this form-only edit".into());
        app.profiles.changed();
        let draft = app.profiles.draft.clone();
        let epoch = app.profiles.epoch;
        let raw = app.documents.iter().find(|doc| doc.id == profile).unwrap();
        let original = (
            raw.text.clone(),
            raw.saved_text.clone(),
            raw.revision.clone(),
            raw.edit_version,
        );
        app.active_document = Some(1);
        let first = start(&mut app, &commands);
        if acknowledge_first {
            enqueue_ack(&app, &first, true);
            app.poll();
        }
        app.save_document(2);
        app.save_profile();
        app.queue_profile_action(profile_ui::Action::Save);
        app.finish_profile_actions();
        let raw = app.documents.iter().find(|doc| doc.id == profile).unwrap();
        assert_eq!(
            (
                raw.text.clone(),
                raw.saved_text.clone(),
                raw.revision.clone(),
                raw.edit_version
            ),
            original
        );
        assert_eq!(app.profiles.draft, draft);
        assert_eq!(app.profiles.epoch, epoch);
        assert!(app.profiles.dirty());
        assert!(!raw.saving);
        idle(&commands);
        app.cancel_save_all();
        if !acknowledge_first {
            enqueue_ack(&app, &first, true);
        }
        frame(&mut app, 2.0, vec![]);
        assert!(!app.save_all_busy());
        // The same untouched form remains saveable by its explicit action.
        app.save_profile();
        let saved = write(&commands, profile_ui::PATH);
        assert!(
            matches!(saved.op, Operation::Write { text, .. } if text.contains("retain this form-only edit"))
        );
    }
}

#[test]
fn strict_native_save_all_chord_never_falls_through_to_single_save() {
    for modifiers in [
        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
        egui::Modifiers::COMMAND | egui::Modifiers::CTRL | egui::Modifiers::SHIFT,
        egui::Modifiers::COMMAND | egui::Modifiers::MAC_CMD | egui::Modifiers::SHIFT,
    ] {
        let (mut app, commands) = app();
        app.active_document = Some(3);
        frame(&mut app, 0.0, vec![]);
        frame(&mut app, 1.0, key(egui::Key::S, modifiers));
        assert!(app.save_all_busy());
        write(&commands, "file-1.txt");
        assert!(
            !app.documents[2].saving,
            "ordinary Save must not consume the Save All chord"
        );
    }
    for modifiers in [
        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT | egui::Modifiers::ALT,
        egui::Modifiers::COMMAND
            | egui::Modifiers::CTRL
            | egui::Modifiers::MAC_CMD
            | egui::Modifiers::SHIFT,
    ] {
        let (mut app, commands) = app();
        frame(&mut app, 0.0, vec![]);
        frame(&mut app, 1.0, key(egui::Key::S, modifiers));
        assert!(!app.save_all_busy());
        assert!(app.documents.iter().all(|doc| !doc.saving));
        idle(&commands);
    }
    // The more-specific matcher must retain the existing active-tab command.
    let (mut app, commands) = app();
    app.active_document = Some(3);
    frame(&mut app, 0.0, vec![]);
    frame(&mut app, 1.0, key(egui::Key::S, egui::Modifiers::COMMAND));
    assert!(!app.save_all_busy());
    write(&commands, "file-3.txt");
    assert!(!app.documents[0].saving);
}

#[test]
fn mixed_native_save_all_chord_and_typing_preserves_input_without_sending_a_write() {
    let (mut app, commands) = app();
    frame(&mut app, 0.0, vec![]);
    focus(&app, 1);
    let before = app.documents[0].text.clone();
    let mut events = key(
        egui::Key::S,
        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
    );
    events.push(egui::Event::Text("same frame".into()));
    frame(&mut app, 1.0, events);
    assert_ne!(app.documents[0].text, before);
    assert!(!app.save_all_busy());
    idle(&commands);
}

#[test]
fn save_controls_and_menu_are_visible_and_clickable_at_supported_minimum_width() {
    let (mut app, commands) = app();
    frame(&mut app, 0.0, vec![]);
    let output = frame(&mut app, 0.1, vec![]);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(780.0, 540.0));
    for label in [
        "Save",
        "▾",
        "Recovery",
        "Open workspace",
        "Reconnect",
        "Open file",
    ] {
        let (text, clip) = labels(&output)
            .into_iter()
            .find(|(text, _)| text.galley.job.text == label)
            .unwrap_or_else(|| panic!("missing header control {label}"));
        let rect = text.visual_bounding_rect();
        assert!(
            screen.expand(1.0).contains_rect(rect),
            "{label} outside minimum window: {rect:?}"
        );
        assert!(
            clip.expand(1.0).contains_rect(rect),
            "{label} clipped at minimum width: {rect:?}"
        );
        assert!(rect.bottom() <= 54.0, "{label} escaped header");
    }
    let menu = open_save_menu(&mut app, 1.0);
    let at = label_rect(&menu, MENU_SAVE_ALL).center();
    assert!(screen.contains(at));
    click(&mut app, 1.1, at);
    assert!(app.save_all_busy());
    write(&commands, "file-1.txt");
}

#[test]
fn batch_pom_acknowledgement_reuses_maven_witness_hook_only_for_exact_saved_bytes() {
    use super::{Action, ActionKind};
    use crate::java_language::ServerMode;
    use serde_json::json;
    const POM: &str = "<project><modelVersion>4.0.0</modelVersion></project>";
    const CHANGED: &str = "<project><modelVersion>4.0.0</modelVersion><!-- draft --></project>";
    for exact in [false, true] {
        let (mut app, commands) = app();
        app.documents[0] = Document::new(1, "pom.xml".into(), POM.into(), revision(POM));
        editor_state::commit(&app.editor_ctx, &mut app.documents[0], CHANGED.into(), 0);
        app.language.mode = ServerMode::Java;
        app.language.maven.enabled = true;
        // Activate through the ordinary typed language response handler. This
        // synthetic session isolates the post-Write POM witness from auto-sync.
        app.apply_language_action(
            Action {
                session: app.language.session,
                kind: ActionKind::Start,
            },
            json!({
                "started": true,
                "initialize": {"capabilities": {}, "cedar_java_profile": "maven_leaf",
                    "cedar_java_maven_model": true, "cedar_java_maven_pom_sha256": revision(POM)},
                "root_uri": "file:///project"
            }),
        );
        app.language.automatic = false;
        assert!(app.language.maven_model.active());
        assert!(!app.language.maven_model.restart_required());
        app.queue_save_all();
        frame(&mut app, 0.0, vec![]);
        let pom = write(&commands, "pom.xml");
        enqueue_ack(&app, &pom, exact);
        frame(&mut app, 1.0, vec![]);
        assert_eq!(app.language.maven_model.restart_required(), exact);
        assert_eq!(
            app.language.maven_model.pom_sha256(),
            Some(revision(POM).as_str())
        );
        assert!(app.language.running);
        if exact {
            assert_eq!(app.documents[0].saved_text, CHANGED);
            assert!(!app.documents[0].save_outcome_unknown());
            write(&commands, "file-2.txt");
        } else {
            assert_eq!(app.documents[0].saved_text, POM);
            assert!(app.documents[0].save_outcome_unknown());
            assert!(!app.save_all_busy());
            idle(&commands);
        }
    }
}
