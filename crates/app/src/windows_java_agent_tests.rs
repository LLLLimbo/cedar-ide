//! Opt-in real Windows cedar_agent::serve → Workspace → JDT → editor acceptance.
//! Run serially with explicit verified prebuilt paths. Cross-compiling is not a
//! native runtime pass and this fixture grants no shipping Windows capability.
use super::*;
use cedar_protocol::{LanguageQueryKind, Operation, Payload, JAVA_LANGUAGE_SESSION_CAPABILITIES};
use std::{
    fs,
    io::Read,
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
        GetExitCodeProcess, GetProcessId, GetProcessTimes, OpenProcess, WaitForSingleObject,
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

fn checked<T>(action: impl FnOnce() -> CheckResult<T>) -> CheckResult<T> {
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
        require(created != 0, "invalid Java root creation time")?;
        let observed = Self::open_current(session, pid)?;
        require(
            observed.created == created,
            "PID creation time did not match the started Java root",
        )?;
        Ok(observed)
    }
    fn open_current(session: u32, pid: u32) -> CheckResult<Self> {
        require(pid != 0, "invalid Java root identity")?;
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
        let observed = Self {
            session,
            pid,
            created: actual,
            handle,
        };
        require(
            observed.live()?,
            "Java root was not live when observation handle opened",
        )?;
        Ok(observed)
    }
    fn live(&self) -> CheckResult<bool> {
        // SAFETY: Held query handle; verify this same native identity before each
        // live assertion, without opening another handle or gaining kill rights.
        require(
            unsafe { GetProcessId(self.handle.as_raw_handle()) } == self.pid,
            "retained process PID changed",
        )?;
        let mut times = [FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        }; 4];
        // SAFETY: Four distinct writable FILETIME outputs and a held query handle.
        let ok = unsafe {
            GetProcessTimes(
                self.handle.as_raw_handle(),
                &mut times[0],
                &mut times[1],
                &mut times[2],
                &mut times[3],
            )
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let created =
            (u64::from(times[0].dwHighDateTime) << 32) | u64::from(times[0].dwLowDateTime);
        require(
            created == self.created,
            "retained process creation time changed",
        )?;
        // SAFETY: Held observation-only handle, zero-time wait.
        match unsafe { WaitForSingleObject(self.handle.as_raw_handle(), 0) } {
            WAIT_TIMEOUT => Ok(true),
            WAIT_OBJECT_0 => Ok(false),
            _ => Err(std::io::Error::last_os_error().to_string()),
        }
    }
    fn exit_code(&self) -> CheckResult<u32> {
        self.exit_code_with_timeout(1500)
    }
    fn exit_code_with_timeout(&self, milliseconds: u32) -> CheckResult<u32> {
        // SAFETY: Same retained SYNCHRONIZE handle, bounded wait, no PID reuse.
        match unsafe { WaitForSingleObject(self.handle.as_raw_handle(), milliseconds) } {
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

struct JavaInputs {
    distribution: PathBuf,
    java: PathBuf,
    agent: PathBuf,
    task: PathBuf,
    args: Vec<String>,
}
impl JavaInputs {
    fn read() -> CheckResult<Self> {
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
        let agent = environment_path("CEDAR_AGENT_LANGUAGE_VALIDATION_BIN")?;
        let task = environment_path("CEDAR_WINPROCESS_FIXTURE_BIN")?;
        require(
            java.is_file()
                && java
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("java.exe"))
                && text(&java)?.is_ascii(),
            "Java must be an identity-checked ordinary absolute ASCII java.exe",
        )?;
        for binary in [&agent, &task] {
            require(
                binary.is_file()
                    && binary
                        .extension()
                        .is_some_and(|extension| extension == "exe"),
                "acceptance fixtures must be exact prebuilt executables",
            )?;
        }
        let args = launch_arguments(&distribution)?;
        Ok(Self {
            distribution,
            java,
            agent,
            task,
            args,
        })
    }
    fn spawn_agent(&self, root: &Path) -> RawAgent {
        let mut command = Command::new(&self.agent);
        command
            .arg("--synthetic-root")
            .arg(root)
            .arg("--java-distribution")
            .arg(&self.distribution)
            .arg("--allow-run");
        RawAgent::from_command(command)
    }
}

fn source_hover(agent: &mut RawAgent) -> CheckResult<()> {
    let byte = SOURCE.rfind("greeting").ok_or("fixture reference")? + 2;
    let cursor = completion::byte_to_position(SOURCE, byte)?;
    let value = language(
        agent,
        Operation::LanguageQuery {
            path: SOURCE_PATH.into(),
            line: cursor.line,
            character: cursor.character,
            kind: LanguageQueryKind::Hover,
        },
        FEATURE_TIMEOUT,
    )?;
    require(
        hover_has_source_variable(&value),
        "real hover did not identify String greeting",
    )
}

fn task_identity(path: &Path) -> CheckResult<(u32, u64)> {
    let metadata = io(fs::symlink_metadata(path))?;
    require(
        metadata.is_file() && !metadata.file_type().is_symlink() && metadata.len() <= 64,
        "task readiness must be a bounded ordinary file",
    )?;
    let mut bytes = Vec::new();
    io(io(fs::File::open(path))?.take(65).read_to_end(&mut bytes))?;
    parse_task_identity(&bytes)
}

struct KnownTask {
    id: Option<u64>,
    lifetime: PathBuf,
    ready: PathBuf,
    expired: PathBuf,
    observed: Option<RootObservation>,
    identity_verified: bool,
    lock_verified: bool,
    cancelled: bool,
    exit_code: Option<u32>,
    lock_released: bool,
    cap_not_reached: bool,
}
impl KnownTask {
    fn lock_is_held(&self) -> CheckResult<()> {
        let error = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.lifetime)
            .err()
            .ok_or("task lifetime lock was released while task should be live")?;
        require(
            error.raw_os_error() == Some(32),
            "task lifetime lock did not return the expected sharing violation",
        )
    }
    fn assert_live(&self) -> CheckResult<()> {
        let observed = self
            .observed
            .as_ref()
            .ok_or("missing retained task observation")?;
        require(
            task_identity(&self.ready)? == (observed.pid, observed.created),
            "task readiness identity changed",
        )?;
        require(observed.live()?, "retained task root is not live")?;
        self.lock_is_held()?;
        require(
            !self.expired.exists(),
            "task fixture reached its finite safety cap",
        )
    }
    fn observe_exit(&mut self) -> CheckResult<()> {
        self.observe_exit_with_timeout(1500)
    }
    fn observe_exit_with_timeout(&mut self, milliseconds: u32) -> CheckResult<()> {
        let code = self
            .observed
            .as_ref()
            .ok_or("missing task observation during cleanup")?
            .exit_code_with_timeout(milliseconds)?;
        self.exit_code = Some(code);
        self.cap_not_reached = code != 124 && !self.expired.exists();
        require(self.cap_not_reached, "task fixture reached its safety cap")?;
        require(code != 125, "task fixture failed independently")?;
        io(fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.lifetime))?;
        self.lock_released = true;
        Ok(())
    }
}

