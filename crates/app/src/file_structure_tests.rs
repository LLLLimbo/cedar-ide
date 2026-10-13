//! Actual shipping-frame tests for the bounded current-file reading journey.
use super::*;
use crate::worker::{Command, Worker};
use cedar_protocol::Payload;
use serde_json::json;
use std::sync::mpsc::Receiver;

const SOURCE: &str = "struct 雪 {\r\n  fn same() { let café = \"😀\"; }\r\n  fn same() {}\r\n}\r\n";
fn range(text: &str, marker: &str, last: bool) -> Range {
    let byte = if last {
        text.rfind(marker)
    } else {
        text.find(marker)
    }
    .unwrap();
    let chars = text[..byte].chars().count();
    let start = crate::model::cursor_location(text, chars);
    let end = crate::model::cursor_location(text, chars + marker.chars().count());
    Range {
        start: utf16_position(text, start).unwrap(),
        end: utf16_position(text, end).unwrap(),
    }
}
fn range_json(range: Range) -> Value {
    json!({"start":{"line":range.start.line,"character":range.start.character},"end":{"line":range.end.line,"character":range.end.character}})
}
fn symbols() -> Value {
    let root = range_json(range(SOURCE, "雪", false));
    let one = range_json(range(SOURCE, "same", false));
    let two = range_json(range(SOURCE, "same", true));
    let unicode = range_json(range(SOURCE, "café", false));
    json!([{"name":"雪","kind":23,"range":{"start":{"line":0,"character":0},"end":{"line":3,"character":1}},"selectionRange":root,
        "children":[{"name":"same","detail":"first 😀","kind":6,"range":one,"selectionRange":one},
        {"name":"café","kind":13,"range":unicode,"selectionRange":unicode},
        {"name":"same","detail":"second","kind":6,"range":two,"selectionRange":two}]}])
}
fn app() -> (CedarApp, Receiver<Command>) {
    let mut app = CedarApp::empty();
    app.state = crate::ConnectionState::Ready;
    app.active_form = Some(crate::ConnectForm {
        allow_run: true,
        ..Default::default()
    });
    app.agent_info = Some(crate::agent_support::full_test_agent());
    app.language.running = true;
    app.language.automatic = false;
    app.language.next_events = f64::INFINITY;
    app.language.capabilities = json!({"documentSymbolProvider":true});
    app.open_form = false;
    app.documents.push(Document::new(
        1,
        "main.rs".into(),
        SOURCE.into(),
        "r0".into(),
    ));
    app.active_document = Some(1);
    app.next_document = 2;
    let mut editor = crate::editor_state::load(&app.editor_ctx, &mut app.documents[0]);
    editor
        .cursor
        .set_char_range(Some(egui::text::CCursorRange::one(
            egui::text::CCursor::new(0),
        )));
    editor.store(&app.editor_ctx, egui::Id::new(("editor", 1u64)));
    app.editor_ctx
        .memory_mut(|memory| memory.request_focus(egui::Id::new(("editor", 1u64))));
    acknowledge(&mut app);
    let (worker, rx) = Worker::recording();
    app.worker = Some(worker);
    (app, rx)
}
fn acknowledge(app: &mut CedarApp) {
    app.language.sync.acknowledge(
        1,
        Acknowledged {
            version: app.language.sync.next_version(1).unwrap(),
            edit_version: app.documents[0].edit_version,
            uri: "file:///workspace/main.rs".into(),
        },
    );
}
fn seed(app: &mut CedarApp, value: Value) {
    app.request_language_navigation_feature(FeatureKind::Outline);
    let Some(FeatureStep::Dispatch(request, _)) = app.next_language_feature_step() else {
        panic!("synced fixture");
    };
    app.apply_language_feature(*request, value);
    app.tools_open = false;
    app.language.view = View::Problems;
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
fn frame(app: &mut CedarApp, events: Vec<egui::Event>) {
    let ctx = app.editor_ctx.clone();
    let time = ctx.input(|input| input.time) + 0.05;
    let mut input = egui::RawInput {
        time: Some(time),
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1100.0, 850.0),
        )),
        events,
        ..Default::default()
    };
    let mut native = eframe::Frame::_new_kittest();
    eframe::App::raw_input_hook(app, &ctx, &mut input);
    let _ = ctx.run(input, |ctx| eframe::App::update(app, ctx, &mut native));
}
fn settle(app: &mut CedarApp) {
    for _ in 0..3 {
        frame(app, vec![]);
    }
}
fn open(app: &mut CedarApp) {
    frame(
        app,
        key(
            egui::Key::F12,
            egui::Modifiers::CTRL | egui::Modifiers::COMMAND,
        ),
    );
    settle(app);
    assert!(app.file_structure_open());
}
fn selection(app: &CedarApp) -> egui::text::CCursorRange {
    egui::TextEdit::load_state(&app.editor_ctx, egui::Id::new(("editor", 1u64)))
        .unwrap()
        .cursor
        .char_range()
        .unwrap()
}
fn target(app: &CedarApp, marker: &str, last: bool) {
    let expected = range(SOURCE, marker, last);
    let start = completion::position_to_offsets(SOURCE, expected.start)
        .unwrap()
        .1;
    let end = completion::position_to_offsets(SOURCE, expected.end)
        .unwrap()
        .1;
    assert_eq!(
        selection(app).sorted().map(|cursor| cursor.index),
        [start, end]
    );
}
fn pointer(pos: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        },
    ]
}
fn reply(app: &mut CedarApp, command: &Command, value: Value) {
    let (tx, rx) = std::sync::mpsc::channel();
    app.result_rx = rx;
    tx.send(crate::worker::WorkerEvent::Response(crate::worker::Event {
        generation: app.generation,
        id: command.id,
        connected: true,
        result: Ok(Payload::Language { value }),
    }))
    .unwrap();
    settle(app);
}

