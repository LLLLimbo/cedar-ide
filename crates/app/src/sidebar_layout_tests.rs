//! Geometry and interaction checks exercise the production sidebar, including
//! widgets outside the paint clip. Test-only probes do not affect widget IDs.
use super::*;

const PROBE: &str = "sidebar_layout_test_probe";
const REVEAL_DIAGNOSTIC: &str = "sidebar_reveal_diagnostic";
// The selected matrix case has eight existing frames and egui allows two
// layout passes by default. Observe at most those sixteen passes; never retry.
const MAX_DIAGNOSTIC_PASSES: usize = 16;

#[derive(Clone, Copy, serde::Serialize)]
enum DiagnosticStage {
    InitialLayout,
    InitialSearchFocus,
}

#[derive(Clone, serde::Serialize)]
struct RevealDiagnostic {
    requested: bool,
    activated: bool,
    resized: bool,
    gained_focus: bool,
    has_focus: bool,
    range: [f32; 4],
}

#[derive(Clone, serde::Serialize)]
struct ScrollDiagnostic {
    viewport: [f32; 4],
    content: [f32; 2],
    offset_before_layout: Option<[f32; 2]>,
    offset_after_layout: [f32; 2],
}

#[derive(Clone, serde::Serialize)]
struct WidgetDiagnostic {
    rect: [f32; 4],
    paint_clip: [f32; 4],
    reveal: Option<RevealDiagnostic>,
}

#[derive(Clone, serde::Serialize)]
struct PassDiagnostic {
    stage: DiagnosticStage,
    frame: usize,
    pass_in_frame: usize,
    cumulative_pass: u64,
    native_pixels_per_point: Option<f32>,
    pixels_per_point: f32,
    gui_rounding: f32,
    clip_rect_margin: f32,
    tools_rect: Option<[f32; 4]>,
    outer: Option<ScrollDiagnostic>,
    search: Option<WidgetDiagnostic>,
}

#[derive(Clone, serde::Serialize)]
struct RevealDiagnostics {
    width: f32,
    font_size: f32,
    window: [f32; 2],
    passes: Vec<PassDiagnostic>,
    truncated: bool,
    #[serde(skip)]
    stage: DiagnosticStage,
    #[serde(skip)]
    next_frame: usize,
}

fn rect_coordinates(rect: egui::Rect) -> [f32; 4] {
    [rect.left(), rect.top(), rect.right(), rect.bottom()]
}

fn begin_reveal_diagnostics(app: &CedarApp, width: f32, font_size: f32, window: [f32; 2]) {
    app.editor_ctx.data_mut(|data| {
        data.insert_temp(
            egui::Id::new(REVEAL_DIAGNOSTIC),
            RevealDiagnostics {
                width,
                font_size,
                window,
                passes: Vec::with_capacity(MAX_DIAGNOSTIC_PASSES),
                truncated: false,
                stage: DiagnosticStage::InitialLayout,
                next_frame: 0,
            },
        );
    });
}

fn search_focus_diagnostic_stage(app: &CedarApp) {
    app.editor_ctx.data_mut(|data| {
        if let Some(mut trace) =
            data.get_temp::<RevealDiagnostics>(egui::Id::new(REVEAL_DIAGNOSTIC))
        {
            trace.stage = DiagnosticStage::InitialSearchFocus;
            trace.next_frame = 0;
            data.insert_temp(egui::Id::new(REVEAL_DIAGNOSTIC), trace);
        }
    });
}

fn diagnostic_frame(ctx: &egui::Context) -> Option<usize> {
    ctx.data_mut(|data| {
        let mut trace = data.get_temp::<RevealDiagnostics>(egui::Id::new(REVEAL_DIAGNOSTIC))?;
        let frame = trace.next_frame;
        trace.next_frame += 1;
        data.insert_temp(egui::Id::new(REVEAL_DIAGNOSTIC), trace);
        Some(frame)
    })
}

