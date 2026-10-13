use super::*;
use crate::*;

fn fixture(path: &str, mode: Mode) -> (CedarApp, Receiver<Command>) {
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
    app.documents
        .push(Document::new(1, path.into(), "saved".into(), "r0".into()));
    app.active_document = Some(1);
    app.next_document = 2;
    app.explorer_set_mode(mode);
    let (worker, commands) = Worker::recording();
    app.worker = Some(worker);
    (app, commands)
}

fn entry(path: &str, is_dir: bool) -> Entry {
    Entry {
        name: path.rsplit('/').next().unwrap().into(),
        path: path.into(),
        is_dir,
    }
}

fn frame(app: &mut CedarApp, events: Vec<egui::Event>) {
    frame_at(app, [900.0, 740.0], events);
}

fn frame_at(app: &mut CedarApp, size: [f32; 2], events: Vec<egui::Event>) {
    let ctx = app.editor_ctx.clone();
    let mut input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size.into())),
        events,
        ..Default::default()
    };
    let mut native = eframe::Frame::_new_kittest();
    eframe::App::raw_input_hook(app, &ctx, &mut input);
    let _ = ctx.run(input, |ctx| eframe::App::update(app, ctx, &mut native));
}

fn settle(app: &mut CedarApp) {
    for _ in 0..5 {
        frame(app, vec![]);
    }
}

