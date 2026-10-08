//! Opt-in real Windows cedar_agent::serve → Workspace → JDT → editor acceptance.
//! Run serially with explicit verified prebuilt paths. Cross-compiling is not a
//! native runtime pass and this fixture grants no shipping Windows capability.
use super::*;
use cedar_protocol::{LanguageQueryKind, Operation, Payload, LANGUAGE_SESSION_CAPABILITIES};
use std::{
    fs,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    panic::{catch_unwind, AssertUnwindSafe},
    path::{Component, Path, PathBuf, Prefix},
    process::Command,
    thread,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{FILETIME, WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::Threading::{
        GetExitCodeProcess, GetProcessTimes, OpenProcess, WaitForSingleObject,
        PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    },
};

#[allow(dead_code)]
#[path = "../../client/tests/support/windows_task_harness.rs"]
mod harness;
use harness::{RawAgent, Watchdog};

const START_TIMEOUT: Duration = Duration::from_secs(75);
const FEATURE_TIMEOUT: Duration = Duration::from_secs(75);
// Includes the profile's fixed 60-second JDK symbol request, shutdown request,
// graceful process wait and bounded transport teardown, never just the RPC.
const STOP_TIMEOUT: Duration = Duration::from_secs(150);
const EXIT_TIMEOUT: Duration = Duration::from_secs(30);
const EVIDENCE: &str = ".cedar-windows-java-evidence.jsonl";

fn checked(action: impl FnOnce() -> CheckResult<()>) -> CheckResult<()> {
    match catch_unwind(AssertUnwindSafe(action)) {
        Ok(result) => result,
        Err(panic) => Err(panic
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| panic.downcast_ref::<&str>().map(|s| (*s).into()))
            .unwrap_or_else(|| "acceptance assertion panicked".into())),
    }
}
fn require(value: bool, message: &str) -> CheckResult<()> {
    if value {
        Ok(())
    } else {
        Err(message.into())
    }
}
fn text(path: &Path) -> CheckResult<String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| "fixture path is not UTF-8".into())
}
fn io<T>(value: std::io::Result<T>) -> CheckResult<T> {
    value.map_err(|error| error.to_string())
}
fn ordinary_path(path: &Path) -> CheckResult<PathBuf> {
    require(
        path.is_absolute(),
        "fixture input must be an absolute local path",
    )?;
    let canonical = io(path.canonicalize())?;
    let path_text = text(&canonical)?;
    let plain = match canonical.components().next() {
        Some(Component::Prefix(prefix)) => match prefix.kind() {
            Prefix::VerbatimDisk(_) => PathBuf::from(
                path_text
                    .strip_prefix(r"\\?\")
                    .ok_or("invalid verbatim drive path")?,
            ),
            Prefix::Disk(_) => canonical.clone(),
            _ => return Err("UNC/device fixture paths are not supported".into()),
        },
        _ => return Err("fixture must have a local drive prefix".into()),
    };
    require(
        io(plain.canonicalize())? == canonical,
        "ordinary path changed the selected canonical file identity",
    )?;
    Ok(plain)
}
fn environment_path(name: &str) -> CheckResult<PathBuf> {
    let path = PathBuf::from(
        std::env::var_os(name)
            .ok_or_else(|| format!("set {name} to the exact verified prebuilt path"))?,
    );
    ordinary_path(&path)
}
fn directory_url(path: &Path) -> CheckResult<String> {
    url::Url::from_directory_path(ordinary_path(path)?)
        .map(String::from)
        .map_err(|_| "cannot encode local directory URL".into())
}
fn launch_arguments(distribution: &Path) -> CheckResult<Vec<String>> {
    let jars: Vec<_> = io(fs::read_dir(distribution.join("plugins")))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("org.eclipse.equinox.launcher_"))
                && path.extension().is_some_and(|extension| extension == "jar")
        })
        .collect();
    require(jars.len() == 1, "expected exactly one Equinox launcher")?;
    let relative = jars[0]
        .strip_prefix(distribution)
        .map_err(|e| e.to_string())?;
    require(
        text(relative)?.is_ascii()
            && relative
                .components()
                .all(|c| matches!(c, Component::Normal(_)))
            && io(distribution.join(relative).canonicalize())? == io(jars[0].canonicalize())?,
        "launcher must be an identity-checked ASCII relative file",
    )?;
    let mut args: Vec<String> = [
        "-Declipse.application=org.eclipse.jdt.ls.core.id1",
        "-Dosgi.bundles.defaultStartLevel=4",
        "-Declipse.product=org.eclipse.jdt.ls.core.product",
        "-Dlog.level=WARNING",
        "-Xmx512m",
        "--add-modules=ALL-SYSTEM",
        "--add-opens",
        "java.base/java.util=ALL-UNNAMED",
        "--add-opens",
        "java.base/java.lang=ALL-UNNAMED",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    if let Some(directory) = std::env::var_os("CEDAR_JAVA_ERROR_DIR") {
        let directory = ordinary_path(Path::new(&directory))?;
        let value = text(&directory)?;
        require(
            directory.is_dir()
                && value.is_ascii()
                && !value.bytes().any(|b| b.is_ascii_control() || b == b'%'),
            "crash report directory must be existing ordinary ASCII without template escapes",
        )?;
        args.push(format!(
            "-XX:ErrorFile={}",
            directory.join("hs_err_pid%p.log").display()
        ));
        args.push("-XX:-CreateCoredumpOnCrash".into());
    }
    args.extend([
        "-jar".into(),
        text(relative)?,
        "-configuration".into(),
        directory_url(&distribution.join("config_win"))?,
        "-data".into(),
    ]);
    Ok(args)
}
fn language(agent: &mut RawAgent, op: Operation, timeout: Duration) -> CheckResult<Value> {
    let payload = agent
        .request_with_timeout(op, timeout)
        .map_err(|error| format!("{}: {}", error.code, error.message))?;
    match payload {
        Payload::Language { value } => Ok(value),
        _ => Err("unexpected non-language response".into()),
    }
}
fn unchanged(path: &Path) -> CheckResult<()> {
    require(
        io(fs::read(path))? == SOURCE.as_bytes(),
        "synthetic source was implicitly saved",
    )
}
fn evidence(root: &Path) -> CheckResult<Vec<AgentEvidence>> {
    let path = root.join(EVIDENCE);
    let metadata = io(fs::symlink_metadata(&path))?;
    require(
        metadata.is_file() && !metadata.file_type().is_symlink() && metadata.len() <= 32 * 1024,
        "lifecycle evidence must remain a bounded ordinary file",
    )?;
    parse_evidence(&io(fs::read(path))?)
}

