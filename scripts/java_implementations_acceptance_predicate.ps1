# Pure Quick-only predicate. Loading this file launches nothing.
# Duplicate JSON keys/receipts are rejected by the collector before this step.
function Assert-JavaImplementationsReceipt([object] $Receipts) {
    if ($Receipts -isnot [array]) { throw 'Implementation receipts must be a JSON array.' }
    $matching = [Collections.Generic.List[object]]::new()
    for ($index = 0; $index -lt $Receipts.Length; $index++) {
        $candidate = $Receipts[$index]
        if ($candidate -is [array] -or $candidate -isnot [pscustomobject] -or
            $candidate.kind -isnot [string]) {
            throw 'Implementation receipts must contain scalar receipt objects.'
        }
        if ($candidate.kind -ceq 'windows_java_implementations') { $matching.Add($candidate) }
    }
    if ($matching.Count -ne 1) { throw 'Expected exactly one Quick implementation receipt.' }
    $record = $matching[0]
    if ($record.failure_stage -isnot [string] -or $record.failure_stage -cne 'none') {
        throw 'Implementation receipt did not complete its finite semantic witness.'
    }
    foreach ($field in @('exercised', 'capability_supported', 'provider_supported',
        'query_version_acknowledged', 'targets_unopened', 'exact_type_uris', 'exact_type_ranges',
        'exact_method_uri', 'exact_method_range', 'inherited_method_absent', 'negative_query_empty',
        'utf16_ranges_exact', 'resolved_path_exact', 'ordinary_read_exact', 'actual_frontend_navigation',
        'full_selection_preserved', 'dirty_buffer_reused', 'undo_redo_preserved',
        'retained_context_preserved', 'source_files_unchanged', 'root_handle_signaled',
        'client_reaped', 'synthetic_root_removed', 'success')) {
        if ($record.$field -isnot [bool] -or -not $record.$field) {
            throw 'Implementation semantic, frontend or owned-cleanup witness is incomplete.'
        }
    }
    foreach ($field in @('primary_failed', 'cleanup_failed', 'elapsed_saturated')) {
        if ($record.$field -isnot [bool] -or $record.$field) { throw 'Implementation acceptance failed.' }
    }
    $counts = @{ type_result_count = 2; method_result_count = 1; negative_result_count = 0 }
    foreach ($field in $counts.Keys) {
        if (($record.$field -isnot [int] -and $record.$field -isnot [long]) -or
            $record.$field -ne $counts[$field]) { throw 'Implementation result counts were not exact.' }
    }
    if (($record.elapsed_ms -isnot [int] -and $record.elapsed_ms -isnot [long]) -or
        $record.elapsed_ms -lt 0 -or $record.elapsed_ms -gt 240000) {
        throw 'Implementation acceptance elapsed time is invalid.'
    }
}
