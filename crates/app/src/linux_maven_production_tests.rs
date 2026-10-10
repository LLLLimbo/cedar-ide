//! Opt-in Linux Maven acceptance through the shipping agent and normal Client.
//! Each fresh case owns 360 seconds of work plus 120 seconds of cleanup. The
//! pair owns 960 seconds; the driver has a separate 1020-second emergency bound.
//! Backend-owned Linux Stop and normal Client reaping are the cleanup witnesses.
//! No Windows handles, source navigation, Maven goals or network isolation claims.
use super::*;
use cedar_client::Client;
use cedar_protocol::{
    AgentInfo, LanguageQueryKind, MavenDependenciesSnapshot, MavenDependencyObservation,
    MavenDependencyScope, MavenLibraryRoot, Operation, Payload,
};
use sha2::{Digest, Sha256};
use std::{
    cell::Cell,
    collections::BTreeMap,
    fs,
    io::Read,
    panic::{catch_unwind, AssertUnwindSafe},
    path::{Component, Path, PathBuf},
    rc::Rc,
    sync::mpsc,
    thread,
    time::Instant,
};

const PRIMARY: Duration = Duration::from_secs(360);
const CLEANUP: Duration = Duration::from_secs(120);
const CASE: Duration = Duration::from_secs(480);
const PAIR: Duration = Duration::from_secs(960);
const REQUEST: Duration = Duration::from_secs(75);
const SHORT_REQUEST: Duration = Duration::from_secs(30);
const STARTUP: Duration = Duration::from_secs(75);
const REAP: Duration = Duration::from_secs(30);
const INTERVAL: Duration = Duration::from_millis(250);
const CACHE_FILES: usize = 83;

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
fn directory_url(path: &Path) -> CheckResult<String> {
    url::Url::from_directory_path(ordinary_path(path)?)
        .map(String::from)
        .map_err(|_| "cannot encode owned directory URL".into())
}

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

fn budget_admits(now: Instant, deadline: Instant, cost: Duration, reserve: Duration) -> bool {
    now < deadline && deadline.saturating_duration_since(now) >= cost + reserve
}
#[derive(Clone)]
struct Budget {
    deadline: Instant,
    refused: Rc<Cell<bool>>,
}
impl Budget {
    fn run<T>(
        &self,
        cost: Duration,
        reserve: Duration,
        action: impl FnOnce() -> CheckResult<T>,
    ) -> CheckResult<T> {
        if !budget_admits(Instant::now(), self.deadline, cost, reserve) {
            self.refused.set(true);
            return Err("operation lacks its full unchanged RPC or reaping envelope".into());
        }
        let result = action();
        require(
            Instant::now() < self.deadline,
            "operation returned after its fixed deadline",
        )?;
        result
    }
}
struct AcceptanceClient {
    inner: Client,
    budget: Budget,
    active: bool,
    startup_attempted: bool,
    pending: Option<u64>,
}
impl AcceptanceClient {
    fn request(&mut self, operation: Operation) -> CheckResult<Payload> {
        let cost = if self.active
            && !matches!(
                &operation,
                Operation::Read { .. }
                    | Operation::LanguageStartJavaBegin { .. }
                    | Operation::LanguageStartJavaMavenBegin { .. }
                    | Operation::LanguageStartJavaPoll { .. }
                    | Operation::LanguageStartJavaCancel { .. }
            ) {
            REQUEST
        } else {
            SHORT_REQUEST
        };
        let budget = self.budget.clone();
        budget.run(cost, Duration::ZERO, || {
            if matches!(&operation, Operation::LanguageStartJavaMavenBegin { .. }) {
                self.startup_attempted = true;
            }
            let result = self.inner.request(operation);
            // Preserve ownership before a late response fails its deadline.
            if let Ok(Payload::Language { value }) = &result {
                if let Some(id) = value["startup_id"].as_u64().filter(|id| *id != 0) {
                    self.pending = Some(id);
                    if value["state"] == "ready" && value["language"]["started"] == true {
                        self.active = true;
                    }
                }
            }
            result
        })
    }
}
fn language(client: &mut AcceptanceClient, operation: Operation) -> CheckResult<Value> {
    match client.request(operation)? {
        Payload::Language { value } => Ok(value),
        _ => Err("non-language response".into()),
    }
}
fn rejected(client: &mut AcceptanceClient, operation: Operation, code: &str) -> CheckResult<()> {
    require(
        client.request(operation).is_err_and(|error| {
            error
                .split_once(':')
                .map_or(error.as_str(), |(actual, _)| actual)
                == code
        }),
        "expected typed rejection was not received",
    )
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum CaseKind {
    #[default]
    Present,
    Missing,
}
impl CaseKind {
    fn present(self) -> bool {
        self == Self::Present
    }
}
#[derive(Clone, Copy, Default, Serialize)]
#[serde(rename_all = "snake_case")]
enum Stage {
    #[default]
    None,
    Setup,
    Trust,
    Startup,
    Model,
    Dependencies,
    Semantics,
    PomChange,
    Stop,
    ClientReap,
    FixtureCleanup,
}
#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum ModelStatus {
    #[default]
    None,
    Imported,
    Unresolved,
    Unavailable,
}
#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum DependencyObservation {
    #[default]
    None,
    Present,
    Absent,
    NotObserved,
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
    EventBound,
    UnknownEvent,
    ClosedEvent,
    LaggedEvent,
    MissingEventType,
    MissingDiagnosticUri,
    MissingDiagnostics,
    DiagnosticBound,
    UnexpectedPomDiagnostic,
    UnexpectedSourceDiagnostic,
    UnexpectedProjectDiagnostic,
    ForeignDocument,
    UriEncoding,
    Other,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum DiagnosticOrigin {
    #[default]
    None,
    Pom,
    Source,
    OwnedProjectRoot,
    OwnedProjectWithoutTrailingSlash,
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
    IntegerInvalidClasspath,
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

#[derive(Default, Serialize)]
struct CaseEvidence {
    case: CaseKind,
    failure_stage: Stage,
    model_status: ModelStatus,
    dependency_observation: DependencyObservation,
    event_probe_outcome: EventProbeOutcome,
    event_error_code: ClientErrorCode,
    event_rejection: EventRejection,
    rejected_diagnostic_origin: DiagnosticOrigin,
    rejected_diagnostic_code_shape: DiagnosticCodeShape,
    rejected_diagnostic_severity: DiagnosticSeverity,
    rejected_diagnostic_message_class: DiagnosticMessageClass,
    rejected_diagnostic_source_java: bool,
    rejected_diagnostic_zero_range: bool,
    rejected_diagnostic_dependency_jar_absent: bool,
    rejected_diagnostic_dependency_pom_absent: bool,
    exact_capabilities: bool,
    trust_off_rejected: bool,
    trust_off_client_reaped: bool,
    model_without_session_rejected: bool,
    dependencies_without_session_rejected: bool,
    local_frontend_gates: bool,
    async_begin: bool,
    read_while_starting: bool,
    ready: bool,
    ascii_control_home: bool,
    maven_nature: bool,
    custom_source: bool,
    compiler_17: bool,
    exact_dependency_reference: bool,
    dependency_insight: bool,
    frontend_binding: bool,
    pom_restart_required: bool,
    dependencies_pom_restart_required: bool,
    source_unchanged: bool,
    pom_expected: bool,
    repository_inputs_unchanged: bool,
    jar_present_before: bool,
    jar_present_after: bool,
    pom_present_before: bool,
    pom_present_after: bool,
    hover: bool,
    completion: bool,
    deliberate_type_diagnostic: bool,
    dirty_change_acknowledged: bool,
    correction_diagnostics: bool,
    no_autosave: bool,
    offline_pom_diagnostic: bool,
    project_missing_library_diagnostic: bool,
    startup_cleanup_verified: bool,
    client_reaped: bool,
    synthetic_root_removed: bool,
    model_queries: u16,
    generated_metadata_files: u16,
    lifecycle_metadata_files: u8,
    lifecycle_metadata_mask: u8,
    foreign_repository_files: u16,
    generated_data_files: u32,
    generated_data_bytes: u64,
    generated_project_files: u16,
    generated_project_bytes: u64,
    stop: Option<LinuxStop>,
    primary_deadline_ms: u32,
    outer_deadline_ms: u32,
    cleanup_reserve_ms: u32,
    stop_timeout_ms: u32,
    client_reap_ms: u32,
    startup_request_timeout_ms: u32,
    request_timeout_ms: u32,
    elapsed_ms: u32,
    elapsed_saturated: bool,
    primary_failed: bool,
    cleanup_failed: bool,
    budget_refused: bool,
    success: bool,
}
impl CaseEvidence {
    fn new(case: CaseKind) -> Self {
        Self {
            case,
            primary_deadline_ms: 360_000,
            outer_deadline_ms: 480_000,
            cleanup_reserve_ms: 120_000,
            stop_timeout_ms: 75_000,
            client_reap_ms: 30_000,
            startup_request_timeout_ms: 30_000,
            request_timeout_ms: 75_000,
            ..Self::default()
        }
    }
}
#[derive(Serialize)]
struct Evidence {
    schema_version: u8,
    kind: &'static str,
    route: &'static str,
    source_sha256: &'static str,
    pom_sha256: &'static str,
    dependency_jar_sha256: &'static str,
    cases: [CaseEvidence; 2],
    pair_deadline_ms: u32,
    cache_input_files: u16,
    cache_input_unchanged: bool,
    fixture_inputs_verified: bool,
    elapsed_ms: u32,
    elapsed_saturated: bool,
    primary_failed: bool,
    cleanup_failed: bool,
    success: bool,
}

// The frozen synthetic source, POM and 422-byte JAR match the Windows fixture.
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
                !metadata.file_type().is_symlink(),
                "Maven cache inventory contains a link",
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
            let mut contents = Vec::new();
            io(io(fs::File::open(&path))?
                .take(metadata.len() + 1)
                .read_to_end(&mut contents))?;
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
            && ordinary_path(&mirror)? == mirror
            && io(fs::read_dir(&mirror))?.next().is_none(),
        "Maven owned file mirror must remain an empty ordinary directory",
    )?;
    directory_url(&mirror)
}

