//! Building every feature must never opt an ordinary host into Windows LSP.
use crate::{BackendMode, Workspace};
use cedar_protocol::{Operation, Payload};

fn start() -> Operation {
    Operation::LanguageStart {
        program: "must-not-be-launched".into(),
        args: vec![],
    }
}

#[test]
fn validation_requires_synthetic_root_and_separate_execution_trust() {
    let root = tempfile::tempdir().unwrap();
    assert!(Workspace::for_windows_language_validation(root.path()).is_err());
    let marker = root.path().join(".cedar-windows-language-validation");
    std::fs::create_dir(&marker).unwrap();
    assert_eq!(
        Workspace::for_windows_language_validation(root.path())
            .unwrap_err()
            .code,
        "invalid_validation_root"
    );
    std::fs::remove_dir(&marker).unwrap();
    std::fs::write(&marker, b"wrong marker").unwrap();
    assert_eq!(
        Workspace::for_windows_language_validation(root.path())
            .unwrap_err()
            .code,
        "invalid_validation_root"
    );
    std::fs::write(marker, b"cedar-windows-language-validation-v1\n").unwrap();
    let mut fixture = Workspace::for_windows_language_validation(root.path()).unwrap();
    assert_eq!(fixture.backend_mode, BackendMode::IsolatedAgent);
    assert_eq!(fixture.handle(start()).unwrap_err().code, "run_disabled");
    // Even the fixture must not manufacture production capability evidence.
    let mut ordinary =
        Workspace::with_backend_mode(root.path(), BackendMode::IsolatedAgent).unwrap();
    assert_eq!(
        serde_json::to_value(fixture.handle(Operation::Hello).unwrap()).unwrap(),
        serde_json::to_value(ordinary.handle(Operation::Hello).unwrap()).unwrap()
    );
    for mut normal in [Workspace::open(root.path()).unwrap(), ordinary] {
        assert!(!normal.windows_language_validation);
        normal.set_allow_run(true);
        if cfg!(windows) {
            assert_eq!(
                normal.handle(start()).unwrap_err().code,
                "unsupported_platform"
            );
            let Payload::Hello {
                agent: Some(info), ..
            } = normal.handle(Operation::Hello).unwrap()
            else {
                panic!("expected metadata");
            };
            for capability in cedar_protocol::LANGUAGE_SESSION_CAPABILITIES {
                assert!(!info.supports(capability));
            }
        }
    }
}
