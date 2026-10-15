//! Production-frame keyboard access tests; probes retain rendered responses,
//! never reconstructed auto IDs or a second focus/navigation implementation.
use super::*;

const PROBE: &str = "workspace_access_test_probe";
const REPORT_SCROLL: &str = "workspace_access_report_scroll";
const SIZE: [f32; 2] = [780.0, 540.0];
const TOOLS: [(Tool, &str); 5] = [
    (Tool::Search, "FIND RESULTS"),
    (Tool::Git, "GIT CHANGES"),
    (Tool::Run, "COMMANDS"),
    (Tool::Language, "LANGUAGE"),
    (Tool::Tests, "TEST RESULTS"),
];

#[derive(Clone)]
struct Widget {
    id: egui::Id,
    rect: egui::Rect,
    clip: egui::Rect,
    observation: serde_json::Value,
}

pub(super) fn begin(ctx: &egui::Context) {
    ctx.data_mut(|data| data.insert_temp(egui::Id::new(PROBE), HashMap::<String, Widget>::new()));
}

pub(super) fn record(ui: &egui::Ui, name: &str, response: &egui::Response) {
    let widget = Widget {
        id: response.id,
        rect: response.rect,
        clip: ui.clip_rect(),
        // Copy plain values while the real response is current. Do not retain
        // Response/Context or call Context recursively inside its data lock.
        observation: serde_json::json!({
            "id": format!("{:?}", response.id),
            "changed": response.changed(),
            "has_focus": response.has_focus(),
            "gained_focus": response.gained_focus(),
            "lost_focus": response.lost_focus(),
            "sizing_pass": ui.is_sizing_pass(),
            "cursor": egui::TextEdit::load_state(ui.ctx(), response.id)
                .and_then(|state| state.cursor.char_range())
                .map(|range| format!("{range:?}")),
        }),
    };
    ui.ctx().data_mut(|data| {
        data.get_temp_mut_or_default::<HashMap<String, Widget>>(egui::Id::new(PROBE))
            .insert(name.into(), widget);
    });
}

pub(super) fn record_report_scroll(ctx: &egui::Context, id: egui::Id, offset: egui::Vec2) {
    ctx.data_mut(|data| data.insert_temp(egui::Id::new(REPORT_SCROLL), (id, offset)));
}

// Return the ScrollArea identity captured from the production render. Native
// child interactions can then be inspected through Context::read_response.
pub(super) fn recorded_report_scroll_id(app: &CedarApp) -> egui::Id {
    app.editor_ctx.data(|data| {
        data.get_temp::<(egui::Id, egui::Vec2)>(egui::Id::new(REPORT_SCROLL))
            .expect("report ScrollArea was not rendered")
            .0
    })
}

fn widget(app: &CedarApp, name: &str) -> Widget {
    app.editor_ctx.data(|data| {
        data.get_temp::<HashMap<String, Widget>>(egui::Id::new(PROBE))
            .unwrap()
            .get(name)
            .unwrap_or_else(|| panic!("missing {name}"))
            .clone()
    })
}

pub(super) fn recorded_observation(app: &CedarApp, name: &str) -> serde_json::Value {
    widget(app, name).observation
}

pub(super) fn recorded_response(app: &CedarApp, name: &str) -> egui::Response {
    app.editor_ctx.read_response(widget(app, name).id).unwrap()
}

pub(super) fn recorded_rect(app: &CedarApp, name: &str) -> egui::Rect {
    widget(app, name).rect
}

pub(super) fn recorded_clip(app: &CedarApp, name: &str) -> egui::Rect {
    widget(app, name).clip
}

fn focused(app: &CedarApp) -> Option<egui::Id> {
    app.editor_ctx.memory(|memory| memory.focused())
}

fn frame(app: &mut CedarApp, size: [f32; 2], events: Vec<egui::Event>) {
    let _ = frame_repaint(app, size, events);
}

fn frame_repaint(
    app: &mut CedarApp,
    size: [f32; 2],
    events: Vec<egui::Event>,
) -> std::time::Duration {
    let ctx = app.editor_ctx.clone();
    let mut native = eframe::Frame::_new_kittest();
    let mut input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size.into())),
        events,
        ..Default::default()
    };
    eframe::App::raw_input_hook(app, &ctx, &mut input);
    let output = ctx.run(input, |ctx| eframe::App::update(app, ctx, &mut native));
    output.viewport_output[&egui::ViewportId::ROOT].repaint_delay
}

fn settle(app: &mut CedarApp, size: [f32; 2]) {
    for _ in 0..4 {
        frame(app, size, vec![]);
    }
}

fn key_event(key: egui::Key, modifiers: egui::Modifiers, pressed: bool) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: Some(key),
        pressed,
        repeat: false,
        modifiers,
    }
}

fn key(key: egui::Key, modifiers: egui::Modifiers) -> Vec<egui::Event> {
    vec![
        key_event(key, modifiers, true),
        key_event(key, modifiers, false),
    ]
}

fn toggle(app: &mut CedarApp, size: [f32; 2]) {
    frame(app, size, key(egui::Key::J, egui::Modifiers::COMMAND));
}

fn explorer(app: &mut CedarApp, size: [f32; 2]) {
    frame(
        app,
        size,
        key(
            egui::Key::E,
            egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
        ),
    );
}

fn focus(app: &mut CedarApp, id: egui::Id) {
    app.editor_ctx.memory_mut(|memory| memory.request_focus(id));
    frame(app, SIZE, vec![]);
    assert_eq!(focused(app), Some(id));
}