fn check_repository(
    paths: &CasePaths,
    before: &Inventory,
    record: &mut CaseEvidence,
) -> CheckResult<()> {
    record.jar_present_after = !absent(&paths.repository.join(DEPENDENCY_JAR))?;
    record.pom_present_after = !absent(&paths.repository.join(DEPENDENCY_POM))?;
    // Check every immutable input independently before inspecting any new
    // metadata. Oversized or invalid new files cannot suppress these checks.
    record.repository_inputs_unchanged = before.iter().all(|(relative, seal)| {
        let path = paths.repository.join(relative);
        fs::symlink_metadata(&path).is_ok_and(|metadata| {
            metadata.is_file()
                && !metadata.file_type().is_symlink()
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
            && record.jar_present_after == record.case.present()
            && record.pom_present_after == record.case.present(),
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
                !metadata.file_type().is_symlink(),
                "generated Maven metadata contains a link",
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

fn capabilities(client: &Client) -> CheckResult<AgentInfo> {
    let Payload::Hello {
        protocol: 4,
        agent: Some(info),
        ..
    } = client.handshake()
    else {
        return Err("shipping Linux agent metadata missing".into());
    };
    info.validate().map_err(|_| "invalid agent metadata")?;
    require(
        info.os == "linux"
            && info.arch == "x86_64"
            && info.schema == 1
            && info.version == env!("CARGO_PKG_VERSION")
            && info
                .capabilities
                .iter()
                .map(String::as_str)
                .eq(EXPECTED_CAPABILITIES.iter().copied())
            && info
                .capability_groups
                .iter()
                .map(String::as_str)
                .eq(["java_maven_dependencies_v1", "java_maven_leaf_v1"]),
        "Linux shipping agent must advertise exact 31 flat capabilities and two known Maven groups",
    )?;
    Ok(info.clone())
}

fn inspect_model(value: &Value, paths: &CasePaths, record: &mut CaseEvidence) -> CheckResult<bool> {
    require(
        value["profile"] == "maven_leaf"
            && value["pom_path"] == "pom.xml"
            && value["pom_sha256"] == POM_SHA256
            && value["restart_required"] == false,
        "Maven model profile or owned POM identity mismatch",
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
    require(entries.len() <= 256, "Maven classpath bound exceeded")?;
    let mut expected = 0;
    for entry in entries {
        match entry["kind"].as_str() {
            Some("source") => {
                let relative =
                    Path::new(entry["path"].as_str().ok_or("Maven source path missing")?);
                require(
                    !relative.is_absolute()
                        && relative
                            .components()
                            .all(|part| matches!(part, Component::Normal(_))),
                    "Maven source path escaped the owned leaf",
                )?;
            }
            Some("library") => {
                require(
                    entry["path"].as_str() == paths.repository.join(DEPENDENCY_JAR).to_str()
                        && entry["resolved"] == record.case.present()
                        && (entry["origin"] == "model"
                            || (!record.case.present() && entry["origin"] == "declared")),
                    "Maven library identity, kind or resolution differs from fixture",
                )?;
                expected += 1;
            }
            Some("container") => require(
                entry["path"].as_str().is_some_and(|path| {
                    path.starts_with("org.eclipse.jdt.launching.JRE_CONTAINER")
                }),
                "foreign Maven container",
            )?,
            _ => return Err("unexpected Maven classpath entry kind".into()),
        }
    }
    require(expected <= 1, "duplicate Maven library reference")?;
    record.exact_dependency_reference = expected == 1;
    Ok(record.exact_dependency_reference
        && record.model_status
            == if record.case.present() {
                ModelStatus::Imported
            } else {
                ModelStatus::Unresolved
            }
        && value["unresolved_count"] == if record.case.present() { 0 } else { 1 })
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

#[derive(Clone, Copy, Default)]
struct EventTrace {
    rejection: EventRejection,
    origin: DiagnosticOrigin,
    code_shape: DiagnosticCodeShape,
    severity: DiagnosticSeverity,
    message_class: DiagnosticMessageClass,
    source_java: bool,
    zero_range: bool,
    dependency_jar_absent: bool,
    dependency_pom_absent: bool,
}

fn diagnostic_candidate(
    trace: &mut EventTrace,
    origin: DiagnosticOrigin,
    item: &Value,
    paths: &CasePaths,
) {
    trace.origin = origin;
    trace.source_java = item["source"] == "Java";
    trace.zero_range =
        item["range"] == json!({"start":{"line":0,"character":0},"end":{"line":0,"character":0}});
    trace.dependency_jar_absent =
        absent(&paths.repository.join(DEPENDENCY_JAR)).is_ok_and(|value| value);
    trace.dependency_pom_absent =
        absent(&paths.repository.join(DEPENDENCY_POM)).is_ok_and(|value| value);
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
        Value::Number(code) if code.as_i64() == Some(964) => {
            DiagnosticCodeShape::IntegerInvalidClasspath
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
        Some(message) if paths.repository.join(DEPENDENCY_JAR).to_str().is_some_and(|path|
            message == format!("The container 'Maven Dependencies' references non existing library '{path}'")) => DiagnosticMessageClass::OwnedMissingMavenLibrary,
        _ => DiagnosticMessageClass::Other,
    };
}

#[derive(Default)]
struct Diagnostics {
    trace: EventTrace,
    type_error: bool,
    corrected: bool,
    offline_pom: bool,
    missing_project: bool,
}
fn exact_type_diagnostic(item: &Value) -> bool {
    item["severity"] == 1
        && item["code"] == "16777233"
        && item["message"] == "Type mismatch: cannot convert from int to String"
        && item["range"]
            == json!({"start":{"line":5,"character":25},"end":{"line":5,"character":30}})
}
fn owned_project_marker_uri(actual: &str, expected: &str, kind: CaseKind) -> bool {
    // JDT's Linux project marker can name the exact owned directory without
    // its terminal slash. Accept that single spelling only for this missing
    // fixture; leave all shared URI comparisons and other documents unchanged.
    same_local_uri(actual, expected)
        || (!kind.present()
            && expected
                .strip_suffix('/')
                .is_some_and(|without_slash| same_local_uri(actual, without_slash)))
}
fn inspect_events(
    value: &Value,
    paths: &CasePaths,
    kind: CaseKind,
    state: &mut Diagnostics,
    correction: bool,
) -> CheckResult<()> {
    state.trace = EventTrace::default();
    let result = inspect_events_inner(value, paths, kind, state, correction);
    if let Err(error) = &result {
        state.trace.rejection = match error.as_str() {
            "Maven event stream truncated" => EventRejection::Truncated,
            "Maven event list missing" => EventRejection::MissingEvents,
            "Maven event list exceeds bound" => EventRejection::EventBound,
            "diagnostic URI missing" => EventRejection::MissingDiagnosticUri,
            "diagnostic list missing" => EventRejection::MissingDiagnostics,
            "Maven diagnostic list exceeds bound" => EventRejection::DiagnosticBound,
            "unexpected owned POM diagnostic" => EventRejection::UnexpectedPomDiagnostic,
            "unexpected owned source diagnostic" => EventRejection::UnexpectedSourceDiagnostic,
            "unexpected owned project diagnostic" => EventRejection::UnexpectedProjectDiagnostic,
            "foreign nonempty diagnostic batch" => EventRejection::ForeignDocument,
            "source URI failed" | "POM URI failed" | "project URI failed" => {
                EventRejection::UriEncoding
            }
            "Maven event stream closed, lagged or malformed" => state.trace.rejection,
            _ => EventRejection::Other,
        };
    } else {
        // Accepted candidates never masquerade as rejected diagnostics.
        state.trace = EventTrace::default();
    }
    result
}
fn inspect_events_inner(
    value: &Value,
    paths: &CasePaths,
    kind: CaseKind,
    state: &mut Diagnostics,
    correction: bool,
) -> CheckResult<()> {
    require(value["truncated"] == false, "Maven event stream truncated")?;
    let events = value["events"]
        .as_array()
        .ok_or("Maven event list missing")?;
    require(events.len() <= 4096, "Maven event list exceeds bound")?;
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
                state.trace = EventTrace {
                    origin: DiagnosticOrigin::Missing,
                    ..EventTrace::default()
                };
                let batch = &event["value"];
                let uri = batch["uri"].as_str().ok_or("diagnostic URI missing")?;
                state.trace.origin = if same_local_uri(uri, pom_uri.as_str()) {
                    DiagnosticOrigin::Pom
                } else if same_local_uri(uri, source_uri.as_str()) {
                    DiagnosticOrigin::Source
                } else if same_local_uri(uri, project_uri.as_str()) {
                    DiagnosticOrigin::OwnedProjectRoot
                } else if same_local_uri(uri, project_uri.as_str().trim_end_matches('/')) {
                    // Retain the observed spelling as a finite diagnostic tag.
                    DiagnosticOrigin::OwnedProjectWithoutTrailingSlash
                } else {
                    DiagnosticOrigin::Foreign
                };
                let items = batch["diagnostics"]
                    .as_array()
                    .ok_or("diagnostic list missing")?;
                require(items.len() <= 4096, "Maven diagnostic list exceeds bound")?;
                if same_local_uri(uri, pom_uri.as_str()) {
                    for item in items {
                        diagnostic_candidate(&mut state.trace, DiagnosticOrigin::Pom, item, paths);
                        require(
                            !kind.present()
                                && item["severity"] == 1
                                && item["code"] == "0"
                                && item["message"]
                                    == format!("Offline / Missing artifact {DEPENDENCY_GAV}"),
                            "unexpected owned POM diagnostic",
                        )?;
                        state.offline_pom = true;
                    }
                } else if same_local_uri(uri, source_uri.as_str()) {
                    if correction
                        && batch
                            .get("version")
                            .is_none_or(|version| version.is_null() || version == 2)
                    {
                        // A later eligible nonempty batch must supersede an
                        // earlier empty batch in this same ordered response.
                        state.corrected = items.is_empty();
                    }
                    for item in items {
                        diagnostic_candidate(
                            &mut state.trace,
                            DiagnosticOrigin::Source,
                            item,
                            paths,
                        );
                        if exact_type_diagnostic(item) {
                            if batch
                                .get("version")
                                .is_none_or(|version| version.is_null() || version == 1)
                            {
                                state.type_error = true;
                            }
                        } else {
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
                                "unexpected owned source diagnostic",
                            )?;
                        }
                    }
                } else if owned_project_marker_uri(uri, project_uri.as_str(), kind) {
                    // Source-supported JDT Core INVALID_CLASSPATH (964), m2e
                    // retained absent JAR, JDT LS Java/zero-range project marker.
                    // Linux paths remain exact and case-sensitive here.
                    for item in items {
                        let origin = state.trace.origin;
                        diagnostic_candidate(&mut state.trace, origin, item, paths);
                        require(!kind.present() && item["severity"] == 1 && item["code"] == "964"
                            && item["source"] == "Java"
                            && item["range"] == json!({"start":{"line":0,"character":0},"end":{"line":0,"character":0}})
                            && item["message"] == format!("The container 'Maven Dependencies' references non existing library '{}'",
                                text(&paths.repository.join(DEPENDENCY_JAR))?)
                            && absent(&paths.repository.join(DEPENDENCY_JAR))?
                            && absent(&paths.repository.join(DEPENDENCY_POM))?, "unexpected owned project diagnostic")?;
                        state.missing_project = true;
                    }
                } else {
                    if let Some(item) = items.first() {
                        let origin = state.trace.origin;
                        diagnostic_candidate(&mut state.trace, origin, item, paths);
                    }
                    require(items.is_empty(), "foreign nonempty diagnostic batch")?;
                }
            }
            _ => {
                state.trace = EventTrace {
                    rejection: match event["type"].as_str() {
                        Some("closed") => EventRejection::ClosedEvent,
                        Some("lagged") => EventRejection::LaggedEvent,
                        None => EventRejection::MissingEventType,
                        Some(_) => EventRejection::UnknownEvent,
                    },
                    ..EventTrace::default()
                };
                return Err("Maven event stream closed, lagged or malformed".into());
            }
        }
    }
    Ok(())
}
fn events(
    client: &mut AcceptanceClient,
    paths: &CasePaths,
    record: &mut CaseEvidence,
    state: &mut Diagnostics,
    correction: bool,
) -> CheckResult<()> {
    record_event_response(
        client.request(Operation::LanguageEvents),
        paths,
        record,
        state,
        correction,
    )
}
fn record_event_response(
    response: CheckResult<Payload>,
    paths: &CasePaths,
    record: &mut CaseEvidence,
    state: &mut Diagnostics,
    correction: bool,
) -> CheckResult<()> {
    state.trace = EventTrace::default();
    record.event_error_code = ClientErrorCode::None;
    let result = match response {
        Err(error) => {
            record.event_probe_outcome = EventProbeOutcome::RequestFailed;
            record.event_error_code = client_error_code(&error);
            Err("Maven event request failed".into())
        }
        Ok(Payload::Language { value }) => {
            record.event_probe_outcome = EventProbeOutcome::ResponseReceived;
            let result = inspect_events(&value, paths, record.case, state, correction);
            record.event_probe_outcome = if result.is_ok() {
                EventProbeOutcome::EventsAccepted
            } else {
                EventProbeOutcome::EventsRejected
            };
            result
        }
        Ok(_) => {
            record.event_probe_outcome = EventProbeOutcome::NonLanguagePayload;
            Err("Maven event response was not a language payload".into())
        }
    };
    record.event_rejection = state.trace.rejection;
    record.rejected_diagnostic_origin = state.trace.origin;
    record.rejected_diagnostic_code_shape = state.trace.code_shape;
    record.rejected_diagnostic_severity = state.trace.severity;
    record.rejected_diagnostic_message_class = state.trace.message_class;
    record.rejected_diagnostic_source_java = state.trace.source_java;
    record.rejected_diagnostic_zero_range = state.trace.zero_range;
    record.rejected_diagnostic_dependency_jar_absent = state.trace.dependency_jar_absent;
    record.rejected_diagnostic_dependency_pom_absent = state.trace.dependency_pom_absent;
    // Preserve earlier accepted observations even when a later event rejects
    // this batch. The rejected batch still fails the original predicate.
    record.offline_pom_diagnostic = state.offline_pom;
    record.project_missing_library_diagnostic = state.missing_project;
    result
}
fn await_model(
    client: &mut AcceptanceClient,
    paths: &CasePaths,
    record: &mut CaseEvidence,
    state: &mut Diagnostics,
) -> CheckResult<()> {
    for _ in 0..240 {
        record.model_queries += 1;
        let ready = inspect_model(
            &language(client, Operation::LanguageMavenModel)?,
            paths,
            record,
        )?;
        events(client, paths, record, state, false)?;
        if ready && (record.case.present() || (state.offline_pom && state.missing_project)) {
            return Ok(());
        }
        client.budget.run(INTERVAL, Duration::ZERO, || {
            thread::sleep(INTERVAL);
            Ok(())
        })?;
    }
    Err("Maven model did not establish its bounded present/missing witness".into())
}

fn dependencies_operation(startup_id: u64) -> Operation {
    Operation::LanguageMavenDependencies {
        startup_id,
        pom_sha256: POM_SHA256.into(),
    }
}
fn inspect_dependencies(
    snapshot: &MavenDependenciesSnapshot,
    id: u64,
    record: &mut CaseEvidence,
) -> CheckResult<()> {
    snapshot
        .validate_for(id, POM_SHA256, false)
        .map_err(|_| "Maven dependency snapshot identity rejected")?;
    require(
        snapshot.declarations.len() == 1,
        "dependency declaration count differs",
    )?;
    let declaration = &snapshot.declarations[0];
    require(
        declaration.group_id == "dev.cedar.fixture"
            && declaration.artifact_id == "arithmetic"
            && declaration.version == "1.0.0"
            && declaration.classifier.is_none()
            && declaration.scope == MavenDependencyScope::Compile
            && !declaration.scope_explicit
            && !declaration.optional
            && !declaration.optional_explicit
            && declaration.expected_jar_path == DEPENDENCY_JAR
            && declaration.regular_file_present == record.case.present(),
        "dependency declaration or default provenance differs from the sealed fixture",
    )?;
    let MavenDependencyObservation::Available { libraries } = &snapshot.observation else {
        return Err("Maven dependency insight did not observe the JDT model".into());
    };
    require(libraries.len() <= 1, "extra observed dependency libraries")?;
    if let Some(library) = libraries.first() {
        require(
            library.root == MavenLibraryRoot::LocalRepository
                && library.relative_path == DEPENDENCY_JAR
                && library.declaration_indices == [0]
                && library.regular_file_present == record.case.present(),
            "observed dependency identity or file presence differs",
        )?;
        record.dependency_observation = if library.regular_file_present {
            DependencyObservation::Present
        } else {
            DependencyObservation::Absent
        };
    } else {
        require(!record.case.present(), "JDT omitted the present dependency")?;
        record.dependency_observation = DependencyObservation::NotObserved;
    }
    record.dependency_insight = true;
    Ok(())
}
fn dependency_query(
    client: &mut AcceptanceClient,
    paths: &CasePaths,
    id: u64,
    info: &AgentInfo,
    local_info: &AgentInfo,
    record: &mut CaseEvidence,
) -> CheckResult<()> {
    let Payload::MavenDependencies { snapshot } = client.request(dependencies_operation(id))?
    else {
        return Err("dependency insight returned an untyped response".into());
    };
    inspect_dependencies(&snapshot, id, record)?;
    let ctx = eframe::egui::Context::default();
    let mut document = Document::new(1, SOURCE_FILE.into(), FIXTURE_SOURCE.into(), "r0".into());
    editor_state::commit(
        &ctx,
        &mut document,
        format!("{FIXTURE_SOURCE}// unsaved dependency view witness\n"),
        0,
    );
    crate::language_ui::verify_native_linux_maven_dependencies(
        &snapshot,
        id,
        POM_SHA256,
        &mut document,
        &ctx,
        info,
        local_info,
    )?;
    require(
        document.dirty() && source_unchanged(paths),
        "dependency view lost its dirty source baseline",
    )?;
    record.frontend_binding = true;
    record.local_frontend_gates = true;
    client.budget.run(Duration::ZERO, Duration::ZERO, || Ok(()))
}

fn semantics(
    client: &mut AcceptanceClient,
    paths: &CasePaths,
    record: &mut CaseEvidence,
    diagnostics: &mut Diagnostics,
) -> CheckResult<()> {
    let opened = language(
        client,
        Operation::LanguageOpen {
            path: SOURCE_FILE.into(),
            language_id: "java".into(),
            version: 1,
            text: FIXTURE_SOURCE.into(),
        },
    )?;
    let expected =
        url::Url::from_file_path(paths.root.join(SOURCE_FILE)).map_err(|_| "source URI failed")?;
    require(
        opened["version"] == 1
            && opened["opened"]
                .as_str()
                .is_some_and(|uri| same_local_uri(uri, expected.as_str())),
        "didOpen identity mismatch",
    )?;
    let cursor = completion::byte_to_position(
        FIXTURE_SOURCE,
        FIXTURE_SOURCE
            .find("Arithmetic.answer")
            .ok_or("fixture call missing")?
            + "Arithmetic.an".len(),
    )?;
    let query = |kind| Operation::LanguageQuery {
        path: SOURCE_FILE.into(),
        line: cursor.line,
        character: cursor.character,
        kind,
    };
    require(
        crate::language_results::hover_text(&language(client, query(LanguageQueryKind::Hover))?)
            .contains("int cedar.fixture.Arithmetic.answer()"),
        "hover did not resolve exact generated dependency method",
    )?;
    record.hover = true;
    let completion = language(client, query(LanguageQueryKind::Completion))?;
    require(
        completion::parse_completion_result(&completion)?
            .candidates
            .iter()
            .any(|item| {
                item.label == "answer() : int"
                    && item.item["detail"] == "Arithmetic.answer() : int"
                    && item.item["textEdit"]["newText"] == "answer"
            }),
        "completion did not resolve exact generated dependency method",
    )?;
    record.completion = true;
    while !diagnostics.type_error {
        events(client, paths, record, diagnostics, false)?;
        if !diagnostics.type_error {
            client.budget.run(INTERVAL, Duration::ZERO, || {
                thread::sleep(INTERVAL);
                Ok(())
            })?;
        }
    }
    record.deliberate_type_diagnostic = true;
    let ctx = eframe::egui::Context::default();
    let mut document = Document::new(1, SOURCE_FILE.into(), FIXTURE_SOURCE.into(), "r0".into());
    let corrected = FIXTURE_SOURCE.replace(
        "String invalid = known;",
        "String invalid = Integer.toString(known);",
    );
    editor_state::commit(&ctx, &mut document, corrected.clone(), 0);
    require(
        document.dirty()
            && document.saved_text == FIXTURE_SOURCE
            && document.text == corrected
            && document.edit_version == 1
            && source_unchanged(paths),
        "frontend transaction lost dirty source baseline",
    )?;
    let changed = language(
        client,
        Operation::LanguageChange {
            path: SOURCE_FILE.into(),
            version: 2,
            text: document.text.clone(),
        },
    )?;
    require(
        changed["version"] == 2
            && changed["changed"]
                .as_str()
                .is_some_and(|uri| same_local_uri(uri, expected.as_str())),
        "didChange identity mismatch",
    )?;
    record.dirty_change_acknowledged = true;
    while !diagnostics.corrected {
        events(client, paths, record, diagnostics, true)?;
        if !diagnostics.corrected {
            client.budget.run(INTERVAL, Duration::ZERO, || {
                thread::sleep(INTERVAL);
                Ok(())
            })?;
        }
    }
    record.correction_diagnostics = true;
    require(
        document.dirty() && source_unchanged(paths),
        "unsaved correction saved source",
    )?;
    record.no_autosave = true;
    let closed = language(
        client,
        Operation::LanguageClose {
            path: SOURCE_FILE.into(),
        },
    )?;
    require(
        closed["closed"]
            .as_str()
            .is_some_and(|uri| same_local_uri(uri, expected.as_str())),
        "didClose identity mismatch",
    )
}

fn connect(
    slot: &mut Option<AcceptanceClient>,
    budget: &Budget,
    binary: &Path,
    root: &Path,
    trust: bool,
) -> CheckResult<()> {
    budget.run(REQUEST, Duration::ZERO, || {
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
            budget: budget.clone(),
            active: false,
            startup_attempted: false,
            pending: None,
        });
        Ok(())
    })
}
fn reap(slot: &mut Option<AcceptanceClient>, budget: &Budget) -> CheckResult<()> {
    budget.run(REAP, Duration::ZERO, || {
        slot.take()
            .ok_or("missing Client")?
            .inner
            .close_and_wait(REAP)
    })
}
fn startup(
    client: &mut AcceptanceClient,
    operation: Operation,
    record: &mut CaseEvidence,
) -> CheckResult<u64> {
    let primary_deadline = client.budget.deadline;
    client.budget.deadline = primary_deadline.min(Instant::now() + STARTUP);
    let result = checked(|| {
        let begin = language(client, operation)?;
        let id = client
            .pending
            .ok_or("Maven Begin omitted startup identity")?;
        require(
            begin["state"] == "starting",
            "Begin omitted asynchronous Starting acknowledgement",
        )?;
        record.async_begin = true;
        require(
            matches!(client.request(Operation::Read { path:SOURCE_FILE.into() })?,
            Payload::File {path, text, ..} if path == SOURCE_FILE && text == FIXTURE_SOURCE),
            "ordinary Read failed while Maven startup was pending",
        )?;
        record.read_while_starting = true;
        loop {
            let response = language(client, Operation::LanguageStartJavaPoll { startup_id: id })?;
            require(
                response["startup_id"] == id,
                "Maven startup identity changed",
            )?;
            match response["state"].as_str() {
                Some("ready") => {
                    let ready = &response["language"];
                    require(
                        ready["started"] == true
                            && ready["initialize"]["cedar_java_profile"] == "maven_leaf"
                            && ready["initialize"]["cedar_java_maven_model"] == true
                            && ready["initialize"]["cedar_java_maven_pom_sha256"] == POM_SHA256,
                        "Maven Ready lacks exact supported profile and POM identity",
                    )?;
                    record.ready = true;
                    return Ok(id);
                }
                Some("starting") => client.budget.run(INTERVAL, Duration::ZERO, || {
                    thread::sleep(INTERVAL);
                    Ok(())
                })?,
                _ => return Err("Maven startup failed or returned invalid state".into()),
            }
        }
    });
    client.budget.deadline = primary_deadline;
    result
}
fn cancel(client: &mut AcceptanceClient, id: u64, budget: &Budget) -> CheckResult<()> {
    let mut operation = Operation::LanguageStartJavaCancel { startup_id: id };
    loop {
        let value = budget.run(SHORT_REQUEST, REAP, || {
            match client.inner.request(operation) {
                Ok(Payload::Language { value }) => Ok(value),
                _ => Err("Maven cancellation failed".into()),
            }
        })?;
        require(
            value["startup_id"] == id,
            "Maven cancellation identity mismatch",
        )?;
        match value["state"].as_str() {
            Some("cancelled" | "failed") => {
                return require(
                    value["cleanup_verified"] == true,
                    "startup cleanup unverified",
                )
            }
            Some("cancelling") => {}
            _ => return Err("unexpected startup cancellation state".into()),
        }
        budget.run(INTERVAL, REAP, || {
            thread::sleep(INTERVAL);
            Ok(())
        })?;
        operation = Operation::LanguageStartJavaPoll { startup_id: id };
    }
}
fn cleanup_owned(record: &CaseEvidence) -> bool {
    record.client_reaped
        && (record.stop.as_ref().is_some_and(|stop| stop.cleanup_joined)
            || record.startup_cleanup_verified)
}
fn case_passed(r: &CaseEvidence) -> bool {
    [
        r.exact_capabilities,
        r.trust_off_rejected,
        r.trust_off_client_reaped,
        r.model_without_session_rejected,
        r.dependencies_without_session_rejected,
        r.local_frontend_gates,
        r.async_begin,
        r.read_while_starting,
        r.ready,
        r.ascii_control_home,
        r.maven_nature,
        r.custom_source,
        r.compiler_17,
        r.exact_dependency_reference,
        r.dependency_insight,
        r.frontend_binding,
        r.pom_restart_required,
        r.dependencies_pom_restart_required,
        r.source_unchanged,
        r.pom_expected,
        r.repository_inputs_unchanged,
        r.client_reaped,
        r.synthetic_root_removed,
    ]
    .into_iter()
    .all(|v| v)
        && r.event_probe_outcome == EventProbeOutcome::EventsAccepted
        && r.event_error_code == ClientErrorCode::None
        && r.event_rejection == EventRejection::None
        && r.rejected_diagnostic_origin == DiagnosticOrigin::None
        && r.rejected_diagnostic_code_shape == DiagnosticCodeShape::None
        && r.rejected_diagnostic_severity == DiagnosticSeverity::None
        && r.rejected_diagnostic_message_class == DiagnosticMessageClass::None
        && !r.rejected_diagnostic_source_java
        && !r.rejected_diagnostic_zero_range
        && !r.rejected_diagnostic_dependency_jar_absent
        && !r.rejected_diagnostic_dependency_pom_absent
        && !r.primary_failed
        && !r.cleanup_failed
        && !r.budget_refused
        && !r.elapsed_saturated
        && !r.startup_cleanup_verified
        && r.elapsed_ms < 480_000
        && (1..=240).contains(&r.model_queries)
        && r.foreign_repository_files == 0
        && r.generated_metadata_files <= 128
        && r.lifecycle_metadata_files <= 6
        && r.lifecycle_metadata_mask <= 63
        && u32::from(r.lifecycle_metadata_files) == r.lifecycle_metadata_mask.count_ones()
        && r.lifecycle_metadata_files as u16 <= r.generated_metadata_files
        && (1..=4096).contains(&r.generated_data_files)
        && (1..=128 * 1024 * 1024).contains(&r.generated_data_bytes)
        && r.generated_project_files <= 256
        && r.generated_project_bytes <= 16 * 1024 * 1024
        && r.stop
            .as_ref()
            .is_some_and(|stop| stop.cleanup_joined && stop.status != StopStatus::Error)
        && [
            r.jar_present_before,
            r.jar_present_after,
            r.pom_present_before,
            r.pom_present_after,
            r.hover,
            r.completion,
            r.deliberate_type_diagnostic,
            r.dirty_change_acknowledged,
            r.correction_diagnostics,
            r.no_autosave,
        ]
        .into_iter()
        .all(|v| v == r.case.present())
        && r.offline_pom_diagnostic != r.case.present()
        && r.project_missing_library_diagnostic != r.case.present()
        && if r.case.present() {
            r.model_status == ModelStatus::Imported
                && r.dependency_observation == DependencyObservation::Present
        } else {
            r.model_status == ModelStatus::Unresolved
                && matches!(
                    r.dependency_observation,
                    DependencyObservation::Absent | DependencyObservation::NotObserved
                )
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
    let started = Instant::now();
    // This test-only emergency process boundary never certifies cleanup.
    // Its 480 seconds cannot be borrowed from the following case.
    let _watchdog = Watchdog::new(CASE);
    let deadline = (started + CASE).min(pair_deadline);
    let budget = Budget {
        deadline: (started + PRIMARY).min(deadline),
        refused: Rc::new(Cell::new(false)),
    };
    let mut record = CaseEvidence::new(kind);
    record.failure_stage = Stage::Setup;
    record.client_reaped = true;
    let mut fixture: Option<tempfile::TempDir> = None;
    let mut paths = None;
    let mut inventory = Inventory::new();
    let mut client = None;
    let mut pom_changed = false;
    let primary = checked(|| {
        require(
            budget_admits(started, pair_deadline, CASE, Duration::ZERO),
            "pair lacks full case envelope",
        )?;
        fixture = Some(io(tempfile::Builder::new()
            .prefix("cedar-linux-maven-")
            .tempdir())?);
        let base = ordinary_path(fixture.as_ref().unwrap().path())?;
        let (prepared, before) = setup_case(&base, input, cache, kind)?;
        paths = Some(prepared);
        inventory = before;
        let paths = paths.as_ref().unwrap();
        record.jar_present_before = !absent(&paths.repository.join(DEPENDENCY_JAR))?;
        record.pom_present_before = !absent(&paths.repository.join(DEPENDENCY_POM))?;
        require(
            record.jar_present_before == kind.present()
                && record.pom_present_before == kind.present(),
            "dependency preflight presence differs",
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
        // Read actual Local metadata, then exercise the same normal Client gates.
        let local_info = budget.run(SHORT_REQUEST, Duration::ZERO, || {
            let mut local = Client::connect(cedar_client::ConnectionSpec::Local {
                root: paths.root.clone(),
                allow_run: true,
            })?;
            let Payload::Hello {
                agent: Some(info), ..
            } = local.handshake()
            else {
                return Err("Local metadata missing".into());
            };
            let info = info.clone();
            info.validate().map_err(|_| "Local metadata invalid")?;
            for operation in [
                start()?,
                Operation::LanguageMavenModel,
                dependencies_operation(1),
            ] {
                require(
                    local
                        .request(operation)
                        .is_err_and(|error| error.starts_with("unsupported_operation:")),
                    "Local Client permitted Maven",
                )?;
            }
            require(
                io(fs::read_dir(&paths.data))?.next().is_none(),
                "Local Maven gate created controls",
            )?;
            local.close_and_wait(REAP)?;
            Ok(info)
        })?;
        record.client_reaped = false;
        connect(&mut client, &budget, binary, &paths.root, false)?;
        let untrusted = client.as_mut().ok_or("untrusted Client missing")?;
        capabilities(&untrusted.inner)?;
        rejected(untrusted, start()?, "run_disabled")?;
        rejected(untrusted, Operation::LanguageMavenModel, "run_disabled")?;
        rejected(untrusted, dependencies_operation(1), "run_disabled")?;
        require(
            io(fs::read_dir(&paths.data))?.next().is_none(),
            "trust-off Maven gate created controls",
        )?;
        record.trust_off_rejected = true;
        reap(&mut client, &budget)?;
        record.trust_off_client_reaped = true;
        record.client_reaped = true;
        record.client_reaped = false;
        connect(&mut client, &budget, binary, &paths.root, true)?;
        let client = client.as_mut().ok_or("trusted Client missing")?;
        let info = capabilities(&client.inner)?;
        record.exact_capabilities = true;
        rejected(
            client,
            Operation::LanguageMavenModel,
            "language_not_running",
        )?;
        record.model_without_session_rejected = true;
        rejected(client, dependencies_operation(1), "language_not_running")?;
        record.dependencies_without_session_rejected = true;
        record.failure_stage = Stage::Startup;
        let id = startup(client, start()?, &mut record)?;
        verify_control(paths)?;
        record.ascii_control_home = true;
        record.failure_stage = Stage::Model;
        let mut diagnostics = Diagnostics::default();
        await_model(client, paths, &mut record, &mut diagnostics)?;
        record.failure_stage = Stage::Dependencies;
        dependency_query(client, paths, id, &info, &local_info, &mut record)?;
        if kind.present() {
            record.failure_stage = Stage::Semantics;
            semantics(client, paths, &mut record, &mut diagnostics)?;
        }
        record.failure_stage = Stage::PomChange;
        budget.run(Duration::ZERO, Duration::ZERO, || {
            io(fs::write(
                paths.root.join("pom.xml"),
                format!("{FIXTURE_POM}\n"),
            ))
        })?;
        pom_changed = true;
        rejected(
            client,
            Operation::LanguageMavenModel,
            "language_maven_restart_required",
        )?;
        record.pom_restart_required = true;
        rejected(
            client,
            dependencies_operation(id),
            "language_maven_restart_required",
        )?;
        record.dependencies_pom_restart_required = true;
        Ok(())
    });
    record.primary_failed = primary.is_err();
    let primary_stage = record.failure_stage;
    let cleanup_started = Instant::now();
    let cleanup = Budget {
        deadline: deadline.min(cleanup_started + CLEANUP),
        refused: budget.refused.clone(),
    };
    let mut cleanup_failure_stage = Stage::None;
    if !budget_admits(cleanup_started, deadline, CLEANUP, Duration::ZERO) {
        budget.refused.set(true);
        record.cleanup_failed = true;
        cleanup_failure_stage = Stage::Stop;
    }
    record.failure_stage = Stage::Stop;
    let mut owns_closed = client
        .as_ref()
        .is_none_or(|client| !client.startup_attempted);
    if let Some(client) = client.as_mut() {
        let stopped = checked(|| {
            if client.active {
                cleanup.run(REQUEST, REAP, || {
                    let Payload::Language { value } =
                        client.inner.request(Operation::LanguageStop)?
                    else {
                        return Err("Stop did not return language evidence".into());
                    };
                    let stop = parse_stop(value)?;
                    owns_closed = true;
                    let acceptable = stop.status != StopStatus::Error;
                    record.stop = Some(stop);
                    require(acceptable, "Linux Stop reported cleanup quality error")
                })
            } else if let Some(id) = client.pending {
                cancel(client, id, &cleanup)?;
                owns_closed = true;
                record.startup_cleanup_verified = true;
                Ok(())
            } else {
                require(
                    !client.startup_attempted,
                    "startup identity missing; cleanup cannot be certified",
                )
            }
        });
        record.cleanup_failed |= stopped.is_err();
        if stopped.is_err() && matches!(cleanup_failure_stage, Stage::None) {
            cleanup_failure_stage = Stage::Stop;
        }
    }
    if client.is_some() {
        record.failure_stage = Stage::ClientReap;
        record.client_reaped = checked(|| reap(&mut client, &cleanup)).is_ok();
        record.cleanup_failed |= !record.client_reaped;
        if !record.client_reaped && matches!(cleanup_failure_stage, Stage::None) {
            cleanup_failure_stage = Stage::ClientReap;
        }
    }
    // Dropping a Client whose admitted reaping failed is emergency cleanup,
    // never a positive receipt. Leave the fixture if ownership is unverified.
    drop(client);
    record.failure_stage = Stage::FixtureCleanup;
    let verified = checked(|| {
        cleanup.run(Duration::ZERO, Duration::ZERO, || {
            if let Some(paths) = paths.as_ref() {
                record.source_unchanged = source_unchanged(paths);
                let expected = if pom_changed {
                    format!("{FIXTURE_POM}\n")
                } else {
                    FIXTURE_POM.into()
                };
                record.pom_expected = fs::read(paths.root.join("pom.xml"))
                    .is_ok_and(|bytes| bytes == expected.as_bytes());
                check_repository(paths, &inventory, &mut record)?;
                let (files, bytes) = generated_metadata(&paths.data, false)?;
                record.generated_data_files = files;
                record.generated_data_bytes = bytes;
                let (files, bytes) = generated_metadata(&paths.root, true)?;
                record.generated_project_files = files as u16;
                record.generated_project_bytes = bytes;
                require(
                    record.source_unchanged && record.pom_expected,
                    "fixture inputs changed",
                )?;
            }
            Ok(())
        })
    });
    record.cleanup_failed |= verified.is_err();
    if verified.is_err() && matches!(cleanup_failure_stage, Stage::None) {
        cleanup_failure_stage = Stage::FixtureCleanup;
    }
    if let Some(fixture) = fixture {
        if record.client_reaped && owns_closed {
            record.synthetic_root_removed =
                checked(|| cleanup.run(Duration::ZERO, Duration::ZERO, || io(fixture.close())))
                    .is_ok();
            record.cleanup_failed |= !record.synthetic_root_removed;
            if !record.synthetic_root_removed && matches!(cleanup_failure_stage, Stage::None) {
                cleanup_failure_stage = Stage::FixtureCleanup;
            }
        } else {
            let _ = fixture.keep();
        }
    }
    let elapsed = started.elapsed().as_millis();
    record.elapsed_ms = elapsed.min(480_000) as u32;
    record.elapsed_saturated = elapsed > 480_000;
    record.budget_refused = budget.refused.get();
    record.cleanup_failed |= Instant::now() >= cleanup.deadline || Instant::now() >= deadline;
    record.success = case_passed(&record);
    record.failure_stage = if record.success {
        Stage::None
    } else if record.primary_failed {
        primary_stage
    } else if !matches!(cleanup_failure_stage, Stage::None) {
        cleanup_failure_stage
    } else {
        Stage::FixtureCleanup
    };
    record
}

#[test]
#[ignore = "requires exact native Linux normal CEDAR_AGENT_BIN, JDK21 CEDAR_JAVA, pinned Unicode CEDAR_JDTLS_HOME, and frozen CEDAR_MAVEN_CACHE_INPUT; run serially"]
fn real_linux_normal_agent_java_maven_acceptance() -> CheckResult<()> {
    println!();
    let started = Instant::now();
    let _watchdog = Watchdog::new(PAIR);
    let deadline = started + PAIR;
    let mut record = Evidence {
        schema_version: 1,
        kind: "linux_java_maven",
        route: "normal_agent_normal_client",
        source_sha256: SOURCE_SHA256,
        pom_sha256: POM_SHA256,
        dependency_jar_sha256: JAR_SHA256,
        cases: [
            CaseEvidence::new(CaseKind::Present),
            CaseEvidence::new(CaseKind::Missing),
        ],
        pair_deadline_ms: 960_000,
        cache_input_files: 0,
        cache_input_unchanged: false,
        fixture_inputs_verified: false,
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
                "Java or Maven launcher environment injection",
            )?;
        }
        let binary = environment_path("CEDAR_AGENT_BIN")?;
        let java = environment_path("CEDAR_JAVA")?;
        let distribution = environment_path("CEDAR_JDTLS_HOME")?;
        require(
            binary.is_file()
                && binary.file_name().is_some_and(|name| name == "cedar-agent")
                && java.is_file()
                && java.file_name().is_some_and(|name| name == "java")
                && text(&java)?.is_ascii()
                && distribution.is_dir()
                && !text(&distribution)?.is_ascii()
                && distribution.join("config_linux").is_dir(),
            "exact normal Linux inputs missing",
        )?;
        let (input, cache) = verified_cache()?;
        record.cache_input_files = cache.len() as u16;
        sealed_jar()?;
        require(
            hash(FIXTURE_SOURCE.as_bytes()) == SOURCE_SHA256
                && hash(FIXTURE_POM.as_bytes()) == POM_SHA256,
            "sealed Maven source or POM differs from Windows fixture",
        )?;
        record.fixture_inputs_verified = true;
        record.cases[0] = run_case(
            CaseKind::Present,
            &binary,
            &java,
            &distribution,
            &input,
            &cache,
            deadline,
        );
        if cleanup_owned(&record.cases[0])
            && !record.cases[0].cleanup_failed
            && budget_admits(Instant::now(), deadline, CASE, Duration::ZERO)
        {
            record.cases[1] = run_case(
                CaseKind::Missing,
                &binary,
                &java,
                &distribution,
                &input,
                &cache,
                deadline,
            );
        } else {
            record.cases[1].failure_stage = Stage::Setup;
            record.cases[1].primary_failed = true;
            record.cases[1].budget_refused =
                !budget_admits(Instant::now(), deadline, CASE, Duration::ZERO);
        }
        record.cache_input_unchanged = snapshot(&input).is_ok_and(|after| after == cache);
        Ok(())
    });
    record.primary_failed = result.is_err() || record.cases.iter().any(|case| case.primary_failed);
    record.cleanup_failed =
        record.cases.iter().any(|case| case.cleanup_failed) || !record.cache_input_unchanged;
    let elapsed = started.elapsed().as_millis();
    record.elapsed_ms = elapsed.min(960_000) as u32;
    record.elapsed_saturated = elapsed > 960_000;
    record.success = !record.primary_failed
        && !record.cleanup_failed
        && !record.elapsed_saturated
        && record.fixture_inputs_verified
        && record.cache_input_files == 83
        && record.cases.iter().all(|case| case.success)
        && Instant::now() < deadline;
    println!(
        "\n{}",
        serde_json::to_string(&record).expect("typed Linux Maven receipt")
    );
    require(
        record.success,
        "Linux normal-agent Maven acceptance failed; private details remain private",
    )
}