#[test]
fn file_structure_actual_frames_hierarchy_duplicates_unicode_crlf_filter_keyboard_and_cache() {
    let (mut app, rx) = app();
    seed(&mut app, symbols());
    settle(&mut app);
    open(&mut app);
    assert_eq!(
        app.file_structure_rows(),
        vec![(0, "雪"), (1, "same"), (2, "café"), (3, "same")]
    );
    assert_eq!(
        app.language.features.outline.as_ref().unwrap().items[3].depth,
        1
    );
    frame(&mut app, vec![egui::Event::Text("same".into())]);
    assert_eq!(app.file_structure_rows(), vec![(1, "same"), (3, "same")]);
    frame(&mut app, key(egui::Key::End, egui::Modifiers::NONE));
    assert_eq!(app.file_structure_selected(), Some(3));
    frame(&mut app, key(egui::Key::Home, egui::Modifiers::NONE));
    assert_eq!(app.file_structure_selected(), Some(1));
    frame(&mut app, key(egui::Key::ArrowDown, egui::Modifiers::NONE));
    assert_eq!(app.file_structure_selected(), Some(3));
    frame(&mut app, key(egui::Key::Enter, egui::Modifiers::NONE));
    settle(&mut app);
    assert!(!app.file_structure_open());
    target(&app, "same", true);
    assert!(!app.tools_open);
    assert!(app.language.view == View::Problems);
    open(&mut app);
    frame(&mut app, vec![egui::Event::Text("café".into())]);
    frame(&mut app, key(egui::Key::Enter, egui::Modifiers::NONE));
    settle(&mut app);
    target(&app, "café", false);
    assert!(
        rx.try_recv().is_err(),
        "cache, filtering and local selection must perform zero operations"
    );
}

