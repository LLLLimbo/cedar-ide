//! Bounds and interaction regressions for the production sidebar footer.
use super::*;

const HELP: &str = "Ctrl/Cmd+P  Choose a file\nCtrl/Cmd+G  Go to line\nCtrl/Cmd+F  Find in file\nCtrl/Cmd+S  Save changes";
const TOOLS: [(Tool, &str); 5] = [
    (Tool::Search, "Search"),
    (Tool::Git, "Git"),
    (Tool::Run, "Run"),
    (Tool::Language, "LSP"),
    (Tool::Tests, "Tests"),
];

fn app(width: f32, long_directory: bool) -> (CedarApp, Receiver<Command>) {
    let mut app = CedarApp::empty();
    app.state = ConnectionState::Ready;
    app.open_form = false;
    // Match the native metrics in CedarApp::new: default egui padding does not
    // reproduce the wrapped Tests control seen in the native window.
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
    app.editor_ctx.data_mut(|data| {
        data.insert_persisted(
            egui::Id::new("explorer"),
            egui::containers::panel::PanelState {
                rect: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 540.0)),
            },
        );
    });
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

fn frame(app: &mut CedarApp, size: [f32; 2], events: Vec<egui::Event>) -> egui::FullOutput {
    scaled_frame(app, size, events, 1.0)
}

fn scaled_frame(
    app: &mut CedarApp,
    size: [f32; 2],
    events: Vec<egui::Event>,
    scale: f32,
) -> egui::FullOutput {
    render_frame(app, size, events, scale, false)
}

fn render_frame(
    app: &mut CedarApp,
    size: [f32; 2],
    events: Vec<egui::Event>,
    scale: f32,
    show_tools: bool,
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
        app.header(ctx);
        app.footer(ctx);
        if show_tools {
            app.tools(ctx);
        }
        app.sidebar(ctx);
        egui::CentralPanel::default().show(ctx, |_ui| {});
    })
}

fn text_rect(output: &egui::FullOutput, label: &str) -> (egui::Rect, egui::Rect) {
    fn find(shape: &egui::epaint::Shape, label: &str) -> Option<egui::Rect> {
        match shape {
            egui::epaint::Shape::Text(text) if text.galley.job.text == label => {
                Some(text.galley.rect.translate(text.pos.to_vec2()))
            }
            egui::epaint::Shape::Vec(shapes) => shapes.iter().find_map(|shape| find(shape, label)),
            _ => None,
        }
    }
    output
        .shapes
        .iter()
        .find_map(|shape| find(&shape.shape, label).map(|rect| (rect, shape.clip_rect)))
        .unwrap_or_else(|| panic!("missing sidebar text {label:?}"))
}

fn selected_rect(output: &egui::FullOutput, label: &str, fill: Color32) -> egui::Rect {
    let at = text_rect(output, label).0.center();
    fn find(shape: &egui::epaint::Shape, at: egui::Pos2, fill: Color32) -> Option<egui::Rect> {
        match shape {
            egui::epaint::Shape::Rect(rect) if rect.fill == fill && rect.rect.contains(at) => {
                Some(rect.rect)
            }
            egui::epaint::Shape::Vec(shapes) => {
                shapes.iter().find_map(|shape| find(shape, at, fill))
            }
            _ => None,
        }
    }
    output
        .shapes
        .iter()
        .find_map(|shape| find(&shape.shape, at, fill))
        .unwrap_or_else(|| panic!("missing selected control bounds for {label}"))
}

fn click(app: &mut CedarApp, size: [f32; 2], label: &str) {
    let at = text_rect(&frame(app, size, vec![egui::Event::PointerGone]), label)
        .0
        .center();
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
}

