use super::*;
use crate::{
    editor_state,
    model::Document,
    worker::{Command, Event, Worker},
    ConnectForm, ConnectionState, Tool,
};
use std::sync::mpsc::Receiver;

fn connected() -> (CedarApp, Receiver<Command>) {
    let mut app = CedarApp::empty();
    let form = ConnectForm {
        local_root: "/project".into(),
        allow_run: true,
        ..Default::default()
    };
    app.workspace_key = Some(form.key());
    app.active_form = Some(form);
    app.root = "/project".into();
    app.state = ConnectionState::Ready;
    app.generation = 7;
    let mut info = crate::agent_support::full_test_agent();
    info.capabilities
        .extend(["git_changes".into(), "git_diff".into()]);
    app.agent_info = Some(info);
    app.set_git_program("/usr/bin/git".into());
    app.documents.push(Document::new(
        1,
        "a.rs".into(),
        "saved".into(),
        "revision".into(),
    ));
    app.active_document = Some(1);
    app.next_document = 2;
    let (worker, rx) = Worker::recording();
    app.worker = Some(worker);
    (app, rx)
}

fn file(path: &str) -> GitChange {
    GitChange {
        path: path.into(),
        index: 'M',
        worktree: 'M',
        kind: GitChangeKind::File,
        can_diff_staged: true,
        can_diff_unstaged: true,
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

fn status(app: &mut CedarApp, rx: &Receiver<Command>, entries: Vec<GitChange>) {
    app.refresh_git_changes();
    let command = rx.try_recv().unwrap();
    assert!(
        matches!(&command.op, Operation::GitChanges { git_executable } if git_executable == "/usr/bin/git")
    );
    reply(app, command, Ok(Payload::GitChanges { entries }));
    assert!(app.git_state.status_ready);
}

fn diff_reply(command: &Command, text: &str) -> Payload {
    let Operation::GitDiff { path, kind, .. } = &command.op else {
        panic!("expected diff")
    };
    Payload::GitDiff {
        path: path.clone(),
        kind: *kind,
        text: text.into(),
    }
}

fn patch(app: &CedarApp) -> Option<&str> {
    app.git_state
        .patch
        .as_ref()
        .map(|patch| patch.text.as_str())
}

fn text_rect(output: &egui::FullOutput, expected: &str) -> Option<egui::Rect> {
    fn find(shape: &egui::epaint::Shape, expected: &str) -> Option<egui::Rect> {
        match shape {
            egui::epaint::Shape::Text(text) if text.galley.job.text == expected => {
                Some(text.visual_bounding_rect())
            }
            egui::epaint::Shape::Vec(shapes) => {
                shapes.iter().find_map(|shape| find(shape, expected))
            }
            _ => None,
        }
    }
    output
        .shapes
        .iter()
        .find_map(|shape| find(&shape.shape, expected))
}

fn frame(
    app: &mut CedarApp,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
    sidebar: bool,
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1320.0, 880.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            if sidebar {
                app.sidebar(ctx);
                app.tools(ctx);
            } else {
                egui::CentralPanel::default().show(ctx, |ui| app.git_panel(ui));
            }
        },
    )
}

fn click(app: &mut CedarApp, ctx: &egui::Context, at: egui::Pos2, sidebar: bool) {
    for pressed in [true, false] {
        frame(
            app,
            ctx,
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            sidebar,
        );
    }
}

fn history(app: &mut CedarApp, redo: bool) {
    let ctx = app.editor_ctx.clone();
    let _ = ctx.run(
        egui::RawInput {
            events: vec![egui::Event::Key {
                key: egui::Key::Z,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: if redo {
                    egui::Modifiers::COMMAND | egui::Modifiers::SHIFT
                } else {
                    egui::Modifiers::COMMAND
                },
            }],
            ..Default::default()
        },
        |ctx| {
            ctx.memory_mut(|memory| memory.request_focus(egui::Id::new(("editor", 1u64))));
            editor_state::history_shortcut(ctx, &mut app.documents[0]);
        },
    );
}

