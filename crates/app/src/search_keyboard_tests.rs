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

fn tab_from_search_submit_to_first_row(app: &mut CedarApp) {
    assert_eq!(
        focused(app),
        Some(recorded_response(app, "search_submit").id)
    );
    let area_id = recorded_scroll_id(app).with("area");
    if app
        .editor_ctx
        .read_response(area_id)
        .is_some_and(|response| response.sense.is_focusable())
    {
        frame(app, key(egui::Key::Tab));
        assert_eq!(focused(app), Some(area_id));
    }
    let group = recorded_response(app, "search_group:0");
    assert!(group.enabled() && group.sense.is_focusable());
    for expected in [group.id, id(app, 0)] {
        frame(app, key(egui::Key::Tab));
        assert_eq!(focused(app), Some(expected));
    }
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
    tab_from_search_submit_to_first_row(&mut app);
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
    assert_eq!(
        focused(&app),
        Some(recorded_response(&app, "search_group:0").id)
    );
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
fn search_keyboard_enter_preview_and_double_click_open_dirty_buffer_preserving_undo() {
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
        let cursor = app.documents[0].cursor;
        let history = pointer_editor_history(&app);
        match activation {
            0 => {
                frame(&mut app, key(egui::Key::Enter));
            }
            1 => {
                let mut events = key(egui::Key::Space);
                events.insert(1, egui::Event::Text(" ".into()));
                frame(&mut app, events);
                settle(&mut app);
                assert_eq!(app.search_state.selected_index(), Some(0));
                assert!(app.has_search_preview());
                assert_eq!(
                    app.search_preview_text(),
                    Some(app.documents[0].text.as_str())
                );
                assert_eq!(app.navigation_epoch, departure);
                assert_eq!(app.documents[0].cursor, cursor);
                assert!(pointer_editor_history(&app) == history);
                assert!(app.location_history.back.is_empty());
                assert!(commands.try_recv().is_err());
                assert_eq!(focused(&app), Some(id(&app, 0)));
                frame(&mut app, key(egui::Key::Enter));
            }
            _ => {
                assert_visible(&app, 0);
                let pos = recorded_rect(&app, "search_row:0").center();
                frame(&mut app, pointer(pos, true));
                assert!(recorded_response(&app, "search_row:0").is_pointer_button_down_on());
                frame(&mut app, pointer(pos, false));
                assert!(recorded_response(&app, "search_row:0")
                    .clicked_by(egui::PointerButton::Primary));
                assert!(!recorded_response(&app, "search_row:0")
                    .double_clicked_by(egui::PointerButton::Primary));
                assert_eq!(app.search_state.selected_index(), Some(0));
                assert_eq!(app.navigation_epoch, departure);
                assert_eq!(app.documents[0].cursor, cursor);
                assert!(pointer_editor_history(&app) == history);
                assert!(app.location_history.back.is_empty());
                assert!(!app.has_search_preview());
                assert!(commands.try_recv().is_err());
                frame(&mut app, pointer(pos, true));
                assert!(recorded_response(&app, "search_row:0").is_pointer_button_down_on());
                frame(&mut app, pointer(pos, false));
                assert!(recorded_response(&app, "search_row:0")
                    .double_clicked_by(egui::PointerButton::Primary));
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
        "Search incomplete; more matches may exist."
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
        frame(&mut app, vec![]);
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
        let original_source = app.search_state.accepted.clone();
        app.search();
        let older = commands.try_recv().unwrap();
        app.search();
        assert!(
            commands.try_recv().is_err(),
            "only one physical Search may be pending"
        );
        // The completed backend request releases its physical slot before the
        // staged result is admitted after current-frame controls. A fresh
        // submission must make that queued older completion inert.
        reply(&mut app, older.id, result.clone(), true);
        app.search();
        let newer = commands.try_recv().unwrap();
        assert_ne!(newer.id, older.id);
        app.error = Some("newer error must survive".into());
        let staged_notice = app.notice.clone();
        frame(&mut app, vec![]);
        assert_eq!(app.search_results.len(), 8);
        assert_eq!(app.search_state.accepted, original_source);
        assert_eq!(app.search_state.pending.as_ref().unwrap().request, newer.id);
        assert_eq!(app.error.as_deref(), Some("newer error must survive"));
        assert_eq!(app.notice, staged_notice);
        assert!(!app.search_rows_active());
        assert!(commands.try_recv().is_err());
        reply(
            &mut app,
            newer.id,
            Ok(Payload::Matches {
                matches: rows(2),
                truncated: false,
            }),
            true,
        );
        frame(&mut app, vec![]);
        app.error = Some("newer error must survive".into());
        let notice = app.notice.clone();
        reply(&mut app, older.id, result, true);
        frame(&mut app, vec![]);
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
    focus(&mut app, "search_query");
    frame(&mut app, vec![egui::Event::Text("x".into())]);
    assert!(app.search_state.pending.is_none());
    assert!(!app.search_rows_active());
    app.search();
    assert!(
        commands.try_recv().is_err(),
        "retiring a query must not release its physical Search slot"
    );
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
        assert_eq!(app.search_state.selected_index(), Some(0));
        assert!(!app.has_search_preview());
    }
}

#[test]
fn search_keyboard_owned_space_release_keeps_new_editor_space_and_unrelated_input() {
    let (mut app, commands) = app(8);
    focus(&mut app, "search_row:0");
    let epoch = app.navigation_epoch;
    frame(
        &mut app,
        vec![
            edge(egui::Key::Space, true, false),
            egui::Event::Text(" ".into()),
        ],
    );
    settle(&mut app);
    assert!(app.has_search_preview());
    assert_eq!(app.navigation_epoch, epoch);
    assert!(app.search_state.claims(egui::Key::Space));
    // Preview retains the result focus. Transfer it through the actual editor
    // control while the original Space lifetime is still held.
    click_control(&mut app, "editor");
    assert_eq!(focused(&app), Some(egui::Id::new(("editor", 1_u64))));
    assert!(app.search_state.claims(egui::Key::Space));
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

const CROSS_SURFACE_REPORT_XML: &str = "<testsuite name='Rows' tests='2' failures='0' errors='0' skipped='0'><testcase name='first' time='0'/><testcase name='second' time='0'/></testsuite>";

fn cross_surface_report() -> test_reports::TestReport {
    let report = test_reports::parse_report(CROSS_SURFACE_REPORT_XML)
        .expect("cross-surface fixture must satisfy the strict report parser before input testing");
    assert_eq!(report.suite_name, "Rows");
    assert_eq!(
        report.counts,
        test_reports::TestCounts {
            total: 2,
            passed: 2,
            failed: 0,
            errors: 0,
            skipped: 0,
            unsupported: 0,
        }
    );
    assert_eq!(report.cases.len(), 2);
    for (case, name) in report.cases.iter().zip(["first", "second"]) {
        assert_eq!(case.name, name);
        assert_eq!(case.status, test_reports::TestStatus::Passed);
        assert_eq!(case.duration_seconds, 0.0);
    }
    report
}

#[test]
fn search_keyboard_cross_surface_report_fixture_is_strict_and_complete() {
    cross_surface_report();
    // The testcase duration is mandatory independently of the suite counters.
    let missing_time = CROSS_SURFACE_REPORT_XML.replacen(" time='0'", "", 1);
    assert_eq!(
        test_reports::parse_report(&missing_time),
        Err(test_reports::ReportError::InvalidStructure(
            "required attribute missing"
        ))
    );
}

fn load_other_surfaces(app: &mut CedarApp, commands: &Receiver<Command>) {
    let report = cross_surface_report();
    let load = test_report_ui::Load {
        generation: app.generation,
        id: 999,
        path: "TEST.xml".into(),
    };
    app.test_report.path = load.path.clone();
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
        assert_eq!(app.search_state.selected_index(), Some(2));
        assert_eq!(focused(&app), Some(id(&app, 2)));
        assert_eq!(app.active_document, Some(1));
        assert!(app.location_history.back.is_empty());
        assert!(!app.has_search_preview());
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
        assert_eq!(app.search_state.selected_index(), Some(0));
        assert!(!app.has_search_preview());
        assert!(
            commands.try_recv().is_err(),
            "composition must cancel the pending pointer press"
        );
    }
}

// This composes the route missing from the small-row pointer fixtures: native
// focus traversal, End scrolling, asynchronous Open completion, then a click on
// the preceding visible row. It is controlled egui input, not a native trace.
const SCROLLED_DRAFT: &str = "a-draft.txt";
const SCROLLED_UNOPENED: &str = "b-unopened.txt";
const SCROLLED_COUNT: usize = 192;

fn scrolled_text(path: &str) -> String {
    let mut text = format!("{path} header α😀 e\u{301} café Ω\r\n");
    for index in 0..96 {
        text.push_str(&format!("needle {index:03} α😀 e\u{301} café Ω\r\n"));
    }
    assert!(text.len() < 1024 * 1024);
    text
}

fn command_modifiers() -> egui::Modifiers {
    egui::Modifiers {
        ctrl: true,
        command: true,
        ..egui::Modifiers::NONE
    }
}

fn clipped_center(app: &CedarApp, probe: &str) -> egui::Pos2 {
    let rect = recorded_rect(app, probe);
    let clip = recorded_clip(app, probe);
    let visible = rect.intersect(clip);
    assert!(visible.is_finite() && visible.width() > 0.0 && visible.height() > 0.0);
    visible.center()
}

fn click_control(app: &mut CedarApp, probe: &str) {
    let pos = clipped_center(app, probe);
    frame(app, pointer(pos, true));
    frame(app, pointer(pos, false));
}

fn prepare_scrolled_viewport(app: &mut CedarApp) {
    // Establish a real user-chosen height while Search is still empty. Never
    // write tools_height, ScrollArea state, or PanelState in this reproducer.
    let handle_id = tools_resize::native_resize_id();
    let handle = app.editor_ctx.read_response(handle_id).unwrap();
    assert!(handle.enabled() && handle.sense.senses_drag());
    let start = handle.rect.center();
    let panel = egui::containers::panel::PanelState::load(&app.editor_ctx, egui::Id::new("tools"))
        .unwrap()
        .rect;
    let target = egui::pos2(start.x, panel.bottom() - 480.0);
    frame(app, pointer(start, true));
    frame(app, vec![egui::Event::PointerMoved(target)]);
    frame(app, pointer(target, false));
    assert!(app
        .tools_height
        .is_some_and(|height| (height - 480.0).abs() <= 1.0));
    frame(app, vec![]);
}

fn scrolled_pointer_app() -> (CedarApp, Receiver<Command>) {
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
    let saved = scrolled_text(SCROLLED_DRAFT);
    app.documents.push(Document::new(
        1,
        SCROLLED_DRAFT.into(),
        saved.clone(),
        "a".repeat(64),
    ));
    app.active_document = Some(1);
    app.next_document = 2;
    let (worker, commands) = Worker::recording();
    app.worker = Some(worker);
    settle(&mut app);

    click_control(&mut app, "editor");
    assert_eq!(focused(&app), Some(egui::Id::new(("editor", 1_u64))));
    frame(&mut app, modified_key(egui::Key::Home, command_modifiers()));
    frame(
        &mut app,
        modified_key(egui::Key::End, egui::Modifiers::SHIFT),
    );
    frame(&mut app, modified_key(egui::Key::D, command_modifiers()));
    let header = saved.split_inclusive('\n').next().unwrap();
    assert_eq!(app.documents[0].text, format!("{header}{saved}"));
    assert_eq!(app.documents[0].saved_text, saved);
    assert!(app.documents[0].dirty());
    assert!(app.documents[0]
        .text
        .lines()
        .nth(2)
        .unwrap()
        .starts_with("needle 000"));
    frame(&mut app, modified_key(egui::Key::J, command_modifiers()));
    assert!(app.tools_open && app.tool == Tool::Search);
    settle(&mut app);
    prepare_scrolled_viewport(&mut app);
    click_control(&mut app, "search_query");
    assert_eq!(
        focused(&app),
        Some(recorded_response(&app, "search_query").id)
    );
    frame(&mut app, vec![egui::Event::Text("needle".into())]);
    frame(&mut app, key(egui::Key::Enter));
    let search = commands.try_recv().unwrap();
    assert!(matches!(&search.op, Operation::Search { query, limit: 500 } if query == "needle"));
    assert!(commands.try_recv().is_err());
    let matches = [SCROLLED_DRAFT, SCROLLED_UNOPENED]
        .into_iter()
        .flat_map(|path| {
            (0..96).map(move |index| SearchMatch {
                path: path.into(),
                line: index + 2,
                text: format!("needle {index:03} α😀 e\u{301} café Ω"),
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(matches.len(), SCROLLED_COUNT);
    assert!(validate(&matches).is_ok());
    reply(
        &mut app,
        search.id,
        Ok(Payload::Matches {
            matches,
            truncated: false,
        }),
        true,
    );
    frame(&mut app, vec![]);

    click_control(&mut app, "search_query");
    frame(&mut app, key(egui::Key::Tab));
    assert_eq!(
        focused(&app),
        Some(recorded_response(&app, "search_submit").id)
    );
    tab_from_search_submit_to_first_row(&mut app);
    frame(&mut app, key(egui::Key::End));
    assert_eq!(focused(&app), Some(id(&app, SCROLLED_COUNT - 1)));
    // End's reveal is applied by the ScrollArea on the following layout frame.
    frame(&mut app, vec![]);
    assert_visible(&app, SCROLLED_COUNT - 1);
    assert_visible(&app, SCROLLED_COUNT - 2);
    frame(&mut app, key(egui::Key::Enter));
    let read = commands.try_recv().unwrap();
    assert!(matches!(&read.op, Operation::Read { path } if path == SCROLLED_UNOPENED));
    assert!(commands.try_recv().is_err());
    assert_eq!(app.documents.len(), 1);
    reply(
        &mut app,
        read.id,
        Ok(Payload::File {
            path: SCROLLED_UNOPENED.into(),
            text: scrolled_text(SCROLLED_UNOPENED),
            revision: "b".repeat(64),
        }),
        true,
    );
    // This is the actual Open completion frame; the next frame is pointer arrival.
    frame(&mut app, vec![]);
    assert_eq!(app.documents.len(), 2);
    assert_eq!(app.active().unwrap().path, SCROLLED_UNOPENED);
    assert_eq!(app.active().unwrap().cursor, (97, 1));
    assert!(app.active().unwrap().jump_to.is_none());
    assert!(!app.navigation.restore_focus);
    assert!(app.location_history.pending.is_none());
    assert_eq!(app.location_history.back.len(), 1);
    assert_eq!(focused(&app), Some(egui::Id::new(("editor", 2_u64))));
    assert!(app.search_rows_active());
    assert_visible(&app, SCROLLED_COUNT - 2);
    assert!(commands.try_recv().is_err());
    (app, commands)
}

struct PointerFrame {
    case: RawMotionAt,
    edge: PointerEdge,
    target_index: usize,
    target_id: egui::Id,
    edge_pointer: egui::Pos2,
    pre_hook_previous_render_target: Option<PointerRow>,
    pre_hook_previous_pointer: Option<egui::Pos2>,
    pre_hook_scroll: PointerScroll,
    captured: search_keyboard::PointerProbe,
    passes: Vec<PointerPass>,
}

struct PointerPass {
    number: u64,
    pre_update_scroll: PointerScroll,
    post_update_scroll: PointerScroll,
    input_pointer: Option<egui::Pos2>,
    input_pointer_delta: egui::Vec2,
    input_pointer_any_down: bool,
    input_pointer_any_click: bool,
    post_update_scroll_area_dragged: Option<bool>,
    consumed: search_keyboard::PointerProbe,
    rows: Vec<PointerRow>,
}

const POINTER_RESPONSE_PROBE: &str = "search_pointer_response_probe";
const POINTER_DIAGNOSTIC_PASSES: usize = 8;
const POINTER_DIAGNOSTIC_TARGET_ROWS: usize = 2;

#[derive(Clone, Copy, Debug)]
enum PointerEdge {
    Arrival,
    Press,
    Held,
    Release,
}

// Diagnostics must not panic before the original assertion, expose content,
// or serialize non-finite geometry. Missing/non-finite values are statuses.
struct PointerNumbers<const N: usize>(Option<[f32; N]>);

impl<const N: usize> std::fmt::Debug for PointerNumbers<N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            Some(values) if values.iter().all(|value| value.is_finite()) => {
                std::fmt::Debug::fmt(&values, f)
            }
            Some(_) => f.write_str("non_finite"),
            None => f.write_str("missing"),
        }
    }
}

fn pointer_rect(rect: egui::Rect) -> PointerNumbers<4> {
    PointerNumbers(Some([rect.min.x, rect.min.y, rect.max.x, rect.max.y]))
}

fn pointer_pos(pos: Option<egui::Pos2>) -> PointerNumbers<2> {
    PointerNumbers(pos.map(|pos| [pos.x, pos.y]))
}

#[derive(PartialEq)]
struct PointerScroll {
    id: Option<egui::Id>,
    offset: Option<egui::Vec2>,
}

impl PointerScroll {
    fn load(ctx: &egui::Context) -> Self {
        let id = ctx.data(|data| data.get_temp::<egui::Id>(egui::Id::new(SCROLL)));
        let offset = id
            .and_then(|id| egui::scroll_area::State::load(ctx, id))
            .map(|state| state.offset);
        Self { id, offset }
    }
}

impl std::fmt::Debug for PointerScroll {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Scroll")
            .field("id", &self.id)
            .field(
                "offset",
                &PointerNumbers(self.offset.map(|offset| [offset.x, offset.y])),
            )
            .finish()
    }
}

#[derive(Clone)]
struct PointerRow {
    pass: u64,
    sizing: bool,
    index: usize,
    id: egui::Id,
    rect: egui::Rect,
    clip: egui::Rect,
    enabled: bool,
    down: bool,
    clicked: bool,
    cedar_click: Option<usize>,
}

impl std::fmt::Debug for PointerRow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Gate")
            .field("pass", &self.pass)
            .field("sizing", &self.sizing)
            .field("index", &self.index)
            .field("id", &self.id)
            .field("rect", &pointer_rect(self.rect))
            .field("clip", &pointer_rect(self.clip))
            .field("enabled", &self.enabled)
            .field("down", &self.down)
            .field("clicked", &self.clicked)
            .field("cedar_click", &self.cedar_click)
            .finish()
    }
}