struct TaskAcceptance {
    program: Option<PathBuf>,
    tasks: Vec<KnownTask>,
    record: ConcurrencyEvidence,
    stage: OwnershipStage,
}
impl TaskAcceptance {
    fn new() -> Self {
        Self {
            program: None,
            tasks: Vec::new(),
            record: ConcurrencyEvidence::new(),
            stage: OwnershipStage::Setup,
        }
    }
    fn checked<T>(&mut self, action: impl FnOnce(&mut Self) -> CheckResult<T>) -> CheckResult<T> {
        let result = checked(|| action(self));
        if result.is_err() && self.record.failure_stage == OwnershipStage::None {
            self.record.failure_stage = self.stage;
        }
        result
    }
    fn start(&mut self, agent: &mut RawAgent, root: &Path, name: &str) -> CheckResult<usize> {
        self.stage = OwnershipStage::TaskStart;
        let directory = root.join(name);
        io(fs::create_dir(&directory))?;
        let index = self.tasks.len();
        self.tasks.push(KnownTask {
            id: None,
            lifetime: directory.join("task.lock"),
            ready: directory.join("task.ready"),
            expired: directory.join("task.expired"),
            observed: None,
            identity_verified: false,
            lock_verified: false,
            cancelled: false,
            exit_code: None,
            lock_released: false,
            cap_not_reached: false,
        });
        let task = &mut self.tasks[index];
        let response = agent
            .request(Operation::RunStart {
                program: text(self.program.as_ref().ok_or("missing exact task fixture")?)?,
                args: vec![
                    "java-validation-live".into(),
                    text(&task.lifetime)?,
                    text(&task.ready)?,
                    text(&task.expired)?,
                ],
                // Beyond the fixture cap, so a manager timeout cannot fake cleanup.
                timeout_secs: 300,
            })
            .map_err(|e| e.to_string())?;
        task.id = Some(harness::task(response).id);
        let deadline = Instant::now() + Duration::from_secs(3);
        while !task.ready.is_file() {
            require(
                Instant::now() < deadline,
                "task did not publish bounded readiness",
            )?;
            thread::sleep(Duration::from_millis(5));
        }
        self.stage = OwnershipStage::TaskIdentity;
        let (pid, created) = task_identity(&task.ready)?;
        task.observed = Some(RootObservation::open(0, pid, created)?);
        task.assert_live()?;
        task.identity_verified = true;
        task.lock_verified = true;
        self.assert_running(agent, index)?;
        Ok(index)
    }
    fn assert_running(&self, agent: &mut RawAgent, index: usize) -> CheckResult<()> {
        let task = &self.tasks[index];
        task.assert_live()?;
        let snapshot = harness::task(
            agent
                .request(Operation::RunPoll {
                    task_id: task.id.ok_or("missing task id")?,
                })
                .map_err(|e| e.to_string())?,
        );
        require(
            snapshot.state == cedar_tasks::TaskState::Running,
            "known task is not Running",
        )
    }
    fn cancel(&mut self, agent: &mut RawAgent, index: usize) -> CheckResult<()> {
        self.stage = OwnershipStage::TaskCancel;
        let task = &mut self.tasks[index];
        let id = task.id.ok_or("missing task id for explicit cleanup")?;
        agent
            .request(Operation::RunCancel { task_id: id })
            .map_err(|e| e.to_string())?;
        let terminal = agent.terminal(id, "real Java coexistence task cancellation");
        require(
            terminal.state == cedar_tasks::TaskState::Cancelled,
            "task did not terminate as Cancelled",
        )?;
        self.stage = OwnershipStage::TaskExit;
        task.observe_exit()?;
        agent.stable(&terminal);
        task.cancelled = true;
        Ok(())
    }
    fn cancel_pending(&mut self, agent: &mut RawAgent) -> Vec<String> {
        let mut errors = Vec::new();
        for index in 0..self.tasks.len() {
            if self.tasks[index].id.is_some() && !self.tasks[index].cancelled {
                if let Err(error) = self.checked(|owned| owned.cancel(agent, index)) {
                    errors.push(error);
                }
            }
        }
        errors
    }
    fn observe_all_exits(&mut self) -> Vec<String> {
        let mut errors = Vec::new();
        for task in &mut self.tasks {
            if let Err(error) = task.observe_exit() {
                errors.push(error);
            }
        }
        if !errors.is_empty() && self.record.failure_stage == OwnershipStage::None {
            self.record.failure_stage = OwnershipStage::TaskExit;
        }
        errors
    }
    fn finalize(
        &mut self,
        source_unchanged: bool,
        primary_failed: bool,
        cleanup_failed: bool,
        fixture_removed: bool,
    ) {
        self.record.tasks_started =
            self.tasks.iter().filter(|task| task.id.is_some()).count() as u32;
        self.record.tasks_completed = self
            .tasks
            .iter()
            .filter(|task| task.cancelled && task.exit_code.is_some())
            .count() as u32;
        let exactly_two = self.tasks.len() == 2;
        self.record.task_identities_verified =
            exactly_two && self.tasks.iter().all(|task| task.identity_verified);
        self.record.task_locks_verified =
            exactly_two && self.tasks.iter().all(|task| task.lock_verified);
        self.record.tasks_exited =
            exactly_two && self.tasks.iter().all(|task| task.exit_code.is_some());
        self.record.task_locks_released =
            exactly_two && self.tasks.iter().all(|task| task.lock_released);
        self.record.task_caps_not_reached =
            exactly_two && self.tasks.iter().all(|task| task.cap_not_reached);
        self.record.source_unchanged = source_unchanged;
        self.record.primary_failed = primary_failed;
        self.record.cleanup_failed = cleanup_failed;
        self.record.success = self.record.tasks_started == 2
            && self.record.tasks_completed == 2
            && self.record.language_stop_preserved_task
            && self.record.task_cancel_preserved_java
            && self.record.hover_after_cancel
            && self.record.task_identities_verified
            && self.record.task_locks_verified
            && self.record.tasks_exited
            && self.record.task_locks_released
            && self.record.task_caps_not_reached
            && source_unchanged
            && fixture_removed
            && !primary_failed
            && !cleanup_failed;
        if !self.record.success && self.record.failure_stage == OwnershipStage::None {
            self.record.failure_stage = OwnershipStage::Setup;
        }
    }
}