#[test]
fn linux_maven_deadlines_admit_full_rpc_and_cleanup_envelopes() {
    let now = Instant::now();
    assert_eq!(PRIMARY + CLEANUP, CASE);
    assert_eq!(CASE * 2, PAIR);
    assert_eq!(REQUEST + REAP, Duration::from_secs(105));
    for cost in [SHORT_REQUEST, REQUEST, REAP] {
        assert!(budget_admits(now, now + cost, cost, Duration::ZERO));
        assert!(!budget_admits(
            now,
            now + cost - Duration::from_nanos(1),
            cost,
            Duration::ZERO
        ));
    }
    assert!(budget_admits(now, now + CLEANUP, REQUEST, REAP));
    assert!(!budget_admits(
        now,
        now + Duration::from_secs(104),
        REQUEST,
        REAP
    ));
    assert!(!budget_admits(now, now, Duration::ZERO, Duration::ZERO));
    let budget = Budget {
        deadline: now + SHORT_REQUEST - Duration::from_secs(1),
        refused: Rc::new(Cell::new(false)),
    };
    assert!(budget
        .run(SHORT_REQUEST, Duration::ZERO, || -> CheckResult<()> {
            panic!("must never dispatch underfunded RPC")
        })
        .is_err());
    assert!(budget.refused.get());
    let mut record = CaseEvidence::new(CaseKind::Missing);
    record.budget_refused = true;
    assert!(!case_passed(&record));
}

