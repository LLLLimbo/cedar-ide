//! Shared, pure acceptance predicates and the real headless editor transaction.
//! Native Windows orchestration is feature-gated separately below.
#![allow(dead_code)]

use crate::{completion, editor_state, language_results, model::Document};
use eframe::egui;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{cell::Cell, time::Duration};

pub(super) const SOURCE: &str = "public class Main {\n    public static void main(String[] args) {\n        String greeting = \"Hello Cedar\";\n        System.out.println(greeting);\n        int broken = \"oops\";\n    }\n}\n";
pub(super) const PROJECT_DIR: &str = "project with spaces 雪";
pub(super) const SOURCE_PATH: &str = "project with spaces 雪/src/Main.java";
pub(super) const INITIAL_DATA_DIR: &str = "jdt data initial 雪";
pub(super) const RESTART_DATA_DIR: &str = "jdt data restart 雪";
pub(super) type CheckResult<T> = Result<T, String>;

#[path = "java_idle_acceptance_tests.rs"]
mod idle;

#[path = "java_organize_acceptance_tests.rs"]
mod organize;

#[path = "java_workspace_type_acceptance_tests.rs"]
mod workspace_types;

#[path = "java_implementations_acceptance_tests.rs"]
mod implementations;

pub(super) fn validate_fixture_layout(
    root: &std::path::Path,
    data: &std::path::Path,
) -> CheckResult<()> {
    let project = root.join(PROJECT_DIR);
    // Eclipse forbids project locations that contain its workspace (-data).
    // Both remain inside the synthetic agent root, as distinct sibling areas.
    if data.parent() != Some(root)
        || data.starts_with(&project)
        || project.starts_with(data)
        || root.join(SOURCE_PATH) != project.join("src/Main.java")
    {
        return Err("synthetic Java project and JDT data must be distinct siblings".into());
    }
    Ok(())
}

pub(super) fn corrected_source() -> String {
    SOURCE.replace("int broken = \"oops\";", "int correctedOnly = 42;")
}

pub(super) fn refreshed_source() -> String {
    corrected_source().replace("correctedOnly", "refreshedOnly")
}

/// Synthetic content witness, not a general freshness rule for unversioned
/// diagnostics. Earlier fixture versions never contain this identifier.
pub(super) fn refresh_diagnostics_match(value: &Value, uri: &str) -> Option<bool> {
    let actual_uri = value["uri"].as_str()?;
    if !same_local_uri(actual_uri, uri) {
        return None;
    }
    let mut parsed = language_results::Diagnostics::default();
    parsed.apply(value).ok()?;
    let batch = parsed.files.get(actual_uri)?;
    if batch.version.is_some_and(|version| version != 6)
        || batch.items.iter().any(|item| item.severity == 1)
        || !batch.items.iter().any(|item| {
            item.severity == 2
                && item.message.contains("refreshedOnly")
                && item.message.contains("not used")
                && item.range == marker_range(&refreshed_source(), "refreshedOnly")
        })
    {
        return None;
    }
    Some(batch.version.is_none())
}

#[test]
fn refresh_witness_requires_the_new_unsaved_identifier_and_preserves_unversioned_uncertainty() {
    let uri = "file:///project/Main.java";
    let range = marker_range(&refreshed_source(), "refreshedOnly");
    let batch = json!({"uri":uri,"diagnostics":[{
        "severity":2,"message":"The value of the local variable refreshedOnly is not used",
        "range":{"start":{"line":range.start.line,"character":range.start.character},
                 "end":{"line":range.end.line,"character":range.end.character}}
    }]});
    assert_eq!(refresh_diagnostics_match(&batch, uri), Some(true));
    let mut versioned = batch.clone();
    versioned["version"] = json!(6);
    assert_eq!(refresh_diagnostics_match(&versioned, uri), Some(false));
    versioned["version"] = json!(5);
    assert_eq!(refresh_diagnostics_match(&versioned, uri), None);
    let mut old = batch.clone();
    old["diagnostics"][0]["message"] = json!("correctedOnly is not used");
    assert_eq!(refresh_diagnostics_match(&old, uri), None);
    assert_eq!(
        refresh_diagnostics_match(&batch, "file:///other.java"),
        None
    );
    assert_eq!(
        refresh_diagnostics_match(&json!({"uri":uri,"diagnostics":[]}), uri),
        None
    );
}

pub(super) fn marker_range(source: &str, marker: &str) -> completion::Range {
    let start = source.find(marker).expect("synthetic source marker");
    completion::Range {
        start: completion::byte_to_position(source, start).unwrap(),
        end: completion::byte_to_position(source, start + marker.len()).unwrap(),
    }
}

// JDT legitimately returns file:/ with raw Unicode. Compare decoded local path
// bytes, rejecting authorities, query/fragment data, malformed escapes and NUL.
pub(super) fn same_local_uri(actual: &str, expected: &str) -> bool {
    fn path(uri: &str) -> Option<Vec<u8>> {
        let path = uri.strip_prefix("file:")?;
        let path = path.strip_prefix("//").unwrap_or(path);
        if !path.starts_with('/') || path.starts_with("//") || path.contains(['?', '#', '\\']) {
            return None;
        }
        let mut decoded = Vec::new();
        let mut bytes = path.bytes();
        while let Some(byte) = bytes.next() {
            decoded.push(if byte == b'%' {
                let high = (bytes.next()? as char).to_digit(16)?;
                let low = (bytes.next()? as char).to_digit(16)?;
                ((high << 4) | low) as u8
            } else {
                byte
            });
        }
        if decoded.contains(&0) {
            return None;
        }
        if decoded.get(2) == Some(&b':') && decoded.get(1).is_some_and(u8::is_ascii_alphabetic) {
            decoded[1].make_ascii_uppercase();
        }
        Some(decoded)
    }
    matches!((path(actual), path(expected)), (Some(a), Some(b)) if a == b)
}

