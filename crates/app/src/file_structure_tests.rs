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
    frame_sized(app, events, egui::vec2(1100.0, 850.0));
}
fn frame_sized(app: &mut CedarApp, events: Vec<egui::Event>, size: egui::Vec2) {
    let ctx = app.editor_ctx.clone();
    let time = ctx.input(|input| input.time) + 0.05;
    let mut input = egui::RawInput {
        time: Some(time),
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
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

fn tools_menu(app: &mut CedarApp) -> egui::Response {
    let at = crate::workspace_access_tests::recorded_rect(app, "tools_menu").center();
    frame(app, pointer(at, true));
    frame(app, pointer(at, false));
    settle(app);
    crate::workspace_access_tests::recorded_response(app, "file_structure_menu")
}

fn queue_menu(app: &mut CedarApp) {
    let response = tools_menu(app);
    assert!(response.enabled());
    frame(app, pointer(response.rect.center(), true));
    frame(app, pointer(response.rect.center(), false));
    assert!(
        !app.file_structure_open(),
        "menu intent waits for a native frame"
    );
    assert!(app.language.features.file_structure.menu.is_some());
}

#[test]
fn file_structure_menu_actual_gesture_defers_once_and_immediate_filter_typing_is_local() {
    let (mut app, rx) = app();
    seed(&mut app, symbols());
    settle(&mut app);
    let departure = selection(&app);
    queue_menu(&mut app);
    frame(&mut app, vec![egui::Event::Text("same".into())]);
    assert!(app.file_structure_open());
    assert_eq!(app.file_structure_query(), Some("same"));
    assert_eq!(app.documents[0].text, SOURCE);
    assert_eq!(selection(&app), departure);
    assert!(!app.tools_open);
    assert!(app.language.view == View::Problems);
    assert!(app.language.features.file_structure.menu.is_none());
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
fn file_structure_menu_keyboard_enter_and_space_claim_the_owned_activation() {
    for activation in [egui::Key::Enter, egui::Key::Space] {
        let (mut app, rx) = app();
        seed(&mut app, symbols());
        settle(&mut app);
        let response = tools_menu(&mut app);
        app.editor_ctx
            .memory_mut(|memory| memory.request_focus(response.id));
        frame(&mut app, vec![]);
        let mut press = key(activation, egui::Modifiers::NONE);
        press.pop(); // The native activation remains held across the handoff.
        if activation == egui::Key::Space {
            press.push(egui::Event::Text(" ".into()));
        }
        frame(&mut app, press);
        assert!(app.language.features.file_structure.menu.is_some());
        let mut repeated = key(activation, egui::Modifiers::NONE);
        repeated.pop();
        if let egui::Event::Key { repeat, .. } = &mut repeated[0] {
            *repeat = true;
        }
        if activation == egui::Key::Space {
            repeated.push(egui::Event::Text(" ".into()));
        }
        frame(&mut app, repeated);
        assert!(app.file_structure_open());
        assert_eq!(app.file_structure_query(), Some(""));
        frame(
            &mut app,
            vec![key(activation, egui::Modifiers::NONE).pop().unwrap()],
        );
        frame(&mut app, vec![egui::Event::Text("café".into())]);
        assert_eq!(app.file_structure_query(), Some("café"));
        assert_eq!(app.documents[0].text, SOURCE);
        assert!(rx.try_recv().is_err());
    }
}

#[test]
fn file_structure_menu_lifecycle_only_ime_preserves_pointer_ownership() {
    let (mut app, rx) = app();
    seed(&mut app, symbols());
    settle(&mut app);
    let at = tools_menu(&mut app).rect.center();
    let mut press = vec![egui::Event::Ime(egui::ImeEvent::Enabled)];
    press.extend(pointer(at, true));
    frame(&mut app, press);
    frame(
        &mut app,
        vec![
            egui::Event::PointerMoved(at),
            egui::Event::Ime(egui::ImeEvent::Disabled),
        ],
    );
    let mut release = vec![
        egui::Event::Ime(egui::ImeEvent::Enabled),
        egui::Event::Ime(egui::ImeEvent::Disabled),
    ];
    release.extend(pointer(at, false));
    frame(&mut app, release);
    assert!(app.language.features.file_structure.menu.is_some());
    settle(&mut app);
    assert!(app.file_structure_open());
    assert!(rx.try_recv().is_err());
}

#[test]
fn file_structure_menu_mixed_or_composing_pointer_and_foreign_capture_never_open() {
    for poison in [
        "text",
        "paste",
        "key",
        "wheel",
        "preedit",
        "empty_preedit",
        "commit",
        "empty_commit",
        "prior_composition",
        "foreign_capture",
        "poisoned_press",
    ] {
        let (mut app, rx) = app();
        seed(&mut app, symbols());
        settle(&mut app);
        let at = tools_menu(&mut app).rect.center();
        if poison == "prior_composition" {
            frame(
                &mut app,
                vec![egui::Event::Ime(egui::ImeEvent::Preedit("x".into()))],
            );
        }
        let mut press = pointer(
            if poison == "foreign_capture" {
                egui::pos2(900.0, 500.0)
            } else {
                at
            },
            true,
        );
        if poison == "poisoned_press" {
            press.push(egui::Event::Text("x".into()));
        }
        frame(&mut app, press);
        let mut release = pointer(at, false);
        match poison {
            "text" => release.push(egui::Event::Text("x".into())),
            "paste" => release.push(egui::Event::Paste("x".into())),
            "key" => release.extend(key(egui::Key::ArrowDown, egui::Modifiers::NONE)),
            "wheel" => release.push(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, 1.0),
                modifiers: egui::Modifiers::NONE,
            }),
            "preedit" => release.push(egui::Event::Ime(egui::ImeEvent::Preedit("x".into()))),
            "empty_preedit" => {
                release.push(egui::Event::Ime(egui::ImeEvent::Preedit(String::new())))
            }
            "commit" => release.push(egui::Event::Ime(egui::ImeEvent::Commit("x".into()))),
            "empty_commit" => release.push(egui::Event::Ime(egui::ImeEvent::Commit(String::new()))),
            "prior_composition" => release.push(egui::Event::Ime(egui::ImeEvent::Disabled)),
            _ => {}
        }
        frame(&mut app, release);
        settle(&mut app);
        assert!(!app.file_structure_open(), "{poison}");
        assert!(
            app.language.features.file_structure.menu.is_none(),
            "{poison}"
        );
        assert!(rx.try_recv().is_err(), "{poison}");
    }
}

#[test]
fn file_structure_menu_pending_identity_and_missing_raw_expire_without_resurrection() {
    for change in [
        "generation",
        "session",
        "document",
        "edit",
        "navigation",
        "foreign_modal",
        "foreign_focus",
        "navigation_dialog",
        "dismiss",
        "escape",
        "mixed_escape",
        "new_pointer",
        "focus_loss",
        "missing",
        "skipped",
    ] {
        let (mut app, rx) = app();
        seed(&mut app, symbols());
        settle(&mut app);
        queue_menu(&mut app);
        match change {
            "generation" => app.generation += 1,
            "session" => app.language.session += 1,
            "document" => app.active_document = None,
            "edit" => app.documents[0].edit_version += 1,
            "navigation" => app.navigation_epoch += 1,
            "foreign_modal" => app.confirm = Some(crate::Confirm::CloseWindow),
            "foreign_focus" => app
                .editor_ctx
                .memory_mut(|memory| memory.request_focus(egui::Id::new("foreign focus"))),
            "navigation_dialog" => app.show_file_chooser(),
            "dismiss" => app.dismiss_file_structure(),
            "skipped" => {
                let ctx = app.editor_ctx.clone();
                eframe::App::raw_input_hook(&mut app, &ctx, &mut egui::RawInput::default());
            }
            _ => {}
        }
        if change == "missing" {
            let ctx = app.editor_ctx.clone();
            let mut native = eframe::Frame::_new_kittest();
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                eframe::App::update(&mut app, ctx, &mut native)
            });
        } else {
            let events = match change {
                "escape" => key(egui::Key::Escape, egui::Modifiers::NONE),
                "mixed_escape" => {
                    let mut events = key(egui::Key::Escape, egui::Modifiers::NONE);
                    events.push(egui::Event::Text("x".into()));
                    events
                }
                "new_pointer" => pointer(egui::pos2(900.0, 500.0), true),
                "focus_loss" => vec![egui::Event::WindowFocused(false)],
                _ => vec![],
            };
            frame(&mut app, events);
        }
        assert!(!app.file_structure_open(), "{change}");
        assert!(
            app.language.features.file_structure.menu.is_none(),
            "{change}"
        );
        app.confirm = None;
        app.dismiss_navigation();
        settle(&mut app);
        assert!(!app.file_structure_open(), "no resurrection: {change}");
        assert!(rx.try_recv().is_err(), "{change}");
    }
}