impl std::fmt::Debug for PointerFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let target = self.target_index;
        writeln!(
            f,
            "case={:?} edge={:?} target_index={target} target_id={:?} edge_pointer={:?}",
            self.case,
            self.edge,
            self.target_id,
            pointer_pos(Some(self.edge_pointer)),
        )?;
        writeln!(
            f,
            "pre_hook_previous_render_target={:?} pre_hook_previous_pointer={:?} pre_hook_scroll={:?} post_hook_ticket={:?}",
            self.pre_hook_previous_render_target,
            pointer_pos(self.pre_hook_previous_pointer),
            self.pre_hook_scroll,
            self.captured,
        )?;
        // Keep the retained final pass even if egui's configured pass count
        // grows. Assertions still examine every pass and every row.
        for (index, pass) in self.passes.iter().enumerate().filter(|(index, _)| {
            *index < POINTER_DIAGNOSTIC_PASSES - 1 || *index + 1 == self.passes.len()
        }) {
            writeln!(
                f,
                "pass={} retained={} recorded_row_count={} pre_update_scroll={:?} post_update_scroll={:?} input_pointer={:?} input_pointer_delta={:?} input_pointer_any_down={} input_pointer_any_click={} post_update_scroll_area_dragged={:?} post_update_ticket={:?}",
                pass.number,
                index + 1 == self.passes.len(),
                pass.rows.len(),
                pass.pre_update_scroll,
                pass.post_update_scroll,
                pointer_pos(pass.input_pointer),
                PointerNumbers(Some([pass.input_pointer_delta.x, pass.input_pointer_delta.y])),
                pass.input_pointer_any_down,
                pass.input_pointer_any_click,
                pass.post_update_scroll_area_dragged,
                pass.consumed,
            )?;
            let targets = pass.rows.iter().filter(|row| row.index == target);
            let target_count = targets.clone().count();
            writeln!(f, "gate_target_count={target_count}")?;
            for row in targets.take(POINTER_DIAGNOSTIC_TARGET_ROWS) {
                writeln!(f, "actual_target_response={row:?}")?;
            }
            writeln!(
                f,
                "omitted_target_rows={}",
                target_count.saturating_sub(POINTER_DIAGNOSTIC_TARGET_ROWS)
            )?;
        }
        write!(
            f,
            "pass_count={} omitted_passes={}",
            self.passes.len(),
            self.passes.len().saturating_sub(POINTER_DIAGNOSTIC_PASSES)
        )
    }
}

