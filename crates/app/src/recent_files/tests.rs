use super::*;
use crate::{
    model::Document,
    worker::{Command, Event, Worker},
    ConnectForm, ConnectionState,
};
use cedar_protocol::{Operation, Payload};
use std::sync::mpsc::Receiver;
fn app() -> (CedarApp, Receiver<Command>) {
    let mut app = CedarApp::empty();
    let form = ConnectForm {
        local_root: "/workspace".into(),
        ..Default::default()
    };
    app.workspace_key = Some(form.key());
    app.active_form = Some(form);
    app.root = "/workspace".into();
    app.state = ConnectionState::Ready;
    app.open_form = false;
    app.recent_connected();
    app.documents.push(Document::new(
        1,
        "first.rs".into(),
        "first\nsecond".into(),
        "r0".into(),
    ));
    app.active_document = Some(1);
    app.next_document = 2;
    let (worker, commands) = Worker::recording();
    app.worker = Some(worker);
    (app, commands)
}
fn frame(app: &mut CedarApp, events: Vec<egui::Event>) {
    let ctx = app.editor_ctx.clone();
    let mut frame = eframe::Frame::_new_kittest();
    let mut raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(780.0, 540.0),
        )),
        events,
        ..Default::default()
    };
    eframe::App::raw_input_hook(app, &ctx, &mut raw);
    let _ = ctx.run(raw, |ctx| eframe::App::update(app, ctx, &mut frame));
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
fn show(app: &mut CedarApp) {
    frame(app, vec![]);
    frame(app, key(egui::Key::E, egui::Modifiers::COMMAND));
    frame(app, vec![]);
}
#[test]
fn bounded_exact_paths_deduplicate_and_evict_without_text_or_case_folding() {
    let mut recent = RecentFiles::default();
    for n in 0..70 {
        recent.touch(&format!("{n}.rs"));
    }
    assert_eq!(recent.paths.len(), 64);
    assert_eq!(recent.paths.last().unwrap().as_ref(), "6.rs");
    recent.touch("A.rs");
    recent.touch("a.rs");
    recent.touch("A.rs");
    assert_eq!(recent.paths[0].as_ref(), "A.rs");
    assert_eq!(recent.paths[1].as_ref(), "a.rs");
    let old = recent.paths.clone();
    for path in ["../bad", "/bad", "C:bad", "a\\b", ""] {
        recent.touch(path);
    }
    recent.touch(&"x".repeat(4097));
    assert_eq!(old, recent.paths);
    assert_eq!(
        recent.bytes,
        recent.paths.iter().map(|p| p.len()).sum::<usize>()
    );
    assert!(recent.bytes <= PATH_BYTES);
    for n in 0..64 {
        recent.touch(&format!("{n:02}{}", "x".repeat(4094)));
    }
    assert_eq!(recent.paths.len(), 64);
    assert_eq!(recent.bytes, PATH_BYTES);
}
#[test]
fn observer_admits_settled_activation_and_same_tab_first_save_but_clear_never_idle_reseeds() {
    let (mut app, _) = app();
    frame(&mut app, vec![]);
    assert_eq!(app.recent_files.paths[0].as_ref(), "first.rs");
    app.recent_clear();
    frame(&mut app, vec![]);
    assert!(app.recent_files.paths.is_empty());
    app.documents
        .push(Document::new(2, "new.rs".into(), "draft".into(), "".into()));
    app.documents[1].revision = None;
    app.active_document = Some(2);
    frame(&mut app, vec![]);
    assert!(app.recent_files.paths.is_empty());
    app.documents[1].revision = Some("saved".into());
    frame(&mut app, vec![]);
    assert_eq!(app.recent_files.paths[0].as_ref(), "new.rs");
}
#[test]
fn closed_file_reopens_through_exactly_one_ordinary_read_without_a_caret_hint() {
    let (mut app, commands) = app();
    frame(&mut app, vec![]);
    app.remove_tab(1);
    frame(&mut app, vec![]);
    show(&mut app);
    assert_eq!(app.recent_files.visible_paths(), vec!["first.rs"]);
    assert!(commands.try_recv().is_err());
    frame(&mut app, key(egui::Key::Enter, egui::Modifiers::NONE));
    let cmd = commands
        .try_recv()
        .expect("explicit Recent open sends Read");
    assert!(matches!(&cmd.op,Operation::Read{path} if path=="first.rs"));
    assert!(matches!(
        app.pending.get(&cmd.id),
        Some(crate::Job::Open { line: None, .. })
    ));
    app.apply_event(Event {
        generation: app.generation,
        id: cmd.id,
        connected: true,
        result: Ok(Payload::File {
            path: "first.rs".into(),
            text: "fresh changed contents".into(),
            revision: "r1".into(),
        }),
    });
    // Wait through modal release and native editor focus, where pinned egui
    // initializes the ordinary Open(None) cursor. The independent Browse
    // ordinary-vs-Recent control verifies this same default for changed bytes.
    for _ in 0..4 {
        frame(&mut app, vec![]);
    }
    let doc = app.active().unwrap();
    assert_ne!(doc.id, 1);
    assert_eq!(doc.text, "fresh changed contents");
    assert_eq!(
        doc.cursor,
        crate::model::cursor_location(&doc.text, doc.text.chars().count())
    );
    assert!(doc.jump_to.is_none());
    assert!(commands.try_recv().is_err());
}
#[test]
fn dirty_open_file_reuses_the_existing_document_without_a_read() {
    let (mut app, commands) = app();
    frame(&mut app, vec![]);
    app.documents[0].text = "local dirty text".into();
    show(&mut app);
    frame(&mut app, key(egui::Key::Enter, egui::Modifiers::NONE));
    frame(&mut app, vec![]);
    assert_eq!(app.active_document, Some(1));
    assert_eq!(app.documents[0].text, "local dirty text");
    assert!(commands.try_recv().is_err());
}
#[test]
fn reconnect_clear_baselines_retained_tabs_and_retired_shortcuts_cannot_open() {
    let (mut app, _) = app();
    frame(&mut app, vec![]);
    show(&mut app);
    assert!(app.recent_files.view.is_some());
    app.reset_recent_files();
    app.generation += 1;
    app.recent_connected();
    frame(&mut app, vec![]);
    assert!(app.recent_files.paths.is_empty());
    assert!(app.recent_files.view.is_none());
    frame(&mut app, vec![]);
    assert!(app.recent_files.paths.is_empty());
    app.documents.push(Document::new(
        2,
        "peer.rs".into(),
        "peer".into(),
        "p".into(),
    ));
    app.active_document = Some(2);
    frame(&mut app, vec![]);
    assert_eq!(app.recent_files.paths[0].as_ref(), "peer.rs");
}
#[test]
fn command_e_is_exact_and_preserves_mac_control_e_and_project_shortcut() {
    assert!(shortcut(egui::Key::E, egui::Modifiers::COMMAND));
    assert!(!shortcut(
        egui::Key::E,
        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT
    ));
    assert!(!shortcut(
        egui::Key::E,
        egui::Modifiers {
            ctrl: true,
            command: false,
            ..Default::default()
        }
    ));
    assert!(!shortcut(
        egui::Key::E,
        egui::Modifiers {
            ctrl: true,
            mac_cmd: true,
            command: true,
            ..Default::default()
        }
    ));
}
#[test]
fn mixed_and_repeated_global_batches_do_not_open_recent() {
    let (mut app, _) = app();
    frame(&mut app, vec![]);
    let mut mixed = key(egui::Key::E, egui::Modifiers::COMMAND);
    mixed.push(egui::Event::Paste("x".into()));
    frame(&mut app, mixed);
    assert!(app.recent_files.view.is_none());
    let mut repeat = key(egui::Key::E, egui::Modifiers::COMMAND);
    if let egui::Event::Key { repeat, .. } = &mut repeat[0] {
        *repeat = true;
    }
    frame(&mut app, repeat);
    assert!(app.recent_files.view.is_none());
    show(&mut app);
    assert!(app.recent_files.view.is_some());
}
#[test]
fn epoch_saturation_refuses_admission_without_reusing_tokens() {
    let mut recent = RecentFiles {
        epoch: u64::MAX,
        ..RecentFiles::default()
    };
    recent.touch("new.rs");
    assert!(recent.exhausted);
    assert!(recent.paths.is_empty());
}

