//! Close lifecycle tests use real recovery files and deterministic worker gates.
use super::*;
use crate::recovery::{ClosePhase, CLOSE_OBSERVATION};
use crate::recovery_actor::Availability;
use cedar_recovery::{record_id, Draft, Store, WorkspaceIdentity};
use std::time::{Duration, Instant};

fn workspace() -> WorkspaceIdentity {
    WorkspaceIdentity::Local {
        root: "/synthetic/project".into(),
    }
}
fn draft() -> Draft {
    Draft {
        workspace: workspace(),
        path: "main.rs".into(),
        text: "older recovery".into(),
        base_text: "base".into(),
        base_revision: Some("r0".into()),
        modified_ms: 1,
    }
}
fn app_at(path: &std::path::Path) -> CedarApp {
    let mut app = CedarApp::empty();
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
    app.recovery
        .start(Ok(path.into()), &egui::Context::default());
    wait(&mut app, |app| {
        app.recovery.initialized || app.recovery.error.is_some()
    });
    let mut doc = Document::new(1, "main.rs".into(), "base".into(), "r0".into());
    doc.text = "current unsaved text".into();
    doc.edit_version = 1;
    app.documents.push(doc);
    app.active_document = Some(1);
    app.next_document = 2;
    app
}
fn tick(app: &mut CedarApp) {
    app.recovery_tick(&egui::Context::default());
    app.finish_recovery_close_frame(&egui::Context::default());
}
fn wait(app: &mut CedarApp, done: impl Fn(&CedarApp) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done(app) {
        tick(app);
        assert!(
            Instant::now() < deadline,
            "recovery did not settle: {:?}",
            app.recovery.error
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn persist(app: &mut CedarApp) {
    app.recovery_tick(&egui::Context::default());
    app.recovery.flush();
    wait(app, |app| {
        app.recovery.protected(&workspace(), &app.documents[0])
    });
}
fn expire_discard(app: &mut CedarApp) {
    app.recovery.closing.as_mut().unwrap().phase = ClosePhase::Discarding {
        started: Instant::now() - CLOSE_OBSERVATION,
    };
    app.recovery.observe_close_deadline();
    assert!(matches!(
        app.recovery.closing.as_ref().unwrap().phase,
        ClosePhase::NeedsDecision
    ));
}
#[test]
fn failed_open_can_quit_only_after_explicit_retained_copy_confirmation() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("recovery");
    std::fs::write(&path, "blocker").unwrap();
    let mut app = app_at(&path);
    app.finish_recovery_close(&egui::Context::default());
    tick(&mut app);
    assert!(!app.allow_close);
    assert!(matches!(
        app.recovery.closing.as_ref().unwrap().phase,
        ClosePhase::NeedsDecision
    ));
    assert_eq!(app.recovery.test_snapshot(), (0, 0, 0));
    app.retain_recovery_and_quit();
    wait(&mut app, |app| {
        matches!(
            app.recovery.closing.as_ref().unwrap().phase,
            ClosePhase::AwaitingConfirmation { .. }
        )
    });
    assert!(!app.allow_close);
    assert!(!app.recovery.protected(&workspace(), &app.documents[0]));
    app.confirm_retained_recovery_close();
    tick(&mut app);
    assert!(app.allow_close);
    drop(app);
    assert_eq!(std::fs::read_to_string(path).unwrap(), "blocker");
}
#[test]
fn retry_after_failed_initial_listing_never_overwrites_unreviewed_copy() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("recovery");
    let mut locked = Store::open(&path).unwrap();
    locked.write(1, &draft()).unwrap();
    let mut app = app_at(&path);
    for _ in 0..20 {
        tick(&mut app);
    }
    assert_eq!(app.recovery.test_snapshot(), (0, 0, 0));
    drop(locked);
    app.recovery.retry(&egui::Context::default());
    wait(&mut app, |app| app.recovery.initialized);
    for _ in 0..20 {
        tick(&mut app);
    }
    assert!(!app.recovery.protected(&workspace(), &app.documents[0]));
    assert_eq!(
        app.recovery.status(Some(&workspace()), app.active()).0,
        "Older recovery waiting"
    );
    drop(app);
    assert_eq!(
        Store::open(path)
            .unwrap()
            .read(&record_id(&workspace(), "main.rs").unwrap())
            .unwrap()
            .text,
        "older recovery"
    );
}
#[test]
fn keep_editing_during_drain_waits_for_settlement_and_cancels_late_quit() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("recovery");
    let mut app = app_at(&path);
    let (entered, release) = app.recovery.hold_next_operation(false);
    app.recovery_tick(&egui::Context::default());
    app.recovery.flush();
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    app.finish_recovery_close(&egui::Context::default());
    expire_discard(&mut app);
    app.retain_recovery_and_quit();
    app.keep_editing_recovery(&egui::Context::default());
    assert!(app.recovery.resuming());
    assert!(!app.recovery.can_retry());
    app.documents[0].text.push_str(" after Keep editing");
    app.documents[0].edit_version += 1;
    tick(&mut app);
    assert!(!app.allow_close);
    release.send(()).unwrap();
    wait(&mut app, |app| !app.recovery.resuming());
    persist(&mut app);
    assert!(app.recovery.closing.is_none());
    assert!(!app.allow_close);
    drop(app);
    assert!(Store::open(path)
        .unwrap()
        .read(&record_id(&workspace(), "main.rs").unwrap())
        .unwrap()
        .text
        .ends_with("after Keep editing"));
}
#[test]
fn expired_barrier_never_accepts_a_late_proof() {
    let temp = tempfile::tempdir().unwrap();
    let mut app = app_at(&temp.path().join("recovery"));
    let (entered, release) = app.recovery.hold_next_operation(false);
    app.recovery_tick(&egui::Context::default());
    app.recovery.flush();
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    app.finish_recovery_close(&egui::Context::default());
    expire_discard(&mut app);
    app.retain_recovery_and_quit();
    if let ClosePhase::Draining { started, .. } = &mut app.recovery.closing.as_mut().unwrap().phase
    {
        *started = Instant::now() - CLOSE_OBSERVATION;
    }
    tick(&mut app);
    assert!(matches!(
        app.recovery.closing.as_ref().unwrap().phase,
        ClosePhase::Blocked { .. }
    ));
    release.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.recovery.test_snapshot().0 != 0
        || app.recovery.availability() == Availability::Starting
    {
        tick(&mut app);
        assert!(Instant::now() < deadline);
    }
    // Give the worker's proof an observable UI turn; no deadline can re-arm it.
    for _ in 0..20 {
        tick(&mut app);
        std::thread::sleep(Duration::from_millis(1));
    }
    app.confirm_retained_recovery_close();
    tick(&mut app);
    assert!(!app.allow_close);
    assert!(matches!(
        app.recovery.closing.as_ref().unwrap().phase,
        ClosePhase::Blocked { .. }
    ));
    app.keep_editing_recovery(&egui::Context::default());
    wait(&mut app, |app| !app.recovery.resuming());
}
#[test]
fn stopped_worker_during_resume_allows_retry_without_fabricating_proof() {
    let temp = tempfile::tempdir().unwrap();
    let mut app = app_at(&temp.path().join("recovery"));
    let (entered, release) = app.recovery.hold_next_operation(true);
    app.recovery_tick(&egui::Context::default());
    app.recovery.flush();
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    app.finish_recovery_close(&egui::Context::default());
    expire_discard(&mut app);
    app.retain_recovery_and_quit();
    app.keep_editing_recovery(&egui::Context::default());
    release.send(()).unwrap();
    wait(&mut app, |app| {
        app.recovery.availability() == Availability::Stopped && !app.recovery.resuming()
    });
    assert!(!app.allow_close);
    assert!(app.recovery.can_retry());
    app.recovery.retry(&egui::Context::default());
    wait(&mut app, |app| app.recovery.initialized);
    persist(&mut app);
}
#[test]
fn retained_confirmation_is_invalidated_by_document_workspace_or_profile_change() {
    for change in 0..4 {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("blocked");
        std::fs::write(&path, "blocker").unwrap();
        let mut app = app_at(&path);
        app.finish_recovery_close(&egui::Context::default());
        app.retain_recovery_and_quit();
        wait(&mut app, |app| {
            matches!(
                app.recovery.closing.as_ref().unwrap().phase,
                ClosePhase::AwaitingConfirmation { .. }
            )
        });
        app.confirm_retained_recovery_close();
        match change {
            0 => {
                app.documents[0].text.push('!');
                app.documents[0].edit_version += 1;
            }
            1 => app.generation += 1,
            2 => app.root.push_str("/changed"),
            _ => {
                app.profiles.draft.name.push_str("new form edit");
                app.profiles.changed();
            }
        }
        tick(&mut app);
        assert!(!app.allow_close);
        assert!(app.recovery.closing.is_none());
    }
}
#[test]
fn applied_discard_then_keep_editing_backs_up_unchanged_dirty_text_again() {
    let temp = tempfile::tempdir().unwrap();
    let mut app = app_at(&temp.path().join("recovery"));
    persist(&mut app);
    app.finish_recovery_close(&egui::Context::default());
    let deadline = Instant::now() + Duration::from_secs(5);
    while !app.recovery.removals_finished() {
        app.recovery.poll();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    let text = app.documents[0].text.clone();
    app.keep_editing_recovery(&egui::Context::default());
    wait(&mut app, |app| !app.recovery.resuming());
    persist(&mut app);
    assert_eq!(app.documents[0].text, text);
    assert!(!app.allow_close);
}
#[test]
fn clean_retained_copy_is_not_removed_by_retry_or_normal_save_cleanup() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("recovery");
    let mut app = app_at(&path);
    persist(&mut app);
    // Start the retained path without authorizing any deletion.
    app.recovery.begin_close(crate::recovery::CloseGuard::new(
        &app.documents,
        Some(workspace()),
        app.generation,
        app.profiles.epoch,
    ));
    app.recovery.closing.as_mut().unwrap().phase = ClosePhase::NeedsDecision;
    app.retain_recovery_and_quit();
    wait(&mut app, |app| {
        matches!(
            app.recovery.closing.as_ref().unwrap().phase,
            ClosePhase::AwaitingConfirmation { .. }
        )
    });
    app.documents[0].saved_text = app.documents[0].text.clone();
    app.keep_editing_recovery(&egui::Context::default());
    wait(&mut app, |app| !app.recovery.resuming());
    app.recovery.retry(&egui::Context::default());
    wait(&mut app, |app| app.recovery.initialized);
    for _ in 0..20 {
        tick(&mut app);
    }
    assert_eq!(app.recovery.test_snapshot().2, 0);
    drop(app);
    assert_eq!(Store::open(path).unwrap().list().unwrap().drafts.len(), 1);
}