#[test]
fn trust_and_both_capabilities_are_required_without_inferred_permission() {
    for missing in ["trust", "git_changes", "git_diff", "metadata", "connection"] {
        let (mut app, rx) = connected();
        match missing {
            "trust" => {
                app.active_form.as_mut().unwrap().allow_run = false;
                app.form.allow_run = true; // Editing the next connection never grants trust.
            }
            "metadata" => app.agent_info = None,
            "connection" => app.state = ConnectionState::Disconnected,
            cap => app
                .agent_info
                .as_mut()
                .unwrap()
                .capabilities
                .retain(|name| name != cap),
        }
        app.refresh_git_changes();
        assert!(rx.try_recv().is_err(), "{missing}");
        assert!(app.git_state.message.is_some(), "{missing}");
        assert!(app.git_state.request.is_none());
        assert!(!app.git_state.status_ready);
        assert_eq!(app.documents[0].text, "saved");
    }
    let (mut app, rx) = connected();
    app.active_form.as_mut().unwrap().allow_run = false;
    for op in [
        Operation::GitChanges {
            git_executable: "/usr/bin/git".into(),
        },
        Operation::GitDiff {
            git_executable: "/usr/bin/git".into(),
            path: "a.rs".into(),
            kind: GitDiffKind::Staged,
        },
    ] {
        assert!(app.operation_problem(&op).is_some());
        assert_eq!(app.request(op, Job::Git), 0);
    }
    assert!(rx.try_recv().is_err());
}

#[test]
fn program_must_be_explicit_and_is_transmitted_exactly_for_either_host_platform() {
    for invalid in ["", "git", "bin/git", "C:git.exe", "/bad\0git"] {
        let (mut app, rx) = connected();
        app.set_git_program(invalid.into());
        app.refresh_git_changes();
        assert!(rx.try_recv().is_err(), "{invalid:?}");
    }
    for program in [
        "/opt/a b/git ",
        "C:\\Program Files\\Git\\cmd\\git.exe",
        "\\\\server\\share\\git.exe",
    ] {
        let (mut app, rx) = connected();
        app.set_git_program(program.into());
        app.refresh_git_changes();
        assert!(
            matches!(rx.try_recv().unwrap().op, Operation::GitChanges { git_executable } if git_executable == program)
        );
    }
}

#[test]
fn new_refresh_rejects_older_status_success_and_failure() {
    for error in [false, true] {
        let (mut app, rx) = connected();
        app.refresh_git_changes();
        let old = rx.try_recv().unwrap();
        app.refresh_git_changes();
        let new = rx.try_recv().unwrap();
        reply(
            &mut app,
            new,
            Ok(Payload::GitChanges {
                entries: vec![file("new.rs")],
            }),
        );
        reply(
            &mut app,
            old,
            if error {
                Err("old failure".into())
            } else {
                Ok(Payload::GitChanges {
                    entries: vec![file("old.rs")],
                })
            },
        );
        assert_eq!(app.git_state.entries, [file("new.rs")]);
        assert!(app.git_state.message.is_none());
        assert!(app.git_state.request.is_none());
        assert!(app.error.is_none());
    }
}

#[test]
fn program_edit_and_return_to_same_text_invalidate_status_success_and_failure() {
    for error in [false, true] {
        let (mut app, rx) = connected();
        app.refresh_git_changes();
        let old = rx.try_recv().unwrap();
        app.set_git_program("/other/git".into());
        app.set_git_program("/usr/bin/git".into());
        app.git_state.message = Some("current config".into());
        reply(
            &mut app,
            old,
            if error {
                Err("old failure".into())
            } else {
                Ok(Payload::GitChanges {
                    entries: vec![file("old.rs")],
                })
            },
        );
        assert!(app.git_state.entries.is_empty());
        assert!(!app.git_state.status_ready);
        assert_eq!(app.git_state.message.as_deref(), Some("current config"));
        assert!(app.git_state.request.is_none());
    }
}

#[test]
fn exact_path_and_kind_selection_rejects_late_patches_and_errors() {
    for change_kind in [false, true] {
        for error in [false, true] {
            let (mut app, rx) = connected();
            status(&mut app, &rx, vec![file("a.rs"), file("b.rs")]);
            app.select_git_diff("a.rs".into(), GitDiffKind::Staged);
            let old = rx.try_recv().unwrap();
            app.select_git_diff(
                if change_kind { "a.rs" } else { "b.rs" }.into(),
                GitDiffKind::Unstaged,
            );
            let new = rx.try_recv().unwrap();
            let result = diff_reply(&new, "new patch");
            reply(&mut app, new, Ok(result));
            let result = if error {
                Err("old diff error".into())
            } else {
                Ok(diff_reply(&old, "old patch"))
            };
            reply(&mut app, old, result);
            assert_eq!(patch(&app), Some("new patch"));
            assert!(app.git_state.message.is_none());
            assert!(app.error.is_none());
        }
    }
}

