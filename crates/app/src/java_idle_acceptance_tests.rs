//! Test-only wall-clock admissions and a separate idle workflow receipt. The
//! historical diagnostic/recovery receipts and transport deadlines stay intact.
use super::*;
use cedar_protocol::Operation;

pub(super) const OUTER_BUDGET: Duration = Duration::from_secs(480);
pub(super) const PRIMARY_BUDGET: Duration = Duration::from_secs(360);
pub(super) const CLEANUP_RESERVE: Duration = Duration::from_secs(120);
pub(super) const REQUEST_BUDGET: Duration = Duration::from_secs(75);
pub(super) const DIAGNOSTIC_ADMISSION: Duration = Duration::from_secs(135);
pub(super) const SPONTANEOUS_DISPATCH: Duration = Duration::from_secs(60);
pub(super) const INITIAL_IDLE: Duration = Duration::from_secs(30);
pub(super) const RECOVERY_ADMISSION: Duration = Duration::from_secs(240);
pub(super) const ROOT_EXIT_BUDGET: Duration = Duration::from_secs(3);
pub(super) const CLIENT_REAP_BUDGET: Duration = Duration::from_secs(30);

#[derive(Default)]
pub(super) struct Budget {
    cleanup: Cell<bool>,
    primary_failed: Cell<bool>,
    cleanup_failed: Cell<bool>,
    cleanup_started: Cell<Option<Duration>>,
}
impl Budget {
    fn cutoff(&self) -> Duration {
        if self.cleanup.get() {
            OUTER_BUDGET
        } else {
            PRIMARY_BUDGET
        }
    }

    fn fail(&self) {
        if self.cleanup.get() {
            self.cleanup_failed.set(true);
        } else {
            self.primary_failed.set(true);
        }
    }

    pub fn remaining_primary(&self, now: Duration) -> Duration {
        PRIMARY_BUDGET.saturating_sub(now)
    }

    pub fn admit(&self, now: Duration, cost: Duration) -> CheckResult<()> {
        if now >= self.cutoff() || self.cutoff().saturating_sub(now) < cost {
            self.fail();
            return Err("idle workflow cannot admit the full unchanged operation deadline".into());
        }
        Ok(())
    }

    pub fn check(&self, now: Duration) -> CheckResult<()> {
        if now >= self.cutoff() {
            self.fail();
            return Err("idle workflow operation returned after its fixed deadline".into());
        }
        Ok(())
    }

    pub fn run<T>(
        &self,
        now: impl Fn() -> Duration,
        cost: Duration,
        operation: impl FnOnce() -> CheckResult<T>,
    ) -> CheckResult<T> {
        self.admit(now(), cost)?;
        let outcome = operation();
        // Check errors as well as successful returns. A late result is never
        // evidence of a timely primary operation or completed cleanup.
        self.check(now())?;
        outcome
    }

    pub fn begin_cleanup(&self, now: Duration) {
        let _ = self.check(now);
        self.cleanup_started.set(Some(now));
        self.cleanup.set(true);
    }

    pub fn deadlines_met(&self, now: Duration) -> bool {
        self.cleanup_started
            .get()
            .is_some_and(|at| at < PRIMARY_BUDGET)
            && !self.primary_failed.get()
            && !self.cleanup_failed.get()
            && now < OUTER_BUDGET
    }
}

// No individual records are printed here. This helper preserves the original
// 60-second dispatch window, including one already-admitted 75-second last poll.
pub(super) fn diagnostics(
    budget: &Budget,
    classification: &mut DiagnosticEvidence,
    uri: &str,
    mut request: impl FnMut(Operation) -> CheckResult<Value>,
    now: impl Fn() -> Duration,
    mut pause: impl FnMut(Duration),
) -> CheckResult<()> {
    budget.admit(now(), DIAGNOSTIC_ADMISSION)?;
    let dispatch_deadline = now() + SPONTANEOUS_DISPATCH;
    while now() < dispatch_deadline {
        classification.begin_poll();
        let response = budget.run(&now, REQUEST_BUDGET, || request(Operation::LanguageEvents))?;
        // Malformed diagnostic payloads are failures, not an eligible timeout.
        if !diagnostic_payloads_valid(&response) {
            classification.result = DiagnosticResult::MalformedEvents;
            return Err("idle workflow received malformed diagnostic data".into());
        }
        match classification.inspect_response(&response, uri) {
            Ok(true) => return Ok(()),
            Ok(false) => {}
            Err(_) => return Err("idle workflow diagnostic stream failed".into()),
        }
        pause(Duration::from_millis(100));
    }
    budget.check(now())?;
    classification.result = DiagnosticResult::Timeout;
    Err("idle workflow spontaneous diagnostic witness timed out".into())
}

