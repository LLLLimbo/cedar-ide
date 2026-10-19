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
            app.pending.get(&open.id),
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
            for ordering in ["key first", "pointer first", "pointer between key and text"] {
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
                let events = match ordering {
                    "key first" => [press, pointing].concat(),
                    "pointer first" => [pointing, press].concat(),
                    _ => {
                        let mut events = vec![edge(activation, true, false)];
                        events.extend(pointing);
                        if activation == egui::Key::Space {
                            events.push(egui::Event::Text(" ".into()));
                        }
                        events
                    }
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
                    "{surface:?}, {activation:?}, ordering={ordering}: mixed batch"
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

#[test]
fn case_search_first_ever_pointer_checkbox_press_is_native_and_tooltip_cannot_steal_it() {
    for surface in [Surface::Browse, Surface::Bottom] {
        for separate_hover in [false, true] {
            let (mut app, commands) = fixture();
            enter(&mut app, surface);
            replace(&mut app, surface.query(), "needle");
            assert!(
                app.editor_ctx
                    .input(|input| input.pointer.latest_pos().is_none()),
                "fixture must not prewarm pointer state"
            );
            let pos = recorded_rect(&app, checkbox(surface)).center();
            if separate_hover {
                frame(&mut app, vec![egui::Event::PointerMoved(pos)]);
            }
            frame(&mut app, pointer(pos, true));
            assert!(
                recorded_response(&app, checkbox(surface)).is_pointer_button_down_on(),
                "{surface:?}, separate_hover={separate_hover}: native press"
            );
            assert!(app.search_match_case);
            frame(&mut app, pointer(pos, false));
            assert!(
                recorded_response(&app, checkbox(surface)).clicked(),
                "{surface:?}: native release"
            );
            assert!(!app.search_match_case);
            assert!(commands.try_recv().is_err());
        }
    }
}

#[test]
fn case_search_case_only_peer_files_provider_exposes_enabled_text_entry() {
    let (mut app, commands) = fixture();
    app.scoped_search = None;
    app.agent_info
        .as_mut()
        .unwrap()
        .capabilities
        .retain(|name| name != "search");
    assert!(!app.backend_supports("search"));
    assert!(app.backend_case_search_supported());
    app.show_browse(browse::Provider::Files);
    settle(&mut app);
    assert!(recorded_response(&app, "browse_text").enabled());
    click(&mut app, "browse_text");
    settle(&mut app);
    assert!(recorded_response(&app, "browse_match_case").enabled());
    configure(&mut app, &commands, Surface::Browse);
    let command = submit(&mut app, &commands, Surface::Browse);
    assert!(
        matches!(&command.op, Operation::SearchCase { request: actual } if *actual == request("src", &["*.rs", "*.md"]))
    );
    assert!(commands.try_recv().is_err());
}

#[test]
fn case_search_owned_space_filters_only_ordered_single_companion_and_respects_release() {
    for surface in [Surface::Browse, Surface::Bottom] {
        for scenario in [
            "adjacent",
            "motion",
            "pointer",
            "reversed",
            "unrelated",
            "ime",
            "release/repress",
        ] {
            let (mut app, commands) = fixture();
            enter(&mut app, surface);
            focus(&mut app, checkbox(surface));
            // Rejected mixed key is nevertheless owned until its physical release.
            frame(
                &mut app,
                vec![edge(egui::Key::Space, true, false), egui::Event::Copy],
            );
            assert!(app.search_match_case);
            if matches!(surface, Surface::Browse) {
                app.browse_dismiss();
                settle(&mut app);
            }
            focus(&mut app, "editor");
            let space = egui::Event::Text(" ".into());
            let repeat = edge(egui::Key::Space, true, true);
            let (events, expected) = match scenario {
                "adjacent" => (
                    vec![repeat, space.clone(), space.clone()],
                    vec![space.clone()],
                ),
                "motion" => {
                    let moved = egui::Event::PointerMoved(egui::pos2(3.0, 4.0));
                    let relative = egui::Event::MouseMoved(egui::vec2(1.0, 2.0));
                    let text = egui::Event::Text("tail".into());
                    (
                        vec![
                            repeat,
                            moved.clone(),
                            relative.clone(),
                            space.clone(),
                            text.clone(),
                        ],
                        vec![moved, relative, text],
                    )
                }
                "pointer" => {
                    let pointing = pointer(egui::pos2(3.0, 4.0), true);
                    let mut events = vec![repeat];
                    events.extend(pointing.clone());
                    events.push(space.clone());
                    (events, pointing)
                }
                "reversed" => (
                    vec![space.clone(), edge(egui::Key::Space, true, false)],
                    vec![space.clone()],
                ),
                "unrelated" => {
                    let text = egui::Event::Text("unrelated".into());
                    (
                        vec![repeat, text.clone(), space.clone()],
                        vec![text, space.clone()],
                    )
                }
                "ime" => {
                    let ime = vec![
                        egui::Event::Ime(egui::ImeEvent::Enabled),
                        egui::Event::Ime(egui::ImeEvent::Preedit("draft".into())),
                        space.clone(),
                        egui::Event::Ime(egui::ImeEvent::Commit("語".into())),
                        egui::Event::Ime(egui::ImeEvent::Disabled),
                    ];
                    let mut events = vec![repeat];
                    events.extend(ime.clone());
                    (events, ime)
                }
                "release/repress" => {
                    let events = vec![
                        edge(egui::Key::Space, false, false),
                        edge(egui::Key::Space, true, false),
                        space.clone(),
                        edge(egui::Key::Space, false, false),
                    ];
                    let expected = if matches!(surface, Surface::Browse) {
                        events[1..].to_vec()
                    } else {
                        events.clone()
                    };
                    (events, expected)
                }
                _ => unreachable!(),
            };
            let ctx = app.editor_ctx.clone();
            let mut raw = egui::RawInput {
                time: Some(ctx.input(|input| input.time) + 0.1),
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SIZE)),
                events,
                focused: true,
                ..Default::default()
            };
            eframe::App::raw_input_hook(&mut app, &ctx, &mut raw);
            assert_eq!(
                raw.events, expected,
                "{surface:?}, {scenario}: preserve native ordering and unrelated input"
            );
            assert!(commands.try_recv().is_err());
        }
    }
}

