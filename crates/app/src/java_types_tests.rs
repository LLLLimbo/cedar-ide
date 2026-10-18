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
    let mut input = egui::RawInput {
        time: Some(time),
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1320.0, 880.0),
        )),
        events,
        ..Default::default()
    };
    eframe::App::raw_input_hook(app, &ctx, &mut input);
    ctx.run(input, |ctx| eframe::App::update(app, ctx, &mut native))
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

fn language_entry_center(app: &CedarApp, output: &egui::FullOutput) -> egui::Pos2 {
    let rect = crate::workspace_access_tests::recorded_rect(app, "java_type_language_entry");
    let clip = crate::workspace_access_tests::recorded_clip(app, "java_type_language_entry");
    let visible = rect.intersect(clip);
    output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::epaint::Shape::Text(text)
                if text.galley.job.text == "Find Java type"
                    && visible.contains(text.visual_bounding_rect().center()) =>
            {
                Some(text.visual_bounding_rect().center())
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("Lower type entry is not painted at {rect:?} / {clip:?}"))
}

fn click(app: &mut CedarApp, time: f64, wanted: &str) {
    frame(app, time, vec![]);
    let output = frame(app, time + 0.01, vec![]);
    let at = if wanted == "Find Java type" {
        // Keep the original Language-panel tests on their original control,
        // independently of the identically labelled common header entry.
        language_entry_center(app, &output)
    } else {
        label(&output, wanted).unwrap_or_else(|| panic!("Missing label: {wanted}"))
    };
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

fn hover_type_control(app: &mut CedarApp) -> (egui::FullOutput, egui::FullOutput) {
    frame(app, 0.0, vec![]);
    let output = frame(app, 0.01, vec![]);
    let at = language_entry_center(app, &output);
    let delay = f64::from(app.editor_ctx.style().interaction.tooltip_delay);
    assert!(delay > 0.0);
    frame(app, 0.1, vec![egui::Event::PointerMoved(at)]);
    // First pointer appearance has no velocity history in egui and may show a
    // tooltip immediately. Compare no pointer with a hover settled past delay.
    frame(app, 0.1 + delay + 1.0, vec![]);
    let hovered = frame(app, 0.2 + delay + 1.0, vec![]);
    (output, hovered)
}

#[test]
fn actual_frames_disabled_java_type_hover_draws_reason_without_dispatch() {
    for gate in ["trust", "agent", "provider"] {
        let (mut app, commands) = type_app();
        app.agent_info.as_mut().unwrap().os = "windows".into();
        app.language.view = View::Problems;
        let reason = match gate {
            "trust" => {
                app.active_form.as_mut().unwrap().allow_run = false;
                "Find Java type requires trusted tool permission for this connection"
            }
            "agent" => {
                app.agent_info
                    .as_mut()
                    .unwrap()
                    .capabilities
                    .retain(|name| name != "language_workspace_symbols");
                "The workspace agent does not advertise language_workspace_symbols. Drafts remain editable; upgrade or use an agent with this capability"
            }
            "provider" => {
                app.language.capabilities = json!({"workspaceSymbolProvider":false});
                "The running Java server does not advertise workspace symbol search"
            }
            _ => unreachable!(),
        };
        let (unhovered, hovered) = hover_type_control(&mut app);
        assert!(label(&unhovered, reason).is_none(), "no hover for {gate}");
        assert!(
            label(&hovered, reason).is_some(),
            "disabled reason for {gate}"
        );
        assert!(
            commands.try_recv().is_err(),
            "hover must not dispatch for {gate}"
        );
        click(&mut app, 3.0, "Find Java type");
        assert!(
            app.language.view == View::Problems,
            "disabled click for {gate}"
        );
        assert!(commands.try_recv().is_err());
        assert!(app.language.types.pending.is_none());
    }
}

#[test]
fn actual_frames_enabled_java_type_hover_has_no_disabled_reason_and_remains_usable() {
    let (mut app, commands) = type_app();
    app.agent_info.as_mut().unwrap().os = "windows".into();
    app.language.view = View::Problems;
    let (_, hovered) = hover_type_control(&mut app);
    for reason in [
        "Find Java type requires trusted tool permission for this connection",
        "The workspace agent does not advertise language_workspace_symbols. Drafts remain editable; upgrade or use an agent with this capability",
        "The running Java server does not advertise workspace symbol search",
    ] {
        assert!(label(&hovered, reason).is_none());
    }
    assert!(commands.try_recv().is_err());
    click(&mut app, 3.0, "Find Java type");
    assert!(app.language.view == View::JavaTypes);
    assert!(commands.try_recv().is_err(), "opening the chooser is inert");
    click(&mut app, 4.0, "Search");
    assert!(matches!(
        commands.try_recv().unwrap().op,
        Operation::LanguageWorkspaceSymbols { query } if query == "Target"
    ));
    assert!(commands.try_recv().is_err());
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

// The common header is exercised through the complete production raw-input
// hook and update, with pointer coordinates from its actual rendered response.
fn entry_frame(app: &mut CedarApp, events: Vec<egui::Event>) -> egui::FullOutput {
    let ctx = app.editor_ctx.clone();
    let mut input = egui::RawInput {
        time: Some(ctx.input(|input| input.time) + 0.1),
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1320.0, 880.0),
        )),
        events,
        ..Default::default()
    };
    eframe::App::raw_input_hook(app, &ctx, &mut input);
    let mut native = eframe::Frame::_new_kittest();
    ctx.run(input, |ctx| eframe::App::update(app, ctx, &mut native))
}

