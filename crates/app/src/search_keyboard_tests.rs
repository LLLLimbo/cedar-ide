//! Bounded snapshot admission and complete production raw-hook/update frames.
//! Rust execution is reserved for CI; these fixtures do not launch a backend.
use super::*;
use crate::search_keyboard::{clean_key, destination, row_id, validate, MAX_ROWS, MAX_TEXT_BYTES};
use crate::workspace_access_tests::{recorded_clip, recorded_rect, recorded_response};

const SIZE: egui::Vec2 = egui::vec2(1200.0, 900.0);
const SCROLL: &str = "search_keyboard_scroll_probe";

pub(super) fn record_scroll(ctx: &egui::Context, id: egui::Id) {
    ctx.data_mut(|data| data.insert_temp(egui::Id::new(SCROLL), id));
}

pub(super) fn recorded_scroll_id(app: &CedarApp) -> egui::Id {
    app.editor_ctx.data(|data| {
        data.get_temp(egui::Id::new(SCROLL))
            .expect("Search scroll was not rendered")
    })
}

fn rows(count: usize) -> Vec<SearchMatch> {
    (0..count)
        .map(|index| SearchMatch {
            path: if index < 2 {
                "draft.txt".into()
            } else {
                format!("file-{index}.txt")
            },
            line: 2,
            text: "second Ω😀".into(),
        })
        .collect()
}

fn app(count: usize) -> (CedarApp, Receiver<Command>) {
    let mut app = CedarApp::empty();
    let form = ConnectForm {
        local_root: "/workspace".into(),
        allow_run: false,
        ..Default::default()
    };
    app.workspace_key = Some(form.key());
    app.active_form = Some(form);
    app.root = "/workspace".into();
    app.agent_info = Some(agent_support::full_test_agent());
    app.state = ConnectionState::Ready;
    app.open_form = false;
    app.tool = Tool::Search;
    app.tools_open = true;
    app.tools_height = Some(480.0);
    app.documents.push(Document::new(
        1,
        "draft.txt".into(),
        "saved Ω😀\r\nsecond\n".into(),
        "a".repeat(64),
    ));
    app.active_document = Some(1);
    app.next_document = 2;
    let (worker, commands) = Worker::recording();
    app.worker = Some(worker);
    app.search_query = "second".into();
    app.search();
    let search = commands.try_recv().unwrap();
    reply(
        &mut app,
        search.id,
        Ok(Payload::Matches {
            matches: rows(count),
            truncated: false,
        }),
        true,
    );
    settle(&mut app);
    assert!(
        app.system_fonts.needs_probe(),
        "fixture must not start optional font I/O"
    );
    (app, commands)
}

fn reply(app: &mut CedarApp, id: u64, result: Result<Payload, String>, connected: bool) {
    app.apply_event(Event {
        generation: app.generation,
        id,
        result,
        connected,
    });
}

fn raw(app: &CedarApp, events: Vec<egui::Event>) -> egui::RawInput {
    egui::RawInput {
        time: Some(app.editor_ctx.input(|input| input.time) + 0.1),
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SIZE)),
        events,
        ..Default::default()
    }
}

fn render(app: &mut CedarApp, input: egui::RawInput) -> egui::FullOutput {
    let ctx = app.editor_ctx.clone();
    let mut native = eframe::Frame::_new_kittest();
    ctx.run(input, |ctx| eframe::App::update(app, ctx, &mut native))
}

fn frame(app: &mut CedarApp, events: Vec<egui::Event>) -> egui::FullOutput {
    let ctx = app.editor_ctx.clone();
    let mut input = raw(app, events);
    eframe::App::raw_input_hook(app, &ctx, &mut input);
    render(app, input)
}

fn settle(app: &mut CedarApp) {
    for _ in 0..3 {
        frame(app, vec![]);
    }
}

fn edge(key: egui::Key, pressed: bool, repeat: bool) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: Some(key),
        pressed,
        repeat,
        modifiers: egui::Modifiers::NONE,
    }
}

fn key(key: egui::Key) -> Vec<egui::Event> {
    vec![edge(key, true, false), edge(key, false, false)]
}

