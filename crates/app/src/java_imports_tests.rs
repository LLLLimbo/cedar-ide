//! The explicit Java operation shares formatting's checked edit transaction.
use super::*;

const JAVA_BEFORE: &str = "// 🐻 café\r\npackage demo;\r\nimport java.util.Set;\r\nimport java.util.List;\r\nimport java.util.Map;\r\nclass Demo { List<String> xs; Set<String> ys; ArrayList<String> zs; }\r\n";
const JAVA_AFTER: &str = "// 🐻 café\r\npackage demo;\r\nimport java.util.ArrayList;\r\nimport java.util.List;\r\nimport java.util.Set;\r\nclass Demo { List<String> xs; Set<String> ys; ArrayList<String> zs; }\r\n";

fn java_app() -> CedarApp {
    let mut app = app();
    app.open_form = false;
    app.active_form = Some(crate::ConnectForm {
        allow_run: true,
        ..Default::default()
    });
    app.language.mode = ServerMode::Java;
    app.language.java_organize_imports_supported = true;
    app.documents[0] = Document::new(1, "Demo.java".into(), JAVA_BEFORE.into(), "r0".into());
    acknowledge(&mut app, 1, 2);
    app.editor_ctx
        .memory_mut(|memory| memory.request_focus(egui::Id::new(("editor", 1u64))));
    app
}

fn import_edits() -> Value {
    json!([{"range":{"start":{"line":2,"character":0},"end":{"line":5,"character":0}},"newText":"import java.util.ArrayList;\r\nimport java.util.List;\r\nimport java.util.Set;\r\n"}])
}

fn import_preview(app: &mut CedarApp) {
    let request = dispatch(app, FeatureKind::OrganizeJavaImports);
    app.apply_language_feature(request, import_edits());
    assert!(app.language.features.preview.is_some());
    assert!(app.language.view == View::Imports);
}

#[test]
fn imports_require_trust_ready_typed_java_agent_session_flag_and_current_java_document() {
    for missing in 0..10 {
        let mut app = java_app();
        match missing {
            0 => app.state = crate::ConnectionState::Disconnected,
            1 => app.language.running = false,
            2 => app.language.mode = ServerMode::Generic,
            3 => app.language.java_organize_imports_supported = false,
            4 => app
                .agent_info
                .as_mut()
                .unwrap()
                .capabilities
                .retain(|name| name != "language_organize_java_imports"),
            5 => app.active_form.as_mut().unwrap().allow_run = false,
            6 => app.documents[0].path = "notes.txt".into(),
            7 => app.active_document = None,
            8 => app.language.diagnostics_exited = true,
            9 => app.close_after_language_stop = true,
            _ => unreachable!(),
        }
        // Generic Java plus an advertised command must never impersonate typed Java.
        app.language.language_id = "java".into();
        app.language.capabilities =
            json!({"executeCommandProvider":{"commands":["java.edit.organizeImports"]}});
        app.request_language_navigation_feature(FeatureKind::OrganizeJavaImports);
        assert!(
            app.language.features.intent.is_none(),
            "missing prerequisite {missing}"
        );
        assert!(app.next_language_feature_step().is_none());
        assert!(app.error.is_some());
        assert_eq!(app.documents[0].text, JAVA_BEFORE);
    }
}

#[test]
fn imports_metadata_is_an_explicit_agent_owned_boolean_for_this_typed_session() {
    for (mode, metadata, expected) in [
        (ServerMode::Java, json!(true), true),
        (ServerMode::Java, json!(false), false),
        (ServerMode::Java, Value::Null, false),
        (ServerMode::Java, json!({}), false),
        (ServerMode::Java, json!("true"), false),
        (ServerMode::Generic, json!(true), false),
    ] {
        let mut app = java_app();
        app.language.mode = mode;
        app.apply_language_action(Action { session: app.language.session, kind: ActionKind::Start }, json!({"initialize":{"capabilities":{"executeCommandProvider":{"commands":["java.edit.organizeImports"]}},"cedar_java_organize_imports":metadata}}));
        assert_eq!(app.language.java_organize_imports_supported, expected);
        app.language.reset();
        assert!(!app.language.java_organize_imports_supported);
    }
    let mut app = java_app();
    app.apply_language_action(
        Action {
            session: app.language.session,
            kind: ActionKind::Start,
        },
        json!({"initialize":{"capabilities":{"cedar_java_organize_imports":true}}}),
    );
    assert!(
        !app.language.java_organize_imports_supported,
        "a raw server capability cannot substitute for agent metadata"
    );
}

