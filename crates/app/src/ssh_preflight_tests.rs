//! Pure SSH form admission and retained-workspace checks. This is a child of
//! recovery_ui so the same checks cover its private restore entrypoint.
//! Recording workers and in-memory editor/recovery data never launch SSH,
//! consult SSH configuration, or read or write workspace files.
use super::restore_form;
use crate::*;
use cedar_recovery::{record_id, Draft, DraftMetadata, WorkspaceIdentity};

const ROOT: &str = "/canonical/retained-workspace";
const BASE: &str = "saved text 草稿 🐻\n";
const DRAFT: &str = "first edit 草稿 🐻\n";
const NEWER: &str = "newer retained edit 草稿 🐻\n";
const PROFILE: &str = r#"{"version":1,"profiles":[{"name":"Check","program":"cargo","args":["check","--offline"],"timeout_secs":30}]}"#;

fn ssh_form() -> ConnectForm {
    ConnectForm {
        ssh: true,
        local_root: "/unused-local-root".into(),
        host: "user@next.example.invalid".into(),
        port: "22".into(),
        remote_root: "/next-workspace".into(),
        agent: "/opt/cedar-agent".into(),
        allow_run: false,
    }
}

fn invalid_forms() -> Vec<(&'static str, ConnectForm)> {
    let mut forms = Vec::new();
    for (field, value) in [
        ("host", ""),
        ("host", "   "),
        ("host", "-oProxyCommand=anything"),
        ("host", "user@host name"),
        ("host", "host\nother"),
        ("host", "host\0other"),
        ("host", "host;command"),
        ("host", "$(command)"),
        ("root", ""),
        ("root", "relative/workspace"),
        ("root", "~/workspace"),
        ("root", "C:\\workspace"),
        ("root", "/work\nspace"),
        ("root", "/work\0space"),
        ("root", "/work\u{7f}space"),
        ("agent", ""),
        ("agent", "-agent"),
        ("agent", "cedar\nagent"),
        ("agent", "cedar\tagent"),
        ("agent", "cedar\0agent"),
        ("agent", "cedar\u{85}agent"),
        ("port", ""),
        ("port", "0"),
        ("port", "65536"),
        ("port", "-22"),
        ("port", "22x"),
        ("port", "2 2"),
        ("port", " 22 "),
    ] {
        let mut form = ssh_form();
        match field {
            "host" => form.host = value.into(),
            "root" => form.remote_root = value.into(),
            "agent" => form.agent = value.into(),
            "port" => form.port = value.into(),
            _ => unreachable!(),
        }
        forms.push((field, form));
    }
    forms
}

#[test]
fn ssh_form_rejects_invalid_fields_before_any_worker_can_start() {
    for (field, form) in invalid_forms() {
        assert!(form.spec().is_err(), "invalid {field} was accepted");
        // The client remains the authority for nonempty endpoint validation.
        // Compare its explanation for values that pass the form's blank/port
        // checks instead of duplicating the host/path rules in these tests.
        if let Ok(port) = form.port.parse::<u16>() {
            if port != 0
                && !form.host.trim().is_empty()
                && !form.remote_root.trim().is_empty()
                && !form.agent.trim().is_empty()
            {
                assert_eq!(
                    form.spec().unwrap_err(),
                    cedar_client::ssh_arguments(
                        form.host.trim(),
                        port,
                        form.remote_root.trim(),
                        form.agent.trim(),
                        form.allow_run,
                    )
                    .unwrap_err(),
                    "invalid {field} must use the client validator"
                );
            }
        }
    }
}