#[test]
fn selecting_a_status_only_row_invalidates_the_previous_diff() {
    let (mut app, rx) = connected();
    let mut untracked = file("new file.txt");
    untracked.kind = GitChangeKind::Untracked;
    untracked.index = '?';
    untracked.worktree = '?';
    untracked.can_diff_staged = false;
    untracked.can_diff_unstaged = false;
    status(&mut app, &rx, vec![file("a.rs"), untracked]);
    app.select_git_diff("a.rs".into(), GitDiffKind::Staged);
    let old = rx.try_recv().unwrap();
    app.select_git_entry("new file.txt".into());
    let result = diff_reply(&old, "old patch");
    reply(&mut app, old, Ok(result));
    assert_eq!(patch(&app), None);
    assert!(app.git_state.message.is_none());
    assert!(app.git_state.request.is_none());
    assert!(rx.try_recv().is_err());
}

#[test]
fn refresh_config_and_reconnect_invalidate_pending_diffs() {
    for invalidation in ["refresh", "config", "reconnect"] {
        for error in [false, true] {
            let (mut app, rx) = connected();
            status(&mut app, &rx, vec![file("a.rs")]);
            app.select_git_diff("a.rs".into(), GitDiffKind::Staged);
            let old = rx.try_recv().unwrap();
            let generation = app.generation;
            match invalidation {
                "refresh" => app.refresh_git_changes(),
                "config" => app.set_git_program("/new/git".into()),
                "reconnect" => {
                    app.generation += 1;
                    app.reset_git(false);
                }
                _ => unreachable!(),
            }
            app.error = Some("current unrelated error".into());
            app.apply_event(Event {
                generation,
                id: old.id,
                connected: true,
                result: if error {
                    Err("old diff error".into())
                } else {
                    Ok(diff_reply(&old, "old patch"))
                },
            });
            assert_eq!(patch(&app), None);
            assert_eq!(app.error.as_deref(), Some("current unrelated error"));
            assert!(app.git_state.message.is_none());
        }
    }
}

#[test]
fn reconnect_reset_clears_results_and_endpoint_switch_clears_program() {
    for switch_endpoint in [false, true] {
        let (mut app, rx) = connected();
        status(&mut app, &rx, vec![file("a.rs")]);
        app.select_git_diff("a.rs".into(), GitDiffKind::Staged);
        let old = rx.try_recv().unwrap();
        let old_generation = app.generation;
        let info = app.agent_info.clone();
        let mut form = app.active_form.clone().unwrap();
        form.allow_run = false;
        if switch_endpoint {
            form.local_root = "/next".into();
        }
        app.disconnected("connection lost".into());
        assert!(app.git_state.entries.is_empty());
        assert!(!app.git_state.status_ready);
        app.generation += 1;
        app.state = ConnectionState::Connecting;
        app.connecting_form = Some(form.clone());
        let (worker, fresh) = Worker::recording();
        app.worker = Some(worker);
        app.apply_event(Event {
            generation: app.generation,
            id: 0,
            connected: true,
            result: Ok(Payload::Hello {
                protocol: cedar_protocol::PROTOCOL_VERSION,
                root: form.local_root,
                agent: info,
            }),
        });
        assert!(app.ready());
        assert!(!app.execution_trusted());
        assert_eq!(
            app.git_state.program,
            if switch_endpoint { "" } else { "/usr/bin/git" }
        );
        assert!(matches!(
            fresh.try_recv().unwrap().op,
            Operation::List { .. }
        ));
        assert!(fresh.try_recv().is_err());
        app.apply_event(Event {
            generation: old_generation,
            id: old.id,
            connected: false,
            result: Err("late previous-connection failure".into()),
        });
        assert!(app.ready());
        assert!(app.error.is_none());
        assert!(app.git_state.entries.is_empty());
        assert_eq!(patch(&app), None);
    }
}