fn key(app: &mut CedarApp, size: [f32; 2], key: egui::Key) {
    for pressed in [true, false] {
        frame(
            app,
            size,
            vec![egui::Event::Key {
                key,
                physical_key: Some(key),
                pressed,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
    }
}

#[test]
fn sidebar_footer_bounds_cover_narrow_default_wide_and_long_explorer() {
    for size in [[780.0, 540.0], [1178.0, 814.0], [1320.0, 880.0]] {
        for width in [180.0, 246.0, 460.0] {
            for long_directory in [false, true] {
                let (mut app, commands) = app(width, long_directory);
                let controls = assert_footer_bounds(&mut app, size, width, long_directory, 1.0);
                assert_eq!(controls[0].top(), controls[1].top());
                assert_eq!(controls[1].top(), controls[2].top());
                assert_eq!(controls[3].top(), controls[4].top());
                assert!(controls[2].bottom() < controls[3].top());
                assert!(commands.try_recv().is_err());
            }
        }
    }
}

fn assert_footer_bounds(
    app: &mut CedarApp,
    size: [f32; 2],
    width: f32,
    long_directory: bool,
    scale: f32,
) -> Vec<egui::Rect> {
    let mut controls = Vec::new();
    for (tool, label) in TOOLS {
        app.tools_open = true;
        app.tool = tool;
        let output = scaled_frame(app, size, vec![egui::Event::PointerGone], scale);
        let fill = app.editor_ctx.style().visuals.selection.bg_fill;
        controls.push(selected_rect(&output, label, fill));
    }
    let output = scaled_frame(app, size, vec![], scale);
    let panel =
        egui::containers::panel::PanelState::load(&app.editor_ctx, egui::Id::new("explorer"))
            .unwrap()
            .rect;
    let inner = panel.shrink(12.0);
    assert!(
        (panel.width() - width).abs() < 0.1,
        "sidebar expanded past {width}: {panel:?}"
    );
    let (help, help_clip) = text_rect(&output, HELP);
    assert!(
        inner.contains_rect(help),
        "help outside {inner:?}: {help:?}"
    );
    assert!(
        help_clip.contains_rect(help),
        "help clipped: {help:?} / {help_clip:?}"
    );
    for (index, control) in controls.iter().enumerate() {
        assert!(
            inner.contains_rect(*control),
            "{} outside {inner:?}: {control:?}",
            TOOLS[index].1
        );
        assert!(
            control.bottom() < help.top(),
            "{} overlaps help",
            TOOLS[index].1
        );
        for other in &controls[index + 1..] {
            assert!(
                !control.intersects(*other),
                "controls overlap: {control:?} / {other:?}"
            );
        }
    }
    if long_directory {
        let (_, explorer_clip) = text_rect(&output, "·  file-000.txt");
        assert!(
            explorer_clip.bottom() < controls[0].top(),
            "long explorer overlaps tools: {explorer_clip:?} / {:?}",
            controls[0]
        );
    }
    controls
}

#[test]
fn sidebar_footer_scaling_and_larger_font_keep_every_control_in_bounds() {
    for size in [[780.0, 540.0], [1178.0, 814.0]] {
        for width in [180.0, 246.0, 460.0] {
            for scale in [1.0, 1.5, 2.0] {
                for font_size in [13.0, 18.0] {
                    let (mut app, commands) = app(width, true);
                    app.editor_ctx.style_mut(|style| {
                        style
                            .text_styles
                            .insert(egui::TextStyle::Button, FontId::proportional(font_size));
                    });
                    let controls = assert_footer_bounds(&mut app, size, width, true, scale);
                    if width == 180.0 && font_size == 18.0 {
                        // At this size, even zero gaps cannot fit Search/Git/Run.
                        for pair in controls.windows(2) {
                            assert!(
                                pair[0].bottom() < pair[1].top(),
                                "large-font fallback is not stacked"
                            );
                        }
                    }
                    assert!(commands.try_recv().is_err());
                }
            }
        }
    }
}

#[test]
fn sidebar_tool_pointer_targets_open_and_keep_every_tool_selected() {
    let size = [1178.0, 814.0];
    for width in [180.0, 246.0, 460.0] {
        for font_size in [13.0, 18.0] {
            let (mut app, commands) = app(width, true);
            app.editor_ctx.style_mut(|style| {
                style
                    .text_styles
                    .insert(egui::TextStyle::Button, FontId::proportional(font_size));
            });
            for (tool, label) in TOOLS {
                app.tools_open = false;
                click(&mut app, size, label);
                assert!(
                    app.tools_open && app.tool == tool,
                    "failed to open {label} at {width}"
                );
                click(&mut app, size, label);
                assert!(
                    app.tools_open && app.tool == tool,
                    "repeat click changed {label} at {width}"
                );
            }
            assert!(commands.try_recv().is_err());
        }
    }
}

#[test]
fn sidebar_tool_tab_order_remains_search_git_run_lsp_tests() {
    let size = [780.0, 540.0];
    for width in [180.0, 246.0, 460.0] {
        for font_size in [13.0, 18.0] {
            let (mut app, commands) = app(width, false);
            app.editor_ctx.style_mut(|style| {
                style
                    .text_styles
                    .insert(egui::TextStyle::Button, FontId::proportional(font_size));
            });
            let output = frame(&mut app, size, vec![]);
            let search_at = text_rect(&output, "Search").0.center();
            let mut found_search = false;
            for _ in 0..20 {
                key(&mut app, size, egui::Key::Tab);
                let focused = app.editor_ctx.memory(|memory| memory.focused());
                if focused
                    .and_then(|id| app.editor_ctx.read_response(id))
                    .is_some_and(|response| response.rect.contains(search_at))
                {
                    found_search = true;
                    break;
                }
            }
            assert!(found_search, "Search is not keyboard reachable at {width}");
            for (index, (tool, label)) in TOOLS.into_iter().enumerate() {
                if index > 0 {
                    key(&mut app, size, egui::Key::Tab);
                }
                key(&mut app, size, egui::Key::Enter);
                assert!(
                    app.tools_open && app.tool == tool,
                    "keyboard order missed {label} at {width}"
                );
            }
            assert!(commands.try_recv().is_err());
        }
    }
}

#[test]
fn sidebar_footer_full_layout_with_tests_open_at_observed_and_default_size() {
    for size in [[1178.0, 814.0], [1320.0, 880.0]] {
        for width in [180.0, 246.0, 460.0] {
            let (mut app, commands) = app(width, true);
            app.tool = Tool::Tests;
            app.tools_open = true;
            for _ in 0..3 {
                let output = render_frame(&mut app, size, vec![], 1.0, true);
                let sidebar = egui::containers::panel::PanelState::load(
                    &app.editor_ctx,
                    egui::Id::new("explorer"),
                )
                .unwrap()
                .rect;
                let tools = egui::containers::panel::PanelState::load(
                    &app.editor_ctx,
                    egui::Id::new("tools"),
                )
                .unwrap()
                .rect;
                assert!(sidebar.bottom() <= tools.top());
                let (help, clip) = text_rect(&output, HELP);
                assert!(sidebar.contains_rect(help) && clip.contains_rect(help));
                for (_, label) in TOOLS {
                    let (text, clip) = text_rect(&output, label);
                    assert!(
                        sidebar.contains_rect(text) && clip.contains_rect(text),
                        "{label} clipped with Tests open at {size:?}/{width}"
                    );
                    assert!(text.bottom() < help.top());
                }
                let tests = selected_rect(
                    &output,
                    "Tests",
                    app.editor_ctx.style().visuals.selection.bg_fill,
                );
                assert!(sidebar.shrink(12.0).contains_rect(tests));
                let (_, explorer_clip) = text_rect(&output, "·  file-000.txt");
                assert!(explorer_clip.bottom() < text_rect(&output, "Search").0.top());
            }
            assert!(commands.try_recv().is_err());
        }
    }
}

#[test]
#[ignore = "diagnostic for the pre-existing minimum-height constraint with the tools panel open"]
fn sidebar_minimum_height_with_tests_open_reports_actual_bounds() {
    for width in [180.0, 246.0, 460.0] {
        let (mut app, _) = app(width, true);
        app.tool = Tool::Tests;
        app.tools_open = true;
        let mut output = render_frame(&mut app, [780.0, 540.0], vec![], 1.0, true);
        for _ in 0..2 {
            output = render_frame(&mut app, [780.0, 540.0], vec![], 1.0, true);
        }
        let sidebar =
            egui::containers::panel::PanelState::load(&app.editor_ctx, egui::Id::new("explorer"))
                .unwrap()
                .rect;
        let (help, help_clip) = text_rect(&output, HELP);
        let (search, _) = text_rect(&output, "Search");
        let (tests, _) = text_rect(&output, "Tests");
        let (_, explorer_clip) = text_rect(&output, "·  file-000.txt");
        eprintln!("minimum screen, sidebar {width}: sidebar={sidebar:?}, Search={search:?}, Tests={tests:?}, help={help:?}, help_clip={help_clip:?}, explorer_clip={explorer_clip:?}");
        assert!(sidebar.is_finite() && help.is_finite() && search.is_finite() && tests.is_finite());
    }
}