struct RootObservation {
    session: u32,
    pid: u32,
    created: u64,
    handle: OwnedHandle,
}
impl RootObservation {
    fn open(session: u32, pid: u32, created: u64) -> CheckResult<Self> {
        require(pid != 0 && created != 0, "invalid Java root identity")?;
        // SAFETY: PID comes from this private fixture's just-started session.
        // Only observation rights are requested; creation time is checked below.
        let raw = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                0,
                pid,
            )
        };
        if raw.is_null() {
            return Err(std::io::Error::last_os_error().to_string());
        }
        // SAFETY: The successful OpenProcess result is fresh ownership.
        let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
        let mut times = [FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        }; 4];
        // SAFETY: Held query handle and four distinct writable FILETIME outputs.
        let ok = unsafe {
            GetProcessTimes(
                handle.as_raw_handle(),
                &mut times[0],
                &mut times[1],
                &mut times[2],
                &mut times[3],
            )
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let actual = (u64::from(times[0].dwHighDateTime) << 32) | u64::from(times[0].dwLowDateTime);
        require(
            created == actual,
            "PID creation time did not match the started Java root",
        )?;
        let observed = Self {
            session,
            pid,
            created,
            handle,
        };
        require(
            observed.live()?,
            "Java root was not live when observation handle opened",
        )?;
        Ok(observed)
    }
    fn live(&self) -> CheckResult<bool> {
        // SAFETY: Held observation-only handle, zero-time wait.
        match unsafe { WaitForSingleObject(self.handle.as_raw_handle(), 0) } {
            WAIT_TIMEOUT => Ok(true),
            WAIT_OBJECT_0 => Ok(false),
            _ => Err(std::io::Error::last_os_error().to_string()),
        }
    }
    fn exit_code(&self) -> CheckResult<u32> {
        // SAFETY: Same retained SYNCHRONIZE handle, bounded wait, no PID reuse.
        match unsafe { WaitForSingleObject(self.handle.as_raw_handle(), 1500) } {
            WAIT_OBJECT_0 => {}
            WAIT_TIMEOUT => return Err("Java root remained live after stop/agent cleanup".into()),
            _ => return Err(std::io::Error::last_os_error().to_string()),
        }
        let mut code = 0;
        // SAFETY: Held query handle and writable DWORD; signalled wait proves
        // termination separately from any native exit value, including 259.
        if unsafe { GetExitCodeProcess(self.handle.as_raw_handle(), &mut code) } == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(code)
    }
}

