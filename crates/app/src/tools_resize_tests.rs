//! Real egui frames exercise Cedar's native tools edge and production contents.
//! These tests never seed PanelState or manufacture a resize response.
use super::*;
use sha2::{Digest, Sha256};

const SIZE: [f32; 2] = [1280.0, 900.0];

#[derive(Clone, Copy)]
struct Geometry {
    available: egui::Rect,
    panel: Option<egui::Rect>,
    preferred: Option<f32>,
}

fn app(tool: Tool) -> (CedarApp, Receiver<Command>) {
    let mut app = CedarApp::empty();
    app.state = ConnectionState::Ready;
    app.open_form = false;
    app.tool = tool;
    app.tools_open = true;
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
    let (worker, commands) = Worker::recording();
    app.worker = Some(worker);
    (app, commands)
}

fn frames(
    app: &mut CedarApp,
    size: [f32; 2],
    events: Vec<egui::Event>,
    discard_first: bool,
) -> Vec<Geometry> {
    let ctx = app.editor_ctx.clone();
    ctx.options_mut(|options| options.max_passes = 2.try_into().unwrap());
    let mut passes = Vec::new();
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size.into())),
            events,
            ..Default::default()
        },
        |ctx| {
            workspace_access_tests::begin(ctx);
            app.shortcuts(ctx);
            app.header(ctx);
            app.footer(ctx);
            app.notifications(ctx);
            let available = ctx.available_rect();
            app.tools(ctx);
            let panel = app.tools_open.then(|| {
                egui::containers::panel::PanelState::load(ctx, egui::Id::new("tools"))
                    .unwrap()
                    .rect
            });
            app.sidebar(ctx);
            egui::CentralPanel::default().show(ctx, |_| {});
            passes.push(Geometry {
                available,
                panel,
                preferred: app.tools_height,
            });
            if discard_first && passes.len() == 1 {
                ctx.request_discard("exercise tools resize across repeated layout passes");
            }
        },
    );
    passes
}

fn frame(app: &mut CedarApp, size: [f32; 2], events: Vec<egui::Event>) -> Geometry {
    *frames(app, size, events, false).last().unwrap()
}

fn settle(app: &mut CedarApp, size: [f32; 2]) -> Geometry {
    let mut geometry = frame(app, size, vec![]);
    for _ in 0..3 {
        geometry = frame(app, size, vec![]);
    }
    geometry
}

fn native(app: &CedarApp) -> egui::Response {
    app.editor_ctx
        .read_response(tools_resize::native_resize_id())
        .unwrap()
}

