//! Actual production frames check compact shell geometry and the moved menus.
//! Pointer / native Tab activation uses the rendered responses, never injected focus.
use super::*;

const ESSENTIAL: [&str; 5] = [
    "workspace_menu",
    "browse_entry",
    "project_text_entry",
    "find_results_entry",
    "tools_menu",
];
const LEGACY: [(Tool, &str); 4] = [
    (Tool::Git, "Git Changes"),
    (Tool::Run, "Commands"),
    (Tool::Language, "Language"),
    (Tool::Tests, "Test Results"),
];

fn fixture(font: f32) -> (CedarApp, Receiver<Command>) {
    let mut app = CedarApp::empty();
    let form = ConnectForm {
        local_root: "/projects/browsing-shell".into(),
        ..Default::default()
    };
    app.workspace_key = Some(form.key());
    app.active_form = Some(form);
    app.root = "/projects/browsing-shell".into();
    app.state = ConnectionState::Ready;
    app.agent_info = Some(agent_support::full_test_agent());
    app.open_form = false;
    app.tool = Tool::Search;
    app.tools_open = false;
    app.documents.push(Document::new(
        1,
        "origin.txt".into(),
        "saved 雪".into(),
        "a".repeat(64),
    ));
    app.documents[0].text = "precious unsaved draft 雪".into();
    app.active_document = Some(1);
    app.search_query = "retained query".into();
    app.search_scope = "src".into();
    app.search_masks = "*.rs".into();
    app.editor_ctx.style_mut(|style| {
        style.spacing.item_spacing = egui::vec2(9.0, 8.0);
        style.spacing.button_padding = egui::vec2(10.0, 6.0);
        style
            .text_styles
            .insert(egui::TextStyle::Body, FontId::proportional(font));
        style
            .text_styles
            .insert(egui::TextStyle::Button, FontId::proportional(font));
    });
    let (worker, commands) = Worker::recording();
    app.worker = Some(worker);
    (app, commands)
}
fn frame(app: &mut CedarApp, size: [f32; 2], events: Vec<egui::Event>) -> egui::FullOutput {
    let ctx = app.editor_ctx.clone();
    let mut input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size.into())),
        events,
        ..Default::default()
    };
    eframe::App::raw_input_hook(app, &ctx, &mut input);
    let mut native = eframe::Frame::_new_kittest();
    ctx.run(input, |ctx| eframe::App::update(app, ctx, &mut native))
}
fn settle(app: &mut CedarApp, size: [f32; 2]) {
    for _ in 0..4 {
        frame(app, size, vec![]);
    }
}
fn visible(app: &CedarApp, size: [f32; 2], name: &str) -> egui::Rect {
    let rect = workspace_access_tests::recorded_rect(app, name);
    let clip = workspace_access_tests::recorded_clip(app, name);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size.into());
    assert!(
        rect.is_positive() && screen.contains_rect(rect),
        "{name} outside screen: {rect:?}"
    );
    assert!(
        clip.contains_rect(rect),
        "{name} clipped: {rect:?} / {clip:?}"
    );
    rect
}
fn click(app: &mut CedarApp, size: [f32; 2], name: &str) {
    let at = visible(app, size, name).center();
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
fn tab_to(app: &mut CedarApp, size: [f32; 2], name: &str) {
    let target = workspace_access_tests::recorded_response(app, name).id;
    for _ in 0..80 {
        if app.editor_ctx.memory(|memory| memory.focused()) == Some(target) {
            return;
        }
        key(app, size, egui::Key::Tab, egui::Modifiers::NONE);
    }
    panic!("native Tab never reached {name}");
}
fn unchanged(app: &CedarApp, commands: &Receiver<Command>) {
    assert_eq!(app.documents[0].text, "precious unsaved draft 雪");
    assert_eq!(app.documents[0].saved_text, "saved 雪");
    assert!(app.documents[0].dirty());
    assert_eq!(app.active_document, Some(1));
    assert_eq!(app.search_query, "retained query");
    assert_eq!(app.search_scope, "src");
    assert_eq!(app.search_masks, "*.rs");
    assert!(
        commands.try_recv().is_err(),
        "shell transition issued backend work"
    );
}

#[test]
fn compact_shell_primary_entries_and_closed_menu_status_fit_at_both_fonts() {
    for size in [[780.0, 540.0], [1280.0, 900.0]] {
        for font in [13.0, 18.0] {
            let (mut app, commands) = fixture(font);
            settle(&mut app, size);
            let rects = ESSENTIAL.map(|name| visible(&app, size, name));
            for (index, rect) in rects.iter().enumerate() {
                for other in &rects[index + 1..] {
                    assert!(!rect.intersects(*other));
                }
            }
            let header =
                egui::containers::panel::PanelState::load(&app.editor_ctx, egui::Id::new("header"))
                    .unwrap()
                    .rect;
            assert_eq!(header.height(), if font > 15.0 { 48.0 } else { 40.0 });
            let back = visible(&app, size, "Back");
            let forward = visible(&app, size, "Forward");
            let tab = visible(&app, size, "editor_tab:1");
            assert!(back.right() < forward.left() && forward.right() < tab.left());
            assert!(
                back.y_range().intersects(tab.y_range())
                    && forward.y_range().intersects(tab.y_range()),
                "history controls left the tab band"
            );
            visible(&app, size, "connection_status");
            visible(&app, size, "recovery_status");
            let output = frame(&mut app, size, vec![]);
            for text in ["Connected", "UNSAVED"] {
                assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::epaint::Shape::Text(draw) if draw.galley.text() == text && shape.clip_rect.contains_rect(draw.visual_bounding_rect()))), "closed shell hides {text}");
            }
            app.error = Some("A visible workspace error".into());
            settle(&mut app, size);
            visible(&app, size, "error_status");
            visible(&app, size, "recovery_status");
            unchanged(&app, &commands);
        }
    }
}