#[test]
fn file_structure_actual_frames_no_match_bound_escape_and_native_tab() {
    let (mut app, rx) = app();
    seed(&mut app, symbols());
    settle(&mut app);
    let departure = selection(&app);
    open(&mut app);
    frame(&mut app, vec![egui::Event::Paste("z".repeat(1024))]);
    assert_eq!(
        app.file_structure_query().unwrap().chars().count(),
        MAX_FILTER_CHARS
    );
    assert!(app.file_structure_rows().is_empty());
    frame(&mut app, key(egui::Key::Enter, egui::Modifiers::NONE));
    assert!(app.file_structure_open());
    app.editor_ctx
        .memory_mut(|memory| memory.request_focus(egui::Id::new(FILTER)));
    frame(&mut app, key(egui::Key::Tab, egui::Modifiers::NONE));
    settle(&mut app);
    assert_ne!(
        app.editor_ctx.memory(|memory| memory.focused()),
        Some(egui::Id::new(FILTER))
    );
    frame(&mut app, key(egui::Key::Escape, egui::Modifiers::NONE));
    settle(&mut app);
    assert!(!app.file_structure_open());
    assert_eq!(selection(&app), departure);
    assert_eq!(
        app.editor_ctx.memory(|memory| memory.focused()),
        Some(egui::Id::new(("editor", 1u64)))
    );
    assert!(rx.try_recv().is_err());
}

