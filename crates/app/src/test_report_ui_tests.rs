//! Headless pointer/key tests over the production read-only report panel.
use super::*;
use sha2::{Digest, Sha256};

const PATH: &str = "target/surefire-reports/TEST-example.xml";
const REPORT: &str = r#"<testsuite name="Example" tests="4" failures="1" errors="1" skipped="1" time="0.4">
<testcase name="passing" classname="Example" time="0.1"/>
<testcase name="failing" classname="Example" time="0.1"><failure message="expected true" type="AssertionError">trace detail</failure></testcase>
<testcase name="broken" classname="Example" time="0.1"><error message="exception" type="Exception">error detail</error></testcase>
<testcase name="omitted" classname="Example" time="0.1"><skipped message="disabled"/></testcase>
</testsuite>"#;

fn app() -> (CedarApp, Receiver<Command>) {
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
    app.documents.push(Document::new(
        1,
        "src/Main.java".into(),
        "source 雪🐻\r\nsecond\n".into(),
        "a".repeat(64),
    ));
    app.active_document = Some(1);
    app.next_document = 2;
    let (worker, commands) = Worker::recording();
    app.worker = Some(worker);
    (app, commands)
}

fn payload(path: &str, text: &str) -> Payload {
    Payload::File {
        path: path.into(),
        text: text.into(),
        revision: format!("{:x}", Sha256::digest(text.as_bytes())),
    }
}

fn reply(app: &mut CedarApp, command: Command, result: Result<Payload, String>) {
    app.apply_event(Event {
        generation: app.generation,
        id: command.id,
        connected: true,
        result,
    });
}

fn load(app: &mut CedarApp, commands: &Receiver<Command>, path: &str) -> Command {
    app.test_report.path = path.into();
    app.load_test_report();
    let command = commands.try_recv().unwrap();
    assert!(matches!(&command.op, Operation::Read { path: actual } if actual == path));
    assert!(matches!(
        app.pending.get(&command.id),
        Some(Job::TestReportRead(_))
    ));
    command
}

fn frame(app: &mut CedarApp, time: f64, events: Vec<egui::Event>) -> egui::FullOutput {
    let ctx = app.editor_ctx.clone();
    ctx.run(
        egui::RawInput {
            time: Some(time),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1600.0, 1200.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            app.begin_navigation_frame(ctx);
            egui::SidePanel::right("test_results_controls")
                .exact_width(800.0)
                .show(ctx, |ui| app.test_report_panel(ui));
            egui::CentralPanel::default().show(ctx, |ui| app.editor(ui));
        },
    )
}

fn label_rect(output: &egui::FullOutput, label: &str) -> egui::Rect {
    fn find(shape: &egui::epaint::Shape, label: &str) -> Option<egui::Rect> {
        match shape {
            egui::epaint::Shape::Text(text) if text.galley.job.text == label => {
                Some(text.visual_bounding_rect())
            }
            egui::epaint::Shape::Vec(shapes) => shapes.iter().find_map(|shape| find(shape, label)),
            _ => None,
        }
    }
    output
        .shapes
        .iter()
        .find_map(|shape| find(&shape.shape, label))
        .unwrap_or_else(|| panic!("missing label {label}"))
}

