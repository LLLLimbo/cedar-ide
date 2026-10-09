//! Production-frame keyboard access tests; probes retain rendered responses,
//! never reconstructed auto IDs or a second focus/navigation implementation.
use super::*;

const PROBE: &str = "workspace_access_test_probe";
const REPORT_SCROLL: &str = "workspace_access_report_scroll";
const SIZE: [f32; 2] = [780.0, 540.0];
const TOOLS: [(Tool, &str); 5] = [
    (Tool::Search, "PROJECT SEARCH"),
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
}

pub(super) fn begin(ctx: &egui::Context) {
    ctx.data_mut(|data| data.insert_temp(egui::Id::new(PROBE), HashMap::<String, Widget>::new()));
}

pub(super) fn record(ui: &egui::Ui, name: &str, response: &egui::Response) {
    let widget = Widget {
        id: response.id,
        rect: response.rect,
        clip: ui.clip_rect(),
    };
    ui.ctx().data_mut(|data| {
        data.get_temp_mut_or_default::<HashMap<String, Widget>>(egui::Id::new(PROBE))
            .insert(name.into(), widget);
    });
}

pub(super) fn record_report_scroll(ctx: &egui::Context, id: egui::Id, offset: egui::Vec2) {
    ctx.data_mut(|data| data.insert_temp(egui::Id::new(REPORT_SCROLL), (id, offset)));
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

fn focused(app: &CedarApp) -> Option<egui::Id> {
    app.editor_ctx.memory(|memory| memory.focused())
}

fn frame(app: &mut CedarApp, size: [f32; 2], events: Vec<egui::Event>) {
    let ctx = app.editor_ctx.clone();
    let mut native = eframe::Frame::_new_kittest();
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size.into())),
            events,
            ..Default::default()
        },
        |ctx| eframe::App::update(app, ctx, &mut native),
    );
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
fn every_tool_hides_to_visible_editor_and_reopens_same_rendered_header_without_work() {
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
            Some(widget(&app, header).id),
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

#[test]
fn offline_explorer_shortcut_does_not_focus_an_unrequested_fallback() {
    let (mut app, commands) = app(Tool::Search);
    app.state = ConnectionState::Disconnected;
    settle(&mut app, SIZE);
    let source = widget(&app, "search_query").id;
    focus(&mut app, source);
    explorer(&mut app, SIZE);
    assert!(!app.tools_open);
    assert_ne!(focused(&app), Some(widget(&app, "explorer_refresh").id));
    assert_ne!(focused(&app), Some(widget(&app, "sidebar_Search").id));
    // No-document hide may use a rendered enabled Explorer selector instead.
    app.documents.clear();
    app.active_document = None;
    toggle(&mut app, SIZE);
    toggle(&mut app, SIZE);
    assert_eq!(focused(&app), Some(widget(&app, "sidebar_Search").id));
    assert!(commands.try_recv().is_err());
}

#[test]
fn explorer_shortcut_focuses_refresh_without_read_and_tabs_through_first_and_last_entries() {
    let (mut app, commands) = app(Tool::Language);
    app.entries = (0..24)
        .map(|index| Entry {
            name: format!("dir-{index:02}"),
            path: format!("dir-{index:02}"),
            is_dir: true,
        })
        .collect();
    settle(&mut app, SIZE);
    explorer(&mut app, SIZE);
    assert!(!app.tools_open);
    assert_eq!(focused(&app), Some(widget(&app, "explorer_refresh").id));
    assert!(commands.try_recv().is_err());
    let mut saw_first = false;
    let mut saw_last = false;
    for _ in 0..40 {
        frame(&mut app, SIZE, key(egui::Key::Tab, egui::Modifiers::NONE));
        frame(&mut app, SIZE, vec![]);
        for (name, seen) in [("dir-00", &mut saw_first), ("dir-23", &mut saw_last)] {
            let row = widget(&app, name);
            if focused(&app) == Some(row.id) {
                assert!(row.clip.contains_rect(row.rect), "{name} clipped");
                *seen = true;
            }
        }
        if saw_last {
            break;
        }
    }
    assert!(saw_first && saw_last);
    assert!(commands.try_recv().is_err());
    frame(&mut app, SIZE, key(egui::Key::Enter, egui::Modifiers::NONE));
    assert!(
        matches!(commands.try_recv().unwrap().op, Operation::List { path } if path == "dir-23")
    );
    assert!(commands.try_recv().is_err());
}

#[test]
fn no_document_empty_explorer_and_resize_use_available_anchors() {
    let (mut app, commands) = app(Tool::Tests);
    app.documents.clear();
    app.active_document = None;
    settle(&mut app, SIZE);
    toggle(&mut app, SIZE);
    assert_eq!(focused(&app), Some(widget(&app, "explorer_refresh").id));
    for size in [[1178.0, 814.0], SIZE, [640.0, 480.0]] {
        toggle(&mut app, size);
        assert_eq!(focused(&app), Some(widget(&app, "TEST RESULTS").id));
        explorer(&mut app, size);
        assert!(!app.tools_open);
        assert_eq!(focused(&app), Some(widget(&app, "explorer_refresh").id));
    }
    assert!(commands.try_recv().is_err());
}

#[test]
fn find_remains_open_and_newer_find_focus_wins_mixed_shortcuts() {
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
    assert!(!app.tools_open);
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
    assert_eq!(focused(&app), Some(widget(&app, "PROJECT SEARCH").id));
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
        assert!(!app.tools_open);
        assert_eq!(focused(&app), Some(widget(&app, "explorer_refresh").id));
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
fn access_actions_cancel_older_sidebar_activation_reveal_across_resize() {
    for use_explorer in [false, true] {
        let (mut app, commands) = app(Tool::Language);
        app.editor_ctx.style_mut(|style| {
            style
                .text_styles
                .insert(egui::TextStyle::Button, FontId::proportional(18.0));
        });
        settle(&mut app, SIZE);
        app.tools_open = false;
        settle(&mut app, SIZE);
        let selector = widget(&app, "sidebar_Tests").clone();
        focus(&mut app, selector.id);
        let at = widget(&app, "sidebar_Tests").rect.center();
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
        assert!(app.tools_open && app.tool == Tool::Tests);
        if use_explorer {
            explorer(&mut app, SIZE);
        } else {
            toggle(&mut app, SIZE);
        }
        let wanted = if use_explorer {
            widget(&app, "explorer_refresh").id
        } else {
            egui::Id::new(("editor", 1u64))
        };
        assert_eq!(focused(&app), Some(wanted));
        settle(&mut app, SIZE);
        settle(&mut app, [1178.0, 814.0]);
        assert_eq!(focused(&app), Some(wanted));
        assert_ne!(focused(&app), Some(selector.id));
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
    for change in ["generation", "document", "navigation", "focus", "modal"] {
        let (mut app, commands) = app(Tool::Search);
        settle(&mut app, SIZE);
        let source = widget(&app, "search_query").id;
        focus(&mut app, source);
        let ctx = app.editor_ctx.clone();
        let newer = egui::Id::new("newer_access_owner");
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SIZE.into())),
                events: key(egui::Key::J, egui::Modifiers::COMMAND),
                ..Default::default()
            },
            |ctx| {
                app.workspace_access_shortcuts(ctx);
                egui::CentralPanel::default().show(ctx, |ui| {
                    app.editor(ui);
                    ui.add(egui::TextEdit::singleline(&mut String::new()).id(newer));
                });
                match change {
                    "generation" => app.generation += 1,
                    "document" => app.active_document = None,
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
            },
        );
        assert!(commands.try_recv().is_err());
    }
}
