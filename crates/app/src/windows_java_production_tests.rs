//! Shipping agent and separate nonshipping GC-control acceptance through the
//! same capability-enforcing Client. Neither route includes GUI acceptance.
use super::*;
use crate::java_language::{JavaRootExit, JavaStopOutcome, StopReason, StopStatus};
use cedar_client::Client;
use windows_sys::Win32::System::Threading::QueryFullProcessImageNameW;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ObservationProfile {
    Quick,
    IdleCorrection,
    ResourceBaseline,
    GcDiagnostic,
}

impl ObservationProfile {
    fn observes_resources(self) -> bool {
        matches!(self, Self::ResourceBaseline | Self::GcDiagnostic)
    }

    fn agent_selection(self) -> (&'static str, &'static str) {
        match self {
            Self::Quick | Self::IdleCorrection | Self::ResourceBaseline => {
                ("CEDAR_AGENT_BIN", "cedar-agent.exe")
            }
            Self::GcDiagnostic => (
                "CEDAR_GC_DIAGNOSTIC_AGENT_BIN",
                "cedar-agent-java-gc-diagnostic.exe",
            ),
        }
    }

    fn evidence(self) -> ProductionEvidence {
        match self {
            Self::Quick | Self::ResourceBaseline => ProductionEvidence::new(),
            Self::IdleCorrection => ProductionEvidence {
                kind: "windows_java_idle_correction",
                ..ProductionEvidence::new()
            },
            Self::GcDiagnostic => ProductionEvidence {
                kind: "windows_java_gc_control",
                route: "diagnostic_agent_normal_client",
                ..ProductionEvidence::new()
            },
        }
    }
}

// This adapter is test-only. Old profiles forward every operation unchanged;
// Idle conservatively admits the existing Client request deadline and checks
// again on return. No mutable dereference or transport-timeout override exists.
#[derive(Clone)]
struct AcceptanceClock {
    started: Instant,
    idle: Option<std::rc::Rc<idle::Budget>>,
}
impl AcceptanceClock {
    fn run<T>(&self, cost: Duration, operation: impl FnOnce() -> CheckResult<T>) -> CheckResult<T> {
        match &self.idle {
            Some(budget) => budget.run(|| self.started.elapsed(), cost, operation),
            None => operation(),
        }
    }
}
struct AcceptanceClient {
    inner: Client,
    clock: AcceptanceClock,
}
impl AcceptanceClient {
    fn request(&mut self, op: Operation) -> CheckResult<Payload> {
        self.clock
            .run(idle::REQUEST_BUDGET, || self.inner.request(op))
    }
    fn handshake(&self) -> &Payload {
        self.inner.handshake()
    }
    fn is_connected(&self) -> bool {
        self.inner.is_connected()
    }
}
fn connect_client(
    slot: &mut Option<AcceptanceClient>,
    clock: &AcceptanceClock,
    binary: &Path,
    root: &Path,
    allow_run: bool,
) -> CheckResult<()> {
    // Spawn includes the initial Hello. Retain the returned connection before
    // the post-call clock check so a late Hello still reaches owned cleanup.
    clock.run(idle::REQUEST_BUDGET, || {
        *slot = Some(AcceptanceClient {
            inner: Client::spawn_agent(binary, root, allow_run)?,
            clock: clock.clone(),
        });
        Ok(())
    })
}
fn reap_client(
    slot: &mut Option<AcceptanceClient>,
    clock: &AcceptanceClock,
    cleanup: bool,
) -> CheckResult<()> {
    let cost = if cleanup {
        idle::CLIENT_REAP_BUDGET
    } else {
        idle::REQUEST_BUDGET
    };
    // Leave ownership in the slot if primary admission refuses this reap; the
    // independent cleanup phase must still be able to reap the owned client.
    clock.run(cost, || {
        slot.take()
            .ok_or("normal Client missing during reap")?
            .inner
            .close_and_wait(EXIT_TIMEOUT)
    })
}
fn profile_phase(profile: ObservationProfile, started: Instant, phase: ResourcePhase) -> bool {
    profile != ObservationProfile::IdleCorrection && resource_phase(started, phase)
}

const GC_WORKSPACE_MARKER: &str = ".cedar-windows-java-gc-diagnostic";
const GC_WORKSPACE_MARKER_CONTENTS: &[u8] =
    b"cedar-windows-java-gc-diagnostic-v1\nsynthetic-data-only\n";
const GC_DISTRIBUTION_MARKER: &str = ".cedar-windows-java-gc-diagnostic-distribution";
const GC_DISTRIBUTION_MARKER_CONTENTS: &[u8] =
    b"cedar-windows-java-gc-diagnostic-distribution-v1\nsynthetic-data-only\n";
const GC_SELECTION_FILE: &str = ".cedar-java-gc-selection-private.json";

fn require_absent(path: &Path) -> CheckResult<()> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
        Ok(_) => Err("GC control requires a new, absent owned path".into()),
    }
}

fn verify_gc_logs_absent(distribution: &Path) -> CheckResult<()> {
    // Match the host's fixed logging namespace, including every rotation slot
    // and malformed/preexisting names. No PID discovery or outside scan.
    for (index, entry) in io(fs::read_dir(distribution))?.enumerate() {
        require(index < 4096, "GC distribution entry bound exceeded")?;
        let name = io(entry)?.file_name();
        let name = name
            .to_str()
            .ok_or("GC distribution filename is not UTF-8")?;
        require(
            !name.to_ascii_lowercase().starts_with("cedar-gc"),
            "GC logs must be absent before this owned Java launch",
        )?;
    }
    Ok(())
}

fn prepare_gc_selection(distribution: &Path, selection: &Path) -> CheckResult<fs::File> {
    use std::os::windows::fs::MetadataExt;
    require(
        selection.is_absolute()
            && selection
                .file_name()
                .is_some_and(|name| name == GC_SELECTION_FILE)
            && !selection
                .components()
                .any(|part| matches!(part, Component::ParentDir))
            && selection.parent().is_some_and(|parent| {
                ordinary_path(parent).is_ok_and(|parent| parent == distribution)
            }),
        "GC selection must be the fixed private file in the verified distribution",
    )?;
    let marker = distribution.join(GC_DISTRIBUTION_MARKER);
    let metadata = io(fs::symlink_metadata(&marker))?;
    require(
        metadata.is_file()
            && !metadata.file_type().is_symlink()
            && metadata.file_attributes() & 0x400 == 0
            && metadata.len() == GC_DISTRIBUTION_MARKER_CONTENTS.len() as u64,
        "GC distribution marker must be a bounded ordinary file",
    )?;
    let mut bytes = Vec::new();
    io(io(fs::File::open(marker))?
        .take(GC_DISTRIBUTION_MARKER_CONTENTS.len() as u64 + 1)
        .read_to_end(&mut bytes))?;
    require(
        bytes == GC_DISTRIBUTION_MARKER_CONTENTS,
        "GC distribution marker content mismatch",
    )?;
    require_absent(selection)?;
    verify_gc_logs_absent(distribution)?;
    // Reserve the fresh path before launch; keep the handle private and leave
    // the file empty on failed/forced shutdown. A partial write is not proof.
    io(fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(selection))
}

