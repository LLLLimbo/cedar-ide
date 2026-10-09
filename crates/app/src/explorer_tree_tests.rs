//! These tests dispatch through the production browser methods, feed real
//! worker event shapes, and render the production sidebar and raw-input hook.
use super::*;
use explorer_tree::{row_id, Mode, MAX_DEPTH, MAX_ROWS, MAX_SNAPSHOTS, MAX_TEXT_BYTES};
use sha2::{Digest, Sha256};

fn fixture() -> (CedarApp, Receiver<Command>) {
    let mut app = CedarApp::empty();
    app.state = ConnectionState::Ready;
    app.open_form = false;
    let (worker, commands) = Worker::recording();
    app.worker = Some(worker);
    (app, commands)
}
fn entry(path: &str, is_dir: bool) -> Entry {
    Entry {
        name: path.rsplit('/').next().unwrap().into(),
        path: path.into(),
        is_dir,
    }
}
fn reply(app: &mut CedarApp, command: Command, entries: Vec<Entry>) {
    app.apply_worker_event(WorkerEvent::Response(Event {
        generation: app.generation,
        id: command.id,
        connected: true,
        result: Ok(Payload::Entries { entries }),
    }));
}
fn error(app: &mut CedarApp, command: Command, message: &str, connected: bool) {
    app.apply_event(Event {
        generation: app.generation,
        id: command.id,
        connected,
        result: Err(message.into()),
    });
}
fn root(app: &mut CedarApp, commands: &Receiver<Command>, entries: Vec<Entry>) {
    app.explorer_set_mode(Mode::Tree);
    app.explorer_expand("");
    reply(app, commands.try_recv().unwrap(), entries);
}
fn row(app: &CedarApp, path: &str) -> explorer_tree::Row {
    app.explorer_visible_rows()
        .into_iter()
        .find(|row| row.entry.path == path)
        .unwrap()
}
fn key_event(key: egui::Key, repeat: bool) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: Some(key),
        pressed: true,
        repeat,
        modifiers: egui::Modifiers::NONE,
    }
}
fn frame(app: &mut CedarApp, events: Vec<egui::Event>) -> egui::FullOutput {
    let output = held_frame(app, events.clone());
    let releases: Vec<_> = events
        .into_iter()
        .filter_map(|event| {
            if let egui::Event::Key {
                key,
                physical_key,
                modifiers,
                pressed: true,
                ..
            } = event
            {
                Some(egui::Event::Key {
                    key,
                    physical_key,
                    modifiers,
                    pressed: false,
                    repeat: false,
                })
            } else {
                None
            }
        })
        .collect();
    if !releases.is_empty() {
        held_frame(app, releases);
    }
    output
}
fn held_frame(app: &mut CedarApp, events: Vec<egui::Event>) -> egui::FullOutput {
    let ctx = app.editor_ctx.clone();
    let mut input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(780.0, 540.0),
        )),
        events,
        time: Some(ctx.cumulative_pass_nr() as f64 * 0.25),
        ..Default::default()
    };
    eframe::App::raw_input_hook(app, &ctx, &mut input);
    ctx.run(input, |ctx| {
        app.begin_navigation_frame(ctx);
        if let Some(id) = ctx.data_mut(|data| {
            let id = data.get_temp::<egui::Id>(egui::Id::new("tree_test_focus"));
            data.remove::<egui::Id>(egui::Id::new("tree_test_focus"));
            id
        }) {
            ctx.memory_mut(|memory| memory.request_focus(id));
        }
        app.shortcuts(ctx);
        app.navigation_window(ctx);
        app.header(ctx);
        app.footer(ctx);
        app.notifications(ctx);
        app.tools(ctx);
        app.sidebar(ctx);
        egui::CentralPanel::default().show(ctx, |ui| {
            if !app.documents.is_empty() {
                app.editor(ui);
            }
        });
        app.dialogs(ctx);
        app.finish_workspace_access_frame(ctx);
    })
}
fn settle(app: &mut CedarApp) {
    for _ in 0..4 {
        frame(app, vec![]);
    }
}
fn focus(app: &mut CedarApp, path: &str) {
    app.editor_ctx
        .memory_mut(|memory| memory.request_focus(row_id(path)));
    app.explorer_select(path);
}

#[test]
fn flat_is_default_and_all_modes_share_one_nonqueued_request_slot() {
    let (mut app, commands) = fixture();
    assert_eq!(app.explorer.mode, Mode::Flat);
    app.list(String::new());
    let flat = commands.try_recv().unwrap();
    app.list("next".into());
    app.explorer_set_mode(Mode::Tree);
    app.explorer_expand("");
    assert!(commands.try_recv().is_err());
    assert!(app.explorer_busy());
    reply(&mut app, flat, vec![entry("obsolete", true)]);
    assert!(!app.explorer_busy());
    assert_eq!(app.explorer_visible_rows().len(), 1);
    assert!(app.entries.is_empty());
    app.explorer_expand("");
    let tree = commands.try_recv().unwrap();
    assert!(matches!(tree.op, Operation::List { ref path } if path.is_empty()));
    app.explorer_set_mode(Mode::Flat);
    app.list("next".into());
    assert!(commands.try_recv().is_err());
    reply(&mut app, tree, vec![entry("ignored", true)]);
    assert!(!app.explorer_busy());
    assert!(app.entries.is_empty());
}

