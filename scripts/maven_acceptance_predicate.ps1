# Pure release predicate. Loading this file launches no process or network call.
function Assert-MavenReceipt([object[]] $Receipts) {
    $matching = @($Receipts | Where-Object { $_.kind -ceq 'windows_java_maven' })
    if ($matching.Count -ne 1) { throw 'Expected one normal-route Maven pair receipt.' }
    $record = $matching[0]
    if ($record -isnot [pscustomobject]) { throw 'Maven receipt must be one object.' }
    foreach ($field in @('kind', 'route', 'source_sha256', 'pom_sha256', 'dependency_jar_sha256')) {
        if ($record.$field -isnot [string]) { throw 'Maven identity field must be a scalar string.' }
    }
    if ($record.schema_version -ne 1 -or $record.route -cne 'normal_agent_normal_client' -or
        $record.pair_count -ne 1 -or $record.cache_input_files -ne 83 -or
        $record.source_sha256 -cne '5dda0de22c2184b1420be8e68f8a37e9165b59658d5c5cbf9fe2ee770a1003e5' -or
        $record.pom_sha256 -cne 'c13116c2a4d7dd73f28f604480b6aad3ce818a11db526b7f61737c1c3864b65b' -or
        $record.dependency_jar_sha256 -cne '82579c654968c77f0bd3d04c28a22b24396c35270ce76d015807410438952b5d') {
        throw 'Maven route, pair or generated-input identity did not match.'
    }
    foreach ($field in @('cache_input_unchanged', 'fixture_inputs_verified', 'success')) {
        if ($record.$field -isnot [bool] -or -not $record.$field) { throw 'Maven pair witness is incomplete.' }
    }
    foreach ($field in @('primary_failed', 'cleanup_failed', 'elapsed_saturated')) {
        if ($record.$field -isnot [bool] -or $record.$field) { throw 'Maven pair failed.' }
    }
    foreach ($field in @('schema_version', 'pair_count', 'cache_input_files', 'elapsed_ms')) {
        if ($record.$field -isnot [int] -and $record.$field -isnot [long]) { throw 'Maven counter type is invalid.' }
    }
    if ($record.elapsed_ms -lt 0 -or $record.elapsed_ms -gt 360000) { throw 'Maven pair exceeded its bound.' }
    foreach ($name in @('present', 'missing')) {
        $case = $record.$name
        if ($null -eq $case -or $case -isnot [pscustomobject]) { throw 'Maven case must be one object.' }
        foreach ($field in @('case', 'failure_stage', 'model_status', 'stop_status', 'stop_reason')) {
            if ($case.$field -isnot [string]) { throw 'Maven case tag must be a scalar string.' }
        }
        if ($case.case -cne $name -or $case.failure_stage -cne 'none') {
            throw 'Maven case did not complete.'
        }
        foreach ($field in @('java_capabilities', 'generic_start_rejected', 'untrusted_start_rejected',
            'model_without_session_rejected', 'async_start_begin_acknowledged', 'async_start_read_while_starting',
            'async_start_ready', 'root_identity_verified', 'root_observed_live', 'maven_nature',
            'custom_source', 'compiler_17', 'exact_dependency_reference', 'stale_startup_rejected', 'changed_pom_restart_required',
            'source_unchanged', 'pom_expected', 'repository_inputs_unchanged', 'cleanup_joined',
            'stop_outcome_verified',
            'root_handle_signaled', 'model_after_stop_rejected', 'client_reaped', 'synthetic_root_removed', 'success')) {
            if ($case.$field -isnot [bool] -or -not $case.$field) { throw 'Maven case witness is incomplete.' }
        }
        foreach ($field in @('primary_failed', 'cleanup_failed')) {
            if ($case.$field -isnot [bool] -or $case.$field) { throw 'Maven case failed.' }
        }
        foreach ($field in @('shutdown_response_received', 'exit_frame_completed')) {
            if ($case.$field -isnot [bool]) { throw 'Maven shutdown witness type is invalid.' }
        }
        foreach ($field in @('model_queries', 'unexpected_dependency_references', 'foreign_repository_files',
            'generated_metadata_files', 'generated_data_files', 'generated_data_bytes',
            'generated_project_files', 'generated_project_bytes', 'root_exit_code')) {
            if ($case.$field -isnot [int] -and $case.$field -isnot [long]) { throw 'Maven case counter type is invalid.' }
            if ($case.$field -lt 0) { throw 'Maven case counter is negative.' }
        }
        if ($case.model_queries -lt 1 -or $case.model_queries -gt 240 -or
            $case.unexpected_dependency_references -ne 0 -or $case.foreign_repository_files -ne 0 -or
            $case.generated_metadata_files -gt 128 -or $case.generated_data_files -lt 1 -or
            $case.generated_data_files -gt 4096 -or $case.generated_data_bytes -lt 1 -or
            $case.generated_data_bytes -gt 134217728 -or $case.generated_project_files -gt 256 -or
            $case.generated_project_bytes -gt 16777216 -or $case.root_exit_code -gt 4294967295) {
            throw 'Maven case bounds or dependency identity failed.'
        }
        if ($case.stop_status -ceq 'graceful') {
            if ($case.stop_reason -cne 'root_exited' -or $case.root_exit_code -ne 0 -or
                -not $case.shutdown_response_received -or -not $case.exit_frame_completed) {
                throw 'Maven graceful stop did not prove a zero root exit.'
            }
        } elseif ($case.stop_status -ceq 'forced') {
            if ($case.stop_reason -cnotin @('grace_expired', 'aborted')) {
                throw 'Maven forced stop reason is invalid.'
            }
        } else { throw 'Maven stop outcome was not verified.' }
        $present = $name -ceq 'present'
        foreach ($field in @('dependency_jar_present_before', 'dependency_jar_present_after',
            'dependency_pom_present_before', 'dependency_pom_present_after')) {
            if ($case.$field -isnot [bool] -or $case.$field -ne $present) {
                throw 'Maven dependency presence or absence was not preserved.'
            }
        }
        if ($present) {
            if ($case.model_status -cne 'imported') { throw 'Maven present model did not import.' }
            foreach ($field in @('hover', 'completion', 'deliberate_type_diagnostic',
                'dirty_change_acknowledged', 'no_autosave', 'changed_pom_restart_required')) {
                if ($case.$field -isnot [bool] -or -not $case.$field) { throw 'Maven present semantics are incomplete.' }
            }
        } else {
            if ($case.model_status -cne 'unresolved' -or $case.offline_pom_diagnostic -isnot [bool] -or
                -not $case.offline_pom_diagnostic) { throw 'Maven missing case did not prove an offline unresolved artifact.' }
        }
    }
    return 1
}
