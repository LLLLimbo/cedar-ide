use super::*;
use crate::{
    agent_support::full_test_agent,
    model::Document,
    worker::{Command, Event, Worker},
    ConnectForm, ConnectionState,
};
use cedar_protocol::{JAVA_LANGUAGE_SESSION_CAPABILITIES, JAVA_STARTUP_CAPABILITIES};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::sync::mpsc::Receiver;

const POM: &str = "<project><modelVersion>4.0.0</modelVersion></project>";
fn pom_hash() -> String {
    format!("{:x}", Sha256::digest(POM.as_bytes()))
}
fn java_configuration() -> JavaConfiguration {
    JavaConfiguration {
        executable: r"C:\Program Files\Java\bin\java.exe".into(),
        distribution: r"D:\JDT 雪".into(),
        data_directory: r"D:\Java data".into(),
    }
}
fn app() -> (CedarApp, Receiver<Command>) {
    let mut app = CedarApp::empty();
    app.state = ConnectionState::Ready;
    app.generation = 9;
    app.root = r"D:\工作区 雪".into();
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
        ]
        .into_iter()
        .map(str::to_owned),
    );
    info.capabilities
        .extend(JAVA_STARTUP_CAPABILITIES.iter().map(|name| (*name).into()));
    app.agent_info = Some(info);
    app.language.mode = ServerMode::Java;
    app.language.java = java_configuration();
    app.language.maven.enabled = true;
    app.language.maven.local_repository = r"D:\Maven cache 雪".into();
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
fn language(value: Value) -> Result<Payload, String> {
    Ok(Payload::Language { value })
}
fn ready_value() -> Value {
    json!({"started":true,"initialize":{"capabilities":{"completionProvider":{}},"cedar_java_profile":"maven_leaf","cedar_java_maven_model":true,"cedar_java_maven_pom_sha256":pom_hash()},"root_uri":"file:///D:/workspace"})
}
fn tick(app: &mut CedarApp, now: f64) {
    let ctx = app.editor_ctx.clone();
    ctx.begin_pass(egui::RawInput {
        time: Some(now),
        ..Default::default()
    });
    app.language_tick(&ctx);
    let _ = ctx.end_pass();
}
fn start(app: &mut CedarApp, rx: &Receiver<Command>) {
    start_with_hash(app, rx, &pom_hash());
}
fn start_with_hash(app: &mut CedarApp, rx: &Receiver<Command>, hash: &str) {
    app.start_language();
    let begin = rx.try_recv().unwrap();
    assert!(matches!(
        begin.op,
        Operation::LanguageStartJavaMavenBegin { .. }
    ));
    reply(
        app,
        begin,
        language(json!({"state":"starting","startup_id":42,"process_id":null})),
    );
    let now = app.editor_ctx.input(|input| input.time);
    tick(app, now + 0.3);
    let poll = rx.try_recv().unwrap();
    assert!(matches!(
        poll.op,
        Operation::LanguageStartJavaPoll { startup_id: 42 }
    ));
    let mut ready = ready_value();
    ready["initialize"]["cedar_java_maven_pom_sha256"] = json!(hash);
    reply(
        app,
        poll,
        language(json!({"state":"ready","startup_id":42,"language":ready})),
    );
    assert!(app.language.running);
    assert!(app.language.maven_model.active());
    assert!(
        rx.try_recv().is_err(),
        "startup does not request a Maven model"
    );
}
fn model_value() -> Value {
    json!({"profile":"maven_leaf","status":"imported","pom_path":"pom.xml","pom_sha256":pom_hash(),"restart_required":false,"maven_nature":true,
        "compiler":{"source":"21","compliance":"21","target":"21","release_enabled":true},
        "source_paths":["src/main/java","src/test/java","."],
        "classpath":[{"kind":"library","path":"D:\\Maven cache 雪\\library.jar","resolved":true,"origin":"model"}],"unresolved_count":0})
}
fn model_request(app: &mut CedarApp, rx: &Receiver<Command>) -> Command {
    app.check_maven_model();
    let command = rx.try_recv().unwrap();
    assert!(matches!(command.op, Operation::LanguageMavenModel));
    command
}

fn server_closed(app: &mut CedarApp) {
    app.apply_language_action(
        Action {
            session: app.language.session,
            kind: ActionKind::Events,
        },
        json!({"events":[{"type":"closed","private":"private server details"}]}),
    );
}

fn editor_frame(app: &mut CedarApp, time: f64, events: Vec<egui::Event>) {
    let ctx = app.editor_ctx.clone();
    let _ = ctx.run(
        egui::RawInput {
            time: Some(time),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 600.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| app.editor(ui));
        },
    );
}

fn model_controls_frame(
    app: &mut CedarApp,
    time: f64,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let ctx = app.editor_ctx.clone();
    ctx.run(
        egui::RawInput {
            time: Some(time),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 600.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| app.maven_model_controls(ui));
        },
    )
}

fn model_text(output: &egui::FullOutput) -> Vec<&egui::epaint::TextShape> {
    fn collect<'a>(shape: &'a egui::epaint::Shape, text: &mut Vec<&'a egui::epaint::TextShape>) {
        match shape {
            egui::epaint::Shape::Text(value) => text.push(value),
            egui::epaint::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect(shape, text);
                }
            }
            _ => {}
        }
    }
    let mut text = Vec::new();
    for shape in &output.shapes {
        collect(&shape.shape, &mut text);
    }
    text
}

