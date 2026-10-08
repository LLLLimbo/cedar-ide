//! Nonshipping fixture profile; no protocol method, capability, or trust grant.
#[cfg(test)]
use super::java_launch::ordinary_ascii_java_spelling;
use super::java_launch::{
    self, ordinary_local_path, regular_path, relative_launcher, validate_distribution,
    validate_java_executable,
};
use crate::{error, io_error, Workspace};
use cedar_language::{ClientOptions, LspClient, ProcessConfig, Range};
use cedar_protocol::RemoteError;
use serde_json::{json, Value};
#[cfg(test)]
use std::fs;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

pub const WINDOWS_JAVA_VALIDATION_MARKER: &str = ".cedar-windows-java-validation";
pub const WINDOWS_JAVA_VALIDATION_MARKER_CONTENTS: &[u8] = b"cedar-windows-java-validation-v1\n";
pub const WINDOWS_JAVA_EVIDENCE_FILE: &str = ".cedar-windows-java-evidence.jsonl";
const MAX_SESSIONS: u32 = 16;
const MAX_RECORD_BYTES: usize = 1024;

fn invalid(message: &'static str) -> RemoteError {
    error("invalid_java_validation", message)
}

pub(super) fn require_marker(workspace: &Workspace) -> Result<(), RemoteError> {
    let marker = workspace.resolve(WINDOWS_JAVA_VALIDATION_MARKER, false)?;
    regular_path(&marker, false)?;
    let mut contents = Vec::new();
    File::open(marker)
        .map_err(io_error)?
        .take(WINDOWS_JAVA_VALIDATION_MARKER_CONTENTS.len() as u64 + 1)
        .read_to_end(&mut contents)
        .map_err(io_error)?;
    if contents != WINDOWS_JAVA_VALIDATION_MARKER_CONTENTS {
        return Err(invalid(
            "Expected the exact synthetic Java validation marker",
        ));
    }
    Ok(())
}

#[derive(Debug)]
pub(super) struct JavaValidationProfile {
    distribution: PathBuf,
    launcher: PathBuf,
    configuration_uri: String,
    root: PathBuf,
    sessions: u32,
    evidence: EvidenceLog,
}

impl JavaValidationProfile {
    pub(super) fn new(workspace: &Workspace, distribution: &Path) -> Result<Self, RemoteError> {
        if !cfg!(windows) {
            return Err(error(
                "unsupported_platform",
                "Java validation requires Windows",
            ));
        }
        let (distribution, launcher, configuration_uri) = validate_distribution(distribution)?;
        let evidence_path = workspace.resolve(WINDOWS_JAVA_EVIDENCE_FILE, true)?;
        let evidence = EvidenceLog::create(&evidence_path)?;
        Ok(Self {
            distribution,
            launcher,
            configuration_uri,
            root: workspace.root.clone(),
            sessions: 0,
            evidence,
        })
    }

    pub(super) fn configure(
        &self,
        config: &mut ProcessConfig,
        options: &mut ClientOptions,
    ) -> Result<Value, RemoteError> {
        self.configure_inner(config, options).map_err(|mut error| {
            if error.code == "invalid_java_launch" {
                error.code = "invalid_java_validation".into();
            }
            error
        })
    }

    fn configure_inner(
        &self,
        config: &mut ProcessConfig,
        options: &mut ClientOptions,
    ) -> Result<Value, RemoteError> {
        if self.sessions >= MAX_SESSIONS {
            return Err(invalid("Java validation session limit reached"));
        }
        validate_java_executable(config.program.as_os_str())?;
        // Recheck selected paths immediately before launch. This is a fixture
        // identity check, not protection from hostile concurrent file writers.
        let (distribution, launcher, configuration_uri) =
            validate_distribution(&self.distribution)?;
        if distribution != self.distribution
            || launcher != self.launcher
            || configuration_uri != self.configuration_uri
        {
            return Err(invalid("Java distribution changed before launch"));
        }
        validate_arguments(
            config,
            &self.distribution,
            &self.launcher,
            &self.configuration_uri,
            &self.root,
        )?;
        java_launch::check_environment()?;
        config.working_directory = Some(self.distribution.clone());
        *options = java_launch::client_options();
        let mut initialization = java_launch::initialization_options();
        initialization["extendedClientCapabilities"] = json!({"classFileContentsSupport":true});
        Ok(initialization)
    }

