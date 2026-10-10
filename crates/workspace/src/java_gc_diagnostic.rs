//! Fixed, nonshipping GC evidence for one already-validated production launch.
//! Markers are explicit fixture opt-ins, not a sandbox against concurrent writers.
use super::java_launch::{regular_path, JavaLaunch};
use crate::{error, io_error};
use cedar_protocol::RemoteError;
use std::{
    ffi::OsStr,
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
};

pub const WINDOWS_JAVA_GC_DIAGNOSTIC_MARKER: &str = ".cedar-windows-java-gc-diagnostic";
pub const WINDOWS_JAVA_GC_DIAGNOSTIC_MARKER_CONTENTS: &[u8] =
    b"cedar-windows-java-gc-diagnostic-v1\nsynthetic-data-only\n";
pub const WINDOWS_JAVA_GC_DIAGNOSTIC_DISTRIBUTION_MARKER: &str =
    ".cedar-windows-java-gc-diagnostic-distribution";
pub const WINDOWS_JAVA_GC_DIAGNOSTIC_DISTRIBUTION_MARKER_CONTENTS: &[u8] =
    b"cedar-windows-java-gc-diagnostic-distribution-v1\nsynthetic-data-only\n";
pub const WINDOWS_JAVA_GC_DIAGNOSTIC_OPTION: &str =
    "-Xlog:gc=info:file=cedar-gc-%p.log:uptimemillis,level,tags:filecount=2,filesize=64K";
const MAX_DISTRIBUTION_ENTRIES: usize = 4096;

fn invalid(message: &'static str) -> RemoteError {
    error("invalid_java_gc_diagnostic", message)
}

