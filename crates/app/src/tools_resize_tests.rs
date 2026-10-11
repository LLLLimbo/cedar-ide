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
    automatic_clicked: bool,
    capture_allowed: bool,
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
    input_frames(
        app,
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size.into())),
            events,
            ..Default::default()
        },
        discard_first,
    )
}

fn input_frames(
    app: &mut CedarApp,
    mut input: egui::RawInput,
    discard_first: bool,
) -> Vec<Geometry> {
    let ctx = app.editor_ctx.clone();
    ctx.options_mut(|options| options.max_passes = 2.try_into().unwrap());
    // The production hook observes the original batch exactly once, including
    // frames which egui lays out twice. Never replay it in the render closure.
    eframe::App::raw_input_hook(app, &ctx, &mut input);
    let mut passes = Vec::new();
    let _ = ctx.run(input, |ctx| {
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
        let automatic_clicked = panel.is_some()
            && workspace_access_tests::recorded_response(app, "tools_automatic_height").clicked();
        app.sidebar(ctx);
        egui::CentralPanel::default().show(ctx, |ui| {
            if !app.documents.is_empty() {
                app.editor(ui);
            }
        });
        passes.push(Geometry {
            available,
            panel,
            preferred: app.tools_height,
            automatic_clicked,
            capture_allowed: app.tools_height_reset.capture_allowed(),
        });
        if discard_first && passes.len() == 1 {
            ctx.request_discard("exercise tools resize across repeated layout passes");
        }
    });
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
    assert!(matches!(&command.op, Operation::Read { path } if path == "TEST-tools.xml"));
    let load = app
        .test_report
        .loading
        .clone()
        .expect("resize fixture report Load is pending");
    assert_eq!(
        load.generation, app.generation,
        "resize fixture Load generation"
    );
    assert_eq!(load.path, "TEST-tools.xml", "resize fixture Load path");
    assert!(
        matches!(app.pending.get(&command.id), Some(Job::TestReportRead(pending)) if pending == &load)
    );
    assert_height(settle(&mut app, SIZE), 430.0);
    assert_eq!(
        app.test_report.loading.as_ref(),
        Some(&load),
        "resize fixture Load survives loading frames"
    );
    let cases = (0..32)
        .map(|index| format!("<testcase name=\"case-{index}\" time=\"0.1\"/>"))
        .collect::<String>();
    let text = format!("<testsuite name=\"Tools\" tests=\"32\" failures=\"0\" errors=\"0\" skipped=\"0\">{cases}</testsuite>");
    let parsed = test_reports::parse_report(&text)
        .expect("resize fixture XML must satisfy the strict report parser");
    assert_eq!(parsed.cases.len(), 32, "resize fixture parsed case count");
    assert_eq!(
        parsed.counts.passed, 32,
        "resize fixture parsed passed count"
    );
    let revision = format!("{:x}", Sha256::digest(text.as_bytes()));
    app.apply_event(Event {
        generation: app.generation,
        id: command.id,
        connected: true,
        result: Ok(Payload::File {
            path: app.test_report.path.clone(),
            text,
            revision: revision.clone(),
        }),
    });
    assert!(
        app.test_report.snapshot.is_some(),
        "resize fixture reply must create its validated report snapshot"
    );
    let snapshot = app.test_report.snapshot.as_ref().unwrap();
    assert_eq!(
        snapshot.source, load,
        "resize fixture accepted Load identity"
    );
    assert_eq!(
        snapshot.revision, revision,
        "resize fixture accepted source revision"
    );
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

const AUTOMATIC: &str = "tools_automatic_height";

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

fn space() -> Vec<egui::Event> {
    let mut events = key(egui::Key::Space, egui::Modifiers::NONE);
    events.insert(1, egui::Event::Text(" ".into()));
    events
}

fn visible_rect(app: &CedarApp, size: [f32; 2], name: &str) -> egui::Rect {
    let rect = workspace_access_tests::recorded_rect(app, name);
    let clip = workspace_access_tests::recorded_clip(app, name);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size.into());
    assert!(rect.is_positive(), "{name} has no rendered area: {rect:?}");
    assert!(
        clip.contains_rect(rect),
        "{name} is clipped: {rect:?} / {clip:?}"
    );
    assert!(
        screen.contains_rect(rect),
        "{name} is outside the window: {rect:?}"
    );
    rect
}

