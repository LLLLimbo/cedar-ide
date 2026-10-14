use super::*;
use crate::{
    editor_state,
    model::Document,
    worker::{Command as WorkerCommand, Worker},
};
use std::sync::mpsc::Receiver;

fn cursor(primary: usize, secondary: usize) -> CCursorRange {
    CCursorRange::two(
        egui::text::CCursor::new(secondary),
        egui::text::CCursor::new(primary),
    )
}
fn set_selection(app: &mut CedarApp, range: CCursorRange) {
    let doc = app
        .documents
        .iter_mut()
        .find(|doc| Some(doc.id) == app.active_document)
        .unwrap();
    let mut state = editor_state::load(&app.editor_ctx, doc);
    state.cursor.set_char_range(Some(range));
    state.store(&app.editor_ctx, egui::Id::new(("editor", doc.id)));
}
fn live(app: &CedarApp) -> CCursorRange {
    selection(&app.editor_ctx, app.active_document.unwrap()).unwrap()
}
fn editor_focus(app: &CedarApp) {
    app.editor_ctx.memory_mut(|memory| {
        memory.request_focus(egui::Id::new(("editor", app.active_document.unwrap())))
    });
}
fn query_focus(app: &CedarApp) {
    app.editor_ctx
        .memory_mut(|memory| memory.request_focus(egui::Id::new(replace::FIND_INPUT)));
}
fn app(text: &str) -> (CedarApp, Receiver<WorkerCommand>) {
    let mut app = CedarApp::empty();
    // Match production startup while retaining the real asynchronous fallback.
    app.editor_ctx.set_fonts(crate::system_fonts::base_fonts());
    app.open_form = false;
    app.documents
        .push(Document::new(1, "main.rs".into(), text.into(), "r0".into()));
    app.active_document = Some(1);
    app.next_document = 2;
    set_selection(&mut app, cursor(0, 0));
    editor_focus(&app);
    let (worker, rx) = Worker::recording();
    app.worker = Some(worker);
    settle(&mut app);
    (app, rx)
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
fn frame(app: &mut CedarApp, events: Vec<egui::Event>) -> egui::FullOutput {
    frame_hook(app, events, true, false)
}
fn frame_hook(
    app: &mut CedarApp,
    events: Vec<egui::Event>,
    hook: bool,
    discard: bool,
) -> egui::FullOutput {
    let ctx = app.editor_ctx.clone();
    let mut raw = egui::RawInput {
        time: Some(ctx.input(|input| input.time) + 0.05),
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1100.0, 850.0),
        )),
        events,
        ..Default::default()
    };
    if hook {
        eframe::App::raw_input_hook(app, &ctx, &mut raw);
    }
    let mut native = eframe::Frame::_new_kittest();
    ctx.run(raw, |ctx| {
        eframe::App::update(app, ctx, &mut native);
        if discard && ctx.output(|output| output.num_completed_passes) == 0 {
            ctx.request_discard("Find multipass witness");
        }
    })
}
fn settle(app: &mut CedarApp) {
    for _ in 0..3 {
        frame(app, vec![]);
    }
}
fn open(app: &mut CedarApp) {
    frame(app, key(egui::Key::F, egui::Modifiers::COMMAND));
    settle(app);
    assert!(app.find_open);
}
fn find(app: &mut CedarApp, query: &str) {
    app.find_query = query.into();
    open(app);
}
fn step(app: &mut CedarApp, previous: bool) {
    frame(
        app,
        key(
            egui::Key::F3,
            if previous {
                egui::Modifiers::SHIFT
            } else {
                egui::Modifiers::NONE
            },
        ),
    );
}
fn button(app: &mut CedarApp, name: &str) {
    let at = crate::workspace_access_tests::recorded_rect(app, name).center();
    frame(app, pointer(at, true));
    frame(app, pointer(at, false));
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

#[test]
fn caret_relative_boundaries_exact_reversed_hits_adjacent_and_literal_ranges() {
    let hits = vec![2..4, 4..6, 9..11];
    for (selection, next, previous) in [
        (cursor(0, 0), 0, 2),
        (cursor(2, 2), 0, 2),
        (cursor(3, 3), 1, 2),
        (cursor(4, 4), 1, 0),
        (cursor(6, 6), 2, 1),
        (cursor(12, 12), 0, 2),
        (cursor(4, 2), 1, 2),
        (cursor(2, 4), 1, 2),
        (cursor(3, 8), 2, 2),
    ] {
        assert_eq!(target(&hits, selection, Direction::Next), Some(next));
        assert_eq!(
            target(&hits, selection, Direction::Previous),
            Some(previous)
        );
    }
    assert_eq!(
        model::find_ranges("aaa .* 雪😀 .*", ".*"),
        vec![4..6, 10..12]
    );
    assert_eq!(model::find_ranges("aaaaa", "aa"), vec![0..2, 2..4]);
    assert_eq!(target(&[], cursor(0, 0), Direction::Next), None);
}

#[test]
fn selection_seed_is_whole_unicode_bounded_and_never_moves_the_caret() {
    for (text, selected, expected, message) in [
        ("é😀雪 x".to_owned(), cursor(0, 3), "é😀雪", false),
        ("first\r\nsecond".to_owned(), cursor(0, 8), "old", true),
        ("a\0b".to_owned(), cursor(0, 3), "old", true),
        ("😀".repeat(1025), cursor(0, 1025), "old", true),
        ("ok".to_owned(), cursor(1, 1), "old", false),
    ] {
        let (mut app, rx) = app(&text);
        app.find_query = "old".into();
        set_selection(&mut app, selected);
        open(&mut app);
        assert_eq!(app.find_query, expected);
        assert!(same_selection(Some(live(&app)), Some(selected)));
        assert_eq!(app.file_find.message.is_some(), message);
        assert_eq!(app.documents[0].text, text);
        assert!(app.location_history.back.is_empty());
        assert!(rx.try_recv().is_err());
    }
    let (mut app, _) = app(&"😀".repeat(1024));
    set_selection(&mut app, cursor(1024, 0));
    open(&mut app);
    assert_eq!(app.find_query.len(), 4096);
}

#[test]
fn actual_find_journey_unifies_enter_buttons_f3_and_one_back_departure() {
    let (mut app, rx) = app("é😀 cat\r\ncat cat");
    let origin = cursor(1, 1);
    set_selection(&mut app, origin);
    find(&mut app, "cat");
    let count = app.file_find_count(&app.editor_ctx.clone());
    assert_eq!(count, "3 matches");
    frame(&mut app, key(egui::Key::Enter, egui::Modifiers::NONE));
    assert_eq!(live(&app).sorted().map(|c| c.index), [3, 6]);
    button(&mut app, "find_next");
    assert_eq!(live(&app).sorted().map(|c| c.index), [8, 11]);
    step(&mut app, false);
    assert_eq!(live(&app).sorted().map(|c| c.index), [12, 15]);
    button(&mut app, "find_previous");
    assert_eq!(live(&app).sorted().map(|c| c.index), [8, 11]);
    assert_eq!(
        app.file_find_count(&app.editor_ctx.clone()),
        "2 of 3 matches"
    );
    assert_eq!(app.location_history.back.len(), 1);
    assert!(same_selection(
        Some(app.location_history.back[0].selection),
        Some(origin)
    ));
    frame(&mut app, key(egui::Key::Escape, egui::Modifiers::NONE));
    settle(&mut app);
    assert!(!app.find_open);
    frame(
        &mut app,
        key(egui::Key::OpenBracket, egui::Modifiers::COMMAND),
    );
    settle(&mut app);
    assert!(same_selection(Some(live(&app)), Some(origin)));
    frame(
        &mut app,
        key(egui::Key::CloseBracket, egui::Modifiers::COMMAND),
    );
    settle(&mut app);
    assert_eq!(live(&app).sorted().map(|c| c.index), [8, 11]);
    assert_eq!(app.documents[0].text, "é😀 cat\r\ncat cat");
    assert_eq!(app.documents[0].edit_version, 0);
    assert!(rx.try_recv().is_err());
}

#[test]
fn one_hit_self_wrap_preserves_affinity_direction_focus_and_history() {
    let (mut app, _) = app("cat");
    app.find_query = "cat".into();
    open(&mut app);
    let mut selected = cursor(0, 3);
    selected.primary.prefer_next_row = true;
    selected.secondary.prefer_next_row = false;
    set_selection(&mut app, selected);
    query_focus(&app);
    let epoch = app.navigation_epoch;
    step(&mut app, false);
    step(&mut app, true);
    assert!(same_selection(Some(live(&app)), Some(selected)));
    assert_eq!(app.navigation_epoch, epoch);
    assert!(app.location_history.back.is_empty());
    assert_eq!(
        app.editor_ctx.memory(|m| m.focused()),
        Some(egui::Id::new(replace::FIND_INPUT))
    );
}

#[test]
fn query_typing_tab_edit_and_external_navigation_end_a_loop_without_ordinal_leak() {
    let (mut app, _) = app("cat cat dog dog");
    find(&mut app, "cat");
    step(&mut app, false);
    assert_eq!(live(&app).sorted().map(|c| c.index), [0, 3]);
    query_focus(&app);
    frame(&mut app, key(egui::Key::A, egui::Modifiers::COMMAND));
    let before = live(&app);
    frame(&mut app, vec![egui::Event::Text("dog".into())]);
    assert_eq!(live(&app), before);
    assert!(app.file_find.journey.is_none());
    step(&mut app, false);
    assert_eq!(live(&app).sorted().map(|c| c.index), [8, 11]);
    assert_eq!(app.location_history.back.len(), 2);
    app.documents.push(Document::new(
        2,
        "second.rs".into(),
        "dog first dog".into(),
        "r1".into(),
    ));
    app.activate_history_tab(2);
    settle(&mut app);
    set_selection(&mut app, cursor(4, 4));
    editor_focus(&app);
    step(&mut app, false);
    assert_eq!(live(&app).sorted().map(|c| c.index), [10, 13]);
    app.documents[1].text = "dog dog".into();
    app.documents[1].edit_version += 1;
    set_selection(&mut app, cursor(0, 0));
    step(&mut app, false);
    assert_eq!(live(&app).sorted().map(|c| c.index), [0, 3]);
    let count = app.location_history.back.len();
    app.navigation_changed();
    step(&mut app, false);
    assert_eq!(app.location_history.back.len(), count + 1);
}

#[test]
fn retained_cap_and_live_ordinal_are_truthful_and_no_match_preserves_selection() {
    let (mut app, _) = app(&"雪 ".repeat(10001));
    find(&mut app, "雪");
    assert_eq!(
        app.file_find_count(&app.editor_ctx.clone()),
        "First 10000 matches retained"
    );
    set_selection(&mut app, cursor(20000, 20000));
    step(&mut app, false);
    assert_eq!(live(&app).sorted().map(|c| c.index), [0, 1]);
    assert_eq!(
        app.file_find_count(&app.editor_ctx.clone()),
        "1 of first 10000 matches retained"
    );
    app.find_query = "NO MATCH".into();
    let before = live(&app);
    let back = app.location_history.back.len();
    step(&mut app, false);
    assert_eq!(live(&app), before);
    assert_eq!(app.location_history.back.len(), back);
    assert_eq!(app.file_find_count(&app.editor_ctx.clone()), "0 matches");
}

#[test]
fn original_mixed_ime_foreign_release_repeated_and_missing_capture_do_not_navigate() {
    let competing = vec![
        egui::Event::Text("x".into()),
        egui::Event::Paste("x".into()),
        egui::Event::Ime(egui::ImeEvent::Preedit(String::new())),
        egui::Event::Ime(egui::ImeEvent::Commit(String::new())),
        key(egui::Key::F8, egui::Modifiers::NONE).remove(1),
        egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0., 1.),
            modifiers: egui::Modifiers::NONE,
        },
        egui::Event::WindowFocused(false),
    ];
    for event in competing {
        let (mut app, _) = app("cat cat");
        find(&mut app, "cat");
        let before = live(&app);
        let epoch = app.navigation_epoch;
        let mut events = key(egui::Key::F3, egui::Modifiers::NONE);
        events.push(event);
        frame(&mut app, events);
        assert_eq!(app.navigation_epoch, epoch);
        assert_eq!(live(&app), before);
        assert!(app.location_history.back.is_empty());
    }
    let (mut app, _) = app("cat cat");
    find(&mut app, "cat");
    let before = live(&app);
    let mut repeated = key(egui::Key::F3, egui::Modifiers::NONE);
    if let egui::Event::Key { repeat, .. } = &mut repeated[1] {
        *repeat = true;
    }
    frame(&mut app, repeated);
    assert_eq!(live(&app), before);
    frame_hook(
        &mut app,
        key(egui::Key::F3, egui::Modifiers::NONE),
        false,
        false,
    );
    assert_eq!(live(&app), before);
}

