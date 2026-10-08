//! Headless native integration with actual private storage and synthetic workspaces.
use super::*;
use cedar_recovery::{record_id, Draft, Store, WorkspaceIdentity};
use std::time::{Duration, Instant};

fn workspace() -> WorkspaceIdentity {
    WorkspaceIdentity::Local {
        root: "/synthetic/project".into(),
    }
}
fn sample() -> Draft {
    Draft {
        workspace: workspace(),
        path: "main.rs".into(),
        text: "recovered text".into(),
        base_text: "original disk".into(),
        base_revision: Some("original-revision".into()),
        modified_ms: 1,
    }
}
fn app_at(path: &std::path::Path) -> CedarApp {
    let mut app = CedarApp::empty();
    app.recovery
        .start(Ok(path.into()), &egui::Context::default());
    wait(&mut app, |app| {
        app.recovery.initialized || app.recovery.error.is_some()
    });
    app
}
fn connected(app: &mut CedarApp) {
    let form = ConnectForm {
        local_root: "/synthetic/project".into(),
        allow_run: false,
        ..Default::default()
    };
    app.workspace_key = Some(form.key());
    app.active_form = Some(form);
    app.root = "/synthetic/project".into();
    app.state = ConnectionState::Ready;
    app.open_form = false;
}
fn wait(app: &mut CedarApp, done: impl Fn(&CedarApp) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        app.recovery_tick(&egui::Context::default());
        app.finish_recovery_close_frame(&egui::Context::default());
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
fn dirty_doc(app: &mut CedarApp) {
    let mut doc = Document::new(
        1,
        "main.rs".into(),
        "original disk".into(),
        "original-revision".into(),
    );
    doc.text = "current draft".into();
    doc.edit_version = 1;
    app.documents.push(doc);
    app.active_document = Some(1);
    app.next_document = 2;
}
fn persist(app: &mut CedarApp) {
    app.recovery_tick(&egui::Context::default());
    app.recovery.flush();
    wait(app, |app| {
        app.recovery.protected(&workspace(), &app.documents[0])
    });
}

#[test]
fn disk_comparison_and_rejected_dirty_reload_preserve_owned_recovery() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("recovery");
    let mut app = app_at(&path);
    connected(&mut app);
    dirty_doc(&mut app);
    persist(&mut app);
    let (worker, rx) = worker::Worker::recording();
    app.worker = Some(worker);
    app.compare_with_disk();
    let command = rx.try_recv().unwrap();
    assert!(matches!(command.op, Operation::Read { .. }));
    app.apply_event(Event {
        generation: app.generation,
        id: command.id,
        connected: true,
        result: Ok(Payload::File {
            path: "main.rs".into(),
            text: "externally changed disk".into(),
            revision: "b".repeat(64),
        }),
    });
    app.reload_from_disk();
    app.finish_disk_reload(&egui::Context::default());
    app.recovery_tick(&egui::Context::default());
    assert!(rx.try_recv().is_err());
    assert!(app.recovery.protected(&workspace(), &app.documents[0]));
    assert_eq!(app.documents[0].saved_text, "original disk");
    assert_eq!(
        app.documents[0].revision.as_deref(),
        Some("original-revision")
    );
    drop(app);
    let store = Store::open(path).unwrap();
    let saved = store
        .read(&record_id(&workspace(), "main.rs").unwrap())
        .unwrap();
    assert_eq!(saved.text, "current draft");
    assert_eq!(saved.base_text, "original disk");
    assert_eq!(saved.base_revision.as_deref(), Some("original-revision"));
}

