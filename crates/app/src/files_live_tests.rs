//! Files admission through the real raw-input hook and complete egui updates.
//! Only the clock and worker replies are controlled; input uses rendered widgets.
use super::*;

pub(super) fn at(app: &mut CedarApp, time: f64, events: Vec<egui::Event>) -> egui::FullOutput {
    let ctx = app.editor_ctx.clone();
    let mut input = egui::RawInput {
        time: Some(time),
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SIZE)),
        events,
        ..Default::default()
    };
    eframe::App::raw_input_hook(app, &ctx, &mut input);
    let mut native = eframe::Frame::_new_kittest();
    ctx.run(input, |ctx| eframe::App::update(app, ctx, &mut native))
}
fn now(app: &CedarApp) -> f64 {
    app.editor_ctx.input(|input| input.time)
}
fn after(app: &mut CedarApp, dt: f64, events: Vec<egui::Event>) -> egui::FullOutput {
    at(app, now(app) + dt, events)
}
fn fast_click(app: &mut CedarApp, probe: &str) {
    let pos = visible_center(app, probe);
    after(app, 0.001, vec![egui::Event::PointerMoved(pos)]);
    after(app, 0.001, pointer(pos, true));
    assert!(
        recorded_response(app, probe).is_pointer_button_down_on(),
        "{probe}"
    );
    after(app, 0.001, pointer(pos, false));
    assert!(recorded_response(app, probe).clicked(), "{probe}");
}
fn select_control(app: &mut CedarApp, probe: &str) {
    fast_click(app, probe);
    after(app, 0.001, modified_key(egui::Key::A, command_modifiers()));
}
pub(super) fn edit(app: &mut CedarApp, probe: &str, text: &str) {
    select_control(app, probe);
    after(
        app,
        0.001,
        if text.is_empty() {
            key(egui::Key::Backspace)
        } else {
            vec![egui::Event::Text(text.into())]
        },
    );
}
fn settle_files(app: &mut CedarApp) {
    after(app, browse::FILES_DEBOUNCE_SECONDS, vec![]);
}

#[test]
fn files_live_burst_has_fixed_250ms_deadline_and_typing_only_finds_paths() {
    let (mut app, commands) = discovery_fixture();
    enter(&mut app, false);
    let departure = selection_signature(selection(&app));
    let undo = undo_texts(&app);
    for text in ["n", "e", "e"] {
        after(&mut app, 0.1, vec![egui::Event::Text(text.into())]);
        assert!(commands.try_recv().is_err());
    }
    let edited = now(&app);
    at(&mut app, edited + 0.249, vec![]);
    assert!(commands.try_recv().is_err());
    // Repeating the same layout/time cannot renew or prematurely consume it.
    at(&mut app, edited + 0.249, vec![]);
    assert!(commands.try_recv().is_err());
    at(&mut app, edited + 0.250, vec![]);
    let request = expect_discovery(&commands, "nee", "");
    after(&mut app, 10.0, vec![]);
    assert!(commands.try_recv().is_err());
    reply(
        &mut app,
        request.id,
        Ok(discovery_result("nee", "", &["src/needle.rs"], vec![])),
    );
    after(&mut app, 0.001, vec![]);
    assert_eq!(app.browse.paths(), vec!["src/needle.rs"]);
    assert_eq!(app.documents.len(), 1);
    assert_draft(&app, departure, &undo);
    assert!(
        commands.try_recv().is_err(),
        "typing and accepting paths must issue zero Read"
    );
}

#[test]
fn files_live_final_scope_and_final_frame_edit_replace_due_intent() {
    let (mut app, commands) = discovery_fixture();
    enter(&mut app, false);
    edit(&mut app, "browse_query", "needle");
    edit(&mut app, "browse_scope", "src ");
    let edited = now(&app);
    at(
        &mut app,
        edited + 0.250,
        vec![egui::Event::Text("deep".into())],
    );
    assert_eq!(app.browse.scope(), "src deep");
    assert!(
        commands.try_recv().is_err(),
        "final input must win over an expired deadline"
    );
    let edited = now(&app);
    at(&mut app, edited + 0.249, vec![]);
    assert!(commands.try_recv().is_err());
    at(&mut app, edited + 0.250, vec![]);
    expect_discovery(&commands, "needle", "src deep");
}