#[test]
fn linux_maven_stop_rejects_windows_mixed_untagged_and_unjoined_receipts() {
    let graceful = json!({"stopped":true,"shutdown":{"platform":"linux","status":"graceful","reason":"root_exited",
        "root_exit":{"kind":"code","code":0},"cleanup_joined":true,"shutdown_response_received":true,"exit_frame_completed":true}});
    assert!(parse_stop(graceful.clone()).is_ok());
    let mut forced = graceful.clone();
    forced["shutdown"]["status"] = json!("forced");
    forced["shutdown"]["reason"] = json!("grace_expired");
    forced["shutdown"]["root_exit"] = json!({"kind":"signal","signal":9});
    assert!(parse_stop(forced.clone()).is_ok());
    for (field, value) in [
        ("platform", json!("windows")),
        ("cleanup_joined", json!(false)),
        ("root_exit", json!({"code":0})),
        ("root_exit", json!({"kind":"code","code":256})),
        ("root_exit", json!({"kind":"signal","signal":0})),
        ("root_exit", json!({"kind":"signal","signal":65})),
        ("root_exit", json!({"kind":"code","code":0,"signal":9})),
        ("root_exit_code", json!(0)),
    ] {
        let mut wrong = forced.clone();
        wrong["shutdown"][field] = value;
        assert!(parse_stop(wrong).is_err(), "{field}");
    }
    for field in ["shutdown_response_received", "exit_frame_completed"] {
        let mut wrong = graceful.clone();
        wrong["shutdown"][field] = json!(false);
        assert!(parse_stop(wrong).is_err());
    }
}