// Called next to the actual row_control response and before its click gate.
// Snapshot values here: Context::read_response would reconstruct the response
// later and can change focus, while clicked_by consults live per-frame input.
pub(super) fn record_pointer_response(
    ui: &egui::Ui,
    index: usize,
    response: &egui::Response,
    cedar_click: Option<usize>,
) {
    let probe = egui::Id::new(POINTER_RESPONSE_PROBE);
    let observe_all = ui
        .ctx()
        .data(|data| data.get_temp::<bool>(probe).unwrap_or(false));
    if !observe_all && index != SCROLLED_COUNT - 2 {
        return;
    }
    let row = PointerRow {
        pass: ui.ctx().cumulative_pass_nr(),
        sizing: ui.is_sizing_pass(),
        index,
        id: response.id,
        rect: response.rect,
        clip: ui.clip_rect(),
        enabled: response.enabled(),
        down: response.is_pointer_button_down_on(),
        clicked: response.clicked_by(egui::PointerButton::Primary),
        cedar_click,
    };
    ui.ctx().data_mut(|data| {
        // A passive singleton also records the fixture's previous layout.
        // This is the actual response; never reconstruct it via read_response.
        if index == SCROLLED_COUNT - 2 {
            data.insert_temp(probe.with("previous_target"), row.clone());
        }
        if observe_all {
            data.get_temp_mut_or_default::<Vec<PointerRow>>(probe.with("rows"))
                .push(row);
        }
    });
}

