//! Parser, query lifetime, navigation and actual egui-frame regressions.
use super::*;
use crate::{
    worker::{Command, Event, Worker},
    Payload,
};
use serde_json::json;
use std::sync::mpsc::Receiver;

const URI: &str = "file:///workspace/Target.java";
const SOURCE: &str = "class Target {}\n";

fn symbol(name: &str, uri: &str) -> Value {
    json!({"name":name,"kind":5,"containerName":"demo","location":{"uri":uri,
        "range":{"start":{"line":0,"character":6},"end":{"line":0,"character":12}}}})
}
fn symbols() -> Value {
    json!([symbol("Target", URI)])
}

fn type_app() -> (CedarApp, Receiver<Command>) {
    let mut app = CedarApp::empty();
    app.state = crate::ConnectionState::Ready;
    app.agent_info = Some(crate::agent_support::full_test_agent());
    app.active_form = Some(crate::ConnectForm {
        allow_run: true,
        ..Default::default()
    });
    app.language.mode = ServerMode::Java;
    app.language.running = true;
    app.language.session = 3;
    app.language.automatic = false;
    app.language.capabilities = json!({"workspaceSymbolProvider":true});
    app.language.types.query = "Target".into();
    app.language.view = View::JavaTypes;
    app.tools_open = true;
    app.tool = crate::Tool::Language;
    app.open_form = false;
    let (worker, commands) = Worker::recording();
    app.worker = Some(worker);
    (app, commands)
}

fn reply(app: &mut CedarApp, command: &Command, result: Result<Payload, String>) {
    app.apply_event(Event {
        generation: app.generation,
        id: command.id,
        connected: true,
        result,
    });
}
fn search(app: &mut CedarApp, commands: &Receiver<Command>, value: Value) {
    app.request_java_types();
    let command = commands.try_recv().unwrap();
    assert!(
        matches!(&command.op, Operation::LanguageWorkspaceSymbols { query } if query == &app.language.types.query)
    );
    reply(app, &command, Ok(Payload::Language { value }));
}
fn resolve(app: &mut CedarApp, commands: &Receiver<Command>, path: &str) {
    app.select_java_type(0);
    let command = commands.try_recv().unwrap();
    assert!(matches!(command.op, Operation::LanguageResolveUri { .. }));
    reply(
        app,
        &command,
        Ok(Payload::Language {
            value: json!({"path":path}),
        }),
    );
}
fn selection(app: &CedarApp, expected: Range) -> Result<(), String> {
    let doc = app.active().ok_or("No selected document")?;
    let start = completion::position_to_offsets(&doc.text, expected.start)?.1;
    let end = completion::position_to_offsets(&doc.text, expected.end)?.1;
    let state = egui::TextEdit::load_state(&app.editor_ctx, egui::Id::new(("editor", doc.id)))
        .ok_or("No editor selection state")?;
    let range = state.cursor.char_range().ok_or("No selection")?;
    if range.primary.index.min(range.secondary.index) != start
        || range.primary.index.max(range.secondary.index) != end
    {
        return Err("Selected range differs from the complete UTF-16 type range".into());
    }
    Ok(())
}

#[test]
fn workspace_query_is_literal_bounded_utf8_and_not_blank_or_controlled() {
    for query in ["", " ", "\u{3000}", "A\nB", "A\tB", "A\u{7f}", "A\u{85}"] {
        assert!(language_navigation_results::validate_workspace_query(query).is_err());
    }
    for query in [" Target ".to_owned(), "é".repeat(128), "😀".repeat(64)] {
        assert!(language_navigation_results::validate_workspace_query(&query).is_ok());
    }
    assert!(language_navigation_results::validate_workspace_query(&"é".repeat(129)).is_err());
    let (mut app, commands) = type_app();
    app.language.types.query = " Target ".into();
    app.request_java_types();
    assert!(
        matches!(commands.try_recv().unwrap().op, Operation::LanguageWorkspaceSymbols { query } if query == " Target ")
    );
}