#[test]
fn case_search_rejected_checkbox_key_and_native_submit_click_never_search() {
    for surface in [Surface::Browse, Surface::Bottom] {
        for activation in [egui::Key::Enter, egui::Key::Space] {
            for key_first in [false, true] {
                let (mut app, commands) = fixture();
                enter(&mut app, surface);
                replace(&mut app, surface.query(), "needle");
                let pos = recorded_rect(&app, surface.submit()).center();
                frame(&mut app, vec![egui::Event::PointerMoved(pos)]);
                frame(&mut app, pointer(pos, true));
                assert!(recorded_response(&app, surface.submit()).is_pointer_button_down_on());
                // The raw release batch begins with the checkbox actually focused.
                focus(&mut app, checkbox(surface));
                let release = pointer(pos, false);
                let mut activation_events = vec![edge(activation, true, false)];
                if activation == egui::Key::Space {
                    activation_events.push(egui::Event::Text(" ".into()));
                }
                activation_events.push(edge(activation, false, false));
                let events = if key_first {
                    [activation_events, release].concat()
                } else {
                    [release, activation_events].concat()
                };
                frame(&mut app, events);
                assert!(
                    recorded_response(&app, surface.submit()).clicked(),
                    "{surface:?}, {activation:?}: real native submit click"
                );
                assert!(app.search_match_case, "mixed checkbox key was rejected");
                assert!(app.search_state.pending.is_none());
                assert!(commands.try_recv().is_err(), "{surface:?}, {activation:?}, key_first={key_first}: rejected checkbox gesture cannot submit");
            }
        }
    }
}

