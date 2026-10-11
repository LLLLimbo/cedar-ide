//! Opt-in, one-session real JDT proof through the unchanged shipping Client.
//! The driver selects/copies the test executable and its normal agent sibling.
//! This is a headless typed-route witness, never native editor/UI acceptance.
#![cfg(any(target_os = "linux", windows))]

use crate::{completion, java_language};
use cedar_client::{Client, ConnectionSpec};
use cedar_language::signature_help::{self, SignatureHelp};
use cedar_protocol::{AgentInfo, Operation, Payload};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    panic::{catch_unwind, resume_unwind, AssertUnwindSafe},
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

const ACTIVE: Duration = Duration::from_secs(570);
const CLEANUP: Duration = Duration::from_secs(180);
const WATCHDOG: Duration = Duration::from_secs(780);
const STARTUP: Duration = Duration::from_secs(75);
const SHORT: Duration = Duration::from_secs(30);
const REQUEST: Duration = Duration::from_secs(75);
const REAP: Duration = Duration::from_secs(30);
const POLL_DELAY: Duration = Duration::from_millis(50);
const MAX_POLLS: u16 = 1024;
const PROJECT: &str = "project with spaces 雪";
const SOURCE_PATH: &str = "project with spaces 雪/src/CedarParameterFixture.java";
const SOURCE: &str = "public class CedarParameterFixture {\n    static String cedarParameter(String first, int second) { return first; }\n    static String cedarParameter(String first, String second) { return second; }\n    void exercise() {\n        /* 雪😀 */ String selected = cedarParameter(\"alpha\", 7);\n        int outsideCall = 9;\n    }\n}\n";
const SOURCE_SHA256: &str = "2a196b0dc072953dcf36761bb91532ccbc6c64539b2d1cdcdb4857ab33045211";
const LABELS: [&str; 2] = [
    "cedarParameter(String first, int second) : String",
    "cedarParameter(String first, String second) : String",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Failure {
    Setup,
    Io,
    Admission,
    Deadline,
    Protocol,
    Connection,
    Capability,
    Provider,
    Startup,
    Semantics,
    RootObservation,
    Stop,
    Reap,
    Receipt,
}
type ProofResult<T> = Result<T, Failure>;

fn require(value: bool, failure: Failure) -> ProofResult<()> {
    if value {
        Ok(())
    } else {
        Err(failure)
    }
}
fn io<T>(result: std::io::Result<T>) -> ProofResult<T> {
    result.map_err(|_| Failure::Io)
}

// Conservative admission uses the full normal Client timeout, never a shorter
// proof-specific request timeout. The active clock is created once, not renewed.
fn admit(elapsed: Duration, limit: Duration, cost: Duration) -> ProofResult<()> {
    require(
        elapsed <= limit && cost <= limit.saturating_sub(elapsed),
        Failure::Admission,
    )
}
fn within(elapsed: Duration, limit: Duration) -> ProofResult<()> {
    require(elapsed <= limit, Failure::Deadline)
}
#[derive(Clone, Copy)]
struct Budget {
    started: Instant,
    limit: Duration,
}
impl Budget {
    fn new(limit: Duration) -> Self {
        Self {
            started: Instant::now(),
            limit,
        }
    }
    fn admit(self, cost: Duration) -> ProofResult<()> {
        admit(self.started.elapsed(), self.limit, cost)
    }
    fn check(self) -> ProofResult<()> {
        within(self.started.elapsed(), self.limit)
    }
}

// This last-resort process boundary is not proof of kernel/descendant cleanup.
struct Watchdog {
    sender: mpsc::Sender<()>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Watchdog {
    fn new() -> Self {
        let (sender, receiver) = mpsc::channel();
        let worker = thread::spawn(move || {
            if matches!(
                receiver.recv_timeout(WATCHDOG),
                Err(mpsc::RecvTimeoutError::Timeout)
            ) {
                std::process::abort();
            }
        });
        Self {
            sender,
            worker: Some(worker),
        }
    }
}
impl Drop for Watchdog {
    fn drop(&mut self) {
        let _ = self.sender.send(());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Stage {
    None,
    #[default]
    Setup,
    Connect,
    Capabilities,
    List,
    Read,
    Begin,
    Poll,
    Open,
    FirstArgument,
    SecondArgument,
    OutsideCall,
    Close,
    SourceCheck,
    Cleanup,
    Receipt,
}

#[derive(Clone, Default, Serialize)]
struct Evidence {
    schema_version: u8,
    kind: &'static str,
    platform: &'static str,
    route: &'static str,
    status: &'static str,
    source_fixture_sha256: &'static str,
    failure_stage: Stage,
    primary_failed: bool,
    cleanup_failed: bool,
    exact_capabilities: bool,
    provider_supported: bool,
    open_version_acknowledged: bool,
    first_argument_labels_exact: bool,
    first_argument_active_exact: bool,
    second_argument_labels_exact: bool,
    second_argument_active_exact: bool,
    outside_call_empty: bool,
    source_unchanged: bool,
    startup_terminal: bool,
    stop_verified: bool,
    client_reaped: bool,
    root_exit_observed: bool,
    fixture_removed: bool,
    fixture_retained: bool,
    all_owned_terminal: bool,
    active_deadline_met: bool,
    cleanup_deadline_met: bool,
    elapsed_saturated: bool,
    capability_count: u8,
    hello_count: u8,
    list_count: u8,
    read_count: u8,
    begin_count: u8,
    poll_count: u16,
    open_count: u8,
    signature_count: u8,
    close_count: u8,
    stop_count: u8,
    cancel_count: u8,
    cleanup_poll_count: u16,
    first_result_count: u8,
    second_result_count: u8,
    outside_result_count: u8,
    active_budget_ms: u32,
    cleanup_budget_ms: u32,
    watchdog_ms: u32,
    startup_budget_ms: u32,
    startup_request_ms: u32,
    request_ms: u32,
    reap_ms: u32,
    active_elapsed_ms: u32,
    cleanup_elapsed_ms: u32,
    elapsed_ms: u32,
}
impl Evidence {
    fn new() -> Self {
        Self {
            schema_version: 1,
            kind: "java_parameter_info_normal_agent",
            platform: std::env::consts::OS,
            route: "normal_agent_normal_client",
            status: "failed",
            source_fixture_sha256: SOURCE_SHA256,
            active_budget_ms: 570_000,
            cleanup_budget_ms: 180_000,
            watchdog_ms: 780_000,
            startup_budget_ms: 75_000,
            startup_request_ms: 30_000,
            request_ms: 75_000,
            reap_ms: 30_000,
            ..Self::default()
        }
    }
    fn successful(&self) -> bool {
        !self.primary_failed
            && !self.cleanup_failed
            && !self.elapsed_saturated
            && self.exact_capabilities
            && self.provider_supported
            && self.open_version_acknowledged
            && self.first_argument_labels_exact
            && self.first_argument_active_exact
            && self.second_argument_labels_exact
            && self.second_argument_active_exact
            && self.outside_call_empty
            && self.source_unchanged
            && self.startup_terminal
            && self.stop_verified
            && self.client_reaped
            && self.root_exit_observed
            && self.fixture_removed
            && !self.fixture_retained
            && self.all_owned_terminal
            && self.active_deadline_met
            && self.cleanup_deadline_met
            && self.capability_count == 32
            && self.hello_count == 1
            && self.list_count == 1
            && self.read_count == 1
            && self.begin_count == 1
            && (1..=MAX_POLLS).contains(&self.poll_count)
            && self.open_count == 1
            && self.signature_count == 3
            && self.close_count == 1
            && self.stop_count == 1
            && self.cancel_count == 0
            && self.cleanup_poll_count == 0
            && self.first_result_count == 2
            && self.second_result_count == 2
            && self.outside_result_count == 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Call {
    List,
    Read,
    Begin,
    Poll,
    Open,
    First,
    Second,
    Outside,
    Close,
    Cancel,
    CleanupPoll,
    Stop,
}
impl Call {
    fn cost(self) -> Duration {
        match self {
            Self::List
            | Self::Read
            | Self::Begin
            | Self::Poll
            | Self::Cancel
            | Self::CleanupPoll => SHORT,
            _ => REQUEST,
        }
    }
    fn stage(self) -> Stage {
        match self {
            Self::List => Stage::List,
            Self::Read => Stage::Read,
            Self::Begin => Stage::Begin,
            Self::Poll => Stage::Poll,
            Self::Open => Stage::Open,
            Self::First => Stage::FirstArgument,
            Self::Second => Stage::SecondArgument,
            Self::Outside => Stage::OutsideCall,
            Self::Close => Stage::Close,
            Self::Cancel | Self::CleanupPoll | Self::Stop => Stage::Cleanup,
        }
    }
    fn matches(self, operation: &Operation) -> bool {
        matches!(
            (self, operation),
            (Self::List, Operation::List { .. })
                | (Self::Read, Operation::Read { .. })
                | (Self::Begin, Operation::LanguageStartJavaBegin { .. })
                | (
                    Self::Poll | Self::CleanupPoll,
                    Operation::LanguageStartJavaPoll { .. }
                )
                | (Self::Open, Operation::LanguageOpen { .. })
                | (
                    Self::First | Self::Second | Self::Outside,
                    Operation::LanguageJavaSignatureHelp { .. }
                )
                | (Self::Close, Operation::LanguageClose { .. })
                | (Self::Cancel, Operation::LanguageStartJavaCancel { .. })
                | (Self::Stop, Operation::LanguageStop)
        )
    }
}

// No general-purpose call path exists. Failed or repeated calls cannot replay a
// semantic request, and the only dynamic sequence is bounded startup polling.
struct Ledger {
    next: Call,
}
impl Default for Ledger {
    fn default() -> Self {
        Self { next: Call::List }
    }
}
impl Ledger {
    fn record(&mut self, call: Call, e: &mut Evidence) -> ProofResult<()> {
        let count = match call {
            Call::Poll => {
                require(
                    self.next == Call::Poll && e.poll_count < MAX_POLLS,
                    Failure::Protocol,
                )?;
                e.poll_count += 1;
                return Ok(());
            }
            Call::CleanupPoll => {
                require(
                    e.cancel_count == 1 && e.cleanup_poll_count < MAX_POLLS,
                    Failure::Protocol,
                )?;
                e.cleanup_poll_count += 1;
                return Ok(());
            }
            Call::Cancel => &mut e.cancel_count,
            Call::Stop => &mut e.stop_count,
            _ => {
                require(call == self.next, Failure::Protocol)?;
                self.next = match call {
                    Call::List => Call::Read,
                    Call::Read => Call::Begin,
                    Call::Begin => Call::Poll,
                    Call::Open => Call::First,
                    Call::First => Call::Second,
                    Call::Second => Call::Outside,
                    Call::Outside => Call::Close,
                    Call::Close => Call::Stop,
                    _ => return Err(Failure::Protocol),
                };
                match call {
                    Call::List => &mut e.list_count,
                    Call::Read => &mut e.read_count,
                    Call::Begin => &mut e.begin_count,
                    Call::Open => &mut e.open_count,
                    Call::First | Call::Second | Call::Outside => {
                        require(e.signature_count < 3, Failure::Protocol)?;
                        e.signature_count += 1;
                        return Ok(());
                    }
                    Call::Close => &mut e.close_count,
                    _ => return Err(Failure::Protocol),
                }
            }
        };
        require(*count == 0, Failure::Protocol)?;
        *count = 1;
        Ok(())
    }
    fn ready(&mut self) -> ProofResult<()> {
        require(self.next == Call::Poll, Failure::Protocol)?;
        self.next = Call::Open;
        Ok(())
    }
}

fn expected_capabilities() -> Vec<&'static str> {
    let mut values = vec![
        "git_changes",
        "git_diff",
        "java_diagnostics_refresh",
        "language_change",
        "language_close",
        "language_document_symbols",
        "language_events",
        "language_format",
        "language_java_implementations",
        "language_java_signature_help",
        "language_open",
        "language_organize_java_imports",
        "language_query",
        "language_references",
        "language_resolve_completion",
        "language_resolve_uri",
        "language_start_java",
        "language_start_java_begin",
        "language_start_java_cancel",
        "language_start_java_poll",
        "language_stop",
        "language_workspace_symbols",
        "list",
        "read",
        "run_cancel",
        "run_poll",
        "run_start",
        "search",
        "write",
    ];
    if cfg!(windows) {
        values.extend([
            "language_maven_dependencies",
            "language_maven_model",
            "language_start_java_maven_begin",
        ]);
    } else {
        values.extend(["git_status", "language_start", "run"]);
    }
    values.sort_unstable();
    values
}
fn exact_capabilities(info: &AgentInfo) -> bool {
    let groups: &[&str] = if cfg!(windows) {
        &[]
    } else {
        &["java_maven_dependencies_v1", "java_maven_leaf_v1"]
    };
    info.validate().is_ok()
        && info.schema == 1
        && info.version == env!("CARGO_PKG_VERSION")
        && info.os == std::env::consts::OS
        && info.arch == std::env::consts::ARCH
        && info.capabilities.len() == 32
        && info
            .capabilities
            .iter()
            .map(String::as_str)
            .eq(expected_capabilities())
        && info
            .capability_groups
            .iter()
            .map(String::as_str)
            .eq(groups.iter().copied())
}

fn positions() -> ProofResult<[completion::Position; 3]> {
    let first = SOURCE.rfind("\"alpha\"").ok_or(Failure::Setup)? + 3;
    let second = SOURCE.rfind(", 7").ok_or(Failure::Setup)? + 3;
    let outside = SOURCE.find("outsideCall").ok_or(Failure::Setup)? + 5;
    [first, second, outside]
        .map(|byte| completion::byte_to_position(SOURCE, byte).map_err(|_| Failure::Setup))
        .into_iter()
        .collect::<ProofResult<Vec<_>>>()?
        .try_into()
        .map_err(|_| Failure::Setup)
}
fn exact_labels(help: &SignatureHelp) -> bool {
    if help.signatures.len() != 2 {
        return false;
    }
    LABELS
        .iter()
        .all(|label| help.signatures.iter().filter(|s| s.label == *label).count() == 1)
        && help.signatures.iter().all(|signature| {
            let second = if signature.label == LABELS[0] {
                "int second"
            } else {
                "String second"
            };
            signature.parameters.len() == 2
                && signature
                    .parameters
                    .iter()
                    .zip(["String first", second])
                    .all(|(parameter, expected)| {
                        parameter
                            .highlight
                            .clone()
                            .and_then(|range| signature.label.get(range))
                            == Some(expected)
                    })
        })
}
fn exact_active(help: &SignatureHelp, expected: usize) -> bool {
    let Some(selected) = help.server_selected_signature() else {
        return false;
    };
    let signature = &help.signatures[selected];
    signature.label == LABELS[0]
        && help.active_parameter(selected) == Some(expected)
        && help
            .highlight(selected)
            .and_then(|range| signature.label.get(range))
            == Some(if expected == 0 {
                "String first"
            } else {
                "int second"
            })
}
fn provider_supported(value: &Value) -> bool {
    let initialize = &value["initialize"];
    initialize["serverInfo"]["name"] == "JDT Language Server (Standard)"
        && initialize["serverInfo"]["version"] == "1.61.0-SNAPSHOT"
        && initialize["capabilities"]["signatureHelpProvider"].is_object()
}
fn unchanged(path: &Path) -> bool {
    fs::read(path).is_ok_and(|bytes| {
        bytes == SOURCE.as_bytes() && format!("{:x}", Sha256::digest(&bytes)) == SOURCE_SHA256
    })
}

#[cfg(windows)]
struct RootObservation(std::os::windows::io::OwnedHandle);
#[cfg(windows)]
impl RootObservation {
    fn open(pid: u32) -> ProofResult<Self> {
        use std::os::windows::io::FromRawHandle;
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
        };
        // SAFETY: a query/synchronize-only handle to the exact PID reported by
        // this owned startup; it is non-inheritable and never used to terminate.
        let handle = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                0,
                pid,
            )
        };
        if handle.is_null() {
            return Err(Failure::RootObservation);
        }
        Ok(Self(unsafe {
            std::os::windows::io::OwnedHandle::from_raw_handle(handle)
        }))
    }
    fn exited(&self) -> ProofResult<Option<u32>> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::{
            Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT},
            System::Threading::{GetExitCodeProcess, WaitForSingleObject},
        };
        // SAFETY: this owned handle remains alive during both bounded queries.
        match unsafe { WaitForSingleObject(self.0.as_raw_handle(), 3000) } {
            WAIT_OBJECT_0 => {
                let mut code = 0;
                require(
                    unsafe { GetExitCodeProcess(self.0.as_raw_handle(), &mut code) } != 0,
                    Failure::RootObservation,
                )?;
                Ok(Some(code))
            }
            WAIT_TIMEOUT => Ok(None),
            _ => Err(Failure::RootObservation),
        }
    }
}