#[test]
fn file_structure_menu_unavailable_trust_off_and_uncached_provider_contracts() {
    for case in 0..6 {
        let (mut app, rx) = app();
        match case {
            0 => {
                app.active_form.as_mut().unwrap().allow_run = false;
                app.language.running = false;
            }
            1 => app.language.capabilities = json!({"documentSymbolProvider":false}),
            2 => app.agent_info = None,
            3 => app.documents[0].path = "notes.txt".into(),
            4 => app.active_document = None,
            _ => {}
        }
        settle(&mut app);
        queue_menu(&mut app);
        settle(&mut app);
        assert!(app.file_structure_open());
        assert!(!app.tools_open);
        assert!(app.language.view == View::Problems);
        if case < 5 {
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
            if case == 0 {
                assert!(!app.active_form.as_ref().unwrap().allow_run);
                assert!(!app.language.running);
            }
        } else {
            let query = rx.try_recv().unwrap();
            assert!(matches!(
                query.op,
                Operation::LanguageDocumentSymbols { .. }
            ));
            reply(&mut app, &query, symbols());
            assert_eq!(app.file_structure_rows().len(), 4);
            assert!(rx.try_recv().is_err());
            assert!(app.language.view == View::Problems);
        }
    }
}

#[test]
fn file_structure_menu_discarded_passes_wait_for_native_frame_and_request_once() {
    let (mut app, rx) = app();
    settle(&mut app);
    let at = tools_menu(&mut app).rect.center();
    frame(&mut app, pointer(at, true));
    let ctx = app.editor_ctx.clone();
    let mut raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1100.0, 850.0),
        )),
        events: pointer(at, false),
        ..Default::default()
    };
    eframe::App::raw_input_hook(&mut app, &ctx, &mut raw);
    let mut native = eframe::Frame::_new_kittest();
    let mut passes = 0;
    let output = ctx.run(raw, |ctx| {
        eframe::App::update(&mut app, ctx, &mut native);
        assert!(!app.file_structure_open());
        assert!(app.language.features.file_structure.menu.is_some());
        assert!(rx.try_recv().is_err());
        passes += 1;
        if passes == 1 {
            ctx.request_discard("menu authority sizing retry");
        }
    });
    assert!(output.platform_output.num_completed_passes >= 2);
    let mut raw = egui::RawInput::default();
    eframe::App::raw_input_hook(&mut app, &ctx, &mut raw);
    let mut serial = None;
    let mut passes = 0;
    let output = ctx.run(raw, |ctx| {
        eframe::App::update(&mut app, ctx, &mut native);
        assert!(app.file_structure_open());
        let current = app
            .language
            .features
            .file_structure
            .popup
            .as_ref()
            .unwrap()
            .serial;
        if let Some(serial) = serial {
            assert_eq!(serial, current);
        } else {
            serial = Some(current);
        }
        passes += 1;
        if passes == 1 {
            ctx.request_discard("popup sizing retry");
        }
    });
    assert!(output.platform_output.num_completed_passes >= 2);
    assert!(matches!(
        rx.try_recv().unwrap().op,
        Operation::LanguageDocumentSymbols { .. }
    ));
    assert!(rx.try_recv().is_err());
}

