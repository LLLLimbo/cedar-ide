# Pure Quick-only release predicate. Loading this file launches nothing.
function Assert-WorkspaceTypeReceipt([object[]] $Receipts) {
    $matching = @($Receipts | Where-Object { $_.kind -ceq 'windows_java_workspace_types' })
    if ($matching.Count -ne 1) { throw 'Expected exactly one Quick workspace type receipt.' }
    $record = $matching[0]
    if ($record -isnot [pscustomobject] -or $record.kind -isnot [string] -or
        $record.failure_stage -isnot [string] -or $record.failure_stage -cne 'none') {
        throw 'Workspace type receipt did not complete its finite semantic witness.'
    }
    foreach ($field in @('exercised', 'capability_supported', 'provider_supported', 'target_unopened',
        'exact_type_name', 'exact_type_uri', 'exact_declaration_range', 'negative_query_empty',
        'resolved_path_exact', 'ordinary_read_exact', 'actual_frontend_navigation',
        'dirty_buffer_reused', 'undo_redo_preserved', 'source_unchanged',
        'root_handle_signaled', 'client_reaped', 'synthetic_root_removed', 'success')) {
        if ($record.$field -isnot [bool] -or -not $record.$field) {
            throw 'Workspace type semantic, frontend or owned-cleanup witness is incomplete.'
        }
    }
    foreach ($field in @('primary_failed', 'cleanup_failed', 'elapsed_saturated')) {
        if ($record.$field -isnot [bool] -or $record.$field) { throw 'Workspace type acceptance failed.' }
    }
    if (($record.elapsed_ms -isnot [int] -and $record.elapsed_ms -isnot [long]) -or
        $record.elapsed_ms -lt 0 -or $record.elapsed_ms -gt 240000) {
        throw 'Workspace type acceptance elapsed time is invalid.'
    }
}