struct Owner {
    evidence: Evidence,
    stage: Stage,
    ledger: Ledger,
    client: Option<Client>,
    connect_attempted: bool,
    connect_returned: bool,
    begin_sent: bool,
    startup_id: Option<u64>,
    ready: bool,
    java_terminal: bool,
    reported_pid: Option<u32>,
    #[cfg(windows)]
    root: Option<RootObservation>,
    #[cfg(windows)]
    expected_exit: Option<u32>,
}
impl Owner {
    fn new() -> Self {
        Self {
            evidence: Evidence::new(),
            stage: Stage::Setup,
            ledger: Ledger::default(),
            client: None,
            connect_attempted: false,
            connect_returned: false,
            begin_sent: false,
            startup_id: None,
            ready: false,
            java_terminal: false,
            reported_pid: None,
            #[cfg(windows)]
            root: None,
            #[cfg(windows)]
            expected_exit: None,
        }
    }
    fn connect(&mut self, root: &Path, active: Budget) -> ProofResult<()> {
        self.stage = Stage::Connect;
        active.admit(SHORT)?;
        self.connect_attempted = true;
        self.evidence.hello_count = 1;
        #[cfg(target_os = "linux")]
        let spec = ConnectionSpec::BundledLinux {
            root: root.to_owned(),
            allow_run: true,
        };
        #[cfg(windows)]
        let spec = ConnectionSpec::Local {
            root: root.to_owned(),
            allow_run: true,
        };
        // Retain the returned Client before any post-return check. A failed
        // Hello does not return ownership evidence through this ordinary API.
        let connected = Client::connect(spec);
        if let Ok(client) = connected {
            self.client = Some(client);
            self.connect_returned = true;
        }
        active.check()?;
        require(self.client.is_some(), Failure::Connection)
    }
    fn observe(&mut self, call: Call, payload: &Payload) -> ProofResult<()> {
        let Payload::Language { value } = payload else {
            return Ok(());
        };
        if call == Call::Begin {
            self.startup_id = value["startup_id"].as_u64().filter(|id| *id != 0);
        }
        if matches!(
            call,
            Call::Begin | Call::Poll | Call::Cancel | Call::CleanupPoll
        ) && self.startup_id.is_some()
            && value["startup_id"].as_u64() == self.startup_id
        {
            if value["state"] == "ready" {
                self.ready = true;
                self.evidence.startup_terminal = true;
            }
            if matches!(value["state"].as_str(), Some("cancelled" | "failed"))
                && value["cleanup_verified"] == true
            {
                self.java_terminal = true;
                self.evidence.startup_terminal = true;
            }
            let pid = value["process_id"]
                .as_u64()
                .or_else(|| value["language"]["process_id"].as_u64())
                .and_then(|pid| u32::try_from(pid).ok())
                .filter(|pid| *pid != 0);
            if let Some(pid) = pid {
                require(
                    self.reported_pid.is_none_or(|previous| previous == pid),
                    Failure::RootObservation,
                )?;
                self.reported_pid = Some(pid);
                #[cfg(windows)]
                if self.root.is_none() {
                    self.root = Some(RootObservation::open(pid)?);
                }
            }
        }
        if call == Call::Stop {
            if let Ok(stop) = java_language::JavaStopOutcome::parse(value) {
                let platform_matches = if cfg!(windows) {
                    matches!(stop.root_exit, java_language::JavaRootExit::WindowsCode(_))
                } else {
                    matches!(
                        stop.root_exit,
                        java_language::JavaRootExit::LinuxCode(_)
                            | java_language::JavaRootExit::LinuxSignal(_)
                    )
                };
                if platform_matches {
                    self.java_terminal = true;
                    self.evidence.stop_verified = true;
                    #[cfg(windows)]
                    {
                        self.expected_exit = stop.root_exit_code();
                    }
                }
            }
        }
        Ok(())
    }
    fn call(
        &mut self,
        call: Call,
        operation: Operation,
        budgets: &[Budget],
    ) -> ProofResult<Payload> {
        self.stage = call.stage();
        require(call.matches(&operation), Failure::Protocol)?;
        for budget in budgets {
            budget.admit(call.cost())?;
        }
        self.ledger.record(call, &mut self.evidence)?;
        if call == Call::Begin {
            self.begin_sent = true;
        }
        let result = self
            .client
            .as_mut()
            .ok_or(Failure::Connection)?
            .request(operation);
        // Lifecycle evidence must survive late returns and later semantic errors.
        let observation = if let Ok(payload) = &result {
            self.observe(call, payload)
        } else {
            Ok(())
        };
        for budget in budgets {
            budget.check()?;
        }
        observation?;
        result.map_err(|_| Failure::Protocol)
    }
    fn language(
        &mut self,
        call: Call,
        operation: Operation,
        budgets: &[Budget],
    ) -> ProofResult<Value> {
        match self.call(call, operation, budgets)? {
            Payload::Language { value } => Ok(value),
            _ => Err(Failure::Protocol),
        }
    }
    fn startup(
        &mut self,
        java: String,
        distribution: String,
        data: String,
        active: Budget,
    ) -> ProofResult<Value> {
        // The shared window starts before Begin, including its full request.
        let startup = Budget::new(STARTUP);
        let value = self.language(
            Call::Begin,
            Operation::LanguageStartJavaBegin {
                java_executable: java,
                distribution,
                data_directory: data,
            },
            &[active, startup],
        )?;
        let id = self.startup_id.ok_or(Failure::Startup)?;
        require(value["state"] == "starting", Failure::Startup)?;
        loop {
            let value = self.language(
                Call::Poll,
                Operation::LanguageStartJavaPoll { startup_id: id },
                &[active, startup],
            )?;
            require(value["startup_id"] == id, Failure::Startup)?;
            match value["state"].as_str() {
                Some("ready") => {
                    require(
                        value["language"]["started"] == true && self.reported_pid.is_some(),
                        Failure::Startup,
                    )?;
                    self.ledger.ready()?;
                    return Ok(value["language"].clone());
                }
                Some("starting") => {
                    // Admit the delay and the next complete request together.
                    active.admit(POLL_DELAY + SHORT)?;
                    startup.admit(POLL_DELAY + SHORT)?;
                    thread::sleep(POLL_DELAY);
                    active.check()?;
                    startup.check()?;
                }
                _ => return Err(Failure::Startup),
            }
        }
    }
    fn settle_java(&mut self, cleanup: Budget) -> ProofResult<()> {
        self.stage = Stage::Cleanup;
        if !self.begin_sent || self.java_terminal {
            return Ok(());
        }
        if !self.ready {
            let id = self.startup_id.ok_or(Failure::Startup)?;
            let cancel = Budget::new(STARTUP);
            let mut call = Call::Cancel;
            loop {
                // Even a Ready/cancel race retains the full Stop + reap reserve.
                cleanup.admit(call.cost() + REQUEST + REAP)?;
                let op = if call == Call::Cancel {
                    Operation::LanguageStartJavaCancel { startup_id: id }
                } else {
                    Operation::LanguageStartJavaPoll { startup_id: id }
                };
                let value = self.language(call, op, &[cleanup, cancel])?;
                require(value["startup_id"] == id, Failure::Startup)?;
                if self.java_terminal {
                    return Ok(());
                }
                if self.ready {
                    break;
                }
                require(value["state"] == "cancelling", Failure::Startup)?;
                cleanup.admit(POLL_DELAY + SHORT + REQUEST + REAP)?;
                cancel.admit(POLL_DELAY + SHORT)?;
                thread::sleep(POLL_DELAY);
                cleanup.check()?;
                cancel.check()?;
                call = Call::CleanupPoll;
            }
        }
        cleanup.admit(REQUEST + REAP)?;
        let value = self.language(Call::Stop, Operation::LanguageStop, &[cleanup])?;
        let stop = java_language::JavaStopOutcome::parse(&value).map_err(|_| Failure::Stop)?;
        require(
            self.evidence.stop_verified && stop.status != java_language::StopStatus::Error,
            Failure::Stop,
        )
    }
    fn reap(&mut self, cleanup: Budget) -> ProofResult<()> {
        if self.client.is_none() {
            return require(!self.connect_attempted, Failure::Reap);
        }
        cleanup.admit(REAP)?;
        let result = self
            .client
            .take()
            .ok_or(Failure::Reap)?
            .close_and_wait(REAP);
        self.evidence.client_reaped = result.is_ok();
        cleanup.check()?;
        result.map_err(|_| Failure::Reap)
    }
    fn observe_root(&mut self, cleanup: Budget, before_reap: bool) -> ProofResult<()> {
        #[cfg(windows)]
        {
            if self.evidence.root_exit_observed {
                return Ok(());
            }
            if let Some(root) = &self.root {
                cleanup.admit(
                    Duration::from_secs(3) + if before_reap { REAP } else { Duration::ZERO },
                )?;
                let exit = root.exited()?;
                cleanup.check()?;
                if let Some(exit) = exit {
                    require(
                        self.expected_exit.is_none_or(|expected| expected == exit),
                        Failure::RootObservation,
                    )?;
                    self.evidence.root_exit_observed = true;
                }
            }
        }
        #[cfg(not(windows))]
        {
            let _ = (cleanup, before_reap);
            self.evidence.root_exit_observed = self.java_terminal;
        }
        Ok(())
    }
    fn all_terminal(&self) -> bool {
        let client =
            !self.connect_attempted || (self.connect_returned && self.evidence.client_reaped);
        let java = !self.begin_sent || (self.evidence.startup_terminal && self.java_terminal);
        #[cfg(windows)]
        let observed = self.reported_pid.is_none() || self.evidence.root_exit_observed;
        #[cfg(not(windows))]
        let observed = true;
        client && java && observed
    }
}