fn modified_key(key: egui::Key, modifiers: egui::Modifiers) -> Vec<egui::Event> {
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

fn focus(app: &mut CedarApp, probe: &str) {
    recorded_response(app, probe).request_focus();
    frame(app, vec![]);
    assert_eq!(focused(app), Some(recorded_response(app, probe).id));
}

fn focused(app: &CedarApp) -> Option<egui::Id> {
    app.editor_ctx.memory(|memory| memory.focused())
}

fn id(app: &CedarApp, index: usize) -> egui::Id {
    row_id(app.search_state.accepted.as_ref().unwrap(), index)
}

fn has_text(output: &egui::FullOutput, expected: &str) -> bool {
    fn contains(shape: &egui::epaint::Shape, expected: &str) -> bool {
        match shape {
            egui::epaint::Shape::Text(text) => text.galley.job.text.contains(expected),
            egui::epaint::Shape::Vec(shapes) => {
                shapes.iter().any(|shape| contains(shape, expected))
            }
            _ => false,
        }
    }
    output
        .shapes
        .iter()
        .any(|shape| contains(&shape.shape, expected))
}

fn assert_visible(app: &CedarApp, index: usize) {
    let probe = format!("search_row:{index}");
    let rect = recorded_rect(app, &probe);
    let clip = recorded_clip(app, &probe);
    assert!(
        clip.height() > rect.height(),
        "positive viewport required: {clip:?}"
    );
    assert!(
        rect.top() >= clip.top() - 1.0 && rect.bottom() <= clip.bottom() + 1.0,
        "row {index}: {rect:?} outside {clip:?}"
    );
}

#[test]
fn search_keyboard_bounds_count_exact_utf8_bytes_and_distinct_identities() {
    assert!(validate(&rows(MAX_ROWS)).is_ok());
    assert!(validate(&rows(MAX_ROWS + 1)).is_err());
    let mut row = SearchMatch {
        path: "Ω".into(),
        line: usize::MAX,
        text: "x".repeat(MAX_TEXT_BYTES - 2),
    };
    assert!(validate(&[row.clone()]).is_ok());
    row.text.push('x');
    assert!(validate(&[row]).is_err());
    assert!(validate(&[]).is_ok());
    let mut aggregate = vec![
        SearchMatch {
            path: "x".into(),
            line: 0,
            text: "x".repeat(MAX_TEXT_BYTES / 2 - 1)
        };
        2
    ];
    assert!(validate(&aggregate).is_ok());
    aggregate[1].text.push('x');
    assert!(validate(&aggregate).is_err());
    let source = search_keyboard::Source {
        workspace: WorkspaceKey::Local {
            root: "/workspace".into(),
        },
        root: "/workspace".into(),
        generation: 3,
        request: 17,
        query: "second".into(),
    };
    assert_ne!(row_id(&source, 0), row_id(&source, 1));
    for changed in [
        search_keyboard::Source {
            request: source.request + 1,
            ..source.clone()
        },
        search_keyboard::Source {
            generation: source.generation + 1,
            ..source.clone()
        },
        search_keyboard::Source {
            root: "/other".into(),
            ..source.clone()
        },
        search_keyboard::Source {
            workspace: WorkspaceKey::Local {
                root: "/other".into(),
            },
            ..source.clone()
        },
    ] {
        assert_ne!(row_id(&source, 0), row_id(&changed, 0));
    }
}

#[test]
fn search_keyboard_destination_clamps_empty_single_and_many_rows() {
    assert_eq!(destination(0, 0, egui::Key::End), None);
    for key in [
        egui::Key::ArrowUp,
        egui::Key::ArrowDown,
        egui::Key::Home,
        egui::Key::End,
    ] {
        assert_eq!(destination(1, 0, key), Some(0));
    }
    assert_eq!(destination(500, 0, egui::Key::ArrowUp), Some(0));
    assert_eq!(destination(500, 499, egui::Key::ArrowDown), Some(499));
    assert_eq!(destination(500, 250, egui::Key::Home), Some(0));
    assert_eq!(destination(500, 250, egui::Key::End), Some(499));
    assert_eq!(destination(500, 500, egui::Key::Home), None);
    assert_eq!(destination(500, 1, egui::Key::Enter), None);
}

#[test]
fn search_keyboard_exact_space_companion_and_mixed_batch_veto() {
    let mut space = key(egui::Key::Space);
    space.insert(1, egui::Event::Text(" ".into()));
    assert_eq!(clean_key(&space), Some(egui::Key::Space));
    for extra in [
        egui::Event::Text(" ".into()),
        egui::Event::Text("x".into()),
        egui::Event::Paste(" ".into()),
        egui::Event::Ime(egui::ImeEvent::Commit(" ".into())),
        edge(egui::Key::Enter, true, false),
        egui::Event::PointerMoved(egui::Pos2::ZERO),
    ] {
        let mut mixed = space.clone();
        mixed.push(extra);
        assert_eq!(clean_key(&mixed), None);
    }
    assert_eq!(
        clean_key(&modified_key(egui::Key::Enter, egui::Modifiers::SHIFT)),
        None
    );
    assert_eq!(
        clean_key(&[
            edge(egui::Key::Space, false, false),
            edge(egui::Key::Space, true, false),
            egui::Event::Text(" ".into())
        ]),
        None
    );
}

#[test]
fn search_keyboard_native_tab_and_explicit_query_enter_remain_available() {
    let (mut app, commands) = app(8);
    focus(&mut app, "search_submit");
    frame(&mut app, key(egui::Key::Tab));
    assert_eq!(focused(&app), Some(id(&app, 0)));
    frame(&mut app, key(egui::Key::Tab));
    assert_eq!(focused(&app), Some(id(&app, 1)));
    frame(
        &mut app,
        modified_key(egui::Key::Tab, egui::Modifiers::SHIFT),
    );
    // Pinned egui applies Previous on its next pass so gained_focus is visible.
    frame(&mut app, vec![]);
    assert_eq!(focused(&app), Some(id(&app, 0)));
    assert!(commands.try_recv().is_err());
    focus(&mut app, "search_query");
    frame(&mut app, key(egui::Key::Enter));
    assert!(
        matches!(commands.try_recv().unwrap().op, Operation::Search { query, limit: 500 } if query == "second")
    );
    assert!(commands.try_recv().is_err());
    assert!(!app.search_rows_active());
}

#[test]
fn search_keyboard_native_tab_overflow_visits_actual_scroll_area_then_rows() {
    let (mut app, commands) = app(160);
    focus(&mut app, "search_submit");
    let area_id = recorded_scroll_id(&app).with("area");
    let area = app.editor_ctx.read_response(area_id).unwrap();
    assert!(area.sense.is_focusable() && area.sense.senses_drag());
    assert!(area.rect.is_finite() && area.rect.height() > 0.0);
    frame(&mut app, key(egui::Key::Tab));
    assert_eq!(focused(&app), Some(area_id));
    frame(&mut app, key(egui::Key::Tab));
    assert_eq!(focused(&app), Some(id(&app, 0)));
    frame(&mut app, key(egui::Key::Tab));
    assert_eq!(focused(&app), Some(id(&app, 1)));
    frame(
        &mut app,
        modified_key(egui::Key::Tab, egui::Modifiers::SHIFT),
    );
    frame(&mut app, vec![]);
    assert_eq!(focused(&app), Some(id(&app, 0)));
    assert!(commands.try_recv().is_err());
}

#[test]
fn search_keyboard_arrows_home_end_reveal_real_scroll_viewport_and_duplicates() {
    let (mut app, commands) = app(160);
    let scroll = recorded_scroll_id(&app);
    assert!(egui::scroll_area::State::load(&app.editor_ctx, scroll).is_some());
    focus(&mut app, "search_row:0");
    frame(&mut app, key(egui::Key::ArrowUp));
    assert_eq!(focused(&app), Some(id(&app, 0)));
    frame(&mut app, key(egui::Key::ArrowDown));
    assert_eq!(focused(&app), Some(id(&app, 1)));
    frame(&mut app, key(egui::Key::End));
    settle(&mut app);
    assert_eq!(focused(&app), Some(id(&app, 159)));
    assert_visible(&app, 159);
    frame(&mut app, key(egui::Key::ArrowDown));
    assert_eq!(focused(&app), Some(id(&app, 159)));
    frame(&mut app, key(egui::Key::Home));
    settle(&mut app);
    assert_eq!(focused(&app), Some(id(&app, 0)));
    assert_visible(&app, 0);
    assert!(commands.try_recv().is_err());
}

#[test]
fn search_keyboard_enter_space_and_pointer_open_dirty_buffer_preserving_undo() {
    for activation in 0..3 {
        let (mut app, commands) = app(8);
        let original = app.documents[0].text.clone();
        editor_state::commit(
            &app.editor_ctx,
            &mut app.documents[0],
            "dirty Ω😀\r\nsecond retained\n".into(),
            0,
        );
        settle(&mut app);
        focus(&mut app, "search_row:0");
        let departure = app.navigation_epoch;
        match activation {
            0 => {
                frame(&mut app, key(egui::Key::Enter));
            }
            1 => {
                let mut events = key(egui::Key::Space);
                events.insert(1, egui::Event::Text(" ".into()));
                frame(&mut app, events);
            }
            _ => {
                assert_visible(&app, 0);
                let pos = recorded_rect(&app, "search_row:0").center();
                frame(&mut app, pointer(pos, true));
                assert!(recorded_response(&app, "search_row:0").is_pointer_button_down_on());
                frame(&mut app, pointer(pos, false));
                assert!(recorded_response(&app, "search_row:0")
                    .clicked_by(egui::PointerButton::Primary));
            }
        }
        settle(&mut app);
        assert_eq!(app.navigation_epoch, departure + 1);
        assert_eq!(app.active_document, Some(1));
        assert_eq!(app.documents[0].text, "dirty Ω😀\r\nsecond retained\n");
        assert_eq!(app.documents[0].saved_text, original);
        assert!(app.documents[0].dirty());
        assert!(
            commands.try_recv().is_err(),
            "existing dirty Open must not issue Read or Write"
        );
        assert_eq!(app.documents[0].cursor, (2, 1));
        assert_eq!(app.location_history.back.len(), 1);
        frame(
            &mut app,
            modified_key(egui::Key::Z, egui::Modifiers::COMMAND),
        );
        assert_eq!(app.documents[0].text, original);
    }
}

#[test]
fn search_keyboard_unopened_activation_dispatches_one_ordinary_read_and_no_repeat() {
    let (mut app, commands) = app(8);
    focus(&mut app, "search_row:2");
    frame(&mut app, vec![edge(egui::Key::Enter, true, false)]);
    let open = commands.try_recv().unwrap();
    assert!(matches!(&open.op, Operation::Read { path } if path == "file-2.txt"));
    for repeat in [false, true] {
        frame(&mut app, vec![edge(egui::Key::Enter, true, repeat)]);
        assert!(commands.try_recv().is_err());
    }
    frame(&mut app, vec![edge(egui::Key::Enter, false, false)]);
    reply(
        &mut app,
        open.id,
        Ok(Payload::File {
            path: "file-2.txt".into(),
            text: "first\ncurrent second\n".into(),
            revision: "b".repeat(64),
        }),
        true,
    );
    settle(&mut app);
    assert_eq!(app.active().unwrap().path, "file-2.txt");
    assert_eq!(app.active().unwrap().cursor, (2, 1));
    assert_eq!(app.location_history.back.len(), 1);
    assert!(commands.try_recv().is_err());
}

#[test]
fn search_keyboard_history_uses_submitted_query_and_old_rows_stay_inactive() {
    let (mut app, commands) = app(8);
    let old = id(&app, 0);
    app.search_query = "replacement".into();
    app.search();
    let search = commands.try_recv().unwrap();
    settle(&mut app);
    assert!(!app.search_rows_active());
    assert_eq!(app.search_results.len(), 8);
    assert_eq!(app.search_state.accepted.as_ref().unwrap().query, "second");
    assert!(!recorded_response(&app, "search_row:0").enabled());
    let _ = recorded_rect(&app, "search_inactive");
    app.search_query = "edited while pending".into();
    reply(
        &mut app,
        search.id,
        Ok(Payload::Matches {
            matches: rows(3),
            truncated: true,
        }),
        true,
    );
    settle(&mut app);
    assert_eq!(
        app.search_state.accepted.as_ref().unwrap().query,
        "replacement"
    );
    assert_eq!(app.search_results.len(), 3);
    assert_ne!(id(&app, 0), old);
    assert!(!app.search_rows_active());
    let _ = recorded_rect(&app, "search_history");
    let _ = recorded_rect(&app, "search_truncated");
    let output = frame(&mut app, vec![]);
    assert!(has_text(&output, "Historical results for \"replacement\""));
    assert!(has_text(
        &output,
        "Search stopped at an engine limit; more matches may exist. Narrow your query."
    ));
    assert!(!has_text(&output, "Results capped at 500"));
}

#[test]
fn search_keyboard_rejected_whole_batch_and_current_error_keep_old_rows_inactive() {
    for result in [
        Ok(Payload::Matches {
            matches: rows(501),
            truncated: false,
        }),
        Ok(Payload::Matches {
            matches: vec![SearchMatch {
                path: "file".into(),
                line: 1,
                text: "x".repeat(MAX_TEXT_BYTES),
            }],
            truncated: false,
        }),
        Ok(Payload::GitStatus {
            text: "wrong payload".into(),
        }),
        Err("current search failure".into()),
    ] {
        let (mut app, commands) = app(8);
        let source = app.search_state.accepted.clone();
        app.search();
        let search = commands.try_recv().unwrap();
        reply(&mut app, search.id, result, true);
        assert_eq!(app.search_results.len(), 8);
        assert_eq!(app.search_state.accepted, source);
        assert!(app.search_state.pending.is_none());
        assert!(app.error.is_some());
        assert!(!app.search_rows_active());
    }
}

#[test]
fn search_keyboard_retired_success_error_and_payload_are_silent_but_transport_loss_wins() {
    for result in [
        Ok(Payload::Matches {
            matches: rows(99),
            truncated: true,
        }),
        Ok(Payload::GitStatus {
            text: "retired unexpected payload".into(),
        }),
        Err("retired failure".into()),
    ] {
        let (mut app, commands) = app(8);
        app.search();
        let older = commands.try_recv().unwrap();
        app.search();
        let newer = commands.try_recv().unwrap();
        reply(
            &mut app,
            newer.id,
            Ok(Payload::Matches {
                matches: rows(2),
                truncated: false,
            }),
            true,
        );
        app.error = Some("newer error must survive".into());
        let notice = app.notice.clone();
        reply(&mut app, older.id, result, true);
        assert_eq!(app.search_results.len(), 2);
        assert_eq!(
            app.search_state.accepted.as_ref().unwrap().request,
            newer.id
        );
        assert_eq!(app.error.as_deref(), Some("newer error must survive"));
        assert_eq!(app.notice, notice);
        assert!(app.search_rows_active());
    }
    let (mut app, commands) = app(8);
    app.search();
    let older = commands.try_recv().unwrap();
    app.search();
    let _newer = commands.try_recv().unwrap();
    reply(&mut app, older.id, Err("transport lost".into()), false);
    assert!(app.state == ConnectionState::Disconnected);
    assert!(!app.search_rows_active());
    assert_eq!(app.search_results.len(), 8);
}

#[test]
fn search_keyboard_capture_is_cancelled_by_query_request_session_workspace_focus_and_modal_change()
{
    for change in 0..8 {
        let (mut app, commands) = app(8);
        focus(&mut app, "search_row:2");
        let ctx = app.editor_ctx.clone();
        let mut input = raw(&app, key(egui::Key::Enter));
        eframe::App::raw_input_hook(&mut app, &ctx, &mut input);
        match change {
            0 => app.search_query.push('x'),
            1 => app.search_request += 1,
            2 => app.generation += 1,
            3 => app.root = "/other".into(),
            4 => {
                app.workspace_key = Some(WorkspaceKey::Local {
                    root: "/other".into(),
                })
            }
            5 => ctx.memory_mut(|memory| memory.request_focus(egui::Id::new(("editor", 1_u64)))),
            6 => app.new_file = true,
            _ => app.tools_open = false,
        }
        render(&mut app, input);
        assert!(
            commands.try_recv().is_err(),
            "captured input survived change {change}"
        );
        assert_eq!(app.active_document, Some(1));
    }
}

#[test]
fn search_keyboard_missing_raw_hook_and_window_focus_loss_cannot_activate() {
    let (mut app, commands) = app(8);
    focus(&mut app, "search_row:2");
    let input = raw(&app, key(egui::Key::Enter));
    render(&mut app, input);
    assert!(commands.try_recv().is_err());
    focus(&mut app, "search_row:2");
    let ctx = app.editor_ctx.clone();
    let mut input = raw(&app, key(egui::Key::Enter));
    input.focused = false;
    eframe::App::raw_input_hook(&mut app, &ctx, &mut input);
    render(&mut app, input);
    assert!(commands.try_recv().is_err());
}

#[test]
fn search_keyboard_mixed_modified_and_ime_frames_never_open_or_move_rows() {
    for events in [
        [key(egui::Key::Enter), key(egui::Key::ArrowDown)].concat(),
        [key(egui::Key::Space), vec![egui::Event::Paste(" ".into())]].concat(),
        [key(egui::Key::Enter), vec![egui::Event::Text("x".into())]].concat(),
        modified_key(egui::Key::End, egui::Modifiers::SHIFT),
        [
            key(egui::Key::Enter),
            vec![egui::Event::Ime(egui::ImeEvent::Commit("x".into()))],
        ]
        .concat(),
        [
            key(egui::Key::Enter),
            vec![egui::Event::PointerMoved(egui::pos2(4.0, 4.0))],
        ]
        .concat(),
    ] {
        let (mut app, commands) = app(8);
        focus(&mut app, "search_row:2");
        frame(&mut app, events);
        assert_eq!(focused(&app), Some(id(&app, 2)));
        assert!(commands.try_recv().is_err());
    }
    let (mut app, commands) = app(8);
    focus(&mut app, "search_row:2");
    frame(
        &mut app,
        vec![egui::Event::Ime(egui::ImeEvent::Preedit("compose".into()))],
    );
    frame(&mut app, key(egui::Key::Enter));
    assert!(commands.try_recv().is_err());
    frame(&mut app, vec![egui::Event::Ime(egui::ImeEvent::Disabled)]);
    frame(&mut app, key(egui::Key::Enter));
    assert!(matches!(
        commands.try_recv().unwrap().op,
        Operation::Read { .. }
    ));
}

#[test]
fn search_keyboard_pointer_release_cannot_retarget_new_batch_or_changed_query() {
    for replacement in [false, true] {
        let (mut app, commands) = app(8);
        let pos = recorded_rect(&app, "search_row:2").center();
        frame(&mut app, pointer(pos, true));
        if replacement {
            app.search();
            let search = commands.try_recv().unwrap();
            reply(
                &mut app,
                search.id,
                Ok(Payload::Matches {
                    matches: rows(8),
                    truncated: false,
                }),
                true,
            );
        } else {
            app.search_query.push('x');
        }
        frame(&mut app, pointer(pos, false));
        assert!(commands.try_recv().is_err());
        assert_eq!(app.active_document, Some(1));
    }
}

#[test]
fn search_keyboard_owned_space_release_keeps_new_editor_space_and_unrelated_input() {
    let (mut app, commands) = app(8);
    focus(&mut app, "search_row:0");
    frame(
        &mut app,
        vec![
            edge(egui::Key::Space, true, false),
            egui::Event::Text(" ".into()),
        ],
    );
    settle(&mut app);
    let before = app.documents[0].text.clone();
    let ctx = app.editor_ctx.clone();
    let mut input = raw(
        &app,
        vec![
            edge(egui::Key::Space, false, false),
            edge(egui::Key::Space, true, false),
            egui::Event::Text(" ".into()),
        ],
    );
    eframe::App::raw_input_hook(&mut app, &ctx, &mut input);
    assert_eq!(
        input.events.len(),
        2,
        "only the original owned release is suppressed"
    );
    assert!(matches!(
        input.events[0],
        egui::Event::Key { pressed: true, .. }
    ));
    assert!(matches!(&input.events[1], egui::Event::Text(text) if text == " "));
    render(&mut app, input);
    assert_eq!(app.documents[0].text.len(), before.len() + 1);
    assert!(commands.try_recv().is_err());
}

fn load_other_surfaces(app: &mut CedarApp, commands: &Receiver<Command>) {
    let xml = "<testsuite name='Rows' tests='2' failures='0' errors='0' skipped='0'><testcase name='first'/><testcase name='second'/></testsuite>";
    let load = test_report_ui::Load {
        generation: app.generation,
        id: 999,
        path: "TEST.xml".into(),
    };
    app.test_report.path = load.path.clone();
    let report = test_reports::parse_report(xml).unwrap();
    assert_eq!(report.cases.len(), 2);
    app.test_report.snapshot = Some(test_report_ui::Snapshot {
        source: load,
        revision: "a".repeat(64),
        report,
    });
    app.explorer_set_mode(explorer_tree::Mode::Tree);
    app.explorer_expand("");
    let list = commands.try_recv().unwrap();
    reply(
        app,
        list.id,
        Ok(Payload::Entries {
            entries: vec![Entry {
                name: "unopened.txt".into(),
                path: "unopened.txt".into(),
                is_dir: false,
            }],
        }),
        true,
    );
    settle(app);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Surface {
    Search,
    Report,
    Explorer,
}

fn surface_focus(app: &mut CedarApp, surface: Surface) -> egui::Id {
    match surface {
        Surface::Search => app.tool = Tool::Search,
        Surface::Report => app.tool = Tool::Tests,
        Surface::Explorer => {}
    }
    settle(app);
    let id = match surface {
        Surface::Search => id(app, 0),
        Surface::Report => {
            report_keyboard::row_id(&app.test_report.snapshot.as_ref().unwrap().source, 0)
        }
        Surface::Explorer => explorer_tree::row_id(""),
    };
    app.editor_ctx.read_response(id).unwrap().request_focus();
    frame(app, vec![]);
    assert_eq!(focused(app), Some(id));
    id
}

fn surface_second(app: &CedarApp, surface: Surface) -> egui::Id {
    match surface {
        Surface::Search => id(app, 1),
        Surface::Report => {
            report_keyboard::row_id(&app.test_report.snapshot.as_ref().unwrap().source, 1)
        }
        Surface::Explorer => explorer_tree::row_id("unopened.txt"),
    }
}

fn claims(app: &CedarApp, surface: Surface, key: egui::Key) -> bool {
    match surface {
        Surface::Search => app.search_state.claims(key),
        Surface::Report => app.test_report.keyboard.claims(key),
        Surface::Explorer => app.explorer.claims(key),
    }
}

#[test]
fn search_keyboard_all_six_surface_transfers_preserve_original_hold_release_and_next_fresh() {
    for from in [Surface::Search, Surface::Report, Surface::Explorer] {
        for to in [Surface::Search, Surface::Report, Surface::Explorer] {
            if from == to {
                continue;
            }
            let (mut app, commands) = app(8);
            load_other_surfaces(&mut app, &commands);
            surface_focus(&mut app, from);
            frame(&mut app, vec![edge(egui::Key::ArrowDown, true, false)]);
            assert_eq!(focused(&app), Some(surface_second(&app, from)));
            let target = surface_focus(&mut app, to);
            for repeat in [false, true] {
                frame(&mut app, vec![edge(egui::Key::ArrowDown, true, repeat)]);
                assert_eq!(
                    focused(&app),
                    Some(target),
                    "foreign-held repeat moved {from:?} -> {to:?}"
                );
                assert!(claims(&app, from, egui::Key::ArrowDown));
                assert!(
                    !claims(&app, to, egui::Key::ArrowDown),
                    "foreign-held repeat acquired a second owner {from:?} -> {to:?}"
                );
            }
            frame(&mut app, vec![edge(egui::Key::ArrowDown, false, false)]);
            surface_focus(&mut app, from);
            frame(&mut app, key(egui::Key::ArrowDown));
            assert_eq!(
                focused(&app),
                Some(surface_second(&app, from)),
                "original lifetime stuck after {from:?} -> {to:?}"
            );
            surface_focus(&mut app, to);
            frame(&mut app, key(egui::Key::ArrowDown));
            assert_eq!(
                focused(&app),
                Some(surface_second(&app, to)),
                "recipient lifetime stuck after {from:?} -> {to:?}"
            );
            assert!(commands.try_recv().is_err());
        }
    }
}

#[test]
fn search_keyboard_foreign_claims_cannot_promote_mixed_batch_into_search_or_other_action() {
    for from in [Surface::Search, Surface::Report, Surface::Explorer] {
        for to in [Surface::Search, Surface::Report, Surface::Explorer] {
            if from == to {
                continue;
            }
            let (mut app, commands) = app(8);
            load_other_surfaces(&mut app, &commands);
            surface_focus(&mut app, from);
            frame(&mut app, vec![edge(egui::Key::ArrowDown, true, false)]);
            let target = surface_focus(&mut app, to);
            let before = app.navigation_epoch;
            frame(
                &mut app,
                vec![
                    edge(egui::Key::ArrowDown, true, true),
                    edge(egui::Key::Enter, true, false),
                    edge(egui::Key::Enter, false, false),
                ],
            );
            assert_eq!(focused(&app), Some(target));
            assert_eq!(app.navigation_epoch, before);
            assert!(app.test_report.selected.is_none());
            assert_eq!(
                app.explorer_visible_rows().len(),
                2,
                "mixed Enter changed the Explorer expansion"
            );
            assert!(
                commands.try_recv().is_err(),
                "filtered batch dispatched {from:?} -> {to:?}"
            );
            frame(
                &mut app,
                vec![
                    edge(egui::Key::ArrowDown, false, false),
                    edge(egui::Key::ArrowDown, true, false),
                    edge(egui::Key::ArrowDown, false, false),
                ],
            );
            // Explorer admits a single actionable press with ordered releases;
            // Search/Tests conservatively reject a leading-release batch.
            if to != Surface::Explorer {
                assert_eq!(focused(&app), Some(target));
            }
            surface_focus(&mut app, to);
            frame(&mut app, key(egui::Key::ArrowDown));
            assert_eq!(focused(&app), Some(surface_second(&app, to)));
        }
    }
}

#[test]
fn search_keyboard_pointer_allows_only_inactive_ime_lifecycle_notifications() {
    for enabled in [true, false] {
        let (mut app, commands) = app(8);
        focus(&mut app, "search_query");
        let pos = recorded_rect(&app, "search_row:2").center();
        assert_visible(&app, 2);
        let lifecycle = if enabled {
            egui::ImeEvent::Enabled
        } else {
            egui::ImeEvent::Disabled
        };
        let mut press = pointer(pos, true);
        press.push(egui::Event::Ime(lifecycle.clone()));
        frame(&mut app, press);
        frame(&mut app, vec![egui::Event::Ime(lifecycle)]);
        frame(&mut app, pointer(pos, false));
        assert!(recorded_response(&app, "search_row:2").clicked_by(egui::PointerButton::Primary));
        assert!(
            matches!(commands.try_recv().unwrap().op, Operation::Read { path } if path == "file-2.txt")
        );
        assert!(commands.try_recv().is_err());
    }
    for composition in [
        egui::ImeEvent::Preedit(String::new()),
        egui::ImeEvent::Preedit("text".into()),
        egui::ImeEvent::Commit(String::new()),
        egui::ImeEvent::Commit("text".into()),
    ] {
        let (mut app, commands) = app(8);
        let pos = recorded_rect(&app, "search_row:2").center();
        frame(&mut app, pointer(pos, true));
        frame(&mut app, vec![egui::Event::Ime(composition)]);
        frame(&mut app, vec![egui::Event::Ime(egui::ImeEvent::Disabled)]);
        frame(&mut app, pointer(pos, false));
        assert!(
            commands.try_recv().is_err(),
            "composition must cancel the pending pointer press"
        );
    }
}

#[test]
fn search_keyboard_workspace_changing_hello_preserves_held_lifetime_until_release() {
    for held in [egui::Key::Enter, egui::Key::Space] {
        let (mut app, commands) = app(8);
        focus(&mut app, "search_row:0");
        frame(&mut app, vec![edge(held, true, false)]);
        assert!(commands.try_recv().is_err());
        // Exercise the actual accepted Hello transition after an explicit
        // generation retirement; no backend process is launched by this test.
        app.search_state.retire();
        app.generation += 1;
        app.state = ConnectionState::Connecting;
        app.connecting_form = Some(ConnectForm {
            local_root: "/other".into(),
            ..Default::default()
        });
        reply(
            &mut app,
            0,
            Ok(Payload::Hello {
                protocol: cedar_protocol::PROTOCOL_VERSION,
                root: "/other".into(),
                agent: Some(agent_support::full_test_agent()),
            }),
            true,
        );
        assert!(matches!(
            commands.try_recv().unwrap().op,
            Operation::List { .. }
        ));
        assert!(app.search_state.accepted.is_none());
        assert!(app.search_results.is_empty());
        settle(&mut app);
        let ctx = app.editor_ctx.clone();
        let mut repeat = raw(&app, vec![edge(held, true, true)]);
        eframe::App::raw_input_hook(&mut app, &ctx, &mut repeat);
        assert!(
            repeat.events.is_empty(),
            "held {held:?} leaked through accepted workspace Hello"
        );
        render(&mut app, repeat);
        let mut release = raw(&app, vec![edge(held, false, false)]);
        eframe::App::raw_input_hook(&mut app, &ctx, &mut release);
        assert!(release.events.is_empty());
        render(&mut app, release);
        app.search();
        let search = commands.try_recv().unwrap();
        reply(
            &mut app,
            search.id,
            Ok(Payload::Matches {
                matches: rows(8),
                truncated: false,
            }),
            true,
        );
        settle(&mut app);
        focus(&mut app, "search_row:2");
        frame(&mut app, key(held));
        assert!(
            matches!(commands.try_recv().unwrap().op, Operation::Read { path } if path == "file-2.txt")
        );
        assert!(commands.try_recv().is_err());
    }
}
