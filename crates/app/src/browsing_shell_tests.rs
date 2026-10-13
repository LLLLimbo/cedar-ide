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

pub(super) fn fixture(font: f32) -> (CedarApp, Receiver<Command>) {
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
pub(super) fn settle(app: &mut CedarApp, size: [f32; 2]) {
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
pub(super) fn click(app: &mut CedarApp, size: [f32; 2], name: &str) {
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
    if name == "editor" {
        assert_eq!(
            app.editor_ctx.memory(|memory| memory.focused()),
            Some(workspace_access_tests::recorded_response(app, name).id),
            "native editor click must establish the selection owner"
        );
    }
}
pub(super) fn key(app: &mut CedarApp, size: [f32; 2], key: egui::Key, modifiers: egui::Modifiers) {
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

#[test]
fn diagnostic_workspace_escape_closes_menu_after_native_key() {
    let size = [1364.0, 1024.0];
    let (mut app, commands) = fixture(13.0);
    app.documents.clear();
    app.active_document = None;
    settle(&mut app, size);
    click(&mut app, size, "workspace_menu");
    let menu_painted = |output: &egui::FullOutput| {
        output.shapes.iter().any(|shape| {
            matches!(&shape.shape, egui::epaint::Shape::Text(draw)
            if draw.galley.text().contains("Save all editor buffers"))
        })
    };
    let before = frame(&mut app, size, vec![]);
    assert!(menu_painted(&before), "actual Workspace menu must be open");
    let ctx = app.editor_ctx.clone();
    let mut raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size.into())),
        events: vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: Some(egui::Key::Escape),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
        ..Default::default()
    };
    let escape = |events: &[egui::Event]| {
        events.iter().any(|event| {
            matches!(
                event,
                egui::Event::Key {
                    key: egui::Key::Escape,
                    pressed: true,
                    ..
                }
            )
        })
    };
    assert!(escape(&raw.events));
    eframe::App::raw_input_hook(&mut app, &ctx, &mut raw);
    let post_hook = escape(&raw.events);
    assert!(post_hook, "raw hook must retain the delivered Escape");
    let mut native = eframe::Frame::_new_kittest();
    let mut post_update = false;
    let pressed = ctx.run(raw, |ctx| {
        eframe::App::update(&mut app, ctx, &mut native);
        post_update = ctx.input(|input| escape(&input.events));
    });
    frame(
        &mut app,
        size,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: Some(egui::Key::Escape),
            pressed: false,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    let idle = frame(&mut app, size, vec![]);
    assert!(
        commands.try_recv().is_err(),
        "Escape must issue no backend work"
    );
    println!("workspace_escape_diagnostic post_hook={post_hook} post_update={post_update} painted_before={} painted_pressed={} painted_idle={}", menu_painted(&before), menu_painted(&pressed), menu_painted(&idle));
    assert!(
        !menu_painted(&idle),
        "delivered native Escape must dismiss Workspace after release and idle repaint"
    );
}

fn escape_event(pressed: bool) -> egui::Event {
    egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: Some(egui::Key::Escape),
        pressed,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}

fn header_menus(ctx: &egui::Context) -> HeaderMenus {
    ctx.data(|data| data.get_temp::<HeaderMenus>(HeaderMenus::key()).unwrap())
}

pub(super) fn open_header_root(ctx: &egui::Context) -> Option<egui::Id> {
    egui::menu::BarState::load(ctx, header_menus(ctx).bar)
        .as_ref()
        .map(|root| root.id)
}

fn menu_painted(output: &egui::FullOutput, menu: &str) -> bool {
    let text = if menu == "workspace_menu" {
        "Save all editor buffers · Ctrl/Cmd+Shift+S"
    } else {
        "Test Results"
    };
    output.shapes.iter().any(|shape| {
        matches!(&shape.shape, egui::epaint::Shape::Text(draw) if draw.galley.text() == text)
    })
}

pub(super) type DraftSnapshot = (
    String,
    String,
    u64,
    Option<u64>,
    (usize, bool, usize, bool),
    String,
);

pub(super) fn draft_snapshot(app: &CedarApp) -> DraftSnapshot {
    let doc = &app.documents[0];
    let selection = egui::TextEdit::load_state(&app.editor_ctx, egui::Id::new(("editor", doc.id)))
        .unwrap()
        .cursor
        .char_range()
        .unwrap();
    (
        doc.text.clone(),
        doc.saved_text.clone(),
        doc.edit_version,
        app.active_document,
        (
            selection.primary.index,
            selection.primary.prefer_next_row,
            selection.secondary.index,
            selection.secondary.prefer_next_row,
        ),
        format!(
            "{:?}/{:?}",
            app.location_history.back, app.location_history.forward
        ),
    )
}

#[test]
fn header_escape_closes_each_rendered_root_and_preserves_dirty_selection_and_native_undo() {
    let size = [1280.0, 900.0];
    for menu in ["workspace_menu", "tools_menu"] {
        let (mut app, commands) = fixture(13.0);
        settle(&mut app, size);
        click(&mut app, size, "editor");
        let before_edit = app.documents[0].text.clone();
        frame(
            &mut app,
            size,
            vec![egui::Event::Text(" native edit".into())],
        );
        assert_ne!(app.documents[0].text, before_edit);
        key(&mut app, size, egui::Key::ArrowLeft, egui::Modifiers::SHIFT);
        let snapshot = draft_snapshot(&app);
        let identity = (app.generation, app.navigation_epoch, app.language.session);
        click(&mut app, size, menu);
        let before = frame(&mut app, size, vec![]);
        let root = workspace_access_tests::recorded_response(&app, menu).id;
        assert_eq!(open_header_root(&app.editor_ctx), Some(root));
        assert!(menu_painted(&before, menu));
        let pressed = frame(&mut app, size, vec![escape_event(true)]);
        assert_eq!(open_header_root(&app.editor_ctx), None);
        assert!(!menu_painted(&pressed, menu));
        frame(&mut app, size, vec![escape_event(false)]);
        let idle = frame(&mut app, size, vec![]);
        assert!(!menu_painted(&idle, menu));
        assert_eq!(draft_snapshot(&app), snapshot);
        assert_eq!(
            (app.generation, app.navigation_epoch, app.language.session),
            identity
        );
        assert!(app.documents[0].dirty());
        assert!(commands.try_recv().is_err());

        click(&mut app, size, "editor");
        key(&mut app, size, egui::Key::Z, egui::Modifiers::COMMAND);
        assert_eq!(
            app.documents[0].text, before_edit,
            "Undo after {menu} Escape"
        );
        key(
            &mut app,
            size,
            egui::Key::Z,
            egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
        );
        assert_eq!(
            app.documents[0].text, snapshot.0,
            "Redo after {menu} Escape"
        );
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn header_escape_keeps_existing_no_menu_dismissal_and_only_dismisses_its_menu() {
    let size = [1280.0, 900.0];
    let (mut app, commands) = fixture(13.0);
    app.find_open = true;
    settle(&mut app, size);
    click(&mut app, size, "editor");
    let snapshot = draft_snapshot(&app);
    key(&mut app, size, egui::Key::Escape, egui::Modifiers::NONE);
    assert!(!app.find_open, "no-menu Escape still closes inline Find");
    assert_eq!(draft_snapshot(&app), snapshot);
    for menu in ["workspace_menu", "tools_menu"] {
        app.find_open = true;
        settle(&mut app, size);
        click(&mut app, size, menu);
        key(&mut app, size, egui::Key::Escape, egui::Modifiers::NONE);
        assert_eq!(open_header_root(&app.editor_ctx), None);
        assert!(
            app.find_open,
            "owned menu Escape does not also close inline Find"
        );
        key(&mut app, size, egui::Key::Escape, egui::Modifiers::NONE);
        assert!(!app.find_open);
        assert_eq!(draft_snapshot(&app), snapshot);
    }
    assert!(commands.try_recv().is_err());
}

#[test]
fn header_escape_rejects_stale_pass_viewport_context_and_unowned_root() {
    let size = [1280.0, 900.0];
    for invalid in [
        "missing", "stale", "overflow", "viewport", "context", "root", "bar",
    ] {
        let (mut app, commands) = fixture(13.0);
        settle(&mut app, size);
        click(&mut app, size, "editor");
        click(&mut app, size, "workspace_menu");
        // Leave the menu button so egui's ordinary hover switching cannot adopt
        // an intentionally substituted unrelated root in this adverse fixture.
        frame(
            &mut app,
            size,
            vec![egui::Event::PointerMoved(egui::pos2(1100.0, 800.0))],
        );
        let snapshot = draft_snapshot(&app);
        let ctx = app.editor_ctx.clone();
        let mut header = header_menus(&ctx);
        let actual_bar = header.bar;
        let mut bar = egui::menu::BarState::load(&ctx, actual_bar);
        let mut expected_root = bar.as_ref().unwrap().id;
        let other = egui::Id::new("unrelated_menu_root");
        match invalid {
            "missing" => {}
            "stale" => header.pass -= 1,
            "overflow" => header.pass = u64::MAX,
            "viewport" => header.viewport = egui::ViewportId::from_hash_of("other_viewport"),
            "context" => app.editor_ctx = egui::Context::default(),
            "root" => {
                bar.as_mut().unwrap().id = other;
                expected_root = other;
                bar.store(&ctx, actual_bar);
            }
            "bar" => {
                header.bar = egui::Id::new("unrelated_menu_bar");
                bar.as_mut().unwrap().id = other;
                bar.store(&ctx, header.bar);
            }
            _ => unreachable!(),
        }
        ctx.data_mut(|data| {
            if invalid == "missing" {
                data.remove::<HeaderMenus>(HeaderMenus::key());
            } else {
                data.insert_temp(HeaderMenus::key(), header);
            }
        });
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size.into())),
            events: vec![escape_event(true)],
            ..Default::default()
        };
        eframe::App::raw_input_hook(&mut app, &ctx, &mut input);
        let mut native = eframe::Frame::_new_kittest();
        let _ = ctx.run(input, |ctx| eframe::App::update(&mut app, ctx, &mut native));
        assert_eq!(
            egui::menu::BarState::load(&ctx, actual_bar)
                .as_ref()
                .map(|root| root.id),
            Some(expected_root),
            "{invalid} must not close an unproven menu"
        );
        if invalid == "bar" {
            assert_eq!(
                egui::menu::BarState::load(&ctx, header.bar)
                    .as_ref()
                    .map(|root| root.id),
                Some(other)
            );
        }
        app.editor_ctx = ctx;
        frame(&mut app, size, vec![escape_event(false)]);
        frame(&mut app, size, vec![]);
        assert_eq!(open_header_root(&app.editor_ctx), Some(expected_root));
        assert_eq!(draft_snapshot(&app), snapshot);
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn header_escape_closes_menu_while_ready_open_is_retired_without_replacing_dirty_editor() {
    let size = [1280.0, 900.0];
    for menu in ["workspace_menu", "tools_menu"] {
        let (mut app, commands) = fixture(13.0);
        app.next_document = 2;
        settle(&mut app, size);
        click(&mut app, size, "editor");
        app.open("queued.txt".into(), Some(1));
        let read = commands.try_recv().unwrap();
        assert!(matches!(&read.op, Operation::Read { path } if path == "queued.txt"));
        click(&mut app, size, menu);
        assert!(open_header_root(&app.editor_ctx).is_some());
        let snapshot = draft_snapshot(&app);
        let navigation = app.navigation_epoch;
        app.result_tx
            .send(WorkerEvent::Response(Event {
                generation: app.generation,
                id: read.id,
                connected: true,
                result: Ok(Payload::File {
                    path: "queued.txt".into(),
                    text: "must not open".into(),
                    revision: "b".repeat(64),
                }),
            }))
            .unwrap();
        key(&mut app, size, egui::Key::Escape, egui::Modifiers::NONE);
        assert_eq!(open_header_root(&app.editor_ctx), None);
        assert_eq!(app.navigation_epoch, navigation + 1);
        assert!(!app.pending.contains_key(&read.id));
        assert!(app.location_history.pending.is_none());
        // A completed ordinary Read is retained as an inactive buffer even
        // after its navigation is superseded. Escape must prevent activation,
        // focus transfer and history admission, while preserving that policy.
        assert_eq!(app.documents.len(), 2);
        let loaded = &app.documents[1];
        assert_eq!(loaded.id, 2);
        assert_eq!(loaded.path, "queued.txt");
        assert_eq!(loaded.text, "must not open");
        assert_eq!(loaded.saved_text, "must not open");
        assert_eq!(loaded.revision.as_deref(), Some("b".repeat(64).as_str()));
        assert!(!loaded.dirty());
        assert_ne!(
            app.editor_ctx.memory(|memory| memory.focused()),
            Some(egui::Id::new(("editor", loaded.id)))
        );
        assert_eq!(draft_snapshot(&app), snapshot);
        assert!(
            commands.try_recv().is_err(),
            "Escape must not issue another operation"
        );
    }
}