#[test]
fn explicit_expand_is_lazy_collapse_releases_cache_and_keeps_drafts() {
    let (mut app, commands) = fixture();
    let mut doc = Document::new(1, "src/雪.rs".into(), "saved".into(), "r0".into());
    doc.text = "precious draft".into();
    app.documents.push(doc);
    root(
        &mut app,
        &commands,
        vec![entry("src", true), entry("test", true)],
    );
    assert!(commands.try_recv().is_err());
    assert!(!row(&app, "src").loaded);
    app.explorer_expand("src");
    reply(
        &mut app,
        commands.try_recv().unwrap(),
        vec![entry("src/雪.rs", false), entry("src/nested", true)],
    );
    app.explorer_select("src/雪.rs");
    assert_eq!(app.explorer_scope(), "src");
    app.explorer_collapse("src");
    assert_eq!(app.explorer.selected, "src");
    assert!(!row(&app, "src").loaded);
    assert_eq!(app.explorer.usage(), Some((1, 2, 14)));
    assert_eq!(app.documents[0].text, "precious draft");
    assert!(app.documents[0].dirty());
    assert!(commands.try_recv().is_err());
    app.explorer_expand("src");
    assert!(matches!(commands.try_recv().unwrap().op, Operation::List { path } if path == "src"));
}

#[test]
fn collapsed_success_and_errors_are_obsolete_but_current_transport_loss_wins() {
    for failed in [false, true] {
        let (mut app, commands) = fixture();
        root(&mut app, &commands, vec![entry("src", true)]);
        app.explorer_expand("src");
        let request = commands.try_recv().unwrap();
        app.explorer_collapse("src");
        app.explorer_expand("src");
        assert!(commands.try_recv().is_err());
        if failed {
            error(&mut app, request, "old failure", true);
        } else {
            reply(&mut app, request, vec![entry("src/old", false)]);
        }
        assert!(!app.explorer_busy());
        assert!(!row(&app, "src").expanded);
        assert!(app.error.is_none());
        app.explorer_expand("src");
        let request = commands.try_recv().unwrap();
        app.explorer_collapse("src");
        error(&mut app, request, "actual transport loss", false);
        assert!(app.state == ConnectionState::Disconnected);
        assert!(app
            .error
            .as_deref()
            .unwrap()
            .contains("actual transport loss"));
    }
}

#[test]
fn obsolete_generation_or_unknown_id_cannot_clear_new_busy_slot() {
    let (mut app, commands) = fixture();
    root(&mut app, &commands, vec![entry("src", true)]);
    app.explorer_expand("src");
    let old = commands.try_recv().unwrap();
    app.generation += 1;
    app.pending.clear();
    app.explorer.reset_connection();
    app.explorer_expand("");
    let new = commands.try_recv().unwrap();
    app.apply_event(Event {
        generation: app.generation - 1,
        id: old.id,
        connected: false,
        result: Err("old transport".into()),
    });
    app.apply_event(Event {
        generation: app.generation,
        id: new.id + 1,
        connected: true,
        result: Ok(Payload::Entries { entries: vec![] }),
    });
    assert!(app.ready());
    assert_eq!(app.explorer.outstanding.as_ref().unwrap().request, new.id);
    reply(&mut app, new, vec![entry("new", true)]);
    assert!(!app.explorer_busy());
    assert_eq!(app.explorer_visible_rows().len(), 2);
}

#[test]
fn malformed_branch_rejects_whole_snapshot_and_retry_is_explicit() {
    let malformed = vec![
        vec![Entry {
            name: "different".into(),
            path: "src/file".into(),
            is_dir: false,
        }],
        vec![entry("outside", false)],
        vec![entry("src/nested/file", false)],
        vec![entry("src/../file", false)],
        vec![entry("src/./file", false)],
        vec![entry("src//file", false)],
        vec![entry("src/file", false), entry("src/file", true)],
        vec![Entry {
            name: String::new(),
            path: "src/".into(),
            is_dir: false,
        }],
        vec![entry("src/C:drive", false)],
        vec![entry("src/back\\slash", false)],
        vec![entry("src/nul\0", false)],
    ];
    for entries in malformed {
        let (mut app, commands) = fixture();
        root(&mut app, &commands, vec![entry("src", true)]);
        app.explorer_expand("src");
        reply(&mut app, commands.try_recv().unwrap(), entries);
        assert!(row(&app, "src").error.is_some());
        assert!(!row(&app, "src").loaded);
        assert_eq!(app.explorer_visible_rows().len(), 2);
        app.explorer_expand("src");
        assert!(
            commands.try_recv().is_err(),
            "error requires explicit Refresh/Retry"
        );
        app.explorer_refresh("src");
        reply(
            &mut app,
            commands.try_recv().unwrap(),
            vec![entry("src/ok", false)],
        );
        assert!(row(&app, "src").error.is_none());
        assert_eq!(app.explorer_visible_rows().len(), 3);
    }
}

#[test]
fn refresh_is_transactional_and_failure_keeps_old_children_labeled_stale() {
    let (mut app, commands) = fixture();
    root(
        &mut app,
        &commands,
        vec![entry("src", true), entry("other", true)],
    );
    app.explorer_expand("src");
    reply(
        &mut app,
        commands.try_recv().unwrap(),
        vec![entry("src/old", false)],
    );
    app.explorer_refresh("src");
    let refresh = commands.try_recv().unwrap();
    assert!(row(&app, "src/old").entry.path.ends_with("old"));
    reply(&mut app, refresh, vec![entry("src/invalid/nested", false)]);
    assert!(row(&app, "src").stale);
    assert!(row(&app, "src").error.is_some());
    assert!(row(&app, "src/old").entry.path.ends_with("old"));
    app.explorer_refresh("src");
    reply(
        &mut app,
        commands.try_recv().unwrap(),
        vec![entry("src/new", false)],
    );
    assert!(!row(&app, "src").stale);
    assert!(row(&app, "src").error.is_none());
    assert!(!app
        .explorer_visible_rows()
        .iter()
        .any(|row| row.entry.path == "src/old"));
    assert!(row(&app, "other").entry.is_dir);
}