#[test]
fn case_search_ime_enabled_without_preedit_blocks_checkbox_until_disabled_and_fresh_gesture() {
    for surface in [Surface::Browse, Surface::Bottom] {
        for activation in [egui::Key::Enter, egui::Key::Space] {
            let (mut app, commands) = fixture();
            enter(&mut app, surface);
            replace(&mut app, surface.query(), "needle");
            focus(&mut app, checkbox(surface));
            frame(&mut app, vec![egui::Event::Ime(egui::ImeEvent::Enabled)]);
            frame(&mut app, key(activation, egui::Modifiers::NONE));
            assert!(
                app.search_match_case,
                "{surface:?}, {activation:?}: Enabled is composing before first Preedit"
            );
            assert_eq!(app.search_query, "needle");
            assert!(app.search_state.pending.is_none());
            assert!(commands.try_recv().is_err());
            frame(&mut app, vec![egui::Event::Ime(egui::ImeEvent::Disabled)]);
            frame(&mut app, key(activation, egui::Modifiers::NONE));
            assert!(
                !app.search_match_case,
                "{surface:?}, {activation:?}: fresh gesture recovers after Disabled"
            );
            assert!(commands.try_recv().is_err());
        }
    }
}

#[test]
fn case_search_exhausted_draft_revision_rejects_fresh_and_captured_checkbox_gestures() {
    for surface in [Surface::Browse, Surface::Bottom] {
        for activation in [None, Some(egui::Key::Enter), Some(egui::Key::Space)] {
            for captured_before_exhaustion in [false, true] {
                let (mut app, commands) = fixture();
                enter(&mut app, surface);
                replace(&mut app, surface.query(), "needle");
                if !captured_before_exhaustion {
                    app.search_state.exhaust_draft_revision();
                    settle(&mut app);
                }
                let events = if let Some(activation) = activation {
                    focus(&mut app, checkbox(surface));
                    key(activation, egui::Modifiers::NONE)
                } else {
                    let pos = recorded_rect(&app, checkbox(surface)).center();
                    frame(&mut app, vec![egui::Event::PointerMoved(pos)]);
                    frame(&mut app, pointer(pos, true));
                    assert!(recorded_response(&app, checkbox(surface)).is_pointer_button_down_on());
                    pointer(pos, false)
                };
                if captured_before_exhaustion {
                    captured_frame(&mut app, SIZE, events, |app| {
                        app.search_state.exhaust_draft_revision();
                        app.search_match_case = false;
                        app.search_filters_changed();
                        app.search_match_case = true;
                        app.search_filters_changed();
                    });
                } else {
                    frame(&mut app, events);
                }
                assert!(app.search_match_case, "{surface:?}, {activation:?}, captured={captured_before_exhaustion}: exhausted identity cannot authorize toggle");
                assert!(app.search_state.pending.is_none());
                assert!(commands.try_recv().is_err());
            }
        }
    }
}

fn draft_away_and_back(app: &mut CedarApp, field: &str) {
    match field {
        "query" => {
            let original = app.search_query.clone();
            app.search_query.push('x');
            app.search_filters_changed();
            app.search_query = original;
        }
        "scope" => {
            let original = app.search_scope.clone();
            app.search_scope = "other".into();
            app.search_filters_changed();
            app.search_scope = original;
        }
        "masks" => {
            let original = app.search_masks.clone();
            app.search_masks = "*.txt".into();
            app.search_filters_changed();
            app.search_masks = original;
        }
        "boolean" => {
            app.search_match_case = !app.search_match_case;
            app.search_filters_changed();
            app.search_match_case = !app.search_match_case;
        }
        _ => unreachable!(),
    }
    app.search_filters_changed();
}