fn diagnostic_payloads_valid(response: &Value) -> bool {
    response["events"].as_array().is_none_or(|events| {
        events.iter().all(|event| {
            event["type"] != "diagnostics"
                || language_results::Diagnostics::default()
                    .apply(&event["value"])
                    .is_ok()
        })
    })
}

#[derive(Default, Serialize)]
pub(super) struct Correction {
    #[serde(skip)]
    considered: bool,
    pub spontaneous_result: DiagnosticResult,
    pub spontaneous_success: bool,
    pub spontaneous_matching_batches: u32,
    pub recovery_attempts: u32,
    pub recovery_result: CorrectionRecoveryResult,
    pub recovery_acknowledged: bool,
    pub recovery_witness: bool,
    pub recovery_unversioned: bool,
    pub recovery_budget_sufficient: bool,
    pub recovery_available_budget_ms: u32,
}
impl Correction {
    pub fn run(
        &mut self,
        budget: &Budget,
        uri: &str,
        mut request: impl FnMut(Operation) -> CheckResult<Value>,
        now: impl Fn() -> Duration,
        mut pause: impl FnMut(Duration),
    ) -> CheckResult<()> {
        if self.considered {
            return Err("idle correction may only be considered once".into());
        }
        self.considered = true;
        let mut original = DiagnosticEvidence::new(1, DiagnosticPhase::Correction);
        let outcome = diagnostics(budget, &mut original, uri, &mut request, &now, &mut pause);
        self.spontaneous_result = original.result;
        self.spontaneous_success = outcome.is_ok();
        self.spontaneous_matching_batches = original.matching_batches;
        if outcome.is_ok() {
            return Ok(());
        }
        if original.result != DiagnosticResult::Timeout {
            self.recovery_result = CorrectionRecoveryResult::NotEligible;
            return outcome;
        }
        let recovery_started = now();
        let available = budget.remaining_primary(recovery_started);
        self.recovery_available_budget_ms = available.as_millis().min(360_000) as u32;
        if available < RECOVERY_ADMISSION {
            self.recovery_result = CorrectionRecoveryResult::InsufficientBudget;
            return Err(
                "idle recovery requires its full 165 seconds plus 75 seconds for Close".into(),
            );
        }
        self.recovery_budget_sufficient = true;
        let mut recovery = CorrectionRecoveryEvidence::new(1, original.result, available);
        let malformed = Cell::new(false);
        let recovered = recovery.run(
            uri,
            // The legacy helper subtracts its first elapsed read before
            // admitting 165 seconds. Leave the complete Close reserve outside
            // its available envelope, including elapsed admission bookkeeping.
            available - REQUEST_BUDGET,
            |op, _unchanged_timeout| {
                let value = budget.run(&now, REQUEST_BUDGET, || request(op))?;
                if !diagnostic_payloads_valid(&value) {
                    malformed.set(true);
                    return Err("idle recovery received malformed diagnostic data".into());
                }
                Ok(value)
            },
            || now().saturating_sub(recovery_started),
            pause,
        );
        self.recovery_budget_sufficient = recovery.budget_sufficient;
        self.recovery_attempts = recovery.attempts;
        self.recovery_result = if malformed.get() {
            CorrectionRecoveryResult::MalformedEvents
        } else {
            recovery.result
        };
        self.recovery_acknowledged = recovery.acknowledged;
        self.recovery_witness = recovery.witness;
        self.recovery_unversioned = recovery.unversioned;
        recovered
    }

    pub fn accepted(&self) -> bool {
        (self.spontaneous_result == DiagnosticResult::Matched
            && self.spontaneous_success
            && self.spontaneous_matching_batches > 0
            && self.recovery_attempts == 0
            && self.recovery_result == CorrectionRecoveryResult::NotAttempted)
            || (self.spontaneous_result == DiagnosticResult::Timeout
                && !self.spontaneous_success
                && self.spontaneous_matching_batches == 0
                && self.recovery_attempts == 1
                && self.recovery_result == CorrectionRecoveryResult::Matched
                && self.recovery_budget_sufficient
                && self.recovery_acknowledged
                && self.recovery_witness)
    }
}