#[test]
fn held_and_composing_lifetimes_require_a_new_clean_gesture() {
    let (mut app, _) = app("cat cat cat");
    find(&mut app, "cat");
    frame(
        &mut app,
        vec![key(egui::Key::F3, egui::Modifiers::NONE).remove(0)],
    );
    assert_eq!(live(&app).sorted().map(|c| c.index), [0, 3]);
    frame(
        &mut app,
        vec![key(egui::Key::F3, egui::Modifiers::NONE).remove(0)],
    );
    assert_eq!(live(&app).sorted().map(|c| c.index), [0, 3]);
    frame(
        &mut app,
        vec![key(egui::Key::F3, egui::Modifiers::NONE).remove(1)],
    );
    step(&mut app, false);
    assert_eq!(live(&app).sorted().map(|c| c.index), [4, 7]);
    query_focus(&app);
    frame(
        &mut app,
        vec![egui::Event::Ime(egui::ImeEvent::Preedit(String::new()))],
    );
    let epoch = app.navigation_epoch;
    step(&mut app, false);
    assert_eq!(app.navigation_epoch, epoch);
    frame(&mut app, vec![egui::Event::Ime(egui::ImeEvent::Disabled)]);
    step(&mut app, false);
    assert_eq!(live(&app).sorted().map(|c| c.index), [8, 11]);
}