#[test]
fn case_search_captured_checkbox_identity_cannot_revive_after_draft_away_and_back() {
    for surface in [Surface::Browse, Surface::Bottom] {
        for field in ["query", "scope", "masks", "boolean"] {
            for gesture in [
                "captured Enter",
                "captured Space",
                "captured release",
                "cross-frame press",
            ] {
                let (mut app, commands) = fixture();
                enter(&mut app, surface);
                accepted(&mut app, &commands, surface);
                let source = app.search_state.accepted.clone();
                let before = app.search_match_case;
                let events = match gesture {
                    "captured Enter" | "captured Space" => {
                        focus(&mut app, checkbox(surface));
                        key(
                            if gesture == "captured Enter" {
                                egui::Key::Enter
                            } else {
                                egui::Key::Space
                            },
                            egui::Modifiers::NONE,
                        )
                    }
                    _ => {
                        let pos = recorded_rect(&app, checkbox(surface)).center();
                        frame(&mut app, vec![egui::Event::PointerMoved(pos)]);
                        frame(&mut app, pointer(pos, true));
                        assert!(
                            recorded_response(&app, checkbox(surface)).is_pointer_button_down_on()
                        );
                        pointer(pos, false)
                    }
                };
                if gesture == "cross-frame press" {
                    draft_away_and_back(&mut app, field);
                    settle(&mut app);
                    frame(&mut app, events);
                } else {
                    captured_frame(&mut app, SIZE, events, |app| {
                        draft_away_and_back(app, field)
                    });
                }
                assert_eq!(
                    app.search_match_case, before,
                    "{surface:?}, {field}, {gesture}: stale gesture cannot toggle"
                );
                assert_eq!(app.search_query, "needle");
                assert_eq!(app.search_scope, "src");
                assert_eq!(app.search_masks, "*.rs; *.md");
                assert_eq!(app.search_state.accepted, source);
                assert!(!app.search_rows_active());
                assert!(app.search_state.pending.is_none());
                assert!(commands.try_recv().is_err());
            }
        }
    }
}

#[test]
fn case_search_ime_enabled_retires_checkbox_pointer_press_without_laundering_release() {
    for surface in [Surface::Browse, Surface::Bottom] {
        for enabled_before_press in [false, true] {
            let (mut app, commands) = fixture();
            enter(&mut app, surface);
            let pos = recorded_rect(&app, checkbox(surface)).center();
            frame(&mut app, vec![egui::Event::PointerMoved(pos)]);
            if enabled_before_press {
                frame(&mut app, vec![egui::Event::Ime(egui::ImeEvent::Enabled)]);
            }
            frame(&mut app, pointer(pos, true));
            if !enabled_before_press {
                frame(&mut app, vec![egui::Event::Ime(egui::ImeEvent::Enabled)]);
            }
            frame(&mut app, vec![egui::Event::Ime(egui::ImeEvent::Disabled)]);
            frame(&mut app, pointer(pos, false));
            assert!(
                app.search_match_case,
                "{surface:?}: ending composition cannot authorize an older press"
            );
            click(&mut app, checkbox(surface));
            assert!(
                !app.search_match_case,
                "a fresh native pointer gesture recovers"
            );
            assert!(commands.try_recv().is_err());
        }
    }
}

// Regression for U+212A: retaining a character in LayoutJob::text alone does
// not prove that the painted glyph is anything other than the missing box.
const KELVIN_SAVED: &str = "let kelvin = \"K K k\";\r\n// K saved\r\n";
const KELVIN_DRAFT: &str = "let kelvin = \"K K k\";\r\n// K draft\r\n";
const KELVIN_SNIPPET: &str = "let kelvin = \"K K k\";";

fn kelvin_metrics() -> [(egui::Vec2, f32, f32); 3] {
    [
        (egui::vec2(1100.0, 800.0), 14.0, 1.0),
        (egui::vec2(900.0, 700.0), 18.0, 1.25),
        (egui::vec2(1280.0, 900.0), 20.0, 2.0),
    ]
}

fn kelvin_fixture(path: &str) -> (CedarApp, Receiver<Command>) {
    let (mut app, commands) = fixture();
    app.documents[0] = Document::new(1, path.into(), KELVIN_SAVED.into(), "c".repeat(64));
    editor_state::commit(
        &app.editor_ctx,
        &mut app.documents[0],
        KELVIN_DRAFT.into(),
        3,
    );
    settle(&mut app);
    assert_eq!(app.documents[0].text.as_bytes(), KELVIN_DRAFT.as_bytes());
    assert!(commands.try_recv().is_err());
    (app, commands)
}

