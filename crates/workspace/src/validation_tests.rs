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

#[test]
fn java_profile_requires_both_exact_markers_and_has_no_ordinary_constructor_side_effects() {
    use crate::{
        WINDOWS_JAVA_EVIDENCE_FILE, WINDOWS_JAVA_VALIDATION_MARKER,
        WINDOWS_JAVA_VALIDATION_MARKER_CONTENTS,
    };
    let root = tempfile::tempdir().unwrap();
    let distribution = root.path().join("distribution");
    std::fs::create_dir_all(distribution.join("plugins")).unwrap();
    std::fs::create_dir(distribution.join("config_win")).unwrap();
    std::fs::write(
        distribution.join("plugins/org.eclipse.equinox.launcher_1.jar"),
        b"fixture",
    )
    .unwrap();
    let java_marker = root.path().join(WINDOWS_JAVA_VALIDATION_MARKER);
    std::fs::write(&java_marker, WINDOWS_JAVA_VALIDATION_MARKER_CONTENTS).unwrap();
    assert!(Workspace::for_windows_java_validation(root.path(), &distribution).is_err());
    std::fs::write(
        root.path().join(".cedar-windows-language-validation"),
        b"cedar-windows-language-validation-v1\n",
    )
    .unwrap();
    for contents in [
        b"wrong".as_slice(),
        b"cedar-windows-java-validation-v1".as_slice(),
        b"cedar-windows-java-validation-v1\nextra".as_slice(),
    ] {
        std::fs::write(&java_marker, contents).unwrap();
        assert!(Workspace::for_windows_java_validation(root.path(), &distribution).is_err());
        assert!(!root.path().join(WINDOWS_JAVA_EVIDENCE_FILE).exists());
    }
    std::fs::remove_file(&java_marker).unwrap();
    std::fs::create_dir(&java_marker).unwrap();
    assert!(Workspace::for_windows_java_validation(root.path(), &distribution).is_err());
    std::fs::remove_dir(&java_marker).unwrap();
    std::fs::write(&java_marker, WINDOWS_JAVA_VALIDATION_MARKER_CONTENTS).unwrap();
    for mut normal in [
        Workspace::open(root.path()).unwrap(),
        Workspace::with_backend_mode(root.path(), BackendMode::IsolatedAgent).unwrap(),
        Workspace::for_windows_language_validation(root.path()).unwrap(),
    ] {
        assert!(normal.windows_java_validation.is_none());
        assert_eq!(normal.handle(start()).unwrap_err().code, "run_disabled");
    }
    assert!(!root.path().join(WINDOWS_JAVA_EVIDENCE_FILE).exists());
    #[cfg(windows)]
    {
        let mut fixture =
            Workspace::for_windows_java_validation(root.path(), &distribution).unwrap();
        assert_eq!(fixture.handle(start()).unwrap_err().code, "run_disabled");
        let mut normal =
            Workspace::with_backend_mode(root.path(), BackendMode::IsolatedAgent).unwrap();
        assert_eq!(
            serde_json::to_value(fixture.handle(Operation::Hello).unwrap()).unwrap(),
            serde_json::to_value(normal.handle(Operation::Hello).unwrap()).unwrap()
        );
        fixture.set_allow_run(true);
        assert_eq!(
            fixture.handle(start()).unwrap_err().code,
            "invalid_java_validation"
        );
        drop(fixture);
        assert!(Workspace::for_windows_java_validation(root.path(), &distribution).is_err());
        assert_eq!(
            std::fs::read(root.path().join(WINDOWS_JAVA_EVIDENCE_FILE)).unwrap(),
            b""
        );
    }
    #[cfg(not(windows))]
    assert_eq!(
        Workspace::for_windows_java_validation(root.path(), &distribution)
            .unwrap_err()
            .code,
        "unsupported_platform"
    );
}

#[cfg(unix)]
#[test]
fn java_profile_rejects_marker_symlink_even_to_exact_contents() {
    use crate::{WINDOWS_JAVA_VALIDATION_MARKER, WINDOWS_JAVA_VALIDATION_MARKER_CONTENTS};
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join(".cedar-windows-language-validation"),
        b"cedar-windows-language-validation-v1\n",
    )
    .unwrap();
    std::fs::write(
        root.path().join("target"),
        WINDOWS_JAVA_VALIDATION_MARKER_CONTENTS,
    )
    .unwrap();
    std::os::unix::fs::symlink("target", root.path().join(WINDOWS_JAVA_VALIDATION_MARKER)).unwrap();
    assert_eq!(
        Workspace::for_windows_java_validation(root.path(), root.path())
            .unwrap_err()
            .code,
        "invalid_path"
    );
}