#[test]
fn workspace_and_tools_menus_remain_visible_and_pointer_reachable_after_resize() {
    for font in [13.0, 18.0] {
        let (mut app, commands) = fixture(font);
        for size in [[1280.0, 900.0], [780.0, 540.0], [1280.0, 900.0]] {
            settle(&mut app, size);
            click(&mut app, size, "workspace_menu");
            for name in [
                "workspace_open",
                "workspace_reconnect",
                "workspace_recovery",
                "workspace_save",
                "workspace_save_all",
                "workspace_cancel_saves",
                "workspace_copy_draft",
                "workspace_loaded_files",
            ] {
                visible(&app, size, name);
            }
            // Clicking the menu button closes it without invoking an action.
            click(&mut app, size, "workspace_menu");
            for (tool, label) in LEGACY {
                click(&mut app, size, "tools_menu");
                let item = format!("tool_menu_{label}");
                visible(&app, size, &item);
                click(&mut app, size, &item);
                assert!(app.tools_open && app.tool == tool);
                // Menus remain in the header even when a tall Language body uses
                // the Project viewport. Repeated selection preserves the panel.
                click(&mut app, size, "tools_menu");
                click(&mut app, size, &item);
                assert!(app.tools_open && app.tool == tool);
                unchanged(&app, &commands);
            }
            click(&mut app, size, "find_results_entry");
            assert!(app.tools_open && app.tool == Tool::Search);
            unchanged(&app, &commands);
        }
    }
}

#[test]
fn long_workspace_identity_keeps_every_menu_action_reachable() {
    let size = [780.0, 540.0];
    for font in [13.0, 18.0] {
        let (mut app, commands) = fixture(font);
        app.root = format!(
            "/projects/{}/workspace",
            "a-long-directory-name/".repeat(120)
        );
        // Produce a real, bounded completion status without sending a Write.
        app.queue_save_all();
        app.cancel_save_all();
        settle(&mut app, size);
        assert!(app.save_all_message().unwrap().contains("Cancelled"));
        click(&mut app, size, "workspace_menu");
        for name in [
            "workspace_open",
            "workspace_reconnect",
            "workspace_recovery",
            "workspace_save",
            "workspace_save_all",
            "workspace_copy_draft",
            "workspace_loaded_files",
        ] {
            tab_to(&mut app, size, name);
            visible(&app, size, name);
        }
        let summary = app.save_all_message().unwrap();
        let output = frame(&mut app, size, vec![]);
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::epaint::Shape::Text(draw) if draw.galley.text() == summary && shape.clip_rect.contains_rect(draw.visual_bounding_rect()))), "full Save All status is not visible with the lower actions");
        unchanged(&app, &commands);
    }
}

#[test]
fn moved_tool_menu_uses_native_tab_enter_and_keeps_dirty_editor_state() {
    for font in [13.0, 18.0] {
        let size = [780.0, 540.0];
        let (mut app, commands) = fixture(font);
        settle(&mut app, size);
        for (tool, label) in LEGACY {
            tab_to(&mut app, size, "tools_menu");
            key(&mut app, size, egui::Key::Enter, egui::Modifiers::NONE);
            let item = format!("tool_menu_{label}");
            visible(&app, size, &item);
            tab_to(&mut app, size, "tool_menu_Git Changes");
            for (_, next) in &LEGACY[1..] {
                key(&mut app, size, egui::Key::Tab, egui::Modifiers::NONE);
                assert_eq!(
                    app.editor_ctx.memory(|memory| memory.focused()),
                    Some(
                        workspace_access_tests::recorded_response(
                            &app,
                            &format!("tool_menu_{next}")
                        )
                        .id
                    )
                );
                visible(&app, size, &format!("tool_menu_{next}"));
            }
            for (_, previous) in LEGACY[..3].iter().rev() {
                key(&mut app, size, egui::Key::Tab, egui::Modifiers::SHIFT);
                assert_eq!(
                    app.editor_ctx.memory(|memory| memory.focused()),
                    Some(
                        workspace_access_tests::recorded_response(
                            &app,
                            &format!("tool_menu_{previous}")
                        )
                        .id
                    )
                );
                visible(&app, size, &format!("tool_menu_{previous}"));
            }
            tab_to(&mut app, size, &item);
            key(&mut app, size, egui::Key::Enter, egui::Modifiers::NONE);
            assert!(app.tools_open && app.tool == tool);
            unchanged(&app, &commands);
        }
    }
}

#[test]
fn connected_welcome_opens_the_existing_project_files_provider_without_a_request() {
    let size = [780.0, 540.0];
    let (mut app, commands) = fixture(13.0);
    app.documents.clear();
    app.active_document = None;
    app.file_discovery = Some(cedar_protocol::FileDiscoveryDescriptor::V1);
    settle(&mut app, size);
    click(&mut app, size, "welcome_files");
    assert!(app.browse_marker_open());
    let output = frame(&mut app, size, vec![]);
    assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::epaint::Shape::Text(draw) if draw.galley.text().starts_with("Project files · explicit filename search"))));
    assert!(commands.try_recv().is_err());
}