fn await_diagnostics(
    agent: &mut RawAgent,
    uri: &str,
    session: u32,
    phase: DiagnosticPhase,
) -> CheckResult<()> {
    let started = Instant::now();
    let deadline = started + Duration::from_secs(60);
    let mut receipt = DiagnosticEvidence::new(session, phase);
    // RawAgent transport deadlines may panic. Capture them here so this one
    // bounded receipt is emitted before the outer session performs cleanup.
    let outcome = checked(|| {
        while Instant::now() < deadline {
            receipt.begin_poll();
            let response = language(agent, Operation::LanguageEvents, FEATURE_TIMEOUT)?;
            match receipt.inspect_response(&response, uri) {
                Ok(true) => return Ok(()),
                Ok(false) => {}
                Err(_) => return Err("diagnostic event stream failed; see typed receipt".into()),
            }
            thread::sleep(Duration::from_millis(100));
        }
        receipt.result = DiagnosticResult::Timeout;
        Err("timed out awaiting exact source-specific diagnostics".into())
    });
    receipt.finish_elapsed(started.elapsed().as_millis());
    println!(
        "{}",
        serde_json::to_string(&receipt).expect("typed diagnostic evidence")
    );
    outcome
}

#[allow(clippy::too_many_arguments)]
fn run_semantics(
    agent: &mut RawAgent,
    root: &Path,
    java: &str,
    args: &[String],
    data: &Path,
    observed: &mut Vec<RootObservation>,
    record: &mut SessionEvidence,
    stage: &Cell<FailureStage>,
) -> CheckResult<()> {
    let source = root.join(SOURCE_PATH);
    let mut args = args.to_vec();
    args.push(directory_url(data)?);
    stage.set(FailureStage::Initialize);
    let started = Instant::now();
    let initialized = language(
        agent,
        Operation::LanguageStart {
            program: java.into(),
            args,
        },
        START_TIMEOUT,
    )?;
    record.initialization_ms = started.elapsed().as_millis() as u64;
    require(
        initialized["started"] == true,
        "LanguageStart did not confirm initialization",
    )?;
    let records = evidence(root)?;
    let identities: Vec<_> = records
        .iter()
        .filter_map(|row| match row {
            AgentEvidence::Started {
                session,
                pid,
                creation_time_100ns_since_1601,
            } if *session == record.session => Some((*pid, *creation_time_100ns_since_1601)),
            _ => None,
        })
        .collect();
    require(
        identities.len() == 1,
        "missing or repeated started identity for this session",
    )?;
    let (pid, created) = identities[0];
    let process = RootObservation::open(record.session, pid, created)?;
    require(
        !observed
            .iter()
            .any(|old| (old.pid, old.created) == (pid, created)),
        "restart reused an earlier root identity",
    )?;
    observed.push(process);
    record.root_observed_live = true;
    record.root_identity_verified = true;
    require(
        initialized.pointer("/initialize/capabilities/completionProvider/resolveProvider")
            == Some(&Value::Bool(true)),
        "JDT did not advertise deferred completion resolve",
    )?;
    require(
        data.join(".metadata").is_dir(),
        "JDT did not use the intended Unicode data directory",
    )?;
    unchanged(&source)?;
    stage.set(FailureStage::Open);
    let opened = language(
        agent,
        Operation::LanguageOpen {
            path: SOURCE_PATH.into(),
            language_id: "java".into(),
            version: 1,
            text: SOURCE.into(),
        },
        FEATURE_TIMEOUT,
    )?;
    let uri = opened["opened"]
        .as_str()
        .ok_or("missing opened document URI")?
        .to_owned();
    let expected_uri = url::Url::from_file_path(ordinary_path(&source)?)
        .map_err(|_| "cannot encode source URI")?;
    require(
        same_local_uri(&uri, expected_uri.as_str()),
        "agent opened an unexpected document URI",
    )?;
    unchanged(&source)?;
    stage.set(FailureStage::Diagnostics);
    await_diagnostics(agent, &uri, record.session, DiagnosticPhase::Initial)?;
    record.exact_diagnostics = true;
    unchanged(&source)?;
    let cursor = completion::byte_to_position(
        SOURCE,
        SOURCE.rfind("greeting").ok_or("fixture reference")? + 3,
    )?;
    stage.set(FailureStage::Definition);
    let definition = language(
        agent,
        Operation::LanguageQuery {
            path: SOURCE_PATH.into(),
            line: cursor.line,
            character: cursor.character,
            kind: LanguageQueryKind::Definition,
        },
        FEATURE_TIMEOUT,
    )?;
    let definition_uri = exact_definition(&definition, &uri)?;
    let confined = language(
        agent,
        Operation::LanguageResolveUri {
            uri: definition_uri,
        },
        FEATURE_TIMEOUT,
    )?;
    require(
        confined["path"] == SOURCE_PATH,
        "definition URI did not resolve to the source",
    )?;
    record.exact_definition = true;
    unchanged(&source)?;
    stage.set(FailureStage::Completion);
    let response = language(
        agent,
        Operation::LanguageQuery {
            path: SOURCE_PATH.into(),
            line: cursor.line,
            character: cursor.character,
            kind: LanguageQueryKind::Completion,
        },
        FEATURE_TIMEOUT,
    )?;
    let result = completion::parse_completion_result(&response)?;
    require(
        result.candidates.iter().any(|candidate| {
            candidate.label.starts_with("greeting") && candidate.item["textEdit"].is_object()
        }),
        "real completion omitted the local source variable",
    )?;
    let candidate = result
        .candidates
        .into_iter()
        .find(|candidate| candidate.label.starts_with("GregorianCalendar"))
        .ok_or("missing real GregorianCalendar completion")?;
    require(
        candidate.disabled_reason.is_none(),
        "real completion was disabled by the frontend",
    )?;
    record.real_completion = true;
    unchanged(&source)?;
    let original = candidate.item;
    require(
        original["textEdit"].is_object()
            && !original["data"].is_null()
            && original
                .get("additionalTextEdits")
                .is_none_or(|v| v.is_null() || v.as_array().is_some_and(Vec::is_empty)),
        "completion import was not deferred behind opaque resolve data",
    )?;
    stage.set(FailureStage::Resolve);
    let resolved = language(
        agent,
        Operation::LanguageResolveCompletion {
            item: original.clone(),
        },
        FEATURE_TIMEOUT,
    )?;
    let edits = resolved["additionalTextEdits"]
        .as_array()
        .ok_or("resolved completion has no imports")?;
    require(
        edits.len() == 1
            && edits[0]["newText"]
                .as_str()
                .is_some_and(|value| value.contains("import java.util.GregorianCalendar;")),
        "resolve did not return exactly one expected import edit",
    )?;
    record.deferred_import_resolve = true;
    crate::language_ui::validate_resolved_identity(&original, &resolved)?;
    require(
        original["textEdit"] == resolved["textEdit"],
        "resolved primary edit changed",
    )?;
    record.primary_identity_unchanged = true;
    unchanged(&source)?;
    editor_transaction(original, resolved, stage, |version, doc| {
        unchanged(&source)?;
        let changed = language(
            agent,
            Operation::LanguageChange {
                path: doc.path.clone(),
                version,
                text: doc.text.clone(),
            },
            FEATURE_TIMEOUT,
        )?;
        require(
            changed["version"] == version
                && changed["changed"]
                    .as_str()
                    .is_some_and(|actual| same_local_uri(actual, &uri)),
            "didChange did not acknowledge the actual editor document",
        )?;
        unchanged(&source)?;
        match version {
            2 => {
                record.two_atomic_edits = true;
                record.advisory_command_skipped = true;
            }
            3 => record.actual_undo = true,
            4 => record.actual_redo = true,
            _ => return Err("unexpected editor sync version".into()),
        }
        Ok(())
    })?;
    record.versions_2_3_4_synced = true;
    // A separate unsaved correction has a unique warning witness, so an old
    // empty diagnostic batch cannot be mistaken for successful didChange.
    stage.set(FailureStage::Correction);
    record.correction_change_result = CorrectionChangeResult::RequestError;
    let changed = language(
        agent,
        Operation::LanguageChange {
            path: SOURCE_PATH.into(),
            version: 5,
            text: corrected_source(),
        },
        FEATURE_TIMEOUT,
    )?;
    record.correction_change_result = CorrectionChangeResult::AcknowledgementMismatch;
    require(
        changed["version"] == 5
            && changed["changed"]
                .as_str()
                .is_some_and(|actual| same_local_uri(actual, &uri)),
        "corrected draft version or URI mismatch",
    )?;
    record.correction_change_result = CorrectionChangeResult::Acknowledged;
    record.correction_change_acknowledged = true;
    await_diagnostics(agent, &uri, record.session, DiagnosticPhase::Correction)?;
    record.correction_diagnostics = true;
    unchanged(&source)?;
    stage.set(FailureStage::Close);
    language(
        agent,
        Operation::LanguageClose {
            path: SOURCE_PATH.into(),
        },
        FEATURE_TIMEOUT,
    )?;
    unchanged(&source)?;
    require(
        observed.last().ok_or("missing live observer")?.live()?,
        "Java root exited before explicit LanguageStop",
    )?;
    record.source_unchanged = true;
    record.semantic_checks_passed = true;
    Ok(())
}