#[test]
fn snapshot_and_row_caps_reject_without_eviction_or_partial_success() {
    let (mut app, commands) = fixture();
    root(
        &mut app,
        &commands,
        (0..MAX_SNAPSHOTS)
            .map(|n| entry(&format!("d{n:02}"), true))
            .collect(),
    );
    for n in 0..MAX_SNAPSHOTS - 1 {
        app.explorer_expand(&format!("d{n:02}"));
        reply(&mut app, commands.try_recv().unwrap(), vec![]);
    }
    assert_eq!(app.explorer.usage().unwrap().0, MAX_SNAPSHOTS);
    app.explorer_expand("d63");
    assert!(commands.try_recv().is_err());
    assert!(app.explorer.message.as_deref().unwrap().contains("limit"));
    assert!(row(&app, "d00").loaded);
    app.explorer_collapse("d00");
    app.explorer_expand("d63");
    reply(&mut app, commands.try_recv().unwrap(), vec![]);
    assert!(row(&app, "d63").loaded);

    let (mut app, commands) = fixture();
    let mut entries = vec![entry("src", true)];
    entries.extend((1..MAX_ROWS).map(|n| entry(&format!("f{n}"), false)));
    root(&mut app, &commands, entries);
    app.explorer_expand("src");
    reply(
        &mut app,
        commands.try_recv().unwrap(),
        vec![entry("src/extra", false)],
    );
    assert!(row(&app, "src")
        .error
        .as_deref()
        .unwrap()
        .contains("cache limit"));
    assert_eq!(app.explorer.usage().unwrap().1, MAX_ROWS);
    assert_eq!(app.explorer_visible_rows().len(), MAX_ROWS + 1);
}

#[test]
fn utf8_names_paths_and_snapshot_keys_share_exact_byte_budget() {
    let (mut app, commands) = fixture();
    let mut entries = vec![entry("雪", true)]; // Six UTF-8 bytes in the root snapshot.
    let mut remaining = MAX_TEXT_BYTES - 6;
    let mut n = 0;
    while remaining > 0 {
        let length = (remaining / 2).min(4096);
        let name = format!("{n:03}{}", "x".repeat(length - 3));
        entries.push(entry(&name, false));
        remaining -= length * 2;
        n += 1;
    }
    root(&mut app, &commands, entries);
    assert_eq!(app.explorer.usage(), Some((1, n + 1, MAX_TEXT_BYTES)));
    app.explorer_expand("雪");
    assert!(
        commands.try_recv().is_err(),
        "the three-byte snapshot key itself must fit"
    );
    assert_eq!(app.explorer.usage().unwrap().2, MAX_TEXT_BYTES);
    app.explorer_refresh("");
    reply(
        &mut app,
        commands.try_recv().unwrap(),
        vec![entry("雪", true)],
    );
    app.explorer_expand("雪");
    reply(
        &mut app,
        commands.try_recv().unwrap(),
        vec![entry("雪/猫", false)],
    );
    assert_eq!(app.explorer.usage(), Some((2, 2, 19))); // 6 + key 3 + name 3 + path 7.
}

#[test]
fn replacement_cannot_overflow_aggregate_budget_and_does_not_evict_siblings() {
    let (mut app, commands) = fixture();
    root(
        &mut app,
        &commands,
        vec![entry("a", true), entry("b", true)],
    );
    app.explorer_expand("a");
    reply(
        &mut app,
        commands.try_recv().unwrap(),
        (0..2000)
            .map(|n| entry(&format!("a/f{n}"), false))
            .collect(),
    );
    app.explorer_expand("b");
    reply(
        &mut app,
        commands.try_recv().unwrap(),
        vec![entry("b/old", false)],
    );
    let usage = app.explorer.usage();
    app.explorer_refresh("b");
    reply(
        &mut app,
        commands.try_recv().unwrap(),
        (0..3000)
            .map(|n| entry(&format!("b/f{n}"), false))
            .collect(),
    );
    assert_eq!(app.explorer.usage(), usage);
    assert!(row(&app, "b").stale);
    assert!(row(&app, "b/old").entry.path.ends_with("old"));
    assert!(row(&app, "a/f1999").entry.path.ends_with("1999"));
}

#[test]
fn depth_and_path_limits_reject_complete_overdepth_branch() {
    let (mut app, commands) = fixture();
    root(&mut app, &commands, vec![entry("d", true)]);
    let mut path = "d".to_string();
    for _ in 1..MAX_DEPTH {
        app.explorer_expand(&path);
        let child = format!("{path}/d");
        reply(
            &mut app,
            commands.try_recv().unwrap(),
            vec![entry(&child, true)],
        );
        path = child;
    }
    assert_eq!(row(&app, &path).depth, MAX_DEPTH);
    app.explorer_expand(&path);
    reply(
        &mut app,
        commands.try_recv().unwrap(),
        vec![entry(&format!("{path}/beyond"), false)],
    );
    assert!(row(&app, &path).error.is_some());
    assert_eq!(app.explorer_visible_rows().len(), MAX_DEPTH + 1);
    app.explorer_refresh("");
    reply(
        &mut app,
        commands.try_recv().unwrap(),
        vec![entry(&"z".repeat(4097), false)],
    );
    assert!(row(&app, "").stale);
    assert_eq!(app.explorer_visible_rows().len(), MAX_DEPTH + 1);
}