fn kelvin_frame(
    app: &mut CedarApp,
    size: egui::Vec2,
    font_size: f32,
    pixels_per_point: f32,
) -> egui::FullOutput {
    app.font_size = font_size;
    app.editor_ctx.set_pixels_per_point(pixels_per_point);
    app.editor_ctx.style_mut(|style| {
        for kind in [egui::TextStyle::Body, egui::TextStyle::Button] {
            style
                .text_styles
                .insert(kind, FontId::proportional(font_size));
        }
        style
            .text_styles
            .insert(egui::TextStyle::Monospace, FontId::monospace(font_size));
    });
    for _ in 0..3 {
        frame_at(app, size, vec![]);
    }
    let output = frame_at(app, size, vec![]);
    assert_eq!(output.pixels_per_point, pixels_per_point);
    output
}

fn kelvin_assert_painted<'a>(
    app: &CedarApp,
    output: &'a egui::FullOutput,
    expected: &str,
    viewport: egui::Rect,
    expected_font: &FontId,
    label: &str,
) -> &'a egui::Galley {
    kelvin_assert_painted_in_clip(app, output, expected, viewport, expected_font, label, None)
}

fn kelvin_assert_painted_in_clip<'a>(
    app: &CedarApp,
    output: &'a egui::FullOutput,
    expected: &str,
    viewport: egui::Rect,
    expected_font: &FontId,
    label: &str,
    expected_clip: Option<egui::Rect>,
) -> &'a egui::Galley {
    fn collect<'a>(
        shape: &'a egui::epaint::Shape,
        clip: egui::Rect,
        expected: &str,
        found: &mut Vec<(&'a egui::epaint::TextShape, egui::Rect)>,
    ) {
        match shape {
            egui::epaint::Shape::Text(draw) if draw.galley.text() == expected => {
                found.push((draw, clip));
            }
            egui::epaint::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect(shape, clip, expected, found);
                }
            }
            _ => {}
        }
    }
    let mut found = Vec::new();
    for shape in &output.shapes {
        collect(&shape.shape, shape.clip_rect, expected, &mut found);
    }
    let visible = found
        .into_iter()
        .filter(|(draw, clip)| {
            let bounds = egui::Rect::from_min_size(draw.pos, draw.galley.size());
            expected_clip.is_none_or(|expected| expected == *clip)
                && viewport.intersect(*clip).intersects(bounds)
        })
        .collect::<Vec<_>>();
    if expected_clip.is_some() {
        assert_eq!(
            visible.len(),
            1,
            "{label}: one exact code Galley in the actual preview clip"
        );
    }
    let (draw, clip) = visible
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("{label}: no actual visible text shape for {expected:?}"));
    assert_eq!(
        draw.galley.text().as_bytes(),
        expected.as_bytes(),
        "{label}"
    );
    let mut count = 0;
    for row in &draw.galley.rows {
        for glyph in row.glyphs.iter().filter(|glyph| glyph.chr == 'K') {
            count += 1;
            let section = &draw.galley.job.sections[glyph.section_index as usize];
            assert!(draw.galley.text()[section.byte_range.clone()].contains('K'));
            let font = &section.format.font_id;
            assert_eq!(font, expected_font, "{label}: actual section font");
            let uv = glyph.uv_rect;
            assert!(
                uv.offset.is_finite()
                    && uv.size.is_finite()
                    && uv.size.x > 0.0
                    && uv.size.y > 0.0
                    && uv.min[0] < uv.max[0]
                    && uv.min[1] < uv.max[1],
                "{label}: Kelvin needs finite, nonempty painted ink: {uv:?}"
            );
            let ink =
                egui::Rect::from_min_size(draw.pos + glyph.pos.to_vec2() + uv.offset, uv.size);
            assert!(
                ink.is_finite() && clip.contains_rect(ink) && viewport.contains_rect(ink),
                "{label}: Kelvin ink must actually be visible: {ink:?}, clip={clip:?}, viewport={viewport:?}"
            );
            app.editor_ctx.fonts(|fonts| {
                assert!(
                    fonts.has_glyph(font, 'K'),
                    "{label}: actual section font lacks U+212A"
                );
                assert!(!fonts.has_glyph(font, '\u{10ffff}'));
                let missing =
                    fonts.layout_no_wrap("\u{10ffff}".into(), font.clone(), Color32::WHITE);
                let replacement = missing.rows[0].glyphs[0].uv_rect;
                assert_ne!(
                    (uv.min, uv.max),
                    (replacement.min, replacement.max),
                    "{label}: actual U+212A uses replacement atlas UV"
                );
                let supported = fonts.layout_no_wrap("K".into(), font.clone(), Color32::WHITE);
                assert_eq!(
                    uv, supported.rows[0].glyphs[0].uv_rect,
                    "{label}: actual glyph UV"
                );
            });
            // These atlas coordinates also occur in the actual row mesh that
            // App::update painted; this is not a separately constructed job.
            for corner in [uv.min, uv.max] {
                assert!(
                    row.visuals.mesh.vertices.iter().any(|vertex| {
                        vertex.uv == egui::pos2(f32::from(corner[0]), f32::from(corner[1]))
                    }),
                    "{label}: Kelvin atlas corner absent from painted mesh"
                );
            }
        }
    }
    assert_eq!(
        count,
        expected.matches('K').count(),
        "{label}: original Kelvin glyph count"
    );
    assert!(count > 0, "{label}: expected Kelvin witness");
    &draw.galley
}