fn click_widget(app: &mut CedarApp, size: [f32; 2], name: &str) -> Geometry {
    let at = visible_rect(app, size, name).center();
    assert!(workspace_access_tests::recorded_response(app, name).enabled());
    frame(
        app,
        size,
        vec![
            egui::Event::PointerMoved(at),
            button(at, egui::PointerButton::Primary, true),
        ],
    );
    assert!(workspace_access_tests::recorded_response(app, name).is_pointer_button_down_on());
    assert!(visible_rect(app, size, name).contains(at));
    let released = frame(
        app,
        size,
        vec![button(at, egui::PointerButton::Primary, false)],
    );
    assert!(workspace_access_tests::recorded_response(app, name)
        .clicked_by(egui::PointerButton::Primary));
    released
}

fn tab_to(app: &mut CedarApp, size: [f32; 2], name: &str) {
    // Pinned egui 0.31.1 registers native scrollbars as focus stops too. Follow
    // actual focused IDs instead of assuming a fixed number/order of Tab stops.
    for _ in 0..80 {
        frame(app, size, key(egui::Key::Tab, egui::Modifiers::NONE));
        let response = workspace_access_tests::recorded_response(app, name);
        if app.editor_ctx.memory(|memory| memory.focused()) == Some(response.id) {
            assert!(response.enabled() && response.has_focus());
            visible_rect(app, size, name);
            return;
        }
    }
    panic!("Tab never reached the rendered {name} control");
}

fn assert_automatic(app: &CedarApp, geometry: Geometry) {
    assert_eq!(geometry.preferred, None);
    assert_eq!(app.tools_height, None);
    assert!(!workspace_access_tests::recorded_response(app, AUTOMATIC).enabled());
}

fn reset_while_primary_held(app: &mut CedarApp) -> egui::Pos2 {
    settle(app, SIZE);
    resize(app, SIZE, 430.0);
    let (origin, geometry) = press(app, SIZE);
    let moved = egui::pos2(origin.x, geometry.available.bottom() - 350.0);
    assert_height(
        frame(app, SIZE, vec![egui::Event::PointerMoved(moved)]),
        350.0,
    );
    // A pointer press elsewhere surrenders button focus in pinned egui. Reach
    // the real button with Tab after starting the native drag, without releasing.
    tab_to(app, SIZE, AUTOMATIC);
    assert!(app.editor_ctx.input(|input| input.pointer.primary_down()));
    assert!(native(app).dragged_by(egui::PointerButton::Primary));
    let passes = frames(
        app,
        SIZE,
        key(egui::Key::Enter, egui::Modifiers::NONE),
        true,
    );
    assert_eq!(passes.len(), 2);
    assert!(
        passes[0].automatic_clicked,
        "Enter must activate the actual button"
    );
    for pass in passes {
        assert_eq!(pass.preferred, None, "reset must win every render pass");
        assert!(!pass.capture_allowed);
    }
    assert!(app.editor_ctx.input(|input| input.pointer.primary_down()));
    moved
}

#[test]
fn tools_automatic_height_pointer_reset_restores_passive_layout_and_disabled_noop() {
    let (mut app, commands) = app(Tool::Search);
    let baseline = settle(&mut app, SIZE).panel.unwrap().height();
    resize(&mut app, SIZE, 430.0);
    let released = click_widget(&mut app, SIZE, AUTOMATIC);
    assert!(released.automatic_clicked);
    assert_eq!(released.preferred, None);
    let automatic = settle(&mut app, SIZE);
    assert_automatic(&app, automatic);
    near(automatic.panel.unwrap().height(), baseline);

    let at = visible_rect(&app, SIZE, AUTOMATIC).center();
    for pressed in [true, false] {
        frame(
            &mut app,
            SIZE,
            vec![
                egui::Event::PointerMoved(at),
                button(at, egui::PointerButton::Primary, pressed),
            ],
        );
        let response = workspace_access_tests::recorded_response(&app, AUTOMATIC);
        assert!(!response.enabled() && !response.clicked());
        assert_eq!(app.tools_height, None);
    }
    let after_noop = settle(&mut app, SIZE);
    near(after_noop.panel.unwrap().height(), baseline);
    assert_automatic(&app, after_noop);
    resize(&mut app, SIZE, 375.0);
    assert!(workspace_access_tests::recorded_response(&app, AUTOMATIC).enabled());
    assert!(commands.try_recv().is_err());
}

