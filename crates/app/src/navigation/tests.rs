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
        Kind::Files { selected } => selected.as_deref(),
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
    assert_eq!(result.items[0].path, "z.rs");
    assert_eq!(result.items[1].path, "中文\\é.rs ");
    assert_eq!(result.items[2].path, "00.rs");
    assert!(result.items[..2].iter().all(|item| item.open));
    assert!(result.items[2..].iter().all(|item| !item.open));
    let result = candidates(&documents, &entries, "中文\\É");
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.items[0].path, "中文\\é.rs ");
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