#[test]
fn case_search_kelvin_glyph_rows_and_shared_preview_survive_native_frames() {
    for surface in [Surface::Browse, Surface::Bottom] {
        for (size, font_size, pixels_per_point) in kelvin_metrics() {
            let (mut app, commands) = kelvin_fixture("src/kelvin.rs");
            let before = editor_snapshot(&app);
            enter(&mut app, surface);
            let navigation = app.navigation_epoch;
            replace(&mut app, surface.query(), "K");
            click(&mut app, checkbox(surface));
            let pending = submit(&mut app, &commands, surface);
            let request = CaseSearchRequest {
                query: "K".into(),
                scope: String::new(),
                masks: vec![],
                limit: 500,
                case_sensitive: false,
            };
            assert!(
                matches!(&pending.op, Operation::SearchCase { request: actual } if *actual == request)
            );
            reply(
                &mut app,
                pending.id,
                Ok(Payload::CaseMatches {
                    result: CaseSearchResult {
                        request,
                        matches: vec![CaseSearchMatch {
                            path: "src/kelvin.rs".into(),
                            line: 1,
                            text: KELVIN_SNIPPET.into(),
                        }],
                        incomplete: false,
                        reasons: vec![],
                        examined_entries: 1,
                        read_bytes: KELVIN_SAVED.len(),
                    },
                }),
            );
            let output = kelvin_frame(&mut app, size, font_size, pixels_per_point);
            assert!(app.search_rows_active());
            assert_eq!(app.search_query.as_bytes(), "K".as_bytes());
            assert_eq!(
                triples(&app),
                vec![("src/kelvin.rs".into(), 1, KELVIN_SNIPPET.into())]
            );
            let label =
                format!("{surface:?}, {size:?}, font={font_size}, scale={pixels_per_point}");
            let row = recorded_rect(&app, &surface.row(0))
                .intersect(recorded_clip(&app, &surface.row(0)))
                .intersect(egui::Rect::from_min_size(egui::Pos2::ZERO, size));
            let row_text = KELVIN_SNIPPET.to_owned();
            let row_font = FontId::monospace(font_size);
            kelvin_assert_painted(&app, &output, &row_text, row, &row_font, &label);

            recorded_response(&app, &surface.row(0)).request_focus();
            frame_at(&mut app, size, vec![]);
            frame_at(&mut app, size, key(egui::Key::Space, egui::Modifiers::NONE));
            let output = kelvin_frame(&mut app, size, font_size, pixels_per_point);
            assert_eq!(
                match surface {
                    Surface::Browse => app.browse_preview_text(),
                    Surface::Bottom => app.search_preview_text(),
                }
                .unwrap()
                .as_bytes(),
                KELVIN_DRAFT.as_bytes(),
                "{label}: preview must retain exact current draft bytes"
            );
            assert_eq!(
                app.project_text_preview_context().unwrap().text,
                "▶ 1 | let kelvin = \"K K k\";\n  2 | // K draft\n  3 | "
            );
            let (_, viewport): (egui::Id, egui::Rect) = app
                .editor_ctx
                .data(|data| {
                    data.get_temp(egui::Id::new((
                        "project_text_context_scroll",
                        matches!(surface, Surface::Bottom),
                    )))
                })
                .expect("actual shared preview viewport");
            kelvin_assert_painted_in_clip(
                &app,
                &output,
                KELVIN_SNIPPET,
                viewport,
                &FontId::monospace(font_size),
                &label,
                Some(recorded_clip(&app, "project_text_context_target")),
            );
            assert_eq!(
                triples(&app),
                vec![("src/kelvin.rs".into(), 1, KELVIN_SNIPPET.into())]
            );
            assert_eq!(
                editor_snapshot(&app),
                before,
                "{label}: read-only surfaces changed editor"
            );
            assert_eq!(app.navigation_epoch, navigation);
            assert_eq!(app.active_document, Some(1));
            assert_eq!(app.documents.len(), 1);
            assert_eq!(
                app.system_fonts.probe_state_for_test(),
                "unchecked",
                "{label}: Kelvin must not trigger an OS CJK probe"
            );
            assert!(
                commands.try_recv().is_err(),
                "{label}: extra worker operation"
            );
        }
    }
}

