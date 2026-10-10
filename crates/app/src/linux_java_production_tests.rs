//! Real Linux shipping-agent typed Java acceptance. All source is synthetic;
//! raw output stays in the caller's private scratch. No GUI, Maven, network
//! isolation, Windows handle, or escaped-descendant claims are made here.
use super::*;
use cedar_client::Client;
use cedar_protocol::{LanguageQueryKind, Operation, Payload};
use std::{
    fs,
    panic::{catch_unwind, AssertUnwindSafe},
    path::{Path, PathBuf},
    rc::Rc,
    sync::mpsc,
    thread,
    time::Instant,
};

const REQUEST: Duration = Duration::from_secs(75);
const STARTUP_REQUEST: Duration = Duration::from_secs(30);
const STARTUP: Duration = Duration::from_secs(75);
const RESTART: Duration = Duration::from_secs(180);
const REAP: Duration = Duration::from_secs(30);
const EXPECTED_CAPABILITIES: &[&str] = &[
    "git_changes",
    "git_diff",
    "git_status",
    "java_diagnostics_refresh",
    "language_change",
    "language_close",
    "language_document_symbols",
    "language_events",
    "language_format",
    "language_java_implementations",
    "language_open",
    "language_organize_java_imports",
    "language_query",
    "language_references",
    "language_resolve_completion",
    "language_resolve_uri",
    "language_start",
    "language_start_java",
    "language_start_java_begin",
    "language_start_java_cancel",
    "language_start_java_poll",
    "language_stop",
    "language_workspace_symbols",
    "list",
    "read",
    "run",
    "run_cancel",
    "run_poll",
    "run_start",
    "search",
    "write",
];

fn require(value: bool, message: &str) -> CheckResult<()> {
    if value {
        Ok(())
    } else {
        Err(message.into())
    }
}
fn io<T>(value: std::io::Result<T>) -> CheckResult<T> {
    value.map_err(|e| e.to_string())
}
fn checked<T>(action: impl FnOnce() -> CheckResult<T>) -> CheckResult<T> {
    catch_unwind(AssertUnwindSafe(action))
        .unwrap_or_else(|_| Err("acceptance assertion panicked".into()))
}
fn text(path: &Path) -> CheckResult<String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or("fixture path is not UTF-8".into())
}
fn ordinary_path(path: &Path) -> CheckResult<PathBuf> {
    require(path.is_absolute(), "fixture path must be absolute")?;
    io(path.canonicalize())
}
fn environment_path(name: &str) -> CheckResult<PathBuf> {
    ordinary_path(&PathBuf::from(
        std::env::var_os(name).ok_or("explicit prebuilt path missing")?,
    ))
}
fn unchanged(source: &Path) -> CheckResult<()> {
    require(
        io(fs::read(source))? == SOURCE.as_bytes(),
        "source file changed",
    )
}
fn admit_fixed(elapsed: Duration, limit: Duration, cost: Duration) -> CheckResult<()> {
    require(
        elapsed < limit && limit.saturating_sub(elapsed) >= cost,
        "operation lacks its complete unchanged deadline",
    )
}
fn check_fixed(elapsed: Duration, limit: Duration) -> CheckResult<()> {
    require(
        elapsed < limit,
        "operation returned after its fixed deadline",
    )
}

// An emergency process boundary is never a cleanup witness. The script has a
// separate 720-second runtime boundary; each phase retains its own earlier limit.
struct Watchdog(mpsc::Sender<()>);
impl Watchdog {
    fn new(limit: Duration) -> Self {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            if matches!(rx.recv_timeout(limit), Err(mpsc::RecvTimeoutError::Timeout)) {
                std::process::abort();
            }
        });
        Self(tx)
    }
}
impl Drop for Watchdog {
    fn drop(&mut self) {
        let _ = self.0.send(());
    }
}

