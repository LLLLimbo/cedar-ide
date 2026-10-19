//! Case search UI/negotiation through production raw-hook and update frames.
//! Recording workers establish deterministic app behavior only: no process,
//! filesystem, backend matching, real OS GUI, SSH or package claim is made.
use super::*;
use crate::scoped_search_tests::{
    browse_assert_status_fields_reachable, captured_frame, click, click_at_size, enter,
    fixture as scoped_fixture, focus, frame, frame_at, has_text, key, pointer, replace, reply,
    rows, settle, submit, Surface, SIZE,
};
use crate::workspace_access_tests::{recorded_clip, recorded_rect, recorded_response};
use cedar_protocol::{
    CaseSearchDescriptor, CaseSearchMatch, CaseSearchRequest, CaseSearchResult,
    ScopedSearchDescriptor, ScopedSearchIncompleteReason,
};

const UNAVAILABLE: &str = "Case-insensitive search is not available from this agent. Turn on Match case to use its existing search, or upgrade the agent.";

fn checkbox(surface: Surface) -> &'static str {
    match surface {
        Surface::Browse => "browse_match_case",
        Surface::Bottom => "search_match_case",
    }
}
fn fixture() -> (CedarApp, Receiver<Command>) {
    let (mut app, commands) = scoped_fixture();
    app.case_search = Some(CaseSearchDescriptor::V1);
    assert!(app.search_match_case);
    (app, commands)
}
fn request(scope: &str, masks: &[&str]) -> CaseSearchRequest {
    CaseSearchRequest {
        query: "needle".into(),
        scope: scope.into(),
        masks: masks.iter().map(|mask| (*mask).into()).collect(),
        limit: 500,
        case_sensitive: false,
    }
}
fn result(request: CaseSearchRequest) -> CaseSearchResult {
    CaseSearchResult {
        request,
        matches: rows()
            .into_iter()
            .map(|row| CaseSearchMatch {
                path: row.path,
                line: row.line,
                text: row.text,
            })
            .collect(),
        incomplete: false,
        reasons: vec![],
        examined_entries: 4,
        read_bytes: 128,
    }
}
fn payload(request: CaseSearchRequest) -> Payload {
    Payload::CaseMatches {
        result: result(request),
    }
}
fn configure(app: &mut CedarApp, commands: &Receiver<Command>, surface: Surface) {
    replace(app, surface.query(), "needle");
    replace(app, surface.scope(), "src");
    replace(app, surface.masks(), "*.rs; *.md");
    if app.search_match_case {
        click(app, checkbox(surface));
    }
    assert!(!app.search_match_case);
    assert!(
        commands.try_recv().is_err(),
        "editing and toggling never submits"
    );
}
fn accepted(app: &mut CedarApp, commands: &Receiver<Command>, surface: Surface) {
    configure(app, commands, surface);
    let pending = submit(app, commands, surface);
    assert!(
        matches!(&pending.op, Operation::SearchCase { request: actual }
        if *actual == request("src", &["*.rs", "*.md"]))
    );
    reply(
        app,
        pending.id,
        Ok(payload(request("src", &["*.rs", "*.md"]))),
    );
    settle(app);
    assert!(app.search_rows_active());
    assert!(recorded_response(app, &surface.row(0)).enabled());
}
fn triples(app: &CedarApp) -> Vec<(String, usize, String)> {
    app.search_results
        .iter()
        .map(|row| (row.path.clone(), row.line, row.text.clone()))
        .collect()
}
fn hello(
    descriptor: Option<CaseSearchDescriptor>,
    scoped: Option<ScopedSearchDescriptor>,
) -> Payload {
    Payload::Hello {
        protocol: cedar_protocol::PROTOCOL_VERSION,
        root: "/workspace".into(),
        agent: Some(agent_support::full_test_agent()),
        file_discovery: None,
        scoped_search: scoped,
        case_search: descriptor,
    }
}

#[test]
fn case_search_hello_exact_v1_gates_operation_independently_of_trust_and_scoped() {
    for descriptor in [
        None,
        Some(CaseSearchDescriptor::V1),
        Some(CaseSearchDescriptor { version: 2 }),
        Some(CaseSearchDescriptor { version: u32::MAX }),
        Some(CaseSearchDescriptor { version: 0 }),
    ] {
        for trust in [false, true] {
            for scoped in [None, Some(ScopedSearchDescriptor::V1)] {
                let mut app = CedarApp::empty();
                let (worker, commands) = Worker::recording();
                app.worker = Some(worker);
                app.state = ConnectionState::Connecting;
                app.connecting_form = Some(ConnectForm {
                    local_root: "/workspace".into(),
                    allow_run: trust,
                    ..Default::default()
                });
                reply(&mut app, 0, Ok(hello(descriptor, scoped)));
                if descriptor.is_some_and(|descriptor| descriptor.version == 0) {
                    assert!(!app.ready());
                    assert!(app.case_search.is_none());
                    assert!(commands.try_recv().is_err());
                    continue;
                }
                assert!(app.ready());
                assert_eq!(app.case_search, descriptor);
                let list = commands.try_recv().unwrap();
                assert!(matches!(list.op, Operation::List { .. }));
                reply(&mut app, list.id, Ok(Payload::Entries { entries: vec![] }));
                for sensitive in [false, true] {
                    let mut request = request("src", &["*.rs"]);
                    request.case_sensitive = sensitive;
                    let operation = Operation::SearchCase { request };
                    let supported = descriptor == Some(CaseSearchDescriptor::V1);
                    assert_eq!(app.operation_problem(&operation).is_none(), supported);
                    if !supported {
                        assert_eq!(app.request(operation, Job::Git), 0);
                        assert!(commands.try_recv().is_err());
                    }
                }
                assert!(app
                    .operation_problem(&Operation::Search {
                        query: "needle".into(),
                        limit: 500
                    })
                    .is_none());
                assert!(commands.try_recv().is_err());
            }
        }
    }
}