#[test]
fn button_owner_stale_or_mixed_release_cannot_move_and_multipass_moves_once() {
    let (mut app, _) = app("cat cat cat");
    find(&mut app, "cat");
    let at = crate::workspace_access_tests::recorded_rect(&app, "find_next").center();
    frame(&mut app, pointer(at, true));
    app.find_query = "dog".into();
    frame(&mut app, pointer(at, false));
    assert!(app.location_history.back.is_empty());
    app.find_query = "cat".into();
    settle(&mut app);
    let at = crate::workspace_access_tests::recorded_rect(&app, "find_next").center();
    frame(&mut app, pointer(at, true));
    let mut release = pointer(at, false);
    release.push(egui::Event::Ime(egui::ImeEvent::Commit(String::new())));
    frame(&mut app, release);
    assert!(app.location_history.back.is_empty());
    editor_focus(&app);
    frame_hook(
        &mut app,
        key(egui::Key::F3, egui::Modifiers::NONE),
        true,
        true,
    );
    assert_eq!(live(&app).sorted().map(|c| c.index), [0, 3]);
    assert_eq!(app.location_history.back.len(), 1);
}

#[test]
fn foreign_fields_and_saturated_versions_cannot_navigate() {
    let (mut app, _) = app("cat cat");
    find(&mut app, "cat");
    app.editor_ctx
        .memory_mut(|m| m.request_focus(egui::Id::new("literal_replace_text")));
    step(&mut app, false);
    assert!(app.location_history.back.is_empty());
    editor_focus(&app);
    app.documents[0].edit_version = u64::MAX;
    step(&mut app, false);
    assert!(app.location_history.back.is_empty());
    assert!(app.file_find.snapshot.is_none());
    assert!(app.file_find.message.unwrap().contains("exhausted"));
}