    pub(super) fn begin(
        &mut self,
        client: &LspClient,
    ) -> Result<JavaValidationSession, RemoteError> {
        let observed = ObservedProcess::open(client.process_id())?;
        self.sessions += 1;
        let session = JavaValidationSession {
            session: self.sessions,
            observed,
            symbol_provider: false,
        };
        self.evidence.write(EvidenceRecord::Started {
            session: session.session,
            pid: client.process_id(),
            creation_time_100ns_since_1601: session.observed.creation_time,
        })?;
        Ok(session)
    }

    pub(super) fn finish(
        &mut self,
        client: LspClient,
        session: JavaValidationSession,
        initialization_failure: Option<RemoteError>,
    ) -> Result<(), RemoteError> {
        // A fixed semantic witness in this exact live session. This is not a
        // formal barrier/join for JDT's unobservable background jobs.
        let witness = initialization_failure.map_or_else(
            || verify_jdk_symbol(&client, session.symbol_provider),
            |_| Err(invalid("Java initialization did not complete")),
        );
        let shutdown_started = Instant::now();
        let shutdown_api_succeeded = client.shutdown().is_ok();
        // LspClient owns the transport; dropping it joins the worker even after
        // a failed witness, failed initialize, failed shutdown, or forced cleanup.
        drop(client);
        let observation = session.observed.exit_observation();
        let shutdown_elapsed_ms =
            u64::try_from(shutdown_started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let gracefully_exited =
            shutdown_api_succeeded && observation.signaled && observation.exit_code == Some(0);
        let evidence = self.evidence.write(EvidenceRecord::Stopped {
            session: session.session,
            jdk_symbol_verified: witness.is_ok(),
            shutdown_api_succeeded,
            root_handle_signaled: observation.signaled,
            root_exit_code: observation.exit_code,
            gracefully_exited,
            shutdown_elapsed_ms,
        });
        finish_result(
            witness.is_ok(),
            shutdown_api_succeeded,
            observation,
            evidence.is_ok(),
        )
    }
}

#[derive(Debug)]
pub(super) struct JavaValidationSession {
    session: u32,
    observed: ObservedProcess,
    symbol_provider: bool,
}

impl JavaValidationSession {
    pub(super) fn initialized(&mut self, result: &Value) -> Result<(), RemoteError> {
        let provider = &result["capabilities"]["workspaceSymbolProvider"];
        self.symbol_provider = provider == &Value::Bool(true) || provider.is_object();
        self.observed.assert_live()
    }
}

fn verify_jdk_symbol(client: &LspClient, advertised: bool) -> Result<(), RemoteError> {
    if !advertised {
        return Err(invalid("JDT did not advertise workspace symbol support"));
    }
    let symbols = client
        .request("workspace/symbol", json!({"query":"java.lang.String"}))
        .map_err(|_| invalid("The fixed JDK symbol request failed"))?;
    if !has_jdk_string_symbol(&symbols) {
        return Err(invalid(
            "The indexed java.lang.String class was not verified",
        ));
    }
    Ok(())
}

fn has_jdk_string_symbol(value: &Value) -> bool {
    value.as_array().is_some_and(|items| {
        items.iter().any(|item| {
            item["name"] == "String"
                && item["containerName"] == "java.lang"
                && item["kind"] == 5
                && item["location"]["uri"].as_str().is_some_and(|uri| {
                    uri.strip_prefix("jdt://contents/")
                        .is_some_and(|path| !path.is_empty())
                        && !uri.chars().any(|ch| ch.is_whitespace() || ch.is_control())
                })
                && serde_json::from_value::<Range>(item["location"]["range"].clone()).is_ok_and(
                    |range| {
                        [
                            range.start.line,
                            range.start.character,
                            range.end.line,
                            range.end.character,
                        ]
                        .into_iter()
                        .all(|value| value <= i32::MAX as u32)
                            && (range.start.line, range.start.character)
                                <= (range.end.line, range.end.character)
                    },
                )
        })
    })
}

#[derive(Debug, Clone, Copy)]
struct ExitObservation {
    signaled: bool,
    exit_code: Option<u32>,
}

fn finish_result(
    witness: bool,
    shutdown: bool,
    root: ExitObservation,
    evidence: bool,
) -> Result<(), RemoteError> {
    if witness && shutdown && root.signaled && root.exit_code == Some(0) && evidence {
        return Ok(());
    }
    // Fixed, bounded fields preserve simultaneous semantic and cleanup failures
    // without exposing server payloads, argv, environment, or local paths.
    Err(error("java_validation_failed", format!(
        "Java validation failed: jdk_symbol_verified={witness}, shutdown_api_succeeded={shutdown}, root_handle_signaled={}, root_exit_code={:?}, evidence_written={evidence}",
        root.signaled, root.exit_code,
    )))
}

fn unique_argument<'a>(
    config: &'a ProcessConfig,
    flag: &str,
) -> Result<&'a std::ffi::OsStr, RemoteError> {
    let mut found = None;
    for (index, arg) in config.args.iter().enumerate() {
        if arg == flag {
            if found.is_some() {
                return Err(invalid("Duplicate Java fixture location argument"));
            }
            found = Some(
                config
                    .args
                    .get(index + 1)
                    .ok_or_else(|| invalid("Missing Java fixture location argument"))?
                    .as_os_str(),
            );
        }
    }
    found.ok_or_else(|| invalid("Missing Java fixture location flag"))
}

