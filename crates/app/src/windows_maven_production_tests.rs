//! Finite, opt-in native Windows Maven acceptance through the shipping agent and
//! normal capability-enforcing Client. This tests one generated leaf with its
//! dependency present and one fresh leaf with that dependency missing. It does
//! not authorize user projects, run Maven goals, establish GUI trust, or claim
//! network isolation: JDT can still request public Gradle version metadata.
use super::*;
use crate::{
    editor_state,
    java_language::{JavaRootExit, JavaStopOutcome, StopReason, StopStatus},
    model::Document,
};
use cedar_client::Client;
use cedar_protocol::{
    MavenDependenciesSnapshot, MavenDependencyObservation, MavenDependencyScope, MavenLibraryRoot,
    JAVA_MAVEN_DEPENDENCIES_CAPABILITY,
};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, os::windows::fs::MetadataExt};
use windows_sys::Win32::System::Threading::QueryFullProcessImageNameW;

// The backend bounds its single JDT insight query to five seconds. The normal
// Client still permits a 75-second active Java RPC (30 seconds without a
// session); admission reserves that full wire envelope, never a new timeout.
const DEPENDENCY_RPC_BUDGET: Duration = Duration::from_secs(75);
const NO_SESSION_RPC_BUDGET: Duration = Duration::from_secs(30);
const MODEL_BUDGET: Duration = Duration::from_secs(60);
const MODEL_INTERVAL: Duration = Duration::from_millis(250);
const MAX_MODEL_QUERIES: u16 = 240;
const SEMANTIC_BUDGET: Duration = Duration::from_secs(20);
const PAIR_BUDGET: Duration = Duration::from_secs(360);
// Test observation only: the normal Client already allows 75 seconds for Stop.
// Allow another 15 seconds for verification/reaping, without changing the
// production shutdown grace, RPC timeout, or the 360-second pair watchdog.
const CLEANUP_BUDGET: Duration = Duration::from_secs(90);
const CLIENT_REAP_BUDGET: Duration = Duration::from_secs(10);
const CACHE_FILES: usize = 83;
const SOURCE_FILE: &str = "source-java/demo/Main.java";
const DEPENDENCY_DIRECTORY: &str = "dev/cedar/fixture/arithmetic/1.0.0";
const DEPENDENCY_JAR: &str = "dev/cedar/fixture/arithmetic/1.0.0/arithmetic-1.0.0.jar";
const DEPENDENCY_POM: &str = "dev/cedar/fixture/arithmetic/1.0.0/arithmetic-1.0.0.pom";
const DEPENDENCY_GAV: &str = "dev.cedar.fixture:arithmetic:jar:1.0.0";
const JAR_SHA256: &str = "82579c654968c77f0bd3d04c28a22b24396c35270ce76d015807410438952b5d";
const SOURCE_SHA256: &str = "5dda0de22c2184b1420be8e68f8a37e9165b59658d5c5cbf9fe2ee770a1003e5";
const POM_SHA256: &str = "c13116c2a4d7dd73f28f604480b6aad3ce818a11db526b7f61737c1c3864b65b";
const FIXTURE_SOURCE: &str = "package demo;\nimport cedar.fixture.Arithmetic;\npublic class Main {\n    public static void main(String[] args) {\n        int known = Arithmetic.answer();\n        String invalid = known;\n        System.out.println(invalid);\n    }\n}\n";
const FIXTURE_POM: &str = "<project xmlns=\"http://maven.apache.org/POM/4.0.0\">\n  <modelVersion>4.0.0</modelVersion>\n  <groupId>dev.cedar.feasibility</groupId>\n  <artifactId>leaf-project</artifactId>\n  <version>1.0.0</version>\n  <properties>\n    <maven.compiler.release>17</maven.compiler.release>\n    <maven.compiler.source>17</maven.compiler.source>\n    <maven.compiler.target>17</maven.compiler.target>\n    <project.build.sourceEncoding>UTF-8</project.build.sourceEncoding>\n  </properties>\n  <build><sourceDirectory>source-java</sourceDirectory></build>\n  <dependencies>\n    <dependency>\n      <groupId>dev.cedar.fixture</groupId>\n      <artifactId>arithmetic</artifactId>\n      <version>1.0.0</version>\n    </dependency>\n  </dependencies>\n</project>\n";
const ARTIFACT_POM: &str = "<project xmlns=\"http://maven.apache.org/POM/4.0.0\">\n  <modelVersion>4.0.0</modelVersion>\n  <groupId>dev.cedar.fixture</groupId>\n  <artifactId>arithmetic</artifactId>\n  <version>1.0.0</version>\n</project>\n";

// Deterministic stored ZIP (1980 timestamp, one Java 17 class, no manifest or
// executable entry point), generated solely from this reviewed source:
// package cedar.fixture;
// public final class Arithmetic {
//     public static int answer() { return 42; }
// }
// The JDK 21 compiler used -proc:none -source 17 -target 17, producing classfile
// version 61 (264 bytes); this does not claim a Java 17 platform API check.
// Acceptance decodes and hashes the sealed 422-byte JAR; it never invokes
// javac, Maven, a wrapper, or a build.
const JAR_HEX: &str = concat!(
    "504b0304140000000000000021003be39e7508010000080100001e00000063656461722f666978747572652f41726974686d657469632e636c617373",
    "cafebabe0000003d000f0a000200030700040c000500060100106a6176612f6c616e672f4f626a6563740100063c696e69743e01000328295607000801001863656461722f666978747572652f41726974686d65746963010004436f646501000f4c696e654e756d6265725461626c65010006616e7377657201000328294901000a536f7572636546696c6501000f41726974686d657469632e6a617661003100070002000000000002000100050006000100090000001d00010001000000052ab70001b100000001000a000000060001000000020009000b000c000100090000001b0001000000000003102aac00000001000a000000060001000000030001000d00000002000e",
    "504b01021403140000000000000021003be39e7508010000080100001e0000000000000000000000a4810000000063656461722f666978747572652f41726974686d657469632e636c617373504b050600000000010001004c000000440100000000"
);

#[derive(Clone, Copy, Default, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum CaseKind {
    #[default]
    Present,
    Missing,
}
impl CaseKind {
    fn present(self) -> bool {
        matches!(self, Self::Present)
    }
}

fn case_budget(kind: CaseKind) -> Duration {
    START_TIMEOUT + MODEL_BUDGET + semantic_budget(kind) + CLEANUP_BUDGET
}

fn semantic_budget(kind: CaseKind) -> Duration {
    if kind.present() {
        SEMANTIC_BUDGET
    } else {
        Duration::ZERO
    }
}

fn budget_admits(now: Instant, deadline: Instant, work: Duration, reserve: Duration) -> bool {
    now < deadline && deadline.saturating_duration_since(now) >= work + reserve
}

fn admit_work(deadline: Instant, work: Duration, reserve: Duration) -> CheckResult<()> {
    require(
        budget_admits(Instant::now(), deadline, work, reserve),
        "Maven pair has insufficient remaining observation and cleanup budget",
    )
}

fn bounded_cleanup_deadline(now: Instant, pair_deadline: Instant) -> Instant {
    (now + CLEANUP_BUDGET).min(pair_deadline)
}