#[test]
fn parser_keeps_only_flat_complete_locations_and_sorts_deterministically() {
    let mut tagged = symbol("A", "jdt://contents/dependency.jar/A.class");
    tagged["tags"] = json!([1]);
    let rows = language_navigation_results::parse_workspace_symbols(&json!([
        symbol("Z", URI),
        symbol("A", "file:///b"),
        tagged,
        symbol("A", "file:///a")
    ]))
    .unwrap();
    assert_eq!(
        rows.iter().map(|row| row.name.as_str()).collect::<Vec<_>>(),
        vec!["A", "A", "A", "Z"]
    );
    assert_eq!(rows[0].location.uri, "file:///a");
    assert_eq!(rows[1].location.uri, "file:///b");
    assert!(rows[2].deprecated);
    assert_eq!(rows[2].location.range.end.character, 12);
    for value in [Value::Null, json!([])] {
        assert!(language_navigation_results::parse_workspace_symbols(&value)
            .unwrap()
            .is_empty());
    }
}

#[test]
fn parser_rejects_partial_hierarchical_links_malformed_ranges_and_metadata() {
    let parse = language_navigation_results::parse_workspace_symbols;
    for value in [json!({}), symbol("Target", URI), json!([null]), json!(true)] {
        assert!(parse(&value).is_err());
    }
    for key in ["name", "kind", "location"] {
        let mut value = symbol("Target", URI);
        value.as_object_mut().unwrap().remove(key);
        assert!(parse(&json!([value])).is_err());
    }
    for (key, invalid) in [
        ("name", json!("")),
        ("name", json!("A\nB")),
        ("containerName", json!("a\tb")),
        ("name", json!("n".repeat(4097))),
        ("containerName", json!(null)),
        ("kind", json!(0)),
        ("kind", json!(27)),
        ("kind", json!(5.0)),
        ("kind", json!(-1)),
        ("kind", json!("5")),
        ("deprecated", json!(1)),
        ("tags", json!([2])),
        ("tags", json!([1, 1])),
        ("tags", json!(null)),
        ("children", json!([])),
        ("range", json!({})),
        ("selectionRange", json!({})),
        ("detail", json!("x")),
        ("data", json!({"resolve":true})),
    ] {
        let mut value = symbol("Target", URI);
        value[key] = invalid;
        assert!(parse(&json!([value])).is_err(), "field {key}");
    }
    for uri in [
        "",
        "relative.java",
        "1bad:///A",
        "file:///a b",
        "file:///a\nb",
    ] {
        assert!(parse(&json!([symbol("A", uri)])).is_err());
    }
    for invalid in [
        json!(-1),
        json!(1.0),
        json!(u32::MAX),
        json!("1"),
        Value::Null,
    ] {
        let mut value = symbol("Target", URI);
        value["location"]["range"]["start"]["line"] = invalid;
        assert!(parse(&json!([value])).is_err());
    }
    let mut value = symbol("Target", URI);
    value["location"].as_object_mut().unwrap().remove("range");
    assert!(parse(&json!([value])).is_err());
    let mut value = symbol("Target", URI);
    value["location"]["targetUri"] = json!(URI);
    assert!(parse(&json!([value])).is_err());
    let mut value = symbol("Target", URI);
    value["location"]["range"]["end"]["character"] = json!(5);
    assert!(parse(&json!([value])).is_err());
}

#[test]
fn parser_enforces_inclusive_result_text_and_raw_response_limits() {
    let parse = language_navigation_results::parse_workspace_symbols;
    assert_eq!(
        parse(&json!(vec![symbol("A", URI); 256])).unwrap().len(),
        256
    );
    assert!(parse(&json!(vec![symbol("A", URI); 257])).is_err());
    let uri = format!("file:///{}", "u".repeat(16 * 1024 - 8));
    assert!(parse(&json!([symbol("A", &uri)])).is_ok());
    assert!(parse(&json!([symbol("A", &(uri + "x"))])).is_err());
    let name = "n".repeat(4096);
    let container = "c".repeat(4096);
    let mut item = symbol(&name, URI);
    item["containerName"] = json!(container);
    assert!(parse(&json!(vec![item; 65])).is_err());
    let mut item = symbol("A", URI);
    item["ignored"] = json!("x".repeat(1024 * 1024));
    assert!(parse(&json!([item])).is_err());
    let mut deep = json!(0);
    for _ in 0..12 {
        deep = json!([deep]);
    }
    let mut item = symbol("A", URI);
    item["ignored"] = deep;
    assert!(parse(&json!([item])).is_err());
    let mut item = symbol("A", URI);
    item["ignored"] = json!(vec![0; 16 * 1024]);
    assert!(parse(&json!([item])).is_err());
}