#[test]
fn save_marks_actual_parent_stale_without_list_and_new_file_uses_selected_scope() {
    let (mut app, commands) = fixture();
    app.root = "/project".into();
    let form = ConnectForm {
        local_root: app.root.clone(),
        ..Default::default()
    };
    app.workspace_key = Some(form.key());
    app.active_form = Some(form);
    root(
        &mut app,
        &commands,
        vec![entry("a", true), entry("b", true)],
    );
    for directory in ["a", "b"] {
        app.explorer_expand(directory);
        reply(
            &mut app,
            commands.try_recv().unwrap(),
            vec![entry(&format!("{directory}/file"), false)],
        );
    }
    app.explorer_select("b/file");
    app.explorer_new_file();
    assert_eq!(app.new_path, "b/");
    let mut document = Document::new(7, "a/file".into(), "old".into(), "r0".into());
    document.text = "acknowledged save".into();
    document.saving = true;
    app.documents.push(document);
    app.next_request = 99;
    let submission = interrupted_save::InterruptedSave::capture(&app, &app.documents[0]).unwrap();
    app.next_request += 1;
    app.pending.insert(
        99,
        Job::Save {
            document: 7,
            snapshot: "acknowledged save".into(),
            submission: Some(submission),
        },
    );
    app.apply_worker_event(WorkerEvent::Response(Event {
        generation: app.generation,
        id: 99,
        connected: true,
        result: Ok(Payload::Written {
            revision: format!("{:x}", Sha256::digest(b"acknowledged save")),
        }),
    }));
    assert!(!app.documents[0].dirty());
    assert!(!app.documents[0].saving);
    assert!(row(&app, "a").stale);
    assert!(!row(&app, "b").stale);
    assert_eq!(app.explorer_scope(), "b");
    assert_eq!(app.explorer_scope_entries()[0].path, "b/file");
    assert!(commands.try_recv().is_err());
    app.explorer_collapse("b");
    assert!(!app.explorer_scope_loaded());
    assert!(app.explorer_scope_entries().is_empty());
}

#[test]
fn opening_existing_tree_file_retains_document_identity_and_undo_history() {
    let (mut app, commands) = fixture();
    root(&mut app, &commands, vec![entry("draft.rs", false)]);
    let mut doc = Document::new(5, "draft.rs".into(), "saved".into(), "r0".into());
    doc.text = "dirty".into();
    app.documents.push(doc);
    let history = editor_state::load(&app.editor_ctx, &mut app.documents[0]);
    history
        .clone()
        .store(&app.editor_ctx, egui::Id::new(("editor", 5u64)));
    let before = (
        app.documents[0].id,
        app.documents[0].text.clone(),
        app.documents[0].saved_text.clone(),
        app.documents[0].edit_version,
    );
    app.explorer_activate("draft.rs");
    assert!(commands.try_recv().is_err());
    assert_eq!(app.active_document, Some(5));
    assert_eq!(
        (
            app.documents[0].id,
            app.documents[0].text.clone(),
            app.documents[0].saved_text.clone(),
            app.documents[0].edit_version
        ),
        before
    );
}

#[test]
fn flat_failed_navigation_retry_keeps_the_actual_target() {
    let (mut app, commands) = fixture();
    app.list("sub".into());
    error(&mut app, commands.try_recv().unwrap(), "denied", true);
    assert_eq!(app.directory, "");
    let ctx = app.editor_ctx.clone();
    let mut retry = None;
    for _ in 0..3 {
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                app.explorer_status(ui, false);
            });
        });
    }
    // The stored failed target is exercised by the actual Retry button, located
    // from the render rather than by calling List with the expected string.
    let output = ctx.run(Default::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            app.explorer_status(ui, false);
        });
    });
    for shape in output.shapes {
        if let egui::epaint::Shape::Text(text) = shape.shape {
            if text.galley.text() == "Retry" {
                retry = Some(text.visual_bounding_rect().center());
            }
        }
    }
    let at = retry.unwrap();
    for pressed in [true, false] {
        let _ = ctx.run(
            egui::RawInput {
                events: vec![
                    egui::Event::PointerMoved(at),
                    egui::Event::PointerButton {
                        pos: at,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| app.explorer_status(ui, false));
            },
        );
    }
    assert!(matches!(commands.try_recv().unwrap().op, Operation::List { path } if path == "sub"));
}

#[test]
fn keyboard_uses_production_raw_hook_and_first_consecutive_keys_keep_row_focus() {
    let (mut app, commands) = fixture();
    root(
        &mut app,
        &commands,
        vec![entry("dir", true), entry("a", false), entry("z", false)],
    );
    settle(&mut app);
    focus(&mut app, ""); // No settling frame: exercise the first key after focus.
    for (key, expected) in [
        (egui::Key::ArrowDown, "dir"),
        (egui::Key::ArrowDown, "a"),
        (egui::Key::End, "z"),
        (egui::Key::ArrowUp, "a"),
        (egui::Key::Home, ""),
    ] {
        frame(&mut app, vec![key_event(key, false)]);
        assert_eq!(app.explorer.selected, expected);
        assert_eq!(
            app.editor_ctx.memory(|memory| memory.focused()),
            Some(row_id(expected))
        );
    }
    assert!(commands.try_recv().is_err());
    focus(&mut app, "dir");
    frame(&mut app, vec![key_event(egui::Key::ArrowRight, false)]);
    reply(
        &mut app,
        commands.try_recv().unwrap(),
        vec![entry("dir/file", false)],
    );
    frame(&mut app, vec![key_event(egui::Key::ArrowRight, false)]);
    assert_eq!(app.explorer.selected, "dir/file");
    frame(&mut app, vec![key_event(egui::Key::ArrowLeft, false)]);
    assert_eq!(app.explorer.selected, "dir");
    frame(&mut app, vec![key_event(egui::Key::ArrowLeft, false)]);
    assert!(!row(&app, "dir").expanded);
    assert!(commands.try_recv().is_err());
}