fn stop_session(
    agent: &mut RawAgent,
    root: &Path,
    observed: &[RootObservation],
    record: &mut SessionEvidence,
    stage: &Cell<FailureStage>,
) -> CheckResult<()> {
    // Preserve cleanup evidence even when the profile returns a semantic or
    // graceful-shutdown error. Its stop path always attempts transport cleanup.
    stage.set(FailureStage::Stop);
    let stop = checked(|| {
        let value = language(agent, Operation::LanguageStop, STOP_TIMEOUT)?;
        require(
            value["stopped"] == true,
            "LanguageStop did not acknowledge the session",
        )
    });
    stage.set(FailureStage::RootExit);
    let native = observed
        .iter()
        .find(|process| process.session == record.session)
        .ok_or_else(|| "missing independent retained root handle".to_owned())
        .and_then(RootObservation::exit_code);
    // Keep the independently observed result even if the agent's evidence file
    // is absent or malformed after failure. Never substitute a Job/listener test.
    record.root_handle_signaled = native.is_ok();
    record.root_exit_code = native.as_ref().ok().copied();
    let receipt = (|| -> CheckResult<()> {
        let records = evidence(root)?;
        let stops: Vec<_> = records.iter().filter(|row| matches!(row, AgentEvidence::Stopped { session, .. } if *session == record.session)).collect();
        require(
            stops.len() == 1,
            "missing or duplicate stopped lifecycle evidence",
        )?;
        let AgentEvidence::Stopped {
            jdk_symbol_verified,
            shutdown_api_succeeded,
            root_handle_signaled,
            root_exit_code,
            gracefully_exited,
            shutdown_elapsed_ms,
            ..
        } = stops[0]
        else {
            unreachable!()
        };
        record.jdk_symbol_verified = *jdk_symbol_verified;
        record.shutdown_api_succeeded = *shutdown_api_succeeded;
        record.shutdown_elapsed_ms = *shutdown_elapsed_ms;
        let actual = *native.as_ref().map_err(Clone::clone)?;
        record.gracefully_exited = actual == 0 && *gracefully_exited;
        require(
            *root_handle_signaled && *root_exit_code == Some(actual),
            "agent and independent root observations disagree",
        )?;
        require(
            *jdk_symbol_verified && *shutdown_api_succeeded && actual == 0 && *gracefully_exited,
            "Java stop did not prove JDK witness plus graceful actual exit zero",
        )
    })();
    if stop.is_err() {
        // A later successful observation must not replace the first failed step.
        stage.set(FailureStage::Stop);
    }
    match (stop, receipt) {
        (Err(primary), Err(cleanup)) => {
            Err(format!("{primary}; lifecycle cleanup evidence: {cleanup}"))
        }
        (Err(error), _) | (_, Err(error)) => Err(error),
        _ => Ok(()),
    }
}

