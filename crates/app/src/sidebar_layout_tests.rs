//! Geometry and interaction checks exercise the production sidebar, including
//! widgets outside the paint clip. Test-only probes do not affect widget IDs.
use super::*;

const TOOLS: [(Tool, &str); 5] = [
    (Tool::Search, "Search"),
    (Tool::Git, "Git"),
    (Tool::Run, "Run"),
    (Tool::Language, "LSP"),
    (Tool::Tests, "Tests"),
];
const PROBE: &str = "sidebar_layout_test_probe";

#[derive(Clone)]
struct Widget {
    id: egui::Id,
    rect: egui::Rect,
    clip: egui::Rect,
}
#[derive(Clone)]
struct Scroll {
    id: egui::Id,
    rect: egui::Rect,
    content: egui::Vec2,
    offset: egui::Vec2,
}
#[derive(Clone, Default)]
struct Snapshot {
    widgets: HashMap<String, Widget>,
    outer: Option<Scroll>,
    explorer: Option<Scroll>,
}

pub(super) fn begin(ctx: &egui::Context) {
    ctx.data_mut(|data| data.insert_temp(egui::Id::new(PROBE), Snapshot::default()));
}
pub(super) fn record(ui: &egui::Ui, label: &str, response: &egui::Response) {
    ui.ctx().data_mut(|data| {
        data.get_temp_mut_or_default::<Snapshot>(egui::Id::new(PROBE))
            .widgets
            .insert(
                label.into(),
                Widget {
                    id: response.id,
                    rect: response.rect,
                    clip: ui.clip_rect(),
                },
            );
    });
}
pub(super) fn record_scroll<R>(
    ctx: &egui::Context,
    outer: bool,
    output: &egui::scroll_area::ScrollAreaOutput<R>,
) {
    ctx.data_mut(|data| {
        let snapshot = data.get_temp_mut_or_default::<Snapshot>(egui::Id::new(PROBE));
        let scroll = Some(Scroll {
            id: output.id,
            rect: output.inner_rect,
            content: output.content_size,
            offset: output.state.offset,
        });
        if outer {
            snapshot.outer = scroll;
        } else {
            snapshot.explorer = scroll;
        }
    });
}
fn snapshot(app: &CedarApp) -> Snapshot {
    app.editor_ctx
        .data(|data| data.get_temp::<Snapshot>(egui::Id::new(PROBE)).unwrap())
}
fn widget(app: &CedarApp, label: &str) -> Widget {
    snapshot(app)
        .widgets
        .get(label)
        .unwrap_or_else(|| panic!("missing {label}"))
        .clone()
}
fn panel(app: &CedarApp, id: &str) -> egui::Rect {
    egui::containers::panel::PanelState::load(&app.editor_ctx, egui::Id::new(id))
        .unwrap()
        .rect
}
fn set_width(app: &CedarApp, width: f32) {
    app.editor_ctx.data_mut(|data| {
        data.insert_persisted(
            egui::Id::new("explorer"),
            egui::containers::panel::PanelState {
                rect: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 540.0)),
            },
        )
    });
}
fn app(width: f32, long_directory: bool, font_size: f32) -> (CedarApp, Receiver<Command>) {
    let mut app = CedarApp::empty();
    app.state = ConnectionState::Ready;
    app.open_form = false;
    app.tool = Tool::Tests;
    app.tools_open = true;
    // Match CedarApp::new, not egui's smaller default spacing.
    app.editor_ctx.style_mut(|style| {
        style.spacing.item_spacing = egui::vec2(9.0, 8.0);
        style.spacing.button_padding = egui::vec2(10.0, 6.0);
        style
            .text_styles
            .insert(egui::TextStyle::Body, FontId::proportional(14.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, FontId::proportional(font_size));
        style
            .text_styles
            .insert(egui::TextStyle::Monospace, FontId::monospace(14.0));
    });
    set_width(&app, width);
    if long_directory {
        app.entries = (0..64)
            .map(|index| {
                let name = format!("file-{index:03}.txt");
                Entry {
                    path: name.clone(),
                    name,
                    is_dir: false,
                }
            })
            .collect();
    }
    let (worker, commands) = Worker::recording();
    app.worker = Some(worker);
    (app, commands)
}
fn load_long_report(app: &mut CedarApp) {
    let cases = (0..32)
        .map(|index| format!("<testcase name=\"case-{index}\" time=\"0.1\"/>"))
        .collect::<String>();
    app.test_report.path = "TEST-sidebar.xml".into();
    app.test_report.snapshot = Some(test_report_ui::Snapshot {
        source: test_report_ui::Load {
            generation: app.generation,
            id: 1,
            path: app.test_report.path.clone(),
        },
        revision: "fixture".into(),
        report: test_reports::parse_report(&format!(
            "<testsuite name=\"Sidebar\" tests=\"32\" failures=\"0\" errors=\"0\" skipped=\"0\">{cases}</testsuite>"
        ))
        .unwrap(),
    });
}
fn frame(app: &mut CedarApp, size: [f32; 2], events: Vec<egui::Event>) -> egui::FullOutput {
    scaled_frame(app, size, events, 1.0)
}
fn scaled_frame(
    app: &mut CedarApp,
    size: [f32; 2],
    events: Vec<egui::Event>,
    scale: f32,
) -> egui::FullOutput {
    let ctx = app.editor_ctx.clone();
    let mut input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size.into())),
        events,
        ..Default::default()
    };
    input
        .viewports
        .get_mut(&egui::ViewportId::ROOT)
        .unwrap()
        .native_pixels_per_point = Some(scale);
    ctx.run(input, |ctx| {
        if let Some(id) = ctx.data_mut(|data| {
            let id = data.get_temp::<egui::Id>(egui::Id::new("sidebar_test_request_focus"));
            data.remove::<egui::Id>(egui::Id::new("sidebar_test_request_focus"));
            id
        }) {
            ctx.memory_mut(|memory| memory.request_focus(id));
        }
        app.shortcuts(ctx);
        app.header(ctx);
        app.footer(ctx);
        app.notifications(ctx);
        app.tools(ctx);
        app.sidebar(ctx);
        egui::CentralPanel::default().show(ctx, |ui| {
            if !app.documents.is_empty() {
                app.editor(ui);
            }
        });
    })
}
fn settle(app: &mut CedarApp, size: [f32; 2]) {
    for _ in 0..4 {
        frame(app, size, vec![]);
    }
}
fn assert_visible(app: &CedarApp, label: &str) {
    let widget = widget(app, label);
    let outer = snapshot(app).outer.unwrap();
    assert!(
        outer.rect.contains_rect(widget.rect),
        "{label} outside outer viewport: {:?} / {:?}",
        widget.rect,
        outer.rect
    );
    assert!(
        widget.clip.contains_rect(widget.rect),
        "{label} clipped: {:?} / {:?}",
        widget.rect,
        widget.clip
    );
}
fn focus(app: &mut CedarApp, size: [f32; 2], label: &str) {
    let id = widget(app, label).id;
    app.editor_ctx
        .data_mut(|data| data.insert_temp(egui::Id::new("sidebar_test_request_focus"), id));
    settle(app, size);
    assert_eq!(app.editor_ctx.memory(|memory| memory.focused()), Some(id));
    assert_visible(app, label);
}
fn key(app: &mut CedarApp, size: [f32; 2], key: egui::Key, modifiers: egui::Modifiers) {
    for pressed in [true, false] {
        frame(
            app,
            size,
            vec![egui::Event::Key {
                key,
                physical_key: Some(key),
                pressed,
                repeat: false,
                modifiers,
            }],
        );
    }
    settle(app, size);
}
fn click(app: &mut CedarApp, size: [f32; 2], label: &str) {
    assert_visible(app, label);
    let at = widget(app, label).rect.center();
    for pressed in [true, false] {
        frame(
            app,
            size,
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    settle(app, size);
}
fn wheel(app: &mut CedarApp, size: [f32; 2], at: egui::Pos2, delta: f32) {
    frame(
        app,
        size,
        vec![
            egui::Event::PointerMoved(at),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, delta),
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    for _ in 0..30 {
        frame(app, size, vec![]);
    }
}
fn scroll_outer_bottom(app: &mut CedarApp, size: [f32; 2]) {
    let outer = snapshot(app).outer.unwrap();
    // The outer gutter is outside the inner explorer and always owns this wheel.
    wheel(
        app,
        size,
        egui::pos2(outer.rect.right() + 5.0, outer.rect.center().y),
        -10_000.0,
    );
    assert_visible(app, "help");
}
fn assert_geometry(app: &CedarApp, width: f32) {
    let snapshot = snapshot(app);
    let outer = snapshot.outer.unwrap();
    let explorer = snapshot.explorer.unwrap();
    let sidebar = panel(app, "explorer");
    assert!(
        (sidebar.width() - width).abs() < 0.1,
        "sidebar expanded: {sidebar:?} / {width}"
    );
    assert!(
        (sidebar.shrink(12.0).right() - outer.rect.right() - 10.0).abs() < 0.1,
        "outer gutter changed"
    );
    assert!(
        (outer.rect.right() - explorer.rect.right() - 10.0).abs() < 0.1,
        "inner gutter changed"
    );
    assert!(outer.rect.is_finite() && explorer.rect.is_finite());
    assert!(outer.offset.is_finite() && explorer.offset.is_finite());
    let controls: Vec<_> = TOOLS
        .iter()
        .map(|(_, label)| snapshot.widgets[*label].rect)
        .collect();
    for (index, control) in controls.iter().enumerate() {
        assert!(control.left() >= outer.rect.left() && control.right() <= outer.rect.right());
        assert!(control.bottom() < snapshot.widgets["help"].rect.top());
        for other in &controls[index + 1..] {
            assert!(
                !control.intersects(*other),
                "controls overlap: {control:?} / {other:?}"
            );
        }
    }
    assert!(
        explorer.rect.bottom() < controls[0].top(),
        "explorer overlaps selectors"
    );
    assert!(snapshot.widgets["Up"].rect.bottom() < explorer.rect.top());
}

#[test]
fn sidebar_footer_bounds_cover_narrow_default_wide_and_long_explorer() {
    for size in [[780.0, 540.0], [1178.0, 814.0], [1320.0, 880.0]] {
        for width in [180.0, 246.0, 460.0] {
            for long_directory in [false, true] {
                let (mut app, commands) = app(width, long_directory, 13.0);
                app.tools_open = false;
                settle(&mut app, size);
                assert_geometry(&app, width);
                let controls: Vec<_> = TOOLS
                    .iter()
                    .map(|(_, label)| widget(&app, label).rect)
                    .collect();
                if controls[0].top() != controls[1].top() {
                    for pair in controls.windows(2) {
                        assert!(pair[0].bottom() < pair[1].top());
                    }
                } else {
                    assert_eq!(controls[0].top(), controls[1].top());
                    assert_eq!(controls[1].top(), controls[2].top());
                    assert_eq!(controls[3].top(), controls[4].top());
                    assert!(controls[2].bottom() < controls[3].top());
                }
                assert_visible(&app, "help");
                assert!(
                    (widget(&app, "help").rect.bottom()
                        - snapshot(&app).outer.unwrap().rect.bottom())
                    .abs()
                        < 1.0,
                    "ordinary footer moved from bottom"
                );
                assert!(commands.try_recv().is_err());
            }
        }
    }
}

#[test]
fn sidebar_compact_actual_panel_order_keeps_controls_and_help_reachable() {
    for size in [[780.0, 540.0], [1178.0, 814.0], [1320.0, 880.0]] {
        for width in [180.0, 246.0, 460.0] {
            for font_size in [13.0, 18.0] {
                for long_directory in [false, true] {
                    let (mut app, commands) = app(width, long_directory, font_size);
                    load_long_report(&mut app);
                    settle(&mut app, size);
                    // 245 is the production request. Wrapped tools content may
                    // enlarge it; exercise that actual, tighter sidebar budget.
                    let tools = panel(&app, "tools");
                    assert!(
                        tools.height() >= 244.0 && tools.top() >= panel(&app, "header").bottom()
                    );
                    assert!(panel(&app, "explorer").bottom() <= panel(&app, "tools").top());
                    assert_geometry(&app, width);
                    let outer_id = snapshot(&app).outer.unwrap().id;
                    let inner_id = snapshot(&app).explorer.unwrap().id;
                    let selector_ids: Vec<_> = TOOLS
                        .iter()
                        .map(|(_, label)| widget(&app, label).id)
                        .collect();
                    for (_, label) in TOOLS {
                        focus(&mut app, size, label);
                        assert_geometry(&app, width);
                    }
                    scroll_outer_bottom(&mut app, size);
                    assert_geometry(&app, width);
                    assert_eq!(snapshot(&app).outer.unwrap().id, outer_id);
                    assert_eq!(snapshot(&app).explorer.unwrap().id, inner_id);
                    assert_eq!(
                        TOOLS
                            .iter()
                            .map(|(_, label)| widget(&app, label).id)
                            .collect::<Vec<_>>(),
                        selector_ids
                    );
                    // A focused selector must not keep snapping back during wheel scrolling.
                    let offset = snapshot(&app).outer.unwrap().offset;
                    settle(&mut app, size);
                    assert_eq!(snapshot(&app).outer.unwrap().offset, offset);
                    assert_visible(&app, "help");
                    focus(&mut app, size, "R");
                    focus(&mut app, size, "+");
                    assert!(commands.try_recv().is_err());
                }
            }
        }
    }
}

#[test]
fn sidebar_footer_scaling_and_larger_font_do_not_oscillate_gutters() {
    for width in [180.0, 246.0, 460.0] {
        for scale in [1.0, 1.5, 2.0] {
            for font_size in [13.0, 18.0] {
                let (mut app, commands) = app(width, true, font_size);
                let size = [780.0, 540.0];
                // The production tools panel settles to its content height after
                // its initial 245-point request. This is separate from gutters.
                for _ in 0..4 {
                    scaled_frame(&mut app, size, vec![], scale);
                }
                let mut previous = None;
                for _ in 0..20 {
                    scaled_frame(&mut app, size, vec![], scale);
                    assert_geometry(&app, width);
                    let now = (
                        snapshot(&app).outer.unwrap().rect,
                        widget(&app, "Search").rect,
                        widget(&app, "help").rect,
                    );
                    if let Some(previous) = previous {
                        assert_eq!(now, previous, "gutter or wrapping oscillated");
                    }
                    previous = Some(now);
                }
                assert!(commands.try_recv().is_err());
            }
        }
    }
}

#[test]
fn sidebar_tool_pointer_targets_open_and_keep_every_tool_selected() {
    for size in [[780.0, 540.0], [1178.0, 814.0]] {
        for width in [180.0, 246.0, 460.0] {
            for font_size in [13.0, 18.0] {
                let (mut app, commands) = app(width, true, font_size);
                settle(&mut app, size);
                for (tool, label) in TOOLS {
                    app.tools_open = false;
                    settle(&mut app, size);
                    focus(&mut app, size, label);
                    click(&mut app, size, label);
                    assert!(app.tools_open && app.tool == tool, "failed to open {label}");
                    if snapshot(&app).outer.is_none() {
                        // The large-font Language body can use the whole compact
                        // window. Closing/reopening recovers geometry; changing
                        // that tools-panel policy is outside this sidebar change.
                        assert!(size[1] == 540.0 && tool == Tool::Language);
                        assert!(panel(&app, "explorer").height() <= 24.0);
                        assert!(snapshot(&app).widgets.is_empty());
                        app.tools_open = false;
                        settle(&mut app, size);
                        focus(&mut app, size, label);
                    }
                    click(&mut app, size, label);
                    assert!(
                        app.tools_open && app.tool == tool,
                        "repeat click changed {label}"
                    );
                }
                assert!(commands.try_recv().is_err());
            }
        }
    }
}

#[test]
fn sidebar_tool_tab_shift_tab_and_enter_follow_visual_order() {
    // All tool bodies fit at this height. Compact loaded-Tests reachability is
    // checked separately; Language can consume all remaining compact height.
    let size = [1178.0, 814.0];
    for width in [180.0, 246.0, 460.0] {
        for font_size in [13.0, 18.0] {
            let (mut app, commands) = app(width, false, font_size);
            settle(&mut app, size);
            focus(&mut app, size, "+");
            // Flat and Tree are intentional mode controls before the native
            // inner scrollbar and the tool selectors. Assert their actual order.
            for label in ["Flat", "Tree"] {
                key(&mut app, size, egui::Key::Tab, egui::Modifiers::NONE);
                assert_eq!(
                    app.editor_ctx.memory(|memory| memory.focused()),
                    Some(widget(&app, label).id)
                );
                assert_visible(&app, label);
            }
            // Native egui scrollbars are also focusable Tab stops.
            for _ in 0..3 {
                key(&mut app, size, egui::Key::Tab, egui::Modifiers::NONE);
                if app.editor_ctx.memory(|memory| memory.focused())
                    == Some(widget(&app, "Search").id)
                {
                    break;
                }
            }
            for (index, (tool, label)) in TOOLS.into_iter().enumerate() {
                if index > 0 {
                    key(&mut app, size, egui::Key::Tab, egui::Modifiers::NONE);
                }
                assert_eq!(
                    app.editor_ctx.memory(|memory| memory.focused()),
                    Some(widget(&app, label).id),
                    "Tab missed {label}; widget ids {:?}",
                    snapshot(&app)
                        .widgets
                        .iter()
                        .map(|(label, w)| (label, w.id))
                        .collect::<Vec<_>>()
                );
                assert_visible(&app, label);
                key(&mut app, size, egui::Key::Enter, egui::Modifiers::NONE);
                assert!(app.tools_open && app.tool == tool, "Enter missed {label}");
            }
            for (_, label) in TOOLS[..4].iter().rev() {
                key(&mut app, size, egui::Key::Tab, egui::Modifiers::SHIFT);
                assert_eq!(
                    app.editor_ctx.memory(|memory| memory.focused()),
                    Some(widget(&app, label).id),
                    "Shift+Tab missed {label}"
                );
                assert_visible(&app, label);
            }
            assert!(commands.try_recv().is_err());
        }
    }
}

#[test]
fn sidebar_last_unicode_entry_reveals_both_scroll_levels_and_opens_existing_draft() {
    let size = [780.0, 540.0];
    let name = "最后一份 Unicode draft 文件.txt";
    for width in [180.0, 246.0, 460.0] {
        for font_size in [13.0, 18.0] {
            let (mut app, commands) = app(width, true, font_size);
            app.entries[63] = Entry {
                path: name.into(),
                name: name.into(),
                is_dir: false,
            };
            let mut document = Document::new(1, name.into(), "saved".into(), "r1".into());
            document.text = "precious unsaved draft 雪".into();
            app.documents.push(document);
            app.active_document = Some(1);
            settle(&mut app, size);
            focus(&mut app, size, "Search");
            scroll_outer_bottom(&mut app, size);
            assert!(snapshot(&app).outer.unwrap().offset.y > 0.0);
            // Shift+Tab passes the native explorer scrollbar before the final file.
            for _ in 0..3 {
                key(&mut app, size, egui::Key::Tab, egui::Modifiers::SHIFT);
                if app.editor_ctx.memory(|memory| memory.focused()) == Some(widget(&app, name).id) {
                    break;
                }
            }
            assert_eq!(
                app.editor_ctx.memory(|memory| memory.focused()),
                Some(widget(&app, name).id)
            );
            assert_visible(&app, name);
            let explorer = snapshot(&app).explorer.unwrap();
            assert!(explorer.offset.y > 0.0);
            assert!(explorer.rect.contains_rect(widget(&app, name).rect));
            assert!(explorer.rect.height() >= widget(&app, name).rect.height());
            key(&mut app, size, egui::Key::Enter, egui::Modifiers::NONE);
            assert_eq!(app.documents.len(), 1);
            assert_eq!(app.documents[0].text, "precious unsaved draft 雪");
            assert!(commands.try_recv().is_err());
        }
    }
}

#[test]
fn sidebar_resize_and_tools_reopen_preserve_focus_draft_selection_and_undo() {
    let (mut app, commands) = app(460.0, true, 18.0);
    app.documents.push(Document::new(
        1,
        "file-063.txt".into(),
        "saved 雪".into(),
        "r1".into(),
    ));
    app.active_document = Some(1);
    app.documents[0].jump_to = Some(7);
    let large = [1320.0, 880.0];
    settle(&mut app, large);
    frame(&mut app, large, vec![egui::Event::Text(" draft".into())]);
    let draft = app.documents[0].text.clone();
    assert_eq!(draft, "saved 雪 draft");
    let id = egui::Id::new(("editor", 1_u64));
    let selection =
        egui::text::CCursorRange::two(egui::text::CCursor::new(1), egui::text::CCursor::new(4));
    let mut state = egui::TextEdit::load_state(&app.editor_ctx, id).unwrap();
    state.cursor.set_char_range(Some(selection));
    state.store(&app.editor_ctx, id);
    focus(&mut app, large, "Tests");
    let focus_id = widget(&app, "Tests").id;
    let outer_id = snapshot(&app).outer.unwrap().id;
    let inner_id = snapshot(&app).explorer.unwrap().id;
    for (width, size, tools_open) in [
        (180.0, [780.0, 540.0], true),
        (246.0, large, true),
        (180.0, [780.0, 540.0], false),
        (180.0, [780.0, 540.0], true),
        (460.0, large, true),
    ] {
        set_width(&app, width);
        app.tools_open = tools_open;
        settle(&mut app, size);
        assert_eq!(widget(&app, "Tests").id, focus_id);
        assert_eq!(
            app.editor_ctx.memory(|memory| memory.focused()),
            Some(focus_id)
        );
        assert_visible(&app, "Tests");
        assert_geometry(&app, width);
        assert_eq!(snapshot(&app).outer.unwrap().id, outer_id);
        assert_eq!(snapshot(&app).explorer.unwrap().id, inner_id);
        assert_eq!(app.documents[0].text, draft);
        assert_eq!(app.documents[0].saved_text, "saved 雪");
        assert_eq!(
            egui::TextEdit::load_state(&app.editor_ctx, id)
                .unwrap()
                .cursor
                .char_range(),
            Some(selection)
        );
    }
    app.editor_ctx.memory_mut(|memory| memory.request_focus(id));
    key(&mut app, large, egui::Key::Z, egui::Modifiers::COMMAND);
    assert_eq!(app.documents[0].text, "saved 雪");
    key(
        &mut app,
        large,
        egui::Key::Z,
        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
    );
    assert_eq!(app.documents[0].text, draft);
    assert!(app.documents[0].dirty());
    assert!(commands.try_recv().is_err());
}

#[test]
fn sidebar_zero_height_is_finite_and_recovers_without_claiming_access() {
    let (mut app, commands) = app(180.0, true, 18.0);
    settle(&mut app, [780.0, 280.0]);
    let snapshot = snapshot(&app);
    if let Some(outer) = snapshot.outer {
        for scroll in [outer, snapshot.explorer.unwrap()] {
            assert!(
                scroll.rect.is_finite() && scroll.content.is_finite() && scroll.offset.is_finite()
            );
        }
    } else {
        assert!(snapshot.widgets.is_empty() && snapshot.explorer.is_none());
        assert!(panel(&app, "explorer").height() <= 24.0);
    }
    settle(&mut app, [780.0, 540.0]);
    focus(&mut app, [780.0, 540.0], "Tests");
    scroll_outer_bottom(&mut app, [780.0, 540.0]);
    assert!(commands.try_recv().is_err());
}

#[test]
fn sidebar_compact_tab_order_survives_a_visible_notification() {
    let size = [780.0, 540.0];
    for width in [180.0, 246.0, 460.0] {
        for font_size in [13.0, 18.0] {
            let (mut app, commands) = app(width, false, font_size);
            load_long_report(&mut app);
            app.error =
                Some("A previous read failed. The current draft is still available.".into());
            settle(&mut app, size);
            focus(&mut app, size, "Search");
            for (_, label) in &TOOLS[1..] {
                key(&mut app, size, egui::Key::Tab, egui::Modifiers::NONE);
                assert_eq!(
                    app.editor_ctx.memory(|memory| memory.focused()),
                    Some(widget(&app, label).id)
                );
                assert_visible(&app, label);
            }
            key(&mut app, size, egui::Key::Enter, egui::Modifiers::NONE);
            assert!(app.tool == Tool::Tests && app.tools_open);
            for (_, label) in TOOLS[..4].iter().rev() {
                key(&mut app, size, egui::Key::Tab, egui::Modifiers::SHIFT);
                assert_eq!(
                    app.editor_ctx.memory(|memory| memory.focused()),
                    Some(widget(&app, label).id)
                );
                assert_visible(&app, label);
            }
            scroll_outer_bottom(&mut app, size);
            assert_geometry(&app, width);
            assert!(commands.try_recv().is_err());
        }
    }
}

#[test]
fn sidebar_inner_wheel_hands_off_at_boundary_and_outer_thumb_reaches_help() {
    let size = [780.0, 540.0];
    let (mut app, commands) = app(180.0, true, 13.0);
    load_long_report(&mut app);
    settle(&mut app, size);
    focus(&mut app, size, "file-000.txt");
    let explorer = snapshot(&app).explorer.unwrap();
    wheel(&mut app, size, explorer.rect.center(), -10_000.0);
    assert!(snapshot(&app).explorer.unwrap().offset.y > 0.0);
    let focused = app.editor_ctx.memory(|memory| memory.focused());
    // Once the inner area reaches its boundary, continued wheel input can
    // scroll the containing sidebar. The gained-focus correction has ended.
    let at = snapshot(&app)
        .explorer
        .unwrap()
        .rect
        .intersect(snapshot(&app).outer.unwrap().rect)
        .center();
    wheel(&mut app, size, at, -10_000.0);
    assert!(snapshot(&app).outer.unwrap().offset.y > 0.0);
    assert_eq!(app.editor_ctx.memory(|memory| memory.focused()), focused);
    assert_visible(&app, "help");

    focus(&mut app, size, "R");
    let outer = snapshot(&app).outer.unwrap();
    assert!(outer.content.y > outer.rect.height());
    let handle_height = outer.rect.height() * outer.rect.height() / outer.content.y;
    let start = egui::pos2(
        outer.rect.right() + 5.0,
        outer.rect.top() + handle_height / 2.0,
    );
    let end = egui::pos2(start.x, outer.rect.bottom());
    frame(
        &mut app,
        size,
        vec![
            egui::Event::PointerMoved(start),
            egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    frame(&mut app, size, vec![egui::Event::PointerMoved(end)]);
    frame(
        &mut app,
        size,
        vec![egui::Event::PointerButton {
            pos: end,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    settle(&mut app, size);
    assert_visible(&app, "help");
    assert_geometry(&app, 180.0);
    assert!(commands.try_recv().is_err());
}

#[test]
fn sidebar_short_file_reveals_when_only_part_of_inner_viewport_fits() {
    let size = [780.0, 540.0];
    let (mut app, commands) = app(180.0, true, 13.0);
    app.tool = Tool::Language;
    settle(&mut app, size);
    let outer = snapshot(&app).outer.unwrap();
    let file_height = widget(&app, "file-063.txt").rect.height();
    assert!(outer.rect.height() >= file_height && outer.rect.height() < 64.0);
    focus(&mut app, size, "file-063.txt");
    assert!(snapshot(&app).explorer.unwrap().offset.y > 0.0);
    let outer = snapshot(&app).outer.unwrap();
    wheel(
        &mut app,
        size,
        egui::pos2(outer.rect.right() + 5.0, outer.rect.center().y),
        -10_000.0,
    );
    let offset = snapshot(&app).outer.unwrap().offset;
    settle(&mut app, size);
    assert_eq!(snapshot(&app).outer.unwrap().offset, offset);
    assert!(commands.try_recv().is_err());
}

#[test]
fn sidebar_held_outer_thumb_recovers_after_zero_height() {
    let size = [780.0, 540.0];
    let tiny = [780.0, 180.0];
    let (mut app, commands) = app(180.0, true, 13.0);
    load_long_report(&mut app);
    settle(&mut app, size);
    let outer = snapshot(&app).outer.unwrap();
    let handle_height = outer.rect.height() * outer.rect.height() / outer.content.y;
    let start = egui::pos2(
        outer.rect.right() + 5.0,
        outer.rect.top() + handle_height / 2.0,
    );
    let moved = start + egui::vec2(0.0, 15.0);
    frame(
        &mut app,
        size,
        vec![
            egui::Event::PointerMoved(start),
            egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    frame(&mut app, size, vec![egui::Event::PointerMoved(moved)]);
    assert!(app
        .editor_ctx
        .read_response(outer.id.with(1_usize))
        .unwrap()
        .dragged());
    let held_offset = snapshot(&app).outer.unwrap().offset;
    frame(&mut app, tiny, vec![]);
    let skipped = snapshot(&app);
    assert!(skipped.outer.is_none() && skipped.explorer.is_none() && skipped.widgets.is_empty());
    assert!(panel(&app, "explorer").height() <= 24.0);
    assert_eq!(
        egui::scroll_area::State::load(&app.editor_ctx, outer.id)
            .unwrap()
            .offset,
        held_offset
    );
    frame(
        &mut app,
        tiny,
        vec![egui::Event::PointerButton {
            pos: moved,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    settle(&mut app, size);
    assert_eq!(snapshot(&app).outer.unwrap().id, outer.id);
    assert_geometry(&app, 180.0);
    focus(&mut app, size, "Tests");
    scroll_outer_bottom(&mut app, size);
    assert!(commands.try_recv().is_err());
}

fn click_tests_after_tall_language(app: &mut CedarApp, focused: bool) -> egui::Id {
    let size = [780.0, 540.0];
    app.tool = Tool::Language;
    settle(app, size);
    assert!(snapshot(app).outer.is_none());
    app.tools_open = false;
    settle(app, size);
    if focused {
        focus(app, size, "Tests");
    }
    let target = widget(app, "Tests");
    assert_visible(app, "Tests");
    let at = target.rect.center();
    for pressed in [true, false] {
        frame(
            app,
            size,
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    assert!(app.tools_open && app.tool == Tool::Tests);
    target.id
}

#[test]
fn sidebar_clicked_selector_survives_temporary_zero_space_without_inventing_focus() {
    let size = [780.0, 540.0];
    for had_focus in [false, true] {
        let (mut app, commands) = app(180.0, true, 18.0);
        let target = click_tests_after_tall_language(&mut app, had_focus);
        assert_eq!(
            app.editor_ctx.memory(|memory| memory.focused()),
            had_focus.then_some(target)
        );
        frame(&mut app, size, vec![]);
        assert!(snapshot(&app).outer.is_none());
        settle(&mut app, size);
        assert_visible(&app, "Tests");
        assert_eq!(
            app.editor_ctx.memory(|memory| memory.focused()),
            had_focus.then_some(target)
        );
        // The one pending activation is consumed, so later wheel input stays put.
        scroll_outer_bottom(&mut app, size);
        let offset = snapshot(&app).outer.unwrap().offset;
        settle(&mut app, size);
        assert_eq!(snapshot(&app).outer.unwrap().offset, offset);
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn sidebar_new_intent_during_zero_space_cancels_pending_activation() {
    let size = [780.0, 540.0];
    for intent in [
        "escape",
        "tab",
        "pointer",
        "wheel",
        "zoom",
        "text",
        "ime",
        "new_owner",
        "closed",
        "changed_tool",
    ] {
        let (mut app, commands) = app(180.0, true, 18.0);
        let target = click_tests_after_tall_language(&mut app, true);
        let events = match intent {
            "escape" | "tab" => vec![egui::Event::Key {
                key: if intent == "escape" {
                    egui::Key::Escape
                } else {
                    egui::Key::Tab
                },
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            "pointer" => vec![
                egui::Event::PointerMoved(egui::pos2(740.0, 20.0)),
                egui::Event::PointerButton {
                    pos: egui::pos2(740.0, 20.0),
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            "wheel" => vec![egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -10.0),
                modifiers: egui::Modifiers::NONE,
            }],
            "zoom" => vec![egui::Event::Zoom(1.1)],
            "text" => vec![egui::Event::Text("new intent".into())],
            "ime" => vec![egui::Event::Ime(egui::ImeEvent::Commit("新".into()))],
            "new_owner" => {
                app.editor_ctx.data_mut(|data| {
                    data.insert_temp(
                        egui::Id::new("sidebar_test_request_focus"),
                        egui::Id::new("test_report_path"),
                    )
                });
                vec![]
            }
            "closed" => {
                app.tools_open = false;
                vec![]
            }
            "changed_tool" => {
                app.tool = Tool::Run;
                vec![]
            }
            _ => unreachable!(),
        };
        frame(&mut app, size, events);
        if !matches!(intent, "closed" | "changed_tool" | "zoom") {
            assert!(
                snapshot(&app).outer.is_none(),
                "expected zero-space interruption for {intent}"
            );
        }
        if intent == "pointer" {
            frame(
                &mut app,
                size,
                vec![egui::Event::PointerButton {
                    pos: egui::pos2(740.0, 20.0),
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
        }
        settle(&mut app, size);
        if !matches!(intent, "closed" | "changed_tool") {
            assert_ne!(
                app.editor_ctx.memory(|memory| memory.focused()),
                Some(target),
                "old focus restored after {intent}"
            );
        }
        if intent == "new_owner" {
            assert_eq!(
                app.editor_ctx.memory(|memory| memory.focused()),
                Some(egui::Id::new("test_report_path"))
            );
        }
        assert!(
            commands.try_recv().is_err(),
            "unexpected command after {intent}"
        );
    }
}