#[test]
fn requires_trusted_running_typed_java_and_both_capabilities_but_no_active_editor() {
    for gate in 0..10 {
        let (mut app, commands) = type_app();
        match gate {
            0 => app.state = crate::ConnectionState::Disconnected,
            1 => app.language.running = false,
            2 => app.active_form.as_mut().unwrap().allow_run = false,
            3 => app.language.mode = ServerMode::Generic,
            4 => app.language.diagnostics_exited = true,
            5 => app.language.capabilities = json!({"workspaceSymbolProvider":false}),
            6 => app.language.capabilities = json!({"workspaceSymbolProvider":"true"}),
            7 => app
                .agent_info
                .as_mut()
                .unwrap()
                .capabilities
                .retain(|name| name != "language_workspace_symbols"),
            8 => app.close_after_language_stop = true,
            9 => app.language.types.query = " ".into(),
            _ => unreachable!(),
        }
        app.request_java_types();
        assert!(commands.try_recv().is_err(), "gate {gate}");
        assert!(app
            .operation_problem(&Operation::LanguageWorkspaceSymbols {
                query: app.language.types.query.clone()
            })
            .is_some());
        assert!(app.language.types.pending.is_none());
    }
    for provider in [json!(true), json!({}), json!({"resolveProvider":true})] {
        let (mut app, commands) = type_app();
        app.language.capabilities = json!({"workspaceSymbolProvider":provider});
        search(&mut app, &commands, symbols());
        assert!(app.language.types.snapshot.is_some());
        assert!(app.documents.is_empty());
    }
}

#[test]
fn search_does_not_sync_save_start_wait_for_indexing_or_change_automatic_preference() {
    for automatic in [false, true] {
        let (mut app, commands) = type_app();
        app.language.automatic = automatic;
        let mut doc = Document::new(1, "Unrelated.java".into(), "disk".into(), "r".into());
        doc.text = "unsynchronized draft".into();
        doc.edit_version = 7;
        app.documents.push(doc);
        app.request_java_types();
        assert!(matches!(
            commands.try_recv().unwrap().op,
            Operation::LanguageWorkspaceSymbols { .. }
        ));
        assert!(commands.try_recv().is_err());
        assert!(app.language.sync.opened.is_empty());
        assert_eq!(app.language.automatic, automatic);
        assert_eq!(app.documents[0].text, "unsynchronized draft");
        assert_eq!(app.documents[0].edit_version, 7);
    }
}

fn invalidate(app: &mut CedarApp, cause: usize) {
    match cause {
        0 => app.language.types.query.push('x'),
        1 => app.language.types.reset(),
        2 => app.language.session += 1,
        3 => app.generation += 1,
        4 => app.active_form.as_mut().unwrap().allow_run = false,
        5 => app.language.running = false,
        6 => app.language.diagnostics_exited = true,
        7 => app.tools_open = false,
        8 => app.tool = crate::Tool::Search,
        9 => app.language.view = View::Problems,
        10 => app.language.capabilities = Value::Null,
        11 => app.language.types.sequence += 1,
        _ => unreachable!(),
    }
}

#[test]
fn all_query_lifetime_changes_drop_delayed_success_errors_and_unexpected_payloads() {
    for cause in 0..12 {
        for result in [
            Ok(Payload::Language { value: symbols() }),
            Err("private old error".into()),
            Ok(Payload::Entries { entries: vec![] }),
        ] {
            let (mut app, commands) = type_app();
            app.request_java_types();
            let command = commands.try_recv().unwrap();
            let generation = app.generation;
            invalidate(&mut app, cause);
            app.invalidate_java_types();
            app.apply_event(Event {
                generation,
                id: command.id,
                connected: true,
                result,
            });
            assert!(app.language.types.snapshot.is_none(), "cause {cause}");
            assert!(app.error.is_none(), "cause {cause}");
            assert!(commands.try_recv().is_err());
        }
    }
}