#[test]
fn case_search_only_first_validated_hello_can_set_negotiated_support() {
    for initial in [
        None,
        Some(CaseSearchDescriptor { version: 2 }),
        Some(CaseSearchDescriptor::V1),
    ] {
        let mut app = CedarApp::empty();
        let (worker, commands) = Worker::recording();
        app.worker = Some(worker);
        app.state = ConnectionState::Connecting;
        app.connecting_form = Some(ConnectForm {
            local_root: "/workspace".into(),
            ..Default::default()
        });
        reply(&mut app, 0, Ok(hello(initial, None)));
        let list = commands.try_recv().unwrap();
        reply(&mut app, list.id, Ok(Payload::Entries { entries: vec![] }));
        for late in [
            None,
            Some(CaseSearchDescriptor::V1),
            Some(CaseSearchDescriptor { version: 2 }),
        ] {
            reply(
                &mut app,
                0,
                Ok(hello(late, Some(ScopedSearchDescriptor::V1))),
            );
            assert_eq!(
                app.case_search, initial,
                "late Hello cannot upgrade or downgrade support"
            );
            assert!(commands.try_recv().is_err());
        }
    }
}

#[test]
fn case_search_both_surfaces_share_default_and_route_only_on_explicit_submit() {
    for surface in [Surface::Browse, Surface::Bottom] {
        let (mut app, commands) = fixture();
        enter(&mut app, surface);
        replace(&mut app, surface.query(), "needle");
        let legacy = submit(&mut app, &commands, surface);
        assert!(matches!(&legacy.op, Operation::Search { query, limit: 500 } if query == "needle"));
        reply(
            &mut app,
            legacy.id,
            Ok(Payload::Matches {
                matches: rows(),
                truncated: false,
            }),
        );
        settle(&mut app);
        replace(&mut app, surface.scope(), "src");
        replace(&mut app, surface.masks(), "*.rs; *.md");
        let scoped = submit(&mut app, &commands, surface);
        assert!(matches!(&scoped.op, Operation::SearchIn { request }
            if request.scope == "src" && request.masks == ["*.rs", "*.md"]));
        reply(
            &mut app,
            scoped.id,
            Ok(crate::scoped_search_tests::payload(
                crate::scoped_search_tests::request("needle", "src", &["*.rs", "*.md"]),
                rows(),
            )),
        );
        settle(&mut app);
        click(&mut app, checkbox(surface));
        assert!(!app.search_match_case);
        assert!(!app.search_rows_active());
        assert!(commands.try_recv().is_err());
        let filtered = submit(&mut app, &commands, surface);
        assert!(
            matches!(&filtered.op, Operation::SearchCase { request: actual }
            if *actual == request("src", &["*.rs", "*.md"]))
        );
        reply(
            &mut app,
            filtered.id,
            Ok(payload(request("src", &["*.rs", "*.md"]))),
        );
        settle(&mut app);
        replace(&mut app, surface.scope(), "");
        replace(&mut app, surface.masks(), "");
        let root = submit(&mut app, &commands, surface);
        assert!(
            matches!(&root.op, Operation::SearchCase { request: actual }
            if *actual == request("", &[])),
            "unchecked root must never revive Search"
        );
        reply(&mut app, root.id, Ok(payload(request("", &[]))));
        settle(&mut app);
        if matches!(surface, Surface::Browse) {
            app.browse_dismiss();
        }
        enter(
            &mut app,
            match surface {
                Surface::Browse => Surface::Bottom,
                Surface::Bottom => Surface::Browse,
            },
        );
        assert!(
            !app.search_match_case,
            "the other surface shows the shared false draft"
        );
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn case_search_case_only_peer_supports_root_and_filtered_without_scoped_descriptor() {
    for surface in [Surface::Browse, Surface::Bottom] {
        let (mut app, commands) = fixture();
        app.scoped_search = None;
        app.agent_info
            .as_mut()
            .unwrap()
            .capabilities
            .retain(|name| name != "search");
        assert!(!app.backend_supports("search"));
        enter(&mut app, surface);
        configure(&mut app, &commands, surface);
        for filtered in [true, false] {
            if !filtered {
                replace(&mut app, surface.scope(), "");
                replace(&mut app, surface.masks(), "");
            }
            let expected = if filtered {
                request("src", &["*.rs", "*.md"])
            } else {
                request("", &[])
            };
            let command = submit(&mut app, &commands, surface);
            assert!(
                matches!(&command.op, Operation::SearchCase { request } if *request == expected)
            );
            reply(&mut app, command.id, Ok(payload(expected)));
            settle(&mut app);
            assert!(app.search_rows_active());
        }
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn case_search_unavailable_false_keeps_checkbox_usable_filters_exact_and_zero_fallback() {
    for surface in [Surface::Browse, Surface::Bottom] {
        for descriptor in [
            None,
            Some(CaseSearchDescriptor { version: 2 }),
            Some(CaseSearchDescriptor { version: u32::MAX }),
        ] {
            let (mut app, commands) = fixture();
            app.case_search = descriptor;
            enter(&mut app, surface);
            configure(&mut app, &commands, surface);
            assert_eq!(app.search_scope, "src");
            assert_eq!(app.search_masks, "*.rs; *.md");
            let output = frame(&mut app, vec![]);
            assert!(has_text(&output, UNAVAILABLE));
            assert!(recorded_response(&app, checkbox(surface)).enabled());
            assert!(!recorded_response(&app, surface.submit()).enabled());
            app.search();
            assert!(commands.try_recv().is_err());
            replace(&mut app, surface.scope(), "");
            replace(&mut app, surface.masks(), "");
            app.search();
            assert!(!app.search_match_case);
            assert!(
                commands.try_recv().is_err(),
                "clearing filters never bypasses unavailable case search"
            );
            click(&mut app, checkbox(surface));
            assert!(app.search_match_case);
            assert!(
                commands.try_recv().is_err(),
                "restoring checked mode still requires submit"
            );
            let command = submit(&mut app, &commands, surface);
            assert!(matches!(command.op, Operation::Search { .. }));
        }
    }
}

#[test]
fn case_search_rejects_every_echo_field_and_order_before_accepting_any_rows_or_fonts() {
    for surface in [Surface::Browse, Surface::Bottom] {
        for field in 0..6 {
            let (mut app, commands) = fixture();
            enter(&mut app, surface);
            accepted(&mut app, &commands, surface);
            let retained = app.search_state.accepted.clone();
            let historical = triples(&app);
            let pending = submit(&mut app, &commands, surface);
            let mut bad = result(request("src", &["*.rs", "*.md"]));
            match field {
                0 => bad.request.query.push('x'),
                1 => bad.request.scope = "other".into(),
                2 => bad.request.masks.push("*".into()),
                3 => bad.request.masks.reverse(),
                4 => bad.request.limit -= 1,
                _ => bad.request.case_sensitive = true,
            }
            bad.matches[0].text = "中文 rejected echo".into();
            reply(
                &mut app,
                pending.id,
                Ok(Payload::CaseMatches { result: bad }),
            );
            frame(&mut app, vec![]);
            assert_eq!(
                app.search_state.accepted, retained,
                "{surface:?}, field={field}"
            );
            assert_eq!(triples(&app), historical);
            assert!(!app.search_rows_active());
            assert!(app.search_state.pending.is_none());
            assert!(app.error.is_some());
            assert!(!app.cjk_seen);
            assert!(app.system_fonts.needs_probe());
            assert!(commands.try_recv().is_err());
        }
    }
}

#[test]
fn case_search_revalidates_rows_counters_incompleteness_and_wrong_payload_before_conversion() {
    for surface in [Surface::Browse, Surface::Bottom] {
        for invalid in 0..10 {
            let (mut app, commands) = fixture();
            enter(&mut app, surface);
            configure(&mut app, &commands, surface);
            let pending = submit(&mut app, &commands, surface);
            let mut bad = result(request("src", &["*.rs", "*.md"]));
            bad.matches[0].text = "中文 invalid batch".into();
            let response = match invalid {
                0 => {
                    bad.matches[2].path = "outside.rs".into();
                    Payload::CaseMatches { result: bad }
                }
                1 => {
                    bad.matches[2].path = "src/case.RS".into();
                    Payload::CaseMatches { result: bad }
                }
                2 => {
                    bad.matches[2].line = 0;
                    Payload::CaseMatches { result: bad }
                }
                3 => {
                    bad.matches[2].text.push('\n');
                    Payload::CaseMatches { result: bad }
                }
                4 => {
                    bad.matches.reverse();
                    Payload::CaseMatches { result: bad }
                }
                5 => {
                    bad.matches[1] = bad.matches[0].clone();
                    Payload::CaseMatches { result: bad }
                }
                6 => {
                    bad.examined_entries = 0;
                    Payload::CaseMatches { result: bad }
                }
                7 => {
                    bad.incomplete = true;
                    Payload::CaseMatches { result: bad }
                }
                8 => Payload::Matches {
                    matches: rows(),
                    truncated: false,
                },
                _ => crate::scoped_search_tests::payload(
                    crate::scoped_search_tests::request("needle", "src", &["*.rs", "*.md"]),
                    rows(),
                ),
            };
            reply(&mut app, pending.id, Ok(response));
            frame(&mut app, vec![]);
            assert!(
                app.search_results.is_empty(),
                "{surface:?}, invalid={invalid}"
            );
            assert!(app.search_state.accepted.is_none());
            assert!(!app.search_rows_active());
            assert!(app.error.is_some());
            assert!(!app.cjk_seen);
            assert!(app.system_fonts.needs_probe());
            assert!(commands.try_recv().is_err());
        }
    }
}

#[test]
fn case_search_toggle_and_toggle_back_retire_authority_without_cancelling_physical_search() {
    for surface in [Surface::Browse, Surface::Bottom] {
        for checked in [false, true] {
            for late in 0..3 {
                let (mut app, commands) = fixture();
                enter(&mut app, surface);
                if checked {
                    crate::scoped_search_tests::accepted(&mut app, &commands, surface);
                } else {
                    accepted(&mut app, &commands, surface);
                }
                let retained = app.search_state.accepted.clone();
                let historical = triples(&app);
                let pending = submit(&mut app, &commands, surface);
                assert!(app.search_busy());
                assert!(app.search_state.pending.is_some());
                click(&mut app, checkbox(surface));
                assert_eq!(app.search_match_case, !checked);
                assert!(!app.search_rows_active());
                assert!(app.search_state.pending.is_none());
                assert!(
                    app.search_busy(),
                    "the physical Search job still owns its single flight"
                );
                assert!(app.pending.contains_key(&pending.id));
                assert!(!recorded_response(&app, surface.submit()).enabled());
                click(&mut app, checkbox(surface));
                assert_eq!(app.search_match_case, checked);
                assert!(
                    !app.search_rows_active(),
                    "returning to identical tuple cannot reactivate it"
                );
                let before_error = app.error.clone();
                let response = match late {
                    0 => {
                        if checked {
                            Ok(crate::scoped_search_tests::payload(
                                crate::scoped_search_tests::request("needle", "src", &["*.rs"]),
                                rows(),
                            ))
                        } else {
                            Ok(payload(request("src", &["*.rs", "*.md"])))
                        }
                    }
                    1 => Err("中文 retired case search failure".into()),
                    _ => Ok(Payload::GitStatus {
                        text: "中文 retired malformed payload".into(),
                    }),
                };
                reply(&mut app, pending.id, response);
                frame(&mut app, vec![]);
                assert_eq!(app.search_state.accepted, retained);
                assert_eq!(triples(&app), historical);
                assert_eq!(app.error, before_error);
                assert!(!app.search_busy());
                assert!(!app.search_rows_active());
                assert!(!app.has_search_preview() && !app.browse.preview_visible());
                assert!(!app.cjk_seen);
                assert!(commands.try_recv().is_err());
                let fresh = submit(&mut app, &commands, surface);
                assert!(
                    fresh.id > pending.id,
                    "only fresh explicit submit creates new authority"
                );
            }
        }
    }
}

#[test]
fn case_search_toggle_retires_accepted_dirty_preview_without_editor_or_navigation_changes() {
    for surface in [Surface::Browse, Surface::Bottom] {
        let (mut app, commands) = fixture();
        enter(&mut app, surface);
        accepted(&mut app, &commands, surface);
        let text = app.documents[0].text.clone();
        let saved = app.documents[0].saved_text.clone();
        let version = app.documents[0].edit_version;
        focus(&mut app, &surface.row(0));
        frame(&mut app, key(egui::Key::Space, egui::Modifiers::NONE));
        settle(&mut app);
        assert_eq!(
            match surface {
                Surface::Browse => app.browse_preview_text(),
                Surface::Bottom => app.search_preview_text(),
            },
            Some(text.as_str())
        );
        let epoch = app.navigation_epoch;
        click(&mut app, checkbox(surface));
        assert!(!app.has_search_preview() && !app.browse.preview_visible());
        assert!(!app.search_rows_active());
        assert_eq!(app.documents[0].text, text);
        assert_eq!(app.documents[0].saved_text, saved);
        assert_eq!(app.documents[0].edit_version, version);
        assert_eq!(app.navigation_epoch, epoch);
        assert!(app.location_history.back.is_empty());
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn case_search_mode_and_route_are_part_of_retained_source_authority() {
    for surface in [Surface::Browse, Surface::Bottom] {
        let (mut app, commands) = fixture();
        enter(&mut app, surface);
        accepted(&mut app, &commands, surface);
        let source = app.search_state.accepted.clone().unwrap();
        assert_eq!(source.specification.route, search_keyboard::Route::CaseV1);
        assert!(!source.specification.case_sensitive);
        assert_eq!(
            source.specification.case_request(),
            request("src", &["*.rs", "*.md"])
        );
        for part in 0..3 {
            let mut altered = source.clone();
            match part {
                0 => altered.specification.case_sensitive = true,
                1 => altered.specification.route = search_keyboard::Route::ScopedV1,
                _ => altered.specification.route = search_keyboard::Route::Legacy,
            }
            app.search_state.accepted = Some(altered);
            assert!(!app.search_rows_active(), "{surface:?}, part={part}");
        }
        app.search_state.accepted = Some(source);
        assert!(app.search_rows_active());
        app.case_search = Some(CaseSearchDescriptor { version: 2 });
        assert!(!app.search_rows_active());
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn case_search_historical_mode_count_and_empty_incomplete_stay_with_accepted_snapshot() {
    for surface in [Surface::Browse, Surface::Bottom] {
        for empty in [false, true] {
            for incomplete in [false, true] {
                let (mut app, commands) = fixture();
                enter(&mut app, surface);
                assert!(!has_text(&frame(&mut app, vec![]), "0 matching lines"));
                configure(&mut app, &commands, surface);
                let pending = submit(&mut app, &commands, surface);
                let mut accepted = result(request("src", &["*.rs", "*.md"]));
                if empty {
                    accepted.matches.clear();
                }
                accepted.incomplete = incomplete;
                if incomplete {
                    accepted
                        .reasons
                        .push(ScopedSearchIncompleteReason::ReadByteLimit);
                }
                accepted.examined_entries = 37;
                accepted.read_bytes = 8192;
                reply(
                    &mut app,
                    pending.id,
                    Ok(Payload::CaseMatches { result: accepted }),
                );
                settle(&mut app);
                let count = if empty { 0 } else { 3 };
                let files = if empty { 0 } else { 2 };
                let expected = if incomplete {
                    format!("{count} matching lines observed in {files} files · incomplete")
                } else {
                    format!("{count} matching lines in {files} files")
                };
                let snapshot = app.search_state.accepted.clone();
                let output = frame(&mut app, vec![]);
                assert!(has_text(&output, "Case-insensitive"));
                if matches!(surface, Surface::Browse) {
                    assert!(has_text(&output, &expected));
                    if empty {
                        assert_eq!(
                            has_text(&output, "No matching lines in the searched scope."),
                            !incomplete
                        );
                    }
                    let mut fields = vec!["37 entries examined", "8192 bytes read"];
                    if incomplete {
                        fields.push("Incomplete: read byte limit");
                    }
                    browse_assert_status_fields_reachable(&mut app, SIZE, &fields, &[]);
                } else {
                    assert!(has_text(&output, "37 entries examined"));
                    assert!(has_text(&output, "8192 bytes read"));
                    assert_eq!(has_text(&output, "read byte limit"), incomplete);
                }
                click(&mut app, checkbox(surface));
                replace(&mut app, surface.query(), "changed draft");
                replace(&mut app, surface.scope(), "other");
                replace(&mut app, surface.masks(), "*.txt");
                let historical = frame(&mut app, vec![]);
                assert_eq!(app.search_state.accepted, snapshot);
                assert!(has_text(&historical, "Case-insensitive"));
                assert!(has_text(&historical, "Historical results for \"needle\""));
                assert!(!has_text(
                    &historical,
                    "Historical results for \"changed draft\""
                ));
                if matches!(surface, Surface::Browse) {
                    assert!(has_text(&historical, &expected));
                }
                assert_eq!(app.search_truncated, incomplete);
                assert!(commands.try_recv().is_err());
            }
        }
    }
}

#[test]
fn case_search_false_draft_survives_provider_hide_workspace_and_reconnect_downgrade() {
    for surface in [Surface::Browse, Surface::Bottom] {
        for switch_workspace in [false, true] {
            let (mut app, commands) = fixture();
            enter(&mut app, surface);
            accepted(&mut app, &commands, surface);
            if matches!(surface, Surface::Browse) {
                app.show_browse(browse::Provider::Files);
                settle(&mut app);
                app.show_browse(browse::Provider::ProjectText);
                settle(&mut app);
                app.browse_dismiss();
                settle(&mut app);
            } else {
                app.tools_open = false;
                settle(&mut app);
            }
            if matches!(surface, Surface::Browse) {
                app.show_browse(browse::Provider::ProjectText);
                settle(&mut app);
            } else {
                enter(&mut app, surface);
            }
            assert!(!app.search_match_case);
            assert!(commands.try_recv().is_err());
            let old_generation = app.generation;
            let old = submit(&mut app, &commands, surface);
            app.disconnected("case fixture reconnect".into());
            assert!(app.case_search.is_none());
            assert!(!app.search_match_case);
            app.generation += 1;
            let (worker, reconnected) = Worker::recording();
            app.worker = Some(worker);
            app.state = ConnectionState::Connecting;
            let mut form = app.active_form.clone().unwrap();
            let mut handshake = hello(None, Some(ScopedSearchDescriptor::V1));
            if switch_workspace {
                app.documents[0].saved_text = app.documents[0].text.clone();
                form.local_root = "/different".into();
                if let Payload::Hello { root, .. } = &mut handshake {
                    *root = "/different".into();
                }
            }
            app.connecting_form = Some(form);
            reply(&mut app, 0, Ok(handshake));
            assert!(app.ready());
            let list = reconnected.try_recv().unwrap();
            assert!(matches!(list.op, Operation::List { .. }));
            reply(&mut app, list.id, Ok(Payload::Entries { entries: vec![] }));
            app.apply_event(Event {
                generation: old_generation,
                id: old.id,
                connected: true,
                result: Ok(payload(request("src", &["*.rs", "*.md"]))),
            });
            // Retire the old generation's modal context before explicitly reopening.
            settle(&mut app);
            if matches!(surface, Surface::Browse) {
                app.show_browse(browse::Provider::ProjectText);
                settle(&mut app);
            } else {
                enter(&mut app, surface);
            }
            assert!(!app.search_match_case);
            assert_eq!(app.search_query, "needle");
            assert_eq!(app.search_scope, "src");
            assert_eq!(app.search_masks, "*.rs; *.md");
            assert!(!app.search_rows_active());
            assert!(!recorded_response(&app, surface.submit()).enabled());
            assert!(has_text(&frame(&mut app, vec![]), UNAVAILABLE));
            app.search();
            assert!(reconnected.try_recv().is_err());
            click(&mut app, checkbox(surface));
            let fresh = submit(&mut app, &reconnected, surface);
            assert!(matches!(fresh.op, Operation::SearchIn { .. }));
        }
    }
    assert!(
        CedarApp::empty().search_match_case,
        "the UI draft is not persisted across restarts"
    );
}

#[test]
fn case_search_all_local_connection_reset_paths_clear_descriptor_but_keep_checkbox_draft() {
    for reset in [
        "transport",
        "cancel",
        "disconnect",
        "recovery",
        "bad hello",
        "worker closed",
    ] {
        let (mut app, commands) = fixture();
        app.search_match_case = false;
        match reset {
            "transport" => app.disconnected("lost transport".into()),
            "cancel" => {
                app.state = ConnectionState::Connecting;
                app.cancel_connection();
            }
            "disconnect" => app.disconnect_idle(),
            "recovery" => {
                app.state = ConnectionState::ClosingTransport;
                let ctx = app.editor_ctx.clone();
                app.keep_editing_recovery(&ctx);
            }
            "bad hello" => {
                app.state = ConnectionState::Connecting;
                app.connecting_form = app.active_form.clone();
                reply(
                    &mut app,
                    0,
                    Ok(hello(Some(CaseSearchDescriptor { version: 0 }), None)),
                );
            }
            "worker closed" => app.connection_closed(app.generation, Ok(())),
            _ => unreachable!(),
        }
        assert!(app.case_search.is_none(), "{reset} retained descriptor");
        assert!(!app.search_match_case, "{reset} silently reset user draft");
        assert!(!app.ready(), "{reset} retained operation admission");
        assert!(commands.try_recv().is_err());
    }
}

fn edge(key: egui::Key, pressed: bool, repeat: bool) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: Some(key),
        pressed,
        repeat,
        modifiers: egui::Modifiers::NONE,
    }
}

#[test]
fn case_search_native_tab_order_and_space_enter_toggle_without_submitting_or_editing() {
    for surface in [Surface::Browse, Surface::Bottom] {
        for activation in [egui::Key::Space, egui::Key::Enter] {
            let (mut app, commands) = fixture();
            enter(&mut app, surface);
            replace(&mut app, surface.query(), "needle");
            focus(&mut app, surface.query());
            for probe in [
                surface.scope(),
                surface.masks(),
                checkbox(surface),
                surface.submit(),
            ] {
                let expected = recorded_response(&app, probe).id;
                frame(&mut app, key(egui::Key::Tab, egui::Modifiers::NONE));
                assert_eq!(
                    app.editor_ctx.memory(|memory| memory.focused()),
                    Some(expected)
                );
            }
            focus(&mut app, checkbox(surface));
            let text = app.documents[0].text.clone();
            let epoch = app.navigation_epoch;
            frame(&mut app, vec![edge(activation, true, false)]);
            assert!(!app.search_match_case);
            for repeat in [false, true, true] {
                frame(&mut app, vec![edge(activation, true, repeat)]);
                assert!(
                    !app.search_match_case,
                    "held {activation:?} must not toggle again"
                );
            }
            frame(&mut app, vec![edge(activation, false, false)]);
            frame(&mut app, key(activation, egui::Modifiers::NONE));
            assert!(app.search_match_case);
            assert_eq!(app.search_query, "needle");
            assert_eq!(app.documents[0].text, text);
            assert_eq!(app.navigation_epoch, epoch);
            assert!(commands.try_recv().is_err());
            focus(&mut app, surface.query());
            frame(&mut app, key(egui::Key::Enter, egui::Modifiers::NONE));
            assert!(
                matches!(commands.try_recv().unwrap().op, Operation::Search { .. }),
                "query Enter remains explicit submit"
            );
            assert!(commands.try_recv().is_err());
        }
    }
}

fn replay_frame(app: &mut CedarApp, events: Vec<egui::Event>, focused: bool, discard: bool) {
    let ctx = app.editor_ctx.clone();
    ctx.options_mut(|options| options.max_passes = 2.try_into().unwrap());
    let mut input = egui::RawInput {
        time: Some(ctx.input(|input| input.time) + 0.1),
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SIZE)),
        events,
        focused,
        ..Default::default()
    };
    eframe::App::raw_input_hook(app, &ctx, &mut input);
    let mut native = eframe::Frame::_new_kittest();
    let mut passes = 0;
    let _ = ctx.run(input, |ctx| {
        eframe::App::update(app, ctx, &mut native);
        passes += 1;
        if discard && passes == 1 {
            ctx.request_discard("case control original input must have one effect");
        }
    });
    if discard {
        assert_eq!(passes, 2);
    }
}

#[test]
fn case_search_checkbox_discards_and_pointer_releases_have_one_local_effect() {
    for surface in [Surface::Browse, Surface::Bottom] {
        for activation in [None, Some(egui::Key::Enter), Some(egui::Key::Space)] {
            let (mut app, commands) = fixture();
            enter(&mut app, surface);
            replace(&mut app, surface.query(), "needle");
            let events = if let Some(activation) = activation {
                focus(&mut app, checkbox(surface));
                key(activation, egui::Modifiers::NONE)
            } else {
                let pos = recorded_rect(&app, checkbox(surface)).center();
                frame(&mut app, vec![egui::Event::PointerMoved(pos)]);
                frame(&mut app, pointer(pos, true));
                pointer(pos, false)
            };
            replay_frame(&mut app, events, true, true);
            assert!(
                !app.search_match_case,
                "{surface:?}, {activation:?}: discarded render pass must not toggle a second time"
            );
            assert!(commands.try_recv().is_err());
        }
    }
}

#[test]
fn case_search_checkbox_rejects_foreign_modal_focus_loss_ime_and_modified_keys() {
    for surface in [Surface::Browse, Surface::Bottom] {
        for activation in [egui::Key::Space, egui::Key::Enter] {
            for barrier in [
                "modal",
                "focus loss",
                "preedit",
                "commit",
                "modified",
                "repeat",
            ] {
                let (mut app, commands) = fixture();
                enter(&mut app, surface);
                replace(&mut app, surface.query(), "needle");
                focus(&mut app, checkbox(surface));
                let text = app.documents[0].text.clone();
                let mut events = key(activation, egui::Modifiers::NONE);
                match barrier {
                    "modal" => app.confirm = Some(Confirm::CloseTab(1)),
                    "preedit" => {
                        events.insert(
                            0,
                            egui::Event::Ime(egui::ImeEvent::Preedit("compose".into())),
                        );
                    }
                    "commit" => {
                        events.insert(0, egui::Event::Ime(egui::ImeEvent::Commit("x".into())));
                    }
                    "modified" => events = key(activation, egui::Modifiers::SHIFT),
                    "repeat" => events = vec![edge(activation, true, true)],
                    "focus loss" => events.insert(0, egui::Event::WindowFocused(false)),
                    _ => unreachable!(),
                }
                replay_frame(&mut app, events, barrier != "focus loss", false);
                assert!(
                    app.search_match_case,
                    "{surface:?}, {activation:?}, {barrier}"
                );
                assert_eq!(app.search_query, "needle");
                assert_eq!(
                    app.documents[0].text, text,
                    "input must not replay into editor"
                );
                assert!(commands.try_recv().is_err());
            }
        }
    }
}

#[test]
fn case_search_mixed_toggle_submit_batches_never_search() {
    for surface in [Surface::Browse, Surface::Bottom] {
        for pointer_first in [false, true] {
            let (mut app, commands) = fixture();
            enter(&mut app, surface);
            replace(&mut app, surface.query(), "needle");
            let position = recorded_rect(&app, checkbox(surface)).center();
            frame(&mut app, vec![egui::Event::PointerMoved(position)]);
            frame(&mut app, pointer(position, true));
            let mut events = pointer(position, false);
            let enter = key(egui::Key::Enter, egui::Modifiers::NONE);
            if pointer_first {
                events.extend(enter);
            } else {
                events.splice(0..0, enter);
            }
            frame(&mut app, events);
            settle(&mut app);
            assert!(
                commands.try_recv().is_err(),
                "a mixed toggle/submit batch has no search authority"
            );
            assert!(app.search_state.pending.is_none());
        }
    }
}

#[test]
fn case_search_toggle_invalidates_captured_row_activation_and_pointer_press() {
    for surface in [Surface::Browse, Surface::Bottom] {
        for activation in [None, Some(egui::Key::Space), Some(egui::Key::Enter)] {
            let (mut app, commands) = fixture();
            enter(&mut app, surface);
            accepted(&mut app, &commands, surface);
            focus(&mut app, &surface.row(2));
            let epoch = app.navigation_epoch;
            let events = if let Some(activation) = activation {
                key(activation, egui::Modifiers::NONE)
            } else {
                let pos = recorded_rect(&app, &surface.row(2))
                    .intersect(recorded_clip(&app, &surface.row(2)))
                    .center();
                frame(&mut app, vec![egui::Event::PointerMoved(pos)]);
                frame(&mut app, pointer(pos, true));
                pointer(pos, false)
            };
            captured_frame(&mut app, SIZE, events, |app| {
                app.search_match_case = true;
                app.search_filters_changed();
            });
            settle(&mut app);
            assert!(!app.search_rows_active());
            assert_eq!(app.navigation_epoch, epoch);
            assert!(!app.has_search_preview() && !app.browse.preview_visible());
            assert!(commands.try_recv().is_err());
        }
    }
}

#[test]
fn case_search_queued_open_cannot_win_over_same_frame_native_toggle() {
    for surface in [Surface::Browse, Surface::Bottom] {
        let (mut app, commands) = fixture();
        enter(&mut app, surface);
        accepted(&mut app, &commands, surface);
        focus(&mut app, &surface.row(2));
        frame(&mut app, key(egui::Key::Enter, egui::Modifiers::NONE));
        let open = commands
            .try_recv()
            .expect("ordinary unopened text result sends Read");
        assert!(matches!(&open.op, Operation::Read { path } if path == "src/target.rs"));
        // Browse Open hides its dialog; reopening controls does not authorize
        // applying the read after a newer native control gesture.
        enter(&mut app, surface);
        let pos = recorded_rect(&app, checkbox(surface)).center();
        frame(&mut app, vec![egui::Event::PointerMoved(pos)]);
        frame(&mut app, pointer(pos, true));
        app.result_tx
            .send(WorkerEvent::Response(Event {
                generation: app.generation,
                id: open.id,
                connected: true,
                result: Ok(Payload::File {
                    path: "src/target.rs".into(),
                    text: "stale opened needle".into(),
                    revision: "b".repeat(64),
                }),
            }))
            .unwrap();
        frame(&mut app, pointer(pos, false));
        settle(&mut app);
        assert!(app.search_match_case);
        assert_eq!(app.active_document, Some(1));
        assert_eq!(app.documents.len(), 2);
        let cached = app
            .documents
            .iter()
            .find(|document| document.path == "src/target.rs")
            .unwrap();
        assert_eq!(cached.text, "stale opened needle");
        assert!(cached.jump_to.is_none());
        assert_ne!(app.active_document, Some(cached.id));
        assert!(app.location_history.back.is_empty());
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn case_search_checkbox_geometry_is_fully_visible_at_existing_sizes_and_fonts() {
    for surface in [Surface::Browse, Surface::Bottom] {
        for size in [
            egui::vec2(1280.0, 900.0),
            egui::vec2(780.0, 540.0),
            egui::vec2(823.0, 540.0),
            egui::vec2(825.0, 540.0),
        ] {
            for font in [13.0, 18.0] {
                let (mut app, commands) = fixture();
                enter(&mut app, surface);
                accepted(&mut app, &commands, surface);
                app.tools_height = None;
                app.editor_ctx.style_mut(|style| {
                    for kind in [egui::TextStyle::Body, egui::TextStyle::Button] {
                        style.text_styles.insert(kind, FontId::proportional(font));
                    }
                    style
                        .text_styles
                        .insert(egui::TextStyle::Monospace, FontId::monospace(font));
                });
                for _ in 0..4 {
                    frame_at(&mut app, size, vec![]);
                }
                let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
                for probe in [
                    surface.query(),
                    surface.scope(),
                    surface.masks(),
                    checkbox(surface),
                    surface.submit(),
                ] {
                    let rect = recorded_rect(&app, probe);
                    let clip = recorded_clip(&app, probe);
                    assert!(rect.is_finite() && rect.is_positive());
                    assert!(
                        screen.contains_rect(rect) && clip.contains_rect(rect),
                        "{surface:?}, {size:?}, font={font}: {probe} {rect:?} clipped by {clip:?}"
                    );
                }
                let toggle = recorded_rect(&app, checkbox(surface));
                let submit = recorded_rect(&app, surface.submit());
                assert!(
                    !toggle.intersects(submit),
                    "checkbox and submit have distinct targets"
                );
                click_at_size(&mut app, size, checkbox(surface));
                assert!(app.search_match_case);
                assert!(!app.search_rows_active());
                assert!(commands.try_recv().is_err());
            }
        }
    }
}

fn continuity_frame(
    app: &mut CedarApp,
    surface: Surface,
    events: Vec<egui::Event>,
    discard: bool,
) -> Option<(bool, bool)> {
    match surface {
        Surface::Browse => crate::browse_tests::case_row_continuity_frame(app, events, discard),
        Surface::Bottom => {
            crate::search_keyboard_tests::case_row_continuity_frame(app, events, discard)
        }
    }
}

#[test]
fn case_search_accepted_native_double_click_not_triple_opens_once_across_discarded_passes() {
    for surface in [Surface::Browse, Surface::Bottom] {
        let (mut app, commands) = fixture();
        enter(&mut app, surface);
        accepted(&mut app, &commands, surface);
        // The setup submit is a real click. Let its native click lifetime expire.
        for _ in 0..6 {
            frame(&mut app, vec![]);
        }
        for _ in 0..3 {
            let _ = continuity_frame(&mut app, surface, vec![], false);
        }
        let name = surface.row(2);
        let pos = recorded_rect(&app, &name)
            .intersect(recorded_clip(&app, &name))
            .center();
        let _ = continuity_frame(
            &mut app,
            surface,
            vec![egui::Event::PointerMoved(pos)],
            false,
        );
        let _ = continuity_frame(&mut app, surface, pointer(pos, true), false);
        assert_eq!(
            continuity_frame(&mut app, surface, pointer(pos, false), true),
            Some((false, false)),
            "{surface:?}: first click must be single"
        );
        assert!(commands.try_recv().is_err());
        let _ = continuity_frame(&mut app, surface, pointer(pos, true), false);
        assert_eq!(
            continuity_frame(&mut app, surface, pointer(pos, false), true),
            Some((true, false)),
            "{surface:?}: actual double, never a triple substitute"
        );
        let open = commands.try_recv().unwrap();
        assert!(matches!(&open.op, Operation::Read { path } if path == "src/target.rs"));
        assert!(matches!(
            app.jobs.get(&open.id),
            Some(Job::SearchOpen { .. })
        ));
        assert!(commands.try_recv().is_err());
        reply(
            &mut app,
            open.id,
            Ok(Payload::File {
                path: "src/target.rs".into(),
                text: "first\nneedle\n".into(),
                revision: "b".repeat(64),
            }),
        );
        settle(&mut app);
        assert_ne!(app.active_document, Some(1));
        assert_eq!(app.location_history.back.len(), 1);
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn case_search_mixed_toggle_typed_submit_permutations_have_no_search_authority() {
    for surface in [Surface::Browse, Surface::Bottom] {
        for release_first in [false, true] {
            for editing in [
                egui::Event::Text("x".into()),
                egui::Event::Paste("x".into()),
                egui::Event::Ime(egui::ImeEvent::Commit("x".into())),
            ] {
                let (mut app, commands) = fixture();
                enter(&mut app, surface);
                replace(&mut app, surface.query(), "needle");
                let pos = recorded_rect(&app, checkbox(surface)).center();
                frame(&mut app, vec![egui::Event::PointerMoved(pos)]);
                frame(&mut app, pointer(pos, true));
                let release = pointer(pos, false);
                let mut edit_and_submit = vec![editing.clone()];
                edit_and_submit.extend(key(egui::Key::Enter, egui::Modifiers::NONE));
                let events = if release_first {
                    [release, edit_and_submit].concat()
                } else {
                    [edit_and_submit, release].concat()
                };
                frame(&mut app, events);
                settle(&mut app);
                assert!(
                    app.search_state.pending.is_none(),
                    "{surface:?}, release_first={release_first}, {editing:?}"
                );
                assert!(commands.try_recv().is_err());
            }
        }
    }
}

#[test]
fn case_search_typing_and_enter_preserves_each_surfaces_checked_baseline_policy() {
    for surface in [Surface::Browse, Surface::Bottom] {
        for checked in [true, false] {
            let (mut app, commands) = fixture();
            enter(&mut app, surface);
            if !checked {
                click(&mut app, checkbox(surface));
            }
            focus(&mut app, surface.query());
            let mut events = vec![egui::Event::Text("needle".into())];
            events.extend(key(egui::Key::Enter, egui::Modifiers::NONE));
            frame(&mut app, events);
            assert_eq!(app.search_query, "needle");
            if matches!(surface, Surface::Browse) {
                // Browse's established clean_press gate rejects mixed text+Enter.
                assert!(commands.try_recv().is_err(), "checked={checked}");
                frame(&mut app, key(egui::Key::Enter, egui::Modifiers::NONE));
            }
            let command = commands
                .try_recv()
                .expect("Bottom mixed Enter or Browse fresh Enter submits");
            if checked {
                assert!(
                    matches!(&command.op, Operation::Search { query, limit: 500 } if query == "needle")
                );
            } else {
                assert!(
                    matches!(&command.op, Operation::SearchCase { request: actual } if *actual == request("", &[]))
                );
            }
            assert!(commands.try_recv().is_err());
        }
    }
}

#[derive(Debug, PartialEq)]
struct EditorSnapshot {
    text: String,
    saved: String,
    version: u64,
    selection: crate::scoped_search_tests::SelectionKey,
    history: crate::scoped_search_tests::HistorySnapshot,
}
fn editor_snapshot(app: &CedarApp) -> EditorSnapshot {
    EditorSnapshot {
        text: app.documents[0].text.clone(),
        saved: app.documents[0].saved_text.clone(),
        version: app.documents[0].edit_version,
        selection: crate::scoped_search_tests::selection(app),
        history: crate::scoped_search_tests::history(app),
    }
}

#[test]
fn case_search_rejected_checkbox_key_lifetime_cannot_replay_after_editor_focus_transfer() {
    for surface in [Surface::Browse, Surface::Bottom] {
        for activation in [egui::Key::Space, egui::Key::Enter] {
            for key_first in [false, true] {
                let (mut app, commands) = fixture();
                // Bottom uses an actual native pointer transfer. Browse is modal:
                // its separate branch deliberately tests view-loss then focus transfer.
                if matches!(surface, Surface::Bottom) {
                    enter(&mut app, surface);
                }
                let rect = recorded_rect(&app, "editor").intersect(recorded_clip(&app, "editor"));
                let pos = egui::pos2(rect.left() + 24.0, rect.top() + 8.0);
                // Seed the exact caret produced by this pointer before checkbox focus.
                frame(&mut app, vec![egui::Event::PointerMoved(pos)]);
                frame(&mut app, pointer(pos, true));
                frame(&mut app, pointer(pos, false));
                for _ in 0..6 {
                    frame(&mut app, vec![]);
                }
                if matches!(surface, Surface::Browse) {
                    enter(&mut app, surface);
                }
                focus(&mut app, checkbox(surface));
                let before = editor_snapshot(&app);
                let mut press = vec![edge(activation, true, false)];
                if activation == egui::Key::Space {
                    press.push(egui::Event::Text(" ".into()));
                }
                let pointing = pointer(pos, true);
                let events = if key_first {
                    [press, pointing].concat()
                } else {
                    [pointing, press].concat()
                };
                if matches!(surface, Surface::Browse) {
                    captured_frame(&mut app, SIZE, events, |app| {
                        app.browse_dismiss();
                        recorded_response(app, "editor").request_focus();
                    });
                } else {
                    frame(&mut app, events);
                }
                assert_eq!(
                    editor_snapshot(&app),
                    before,
                    "{surface:?}, {activation:?}, key_first={key_first}: mixed batch"
                );
                frame(&mut app, pointer(pos, false));
                assert_eq!(
                    editor_snapshot(&app),
                    before,
                    "{surface:?}, {activation:?}: pointer release"
                );
                if matches!(surface, Surface::Browse) {
                    // The old modal hit-layer can swallow the pointer; no claim
                    // is made that a background pointer penetrates a modal.
                    recorded_response(&app, "editor").request_focus();
                }
                frame(&mut app, vec![]);
                assert_eq!(
                    app.editor_ctx.memory(|memory| memory.focused()),
                    Some(recorded_response(&app, "editor").id),
                    "{surface:?}: actual editor focus before repeats"
                );
                for repeat in [true, false, true] {
                    let mut events = vec![edge(activation, true, repeat)];
                    if activation == egui::Key::Space {
                        events.push(egui::Event::Text(" ".into()));
                    }
                    frame(&mut app, events);
                    assert_eq!(
                        editor_snapshot(&app),
                        before,
                        "{surface:?}, {activation:?}, repeat={repeat}"
                    );
                }
                frame(&mut app, vec![edge(activation, false, false)]);
                assert_eq!(editor_snapshot(&app), before);
                let mut fresh = vec![edge(activation, true, false)];
                if activation == egui::Key::Space {
                    fresh.push(egui::Event::Text(" ".into()));
                }
                fresh.push(edge(activation, false, false));
                frame(&mut app, fresh);
                assert_ne!(
                    app.documents[0].text, before.text,
                    "fresh editor key must recover after physical release"
                );
                assert_eq!(app.documents[0].edit_version, before.version + 1);
                assert_eq!(app.documents[0].saved_text, before.saved);
                assert!(commands.try_recv().is_err());
            }
        }
    }
}