fn predicate_paths(base: &Path) -> CasePaths {
    CasePaths {
        root: base.join("workspace 雪"),
        repository: base.join("repository 雪"),
        data: base.join("data"),
    }
}
fn model_fixture(paths: &CasePaths, present: bool) -> Value {
    json!({"profile":"maven_leaf","status":if present { "imported" } else { "unresolved" },
        "pom_path":"pom.xml","pom_sha256":POM_SHA256,"restart_required":false,"maven_nature":true,
        "compiler":{"source":"17","compliance":"17","target":"17"},"source_paths":["source-java"],
        "unresolved_count":if present { 0 } else { 1 }, "classpath":[
            {"kind":"source","path":"source-java","resolved":true,"origin":"model"},
            {"kind":"library","path":paths.repository.join(DEPENDENCY_JAR).to_str(),"resolved":present,"origin":"model"}]})
}
#[test]
fn linux_maven_model_requires_exact_linux_path_kind_compiler_and_resolution() {
    let paths = predicate_paths(Path::new("/owned"));
    for kind in [CaseKind::Present, CaseKind::Missing] {
        let expected = model_fixture(&paths, kind.present());
        let mut record = CaseEvidence::new(kind);
        assert!(inspect_model(&expected, &paths, &mut record).unwrap());
        for (field, value) in [
            ("kind", json!("container")),
            ("path", json!("/owned/Repository 雪/foreign.jar")),
            ("resolved", json!(!kind.present())),
            ("origin", json!("foreign")),
        ] {
            let mut wrong = expected.clone();
            wrong["classpath"][1][field] = value;
            assert!(inspect_model(&wrong, &paths, &mut record).is_err());
        }
        let mut duplicate = expected.clone();
        duplicate["classpath"]
            .as_array_mut()
            .unwrap()
            .push(expected["classpath"][1].clone());
        assert!(inspect_model(&duplicate, &paths, &mut record).is_err());
        let mut wrong = expected.clone();
        wrong["compiler"]["source"] = json!("21");
        assert!(!inspect_model(&wrong, &paths, &mut record).unwrap());
    }
}
fn diagnostic_batch(uri: &str, item: Value) -> Value {
    json!({"truncated":false,"events":[{"type":"diagnostics","value":{"uri":uri,"diagnostics":[item]}}]})
}
#[test]
fn linux_maven_missing_markers_require_owned_pom_full_gav_and_absent_artifacts() -> CheckResult<()>
{
    let temp = io(tempfile::tempdir())?;
    let paths = predicate_paths(temp.path());
    io(fs::create_dir(&paths.root))?;
    io(fs::create_dir_all(
        paths.repository.join(DEPENDENCY_DIRECTORY),
    ))?;
    let pom_uri = url::Url::from_file_path(paths.root.join("pom.xml")).unwrap();
    let project_uri = url::Url::from_directory_path(&paths.root).unwrap();
    let pom = diagnostic_batch(
        pom_uri.as_str(),
        json!({"severity":1,"code":"0","message":format!("Offline / Missing artifact {DEPENDENCY_GAV}")}),
    );
    let project = diagnostic_batch(
        project_uri.as_str(),
        json!({"severity":1,"code":"964","source":"Java",
        "range":{"start":{"line":0,"character":0},"end":{"line":0,"character":0}},
        "message":format!("The container 'Maven Dependencies' references non existing library '{}'",text(&paths.repository.join(DEPENDENCY_JAR))?)}),
    );
    let mut state = Diagnostics::default();
    inspect_events(&project, &paths, CaseKind::Missing, &mut state, false)?;
    assert!(state.missing_project && !state.offline_pom);
    inspect_events(&pom, &paths, CaseKind::Missing, &mut state, false)?;
    assert!(state.offline_pom);
    for batch in [&pom, &project] {
        assert!(inspect_events(
            batch,
            &paths,
            CaseKind::Present,
            &mut Diagnostics::default(),
            false
        )
        .is_err());
        let mut wrong = batch.clone();
        wrong["events"][0]["value"]["uri"] = json!("file:///foreign/pom.xml");
        assert!(inspect_events(
            &wrong,
            &paths,
            CaseKind::Missing,
            &mut Diagnostics::default(),
            false
        )
        .is_err());
        let mut wrong = batch.clone();
        wrong["events"][0]["value"]["diagnostics"][0]["message"] =
            json!("Missing artifact arithmetic");
        assert!(inspect_events(
            &wrong,
            &paths,
            CaseKind::Missing,
            &mut Diagnostics::default(),
            false
        )
        .is_err());
        let mut wrong = batch.clone();
        wrong["events"]
            .as_array_mut()
            .unwrap()
            .push(json!({"type":"lagged"}));
        assert!(inspect_events(
            &wrong,
            &paths,
            CaseKind::Missing,
            &mut Diagnostics::default(),
            false
        )
        .is_err());
    }
    io(fs::write(
        paths.repository.join(DEPENDENCY_POM),
        ARTIFACT_POM,
    ))?;
    assert!(inspect_events(
        &project,
        &paths,
        CaseKind::Missing,
        &mut Diagnostics::default(),
        false
    )
    .is_err());
    Ok(())
}