#[test]
fn current_transport_failure_wins_over_an_obsolete_query() {
    let (mut app, commands) = type_app();
    app.request_java_types();
    let command = commands.try_recv().unwrap();
    app.language.types.reset();
    app.apply_event(Event {
        generation: app.generation,
        id: command.id,
        connected: false,
        result: Err("connection ended".into()),
    });
    assert!(matches!(app.state, crate::ConnectionState::Disconnected));
}

#[test]
fn snapshots_survive_editing_unrelated_drafts_but_do_not_claim_draft_freshness() {
    let (mut app, commands) = type_app();
    search(&mut app, &commands, symbols());
    app.documents.push(Document::new(
        1,
        "Other.java".into(),
        "x".into(),
        "r".into(),
    ));
    app.active_document = Some(1);
    app.documents[0].text.push('y');
    app.documents[0].edit_version += 1;
    app.invalidate_java_types();
    assert!(app.language.types.snapshot.is_some());
    assert!(INDEX_NOTICE.contains("Unversioned"));
    assert!(INDEX_NOTICE.contains("may lag unsaved edits"));
}

#[test]
fn malformed_and_empty_results_clear_pending_without_showing_prior_rows() {
    for result in [
        symbols(),
        json!([]),
        Value::Null,
        json!([{"name":"broken"}]),
    ] {
        let (mut app, commands) = type_app();
        search(&mut app, &commands, result.clone());
        assert!(app.language.types.pending.is_none());
        if result == json!([{"name":"broken"}]) {
            assert!(app.language.types.error.is_some());
            assert!(app.language.types.snapshot.is_none());
        }
        app.request_java_types();
        assert!(app.language.types.snapshot.is_none());
    }
}

#[test]
fn navigation_uses_only_resolve_then_read_and_rejects_unsupported_or_unsafe_targets() {
    for uri in [
        "jdt://contents/rt.jar/Target.class",
        "https://example.com/Target.java",
    ] {
        let (mut app, commands) = type_app();
        search(&mut app, &commands, json!([symbol("Target", uri)]));
        app.select_java_type(0);
        assert!(commands.try_recv().is_err());
        assert!(app.documents.is_empty());
    }
    for path in [
        "../Target.java",
        "/Target.java",
        "C:/Target.java",
        "a\\Target.java",
    ] {
        let (mut app, commands) = type_app();
        search(&mut app, &commands, symbols());
        resolve(&mut app, &commands, path);
        assert!(commands.try_recv().is_err());
        assert!(app.documents.is_empty());
    }
    let (mut app, commands) = type_app();
    search(&mut app, &commands, symbols());
    resolve(&mut app, &commands, "Target.java");
    let command = commands.try_recv().unwrap();
    assert!(matches!(command.op, Operation::Read { ref path } if path == "Target.java"));
    reply(
        &mut app,
        &command,
        Ok(Payload::File {
            path: "Target.java".into(),
            text: SOURCE.into(),
            revision: "r".into(),
        }),
    );
    assert_eq!(app.active().unwrap().path, "Target.java");
    selection(
        &app,
        language_navigation_results::parse_workspace_symbols(&symbols()).unwrap()[0]
            .location
            .range,
    )
    .unwrap();
    assert!(commands.try_recv().is_err());
}