fn observe_pointer_frame(
    app: &mut CedarApp,
    original: Vec<egui::Event>,
    case: RawMotionAt,
    edge: PointerEdge,
    target_id: egui::Id,
    edge_pointer: egui::Pos2,
) -> PointerFrame {
    // The independent original survives the hook unchanged. Neither the probe
    // nor this helper inserts Cedar tickets or mutates egui focus/ownership.
    let mut input = raw(app, original.clone());
    let ctx = app.editor_ctx.clone();
    let probe = egui::Id::new(POINTER_RESPONSE_PROBE);
    let pre_hook_previous_render_target =
        ctx.data(|data| data.get_temp::<PointerRow>(probe.with("previous_target")));
    let pre_hook_previous_pointer = ctx.input(|input| input.pointer.interact_pos());
    let pre_hook_scroll = PointerScroll::load(&ctx);
    let target_index = (0..app.search_results.len())
        .find(|index| id(app, *index) == target_id)
        .unwrap();
    eframe::App::raw_input_hook(app, &ctx, &mut input);
    assert_original_pointer_events(&input.events, &original);
    let captured = app.search_state.pointer_probe();
    let mut native = eframe::Frame::_new_kittest();
    let mut passes = Vec::new();
    ctx.data_mut(|data| data.insert_temp(probe, true));
    let _ = ctx.run(input, |ctx| {
        ctx.data_mut(|data| data.insert_temp(probe.with("rows"), Vec::<PointerRow>::new()));
        let pre_update_scroll = PointerScroll::load(ctx);
        eframe::App::update(app, ctx, &mut native);
        let post_update_scroll = PointerScroll::load(ctx);
        let post_update_scroll_area_dragged = post_update_scroll
            .id
            .map(|id| ctx.is_being_dragged(id.with("area")));
        passes.push(PointerPass {
            number: ctx.cumulative_pass_nr(),
            pre_update_scroll,
            post_update_scroll,
            input_pointer: ctx.input(|input| input.pointer.interact_pos()),
            input_pointer_delta: ctx.input(|input| input.pointer.delta()),
            input_pointer_any_down: ctx.input(|input| input.pointer.any_down()),
            input_pointer_any_click: ctx.input(|input| input.pointer.any_click()),
            post_update_scroll_area_dragged,
            consumed: app.search_state.pointer_probe(),
            rows: ctx.data_mut(|data| data.remove_temp(probe.with("rows")).unwrap()),
        });
    });
    ctx.data_mut(|data| data.remove::<bool>(probe));
    assert!(!passes.is_empty());
    assert!(passes
        .windows(2)
        .all(|pair| pair[0].number < pair[1].number));
    PointerFrame {
        case,
        edge,
        target_index,
        target_id,
        edge_pointer,
        pre_hook_previous_render_target,
        pre_hook_previous_pointer,
        pre_hook_scroll,
        captured,
        passes,
    }
}

fn assert_original_pointer_events(actual: &[egui::Event], original: &[egui::Event]) {
    assert_eq!(actual.len(), original.len());
    for (index, (actual, original)) in actual.iter().zip(original).enumerate() {
        // NaN is not equal to itself. Compare the untouched raw companion's
        // bits; this asserts preservation, not sanitization or vector validity.
        let same = match (actual, original) {
            (egui::Event::MouseMoved(actual), egui::Event::MouseMoved(original)) => {
                actual.x.to_bits() == original.x.to_bits()
                    && actual.y.to_bits() == original.y.to_bits()
            }
            _ => actual == original,
        };
        assert!(same, "the hook changed pointer-batch event {index}");
    }
}

fn assert_pointer_owners(
    frame: &PointerFrame,
    target_id: egui::Id,
    pos: egui::Pos2,
    down: bool,
    clicked: bool,
    cedar_click: bool,
) {
    let target = SCROLLED_COUNT - 2;
    for pass in &frame.passes {
        for row in &pass.rows {
            assert_eq!(row.pass, pass.number, "{frame:?}");
            assert!(
                !row.down || (down && row.index == target),
                "unexpected press owner: {frame:?}"
            );
            assert!(
                !row.clicked || (clicked && row.index == target),
                "unexpected click owner: {frame:?}"
            );
            assert!(
                row.cedar_click.is_none() || (cedar_click && row.cedar_click == Some(target)),
                "unexpected Cedar click admission: {frame:?}"
            );
        }
    }
    assert_eq!(
        frame.passes.iter().flat_map(|pass| &pass.rows).any(|row| {
            !row.sizing && row.index == target && row.clicked && row.cedar_click == Some(target)
        }),
        cedar_click,
        "the actual response and Cedar click ticket must coincide in one pass: {frame:?}"
    );
    // A discarded/sizing pass alone cannot satisfy the ownership witness.
    let retained = frame.passes.last().unwrap();
    let rows = retained
        .rows
        .iter()
        .filter(|row| !row.sizing)
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), SCROLLED_COUNT, "{frame:?}");
    let row = rows.into_iter().find(|row| row.index == target).unwrap();
    assert_eq!(row.id, target_id, "{frame:?}");
    assert!(row.enabled, "{frame:?}");
    assert!(row.rect.intersect(row.clip).contains(pos), "{frame:?}");
    assert_eq!(row.down, down, "retained-pass press ownership: {frame:?}");
    assert_eq!(
        row.clicked, clicked,
        "retained-pass click ownership: {frame:?}"
    );
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RawMotionAt {
    None,
    Press,
    Held,
    Release,
}

type PointerSelection = (usize, bool, usize, bool);
type PointerHistory = [Vec<(PointerSelection, String)>; 2];

fn pointer_editor_history(app: &CedarApp) -> Vec<(u64, PointerSelection, PointerHistory)> {
    let selection = |range: egui::text::CCursorRange| {
        (
            range.primary.index,
            range.primary.prefer_next_row,
            range.secondary.index,
            range.secondary.prefer_next_row,
        )
    };
    app.documents
        .iter()
        .map(|document| {
            let state =
                egui::TextEdit::load_state(&app.editor_ctx, egui::Id::new(("editor", document.id)))
                    .unwrap();
            let cursor = state.cursor.char_range().unwrap();
            let mut snapshots = PointerHistory::default();
            // Observe reachable Undo/Redo states on disposable clones, including
            // selection affinity. Never compare the Undoer's per-frame clocks or
            // send Undo/Redo input, store state, or render another frame here.
            for (index, redo) in [false, true].into_iter().enumerate() {
                let mut history = state.undoer();
                let mut current = (cursor, document.text.clone());
                for _ in 0..=editor_state::MAX_UNDO_STATES + 1 {
                    let next = if redo {
                        history.redo(&current)
                    } else {
                        history.undo(&current)
                    }
                    .cloned();
                    let Some(next) = next else {
                        break;
                    };
                    snapshots[index].push((selection(next.0), next.1.clone()));
                    current = next;
                }
                assert!(snapshots[index].len() <= editor_state::MAX_UNDO_STATES + 1);
            }
            (document.id, selection(cursor), snapshots)
        })
        .collect()
}