#[test]
fn keep_editing_before_in_flight_remove_ack_rebacks_up_unchanged_dirty_text() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("recovery");
    let mut app = app_at(&path);
    persist(&mut app);
    app.recovery.refresh(false);
    wait(&mut app, |app| {
        !app.recovery.loading && !app.recovery.drafts.is_empty()
    });
    let (entered, release) = app.recovery.hold_next_operation(false);
    app.finish_recovery_close(&egui::Context::default());
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    let text = app.documents[0].text.clone();
    app.keep_editing_recovery(&egui::Context::default());
    assert!(app.recovery.resuming());
    app.request_window_close(&egui::Context::default());
    app.begin_close(&egui::Context::default());
    assert!(app.recovery.closing.is_none());
    assert!(app.confirm.is_none());
    release.send(()).unwrap();
    wait(&mut app, |app| !app.recovery.resuming());
    persist(&mut app);
    assert_eq!(app.documents[0].text, text);
    assert!(!app.allow_close);
    drop(app);
    assert_eq!(
        Store::open(path)
            .unwrap()
            .read(&record_id(&workspace(), "main.rs").unwrap())
            .unwrap()
            .text,
        text
    );
}
#[test]
fn retained_copy_is_not_adopted_by_a_reopened_tab_with_a_new_document_id() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("recovery");
    let mut app = app_at(&path);
    persist(&mut app);
    let original = app.documents[0].text.clone();
    app.recovery.begin_close(app.recovery_close_guard());
    app.recovery.closing.as_mut().unwrap().phase = ClosePhase::NeedsDecision;
    app.retain_recovery_and_quit();
    wait(&mut app, |app| {
        matches!(
            app.recovery.closing.as_ref().unwrap().phase,
            ClosePhase::AwaitingConfirmation { .. }
        )
    });
    app.documents[0].saved_text = original.clone();
    app.keep_editing_recovery(&egui::Context::default());
    wait(&mut app, |app| !app.recovery.resuming());
    app.recovery.retry(&egui::Context::default());
    wait(&mut app, |app| app.recovery.initialized);
    app.remove_tab(1);
    let mut reopened = Document::new(2, "main.rs".into(), "new base".into(), "r1".into());
    reopened.text = "different tab text must not replace retained draft".into();
    reopened.edit_version = 1;
    app.documents.push(reopened);
    app.active_document = Some(2);
    for _ in 0..20 {
        tick(&mut app);
    }
    assert!(!app.recovery.protected(&workspace(), &app.documents[0]));
    assert_eq!(
        app.recovery.status(Some(&workspace()), app.active()).0,
        "Older recovery waiting"
    );
    assert_eq!(app.recovery.test_snapshot().0, 0);
    drop(app);
    assert_eq!(
        Store::open(path)
            .unwrap()
            .read(&record_id(&workspace(), "main.rs").unwrap())
            .unwrap()
            .text,
        original
    );
}
#[test]
fn language_stop_bridge_rejects_changed_workspace_session_confirmation() {
    for change_workspace in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let mut app = app_at(&temp.path().join("recovery"));
        app.close_after_language_stop = true;
        app.close_snapshot = Some(app.draft_versions());
        app.recovery.language_close_guard = Some(app.recovery_close_guard());
        if change_workspace {
            app.root.push_str("/other");
        } else {
            app.generation += 1;
        }
        app.finish_pending_close(&egui::Context::default());
        assert!(!app.allow_close);
        assert!(app.recovery.closing.is_none());
        assert!(app.recovery.language_close_guard.is_none());
        assert!(app.close_snapshot.is_none());
        assert!(matches!(app.confirm, Some(Confirm::CloseWindow)));
    }
}