#[test]
fn original_global_edge_survives_capture_and_cancels_a_ready_open() {
    let (mut app, commands) = app();
    frame(&mut app, vec![]);
    app.open("late.rs".into(), None);
    let command = commands.try_recv().unwrap();
    app.result_tx
        .send(crate::worker::WorkerEvent::Response(Event {
            generation: app.generation,
            id: command.id,
            connected: true,
            result: Ok(Payload::File {
                path: "late.rs".into(),
                text: "late".into(),
                revision: "r".into(),
            }),
        }))
        .unwrap();
    let ctx = app.editor_ctx.clone();
    let events = key(egui::Key::E, egui::Modifiers::COMMAND);
    let mut raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(780.0, 540.0),
        )),
        events: events.clone(),
        ..Default::default()
    };
    eframe::App::raw_input_hook(&mut app, &ctx, &mut raw);
    assert_eq!(raw.events, events);
    let mut native = eframe::Frame::_new_kittest();
    let _ = ctx.run(raw, |ctx| eframe::App::update(&mut app, ctx, &mut native));
    frame(&mut app, vec![]);
    assert_eq!(app.active_document, Some(1));
    assert!(app.documents.iter().any(|doc| doc.path == "late.rs"));
    assert!(!app
        .recent_files
        .paths
        .iter()
        .any(|path| path.as_ref() == "late.rs"));
    assert!(app.recent_files.view.is_some());
}
#[test]
fn held_other_key_and_ime_preedit_cannot_authorize_recent() {
    let (mut app, _) = app();
    frame(&mut app, vec![]);
    frame(
        &mut app,
        vec![key(egui::Key::A, egui::Modifiers::NONE).remove(0)],
    );
    frame(&mut app, key(egui::Key::E, egui::Modifiers::COMMAND));
    assert!(app.recent_files.view.is_none());
    frame(
        &mut app,
        vec![key(egui::Key::A, egui::Modifiers::NONE).remove(1)],
    );
    frame(
        &mut app,
        vec![egui::Event::Ime(egui::ImeEvent::Preedit("x".into()))],
    );
    frame(&mut app, key(egui::Key::E, egui::Modifiers::COMMAND));
    assert!(app.recent_files.view.is_none());
    frame(
        &mut app,
        vec![egui::Event::Ime(egui::ImeEvent::Commit("".into()))],
    );
    frame(&mut app, key(egui::Key::E, egui::Modifiers::COMMAND));
    assert!(app.recent_files.view.is_some());
}
#[test]
fn native_pointer_open_requires_press_release_on_the_same_current_control() {
    let (mut app, commands) = app();
    frame(&mut app, vec![]);
    app.remove_tab(1);
    frame(&mut app, vec![]);
    show(&mut app);
    let pos = crate::workspace_access_tests::recorded_rect(&app, "recent_open").center();
    let pointer = |pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    frame(
        &mut app,
        vec![egui::Event::PointerMoved(pos), pointer(false)],
    );
    assert!(commands.try_recv().is_err());
    frame(&mut app, vec![pointer(true)]);
    frame(&mut app, vec![pointer(false)]);
    assert!(matches!(commands.try_recv().unwrap().op,Operation::Read{path} if path=="first.rs"));
}
#[test]
fn clear_native_button_cancels_old_row_press_and_does_not_idle_reseed() {
    let (mut app, commands) = app();
    show(&mut app);
    let pos = crate::workspace_access_tests::recorded_rect(&app, "recent_clear").center();
    frame(
        &mut app,
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    frame(&mut app, vec![]);
    assert!(app.recent_files.paths.is_empty());
    assert!(app.recent_files.view.is_none());
    assert!(commands.try_recv().is_err());
}
#[test]
fn one_pass_requested_sizing_never_admits_an_observed_file() {
    let (mut app, _) = app();
    let ctx = app.editor_ctx.clone();
    ctx.options_mut(|options| options.max_passes = std::num::NonZeroUsize::new(1).unwrap());
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        ctx.request_discard("test a one-pass sizing boundary");
        app.finish_recent_frame(ctx);
    });
    assert!(app.recent_files.paths.is_empty());
    frame(&mut app, vec![]);
    frame(&mut app, vec![]);
    assert_eq!(app.recent_files.paths[0].as_ref(), "first.rs");
}