#[test]
fn manual_query_and_draft_size_preserve_existing_literal_behavior() {
    let source = "x".repeat(cedar_protocol::MAX_FILE_BYTES + 1);
    let (mut app, _) = app("small");
    app.documents[0].text = source.clone();
    app.documents[0].edit_version += 1;
    app.find_query = source;
    let ctx = app.editor_ctx.clone();
    app.sync_file_find(&ctx);
    assert_eq!(app.file_find_count(&ctx), "1 matches");
    app.step_file_find(&ctx, Direction::Next);
    assert_eq!(
        live(&app).sorted().map(|c| c.index),
        [0, cedar_protocol::MAX_FILE_BYTES + 1]
    );
}

#[test]
fn invalid_scalar_selection_is_not_clamped_into_a_different_seed_or_navigation() {
    let (mut app, _) = app("é😀");
    app.find_query = "old".into();
    let selected = cursor(0, 9);
    set_selection(&mut app, selected);
    let ctx = app.editor_ctx.clone();
    app.open_file_find(&ctx, true);
    assert_eq!(app.find_query, "old");
    assert_eq!(live(&app), selected);
    assert!(app.file_find.message.unwrap().contains("outside"));
    app.find_query = "é".into();
    app.step_file_find(&ctx, Direction::Next);
    assert_eq!(live(&app), selected);
    assert!(app.location_history.back.is_empty());
}

#[test]
fn manual_selection_affinities_end_the_loop_and_housekeeping_pointer_is_admitted() {
    let (mut app, _) = app("cat cat cat");
    find(&mut app, "cat");
    step(&mut app, false);
    let mut manual = cursor(4, 4);
    manual.primary.prefer_next_row = true;
    manual.secondary.prefer_next_row = false;
    set_selection(&mut app, manual);
    settle(&mut app);
    assert!(app.file_find.journey.is_none());
    let at = crate::workspace_access_tests::recorded_rect(&app, "find_next").center();
    let mut press = pointer(at, true);
    press.push(egui::Event::Ime(egui::ImeEvent::Disabled));
    frame(&mut app, press);
    let mut release = pointer(at, false);
    release.push(egui::Event::Ime(egui::ImeEvent::Enabled));
    frame(&mut app, release);
    assert_eq!(live(&app).sorted().map(|c| c.index), [4, 7]);
    assert_eq!(app.location_history.back.len(), 2);
    assert!(same_selection(
        Some(app.location_history.back[1].selection),
        Some(manual)
    ));
}