#[derive(Clone)]
struct Clock {
    started: Instant,
    budget: Rc<idle::Budget>,
}
impl Clock {
    fn run<T>(&self, cost: Duration, action: impl FnOnce() -> CheckResult<T>) -> CheckResult<T> {
        self.budget.run(|| self.started.elapsed(), cost, action)
    }
}
struct AcceptanceClient {
    inner: Client,
    clock: Clock,
    restart: Option<Instant>,
}
impl AcceptanceClient {
    fn request(&mut self, operation: Operation) -> CheckResult<Payload> {
        let cost = match &operation {
            Operation::LanguageStartJavaBegin { .. }
            | Operation::LanguageStartJavaPoll { .. }
            | Operation::LanguageStartJavaCancel { .. }
            | Operation::Read { .. } => STARTUP_REQUEST,
            _ => REQUEST,
        };
        if let Some(started) = self.restart {
            admit_fixed(started.elapsed(), RESTART, cost)?;
            let result = self.inner.request(operation);
            check_fixed(started.elapsed(), RESTART)?;
            result
        } else {
            self.clock.run(cost, || self.inner.request(operation))
        }
    }
    fn handshake(&self) -> &Payload {
        self.inner.handshake()
    }
}
fn client_language(client: &mut AcceptanceClient, operation: Operation) -> CheckResult<Value> {
    match client.request(operation)? {
        Payload::Language { value } => Ok(value),
        _ => Err("typed Java operation returned a non-language response".into()),
    }
}
fn connect(
    slot: &mut Option<AcceptanceClient>,
    clock: &Clock,
    binary: &Path,
    root: &Path,
    trust: bool,
) -> CheckResult<()> {
    clock.run(STARTUP_REQUEST, || {
        *slot = Some(AcceptanceClient {
            inner: {
                let executable = std::env::current_exe().map_err(|e| e.to_string())?;
                let sibling = executable
                    .parent()
                    .ok_or("test executable has no parent")?
                    .join("cedar-agent");
                require(
                    sibling.canonicalize().map_err(|e| e.to_string())?
                        == binary.canonicalize().map_err(|e| e.to_string())?,
                    "selected acceptance agent is not the exact test executable sibling",
                )?;
                Client::connect(cedar_client::ConnectionSpec::BundledLinux {
                    root: root.to_owned(),
                    allow_run: trust,
                })?
            },
            clock: clock.clone(),
            restart: None,
        });
        Ok(())
    })
}
fn reap(slot: &mut Option<AcceptanceClient>, clock: &Clock) -> CheckResult<()> {
    if let Some(started) = slot.as_ref().and_then(|client| client.restart) {
        admit_fixed(started.elapsed(), RESTART, REAP)?;
        let result = slot
            .take()
            .ok_or("missing Client")?
            .inner
            .close_and_wait(REAP);
        check_fixed(started.elapsed(), RESTART)?;
        result
    } else {
        clock.run(REAP, || {
            slot.take()
                .ok_or("missing Client")?
                .inner
                .close_and_wait(REAP)
        })
    }
}
fn capabilities(client: &AcceptanceClient) -> CheckResult<()> {
    let Payload::Hello {
        protocol: 4,
        agent: Some(info),
        ..
    } = client.handshake()
    else {
        return Err("shipping agent metadata missing".into());
    };
    info.validate().map_err(|e| e.to_string())?;
    require(
        info.os == "linux"
            && info.arch == "x86_64"
            && info.schema == 1
            && info.version == env!("CARGO_PKG_VERSION")
            && info.capabilities.len() == 31
            && info.capability_groups == ["java_maven_dependencies_v1", "java_maven_leaf_v1"]
            && info
                .capabilities
                .iter()
                .map(String::as_str)
                .eq(EXPECTED_CAPABILITIES.iter().copied())
            && [
                "language_start_java_maven_begin",
                "language_maven_model",
                "language_maven_dependencies",
            ]
            .iter()
            .all(|capability| info.supports(capability)),
        "Linux shipping capability set was not exact",
    )
}
fn startup(
    client: &mut AcceptanceClient,
    operation: Operation,
    pending: &mut Option<u64>,
    ready: &mut bool,
    source: &Path,
    read_seen: &mut bool,
) -> CheckResult<Value> {
    let began = Instant::now();
    admit_fixed(began.elapsed(), STARTUP, STARTUP_REQUEST)?;
    let begin = client_language(client, operation)?;
    // Retain identity before any post-call validation so cleanup still owns it.
    *pending = begin["startup_id"].as_u64().filter(|id| *id != 0);
    check_fixed(began.elapsed(), STARTUP)?;
    let id = (*pending).ok_or("Begin omitted startup identity")?;
    require(
        begin["state"] == "starting",
        "Begin omitted Starting acknowledgement",
    )?;
    admit_fixed(began.elapsed(), STARTUP, STARTUP_REQUEST)?;
    let read = client.request(Operation::Read {
        path: SOURCE_PATH.into(),
    })?;
    check_fixed(began.elapsed(), STARTUP)?;
    require(
        matches!(read, Payload::File { path, text, .. } if path == SOURCE_PATH && text == SOURCE),
        "ordinary source Read failed while startup was pending",
    )?;
    unchanged(source)?;
    *read_seen = true;
    loop {
        admit_fixed(began.elapsed(), STARTUP, STARTUP_REQUEST)?;
        let response =
            client_language(client, Operation::LanguageStartJavaPoll { startup_id: id })?;
        if response["state"] == "ready" {
            *ready = true;
        }
        check_fixed(began.elapsed(), STARTUP)?;
        require(response["startup_id"] == id, "startup identity changed")?;
        match response["state"].as_str() {
            Some("ready") => {
                require(
                    response["language"]["started"] == true,
                    "Ready omitted started witness",
                )?;
                return Ok(response["language"].clone());
            }
            Some("starting") => thread::sleep(Duration::from_millis(50)),
            _ => return Err("startup failed or returned an invalid state".into()),
        }
    }
}
fn cancel_startup(client: &mut AcceptanceClient, id: u64) -> CheckResult<()> {
    let began = Instant::now();
    let mut operation = Operation::LanguageStartJavaCancel { startup_id: id };
    loop {
        admit_fixed(began.elapsed(), STARTUP, STARTUP_REQUEST)?;
        let status = client_language(client, operation)?;
        check_fixed(began.elapsed(), STARTUP)?;
        require(status["startup_id"] == id, "cancel identity mismatch")?;
        match status["state"].as_str() {
            Some("cancelled" | "failed") => {
                return require(
                    status["cleanup_verified"] == true,
                    "startup cleanup unverified",
                )
            }
            Some("cancelling") => {}
            _ => return Err("unexpected startup cancellation state".into()),
        }
        thread::sleep(Duration::from_millis(50));
        operation = Operation::LanguageStartJavaPoll { startup_id: id };
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum StopStatus {
    Graceful,
    Forced,
    Error,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum StopReason {
    RootExited,
    GraceExpired,
    Aborted,
    TransportFailure,
    WorkerPanicked,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum RootExit {
    Code { code: u16 },
    Signal { signal: u8 },
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LinuxStop {
    platform: String,
    status: StopStatus,
    reason: StopReason,
    root_exit: RootExit,
    cleanup_joined: bool,
    shutdown_response_received: bool,
    exit_frame_completed: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Stopped {
    stopped: bool,
    shutdown: LinuxStop,
}
fn parse_stop(value: Value) -> CheckResult<LinuxStop> {
    let result: Stopped =
        serde_json::from_value(value).map_err(|_| "malformed Linux Stop receipt")?;
    let s = result.shutdown;
    require(
        result.stopped && s.platform == "linux" && s.cleanup_joined,
        "Linux Stop did not establish owned cleanup",
    )?;
    require(
        match s.root_exit {
            RootExit::Code { code } => code <= 255,
            RootExit::Signal { signal } => (1..=64).contains(&signal),
        },
        "Linux Stop exit value was outside its platform range",
    )?;
    require(
        s.status != StopStatus::Graceful
            || (s.reason == StopReason::RootExited
                && matches!(s.root_exit, RootExit::Code { code: 0 })
                && s.shutdown_response_received
                && s.exit_frame_completed),
        "graceful Linux Stop lacked natural code-zero protocol completion",
    )?;
    Ok(s)
}

#[derive(Default, Serialize)]
struct Evidence {
    schema_version: u32,
    kind: &'static str,
    route: &'static str,
    status: &'static str,
    capability_count: u32,
    capability_group_count: u32,
    exact_capabilities: bool,
    maven_groups_advertised: bool,
    maven_trust_off_rejected: bool,
    trust_off_rejected: bool,
    trust_off_client_reaped: bool,
    async_begin: bool,
    read_while_starting: bool,
    ready: bool,
    selected_external_data: bool,
    semantic_diagnostics: bool,
    hover: bool,
    exact_definition: bool,
    real_completion: bool,
    deferred_import_resolve: bool,
    editor_apply_undo_redo: bool,
    versions_2_3_4_synced: bool,
    correction_acknowledged: bool,
    spontaneous_result: DiagnosticResult,
    spontaneous_success: bool,
    recovery_attempts: u32,
    recovery_result: CorrectionRecoveryResult,
    recovery_acknowledged: bool,
    recovery_witness: bool,
    recovery_unversioned: bool,
    recovery_budget_sufficient: bool,
    workflow_success: bool,
    refresh_supported: bool,
    refresh_acknowledged: bool,
    refresh_witness: bool,
    refresh_unversioned: bool,
    organize_imports: bool,
    organize_main_edits: u16,
    organize_ambiguity_edits: u16,
    organize_editor_stages: u16,
    implementations: bool,
    implementation_type_count: u16,
    implementation_method_count: u16,
    implementation_negative_count: u16,
    source_files_unchanged: bool,
    close_acknowledged: bool,
    initial_stop: Option<LinuxStop>,
    restart_stop: Option<LinuxStop>,
    same_agent_restart: bool,
    restart_ready: bool,
    restart_read_while_starting: bool,
    startup_cleanup_verified: bool,
    client_reaped: bool,
    synthetic_root_removed: bool,
    primary_failed: bool,
    cleanup_failed: bool,
    restart_failed: bool,
    failure_stage: FailureStage,
    primary_deadline_ms: u32,
    outer_deadline_ms: u32,
    cleanup_reserve_ms: u32,
    startup_deadline_ms: u32,
    startup_request_timeout_ms: u32,
    request_timeout_ms: u32,
    spontaneous_dispatch_window_ms: u32,
    recovery_admission_ms: u32,
    restart_deadline_ms: u32,
    client_reap_ms: u32,
    elapsed_ms: u32,
    main_elapsed_ms: u32,
    restart_elapsed_ms: u32,
    elapsed_saturated: bool,
    main_deadline_met: bool,
    restart_deadline_met: bool,
}
impl Evidence {
    fn record_stop(&mut self, stop: LinuxStop, restart: bool) {
        // Parsing already established backend-owned cleanup. A protocol or
        // transport quality error makes acceptance red without erasing that
        // ownership evidence or falsely calling cleanup unknown.
        let quality_failed = stop.status == StopStatus::Error;
        if restart {
            self.restart_failed |= quality_failed;
            self.restart_stop = Some(stop);
        } else {
            self.primary_failed |= quality_failed;
            self.initial_stop = Some(stop);
        }
        if quality_failed && matches!(self.failure_stage, FailureStage::None) {
            self.failure_stage = FailureStage::Stop;
        }
    }

    fn new() -> Self {
        Self {
            schema_version: 1,
            kind: "linux_java_production",
            route: "normal_agent_normal_client",
            status: "failed",
            primary_deadline_ms: 360_000,
            outer_deadline_ms: 480_000,
            cleanup_reserve_ms: 120_000,
            startup_deadline_ms: 75_000,
            startup_request_timeout_ms: 30_000,
            request_timeout_ms: 75_000,
            spontaneous_dispatch_window_ms: 60_000,
            recovery_admission_ms: 240_000,
            restart_deadline_ms: 180_000,
            client_reap_ms: 30_000,
            ..Self::default()
        }
    }
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

fn prepare_organize_files(
    root: &Path,
    created: &mut Vec<(PathBuf, &'static str)>,
) -> CheckResult<()> {
    use std::io::Write;
    for fixture in organize::FIXTURES {
        let path = root.join(fixture.path);
        io(fs::create_dir_all(
            path.parent().ok_or("synthetic source parent missing")?,
        ))?;
        let mut file = io(fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path))?;
        // Track partial writes too, so any failure is checked before root removal.
        created.push((path, fixture.source));
        io(file.write_all(fixture.source.as_bytes()))?;
    }
    Ok(())
}

fn organize_files_unchanged(created: &[(PathBuf, &'static str)]) -> bool {
    created.len() == organize::FIXTURES.len()
        && created
            .iter()
            .all(|(path, source)| fs::read(path).is_ok_and(|bytes| bytes == source.as_bytes()))
}

fn prepare_implementation_files(
    root: &Path,
    fixture: &mut Option<implementations::Fixture>,
    created: &mut Vec<(PathBuf, String)>,
) -> CheckResult<()> {
    use std::io::Write;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "synthetic implementation clock predates the epoch")?
        .as_nanos();
    *fixture = Some(implementations::Fixture::new(nonce));
    for source in fixture.as_ref().unwrap().sources() {
        let path = root.join(&source.path);
        io(fs::create_dir_all(
            path.parent()
                .ok_or("synthetic implementation parent missing")?,
        ))?;
        let mut file = io(fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path))?;
        // Retain expected bytes even when a partial write fails.
        created.push((path, source.text.clone()));
        io(file.write_all(source.text.as_bytes()))?;
    }
    Ok(())
}

fn implementation_files_unchanged(created: &[(PathBuf, String)]) -> bool {
    created.len() == 3
        && created
            .iter()
            .all(|(path, source)| fs::read(path).is_ok_and(|bytes| bytes == source.as_bytes()))
}

fn implementation_request(
    client: &mut AcceptanceClient,
    deadline: Instant,
    op: Operation,
) -> CheckResult<Payload> {
    require(
        Instant::now()
            .checked_add(REQUEST)
            .is_some_and(|end| end < deadline),
        "implementation witness lacks its existing request budget before the Linux primary deadline",
    )?;
    let result = client.request(op)?;
    require(
        Instant::now() < deadline,
        "implementation request exceeded the Linux primary deadline",
    )?;
    Ok(result)
}

fn implementation_language(
    client: &mut AcceptanceClient,
    deadline: Instant,
    op: Operation,
) -> CheckResult<Value> {
    match implementation_request(client, deadline, op)? {
        Payload::Language { value } => Ok(value),
        _ => Err("implementation request returned no language result".into()),
    }
}

fn production_implementations(
    client: &mut AcceptanceClient,
    initialized: &Value,
    root: &Path,
    fixture: &Option<implementations::Fixture>,
    created: &[(PathBuf, String)],
    deadline: Instant,
    receipt: &mut implementations::Evidence,
) -> CheckResult<()> {
    receipt.exercised = true;
    receipt.failure_stage = implementations::Stage::Support;
    let Payload::Hello {
        agent: Some(info), ..
    } = client.handshake()
    else {
        return Err("implementation agent metadata missing".into());
    };
    require(
        info.supports("language_java_implementations"),
        "implementation capability missing",
    )?;
    receipt.capability_supported = true;
    let provider = &initialized["initialize"]["capabilities"]["implementationProvider"];
    require(
        provider == true || provider.is_object(),
        "implementation provider missing",
    )?;
    receipt.provider_supported = true;
    let fixture = fixture
        .as_ref()
        .ok_or("synthetic implementation fixture missing")?;
    receipt.failure_stage = implementations::Stage::Open;
    let opened = implementation_language(
        client,
        deadline,
        Operation::LanguageOpen {
            path: fixture.interface.path.clone(),
            language_id: "java".into(),
            version: 1,
            text: fixture.interface.text.clone(),
        },
    )?;
    let expected_uri = |path: &str| -> CheckResult<String> {
        url::Url::from_file_path(ordinary_path(&root.join(path))?)
            .map(|uri| uri.to_string())
            .map_err(|_| "cannot encode synthetic implementation URI".into())
    };
    let interface_uri = expected_uri(&fixture.interface.path)?;
    require(
        opened["version"] == 1
            && opened["opened"]
                .as_str()
                .is_some_and(|uri| same_local_uri(uri, &interface_uri)),
        "implementation query source didOpen URI/version mismatch",
    )?;
    receipt.query_version_acknowledged = true;
    let concrete_uri = expected_uri(&fixture.concrete.path)?;
    let inherited_uri = expected_uri(&fixture.inherited.path)?;
    let operation = |cursor: completion::Position| Operation::LanguageJavaImplementations {
        path: fixture.interface.path.clone(),
        version: 1,
        line: cursor.line,
        character: cursor.character,
    };
    // All three sources existed before startup. Only the interface was opened;
    // there is one request per exact declaration after existing readiness,
    // without indexing retries, substitute searches or additional sleeps.
    receipt.failure_stage = implementations::Stage::TypeQuery;
    let types = implementation_language(client, deadline, operation(fixture.type_cursor()))?;
    receipt.type_result_count = implementations::result_count(&types)?;
    let target_uri = fixture.exact_types(&types, &concrete_uri, &inherited_uri)?;
    receipt.targets_unopened = true;
    receipt.exact_type_uris = true;
    receipt.exact_type_ranges = true;
    receipt.failure_stage = implementations::Stage::MethodQuery;
    let methods = implementation_language(client, deadline, operation(fixture.method_cursor()))?;
    receipt.method_result_count = implementations::result_count(&methods)?;
    fixture.exact_method(&methods, &concrete_uri)?;
    receipt.exact_method_uri = true;
    receipt.exact_method_range = true;
    receipt.inherited_method_absent = true;
    receipt.utf16_ranges_exact = true;
    receipt.failure_stage = implementations::Stage::NegativeQuery;
    let negative = implementation_language(client, deadline, operation(fixture.negative_cursor()))?;
    receipt.negative_result_count = implementations::result_count(&negative)?;
    require(
        workspace_types::empty_result(&negative),
        "unimplemented interface query was not empty",
    )?;
    receipt.negative_query_empty = true;
    receipt.failure_stage = implementations::Stage::Resolve;
    let resolved = implementation_language(
        client,
        deadline,
        Operation::LanguageResolveUri {
            uri: target_uri.clone(),
        },
    )?;
    require(
        resolved["path"] == fixture.concrete.path,
        "implementation target did not resolve to its owned source",
    )?;
    receipt.resolved_path_exact = true;
    receipt.failure_stage = implementations::Stage::Read;
    let Payload::File {
        path,
        text,
        revision,
    } = implementation_request(
        client,
        deadline,
        Operation::Read {
            path: fixture.concrete.path.clone(),
        },
    )?
    else {
        return Err("implementation ordinary Read returned no source file".into());
    };
    for name in [&fixture.concrete.name, &fixture.method] {
        let range = marker_range(&fixture.concrete.text, name);
        let (start, _) = completion::position_to_offsets(&text, range.start)?;
        let (end, _) = completion::position_to_offsets(&text, range.end)?;
        require(
            text.get(start..end) == Some(name.as_str()),
            "implementation UTF-16 selection missed its exact name",
        )?;
    }
    require(
        path == fixture.concrete.path
            && text == fixture.concrete.text
            && implementation_files_unchanged(created),
        "implementation ordinary Read/source mismatch",
    )?;
    receipt.ordinary_read_exact = true;
    receipt.failure_stage = implementations::Stage::Frontend;
    crate::CedarApp::java_implementations_navigation_acceptance(
        &fixture.interface.path,
        &fixture.interface.text,
        fixture.type_cursor(),
        types,
        &target_uri,
        &path,
        &text,
        &revision,
    )?;
    receipt.actual_frontend_navigation = true;
    receipt.full_selection_preserved = true;
    receipt.dirty_buffer_reused = true;
    receipt.undo_redo_preserved = true;
    receipt.retained_context_preserved = true;
    require(
        implementation_files_unchanged(created),
        "implementation navigation changed source bytes",
    )?;
    receipt.failure_stage = implementations::Stage::Close;
    implementation_language(
        client,
        deadline,
        Operation::LanguageClose {
            path: fixture.interface.path.clone(),
        },
    )?;
    require(
        receipt.semantics_passed(),
        "implementation semantic witness was incomplete",
    )?;
    receipt.failure_stage = implementations::Stage::None;
    Ok(())
}

fn organize_language(
    client: &mut AcceptanceClient,
    deadline: Instant,
    op: Operation,
) -> CheckResult<Value> {
    // No indexing retries or added wait window. This bounds when additional
    // work may begin; each request keeps the normal Client timeout and the
    // original Quick watchdog remains the hard total envelope.
    require(
        Instant::now() < deadline,
        "organize imports exhausted the Linux primary budget",
    )?;
    client_language(client, op)
}

fn organize_open(
    client: &mut AcceptanceClient,
    root: &Path,
    fixture: &organize::Fixture,
    deadline: Instant,
) -> CheckResult<String> {
    let value = organize_language(
        client,
        deadline,
        Operation::LanguageOpen {
            path: fixture.path.into(),
            language_id: "java".into(),
            version: 1,
            text: fixture.source.into(),
        },
    )?;
    let uri = value["opened"]
        .as_str()
        .ok_or("organize fixture didOpen omitted URI")?;
    let expected = url::Url::from_file_path(ordinary_path(&root.join(fixture.path))?)
        .map_err(|_| "cannot encode organize fixture URI")?;
    require(
        value["version"] == 1 && same_local_uri(uri, expected.as_str()),
        "organize fixture didOpen URI/version mismatch",
    )?;
    Ok(uri.to_owned())
}

fn organize_change(
    client: &mut AcceptanceClient,
    deadline: Instant,
    uri: &str,
    version: i32,
    text: &str,
) -> CheckResult<()> {
    let value = organize_language(
        client,
        deadline,
        Operation::LanguageChange {
            path: organize::MAIN.path.into(),
            version,
            text: text.into(),
        },
    )?;
    require(
        value["version"] == version
            && value["changed"]
                .as_str()
                .is_some_and(|actual| same_local_uri(actual, uri)),
        "organize fixture didChange URI/version mismatch",
    )
}

fn production_organize_imports(
    client: &mut AcceptanceClient,
    root: &Path,
    initialized: &Value,
    created: &[(PathBuf, &'static str)],
    deadline: Instant,
    receipt: &mut organize::Evidence,
) -> CheckResult<()> {
    receipt.exercised = true;
    receipt.failure_stage = organize::Stage::Support;
    let Payload::Hello {
        agent: Some(info), ..
    } = client.handshake()
    else {
        return Err("organize imports agent metadata missing".into());
    };
    require(
        info.supports("language_organize_java_imports")
            && initialized["initialize"]["cedar_java_organize_imports"] == true,
        "vetted Standard JDT organize imports support was not established",
    )?;
    receipt.supported = true;
    receipt.failure_stage = organize::Stage::IndexWitness;
    organize_open(client, root, &organize::INDEX, deadline)?;
    // Both ambiguous candidates must resolve independently to their exact
    // generated source declaration. Merely creating their files is not proof.
    for (fixture, qualified, name, witness) in [
        (
            &organize::LEFT,
            "cedarimportfixture.left.CedarSharedType",
            "CedarSharedType",
            &mut receipt.left_candidate_indexed,
        ),
        (
            &organize::RIGHT,
            "cedarimportfixture.right.CedarSharedType",
            "CedarSharedType",
            &mut receipt.right_candidate_indexed,
        ),
        (
            &organize::UNIQUE,
            "cedarimportfixture.unique.CedarUnsavedUnique",
            "CedarUnsavedUnique",
            &mut receipt.unsaved_type_indexed,
        ),
        (
            &organize::INDEPENDENT,
            "cedarimportfixture.unique.CedarIndependentUnique",
            "CedarIndependentUnique",
            &mut receipt.independent_type_indexed,
        ),
    ] {
        let byte = organize::INDEX
            .source
            .find(qualified)
            .ok_or("synthetic index marker missing")?
            + qualified.len()
            - name.len()
            + 2;
        let cursor = completion::byte_to_position(organize::INDEX.source, byte)?;
        let definition = organize_language(
            client,
            deadline,
            Operation::LanguageQuery {
                path: organize::INDEX.path.into(),
                line: cursor.line,
                character: cursor.character,
                kind: LanguageQueryKind::Definition,
            },
        )?;
        let expected = url::Url::from_file_path(ordinary_path(&root.join(fixture.path))?)
            .map_err(|_| "cannot encode indexed fixture URI")?;
        require(
            organize::exact_indexed_definition(&definition, expected.as_str(), fixture, name),
            "synthetic project type was not indexed at its exact declaration",
        )?;
        *witness = true;
    }
    receipt.failure_stage = organize::Stage::UnsavedSync;
    let uri = organize_open(client, root, &organize::MAIN, deadline)?;
    organize_change(client, deadline, &uri, 2, organize::DRAFT)?;
    receipt.unsaved_version_acknowledged = true;
    require(
        organize_files_unchanged(created),
        "organize fixtures changed on disk during synchronization",
    )?;
    receipt.failure_stage = organize::Stage::Organize;
    let value = organize_language(
        client,
        deadline,
        Operation::LanguageOrganizeJavaImports {
            path: organize::MAIN.path.into(),
            version: 2,
        },
    )?;
    let plan = organize::main_plan(&value)?;
    receipt.main_edit_count =
        u16::try_from(plan.edit_count).map_err(|_| "organize edit count exceeded bound")?;
    receipt.sorted_retained_imports = true;
    receipt.unused_import_removed = true;
    receipt.unsaved_unique_import_added = true;
    require(
        organize_files_unchanged(created),
        "organize command changed source files on disk",
    )?;
    crate::CedarApp::organize_imports_acceptance_transaction(
        organize::MAIN.path,
        organize::MAIN.source,
        organize::DRAFT,
        value,
        |phase, doc| {
            let (expected_phase, expected_text, edit_version, sync_version, stage) =
                match receipt.observed_editor_stages {
                    0 => (
                        "preview",
                        organize::DRAFT,
                        1,
                        None,
                        organize::Stage::Preview,
                    ),
                    1 => ("cancel", organize::DRAFT, 1, None, organize::Stage::Cancel),
                    2 => (
                        "apply",
                        plan.text.as_str(),
                        2,
                        Some(3),
                        organize::Stage::Apply,
                    ),
                    3 => ("undo", organize::DRAFT, 3, Some(4), organize::Stage::Undo),
                    4 => (
                        "redo",
                        plan.text.as_str(),
                        4,
                        Some(5),
                        organize::Stage::Redo,
                    ),
                    _ => return Err("unexpected repeated organize editor stage".into()),
                };
            receipt.failure_stage = stage;
            require(
                phase == expected_phase
                    && doc.path == organize::MAIN.path
                    && doc.text == expected_text
                    && doc.edit_version == edit_version
                    && doc.saved_text == organize::MAIN.source
                    && doc.revision.as_deref() == Some("r0")
                    && doc.dirty()
                    && organize_files_unchanged(created),
                "organize preview/cancel/history/disk invariant failed",
            )?;
            if let Some(version) = sync_version {
                organize_change(client, deadline, &uri, version, &doc.text)?;
                require(
                    organize_files_unchanged(created),
                    "organize editor synchronization changed disk",
                )?;
            }
            match phase {
                "preview" => receipt.preview_unchanged = true,
                "cancel" => receipt.cancel_unchanged = true,
                "apply" => receipt.actual_frontend_apply = true,
                "undo" => receipt.one_undo_exact = true,
                "redo" => {
                    receipt.one_redo_exact = true;
                    receipt.draft_versions_synced = true;
                }
                _ => unreachable!(),
            }
            receipt.observed_editor_stages += 1;
            Ok(())
        },
    )?;
    receipt.failure_stage = organize::Stage::Ambiguity;
    organize_open(client, root, &organize::AMBIGUOUS, deadline)?;
    let ambiguous = organize_language(
        client,
        deadline,
        Operation::LanguageOrganizeJavaImports {
            path: organize::AMBIGUOUS.path.into(),
            version: 1,
        },
    )?;
    let ambiguity = organize::ambiguity_plan(&ambiguous)?;
    receipt.ambiguity_edit_count =
        u16::try_from(ambiguity.edit_count).map_err(|_| "ambiguity edit count exceeded bound")?;
    receipt.ambiguous_candidates_skipped = true;
    receipt.independent_import_added = true;
    require(
        organize_files_unchanged(created),
        "ambiguous organize command changed disk",
    )?;
    receipt.failure_stage = organize::Stage::Close;
    for fixture in [&organize::INDEX, &organize::MAIN, &organize::AMBIGUOUS] {
        organize_language(
            client,
            deadline,
            Operation::LanguageClose {
                path: fixture.path.into(),
            },
        )?;
    }
    require(
        receipt.semantics_passed(),
        "organize semantic acceptance was incomplete",
    )?;
    receipt.failure_stage = organize::Stage::None;
    Ok(())
}

fn explicit_refresh(client: &mut AcceptanceClient, clock: &Clock, uri: &str) -> CheckResult<()> {
    require(
        clock.budget.remaining_primary(clock.started.elapsed()) >= idle::RECOVERY_ADMISSION,
        "explicit refresh needs its full request/witness/Close admission",
    )?;
    let requested = client_language(
        client,
        Operation::LanguageRefreshJavaDiagnostics {
            path: SOURCE_PATH.into(),
            version: 6,
        },
    )?;
    require(
        requested["version"] == 6
            && requested["notification_only"] == true
            && requested["diagnostics_refresh_requested"]
                .as_str()
                .is_some_and(|actual| same_local_uri(actual, uri)),
        "explicit refresh acknowledgement mismatch",
    )?;
    let dispatch = Instant::now() + Duration::from_secs(15);
    while Instant::now() < dispatch {
        let value = client_language(client, Operation::LanguageEvents)?;
        require(
            value["truncated"] == false,
            "refresh stream truncated or malformed",
        )?;
        let events = value["events"].as_array().ok_or("refresh events missing")?;
        let mut matched = false;
        for event in events {
            match event["type"].as_str() {
                Some("diagnostics") => {
                    language_results::Diagnostics::default().apply(&event["value"])?;
                    matched |= refresh_diagnostics_match(&event["value"], uri) == Some(true);
                }
                Some("notification" | "unsupported_server_request") => {}
                _ => return Err("refresh event stream closed, lagged, or malformed".into()),
            }
        }
        if matched {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err("explicit refresh exact unversioned witness timed out".into())
}

#[test]
#[ignore = "requires an explicitly verified Linux normal agent, pinned JDT 1.61.0, and JDK 21"]
fn real_linux_normal_agent_java_editor_acceptance() -> CheckResult<()> {
    println!();
    let started = Instant::now();
    let main_watchdog = Watchdog::new(idle::OUTER_BUDGET);
    let clock = Clock {
        started,
        budget: Rc::new(idle::Budget::default()),
    };
    let mut evidence = Evidence::new();
    let mut fixture: Option<tempfile::TempDir> = None;
    let mut client = None;
    let mut source_path = None;
    let mut selected = None;
    let mut startup_id = None;
    let mut server_ready = false;
    let mut organize_files = Vec::new();
    let mut organize_receipt = organize::Evidence::new();
    let mut implementation_fixture = None;
    let mut implementation_files = Vec::new();
    let mut implementation_receipt = implementations::Evidence::new();
    let mut correction = idle::Correction::default();
    let stage = Cell::new(FailureStage::Setup);
    let primary = checked(|| {
        let java = environment_path("CEDAR_JAVA")?;
        let distribution = environment_path("CEDAR_JDTLS_HOME")?;
        let binary = environment_path("CEDAR_AGENT_BIN")?;
        require(
            binary.is_file() && binary.file_name().is_some_and(|name| name == "cedar-agent"),
            "acceptance requires the exact normal cedar-agent binary",
        )?;
        require(
            distribution
                .file_name()
                .is_some_and(|name| name.to_string_lossy().contains('雪')),
            "distribution must use the generated Unicode runtime path",
        )?;
        fixture = Some(io(tempfile::Builder::new()
            .prefix("cedar Linux Java 雪 ")
            .tempdir())?);
        let base = ordinary_path(fixture.as_ref().unwrap().path())?;
        let root = base.join("workspace 雪");
        io(fs::create_dir(&root))?;
        prepare_project_files(&root)?;
        prepare_organize_files(&root, &mut organize_files)?;
        prepare_implementation_files(
            &root,
            &mut implementation_fixture,
            &mut implementation_files,
        )?;
        let data = base.join("external JDT data initial 雪");
        let restart_data = base.join("external JDT data restart 雪");
        io(fs::create_dir(&data))?;
        io(fs::create_dir(&restart_data))?;
        require(
            !data.starts_with(&root)
                && !restart_data.starts_with(&root)
                && !root.join(".cedar-windows-language-validation").exists()
                && !root.join(".cedar-windows-java-validation").exists()
                && !root.join(".cedar-windows-java-gc-diagnostic").exists(),
            "shipping acceptance must have distinct external data and no validation opt-in",
        )?;
        let source = root.join(SOURCE_PATH);
        source_path = Some(source.clone());
        selected = Some((java.clone(), distribution.clone(), restart_data));
        let operation = || -> CheckResult<Operation> {
            Ok(Operation::LanguageStartJavaBegin {
                java_executable: text(&java)?,
                distribution: text(&distribution)?,
                data_directory: text(&data)?,
            })
        };
        connect(&mut client, &clock, &binary, &root, false)?;
        capabilities(client.as_ref().unwrap())?;
        evidence.capability_count = 31;
        evidence.capability_group_count = 2;
        evidence.exact_capabilities = true;
        evidence.maven_groups_advertised = true;
        let denied = client
            .as_mut()
            .unwrap()
            .request(operation()?)
            .err()
            .ok_or("untrusted Java unexpectedly launched")?;
        require(
            denied.contains("run_disabled") && !data.join(".metadata").exists(),
            "untrusted Java was not rejected before launch",
        )?;
        evidence.trust_off_rejected = true;
        // The unchanged Client recognizes the two Maven groups. All three
        // routes must reach the backend's execution-trust gate; this basic Java
        // acceptance never performs a trusted Maven start or model request.
        for operation in [
            Operation::LanguageStartJavaMavenBegin {
                java_executable: String::new(),
                distribution: String::new(),
                data_directory: String::new(),
                local_repository: String::new(),
            },
            Operation::LanguageMavenModel,
            Operation::LanguageMavenDependencies {
                startup_id: 1,
                pom_sha256: "a".repeat(64),
            },
        ] {
            let error = client
                .as_mut()
                .unwrap()
                .request(operation)
                .err()
                .ok_or("untrusted Maven unexpectedly succeeded")?;
            require(
                error.starts_with("run_disabled:") && !data.join(".metadata").exists(),
                "Maven did not fail the backend trust gate before execution",
            )?;
            require(
                matches!(
                    client.as_mut().unwrap().request(Operation::Read {
                        path: SOURCE_PATH.into(),
                    })?,
                    Payload::File { text, .. } if text == SOURCE
                ),
                "file access did not survive untrusted Maven refusal",
            )?;
        }
        evidence.maven_trust_off_rejected = true;
        reap(&mut client, &clock)?;
        evidence.trust_off_client_reaped = true;
        unchanged(&source)?;
        connect(&mut client, &clock, &binary, &root, true)?;
        let client = client.as_mut().unwrap();
        capabilities(client)?;
        stage.set(FailureStage::Initialize);
        let initialized = startup(
            client,
            operation()?,
            &mut startup_id,
            &mut server_ready,
            &source,
            &mut evidence.read_while_starting,
        )?;
        evidence.async_begin = true;
        evidence.ready = true;
        require(
            data.join(".metadata").is_dir(),
            "selected external JDT data was unused",
        )?;
        evidence.selected_external_data = true;
        require(
            initialized["initialize"]["cedar_java_diagnostics_refresh"] == true,
            "pinned Standard JDT refresh support missing",
        )?;
        evidence.refresh_supported = true;
        unchanged(&source)?;
        stage.set(FailureStage::Open);
        let opened = client_language(
            client,
            Operation::LanguageOpen {
                path: SOURCE_PATH.into(),
                language_id: "java".into(),
                version: 1,
                text: SOURCE.into(),
            },
        )?;
        let uri = opened["opened"]
            .as_str()
            .ok_or("didOpen omitted URI")?
            .to_owned();
        let expected = url::Url::from_file_path(&source).map_err(|_| "cannot encode source URI")?;
        require(
            opened["version"] == 1 && same_local_uri(&uri, expected.as_str()),
            "didOpen identity mismatch",
        )?;
        stage.set(FailureStage::Diagnostics);
        idle::diagnostics(
            &clock.budget,
            &mut DiagnosticEvidence::new(1, DiagnosticPhase::Initial),
            &uri,
            |op| client_language(client, op),
            || started.elapsed(),
            thread::sleep,
        )?;
        evidence.semantic_diagnostics = true;
        unchanged(&source)?;
        let cursor = completion::byte_to_position(
            SOURCE,
            SOURCE.rfind("greeting").ok_or("source reference missing")? + 3,
        )?;
        let query = |kind| Operation::LanguageQuery {
            path: SOURCE_PATH.into(),
            line: cursor.line,
            character: cursor.character,
            kind,
        };
        stage.set(FailureStage::Hover);
        let hover = client_language(client, query(LanguageQueryKind::Hover))?;
        require(
            hover_has_source_variable(&hover),
            "hover missed the source String variable",
        )?;
        evidence.hover = true;
        stage.set(FailureStage::Definition);
        let definition = client_language(client, query(LanguageQueryKind::Definition))?;
        let target = exact_definition(&definition, &uri)?;
        let resolved_uri = client_language(client, Operation::LanguageResolveUri { uri: target })?;
        require(
            resolved_uri["path"] == SOURCE_PATH,
            "definition resolved outside the exact source",
        )?;
        evidence.exact_definition = true;
        stage.set(FailureStage::Completion);
        let response = client_language(client, query(LanguageQueryKind::Completion))?;
        let results = completion::parse_completion_result(&response)?;
        require(
            results.candidates.iter().any(|item| {
                item.label.starts_with("greeting") && item.item["textEdit"].is_object()
            }),
            "completion omitted source variable",
        )?;
        let candidate = results
            .candidates
            .into_iter()
            .find(|item| item.label.starts_with("GregorianCalendar"))
            .ok_or("completion omitted GregorianCalendar")?;
        require(
            candidate.disabled_reason.is_none(),
            "frontend disabled real completion",
        )?;
        let original = candidate.item;
        require(
            original["textEdit"].is_object()
                && !original["data"].is_null()
                && original
                    .get("additionalTextEdits")
                    .is_none_or(|v| v.is_null() || v.as_array().is_some_and(Vec::is_empty)),
            "completion import was not deferred",
        )?;
        evidence.real_completion = true;
        stage.set(FailureStage::Resolve);
        let resolved = client_language(
            client,
            Operation::LanguageResolveCompletion {
                item: original.clone(),
            },
        )?;
        crate::language_ui::validate_resolved_identity(&original, &resolved)?;
        let edits = resolved["additionalTextEdits"]
            .as_array()
            .ok_or("resolve omitted import")?;
        require(
            edits.len() == 1
                && edits[0]["newText"]
                    .as_str()
                    .is_some_and(|text| text.contains("import java.util.GregorianCalendar;")),
            "resolve omitted exact deferred import",
        )?;
        evidence.deferred_import_resolve = true;
        editor_transaction(original, resolved, &stage, |version, document| {
            unchanged(&source)?;
            let changed = client_language(
                client,
                Operation::LanguageChange {
                    path: document.path.clone(),
                    version,
                    text: document.text.clone(),
                },
            )?;
            require(
                changed["version"] == version
                    && changed["changed"]
                        .as_str()
                        .is_some_and(|actual| same_local_uri(actual, &uri)),
                "editor synchronization identity mismatch",
            )?;
            unchanged(&source)
        })?;
        evidence.editor_apply_undo_redo = true;
        evidence.versions_2_3_4_synced = true;
        stage.set(FailureStage::Correction);
        let changed = client_language(
            client,
            Operation::LanguageChange {
                path: SOURCE_PATH.into(),
                version: 5,
                text: corrected_source(),
            },
        )?;
        require(
            changed["version"] == 5
                && changed["changed"]
                    .as_str()
                    .is_some_and(|actual| same_local_uri(actual, &uri)),
            "correction identity mismatch",
        )?;
        evidence.correction_acknowledged = true;
        correction.run(
            &clock.budget,
            &uri,
            |op| client_language(client, op),
            || started.elapsed(),
            thread::sleep,
        )?;
        // A recovered workflow retains its original spontaneous failure in the
        // receipt. Only one exact, admitted typed recovery can restore workflow success.
        require(
            correction.accepted(),
            "correction workflow did not establish an exact witness",
        )?;
        unchanged(&source)?;
        let changed = client_language(
            client,
            Operation::LanguageChange {
                path: SOURCE_PATH.into(),
                version: 6,
                text: refreshed_source(),
            },
        )?;
        require(
            changed["version"] == 6
                && changed["changed"]
                    .as_str()
                    .is_some_and(|actual| same_local_uri(actual, &uri)),
            "refresh draft identity mismatch",
        )?;
        explicit_refresh(client, &clock, &uri)?;
        evidence.refresh_acknowledged = true;
        evidence.refresh_witness = true;
        evidence.refresh_unversioned = true;
        stage.set(FailureStage::Apply);
        production_organize_imports(
            client,
            &root,
            &initialized,
            &organize_files,
            started + idle::PRIMARY_BUDGET,
            &mut organize_receipt,
        )?;
        evidence.organize_imports = organize_receipt.semantics_passed();
        stage.set(FailureStage::Definition);
        production_implementations(
            client,
            &initialized,
            &root,
            &implementation_fixture,
            &implementation_files,
            started + idle::PRIMARY_BUDGET,
            &mut implementation_receipt,
        )?;
        evidence.implementations = implementation_receipt.semantics_passed();
        unchanged(&source)?;
        stage.set(FailureStage::Close);
        let closed = client_language(
            client,
            Operation::LanguageClose {
                path: SOURCE_PATH.into(),
            },
        )?;
        require(
            closed["closed"]
                .as_str()
                .is_some_and(|actual| same_local_uri(actual, &uri)),
            "Close omitted the exact source URI",
        )?;
        evidence.close_acknowledged = true;
        Ok(())
    });
    evidence.primary_failed = primary.is_err();
    evidence.failure_stage = if primary.is_err() {
        stage.get()
    } else {
        FailureStage::None
    };
    evidence.spontaneous_result = correction.spontaneous_result;
    evidence.spontaneous_success = correction.spontaneous_success;
    evidence.recovery_attempts = correction.recovery_attempts;
    evidence.recovery_result = correction.recovery_result;
    evidence.recovery_acknowledged = correction.recovery_acknowledged;
    evidence.recovery_witness = correction.recovery_witness;
    evidence.recovery_unversioned = correction.recovery_unversioned;
    evidence.recovery_budget_sufficient = correction.recovery_budget_sufficient;
    evidence.workflow_success = correction.accepted();
    evidence.organize_main_edits = organize_receipt.main_edit_count;
    evidence.organize_ambiguity_edits = organize_receipt.ambiguity_edit_count;
    evidence.organize_editor_stages = u16::from(organize_receipt.observed_editor_stages);
    evidence.implementation_type_count = implementation_receipt.type_result_count;
    evidence.implementation_method_count = implementation_receipt.method_result_count;
    evidence.implementation_negative_count = implementation_receipt.negative_result_count;
    clock.budget.begin_cleanup(started.elapsed());
    let cleanup = checked(|| {
        if let Some(client) = client.as_mut() {
            if server_ready {
                let stop = parse_stop(client_language(client, Operation::LanguageStop)?)?;
                evidence.record_stop(stop, false);
                server_ready = false;
                startup_id = None;
            } else if let Some(id) = startup_id {
                cancel_startup(client, id)?;
                evidence.startup_cleanup_verified = true;
                startup_id = None;
            }
        }
        Ok(())
    });
    evidence.cleanup_failed = cleanup.is_err();
    evidence.main_elapsed_ms = started.elapsed().as_millis().min(u32::MAX as u128) as u32;
    evidence.main_deadline_met = clock.budget.deadlines_met(started.elapsed());
    if !clock.budget.deadlines_met(started.elapsed()) {
        evidence.primary_failed = true;
    }
    // One more owned startup occurs only after the first session's successful
    // Stop. Retaining this Client proves reuse of the same agent connection.
    if !evidence.primary_failed && !evidence.cleanup_failed {
        drop(main_watchdog);
        let restart_started = Instant::now();
        let _restart_watchdog = Watchdog::new(RESTART);
        client.as_mut().unwrap().restart = Some(restart_started);
        let restart = checked(|| {
            let (java, distribution, data) =
                selected.as_ref().ok_or("restart selection missing")?;
            let connection = client.as_mut().ok_or("restart Client missing")?;
            let operation = Operation::LanguageStartJavaBegin {
                java_executable: text(java)?,
                distribution: text(distribution)?,
                data_directory: text(data)?,
            };
            let initialized = startup(
                connection,
                operation,
                &mut startup_id,
                &mut server_ready,
                source_path.as_ref().unwrap(),
                &mut evidence.restart_read_while_starting,
            )?;
            require(
                initialized["started"] == true && data.join(".metadata").is_dir(),
                "restart was not Ready in selected data",
            )?;
            evidence.same_agent_restart = true;
            evidence.restart_ready = true;
            Ok(())
        });
        evidence.restart_failed = restart.is_err();
        let restart_cleanup = checked(|| {
            if let Some(connection) = client.as_mut() {
                if server_ready {
                    let stop = parse_stop(client_language(connection, Operation::LanguageStop)?)?;
                    evidence.record_stop(stop, true);
                    server_ready = false;
                } else if let Some(id) = startup_id {
                    cancel_startup(connection, id)?;
                    evidence.startup_cleanup_verified = true;
                }
            }
            Ok(())
        });
        evidence.cleanup_failed |= restart_cleanup.is_err();
        // Reap remains independent when Stop/cancel failed.
        let reaped = checked(|| reap(&mut client, &clock));
        evidence.client_reaped = reaped.is_ok();
        evidence.cleanup_failed |= reaped.is_err();
        evidence.restart_elapsed_ms =
            restart_started.elapsed().as_millis().min(u32::MAX as u128) as u32;
        evidence.restart_deadline_met = check_fixed(restart_started.elapsed(), RESTART).is_ok();
        evidence.restart_failed |= check_fixed(restart_started.elapsed(), RESTART).is_err();
    } else {
        if client.is_some() {
            let reaped = checked(|| reap(&mut client, &clock));
            evidence.client_reaped = reaped.is_ok();
            evidence.cleanup_failed |= reaped.is_err();
        }
        drop(main_watchdog);
    }
    evidence.source_files_unchanged = source_path
        .as_ref()
        .is_some_and(|source| unchanged(source).is_ok())
        && organize_files_unchanged(&organize_files)
        && implementation_files_unchanged(&implementation_files);
    if let Some(fixture) = fixture.take() {
        let path = fixture.path().to_owned();
        evidence.synthetic_root_removed = fixture.close().is_ok() && !path.exists();
    }
    evidence.cleanup_failed |= !evidence.source_files_unchanged || !evidence.synthetic_root_removed;
    evidence.elapsed_saturated = started.elapsed().as_millis() > u32::MAX as u128;
    evidence.elapsed_ms = started.elapsed().as_millis().min(u32::MAX as u128) as u32;
    if !evidence.primary_failed
        && !evidence.cleanup_failed
        && !evidence.restart_failed
        && evidence.client_reaped
        && evidence.initial_stop.is_some()
        && evidence.restart_stop.is_some()
        && evidence.same_agent_restart
        && evidence.workflow_success
        && evidence.refresh_unversioned
        && evidence.main_deadline_met
        && evidence.restart_deadline_met
        && !evidence.elapsed_saturated
        && evidence.organize_imports
        && evidence.implementations
    {
        evidence.status = "success";
    }
    println!(
        "\n{}",
        serde_json::to_string(&evidence).map_err(|_| "receipt serialization failed")?
    );
    require(
        evidence.status == "success",
        "Linux typed Java acceptance failed; inspect sanitized receipt",
    )
}

#[test]
fn linux_java_clock_admission_and_late_returns_keep_fixed_deadlines() {
    for (limit, cost) in [
        (STARTUP, STARTUP_REQUEST),
        (idle::PRIMARY_BUDGET, REQUEST),
        (idle::OUTER_BUDGET, REQUEST),
        (RESTART, REAP),
    ] {
        assert!(admit_fixed(limit - cost, limit, cost).is_ok());
        assert!(admit_fixed(limit - cost + Duration::from_nanos(1), limit, cost).is_err());
        assert!(admit_fixed(limit, limit, Duration::ZERO).is_err());
        assert!(check_fixed(limit - Duration::from_nanos(1), limit).is_ok());
        assert!(check_fixed(limit, limit).is_err());
    }
    // A fresh poll cannot renew the original startup observation deadline.
    let elapsed = STARTUP_REQUEST + Duration::from_secs(16);
    assert!(admit_fixed(elapsed, STARTUP, STARTUP_REQUEST).is_err());
}

#[test]
fn linux_java_stop_predicate_rejects_windows_unknown_and_inconsistent_receipts() {
    let good = json!({"stopped":true,"shutdown":{"platform":"linux","status":"graceful",
        "reason":"root_exited","root_exit":{"kind":"code","code":0},"cleanup_joined":true,
        "shutdown_response_received":true,"exit_frame_completed":true}});
    assert!(parse_stop(good.clone()).is_ok());
    for (pointer, value) in [
        ("/stopped", json!(false)),
        ("/shutdown/platform", json!("windows")),
        ("/shutdown/status", json!("unverified")),
        ("/shutdown/cleanup_joined", json!(false)),
        ("/shutdown/reason", json!("grace_expired")),
        ("/shutdown/root_exit/code", json!(1)),
        ("/shutdown/root_exit/code", json!(256)),
        ("/shutdown/shutdown_response_received", json!(false)),
        ("/shutdown/exit_frame_completed", json!(false)),
        ("/shutdown/root_exit", json!({"kind":"signal","signal":9})),
    ] {
        let mut wrong = good.clone();
        *wrong.pointer_mut(pointer).unwrap() = value;
        assert!(parse_stop(wrong).is_err(), "{pointer}");
    }
    let mut forced = good.clone();
    forced["shutdown"]["status"] = json!("forced");
    forced["shutdown"]["reason"] = json!("grace_expired");
    for signal in [1, 9, 64] {
        forced["shutdown"]["root_exit"] = json!({"kind":"signal","signal":signal});
        assert!(parse_stop(forced.clone()).is_ok());
    }
    for exit in [
        json!({"kind":"signal","signal":0}),
        json!({"kind":"signal","signal":65}),
        json!({"kind":"code","code":-1}),
        json!({"kind":"code","code":256}),
        json!({"kind":"code","code":0,"signal":9}),
        json!({"kind":"unknown"}),
    ] {
        forced["shutdown"]["root_exit"] = exit;
        assert!(parse_stop(forced.clone()).is_err());
    }
    for pointer in ["/shutdown", "/shutdown/root_exit"] {
        let mut extra = good.clone();
        extra.pointer_mut(pointer).unwrap()["private"] = json!("private sentinel");
        assert!(parse_stop(extra).is_err());
    }
    let mut windows = good.clone();
    windows["shutdown"]
        .as_object_mut()
        .unwrap()
        .remove("root_exit");
    windows["shutdown"]["root_exit_code"] = json!(0);
    assert!(parse_stop(windows).is_err());
}

#[test]
fn linux_java_stop_quality_error_retains_proven_ownership_without_cleanup_uncertainty() {
    let value = json!({"stopped":true,"shutdown":{"platform":"linux","status":"error",
        "reason":"transport_failure","root_exit":{"kind":"code","code":7},"cleanup_joined":true,
        "shutdown_response_received":false,"exit_frame_completed":false}});
    let parsed = parse_stop(value).unwrap();
    let mut initial = Evidence::new();
    initial.record_stop(parsed.clone(), false);
    assert!(initial.primary_failed);
    assert!(!initial.cleanup_failed);
    assert!(!initial.restart_failed);
    assert!(matches!(initial.failure_stage, FailureStage::Stop));
    assert_eq!(initial.initial_stop.unwrap().status, StopStatus::Error);
    let mut restart = Evidence::new();
    restart.record_stop(parsed, true);
    assert!(!restart.primary_failed);
    assert!(!restart.cleanup_failed);
    assert!(restart.restart_failed);
    assert!(matches!(restart.failure_stage, FailureStage::Stop));
    assert_eq!(restart.restart_stop.unwrap().status, StopStatus::Error);
}