fn assert_scrolled_pointer_companion(raw_motion: RawMotionAt) {
    let (mut app, commands) = scrolled_pointer_app();
    let target = SCROLLED_COUNT - 2;
    let target_id = id(&app, target);
    let ticket = Some((target, target_id));
    let pos = clipped_center(&app, &format!("search_row:{target}"));
    let before = app.navigation_epoch;
    let dirty = app.documents[0].text.clone();
    let saved = app.documents[0].saved_text.clone();
    let original_departure = app.location_history.back[0];
    let selection = egui::TextEdit::load_state(&app.editor_ctx, egui::Id::new(("editor", 2_u64)))
        .unwrap()
        .cursor
        .char_range()
        .unwrap();
    assert_eq!(
        selection.primary.index,
        line_start(&app.active().unwrap().text, 97)
    );
    assert_eq!(selection.secondary.index, selection.primary.index);
    let edit_version = app.active().unwrap().edit_version;
    let editor_documents = |app: &CedarApp| {
        app.documents
            .iter()
            .map(|document| {
                (
                    document.id,
                    document.path.clone(),
                    document.text.clone(),
                    document.saved_text.clone(),
                    document.revision.clone(),
                    document.cursor,
                    document.jump_to,
                    document.scroll_to,
                    document.edit_version,
                    document.dirty(),
                )
            })
            .collect::<Vec<_>>()
    };
    let documents_before = editor_documents(&app);
    let history_before = pointer_editor_history(&app);
    let active_before = app.active_document;
    let focus_before = focused(&app);
    let next_request_before = app.next_request;
    let accepted_before = app.search_state.accepted.clone();
    let pending_before = app.search_state.pending.clone();
    let tickets_before = app.search_state.pointer_probe();
    assert_eq!(tickets_before.pressed, None);
    assert_eq!(tickets_before.click, None);
    assert!(app.pending.is_empty());
    assert!(app.location_history.forward.is_empty());
    assert!(!app.editor_ctx.input(|input| input.pointer.any_down()));
    assert!(!app
        .editor_ctx
        .is_being_dragged(recorded_scroll_id(&app).with("area")));

    // Model an already-arrived-pointer click explicitly: exactly one movement-
    // only frame, then the existing press/held/release variants at this position.
    // This does not model arbitrary native grouping or repair the historical
    // combined-arrival-and-press case whose nonzero delta scrolled the rows.
    let arrival = observe_pointer_frame(
        &mut app,
        vec![egui::Event::PointerMoved(pos)],
        raw_motion,
        PointerEdge::Arrival,
        target_id,
        pos,
    );
    // At most four distinct edges are retained (Arrival/Press/Held/Release),
    // with the same bounded pass and row diagnostics for every edge.
    eprintln!("{arrival:?}");
    assert_pointer_owners(&arrival, target_id, pos, false, false, false);
    let previous_pointer = arrival.pre_hook_previous_pointer.unwrap();
    assert!(
        previous_pointer.is_finite() && previous_pointer != pos,
        "{arrival:?}"
    );
    assert!(
        arrival.passes[0].input_pointer_delta == pos - previous_pointer,
        "{arrival:?}"
    );
    let previous = arrival.pre_hook_previous_render_target.as_ref().unwrap();
    assert_eq!(previous.index, target);
    assert_eq!(previous.id, target_id);
    assert!(!previous.sizing && previous.enabled);
    assert!(previous.rect.is_finite() && previous.clip.is_finite());
    assert!(previous.rect.intersect(previous.clip).contains(pos));
    assert!(arrival.pre_hook_scroll.id.is_some());
    assert!(arrival.pre_hook_scroll.offset.unwrap().is_finite());
    assert_eq!(arrival.captured, tickets_before, "{arrival:?}");
    for pass in &arrival.passes {
        assert!(pass.input_pointer == Some(pos), "{arrival:?}");
        assert!(!pass.input_pointer_any_down, "{arrival:?}");
        assert!(!pass.input_pointer_any_click, "{arrival:?}");
        assert_eq!(
            pass.post_update_scroll_area_dragged,
            Some(false),
            "{arrival:?}"
        );
        assert_eq!(
            pass.pre_update_scroll, arrival.pre_hook_scroll,
            "{arrival:?}"
        );
        assert_eq!(
            pass.post_update_scroll, arrival.pre_hook_scroll,
            "{arrival:?}"
        );
        assert_eq!(pass.consumed, tickets_before, "{arrival:?}");
        for row in pass.rows.iter().filter(|row| row.index == target) {
            assert_eq!(row.id, previous.id, "{arrival:?}");
            assert!(row.rect == previous.rect, "{arrival:?}");
            assert!(row.clip == previous.clip, "{arrival:?}");
            assert_eq!(row.enabled, previous.enabled, "{arrival:?}");
        }
    }
    assert_eq!(clipped_center(&app, &format!("search_row:{target}")), pos);
    assert_eq!(app.active_document, active_before);
    assert_eq!(focused(&app), focus_before);
    assert!(editor_documents(&app) == documents_before, "{arrival:?}");
    assert!(
        pointer_editor_history(&app) == history_before,
        "{arrival:?}"
    );
    let arrived_selection =
        egui::TextEdit::load_state(&app.editor_ctx, egui::Id::new(("editor", 2_u64)))
            .unwrap()
            .cursor
            .char_range()
            .unwrap();
    assert!(location_history::same_selection(
        arrived_selection,
        selection
    ));
    assert_eq!(app.navigation_epoch, before);
    assert!(!app.navigation.restore_focus);
    assert!(app.location_history.pending.is_none());
    assert_eq!(app.location_history.back.len(), 1);
    assert!(app.location_history.forward.is_empty());
    let arrival_departure = app.location_history.back[0];
    assert_eq!(arrival_departure.document, original_departure.document);
    assert_eq!(arrival_departure.generation, original_departure.generation);
    assert_eq!(
        arrival_departure.edit_version,
        original_departure.edit_version
    );
    assert!(location_history::same_selection(
        arrival_departure.selection,
        original_departure.selection
    ));
    assert!(app.search_state.accepted == accepted_before, "{arrival:?}");
    assert!(app.search_state.pending == pending_before, "{arrival:?}");
    assert_eq!(app.search_state.pointer_probe(), tickets_before);
    assert_eq!(app.next_request, next_request_before);
    assert!(app.pending.is_empty());
    assert!(commands.try_recv().is_err());

    let mut press = pointer(pos, true);
    if raw_motion == RawMotionAt::Press {
        press.insert(1, egui::Event::MouseMoved(egui::vec2(1.0, -1.0)));
    }
    let down = observe_pointer_frame(
        &mut app,
        press,
        raw_motion,
        PointerEdge::Press,
        target_id,
        pos,
    );
    // The test harness captures these bounded witnesses: a later held/release
    // failure must retain the earlier press offset/delta without another frame.
    eprintln!("{down:?}");
    assert_pointer_owners(&down, target_id, pos, true, false, false);
    assert_eq!(down.captured.pressed, ticket);
    assert_eq!(down.captured.click, None);
    for pass in &down.passes {
        assert!(pass.input_pointer == Some(pos), "{down:?}");
        assert!(pass.input_pointer_delta == egui::Vec2::ZERO, "{down:?}");
        assert_eq!(pass.pre_update_scroll, arrival.pre_hook_scroll, "{down:?}");
        assert_eq!(pass.post_update_scroll, arrival.pre_hook_scroll, "{down:?}");
        assert_eq!(
            pass.consumed, down.captured,
            "pass {}: {down:?}",
            pass.number
        );
    }
    assert_eq!(app.navigation_epoch, before);
    assert_eq!(app.active().unwrap().cursor, (97, 1));
    if raw_motion == RawMotionAt::Held {
        let held = observe_pointer_frame(
            &mut app,
            vec![egui::Event::MouseMoved(egui::vec2(1.0, -1.0))],
            raw_motion,
            PointerEdge::Held,
            target_id,
            pos,
        );
        eprintln!("{held:?}");
        assert_pointer_owners(&held, target_id, pos, true, false, false);
        assert_eq!(held.captured.pressed, ticket);
        assert_eq!(held.captured.click, None);
        for pass in &held.passes {
            assert_eq!(
                pass.consumed, held.captured,
                "pass {}: {held:?}",
                pass.number
            );
        }
        assert_eq!(app.navigation_epoch, before);
        assert_eq!(app.active().unwrap().cursor, (97, 1));
    }
    // Recompute from this presentation, rather than trusting pre-press geometry.
    let release_pos = clipped_center(&app, &format!("search_row:{target}"));
    assert_eq!(release_pos, pos);
    let mut release = pointer(release_pos, false);
    if raw_motion == RawMotionAt::Release {
        release.insert(1, egui::Event::MouseMoved(egui::vec2(1.0, -1.0)));
    }
    let up = observe_pointer_frame(
        &mut app,
        release,
        raw_motion,
        PointerEdge::Release,
        target_id,
        release_pos,
    );
    eprintln!("{up:?}");
    assert_pointer_owners(&up, target_id, release_pos, false, true, true);
    assert_eq!(up.captured.pressed, None);
    assert_eq!(up.captured.click, ticket);
    for pass in &up.passes {
        assert_eq!(pass.consumed.pressed, None, "pass {}: {up:?}", pass.number);
        assert_eq!(
            pass.consumed.click, None,
            "click ticket must be consumed in pass {}: {up:?}",
            pass.number
        );
    }
    assert_eq!(app.search_state.selected_index(), Some(target));
    assert_eq!(focused(&app), Some(target_id));
    assert_eq!(app.navigation_epoch, before);
    assert_eq!(app.active_document, active_before);
    assert!(editor_documents(&app) == documents_before, "{up:?}");
    assert!(pointer_editor_history(&app) == history_before, "{up:?}");
    assert_eq!(app.active().unwrap().cursor, (97, 1));
    assert_eq!(app.location_history.back.len(), 1);
    assert!(app.location_history.forward.is_empty());
    assert!(app.location_history.pending.is_none());
    assert_eq!(app.next_request, next_request_before);
    assert!(app.pending.is_empty());
    assert!(!app.has_search_preview());
    assert!(commands.try_recv().is_err());

    // The witnessed single click only selects. A separate native Enter owns
    // the ordinary Open and its existing editor/history transition below.
    frame(&mut app, key(egui::Key::Enter));
    assert_eq!(app.navigation_epoch, before + 1);
    assert_eq!(app.active().unwrap().cursor, (96, 1));
    assert_eq!(app.location_history.back.len(), 2);
    assert!(app.location_history.forward.is_empty());
    assert!(app.location_history.pending.is_none());
    let retained_departure = app.location_history.back[0];
    assert_eq!(retained_departure.document, original_departure.document);
    assert_eq!(retained_departure.generation, original_departure.generation);
    assert_eq!(
        retained_departure.edit_version,
        original_departure.edit_version
    );
    assert!(location_history::same_selection(
        retained_departure.selection,
        original_departure.selection
    ));
    let departure = app.location_history.back[1];
    assert_eq!(departure.document, 2);
    assert_eq!(departure.generation, app.generation);
    assert_eq!(departure.edit_version, edit_version);
    assert!(location_history::same_selection(
        departure.selection,
        selection
    ));
    assert_eq!(app.documents.len(), 2);
    assert_eq!(app.documents[0].text, dirty);
    assert_eq!(app.documents[0].saved_text, saved);
    assert!(app.documents[0].dirty());
    assert_eq!(app.active().unwrap().text, scrolled_text(SCROLLED_UNOPENED));
    assert_eq!(app.active().unwrap().path, SCROLLED_UNOPENED);
    assert!(
        commands.try_recv().is_err(),
        "the already-open target must issue no second Read or other operation"
    );
}