#[test]
fn file_structure_menu_press_identity_changes_cannot_be_repaired_by_release() {
    for change in [
        "generation",
        "session",
        "document",
        "edit",
        "navigation",
        "focus",
        "missing",
    ] {
        let (mut app, rx) = app();
        seed(&mut app, symbols());
        settle(&mut app);
        let at = tools_menu(&mut app).rect.center();
        frame(&mut app, pointer(at, true));
        match change {
            "generation" => app.generation += 1,
            "session" => app.language.session += 1,
            "document" => app.active_document = None,
            "edit" => app.documents[0].edit_version += 1,
            "navigation" => app.navigation_epoch += 1,
            "focus" => app
                .editor_ctx
                .memory_mut(|memory| memory.request_focus(egui::Id::new("foreign focus"))),
            _ => {}
        }
        if change == "missing" {
            let ctx = app.editor_ctx.clone();
            let mut native = eframe::Frame::_new_kittest();
            let _ = ctx.run(
                egui::RawInput {
                    events: pointer(at, false),
                    ..Default::default()
                },
                |ctx| eframe::App::update(&mut app, ctx, &mut native),
            );
        } else {
            frame(&mut app, pointer(at, false));
        }
        settle(&mut app);
        assert!(!app.file_structure_open(), "{change}");
        assert!(
            app.language.features.file_structure.menu.is_none(),
            "{change}"
        );
        assert!(rx.try_recv().is_err(), "{change}");
    }
}