#[test]
fn file_structure_actual_frames_pointer_identity_and_mixed_input_guards() {
    let (mut app, rx) = app();
    seed(&mut app, symbols());
    settle(&mut app);
    open(&mut app);
    let rect = crate::workspace_access_tests::recorded_rect(&app, "file_structure_row_3");
    frame(&mut app, pointer(rect.center(), true));
    frame(&mut app, pointer(rect.center(), false));
    settle(&mut app);
    target(&app, "same", true);
    open(&mut app);
    let rect = crate::workspace_access_tests::recorded_rect(&app, "file_structure_row_1");
    frame(&mut app, pointer(rect.center(), true));
    let mut mixed = pointer(rect.center(), false);
    mixed.push(egui::Event::Text("café".into()));
    frame(&mut app, mixed);
    settle(&mut app);
    assert!(app.file_structure_open());
    target(&app, "same", true);
    let mut mixed = key(egui::Key::Enter, egui::Modifiers::NONE);
    mixed.push(egui::Event::Ime(egui::ImeEvent::Commit("x".into())));
    frame(&mut app, mixed);
    assert!(app.file_structure_open());
    frame(
        &mut app,
        vec![egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: true,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert!(app.file_structure_open());
    assert!(rx.try_recv().is_err());
}

#[test]
fn file_structure_actual_frames_dirty_selection_back_and_undo() {
    let (mut app, rx) = app();
    settle(&mut app);
    let dirty = format!("{SOURCE}// dirty\r\n");
    crate::editor_state::commit(&app.editor_ctx, &mut app.documents[0], dirty.clone(), 3);
    acknowledge(&mut app);
    seed(&mut app, symbols());
    settle(&mut app);
    let directional = egui::text::CCursorRange {
        primary: egui::text::CCursor {
            index: 2,
            prefer_next_row: false,
        },
        secondary: egui::text::CCursor {
            index: 8,
            prefer_next_row: true,
        },
    };
    let mut state = crate::editor_state::load(&app.editor_ctx, &mut app.documents[0]);
    state.cursor.set_char_range(Some(directional));
    state.store(&app.editor_ctx, egui::Id::new(("editor", 1u64)));
    let departure = selection(&app);
    open(&mut app);
    frame(&mut app, key(egui::Key::End, egui::Modifiers::NONE));
    frame(&mut app, key(egui::Key::Enter, egui::Modifiers::NONE));
    settle(&mut app);
    assert!(app.documents[0].dirty());
    assert_eq!(app.documents[0].text, dirty);
    frame(
        &mut app,
        key(
            egui::Key::OpenBracket,
            egui::Modifiers::CTRL | egui::Modifiers::COMMAND,
        ),
    );
    settle(&mut app);
    assert_eq!(selection(&app), departure);
    assert_eq!(
        selection(&app).primary.prefer_next_row,
        departure.primary.prefer_next_row
    );
    assert_eq!(
        selection(&app).secondary.prefer_next_row,
        departure.secondary.prefer_next_row
    );
    frame(
        &mut app,
        key(
            egui::Key::Z,
            egui::Modifiers::CTRL | egui::Modifiers::COMMAND,
        ),
    );
    settle(&mut app);
    assert_eq!(app.documents[0].text, SOURCE);
    assert!(rx.try_recv().is_err());
}

#[test]
fn file_structure_actual_frames_unavailable_never_requests() {
    for case in 0..4 {
        let (mut app, rx) = app();
        match case {
            0 => app.language.running = false,
            1 => app.language.capabilities = json!({"documentSymbolProvider":false}),
            2 => app.agent_info = None,
            _ => app.documents[0].path = "notes.txt".into(),
        }
        settle(&mut app);
        open(&mut app);
        assert!(app
            .language
            .features
            .file_structure
            .popup
            .as_ref()
            .unwrap()
            .error
            .is_some());
        assert!(rx.try_recv().is_err());
        assert!(!app.tools_open);
    }
}

#[test]
fn file_structure_actual_frames_dismissed_reopened_and_edited_reply_is_inert() {
    let (mut app, rx) = app();
    settle(&mut app);
    open(&mut app);
    let old = rx.try_recv().unwrap();
    assert!(matches!(old.op, Operation::LanguageDocumentSymbols { .. }));
    frame(&mut app, key(egui::Key::Escape, egui::Modifiers::NONE));
    settle(&mut app);
    reply(&mut app, &old, symbols());
    assert!(!app.file_structure_open());
    assert!(app.language.features.outline.is_none());
    assert!(!app.tools_open);
    open(&mut app);
    let old = rx.try_recv().unwrap();
    open(&mut app); // A repeated invocation retires the exact older destination.
    reply(&mut app, &old, symbols());
    let fresh = rx.try_recv().unwrap();
    reply(&mut app, &fresh, symbols());
    assert!(app.file_structure_open());
    assert_eq!(app.file_structure_rows().len(), 4);
    assert!(!app.tools_open);
    app.documents[0].edit_version += 1;
    app.documents[0].text.push('x');
    settle(&mut app);
    assert!(!app.file_structure_open());
    assert!(app.language.features.outline.is_none());
}

#[test]
fn file_structure_actual_frames_flat_symbol_uses_uri_route_and_dismisses_late_result() {
    let (mut app, rx) = app();
    seed(
        &mut app,
        json!([{"name":"same","kind":6,"location":{"uri":"file:///workspace/main.rs","range":range_json(range(SOURCE,"same",true))}}]),
    );
    settle(&mut app);
    let departure = selection(&app);
    open(&mut app);
    frame(&mut app, key(egui::Key::Enter, egui::Modifiers::NONE));
    let resolve = rx.try_recv().unwrap();
    assert!(matches!(resolve.op, Operation::LanguageResolveUri { .. }));
    assert_eq!(selection(&app), departure);
    frame(&mut app, key(egui::Key::Escape, egui::Modifiers::NONE));
    settle(&mut app);
    reply(&mut app, &resolve, json!({"path":"main.rs"}));
    assert_eq!(selection(&app), departure);
    open(&mut app);
    frame(&mut app, key(egui::Key::Enter, egui::Modifiers::NONE));
    let resolve = rx.try_recv().unwrap();
    reply(&mut app, &resolve, json!({"path":"main.rs"}));
    target(&app, "same", true);
    assert!(!app.file_structure_open());
    assert!(rx.try_recv().is_err());
}

#[test]
fn file_structure_actual_frames_persistent_ime_and_foreign_modal_retire() {
    let (mut app, rx) = app();
    seed(&mut app, symbols());
    settle(&mut app);
    open(&mut app);
    frame(
        &mut app,
        vec![
            egui::Event::Ime(egui::ImeEvent::Enabled),
            egui::Event::Ime(egui::ImeEvent::Preedit("c".into())),
        ],
    );
    frame(&mut app, key(egui::Key::Enter, egui::Modifiers::NONE));
    assert!(app.file_structure_open());
    frame(&mut app, vec![egui::Event::Ime(egui::ImeEvent::Disabled)]);
    app.confirm = Some(crate::Confirm::CloseWindow);
    frame(&mut app, key(egui::Key::Enter, egui::Modifiers::NONE));
    assert!(!app.file_structure_open());
    assert!(rx.try_recv().is_err());
}

#[test]
fn file_structure_actual_frames_flat_queued_read_success_and_newer_input_retirement() {
    for retire in [false, true] {
        let (mut app, rx) = app();
        seed(
            &mut app,
            json!([{"name":"same","kind":6,"location":{"uri":"file:///workspace/other.rs","range":range_json(range(SOURCE,"same",true))}}]),
        );
        settle(&mut app);
        open(&mut app);
        frame(&mut app, key(egui::Key::Enter, egui::Modifiers::NONE));
        let resolve = rx.try_recv().unwrap();
        reply(&mut app, &resolve, json!({"path":"other.rs"}));
        let read = rx.try_recv().unwrap();
        assert!(matches!(read.op,Operation::Read{ref path} if path=="other.rs"));
        let (tx, replies) = std::sync::mpsc::channel();
        app.result_rx = replies;
        tx.send(crate::worker::WorkerEvent::Response(crate::worker::Event {
            generation: app.generation,
            id: read.id,
            connected: true,
            result: Ok(Payload::File {
                path: "other.rs".into(),
                text: SOURCE.into(),
                revision: "r1".into(),
            }),
        }))
        .unwrap();
        frame(
            &mut app,
            if retire {
                key(egui::Key::Escape, egui::Modifiers::NONE)
            } else {
                vec![]
            },
        );
        settle(&mut app);
        if retire {
            assert_eq!(app.active_document, Some(1));
            assert_eq!(app.documents.len(), 1);
        } else {
            assert_eq!(app.active().unwrap().path, "other.rs");
            assert_eq!(app.documents.len(), 2);
        }
        assert!(rx.try_recv().is_err());
    }
}

#[test]
fn file_structure_actual_frames_partial_sync_capability_and_shortcut_modifiers() {
    for capability in ["language_open", "language_change"] {
        let (mut app, rx) = app();
        app.agent_info
            .as_mut()
            .unwrap()
            .capabilities
            .retain(|name| name != capability);
        if capability == "language_open" {
            app.language.sync.clear();
        } else {
            app.documents[0].edit_version += 1;
        }
        settle(&mut app);
        open(&mut app);
        settle(&mut app);
        assert!(app
            .language
            .features
            .file_structure
            .popup
            .as_ref()
            .unwrap()
            .error
            .as_ref()
            .unwrap()
            .contains(capability));
        assert!(rx.try_recv().is_err());
    }
    let command = egui::Modifiers::COMMAND;
    for (modifiers, allowed) in [
        (egui::Modifiers::CTRL | command, true),
        (
            egui::Modifiers {
                mac_cmd: true,
                command: true,
                ..Default::default()
            },
            true,
        ),
        (egui::Modifiers::CTRL, false),
        (
            egui::Modifiers {
                ctrl: true,
                mac_cmd: true,
                command: true,
                ..Default::default()
            },
            false,
        ),
        (
            egui::Modifiers::CTRL | command | egui::Modifiers::SHIFT,
            false,
        ),
        (
            egui::Modifiers::CTRL | command | egui::Modifiers::ALT,
            false,
        ),
    ] {
        let (mut app, rx) = app();
        seed(&mut app, symbols());
        settle(&mut app);
        frame(&mut app, key(egui::Key::F12, modifiers));
        settle(&mut app);
        assert_eq!(app.file_structure_open(), allowed, "{modifiers:?}");
        assert!(rx.try_recv().is_err());
    }
}

#[test]
fn file_structure_actual_frames_snapshot_replacement_and_late_error_are_inert() {
    let (mut app, rx) = app();
    seed(&mut app, symbols());
    settle(&mut app);
    open(&mut app);
    let departure = selection(&app);
    let pos = crate::workspace_access_tests::recorded_rect(&app, "file_structure_row_3").center();
    frame(&mut app, pointer(pos, true));
    seed(&mut app, symbols());
    frame(&mut app, pointer(pos, false));
    settle(&mut app);
    assert_eq!(selection(&app), departure);
    assert!(app.file_structure_open());
    assert!(rx.try_recv().is_err());
    app.dismiss_file_structure();
    settle(&mut app);
    app.language.features.outline = None;
    open(&mut app);
    let old = rx.try_recv().unwrap();
    frame(&mut app, key(egui::Key::Escape, egui::Modifiers::NONE));
    settle(&mut app);
    open(&mut app);
    app.error = Some("current error".into());
    let (tx, replies) = std::sync::mpsc::channel();
    app.result_rx = replies;
    tx.send(crate::worker::WorkerEvent::Response(crate::worker::Event {
        generation: app.generation,
        id: old.id,
        connected: true,
        result: Err("late old error".into()),
    }))
    .unwrap();
    settle(&mut app);
    assert_eq!(app.error.as_deref(), Some("current error"));
    assert!(app.file_structure_open());
    let fresh = rx.try_recv().unwrap();
    reply(&mut app, &fresh, symbols());
    assert_eq!(app.file_structure_rows().len(), 4);
}

#[test]
fn file_structure_actual_frames_pointer_loss_relative_motion_and_counter_exhaustion() {
    let (mut app, rx) = app();
    seed(&mut app, symbols());
    settle(&mut app);
    open(&mut app);
    let pos = crate::workspace_access_tests::recorded_rect(&app, "file_structure_row_3").center();
    frame(&mut app, pointer(pos, true));
    frame(&mut app, vec![egui::Event::PointerGone]);
    frame(&mut app, pointer(pos, false));
    settle(&mut app);
    assert!(app.file_structure_open());
    let mut press = pointer(pos, true);
    press.insert(0, egui::Event::MouseMoved(egui::vec2(1.0, 0.0)));
    frame(&mut app, press);
    let mut release = pointer(pos, false);
    release.insert(0, egui::Event::MouseMoved(egui::vec2(1.0, 0.0)));
    frame(&mut app, release);
    settle(&mut app);
    assert!(!app.file_structure_open());
    target(&app, "same", true);
    assert!(rx.try_recv().is_err());
    open(&mut app);
    app.language
        .features
        .file_structure
        .popup
        .as_mut()
        .unwrap()
        .filter = u64::MAX;
    frame(&mut app, vec![egui::Event::Text("x".into())]);
    settle(&mut app);
    assert!(!app.file_structure_open());
    assert!(app.language.features.file_structure.exhausted);
    app.show_file_structure();
    assert!(!app.file_structure_open());
}

#[test]
fn file_structure_actual_frames_unfocused_queued_result_and_cjk_stale_guard() {
    let (mut app, rx) = app();
    seed(
        &mut app,
        json!([{"name":"雪","kind":6,"location":{"uri":"file:///workspace/main.rs","range":range_json(range(SOURCE,"same",true))}}]),
    );
    assert!(app.language.cjk_seen);
    settle(&mut app);
    open(&mut app);
    frame(&mut app, key(egui::Key::Enter, egui::Modifiers::NONE));
    let command = rx.try_recv().unwrap();
    let departure = selection(&app);
    let (tx, replies) = std::sync::mpsc::channel();
    app.result_rx = replies;
    tx.send(crate::worker::WorkerEvent::Response(crate::worker::Event {
        generation: app.generation,
        id: command.id,
        connected: true,
        result: Ok(Payload::Language {
            value: json!({"path":"main.rs"}),
        }),
    }))
    .unwrap();
    let ctx = app.editor_ctx.clone();
    let mut native = eframe::Frame::_new_kittest();
    let mut input = egui::RawInput {
        focused: false,
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1100.0, 850.0),
        )),
        ..Default::default()
    };
    eframe::App::raw_input_hook(&mut app, &ctx, &mut input);
    let _ = ctx.run(input, |ctx| eframe::App::update(&mut app, ctx, &mut native));
    assert_eq!(selection(&app), departure);
    assert!(app.language.features.file_structure.popup.is_none());
    app.language.cjk_seen = false;
    app.language.features.outline = None;
    app.show_file_structure();
    let Some(FeatureStep::Dispatch(request, _)) = app.next_language_feature_step() else {
        panic!("sync")
    };
    app.dismiss_file_structure();
    app.apply_language_feature(*request, symbols());
    assert!(!app.language.cjk_seen);
}