#[derive(serde::Serialize)]
struct GcControlSelection {
    schema_version: u8,
    kind: &'static str,
    pid: u32,
    creation_time_100ns_since_1601: u64,
    owned_jvm_identity_verified: bool,
    root_image_verified: bool,
    log_files_absent_before_launch: bool,
    root_handle_signaled: bool,
    root_exit_code: u32,
    natural_shutdown_verified: bool,
}

fn finalize_gc_selection(file: &mut fs::File, process: &RootObservation) -> CheckResult<()> {
    use std::io::Write;
    require(
        process.pid != 0 && process.created != 0 && process.exit_code_with_timeout(0)? == 0,
        "GC selection requires the retained owned Java root's zero exit",
    )?;
    let witness = GcControlSelection {
        schema_version: 1,
        kind: "cedar_gc_control_selection",
        pid: process.pid,
        creation_time_100ns_since_1601: process.created,
        owned_jvm_identity_verified: true,
        root_image_verified: true,
        log_files_absent_before_launch: true,
        root_handle_signaled: true,
        root_exit_code: 0,
        natural_shutdown_verified: true,
    };
    let encoded = serde_json::to_vec(&witness).map_err(|error| error.to_string())?;
    io(file.write_all(&encoded))?;
    io(file.write_all(b"\n"))?;
    io(file.sync_all())
}

fn gc_shutdown_is_natural(outcome: &JavaStopOutcome, actual_exit: u32) -> bool {
    outcome.status == StopStatus::Graceful
        && outcome.reason == StopReason::RootExited
        && outcome.root_exit == JavaRootExit::WindowsCode(0)
        && actual_exit == 0
        && outcome.shutdown_response_received
        && outcome.exit_frame_completed
        && outcome.cleanup_joined
}

#[derive(serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum ResourcePhase {
    Starting,
    JavaInitialized,
    SemanticReadyIdle,
    QueryWorkload,
    CorrectionReadyIdle,
    Closing,
    Cleanup,
    Complete,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum ResourceLatency {
    JavaInitialize,
    OpenExactDiagnostics,
    DefinitionConfinedUri,
    Completion,
    DeferredImportResolve,
    EditorApplyUndoRedo,
    CorrectionExactDiagnostics,
    StopVerifiedRootExit,
}

#[derive(serde::Serialize)]
struct PhaseMarker {
    phase: ResourcePhase,
    elapsed_ms: u128,
}

#[derive(serde::Serialize)]
struct LatencyMarker {
    latency: ResourceLatency,
    elapsed_ms: u128,
    duration_ns: u128,
}

// Test-only observation. The enclosing sampler exclusively creates this file in
// its generated scratch directory. Never create it here: the long baseline must
// refuse startup when instrumentation was not prepared. Later marker failures
// make the resource report incomplete; they do not change semantic acceptance.
fn resource_marker(marker: &impl serde::Serialize) -> bool {
    use std::io::Write;
    let Some(path) = std::env::var_os("CEDAR_RESOURCE_PHASE_PATH") else {
        return false;
    };
    let Ok(mut file) = fs::OpenOptions::new().append(true).open(path) else {
        return false;
    };
    let Ok(line) = serde_json::to_string(marker) else {
        return false;
    };
    writeln!(file, "{line}").is_ok()
}

fn resource_phase(started: Instant, phase: ResourcePhase) -> bool {
    resource_marker(&PhaseMarker {
        phase,
        elapsed_ms: started.elapsed().as_millis(),
    })
}

fn resource_latency(
    profile: ObservationProfile,
    started: Instant,
    operation_started: Instant,
    latency: ResourceLatency,
) {
    if profile.observes_resources() {
        let finished = Instant::now();
        resource_marker(&LatencyMarker {
            latency,
            elapsed_ms: finished.duration_since(started).as_millis(),
            duration_ns: finished.duration_since(operation_started).as_nanos(),
        });
    }
}