fn path_text(path: &Path) -> ProofResult<String> {
    path.to_str().map(str::to_owned).ok_or(Failure::Setup)
}
fn explicit_path(name: &str) -> ProofResult<PathBuf> {
    let path = PathBuf::from(std::env::var_os(name).ok_or(Failure::Setup)?);
    require(path.is_absolute(), Failure::Setup)?;
    ordinary_path(&path)
}
fn ordinary_path(path: &Path) -> ProofResult<PathBuf> {
    let canonical = io(path.canonicalize())?;
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        let text = path_text(&canonical)?;
        let ordinary = match canonical.components().next() {
            Some(Component::Prefix(prefix)) => match prefix.kind() {
                Prefix::VerbatimDisk(_) => {
                    PathBuf::from(text.strip_prefix(r"\\?\").ok_or(Failure::Setup)?)
                }
                Prefix::Disk(_) => canonical.clone(),
                _ => return Err(Failure::Setup),
            },
            _ => return Err(Failure::Setup),
        };
        require(io(ordinary.canonicalize())? == canonical, Failure::Setup)?;
        Ok(ordinary)
    }
    #[cfg(not(windows))]
    {
        Ok(canonical)
    }
}
fn prepare_fixture(slot: &mut Option<PathBuf>) -> ProofResult<()> {
    // keep() prevents TempDir Drop from deleting evidence after uncertain exits.
    let root = io(tempfile::Builder::new()
        .prefix("cedar-parameter-info-")
        .tempdir())?
    .keep();
    *slot = Some(root.clone());
    let project = root.join("workspace").join(PROJECT);
    io(fs::create_dir_all(project.join("src")))?;
    io(fs::create_dir_all(project.join(".settings")))?;
    io(fs::create_dir(root.join("jdt-data")))?;
    io(fs::write(project.join(".project"), "<?xml version=\"1.0\"?><projectDescription><name>cedar-parameter-info</name><projects/><buildSpec><buildCommand><name>org.eclipse.jdt.core.javabuilder</name><arguments/></buildCommand></buildSpec><natures><nature>org.eclipse.jdt.core.javanature</nature></natures></projectDescription>"))?;
    io(fs::write(project.join(".classpath"), "<?xml version=\"1.0\"?><classpath><classpathentry kind=\"src\" path=\"src\"/><classpathentry kind=\"con\" path=\"org.eclipse.jdt.launching.JRE_CONTAINER\"/><classpathentry kind=\"output\" path=\"bin\"/></classpath>"))?;
    io(fs::write(project.join(".settings/org.eclipse.jdt.core.prefs"), "eclipse.preferences.version=1\norg.eclipse.jdt.core.compiler.codegen.targetPlatform=21\norg.eclipse.jdt.core.compiler.compliance=21\norg.eclipse.jdt.core.compiler.source=21\n"))?;
    let mut file = io(fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join("workspace").join(SOURCE_PATH)))?;
    io(file.write_all(SOURCE.as_bytes()))?;
    require(
        unchanged(&root.join("workspace").join(SOURCE_PATH)),
        Failure::Setup,
    )
}
fn primary(owner: &mut Owner, fixture: &mut Option<PathBuf>, active: Budget) -> ProofResult<()> {
    let java = explicit_path("CEDAR_JAVA_COMMAND")?;
    let distribution = explicit_path("CEDAR_JDT_LS_ROOT")?;
    require(java.is_file() && distribution.is_dir(), Failure::Setup)?;
    let executable = io(std::env::current_exe())?;
    let sibling = executable
        .parent()
        .ok_or(Failure::Setup)?
        .join(if cfg!(windows) {
            "cedar-agent.exe"
        } else {
            "cedar-agent"
        });
    require(sibling.is_file(), Failure::Setup)?;
    prepare_fixture(fixture)?;
    let fixture_root = fixture.as_ref().ok_or(Failure::Setup)?;
    let root = ordinary_path(&fixture_root.join("workspace"))?;
    owner.connect(&root, active)?;
    owner.stage = Stage::Capabilities;
    let Payload::Hello {
        protocol: 4,
        agent: Some(info),
        ..
    } = owner
        .client
        .as_ref()
        .ok_or(Failure::Connection)?
        .handshake()
    else {
        return Err(Failure::Capability);
    };
    owner.evidence.capability_count = info.capabilities.len().min(32) as u8;
    owner.evidence.exact_capabilities = exact_capabilities(info);
    require(owner.evidence.exact_capabilities, Failure::Capability)?;
    let listed = owner.call(Call::List, Operation::List { path: ".".into() }, &[active])?;
    require(
        matches!(listed, Payload::Entries { entries } if entries.iter().any(|entry| entry.path == PROJECT && entry.is_dir)),
        Failure::Protocol,
    )?;
    let read = owner.call(
        Call::Read,
        Operation::Read {
            path: SOURCE_PATH.into(),
        },
        &[active],
    )?;
    require(
        matches!(read, Payload::File { path, text, .. } if path == SOURCE_PATH && text == SOURCE),
        Failure::Semantics,
    )?;
    let initialized = owner.startup(
        path_text(&java)?,
        path_text(&distribution)?,
        path_text(&ordinary_path(&fixture_root.join("jdt-data"))?)?,
        active,
    )?;
    owner.evidence.provider_supported = provider_supported(&initialized);
    require(owner.evidence.provider_supported, Failure::Provider)?;
    let opened = owner.language(
        Call::Open,
        Operation::LanguageOpen {
            path: SOURCE_PATH.into(),
            language_id: "java".into(),
            version: 1,
            text: SOURCE.into(),
        },
        &[active],
    )?;
    let uri = opened["opened"]
        .as_str()
        .ok_or(Failure::Protocol)?
        .to_owned();
    owner.evidence.open_version_acknowledged = opened["version"] == 1;
    require(owner.evidence.open_version_acknowledged, Failure::Semantics)?;
    for (index, (call, position)) in [Call::First, Call::Second, Call::Outside]
        .into_iter()
        .zip(positions()?)
        .enumerate()
    {
        let value = owner.language(
            call,
            Operation::LanguageJavaSignatureHelp {
                path: SOURCE_PATH.into(),
                version: 1,
                line: position.line,
                character: position.character,
            },
            &[active],
        )?;
        let help = signature_help::parse(&value).map_err(|_| Failure::Semantics)?;
        let count = help.signatures.len() as u8;
        match index {
            0 => {
                owner.evidence.first_result_count = count;
                owner.evidence.first_argument_labels_exact = exact_labels(&help);
                owner.evidence.first_argument_active_exact = exact_active(&help, 0);
                require(
                    owner.evidence.first_argument_labels_exact
                        && owner.evidence.first_argument_active_exact,
                    Failure::Semantics,
                )?;
            }
            1 => {
                owner.evidence.second_result_count = count;
                owner.evidence.second_argument_labels_exact = exact_labels(&help);
                owner.evidence.second_argument_active_exact = exact_active(&help, 1);
                require(
                    owner.evidence.second_argument_labels_exact
                        && owner.evidence.second_argument_active_exact,
                    Failure::Semantics,
                )?;
            }
            _ => {
                owner.evidence.outside_result_count = count;
                // A nonempty negative response is a failure, never a trigger
                // for a fourth/recovery query.
                owner.evidence.outside_call_empty = help.signatures.is_empty();
                require(owner.evidence.outside_call_empty, Failure::Semantics)?;
            }
        }
    }
    let closed = owner.language(
        Call::Close,
        Operation::LanguageClose {
            path: SOURCE_PATH.into(),
        },
        &[active],
    )?;
    require(closed["closed"] == uri, Failure::Protocol)?;
    owner.stage = Stage::SourceCheck;
    owner.evidence.source_unchanged = unchanged(&root.join(SOURCE_PATH));
    require(owner.evidence.source_unchanged, Failure::Semantics)?;
    active.check()
}

