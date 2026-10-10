//! Fixed local-agent route and ownership-warning adoption are frontend-only.
use super::*;

#[test]
fn local_form_keeps_root_identity_and_embedded_client_api_separate() {
    let form = ConnectForm {
        local_root: "/generated/local workspace".into(),
        allow_run: false,
        ..Default::default()
    };
    assert_eq!(
        form.key(),
        WorkspaceKey::Local {
            root: form.local_root.clone()
        }
    );
    assert_eq!(
        recovery_ui::identity(&form, &form.local_root),
        cedar_recovery::WorkspaceIdentity::Local {
            root: form.local_root.clone()
        }
    );
    #[cfg(target_os = "linux")]
    assert!(
        matches!(form.spec().unwrap(), ConnectionSpec::BundledLinux { root, allow_run: false } if root == std::path::Path::new(&form.local_root))
    );
    #[cfg(not(target_os = "linux"))]
    assert!(
        matches!(form.spec().unwrap(), ConnectionSpec::Local { root, allow_run: false } if root == std::path::Path::new(&form.local_root))
    );
}

#[test]
fn retired_owned_attempt_warning_is_bounded_and_cannot_adopt_session_state() {
    let mut app = CedarApp::empty();
    app.generation = 12;
    app.state = ConnectionState::Ready;
    app.root = "/new/session".into();
    app.notice = "new connection ready".into();
    app.error = Some("current error".into());
    let mut doc = Document::new(1, "draft.txt".into(), "baseline".into(), "revision".into());
    doc.text = "retained draft 🐻".into();
    app.documents.push(doc);
    app.active_document = Some(1);
    for generation in [4, 4, 2, 7, 5] {
        app.apply_worker_event(WorkerEvent::AttemptCleanupUnverified { generation });
        assert!(app.unverified_local_close);
        assert!(app.state == ConnectionState::Ready);
        assert_eq!(app.generation, 12);
        assert_eq!(app.root, "/new/session");
        assert_eq!(app.notice, "new connection ready");
        assert_eq!(app.error.as_deref(), Some("current error"));
        assert_eq!(app.documents[0].text, "retained draft 🐻");
        assert_eq!(app.documents[0].saved_text, "baseline");
        assert_eq!(app.active_document, Some(1));
    }
    assert_eq!(app.unverified_attempt_generation, Some(7));
    app.apply_worker_event(WorkerEvent::AttemptCleanupUnverified { generation: 13 });
    assert_eq!(app.unverified_attempt_generation, Some(7));
    app.apply_worker_event(WorkerEvent::Closed {
        generation: 7,
        result: Ok(()),
    });
    assert!(app.unverified_local_close);
    assert!(app.state == ConnectionState::Ready);
}

#[test]
fn peer_error_text_cannot_create_owned_attempt_warning() {
    let mut app = CedarApp::empty();
    app.generation = 3;
    app.state = ConnectionState::Connecting;
    app.connecting_form = Some(ConnectForm {
        local_root: "/generated/local".into(),
        ..Default::default()
    });
    app.apply_worker_event(WorkerEvent::Response(Event {
        generation: 3,
        id: 0,
        connected: false,
        result: Err("bundled_agent_cleanup_unverified: arbitrary peer stderr".into()),
    }));
    assert!(app.state == ConnectionState::Disconnected);
    assert!(app
        .error
        .as_deref()
        .unwrap()
        .contains("arbitrary peer stderr"));
    assert!(!app.unverified_local_close);
    assert_eq!(app.unverified_attempt_generation, None);
}