fn click(app: &mut CedarApp, name: &str) {
    let rect = workspace_access_tests::recorded_rect(app, name);
    for pressed in [true, false] {
        frame(
            app,
            vec![
                egui::Event::PointerMoved(rect.center()),
                egui::Event::PointerButton {
                    pos: rect.center(),
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
}

fn start(app: &mut CedarApp) {
    let ctx = app.editor_ctx.clone();
    app.start_project_reveal(&ctx);
    frame(app, vec![]);
}

fn reply(app: &mut CedarApp, command: Command, result: Result<Payload, String>) {
    app.apply_worker_event(WorkerEvent::Response(Event {
        generation: app.generation,
        id: command.id,
        connected: true,
        result,
    }));
}

fn listed(app: &mut CedarApp, command: Command, entries: Vec<Entry>) {
    reply(app, command, Ok(Payload::Entries { entries }));
}

fn expect_list(commands: &Receiver<Command>, path: &str) -> Command {
    let command = commands.try_recv().expect("expected one exact List");
    assert!(matches!(&command.op, Operation::List { path: actual } if actual == path));
    assert!(
        commands.try_recv().is_err(),
        "only one List may be outstanding"
    );
    command
}

#[test]
fn project_reveal_flat_actual_button_loads_only_parent_and_focuses_exact_visible_file() {
    let (mut app, commands) = fixture("src/current.rs", Mode::Flat);
    settle(&mut app);
    click(&mut app, "project_select_current");
    let command = expect_list(&commands, "src");
    listed(
        &mut app,
        command,
        vec![entry("src/other.rs", false), entry("src/current.rs", false)],
    );
    settle(&mut app);
    let response = workspace_access_tests::recorded_response(&app, "current.rs");
    assert!(response.has_focus());
    assert!(response
        .rect
        .intersect(workspace_access_tests::recorded_clip(&app, "current.rs"))
        .is_positive());
    assert_eq!(app.explorer.mode, Mode::Flat);
    assert_eq!(app.explorer.selected, "src/current.rs");
    assert!(!app.project_reveal_pending());
    assert!(commands.try_recv().is_err());
}

#[test]
fn project_reveal_tree_lists_only_exact_ancestors_then_cached_reveal_has_no_requests() {
    let (mut app, commands) = fixture("one/two/current.rs", Mode::Tree);
    settle(&mut app);
    click(&mut app, "project_select_current");
    for (path, child, is_dir) in [
        ("", "one", true),
        ("one", "one/two", true),
        ("one/two", "one/two/current.rs", false),
    ] {
        let command = expect_list(&commands, path);
        listed(&mut app, command, vec![entry(child, is_dir)]);
        assert!(
            commands.try_recv().is_err(),
            "reply polling never dispatches the next List"
        );
        settle(&mut app);
    }
    assert_eq!(app.explorer.mode, Mode::Tree);
    assert_eq!(app.explorer.selected, "one/two/current.rs");
    assert!(workspace_access_tests::recorded_response(&app, "current.rs").has_focus());
    click(&mut app, "project_select_current");
    settle(&mut app);
    assert!(!app.project_reveal_pending());
    assert!(commands.try_recv().is_err());
}

#[test]
fn project_reveal_busy_has_clear_feedback_and_does_not_queue() {
    let (mut app, commands) = fixture("src/current.rs", Mode::Tree);
    app.explorer_refresh("");
    let outstanding = expect_list(&commands, "");
    start(&mut app);
    assert!(!app.project_reveal_pending());
    assert!(app.explorer.message.as_deref().unwrap().contains("busy"));
    listed(&mut app, outstanding, vec![entry("src", true)]);
    settle(&mut app);
    assert!(commands.try_recv().is_err());
}

#[test]
fn project_reveal_missing_or_wrong_kind_never_selects_a_substitute() {
    for entries in [vec![entry("other", true)], vec![entry("src", false)]] {
        let (mut app, commands) = fixture("src/current.rs", Mode::Tree);
        start(&mut app);
        let command = expect_list(&commands, "");
        listed(&mut app, command, entries);
        settle(&mut app);
        assert!(!app.project_reveal_pending());
        assert!(app.explorer.message.as_deref().unwrap().contains("missing"));
        assert_ne!(app.explorer.selected, "src/current.rs");
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn project_reveal_error_wrong_payload_and_overbudget_reply_stop_continuation() {
    for result in [
        Err("directory access refused".into()),
        Ok(Payload::Written {
            revision: "wrong-payload".into(),
        }),
        Ok(Payload::Entries {
            entries: vec![entry("src/deeper", true)],
        }),
        Ok(Payload::Entries {
            entries: (0..=MAX_ROWS)
                .map(|n| entry(&format!("f{n}"), false))
                .collect(),
        }),
    ] {
        let (mut app, commands) = fixture("src/current.rs", Mode::Tree);
        start(&mut app);
        let command = expect_list(&commands, "");
        reply(&mut app, command, result);
        settle(&mut app);
        assert!(!app.project_reveal_pending());
        assert!(app.explorer.message.as_deref().unwrap().contains("failed"));
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn project_reveal_cancel_keeps_list_slot_and_late_ordinary_cache_without_next_request() {
    let (mut app, commands) = fixture("src/current.rs", Mode::Tree);
    settle(&mut app);
    click(&mut app, "project_select_current");
    let command = expect_list(&commands, "");
    click(&mut app, "project_reveal_cancel");
    assert!(!app.project_reveal_pending());
    assert!(app.explorer_busy());
    listed(&mut app, command, vec![entry("src", true)]);
    settle(&mut app);
    assert!(app
        .explorer_visible_rows()
        .iter()
        .any(|row| row.entry.path == "src"));
    assert!(!app.explorer_busy());
    assert!(commands.try_recv().is_err());
    assert_ne!(
        app.editor_ctx.memory(|memory| memory.focused()),
        Some(row_id("src/current.rs"))
    );
}

#[test]
fn project_reveal_original_mixed_input_cancels_before_a_ready_reply_can_continue() {
    let (mut app, commands) = fixture("src/current.rs", Mode::Tree);
    start(&mut app);
    let command = expect_list(&commands, "");
    let ctx = app.editor_ctx.clone();
    let mut input = egui::RawInput {
        events: vec![
            egui::Event::Text("x".into()),
            egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: Some(egui::Key::Escape),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            },
        ],
        ..Default::default()
    };
    eframe::App::raw_input_hook(&mut app, &ctx, &mut input);
    assert!(!app.project_reveal_pending());
    listed(&mut app, command, vec![entry("src", true)]);
    settle(&mut app);
    assert!(commands.try_recv().is_err());
}

#[test]
fn project_reveal_releases_and_pointer_motion_do_not_cancel_but_new_press_does() {
    let (mut app, commands) = fixture("src/current.rs", Mode::Tree);
    start(&mut app);
    let _command = expect_list(&commands, "");
    let release = egui::Event::Key {
        key: egui::Key::Enter,
        physical_key: Some(egui::Key::Enter),
        pressed: false,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    app.capture_project_reveal_input(&egui::RawInput {
        events: vec![release, egui::Event::PointerMoved(egui::pos2(22.0, 33.0))],
        ..Default::default()
    });
    assert!(app.project_reveal_pending());
    app.capture_project_reveal_input(&egui::RawInput {
        events: vec![egui::Event::Paste("new".into())],
        ..Default::default()
    });
    assert!(!app.project_reveal_pending());
    assert!(app.explorer_busy());
}

#[test]
fn project_reveal_document_path_version_navigation_mode_and_connection_changes_invalidate() {
    for change in 0..6 {
        let (mut app, commands) = fixture("src/current.rs", Mode::Tree);
        start(&mut app);
        let command = expect_list(&commands, "");
        match change {
            0 => app.active_document = None,
            1 => app.documents[0].path = "elsewhere.rs".into(),
            2 => app.documents[0].edit_version += 1,
            3 => app.navigation_changed(),
            4 => app.explorer_set_mode(Mode::Flat),
            _ => app.generation += 1,
        }
        listed(&mut app, command, vec![entry("src", true)]);
        settle(&mut app);
        assert!(!app.project_reveal_pending(), "identity change {change}");
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn project_reveal_manual_selection_or_collapse_cancels_even_without_native_input() {
    for collapse in [false, true] {
        let (mut app, commands) = fixture("src/current.rs", Mode::Tree);
        start(&mut app);
        let command = expect_list(&commands, "");
        if collapse {
            app.explorer_collapse("");
        } else {
            app.explorer_select("");
        }
        listed(&mut app, command, vec![entry("src", true)]);
        settle(&mut app);
        assert!(!app.project_reveal_pending());
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn project_reveal_fixed_deadline_stops_waiting_continuation_and_final_focus() {
    for after_reply in [false, true] {
        let (mut app, commands) = fixture("current.rs", Mode::Tree);
        start(&mut app);
        let command = expect_list(&commands, "");
        let original = app.explorer.reveal.as_ref().unwrap().deadline;
        if after_reply {
            listed(&mut app, command, vec![entry("current.rs", false)]);
            assert_eq!(app.explorer.reveal.as_ref().unwrap().deadline, original);
        }
        app.explorer.reveal.as_mut().unwrap().deadline = Instant::now() - Duration::from_millis(1);
        settle(&mut app);
        assert!(!app.project_reveal_pending());
        assert!(app
            .explorer
            .message
            .as_deref()
            .unwrap()
            .contains("deadline"));
        assert_ne!(
            app.editor_ctx.memory(|memory| memory.focused()),
            Some(row_id("current.rs"))
        );
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn project_reveal_request_depth_and_path_limits_are_bounded() {
    assert_eq!(MAX_PROJECT_REVEAL_LISTS, 32);
    assert_eq!(PROJECT_REVEAL_DEADLINE, Duration::from_secs(30));
    for (path, mode) in [
        ("a/".repeat(32) + "file", Mode::Tree),
        ("a".repeat(MAX_PATH_BYTES + 1), Mode::Flat),
        ("../file".into(), Mode::Flat),
    ] {
        let (mut app, commands) = fixture(&path, mode);
        start(&mut app);
        assert!(!app.project_reveal_pending());
        assert!(commands.try_recv().is_err());
    }
    let (mut app, commands) = fixture("src/current.rs", Mode::Tree);
    let ctx = app.editor_ctx.clone();
    app.start_project_reveal(&ctx);
    app.explorer.reveal.as_mut().unwrap().requests = MAX_PROJECT_REVEAL_LISTS;
    settle(&mut app);
    assert!(!app.project_reveal_pending());
    assert!(app
        .explorer
        .message
        .as_deref()
        .unwrap()
        .contains("32-directory"));
    assert!(commands.try_recv().is_err());
}

#[test]
fn project_reveal_flat_cached_exact_file_needs_zero_requests_and_missing_does_not_fallback() {
    for present in [true, false] {
        let (mut app, commands) = fixture("current.rs", Mode::Flat);
        app.list(String::new());
        let command = expect_list(&commands, "");
        listed(
            &mut app,
            command,
            vec![entry(
                if present { "current.rs" } else { "other.rs" },
                false,
            )],
        );
        settle(&mut app);
        click(&mut app, "project_select_current");
        settle(&mut app);
        assert!(!app.project_reveal_pending());
        assert!(commands.try_recv().is_err());
        assert_eq!(
            app.explorer
                .message
                .as_deref()
                .unwrap()
                .contains("selected in Project"),
            present
        );
    }
}

#[test]
fn project_reveal_clipped_exact_row_scrolls_into_view_before_success() {
    let (mut app, commands) = fixture("z_current.rs", Mode::Tree);
    start(&mut app);
    let command = expect_list(&commands, "");
    let mut entries = (0..180)
        .map(|i| entry(&format!("a{i:03}.rs"), false))
        .collect::<Vec<_>>();
    entries.push(entry("z_current.rs", false));
    listed(&mut app, command, entries);
    settle(&mut app);
    let response = workspace_access_tests::recorded_response(&app, "z_current.rs");
    assert!(response.has_focus());
    assert!(response
        .rect
        .intersect(workspace_access_tests::recorded_clip(&app, "z_current.rs"))
        .is_positive());
    assert!(!app.project_reveal_pending());
    assert!(commands.try_recv().is_err());
}

#[test]
fn project_reveal_dirty_document_and_retained_search_are_unchanged() {
    let (mut app, commands) = fixture("src/current.rs", Mode::Flat);
    app.documents[0].text = "dirty selection 雪".into();
    app.documents[0].edit_version = 4;
    app.search_query = "retained needle".into();
    app.search_scope = "retained/scope".into();
    let before = (
        app.documents[0].text.clone(),
        app.documents[0].edit_version,
        app.documents[0].saved_text.clone(),
        app.search_query.clone(),
        app.search_scope.clone(),
        app.navigation_epoch,
    );
    start(&mut app);
    let command = expect_list(&commands, "src");
    listed(&mut app, command, vec![entry("src/current.rs", false)]);
    settle(&mut app);
    assert_eq!(
        (
            app.documents[0].text.clone(),
            app.documents[0].edit_version,
            app.documents[0].saved_text.clone(),
            app.search_query.clone(),
            app.search_scope.clone(),
            app.navigation_epoch
        ),
        before
    );
    assert!(commands.try_recv().is_err());
}

#[test]
fn project_reveal_discarded_pass_cannot_dispatch_or_finish_visible_target() {
    for ready in [false, true] {
        let (mut app, commands) = fixture("current.rs", Mode::Tree);
        settle(&mut app);
        if ready {
            app.explorer_refresh("");
            let command = expect_list(&commands, "");
            listed(&mut app, command, vec![entry("current.rs", false)]);
            settle(&mut app);
        }
        let ctx = app.editor_ctx.clone();
        let before_focus = ctx.memory(|memory| memory.focused());
        app.start_project_reveal(&ctx);
        app.explorer.reveal.as_mut().unwrap().ready = ready;
        ctx.options_mut(|options| options.max_passes = std::num::NonZeroUsize::new(1).unwrap());
        let mut native = eframe::Frame::_new_kittest();
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 740.0),
            )),
            ..Default::default()
        };
        eframe::App::raw_input_hook(&mut app, &ctx, &mut input);
        let _ = ctx.run(input, |ctx| {
            ctx.request_discard("Project reveal one-pass sizing boundary");
            eframe::App::update(&mut app, ctx, &mut native);
        });
        assert!(app.project_reveal_pending());
        assert!(commands.try_recv().is_err());
        assert_eq!(ctx.memory(|memory| memory.focused()), before_focus);
        assert_ne!(
            app.explorer.message.as_deref(),
            Some("Current file selected in Project")
        );
    }
}

#[test]
fn project_reveal_keyboard_button_uses_original_clean_activation_guard() {
    for mixed in [false, true] {
        let (mut app, commands) = fixture("current.rs", Mode::Flat);
        settle(&mut app);
        workspace_access_tests::recorded_response(&app, "project_select_current").request_focus();
        frame(&mut app, vec![]);
        let mut events = vec![
            egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: Some(egui::Key::Enter),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: Some(egui::Key::Enter),
                pressed: false,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            },
        ];
        if mixed {
            events.push(egui::Event::Paste("competing edit".into()));
        }
        frame(&mut app, events);
        if mixed {
            assert!(!app.project_reveal_pending());
            assert!(commands.try_recv().is_err());
        } else {
            expect_list(&commands, "");
            assert!(app.project_reveal_pending());
        }
    }
}

#[test]
fn project_reveal_depth_32_uses_exactly_32_lists_and_no_extra_request() {
    let directories = (0..31).map(|n| format!("d{n}")).collect::<Vec<_>>();
    let target = format!("{}/current.rs", directories.join("/"));
    let (mut app, commands) = fixture(&target, Mode::Tree);
    start(&mut app);
    for n in 0..32 {
        let parent = directories[..n.min(31)].join("/");
        let command = expect_list(&commands, &parent);
        let child = if n == 31 {
            target.clone()
        } else {
            directories[..=n].join("/")
        };
        listed(&mut app, command, vec![entry(&child, n != 31)]);
        settle(&mut app);
    }
    assert!(!app.project_reveal_pending());
    assert_eq!(app.explorer.selected, target);
    assert!(workspace_access_tests::recorded_response(&app, "current.rs").has_focus());
    assert!(commands.try_recv().is_err());
}

#[test]
fn project_reveal_expired_intent_accepts_only_ordinary_late_cache_without_focus_or_continuation() {
    let (mut app, commands) = fixture("src/current.rs", Mode::Tree);
    start(&mut app);
    let command = expect_list(&commands, "");
    app.explorer.reveal.as_mut().unwrap().deadline = Instant::now() - Duration::from_millis(1);
    settle(&mut app);
    assert!(!app.project_reveal_pending());
    assert!(app.explorer_busy());
    let focus = app.editor_ctx.memory(|memory| memory.focused());
    listed(&mut app, command, vec![entry("src", true)]);
    settle(&mut app);
    assert!(app
        .explorer_visible_rows()
        .iter()
        .any(|row| row.entry.path == "src"));
    assert_eq!(app.editor_ctx.memory(|memory| memory.focused()), focus);
    assert!(commands.try_recv().is_err());
}

#[test]
fn project_reveal_unrendered_or_zero_viewport_target_cannot_finish_from_cache_alone() {
    for zero_viewport in [false, true] {
        let (mut app, commands) = fixture("current.rs", Mode::Tree);
        app.explorer_refresh("");
        let command = expect_list(&commands, "");
        listed(&mut app, command, vec![entry("current.rs", false)]);
        settle(&mut app);
        let ctx = app.editor_ctx.clone();
        app.start_project_reveal(&ctx);
        app.explorer.reveal.as_mut().unwrap().ready = true;
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(640.0, 32.0),
            )),
            ..Default::default()
        };
        eframe::App::raw_input_hook(&mut app, &ctx, &mut input);
        let mut native = eframe::Frame::_new_kittest();
        let _ = ctx.run(input, |ctx| {
            if zero_viewport {
                eframe::App::update(&mut app, ctx, &mut native);
            } else {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.label("Project was not rendered in this pass");
                });
                app.finish_project_reveal_frame(ctx);
            }
        });
        assert_ne!(
            ctx.memory(|memory| memory.focused()),
            Some(row_id("current.rs"))
        );
        assert_ne!(
            app.explorer.message.as_deref(),
            Some("Current file selected in Project")
        );
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn project_reveal_original_content_cancellation_survives_all_live_events_being_consumed() {
    for content in [
        egui::Event::Text("x".into()),
        egui::Event::Paste("x".into()),
        egui::Event::Ime(egui::ImeEvent::Commit("x".into())),
    ] {
        let (mut app, commands) = fixture("src/current.rs", Mode::Tree);
        start(&mut app);
        let command = expect_list(&commands, "");
        let ctx = app.editor_ctx.clone();
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 740.0),
            )),
            events: vec![
                egui::Event::Key {
                    key: egui::Key::ArrowRight,
                    physical_key: Some(egui::Key::ArrowRight),
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                },
                content,
            ],
            ..Default::default()
        };
        eframe::App::raw_input_hook(&mut app, &ctx, &mut input);
        input.events.clear();
        listed(&mut app, command, vec![entry("src", true)]);
        let mut native = eframe::Frame::_new_kittest();
        let _ = ctx.run(input, |ctx| eframe::App::update(&mut app, ctx, &mut native));
        assert!(!app.project_reveal_pending());
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn project_reveal_action_is_readable_and_keyboard_usable_at_declared_small_window_fonts() {
    let size = [780.0, 540.0];
    let keys = |key| {
        vec![true, false]
            .into_iter()
            .map(|pressed| egui::Event::Key {
                key,
                physical_key: Some(key),
                pressed,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            })
            .collect()
    };
    for font_size in [13.0, 18.0] {
        let (mut app, commands) = fixture("current.rs", Mode::Tree);
        app.editor_ctx.style_mut(|style| {
            style.spacing.item_spacing = egui::vec2(9.0, 8.0);
            style.spacing.button_padding = egui::vec2(10.0, 6.0);
            style.text_styles.insert(
                egui::TextStyle::Button,
                egui::FontId::proportional(font_size),
            );
        });
        for _ in 0..4 {
            frame_at(&mut app, size, vec![]);
        }
        for _ in 0..40 {
            if workspace_access_tests::recorded_response(&app, "project_select_current").has_focus()
            {
                break;
            }
            frame_at(&mut app, size, keys(egui::Key::Tab));
        }
        for _ in 0..4 {
            frame_at(&mut app, size, vec![]);
        }
        let response = workspace_access_tests::recorded_response(&app, "project_select_current");
        let clip = workspace_access_tests::recorded_clip(&app, "project_select_current");
        assert!(
            response.has_focus(),
            "native Tab must reach the action at font {font_size}"
        );
        assert!(response.enabled());
        assert!(response.rect.is_finite() && response.rect.is_positive());
        assert!(
            clip.contains_rect(response.rect),
            "wrapped action must be fully readable at font {font_size}: {:?} / {clip:?}",
            response.rect
        );
        assert!(
            response.rect.top()
                >= workspace_access_tests::recorded_rect(&app, "explorer_tree_mode").bottom()
        );
        assert!(response.rect.bottom() < workspace_access_tests::recorded_rect(&app, "/").top());
        assert!(
            commands.try_recv().is_err(),
            "Tab and automatic scroll must not dispatch"
        );
        frame_at(&mut app, size, keys(egui::Key::Enter));
        expect_list(&commands, "");
        assert!(app.project_reveal_pending());
    }
}

#[test]
fn project_reveal_cancel_and_manual_row_keep_their_pointer_targets_during_cancellation() {
    for font_size in [13.0, 18.0] {
        for cancel_button in [true, false] {
            let (mut app, commands) = fixture("src/current.rs", Mode::Tree);
            app.editor_ctx.style_mut(|style| {
                style.text_styles.insert(
                    egui::TextStyle::Button,
                    egui::FontId::proportional(font_size),
                );
            });
            app.editor_ctx.data_mut(|data| {
                data.insert_persisted(
                    egui::Id::new("explorer"),
                    egui::containers::panel::PanelState {
                        rect: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(180.0, 740.0)),
                    },
                )
            });
            app.explorer_refresh("");
            let command = expect_list(&commands, "");
            listed(&mut app, command, vec![entry("src", true)]);
            settle(&mut app);
            click(&mut app, "project_select_current");
            let command = expect_list(&commands, "src");
            settle(&mut app);
            let cancel_rect = workspace_access_tests::recorded_rect(&app, "project_reveal_cancel");
            let row_rect = workspace_access_tests::recorded_rect(&app, "src");
            let pos = if cancel_button {
                cancel_rect.center()
            } else {
                row_rect.center()
            };
            for pressed in [true, false] {
                frame(
                    &mut app,
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
                assert!(!app.project_reveal_pending());
                assert_eq!(
                    workspace_access_tests::recorded_rect(&app, "project_reveal_cancel"),
                    cancel_rect
                );
                assert_eq!(workspace_access_tests::recorded_rect(&app, "src"), row_rect);
                assert!(
                    commands.try_recv().is_err(),
                    "cancellation must not click through to a new List or Read"
                );
            }
            let src = app
                .explorer_visible_rows()
                .into_iter()
                .find(|row| row.entry.path == "src")
                .unwrap();
            assert_eq!(
                src.expanded, cancel_button,
                "manual row gesture should collapse exactly src; Cancel should not collapse it"
            );
            listed(&mut app, command, vec![entry("src/current.rs", false)]);
            settle(&mut app);
            assert!(commands.try_recv().is_err());
        }
    }
}

#[test]
fn project_reveal_canceled_flat_list_may_settle_ordinary_directory_without_revival_or_focus() {
    let (mut app, commands) = fixture("src/current.rs", Mode::Flat);
    settle(&mut app);
    click(&mut app, "project_select_current");
    let command = expect_list(&commands, "src");
    click(&mut app, "project_reveal_cancel");
    let focused = app.editor_ctx.memory(|memory| memory.focused());
    assert!(!app.project_reveal_pending());
    assert!(app.explorer_busy());
    listed(&mut app, command, vec![entry("src/current.rs", false)]);
    settle(&mut app);
    assert_eq!(app.directory, "src");
    assert_eq!(app.entries[0].path, "src/current.rs");
    assert!(!app.explorer_busy());
    assert!(!app.project_reveal_pending());
    assert_eq!(app.editor_ctx.memory(|memory| memory.focused()), focused);
    assert!(commands.try_recv().is_err());
}

#[test]
fn project_reveal_content_free_ime_lifecycle_keeps_same_pending_ticket_and_deadline() {
    let (mut app, commands) = fixture("src/current.rs", Mode::Tree);
    settle(&mut app);
    click(&mut app, "project_select_current");
    let command = expect_list(&commands, "");
    let intent = app.explorer.reveal.as_ref().unwrap();
    let deadline = intent.deadline;
    let ticket = intent.pending.clone();
    for lifecycle in [egui::ImeEvent::Enabled, egui::ImeEvent::Disabled] {
        frame(&mut app, vec![egui::Event::Ime(lifecycle)]);
        let intent = app
            .explorer
            .reveal
            .as_ref()
            .expect("content-free lifecycle must retain intent");
        assert_eq!(
            intent.deadline, deadline,
            "lifecycle must never renew the deadline"
        );
        assert_eq!(intent.pending, ticket);
        assert!(commands.try_recv().is_err());
    }
    listed(&mut app, command, vec![entry("src", true)]);
    settle(&mut app);
    expect_list(&commands, "src");
    assert_eq!(app.explorer.reveal.as_ref().unwrap().deadline, deadline);
}

#[test]
fn project_reveal_mixed_lifecycle_with_even_empty_composition_cancels_before_ready_reply() {
    for content in [
        egui::Event::Ime(egui::ImeEvent::Preedit(String::new())),
        egui::Event::Ime(egui::ImeEvent::Preedit("composing".into())),
        egui::Event::Ime(egui::ImeEvent::Commit(String::new())),
        egui::Event::Ime(egui::ImeEvent::Commit("committed".into())),
        egui::Event::Text("text".into()),
        egui::Event::Paste("paste".into()),
    ] {
        let (mut app, commands) = fixture("src/current.rs", Mode::Tree);
        settle(&mut app);
        click(&mut app, "project_select_current");
        let command = expect_list(&commands, "");
        let ctx = app.editor_ctx.clone();
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 740.0),
            )),
            events: vec![
                egui::Event::Ime(egui::ImeEvent::Enabled),
                content,
                egui::Event::Ime(egui::ImeEvent::Disabled),
            ],
            ..Default::default()
        };
        eframe::App::raw_input_hook(&mut app, &ctx, &mut input);
        assert!(
            !app.project_reveal_pending(),
            "original content must cancel before the ready reply"
        );
        input.events.clear();
        listed(&mut app, command, vec![entry("src", true)]);
        let mut native = eframe::Frame::_new_kittest();
        let _ = ctx.run(input, |ctx| eframe::App::update(&mut app, ctx, &mut native));
        frame(&mut app, vec![egui::Event::Ime(egui::ImeEvent::Disabled)]);
        assert!(
            !app.project_reveal_pending(),
            "later Disabled cannot revive a canceled intent"
        );
        assert!(app
            .explorer_visible_rows()
            .iter()
            .any(|row| row.entry.path == "src"));
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn project_reveal_prior_composition_cannot_be_masked_by_disabled_on_entry() {
    for preedit in ["", "active composition"] {
        for pointer in [true, false] {
            let (mut app, commands) = fixture("current.rs", Mode::Flat);
            settle(&mut app);
            if !pointer {
                workspace_access_tests::recorded_response(&app, "project_select_current")
                    .request_focus();
                frame(&mut app, vec![]);
            }
            frame(
                &mut app,
                vec![egui::Event::Ime(egui::ImeEvent::Preedit(preedit.into()))],
            );
            if pointer {
                let pos =
                    workspace_access_tests::recorded_rect(&app, "project_select_current").center();
                frame(
                    &mut app,
                    vec![
                        egui::Event::Ime(egui::ImeEvent::Disabled),
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed: true,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
                frame(
                    &mut app,
                    vec![egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: egui::Modifiers::NONE,
                    }],
                );
            } else {
                frame(
                    &mut app,
                    vec![
                        egui::Event::Ime(egui::ImeEvent::Disabled),
                        egui::Event::Key {
                            key: egui::Key::Enter,
                            physical_key: Some(egui::Key::Enter),
                            pressed: true,
                            repeat: false,
                            modifiers: egui::Modifiers::NONE,
                        },
                        egui::Event::Key {
                            key: egui::Key::Enter,
                            physical_key: Some(egui::Key::Enter),
                            pressed: false,
                            repeat: false,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
            assert!(!app.project_reveal_pending());
            assert!(
                commands.try_recv().is_err(),
                "Disabled must not clean an already-composing press"
            );
            click(&mut app, "project_select_current");
            expect_list(&commands, "");
            assert!(
                app.project_reveal_pending(),
                "a later independent clean click remains usable"
            );
        }
    }
}