pub(super) fn diagnostics_match(value: &Value, uri: &str, version: i32, corrected: bool) -> bool {
    let Some(actual_uri) = value["uri"].as_str() else {
        return false;
    };
    if !same_local_uri(actual_uri, uri) {
        return false;
    }
    let mut parsed = language_results::Diagnostics::default();
    if parsed.apply(value).is_err() {
        return false;
    }
    let Some(batch) = parsed.files.get(actual_uri) else {
        return false;
    };
    if batch.version.is_some_and(|v| v != version) {
        return false;
    }
    if corrected {
        !batch.items.iter().any(|d| d.severity == 1)
            && batch.items.iter().any(|d| {
                d.severity == 2
                    && d.message.contains("correctedOnly")
                    && d.message.contains("not used")
                    && d.range == marker_range(&corrected_source(), "correctedOnly")
            })
    } else {
        batch.items.iter().any(|d| {
            d.severity == 1
                && d.message.contains("cannot convert from String to int")
                && d.range == marker_range(SOURCE, "\"oops\"")
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum DiagnosticPhase {
    #[default]
    Initial,
    Correction,
}
impl DiagnosticPhase {
    pub fn version(self) -> i32 {
        match self {
            Self::Initial => 1,
            Self::Correction => 5,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum DiagnosticResult {
    Matched,
    Timeout,
    #[default]
    RequestError,
    MalformedEvents,
    Truncated,
    Lagged,
    Closed,
}

pub(super) struct DiagnosticWaitFailure {
    pub message: String,
    pub result: DiagnosticResult,
}
impl DiagnosticWaitFailure {
    /// The caller uses this only after the correction wait has already failed.
    /// A successful diagnostic probe never changes that original failure.
    pub fn with_timeout_probe(self, probe: impl FnOnce()) -> String {
        if self.result == DiagnosticResult::Timeout {
            probe();
        }
        self.message
    }
}

// A user-equivalent refresh may recover the workflow, but never the original
// spontaneous-push verdict. Admission covers the full request and witness wait;
// the existing aggregate watchdog does not guarantee a cleanup reserve.
pub(super) const CORRECTION_REFRESH_REQUEST_TIMEOUT: Duration = Duration::from_secs(75);
pub(super) const CORRECTION_REFRESH_DISPATCH_WINDOW: Duration = Duration::from_secs(15);
// Include one final in-flight event poll. Do not shorten RawAgent transport
// deadlines: its pipes must remain usable for normal owned session cleanup.
pub(super) const CORRECTION_REFRESH_BUDGET: Duration = Duration::from_secs(165);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum CorrectionRecoveryResult {
    #[default]
    NotAttempted,
    NotEligible,
    InsufficientBudget,
    RequestError,
    AcknowledgementMismatch,
    Timeout,
    MalformedEvents,
    Truncated,
    Lagged,
    Closed,
    Matched,
}

#[derive(Serialize)]
pub(super) struct CorrectionRecoveryEvidence {
    kind: &'static str,
    session: u32,
    original_result: DiagnosticResult,
    pub result: CorrectionRecoveryResult,
    pub attempts: u32,
    pub acknowledged: bool,
    pub witness: bool,
    pub unversioned: bool,
    pub budget_sufficient: bool,
    available_budget_ms: u32,
    required_budget_ms: u32,
    request_timeout_ms: u32,
    witness_dispatch_window_ms: u32,
    event_poll_timeout_ms: u32,
    cleanup_reserve_guaranteed: bool,
    elapsed_ms: u32,
    elapsed_saturated: bool,
    polls: u32,
    events: u32,
    counters_saturated: bool,
}
impl CorrectionRecoveryEvidence {
    pub fn new(session: u32, original_result: DiagnosticResult, available: Duration) -> Self {
        Self {
            kind: "windows_java_correction_recovery",
            session,
            original_result,
            result: CorrectionRecoveryResult::NotAttempted,
            attempts: 0,
            acknowledged: false,
            witness: false,
            unversioned: false,
            budget_sufficient: false,
            available_budget_ms: available.as_millis().min(360_000) as u32,
            required_budget_ms: CORRECTION_REFRESH_BUDGET.as_millis() as u32,
            request_timeout_ms: CORRECTION_REFRESH_REQUEST_TIMEOUT.as_millis() as u32,
            witness_dispatch_window_ms: CORRECTION_REFRESH_DISPATCH_WINDOW.as_millis() as u32,
            event_poll_timeout_ms: CORRECTION_REFRESH_REQUEST_TIMEOUT.as_millis() as u32,
            cleanup_reserve_guaranteed: false,
            elapsed_ms: 0,
            elapsed_saturated: false,
            polls: 0,
            events: 0,
            counters_saturated: false,
        }
    }

    pub fn finish_elapsed(&mut self, milliseconds: u128) {
        self.elapsed_ms = milliseconds.min(300_000) as u32;
        self.elapsed_saturated = milliseconds > 300_000;
    }

    /// Only the fixed typed refresh and read-only event polls are available in
    /// this workflow. In particular, no edit replay, save, or command bridge.
    pub fn run(
        &mut self,
        uri: &str,
        available: Duration,
        mut request: impl FnMut(cedar_protocol::Operation, Duration) -> CheckResult<Value>,
        mut elapsed: impl FnMut() -> Duration,
        mut pause: impl FnMut(Duration),
    ) -> CheckResult<()> {
        use cedar_protocol::Operation;
        if self.result != CorrectionRecoveryResult::NotAttempted {
            return Err("correction recovery may only be considered once".into());
        }
        if self.original_result != DiagnosticResult::Timeout {
            self.result = CorrectionRecoveryResult::NotEligible;
            return Err("only the original correction timeout permits recovery".into());
        }
        let started = elapsed();
        if available.saturating_sub(started) < CORRECTION_REFRESH_BUDGET {
            self.result = CorrectionRecoveryResult::InsufficientBudget;
            return Err("insufficient existing watchdog budget for one correction refresh".into());
        }
        self.budget_sufficient = true;
        let deadline = started + CORRECTION_REFRESH_BUDGET;
        self.result = CorrectionRecoveryResult::RequestError;
        self.attempts = 1;
        let acknowledgement = request(
            Operation::LanguageRefreshJavaDiagnostics {
                path: SOURCE_PATH.into(),
                version: 5,
            },
            CORRECTION_REFRESH_REQUEST_TIMEOUT,
        )?;
        if elapsed() >= deadline {
            self.result = CorrectionRecoveryResult::Timeout;
            return Err("correction recovery exceeded its existing budget".into());
        }
        if acknowledgement["version"] != 5
            || acknowledgement["notification_only"] != true
            || !acknowledgement["diagnostics_refresh_requested"]
                .as_str()
                .is_some_and(|actual| same_local_uri(actual, uri))
        {
            self.result = CorrectionRecoveryResult::AcknowledgementMismatch;
            return Err("version-5 correction refresh acknowledgement mismatch".into());
        }
        self.acknowledged = true;
        let dispatch_deadline = elapsed() + CORRECTION_REFRESH_DISPATCH_WINDOW;
        let mut diagnostics = DiagnosticEvidence::new(self.session, DiagnosticPhase::Correction);
        loop {
            let now = elapsed();
            if now >= dispatch_deadline
                || deadline.saturating_sub(now) < CORRECTION_REFRESH_REQUEST_TIMEOUT
            {
                self.result = CorrectionRecoveryResult::Timeout;
                return Err(
                    "single correction refresh produced no exact current-source warning".into(),
                );
            }
            self.result = CorrectionRecoveryResult::RequestError;
            diagnostics.begin_poll();
            self.polls = diagnostics.polls;
            self.counters_saturated = diagnostics.counters_saturated;
            // Admission reserves this entire final poll, including when it
            // finishes after the dispatch window. The wall deadline stays fixed.
            let response = request(
                Operation::LanguageEvents,
                CORRECTION_REFRESH_REQUEST_TIMEOUT,
            )?;
            if elapsed() >= deadline {
                self.result = CorrectionRecoveryResult::Timeout;
                return Err("correction refresh witness arrived after its deadline".into());
            }
            let matched = diagnostics.inspect_response(&response, uri);
            self.events = diagnostics.events;
            self.counters_saturated = diagnostics.counters_saturated;
            match matched {
                Ok(true) => {
                    self.unversioned = response["events"]
                        .as_array()
                        .expect("validated event array")
                        .iter()
                        .find(|event| {
                            event["type"] == "diagnostics"
                                && diagnostics_match(&event["value"], uri, 5, true)
                        })
                        .expect("validated exact correction witness")["value"]["version"]
                        .is_null();
                    self.witness = true;
                    self.result = CorrectionRecoveryResult::Matched;
                    return Ok(());
                }
                Ok(false) => {}
                Err(result) => {
                    self.result = match result {
                        DiagnosticResult::MalformedEvents => {
                            CorrectionRecoveryResult::MalformedEvents
                        }
                        DiagnosticResult::Truncated => CorrectionRecoveryResult::Truncated,
                        DiagnosticResult::Lagged => CorrectionRecoveryResult::Lagged,
                        DiagnosticResult::Closed => CorrectionRecoveryResult::Closed,
                        _ => CorrectionRecoveryResult::RequestError,
                    };
                    return Err("correction recovery event stream failed; see typed receipt".into());
                }
            }
            pause(Duration::from_millis(100).min(dispatch_deadline.saturating_sub(elapsed())));
        }
    }
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum CorrectionHoverResult {
    Matched,
    NoMatch,
    RequestError,
}

/// Failure-only observation of the unique version-5 variable. Never stores the
/// hover payload or error text, and is not an alternative diagnostics verdict.
#[derive(Serialize)]
pub(super) struct CorrectionHoverEvidence {
    kind: &'static str,
    session: u32,
    result: CorrectionHoverResult,
    elapsed_ms: u32,
    elapsed_saturated: bool,
}
impl CorrectionHoverEvidence {
    pub fn observed(session: u32, outcome: &CheckResult<Value>, milliseconds: u128) -> Self {
        Self {
            kind: "windows_java_correction_hover",
            session,
            result: match outcome {
                Ok(value) if hover_has_tokens(value, &["int", "correctedOnly"]) => {
                    CorrectionHoverResult::Matched
                }
                Ok(_) => CorrectionHoverResult::NoMatch,
                Err(_) => CorrectionHoverResult::RequestError,
            },
            elapsed_ms: milliseconds.min(300_000) as u32,
            elapsed_saturated: milliseconds > 300_000,
        }
    }
}

const DIAGNOSTIC_COUNTER_LIMIT: u32 = 65_535;
fn add_diagnostic_count(counter: &mut u32, saturated: &mut bool, amount: usize) {
    let remaining = (DIAGNOSTIC_COUNTER_LIMIT - *counter) as usize;
    if amount > remaining {
        *counter = DIAGNOSTIC_COUNTER_LIMIT;
        *saturated = true;
    } else {
        *counter += amount as u32;
    }
}

/// One fixed-size receipt per await. Counts classify the exact predicate without
/// retaining or printing any server message, URI, source text or arbitrary error.
#[derive(Default, Serialize)]
pub(super) struct DiagnosticEvidence {
    kind: &'static str,
    session: u32,
    phase: DiagnosticPhase,
    pub result: DiagnosticResult,
    elapsed_ms: u32,
    elapsed_saturated: bool,
    polls: u32,
    events: u32,
    diagnostic_batches: u32,
    uri_match_batches: u32,
    parsed_batches: u32,
    version_match_batches: u32,
    unversioned_batches: u32,
    eligible_batches: u32,
    eligible_empty_batches: u32,
    eligible_error_free_batches: u32,
    error_diagnostics: u32,
    warning_diagnostics: u32,
    expected_message_diagnostics: u32,
    expected_severity_diagnostics: u32,
    expected_range_diagnostics: u32,
    expected_joint_diagnostics: u32,
    eligible_expected_joint_diagnostics: u32,
    eligible_error_diagnostics: u32,
    matching_batches: u32,
    counters_saturated: bool,
}
impl DiagnosticEvidence {
    pub fn new(session: u32, phase: DiagnosticPhase) -> Self {
        Self {
            kind: "windows_java_diagnostics",
            session,
            phase,
            ..Self::default()
        }
    }
    pub fn finish_elapsed(&mut self, milliseconds: u128) {
        self.elapsed_ms = milliseconds.min(300_000) as u32;
        self.elapsed_saturated = milliseconds > 300_000;
    }
    pub fn begin_poll(&mut self) {
        self.result = DiagnosticResult::RequestError;
        add_diagnostic_count(&mut self.polls, &mut self.counters_saturated, 1);
    }
    pub fn inspect_response(
        &mut self,
        response: &Value,
        expected_uri: &str,
    ) -> Result<bool, DiagnosticResult> {
        let result = (|| {
            match response.get("truncated").and_then(Value::as_bool) {
                Some(false) => {}
                Some(true) => return Err(DiagnosticResult::Truncated),
                None => return Err(DiagnosticResult::MalformedEvents),
            }
            let events = response["events"]
                .as_array()
                .ok_or(DiagnosticResult::MalformedEvents)?;
            let mut matched = false;
            for event in events {
                add_diagnostic_count(&mut self.events, &mut self.counters_saturated, 1);
                match event["type"].as_str() {
                    Some("diagnostics") => {
                        matched |= self.observe_batch(&event["value"], expected_uri);
                    }
                    Some("notification" | "unsupported_server_request") => {}
                    Some("lagged") => return Err(DiagnosticResult::Lagged),
                    Some("closed") => return Err(DiagnosticResult::Closed),
                    _ => return Err(DiagnosticResult::MalformedEvents),
                }
            }
            Ok(matched)
        })();
        match result {
            Ok(true) => self.result = DiagnosticResult::Matched,
            Err(category) => self.result = category,
            Ok(false) => {}
        }
        result
    }
    fn observe_batch(&mut self, value: &Value, expected_uri: &str) -> bool {
        macro_rules! count {
            ($field:ident, $amount:expr) => {
                add_diagnostic_count(&mut self.$field, &mut self.counters_saturated, $amount)
            };
        }
        count!(diagnostic_batches, 1);
        let uri_matches = value["uri"]
            .as_str()
            .is_some_and(|uri| same_local_uri(uri, expected_uri));
        count!(uri_match_batches, usize::from(uri_matches));
        let mut parsed = language_results::Diagnostics::default();
        if parsed.apply(value).is_err() {
            return false;
        }
        let Some(batch) = parsed.files.values().next() else {
            return false;
        };
        count!(parsed_batches, 1);
        let version_matches = batch.version.is_none_or(|v| v == self.phase.version());
        count!(version_match_batches, usize::from(version_matches));
        count!(unversioned_batches, usize::from(batch.version.is_none()));
        let eligible = uri_matches && version_matches;
        count!(eligible_batches, usize::from(eligible));
        count!(
            eligible_empty_batches,
            usize::from(eligible && batch.items.is_empty())
        );
        let error_count = batch.items.iter().filter(|item| item.severity == 1).count();
        count!(error_diagnostics, error_count);
        count!(
            warning_diagnostics,
            batch.items.iter().filter(|item| item.severity == 2).count()
        );
        if eligible {
            count!(eligible_error_diagnostics, error_count);
            count!(eligible_error_free_batches, usize::from(error_count == 0));
        }
        let corrected = self.phase == DiagnosticPhase::Correction;
        let expected_range = if corrected {
            marker_range(&corrected_source(), "correctedOnly")
        } else {
            marker_range(SOURCE, "\"oops\"")
        };
        let expected_severity = if corrected { 2 } else { 1 };
        for item in &batch.items {
            let message_matches = if corrected {
                item.message.contains("correctedOnly") && item.message.contains("not used")
            } else {
                item.message.contains("cannot convert from String to int")
            };
            let severity_matches = item.severity == expected_severity;
            let range_matches = item.range == expected_range;
            count!(expected_message_diagnostics, usize::from(message_matches));
            count!(expected_severity_diagnostics, usize::from(severity_matches));
            count!(expected_range_diagnostics, usize::from(range_matches));
            let joint = message_matches && severity_matches && range_matches;
            count!(expected_joint_diagnostics, usize::from(joint));
            count!(
                eligible_expected_joint_diagnostics,
                usize::from(eligible && joint)
            );
        }
        // Acceptance remains the original exact predicate, not any aggregate
        // count or combination of partial witnesses from different messages.
        let matched = diagnostics_match(value, expected_uri, self.phase.version(), corrected);
        count!(matching_batches, usize::from(matched));
        matched
    }
}

pub(super) fn exact_definition(value: &Value, uri: &str) -> CheckResult<String> {
    let locations = language_results::parse_definitions(value)?;
    if locations.len() != 1
        || !same_local_uri(&locations[0].uri, uri)
        || locations[0].range != marker_range(SOURCE, "greeting")
    {
        return Err("definition did not identify the exact source declaration".into());
    }
    Ok(locations[0].uri.clone())
}

/// Exercise CedarApp's actual resolve acceptance and Document Undo/Redo. The
/// callback synchronizes these exact buffers, never an Undoer clone or inferred
/// replacement text. No OS window, Client capability bypass or trust action.
pub(super) fn editor_transaction(
    original: Value,
    resolved: Value,
    stage: &Cell<FailureStage>,
    mut synchronized: impl FnMut(i32, &Document) -> CheckResult<()>,
) -> CheckResult<()> {
    use crate::language_ui::{Action, ActionKind, QueryContext};
    stage.set(FailureStage::Apply);
    crate::language_ui::validate_resolved_identity(&original, &resolved)?;
    let byte = SOURCE.rfind("greeting").ok_or("missing reference")? + 3;
    let cursor = completion::byte_to_position(SOURCE, byte)?;
    let before_cursor = SOURCE[..byte].chars().count();
    let applied = completion::apply_completion(SOURCE, cursor, &resolved)?;
    if applied.edit_count != 2
        || !applied.skipped_advisory
        || !applied
            .text
            .starts_with("import java.util.GregorianCalendar;\n")
        || !applied
            .text
            .contains("System.out.println(GregorianCalendar);")
    {
        return Err("completion was not an atomic primary edit plus deferred import".into());
    }
    let mut app = crate::CedarApp::empty();
    app.language.running = true;
    app.language.session = 1;
    app.language.acceptance_sequence = 1;
    let mut doc = Document::new(
        1,
        SOURCE_PATH.into(),
        SOURCE.into(),
        "synthetic-original".into(),
    );
    doc.cursor = crate::model::cursor_location(SOURCE, before_cursor);
    let mut state = editor_state::load(&app.editor_ctx, &mut doc);
    state
        .cursor
        .set_char_range(Some(egui::text::CCursorRange::one(
            egui::text::CCursor::new(before_cursor),
        )));
    state.store(&app.editor_ctx, egui::Id::new(("editor", 1u64)));
    app.documents.push(doc);
    app.active_document = Some(1);
    app.apply_language_action(
        Action {
            session: 1,
            kind: ActionKind::ResolveCompletion {
                context: QueryContext {
                    session: 1,
                    document: 1,
                    edit_version: 0,
                    source: SOURCE.into(),
                    cursor,
                },
                original,
                acceptance: 1,
            },
        },
        resolved,
    );
    if let Some(error) = app.error.take() {
        return Err(error);
    }
    check_document(&mut app, &applied.text, applied.cursor_chars, 1, true)?;
    stage.set(FailureStage::Sync);
    synchronized(2, &app.documents[0])?;
    stage.set(FailureStage::Undo);
    history_key(&mut app, false);
    check_document(&mut app, SOURCE, before_cursor, 2, false)?;
    stage.set(FailureStage::Sync);
    synchronized(3, &app.documents[0])?;
    stage.set(FailureStage::Redo);
    history_key(&mut app, true);
    check_document(&mut app, &applied.text, applied.cursor_chars, 3, true)?;
    stage.set(FailureStage::Sync);
    synchronized(4, &app.documents[0])?;
    Ok(())
}

fn check_document(
    app: &mut crate::CedarApp,
    expected_text: &str,
    expected_cursor: usize,
    version: u64,
    dirty: bool,
) -> CheckResult<()> {
    let doc = &mut app.documents[0];
    let state = editor_state::load(&app.editor_ctx, doc);
    let expected_range = egui::text::CCursorRange::one(egui::text::CCursor::new(expected_cursor));
    if doc.text != expected_text
        || doc.cursor != crate::model::cursor_location(expected_text, expected_cursor)
        || state.cursor.char_range() != Some(expected_range)
        || doc.edit_version != version
        || doc.dirty() != dirty
        || doc.saved_text != SOURCE
        || doc.revision.as_deref() != Some("synthetic-original")
    {
        return Err("actual Document text/cursor/version/dirty/history invariant failed".into());
    }
    Ok(())
}

fn history_key(app: &mut crate::CedarApp, redo: bool) {
    let ctx = app.editor_ctx.clone();
    let _ = ctx.run(
        egui::RawInput {
            events: vec![egui::Event::Key {
                key: egui::Key::Z,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: if redo {
                    egui::Modifiers::COMMAND | egui::Modifiers::SHIFT
                } else {
                    egui::Modifiers::COMMAND
                },
            }],
            ..Default::default()
        },
        |ctx| {
            ctx.memory_mut(|memory| memory.request_focus(egui::Id::new(("editor", 1u64))));
            editor_state::history_shortcut(ctx, &mut app.documents[0]);
        },
    );
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub(super) enum AgentEvidence {
    #[serde(rename = "agent_java_started")]
    Started {
        session: u32,
        pid: u32,
        creation_time_100ns_since_1601: u64,
    },
    #[serde(rename = "agent_java_stopped")]
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

pub(super) fn parse_evidence(bytes: &[u8]) -> CheckResult<Vec<AgentEvidence>> {
    if bytes.len() > 32 * 1024 || !bytes.ends_with(b"\n") {
        return Err("invalid bounded Java lifecycle evidence".into());
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "non-UTF-8 lifecycle evidence")?;
    let lines: Vec<_> = text.lines().collect();
    if lines.is_empty() || lines.len() > 32 {
        return Err("invalid lifecycle record count".into());
    }
    lines
        .into_iter()
        .map(|line| {
            serde_json::from_str(line).map_err(|_| "invalid typed Java lifecycle record".into())
        })
        .collect()
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum SessionMode {
    Initial,
    FreshData,
    ReusedData,
}

#[derive(Clone, Copy, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum CorrectionChangeResult {
    #[default]
    NotAttempted,
    RequestError,
    AcknowledgementMismatch,
    Acknowledged,
}

#[derive(Serialize)]
pub(super) struct SessionEvidence {
    kind: &'static str,
    pub session: u32,
    mode: SessionMode,
    pub initialization_ms: u64,
    pub semantic_checks_passed: bool,
    pub exact_diagnostics: bool,
    pub exact_definition: bool,
    pub real_completion: bool,
    pub deferred_import_resolve: bool,
    pub primary_identity_unchanged: bool,
    pub two_atomic_edits: bool,
    pub advisory_command_skipped: bool,
    pub actual_undo: bool,
    pub actual_redo: bool,
    pub versions_2_3_4_synced: bool,
    pub correction_change_result: CorrectionChangeResult,
    pub correction_change_acknowledged: bool,
    pub correction_diagnostics: bool,
    pub correction_recovery_result: CorrectionRecoveryResult,
    pub correction_recovery_attempts: u32,
    pub correction_recovery_acknowledged: bool,
    pub correction_recovery_witness: bool,
    pub correction_recovery_unversioned: bool,
    pub correction_recovery_budget_sufficient: bool,
    pub workflow_success: bool,
    pub source_unchanged: bool,
    pub root_observed_live: bool,
    pub root_identity_verified: bool,
    pub jdk_symbol_verified: bool,
    pub shutdown_api_succeeded: bool,
    pub root_handle_signaled: bool,
    pub root_exit_code: Option<u32>,
    pub gracefully_exited: bool,
    pub shutdown_elapsed_ms: u64,
}
impl SessionEvidence {
    pub fn record_correction_recovery(&mut self, receipt: &CorrectionRecoveryEvidence) {
        self.correction_recovery_result = receipt.result;
        self.correction_recovery_attempts = receipt.attempts;
        self.correction_recovery_acknowledged = receipt.acknowledged;
        self.correction_recovery_witness = receipt.witness;
        self.correction_recovery_unversioned = receipt.unversioned;
        self.correction_recovery_budget_sufficient = receipt.budget_sufficient;
    }

    pub fn new(session: u32, mode: SessionMode) -> Self {
        Self {
            kind: "windows_java_session",
            session,
            mode,
            initialization_ms: 0,
            semantic_checks_passed: false,
            exact_diagnostics: false,
            exact_definition: false,
            real_completion: false,
            deferred_import_resolve: false,
            primary_identity_unchanged: false,
            two_atomic_edits: false,
            advisory_command_skipped: false,
            actual_undo: false,
            actual_redo: false,
            versions_2_3_4_synced: false,
            correction_change_result: CorrectionChangeResult::NotAttempted,
            correction_change_acknowledged: false,
            correction_diagnostics: false,
            correction_recovery_result: CorrectionRecoveryResult::NotAttempted,
            correction_recovery_attempts: 0,
            correction_recovery_acknowledged: false,
            correction_recovery_witness: false,
            correction_recovery_unversioned: false,
            correction_recovery_budget_sufficient: false,
            workflow_success: false,
            source_unchanged: false,
            root_observed_live: false,
            root_identity_verified: false,
            jdk_symbol_verified: false,
            shutdown_api_succeeded: false,
            root_handle_signaled: false,
            root_exit_code: None,
            gracefully_exited: false,
            shutdown_elapsed_ms: 0,
        }
    }
}

#[derive(Clone, Copy, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum FailureStage {
    #[default]
    None,
    Setup,
    Initialize,
    Open,
    Diagnostics,
    Hover,
    Definition,
    Completion,
    Resolve,
    Apply,
    Undo,
    Redo,
    Sync,
    Correction,
    Close,
    Stop,
    RootExit,
    AgentExit,
    FixtureCleanup,
}

#[derive(Serialize)]
pub(super) struct CleanupEvidence {
    pub kind: &'static str,
    pub sessions_completed: u32,
    pub agent_exit_zero: bool,
    pub source_unchanged: bool,
    pub observed_roots_exited: bool,
    pub synthetic_root_removed: bool,
    pub failure_stage: FailureStage,
    pub primary_failed: bool,
    pub cleanup_failed: bool,
    pub spontaneous_success: bool,
    pub workflow_success: bool,
    pub success: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum OwnershipStage {
    #[default]
    None,
    Setup,
    Initialize,
    Open,
    Diagnostics,
    Hover,
    TaskStart,
    TaskIdentity,
    LanguageStop,
    TaskSurvival,
    TaskCancel,
    JavaSurvival,
    OwnerDeath,
    AgentExit,
    JavaExit,
    TaskExit,
    Source,
    FixtureCleanup,
}

#[derive(Default, Serialize)]
pub(super) struct ConcurrencyEvidence {
    pub kind: &'static str,
    pub tasks_started: u32,
    pub tasks_completed: u32,
    pub language_stop_preserved_task: bool,
    pub task_cancel_preserved_java: bool,
    pub hover_after_cancel: bool,
    pub task_identities_verified: bool,
    pub task_locks_verified: bool,
    pub tasks_exited: bool,
    pub task_locks_released: bool,
    pub task_caps_not_reached: bool,
    pub source_unchanged: bool,
    pub primary_failed: bool,
    pub cleanup_failed: bool,
    pub success: bool,
    pub failure_stage: OwnershipStage,
}
impl ConcurrencyEvidence {
    pub fn new() -> Self {
        Self {
            kind: "windows_java_concurrency",
            ..Self::default()
        }
    }
}

#[derive(Default, Serialize)]
pub(super) struct ForcedCleanupEvidence {
    pub kind: &'static str,
    pub java_observed_live: bool,
    pub java_identity_verified: bool,
    pub task_observed_live: bool,
    pub task_identity_verified: bool,
    pub task_lock_verified: bool,
    pub owner_death_injected: bool,
    pub agent_exit_observed: bool,
    pub agent_exit_nonzero: bool,
    pub java_exit_observed: bool,
    pub task_exit_observed: bool,
    pub task_lock_released: bool,
    pub task_cap_not_reached: bool,
    pub source_unchanged: bool,
    pub synthetic_root_removed: bool,
    pub primary_failed: bool,
    pub cleanup_failed: bool,
    pub success: bool,
    pub java_exit_code: Option<u32>,
    pub task_exit_code: Option<u32>,
    pub failure_stage: OwnershipStage,
    pub elapsed_ms: u32,
    pub elapsed_saturated: bool,
}
impl ForcedCleanupEvidence {
    pub fn new() -> Self {
        Self {
            kind: "windows_java_forced_cleanup",
            ..Self::default()
        }
    }
}

pub(super) fn parse_task_identity(bytes: &[u8]) -> CheckResult<(u32, u64)> {
    if bytes.len() > 64 || !bytes.ends_with(b"\n") {
        return Err("invalid bounded task identity record".into());
    }
    let text = std::str::from_utf8(&bytes[..bytes.len() - 1])
        .map_err(|_| "task identity must contain ASCII decimal fields")?;
    let (pid, created) = text
        .split_once(' ')
        .ok_or("task identity requires two fields")?;
    if pid.is_empty()
        || created.is_empty()
        || !pid.bytes().all(|b| b.is_ascii_digit())
        || !created.bytes().all(|b| b.is_ascii_digit())
    {
        return Err("task identity requires two decimal fields".into());
    }
    let pid = pid.parse::<u32>().map_err(|_| "task PID exceeds bounds")?;
    let created = created
        .parse::<u64>()
        .map_err(|_| "task creation time exceeds bounds")?;
    if pid == 0 || created == 0 {
        return Err("task identity fields must be nonzero".into());
    }
    Ok((pid, created))
}

pub(super) fn hover_has_source_variable(value: &Value) -> bool {
    hover_has_tokens(value, &["String", "greeting"])
}

fn hover_has_tokens(value: &Value, expected: &[&str]) -> bool {
    let Some(contents) = value.get("contents") else {
        return false;
    };
    let text = language_results::hover_text(&json!({"contents": contents}));
    expected.iter().all(|expected| {
        text.split(|ch: char| !ch.is_alphanumeric() && ch != '_' && ch != '$')
            .any(|word| word == *expected)
    })
}

#[test]
fn correction_hover_requires_the_unique_corrected_variable_and_never_retains_payloads() {
    for contents in [
        json!("int correctedOnly"),
        json!({"kind":"markdown","value":"```java\nint correctedOnly\n```"}),
        json!([{"language":"java","value":"int correctedOnly"}]),
    ] {
        let receipt = CorrectionHoverEvidence::observed(
            2,
            &Ok(json!({"contents":contents,"private":"private payload sentinel"})),
            12,
        );
        assert_eq!(receipt.result, CorrectionHoverResult::Matched);
        let serialized = serde_json::to_value(&receipt).unwrap();
        assert_eq!(serialized.as_object().unwrap().len(), 5);
        assert_eq!(serialized["elapsed_ms"], 12);
        assert!(!serialized.to_string().contains("private"));
    }
    for value in [
        Value::Null,
        json!({"contents":null}),
        json!({"contents":"String greeting"}),
        json!({"contents":"int broken"}),
        json!({"contents":"print correctedOnly"}),
        json!({"contents":"int correctedOnlyExtra"}),
        json!({"contents":"int $correctedOnly"}),
        json!({"private":"int correctedOnly"}),
    ] {
        assert_eq!(
            CorrectionHoverEvidence::observed(2, &Ok(value), 0).result,
            CorrectionHoverResult::NoMatch
        );
    }
    let receipt =
        CorrectionHoverEvidence::observed(2, &Err("private error sentinel".into()), u128::MAX);
    assert_eq!(receipt.result, CorrectionHoverResult::RequestError);
    assert_eq!(receipt.elapsed_ms, 300_000);
    assert!(receipt.elapsed_saturated);
    assert!(!serde_json::to_string(&receipt).unwrap().contains("private"));
}

#[test]
fn correction_probe_runs_only_after_timeout_and_cannot_replace_the_original_failure() {
    for result in [
        DiagnosticResult::Timeout,
        DiagnosticResult::RequestError,
        DiagnosticResult::MalformedEvents,
        DiagnosticResult::Truncated,
        DiagnosticResult::Lagged,
        DiagnosticResult::Closed,
    ] {
        let mut probes = 0;
        let failure: Result<(), DiagnosticWaitFailure> = Err(DiagnosticWaitFailure {
            message: "original correction failure".into(),
            result,
        });
        let outcome = failure.map_err(|failure| failure.with_timeout_probe(|| probes += 1));
        assert_eq!(outcome, Err("original correction failure".into()));
        assert_eq!(probes, usize::from(result == DiagnosticResult::Timeout));
    }
    let matched: Result<(), DiagnosticWaitFailure> = Ok(());
    assert_eq!(
        matched.map_err(|failure| failure.with_timeout_probe(|| panic!("probe after success"))),
        Ok(())
    );
}

#[test]
fn task_identity_requires_exact_bounded_nonzero_decimal_fields() {
    assert_eq!(
        parse_task_identity(b"42 134359011488974354\n").unwrap(),
        (42, 134359011488974354)
    );
    for invalid in [
        b"".as_slice(),
        b"42 1",
        b"0 1\n",
        b"1 0\n",
        b"+1 2\n",
        b"1  2\n",
        b"1 2 3\n",
        b"1 2\r\n",
        b"4294967296 1\n",
        b"1 18446744073709551616\n",
    ] {
        assert!(parse_task_identity(invalid).is_err());
    }
    assert!(parse_task_identity(&[b'1'; 65]).is_err());
}

#[test]
fn hover_witness_requires_actual_symbol_and_type_tokens() {
    for contents in [
        json!("String greeting"),
        json!({"kind":"markdown","value":"String greeting"}),
        json!([{"language":"java","value":"String greeting"}]),
    ] {
        assert!(hover_has_source_variable(&json!({"contents":contents})));
    }
    for value in [
        Value::Null,
        json!({}),
        json!({"contents":""}),
        json!({"contents":"Strings greeting"}),
        json!({"contents":"String greetingSuffix"}),
        json!({"contents":"int greeting"}),
    ] {
        assert!(!hover_has_source_variable(&value));
    }
}

#[derive(Clone, Copy, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ProductionStopStatus {
    #[default]
    NotAttempted,
    Graceful,
    Forced,
    Error,
}
#[derive(Clone, Copy, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ProductionStopReason {
    #[default]
    NotAttempted,
    RootExited,
    GraceExpired,
    Aborted,
    TransportFailure,
    WorkerPanicked,
}
#[derive(Default, Serialize)]
pub(super) struct ProductionEvidence {
    pub kind: &'static str,
    pub route: &'static str,
    pub java_capabilities: bool,
    pub async_start_exercised: bool,
    pub async_start_begin_acknowledged: bool,
    pub async_start_read_while_starting: bool,
    pub async_start_ready: bool,
    pub generic_start_rejected: bool,
    pub untrusted_start_rejected: bool,
    pub root_observed_live: bool,
    pub root_identity_verified: bool,
    pub semantic_diagnostics: bool,
    pub exact_definition: bool,
    pub real_completion: bool,
    pub deferred_import_resolve: bool,
    pub actual_editor_apply_undo_redo: bool,
    pub versions_2_3_4_synced: bool,
    pub correction_acknowledged: bool,
    pub correction_diagnostics: bool,
    pub diagnostics_refresh_exercised: bool,
    pub diagnostics_refresh_supported: bool,
    pub diagnostics_refresh_requested: bool,
    pub diagnostics_refresh_witness: bool,
    pub diagnostics_refresh_unversioned: bool,
    pub source_unchanged: bool,
    pub stop_outcome_verified: bool,
    pub shutdown_response_received: bool,
    pub exit_frame_completed: bool,
    pub cleanup_joined: bool,
    pub root_handle_signaled: bool,
    pub client_reaped: bool,
    pub synthetic_root_removed: bool,
    pub primary_failed: bool,
    pub cleanup_failed: bool,
    pub success: bool,
    pub stop_status: ProductionStopStatus,
    pub stop_reason: ProductionStopReason,
    pub root_exit_code: Option<u32>,
    pub failure_stage: FailureStage,
    pub elapsed_ms: u32,
    pub elapsed_saturated: bool,
}
impl ProductionEvidence {
    pub fn new() -> Self {
        Self {
            kind: "windows_java_production",
            route: "normal_agent_client",
            ..Self::default()
        }
    }
}

#[cfg(windows)]
#[path = "windows_java_agent_tests.rs"]
mod windows;

#[test]
fn generated_java_layout_keeps_project_and_server_data_separate() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path();
    for name in [INITIAL_DATA_DIR, RESTART_DATA_DIR] {
        let data = root.join(name);
        validate_fixture_layout(root, &data).unwrap();
        assert!(!root.join(SOURCE_PATH).starts_with(&data));
    }
    for invalid in [
        root.to_path_buf(),
        root.join(PROJECT_DIR),
        root.join(PROJECT_DIR).join(INITIAL_DATA_DIR),
    ] {
        assert!(validate_fixture_layout(root, &invalid).is_err());
    }
}

#[test]
fn exact_java_witnesses_reject_unrelated_stale_and_empty_results() {
    const URI: &str = "file:///C:/fixture%20%E9%9B%AA/src/Main.java";
    let range = |range: completion::Range| json!({"start":{"line":range.start.line,"character":range.start.character},"end":{"line":range.end.line,"character":range.end.character}});
    let mut initial = json!({"uri":URI,"version":1,"diagnostics":[{"severity":1,"message":"Type mismatch: cannot convert from String to int","range":range(marker_range(SOURCE,"\"oops\""))}]});
    assert!(diagnostics_match(&initial, URI, 1, false));
    initial["version"] = json!(2);
    assert!(!diagnostics_match(&initial, URI, 1, false));
    initial["version"] = Value::Null;
    initial["diagnostics"][0]["range"]["start"]["character"] = json!(0);
    assert!(!diagnostics_match(&initial, URI, 1, false));
    let mut corrected = json!({"uri":URI,"diagnostics":[{"severity":2,"message":"The value of the local variable correctedOnly is not used","range":range(marker_range(&corrected_source(),"correctedOnly"))}]});
    assert!(diagnostics_match(&corrected, URI, 5, true));
    corrected["diagnostics"] = json!([]);
    assert!(!diagnostics_match(&corrected, URI, 5, true));
    let definition = json!({"uri":"file:/c:/fixture%20雪/src/Main.java","range":range(marker_range(SOURCE,"greeting"))});
    assert!(exact_definition(&definition, URI).is_ok());
    assert!(exact_definition(&definition, "file:///C:/other/Main.java").is_err());
    for invalid in [
        "file://host/C:/fixture%20雪/src/Main.java",
        "file:///C:/fixture%20雪/src/Main.java#fragment",
        "file:///C:/fixture%20雪/src/Main.java%00",
        "file:///C:/fixture%ZZ雪/src/Main.java",
    ] {
        assert!(!same_local_uri(invalid, URI));
    }
}

#[test]
fn real_jdt_capture_drives_actual_document_history_and_versions() {
    let captured: Value = serde_json::from_str(
        include_str!("../../language/tests/evidence/jdtls-1.61.0-completion-resolve.jsonl").trim(),
    )
    .unwrap();
    let mut versions = Vec::new();
    editor_transaction(
        captured["input"].clone(),
        captured["payload"].clone(),
        &Cell::new(FailureStage::Apply),
        |version, doc| {
            versions.push((version, doc.edit_version, doc.dirty()));
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(versions, [(2, 1, true), (3, 2, false), (4, 3, true)]);
    let mut changed = captured["payload"].clone();
    changed["textEdit"]["newText"] = json!("different");
    let mut called = false;
    assert!(editor_transaction(
        captured["input"].clone(),
        changed,
        &Cell::new(FailureStage::Apply),
        |_, _| {
            called = true;
            Ok(())
        }
    )
    .is_err());
    assert!(
        !called,
        "identity replacement must be rejected before buffer synchronization"
    );
}

#[test]
fn lifecycle_evidence_is_typed_bounded_and_requires_complete_lines() {
    let line = b"{\"kind\":\"agent_java_started\",\"session\":1,\"pid\":7,\"creation_time_100ns_since_1601\":1}\n";
    assert!(parse_evidence(line).is_ok());
    assert!(parse_evidence(&line[..line.len() - 1]).is_err());
    assert!(parse_evidence(&line.repeat(33)).is_err());
    assert!(parse_evidence(b"{\"kind\":\"private_raw_protocol\"}\n").is_err());
    assert!(parse_evidence(b"{\"kind\":\"agent_java_started\",\"session\":1,\"pid\":7,\"creation_time_100ns_since_1601\":1,\"raw\":\"private\"}\n").is_err());
}

fn diagnostic_fixture(phase: DiagnosticPhase, version: Option<i32>, uri: &str) -> Value {
    let (range, severity, message) = match phase {
        DiagnosticPhase::Initial => (
            marker_range(SOURCE, "\"oops\""),
            1,
            "Type mismatch: cannot convert from String to int",
        ),
        DiagnosticPhase::Correction => (
            marker_range(&corrected_source(), "correctedOnly"),
            2,
            "The value of the local variable correctedOnly is not used",
        ),
    };
    json!({"uri":uri,"version":version,"diagnostics":[{"severity":severity,"message":message,"range":{"start":{"line":range.start.line,"character":range.start.character},"end":{"line":range.end.line,"character":range.end.character}}}]})
}
fn diagnostic_events(batch: Value) -> Value {
    json!({"truncated":false,"events":[{"type":"diagnostics","value":batch}]})
}

fn correction_refresh_ack(uri: &str) -> Value {
    json!({"diagnostics_refresh_requested":uri,"version":5,"notification_only":true})
}

#[test]
fn correction_recovery_keeps_the_original_timeout_and_uses_one_typed_refresh() {
    use cedar_protocol::Operation;
    let uri = "file:///private-fixture/Main.java";
    for version in [Some(5), None] {
        let mut original = DiagnosticEvidence::new(2, DiagnosticPhase::Correction);
        original.result = DiagnosticResult::Timeout;
        original.polls = 593;
        original.finish_elapsed(60_020);
        let before = serde_json::to_value(&original).unwrap();
        let mut recovery =
            CorrectionRecoveryEvidence::new(2, original.result, CORRECTION_REFRESH_BUDGET);
        let mut refreshes = 0;
        let mut polls = 0;
        recovery
            .run(
                uri,
                CORRECTION_REFRESH_BUDGET,
                |operation, timeout| {
                    assert_eq!(timeout, Duration::from_secs(75));
                    match operation {
                        Operation::LanguageRefreshJavaDiagnostics { path, version } => {
                            refreshes += 1;
                            assert_eq!(path, SOURCE_PATH);
                            assert_eq!(version, 5);
                            Ok(correction_refresh_ack(uri))
                        }
                        Operation::LanguageEvents => {
                            polls += 1;
                            Ok(diagnostic_events(diagnostic_fixture(
                                DiagnosticPhase::Correction,
                                version,
                                uri,
                            )))
                        }
                        _ => panic!("recovery replayed or changed the document"),
                    }
                },
                || Duration::ZERO,
                |_| panic!("exact witness should finish immediately"),
            )
            .unwrap();
        assert_eq!((refreshes, polls), (1, 1));
        assert_eq!(recovery.result, CorrectionRecoveryResult::Matched);
        assert!(recovery.acknowledged && recovery.witness && recovery.budget_sufficient);
        assert_eq!(recovery.unversioned, version.is_none());
        assert!(!recovery.cleanup_reserve_guaranteed);
        assert_eq!(serde_json::to_value(&original).unwrap(), before);
        let mut session = SessionEvidence::new(2, SessionMode::FreshData);
        session.record_correction_recovery(&recovery);
        assert!(!session.correction_diagnostics && !session.semantic_checks_passed);
        assert!(
            !session.workflow_success,
            "remaining semantics and cleanup must still pass"
        );
        assert!(session.correction_recovery_witness);
        assert!(recovery
            .run(
                uri,
                CORRECTION_REFRESH_BUDGET,
                |_, _| panic!("second recovery dispatch"),
                || Duration::ZERO,
                |_| {}
            )
            .is_err());
        assert_eq!(recovery.attempts, 1);
        let serialized = serde_json::to_value(&recovery).unwrap();
        assert!(serialized
            .as_object()
            .unwrap()
            .values()
            .all(|v| v.is_boolean() || v.is_string() || v.is_u64()));
        assert!(!serialized.to_string().contains("private"));
    }
}

#[test]
fn correction_recovery_refuses_other_failures_and_insufficient_existing_budget() {
    for result in [
        DiagnosticResult::Matched,
        DiagnosticResult::RequestError,
        DiagnosticResult::MalformedEvents,
        DiagnosticResult::Truncated,
        DiagnosticResult::Lagged,
        DiagnosticResult::Closed,
    ] {
        let mut recovery = CorrectionRecoveryEvidence::new(1, result, CORRECTION_REFRESH_BUDGET);
        assert!(recovery
            .run(
                "file:///fixture/Main.java",
                CORRECTION_REFRESH_BUDGET,
                |_, _| panic!("non-timeout must never dispatch recovery"),
                || Duration::ZERO,
                |_| {}
            )
            .is_err());
        assert_eq!(recovery.result, CorrectionRecoveryResult::NotEligible);
        assert_eq!(recovery.attempts, 0);
    }
    for (available, elapsed) in [
        (Duration::ZERO, Duration::ZERO),
        (
            CORRECTION_REFRESH_BUDGET - Duration::from_millis(1),
            Duration::ZERO,
        ),
        (CORRECTION_REFRESH_BUDGET, Duration::from_millis(1)),
    ] {
        let mut recovery = CorrectionRecoveryEvidence::new(1, DiagnosticResult::Timeout, available);
        assert!(recovery
            .run(
                "file:///fixture/Main.java",
                available,
                |_, _| panic!("insufficient budget must never dispatch recovery"),
                || elapsed,
                |_| {}
            )
            .is_err());
        assert_eq!(
            recovery.result,
            CorrectionRecoveryResult::InsufficientBudget
        );
        assert_eq!(recovery.attempts, 0);
        assert!(!recovery.budget_sufficient);
    }
}

#[test]
fn correction_recovery_requires_the_exact_acknowledgement_without_retry() {
    let uri = "file:///fixture/Main.java";
    for response in [
        Err("private request error".into()),
        Ok(Value::Null),
        Ok(correction_refresh_ack("file:///other.java")),
        Ok(json!({"diagnostics_refresh_requested":uri,"version":4,"notification_only":true})),
        Ok(json!({"diagnostics_refresh_requested":uri,"version":5,"notification_only":false})),
    ] {
        let mut recovery = CorrectionRecoveryEvidence::new(
            1,
            DiagnosticResult::Timeout,
            CORRECTION_REFRESH_BUDGET,
        );
        let mut requests = 0;
        assert!(recovery
            .run(
                uri,
                CORRECTION_REFRESH_BUDGET,
                |operation, _| {
                    assert!(matches!(
                        operation,
                        cedar_protocol::Operation::LanguageRefreshJavaDiagnostics {
                            version: 5,
                            ..
                        }
                    ));
                    requests += 1;
                    response.clone()
                },
                || Duration::ZERO,
                |_| {}
            )
            .is_err());
        assert_eq!(requests, 1);
        assert_eq!(recovery.attempts, 1);
        assert!(!recovery.acknowledged && !recovery.witness);
        assert_eq!(
            recovery.result,
            if response.is_err() {
                CorrectionRecoveryResult::RequestError
            } else {
                CorrectionRecoveryResult::AcknowledgementMismatch
            }
        );
        assert!(!serde_json::to_string(&recovery)
            .unwrap()
            .contains("private"));
    }
}

#[test]
fn correction_recovery_never_accepts_empty_stale_wrong_or_error_bearing_witnesses() {
    let uri = "file:///fixture/Main.java";
    let baseline = diagnostic_fixture(DiagnosticPhase::Correction, Some(5), uri);
    let mut batches = vec![
        json!({"uri":uri,"version":5,"diagnostics":[]}),
        diagnostic_fixture(DiagnosticPhase::Correction, Some(4), uri),
        diagnostic_fixture(DiagnosticPhase::Correction, Some(5), "file:///other.java"),
    ];
    for field in ["message", "severity", "range", "residual_error"] {
        let mut batch = baseline.clone();
        match field {
            "message" => batch["diagnostics"][0]["message"] = json!("private wrong warning"),
            "severity" => batch["diagnostics"][0]["severity"] = json!(3),
            "range" => batch["diagnostics"][0]["range"]["start"]["character"] = json!(0),
            "residual_error" => batch["diagnostics"].as_array_mut().unwrap().push(
                diagnostic_fixture(DiagnosticPhase::Initial, Some(5), uri)["diagnostics"][0]
                    .clone(),
            ),
            _ => unreachable!(),
        }
        batches.push(batch);
    }
    for batch in batches {
        let clock = Cell::new(Duration::ZERO);
        let mut refreshes = 0;
        let mut recovery = CorrectionRecoveryEvidence::new(
            1,
            DiagnosticResult::Timeout,
            CORRECTION_REFRESH_BUDGET,
        );
        assert!(recovery
            .run(
                uri,
                CORRECTION_REFRESH_BUDGET,
                |operation, _| {
                    match operation {
                        cedar_protocol::Operation::LanguageRefreshJavaDiagnostics { .. } => {
                            refreshes += 1;
                            Ok(correction_refresh_ack(uri))
                        }
                        cedar_protocol::Operation::LanguageEvents => {
                            clock.set(clock.get() + Duration::from_secs(1));
                            Ok(diagnostic_events(batch.clone()))
                        }
                        _ => panic!("unexpected recovery operation"),
                    }
                },
                || clock.get(),
                |duration| clock.set(clock.get() + duration)
            )
            .is_err());
        assert_eq!(refreshes, 1);
        assert_eq!(recovery.result, CorrectionRecoveryResult::Timeout);
        assert!(recovery.acknowledged);
        assert!(!recovery.witness);
    }
}

#[test]
fn correction_recovery_rejects_stream_loss_even_after_an_exact_warning() {
    let uri = "file:///fixture/Main.java";
    for (response, expected) in [
        (Value::Null, CorrectionRecoveryResult::MalformedEvents),
        (
            json!({"truncated":true,"events":[]}),
            CorrectionRecoveryResult::Truncated,
        ),
        (
            json!({"truncated":false,"events":[{"type":"lagged"}]}),
            CorrectionRecoveryResult::Lagged,
        ),
        (
            json!({"truncated":false,"events":[{"type":"closed"}]}),
            CorrectionRecoveryResult::Closed,
        ),
    ] {
        for preceding_match in [false, true] {
            let mut response = response.clone();
            if preceding_match && response["events"].is_array() {
                response["events"].as_array_mut().unwrap().insert(0,
                    json!({"type":"diagnostics","value":diagnostic_fixture(DiagnosticPhase::Correction, Some(5), uri)}));
            }
            let mut recovery = CorrectionRecoveryEvidence::new(
                1,
                DiagnosticResult::Timeout,
                CORRECTION_REFRESH_BUDGET,
            );
            assert!(recovery
                .run(
                    uri,
                    CORRECTION_REFRESH_BUDGET,
                    |operation, _| {
                        Ok(
                            if matches!(
                                operation,
                                cedar_protocol::Operation::LanguageRefreshJavaDiagnostics { .. }
                            ) {
                                correction_refresh_ack(uri)
                            } else {
                                response.clone()
                            },
                        )
                    },
                    || Duration::ZERO,
                    |_| {}
                )
                .is_err());
            assert_eq!(recovery.result, expected);
            assert!(!recovery.witness);
        }
    }
}

#[test]
fn correction_recovery_reserves_the_last_poll_and_rejects_a_late_exact_witness() {
    let uri = "file:///fixture/Main.java";
    // A poll dispatched inside the 15-second window retains its normal 75-second
    // transport bound. Neither that bound nor its receipt claims a cleanup reserve.
    for (finished, accepted) in [
        (Duration::from_secs(120), true),
        (CORRECTION_REFRESH_BUDGET, false),
    ] {
        let clock = Cell::new(Duration::ZERO);
        let mut recovery = CorrectionRecoveryEvidence::new(
            1,
            DiagnosticResult::Timeout,
            CORRECTION_REFRESH_BUDGET,
        );
        let outcome = recovery.run(
            uri,
            CORRECTION_REFRESH_BUDGET,
            |operation, timeout| {
                assert_eq!(timeout, Duration::from_secs(75));
                if matches!(
                    operation,
                    cedar_protocol::Operation::LanguageRefreshJavaDiagnostics { .. }
                ) {
                    clock.set(Duration::from_secs(75));
                    Ok(correction_refresh_ack(uri))
                } else {
                    clock.set(finished);
                    Ok(diagnostic_events(diagnostic_fixture(
                        DiagnosticPhase::Correction,
                        Some(5),
                        uri,
                    )))
                }
            },
            || clock.get(),
            |_| {},
        );
        assert_eq!(outcome.is_ok(), accepted);
        assert_eq!(recovery.witness, accepted);
        assert_eq!(
            recovery.result,
            if accepted {
                CorrectionRecoveryResult::Matched
            } else {
                CorrectionRecoveryResult::Timeout
            }
        );
        assert_eq!(recovery.required_budget_ms, 165_000);
        assert_eq!(recovery.witness_dispatch_window_ms, 15_000);
        assert_eq!(recovery.event_poll_timeout_ms, 75_000);
    }
    let mut recovery = CorrectionRecoveryEvidence::new(1, DiagnosticResult::Timeout, Duration::MAX);
    recovery.finish_elapsed(u128::MAX);
    assert_eq!(recovery.available_budget_ms, 360_000);
    assert_eq!(recovery.elapsed_ms, 300_000);
    assert!(recovery.elapsed_saturated);
}

#[test]
fn diagnostic_receipt_distinguishes_versions_uris_and_valid_unversioned_witnesses() {
    const URI: &str = "file:///fixture%20%E9%9B%AA/Main.java";
    for phase in [DiagnosticPhase::Initial, DiagnosticPhase::Correction] {
        for version in [Some(phase.version()), None] {
            let mut receipt = DiagnosticEvidence::new(2, phase);
            receipt.begin_poll();
            assert_eq!(
                receipt.inspect_response(
                    &diagnostic_events(diagnostic_fixture(phase, version, URI)),
                    URI
                ),
                Ok(true)
            );
            assert_eq!(receipt.result, DiagnosticResult::Matched);
            assert_eq!(receipt.polls, 1);
            assert_eq!(receipt.events, 1);
            assert_eq!(receipt.parsed_batches, 1);
            assert_eq!(receipt.uri_match_batches, 1);
            assert_eq!(receipt.version_match_batches, 1);
            assert_eq!(receipt.unversioned_batches, u32::from(version.is_none()));
            assert_eq!(receipt.eligible_batches, 1);
            assert_eq!(receipt.expected_joint_diagnostics, 1);
            assert_eq!(receipt.eligible_expected_joint_diagnostics, 1);
            assert_eq!(receipt.matching_batches, 1);
        }
        for (uri, version) in [
            (URI, phase.version() - 1),
            ("file:///other.java", phase.version()),
        ] {
            let mut receipt = DiagnosticEvidence::new(2, phase);
            receipt.begin_poll();
            assert_eq!(
                receipt.inspect_response(
                    &diagnostic_events(diagnostic_fixture(phase, Some(version), uri)),
                    URI
                ),
                Ok(false)
            );
            assert_eq!(receipt.parsed_batches, 1);
            assert_eq!(receipt.uri_match_batches, u32::from(uri == URI));
            assert_eq!(
                receipt.version_match_batches,
                u32::from(version == phase.version())
            );
            assert_eq!(receipt.expected_joint_diagnostics, 1);
            assert_eq!(receipt.eligible_batches, 0);
            assert_eq!(receipt.eligible_expected_joint_diagnostics, 0);
            assert_eq!(receipt.matching_batches, 0);
        }
    }
}

#[test]
fn diagnostic_receipt_separates_wrong_triples_empty_batches_and_residual_errors() {
    const URI: &str = "file:///fixture/Main.java";
    let baseline = diagnostic_fixture(DiagnosticPhase::Correction, Some(5), URI);
    for field in ["message", "severity", "range"] {
        let mut batch = baseline.clone();
        match field {
            "message" => batch["diagnostics"][0]["message"] = json!("unrelated private text"),
            "severity" => batch["diagnostics"][0]["severity"] = json!(3),
            "range" => batch["diagnostics"][0]["range"]["start"]["character"] = json!(0),
            _ => unreachable!(),
        }
        let mut receipt = DiagnosticEvidence::new(1, DiagnosticPhase::Correction);
        assert_eq!(
            receipt.inspect_response(&diagnostic_events(batch), URI),
            Ok(false)
        );
        assert_eq!(receipt.eligible_batches, 1);
        assert_eq!(
            receipt.expected_message_diagnostics,
            u32::from(field != "message")
        );
        assert_eq!(
            receipt.expected_severity_diagnostics,
            u32::from(field != "severity")
        );
        assert_eq!(
            receipt.expected_range_diagnostics,
            u32::from(field != "range")
        );
        assert_eq!(receipt.expected_joint_diagnostics, 0);
    }
    let mut residual = baseline.clone();
    residual["diagnostics"]
        .as_array_mut()
        .unwrap()
        .push(diagnostic_fixture(DiagnosticPhase::Initial, Some(5), URI)["diagnostics"][0].clone());
    let mut receipt = DiagnosticEvidence::new(1, DiagnosticPhase::Correction);
    assert_eq!(
        receipt.inspect_response(&diagnostic_events(residual), URI),
        Ok(false)
    );
    assert_eq!(receipt.warning_diagnostics, 1);
    assert_eq!(receipt.error_diagnostics, 1);
    assert_eq!(receipt.eligible_error_diagnostics, 1);
    assert_eq!(receipt.eligible_error_free_batches, 0);
    assert_eq!(receipt.eligible_expected_joint_diagnostics, 1);
    assert_eq!(receipt.matching_batches, 0);
    let mut empty = baseline.clone();
    empty["diagnostics"] = json!([]);
    let mut receipt = DiagnosticEvidence::new(1, DiagnosticPhase::Correction);
    assert_eq!(
        receipt.inspect_response(&diagnostic_events(empty), URI),
        Ok(false)
    );
    assert_eq!(receipt.eligible_empty_batches, 1);
    assert_eq!(receipt.eligible_error_free_batches, 1);
    assert_eq!(receipt.matching_batches, 0);
    let mut malformed = baseline;
    malformed["diagnostics"][0]["range"] = Value::Null;
    let mut receipt = DiagnosticEvidence::new(1, DiagnosticPhase::Correction);
    assert_eq!(
        receipt.inspect_response(&diagnostic_events(malformed), URI),
        Ok(false)
    );
    assert_eq!(receipt.diagnostic_batches, 1);
    assert_eq!(receipt.parsed_batches, 0);
}

#[test]
fn diagnostic_stream_classification_rejects_loss_and_malformed_frames() {
    const URI: &str = "file:///fixture/Main.java";
    let good = diagnostic_events(diagnostic_fixture(DiagnosticPhase::Initial, Some(1), URI));
    let mut truncated = good.clone();
    truncated["truncated"] = json!(true);
    for (response, expected) in [
        (Value::Null, DiagnosticResult::MalformedEvents),
        (
            json!({"truncated":false,"events":null}),
            DiagnosticResult::MalformedEvents,
        ),
        (
            json!({"truncated":"false","events":[]}),
            DiagnosticResult::MalformedEvents,
        ),
        (
            json!({"truncated":false,"events":[{"type":"unknown"}]}),
            DiagnosticResult::MalformedEvents,
        ),
        (truncated, DiagnosticResult::Truncated),
        (
            json!({"truncated":false,"events":[{"type":"lagged","dropped":1}]}),
            DiagnosticResult::Lagged,
        ),
        (
            json!({"truncated":false,"events":[{"type":"closed","message":"private error"}]}),
            DiagnosticResult::Closed,
        ),
    ] {
        let mut receipt = DiagnosticEvidence::new(1, DiagnosticPhase::Initial);
        assert_eq!(receipt.inspect_response(&response, URI), Err(expected));
        assert_eq!(receipt.result, expected);
        assert_eq!(receipt.matching_batches, 0);
    }
    for category in ["lagged", "closed"] {
        let mut response = good.clone();
        response["events"]
            .as_array_mut()
            .unwrap()
            .push(json!({"type":category}));
        let mut receipt = DiagnosticEvidence::new(1, DiagnosticPhase::Initial);
        assert!(
            receipt.inspect_response(&response, URI).is_err(),
            "a preceding match must not hide event-stream failure"
        );
        assert_eq!(receipt.matching_batches, 1);
    }
}

#[test]
fn diagnostic_receipts_are_bounded_and_never_retain_server_text() {
    let mut receipt = DiagnosticEvidence::new(3, DiagnosticPhase::Correction);
    assert_eq!(receipt.result, DiagnosticResult::RequestError);
    assert_eq!(receipt.events, 0);
    receipt.polls = DIAGNOSTIC_COUNTER_LIMIT;
    receipt.begin_poll();
    assert_eq!(receipt.polls, DIAGNOSTIC_COUNTER_LIMIT);
    assert!(receipt.counters_saturated);
    receipt.finish_elapsed(u128::MAX);
    assert_eq!(receipt.elapsed_ms, 300_000);
    assert!(receipt.elapsed_saturated);
    receipt.result = DiagnosticResult::Timeout;
    let private_uri = "file:///private-user-directory/Main.java";
    let mut batch = diagnostic_fixture(DiagnosticPhase::Correction, Some(5), private_uri);
    batch["diagnostics"][0]["message"] = json!("private message sentinel");
    receipt
        .inspect_response(&diagnostic_events(batch), private_uri)
        .unwrap();
    let serialized = serde_json::to_string(&receipt).unwrap();
    assert!(serialized.len() < 2048);
    assert!(!serialized.contains("private"));
    assert!(serialized.contains("\"result\":\"timeout\""));
    let mut counter = 0;
    let mut saturated = false;
    add_diagnostic_count(&mut counter, &mut saturated, usize::MAX);
    assert_eq!(counter, DIAGNOSTIC_COUNTER_LIMIT);
    assert!(saturated);
}