#[test]
fn imports_sync_exact_unsaved_source_then_send_only_typed_path_and_acknowledged_version() {
    let mut app = java_app();
    app.documents[0].text.push_str("// unsaved 🦀\r\n");
    app.documents[0].edit_version += 1;
    app.request_language_navigation_feature(FeatureKind::OrganizeJavaImports);
    assert!(matches!(
        app.next_language_feature_step(),
        Some(FeatureStep::Sync(1))
    ));
    assert!(app.java_imports_operation_problem("Demo.java", 2).is_some());
    acknowledge(&mut app, 1, 9);
    let Some(FeatureStep::Dispatch(request, operation)) = app.next_language_feature_step() else {
        panic!("expected import request")
    };
    assert_eq!(request.source.text, app.documents[0].text);
    assert_eq!(request.source.lsp_version, Some(9));
    assert!(
        matches!(operation, Operation::LanguageOrganizeJavaImports { ref path, version: 9 } if path == "Demo.java")
    );
    assert!(app.operation_problem(&operation).is_none());
    assert!(app
        .java_imports_operation_problem("Other.java", 9)
        .is_some());
    assert!(app.java_imports_operation_problem("Demo.java", 8).is_some());
    assert!(app.java_imports_operation_problem("Demo.java", 0).is_some());
    app.apply_language_feature(*request, import_edits());
    assert!(app.language.features.preview.is_some());
    assert_eq!(app.documents[0].saved_text, JAVA_BEFORE);
}

fn invalidate_imports(app: &mut CedarApp, change: usize) {
    match change {
        0 => app.documents[0].text.push('x'),
        1 => app.documents[0].edit_version += 1,
        2 => {
            app.navigation_changed();
            app.active_document = None;
            app.active_document = Some(1);
        }
        3 => app.language.features.cancel_pending(),
        4 => app.generation += 1,
        5 => app.language.session += 1,
        6 => app.request_language_navigation_feature(FeatureKind::Outline),
        7 => {
            app.documents[0].id = 2;
            app.active_document = Some(2);
        }
        8 => app.documents[0].path = "Renamed.java".into(),
        9 => app.language.sync.opened.get_mut(&1).unwrap().version += 1,
        10 => app.language.sync.opened.get_mut(&1).unwrap().edit_version += 1,
        11 => app.active_form.as_mut().unwrap().allow_run = false,
        12 => app.language.java_organize_imports_supported = false,
        13 => app.language.reset(),
        14 => app.apply_language_events(&json!({"events":[{"type":"closed"}],"truncated":false})),
        _ => unreachable!(),
    }
}

#[test]
fn imports_reject_stale_results_and_apply_after_every_document_session_and_request_change() {
    for after_preview in [false, true] {
        for change in 0..15 {
            let mut app = java_app();
            let request = dispatch(&mut app, FeatureKind::OrganizeJavaImports);
            if after_preview {
                app.apply_language_feature(request.clone(), import_edits());
            }
            invalidate_imports(&mut app, change);
            let text = app.documents[0].text.clone();
            let version = app.documents[0].edit_version;
            if !after_preview {
                app.apply_language_feature(request, import_edits());
            }
            app.apply_format_preview();
            assert_eq!(
                app.documents[0].text, text,
                "change={change}, preview={after_preview}"
            );
            assert_eq!(app.documents[0].edit_version, version);
            assert!(app.language.features.preview.is_none());
            assert!(!app.documents[0].undo_initialized);
        }
    }
}

#[test]
fn imports_empty_or_identical_result_is_not_correctness_evidence_and_has_no_history() {
    let same = json!([{"range":{"start":{"line":2,"character":0},"end":{"line":5,"character":0}},"newText":"import java.util.Set;\r\nimport java.util.List;\r\nimport java.util.Map;\r\n"}]);
    for value in [json!([]), same] {
        let mut app = java_app();
        let request = dispatch(&mut app, FeatureKind::OrganizeJavaImports);
        app.apply_language_feature(request, value);
        app.apply_format_preview();
        assert!(app.language.features.preview.is_none());
        assert!(app.notice.contains("unresolved types may remain"));
        assert_eq!(app.documents[0].edit_version, 0);
        assert!(!app.documents[0].undo_initialized);
        assert!(!app.documents[0].dirty());
    }
}