fn require_exact_marker(directory: &Path, name: &str, expected: &[u8]) -> Result<(), RemoteError> {
    let path = directory.join(name);
    regular_path(&path, false)?;
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(io_error)?
        .take(expected.len() as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes != expected {
        return Err(invalid("Expected the exact synthetic GC diagnostic marker"));
    }
    Ok(())
}

fn require_fresh_distribution(distribution: &Path) -> Result<(), RemoteError> {
    require_exact_marker(
        distribution,
        WINDOWS_JAVA_GC_DIAGNOSTIC_DISTRIBUTION_MARKER,
        WINDOWS_JAVA_GC_DIAGNOSTIC_DISTRIBUTION_MARKER_CONTENTS,
    )?;
    for (index, entry) in fs::read_dir(distribution).map_err(io_error)?.enumerate() {
        if index >= MAX_DISTRIBUTION_ENTRIES {
            return Err(invalid("GC diagnostic distribution scan exceeds its bound"));
        }
        let entry = entry.map_err(io_error)?;
        // Windows names are case-insensitive. Reject all matching entries,
        // including directories, symlinks, rotations and non-UTF-8 suffixes.
        if entry
            .file_name()
            .to_string_lossy()
            .to_ascii_lowercase()
            .starts_with("cedar-gc")
        {
            return Err(invalid("GC diagnostic requires no previous cedar-gc files"));
        }
    }
    Ok(())
}

#[derive(Debug)]
pub(super) struct JavaGcDiagnosticProfile {
    root: PathBuf,
    attempted: bool,
}

impl JavaGcDiagnosticProfile {
    pub(super) fn new(root: &Path) -> Result<Self, RemoteError> {
        require_exact_marker(
            root,
            WINDOWS_JAVA_GC_DIAGNOSTIC_MARKER,
            WINDOWS_JAVA_GC_DIAGNOSTIC_MARKER_CONTENTS,
        )?;
        Ok(Self {
            root: root.to_path_buf(),
            attempted: false,
        })
    }

    /// Called only after execution trust, platform, active-session and unchanged
    /// production validation succeed. A failed spawn still consumes the attempt.
    pub(super) fn decorate(&mut self, launch: &mut JavaLaunch) -> Result<(), RemoteError> {
        if self.attempted {
            return Err(invalid(
                "The single GC diagnostic launch attempt was already used",
            ));
        }
        require_exact_marker(
            &self.root,
            WINDOWS_JAVA_GC_DIAGNOSTIC_MARKER,
            WINDOWS_JAVA_GC_DIAGNOSTIC_MARKER_CONTENTS,
        )?;
        let distribution =
            launch.config.working_directory.as_deref().ok_or_else(|| {
                invalid("GC diagnostic requires the production working directory")
            })?;
        require_fresh_distribution(distribution)?;
        let mut jars = launch
            .config
            .args
            .iter()
            .enumerate()
            .filter_map(|(index, arg)| (arg == OsStr::new("-jar")).then_some(index));
        let jar = jars
            .next()
            .ok_or_else(|| invalid("GC diagnostic requires the production JAR argument"))?;
        if jars.next().is_some()
            || launch
                .config
                .args
                .iter()
                .any(|arg| arg.to_string_lossy().starts_with("-Xlog"))
        {
            return Err(invalid(
                "GC diagnostic requires an undecorated production launch",
            ));
        }
        // This is the sole mutation: keep heap, collector, executable, CWD,
        // encoded Unicode URLs, request bounds and initialization unchanged.
        launch
            .config
            .args
            .insert(jar, WINDOWS_JAVA_GC_DIAGNOSTIC_OPTION.into());
        self.attempted = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{java_launch, BackendMode, Workspace};
    use cedar_language::ProcessConfig;
    use cedar_protocol::{Operation, Payload};

    fn marked_root() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join(WINDOWS_JAVA_GC_DIAGNOSTIC_MARKER),
            WINDOWS_JAVA_GC_DIAGNOSTIC_MARKER_CONTENTS,
        )
        .unwrap();
        root
    }

    fn marked_distribution() -> tempfile::TempDir {
        let distribution = tempfile::tempdir().unwrap();
        fs::write(
            distribution
                .path()
                .join(WINDOWS_JAVA_GC_DIAGNOSTIC_DISTRIBUTION_MARKER),
            WINDOWS_JAVA_GC_DIAGNOSTIC_DISTRIBUTION_MARKER_CONTENTS,
        )
        .unwrap();
        distribution
    }

    // Pure configuration witness: no test in this module executes Java.
    fn launch(distribution: &Path) -> JavaLaunch {
        let mut config = ProcessConfig::new(r"C:\fixed jdk\bin\java.exe");
        config.working_directory = Some(distribution.to_path_buf());
        config.args = [
            "-Dlog.level=WARNING",
            "-Xmx512m",
            "--add-modules=ALL-SYSTEM",
            "-jar",
            "plugins/org.eclipse.equinox.launcher_1.jar",
            "-configuration",
            "file:///C:/distribution%20%E9%9B%AA/config_win/",
            "-data",
            "file:///C:/data%20%E9%9B%AA/",
        ]
        .into_iter()
        .map(Into::into)
        .collect();
        JavaLaunch {
            config,
            options: java_launch::client_options(),
            initialization_options: java_launch::initialization_options(),
            maven: None,
        }
    }

    fn invalid_start() -> Operation {
        Operation::LanguageStartJava {
            java_executable: "must-not-be-inspected\0".into(),
            distribution: "must-not-be-inspected\0".into(),
            data_directory: "must-not-be-inspected\0".into(),
        }
    }

    #[test]
    fn normal_constructors_ignore_all_fixture_markers_even_with_all_features() {
        let root = marked_root();
        fs::write(
            root.path().join(".cedar-windows-language-validation"),
            b"cedar-windows-language-validation-v1\n",
        )
        .unwrap();
        fs::write(
            root.path().join(".cedar-windows-java-validation"),
            b"cedar-windows-java-validation-v1\n",
        )
        .unwrap();
        fs::write(
            root.path()
                .join(WINDOWS_JAVA_GC_DIAGNOSTIC_DISTRIBUTION_MARKER),
            WINDOWS_JAVA_GC_DIAGNOSTIC_DISTRIBUTION_MARKER_CONTENTS,
        )
        .unwrap();
        for workspace in [
            Workspace::open(root.path()).unwrap(),
            Workspace::with_backend_mode(root.path(), BackendMode::InProcess).unwrap(),
            Workspace::with_backend_mode(root.path(), BackendMode::IsolatedAgent).unwrap(),
        ] {
            assert!(workspace.windows_java_gc_diagnostic.is_none());
            assert!(!workspace.allow_run);
            #[cfg(feature = "windows-language-validation")]
            {
                assert!(!workspace.windows_language_validation);
                assert!(workspace.windows_java_validation.is_none());
            }
        }
    }

    #[test]
    fn diagnostic_constructor_preserves_trust_capabilities_and_validation_isolation() {
        let root = marked_root();
        let mut diagnostic = Workspace::for_windows_java_gc_diagnostic(root.path()).unwrap();
        let mut ordinary =
            Workspace::with_backend_mode(root.path(), BackendMode::IsolatedAgent).unwrap();
        assert_eq!(diagnostic.backend_mode, BackendMode::IsolatedAgent);
        assert!(!diagnostic.allow_run);
        assert!(diagnostic.language.is_none());
        #[cfg(feature = "windows-language-validation")]
        {
            assert!(!diagnostic.windows_language_validation);
            assert!(diagnostic.windows_java_validation.is_none());
        }
        let Payload::Hello { agent: a, .. } = diagnostic.handle(Operation::Hello).unwrap() else {
            panic!("Hello must remain available without trust")
        };
        let Payload::Hello { agent: mut b, .. } = ordinary.handle(Operation::Hello).unwrap() else {
            panic!("ordinary Hello")
        };
        for capability in cedar_protocol::JAVA_MAVEN_CAPABILITIES {
            assert!(!a.as_ref().unwrap().supports(capability));
        }
        assert!(!a
            .as_ref()
            .unwrap()
            .supports(cedar_protocol::JAVA_MAVEN_DEPENDENCIES_CAPABILITY));
        assert!(a.as_ref().unwrap().capability_groups.is_empty());
        b.as_mut().unwrap().capabilities.retain(|name| {
            !cedar_protocol::JAVA_MAVEN_CAPABILITIES.contains(&name.as_str())
                && name != cedar_protocol::JAVA_MAVEN_DEPENDENCIES_CAPABILITY
        });
        b.as_mut().unwrap().capability_groups.retain(|name| {
            name != cedar_protocol::JAVA_MAVEN_LEAF_GROUP
                && name != cedar_protocol::JAVA_MAVEN_DEPENDENCIES_GROUP
        });
        assert_eq!(a, b);
        let maven_start = || Operation::LanguageStartJavaMavenBegin {
            java_executable: "must-not-be-inspected\0".into(),
            distribution: "must-not-be-inspected\0".into(),
            data_directory: "must-not-be-inspected\0".into(),
            local_repository: "must-not-be-inspected\0".into(),
        };
        assert_eq!(
            diagnostic.handle(maven_start()).unwrap_err().code,
            "run_disabled"
        );
        assert_eq!(
            diagnostic.handle(invalid_start()).unwrap_err().code,
            "run_disabled"
        );
        assert!(
            !diagnostic
                .windows_java_gc_diagnostic
                .as_ref()
                .unwrap()
                .attempted
        );
        diagnostic.set_allow_run(true);
        assert_eq!(
            diagnostic.handle(maven_start()).unwrap_err().code,
            "unsupported_platform"
        );
        assert_eq!(
            diagnostic
                .handle(Operation::LanguageMavenDependencies {
                    startup_id: 1,
                    pom_sha256: "unused".into()
                })
                .unwrap_err()
                .code,
            "unsupported_platform"
        );
        assert_eq!(
            diagnostic
                .handle(Operation::LanguageMavenModel)
                .unwrap_err()
                .code,
            "unsupported_platform"
        );
        assert_eq!(
            diagnostic.handle(invalid_start()).unwrap_err().code,
            if cfg!(windows) {
                "invalid_java_launch"
            } else {
                "unsupported_platform"
            }
        );
        assert!(
            !diagnostic
                .windows_java_gc_diagnostic
                .as_ref()
                .unwrap()
                .attempted
        );
        assert!(diagnostic.language.is_none());
        assert!(diagnostic.tasks.is_none());
    }

    #[cfg(windows)]
    #[test]
    fn diagnostic_does_not_enable_generic_windows_language_start() {
        let root = marked_root();
        let mut workspace = Workspace::for_windows_java_gc_diagnostic(root.path()).unwrap();
        workspace.set_allow_run(true);
        assert_eq!(
            workspace
                .handle(Operation::LanguageStart {
                    program: "must-not-run".into(),
                    args: vec![],
                })
                .unwrap_err()
                .code,
            "unsupported_platform"
        );
        assert!(
            !workspace
                .windows_java_gc_diagnostic
                .as_ref()
                .unwrap()
                .attempted
        );
    }

    #[test]
    fn constructor_requires_exact_regular_root_marker() {
        let root = tempfile::tempdir().unwrap();
        let marker = root.path().join(WINDOWS_JAVA_GC_DIAGNOSTIC_MARKER);
        assert!(Workspace::for_windows_java_gc_diagnostic(root.path()).is_err());
        for bytes in [
            b"".as_slice(),
            b"cedar-windows-java-validation-v1\n",
            WINDOWS_JAVA_GC_DIAGNOSTIC_MARKER_CONTENTS
                .strip_suffix(b"\n")
                .unwrap(),
        ] {
            fs::write(&marker, bytes).unwrap();
            assert!(Workspace::for_windows_java_gc_diagnostic(root.path()).is_err());
        }
        let mut oversized = WINDOWS_JAVA_GC_DIAGNOSTIC_MARKER_CONTENTS.to_vec();
        oversized.extend_from_slice(b"extra");
        fs::write(&marker, oversized).unwrap();
        assert!(Workspace::for_windows_java_gc_diagnostic(root.path()).is_err());
        fs::remove_file(&marker).unwrap();
        fs::create_dir(&marker).unwrap();
        assert!(Workspace::for_windows_java_gc_diagnostic(root.path()).is_err());
    }

    #[test]
    fn decorator_adds_one_exact_option_before_jar_preserving_everything_else() {
        let root = marked_root();
        let distribution = marked_distribution();
        let mut profile = JavaGcDiagnosticProfile::new(root.path()).unwrap();
        let mut decorated = launch(distribution.path());
        let original_args = decorated.config.args.clone();
        let original_program = decorated.config.program.clone();
        let original_cwd = decorated.config.working_directory.clone();
        let original_stderr = decorated.config.inherit_stderr;
        let original_options = format!("{:?}", decorated.options);
        let original_initialization = decorated.initialization_options.clone();
        profile.decorate(&mut decorated).unwrap();
        assert_eq!(
            WINDOWS_JAVA_GC_DIAGNOSTIC_OPTION,
            "-Xlog:gc=info:file=cedar-gc-%p.log:uptimemillis,level,tags:filecount=2,filesize=64K"
        );
        assert_eq!(
            decorated
                .config
                .args
                .iter()
                .filter(|arg| arg.to_string_lossy().starts_with("-Xlog"))
                .count(),
            1
        );
        let added = decorated
            .config
            .args
            .iter()
            .position(|arg| arg == OsStr::new(WINDOWS_JAVA_GC_DIAGNOSTIC_OPTION))
            .unwrap();
        assert_eq!(decorated.config.args[added + 1], OsStr::new("-jar"));
        decorated.config.args.remove(added);
        assert_eq!(decorated.config.args, original_args);
        assert!(decorated.config.args.contains(&"-Xmx512m".into()));
        assert_eq!(decorated.config.program, original_program);
        assert_eq!(decorated.config.working_directory, original_cwd);
        assert_eq!(decorated.config.inherit_stderr, original_stderr);
        assert_eq!(format!("{:?}", decorated.options), original_options);
        assert_eq!(decorated.initialization_options, original_initialization);
        assert!(decorated
            .initialization_options
            .get("extendedClientCapabilities")
            .is_none());
        assert!(profile.attempted);
        let second = profile.decorate(&mut decorated).unwrap_err();
        assert_eq!(second.code, "invalid_java_gc_diagnostic");
        assert_eq!(decorated.config.args, original_args);
    }

    #[test]
    fn invalid_distribution_marker_or_prior_log_does_not_consume_attempt() {
        let root = marked_root();
        let distribution = marked_distribution();
        let mut profile = JavaGcDiagnosticProfile::new(root.path()).unwrap();
        let marker = distribution
            .path()
            .join(WINDOWS_JAVA_GC_DIAGNOSTIC_DISTRIBUTION_MARKER);
        fs::write(&marker, b"wrong\n").unwrap();
        let mut value = launch(distribution.path());
        let original = value.config.args.clone();
        assert!(profile.decorate(&mut value).is_err());
        assert!(!profile.attempted);
        fs::write(
            &marker,
            WINDOWS_JAVA_GC_DIAGNOSTIC_DISTRIBUTION_MARKER_CONTENTS,
        )
        .unwrap();
        for name in [
            "cedar-gc-123.log",
            "cedar-gc-123.log.0",
            "CEDAR-GC-456.LOG",
            "cedar-gc.log",
        ] {
            let prior = distribution.path().join(name);
            fs::write(&prior, b"prior").unwrap();
            assert!(profile.decorate(&mut value).is_err(), "{name}");
            assert!(!profile.attempted);
            assert_eq!(value.config.args, original);
            fs::remove_file(prior).unwrap();
        }
        let prior = distribution.path().join("cedar-gc-directory");
        fs::create_dir(&prior).unwrap();
        assert!(profile.decorate(&mut value).is_err());
        assert!(!profile.attempted);
        fs::remove_dir(prior).unwrap();
        profile.decorate(&mut value).unwrap();
    }

    #[test]
    fn root_marker_is_rechecked_before_consuming_attempt() {
        let root = marked_root();
        let distribution = marked_distribution();
        let mut profile = JavaGcDiagnosticProfile::new(root.path()).unwrap();
        fs::write(
            root.path().join(WINDOWS_JAVA_GC_DIAGNOSTIC_MARKER),
            b"changed",
        )
        .unwrap();
        assert!(profile.decorate(&mut launch(distribution.path())).is_err());
        assert!(!profile.attempted);
    }

    #[test]
    fn distribution_scan_is_bounded() {
        let distribution = marked_distribution();
        // The marker plus these entries exceed the fixed scan budget.
        for index in 0..MAX_DISTRIBUTION_ENTRIES {
            fs::write(distribution.path().join(format!("entry-{index}")), []).unwrap();
        }
        assert_eq!(
            require_fresh_distribution(distribution.path())
                .unwrap_err()
                .code,
            "invalid_java_gc_diagnostic"
        );
    }

    #[test]
    fn malformed_or_predecorated_recipe_never_consumes_attempt() {
        let root = marked_root();
        let distribution = marked_distribution();
        for mode in 0..4 {
            let mut profile = JavaGcDiagnosticProfile::new(root.path()).unwrap();
            let mut value = launch(distribution.path());
            match mode {
                0 => value.config.working_directory = None,
                1 => value.config.args.retain(|arg| arg != OsStr::new("-jar")),
                2 => value.config.args.push("-jar".into()),
                _ => value
                    .config
                    .args
                    .push(WINDOWS_JAVA_GC_DIAGNOSTIC_OPTION.into()),
            }
            let args = value.config.args.clone();
            assert!(profile.decorate(&mut value).is_err());
            assert_eq!(value.config.args, args);
            assert!(!profile.attempted);
        }
    }

    #[cfg(unix)]
    #[test]
    fn markers_and_prior_log_aliases_are_rejected() {
        use std::os::unix::fs::symlink;
        let root = marked_root();
        let distribution = marked_distribution();
        let root_marker = root.path().join(WINDOWS_JAVA_GC_DIAGNOSTIC_MARKER);
        let target = root.path().join("marker-target");
        fs::rename(&root_marker, &target).unwrap();
        symlink(&target, &root_marker).unwrap();
        assert!(JavaGcDiagnosticProfile::new(root.path()).is_err());
        let distribution_marker = distribution
            .path()
            .join(WINDOWS_JAVA_GC_DIAGNOSTIC_DISTRIBUTION_MARKER);
        let target = distribution.path().join("marker-target");
        fs::rename(&distribution_marker, &target).unwrap();
        symlink(&target, &distribution_marker).unwrap();
        assert!(require_fresh_distribution(distribution.path()).is_err());
        fs::remove_file(&distribution_marker).unwrap();
        fs::rename(&target, &distribution_marker).unwrap();
        symlink("nonexistent", distribution.path().join("cedar-gc-123.log")).unwrap();
        assert!(require_fresh_distribution(distribution.path()).is_err());
    }
}