fn elapsed_ms(elapsed: Duration, cap: Duration, saturated: &mut bool) -> u32 {
    if elapsed > cap {
        *saturated = true;
    }
    elapsed.as_millis().min(cap.as_millis()) as u32
}
fn may_remove_fixture(
    all_owned_terminal: bool,
    cleanup_failed: bool,
    cleanup_elapsed: Duration,
) -> bool {
    all_owned_terminal && !cleanup_failed && within(cleanup_elapsed, CLEANUP).is_ok()
}
fn write_receipt(path: &Path, evidence: &Evidence) -> ProofResult<()> {
    let bytes = serde_json::to_vec(evidence).map_err(|_| Failure::Receipt)?;
    require(bytes.len() <= 8192, Failure::Receipt)?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| Failure::Receipt)?;
    file.write_all(&bytes).map_err(|_| Failure::Receipt)?;
    file.write_all(b"\n").map_err(|_| Failure::Receipt)
}

type PanicPayload = Box<dyn std::any::Any + Send>;
fn settle_steps(
    mut action: impl FnMut(u8) -> ProofResult<()>,
    original_panic: &mut Option<PanicPayload>,
) -> bool {
    let mut failed = false;
    for step in 0..4 {
        match catch_unwind(AssertUnwindSafe(|| action(step))) {
            Ok(Ok(())) => {}
            Ok(Err(_)) => failed = true,
            Err(payload) => {
                failed = true;
                if original_panic.is_none() {
                    *original_panic = Some(payload);
                }
            }
        }
    }
    failed
}

