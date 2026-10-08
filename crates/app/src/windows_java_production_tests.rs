//! Normal shipping agent + capability-enforcing Client acceptance. Bundle path
//! discovery is covered by the existing isolated Local bundle tests separately.
use super::*;
use crate::java_language::{JavaStopOutcome, StopReason, StopStatus};
use cedar_client::Client;
use windows_sys::Win32::System::Threading::QueryFullProcessImageNameW;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ObservationProfile {
    Quick,
    ResourceBaseline,
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
    if profile == ObservationProfile::ResourceBaseline {
        let finished = Instant::now();
        resource_marker(&LatencyMarker {
            latency,
            elapsed_ms: finished.duration_since(started).as_millis(),
            duration_ns: finished.duration_since(operation_started).as_nanos(),
        });
    }
}

fn client_language(client: &mut Client, op: Operation) -> CheckResult<Value> {
    match client.request(op)? {
        Payload::Language { value } => Ok(value),
        _ => Err("normal Client returned a non-language response".into()),
    }
}
fn production_diagnostics(
    client: &mut Client,
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
fn require_java_capabilities(client: &Client) -> CheckResult<()> {
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

#[test]
#[ignore = "requires native Windows, installed JDT/Java and exact normal CEDAR_AGENT_BIN; run serially"]
fn real_windows_normal_agent_java_editor_acceptance() -> CheckResult<()> {
    normal_agent_java_acceptance(ObservationProfile::Quick)
}

#[test]
#[ignore = "requires native Windows, installed JDT/Java, exact normal CEDAR_AGENT_BIN and sampler marker file; run serially"]
fn real_windows_normal_agent_java_resource_baseline() -> CheckResult<()> {
    normal_agent_java_acceptance(ObservationProfile::ResourceBaseline)
}

fn normal_agent_java_acceptance(profile: ObservationProfile) -> CheckResult<()> {
    println!();
    let _watchdog = Watchdog::start_with_timeout(Duration::from_secs(240));
    let started = Instant::now();
    let instrumentation_ready = resource_phase(started, ResourcePhase::Starting);
    let mut fixture: Option<tempfile::TempDir> = None;
    let mut client: Option<Client> = None;
    let mut observed: Option<RootObservation> = None;
    let mut server_started = false;
    let mut all_clients_reaped = false;
    let stage = Cell::new(FailureStage::Setup);
    let mut failure_stage = None;
    let mut record = ProductionEvidence::new();
    let mut cleanup_errors = Vec::new();
    let mut source_path: Option<PathBuf> = None;
    let primary = checked(|| {
        require(
            profile == ObservationProfile::Quick || instrumentation_ready,
            "resource baseline requires the sampler's existing writable marker file",
        )?;
        let distribution = environment_path("CEDAR_JDTLS_HOME")?;
        let java = environment_path("CEDAR_JAVA")?;
        let binary = environment_path("CEDAR_AGENT_BIN")?;
        require(
            binary.is_file()
                && binary
                    .file_name()
                    .is_some_and(|name| name == "cedar-agent.exe"),
            "production acceptance requires the exact normal cedar-agent.exe",
        )?;
        fixture = Some(io(tempfile::Builder::new()
            .prefix("cedar normal Java 雪 ")
            .tempdir())?);
        let base = ordinary_path(fixture.as_ref().unwrap().path())?;
        let root = base.join("workspace 雪");
        io(fs::create_dir(&root))?;
        prepare_project_files(&root)?;
        let data = base.join("external JDT data 雪");
        io(fs::create_dir(&data))?;
        require(
            !data.starts_with(&root)
                && !root.join(".cedar-windows-language-validation").exists()
                && !root.join(".cedar-windows-java-validation").exists(),
            "production fixture must have no validation opt-in markers",
        )?;
        let source = root.join(SOURCE_PATH);
        source_path = Some(source.clone());
        let start_operation = || -> CheckResult<Operation> {
            Ok(Operation::LanguageStartJava {
                java_executable: text(&java)?,
                distribution: text(&distribution)?,
                data_directory: text(&data)?,
            })
        };
        // A normal untrusted connection cannot launch Java; metadata is not trust.
        client = Some(Client::spawn_agent(&binary, &root, false)?);
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
        client.take().unwrap().close_and_wait(EXIT_TIMEOUT)?;
        all_clients_reaped = true;
        unchanged(&source)?;
        // This explicit synthetic trust is test authorization, never a GUI action.
        all_clients_reaped = false;
        client = Some(Client::spawn_agent(&binary, &root, true)?);
        let client = client.as_mut().unwrap();
        require_java_capabilities(client)?;
        record.java_capabilities = true;
        stage.set(FailureStage::Initialize);
        let initialize_started = Instant::now();
        let initialized = client_language(client, start_operation()?)?;
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
        observed = Some(RootObservation::open_current(1, pid)?);
        record.root_observed_live = true;
        verify_java_image(observed.as_ref().unwrap(), &java)?;
        record.root_identity_verified = true;
        resource_latency(
            profile,
            started,
            initialize_started,
            ResourceLatency::JavaInitialize,
        );
        resource_phase(started, ResourcePhase::JavaInitialized);
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
        production_diagnostics(client, &uri, DiagnosticPhase::Initial)?;
        record.semantic_diagnostics = true;
        unchanged(&source)?;
        resource_latency(
            profile,
            started,
            open_started,
            ResourceLatency::OpenExactDiagnostics,
        );
        if resource_phase(started, ResourcePhase::SemanticReadyIdle)
            || profile == ObservationProfile::ResourceBaseline
        {
            // Defined observation interval after exact diagnostics, not a claim
            // that JDT indexing or other background work has fully settled.
            thread::sleep(Duration::from_secs(match profile {
                ObservationProfile::Quick => 2,
                ObservationProfile::ResourceBaseline => 30,
            }));
        }
        resource_phase(started, ResourcePhase::QueryWorkload);
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
        production_diagnostics(client, &uri, DiagnosticPhase::Correction)?;
        record.correction_diagnostics = true;
        unchanged(&source)?;
        resource_latency(
            profile,
            started,
            correction_started,
            ResourceLatency::CorrectionExactDiagnostics,
        );
        if profile == ObservationProfile::ResourceBaseline {
            resource_phase(started, ResourcePhase::CorrectionReadyIdle);
            // A second fixed observation window, not an indexing-settled claim.
            thread::sleep(Duration::from_secs(30));
            resource_phase(started, ResourcePhase::Closing);
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
    resource_phase(started, ResourcePhase::Cleanup);
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
            let actual = observed
                .as_ref()
                .ok_or("normal Java observer missing")?
                .exit_code_with_timeout(3000)?;
            record.root_handle_signaled = true;
            record.root_exit_code = Some(actual);
            require(
                actual == outcome.root_exit_code,
                "typed stop and actual retained process exit disagree",
            )?;
            record.stop_outcome_verified = true;
            require(
                outcome.status != StopStatus::Error,
                "normal Java stop reported cleanup errors",
            )?;
            resource_latency(
                profile,
                started,
                stop_started,
                ResourceLatency::StopVerifiedRootExit,
            );
            Ok(())
        });
        if let Err(error) = stop {
            failure_stage.get_or_insert(stage.get());
            cleanup_errors.push(error);
        }
    }
    if let Some(client) = client.take() {
        match checked(|| client.close_and_wait(EXIT_TIMEOUT)) {
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
        Some(process) => match process.exit_code_with_timeout(3000) {
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
    record.primary_failed = primary.is_err();
    record.cleanup_failed = !cleanup_errors.is_empty();
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
        && record.correction_diagnostics
        && record.source_unchanged
        && record.stop_outcome_verified
        && record.cleanup_joined
        && record.root_handle_signaled
        && record.client_reaped
        && record.synthetic_root_removed;
    record.failure_stage = if record.success {
        FailureStage::None
    } else {
        failure_stage.unwrap_or(FailureStage::Setup)
    };
    let elapsed = started.elapsed().as_millis();
    record.elapsed_ms = elapsed.min(300_000) as u32;
    record.elapsed_saturated = elapsed > 300_000;
    println!(
        "{}",
        serde_json::to_string(&record).expect("typed normal Java evidence")
    );
    resource_phase(started, ResourcePhase::Complete);
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