fn diagnostic_outer_offset(ctx: &egui::Context, frame: Option<usize>) -> Option<[f32; 2]> {
    frame?;
    let previous = ctx.data(|data| data.get_temp::<Snapshot>(egui::Id::new(PROBE)))?;
    let state = egui::scroll_area::State::load(ctx, previous.outer?.id)?;
    Some([state.offset.x, state.offset.y])
}

// Observe the unchanged reveal predicate before it runs. This allocates no UI,
// consumes no input, and never requests focus, scrolling, repaint, or a pass.
pub(super) fn record_reveal(response: &egui::Response, activated: bool, resized: bool) {
    if !response.ctx.data(|data| {
        data.get_temp::<RevealDiagnostics>(egui::Id::new(REVEAL_DIAGNOSTIC))
            .is_some_and(|trace| trace.passes.len() < MAX_DIAGNOSTIC_PASSES)
    }) {
        return;
    }
    let gained_focus = response.gained_focus();
    let has_focus = response.has_focus();
    let reveal = RevealDiagnostic {
        requested: activated || gained_focus || (resized && has_focus),
        activated,
        resized,
        gained_focus,
        has_focus,
        range: rect_coordinates(response.rect),
    };
    response.ctx.data_mut(|data| {
        data.get_temp_mut_or_default::<Snapshot>(egui::Id::new(PROBE))
            .reveals
            .insert(response.id, reveal);
    });
}

fn record_diagnostic_pass(
    ctx: &egui::Context,
    frame: Option<usize>,
    pass_in_frame: usize,
    offset_before_layout: Option<[f32; 2]>,
) {
    let Some(frame) = frame else { return };
    let Some(mut trace) =
        ctx.data(|data| data.get_temp::<RevealDiagnostics>(egui::Id::new(REVEAL_DIAGNOSTIC)))
    else {
        return;
    };
    if trace.passes.len() == MAX_DIAGNOSTIC_PASSES {
        trace.truncated = true;
    } else {
        let snapshot = ctx.data(|data| data.get_temp::<Snapshot>(egui::Id::new(PROBE)));
        let outer = snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.outer.as_ref());
        let search = snapshot.as_ref().and_then(|snapshot| {
            snapshot.widgets.get("help").map(|widget| WidgetDiagnostic {
                rect: rect_coordinates(widget.rect),
                paint_clip: rect_coordinates(widget.clip),
                reveal: snapshot.reveals.get(&widget.id).cloned(),
            })
        });
        trace.passes.push(PassDiagnostic {
            stage: trace.stage,
            frame,
            pass_in_frame,
            cumulative_pass: ctx.cumulative_pass_nr(),
            native_pixels_per_point: ctx.native_pixels_per_point(),
            pixels_per_point: ctx.pixels_per_point(),
            gui_rounding: egui::emath::GUI_ROUNDING,
            clip_rect_margin: ctx.style().visuals.clip_rect_margin,
            tools_rect: egui::containers::panel::PanelState::load(ctx, egui::Id::new("tools"))
                .map(|state| rect_coordinates(state.rect)),
            outer: outer.map(|outer| ScrollDiagnostic {
                viewport: rect_coordinates(outer.rect),
                content: [outer.content.x, outer.content.y],
                offset_before_layout,
                offset_after_layout: [outer.offset.x, outer.offset.y],
            }),
            search,
        });
    }
    ctx.data_mut(|data| data.insert_temp(egui::Id::new(REVEAL_DIAGNOSTIC), trace));
}

fn print_reveal_diagnostics(ctx: &egui::Context) {
    if let Some(trace) =
        ctx.data(|data| data.get_temp::<RevealDiagnostics>(egui::Id::new(REVEAL_DIAGNOSTIC)))
    {
        // Numeric arrays preserve f32 precision; egui Rect Debug rounds to one
        // decimal. Only fixed stage names, geometry, and booleans are emitted.
        if let Ok(encoded) = serde_json::to_string(&trace) {
            eprintln!("sidebar_reveal_diagnostic {encoded}");
        }
    }
}

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
    reveals: HashMap<egui::Id, RevealDiagnostic>,
    footer_geometry: Option<FooterGeometry>,
}