#[test]
fn linux_maven_fixture_and_missing_dependency_observation_remain_exact() {
    assert_eq!(sealed_jar().unwrap().len(), 422);
    assert_eq!(hash(FIXTURE_SOURCE.as_bytes()), SOURCE_SHA256);
    assert_eq!(hash(FIXTURE_POM.as_bytes()), POM_SHA256);
    for (present, observed) in [(true, true), (false, true), (false, false)] {
        let mut value = json!({"schema":1,"profile":"maven_leaf","startup_id":7,"pom_path":"pom.xml","pom_sha256":POM_SHA256,
            "declarations":[{"group_id":"dev.cedar.fixture","artifact_id":"arithmetic","version":"1.0.0",
                "classifier":null,"scope":"compile","scope_explicit":false,"optional":false,"optional_explicit":false,
                "expected_jar_path":DEPENDENCY_JAR,"regular_file_present":present}],
            "observation":{"status":"available","libraries":[]}});
        if observed {
            value["observation"]["libraries"] = json!([{"root":"local_repository","relative_path":DEPENDENCY_JAR,"regular_file_present":present,"declaration_indices":[0]}]);
        }
        let snapshot: MavenDependenciesSnapshot = serde_json::from_value(value).unwrap();
        let kind = if present {
            CaseKind::Present
        } else {
            CaseKind::Missing
        };
        let mut record = CaseEvidence::new(kind);
        inspect_dependencies(&snapshot, 7, &mut record).unwrap();
        assert!(record.dependency_insight);
        assert!(
            record.dependency_observation
                == if present {
                    DependencyObservation::Present
                } else if observed {
                    DependencyObservation::Absent
                } else {
                    DependencyObservation::NotObserved
                }
        );
        assert!(inspect_dependencies(&snapshot, 8, &mut record).is_err());
        let mut wrong = snapshot.clone();
        wrong.declarations[0].scope_explicit = true;
        assert!(inspect_dependencies(&wrong, 7, &mut record).is_err());
    }
}