fn entry_click(app: &mut CedarApp) {
    for _ in 0..3 {
        entry_frame(app, vec![]);
    }
    let response = crate::workspace_access_tests::recorded_response(app, "java_type_entry");
    let clip = crate::workspace_access_tests::recorded_clip(app, "java_type_entry");
    assert!(clip.contains_rect(response.rect));
    let at = response.rect.center();
    entry_frame(app, vec![egui::Event::PointerMoved(at)]);
    for pressed in [true, false] {
        entry_frame(
            app,
            vec![egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            }],
        );
    }
}

fn closed_type_app() -> (CedarApp, Receiver<Command>) {
    let (mut app, commands) = type_app();
    app.tools_open = false;
    app.tool = crate::Tool::Search;
    app.language.view = View::Problems;
    (app, commands)
}

#[test]
fn common_java_type_reason_checks_connection_and_closing_before_trust_or_session() {
    use crate::ConnectionState;

    for (state, exhausted) in [
        (ConnectionState::Idle, false),
        (ConnectionState::Disconnected, false),
        (ConnectionState::Connecting, false),
        (ConnectionState::Disconnecting, false),
        (ConnectionState::ClosingTransport, false),
        (ConnectionState::CleanupUnverified, false),
        (ConnectionState::Ready, true),
    ] {
        let (mut app, commands) = closed_type_app();
        app.state = state;
        app.connection_generation_exhausted = exhausted;
        app.active_form = None;
        app.language.running = false;
        app.language.mode = ServerMode::Generic;
        app.close_after_language_stop = true;
        let expected = "Find Java type requires a ready workspace connection";
        assert_eq!(app.java_types_problem().as_deref(), Some(expected));
        app.request_java_types();
        assert_eq!(app.error.as_deref(), Some(expected));
        assert!(commands.try_recv().is_err());
        assert!(app.language.types.pending.is_none());
    }

    for recovery_close in [false, true] {
        let (mut app, commands) = closed_type_app();
        app.active_form.as_mut().unwrap().allow_run = false;
        app.language.running = false;
        app.language.mode = ServerMode::Generic;
        if recovery_close {
            app.recovery.begin_close(crate::recovery::CloseGuard::new(
                &[],
                None,
                app.generation,
                0,
            ));
        } else {
            app.close_after_language_stop = true;
        }
        let expected = "Finish closing the current session first";
        assert_eq!(app.java_types_problem().as_deref(), Some(expected));
        app.request_java_types();
        assert_eq!(app.error.as_deref(), Some(expected));
        assert!(commands.try_recv().is_err());
        assert!(app.language.types.pending.is_none());
    }
    let (app, commands) = closed_type_app();
    assert!(app.java_types_problem().is_none());
    assert!(commands.try_recv().is_err());
}

#[test]
fn common_java_type_entry_is_visible_disabled_and_explained_for_every_service_gate() {
    for gate in 0..11 {
        let (mut app, commands) = closed_type_app();
        match gate {
            0 | 9 => {
                app.state = if gate == 0 {
                    crate::ConnectionState::Disconnected
                } else {
                    crate::ConnectionState::Idle
                };
                app.active_form = None;
                app.language.running = false;
                app.language.mode = ServerMode::Generic;
            }
            1 => app.language.running = false,
            2 | 10 => {
                if gate == 2 {
                    app.active_form.as_mut().unwrap().allow_run = false;
                } else {
                    app.active_form = None;
                }
                app.language.running = false;
                app.language.mode = ServerMode::Generic;
            }
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
            _ => unreachable!(),
        }
        // Explicit expectations catch prerequisite precedence, rather than
        // accepting whatever reason the implementation happens to return.
        let reason = match gate {
            0 | 9 => "Find Java type requires a ready workspace connection".into(),
            1 | 4 => "Start a Java / JDT LS session for this connection first".into(),
            2 | 10 => "Find Java type requires trusted tool permission for this connection".into(),
            3 => "Find Java type requires a typed Java / JDT LS session".into(),
            5 | 6 => "The running Java server does not advertise workspace symbol search".into(),
            7 => app.unsupported_message("language_workspace_symbols"),
            8 => "Finish closing the current session first".into(),
            _ => unreachable!(),
        };
        assert_eq!(
            app.java_types_problem(),
            Some(reason.clone()),
            "gate {gate}"
        );
        let unhovered = entry_frame(&mut app, vec![]);
        assert!(label(&unhovered, "Find Java type").is_some());
        let response = crate::workspace_access_tests::recorded_response(&app, "java_type_entry");
        assert!(!response.enabled(), "gate {gate}");
        assert!(label(&unhovered, &reason).is_none());
        entry_frame(
            &mut app,
            vec![egui::Event::PointerMoved(response.rect.center())],
        );
        let delay = f64::from(app.editor_ctx.style().interaction.tooltip_delay);
        for _ in 0..((delay / 0.1).ceil() as usize + 12) {
            entry_frame(&mut app, vec![]);
        }
        let hovered = entry_frame(&mut app, vec![]);
        assert!(label(&hovered, &reason).is_some(), "reason for gate {gate}");
        entry_click(&mut app);
        assert!(!app.tools_open);
        assert!(app.tool == crate::Tool::Search);
        assert!(app.language.view == View::Problems);
        assert!(app.language.types.pending.is_none());
        assert!(commands.try_recv().is_err(), "gate {gate} dispatched work");
    }
}