#[test]
fn repeated_enter_and_mixed_pointer_enter_do_not_open_or_toggle_rows() {
    let (mut app, commands) = fixture();
    root(&mut app, &commands, vec![entry("dir", true)]);
    settle(&mut app);
    focus(&mut app, "dir");
    frame(&mut app, vec![key_event(egui::Key::Enter, true)]);
    assert!(!row(&app, "dir").expanded);
    let at = app
        .editor_ctx
        .read_response(row_id("dir"))
        .unwrap()
        .rect
        .center();
    frame(
        &mut app,
        vec![
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    frame(
        &mut app,
        vec![
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
            key_event(egui::Key::Enter, false),
        ],
    );
    assert!(!row(&app, "dir").expanded);
    assert!(commands.try_recv().is_err());
    frame(
        &mut app,
        vec![
            key_event(egui::Key::Enter, false),
            egui::Event::Text("x".into()),
        ],
    );
    assert!(!row(&app, "dir").expanded);
    assert!(commands.try_recv().is_err());
}

#[test]
fn tab_modals_editor_and_newer_focus_take_precedence() {
    let (mut app, commands) = fixture();
    root(&mut app, &commands, vec![entry("dir", true)]);
    settle(&mut app);
    focus(&mut app, "dir");
    frame(&mut app, vec![key_event(egui::Key::Tab, false)]);
    assert_ne!(
        app.editor_ctx.memory(|memory| memory.focused()),
        Some(row_id("dir"))
    );
    assert!(commands.try_recv().is_err());
    focus(&mut app, "dir");
    app.new_file = true;
    frame(&mut app, vec![key_event(egui::Key::Enter, false)]);
    assert!(commands.try_recv().is_err());
    app.new_file = false;
    focus(&mut app, "dir");
    let ctx = app.editor_ctx.clone();
    let mut input = egui::RawInput {
        events: vec![key_event(egui::Key::Enter, false)],
        ..Default::default()
    };
    app.explorer_tree_input(&ctx, &mut input);
    ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("newer_focus")));
    let _ = ctx.run(input, |ctx| app.explorer_tree_shortcuts(ctx));
    assert!(app.explorer.key_reveal.is_none());
    assert!(!row(&app, "dir").expanded);
    assert!(commands.try_recv().is_err());
}

#[test]
fn poll_replacing_a_focused_directory_with_a_file_cancels_captured_enter() {
    let (mut app, commands) = fixture();
    root(&mut app, &commands, vec![entry("target", true)]);
    settle(&mut app);
    app.explorer_refresh("");
    let refresh = commands.try_recv().unwrap();
    focus(&mut app, "target");
    let ctx = app.editor_ctx.clone();
    let mut input = egui::RawInput {
        events: vec![key_event(egui::Key::Enter, false)],
        ..Default::default()
    };
    app.explorer_tree_input(&ctx, &mut input);
    reply(&mut app, refresh, vec![entry("target", false)]);
    let _ = ctx.run(input, |ctx| app.explorer_tree_shortcuts(ctx));
    assert!(commands.try_recv().is_err());
    assert!(app.documents.is_empty());
}

#[test]
fn deep_rows_have_finite_visible_width_at_minimum_sidebar_size() {
    let (mut app, commands) = fixture();
    root(&mut app, &commands, vec![entry("d", true)]);
    let mut path = "d".to_string();
    for _ in 1..MAX_DEPTH {
        app.explorer_expand(&path);
        let child = format!("{path}/d");
        reply(
            &mut app,
            commands.try_recv().unwrap(),
            vec![entry(&child, true)],
        );
        path = child;
    }
    app.editor_ctx.data_mut(|data| {
        data.insert_persisted(
            egui::Id::new("explorer"),
            egui::containers::panel::PanelState {
                rect: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(180.0, 540.0)),
            },
        )
    });
    settle(&mut app);
    app.editor_ctx
        .data_mut(|data| data.insert_temp(egui::Id::new("tree_test_focus"), row_id(&path)));
    settle(&mut app);
    let output = frame(&mut app, vec![]);
    let response = app.editor_ctx.read_response(row_id(&path)).unwrap();
    assert!(response.rect.is_finite());
    assert!(response.rect.width() > 50.0 && response.rect.width() < 180.0);
    let (clip, painted) = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::epaint::Shape::Text(text)
                if text.galley.text().contains("not loaded")
                    && response.rect.contains(text.visual_bounding_rect().center()) =>
            {
                Some((shape.clip_rect, text.visual_bounding_rect()))
            }
            _ => None,
        })
        .expect("the final depth-32 row must be painted after focus reveal");
    assert!(
        clip.contains_rect(response.rect),
        "deep row hit target is clipped"
    );
    assert!(clip.contains_rect(painted), "deep row text is clipped");
    frame(&mut app, vec![key_event(egui::Key::Enter, false)]);
    assert!(
        matches!(commands.try_recv().unwrap().op, Operation::List { path: requested } if requested == path)
    );
}

#[test]
fn terminal_wire_branch_and_mode_counters_fail_closed_without_dispatch() {
    for mode in [Mode::Flat, Mode::Tree] {
        for terminal in [0, u64::MAX] {
            let (mut app, commands) = fixture();
            app.explorer_set_mode(mode);
            app.next_request = terminal;
            app.explorer_refresh("");
            assert!(commands.try_recv().is_err());
            assert!(!app.explorer_busy());
            assert!(app
                .explorer
                .message
                .as_deref()
                .unwrap()
                .contains("exhausted"));
        }
    }
    for epoch in [u64::MAX - 1, u64::MAX] {
        let (mut app, commands) = fixture();
        app.explorer_set_mode(Mode::Tree);
        app.explorer.epoch = epoch;
        app.explorer_expand("");
        assert!(commands.try_recv().is_err());
        assert!(!row(&app, "").expanded);
        assert!(app
            .explorer
            .message
            .as_deref()
            .unwrap()
            .contains("exhausted"));
    }
    let (mut app, commands) = fixture();
    app.explorer.mode_epoch = u64::MAX;
    app.explorer_set_mode(Mode::Tree);
    app.list(String::new());
    assert_eq!(app.explorer.mode, Mode::Flat);
    assert!(commands.try_recv().is_err());
}