fn prepare_project(root: &Path) -> CheckResult<()> {
    io(fs::write(
        root.join(".cedar-windows-language-validation"),
        "cedar-windows-language-validation-v1\n",
    ))?;
    io(fs::write(
        root.join(".cedar-windows-java-validation"),
        "cedar-windows-java-validation-v1\n",
    ))?;
    prepare_project_files(root)
}

fn prepare_project_files(root: &Path) -> CheckResult<()> {
    let project = root.join(PROJECT_DIR);
    io(fs::create_dir_all(project.join("src")))?;
    io(fs::create_dir_all(project.join(".settings")))?;
    io(fs::write(project.join(".project"), "<?xml version=\"1.0\"?><projectDescription><name>cedar-editor-smoke</name><projects/><buildSpec><buildCommand><name>org.eclipse.jdt.core.javabuilder</name><arguments/></buildCommand></buildSpec><natures><nature>org.eclipse.jdt.core.javanature</nature></natures></projectDescription>"))?;
    io(fs::write(project.join(".classpath"), "<?xml version=\"1.0\"?><classpath><classpathentry kind=\"src\" path=\"src\"/><classpathentry kind=\"con\" path=\"org.eclipse.jdt.launching.JRE_CONTAINER\"/><classpathentry kind=\"output\" path=\"bin\"/></classpath>"))?;
    io(fs::write(project.join(".settings/org.eclipse.jdt.core.prefs"), "eclipse.preferences.version=1\norg.eclipse.jdt.core.compiler.codegen.targetPlatform=21\norg.eclipse.jdt.core.compiler.compliance=21\norg.eclipse.jdt.core.compiler.source=21\norg.eclipse.jdt.core.compiler.problem.unusedLocal=warning\n"))?;
    io(fs::write(root.join(SOURCE_PATH), SOURCE))?;
    Ok(())
}