#[test]
fn keep_editing_clears_canceled_queued_read_and_refresh_without_leaving_ui_busy() {
    let temp = tempfile::tempdir().unwrap();
    let mut app = app_at(&temp.path().join("recovery"));
    persist(&mut app);
    let (entered, release) = app.recovery.hold_next_operation(false);
    app.documents[0].text.push_str(" newer");
    app.documents[0].edit_version += 1;
    app.recovery_tick(&egui::Context::default());
    app.recovery.flush();
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    app.recovery
        .request_restore(record_id(&workspace(), "main.rs").unwrap());
    app.recovery.refresh(false);
    assert!(app.recovery.reading.is_some());
    assert!(app.recovery.loading);
    app.finish_recovery_close(&egui::Context::default());
    app.keep_editing_recovery(&egui::Context::default());
    assert!(app.recovery.reading.is_none());
    assert!(!app.recovery.loading);
    release.send(()).unwrap();
    wait(&mut app, |app| !app.recovery.resuming());
    assert!(app.recovery.pending_restore.is_none());
    persist(&mut app);
    wait(&mut app, |app| !app.recovery.loading);
    assert!(app.recovery.reading.is_none());
}
#[test]
fn in_flight_read_from_canceled_close_is_discarded_before_a_fresh_restore_request() {
    let temp = tempfile::tempdir().unwrap();
    let mut app = app_at(&temp.path().join("recovery"));
    persist(&mut app);
    let id = record_id(&workspace(), "main.rs").unwrap();
    let (entered, release) = app.recovery.hold_next_read();
    app.recovery.request_restore(id.clone());
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    app.finish_recovery_close(&egui::Context::default());
    app.keep_editing_recovery(&egui::Context::default());
    assert!(app.recovery.reading.is_none());
    assert!(app.recovery.resuming());
    release.send(()).unwrap();
    wait(&mut app, |app| !app.recovery.resuming());
    assert!(app.recovery.pending_restore.is_none());
    let (entered, release) = app.recovery.hold_next_read();
    app.recovery.request_restore(id.clone());
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    // Repeated dismissal without an active close must not clear the new read.
    app.keep_editing_recovery(&egui::Context::default());
    assert_eq!(app.recovery.reading, Some(id));
    release.send(()).unwrap();
    wait(&mut app, |app| app.recovery.pending_restore.is_some());
}
#[test]
fn keep_editing_resumes_a_retry_listing_canceled_while_io_was_in_flight() {
    let temp = tempfile::tempdir().unwrap();
    let mut app = app_at(&temp.path().join("recovery"));
    let (entered, release) = app.recovery.hold_next_operation(false);
    app.recovery_tick(&egui::Context::default());
    app.recovery.flush();
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    app.recovery.retry(&egui::Context::default());
    assert!(!app.recovery.initialized);
    app.finish_recovery_close(&egui::Context::default());
    app.keep_editing_recovery(&egui::Context::default());
    release.send(()).unwrap();
    wait(&mut app, |app| {
        !app.recovery.resuming() && app.recovery.initialized
    });
    persist(&mut app);
    assert!(app.recovery.error.is_none());
}

#[test]
fn failed_open_keep_editing_never_reopens_without_a_fresh_retry() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("recovery");
    std::fs::write(&path, "blocker").unwrap();
    let mut app = app_at(&path);
    std::fs::remove_file(&path).unwrap();
    // Hold a read so Retry is certainly queued, then cancel that request with
    // the close fence before it can reopen the now-usable location.
    let (entered, release) = app.recovery.hold_next_read();
    app.recovery
        .request_restore(record_id(&workspace(), "main.rs").unwrap());
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    app.recovery.retry(&egui::Context::default());
    app.finish_recovery_close(&egui::Context::default());
    app.keep_editing_recovery(&egui::Context::default());
    release.send(()).unwrap();
    wait(&mut app, |app| {
        !app.recovery.resuming() && !app.recovery.loading
    });
    assert!(matches!(
        app.recovery.availability(),
        Availability::Unavailable(_)
    ));
    assert!(!app.recovery.initialized);
    assert!(
        !path.exists(),
        "resume must not replay the canceled Store::open"
    );
    assert!(app.recovery.pending_restore.is_none());
    app.recovery.retry(&egui::Context::default());
    wait(&mut app, |app| app.recovery.initialized);
    persist(&mut app);
    assert!(path.is_dir());
}
