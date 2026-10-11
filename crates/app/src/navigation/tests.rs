use super::*;
use crate::{
    editor_state,
    worker::{Command, Event, Worker},
    ConnectForm, ConnectionState, Job,
};
use cedar_protocol::{Operation, Payload};
use std::sync::mpsc::Receiver;

fn entry(path: &str, is_dir: bool) -> Entry {
    Entry {
        path: path.into(),
        name: path.rsplit('/').next().unwrap().into(),
        is_dir,
    }
}

fn app() -> (CedarApp, Receiver<Command>) {
    let mut app = CedarApp::empty();
    let form = ConnectForm {
        local_root: "/workspace".into(),
        allow_run: false,
        ..Default::default()
    };
    app.workspace_key = Some(form.key());
    app.active_form = Some(form);
    app.root = "/workspace".into();
    app.state = ConnectionState::Ready;
    app.open_form = false;
    app.documents.push(Document::new(
        1,
        "first.rs".into(),
        "é🐻\r\nsecond\nlast\n".into(),
        "r0".into(),
    ));
    app.active_document = Some(1);
    app.next_document = 2;
    let (worker, commands) = Worker::recording();
    app.worker = Some(worker);
    (app, commands)
}

fn frame(app: &mut CedarApp, time: f64, events: Vec<egui::Event>) {
    let ctx = app.editor_ctx.clone();
    let mut frame = eframe::Frame::_new_kittest();
    let output = ctx.run(
        egui::RawInput {
            time: Some(time),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(780.0, 540.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| eframe::App::update(app, ctx, &mut frame),
    );
    assert!(output
        .shapes
        .iter()
        .all(|shape| shape.clip_rect.is_finite()));
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

fn file_selection(app: &CedarApp) -> Option<&str> {
    match &app.navigation.dialog.as_ref().unwrap().kind {
        Kind::Files { selected, .. } => selected.as_deref(),
        _ => panic!("expected file chooser"),
    }
}

fn reply(app: &mut CedarApp, command: Command, path: &str) {
    app.apply_event(Event {
        generation: app.generation,
        id: command.id,
        connected: true,
        result: Ok(Payload::File {
            path: path.into(),
            text: "disk contents\nsecond line".into(),
            revision: "disk-revision".into(),
        }),
    });
}

#[test]
fn candidates_are_bounded_open_first_literal_deduplicated_and_deterministic() {
    let documents = vec![
        Document::new(1, "z.rs".into(), "draft".into(), "r".into()),
        Document::new(2, "中文\\é.rs ".into(), "draft".into(), "r".into()),
    ];
    let mut entries = vec![entry("z.rs", false), entry("folder", true)];
    entries.extend(
        (0..80)
            .rev()
            .map(|index| entry(&format!("{index:02}.rs"), false)),
    );
    let result = candidates(&documents, &entries, "");
    assert_eq!(result.items.len(), MAX_RESULTS);
    assert!(result.truncated);
    assert_eq!(result.items[0].path.as_ref(), "z.rs");
    assert_eq!(result.items[1].path.as_ref(), "中文\\é.rs ");
    assert_eq!(result.items[2].path.as_ref(), "00.rs");
    assert!(result.items[..2].iter().all(|item| item.open));
    assert!(result.items[2..].iter().all(|item| !item.open));
    let result = candidates(&documents, &entries, "中文\\É");
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.items[0].path.as_ref(), "中文\\é.rs ");
    assert!(!result.truncated);
    assert!(candidates(&documents, &entries, "folder").items.is_empty());
}

#[test]
fn line_validation_counts_empty_trailing_and_crlf_lines_and_rejects_overflow() {
    for (text, count) in [("", 1), ("one", 1), ("é🐻\r\nlast\n", 3)] {
        assert_eq!(line_count(text), count);
        assert_eq!(parse_line(&count.to_string(), text), Ok(count));
        for input in ["", "0", "-1", "+1", "1.5", "1:2", "18446744073709551616000"] {
            assert!(parse_line(input, text).is_err(), "accepted {input}");
        }
        assert!(parse_line(&(count + 1).to_string(), text).is_err());
    }
    assert_eq!(parse_line(" 2 ", "é🐻\r\nlast\n"), Ok(2));
    assert_eq!(crate::model::line_start("é🐻\r\nlast\n", 2), 4);
    assert_eq!(typed_path(" src\\main.rs "), "src/main.rs");
}

#[test]
fn opening_typing_repeating_and_dismissing_never_reaches_editor() {
    let (mut app, commands) = app();
    app.entries = vec![entry("second.rs", false), entry("second_test.rs", false)];
    frame(&mut app, 0.0, vec![]);
    app.editor_ctx
        .memory_mut(|memory| memory.request_focus(egui::Id::new(("editor", 1u64))));
    let before = app.documents[0].text.clone();
    let mut events = key(egui::Key::P, egui::Modifiers::COMMAND);
    events.push(egui::Event::Text("second".into()));
    frame(&mut app, 1.0, events);
    assert_eq!(app.navigation.query, "second");
    assert_eq!(app.documents[0].text, before);
    assert_eq!(file_selection(&app), Some("second.rs"));
    frame(
        &mut app,
        2.0,
        key(egui::Key::ArrowDown, egui::Modifiers::NONE),
    );
    assert_eq!(file_selection(&app), Some("second_test.rs"));
    let epoch = app.navigation_epoch;
    frame(&mut app, 3.0, key(egui::Key::P, egui::Modifiers::COMMAND));
    assert_eq!(app.navigation_epoch, epoch);
    assert_eq!(file_selection(&app), Some("second_test.rs"));
    assert_eq!(app.navigation.query, "second");
    let mut events = key(egui::Key::Escape, egui::Modifiers::NONE);
    events.push(egui::Event::Paste("must stay out of source".into()));
    frame(&mut app, 4.0, events);
    assert!(app.navigation.dialog.is_none());
    assert_eq!(app.documents[0].text, before);
    assert_eq!(app.documents[0].edit_version, 0);
    frame(&mut app, 5.0, vec![]);
    frame(&mut app, 5.5, vec![]);
    assert!(app
        .editor_ctx
        .memory(|memory| memory.has_focus(egui::Id::new(("editor", 1u64)))));
    frame(&mut app, 6.0, vec![egui::Event::Text("resumed".into())]);
    assert!(app.documents[0].text.contains("resumed"));
    assert!(commands.try_recv().is_err());
}

#[test]
fn changing_query_or_directory_cannot_leave_a_stale_selected_row() {
    let (mut app, _) = app();
    app.entries = vec![entry("aa.rs", false), entry("bb.rs", false)];
    app.navigation.query = ".rs".into();
    app.show_file_chooser();
    frame(&mut app, 0.0, vec![]);
    frame(
        &mut app,
        1.0,
        key(egui::Key::ArrowDown, egui::Modifiers::NONE),
    );
    assert_eq!(file_selection(&app), Some("aa.rs"));
    assert!(app
        .editor_ctx
        .memory(|memory| memory.has_focus(egui::Id::new(FILE_INPUT))));
    app.entries = vec![entry("cc.rs", false)];
    frame(&mut app, 2.0, vec![]);
    assert_eq!(file_selection(&app), Some("first.rs"));
    let mut events = key(egui::Key::A, egui::Modifiers::COMMAND);
    events.push(egui::Event::Text("cc".into()));
    frame(&mut app, 3.0, events);
    assert_eq!(app.navigation.query, "cc");
    assert_eq!(file_selection(&app), Some("cc.rs"));
    frame(
        &mut app,
        4.0,
        key(egui::Key::ArrowDown, egui::Modifiers::NONE),
    );
    assert_eq!(file_selection(&app), Some("cc.rs"));
}

#[test]
fn selection_sends_literal_path_but_explicit_path_keeps_old_normalization() {
    for explicit in [false, true] {
        let (mut app, commands) = app();
        app.entries = vec![entry(" 中文\\é.rs ", false)];
        app.navigation.query = " 中文\\é.rs ".into();
        app.show_file_chooser();
        frame(&mut app, 0.0, vec![]);
        frame(
            &mut app,
            1.0,
            key(
                egui::Key::Enter,
                if explicit {
                    egui::Modifiers::COMMAND
                } else {
                    egui::Modifiers::NONE
                },
            ),
        );
        let command = commands.try_recv().unwrap();
        let expected = if explicit {
            "中文/é.rs"
        } else {
            " 中文\\é.rs "
        };
        assert!(matches!(command.op, Operation::Read { path } if path == expected));
        assert!(app.navigation.dialog.is_none());
        assert_eq!(app.documents.len(), 1);
        assert_eq!(app.documents[0].edit_version, 0);
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn choosing_dirty_open_buffer_offline_preserves_draft_and_restores_focus() {
    let (mut app, commands) = app();
    let mut second = Document::new(2, "other.rs".into(), "baseline".into(), "r1".into());
    second.text = "precious dirty draft".into();
    second.edit_version = 9;
    app.documents.push(second);
    app.state = ConnectionState::Disconnected;
    app.navigation.query = "other".into();
    app.show_file_chooser();
    frame(&mut app, 0.0, vec![]);
    frame(&mut app, 1.0, key(egui::Key::Enter, egui::Modifiers::NONE));
    assert_eq!(app.active_document, Some(2));
    assert!(app.navigation.dialog.is_none());
    frame(&mut app, 2.0, vec![]);
    frame(&mut app, 2.5, vec![]);
    let doc = &app.documents[1];
    assert_eq!(doc.text, "precious dirty draft");
    assert_eq!(doc.saved_text, "baseline");
    assert_eq!(doc.revision.as_deref(), Some("r1"));
    assert_eq!(doc.edit_version, 9);
    assert!(app
        .editor_ctx
        .memory(|memory| memory.has_focus(egui::Id::new(("editor", 2u64)))));
    assert!(commands.try_recv().is_err());
    assert_eq!(app.location_history.back.len(), 1);
    assert_eq!(app.location_history.back[0].document, 1);
    assert!(app.location_history.pending.is_none());
}

#[test]
fn line_dialog_switch_repeat_validation_and_jump_preserve_source() {
    let (mut app, commands) = app();
    let before = app.documents[0].text.clone();
    frame(&mut app, 0.0, key(egui::Key::P, egui::Modifiers::COMMAND));
    let mut events = key(egui::Key::G, egui::Modifiers::COMMAND);
    events.push(egui::Event::Text("999".into()));
    frame(&mut app, 1.0, events);
    frame(&mut app, 2.0, key(egui::Key::Enter, egui::Modifiers::NONE));
    assert!(matches!(
        &app.navigation.dialog.as_ref().unwrap().kind,
        Kind::Line { error: Some(_), .. }
    ));
    let epoch = app.navigation_epoch;
    let mut events = key(egui::Key::G, egui::Modifiers::COMMAND);
    events.push(egui::Event::Text("2".into()));
    frame(&mut app, 3.0, events);
    assert_eq!(app.navigation_epoch, epoch);
    frame(&mut app, 4.0, key(egui::Key::Enter, egui::Modifiers::NONE));
    assert!(app.navigation.dialog.is_none());
    assert_eq!(app.documents[0].jump_to, Some(4));
    frame(&mut app, 5.0, vec![]);
    frame(&mut app, 5.5, vec![]);
    assert_eq!(app.documents[0].cursor, (2, 1));
    assert_eq!(app.documents[0].text, before);
    assert_eq!(app.documents[0].saved_text, before);
    assert_eq!(app.documents[0].revision.as_deref(), Some("r0"));
    assert_eq!(app.documents[0].edit_version, 0);
    assert!(commands.try_recv().is_err());
}

#[test]
fn accept_uses_only_input_before_enter_and_discards_later_text_or_paste() {
    for suffix in [
        egui::Event::Text("3".into()),
        egui::Event::Paste("3".into()),
    ] {
        let (mut app, commands) = app();
        frame(&mut app, 0.0, key(egui::Key::G, egui::Modifiers::COMMAND));
        let mut events = vec![egui::Event::Text("2".into())];
        events.extend(key(egui::Key::Enter, egui::Modifiers::NONE));
        events.push(suffix);
        frame(&mut app, 1.0, events);
        assert!(app.navigation.dialog.is_none());
        assert_eq!(app.documents[0].jump_to, Some(4));
        frame(&mut app, 2.0, vec![]);
        frame(&mut app, 3.0, vec![]);
        assert_eq!(app.documents[0].cursor, (2, 1));
        assert_eq!(app.documents[0].edit_version, 0);
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn chooser_and_line_pastes_have_finite_limits() {
    let (mut app, _) = app();
    let mut events = key(egui::Key::P, egui::Modifiers::COMMAND);
    events.push(egui::Event::Paste("é".repeat(MAX_QUERY_CHARS + 1)));
    frame(&mut app, 0.0, events);
    assert_eq!(app.navigation.query.chars().count(), MAX_QUERY_CHARS);
    let mut events = key(egui::Key::G, egui::Modifiers::COMMAND);
    events.push(egui::Event::Paste("9".repeat(100)));
    frame(&mut app, 1.0, events);
    assert!(
        matches!(&app.navigation.dialog.as_ref().unwrap().kind, Kind::Line { input, .. } if input.len() == 20)
    );
}

#[test]
fn single_pass_modal_replays_opening_input_once_and_drops_it_when_identity_changes() {
    for invalidate in [false, true] {
        let (mut app, commands) = app();
        app.editor_ctx
            .options_mut(|options| options.max_passes = 1.try_into().unwrap());
        let mut events = key(egui::Key::G, egui::Modifiers::COMMAND);
        events.push(egui::Event::Text("2".into()));
        events.extend(key(egui::Key::Enter, egui::Modifiers::NONE));
        frame(&mut app, 0.0, events);
        assert!(app.navigation.dialog.is_some());
        assert!(app.documents[0].jump_to.is_none());
        if invalidate {
            app.generation += 1;
        }
        frame(&mut app, 1.0, vec![]);
        assert!(app.navigation.dialog.is_none());
        assert_eq!(
            app.documents[0].jump_to,
            if invalidate { None } else { Some(4) }
        );
        frame(&mut app, 2.0, vec![]);
        frame(&mut app, 3.0, vec![]);
        assert_eq!(app.documents[0].edit_version, 0);
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn discard_confirmation_dismisses_navigation_and_owns_its_keyboard_input() {
    let (mut app, commands) = app();
    app.entries = vec![entry("new.rs", false)];
    app.navigation.query = "new.rs".into();
    frame(&mut app, 0.0, key(egui::Key::P, egui::Modifiers::COMMAND));
    app.confirm = Some(crate::Confirm::CloseWindow);
    let mut events = key(egui::Key::Enter, egui::Modifiers::NONE);
    events.push(egui::Event::Text("queued text".into()));
    frame(&mut app, 1.0, events);
    assert!(app.navigation.dialog.is_none());
    assert!(matches!(app.confirm, Some(crate::Confirm::CloseWindow)));
    assert_eq!(app.active_document, Some(1));
    assert_eq!(app.documents[0].edit_version, 0);
    assert!(commands.try_recv().is_err());
    frame(&mut app, 2.0, key(egui::Key::P, egui::Modifiers::COMMAND));
    assert!(app.navigation.dialog.is_none());
    app.confirm = None;
    frame(&mut app, 3.0, vec![]);
    frame(&mut app, 4.0, vec![]);
    assert!(app
        .editor_ctx
        .memory(|memory| memory.has_focus(egui::Id::new(("editor", 1u64)))));
    frame(&mut app, 5.0, vec![egui::Event::Text("resumed".into())]);
    assert!(app.documents[0].text.contains("resumed"));
}

#[test]
fn late_file_read_cannot_steal_selection_or_go_to_line_cursor() {
    for line in [false, true] {
        let (mut app, commands) = app();
        app.documents[0].jump_to = Some(0);
        frame(&mut app, -1.0, vec![]);
        let original_cursor = app.documents[0].cursor;
        app.open("late.rs".into(), Some(2));
        let command = commands.try_recv().unwrap();
        frame(
            &mut app,
            0.0,
            key(
                if line { egui::Key::G } else { egui::Key::P },
                egui::Modifiers::COMMAND,
            ),
        );
        if line {
            frame(&mut app, 1.0, vec![egui::Event::Text("3".into())]);
        }
        frame(&mut app, 2.0, key(egui::Key::Enter, egui::Modifiers::NONE));
        reply(&mut app, command, "late.rs");
        frame(&mut app, 3.0, vec![]);
        frame(&mut app, 3.5, vec![]);
        assert_eq!(app.active_document, Some(1));
        assert_eq!(app.documents.len(), 2);
        assert_eq!(
            app.documents[0].cursor,
            if line { (3, 1) } else { original_cursor }
        );
        assert!(app
            .editor_ctx
            .memory(|memory| memory.has_focus(egui::Id::new(("editor", 1u64)))));
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn active_document_or_generation_change_dismisses_line_dialog_and_swallow_queued_input() {
    for generation in [false, true] {
        let (mut app, commands) = app();
        frame(&mut app, 0.0, key(egui::Key::G, egui::Modifiers::COMMAND));
        if generation {
            app.generation += 1;
        } else {
            app.documents.push(Document::new(
                2,
                "replacement.rs".into(),
                "replacement".into(),
                "r".into(),
            ));
            app.active_document = Some(2);
        }
        let mut events = vec![egui::Event::Text("2".into())];
        events.extend(key(egui::Key::Enter, egui::Modifiers::NONE));
        frame(&mut app, 1.0, events);
        assert!(app.navigation.dialog.is_none());
        assert!(app
            .documents
            .iter()
            .all(|doc| doc.edit_version == 0 && doc.jump_to.is_none()));
        assert_eq!(app.documents[0].cursor, (1, 1));
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn chooser_reuses_pending_reads_and_never_exceeds_thirty_two_buffers() {
    let (mut app, commands) = app();
    app.open("pending.rs".into(), None);
    let command = commands.try_recv().unwrap();
    app.navigation.query = "pending.rs".into();
    app.entries = vec![entry("pending.rs", false)];
    app.show_file_chooser();
    frame(&mut app, 0.0, vec![]);
    frame(&mut app, 1.0, key(egui::Key::Enter, egui::Modifiers::NONE));
    assert!(app.navigation.dialog.is_none());
    assert!(commands.try_recv().is_err());
    assert!(
        matches!(app.pending.get(&command.id), Some(Job::Open { navigation, .. }) if *navigation == app.navigation_epoch)
    );
    for id in 2..=32 {
        app.documents.push(Document::new(
            id,
            format!("{id}.rs"),
            String::new(),
            "r".into(),
        ));
    }
    reply(&mut app, command, "pending.rs");
    assert_eq!(app.documents.len(), 32);
    assert!(app
        .error
        .as_ref()
        .is_some_and(|error| error.contains("32-buffer")));
    app.show_file_chooser();
    frame(&mut app, 2.0, vec![]);
    frame(&mut app, 3.0, key(egui::Key::Enter, egui::Modifiers::NONE));
    assert!(app.navigation.dialog.is_none());
    assert!(commands.try_recv().is_err());
    assert_eq!(app.documents.len(), 32);
}

#[test]
fn repeated_line_jumps_after_partial_undo_preserve_complete_redo_and_saved_baseline() {
    let (mut app, commands) = app();
    let baseline = app.documents[0].text.clone();
    frame(&mut app, 0.0, vec![]);
    let ctx = app.editor_ctx.clone();
    editor_state::commit(
        &ctx,
        &mut app.documents[0],
        "first edit\nsecond\n".into(),
        0,
    );
    frame(&mut app, 1.0, vec![]);
    editor_state::commit(
        &ctx,
        &mut app.documents[0],
        "second edit\nsecond\n".into(),
        0,
    );
    frame(&mut app, 2.0, vec![]);
    frame(&mut app, 3.0, key(egui::Key::Z, egui::Modifiers::COMMAND));
    assert_eq!(app.documents[0].text, "first edit\nsecond\n");
    let edit_version = app.documents[0].edit_version;
    for index in 0..8 {
        let time = 4.0 + f64::from(index) * 4.0;
        let mut events = key(egui::Key::G, egui::Modifiers::COMMAND);
        events.push(egui::Event::Text(
            if index % 2 == 0 { "2" } else { "3" }.into(),
        ));
        frame(&mut app, time, events);
        frame(
            &mut app,
            time + 1.0,
            key(egui::Key::Enter, egui::Modifiers::NONE),
        );
        frame(&mut app, time + 2.0, vec![]);
        frame(&mut app, time + 2.5, vec![]);
        assert_eq!(app.documents[0].edit_version, edit_version);
    }
    frame(
        &mut app,
        40.0,
        key(
            egui::Key::Z,
            egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
        ),
    );
    assert_eq!(app.documents[0].text, "second edit\nsecond\n");
    frame(&mut app, 41.0, key(egui::Key::Z, egui::Modifiers::COMMAND));
    assert_eq!(app.documents[0].text, "first edit\nsecond\n");
    frame(&mut app, 42.0, key(egui::Key::Z, egui::Modifiers::COMMAND));
    assert_eq!(app.documents[0].text, baseline);
    assert_eq!(app.documents[0].saved_text, baseline);
    assert_eq!(app.documents[0].revision.as_deref(), Some("r0"));
    assert!(commands.try_recv().is_err());
}

#[test]
fn chooser_read_ready_on_closing_modal_idle_frame_completes_and_admits() {
    let (mut app, commands) = app();
    app.navigation.query = "next.rs".into();
    app.show_file_chooser();
    frame(&mut app, 0.0, vec![]);
    frame(
        &mut app,
        1.0,
        key(egui::Key::Enter, egui::Modifiers::COMMAND),
    );
    let read = commands.try_recv().unwrap();
    assert!(app.navigation.dialog.is_none());
    app.result_tx
        .send(crate::worker::WorkerEvent::Response(Event {
            generation: app.generation,
            id: read.id,
            connected: true,
            result: Ok(Payload::File {
                path: "next.rs".into(),
                text: "next".into(),
                revision: "r".into(),
            }),
        }))
        .unwrap();
    frame(&mut app, 2.0, vec![]);
    frame(&mut app, 3.0, vec![]);
    assert_eq!(app.active().unwrap().path, "next.rs");
    assert_eq!(app.location_history.back.len(), 1);
    assert!(app.location_history.pending.is_none());
}

fn list_reply(app: &mut CedarApp, command: Command, entries: Vec<Entry>) {
    app.apply_event(Event {
        generation: app.generation,
        id: command.id,
        connected: true,
        result: Ok(Payload::Entries { entries }),
    });
}

fn admit_flat(app: &mut CedarApp, commands: &Receiver<Command>, entries: Vec<Entry>) {
    app.list(String::new());
    list_reply(app, commands.try_recv().unwrap(), entries);
}

fn admit_tree(app: &mut CedarApp, commands: &Receiver<Command>) {
    app.explorer_set_mode(Mode::Tree);
    app.explorer_expand("");
    list_reply(
        app,
        commands.try_recv().unwrap(),
        vec![entry("left", true), entry("right", true)],
    );
    for path in ["left", "right"] {
        app.explorer_expand(path);
        list_reply(
            app,
            commands.try_recv().unwrap(),
            vec![entry(&format!("{path}/雪.rs"), false)],
        );
    }
}

fn enter_loaded(app: &mut CedarApp, time: f64) {
    frame(app, time, key(egui::Key::Tab, egui::Modifiers::SHIFT));
    // egui deliberately applies backward Tab focus on the next pass.
    frame(app, time + 0.01, vec![]);
    assert!(app
        .editor_ctx
        .memory(|memory| memory.has_focus(egui::Id::new("navigation_scope_loaded"))));
    let before = app.navigation.query.clone();
    let mut events = key(egui::Key::Space, egui::Modifiers::NONE);
    events.push(egui::Event::Text(" ".into()));
    frame(app, time + 0.1, events);
    assert_eq!(app.navigation.query, before);
    assert!(app.navigation.loaded_scope());
    assert!(app
        .editor_ctx
        .memory(|memory| memory.has_focus(egui::Id::new(FILE_INPUT))));
}

fn chooser_error(app: &CedarApp) -> Option<&str> {
    match &app.navigation.dialog.as_ref().unwrap().kind {
        Kind::Files { error, .. } => error.as_deref(),
        _ => panic!("expected file chooser"),
    }
}

fn pointer(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

#[test]
fn loaded_snapshot_has_checked_whole_source_budgets_and_shared_exact_paths() {
    let documents = vec![Document::new(
        1,
        "雪/é.rs ".into(),
        String::new(),
        "r".into(),
    )];
    let files = vec![
        LoadedFile {
            path: "z.rs",
            admission: FileAdmission(1),
        },
        LoadedFile {
            path: "雪/é.rs ",
            admission: FileAdmission(2),
        },
        LoadedFile {
            path: "A.rs",
            admission: FileAdmission(3),
        },
        LoadedFile {
            path: "a.rs",
            admission: FileAdmission(4),
        },
    ];
    let snapshot = loaded_snapshot(&documents, &files).unwrap();
    assert_eq!(
        snapshot
            .iter()
            .map(|item| item.path.as_ref())
            .collect::<Vec<_>>(),
        ["雪/é.rs ", "A.rs", "a.rs", "z.rs"]
    );
    assert_eq!(snapshot[0].source, Source::Buffer(1));
    let result = snapshot_matches(&snapshot, "雪/É");
    assert_eq!(result.items.len(), 1);
    assert!(Arc::ptr_eq(&result.items[0].path, &snapshot[0].path));
    let maximum: Vec<_> = (0..MAX_CACHED_PATHS)
        .map(|_| LoadedFile {
            path: "duplicate",
            admission: FileAdmission(1),
        })
        .collect();
    assert_eq!(loaded_snapshot(&[], &maximum).unwrap().len(), 1);
    let mut excessive = maximum;
    excessive.push(LoadedFile {
        path: "one more",
        admission: FileAdmission(1),
    });
    assert!(loaded_snapshot(&[], &excessive)
        .unwrap_err()
        .contains("whole snapshot"));
    let huge = "é".repeat(MAX_SNAPSHOT_PATH_BYTES / 2);
    let maximum = [LoadedFile {
        path: &huge,
        admission: FileAdmission(1),
    }];
    assert!(loaded_snapshot(&[], &maximum).is_ok());
    assert!(loaded_snapshot(&documents, &maximum).is_err());
    let too_many: Vec<_> = (0..=MAX_BUFFERS)
        .map(|id| Document::new(id as u64, format!("{id}"), String::new(), "r".into()))
        .collect();
    assert!(loaded_snapshot(&too_many, &[]).is_err());
    let paths: Vec<_> = (0..100).map(|index| format!("{index:03}.rs")).collect();
    let files: Vec<_> = paths
        .iter()
        .map(|path| LoadedFile {
            path,
            admission: FileAdmission(1),
        })
        .collect();
    let snapshot = loaded_snapshot(&[], &files).unwrap();
    let result = snapshot_matches(&snapshot, ".rs");
    assert_eq!(result.items.len(), 64);
    assert!(result.truncated);
    assert_eq!(
        snapshot_matches(&snapshot, "099").items[0].path.as_ref(),
        "099.rs"
    );
}

#[test]
fn loaded_scope_is_keyboard_accessible_local_and_defaults_current_on_new_dialog() {
    let (mut app, commands) = app();
    admit_tree(&mut app, &commands);
    frame(&mut app, 0.0, key(egui::Key::P, egui::Modifiers::COMMAND));
    assert!(!app.navigation.loaded_scope());
    assert_eq!(app.navigation.visible_paths(), ["first.rs"]);
    let history = app.location_history.back.len();
    enter_loaded(&mut app, 1.0);
    assert_eq!(
        app.navigation.visible_paths(),
        ["first.rs", "left/雪.rs", "right/雪.rs"]
    );
    frame(&mut app, 2.0, key(egui::Key::P, egui::Modifiers::COMMAND));
    assert!(app.navigation.loaded_scope());
    assert_eq!(app.location_history.back.len(), history);
    frame(&mut app, 3.0, key(egui::Key::Escape, egui::Modifiers::NONE));
    frame(&mut app, 4.0, key(egui::Key::P, egui::Modifiers::COMMAND));
    assert!(!app.navigation.loaded_scope());
    assert_eq!(app.location_history.back.len(), history);
    assert!(commands.try_recv().is_err());
}

#[test]
fn loaded_flat_requires_current_admission_not_retained_display_rows() {
    let (mut app, commands) = app();
    app.entries = vec![entry("never-admitted.rs", false)];
    assert!(app.explorer_loaded_files().unwrap().is_empty());
    assert_eq!(app.explorer_loaded_directory_count(), 0);
    admit_flat(&mut app, &commands, vec![]);
    assert!(app.explorer_loaded_files().unwrap().is_empty());
    assert_eq!(app.explorer_loaded_directory_count(), 1);
    admit_flat(&mut app, &commands, vec![entry("accepted.rs", false)]);
    assert_eq!(app.explorer_loaded_files().unwrap()[0].path, "accepted.rs");
    app.explorer_set_mode(Mode::Tree);
    app.explorer_set_mode(Mode::Flat);
    assert!(!app.entries.is_empty());
    assert!(app.explorer_loaded_files().unwrap().is_empty());
    assert_eq!(app.explorer_loaded_directory_count(), 0);
    admit_flat(&mut app, &commands, vec![entry("new.rs", false)]);
    app.explorer.reset_connection();
    assert!(!app.entries.is_empty());
    assert!(app.explorer_loaded_files().unwrap().is_empty());
    assert_eq!(app.explorer_loaded_directory_count(), 0);
    assert!(commands.try_recv().is_err());
}

#[test]
fn loaded_snapshot_stays_frozen_and_enter_never_uses_unmatched_filter_as_path() {
    let (mut app, commands) = app();
    admit_flat(&mut app, &commands, vec![entry("before.rs", false)]);
    app.navigation.query = "after".into();
    app.show_file_chooser();
    frame(&mut app, 0.0, vec![]);
    enter_loaded(&mut app, 1.0);
    assert!(app.navigation.visible_paths().is_empty());
    admit_flat(&mut app, &commands, vec![entry("after.rs", false)]);
    frame(&mut app, 2.0, key(egui::Key::Enter, egui::Modifiers::NONE));
    assert!(app.navigation.visible_paths().is_empty());
    assert!(app.navigation.dialog_open());
    assert!(commands.try_recv().is_err());
    frame(
        &mut app,
        3.0,
        key(egui::Key::Enter, egui::Modifiers::COMMAND),
    );
    assert!(matches!(commands.try_recv().unwrap().op, Operation::Read { path } if path == "after"));
}

#[test]
fn loaded_selection_revalidates_collapse_refresh_retype_and_document_identity() {
    for action in ["collapse", "refresh", "retype", "closed-buffer"] {
        let (mut app, commands) = app();
        admit_tree(&mut app, &commands);
        if action == "closed-buffer" {
            app.documents.push(Document::new(
                2,
                "left/雪.rs".into(),
                "dirty".into(),
                "r".into(),
            ));
        }
        app.navigation.query = "left/".into();
        app.show_file_chooser();
        frame(&mut app, 0.0, vec![]);
        enter_loaded(&mut app, 1.0);
        assert_eq!(file_selection(&app), Some("left/雪.rs"));
        match action {
            "collapse" => app.explorer_collapse("left"),
            "refresh" | "retype" => {
                app.explorer_refresh("left");
                list_reply(
                    &mut app,
                    commands.try_recv().unwrap(),
                    vec![entry("left/雪.rs", action == "retype")],
                );
            }
            _ => {
                app.documents.retain(|doc| doc.id != 2);
                app.documents.push(Document::new(
                    3,
                    "left/雪.rs".into(),
                    "replacement".into(),
                    "r".into(),
                ));
            }
        }
        frame(&mut app, 2.0, key(egui::Key::Enter, egui::Modifiers::NONE));
        assert!(app.navigation.dialog_open(), "{action}");
        assert!(
            chooser_error(&app).unwrap().contains("canceled"),
            "{action}"
        );
        assert_eq!(app.active_document, Some(1));
        assert!(commands.try_recv().is_err());
        assert!(app.location_history.back.is_empty());
    }
}

#[test]
fn stale_admitted_listing_is_selectable_but_new_success_has_a_new_identity() {
    let (mut app, commands) = app();
    admit_flat(&mut app, &commands, vec![entry("stale.rs", false)]);
    let original = app.explorer_loaded_files().unwrap()[0].admission;
    app.list(String::new());
    let refresh = commands.try_recv().unwrap();
    app.apply_event(Event {
        generation: app.generation,
        id: refresh.id,
        connected: true,
        result: Err("refresh failed".into()),
    });
    let files = app.explorer_loaded_files().unwrap();
    assert_eq!(
        app.explorer_loaded_file_stale(files[0].path, files[0].admission),
        Some(true)
    );
    assert_eq!(files[0].admission, original);
    app.navigation.query = "stale.rs".into();
    app.show_file_chooser();
    frame(&mut app, 0.0, vec![]);
    enter_loaded(&mut app, 1.0);
    frame(&mut app, 2.0, key(egui::Key::Enter, egui::Modifiers::NONE));
    assert!(
        matches!(commands.try_recv().unwrap().op, Operation::Read { path } if path == "stale.rs")
    );
    assert!(commands.try_recv().is_err());
}

#[test]
fn current_enter_uses_displayed_identity_when_listing_changes_in_acceptance_frame() {
    let (mut app, commands) = app();
    app.entries = vec![entry("old.rs", false)];
    app.navigation.query = ".rs".into();
    app.show_file_chooser();
    frame(&mut app, 0.0, vec![]);
    frame(
        &mut app,
        1.0,
        key(egui::Key::ArrowDown, egui::Modifiers::NONE),
    );
    assert_eq!(file_selection(&app), Some("old.rs"));
    app.entries = vec![entry("replacement.rs", false)];
    frame(&mut app, 2.0, key(egui::Key::Enter, egui::Modifiers::NONE));
    assert!(chooser_error(&app).unwrap().contains("canceled"));
    assert_eq!(app.active_document, Some(1));
    assert!(commands.try_recv().is_err());
}

#[test]
fn captured_click_removed_row_cancels_visibly_without_retargeting() {
    let (mut app, commands) = app();
    app.entries = vec![entry("old.rs", false)];
    app.navigation.query = "old".into();
    app.show_file_chooser();
    frame(&mut app, 0.0, vec![]);
    frame(&mut app, 0.1, vec![]);
    let pos = crate::workspace_access_tests::recorded_rect(&app, "navigation_file:old.rs").center();
    frame(
        &mut app,
        1.0,
        vec![egui::Event::PointerMoved(pos), pointer(pos, true)],
    );
    app.entries = vec![entry("old-replacement.rs", false)];
    frame(&mut app, 1.1, vec![pointer(pos, false)]);
    assert!(chooser_error(&app).unwrap().contains("canceled"));
    assert_eq!(app.active_document, Some(1));
    assert!(commands.try_recv().is_err());
}

#[test]
fn captured_pointer_requires_real_click_and_yields_to_competing_input() {
    for action in ["drag", "text", "explicit", "focus"] {
        let (mut app, commands) = app();
        app.entries = vec![entry("old.rs", false)];
        app.navigation.query = "old".into();
        app.show_file_chooser();
        frame(&mut app, 0.0, vec![]);
        frame(&mut app, 0.1, vec![]);
        let pos =
            crate::workspace_access_tests::recorded_rect(&app, "navigation_file:old.rs").center();
        frame(
            &mut app,
            1.0,
            vec![egui::Event::PointerMoved(pos), pointer(pos, true)],
        );
        if action == "drag" {
            frame(
                &mut app,
                1.05,
                vec![egui::Event::PointerMoved(pos + egui::vec2(100.0, 0.0))],
            );
        }
        let mut events = vec![egui::Event::PointerMoved(pos), pointer(pos, false)];
        match action {
            "text" => events.push(egui::Event::Text("new".into())),
            "explicit" => events.extend(key(egui::Key::Enter, egui::Modifiers::COMMAND)),
            "focus" => events.push(egui::Event::WindowFocused(false)),
            _ => {}
        }
        frame(&mut app, 1.1, events);
        if action == "explicit" {
            assert!(
                matches!(commands.try_recv().unwrap().op, Operation::Read { path } if path == "old")
            );
        } else {
            assert!(app.navigation.dialog_open(), "{action}");
        }
        assert_eq!(app.active_document, Some(1));
        assert!(commands.try_recv().is_err(), "{action}");
    }
}

#[test]
fn scope_enter_activation_preserves_query_and_refusal_is_visible_until_scope_exit() {
    let (mut app, commands) = app();
    app.navigation.query = "unchanged".into();
    app.documents[0].path = "é".repeat(MAX_SNAPSHOT_PATH_BYTES / 2 + 1);
    app.show_file_chooser();
    frame(&mut app, 0.0, vec![]);
    frame(&mut app, 1.0, key(egui::Key::Tab, egui::Modifiers::SHIFT));
    frame(&mut app, 1.1, vec![]);
    frame(&mut app, 2.0, key(egui::Key::Enter, egui::Modifiers::NONE));
    assert!(app.navigation.loaded_scope());
    assert_eq!(app.navigation.query, "unchanged");
    assert!(app.navigation.visible_paths().is_empty());
    assert!(chooser_error(&app).unwrap().contains("whole snapshot"));
    frame(&mut app, 3.0, vec![egui::Event::Text("x".into())]);
    assert!(chooser_error(&app).unwrap().contains("whole snapshot"));
    assert!(commands.try_recv().is_err());
    frame(
        &mut app,
        4.0,
        key(egui::Key::Enter, egui::Modifiers::COMMAND),
    );
    assert!(
        matches!(commands.try_recv().unwrap().op, Operation::Read { path } if path == "unchangedx")
    );
}

#[test]
fn pending_refresh_failure_updates_stale_label_without_replacing_snapshot_identity() {
    for tree in [false, true] {
        let (mut app, commands) = app();
        let path = if tree {
            admit_tree(&mut app, &commands);
            "left/雪.rs"
        } else {
            admit_flat(&mut app, &commands, vec![entry("cached.rs", false)]);
            "cached.rs"
        };
        app.explorer_refresh(if tree { "left" } else { "" });
        let refresh = commands.try_recv().unwrap();
        app.navigation.query = path.into();
        app.show_file_chooser();
        frame(&mut app, 0.0, vec![]);
        enter_loaded(&mut app, 1.0);
        let before = match &app.navigation.dialog.as_ref().unwrap().kind {
            Kind::Files { snapshot, .. } => snapshot
                .iter()
                .find(|item| item.path.as_ref() == path)
                .unwrap()
                .clone(),
            _ => unreachable!(),
        };
        assert!(!app.navigation_candidate_label(&before).contains("stale"));
        app.apply_event(Event {
            generation: app.generation,
            id: refresh.id,
            connected: true,
            result: Err("refresh failed after capture".into()),
        });
        assert!(app.navigation_candidate_current(&before));
        assert!(app
            .navigation_candidate_label(&before)
            .contains("stale listing"));
        frame(&mut app, 2.0, key(egui::Key::Enter, egui::Modifiers::NONE));
        assert!(
            matches!(commands.try_recv().unwrap().op, Operation::Read { path: opened } if opened == path)
        );
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn loaded_mode_or_generation_change_visibly_cancels_without_opening() {
    for generation in [false, true] {
        let (mut app, commands) = app();
        admit_flat(&mut app, &commands, vec![entry("cached.rs", false)]);
        app.navigation.query = "cached".into();
        app.show_file_chooser();
        frame(&mut app, 0.0, vec![]);
        enter_loaded(&mut app, 1.0);
        if generation {
            app.generation += 1;
        } else {
            app.explorer_set_mode(Mode::Tree);
        }
        frame(&mut app, 2.0, key(egui::Key::Enter, egui::Modifiers::NONE));
        assert!(!app.navigation.dialog_open());
        assert!(app
            .error
            .as_deref()
            .unwrap()
            .contains("File chooser canceled"));
        assert_eq!(app.documents[0].edit_version, 0);
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn loaded_unopened_offline_refuses_but_dirty_open_buffer_preserves_editor_state() {
    let (mut app, commands) = app();
    admit_flat(&mut app, &commands, vec![entry("cached.rs", false)]);
    app.navigation.query = "cached".into();
    app.show_file_chooser();
    frame(&mut app, 0.0, vec![]);
    enter_loaded(&mut app, 1.0);
    app.state = ConnectionState::Disconnected;
    frame(&mut app, 2.0, key(egui::Key::Enter, egui::Modifiers::NONE));
    assert!(chooser_error(&app).unwrap().contains("Connect"));
    assert!(commands.try_recv().is_err());
    frame(&mut app, 3.0, key(egui::Key::Escape, egui::Modifiers::NONE));
    let mut doc = Document::new(2, "draft.rs".into(), "baseline".into(), "revision".into());
    doc.text = "dirty draft".into();
    doc.edit_version = 3;
    app.documents.push(doc);
    app.navigation.query = "draft".into();
    app.show_file_chooser();
    frame(&mut app, 4.0, vec![]);
    enter_loaded(&mut app, 5.0);
    frame(&mut app, 6.0, key(egui::Key::Enter, egui::Modifiers::NONE));
    assert_eq!(app.active_document, Some(2));
    assert_eq!(app.documents[1].text, "dirty draft");
    assert_eq!(app.documents[1].saved_text, "baseline");
    assert_eq!(app.documents[1].revision.as_deref(), Some("revision"));
    assert_eq!(app.documents[1].edit_version, 3);
    assert_eq!(app.location_history.back.len(), 1);
    assert!(commands.try_recv().is_err());
}

#[test]
fn explicit_typed_path_still_owns_command_enter_while_scope_control_is_focused() {
    let (mut app, commands) = app();
    app.navigation.query = "typed.rs".into();
    app.show_file_chooser();
    frame(&mut app, 0.0, vec![]);
    frame(&mut app, 1.0, key(egui::Key::Tab, egui::Modifiers::SHIFT));
    frame(&mut app, 1.1, vec![]);
    assert!(app
        .editor_ctx
        .memory(|memory| memory.has_focus(egui::Id::new("navigation_scope_loaded"))));
    frame(
        &mut app,
        2.0,
        key(egui::Key::Enter, egui::Modifiers::COMMAND),
    );
    assert!(!app.navigation.dialog_open());
    assert!(
        matches!(commands.try_recv().unwrap().op, Operation::Read { path } if path == "typed.rs")
    );
    assert!(commands.try_recv().is_err());
}

#[test]
fn backward_tab_scope_focus_requires_one_following_frame_before_native_space_activation() {
    let (mut app, commands) = app();
    let source = app.documents[0].text.clone();
    let mut opening = key(egui::Key::P, egui::Modifiers::COMMAND);
    opening.push(egui::Event::Text("first".into()));
    frame(&mut app, 0.0, opening);
    frame(&mut app, 0.1, vec![]);
    assert_eq!(app.navigation.query, "first");
    assert!(!app.navigation.loaded_scope());
    assert!(app
        .editor_ctx
        .memory(|memory| memory.has_focus(egui::Id::new(FILE_INPUT))));

    let loaded_id = egui::Id::new("navigation_scope_loaded");
    frame(&mut app, 1.0, key(egui::Key::Tab, egui::Modifiers::SHIFT));
    // egui 0.31.1 interested_in_focus(Previous) records id_next_frame;
    // begin_pass applies it on the following frame, not the Shift+Tab frame.
    assert!(!app.editor_ctx.memory(|memory| memory.has_focus(loaded_id)));
    assert!(!app.navigation.loaded_scope());

    frame(&mut app, 2.0, vec![]);
    assert!(app.editor_ctx.memory(|memory| memory.has_focus(loaded_id)));
    assert!(!app.navigation.loaded_scope());

    let mut activation = key(egui::Key::Space, egui::Modifiers::NONE);
    activation.push(egui::Event::Text(" ".into()));
    frame(&mut app, 3.0, activation);
    assert!(app.navigation.loaded_scope());
    assert_eq!(app.navigation.query, "first");
    assert!(app
        .editor_ctx
        .memory(|memory| memory.has_focus(egui::Id::new(FILE_INPUT))));
    assert_eq!(app.documents[0].text, source);
    assert_eq!(app.documents[0].edit_version, 0);
    assert!(commands.try_recv().is_err());
}
