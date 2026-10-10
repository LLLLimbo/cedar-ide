use super::*;
use crate::{
    agent_support::full_test_agent,
    editor_state,
    model::Document,
    worker::{Command, Event, Worker},
    ConnectForm, ConnectionState,
};
use cedar_protocol::{
    MavenDependencyDeclaration, MavenDependencyUnavailableReason, MavenObservedLibrary,
    JAVA_STARTUP_CAPABILITIES,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::sync::mpsc::Receiver;

fn hash() -> String {
    "a".repeat(64)
}
fn app() -> (CedarApp, Receiver<Command>) {
    let mut app = CedarApp::empty();
    app.state = ConnectionState::Ready;
    app.open_form = false;
    app.generation = 9;
    app.root = r"D:\workspace".into();
    let form = ConnectForm {
        local_root: app.root.clone(),
        allow_run: true,
        ..Default::default()
    };
    app.workspace_key = Some(form.key());
    app.active_form = Some(form);
    let mut info = full_test_agent();
    info.os = "windows".into();
    info.capabilities.extend(
        [
            "language_start_java",
            "language_start_java_maven_begin",
            "language_maven_model",
            JAVA_MAVEN_DEPENDENCIES_CAPABILITY,
        ]
        .into_iter()
        .map(str::to_owned),
    );
    info.capabilities
        .extend(JAVA_STARTUP_CAPABILITIES.iter().map(|name| (*name).into()));
    app.agent_info = Some(info);
    app.language.mode = ServerMode::Java;
    app.language.java.executable = r"C:\Java\bin\java.exe".into();
    app.language.java.distribution = r"D:\jdt".into();
    app.language.java.data_directory = r"D:\data".into();
    app.language.maven.enabled = true;
    app.language.maven.local_repository = r"D:\repository".into();
    let (worker, rx) = Worker::recording();
    app.worker = Some(worker);
    (app, rx)
}
fn reply(app: &mut CedarApp, command: Command, result: Result<Payload, String>) {
    app.apply_event(Event {
        generation: app.generation,
        id: command.id,
        connected: true,
        result,
    });
}
fn tick(app: &mut CedarApp, time: f64) {
    let ctx = app.editor_ctx.clone();
    let _ = ctx.run(
        egui::RawInput {
            time: Some(time),
            ..Default::default()
        },
        |ctx| app.language_tick(ctx),
    );
}
fn start(app: &mut CedarApp, rx: &Receiver<Command>, id: u64, hash: &str) {
    app.start_language();
    let begin = rx.try_recv().expect("explicit Maven startup");
    assert!(matches!(
        begin.op,
        Operation::LanguageStartJavaMavenBegin { .. }
    ));
    reply(
        app,
        begin,
        Ok(Payload::Language {
            value: json!({"state":"starting","startup_id":id,"process_id":null}),
        }),
    );
    tick(app, 0.3);
    let poll = rx.try_recv().unwrap();
    assert!(matches!(poll.op, Operation::LanguageStartJavaPoll { startup_id } if startup_id == id));
    reply(
        app,
        poll,
        Ok(Payload::Language {
            value: json!({"state":"ready","startup_id":id,"language":{"started":true,"initialize":{"capabilities":{"executeCommandProvider":{"commands":["java.project.getSettings"]}},"cedar_java_profile":"maven_leaf","cedar_java_maven_model":true,"cedar_java_maven_pom_sha256":hash}}}),
        }),
    );
    assert_eq!(app.language.running_startup_id, Some(id));
    assert!(app.language.running);
    app.language.automatic = false;
    assert!(rx.try_recv().is_err());
}
fn snapshot() -> MavenDependenciesSnapshot {
    let declaration = MavenDependencyDeclaration {
        group_id: "dev.cedar".into(),
        artifact_id: "library".into(),
        version: "1.0".into(),
        classifier: None,
        scope: MavenDependencyScope::Compile,
        scope_explicit: false,
        optional: false,
        optional_explicit: false,
        expected_jar_path: "dev/cedar/library/1.0/library-1.0.jar".into(),
        regular_file_present: true,
    };
    MavenDependenciesSnapshot {
        schema: 1,
        profile: "maven_leaf".into(),
        startup_id: 42,
        pom_path: "pom.xml".into(),
        pom_sha256: hash(),
        declarations: vec![declaration.clone()],
        observation: MavenDependencyObservation::Available {
            libraries: vec![MavenObservedLibrary {
                root: MavenLibraryRoot::LocalRepository,
                relative_path: declaration.expected_jar_path,
                regular_file_present: true,
                declaration_indices: vec![0],
            }],
        },
    }
}
fn inspect(app: &mut CedarApp, rx: &Receiver<Command>) -> Command {
    app.inspect_maven_dependencies();
    let command = rx.try_recv().expect("one dependency request");
    assert!(
        matches!(&command.op, Operation::LanguageMavenDependencies { startup_id, pom_sha256 } if Some(*startup_id) == app.language.running_startup_id && Some(pom_sha256.as_str()) == app.language.maven_model.pom_sha256())
    );
    command
}
fn frame(
    app: &mut CedarApp,
    time: f64,
    controls: bool,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let ctx = app.editor_ctx.clone();
    ctx.run(
        egui::RawInput {
            time: Some(time),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1300.0, 900.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                if controls {
                    app.maven_dependencies_controls(ui);
                } else {
                    app.maven_dependencies_view(ui);
                }
            });
        },
    )
}
fn editor_frame(app: &mut CedarApp, time: f64) -> egui::FullOutput {
    let ctx = app.editor_ctx.clone();
    ctx.run(
        egui::RawInput {
            time: Some(time),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1600.0, 900.0),
            )),
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.columns(2, |columns| {
                    app.editor(&mut columns[0]);
                    app.maven_dependencies_view(&mut columns[1]);
                });
            });
        },
    )
}
fn current_selection(ctx: &egui::Context, document: u64) -> String {
    // Debug includes both primary/secondary indices and wrapped-row affinity,
    // whereas CCursor's PartialEq intentionally omits affinity.
    format!(
        "{:?}",
        egui::TextEdit::load_state(ctx, egui::Id::new(("editor", document)))
            .and_then(|state| state.cursor.char_range())
    )
}
fn shapes(output: &egui::FullOutput) -> Vec<&egui::epaint::TextShape> {
    fn collect<'a>(shape: &'a egui::epaint::Shape, out: &mut Vec<&'a egui::epaint::TextShape>) {
        match shape {
            egui::epaint::Shape::Text(text) => out.push(text),
            egui::epaint::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect(shape, out);
                }
            }
            _ => {}
        }
    }
    let mut out = vec![];
    for shape in &output.shapes {
        collect(&shape.shape, &mut out);
    }
    out
}
fn rendered(output: &egui::FullOutput) -> String {
    shapes(output)
        .iter()
        .map(|shape| shape.galley.job.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}
fn apply_snapshot(app: &mut CedarApp, command: Command, snapshot: MavenDependenciesSnapshot) {
    reply(app, command, Ok(Payload::MavenDependencies { snapshot }));
}

fn grouped_app(dependencies: bool) -> (CedarApp, Receiver<Command>) {
    let (mut app, rx) = app();
    let info = app.agent_info.as_mut().unwrap();
    info.capabilities.retain(|name| {
        !matches!(
            name.as_str(),
            "language_start_java_maven_begin"
                | "language_maven_model"
                | "language_maven_dependencies"
        )
    });
    info.capability_groups = vec!["java_maven_leaf_v1".into()];
    if dependencies {
        info.capability_groups
            .push("java_maven_dependencies_v1".into());
    }
    info.validate().unwrap();
    (app, rx)
}

#[test]
fn grouped_maven_claims_require_explicit_start_and_optional_dependency_inspection() {
    for dependencies in [false, true] {
        let (mut app, rx) = grouped_app(dependencies);
        app.inspect_maven_dependencies();
        assert!(rx.try_recv().is_err(), "a group must not launch Java");
        assert!(app
            .operation_problem(&Operation::LanguageMavenModel)
            .is_some());

        start(&mut app, &rx, 42, &hash());
        assert!(app
            .operation_problem(&Operation::LanguageMavenModel)
            .is_none());
        assert_eq!(
            app.operation_problem(&Operation::LanguageMavenDependencies {
                startup_id: 42,
                pom_sha256: hash(),
            })
            .is_none(),
            dependencies
        );
        app.inspect_maven_dependencies();
        if dependencies {
            let command = rx
                .try_recv()
                .expect("explicit grouped dependency inspection");
            assert!(matches!(
                &command.op,
                Operation::LanguageMavenDependencies { startup_id: 42, pom_sha256 }
                    if pom_sha256 == &hash()
            ));
            apply_snapshot(&mut app, command, snapshot());
            assert!(app.language.maven_dependencies.snapshot.is_some());
        } else {
            assert!(app.language.maven_dependencies.snapshot.is_none());
        }
        assert!(rx.try_recv().is_err());
        tick(&mut app, 60.0);
        tick(&mut app, 600.0);
        assert!(
            rx.try_recv().is_err(),
            "groups must not add automatic requests"
        );
    }
}

#[test]
fn grouped_dependency_dispatch_preserves_trust_ready_session_and_pom_guards() {
    for gate in [
        "connection",
        "trust",
        "running",
        "startup",
        "profile",
        "provider",
        "closing",
        "closed",
        "changed_pom",
        "session",
        "leaf_group",
        "dependency_version",
        "request_startup",
        "request_pom",
    ] {
        let (mut app, rx) = grouped_app(true);
        start(&mut app, &rx, 42, &hash());
        let mut document = Document::new(1, "Main.java".into(), "disk".into(), "rev".into());
        document.text = "unsaved draft".into();
        app.documents.push(document);
        let mut startup_id = 42;
        let mut pom_sha256 = hash();
        match gate {
            "connection" => app.state = ConnectionState::Disconnected,
            "trust" => {
                app.active_form.as_mut().unwrap().allow_run = false;
                app.form.allow_run = true;
            }
            "running" => app.language.running = false,
            "startup" => app.language.running_startup_id = None,
            "profile" => app.language.mode = ServerMode::Generic,
            "provider" => {
                app.language.capabilities["executeCommandProvider"]["commands"] =
                    json!(["java.project.getSettings.other"]);
            }
            "closing" => app.close_after_language_stop = true,
            "closed" => app.language.diagnostics_exited = true,
            "changed_pom" => {
                app.observe_maven_pom_acknowledgement(app.next_request, "pom.xml", &"b".repeat(64));
            }
            "session" => app.language.reset(),
            "leaf_group" => app
                .agent_info
                .as_mut()
                .unwrap()
                .capability_groups
                .retain(|group| group != "java_maven_leaf_v1"),
            "dependency_version" => {
                app.agent_info.as_mut().unwrap().capability_groups = vec![
                    "java_maven_leaf_v1".into(),
                    "java_maven_dependencies_v2".into(),
                ];
            }
            "request_startup" => startup_id = 43,
            "request_pom" => pom_sha256 = "b".repeat(64),
            _ => unreachable!(),
        }
        let operation = Operation::LanguageMavenDependencies {
            startup_id,
            pom_sha256,
        };
        assert!(app.operation_problem(&operation).is_some(), "{gate}");
        let next = app.next_request;
        assert_eq!(app.request(operation, crate::Job::Git), 0, "{gate}");
        assert_eq!(app.next_request, next, "{gate}");
        assert!(rx.try_recv().is_err(), "{gate} must block dispatch");
        assert_eq!(app.documents[0].text, "unsaved draft", "{gate}");
        assert_eq!(app.documents[0].revision.as_deref(), Some("rev"), "{gate}");
    }
}

#[test]
fn grouped_claims_cannot_replace_the_maven_ready_witness() {
    for missing in [
        "cedar_java_profile",
        "cedar_java_maven_model",
        "cedar_java_maven_pom_sha256",
    ] {
        let (mut app, rx) = grouped_app(true);
        app.start_language();
        let begin = rx.try_recv().unwrap();
        assert!(matches!(
            begin.op,
            Operation::LanguageStartJavaMavenBegin { .. }
        ));
        reply(
            &mut app,
            begin,
            Ok(Payload::Language {
                value: json!({"state":"starting","startup_id":42,"process_id":null}),
            }),
        );
        tick(&mut app, 0.3);
        let poll = rx.try_recv().unwrap();
        assert!(matches!(
            poll.op,
            Operation::LanguageStartJavaPoll { startup_id: 42 }
        ));
        let mut value = json!({"state":"ready","startup_id":42,"language":{"started":true,"initialize":{"capabilities":{"executeCommandProvider":{"commands":["java.project.getSettings"]}},"cedar_java_profile":"maven_leaf","cedar_java_maven_model":true,"cedar_java_maven_pom_sha256":hash()}}});
        value["language"]["initialize"]
            .as_object_mut()
            .unwrap()
            .remove(missing);
        reply(&mut app, poll, Ok(Payload::Language { value }));
        assert!(!app.language.running, "missing {missing}");
        assert!(app.language.restart_blocked, "missing {missing}");
        app.inspect_maven_dependencies();
        assert!(app
            .operation_problem(&Operation::LanguageMavenModel)
            .is_some());
        assert!(
            rx.try_recv().is_err(),
            "missing {missing} must not dispatch"
        );
    }
}

#[test]
fn dependency_inspection_requires_optional_capability_trust_provider_and_verified_ready() {
    for missing in [
        "capability",
        "trust",
        "provider",
        "startup",
        "profile",
        "mandatory",
        "restart",
        "closed",
    ] {
        let (mut app, rx) = app();
        start(&mut app, &rx, 42, &hash());
        match missing {
            "capability" => app
                .agent_info
                .as_mut()
                .unwrap()
                .capabilities
                .retain(|name| name != JAVA_MAVEN_DEPENDENCIES_CAPABILITY),
            "trust" => app.active_form.as_mut().unwrap().allow_run = false,
            "provider" => {
                app.language.capabilities["executeCommandProvider"]["commands"] =
                    json!(["java.project.getSettings.other"])
            }
            "startup" => app.language.running_startup_id = None,
            "profile" => app.language.mode = ServerMode::Generic,
            "mandatory" => app
                .agent_info
                .as_mut()
                .unwrap()
                .capabilities
                .retain(|name| name != "language_maven_model"),
            "restart" => app.language.maven_model.require_restart(),
            "closed" => app.language.diagnostics_exited = true,
            _ => unreachable!(),
        }
        app.inspect_maven_dependencies();
        assert!(rx.try_recv().is_err(), "{missing} must not dispatch");
        assert!(app
            .operation_problem(&Operation::LanguageMavenDependencies {
                startup_id: 42,
                pom_sha256: hash()
            })
            .is_some());
        if missing == "capability" {
            assert!(
                app.backend_java_maven_supported(),
                "optional capability cannot disable existing Maven profile"
            );
        }
    }
    let (mut app, rx) = app();
    app.inspect_maven_dependencies();
    assert!(rx.try_recv().is_err(), "inspection must not launch Java");
}

#[test]
fn dependency_button_dispatches_once_and_never_polls_saves_or_changes_tools() {
    let (mut app, rx) = app();
    start(&mut app, &rx, 42, &hash());
    app.tools_open = false;
    app.run_state.output = "existing build output".into();
    let previous_tool = app.tool;
    frame(&mut app, 1.0, true, vec![]);
    let output = frame(&mut app, 1.1, true, vec![]);
    let at = shapes(&output)
        .into_iter()
        .find(|text| text.galley.job.text == "Inspect dependencies")
        .unwrap()
        .visual_bounding_rect()
        .center();
    for (index, pressed) in [true, false].into_iter().enumerate() {
        frame(
            &mut app,
            1.2 + index as f64 / 100.0,
            true,
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
    let command = rx.try_recv().unwrap();
    assert!(matches!(
        command.op,
        Operation::LanguageMavenDependencies { startup_id: 42, .. }
    ));
    app.inspect_maven_dependencies();
    assert!(rx.try_recv().is_err());
    apply_snapshot(&mut app, command, snapshot());
    tick(&mut app, 60.0);
    tick(&mut app, 600.0);
    assert!(rx.try_recv().is_err());
    assert_eq!(app.run_state.output, "existing build output");
    assert!(app.tool == previous_tool);
    assert!(!app.tools_open);
    assert!(!app.language.automatic);
}

#[test]
fn dependency_request_and_reply_require_every_identity_and_current_sequence() {
    for mismatch in [
        "request",
        "action_session",
        "session",
        "generation",
        "startup",
        "pom",
        "sequence",
        "trust",
        "provider",
    ] {
        let (mut app, rx) = app();
        start(&mut app, &rx, 42, &hash());
        let command = inspect(&mut app, &rx);
        let crate::Job::Language(mut action) = app.pending.remove(&command.id).unwrap() else {
            unreachable!()
        };
        let mut request = command.id;
        match mismatch {
            "request" => request += 1,
            "action_session" => action.session += 1,
            "session" => app.language.session += 1,
            "generation" => app.generation += 1,
            "startup" => app.language.running_startup_id = Some(43),
            "pom" => {
                app.observe_maven_pom_acknowledgement(command.id + 1, "pom.xml", &"b".repeat(64))
            }
            "sequence" => app.language.maven_dependencies.sequence += 1,
            "trust" => app.active_form.as_mut().unwrap().allow_run = false,
            "provider" => app.language.capabilities = json!({}),
            _ => unreachable!(),
        }
        app.language.output = "newer output".into();
        app.error = Some("newer error".into());
        app.apply_maven_dependencies_event(
            request,
            action,
            Ok(Payload::MavenDependencies {
                snapshot: snapshot(),
            }),
            true,
        );
        assert!(
            app.language.maven_dependencies.snapshot.is_none(),
            "{mismatch}"
        );
        assert_eq!(app.language.output, "newer output");
        assert_eq!(app.error.as_deref(), Some("newer error"));
    }
    let (mut app, rx) = app();
    start(&mut app, &rx, 42, &hash());
    let old = inspect(&mut app, &rx);
    let crate::Job::Language(old_action) = app.pending.remove(&old.id).unwrap() else {
        unreachable!()
    };
    app.language.maven_dependencies.reset();
    let newer = inspect(&mut app, &rx);
    apply_snapshot(&mut app, newer, snapshot());
    app.apply_maven_dependencies_event(
        old.id,
        old_action,
        Err("language_maven_restart_required: stale".into()),
        true,
    );
    assert!(app.language.maven_dependencies.snapshot.is_some());
    assert!(!app.language.maven_model.restart_required());
}

#[test]
fn dependency_snapshot_invalidates_on_stop_restart_disconnect_pom_and_trust() {
    for change in [
        "stop",
        "reset",
        "disconnect",
        "pom",
        "trust",
        "generation",
        "startup",
        "closed",
    ] {
        let (mut app, rx) = app();
        start(&mut app, &rx, 42, &hash());
        let command = inspect(&mut app, &rx);
        apply_snapshot(&mut app, command, snapshot());
        assert!(app.language.maven_dependencies.snapshot.is_some());
        match change {
            "stop" => app.stop_language(),
            "reset" => app.language.reset(),
            "disconnect" => app.disconnected("test disconnect".into()),
            "pom" => {
                app.observe_maven_pom_acknowledgement(app.next_request, "pom.xml", &"b".repeat(64))
            }
            "trust" => app.active_form.as_mut().unwrap().allow_run = false,
            "generation" => app.generation += 1,
            "startup" => app.language.running_startup_id = Some(99),
            "closed" => app.apply_language_events(&json!({"events":[{"type":"closed"}]})),
            _ => unreachable!(),
        }
        app.invalidate_maven_dependencies();
        assert!(
            app.language.maven_dependencies.snapshot.is_none(),
            "{change}"
        );
        assert!(
            app.language.maven_dependencies.pending.is_none(),
            "{change}"
        );
    }
}

#[test]
fn malformed_oversized_or_wrong_typed_dependency_payload_is_unknown() {
    let mut bad_snapshots = vec![];
    for field in [
        "schema",
        "startup",
        "pom",
        "hash",
        "scope",
        "path",
        "association",
        "rows",
        "bytes",
        "aggregate",
    ] {
        let mut bad = snapshot();
        match field {
            "schema" => bad.schema = 2,
            "startup" => bad.startup_id += 1,
            "pom" => bad.pom_path = "other/pom.xml".into(),
            "hash" => bad.pom_sha256 = "b".repeat(64),
            "scope" => bad.declarations[0].scope = MavenDependencyScope::Test,
            "path" => bad.declarations[0].expected_jar_path = "../secret.jar".into(),
            "association" => {
                if let MavenDependencyObservation::Available { libraries } = &mut bad.observation {
                    libraries[0].declaration_indices = vec![1];
                }
            }
            "rows" => bad.declarations = vec![bad.declarations[0].clone(); 257],
            "bytes" => {
                if let MavenDependencyObservation::Available { libraries } = &mut bad.observation {
                    libraries[0].relative_path = "x".repeat(4097);
                }
            }
            "aggregate" => {
                bad.observation = MavenDependencyObservation::Available {
                    libraries: (0..40)
                        .map(|index| MavenObservedLibrary {
                            root: MavenLibraryRoot::Workspace,
                            relative_path: format!("lib/{index}/{}.jar", "x".repeat(4000)),
                            regular_file_present: true,
                            declaration_indices: vec![],
                        })
                        .collect(),
                }
            }
            _ => unreachable!(),
        }
        bad_snapshots.push(Ok(Payload::MavenDependencies { snapshot: bad }));
    }
    bad_snapshots.extend([
        Ok(Payload::Language {
            value: json!({"status":"available","libraries":[]}),
        }),
        Err("private backend path C:\\secret".into()),
    ]);
    for result in bad_snapshots {
        let (mut app, rx) = app();
        start(&mut app, &rx, 42, &hash());
        let command = inspect(&mut app, &rx);
        reply(&mut app, command, result);
        assert!(app.language.maven_dependencies.status == Status::Unavailable);
        assert!(app.language.maven_dependencies.snapshot.is_none());
        assert_eq!(app.language.output, UNKNOWN);
        assert!(!app.language.output.contains("secret"));
    }
}

#[test]
fn rendered_dependency_evidence_keeps_declarations_observations_and_unknown_separate() {
    for unavailable in [false, true] {
        let (mut app, rx) = app();
        start(&mut app, &rx, 42, &hash());
        let command = inspect(&mut app, &rx);
        let mut data = snapshot();
        data.declarations[0].classifier = Some("tests".into());
        data.declarations[0].scope = MavenDependencyScope::Test;
        data.declarations[0].scope_explicit = true;
        data.declarations[0].optional = true;
        data.declarations[0].optional_explicit = true;
        data.declarations[0].regular_file_present = false;
        data.declarations[0].expected_jar_path = data.declarations[0].repository_jar_path();
        data.observation = if unavailable {
            MavenDependencyObservation::Unavailable {
                reason: MavenDependencyUnavailableReason::ModelUnavailable,
            }
        } else {
            MavenDependencyObservation::Available {
                libraries: vec![MavenObservedLibrary {
                    root: MavenLibraryRoot::Workspace,
                    relative_path: "lib/unrelated.jar".into(),
                    regular_file_present: true,
                    declaration_indices: vec![],
                }],
            }
        };
        apply_snapshot(&mut app, command, data);
        frame(&mut app, 1.0, false, vec![]);
        let text = rendered(&frame(&mut app, 1.1, false, vec![]));
        for expected in [
            "Captured POM declarations (1)",
            "dev.cedar:library:1.0",
            "classifier: tests",
            "Scope: test (explicit)",
            "optional: true (explicit)",
            "Expected regular file: absent",
            "Observed JDT library rows",
        ] {
            assert!(text.contains(expected), "missing {expected}: {text}");
        }
        if unavailable {
            assert!(text.contains("JDT library membership is unknown"));
            assert!(!text.contains("0 library rows"));
            assert!(!text.contains("No declaration path match"));
        } else {
            assert!(text.contains("Observed library · workspace relative: lib/unrelated.jar"));
            assert!(text.contains("Observed regular file: present"));
            assert!(text.contains("coordinates and scope are unknown"));
        }
        assert!(rx.try_recv().is_err(), "rendering paths is inert");
    }
}

#[test]
fn dependency_lists_remain_scrollable_past_twenty_rows() {
    let (mut app, rx) = app();
    start(&mut app, &rx, 42, &hash());
    let command = inspect(&mut app, &rx);
    let mut data = snapshot();
    data.declarations = (0..256)
        .map(|index| {
            let mut declaration = snapshot().declarations.remove(0);
            declaration.group_id = "g".into();
            declaration.artifact_id = format!("a{index:03}");
            declaration.version = "1".into();
            declaration.expected_jar_path = declaration.repository_jar_path();
            declaration
        })
        .collect();
    data.observation = MavenDependencyObservation::Available {
        libraries: data
            .declarations
            .iter()
            .enumerate()
            .map(|(index, declaration)| MavenObservedLibrary {
                root: MavenLibraryRoot::LocalRepository,
                relative_path: declaration.expected_jar_path.clone(),
                regular_file_present: true,
                declaration_indices: vec![index as u16],
            })
            .collect(),
    };
    assert!(
        data.validate_for(42, &hash(), true).is_ok(),
        "the full bounded lists fit the wire budget"
    );
    apply_snapshot(&mut app, command, data);
    frame(&mut app, 1.0, false, vec![]);
    frame(&mut app, 1.1, false, vec![]);
    let mut text = String::new();
    for index in 0..96 {
        let output = frame(
            &mut app,
            1.2 + index as f64 * 0.2,
            false,
            vec![
                egui::Event::PointerMoved(egui::pos2(400.0, 380.0)),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -900.0),
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        text.push_str(&rendered(&output));
    }
    assert!(
        text.contains("Declaration #256"),
        "last bounded declaration remains accessible by scroll"
    );
    assert!(
        text.contains("Observed library · local repository relative: g/a255/1/a255-1.jar"),
        "last bounded observed row remains accessible by scroll"
    );
    assert!(rx.try_recv().is_err());
}

#[test]
fn dependency_overflow_is_fail_closed_without_a_request() {
    for request in [false, true] {
        let (mut app, rx) = app();
        start(&mut app, &rx, 42, &hash());
        if request {
            app.next_request = u64::MAX;
        } else {
            app.language.maven_dependencies.sequence = u64::MAX;
        }
        app.inspect_maven_dependencies();
        assert!(app.language.maven_dependencies.status == Status::Unavailable);
        assert!(rx.try_recv().is_err());
    }
}

fn document_state(document: &Document) -> String {
    format!("{document:?}")
}
fn history_state(
    ctx: &egui::Context,
    document: &Document,
) -> Result<(egui::text::CCursorRange, String), String> {
    let state = egui::TextEdit::load_state(ctx, egui::Id::new(("editor", document.id)))
        .ok_or("Missing editor history")?;
    let cursor = state
        .cursor
        .char_range()
        .ok_or("Missing editor selection")?;
    // The production shortcut skips selection-only checkpoints. Probe a clone
    // by the same next-text transition rule, without changing native history.
    let mut history = state.undoer();
    let mut current = (cursor, document.text.clone());
    for _ in 0..=editor_state::MAX_UNDO_STATES {
        let previous = history
            .undo(&current)
            .cloned()
            .ok_or("Missing single Undo baseline")?;
        if previous.1 != document.text {
            return Ok(previous);
        }
        current = previous;
    }
    Err("Missing single Undo text transition".into())
}

/// Native acceptance installs its actual dirty Document and native Undo state,
/// then exercises the production request/apply/render/invalidation paths.
pub(crate) fn verify_native_maven_dependencies(
    snapshot: &MavenDependenciesSnapshot,
    startup_id: u64,
    pom_sha256: &str,
    document: &mut Document,
    editor_ctx: &egui::Context,
) -> Result<(), String> {
    verify_native_dependencies_with_agent(
        snapshot, startup_id, pom_sha256, document, editor_ctx, None, None,
    )
}

#[cfg(target_os = "linux")]
pub(crate) fn verify_native_linux_maven_dependencies(
    snapshot: &MavenDependenciesSnapshot,
    startup_id: u64,
    pom_sha256: &str,
    document: &mut Document,
    editor_ctx: &egui::Context,
    agent_info: &cedar_protocol::AgentInfo,
    local_agent_info: &cedar_protocol::AgentInfo,
) -> Result<(), String> {
    agent_info
        .validate()
        .map_err(|_| "Invalid native Linux metadata")?;
    local_agent_info
        .validate()
        .map_err(|_| "Invalid local Linux metadata")?;
    if agent_info.os != "linux"
        || agent_info.capability_groups
            != [
                cedar_protocol::JAVA_MAVEN_DEPENDENCIES_GROUP,
                cedar_protocol::JAVA_MAVEN_LEAF_GROUP,
            ]
        || agent_info.capabilities.iter().any(|name| {
            cedar_protocol::JAVA_MAVEN_CAPABILITIES.contains(&name.as_str())
                || name == JAVA_MAVEN_DEPENDENCIES_CAPABILITY
        })
    {
        return Err("Expected native Linux grouped Maven claims".into());
    }
    if local_agent_info.os != "linux"
        || !local_agent_info.capability_groups.is_empty()
        || local_agent_info.supports("language_start_java_maven_begin")
        || local_agent_info.supports("language_maven_model")
        || local_agent_info.supports(JAVA_MAVEN_DEPENDENCIES_CAPABILITY)
    {
        return Err("Local workspace unexpectedly claims Maven support".into());
    }
    verify_native_dependencies_with_agent(
        snapshot,
        startup_id,
        pom_sha256,
        document,
        editor_ctx,
        Some(agent_info),
        Some(local_agent_info),
    )
}

fn verify_native_dependencies_with_agent(
    snapshot: &MavenDependenciesSnapshot,
    startup_id: u64,
    pom_sha256: &str,
    document: &mut Document,
    editor_ctx: &egui::Context,
    agent_info: Option<&cedar_protocol::AgentInfo>,
    local_agent_info: Option<&cedar_protocol::AgentInfo>,
) -> Result<(), String> {
    // The legacy wrapper installs the Windows fixture. The Linux wrapper
    // supplies its validated native metadata and uses case-sensitive paths.
    snapshot
        .validate_for(startup_id, pom_sha256, agent_info.is_none())
        .map_err(|_| "Native dependency snapshot rejected")?;
    if !document.dirty() {
        return Err("Native dependency check requires a dirty document".into());
    }
    let before = document_state(document);
    let undo = history_state(editor_ctx, document)?;
    let selection = current_selection(editor_ctx, document.id);
    let (mut app, rx) = app();
    if let Some(local) = local_agent_info {
        app.agent_info = Some(local.clone());
        if app.backend_java_maven_supported() {
            return Err("Local metadata incorrectly enables Maven controls".into());
        }
        app.start_language();
        if rx.try_recv().is_ok() {
            return Err("Local Maven rejection enqueued startup".into());
        }
    }
    if let Some(agent) = agent_info {
        app.agent_info = Some(agent.clone());
        app.active_form.as_mut().unwrap().allow_run = false;
        app.start_language();
        if rx.try_recv().is_ok() || !app.backend_java_maven_supported() {
            return Err("Trust-off Maven controls failed to refuse startup".into());
        }
        app.active_form.as_mut().unwrap().allow_run = true;
        app.error = None;
    }
    start(&mut app, &rx, startup_id, pom_sha256);
    app.editor_ctx = editor_ctx.clone();
    let original = std::mem::replace(
        document,
        Document::new(u64::MAX, String::new(), String::new(), String::new()),
    );
    app.active_document = Some(original.id);
    app.documents.push(original);
    let result = (|| {
        let info = app.agent_info.as_mut().unwrap();
        let original_info = info.clone();
        info.capabilities
            .retain(|name| name != JAVA_MAVEN_DEPENDENCIES_CAPABILITY);
        info.capability_groups
            .retain(|name| name != cedar_protocol::JAVA_MAVEN_DEPENDENCIES_GROUP);
        app.inspect_maven_dependencies();
        if rx.try_recv().is_ok() || !app.backend_java_maven_supported() {
            return Err(
                "Optional dependency capability did not independently gate the request".into(),
            );
        }
        app.agent_info = Some(original_info);
        app.error = None;
        let command = inspect(&mut app, &rx);
        apply_snapshot(&mut app, command, snapshot.clone());
        if app.language.maven_dependencies.snapshot.is_none() {
            return Err("Production dependency reply was not retained".into());
        }
        frame(&mut app, 1.0, false, vec![]);
        let text = rendered(&frame(&mut app, 1.1, false, vec![]));
        if !text.contains("Captured POM declarations")
            || !text.contains("Observed JDT library rows")
            || !text.contains("Scope: compile (default)")
            || !text.contains("optional: false (default)")
        {
            return Err("Production dependency view did not render distinct provenance".into());
        }
        let command = inspect(&mut app, &rx);
        let changed_hash = if pom_sha256 == "b".repeat(64) {
            "c".repeat(64)
        } else {
            "b".repeat(64)
        };
        app.observe_maven_pom_acknowledgement(app.next_request, "pom.xml", &changed_hash);
        apply_snapshot(&mut app, command, snapshot.clone());
        if app.language.maven_dependencies.snapshot.is_some()
            || app.language.maven_dependencies.pending.is_some()
            || !app.language.maven_model.restart_required()
        {
            return Err("Production dependency state accepted a stale POM reply".into());
        }
        if document_state(&app.documents[0]) != before
            || history_state(editor_ctx, &app.documents[0])? != undo
            || current_selection(editor_ctx, app.documents[0].id) != selection
        {
            return Err(
                "Dependency inspection changed the dirty buffer, selection or Undo baseline".into(),
            );
        }
        if rx.try_recv().is_ok() {
            return Err("Dependency view requested an unexpected side effect".into());
        }
        Ok(())
    })();
    *document = app.documents.remove(0);
    result
}

#[test]
fn dirty_old_pom_baseline_and_single_undo_survive_actual_dependency_path() {
    let ctx = egui::Context::default();
    let mut document = Document::new(
        1,
        "pom.xml".into(),
        "older saved POM".into(),
        "c".repeat(64),
    );
    editor_state::commit(&ctx, &mut document, "unsaved POM draft".into(), 2);
    let mut state =
        egui::TextEdit::load_state(&ctx, egui::Id::new(("editor", document.id))).unwrap();
    state.cursor.set_char_range(Some(egui::text::CCursorRange {
        primary: egui::text::CCursor {
            index: 2,
            prefer_next_row: false,
        },
        secondary: egui::text::CCursor {
            index: 12,
            prefer_next_row: true,
        },
    }));
    state.store(&ctx, egui::Id::new(("editor", document.id)));
    ctx.memory_mut(|memory| memory.request_focus(egui::Id::new(("editor", document.id))));
    let before = document_state(&document);
    verify_native_maven_dependencies(&snapshot(), 42, &hash(), &mut document, &ctx).unwrap();
    #[cfg(target_os = "linux")]
    {
        let (fixture, _) = grouped_app(true);
        let mut grouped = fixture.agent_info.unwrap();
        grouped.os = "linux".into();
        grouped.capability_groups.sort();
        verify_native_linux_maven_dependencies(
            &snapshot(),
            42,
            &hash(),
            &mut document,
            &ctx,
            &grouped,
            &full_test_agent(),
        )
        .unwrap();
    }
    assert_eq!(document_state(&document), before);
    assert_eq!(history_state(&ctx, &document).unwrap().1, "older saved POM");
}

#[test]
fn dependency_disconnect_remains_visible_even_for_an_obsolete_action() {
    let (mut app, rx) = app();
    start(&mut app, &rx, 42, &hash());
    let command = inspect(&mut app, &rx);
    app.language.reset();
    app.apply_event(Event {
        generation: app.generation,
        id: command.id,
        connected: false,
        result: Err("private error".into()),
    });
    assert!(!app.ready());
    assert!(!app.error.as_deref().unwrap_or("").contains("private"));
}

#[test]
fn dependency_case_collision_renders_every_association_as_ambiguous() {
    let (mut app, rx) = app();
    start(&mut app, &rx, 42, &hash());
    let command = inspect(&mut app, &rx);
    let mut data = snapshot();
    let mut second = data.declarations[0].clone();
    second.artifact_id = "LIBRARY".into();
    second.expected_jar_path = second.repository_jar_path();
    data.declarations.push(second);
    if let MavenDependencyObservation::Available { libraries } = &mut data.observation {
        libraries[0].declaration_indices = vec![0, 1];
    }
    apply_snapshot(&mut app, command, data);
    frame(&mut app, 1.0, false, vec![]);
    let text = rendered(&frame(&mut app, 1.1, false, vec![]));
    assert!(
        text.contains(
            "Declaration path matches: #1, #2 (ambiguous; no unique dependency identity)"
        ),
        "{text}"
    );
}

#[test]
fn old_pom_acknowledgement_cannot_replace_the_imported_hash_but_new_witness_invalidates() {
    let (mut app, rx) = app();
    app.open("pom.xml".into(), None);
    let old_read = rx.try_recv().unwrap();
    start(&mut app, &rx, 42, &hash());
    let command = inspect(&mut app, &rx);
    reply(
        &mut app,
        old_read,
        Ok(Payload::File {
            path: "pom.xml".into(),
            text: "old disk POM".into(),
            revision: "b".repeat(64),
        }),
    );
    apply_snapshot(&mut app, command, snapshot());
    assert!(app.language.maven_dependencies.snapshot.is_some());
    assert_eq!(app.language.maven_model.pom_sha256(), Some(hash().as_str()));
    app.observe_maven_pom_acknowledgement(app.next_request, "pom.xml", &"c".repeat(64));
    assert!(app.language.maven_dependencies.snapshot.is_none());
    assert!(app.language.maven_model.restart_required());
    assert_eq!(app.documents[0].saved_text, "old disk POM");
}

fn content_hash(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}
fn pom_payload(text: &str) -> Payload {
    Payload::File {
        path: "pom.xml".into(),
        text: text.into(),
        revision: content_hash(text),
    }
}
fn captured(app: &mut CedarApp, rx: &Receiver<Command>, imported: &str) {
    start(app, rx, 42, &content_hash(imported));
    let command = inspect(app, rx);
    let mut data = snapshot();
    data.pom_sha256 = content_hash(imported);
    apply_snapshot(app, command, data);
    assert!(app.language.maven_dependencies.snapshot.is_some());
}

#[test]
fn accepted_disk_comparison_and_reload_verification_invalidate_captured_pom() {
    for verification in [false, true] {
        let (mut app, rx) = app();
        app.documents.push(Document::new(
            1,
            "pom.xml".into(),
            "base".into(),
            content_hash("base"),
        ));
        app.active_document = Some(1);
        captured(&mut app, &rx, "base");
        let before = document_state(&app.documents[0]);
        app.compare_with_disk();
        let compare = rx.try_recv().unwrap();
        if verification {
            reply(&mut app, compare, Ok(pom_payload("base")));
            assert!(app.language.maven_dependencies.snapshot.is_some());
            app.reload_from_disk();
            let verify = rx.try_recv().unwrap();
            reply(&mut app, verify, Ok(pom_payload("new disk")));
        } else {
            reply(&mut app, compare, Ok(pom_payload("new disk")));
        }
        assert!(app.language.maven_dependencies.snapshot.is_none());
        assert!(app.language.maven_model.restart_required());
        assert_eq!(document_state(&app.documents[0]), before);
    }
}

#[test]
fn accepted_merge_verification_invalidates_pom_without_applying_or_saving_merge() {
    let (mut app, rx) = app();
    let base = "one\ntwo\nthree\n";
    let disk = "one\ntwo\nTHREE\n";
    app.documents.push(Document::new(
        1,
        "pom.xml".into(),
        base.into(),
        content_hash(base),
    ));
    app.active_document = Some(1);
    editor_state::commit(
        &app.editor_ctx,
        &mut app.documents[0],
        "ONE\ntwo\nthree\n".into(),
        2,
    );
    captured(&mut app, &rx, disk);
    app.compare_with_disk();
    let compare = rx.try_recv().unwrap();
    reply(&mut app, compare, Ok(pom_payload(disk)));
    let ctx = app.editor_ctx.clone();
    app.preview_disk_merge(&ctx);
    app.apply_disk_merge(&ctx);
    let verify = rx.try_recv().expect("merge verification read");
    let before = document_state(&app.documents[0]);
    reply(&mut app, verify, Ok(pom_payload("one\ntwo\nNEW\n")));
    assert!(app.language.maven_dependencies.snapshot.is_none());
    assert!(app.language.maven_model.restart_required());
    assert_eq!(document_state(&app.documents[0]), before);
    assert!(rx.try_recv().is_err());
}

#[test]
fn rejected_or_prestartup_disk_comparison_cannot_invalidate_pom() {
    for rejection in [
        "path",
        "revision",
        "nul",
        "stale_draft",
        "dismissed",
        "prestartup",
    ] {
        let (mut app, rx) = app();
        app.documents.push(Document::new(
            1,
            "pom.xml".into(),
            "base".into(),
            content_hash("base"),
        ));
        app.active_document = Some(1);
        let early = if rejection == "prestartup" {
            app.compare_with_disk();
            Some(rx.try_recv().unwrap())
        } else {
            None
        };
        captured(&mut app, &rx, "base");
        let command = early.unwrap_or_else(|| {
            app.compare_with_disk();
            rx.try_recv().unwrap()
        });
        let payload = match rejection {
            "path" => Payload::File {
                path: "another.xml".into(),
                text: "disk".into(),
                revision: content_hash("disk"),
            },
            "revision" => Payload::File {
                path: "pom.xml".into(),
                text: "disk".into(),
                revision: "bad".into(),
            },
            "nul" => pom_payload("disk\0"),
            "stale_draft" => {
                app.documents[0].edit_version += 1;
                pom_payload("disk")
            }
            "dismissed" => {
                app.dismiss_disk_review();
                pom_payload("disk")
            }
            _ => pom_payload("disk"),
        };
        reply(&mut app, command, Ok(payload));
        assert!(
            app.language.maven_dependencies.snapshot.is_some(),
            "{rejection}"
        );
        assert!(!app.language.maven_model.restart_required(), "{rejection}");
    }
}

#[test]
fn accepted_first_or_second_interrupted_save_read_invalidates_pom() {
    for second in [false, true] {
        let (mut app, rx) = app();
        app.documents.push(Document::new(
            1,
            "pom.xml".into(),
            "base".into(),
            content_hash("base"),
        ));
        app.active_document = Some(1);
        editor_state::commit(
            &app.editor_ctx,
            &mut app.documents[0],
            "submitted".into(),
            2,
        );
        app.documents[0].interrupted_save =
            crate::interrupted_save::InterruptedSave::capture(&app, &app.documents[0]);
        captured(&mut app, &rx, "base");
        app.check_interrupted_save();
        let first = rx.try_recv().unwrap();
        let before = document_state(&app.documents[0]);
        reply(
            &mut app,
            first,
            Ok(pom_payload(if second { "base" } else { "submitted" })),
        );
        if second {
            assert!(app.language.maven_dependencies.snapshot.is_some());
            let verify = rx.try_recv().unwrap();
            reply(&mut app, verify, Ok(pom_payload("submitted")));
        }
        assert!(app.language.maven_dependencies.snapshot.is_none());
        assert!(app.language.maven_model.restart_required());
        assert_eq!(
            document_state(&app.documents[0]),
            before,
            "no baseline adoption at this stage"
        );
    }
}

#[test]
fn rejected_interrupted_save_reads_do_not_claim_a_new_pom_witness() {
    for rejection in ["path", "hash", "foreign_text", "stale_draft", "dismissed"] {
        let (mut app, rx) = app();
        app.documents.push(Document::new(
            1,
            "pom.xml".into(),
            "base".into(),
            content_hash("base"),
        ));
        app.active_document = Some(1);
        editor_state::commit(
            &app.editor_ctx,
            &mut app.documents[0],
            "submitted".into(),
            2,
        );
        app.documents[0].interrupted_save =
            crate::interrupted_save::InterruptedSave::capture(&app, &app.documents[0]);
        captured(&mut app, &rx, "base");
        app.check_interrupted_save();
        let command = rx.try_recv().unwrap();
        let payload = match rejection {
            "path" => Payload::File {
                path: "other.xml".into(),
                text: "submitted".into(),
                revision: content_hash("submitted"),
            },
            "hash" => Payload::File {
                path: "pom.xml".into(),
                text: "submitted".into(),
                revision: content_hash("wrong"),
            },
            "foreign_text" => pom_payload("other writer"),
            "stale_draft" => {
                app.documents[0].edit_version += 1;
                pom_payload("submitted")
            }
            "dismissed" => {
                app.interrupted_save_check.invalidate();
                pom_payload("submitted")
            }
            _ => unreachable!(),
        };
        reply(&mut app, command, Ok(payload));
        assert!(
            app.language.maven_dependencies.snapshot.is_some(),
            "{rejection}"
        );
        assert!(!app.language.maven_model.restart_required());
    }
}

#[test]
fn validated_report_read_is_pom_evidence_even_when_the_report_parser_rejects_it() {
    for witness in ["valid", "path", "hash", "stale", "prestartup"] {
        let (mut app, rx) = app();
        app.test_report.path = "pom.xml".into();
        let early = if witness == "prestartup" {
            app.load_test_report();
            Some(rx.try_recv().unwrap())
        } else {
            None
        };
        captured(&mut app, &rx, "base");
        let command = early.unwrap_or_else(|| {
            app.load_test_report();
            rx.try_recv().unwrap()
        });
        let payload = match witness {
            "path" => Payload::File {
                path: "different.xml".into(),
                text: "new POM".into(),
                revision: content_hash("new POM"),
            },
            "hash" => Payload::File {
                path: "pom.xml".into(),
                text: "new POM".into(),
                revision: content_hash("forged"),
            },
            "stale" => {
                app.test_report.clear();
                pom_payload("new POM")
            }
            _ => pom_payload("new POM"),
        };
        reply(&mut app, command, Ok(payload));
        assert_eq!(
            app.language.maven_model.restart_required(),
            witness == "valid",
            "{witness}"
        );
        assert_eq!(
            app.language.maven_dependencies.snapshot.is_none(),
            witness == "valid",
            "{witness}"
        );
        assert!(
            app.test_report.snapshot.is_none(),
            "a POM is not a parsed test report"
        );
    }
}

#[test]
fn warmed_native_editor_preserves_full_reversed_selection_focus_and_undo_after_reply() {
    let (mut app, rx) = app();
    start(&mut app, &rx, 42, &hash());
    app.documents.push(Document::new(
        1,
        "pom.xml".into(),
        "saved baseline".into(),
        "c".repeat(64),
    ));
    app.active_document = Some(1);
    editor_state::commit(
        &app.editor_ctx,
        &mut app.documents[0],
        "unsaved POM draft".into(),
        2,
    );
    editor_frame(&mut app, 1.0);
    editor_frame(&mut app, 1.1);
    let id = egui::Id::new(("editor", 1u64));
    let mut state = egui::TextEdit::load_state(&app.editor_ctx, id).unwrap();
    state.cursor.set_char_range(Some(egui::text::CCursorRange {
        primary: egui::text::CCursor {
            index: 2,
            prefer_next_row: false,
        },
        secondary: egui::text::CCursor {
            index: 12,
            prefer_next_row: true,
        },
    }));
    state.store(&app.editor_ctx, id);
    app.editor_ctx.memory_mut(|memory| memory.request_focus(id));
    // Register a stable, live editor before the request; ordinary scroll_to
    // consumption has already happened and is not attributed to inspection.
    editor_frame(&mut app, 1.2);
    let before = document_state(&app.documents[0]);
    let selection = current_selection(&app.editor_ctx, 1);
    let undo = history_state(&app.editor_ctx, &app.documents[0]).unwrap();
    assert_eq!(app.editor_ctx.memory(|memory| memory.focused()), Some(id));
    let command = inspect(&mut app, &rx);
    apply_snapshot(&mut app, command, snapshot());
    editor_frame(&mut app, 1.3);
    assert_eq!(document_state(&app.documents[0]), before);
    assert_eq!(current_selection(&app.editor_ctx, 1), selection);
    assert_eq!(
        history_state(&app.editor_ctx, &app.documents[0]).unwrap(),
        undo
    );
    assert_eq!(app.editor_ctx.memory(|memory| memory.focused()), Some(id));
    let ctx = app.editor_ctx.clone();
    let _ = ctx.run(
        egui::RawInput {
            time: Some(1.4),
            events: vec![egui::Event::Key {
                key: egui::Key::Z,
                physical_key: Some(egui::Key::Z),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::COMMAND,
            }],
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| app.editor(ui));
        },
    );
    assert_eq!(
        app.documents[0].text, "saved baseline",
        "one native Undo crosses selection-only checkpoints"
    );
    assert!(rx.try_recv().is_err());
}

#[test]
fn same_frame_dependency_reply_then_new_pom_witness_leaves_no_stale_display() {
    let (mut app, rx) = app();
    start(&mut app, &rx, 42, &hash());
    app.documents.push(Document::new(
        1,
        "pom.xml".into(),
        "saved baseline".into(),
        "c".repeat(64),
    ));
    app.active_document = Some(1);
    editor_state::commit(
        &app.editor_ctx,
        &mut app.documents[0],
        "unsaved POM draft".into(),
        2,
    );
    editor_frame(&mut app, 1.0);
    let before = document_state(&app.documents[0]);
    let command = inspect(&mut app, &rx);
    // The real update polls replies before rendering its editor and language view.
    app.result_tx
        .send(crate::worker::WorkerEvent::Response(Event {
            generation: app.generation,
            id: command.id,
            connected: true,
            result: Ok(Payload::MavenDependencies {
                snapshot: snapshot(),
            }),
        }))
        .unwrap();
    app.test_report.path = "pom.xml".into();
    app.load_test_report();
    let read = rx.try_recv().unwrap();
    app.result_tx
        .send(crate::worker::WorkerEvent::Response(Event {
            generation: app.generation,
            id: read.id,
            connected: true,
            result: Ok(pom_payload("new disk POM")),
        }))
        .unwrap();
    let ctx = app.editor_ctx.clone();
    let mut native = eframe::Frame::_new_kittest();
    let _ = ctx.run(
        egui::RawInput {
            time: Some(1.1),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1600.0, 900.0),
            )),
            ..Default::default()
        },
        |ctx| eframe::App::update(&mut app, ctx, &mut native),
    );
    assert!(app.language.maven_model.restart_required());
    assert!(app.language.maven_dependencies.snapshot.is_none());
    assert_eq!(document_state(&app.documents[0]), before);
    assert!(rx.try_recv().is_err());
}
