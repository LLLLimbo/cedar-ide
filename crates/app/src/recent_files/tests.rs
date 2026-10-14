use super::*;

// Geometry only: never retain Response, Context, Ui, or ScrollAreaOutput here.
#[derive(Clone)]
pub(super) struct Geometry {
    pub surface: egui::Rect,
    pub rows: egui::Rect,
    pub status: egui::Rect,
    pub headline: egui::Rect,
    pub details: egui::Rect,
    pub path: egui::Rect,
    pub rows_id: egui::Id,
    pub status_id: egui::Id,
    pub rows_content: egui::Vec2,
    pub status_content: egui::Vec2,
    pub visible: bool,
    pub row_height: f32,
}

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

use crate::workspace_access_tests::{recorded_clip, recorded_rect, recorded_response};

fn visual_app(count: usize, font: f32) -> (CedarApp, Receiver<Command>) {
    let (mut app, commands) = app();
    app.editor_ctx.set_fonts(crate::system_fonts::base_fonts());
    app.editor_ctx.style_mut(|style| {
        style.spacing.item_spacing = egui::vec2(6.0, 4.0);
        style.spacing.button_padding = egui::vec2(8.0, 4.0);
        for text in [egui::TextStyle::Body, egui::TextStyle::Button] {
            style
                .text_styles
                .insert(text, egui::FontId::proportional(font));
        }
        style.text_styles.insert(
            egui::TextStyle::Monospace,
            egui::FontId::monospace(font + 1.0),
        );
    });
    if count == 0 {
        app.documents.clear();
        app.active_document = None;
    }
    visual_frame(&mut app, egui::vec2(1178.0, 814.0), 0.01, vec![]);
    for n in 1..count {
        app.recent_files.touch(&format!("parent/file{n:03}.rs"));
    }
    assert_eq!(app.recent_files.paths.len(), count);
    (app, commands)
}

fn visual_frame(
    app: &mut CedarApp,
    size: egui::Vec2,
    dt: f64,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    visual_frame_with_discard(app, size, dt, events, false)
}
fn visual_frame_with_discard(
    app: &mut CedarApp,
    size: egui::Vec2,
    dt: f64,
    events: Vec<egui::Event>,
    discard_first: bool,
) -> egui::FullOutput {
    let ctx = app.editor_ctx.clone();
    let mut raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
        time: Some(ctx.input(|input| input.time) + dt),
        events,
        ..Default::default()
    };
    eframe::App::raw_input_hook(app, &ctx, &mut raw);
    let mut native = eframe::Frame::_new_kittest();
    let mut first_pass = true;
    ctx.run(raw, |ctx| {
        if discard_first && first_pass {
            ctx.request_discard("Recent presentation discard boundary");
        }
        first_pass = false;
        let smooth_input = ctx.input(|input| input.smooth_scroll_delta);
        ctx.data_mut(|data| {
            data.insert_temp(egui::Id::new("recent_smooth_input_before_ui"), smooth_input)
        });
        eframe::App::update(app, ctx, &mut native);
        // Context swaps its this/previous widget maps at end_pass. Read actual
        // current Responses inside update's pass, not the preceding sizing pass
        // that read_response may prefer after Context::run has returned.
        let mut flags = std::collections::HashMap::<egui::Id, [bool; 4]>::new();
        if let Some(view) = &app.recent_files.view {
            let probes = [
                "recent_query",
                "recent_open",
                "recent_clear",
                "recent_close",
            ]
            .map(str::to_owned)
            .into_iter()
            .chain(
                view.rows
                    .iter()
                    .filter(|row| matches_query(&row.path, &view.query))
                    .map(|row| format!("recent_row:{}", row.path)),
            );
            for probe in probes {
                let response = recorded_response(app, &probe);
                flags.insert(
                    response.id,
                    [
                        response.enabled(),
                        response.is_pointer_button_down_on(),
                        response.double_clicked(),
                        response.triple_clicked(),
                    ],
                );
            }
        }
        ctx.data_mut(|data| data.insert_temp(egui::Id::new("recent_actual_response_flags"), flags));
    })
}
fn visual_flags(app: &CedarApp, probe: &str) -> [bool; 4] {
    let id = recorded_response(app, probe).id;
    app.editor_ctx.data(|data| {
        data.get_temp::<std::collections::HashMap<egui::Id, [bool; 4]>>(egui::Id::new(
            "recent_actual_response_flags",
        ))
        .unwrap()[&id]
    })
}

