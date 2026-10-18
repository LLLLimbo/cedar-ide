//! Production welcome layout, wheel and Tab events. No injected scroll/focus state.
use super::*;

const PROBE: &str = "welcome_layout_probe";
const NORMAL: [f32; 2] = [1180.0, 812.0];
const COMPACT: [f32; 2] = [780.0, 880.0];

#[derive(Clone, Debug)]
struct Geometry {
    id: egui::Id,
    viewport: egui::Rect,
    inner: egui::Rect,
    content: egui::Vec2,
    offset: egui::Vec2,
    restored_scroll_style: egui::style::ScrollStyle,
}

pub(super) fn record(
    ui: &egui::Ui,
    viewport: egui::Rect,
    scroll: &egui::scroll_area::ScrollAreaOutput<()>,
) {
    let geometry = Geometry {
        id: scroll.id,
        viewport,
        inner: scroll.inner_rect,
        content: scroll.content_size,
        offset: scroll.state.offset,
        restored_scroll_style: ui.spacing().scroll,
    };
    ui.ctx()
        .data_mut(|data| data.insert_temp(egui::Id::new(PROBE), geometry));
}

fn geometry(app: &CedarApp) -> Geometry {
    app.editor_ctx
        .data(|data| data.get_temp(egui::Id::new(PROBE)).unwrap())
}

fn fixture(zoom: f32, long_path: bool) -> (CedarApp, Receiver<Command>, impl FnOnce()) {
    let (mut app, commands) = browsing_shell_tests::fixture(13.0);
    app.documents.clear();
    app.active_document = None;
    if long_path {
        app.root = format!("/workspace/{}/project", "long-directory/".repeat(60));
    }
    let (fonts, hold_fonts) = system_fonts::SystemFonts::pending_failure_for_test();
    app.system_fonts = fonts;
    app.editor_ctx.set_zoom_factor(zoom);
    (app, commands, hold_fonts)
}

fn frame(app: &mut CedarApp, pixels: [f32; 2], events: Vec<egui::Event>) -> egui::FullOutput {
    let ctx = app.editor_ctx.clone();
    let points = egui::vec2(pixels[0], pixels[1]) / ctx.zoom_factor();
    let mut input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, points)),
        time: Some(ctx.cumulative_pass_nr() as f64 * 0.25),
        events,
        ..Default::default()
    };
    eframe::App::raw_input_hook(app, &ctx, &mut input);
    let mut native = eframe::Frame::_new_kittest();
    ctx.run(input, |ctx| eframe::App::update(app, ctx, &mut native))
}

fn settle(app: &mut CedarApp, pixels: [f32; 2]) -> egui::FullOutput {
    for _ in 0..5 {
        frame(app, pixels, vec![]);
    }
    frame(app, pixels, vec![])
}