fn click(app: &mut CedarApp, time: f64, label: &str) {
    let at = label_rect(&frame(app, time, vec![]), label).center();
    for (index, pressed) in [true, false].into_iter().enumerate() {
        frame(
            app,
            time + 0.01 + index as f64 * 0.01,
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

#[test]
fn test_report_load_is_explicit_read_only_and_available_without_execution_trust() {
    let (mut app, commands) = app();
    frame(&mut app, 0.0, vec![]);
    assert!(commands.try_recv().is_err());
    assert!(app.test_report.path.is_empty());
    assert!(app.test_report.snapshot.is_none());
    app.editor_ctx
        .memory_mut(|memory| memory.request_focus(egui::Id::new("test_report_path")));
    frame(&mut app, 1.0, vec![egui::Event::Text(PATH.into())]);
    frame(&mut app, 2.0, key(egui::Key::Enter, egui::Modifiers::NONE));
    assert!(
        commands.try_recv().is_err(),
        "typing a path must not load it"
    );
    click(&mut app, 3.0, "Load report");
    let command = commands.try_recv().unwrap();
    assert!(matches!(&command.op, Operation::Read { path } if path == PATH));
    assert!(!app.execution_trusted());
    assert!(app.test_report.loading.is_some());
    reply(&mut app, command, Ok(payload(PATH, REPORT)));
    assert_eq!(
        app.test_report
            .snapshot
            .as_ref()
            .unwrap()
            .report
            .counts
            .total,
        4
    );
    assert!(app.test_report.loading.is_none());
    for time in [4.0, 40.0, 400.0] {
        frame(&mut app, time, vec![]);
    }
    assert!(
        commands.try_recv().is_err(),
        "idle frames must not refresh a report"
    );
    assert_eq!(app.documents.len(), 1);
    click(&mut app, 401.0, "Refresh report");
    let refresh = commands.try_recv().unwrap();
    assert!(matches!(refresh.op, Operation::Read { .. }));
    assert!(app.test_report.snapshot.is_none());
    reply(&mut app, refresh, Ok(payload(PATH, REPORT)));
    click(&mut app, 402.0, "Clear report");
    assert!(app.test_report.snapshot.is_none());
    assert!(app.test_report.loading.is_none());
    assert_eq!(app.test_report.path, PATH);
    assert!(commands.try_recv().is_err());
}

#[test]
fn test_report_rejects_non_root_relative_or_ambiguous_paths_before_read() {
    let (mut app, commands) = app();
    for path in [
        "",
        "/tmp/report.xml",
        "../report.xml",
        "a/../report.xml",
        "C:/report.xml",
        "C:report.xml",
        "\\\\host\\report.xml",
        "a\\report.xml",
        "https://example/report.xml",
        "./report.xml",
        "a//report.xml",
        "a/./report.xml",
        "report.xml/",
        "a\0.xml",
        "a\n.xml",
        "a\u{202e}.xml",
    ] {
        app.test_report.path = path.into();
        app.load_test_report();
        assert!(commands.try_recv().is_err(), "accepted {path:?}");
        assert!(app
            .test_report
            .message
            .as_deref()
            .unwrap()
            .contains("root-relative"));
        assert!(app.test_report.loading.is_none());
    }
    app.test_report.path = "a".repeat(4097);
    app.load_test_report();
    assert!(commands.try_recv().is_err());
    let command = load(&mut app, &commands, "中文 folder/report.xml");
    reply(
        &mut app,
        command,
        Ok(payload("中文 folder/report.xml", REPORT)),
    );
    assert!(app.test_report.snapshot.is_some());
}

#[test]
fn test_report_newer_load_wins_and_stale_errors_cannot_replace_it() {
    let (mut app, commands) = app();
    let old = load(&mut app, &commands, PATH);
    let old_load = app.test_report.loading.clone().unwrap();
    let newer = load(&mut app, &commands, PATH);
    let newer_load = app.test_report.loading.clone().unwrap();
    assert!(newer_load.id > old_load.id);
    let newer_text = REPORT.replace("Example", "Newest");
    reply(&mut app, newer, Ok(payload(PATH, &newer_text)));
    reply(&mut app, old, Err("late failure".into()));
    assert_eq!(
        app.test_report.snapshot.as_ref().unwrap().report.suite_name,
        "Newest"
    );
    assert!(app.test_report.message.is_none());
    app.apply_test_report_read(old_load, Ok(payload(PATH, REPORT)));
    assert_eq!(
        app.test_report.snapshot.as_ref().unwrap().source,
        newer_load
    );
    assert!(commands.try_recv().is_err());
}

#[test]
fn test_report_path_edit_and_clear_invalidate_late_success_and_failure() {
    let (mut app, commands) = app();
    let old = load(&mut app, &commands, PATH);
    frame(&mut app, 0.0, vec![]);
    app.editor_ctx
        .memory_mut(|memory| memory.request_focus(egui::Id::new("test_report_path")));
    frame(&mut app, 1.0, key(egui::Key::A, egui::Modifiers::COMMAND));
    frame(&mut app, 2.0, vec![egui::Event::Text("other.xml".into())]);
    assert_eq!(app.test_report.path, "other.xml");
    reply(&mut app, old, Ok(payload(PATH, REPORT)));
    assert!(app.test_report.snapshot.is_none());
    assert!(app.test_report.message.is_none());
    let next = load(&mut app, &commands, "other.xml");
    let id = app.test_report.loading.as_ref().unwrap().id;
    click(&mut app, 3.0, "Clear report");
    reply(&mut app, next, Err("late read error".into()));
    assert!(app.test_report.message.is_none());
    let final_read = load(&mut app, &commands, PATH);
    assert!(app.test_report.loading.as_ref().unwrap().id > id);
    reply(&mut app, final_read, Ok(payload(PATH, REPORT)));
    assert_eq!(app.test_report.snapshot.as_ref().unwrap().source.path, PATH);
}

#[test]
fn test_report_repeated_load_clear_and_edit_bound_outstanding_jobs() {
    let (mut app, commands) = app();
    let first = load(&mut app, &commands, PATH);
    app.test_report.clear();
    let second = load(&mut app, &commands, "other.xml");
    for index in 0..100 {
        app.test_report.clear();
        app.test_report.path = format!("report-{index}.xml");
        app.test_report.path_edited();
        app.load_test_report();
    }
    assert!(commands.try_recv().is_err());
    assert_eq!(app.pending.len(), 2);
    reply(&mut app, first, Ok(payload(PATH, REPORT)));
    let latest = load(&mut app, &commands, "last.xml");
    reply(&mut app, latest, Ok(payload("last.xml", REPORT)));
    reply(&mut app, second, Ok(payload("other.xml", REPORT)));
    assert!(app.pending.is_empty());
    assert_eq!(
        app.test_report.snapshot.as_ref().unwrap().source.path,
        "last.xml"
    );
}

#[test]
fn test_report_old_connection_event_cannot_publish_but_current_transport_loss_is_visible() {
    let (mut app, commands) = app();
    let old = load(&mut app, &commands, PATH);
    let old_generation = app.generation;
    app.disconnected("synthetic disconnect".into());
    app.generation += 1;
    let (worker, reconnected) = Worker::recording();
    app.worker = Some(worker);
    app.state = ConnectionState::Ready;
    app.agent_info = Some(agent_support::full_test_agent());
    let newer = load(&mut app, &reconnected, PATH);
    app.apply_event(Event {
        generation: old_generation,
        id: old.id,
        connected: false,
        result: Ok(payload(PATH, REPORT)),
    });
    assert!(app.ready());
    assert!(app.test_report.snapshot.is_none());
    reply(&mut app, newer, Ok(payload(PATH, REPORT)));
    let stale = load(&mut app, &reconnected, PATH);
    app.test_report.clear();
    app.apply_event(Event {
        generation: app.generation,
        id: stale.id,
        connected: false,
        result: Err("wire closed".into()),
    });
    assert!(!app.ready());
    assert!(app.test_report.snapshot.is_none());
    assert!(app.error.as_deref().unwrap().contains("connection closed"));
    assert_eq!(app.documents.len(), 1);
}

#[test]
fn test_report_rejects_wrong_payload_path_revision_xml_and_overflow_without_old_success() {
    let (mut app, commands) = app();
    let wrong_revision = Payload::File {
        path: PATH.into(),
        text: REPORT.into(),
        revision: "not-a-revision".into(),
    };
    let failures = vec![
        Ok(payload("wrong.xml", REPORT)),
        Ok(wrong_revision),
        Ok(Payload::File {
            path: PATH.into(),
            text: REPORT.into(),
            revision: "0".repeat(64),
        }),
        Ok(Payload::Entries { entries: vec![] }),
        Ok(payload(PATH, "<testsuite>")),
        Ok(payload(
            PATH,
            &"x".repeat(test_reports::MAX_REPORT_BYTES + 1),
        )),
        Err("not found".into()),
    ];
    for failure in failures {
        let success = load(&mut app, &commands, PATH);
        reply(&mut app, success, Ok(payload(PATH, REPORT)));
        assert!(app.test_report.snapshot.is_some());
        let rejected = load(&mut app, &commands, PATH);
        assert!(app.test_report.snapshot.is_none());
        reply(&mut app, rejected, failure);
        assert!(app.test_report.snapshot.is_none());
        assert!(app.test_report.loading.is_none());
        assert!(app
            .test_report
            .message
            .as_deref()
            .unwrap()
            .starts_with("Report not loaded:"));
        assert!(app.ready());
        assert_eq!(app.documents.len(), 1);
    }
}

#[test]
fn test_report_filter_and_selection_keep_original_case_and_historical_revision() {
    let (mut app, commands) = app();
    let command = load(&mut app, &commands, PATH);
    reply(&mut app, command, Ok(payload(PATH, REPORT)));
    click(&mut app, 0.0, "Failed");
    assert_eq!(app.test_report.visible_cases(), vec![1]);
    click(&mut app, 1.0, "failing · Failed · 0.100 s");
    assert_eq!(app.test_report.selected, Some(1));
    let output = frame(&mut app, 2.0, vec![]);
    label_rect(&output, "expected true");
    label_rect(&output, "trace detail");
    label_rect(&output, "Type: AssertionError");
    label_rect(&output, &format!("Path: {PATH}"));
    label_rect(
        &output,
        &format!("Source revision: {:x}", Sha256::digest(REPORT.as_bytes())),
    );
    label_rect(&output, "Historical report snapshot. It does not establish the current source or test state. Refresh report explicitly to read disk again");
    click(&mut app, 3.0, "Skipped");
    assert_eq!(app.test_report.visible_cases(), vec![3]);
    assert!(app.test_report.selected.is_none());
    click(&mut app, 4.0, "All");
    app.editor_ctx
        .memory_mut(|memory| memory.request_focus(egui::Id::new("test_report_filter")));
    frame(&mut app, 5.0, vec![egui::Event::Text("broken".into())]);
    assert_eq!(app.test_report.visible_cases(), vec![2]);
    app.test_report.select(0);
    assert!(app.test_report.selected.is_none());
    click(&mut app, 6.0, "broken · Error · 0.100 s");
    assert_eq!(app.test_report.selected, Some(2));
    assert_eq!(app.documents.len(), 1);
    assert_eq!(app.active_document, Some(1));
    assert!(commands.try_recv().is_err());
}

#[test]
fn test_report_snapshot_does_not_touch_same_path_dirty_editor_or_native_undo() {
    let (mut app, commands) = app();
    app.documents[0].path = PATH.into();
    let baseline = app.documents[0].text.clone();
    app.documents[0].jump_to = Some(baseline.chars().count());
    frame(&mut app, 0.0, vec![]);
    frame(
        &mut app,
        1.0,
        vec![egui::Event::Text("unsaved report text 雪".into())],
    );
    let draft = app.documents[0].text.clone();
    assert_ne!(draft, baseline);
    let version = app.documents[0].edit_version;
    let revision = app.documents[0].revision.clone();
    let cursor = app.documents[0].cursor;
    app.test_report.path = PATH.into();
    click(&mut app, 2.0, "Load report");
    let command = commands.try_recv().unwrap();
    reply(&mut app, command, Ok(payload(PATH, REPORT)));
    click(&mut app, 3.0, "failing · Failed · 0.100 s");
    assert_eq!(app.documents.len(), 1);
    assert_eq!(app.active_document, Some(1));
    assert_eq!(app.documents[0].text, draft);
    assert_eq!(app.documents[0].saved_text, baseline);
    assert_eq!(app.documents[0].edit_version, version);
    assert_eq!(app.documents[0].revision, revision);
    assert_eq!(app.documents[0].cursor, cursor);
    app.editor_ctx
        .memory_mut(|memory| memory.request_focus(egui::Id::new(("editor", 1_u64))));
    frame(&mut app, 4.0, key(egui::Key::Z, egui::Modifiers::COMMAND));
    assert_eq!(app.documents[0].text, baseline);
    frame(
        &mut app,
        5.0,
        key(
            egui::Key::Z,
            egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
        ),
    );
    assert_eq!(app.documents[0].text, draft);
    assert!(app.documents[0].dirty());
    assert!(commands.try_recv().is_err());
}

#[test]
fn test_report_controls_and_entry_are_reachable_at_narrow_native_layout() {
    let (mut app, commands) = app();
    let ctx = app.editor_ctx.clone();
    app.tools_open = true;
    app.tool = Tool::Tests;
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(760.0, 700.0),
            )),
            ..Default::default()
        },
        |ctx| {
            app.sidebar(ctx);
            app.tools(ctx);
            egui::CentralPanel::default().show(ctx, |_| {});
        },
    );
    for label in [
        "Tests",
        "TEST RESULTS",
        "Load report",
        "Refresh report",
        "Clear report",
    ] {
        let rect = label_rect(&output, label);
        assert!(
            rect.min.x >= 0.0 && rect.max.x <= 760.0,
            "offscreen {label}: {rect:?}"
        );
        assert!(
            rect.min.y >= 0.0 && rect.max.y <= 700.0,
            "offscreen {label}: {rect:?}"
        );
    }
    assert!(commands.try_recv().is_err());
}