#[test]
fn tools_automatic_height_real_tab_enter_and_space_restore_automatic_mode() {
    for activation in [egui::Key::Enter, egui::Key::Space] {
        let (mut app, commands) = app(Tool::Search);
        let baseline = settle(&mut app, SIZE).panel.unwrap().height();
        resize(&mut app, SIZE, 430.0);
        tab_to(&mut app, SIZE, AUTOMATIC);
        let events = if activation == egui::Key::Space {
            space()
        } else {
            key(activation, egui::Modifiers::NONE)
        };
        let activated = frame(&mut app, SIZE, events);
        assert!(
            activated.automatic_clicked,
            "{activation:?} must activate the real button"
        );
        assert_eq!(activated.preferred, None);
        let automatic = settle(&mut app, SIZE);
        assert_automatic(&app, automatic);
        near(automatic.panel.unwrap().height(), baseline);
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn tools_automatic_height_pointer_reset_survives_discarded_layout_pass() {
    let (mut app, commands) = app(Tool::Search);
    let baseline = settle(&mut app, SIZE).panel.unwrap().height();
    resize(&mut app, SIZE, 430.0);
    let at = visible_rect(&app, SIZE, AUTOMATIC).center();
    frame(
        &mut app,
        SIZE,
        vec![
            egui::Event::PointerMoved(at),
            button(at, egui::PointerButton::Primary, true),
        ],
    );
    assert!(workspace_access_tests::recorded_response(&app, AUTOMATIC).is_pointer_button_down_on());
    let passes = frames(
        &mut app,
        SIZE,
        vec![button(at, egui::PointerButton::Primary, false)],
        true,
    );
    assert_eq!(passes.len(), 2);
    assert!(passes[0].automatic_clicked);
    for pass in passes {
        assert_eq!(pass.preferred, None);
        assert!(!pass.capture_allowed);
    }
    let automatic = settle(&mut app, SIZE);
    assert_automatic(&app, automatic);
    near(automatic.panel.unwrap().height(), baseline);
    assert!(
        automatic.capture_allowed,
        "a released click cannot latch the guard"
    );
    assert!(commands.try_recv().is_err());
}

#[test]
fn tools_automatic_height_held_drag_waits_for_release_and_blocks_release_repress_batch() {
    let (mut app, commands) = app(Tool::Search);
    let moved = reset_while_primary_held(&mut app);
    let held = moved - egui::vec2(0.0, 55.0);
    for events in [vec![egui::Event::PointerMoved(held)], vec![]] {
        let passes = frames(&mut app, SIZE, events, true);
        assert_eq!(passes.len(), 2);
        for pass in passes {
            assert_eq!(pass.preferred, None);
            assert!(!pass.capture_allowed, "idle/held input is not a release");
        }
    }
    let origin = native(&app).rect.center();
    let passes = frames(
        &mut app,
        SIZE,
        vec![
            button(held, egui::PointerButton::Primary, false),
            egui::Event::PointerMoved(origin),
            button(origin, egui::PointerButton::Primary, true),
            // Pinned egui hit-tests the final batch position. Stay inside the
            // actual divider so this new gesture really acquires that handle.
            egui::Event::PointerMoved(origin - egui::vec2(0.0, 1.0)),
        ],
        true,
    );
    assert_eq!(passes.len(), 2);
    let available_bottom = passes.last().unwrap().available.bottom();
    for pass in passes {
        assert_eq!(pass.preferred, None, "same-batch repress cannot undo reset");
        assert!(
            !pass.capture_allowed,
            "the real release unblocks only a later input frame"
        );
    }
    let moved_again = egui::pos2(origin.x, available_bottom - 385.0);
    let captured = frame(&mut app, SIZE, vec![egui::Event::PointerMoved(moved_again)]);
    assert!(captured.capture_allowed);
    assert!(native(&app).dragged_by(egui::PointerButton::Primary));
    assert_height(captured, 385.0);
    frame(
        &mut app,
        SIZE,
        vec![button(moved_again, egui::PointerButton::Primary, false)],
    );
    assert_height(settle(&mut app, SIZE), 385.0);
    click_widget(&mut app, SIZE, AUTOMATIC);
    let automatic = settle(&mut app, SIZE);
    assert_automatic(&app, automatic);
    resize(&mut app, SIZE, 365.0);
    assert!(commands.try_recv().is_err());
}

#[test]
fn tools_automatic_height_focus_loss_and_pointer_gone_do_not_fabricate_release() {
    for focus_lost in [false, true] {
        let (mut app, commands) = app(Tool::Search);
        reset_while_primary_held(&mut app);
        let passes = input_frames(
            &mut app,
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SIZE.into())),
                focused: !focus_lost,
                events: if focus_lost {
                    vec![egui::Event::WindowFocused(false), egui::Event::PointerGone]
                } else {
                    vec![egui::Event::PointerGone]
                },
                ..Default::default()
            },
            true,
        );
        for pass in passes {
            assert_eq!(pass.preferred, None);
            assert!(!pass.capture_allowed);
        }
        let regained = frame(&mut app, SIZE, vec![egui::Event::WindowFocused(true)]);
        assert_eq!(regained.preferred, None);
        assert!(!regained.capture_allowed);
        let origin = native(&app).rect.center();
        frame(
            &mut app,
            SIZE,
            vec![
                egui::Event::PointerMoved(origin),
                button(origin, egui::PointerButton::Primary, true),
            ],
        );
        let moved = origin - egui::vec2(0.0, 50.0);
        let still_blocked = frame(&mut app, SIZE, vec![egui::Event::PointerMoved(moved)]);
        assert_eq!(still_blocked.preferred, None);
        assert!(!still_blocked.capture_allowed);
        let released = frames(
            &mut app,
            SIZE,
            vec![button(moved, egui::PointerButton::Primary, false)],
            true,
        );
        for pass in released {
            assert_eq!(pass.preferred, None);
            assert!(!pass.capture_allowed);
        }
        let automatic = settle(&mut app, SIZE);
        assert_automatic(&app, automatic);
        assert!(automatic.capture_allowed);
        resize(&mut app, SIZE, 390.0);
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn tools_automatic_height_real_release_while_hidden_allows_the_next_native_drag() {
    let (mut app, commands) = app(Tool::Search);
    let held = reset_while_primary_held(&mut app);
    tab_to(&mut app, SIZE, "close_tools");
    frame(&mut app, SIZE, key(egui::Key::Enter, egui::Modifiers::NONE));
    assert!(workspace_access_tests::recorded_response(&app, "close_tools").clicked());
    assert!(!app.tools_open);
    assert!(app.editor_ctx.input(|input| input.pointer.primary_down()));
    let hidden = frame(&mut app, SIZE, vec![]);
    assert!(hidden.panel.is_none() && !hidden.capture_allowed);
    let released = frames(
        &mut app,
        SIZE,
        vec![button(held, egui::PointerButton::Primary, false)],
        true,
    );
    assert_eq!(released.len(), 2);
    for pass in released {
        assert!(pass.panel.is_none());
        assert_eq!(pass.preferred, None);
        assert!(!pass.capture_allowed);
    }
    let ready = frame(&mut app, SIZE, vec![]);
    assert!(ready.panel.is_none() && ready.capture_allowed);
    click_widget(&mut app, SIZE, "sidebar_Search");
    let reopened = settle(&mut app, SIZE);
    assert!(app.tools_open && app.tool == Tool::Search);
    assert_automatic(&app, reopened);
    resize(&mut app, SIZE, 380.0);
    assert!(commands.try_recv().is_err());
}

fn begin_report(app: &mut CedarApp, commands: &Receiver<Command>) -> u64 {
    app.test_report.path = "TEST-automatic-height.xml".into();
    app.load_test_report();
    let command = commands.try_recv().expect("fixture's explicit report read");
    assert!(matches!(&command.op, Operation::Read { path } if path == "TEST-automatic-height.xml"));
    let load = app
        .test_report
        .loading
        .as_ref()
        .expect("pending report Load");
    assert_eq!(load.generation, app.generation);
    assert_eq!(load.path, "TEST-automatic-height.xml");
    assert!(
        matches!(app.pending.get(&command.id), Some(Job::TestReportRead(pending)) if pending == load)
    );
    assert!(commands.try_recv().is_err());
    command.id
}

fn finish_report(app: &mut CedarApp, id: u64) {
    let load = app
        .test_report
        .loading
        .clone()
        .expect("fixture's pending Load");
    let cases = (0..32)
        .map(|index| format!("<testcase name=\"case-{index}\" time=\"0.1\"/>"))
        .collect::<String>();
    let text = format!("<testsuite name=\"Automatic height\" tests=\"32\" failures=\"0\" errors=\"0\" skipped=\"0\" time=\"3.2\">{cases}</testsuite>");
    let parsed = test_reports::parse_report(&text).expect("strict-parser-valid reset fixture");
    assert_eq!(parsed.cases.len(), 32);
    assert_eq!(parsed.counts.total, 32);
    assert_eq!(parsed.counts.passed, 32);
    assert_eq!(parsed.duration_seconds, Some(3.2));
    let revision = format!("{:x}", Sha256::digest(text.as_bytes()));
    app.apply_event(Event {
        generation: app.generation,
        id,
        connected: true,
        result: Ok(Payload::File {
            path: load.path.clone(),
            text,
            revision: revision.clone(),
        }),
    });
    assert!(app.test_report.loading.is_none());
    let snapshot = app
        .test_report
        .snapshot
        .as_ref()
        .expect("validated reset fixture snapshot");
    assert_eq!(snapshot.source, load);
    assert_eq!(snapshot.revision, revision);
    assert_eq!(snapshot.report, parsed);
}

#[test]
fn tools_automatic_height_empty_loading_populated_and_clear_remain_automatic() {
    for phase in 0..3 {
        let (mut app, commands) = app(Tool::Tests);
        let empty_height = settle(&mut app, SIZE).panel.unwrap().height();
        let load_id = (phase > 0).then(|| begin_report(&mut app, &commands));
        if phase == 2 {
            finish_report(&mut app, load_id.unwrap());
        }
        settle(&mut app, SIZE);
        resize(&mut app, SIZE, 430.0);
        let pending = app.test_report.loading.clone();
        let snapshot = app.test_report.snapshot.as_ref().map(|snapshot| {
            (
                snapshot.source.clone(),
                snapshot.revision.clone(),
                snapshot.report.clone(),
            )
        });
        assert!(click_widget(&mut app, SIZE, AUTOMATIC).automatic_clicked);
        let automatic = settle(&mut app, SIZE);
        assert_automatic(&app, automatic);
        assert_eq!(
            app.test_report.loading, pending,
            "reset must not cancel a pending read"
        );
        assert_eq!(
            app.test_report.snapshot.as_ref().map(|snapshot| {
                (
                    snapshot.source.clone(),
                    snapshot.revision.clone(),
                    snapshot.report.clone(),
                )
            }),
            snapshot,
            "reset must not clear or replace a report"
        );
        if phase == 1 {
            finish_report(&mut app, load_id.unwrap());
            let populated = settle(&mut app, SIZE);
            assert_automatic(&app, populated);
        }
        // The existing Clear operation owns report state, independently of the
        // real reset button which was activated above. It has no test recorder.
        app.test_report.clear();
        let cleared = settle(&mut app, SIZE);
        assert_automatic(&app, cleared);
        assert!(app.test_report.loading.is_none() && app.test_report.snapshot.is_none());
        near(cleared.panel.unwrap().height(), empty_height);
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn tools_automatic_height_close_switch_disconnect_and_reconnect_retain_automatic_mode() {
    let (mut app, commands) = app(Tool::Search);
    let baseline = settle(&mut app, SIZE).panel.unwrap().height();
    resize(&mut app, SIZE, 430.0);
    click_widget(&mut app, SIZE, AUTOMATIC);
    settle(&mut app, SIZE);
    click_widget(&mut app, SIZE, "close_tools");
    assert!(!app.tools_open);
    assert_eq!(app.tools_height, None);
    assert!(settle(&mut app, SIZE).panel.is_none());
    click_widget(&mut app, SIZE, "sidebar_Search");
    let reopened = settle(&mut app, SIZE);
    assert!(app.tools_open && app.tool == Tool::Search);
    assert_automatic(&app, reopened);
    near(reopened.panel.unwrap().height(), baseline);
    for (tool, label) in [
        (Tool::Git, "GIT CHANGES"),
        (Tool::Run, "COMMANDS"),
        (Tool::Language, "LANGUAGE"),
        (Tool::Tests, "TEST RESULTS"),
        (Tool::Search, "PROJECT SEARCH"),
    ] {
        click_widget(&mut app, SIZE, label);
        let geometry = settle(&mut app, SIZE);
        assert!(app.tools_open && app.tool == tool);
        assert_automatic(&app, geometry);
    }
    assert!(commands.try_recv().is_err());
    app.disconnected("automatic-height fixture connection loss".into());
    let disconnected = settle(&mut app, SIZE);
    assert_automatic(&app, disconnected);
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
            root: "tools-automatic-height-fixture".into(),
            agent: None,
        }),
    });
    assert!(app.ready());
    let reconnected = settle(&mut app, SIZE);
    assert_automatic(&app, reconnected);
    // Only reconnection's existing initial directory read is permitted.
    for command in reconnected_commands.try_iter() {
        assert!(matches!(command.op, Operation::List { .. }));
    }
    resize(&mut app, SIZE, 365.0);
    assert!(reconnected_commands.try_recv().is_err());
    assert_eq!(CedarApp::empty().tools_height, None);
}