fn validate_arguments(
    config: &ProcessConfig,
    distribution: &Path,
    launcher: &Path,
    configuration_uri: &str,
    root: &Path,
) -> Result<(), RemoteError> {
    let submitted = Path::new(unique_argument(config, "-jar")?);
    if submitted != launcher
        || relative_launcher(distribution, &distribution.join(submitted))? != launcher
    {
        return Err(invalid(
            "Java -jar must select the verified relative launcher",
        ));
    }
    if unique_argument(config, "-configuration")?.to_str() != Some(configuration_uri) {
        return Err(invalid(
            "Java -configuration must select the verified encoded directory URL",
        ));
    }
    let data = unique_argument(config, "-data")?
        .to_str()
        .ok_or_else(|| invalid("Java data URL must be UTF-8"))?;
    let parsed = url::Url::parse(data).map_err(|_| invalid("Invalid Java data URL"))?;
    if parsed.scheme() != "file"
        || parsed.host_str().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(invalid("Java data must use a plain local file URL"));
    }
    let path = parsed
        .to_file_path()
        .map_err(|_| invalid("Invalid Java data directory"))?;
    regular_path(&path, true)?;
    let canonical = path.canonicalize().map_err(io_error)?;
    let relative = canonical
        .strip_prefix(root)
        .map_err(|_| invalid("Java data directory must be inside the synthetic root"))?;
    if relative.as_os_str().is_empty() {
        return Err(invalid(
            "Java data directory must be distinct from the synthetic root",
        ));
    }
    let expected = url::Url::from_directory_path(ordinary_local_path(&path)?)
        .map_err(|_| invalid("Invalid Java data directory URL"))?;
    if parsed != expected {
        return Err(invalid(
            "Java data URL must encode the exact ordinary directory",
        ));
    }
    Ok(())
}

#[derive(Debug)]
struct EvidenceLog {
    file: File,
    records: u32,
}