#[test]
fn test_report_zero_and_flaky_outcomes_never_display_invented_success() {
    let (mut app, commands) = app();
    let empty = r#"<testsuite name="empty" tests="0" failures="0" errors="0" skipped="0"/>"#;
    let command = load(&mut app, &commands, PATH);
    reply(&mut app, command, Ok(payload(PATH, empty)));
    let output = frame(&mut app, 0.0, vec![]);
    label_rect(
        &output,
        "No test outcomes in this report; success cannot be inferred",
    );
    assert_eq!(
        app.test_report
            .snapshot
            .as_ref()
            .unwrap()
            .report
            .counts
            .passed,
        0
    );

    let flaky = r#"<testsuite name="flaky" tests="1" failures="0" errors="0" skipped="0" flakes="1"><testcase name="retry" time="0.2"><flakyFailure message="attempt"><stackTrace>retry trace</stackTrace></flakyFailure></testcase></testsuite>"#;
    let command = load(&mut app, &commands, PATH);
    reply(&mut app, command, Ok(payload(PATH, flaky)));
    let output = frame(&mut app, 1.0, vec![]);
    label_rect(
        &output,
        "Unsupported or flaky outcomes are not counted as passed",
    );
    let counts = &app.test_report.snapshot.as_ref().unwrap().report.counts;
    assert_eq!(counts.passed, 0);
    assert_eq!(counts.unsupported, 1);
    click(&mut app, 2.0, "Unsupported / flaky");
    assert_eq!(app.test_report.visible_cases(), vec![0]);
    click(&mut app, 3.0, "retry · Unsupported / flaky · 0.200 s");
    let output = frame(&mut app, 4.0, vec![]);
    label_rect(&output, "attempt");
    label_rect(&output, "retry trace");
    assert!(commands.try_recv().is_err());
}