fn probe_events(value: Value, paths: &CasePaths) -> CaseEvidence {
    let mut record = CaseEvidence::new(CaseKind::Missing);
    let result = record_event_response(
        Ok(Payload::Language { value }),
        paths,
        &mut record,
        &mut Diagnostics::default(),
        false,
    );
    assert!(result.is_err());
    assert_eq!(
        record.event_probe_outcome,
        EventProbeOutcome::EventsRejected
    );
    assert_eq!(record.event_error_code, ClientErrorCode::None);
    assert!(!case_passed(&record));
    let serialized = serde_json::to_string(&record).unwrap();
    assert!(
        !serialized.contains("private-sentinel")
            && !serialized.contains("/owned")
            && !serialized.contains("file:")
    );
    record
}

#[test]
fn linux_maven_event_probe_separates_request_payload_and_structure_without_raw_errors() {
    let paths = predicate_paths(Path::new("/owned/private-sentinel"));
    let mut record = CaseEvidence::new(CaseKind::Missing);
    let mut state = Diagnostics::default();
    for (error, code) in [
        (
            "transport_timeout: private-sentinel",
            ClientErrorCode::TransportFailure,
        ),
        (
            "language_not_running: private-sentinel",
            ClientErrorCode::LanguageNotRunning,
        ),
        (
            "private-sentinel transport_timeout: unrelated",
            ClientErrorCode::Other,
        ),
    ] {
        assert!(
            record_event_response(Err(error.into()), &paths, &mut record, &mut state, false)
                .is_err()
        );
        assert_eq!(record.event_probe_outcome, EventProbeOutcome::RequestFailed);
        assert_eq!(record.event_error_code, code);
        assert_eq!(record.event_rejection, EventRejection::None);
        assert_eq!(record.rejected_diagnostic_origin, DiagnosticOrigin::None);
        assert!(!serde_json::to_string(&record)
            .unwrap()
            .contains("private-sentinel"));
    }
    assert!(record_event_response(
        Ok(Payload::Written {
            revision: "private-sentinel".into()
        }),
        &paths,
        &mut record,
        &mut state,
        false
    )
    .is_err());
    assert_eq!(
        record.event_probe_outcome,
        EventProbeOutcome::NonLanguagePayload
    );
    assert_eq!(record.event_error_code, ClientErrorCode::None);
    for (value, expected) in [
        (Value::Null, EventRejection::Truncated),
        (
            json!({"truncated":true,"events":[]}),
            EventRejection::Truncated,
        ),
        (json!({"truncated":false}), EventRejection::MissingEvents),
        (
            json!({"truncated":false,"events":[{"type":"closed"}]}),
            EventRejection::ClosedEvent,
        ),
        (
            json!({"truncated":false,"events":[{"type":"lagged"}]}),
            EventRejection::LaggedEvent,
        ),
        (
            json!({"truncated":false,"events":[{}]}),
            EventRejection::MissingEventType,
        ),
        (
            json!({"truncated":false,"events":[{"type":"private-sentinel"}]}),
            EventRejection::UnknownEvent,
        ),
        (
            json!({"truncated":false,"events":[{"type":"diagnostics","value":{}}]}),
            EventRejection::MissingDiagnosticUri,
        ),
        (
            json!({"truncated":false,"events":[{"type":"diagnostics","value":{"uri":"file:///private-sentinel"}}]}),
            EventRejection::MissingDiagnostics,
        ),
    ] {
        assert_eq!(probe_events(value, &paths).event_rejection, expected);
    }
}

fn project_marker(paths: &CasePaths) -> Value {
    let project_uri = url::Url::from_directory_path(&paths.root).unwrap();
    diagnostic_batch(
        project_uri.as_str(),
        json!({"severity":1,"code":"964","source":"Java",
        "range":{"start":{"line":0,"character":0},"end":{"line":0,"character":0}},
        "message":format!("The container 'Maven Dependencies' references non existing library '{}'", paths.repository.join(DEPENDENCY_JAR).to_str().unwrap())}),
    )
}

#[test]
fn linux_maven_event_probe_classifies_uri_path_code_source_severity_and_range_without_relaxing_them(
) {
    let paths = predicate_paths(Path::new("/owned/private-sentinel"));
    let expected = project_marker(&paths);
    for (field, value) in [("code",json!(964)), ("code",json!("private-sentinel")),
        ("source",json!("private-sentinel")), ("severity",json!(2)),
        ("range",json!({"start":{"line":0,"character":1},"end":{"line":0,"character":1}})),
        ("message",json!("The container 'Maven Dependencies' references non existing library '/foreign/private-sentinel.jar'"))] {
        let mut wrong = expected.clone(); wrong["events"][0]["value"]["diagnostics"][0][field] = value;
        let record = probe_events(wrong, &paths);
        assert_eq!(record.event_rejection, EventRejection::UnexpectedProjectDiagnostic);
        assert_eq!(record.rejected_diagnostic_origin, DiagnosticOrigin::OwnedProjectRoot);
        assert!(record.rejected_diagnostic_dependency_jar_absent && record.rejected_diagnostic_dependency_pom_absent);
        assert_eq!(record.rejected_diagnostic_source_java, field != "source");
        assert_eq!(record.rejected_diagnostic_zero_range, field != "range");
        if field == "message" { assert_eq!(record.rejected_diagnostic_message_class, DiagnosticMessageClass::Other); }
        else { assert_eq!(record.rejected_diagnostic_message_class, DiagnosticMessageClass::OwnedMissingMavenLibrary); }
        if field == "severity" { assert_eq!(record.rejected_diagnostic_severity, DiagnosticSeverity::Warning); }
        else { assert_eq!(record.rejected_diagnostic_severity, DiagnosticSeverity::Error); }
    }
    let root_uri = url::Url::from_directory_path(&paths.root).unwrap();
    for (uri, expected_origin) in [
        (
            "file:///foreign/private-sentinel",
            DiagnosticOrigin::Foreign,
        ),
        (
            root_uri.as_str().trim_end_matches('/'),
            DiagnosticOrigin::OwnedProjectWithoutTrailingSlash,
        ),
    ] {
        let mut wrong = expected.clone();
        wrong["events"][0]["value"]["uri"] = json!(uri);
        if expected_origin == DiagnosticOrigin::OwnedProjectWithoutTrailingSlash {
            // The owned spelling is now permitted for the missing fixture,
            // but a numeric code still fails the unchanged marker predicate.
            wrong["events"][0]["value"]["diagnostics"][0]["code"] = json!(964);
        }
        let record = probe_events(wrong, &paths);
        assert_eq!(
            record.event_rejection,
            if expected_origin == DiagnosticOrigin::OwnedProjectWithoutTrailingSlash {
                EventRejection::UnexpectedProjectDiagnostic
            } else {
                EventRejection::ForeignDocument
            }
        );
        assert_eq!(record.rejected_diagnostic_origin, expected_origin);
        assert_eq!(
            record.rejected_diagnostic_message_class,
            DiagnosticMessageClass::OwnedMissingMavenLibrary
        );
    }
    let pom_uri = url::Url::from_file_path(paths.root.join("pom.xml")).unwrap();
    let record = probe_events(
        diagnostic_batch(
            pom_uri.as_str(),
            json!({"severity":1,"code":"0", "message":format!("Missing artifact {DEPENDENCY_GAV}")}),
        ),
        &paths,
    );
    assert_eq!(
        record.event_rejection,
        EventRejection::UnexpectedPomDiagnostic
    );
    assert_eq!(
        record.rejected_diagnostic_message_class,
        DiagnosticMessageClass::PlainMissingOwnedDependency
    );
}