#[test]
fn clean_disk_reload_undo_and_tab_discard_never_take_ownership_of_older_recovery() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("recovery");
    {
        Store::open(&path).unwrap().write(1, &sample()).unwrap();
    }
    let mut app = app_at(&path);
    connected(&mut app);
    app.documents.push(Document::new(
        1,
        "main.rs".into(),
        "original disk".into(),
        "a".repeat(64),
    ));
    app.active_document = Some(1);
    let (worker, rx) = worker::Worker::recording();
    app.worker = Some(worker);
    for verify in [false, true] {
        if verify {
            app.reload_from_disk();
        } else {
            app.compare_with_disk();
        }
        let command = rx.try_recv().unwrap();
        assert!(matches!(command.op, Operation::Read { .. }));
        app.apply_event(Event {
            generation: app.generation,
            id: command.id,
            connected: true,
            result: Ok(Payload::File {
                path: "main.rs".into(),
                text: "new disk".into(),
                revision: "b".repeat(64),
            }),
        });
    }
    let ctx = app.editor_ctx.clone();
    app.finish_disk_reload(&ctx);
    app.recovery_tick(&ctx);
    assert_eq!(app.documents[0].text, "new disk");
    assert!(!app.documents[0].dirty());
    let _ = ctx.run(
        egui::RawInput {
            events: vec![egui::Event::Key {
                key: egui::Key::Z,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::COMMAND,
            }],
            ..Default::default()
        },
        |ctx| {
            ctx.memory_mut(|memory| memory.request_focus(egui::Id::new(("editor", 1u64))));
            editor_state::history_shortcut(ctx, &mut app.documents[0]);
        },
    );
    app.recovery_tick(&ctx);
    assert_eq!(app.documents[0].text, "original disk");
    assert!(app.documents[0].dirty());
    assert_eq!(
        app.recovery.status(Some(&workspace()), app.active()).0,
        "Older recovery waiting"
    );
    app.remove_tab(1);
    drop(app);
    let store = Store::open(path).unwrap();
    let old = store
        .read(&record_id(&workspace(), "main.rs").unwrap())
        .unwrap();
    assert_eq!(old.text, sample().text);
    assert_eq!(old.base_revision, sample().base_revision);
}
#[test]
fn durable_status_waits_for_matching_actual_store_ack() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("recovery");
    let mut app = app_at(&path);
    connected(&mut app);
    dirty_doc(&mut app);
    app.recovery_tick(&egui::Context::default());
    assert!(!app.recovery.protected(&workspace(), &app.documents[0]));
    persist(&mut app);
    assert!(app.recovery.protected(&workspace(), &app.documents[0]));
    app.documents[0].text.push_str(" newer");
    app.documents[0].edit_version += 1;
    assert!(!app.recovery.protected(&workspace(), &app.documents[0]));
    persist(&mut app);
    drop(app);
    let store = Store::open(path).unwrap();
    assert_eq!(
        store
            .read(&record_id(&workspace(), "main.rs").unwrap())
            .unwrap()
            .text,
        "current draft newer"
    );
}
#[test]
fn save_while_typing_preserves_newer_draft_and_updates_base() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("recovery");
    let mut app = app_at(&path);
    connected(&mut app);
    dirty_doc(&mut app);
    persist(&mut app);
    app.documents[0].text = "typed after save request".into();
    app.documents[0].edit_version += 1;
    app.pending.insert(
        17,
        Job::Save {
            document: 1,
            snapshot: "current draft".into(),
            submission: None,
        },
    );
    app.apply_event(Event {
        generation: 0,
        id: 17,
        connected: true,
        result: Ok(Payload::Written {
            revision: "next-revision".into(),
        }),
    });
    persist(&mut app);
    drop(app);
    let store = Store::open(path).unwrap();
    let draft = store
        .read(&record_id(&workspace(), "main.rs").unwrap())
        .unwrap();
    assert_eq!(draft.text, "typed after save request");
    assert_eq!(draft.base_text, "current draft");
    assert_eq!(draft.base_revision.as_deref(), Some("next-revision"));
}
#[test]
fn explicit_discard_removes_owned_copy_and_pending_write() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("recovery");
    let mut app = app_at(&path);
    connected(&mut app);
    dirty_doc(&mut app);
    app.recovery_tick(&egui::Context::default());
    app.remove_tab(1);
    drop(app);
    assert!(Store::open(path).unwrap().list().unwrap().drafts.is_empty());
}
#[test]
fn unopened_recovery_survives_unrelated_tab_save_and_discard() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("recovery");
    {
        let mut store = Store::open(&path).unwrap();
        store.write(1, &sample()).unwrap();
    }
    let mut app = app_at(&path);
    connected(&mut app);
    dirty_doc(&mut app);
    app.recovery_tick(&egui::Context::default());
    assert_eq!(
        app.recovery.status(Some(&workspace()), app.active()).0,
        "Older recovery waiting"
    );
    app.pending.insert(
        17,
        Job::Save {
            document: 1,
            snapshot: "current draft".into(),
            submission: None,
        },
    );
    app.apply_event(Event {
        generation: 0,
        id: 17,
        connected: true,
        result: Ok(Payload::Written {
            revision: "changed-disk".into(),
        }),
    });
    app.remove_tab(1);
    drop(app);
    let store = Store::open(path).unwrap();
    assert_eq!(
        store
            .read(&record_id(&workspace(), "main.rs").unwrap())
            .unwrap()
            .text,
        "recovered text"
    );
}
#[test]
fn startup_never_connects_or_starts_tools_and_restore_retains_original_base() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("recovery");
    {
        let mut store = Store::open(&path).unwrap();
        store.write(1, &sample()).unwrap();
    }
    let mut app = app_at(&path);
    assert!(app.worker.is_none());
    assert!(app.active_form.is_none());
    assert!(!app.language.running);
    assert!(app.recovery.visible);
    assert_eq!(app.recovery.drafts.len(), 1);
    connected(&mut app);
    app.install_recovered(sample()).unwrap();
    assert!(app.documents[0].dirty());
    assert!(!app.documents[0].saving);
    assert_eq!(app.documents[0].saved_text, "original disk");
    assert_eq!(
        app.documents[0].revision.as_deref(),
        Some("original-revision")
    );
    // A conflict from a newer disk revision leaves the restored text untouched.
    app.pending.insert(
        18,
        Job::Save {
            document: app.documents[0].id,
            snapshot: "recovered text".into(),
            submission: None,
        },
    );
    app.apply_event(Event {
        generation: 0,
        id: 18,
        connected: true,
        result: Err("conflict: disk changed externally".into()),
    });
    assert_eq!(app.documents[0].text, "recovered text");
    assert_eq!(
        app.documents[0].revision.as_deref(),
        Some("original-revision")
    );
    assert!(!app.language.running);
    assert!(!app.active_form.as_ref().unwrap().allow_run);
}
#[test]
fn restore_rejects_identity_mismatch_trust_and_existing_dirty_tab() {
    let mut app = CedarApp::empty();
    connected(&mut app);
    app.root = "/wrong".into();
    assert!(app.install_recovered(sample()).is_err());
    app.root = "/synthetic/project".into();
    app.active_form.as_mut().unwrap().allow_run = true;
    assert!(app.install_recovered(sample()).is_err());
    app.active_form.as_mut().unwrap().allow_run = false;
    dirty_doc(&mut app);
    assert!(app.install_recovered(sample()).is_err());
    assert_eq!(app.documents[0].text, "current draft");
}
#[test]
fn recovery_connect_handshake_must_match_before_clearing_current_tabs() {
    let mut app = CedarApp::empty();
    connected(&mut app);
    app.documents.push(Document::new(
        1,
        "other.rs".into(),
        "safe".into(),
        "r".into(),
    ));
    app.recovery.pending_restore = Some(sample());
    app.recovery.restoring_generation = Some(3);
    app.generation = 3;
    app.state = ConnectionState::Connecting;
    app.connecting_form = Some(ConnectForm {
        local_root: "/synthetic/project".into(),
        ..Default::default()
    });
    app.apply_event(Event {
        generation: 3,
        id: 0,
        connected: true,
        result: Ok(Payload::Hello {
            protocol: cedar_protocol::PROTOCOL_VERSION,
            agent: None,
            root: "/wrong".into(),
        }),
    });
    assert_eq!(app.documents[0].text, "safe");
    assert!(app.recovery.pending_restore.is_some());
    assert!(app.recovery.error.is_some());
    assert!(app.state == ConnectionState::Disconnected);
}
#[test]
fn busy_store_is_actionable_and_retry_after_lock_release_works() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("recovery");
    let owner = Store::open(&path).unwrap();
    let mut app = app_at(&path);
    assert!(app.recovery.error.is_some());
    assert!(!app.recovery.initialized);
    connected(&mut app);
    dirty_doc(&mut app);
    assert_eq!(app.documents[0].text, "current draft");
    drop(owner);
    app.recovery.retry(&egui::Context::default());
    wait(&mut app, |app| app.recovery.initialized);
    persist(&mut app);
}
#[test]
fn disabled_recovery_retains_existing_copy_and_does_not_certify_new_typing() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("recovery");
    let mut app = app_at(&path);
    connected(&mut app);
    dirty_doc(&mut app);
    persist(&mut app);
    app.recovery.set_enabled(false);
    app.documents[0].text.push_str(" off");
    app.documents[0].edit_version += 1;
    app.recovery_tick(&egui::Context::default());
    assert!(!app.recovery.protected(&workspace(), &app.documents[0]));
    drop(app);
    let store = Store::open(path).unwrap();
    assert_eq!(
        store
            .read(&record_id(&workspace(), "main.rs").unwrap())
            .unwrap()
            .text,
        "current draft"
    );
}
#[test]
fn recovery_review_layout_is_headless_safe() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("recovery");
    {
        let mut store = Store::open(&path).unwrap();
        store.write(1, &sample()).unwrap();
    }
    let mut app = app_at(&path);
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
            |ctx| app.recovery_window(ctx),
        );
        assert!(!output.shapes.is_empty());
    }
}