#[test]
fn test_report_read_and_clear_preserve_owned_recovery_of_same_path_draft() {
    use cedar_recovery::{record_id, Store, WorkspaceIdentity};
    use std::time::{Duration, Instant};

    fn wait(app: &mut CedarApp, done: impl Fn(&CedarApp) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            app.recovery_tick(&egui::Context::default());
            if done(app) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "recovery timed out: {:?}",
                app.recovery.error
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    let (mut app, commands) = app();
    let temp = tempfile::tempdir().unwrap();
    let store_path = temp.path().join("report-recovery");
    app.recovery
        .start(Ok(store_path.clone()), &egui::Context::default());
    wait(&mut app, |app| app.recovery.initialized);
    let workspace = WorkspaceIdentity::Local {
        root: "/workspace".into(),
    };
    app.documents[0].path = PATH.into();
    app.documents[0].text.push_str("unsaved local report draft");
    app.documents[0].edit_version += 1;
    let text = app.documents[0].text.clone();
    let baseline = app.documents[0].saved_text.clone();
    let revision = app.documents[0].revision.clone();
    app.recovery_tick(&egui::Context::default());
    app.recovery.flush();
    wait(&mut app, |app| {
        app.recovery.protected(&workspace, &app.documents[0])
    });
    let command = load(&mut app, &commands, PATH);
    reply(&mut app, command, Ok(payload(PATH, REPORT)));
    click(&mut app, 0.0, "failing · Failed · 0.100 s");
    click(&mut app, 1.0, "Clear report");
    app.recovery_tick(&egui::Context::default());
    assert!(app.recovery.protected(&workspace, &app.documents[0]));
    assert_eq!(
        app.recovery
            .status(Some(&workspace), Some(&app.documents[0])),
        ("Draft backed up locally", true)
    );
    assert_eq!(app.documents[0].text, text);
    assert_eq!(app.documents[0].saved_text, baseline);
    assert_eq!(app.documents[0].revision, revision);
    assert!(commands.try_recv().is_err());
    drop(app);
    let store = Store::open(store_path).unwrap();
    let saved = store.read(&record_id(&workspace, PATH).unwrap()).unwrap();
    assert_eq!(saved.text, text);
    assert_eq!(saved.base_text, baseline);
    assert_eq!(saved.base_revision, revision);
}