const CHECK_FOOTER: &str = "sidebar_check_exact_footer";

#[derive(Clone, Copy, Debug)]
struct FooterGeometry {
    initial_content_origin_y: f32,
    pre_footer_content_min_y: f32,
    final_content_min_y: f32,
    viewport: egui::Rect,
    offset_y: f32,
    spacing_y: f32,
}

#[derive(Clone, Debug)]
struct FooterRequest {
    id: egui::Id,
    row: egui::Rect,
    target: Option<egui::Rect>,
}

#[derive(Clone, Default)]
struct FooterChecks {
    passes: usize,
    requests: Vec<FooterRequest>,
}

// Opt-in observations assert every layout pass of only the new regressions.
// They allocate no widgets and do not request focus, scrolling, or extra frames.
pub(super) fn record_footer_target(
    response: &egui::Response,
    requested: bool,
    target: Option<egui::Rect>,
) {
    response.ctx.data_mut(|data| {
        if let Some(mut checks) = data.get_temp::<FooterChecks>(egui::Id::new(CHECK_FOOTER)) {
            if requested {
                assert!(
                    checks.requests.len() < 128,
                    "unexpected repeated footer requests"
                );
                checks.requests.push(FooterRequest {
                    id: response.id,
                    row: response.rect,
                    target,
                });
            } else {
                assert!(target.is_none(), "passive footer emitted a correction");
            }
            data.insert_temp(egui::Id::new(CHECK_FOOTER), checks);
        }
    });
}