#[test]
fn search_keyboard_scrolled_open_completion_then_pointer_has_both_owners() {
    assert_scrolled_pointer_companion(RawMotionAt::None);
}

#[test]
fn search_keyboard_scrolled_mouse_moved_on_press_preserves_both_owners() {
    // Controlled companion admission, not evidence that the failed native
    // click contained MouseMoved or that this correction fixes that failure.
    // Pinned source: emilk/egui tag 0.31.1, crates/eframe/src/native/
    // glow_integration.rs:433-447 forwards DeviceEvent::MouseMotion;
    // crates/egui-winit/src/lib.rs:529-530 appends MouseMoved independently of
    // CursorMoved (591-621). These cached files match the Cargo.lock .crates.
    // These variants deliberately choose frame grouping; native grouping is
    // absent from the failed receipt and is not reconstructed by this fixture.
    assert_scrolled_pointer_companion(RawMotionAt::Press);
}

#[test]
fn search_keyboard_scrolled_mouse_moved_while_held_preserves_both_owners() {
    assert_scrolled_pointer_companion(RawMotionAt::Held);
}

#[test]
fn search_keyboard_scrolled_mouse_moved_on_release_preserves_both_owners() {
    assert_scrolled_pointer_companion(RawMotionAt::Release);
}

const COMPANION_TARGET: usize = 2;

fn companion_app() -> (CedarApp, Receiver<Command>, egui::Pos2) {
    let (mut app, commands) = app(8);
    let pos = clipped_center(&app, "search_row:2");
    frame(&mut app, vec![egui::Event::PointerMoved(pos)]);
    assert_visible(&app, COMPANION_TARGET);
    (app, commands, pos)
}

fn companion_pointer(pos: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
    let mut events = pointer(pos, pressed);
    events.insert(1, egui::Event::MouseMoved(egui::vec2(1.0, -1.0)));
    events
}

fn companion_frame(
    app: &mut CedarApp,
    events: Vec<egui::Event>,
    edge: PointerEdge,
    pos: egui::Pos2,
) -> PointerFrame {
    let target_id = id(app, COMPANION_TARGET);
    let case = match edge {
        PointerEdge::Arrival => RawMotionAt::None,
        PointerEdge::Press => RawMotionAt::Press,
        PointerEdge::Held => RawMotionAt::Held,
        PointerEdge::Release => RawMotionAt::Release,
    };
    observe_pointer_frame(app, events, case, edge, target_id, pos)
}

fn assert_no_companion_ticket(frame: &PointerFrame) {
    assert_eq!(frame.captured.pressed, None, "{frame:?}");
    assert_eq!(frame.captured.click, None, "{frame:?}");
    for pass in &frame.passes {
        assert_eq!(pass.consumed, frame.captured, "{frame:?}");
        assert!(
            pass.rows.iter().all(|row| row.cedar_click.is_none()),
            "{frame:?}"
        );
    }
}

fn assert_companion_quiet(
    app: &CedarApp,
    commands: &Receiver<Command>,
    epoch: u64,
    next_request: u64,
) {
    assert_eq!(app.navigation_epoch, epoch);
    assert_eq!(app.next_request, next_request);
    assert_eq!(app.active_document, Some(1));
    assert_eq!(app.documents.len(), 1);
    assert_eq!(app.documents[0].text, "saved Ω😀\r\nsecond\n");
    assert_eq!(app.documents[0].saved_text, app.documents[0].text);
    assert!(app.pending.is_empty());
    assert!(app.location_history.back.is_empty());
    assert!(app.location_history.forward.is_empty());
    assert!(app.location_history.pending.is_none());
    assert_eq!(app.search_state.selected_index(), Some(0));
    assert!(!app.has_search_preview());
    assert!(commands.try_recv().is_err());
}

fn tab_to_first_search_row(app: &mut CedarApp) {
    click_control(app, "search_query");
    frame(app, key(egui::Key::Tab));
    assert_eq!(
        focused(app),
        Some(recorded_response(app, "search_submit").id)
    );
    tab_from_search_submit_to_first_row(app);
}

#[test]
fn search_keyboard_mouse_moved_cannot_manufacture_an_absolute_target_or_owned_press() {
    // The vector is non-authoritative metadata, including zero, very large and
    // non-finite values. These are ownership tests, not egui numeric guarantees.
    for delta in [
        egui::Vec2::ZERO,
        egui::vec2(1.0e30, -1.0e30),
        egui::vec2(f32::NAN, 0.0),
        egui::vec2(f32::INFINITY, f32::NEG_INFINITY),
    ] {
        for sequence in 0..4 {
            let (mut app, commands, pos) = if sequence == 0 {
                let (app, commands) = app(8);
                let pos = clipped_center(&app, "search_row:2");
                assert!(app
                    .editor_ctx
                    .input(|input| input.pointer.interact_pos().is_none()));
                (app, commands, pos)
            } else {
                companion_app()
            };
            let epoch = app.navigation_epoch;
            let next_request = app.next_request;
            let previous_pos = app.editor_ctx.input(|input| input.pointer.interact_pos());
            let mut events = vec![egui::Event::MouseMoved(delta)];
            match sequence {
                0 => {}
                1 => events.insert(0, egui::Event::PointerMoved(pos)),
                2 => events.extend(pointer(pos, false)),
                _ => {
                    for pressed in [true, false] {
                        events.push(egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Secondary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        });
                    }
                }
            }
            let observed = companion_frame(&mut app, events, PointerEdge::Held, pos);
            assert_no_companion_ticket(&observed);
            assert!(observed
                .passes
                .iter()
                .flat_map(|pass| &pass.rows)
                .all(|row| !row.clicked));
            assert_eq!(
                app.editor_ctx.input(|input| input.pointer.interact_pos()),
                previous_pos
            );
            assert_companion_quiet(&app, &commands, epoch, next_request);
        }
    }
}