#[test]
fn file_structure_actual_frames_ime_pointer_lifecycle_and_stale_capture() {
    for composing in [false, true] {
        let (mut app, rx) = app();
        seed(&mut app, symbols());
        settle(&mut app);
        open(&mut app);
        let pos =
            crate::workspace_access_tests::recorded_rect(&app, "file_structure_row_3").center();
        let mut press = vec![egui::Event::Ime(egui::ImeEvent::Enabled)];
        if composing {
            press.push(egui::Event::Ime(egui::ImeEvent::Preedit("x".into())));
        }
        press.extend(pointer(pos, true));
        frame(&mut app, press);
        let mut release = vec![egui::Event::Ime(egui::ImeEvent::Disabled)];
        release.extend(pointer(pos, false));
        frame(&mut app, release);
        settle(&mut app);
        assert_eq!(app.file_structure_open(), composing);
        assert!(rx.try_recv().is_err());
    }
    let (mut app, rx) = app();
    seed(&mut app, symbols());
    settle(&mut app);
    open(&mut app);
    let departure = selection(&app);
    frame(&mut app, key(egui::Key::End, egui::Modifiers::NONE));
    let ctx = app.editor_ctx.clone();
    let mut native = eframe::Frame::_new_kittest();
    let _ = ctx.run(
        egui::RawInput {
            events: key(egui::Key::Enter, egui::Modifiers::NONE),
            ..Default::default()
        },
        |ctx| eframe::App::update(&mut app, ctx, &mut native),
    );
    assert!(!app.file_structure_open());
    assert_eq!(selection(&app), departure);
    assert!(rx.try_recv().is_err());
}