pub(super) fn record_footer_geometry(
    ui: &egui::Ui,
    initial_content_origin_y: f32,
    pre_footer_content_min_y: f32,
    actual_viewport: egui::Rect,
    viewport_offset_y: f32,
    outer_item_spacing_y: f32,
) {
    let geometry = FooterGeometry {
        initial_content_origin_y,
        pre_footer_content_min_y,
        final_content_min_y: ui.min_rect().top(),
        viewport: actual_viewport,
        offset_y: viewport_offset_y,
        spacing_y: outer_item_spacing_y,
    };
    ui.ctx().data_mut(|data| {
        if let Some(mut checks) = data.get_temp::<FooterChecks>(egui::Id::new(CHECK_FOOTER)) {
            assert_eq!(
                geometry.pre_footer_content_min_y, geometry.final_content_min_y,
                "footer layout changed the minimum used by the scroll solver: {geometry:?}"
            );
            checks.passes += 1;
            data.insert_temp(egui::Id::new(CHECK_FOOTER), checks);
        }
        data.get_temp_mut_or_default::<Snapshot>(egui::Id::new(PROBE))
            .footer_geometry = Some(geometry);
    });
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
    let diagnostic_frame = diagnostic_frame(&ctx);
    let mut diagnostic_pass = 0;
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
        let offset_before_layout = diagnostic_outer_offset(ctx, diagnostic_frame);
        app.sidebar(ctx);
        record_diagnostic_pass(ctx, diagnostic_frame, diagnostic_pass, offset_before_layout);
        diagnostic_pass += 1;
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
    if !outer.rect.contains_rect(widget.rect) || !widget.clip.contains_rect(widget.rect) {
        print_reveal_diagnostics(&app.editor_ctx);
    }
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
        (sidebar.shrink(8.0).right() - outer.rect.right() - 10.0).abs() < 0.1,
        "outer gutter changed"
    );
    assert!(
        (outer.rect.right() - explorer.rect.right() - 10.0).abs() < 0.1,
        "inner gutter changed"
    );
    assert!(outer.rect.is_finite() && explorer.rect.is_finite());
    assert!(outer.offset.is_finite() && explorer.offset.is_finite());
    let help = snapshot.widgets["help"].rect;
    assert!(help.left() >= outer.rect.left() && help.right() <= outer.rect.right());
    assert!(
        explorer.rect.bottom() < help.top(),
        "Project rows overlap help"
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
                    assert!(panel(&app, "tools").top() >= panel(&app, "header").bottom());
                    assert!(panel(&app, "explorer").bottom() <= panel(&app, "tools").top());
                    assert_geometry(&app, width);
                    let outer_id = snapshot(&app).outer.unwrap().id;
                    let inner_id = snapshot(&app).explorer.unwrap().id;
                    let help_id = widget(&app, "help").id;
                    focus(&mut app, size, "help");
                    scroll_outer_bottom(&mut app, size);
                    assert_geometry(&app, width);
                    assert_eq!(snapshot(&app).outer.unwrap().id, outer_id);
                    assert_eq!(snapshot(&app).explorer.unwrap().id, inner_id);
                    assert_eq!(widget(&app, "help").id, help_id);
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
                        widget(&app, "help").rect,
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
fn sidebar_last_unicode_entry_reveals_both_scroll_levels_and_opens_existing_draft() {
    let size = [780.0, 540.0];
    let name = "最后一份 Unicode draft 文件.txt";
    for width in [180.0, 246.0, 460.0] {
        for font_size in [13.0, 18.0] {
            let (mut app, commands) = app(width, true, font_size);
            // Loaded results leave a genuinely constrained Project viewport.
            // The compact shell gives an empty tool enough space to avoid the
            // outer overflow this two-level reveal regression must exercise.
            load_long_report(&mut app);
            app.entries[63] = Entry {
                path: name.into(),
                name: name.into(),
                is_dir: false,
            };
            let mut document = Document::new(1, name.into(), "saved".into(), "r1".into());
            document.text = "precious unsaved draft 雪".into();
            app.documents.push(document);
            app.active_document = Some(1);
            begin_reveal_diagnostics(&app, width, font_size, size);
            settle(&mut app, size);
            search_focus_diagnostic_stage(&app);
            focus(&mut app, size, "help");
            app.editor_ctx.data_mut(|data| {
                data.remove::<RevealDiagnostics>(egui::Id::new(REVEAL_DIAGNOSTIC))
            });
            scroll_outer_bottom(&mut app, size);
            let outer = snapshot(&app).outer.unwrap();
            assert!(
                outer.offset.y > 0.0,
                "width={width} font={font_size} outer={:?} content={:?} offset={:?}",
                outer.rect,
                outer.content,
                outer.offset
            );
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
    focus(&mut app, large, "help");
    let focus_id = widget(&app, "help").id;
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
        assert_eq!(widget(&app, "help").id, focus_id);
        assert_eq!(
            app.editor_ctx.memory(|memory| memory.focused()),
            Some(focus_id)
        );
        assert_visible(&app, "help");
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
        assert!(panel(&app, "explorer").height() <= 16.0);
    }
    settle(&mut app, [780.0, 540.0]);
    focus(&mut app, [780.0, 540.0], "help");
    scroll_outer_bottom(&mut app, [780.0, 540.0]);
    assert!(commands.try_recv().is_err());
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
    // A user-sized tools panel constrains the actual Project viewport here;
    // the assertion below verifies that one row fits but the inner area does not.
    app.tools_height = Some(400.0);
    settle(&mut app, size);
    let outer = snapshot(&app).outer.unwrap();
    let file_height = widget(&app, "file-063.txt").rect.height();
    assert!(
        outer.rect.height() >= file_height && outer.rect.height() < 64.0,
        "outer={:?} height={} file_height={file_height}",
        outer.rect,
        outer.rect.height()
    );
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
    assert!(panel(&app, "explorer").height() <= 16.0);
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
    focus(&mut app, size, "help");
    scroll_outer_bottom(&mut app, size);
    assert!(commands.try_recv().is_err());
}

fn check_footer_geometry(app: &CedarApp) {
    let snapshot = snapshot(app);
    let geometry = snapshot.footer_geometry.unwrap();
    let outer = snapshot.outer.unwrap();
    assert_eq!(geometry.viewport, outer.rect);
    assert_eq!(
        geometry.pre_footer_content_min_y,
        geometry.final_content_min_y
    );
    assert_eq!(
        geometry.initial_content_origin_y + geometry.offset_y,
        outer.rect.top()
    );
    assert_eq!(geometry.spacing_y, 8.0);
}

fn enable_footer_checks(app: &CedarApp) {
    app.editor_ctx
        .data_mut(|data| data.insert_temp(egui::Id::new(CHECK_FOOTER), FooterChecks::default()));
}

fn footer_checks(app: &CedarApp) -> FooterChecks {
    app.editor_ctx.data(|data| {
        data.get_temp::<FooterChecks>(egui::Id::new(CHECK_FOOTER))
            .unwrap()
    })
}

fn footer_scaled_settle(app: &mut CedarApp, size: [f32; 2], scale: f32) {
    for _ in 0..4 {
        scaled_frame(app, size, vec![], scale);
        check_footer_geometry(app);
    }
}

fn footer_scaled_tab(app: &mut CedarApp, size: [f32; 2], scale: f32, modifiers: egui::Modifiers) {
    for pressed in [true, false] {
        scaled_frame(
            app,
            size,
            vec![egui::Event::Key {
                key: egui::Key::Tab,
                physical_key: Some(egui::Key::Tab),
                pressed,
                repeat: false,
                modifiers,
            }],
            scale,
        );
        check_footer_geometry(app);
    }
    footer_scaled_settle(app, size, scale);
}

#[test]
fn sidebar_footer_exact_viewport_and_content_min_survive_native_tabs_at_three_scales() {
    let size = [780.0, 540.0];
    let mut corrected = 0;
    for (width, font_size, scale) in [
        (180.0, 13.0, 1.0),
        (246.0, 18.0, 1.0),
        (460.0, 18.0, 1.0),
        (180.0, 18.0, 1.25),
        (246.0, 13.0, 1.25),
        (460.0, 18.0, 1.25),
        (180.0, 18.0, 2.0),
        (246.0, 18.0, 2.0),
        (460.0, 13.0, 2.0),
    ] {
        let (mut app, commands) = app(width, true, font_size);
        load_long_report(&mut app);
        enable_footer_checks(&app);
        footer_scaled_settle(&mut app, size, scale);
        assert_geometry(&app, width);
        let help_id = widget(&app, "help").id;
        app.editor_ctx.data_mut(|data| {
            data.insert_temp(egui::Id::new("sidebar_test_request_focus"), help_id)
        });
        footer_scaled_settle(&mut app, size, scale);
        assert_eq!(
            app.editor_ctx.memory(|memory| memory.focused()),
            Some(help_id)
        );
        assert_visible(&app, "help");
        // The last actual file remains reachable with native Shift+Tab and Tab.
        let last_id = widget(&app, "file-063.txt").id;
        for _ in 0..3 {
            footer_scaled_tab(&mut app, size, scale, egui::Modifiers::SHIFT);
            if app.editor_ctx.memory(|memory| memory.focused()) == Some(last_id) {
                break;
            }
        }
        assert_eq!(
            app.editor_ctx.memory(|memory| memory.focused()),
            Some(last_id)
        );
        assert_visible(&app, "file-063.txt");
        for _ in 0..3 {
            footer_scaled_tab(&mut app, size, scale, egui::Modifiers::NONE);
            if app.editor_ctx.memory(|memory| memory.focused()) == Some(help_id) {
                break;
            }
        }
        assert_eq!(
            app.editor_ctx.memory(|memory| memory.focused()),
            Some(help_id)
        );
        assert_visible(&app, "help");
        assert_eq!(widget(&app, "help").id, help_id);
        let checks = footer_checks(&app);
        assert!(checks.passes > 0);
        assert!(checks.requests.iter().any(|request| request.id == help_id));
        for request in &checks.requests {
            if let Some(target) = request.target {
                assert_eq!(target.x_range(), request.row.x_range());
                assert_eq!(target.height(), request.row.height());
                corrected += 1;
            }
        }
        let previous = snapshot(&app).outer.unwrap().offset;
        let requests = checks.requests.len();
        footer_scaled_settle(&mut app, size, scale);
        assert_eq!(snapshot(&app).outer.unwrap().offset, previous);
        assert_eq!(footer_checks(&app).requests.len(), requests);
        assert!(commands.try_recv().is_err());
    }
    assert!(
        corrected > 0,
        "native footer focus never exercised the correction"
    );
}

#[test]
fn sidebar_wrapped_selected_file_and_footer_keep_native_reveal_order_after_wheel() {
    let size = [780.0, 540.0];
    let name = "最后一份 Unicode draft 文件.txt";
    let (mut app, commands) = app(180.0, true, 18.0);
    app.entries[63] = Entry {
        path: name.into(),
        name: name.into(),
        is_dir: false,
    };
    let mut document = Document::new(1, name.into(), "saved".into(), "r1".into());
    document.text = "precious unsaved draft 雪".into();
    app.documents.push(document);
    app.active_document = Some(1);
    enable_footer_checks(&app);
    settle(&mut app, size);
    focus(&mut app, size, "help");
    scroll_outer_bottom(&mut app, size);
    assert!(snapshot(&app).outer.unwrap().offset.y > 0.0);
    let file_id = widget(&app, name).id;
    for _ in 0..3 {
        key(&mut app, size, egui::Key::Tab, egui::Modifiers::SHIFT);
        check_footer_geometry(&app);
        if app.editor_ctx.memory(|memory| memory.focused()) == Some(file_id) {
            break;
        }
    }
    assert_eq!(
        app.editor_ctx.memory(|memory| memory.focused()),
        Some(file_id)
    );
    assert_visible(&app, name);
    assert!(snapshot(&app).explorer.unwrap().offset.y > 0.0);
    assert!(snapshot(&app)
        .explorer
        .unwrap()
        .rect
        .contains_rect(widget(&app, name).rect));
    assert!(widget(&app, name).rect.height() > 27.0);
    let nested_offset = snapshot(&app).explorer.unwrap().offset;
    let search_id = widget(&app, "help").id;
    let requests_before_tab = footer_checks(&app).requests.len();
    for _ in 0..3 {
        key(&mut app, size, egui::Key::Tab, egui::Modifiers::NONE);
        check_footer_geometry(&app);
        if app.editor_ctx.memory(|memory| memory.focused()) == Some(search_id) {
            break;
        }
    }
    assert_eq!(
        app.editor_ctx.memory(|memory| memory.focused()),
        Some(search_id)
    );
    assert_visible(&app, "help");
    assert_eq!(snapshot(&app).explorer.unwrap().offset, nested_offset);
    assert!(footer_checks(&app).requests[requests_before_tab..]
        .iter()
        .any(|request| request.id == search_id));
    let outer_offset = snapshot(&app).outer.unwrap().offset;
    settle(&mut app, size);
    assert_eq!(snapshot(&app).outer.unwrap().offset, outer_offset);
    assert_eq!(app.active_document, Some(1));
    assert_eq!(app.documents[0].text, "precious unsaved draft 雪");
    assert_eq!(app.documents[0].saved_text, "saved");
    assert!(commands.try_recv().is_err());
}

#[derive(Clone, Copy, Debug)]
struct FooterContractFrame {
    viewport: egui::Rect,
    row: egui::Rect,
    clip: egui::Rect,
    help: egui::Rect,
    target: Option<egui::Rect>,
    initial_origin_y: f32,
    content_min_y: f32,
    offset_before: f32,
    offset_after: f32,
}

#[derive(Clone, Copy)]
struct FooterContractCase {
    row_content_top: f32,
    row_height: f32,
    request: bool,
    correct: bool,
    later_help_request: bool,
}

// This is a pinned-egui public ScrollArea contract, separate from the production
// sidebar tests above. Exact public interaction rectangles avoid font rounding
// hiding a 0.125-point deficit. No stored State or private pass state is changed.
fn footer_contract_frame(
    ctx: &egui::Context,
    scale: f32,
    initial_offset: Option<f32>,
    case: FooterContractCase,
) -> FooterContractFrame {
    let mut input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(320.0, 240.0),
        )),
        ..Default::default()
    };
    input
        .viewports
        .get_mut(&egui::ViewportId::ROOT)
        .unwrap()
        .native_pixels_per_point = Some(scale);
    let mut observed = None;
    let _ = ctx.run(input, |ctx| {
        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(12.0))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 8.0;
                ui.visuals_mut().clip_rect_margin = 3.0;
                let mut area = egui::ScrollArea::vertical()
                    .id_salt("footer_contract")
                    .max_height(100.125)
                    .min_scrolled_height(0.0)
                    .auto_shrink([false, false])
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                    .drag_to_scroll(false)
                    .animated(false);
                if let Some(offset) = initial_offset {
                    area = area.vertical_scroll_offset(offset);
                }
                let output = area.show_viewport(ui, |ui, viewport| {
                    let initial_origin = ui.max_rect().min;
                    let actual_viewport = viewport.translate(initial_origin.to_vec2());
                    ui.set_min_size(egui::vec2(160.0, 400.0));
                    // A prior child can expand min_rect without moving the initial
                    // origin that converts show_viewport to screen coordinates.
                    ui.expand_to_include_rect(egui::Rect::from_min_max(
                        initial_origin - egui::vec2(0.0, 0.125),
                        initial_origin,
                    ));
                    let content_min_y = ui.min_rect().top();
                    assert_eq!(content_min_y, initial_origin.y - 0.125);
                    let row = ui.interact(
                        egui::Rect::from_min_size(
                            initial_origin + egui::vec2(20.0, case.row_content_top),
                            egui::vec2(60.0, case.row_height),
                        ),
                        ui.make_persistent_id("fractional_footer_row"),
                        egui::Sense::click(),
                    );
                    let mut target = None;
                    if case.request {
                        row.scroll_to_me(None);
                        if case.correct {
                            target = sidebar_reveal::footer_target(
                                row.rect,
                                actual_viewport,
                                content_min_y,
                                viewport.min.y,
                                ui.spacing().item_spacing.y,
                            );
                            if let Some(target) = target {
                                ui.scroll_to_rect(target, Some(egui::Align::TOP));
                            }
                        }
                    }
                    let help = ui.put(
                        egui::Rect::from_min_size(
                            initial_origin + egui::vec2(20.0, 300.0),
                            egui::vec2(120.0, 24.0),
                        ),
                        egui::Label::new("Later help text").selectable(true),
                    );
                    if case.request && case.later_help_request {
                        // A controlled public request from a real Label response;
                        // this does not claim to simulate pointer text selection.
                        help.scroll_to_me(Some(egui::Align::TOP));
                    }
                    assert_eq!(content_min_y, ui.min_rect().top());
                    FooterContractFrame {
                        viewport: actual_viewport,
                        row: row.rect,
                        clip: ui.clip_rect(),
                        help: help.rect,
                        target,
                        initial_origin_y: initial_origin.y,
                        content_min_y,
                        offset_before: viewport.min.y,
                        offset_after: f32::NAN,
                    }
                });
                assert_eq!(output.inner.viewport, output.inner_rect);
                observed = Some(FooterContractFrame {
                    offset_after: output.state.offset.y,
                    ..output.inner
                });
            });
    });
    observed.unwrap()
}