enum EvidenceRecord {
    Started {
        session: u32,
        pid: u32,
        creation_time_100ns_since_1601: u64,
    },
    Stopped {
        session: u32,
        jdk_symbol_verified: bool,
        shutdown_api_succeeded: bool,
        root_handle_signaled: bool,
        root_exit_code: Option<u32>,
        gracefully_exited: bool,
        shutdown_elapsed_ms: u64,
    },
}

impl EvidenceRecord {
    fn value(self) -> Value {
        match self {
            Self::Started {
                session,
                pid,
                creation_time_100ns_since_1601,
            } => {
                json!({"kind":"agent_java_started", "session":session, "pid":pid, "creation_time_100ns_since_1601":creation_time_100ns_since_1601})
            }
            Self::Stopped {
                session,
                jdk_symbol_verified,
                shutdown_api_succeeded,
                root_handle_signaled,
                root_exit_code,
                gracefully_exited,
                shutdown_elapsed_ms,
            } => {
                json!({"kind":"agent_java_stopped", "session":session, "jdk_symbol_verified":jdk_symbol_verified, "shutdown_api_succeeded":shutdown_api_succeeded, "root_handle_signaled":root_handle_signaled, "root_exit_code":root_exit_code, "gracefully_exited":gracefully_exited, "shutdown_elapsed_ms":shutdown_elapsed_ms})
            }
        }
    }
}

impl EvidenceLog {
    fn create(path: &Path) -> Result<Self, RemoteError> {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(1);
        } // FILE_SHARE_READ only
        Ok(Self {
            file: options.open(path).map_err(io_error)?,
            records: 0,
        })
    }

    fn write(&mut self, record: EvidenceRecord) -> Result<(), RemoteError> {
        if self.records >= MAX_SESSIONS * 2 {
            return Err(invalid("Java evidence record limit reached"));
        }
        let mut bytes = serde_json::to_vec(&record.value())
            .map_err(|_| invalid("Java evidence serialization failed"))?;
        if bytes.len() >= MAX_RECORD_BYTES {
            return Err(invalid("Java evidence record exceeds limit"));
        }
        bytes.push(b'\n');
        self.file.write_all(&bytes).map_err(io_error)?;
        self.file.flush().map_err(io_error)?;
        self.records += 1;
        Ok(())
    }
}

#[cfg(windows)]
use process_observer::ObservedProcess;
#[cfg(windows)]
mod process_observer {
    use super::*;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::Foundation::{FILETIME, WAIT_OBJECT_0, WAIT_TIMEOUT};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, GetProcessTimes, OpenProcess, WaitForSingleObject,
        PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    };

    #[derive(Debug)]
    pub(super) struct ObservedProcess {
        handle: OwnedHandle,
        pub(super) creation_time: u64,
    }
    impl ObservedProcess {
        pub(super) fn open(pid: u32) -> Result<Self, RemoteError> {
            // SAFETY: Known freshly spawned root; noninheritable query/wait-only
            // observation handle, never termination or mutation authority.
            let raw = unsafe {
                OpenProcess(
                    PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                    0,
                    pid,
                )
            };
            if raw.is_null() {
                return Err(io_error(std::io::Error::last_os_error()));
            }
            // SAFETY: Fresh owned handle closed exactly once by OwnedHandle.
            let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
            let mut times = [FILETIME {
                dwLowDateTime: 0,
                dwHighDateTime: 0,
            }; 4];
            // SAFETY: Held query handle and distinct initialized output objects.
            if unsafe {
                GetProcessTimes(
                    handle.as_raw_handle(),
                    &mut times[0],
                    &mut times[1],
                    &mut times[2],
                    &mut times[3],
                )
            } == 0
            {
                return Err(io_error(std::io::Error::last_os_error()));
            }
            let observed = Self {
                handle,
                creation_time: (u64::from(times[0].dwHighDateTime) << 32)
                    | u64::from(times[0].dwLowDateTime),
            };
            observed.assert_live()?;
            Ok(observed)
        }
        pub(super) fn assert_live(&self) -> Result<(), RemoteError> {
            // SAFETY: Retained SYNCHRONIZE handle, observation only.
            if unsafe { WaitForSingleObject(self.handle.as_raw_handle(), 0) } == WAIT_TIMEOUT {
                Ok(())
            } else {
                Err(invalid("Observed Java root is no longer live"))
            }
        }
        pub(super) fn exit_observation(&self) -> ExitObservation {
            // SAFETY: The original handle remains retained, preventing PID reuse
            // from changing this identity; bounded observation after client join.
            let signaled =
                unsafe { WaitForSingleObject(self.handle.as_raw_handle(), 1500) } == WAIT_OBJECT_0;
            let mut code = 0;
            // SAFETY: Query the retained handle only after termination signaled.
            let exit_code = if signaled
                && unsafe { GetExitCodeProcess(self.handle.as_raw_handle(), &mut code) } != 0
            {
                Some(code)
            } else {
                None
            };
            ExitObservation {
                signaled,
                exit_code,
            }
        }
    }
}