#[test]
#[ignore = "CI-only: requires explicitly pinned JDT/JDK and a verified normal agent sibling"]
fn real_jdt_parameter_info_uses_normal_agent_route() {
    assert_eq!(
        std::env::var("CEDAR_REAL_JDT_PARAMETER_INFO").as_deref(),
        Ok("1"),
        "explicit proof opt-in required"
    );
    let receipt = PathBuf::from(
        std::env::var_os("CEDAR_REAL_JDT_PARAMETER_INFO_RECEIPT")
            .expect("explicit receipt path required"),
    );
    assert!(
        receipt.is_absolute() && !receipt.exists(),
        "receipt must be a new absolute file"
    );
    let _watchdog = Watchdog::new();
    let elapsed = Instant::now();
    let active = Budget::new(ACTIVE);
    let mut owner = Owner::new();
    let mut fixture = None;
    let result = catch_unwind(AssertUnwindSafe(|| {
        primary(&mut owner, &mut fixture, active)
    }));
    let mut panic = None;
    match result {
        Ok(Ok(())) => {}
        Ok(Err(_)) => owner.evidence.primary_failed = true,
        Err(payload) => {
            owner.evidence.primary_failed = true;
            panic = Some(payload);
        }
    }
    if owner.evidence.primary_failed {
        owner.evidence.failure_stage = owner.stage;
    }
    owner.evidence.active_deadline_met = active.check().is_ok();
    owner.evidence.active_elapsed_ms = elapsed_ms(
        active.started.elapsed(),
        ACTIVE,
        &mut owner.evidence.elapsed_saturated,
    );
    let cleanup = Budget::new(CLEANUP);
    // Independently catch settlement, observation and reap: a panic in one does
    // not skip the remaining owned steps. Resume the original panic afterward.
    owner.evidence.cleanup_failed = settle_steps(
        |step| match step {
            0 => owner.settle_java(cleanup),
            1 | 3 => owner.observe_root(cleanup, step == 1),
            _ => owner.reap(cleanup),
        },
        &mut panic,
    );
    owner.evidence.all_owned_terminal = owner.all_terminal();
    if !owner.evidence.all_owned_terminal {
        owner.evidence.cleanup_failed = true;
    }
    if let Some(root) = &fixture {
        owner.evidence.source_unchanged = unchanged(&root.join("workspace").join(SOURCE_PATH));
        if !owner.evidence.source_unchanged {
            if !owner.evidence.primary_failed {
                owner.evidence.failure_stage = Stage::SourceCheck;
            }
            owner.evidence.primary_failed = true;
        }
        if may_remove_fixture(
            owner.evidence.all_owned_terminal,
            owner.evidence.cleanup_failed,
            cleanup.started.elapsed(),
        ) {
            owner.evidence.fixture_removed = fs::remove_dir_all(root).is_ok();
        }
        owner.evidence.fixture_retained = !owner.evidence.fixture_removed;
        if owner.evidence.fixture_retained {
            owner.evidence.cleanup_failed = true;
        }
    }
    owner.evidence.cleanup_deadline_met = cleanup.check().is_ok();
    if !owner.evidence.cleanup_deadline_met {
        owner.evidence.cleanup_failed = true;
    }
    owner.evidence.cleanup_elapsed_ms = elapsed_ms(
        cleanup.started.elapsed(),
        CLEANUP,
        &mut owner.evidence.elapsed_saturated,
    );
    owner.evidence.elapsed_ms = elapsed_ms(
        elapsed.elapsed(),
        WATCHDOG,
        &mut owner.evidence.elapsed_saturated,
    );
    if owner.evidence.cleanup_failed
        && owner.evidence.failure_stage == Stage::Setup
        && !owner.evidence.primary_failed
    {
        owner.evidence.failure_stage = Stage::Cleanup;
    }
    if owner.evidence.successful() {
        owner.evidence.status = "passed";
        owner.evidence.failure_stage = Stage::None;
    } else if !owner.evidence.primary_failed && owner.evidence.failure_stage == Stage::Setup {
        owner.evidence.failure_stage = Stage::Cleanup;
    }
    owner.stage = Stage::Receipt;
    let receipt_result = write_receipt(&receipt, &owner.evidence);
    if let Some(payload) = panic {
        resume_unwind(payload);
    }
    assert!(
        receipt_result.is_ok(),
        "bounded parameter-info receipt write failed"
    );
    assert_eq!(
        owner.evidence.status, "passed",
        "parameter-info proof failed; inspect fixed receipt"
    );
}