#[test]
fn tools_automatic_height_narrow_header_controls_remain_visible_and_clickable() {
    let labels = [
        "PROJECT SEARCH",
        "GIT CHANGES",
        "COMMANDS",
        "LANGUAGE",
        "TEST RESULTS",
        AUTOMATIC,
        "close_tools",
    ];
    for size in [SIZE, [780.0, 540.0]] {
        for font_size in [13.0, 18.0] {
            for tool in [Tool::Search, Tool::Tests] {
                let (mut app, commands) = app(tool);
                app.editor_ctx.style_mut(|style| {
                    style
                        .text_styles
                        .insert(egui::TextStyle::Button, FontId::proportional(font_size));
                    style
                        .text_styles
                        .insert(egui::TextStyle::Body, FontId::proportional(font_size));
                });
                settle(&mut app, size);
                // 430px may be window-clamped here; establish a smaller allocation
                // using the same real native divider as the wide-window cases.
                resize(&mut app, size, 320.0);
                let rects = labels.map(|name| visible_rect(&app, size, name));
                for left in 0..rects.len() {
                    for right in left + 1..rects.len() {
                        assert!(
                            !rects[left].intersects(rects[right]),
                            "{} overlaps {} at font {font_size}",
                            labels[left],
                            labels[right]
                        );
                    }
                }
                assert!(
                    rects[5].right() < rects[6].left(),
                    "close stays to the right of Automatic height"
                );
                near(rects[5].center().y, rects[6].center().y);
                let reset = click_widget(&mut app, size, AUTOMATIC);
                assert!(reset.automatic_clicked);
                let automatic = settle(&mut app, size);
                assert_automatic(&app, automatic);
                for name in labels {
                    visible_rect(&app, size, name);
                }
                click_widget(&mut app, size, "close_tools");
                assert!(!app.tools_open);
                assert_eq!(app.tools_height, None);
                assert!(settle(&mut app, size).panel.is_none());
                assert!(commands.try_recv().is_err());
            }
        }
    }
}