#[test]
fn file_structure_actual_frames_immediate_escape_reopen_then_type() {
    let (mut app, rx) = app();
    seed(&mut app, symbols());
    settle(&mut app);
    open(&mut app);
    let source = app.documents[0].text.clone();
    frame(&mut app, key(egui::Key::Escape, egui::Modifiers::NONE));
    frame(
        &mut app,
        key(
            egui::Key::F12,
            egui::Modifiers::CTRL | egui::Modifiers::COMMAND,
        ),
    );
    assert!(app.file_structure_open());
    assert!(!app.navigation.restore_focus);
    frame(&mut app, vec![egui::Event::Text("co".into())]);
    assert!(app.file_structure_open());
    assert_eq!(app.file_structure_query(), Some("co"));
    assert_eq!(app.documents[0].text, source);
    assert!(rx.try_recv().is_err());
}

#[test]
fn file_structure_actual_frames_immediate_reopen_respects_foreign_modal() {
    let (mut app, rx) = app();
    seed(&mut app, symbols());
    settle(&mut app);
    open(&mut app);
    let source = app.documents[0].text.clone();
    frame(&mut app, key(egui::Key::Escape, egui::Modifiers::NONE));
    app.confirm = Some(crate::Confirm::CloseWindow);
    frame(
        &mut app,
        key(
            egui::Key::F12,
            egui::Modifiers::CTRL | egui::Modifiers::COMMAND,
        ),
    );
    frame(&mut app, vec![egui::Event::Text("co".into())]);
    assert!(!app.file_structure_open());
    assert_eq!(app.documents[0].text, source);
    assert!(rx.try_recv().is_err());
}