fn geometry(app: &CedarApp) -> Geometry {
    app.editor_ctx
        .data(|data| data.get_temp(egui::Id::new("recent_geometry")))
        .unwrap()
}
fn visual_enter(app: &mut CedarApp, size: egui::Vec2) -> egui::FullOutput {
    let mut output = visual_frame(app, size, 0.01, key(egui::Key::E, egui::Modifiers::COMMAND));
    // Only egui's explicitly invisible Area sizing pass may be skipped.
    // Never settle away a visible layout or font-install failure.
    if !geometry(app).visible {
        output = visual_frame(app, size, 0.01, vec![]);
    }
    assert!(
        geometry(app).visible,
        "first actual visible pass must be observed"
    );
    assert_eq!(
        app.editor_ctx.memory(|memory| memory.focused()),
        Some(egui::Id::new(QUERY))
    );
    output
}
fn visual_click(app: &mut CedarApp, size: egui::Vec2, probe: &str) -> egui::FullOutput {
    let rect = recorded_rect(app, probe);
    let visible = rect.intersect(recorded_clip(app, probe));
    assert!(visible.is_positive() && app.editor_ctx.screen_rect().contains_rect(visible));
    assert!(visual_flags(app, probe)[0]);
    let pos = visible.center();
    let pointer = |pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    visual_frame(app, size, 0.01, vec![egui::Event::PointerMoved(pos)]);
    assert_eq!(
        recorded_rect(app, probe),
        rect,
        "hover cannot move its control"
    );
    let pre_hit = app.editor_ctx.viewport(|v| {
        v.prev_pass
            .widgets
            .layers()
            .flat_map(|(_, rows)| rows.iter())
            .filter(|r| r.interact_rect.contains(pos))
            .copied()
            .collect::<Vec<_>>()
    });
    visual_frame(app, size, 0.01, vec![pointer(true)]);
    if !visual_flags(app, probe)[1] {
        eprintln!("RECENT_PRE_HIT {pre_hit:?}");
        eprintln!("RECENT_HIT_MISS probe={probe} id={:?} pos={pos:?} snapshot={:?} widgets={:?} presented={:?}",
            recorded_response(app,probe).id,
            app.editor_ctx.interaction_snapshot(|s| (s.clicked, s.dragged, s.hovered.clone(), s.contains_pointer.clone())),
            app.editor_ctx.viewport(|v| v.prev_pass.widgets.layers().flat_map(|(_, rows)| rows.iter())
                .filter(|r| r.interact_rect.contains(pos)).copied().collect::<Vec<_>>()),
            app.recent_files.input.presented.iter().filter(|p| p.rect.contains(pos)).map(|p| (&p.target,p.id,p.rect)).collect::<Vec<_>>());
    }
    assert!(visual_flags(app, probe)[1]);
    visual_frame(app, size, 0.01, vec![pointer(false)])
}
fn visual_edit(app: &mut CedarApp, size: egui::Vec2, text: &str) -> egui::FullOutput {
    visual_click(app, size, "recent_query");
    visual_frame(app, size, 0.01, key(egui::Key::A, egui::Modifiers::COMMAND));
    visual_frame(app, size, 0.01, vec![egui::Event::Text(text.into())])
}
fn visual_footer(app: &CedarApp) -> [egui::Rect; 3] {
    ["recent_open", "recent_clear", "recent_close"].map(|probe| {
        let rect = recorded_rect(app, probe);
        assert!(
            app.editor_ctx.screen_rect().contains_rect(rect),
            "{probe}: {rect:?}"
        );
        assert!(
            recorded_clip(app, probe).contains_rect(rect),
            "{probe} clip"
        );
        rect
    })
}
#[derive(Debug)]
struct Paint {
    text: String,
    rect: egui::Rect,
    clip: egui::Rect,
    font: f32,
    rows: usize,
    color: egui::Color32,
}
fn painted(output: &egui::FullOutput) -> Vec<Paint> {
    fn visit(shape: &egui::epaint::Shape, clip: egui::Rect, out: &mut Vec<Paint>) {
        match shape {
            egui::epaint::Shape::Text(text) => out.push(Paint {
                text: text.galley.text().into(),
                rect: text.galley.rect.translate(text.pos.to_vec2()),
                clip,
                font: text
                    .galley
                    .job
                    .sections
                    .iter()
                    .map(|s| s.format.font_id.size)
                    .fold(f32::INFINITY, f32::min),
                rows: text.galley.rows.len(),
                color: text.fallback_color,
            }),
            egui::epaint::Shape::Vec(shapes) => {
                for shape in shapes {
                    visit(shape, clip, out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for shape in &output.shapes {
        visit(&shape.shape, shape.clip_rect, &mut out);
    }
    out
}
fn assert_painted(output: &egui::FullOutput, expected: &str, band: egui::Rect, font: f32) {
    let paints = painted(output);
    assert!(
        paints.iter().any(|p| p.text == expected
            && p.rect.is_positive()
            && band.expand(0.6).contains_rect(p.rect)
            && p.clip.expand(0.6).contains_rect(p.rect)
            && p.font >= font),
        "missing complete {expected:?} in {band:?}: {paints:?}"
    );
}
fn visual_layout(
    app: &CedarApp,
    output: &egui::FullOutput,
    font: f32,
    stage: &str,
) -> [egui::Rect; 3] {
    let g = geometry(app);
    let screen = app.editor_ctx.screen_rect();
    let footer = visual_footer(app);
    assert!(
        g.visible && g.surface.is_positive() && screen.contains_rect(g.surface),
        "{stage}: {:?}",
        g.surface
    );
    for rect in [g.rows, g.status, g.headline, g.details, g.path] {
        assert!(
            rect.is_positive() && g.surface.contains_rect(rect),
            "{stage}: {rect:?}"
        );
    }
    assert!(g.status.contains_rect(g.headline) && g.status.contains_rect(g.details));
    assert!(g.headline.bottom() <= g.details.top() && g.status.bottom() < g.rows.top());
    assert!(g.rows.bottom() < g.path.top() && g.path.bottom() < footer[0].top());
    let query = recorded_rect(app, "recent_query");
    assert!(screen.contains_rect(query) && recorded_clip(app, "recent_query").contains_rect(query));
    assert!(query.bottom() < g.status.top());
    for (probe, expected) in [("recent_clear", "Clear recent"), ("recent_close", "Close")] {
        assert_painted(output, expected, recorded_rect(app, probe), font);
    }
    let open = if app
        .recent_files
        .view
        .as_ref()
        .unwrap()
        .selected
        .as_ref()
        .is_some_and(|path| {
            app.recent_files
                .view
                .as_ref()
                .unwrap()
                .rows
                .iter()
                .any(|r| &r.path == path && r.document.is_none())
        }) {
        "Open closed file"
    } else {
        "Open"
    };
    assert_painted(output, open, footer[0], font);
    let paints = painted(output);
    let title = paints
        .iter()
        .find(|p| p.text == "Recent files" && p.rect.top() >= g.surface.top())
        .unwrap();
    assert!(
        (title.rect.left() - g.surface.left()).abs() <= 1.0,
        "title paint must be left-aligned"
    );
    assert!(title.clip.contains_rect(title.rect));
    eprintln!("RECENT_GEOMETRY stage={stage:?} font={font} size={:?} surface={:?} rows={:?} row_height={} content={:?} footer={footer:?}", screen.size(), g.surface, g.rows, g.row_height, g.rows_content);
    footer
}

#[test]
fn compact_first_visible_and_settled_geometry_count_filter_and_footer_matrix() {
    for size in [
        egui::vec2(1178.0, 814.0),
        egui::vec2(780.0, 540.0),
        egui::vec2(700.0, 480.0),
    ] {
        for font in [13.0, 18.0] {
            let mut expected = None;
            for count in [0, 1, 3, 64] {
                let (mut app, commands) = visual_app(count, font);
                let first = visual_enter(&mut app, size);
                let footer = visual_layout(&app, &first, font, &format!("first {count}"));
                if let Some(expected) = expected {
                    assert_eq!(footer, expected, "history count must not move footer");
                } else {
                    expected = Some(footer);
                }
                let count_text = recent_count(count, count, true);
                assert_painted(&first, &count_text, geometry(&app).headline, font);
                assert_eq!(visual_flags(&app, "recent_open")[0], count > 0);
                assert_eq!(visual_flags(&app, "recent_clear")[0], count > 0);
                let settled = visual_frame(&mut app, size, 0.01, vec![]);
                assert_eq!(visual_layout(&app, &settled, font, "settled"), footer);
                if count == 64 {
                    let complete = app
                        .recent_files
                        .visible_paths()
                        .iter()
                        .filter(|path| {
                            let probe = format!("recent_row:{path}");
                            recorded_clip(&app, &probe).contains_rect(recorded_rect(&app, &probe))
                        })
                        .count();
                    if size.x >= 780.0 {
                        assert!(
                            complete >= if size.x > 780.0 { 12 } else { 8 },
                            "{size:?}/{font}: only {complete} full rows"
                        );
                    }
                    eprintln!("RECENT_CAPACITY size={size:?} font={font} fully_visible={complete} resilience={}", size.x < 780.0);
                    let output = visual_edit(&mut app, size, "file001");
                    assert_eq!(app.recent_files.visible_paths(), ["parent/file001.rs"]);
                    assert_eq!(visual_layout(&app, &output, font, "one filter"), footer);
                    assert_painted(
                        &output,
                        "1 of 64 recent files",
                        geometry(&app).headline,
                        font,
                    );
                    let output = visual_edit(&mut app, size, "no-such-filter");
                    assert!(app.recent_files.visible_paths().is_empty());
                    assert!(!visual_flags(&app, "recent_open")[0]);
                    assert!(visual_flags(&app, "recent_clear")[0]);
                    assert_eq!(visual_layout(&app, &output, font, "no match"), footer);
                    assert_painted(
                        &output,
                        "No recent files match this filter",
                        geometry(&app).headline,
                        font,
                    );
                    visual_edit(&mut app, size, "first.rs");
                    let output = visual_frame(&mut app, size, 0.01, vec![]);
                    assert_eq!(visual_layout(&app, &output, font, "active open"), footer);
                    assert!(visual_flags(&app, "recent_open")[0]);
                }
                assert!(
                    commands.try_recv().is_err(),
                    "all presentation/filter operations are local"
                );
            }
        }
    }
}

#[test]
fn compact_names_parents_badges_unicode_exact_hover_and_native_selection_events() {
    let size = egui::vec2(780.0, 540.0);
    for font in [13.0, 18.0] {
        let (mut app, commands) = visual_app(1, font);
        let unicode = format!(
            "文档/{}/same-e\u{301}-👩🏽\u{200d}💻.rs",
            "长路径/".repeat(20)
        );
        let ascii = format!(
            "parent/{}/long-file-name-{}.rs",
            "nested/".repeat(20),
            "x".repeat(120)
        );
        for path in ["src/main.rs", "tests/main.rs", &unicode, &ascii] {
            app.recent_files.touch(path);
        }
        app.documents.push(Document::new(
            2,
            "src/main.rs".into(),
            "source".into(),
            "r1".into(),
        ));
        let first = visual_enter(&mut app, size);
        visual_layout(&app, &first, font, "long names");
        let g = geometry(&app);
        let paints = painted(&first);
        for path in [
            "src/main.rs",
            "tests/main.rs",
            unicode.as_str(),
            ascii.as_str(),
        ] {
            let probe = format!("recent_row:{path}");
            let row = recorded_rect(&app, &probe);
            assert!(recorded_clip(&app, &probe).contains_rect(row));
            assert!((row.width() - g.rows.width()).abs() <= 18.0);
            let (parent, name) = path.rsplit_once('/').unwrap();
            let n = paints
                .iter()
                .find(|p| p.text == name && row.contains_rect(p.rect))
                .expect("painted full original basename layout string");
            let p = paints
                .iter()
                .find(|p| p.text == parent && row.contains_rect(p.rect))
                .expect("painted full original parent layout string");
            assert_eq!(n.rows, 1);
            assert_eq!(p.rows, 1);
            // egui's stock modal fade-in legitimately scales first-paint colors.
            // Check that actual painted colors preserve the intended tokens at
            // that opacity instead of waiting for the animation to disappear.
            for (actual, token) in [(n.color, chrome::TEXT), (p.color, chrome::MUTED)] {
                let expected = token.gamma_multiply(f32::from(actual.a()) / 255.0);
                assert!(actual.a() > 0);
                assert!(actual
                    .to_array()
                    .iter()
                    .zip(expected.to_array())
                    .all(|(a, b)| a.abs_diff(b) <= 1));
            }
            assert!(n.color.r() > p.color.r());
            assert!(n.clip.contains_rect(n.rect) && p.clip.contains_rect(p.rect));
            assert!((n.rect.left() - row.left() - 26.0).abs() <= 0.6);
            assert!(p.rect.left() > n.rect.right());
            let state = if path == "src/main.rs" {
                "Open"
            } else {
                "Closed"
            };
            assert_painted(&first, state, row, font);
        }
        let probe = format!("recent_row:{unicode}");
        let footer = visual_footer(&app);
        let output = visual_click(&mut app, size, &probe);
        assert_eq!(app.recent_files.selected_path(), Some(unicode.as_str()));
        assert_eq!(app.active_document, Some(1));
        assert!(app.recent_files.view.is_some());
        let labels = output
            .platform_output
            .events
            .iter()
            .filter_map(|event| match event {
                egui::output::OutputEvent::Clicked(info)
                    if info.typ == egui::WidgetType::SelectableLabel =>
                {
                    Some(info)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(labels.len(), 1, "one semantic click per real row response");
        assert_eq!(
            labels[0].label.as_deref(),
            Some(format!("{unicode} · Closed").as_str())
        );
        assert!(labels[0].enabled);
        let output = visual_frame(&mut app, size, 0.01, vec![]);
        assert_eq!(visual_footer(&app), footer);
        assert_painted(&output, &unicode, geometry(&app).path, font);
        assert_eq!(
            app.editor_ctx.memory(|memory| memory.focused()),
            Some(egui::Id::new(QUERY))
        );
        let pos = recorded_rect(&app, &probe).center();
        // Native hover ignores movement within 100 ms of a click. Observe
        // that quiet interval, then actually move before waiting for the delay.
        visual_frame(&mut app, size, 0.2, vec![]);
        for offset in [8.0, 4.0, 0.0] {
            visual_frame(
                &mut app,
                size,
                0.016,
                vec![egui::Event::PointerMoved(pos - egui::vec2(offset, 0.0))],
            );
        }
        let delay = f64::from(app.editor_ctx.style().interaction.tooltip_delay);
        visual_frame(&mut app, size, 1.0 + delay, vec![]);
        let hover = visual_frame(&mut app, size, 0.1, vec![]);
        assert!(
            painted(&hover)
                .iter()
                .any(|p| p.text.contains(&unicode)
                    && p.text.contains("No closed caret is remembered")),
            "real hover must expose exact path and closed semantics"
        );
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn compact_double_and_triple_click_only_select_then_explicit_open_reads_exact_path() {
    let size = egui::vec2(780.0, 540.0);
    let (mut app, commands) = visual_app(3, 13.0);
    visual_enter(&mut app, size);
    let path = "parent/file001.rs";
    let probe = format!("recent_row:{path}");
    for count in 1..=3 {
        visual_click(&mut app, size, &probe);
        assert_eq!(app.recent_files.selected_path(), Some(path));
        assert_eq!(app.active_document, Some(1));
        assert!(app.recent_files.view.is_some());
        assert!(commands.try_recv().is_err());
        let flags = visual_flags(&app, &probe);
        if count == 2 {
            assert!(flags[2]);
        }
        if count == 3 {
            assert!(flags[3]);
        }
    }
    visual_frame(&mut app, size, 0.01, vec![]);
    visual_click(&mut app, size, "recent_open");
    let read = commands.try_recv().unwrap();
    assert!(matches!(read.op, Operation::Read{path: ref p} if p == path));
    assert!(matches!(
        app.pending.get(&read.id),
        Some(crate::Job::Open { line: None, .. })
    ));
    assert!(commands.try_recv().is_err());
}

#[test]
fn compact_owned_end_reveals_once_keeps_query_focus_and_later_wheel_wins() {
    let size = egui::vec2(780.0, 540.0);
    let (mut app, commands) = visual_app(64, 18.0);
    visual_enter(&mut app, size);
    visual_frame(
        &mut app,
        size,
        0.01,
        key(egui::Key::End, egui::Modifiers::NONE),
    );
    // ScrollArea applies scroll_to_me after laying out content, so the repaint
    // is inspected as well as its immediate state. This does not skip entry.
    let g = geometry(&app);
    assert!(
        egui::scroll_area::State::load(&app.editor_ctx, g.rows_id)
            .unwrap()
            .offset
            .y
            > 0.0
    );
    visual_frame(&mut app, size, 0.01, vec![]);
    let row = recorded_rect(&app, "recent_row:first.rs");
    assert!(recorded_clip(&app, "recent_row:first.rs").contains_rect(row));
    assert_eq!(app.recent_files.selected_path(), Some("first.rs"));
    assert_eq!(
        app.editor_ctx.memory(|memory| memory.focused()),
        Some(egui::Id::new(QUERY))
    );
    let footer = visual_footer(&app);
    visual_frame(
        &mut app,
        size,
        0.01,
        vec![
            egui::Event::PointerMoved(g.rows.center()),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, 8000.0),
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    visual_frame(&mut app, size, 0.5, vec![]);
    let offset = egui::scroll_area::State::load(&app.editor_ctx, g.rows_id)
        .unwrap()
        .offset
        .y;
    assert_eq!(
        offset, 0.0,
        "a later wheel gesture must win over the old selection reveal"
    );
    visual_frame(&mut app, size, 0.1, vec![]);
    assert_eq!(
        egui::scroll_area::State::load(&app.editor_ctx, g.rows_id)
            .unwrap()
            .offset
            .y,
        offset
    );
    assert_eq!(visual_footer(&app), footer);
    assert_eq!(app.recent_files.selected_path(), Some("first.rs"));
    assert!(commands.try_recv().is_err());
}

#[test]
fn compact_long_status_scroll_preserves_headline_path_footer_and_full_message() {
    let size = egui::vec2(780.0, 540.0);
    let (mut app, commands) = visual_app(3, 18.0);
    visual_enter(&mut app, size);
    let footer = visual_footer(&app);
    const MESSAGE: &str = "This file changed since it was shown. Reopen Recent files to choose it again. The previous selection is unavailable; this explanation remains readable without moving the actions. End of message.";
    app.recent_files.view.as_mut().unwrap().message = Some(MESSAGE);
    let first = visual_frame(&mut app, size, 0.01, vec![]);
    assert_eq!(
        visual_layout(&app, &first, 18.0, "first long status"),
        footer
    );
    let g = geometry(&app);
    assert!(g.status_content.y > g.details.height());
    assert_painted(&first, "Selection is out of date", g.headline, 18.0);
    assert!(painted(&first).iter().any(|p| p.text == MESSAGE));
    visual_frame(
        &mut app,
        size,
        0.01,
        vec![
            egui::Event::PointerMoved(g.details.center()),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -8000.0),
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    let output = visual_frame(&mut app, size, 0.5, vec![]);
    let offset = egui::scroll_area::State::load(&app.editor_ctx, g.status_id)
        .unwrap()
        .offset
        .y;
    assert!(offset >= g.status_content.y - g.details.height() - 1.0 && offset > 0.0);
    assert_eq!(visual_layout(&app, &output, 18.0, "status tail"), footer);
    assert_painted(&output, "Selection is out of date", g.headline, 18.0);
    let tail = painted(&output)
        .into_iter()
        .find(|p| p.text == MESSAGE)
        .unwrap();
    assert!(tail.rect.bottom() <= tail.clip.bottom() + 0.6 && tail.rect.bottom() > tail.clip.top());
    assert_eq!(
        egui::scroll_area::State::load(&app.editor_ctx, g.rows_id)
            .unwrap()
            .offset
            .y,
        0.0
    );
    assert!(commands.try_recv().is_err());
}

#[test]
fn compact_font_install_first_visible_frame_and_reopen_forget_filter_undo() {
    let size = egui::vec2(780.0, 540.0);
    for font in [13.0, 18.0] {
        let (mut app, commands) = visual_app(3, font);
        let (fonts, deliver) = crate::system_fonts::SystemFonts::pending_success_for_test();
        app.system_fonts = fonts;
        let first = visual_enter(&mut app, size);
        let footer = visual_layout(&app, &first, font, "pre font install");
        deliver();
        for stage in [
            "receiver boundary",
            "first installed font frame",
            "settled installed font",
        ] {
            let output = visual_frame(&mut app, size, 0.01, vec![]);
            assert_eq!(visual_layout(&app, &output, font, stage), footer);
        }
        visual_edit(&mut app, size, "file001");
        visual_frame(
            &mut app,
            size,
            0.01,
            key(egui::Key::Escape, egui::Modifiers::NONE),
        );
        assert!(app.recent_files.view.is_none());
        visual_frame(&mut app, size, 0.01, vec![]);
        let output = visual_enter(&mut app, size);
        assert_eq!(visual_layout(&app, &output, font, "reopen"), footer);
        assert!(app.recent_files.view.as_ref().unwrap().query.is_empty());
        visual_frame(
            &mut app,
            size,
            0.01,
            key(egui::Key::Z, egui::Modifiers::COMMAND),
        );
        assert!(app.recent_files.view.as_ref().unwrap().query.is_empty());
        assert_eq!(app.recent_files.visible_paths().len(), 3);
        assert_eq!(app.recent_files.selected_path(), Some("parent/file002.rs"));
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn compact_paste_ime_parent_filter_and_unicode_limit_remain_local() {
    let size = egui::vec2(780.0, 540.0);
    let (mut app, commands) = visual_app(1, 13.0);
    let path = "模块/Ärea/same-e\u{301}-👩🏽\u{200d}💻.rs";
    app.recent_files.touch(path);
    visual_enter(&mut app, size);
    let footer = visual_footer(&app);
    visual_click(&mut app, size, "recent_query");
    visual_frame(
        &mut app,
        size,
        0.01,
        vec![egui::Event::Paste("äREA".into())],
    );
    assert_eq!(app.recent_files.visible_paths(), [path]);
    assert_eq!(app.recent_files.selected_path(), Some(path));
    assert!(commands.try_recv().is_err());
    visual_frame(
        &mut app,
        size,
        0.01,
        key(egui::Key::A, egui::Modifiers::COMMAND),
    );
    visual_frame(
        &mut app,
        size,
        0.01,
        vec![egui::Event::Ime(egui::ImeEvent::Enabled)],
    );
    visual_frame(
        &mut app,
        size,
        0.01,
        vec![egui::Event::Ime(egui::ImeEvent::Preedit("模".into()))],
    );
    visual_frame(
        &mut app,
        size,
        0.01,
        vec![egui::Event::Ime(egui::ImeEvent::Commit("模块".into()))],
    );
    visual_frame(
        &mut app,
        size,
        0.01,
        vec![egui::Event::Ime(egui::ImeEvent::Disabled)],
    );
    assert_eq!(app.recent_files.visible_paths(), [path]);
    assert_eq!(app.recent_files.view.as_ref().unwrap().query, "模块");
    assert_eq!(visual_footer(&app), footer);
    visual_frame(
        &mut app,
        size,
        0.01,
        key(egui::Key::A, egui::Modifiers::COMMAND),
    );
    visual_frame(
        &mut app,
        size,
        0.01,
        vec![egui::Event::Paste("界".repeat(QUERY_CHARS + 5))],
    );
    assert_eq!(
        app.recent_files
            .view
            .as_ref()
            .unwrap()
            .query
            .chars()
            .count(),
        QUERY_CHARS
    );
    assert!(!visual_flags(&app, "recent_open")[0]);
    assert!(visual_flags(&app, "recent_clear")[0]);
    assert_eq!(visual_footer(&app), footer);
    assert!(commands.try_recv().is_err());
}

#[test]
fn compact_clipped_row_hit_testing_uses_only_visible_intersection() {
    let size = egui::vec2(780.0, 540.0);
    let (mut app, commands) = visual_app(64, 18.0);
    visual_enter(&mut app, size);
    let g = geometry(&app);
    let path = app
        .recent_files
        .visible_paths()
        .into_iter()
        .find(|path| {
            let probe = format!("recent_row:{path}");
            let rect = recorded_rect(&app, &probe);
            rect.top() < g.rows.bottom() && rect.bottom() > g.rows.bottom()
        })
        .unwrap()
        .to_owned();
    let probe = format!("recent_row:{path}");
    let rect = recorded_rect(&app, &probe);
    let before = app.recent_files.selected_path().unwrap().to_owned();
    let pos = egui::pos2(rect.center().x, g.rows.bottom() + 1.0);
    assert!(rect.contains(pos) && !recorded_clip(&app, &probe).contains(pos));
    let pointer = |pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    visual_frame(&mut app, size, 0.01, vec![egui::Event::PointerMoved(pos)]);
    visual_frame(&mut app, size, 0.01, vec![pointer(true)]);
    visual_frame(&mut app, size, 0.01, vec![pointer(false)]);
    assert_eq!(app.recent_files.selected_path(), Some(before.as_str()));
    assert!(app.recent_files.view.is_some());
    visual_click(&mut app, size, &probe);
    assert_eq!(app.recent_files.selected_path(), Some(path.as_str()));
    assert_eq!(app.active_document, Some(1));
    assert!(commands.try_recv().is_err());
}

#[test]
fn compact_paint_only_hover_fits_edges_on_first_paint_and_cannot_take_native_press() {
    for size in [
        egui::vec2(1178.0, 814.0),
        egui::vec2(780.0, 540.0),
        egui::vec2(700.0, 480.0),
    ] {
        for font in [13.0, 18.0] {
            let (mut app, commands) = visual_app(0, font);
            let path = format!(
                "文档/{}/long-{}-e\u{301}-👩🏽\u{200d}💻.rs",
                "长路径/".repeat(20),
                "x".repeat(60)
            );
            app.recent_files.touch(&path);
            for n in 1..64 {
                app.recent_files.touch(&format!("parent/short{n}.rs"));
            }
            visual_enter(&mut app, size);
            let footer = visual_footer(&app);
            visual_frame(
                &mut app,
                size,
                0.01,
                key(egui::Key::End, egui::Modifiers::NONE),
            );
            visual_frame(&mut app, size, 0.01, vec![]);
            let probe = format!("recent_row:{path}");
            let row = recorded_rect(&app, &probe);
            assert!(recorded_clip(&app, &probe).contains_rect(row));
            let pos = row.center();
            let first_hover =
                visual_frame(&mut app, size, 0.01, vec![egui::Event::PointerMoved(pos)]);
            let exact = format!("{path} · Closed\nClosed; ordinary Open position is used. No closed caret is remembered.");
            for (stage, output) in [
                ("first hover paint", first_hover),
                (
                    "settled hover paint",
                    visual_frame(&mut app, size, 1.0, vec![]),
                ),
            ] {
                let draws = painted(&output);
                let draw = draws
                    .iter()
                    .find(|p| p.text == exact)
                    .expect("full hover text painted without an invisible Area sizing frame");
                assert!(
                    draw.clip.contains_rect(draw.rect),
                    "{stage} {size:?}/{font}: {:?} clipped by {:?}",
                    draw.rect,
                    draw.clip
                );
                assert!(app.editor_ctx.screen_rect().contains_rect(draw.rect));
                assert!(draw.font >= font && draw.rows > 1);
                assert!(
                    draw.rect.bottom() <= row.top() || draw.rect.top() >= row.bottom(),
                    "measured tooltip must avoid its source row when it fits"
                );
                let layer = egui::LayerId::new(
                    egui::Order::Tooltip,
                    recorded_response(&app, &probe).id.with("recent_tooltip"),
                );
                assert_eq!(
                    app.editor_ctx
                        .viewport(|v| v.prev_pass.widgets.get_layer(layer).count()),
                    0,
                    "paint-only hover creates no hit-test widgets"
                );
                assert_eq!(visual_footer(&app), footer);
                eprintln!("RECENT_HOVER_GEOMETRY stage={stage:?} size={size:?} font={font} row={row:?} hover={:?} clip={:?}", draw.rect, draw.clip);
            }
            let pointer = |pressed| egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            visual_frame(&mut app, size, 0.01, vec![pointer(true)]);
            assert!(visual_flags(&app, &probe)[1]);
            visual_frame(&mut app, size, 0.01, vec![pointer(false)]);
            assert_eq!(app.recent_files.selected_path(), Some(path.as_str()));
            assert!(app.recent_files.view.is_some());
            assert!(commands.try_recv().is_err());
        }
    }
}

#[test]
fn compact_reopen_after_scrolling_starts_with_default_other_visible() {
    let size = egui::vec2(780.0, 540.0);
    let (mut app, commands) = visual_app(64, 18.0);
    visual_enter(&mut app, size);
    let footer = visual_footer(&app);
    visual_frame(
        &mut app,
        size,
        0.01,
        key(egui::Key::End, egui::Modifiers::NONE),
    );
    visual_frame(&mut app, size, 0.01, vec![]);
    assert!(
        egui::scroll_area::State::load(&app.editor_ctx, geometry(&app).rows_id)
            .unwrap()
            .offset
            .y
            > 0.0
    );
    visual_frame(
        &mut app,
        size,
        0.01,
        key(egui::Key::Escape, egui::Modifiers::NONE),
    );
    visual_frame(&mut app, size, 0.01, vec![]);
    let first = visual_enter(&mut app, size);
    assert_eq!(
        visual_layout(&app, &first, 18.0, "reopen after scroll"),
        footer
    );
    assert_eq!(app.recent_files.selected_path(), Some("parent/file063.rs"));
    let probe = "recent_row:parent/file063.rs";
    assert!(
        recorded_clip(&app, probe).contains_rect(recorded_rect(&app, probe)),
        "fresh default selection must be visible immediately after reopen"
    );
    assert!(app.recent_files.view.as_ref().unwrap().query.is_empty());
    assert!(commands.try_recv().is_err());
}

#[test]
fn compact_stale_row_hover_is_explanatory_and_cannot_authorize_open() {
    let size = egui::vec2(780.0, 540.0);
    let (mut app, commands) = visual_app(1, 18.0);
    app.documents.push(Document::new(
        2,
        "src/main.rs".into(),
        "peer".into(),
        "r1".into(),
    ));
    app.recent_files.touch("src/main.rs");
    visual_enter(&mut app, size);
    // A same-path replacement has a new identity, while the origin is intact.
    app.documents[1].id = 3;
    visual_frame(&mut app, size, 0.01, vec![]);
    let probe = "recent_row:src/main.rs";
    assert!(!visual_flags(&app, probe)[0] && !visual_flags(&app, "recent_open")[0]);
    let id = recorded_response(&app, probe).id;
    assert!(app.recent_files.input.presented.iter().all(|p| p.id != id));
    let pos = recorded_rect(&app, probe).center();
    let hover = visual_frame(&mut app, size, 1.0, vec![egui::Event::PointerMoved(pos)]);
    assert!(painted(&hover).iter().any(|p| p.text == "src/main.rs · Open\nThis file changed since it was shown. Reopen Recent files to choose it again."));
    let pointer = |pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    visual_frame(&mut app, size, 0.01, vec![pointer(true)]);
    let output = visual_frame(&mut app, size, 0.01, vec![pointer(false)]);
    assert!(output.platform_output.events.iter().all(|e| !matches!(e, egui::output::OutputEvent::Clicked(info) if info.typ == egui::WidgetType::SelectableLabel)));
    assert_eq!(app.active_document, Some(1));
    assert_eq!(app.documents.len(), 2);
    assert!(app.recent_files.view.is_some());
    assert!(commands.try_recv().is_err());
}

#[test]
fn compact_owned_end_survives_requested_discard_into_first_visible_pass() {
    let size = egui::vec2(780.0, 540.0);
    let (mut app, commands) = visual_app(64, 18.0);
    visual_enter(&mut app, size);
    let footer = visual_footer(&app);
    let output = visual_frame_with_discard(
        &mut app,
        size,
        0.01,
        key(egui::Key::End, egui::Modifiers::NONE),
        true,
    );
    assert_eq!(output.platform_output.num_completed_passes, 2);
    assert_eq!(app.recent_files.selected_path(), Some("first.rs"));
    // ScrollArea commits its reveal offset at the end of layout. Check that
    // discarded-pass ownership was retained before inspecting the repaint.
    let g = geometry(&app);
    let offset = egui::scroll_area::State::load(&app.editor_ctx, g.rows_id)
        .unwrap()
        .offset
        .y;
    eprintln!(
        "RECENT_DISCARD_REVEAL passes={} selected={:?} offset={offset}",
        output.platform_output.num_completed_passes,
        app.recent_files.selected_path()
    );
    assert!(
        offset > 0.0,
        "a discarded pass must not consume the owned End reveal"
    );
    let output = visual_frame(&mut app, size, 0.01, vec![]);
    assert!(recorded_clip(&app, "recent_row:first.rs")
        .contains_rect(recorded_rect(&app, "recent_row:first.rs")));
    assert_eq!(
        visual_layout(&app, &output, 18.0, "discarded End reveal"),
        footer
    );
    assert_eq!(
        app.editor_ctx.memory(|m| m.focused()),
        Some(egui::Id::new(QUERY))
    );
    assert!(commands.try_recv().is_err());
}

#[test]
fn compact_hover_stays_suppressed_after_click_until_real_motion_and_while_scrolling() {
    let size = egui::vec2(780.0, 540.0);
    let (mut app, commands) = visual_app(64, 18.0);
    visual_enter(&mut app, size);
    let probe = "recent_row:parent/file062.rs";
    visual_click(&mut app, size, probe);
    let output = visual_frame(&mut app, size, 3.0, vec![]);
    let is_tooltip = |p: &Paint| p.text.contains("Closed; ordinary Open position is used");
    assert!(
        !painted(&output).iter().any(is_tooltip),
        "a click followed by rest must not reopen its tooltip"
    );
    let pos = recorded_rect(&app, probe).center();
    for offset in [8.0, 4.0, 0.0] {
        visual_frame(
            &mut app,
            size,
            0.016,
            vec![egui::Event::PointerMoved(pos - egui::vec2(offset, 0.0))],
        );
    }
    let output = visual_frame(&mut app, size, 1.0, vec![]);
    assert!(painted(&output).iter().any(is_tooltip));
    // Isolate smooth-wheel suppression from the elapsed-delay gate. This
    // single boundary case uses zero delay; the rest uses production timing.
    app.editor_ctx
        .style_mut(|style| style.interaction.tooltip_delay = 0.0);
    let output = visual_frame(
        &mut app,
        size,
        0.016,
        vec![egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -100.0),
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert_ne!(app.editor_ctx.data(|data| data.get_temp::<egui::Vec2>(egui::Id::new("recent_smooth_input_before_ui"))).unwrap(), egui::Vec2::ZERO,
        "observe native smooth delta before ScrollArea legitimately consumes it");
    assert!(
        !painted(&output).iter().any(is_tooltip),
        "smooth wheel input must suppress the tooltip"
    );
    assert!(commands.try_recv().is_err());
}

#[test]
fn compact_repeated_reopen_uses_bounded_scroll_and_presentation_storage() {
    let size = egui::vec2(780.0, 540.0);
    let (mut app, commands) = visual_app(64, 18.0);
    visual_enter(&mut app, size);
    let id = geometry(&app).rows_id;
    let states = app
        .editor_ctx
        .data(|data| data.count::<egui::scroll_area::State>());
    for _ in 0..32 {
        visual_frame(
            &mut app,
            size,
            0.01,
            key(egui::Key::End, egui::Modifiers::NONE),
        );
        visual_frame(
            &mut app,
            size,
            0.01,
            key(egui::Key::Escape, egui::Modifiers::NONE),
        );
        assert_eq!(
            app.editor_ctx.data(|data| data.count::<Presentation>()),
            0,
            "dismissal clears the bounded presentation record"
        );
        visual_frame(&mut app, size, 0.01, vec![]);
        visual_enter(&mut app, size);
        assert_eq!(geometry(&app).rows_id, id);
        assert_eq!(
            app.editor_ctx
                .data(|data| data.count::<egui::scroll_area::State>()),
            states
        );
        assert_eq!(app.editor_ctx.data(|data| data.count::<Presentation>()), 1);
        assert_eq!(
            egui::scroll_area::State::load(&app.editor_ctx, id)
                .unwrap()
                .offset
                .y,
            0.0
        );
    }
    assert!(commands.try_recv().is_err());
}

#[test]
fn compact_discarded_pending_reveal_yields_to_later_wheel_and_cancel() {
    let size = egui::vec2(780.0, 540.0);
    let (mut app, commands) = visual_app(64, 18.0);
    visual_enter(&mut app, size);
    app.editor_ctx
        .options_mut(|options| options.max_passes = std::num::NonZeroUsize::new(1).unwrap());
    let output = visual_frame_with_discard(
        &mut app,
        size,
        0.01,
        key(egui::Key::End, egui::Modifiers::NONE),
        true,
    );
    assert_eq!(output.platform_output.num_completed_passes, 1);
    assert!(app.editor_ctx.data(|data| data
        .get_temp::<Presentation>(egui::Id::new(PRESENTATION))
        .unwrap()
        .reveal
        .is_some()));
    let g = geometry(&app);
    app.editor_ctx
        .options_mut(|options| options.max_passes = std::num::NonZeroUsize::new(2).unwrap());
    visual_frame(
        &mut app,
        size,
        0.01,
        vec![
            egui::Event::PointerMoved(g.rows.center()),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, 80.0),
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    assert!(app.editor_ctx.data(|data| data
        .get_temp::<Presentation>(egui::Id::new(PRESENTATION))
        .unwrap()
        .reveal
        .is_none()));
    assert_eq!(
        egui::scroll_area::State::load(&app.editor_ctx, g.rows_id)
            .unwrap()
            .offset
            .y,
        0.0
    );
    visual_frame(
        &mut app,
        size,
        0.01,
        key(egui::Key::Escape, egui::Modifiers::NONE),
    );
    assert_eq!(app.editor_ctx.data(|data| data.count::<Presentation>()), 0);
    assert!(commands.try_recv().is_err());
}

#[test]
fn compact_maximum_legal_path_keeps_exact_text_with_explicit_tooltip_height_boundary() {
    let size = egui::vec2(780.0, 540.0);
    let (mut app, commands) = visual_app(0, 18.0);
    let path = format!("parent/{}.rs", "x".repeat(4086));
    assert_eq!(path.len(), 4096);
    app.recent_files.touch(&path);
    visual_enter(&mut app, size);
    let probe = format!("recent_row:{path}");
    let pos = recorded_rect(&app, &probe).center();
    let output = visual_frame(&mut app, size, 1.0, vec![egui::Event::PointerMoved(pos)]);
    let paints = painted(&output);
    let tooltip = paints
        .iter()
        .find(|p| p.text.starts_with(&path) && p.text.contains("No closed caret is remembered"))
        .unwrap();
    assert!(
        tooltip.rect.height() > tooltip.clip.height(),
        "fixture must demonstrate the real noninteractive screen-height boundary"
    );
    assert_eq!(app.recent_files.selected_path(), Some(path.as_str()));
    assert_eq!(
        app.recent_files.view.as_ref().unwrap().rows[0]
            .path
            .as_bytes(),
        path.as_bytes()
    );
    eprintln!("RECENT_MAX_PATH_BOUNDARY exact_bytes={} tooltip_height={} clip_height={} full_layout_string_retained=true all_characters_visible=false", path.len(), tooltip.rect.height(), tooltip.clip.height());
    assert!(commands.try_recv().is_err());
}

#[test]
fn compact_reopen_resets_native_drag_momentum_before_first_visible_frame() {
    let size = egui::vec2(780.0, 540.0);
    let (mut app, commands) = visual_app(64, 18.0);
    visual_enter(&mut app, size);
    visual_frame(&mut app, size, 0.01, vec![]);
    let rows = geometry(&app).rows;
    let start = egui::pos2(rows.center().x, rows.bottom() - 20.0);
    let pointer = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    visual_frame(
        &mut app,
        size,
        0.016,
        vec![egui::Event::PointerMoved(start)],
    );
    visual_frame(&mut app, size, 0.016, vec![pointer(start, true)]);
    let mut pos = start;
    for _ in 0..6 {
        pos.y -= 20.0;
        visual_frame(&mut app, size, 0.016, vec![egui::Event::PointerMoved(pos)]);
    }
    visual_frame(&mut app, size, 0.016, vec![pointer(pos, false)]);
    let state = egui::scroll_area::State::load(&app.editor_ctx, geometry(&app).rows_id).unwrap();
    eprintln!(
        "real native drag before dismissal: offset={:?}, velocity={:?}",
        state.offset,
        state.velocity()
    );
    assert!(state.offset.y > 0.0);
    assert!(
        state.velocity().y.abs() > 20.0,
        "fixture must produce real native drag inertia"
    );
    visual_frame(
        &mut app,
        size,
        0.016,
        key(egui::Key::Escape, egui::Modifiers::NONE),
    );
    let first = visual_enter(&mut app, size);
    visual_layout(&app, &first, 18.0, "reopen during native inertia");
    let state = egui::scroll_area::State::load(&app.editor_ctx, geometry(&app).rows_id).unwrap();
    eprintln!(
        "first reopened frame: offset={:?}, velocity={:?}",
        state.offset,
        state.velocity()
    );
    assert_eq!(state.velocity(), egui::Vec2::ZERO);
    assert_eq!(state.offset, egui::Vec2::ZERO);
    let probe = "recent_row:parent/file063.rs";
    assert_eq!(app.recent_files.selected_path(), Some("parent/file063.rs"));
    assert!(recorded_clip(&app, probe).contains_rect(recorded_rect(&app, probe)));
    assert!(commands.try_recv().is_err());
}