#[test]
fn fixed_budget_admission_includes_every_full_normal_timeout() {
    assert_eq!(
        SHORT * 3 + STARTUP + REQUEST * 5 + Duration::from_secs(30),
        ACTIVE
    );
    assert_eq!(STARTUP + REQUEST + REAP, CLEANUP);
    for (limit, cost, last) in [
        (STARTUP, SHORT, Duration::from_secs(45)),
        (ACTIVE, REQUEST, Duration::from_secs(495)),
        (CLEANUP, REQUEST + REAP, Duration::from_secs(75)),
        (CLEANUP, SHORT + REQUEST + REAP, Duration::from_secs(45)),
    ] {
        assert!(admit(last, limit, cost).is_ok());
        assert_eq!(
            admit(last + Duration::from_nanos(1), limit, cost),
            Err(Failure::Admission)
        );
        assert!(within(limit, limit).is_ok());
        assert_eq!(
            within(limit + Duration::from_nanos(1), limit),
            Err(Failure::Deadline)
        );
    }
    // Begin plus one slow Poll must not receive a fresh 75-second window.
    assert!(admit(SHORT, STARTUP, SHORT).is_ok());
    assert_eq!(admit(SHORT * 2, STARTUP, SHORT), Err(Failure::Admission));
    assert!(admit(Duration::from_millis(44_950), STARTUP, POLL_DELAY + SHORT).is_ok());
    assert!(admit(Duration::from_millis(44_951), STARTUP, POLL_DELAY + SHORT).is_err());
    assert_eq!(admit(ACTIVE, ACTIVE, REQUEST), Err(Failure::Admission));
    assert!(ACTIVE + CLEANUP < WATCHDOG);
}

#[test]
fn failed_or_unverified_cleanup_always_retains_the_fixture() {
    assert!(may_remove_fixture(true, false, Duration::from_secs(100)));
    assert!(!may_remove_fixture(true, true, Duration::from_secs(100)));
    assert!(!may_remove_fixture(false, false, Duration::from_secs(100)));
    assert!(!may_remove_fixture(false, true, Duration::from_secs(100)));
    assert!(!may_remove_fixture(
        true,
        false,
        CLEANUP + Duration::from_nanos(1)
    ));
    // Primary semantics may fail without invalidating independently verified
    // cleanup; a cleanup failure can never be overridden by terminal booleans.
    let mut evidence = Evidence::new();
    evidence.primary_failed = true;
    evidence.all_owned_terminal = true;
    evidence.cleanup_failed = true;
    assert!(!may_remove_fixture(
        evidence.all_owned_terminal,
        evidence.cleanup_failed,
        Duration::ZERO
    ));
}

#[test]
fn ledger_allows_only_one_ordered_three_query_session() {
    let mut ledger = Ledger::default();
    let mut evidence = Evidence::new();
    assert!(ledger.record(Call::Begin, &mut evidence).is_err());
    ledger.record(Call::List, &mut evidence).unwrap();
    assert!(ledger.record(Call::List, &mut evidence).is_err());
    ledger.record(Call::Read, &mut evidence).unwrap();
    ledger.record(Call::Begin, &mut evidence).unwrap();
    assert!(ledger.record(Call::Read, &mut evidence).is_err());
    assert!(ledger.record(Call::Open, &mut evidence).is_err());
    for _ in 0..MAX_POLLS {
        ledger.record(Call::Poll, &mut evidence).unwrap();
    }
    assert!(ledger.record(Call::Poll, &mut evidence).is_err());
    ledger.ready().unwrap();
    for call in [
        Call::Open,
        Call::First,
        Call::Second,
        Call::Outside,
        Call::Close,
        Call::Stop,
    ] {
        ledger.record(call, &mut evidence).unwrap();
    }
    for extra in [
        Call::Read,
        Call::Begin,
        Call::Poll,
        Call::Open,
        Call::First,
        Call::Outside,
        Call::Close,
        Call::Stop,
    ] {
        assert!(ledger.record(extra, &mut evidence).is_err());
    }
    assert_eq!(evidence.signature_count, 3);
    assert_eq!(evidence.poll_count, MAX_POLLS);
    for (call, op) in [
        (Call::Read, Operation::LanguageEvents),
        (
            Call::First,
            Operation::LanguageRefreshJavaDiagnostics {
                path: SOURCE_PATH.into(),
                version: 1,
            },
        ),
        (
            Call::First,
            Operation::Read {
                path: SOURCE_PATH.into(),
            },
        ),
        (Call::Begin, Operation::Hello),
    ] {
        assert!(!call.matches(&op));
    }
}