fn await_diagnostics(
    agent: &mut RawAgent,
    uri: &str,
    session: u32,
    phase: DiagnosticPhase,
) -> Result<(), DiagnosticWaitFailure> {
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
    outcome.map_err(|message| DiagnosticWaitFailure {
        message,
        result: receipt.result,
    })
}

fn recover_correction(
    agent: &mut RawAgent,
    uri: &str,
    failure: DiagnosticWaitFailure,
    record: &mut SessionEvidence,
    acceptance_deadline: Instant,
) -> CheckResult<()> {
    let started = Instant::now();
    let available = acceptance_deadline.saturating_duration_since(started);
    let mut receipt = CorrectionRecoveryEvidence::new(record.session, failure.result, available);
    let outcome = checked(|| {
        receipt.run(
            uri,
            available,
            |operation, timeout| language(agent, operation, timeout),
            || started.elapsed(),
            thread::sleep,
        )
    });
    receipt.finish_elapsed(started.elapsed().as_millis());
    record.record_correction_recovery(&receipt);
    println!(
        "{}",
        serde_json::to_string(&receipt).expect("typed correction recovery evidence")
    );
    outcome
        .map_err(|recovery| format!("{}; explicit refresh recovery: {recovery}", failure.message))
}

#[allow(clippy::too_many_arguments)]
fn start_java_session(
    agent: &mut RawAgent,
    root: &Path,
    java: &str,
    args: &[String],
    data: &Path,
    observed: &mut Vec<RootObservation>,
    record: &mut SessionEvidence,
    stage: &Cell<FailureStage>,
) -> CheckResult<()> {
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
        initialized["initialize"]["cedar_java_diagnostics_refresh"] == true,
        "vetted Java validation session did not authorize typed diagnostics refresh",
    )?;
    require(
        data.join(".metadata").is_dir(),
        "JDT did not use the intended Unicode data directory",
    )?;
    Ok(())
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
    ownership: &mut TaskAcceptance,
    acceptance_deadline: Instant,
) -> CheckResult<()> {
    let source = root.join(SOURCE_PATH);
    start_java_session(agent, root, java, args, data, observed, record, stage)?;
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
    await_diagnostics(agent, &uri, record.session, DiagnosticPhase::Initial)
        .map_err(|failure| failure.message)?;
    record.exact_diagnostics = true;
    unchanged(&source)?;
    if record.session == 2 {
        ownership.checked(|owned| {
            let task = owned.start(agent, root, "task cancel 雪")?;
            owned.stage = OwnershipStage::JavaSurvival;
            require(
                observed.last().ok_or("missing Java observer")?.live()?,
                "Java stopped before task cancellation",
            )?;
            owned.cancel(agent, task)?;
            owned.stage = OwnershipStage::JavaSurvival;
            require(
                observed.last().ok_or("missing Java observer")?.live()?,
                "task cancellation stopped Java",
            )?;
            owned.record.task_cancel_preserved_java = true;
            owned.stage = OwnershipStage::Hover;
            source_hover(agent)?;
            require(
                observed.last().ok_or("missing Java observer")?.live()?,
                "Java stopped during post-cancel hover",
            )?;
            owned.record.hover_after_cancel = true;
            unchanged(&source)
        })?;
    }

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
    match await_diagnostics(agent, &uri, record.session, DiagnosticPhase::Correction) {
        Ok(()) => record.correction_diagnostics = true,
        Err(failure) if failure.result == DiagnosticResult::Timeout => {
            // The original receipt is already printed and remains a timeout.
            // Keep the identical version-5 draft; do not replay, change or save.
            recover_correction(agent, &uri, failure, record, acceptance_deadline)?;
        }
        Err(failure) => return Err(failure.message),
    }
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
    record.semantic_checks_passed = record.correction_diagnostics;
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
    let acceptance_deadline = Instant::now() + Duration::from_secs(360);
    let _watchdog = Watchdog::start_with_timeout(Duration::from_secs(360));
    let mut fixture: Option<tempfile::TempDir> = None;
    let mut agent: Option<RawAgent> = None;
    let mut observed = Vec::new();
    let mut completed = 0;
    let mut spontaneous_sessions = 0;
    let mut ownership = TaskAcceptance::new();
    let mut cleanup_errors = Vec::new();
    let stage = Cell::new(FailureStage::Setup);
    let mut failure_stage = None;
    let primary = checked(|| {
        let inputs = JavaInputs::read()?;
        ownership.program = Some(inputs.task.clone());
        fixture = Some(io(tempfile::Builder::new()
            .prefix("cedar agent editor 雪 ")
            .tempdir())?);
        let root = ordinary_path(fixture.as_ref().unwrap().path())?;
        prepare_project(&root)?;
        agent = Some(inputs.spawn_agent(&root));
        let agent = agent.as_mut().unwrap();
        let info = harness::metadata(
            agent
                .request_with_timeout(Operation::Hello, FEATURE_TIMEOUT)
                .map_err(|e| e.message)?,
        );
        require(
            !info.supports("language_start")
                && JAVA_LANGUAGE_SESSION_CAPABILITIES
                    .iter()
                    .all(|capability| info.supports(capability)),
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
            let mut semantics = checked(|| {
                validate_fixture_layout(&root, &data)?;
                require(
                    data.exists() == (session == 3),
                    "fresh/reused data directory precondition failed",
                )?;
                io(fs::create_dir_all(&data))?;
                run_semantics(
                    agent,
                    &root,
                    &text(&inputs.java)?,
                    &inputs.args,
                    &data,
                    &mut observed,
                    &mut record,
                    &stage,
                    &mut ownership,
                    acceptance_deadline,
                )
            });
            if semantics.is_err() {
                failure_stage.get_or_insert(stage.get());
            }
            let task_for_stop = if semantics.is_ok() && session == 1 {
                match ownership.checked(|owned| owned.start(agent, &root, "task stop 雪")) {
                    Ok(index) => Some(index),
                    Err(error) => {
                        semantics = Err(error);
                        None
                    }
                }
            } else {
                None
            };
            if task_for_stop.is_some() {
                ownership.stage = OwnershipStage::LanguageStop;
            }
            let stop = if agent.has_protocol_pipes() {
                checked(|| stop_session(agent, &root, &observed, &mut record, &stage))
            } else {
                Err("protocol pipes unavailable after bounded request failure".into())
            };
            if let Some(index) = task_for_stop {
                if stop.is_ok() {
                    let coexistence = ownership.checked(|owned| {
                        owned.stage = OwnershipStage::TaskSurvival;
                        owned.assert_running(agent, index)?;
                        owned.record.language_stop_preserved_task = true;
                        owned.cancel(agent, index)
                    });
                    if let Err(error) = coexistence {
                        semantics = Err(error);
                    }
                } else if ownership.record.failure_stage == OwnershipStage::None {
                    ownership.record.failure_stage = OwnershipStage::LanguageStop;
                }
            }
            let disk = unchanged(&root.join(SOURCE_PATH));
            record.source_unchanged = disk.is_ok();
            record.workflow_success = semantics.is_ok() && stop.is_ok() && disk.is_ok();
            if record.semantic_checks_passed {
                spontaneous_sessions += 1;
            }
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
    if let Some(agent) = &mut agent {
        if agent.has_protocol_pipes() {
            cleanup_errors.extend(ownership.cancel_pending(agent));
        }
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
    let task_exit_errors = ownership.observe_all_exits();
    let observed_tasks_exited = task_exit_errors.is_empty();
    cleanup_errors.extend(task_exit_errors);
    let source_unchanged = fixture
        .as_ref()
        .is_some_and(|temp| unchanged(&temp.path().join(SOURCE_PATH)).is_ok());
    if fixture.is_some() && !source_unchanged {
        failure_stage.get_or_insert(FailureStage::FixtureCleanup);
        cleanup_errors.push("source bytes changed before fixture removal".into());
    }
    let synthetic_root_removed = match fixture.take() {
        Some(temp) if agent_cleanup_complete && observed_roots_exited && observed_tasks_exited => {
            match temp.close() {
                Ok(()) => true,
                Err(error) => {
                    failure_stage.get_or_insert(FailureStage::FixtureCleanup);
                    cleanup_errors.push(error.to_string());
                    false
                }
            }
        }
        Some(temp) => {
            // Keep private files when teardown could not be proved. TempDir's
            // Drop must not race a surviving process or erase failure evidence.
            let _ = temp.keep();
            false
        }
        None => false,
    };
    ownership.finalize(
        source_unchanged,
        primary.is_err(),
        !cleanup_errors.is_empty(),
        synthetic_root_removed,
    );
    println!(
        "{}",
        serde_json::to_string(&ownership.record).expect("typed concurrency evidence")
    );
    let success = ownership.record.success
        && primary.is_ok()
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
            spontaneous_success: success && spontaneous_sessions == 3,
            workflow_success: success,
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

#[test]
#[ignore = "requires native Windows, verified JDT/Java and exact prebuilt agent/task fixtures; run serially"]
fn real_windows_agent_java_forced_owner_cleanup() -> CheckResult<()> {
    println!();
    let _watchdog = Watchdog::start_with_timeout(Duration::from_secs(180));
    let started = Instant::now();
    let mut fixture: Option<tempfile::TempDir> = None;
    let mut agent: Option<RawAgent> = None;
    let mut observed = Vec::new();
    let mut ownership = TaskAcceptance::new();
    let mut record = ForcedCleanupEvidence::new();
    let mut java_record = SessionEvidence::new(1, SessionMode::Initial);
    let java_stage = Cell::new(FailureStage::Setup);
    let stage = Cell::new(OwnershipStage::Setup);
    let mut hover_verified = false;
    let mut cleanup_errors = Vec::new();
    let primary = checked(|| {
        let inputs = JavaInputs::read()?;
        ownership.program = Some(inputs.task.clone());
        fixture = Some(io(tempfile::Builder::new()
            .prefix("cedar forced Java 雪 ")
            .tempdir())?);
        let root = ordinary_path(fixture.as_ref().unwrap().path())?;
        prepare_project(&root)?;
        let data = root.join(INITIAL_DATA_DIR);
        validate_fixture_layout(&root, &data)?;
        io(fs::create_dir(&data))?;
        agent = Some(inputs.spawn_agent(&root));
        let agent = agent.as_mut().unwrap();
        stage.set(OwnershipStage::Initialize);
        start_java_session(
            agent,
            &root,
            &text(&inputs.java)?,
            &inputs.args,
            &data,
            &mut observed,
            &mut java_record,
            &java_stage,
        )?;
        unchanged(&root.join(SOURCE_PATH))?;
        stage.set(OwnershipStage::Open);
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
        let expected_uri = url::Url::from_file_path(ordinary_path(&root.join(SOURCE_PATH))?)
            .map_err(|_| "cannot encode forced fixture source URI")?;
        require(
            opened["version"] == 1
                && opened["opened"]
                    .as_str()
                    .is_some_and(|uri| same_local_uri(uri, expected_uri.as_str())),
            "forced fixture didOpen acknowledgement mismatch",
        )?;
        unchanged(&root.join(SOURCE_PATH))?;
        stage.set(OwnershipStage::Hover);
        source_hover(agent)?;
        hover_verified = true;
        unchanged(&root.join(SOURCE_PATH))?;
        stage.set(OwnershipStage::TaskStart);
        let task = ownership.checked(|owned| owned.start(agent, &root, "task forced 雪"))?;
        stage.set(OwnershipStage::TaskIdentity);
        ownership.assert_running(agent, task)?;
        record.task_observed_live = true;
        stage.set(OwnershipStage::JavaSurvival);
        require(observed[0].live()?, "Java exited before forced owner death")?;
        record.java_observed_live = true;
        stage.set(OwnershipStage::Source);
        unchanged(&root.join(SOURCE_PATH))?;
        // Last live checks retain the original handles and revalidate the task's
        // published identity and exclusive lock immediately before owner death.
        ownership.assert_running(agent, task)?;
        require(
            observed[0].live()?,
            "Java was not live at forced owner death",
        )?;
        stage.set(OwnershipStage::OwnerDeath);
        agent.inject_failure("forced-agent-death");
        record.owner_death_injected = true;
        stage.set(OwnershipStage::AgentExit);
        let status = agent.wait_exit_with_timeout("forced real Java agent death", EXIT_TIMEOUT);
        record.agent_exit_observed = true;
        record.agent_exit_nonzero = !status.success();
        require(
            !status.success(),
            "forced owner death unexpectedly reported success",
        )?;
        stage.set(OwnershipStage::JavaExit);
        record.java_exit_code = Some(observed[0].exit_code_with_timeout(3000)?);
        record.java_exit_observed = true;
        stage.set(OwnershipStage::TaskExit);
        ownership.tasks[task].observe_exit_with_timeout(3000)?;
        stage.set(OwnershipStage::Source);
        unchanged(&root.join(SOURCE_PATH))?;
        Ok(())
    });
    if primary.is_err() {
        record.failure_stage = if ownership.record.failure_stage == OwnershipStage::None {
            stage.get()
        } else {
            ownership.record.failure_stage
        };
    }
    // Expected forced death is deliberately distinct from graceful Stop. For
    // any earlier failure, first close this agent's stdin, then explicitly abort
    // only its exact Child if bounded EOF cleanup fails. Never kill a looked-up PID.
    let mut agent_cleanup_complete = agent.is_none();
    if let Some(agent) = &mut agent {
        if record.agent_exit_observed {
            agent_cleanup_complete = true;
        } else {
            let cleanup = checked(|| {
                if record.owner_death_injected {
                    let status =
                        agent.wait_exit_with_timeout("forced agent failure cleanup", EXIT_TIMEOUT);
                    record.agent_exit_observed = true;
                    record.agent_exit_nonzero = !status.success();
                } else {
                    agent.close_cleanly_with_timeout(EXIT_TIMEOUT);
                }
                Ok(())
            });
            match cleanup {
                Ok(()) => agent_cleanup_complete = true,
                Err(error) => {
                    cleanup_errors.push(error);
                    if record.failure_stage == OwnershipStage::None {
                        record.failure_stage = OwnershipStage::AgentExit;
                    }
                    match checked(|| {
                        agent.abort_and_wait_with_timeout(EXIT_TIMEOUT);
                        Ok(())
                    }) {
                        Ok(()) => agent_cleanup_complete = true,
                        Err(error) => cleanup_errors.push(error),
                    }
                }
            }
        }
    }
    let mut java_exited = true;
    for process in &observed {
        match process.exit_code_with_timeout(3000) {
            Ok(code) => {
                record.java_exit_observed = true;
                record.java_exit_code = Some(code);
            }
            Err(error) => {
                java_exited = false;
                cleanup_errors.push(error);
            }
        }
    }
    if !java_exited && record.failure_stage == OwnershipStage::None {
        record.failure_stage = OwnershipStage::JavaExit;
    }
    let task_errors = ownership.observe_all_exits();
    let tasks_exited = task_errors.is_empty();
    if !tasks_exited && record.failure_stage == OwnershipStage::None {
        record.failure_stage = OwnershipStage::TaskExit;
    }
    cleanup_errors.extend(task_errors);
    record.java_identity_verified = java_record.root_identity_verified;
    record.java_observed_live |= java_record.root_observed_live;
    if let Some(task) = ownership.tasks.first() {
        record.task_identity_verified = task.identity_verified;
        record.task_lock_verified = task.lock_verified;
        record.task_exit_observed = task.exit_code.is_some();
        record.task_exit_code = task.exit_code;
        record.task_lock_released = task.lock_released;
        record.task_cap_not_reached = task.cap_not_reached;
    }
    record.source_unchanged = fixture
        .as_ref()
        .is_some_and(|temp| unchanged(&temp.path().join(SOURCE_PATH)).is_ok());
    if fixture.is_some() && !record.source_unchanged {
        cleanup_errors.push("source bytes changed before forced fixture removal".into());
        if record.failure_stage == OwnershipStage::None {
            record.failure_stage = OwnershipStage::Source;
        }
    }
    record.synthetic_root_removed = match fixture.take() {
        Some(temp) if agent_cleanup_complete && java_exited && tasks_exited => match temp.close() {
            Ok(()) => true,
            Err(error) => {
                cleanup_errors.push(error.to_string());
                if record.failure_stage == OwnershipStage::None {
                    record.failure_stage = OwnershipStage::FixtureCleanup;
                }
                false
            }
        },
        Some(temp) => {
            let _ = temp.keep();
            false
        }
        None => false,
    };
    record.primary_failed = primary.is_err();
    record.cleanup_failed = !cleanup_errors.is_empty();
    record.success = primary.is_ok()
        && cleanup_errors.is_empty()
        && hover_verified
        && observed.len() == 1
        && ownership.tasks.len() == 1
        && record.java_observed_live
        && record.java_identity_verified
        && record.task_observed_live
        && record.task_identity_verified
        && record.task_lock_verified
        && record.owner_death_injected
        && record.agent_exit_observed
        && record.agent_exit_nonzero
        && record.java_exit_observed
        && record.task_exit_observed
        && record.task_lock_released
        && record.task_cap_not_reached
        && record.source_unchanged
        && record.synthetic_root_removed;
    let elapsed = started.elapsed().as_millis();
    record.elapsed_ms = elapsed.min(300_000) as u32;
    record.elapsed_saturated = elapsed > 300_000;
    println!(
        "{}",
        serde_json::to_string(&record).expect("typed forced cleanup evidence")
    );
    match primary {
        Err(error) => Err(format!(
            "forced owner primary failure: {error}; cleanup failures: {cleanup_errors:?}"
        )),
        Ok(()) if !record.success => {
            Err(format!("forced owner cleanup failure: {cleanup_errors:?}"))
        }
        Ok(()) => Ok(()),
    }
}

#[path = "windows_java_production_tests.rs"]
mod production;