#[derive(Serialize)]
pub(super) struct Receipt<'a> {
    #[serde(flatten)]
    base: &'a ProductionEvidence,
    #[serde(flatten)]
    correction: &'a Correction,
    workflow_success: bool,
    primary_deadline_ms: u32,
    outer_deadline_ms: u32,
    cleanup_reserve_ms: u32,
    request_timeout_ms: u32,
    spontaneous_dispatch_window_ms: u32,
    diagnostic_wait_admission_ms: u32,
    initial_idle_ms: u32,
    recovery_budget_ms: u32,
    recovery_admission_ms: u32,
    close_budget_ms: u32,
    stop_budget_ms: u32,
    root_exit_budget_ms: u32,
    client_reap_budget_ms: u32,
    cleanup_bookkeeping_ms: u32,
    primary_deadline_met: bool,
    cleanup_deadline_met: bool,
    cleanup_reserve_preserved: bool,
    deadline_failed: bool,
    primary_elapsed_ms: u32,
    cleanup_started_ms: u32,
}
impl<'a> Receipt<'a> {
    pub fn new(
        base: &'a ProductionEvidence,
        correction: &'a Correction,
        budget: &Budget,
        now: Duration,
    ) -> Self {
        let cleanup_started = budget.cleanup_started.get().unwrap_or(now);
        Self {
            base,
            correction,
            workflow_success: base.success,
            primary_deadline_ms: 360_000,
            outer_deadline_ms: 480_000,
            cleanup_reserve_ms: 120_000,
            request_timeout_ms: 75_000,
            spontaneous_dispatch_window_ms: 60_000,
            diagnostic_wait_admission_ms: 135_000,
            initial_idle_ms: 30_000,
            recovery_budget_ms: 165_000,
            recovery_admission_ms: 240_000,
            close_budget_ms: 75_000,
            stop_budget_ms: 75_000,
            root_exit_budget_ms: 3_000,
            client_reap_budget_ms: 30_000,
            cleanup_bookkeeping_ms: 9_000,
            primary_deadline_met: !budget.primary_failed.get() && cleanup_started < PRIMARY_BUDGET,
            cleanup_deadline_met: !budget.cleanup_failed.get() && now < OUTER_BUDGET,
            cleanup_reserve_preserved: cleanup_started <= PRIMARY_BUDGET,
            deadline_failed: !budget.deadlines_met(now),
            primary_elapsed_ms: cleanup_started.as_millis().min(480_000) as u32,
            cleanup_started_ms: cleanup_started.as_millis().min(480_000) as u32,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const URI: &str = "file:///synthetic/Main.java";

    fn witness(versioned: bool) -> Value {
        let range = marker_range(&corrected_source(), "correctedOnly");
        let mut value = json!({"uri": URI, "diagnostics":[{"severity":2,
            "message":"The value of the local variable correctedOnly is not used",
            "range":{"start":{"line":range.start.line,"character":range.start.character},
                     "end":{"line":range.end.line,"character":range.end.character}}}]});
        if versioned {
            value["version"] = json!(5);
        }
        json!({"truncated":false,"events":[{"type":"diagnostics","value":value}]})
    }
    fn acknowledgement() -> Value {
        json!({"version":5,"notification_only":true,"diagnostics_refresh_requested":URI})
    }
    fn empty() -> Value {
        json!({"truncated":false,"events":[]})
    }

    #[test]
    fn matching_spontaneous_witness_never_refreshes() {
        let clock = Cell::new(Duration::ZERO);
        let budget = Budget::default();
        let mut correction = Correction::default();
        correction
            .run(
                &budget,
                URI,
                |op| {
                    assert!(matches!(op, Operation::LanguageEvents));
                    Ok(witness(true))
                },
                || clock.get(),
                |delay| clock.set(clock.get() + delay),
            )
            .unwrap();
        assert!(correction.accepted());
        assert_eq!(correction.spontaneous_result, DiagnosticResult::Matched);
        assert_eq!(correction.recovery_attempts, 0);
    }

    #[test]
    fn timeout_has_exactly_one_refresh_without_rewriting_original_result() {
        for versioned in [true, false] {
            let clock = Cell::new(Duration::from_secs(40));
            let refreshed = Cell::new(false);
            let mut correction = Correction::default();
            correction
                .run(
                    &Budget::default(),
                    URI,
                    |op| match op {
                        Operation::LanguageEvents if !refreshed.get() => Ok(empty()),
                        Operation::LanguageEvents => Ok(witness(versioned)),
                        Operation::LanguageRefreshJavaDiagnostics { path, version } => {
                            assert_eq!(path, SOURCE_PATH);
                            assert_eq!(version, 5);
                            assert!(!refreshed.replace(true));
                            Ok(acknowledgement())
                        }
                        _ => panic!("edit replay or unrelated request"),
                    },
                    || clock.get(),
                    |delay| clock.set(clock.get() + delay),
                )
                .unwrap();
            assert!(correction.accepted());
            assert_eq!(correction.spontaneous_result, DiagnosticResult::Timeout);
            assert!(!correction.spontaneous_success);
            assert_eq!(correction.spontaneous_matching_batches, 0);
            assert_eq!(correction.recovery_attempts, 1);
            assert_eq!(correction.recovery_unversioned, !versioned);
        }
    }

    #[test]
    fn recovery_admission_reserves_close_at_exact_240_second_boundary() {
        for milliseconds in [120_000, 120_001] {
            let clock = Cell::new(Duration::from_millis(milliseconds - 60_000));
            let refreshed = Cell::new(false);
            let mut correction = Correction::default();
            let result = correction.run(
                &Budget::default(),
                URI,
                |op| match op {
                    Operation::LanguageEvents if !refreshed.get() => Ok(empty()),
                    Operation::LanguageEvents => Ok(witness(true)),
                    Operation::LanguageRefreshJavaDiagnostics { .. } => {
                        refreshed.set(true);
                        Ok(acknowledgement())
                    }
                    _ => panic!("unexpected request"),
                },
                || clock.get(),
                |delay| clock.set(clock.get() + delay),
            );
            assert_eq!(result.is_ok(), milliseconds == 120_000);
            assert_eq!(refreshed.get(), milliseconds == 120_000);
            assert_eq!(
                correction.recovery_available_budget_ms,
                (360_000 - milliseconds) as u32
            );
            if milliseconds == 120_001 {
                assert_eq!(
                    correction.recovery_result,
                    CorrectionRecoveryResult::InsufficientBudget
                );
            }
        }
    }

    #[test]
    fn advancing_clock_at_helper_admission_preserves_the_close_reserve() {
        for start_seconds in [40, 60] {
            let clock = Cell::new(Duration::from_secs(start_seconds));
            let timeout_at = Duration::from_secs(start_seconds + 60);
            let reads_after_timeout = Cell::new(0);
            let refreshed = Cell::new(false);
            let mut correction = Correction::default();
            let result = correction.run(
                &Budget::default(),
                URI,
                |op| match op {
                    Operation::LanguageEvents if !refreshed.get() => Ok(empty()),
                    Operation::LanguageEvents => Ok(witness(true)),
                    Operation::LanguageRefreshJavaDiagnostics { .. } => {
                        refreshed.set(true);
                        Ok(acknowledgement())
                    }
                    _ => panic!("unexpected request"),
                },
                || {
                    if clock.get() >= timeout_at {
                        let reads = reads_after_timeout.get() + 1;
                        reads_after_timeout.set(reads);
                        // Last loop condition, final wait check, admission snapshot,
                        // then the unchanged helper's initial elapsed-clock read.
                        if reads == 4 {
                            clock.set(clock.get() + Duration::from_millis(1));
                        }
                    }
                    clock.get()
                },
                |delay| clock.set(clock.get() + delay),
            );
            assert_eq!(result.is_ok(), start_seconds == 40);
            assert_eq!(refreshed.get(), start_seconds == 40);
            assert_eq!(correction.recovery_budget_sufficient, start_seconds == 40);
            if start_seconds == 60 {
                assert_eq!(
                    correction.recovery_result,
                    CorrectionRecoveryResult::InsufficientBudget
                );
                assert_eq!(correction.recovery_attempts, 0);
            }
        }
    }

    #[test]
    fn failed_refresh_and_wrong_acknowledgement_fail_the_workflow() {
        for error in [true, false] {
            let clock = Cell::new(Duration::ZERO);
            let mut correction = Correction::default();
            let result = correction.run(
                &Budget::default(),
                URI,
                |op| match op {
                    Operation::LanguageEvents => Ok(empty()),
                    Operation::LanguageRefreshJavaDiagnostics { .. } if error => {
                        Err("request failed".into())
                    }
                    Operation::LanguageRefreshJavaDiagnostics { .. } => Ok(json!({"version":4})),
                    _ => panic!("unexpected request"),
                },
                || clock.get(),
                |delay| clock.set(clock.get() + delay),
            );
            assert!(result.is_err());
            assert!(!correction.accepted());
            assert_eq!(correction.recovery_attempts, 1);
            assert_eq!(
                correction.recovery_result,
                if error {
                    CorrectionRecoveryResult::RequestError
                } else {
                    CorrectionRecoveryResult::AcknowledgementMismatch
                }
            );
        }
    }

    #[test]
    fn malformed_and_noneligible_results_never_refresh() {
        for response in [
            Value::Null,
            json!({"truncated":true,"events":[]}),
            json!({"truncated":false,"events":[{"type":"closed"}]}),
            json!({"truncated":false,"events":[{"type":"lagged"}]}),
            json!({"truncated":false,"events":[{"type":"diagnostics","value":{}}]}),
        ] {
            let mut correction = Correction::default();
            assert!(correction
                .run(
                    &Budget::default(),
                    URI,
                    |op| {
                        assert!(matches!(op, Operation::LanguageEvents));
                        Ok(response.clone())
                    },
                    || Duration::ZERO,
                    |_| panic!("invalid stream must fail immediately")
                )
                .is_err());
            assert_eq!(correction.recovery_attempts, 0);
            assert_eq!(
                correction.recovery_result,
                CorrectionRecoveryResult::NotEligible
            );
            assert!(!correction.accepted());
        }
    }

    #[test]
    fn request_failure_never_recovers_or_retries_the_original_wait() {
        let mut correction = Correction::default();
        assert!(correction
            .run(
                &Budget::default(),
                URI,
                |op| {
                    assert!(matches!(op, Operation::LanguageEvents));
                    Err("request failed".into())
                },
                || Duration::ZERO,
                |_| panic!("no retry")
            )
            .is_err());
        assert_eq!(
            correction.spontaneous_result,
            DiagnosticResult::RequestError
        );
        assert_eq!(
            correction.recovery_result,
            CorrectionRecoveryResult::NotEligible
        );
        assert_eq!(correction.recovery_attempts, 0);
        assert!(correction
            .run(
                &Budget::default(),
                URI,
                |_| panic!("no second consideration"),
                || Duration::ZERO,
                |_| panic!("no retry")
            )
            .is_err());
    }

    #[test]
    fn failed_recovery_streams_and_missing_witness_never_pass() {
        for response in [
            Value::Null,
            json!({"truncated":true,"events":[]}),
            json!({"truncated":false,"events":[{"type":"closed"}]}),
            json!({"truncated":false,"events":[{"type":"lagged"}]}),
            json!({"truncated":false,"events":[{"type":"diagnostics","value":{}}]}),
            empty(),
        ] {
            let clock = Cell::new(Duration::ZERO);
            let refreshed = Cell::new(false);
            let mut correction = Correction::default();
            assert!(correction
                .run(
                    &Budget::default(),
                    URI,
                    |op| match op {
                        Operation::LanguageEvents if !refreshed.get() => Ok(empty()),
                        Operation::LanguageEvents => Ok(response.clone()),
                        Operation::LanguageRefreshJavaDiagnostics { .. } => {
                            assert!(!refreshed.replace(true));
                            Ok(acknowledgement())
                        }
                        _ => panic!("unexpected request"),
                    },
                    || clock.get(),
                    |delay| clock.set(clock.get() + delay)
                )
                .is_err());
            assert_eq!(correction.spontaneous_result, DiagnosticResult::Timeout);
            assert_eq!(correction.recovery_attempts, 1);
            assert!(!correction.recovery_witness);
            assert!(!correction.accepted());
        }
    }

    #[test]
    fn primary_admission_rejects_work_before_calling_it() {
        for (remaining, cost) in [
            (74, REQUEST_BUDGET),
            (134, DIAGNOSTIC_ADMISSION),
            (29, INITIAL_IDLE),
        ] {
            let budget = Budget::default();
            let now = PRIMARY_BUDGET - Duration::from_secs(remaining);
            assert!(budget
                .run(
                    || now,
                    cost,
                    || -> CheckResult<()> {
                        panic!("operation cannot start without its complete deadline")
                    }
                )
                .is_err());
            budget.begin_cleanup(now);
            budget.run(|| now, REQUEST_BUDGET, || Ok(())).unwrap();
            assert!(!budget.deadlines_met(now));
        }
    }

    #[test]
    fn refused_primary_reap_preserves_ownership_for_cleanup() {
        let budget = Budget::default();
        let clock = Cell::new(Duration::from_secs(286));
        let mut owned = Some("owned client");
        assert!(budget
            .run(
                || clock.get(),
                REQUEST_BUDGET,
                || { owned.take().ok_or("missing client".into()) }
            )
            .is_err());
        assert_eq!(owned, Some("owned client"));
        budget.begin_cleanup(clock.get());
        assert_eq!(
            budget
                .run(
                    || clock.get(),
                    CLIENT_REAP_BUDGET,
                    || { owned.take().ok_or("missing client".into()) }
                )
                .unwrap(),
            "owned client"
        );
        assert!(owned.is_none());
    }

    #[test]
    fn late_calls_fail_and_cleanup_still_has_its_independent_reserve() {
        let budget = Budget::default();
        let clock = Cell::new(Duration::from_secs(285));
        assert!(budget
            .run(
                || clock.get(),
                REQUEST_BUDGET,
                || {
                    clock.set(PRIMARY_BUDGET);
                    Ok(())
                }
            )
            .is_err());
        budget.begin_cleanup(clock.get());
        // Crossing the primary deadline cannot strand cleanup behind that
        // admission gate. Stop and both root waits plus reaping still fit.
        for cost in [
            REQUEST_BUDGET,
            ROOT_EXIT_BUDGET,
            CLIENT_REAP_BUDGET,
            ROOT_EXIT_BUDGET,
        ] {
            budget
                .run(
                    || clock.get(),
                    cost,
                    || {
                        clock.set(clock.get() + cost);
                        Ok(())
                    },
                )
                .unwrap();
        }
        assert_eq!(OUTER_BUDGET - clock.get(), Duration::from_secs(9));
        assert!(!budget.deadlines_met(clock.get()));
        let mut base = ProductionEvidence::new();
        base.kind = "windows_java_idle_correction";
        let correction = Correction::default();
        let receipt =
            serde_json::to_value(Receipt::new(&base, &correction, &budget, clock.get())).unwrap();
        assert_eq!(receipt["cleanup_reserve_preserved"], true);
        assert_eq!(receipt["cleanup_deadline_met"], true);
        assert_eq!(receipt["workflow_success"], false);
    }

    #[test]
    fn outer_480_second_deadline_cannot_pass_even_with_successful_cleanup_call() {
        let budget = Budget::default();
        budget.begin_cleanup(Duration::from_secs(359));
        let clock = Cell::new(Duration::from_secs(405));
        assert!(budget
            .run(
                || clock.get(),
                REQUEST_BUDGET,
                || {
                    clock.set(OUTER_BUDGET);
                    Ok(())
                }
            )
            .is_err());
        assert!(!budget.deadlines_met(clock.get()));
    }

    #[test]
    fn final_spontaneous_poll_may_finish_after_dispatch_but_not_primary_cutoff() {
        let clock = Cell::new(Duration::ZERO);
        let mut polls = 0;
        let mut classification = DiagnosticEvidence::new(1, DiagnosticPhase::Correction);
        diagnostics(
            &Budget::default(),
            &mut classification,
            URI,
            |_| {
                polls += 1;
                if polls == 1 {
                    clock.set(Duration::from_millis(59_800));
                    Ok(empty())
                } else {
                    clock.set(clock.get() + REQUEST_BUDGET);
                    Ok(witness(true))
                }
            },
            || clock.get(),
            |delay| clock.set(clock.get() + delay),
        )
        .unwrap();
        assert!(clock.get() > SPONTANEOUS_DISPATCH);
        assert_eq!(classification.result, DiagnosticResult::Matched);
    }

    #[test]
    fn late_recovery_witness_is_rejected_by_the_unchanged_165_second_helper() {
        let clock = Cell::new(Duration::ZERO);
        let refreshed = Cell::new(false);
        let mut correction = Correction::default();
        assert!(correction
            .run(
                &Budget::default(),
                URI,
                |op| match op {
                    Operation::LanguageEvents if !refreshed.get() => Ok(empty()),
                    Operation::LanguageRefreshJavaDiagnostics { .. } => {
                        refreshed.set(true);
                        Ok(acknowledgement())
                    }
                    Operation::LanguageEvents => {
                        clock.set(clock.get() + CORRECTION_REFRESH_BUDGET);
                        Ok(witness(true))
                    }
                    _ => panic!("unexpected request"),
                },
                || clock.get(),
                |delay| clock.set(clock.get() + delay)
            )
            .is_err());
        assert_eq!(
            correction.recovery_result,
            CorrectionRecoveryResult::Timeout
        );
        assert!(!correction.recovery_witness);
        assert!(!correction.accepted());
    }
}