fn hover_model_control(app: &mut CedarApp) -> (egui::Pos2, egui::FullOutput, egui::FullOutput) {
    model_controls_frame(app, 0.0, vec![]);
    let output = model_controls_frame(app, 0.01, vec![]);
    let at = model_text(&output)
        .into_iter()
        .find(|text| text.galley.job.text == "Check Maven model")
        .expect("Maven model control is visible")
        .visual_bounding_rect()
        .center();
    let delay = f64::from(app.editor_ctx.style().interaction.tooltip_delay);
    assert!(delay > 0.0);
    model_controls_frame(app, 0.1, vec![egui::Event::PointerMoved(at)]);
    // First pointer appearance has no velocity history in egui and may show a
    // tooltip immediately. Compare no pointer with a hover settled past delay.
    model_controls_frame(app, 0.1 + delay + 1.0, vec![]);
    let hovered = model_controls_frame(app, 0.2 + delay + 1.0, vec![]);
    (at, output, hovered)
}

fn click_model_control(app: &mut CedarApp, at: egui::Pos2) {
    for (offset, pressed) in [true, false].into_iter().enumerate() {
        model_controls_frame(
            app,
            3.0 + offset as f64 * 0.01,
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
fn actual_frames_disabled_maven_model_hover_draws_reason_without_dispatch() {
    for gate in ["trust", "agent", "model", "restart", "closing", "exited"] {
        let (mut app, rx) = app();
        // Activate the existing synthetic Windows profile without starting Java.
        app.language.running = true;
        app.language.session = 3;
        app.activate_maven_model(&ready_value());
        let reason = match gate {
            "trust" => {
                app.active_form.as_mut().unwrap().allow_run = false;
                "Maven model checks require trusted tool permission for this connection"
            }
            "agent" => {
                app.agent_info
                    .as_mut()
                    .unwrap()
                    .capabilities
                    .retain(|name| name != "language_maven_model");
                "The workspace agent does not advertise the complete typed Maven Java lifecycle. Drafts remain editable; upgrade or use an agent with this capability"
            }
            "model" => {
                app.language.maven_model.supported = false;
                UNAVAILABLE
            }
            "restart" => {
                app.language.maven_model.require_restart();
                RESTART
            }
            "closing" => {
                app.close_after_language_stop = true;
                "Wait for the current language request or close operation to finish"
            }
            "exited" => {
                server_closed(&mut app);
                SERVER_EXITED
            }
            _ => unreachable!(),
        };
        let (at, unhovered, hovered) = hover_model_control(&mut app);
        let count_reason = |output: &egui::FullOutput| {
            model_text(output)
                .iter()
                .filter(|text| text.galley.job.text == reason)
                .count()
        };
        // Terminal states also have a visible status label; the tooltip must add
        // another painted copy, so that label alone cannot satisfy this test.
        let status_count = usize::from(app.language.maven_model.message() == reason);
        assert_eq!(
            count_reason(&unhovered),
            status_count,
            "no hover for {gate}"
        );
        assert_eq!(
            count_reason(&hovered),
            status_count + 1,
            "disabled reason for {gate}"
        );
        assert!(rx.try_recv().is_err(), "hover must not dispatch for {gate}");
        click_model_control(&mut app, at);
        assert!(rx.try_recv().is_err(), "disabled click for {gate}");
        assert!(app.language.maven_model.pending.is_none());
    }
}

#[test]
fn actual_frames_enabled_maven_model_hover_has_no_disabled_reason_and_remains_usable() {
    let (mut app, rx) = app();
    app.language.running = true;
    app.language.session = 3;
    app.activate_maven_model(&ready_value());
    let (at, _, hovered) = hover_model_control(&mut app);
    assert_eq!(
        model_text(&hovered)
            .iter()
            .map(|text| text.galley.job.text.as_str())
            .collect::<Vec<_>>(),
        vec!["Check Maven model", app.language.maven_model.message()],
        "enabled controls draw only the button and ordinary model status"
    );
    assert!(rx.try_recv().is_err());
    click_model_control(&mut app, at);
    assert!(matches!(
        rx.try_recv().unwrap().op,
        Operation::LanguageMavenModel
    ));
    assert!(rx.try_recv().is_err());
    assert!(app.language.maven_model.pending.is_some());
    assert!(app.language.view == View::Maven);
}

#[test]
fn maven_is_opt_in_and_host_paths_are_literal_without_cache_discovery() {
    assert!(!MavenConfiguration::default().enabled);
    let mut config = MavenConfiguration {
        enabled: true,
        local_repository: r"D:\Maven cache 雪\$(literal)".into(),
    };
    let mut java = java_configuration();
    assert!(
        matches!(config.operation(&java).unwrap(), Operation::LanguageStartJavaMavenBegin {
        java_executable, distribution, data_directory, local_repository
    } if java_executable == java.executable && distribution == java.distribution && data_directory == java.data_directory && local_repository == config.local_repository)
    );
    for cache in ["", "  ", "bad\0path", "bad\npath"] {
        config.local_repository = cache.into();
        assert!(config
            .operation(&java)
            .unwrap_err()
            .contains("existing local Maven repository"));
    }
    config.local_repository = "x".repeat(4097);
    assert!(config.operation(&java).is_err());
    config.local_repository = r"D:\cache".into();
    java.data_directory = r"D:\数据".into();
    assert!(config
        .operation(&java)
        .unwrap_err()
        .contains("ASCII JDT data/control"));
    assert!(
        java.operation().is_ok(),
        "ordinary Java retains its existing Unicode data path behavior"
    );
}

#[test]
fn maven_never_falls_back_when_trust_or_any_required_capability_is_missing() {
    for missing in JAVA_LANGUAGE_SESSION_CAPABILITIES
        .iter()
        .chain(JAVA_STARTUP_CAPABILITIES)
        .chain(["language_start_java_maven_begin", "language_maven_model"].iter())
    {
        let (mut app, rx) = app();
        app.agent_info
            .as_mut()
            .unwrap()
            .capabilities
            .retain(|name| name.as_str() != *missing);
        app.start_language();
        assert!(
            rx.try_recv().is_err(),
            "missing {missing} must not select another Java recipe"
        );
        assert_eq!(app.language.session, 0);
        assert!(!app.language.running);
    }
    let (mut app, rx) = app();
    app.active_form.as_mut().unwrap().allow_run = false;
    app.form.allow_run = true;
    app.start_language();
    assert!(rx.try_recv().is_err());
    assert!(app
        .operation_problem(&app.language.maven.operation(&app.language.java).unwrap())
        .is_some());
    assert!(!app.execution_trusted());
    app.active_form.as_mut().unwrap().allow_run = true;
    app.agent_info = None;
    app.start_language();
    assert!(
        rx.try_recv().is_err(),
        "legacy protocol-4 is not Maven support"
    );
}

#[test]
fn ordinary_java_keeps_both_async_and_legacy_recipes() {
    for asynchronous in [false, true] {
        let (mut app, rx) = app();
        app.language.maven.enabled = false;
        app.language.maven.local_repository.clear();
        app.language.java.data_directory = r"D:\Java data 雪".into();
        if !asynchronous {
            app.agent_info
                .as_mut()
                .unwrap()
                .capabilities
                .retain(|name| name != "language_start_java_begin");
        }
        app.start_language();
        let command = rx.try_recv().unwrap();
        assert!(if asynchronous {
            matches!(command.op, Operation::LanguageStartJavaBegin { .. })
        } else {
            matches!(command.op, Operation::LanguageStartJava { .. })
        });
    }
}

#[test]
fn typed_startup_requires_profile_hash_and_capability_witness_but_never_model_polling() {
    for missing in [
        "cedar_java_profile",
        "cedar_java_maven_pom_sha256",
        "cedar_java_maven_model",
    ] {
        let (mut app, rx) = app();
        app.start_language();
        let begin = rx.try_recv().unwrap();
        reply(
            &mut app,
            begin,
            language(json!({"state":"starting","startup_id":42,"process_id":null})),
        );
        tick(&mut app, 0.3);
        let poll = rx.try_recv().unwrap();
        let mut value = ready_value();
        value["initialize"].as_object_mut().unwrap().remove(missing);
        reply(
            &mut app,
            poll,
            language(json!({"state":"ready","startup_id":42,"language":value})),
        );
        assert!(!app.language.running);
        assert!(app.language.restart_blocked);
        assert!(rx.try_recv().is_err());
    }
    let (mut app, rx) = app();
    start(&mut app, &rx);
    app.language.automatic = false;
    tick(&mut app, 60.0);
    tick(&mut app, 600.0);
    assert!(rx.try_recv().is_err());
    assert!(app.language.maven_model.status == ModelStatus::Unchecked);
}

#[test]
fn maven_cancel_before_ready_uses_existing_owner_and_waits_for_cleanup() {
    let (mut app, rx) = app();
    app.start_language();
    let begin = rx.try_recv().unwrap();
    app.cancel_java_startup();
    assert!(rx.try_recv().is_err());
    reply(
        &mut app,
        begin,
        language(json!({"state":"starting","startup_id":42,"process_id":null})),
    );
    let cancel = rx.try_recv().unwrap();
    assert!(matches!(
        cancel.op,
        Operation::LanguageStartJavaCancel { startup_id: 42 }
    ));
    assert!(app.language.startup_active());
    assert!(!app.language.running);
    reply(
        &mut app,
        cancel,
        language(json!({"state":"cancelled","startup_id":42,"cleanup_verified":true})),
    );
    assert!(!app.language.startup_active());
    assert!(!app.language.maven_model.active());
    assert!(
        app.language.maven.enabled,
        "the explicit selection is retained without starting it"
    );
    assert!(rx.try_recv().is_err());
}

#[test]
fn model_parser_accepts_only_the_bounded_fixed_schema() {
    let hash = pom_hash();
    assert!(Model::parse(model_value(), &hash).is_ok());
    let mut unresolved = model_value();
    unresolved["status"] = json!("unresolved");
    unresolved["unresolved_count"] = json!(1);
    unresolved["classpath"][0]["resolved"] = json!(false);
    unresolved["classpath"][0]["origin"] = json!("declared");
    assert!(Model::parse(unresolved, &hash).is_ok());
    let mut unavailable = model_value();
    unavailable["status"] = json!("unavailable");
    unavailable["maven_nature"] = json!(false);
    unavailable["compiler"] =
        json!({"source":null,"compliance":null,"target":null,"release_enabled":null});
    assert!(Model::parse(unavailable, &hash).is_ok());
    for (key, value) in [
        ("profile", json!("generic")),
        ("status", json!("building")),
        ("pom_path", json!("child/pom.xml")),
        ("pom_sha256", json!("a".repeat(64))),
        ("restart_required", json!(true)),
        ("maven_nature", json!(false)),
        ("source_paths", json!(["../secret"])),
        ("source_paths", json!(["src\nsecret"])),
        ("source_paths", json!(vec!["src"; 65])),
        ("source_paths", json!(["x".repeat(4097)])),
        (
            "classpath",
            json!(vec![model_value()["classpath"][0].clone(); 257]),
        ),
        ("unresolved_count", json!(1)),
        ("message", json!("x".repeat(257))),
        ("command", json!({"command":"must-not-run"})),
    ] {
        let mut bad = model_value();
        bad[key] = value;
        assert!(Model::parse(bad, &hash).is_err(), "must reject {key}");
    }
    for key in ["source", "compliance", "target", "release_enabled"] {
        let mut bad = model_value();
        bad["compiler"].as_object_mut().unwrap().remove(key);
        assert!(Model::parse(bad, &hash).is_err());
    }
    for (key, value) in [
        ("source", json!("x".repeat(33))),
        ("target", json!("雪")),
        ("release_enabled", json!(21)),
        ("release", json!(21)),
    ] {
        let mut bad = model_value();
        bad["compiler"][key] = value;
        assert!(Model::parse(bad, &hash).is_err());
    }
    for (key, value) in [
        ("path", json!("x".repeat(4097))),
        ("path", json!("line\nbreak")),
        ("kind", json!("command")),
        ("origin", json!("arbitrary")),
        ("resolved", json!(false)),
    ] {
        let mut bad = model_value();
        bad["classpath"][0][key] = value;
        assert!(Model::parse(bad, &hash).is_err());
    }
    let mut bad = model_value();
    bad["classpath"] = json!(vec![
        json!({"kind":"library","path":"x".repeat(4096),"resolved":true,"origin":"model"});
        65
    ]);
    assert!(
        Model::parse(bad, &hash).is_err(),
        "combined path bytes are bounded"
    );
    assert!(display_path(&"雪".repeat(200)).chars().count() <= 161);
}

#[test]
fn absent_source_folders_do_not_count_as_unresolved_dependencies() {
    let mut value = model_value();
    value["classpath"].as_array_mut().unwrap().push(json!({
        "kind":"source", "path":"src/test/java", "resolved":false, "origin":"model"
    }));
    let model = Model::parse(value.clone(), &pom_hash()).unwrap();
    assert!(model.status == ImportedStatus::Imported);
    assert_eq!(model.unresolved_count, 0);

    value["unresolved_count"] = json!(1);
    value["status"] = json!("unresolved");
    assert!(
        Model::parse(value.clone(), &pom_hash()).is_err(),
        "a source folder cannot manufacture an unresolved dependency"
    );
    value["classpath"][0]["resolved"] = json!(false);
    assert!(
        Model::parse(value.clone(), &pom_hash()).is_ok(),
        "only the missing library is counted"
    );
    value["unresolved_count"] = json!(2);
    assert!(Model::parse(value, &pom_hash()).is_err());
}

#[test]
fn one_explicit_check_produces_inert_status_and_never_polls_or_saves() {
    let (mut app, rx) = app();
    start(&mut app, &rx);
    app.language.automatic = false;
    let command = model_request(&mut app, &rx);
    app.check_maven_model();
    assert!(
        rx.try_recv().is_err(),
        "duplicate clicks cannot enqueue a second request"
    );
    let mut value = model_value();
    value["message"] = json!("private raw server message <script>command</script>");
    reply(&mut app, command, language(value));
    assert!(app.language.maven_model.status == ModelStatus::Imported);
    assert!(
        app.language.cjk_seen,
        "model paths request the existing Unicode font fallback"
    );
    assert!(!app.language.output.contains("private"));
    assert!(!app.language.output.contains("script"));
    tick(&mut app, 500.0);
    assert!(rx.try_recv().is_err());
}

#[test]
fn dirty_pom_and_java_buffers_keep_selection_and_native_undo_through_check_and_exit() {
    let (mut app, rx) = app();
    app.documents
        .push(Document::new(1, "pom.xml".into(), POM.into(), pom_hash()));
    app.documents.push(Document::new(
        2,
        "Main.java".into(),
        "class Main {}".into(),
        "java-revision".into(),
    ));
    for doc in &mut app.documents {
        let text = format!("{}\n<!-- draft -->", doc.text);
        crate::editor_state::commit(&app.editor_ctx, doc, text, 1);
    }
    for id in [1, 2] {
        app.active_document = Some(id);
        editor_frame(&mut app, id as f64, vec![]);
        editor_frame(&mut app, id as f64 + 0.1, vec![]);
        let editor = egui::Id::new(("editor", id));
        let mut state = egui::TextEdit::load_state(&app.editor_ctx, editor).unwrap();
        state.cursor.set_char_range(Some(egui::text::CCursorRange {
            primary: egui::text::CCursor {
                index: 2,
                prefer_next_row: false,
            },
            secondary: egui::text::CCursor {
                index: 10,
                prefer_next_row: true,
            },
        }));
        state.store(&app.editor_ctx, editor);
        app.editor_ctx
            .memory_mut(|memory| memory.request_focus(editor));
        editor_frame(&mut app, id as f64 + 0.2, vec![]);
    }
    let snapshots: Vec<_> = app
        .documents
        .iter()
        .map(|doc| {
            (
                doc.text.clone(),
                doc.saved_text.clone(),
                doc.revision.clone(),
                doc.edit_version,
                doc.cursor,
                format!(
                    "{:?}",
                    egui::TextEdit::load_state(&app.editor_ctx, egui::Id::new(("editor", doc.id)))
                        .unwrap()
                        .cursor
                        .char_range()
                ),
            )
        })
        .collect();
    start(&mut app, &rx);
    assert!(app.maven_dirty_pom());
    assert!(DIRTY_POM.contains("does not save"));
    let command = model_request(&mut app, &rx);
    reply(&mut app, command, language(model_value()));
    assert!(app.language.maven_model.status == ModelStatus::Imported);
    let late = model_request(&mut app, &rx);
    server_closed(&mut app);
    reply(&mut app, late, language(model_value()));
    server_closed(&mut app);
    assert!(app.language.maven_model.status == ModelStatus::ServerExited);
    for (doc, snapshot) in app.documents.iter().zip(&snapshots) {
        assert_eq!(
            &(
                doc.text.clone(),
                doc.saved_text.clone(),
                doc.revision.clone(),
                doc.edit_version,
                doc.cursor,
                // CCursor equality omits wrapped-row affinity; Debug includes it.
                format!(
                    "{:?}",
                    egui::TextEdit::load_state(&app.editor_ctx, egui::Id::new(("editor", doc.id)))
                        .unwrap()
                        .cursor
                        .char_range()
                ),
            ),
            snapshot
        );
        assert!(doc.dirty());
    }
    for id in [1, 2] {
        app.active_document = Some(id);
        let editor = egui::Id::new(("editor", id));
        editor_frame(&mut app, 5.0 + id as f64, vec![]);
        assert_eq!(
            format!(
                "{:?}",
                egui::TextEdit::load_state(&app.editor_ctx, editor)
                    .unwrap()
                    .cursor
                    .char_range()
            ),
            snapshots[(id - 1) as usize].5,
            "the next editor frame retains both selection endpoints and affinity"
        );
        app.editor_ctx
            .memory_mut(|memory| memory.request_focus(editor));
        editor_frame(
            &mut app,
            5.1 + id as f64,
            vec![egui::Event::Key {
                key: egui::Key::Z,
                physical_key: Some(egui::Key::Z),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::COMMAND,
            }],
        );
        let doc = &app.documents[(id - 1) as usize];
        assert_eq!(doc.text, doc.saved_text, "one native Undo for {}", doc.path);
        assert_eq!(doc.revision, snapshots[(id - 1) as usize].2);
        assert_eq!(doc.edit_version, snapshots[(id - 1) as usize].3 + 1);
    }
    assert!(
        rx.try_recv().is_err(),
        "no Save, Save All, build, sync or reimport was requested"
    );
}

#[test]
fn stale_session_generation_and_cancelled_model_replies_cannot_replace_current_state() {
    for scenario in [0, 1, 2] {
        let (mut app, rx) = app();
        start(&mut app, &rx);
        let command = model_request(&mut app, &rx);
        let generation = app.generation;
        if scenario == 0 {
            app.language.reset();
        }
        if scenario == 1 {
            app.generation += 1;
        }
        if scenario == 2 {
            app.language.maven_model.cancel_pending();
        }
        app.language.output = "newer state".into();
        app.error = Some("newer error".into());
        app.apply_event(Event {
            generation,
            id: command.id,
            connected: true,
            result: Err("language_maven_restart_required: stale".into()),
        });
        assert_eq!(app.language.output, "newer state");
        assert_eq!(app.error.as_deref(), Some("newer error"));
        assert!(app.language.maven_model.status != ModelStatus::RestartRequired);
    }
}

#[test]
fn pom_disk_change_requires_restart_and_late_imported_response_is_ignored() {
    for local_witness in [false, true] {
        let (mut app, rx) = app();
        app.documents
            .push(Document::new(1, "pom.xml".into(), POM.into(), pom_hash()));
        start(&mut app, &rx);
        let command = model_request(&mut app, &rx);
        let result = if local_witness {
            app.documents[0].text = "changed POM".into();
            app.save_document(1);
            let save = rx.try_recv().unwrap();
            assert!(matches!(save.op, Operation::Write { .. }));
            reply(
                &mut app,
                save,
                Ok(Payload::Written {
                    revision: format!("{:x}", Sha256::digest(b"changed POM")),
                }),
            );
            assert!(matches!(rx.try_recv().unwrap().op, Operation::List { .. }));
            language(model_value())
        } else {
            Err("language_maven_restart_required: private server detail".into())
        };
        reply(&mut app, command, result);
        assert!(app.language.maven_model.status == ModelStatus::RestartRequired);
        assert!(app.language.maven_model.model.is_none());
        app.check_maven_model();
        assert!(rx.try_recv().is_err());
        assert!(
            app.language.running,
            "restart requirement does not falsely claim the server stopped"
        );
        app.stop_language();
        assert!(matches!(rx.try_recv().unwrap().op, Operation::LanguageStop));
    }
}

#[test]
fn wrong_content_save_ack_does_not_become_a_pom_disk_witness() {
    let (mut app, rx) = app();
    app.documents
        .push(Document::new(1, "pom.xml".into(), POM.into(), pom_hash()));
    start(&mut app, &rx);
    let model = model_request(&mut app, &rx);
    app.documents[0].text = "changed POM".into();
    app.save_document(1);
    let save = rx.try_recv().unwrap();
    assert!(matches!(save.op, Operation::Write { .. }));
    let wrong_revision = "a".repeat(64);
    assert_ne!(wrong_revision, pom_hash());
    assert_ne!(
        wrong_revision,
        format!("{:x}", Sha256::digest(b"changed POM"))
    );
    reply(
        &mut app,
        save,
        Ok(Payload::Written {
            revision: wrong_revision,
        }),
    );
    assert!(app.ready());
    assert!(app.language.running);
    assert!(app.language.maven_model.status == ModelStatus::Checking);
    assert_eq!(
        app.language.maven_model.pom_sha256(),
        Some(pom_hash().as_str())
    );
    assert_eq!(app.documents[0].text, "changed POM");
    assert_eq!(app.documents[0].saved_text, POM);
    assert_eq!(
        app.documents[0].revision.as_deref(),
        Some(pom_hash().as_str())
    );
    assert!(app.documents[0].dirty());
    assert!(!app.documents[0].saving);
    assert!(app.documents[0].interrupted_save.is_some());
    assert!(!app.documents[0].save_outcome_unverifiable);
    assert!(
        rx.try_recv().is_err(),
        "unverified saves must not refresh Explorer"
    );

    reply(&mut app, model, language(model_value()));
    assert!(app.language.maven_model.status == ModelStatus::Imported);
    assert!(app.language.maven_model.model.is_some());
    assert!(!app.language.maven_model.restart_required());
    app.save_document(1);
    assert!(app.error.as_deref().unwrap().contains("interrupted save"));
    assert!(rx.try_recv().is_err());
}

#[test]
fn preexisting_pom_baseline_does_not_reject_a_newer_server_snapshot() {
    let (mut app, rx) = app();
    app.documents
        .push(Document::new(1, "pom.xml".into(), POM.into(), pom_hash()));
    crate::editor_state::commit(
        &app.editor_ctx,
        &mut app.documents[0],
        "unsaved POM draft".into(),
        1,
    );
    let revision_b = "b".repeat(64);
    start_with_hash(&mut app, &rx, &revision_b);
    let command = model_request(&mut app, &rx);
    let mut model = model_value();
    model["pom_sha256"] = json!(revision_b);
    reply(&mut app, command, language(model));
    assert!(app.language.maven_model.status == ModelStatus::Imported);
    let doc = &app.documents[0];
    assert_eq!(doc.text, "unsaved POM draft");
    assert_eq!(doc.saved_text, POM);
    assert_eq!(doc.revision.as_deref(), Some(pom_hash().as_str()));
    let state =
        egui::TextEdit::load_state(&app.editor_ctx, egui::Id::new(("editor", doc.id))).unwrap();
    let after = (state.cursor.char_range().unwrap(), doc.text.clone());
    assert_eq!(state.undoer().undo(&after).unwrap().1, POM);
    assert!(
        rx.try_recv().is_err(),
        "start and check must not repair or save the old tab"
    );

    // A newly submitted save acknowledgement is evidence after activation.
    // A newer draft typed during that save remains untouched.
    app.save_document(1);
    let save = rx.try_recv().unwrap();
    assert!(matches!(save.op, Operation::Write { .. }));
    crate::editor_state::commit(
        &app.editor_ctx,
        &mut app.documents[0],
        "newer POM draft".into(),
        2,
    );
    reply(
        &mut app,
        save,
        Ok(Payload::Written {
            revision: format!("{:x}", Sha256::digest(b"unsaved POM draft")),
        }),
    );
    assert!(app.language.maven_model.status == ModelStatus::RestartRequired);
    assert_eq!(app.documents[0].text, "newer POM draft");
    assert_eq!(app.documents[0].saved_text, "unsaved POM draft");
    assert!(app.documents[0].dirty());
    assert!(matches!(rx.try_recv().unwrap().op, Operation::List { .. }));
    app.check_maven_model();
    assert!(rx.try_recv().is_err());
}

#[test]
fn preactivation_pending_pom_read_cannot_invalidate_the_ready_hash() {
    let (mut app, rx) = app();
    app.start_language();
    let begin = rx.try_recv().unwrap();
    reply(
        &mut app,
        begin,
        language(json!({"state":"starting","startup_id":42,"process_id":null})),
    );
    app.open("pom.xml".into(), None);
    let old_read = rx.try_recv().unwrap();
    assert!(matches!(old_read.op, Operation::Read { .. }));
    tick(&mut app, 0.3);
    let poll = rx.try_recv().unwrap();
    let mut ready = ready_value();
    ready["initialize"]["cedar_java_maven_pom_sha256"] = json!("b".repeat(64));
    reply(
        &mut app,
        poll,
        language(json!({"state":"ready","startup_id":42,"language":ready})),
    );
    reply(
        &mut app,
        old_read,
        Ok(Payload::File {
            path: "pom.xml".into(),
            text: POM.into(),
            revision: pom_hash(),
        }),
    );
    assert!(app.language.maven_model.status == ModelStatus::Unchecked);
    let command = model_request(&mut app, &rx);
    let mut model = model_value();
    model["pom_sha256"] = json!("b".repeat(64));
    reply(&mut app, command, language(model));
    assert!(app.language.maven_model.status == ModelStatus::Imported);

    // Reopening the clean POM submits a genuinely new observation.
    app.documents.clear();
    app.active_document = None;
    app.open("pom.xml".into(), None);
    let new_read = rx.try_recv().unwrap();
    reply(
        &mut app,
        new_read,
        Ok(Payload::File {
            path: "pom.xml".into(),
            text: "changed POM".into(),
            revision: "c".repeat(64),
        }),
    );
    assert!(app.language.maven_model.status == ModelStatus::RestartRequired);
    assert!(rx.try_recv().is_err());
}

#[test]
fn unavailable_and_bad_model_responses_are_bounded_without_generic_activity_or_error_leaks() {
    for result in [
        language(Value::Null),
        Ok(Payload::Entries { entries: vec![] }),
        Err("private backend details".into()),
    ] {
        let (mut app, rx) = app();
        start(&mut app, &rx);
        let command = model_request(&mut app, &rx);
        app.error = None;
        reply(&mut app, command, result);
        assert!(app.language.maven_model.status == ModelStatus::Unavailable);
        assert_eq!(app.language.output, UNAVAILABLE);
        assert!(app.error.is_none());
    }
    let (mut app, rx) = app();
    assert!(app
        .operation_problem(&Operation::LanguageMavenModel)
        .is_some());
    start(&mut app, &rx);
    app.language.maven_model.supported = false;
    app.check_maven_model();
    assert!(rx.try_recv().is_err());
    app.language.maven_model.supported = true;
    app.active_form.as_mut().unwrap().allow_run = false;
    app.check_maven_model();
    assert!(rx.try_recv().is_err());
}

#[test]
fn model_reply_cannot_hide_disconnect_even_when_the_session_is_stale() {
    let (mut app, rx) = app();
    start(&mut app, &rx);
    let command = model_request(&mut app, &rx);
    app.language.reset();
    app.apply_event(Event {
        generation: app.generation,
        id: command.id,
        connected: false,
        result: Err("private transport details".into()),
    });
    assert!(!app.ready());
    assert!(!app.language.running);
    assert!(!app.error.as_deref().unwrap_or("").contains("private"));
}

#[test]
fn closed_retires_each_model_status_once_without_releasing_the_session() {
    let mut inactive = ModelState::default();
    assert!(!inactive.server_exited());
    assert_eq!(inactive.sequence, 0);
    for status in [
        ModelStatus::Unchecked,
        ModelStatus::Checking,
        ModelStatus::Imported,
        ModelStatus::Unresolved,
        ModelStatus::Unavailable,
        ModelStatus::RestartRequired,
    ] {
        let (mut app, rx) = app();
        start(&mut app, &rx);
        if status != ModelStatus::Unchecked {
            let command = model_request(&mut app, &rx);
            if status != ModelStatus::Checking {
                let mut value = model_value();
                if status == ModelStatus::Unresolved {
                    value["status"] = json!("unresolved");
                    value["classpath"][0]["resolved"] = json!(false);
                    value["unresolved_count"] = json!(1);
                }
                let result = if status == ModelStatus::Unavailable {
                    language(Value::Null)
                } else if status == ModelStatus::RestartRequired {
                    Err("language_maven_restart_required: private details".into())
                } else {
                    language(value)
                };
                reply(&mut app, command, result);
            }
        }
        assert!(app.language.maven_model.status == status);
        let session = app.language.session;
        let floor = app.language.maven_model.observation_request_floor;
        let sequence = app.language.maven_model.sequence;
        let jobs = app.pending.len();
        for _ in 0..2 {
            app.language.output = "intervening activity".into();
            server_closed(&mut app);
            assert!(app.language.maven_model.status == ModelStatus::ServerExited);
            assert_eq!(app.language.maven_model.message(), SERVER_EXITED);
            assert_eq!(app.language.output, SERVER_EXITED);
            assert_eq!(app.language.maven_model.sequence, sequence.wrapping_add(1));
            assert!(app.language.maven_model.pending.is_none());
            assert!(app.language.maven_model.model.is_none());
            assert_eq!(
                app.language.maven_model.pom_sha256(),
                Some(pom_hash().as_str())
            );
            assert_eq!(app.language.maven_model.observation_request_floor, floor);
            assert_eq!(app.pending.len(), jobs);
            assert!(app.language.running && app.language.diagnostics_exited);
            assert_eq!(app.language.session, session);
            assert_eq!(app.maven_model_problem().as_deref(), Some(SERVER_EXITED));
            assert!(!app.language.maven_model.server_exited());
            app.language.maven_model.require_restart();
            assert!(app.language.maven_model.status == ModelStatus::ServerExited);
            assert_eq!(app.language.maven_model.sequence, sequence.wrapping_add(1));
        }
        app.check_maven_model();
        tick(&mut app, 100.0);
        assert!(
            rx.try_recv().is_err(),
            "exit must not dispatch a check or restart"
        );
    }
}

#[test]
fn closed_keeps_model_transport_jobs_and_rejects_late_results_before_adoption() {
    for outcome in [
        "imported",
        "unresolved",
        "unavailable",
        "malformed",
        "payload",
        "error",
        "restart",
    ] {
        for (closed_first, connected) in [(true, true), (false, true), (true, false)] {
            let (mut app, rx) = app();
            start(&mut app, &rx);
            let command = model_request(&mut app, &rx);
            let request = command.id;
            if closed_first {
                server_closed(&mut app);
                assert!(
                    matches!(app.pending.get(&request), Some(crate::Job::Language(action)) if action.is_maven_model())
                );
                assert!(app.language.maven_model.pending.is_none());
            }
            let mut value = model_value();
            let result = match outcome {
                "imported" => language(value),
                "unresolved" => {
                    value["status"] = json!("unresolved");
                    value["classpath"][0]["resolved"] = json!(false);
                    value["unresolved_count"] = json!(1);
                    language(value)
                }
                "unavailable" => {
                    value["status"] = json!("unavailable");
                    language(value)
                }
                "malformed" => language(json!({"private":"雪 raw payload"})),
                "payload" => Ok(Payload::Entries { entries: vec![] }),
                "error" => Err("private 雪 backend error".into()),
                "restart" => Err("language_maven_restart_required: private 雪 detail".into()),
                _ => unreachable!(),
            };
            app.language.cjk_seen = false;
            app.error = Some("existing error".into());
            let output = app.language.output.clone();
            app.apply_event(Event {
                generation: app.generation,
                id: command.id,
                connected,
                result,
            });
            assert!(
                !app.pending.contains_key(&request),
                "reply drains the transport job"
            );
            if !connected {
                assert!(!app.ready());
                assert!(!app.language.running);
                assert!(!app.language.maven_model.active());
                assert!(!app.error.as_deref().unwrap_or("").contains("private"));
            } else {
                if closed_first {
                    assert!(
                        !app.language.cjk_seen,
                        "late {outcome} must not request CJK fonts"
                    );
                    assert_eq!(app.language.output, output, "late {outcome}");
                    assert_eq!(app.error.as_deref(), Some("existing error"));
                } else {
                    server_closed(&mut app);
                }
                assert!(app.ready() && app.language.running);
                assert!(app.language.maven_model.status == ModelStatus::ServerExited);
                assert!(app.language.maven_model.model.is_none());
                assert_eq!(app.language.output, SERVER_EXITED);
            }
            assert!(rx.try_recv().is_err());
        }
    }
}

#[test]
fn pom_read_and_save_acknowledgements_after_exit_cannot_replace_exit_status() {
    for save in [false, true] {
        let (mut app, rx) = app();
        start(&mut app, &rx);
        let changed = "changed on-disk POM";
        let revision = format!("{:x}", Sha256::digest(changed.as_bytes()));
        let result = if save {
            app.documents
                .push(Document::new(1, "pom.xml".into(), POM.into(), pom_hash()));
            crate::editor_state::commit(&app.editor_ctx, &mut app.documents[0], changed.into(), 2);
            app.save_document(1);
            Ok(Payload::Written {
                revision: revision.clone(),
            })
        } else {
            app.open("pom.xml".into(), None);
            Ok(Payload::File {
                path: "pom.xml".into(),
                text: changed.into(),
                revision: revision.clone(),
            })
        };
        let command = rx.try_recv().unwrap();
        assert!(if save {
            matches!(command.op, Operation::Write { .. })
        } else {
            matches!(command.op, Operation::Read { .. })
        });
        assert!(command.id >= app.language.maven_model.observation_request_floor);
        server_closed(&mut app);
        let sequence = app.language.maven_model.sequence;
        reply(&mut app, command, result);
        assert_eq!(app.documents[0].saved_text, changed);
        assert_eq!(
            app.documents[0].revision.as_deref(),
            Some(revision.as_str())
        );
        assert!(app.language.maven_model.status == ModelStatus::ServerExited);
        assert_eq!(app.language.maven_model.sequence, sequence);
        assert_eq!(
            app.language.maven_model.pom_sha256(),
            Some(pom_hash().as_str())
        );
        assert_eq!(app.language.output, SERVER_EXITED);
        if save {
            assert!(matches!(rx.try_recv().unwrap().op, Operation::List { .. }));
        }
        assert!(rx.try_recv().is_err());
    }
}

#[test]
fn explicit_stop_and_reset_clear_exit_while_unverified_cleanup_still_blocks_restart() {
    for outcome in ["graceful", "malformed", "error", "reset"] {
        let (mut app, rx) = app();
        start(&mut app, &rx);
        let old_session = app.language.session;
        let command = model_request(&mut app, &rx);
        let old_context = app.language.maven_model.pending.clone().unwrap();
        server_closed(&mut app);
        reply(&mut app, command, language(model_value()));
        if outcome == "reset" {
            app.language.reset();
        } else {
            app.stop_language();
            let stop = rx.try_recv().unwrap();
            assert!(matches!(stop.op, Operation::LanguageStop));
            let result = match outcome {
                "graceful" => language(
                    json!({"stopped":true,"shutdown":{"status":"graceful","reason":"root_exited","root_exit_code":0,"cleanup_joined":true,"shutdown_response_received":true,"exit_frame_completed":true}}),
                ),
                "malformed" => language(json!({"stopped":true})),
                "error" => Err("private cleanup error".into()),
                _ => unreachable!(),
            };
            reply(&mut app, stop, result);
        }
        assert!(!app.language.running);
        assert!(!app.language.diagnostics_exited);
        assert!(!app.language.maven_model.active());
        assert!(app.language.maven_model.status == ModelStatus::Unchecked);
        assert_ne!(app.language.session, old_session);
        if matches!(outcome, "malformed" | "error") {
            assert!(app.language.restart_blocked);
            assert!(!app.language.output.contains("private"));
            app.start_language();
            assert!(rx.try_recv().is_err());
            continue;
        }
        let fresh_hash = "b".repeat(64);
        start_with_hash(&mut app, &rx, &fresh_hash);
        assert!(app.language.maven_model.status == ModelStatus::Unchecked);
        let fresh = model_request(&mut app, &rx);
        let mut value = model_value();
        value["pom_sha256"] = json!(fresh_hash);
        reply(&mut app, fresh, language(value));
        let output = app.language.output.clone();
        app.apply_language_action(
            Action {
                session: old_session,
                kind: ActionKind::Events,
            },
            json!({"events":[{"type":"closed"}]}),
        );
        app.apply_maven_model_event(
            Action {
                session: old_session,
                kind: ActionKind::MavenModel {
                    context: old_context,
                },
            },
            Err("language_maven_restart_required: stale".into()),
            true,
        );
        assert!(app.language.running && !app.language.diagnostics_exited);
        assert!(app.language.maven_model.status == ModelStatus::Imported);
        assert!(app.language.maven_model.model.is_some());
        assert_eq!(
            app.language.maven_model.pom_sha256(),
            Some(fresh_hash.as_str())
        );
        assert_eq!(app.language.output, output);
        assert!(rx.try_recv().is_err());
    }
}

#[test]
fn actual_model_view_removes_snapshot_rows_after_closed() {
    let (mut app, rx) = app();
    start(&mut app, &rx);
    let command = model_request(&mut app, &rx);
    reply(&mut app, command, language(model_value()));
    for exited in [false, true] {
        if exited {
            server_closed(&mut app);
        }
        let ctx = app.editor_ctx.clone();
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 600.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| app.maven_model_view(ui));
            },
        );
        let text: Vec<_> = model_text(&output)
            .into_iter()
            .map(|shape| shape.galley.job.text.as_str())
            .collect();
        if exited {
            assert_eq!(text, vec![SERVER_EXITED]);
        } else {
            for row in ["Root POM:", "Compiler source:", "Source:", "Library ("] {
                assert!(
                    text.iter().any(|text| text.starts_with(row)),
                    "missing live {row} row"
                );
            }
        }
    }
    assert!(rx.try_recv().is_err());
}