#[test]
fn corrupt_copy_is_visible_retained_and_not_restored() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("recovery");
    {
        let mut store = Store::open(&path).unwrap();
        store.write(1, &sample()).unwrap();
    }
    let file = path.join(format!(
        "{}.draft",
        record_id(&workspace(), "main.rs").unwrap()
    ));
    std::fs::write(&file, b"damaged synthetic record").unwrap();
    let mut app = app_at(&path);
    assert!(app.recovery.drafts.is_empty());
    assert_eq!(app.recovery.issues.len(), 1);
    assert!(app.recovery.visible);
    connected(&mut app);
    dirty_doc(&mut app);
    app.recovery_tick(&egui::Context::default());
    app.recovery.flush();
    wait(&mut app, |app| !app.recovery.failures().is_empty());
    assert!(!app.recovery.protected(&workspace(), &app.documents[0]));
    drop(app);
    assert_eq!(std::fs::read(file).unwrap(), b"damaged synthetic record");
}
#[test]
fn full_store_does_not_prune_and_leaves_current_text_editable() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("recovery");
    {
        let mut store = Store::open(&path).unwrap();
        for index in 0..cedar_recovery::MAX_RECORDS {
            let mut draft = sample();
            draft.path = format!("old-{index}.rs");
            store.write(index as u64 + 1, &draft).unwrap();
        }
    }
    let mut app = app_at(&path);
    connected(&mut app);
    dirty_doc(&mut app);
    app.recovery_tick(&egui::Context::default());
    app.recovery.flush();
    wait(&mut app, |app| !app.recovery.failures().is_empty());
    assert_eq!(app.documents[0].text, "current draft");
    assert!(!app.recovery.protected(&workspace(), &app.documents[0]));
    assert_eq!(app.recovery.drafts.len(), cedar_recovery::MAX_RECORDS);
    drop(app);
    assert_eq!(
        Store::open(path).unwrap().list().unwrap().drafts.len(),
        cedar_recovery::MAX_RECORDS
    );
}
#[test]
fn workspace_switch_does_not_move_pending_drafts_or_inherit_protection() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("recovery");
    let mut app = app_at(&path);
    connected(&mut app);
    dirty_doc(&mut app);
    app.recovery_tick(&egui::Context::default());
    app.disconnected("synthetic disconnect".into());
    persist(&mut app);
    assert!(app.recovery.protected(&workspace(), &app.documents[0]));
    app.connecting_form = app.active_form.clone();
    app.state = ConnectionState::Connecting;
    app.apply_event(Event {
        generation: 0,
        id: 0,
        connected: true,
        result: Ok(Payload::Hello {
            protocol: cedar_protocol::PROTOCOL_VERSION,
            agent: None,
            root: "/different/root".into(),
        }),
    });
    assert!(app.state == ConnectionState::Disconnected);
    assert_eq!(app.root, "/synthetic/project");
    assert_eq!(app.documents[0].text, "current draft");
}
#[test]
fn discard_and_quit_waits_for_removal_ack_and_new_typing_cancels_close() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("recovery");
    let mut app = app_at(&path);
    connected(&mut app);
    dirty_doc(&mut app);
    persist(&mut app);
    app.finish_recovery_close(&egui::Context::default());
    assert!(!app.allow_close);
    assert!(app.recovery.closing.is_some());
    app.documents[0].text.push_str(" after close");
    app.documents[0].edit_version += 1;
    app.recovery_tick(&egui::Context::default());
    assert!(!app.allow_close);
    assert!(app.recovery.closing.is_none());
    assert!(matches!(app.confirm, Some(Confirm::CloseWindow)));
    persist(&mut app);
    app.finish_recovery_close(&egui::Context::default());
    wait(&mut app, |app| app.allow_close);
    drop(app);
    assert!(Store::open(path).unwrap().list().unwrap().drafts.is_empty());
}
#[test]
fn shutdown_with_newer_typing_flushes_latest_and_new_file_base_stays_none() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("recovery");
    let mut app = app_at(&path);
    connected(&mut app);
    dirty_doc(&mut app);
    app.documents[0].revision = None;
    app.documents[0].saved_text.clear();
    app.recovery_tick(&egui::Context::default());
    app.documents[0].text = "latest pending new file".into();
    app.documents[0].edit_version += 1;
    app.recovery_tick(&egui::Context::default());
    drop(app);
    let store = Store::open(path).unwrap();
    let draft = store
        .read(&record_id(&workspace(), "main.rs").unwrap())
        .unwrap();
    assert_eq!(draft.text, "latest pending new file");
    assert_eq!(draft.base_revision, None);
    assert!(draft.base_text.is_empty());
}