type SelectionWitness = (usize, bool, usize, bool);

#[derive(Debug, PartialEq)]
struct EditorWitness {
    text: String,
    saved: String,
    revision: Option<String>,
    version: u64,
    cursor: (usize, usize),
    selection: SelectionWitness,
    history: [Vec<(SelectionWitness, String)>; 2],
}

fn editor_witness(app: &CedarApp) -> EditorWitness {
    let selection = |range: egui::text::CCursorRange| {
        (
            range.primary.index,
            range.primary.prefer_next_row,
            range.secondary.index,
            range.secondary.prefer_next_row,
        )
    };
    let document = &app.documents[0];
    let state = egui::TextEdit::load_state(&app.editor_ctx, egui::Id::new(("editor", document.id)))
        .unwrap();
    let cursor = state.cursor.char_range().unwrap();
    let mut reachable: [Vec<(SelectionWitness, String)>; 2] = Default::default();
    for (index, redo) in [false, true].into_iter().enumerate() {
        // Inspect disposable native Undoer clones, preserving both cursor
        // affinities and every reachable state without changing editor history.
        let mut history = state.undoer();
        let mut current = (cursor, document.text.clone());
        for _ in 0..=editor_state::MAX_UNDO_STATES + 1 {
            let next = if redo {
                history.redo(&current)
            } else {
                history.undo(&current)
            }
            .cloned();
            let Some(next) = next else { break };
            reachable[index].push((selection(next.0), next.1.clone()));
            current = next;
        }
        assert!(reachable[index].len() <= editor_state::MAX_UNDO_STATES + 1);
    }
    EditorWitness {
        text: document.text.clone(),
        saved: document.saved_text.clone(),
        revision: document.revision.clone(),
        version: document.edit_version,
        cursor: document.cursor,
        selection: selection(cursor),
        history: reachable,
    }
}