#[test]
fn file_structure_actual_frames_runtime_symbol_selection_waits_for_activation() {
    let (mut app, rx) = app();
    let mock_source = "class Hello { /* café 😀 */ int count; }\n";
    app.documents[0].text = mock_source.into();
    app.documents[0].saved_text = mock_source.into();
    app.language.sync.clear();
    let mut state = crate::editor_state::load(&app.editor_ctx, &mut app.documents[0]);
    state
        .cursor
        .set_char_range(Some(egui::text::CCursorRange::one(
            egui::text::CCursor::new(5),
        )));
    state.store(&app.editor_ctx, egui::Id::new(("editor", 1u64)));
    let run_frame = |app: &mut CedarApp, events| {
        let ctx = app.editor_ctx.clone();
        let mut native = eframe::Frame::_new_kittest();
        let mut input = egui::RawInput {
            time: Some(ctx.input(|input| input.time) + 0.25),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 960.0),
            )),
            events,
            ..Default::default()
        };
        eframe::App::raw_input_hook(app, &ctx, &mut input);
        ctx.run(input, |ctx| eframe::App::update(app, ctx, &mut native))
    };
    run_frame(&mut app, vec![]);
    run_frame(
        &mut app,
        key(
            egui::Key::F12,
            egui::Modifiers::CTRL | egui::Modifiers::COMMAND,
        ),
    );
    let sync = rx.try_recv().unwrap();
    assert!(matches!(sync.op, Operation::LanguageOpen { .. }));
    app.apply_event(crate::worker::Event {
        generation: app.generation,
        id: sync.id,
        connected: true,
        result: Ok(Payload::Language {
            value: json!({"opened":"file:///workspace/main.rs"}),
        }),
    });
    run_frame(&mut app, vec![]);
    let query = rx.try_recv().unwrap();
    assert!(matches!(
        query.op,
        Operation::LanguageDocumentSymbols { .. }
    ));
    let range = json!({"start":{"line":0,"character":0},"end":{"line":0,"character":1}});
    app.apply_event(crate::worker::Event{generation:app.generation,id:query.id,connected:true,result:Ok(Payload::Language{value:json!([{"name":"Hello","kind":5,"range":range,"selectionRange":range,"children":[{"name":"count","kind":8,"range":range,"selectionRange":range}]}])})});
    run_frame(&mut app, vec![]);
    run_frame(&mut app, key(egui::Key::Escape, egui::Modifiers::NONE));
    run_frame(
        &mut app,
        key(
            egui::Key::F12,
            egui::Modifiers::CTRL | egui::Modifiers::COMMAND,
        ),
    );
    run_frame(&mut app, vec![egui::Event::Text("co".into())]);
    assert_eq!(app.file_structure_query(), Some("co"));
    run_frame(&mut app, key(egui::Key::Enter, egui::Modifiers::NONE));
    assert!(app.file_structure_activation_pending());
    let pending = |app: &CedarApp| {
        app.file_structure_activation_pending()
            || app.navigation.restore_focus
            || app.active().is_some_and(|doc| doc.jump_to.is_some())
            || app
                .editor_ctx
                .memory(|memory| memory.top_modal_layer().is_some())
    };
    for frame_number in 0..3 {
        if !pending(&app) {
            break;
        }
        let output = run_frame(&mut app, vec![]);
        if frame_number == 0 {
            assert!(app.file_structure_activation_pending());
            assert!(!app
                .editor_ctx
                .memory(|memory| memory.top_modal_layer().is_some()));
            assert_eq!(selection(&app).sorted().map(|cursor| cursor.index), [5, 5]);
            assert_eq!(
                output.viewport_output[&egui::ViewportId::ROOT].repaint_delay,
                std::time::Duration::ZERO
            );
        }
        assert!(rx.try_recv().is_err(), "settling must remain local");
    }
    assert!(!pending(&app));
    assert_eq!(selection(&app).sorted().map(|cursor| cursor.index), [0, 1]);
    assert_eq!(app.documents[0].text, mock_source);
    assert!(rx.try_recv().is_err());
}