#[test]
fn actual_recovered_base_conflicts_with_external_disk_edit_without_overwrite() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    std::fs::create_dir(&project).unwrap();
    std::fs::write(project.join("main.rs"), "original disk").unwrap();
    let mut backend = cedar_workspace::Workspace::open(&project).unwrap();
    let Payload::File { revision, text, .. } = backend
        .handle(Operation::Read {
            path: "main.rs".into(),
        })
        .unwrap()
    else {
        panic!("file read")
    };
    let root = backend.root().to_string_lossy().into_owned();
    let workspace = WorkspaceIdentity::Local { root: root.clone() };
    let draft = Draft {
        workspace: workspace.clone(),
        path: "main.rs".into(),
        text: "precious crash draft".into(),
        base_text: text,
        base_revision: Some(revision),
        modified_ms: 1,
    };
    let path = temp.path().join("recovery");
    {
        let mut store = Store::open(&path).unwrap();
        store.write(1, &draft).unwrap();
    }
    std::fs::write(project.join("main.rs"), "changed outside editor").unwrap();
    let mut app = app_at(&path);
    let form = ConnectForm {
        local_root: root.clone(),
        allow_run: false,
        ..Default::default()
    };
    app.workspace_key = Some(form.key());
    app.active_form = Some(form);
    app.root = root;
    app.state = ConnectionState::Ready;
    let id = record_id(&workspace, "main.rs").unwrap();
    app.recovery.request_restore(id);
    wait(&mut app, |app| app.recovery.pending_restore.is_some());
    let restored = app.recovery.pending_restore.take().unwrap();
    app.install_recovered(restored).unwrap();
    let doc = &app.documents[0];
    let result = backend.handle(Operation::Write {
        path: doc.path.clone(),
        text: doc.text.clone(),
        expected_revision: doc.revision.clone(),
    });
    assert_eq!(result.unwrap_err().code, "conflict");
    assert_eq!(
        std::fs::read_to_string(project.join("main.rs")).unwrap(),
        "changed outside editor"
    );
    assert_eq!(app.documents[0].text, "precious crash draft");
    assert!(app.documents[0].dirty());
}