#[test]
fn tools_automatic_height_preserves_dirty_selection_undo_redo_and_results_without_commands() {
    let (mut app, commands) = app(Tool::Tests);
    let saved = "saved alpha\nsecond line\n";
    let dirty = "dirty alpha\nsecond line\n";
    let later = "later alpha\nsecond line\n";
    app.documents.push(Document::new(
        1,
        "draft.txt".into(),
        saved.into(),
        "a".repeat(64),
    ));
    app.active_document = Some(1);
    app.next_document = 2;
    app.search_query = "retained query".into();
    app.search_results.push(SearchMatch {
        path: "draft.txt".into(),
        line: 2,
        text: "retained result".into(),
    });
    app.run_state.output = "retained run output".into();
    app.profiles.draft.program = "retained command".into();
    let report_id = begin_report(&mut app, &commands);
    finish_report(&mut app, report_id);
    settle(&mut app, SIZE);
    editor_state::commit(&app.editor_ctx, &mut app.documents[0], dirty.into(), 2);
    editor_state::commit(&app.editor_ctx, &mut app.documents[0], later.into(), 3);
    frame(&mut app, SIZE, key(egui::Key::Z, egui::Modifiers::COMMAND));
    assert_eq!(
        app.documents[0].text, dirty,
        "fixture must retain a real Redo branch"
    );
    let mut selected =
        egui::text::CCursorRange::two(egui::text::CCursor::new(2), egui::text::CCursor::new(9));
    selected.primary.prefer_next_row = true;
    selected.secondary.prefer_next_row = false;
    editor_state::move_selection(&app.editor_ctx, &mut app.documents[0], selected);
    settle(&mut app, SIZE);
    resize(&mut app, SIZE, 430.0);
    let before = editor_witness(&app);
    assert_ne!(before.selection.0, before.selection.2);
    assert!(!before.history[0].is_empty() && !before.history[1].is_empty());
    let report = app.test_report.snapshot.as_ref().unwrap();
    let report_before = (
        report.source.clone(),
        report.revision.clone(),
        report.report.clone(),
    );
    let search_before = serde_json::to_value(&app.search_results).unwrap();
    let navigation_before = app.navigation_epoch;
    assert!(commands.try_recv().is_err());
    assert!(click_widget(&mut app, SIZE, AUTOMATIC).automatic_clicked);
    let automatic = settle(&mut app, SIZE);
    assert_automatic(&app, automatic);
    assert_eq!(editor_witness(&app), before);
    resize(&mut app, SIZE, 410.0);
    tab_to(&mut app, SIZE, AUTOMATIC);
    assert!(frame(&mut app, SIZE, space()).automatic_clicked);
    let keyboard_reset = settle(&mut app, SIZE);
    assert_automatic(&app, keyboard_reset);
    assert_eq!(
        editor_witness(&app),
        before,
        "Space's native text companion must not touch the dirty editor"
    );
    assert!(app.documents[0].dirty());
    assert_eq!(app.active_document, Some(1));
    assert_eq!(app.navigation_epoch, navigation_before);
    let report = app.test_report.snapshot.as_ref().unwrap();
    assert_eq!(
        (
            report.source.clone(),
            report.revision.clone(),
            report.report.clone()
        ),
        report_before
    );
    assert_eq!(
        serde_json::to_value(&app.search_results).unwrap(),
        search_before
    );
    assert_eq!(app.search_query, "retained query");
    assert_eq!(app.run_state.output, "retained run output");
    assert_eq!(app.profiles.draft.program, "retained command");
    assert!(
        commands.try_recv().is_err(),
        "reset must dispatch no backend operation"
    );

    tab_to(&mut app, SIZE, "editor");
    frame(&mut app, SIZE, key(egui::Key::Z, egui::Modifiers::COMMAND));
    assert_eq!(app.documents[0].text, saved);
    frame(&mut app, SIZE, key(egui::Key::Y, egui::Modifiers::COMMAND));
    assert_eq!(app.documents[0].text, dirty);
    frame(&mut app, SIZE, key(egui::Key::Y, egui::Modifiers::COMMAND));
    assert_eq!(app.documents[0].text, later);
    assert!(
        commands.try_recv().is_err(),
        "local Undo/Redo must remain local"
    );
}