#[test]
fn ready_read_cannot_preempt_current_find_open_or_step_or_replay_on_idle() {
    for action in 0..4 {
        let (mut app, rx) = app("cat cat");
        app.state = ConnectionState::Ready;
        app.agent_info = Some(crate::agent_support::full_test_agent());
        if action != 0 {
            find(&mut app, "cat");
        }
        editor_focus(&app);
        if action == 0 {
            set_selection(&mut app, cursor(0, 3));
        }
        if action == 2 {
            app.find_query = "missing".into();
        }
        let before = live(&app);
        app.open("late.rs".into(), None);
        let request = rx.try_recv().unwrap();
        app.result_tx
            .send(crate::worker::WorkerEvent::Response(crate::worker::Event {
                id: request.id,
                generation: app.generation,
                connected: true,
                result: Ok(cedar_protocol::Payload::File {
                    path: "late.rs".into(),
                    text: "late".into(),
                    revision: "r".into(),
                }),
            }))
            .unwrap();
        let mut events = if action == 0 {
            key(egui::Key::F, egui::Modifiers::COMMAND)
        } else {
            key(egui::Key::F3, egui::Modifiers::NONE)
        };
        if action == 3 {
            events.push(egui::Event::Ime(egui::ImeEvent::Commit(String::new())));
        }
        frame(&mut app, events);
        assert_eq!(app.active_document, Some(1));
        assert!(app.documents.iter().any(|doc| doc.path == "late.rs"));
        if action == 0 {
            assert!(app.find_open);
            assert_eq!(app.find_query, "cat");
            assert_eq!(live(&app), before);
        }
        if action == 1 {
            assert_eq!(live(&app).sorted().map(|c| c.index), [0, 3]);
            assert_eq!(app.location_history.back.len(), 1);
        } else {
            assert_eq!(live(&app), before);
            assert!(app.location_history.back.is_empty());
        }
        let epoch = app.navigation_epoch;
        let selected = live(&app);
        settle(&mut app);
        assert_eq!(app.navigation_epoch, epoch);
        assert_eq!(live(&app), selected);
        assert!(rx.try_recv().is_err());
    }
}

#[test]
fn accepted_field_enter_cannot_leak_after_capture_into_a_new_modal_or_focus_owner() {
    for interruption in 0..4 {
        let (mut app, _) = app("cat cat");
        find(&mut app, "cat");
        let ctx = app.editor_ctx.clone();
        let before = live(&app);
        let mut raw = egui::RawInput {
            time: Some(ctx.input(|input| input.time) + 0.05),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1100., 850.),
            )),
            events: key(egui::Key::Enter, egui::Modifiers::NONE),
            ..Default::default()
        };
        eframe::App::raw_input_hook(&mut app, &ctx, &mut raw);
        assert!(app.file_find.key.is_some());
        match interruption {
            0 => app.confirm = Some(crate::Confirm::CloseWindow),
            1 => {
                ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("literal_replace_text")))
            }
            2 => app.navigation_changed(),
            _ => app.generation += 1,
        }
        let mut native = eframe::Frame::_new_kittest();
        let _ = ctx.run(raw, |ctx| {
            eframe::App::update(&mut app, ctx, &mut native);
            assert!(!ctx.input(|input| input.events.iter().any(|event| matches!(
                event,
                egui::Event::Key {
                    key: egui::Key::Enter,
                    ..
                }
            ))));
        });
        assert_eq!(live(&app), before);
        assert!(app.location_history.back.is_empty());
        assert_eq!(app.documents[0].text, "cat cat");
        assert!(!app.allow_close);
        if interruption == 0 {
            assert!(matches!(app.confirm, Some(crate::Confirm::CloseWindow)));
        }
    }
}

#[test]
fn mixed_ctrl_f_does_not_open_or_steal_input_from_its_original_owner() {
    let (mut app, _) = app("cat cat");
    app.find_query = "retained".into();
    let mut events = key(egui::Key::F, egui::Modifiers::COMMAND);
    events.push(egui::Event::Text("!".into()));
    frame(&mut app, events);
    assert!(!app.find_open);
    assert_eq!(app.find_query, "retained");
    assert_eq!(app.documents[0].text, "!cat cat");
    assert!(app.location_history.back.is_empty());
}