#[test]
fn native_shaped_repeats_stay_held_across_focus_mode_and_connection_changes() {
    let (mut app, commands) = fixture();
    root(&mut app, &commands, vec![entry("dir", true)]);
    settle(&mut app);
    focus(&mut app, "dir");
    held_frame(&mut app, vec![key_event(egui::Key::Enter, false)]);
    reply(&mut app, commands.try_recv().unwrap(), vec![]);
    for _ in 0..3 {
        held_frame(&mut app, vec![key_event(egui::Key::Enter, false)]);
    }
    assert!(row(&app, "dir").expanded);
    assert!(commands.try_recv().is_err());
    app.explorer_set_mode(Mode::Flat);
    held_frame(&mut app, vec![key_event(egui::Key::Enter, false)]);
    app.explorer_set_mode(Mode::Tree);
    app.explorer.reset_connection();
    focus(&mut app, "");
    held_frame(&mut app, vec![key_event(egui::Key::Enter, false)]);
    assert!(!row(&app, "").expanded);
    assert!(commands.try_recv().is_err());
    held_frame(
        &mut app,
        vec![egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: Some(egui::Key::Enter),
            pressed: false,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    held_frame(&mut app, vec![key_event(egui::Key::Enter, false)]);
    assert!(matches!(commands.try_recv().unwrap().op, Operation::List { path } if path.is_empty()));
}

#[test]
fn warmed_dirty_editor_ignores_tree_mixed_text_paste_enter_and_preserves_undo() {
    let (mut app, commands) = fixture();
    root(
        &mut app,
        &commands,
        vec![entry("dir", true), entry("draft.rs", false)],
    );
    app.documents.push(Document::new(
        1,
        "draft.rs".into(),
        "saved".into(),
        "r0".into(),
    ));
    app.active_document = Some(1);
    settle(&mut app);
    app.editor_ctx
        .memory_mut(|memory| memory.request_focus(egui::Id::new(("editor", 1u64))));
    frame(&mut app, vec![egui::Event::Text("dirty".into())]);
    settle(&mut app);
    let before = (
        app.documents[0].text.clone(),
        app.documents[0].saved_text.clone(),
        app.documents[0].edit_version,
    );
    assert!(app.documents[0].dirty());
    for edit in [
        egui::Event::Text("BAD".into()),
        egui::Event::Paste("BAD".into()),
    ] {
        focus(&mut app, "dir");
        frame(&mut app, vec![key_event(egui::Key::Enter, false), edit]);
        assert!(!row(&app, "dir").expanded);
        assert_eq!(
            (
                app.documents[0].text.clone(),
                app.documents[0].saved_text.clone(),
                app.documents[0].edit_version
            ),
            before
        );
        assert!(commands.try_recv().is_err());
    }
    app.explorer_activate("draft.rs");
    settle(&mut app);
    let mut undo = key_event(egui::Key::Z, false);
    if let egui::Event::Key { modifiers, .. } = &mut undo {
        *modifiers = egui::Modifiers::COMMAND;
    }
    frame(&mut app, vec![undo]);
    assert_eq!(app.documents[0].text, "saved");
    assert!(commands.try_recv().is_err());
}

#[test]
fn native_held_key_is_released_by_either_genuine_focus_loss_signal() {
    for window_event in [false, true] {
        let (mut app, commands) = fixture();
        root(&mut app, &commands, vec![entry("dir", true)]);
        settle(&mut app);
        focus(&mut app, "dir");
        held_frame(&mut app, vec![key_event(egui::Key::Enter, false)]);
        reply(&mut app, commands.try_recv().unwrap(), vec![]);
        assert!(row(&app, "dir").expanded);
        let ctx = app.editor_ctx.clone();
        let mut lost = egui::RawInput {
            focused: window_event,
            events: if window_event {
                vec![egui::Event::WindowFocused(false)]
            } else {
                vec![]
            },
            ..Default::default()
        };
        eframe::App::raw_input_hook(&mut app, &ctx, &mut lost);
        let _ = ctx.run(lost, |ctx| app.sidebar(ctx));
        focus(&mut app, "dir");
        held_frame(&mut app, vec![key_event(egui::Key::Enter, false)]);
        assert!(
            !row(&app, "dir").expanded,
            "focus loss must clear the native held-key latch"
        );
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn held_enter_opening_a_dirty_file_cannot_repeat_into_the_editor() {
    let (mut app, commands) = fixture();
    root(&mut app, &commands, vec![entry("draft.rs", false)]);
    app.documents.push(Document::new(
        1,
        "draft.rs".into(),
        "saved".into(),
        "r0".into(),
    ));
    app.active_document = Some(1);
    settle(&mut app);
    let editor = egui::Id::new(("editor", 1u64));
    app.editor_ctx
        .memory_mut(|memory| memory.request_focus(editor));
    frame(&mut app, vec![egui::Event::Text("dirty".into())]);
    settle(&mut app);
    let before = (app.documents[0].text.clone(), app.documents[0].edit_version);
    assert!(app.documents[0].dirty());
    focus(&mut app, "draft.rs");
    held_frame(&mut app, vec![key_event(egui::Key::Enter, false)]);
    for _ in 0..4 {
        held_frame(&mut app, vec![key_event(egui::Key::Enter, false)]);
    }
    assert_eq!(
        app.editor_ctx.memory(|memory| memory.focused()),
        Some(editor)
    );
    assert_eq!(
        (app.documents[0].text.clone(), app.documents[0].edit_version),
        before
    );
    assert!(commands.try_recv().is_err());
    let mut undo = key_event(egui::Key::Z, false);
    if let egui::Event::Key { modifiers, .. } = &mut undo {
        *modifiers = egui::Modifiers::COMMAND;
    }
    frame(&mut app, vec![undo]);
    assert_eq!(app.documents[0].text, "saved");
    held_frame(
        &mut app,
        vec![egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: Some(egui::Key::Enter),
            pressed: false,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    frame(&mut app, vec![key_event(egui::Key::Enter, false)]);
    assert!(
        app.documents[0].text.contains('\n'),
        "fresh Enter belongs to the editor after release"
    );
}

#[test]
fn compact_retry_and_mode_focus_reveal_through_both_scroll_areas() {
    let (mut app, commands) = fixture();
    root(&mut app, &commands, vec![entry("dir", true)]);
    app.explorer_expand("dir");
    error(
        &mut app,
        commands.try_recv().unwrap(),
        &"long rejected directory response ".repeat(30),
        true,
    );
    app.tools_open = true;
    app.tool = Tool::Search;
    app.editor_ctx.data_mut(|data| {
        data.insert_persisted(
            egui::Id::new("explorer"),
            egui::containers::panel::PanelState {
                rect: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(180.0, 540.0)),
            },
        )
    });
    settle(&mut app);
    for name in ["tree_retry:dir", "explorer_flat_mode", "explorer_tree_mode"] {
        let sidebar =
            egui::containers::panel::PanelState::load(&app.editor_ctx, egui::Id::new("explorer"))
                .unwrap()
                .rect;
        let gutter = egui::pos2(sidebar.right() - 17.0, sidebar.bottom() - 20.0);
        frame(
            &mut app,
            vec![
                egui::Event::PointerMoved(gutter),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -2000.0),
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        for _ in 0..20 {
            frame(&mut app, vec![]);
        }
        let id = workspace_access_tests::recorded_response(&app, name).id;
        app.editor_ctx
            .data_mut(|data| data.insert_temp(egui::Id::new("tree_test_focus"), id));
        settle(&mut app);
        let output = frame(&mut app, vec![]);
        let response = workspace_access_tests::recorded_response(&app, name);
        assert_eq!(app.editor_ctx.memory(|memory| memory.focused()), Some(id));
        let label = match name {
            "tree_retry:dir" => "Retry",
            "explorer_flat_mode" => "Flat",
            _ => "Tree",
        };
        let text = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                    Some((shape.clip_rect, text.visual_bounding_rect()))
                }
                _ => None,
            })
            .unwrap();
        assert!(
            text.0.contains_rect(response.rect),
            "{name} button remains clipped: {:?} / {:?}",
            response.rect,
            text.0
        );
        assert!(text.0.contains_rect(text.1), "{name} label remains clipped");
    }
    assert!(commands.try_recv().is_err());
}

#[test]
fn same_row_end_reveals_after_wheel_without_snapping_back_during_idle() {
    let (mut app, commands) = fixture();
    root(
        &mut app,
        &commands,
        (0..100)
            .map(|n| entry(&format!("file{n:03}.txt"), false))
            .collect(),
    );
    settle(&mut app);
    focus(&mut app, "");
    frame(&mut app, vec![key_event(egui::Key::End, false)]);
    settle(&mut app);
    let last = "file099.txt";
    let id = row_id(last);
    let response = app.editor_ctx.read_response(id).unwrap();
    let at = response.rect.center();
    assert_eq!(app.editor_ctx.memory(|memory| memory.focused()), Some(id));
    frame(
        &mut app,
        vec![
            egui::Event::PointerMoved(at),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, 5000.0),
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    for _ in 0..25 {
        frame(&mut app, vec![]);
    }
    assert!(
        app.editor_ctx.read_response(id).unwrap().rect.top() > 540.0,
        "wheel must remain free to scroll away from the focused row"
    );
    assert_eq!(app.editor_ctx.memory(|memory| memory.focused()), Some(id));
    frame(&mut app, vec![key_event(egui::Key::End, false)]);
    settle(&mut app);
    let output = frame(&mut app, vec![]);
    let response = app.editor_ctx.read_response(id).unwrap();
    let (clip, painted) = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::epaint::Shape::Text(text) if text.galley.text().contains(last) => {
                Some((shape.clip_rect, text.visual_bounding_rect()))
            }
            _ => None,
        })
        .expect("explicit End must repaint the same selected final row");
    assert!(clip.contains_rect(response.rect));
    assert!(clip.contains_rect(painted));
    assert!(commands.try_recv().is_err());
}

fn global_retry_fixture(tree: bool) -> (CedarApp, Receiver<Command>) {
    let (mut app, commands) = fixture();
    if tree {
        root(
            &mut app,
            &commands,
            (0..65).map(|n| entry(&format!("d{n:02}"), true)).collect(),
        );
        for n in 0..63 {
            app.explorer_expand(&format!("d{n:02}"));
            reply(&mut app, commands.try_recv().unwrap(), vec![]);
        }
        app.explorer_expand("d63");
        assert!(commands.try_recv().is_err());
    } else {
        app.list("missing-a".into());
        error(
            &mut app,
            commands.try_recv().unwrap(),
            &"bounded directory failure ".repeat(20),
            true,
        );
    }
    app.tools_open = true;
    app.tool = Tool::Search;
    app.editor_ctx.data_mut(|data| {
        data.insert_persisted(
            egui::Id::new("explorer"),
            egui::containers::panel::PanelState {
                rect: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(180.0, 540.0)),
            },
        )
    });
    settle(&mut app);
    (app, commands)
}

fn focus_rendered(app: &mut CedarApp, id: egui::Id) {
    app.editor_ctx
        .data_mut(|data| data.insert_temp(egui::Id::new("tree_test_focus"), id));
    settle(app);
}

fn footer_ids(app: &CedarApp) -> Vec<egui::Id> {
    ["Search", "Git", "Run", "LSP", "Tests"]
        .iter()
        .map(|label| workspace_access_tests::recorded_response(app, &format!("sidebar_{label}")).id)
        .collect()
}

#[test]
fn global_retry_controls_keep_path_identity_and_reveal_in_compact_outer_scroll() {
    for tree in [false, true] {
        let (mut app, commands) = global_retry_fixture(tree);
        let footer = footer_ids(&app);
        let expected_path = if tree { "d63" } else { "missing-a" };
        let id = workspace_access_tests::recorded_response(&app, "explorer_status_retry").id;
        assert_eq!(
            id,
            explorer_tree::retry_id(if tree { "tree_limit" } else { "flat" }, expected_path)
        );
        // Adding/removing an unrelated status widget cannot rename Retry or
        // any of the five existing footer selectors.
        app.explorer.message = Some("An additional status label".into());
        settle(&mut app);
        assert_eq!(
            workspace_access_tests::recorded_response(&app, "explorer_status_retry").id,
            id
        );
        assert_eq!(footer_ids(&app), footer);
        app.explorer.message = None;
        settle(&mut app);
        assert_eq!(footer_ids(&app), footer);
        let sidebar =
            egui::containers::panel::PanelState::load(&app.editor_ctx, egui::Id::new("explorer"))
                .unwrap()
                .rect;
        let at = egui::pos2(sidebar.right() - 17.0, sidebar.bottom() - 20.0);
        frame(
            &mut app,
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -5000.0),
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        for _ in 0..25 {
            frame(&mut app, vec![]);
        }
        focus_rendered(&mut app, id);
        let output = frame(&mut app, vec![]);
        let response = workspace_access_tests::recorded_response(&app, "explorer_status_retry");
        let label = if tree { "Retry branch" } else { "Retry" };
        let (clip, painted) = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                    Some((shape.clip_rect, text.visual_bounding_rect()))
                }
                _ => None,
            })
            .expect("global retry must paint after keyboard focus reveal");
        assert!(clip.contains_rect(response.rect));
        assert!(clip.contains_rect(painted));
        assert_eq!(app.editor_ctx.memory(|memory| memory.focused()), Some(id));
        if tree {
            app.explorer_collapse("d00");
        }
        frame(&mut app, vec![key_event(egui::Key::Enter, false)]);
        assert!(
            matches!(commands.try_recv().unwrap().op, Operation::List { path } if path == expected_path)
        );
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn captured_global_retry_press_cannot_dispatch_a_replacement_target() {
    for tree in [false, true] {
        let (mut app, commands) = global_retry_fixture(tree);
        let original = workspace_access_tests::recorded_response(&app, "explorer_status_retry").id;
        focus_rendered(&mut app, original);
        let at = app
            .editor_ctx
            .read_response(original)
            .unwrap()
            .rect
            .center();
        frame(
            &mut app,
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        if tree {
            app.explorer_expand("d64");
            app.explorer_collapse("d00"); // A wrongly retargeted click could now dispatch.
        } else {
            app.list("missing-b".into());
            error(
                &mut app,
                commands.try_recv().unwrap(),
                &"bounded directory failure ".repeat(20),
                true,
            );
        }
        frame(
            &mut app,
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        assert_ne!(
            workspace_access_tests::recorded_response(&app, "explorer_status_retry").id,
            original
        );
        assert!(
            commands.try_recv().is_err(),
            "a captured press must not retry a different directory"
        );
    }
}

#[test]
fn branch_retry_and_footer_ids_survive_inserted_preceding_sibling_rows() {
    let (mut app, commands) = fixture();
    root(
        &mut app,
        &commands,
        vec![entry("a", true), entry("z", true)],
    );
    app.explorer_expand("z");
    error(&mut app, commands.try_recv().unwrap(), "failed z", true);
    settle(&mut app);
    let id = workspace_access_tests::recorded_response(&app, "tree_retry:z").id;
    let footer = footer_ids(&app);
    assert_eq!(id, explorer_tree::retry_id("branch", "z"));
    app.explorer_expand("a");
    settle(&mut app); // Loading status adds a widget in its fixed scope.
    assert_eq!(footer_ids(&app), footer);
    reply(
        &mut app,
        commands.try_recv().unwrap(),
        (0..12).map(|n| entry(&format!("a/f{n}"), false)).collect(),
    );
    settle(&mut app);
    assert_eq!(
        workspace_access_tests::recorded_response(&app, "tree_retry:z").id,
        id
    );
    assert_eq!(footer_ids(&app), footer);
}

#[test]
fn explicit_explorer_open_admits_one_location_after_read_completion() {
    let (mut app, commands) = fixture();
    root(&mut app, &commands, vec![entry("next.rs", false)]);
    app.documents.push(Document::new(
        1,
        "source.rs".into(),
        "source".into(),
        "r".into(),
    ));
    app.active_document = Some(1);
    app.next_document = 2;
    app.explorer_activate("next.rs");
    let command = commands.try_recv().unwrap();
    assert!(app.location_history.back.is_empty());
    app.apply_event(Event {
        generation: app.generation,
        id: command.id,
        connected: true,
        result: Ok(Payload::File {
            path: "next.rs".into(),
            text: "next".into(),
            revision: "r".into(),
        }),
    });
    assert_eq!(app.location_history.back.len(), 1);
    assert_eq!(app.location_history.back[0].document, 1);
    assert!(app.location_history.pending.is_none());
    assert!(commands.try_recv().is_err());
}