#[test]
#[ignore = "requires native Windows, verified JDT/Java and exact prebuilt validation agent; run serially"]
fn real_windows_agent_java_editor_transactions() -> CheckResult<()> {
    // libtest prints its test-name prefix without a newline under --nocapture.
    // Keep every subsequent typed record a standalone JSON line for sanitation.
    println!();
    let _watchdog = Watchdog::start_with_timeout(Duration::from_secs(360));
    let mut fixture: Option<tempfile::TempDir> = None;
    let mut agent: Option<RawAgent> = None;
    let mut observed = Vec::new();
    let mut completed = 0;
    let mut cleanup_errors = Vec::new();
    let stage = Cell::new(FailureStage::Setup);
    let mut failure_stage = None;
    let primary = checked(|| {
        for name in [
            "CLIENT_PORT",
            "CLIENT_HOST",
            "socket.stream.debug",
            "JDK_JAVA_OPTIONS",
            "JAVA_TOOL_OPTIONS",
            "_JAVA_OPTIONS",
        ] {
            require(
                std::env::var_os(name).is_none(),
                "unexpected Java launcher/socket environment injection",
            )?;
        }
        let distribution = environment_path("CEDAR_JDTLS_HOME")?;
        let java = environment_path("CEDAR_JAVA")?;
        let binary = environment_path("CEDAR_AGENT_LANGUAGE_VALIDATION_BIN")?;
        require(
            java.is_file()
                && java
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("java.exe"))
                && text(&java)?.is_ascii(),
            "Java must be an identity-checked ordinary absolute ASCII java.exe",
        )?;
        require(
            binary.is_file()
                && binary
                    .extension()
                    .is_some_and(|extension| extension == "exe"),
            "validation agent must be the exact prebuilt executable",
        )?;
        let args = launch_arguments(&distribution)?;
        fixture = Some(io(tempfile::Builder::new()
            .prefix("cedar agent editor 雪 ")
            .tempdir())?);
        let root = ordinary_path(fixture.as_ref().unwrap().path())?;
        let project = root.join(PROJECT_DIR);
        io(fs::create_dir_all(project.join("src")))?;
        io(fs::create_dir_all(project.join(".settings")))?;
        io(fs::write(
            root.join(".cedar-windows-language-validation"),
            "cedar-windows-language-validation-v1\n",
        ))?;
        io(fs::write(
            root.join(".cedar-windows-java-validation"),
            "cedar-windows-java-validation-v1\n",
        ))?;
        io(fs::write(project.join(".project"), "<?xml version=\"1.0\"?><projectDescription><name>cedar-editor-smoke</name><projects/><buildSpec><buildCommand><name>org.eclipse.jdt.core.javabuilder</name><arguments/></buildCommand></buildSpec><natures><nature>org.eclipse.jdt.core.javanature</nature></natures></projectDescription>"))?;
        io(fs::write(project.join(".classpath"), "<?xml version=\"1.0\"?><classpath><classpathentry kind=\"src\" path=\"src\"/><classpathentry kind=\"con\" path=\"org.eclipse.jdt.launching.JRE_CONTAINER\"/><classpathentry kind=\"output\" path=\"bin\"/></classpath>"))?;
        io(fs::write(project.join(".settings/org.eclipse.jdt.core.prefs"), "eclipse.preferences.version=1\norg.eclipse.jdt.core.compiler.codegen.targetPlatform=21\norg.eclipse.jdt.core.compiler.compliance=21\norg.eclipse.jdt.core.compiler.source=21\norg.eclipse.jdt.core.compiler.problem.unusedLocal=warning\n"))?;
        io(fs::write(root.join(SOURCE_PATH), SOURCE))?;
        let mut command = Command::new(binary);
        command
            .arg("--synthetic-root")
            .arg(&root)
            .arg("--java-distribution")
            .arg(&distribution)
            .arg("--allow-run");
        agent = Some(RawAgent::from_command(command));
        let agent = agent.as_mut().unwrap();
        let info = harness::metadata(
            agent
                .request_with_timeout(Operation::Hello, FEATURE_TIMEOUT)
                .map_err(|e| e.message)?,
        );
        require(
            LANGUAGE_SESSION_CAPABILITIES
                .iter()
                .all(|capability| !info.supports(capability)),
            "nonshipping profile changed normal Hello capability evidence",
        )?;
        for (session, mode, name) in [
            (1, SessionMode::Initial, INITIAL_DATA_DIR),
            (2, SessionMode::FreshData, RESTART_DATA_DIR),
            (3, SessionMode::ReusedData, RESTART_DATA_DIR),
        ] {
            stage.set(FailureStage::Setup);
            let mut record = SessionEvidence::new(session, mode);
            let data = root.join(name);
            let semantics = checked(|| {
                validate_fixture_layout(&root, &data)?;
                require(
                    data.exists() == (session == 3),
                    "fresh/reused data directory precondition failed",
                )?;
                io(fs::create_dir_all(&data))?;
                run_semantics(
                    agent,
                    &root,
                    &text(&java)?,
                    &args,
                    &data,
                    &mut observed,
                    &mut record,
                    &stage,
                )
            });
            if semantics.is_err() {
                failure_stage.get_or_insert(stage.get());
            }
            let stop = if agent.has_protocol_pipes() {
                checked(|| stop_session(agent, &root, &observed, &mut record, &stage))
            } else {
                Err("protocol pipes unavailable after bounded request failure".into())
            };
            let disk = unchanged(&root.join(SOURCE_PATH));
            record.source_unchanged = disk.is_ok();
            println!(
                "{}",
                serde_json::to_string(&record).expect("typed session evidence")
            );
            if let Err(error) = stop {
                failure_stage.get_or_insert(stage.get());
                cleanup_errors.push(error);
            }
            if let Err(error) = disk {
                failure_stage.get_or_insert(FailureStage::FixtureCleanup);
                cleanup_errors.push(error);
            }
            semantics?;
            if !cleanup_errors.is_empty() {
                break;
            }
            completed += 1;
        }
        Ok(())
    });
    if primary.is_err() {
        failure_stage.get_or_insert(stage.get());
    }
    // All cleanup assertions precede the Child/TempDir Drop backstops. Even a
    // request timeout keeps its pipe worker owned until explicit abort/reap/join.
    let mut agent_exit_zero = false;
    let mut agent_cleanup_complete = agent.is_none();
    if let Some(agent) = &mut agent {
        let close = checked(|| {
            agent.close_cleanly_with_timeout(EXIT_TIMEOUT);
            Ok(())
        });
        if let Err(error) = close {
            failure_stage.get_or_insert(FailureStage::AgentExit);
            cleanup_errors.push(error);
            let abort = checked(|| {
                agent.abort_and_wait_with_timeout(EXIT_TIMEOUT);
                Ok(())
            });
            if let Err(error) = abort {
                cleanup_errors.push(error);
            } else {
                agent_cleanup_complete = true;
            }
        } else {
            agent_exit_zero = true;
            agent_cleanup_complete = true;
        }
    }
    let observed_roots_exited = observed.iter().all(|root| root.exit_code().is_ok());
    if !observed_roots_exited {
        failure_stage.get_or_insert(FailureStage::RootExit);
        cleanup_errors.push("an observed Java root survived explicit agent cleanup".into());
    }
    let source_unchanged = fixture
        .as_ref()
        .is_some_and(|temp| unchanged(&temp.path().join(SOURCE_PATH)).is_ok());
    if fixture.is_some() && !source_unchanged {
        failure_stage.get_or_insert(FailureStage::FixtureCleanup);
        cleanup_errors.push("source bytes changed before fixture removal".into());
    }
    let synthetic_root_removed = match fixture.take() {
        Some(temp) if agent_cleanup_complete && observed_roots_exited => match temp.close() {
            Ok(()) => true,
            Err(error) => {
                failure_stage.get_or_insert(FailureStage::FixtureCleanup);
                cleanup_errors.push(error.to_string());
                false
            }
        },
        Some(temp) => {
            // Keep private files when teardown could not be proved. TempDir's
            // Drop must not race a surviving process or erase failure evidence.
            let _ = temp.keep();
            false
        }
        None => false,
    };
    let success = primary.is_ok()
        && cleanup_errors.is_empty()
        && completed == 3
        && agent_exit_zero
        && observed.len() == 3
        && observed_roots_exited
        && source_unchanged
        && synthetic_root_removed;
    println!(
        "{}",
        serde_json::to_string(&CleanupEvidence {
            kind: "windows_java_cleanup",
            sessions_completed: completed,
            agent_exit_zero,
            source_unchanged,
            observed_roots_exited,
            synthetic_root_removed,
            failure_stage: if success {
                FailureStage::None
            } else {
                failure_stage.unwrap_or(FailureStage::Setup)
            },
            primary_failed: primary.is_err(),
            cleanup_failed: !cleanup_errors.is_empty(),
            success,
        })
        .expect("typed cleanup evidence")
    );
    match primary {
        Err(primary) => Err(format!(
            "primary acceptance failure: {primary}; cleanup failures: {cleanup_errors:?}"
        )),
        Ok(()) if !success => Err(format!("acceptance cleanup failed: {cleanup_errors:?}")),
        Ok(()) => Ok(()),
    }
}