#[test]
fn failed_reads_and_mismatched_payloads_clear_loading_without_partial_results() {
    for failure in [0, 1, 2, 3, 4] {
        let (mut app, rx) = connected();
        status(&mut app, &rx, vec![file("a.rs")]);
        app.select_git_diff("a.rs".into(), GitDiffKind::Staged);
        let command = rx.try_recv().unwrap();
        let result = match failure {
            0 => Err("Git timed out".into()),
            1 => Ok(Payload::GitDiff {
                path: "other.rs".into(),
                kind: GitDiffKind::Staged,
                text: "wrong path".into(),
            }),
            2 => Ok(Payload::GitDiff {
                path: "a.rs".into(),
                kind: GitDiffKind::Unstaged,
                text: "wrong kind".into(),
            }),
            3 => Ok(diff_reply(&command, &"x".repeat(MAX_OUTPUT_BYTES + 1))),
            _ => Ok(Payload::GitChanges { entries: vec![] }),
        };
        reply(&mut app, command, result);
        assert_eq!(patch(&app), None);
        assert!(app.git_state.request.is_none());
        assert!(app.git_state.message.is_some());
        assert!(app.pending.is_empty());
        assert!(app.git_state.status_ready);
    }
    let (mut app, rx) = connected();
    status(&mut app, &rx, vec![file("a.rs")]);
    app.refresh_git_changes();
    reply(
        &mut app,
        rx.try_recv().unwrap(),
        Err("status failed".into()),
    );
    assert!(!app.git_state.status_ready);
    assert!(app.git_state.entries.is_empty());
    assert!(app.git_state.request.is_none());
}

#[test]
fn invalid_or_oversized_status_is_not_reported_as_clean() {
    let invalid = [
        vec![file("../outside")],
        vec![file(".git/config")],
        vec![file("/absolute")],
        vec![file("a.rs"), file("a.rs")],
        vec![file(&"a".repeat(MAX_PATH_BYTES + 1))],
        (0..=MAX_ENTRIES)
            .map(|i| file(&format!("file{i}")))
            .collect(),
        vec![GitChange {
            kind: GitChangeKind::Conflict,
            ..file("conflict")
        }],
    ];
    for entries in invalid {
        let (mut app, rx) = connected();
        app.refresh_git_changes();
        reply(
            &mut app,
            rx.try_recv().unwrap(),
            Ok(Payload::GitChanges { entries }),
        );
        assert!(!app.git_state.status_ready);
        assert!(app.git_state.entries.is_empty());
        assert!(app.git_state.message.as_ref().unwrap().contains("Invalid"));
        assert!(app.git_state.request.is_none());
    }
}

#[test]
fn status_only_entries_never_dispatch_diffs_even_if_peer_flags_are_wrong() {
    for kind in [
        GitChangeKind::Untracked,
        GitChangeKind::Conflict,
        GitChangeKind::Unsupported,
    ] {
        let (mut app, rx) = connected();
        // Bypass response validation to test the dispatch guard itself.
        app.git_state.entries = vec![GitChange {
            kind,
            ..file("a.rs")
        }];
        for diff_kind in [GitDiffKind::Staged, GitDiffKind::Unstaged] {
            app.select_git_diff("a.rs".into(), diff_kind);
            assert!(rx.try_recv().is_err());
            assert!(app.git_state.request.is_none());
        }
    }
}

#[test]
fn escaped_labels_never_change_wire_paths_and_diffs_do_not_touch_drafts_or_undo() {
    let (mut app, rx) = connected();
    let path = "nested/你好 tab\tline\n-literal*[x]\\n.txt";
    assert_ne!(path_label(path), path);
    assert!(!path_label(path).contains('\n'));
    assert!(path_label(path).contains("\\n"));
    assert_ne!(path_label("a\nb"), path_label("a\\nb"));
    editor_state::commit(
        &app.editor_ctx,
        &mut app.documents[0],
        "unsaved draft".into(),
        2,
    );
    let before = format!("{:?}", app.documents[0]);
    let navigation = app.navigation_epoch;
    status(&mut app, &rx, vec![file(path)]);
    for kind in [GitDiffKind::Staged, GitDiffKind::Unstaged] {
        app.select_git_diff(path.into(), kind);
        let command = rx.try_recv().unwrap();
        assert!(
            matches!(&command.op, Operation::GitDiff { path: actual, kind: actual_kind, git_executable } if actual == path && *actual_kind == kind && git_executable == "/usr/bin/git")
        );
        let result = diff_reply(&command, "-disk\n+index\n");
        reply(&mut app, command, Ok(result));
        assert_eq!(patch(&app), Some("-disk\n+index\n"));
        assert_eq!(format!("{:?}", app.documents[0]), before);
        assert_eq!(app.navigation_epoch, navigation);
        assert_eq!(app.active_document, Some(1));
        assert!(rx.try_recv().is_err());
    }
    history(&mut app, false);
    assert_eq!(app.documents[0].text, "saved");
    history(&mut app, true);
    assert_eq!(app.documents[0].text, "unsaved draft");
    assert_eq!(app.documents[0].revision.as_deref(), Some("revision"));
}