#[test]
fn common_java_type_entry_focuses_query_and_reopens_without_search_or_draft_mutation() {
    let (mut app, commands) = closed_type_app();
    let draft = "class Target { int draft; }\n";
    let mut doc = Document::new(1, "Target.java".into(), SOURCE.into(), "r".into());
    crate::editor_state::commit(&app.editor_ctx, &mut doc, draft.into(), 8);
    app.documents.push(doc);
    app.active_document = Some(1);
    entry_frame(&mut app, vec![]);
    let selection = egui::TextEdit::load_state(&app.editor_ctx, egui::Id::new(("editor", 1_u64)))
        .unwrap()
        .cursor
        .char_range();
    let version = app.documents[0].edit_version;
    for _ in 0..2 {
        entry_click(&mut app);
        assert!(app.tools_open && app.tool == crate::Tool::Language);
        assert!(app.language.view == View::JavaTypes);
        assert!(app
            .editor_ctx
            .memory(|memory| memory.has_focus(egui::Id::new(QUERY_INPUT))));
        assert_eq!(app.language.types.query, "Target");
        assert!(app.language.types.pending.is_none());
        assert!(commands.try_recv().is_err());
        entry_frame(&mut app, key(egui::Key::Escape, egui::Modifiers::NONE));
        assert!(app.language.view == View::Problems);
    }
    entry_click(&mut app);
    // Typing is the very next raw frame, without another query-focus click.
    entry_frame(&mut app, vec![egui::Event::Text("X".into())]);
    assert!(app.language.types.query.contains('X'));
    assert!(commands.try_recv().is_err());
    assert_eq!(app.documents[0].text, draft);
    assert_eq!(app.documents[0].saved_text, SOURCE);
    assert_eq!(app.documents[0].edit_version, version);
    assert_eq!(app.active_document, Some(1));
    assert_eq!(
        egui::TextEdit::load_state(&app.editor_ctx, egui::Id::new(("editor", 1_u64)))
            .unwrap()
            .cursor
            .char_range(),
        selection
    );
    assert!(!app.language.automatic);
    assert!(app.language.sync.opened.is_empty());
    entry_frame(&mut app, key(egui::Key::Enter, egui::Modifiers::NONE));
    assert!(matches!(commands.try_recv().unwrap().op,
        Operation::LanguageWorkspaceSymbols { query } if query == app.language.types.query));
    assert!(commands.try_recv().is_err());

    let state =
        egui::TextEdit::load_state(&app.editor_ctx, egui::Id::new(("editor", 1_u64))).unwrap();
    let mut history = state.undoer();
    let current = (state.cursor.char_range().unwrap(), draft.to_owned());
    let previous = history.undo(&current).unwrap().clone();
    assert_eq!(previous.1, SOURCE);
    assert_eq!(history.redo(&previous).unwrap().1, draft);
}

#[test]
fn common_java_type_reopen_retires_hidden_pending_queries_before_they_can_revive() {
    for hidden in 0..3 {
        for fail in [false, true] {
            let (mut app, commands) = type_app();
            app.request_java_types();
            let command = commands.try_recv().unwrap();
            match hidden {
                0 => app.tools_open = false,
                1 => app.tool = crate::Tool::Search,
                2 => app.language.view = View::Problems,
                _ => unreachable!(),
            }
            // No idle invalidation frame between hiding and reopening.
            app.show_java_types(&app.editor_ctx.clone());
            assert!(app.language.types.pending.is_none());
            reply(
                &mut app,
                &command,
                if fail {
                    Err("obsolete type search failure".into())
                } else {
                    Ok(Payload::Language { value: symbols() })
                },
            );
            assert!(app.language.types.snapshot.is_none());
            assert!(app.language.types.error.is_none());
            assert!(app.error.is_none());
            assert_eq!(app.language.types.query, "Target");
            assert!(commands.try_recv().is_err());
        }
    }
}

#[test]
fn common_java_type_entry_preserves_a_current_snapshot_without_repeating_search() {
    let (mut app, commands) = type_app();
    search(&mut app, &commands, symbols());
    let sequence = app.language.types.sequence;
    entry_click(&mut app);
    assert_eq!(app.language.types.sequence, sequence);
    assert_eq!(app.language.types.snapshot.as_ref().unwrap().rows.len(), 1);
    assert!(commands.try_recv().is_err());
}