#[test]
fn owned_space_repeat_companion_stops_at_release_after_clear_returns_to_editor() {
    let (mut app, _) = app();
    show(&mut app);
    for _ in 0..8 {
        if crate::workspace_access_tests::recorded_response(&app, "recent_clear").has_focus() {
            break;
        }
        frame(&mut app, key(egui::Key::Tab, egui::Modifiers::NONE));
    }
    assert!(crate::workspace_access_tests::recorded_response(&app, "recent_clear").has_focus());
    let mut press = key(egui::Key::Space, egui::Modifiers::NONE);
    press.truncate(1);
    press.push(egui::Event::Text(" ".into()));
    frame(&mut app, press);
    assert!(app.recent_files.view.is_none());
    frame(&mut app, vec![]);
    frame(&mut app, vec![]);
    let before = app.active().unwrap().text.clone();
    let mut repeat = key(egui::Key::Space, egui::Modifiers::NONE);
    repeat.truncate(1);
    if let egui::Event::Key { repeat, .. } = &mut repeat[0] {
        *repeat = true;
    }
    repeat.push(egui::Event::Text(" ".into()));
    frame(&mut app, repeat);
    assert_eq!(app.active().unwrap().text, before);
    let release = key(egui::Key::Space, egui::Modifiers::NONE).remove(1);
    frame(&mut app, vec![release, egui::Event::Text(" ".into())]);
    assert_eq!(app.active().unwrap().text.len(), before.len() + 1);
}
#[test]
fn dismissing_recent_forgets_native_query_undo_and_escape_without_control_focus_works() {
    let (mut app, _) = app();
    show(&mut app);
    frame(&mut app, vec![egui::Event::Text("private-query".into())]);
    let id = egui::Id::new(QUERY);
    app.editor_ctx
        .memory_mut(|memory| memory.surrender_focus(id));
    frame(&mut app, key(egui::Key::Escape, egui::Modifiers::NONE));
    assert!(app.recent_files.view.is_none());
    show(&mut app);
    frame(&mut app, key(egui::Key::Z, egui::Modifiers::COMMAND));
    assert_eq!(app.recent_files.view.as_ref().unwrap().query, "");
}
#[test]
fn failed_closed_read_and_full_buffer_limit_do_not_replace_any_draft() {
    let (mut app, commands) = app();
    frame(&mut app, vec![]);
    app.remove_tab(1);
    frame(&mut app, vec![]);
    show(&mut app);
    frame(&mut app, key(egui::Key::Enter, egui::Modifiers::NONE));
    let command = commands.try_recv().unwrap();
    app.apply_event(Event {
        generation: app.generation,
        id: command.id,
        connected: true,
        result: Err("File no longer exists".into()),
    });
    assert!(app.documents.is_empty());
    assert!(app.error.as_deref().unwrap().contains("no longer"));
    for id in 2..34 {
        app.documents.push(Document::new(
            id,
            format!("{id}.rs"),
            "retained".into(),
            "r".into(),
        ));
    }
    app.active_document = Some(2);
    frame(&mut app, vec![]);
    show(&mut app);
    frame(&mut app, vec![egui::Event::Text("first.rs".into())]);
    frame(&mut app, key(egui::Key::Enter, egui::Modifiers::NONE));
    assert_eq!(app.documents.len(), 32);
    assert!(app.documents.iter().all(|doc| doc.text == "retained"));
    assert!(commands.try_recv().is_err());
}

#[test]
fn pointer_held_key_and_stale_origin_version_cannot_open_or_clear() {
    for stale_version in [false, true] {
        let (mut app, commands) = app();
        show(&mut app);
        let pos = crate::workspace_access_tests::recorded_rect(&app, "recent_clear").center();
        if stale_version {
            app.documents[0].edit_version += 1;
        } else {
            frame(
                &mut app,
                vec![key(egui::Key::A, egui::Modifiers::NONE).remove(0)],
            );
        }
        frame(
            &mut app,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        assert_eq!(app.recent_files.paths.len(), 1);
        assert!(commands.try_recv().is_err());
    }
}
#[test]
fn pointer_click_accepts_only_noncomposing_ime_lifecycle_companions() {
    for lifecycle in [egui::ImeEvent::Enabled, egui::ImeEvent::Disabled] {
        let (mut app, commands) = app();
        show(&mut app);
        let pos = crate::workspace_access_tests::recorded_rect(&app, "recent_clear").center();
        frame(
            &mut app,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::Ime(lifecycle),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        assert!(app.recent_files.paths.is_empty());
        assert!(commands.try_recv().is_err());
    }
}