#[test]
fn imports_reject_entire_malformed_or_overlapping_or_oversize_or_bad_utf16_response() {
    let valid = import_edits()[0].clone();
    let bad = |start_line, start_char, end_line, end_char| json!({"range":{"start":{"line":start_line,"character":start_char},"end":{"line":end_line,"character":end_char}},"newText":"unsafe"});
    let mut annotated = valid.clone();
    annotated["annotationId"] = json!("hidden");
    let mut oversized = valid.clone();
    oversized["newText"] = json!("x".repeat(MAX_FILE_BYTES + 1));
    for value in [
        Value::Null,
        json!({"changes":{"file:///workspace/Demo.java":[valid.clone()]}}),
        json!([valid.clone(), annotated]),
        json!([valid.clone(), bad(0, 4, 0, 5)]), // Inside the bear's surrogate pair.
        json!([valid.clone(), bad(0, 99, 1, 0)]), // Past CRLF line end.
        json!([valid.clone(), bad(999, 0, 999, 1)]),
        json!([valid.clone(), bad(3, 0, 3, 1)]), // Overlaps the import block.
        json!([valid.clone(), bad(5, 0, 5, 0)]), // Insertion at another edit's boundary.
        json!([valid.clone(), oversized]),
        Value::Array(vec![valid; text_edits::MAX_TEXT_EDITS + 1]),
    ] {
        let mut app = java_app();
        let request = dispatch(&mut app, FeatureKind::OrganizeJavaImports);
        app.apply_language_feature(request, value);
        assert!(app.error.is_some());
        assert!(app.language.features.preview.is_none());
        assert_eq!(app.documents[0].text, JAVA_BEFORE);
        assert_eq!(app.documents[0].edit_version, 0);
        assert!(!app.documents[0].undo_initialized);
    }
}

fn imports_frame(
    app: &mut CedarApp,
    time: f64,
    size: [f32; 2],
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let ctx = app.editor_ctx.clone();
    let mut frame = eframe::Frame::_new_kittest();
    ctx.run(
        egui::RawInput {
            time: Some(time),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(size[0], size[1]),
            )),
            events,
            ..Default::default()
        },
        |ctx| eframe::App::update(app, ctx, &mut frame),
    )
}

fn label_position(output: &egui::FullOutput, label: &str) -> Option<egui::Pos2> {
    fn find(shape: &egui::epaint::Shape, label: &str) -> Option<egui::Pos2> {
        match shape {
            egui::epaint::Shape::Text(text) if text.galley.job.text == label => {
                Some(text.visual_bounding_rect().center())
            }
            egui::epaint::Shape::Vec(shapes) => shapes.iter().find_map(|shape| find(shape, label)),
            _ => None,
        }
    }
    output
        .shapes
        .iter()
        .find_map(|shape| find(&shape.shape, label))
}