#[test]
fn linux_maven_event_probe_preserves_accepted_markers_before_later_failure_and_clears_accepted_trace(
) {
    let paths = predicate_paths(Path::new("/owned/private-sentinel"));
    let pom_uri = url::Url::from_file_path(paths.root.join("pom.xml")).unwrap();
    let mut accepted = diagnostic_batch(
        pom_uri.as_str(),
        json!({"severity":1,"code":"0", "message":format!("Offline / Missing artifact {DEPENDENCY_GAV}")}),
    );
    accepted["events"]
        .as_array_mut()
        .unwrap()
        .push(project_marker(&paths)["events"][0].clone());
    let mut failed = accepted.clone();
    failed["events"]
        .as_array_mut()
        .unwrap()
        .push(json!({"type":"closed","message":"private-sentinel"}));
    let record = probe_events(failed, &paths);
    assert!(record.offline_pom_diagnostic && record.project_missing_library_diagnostic);
    assert_eq!(record.event_rejection, EventRejection::ClosedEvent);
    assert_eq!(record.rejected_diagnostic_origin, DiagnosticOrigin::None);
    let mut record = CaseEvidence::new(CaseKind::Missing);
    record_event_response(
        Ok(Payload::Language { value: accepted }),
        &paths,
        &mut record,
        &mut Diagnostics::default(),
        false,
    )
    .unwrap();
    assert_eq!(
        record.event_probe_outcome,
        EventProbeOutcome::EventsAccepted
    );
    assert!(record.offline_pom_diagnostic && record.project_missing_library_diagnostic);
    assert_eq!(record.event_rejection, EventRejection::None);
    assert_eq!(
        record.rejected_diagnostic_message_class,
        DiagnosticMessageClass::None
    );
    assert!(
        !record.rejected_diagnostic_source_java
            && !record.rejected_diagnostic_zero_range
            && !record.rejected_diagnostic_dependency_jar_absent
            && !record.rejected_diagnostic_dependency_pom_absent
    );
}

#[test]
fn linux_maven_missing_project_uri_accepts_only_the_owned_terminal_slash_variant() -> CheckResult<()>
{
    let temp = io(tempfile::tempdir())?;
    let paths = predicate_paths(temp.path());
    io(fs::create_dir(&paths.root))?;
    io(fs::create_dir_all(
        paths.repository.join(DEPENDENCY_DIRECTORY),
    ))?;
    let expected = url::Url::from_directory_path(&paths.root)
        .unwrap()
        .to_string();
    let without = expected.strip_suffix('/').unwrap();
    let raw = format!("file:{}", text(&paths.root)?);
    let accepted = [
        expected.clone(),
        without.into(),
        format!("{raw}/"),
        raw,
        expected.replace("%E9%9B%AA", "%e9%9b%aa"),
        without.replace("%E9%9B%AA", "%e9%9b%aa"),
    ];
    // The shared comparator still distinguishes the two directory spellings.
    assert!(!same_local_uri(without, &expected));
    for uri in &accepted {
        let mut batch = project_marker(&paths);
        batch["events"][0]["value"]["uri"] = json!(uri);
        let mut state = Diagnostics::default();
        inspect_events(&batch, &paths, CaseKind::Missing, &mut state, false)?;
        assert!(state.missing_project && !state.offline_pom);
        assert!(inspect_events(
            &batch,
            &paths,
            CaseKind::Present,
            &mut Diagnostics::default(),
            false
        )
        .is_err());
        for (field, value) in [("code",json!(964)), ("code",json!(true)), ("source",Value::Null),
            ("severity",json!("1")), ("severity",json!(2)), ("range",json!({"start":{"line":"0","character":0},"end":{"line":0,"character":0}})),
            ("message",json!("The container 'Maven Dependencies' references non existing library '/foreign/private-sentinel.jar'"))] {
            let mut wrong = batch.clone(); wrong["events"][0]["value"]["diagnostics"][0][field] = value;
            assert!(inspect_events(&wrong, &paths, CaseKind::Missing, &mut Diagnostics::default(), false).is_err());
        }
        let mut wrong = batch.clone();
        wrong["events"][0]["value"]["diagnostics"][0]
            .as_object_mut()
            .unwrap()
            .remove("code");
        assert!(inspect_events(
            &wrong,
            &paths,
            CaseKind::Missing,
            &mut Diagnostics::default(),
            false
        )
        .is_err());
    }
    let uri_path = expected.strip_prefix("file://").unwrap();
    let ancestor = url::Url::from_directory_path(temp.path())
        .unwrap()
        .to_string();
    for uri in [
        expected.replace('%', "%25"),
        format!("file://localhost{uri_path}"),
        format!("file://foreign{uri_path}"),
        format!("{expected}?private-sentinel"),
        format!("{without}#private-sentinel"),
        format!("{without}-sibling/"),
        ancestor,
        format!("{expected}/"),
        format!("{without}//"),
        format!("{without}%2F%2F"),
        format!("{without}/../workspace%20%E9%9B%AA/"),
        format!("file:///{uri_path}"),
        format!("{without}%00"),
        format!("{without}%zz"),
    ] {
        let mut wrong = project_marker(&paths);
        wrong["events"][0]["value"]["uri"] = json!(uri);
        assert!(!owned_project_marker_uri(
            &uri,
            &expected,
            CaseKind::Missing
        ));
        assert!(inspect_events(
            &wrong,
            &paths,
            CaseKind::Missing,
            &mut Diagnostics::default(),
            false
        )
        .is_err());
    }
    for file in [DEPENDENCY_JAR, DEPENDENCY_POM] {
        io(fs::write(
            paths.repository.join(file),
            b"owned presence witness",
        ))?;
        for uri in &accepted {
            let mut wrong = project_marker(&paths);
            wrong["events"][0]["value"]["uri"] = json!(uri);
            assert!(inspect_events(
                &wrong,
                &paths,
                CaseKind::Missing,
                &mut Diagnostics::default(),
                false
            )
            .is_err());
        }
        io(fs::remove_file(paths.repository.join(file)))?;
    }
    Ok(())
}

fn complete_case_fixture(kind: CaseKind) -> CaseEvidence {
    let mut record = CaseEvidence::new(kind);
    macro_rules! mark { ($($field:ident),* $(,)?) => { $(record.$field = true;)* }; }
    mark!(
        exact_capabilities,
        trust_off_rejected,
        trust_off_client_reaped,
        model_without_session_rejected,
        dependencies_without_session_rejected,
        local_frontend_gates,
        async_begin,
        read_while_starting,
        ready,
        ascii_control_home,
        maven_nature,
        custom_source,
        compiler_17,
        exact_dependency_reference,
        dependency_insight,
        frontend_binding,
        pom_restart_required,
        dependencies_pom_restart_required,
        source_unchanged,
        pom_expected,
        repository_inputs_unchanged,
        client_reaped,
        synthetic_root_removed
    );
    record.event_probe_outcome = EventProbeOutcome::EventsAccepted;
    record.model_status = if kind.present() {
        ModelStatus::Imported
    } else {
        ModelStatus::Unresolved
    };
    record.dependency_observation = if kind.present() {
        DependencyObservation::Present
    } else {
        DependencyObservation::Absent
    };
    record.offline_pom_diagnostic = !kind.present();
    record.project_missing_library_diagnostic = !kind.present();
    record.model_queries = 1;
    record.generated_data_files = 1;
    record.generated_data_bytes = 1;
    if kind.present() {
        mark!(
            jar_present_before,
            jar_present_after,
            pom_present_before,
            pom_present_after,
            hover,
            completion,
            deliberate_type_diagnostic,
            dirty_change_acknowledged,
            correction_diagnostics,
            no_autosave
        );
    }
    record.stop = Some(
        parse_stop(
            json!({"stopped":true,"shutdown":{"platform":"linux","status":"graceful",
        "reason":"root_exited","root_exit":{"kind":"code","code":0},"cleanup_joined":true,
        "shutdown_response_received":true,"exit_frame_completed":true}}),
        )
        .unwrap(),
    );
    record
}
#[test]
fn linux_maven_case_predicate_requires_both_missing_diagnostics_and_the_correct_case() {
    for kind in [CaseKind::Missing, CaseKind::Present] {
        let mut record = complete_case_fixture(kind);
        assert!(case_passed(&record));
        record.project_missing_library_diagnostic = kind.present();
        assert!(!case_passed(&record));
        record.project_missing_library_diagnostic = !kind.present();
        assert!(case_passed(&record));
        record.offline_pom_diagnostic = kind.present();
        assert!(!case_passed(&record));
    }
    let mut record = complete_case_fixture(CaseKind::Missing);
    record.model_status = ModelStatus::Imported;
    assert!(!case_passed(&record));
    record.model_status = ModelStatus::Unresolved;
    record.exact_dependency_reference = false;
    assert!(!case_passed(&record));
}