#[test]
fn search_keyboard_mouse_moved_does_not_exempt_mixed_batches_from_pointer_vetoes() {
    for noise in [
        egui::Event::Text(String::new()),
        egui::Event::Text("text".into()),
        egui::Event::Paste(String::new()),
        egui::Event::Paste("paste".into()),
        egui::Event::Ime(egui::ImeEvent::Preedit(String::new())),
        egui::Event::Ime(egui::ImeEvent::Preedit("preedit".into())),
        egui::Event::Ime(egui::ImeEvent::Commit(String::new())),
        egui::Event::Ime(egui::ImeEvent::Commit("commit".into())),
        egui::Event::PointerGone,
        egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::Vec2::ZERO,
            modifiers: egui::Modifiers::NONE,
        },
    ] {
        for at in [RawMotionAt::Press, RawMotionAt::Held, RawMotionAt::Release] {
            let (mut app, commands, pos) = companion_app();
            let epoch = app.navigation_epoch;
            let next_request = app.next_request;
            let mut press = companion_pointer(pos, true);
            if at == RawMotionAt::Press {
                press.push(noise.clone());
            }
            let down = companion_frame(&mut app, press, PointerEdge::Press, pos);
            if at == RawMotionAt::Press {
                assert_no_companion_ticket(&down);
            } else {
                assert_eq!(
                    down.captured.pressed,
                    Some((COMPANION_TARGET, id(&app, COMPANION_TARGET))),
                    "{down:?}"
                );
                assert_eq!(down.captured.click, None);
            }
            if at == RawMotionAt::Held {
                let held = companion_frame(
                    &mut app,
                    vec![egui::Event::MouseMoved(egui::Vec2::ZERO), noise.clone()],
                    PointerEdge::Held,
                    pos,
                );
                assert_no_companion_ticket(&held);
            }
            let mut release = companion_pointer(pos, false);
            if at == RawMotionAt::Release {
                release.push(noise.clone());
            }
            let up = companion_frame(&mut app, release, PointerEdge::Release, pos);
            assert_no_companion_ticket(&up);
            assert_companion_quiet(&app, &commands, epoch, next_request);
        }
    }
}

#[test]
fn search_keyboard_mouse_moved_cannot_end_prior_composition_or_override_button_modifiers() {
    for veto in 0..4 {
        let (mut app, commands, pos) = companion_app();
        let epoch = app.navigation_epoch;
        let next_request = app.next_request;
        let mut press = companion_pointer(pos, true);
        if veto < 2 {
            frame(
                &mut app,
                vec![egui::Event::Ime(egui::ImeEvent::Preedit(
                    if veto == 0 { "" } else { "preedit" }.into(),
                ))],
            );
            press.push(egui::Event::Ime(egui::ImeEvent::Disabled));
        } else if let egui::Event::PointerButton {
            button, modifiers, ..
        } = press.last_mut().unwrap()
        {
            if veto == 2 {
                *modifiers = egui::Modifiers::SHIFT;
            } else {
                *button = egui::PointerButton::Secondary;
            }
        }
        let down = companion_frame(&mut app, press, PointerEdge::Press, pos);
        assert_no_companion_ticket(&down);
        let up = companion_frame(
            &mut app,
            companion_pointer(pos, false),
            PointerEdge::Release,
            pos,
        );
        assert_no_companion_ticket(&up);
        assert_companion_quiet(&app, &commands, epoch, next_request);
    }
}

#[test]
fn search_keyboard_mouse_moved_does_not_make_enter_or_space_a_clean_key_batch() {
    for key_to_press in [egui::Key::Enter, egui::Key::Space] {
        let (mut app, commands, _) = companion_app();
        tab_to_first_search_row(&mut app);
        let epoch = app.navigation_epoch;
        let next_request = app.next_request;
        let mut events = key(key_to_press);
        events.insert(1, egui::Event::MouseMoved(egui::vec2(1.0, -1.0)));
        if key_to_press == egui::Key::Space {
            events.insert(2, egui::Event::Text(" ".into()));
        }
        assert_eq!(clean_key(&events), None);
        let ctx = app.editor_ctx.clone();
        let mut input = raw(&app, events);
        eframe::App::raw_input_hook(&mut app, &ctx, &mut input);
        assert!(input
            .events
            .iter()
            .any(|event| matches!(event, egui::Event::MouseMoved(delta) if *delta == egui::vec2(1.0, -1.0))));
        render(&mut app, input);
        assert_eq!(focused(&app), Some(id(&app, 0)));
        assert_eq!(app.search_state.pointer_probe().pressed, None);
        assert_eq!(app.search_state.pointer_probe().click, None);
        assert_companion_quiet(&app, &commands, epoch, next_request);
    }
}

fn mixed_companion_event_kinds(events: &[egui::Event]) -> Vec<&'static str> {
    events
        .iter()
        .take(5)
        .map(|event| match event {
            egui::Event::PointerMoved(_) => "PointerMoved",
            egui::Event::MouseMoved(_) => "MouseMoved",
            egui::Event::PointerButton { pressed: true, .. } => "PointerButtonDown",
            egui::Event::PointerButton { pressed: false, .. } => "PointerButtonUp",
            egui::Event::Key { pressed: true, .. } => "KeyDown",
            egui::Event::Key { pressed: false, .. } => "KeyUp",
            _ => "Other",
        })
        .collect()
}

fn mixed_companion_focus_role(app: &CedarApp, focus: Option<egui::Id>) -> &'static str {
    let row = |index| {
        app.search_state
            .accepted
            .as_ref()
            .map(|batch| row_id(batch, index))
    };
    match focus {
        None => "none",
        Some(focus) if Some(focus) == row(0) => "first_search_row",
        Some(focus) if Some(focus) == row(COMPANION_TARGET) => "target_search_row",
        Some(_) => "other",
    }
}

fn mixed_companion_key_frame(
    app: &mut CedarApp,
    pointer_events: Vec<egui::Event>,
    key_to_press: egui::Key,
    at: RawMotionAt,
) {
    let mut original = pointer_events.clone();
    original.extend(key(key_to_press));
    // Declare the contract from the phase, never from observed focus. The real
    // row-2 press clears row-0 focus before a Held or Release mixed frame.
    let (expected_focus, expected_residual, expected_len) = match at {
        RawMotionAt::Press => (Some(id(app, 0)), pointer_events, 3),
        RawMotionAt::Held => (None, original.clone(), 3),
        RawMotionAt::Release => (None, original.clone(), 5),
        RawMotionAt::None => panic!("mixed key fixture requires a pointer phase"),
    };
    let pre_hook_focus = focused(app);
    let pre_hook_claimed = app.search_state.claims(key_to_press);
    let mut input = raw(app, original.clone());
    let ctx = app.editor_ctx.clone();
    eframe::App::raw_input_hook(app, &ctx, &mut input);
    // Diagnostics expose at most five event kinds per sequence, without their
    // payloads, text, paths or focus IDs. Counts still expose unexpected extras.
    let diagnostic = format!(
        "phase={at:?} key={key_to_press:?} expected_focus_role={} pre_hook_focus_role={} original_count={} original={:?} residual_count={} residual={:?}",
        mixed_companion_focus_role(app, expected_focus),
        mixed_companion_focus_role(app, pre_hook_focus),
        original.len(),
        mixed_companion_event_kinds(&original),
        input.events.len(),
        mixed_companion_event_kinds(&input.events),
    );
    assert!(pre_hook_focus == expected_focus, "{diagnostic}");
    assert!(!pre_hook_claimed, "initial key claim: {diagnostic}");
    assert_eq!(expected_residual.len(), expected_len, "{diagnostic}");
    assert_eq!(input.events.len(), expected_len, "{diagnostic}");
    // Only Press owns/removes both key edges. Held/Release retain the original
    // pair and pointer events in order; every phase must remain ticket-free.
    assert!(input.events == expected_residual, "{diagnostic}");
    assert!(!app.search_state.claims(key_to_press), "{diagnostic}");
    assert!(
        app.search_state.pointer_probe().pressed.is_none(),
        "{diagnostic}"
    );
    assert!(
        app.search_state.pointer_probe().click.is_none(),
        "{diagnostic}"
    );
    render(app, input);
    let post_render_focus_role = mixed_companion_focus_role(app, focused(app));
    assert!(
        !app.search_state.claims(key_to_press),
        "{diagnostic} post_render_focus_role={post_render_focus_role}"
    );
    assert!(
        !ctx.input(|input| input.keys_down.contains(&key_to_press)),
        "{diagnostic} post_render_focus_role={post_render_focus_role}"
    );
}