#[test]
fn ssh_form_preserves_literal_unicode_apostrophes_and_shell_metacharacters() {
    for host in [
        "workspace-alias",
        "user@example.invalid",
        "user@[2001:db8::1]",
    ] {
        for port in [1, 22, 65535] {
            for allow_run in [false, true] {
                let form = ConnectForm {
                    host: format!("  {host}  "),
                    port: port.to_string(),
                    remote_root: "  /work/草稿's ; $(literal) `text` & [one]  ".into(),
                    agent: "  /opt/工具's agent; $(literal) &  ".into(),
                    allow_run,
                    ..ssh_form()
                };
                let ConnectionSpec::Ssh {
                    host: actual_host,
                    port: actual_port,
                    root,
                    agent_path,
                    allow_run: actual_trust,
                } = form.spec().expect("literal POSIX paths must be accepted")
                else {
                    panic!("SSH form must produce an SSH specification");
                };
                assert_eq!(actual_host, host);
                assert_eq!(actual_port, port);
                assert_eq!(root, form.remote_root.trim());
                assert_eq!(agent_path, form.agent.trim());
                assert_eq!(actual_trust, allow_run);
                let args = cedar_client::ssh_arguments(
                    &actual_host,
                    actual_port,
                    &root,
                    &agent_path,
                    actual_trust,
                )
                .unwrap();
                let suffix = if allow_run { " --allow-run" } else { "" };
                assert_eq!(
                    args.last().unwrap(),
                    &format!(
                        "exec '/opt/工具'\\''s agent; $(literal) &' --root '/work/草稿'\\''s ; $(literal) `text` & [one]'{suffix}"
                    )
                );
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Dirty {
    None,
    Document,
    Profile,
}

fn live_app(dirty: Dirty) -> (CedarApp, Receiver<Command>) {
    let mut app = CedarApp::empty();
    let form = ConnectForm {
        host: "user@retained.example.invalid".into(),
        remote_root: "/retained-workspace".into(),
        allow_run: true,
        ..ssh_form()
    };
    let (worker, commands) = Worker::recording();
    app.worker = Some(worker);
    app.generation = 7;
    app.workspace_key = Some(form.key());
    app.active_form = Some(form.clone());
    app.form = form;
    app.root = ROOT.into();
    app.state = ConnectionState::Ready;
    app.agent_info = Some(agent_support::full_test_agent());
    app.open_form = true;
    app.notice = "Retained workspace is ready".into();
    app.documents.push(Document::new(
        1,
        "draft.txt".into(),
        BASE.into(),
        "r0".into(),
    ));
    editor_state::commit(&app.editor_ctx, &mut app.documents[0], DRAFT.into(), 6);
    editor_state::commit(&app.editor_ctx, &mut app.documents[0], NEWER.into(), 9);
    if !matches!(dirty, Dirty::Document) {
        app.documents[0].acknowledge_save(NEWER.into(), "r2".into());
    }
    let mut selection =
        egui::text::CCursorRange::two(egui::text::CCursor::new(3), egui::text::CCursor::new(12));
    selection.primary.prefer_next_row = true;
    selection.secondary.prefer_next_row = false;
    let id = egui::Id::new(("editor", 1u64));
    let mut state = egui::TextEdit::load_state(&app.editor_ctx, id).unwrap();
    state.cursor.set_char_range(Some(selection));
    state.store(&app.editor_ctx, id);
    app.documents.push(Document::new(
        2,
        profile_ui::PATH.into(),
        PROFILE.into(),
        "profiles-r0".into(),
    ));
    app.active_document = Some(1);
    app.next_document = 3;
    app.profiles.connected(app.recovery_workspace().unwrap());
    app.load_profiles();
    app.select_profile(Some(0));
    if matches!(dirty, Dirty::Profile) {
        app.profiles.draft.args.push("--all-targets".into());
        app.profiles.changed();
    }
    assert_eq!(app.dirty(), !matches!(dirty, Dirty::None));
    assert!(app.profiles.owns_document(2));
    assert!(app.profile_run_problem().is_none());

    let draft = Draft {
        workspace: app.recovery_workspace().unwrap(),
        path: "recovered.txt".into(),
        text: "private retained recovery 草稿".into(),
        base_text: "recovery baseline".into(),
        base_revision: Some("recovery-r0".into()),
        modified_ms: 123,
    };
    let id = record_id(&draft.workspace, &draft.path).unwrap();
    app.recovery.enabled = true;
    app.recovery.initialized = true;
    app.recovery.visible = true;
    app.recovery.drafts.push(DraftMetadata {
        id: id.clone(),
        workspace: draft.workspace.clone(),
        path: draft.path.clone(),
        base_revision: draft.base_revision.clone(),
        modified_ms: draft.modified_ms,
        text_bytes: draft.text.len(),
        base_text_bytes: draft.base_text.len(),
    });
    app.recovery.pending_restore = Some(draft);
    app.recovery.remove_confirmation = Some(id);
    (app, commands)
}

#[derive(Debug, PartialEq, Eq)]
struct FormSnapshot(bool, String, String, String, String, String, bool);

fn form_snapshot(form: &ConnectForm) -> FormSnapshot {
    FormSnapshot(
        form.ssh,
        form.local_root.clone(),
        form.host.clone(),
        form.port.clone(),
        form.remote_root.clone(),
        form.agent.clone(),
        form.allow_run,
    )
}

#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    generation: u64,
    root: String,
    workspace_key: Option<WorkspaceKey>,
    active_form: FormSnapshot,
    edited_form: FormSnapshot,
    documents: String,
    active_document: Option<u64>,
    next_document: u64,
    next_request: u64,
    navigation_epoch: u64,
    agent: String,
    notice: String,
    profile: task_profiles::TaskProfile,
    profile_epoch: u64,
    profile_message: Option<String>,
    profile_dirty: bool,
    recovery_workspace: Option<WorkspaceIdentity>,
    recovery_drafts: Vec<DraftMetadata>,
    recovery_pending: Option<Draft>,
    recovery_remove: Option<cedar_recovery::RecordId>,
}

fn snapshot(app: &CedarApp) -> Snapshot {
    Snapshot {
        generation: app.generation,
        root: app.root.clone(),
        workspace_key: app.workspace_key.clone(),
        active_form: form_snapshot(app.active_form.as_ref().unwrap()),
        edited_form: form_snapshot(&app.form),
        // Document's Debug representation includes its entire current state,
        // including revision, cursor hints, dirty/save and Undo initialization.
        documents: format!("{:?}", app.documents),
        active_document: app.active_document,
        next_document: app.next_document,
        next_request: app.next_request,
        navigation_epoch: app.navigation_epoch,
        agent: format!("{:?}", app.agent_info),
        notice: app.notice.clone(),
        profile: app.profiles.draft.clone(),
        profile_epoch: app.profiles.epoch,
        profile_message: app.profiles.message.clone(),
        profile_dirty: app.profiles.dirty(),
        recovery_workspace: app.recovery_workspace(),
        recovery_drafts: app.recovery.drafts.clone(),
        recovery_pending: app.recovery.pending_restore.clone(),
        recovery_remove: app.recovery.remove_confirmation.clone(),
    }
}

fn selection(app: &CedarApp) -> egui::text::CCursorRange {
    egui::TextEdit::load_state(&app.editor_ctx, egui::Id::new(("editor", 1u64)))
        .unwrap()
        .cursor
        .char_range()
        .unwrap()
}

fn history_shortcut(app: &mut CedarApp, redo: bool) {
    let ctx = app.editor_ctx.clone();
    let modifiers = if redo {
        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT
    } else {
        egui::Modifiers::COMMAND
    };
    let _ = ctx.run(
        egui::RawInput {
            events: vec![egui::Event::Key {
                key: egui::Key::Z,
                physical_key: Some(egui::Key::Z),
                pressed: true,
                repeat: false,
                modifiers,
            }],
            ..Default::default()
        },
        |ctx| {
            ctx.memory_mut(|memory| memory.request_focus(egui::Id::new(("editor", 1u64))));
            editor_state::history_shortcut(ctx, &mut app.documents[0]);
        },
    );
}

fn assert_retained(
    app: &mut CedarApp,
    commands: &Receiver<Command>,
    before: Snapshot,
    cursor: egui::text::CCursorRange,
) {
    assert!(app.ready());
    assert!(app.worker.is_some());
    assert!(app.connecting_form.is_none());
    assert!(app.pending.is_empty());
    assert!(app.open_form);
    assert!(app.active_form.as_ref().unwrap().allow_run);
    assert!(app.profiles.owns_document(2));
    assert!(app.profile_run_problem().is_none());
    assert!(app.recovery.enabled && app.recovery.initialized && app.recovery.visible);
    assert!(!app.recovery.loading);
    assert!(app.recovery.error.is_none());
    assert!(app.recovery.issues.is_empty());
    assert!(app.recovery.reading.is_none());
    assert!(app.recovery.restoring_generation.is_none());
    assert!(app.recovery.closing.is_none());
    assert!(!app.recovery.has_actor());
    assert_eq!(snapshot(app), before);
    assert!(location_history::same_selection(cursor, selection(app)));
    // Empty (rather than Disconnected) proves the original recording worker
    // still owns its sender and accepted no commands.
    assert!(matches!(
        commands.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    let saved = app.documents[0].saved_text.clone();
    let revision = app.documents[0].revision.clone();
    history_shortcut(app, false);
    assert_eq!(app.documents[0].text, DRAFT);
    history_shortcut(app, false);
    assert_eq!(app.documents[0].text, BASE);
    history_shortcut(app, true);
    assert_eq!(app.documents[0].text, DRAFT);
    history_shortcut(app, true);
    assert_eq!(app.documents[0].text, NEWER);
    assert_eq!(app.documents[0].saved_text, saved);
    assert_eq!(app.documents[0].revision, revision);
    // Exercise the retained baseline and parsed profile file as well as their
    // visible draft/source state; neither operation may send a command.
    let original = task_profiles::parse_task_file(PROFILE)
        .unwrap()
        .profiles
        .remove(0);
    app.discard_profile_form();
    assert_eq!(app.profiles.draft, original);
    app.select_profile(None);
    app.select_profile(Some(0));
    assert_eq!(app.profiles.draft, original);
    assert!(app.profile_run_problem().is_none());
    assert!(matches!(
        commands.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
}

#[test]
fn invalid_ssh_connect_preserves_live_workspace_and_native_editor_history() {
    for dirty in [Dirty::None, Dirty::Document, Dirty::Profile] {
        for (field, form) in invalid_forms() {
            // Fail closed before connect if form validation ever regresses:
            // even a failing test must not start a real SSH process.
            let expected = form.spec().expect_err("invalid form must fail preflight");
            let (mut app, commands) = live_app(dirty);
            app.form = form.clone();
            let before = snapshot(&app);
            let cursor = selection(&app);
            app.connect(&app.editor_ctx.clone(), form);
            if matches!(dirty, Dirty::None) {
                assert_eq!(app.error.as_deref(), Some(expected.as_str()), "{field}");
            } else {
                assert!(app
                    .error
                    .as_deref()
                    .unwrap()
                    .contains("switching workspaces"));
            }
            assert_retained(&mut app, &commands, before, cursor);
        }
    }
}

#[test]
fn invalid_ssh_recovery_form_preserves_live_workspace_and_saved_copy() {
    for dirty in [Dirty::None, Dirty::Document, Dirty::Profile] {
        for (field, form) in invalid_forms() {
            // Recovery metadata stores a u16, so malformed port text and
            // overflow cannot enter this route; zero remains representable.
            let Ok(port) = form.port.parse::<u16>() else {
                continue;
            };
            let workspace = WorkspaceIdentity::Ssh {
                host: form.host,
                port,
                root: form.remote_root,
                agent_path: form.agent,
            };
            let restored_form = restore_form(&workspace);
            assert!(!restored_form.allow_run);
            let expected = restored_form
                .spec()
                .expect_err("invalid recovery endpoint must fail before starting SSH");
            let (mut app, commands) = live_app(dirty);
            app.recovery.pending_restore.as_mut().unwrap().workspace = workspace;
            let before = snapshot(&app);
            let cursor = selection(&app);
            app.begin_restore(&app.editor_ctx.clone());
            if matches!(dirty, Dirty::None) {
                assert_eq!(app.error.as_deref(), Some(expected.as_str()), "{field}");
            } else {
                assert!(app
                    .error
                    .as_deref()
                    .unwrap()
                    .contains("switching workspaces"));
            }
            assert_retained(&mut app, &commands, before, cursor);
        }
    }
}