fn app(tool: Tool) -> (CedarApp, Receiver<Command>) {
    let mut app = CedarApp::empty();
    // Match production startup while retaining the real asynchronous fallback.
    app.editor_ctx.set_fonts(crate::system_fonts::base_fonts());
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
    app.tool = tool;
    app.tools_open = true;
    app.documents.push(Document::new(
        1,
        "draft.txt".into(),
        "saved 雪".into(),
        "a".repeat(64),
    ));
    app.active_document = Some(1);
    app.next_document = 2;
    app.editor_ctx.style_mut(|style| {
        style.spacing.item_spacing = egui::vec2(9.0, 8.0);
        style.spacing.button_padding = egui::vec2(10.0, 6.0);
        style
            .text_styles
            .insert(egui::TextStyle::Body, FontId::proportional(14.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, FontId::proportional(13.0));
        style
            .text_styles
            .insert(egui::TextStyle::Monospace, FontId::monospace(14.0));
    });
    app.search_query = "retained query".into();
    app.search_results.push(SearchMatch {
        path: "draft.txt".into(),
        line: 1,
        text: "saved 雪".into(),
    });
    app.profiles.draft.program = "existing-command".into();
    app.profiles.draft.args = vec!["existing-argument".into()];
    app.run_state.output = "retained run output".into();
    app.test_report.path = "TEST-retained.xml".into();
    let cases = (0..32)
        .map(|index| format!("<testcase name=\"case-{index}\" time=\"0.1\"/>"))
        .collect::<String>();
    app.test_report.snapshot = Some(test_report_ui::Snapshot {
        source: test_report_ui::Load { generation: 0, id: 1, path: app.test_report.path.clone() },
        revision: "retained report revision".into(),
        report: test_reports::parse_report(&format!("<testsuite name=\"Access\" tests=\"32\" failures=\"0\" errors=\"0\" skipped=\"0\">{cases}</testsuite>")).unwrap(),
    });
    app.test_report.filter = test_report_ui::Filter::Passed;
    app.test_report.name_filter = "case-".into();
    app.test_report.selected = Some(31);
    let (worker, commands) = Worker::recording();
    app.worker = Some(worker);
    (app, commands)
}

fn selection(app: &CedarApp) -> Option<egui::text::CCursorRange> {
    egui::TextEdit::load_state(&app.editor_ctx, egui::Id::new(("editor", 1u64)))
        .unwrap()
        .cursor
        .char_range()
}

#[test]
fn every_tool_hides_to_visible_editor_and_reopens_useful_rendered_content_without_work() {
    for (tool, header) in TOOLS {
        let (mut app, commands) = app(tool);
        settle(&mut app, SIZE);
        editor_state::commit(
            &app.editor_ctx,
            &mut app.documents[0],
            "precious draft 雪".into(),
            4,
        );
        let range =
            egui::text::CCursorRange::two(egui::text::CCursor::new(1), egui::text::CCursor::new(4));
        let editor = egui::Id::new(("editor", 1u64));
        let mut state = egui::TextEdit::load_state(&app.editor_ctx, editor).unwrap();
        state.cursor.set_char_range(Some(range));
        state.store(&app.editor_ctx, editor);
        let interrupted =
            interrupted_save::InterruptedSave::capture(&app, &app.documents[0]).unwrap();
        app.documents[0].interrupted_save = Some(interrupted.clone());
        let version = app.documents[0].edit_version;
        let session = app.language.session;
        let navigation = app.navigation_epoch;
        toggle(&mut app, SIZE);
        assert!(!app.tools_open);
        assert_eq!(focused(&app), Some(editor), "hide {header}");
        let rendered = widget(&app, "editor");
        assert!(rendered.rect.intersect(rendered.clip).height() > 40.0);
        assert_eq!(selection(&app), Some(range));
        toggle(&mut app, SIZE);
        assert!(app.tools_open && app.tool == tool);
        assert_eq!(
            focused(&app),
            Some(
                widget(
                    &app,
                    if tool == Tool::Search {
                        "search_query"
                    } else {
                        header
                    }
                )
                .id
            ),
            "show {header}"
        );
        assert_eq!(app.documents[0].text, "precious draft 雪");
        assert_eq!(app.documents[0].saved_text, "saved 雪");
        assert_eq!(app.documents[0].edit_version, version);
        assert_eq!(app.documents[0].interrupted_save, Some(interrupted));
        assert_eq!(selection(&app), Some(range));
        assert_eq!(app.search_query, "retained query");
        assert_eq!(app.search_results.len(), 1);
        assert_eq!(app.profiles.draft.program, "existing-command");
        assert_eq!(app.profiles.draft.args, ["existing-argument"]);
        assert_eq!(app.run_state.output, "retained run output");
        assert_eq!(app.test_report.path, "TEST-retained.xml");
        assert_eq!(app.test_report.filter, test_report_ui::Filter::Passed);
        assert_eq!(app.test_report.name_filter, "case-");
        assert_eq!(app.test_report.selected, Some(31));
        assert_eq!(
            app.test_report.snapshot.as_ref().unwrap().revision,
            "retained report revision"
        );
        assert_eq!(app.language.session, session);
        assert_eq!(app.navigation_epoch, navigation);
        assert!(!app.execution_trusted());
        toggle(&mut app, SIZE);
        frame(&mut app, SIZE, key(egui::Key::Z, egui::Modifiers::COMMAND));
        assert_eq!(app.documents[0].text, "saved 雪");
        frame(
            &mut app,
            SIZE,
            key(
                egui::Key::Z,
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
            ),
        );
        assert_eq!(app.documents[0].text, "precious draft 雪");
        assert!(
            commands.try_recv().is_err(),
            "access emitted work for {header}"
        );
    }
}

#[test]
fn close_pointer_from_focused_field_and_tall_language_returns_after_rendering() {
    for tool in [Tool::Search, Tool::Language, Tool::Tests] {
        let (mut app, commands) = app(tool);
        if tool == Tool::Language {
            app.editor_ctx.style_mut(|style| {
                style
                    .text_styles
                    .insert(egui::TextStyle::Button, FontId::proportional(18.0));
            });
            app.error = Some("An earlier workspace read failed.\nThe current draft is retained.\nReview the connection before retrying.".into());
        }
        settle(&mut app, SIZE);
        if tool == Tool::Language {
            let editor = widget(&app, "editor");
            assert!(
                editor.rect.intersect(editor.clip).height() <= 0.0,
                "fixture must begin with no visible editor: {:?} / {:?}",
                editor.rect,
                editor.clip
            );
        }
        if tool == Tool::Search {
            let input = widget(&app, "search_query").id;
            focus(&mut app, input);
        } else if tool == Tool::Tests {
            focus(&mut app, egui::Id::new("test_report_path"));
        }
        let at = widget(&app, "close_tools").rect.center();
        let mut events = vec![egui::Event::PointerMoved(at)];
        events.extend(
            [true, false]
                .into_iter()
                .map(|pressed| egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                }),
        );
        frame(&mut app, SIZE, events);
        assert!(!app.tools_open);
        assert_eq!(focused(&app), Some(egui::Id::new(("editor", 1u64))));
        frame(&mut app, SIZE, vec![egui::Event::Text("!".into())]);
        assert!(app.documents[0].text.contains('!'));
        assert_eq!(app.search_query, "retained query");
        let editor = widget(&app, "editor");
        assert!(editor.rect.intersect(editor.clip).height() > 40.0);
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn close_button_keyboard_activation_keeps_native_enter_and_space() {
    for keycode in [egui::Key::Enter, egui::Key::Space] {
        let (mut app, commands) = app(Tool::Search);
        settle(&mut app, SIZE);
        let close = widget(&app, "close_tools").id;
        focus(&mut app, close);
        frame(&mut app, SIZE, key(keycode, egui::Modifiers::NONE));
        assert!(!app.tools_open);
        assert_eq!(focused(&app), Some(egui::Id::new(("editor", 1u64))));
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn close_pointer_from_editor_or_find_allows_only_its_own_focus_surrender() {
    for owner in [
        egui::Id::new(("editor", 1u64)),
        egui::Id::new(replace::FIND_INPUT),
    ] {
        let (mut app, commands) = app(Tool::Search);
        app.find_open = true;
        settle(&mut app, SIZE);
        focus(&mut app, owner);
        let at = widget(&app, "close_tools").rect.center();
        let events = [true, false]
            .into_iter()
            .map(|pressed| egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            })
            .collect();
        frame(&mut app, SIZE, events);
        assert!(!app.tools_open);
        assert_eq!(focused(&app), Some(egui::Id::new(("editor", 1u64))));
        assert!(commands.try_recv().is_err());
    }
}

fn load_project(app: &mut CedarApp, commands: &Receiver<Command>, entries: Vec<Entry>) {
    app.list(String::new());
    let request = commands.try_recv().expect("explicit fixture List");
    assert!(matches!(request.op, Operation::List { .. }));
    app.apply_event(Event {
        generation: app.generation,
        id: request.id,
        connected: true,
        result: Ok(Payload::Entries { entries }),
    });
}

fn project_rows(count: usize) -> Vec<Entry> {
    (0..count)
        .map(|index| Entry {
            name: format!("dir-{index:02}"),
            path: format!("dir-{index:02}"),
            is_dir: true,
        })
        .collect()
}

#[test]
fn offline_project_shortcut_focuses_explicit_heading_and_keeps_find_open() {
    let (mut app, commands) = app(Tool::Search);
    app.state = ConnectionState::Disconnected;
    settle(&mut app, SIZE);
    let source = widget(&app, "search_query").id;
    focus(&mut app, source);
    explorer(&mut app, SIZE);
    assert!(app.tools_open);
    assert_eq!(focused(&app), Some(widget(&app, "project_heading").id));
    app.documents.clear();
    app.active_document = None;
    toggle(&mut app, SIZE);
    assert!(!app.tools_open);
    assert_eq!(focused(&app), Some(widget(&app, "project_heading").id));
    assert!(commands.try_recv().is_err());
}

#[test]
fn project_shortcut_focuses_current_first_row_and_flat_retains_native_tab_navigation() {
    let (mut app, commands) = app(Tool::Search);
    load_project(&mut app, &commands, project_rows(24));
    settle(&mut app, SIZE);
    explorer(&mut app, SIZE);
    assert!(app.tools_open);
    assert_eq!(focused(&app), Some(widget(&app, "dir-00").id));
    assert!(commands.try_recv().is_err());
    let mut saw_last = false;
    for _ in 0..40 {
        frame(&mut app, SIZE, key(egui::Key::Tab, egui::Modifiers::NONE));
        frame(&mut app, SIZE, vec![]);
        let row = widget(&app, "dir-23");
        if focused(&app) == Some(row.id) {
            assert!(row.clip.contains_rect(row.rect), "last Flat row clipped");
            saw_last = true;
            break;
        }
    }
    assert!(saw_last);
    assert!(commands.try_recv().is_err());
    frame(&mut app, SIZE, key(egui::Key::Enter, egui::Modifiers::NONE));
    assert!(
        matches!(commands.try_recv().unwrap().op, Operation::List { path } if path == "dir-23")
    );
    assert!(commands.try_recv().is_err());
}

#[test]
fn no_document_empty_project_and_resize_use_explicit_heading() {
    let (mut app, commands) = app(Tool::Tests);
    app.documents.clear();
    app.active_document = None;
    settle(&mut app, SIZE);
    toggle(&mut app, SIZE);
    assert_eq!(focused(&app), Some(widget(&app, "project_heading").id));
    for size in [[1178.0, 814.0], SIZE, [640.0, 480.0]] {
        toggle(&mut app, size);
        assert_eq!(focused(&app), Some(widget(&app, "TEST RESULTS").id));
        explorer(&mut app, size);
        assert!(app.tools_open);
        assert_eq!(focused(&app), Some(widget(&app, "project_heading").id));
        toggle(&mut app, size);
    }
    assert!(commands.try_recv().is_err());
}

#[test]
fn find_remains_open_and_only_a_clean_find_shortcut_transfers_focus() {
    let (mut app, commands) = app(Tool::Search);
    app.find_open = true;
    app.find_query = "saved".into();
    settle(&mut app, SIZE);
    focus(&mut app, egui::Id::new(replace::FIND_INPUT));
    toggle(&mut app, SIZE);
    assert!(!app.tools_open && app.find_open);
    assert_eq!(focused(&app), Some(egui::Id::new(("editor", 1u64))));
    let mut events = key(egui::Key::J, egui::Modifiers::COMMAND);
    events.extend(key(egui::Key::F, egui::Modifiers::COMMAND));
    frame(&mut app, SIZE, events);
    assert!(!app.tools_open && app.find_open);
    assert_eq!(focused(&app), Some(egui::Id::new(("editor", 1u64))));
    assert_eq!(app.find_query, "saved");
    frame(&mut app, SIZE, key(egui::Key::F, egui::Modifiers::COMMAND));
    assert_eq!(focused(&app), Some(egui::Id::new(replace::FIND_INPUT)));
    frame(&mut app, SIZE, vec![egui::Event::Text("new".into())]);
    assert_eq!(app.documents[0].text, "saved 雪");
    assert!(commands.try_recv().is_err());
}

#[test]
fn mixed_text_paste_enter_and_multiple_shortcuts_preserve_original_field_input() {
    for before in [false, true] {
        for text in [
            egui::Event::Text("!".into()),
            egui::Event::Paste("!".into()),
        ] {
            let (mut app, commands) = app(Tool::Search);
            settle(&mut app, SIZE);
            let input = widget(&app, "search_query").id;
            focus(&mut app, input);
            let mut events = key(egui::Key::J, egui::Modifiers::COMMAND);
            if before {
                events.insert(0, text);
            } else {
                events.push(text);
            }
            frame(&mut app, SIZE, events);
            assert!(app.tools_open);
            assert!(app.search_query.contains('!'));
            assert_eq!(app.documents[0].text, "saved 雪");
            assert_eq!(focused(&app), Some(input));
            assert!(commands.try_recv().is_err());
        }
    }
    let (mut app, commands) = app(Tool::Tests);
    settle(&mut app, SIZE);
    focus(&mut app, egui::Id::new("test_report_path"));
    let mut events = key(egui::Key::J, egui::Modifiers::COMMAND);
    events.extend(key(egui::Key::Enter, egui::Modifiers::NONE));
    frame(&mut app, SIZE, events);
    assert!(app.tools_open);
    assert_eq!(app.documents[0].text, "saved 雪");
    let mut events = key(egui::Key::J, egui::Modifiers::COMMAND);
    events.extend(key(
        egui::Key::E,
        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
    ));
    frame(&mut app, SIZE, events);
    assert!(app.tools_open);
    assert!(commands.try_recv().is_err());
}

#[test]
fn held_key_repeats_do_not_toggle_or_take_focus_and_modifiers_are_exact() {
    let (mut app, commands) = app(Tool::Search);
    app.find_open = true;
    settle(&mut app, SIZE);
    frame(
        &mut app,
        SIZE,
        vec![key_event(egui::Key::J, egui::Modifiers::COMMAND, true)],
    );
    assert!(!app.tools_open);
    let find = egui::Id::new(replace::FIND_INPUT);
    focus(&mut app, find);
    for _ in 0..3 {
        frame(
            &mut app,
            SIZE,
            vec![key_event(egui::Key::J, egui::Modifiers::COMMAND, true)],
        );
        assert!(!app.tools_open);
        assert_eq!(focused(&app), Some(find));
    }
    frame(
        &mut app,
        SIZE,
        vec![key_event(egui::Key::J, egui::Modifiers::COMMAND, false)],
    );
    for modifiers in [
        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
        egui::Modifiers::COMMAND | egui::Modifiers::ALT,
    ] {
        frame(&mut app, SIZE, key(egui::Key::J, modifiers));
        assert!(!app.tools_open);
    }
    toggle(&mut app, SIZE);
    assert!(app.tools_open);
    assert_eq!(focused(&app), Some(widget(&app, "search_query").id));
    assert!(commands.try_recv().is_err());
}

#[test]
fn native_window_focus_loss_cancels_access_in_the_same_batch() {
    let (mut app, commands) = app(Tool::Search);
    settle(&mut app, SIZE);
    let mut events = key(egui::Key::J, egui::Modifiers::COMMAND);
    events.push(egui::Event::WindowFocused(false));
    frame(&mut app, SIZE, events);
    assert!(app.tools_open);
    assert!(commands.try_recv().is_err());
}

#[test]
fn platform_command_forms_are_accepted_but_ctrl_plus_mac_command_is_not() {
    for modifiers in [
        egui::Modifiers::CTRL | egui::Modifiers::COMMAND,
        egui::Modifiers::MAC_CMD | egui::Modifiers::COMMAND,
    ] {
        let (mut app, commands) = app(Tool::Search);
        settle(&mut app, SIZE);
        frame(&mut app, SIZE, key(egui::Key::J, modifiers));
        assert!(!app.tools_open);
        frame(&mut app, SIZE, key(egui::Key::J, modifiers));
        assert!(app.tools_open);
        let both = egui::Modifiers::CTRL | egui::Modifiers::MAC_CMD | egui::Modifiers::COMMAND;
        frame(&mut app, SIZE, key(egui::Key::J, both));
        assert!(app.tools_open);
        frame(
            &mut app,
            SIZE,
            key(egui::Key::E, both | egui::Modifiers::SHIFT),
        );
        assert!(app.tools_open);
        frame(
            &mut app,
            SIZE,
            key(egui::Key::E, modifiers | egui::Modifiers::SHIFT),
        );
        assert!(app.tools_open);
        assert_eq!(focused(&app), Some(widget(&app, "project_heading").id));
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn populated_report_retains_scroll_offset_and_selection_after_reopening() {
    let (mut app, commands) = app(Tool::Tests);
    settle(&mut app, SIZE);
    let (id, _) = app.editor_ctx.data(|data| {
        data.get_temp::<(egui::Id, egui::Vec2)>(egui::Id::new(REPORT_SCROLL))
            .unwrap()
    });
    let mut state = egui::scroll_area::State::load(&app.editor_ctx, id).unwrap();
    state.offset.y = 140.0;
    state.store(&app.editor_ctx, id);
    settle(&mut app, SIZE);
    let before = egui::scroll_area::State::load(&app.editor_ctx, id)
        .unwrap()
        .offset;
    assert!(before.y > 0.0);
    toggle(&mut app, SIZE);
    settle(&mut app, SIZE);
    toggle(&mut app, SIZE);
    settle(&mut app, SIZE);
    assert_eq!(
        egui::scroll_area::State::load(&app.editor_ctx, id)
            .unwrap()
            .offset,
        before
    );
    assert_eq!(app.test_report.selected, Some(31));
    assert!(commands.try_recv().is_err());
}

#[test]
fn project_and_editor_access_keep_their_focus_across_resize() {
    for use_project in [false, true] {
        let (mut app, commands) = app(Tool::Tests);
        settle(&mut app, SIZE);
        if use_project {
            explorer(&mut app, SIZE);
        } else {
            toggle(&mut app, SIZE);
        }
        let wanted = if use_project {
            widget(&app, "project_heading").id
        } else {
            egui::Id::new(("editor", 1u64))
        };
        assert_eq!(focused(&app), Some(wanted));
        settle(&mut app, SIZE);
        settle(&mut app, [1178.0, 814.0]);
        assert_eq!(focused(&app), Some(wanted));
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn navigation_and_confirmation_keep_ownership_and_access_intent_never_replays() {
    let (mut app, commands) = app(Tool::Search);
    settle(&mut app, SIZE);
    app.show_file_chooser();
    settle(&mut app, SIZE);
    let owner = focused(&app);
    toggle(&mut app, SIZE);
    assert!(app.tools_open);
    assert_eq!(focused(&app), owner);
    frame(
        &mut app,
        SIZE,
        key(egui::Key::Escape, egui::Modifiers::NONE),
    );
    settle(&mut app, SIZE);
    app.confirm = Some(Confirm::CloseTab(1));
    settle(&mut app, SIZE);
    toggle(&mut app, SIZE);
    assert!(app.tools_open);
    app.confirm = None;
    settle(&mut app, SIZE);
    assert!(app.tools_open);
    assert!(commands.try_recv().is_err());
}

#[test]
fn same_frame_generation_document_navigation_and_new_focus_cancel_final_handoff() {
    for change in [
        "generation",
        "document",
        "version",
        "session",
        "navigation",
        "focus",
        "modal",
    ] {
        let (mut app, commands) = app(Tool::Search);
        settle(&mut app, SIZE);
        let source = widget(&app, "search_query").id;
        focus(&mut app, source);
        let ctx = app.editor_ctx.clone();
        let newer = egui::Id::new("newer_access_owner");
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SIZE.into())),
            events: key(egui::Key::J, egui::Modifiers::COMMAND),
            ..Default::default()
        };
        eframe::App::raw_input_hook(&mut app, &ctx, &mut input);
        let _ = ctx.run(input, |ctx| {
            app.workspace_access_shortcuts(ctx);
            egui::CentralPanel::default().show(ctx, |ui| {
                app.editor(ui);
                ui.add(egui::TextEdit::singleline(&mut String::new()).id(newer));
            });
            match change {
                "generation" => app.generation += 1,
                "document" => app.active_document = None,
                "version" => app.documents[0].edit_version += 1,
                "session" => app.language.session += 1,
                "navigation" => app.navigation_epoch += 1,
                "focus" => ctx.memory_mut(|memory| memory.request_focus(newer)),
                "modal" => app.confirm = Some(Confirm::CloseTab(1)),
                _ => unreachable!(),
            }
            app.finish_workspace_access_frame(ctx);
            assert_ne!(
                ctx.memory(|memory| memory.focused()),
                Some(egui::Id::new(("editor", 1u64))),
                "stale {change} focus"
            );
        });
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn captured_close_survives_panel_motion_and_still_rejects_newer_mixed_input() {
    for mixed in [false, true] {
        let (mut app, commands) = app(Tool::Search);
        settle(&mut app, SIZE);
        let at = widget(&app, "close_tools").rect.center();
        frame(
            &mut app,
            SIZE,
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        let mut events = vec![
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ];
        if mixed {
            events.push(egui::Event::Text("newer input".into()));
        }
        frame(&mut app, [SIZE[0], SIZE[1] + 100.0], events);
        let close = recorded_response(&app, "close_tools");
        assert!(
            close.clicked_by(egui::PointerButton::Primary),
            "egui must recognize the captured close click"
        );
        let emitted = widget(&app, "close_tools").rect;
        assert!(
            !emitted.contains(at),
            "the rendered panel must move before release: press={at:?}, emitted={emitted:?}, interaction={:?}",
            close.rect
        );
        assert_eq!(
            app.tools_open, mixed,
            "only the sole captured click may hide tools"
        );
        if !mixed {
            assert_eq!(focused(&app), Some(egui::Id::new(("editor", 1u64))));
        }
        assert!(commands.try_recv().is_err());
    }
}

const LARGE: [f32; 2] = [1200.0, 900.0];

fn load_search(app: &mut CedarApp, commands: &Receiver<Command>, count: usize) {
    app.tool = Tool::Search;
    app.tools_open = true;
    app.tools_height = Some(420.0);
    app.search();
    let request = commands.try_recv().expect("explicit fixture Search");
    assert!(matches!(request.op, Operation::Search { .. }));
    app.apply_event(Event {
        generation: app.generation,
        id: request.id,
        connected: true,
        result: Ok(Payload::Matches {
            matches: (0..count)
                .map(|index| SearchMatch {
                    path: "draft.txt".into(),
                    line: index + 1,
                    text: format!("retained hit {index}"),
                })
                .collect(),
            truncated: false,
        }),
    });
    settle(app, LARGE);
}

fn click_events(at: egui::Pos2) -> Vec<egui::Event> {
    let mut events = vec![egui::Event::PointerMoved(at)];
    events.extend(
        [true, false]
            .into_iter()
            .map(|pressed| egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            }),
    );
    events
}

#[test]
fn find_entry_and_toggle_choose_only_current_visible_selected_hit_else_query() {
    for state in [
        "active",
        "collapsed",
        "inactive",
        "retired",
        "empty",
        "clipped",
    ] {
        for entry in [false, true] {
            let (mut app, commands) = app(Tool::Search);
            let count = if state == "empty" {
                0
            } else if state == "clipped" {
                80
            } else {
                2
            };
            load_search(&mut app, &commands, count);
            if count > 0 {
                app.select_search_row(count - 1);
            }
            if state == "collapsed" {
                app.search_state.toggle_group(0);
            }
            if state == "inactive" {
                app.search_query = "a new unsubmitted query".into();
            }
            if state == "retired" {
                let query = app.search_query.clone();
                app.search_query.push_str(" changed");
                app.search_filters_changed();
                app.search_query = query;
            }
            settle(&mut app, LARGE);
            let selection = app.search_state.selected_index();
            let expanded = count == 0 || app.search_state.group_expanded(0);
            let source = app.search_state.accepted_snapshot_id();
            let navigation = app.navigation_epoch;
            let version = app.documents[0].edit_version;
            let history = (
                app.location_history.back.len(),
                app.location_history.forward.len(),
            );
            toggle(&mut app, LARGE);
            assert!(!app.tools_open);
            assert_eq!(focused(&app), Some(egui::Id::new(("editor", 1u64))));
            settle(&mut app, LARGE);
            if entry {
                let at = widget(&app, "find_results_entry").rect.center();
                let events = click_events(at);
                frame(&mut app, LARGE, events[..2].to_vec());
                frame(&mut app, LARGE, events[2..].to_vec());
            } else {
                toggle(&mut app, LARGE);
            }
            assert!(
                app.tools_open && app.tool == Tool::Search,
                "{state}/{entry}"
            );
            let expected = if state == "active" {
                "search_row:1"
            } else {
                "search_query"
            };
            assert_eq!(
                focused(&app),
                Some(widget(&app, expected).id),
                "{state}/{entry}"
            );
            assert_eq!(app.search_state.selected_index(), selection);
            assert_eq!(count == 0 || app.search_state.group_expanded(0), expanded);
            assert_eq!(app.search_state.accepted_snapshot_id(), source);
            assert_eq!(app.navigation_epoch, navigation);
            assert_eq!(app.documents[0].edit_version, version);
            assert_eq!(
                (
                    app.location_history.back.len(),
                    app.location_history.forward.len()
                ),
                history
            );
            assert!(
                commands.try_recv().is_err(),
                "focus emitted work for {state}/{entry}"
            );
        }
    }
}

#[derive(Clone)]
struct ProjectRevealFrame {
    row: Widget,
    viewport: egui::Rect,
    offset_before: egui::Vec2,
    offset: egui::Vec2,
    outer_before: egui::Vec2,
    outer_offset: egui::Vec2,
    focused: bool,
    repaint_after: std::time::Duration,
}

fn project_reveal_frame(app: &CedarApp, repaint_after: std::time::Duration) -> ProjectRevealFrame {
    let (viewport, offset_before, offset, outer_before, outer_offset) =
        sidebar_layout_tests::explorer_geometry(app);
    let row = widget(app, "zz-selected.txt");
    ProjectRevealFrame {
        focused: focused(app) == Some(row.id),
        row,
        viewport,
        offset_before,
        offset,
        outer_before,
        outer_offset,
        repaint_after,
    }
}

// ScrollArea commits scroll_to_me at the end of layout, after the current row
// was painted. Admit exactly the repaint it requested only when a changed
// viewport and the already committed offset explain the entire clipping.
fn project_reveal_repaint_pending(before: &ProjectRevealFrame, after: &ProjectRevealFrame) -> bool {
    before.row.clip.contains_rect(before.row.rect)
        && !after.row.clip.contains_rect(after.row.rect)
        && before.row.id == after.row.id
        && before.focused
        && after.focused
        && before.viewport != after.viewport
        && after.offset_before != after.offset
        && before.offset == after.offset_before
        && after.outer_before == after.outer_offset
        && before.outer_offset == after.outer_offset
        && after.repaint_after.is_zero()
        && after
            .row
            .clip
            .contains_rect(after.row.rect.translate(after.offset_before - after.offset))
}

#[test]
fn project_selected_flat_row_is_revealed_without_hiding_find_or_loading() {
    project_selected_flat_row_case(None);
}

#[test]
fn project_selected_flat_row_font_arrival_uses_only_a_committed_repaint() {
    for arrival in 1..=9 {
        assert_eq!(project_selected_flat_row_case(Some(arrival)), arrival == 9);
    }
}

fn project_selected_flat_row_case(arrival: Option<usize>) -> bool {
    let (mut app, commands) = app(Tool::Search);
    let mut deliver = arrival.map(|_| {
        let (fonts, deliver) = system_fonts::SystemFonts::pending_failure_for_test();
        app.system_fonts = fonts;
        deliver
    });
    app.documents[0].path = "zz-selected.txt".into();
    let mut entries = project_rows(32);
    entries.push(Entry {
        name: "zz-selected.txt".into(),
        path: "zz-selected.txt".into(),
        is_dir: false,
    });
    load_project(&mut app, &commands, entries);
    let mut before = None;
    let mut after = None;
    let mut trace = Vec::new();
    for step in 1..=9 {
        if arrival == Some(step) {
            deliver.take().unwrap()();
        }
        let events = if step == 5 {
            key(
                egui::Key::E,
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
            )
        } else {
            vec![]
        };
        let repaint = frame_repaint(&mut app, SIZE, events);
        let current = project_reveal_frame(&app, repaint);
        let coordinates = |rect: egui::Rect| [rect.left(), rect.top(), rect.right(), rect.bottom()];
        trace.push(format!(
            "frame={step} pass={} time={} row={:?} clip={:?} viewport={:?} offset={:?} outer={:?} repaint={repaint:?} font={} error={:?}",
            app.editor_ctx.cumulative_pass_nr(), app.editor_ctx.input(|input| input.time),
            coordinates(current.row.rect), coordinates(current.row.clip), coordinates(current.viewport),
            current.offset, current.outer_offset, app.system_fonts.probe_state_for_test(), app.error,
        ));
        if step == 4 {
            assert!(
                !current.row.rect.intersect(current.row.clip).is_positive(),
                "fixture must clip selected row"
            );
        }
        if step == 5 {
            assert!(app.tools_open && app.tool == Tool::Search);
            assert_eq!(focused(&app), Some(current.row.id));
        }
        before = after;
        after = Some(current);
    }
    let before = before.unwrap();
    let after = after.unwrap();
    let requested_repaint = project_reveal_repaint_pending(&before, &after);
    if requested_repaint {
        // A missing native repaint must never be hidden by an active test pump.
        let mut no_repaint = after.clone();
        no_repaint.repaint_after = std::time::Duration::MAX;
        assert!(!project_reveal_repaint_pending(&before, &no_repaint));
        let repaint = frame_repaint(&mut app, SIZE, vec![]);
        let next = project_reveal_frame(&app, repaint);
        assert_eq!(
            next.offset_before, after.offset,
            "repaint must use the committed offset"
        );
        assert_eq!(next.row.id, after.row.id);
        assert!(next.focused);
    }
    let selected = widget(&app, "zz-selected.txt");
    assert!(
        selected.clip.contains_rect(selected.rect),
        "selected Project row must be revealed; arrival={arrival:?}, committed_repaint={requested_repaint}; {}",
        trace.join("\n")
    );
    assert!(app.tools_open && app.tool == Tool::Search);
    assert_eq!(focused(&app), Some(selected.id));
    assert_eq!(app.documents[0].text, "saved 雪");
    assert!(commands.try_recv().is_err());
    requested_repaint
}

#[test]
fn project_reveal_observer_rejects_a_persistently_clipped_row() {
    let (mut app, commands) = app(Tool::Search);
    let (fonts, deliver) = system_fonts::SystemFonts::pending_failure_for_test();
    app.system_fonts = fonts;
    app.documents[0].path = "zz-selected.txt".into();
    let mut entries = project_rows(32);
    entries.push(Entry {
        name: "zz-selected.txt".into(),
        path: "zz-selected.txt".into(),
        is_dir: false,
    });
    load_project(&mut app, &commands, entries);
    for _ in 0..8 {
        frame(&mut app, SIZE, vec![]);
    }
    let before = project_reveal_frame(&app, std::time::Duration::ZERO);
    deliver();
    let repaint = frame_repaint(&mut app, SIZE, vec![]);
    let after = project_reveal_frame(&app, repaint);
    assert_ne!(before.viewport, after.viewport);
    assert!(!after.row.clip.contains_rect(after.row.rect));
    assert!(!project_reveal_repaint_pending(&before, &after));
    frame(&mut app, SIZE, vec![]);
    let row = widget(&app, "zz-selected.txt");
    assert!(!row.clip.contains_rect(row.rect));
    assert!(commands.try_recv().is_err());
}

#[test]
fn tree_access_preserves_selection_and_native_tree_arrows_without_expanding() {
    let (mut app, commands) = app(Tool::Search);
    app.explorer_set_mode(explorer_tree::Mode::Tree);
    app.explorer_refresh("");
    let request = commands.try_recv().unwrap();
    app.apply_event(Event {
        generation: app.generation,
        id: request.id,
        connected: true,
        result: Ok(Payload::Entries {
            entries: project_rows(3),
        }),
    });
    app.explorer_select("dir-01");
    settle(&mut app, LARGE);
    frame(&mut app, LARGE, key(egui::Key::Num1, egui::Modifiers::ALT));
    assert!(app.tools_open);
    assert_eq!(focused(&app), Some(widget(&app, "dir-01").id));
    assert_eq!(app.explorer.selected, "dir-01");
    assert!(commands.try_recv().is_err());
    frame(
        &mut app,
        LARGE,
        key(egui::Key::ArrowDown, egui::Modifiers::NONE),
    );
    assert_eq!(focused(&app), Some(widget(&app, "dir-02").id));
    assert_eq!(app.explorer.selected, "dir-02");
    assert_eq!(app.explorer_loaded_directory_count(), 1);
    assert!(commands.try_recv().is_err());
}

#[test]
fn project_rejects_retained_flat_rows_after_connection_reset() {
    let (mut app, commands) = app(Tool::Search);
    load_project(&mut app, &commands, project_rows(2));
    settle(&mut app, LARGE);
    app.explorer.reset_connection();
    app.generation += 1;
    settle(&mut app, LARGE);
    explorer(&mut app, LARGE);
    assert!(app.tools_open);
    assert_eq!(focused(&app), Some(widget(&app, "project_heading").id));
    assert!(commands.try_recv().is_err());
}

#[test]
fn original_owned_arrow_mixed_with_access_never_becomes_a_clean_shortcut() {
    for tree in [false, true] {
        let (mut app, commands) = app(Tool::Search);
        if tree {
            app.explorer_set_mode(explorer_tree::Mode::Tree);
            settle(&mut app, LARGE);
            frame(&mut app, LARGE, key(egui::Key::Num1, egui::Modifiers::ALT));
        } else {
            load_search(&mut app, &commands, 2);
            recorded_response(&app, "search_row:0").request_focus();
            frame(&mut app, LARGE, vec![]);
        }
        let mut events = key(egui::Key::J, egui::Modifiers::COMMAND);
        events.extend(key(egui::Key::ArrowDown, egui::Modifiers::NONE));
        frame(&mut app, LARGE, events);
        assert!(app.tools_open, "mixed arrow must veto access, Tree={tree}");
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn access_rejects_prior_held_owned_arrow_pointer_and_ime_until_release() {
    for held in ["tree", "search", "pointer", "ime"] {
        let (mut app, commands) = app(Tool::Search);
        if held == "tree" {
            app.explorer_set_mode(explorer_tree::Mode::Tree);
            settle(&mut app, LARGE);
            frame(&mut app, LARGE, key(egui::Key::Num1, egui::Modifiers::ALT));
        } else if held == "search" {
            load_search(&mut app, &commands, 2);
            recorded_response(&app, "search_row:0").request_focus();
            frame(&mut app, LARGE, vec![]);
        } else {
            settle(&mut app, LARGE);
        }
        let event = match held {
            "tree" | "search" => key_event(egui::Key::ArrowDown, egui::Modifiers::NONE, true),
            "pointer" => egui::Event::PointerButton {
                pos: egui::pos2(1190.0, 890.0),
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
            "ime" => egui::Event::Ime(egui::ImeEvent::Preedit("composition".into())),
            _ => unreachable!(),
        };
        frame(&mut app, LARGE, vec![event]);
        toggle(&mut app, LARGE);
        assert!(app.tools_open, "held {held} must veto access");
        let release = match held {
            "tree" | "search" => key_event(egui::Key::ArrowDown, egui::Modifiers::NONE, false),
            "pointer" => egui::Event::PointerButton {
                pos: egui::pos2(1190.0, 890.0),
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
            "ime" => egui::Event::Ime(egui::ImeEvent::Disabled),
            _ => unreachable!(),
        };
        frame(&mut app, LARGE, vec![release]);
        toggle(&mut app, LARGE);
        assert!(!app.tools_open, "released {held} allows fresh access");
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn capture_origin_changes_before_update_and_missing_metadata_fail_closed() {
    for change in [
        "document",
        "version",
        "session",
        "generation",
        "navigation",
        "query",
        "scope",
        "masks",
        "tool",
        "focus",
        "missing",
    ] {
        let (mut app, commands) = app(Tool::Search);
        settle(&mut app, LARGE);
        let ctx = app.editor_ctx.clone();
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, LARGE.into())),
            events: key(egui::Key::J, egui::Modifiers::COMMAND),
            ..Default::default()
        };
        if change != "missing" {
            eframe::App::raw_input_hook(&mut app, &ctx, &mut input);
        }
        match change {
            "document" => app.active_document = None,
            "version" => app.documents[0].edit_version += 1,
            "session" => app.language.session += 1,
            "generation" => app.generation += 1,
            "navigation" => app.navigation_epoch += 1,
            "query" => app.search_query.push_str("changed"),
            "scope" => app.search_scope = "changed".into(),
            "masks" => app.search_masks = "*.changed".into(),
            "tool" => app.tool = Tool::Git,
            "focus" => {
                ctx.memory_mut(|memory| memory.request_focus(egui::Id::new(("editor", 1u64))))
            }
            "missing" => {}
            _ => unreachable!(),
        }
        let mut native = eframe::Frame::_new_kittest();
        let _ = ctx.run(input, |ctx| eframe::App::update(&mut app, ctx, &mut native));
        assert!(app.tools_open, "stale origin {change}");
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn poisoned_or_stale_primary_press_cannot_hide_on_its_later_clean_release() {
    for poison in ["text", "wheel", "ime", "version", "session", "navigation"] {
        let (mut app, commands) = app(Tool::Search);
        settle(&mut app, LARGE);
        let at = widget(&app, "close_tools").rect.center();
        let mut press = vec![
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ];
        match poison {
            "text" => press.push(egui::Event::Text("newer".into())),
            "wheel" => press.push(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -4.0),
                modifiers: egui::Modifiers::NONE,
            }),
            "ime" => press.push(egui::Event::Ime(egui::ImeEvent::Preedit(
                "composition".into(),
            ))),
            _ => {}
        }
        frame(&mut app, LARGE, press);
        match poison {
            "version" => app.documents[0].edit_version += 1,
            "session" => app.language.session += 1,
            "navigation" => app.navigation_epoch += 1,
            _ => {}
        }
        frame(
            &mut app,
            LARGE,
            vec![egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(
            app.tools_open,
            "poisoned/stale {poison} press must be retired"
        );
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn native_space_activation_repeat_does_not_edit_the_returned_field() {
    for close in [false, true] {
        for activation in [egui::Key::Space, egui::Key::Enter] {
            let (mut app, commands) = app(Tool::Search);
            settle(&mut app, LARGE);
            if !close {
                toggle(&mut app, LARGE);
                settle(&mut app, LARGE);
            }
            let probe = if close {
                "close_tools"
            } else {
                "find_results_entry"
            };
            recorded_response(&app, probe).request_focus();
            frame(&mut app, LARGE, vec![]);
            let text = app.documents[0].text.clone();
            let query = app.search_query.clone();
            let version = app.documents[0].edit_version;
            let press = || {
                let mut events = vec![key_event(activation, egui::Modifiers::NONE, true)];
                if activation == egui::Key::Space {
                    events.push(egui::Event::Text(" ".into()));
                }
                events
            };
            frame(&mut app, LARGE, press());
            assert_eq!(app.tools_open, !close);
            assert_eq!(
                app.documents[0].text, text,
                "initial {activation:?}, close={close}"
            );
            assert_eq!(
                app.search_query, query,
                "initial {activation:?}, close={close}"
            );
            for _ in 0..2 {
                frame(&mut app, LARGE, press());
            }
            assert_eq!(
                app.documents[0].text, text,
                "held {activation:?}, close={close}"
            );
            assert_eq!(app.documents[0].edit_version, version);
            assert_eq!(
                app.search_query, query,
                "held {activation:?}, close={close}"
            );
            assert!(
                commands.try_recv().is_err(),
                "held activation must never submit Search"
            );
            // Release and a new press in the same original batch are distinct.
            let mut fresh = vec![key_event(activation, egui::Modifiers::NONE, false)];
            fresh.extend(press());
            fresh.push(key_event(activation, egui::Modifiers::NONE, false));
            frame(&mut app, LARGE, fresh);
            if activation == egui::Key::Enter && !close {
                assert!(
                    matches!(commands.try_recv().unwrap().op, Operation::Search { .. }),
                    "fresh Enter submits once"
                );
            } else if close {
                assert_ne!(
                    app.documents[0].text, text,
                    "fresh key edits returned editor"
                );
            } else {
                assert_eq!(
                    app.search_query.len(),
                    query.len() + 1,
                    "fresh Space enters one character"
                );
            }
            assert!(commands.try_recv().is_err());
        }
    }
}

#[test]
fn held_activation_filter_preserves_unrelated_text_paste_and_ime() {
    let (mut app, commands) = app(Tool::Search);
    settle(&mut app, LARGE);
    toggle(&mut app, LARGE);
    settle(&mut app, LARGE);
    recorded_response(&app, "find_results_entry").request_focus();
    frame(&mut app, LARGE, vec![]);
    frame(
        &mut app,
        LARGE,
        vec![
            key_event(egui::Key::Space, egui::Modifiers::NONE, true),
            egui::Event::Text(" ".into()),
        ],
    );
    let keep = vec![
        egui::Event::Text("unrelated".into()),
        egui::Event::Paste("paste".into()),
        egui::Event::Ime(egui::ImeEvent::Preedit("composition".into())),
        egui::Event::Ime(egui::ImeEvent::Disabled),
    ];
    let mut events = vec![
        key_event(egui::Key::Space, egui::Modifiers::NONE, true),
        egui::Event::Text(" ".into()),
    ];
    events.extend(keep.clone());
    let ctx = app.editor_ctx.clone();
    let mut raw = egui::RawInput {
        events,
        ..Default::default()
    };
    eframe::App::raw_input_hook(&mut app, &ctx, &mut raw);
    assert_eq!(raw.events, keep);
    let mut unassociated = vec![key_event(egui::Key::Space, egui::Modifiers::NONE, true)];
    unassociated.extend(keep.clone());
    unassociated.push(egui::Event::Text(" ".into()));
    let mut raw = egui::RawInput {
        events: unassociated,
        ..Default::default()
    };
    eframe::App::raw_input_hook(&mut app, &ctx, &mut raw);
    let mut expected = keep;
    expected.push(egui::Event::Text(" ".into()));
    assert_eq!(
        raw.events, expected,
        "a later unassociated space remains ordinary input"
    );
    assert!(commands.try_recv().is_err());
}

#[test]
fn access_toggle_is_not_replayed_by_a_discarded_render_pass() {
    let (mut app, commands) = app(Tool::Search);
    settle(&mut app, LARGE);
    let ctx = app.editor_ctx.clone();
    let mut input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, LARGE.into())),
        events: key(egui::Key::J, egui::Modifiers::COMMAND),
        ..Default::default()
    };
    eframe::App::raw_input_hook(&mut app, &ctx, &mut input);
    let mut native = eframe::Frame::_new_kittest();
    let mut renders = 0;
    let output = ctx.run(input, |ctx| {
        eframe::App::update(&mut app, ctx, &mut native);
        assert!(
            !app.tools_open,
            "toggle must happen once on the original pass"
        );
        renders += 1;
        if renders == 1 {
            ctx.request_discard("access lifetime test");
        }
    });
    assert!(output.platform_output.num_completed_passes >= 2);
    assert!(!app.tools_open);
    assert_eq!(focused(&app), Some(egui::Id::new(("editor", 1u64))));
    assert!(commands.try_recv().is_err());
}

#[test]
fn zero_project_viewport_does_not_focus_invisible_rows() {
    let (mut app, commands) = app(Tool::Search);
    load_project(&mut app, &commands, project_rows(2));
    app.tools_height = Some(500.0);
    settle(&mut app, LARGE);
    recorded_response(&app, "search_query").request_focus();
    frame(&mut app, LARGE, vec![]);
    let row_ids = [widget(&app, "dir-00").id, widget(&app, "dir-01").id];
    let tiny = [640.0, 64.0];
    explorer(&mut app, tiny);
    assert!(app.tools_open);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, tiny.into());
    for id in row_ids {
        assert_ne!(focused(&app), Some(id));
    }
    app.editor_ctx.data(|data| {
        let probes = data
            .get_temp::<HashMap<String, Widget>>(egui::Id::new(PROBE))
            .unwrap();
        for name in ["dir-00", "dir-01"] {
            if let Some(row) = probes.get(name) {
                assert!(
                    !row.clip.intersect(screen).is_positive(),
                    "fixture must leave zero Project viewport"
                );
            }
        }
    });
    settle(&mut app, LARGE);
    assert!(
        !focused(&app).is_some_and(|id| row_ids.contains(&id)),
        "dropped request must not replay"
    );
    assert_eq!(app.documents[0].text, "saved 雪");
    assert!(commands.try_recv().is_err());
}

#[test]
fn native_close_preserves_clean_keyboard_ownership_with_only_primary_pointer_held() {
    for case in [
        "retired",
        "live",
        "other_pointer",
        "key",
        "ime",
        "mixed_text",
        "find_entry",
    ] {
        let (mut app, commands) = app(Tool::Search);
        settle(&mut app, LARGE);
        let at = egui::pos2(1190.0, 890.0);
        frame(
            &mut app,
            LARGE,
            vec![egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        if case != "live" {
            app.tools_height_reset.reset(true);
        }
        let probe = if case == "find_entry" {
            "find_results_entry"
        } else {
            "close_tools"
        };
        recorded_response(&app, probe).request_focus();
        frame(&mut app, LARGE, vec![]);
        match case {
            "other_pointer" => frame(
                &mut app,
                LARGE,
                vec![egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Secondary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                }],
            ),
            "key" => frame(
                &mut app,
                LARGE,
                vec![key_event(egui::Key::A, egui::Modifiers::NONE, true)],
            ),
            "ime" => frame(
                &mut app,
                LARGE,
                vec![egui::Event::Ime(egui::ImeEvent::Preedit(
                    "composition".into(),
                ))],
            ),
            _ => {}
        }
        let before = focused(&app);
        if case == "key" {
            assert_eq!(
                before,
                Some(recorded_response(&app, probe).id),
                "foreign held key must keep the native Close owner"
            );
        }
        let mut events = key(egui::Key::Enter, egui::Modifiers::NONE);
        if case == "mixed_text" {
            events.push(egui::Event::Text("newer".into()));
        }
        frame(&mut app, LARGE, events);
        let accepted = case == "retired" || case == "live";
        assert_eq!(app.tools_open, !accepted, "{case}");
        if accepted {
            assert_eq!(focused(&app), Some(egui::Id::new(("editor", 1u64))));
            if case == "retired" {
                assert!(!app.tools_height_reset.capture_allowed());
            }
        }
        if case == "find_entry" {
            assert_eq!(focused(&app), before);
        }
        if let Ok(command) = commands.try_recv() {
            panic!(
                "{case}: unexpected operation kind {:?}; focus {:?}, intended control {:?}",
                std::mem::discriminant(&command.op),
                focused(&app),
                recorded_response(&app, probe).id
            );
        }
    }
}

#[test]
fn witnessed_ready_retirement_rejects_every_unrelated_transition_and_overflow() {
    for change in [
        "navigation",
        "document",
        "version",
        "session",
        "generation",
        "query",
        "scope",
        "masks",
        "focus",
        "missing_reply",
        "unowned_reply",
        "overflow",
        "second_retirement",
    ] {
        let (mut app, commands) = app(Tool::Search);
        settle(&mut app, LARGE);
        app.open("target.txt".into(), None);
        let request = commands.try_recv().expect("fixture's explicit Read");
        assert!(matches!(request.op, Operation::Read { .. }));
        if change == "overflow" {
            app.navigation_epoch = u64::MAX;
        }
        let ready = workspace_access::ReadyNavigation {
            request: request.id + u64::from(change == "unowned_reply"),
            generation: app.generation,
            navigation: app.navigation_epoch,
        };
        let ctx = app.editor_ctx.clone();
        let mut raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, LARGE.into())),
            events: key(egui::Key::J, egui::Modifiers::COMMAND),
            ..Default::default()
        };
        eframe::App::raw_input_hook(&mut app, &ctx, &mut raw);
        let _ = ctx.run(raw, |ctx| {
            let retirement = app.prepare_workspace_access_retirement(ctx, ready);
            if change == "overflow" || change == "unowned_reply" {
                assert!(retirement.is_none(), "{change} cannot mint a witness");
                return;
            }
            let retirement = retirement.expect("fixture's current pending origin");
            app.navigation_epoch += 1;
            if change == "second_retirement" {
                app.finish_workspace_access_retirement(ctx, retirement);
                let second = workspace_access::ReadyNavigation {
                    navigation: app.navigation_epoch,
                    ..ready
                };
                assert!(
                    app.prepare_workspace_access_retirement(ctx, second)
                        .is_none(),
                    "one gesture cannot chain retirement epochs"
                );
                return;
            }
            match change {
                "navigation" => app.navigation_epoch += 1,
                "document" => app.active_document = None,
                "version" => app.documents[0].edit_version += 1,
                "session" => app.language.session += 1,
                "generation" => app.generation += 1,
                "query" => app.search_query.push_str("changed"),
                "scope" => app.search_scope = "changed".into(),
                "masks" => app.search_masks = "*.changed".into(),
                "focus" => ctx.memory_mut(|memory| {
                    memory.request_focus(egui::Id::new("new retirement owner"))
                }),
                "missing_reply" => {
                    app.pending.remove(&request.id);
                }
                _ => unreachable!(),
            }
            app.finish_workspace_access_retirement(ctx, retirement);
            app.workspace_access_shortcuts(ctx);
            assert!(
                app.tools_open,
                "{change} must not borrow the cancellation witness"
            );
            app.finish_workspace_access_frame(ctx);
        });
        assert!(
            commands.try_recv().is_err(),
            "retirement guard emitted work"
        );
    }
}
