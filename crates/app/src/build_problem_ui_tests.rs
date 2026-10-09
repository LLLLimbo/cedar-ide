//! Real egui pointer/key input over the production command and editor widgets.
use super::*;
use cedar_tasks::{TaskSnapshot, TaskState};

fn app() -> (CedarApp, Receiver<Command>) {
    let mut app = CedarApp::empty();
    let form = ConnectForm {
        local_root: "/workspace".into(),
        allow_run: true,
        ..Default::default()
    };
    app.workspace_key = Some(form.key());
    app.active_form = Some(form);
    app.root = "/workspace".into();
    app.agent_info = Some(agent_support::full_test_agent());
    app.state = ConnectionState::Ready;
    app.open_form = false;
    app.profiles.draft.program = "/tools/javac".into();
    app.profiles.draft.args = vec!["-d".into(), "build classes".into(), "src/Main.java".into()];
    app.documents.push(Document::new(
        1,
        "src/Main.java".into(),
        "é🐻\r\nsecond\nlast\n".into(),
        "r0".into(),
    ));
    app.active_document = Some(1);
    app.next_document = 2;
    let (worker, commands) = Worker::recording();
    app.worker = Some(worker);
    (app, commands)
}

fn snapshot(id: u64, state: TaskState, stdout: &str, stderr: &str) -> TaskSnapshot {
    TaskSnapshot {
        id,
        state,
        stdout: stdout.into(),
        stderr: stderr.into(),
        exit_code: Some(1),
        windows_exit_code: None,
        truncated: false,
        error: None,
    }
}

fn reply_task(app: &mut CedarApp, command: Command, task: TaskSnapshot) {
    app.apply_event(Event {
        generation: app.generation,
        id: command.id,
        connected: true,
        result: Ok(Payload::RunTask {
            snapshot: serde_json::to_value(task).unwrap(),
        }),
    });
}

fn completed(app: &mut CedarApp, commands: &Receiver<Command>, stdout: &str, stderr: &str) {
    app.run();
    let command = commands.try_recv().unwrap();
    assert!(
        matches!(&command.op, Operation::RunStart { program, args, .. } if program == "/tools/javac" && args == &app.profiles.draft.args)
    );
    reply_task(app, command, snapshot(7, TaskState::Failed, stdout, stderr));
    assert!(app.run_state.problems.is_none());
}

// Production widgets in a stable roomy layout make each pointer target visible.
// The full application frame is separately exercised for reconnect/zero effects.
fn frame(
    app: &mut CedarApp,
    time: f64,
    events: Vec<egui::Event>,
    command_form: bool,
) -> egui::FullOutput {
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
            egui::SidePanel::right("build_test_controls")
                .exact_width(680.0)
                .show(ctx, |ui| {
                    if command_form {
                        app.run_panel(ui);
                    } else {
                        app.build_problems_panel(ui);
                    }
                });
            egui::CentralPanel::default().show(ctx, |ui| app.editor(ui));
            app.finish_profile_actions();
        },
    )
}

fn label_position(output: &egui::FullOutput, label: &str) -> egui::Pos2 {
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
        .unwrap_or_else(|| panic!("missing label {label}"))
}

fn click_at(app: &mut CedarApp, time: f64, at: egui::Pos2, command_form: bool) {
    for (index, pressed) in [true, false].into_iter().enumerate() {
        frame(
            app,
            time + index as f64 * 0.01,
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            command_form,
        );
    }
}