#[test]
fn files_live_enter_and_find_flush_once_and_dedupe_identical_pending() {
    for button in [false, true] {
        let (mut app, commands) = discovery_fixture();
        enter(&mut app, false);
        edit(&mut app, "browse_query", "needle");
        if button {
            fast_click(&mut app, "browse_submit");
        } else {
            after(&mut app, 0.001, key(egui::Key::Enter));
        }
        let first = expect_discovery(&commands, "needle", "");
        after(&mut app, 0.001, key(egui::Key::Enter));
        fast_click(&mut app, "browse_submit");
        settle_files(&mut app);
        assert!(commands.try_recv().is_err());
        reply(
            &mut app,
            first.id,
            Ok(discovery_result("needle", "", &["needle.rs"], vec![])),
        );
        after(&mut app, 0.001, vec![]);
        assert_eq!(
            app.browse.paths(),
            vec!["needle.rs"],
            "duplicate submit must retain the flight identity"
        );
        after(&mut app, 3.0, vec![]);
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn files_live_single_flight_latest_replacement_and_aba_retire_late_results() {
    for (last, failed) in [("charlie", false), ("alpha", false), ("alpha", true)] {
        let (mut app, commands) = discovery_fixture();
        enter(&mut app, false);
        edit(&mut app, "browse_query", "alpha");
        settle_files(&mut app);
        let first = expect_discovery(&commands, "alpha", "");
        for value in ["bravo", last] {
            edit(&mut app, "browse_query", value);
            settle_files(&mut app);
            after(&mut app, 0.001, key(egui::Key::Enter));
            assert!(
                commands.try_recv().is_err(),
                "only the actual reply releases the flight"
            );
        }
        app.error = Some("current status".into());
        reply(
            &mut app,
            first.id,
            if failed {
                Err("late error".into())
            } else {
                Ok(discovery_result("alpha", "", &["alpha-old.rs"], vec![]))
            },
        );
        after(&mut app, 0.001, vec![]);
        let latest = expect_discovery(&commands, last, "");
        assert_ne!(first.id, latest.id);
        assert!(app.browse.paths().is_empty());
        assert_eq!(app.error.as_deref(), Some("current status"));
        reply(
            &mut app,
            latest.id,
            Ok(discovery_result(
                last,
                "",
                &[&format!("{last}-new.rs")],
                vec![],
            )),
        );
        after(&mut app, 0.001, vec![]);
        assert_eq!(app.browse.paths(), vec![format!("{last}-new.rs")]);
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn files_live_close_and_provider_switch_win_at_deadline() {
    for probe in ["browse_close", "browse_text"] {
        let (mut app, commands) = discovery_fixture();
        enter(&mut app, false);
        edit(&mut app, "browse_query", "needle");
        let edited = now(&app);
        let pos = visible_center(&app, probe);
        at(&mut app, edited + 0.1, vec![egui::Event::PointerMoved(pos)]);
        at(&mut app, edited + 0.2, pointer(pos, true));
        at(&mut app, edited + 0.25, pointer(pos, false));
        after(&mut app, 1.0, vec![]);
        assert!(
            commands.try_recv().is_err(),
            "{probe} must retire the timer"
        );
        if probe == "browse_close" {
            assert!(!app.browse_marker_open());
        } else {
            assert!(app.browse_marker_open());
        }
    }
}

#[test]
fn files_live_ime_preedit_never_searches_and_commit_gets_full_interval() {
    let (mut app, commands) = discovery_fixture();
    enter(&mut app, false);
    edit(&mut app, "browse_query", "stale");
    select_control(&mut app, "browse_query");
    after(
        &mut app,
        0.25,
        vec![
            egui::Event::Ime(egui::ImeEvent::Enabled),
            egui::Event::Ime(egui::ImeEvent::Preedit("文件".into())),
        ],
    );
    after(&mut app, 5.0, vec![]);
    assert!(commands.try_recv().is_err());
    after(&mut app, 0.001, key(egui::Key::Enter));
    assert!(commands.try_recv().is_err());
    after(
        &mut app,
        0.001,
        vec![egui::Event::Ime(egui::ImeEvent::Commit("文件".into()))],
    );
    assert_eq!(app.browse.query(), "文件");
    let committed = now(&app);
    at(&mut app, committed + 0.249, vec![]);
    assert!(commands.try_recv().is_err());
    at(&mut app, committed + 0.25, vec![]);
    expect_discovery(&commands, "文件", "");
}

#[test]
fn files_live_cancelled_query_or_scope_composition_preserves_native_bytes_and_pauses_automatic_search(
) {
    for probe in ["browse_query", "browse_scope"] {
        for ending in [
            vec![egui::Event::Ime(egui::ImeEvent::Disabled)],
            vec![egui::Event::Ime(egui::ImeEvent::Commit(String::new()))],
            vec![
                egui::Event::Ime(egui::ImeEvent::Commit("甲".into())),
                egui::Event::Ime(egui::ImeEvent::Preedit("乙".into())),
                egui::Event::Ime(egui::ImeEvent::Disabled),
            ],
            vec![
                egui::Event::Ime(egui::ImeEvent::Commit("甲".into())),
                egui::Event::Ime(egui::ImeEvent::Preedit("乙".into())),
                egui::Event::Ime(egui::ImeEvent::Commit(String::new())),
            ],
        ] {
            let (mut app, commands) = discovery_fixture();
            enter(&mut app, false);
            edit(&mut app, "browse_query", "stable");
            edit(&mut app, "browse_scope", "src");
            select_control(&mut app, probe);
            after(
                &mut app,
                0.001,
                vec![
                    egui::Event::Ime(egui::ImeEvent::Enabled),
                    egui::Event::Ime(egui::ImeEvent::Preedit("marked".into())),
                ],
            );
            after(&mut app, 1.0, vec![]);
            assert!(commands.try_recv().is_err());
            let ending_count = ending.len();
            continuity_frame(&mut app, 0.001, ending, true, None);
            let query = app.browse.query().to_owned();
            let scope = app.browse.scope().to_owned();
            let expected = if ending_count == 1 {
                "marked"
            } else {
                "甲乙"
            };
            if probe == "browse_query" {
                assert_eq!(query, expected);
                assert_eq!(scope, "src");
            } else {
                assert_eq!(query, "stable");
                assert_eq!(scope, expected);
            }
            assert!(app.browse.preview_message().unwrap().contains("paused"));
            after(&mut app, 2.0, vec![]);
            assert_eq!(app.browse.query(), query);
            assert_eq!(app.browse.scope(), scope);
            assert!(commands.try_recv().is_err());
            after(&mut app, 0.001, key(egui::Key::Enter));
            expect_discovery(&commands, &query, &scope);
        }
    }
}

#[test]
fn files_live_auto_admission_and_reply_preserve_scope_focus() {
    let (mut app, commands) = discovery_fixture();
    enter(&mut app, false);
    edit(&mut app, "browse_query", "needle");
    fast_click(&mut app, "browse_scope");
    let scope_focus = recorded_response(&app, "browse_scope").id;
    assert_eq!(focused(&app), Some(scope_focus));
    settle_files(&mut app);
    let request = expect_discovery(&commands, "needle", "");
    assert_eq!(focused(&app), Some(scope_focus));
    reply(
        &mut app,
        request.id,
        Ok(discovery_result("needle", "", &["needle.rs"], vec![])),
    );
    after(&mut app, 0.001, vec![]);
    assert_eq!(focused(&app), Some(scope_focus));
    assert_eq!(app.browse.paths(), vec!["needle.rs"]);
    assert!(commands.try_recv().is_err());
}

#[test]
fn files_live_empty_root_loaded_only_but_empty_scoped_query_is_invalid() {
    let (mut app, commands) = discovery_fixture();
    enter(&mut app, false);
    edit(&mut app, "browse_query", "needle");
    edit(&mut app, "browse_query", "");
    settle_files(&mut app);
    assert!(!app.browse.paths().is_empty());
    assert!(commands.try_recv().is_err());
    edit(&mut app, "browse_scope", "src");
    settle_files(&mut app);
    assert!(app.browse.paths().is_empty());
    assert!(app
        .browse
        .preview_message()
        .unwrap()
        .contains("Enter a filename query"));
    after(&mut app, 5.0, vec![]);
    assert!(commands.try_recv().is_err());
}

#[test]
fn files_live_validation_preserves_literal_utf8_and_exact_scope_bounds() {
    for (query, scope, valid) in [
        ("é".repeat(128), "".to_owned(), true),
        ("é".repeat(129), "".to_owned(), false),
        ("needle\0".to_owned(), "".to_owned(), false),
        (" needle ".to_owned(), " src ".to_owned(), true),
        ("needle".to_owned(), "../src".to_owned(), false),
        ("needle".to_owned(), "src//deep".to_owned(), false),
        ("needle".to_owned(), "target".to_owned(), false),
    ] {
        let (mut app, commands) = discovery_fixture();
        enter(&mut app, false);
        edit(&mut app, "browse_query", &query);
        if !scope.is_empty() {
            edit(&mut app, "browse_scope", &scope);
        }
        settle_files(&mut app);
        if valid {
            expect_discovery(&commands, &query, &scope);
        } else {
            assert!(commands.try_recv().is_err());
            assert!(app.browse.preview_message().is_some());
            after(&mut app, 5.0, vec![]);
            assert!(commands.try_recv().is_err());
        }
    }
}

#[test]
fn files_live_old_peer_and_project_text_remain_without_automatic_remote_work() {
    for descriptor in [
        None,
        Some(cedar_protocol::FileDiscoveryDescriptor { version: 2 }),
    ] {
        let (mut app, commands) = fixture();
        app.file_discovery = descriptor;
        enter(&mut app, false);
        edit(&mut app, "browse_query", "origin");
        settle_files(&mut app);
        assert_eq!(app.browse.paths(), vec!["origin.txt"]);
        after(&mut app, 5.0, vec![]);
        assert!(commands.try_recv().is_err());
    }
    let (mut app, commands) = discovery_fixture();
    enter(&mut app, true);
    edit(&mut app, "browse_query", "needle");
    after(&mut app, 5.0, vec![]);
    assert!(commands.try_recv().is_err());
    after(&mut app, 0.001, key(egui::Key::Enter));
    assert!(matches!(
        commands.try_recv().unwrap().op,
        Operation::Search { .. }
    ));
    assert!(commands.try_recv().is_err());
}

#[test]
fn files_live_terminal_outcomes_do_not_retry_or_poll() {
    for result in [
        Err("server_busy".into()),
        Err("permission denied".into()),
        Ok(discovery_result("wrong", "", &[], vec![])),
    ] {
        let (mut app, commands) = discovery_fixture();
        enter(&mut app, false);
        edit(&mut app, "browse_query", "needle");
        settle_files(&mut app);
        let request = expect_discovery(&commands, "needle", "");
        reply(&mut app, request.id, result);
        after(&mut app, 0.001, vec![]);
        let message = app.browse.preview_message().unwrap().to_owned();
        for _ in 0..5 {
            after(&mut app, 1.0, vec![]);
        }
        assert_eq!(app.browse.preview_message(), Some(message.as_str()));
        assert!(commands.try_recv().is_err());
        after(&mut app, 0.001, key(egui::Key::Enter));
        let retry = expect_discovery(&commands, "needle", "");
        assert_ne!(retry.id, request.id);
    }
}

#[test]
fn files_live_edit_retires_preview_read_and_stale_open_activation() {
    let (mut app, commands) = discovery_fixture();
    enter(&mut app, false);
    edit(&mut app, "browse_query", "needle");
    settle_files(&mut app);
    let request = expect_discovery(&commands, "needle", "");
    reply(
        &mut app,
        request.id,
        Ok(discovery_result("needle", "", &["needle.rs"], vec![])),
    );
    after(&mut app, 0.001, vec![]);
    after(&mut app, 0.001, vec![]);
    fast_click(&mut app, "browse_row:0");
    after(&mut app, 0.001, key(egui::Key::Space));
    let preview = commands.try_recv().unwrap();
    assert!(matches!(preview.op, Operation::Read { .. }));
    edit(&mut app, "browse_query", "other");
    assert!(!app.browse.preview_visible());
    assert!(app.browse.paths().is_empty());
    app.browse_open();
    reply(
        &mut app,
        preview.id,
        Ok(file("needle.rs", "retired preview")),
    );
    after(&mut app, 0.001, vec![]);
    assert!(!app.browse.preview_visible());
    assert!(app.browse.preview_text().is_none());
    assert!(commands.try_recv().is_err());
    settle_files(&mut app);
    expect_discovery(&commands, "other", "");
    assert_eq!(app.documents.len(), 1);
}

#[test]
fn files_live_mixed_text_paste_and_commit_enter_use_final_committed_input() {
    for paste in [false, true] {
        let (mut app, commands) = discovery_fixture();
        enter(&mut app, false);
        edit(&mut app, "browse_query", "n");
        let mut events = vec![if paste {
            egui::Event::Paste("eedle".into())
        } else {
            egui::Event::Text("eedle".into())
        }];
        events.extend(key(egui::Key::Enter));
        after(&mut app, 0.25, events);
        assert_eq!(app.browse.query(), "needle");
        assert!(commands.try_recv().is_err());
        settle_files(&mut app);
        expect_discovery(&commands, "needle", "");
    }
    let (mut app, commands) = discovery_fixture();
    enter(&mut app, false);
    after(
        &mut app,
        0.001,
        vec![
            egui::Event::Ime(egui::ImeEvent::Enabled),
            egui::Event::Ime(egui::ImeEvent::Preedit("文".into())),
        ],
    );
    let mut events = vec![egui::Event::Ime(egui::ImeEvent::Commit("文件".into()))];
    events.extend(key(egui::Key::Enter));
    after(&mut app, 1.0, events);
    assert_eq!(app.browse.query(), "文件");
    assert!(commands.try_recv().is_err());
    settle_files(&mut app);
    expect_discovery(&commands, "文件", "");
}

#[test]
fn files_live_held_repeat_and_discarded_layout_do_not_renew_or_duplicate_deadline() {
    let (mut app, commands) = discovery_fixture();
    enter(&mut app, false);
    edit(&mut app, "browse_query", "needle");
    let edited = now(&app);
    at(
        &mut app,
        edited + 0.1,
        vec![edge(egui::Key::ArrowRight, true, false)],
    );
    at(
        &mut app,
        edited + 0.249,
        vec![edge(egui::Key::ArrowRight, true, true)],
    );
    assert!(commands.try_recv().is_err());
    let remaining = edited + 0.25 - now(&app);
    continuity_frame(
        &mut app,
        remaining,
        vec![edge(egui::Key::ArrowRight, false, false)],
        true,
        None,
    );
    expect_discovery(&commands, "needle", "");
    after(&mut app, 1.0, vec![]);
    assert!(commands.try_recv().is_err());
}

#[test]
fn files_live_invalid_clock_retires_wait_without_automatic_retry() {
    for invalid in [f64::NAN, f64::NEG_INFINITY, -1.0] {
        let (mut app, commands) = discovery_fixture();
        enter(&mut app, false);
        edit(&mut app, "browse_query", "needle");
        // egui rejects regressing clocks before update. Inject only the clock
        // sample at the actual end-frame settlement seam, after real UI input.
        let ctx = app.editor_ctx.clone();
        app.finish_files_settlement(&ctx, invalid);
        assert!(commands.try_recv().is_err());
        assert!(app
            .browse
            .preview_message()
            .unwrap()
            .contains("interrupted"));
        after(&mut app, 5.0, vec![]);
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn files_live_close_reopen_and_connection_change_cannot_restore_old_reply() {
    for failed in [false, true] {
        let (mut app, commands) = discovery_fixture();
        enter(&mut app, false);
        edit(&mut app, "browse_query", "alpha");
        settle_files(&mut app);
        let first = expect_discovery(&commands, "alpha", "");
        fast_click(&mut app, "browse_close");
        enter(&mut app, false);
        edit(&mut app, "browse_query", "beta");
        settle_files(&mut app);
        assert!(commands.try_recv().is_err());
        reply(
            &mut app,
            first.id,
            if failed {
                Err("closed failure".into())
            } else {
                Ok(discovery_result("alpha", "", &["alpha.rs"], vec![]))
            },
        );
        after(&mut app, 0.001, vec![]);
        assert!(app.browse.paths().is_empty());
        expect_discovery(&commands, "beta", "");
    }
    let (mut app, commands) = discovery_fixture();
    enter(&mut app, false);
    edit(&mut app, "browse_query", "needle");
    app.generation += 1;
    app.file_discovery = None;
    settle_files(&mut app);
    assert!(!app.browse_marker_open());
    assert!(commands.try_recv().is_err());
}

#[test]
fn files_live_pointer_press_then_query_replacement_keeps_old_release_inert() {
    let (mut app, commands) = discovery_fixture();
    enter(&mut app, false);
    edit(&mut app, "browse_query", "needle");
    settle_files(&mut app);
    let first = expect_discovery(&commands, "needle", "");
    reply(
        &mut app,
        first.id,
        Ok(discovery_result("needle", "", &["needle.rs"], vec![])),
    );
    after(&mut app, 0.001, vec![]);
    after(&mut app, 0.001, vec![]);
    let pos = visible_center(&app, "browse_open");
    after(&mut app, 0.001, vec![egui::Event::PointerMoved(pos)]);
    after(&mut app, 0.001, pointer(pos, true));
    let query_id = recorded_response(&app, "browse_query").id;
    for _ in 0..12 {
        if focused(&app) == Some(query_id) {
            break;
        }
        after(&mut app, 0.001, key(egui::Key::Tab));
    }
    assert_eq!(focused(&app), Some(query_id));
    after(&mut app, 0.001, vec![]);
    assert!(recorded_response(&app, "browse_query").has_focus());
    after(&mut app, 0.001, vec![egui::Event::Text("x".into())]);
    assert_eq!(
        app.browse.query(),
        "needlex",
        "focus={:?}, query_has_focus={}",
        focused(&app),
        recorded_response(&app, "browse_query").has_focus()
    );
    settle_files(&mut app);
    let replacement = expect_discovery(&commands, "needlex", "");
    reply(
        &mut app,
        replacement.id,
        Ok(discovery_result("needlex", "", &["needlex.rs"], vec![])),
    );
    after(&mut app, 0.001, vec![]);
    after(&mut app, 0.001, vec![]);
    assert_eq!(app.browse.paths(), vec!["needlex.rs"]);
    after(&mut app, 0.001, pointer(pos, false));
    assert!(
        commands.try_recv().is_err(),
        "old Open press cannot activate replacement rows"
    );
    assert_eq!(app.documents.len(), 1);
}

#[test]
fn files_live_mixed_batch_cancel_preserves_native_text_and_pauses_admission() {
    let (mut app, commands) = discovery_fixture();
    enter(&mut app, false);
    edit(&mut app, "browse_query", "stable");
    after(
        &mut app,
        0.001,
        vec![
            egui::Event::Text("x".into()),
            egui::Event::Ime(egui::ImeEvent::Enabled),
            egui::Event::Ime(egui::ImeEvent::Preedit("marked".into())),
        ],
    );
    let native_text = app.browse.query().to_owned();
    assert_eq!(native_text, "stablexmarked");
    after(&mut app, 1.0, vec![]);
    assert!(commands.try_recv().is_err());
    after(
        &mut app,
        0.001,
        vec![egui::Event::Ime(egui::ImeEvent::Disabled)],
    );
    assert_eq!(app.browse.query(), native_text);
    after(&mut app, 1.0, vec![]);
    assert!(commands.try_recv().is_err());
    assert!(app.browse.preview_message().unwrap().contains("paused"));
    after(&mut app, 0.001, key(egui::Key::Enter));
    expect_discovery(&commands, &native_text, "");
}

#[test]
fn files_live_composition_focus_loss_and_close_preserve_native_bytes_without_search() {
    let (mut app, commands) = discovery_fixture();
    enter(&mut app, false);
    edit(&mut app, "browse_query", "stable");
    after(
        &mut app,
        0.001,
        vec![
            egui::Event::Ime(egui::ImeEvent::Enabled),
            egui::Event::Ime(egui::ImeEvent::Preedit("marked".into())),
        ],
    );
    let native_text = app.browse.query().to_owned();
    after(&mut app, 0.25, vec![egui::Event::WindowFocused(false)]);
    assert_eq!(app.browse.query(), native_text);
    assert!(commands.try_recv().is_err());
    after(
        &mut app,
        0.001,
        vec![
            egui::Event::WindowFocused(true),
            egui::Event::Ime(egui::ImeEvent::Disabled),
        ],
    );
    after(&mut app, 1.0, vec![]);
    assert_eq!(app.browse.query(), native_text);
    assert!(commands.try_recv().is_err());
    fast_click(&mut app, "browse_close");
    assert!(!app.browse_marker_open());
    assert_eq!(app.browse.query(), native_text);
    after(&mut app, 1.0, vec![]);
    assert!(commands.try_recv().is_err());
}

#[test]
fn files_live_capability_change_and_exhausted_identity_do_not_poll() {
    let (mut app, commands) = discovery_fixture();
    enter(&mut app, false);
    edit(&mut app, "browse_query", "needle");
    app.file_discovery = Some(cedar_protocol::FileDiscoveryDescriptor { version: 2 });
    settle_files(&mut app);
    assert!(app.browse.preview_message().unwrap().contains("paused"));
    app.file_discovery = Some(cedar_protocol::FileDiscoveryDescriptor::V1);
    after(&mut app, 5.0, vec![]);
    assert!(commands.try_recv().is_err());
    after(&mut app, 0.001, key(egui::Key::Enter));
    expect_discovery(&commands, "needle", "");
    let (mut app, commands) = discovery_fixture();
    enter(&mut app, false);
    edit(&mut app, "browse_query", "needle");
    app.next_request = u64::MAX;
    settle_files(&mut app);
    assert!(app.browse.preview_message().unwrap().contains("exhausted"));
    after(&mut app, 5.0, vec![]);
    assert!(commands.try_recv().is_err());
}

#[test]
fn files_live_noop_delete_repeat_never_renews_the_deadline() {
    for probe in ["browse_query", "browse_scope"] {
        let (mut app, commands) = discovery_fixture();
        enter(&mut app, false);
        edit(&mut app, "browse_query", "needle");
        if probe == "browse_scope" {
            edit(&mut app, probe, "src");
        }
        let edited = now(&app);
        at(
            &mut app,
            edited + 0.1,
            vec![edge(egui::Key::Delete, true, false)],
        );
        at(
            &mut app,
            edited + 0.249,
            vec![edge(egui::Key::Delete, true, true)],
        );
        assert!(commands.try_recv().is_err());
        at(
            &mut app,
            edited + 0.25,
            vec![edge(egui::Key::Delete, false, false)],
        );
        expect_discovery(
            &commands,
            "needle",
            if probe == "browse_scope" { "src" } else { "" },
        );
    }
}

#[test]
fn files_live_owned_final_same_text_and_empty_paste_get_fresh_identity_once() {
    for empty_paste in [false, true] {
        let (mut app, commands) = discovery_fixture();
        enter(&mut app, false);
        edit(&mut app, "browse_query", "needle");
        settle_files(&mut app);
        let old = expect_discovery(&commands, "needle", "");
        let events = if empty_paste {
            vec![egui::Event::Paste(String::new())]
        } else {
            let mut events = vec![egui::Event::Text("x".into())];
            events.extend(key(egui::Key::Backspace));
            events
        };
        continuity_frame(&mut app, 0.001, events, true, None);
        assert_eq!(app.browse.query(), "needle");
        let edited = now(&app);
        reply(
            &mut app,
            old.id,
            Ok(discovery_result("needle", "", &["needle-old.rs"], vec![])),
        );
        at(&mut app, edited + 0.249, vec![]);
        assert!(
            app.browse.paths().is_empty(),
            "same bytes must not revive the old ticket"
        );
        assert!(commands.try_recv().is_err());
        at(&mut app, edited + 0.25, vec![]);
        let current = expect_discovery(&commands, "needle", "");
        assert_ne!(old.id, current.id);
        after(&mut app, 1.0, vec![]);
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn files_live_terminal_ime_with_focus_loss_or_escape_preserves_text_and_stays_paused() {
    for focus_loss in [false, true] {
        let (mut app, commands) = discovery_fixture();
        enter(&mut app, false);
        edit(&mut app, "browse_query", "stable");
        after(
            &mut app,
            0.001,
            vec![
                egui::Event::Ime(egui::ImeEvent::Enabled),
                egui::Event::Ime(egui::ImeEvent::Preedit("marked".into())),
            ],
        );
        let mut events = if focus_loss {
            vec![
                egui::Event::Ime(egui::ImeEvent::Commit("final".into())),
                egui::Event::WindowFocused(false),
            ]
        } else {
            let mut events = vec![egui::Event::Ime(egui::ImeEvent::Disabled)];
            events.extend(key(egui::Key::Escape));
            events
        };
        continuity_frame(&mut app, 0.25, std::mem::take(&mut events), true, None);
        assert_eq!(
            app.browse.query(),
            if focus_loss {
                "stablefinal"
            } else {
                "stablemarked"
            }
        );
        assert!(
            app.browse_marker_open(),
            "mixed IME/Escape does not manufacture clean close ownership"
        );
        assert!(commands.try_recv().is_err());
        after(&mut app, 1.0, vec![]);
        assert!(commands.try_recv().is_err());
        assert!(app.browse.preview_message().unwrap().contains("paused"));
        if focus_loss {
            after(&mut app, 0.001, vec![egui::Event::WindowFocused(true)]);
        }
        fast_click(&mut app, "browse_close");
        assert!(!app.browse_marker_open());
        assert_eq!(
            app.browse.query(),
            if focus_loss {
                "stablefinal"
            } else {
                "stablemarked"
            }
        );
        assert!(commands.try_recv().is_err());
    }
}

// Synthetic egui event model only; this does not exercise an OS IME. The
// opposite oracles are intentional: accepted commit must settle; unaccepted
// commit/cancel must preserve final native bytes without automatic admission.
#[derive(Clone, Copy, Debug)]
enum ImeFocusEnding {
    AcceptedThenFocus,
    CancelBeforePointer,
    CancelBetweenPointer,
    CommitBeforePointer,
    CommitBetweenPointer,
    FocusThenLateCommit,
}

fn ime_focus_frame(
    app: &mut CedarApp,
    commands: &Receiver<Command>,
    ledger: &mut Vec<Command>,
    label: &str,
    time: f64,
    events: Vec<egui::Event>,
    discard: bool,
) -> Vec<serde_json::Value> {
    let ctx = app.editor_ctx.clone();
    let raw = format!("{events:?}");
    let before = serde_json::json!({
        "focus": format!("{:?}", focused(app)),
        "query": app.browse.query(), "scope": app.browse.scope(),
        "controller": app.browse.ime_focus_observation(),
    });
    let mut input = egui::RawInput {
        time: Some(time),
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SIZE)),
        events,
        ..Default::default()
    };
    eframe::App::raw_input_hook(app, &ctx, &mut input);
    let hook = app.browse.ime_focus_observation();
    let mut native = eframe::Frame::_new_kittest();
    let mut passes = Vec::new();
    let output = ctx.run(input, |ctx| {
        eframe::App::update(app, ctx, &mut native);
        let mut emitted = Vec::new();
        for command in commands.try_iter() {
            emitted.push(serde_json::json!({"id": command.id, "op": command.op}));
            ledger.push(command);
        }
        let pass = serde_json::json!({
            "label": label, "time": time, "raw": raw,
            "before": before, "after_raw_hook": hook,
            "pass": passes.len(), "cumulative_pass": ctx.cumulative_pass_nr(),
            "query": app.browse.query(), "scope": app.browse.scope(),
            "focus": format!("{:?}", focused(app)),
            "query_response": crate::workspace_access_tests::recorded_observation(app, "browse_query"),
            "scope_response": crate::workspace_access_tests::recorded_observation(app, "browse_scope"),
            "controller": app.browse.ime_focus_observation(),
            "commands": emitted, "documents": app.documents.len(),
            "active_document": app.active_document,
            "preview_visible": app.browse.preview_visible(),
            "result_paths": app.browse.paths(),
            "requested_discard": discard && passes.is_empty(),
        });
        eprintln!("IME_FOCUS_TRACE {pass}");
        if discard && passes.is_empty() {
            ctx.request_discard("IME focus terminal batch one additional pass");
        }
        passes.push(pass);
    });
    assert!(output
        .shapes
        .iter()
        .all(|shape| shape.clip_rect.is_finite()));
    passes
}

fn ime_focus_click(
    app: &mut CedarApp,
    commands: &Receiver<Command>,
    ledger: &mut Vec<Command>,
    probe: &str,
) {
    let pos = visible_center(app, probe);
    for (label, events) in [
        ("pointer_move", vec![egui::Event::PointerMoved(pos)]),
        ("pointer_press", pointer(pos, true)),
        ("pointer_release", pointer(pos, false)),
    ] {
        let t = now(app) + 0.001;
        ime_focus_frame(app, commands, ledger, label, t, events, false);
    }
    assert_eq!(
        focused(app),
        Some(recorded_response(app, probe).id),
        "fixture: actual pointer transfer"
    );
}

fn ime_focus_case(origin: &str, ending: ImeFocusEnding, discard: bool) {
    use ImeFocusEnding::*;
    let destination = if origin == "browse_query" {
        "browse_scope"
    } else {
        "browse_query"
    };
    eprintln!("IME_FOCUS_CASE origin={origin} destination={destination} ending={ending:?} discard={discard}");
    let (mut app, commands) = discovery_fixture();
    enter(&mut app, false);
    let departure = selection_signature(selection(&app));
    let undo = undo_texts(&app);
    let history = (
        app.location_history.back.len(),
        app.location_history.forward.len(),
    );
    edit(&mut app, "browse_query", "stable");
    edit(&mut app, "browse_scope", "src");
    select_control(&mut app, origin);
    let mut ledger: Vec<Command> = commands.try_iter().collect();
    assert!(ledger.is_empty(), "fixture dispatched before composition");
    assert_eq!(focused(&app), Some(recorded_response(&app, origin).id));
    let t = now(&app) + 0.001;
    ime_focus_frame(
        &mut app,
        &commands,
        &mut ledger,
        "preedit",
        t,
        vec![
            egui::Event::Ime(egui::ImeEvent::Enabled),
            egui::Event::Ime(egui::ImeEvent::Preedit("预".into())),
        ],
        false,
    );
    assert_eq!(
        (app.browse.query(), app.browse.scope()),
        if origin == "browse_query" {
            ("预", "src")
        } else {
            ("stable", "预")
        }
    );
    let t = now(&app) + 1.0;
    ime_focus_frame(
        &mut app,
        &commands,
        &mut ledger,
        "preedit_idle",
        t,
        vec![],
        false,
    );
    assert!(ledger.is_empty(), "preedit must not search");
    let terminal = |cancel| {
        vec![
            egui::Event::Ime(if cancel {
                egui::ImeEvent::Preedit(String::new())
            } else {
                egui::ImeEvent::Commit("终".into())
            }),
            egui::Event::Ime(egui::ImeEvent::Disabled),
        ]
    };
    let committed_at;
    if matches!(ending, AcceptedThenFocus) {
        committed_at = now(&app) + 0.001;
        ime_focus_frame(
            &mut app,
            &commands,
            &mut ledger,
            "accepted_terminal",
            committed_at,
            terminal(false),
            discard,
        );
        assert_eq!(
            if origin == "browse_query" {
                app.browse.query()
            } else {
                app.browse.scope()
            },
            "终",
            "positive control must prove actual acceptance"
        );
        ime_focus_click(&mut app, &commands, &mut ledger, destination);
    } else if matches!(ending, FocusThenLateCommit) {
        ime_focus_click(&mut app, &commands, &mut ledger, destination);
        committed_at = now(&app) + 0.001;
        ime_focus_frame(
            &mut app,
            &commands,
            &mut ledger,
            "late_terminal",
            committed_at,
            terminal(false),
            discard,
        );
    } else {
        let pos = visible_center(&app, destination);
        let press = egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        };
        let release = egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        };
        let cancel = matches!(ending, CancelBeforePointer | CancelBetweenPointer);
        let mut events = Vec::new();
        if matches!(ending, CancelBeforePointer | CommitBeforePointer) {
            events.extend(terminal(cancel));
        }
        events.extend([egui::Event::PointerMoved(pos), press]);
        if matches!(ending, CancelBetweenPointer | CommitBetweenPointer) {
            events.extend(terminal(cancel));
        }
        events.push(release);
        committed_at = now(&app) + 0.001;
        let passes = ime_focus_frame(
            &mut app,
            &commands,
            &mut ledger,
            "same_batch_terminal_pointer",
            committed_at,
            events,
            discard,
        );
        let expected_focus = format!("Some({:?})", recorded_response(&app, destination).id);
        assert_eq!(
            passes[0]["focus"].as_str(),
            Some(expected_focus.as_str()),
            "fixture: transfer must occur in first update pass"
        );
    }
    assert_eq!(
        focused(&app),
        Some(recorded_response(&app, destination).id),
        "fixture: terminal transfer must be genuine"
    );
    let final_query = app.browse.query().to_owned();
    let final_scope = app.browse.scope().to_owned();
    let accepted = if origin == "browse_query" {
        final_query == "终"
    } else {
        final_scope == "终"
    };
    let destination_unchanged = if origin == "browse_query" {
        final_scope == "src"
    } else {
        final_query == "stable"
    };
    let cancel = matches!(ending, CancelBeforePointer | CancelBetweenPointer);
    let should_search = accepted && !cancel && destination_unchanged;
    let mut failures = Vec::new();
    if !destination_unchanged {
        failures.push("origin terminal contaminated destination".to_owned());
    }
    if !ledger.is_empty() {
        failures.push("command emitted before full debounce".to_owned());
    }
    ime_focus_frame(
        &mut app,
        &commands,
        &mut ledger,
        "deadline_249ms",
        committed_at + 0.249,
        vec![],
        false,
    );
    if !ledger.is_empty() {
        failures.push("command emitted before 250ms".to_owned());
    }
    ime_focus_frame(
        &mut app,
        &commands,
        &mut ledger,
        "deadline_250ms",
        committed_at + 0.250,
        vec![],
        false,
    );
    ime_focus_frame(
        &mut app,
        &commands,
        &mut ledger,
        "terminal_idle",
        committed_at + 2.0,
        vec![],
        false,
    );
    assert_eq!(
        (app.browse.query(), app.browse.scope()),
        (final_query.as_str(), final_scope.as_str()),
        "idle must preserve final widget bytes"
    );
    let expected_count = usize::from(should_search);
    if ledger.len() != expected_count {
        failures.push(format!("automatic commands={}, expected={expected_count}; actual origin accepted={accepted}; cancel={cancel}", ledger.len()));
    }
    for command in &ledger {
        if !matches!(&command.op, Operation::FindFiles { query, scope, limit } if should_search && query == &final_query && scope == &final_scope && *limit == cedar_protocol::DEFAULT_FILE_DISCOVERY_RESULTS)
        {
            failures.push(format!(
                "unauthorized automatic operation id={} op={:?}",
                command.id, command.op
            ));
        }
    }
    assert_eq!(focused(&app), Some(recorded_response(&app, destination).id));
    assert!(app.browse.paths().is_empty() && !app.browse.preview_visible());
    assert_eq!(app.documents.len(), 1);
    assert_eq!(
        history,
        (
            app.location_history.back.len(),
            app.location_history.forward.len()
        )
    );
    assert_draft(&app, departure, &undo);
    eprintln!(
        "IME_FOCUS_RESULT {}",
        serde_json::json!({"origin": origin, "ending": format!("{ending:?}"), "discard": discard, "accepted": accepted, "destination_unchanged": destination_unchanged, "query": final_query, "scope": final_scope, "automatic_count": ledger.len(), "failures": failures})
    );
    assert!(failures.is_empty(), "{}", failures.join("; "));
    if !should_search {
        // Cancellation/ambiguous termination is not permanent: an actual fresh
        // Text event after refocusing/selecting the original control restores it.
        ime_focus_click(&mut app, &commands, &mut ledger, origin);
        let t = now(&app) + 0.001;
        ime_focus_frame(
            &mut app,
            &commands,
            &mut ledger,
            "recovery_select",
            t,
            modified_key(egui::Key::A, command_modifiers()),
            false,
        );
        let t = now(&app) + 0.001;
        ime_focus_frame(
            &mut app,
            &commands,
            &mut ledger,
            "recovery_text",
            t,
            vec![egui::Event::Text("fresh".into())],
            false,
        );
        let query = app.browse.query().to_owned();
        let scope = app.browse.scope().to_owned();
        ime_focus_frame(
            &mut app,
            &commands,
            &mut ledger,
            "recovery_249ms",
            t + 0.249,
            vec![],
            false,
        );
        assert!(ledger.is_empty());
        ime_focus_frame(
            &mut app,
            &commands,
            &mut ledger,
            "recovery_250ms",
            t + 0.250,
            vec![],
            false,
        );
        assert_eq!(ledger.len(), 1);
        assert!(
            matches!(&ledger[0].op, Operation::FindFiles { query: q, scope: s, limit } if q == &query && s == &scope && *limit == cedar_protocol::DEFAULT_FILE_DISCOVERY_RESULTS)
        );
        assert_draft(&app, departure, &undo);
    }
}

macro_rules! ime_focus_test {
    ($name:ident, $origin:literal, $ending:ident) => {
        #[test]
        fn $name() {
            ime_focus_case($origin, ImeFocusEnding::$ending, false);
        }
    };
}
ime_focus_test!(
    files_live_ime_focus_query_to_scope_accepted_then_focus,
    "browse_query",
    AcceptedThenFocus
);
ime_focus_test!(
    files_live_ime_focus_query_to_scope_cancel_before_pointer,
    "browse_query",
    CancelBeforePointer
);
ime_focus_test!(
    files_live_ime_focus_query_to_scope_cancel_between_pointer,
    "browse_query",
    CancelBetweenPointer
);
ime_focus_test!(
    files_live_ime_focus_query_to_scope_commit_before_pointer,
    "browse_query",
    CommitBeforePointer
);
ime_focus_test!(
    files_live_ime_focus_query_to_scope_commit_between_pointer,
    "browse_query",
    CommitBetweenPointer
);
ime_focus_test!(
    files_live_ime_focus_query_to_scope_focus_then_late_commit,
    "browse_query",
    FocusThenLateCommit
);
ime_focus_test!(
    files_live_ime_focus_scope_to_query_accepted_then_focus,
    "browse_scope",
    AcceptedThenFocus
);
ime_focus_test!(
    files_live_ime_focus_scope_to_query_cancel_before_pointer,
    "browse_scope",
    CancelBeforePointer
);
ime_focus_test!(
    files_live_ime_focus_scope_to_query_cancel_between_pointer,
    "browse_scope",
    CancelBetweenPointer
);
ime_focus_test!(
    files_live_ime_focus_scope_to_query_commit_before_pointer,
    "browse_scope",
    CommitBeforePointer
);
ime_focus_test!(
    files_live_ime_focus_scope_to_query_commit_between_pointer,
    "browse_scope",
    CommitBetweenPointer
);
ime_focus_test!(
    files_live_ime_focus_scope_to_query_focus_then_late_commit,
    "browse_scope",
    FocusThenLateCommit
);

#[test]
fn files_live_ime_focus_query_to_scope_commit_before_pointer_requested_discard() {
    ime_focus_case("browse_query", ImeFocusEnding::CommitBeforePointer, true);
}

fn ime_focus_extra_start(
    origin: &str,
    preedit: &str,
) -> (CedarApp, Receiver<Command>, Vec<Command>) {
    ime_focus_extra_start_with_undo_checkpoint(origin, preedit, false)
}

fn ime_focus_extra_start_with_undo_checkpoint(
    origin: &str,
    preedit: &str,
    checkpoint: bool,
) -> (CedarApp, Receiver<Command>, Vec<Command>) {
    let (mut app, commands) = discovery_fixture();
    enter(&mut app, false);
    edit(&mut app, "browse_query", "stable");
    edit(&mut app, "browse_scope", "src");
    select_control(&mut app, origin);
    let mut ledger: Vec<_> = commands.try_iter().collect();
    assert!(ledger.is_empty());
    if checkpoint {
        let t = now(&app) + 0.001;
        ime_focus_frame(
            &mut app,
            &commands,
            &mut ledger,
            "undo_enable",
            t,
            vec![egui::Event::Ime(egui::ImeEvent::Enabled)],
            false,
        );
        let t = now(&app) + 1.1;
        ime_focus_frame(
            &mut app,
            &commands,
            &mut ledger,
            "undo_checkpoint",
            t,
            vec![],
            false,
        );
        assert!(ledger.is_empty());
    }
    let t = now(&app) + 0.001;
    ime_focus_frame(
        &mut app,
        &commands,
        &mut ledger,
        "extra_preedit",
        t,
        vec![
            egui::Event::Ime(egui::ImeEvent::Enabled),
            egui::Event::Ime(egui::ImeEvent::Preedit(preedit.into())),
        ],
        false,
    );
    assert_eq!(
        if origin == "browse_query" {
            app.browse.query()
        } else {
            app.browse.scope()
        },
        preedit
    );
    let t = now(&app) + 1.0;
    ime_focus_frame(
        &mut app,
        &commands,
        &mut ledger,
        "extra_preedit_idle",
        t,
        vec![],
        false,
    );
    assert!(ledger.is_empty());
    (app, commands, ledger)
}

fn ime_focus_extra_expect_one(
    app: &mut CedarApp,
    commands: &Receiver<Command>,
    ledger: &mut Vec<Command>,
    t: f64,
    query: &str,
    scope: &str,
) {
    ime_focus_frame(
        app,
        commands,
        ledger,
        "extra_249ms",
        t + 0.249,
        vec![],
        false,
    );
    assert!(ledger.is_empty());
    ime_focus_frame(
        app,
        commands,
        ledger,
        "extra_250ms",
        t + 0.250,
        vec![],
        false,
    );
    assert_eq!(ledger.len(), 1);
    assert!(
        matches!(&ledger[0].op, Operation::FindFiles { query: q, scope: s, limit } if q == query && s == scope && *limit == cedar_protocol::DEFAULT_FILE_DISCOVERY_RESULTS)
    );
    ime_focus_frame(app, commands, ledger, "extra_idle", t + 2.0, vec![], false);
    assert_eq!(ledger.len(), 1);
}

#[test]
fn files_live_ime_focus_returned_owner_cannot_revive_late_terminal() {
    for origin in ["browse_query", "browse_scope"] {
        let destination = if origin == "browse_query" {
            "browse_scope"
        } else {
            "browse_query"
        };
        let (mut app, commands, mut ledger) = ime_focus_extra_start(origin, "预");
        let departure = selection_signature(selection(&app));
        let undo = undo_texts(&app);
        ime_focus_click(&mut app, &commands, &mut ledger, destination);
        ime_focus_click(&mut app, &commands, &mut ledger, origin);
        let t = now(&app) + 1.0;
        ime_focus_frame(
            &mut app,
            &commands,
            &mut ledger,
            "returned_owner_idle",
            t,
            vec![],
            false,
        );
        assert!(ledger.is_empty());
        assert_eq!(
            app.browse.ime_focus_observation()["composition_owner_valid"],
            false
        );
        let t = now(&app) + 0.001;
        ime_focus_frame(
            &mut app,
            &commands,
            &mut ledger,
            "returned_owner_late_terminal",
            t,
            vec![
                egui::Event::Ime(egui::ImeEvent::Commit("终".into())),
                egui::Event::Ime(egui::ImeEvent::Disabled),
            ],
            false,
        );
        let final_bytes = (app.browse.query().to_owned(), app.browse.scope().to_owned());
        ime_focus_frame(
            &mut app,
            &commands,
            &mut ledger,
            "returned_owner_late_idle",
            t + 2.0,
            vec![],
            false,
        );
        assert!(
            ledger.is_empty(),
            "returning focus must not revive old ownership"
        );
        assert_eq!(
            (app.browse.query(), app.browse.scope()),
            (final_bytes.0.as_str(), final_bytes.1.as_str())
        );
        // A genuinely fresh lifecycle may establish ownership again.
        select_control(&mut app, origin);
        let t = now(&app) + 0.001;
        ime_focus_frame(
            &mut app,
            &commands,
            &mut ledger,
            "fresh_lifecycle",
            t,
            vec![
                egui::Event::Ime(egui::ImeEvent::Enabled),
                egui::Event::Ime(egui::ImeEvent::Preedit("新".into())),
            ],
            false,
        );
        let t = now(&app) + 0.001;
        ime_focus_frame(
            &mut app,
            &commands,
            &mut ledger,
            "fresh_lifecycle_commit",
            t,
            vec![
                egui::Event::Ime(egui::ImeEvent::Commit("新终".into())),
                egui::Event::Ime(egui::ImeEvent::Disabled),
            ],
            false,
        );
        let query = app.browse.query().to_owned();
        let scope = app.browse.scope().to_owned();
        assert_eq!(
            if origin == "browse_query" {
                &query
            } else {
                &scope
            },
            "新终"
        );
        ime_focus_extra_expect_one(&mut app, &commands, &mut ledger, t, &query, &scope);
        assert_draft(&app, departure, &undo);
    }
}

#[test]
fn files_live_ime_focus_same_value_unicode_commit_keeps_interval_after_transfer() {
    for origin in ["browse_query", "browse_scope"] {
        let destination = if origin == "browse_query" {
            "browse_scope"
        } else {
            "browse_query"
        };
        for text in ["预", "中文😀𐐀e\u{301}"] {
            let (mut app, commands, mut ledger) = ime_focus_extra_start(origin, text);
            let t = now(&app) + 0.001;
            ime_focus_frame(
                &mut app,
                &commands,
                &mut ledger,
                "same_value_commit",
                t,
                vec![
                    egui::Event::Ime(egui::ImeEvent::Commit(text.into())),
                    egui::Event::Ime(egui::ImeEvent::Disabled),
                ],
                false,
            );
            assert_eq!(
                if origin == "browse_query" {
                    app.browse.query()
                } else {
                    app.browse.scope()
                },
                text
            );
            ime_focus_click(&mut app, &commands, &mut ledger, destination);
            let query = app.browse.query().to_owned();
            let scope = app.browse.scope().to_owned();
            ime_focus_extra_expect_one(&mut app, &commands, &mut ledger, t, &query, &scope);
        }
    }
}

#[test]
fn files_live_ime_focus_mixed_text_or_paste_commit_is_not_rejected() {
    for origin in ["browse_query", "browse_scope"] {
        for paste in [false, true] {
            for commit_first in [false, true] {
                let (mut app, commands, mut ledger) = ime_focus_extra_start(origin, "预");
                let text = if paste {
                    egui::Event::Paste("后😀".into())
                } else {
                    egui::Event::Text("后😀".into())
                };
                let commit = egui::Event::Ime(egui::ImeEvent::Commit("终".into()));
                let mut events = if commit_first {
                    vec![commit, text]
                } else {
                    vec![text, commit]
                };
                events.push(egui::Event::Ime(egui::ImeEvent::Disabled));
                let t = now(&app) + 0.001;
                ime_focus_frame(
                    &mut app,
                    &commands,
                    &mut ledger,
                    "mixed_commit",
                    t,
                    events,
                    false,
                );
                assert_eq!(
                    if origin == "browse_query" {
                        app.browse.query()
                    } else {
                        app.browse.scope()
                    },
                    "终后😀"
                );
                let query = app.browse.query().to_owned();
                let scope = app.browse.scope().to_owned();
                ime_focus_extra_expect_one(&mut app, &commands, &mut ledger, t, &query, &scope);
            }
        }
    }
}

#[test]
fn files_live_ime_focus_unicode_scalar_limit_preserves_protocol_rejection() {
    let commit = "终😀𐐀e\u{301}".repeat(1000);
    let expected: String = commit.chars().take(4096).collect();
    for origin in ["browse_query", "browse_scope"] {
        let (mut app, commands, mut ledger) = ime_focus_extra_start(origin, "预");
        let t = now(&app) + 0.001;
        ime_focus_frame(
            &mut app,
            &commands,
            &mut ledger,
            "unicode_limit_commit",
            t,
            vec![
                egui::Event::Ime(egui::ImeEvent::Commit(commit.clone())),
                egui::Event::Ime(egui::ImeEvent::Disabled),
            ],
            false,
        );
        assert_eq!(
            if origin == "browse_query" {
                app.browse.query()
            } else {
                app.browse.scope()
            },
            expected
        );
        let query = app.browse.query().to_owned();
        let scope = app.browse.scope().to_owned();
        assert!(app.browse.ime_focus_observation()["settlement"].is_object());
        let error = cedar_protocol::validate_file_discovery_request(
            &query,
            &scope,
            cedar_protocol::DEFAULT_FILE_DISCOVERY_RESULTS,
        )
        .expect_err("4096 multibyte scalars exceed the existing protocol byte limit");
        ime_focus_frame(
            &mut app,
            &commands,
            &mut ledger,
            "unicode_limit_249ms",
            t + 0.249,
            vec![],
            false,
        );
        assert!(ledger.is_empty());
        ime_focus_frame(
            &mut app,
            &commands,
            &mut ledger,
            "unicode_limit_250ms",
            t + 0.250,
            vec![],
            false,
        );
        assert!(
            ledger.is_empty(),
            "native scalar acceptance does not bypass protocol validation"
        );
        let expected_error = error.to_string();
        assert_eq!(app.browse.preview_message(), Some(expected_error.as_str()));
        assert_eq!(
            (app.browse.query(), app.browse.scope()),
            (query.as_str(), scope.as_str())
        );
    }
}

#[test]
fn files_live_ime_focus_direct_commit_without_preedit_keeps_existing_path() {
    for origin in ["browse_query", "browse_scope"] {
        let (mut app, commands) = discovery_fixture();
        enter(&mut app, false);
        edit(&mut app, "browse_query", "stable");
        edit(&mut app, "browse_scope", "src");
        select_control(&mut app, origin);
        let mut ledger: Vec<_> = commands.try_iter().collect();
        assert!(ledger.is_empty());
        let t = now(&app) + 0.001;
        ime_focus_frame(
            &mut app,
            &commands,
            &mut ledger,
            "direct_commit",
            t,
            vec![
                egui::Event::Ime(egui::ImeEvent::Commit("直接😀".into())),
                egui::Event::Ime(egui::ImeEvent::Disabled),
            ],
            false,
        );
        assert_eq!(
            if origin == "browse_query" {
                app.browse.query()
            } else {
                app.browse.scope()
            },
            "直接😀"
        );
        let query = app.browse.query().to_owned();
        let scope = app.browse.scope().to_owned();
        ime_focus_extra_expect_one(&mut app, &commands, &mut ledger, t, &query, &scope);
    }
}

#[test]
fn files_live_ime_focus_handoff_disabled_preserves_final_bytes_and_explicit_find() {
    for origin in ["browse_query", "browse_scope"] {
        let destination = if origin == "browse_query" {
            "browse_scope"
        } else {
            "browse_query"
        };
        let (mut app, commands, mut ledger) = ime_focus_extra_start(origin, "预");
        ime_focus_click(&mut app, &commands, &mut ledger, destination);
        let t = now(&app) + 0.001;
        ime_focus_frame(
            &mut app,
            &commands,
            &mut ledger,
            "handoff_disabled",
            t,
            vec![egui::Event::Ime(egui::ImeEvent::Disabled)],
            false,
        );
        let query = app.browse.query().to_owned();
        let scope = app.browse.scope().to_owned();
        ime_focus_frame(
            &mut app,
            &commands,
            &mut ledger,
            "handoff_disabled_idle",
            t + 2.0,
            vec![],
            false,
        );
        assert!(ledger.is_empty());
        assert_eq!(
            (app.browse.query(), app.browse.scope()),
            (query.as_str(), scope.as_str())
        );
        let t = now(&app) + 0.001;
        ime_focus_frame(
            &mut app,
            &commands,
            &mut ledger,
            "handoff_explicit_find",
            t,
            key(egui::Key::Enter),
            false,
        );
        assert_eq!(ledger.len(), 1);
        assert!(
            matches!(&ledger[0].op, Operation::FindFiles { query: q, scope: s, .. } if q == &query && s == &scope)
        );
    }
}

#[test]
fn files_live_ime_focus_old_singleflight_reply_cannot_restart_rejected_terminal() {
    for origin in ["browse_query", "browse_scope"] {
        let destination = if origin == "browse_query" {
            "browse_scope"
        } else {
            "browse_query"
        };
        let (mut app, commands) = discovery_fixture();
        enter(&mut app, false);
        edit(&mut app, "browse_query", "stable");
        edit(&mut app, "browse_scope", "src");
        settle_files(&mut app);
        let first = expect_discovery(&commands, "stable", "src");
        select_control(&mut app, origin);
        let mut ledger = Vec::new();
        let t = now(&app) + 0.001;
        ime_focus_frame(
            &mut app,
            &commands,
            &mut ledger,
            "pending_preedit",
            t,
            vec![
                egui::Event::Ime(egui::ImeEvent::Enabled),
                egui::Event::Ime(egui::ImeEvent::Preedit("预".into())),
            ],
            false,
        );
        ime_focus_click(&mut app, &commands, &mut ledger, destination);
        let t = now(&app) + 0.001;
        ime_focus_frame(
            &mut app,
            &commands,
            &mut ledger,
            "pending_late_terminal",
            t,
            vec![
                egui::Event::Ime(egui::ImeEvent::Commit("终".into())),
                egui::Event::Ime(egui::ImeEvent::Disabled),
            ],
            false,
        );
        ime_focus_frame(
            &mut app,
            &commands,
            &mut ledger,
            "pending_terminal_idle",
            t + 2.0,
            vec![],
            false,
        );
        assert!(
            ledger.is_empty(),
            "old physical flight still owns the only slot"
        );
        reply(
            &mut app,
            first.id,
            Ok(discovery_result("stable", "src", &["src/old.rs"], vec![])),
        );
        let t = now(&app) + 0.001;
        ime_focus_frame(
            &mut app,
            &commands,
            &mut ledger,
            "retired_old_reply",
            t,
            vec![],
            false,
        );
        ime_focus_frame(
            &mut app,
            &commands,
            &mut ledger,
            "retired_old_reply_idle",
            t + 2.0,
            vec![],
            false,
        );
        assert!(
            ledger.is_empty(),
            "releasing old flight must not dispatch preedit"
        );
        assert!(app.browse.paths().is_empty());
        edit(&mut app, origin, "fresh");
        let t = now(&app);
        let query = app.browse.query().to_owned();
        let scope = app.browse.scope().to_owned();
        ime_focus_extra_expect_one(&mut app, &commands, &mut ledger, t, &query, &scope);
        assert_ne!(first.id, ledger[0].id);
        let prior_error = app.error.clone();
        reply(&mut app, first.id, Err("stale rejected old request".into()));
        let t = now(&app) + 0.001;
        ime_focus_frame(
            &mut app,
            &commands,
            &mut ledger,
            "duplicate_old_reply",
            t,
            vec![],
            false,
        );
        assert_eq!(app.error, prior_error);
        assert_eq!(ledger.len(), 1);
        assert!(app.browse.paths().is_empty());
    }
}

#[test]
fn files_live_ime_focus_disabled_then_late_commit_cannot_escape_cancelled_owner() {
    let mut failures = Vec::new();
    for origin in ["browse_query", "browse_scope"] {
        let destination = if origin == "browse_query" {
            "browse_scope"
        } else {
            "browse_query"
        };
        let (mut app, commands, mut ledger) = ime_focus_extra_start(origin, "预");
        ime_focus_click(&mut app, &commands, &mut ledger, destination);
        let t = now(&app) + 0.001;
        ime_focus_frame(
            &mut app,
            &commands,
            &mut ledger,
            "cancel_owner_disabled",
            t,
            vec![egui::Event::Ime(egui::ImeEvent::Disabled)],
            false,
        );
        ime_focus_frame(
            &mut app,
            &commands,
            &mut ledger,
            "cancel_owner_idle",
            t + 1.0,
            vec![],
            false,
        );
        assert!(ledger.is_empty());
        let query = app.browse.query().to_owned();
        let scope = app.browse.scope().to_owned();
        let t = now(&app) + 0.001;
        ime_focus_frame(
            &mut app,
            &commands,
            &mut ledger,
            "cancel_owner_late_commit",
            t,
            vec![
                egui::Event::Ime(egui::ImeEvent::Commit("终".into())),
                egui::Event::Ime(egui::ImeEvent::Disabled),
            ],
            false,
        );
        ime_focus_frame(
            &mut app,
            &commands,
            &mut ledger,
            "cancel_owner_late_249ms",
            t + 0.249,
            vec![],
            false,
        );
        assert!(ledger.is_empty());
        ime_focus_frame(
            &mut app,
            &commands,
            &mut ledger,
            "cancel_owner_late_250ms",
            t + 0.250,
            vec![],
            false,
        );
        ime_focus_frame(
            &mut app,
            &commands,
            &mut ledger,
            "cancel_owner_late_idle",
            t + 2.0,
            vec![],
            false,
        );
        assert_eq!(
            (app.browse.query(), app.browse.scope()),
            (query.as_str(), scope.as_str())
        );
        for command in &ledger {
            failures.push(format!(
                "{origin}: canceled owner escaped via late Commit: id={} op={:?}",
                command.id, command.op
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("; "));
}

#[test]
fn files_live_ime_focus_fresh_destination_lifecycle_replaces_invalid_owner() {
    for origin in ["browse_query", "browse_scope"] {
        let destination = if origin == "browse_query" {
            "browse_scope"
        } else {
            "browse_query"
        };
        let (mut app, commands, mut ledger) = ime_focus_extra_start(origin, "预");
        ime_focus_click(&mut app, &commands, &mut ledger, destination);
        select_control(&mut app, destination);
        let t = now(&app) + 0.001;
        ime_focus_frame(
            &mut app,
            &commands,
            &mut ledger,
            "destination_fresh_lifecycle",
            t,
            vec![
                egui::Event::Ime(egui::ImeEvent::Enabled),
                egui::Event::Ime(egui::ImeEvent::Preedit("新".into())),
            ],
            false,
        );
        assert_eq!(
            if destination == "browse_query" {
                app.browse.query()
            } else {
                app.browse.scope()
            },
            "新"
        );
        let t = now(&app) + 0.001;
        ime_focus_frame(
            &mut app,
            &commands,
            &mut ledger,
            "destination_fresh_commit",
            t,
            vec![
                egui::Event::Ime(egui::ImeEvent::Commit("新终".into())),
                egui::Event::Ime(egui::ImeEvent::Disabled),
            ],
            false,
        );
        assert_eq!(
            if destination == "browse_query" {
                app.browse.query()
            } else {
                app.browse.scope()
            },
            "新终"
        );
        let query = app.browse.query().to_owned();
        let scope = app.browse.scope().to_owned();
        ime_focus_extra_expect_one(&mut app, &commands, &mut ledger, t, &query, &scope);
    }
}

#[test]
fn files_live_ime_focus_handoff_barrier_releases_only_for_fresh_owned_actions() {
    for origin in ["browse_query", "browse_scope"] {
        let destination = if origin == "browse_query" {
            "browse_scope"
        } else {
            "browse_query"
        };
        for recovery in [
            "text",
            "paste",
            "same_text",
            "empty_paste",
            "backspace",
            "undo",
            "fresh_ime",
        ] {
            let (mut app, commands, mut ledger) =
                ime_focus_extra_start_with_undo_checkpoint(origin, "预", recovery == "undo");
            ime_focus_click(&mut app, &commands, &mut ledger, destination);
            let t = now(&app) + 0.001;
            ime_focus_frame(
                &mut app,
                &commands,
                &mut ledger,
                "barrier_cancel",
                t,
                vec![egui::Event::Ime(egui::ImeEvent::Disabled)],
                false,
            );
            assert_eq!(app.browse.ime_focus_observation()["handoff_barrier"], true);
            let t = now(&app) + 1.0;
            ime_focus_frame(
                &mut app,
                &commands,
                &mut ledger,
                "barrier_idle",
                t,
                vec![],
                false,
            );
            assert!(ledger.is_empty());
            if matches!(recovery, "text" | "paste" | "same_text" | "fresh_ime") {
                select_control(&mut app, origin);
            } else if recovery == "undo" {
                ime_focus_click(&mut app, &commands, &mut ledger, origin);
            }
            assert_eq!(
                app.browse.ime_focus_observation()["handoff_barrier"],
                true,
                "focus/selection alone must not release {recovery}"
            );
            let events = match recovery {
                "text" => vec![egui::Event::Text("fresh".into())],
                "paste" => vec![egui::Event::Paste("fresh".into())],
                "same_text" => vec![egui::Event::Text("预".into())],
                "empty_paste" => vec![egui::Event::Paste(String::new())],
                "backspace" => key(egui::Key::Backspace),
                "undo" => modified_key(egui::Key::Z, command_modifiers()),
                "fresh_ime" => vec![
                    egui::Event::Ime(egui::ImeEvent::Enabled),
                    egui::Event::Ime(egui::ImeEvent::Preedit("新".into())),
                ],
                _ => unreachable!(),
            };
            let t = now(&app) + 0.001;
            ime_focus_frame(&mut app, &commands, &mut ledger, recovery, t, events, false);
            let t = if recovery == "undo" {
                // Native Undo first restores only the composition selection.
                // changed() alone must not release the cancellation barrier.
                assert_eq!(app.browse.ime_focus_observation()["handoff_barrier"], true);
                assert_eq!(
                    if origin == "browse_query" {
                        app.browse.query()
                    } else {
                        app.browse.scope()
                    },
                    "预"
                );
                assert!(ledger.is_empty());
                let t = now(&app) + 0.001;
                ime_focus_frame(
                    &mut app,
                    &commands,
                    &mut ledger,
                    "undo_actual_bytes",
                    t,
                    modified_key(egui::Key::Z, command_modifiers()),
                    false,
                );
                t
            } else {
                t
            };
            assert_eq!(
                app.browse.ime_focus_observation()["handoff_barrier"],
                false,
                "fresh {recovery} must release the bounded barrier"
            );
            let t = if recovery == "fresh_ime" {
                let t = now(&app) + 0.001;
                ime_focus_frame(
                    &mut app,
                    &commands,
                    &mut ledger,
                    "barrier_fresh_commit",
                    t,
                    vec![
                        egui::Event::Ime(egui::ImeEvent::Commit("新终".into())),
                        egui::Event::Ime(egui::ImeEvent::Disabled),
                    ],
                    false,
                );
                t
            } else {
                t
            };
            let query = app.browse.query().to_owned();
            let scope = app.browse.scope().to_owned();
            if recovery == "undo" {
                assert_eq!(
                    if origin == "browse_query" {
                        query.as_str()
                    } else {
                        scope.as_str()
                    },
                    if origin == "browse_query" {
                        "stable"
                    } else {
                        "src"
                    }
                );
            }
            ime_focus_extra_expect_one(&mut app, &commands, &mut ledger, t, &query, &scope);
        }
    }
}

#[test]
fn files_live_ime_focus_handoff_late_commit_with_backspace_stays_paused() {
    for origin in ["browse_query", "browse_scope"] {
        let destination = if origin == "browse_query" {
            "browse_scope"
        } else {
            "browse_query"
        };
        let (mut app, commands, mut ledger) = ime_focus_extra_start(origin, "预");
        ime_focus_click(&mut app, &commands, &mut ledger, destination);
        let t = now(&app) + 0.001;
        ime_focus_frame(
            &mut app,
            &commands,
            &mut ledger,
            "mixed_cancel",
            t,
            vec![egui::Event::Ime(egui::ImeEvent::Disabled)],
            false,
        );
        assert_eq!(app.browse.ime_focus_observation()["handoff_barrier"], true);
        let before = if destination == "browse_query" {
            app.browse.query()
        } else {
            app.browse.scope()
        }
        .to_owned();
        let t = now(&app) + 0.001;
        let mut events = vec![egui::Event::Ime(egui::ImeEvent::Commit("迟".into()))];
        events.extend(key(egui::Key::Backspace));
        ime_focus_frame(
            &mut app,
            &commands,
            &mut ledger,
            "late_commit_backspace",
            t,
            events,
            false,
        );
        let after = if destination == "browse_query" {
            app.browse.query()
        } else {
            app.browse.scope()
        };
        assert_ne!(
            after, before,
            "the native Backspace must really change destination bytes"
        );
        assert_eq!(
            after,
            if destination == "browse_query" {
                "stabl"
            } else {
                "sr"
            }
        );
        assert_eq!(app.browse.ime_focus_observation()["handoff_barrier"], true);
        assert!(app.browse.ime_focus_observation()["settlement"].is_null());
        for (label, dt) in [
            ("mixed_249ms", 0.249),
            ("mixed_250ms", 0.250),
            ("mixed_idle", 2.0),
        ] {
            ime_focus_frame(
                &mut app,
                &commands,
                &mut ledger,
                label,
                t + dt,
                vec![],
                false,
            );
            assert!(
                ledger.is_empty(),
                "raw IME batch must not become a clean keyboard edit"
            );
        }
    }
}