#[test]
fn search_keyboard_mouse_moved_with_pointer_and_key_remains_mixed_before_filtering() {
    for key_to_press in [egui::Key::Enter, egui::Key::Space] {
        for at in [RawMotionAt::Press, RawMotionAt::Held, RawMotionAt::Release] {
            let (mut app, commands, _) = companion_app();
            tab_to_first_search_row(&mut app);
            let pos = clipped_center(&app, "search_row:2");
            frame(&mut app, vec![egui::Event::PointerMoved(pos)]);
            let epoch = app.navigation_epoch;
            let next_request = app.next_request;
            let press = companion_pointer(pos, true);
            if at == RawMotionAt::Press {
                mixed_companion_key_frame(&mut app, press, key_to_press, at);
            } else {
                let down = companion_frame(&mut app, press, PointerEdge::Press, pos);
                assert_eq!(
                    down.captured.pressed,
                    Some((COMPANION_TARGET, id(&app, COMPANION_TARGET)))
                );
            }
            if at == RawMotionAt::Held {
                mixed_companion_key_frame(
                    &mut app,
                    vec![egui::Event::MouseMoved(egui::Vec2::ZERO)],
                    key_to_press,
                    at,
                );
            }
            let release = companion_pointer(pos, false);
            if at == RawMotionAt::Release {
                mixed_companion_key_frame(&mut app, release, key_to_press, at);
            } else {
                let up = companion_frame(&mut app, release, PointerEdge::Release, pos);
                assert_no_companion_ticket(&up);
            }
            assert_companion_quiet(&app, &commands, epoch, next_request);
        }
    }
}

#[test]
fn search_keyboard_mouse_moved_foreign_key_release_cannot_become_pointer_only_after_filtering() {
    let (mut app, commands, _) = companion_app();
    load_other_surfaces(&mut app, &commands);
    let explorer_id = explorer_tree::row_id("");
    let explorer_pos = app
        .editor_ctx
        .read_response(explorer_id)
        .unwrap()
        .rect
        .center();
    frame(&mut app, vec![egui::Event::PointerMoved(explorer_pos)]);
    frame(&mut app, pointer(explorer_pos, true));
    frame(&mut app, pointer(explorer_pos, false));
    assert_eq!(focused(&app), Some(explorer_id));
    frame(&mut app, vec![edge(egui::Key::ArrowDown, true, false)]);
    assert!(app.explorer.claims(egui::Key::ArrowDown));
    assert!(!app.search_state.claims(egui::Key::ArrowDown));
    let epoch = app.navigation_epoch;
    let next_request = app.next_request;
    let pos = clipped_center(&app, "search_row:2");
    frame(&mut app, vec![egui::Event::PointerMoved(pos)]);
    let down = companion_frame(
        &mut app,
        companion_pointer(pos, true),
        PointerEdge::Press,
        pos,
    );
    assert_no_companion_ticket(&down);
    let ctx = app.editor_ctx.clone();
    let retained = companion_pointer(pos, false);
    let mut original = retained.clone();
    original.insert(0, edge(egui::Key::ArrowDown, false, false));
    let mut input = raw(&app, original);
    eframe::App::raw_input_hook(&mut app, &ctx, &mut input);
    assert_original_pointer_events(&input.events, &retained);
    assert_eq!(app.search_state.pointer_probe().pressed, None);
    assert_eq!(app.search_state.pointer_probe().click, None);
    assert!(!app.explorer.claims(egui::Key::ArrowDown));
    render(&mut app, input);
    assert_companion_quiet(&app, &commands, epoch, next_request);
}

#[test]
fn search_keyboard_mouse_moved_cannot_reuse_a_press_after_view_change_or_outside_release() {
    for change in 0..6 {
        let (mut app, commands, pos) = companion_app();
        let epoch = app.navigation_epoch;
        let next_request = app.next_request;
        let down = companion_frame(
            &mut app,
            companion_pointer(pos, true),
            PointerEdge::Press,
            pos,
        );
        assert_eq!(
            down.captured.pressed,
            Some((COMPANION_TARGET, id(&app, COMPANION_TARGET)))
        );
        let mut release_pos = pos;
        match change {
            0 => app.search_query.push('x'),
            1 => app.search_request += 1,
            2 => app.generation += 1,
            3 => app.new_file = true,
            4 => app.tools_open = false,
            _ => release_pos = clipped_center(&app, "search_row:3"),
        }
        let up = companion_frame(
            &mut app,
            companion_pointer(release_pos, false),
            PointerEdge::Release,
            release_pos,
        );
        assert_no_companion_ticket(&up);
        assert_companion_quiet(&app, &commands, epoch, next_request);
    }
}

#[test]
fn search_keyboard_mouse_moved_click_still_requires_current_view_at_render_and_raw_hook() {
    for change in 0..5 {
        let (mut app, commands, pos) = companion_app();
        let epoch = app.navigation_epoch;
        let next_request = app.next_request;
        let down = companion_frame(
            &mut app,
            companion_pointer(pos, true),
            PointerEdge::Press,
            pos,
        );
        assert!(down.captured.pressed.is_some());
        let ctx = app.editor_ctx.clone();
        let events = companion_pointer(pos, false);
        let mut input = raw(&app, events.clone());
        if change < 4 {
            eframe::App::raw_input_hook(&mut app, &ctx, &mut input);
            assert_original_pointer_events(&input.events, &events);
            assert_eq!(
                app.search_state.pointer_probe().click,
                Some((COMPANION_TARGET, id(&app, COMPANION_TARGET)))
            );
            match change {
                0 => app.search_query.push('x'),
                1 => app.search_request += 1,
                2 => app.generation += 1,
                _ => app.new_file = true,
            }
        }
        render(&mut app, input);
        assert_eq!(app.search_state.pointer_probe().click, None);
        assert_companion_quiet(&app, &commands, epoch, next_request);
    }
}

#[test]
fn search_keyboard_workspace_changing_hello_preserves_held_lifetime_until_release() {
    for held in [egui::Key::Enter, egui::Key::Space] {
        let (mut app, commands) = app(8);
        focus(&mut app, "search_row:0");
        let initial_epoch = app.navigation_epoch;
        frame(&mut app, vec![edge(held, true, false)]);
        if held == egui::Key::Space {
            assert!(app.has_search_preview());
            assert_eq!(app.navigation_epoch, initial_epoch);
            assert!(app.location_history.back.is_empty());
        } else {
            assert_eq!(app.navigation_epoch, initial_epoch + 1);
        }
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
                file_discovery: None,
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
        assert!(!app.has_search_preview());
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
        let epoch = app.navigation_epoch;
        let active = app.active_document;
        let documents = app.documents.len();
        frame(&mut app, key(held));
        assert!(
            matches!(commands.try_recv().unwrap().op, Operation::Read { path } if path == "file-2.txt")
        );
        assert_eq!(app.search_state.selected_index(), Some(2));
        if held == egui::Key::Space {
            assert!(app.has_search_preview());
            assert_eq!(app.navigation_epoch, epoch);
            assert_eq!(app.active_document, active);
            assert_eq!(app.documents.len(), documents);
            assert!(app.location_history.pending.is_none());
        } else {
            assert_eq!(app.navigation_epoch, epoch + 1);
            assert!(!app.has_search_preview());
        }
        assert!(commands.try_recv().is_err());
    }
}