#[test]
fn repeated_requests_are_bounded_and_rejected_attempts_do_not_revive_old_results() {
    let (mut app, rx) = connected();
    for _ in 0..100 {
        app.refresh_git_changes();
    }
    let commands: Vec<_> = rx.try_iter().collect();
    assert_eq!(commands.len(), MAX_PENDING_READS);
    assert_eq!(app.pending.len(), MAX_PENDING_READS);
    assert!(app.git_state.request.is_none());
    for command in commands {
        reply(
            &mut app,
            command,
            Ok(Payload::GitChanges { entries: vec![] }),
        );
    }
    assert!(!app.git_state.status_ready);
    assert!(app.pending.is_empty());
    app.refresh_git_changes();
    assert!(rx.try_recv().is_ok());
}

#[test]
fn transport_loss_invalidates_state_even_after_selection_changes() {
    let (mut app, rx) = connected();
    status(&mut app, &rx, vec![file("a.rs")]);
    app.select_git_diff("a.rs".into(), GitDiffKind::Staged);
    let command = rx.try_recv().unwrap();
    app.set_git_program("/new/git".into());
    app.apply_event(Event {
        generation: app.generation,
        id: command.id,
        connected: false,
        result: Err("connection lost".into()),
    });
    assert!(app.state == ConnectionState::Disconnected);
    assert!(app.git_state.entries.is_empty());
    assert!(app.git_state.request.is_none());
    assert_eq!(patch(&app), None);
    assert!(app.pending.is_empty());
    assert_eq!(app.documents[0].text, "saved");
}

#[test]
fn dispatch_failure_leaves_no_loading_or_old_patch() {
    let (mut app, rx) = connected();
    drop(rx);
    app.refresh_git_changes();
    assert!(app.state == ConnectionState::Disconnected);
    assert!(app.git_state.request.is_none());
    assert!(!app.git_state.status_ready);
    assert!(app.git_state.message.is_some());
}

#[test]
fn opening_panel_and_idle_frames_never_issue_git_reads_and_legacy_fallback_remains_explicit() {
    for typed in [true, false] {
        let (mut app, rx) = connected();
        if !typed {
            app.agent_info = Some(crate::agent_support::full_test_agent());
        }
        app.tool = Tool::Git;
        app.tools_open = true;
        let ctx = egui::Context::default();
        for size in [[780.0, 540.0], [1320.0, 880.0]] {
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(size[0], size[1]),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    app.sidebar(ctx);
                    app.tools(ctx);
                },
            );
            assert!(!output.shapes.is_empty());
            assert!(rx.try_recv().is_err());
            assert!(app.pending.is_empty());
        }
        if typed {
            app.refresh_git_changes();
            assert!(matches!(
                rx.try_recv().unwrap().op,
                Operation::GitChanges { .. }
            ));
        } else {
            app.git();
            let command = rx.try_recv().unwrap();
            assert!(matches!(command.op, Operation::GitStatus));
            reply(
                &mut app,
                command,
                Ok(Payload::GitStatus {
                    text: " M a.rs".into(),
                }),
            );
            assert_eq!(app.git_output, " M a.rs");
        }
    }
}