fn click_label(app: &mut CedarApp, time: f64, size: [f32; 2], label: &str) -> Result<(), String> {
    // One settle frame lets an egui window finish its first size/position pass.
    imports_frame(app, time, size, vec![]);
    let output = imports_frame(app, time + 0.01, size, vec![]);
    let at = label_position(&output, label).ok_or_else(|| format!("Missing UI label: {label}"))?;
    for (index, pressed) in [true, false].into_iter().enumerate() {
        imports_frame(
            app,
            time + 0.02 + index as f64 * 0.01,
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
    Ok(())
}

#[test]
fn actual_frames_import_action_dispatches_once_and_unsupported_action_is_inert() {
    for enabled in [false, true] {
        let mut app = java_app();
        app.language.java_organize_imports_supported = enabled;
        app.tools_open = true;
        app.tool = crate::Tool::Language;
        let (worker, commands) = crate::worker::Worker::recording();
        app.worker = Some(worker);
        click_label(&mut app, 0.0, [1320.0, 880.0], "Organize imports").unwrap();
        if enabled {
            let command = commands.try_recv().unwrap();
            assert!(
                matches!(command.op, Operation::LanguageOrganizeJavaImports { ref path, version: 2 } if path == "Demo.java")
            );
            let Some(Job::Language(action)) = app.pending.remove(&command.id) else {
                panic!("missing request")
            };
            app.apply_language_action(action, import_edits());
            assert!(app.language.features.preview.is_some());
        }
        assert!(commands.try_recv().is_err());
        assert_eq!(app.documents[0].text, JAVA_BEFORE);
        assert_eq!(app.documents[0].edit_version, 0);
    }
}

#[test]
fn actual_frames_import_preview_cancel_escape_and_readonly_labels_preserve_draft() {
    for size in [[780.0, 540.0], [1320.0, 880.0]] {
        for escape in [false, true] {
            let mut app = java_app();
            app.find_open = true;
            import_preview(&mut app);
            imports_frame(&mut app, 0.0, size, vec![]);
            let output = imports_frame(&mut app, 0.1, size, vec![]);
            assert!(label_position(&output, "Organize imports preview").is_some());
            assert!(label_position(&output, IMPORTS_DISCLOSURE).is_some());
            if escape {
                imports_frame(
                    &mut app,
                    1.0,
                    size,
                    key_events(egui::Key::Escape, egui::Modifiers::NONE),
                );
                assert!(app.find_open, "preview Escape must not close Find");
            } else {
                click_label(&mut app, 1.0, size, "Cancel").unwrap();
            }
            assert!(app.language.features.preview.is_none());
            assert!(!app.language.features.preview_open);
            assert_eq!(app.notice, "Organize imports cancelled; draft unchanged");
            assert_eq!(app.documents[0].text, JAVA_BEFORE);
            assert_eq!(app.documents[0].saved_text, JAVA_BEFORE);
            assert_eq!(app.documents[0].edit_version, 0);
        }
    }
}

#[test]
fn actual_frames_import_preview_expires_after_typing_or_tab_roundtrip() {
    for tab in [false, true] {
        let mut app = java_app();
        let size = [1320.0, 880.0];
        app.documents.push(Document::new(
            2,
            "Other.java".into(),
            "class Other {}\r\n".into(),
            "other".into(),
        ));
        app.next_document = 3;
        imports_frame(&mut app, 0.0, size, vec![]);
        import_preview(&mut app);
        if tab {
            // The floating preview initially covers the tab strip. Move it
            // using its real title bar before clicking the underlying tabs.
            imports_frame(&mut app, 0.1, size, vec![]);
            let output = imports_frame(&mut app, 0.2, size, vec![]);
            let title = label_position(&output, "Organize imports preview").unwrap();
            let to = title + egui::vec2(0.0, 220.0);
            imports_frame(
                &mut app,
                0.3,
                size,
                vec![
                    egui::Event::PointerMoved(title),
                    egui::Event::PointerButton {
                        pos: title,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
            imports_frame(&mut app, 0.4, size, vec![egui::Event::PointerMoved(to)]);
            imports_frame(
                &mut app,
                0.5,
                size,
                vec![egui::Event::PointerButton {
                    pos: to,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
            let moved = imports_frame(&mut app, 0.6, size, vec![]);
            let moved_title = label_position(&moved, "Organize imports preview").unwrap();
            assert!(moved_title.y > title.y + 100.0, "the real window drag must expose the tabs: before={title:?}, after={moved_title:?}");
            click_label(&mut app, 1.0, size, "Other.java").unwrap();
            assert_eq!(app.active_document, Some(2));
            click_label(&mut app, 2.0, size, "Demo.java").unwrap();
            assert_eq!(app.active_document, Some(1));
        } else {
            // A preview is an inert review window. A still-focused editor may
            // receive input, which must invalidate rather than rebase the edit.
            imports_frame(&mut app, 1.0, size, vec![egui::Event::Text("x".into())]);
            assert_ne!(app.documents[0].text, JAVA_BEFORE);
        }
        let text = app.documents[0].text.clone();
        let version = app.documents[0].edit_version;
        app.apply_format_preview();
        assert!(app.language.features.preview.is_none());
        assert_eq!(app.documents[0].text, text);
        assert_eq!(app.documents[0].edit_version, version);
    }
}

#[test]
fn actual_frames_import_noop_keeps_existing_edit_history_and_unsaved_version() {
    for identical in [false, true] {
        let mut app = java_app();
        let size = [1320.0, 880.0];
        imports_frame(&mut app, 0.0, size, vec![]);
        imports_frame(&mut app, 1.0, size, vec![egui::Event::Text("x".into())]);
        let draft = app.documents[0].text.clone();
        assert_ne!(draft, JAVA_BEFORE);
        let version = app.documents[0].edit_version;
        acknowledge(&mut app, 1, 3);
        let request = dispatch(&mut app, FeatureKind::OrganizeJavaImports);
        let value = if identical {
            json!([{"range":{"start":{"line":2,"character":0},"end":{"line":5,"character":0}},"newText":"import java.util.Set;\r\nimport java.util.List;\r\nimport java.util.Map;\r\n"}])
        } else {
            json!([])
        };
        app.apply_language_feature(request, value);
        imports_frame(&mut app, 2.0, size, vec![]);
        assert!(app.language.features.preview.is_none());
        assert_eq!(app.documents[0].text, draft);
        assert_eq!(app.documents[0].edit_version, version);
        imports_frame(
            &mut app,
            2.1,
            size,
            key_events(egui::Key::Z, egui::Modifiers::COMMAND),
        );
        assert_eq!(
            app.documents[0].text, JAVA_BEFORE,
            "one Undo still undoes typing"
        );
        imports_frame(
            &mut app,
            2.2,
            size,
            key_events(egui::Key::Y, egui::Modifiers::COMMAND),
        );
        assert_eq!(app.documents[0].text, draft);
    }
}

#[test]
fn actual_frames_import_apply_is_atomic_unicode_crlf_undo_redo_and_repeated_apply_is_inert() {
    let disk = tempfile::tempdir().unwrap();
    let file = disk.path().join("Demo.java");
    std::fs::write(&file, "saved disk baseline\r\n").unwrap();
    let mut stages = Vec::new();
    CedarApp::organize_imports_acceptance_transaction(
        "Demo.java",
        "saved disk baseline\r\n",
        JAVA_BEFORE,
        import_edits(),
        |stage, doc| {
            stages.push(stage);
            assert_eq!(
                doc.text,
                if matches!(stage, "apply" | "redo") {
                    JAVA_AFTER
                } else {
                    JAVA_BEFORE
                }
            );
            assert_eq!(doc.saved_text, "saved disk baseline\r\n");
            assert_eq!(doc.revision.as_deref(), Some("r0"));
            assert!(doc.dirty());
            assert_eq!(
                std::fs::read_to_string(&file).unwrap(),
                "saved disk baseline\r\n"
            );
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(stages, ["preview", "cancel", "apply", "undo", "redo"]);
}

impl CedarApp {
    /// Test-only bridge for a real JDT result. It never owns a live connection;
    /// the acceptance caller observes each editor state and verifies disk/LSP.
    pub(crate) fn organize_imports_acceptance_transaction(
        path: &str,
        saved: &str,
        draft: &str,
        value: Value,
        mut observe: impl FnMut(&'static str, &Document) -> Result<(), String>,
    ) -> Result<(), String> {
        let mut app = java_app();
        app.documents[0] = Document::new(1, path.into(), saved.into(), "r0".into());
        app.documents[0].text = draft.into();
        app.documents[0].edit_version = 1;
        app.language.sync.clear();
        acknowledge(&mut app, 1, 2);
        let size = [1320.0, 880.0];
        imports_frame(&mut app, 0.0, size, vec![]);
        let request = dispatch(&mut app, FeatureKind::OrganizeJavaImports);
        app.apply_language_feature(request, value.clone());
        let after = app
            .language
            .features
            .preview
            .as_ref()
            .ok_or("No import preview was produced")?
            .after
            .clone();
        imports_frame(&mut app, 0.1, size, vec![]);
        observe("preview", &app.documents[0])?;
        click_label(&mut app, 1.0, size, "Cancel")?;
        if app.language.features.preview.is_some() {
            return Err("Cancel did not dismiss the import preview".into());
        }
        observe("cancel", &app.documents[0])?;
        let request = dispatch(&mut app, FeatureKind::OrganizeJavaImports);
        app.apply_language_feature(request, value);
        click_label(&mut app, 2.0, size, "Apply to draft")?;
        if app.documents[0].text != after || app.documents[0].edit_version != 2 {
            return Err("Apply did not commit exactly one import transaction".into());
        }
        app.apply_format_preview();
        if app.documents[0].edit_version != 2 {
            return Err("Repeated Apply mutated the document".into());
        }
        observe("apply", &app.documents[0])?;
        imports_frame(&mut app, 3.0, size, vec![]);
        imports_frame(
            &mut app,
            3.1,
            size,
            key_events(egui::Key::Z, egui::Modifiers::COMMAND),
        );
        if app.documents[0].text != draft || app.documents[0].edit_version != 3 {
            return Err("One Undo did not restore the exact unsaved source".into());
        }
        observe("undo", &app.documents[0])?;
        imports_frame(
            &mut app,
            3.2,
            size,
            key_events(egui::Key::Y, egui::Modifiers::COMMAND),
        );
        if app.documents[0].text != after || app.documents[0].edit_version != 4 {
            return Err("One Redo did not restore the exact organized draft".into());
        }
        observe("redo", &app.documents[0])?;
        Ok(())
    }
}