#[test]
fn obsolete_resolves_and_reads_cannot_open_tabs_or_surface_old_errors() {
    for stage in 0..2 {
        for cause in 0..12 {
            for fail in [false, true] {
                let (mut app, commands) = type_app();
                search(&mut app, &commands, symbols());
                app.select_java_type(0);
                let mut command = commands.try_recv().unwrap();
                if stage == 1 {
                    reply(
                        &mut app,
                        &command,
                        Ok(Payload::Language {
                            value: json!({"path":"Target.java"}),
                        }),
                    );
                    command = commands.try_recv().unwrap();
                }
                let generation = app.generation;
                invalidate(&mut app, cause);
                let result = if fail {
                    Err("old read failure".into())
                } else if stage == 0 {
                    Ok(Payload::Language {
                        value: json!({"path":"Target.java"}),
                    })
                } else {
                    Ok(Payload::File {
                        path: "Target.java".into(),
                        text: SOURCE.into(),
                        revision: "r".into(),
                    })
                };
                app.apply_event(Event {
                    generation,
                    id: command.id,
                    connected: true,
                    result,
                });
                assert!(app.documents.is_empty(), "stage {stage} cause {cause}");
                assert!(app.error.is_none(), "stage {stage} cause {cause}");
                assert!(commands.try_recv().is_err());
            }
        }
    }
}