#[test]
fn file_structure_menu_prelayout_has_no_live_authority_and_live_layout_stays_centered() {
    for passes in [1, 2] {
        let (mut app, rx) = app();
        seed(&mut app, symbols());
        app.editor_ctx
            .options_mut(|options| options.max_passes = passes.try_into().unwrap());
        settle(&mut app);
        let departure = selection(&app);
        let at = tools_menu(&mut app).rect.center();
        frame(&mut app, pointer(at, true));
        let focus = app.editor_ctx.memory(|memory| memory.focused());
        let navigation = app.navigation_epoch;
        let sequence = app.language.features.sequence;
        frame(&mut app, pointer(at, false));
        assert!(!app.file_structure_open());
        assert!(app
            .editor_ctx
            .memory(|memory| memory.top_modal_layer().is_none()));
        assert_eq!(app.editor_ctx.memory(|memory| memory.focused()), focus);
        assert_eq!(app.navigation_epoch, navigation);
        assert_eq!(app.language.features.sequence, sequence);
        assert_eq!(selection(&app), departure);
        assert!(rx.try_recv().is_err());
        frame(&mut app, vec![egui::Event::Text("same".into())]);
        assert_eq!(app.file_structure_query(), Some("same"));
        let filter = app
            .language
            .features
            .file_structure
            .popup
            .as_ref()
            .unwrap()
            .filter;
        assert_eq!(filter, 1);
        let first = egui::AreaState::load(&app.editor_ctx, egui::Id::new(MODAL))
            .unwrap()
            .rect();
        frame(&mut app, vec![]);
        let final_rect = egui::AreaState::load(&app.editor_ctx, egui::Id::new(MODAL))
            .unwrap()
            .rect();
        assert!((final_rect.center() - egui::pos2(550.0, 425.0)).length() <= 1.0);
        if passes == 2 {
            assert_eq!(first, final_rect, "discard must settle before presentation");
        }
        assert_eq!(app.file_structure_query(), Some("same"));
        assert_eq!(
            app.language
                .features
                .file_structure
                .popup
                .as_ref()
                .unwrap()
                .filter,
            filter
        );
        assert_eq!(app.documents[0].text, SOURCE);
        assert_eq!(selection(&app), departure);
        assert!(rx.try_recv().is_err());
    }
}

#[test]
fn file_structure_menu_disabled_and_sizing_response_have_no_admission() {
    let (mut app, rx) = app();
    settle(&mut app);
    let at = tools_menu(&mut app).rect.center();
    frame(&mut app, pointer(at, true));
    let ctx = app.editor_ctx.clone();
    let mut raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1100.0, 850.0),
        )),
        events: pointer(at, false),
        ..Default::default()
    };
    eframe::App::raw_input_hook(&mut app, &ctx, &mut raw);
    let mut native = eframe::Frame::_new_kittest();
    let _ = ctx.run(raw, |ctx| {
        eframe::App::update(&mut app, ctx, &mut native);
        let response =
            crate::workspace_access_tests::recorded_response(&app, "file_structure_menu");
        assert!(response.clicked());
        let mut ui = egui::Ui::new(
            ctx.clone(),
            egui::Id::new("menu guard test"),
            egui::UiBuilder::new(),
        );
        assert!(
            crate::workspace_access::claim_menu_activation(&app, &ui, &response),
            "control proves current native clicked response"
        );
        let disabled = ui.new_child(egui::UiBuilder::new().disabled());
        assert!(!crate::workspace_access::claim_menu_activation(
            &app, &disabled, &response
        ));
        let sizing = ui.new_child(egui::UiBuilder::new().sizing_pass());
        assert!(!crate::workspace_access::claim_menu_activation(
            &app, &sizing, &response
        ));
    });
    assert!(!app.file_structure_open());
    assert!(app.language.features.file_structure.menu.is_some());
    assert!(rx.try_recv().is_err());
}

#[test]
fn file_structure_menu_cancelled_prelayout_reopens_with_measured_geometry_and_text() {
    let (mut app, rx) = app();
    seed(&mut app, symbols());
    settle(&mut app);
    queue_menu(&mut app);
    frame(&mut app, key(egui::Key::Escape, egui::Modifiers::NONE));
    assert!(!app.file_structure_open());
    assert!(app.language.features.file_structure.menu.is_none());
    queue_menu(&mut app);
    frame(&mut app, vec![egui::Event::Text("café".into())]);
    assert_eq!(app.file_structure_query(), Some("café"));
    let first = egui::AreaState::load(&app.editor_ctx, egui::Id::new(MODAL))
        .unwrap()
        .rect();
    frame(&mut app, vec![]);
    let next = egui::AreaState::load(&app.editor_ctx, egui::Id::new(MODAL))
        .unwrap()
        .rect();
    assert_eq!(first, next);
    assert!((first.center() - egui::pos2(550.0, 425.0)).length() <= 1.0);
    assert!(rx.try_recv().is_err());
}