#[test]
fn common_java_type_keyboard_entry_does_not_submit_its_activation_key() {
    for activation in [egui::Key::Enter, egui::Key::Space] {
        let (mut app, commands) = closed_type_app();
        entry_frame(&mut app, vec![]);
        let id = crate::workspace_access_tests::recorded_response(&app, "java_type_entry").id;
        for _ in 0..80 {
            if app.editor_ctx.memory(|memory| memory.has_focus(id)) {
                break;
            }
            entry_frame(&mut app, key(egui::Key::Tab, egui::Modifiers::NONE));
        }
        assert!(app.editor_ctx.memory(|memory| memory.has_focus(id)));
        let mut events = key(activation, egui::Modifiers::NONE);
        if activation == egui::Key::Space {
            events.insert(1, egui::Event::Text(" ".into()));
        }
        entry_frame(&mut app, events);
        assert!(app.tools_open && app.tool == crate::Tool::Language);
        assert!(app.language.view == View::JavaTypes);
        assert!(
            commands.try_recv().is_err(),
            "activation dispatched a query"
        );
        assert_eq!(app.language.types.query, "Target");
        assert!(app
            .editor_ctx
            .memory(|memory| memory.has_focus(egui::Id::new(QUERY_INPUT))));
        entry_frame(&mut app, vec![egui::Event::Text("Z".into())]);
        assert!(app.language.types.query.contains('Z'));
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn common_java_type_entry_cannot_bypass_modal_or_new_service_unavailability() {
    for blocked in ["modal", "trust", "provider"] {
        let (mut app, commands) = closed_type_app();
        let focus = egui::Id::new("another owner");
        app.editor_ctx
            .memory_mut(|memory| memory.request_focus(focus));
        match blocked {
            "modal" => app.confirm = Some(crate::Confirm::CloseWindow),
            "trust" => app.active_form.as_mut().unwrap().allow_run = false,
            "provider" => app.language.capabilities = Value::Null,
            _ => unreachable!(),
        }
        app.show_java_types(&app.editor_ctx.clone());
        assert!(!app.tools_open);
        assert!(app.tool == crate::Tool::Search);
        assert!(app.language.view == View::Problems);
        assert_eq!(
            app.editor_ctx.memory(|memory| memory.focused()),
            Some(focus)
        );
        assert!(commands.try_recv().is_err());
    }
}

fn keyboard_type_app(value: Value) -> (CedarApp, Receiver<Command>) {
    let (mut app, commands) = type_app();
    search(&mut app, &commands, value);
    let ctx = app.editor_ctx.clone();
    app.show_java_types(&ctx);
    entry_frame(&mut app, vec![]);
    entry_frame(&mut app, vec![]);
    assert!(ctx.memory(|memory| memory.has_focus(egui::Id::new(QUERY_INPUT))));
    (app, commands)
}

fn assert_type_result_focus(app: &CedarApp, index: usize) {
    let context = &app.language.types.snapshot.as_ref().unwrap().context;
    assert_eq!(app.language.types.selected, Some(index));
    assert!(app
        .editor_ctx
        .memory(|memory| memory.has_focus(result_id(context, index))));
}

#[test]
fn keyboard_type_query_submit_then_separate_down_and_enter_opens_exact_result() {
    let (mut app, commands) = closed_type_app();
    entry_click(&mut app);
    entry_frame(&mut app, key(egui::Key::Enter, egui::Modifiers::NONE));
    let request = commands.try_recv().unwrap();
    assert!(matches!(
        request.op,
        Operation::LanguageWorkspaceSymbols { .. }
    ));
    assert!(commands.try_recv().is_err());
    assert!(app.language.types.selected.is_none());
    assert!(app
        .editor_ctx
        .memory(|memory| memory.has_focus(egui::Id::new(QUERY_INPUT))));
    reply(
        &mut app,
        &request,
        Ok(Payload::Language { value: symbols() }),
    );
    entry_frame(&mut app, vec![]);
    assert!(commands.try_recv().is_err());
    assert!(app.language.types.selected.is_none());
    entry_frame(&mut app, key(egui::Key::ArrowDown, egui::Modifiers::NONE));
    assert_type_result_focus(&app, 0);
    assert!(commands.try_recv().is_err());
    entry_frame(&mut app, key(egui::Key::Enter, egui::Modifiers::NONE));
    assert!(matches!(commands.try_recv().unwrap().op,
        Operation::LanguageResolveUri { uri } if uri == URI));
    entry_frame(&mut app, vec![]);
    assert!(commands.try_recv().is_err());
    assert!(app.documents.is_empty());
}

#[test]
fn keyboard_type_query_held_enter_cannot_resubmit_or_drop_focus() {
    for finish_request in [false, true] {
        let (mut app, commands) = closed_type_app();
        entry_click(&mut app);
        let enter = key(egui::Key::Enter, egui::Modifiers::NONE);
        entry_frame(&mut app, vec![enter[0].clone()]);
        let request = commands.try_recv().unwrap();
        assert!(matches!(
            request.op,
            Operation::LanguageWorkspaceSymbols { .. }
        ));
        if finish_request {
            reply(
                &mut app,
                &request,
                Ok(Payload::Language { value: symbols() }),
            );
        }
        for repeated in [false, true] {
            let mut event = enter[0].clone();
            if let egui::Event::Key { repeat, .. } = &mut event {
                *repeat = repeated;
            }
            entry_frame(&mut app, vec![event]);
            assert!(commands.try_recv().is_err());
            assert!(app
                .editor_ctx
                .memory(|memory| memory.has_focus(egui::Id::new(QUERY_INPUT))));
        }
        if !finish_request {
            reply(
                &mut app,
                &request,
                Ok(Payload::Language { value: symbols() }),
            );
        }
        entry_frame(&mut app, vec![enter[1].clone()]);
        entry_frame(&mut app, enter);
        assert!(matches!(
            commands.try_recv().unwrap().op,
            Operation::LanguageWorkspaceSymbols { .. }
        ));
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn keyboard_type_native_key_release_survives_focus_transfer_to_result() {
    let (mut app, commands) = keyboard_type_app(symbols());
    let home = key(egui::Key::Home, egui::Modifiers::NONE);
    entry_frame(&mut app, vec![home[0].clone()]);
    assert!(app
        .editor_ctx
        .input(|input| input.key_down(egui::Key::Home)));
    let id = result_id(&app.language.types.snapshot.as_ref().unwrap().context, 0);
    app.editor_ctx.memory_mut(|memory| memory.request_focus(id));
    entry_frame(&mut app, vec![home[1].clone()]);
    assert!(!app
        .editor_ctx
        .input(|input| input.key_down(egui::Key::Home)));
    assert!(app.language.types.held_keys.is_empty());
    assert!(commands.try_recv().is_err());
}

#[test]
fn keyboard_type_missing_open_capabilities_never_selects_a_disabled_result() {
    for missing in ["language_resolve_uri", "read"] {
        let (mut app, commands) = keyboard_type_app(symbols());
        app.agent_info
            .as_mut()
            .unwrap()
            .capabilities
            .retain(|name| name != missing);
        entry_frame(&mut app, key(egui::Key::ArrowDown, egui::Modifiers::NONE));
        assert!(app.language.types.selected.is_none());
        assert!(app
            .editor_ctx
            .memory(|memory| memory.has_focus(egui::Id::new(QUERY_INPUT))));
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn keyboard_type_busy_other_row_click_is_inert_and_keeps_original_activation() {
    let (mut app, commands) = keyboard_type_app(json!([
        symbol("Alpha", "file:///workspace/A.java"),
        symbol("Target", URI)
    ]));
    entry_frame(&mut app, key(egui::Key::ArrowDown, egui::Modifiers::NONE));
    entry_frame(&mut app, key(egui::Key::Enter, egui::Modifiers::NONE));
    let resolve = commands.try_recv().unwrap();
    // After end_pass, read_response can still prefer the preceding pass's
    // widget cache. Repaint while the resolver remains deliberately pending.
    entry_frame(&mut app, vec![]);
    let response = crate::workspace_access_tests::recorded_response(&app, "java_type_row_1");
    assert!(
        !response.enabled(),
        "busy={} selected={:?} focus={:?} row={:?}",
        app.language_busy(),
        app.language.types.selected,
        app.editor_ctx.memory(|memory| memory.focused()),
        response.id
    );
    for pressed in [true, false] {
        entry_frame(
            &mut app,
            vec![egui::Event::PointerButton {
                pos: response.rect.center(),
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            }],
        );
    }
    assert_eq!(app.language.types.selected, Some(0));
    assert!(commands.try_recv().is_err());
    reply(
        &mut app,
        &resolve,
        Ok(Payload::Language {
            value: json!({"path":"A.java"}),
        }),
    );
    assert!(
        matches!(commands.try_recv().unwrap().op, Operation::Read { path } if path == "A.java")
    );
    assert!(commands.try_recv().is_err());
}

#[test]
fn keyboard_type_held_enter_does_not_cancel_ready_resolve_or_read() {
    for repeated in [false, true] {
        let (mut app, commands) = keyboard_type_app(symbols());
        entry_frame(&mut app, key(egui::Key::ArrowDown, egui::Modifiers::NONE));
        let enter = key(egui::Key::Enter, egui::Modifiers::NONE);
        entry_frame(&mut app, vec![enter[0].clone()]);
        let resolve = commands.try_recv().unwrap();
        let mut held = enter[0].clone();
        if let egui::Event::Key { repeat, .. } = &mut held {
            *repeat = repeated;
        }
        app.result_tx
            .send(crate::worker::WorkerEvent::Response(Event {
                generation: app.generation,
                id: resolve.id,
                connected: true,
                result: Ok(Payload::Language {
                    value: json!({"path":"Target.java"}),
                }),
            }))
            .unwrap();
        entry_frame(&mut app, vec![held.clone()]);
        let read = commands
            .try_recv()
            .expect("holding Enter must not cancel its initial resolve");
        assert!(matches!(read.op, Operation::Read { .. }));
        app.result_tx
            .send(crate::worker::WorkerEvent::Response(Event {
                generation: app.generation,
                id: read.id,
                connected: true,
                result: Ok(Payload::File {
                    path: "Target.java".into(),
                    text: SOURCE.into(),
                    revision: "r".into(),
                }),
            }))
            .unwrap();
        entry_frame(&mut app, vec![held.clone()]);
        entry_frame(&mut app, vec![]);
        let editor = egui::Id::new(("editor", app.active().unwrap().id));
        assert!(app.editor_ctx.memory(|memory| memory.has_focus(editor)));
        let selected = egui::TextEdit::load_state(&app.editor_ctx, editor)
            .unwrap()
            .cursor
            .char_range();
        entry_frame(&mut app, vec![held]);
        assert_eq!(
            egui::TextEdit::load_state(&app.editor_ctx, editor)
                .unwrap()
                .cursor
                .char_range(),
            selected
        );
        entry_frame(&mut app, vec![enter[1].clone()]);
        assert_eq!(app.active().unwrap().path, "Target.java");
        assert_eq!(app.active().unwrap().text, SOURCE);
        assert_eq!(app.documents.len(), 1);
        assert!(commands.try_recv().is_err());
        entry_frame(&mut app, enter);
        assert_ne!(
            app.active().unwrap().text,
            SOURCE,
            "fresh editor Enter must work"
        );
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn keyboard_type_newer_selection_retires_pending_or_ready_open_without_focus_steal() {
    for ready_same_frame in [false, true] {
        let (mut app, commands) = keyboard_type_app(json!([
            symbol("Alpha", "file:///workspace/A.java"),
            symbol("Target", URI)
        ]));
        entry_frame(&mut app, key(egui::Key::ArrowDown, egui::Modifiers::NONE));
        entry_frame(&mut app, key(egui::Key::Enter, egui::Modifiers::NONE));
        let resolve = commands.try_recv().unwrap();
        reply(
            &mut app,
            &resolve,
            Ok(Payload::Language {
                value: json!({"path":"A.java"}),
            }),
        );
        let read = commands.try_recv().unwrap();
        assert!(matches!(read.op, Operation::Read { .. }));
        let completion = crate::worker::WorkerEvent::Response(Event {
            generation: app.generation,
            id: read.id,
            connected: true,
            result: Ok(Payload::File {
                path: "A.java".into(),
                text: "class Alpha {}\n".into(),
                revision: "r".into(),
            }),
        });
        if ready_same_frame {
            app.result_tx.send(completion).unwrap();
            entry_frame(&mut app, key(egui::Key::ArrowDown, egui::Modifiers::NONE));
            // Retiring a ready reply also retires the captured navigation epoch;
            // keep the original result focus and require a fresh move.
            assert_type_result_focus(&app, 0);
            entry_frame(&mut app, key(egui::Key::ArrowDown, egui::Modifiers::NONE));
        } else {
            entry_frame(&mut app, key(egui::Key::ArrowDown, egui::Modifiers::NONE));
            app.result_tx.send(completion).unwrap();
            entry_frame(&mut app, vec![]);
        }
        assert_type_result_focus(&app, 1);
        assert!(app.documents.is_empty());
        assert!(app.active_document.is_none());
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn keyboard_type_movement_is_bounded_skips_disabled_rows_and_reveals_end() {
    let values: Vec<_> = (0..40)
        .map(|index| {
            symbol(
                &format!("Type{index:02}"),
                &format!("file:///workspace/Type{index:02}.java"),
            )
        })
        .chain([symbol("Type20a", "jdt://dependency/Type.class")])
        .collect();
    let (mut app, commands) = keyboard_type_app(json!(values));
    for (key_code, expected) in [
        (egui::Key::ArrowDown, 0),
        (egui::Key::ArrowUp, 0),
        (egui::Key::End, 40),
        (egui::Key::ArrowDown, 40),
        (egui::Key::ArrowUp, 39),
        (egui::Key::Home, 0),
    ] {
        entry_frame(&mut app, key(key_code, egui::Modifiers::NONE));
        assert_type_result_focus(&app, expected);
        assert!(commands.try_recv().is_err());
    }
    for _ in 0..21 {
        entry_frame(&mut app, key(egui::Key::ArrowDown, egui::Modifiers::NONE));
    }
    assert_type_result_focus(&app, 22);
    entry_frame(&mut app, key(egui::Key::End, egui::Modifiers::NONE));
    for _ in 0..12 {
        entry_frame(&mut app, vec![]);
    }
    let response = crate::workspace_access_tests::recorded_response(&app, "java_type_row_40");
    let clip = crate::workspace_access_tests::recorded_clip(&app, "java_type_row_40");
    assert!(
        clip.contains(response.rect.center()),
        "selected final row must be visible"
    );
    assert_type_result_focus(&app, 40);
    assert!(commands.try_recv().is_err());
}

#[test]
fn keyboard_type_empty_or_unopenable_results_keep_query_and_send_nothing() {
    for value in [
        json!([]),
        json!([symbol("External", "https://example.com/T.java")]),
    ] {
        let (mut app, commands) = keyboard_type_app(value);
        entry_frame(&mut app, key(egui::Key::ArrowDown, egui::Modifiers::NONE));
        assert!(app.language.types.selected.is_none());
        assert!(app
            .editor_ctx
            .memory(|memory| memory.has_focus(egui::Id::new(QUERY_INPUT))));
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn keyboard_type_query_caret_keys_modifiers_and_edits_do_not_select_results() {
    let (mut app, commands) = keyboard_type_app(symbols());
    for (key_code, modifiers) in [
        (egui::Key::Home, egui::Modifiers::NONE),
        (egui::Key::End, egui::Modifiers::NONE),
        (egui::Key::ArrowUp, egui::Modifiers::NONE),
        (egui::Key::ArrowDown, egui::Modifiers::SHIFT),
        (egui::Key::ArrowDown, egui::Modifiers::COMMAND),
    ] {
        entry_frame(&mut app, key(key_code, modifiers));
        assert!(app.language.types.selected.is_none());
        assert!(app
            .editor_ctx
            .memory(|memory| memory.has_focus(egui::Id::new(QUERY_INPUT))));
        assert!(commands.try_recv().is_err());
    }
    let mut mixed = vec![egui::Event::Text("Changed".into())];
    mixed.extend(key(egui::Key::ArrowDown, egui::Modifiers::NONE));
    entry_frame(&mut app, mixed);
    assert!(app.language.types.snapshot.is_none());
    assert!(app.language.types.selected.is_none());
    assert!(commands.try_recv().is_err());
}

#[test]
fn keyboard_type_result_rejects_mixed_modified_and_composing_enter() {
    for competing in [
        egui::Event::Text("x".into()),
        egui::Event::Paste("x".into()),
        egui::Event::Ime(egui::ImeEvent::Preedit(String::new())),
        egui::Event::Ime(egui::ImeEvent::Commit(String::new())),
        egui::Event::Ime(egui::ImeEvent::Disabled),
    ] {
        let (mut app, commands) = keyboard_type_app(symbols());
        entry_frame(&mut app, key(egui::Key::ArrowDown, egui::Modifiers::NONE));
        let mut events = key(egui::Key::Enter, egui::Modifiers::NONE);
        events.insert(0, competing);
        entry_frame(&mut app, events);
        assert!(commands.try_recv().is_err());
    }
    let (mut app, commands) = keyboard_type_app(symbols());
    entry_frame(&mut app, key(egui::Key::ArrowDown, egui::Modifiers::NONE));
    for modifiers in [
        egui::Modifiers::SHIFT,
        egui::Modifiers::ALT,
        egui::Modifiers::COMMAND,
    ] {
        entry_frame(&mut app, key(egui::Key::Enter, modifiers));
        assert!(commands.try_recv().is_err());
    }
    entry_frame(
        &mut app,
        vec![egui::Event::Ime(egui::ImeEvent::Preedit(
            "composition".into(),
        ))],
    );
    entry_frame(&mut app, key(egui::Key::Enter, egui::Modifiers::NONE));
    assert!(commands.try_recv().is_err());
    entry_frame(&mut app, vec![egui::Event::Ime(egui::ImeEvent::Disabled)]);
    entry_frame(&mut app, key(egui::Key::Enter, egui::Modifiers::NONE));
    assert!(matches!(
        commands.try_recv().unwrap().op,
        Operation::LanguageResolveUri { .. }
    ));
    assert!(commands.try_recv().is_err());
}

#[test]
fn keyboard_type_composition_keeps_query_focus_until_a_fresh_down() {
    let (mut app, commands) = keyboard_type_app(symbols());
    entry_frame(
        &mut app,
        vec![egui::Event::Ime(egui::ImeEvent::Preedit(String::new()))],
    );
    entry_frame(&mut app, key(egui::Key::ArrowDown, egui::Modifiers::NONE));
    assert!(app.language.types.selected.is_none());
    assert!(app
        .editor_ctx
        .memory(|memory| memory.has_focus(egui::Id::new(QUERY_INPUT))));
    assert!(commands.try_recv().is_err());
}

#[test]
fn keyboard_type_held_and_repeat_keys_do_not_move_or_reopen_results() {
    let (mut app, commands) = keyboard_type_app(json!([
        symbol("Alpha", "file:///workspace/A.java"),
        symbol("Target", URI)
    ]));
    let down = key(egui::Key::ArrowDown, egui::Modifiers::NONE);
    entry_frame(&mut app, vec![down[0].clone()]);
    assert_type_result_focus(&app, 0);
    entry_frame(&mut app, vec![down[0].clone()]);
    assert_type_result_focus(&app, 0);
    let mut repeat = down[0].clone();
    if let egui::Event::Key { repeat, .. } = &mut repeat {
        *repeat = true;
    }
    entry_frame(&mut app, vec![repeat]);
    assert_type_result_focus(&app, 0);
    entry_frame(&mut app, vec![down[1].clone()]);
    entry_frame(&mut app, down);
    assert_type_result_focus(&app, 1);
    let enter = key(egui::Key::Enter, egui::Modifiers::NONE);
    entry_frame(&mut app, vec![enter[0].clone()]);
    let resolve = commands.try_recv().unwrap();
    assert!(matches!(resolve.op, Operation::LanguageResolveUri { .. }));
    reply(&mut app, &resolve, Err("bounded failed resolve".into()));
    entry_frame(&mut app, vec![enter[0].clone()]);
    assert!(commands.try_recv().is_err());
    // Losing window focus clears ordinary held-key tracking, but must not
    // turn this still-owned activation into a fresh press on return.
    entry_frame(&mut app, vec![egui::Event::WindowFocused(false)]);
    entry_frame(&mut app, vec![egui::Event::WindowFocused(true)]);
    let id = result_id(&app.language.types.snapshot.as_ref().unwrap().context, 1);
    app.editor_ctx.memory_mut(|memory| memory.request_focus(id));
    entry_frame(&mut app, vec![enter[0].clone()]);
    assert!(commands.try_recv().is_err());
    entry_frame(&mut app, vec![enter[1].clone()]);
    entry_frame(&mut app, enter);
    assert!(matches!(
        commands.try_recv().unwrap().op,
        Operation::LanguageResolveUri { .. }
    ));
    assert!(commands.try_recv().is_err());
}

#[test]
fn keyboard_type_capture_cannot_retarget_after_context_focus_or_navigation_changes() {
    for cause in 0..15 {
        let (mut app, commands) = keyboard_type_app(symbols());
        entry_frame(&mut app, key(egui::Key::ArrowDown, egui::Modifiers::NONE));
        let ctx = app.editor_ctx.clone();
        let mut raw = egui::RawInput {
            time: Some(ctx.input(|input| input.time) + 0.1),
            events: key(egui::Key::Enter, egui::Modifiers::NONE),
            ..Default::default()
        };
        eframe::App::raw_input_hook(&mut app, &ctx, &mut raw);
        assert!(app.language.types.key_intent.is_some());
        match cause {
            0..12 => invalidate(&mut app, cause),
            12 => app.navigation_epoch += 1,
            13 => ctx.memory_mut(|memory| memory.request_focus(egui::Id::new(QUERY_INPUT))),
            14 => {
                app.documents.push(Document::new(
                    1,
                    "other.java".into(),
                    SOURCE.into(),
                    "r".into(),
                ));
                app.active_document = Some(1);
            }
            _ => unreachable!(),
        }
        let mut native = eframe::Frame::_new_kittest();
        let _ = ctx.run(raw, |ctx| eframe::App::update(&mut app, ctx, &mut native));
        assert!(commands.try_recv().is_err(), "stale cause {cause}");
    }
}

#[test]
fn keyboard_type_dismiss_reopen_and_query_change_retire_selected_identity() {
    let (mut app, commands) = keyboard_type_app(symbols());
    entry_frame(&mut app, key(egui::Key::ArrowDown, egui::Modifiers::NONE));
    let old = result_id(&app.language.types.snapshot.as_ref().unwrap().context, 0);
    entry_frame(&mut app, key(egui::Key::Escape, egui::Modifiers::NONE));
    assert!(app.language.types.selected.is_none());
    assert!(app.language.types.snapshot.is_none());
    entry_click(&mut app);
    entry_frame(&mut app, key(egui::Key::ArrowDown, egui::Modifiers::NONE));
    assert!(app.language.types.selected.is_none());
    assert!(commands.try_recv().is_err());
    search(&mut app, &commands, symbols());
    entry_frame(&mut app, vec![]);
    assert_ne!(
        old,
        result_id(&app.language.types.snapshot.as_ref().unwrap().context, 0)
    );
    entry_frame(&mut app, key(egui::Key::ArrowDown, egui::Modifiers::NONE));
    assert_type_result_focus(&app, 0);
    app.editor_ctx
        .memory_mut(|memory| memory.request_focus(egui::Id::new(QUERY_INPUT)));
    entry_frame(&mut app, vec![egui::Event::Text("New".into())]);
    assert!(app.language.types.selected.is_none());
    assert!(app.language.types.snapshot.is_none());
    assert!(commands.try_recv().is_err());
}

#[test]
fn keyboard_type_dirty_buffer_reuse_preserves_text_selection_and_undo() {
    let (mut app, commands) = type_app();
    let draft = "class Target { int draft; }\n";
    let mut doc = Document::new(1, "Target.java".into(), SOURCE.into(), "r".into());
    crate::editor_state::commit(&app.editor_ctx, &mut doc, draft.into(), 8);
    app.documents.push(doc);
    app.active_document = Some(1);
    entry_frame(&mut app, vec![]);
    let editor = egui::Id::new(("editor", 1_u64));
    let selection = egui::TextEdit::load_state(&app.editor_ctx, editor)
        .unwrap()
        .cursor
        .char_range();
    let version = app.documents[0].edit_version;
    search(&mut app, &commands, symbols());
    let ctx = app.editor_ctx.clone();
    app.show_java_types(&ctx);
    entry_frame(&mut app, vec![]);
    entry_frame(&mut app, key(egui::Key::ArrowDown, egui::Modifiers::NONE));
    assert_type_result_focus(&app, 0);
    assert_eq!(
        egui::TextEdit::load_state(&ctx, editor)
            .unwrap()
            .cursor
            .char_range(),
        selection
    );
    assert_eq!(app.documents[0].text, draft);
    assert_eq!(app.documents[0].edit_version, version);
    assert!(commands.try_recv().is_err());
    let enter = key(egui::Key::Enter, egui::Modifiers::NONE);
    entry_frame(&mut app, vec![enter[0].clone()]);
    let resolve = commands.try_recv().unwrap();
    assert!(matches!(&resolve.op, Operation::LanguageResolveUri { uri } if uri == URI));
    reply(
        &mut app,
        &resolve,
        Ok(Payload::Language {
            value: json!({"path":"Target.java"}),
        }),
    );
    entry_frame(&mut app, vec![]);
    entry_frame(&mut app, vec![]);
    assert!(ctx.memory(|memory| memory.has_focus(editor)));
    let selected = egui::TextEdit::load_state(&ctx, editor)
        .unwrap()
        .cursor
        .char_range();
    for repeated in [false, true] {
        let mut held = enter[0].clone();
        if let egui::Event::Key { repeat, .. } = &mut held {
            *repeat = repeated;
        }
        entry_frame(&mut app, vec![held]);
        assert_eq!(app.documents[0].text, draft);
        assert_eq!(
            egui::TextEdit::load_state(&ctx, editor)
                .unwrap()
                .cursor
                .char_range(),
            selected
        );
    }
    entry_frame(&mut app, vec![enter[1].clone()]);
    assert_eq!(app.documents.len(), 1);
    assert_eq!(app.documents[0].text, draft);
    assert_eq!(app.documents[0].saved_text, SOURCE);
    assert_eq!(app.documents[0].edit_version, version);
    assert!(
        commands.try_recv().is_err(),
        "dirty target must not read or write"
    );
    ctx.memory_mut(|memory| memory.request_focus(editor));
    entry_frame(&mut app, key(egui::Key::Z, egui::Modifiers::COMMAND));
    assert_eq!(app.documents[0].text, SOURCE);
    entry_frame(&mut app, key(egui::Key::Y, egui::Modifiers::COMMAND));
    assert_eq!(app.documents[0].text, draft);
    entry_frame(&mut app, enter);
    assert_ne!(app.documents[0].text, draft, "fresh editor Enter must work");
    assert!(commands.try_recv().is_err());
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

#[test]
fn java_type_location_history_admits_once_after_final_selection() {
    let (mut app, commands) = type_app();
    app.documents.push(Document::new(
        1,
        "Source.java".into(),
        "class Source {}".into(),
        "r".into(),
    ));
    app.active_document = Some(1);
    app.next_document = 2;
    search(&mut app, &commands, symbols());
    assert!(app.location_history.back.is_empty());
    resolve(&mut app, &commands, "Target.java");
    assert!(app.location_history.back.is_empty());
    let read = commands.try_recv().unwrap();
    reply(
        &mut app,
        &read,
        Ok(Payload::File {
            path: "Target.java".into(),
            text: SOURCE.into(),
            revision: "r".into(),
        }),
    );
    assert_eq!(app.location_history.back.len(), 1);
    assert_eq!(app.location_history.back[0].document, 1);
    assert!(app.location_history.pending.is_none());
    assert_eq!(app.active_document, Some(2));
}