#[test]
fn cleanup_ledger_keeps_cancel_and_poll_finite_without_semantic_replays() {
    let mut ledger = Ledger::default();
    let mut evidence = Evidence::new();
    assert!(ledger.record(Call::CleanupPoll, &mut evidence).is_err());
    ledger.record(Call::Cancel, &mut evidence).unwrap();
    assert!(ledger.record(Call::Cancel, &mut evidence).is_err());
    for _ in 0..MAX_POLLS {
        ledger.record(Call::CleanupPoll, &mut evidence).unwrap();
    }
    assert!(ledger.record(Call::CleanupPoll, &mut evidence).is_err());
    ledger.record(Call::Stop, &mut evidence).unwrap();
    assert_eq!(evidence.cancel_count, 1);
    assert_eq!(evidence.cleanup_poll_count, MAX_POLLS);
    assert_eq!(evidence.signature_count, 0);
    assert_eq!(evidence.read_count, 0);
}

fn synthetic_signature_result(active: u32) -> Value {
    serde_json::json!({
        "signatures": [
            {"label": LABELS[0], "parameters": [{"label":"String first"},{"label":"int second"}]},
            {"label": LABELS[1], "parameters": [{"label":"String first"},{"label":"String second"}]}
        ],
        "activeSignature": 0,
        "activeParameter": active
    })
}

#[test]
fn semantic_witness_is_exact_and_requires_real_in_range_active_hints() {
    for argument in [0, 1] {
        let value = synthetic_signature_result(argument);
        let help = signature_help::parse(&value).unwrap();
        assert!(exact_labels(&help));
        assert!(exact_active(&help, argument as usize));
        assert!(!exact_active(&help, 1 - argument as usize));
        let mut reversed = value.clone();
        reversed["signatures"].as_array_mut().unwrap().reverse();
        reversed["activeSignature"] = serde_json::json!(1);
        let help = signature_help::parse(&reversed).unwrap();
        assert!(exact_labels(&help));
        assert!(exact_active(&help, argument as usize));
        for (pointer, wrong) in [
            ("/activeSignature", Value::Null),
            ("/activeSignature", serde_json::json!(2)),
            ("/activeSignature", serde_json::json!(1)),
            ("/activeParameter", Value::Null),
            ("/activeParameter", serde_json::json!(2)),
        ] {
            let mut changed = value.clone();
            *changed.pointer_mut(pointer).unwrap() = wrong;
            assert!(!exact_active(
                &signature_help::parse(&changed).unwrap(),
                argument as usize
            ));
        }
        let mut override_hint = value;
        override_hint["signatures"][0]["activeParameter"] = serde_json::json!(99);
        assert!(!exact_active(
            &signature_help::parse(&override_hint).unwrap(),
            argument as usize
        ));
    }
    for value in [Value::Null, serde_json::json!({"signatures":[]})] {
        let help = signature_help::parse(&value).unwrap();
        assert!(help.signatures.is_empty());
        assert!(!exact_labels(&help));
        assert!(!exact_active(&help, 0));
    }
    let good = synthetic_signature_result(0);
    for value in [
        serde_json::json!({"signatures":[good["signatures"][0]]}),
        serde_json::json!({"signatures":[good["signatures"][0],good["signatures"][0]]}),
        serde_json::json!({"signatures":[good["signatures"][0],good["signatures"][1],good["signatures"][1]]}),
    ] {
        assert!(!exact_labels(&signature_help::parse(&value).unwrap()));
    }
    for (pointer, wrong) in [
        (
            "/signatures/1/parameters/1/label",
            serde_json::json!("second"),
        ),
        ("/signatures/0/parameters", serde_json::json!([])),
    ] {
        let mut altered = good.clone();
        *altered.pointer_mut(pointer).unwrap() = wrong;
        assert!(!exact_labels(&signature_help::parse(&altered).unwrap()));
    }
    let mut different_type = good;
    different_type["signatures"][0]["label"] =
        serde_json::json!("cedarParameter(String first, long second) : String");
    assert!(signature_help::parse(&different_type).is_err());
    different_type["signatures"][0]["parameters"][1]["label"] = serde_json::json!("long second");
    assert!(!exact_labels(
        &signature_help::parse(&different_type).unwrap()
    ));
}

#[test]
fn panic_settlement_runs_every_remaining_step_and_preserves_original_unwind() {
    let mut original: Option<PanicPayload> = Some(Box::new("primary panic"));
    let mut visited = Vec::new();
    assert!(settle_steps(
        |step| {
            visited.push(step);
            match step {
                0 => std::panic::panic_any("settlement panic"),
                1 => Err(Failure::RootObservation),
                _ => Ok(()),
            }
        },
        &mut original
    ));
    assert_eq!(visited, [0, 1, 2, 3]);
    assert_eq!(
        original.unwrap().downcast_ref::<&str>(),
        Some(&"primary panic")
    );
    let mut first_cleanup_panic = None;
    assert!(settle_steps(
        |step| {
            if step == 2 {
                std::panic::panic_any("reap panic");
            }
            Ok(())
        },
        &mut first_cleanup_panic
    ));
    assert_eq!(
        first_cleanup_panic.unwrap().downcast_ref::<&str>(),
        Some(&"reap panic")
    );
    let mut absent = None;
    assert!(!settle_steps(|_| Ok(()), &mut absent));
    assert!(absent.is_none());
}

#[test]
fn success_predicate_requires_each_witness_and_exact_nonreplayed_counts() {
    // Synthetic predicate input only: this never runs a process or emits a
    // receipt and cannot stand in for the ignored real-JDT producer.
    let passing = Evidence {
        exact_capabilities: true,
        provider_supported: true,
        open_version_acknowledged: true,
        first_argument_labels_exact: true,
        first_argument_active_exact: true,
        second_argument_labels_exact: true,
        second_argument_active_exact: true,
        outside_call_empty: true,
        source_unchanged: true,
        startup_terminal: true,
        stop_verified: true,
        client_reaped: true,
        root_exit_observed: true,
        fixture_removed: true,
        all_owned_terminal: true,
        active_deadline_met: true,
        cleanup_deadline_met: true,
        capability_count: 32,
        hello_count: 1,
        list_count: 1,
        read_count: 1,
        begin_count: 1,
        poll_count: 1,
        open_count: 1,
        signature_count: 3,
        close_count: 1,
        stop_count: 1,
        first_result_count: 2,
        second_result_count: 2,
        ..Evidence::new()
    };
    assert!(passing.successful());
    macro_rules! rejects {
        ($($field:ident = $wrong:expr),+ $(,)?) => {$({
            let mut altered = passing.clone();
            altered.$field = $wrong;
            assert!(!altered.successful(), stringify!($field));
        })+};
    }
    rejects!(
        exact_capabilities = false,
        provider_supported = false,
        open_version_acknowledged = false,
        first_argument_labels_exact = false,
        first_argument_active_exact = false,
        second_argument_labels_exact = false,
        second_argument_active_exact = false,
        outside_call_empty = false,
        source_unchanged = false,
        startup_terminal = false,
        stop_verified = false,
        client_reaped = false,
        root_exit_observed = false,
        fixture_removed = false,
        all_owned_terminal = false,
        active_deadline_met = false,
        cleanup_deadline_met = false,
        primary_failed = true,
        cleanup_failed = true,
        fixture_retained = true,
        elapsed_saturated = true,
        capability_count = 31,
        hello_count = 2,
        list_count = 2,
        read_count = 2,
        begin_count = 2,
        poll_count = 0,
        poll_count = MAX_POLLS + 1,
        open_count = 2,
        signature_count = 2,
        signature_count = 4,
        close_count = 0,
        stop_count = 0,
        cancel_count = 1,
        cleanup_poll_count = 1,
        first_result_count = 1,
        second_result_count = 3,
        outside_result_count = 1,
    );
}