#[cfg(not(windows))]
#[derive(Debug)]
struct ObservedProcess {
    creation_time: u64,
}
#[cfg(not(windows))]
impl ObservedProcess {
    fn open(_: u32) -> Result<Self, RemoteError> {
        Err(invalid("Java process observation requires Windows"))
    }
    fn assert_live(&self) -> Result<(), RemoteError> {
        Err(invalid("Java process observation requires Windows"))
    }
    fn exit_observation(&self) -> ExitObservation {
        ExitObservation {
            signaled: false,
            exit_code: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn distribution(root: &Path) -> PathBuf {
        let path = root.join("distribution with spaces 雪");
        fs::create_dir_all(path.join("plugins")).unwrap();
        fs::create_dir(path.join("config_win")).unwrap();
        fs::write(
            path.join("plugins/org.eclipse.equinox.launcher_1.jar"),
            b"fixture",
        )
        .unwrap();
        path
    }

    fn symbol() -> Value {
        json!([{"name":"String","containerName":"java.lang","kind":5,
            "location":{"uri":"jdt://contents/java.base/java.lang/String.class",
            "range":{"start":{"line":0,"character":0},"end":{"line":1,"character":0}}}}])
    }

    #[test]
    fn jdk_witness_requires_exact_class_container_binary_uri_and_u31_ordered_range() {
        assert!(has_jdk_string_symbol(&symbol()));
        assert!(!has_jdk_string_symbol(&Value::Null));
        assert!(!has_jdk_string_symbol(&json!([])));
        for (pointer, invalid_value) in [
            ("/0/name", json!("Strings")),
            ("/0/containerName", json!("example")),
            ("/0/kind", json!(6)),
            ("/0/location/uri", json!("file:///String.java")),
            ("/0/location/uri", json!("jdt://contents/")),
            ("/0/location/uri", json!("jdt://contents/space class")),
            ("/0/location/range/start/line", json!(-1)),
            ("/0/location/range/start/line", json!(2)),
            ("/0/location/range/end/character", json!(2147483648_u64)),
            ("/0/location/range/end/character", json!(0.5)),
        ] {
            let mut value = symbol();
            *value.pointer_mut(pointer).unwrap() = invalid_value;
            assert!(!has_jdk_string_symbol(&value), "{pointer}: {value}");
        }
        let mut boundary = symbol();
        boundary[0]["location"]["range"]["end"]["character"] = json!(i32::MAX);
        assert!(has_jdk_string_symbol(&boundary));
    }

    #[test]
    fn semantic_failure_never_hides_cleanup_failure_or_unproven_natural_exit() {
        for witness in [false, true] {
            for shutdown in [false, true] {
                for signaled in [false, true] {
                    for exit_code in [None, Some(0), Some(1), Some(259), Some(u32::MAX)] {
                        for evidence in [false, true] {
                            let result = finish_result(
                                witness,
                                shutdown,
                                ExitObservation {
                                    signaled,
                                    exit_code,
                                },
                                evidence,
                            );
                            assert_eq!(
                                result.is_ok(),
                                witness && shutdown && signaled && exit_code == Some(0) && evidence
                            );
                            if let Err(error) = result {
                                assert_eq!(error.code, "java_validation_failed");
                                assert!(error
                                    .message
                                    .contains(&format!("jdk_symbol_verified={witness}")));
                                assert!(error
                                    .message
                                    .contains(&format!("shutdown_api_succeeded={shutdown}")));
                                assert!(error
                                    .message
                                    .contains(&format!("root_handle_signaled={signaled}")));
                                assert!(error.message.len() < MAX_RECORD_BYTES);
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn java_executable_spelling_rejects_shell_path_fallback_verbatim_unc_and_unicode() {
        for accepted in [
            r"C:\Java\bin\java.exe",
            "D:/Program Files/Java/bin/JAVA.EXE",
        ] {
            assert!(ordinary_ascii_java_spelling(accepted));
        }
        for rejected in [
            "java",
            "java.exe",
            r"C:java.exe",
            r"\Java\java.exe",
            r"\\?\C:\Java\java.exe",
            r"\\server\Java\java.exe",
            r"\\.\C:\Java\java.exe",
            r"C:\雪\java.exe",
            r"C:\Java\java.cmd",
            r"C:\Java\java.exe:stream",
            r"C:\Java\..\java.exe",
            r"C:\Java.\java.exe",
            r"C:\Java \java.exe",
            "C:/Java/\n/java.exe",
            "C://Java/java.exe",
            "C:/Java/./java.exe",
        ] {
            assert!(!ordinary_ascii_java_spelling(rejected), "{rejected:?}");
        }
    }

    #[test]
    fn distribution_requires_existing_config_and_one_exact_ascii_relative_launcher() {
        let temp = tempfile::tempdir().unwrap();
        let path = distribution(temp.path());
        let (ordinary, launcher, uri) = validate_distribution(&path).unwrap();
        assert_eq!(
            ordinary.canonicalize().unwrap(),
            path.canonicalize().unwrap()
        );
        assert_eq!(
            launcher,
            Path::new("plugins").join("org.eclipse.equinox.launcher_1.jar")
        );
        assert!(uri.ends_with("/config_win/"));
        assert!(uri.is_ascii() && uri.contains("%E9%9B%AA"));
        fs::write(
            path.join("plugins/org.eclipse.equinox.launcher_2.jar"),
            b"fixture",
        )
        .unwrap();
        assert!(validate_distribution(&path).is_err());
        fs::remove_file(path.join("plugins/org.eclipse.equinox.launcher_2.jar")).unwrap();
        fs::rename(
            path.join("plugins/org.eclipse.equinox.launcher_1.jar"),
            path.join("plugins/org.eclipse.equinox.launcher_雪.jar"),
        )
        .unwrap();
        assert!(validate_distribution(&path).is_err());
        fs::remove_dir(path.join("config_win")).unwrap();
        assert!(validate_distribution(&path).is_err());
        assert!(validate_distribution(Path::new("relative")).is_err());
        assert!(validate_distribution(&temp.path().join("missing")).is_err());
    }

    #[test]
    fn profile_arguments_confine_data_and_reject_absolute_or_duplicate_launcher_locations() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let path = distribution(&root);
        let (distribution, launcher, uri) = validate_distribution(&path).unwrap();
        let data = root.join("data with spaces 雪");
        fs::create_dir(&data).unwrap();
        let data_uri = url::Url::from_directory_path(ordinary_local_path(&data).unwrap())
            .unwrap()
            .to_string();
        let mut config = ProcessConfig::new("fixture");
        config.args = vec![
            "-jar".into(),
            launcher.clone().into(),
            "-configuration".into(),
            uri.clone().into(),
            "-data".into(),
            data_uri.into(),
        ];
        validate_arguments(&config, &distribution, &launcher, &uri, &root).unwrap();
        let original = config.clone();
        config.args[1] = distribution.join(&launcher).into();
        assert!(validate_arguments(&config, &distribution, &launcher, &uri, &root).is_err());
        config = original.clone();
        config.args.extend(["-jar".into(), launcher.clone().into()]);
        assert!(validate_arguments(&config, &distribution, &launcher, &uri, &root).is_err());
        config = original.clone();
        config.args[3] = "file:///wrong/config_win/".into();
        assert!(validate_arguments(&config, &distribution, &launcher, &uri, &root).is_err());
        let outside = tempfile::tempdir().unwrap();
        config = original.clone();
        config.args[5] =
            url::Url::from_directory_path(ordinary_local_path(outside.path()).unwrap())
                .unwrap()
                .to_string()
                .into();
        assert!(validate_arguments(&config, &distribution, &launcher, &uri, &root).is_err());
        config = original;
        config.args.pop();
        assert!(validate_arguments(&config, &distribution, &launcher, &uri, &root).is_err());
    }

    #[test]
    fn evidence_is_create_new_bounded_and_contains_only_typed_fields() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join(WINDOWS_JAVA_EVIDENCE_FILE);
        let mut evidence = EvidenceLog::create(&path).unwrap();
        assert!(EvidenceLog::create(&path).is_err());
        evidence
            .write(EvidenceRecord::Started {
                session: 1,
                pid: 123,
                creation_time_100ns_since_1601: 456,
            })
            .unwrap();
        evidence
            .write(EvidenceRecord::Stopped {
                session: 1,
                jdk_symbol_verified: false,
                shutdown_api_succeeded: true,
                root_handle_signaled: true,
                root_exit_code: Some(1),
                gracefully_exited: false,
                shutdown_elapsed_ms: 10,
            })
            .unwrap();
        let content = fs::read_to_string(&path).unwrap();
        let lines: Vec<Value> = content
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].as_object().unwrap().len(), 4);
        assert_eq!(lines[1].as_object().unwrap().len(), 8);
        assert_eq!(lines[0]["kind"], "agent_java_started");
        assert_eq!(lines[1]["kind"], "agent_java_stopped");
        assert_eq!(lines[1]["root_exit_code"], 1);
        assert_eq!(lines[1]["gracefully_exited"], false);
        for session in 2..=MAX_SESSIONS {
            for _ in 0..2 {
                evidence
                    .write(EvidenceRecord::Started {
                        session,
                        pid: u32::MAX,
                        creation_time_100ns_since_1601: u64::MAX,
                    })
                    .unwrap();
            }
        }
        assert!(evidence
            .write(EvidenceRecord::Started {
                session: 17,
                pid: 1,
                creation_time_100ns_since_1601: 1
            })
            .is_err());
        assert!(
            fs::metadata(&path).unwrap().len()
                < u64::from(MAX_SESSIONS) * 2 * MAX_RECORD_BYTES as u64
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn evidence_and_distribution_reject_symlink_targets_without_modifying_them() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("original");
        fs::write(&target, b"unchanged").unwrap();
        let alias = temp.path().join(WINDOWS_JAVA_EVIDENCE_FILE);
        std::os::unix::fs::symlink(&target, &alias).unwrap();
        assert!(EvidenceLog::create(&alias).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"unchanged");
        let path = distribution(temp.path());
        let jar = path.join("plugins/org.eclipse.equinox.launcher_1.jar");
        fs::remove_file(&jar).unwrap();
        std::os::unix::fs::symlink(&target, &jar).unwrap();
        assert!(validate_distribution(&path).is_err());
    }
}