#[test]
fn sidebar_footer_public_scroll_area_resolves_exact_eighth_point_deficits() {
    for scale in [1.0, 1.25, 2.0] {
        for (top, expected_offset) in [(39.875, 39.875), (120.25, 40.125)] {
            let ctx = egui::Context::default();
            let case = FooterContractCase {
                row_content_top: top,
                row_height: 20.0,
                request: true,
                correct: true,
                later_help_request: false,
            };
            let first = footer_contract_frame(&ctx, scale, Some(40.0), case);
            assert_eq!(first.offset_before, 40.0);
            assert_eq!(first.viewport.height(), 100.125);
            assert!(!first.viewport.contains_rect(first.row));
            assert!(
                first.clip.contains_rect(first.row),
                "fixture must expose paint-margin ambiguity"
            );
            assert_eq!(first.content_min_y, first.initial_origin_y - 0.125);
            assert!(first.target.is_some());
            assert_eq!(first.offset_after, expected_offset);
            let next = footer_contract_frame(
                &ctx,
                scale,
                None,
                FooterContractCase {
                    request: false,
                    ..case
                },
            );
            assert!(next.viewport.contains_rect(next.row));
            assert_eq!(next.offset_before, expected_offset);
            assert_eq!(next.offset_after, expected_offset);

            // The old unaligned request sees the expanded paint clip and
            // leaves this exact deficit unresolved in the same real container.
            let old_ctx = egui::Context::default();
            let old = footer_contract_frame(
                &old_ctx,
                scale,
                Some(40.0),
                FooterContractCase {
                    correct: false,
                    ..case
                },
            );
            assert_eq!(old.offset_after, 40.0);
            assert!(!old.viewport.contains_rect(old.row));
        }
    }
}