#[test]
fn cached_review_then_remove_then_restore_requires_fresh_durable_ack() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("recovery");
    {
        let mut store = Store::open(&path).unwrap();
        store.write(1, &sample()).unwrap();
    }
    let mut app = app_at(&path);
    connected(&mut app);
    app.recovery
        .request_restore(record_id(&workspace(), "main.rs").unwrap());
    wait(&mut app, |app| app.recovery.pending_restore.is_some());
    app.recovery.remove(workspace(), "main.rs".into());
    wait(&mut app, |app| {
        app.recovery.removals_finished() && app.recovery.drafts.is_empty()
    });
    let cached = app.recovery.pending_restore.take().unwrap();
    app.install_recovered(cached).unwrap();
    assert!(!app.recovery.protected(&workspace(), &app.documents[0]));
    persist(&mut app);
    drop(app);
    let store = Store::open(path).unwrap();
    assert_eq!(
        store
            .read(&record_id(&workspace(), "main.rs").unwrap())
            .unwrap()
            .text,
        "recovered text"
    );
}

#[test]
fn removal_ack_before_frame_input_cannot_close_over_new_typing() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("recovery");
    let mut app = app_at(&path);
    connected(&mut app);
    dirty_doc(&mut app);
    persist(&mut app);
    app.finish_recovery_close(&egui::Context::default());
    let deadline = Instant::now() + Duration::from_secs(5);
    while !app.recovery.removals_finished() {
        app.recovery.poll();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    app.recovery_tick(&egui::Context::default());
    assert!(
        !app.allow_close,
        "first tick cannot commit Close before input"
    );
    app.documents[0].text.push_str(" same-frame input");
    app.documents[0].edit_version += 1;
    app.recovery_tick(&egui::Context::default());
    app.finish_recovery_close_frame(&egui::Context::default());
    assert!(!app.allow_close);
    assert!(app.recovery.closing.is_none());
    assert!(matches!(app.confirm, Some(Confirm::CloseWindow)));
}

#[test]
fn save_started_during_recovery_close_cancels_final_close() {
    let mut app = CedarApp::empty();
    connected(&mut app);
    dirty_doc(&mut app);
    app.finish_recovery_close(&egui::Context::default());
    app.pending.insert(
        77,
        Job::Save {
            document: 1,
            snapshot: "current draft".into(),
            submission: None,
        },
    );
    app.finish_recovery_close_frame(&egui::Context::default());
    assert!(!app.allow_close);
    assert!(app.recovery.closing.is_none());
    assert_eq!(app.documents[0].text, "current draft");
}
