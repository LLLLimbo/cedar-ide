# Pure required-idle release predicate. Loading this file launches nothing.
# The collector rejects duplicate JSON keys before ConvertFrom-Json is used.
function Assert-IdleCorrectionReceipt([object[]] $Receipts) {
    $matching = @($Receipts | Where-Object {
        $_ -is [pscustomobject] -and $_.kind -is [string] -and
        $_.kind -ceq 'windows_java_idle_correction'
    })
    if ($matching.Count -ne 1) { throw 'Expected exactly one required idle correction receipt.' }
    $record = $matching[0]
    foreach ($field in @('kind', 'route', 'failure_stage', 'stop_status', 'stop_reason',
        'spontaneous_result', 'recovery_result')) {
        if ($record.$field -isnot [string]) { throw 'Idle receipt tags must be scalar strings.' }
    }
    if ($record.route -cne 'normal_agent_client' -or $record.failure_stage -cne 'none') {
        throw 'Required idle workflow did not finish on the normal shipping route.'
    }
    foreach ($field in @('java_capabilities', 'generic_start_rejected', 'untrusted_start_rejected',
        'root_observed_live', 'root_identity_verified', 'semantic_diagnostics', 'exact_definition',
        'real_completion', 'deferred_import_resolve', 'actual_editor_apply_undo_redo',
        'versions_2_3_4_synced', 'correction_acknowledged', 'diagnostics_refresh_supported',
        'source_unchanged', 'stop_outcome_verified', 'cleanup_joined', 'root_handle_signaled',
        'client_reaped', 'synthetic_root_removed', 'success', 'workflow_success',
        'primary_deadline_met', 'cleanup_deadline_met', 'cleanup_reserve_preserved')) {
        if ($record.$field -isnot [bool] -or -not $record.$field) {
            throw 'Idle semantic, workflow, deadline or owned-cleanup witness is incomplete.'
        }
    }
    foreach ($field in @('primary_failed', 'cleanup_failed', 'elapsed_saturated', 'deadline_failed',
        'async_start_exercised', 'async_start_begin_acknowledged', 'async_start_read_while_starting',
        'async_start_ready', 'diagnostics_refresh_exercised', 'diagnostics_refresh_requested',
        'diagnostics_refresh_witness', 'diagnostics_refresh_unversioned')) {
        if ($record.$field -isnot [bool] -or $record.$field) {
            throw 'Idle receipt failed or mixed a separate Quick-only workload into its evidence.'
        }
    }
    foreach ($field in @('correction_diagnostics', 'spontaneous_success', 'recovery_acknowledged',
        'recovery_witness', 'recovery_unversioned', 'recovery_budget_sufficient',
        'shutdown_response_received', 'exit_frame_completed')) {
        if ($record.$field -isnot [bool]) { throw 'Idle branch witnesses must be scalar booleans.' }
    }
    $fixed = @{
        primary_deadline_ms = 360000; outer_deadline_ms = 480000;
        cleanup_reserve_ms = 120000; request_timeout_ms = 75000;
        spontaneous_dispatch_window_ms = 60000; diagnostic_wait_admission_ms = 135000;
        initial_idle_ms = 30000; recovery_budget_ms = 165000; recovery_admission_ms = 240000;
        close_budget_ms = 75000; stop_budget_ms = 75000; root_exit_budget_ms = 3000;
        client_reap_budget_ms = 30000; cleanup_bookkeeping_ms = 9000
    }
    foreach ($field in $fixed.Keys) {
        if (($record.$field -isnot [int] -and $record.$field -isnot [long]) -or
            $record.$field -ne $fixed[$field]) { throw 'Idle workflow changed a fixed deadline or admission budget.' }
    }
    $maximum = @{
        elapsed_ms = 479999; primary_elapsed_ms = 359999; cleanup_started_ms = 359999;
        root_exit_code = 4294967295; spontaneous_matching_batches = 65535;
        recovery_attempts = 1; recovery_available_budget_ms = 360000
    }
    foreach ($field in $maximum.Keys) {
        if (($record.$field -isnot [int] -and $record.$field -isnot [long]) -or
            $record.$field -lt 0 -or $record.$field -gt $maximum[$field]) {
            throw 'Idle receipt counter or deadline is invalid.'
        }
    }
    if ($record.primary_elapsed_ms -ne $record.cleanup_started_ms -or
        $record.primary_elapsed_ms -lt $record.initial_idle_ms -or
        $record.elapsed_ms -lt $record.cleanup_started_ms -or
        ($record.outer_deadline_ms - $record.cleanup_started_ms) -lt $record.cleanup_reserve_ms) {
        throw 'Idle primary/cleanup timing contradicts the reserved deadline.'
    }
    if ($record.stop_status -ceq 'graceful') {
        if ($record.stop_reason -cne 'root_exited' -or $record.root_exit_code -ne 0 -or
            -not $record.shutdown_response_received -or -not $record.exit_frame_completed) {
            throw 'Idle graceful stop lacks matching protocol and zero root-exit evidence.'
        }
    } elseif ($record.stop_status -ceq 'forced') {
        if ($record.stop_reason -cnotin @('grace_expired', 'aborted')) {
            throw 'Idle forced stop lacks a matching termination reason.'
        }
    } else { throw 'Idle owned stop outcome was not verified.' }
    if ($record.spontaneous_result -ceq 'matched') {
        if (-not $record.spontaneous_success -or -not $record.correction_diagnostics -or
            $record.spontaneous_matching_batches -lt 1 -or $record.recovery_attempts -ne 0 -or
            $record.recovery_result -cne 'not_attempted' -or $record.recovery_acknowledged -or
            $record.recovery_witness -or $record.recovery_unversioned -or
            $record.recovery_budget_sufficient -or $record.recovery_available_budget_ms -ne 0) {
            throw 'Idle spontaneous success contradicts its separate recovery evidence.'
        }
    } elseif ($record.spontaneous_result -ceq 'timeout') {
        if ($record.spontaneous_success -or $record.correction_diagnostics -or
            $record.spontaneous_matching_batches -ne 0 -or $record.recovery_attempts -ne 1 -or
            $record.recovery_result -cne 'matched' -or -not $record.recovery_acknowledged -or
            -not $record.recovery_witness -or -not $record.recovery_budget_sufficient -or
            $record.recovery_available_budget_ms -lt 240000 -or
            $record.recovery_available_budget_ms -gt 270000 -or
            $record.primary_elapsed_ms -lt 90000 -or
            $record.primary_elapsed_ms -lt (360000 - $record.recovery_available_budget_ms)) {
            throw 'Idle timeout requires exactly one timely supported refresh and an accepted exact witness.'
        }
    } else {
        throw 'Only an original idle timeout may use the single-refresh recovery workflow.'
    }
    return [int]($record.spontaneous_result -ceq 'timeout')
}