#[test]
fn clicking_git_tab_does_not_scan_and_only_refresh_button_issues_status() {
    let (mut app, rx) = connected();
    let ctx = egui::Context::default();
    // First layout pass settles the bottom-aligned sidebar controls.
    frame(&mut app, &ctx, vec![], true);
    let output = frame(&mut app, &ctx, vec![], true);
    let tab = text_rect(&output, "Git").unwrap().center();
    click(&mut app, &ctx, tab, true);
    assert!(app.tool == Tool::Git && app.tools_open);
    assert!(rx.try_recv().is_err());
    let output = frame(&mut app, &ctx, vec![], true);
    let refresh = text_rect(&output, "Refresh status").unwrap().center();
    click(&mut app, &ctx, refresh, true);
    assert!(matches!(
        rx.try_recv().unwrap().op,
        Operation::GitChanges { .. }
    ));
    assert!(rx.try_recv().is_err());
}

#[test]
fn untracked_open_file_button_only_reads_the_exact_path() {
    let (mut app, rx) = connected();
    let path = "new tab\tline\n你好.txt";
    let untracked = GitChange {
        path: path.into(),
        index: '?',
        worktree: '?',
        kind: GitChangeKind::Untracked,
        can_diff_staged: false,
        can_diff_unstaged: false,
    };
    status(&mut app, &rx, vec![untracked]);
    let before = format!("{:?}", app.documents[0]);
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, vec![], false);
    let output = frame(&mut app, &ctx, vec![], false);
    assert!(text_rect(&output, "Staged").is_none());
    assert!(text_rect(&output, "Unstaged").is_none());
    for write in ["Stage", "Commit", "Reset", "Apply patch"] {
        assert!(text_rect(&output, write).is_none());
    }
    let open = text_rect(&output, "Open file").unwrap().center();
    click(&mut app, &ctx, open, false);
    assert!(
        matches!(rx.try_recv().unwrap().op, Operation::Read { path: actual } if actual == path)
    );
    assert!(rx.try_recv().is_err());
    assert_eq!(format!("{:?}", app.documents[0]), before);
}

#[test]
fn diff_buttons_remain_visible_inside_the_resized_tools_panel() {
    for size in [[780.0, 540.0], [1320.0, 880.0]] {
        let (mut app, rx) = connected();
        status(&mut app, &rx, vec![file("a.rs")]);
        app.tool = Tool::Git;
        app.tools_open = true;
        let ctx = egui::Context::default();
        for _ in 0..2 {
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(size[0], size[1]),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    app.sidebar(ctx);
                    app.tools(ctx);
                },
            );
            for expected in ["Staged", "Unstaged"] {
                let rect = text_rect(&output, expected).expect("diff button is painted");
                assert!(
                    output.shapes.iter().any(|shape| match &shape.shape {
                        egui::epaint::Shape::Text(text) if text.galley.job.text == expected =>
                            shape.clip_rect.contains_rect(rect),
                        _ => false,
                    }),
                    "{expected} must be visible at {size:?}"
                );
            }
        }
        assert!(rx.try_recv().is_err());
    }
}

#[test]
fn populated_git_panel_keeps_patch_read_only_across_layouts() {
    let (mut app, rx) = connected();
    status(&mut app, &rx, vec![file("a.rs")]);
    app.select_git_diff("a.rs".into(), GitDiffKind::Staged);
    let command = rx.try_recv().unwrap();
    let result = diff_reply(&command, "diff --git a/a.rs b/a.rs\n-saved\n+on disk\n");
    reply(&mut app, command, Ok(result));
    let before = format!("{:?}", app.documents[0]);
    let ctx = egui::Context::default();
    for size in [[780.0, 540.0], [1320.0, 880.0]] {
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(size[0], size[1]),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| app.git_panel(ui));
            },
        );
        assert!(!output.shapes.is_empty());
    }
    assert_eq!(format!("{:?}", app.documents[0]), before);
    assert!(rx.try_recv().is_err());
}

#[test]
fn explicit_disconnect_refuses_pending_git_read_without_dispatch() {
    let (mut app, commands) = connected();
    app.pending.insert(
        91,
        Job::GitRead(Action {
            generation: app.generation,
            epoch: 1,
            program: "/synthetic/git".into(),
            kind: ReadKind::Changes,
        }),
    );
    app.disconnect_idle();
    assert!(app.state == ConnectionState::Ready);
    assert!(app.worker.is_some());
    assert!(app.pending.contains_key(&91));
    assert!(commands.try_recv().is_err());
}