#[test]
fn case_search_kelvin_glyph_plain_and_highlighted_editors_survive_native_frames() {
    for path in ["src/kelvin.txt", "src/kelvin.rs"] {
        for (size, font_size, pixels_per_point) in kelvin_metrics() {
            let (mut app, commands) = kelvin_fixture(path);
            let before = editor_snapshot(&app);
            let navigation = app.navigation_epoch;
            let output = kelvin_frame(&mut app, size, font_size, pixels_per_point);
            let viewport = recorded_rect(&app, "editor")
                .intersect(recorded_clip(&app, "editor"))
                .intersect(egui::Rect::from_min_size(egui::Pos2::ZERO, size));
            let label = format!("{path}, {size:?}, font={font_size}, scale={pixels_per_point}");
            let galley = kelvin_assert_painted(
                &app,
                &output,
                KELVIN_DRAFT,
                viewport,
                &FontId::monospace(font_size),
                &label,
            );
            if path.ends_with(".txt") {
                assert_eq!(galley.job.sections.len(), 1, "actual plain editor path");
            } else {
                assert!(
                    galley.job.sections.len() > 1,
                    "actual highlighted editor path"
                );
                let colors: std::collections::HashSet<_> = galley
                    .job
                    .sections
                    .iter()
                    .map(|section| section.format.color)
                    .collect();
                assert!(
                    colors.len() > 1,
                    "highlighted editor must paint distinct lexical colors"
                );
            }
            assert!(galley.job.sections.iter().all(|section| {
                section.format.line_height == Some(syntax::editor_line_height(font_size))
            }));
            assert_eq!(
                editor_snapshot(&app),
                before,
                "{label}: rendering changed exact source or history"
            );
            assert_eq!(
                app.documents[0].saved_text.as_bytes(),
                KELVIN_SAVED.as_bytes()
            );
            assert_eq!(app.documents[0].path, path);
            assert_eq!(app.navigation_epoch, navigation);
            assert_eq!(app.active_document, Some(1));
            assert_eq!(app.documents.len(), 1);
            assert_eq!(
                app.system_fonts.probe_state_for_test(),
                "unchecked",
                "{label}: Kelvin must not trigger an OS CJK probe"
            );
            assert!(
                commands.try_recv().is_err(),
                "{label}: rendering issued worker operation"
            );
        }
    }
}