fn prior_cleanup_verified(record: &CaseEvidence) -> bool {
    !record.cleanup_failed
        && record.stop_outcome_verified
        && record.cleanup_joined
        && record.root_handle_signaled
        && record.root_exit_code.is_some()
        && record.client_reaped
        && record.synthetic_root_removed
}
#[derive(Clone, Copy, Default, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum Stage {
    #[default]
    Setup,
    Trust,
    Startup,
    Model,
    Dependencies,
    Semantics,
    PomChange,
    Stop,
    RootExit,
    ClientExit,
    FixtureCleanup,
    None,
}
#[derive(Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum ModelStatus {
    #[default]
    Unavailable,
    Imported,
    Unresolved,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum ModelProbeOutcome {
    #[default]
    NotAttempted,
    RequestFailed,
    NonLanguagePayload,
    ResponseReceived,
    ModelRejected,
    NotReady,
    Ready,
    BudgetExhausted,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum DependencyProbeOutcome {
    #[default]
    NotAttempted,
    RequestFailed,
    NonDependencyPayload,
    ResponseReceived,
    SnapshotRejected,
    Accepted,
    BudgetExhausted,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum DependencyObservation {
    #[default]
    NotAttempted,
    Unavailable,
    ObservedPresentFile,
    ObservedAbsentFile,
    NotObserved,
    Rejected,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum ClientErrorCode {
    #[default]
    None,
    UnsupportedOperation,
    RunDisabled,
    LanguageNotRunning,
    LanguageMavenSessionRequired,
    LanguageMavenUnsupported,
    LanguageMavenRestartRequired,
    LanguageMavenInvalidModel,
    LanguageMavenInvalidDependencies,
    LanguageMavenStaleSnapshot,
    TransportFailure,
    Other,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum ModelRejection {
    #[default]
    None,
    ProfileOrPom,
    Status,
    MissingClasspath,
    ClasspathBound,
    MissingSourcePath,
    EscapedSourcePath,
    DependencyResolution,
    DependencyOrigin,
    EntryKind,
    ForeignOrDuplicateReference,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum EventProbeOutcome {
    #[default]
    NotAttempted,
    RequestFailed,
    NonLanguagePayload,
    ResponseReceived,
    EventsAccepted,
    EventsRejected,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum EventRejection {
    #[default]
    None,
    Truncated,
    MissingEvents,
    UnknownEvent,
    ClosedEvent,
    LaggedEvent,
    MissingEventType,
    MissingDiagnosticUri,
    MissingDiagnostics,
    UnexpectedPomDiagnostic,
    UnexpectedSourceDiagnostic,
    UnexpectedProjectDiagnostic,
    ForeignDocument,
    UriEncoding,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum DiagnosticOrigin {
    #[default]
    None,
    Pom,
    Source,
    OwnedProjectRoot,
    Foreign,
    Missing,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum DiagnosticCodeShape {
    #[default]
    None,
    Missing,
    StringZero,
    StringTypeMismatch,
    StringInvalidClasspath,
    OtherString,
    IntegerZero,
    IntegerTypeMismatch,
    OtherInteger,
    Other,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum DiagnosticSeverity {
    #[default]
    None,
    Missing,
    Error,
    Warning,
    Information,
    Hint,
    Other,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum DiagnosticMessageClass {
    #[default]
    None,
    Missing,
    OfflineOwnedDependency,
    PlainMissingOwnedDependency,
    DeliberateIntToString,
    UnresolvedCedarImport,
    UnresolvedArithmetic,
    OwnedMissingMavenLibrary,
    Other,
}

// Only fixed tags, booleans, bounded counters and native exit status leave this
// test. No event payloads, source text, private paths or server messages appear
// in the receipt. The collector must independently require both cases.
#[derive(Default, serde::Serialize)]
struct CaseEvidence {
    case: CaseKind,
    failure_stage: Stage,
    model_status: ModelStatus,
    model_queries: u16,
    model_probe_outcome: ModelProbeOutcome,
    model_error_code: ClientErrorCode,
    model_rejection: ModelRejection,
    event_probe_outcome: EventProbeOutcome,
    event_error_code: ClientErrorCode,
    event_rejection: EventRejection,
    rejected_diagnostic_origin: DiagnosticOrigin,
    rejected_diagnostic_code_shape: DiagnosticCodeShape,
    rejected_diagnostic_severity: DiagnosticSeverity,
    rejected_diagnostic_message_class: DiagnosticMessageClass,
    java_capabilities: bool,
    dependency_capability_advertised: bool,
    dependency_optional_capability_rejected: bool,
    dependencies_untrusted_rejected: bool,
    dependencies_without_session_rejected: bool,
    dependency_queries: u8,
    dependency_probe_outcome: DependencyProbeOutcome,
    dependency_error_code: ClientErrorCode,
    dependency_snapshot_identity_verified: bool,
    dependency_declaration_count: u16,
    dependency_declaration_exact: bool,
    dependency_default_provenance_verified: bool,
    dependency_expected_jar_verified: bool,
    dependency_declaration_file_present: bool,
    dependency_observed_library_count: u16,
    dependency_observation: DependencyObservation,
    dependency_frontend_identity_verified: bool,
    dependency_frontend_invalidated: bool,
    dependency_dirty_undo_preserved: bool,
    dependencies_changed_pom_restart_required: bool,
    dependencies_after_stop_rejected: bool,
    generic_start_rejected: bool,
    untrusted_start_rejected: bool,
    model_without_session_rejected: bool,
    async_start_begin_acknowledged: bool,
    async_start_read_while_starting: bool,
    async_start_ready: bool,
    root_identity_verified: bool,
    root_observed_live: bool,
    maven_nature: bool,
    custom_source: bool,
    compiler_17: bool,
    exact_dependency_reference: bool,
    unexpected_dependency_references: u16,
    offline_pom_diagnostic: bool,
    owned_project_missing_library_diagnostic: bool,
    hover: bool,
    completion: bool,
    deliberate_type_diagnostic: bool,
    dirty_change_acknowledged: bool,
    no_autosave: bool,
    stale_startup_rejected: bool,
    changed_pom_restart_required: bool,
    source_unchanged: bool,
    pom_expected: bool,
    repository_inputs_unchanged: bool,
    dependency_jar_present_before: bool,
    dependency_jar_present_after: bool,
    dependency_pom_present_before: bool,
    dependency_pom_present_after: bool,
    generated_metadata_files: u16,
    foreign_repository_files: u16,
    lifecycle_metadata_files: u8,
    lifecycle_metadata_mask: u8,
    generated_data_files: u32,
    generated_data_bytes: u64,
    generated_project_files: u16,
    generated_project_bytes: u64,
    stop_status: Option<StopStatus>,
    stop_reason: Option<StopReason>,
    cleanup_joined: bool,
    shutdown_response_received: bool,
    exit_frame_completed: bool,
    stop_outcome_verified: bool,
    root_handle_signaled: bool,
    root_exit_code: Option<u32>,
    model_after_stop_rejected: bool,
    client_reaped: bool,
    synthetic_root_removed: bool,
    primary_failed: bool,
    cleanup_failed: bool,
    success: bool,
}

#[derive(serde::Serialize)]
struct MavenEvidence {
    schema_version: u8,
    kind: &'static str,
    route: &'static str,
    pair_count: u8,
    cache_input_files: u16,
    cache_input_unchanged: bool,
    fixture_inputs_verified: bool,
    source_sha256: &'static str,
    pom_sha256: &'static str,
    dependency_jar_sha256: &'static str,
    present: CaseEvidence,
    missing: CaseEvidence,
    elapsed_ms: u32,
    elapsed_saturated: bool,
    primary_failed: bool,
    cleanup_failed: bool,
    success: bool,
}

#[derive(Clone, PartialEq, Eq)]
struct Seal {
    bytes: u64,
    sha256: String,
}
type Inventory = BTreeMap<PathBuf, Seal>;
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn snapshot(root: &Path) -> CheckResult<Inventory> {
    let mut directories = vec![root.to_path_buf()];
    let mut inventory = Inventory::new();
    let mut entries = 0usize;
    let mut bytes = 0u64;
    while let Some(directory) = directories.pop() {
        for entry in io(fs::read_dir(directory))? {
            entries += 1;
            require(
                entries <= 4096,
                "Maven cache inventory exceeded its entry bound",
            )?;
            let entry = io(entry)?;
            let path = entry.path();
            let metadata = io(fs::symlink_metadata(&path))?;
            require(
                !metadata.file_type().is_symlink() && metadata.file_attributes() & 0x400 == 0,
                "Maven cache inventory contains a link or reparse point",
            )?;
            if metadata.is_dir() {
                directories.push(path);
                continue;
            }
            require(
                metadata.is_file() && metadata.len() <= 8 * 1024 * 1024,
                "Maven cache input is not a bounded ordinary file",
            )?;
            bytes += metadata.len();
            require(
                bytes <= 16 * 1024 * 1024 && inventory.len() < 1024,
                "Maven cache inventory exceeded its byte or file bound",
            )?;
            let contents = io(fs::read(&path))?;
            require(
                contents.len() as u64 == metadata.len(),
                "Maven cache changed while reading",
            )?;
            let relative = path
                .strip_prefix(root)
                .map_err(|_| "Maven cache escaped its root")?
                .to_path_buf();
            inventory.insert(
                relative,
                Seal {
                    bytes: metadata.len(),
                    sha256: hash(&contents),
                },
            );
        }
    }
    Ok(inventory)
}

fn absent(path: &Path) -> CheckResult<bool> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(_) => Err("Maven fixture absence could not be verified".into()),
        Ok(_) => Ok(false),
    }
}
fn sealed_jar() -> CheckResult<Vec<u8>> {
    let bytes = JAR_HEX
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let pair = std::str::from_utf8(pair).map_err(|_| "invalid sealed fixture encoding")?;
            u8::from_str_radix(pair, 16).map_err(|_| "invalid sealed fixture byte")
        })
        .collect::<Result<Vec<_>, _>>()?;
    require(
        bytes.len() == 422 && hash(&bytes) == JAR_SHA256,
        "sealed generated JAR hash mismatch",
    )?;
    Ok(bytes)
}
fn language_value(client: &mut Client, op: Operation) -> CheckResult<Value> {
    match client.request(op)? {
        Payload::Language { value } => Ok(value),
        _ => Err("Maven acceptance received a non-language response".into()),
    }
}

fn client_error_code(error: &str) -> ClientErrorCode {
    // RemoteError Display prefixes the code before ':'. Never search a server
    // message for code-shaped substrings or export the remainder of the error.
    let code = error.split_once(':').map_or(error, |(code, _)| code);
    match code {
        "unsupported_operation" => ClientErrorCode::UnsupportedOperation,
        "run_disabled" => ClientErrorCode::RunDisabled,
        "language_not_running" => ClientErrorCode::LanguageNotRunning,
        "language_maven_session_required" => ClientErrorCode::LanguageMavenSessionRequired,
        "language_maven_unsupported" => ClientErrorCode::LanguageMavenUnsupported,
        "language_maven_restart_required" => ClientErrorCode::LanguageMavenRestartRequired,
        "language_maven_invalid_model" => ClientErrorCode::LanguageMavenInvalidModel,
        "language_maven_invalid_dependencies" => ClientErrorCode::LanguageMavenInvalidDependencies,
        "language_maven_stale_snapshot" => ClientErrorCode::LanguageMavenStaleSnapshot,
        "transport_cancelled"
        | "transport_write"
        | "transport_eof"
        | "transport_read"
        | "transport_timeout"
        | "transport_close"
        | "transport_cleanup_unverified"
        | "disconnected"
        | "protocol_error" => ClientErrorCode::TransportFailure,
        _ => ClientErrorCode::Other,
    }
}

fn model_payload(response: CheckResult<Payload>, record: &mut CaseEvidence) -> CheckResult<Value> {
    record.model_error_code = ClientErrorCode::None;
    record.model_rejection = ModelRejection::None;
    match response {
        Ok(Payload::Language { value }) => {
            record.model_probe_outcome = ModelProbeOutcome::ResponseReceived;
            Ok(value)
        }
        Ok(_) => {
            record.model_probe_outcome = ModelProbeOutcome::NonLanguagePayload;
            Err("Maven acceptance received a non-language response".into())
        }
        Err(error) => {
            record.model_probe_outcome = ModelProbeOutcome::RequestFailed;
            record.model_error_code = client_error_code(&error);
            Err(error)
        }
    }
}

fn clear_rejected_diagnostic(record: &mut CaseEvidence) {
    record.rejected_diagnostic_origin = DiagnosticOrigin::None;
    record.rejected_diagnostic_code_shape = DiagnosticCodeShape::None;
    record.rejected_diagnostic_message_class = DiagnosticMessageClass::None;
    record.rejected_diagnostic_severity = DiagnosticSeverity::None;
}

fn event_payload(response: CheckResult<Payload>, record: &mut CaseEvidence) -> CheckResult<Value> {
    record.event_error_code = ClientErrorCode::None;
    record.event_rejection = EventRejection::None;
    clear_rejected_diagnostic(record);
    match response {
        Ok(Payload::Language { value }) => {
            record.event_probe_outcome = EventProbeOutcome::ResponseReceived;
            Ok(value)
        }
        Ok(_) => {
            record.event_probe_outcome = EventProbeOutcome::NonLanguagePayload;
            Err("Maven acceptance received a non-language response".into())
        }
        Err(error) => {
            record.event_probe_outcome = EventProbeOutcome::RequestFailed;
            record.event_error_code = client_error_code(&error);
            Err(error)
        }
    }
}
fn rejected(client: &mut Client, op: Operation, code: &str) -> CheckResult<()> {
    require(
        client.request(op).is_err_and(|error| {
            error
                .split_once(':')
                .map_or(error.as_str(), |(actual, _)| actual)
                == code
        }),
        "Maven acceptance did not receive the expected typed rejection",
    )
}

fn verify_image(process: &RootObservation, java: &Path) -> CheckResult<()> {
    let mut image = vec![0u16; 32_768];
    let mut length = image.len() as u32;
    // SAFETY: The common Java fixture retained this owned root's query-only
    // process handle; this observes its exact image without searching or killing.
    require(
        unsafe {
            QueryFullProcessImageNameW(
                process.handle.as_raw_handle(),
                0,
                image.as_mut_ptr(),
                &mut length,
            )
        } != 0,
        "Maven root image query failed",
    )?;
    let image =
        String::from_utf16(&image[..length as usize]).map_err(|_| "invalid Maven root image")?;
    require(
        ordinary_path(Path::new(&image))? == ordinary_path(java)?
            && process.created != 0
            && process.live()?,
        "Maven root image or live identity mismatch",
    )
}

fn capabilities(client: &Client) -> CheckResult<()> {
    let Payload::Hello {
        agent: Some(info), ..
    } = client.handshake()
    else {
        return Err("Maven acceptance agent metadata missing".into());
    };
    require(
        !info.supports("language_start")
            && JAVA_LANGUAGE_SESSION_CAPABILITIES
                .iter()
                .all(|name| info.supports(name))
            && info.supports("language_start_java_maven_begin")
            && info.supports("language_maven_model")
            && info.supports(JAVA_MAVEN_DEPENDENCIES_CAPABILITY),
        "normal Windows agent Maven capabilities incomplete",
    )
}

fn await_start(
    client: &mut Client,
    id: u64,
    java: &Path,
    observed: &mut Option<RootObservation>,
    deadline: Instant,
) -> CheckResult<Value> {
    while Instant::now() < deadline {
        let value = language_value(client, Operation::LanguageStartJavaPoll { startup_id: id })?;
        require(
            Instant::now() < deadline,
            "Maven startup poll exceeded its original budget",
        )?;
        require(value["startup_id"] == id, "Maven startup identity changed")?;
        if observed.is_none() {
            if let Some(pid) = value["process_id"]
                .as_u64()
                .and_then(|pid| u32::try_from(pid).ok())
            {
                let process = RootObservation::open_current(1, pid)?;
                verify_image(&process, java)?;
                *observed = Some(process);
            }
        }
        match value["state"].as_str() {
            Some("ready") => return Ok(value["language"].clone()),
            Some("starting") => {}
            _ => return Err("Maven startup failed or returned an invalid state".into()),
        }
        thread::sleep(Duration::from_millis(50));
    }
    Err("Maven startup exceeded its 75-second budget".into())
}

fn cancel_start(client: &mut Client, id: u64, deadline: Instant) -> CheckResult<()> {
    let mut value = language_value(
        client,
        Operation::LanguageStartJavaCancel { startup_id: id },
    )?;
    loop {
        require(
            value["startup_id"] == id,
            "Maven startup cancellation identity mismatch",
        )?;
        match value["state"].as_str() {
            Some("cancelled" | "failed") => {
                return require(
                    value["cleanup_verified"] == true,
                    "Maven cancellation cleanup not verified",
                )
            }
            Some("cancelling") => {}
            _ => return Err("Maven startup cancellation returned an invalid state".into()),
        }
        require(
            Instant::now() < deadline,
            "Maven startup cancellation exceeded cleanup budget",
        )?;
        thread::sleep(Duration::from_millis(50));
        value = language_value(client, Operation::LanguageStartJavaPoll { startup_id: id })?;
    }
}

fn verified_cache() -> CheckResult<(PathBuf, Inventory)> {
    let root = environment_path("CEDAR_MAVEN_CACHE_INPUT")?;
    require(
        root.is_dir(),
        "Maven cache input must be an existing directory",
    )?;
    let inventory = snapshot(&root)?;
    // The public provisioning manifest contains only pinned public artifacts.
    // The imported leaf never provisions these files or contacts a repository.
    let manifest: Value =
        serde_json::from_str(include_str!("../../../scripts/maven_cache_manifest.json"))
            .map_err(|_| "invalid public Maven cache manifest")?;
    let entries = manifest["entries"]
        .as_array()
        .ok_or("Maven manifest entries missing")?;
    require(
        entries.len() == CACHE_FILES
            && inventory.len() == CACHE_FILES
            && manifest["expected_files"] == CACHE_FILES as u64
            && manifest["expected_bytes"] == 4_065_288u64,
        "Maven cache does not match the exact public 83-file input contract",
    )?;
    let mut declared = Inventory::new();
    for entry in entries {
        let path = PathBuf::from(
            entry["path"]
                .as_str()
                .ok_or("cache manifest path missing")?,
        );
        require(
            !path.is_absolute()
                && path
                    .components()
                    .all(|part| matches!(part, Component::Normal(_))),
            "cache manifest path is not confined",
        )?;
        let seal = Seal {
            bytes: entry["bytes"]
                .as_u64()
                .ok_or("cache manifest size missing")?,
            sha256: entry["sha256"]
                .as_str()
                .ok_or("cache manifest hash missing")?
                .into(),
        };
        require(
            declared.insert(path, seal).is_none(),
            "duplicate cache manifest file",
        )?;
    }
    require(
        inventory == declared
            && inventory.values().map(|seal| seal.bytes).sum::<u64>() == 4_065_288,
        "Maven cache paths, sizes or hashes differ from the pinned manifest",
    )?;
    require(
        absent(&root.join(DEPENDENCY_JAR))? && absent(&root.join(DEPENDENCY_POM))?,
        "public cache input unexpectedly contains the generated dependency",
    )?;
    Ok((root, inventory))
}

fn copy_cache(input: &Path, destination: &Path, inventory: &Inventory) -> CheckResult<()> {
    use std::io::Write;

    for (relative, expected) in inventory {
        require(
            expected.bytes <= 8 * 1024 * 1024,
            "pinned cache input exceeds its bounded read limit",
        )?;
        let source = input.join(relative);
        let metadata = io(fs::symlink_metadata(&source))?;
        require(
            metadata.is_file()
                && !metadata.file_type().is_symlink()
                && metadata.file_attributes() & 0x400 == 0
                && metadata.len() == expected.bytes,
            "pinned cache input changed before copying",
        )?;
        let mut contents = Vec::new();
        io(io(fs::File::open(source))?
            .take(expected.bytes + 1)
            .read_to_end(&mut contents))?;
        require(
            contents.len() as u64 == expected.bytes && hash(&contents) == expected.sha256,
            "cache source differs from its pinned input",
        )?;
        let target = destination.join(relative);
        io(fs::create_dir_all(
            target.parent().ok_or("cache destination parent missing")?,
        ))?;
        require(absent(&target)?, "cache destination was not fresh")?;
        // Create a fresh writable owned file without copying readonly attributes
        // or mutating any permissions. The sealed source remains untouched.
        let mut file = io(fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(target))?;
        io(file.write_all(&contents))?;
    }
    Ok(())
}

struct CasePaths {
    root: PathBuf,
    repository: PathBuf,
    data: PathBuf,
}
fn setup_case(
    base: &Path,
    input: &Path,
    cache: &Inventory,
    kind: CaseKind,
) -> CheckResult<(CasePaths, Inventory)> {
    require(
        text(base)?.is_ascii(),
        "Maven control/data fixture parent must be ASCII",
    )?;
    let paths = CasePaths {
        root: base.join("workspace 雪"),
        repository: base.join("repository 雪"),
        data: base.join("data"),
    };
    io(fs::create_dir_all(paths.root.join("source-java/demo")))?;
    io(fs::create_dir(&paths.repository))?;
    io(fs::create_dir(&paths.data))?;
    io(fs::write(paths.root.join(SOURCE_FILE), FIXTURE_SOURCE))?;
    io(fs::write(paths.root.join("pom.xml"), FIXTURE_POM))?;
    copy_cache(input, &paths.repository, cache)?;
    if kind.present() {
        io(fs::create_dir_all(
            paths.repository.join(DEPENDENCY_DIRECTORY),
        ))?;
        io(fs::write(
            paths.repository.join(DEPENDENCY_JAR),
            sealed_jar()?,
        ))?;
        io(fs::write(
            paths.repository.join(DEPENDENCY_POM),
            ARTIFACT_POM,
        ))?;
    }
    for marker in [
        ".cedar-windows-language-validation",
        ".cedar-windows-java-validation",
        ".cedar-windows-java-gc-diagnostic",
        ".project",
        ".classpath",
        ".settings",
    ] {
        require(
            absent(&paths.root.join(marker))?,
            "Maven leaf fixture contains preexisting execution or project markers",
        )?;
    }
    let copied = snapshot(&paths.repository)?;
    require(
        copied.len() == CACHE_FILES + if kind.present() { 2 } else { 0 },
        "Maven case cache file count mismatch",
    )?;
    Ok((paths, copied))
}

fn source_unchanged(paths: &CasePaths) -> bool {
    fs::read(paths.root.join(SOURCE_FILE))
        .is_ok_and(|bytes| hash(&bytes) == hash(FIXTURE_SOURCE.as_bytes()))
}
fn fresh_control_directory(paths: &CasePaths) -> CheckResult<PathBuf> {
    let data_metadata = io(fs::symlink_metadata(&paths.data))?;
    require(
        data_metadata.is_dir()
            && !data_metadata.file_type().is_symlink()
            && data_metadata.file_attributes() & 0x400 == 0
            && text(&paths.data)?.is_ascii()
            && ordinary_path(&paths.data)? == paths.data,
        "Maven selected data directory identity changed",
    )?;
    let entries = io(fs::read_dir(&paths.data))?
        .take(2)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "control directory read failed")?;
    require(
        entries.len() == 1,
        "Maven controls must use exactly one fresh owned directory",
    )?;
    let entry = &entries[0];
    let control = entry.path();
    let metadata = io(fs::symlink_metadata(&control))?;
    require(
        metadata.is_dir()
            && !metadata.file_type().is_symlink()
            && metadata.file_attributes() & 0x400 == 0
            && entry.file_name().to_str().is_some_and(|name| {
                name.strip_prefix("cedar-maven-").is_some_and(|suffix| {
                    !suffix.is_empty()
                        && suffix.len() <= 64
                        && suffix.bytes().all(|byte| byte.is_ascii_alphanumeric())
                })
            })
            && text(&control)?.is_ascii()
            && ordinary_path(&control)? == control,
        "Maven control must be the one fresh ordinary Cedar-owned child",
    )?;
    Ok(control)
}

fn verify_control(paths: &CasePaths) -> CheckResult<()> {
    let control = fresh_control_directory(paths)?;
    require(
        control.join("home").is_dir()
            && control.join("tmp").is_dir()
            && control.join("jdt-data/.metadata").is_dir()
            && control.join("user-settings.xml").is_file()
            && control.join("global-settings.xml").is_file(),
        "Maven data, controls or isolated home layout mismatch",
    )
}

fn owned_mirror_uri(paths: &CasePaths) -> CheckResult<String> {
    // Derive authority from the fixture's sole fresh control subtree, never
    // from a URL or filename supplied by a generated cache file.
    let mirror = fresh_control_directory(paths)?.join("empty-mirror");
    let metadata = io(fs::symlink_metadata(&mirror))?;
    require(
        metadata.is_dir()
            && !metadata.file_type().is_symlink()
            && metadata.file_attributes() & 0x400 == 0
            && ordinary_path(&mirror)? == mirror
            && io(fs::read_dir(&mirror))?.next().is_none(),
        "Maven owned file mirror must remain an empty ordinary directory",
    )?;
    directory_url(&mirror)
}

fn same_path(actual: &str, expected: &Path) -> bool {
    expected.to_str().is_some_and(|expected| {
        actual
            .replace('\\', "/")
            .eq_ignore_ascii_case(&expected.replace('\\', "/"))
    })
}
fn inspect_model(value: &Value, paths: &CasePaths, record: &mut CaseEvidence) -> CheckResult<bool> {
    record.model_rejection = ModelRejection::None;
    let result = inspect_model_inner(value, paths, record);
    match &result {
        Ok(true) => record.model_probe_outcome = ModelProbeOutcome::Ready,
        Ok(false) => record.model_probe_outcome = ModelProbeOutcome::NotReady,
        Err(error) => {
            record.model_probe_outcome = ModelProbeOutcome::ModelRejected;
            record.model_rejection = match error.as_str() {
                "Maven model profile or owned POM hash mismatch" => ModelRejection::ProfileOrPom,
                "invalid Maven model status" => ModelRejection::Status,
                "Maven classpath missing" => ModelRejection::MissingClasspath,
                "Maven model exceeded classpath bound" => ModelRejection::ClasspathBound,
                "Maven source path missing" => ModelRejection::MissingSourcePath,
                "Maven source path escaped the generated leaf" => ModelRejection::EscapedSourcePath,
                "Maven dependency resolution disagrees with actual case cache" => {
                    ModelRejection::DependencyResolution
                }
                "Maven dependency reference has an unexpected origin" => {
                    ModelRejection::DependencyOrigin
                }
                "unexpected Maven classpath entry kind" => ModelRejection::EntryKind,
                "Maven model included a foreign or extra dependency reference" => {
                    ModelRejection::ForeignOrDuplicateReference
                }
                _ => ModelRejection::None,
            };
        }
    }
    result
}

fn inspect_model_inner(
    value: &Value,
    paths: &CasePaths,
    record: &mut CaseEvidence,
) -> CheckResult<bool> {
    require(
        value["profile"] == "maven_leaf"
            && value["pom_path"] == "pom.xml"
            && value["pom_sha256"] == hash(FIXTURE_POM.as_bytes())
            && value["restart_required"] == false,
        "Maven model profile or owned POM hash mismatch",
    )?;
    record.model_status = match value["status"].as_str() {
        Some("imported") => ModelStatus::Imported,
        Some("unresolved") => ModelStatus::Unresolved,
        Some("unavailable") => ModelStatus::Unavailable,
        _ => return Err("invalid Maven model status".into()),
    };
    record.maven_nature = value["maven_nature"] == true;
    record.custom_source = value["source_paths"]
        .as_array()
        .is_some_and(|paths| paths.iter().any(|path| path == "source-java"));
    record.compiler_17 = value["compiler"]["source"] == "17"
        && value["compiler"]["compliance"] == "17"
        && value["compiler"]["target"] == "17";
    if !(record.maven_nature && record.custom_source && record.compiler_17) {
        return Ok(false);
    }
    let entries = value["classpath"]
        .as_array()
        .ok_or("Maven classpath missing")?;
    require(entries.len() <= 256, "Maven model exceeded classpath bound")?;
    let mut expected_count = 0;
    let mut foreign_count = 0;
    for entry in entries {
        match entry["kind"].as_str() {
            Some("source") => {
                let source = entry["path"].as_str().ok_or("Maven source path missing")?;
                let relative = Path::new(source);
                require(
                    !relative.is_absolute()
                        && relative
                            .components()
                            .all(|part| matches!(part, Component::Normal(_))),
                    "Maven source path escaped the generated leaf",
                )?;
            }
            Some("library") => {
                if entry["path"]
                    .as_str()
                    .is_some_and(|path| same_path(path, &paths.repository.join(DEPENDENCY_JAR)))
                {
                    expected_count += 1;
                    require(
                        entry["resolved"] == record.case.present(),
                        "Maven dependency resolution disagrees with actual case cache",
                    )?;
                    require(
                        entry["origin"] == "model"
                            || (!record.case.present() && entry["origin"] == "declared"),
                        "Maven dependency reference has an unexpected origin",
                    )?;
                } else {
                    foreign_count += 1;
                }
            }
            Some("container") => {
                if !entry["path"]
                    .as_str()
                    .is_some_and(|path| path.starts_with("org.eclipse.jdt.launching.JRE_CONTAINER"))
                {
                    foreign_count += 1;
                }
            }
            _ => return Err("unexpected Maven classpath entry kind".into()),
        }
    }
    record.unexpected_dependency_references = foreign_count;
    // An exact kind-1 reference to the deliberately absent JAR is legitimate
    // negative evidence. It is not classified as a foreign dependency.
    require(
        foreign_count == 0 && expected_count <= 1,
        "Maven model included a foreign or extra dependency reference",
    )?;
    record.exact_dependency_reference = expected_count == 1;
    let expected_status = if record.case.present() {
        ModelStatus::Imported
    } else {
        ModelStatus::Unresolved
    };
    Ok(record.exact_dependency_reference
        && record.model_status == expected_status
        && value["unresolved_count"] == if record.case.present() { 0 } else { 1 })
}

#[derive(Default)]
struct DiagnosticState {
    offline_pom: bool,
    owned_project_missing_library_diagnostic: bool,
    type_error: bool,
    corrected: bool,
    trace: EventTrace,
}
#[derive(Clone, Copy, Default)]
struct EventTrace {
    rejection: EventRejection,
    origin: DiagnosticOrigin,
    code_shape: DiagnosticCodeShape,
    severity: DiagnosticSeverity,
    message_class: DiagnosticMessageClass,
}

fn diagnostic_candidate(trace: &mut EventTrace, origin: DiagnosticOrigin, item: &Value) {
    trace.origin = origin;
    trace.code_shape = match &item["code"] {
        Value::Null => DiagnosticCodeShape::Missing,
        Value::String(code) if code == "0" => DiagnosticCodeShape::StringZero,
        Value::String(code) if code == "16777233" => DiagnosticCodeShape::StringTypeMismatch,
        Value::String(code) if code == "964" => DiagnosticCodeShape::StringInvalidClasspath,
        Value::String(_) => DiagnosticCodeShape::OtherString,
        Value::Number(code) if code.as_i64() == Some(0) => DiagnosticCodeShape::IntegerZero,
        Value::Number(code) if code.as_i64() == Some(16_777_233) => {
            DiagnosticCodeShape::IntegerTypeMismatch
        }
        Value::Number(code) if code.is_i64() || code.is_u64() => DiagnosticCodeShape::OtherInteger,
        _ => DiagnosticCodeShape::Other,
    };
    trace.severity = if item["severity"].is_null() {
        DiagnosticSeverity::Missing
    } else {
        match item["severity"].as_u64() {
            Some(1) => DiagnosticSeverity::Error,
            Some(2) => DiagnosticSeverity::Warning,
            Some(3) => DiagnosticSeverity::Information,
            Some(4) => DiagnosticSeverity::Hint,
            _ => DiagnosticSeverity::Other,
        }
    };
    trace.message_class = match item["message"].as_str() {
        Some(message) if message == format!("Offline / Missing artifact {DEPENDENCY_GAV}") => {
            DiagnosticMessageClass::OfflineOwnedDependency
        }
        Some(message) if message == format!("Missing artifact {DEPENDENCY_GAV}") => {
            DiagnosticMessageClass::PlainMissingOwnedDependency
        }
        Some("Type mismatch: cannot convert from int to String") => {
            DiagnosticMessageClass::DeliberateIntToString
        }
        Some("The import cedar cannot be resolved") => {
            DiagnosticMessageClass::UnresolvedCedarImport
        }
        Some("Arithmetic cannot be resolved") => DiagnosticMessageClass::UnresolvedArithmetic,
        None if item["message"].is_null() => DiagnosticMessageClass::Missing,
        _ => DiagnosticMessageClass::Other,
    };
}
fn exact_type_diagnostic(diagnostic: &Value) -> bool {
    diagnostic["severity"] == 1
        && diagnostic["code"] == "16777233"
        && diagnostic["message"] == "Type mismatch: cannot convert from int to String"
        && diagnostic["range"]
            == serde_json::json!({"start":{"line":5,"character":25},"end":{"line":5,"character":30}})
}
fn inspect_events(
    value: &Value,
    paths: &CasePaths,
    kind: CaseKind,
    diagnostics: &mut DiagnosticState,
    correction: bool,
) -> CheckResult<()> {
    diagnostics.trace = EventTrace::default();
    let result = inspect_events_inner(value, paths, kind, diagnostics, correction);
    if let Err(error) = &result {
        let rejection = match error.as_str() {
            "Maven event stream was truncated" => EventRejection::Truncated,
            "Maven events missing" => EventRejection::MissingEvents,
            "Maven event stream closed, lagged or was malformed" => diagnostics.trace.rejection,
            "Maven diagnostic URI missing" => EventRejection::MissingDiagnosticUri,
            "Maven diagnostic list missing" => EventRejection::MissingDiagnostics,
            "Maven POM reported a foreign or unexpected diagnostic" => {
                EventRejection::UnexpectedPomDiagnostic
            }
            "Maven source reported a foreign or unexpected diagnostic" => {
                EventRejection::UnexpectedSourceDiagnostic
            }
            "Maven project reported a foreign or unexpected diagnostic" => {
                EventRejection::UnexpectedProjectDiagnostic
            }
            "Maven diagnostics referenced a foreign document" => EventRejection::ForeignDocument,
            "source URI failed" | "POM URI failed" | "project URI failed" => {
                EventRejection::UriEncoding
            }
            _ => EventRejection::None,
        };
        if !matches!(
            rejection,
            EventRejection::MissingDiagnosticUri
                | EventRejection::MissingDiagnostics
                | EventRejection::UnexpectedPomDiagnostic
                | EventRejection::UnexpectedSourceDiagnostic
                | EventRejection::UnexpectedProjectDiagnostic
                | EventRejection::ForeignDocument
        ) {
            diagnostics.trace = EventTrace::default();
        }
        diagnostics.trace.rejection = rejection;
    } else {
        // Candidate details from an accepted diagnostic never become a claimed
        // rejection, including when the next batch has no diagnostics at all.
        diagnostics.trace = EventTrace::default();
    }
    result
}

fn record_events(
    value: &Value,
    paths: &CasePaths,
    record: &mut CaseEvidence,
    diagnostics: &mut DiagnosticState,
    correction: bool,
) -> CheckResult<()> {
    let result = inspect_events(value, paths, record.case, diagnostics, correction);
    record.event_probe_outcome = if result.is_ok() {
        EventProbeOutcome::EventsAccepted
    } else {
        EventProbeOutcome::EventsRejected
    };
    record.event_rejection = diagnostics.trace.rejection;
    record.rejected_diagnostic_origin = diagnostics.trace.origin;
    record.rejected_diagnostic_code_shape = diagnostics.trace.code_shape;
    record.rejected_diagnostic_severity = diagnostics.trace.severity;
    record.rejected_diagnostic_message_class = diagnostics.trace.message_class;
    record.owned_project_missing_library_diagnostic =
        diagnostics.owned_project_missing_library_diagnostic;
    result
}

fn inspect_events_inner(
    value: &Value,
    paths: &CasePaths,
    kind: CaseKind,
    diagnostics: &mut DiagnosticState,
    correction: bool,
) -> CheckResult<()> {
    require(
        value["truncated"] == false,
        "Maven event stream was truncated",
    )?;
    let events = value["events"].as_array().ok_or("Maven events missing")?;
    let source_uri =
        url::Url::from_file_path(paths.root.join(SOURCE_FILE)).map_err(|_| "source URI failed")?;
    let pom_uri =
        url::Url::from_file_path(paths.root.join("pom.xml")).map_err(|_| "POM URI failed")?;
    let project_uri =
        url::Url::from_directory_path(&paths.root).map_err(|_| "project URI failed")?;
    for event in events {
        match event["type"].as_str() {
            Some("notification" | "unsupported_server_request") => {}
            Some("diagnostics") => {
                let value = &event["value"];
                diagnostics.trace = EventTrace {
                    origin: DiagnosticOrigin::Missing,
                    ..EventTrace::default()
                };
                let uri = value["uri"]
                    .as_str()
                    .ok_or("Maven diagnostic URI missing")?;
                diagnostics.trace.origin = if same_local_uri(uri, pom_uri.as_str()) {
                    DiagnosticOrigin::Pom
                } else if same_local_uri(uri, source_uri.as_str()) {
                    DiagnosticOrigin::Source
                } else if same_local_uri(uri, project_uri.as_str()) {
                    DiagnosticOrigin::OwnedProjectRoot
                } else {
                    DiagnosticOrigin::Foreign
                };
                let items = value["diagnostics"]
                    .as_array()
                    .ok_or("Maven diagnostic list missing")?;
                if same_local_uri(uri, pom_uri.as_str()) {
                    for item in items {
                        diagnostic_candidate(&mut diagnostics.trace, DiagnosticOrigin::Pom, item);
                        require(
                            !kind.present()
                                && item["severity"] == 1
                                && item["code"] == "0"
                                && item["message"]
                                    == format!("Offline / Missing artifact {DEPENDENCY_GAV}"),
                            "Maven POM reported a foreign or unexpected diagnostic",
                        )?;
                        diagnostics.offline_pom = true;
                    }
                } else if same_local_uri(uri, source_uri.as_str()) {
                    if correction
                        && items.is_empty()
                        && value
                            .get("version")
                            .is_none_or(|version| version.is_null() || version == 2)
                    {
                        diagnostics.corrected = true;
                    }
                    for item in items {
                        diagnostic_candidate(
                            &mut diagnostics.trace,
                            DiagnosticOrigin::Source,
                            item,
                        );
                        if exact_type_diagnostic(item) {
                            diagnostics.type_error = true;
                        } else {
                            // The missing dependency can yield the two expected
                            // unresolved source references before didOpen too.
                            require(
                                !kind.present()
                                    && item["severity"] == 1
                                    && matches!(
                                        item["message"].as_str(),
                                        Some(
                                            "The import cedar cannot be resolved"
                                                | "Arithmetic cannot be resolved"
                                        )
                                    ),
                                "Maven source reported a foreign or unexpected diagnostic",
                            )?;
                        }
                    }
                } else if same_local_uri(uri, project_uri.as_str()) {
                    for item in items {
                        diagnostic_candidate(
                            &mut diagnostics.trace,
                            DiagnosticOrigin::OwnedProjectRoot,
                            item,
                        );
                        // Source-backed candidate, not identification of a
                        // historical native failure: JDT Core 6725c16c uses
                        // INVALID_CLASSPATH 964; m2e JDT 8d83cb8 retains the
                        // missing JAR; JDT LS 08eafe6 publishes project markers
                        // with string codes, source Java and a zero range.
                        // JDT's Windows OS path uses backslashes, including
                        // components joined from the fixture's slash constants.
                        let owned_message = item["message"]
                            == format!(
                                "The container 'Maven Dependencies' references non existing library '{}'",
                                text(&paths.repository.join(DEPENDENCY_JAR))?.replace('/', "\\")
                            );
                        if owned_message {
                            diagnostics.trace.message_class =
                                DiagnosticMessageClass::OwnedMissingMavenLibrary;
                        }
                        require(
                            !kind.present()
                                && item["severity"] == 1
                                && item["code"] == "964"
                                && item["source"] == "Java"
                                && item["range"]
                                    == serde_json::json!({"start":{"line":0,"character":0},"end":{"line":0,"character":0}})
                                && owned_message
                                && absent(&paths.repository.join(DEPENDENCY_JAR))?
                                && absent(&paths.repository.join(DEPENDENCY_POM))?,
                            "Maven project reported a foreign or unexpected diagnostic",
                        )?;
                        diagnostics.owned_project_missing_library_diagnostic = true;
                    }
                } else {
                    if let Some(item) = items.first() {
                        diagnostic_candidate(
                            &mut diagnostics.trace,
                            DiagnosticOrigin::Foreign,
                            item,
                        );
                    }
                    require(
                        items.is_empty(),
                        "Maven diagnostics referenced a foreign document",
                    )?;
                }
            }
            _ => {
                diagnostics.trace.rejection = match event["type"].as_str() {
                    Some("closed") => EventRejection::ClosedEvent,
                    Some("lagged") => EventRejection::LaggedEvent,
                    None => EventRejection::MissingEventType,
                    Some(_) => EventRejection::UnknownEvent,
                };
                return Err("Maven event stream closed, lagged or was malformed".into());
            }
        }
    }
    Ok(())
}

fn await_model(
    client: &mut Client,
    paths: &CasePaths,
    record: &mut CaseEvidence,
    diagnostics: &mut DiagnosticState,
) -> CheckResult<()> {
    let deadline = Instant::now() + MODEL_BUDGET;
    while record.model_queries < MAX_MODEL_QUERIES && Instant::now() < deadline {
        record.model_queries += 1;
        let model = model_payload(client.request(Operation::LanguageMavenModel), record)?;
        let ready = inspect_model(&model, paths, record)?;
        let events = event_payload(client.request(Operation::LanguageEvents), record)?;
        record_events(&events, paths, record, diagnostics, false)?;
        record.offline_pom_diagnostic = diagnostics.offline_pom;
        let within_budget = Instant::now() < deadline;
        if !within_budget {
            record.model_probe_outcome = ModelProbeOutcome::BudgetExhausted;
        }
        require(
            within_budget,
            "Maven model request exceeded its fixed budget",
        )?;
        if ready && (record.case.present() || diagnostics.offline_pom) {
            return Ok(());
        }
        if record.model_queries < MAX_MODEL_QUERIES
            && deadline.saturating_duration_since(Instant::now()) >= MODEL_INTERVAL
        {
            thread::sleep(MODEL_INTERVAL);
        } else {
            break;
        }
    }
    record.model_probe_outcome = ModelProbeOutcome::BudgetExhausted;
    Err("Maven model did not establish its bounded present/missing witness".into())
}

fn dependencies_operation(startup_id: u64) -> Operation {
    Operation::LanguageMavenDependencies {
        startup_id,
        pom_sha256: POM_SHA256.into(),
    }
}

fn dependency_payload(
    response: CheckResult<Payload>,
    record: &mut CaseEvidence,
) -> CheckResult<MavenDependenciesSnapshot> {
    record.dependency_error_code = ClientErrorCode::None;
    match response {
        Ok(Payload::MavenDependencies { snapshot }) => {
            record.dependency_probe_outcome = DependencyProbeOutcome::ResponseReceived;
            Ok(snapshot)
        }
        Ok(_) => {
            record.dependency_probe_outcome = DependencyProbeOutcome::NonDependencyPayload;
            Err("Maven dependency insight received a non-dependency response".into())
        }
        Err(error) => {
            record.dependency_probe_outcome = DependencyProbeOutcome::RequestFailed;
            record.dependency_error_code = client_error_code(&error);
            Err(error)
        }
    }
}

fn inspect_dependencies(
    snapshot: &MavenDependenciesSnapshot,
    startup_id: u64,
    record: &mut CaseEvidence,
) -> CheckResult<()> {
    record.dependency_probe_outcome = DependencyProbeOutcome::SnapshotRejected;
    record.dependency_observation = DependencyObservation::Rejected;
    snapshot
        .validate_for(startup_id, POM_SHA256, true)
        .map_err(|_| "Maven dependency snapshot identity or shape rejected".to_string())?;
    record.dependency_snapshot_identity_verified = true;
    record.dependency_declaration_count = snapshot.declarations.len() as u16;
    require(
        snapshot.declarations.len() == 1,
        "Maven dependency insight did not contain the exact declaration count",
    )?;
    let declaration = &snapshot.declarations[0];
    require(
        declaration.group_id == "dev.cedar.fixture"
            && declaration.artifact_id == "arithmetic"
            && declaration.version == "1.0.0"
            && declaration.classifier.is_none(),
        "Maven dependency insight changed the captured declaration",
    )?;
    record.dependency_declaration_exact = true;
    require(
        declaration.scope == MavenDependencyScope::Compile
            && !declaration.scope_explicit
            && !declaration.optional
            && !declaration.optional_explicit,
        "Maven dependency insight lost default compile or optional provenance",
    )?;
    record.dependency_default_provenance_verified = true;
    require(
        declaration.expected_jar_path == DEPENDENCY_JAR,
        "Maven dependency insight changed the exact relative artifact path",
    )?;
    record.dependency_expected_jar_verified = true;
    record.dependency_declaration_file_present = declaration.regular_file_present;
    require(
        declaration.regular_file_present == record.case.present(),
        "Maven declaration file observation contradicts the frozen fixture",
    )?;
    let MavenDependencyObservation::Available { libraries } = &snapshot.observation else {
        record.dependency_observation = DependencyObservation::Unavailable;
        return Err("Maven dependency insight did not observe the JDT model".into());
    };
    record.dependency_observed_library_count = libraries.len() as u16;
    require(
        libraries.len() <= 1,
        "Maven dependency insight included extra observed libraries",
    )?;
    if let Some(library) = libraries.first() {
        require(
            library.root == MavenLibraryRoot::LocalRepository
                && library.relative_path == DEPENDENCY_JAR
                && library.declaration_indices == [0],
            "Maven dependency insight included a foreign or unassociated observation",
        )?;
        record.dependency_observation = if library.regular_file_present {
            DependencyObservation::ObservedPresentFile
        } else {
            DependencyObservation::ObservedAbsentFile
        };
        require(
            library.regular_file_present == record.case.present(),
            "Maven observed library file presence contradicts the frozen fixture",
        )?;
    } else {
        // JDT can omit its absent classpath entry. Record that actual observation
        // separately from the independently observed declaration-file absence.
        record.dependency_observation = DependencyObservation::NotObserved;
        require(
            !record.case.present(),
            "Maven dependency insight omitted the present exact JDT library",
        )?;
    }
    record.dependency_probe_outcome = DependencyProbeOutcome::Accepted;
    Ok(())
}

fn dependency_query(
    client: &mut Client,
    paths: &CasePaths,
    record: &mut CaseEvidence,
    startup_id: u64,
    pair_deadline: Instant,
) -> CheckResult<()> {
    let reserve = semantic_budget(record.case) + CLEANUP_BUDGET;
    admit_work(pair_deadline, DEPENDENCY_RPC_BUDGET, reserve)?;
    record.dependency_queries += 1;
    let queried = Instant::now();
    let snapshot = dependency_payload(client.request(dependencies_operation(startup_id)), record)?;
    if queried.elapsed() >= DEPENDENCY_RPC_BUDGET
        || !budget_admits(Instant::now(), pair_deadline, Duration::ZERO, reserve)
    {
        record.dependency_probe_outcome = DependencyProbeOutcome::BudgetExhausted;
        return Err("Maven dependency insight exhausted its admitted observation budget".into());
    }
    inspect_dependencies(&snapshot, startup_id, record)?;
    let editor_ctx = eframe::egui::Context::default();
    let mut document = Document::new(1, SOURCE_FILE.into(), FIXTURE_SOURCE.into(), "r0".into());
    editor_state::commit(
        &editor_ctx,
        &mut document,
        format!("{FIXTURE_SOURCE}// unsaved dependency view witness\n"),
        0,
    );
    require(
        document.dirty() && source_unchanged(paths),
        "Maven dependency view fixture did not retain its dirty baseline",
    )?;
    crate::language_ui::verify_native_maven_dependencies(
        &snapshot,
        startup_id,
        POM_SHA256,
        &mut document,
        &editor_ctx,
    )?;
    require(
        document.dirty() && source_unchanged(paths),
        "Maven dependency view saved or discarded the dirty fixture",
    )?;
    record.dependency_optional_capability_rejected = true;
    record.dependency_frontend_identity_verified = true;
    record.dependency_frontend_invalidated = true;
    record.dependency_dirty_undo_preserved = true;
    admit_work(pair_deadline, Duration::ZERO, reserve)
}

fn semantic_queries(
    client: &mut Client,
    paths: &CasePaths,
    record: &mut CaseEvidence,
    diagnostics: &mut DiagnosticState,
) -> CheckResult<()> {
    let deadline = Instant::now() + SEMANTIC_BUDGET;
    let opened = language_value(
        client,
        Operation::LanguageOpen {
            path: SOURCE_FILE.into(),
            language_id: "java".into(),
            version: 1,
            text: FIXTURE_SOURCE.into(),
        },
    )?;
    let uri = opened["opened"]
        .as_str()
        .ok_or("Maven source open URI missing")?;
    let expected = url::Url::from_file_path(paths.root.join(SOURCE_FILE))
        .map_err(|_| "Maven source URI failed")?;
    require(
        opened["version"] == 1 && same_local_uri(uri, expected.as_str()),
        "Maven didOpen identity mismatch",
    )?;
    let cursor = completion::byte_to_position(
        FIXTURE_SOURCE,
        FIXTURE_SOURCE
            .find("Arithmetic.answer")
            .ok_or("fixture call missing")?
            + "Arithmetic.an".len(),
    )?;
    let hover = language_value(
        client,
        Operation::LanguageQuery {
            path: SOURCE_FILE.into(),
            line: cursor.line,
            character: cursor.character,
            kind: LanguageQueryKind::Hover,
        },
    )?;
    let hover = crate::language_results::hover_text(&hover);
    require(
        hover.contains("int cedar.fixture.Arithmetic.answer()"),
        "Maven hover did not resolve the exact generated dependency method",
    )?;
    record.hover = true;
    require(
        Instant::now() < deadline,
        "Maven semantic budget expired after hover",
    )?;
    let completion = language_value(
        client,
        Operation::LanguageQuery {
            path: SOURCE_FILE.into(),
            line: cursor.line,
            character: cursor.character,
            kind: LanguageQueryKind::Completion,
        },
    )?;
    let completion = completion::parse_completion_result(&completion)?;
    require(
        completion.candidates.iter().any(|item| {
            item.label == "answer() : int"
                && item.item["detail"] == "Arithmetic.answer() : int"
                && item.item["textEdit"]["newText"] == "answer"
        }),
        "Maven completion did not resolve the exact generated dependency method",
    )?;
    record.completion = true;
    while Instant::now() < deadline && !diagnostics.type_error {
        let events = event_payload(client.request(Operation::LanguageEvents), record)?;
        record_events(&events, paths, record, diagnostics, false)?;
        if !diagnostics.type_error {
            thread::sleep(MODEL_INTERVAL);
        }
    }
    require(
        diagnostics.type_error && Instant::now() < deadline,
        "Maven deliberate int-to-String diagnostic missing",
    )?;
    record.deliberate_type_diagnostic = true;
    let mut document = Document::new(1, SOURCE_FILE.into(), FIXTURE_SOURCE.into(), "r0".into());
    let corrected = FIXTURE_SOURCE.replace(
        "String invalid = known;",
        "String invalid = Integer.toString(known);",
    );
    editor_state::commit(
        &eframe::egui::Context::default(),
        &mut document,
        corrected.clone(),
        0,
    );
    require(
        document.dirty()
            && document.saved_text == FIXTURE_SOURCE
            && document.text == corrected
            && document.edit_version == 1
            && source_unchanged(paths),
        "Maven frontend transaction lost its dirty baseline",
    )?;
    let changed = language_value(
        client,
        Operation::LanguageChange {
            path: document.path.clone(),
            version: 2,
            text: document.text.clone(),
        },
    )?;
    require(
        changed["version"] == 2
            && changed["changed"]
                .as_str()
                .is_some_and(|actual| same_local_uri(actual, uri)),
        "Maven dirty didChange acknowledgment mismatch",
    )?;
    record.dirty_change_acknowledged = true;
    while Instant::now() < deadline && !diagnostics.corrected {
        let events = event_payload(client.request(Operation::LanguageEvents), record)?;
        record_events(&events, paths, record, diagnostics, true)?;
        if !diagnostics.corrected {
            thread::sleep(MODEL_INTERVAL);
        }
    }
    require(
        diagnostics.corrected
            && Instant::now() < deadline
            && document.dirty()
            && source_unchanged(paths),
        "Maven unsaved correction did not clear diagnostics without saving",
    )?;
    record.no_autosave = true;
    language_value(
        client,
        Operation::LanguageClose {
            path: SOURCE_FILE.into(),
        },
    )?;
    require(
        Instant::now() < deadline,
        "Maven semantics exceeded the fixed 20-second budget",
    )
}

fn check_repository(
    paths: &CasePaths,
    before: &Inventory,
    record: &mut CaseEvidence,
) -> CheckResult<()> {
    record.dependency_jar_present_after = !absent(&paths.repository.join(DEPENDENCY_JAR))?;
    record.dependency_pom_present_after = !absent(&paths.repository.join(DEPENDENCY_POM))?;
    // Check every immutable input independently before inspecting any new
    // metadata. Oversized or invalid new files cannot suppress these checks.
    record.repository_inputs_unchanged = before.iter().all(|(relative, seal)| {
        let path = paths.repository.join(relative);
        fs::symlink_metadata(&path).is_ok_and(|metadata| {
            metadata.is_file()
                && !metadata.file_type().is_symlink()
                && metadata.file_attributes() & 0x400 == 0
                && metadata.len() == seal.bytes
                && fs::read(&path).is_ok_and(|bytes| hash(&bytes) == seal.sha256)
        })
    });
    let mirror_uri = owned_mirror_uri(paths)?;
    let after = snapshot(&paths.repository)?;
    for (path, seal) in &after {
        if before.contains_key(path) {
            continue;
        }
        let allowed_parent = path.parent().is_some_and(|parent| {
            parent == Path::new(DEPENDENCY_DIRECTORY)
                || before.keys().any(|known| known.parent() == Some(parent))
        });
        let allowed_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                matches!(
                    name,
                    "_remote.repositories"
                        | "resolver-status.properties"
                        | "m2e-lastUpdated.properties"
                ) || name.ends_with(".lastUpdated")
            });
        if allowed_parent && allowed_name && seal.bytes <= 32 * 1024 {
            record.generated_metadata_files += 1;
        } else {
            let lifecycle_bit = checked(|| {
                use crate::java_maven_metadata_tests::{
                    lifecycle_metadata_bit, MAX_METADATA_BYTES,
                };
                require(
                    seal.bytes <= MAX_METADATA_BYTES as u64,
                    "generated lifecycle metadata exceeds its existing file bound",
                )?;
                let full = paths.repository.join(path);
                let metadata = io(fs::symlink_metadata(&full))?;
                require(
                    metadata.is_file()
                        && !metadata.file_type().is_symlink()
                        && metadata.file_attributes() & 0x400 == 0
                        && metadata.len() == seal.bytes,
                    "generated lifecycle metadata is not the inspected ordinary file",
                )?;
                let mut contents = Vec::new();
                io(io(fs::File::open(full))?
                    .take(MAX_METADATA_BYTES as u64 + 1)
                    .read_to_end(&mut contents))?;
                require(
                    contents.len() as u64 == seal.bytes && hash(&contents) == seal.sha256,
                    "generated lifecycle metadata changed during inspection",
                )?;
                lifecycle_metadata_bit(path, &contents, &mirror_uri)
                    .ok_or_else(|| "generated file is not exact owned lifecycle metadata".into())
            });
            match lifecycle_bit {
                Ok(bit) if record.lifecycle_metadata_mask & bit == 0 => {
                    record.lifecycle_metadata_mask |= bit;
                    record.lifecycle_metadata_files += 1;
                    record.generated_metadata_files += 1;
                }
                _ => record.foreign_repository_files += 1,
            }
        }
    }
    require(
        record.repository_inputs_unchanged
            && record.foreign_repository_files == 0
            && record.generated_metadata_files <= 128
            && record.lifecycle_metadata_files <= 6
            && record.lifecycle_metadata_mask <= 63
            && u32::from(record.lifecycle_metadata_files)
                == record.lifecycle_metadata_mask.count_ones()
            && record.dependency_jar_present_after == record.case.present()
            && record.dependency_pom_present_after == record.case.present(),
        "Maven changed sealed repository inputs or materialized an unapproved artifact",
    )
}

fn generated_metadata(root: &Path, project: bool) -> CheckResult<(u32, u64)> {
    let mut directories = vec![root.to_path_buf()];
    let mut entries = 0u32;
    let mut files = 0u32;
    let mut bytes = 0u64;
    while let Some(directory) = directories.pop() {
        for entry in io(fs::read_dir(directory))? {
            entries += 1;
            require(
                entries <= if project { 512 } else { 4096 },
                "generated Maven metadata exceeded its entry bound",
            )?;
            let path = io(entry)?.path();
            let metadata = io(fs::symlink_metadata(&path))?;
            require(
                !metadata.file_type().is_symlink() && metadata.file_attributes() & 0x400 == 0,
                "generated Maven metadata contains a link or reparse point",
            )?;
            if metadata.is_dir() {
                directories.push(path);
                continue;
            }
            require(
                metadata.is_file(),
                "generated Maven metadata is not an ordinary file",
            )?;
            let relative = path
                .strip_prefix(root)
                .map_err(|_| "generated metadata escaped its root")?;
            if project {
                if relative == Path::new(SOURCE_FILE) || relative == Path::new("pom.xml") {
                    continue;
                }
                require(
                    relative == Path::new(".project")
                        || relative == Path::new(".classpath")
                        || relative.starts_with(".settings"),
                    "Maven leaf contains an unexpected generated file",
                )?;
            }
            files += 1;
            bytes = bytes
                .checked_add(metadata.len())
                .ok_or("generated metadata byte counter overflow")?;
            require(
                files <= if project { 256 } else { 4096 }
                    && bytes
                        <= if project {
                            16 * 1024 * 1024
                        } else {
                            128 * 1024 * 1024
                        },
                "generated Maven metadata exceeded its aggregate file or byte bound",
            )?;
            // Deliberately do not read or hash generated JDT indexes. A normal
            // JDK index can exceed 40 MiB; only original inputs use hash seals.
        }
    }
    Ok((files, bytes))
}

fn dependency_case_passed(record: &CaseEvidence) -> bool {
    record.dependency_capability_advertised
        && record.dependency_optional_capability_rejected
        && record.dependencies_untrusted_rejected
        && record.dependencies_without_session_rejected
        && record.dependency_queries == 1
        && record.dependency_probe_outcome == DependencyProbeOutcome::Accepted
        && record.dependency_error_code == ClientErrorCode::None
        && record.dependency_snapshot_identity_verified
        && record.dependency_declaration_count == 1
        && record.dependency_declaration_exact
        && record.dependency_default_provenance_verified
        && record.dependency_expected_jar_verified
        && record.dependency_declaration_file_present == record.case.present()
        && record.dependency_frontend_identity_verified
        && record.dependency_frontend_invalidated
        && record.dependency_dirty_undo_preserved
        && record.dependencies_changed_pom_restart_required
        && record.dependencies_after_stop_rejected
        && match record.dependency_observation {
            DependencyObservation::ObservedPresentFile => {
                record.case.present() && record.dependency_observed_library_count == 1
            }
            DependencyObservation::ObservedAbsentFile => {
                !record.case.present() && record.dependency_observed_library_count == 1
            }
            DependencyObservation::NotObserved => {
                !record.case.present() && record.dependency_observed_library_count == 0
            }
            _ => false,
        }
}

fn case_passed(record: &CaseEvidence) -> bool {
    dependency_case_passed(record)
        && record.java_capabilities
        && record.generic_start_rejected
        && record.untrusted_start_rejected
        && record.model_without_session_rejected
        && record.async_start_begin_acknowledged
        && record.async_start_read_while_starting
        && record.async_start_ready
        && record.root_identity_verified
        && record.root_observed_live
        && record.maven_nature
        && record.custom_source
        && record.compiler_17
        && record.exact_dependency_reference
        && record.unexpected_dependency_references == 0
        && record.model_queries > 0
        && record.model_queries <= MAX_MODEL_QUERIES
        && record.stale_startup_rejected
        && record.changed_pom_restart_required
        && record.source_unchanged
        && record.pom_expected
        && record.repository_inputs_unchanged
        && record.foreign_repository_files == 0
        && record.generated_metadata_files <= 128
        && record.lifecycle_metadata_files <= 6
        && record.lifecycle_metadata_mask <= 63
        && u32::from(record.lifecycle_metadata_files) == record.lifecycle_metadata_mask.count_ones()
        && record.generated_data_files > 0
        && record.generated_data_files <= 4096
        && record.generated_data_bytes > 0
        && record.generated_data_bytes <= 128 * 1024 * 1024
        && record.generated_project_files <= 256
        && record.generated_project_bytes <= 16 * 1024 * 1024
        && record.dependency_jar_present_before == record.case.present()
        && record.dependency_jar_present_after == record.case.present()
        && record.dependency_pom_present_before == record.case.present()
        && record.dependency_pom_present_after == record.case.present()
        && record.stop_outcome_verified
        && record.cleanup_joined
        && record.root_handle_signaled
        && record.root_exit_code.is_some()
        && record.model_after_stop_rejected
        && record.client_reaped
        && record.synthetic_root_removed
        && !record.primary_failed
        && !record.cleanup_failed
        && if record.case.present() {
            record.model_status == ModelStatus::Imported
                && record.hover
                && record.completion
                && record.deliberate_type_diagnostic
                && record.dirty_change_acknowledged
                && record.no_autosave
        } else {
            record.model_status == ModelStatus::Unresolved && record.offline_pom_diagnostic
        }
}

fn run_case(
    kind: CaseKind,
    binary: &Path,
    java: &Path,
    distribution: &Path,
    input: &Path,
    cache: &Inventory,
    pair_deadline: Instant,
) -> CaseEvidence {
    let mut record = CaseEvidence {
        case: kind,
        client_reaped: true,
        ..CaseEvidence::default()
    };
    let mut fixture: Option<tempfile::TempDir> = None;
    let mut paths: Option<CasePaths> = None;
    let mut repository_before = Inventory::new();
    let mut client: Option<Client> = None;
    let mut observed: Option<RootObservation> = None;
    let mut startup_id = None;
    let mut ready = false;
    let mut pom_changed = false;
    let primary = checked(|| {
        admit_work(pair_deadline, case_budget(kind), Duration::ZERO)?;
        fixture = Some(io(tempfile::Builder::new()
            .prefix("cedar-maven-")
            .tempdir())?);
        let base = ordinary_path(fixture.as_ref().ok_or("Maven fixture missing")?.path())?;
        let (prepared, inventory) = setup_case(&base, input, cache, kind)?;
        paths = Some(prepared);
        repository_before = inventory;
        let paths = paths.as_ref().ok_or("Maven paths missing")?;
        record.dependency_jar_present_before = !absent(&paths.repository.join(DEPENDENCY_JAR))?;
        record.dependency_pom_present_before = !absent(&paths.repository.join(DEPENDENCY_POM))?;
        require(
            record.dependency_jar_present_before == kind.present()
                && record.dependency_pom_present_before == kind.present(),
            "Maven dependency preflight presence mismatch",
        )?;
        let start = || -> CheckResult<Operation> {
            Ok(Operation::LanguageStartJavaMavenBegin {
                java_executable: text(java)?,
                distribution: text(distribution)?,
                data_directory: text(&paths.data)?,
                local_repository: text(&paths.repository)?,
            })
        };
        record.failure_stage = Stage::Trust;
        admit_work(pair_deadline, case_budget(kind), Duration::ZERO)?;
        record.client_reaped = false;
        client = Some(Client::spawn_agent(binary, &paths.root, false)?);
        capabilities(client.as_ref().ok_or("Maven untrusted Client missing")?)?;
        rejected(
            client.as_mut().ok_or("Maven untrusted Client missing")?,
            start()?,
            "run_disabled",
        )?;
        require(
            io(fs::read_dir(&paths.data))?.next().is_none(),
            "untrusted Maven startup created controls",
        )?;
        record.untrusted_start_rejected = true;
        admit_work(pair_deadline, NO_SESSION_RPC_BUDGET, CLEANUP_BUDGET)?;
        rejected(
            client.as_mut().ok_or("Maven untrusted Client missing")?,
            dependencies_operation(1),
            "run_disabled",
        )?;
        record.dependencies_untrusted_rejected = true;
        admit_work(pair_deadline, Duration::ZERO, CLEANUP_BUDGET)?;
        rejected(
            client.as_mut().ok_or("Maven untrusted Client missing")?,
            Operation::LanguageStart {
                program: "must-not-run".into(),
                args: vec![],
            },
            "unsupported_operation",
        )?;
        record.generic_start_rejected = true;
        admit_work(pair_deadline, CLIENT_REAP_BUDGET, CLEANUP_BUDGET)?;
        let reap_budget =
            CLIENT_REAP_BUDGET.min(pair_deadline.saturating_duration_since(Instant::now()));
        client
            .take()
            .ok_or("Maven untrusted Client missing")?
            .close_and_wait(reap_budget)?;
        record.client_reaped = true;
        // This is explicit test trust on a newly generated workspace, never a
        // GUI trust action and never inherited from a user workspace.
        admit_work(pair_deadline, case_budget(kind), Duration::ZERO)?;
        record.client_reaped = false;
        client = Some(Client::spawn_agent(binary, &paths.root, true)?);
        let client = client.as_mut().ok_or("Maven trusted Client missing")?;
        capabilities(client)?;
        record.java_capabilities = true;
        record.dependency_capability_advertised = true;
        rejected(
            client,
            Operation::LanguageMavenModel,
            "language_not_running",
        )?;
        record.model_without_session_rejected = true;
        admit_work(pair_deadline, NO_SESSION_RPC_BUDGET, CLEANUP_BUDGET)?;
        rejected(client, dependencies_operation(1), "language_not_running")?;
        record.dependencies_without_session_rejected = true;
        admit_work(pair_deadline, Duration::ZERO, CLEANUP_BUDGET)?;
        record.failure_stage = Stage::Startup;
        admit_work(
            pair_deadline,
            START_TIMEOUT,
            MODEL_BUDGET + semantic_budget(kind) + CLEANUP_BUDGET,
        )?;
        let startup_deadline = Instant::now() + START_TIMEOUT;
        let started = language_value(client, start()?)?;
        let id = started["startup_id"]
            .as_u64()
            .filter(|id| *id > 0)
            .ok_or("Maven Begin omitted startup identity")?;
        startup_id = Some(id);
        require(
            started["state"] == "starting",
            "Maven Begin did not acknowledge asynchronous Starting",
        )?;
        record.async_start_begin_acknowledged = true;
        let read = client.request(Operation::Read {
            path: SOURCE_FILE.into(),
        })?;
        require(
            matches!(read, Payload::File { path, text, .. } if path == SOURCE_FILE && text == FIXTURE_SOURCE),
            "Maven normal read while startup pending failed",
        )?;
        record.async_start_read_while_starting = true;
        let initialized = await_start(client, id, java, &mut observed, startup_deadline)?;
        ready = initialized["started"] == true;
        require(
            ready
                && initialized["initialize"]["cedar_java_profile"] == "maven_leaf"
                && initialized["initialize"]["cedar_java_maven_model"] == true
                && initialized["initialize"]["cedar_java_maven_pom_sha256"]
                    == hash(FIXTURE_POM.as_bytes()),
            "Maven Ready lacks exact supported profile and POM identity",
        )?;
        record.async_start_ready = true;
        let pid = initialized["process_id"]
            .as_u64()
            .and_then(|pid| u32::try_from(pid).ok())
            .ok_or("Maven Ready root PID missing")?;
        if observed.is_none() {
            observed = Some(RootObservation::open_current(1, pid)?);
        }
        let process = observed.as_ref().ok_or("Maven root observation missing")?;
        require(
            process.pid == pid && process.live()?,
            "Maven Ready changed the live owned Java root",
        )?;
        record.root_observed_live = true;
        verify_image(process, java)?;
        record.root_identity_verified = true;
        verify_control(paths)?;
        admit_work(
            pair_deadline,
            MODEL_BUDGET + semantic_budget(kind),
            CLEANUP_BUDGET,
        )?;
        rejected(
            client,
            Operation::LanguageStartJavaPoll {
                startup_id: id.checked_add(1).ok_or("Maven startup ID overflow")?,
            },
            "unknown_language_startup",
        )?;
        record.stale_startup_rejected = true;
        let mut diagnostics = DiagnosticState::default();
        record.failure_stage = Stage::Model;
        admit_work(
            pair_deadline,
            MODEL_BUDGET,
            semantic_budget(kind) + CLEANUP_BUDGET,
        )?;
        await_model(client, paths, &mut record, &mut diagnostics)?;
        record.failure_stage = Stage::Dependencies;
        dependency_query(client, paths, &mut record, id, pair_deadline)?;
        if kind.present() {
            record.failure_stage = Stage::Semantics;
            admit_work(pair_deadline, SEMANTIC_BUDGET, CLEANUP_BUDGET)?;
            semantic_queries(client, paths, &mut record, &mut diagnostics)?;
        }
        require(
            process.live()? && source_unchanged(paths),
            "Maven root died or source was saved before explicit Stop",
        )?;
        record.failure_stage = Stage::PomChange;
        admit_work(pair_deadline, Duration::ZERO, CLEANUP_BUDGET)?;
        // A deliberate change to this owned input must invalidate the old
        // model. No implicit reimport or new session is permitted here.
        io(fs::write(
            paths.root.join("pom.xml"),
            format!("{FIXTURE_POM}\n"),
        ))?;
        pom_changed = true;
        rejected(
            client,
            Operation::LanguageMavenModel,
            "language_maven_restart_required",
        )?;
        record.changed_pom_restart_required = true;
        admit_work(pair_deadline, DEPENDENCY_RPC_BUDGET, CLEANUP_BUDGET)?;
        rejected(
            client,
            dependencies_operation(id),
            "language_maven_restart_required",
        )?;
        record.dependencies_changed_pom_restart_required = true;
        admit_work(pair_deadline, Duration::ZERO, CLEANUP_BUDGET)?;
        Ok(())
    });
    record.primary_failed = primary.is_err();
    let failure_stage = record.failure_stage;
    let cleanup_started = Instant::now();
    let cleanup_deadline = bounded_cleanup_deadline(cleanup_started, pair_deadline);
    let cleanup_admitted = budget_admits(
        cleanup_started,
        pair_deadline,
        CLEANUP_BUDGET,
        Duration::ZERO,
    );
    let cleanup = checked(|| {
        record.failure_stage = Stage::Stop;
        require(
            cleanup_admitted,
            "Maven cleanup did not retain its required observation reserve",
        )?;
        if ready {
            let client = client.as_mut().ok_or("Maven Client missing during Stop")?;
            let stopped = language_value(client, Operation::LanguageStop)?;
            require(
                Instant::now() < cleanup_deadline,
                "Maven Stop returned after the observation deadline",
            )?;
            let outcome = JavaStopOutcome::parse(&stopped)?;
            record.stop_status = Some(outcome.status);
            record.stop_reason = Some(outcome.reason);
            record.cleanup_joined = outcome.cleanup_joined;
            record.shutdown_response_received = outcome.shutdown_response_received;
            record.exit_frame_completed = outcome.exit_frame_completed;
            record.failure_stage = Stage::RootExit;
            let actual = observed
                .as_ref()
                .ok_or("Maven root handle missing")?
                .exit_code_with_timeout(0)?;
            record.root_handle_signaled = true;
            record.root_exit_code = Some(actual);
            require(
                outcome.root_exit == JavaRootExit::WindowsCode(actual)
                    && match outcome.status {
                        StopStatus::Graceful => true,
                        StopStatus::Forced => matches!(
                            outcome.reason,
                            StopReason::GraceExpired | StopReason::Aborted
                        ),
                        StopStatus::Error => false,
                    },
                "Maven typed Stop disagrees with retained root exit or reports errors",
            )?;
            record.stop_outcome_verified = true;
            admit_work(cleanup_deadline, Duration::ZERO, Duration::ZERO)?;
            // This normal no-session request can retain the Client's 30-second
            // RPC timeout. Admission is not a promise that it returns within
            // the remaining allowance: a late response must fail this case.
            rejected(
                client,
                Operation::LanguageMavenModel,
                "language_not_running",
            )?;
            require(
                Instant::now() < cleanup_deadline,
                "post-Stop model verification exceeded the remaining cleanup budget",
            )?;
            record.model_after_stop_rejected = true;
            admit_work(cleanup_deadline, NO_SESSION_RPC_BUDGET, Duration::ZERO)?;
            rejected(
                client,
                dependencies_operation(startup_id.ok_or("Maven startup identity missing")?),
                "language_not_running",
            )?;
            require(
                Instant::now() < cleanup_deadline,
                "post-Stop dependency verification exceeded the cleanup budget",
            )?;
            record.dependencies_after_stop_rejected = true;
        } else if let (Some(id), Some(client)) = (startup_id, client.as_mut()) {
            cancel_start(client, id, cleanup_deadline)?;
        }
        require(
            Instant::now() < cleanup_deadline,
            "Maven Stop exceeded the fixed cleanup budget",
        )
    });
    record.cleanup_failed = cleanup.is_err();
    let cleanup_stage = record.failure_stage;
    // Closing the normal Client owns the agent reaper and its process/job
    // cleanup. Never kill a process discovered by PID or widen ownership.
    if let Some(client) = client.take() {
        let remaining = cleanup_deadline.saturating_duration_since(Instant::now());
        record.client_reaped = checked(|| client.close_and_wait(remaining)).is_ok();
        if !record.client_reaped {
            record.cleanup_failed = true;
            record.failure_stage = Stage::ClientExit;
        }
    }
    if let Some(process) = observed.as_ref() {
        match process.exit_code_with_timeout(0) {
            Ok(code) => {
                record.root_handle_signaled = true;
                record.root_exit_code = Some(code);
            }
            Err(_) => {
                record.cleanup_failed = true;
                record.failure_stage = Stage::RootExit;
            }
        }
    }
    if let Some(paths) = paths.as_ref() {
        record.source_unchanged = source_unchanged(paths);
        let expected_pom = if pom_changed {
            format!("{FIXTURE_POM}\n")
        } else {
            FIXTURE_POM.into()
        };
        record.pom_expected = fs::read(paths.root.join("pom.xml"))
            .is_ok_and(|bytes| hash(&bytes) == hash(expected_pom.as_bytes()));
        if checked(|| check_repository(paths, &repository_before, &mut record)).is_err()
            || !record.source_unchanged
            || !record.pom_expected
        {
            record.cleanup_failed = true;
            record.failure_stage = Stage::FixtureCleanup;
        }
        let metadata = checked(|| {
            let (files, bytes) = generated_metadata(&paths.data, false)?;
            record.generated_data_files = files;
            record.generated_data_bytes = bytes;
            let (files, bytes) = generated_metadata(&paths.root, true)?;
            record.generated_project_files = files as u16;
            record.generated_project_bytes = bytes;
            Ok(())
        });
        if metadata.is_err() {
            record.cleanup_failed = true;
            record.failure_stage = Stage::FixtureCleanup;
        }
    }
    let root_stopped = observed.is_none() || record.root_handle_signaled;
    if let Some(fixture) = fixture.take() {
        if record.client_reaped && root_stopped {
            record.synthetic_root_removed = fixture.close().is_ok();
            if !record.synthetic_root_removed {
                record.cleanup_failed = true;
                record.failure_stage = Stage::FixtureCleanup;
            }
        } else {
            // Retain only this generated fixture if ownership was not proved
            // closed. Deleting a live server's input is not cleanup evidence.
            let _ = fixture.keep();
        }
    }
    if Instant::now() >= cleanup_deadline || Instant::now() >= pair_deadline {
        record.cleanup_failed = true;
    }
    record.success = case_passed(&record);
    record.failure_stage = if record.success {
        Stage::None
    } else if record.primary_failed {
        failure_stage
    } else if cleanup.is_err() {
        cleanup_stage
    } else {
        record.failure_stage
    };
    record
}

#[test]
#[ignore = "requires native Windows, exact normal CEDAR_AGENT_BIN, CEDAR_JAVA, Unicode CEDAR_JDTLS_HOME, and pinned CEDAR_MAVEN_CACHE_INPUT; run serially"]
fn real_windows_normal_agent_java_maven_acceptance() -> CheckResult<()> {
    println!();
    let _watchdog = Watchdog::start_with_timeout(PAIR_BUDGET);
    let started = Instant::now();
    let pair_deadline = started + PAIR_BUDGET;
    let mut record = MavenEvidence {
        schema_version: 1,
        kind: "windows_java_maven",
        route: "normal_agent_normal_client",
        pair_count: 1,
        cache_input_files: 0,
        cache_input_unchanged: false,
        fixture_inputs_verified: false,
        source_sha256: SOURCE_SHA256,
        pom_sha256: POM_SHA256,
        dependency_jar_sha256: JAR_SHA256,
        present: CaseEvidence::default(),
        missing: CaseEvidence {
            case: CaseKind::Missing,
            ..CaseEvidence::default()
        },
        elapsed_ms: 0,
        elapsed_saturated: false,
        primary_failed: false,
        cleanup_failed: false,
        success: false,
    };
    let result = checked(|| {
        for name in [
            "CLIENT_PORT",
            "CLIENT_HOST",
            "socket.stream.debug",
            "JDK_JAVA_OPTIONS",
            "JAVA_TOOL_OPTIONS",
            "_JAVA_OPTIONS",
            "MAVEN_OPTS",
            "MAVEN_ARGS",
            "MAVEN_CONFIG",
            "M2_HOME",
            "MAVEN_HOME",
        ] {
            require(
                std::env::var_os(name).is_none(),
                "unexpected Java or Maven launcher environment injection",
            )?;
        }
        let binary = environment_path("CEDAR_AGENT_BIN")?;
        let java = environment_path("CEDAR_JAVA")?;
        let distribution = environment_path("CEDAR_JDTLS_HOME")?;
        require(binary.is_file() && binary.file_name().is_some_and(|name| name == "cedar-agent.exe")
            && java.is_file() && java.file_name().is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("java.exe"))
            && text(&java)?.is_ascii() && distribution.is_dir() && !text(&distribution)?.is_ascii(),
            "Maven acceptance requires exact normal agent, ASCII java.exe and Unicode JDT distribution")?;
        let (input, inventory) = verified_cache()?;
        record.cache_input_files = inventory.len() as u16;
        sealed_jar()?;
        require(
            hash(FIXTURE_SOURCE.as_bytes()) == SOURCE_SHA256
                && hash(FIXTURE_POM.as_bytes()) == POM_SHA256,
            "Maven fixture source/POM differ from their fixed receipts",
        )?;
        record.fixture_inputs_verified = true;
        record.present = run_case(
            CaseKind::Present,
            &binary,
            &java,
            &distribution,
            &input,
            &inventory,
            pair_deadline,
        );
        if prior_cleanup_verified(&record.present)
            && budget_admits(
                Instant::now(),
                pair_deadline,
                case_budget(CaseKind::Missing),
                Duration::ZERO,
            )
        {
            record.missing = run_case(
                CaseKind::Missing,
                &binary,
                &java,
                &distribution,
                &input,
                &inventory,
                pair_deadline,
            );
        } else {
            // Both cases remain mandatory. A skipped second launch is a failed
            // pair, never permission to retry or extend the outer watchdog.
            record.missing.primary_failed = true;
            record.missing.failure_stage = Stage::Setup;
        }
        record.cache_input_unchanged = snapshot(&input).is_ok_and(|after| after == inventory);
        Ok(())
    });
    record.primary_failed =
        result.is_err() || record.present.primary_failed || record.missing.primary_failed;
    record.cleanup_failed = record.present.cleanup_failed
        || record.missing.cleanup_failed
        || !record.cache_input_unchanged;
    let elapsed = started.elapsed().as_millis();
    record.elapsed_ms = elapsed.min(360_000) as u32;
    record.elapsed_saturated = elapsed > 360_000;
    record.success = !record.primary_failed
        && !record.cleanup_failed
        && !record.elapsed_saturated
        && record.fixture_inputs_verified
        && record.cache_input_files == 83
        && record.present.success
        && record.missing.success
        && Instant::now() < pair_deadline;
    println!(
        "{}",
        serde_json::to_string(&record).expect("typed Maven acceptance receipt")
    );
    require(
        record.success,
        "native normal-agent Maven acceptance failed; inspect typed per-case receipt",
    )
}

#[test]
fn maven_pair_budget_reserves_cleanup_and_rejects_late_or_unverified_work() {
    let now = Instant::now();
    let pair_deadline = now + PAIR_BUDGET;
    assert_eq!(CLEANUP_BUDGET, Duration::from_secs(90));
    assert_eq!(PAIR_BUDGET, Duration::from_secs(360));
    assert_eq!(MODEL_BUDGET, Duration::from_secs(60));
    assert_eq!(DEPENDENCY_RPC_BUDGET, Duration::from_secs(75));
    assert_eq!(NO_SESSION_RPC_BUDGET, Duration::from_secs(30));
    assert!(budget_admits(
        now + Duration::from_secs(175),
        pair_deadline,
        DEPENDENCY_RPC_BUDGET,
        SEMANTIC_BUDGET + CLEANUP_BUDGET,
    ));
    assert!(!budget_admits(
        now + Duration::from_secs(176),
        pair_deadline,
        DEPENDENCY_RPC_BUDGET,
        SEMANTIC_BUDGET + CLEANUP_BUDGET,
    ));
    assert!(budget_admits(
        now + Duration::from_secs(195),
        pair_deadline,
        DEPENDENCY_RPC_BUDGET,
        CLEANUP_BUDGET,
    ));
    assert!(!budget_admits(
        now + Duration::from_secs(196),
        pair_deadline,
        DEPENDENCY_RPC_BUDGET,
        CLEANUP_BUDGET,
    ));
    assert_eq!(case_budget(CaseKind::Present), Duration::from_secs(245));
    assert_eq!(case_budget(CaseKind::Missing), Duration::from_secs(225));
    assert!(budget_admits(
        now,
        pair_deadline,
        case_budget(CaseKind::Present),
        Duration::ZERO
    ));
    assert!(!budget_admits(
        now + Duration::from_secs(136),
        pair_deadline,
        case_budget(CaseKind::Missing),
        Duration::ZERO
    ));
    assert!(!budget_admits(
        pair_deadline,
        pair_deadline,
        Duration::ZERO,
        Duration::ZERO
    ));
    let clipped = bounded_cleanup_deadline(now + Duration::from_secs(300), pair_deadline);
    assert_eq!(clipped, pair_deadline);
    assert!(!budget_admits(
        now + Duration::from_secs(300),
        pair_deadline,
        CLEANUP_BUDGET,
        Duration::ZERO
    ));
    assert_eq!(
        bounded_cleanup_deadline(now, pair_deadline),
        now + CLEANUP_BUDGET
    );
    let mut prior = CaseEvidence {
        stop_outcome_verified: true,
        cleanup_joined: true,
        root_handle_signaled: true,
        root_exit_code: Some(0),
        client_reaped: true,
        synthetic_root_removed: true,
        ..CaseEvidence::default()
    };
    assert!(prior_cleanup_verified(&prior));
    prior.cleanup_failed = true;
    assert!(!prior_cleanup_verified(&prior));
    prior.cleanup_failed = false;
    prior.client_reaped = false;
    assert!(!prior_cleanup_verified(&prior));
}

fn dependency_snapshot_fixture(present: bool, observed: bool) -> MavenDependenciesSnapshot {
    serde_json::from_value(serde_json::json!({
        "schema": 1,
        "profile": "maven_leaf",
        "startup_id": 7,
        "pom_path": "pom.xml",
        "pom_sha256": POM_SHA256,
        "declarations": [{
            "group_id": "dev.cedar.fixture", "artifact_id": "arithmetic", "version": "1.0.0",
            "classifier": null, "scope": "compile", "scope_explicit": false,
            "optional": false, "optional_explicit": false,
            "expected_jar_path": DEPENDENCY_JAR, "regular_file_present": present
        }],
        "observation": {"status": "available", "libraries": if observed {
            serde_json::json!([{"root": "local_repository", "relative_path": DEPENDENCY_JAR,
                "regular_file_present": present, "declaration_indices": [0]}])
        } else { serde_json::json!([]) }}
    }))
    .expect("fixed dependency snapshot fixture")
}

#[test]
fn maven_dependency_insight_keeps_actual_missing_observation_separate_from_absence(
) -> CheckResult<()> {
    for (present, observed, expected) in [
        (true, true, DependencyObservation::ObservedPresentFile),
        (false, true, DependencyObservation::ObservedAbsentFile),
        (false, false, DependencyObservation::NotObserved),
    ] {
        let snapshot = dependency_snapshot_fixture(present, observed);
        let mut record = CaseEvidence {
            case: if present {
                CaseKind::Present
            } else {
                CaseKind::Missing
            },
            ..CaseEvidence::default()
        };
        inspect_dependencies(&snapshot, 7, &mut record)?;
        assert_eq!(
            record.dependency_probe_outcome,
            DependencyProbeOutcome::Accepted
        );
        assert_eq!(record.dependency_observation, expected);
        assert_eq!(record.dependency_declaration_count, 1);
        assert_eq!(
            record.dependency_observed_library_count,
            if observed { 1 } else { 0 }
        );
        assert_eq!(record.dependency_declaration_file_present, present);
        assert!(record.dependency_snapshot_identity_verified);
        assert!(record.dependency_declaration_exact);
        assert!(record.dependency_default_provenance_verified);
        assert!(record.dependency_expected_jar_verified);
    }
    let mut record = CaseEvidence::default();
    assert!(
        inspect_dependencies(&dependency_snapshot_fixture(true, false), 7, &mut record).is_err()
    );
    assert_eq!(
        record.dependency_observation,
        DependencyObservation::NotObserved
    );
    assert_eq!(
        record.dependency_probe_outcome,
        DependencyProbeOutcome::SnapshotRejected
    );
    Ok(())
}

#[test]
fn maven_dependency_insight_rejects_identity_provenance_foreign_and_forged_observations() {
    let baseline = dependency_snapshot_fixture(true, true);
    let raw = serde_json::to_value(&baseline).unwrap();
    for (path, value) in [
        (vec!["schema"], serde_json::json!(2)),
        (vec!["startup_id"], serde_json::json!(8)),
        (vec!["profile"], serde_json::json!("other")),
        (vec!["pom_path"], serde_json::json!("other.xml")),
        (vec!["pom_sha256"], serde_json::json!("a".repeat(64))),
    ] {
        let mut invalid = raw.clone();
        invalid[path[0]] = value;
        let snapshot: MavenDependenciesSnapshot = serde_json::from_value(invalid).unwrap();
        let mut record = CaseEvidence::default();
        assert!(inspect_dependencies(&snapshot, 7, &mut record).is_err());
        assert!(!record.dependency_snapshot_identity_verified);
    }
    for (field, value) in [
        ("group_id", serde_json::json!("dev.foreign")),
        ("artifact_id", serde_json::json!("other")),
        ("version", serde_json::json!("2.0.0")),
        ("classifier", serde_json::json!("tests")),
        ("scope", serde_json::json!("test")),
        ("scope_explicit", serde_json::json!(true)),
        ("optional", serde_json::json!(true)),
        ("optional_explicit", serde_json::json!(true)),
        (
            "expected_jar_path",
            serde_json::json!("C:/private/SECRET_SENTINEL.jar"),
        ),
        ("regular_file_present", serde_json::json!(false)),
    ] {
        let mut invalid = raw.clone();
        invalid["declarations"][0][field] = value;
        let snapshot: MavenDependenciesSnapshot = serde_json::from_value(invalid).unwrap();
        let mut record = CaseEvidence::default();
        assert!(inspect_dependencies(&snapshot, 7, &mut record).is_err());
        assert_eq!(
            record.dependency_probe_outcome,
            DependencyProbeOutcome::SnapshotRejected
        );
        assert!(!serde_json::to_string(&record)
            .unwrap()
            .contains("SECRET_SENTINEL"));
    }
    for (field, value) in [
        ("root", serde_json::json!("workspace")),
        ("relative_path", serde_json::json!("foreign/library.jar")),
        ("regular_file_present", serde_json::json!(false)),
        ("declaration_indices", serde_json::json!([])),
        ("declaration_indices", serde_json::json!([1])),
        ("declaration_indices", serde_json::json!([0, 0])),
    ] {
        let mut invalid = raw.clone();
        invalid["observation"]["libraries"][0][field] = value;
        let snapshot: MavenDependenciesSnapshot = serde_json::from_value(invalid).unwrap();
        assert!(inspect_dependencies(&snapshot, 7, &mut CaseEvidence::default()).is_err());
    }
    for field in ["declarations", "observation"] {
        let mut invalid = raw.clone();
        if field == "declarations" {
            invalid[field] = serde_json::json!([]);
        } else {
            invalid[field] =
                serde_json::json!({"status": "unavailable", "reason": "model_unavailable"});
        }
        let snapshot: MavenDependenciesSnapshot = serde_json::from_value(invalid).unwrap();
        assert!(inspect_dependencies(&snapshot, 7, &mut CaseEvidence::default()).is_err());
    }
    let mut duplicate = baseline;
    if let MavenDependencyObservation::Available { libraries } = &mut duplicate.observation {
        libraries.push(libraries[0].clone());
    }
    assert!(inspect_dependencies(&duplicate, 7, &mut CaseEvidence::default()).is_err());
    let mut forged_missing = dependency_snapshot_fixture(false, true);
    if let MavenDependencyObservation::Available { libraries } = &mut forged_missing.observation {
        libraries[0].regular_file_present = true;
    }
    let mut record = CaseEvidence {
        case: CaseKind::Missing,
        ..CaseEvidence::default()
    };
    assert!(inspect_dependencies(&forged_missing, 7, &mut record).is_err());
    assert_eq!(
        record.dependency_observation,
        DependencyObservation::ObservedPresentFile
    );
    assert!(!record.dependency_declaration_file_present);
}

#[test]
fn maven_dependency_payload_rejects_legacy_language_values_and_sanitizes_errors() -> CheckResult<()>
{
    let mut record = CaseEvidence::default();
    assert!(dependency_payload(
        Ok(Payload::Language {
            value: serde_json::to_value(dependency_snapshot_fixture(true, true)).unwrap(),
        }),
        &mut record
    )
    .is_err());
    assert_eq!(
        record.dependency_probe_outcome,
        DependencyProbeOutcome::NonDependencyPayload
    );
    for (code, expected) in [
        (
            "language_maven_invalid_dependencies",
            ClientErrorCode::LanguageMavenInvalidDependencies,
        ),
        (
            "language_maven_stale_snapshot",
            ClientErrorCode::LanguageMavenStaleSnapshot,
        ),
        ("transport_timeout", ClientErrorCode::TransportFailure),
    ] {
        assert!(dependency_payload(Err(format!("{code}: SECRET_SENTINEL")), &mut record).is_err());
        assert_eq!(
            record.dependency_probe_outcome,
            DependencyProbeOutcome::RequestFailed
        );
        assert_eq!(record.dependency_error_code, expected);
        assert!(!serde_json::to_string(&record)
            .unwrap()
            .contains("SECRET_SENTINEL"));
    }
    dependency_payload(
        Ok(Payload::MavenDependencies {
            snapshot: dependency_snapshot_fixture(true, true),
        }),
        &mut record,
    )?;
    assert_eq!(
        record.dependency_probe_outcome,
        DependencyProbeOutcome::ResponseReceived
    );
    assert_eq!(record.dependency_error_code, ClientErrorCode::None);
    Ok(())
}

#[test]
fn maven_missing_exact_model_reference_is_expected_negative_evidence() -> CheckResult<()> {
    let paths = CasePaths {
        root: PathBuf::from(r"C:\owned\workspace 雪"),
        repository: PathBuf::from(r"C:\owned\repository 雪"),
        data: PathBuf::from(r"C:\owned\data"),
    };
    let expected = serde_json::json!({
        "profile":"maven_leaf", "status":"unresolved", "pom_path":"pom.xml",
        "pom_sha256":POM_SHA256, "restart_required":false, "maven_nature":true,
        "compiler":{"source":"17","compliance":"17","target":"17"},
        "source_paths":["source-java"], "unresolved_count":1,
        "classpath":[{"kind":"source","path":"source-java","resolved":true,"origin":"model"},
            {"kind":"library","path":text(&paths.repository.join(DEPENDENCY_JAR))?,"resolved":false,"origin":"model"}]
    });
    let mut record = CaseEvidence {
        case: CaseKind::Missing,
        ..CaseEvidence::default()
    };
    assert!(inspect_model(&expected, &paths, &mut record)?);
    assert!(record.exact_dependency_reference);
    assert_eq!(record.unexpected_dependency_references, 0);
    let mut extra = expected.clone();
    extra["classpath"].as_array_mut().unwrap().push(serde_json::json!({
        "kind":"library","path":text(&paths.repository.join("foreign/extra.jar"))?,"resolved":false,"origin":"model"
    }));
    assert!(inspect_model(&extra, &paths, &mut record).is_err());
    assert_eq!(record.unexpected_dependency_references, 1);
    let mut duplicate = expected.clone();
    duplicate["classpath"]
        .as_array_mut()
        .unwrap()
        .push(expected["classpath"][1].clone());
    assert!(inspect_model(&duplicate, &paths, &mut record).is_err());
    Ok(())
}

#[test]
fn maven_model_probe_receipt_distinguishes_request_payload_and_shape_failures() -> CheckResult<()> {
    let paths = CasePaths {
        root: PathBuf::from(r"C:\owned\workspace 雪"),
        repository: PathBuf::from(r"C:\owned\repository 雪"),
        data: PathBuf::from(r"C:\owned\data"),
    };
    let mut record = CaseEvidence::default();
    let error = "language_maven_invalid_model: never-export-private-detail";
    assert_eq!(
        model_payload(Err(error.into()), &mut record).unwrap_err(),
        error
    );
    assert_eq!(record.model_probe_outcome, ModelProbeOutcome::RequestFailed);
    assert_eq!(
        record.model_error_code,
        ClientErrorCode::LanguageMavenInvalidModel
    );
    assert_eq!(
        client_error_code("other: language_maven_invalid_model: never-export-private-detail"),
        ClientErrorCode::Other
    );
    assert_eq!(
        client_error_code("transport_timeout: never-export-private-detail"),
        ClientErrorCode::TransportFailure
    );
    assert_eq!(
        client_error_code("transport_unknown: never-export-private-detail"),
        ClientErrorCode::Other
    );
    assert!(model_payload(
        Ok(Payload::GitStatus {
            text: "never-export-private-detail".into()
        }),
        &mut record
    )
    .is_err());
    assert_eq!(
        record.model_probe_outcome,
        ModelProbeOutcome::NonLanguagePayload
    );
    assert_eq!(record.model_error_code, ClientErrorCode::None);
    let model = model_payload(Ok(Payload::Language { value: Value::Null }), &mut record)?;
    assert_eq!(
        record.model_probe_outcome,
        ModelProbeOutcome::ResponseReceived
    );
    assert!(inspect_model(&model, &paths, &mut record).is_err());
    assert_eq!(record.model_probe_outcome, ModelProbeOutcome::ModelRejected);
    assert_eq!(record.model_rejection, ModelRejection::ProfileOrPom);
    let mut model = serde_json::json!({
        "profile":"maven_leaf", "status":"unavailable", "pom_path":"pom.xml",
        "pom_sha256":POM_SHA256, "restart_required":false, "maven_nature":false
    });
    assert!(!inspect_model(&model, &paths, &mut record)?);
    assert_eq!(record.model_probe_outcome, ModelProbeOutcome::NotReady);
    assert_eq!(record.model_rejection, ModelRejection::None);
    model["status"] = "never-export-private-detail".into();
    assert!(inspect_model(&model, &paths, &mut record).is_err());
    assert_eq!(record.model_rejection, ModelRejection::Status);
    model["status"] = "imported".into();
    model["maven_nature"] = true.into();
    model["source_paths"] = serde_json::json!(["source-java"]);
    model["compiler"] = serde_json::json!({"source":"17","compliance":"17","target":"17"});
    assert!(inspect_model(&model, &paths, &mut record).is_err());
    assert_eq!(record.model_rejection, ModelRejection::MissingClasspath);
    let encoded = serde_json::to_string(&record).unwrap();
    assert!(!encoded.contains("never-export-private-detail"));
    assert!(!encoded.contains("C:\\owned"));
    Ok(())
}

#[test]
fn maven_event_probe_receipt_distinguishes_request_structure_and_diagnostic_rejection(
) -> CheckResult<()> {
    let paths = CasePaths {
        root: PathBuf::from(r"C:\owned\workspace 雪"),
        repository: PathBuf::from(r"C:\owned\repository 雪"),
        data: PathBuf::from(r"C:\owned\data"),
    };
    let mut record = CaseEvidence {
        case: CaseKind::Missing,
        ..CaseEvidence::default()
    };
    let mut diagnostics = DiagnosticState::default();
    let unavailable = serde_json::json!({
        "profile":"maven_leaf", "status":"unavailable", "pom_path":"pom.xml",
        "pom_sha256":POM_SHA256, "restart_required":false, "maven_nature":false
    });
    assert!(!inspect_model(&unavailable, &paths, &mut record)?);
    assert_eq!(record.model_probe_outcome, ModelProbeOutcome::NotReady);
    assert!(event_payload(
        Err("transport_read: never-export-private-detail".into()),
        &mut record
    )
    .is_err());
    assert_eq!(record.event_probe_outcome, EventProbeOutcome::RequestFailed);
    assert_eq!(record.event_error_code, ClientErrorCode::TransportFailure);
    assert!(event_payload(
        Ok(Payload::GitStatus {
            text: "never-export-private-detail".into()
        }),
        &mut record
    )
    .is_err());
    assert_eq!(
        record.event_probe_outcome,
        EventProbeOutcome::NonLanguagePayload
    );
    assert_eq!(record.event_error_code, ClientErrorCode::None);
    for (value, expected) in [
        (
            serde_json::json!({"truncated":true,"events":[]}),
            EventRejection::Truncated,
        ),
        (
            serde_json::json!({"truncated":false}),
            EventRejection::MissingEvents,
        ),
        (
            serde_json::json!({"truncated":false,"events":[{"type":"closed"}]}),
            EventRejection::ClosedEvent,
        ),
        (
            serde_json::json!({"truncated":false,"events":[{"type":"lagged"}]}),
            EventRejection::LaggedEvent,
        ),
        (
            serde_json::json!({"truncated":false,"events":[{}]}),
            EventRejection::MissingEventType,
        ),
        (
            serde_json::json!({"truncated":false,"events":[{"type":1}]}),
            EventRejection::MissingEventType,
        ),
        (
            serde_json::json!({"truncated":false,"events":[{"type":"new_server_event"}]}),
            EventRejection::UnknownEvent,
        ),
    ] {
        let value = event_payload(Ok(Payload::Language { value }), &mut record)?;
        assert_eq!(
            record.event_probe_outcome,
            EventProbeOutcome::ResponseReceived
        );
        assert!(record_events(&value, &paths, &mut record, &mut diagnostics, false).is_err());
        assert_eq!(
            record.event_probe_outcome,
            EventProbeOutcome::EventsRejected
        );
        assert_eq!(record.event_rejection, expected);
        assert_eq!(record.rejected_diagnostic_origin, DiagnosticOrigin::None);
    }
    let pom_uri =
        url::Url::from_file_path(paths.root.join("pom.xml")).map_err(|_| "test POM URI failed")?;
    let batch = |item: Value| {
        serde_json::json!({"truncated":false,"events":[{"type":"diagnostics","value":{
        "uri":pom_uri.as_str(),"diagnostics":[item]}}]})
    };
    for (item, shape, severity, message) in [
        (
            serde_json::json!({"severity":1,"code":0,"message":format!("Offline / Missing artifact {DEPENDENCY_GAV}")}),
            DiagnosticCodeShape::IntegerZero,
            DiagnosticSeverity::Error,
            DiagnosticMessageClass::OfflineOwnedDependency,
        ),
        (
            serde_json::json!({"severity":2,"code":"0","message":format!("Offline / Missing artifact {DEPENDENCY_GAV}")}),
            DiagnosticCodeShape::StringZero,
            DiagnosticSeverity::Warning,
            DiagnosticMessageClass::OfflineOwnedDependency,
        ),
        (
            serde_json::json!({"severity":1,"code":"0","message":format!("Missing artifact {DEPENDENCY_GAV}")}),
            DiagnosticCodeShape::StringZero,
            DiagnosticSeverity::Error,
            DiagnosticMessageClass::PlainMissingOwnedDependency,
        ),
        (
            serde_json::json!({"severity":1,"code":"0","message":"never-export-private-detail"}),
            DiagnosticCodeShape::StringZero,
            DiagnosticSeverity::Error,
            DiagnosticMessageClass::Other,
        ),
    ] {
        assert!(record_events(&batch(item), &paths, &mut record, &mut diagnostics, false).is_err());
        assert_eq!(
            record.event_rejection,
            EventRejection::UnexpectedPomDiagnostic
        );
        assert_eq!(record.rejected_diagnostic_origin, DiagnosticOrigin::Pom);
        assert_eq!(record.rejected_diagnostic_code_shape, shape);
        assert_eq!(record.rejected_diagnostic_severity, severity);
        assert_eq!(record.rejected_diagnostic_message_class, message);
        assert!(!diagnostics.offline_pom);
        assert_eq!(record.model_probe_outcome, ModelProbeOutcome::NotReady);
    }
    let mut missing_uri = batch(serde_json::json!({"code":"0"}));
    missing_uri["events"][0]["value"]
        .as_object_mut()
        .unwrap()
        .remove("uri");
    assert!(record_events(&missing_uri, &paths, &mut record, &mut diagnostics, false).is_err());
    assert_eq!(record.event_rejection, EventRejection::MissingDiagnosticUri);
    assert_eq!(record.rejected_diagnostic_origin, DiagnosticOrigin::Missing);
    assert_eq!(
        record.rejected_diagnostic_code_shape,
        DiagnosticCodeShape::None
    );
    let mut foreign = batch(
        serde_json::json!({"severity":1,"code":16777233,"message":"Type mismatch: cannot convert from int to String"}),
    );
    foreign["events"][0]["value"]["uri"] = "file:///C:/foreign/pom.xml".into();
    assert!(record_events(&foreign, &paths, &mut record, &mut diagnostics, false).is_err());
    assert_eq!(record.event_rejection, EventRejection::ForeignDocument);
    assert_eq!(record.rejected_diagnostic_origin, DiagnosticOrigin::Foreign);
    assert_eq!(
        record.rejected_diagnostic_code_shape,
        DiagnosticCodeShape::IntegerTypeMismatch
    );
    assert_eq!(
        record.rejected_diagnostic_message_class,
        DiagnosticMessageClass::DeliberateIntToString
    );
    let encoded = serde_json::to_string(&record).unwrap();
    assert!(!encoded.contains("never-export-private-detail"));
    assert!(!encoded.contains("file:///"));
    let accepted = batch(
        serde_json::json!({"severity":1,"code":"0","message":format!("Offline / Missing artifact {DEPENDENCY_GAV}")}),
    );
    record_events(&accepted, &paths, &mut record, &mut diagnostics, false)?;
    assert!(diagnostics.offline_pom);
    assert_eq!(
        record.event_probe_outcome,
        EventProbeOutcome::EventsAccepted
    );
    assert_eq!(record.event_rejection, EventRejection::None);
    assert_eq!(record.rejected_diagnostic_origin, DiagnosticOrigin::None);
    assert_eq!(
        record.rejected_diagnostic_code_shape,
        DiagnosticCodeShape::None
    );
    assert_eq!(
        record.rejected_diagnostic_severity,
        DiagnosticSeverity::None
    );
    assert_eq!(
        record.rejected_diagnostic_message_class,
        DiagnosticMessageClass::None
    );
    Ok(())
}

#[test]
fn maven_diagnostic_trace_classifies_only_fixed_codes_and_exact_messages() {
    for (code, shape) in [
        (Value::Null, DiagnosticCodeShape::Missing),
        (serde_json::json!("0"), DiagnosticCodeShape::StringZero),
        (
            serde_json::json!("16777233"),
            DiagnosticCodeShape::StringTypeMismatch,
        ),
        (
            serde_json::json!("964"),
            DiagnosticCodeShape::StringInvalidClasspath,
        ),
        (
            serde_json::json!("private-code"),
            DiagnosticCodeShape::OtherString,
        ),
        (serde_json::json!(0), DiagnosticCodeShape::IntegerZero),
        (
            serde_json::json!(16777233),
            DiagnosticCodeShape::IntegerTypeMismatch,
        ),
        (serde_json::json!(123), DiagnosticCodeShape::OtherInteger),
        (serde_json::json!(964), DiagnosticCodeShape::OtherInteger),
        (serde_json::json!(true), DiagnosticCodeShape::Other),
    ] {
        let mut trace = EventTrace::default();
        diagnostic_candidate(
            &mut trace,
            DiagnosticOrigin::Source,
            &serde_json::json!({"code":code,"severity":3,"message":"prefix Type mismatch: cannot convert from int to String"}),
        );
        assert_eq!(trace.code_shape, shape);
        assert_eq!(trace.severity, DiagnosticSeverity::Information);
        assert_eq!(trace.message_class, DiagnosticMessageClass::Other);
    }
}

#[test]
fn maven_offline_error_requires_exact_owned_pom_and_full_coordinate() -> CheckResult<()> {
    let paths = CasePaths {
        root: PathBuf::from(r"C:\owned\workspace 雪"),
        repository: PathBuf::from(r"C:\owned\repository 雪"),
        data: PathBuf::from(r"C:\owned\data"),
    };
    let uri =
        url::Url::from_file_path(paths.root.join("pom.xml")).map_err(|_| "test POM URI failed")?;
    let expected = serde_json::json!({"truncated":false,"events":[{"type":"diagnostics","value":{
        "uri":uri.as_str(),"diagnostics":[{"severity":1,"code":"0","message":format!("Offline / Missing artifact {DEPENDENCY_GAV}")}]}}]});
    let mut state = DiagnosticState::default();
    inspect_events(&expected, &paths, CaseKind::Missing, &mut state, false)?;
    assert!(state.offline_pom);
    for message in [
        "Missing artifact dev.cedar.fixture:arithmetic:jar:1.0.0",
        "Offline / Missing artifact dev.cedar.fixture:arithmetic:jar:2.0.0",
    ] {
        let mut wrong = expected.clone();
        wrong["events"][0]["value"]["diagnostics"][0]["message"] = message.into();
        assert!(inspect_events(
            &wrong,
            &paths,
            CaseKind::Missing,
            &mut DiagnosticState::default(),
            false
        )
        .is_err());
    }
    let mut foreign = expected.clone();
    foreign["events"][0]["value"]["uri"] = "file:///C:/foreign/pom.xml".into();
    assert!(inspect_events(
        &foreign,
        &paths,
        CaseKind::Missing,
        &mut DiagnosticState::default(),
        false
    )
    .is_err());
    assert!(inspect_events(
        &expected,
        &paths,
        CaseKind::Present,
        &mut DiagnosticState::default(),
        false
    )
    .is_err());
    Ok(())
}

fn missing_project_diagnostic_fixture() -> CheckResult<(tempfile::TempDir, CasePaths, Value)> {
    let fixture = io(tempfile::Builder::new()
        .prefix("cedar-maven-marker-")
        .tempdir())?;
    let base = ordinary_path(fixture.path())?;
    let paths = CasePaths {
        root: base.join("workspace 雪"),
        repository: base.join("repository 雪"),
        data: base.join("data"),
    };
    io(fs::create_dir(&paths.root))?;
    io(fs::create_dir_all(
        paths.repository.join(DEPENDENCY_DIRECTORY),
    ))?;
    let uri = url::Url::from_directory_path(&paths.root).map_err(|_| "test project URI failed")?;
    // Spell the expected Windows OS path independently of the classifier's
    // conversion from the slash-separated fixture constant.
    let jar = paths
        .repository
        .join(r"dev\cedar\fixture\arithmetic\1.0.0\arithmetic-1.0.0.jar");
    let expected = serde_json::json!({"truncated":false,"events":[{"type":"diagnostics","value":{
    "uri":uri.as_str(),"diagnostics":[{
        "severity":1,"code":"964","source":"Java",
        "range":{"start":{"line":0,"character":0},"end":{"line":0,"character":0}},
        "message":format!("The container 'Maven Dependencies' references non existing library '{}'", text(&jar)?)
    }]}}]});
    Ok((fixture, paths, expected))
}

#[test]
fn maven_project_missing_library_requires_exact_owned_marker() -> CheckResult<()> {
    let (_fixture, paths, expected) = missing_project_diagnostic_fixture()?;
    let mut record = CaseEvidence {
        case: CaseKind::Missing,
        ..CaseEvidence::default()
    };
    let mut state = DiagnosticState::default();
    record_events(&expected, &paths, &mut record, &mut state, false)?;
    assert!(record.owned_project_missing_library_diagnostic);
    assert!(state.owned_project_missing_library_diagnostic);
    assert!(!state.offline_pom);
    assert_eq!(record.event_rejection, EventRejection::None);
    assert_eq!(record.rejected_diagnostic_origin, DiagnosticOrigin::None);
    let encoded = serde_json::to_string(&record).unwrap();
    assert!(encoded.contains("\"owned_project_missing_library_diagnostic\":true"));
    assert!(!encoded.contains("file:"));
    assert!(!encoded.contains("non existing library"));
    assert!(!encoded.contains("workspace 雪"));
    assert!(!encoded.contains("repository 雪"));

    for (pointer, replacement) in [
        ("/code", serde_json::json!(964)),
        ("/code", serde_json::json!("963")),
        ("/code", Value::Null),
        ("/severity", serde_json::json!(2)),
        ("/severity", Value::Null),
        ("/source", serde_json::json!("java")),
        ("/source", Value::Null),
        ("/range/start/line", serde_json::json!(1)),
        ("/range/start/character", serde_json::json!(1)),
        ("/range/end/line", serde_json::json!(1)),
        ("/range/end/character", serde_json::json!(1)),
        ("/range", Value::Null),
        (
            "/message",
            serde_json::json!("The project was not built since its build path is incomplete"),
        ),
        (
            "/message",
            serde_json::json!("The container 'Maven Dependencies' references non existing library 'C:\\foreign\\repository 雪\\dev\\cedar\\fixture\\arithmetic\\1.0.0\\arithmetic-1.0.0.jar'"),
        ),
    ] {
        let mut wrong = expected.clone();
        *wrong["events"][0]["value"]["diagnostics"][0]
            .pointer_mut(pointer)
            .unwrap() = replacement;
        let mut record = CaseEvidence {
            case: CaseKind::Missing,
            ..CaseEvidence::default()
        };
        let mut state = DiagnosticState::default();
        assert!(record_events(&wrong, &paths, &mut record, &mut state, false).is_err());
        assert_eq!(
            record.event_rejection,
            EventRejection::UnexpectedProjectDiagnostic
        );
        assert_eq!(
            record.rejected_diagnostic_origin,
            DiagnosticOrigin::OwnedProjectRoot
        );
        assert_eq!(
            record.rejected_diagnostic_message_class,
            if pointer == "/message" {
                DiagnosticMessageClass::Other
            } else {
                DiagnosticMessageClass::OwnedMissingMavenLibrary
            }
        );
        assert!(!record.owned_project_missing_library_diagnostic);
        assert!(!state.owned_project_missing_library_diagnostic);
        assert!(!state.offline_pom);
    }
    Ok(())
}

#[test]
fn maven_project_missing_library_rejects_foreign_uri_and_present_case() -> CheckResult<()> {
    let (_fixture, paths, expected) = missing_project_diagnostic_fixture()?;
    let project_uri = expected["events"][0]["value"]["uri"].as_str().unwrap();
    let raw_uri = project_uri
        .replacen("file:///", "file:/", 1)
        .replace("%20", " ")
        .replace("%E9%9B%AA", "雪");
    let mut raw = expected.clone();
    raw["events"][0]["value"]["uri"] = raw_uri.into();
    let mut state = DiagnosticState::default();
    inspect_events(&raw, &paths, CaseKind::Missing, &mut state, false)?;
    assert!(state.owned_project_missing_library_diagnostic);
    for uri in [
        project_uri.replace("workspace", "foreign"),
        project_uri.trim_end_matches('/').to_owned(),
        format!("{project_uri}child/"),
        format!("{project_uri}?query"),
        format!("{project_uri}#fragment"),
        format!("{project_uri}%00"),
        format!("{project_uri}%GG"),
        project_uri.replacen("file:///", "file://localhost/", 1),
    ] {
        let mut wrong = expected.clone();
        wrong["events"][0]["value"]["uri"] = uri.into();
        let mut record = CaseEvidence {
            case: CaseKind::Missing,
            ..CaseEvidence::default()
        };
        let mut state = DiagnosticState::default();
        assert!(record_events(&wrong, &paths, &mut record, &mut state, false).is_err());
        assert_eq!(record.event_rejection, EventRejection::ForeignDocument);
        assert_eq!(record.rejected_diagnostic_origin, DiagnosticOrigin::Foreign);
        assert!(!record.owned_project_missing_library_diagnostic);
    }
    let mut present = CaseEvidence::default();
    assert!(record_events(
        &expected,
        &paths,
        &mut present,
        &mut DiagnosticState::default(),
        false
    )
    .is_err());
    assert!(!present.owned_project_missing_library_diagnostic);
    assert_eq!(
        present.event_rejection,
        EventRejection::UnexpectedProjectDiagnostic
    );
    assert_eq!(
        present.rejected_diagnostic_code_shape,
        DiagnosticCodeShape::StringInvalidClasspath
    );
    Ok(())
}

#[test]
fn maven_project_missing_library_requires_actual_jar_and_pom_absence() -> CheckResult<()> {
    let (_fixture, paths, expected) = missing_project_diagnostic_fixture()?;
    for relative in [DEPENDENCY_JAR, DEPENDENCY_POM] {
        let artifact = paths.repository.join(relative);
        for directory in [false, true] {
            if directory {
                io(fs::create_dir(&artifact))?;
            } else {
                io(fs::write(&artifact, b"owned marker test"))?;
            }
            let mut state = DiagnosticState::default();
            assert!(
                inspect_events(&expected, &paths, CaseKind::Missing, &mut state, false).is_err()
            );
            assert!(!state.owned_project_missing_library_diagnostic);
            assert_eq!(
                state.trace.rejection,
                EventRejection::UnexpectedProjectDiagnostic
            );
            if directory {
                io(fs::remove_dir(&artifact))?;
            } else {
                io(fs::remove_file(&artifact))?;
            }
        }
    }
    let mut state = DiagnosticState::default();
    inspect_events(&expected, &paths, CaseKind::Missing, &mut state, false)?;
    assert!(state.owned_project_missing_library_diagnostic);
    Ok(())
}

#[test]
fn maven_project_marker_does_not_replace_owned_offline_pom_witness() -> CheckResult<()> {
    let (_fixture, paths, mut expected) = missing_project_diagnostic_fixture()?;
    let pom_uri =
        url::Url::from_file_path(paths.root.join("pom.xml")).map_err(|_| "test POM URI failed")?;
    expected["events"].as_array_mut().unwrap().push(serde_json::json!({
        "type":"diagnostics","value":{"uri":pom_uri.as_str(),"diagnostics":[{
            "severity":1,"code":"0","message":format!("Offline / Missing artifact {DEPENDENCY_GAV}")
        }]}
    }));
    let mut state = DiagnosticState::default();
    inspect_events(&expected, &paths, CaseKind::Missing, &mut state, false)?;
    assert!(state.owned_project_missing_library_diagnostic);
    assert!(state.offline_pom);
    for message in [
        "Missing artifact dev.cedar.fixture:arithmetic:jar:1.0.0",
        "Offline / Missing artifact dev.cedar.fixture:arithmetic:jar:2.0.0",
    ] {
        let mut wrong = expected.clone();
        wrong["events"][1]["value"]["diagnostics"][0]["message"] = message.into();
        let mut state = DiagnosticState::default();
        assert!(inspect_events(&wrong, &paths, CaseKind::Missing, &mut state, false).is_err());
        assert!(state.owned_project_missing_library_diagnostic);
        assert!(!state.offline_pom);
        assert_eq!(
            state.trace.rejection,
            EventRejection::UnexpectedPomDiagnostic
        );
    }
    Ok(())
}