#[test]
fn sidebar_footer_public_scroll_area_preserves_inside_and_oversized_fallback() {
    for (top, height) in [(60.0, 20.0), (20.0, 160.0)] {
        let ctx = egui::Context::default();
        let case = FooterContractCase {
            row_content_top: top,
            row_height: height,
            request: true,
            correct: true,
            later_help_request: false,
        };
        let observed = footer_contract_frame(&ctx, 1.25, Some(40.0), case);
        let native_ctx = egui::Context::default();
        let native = footer_contract_frame(
            &native_ctx,
            1.25,
            Some(40.0),
            FooterContractCase {
                correct: false,
                ..case
            },
        );
        assert!(observed.target.is_none());
        assert_eq!(observed.offset_after, 40.0);
        assert_eq!(observed.offset_after, native.offset_after);
        if height <= observed.viewport.height() {
            assert!(observed.viewport.contains_rect(observed.row));
        } else {
            assert!(observed.row.top() < observed.viewport.top());
            assert!(observed.row.bottom() > observed.viewport.bottom());
        }
    }
}

#[test]
fn sidebar_footer_public_scroll_area_later_help_request_keeps_last_writer_priority() {
    let ctx = egui::Context::default();
    let case = FooterContractCase {
        row_content_top: 120.25,
        row_height: 20.0,
        request: true,
        correct: true,
        later_help_request: true,
    };
    let combined = footer_contract_frame(&ctx, 1.25, Some(40.0), case);
    let native_ctx = egui::Context::default();
    let native = footer_contract_frame(
        &native_ctx,
        1.25,
        Some(40.0),
        FooterContractCase {
            correct: false,
            ..case
        },
    );
    assert!(combined.target.is_some());
    assert_eq!(combined.offset_after, native.offset_after);
    assert!(
        combined.offset_after > 40.125,
        "later help request did not replace the footer target"
    );
    let next = footer_contract_frame(
        &ctx,
        1.25,
        None,
        FooterContractCase {
            request: false,
            ..case
        },
    );
    assert!(next.viewport.contains_rect(next.help));
    assert_eq!(next.offset_after, combined.offset_after);
}