fn wheel(app: &mut CedarApp, pixels: [f32; 2], down: bool) {
    let g = geometry(app);
    let at = g.inner.intersect(g.viewport).center();
    frame(
        app,
        pixels,
        vec![
            egui::Event::PointerMoved(at),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, if down { -10000.0 } else { 10000.0 }),
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    settle(app, pixels);
}

fn key(app: &mut CedarApp, pixels: [f32; 2], key: egui::Key, modifiers: egui::Modifiers) {
    for pressed in [true, false] {
        frame(
            app,
            pixels,
            vec![egui::Event::Key {
                key,
                physical_key: Some(key),
                pressed,
                repeat: false,
                modifiers,
            }],
        );
    }
    settle(app, pixels);
}

fn visible(app: &CedarApp, name: &str) {
    let rect = workspace_access_tests::recorded_rect(app, name);
    let clip = workspace_access_tests::recorded_clip(app, name);
    assert!(
        clip.contains_rect(rect),
        "{name} clipped: {rect:?} / {clip:?}"
    );
    assert!(
        geometry(app).viewport.contains_rect(rect),
        "{name} outside viewport: {rect:?} / {:?}",
        geometry(app)
    );
}

fn tab_to(app: &mut CedarApp, pixels: [f32; 2], name: &str) {
    let id = workspace_access_tests::recorded_response(app, name).id;
    for _ in 0..80 {
        if app.editor_ctx.memory(|memory| memory.focused()) == Some(id) {
            break;
        }
        key(app, pixels, egui::Key::Tab, egui::Modifiers::NONE);
    }
    assert_eq!(
        app.editor_ctx.memory(|memory| memory.focused()),
        Some(id),
        "Tab did not reach {name}"
    );
    visible(app, name);
}

fn click(app: &mut CedarApp, pixels: [f32; 2], name: &str) {
    visible(app, name);
    let pos = workspace_access_tests::recorded_rect(app, name).center();
    for pressed in [true, false] {
        frame(
            app,
            pixels,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    settle(app, pixels);
}

fn assert_idle_scrollbar(app: &CedarApp, output: &egui::FullOutput) {
    let g = geometry(app);
    let bar = app
        .editor_ctx
        .read_response(g.id.with(1usize))
        .expect("overflow scrollbar response");
    assert!(
        g.viewport.contains_rect(bar.rect),
        "scrollbar outside viewport: {:?} / {g:?}",
        bar.rect
    );
    assert!(
        output.shapes.iter().any(|shape| match &shape.shape {
            egui::Shape::Rect(rect) =>
                rect.rect == bar.rect
                    && rect.fill.a() > 0
                    && shape.clip_rect.contains_rect(bar.rect),
            _ => false,
        }),
        "overflow scrollbar track is invisible with the pointer outside: {:?}",
        bar.rect
    );
}

#[test]
fn normal_and_compact_unscaled_welcome_fit_without_scrolling() {
    for pixels in [NORMAL, COMPACT] {
        let (mut app, commands, _hold_fonts) = fixture(1.0, false);
        settle(&mut app, pixels);
        let g = geometry(&app);
        assert!(g.viewport.contains_rect(g.inner), "{g:?}");
        assert!(g.content.y <= g.inner.height(), "{g:?}");
        assert!(
            app.editor_ctx.read_response(g.id.with(1usize)).is_none(),
            "no scrollbar when content fits"
        );
        assert_eq!(
            g.restored_scroll_style,
            app.editor_ctx.style().spacing.scroll
        );
        for name in [
            "welcome_files",
            "welcome_text",
            "welcome_new",
            "welcome_footer",
        ] {
            visible(&app, name);
        }
        wheel(&mut app, pixels, true);
        assert_eq!(geometry(&app).offset, egui::Vec2::ZERO);
        assert!(commands.try_recv().is_err());
        eprintln!("WELCOME_FIT pixels={pixels:?} {g:?}");
    }
}

#[test]
fn scaled_compact_welcome_shows_idle_scrollbar_and_wheel_reaches_footer() {
    let (mut app, commands, _hold_fonts) = fixture(1.4, false);
    let output = settle(&mut app, COMPACT);
    let g = geometry(&app);
    assert_eq!(app.editor_ctx.pixels_per_point(), 1.4);
    assert!(
        g.content.y > g.inner.height(),
        "expected scaled overflow: {g:?}"
    );
    wheel(&mut app, COMPACT, true);
    visible(&app, "welcome_footer");
    let bottom = geometry(&app);
    assert!(bottom.offset.y > 0.0);
    wheel(&mut app, COMPACT, false);
    assert_eq!(geometry(&app).offset.y, 0.0);
    assert!(commands.try_recv().is_err());
    eprintln!("WELCOME_SCALED wheel reaches footer: top={g:?} bottom={bottom:?}");
    assert_idle_scrollbar(&app, &output);
}

#[test]
fn tiny_height_long_path_wheel_can_reveal_the_last_line() {
    let pixels = [780.0, 180.0];
    let (mut app, commands, _hold_fonts) = fixture(1.4, true);
    settle(&mut app, pixels);
    let g = geometry(&app);
    wheel(&mut app, pixels, true);
    let bottom = geometry(&app);
    eprintln!("WELCOME_TINY top={g:?} bottom={bottom:?}");
    assert!(
        g.viewport.contains_rect(g.inner),
        "scroll viewport expands past available height: {g:?}"
    );
    visible(&app, "welcome_footer");
    assert!(commands.try_recv().is_err());
}

#[test]
fn tab_reveals_each_welcome_action_below_a_long_path() {
    let pixels = [780.0, 540.0];
    let (mut app, commands, _hold_fonts) = fixture(1.4, true);
    settle(&mut app, pixels);
    for name in ["welcome_files", "welcome_text", "welcome_new"] {
        tab_to(&mut app, pixels, name);
        eprintln!("WELCOME_TAB {name} {:?}", geometry(&app));
        visible(&app, name);
    }
    for name in ["welcome_text", "welcome_files"] {
        key(&mut app, pixels, egui::Key::Tab, egui::Modifiers::SHIFT);
        let id = workspace_access_tests::recorded_response(&app, name).id;
        assert_eq!(app.editor_ctx.memory(|memory| memory.focused()), Some(id));
        visible(&app, name);
    }
    let focused = app.editor_ctx.memory(|memory| memory.focused());
    wheel(&mut app, pixels, false);
    assert_eq!(geometry(&app).offset.y, 0.0);
    settle(&mut app, pixels);
    assert_eq!(
        geometry(&app).offset.y,
        0.0,
        "idle focus must not pull back a deliberate wheel scroll"
    );
    assert_eq!(app.editor_ctx.memory(|memory| memory.focused()), focused);
    assert!(commands.try_recv().is_err());
}

#[test]
fn welcome_actions_keep_pointer_and_keyboard_activation_after_scroll() {
    for pointer in [false, true] {
        for name in ["welcome_files", "welcome_text", "welcome_new"] {
            let pixels = [780.0, 540.0];
            let (mut app, commands, _hold_fonts) = fixture(1.4, true);
            settle(&mut app, pixels);
            if pointer {
                wheel(&mut app, pixels, true);
                click(&mut app, pixels, name);
            } else {
                tab_to(&mut app, pixels, name);
                key(&mut app, pixels, egui::Key::Enter, egui::Modifiers::NONE);
            }
            if name == "welcome_new" {
                assert!(app.new_file);
                assert!(!app.browse_marker_open());
            } else {
                assert!(app.browse_marker_open());
                assert!(!app.new_file);
                key(&mut app, pixels, egui::Key::Escape, egui::Modifiers::NONE);
                assert!(!app.browse_marker_open());
                // Dismissal must allow another real click on the same action.
                wheel(&mut app, pixels, true);
                click(&mut app, pixels, name);
                assert!(app.browse_marker_open());
            }
            assert!(app.documents.is_empty());
            assert!(
                commands.try_recv().is_err(),
                "opening an action must not issue a backend request"
            );
        }
    }
}

#[test]
fn resize_from_overflow_back_to_normal_clamps_offset_and_removes_scrollbar() {
    let (mut app, commands, _hold_fonts) = fixture(1.4, false);
    settle(&mut app, COMPACT);
    wheel(&mut app, COMPACT, true);
    assert!(geometry(&app).offset.y > 0.0);
    app.editor_ctx.set_zoom_factor(1.0);
    settle(&mut app, NORMAL);
    let g = geometry(&app);
    assert_eq!(g.offset.y, 0.0);
    assert_eq!(
        g.restored_scroll_style,
        app.editor_ctx.style().spacing.scroll
    );
    assert!(app.editor_ctx.read_response(g.id.with(1usize)).is_none());
    for name in [
        "welcome_files",
        "welcome_text",
        "welcome_new",
        "welcome_footer",
    ] {
        visible(&app, name);
    }
    assert!(commands.try_recv().is_err());
}