fn button(at: egui::Pos2, button: egui::PointerButton, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos: at,
        button,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

fn press(app: &mut CedarApp, size: [f32; 2]) -> (egui::Pos2, Geometry) {
    let origin = native(app).rect.center();
    let geometry = frame(
        app,
        size,
        vec![
            egui::Event::PointerMoved(origin),
            button(origin, egui::PointerButton::Primary, true),
        ],
    );
    assert!(native(app).dragged_by(egui::PointerButton::Primary));
    (origin, geometry)
}

fn near(actual: f32, expected: f32) {
    assert!((actual - expected).abs() <= 1.0, "{actual} != {expected}");
}

fn assert_height(geometry: Geometry, expected: f32) {
    near(geometry.preferred.expect("explicit height"), expected);
    let panel = geometry.panel.expect("visible tools");
    near(panel.height(), expected);
    near(panel.bottom(), geometry.available.bottom());
}

fn resize(app: &mut CedarApp, size: [f32; 2], height: f32) -> Geometry {
    let (origin, geometry) = press(app, size);
    let moved = egui::pos2(origin.x, geometry.available.bottom() - height);
    let dragged = frame(app, size, vec![egui::Event::PointerMoved(moved)]);
    assert!(native(app).dragged_by(egui::PointerButton::Primary));
    assert_height(dragged, height);
    frame(
        app,
        size,
        vec![button(moved, egui::PointerButton::Primary, false)],
    );
    assert!(native(app).drag_stopped_by(egui::PointerButton::Primary));
    let settled = settle(app, size);
    assert_height(settled, height);
    settled
}

#[test]
fn tools_resize_passive_click_and_horizontal_motion_do_not_establish_a_preference() {
    let (mut app, commands) = app(Tool::Search);
    let passive = settle(&mut app, SIZE);
    assert_eq!(app.tools_height, None);
    assert!(passive.panel.unwrap().height() < 245.0);
    let (origin, _) = press(&mut app, SIZE);
    assert_eq!(
        app.tools_height, None,
        "drag sense also fires on a stationary press"
    );
    frame(&mut app, SIZE, vec![]);
    assert_eq!(app.tools_height, None);
    let across = origin + egui::vec2(80.0, 0.0);
    frame(&mut app, SIZE, vec![egui::Event::PointerMoved(across)]);
    assert_eq!(app.tools_height, None);
    frame(
        &mut app,
        SIZE,
        vec![button(across, egui::PointerButton::Primary, false)],
    );
    let released = settle(&mut app, SIZE);
    assert_eq!(app.tools_height, None);
    near(
        released.panel.unwrap().height(),
        passive.panel.unwrap().height(),
    );
    assert!(commands.try_recv().is_err());
}

#[test]
fn tools_resize_up_down_and_release_retain_the_last_native_allocation() {
    let (mut app, commands) = app(Tool::Search);
    settle(&mut app, SIZE);
    resize(&mut app, SIZE, 430.0);
    let (origin, geometry) = press(&mut app, SIZE);
    let down = egui::pos2(origin.x, geometry.available.bottom() - 260.0);
    assert_height(
        frame(&mut app, SIZE, vec![egui::Event::PointerMoved(down)]),
        260.0,
    );
    // A release at a new pointer position is not another allocated drag pass.
    let release = down + egui::vec2(0.0, 35.0);
    frame(
        &mut app,
        SIZE,
        vec![
            egui::Event::PointerMoved(release),
            button(release, egui::PointerButton::Primary, false),
        ],
    );
    assert!(native(&app).drag_stopped_by(egui::PointerButton::Primary));
    assert_height(settle(&mut app, SIZE), 260.0);
    assert!(commands.try_recv().is_err());
}

#[test]
fn tools_resize_away_then_back_captures_the_origin_allocation_before_release() {
    let (mut app, commands) = app(Tool::Search);
    settle(&mut app, SIZE);
    let (origin, geometry) = press(&mut app, SIZE);
    let origin_height = geometry.available.bottom() - origin.y;
    let away = egui::pos2(origin.x, geometry.available.bottom() - 430.0);
    assert_height(
        frame(&mut app, SIZE, vec![egui::Event::PointerMoved(away)]),
        430.0,
    );
    assert_height(
        frame(&mut app, SIZE, vec![egui::Event::PointerMoved(origin)]),
        origin_height,
    );
    assert_height(frame(&mut app, SIZE, vec![]), origin_height);
    frame(
        &mut app,
        SIZE,
        vec![button(origin, egui::PointerButton::Primary, false)],
    );
    assert_height(settle(&mut app, SIZE), origin_height);
    assert!(commands.try_recv().is_err());
}

#[test]
fn tools_resize_repeated_layout_passes_preserve_native_height_without_delta_accumulation() {
    let (mut app, commands) = app(Tool::Search);
    settle(&mut app, SIZE);
    let (origin, geometry) = press(&mut app, SIZE);
    let moved = egui::pos2(origin.x, geometry.available.bottom() - 390.0);
    let passes = frames(&mut app, SIZE, vec![egui::Event::PointerMoved(moved)], true);
    assert_eq!(passes.len(), 2);
    for pass in passes {
        assert_height(pass, 390.0);
    }
    let passes = frames(&mut app, SIZE, vec![], true);
    assert_eq!(passes.len(), 2);
    for pass in passes {
        assert_height(pass, 390.0);
    }
    frame(
        &mut app,
        SIZE,
        vec![button(moved, egui::PointerButton::Primary, false)],
    );
    assert_height(settle(&mut app, SIZE), 390.0);
    assert!(commands.try_recv().is_err());
}

#[test]
fn tools_resize_secondary_mixed_buttons_and_another_widget_do_not_establish_a_preference() {
    for mixed in [false, true] {
        let (mut app, commands) = app(Tool::Search);
        settle(&mut app, SIZE);
        let origin = native(&app).rect.center();
        if mixed {
            press(&mut app, SIZE);
        }
        frame(
            &mut app,
            SIZE,
            vec![
                egui::Event::PointerMoved(origin),
                button(origin, egui::PointerButton::Secondary, true),
            ],
        );
        let moved = origin - egui::vec2(0.0, 100.0);
        frame(&mut app, SIZE, vec![egui::Event::PointerMoved(moved)]);
        assert_eq!(app.tools_height, None);
        let mut release = vec![button(moved, egui::PointerButton::Secondary, false)];
        if mixed {
            release.push(button(moved, egui::PointerButton::Primary, false));
        }
        frame(&mut app, SIZE, release);
        settle(&mut app, SIZE);
        assert_eq!(app.tools_height, None);
        assert!(commands.try_recv().is_err());
    }
    let (mut app, commands) = app(Tool::Search);
    settle(&mut app, SIZE);
    let origin = workspace_access_tests::recorded_rect(&app, "search_query").center();
    frame(
        &mut app,
        SIZE,
        vec![
            egui::Event::PointerMoved(origin),
            button(origin, egui::PointerButton::Primary, true),
        ],
    );
    let moved = origin - egui::vec2(0.0, 100.0);
    frame(&mut app, SIZE, vec![egui::Event::PointerMoved(moved)]);
    assert!(!native(&app).dragged());
    assert_eq!(app.tools_height, None);
    frame(
        &mut app,
        SIZE,
        vec![button(moved, egui::PointerButton::Primary, false)],
    );
    assert!(commands.try_recv().is_err());
}

#[test]
fn tools_resize_empty_loading_populated_and_cleared_report_keep_the_preference() {
    let (mut app, commands) = app(Tool::Tests);
    settle(&mut app, SIZE);
    resize(&mut app, SIZE, 430.0);
    app.test_report.path = "TEST-tools.xml".into();
    app.load_test_report();
    let command = commands.try_recv().expect("explicit report read");
    assert!(matches!(command.op, Operation::Read { .. }));
    assert!(app.test_report.loading.is_some());
    assert_height(settle(&mut app, SIZE), 430.0);
    let cases = (0..32)
        .map(|index| format!("<testcase name=\"case-{index}\" time=\"0.1\"/>"))
        .collect::<String>();
    let text = format!("<testsuite name=\"Tools\" tests=\"32\">{cases}</testsuite>");
    let revision = format!("{:x}", Sha256::digest(text.as_bytes()));
    app.apply_event(Event {
        generation: app.generation,
        id: command.id,
        connected: true,
        result: Ok(Payload::File {
            path: app.test_report.path.clone(),
            text,
            revision,
        }),
    });
    assert!(app.test_report.snapshot.is_some());
    assert_height(settle(&mut app, SIZE), 430.0);
    app.test_report.clear();
    assert_height(settle(&mut app, SIZE), 430.0);
    assert!(commands.try_recv().is_err());
}

#[test]
fn tools_resize_hide_tool_switch_disconnect_and_reconnect_keep_runtime_preference() {
    let (mut app, commands) = app(Tool::Search);
    settle(&mut app, SIZE);
    resize(&mut app, SIZE, 430.0);
    let close = workspace_access_tests::recorded_rect(&app, "close_tools").center();
    for pressed in [true, false] {
        frame(
            &mut app,
            SIZE,
            vec![
                egui::Event::PointerMoved(close),
                button(close, egui::PointerButton::Primary, pressed),
            ],
        );
    }
    assert!(!app.tools_open);
    assert_eq!(app.tools_height, Some(430.0));
    let hidden = settle(&mut app, SIZE);
    assert!(hidden.panel.is_none());
    for (tool, label) in [
        (Tool::Search, "Search"),
        (Tool::Git, "Git"),
        (Tool::Run, "Run"),
        (Tool::Language, "LSP"),
        (Tool::Tests, "Tests"),
    ] {
        let at = workspace_access_tests::recorded_rect(&app, &format!("sidebar_{label}")).center();
        for pressed in [true, false] {
            frame(
                &mut app,
                SIZE,
                vec![
                    egui::Event::PointerMoved(at),
                    button(at, egui::PointerButton::Primary, pressed),
                ],
            );
        }
        assert!(app.tools_open && app.tool == tool);
        let geometry = settle(&mut app, SIZE);
        assert_eq!(geometry.preferred, Some(430.0));
        // Native allocation is reserved, but tall content may still overflow.
        assert!(geometry.panel.unwrap().height() >= 429.0);
        near(
            native(&app).rect.center().y,
            geometry.available.bottom() - 430.0,
        );
    }
    assert_height(settle(&mut app, SIZE), 430.0); // The last tool is empty Tests.
    assert!(commands.try_recv().is_err());
    app.disconnected("fixture connection loss".into());
    assert_height(settle(&mut app, SIZE), 430.0);
    let (worker, reconnected_commands) = Worker::recording();
    app.worker = Some(worker);
    app.state = ConnectionState::Connecting;
    app.connecting_form = Some(ConnectForm::default());
    app.generation += 1;
    app.apply_event(Event {
        generation: app.generation,
        id: 0,
        connected: true,
        result: Ok(Payload::Hello {
            protocol: cedar_protocol::PROTOCOL_VERSION,
            root: "tools-resize-fixture".into(),
            agent: None,
        }),
    });
    assert!(app.ready());
    assert_height(settle(&mut app, SIZE), 430.0);
    // Reconnection may request the ordinary initial directory listing.
    for command in reconnected_commands.try_iter() {
        assert!(matches!(command.op, Operation::List { .. }));
    }
    assert_eq!(CedarApp::empty().tools_height, None);
}

#[test]
fn tools_resize_window_clamp_and_regrow_do_not_overwrite_the_preference() {
    let (mut app, commands) = app(Tool::Search);
    settle(&mut app, SIZE);
    resize(&mut app, SIZE, 430.0);
    let small = settle(&mut app, [1280.0, 380.0]);
    assert!(small.available.height() >= 150.0 && small.available.height() < 430.0);
    near(small.panel.unwrap().height(), small.available.height());
    assert_eq!(app.tools_height, Some(430.0));
    assert_height(settle(&mut app, SIZE), 430.0);
    assert!(commands.try_recv().is_err());
}

#[test]
fn tools_resize_content_overflow_does_not_become_the_stored_height() {
    let (mut app, commands) = app(Tool::Tests);
    let narrow = [620.0, 900.0];
    settle(&mut app, narrow);
    let (origin, geometry) = press(&mut app, narrow);
    let moved = egui::pos2(origin.x, geometry.available.bottom() - 150.0);
    let dragged = frame(&mut app, narrow, vec![egui::Event::PointerMoved(moved)]);
    near(dragged.preferred.unwrap(), 150.0);
    // Wrapped report controls retain the existing content-overflow behavior.
    assert!(dragged.panel.unwrap().height() > 150.0);
    frame(
        &mut app,
        narrow,
        vec![button(moved, egui::PointerButton::Primary, false)],
    );
    settle(&mut app, narrow);
    assert_eq!(app.tools_height, Some(150.0));
    app.tool = Tool::Search;
    assert_height(settle(&mut app, SIZE), 150.0);
    assert!(commands.try_recv().is_err());
}