#[test]
fn file_structure_menu_identity_exhaustion_never_focuses_missing_filter() {
    let (mut app, rx) = app();
    settle(&mut app);
    app.language.features.file_structure.serial = u64::MAX;
    queue_menu(&mut app);
    frame(&mut app, vec![]);
    assert!(!app.file_structure_open());
    assert!(app.language.features.file_structure.exhausted);
    assert_ne!(
        app.editor_ctx.memory(|memory| memory.focused()),
        Some(egui::Id::new(FILTER))
    );
    assert!(rx.try_recv().is_err());
}

#[test]
fn file_structure_menu_cancelled_prelayout_does_not_move_subsequent_shortcut_popup() {
    let (mut app, rx) = app();
    seed(&mut app, symbols());
    settle(&mut app);
    queue_menu(&mut app);
    frame(&mut app, key(egui::Key::Escape, egui::Modifiers::NONE));
    frame(
        &mut app,
        key(
            egui::Key::F12,
            egui::Modifiers::CTRL | egui::Modifiers::COMMAND,
        ),
    );
    assert!(app.file_structure_open());
    let first = egui::AreaState::load(&app.editor_ctx, egui::Id::new(MODAL))
        .unwrap()
        .rect();
    frame(&mut app, vec![]);
    let next = egui::AreaState::load(&app.editor_ctx, egui::Id::new(MODAL))
        .unwrap()
        .rect();
    assert_eq!(first, next);
    assert!((first.center() - egui::pos2(550.0, 425.0)).length() <= 1.0);
    assert!(rx.try_recv().is_err());
}

#[test]
fn file_structure_menu_first_frame_enter_cannot_activate_an_unpresented_row() {
    let (mut app, rx) = app();
    seed(&mut app, symbols());
    settle(&mut app);
    let departure = selection(&app);
    queue_menu(&mut app);
    frame(&mut app, key(egui::Key::Enter, egui::Modifiers::NONE));
    assert!(app.file_structure_open());
    assert!(!app.file_structure_activation_pending());
    assert_eq!(selection(&app), departure);
    assert!(rx.try_recv().is_err());
}

#[test]
fn file_structure_menu_full_scroll_list_has_stable_narrow_and_ordinary_geometry() {
    for size in [egui::vec2(480.0, 540.0), egui::vec2(1100.0, 850.0)] {
        let (mut app, rx) = app();
        let source_range = range_json(range(SOURCE, "same", false));
        seed(
            &mut app,
            Value::Array(
                (0..60)
                    .map(|index| {
                        json!({
                            "name": format!("symbol{index:02}"), "kind":6,
                            "range": source_range, "selectionRange": source_range
                        })
                    })
                    .collect(),
            ),
        );
        for _ in 0..3 {
            frame_sized(&mut app, vec![], size);
        }
        let root = crate::workspace_access_tests::recorded_rect(&app, "tools_menu").center();
        frame_sized(&mut app, pointer(root, true), size);
        frame_sized(&mut app, pointer(root, false), size);
        for _ in 0..3 {
            frame_sized(&mut app, vec![], size);
        }
        let item =
            crate::workspace_access_tests::recorded_rect(&app, "file_structure_menu").center();
        frame_sized(&mut app, pointer(item, true), size);
        frame_sized(&mut app, pointer(item, false), size);
        assert!(app.language.features.file_structure.menu.is_some());
        frame_sized(&mut app, vec![egui::Event::Text("symbol".into())], size);
        assert_eq!(app.file_structure_query(), Some("symbol"));
        assert_eq!(app.file_structure_rows().len(), 60);
        let filter = crate::workspace_access_tests::recorded_rect(&app, FILTER);
        assert!(egui::Rect::from_min_size(egui::Pos2::ZERO, size).contains_rect(filter));
        let first = egui::AreaState::load(&app.editor_ctx, egui::Id::new(MODAL))
            .unwrap()
            .rect();
        frame_sized(&mut app, vec![], size);
        let next = egui::AreaState::load(&app.editor_ctx, egui::Id::new(MODAL))
            .unwrap()
            .rect();
        assert_eq!(first, next, "viewport {size:?}");
        assert!((first.center() - egui::Pos2::new(size.x / 2.0, size.y / 2.0)).length() <= 1.0);
        assert!(rx.try_recv().is_err());
    }
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
    for menu in [false, true] {
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
        if menu {
            queue_menu(&mut app);
            settle(&mut app);
        } else {
            open(&mut app);
        }
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