#[test]
fn source_hash_and_utf16_cursor_positions_are_fixed_and_distinct() {
    assert_eq!(
        format!("{:x}", Sha256::digest(SOURCE.as_bytes())),
        SOURCE_SHA256
    );
    let positions = positions().unwrap();
    assert_eq!(positions[0].line, 4);
    assert_eq!(positions[1].line, 4);
    assert_eq!(positions[2].line, 5);
    assert!(positions[0].character < positions[1].character);
    // The non-BMP comment ensures byte/scalar/UTF-16 cursor confusion is caught.
    let line = SOURCE.lines().nth(4).unwrap();
    let byte = line.find("\"alpha\"").unwrap() + 3;
    assert_eq!(
        positions[0].character as usize,
        line[..byte].encode_utf16().count()
    );
    assert_ne!(positions[0].character as usize, byte);
    assert_ne!(
        positions[0].character as usize,
        line[..byte].chars().count()
    );
    for position in positions {
        let operation = Operation::LanguageJavaSignatureHelp {
            path: SOURCE_PATH.into(),
            version: 1,
            line: position.line,
            character: position.character,
        };
        let wire = serde_json::to_value(operation).unwrap();
        assert_eq!(wire.as_object().unwrap().len(), 5);
        assert_eq!(wire["type"], "language_java_signature_help");
        assert_eq!(wire["version"], 1);
        assert_eq!(wire["path"], SOURCE_PATH);
    }
}

#[test]
fn cached_handshake_inventory_and_pinned_provider_fail_closed() {
    let mut info = AgentInfo {
        schema: 1,
        version: env!("CARGO_PKG_VERSION").into(),
        os: std::env::consts::OS.into(),
        arch: std::env::consts::ARCH.into(),
        capabilities: expected_capabilities()
            .into_iter()
            .map(str::to_owned)
            .collect(),
        capability_groups: if cfg!(windows) {
            vec![]
        } else {
            vec![
                "java_maven_dependencies_v1".into(),
                "java_maven_leaf_v1".into(),
            ]
        },
    };
    assert!(exact_capabilities(&info));
    assert_eq!(info.capabilities.len(), 32);
    let index = info
        .capabilities
        .iter()
        .position(|s| s == "language_java_signature_help")
        .unwrap();
    info.capabilities.remove(index);
    assert!(!exact_capabilities(&info));
    let initialize = serde_json::json!({"initialize":{"serverInfo":{"name":"JDT Language Server (Standard)","version":"1.61.0-SNAPSHOT"},"capabilities":{"signatureHelpProvider":{"triggerCharacters":["(",","]}}}});
    assert!(provider_supported(&initialize));
    for (pointer, bad) in [
        (
            "/initialize/serverInfo/name",
            serde_json::json!("JDT Language Server (Syntax)"),
        ),
        (
            "/initialize/serverInfo/version",
            serde_json::json!("unverified"),
        ),
        (
            "/initialize/capabilities/signatureHelpProvider",
            serde_json::json!(false),
        ),
        (
            "/initialize/capabilities/signatureHelpProvider",
            Value::Null,
        ),
    ] {
        let mut altered = initialize.clone();
        *altered.pointer_mut(pointer).unwrap() = bad;
        assert!(!provider_supported(&altered));
    }
}

#[test]
fn late_or_malformed_startup_keeps_ownership_until_verified_settlement() {
    let mut owner = Owner::new();
    owner.begin_sent = true;
    owner
        .observe(
            Call::Begin,
            &Payload::Language {
                value: serde_json::json!({"startup_id":7,"state":"starting"}),
            },
        )
        .unwrap();
    // Model the post-return clock failing after the actual ownership observer.
    assert!(within(STARTUP + Duration::from_nanos(1), STARTUP).is_err());
    assert_eq!(owner.startup_id, Some(7));
    assert!(!owner.all_terminal());
    owner
        .observe(
            Call::Poll,
            &Payload::Language {
                value: serde_json::json!({"startup_id":8,"state":"failed","cleanup_verified":true}),
            },
        )
        .unwrap();
    assert!(!owner.java_terminal);
    owner.observe(Call::Poll, &Payload::Language { value: serde_json::json!({"startup_id":7,"state":"ready","language":{"started":true}}) }).unwrap();
    assert!(owner.ready);
    assert!(owner.evidence.startup_terminal);
    assert!(!owner.java_terminal);
    owner.observe(Call::CleanupPoll, &Payload::Language { value: serde_json::json!({"startup_id":7,"state":"cancelled","cleanup_verified":false}) }).unwrap();
    assert!(!owner.all_terminal());
    owner.observe(Call::CleanupPoll, &Payload::Language { value: serde_json::json!({"startup_id":7,"state":"cancelled","cleanup_verified":true}) }).unwrap();
    assert!(owner.java_terminal);
    assert!(owner.all_terminal());
    // An attempted normal connect with no returned Client never gains a reap
    // witness from an error string or from terminal Java bookkeeping.
    owner.connect_attempted = true;
    assert!(!owner.all_terminal());
    owner.connect_returned = true;
    assert!(!owner.all_terminal());
    owner.evidence.client_reaped = true;
    assert!(owner.all_terminal());
}

#[test]
fn stop_quality_failure_does_not_erase_terminal_ownership_evidence() {
    let mut owner = Owner::new();
    owner.begin_sent = true;
    owner.evidence.startup_terminal = true;
    let shutdown = if cfg!(windows) {
        serde_json::json!({"status":"error","reason":"transport_failure","root_exit_code":17,"cleanup_joined":true,"shutdown_response_received":false,"exit_frame_completed":false})
    } else {
        serde_json::json!({"platform":"linux","status":"error","reason":"transport_failure","root_exit":{"kind":"signal","signal":15},"cleanup_joined":true,"shutdown_response_received":false,"exit_frame_completed":false})
    };
    let value = serde_json::json!({"stopped":true,"shutdown":shutdown});
    owner
        .observe(
            Call::Stop,
            &Payload::Language {
                value: value.clone(),
            },
        )
        .unwrap();
    assert!(owner.java_terminal);
    assert!(owner.evidence.stop_verified);
    assert_eq!(
        java_language::JavaStopOutcome::parse(&value)
            .unwrap()
            .status,
        java_language::StopStatus::Error
    );
    assert!(!owner.evidence.successful());
    for pointer in ["/shutdown/cleanup_joined", "/stopped"] {
        let mut altered = value.clone();
        *altered.pointer_mut(pointer).unwrap() = serde_json::json!(false);
        let mut fresh = Owner::new();
        fresh
            .observe(Call::Stop, &Payload::Language { value: altered })
            .unwrap();
        assert!(!fresh.java_terminal);
        assert!(!fresh.evidence.stop_verified);
    }
}

#[test]
fn receipt_is_bounded_fixed_scalars_and_saturation_never_means_success() {
    let mut evidence = Evidence::new();
    assert!(!evidence.successful());
    let encoded = serde_json::to_value(&evidence).unwrap();
    for (name, value) in encoded.as_object().unwrap() {
        assert!(
            value.is_boolean()
                || value.is_u64()
                || matches!(
                    name.as_str(),
                    "kind"
                        | "platform"
                        | "route"
                        | "status"
                        | "source_fixture_sha256"
                        | "failure_stage"
                )
        );
    }
    let text = encoded.to_string();
    assert!(text.len() < 8192);
    for secret in [
        SOURCE,
        SOURCE_PATH,
        PROJECT,
        LABELS[0],
        LABELS[1],
        "process_id",
        "startup_id",
        "protocol_response",
    ] {
        assert!(!text.contains(secret));
    }
    assert_eq!(
        elapsed_ms(
            ACTIVE + Duration::from_millis(1),
            ACTIVE,
            &mut evidence.elapsed_saturated
        ),
        570_000
    );
    assert!(evidence.elapsed_saturated);
    assert!(!evidence.successful());
}