fn click(app: &mut CedarApp, time: f64, label: &str, command_form: bool) {
    let output = frame(app, time, vec![], command_form);
    click_at(
        app,
        time + 0.01,
        label_position(&output, label),
        command_form,
    );
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

fn read_reply(app: &mut CedarApp, command: Command, path: &str, text: &str) {
    assert!(matches!(command.op, Operation::Read { .. }));
    app.apply_event(Event {
        generation: app.generation,
        id: command.id,
        connected: true,
        result: Ok(Payload::File {
            path: path.into(),
            text: text.into(),
            revision: "disk".into(),
        }),
    });
}

#[test]
fn build_problem_extraction_is_explicit_terminal_only_and_bound_to_submitted_command() {
    let (mut app, commands) = app();
    click(&mut app, 0.0, "Extract javac locations", false);
    assert!(app.run_state.problems.is_none());
    assert!(commands.try_recv().is_err());
    app.run();
    let start = commands.try_recv().unwrap();
    let action = match app.pending.get(&start.id).unwrap() {
        Job::Run(action) => *action,
        _ => panic!(),
    };
    reply_task(
        &mut app,
        start,
        snapshot(
            7,
            TaskState::Running,
            "",
            "src/Main.java:2: error: missing token",
        ),
    );
    click(&mut app, 1.0, "Extract javac locations", false);
    assert!(app.run_state.problems.is_none());
    app.apply_run(
        run_ui::Action {
            epoch: action.epoch,
            kind: run_ui::Kind::Poll(7),
        },
        serde_json::to_value(snapshot(
            7,
            TaskState::Failed,
            "",
            "src/Main.java:2: error: missing token",
        ))
        .unwrap(),
    );
    let submitted = app.run_state.submitted.clone().unwrap();
    // Edit the real executable form after the command has finished.
    frame(&mut app, 2.0, vec![], true);
    app.editor_ctx
        .memory_mut(|memory| memory.request_focus(egui::Id::new("task_program")));
    frame(
        &mut app,
        3.0,
        key(egui::Key::A, egui::Modifiers::COMMAND),
        true,
    );
    frame(
        &mut app,
        4.0,
        vec![egui::Event::Text("/tools/other-compiler".into())],
        true,
    );
    assert_eq!(app.profiles.draft.program, "/tools/other-compiler");
    click(&mut app, 5.0, "Add argument", true);
    click(&mut app, 6.0, "Extract javac locations", false);
    assert_eq!(app.run_state.submitted.as_ref(), Some(&submitted));
    let problems = app.run_state.problems.as_ref().unwrap();
    assert_eq!(problems.rows.len(), 1);
    assert_eq!(problems.rows[0].line, 2);
    assert_eq!(problems.rows[0].stream, build_problems::Stream::Stderr);
    assert!(
        commands.try_recv().is_err(),
        "extraction/form input emitted a backend operation"
    );
}

#[test]
fn build_problem_click_reuses_dirty_unicode_crlf_buffer_and_preserves_undo() {
    let (mut app, commands) = app();
    completed(
        &mut app,
        &commands,
        "",
        "src/Main.java:2: error: missing token",
    );
    let baseline = app.documents[0].text.clone();
    app.documents[0].jump_to = Some(app.documents[0].text.chars().count());
    frame(&mut app, 0.0, vec![], false);
    frame(
        &mut app,
        1.0,
        vec![egui::Event::Text("draft".into())],
        false,
    );
    assert_eq!(app.documents[0].text, format!("{baseline}draft"));
    click(&mut app, 2.0, "Extract javac locations", false);
    click(&mut app, 3.0, "src/Main.java:2 · error · stderr:1", false);
    frame(&mut app, 4.0, vec![], false);
    assert_eq!(app.active_document, Some(1));
    assert_eq!(app.documents.len(), 1);
    assert_eq!(app.documents[0].cursor, (2, 1));
    assert_eq!(app.location_history.back.len(), 1);
    assert!(app.location_history.pending.is_none());
    assert_eq!(app.documents[0].saved_text, baseline);
    assert!(app.documents[0].dirty());
    frame(
        &mut app,
        5.0,
        key(egui::Key::Z, egui::Modifiers::COMMAND),
        false,
    );
    assert_eq!(app.documents[0].text, baseline);
    assert!(!app.documents[0].dirty());
    assert!(commands.try_recv().is_err());
}

#[test]
fn build_problem_invalid_historical_line_never_clamps_existing_or_new_buffers() {
    let (mut app, commands) = app();
    completed(
        &mut app,
        &commands,
        "src/Main.java:99: error: old line\nsrc/Missing.java:9: error: old file",
        "",
    );
    app.documents[0].jump_to = Some(1);
    click(&mut app, 0.0, "Extract javac locations", false);
    let cursor = app.documents[0].cursor;
    click(&mut app, 1.0, "src/Main.java:99 · error · stdout:1", false);
    assert_eq!(app.documents[0].cursor, cursor);
    assert!(app
        .run_state
        .problems_message
        .as_deref()
        .unwrap()
        .contains("unavailable"));
    assert!(commands.try_recv().is_err());
    click(
        &mut app,
        2.0,
        "src/Missing.java:9 · error · stdout:2",
        false,
    );
    let command = commands.try_recv().unwrap();
    read_reply(&mut app, command, "src/Missing.java", "one line");
    assert_eq!(app.documents.len(), 1);
    assert_eq!(app.active_document, Some(1));
    assert!(app
        .run_state
        .problems_message
        .as_deref()
        .unwrap()
        .contains("outdated"));
    assert!(commands.try_recv().is_err());
}

#[test]
fn build_problem_new_navigation_discards_late_read_without_opening_a_tab() {
    let (mut app, commands) = app();
    completed(
        &mut app,
        &commands,
        "src/Other.java:2: error: old\nsrc/Main.java:1: warning: current",
        "",
    );
    click(&mut app, 0.0, "Extract javac locations", false);
    click(&mut app, 1.0, "src/Other.java:2 · error · stdout:1", false);
    let old = commands.try_recv().unwrap();
    click(&mut app, 2.0, "src/Main.java:1 · warning · stdout:2", false);
    read_reply(&mut app, old, "src/Other.java", "first\nsecond");
    assert_eq!(app.documents.len(), 1);
    assert_eq!(app.active_document, Some(1));
    assert!(commands.try_recv().is_err());
}

#[test]
fn build_problem_new_task_and_reconnect_invalidate_rows_and_old_reads() {
    for new_task in [true, false] {
        let (mut app, commands) = app();
        completed(&mut app, &commands, "src/Other.java:1: error: old", "");
        click(&mut app, 0.0, "Extract javac locations", false);
        click(&mut app, 1.0, "src/Other.java:1 · error · stdout:1", false);
        let old = commands.try_recv().unwrap();
        let old_generation = app.generation;
        if new_task {
            app.run();
            assert!(matches!(
                commands.try_recv().unwrap().op,
                Operation::RunStart { .. }
            ));
            assert!(app.run_state.problems.is_none());
        } else {
            app.run_state.reset();
            app.generation += 1;
            app.profiles.disconnected();
            app.language.reset();
            assert!(app.run_state.submitted.is_none());
        }
        app.apply_event(Event {
            generation: old_generation,
            id: old.id,
            connected: true,
            result: Ok(Payload::File {
                path: "src/Other.java".into(),
                text: "late".into(),
                revision: "r".into(),
            }),
        });
        click(&mut app, 2.0, "Extract javac locations", false);
        assert!(app.run_state.problems.is_none());
        assert_eq!(app.documents.len(), 1);
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn build_problem_wrong_task_workspace_or_backend_cannot_relabel_locations() {
    let (mut app, commands) = app();
    completed(&mut app, &commands, "src/Main.java:2: error: original", "");
    app.documents.push(Document::new(
        2,
        "other.txt".into(),
        "keep focus".into(),
        "r".into(),
    ));
    app.active_document = Some(2);
    click(&mut app, 0.0, "Extract javac locations", false);
    let navigation = app.navigation_epoch;
    let cursor = app.documents[0].cursor;
    app.root = "/other".into();
    click(&mut app, 1.0, "src/Main.java:2 · error · stdout:1", false);
    assert_eq!(app.active_document, Some(2));
    assert_eq!(app.navigation_epoch, navigation);
    assert_eq!(app.documents[0].cursor, cursor);
    assert!(commands.try_recv().is_err());
    app.root = "/workspace".into();
    app.agent_info.as_mut().unwrap().os = "windows".into();
    click(&mut app, 2.0, "src/Main.java:2 · error · stdout:1", false);
    assert_eq!(app.active_document, Some(2));
    assert_eq!(app.navigation_epoch, navigation);
    assert_eq!(app.documents[0].cursor, cursor);
    assert!(commands.try_recv().is_err());
    app.agent_info.as_mut().unwrap().os = "linux".into();
    app.run_state.snapshot.as_mut().unwrap().id = 999;
    click(&mut app, 3.0, "src/Main.java:2 · error · stdout:1", false);
    assert_eq!(app.active_document, Some(2));
    assert_eq!(app.navigation_epoch, navigation);
    assert_eq!(app.documents[0].cursor, cursor);
    assert!(commands.try_recv().is_err());
}

#[test]
fn build_problem_bounds_and_unsupported_paths_remain_inert() {
    let (mut app, commands) = app();
    let output = format!(
        "/outside/Main.java:1: error: absolute\n../Escape.java:1: warning: outside\n{}",
        "src/Other.java:1: error: bounded\n".repeat(build_problems::MAX_ROWS + 50)
    );
    completed(&mut app, &commands, &output, "");
    app.run_state.snapshot.as_mut().unwrap().truncated = true;
    click(&mut app, 0.0, "Extract javac locations", false);
    let problems = app.run_state.problems.as_ref().unwrap();
    assert_eq!(problems.rows.len(), build_problems::MAX_ROWS);
    assert!(problems.summary.row_limit_reached);
    assert!(problems.summary.skipped_lines > 0);
    click(
        &mut app,
        1.0,
        "/outside/Main.java:1 · error · stdout:1",
        false,
    );
    click(
        &mut app,
        2.0,
        "../Escape.java:1 · warning · stdout:2",
        false,
    );
    assert!(commands.try_recv().is_err());
    let output = frame(&mut app, 3.0, vec![], false);
    label_position(
        &output,
        "Task output was truncated; extracted locations are incomplete",
    );
    label_position(
        &output,
        "Location display limit reached; some locations were skipped",
    );
    app.documents.extend(
        (2..=32).map(|id| Document::new(id, format!("{id}.java"), "text".into(), "r".into())),
    );
    click(&mut app, 4.0, "src/Other.java:1 · error · stdout:3", false);
    assert!(app.error.as_deref().unwrap().contains("32 buffers"));
    assert_eq!(app.documents.len(), 32);
    assert!(commands.try_recv().is_err());
}

#[test]
fn build_problem_windows_navigation_uses_submitted_backend_and_deduplicates_reads() {
    let (mut app, commands) = app();
    app.agent_info.as_mut().unwrap().os = "windows".into();
    completed(&mut app, &commands, "src\\Other.java:2: warning: note", "");
    click(&mut app, 0.0, "Extract javac locations", false);
    click(
        &mut app,
        1.0,
        "src\\Other.java:2 · warning · stdout:1",
        false,
    );
    let command = commands.try_recv().unwrap();
    assert!(matches!(&command.op, Operation::Read { path } if path == "src/Other.java"));
    click(
        &mut app,
        2.0,
        "src\\Other.java:2 · warning · stdout:1",
        false,
    );
    assert!(commands.try_recv().is_err());
    read_reply(&mut app, command, "src/Other.java", "é🐻\r\nsecond");
    frame(&mut app, 3.0, vec![], false);
    assert_eq!(app.documents.len(), 2);
    assert_eq!(app.active().unwrap().cursor, (2, 1));
    assert!(commands.try_recv().is_err());
}

#[test]
fn build_problem_windows_case_variants_fail_closed_before_and_after_read() {
    for late in [false, true] {
        let (mut app, commands) = app();
        app.agent_info.as_mut().unwrap().os = "windows".into();
        completed(&mut app, &commands, "src/main.java:2: error: ambiguous", "");
        let mut target = app.documents.remove(0);
        target.text.push_str("unsaved");
        app.documents.push(Document::new(
            2,
            "other.txt".into(),
            "keep".into(),
            "r".into(),
        ));
        app.active_document = Some(2);
        if !late {
            app.documents.push(target);
        }
        click(&mut app, 0.0, "Extract javac locations", false);
        click(&mut app, 1.0, "src/main.java:2 · error · stdout:1", false);
        if late {
            let command = commands.try_recv().unwrap();
            let mut target = Document::new(
                1,
                "src/Main.java".into(),
                "first\nsecond".into(),
                "r".into(),
            );
            target.text.push_str("unsaved");
            app.documents.push(target);
            read_reply(&mut app, command, "src/main.java", "disk\nsecond");
        }
        assert_eq!(app.active_document, Some(2));
        assert_eq!(app.documents.len(), 2);
        assert!(app
            .documents
            .iter()
            .find(|doc| doc.id == 1)
            .unwrap()
            .text
            .ends_with("unsaved"));
        assert!(app
            .run_state
            .problems_message
            .as_deref()
            .unwrap()
            .contains("different case spelling"));
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn build_problem_disk_notice_shows_current_unsaved_and_unknown_save_counts() {
    let (mut app, commands) = app();
    app.documents[0].text.push_str("draft");
    app.documents[0].interrupted_save =
        interrupted_save::InterruptedSave::capture(&app, &app.documents[0]);
    app.documents[0].saving = true;
    let output = frame(&mut app, 0.0, vec![], true);
    label_position(&output, "Commands read saved files on disk. Unsaved drafts and saves with unknown outcomes may differ from those files.");
    label_position(
        &output,
        "Current buffers: 1 unsaved · 1 save outcomes unknown · 1 saves pending",
    );
    assert!(commands.try_recv().is_err());
}

#[test]
fn build_problem_coalesced_read_keeps_latest_navigation_intent_and_dirty_buffer() {
    for build_last in [false, true] {
        let (mut app, commands) = app();
        completed(&mut app, &commands, "src/Other.java:2: error: old", "");
        click(&mut app, 0.0, "Extract javac locations", false);
        if build_last {
            app.open("src/Other.java".into(), Some(1));
        }
        click(&mut app, 1.0, "src/Other.java:2 · error · stdout:1", false);
        if !build_last {
            app.open("src/Other.java".into(), Some(1));
        }
        let command = commands.try_recv().unwrap();
        assert!(commands.try_recv().is_err());
        // A draft appearing while Read was queued must win over the reply.
        let mut draft = Document::new(
            2,
            "src/Other.java".into(),
            "saved\nsecond".into(),
            "r".into(),
        );
        draft.text = "draft\nsecond".into();
        app.documents.push(draft);
        read_reply(&mut app, command, "src/Other.java", "disk\nsecond");
        frame(&mut app, 2.0, vec![], false);
        assert_eq!(app.documents.len(), 2);
        assert_eq!(app.active_document, Some(2));
        assert_eq!(app.active().unwrap().text, "draft\nsecond");
        assert_eq!(app.active().unwrap().saved_text, "saved\nsecond");
        assert_eq!(
            app.active().unwrap().cursor.0,
            if build_last { 2 } else { 1 }
        );
        assert!(commands.try_recv().is_err());
    }
}
