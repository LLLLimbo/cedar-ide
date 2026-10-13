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