#[test]
fn newer_navigation_tab_limits_and_invalid_utf16_keep_drafts_intact() {
    let (mut app, commands) = type_app();
    search(&mut app, &commands, symbols());
    resolve(&mut app, &commands, "Target.java");
    let read = commands.try_recv().unwrap();
    app.navigation_changed();
    reply(
        &mut app,
        &read,
        Ok(Payload::File {
            path: "Target.java".into(),
            text: SOURCE.into(),
            revision: "r".into(),
        }),
    );
    assert!(app.documents.is_empty());
    for index in 0..32 {
        app.documents.push(Document::new(
            index + 1,
            format!("{index}.java"),
            "x".into(),
            "r".into(),
        ));
    }
    resolve(&mut app, &commands, "Target.java");
    assert!(commands.try_recv().is_err());
    assert_eq!(app.documents.len(), 32);
    let (mut app, commands) = type_app();
    app.documents.push(Document::new(
        1,
        "Target.java".into(),
        "😀".into(),
        "r".into(),
    ));
    search(&mut app, &commands, symbols());
    resolve(&mut app, &commands, "Target.java");
    assert!(app.error.as_ref().unwrap().contains("range"));
    assert_eq!(app.documents[0].text, "😀");
    assert!(!app.documents[0].dirty());
}

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
fn frame(app: &mut CedarApp, time: f64, events: Vec<egui::Event>) -> egui::FullOutput {
    let ctx = app.editor_ctx.clone();
    let mut native = eframe::Frame::_new_kittest();
    ctx.run(
        egui::RawInput {
            time: Some(time),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1320.0, 880.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| eframe::App::update(app, ctx, &mut native),
    )
}
fn label(output: &egui::FullOutput, wanted: &str) -> Option<egui::Pos2> {
    fn find(shape: &egui::epaint::Shape, wanted: &str) -> Option<egui::Pos2> {
        match shape {
            egui::epaint::Shape::Text(text) if text.galley.job.text == wanted => {
                Some(text.visual_bounding_rect().center())
            }
            egui::epaint::Shape::Vec(shapes) => shapes.iter().find_map(|shape| find(shape, wanted)),
            _ => None,
        }
    }
    output
        .shapes
        .iter()
        .find_map(|shape| find(&shape.shape, wanted))
}
fn click(app: &mut CedarApp, time: f64, wanted: &str) {
    frame(app, time, vec![]);
    let output = frame(app, time + 0.01, vec![]);
    let at = label(&output, wanted).unwrap_or_else(|| panic!("Missing label: {wanted}"));
    for (offset, pressed) in [true, false].into_iter().enumerate() {
        frame(
            app,
            time + 0.02 + offset as f64 * 0.01,
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
}

#[test]
fn actual_frames_search_and_enter_dispatch_once_without_an_editor() {
    for enter in [false, true] {
        let (mut app, commands) = type_app();
        app.language.view = View::Problems;
        click(&mut app, 0.0, "Find Java type");
        assert!(commands.try_recv().is_err());
        if enter {
            app.editor_ctx
                .memory_mut(|memory| memory.request_focus(egui::Id::new(QUERY_INPUT)));
            frame(&mut app, 0.1, vec![]);
            frame(&mut app, 0.2, key(egui::Key::Enter, egui::Modifiers::NONE));
        } else {
            click(&mut app, 0.1, "Search");
        }
        let command = commands
            .try_recv()
            .expect("explicit Search or Enter must dispatch");
        assert!(matches!(
            command.op,
            Operation::LanguageWorkspaceSymbols { .. }
        ));
        frame(&mut app, 0.3, vec![]);
        assert!(commands.try_recv().is_err());
        assert!(app.documents.is_empty());
    }
}

#[test]
fn actual_frames_typing_is_inert_and_escape_dismiss_precedes_global_shortcut() {
    let (mut app, commands) = type_app();
    search(&mut app, &commands, symbols());
    frame(&mut app, 0.0, vec![]);
    app.editor_ctx
        .memory_mut(|memory| memory.request_focus(egui::Id::new(QUERY_INPUT)));
    frame(&mut app, 0.1, vec![egui::Event::Text("雪New".into())]);
    assert!(app.language.types.snapshot.is_none());
    assert!(app.language.cjk_seen);
    assert!(commands.try_recv().is_err());
    app.find_open = true;
    frame(&mut app, 0.2, key(egui::Key::Escape, egui::Modifiers::NONE));
    assert!(app.language.view == View::Problems);
    assert!(
        app.find_open,
        "chooser Escape must not reach the global find-close handler"
    );
    assert!(commands.try_recv().is_err());
}

#[test]
fn actual_frames_dismiss_disabled_provider_and_inert_uri_rows() {
    let (mut app, commands) = type_app();
    search(
        &mut app,
        &commands,
        json!([symbol("External", "https://example.com/Type.java")]),
    );
    let output = frame(&mut app, 0.0, vec![]);
    assert!(label(&output, INDEX_NOTICE).is_some());
    click(&mut app, 0.1, "External · Class");
    assert!(commands.try_recv().is_err());
    click(&mut app, 0.2, "Dismiss");
    assert!(app.language.types.snapshot.is_none());
    assert!(app.language.view == View::Problems);
    app.language.capabilities = json!({"workspaceSymbolProvider":false});
    click(&mut app, 0.3, "Find Java type");
    assert!(app.language.view == View::Problems);
    assert!(commands.try_recv().is_err());
}

#[test]
fn actual_frames_result_click_dispatches_only_the_confined_uri_resolver() {
    let (mut app, commands) = type_app();
    search(&mut app, &commands, symbols());
    click(&mut app, 0.0, "Target · Class");
    let command = commands
        .try_recv()
        .expect("Clicking a type row must request URI confinement");
    assert!(matches!(&command.op, Operation::LanguageResolveUri { uri } if uri == URI));
    assert!(commands.try_recv().is_err());
    assert!(app.documents.is_empty());
}

impl CedarApp {
    /// Headless receipt witness: actual frontend query/selection/read handlers,
    /// fed externally verified native-agent symbols, resolver path and file data.
    pub(crate) fn java_type_navigation_acceptance(
        symbols: Value,
        target_uri: &str,
        path: &str,
        disk_text: &str,
        revision: &str,
    ) -> Result<(), String> {
        let (mut app, commands) = type_app();
        app.language.types.query = language_navigation_results::parse_workspace_symbols(&symbols)?
            .into_iter()
            .find(|row| row.location.uri == target_uri)
            .ok_or("Native target missing from type results")?
            .name;
        app.request_java_types();
        let search = commands.try_recv().map_err(|error| error.to_string())?;
        if !matches!(&search.op, Operation::LanguageWorkspaceSymbols { query } if query == &app.language.types.query)
        {
            return Err("Frontend request did not match the verified native symbol query".into());
        }
        reply(&mut app, &search, Ok(Payload::Language { value: symbols }));
        let snapshot = app
            .language
            .types
            .snapshot
            .as_ref()
            .ok_or("No validated type snapshot")?;
        let index = snapshot
            .rows
            .iter()
            .position(|row| row.location.uri == target_uri)
            .ok_or("Native type target missing from frontend rows")?;
        let range = snapshot.rows[index].location.range;
        app.select_java_type(index);
        let resolve = commands.try_recv().map_err(|error| error.to_string())?;
        if !matches!(&resolve.op,Operation::LanguageResolveUri { uri } if uri == target_uri) {
            return Err("Frontend bypassed URI resolution".into());
        }
        reply(
            &mut app,
            &resolve,
            Ok(Payload::Language {
                value: json!({"path":path}),
            }),
        );
        let read = commands.try_recv().map_err(|error| error.to_string())?;
        if !matches!(&read.op,Operation::Read { path:requested } if requested == path) {
            return Err("Frontend bypassed ordinary file Read".into());
        }
        reply(
            &mut app,
            &read,
            Ok(Payload::File {
                path: path.into(),
                text: disk_text.into(),
                revision: revision.into(),
            }),
        );
        frame(&mut app, 0.0, vec![]);
        frame(&mut app, 0.1, vec![]);
        let doc = app.active().ok_or("Navigation did not open a document")?;
        if doc.path != path || doc.text != disk_text || doc.revision.as_deref() != Some(revision) {
            return Err("Opened document differs from verified native file".into());
        }
        selection(&app, range)?;
        let dirty = format!("{disk_text}\n// retained unsaved type navigation witness\n");
        let cursor = dirty.chars().count();
        crate::editor_state::commit(
            &app.editor_ctx,
            &mut app.documents[0],
            dirty.clone(),
            cursor,
        );
        let id = app.documents[0].id;
        let version = app.documents[0].edit_version;
        // A stale ordinary line jump must not overwrite the type selection in
        // the editor frame after reusing this already-open dirty document.
        app.documents[0].jump_to = Some(0);
        app.select_java_type(index);
        let resolve = commands.try_recv().map_err(|error| error.to_string())?;
        if !matches!(&resolve.op,Operation::LanguageResolveUri { uri } if uri == target_uri) {
            return Err("Dirty target bypassed URI resolution".into());
        }
        reply(
            &mut app,
            &resolve,
            Ok(Payload::Language {
                value: json!({"path":path}),
            }),
        );
        if commands.try_recv().is_ok() {
            return Err("Dirty target triggered an unexpected read or write".into());
        }
        frame(&mut app, 0.2, vec![]);
        frame(&mut app, 0.3, vec![]);
        let doc = app.active().ok_or("Dirty target was not selected")?;
        if app.documents.len() != 1
            || doc.id != id
            || doc.text != dirty
            || doc.saved_text != disk_text
            || doc.revision.as_deref() != Some(revision)
            || doc.edit_version != version
            || !doc.dirty()
        {
            return Err("Type navigation replaced or altered the dirty buffer".into());
        }
        selection(&app, range)?;
        app.editor_ctx
            .memory_mut(|memory| memory.request_focus(egui::Id::new(("editor", id))));
        frame(&mut app, 1.0, key(egui::Key::Z, egui::Modifiers::COMMAND));
        let doc = app.active().ok_or("Undo lost the active document")?;
        if doc.text != disk_text
            || doc.saved_text != disk_text
            || doc.revision.as_deref() != Some(revision)
            || doc.dirty()
            || doc.edit_version <= version
        {
            return Err("Undo did not restore the verified disk text".into());
        }
        let undo_version = doc.edit_version;
        frame(&mut app, 2.0, key(egui::Key::Y, egui::Modifiers::COMMAND));
        let doc = app.active().ok_or("Redo lost the active document")?;
        if doc.text != dirty
            || doc.saved_text != disk_text
            || doc.revision.as_deref() != Some(revision)
            || !doc.dirty()
            || doc.edit_version <= undo_version
        {
            return Err("Redo did not restore the unsaved buffer".into());
        }
        if commands.try_recv().is_ok() {
            return Err("Undo or Redo unexpectedly sent a workspace operation".into());
        }
        Ok(())
    }
}

#[test]
fn chooser_acceptance_witness_preserves_dirty_buffer_selection_and_undo_redo() {
    CedarApp::java_type_navigation_acceptance(symbols(), URI, "Target.java", SOURCE, "r0").unwrap();
}
