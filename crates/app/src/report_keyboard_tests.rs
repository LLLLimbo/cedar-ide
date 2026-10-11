//! Full production raw-hook/update frames, including original-batch ownership.
use super::*;
use crate::report_keyboard::{back_id, clean_key, destination, row_id};
use crate::test_report_ui::{Filter, Load, Snapshot, TestReportPanel};
use crate::workspace_access_tests::{
    recorded_clip, recorded_rect, recorded_report_scroll_id, recorded_response,
};
use sha2::{Digest, Sha256};

const PATH: &str = "target/TEST-keyboard.xml";
const SIZE: egui::Vec2 = egui::vec2(1200.0, 900.0);

fn xml(count: usize) -> String {
    let cases = (0..count).map(|index| {
        let detail = if index % 2 == 1 { format!("<failure message='case {index}'>trace Ω {index}</failure>") } else { String::new() };
        format!("<testcase name='duplicate Ω😀' classname='Class{index}' time='0.1'>{detail}</testcase>")
    }).collect::<String>();
    format!("<testsuite name='Keyboard' tests='{count}' failures='{}' errors='0' skipped='0'>{cases}</testsuite>", count / 2)
}

fn snapshot(id: u64, count: usize) -> Snapshot {
    let xml = xml(count);
    Snapshot {
        source: Load {
            generation: 0,
            id,
            path: PATH.into(),
        },
        revision: format!("{:x}", Sha256::digest(xml.as_bytes())),
        report: test_reports::parse_report(&xml).unwrap(),
    }
}

fn app(count: usize) -> (CedarApp, Receiver<Command>) {
    assert!(!system_fonts::contains_cjk(&xml(count)));
    app_with_snapshot(Some(snapshot(1, count)))
}

fn app_with_snapshot(initial_snapshot: Option<Snapshot>) -> (CedarApp, Receiver<Command>) {
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
    app.tool = Tool::Tests;
    app.tools_open = true;
    app.documents.push(Document::new(
        1,
        "draft.txt".into(),
        "saved Ω😀\r\nsecond\n".into(),
        "a".repeat(64),
    ));
    app.active_document = Some(1);
    app.next_document = 2;
    app.test_report.path = PATH.into();
    app.test_report.snapshot = initial_snapshot;
    assert!(!app.documents[0].has_cjk);
    prepare_report_viewport(&app);
    let (worker, commands) = Worker::recording();
    app.worker = Some(worker);
    settle(&mut app);
    assert!(
        app.system_fonts.needs_probe(),
        "geometry fixture must not start optional font I/O"
    );
    (app, commands)
}

fn prepare_report_viewport(app: &CedarApp) {
    app.editor_ctx.data_mut(|data| {
        data.insert_persisted(
            egui::Id::new("tools"),
            egui::containers::panel::PanelState {
                rect: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(SIZE.x, 480.0)),
            },
        )
    });
}

fn raw(app: &CedarApp, events: Vec<egui::Event>) -> egui::RawInput {
    egui::RawInput {
        time: Some(app.editor_ctx.input(|input| input.time) + 1.0),
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
    frame_after(app, events, 1.0)
}

fn frame_after(app: &mut CedarApp, events: Vec<egui::Event>, seconds: f64) -> egui::FullOutput {
    assert!(seconds.is_finite() && seconds > 0.0);
    let ctx = app.editor_ctx.clone();
    let mut input = raw(app, events);
    input.time = Some(ctx.input(|input| input.time) + seconds);
    eframe::App::raw_input_hook(app, &ctx, &mut input);
    render(app, input)
}

fn settle(app: &mut CedarApp) {
    for _ in 0..3 {
        frame(app, vec![]);
    }
}

fn key_event(
    key: egui::Key,
    pressed: bool,
    repeat: bool,
    modifiers: egui::Modifiers,
) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: Some(key),
        pressed,
        repeat,
        modifiers,
    }
}