fn client_language(client: &mut AcceptanceClient, op: Operation) -> CheckResult<Value> {
    match client.request(op)? {
        Payload::Language { value } => Ok(value),
        _ => Err("normal Client returned a non-language response".into()),
    }
}
fn production_diagnostics(
    client: &mut AcceptanceClient,
    uri: &str,
    phase: DiagnosticPhase,
) -> CheckResult<()> {
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut classification = DiagnosticEvidence::new(1, phase);
    while Instant::now() < deadline {
        classification.begin_poll();
        let value = client_language(client, Operation::LanguageEvents)?;
        match classification.inspect_response(&value, uri) {
            Ok(true) => return Ok(()),
            Ok(false) => {}
            Err(_) => {
                return Err("normal Java event stream failed exact diagnostic validation".into())
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
    // The strict fixture's six diagnostic receipts remain a separate stream.
    Err("normal Java diagnostic witness timed out".into())
}

fn production_refresh_diagnostics(client: &mut AcceptanceClient, uri: &str) -> CheckResult<bool> {
    let deadline = Instant::now() + Duration::from_secs(60);
    while Instant::now() < deadline {
        let value = client_language(client, Operation::LanguageEvents)?;
        require(
            value["truncated"] == false,
            "refresh event stream was truncated",
        )?;
        let events = value["events"]
            .as_array()
            .ok_or("refresh event array missing")?;
        let mut witness = None;
        for event in events {
            match event["type"].as_str() {
                Some("diagnostics") => {
                    if let Some(unversioned) = refresh_diagnostics_match(&event["value"], uri) {
                        witness = Some(unversioned);
                    }
                }
                Some("notification" | "unsupported_server_request") => {}
                _ => return Err("refresh event stream closed, lagged or malformed".into()),
            }
        }
        if let Some(unversioned) = witness {
            return Ok(unversioned);
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err("explicit refresh synthetic diagnostic witness timed out".into())
}

fn await_production_java_startup(
    client: &mut AcceptanceClient,
    startup_id: u64,
    deadline: Instant,
    observed: &mut Option<RootObservation>,
    java: &Path,
) -> CheckResult<Value> {
    while Instant::now() < deadline {
        let status = client_language(client, Operation::LanguageStartJavaPoll { startup_id })?;
        require(
            status["startup_id"] == startup_id,
            "startup poll identity mismatch",
        )?;
        if observed.is_none() {
            if let Some(pid) = status["process_id"]
                .as_u64()
                .and_then(|pid| u32::try_from(pid).ok())
            {
                let root = RootObservation::open_current(1, pid)?;
                verify_java_image(&root, java)?;
                *observed = Some(root);
            }
        }
        match status["state"].as_str() {
            Some("ready") => return Ok(status["language"].clone()),
            Some("starting") => {}
            Some("failed" | "cancelled" | "cancelling") => {
                return Err("normal asynchronous Java startup did not become Ready".into())
            }
            _ => return Err("normal asynchronous Java startup returned an invalid state".into()),
        }
        thread::sleep(Duration::from_millis(50));
    }
    Err("normal asynchronous Java startup exceeded its original observation deadline".into())
}

fn cancel_production_java_startup(
    client: &mut AcceptanceClient,
    startup_id: u64,
) -> CheckResult<()> {
    let deadline = Instant::now() + EXIT_TIMEOUT;
    let mut status = client_language(client, Operation::LanguageStartJavaCancel { startup_id })?;
    loop {
        require(
            status["startup_id"] == startup_id,
            "startup cleanup identity mismatch",
        )?;
        match status["state"].as_str() {
            Some("cancelled" | "failed") => {
                return require(
                    status["cleanup_verified"] == true,
                    "startup cleanup was not verified",
                );
            }
            Some("cancelling") => {}
            _ => return Err("startup cleanup returned a non-cancelling state".into()),
        }
        require(
            Instant::now() < deadline,
            "startup cleanup evidence timed out",
        )?;
        thread::sleep(Duration::from_millis(50));
        status = client_language(client, Operation::LanguageStartJavaPoll { startup_id })?;
    }
}
fn verify_java_image(process: &RootObservation, expected: &Path) -> CheckResult<()> {
    let mut image = vec![0u16; 32_768];
    let mut length = image.len() as u32;
    // SAFETY: Retained query-only process handle and bounded writable UTF-16
    // output. This is identity observation, never a process search or kill path.
    if unsafe {
        QueryFullProcessImageNameW(
            process.handle.as_raw_handle(),
            0,
            image.as_mut_ptr(),
            &mut length,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let image = String::from_utf16(&image[..length as usize])
        .map_err(|_| "invalid observed executable path")?;
    require(
        ordinary_path(Path::new(&image))? == ordinary_path(expected)?,
        "observed root executable differs from the selected Java runtime",
    )?;
    require(
        process.created != 0 && process.live()?,
        "normal Java identity was not live and verifiable",
    )
}
fn require_java_capabilities(client: &AcceptanceClient) -> CheckResult<()> {
    let Payload::Hello {
        agent: Some(info), ..
    } = client.handshake()
    else {
        return Err("normal agent metadata missing".into());
    };
    require(
        !info.supports("language_start")
            && JAVA_LANGUAGE_SESSION_CAPABILITIES
                .iter()
                .all(|name| info.supports(name)),
        "normal Windows agent must expose only scoped Java startup",
    )
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

fn prepare_workspace_type_file(
    root: &Path,
    created: &mut Option<(PathBuf, workspace_types::Fixture)>,
) -> CheckResult<()> {
    use std::io::Write;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "synthetic type clock predates the epoch")?
        .as_nanos();
    let fixture = workspace_types::Fixture::new(nonce);
    let path = root.join(&fixture.path);
    io(fs::create_dir_all(
        path.parent().ok_or("synthetic type parent missing")?,
    ))?;
    let mut file = io(fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path))?;
    // Keep the expected bytes even when the write fails, independently of the
    // semantic result and before the enclosing owned directory is removed.
    *created = Some((path, fixture));
    io(file.write_all(created.as_ref().unwrap().1.source.as_bytes()))
}

fn workspace_type_file_unchanged(created: &Option<(PathBuf, workspace_types::Fixture)>) -> bool {
    created.as_ref().is_some_and(|(path, fixture)| {
        fs::read(path).is_ok_and(|bytes| bytes == fixture.source.as_bytes())
    })
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
            .checked_add(idle::REQUEST_BUDGET)
            .is_some_and(|end| end < deadline),
        "implementation witness lacks its existing request budget before the Quick deadline",
    )?;
    let result = client.request(op)?;
    require(
        Instant::now() < deadline,
        "implementation request exceeded the Quick deadline",
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

fn workspace_type_request(
    client: &mut AcceptanceClient,
    deadline: Instant,
    op: Operation,
) -> CheckResult<Value> {
    require(
        Instant::now() < deadline,
        "workspace type witness exhausted the Quick session budget",
    )?;
    client_language(client, op)
}

fn production_workspace_type(
    client: &mut AcceptanceClient,
    initialized: &Value,
    created: &Option<(PathBuf, workspace_types::Fixture)>,
    deadline: Instant,
    receipt: &mut workspace_types::Evidence,
) -> CheckResult<()> {
    receipt.exercised = true;
    receipt.failure_stage = workspace_types::Stage::Support;
    let Payload::Hello {
        agent: Some(info), ..
    } = client.handshake()
    else {
        return Err("workspace type agent metadata missing".into());
    };
    require(
        info.supports("language_workspace_symbols"),
        "workspace type capability missing",
    )?;
    receipt.capability_supported = true;
    let provider = &initialized["initialize"]["capabilities"]["workspaceSymbolProvider"];
    require(
        provider == true || provider.is_object(),
        "workspace symbol provider missing",
    )?;
    receipt.provider_supported = true;
    let (absolute, fixture) = created.as_ref().ok_or("synthetic workspace type missing")?;
    let expected = url::Url::from_file_path(ordinary_path(absolute)?)
        .map_err(|_| "cannot encode synthetic workspace type URI")?;
    receipt.failure_stage = workspace_types::Stage::Query;
    // This generated source has never received didOpen. There is exactly one
    // explicit symbol query after initial semantic readiness, with no indexing
    // retry or additional readiness/sleep window.
    let symbols = workspace_type_request(
        client,
        deadline,
        Operation::LanguageWorkspaceSymbols {
            query: fixture.name.clone(),
        },
    )?;
    let target_uri = fixture.exact_symbol(&symbols, expected.as_str())?;
    receipt.target_unopened = true;
    receipt.exact_type_name = true;
    receipt.exact_type_uri = true;
    receipt.exact_declaration_range = true;
    receipt.failure_stage = workspace_types::Stage::NegativeQuery;
    let negative = workspace_type_request(
        client,
        deadline,
        Operation::LanguageWorkspaceSymbols {
            query: fixture.negative_query.clone(),
        },
    )?;
    require(
        workspace_types::empty_result(&negative),
        "negative workspace type query was not empty",
    )?;
    receipt.negative_query_empty = true;
    receipt.failure_stage = workspace_types::Stage::Resolve;
    let resolved = workspace_type_request(
        client,
        deadline,
        Operation::LanguageResolveUri {
            uri: target_uri.clone(),
        },
    )?;
    require(
        resolved["path"] == fixture.path,
        "workspace type did not resolve to the owned source",
    )?;
    receipt.resolved_path_exact = true;
    receipt.failure_stage = workspace_types::Stage::Read;
    require(
        Instant::now() < deadline,
        "workspace type read exhausted the Quick session budget",
    )?;
    let Payload::File {
        path,
        text,
        revision,
    } = client.request(Operation::Read {
        path: fixture.path.clone(),
    })?
    else {
        return Err("workspace type ordinary read returned no source file".into());
    };
    let range = fixture.declaration();
    let (start, _) = completion::position_to_offsets(&text, range.start)?;
    let (end, _) = completion::position_to_offsets(&text, range.end)?;
    require(
        path == fixture.path
            && text == fixture.source
            && text.get(start..end) == Some(fixture.name.as_str())
            && workspace_type_file_unchanged(created),
        "workspace type ordinary read/source/declaration mismatch",
    )?;
    receipt.ordinary_read_exact = true;
    receipt.failure_stage = workspace_types::Stage::Frontend;
    crate::CedarApp::java_type_navigation_acceptance(
        symbols,
        &target_uri,
        &path,
        &text,
        &revision,
    )?;
    receipt.actual_frontend_navigation = true;
    receipt.dirty_buffer_reused = true;
    receipt.undo_redo_preserved = true;
    require(
        workspace_type_file_unchanged(created),
        "workspace type navigation changed source bytes",
    )?;
    require(
        receipt.semantics_passed(),
        "workspace type semantic witness was incomplete",
    )?;
    receipt.failure_stage = workspace_types::Stage::None;
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
        "organize imports exhausted the Quick session budget",
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

#[test]
#[ignore = "requires native Windows, installed JDT/Java and exact normal CEDAR_AGENT_BIN; run serially"]
fn real_windows_normal_agent_java_editor_acceptance() -> CheckResult<()> {
    normal_agent_java_acceptance(ObservationProfile::Quick)
}

#[test]
#[ignore = "requires native Windows, installed JDT/Java and exact normal CEDAR_AGENT_BIN; run serially"]
fn real_windows_normal_agent_java_idle_correction_acceptance() -> CheckResult<()> {
    normal_agent_java_acceptance(ObservationProfile::IdleCorrection)
}

#[test]
#[ignore = "requires native Windows, installed JDT/Java, exact normal CEDAR_AGENT_BIN and sampler marker file; run serially"]
fn real_windows_normal_agent_java_resource_baseline() -> CheckResult<()> {
    normal_agent_java_acceptance(ObservationProfile::ResourceBaseline)
}

#[test]
#[ignore = "requires native Windows, fixed diagnostic host, marked synthetic distribution and sampler files; run serially"]
fn real_windows_java_gc_diagnostic_control() -> CheckResult<()> {
    normal_agent_java_acceptance(ObservationProfile::GcDiagnostic)
}

fn normal_agent_java_acceptance(profile: ObservationProfile) -> CheckResult<()> {
    println!();
    let started = Instant::now();
    let _watchdog =
        Watchdog::start_with_timeout(if profile == ObservationProfile::IdleCorrection {
            idle::OUTER_BUDGET
        } else {
            Duration::from_secs(240)
        });
    let idle_budget = (profile == ObservationProfile::IdleCorrection)
        .then(|| std::rc::Rc::new(idle::Budget::default()));
    let clock = AcceptanceClock {
        started,
        idle: idle_budget.clone(),
    };
    let mut idle_correction = idle::Correction::default();
    let instrumentation_ready = profile_phase(profile, started, ResourcePhase::Starting);
    let mut fixture: Option<tempfile::TempDir> = None;
    let mut client: Option<AcceptanceClient> = None;
    let mut observed: Option<RootObservation> = None;
    let mut server_started = false;
    let mut startup_id = None;
    let mut all_clients_reaped = false;
    let stage = Cell::new(FailureStage::Setup);
    let mut failure_stage = None;
    let mut record = profile.evidence();
    let mut gc_selection: Option<fs::File> = None;
    let mut natural_shutdown_verified = false;
    let mut cleanup_errors = Vec::new();
    let mut source_path: Option<PathBuf> = None;
    let mut organize_files = Vec::new();
    let mut organize_receipt = organize::Evidence::new();
    let mut workspace_type_file = None;
    let mut workspace_type_receipt = workspace_types::Evidence::new();
    let mut implementation_fixture = None;
    let mut implementation_files = Vec::new();
    let mut implementation_receipt = implementations::Evidence::new();
    let primary = checked(|| {
        require(
            !profile.observes_resources() || instrumentation_ready,
            "resource observation requires the sampler's existing writable marker file",
        )?;
        let distribution = environment_path("CEDAR_JDTLS_HOME")?;
        let java = environment_path("CEDAR_JAVA")?;
        let (binary_environment, binary_name) = profile.agent_selection();
        let binary = environment_path(binary_environment)?;
        require(
            binary.is_file() && binary.file_name().is_some_and(|name| name == binary_name),
            "acceptance requires the exact binary for its selected observation profile",
        )?;
        fixture = Some(io(tempfile::Builder::new()
            .prefix("cedar normal Java 雪 ")
            .tempdir())?);
        let base = ordinary_path(fixture.as_ref().unwrap().path())?;
        let root = base.join("workspace 雪");
        io(fs::create_dir(&root))?;
        prepare_project_files(&root)?;
        if profile == ObservationProfile::Quick {
            prepare_organize_files(&root, &mut organize_files)?;
            prepare_workspace_type_file(&root, &mut workspace_type_file)?;
            prepare_implementation_files(
                &root,
                &mut implementation_fixture,
                &mut implementation_files,
            )?;
        }
        let data = base.join("external JDT data 雪");
        io(fs::create_dir(&data))?;
        require(
            !data.starts_with(&root)
                && !root.join(".cedar-windows-language-validation").exists()
                && !root.join(".cedar-windows-java-validation").exists(),
            "production fixture must have no validation opt-in markers",
        )?;
        if profile == ObservationProfile::GcDiagnostic {
            io(fs::write(
                root.join(GC_WORKSPACE_MARKER),
                GC_WORKSPACE_MARKER_CONTENTS,
            ))?;
            let selection = PathBuf::from(
                std::env::var_os("CEDAR_GC_SELECTION_PATH")
                    .ok_or("GC control requires the sampler's private selection path")?,
            );
            gc_selection = Some(prepare_gc_selection(&distribution, &selection)?);
        } else {
            require_absent(&root.join(GC_WORKSPACE_MARKER))?;
            require_absent(&distribution.join(GC_DISTRIBUTION_MARKER))?;
        }
        let source = root.join(SOURCE_PATH);
        source_path = Some(source.clone());
        let start_operation = || -> CheckResult<Operation> {
            let java_executable = text(&java)?;
            let distribution = text(&distribution)?;
            let data_directory = text(&data)?;
            Ok(if profile == ObservationProfile::Quick {
                Operation::LanguageStartJavaBegin {
                    java_executable,
                    distribution,
                    data_directory,
                }
            } else {
                Operation::LanguageStartJava {
                    java_executable,
                    distribution,
                    data_directory,
                }
            })
        };
        // A normal untrusted connection cannot launch Java; metadata is not trust.
        connect_client(&mut client, &clock, &binary, &root, false)?;
        require_java_capabilities(client.as_ref().unwrap())?;
        let denied = client
            .as_mut()
            .unwrap()
            .request(start_operation()?)
            .err()
            .ok_or("untrusted normal agent launched Java")?;
        require(
            denied.contains("run_disabled") && !data.join(".metadata").exists(),
            "untrusted Java startup was not rejected before launch",
        )?;
        record.untrusted_start_rejected = true;
        let denied_generic = client
            .as_mut()
            .unwrap()
            .request(Operation::LanguageStart {
                program: "must-not-run".into(),
                args: vec![],
            })
            .err()
            .ok_or("normal Client allowed generic Windows startup")?;
        require(
            denied_generic.contains("unsupported_operation")
                && denied_generic.contains("language_start"),
            "generic startup was not rejected by Client capabilities",
        )?;
        record.generic_start_rejected = true;
        reap_client(&mut client, &clock, false)?;
        all_clients_reaped = true;
        unchanged(&source)?;
        // This explicit synthetic trust is test authorization, never a GUI action.
        if profile == ObservationProfile::GcDiagnostic {
            verify_gc_logs_absent(&distribution)?;
        }
        all_clients_reaped = false;
        connect_client(&mut client, &clock, &binary, &root, true)?;
        let client = client.as_mut().unwrap();
        require_java_capabilities(client)?;
        record.java_capabilities = true;
        stage.set(FailureStage::Initialize);
        let initialize_started = Instant::now();
        let initialized = if profile == ObservationProfile::Quick {
            record.async_start_exercised = true;
            let deadline = Instant::now() + Duration::from_secs(75);
            let begin = client_language(client, start_operation()?)?;
            let id = begin["startup_id"]
                .as_u64()
                .filter(|id| *id != 0)
                .ok_or("asynchronous Java Begin omitted its startup identity")?;
            startup_id = Some(id);
            require(
                begin["state"] == "starting",
                "Begin must acknowledge Starting, not completed initialization",
            )?;
            record.async_start_begin_acknowledged = true;
            let read = client.request(Operation::Read {
                path: SOURCE_PATH.into(),
            })?;
            require(
                matches!(read, Payload::File { path, text, .. } if path == SOURCE_PATH && text == SOURCE),
                "ordinary source read failed while Java startup was pending",
            )?;
            unchanged(&source)?;
            record.async_start_read_while_starting = true;
            let ready = await_production_java_startup(client, id, deadline, &mut observed, &java)?;
            record.async_start_ready = true;
            ready
        } else {
            client_language(client, start_operation()?)?
        };
        require(
            initialized["started"] == true,
            "normal Java startup was not acknowledged",
        )?;
        server_started = true;
        let pid = initialized["process_id"]
            .as_u64()
            .and_then(|pid| u32::try_from(pid).ok())
            .filter(|pid| *pid != 0)
            .ok_or("normal Java start omitted its owned process id")?;
        if let Some(root) = &observed {
            require(
                root.pid == pid && root.live()?,
                "Ready changed the observed startup process",
            )?;
        } else {
            observed = Some(RootObservation::open_current(1, pid)?);
        }
        record.root_observed_live = true;
        verify_java_image(observed.as_ref().unwrap(), &java)?;
        record.root_identity_verified = true;
        if matches!(
            profile,
            ObservationProfile::Quick | ObservationProfile::IdleCorrection
        ) {
            record.diagnostics_refresh_exercised = profile == ObservationProfile::Quick;
            require(
                initialized["initialize"]["cedar_java_diagnostics_refresh"] == true,
                "vetted Standard JDT refresh support was not established",
            )?;
            record.diagnostics_refresh_supported = true;
        }
        resource_latency(
            profile,
            started,
            initialize_started,
            ResourceLatency::JavaInitialize,
        );
        profile_phase(profile, started, ResourcePhase::JavaInitialized);
        require(
            data.join(".metadata").is_dir(),
            "normal recipe did not use the selected external data directory",
        )?;
        unchanged(&source)?;
        stage.set(FailureStage::Open);
        let open_started = Instant::now();
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
            .ok_or("normal didOpen omitted URI")?
            .to_owned();
        let expected = url::Url::from_file_path(ordinary_path(&source)?)
            .map_err(|_| "cannot encode normal source URI")?;
        require(
            opened["version"] == 1 && same_local_uri(&uri, expected.as_str()),
            "normal didOpen URI/version mismatch",
        )?;
        unchanged(&source)?;
        stage.set(FailureStage::Diagnostics);
        if let Some(budget) = &idle_budget {
            idle::diagnostics(
                budget,
                &mut DiagnosticEvidence::new(1, DiagnosticPhase::Initial),
                &uri,
                |op| client_language(client, op),
                || started.elapsed(),
                thread::sleep,
            )?;
        } else {
            production_diagnostics(client, &uri, DiagnosticPhase::Initial)?;
        }
        record.semantic_diagnostics = true;
        unchanged(&source)?;
        resource_latency(
            profile,
            started,
            open_started,
            ResourceLatency::OpenExactDiagnostics,
        );
        if profile == ObservationProfile::IdleCorrection {
            clock.run(idle::INITIAL_IDLE, || {
                thread::sleep(idle::INITIAL_IDLE);
                Ok(())
            })?;
        } else if profile_phase(profile, started, ResourcePhase::SemanticReadyIdle)
            || profile.observes_resources()
        {
            // Defined observation interval after exact diagnostics, not a claim
            // that JDT indexing or other background work has fully settled.
            thread::sleep(Duration::from_secs(match profile {
                ObservationProfile::Quick => 2,
                ObservationProfile::IdleCorrection
                | ObservationProfile::ResourceBaseline
                | ObservationProfile::GcDiagnostic => 30,
            }));
        }
        profile_phase(profile, started, ResourcePhase::QueryWorkload);
        if profile == ObservationProfile::Quick {
            stage.set(FailureStage::Definition);
            production_workspace_type(
                client,
                &initialized,
                &workspace_type_file,
                started + Duration::from_secs(180),
                &mut workspace_type_receipt,
            )?;
            production_implementations(
                client,
                &initialized,
                &root,
                &implementation_fixture,
                &implementation_files,
                started + Duration::from_secs(180),
                &mut implementation_receipt,
            )?;
            unchanged(&source)?;
        }
        let cursor = completion::byte_to_position(
            SOURCE,
            SOURCE.rfind("greeting").ok_or("fixture reference")? + 3,
        )?;
        stage.set(FailureStage::Definition);
        let definition_started = Instant::now();
        let definition = client_language(
            client,
            Operation::LanguageQuery {
                path: SOURCE_PATH.into(),
                line: cursor.line,
                character: cursor.character,
                kind: LanguageQueryKind::Definition,
            },
        )?;
        let definition_uri = exact_definition(&definition, &uri)?;
        let confined = client_language(
            client,
            Operation::LanguageResolveUri {
                uri: definition_uri,
            },
        )?;
        require(
            confined["path"] == SOURCE_PATH,
            "normal definition escaped the exact source",
        )?;
        record.exact_definition = true;
        unchanged(&source)?;
        resource_latency(
            profile,
            started,
            definition_started,
            ResourceLatency::DefinitionConfinedUri,
        );
        stage.set(FailureStage::Completion);
        let completion_started = Instant::now();
        let response = client_language(
            client,
            Operation::LanguageQuery {
                path: SOURCE_PATH.into(),
                line: cursor.line,
                character: cursor.character,
                kind: LanguageQueryKind::Completion,
            },
        )?;
        let results = completion::parse_completion_result(&response)?;
        require(
            results.candidates.iter().any(|candidate| {
                candidate.label.starts_with("greeting") && candidate.item["textEdit"].is_object()
            }),
            "normal completion omitted the local source variable",
        )?;
        let candidate = results
            .candidates
            .into_iter()
            .find(|candidate| candidate.label.starts_with("GregorianCalendar"))
            .ok_or("normal completion omitted GregorianCalendar")?;
        require(
            candidate.disabled_reason.is_none(),
            "normal completion disabled by frontend",
        )?;
        record.real_completion = true;
        let original = candidate.item;
        require(
            original["textEdit"].is_object()
                && !original["data"].is_null()
                && original
                    .get("additionalTextEdits")
                    .is_none_or(|v| v.is_null() || v.as_array().is_some_and(Vec::is_empty)),
            "normal completion did not defer its import",
        )?;
        unchanged(&source)?;
        resource_latency(
            profile,
            started,
            completion_started,
            ResourceLatency::Completion,
        );
        stage.set(FailureStage::Resolve);
        let resolve_started = Instant::now();
        let resolved = client_language(
            client,
            Operation::LanguageResolveCompletion {
                item: original.clone(),
            },
        )?;
        crate::language_ui::validate_resolved_identity(&original, &resolved)?;
        let edits = resolved["additionalTextEdits"]
            .as_array()
            .ok_or("normal resolve omitted import")?;
        require(
            edits.len() == 1
                && edits[0]["newText"]
                    .as_str()
                    .is_some_and(|text| text.contains("import java.util.GregorianCalendar;")),
            "normal resolve did not supply exactly one expected import",
        )?;
        record.deferred_import_resolve = true;
        unchanged(&source)?;
        resource_latency(
            profile,
            started,
            resolve_started,
            ResourceLatency::DeferredImportResolve,
        );
        let editor_started = Instant::now();
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
                "normal editor didChange URI/version mismatch",
            )?;
            unchanged(&source)
        })?;
        record.actual_editor_apply_undo_redo = true;
        record.versions_2_3_4_synced = true;
        resource_latency(
            profile,
            started,
            editor_started,
            ResourceLatency::EditorApplyUndoRedo,
        );
        stage.set(FailureStage::Correction);
        let correction_started = Instant::now();
        let corrected = client_language(
            client,
            Operation::LanguageChange {
                path: SOURCE_PATH.into(),
                version: 5,
                text: corrected_source(),
            },
        )?;
        require(
            corrected["version"] == 5
                && corrected["changed"]
                    .as_str()
                    .is_some_and(|actual| same_local_uri(actual, &uri)),
            "normal correction URI/version mismatch",
        )?;
        record.correction_acknowledged = true;
        if let Some(budget) = &idle_budget {
            let correction = idle_correction.run(
                budget,
                &uri,
                |op| client_language(client, op),
                || started.elapsed(),
                thread::sleep,
            );
            record.correction_diagnostics = idle_correction.spontaneous_success;
            correction?;
        } else {
            production_diagnostics(client, &uri, DiagnosticPhase::Correction)?;
            record.correction_diagnostics = true;
        }
        unchanged(&source)?;
        resource_latency(
            profile,
            started,
            correction_started,
            ResourceLatency::CorrectionExactDiagnostics,
        );
        if profile == ObservationProfile::Quick {
            // A separate explicit action after all original rapid-edit criteria
            // passed. This does not rescue that acceptance or replay an edit.
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
                "refresh draft synchronization mismatch",
            )?;
            unchanged(&source)?;
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
                        .is_some_and(|actual| same_local_uri(actual, &uri)),
                "explicit refresh acknowledgement mismatch",
            )?;
            record.diagnostics_refresh_requested = true;
            record.diagnostics_refresh_unversioned = production_refresh_diagnostics(client, &uri)?;
            record.diagnostics_refresh_witness = true;
            unchanged(&source)?;
            // Extra witnesses apply only to Quick, after every original
            // semantic/async/refresh criterion. Resource and GC workloads stay fixed.
            stage.set(FailureStage::Apply);
            production_organize_imports(
                client,
                &root,
                &initialized,
                &organize_files,
                started + Duration::from_secs(180),
                &mut organize_receipt,
            )?;
            unchanged(&source)?;
        }
        if profile.observes_resources() {
            profile_phase(profile, started, ResourcePhase::CorrectionReadyIdle);
            // A second fixed observation window, not an indexing-settled claim.
            thread::sleep(Duration::from_secs(30));
            profile_phase(profile, started, ResourcePhase::Closing);
        }
        stage.set(FailureStage::Close);
        client_language(
            client,
            Operation::LanguageClose {
                path: SOURCE_PATH.into(),
            },
        )?;
        require(
            observed.as_ref().unwrap().live()?,
            "normal Java stopped before explicit Stop",
        )?;
        unchanged(&source)
    });
    if primary.is_err() {
        failure_stage = Some(stage.get());
    }
    // Cleanup has its own fixed outer deadline, even after primary admission
    // fails. Stop must never be rejected merely for crossing the primary gate.
    if let Some(budget) = &idle_budget {
        budget.begin_cleanup(started.elapsed());
    }
    profile_phase(profile, started, ResourcePhase::Cleanup);
    if !server_started {
        if let (Some(id), Some(client)) = (startup_id, client.as_mut()) {
            let cancel = checked(|| cancel_production_java_startup(client, id));
            if let Err(error) = cancel {
                failure_stage.get_or_insert(FailureStage::Stop);
                cleanup_errors.push(error);
            }
        }
    }
    if server_started {
        stage.set(FailureStage::Stop);
        let stop = checked(|| {
            let client = client.as_mut().ok_or("normal Client missing during Stop")?;
            require(
                client.is_connected(),
                "normal Client disconnected before Stop",
            )?;
            let stop_started = Instant::now();
            let outcome =
                JavaStopOutcome::parse(&client_language(client, Operation::LanguageStop)?)?;
            record.stop_status = match outcome.status {
                StopStatus::Graceful => ProductionStopStatus::Graceful,
                StopStatus::Forced => ProductionStopStatus::Forced,
                StopStatus::Error => ProductionStopStatus::Error,
            };
            record.stop_reason = match outcome.reason {
                StopReason::RootExited => ProductionStopReason::RootExited,
                StopReason::GraceExpired => ProductionStopReason::GraceExpired,
                StopReason::Aborted => ProductionStopReason::Aborted,
                StopReason::TransportFailure => ProductionStopReason::TransportFailure,
                StopReason::WorkerPanicked => ProductionStopReason::WorkerPanicked,
            };
            record.cleanup_joined = outcome.cleanup_joined;
            record.shutdown_response_received = outcome.shutdown_response_received;
            record.exit_frame_completed = outcome.exit_frame_completed;
            stage.set(FailureStage::RootExit);
            let actual = clock.run(idle::ROOT_EXIT_BUDGET, || {
                observed
                    .as_ref()
                    .ok_or("normal Java observer missing")?
                    .exit_code_with_timeout(3000)
            })?;
            record.root_handle_signaled = true;
            record.root_exit_code = Some(actual);
            require(
                outcome.root_exit == JavaRootExit::WindowsCode(actual),
                "typed stop and actual retained process exit disagree",
            )?;
            record.stop_outcome_verified = true;
            require(
                outcome.status != StopStatus::Error,
                "normal Java stop reported cleanup errors",
            )?;
            natural_shutdown_verified = gc_shutdown_is_natural(&outcome, actual);
            resource_latency(
                profile,
                started,
                stop_started,
                ResourceLatency::StopVerifiedRootExit,
            );
            if profile == ObservationProfile::GcDiagnostic {
                require(
                    natural_shutdown_verified,
                    "GC control requires natural Java exit zero and verified graceful cleanup",
                )?;
            }
            Ok(())
        });
        if let Err(error) = stop {
            failure_stage.get_or_insert(stage.get());
            cleanup_errors.push(error);
        }
    }
    if client.is_some() {
        match checked(|| reap_client(&mut client, &clock, true)) {
            Ok(()) => all_clients_reaped = true,
            Err(error) => {
                all_clients_reaped = false;
                failure_stage.get_or_insert(FailureStage::AgentExit);
                cleanup_errors.push(error);
            }
        }
    }
    record.client_reaped = all_clients_reaped;
    let root_stopped = match &observed {
        Some(process) => match clock.run(idle::ROOT_EXIT_BUDGET, || {
            process.exit_code_with_timeout(3000)
        }) {
            Ok(code) => {
                record.root_handle_signaled = true;
                record.root_exit_code = Some(code);
                true
            }
            Err(error) => {
                failure_stage.get_or_insert(FailureStage::RootExit);
                cleanup_errors.push(error);
                false
            }
        },
        None => true,
    };
    record.source_unchanged = source_path
        .as_ref()
        .is_some_and(|path| unchanged(path).is_ok());
    if source_path.is_some() && !record.source_unchanged {
        failure_stage.get_or_insert(FailureStage::FixtureCleanup);
        cleanup_errors.push("normal source bytes changed before cleanup".into());
    }
    if profile == ObservationProfile::Quick {
        implementation_receipt.source_files_unchanged =
            implementation_files_unchanged(&implementation_files);
        if !implementation_files.is_empty() && !implementation_receipt.source_files_unchanged {
            failure_stage.get_or_insert(FailureStage::FixtureCleanup);
            cleanup_errors.push("implementation source bytes changed before cleanup".into());
        }
        workspace_type_receipt.source_unchanged =
            workspace_type_file_unchanged(&workspace_type_file);
        if workspace_type_file.is_some() && !workspace_type_receipt.source_unchanged {
            failure_stage.get_or_insert(FailureStage::FixtureCleanup);
            cleanup_errors.push("workspace type source bytes changed before cleanup".into());
        }
        organize_receipt.source_files_unchanged = organize_files_unchanged(&organize_files);
        if !organize_files.is_empty() && !organize_receipt.source_files_unchanged {
            failure_stage.get_or_insert(FailureStage::FixtureCleanup);
            cleanup_errors.push("organize fixture source bytes changed before cleanup".into());
        }
    }
    record.synthetic_root_removed = match fixture.take() {
        Some(temp) if record.client_reaped && root_stopped => match temp.close() {
            Ok(()) => true,
            Err(error) => {
                failure_stage.get_or_insert(FailureStage::FixtureCleanup);
                cleanup_errors.push(error.to_string());
                false
            }
        },
        Some(temp) => {
            let _ = temp.keep();
            false
        }
        None => false,
    };
    if profile == ObservationProfile::GcDiagnostic
        && natural_shutdown_verified
        && record.root_observed_live
        && record.root_identity_verified
        && record.client_reaped
        && record.synthetic_root_removed
        && cleanup_errors.is_empty()
    {
        let selection = checked(|| {
            finalize_gc_selection(
                gc_selection
                    .as_mut()
                    .ok_or("GC selection reservation missing")?,
                observed.as_ref().ok_or("GC root observer missing")?,
            )
        });
        if let Err(error) = selection {
            failure_stage.get_or_insert(FailureStage::FixtureCleanup);
            cleanup_errors.push(error);
        }
    }
    if let Some(budget) = &idle_budget {
        if let Err(error) = budget.check(started.elapsed()) {
            failure_stage.get_or_insert(FailureStage::FixtureCleanup);
            cleanup_errors.push(error);
        }
    }
    record.primary_failed = primary.is_err();
    record.cleanup_failed = !cleanup_errors.is_empty();
    if profile == ObservationProfile::Quick {
        implementation_receipt.root_handle_signaled = record.root_handle_signaled;
        implementation_receipt.client_reaped = record.client_reaped;
        implementation_receipt.synthetic_root_removed = record.synthetic_root_removed;
        implementation_receipt.primary_failed = record.primary_failed;
        implementation_receipt.cleanup_failed = record.cleanup_failed;
        implementation_receipt.success = primary.is_ok()
            && cleanup_errors.is_empty()
            && implementation_receipt.semantics_passed()
            && implementation_receipt.source_files_unchanged
            && implementation_receipt.root_handle_signaled
            && implementation_receipt.client_reaped
            && implementation_receipt.synthetic_root_removed;
        let elapsed = started.elapsed().as_millis();
        implementation_receipt.elapsed_ms = elapsed.min(240_000) as u32;
        implementation_receipt.elapsed_saturated = elapsed > 240_000;
        println!(
            "{}",
            serde_json::to_string(&implementation_receipt).expect("typed implementation evidence")
        );
        workspace_type_receipt.root_handle_signaled = record.root_handle_signaled;
        workspace_type_receipt.client_reaped = record.client_reaped;
        workspace_type_receipt.synthetic_root_removed = record.synthetic_root_removed;
        workspace_type_receipt.primary_failed = record.primary_failed;
        workspace_type_receipt.cleanup_failed = record.cleanup_failed;
        workspace_type_receipt.success = primary.is_ok()
            && cleanup_errors.is_empty()
            && workspace_type_receipt.semantics_passed()
            && workspace_type_receipt.source_unchanged
            && workspace_type_receipt.root_handle_signaled
            && workspace_type_receipt.client_reaped
            && workspace_type_receipt.synthetic_root_removed;
        let elapsed = started.elapsed().as_millis();
        workspace_type_receipt.elapsed_ms = elapsed.min(240_000) as u32;
        workspace_type_receipt.elapsed_saturated = elapsed > 240_000;
        println!(
            "{}",
            serde_json::to_string(&workspace_type_receipt).expect("typed workspace type evidence")
        );
        organize_receipt.root_handle_signaled = record.root_handle_signaled;
        organize_receipt.client_reaped = record.client_reaped;
        organize_receipt.synthetic_root_removed = record.synthetic_root_removed;
        organize_receipt.primary_failed = record.primary_failed;
        organize_receipt.cleanup_failed = record.cleanup_failed;
        organize_receipt.success = primary.is_ok()
            && cleanup_errors.is_empty()
            && organize_receipt.semantics_passed()
            && organize_receipt.source_files_unchanged
            && organize_receipt.root_handle_signaled
            && organize_receipt.client_reaped
            && organize_receipt.synthetic_root_removed;
        let elapsed = started.elapsed().as_millis();
        organize_receipt.elapsed_ms = elapsed.min(240_000) as u32;
        organize_receipt.elapsed_saturated = elapsed > 240_000;
        println!(
            "{}",
            serde_json::to_string(&organize_receipt).expect("typed organize imports evidence")
        );
    }
    record.success = primary.is_ok()
        && cleanup_errors.is_empty()
        && record.java_capabilities
        && record.generic_start_rejected
        && record.untrusted_start_rejected
        && record.root_observed_live
        && record.root_identity_verified
        && record.semantic_diagnostics
        && record.exact_definition
        && record.real_completion
        && record.deferred_import_resolve
        && record.actual_editor_apply_undo_redo
        && record.versions_2_3_4_synced
        && record.correction_acknowledged
        && (if profile == ObservationProfile::IdleCorrection {
            idle_correction.accepted() && record.diagnostics_refresh_supported
        } else {
            record.correction_diagnostics
        })
        && record.source_unchanged
        && record.stop_outcome_verified
        && record.cleanup_joined
        && record.root_handle_signaled
        && record.client_reaped
        && record.synthetic_root_removed
        && (profile != ObservationProfile::Quick
            || (record.async_start_exercised
                && record.async_start_begin_acknowledged
                && record.async_start_read_while_starting
                && record.async_start_ready
                && record.diagnostics_refresh_exercised
                && record.diagnostics_refresh_supported
                && record.diagnostics_refresh_requested
                && record.diagnostics_refresh_witness
                && workspace_type_receipt.success
                && implementation_receipt.success
                && organize_receipt.success))
        && (profile != ObservationProfile::GcDiagnostic || natural_shutdown_verified)
        && idle_budget
            .as_ref()
            .is_none_or(|budget| budget.deadlines_met(started.elapsed()));
    let finished = started.elapsed();
    if let Some(budget) = &idle_budget {
        if !budget.deadlines_met(finished) {
            record.success = false;
            record.cleanup_failed |= budget.check(finished).is_err();
        }
    }
    record.failure_stage = if record.success {
        FailureStage::None
    } else {
        failure_stage.unwrap_or(FailureStage::Setup)
    };
    let elapsed = finished.as_millis();
    let elapsed_limit = if profile == ObservationProfile::IdleCorrection {
        480_000
    } else {
        300_000
    };
    record.elapsed_ms = elapsed.min(elapsed_limit) as u32;
    record.elapsed_saturated = elapsed > elapsed_limit;
    if let Some(budget) = &idle_budget {
        println!(
            "{}",
            serde_json::to_string(&idle::Receipt::new(
                &record,
                &idle_correction,
                budget,
                finished,
            ))
            .expect("typed idle correction evidence")
        );
    } else {
        println!(
            "{}",
            serde_json::to_string(&record).expect("typed normal Java evidence")
        );
    }
    profile_phase(profile, started, ResourcePhase::Complete);
    match primary {
        Err(error) => Err(format!(
            "normal Java primary failure: {error}; cleanup failures: {cleanup_errors:?}"
        )),
        Ok(()) if !record.success => {
            Err(format!("normal Java cleanup failure: {cleanup_errors:?}"))
        }
        Ok(()) => Ok(()),
    }
}

#[test]
fn idle_workflow_is_shipping_and_never_a_resource_profile() {
    let profile = ObservationProfile::IdleCorrection;
    assert_eq!(
        profile.agent_selection(),
        ("CEDAR_AGENT_BIN", "cedar-agent.exe")
    );
    assert_eq!(profile.evidence().kind, "windows_java_idle_correction");
    assert_eq!(profile.evidence().route, "normal_agent_client");
    assert!(!profile.observes_resources());
    assert!(!ObservationProfile::Quick.observes_resources());
    assert!(ObservationProfile::ResourceBaseline.observes_resources());
    assert!(ObservationProfile::GcDiagnostic.observes_resources());
}

#[test]
fn gc_control_is_separate_from_both_shipping_observation_profiles() {
    for profile in [
        ObservationProfile::Quick,
        ObservationProfile::ResourceBaseline,
    ] {
        assert_eq!(
            profile.agent_selection(),
            ("CEDAR_AGENT_BIN", "cedar-agent.exe")
        );
        assert_eq!(profile.evidence().kind, "windows_java_production");
        assert_eq!(profile.evidence().route, "normal_agent_client");
    }
    let control = ObservationProfile::GcDiagnostic;
    assert_eq!(
        control.agent_selection(),
        (
            "CEDAR_GC_DIAGNOSTIC_AGENT_BIN",
            "cedar-agent-java-gc-diagnostic.exe"
        )
    );
    assert_eq!(control.evidence().kind, "windows_java_gc_control");
    assert_eq!(control.evidence().route, "diagnostic_agent_normal_client");
    assert!(control.observes_resources());
}

#[test]
fn gc_control_rejects_forced_zero_exit_and_incomplete_shutdown() {
    let natural = JavaStopOutcome {
        status: StopStatus::Graceful,
        reason: StopReason::RootExited,
        root_exit: JavaRootExit::WindowsCode(0),
        cleanup_joined: true,
        shutdown_response_received: true,
        exit_frame_completed: true,
    };
    assert!(gc_shutdown_is_natural(&natural, 0));
    assert!(!gc_shutdown_is_natural(&natural, 1));
    for invalid in [
        JavaStopOutcome {
            status: StopStatus::Forced,
            ..natural.clone()
        },
        JavaStopOutcome {
            status: StopStatus::Error,
            ..natural.clone()
        },
        JavaStopOutcome {
            reason: StopReason::GraceExpired,
            ..natural.clone()
        },
        JavaStopOutcome {
            root_exit: JavaRootExit::WindowsCode(1),
            ..natural.clone()
        },
        JavaStopOutcome {
            cleanup_joined: false,
            ..natural.clone()
        },
        JavaStopOutcome {
            shutdown_response_received: false,
            ..natural.clone()
        },
        JavaStopOutcome {
            exit_frame_completed: false,
            ..natural.clone()
        },
    ] {
        assert!(!gc_shutdown_is_natural(&invalid, 0));
    }
}

#[test]
fn gc_selection_reservation_requires_exact_fresh_in_distribution_file() -> CheckResult<()> {
    let fixture = io(tempfile::tempdir())?;
    let distribution = ordinary_path(fixture.path())?;
    let selection = distribution.join(GC_SELECTION_FILE);
    assert!(prepare_gc_selection(&distribution, &selection).is_err());
    io(fs::write(
        distribution.join(GC_DISTRIBUTION_MARKER),
        GC_DISTRIBUTION_MARKER_CONTENTS,
    ))?;
    assert!(prepare_gc_selection(&distribution, &distribution.join("selection.json")).is_err());
    let nested = distribution.join("nested");
    io(fs::create_dir(&nested))?;
    assert!(prepare_gc_selection(&distribution, &nested.join(GC_SELECTION_FILE)).is_err());
    assert!(
        prepare_gc_selection(&distribution, &nested.join("..").join(GC_SELECTION_FILE)).is_err()
    );
    let reserved = prepare_gc_selection(&distribution, &selection)?;
    assert_eq!(io(reserved.metadata())?.len(), 0);
    assert!(prepare_gc_selection(&distribution, &selection).is_err());
    drop(reserved);
    assert_eq!(io(fs::read(selection))?, b"");
    Ok(())
}

#[test]
fn gc_selection_rejects_all_preexisting_logging_namespace_entries() -> CheckResult<()> {
    let fixture = io(tempfile::tempdir())?;
    let distribution = ordinary_path(fixture.path())?;
    io(fs::write(
        distribution.join(GC_DISTRIBUTION_MARKER),
        GC_DISTRIBUTION_MARKER_CONTENTS,
    ))?;
    let selection = distribution.join(GC_SELECTION_FILE);
    for name in [
        "cedar-gc-314.log",
        "CEDAR-GC-314.log.0",
        "cedar-gc-unexpected",
        "cedar-gc.log",
    ] {
        let log = distribution.join(name);
        io(fs::write(&log, b"private preexisting content"))?;
        assert!(prepare_gc_selection(&distribution, &selection).is_err());
        assert!(!selection.exists());
        io(fs::remove_file(log))?;
    }
    io(fs::create_dir(distribution.join("cedar-gc-directory")))?;
    assert!(prepare_gc_selection(&distribution, &selection).is_err());
    assert!(!selection.exists());
    Ok(())
}