fn key(key: egui::Key) -> Vec<egui::Event> {
    vec![
        key_event(key, true, false, egui::Modifiers::NONE),
        key_event(key, false, false, egui::Modifiers::NONE),
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

fn case_id(app: &CedarApp, index: usize) -> egui::Id {
    row_id(&app.test_report.snapshot.as_ref().unwrap().source, index)
}

fn assert_visible(app: &CedarApp, name: &str) {
    let rect = recorded_rect(app, name);
    let clip = recorded_clip(app, name);
    assert!(
        clip.height() > rect.height(),
        "positive content viewport required: {clip:?}"
    );
    assert!(
        rect.top() >= clip.top() - 1.0 && rect.bottom() <= clip.bottom() + 1.0,
        "{name}: {rect:?} outside {clip:?}"
    );
}

fn pointer_events(pos: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
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

fn click_position(app: &CedarApp, name: &str) -> egui::Pos2 {
    let response = recorded_response(app, name);
    let rect = recorded_rect(app, name);
    let clip = recorded_clip(app, name);
    let visible = rect.intersect(clip);
    assert!(response.enabled(), "{name} must be enabled");
    assert!(rect.is_finite() && clip.is_finite() && visible.is_finite());
    assert!(
        visible.width() > 0.0 && visible.height() > 0.0,
        "{name} has no visible hit target: {:?} / {clip:?}",
        rect
    );
    let pos = visible.center();
    assert!(rect.contains(pos) && clip.contains(pos));
    pos
}

fn click(app: &mut CedarApp, name: &str) {
    let pos = click_position(app, name);
    let id = recorded_response(app, name).id;
    // Keep ordinary one-second settle frames. A pointer click alone needs a
    // short press/release interval below egui 0.31's 0.8-second click limit.
    frame_after(app, pointer_events(pos, true), 0.01);
    let pressed = recorded_response(app, name);
    assert_eq!(pressed.id, id);
    assert!(
        pressed.is_pointer_button_down_on(),
        "{name} did not receive the native press"
    );
    frame_after(app, pointer_events(pos, false), 0.01);
    let released = recorded_response(app, name);
    assert_eq!(released.id, id);
    assert!(
        released.clicked_by(egui::PointerButton::Primary),
        "{name} did not receive the native click"
    );
}

fn selection(app: &CedarApp) -> egui::text::CCursorRange {
    egui::TextEdit::load_state(&app.editor_ctx, egui::Id::new(("editor", 1u64)))
        .unwrap()
        .cursor
        .char_range()
        .unwrap()
}

fn history(app: &CedarApp) -> String {
    format!(
        "{:?}",
        egui::TextEdit::load_state(&app.editor_ctx, egui::Id::new(("editor", 1u64)))
            .unwrap()
            .undoer()
    )
}

#[test]
fn report_keyboard_4096_bounds_filtered_xml_order_duplicates_and_clamp() {
    // Short fields stay inside the parser's independent retained-byte bounds.
    let cases = (0..4096)
        .map(|index| {
            if index % 2 == 0 {
                "<testcase name='x' time='0'/>"
            } else {
                "<testcase name='x' time='0'><failure/></testcase>"
            }
        })
        .collect::<String>();
    let report = test_reports::parse_report(&format!("<testsuite name='x' tests='4096' failures='2048' errors='0' skipped='0'>{cases}</testsuite>")).unwrap();
    let mut panel = TestReportPanel::default();
    panel.path = PATH.into();
    panel.snapshot = Some(Snapshot {
        source: Load {
            generation: 9,
            id: 17,
            path: PATH.into(),
        },
        revision: "a".repeat(64),
        report,
    });
    assert_eq!(panel.visible_cases().len(), 4096);
    panel.filter = Filter::Failed;
    let visible = panel.visible_cases();
    assert_eq!(visible, (1..4096).step_by(2).collect::<Vec<_>>());
    assert_eq!(destination(&visible, 1, egui::Key::ArrowUp), Some(1));
    assert_eq!(
        destination(&visible, 4095, egui::Key::ArrowDown),
        Some(4095)
    );
    assert_eq!(destination(&visible, 301, egui::Key::Home), Some(1));
    assert_eq!(destination(&visible, 301, egui::Key::End), Some(4095));
    assert_eq!(destination(&visible, 301, egui::Key::ArrowDown), Some(303));
    assert_eq!(destination(&visible, 300, egui::Key::ArrowDown), None);
    assert_eq!(destination(&[], 0, egui::Key::End), None);
    let load = &panel.snapshot.as_ref().unwrap().source;
    assert_ne!(row_id(load, 1), row_id(load, 3));
    assert_ne!(row_id(load, 1), back_id(load, 1));
    assert_ne!(
        row_id(load, 1),
        row_id(
            &Load {
                id: 18,
                ..load.clone()
            },
            1
        )
    );
    assert_ne!(
        row_id(load, 1),
        row_id(
            &Load {
                generation: 10,
                ..load.clone()
            },
            1
        )
    );
    panel.name_filter = "missing".into();
    assert!(panel.visible_cases().is_empty());
}

#[test]
fn report_keyboard_space_companion_is_exact_and_mixed_ime_input_vetoes() {
    let mut space = key(egui::Key::Space);
    space.insert(1, egui::Event::Text(" ".into()));
    assert_eq!(clean_key(&space), Some(egui::Key::Space));
    for extra in [
        egui::Event::Text("x".into()),
        egui::Event::Text(" ".into()),
        egui::Event::Paste(" ".into()),
        egui::Event::Ime(egui::ImeEvent::Enabled),
        egui::Event::Ime(egui::ImeEvent::Disabled),
        egui::Event::Ime(egui::ImeEvent::Preedit(String::new())),
        egui::Event::Ime(egui::ImeEvent::Commit(String::new())),
        key_event(egui::Key::ArrowDown, true, false, egui::Modifiers::NONE),
    ] {
        let mut events = space.clone();
        events.push(extra);
        assert_eq!(clean_key(&events), None);
    }
    let mut enter = key(egui::Key::Enter);
    enter.push(egui::Event::Text(" ".into()));
    assert_eq!(clean_key(&enter), None);
    assert_eq!(clean_key(&[egui::Event::Text(" ".into())]), None);
}

#[test]
fn report_keyboard_empty_then_explicit_read_prepares_real_viewport_before_navigation() {
    let (mut app, commands) = app_with_snapshot(None);
    assert!(app.test_report.snapshot.is_none());
    assert!(commands.try_recv().is_err());
    // The empty panel has no report ScrollArea to fill the seeded height.
    // egui persists its actual content rect, so the initial 480 is not retained.
    let empty_rect =
        egui::containers::panel::PanelState::load(&app.editor_ctx, egui::Id::new("tools"))
            .unwrap()
            .rect;
    assert!(empty_rect.is_finite());
    assert!(empty_rect.height() >= 150.0 && empty_rect.height() < 480.0);

    click(&mut app, "test_report_load");
    let command = commands.try_recv().unwrap();
    assert!(matches!(&command.op, Operation::Read { path } if path == PATH));
    let load = match app.pending.get(&command.id) {
        Some(Job::TestReportRead(load)) => load.clone(),
        _ => panic!("rendered Load must own the queued report Read"),
    };
    assert_eq!(app.test_report.loading.as_ref(), Some(&load));
    assert_eq!(load.generation, app.generation);
    assert_eq!(load.path, PATH);
    assert!(commands.try_recv().is_err());
    let text = xml(192);
    let revision = format!("{:x}", Sha256::digest(text.as_bytes()));
    app.apply_event(Event {
        generation: app.generation,
        id: command.id,
        connected: true,
        result: Ok(Payload::File {
            path: PATH.into(),
            revision: revision.clone(),
            text,
        }),
    });
    assert!(!app.pending.contains_key(&command.id));
    assert!(app.test_report.loading.is_none());
    let snapshot = app.test_report.snapshot.as_ref().unwrap();
    assert_eq!(snapshot.source, load);
    assert_eq!(snapshot.revision, revision);
    assert_eq!(snapshot.report.cases.len(), 192);

    // Prepare the same ordinary panel size only after the accepted Read exists.
    // Focus and scroll offsets still come exclusively from production frames.
    prepare_report_viewport(&app);
    settle(&mut app);
    let row = recorded_rect(&app, "test_report_case:0");
    let clip = recorded_clip(&app, "test_report_case:0");
    assert!(row.is_finite() && clip.is_finite());
    assert!(row.width() > 0.0 && row.height() > 0.0 && clip.height() > row.height());
    assert!(row.intersect(clip).width() > 0.0);
    assert_visible(&app, "test_report_case:0");
    focus(&mut app, "test_report_case:0");
    for (key_value, index) in [(egui::Key::End, 191), (egui::Key::Home, 0)] {
        frame(&mut app, key(key_value));
        settle(&mut app);
        assert_eq!(focused(&app), Some(case_id(&app, index)));
        assert_visible(&app, &format!("test_report_case:{index}"));
    }
    assert!(app.test_report.selected.is_none());
    assert!(commands.try_recv().is_err());
}

#[test]
fn report_keyboard_native_tab_first_arrow_filtered_end_and_home_reveal_real_rows() {
    let (mut app, commands) = app(96);
    app.test_report.filter = Filter::Failed;
    app.test_report.filter_changed();
    settle(&mut app);
    focus(&mut app, "test_report_filter");
    // Overflowing egui ScrollAreas expose their native drag viewport before
    // the suite. Observe its actual response; do not skip or replace Tab focus.
    let scroll_area_id = recorded_report_scroll_id(&app).with("area");
    let scroll_area = app.editor_ctx.read_response(scroll_area_id).unwrap();
    assert!(scroll_area.sense.is_focusable() && scroll_area.sense.senses_drag());
    assert!(scroll_area.rect.is_finite() && scroll_area.rect.height() > 0.0);
    frame(&mut app, key(egui::Key::Tab));
    assert_eq!(focused(&app), Some(scroll_area.id));
    assert!(app
        .editor_ctx
        .read_response(scroll_area.id)
        .unwrap()
        .has_focus());
    frame(&mut app, key(egui::Key::Tab));
    assert_eq!(
        focused(&app),
        Some(recorded_response(&app, "test_report_suite").id)
    );
    frame(&mut app, key(egui::Key::Tab));
    assert_eq!(focused(&app), Some(case_id(&app, 1)));
    frame(&mut app, key(egui::Key::ArrowDown));
    assert_eq!(focused(&app), Some(case_id(&app, 3)));
    assert_eq!(app.test_report.selected, None);
    frame(&mut app, key(egui::Key::End));
    settle(&mut app);
    assert_eq!(focused(&app), Some(case_id(&app, 95)));
    assert_visible(&app, "test_report_case:95");
    frame(&mut app, key(egui::Key::ArrowDown));
    assert_eq!(focused(&app), Some(case_id(&app, 95)));
    frame(&mut app, key(egui::Key::Home));
    settle(&mut app);
    assert_eq!(focused(&app), Some(case_id(&app, 1)));
    assert_visible(&app, "test_report_case:1");
    frame(&mut app, key(egui::Key::ArrowUp));
    assert_eq!(focused(&app), Some(case_id(&app, 1)));
    assert!(commands.try_recv().is_err());
}

#[test]
fn report_keyboard_enter_space_details_heading_back_and_editor_undo_are_exact() {
    for activate in [egui::Key::Enter, egui::Key::Space] {
        let (mut app, commands) = app(96);
        app.test_report.snapshot.as_mut().unwrap().report.cases[95].details[0].text =
            "long trace Ω\n".repeat(500);
        frame(&mut app, vec![]);
        editor_state::commit(
            &app.editor_ctx,
            &mut app.documents[0],
            "draft Ω😀\r\nkept\n".into(),
            4,
        );
        let range = egui::text::CCursorRange {
            primary: egui::text::CCursor {
                index: 5,
                prefer_next_row: true,
            },
            secondary: egui::text::CCursor {
                index: 1,
                prefer_next_row: false,
            },
        };
        editor_state::move_selection(&app.editor_ctx, &mut app.documents[0], range);
        focus(&mut app, "test_report_case:1");
        frame(&mut app, key(egui::Key::End));
        let before = (
            app.documents[0].text.clone(),
            app.documents[0].saved_text.clone(),
            app.documents[0].revision.clone(),
            app.documents[0].edit_version,
            selection(&app),
            history(&app),
        );
        let mut events = key(activate);
        if activate == egui::Key::Space {
            events.insert(1, egui::Event::Text(" ".into()));
        }
        frame(&mut app, events);
        settle(&mut app);
        assert_eq!(app.test_report.selected, Some(95));
        assert_eq!(
            focused(&app),
            Some(recorded_response(&app, "test_report_back").id)
        );
        assert_visible(&app, "test_report_details");
        assert_visible(&app, "test_report_back");
        assert_eq!(
            app.test_report.snapshot.as_ref().unwrap().report.cases[95]
                .classname
                .as_deref(),
            Some("Class95")
        );
        frame(&mut app, key(egui::Key::Enter));
        settle(&mut app);
        assert_eq!(focused(&app), Some(case_id(&app, 95)));
        assert_visible(&app, "test_report_case:95");
        assert_eq!(app.documents[0].text, before.0);
        assert_eq!(app.documents[0].saved_text, before.1);
        assert_eq!(app.documents[0].revision, before.2);
        assert_eq!(app.documents[0].edit_version, before.3);
        assert_eq!(selection(&app), before.4);
        assert_eq!(
            selection(&app).primary.prefer_next_row,
            before.4.primary.prefer_next_row
        );
        assert_eq!(
            selection(&app).secondary.prefer_next_row,
            before.4.secondary.prefer_next_row
        );
        assert_eq!(history(&app), before.5);
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn report_keyboard_pointer_selection_and_finite_pointer_back_use_actual_responses() {
    let (mut app, commands) = app(12);
    click(&mut app, "test_report_case:1");
    assert_eq!(app.test_report.selected, Some(1));
    assert_eq!(focused(&app), Some(case_id(&app, 1)));
    frame(&mut app, key(egui::Key::Enter));
    settle(&mut app);
    click(&mut app, "test_report_back");
    frame(&mut app, vec![]);
    settle(&mut app);
    assert_eq!(focused(&app), Some(case_id(&app, 1)));
    assert_visible(&app, "test_report_case:1");
    frame(&mut app, key(egui::Key::Enter));
    settle(&mut app);
    click(&mut app, "test_report_back");
    // New input expires the only next-frame return. It cannot reappear later.
    frame(&mut app, vec![egui::Event::Text("x".into())]);
    settle(&mut app);
    assert_ne!(focused(&app), Some(case_id(&app, 1)));
    assert!(commands.try_recv().is_err());
}

#[test]
fn report_keyboard_back_reopens_fully_collapsed_suite_without_lingering_focus() {
    let (mut app, _) = app(96);
    focus(&mut app, "test_report_case:1");
    frame(&mut app, key(egui::Key::End));
    frame(&mut app, key(egui::Key::Enter));
    settle(&mut app);
    focus(&mut app, "test_report_suite");
    frame(&mut app, key(egui::Key::Enter));
    settle(&mut app);
    frame(&mut app, key(egui::Key::Tab));
    assert_eq!(
        focused(&app),
        Some(recorded_response(&app, "test_report_back").id)
    );
    frame(&mut app, key(egui::Key::Enter));
    settle(&mut app);
    assert_eq!(focused(&app), Some(case_id(&app, 95)));
    assert_visible(&app, "test_report_case:95");
    focus(&mut app, "test_report_filter");
    settle(&mut app);
    assert_eq!(
        focused(&app),
        Some(recorded_response(&app, "test_report_filter").id)
    );
}

#[test]
fn report_keyboard_stale_capture_cancels_filter_path_revision_refresh_tool_modal_and_focus() {
    for change in [
        "filter",
        "query",
        "path",
        "revision",
        "refresh",
        "disconnect",
        "generation",
        "tool",
        "hidden",
        "modal",
        "focus",
    ] {
        let (mut app, commands) = app(12);
        focus(&mut app, "test_report_case:1");
        let ctx = app.editor_ctx.clone();
        let mut input = raw(&app, key(egui::Key::Enter));
        eframe::App::raw_input_hook(&mut app, &ctx, &mut input);
        match change {
            "filter" => {
                app.test_report.filter = Filter::Passed;
                app.test_report.filter_changed();
            }
            "query" => {
                app.test_report.name_filter = "Class3".into();
                app.test_report.filter_changed();
            }
            "path" => {
                app.test_report.path = "other.xml".into();
                app.test_report.path_edited();
            }
            "revision" => app.test_report.snapshot.as_mut().unwrap().revision = "b".repeat(64),
            "refresh" => {
                app.load_test_report();
                let command = commands.try_recv().unwrap();
                let text = xml(12);
                app.apply_event(Event {
                    generation: app.generation,
                    id: command.id,
                    connected: true,
                    result: Ok(Payload::File {
                        path: PATH.into(),
                        revision: format!("{:x}", Sha256::digest(text.as_bytes())),
                        text,
                    }),
                });
            }
            "disconnect" => {
                app.state = ConnectionState::Disconnected;
                app.test_report.disconnected();
            }
            "generation" => app.generation += 1,
            "tool" => app.tool = Tool::Search,
            "hidden" => app.tools_open = false,
            "modal" => app.confirm = Some(Confirm::CloseWindow),
            "focus" => {
                ctx.memory_mut(|memory| memory.request_focus(egui::Id::new(("editor", 1u64))))
            }
            _ => unreachable!(),
        }
        render(&mut app, input);
        assert_eq!(app.test_report.selected, None, "{change}");
        if !matches!(change, "disconnect" | "generation" | "modal") {
            app.tool = Tool::Tests;
            app.tools_open = true;
            settle(&mut app);
            assert_eq!(app.test_report.selected, None, "resurrected {change}");
        }
        assert!(commands.try_recv().is_err(), "{change}");
    }
}

#[test]
fn report_keyboard_stale_pointer_press_cannot_select_replacement_or_refiltered_row() {
    for replacement in [false, true] {
        let (mut app, commands) = app(12);
        let pos = recorded_rect(&app, "test_report_case:1").center();
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
            ],
        );
        if replacement {
            app.test_report.snapshot = Some(snapshot(2, 12));
        } else {
            app.test_report.filter = Filter::Passed;
            app.test_report.filter_changed();
        }
        frame(
            &mut app,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert_eq!(app.test_report.selected, None);
        settle(&mut app);
        assert_eq!(app.test_report.selected, None);
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn report_keyboard_mixed_batches_empty_preedit_and_modifier_keys_do_not_activate() {
    let extras = vec![
        egui::Event::Text("x".into()),
        egui::Event::Paste(" ".into()),
        egui::Event::Ime(egui::ImeEvent::Enabled),
        egui::Event::Ime(egui::ImeEvent::Disabled),
        egui::Event::Ime(egui::ImeEvent::Preedit(String::new())),
        egui::Event::Ime(egui::ImeEvent::Commit(String::new())),
        key_event(egui::Key::Home, true, false, egui::Modifiers::NONE),
    ];
    for extra in extras {
        let (mut app, _) = app(12);
        focus(&mut app, "test_report_case:1");
        let mut events = key(egui::Key::Enter);
        events.push(extra);
        frame(&mut app, events);
        settle(&mut app);
        assert_eq!(app.test_report.selected, None);
    }
    let (mut app, _) = app(12);
    focus(&mut app, "test_report_case:1");
    frame(
        &mut app,
        vec![egui::Event::Ime(egui::ImeEvent::Preedit("候".into()))],
    );
    frame(
        &mut app,
        vec![egui::Event::Ime(egui::ImeEvent::Preedit(String::new()))],
    );
    frame(&mut app, key(egui::Key::ArrowDown));
    assert_eq!(focused(&app), Some(case_id(&app, 1)));
    frame(&mut app, key(egui::Key::Enter));
    assert_eq!(app.test_report.selected, None);
    frame(&mut app, vec![egui::Event::Ime(egui::ImeEvent::Disabled)]);
    frame(&mut app, key(egui::Key::Enter));
    assert_eq!(app.test_report.selected, Some(1));
}

#[test]
fn report_keyboard_held_enter_and_space_never_activate_back_or_leak_to_editor() {
    for held in [egui::Key::Enter, egui::Key::Space] {
        let (mut app, _) = app(12);
        focus(&mut app, "test_report_case:1");
        let mut press = vec![key_event(held, true, false, egui::Modifiers::NONE)];
        if held == egui::Key::Space {
            press.push(egui::Event::Text(" ".into()));
        }
        frame(&mut app, press);
        let back = recorded_response(&app, "test_report_back").id;
        assert_eq!(focused(&app), Some(back));
        for repeat in [false, true] {
            let mut events = vec![key_event(held, true, repeat, egui::Modifiers::NONE)];
            if held == egui::Key::Space {
                events.push(egui::Event::Text(" ".into()));
            }
            frame(&mut app, events);
            assert_eq!(focused(&app), Some(back));
        }
        focus(&mut app, "editor");
        let before = app.documents[0].text.clone();
        let mut events = vec![key_event(held, true, true, egui::Modifiers::NONE)];
        if held == egui::Key::Space {
            events.push(egui::Event::Text(" ".into()));
        }
        frame(&mut app, events);
        assert_eq!(app.documents[0].text, before);
        frame(
            &mut app,
            vec![key_event(held, false, false, egui::Modifiers::NONE)],
        );
        if held == egui::Key::Space {
            let mut events = key(egui::Key::Space);
            events.insert(1, egui::Event::Text(" ".into()));
            frame(&mut app, events);
            assert_ne!(app.documents[0].text, before);
        }
    }
}

#[test]
fn report_keyboard_claimed_arrow_cannot_promote_mixed_editor_duplicate_shortcut() {
    let (mut app, _) = app(12);
    focus(&mut app, "test_report_case:1");
    frame(
        &mut app,
        vec![key_event(
            egui::Key::ArrowDown,
            true,
            false,
            egui::Modifiers::NONE,
        )],
    );
    focus(&mut app, "editor");
    let before = (
        app.documents[0].text.clone(),
        selection(&app),
        history(&app),
    );
    let mac = app.editor_ctx.os() == egui::os::OperatingSystem::Mac;
    let modifiers = egui::Modifiers {
        ctrl: !mac,
        mac_cmd: mac,
        command: true,
        ..egui::Modifiers::NONE
    };
    frame(
        &mut app,
        vec![
            key_event(egui::Key::ArrowDown, true, true, egui::Modifiers::NONE),
            key_event(egui::Key::D, true, false, modifiers),
            key_event(egui::Key::D, false, false, modifiers),
        ],
    );
    assert_eq!(app.documents[0].text, before.0);
    assert_eq!(selection(&app), before.1);
    assert_eq!(history(&app), before.2);
}

#[test]
fn report_keyboard_sizing_pass_reveals_once_per_pass_without_replaying_activation() {
    let (mut app, _) = app(96);
    focus(&mut app, "test_report_case:1");
    let ctx = app.editor_ctx.clone();
    let mut input = raw(&app, key(egui::Key::Enter));
    eframe::App::raw_input_hook(&mut app, &ctx, &mut input);
    let mut native = eframe::Frame::_new_kittest();
    let mut passes = 0;
    let _ = ctx.run(input, |ctx| {
        eframe::App::update(&mut app, ctx, &mut native);
        passes += 1;
        if passes == 1 {
            ctx.request_discard("exercise report reveal sizing lifecycle");
        }
    });
    assert!(passes >= 2);
    assert_eq!(app.test_report.selected, Some(1));
    assert_eq!(
        focused(&app),
        Some(recorded_response(&app, "test_report_back").id)
    );
    settle(&mut app);
    assert_visible(&app, "test_report_details");
    focus(&mut app, "test_report_filter");
    settle(&mut app);
    assert_eq!(
        focused(&app),
        Some(recorded_response(&app, "test_report_filter").id)
    );
}

fn load_explorer(app: &mut CedarApp, commands: &Receiver<Command>) {
    app.explorer_set_mode(explorer_tree::Mode::Tree);
    app.explorer_expand("");
    let command = commands.try_recv().unwrap();
    assert!(matches!(command.op, Operation::List { .. }));
    app.apply_event(Event {
        generation: app.generation,
        id: command.id,
        connected: true,
        result: Ok(Payload::Entries {
            entries: vec![Entry {
                name: "unopened.txt".into(),
                path: "unopened.txt".into(),
                is_dir: false,
            }],
        }),
    });
    settle(app);
}

fn focus_explorer(app: &mut CedarApp) {
    let response = app
        .editor_ctx
        .read_response(explorer_tree::row_id("unopened.txt"))
        .unwrap();
    response.request_focus();
    frame(app, vec![]);
    assert_eq!(focused(app), Some(explorer_tree::row_id("unopened.txt")));
}

#[test]
fn report_keyboard_held_arrow_cannot_promote_explorer_enter_into_an_extra_read() {
    let (mut app, commands) = app(12);
    load_explorer(&mut app, &commands);
    focus(&mut app, "test_report_case:1");
    frame(
        &mut app,
        vec![key_event(
            egui::Key::ArrowDown,
            true,
            false,
            egui::Modifiers::NONE,
        )],
    );
    focus_explorer(&mut app);
    frame(
        &mut app,
        vec![
            key_event(egui::Key::ArrowDown, true, true, egui::Modifiers::NONE),
            key_event(egui::Key::Enter, true, false, egui::Modifiers::NONE),
            key_event(egui::Key::Enter, false, false, egui::Modifiers::NONE),
        ],
    );
    assert!(
        commands.try_recv().is_err(),
        "mixed original batch must not Open an Explorer file"
    );
    assert_eq!(app.documents.len(), 1);
    frame(
        &mut app,
        vec![key_event(
            egui::Key::ArrowDown,
            false,
            false,
            egui::Modifiers::NONE,
        )],
    );
    frame(&mut app, key(egui::Key::Enter));
    assert!(
        matches!(commands.try_recv().unwrap().op, Operation::Read { path } if path == "unopened.txt")
    );
}

#[test]
fn report_keyboard_explorer_held_arrow_cannot_promote_report_enter_or_space() {
    for activate in [egui::Key::Enter, egui::Key::Space] {
        let (mut app, commands) = app(12);
        load_explorer(&mut app, &commands);
        focus_explorer(&mut app);
        frame(
            &mut app,
            vec![key_event(
                egui::Key::ArrowDown,
                true,
                false,
                egui::Modifiers::NONE,
            )],
        );
        focus(&mut app, "test_report_case:1");
        let mut events = vec![
            key_event(egui::Key::ArrowDown, true, true, egui::Modifiers::NONE),
            key_event(activate, true, false, egui::Modifiers::NONE),
        ];
        if activate == egui::Key::Space {
            events.push(egui::Event::Text(" ".into()));
        }
        events.push(key_event(activate, false, false, egui::Modifiers::NONE));
        frame(&mut app, events);
        assert_eq!(app.test_report.selected, None);
        assert!(commands.try_recv().is_err());
        frame(
            &mut app,
            vec![key_event(
                egui::Key::ArrowDown,
                false,
                false,
                egui::Modifiers::NONE,
            )],
        );
        frame(&mut app, key(egui::Key::Enter));
        assert_eq!(app.test_report.selected, Some(1));
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn report_keyboard_native_filter_path_load_and_refresh_keep_explicit_read_contract() {
    let (mut app, commands) = app(12);
    focus(&mut app, "test_report_filter");
    frame(&mut app, vec![egui::Event::Text("Class3".into())]);
    assert_eq!(app.test_report.visible_cases(), vec![3]);
    assert!(commands.try_recv().is_err());
    focus(&mut app, "test_report_path");
    frame(&mut app, key(egui::Key::End));
    frame(&mut app, vec![egui::Event::Text(".new".into())]);
    assert_eq!(app.test_report.path, format!("{PATH}.new"));
    assert!(app.test_report.snapshot.is_none());
    frame(&mut app, key(egui::Key::Enter));
    assert!(commands.try_recv().is_err());
    click(&mut app, "test_report_load");
    let load = commands.try_recv().unwrap();
    assert!(matches!(&load.op, Operation::Read { path } if path == &format!("{PATH}.new")));
    let text = xml(12);
    app.apply_event(Event {
        generation: app.generation,
        id: load.id,
        connected: true,
        result: Ok(Payload::File {
            path: format!("{PATH}.new"),
            revision: format!("{:x}", Sha256::digest(text.as_bytes())),
            text,
        }),
    });
    settle(&mut app);
    assert!(app.test_report.snapshot.is_some());
    assert!(commands.try_recv().is_err());
    click(&mut app, "test_report_refresh");
    assert!(
        matches!(&commands.try_recv().unwrap().op, Operation::Read { path } if path == &format!("{PATH}.new"))
    );
    assert!(app.test_report.snapshot.is_none());
    assert!(commands.try_recv().is_err());
}

#[test]
fn report_keyboard_cross_surface_lifetimes_retire_release_in_original_order() {
    let (mut app, commands) = app(12);
    load_explorer(&mut app, &commands);
    let tree_root = explorer_tree::row_id("");
    app.editor_ctx
        .read_response(tree_root)
        .unwrap()
        .request_focus();
    frame(&mut app, vec![]);
    frame(
        &mut app,
        vec![key_event(
            egui::Key::ArrowDown,
            true,
            false,
            egui::Modifiers::NONE,
        )],
    );
    assert_eq!(focused(&app), Some(explorer_tree::row_id("unopened.txt")));
    focus(&mut app, "test_report_case:1");
    frame(
        &mut app,
        vec![key_event(
            egui::Key::ArrowDown,
            true,
            true,
            egui::Modifiers::NONE,
        )],
    );
    assert_eq!(focused(&app), Some(case_id(&app, 1)));
    frame(
        &mut app,
        vec![key_event(
            egui::Key::ArrowDown,
            false,
            false,
            egui::Modifiers::NONE,
        )],
    );
    app.editor_ctx
        .read_response(tree_root)
        .unwrap()
        .request_focus();
    frame(&mut app, vec![]);
    frame(&mut app, key(egui::Key::ArrowDown));
    assert_eq!(
        focused(&app),
        Some(explorer_tree::row_id("unopened.txt")),
        "Explorer's original held key must retire"
    );
    // An editor-held, unclaimed key is likewise not acquired after transfer.
    focus(&mut app, "editor");
    frame(
        &mut app,
        vec![key_event(
            egui::Key::End,
            true,
            false,
            egui::Modifiers::NONE,
        )],
    );
    focus(&mut app, "test_report_case:1");
    frame(
        &mut app,
        vec![key_event(egui::Key::End, true, true, egui::Modifiers::NONE)],
    );
    assert_eq!(focused(&app), Some(case_id(&app, 1)));
    // Release then repress in the same batch remains a mixed report intent;
    // its final release retires bookkeeping in order, allowing the next press.
    frame(
        &mut app,
        vec![
            key_event(egui::Key::End, false, false, egui::Modifiers::NONE),
            key_event(egui::Key::End, true, false, egui::Modifiers::NONE),
            key_event(egui::Key::End, false, false, egui::Modifiers::NONE),
        ],
    );
    assert_eq!(focused(&app), Some(case_id(&app, 1)));
    frame(&mut app, key(egui::Key::End));
    assert_eq!(focused(&app), Some(case_id(&app, 11)));
    assert!(commands.try_recv().is_err());
}

#[test]
fn report_keyboard_claimed_space_survives_clear_until_release_without_resurrection() {
    let (mut app, commands) = app(12);
    focus(&mut app, "test_report_case:1");
    frame(
        &mut app,
        vec![
            key_event(egui::Key::Space, true, false, egui::Modifiers::NONE),
            egui::Event::Text(" ".into()),
        ],
    );
    app.test_report.clear();
    frame(&mut app, vec![]);
    focus(&mut app, "editor");
    let before = app.documents[0].text.clone();
    frame(
        &mut app,
        vec![
            key_event(egui::Key::Space, true, true, egui::Modifiers::NONE),
            egui::Event::Text(" ".into()),
        ],
    );
    assert_eq!(app.documents[0].text, before);
    frame(
        &mut app,
        vec![key_event(
            egui::Key::Space,
            false,
            false,
            egui::Modifiers::NONE,
        )],
    );
    frame(&mut app, vec![egui::Event::Text(" ".into())]);
    assert_ne!(app.documents[0].text, before);
    app.test_report.snapshot = Some(snapshot(2, 12));
    settle(&mut app);
    assert!(app.test_report.selected.is_none());
    assert_eq!(focused(&app), Some(egui::Id::new(("editor", 1u64))));
    assert!(commands.try_recv().is_err());
}

#[test]
fn report_keyboard_pointer_duration_obeys_native_click_threshold() {
    let (mut app, commands) = app(12);
    let name = "test_report_case:1";
    let max_click_duration = app
        .editor_ctx
        .options(|options| options.input_options.max_click_duration);
    assert_eq!(max_click_duration, 0.8, "pinned egui 0.31 click timing");
    let pos = click_position(&app, name);
    frame_after(&mut app, pointer_events(pos, true), 0.01);
    assert!(recorded_response(&app, name).is_pointer_button_down_on());
    frame_after(
        &mut app,
        pointer_events(pos, false),
        max_click_duration + 0.01,
    );
    assert!(!recorded_response(&app, name).clicked_by(egui::PointerButton::Primary));
    assert_eq!(app.test_report.selected, None);
    assert!(commands.try_recv().is_err());
    click(&mut app, name);
    assert_eq!(app.test_report.selected, Some(1));
    assert_eq!(focused(&app), Some(case_id(&app, 1)));
    assert!(commands.try_recv().is_err());
}
