#Requires -Version 7.2
#Requires -PSEdition Core
# Real Windows JDT LS acceptance. External test dependencies are never bundled.
# Run from repository root in an isolated CI/test process, not inside an agent.
# CI supplies a twelve-minute process-tree deadline for the direct, editor and
# forced-owner and required bounded idle workloads. Historical long observations
# are opt-in. A direct caller must supply
# its own enclosing deadline; forced cancellation is failure, never cleanup proof.
[CmdletBinding()]
param(
    [string] $Java = "",
    [string] $ScratchRoot = $env:RUNNER_TEMP,
    [string] $EvidencePath = "",
    [switch] $GcDiagnosticControl,
    [switch] $LongObservationBaselines,
    [switch] $MavenOnly
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
# Handle native exit codes explicitly; exception rendering must not echo raw VM output.
$PSNativeCommandUseErrorActionPreference = $false
if (-not $IsWindows) { throw 'This acceptance script requires native Windows.' }
if ($MavenOnly -and ($GcDiagnosticControl -or $LongObservationBaselines)) { throw 'Select only one isolated acceptance workload.' }
. (Join-Path $PSScriptRoot 'maven_acceptance_predicate.ps1')
. (Join-Path $PSScriptRoot 'java_workspace_type_acceptance_predicate.ps1')
. (Join-Path $PSScriptRoot 'java_idle_acceptance_predicate.ps1')
if ([string]::IsNullOrWhiteSpace($ScratchRoot)) { throw 'Set an explicit test scratch root.' }
if ([string]::IsNullOrWhiteSpace($Java)) {
    if ([string]::IsNullOrWhiteSpace($env:JAVA_HOME_21_X64)) {
        throw 'Java 21 runner installation is missing; set -Java to an absolute java.exe.'
    }
    $Java = Join-Path $env:JAVA_HOME_21_X64 'bin/java.exe'
}
if (-not [IO.Path]::IsPathFullyQualified($Java)) { throw 'Java path must be absolute.' }
$javaFile = Get-Item -LiteralPath $Java
if ($javaFile.PSIsContainer -or $javaFile.Name -cne 'java.exe') { throw 'Expected native java.exe.' }
$Java = $javaFile.FullName
$jdkRoot = Split-Path (Split-Path $Java -Parent) -Parent
$release = Join-Path $jdkRoot 'release'
if (-not (Test-Path -LiteralPath $release -PathType Leaf)) { throw 'JDK release metadata is missing.' }
$releaseText = Get-Content -LiteralPath $release -Raw
if ($releaseText -notmatch '(?m)^JAVA_VERSION="(?<major>[0-9]+)[^"]*"' -or [int]$Matches.major -lt 21) {
    throw 'JDT LS requires a verified Java 21 or later JDK.'
}
# Prepare this standalone test process before Cargo starts any worker threads.
# Never mutate a running multithreaded agent's process-global environment.
foreach ($name in @('CLIENT_PORT', 'CLIENT_HOST', 'socket.stream.debug', 'JDK_JAVA_OPTIONS', 'JAVA_TOOL_OPTIONS', '_JAVA_OPTIONS')) {
    $environmentPath = 'Env:' + $name
    # PowerShell/.NET can preserve an empty environment value when a null string
    # argument is coerced. Remove via the provider and verify actual absence.
    if (Test-Path -LiteralPath $environmentPath) {
        Remove-Item -LiteralPath $environmentPath
    }
    if (Test-Path -LiteralPath $environmentPath) { throw "Failed to remove $name from the test parent." }
}
$env:JAVA_HOME = $jdkRoot
$env:CEDAR_JAVA = $Java
if ($MavenOnly) {
    foreach ($name in @('MAVEN_OPTS', 'MAVEN_ARGS', 'MAVEN_CONFIG', 'MAVEN_USER_HOME',
        'M2_HOME', 'MAVEN_HOME', 'MAVEN_PROJECTBASEDIR', 'MAVEN_CMD_LINE_ARGS', 'MAVEN_EXT_CLASS_PATH')) {
        $environmentPath = 'Env:' + $name
        if (Test-Path -LiteralPath $environmentPath) { Remove-Item -LiteralPath $environmentPath }
        if (Test-Path -LiteralPath $environmentPath) { throw 'Maven test parent environment was not cleared.' }
    }
}
$scratch = [IO.Path]::GetFullPath((Join-Path $ScratchRoot ('cedar-windows-java-' + [Guid]::NewGuid().ToString('N'))))
# The runner's native tar has an ANSI command-line boundary. Only extraction
# staging is ASCII; the installed distribution and all runtime fixtures retain
# their Unicode/spaces paths. Do not change machine locale or weaken the probe.
if ($scratch -match '[^\x00-\x7F]') { throw 'Use an ASCII ScratchRoot for native tar staging; runtime Unicode acceptance stays enabled.' }
if ([string]::IsNullOrWhiteSpace($EvidencePath)) {
    $evidenceName = if ($MavenOnly) { 'cedar-windows-maven-acceptance.txt' } else { 'cedar-windows-java-acceptance.txt' }
    $EvidencePath = Join-Path $ScratchRoot $evidenceName
}
$EvidencePath = [IO.Path]::GetFullPath($EvidencePath)
$crashName = if ($MavenOnly) { 'cedar-maven-crash-diagnostics.json' } else { 'cedar-java-crash-diagnostics.json' }
$CrashEvidencePath = Join-Path (Split-Path $EvidencePath -Parent) $crashName
$ResourceEvidencePath = Join-Path (Split-Path $EvidencePath -Parent) 'cedar-process-tree-baseline.json'
Set-Content -LiteralPath $EvidencePath -Value 'Cedar Windows Java acceptance; missing dependencies or any failed stage fail this run.'
function Record([string] $Text) {
    Write-Output $Text
    Add-Content -LiteralPath $EvidencePath -Value $Text
}
function Assert-ProductionReceipt(
    [object[]] $Receipts,
    [ValidateSet('windows_java_production', 'windows_java_gc_control')]
    [string] $Kind = 'windows_java_production',
    [switch] $RequireWorkspaceTypes
) {
    $expectedRoute = if ($Kind -ceq 'windows_java_gc_control') { 'diagnostic_agent_normal_client' } else { 'normal_agent_client' }
    $production = @($Receipts | Where-Object { $_.kind -ceq $Kind })
    if ($production.Count -ne 1) { throw 'Expected exactly one receipt for the selected Java acceptance route.' }
    $record = $production[0]
    if ($record.route -cne $expectedRoute -or $record.primary_failed -or $record.cleanup_failed -or
        $record.failure_stage -cne 'none' -or $record.elapsed_saturated -or
        $null -eq $record.root_exit_code -or $record.stop_status -cnotin @('graceful', 'forced')) {
        throw 'Production Java route did not establish bounded semantics and verified owned cleanup.'
    }
    foreach ($field in @('java_capabilities', 'generic_start_rejected', 'untrusted_start_rejected',
        'root_observed_live', 'root_identity_verified', 'semantic_diagnostics', 'exact_definition',
        'real_completion', 'deferred_import_resolve', 'actual_editor_apply_undo_redo', 'versions_2_3_4_synced',
        'correction_acknowledged', 'correction_diagnostics', 'source_unchanged', 'stop_outcome_verified',
        'cleanup_joined', 'root_handle_signaled', 'client_reaped', 'synthetic_root_removed', 'success')) {
        if (-not $record.$field) { throw 'Production Java semantic or ownership witness is incomplete.' }
    }
    if ($record.stop_status -ceq 'graceful' -and ($record.root_exit_code -ne 0 -or
        $record.stop_reason -cne 'root_exited' -or -not $record.shutdown_response_received -or
        -not $record.exit_frame_completed)) {
        throw 'Production Java graceful label lacks matching protocol and root-exit evidence.'
    }
    if ($record.stop_status -ceq 'forced' -and $record.stop_reason -cnotin @('grace_expired', 'aborted')) {
        throw 'Production Java forced label lacks a matching termination reason.'
    }
    if ($Kind -ceq 'windows_java_gc_control' -and $record.stop_status -cne 'graceful') {
        throw 'A forced exit cannot satisfy the diagnostic control natural-shutdown requirement.'
    }
    if ($RequireWorkspaceTypes) {
        if ($Kind -cne 'windows_java_production') { throw 'Workspace type witness requires the shipping Quick route.' }
        Assert-WorkspaceTypeReceipt -Receipts $Receipts
    }
}
function Assert-AgentEditorReceipt([object[]] $Receipts) {
    $sessions = @($Receipts | Where-Object { $_.kind -ceq 'windows_java_session' })
    $cleanup = @($Receipts | Where-Object { $_.kind -ceq 'windows_java_cleanup' })
    $diagnostics = @($Receipts | Where-Object { $_.kind -ceq 'windows_java_diagnostics' })
    $recoveries = @($Receipts | Where-Object { $_.kind -ceq 'windows_java_correction_recovery' })
    if ($sessions.Count -ne 3 -or $cleanup.Count -ne 1 -or $diagnostics.Count -ne 6) {
        throw 'Expected three real agent Java sessions, six diagnostic receipts and one cleanup witness; zero filtered tests cannot pass.'
    }
    $expectedModes = @('initial', 'fresh_data', 'reused_data')
    for ($index = 0; $index -lt 3; $index++) {
        $record = $sessions[$index]
        if ($record.session -ne ($index + 1) -or $record.mode -cne $expectedModes[$index] -or
            -not $record.workflow_success -or -not $record.source_unchanged -or
            -not $record.root_identity_verified -or -not $record.jdk_symbol_verified -or
            -not $record.root_handle_signaled -or $record.root_exit_code -ne 0 -or
            -not $record.gracefully_exited -or -not $record.versions_2_3_4_synced -or
            -not $record.correction_change_acknowledged -or
            $record.correction_change_result -cne 'acknowledged') {
            throw 'Agent Java session evidence did not prove required semantics, identity and natural zero exit.'
        }
        foreach ($field in @('exact_diagnostics', 'exact_definition', 'real_completion',
            'deferred_import_resolve', 'primary_identity_unchanged', 'two_atomic_edits',
            'advisory_command_skipped', 'actual_undo', 'actual_redo', 'versions_2_3_4_synced',
            'correction_change_acknowledged', 'source_unchanged', 'root_observed_live',
            'root_identity_verified', 'jdk_symbol_verified', 'shutdown_api_succeeded',
            'root_handle_signaled', 'gracefully_exited', 'workflow_success')) {
            if (-not $record.$field) { throw 'Agent Java session lost a required editor, semantic or cleanup witness.' }
        }
    }
    $spontaneousTimeouts = 0
    for ($index = 0; $index -lt 6; $index++) {
        $record = $diagnostics[$index]
        $expectedSession = [Math]::Floor($index / 2) + 1
        $expectedPhase = if (($index % 2) -eq 0) { 'initial' } else { 'correction' }
        if ($record.session -ne $expectedSession -or $record.phase -cne $expectedPhase -or
            $record.counters_saturated -or $record.elapsed_saturated) {
            throw 'Original diagnostic receipt identity or bounded counters are invalid.'
        }
        $session = $sessions[$expectedSession - 1]
        $recovery = @($recoveries | Where-Object { $_.session -eq $expectedSession })
        if ($record.result -ceq 'matched' -and $record.matching_batches -ge 1) {
            if ($expectedPhase -ceq 'correction' -and
                (-not $session.correction_diagnostics -or -not $session.semantic_checks_passed -or
                 $recovery.Count -ne 0 -or $session.correction_recovery_result -cne 'not_attempted' -or
                 $session.correction_recovery_attempts -ne 0 -or $session.correction_recovery_acknowledged -or
                 $session.correction_recovery_witness -or $session.correction_recovery_unversioned -or
                 $session.correction_recovery_budget_sufficient)) {
                throw 'Spontaneous correction success disagrees with the separate workflow evidence.'
            }
        }
        elseif ($expectedPhase -ceq 'correction' -and $record.result -ceq 'timeout' -and
            $record.matching_batches -eq 0 -and $record.elapsed_ms -ge 60000) {
            $spontaneousTimeouts++
            if ($session.correction_diagnostics -or $session.semantic_checks_passed -or
                $recovery.Count -ne 1 -or $session.correction_recovery_result -cne 'matched' -or
                $session.correction_recovery_attempts -ne 1 -or -not $session.correction_recovery_acknowledged -or
                -not $session.correction_recovery_witness -or -not $session.correction_recovery_budget_sufficient) {
                throw 'Spontaneous timeout must remain failed and have one supported correction recovery.'
            }
            $refresh = $recovery[0]
            if ($refresh.original_result -cne 'timeout' -or $refresh.result -cne 'matched' -or
                $refresh.attempts -ne 1 -or -not $refresh.acknowledged -or -not $refresh.witness -or
                -not $refresh.budget_sufficient -or $refresh.cleanup_reserve_guaranteed -or
                $refresh.available_budget_ms -lt 165000 -or $refresh.required_budget_ms -ne 165000 -or
                $refresh.request_timeout_ms -ne 75000 -or $refresh.witness_dispatch_window_ms -ne 15000 -or
                $refresh.event_poll_timeout_ms -ne 75000 -or $refresh.elapsed_ms -ge 165000 -or
                $refresh.elapsed_saturated -or $refresh.counters_saturated -or
                $refresh.polls -lt 1 -or $refresh.events -lt 1 -or
                $refresh.unversioned -ne $session.correction_recovery_unversioned) {
                throw 'Explicit correction recovery lacks its exact warning, acknowledgement or existing deadline budget.'
            }
        }
        else {
            throw 'Only a spontaneous correction timeout can use the explicit single-refresh recovery workflow.'
        }
    }
    if ($recoveries.Count -ne $spontaneousTimeouts) {
        throw 'Unexpected or repeated correction recovery receipt.'
    }
    if ($cleanup[0].sessions_completed -ne 3 -or -not $cleanup[0].success -or
        -not $cleanup[0].workflow_success -or
        $cleanup[0].spontaneous_success -ne ($spontaneousTimeouts -eq 0) -or
        $cleanup[0].primary_failed -or $cleanup[0].cleanup_failed -or
        $cleanup[0].failure_stage -cne 'none' -or -not $cleanup[0].agent_exit_zero -or
        -not $cleanup[0].source_unchanged -or -not $cleanup[0].observed_roots_exited -or
        -not $cleanup[0].synthetic_root_removed) {
        throw 'Agent Java cleanup evidence did not prove successful completion.'
    }
    return $spontaneousTimeouts
}
$failure = $null
$spontaneousTimeouts = 0
$idleSpontaneousTimeouts = 0
$stage = 'scratch creation'
$created = $false
$probeReport = Join-Path $scratch 'owned-java-probe-private.json'
$javaTranscript = Join-Path $scratch 'real-java-private.txt'
$agentTranscript = Join-Path $scratch 'agent-java-editor-private.txt'
try {
    New-Item -ItemType Directory -Path $scratch | Out-Null
    $created = $true
    $stage = 'source and toolchain metadata'
    $sha = (& git rev-parse HEAD)
    if ($LASTEXITCODE -ne 0) { throw 'Cannot determine source commit.' }
    $dirty = (& git status --porcelain)
    if ($LASTEXITCODE -ne 0) { throw 'Cannot inspect checkout state.' }
    Record ("source_commit=$sha; checkout_dirty=" + [bool]$dirty)
    $rust = (& rustc -vV)
    if ($LASTEXITCODE -ne 0) { throw 'Cannot inspect Rust toolchain.' }
    foreach ($line in $rust) { Record $line }
    $hostLine = @($rust | Where-Object { $_ -match '^host: ' })
    if ($hostLine.Count -ne 1) { throw 'Cannot determine Rust host target.' }
    $target = $hostLine[0].Substring(6)
    if ($target -notmatch '-pc-windows-msvc$') { throw 'Acceptance requires a native Windows MSVC Rust host.' }
    Record ("rust_target=" + $target)
    Record ("OS=" + [Environment]::OSVersion.VersionString)
    Record ("runner_image=" + $env:ImageOS + '; image_version=' + $env:ImageVersion)
    Record ("java_executable=" + $Java)
    # Public JDK metadata only. Never dump inherited environment or credentials.
    foreach ($line in ($releaseText -split "`r?`n")) {
        if ($line -match '^(JAVA_VERSION|IMPLEMENTOR|IMPLEMENTOR_VERSION|JAVA_RUNTIME_VERSION|OS_ARCH)=') { Record $line }
    }
    $stage = 'Java executable verification'
    & $Java -version *> (Join-Path $scratch 'java-version-private.txt')
    if ($LASTEXITCODE -ne 0) { throw 'Selected Java executable failed.' }
    $archiveUrl = 'https://download.eclipse.org/jdtls/milestones/1.61.0/jdt-language-server-1.61.0-202609031315.tar.gz'
    $expectedSha256 = '338e7e73d61836651ba2453919a0d34fa763eb4e7c03342092309bffb8934c64'
    $archive = Join-Path $scratch 'jdtls-1.61.0.tar.gz'
    $stage = 'pinned JDT archive download and verification'
    $download = @{ Uri = $archiveUrl; OutFile = $archive; TimeoutSec = 120 }
    # PowerShell 7.4+ separates connection and stalled-read timeouts. The CI
    # deadline remains the overall wall-clock bound, including retries/cleanup.
    if ((Get-Command Invoke-WebRequest).Parameters.ContainsKey('OperationTimeoutSeconds')) {
        $download.OperationTimeoutSeconds = 120
    }
    Invoke-WebRequest @download
    $actualSha256 = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actualSha256 -cne $expectedSha256) { throw 'JDT archive checksum mismatch; refusing extraction or execution.' }
    Record ("jdt_archive=$archiveUrl; sha256=$actualSha256")
    $staging = Join-Path $scratch 'distribution-staging'
    New-Item -ItemType Directory -Path $staging | Out-Null
    $stage = 'verified archive extraction in ASCII staging'
    & tar.exe -xzf $archive -C $staging
    if ($LASTEXITCODE -ne 0) { throw 'Verified JDT archive extraction failed.' }
    $stagedLaunchers = @(Get-ChildItem -LiteralPath (Join-Path $staging 'plugins') -Filter 'org.eclipse.equinox.launcher_*.jar' -File)
    if ($stagedLaunchers.Count -ne 1) { throw 'Expected one Equinox launcher in verified extraction.' }
    $stagedLauncherName = $stagedLaunchers[0].Name
    $stagedLauncherHash = (Get-FileHash -LiteralPath $stagedLaunchers[0].FullName -Algorithm SHA256).Hash
    $stage = 'move verified extraction to Unicode runtime directory'
    $distribution = Join-Path $scratch 'JDT distribution 雪'
    # PowerShell's filesystem provider uses Unicode-safe APIs. The native tar
    # never receives the Unicode destination; Java and Cedar still must handle it.
    Move-Item -LiteralPath $staging -Destination $distribution
    if ((Test-Path -LiteralPath $staging) -or -not (Test-Path -LiteralPath $distribution -PathType Container)) {
        throw 'Unicode distribution move did not establish the expected directory.'
    }
    Record ("runtime_distribution=" + $distribution + '; extraction_staging_ascii=true')
    if (-not (Test-Path -LiteralPath (Join-Path $distribution 'config_win') -PathType Container)) {
        throw 'Verified distribution lacks config_win.'
    }
    $launchers = @(Get-ChildItem -LiteralPath (Join-Path $distribution 'plugins') -Filter 'org.eclipse.equinox.launcher_*.jar' -File)
    if ($launchers.Count -ne 1) { throw 'Expected exactly one Equinox launcher JAR.' }
    $launcherHash = (Get-FileHash -LiteralPath $launchers[0].FullName -Algorithm SHA256).Hash
    if ($launchers[0].Name -cne $stagedLauncherName -or $launcherHash -cne $stagedLauncherHash) {
        throw 'Unicode move changed the verified launcher.'
    }
    Record ("equinox_launcher=" + $launchers[0].Name + '; sha256=' + $launcherHash.ToLowerInvariant())
    # Preserve all upstream notices in the extraction. No JDK/JDT binary is
    # copied into source, release artifacts or the repository's product bundle.
    if ($MavenOnly) {
        $stage = 'pinned Maven test cache preparation before offline import'
        $env:CEDAR_MAVEN_CACHE_INPUT = Join-Path $scratch 'maven-cache-input'
        $cacheReport = Join-Path $scratch 'maven-cache-private.json'
        & python scripts/prepare_maven_cache.py --destination $env:CEDAR_MAVEN_CACHE_INPUT *> $cacheReport
        if ($LASTEXITCODE -ne 0) { throw 'Pinned Maven cache preparation failed; no import was attempted.' }
        $cache = Get-Content -LiteralPath $cacheReport -Raw | ConvertFrom-Json
        if ($cache.status -cne 'complete' -or $cache.artifact_files -ne 83 -or
            $cache.artifact_bytes -ne 4065288 -or $cache.network_requests -ne 83 -or
            -not $cache.exact_inventory_verified -or $cache.execution_performed -or $cache.import_performed -or
            $cache.manifest_sha256 -cne '2afceba6a8f6b648a1dbf48cc356b931233cc57bc82b52d02fefdd58e5e876ac') {
            throw 'Pinned Maven cache identity or scope was not verified.'
        }
        Record 'Maven CI cache: 83 exact files / 4065288 bytes; registry preparation separate from offline dependency resolution; signatures unverified.'
        $env:CEDAR_JDTLS_HOME = $distribution
        $env:CEDAR_AGENT_BIN = [IO.Path]::GetFullPath('target/release/cedar-agent.exe')
        if (-not (Test-Path -LiteralPath $env:CEDAR_AGENT_BIN -PathType Leaf)) {
            throw 'Build the normal shipping agent before Maven acceptance.'
        }
        $stage = 'normal production Maven present and missing pair'
        & cargo test -p cedar-app --lib --all-features --locked real_windows_normal_agent_java_maven_acceptance -- --ignored --nocapture --test-threads=1 *> $agentTranscript
        if ($LASTEXITCODE -ne 0) { throw 'Normal Windows Maven pair failed; inspect sanitized case witnesses.' }
    } else {
    $stage = 'owned JVM raw stdio diagnostic matrix'
    $probeRoot = Join-Path $scratch 'owned-java-probes'
    New-Item -ItemType Directory -Path $probeRoot | Out-Null
    # Child output can contain fatal VM environment/register dumps. Never stream
    # it to the console or a public artifact, even if ErrorFile creation fails.
    & cargo run --target $target -p cedar-language --example windows_java_probe --locked -- $Java $probeRoot $distribution 1> $probeReport 2> (Join-Path $scratch 'owned-java-probe-stderr-private.txt')
    if ($LASTEXITCODE -ne 0) { throw 'Owned JVM diagnostic driver could not complete collection.' }
    # Driver completion is not a claim that its child cases succeeded. Keep the
    # real JDT assertions as the acceptance gate and capture its own fatal log.
    $realErrors = Join-Path $scratch 'real-jdt-errors'
    New-Item -ItemType Directory -Path $realErrors | Out-Null
    $env:CEDAR_JAVA_ERROR_DIR = $realErrors
    $stage = 'real Java semantic and lifecycle assertions'
    & cargo run --target $target -p cedar-language --example java_smoke --locked -- $distribution $Java --resolve-imports *> $javaTranscript
    if ($LASTEXITCODE -ne 0) { throw 'Real Windows Java acceptance failed.' }
    $stage = 'real agent and headless Java editor assertions'
    $agentErrors = Join-Path $scratch 'agent-jdt-errors'
    New-Item -ItemType Directory -Path $agentErrors | Out-Null
    $env:CEDAR_JAVA_ERROR_DIR = $agentErrors
    $env:CEDAR_JDTLS_HOME = $distribution
    $env:CEDAR_AGENT_LANGUAGE_VALIDATION_BIN = [IO.Path]::GetFullPath('target/release/cedar-agent-language-validation.exe')
    $env:CEDAR_AGENT_BIN = [IO.Path]::GetFullPath('target/release/cedar-agent.exe')
    if (-not (Test-Path -LiteralPath $env:CEDAR_AGENT_BIN -PathType Leaf)) {
        throw 'Build the normal shipping agent before production Java acceptance.'
    }
    $env:CEDAR_WINPROCESS_FIXTURE_BIN = [IO.Path]::GetFullPath('target/release/cedar-winprocess-fixture.exe')
    if (-not (Test-Path -LiteralPath $env:CEDAR_WINPROCESS_FIXTURE_BIN -PathType Leaf)) {
        throw 'Build the native owned task fixture before this test.'
    }
    if (-not (Test-Path -LiteralPath $env:CEDAR_AGENT_LANGUAGE_VALIDATION_BIN -PathType Leaf)) {
        throw 'Build the separate nonshipping agent language validation fixture before this test.'
    }
    # All protocol/panic/JVM text stays private. Only typed status records survive
    # the collector; a zero-test filter cannot satisfy the witness checks below.
    & cargo test -p cedar-app --all-features --locked real_windows_agent_java_editor_transactions -- --ignored --nocapture --test-threads=1 *> $agentTranscript
    $editorExitCode = $LASTEXITCODE
    # This independent ownership case must still execute if an editor assertion
    # fails. Both exit codes remain required; neither failure is masked.
    & cargo test -p cedar-app --all-features --locked real_windows_agent_java_forced_owner_cleanup -- --ignored --nocapture --test-threads=1 *>> $agentTranscript
    $forcedExitCode = $LASTEXITCODE
    # Required normal shipping Client workflow: the fixed initial idle and any
    # one timeout-only refresh have their own receipt, outside resource sampling.
    $stage = 'required bounded normal production idle correction workflow'
    & cargo test -p cedar-app --lib --all-features --locked real_windows_normal_agent_java_idle_correction_acceptance -- --ignored --nocapture --test-threads=1 *>> $agentTranscript
    $idleExitCode = $LASTEXITCODE
    # Build outside the measured interval and reuse this same production JVM run.
    # Cargo/compiler, the observer and earlier validation runs are not included.
    $stage = 'build normal production Java test driver for resource observation'
    $testArtifacts = Join-Path $scratch 'normal-java-test-artifacts-private.jsonl'
    & cargo test -p cedar-app --lib --all-features --locked --no-run --message-format=json 1> $testArtifacts 2>> $agentTranscript
    if ($LASTEXITCODE -ne 0) { throw 'Could not build the normal production Java test driver.' }
    $testDrivers = @(Get-Content -LiteralPath $testArtifacts | ForEach-Object { $_ | ConvertFrom-Json } |
        Where-Object { $_.reason -ceq 'compiler-artifact' -and $_.target.name -ceq 'cedar_app' -and
            $_.profile.test -and $null -ne $_.executable })
    if ($testDrivers.Count -ne 1) { throw 'Expected one exact prebuilt cedar-app library test driver.' }
    $stage = 'normal production Java acceptance with observational process-tree baseline'
    # The sampler publishes fixed typed fields only. Raw child output remains in
    # the private transcript, and the original test failure code is propagated.
    & python scripts/measure_process_tree.py --driver $testDrivers[0].executable `
        --agent $env:CEDAR_AGENT_BIN --java $Java `
        --phase-file (Join-Path $scratch 'resource-phases-private.jsonl') `
        --transcript $agentTranscript --output $ResourceEvidencePath --source-commit $sha
    $productionExitCode = $LASTEXITCODE
    if ($editorExitCode -ne 0 -or $forcedExitCode -ne 0 -or $idleExitCode -ne 0 -or $productionExitCode -ne 0) {
        throw 'Real Windows Java editor, forced-owner, required idle or production-route acceptance failed.'
    }
    # The original pair remains available unchanged for explicit observations,
    # and remains part of the existing GC diagnostic control recipe.
    if ($LongObservationBaselines -or $GcDiagnosticControl) {
        # Two separately observed, unchanged-recipe baselines. Each raw transcript
        # stays inside generated scratch and must independently prove the same normal
        # production semantics and cleanup. Resource values are not pass thresholds.
        $longReports = @()
        for ($trial = 1; $trial -le 2; $trial++) {
            $stage = "long observation baseline trial $trial"
            $trialRoot = Join-Path $scratch ("long-idle-trial-$trial")
            New-Item -ItemType Directory -Path $trialRoot | Out-Null
            $trialTranscript = Join-Path $trialRoot 'agent-transcript-private.txt'
            $trialReport = Join-Path (Split-Path $EvidencePath -Parent) ("cedar-java-long-idle-trial-$trial.json")
            $trialReceipt = Join-Path (Split-Path $EvidencePath -Parent) ("cedar-java-long-idle-acceptance-$trial.json")
            $trialExitCode = 1
            $trialCollectorExitCode = 1
            try {
                & python scripts/measure_process_tree.py --driver $testDrivers[0].executable `
                    --agent $env:CEDAR_AGENT_BIN --java $Java --workload long_idle_baseline --trial $trial `
                    --phase-file (Join-Path $trialRoot 'phases-private.jsonl') `
                    --transcript $trialTranscript --output $trialReport --source-commit $sha
                $trialExitCode = $LASTEXITCODE
            }
            finally {
                # This collector reconstructs bounded typed receipts; never upload
                # the private transcript, JVM output or marker file itself.
                & python scripts/collect_java_crash.py --root $trialRoot --output $trialReceipt --agent-transcript $trialTranscript
                $trialCollectorExitCode = $LASTEXITCODE
            }
            Record ("baseline_trial=$trial; test_exit_code=$trialExitCode; collector_exit_code=$trialCollectorExitCode")
            if ($trialExitCode -ne 0 -or $trialCollectorExitCode -ne 0) {
                throw 'Long observation baseline or its sanitized receipt collection failed.'
            }
            $trialCollected = Get-Content -LiteralPath $trialReceipt -Raw | ConvertFrom-Json
            Assert-ProductionReceipt -Receipts @($trialCollected.agent_transcript.evidence.records)
            $longReports += $trialReport
            Record ("PASS: long observation trial $trial preserved normal Java semantics and owned cleanup.")
        }
        $stage = 'compare unchanged-recipe long observation trials'
        $comparisonReport = Join-Path (Split-Path $EvidencePath -Parent) 'cedar-java-long-idle-comparison.json'
        & python scripts/measure_process_tree.py compare --trial-1 $longReports[0] --trial-2 $longReports[1] --output $comparisonReport
        if ($LASTEXITCODE -ne 0) { throw 'Long observation reports were malformed or not comparable.' }
    } else {
        Record 'Long observation baselines not requested; no long resource trials or comparison executed.'
    }
    if ($GcDiagnosticControl) {
        # This checkpoint opts into one diagnostic host invocation. The ordinary
        # script defaults to no GC diagnostic; no shipping launch flag changes.
        $stage = 'single fixed GC diagnostic control setup'
        $gcBinary = [IO.Path]::GetFullPath('target/release/cedar-agent-java-gc-diagnostic.exe')
        if (-not (Test-Path -LiteralPath $gcBinary -PathType Leaf)) {
            throw 'Build the separate fixed GC diagnostic host before opting in.'
        }
        $shippingHash = (Get-FileHash -LiteralPath $env:CEDAR_AGENT_BIN -Algorithm SHA256).Hash
        $gcMarker = Join-Path $distribution '.cedar-windows-java-gc-diagnostic-distribution'
        $markerBytes = [Text.Encoding]::UTF8.GetBytes("cedar-windows-java-gc-diagnostic-distribution-v1`nsynthetic-data-only`n")
        $markerStream = [IO.File]::Open($gcMarker, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
        try { $markerStream.Write($markerBytes, 0, $markerBytes.Length) }
        finally { $markerStream.Dispose() }
        $gcRoot = Join-Path $scratch 'gc-control-private'
        New-Item -ItemType Directory -Path $gcRoot | Out-Null
        $gcTranscript = Join-Path $gcRoot 'agent-transcript-private.txt'
        $gcSelection = Join-Path $distribution '.cedar-java-gc-selection-private.json'
        $gcResourceReport = Join-Path (Split-Path $EvidencePath -Parent) 'cedar-java-gc-control-resources.json'
        $gcReceiptReport = Join-Path (Split-Path $EvidencePath -Parent) 'cedar-java-gc-control-acceptance.json'
        $gcNumericReport = Join-Path (Split-Path $EvidencePath -Parent) 'cedar-java-gc-control-numeric.json'
        $gcExitCode = 1
        $gcReceiptExitCode = 1
        try {
            $stage = 'single fixed GC diagnostic control execution'
            & python scripts/measure_process_tree.py --driver $testDrivers[0].executable `
                --agent $gcBinary --java $Java --workload gc_diagnostic_control `
                --gc-owned-root $distribution --gc-selection $gcSelection `
                --phase-file (Join-Path $gcRoot 'phases-private.jsonl') `
                --transcript $gcTranscript --output $gcResourceReport --source-commit $sha
            $gcExitCode = $LASTEXITCODE
        }
        finally {
            & python scripts/collect_java_crash.py --root $gcRoot --output $gcReceiptReport --agent-transcript $gcTranscript
            $gcReceiptExitCode = $LASTEXITCODE
            $afterShippingHash = (Get-FileHash -LiteralPath $env:CEDAR_AGENT_BIN -Algorithm SHA256).Hash
            if ($afterShippingHash -cne $shippingHash) {
                throw 'The diagnostic control changed the shipping agent artifact.'
            }
        }
        Record ("gc_control_test_exit=$gcExitCode; gc_control_receipt_exit=$gcReceiptExitCode; shipping_agent_unchanged=true")
        if ($gcExitCode -ne 0 -or $gcReceiptExitCode -ne 0) {
            throw 'The single diagnostic control or sanitized semantic receipt failed.'
        }
        $gcCollected = Get-Content -LiteralPath $gcReceiptReport -Raw | ConvertFrom-Json
        Assert-ProductionReceipt -Receipts @($gcCollected.agent_transcript.evidence.records) -Kind windows_java_gc_control
        $gcObserved = Get-Content -LiteralPath $gcResourceReport -Raw | ConvertFrom-Json
        $corroboration = $gcObserved.gc_selection_corroboration
        if ($corroboration.status -cne 'corroborated' -or -not $corroboration.identity_corroborated -or
            $corroboration.selection_sha256 -cnotmatch '^[a-f0-9]{64}$') {
            throw 'Owned sampler identity did not corroborate the private GC selection witness.'
        }
        $stage = 'bounded numeric GC evidence collection'
        # The exact witness digest binds this reopen to the sampler's retained
        # JVM identity. Raw files and collector stderr remain private scratch.
        & python scripts/collect_gc_control.py --owned-root $distribution --selection $gcSelection `
            --expected-selection-sha256 $corroboration.selection_sha256 `
            1> $gcNumericReport 2> (Join-Path $gcRoot 'collector-stderr-private.txt')
        $gcNumericExitCode = $LASTEXITCODE
        $gcNumbers = Get-Content -LiteralPath $gcNumericReport -Raw | ConvertFrom-Json
        if (-not $gcNumbers.selection_binding_verified -or
            $gcNumbers.selection_sha256 -cne $corroboration.selection_sha256 -or
            $gcNumbers.status -cnotin @('complete', 'partial') -or
            ($gcNumbers.status -ceq 'complete' -and $gcNumericExitCode -ne 0) -or
            ($gcNumbers.status -ceq 'partial' -and $gcNumericExitCode -ne 1) -or
            $gcNumbers.collector -cnotin @('g1', 'serial', 'parallel', 'zgc', 'shenandoah', 'epsilon', 'unknown') -or
            $gcNumbers.heap_observation -cnotin @('observed_gc_points', 'not_observed')) {
            throw 'GC numeric evidence was rejected or did not retain the exact corroborated witness.'
        }
        Record ('GC diagnostic numeric status=' + $gcNumbers.status + '; collector=' + $gcNumbers.collector +
            '; heap_observation=' + $gcNumbers.heap_observation + '; no_tuning_conclusion=true')
    }
    }
}
catch {
    $failure = $_
}
finally {
    # This unique directory is exclusively generated test data. The example
    # joins owned process cleanup before returning; deletion must then succeed.
    if ($created) {
        # Preserve only whitelisted fatal headers/frames and hashes before
        # deleting scratch. Never upload raw hs_err, environment or minidumps.
        try {
            $collectorArgs = @('scripts/collect_java_crash.py', '--root', $scratch, '--output', $CrashEvidencePath)
            if (Test-Path -LiteralPath $probeReport -PathType Leaf) { $collectorArgs += @('--probe-report', $probeReport) }
            if (Test-Path -LiteralPath $javaTranscript -PathType Leaf) { $collectorArgs += @('--java-transcript', $javaTranscript) }
            if (Test-Path -LiteralPath $agentTranscript -PathType Leaf) { $collectorArgs += @('--agent-transcript', $agentTranscript) }
            & python @collectorArgs
            if ($LASTEXITCODE -ne 0) { throw 'JVM crash diagnostic collection was incomplete; inspect the JSON status.' }
            if ($null -eq $failure) {
                $collected = Get-Content -LiteralPath $CrashEvidencePath -Raw | ConvertFrom-Json
                if ($MavenOnly) {
                    $null = Assert-MavenReceipt -Receipts @($collected.agent_transcript.evidence.records)
                } else {
                $spontaneousTimeouts = Assert-AgentEditorReceipt -Receipts @($collected.agent_transcript.evidence.records)
                $idleSpontaneousTimeouts = Assert-IdleCorrectionReceipt -Receipts @($collected.agent_transcript.evidence.records)
                Record ("Required idle workflow verified; original spontaneous timeout count=$idleSpontaneousTimeouts; successful refresh preserves original correction_diagnostics=false.")
                $concurrency = @($collected.agent_transcript.evidence.records | Where-Object { $_.kind -ceq 'windows_java_concurrency' })
                $forced = @($collected.agent_transcript.evidence.records | Where-Object { $_.kind -ceq 'windows_java_forced_cleanup' })
                if ($concurrency.Count -ne 1 -or $forced.Count -ne 1) {
                    throw 'Expected one task-concurrency and one forced-owner receipt.'
                }
                $record = $concurrency[0]
                if ($record.tasks_started -ne 2 -or $record.tasks_completed -ne 2 -or
                    $record.primary_failed -or $record.cleanup_failed -or $record.failure_stage -cne 'none') {
                    throw 'Real Java task-concurrency did not complete both independent task lifetimes.'
                }
                foreach ($field in @('language_stop_preserved_task', 'task_cancel_preserved_java', 'hover_after_cancel',
                    'task_identities_verified', 'task_locks_verified', 'tasks_exited', 'task_locks_released',
                    'task_caps_not_reached', 'source_unchanged', 'success')) {
                    if (-not $record.$field) { throw 'Real Java task-concurrency witness is incomplete.' }
                }
                $record = $forced[0]
                if ($record.primary_failed -or $record.cleanup_failed -or $record.failure_stage -cne 'none' -or
                    $record.elapsed_saturated -or $null -eq $record.java_exit_code -or
                    $null -eq $record.task_exit_code -or $record.task_exit_code -eq 124 -or
                    $record.task_exit_code -eq 125) {
                    throw 'Forced-owner cleanup did not establish bounded owned termination.'
                }
                foreach ($field in @('java_observed_live', 'java_identity_verified', 'task_observed_live',
                    'task_identity_verified', 'task_lock_verified', 'owner_death_injected', 'agent_exit_observed',
                    'agent_exit_nonzero', 'java_exit_observed', 'task_exit_observed', 'task_lock_released',
                    'task_cap_not_reached', 'source_unchanged', 'synthetic_root_removed', 'success')) {
                    if (-not $record.$field) { throw 'Forced-owner cleanup witness is incomplete.' }
                }
                Assert-ProductionReceipt -Receipts @($collected.agent_transcript.evidence.records) -RequireWorkspaceTypes
                }
            }
        }
        catch {
            if ($null -eq $failure) { $stage = 'sanitized crash evidence collection'; $failure = $_ }
            else { Add-Content -LiteralPath $EvidencePath -Value ('Crash evidence collection also failed: ' + $_.Exception.Message) }
        }
        foreach ($name in @('CEDAR_JAVA_ERROR_DIR', 'CEDAR_JDTLS_HOME', 'CEDAR_AGENT_LANGUAGE_VALIDATION_BIN', 'CEDAR_WINPROCESS_FIXTURE_BIN', 'CEDAR_AGENT_BIN', 'CEDAR_MAVEN_CACHE_INPUT')) {
            $environmentPath = 'Env:' + $name
            if (Test-Path -LiteralPath $environmentPath) { Remove-Item -LiteralPath $environmentPath }
        }
        if ($null -eq $failure) { $stage = 'generated dependency cleanup' }
        try {
            Remove-Item -LiteralPath $scratch -Recurse -Force
        }
        catch {
            if ($null -eq $failure) { $failure = $_ }
            else {
                $cleanupFailure = 'Cleanup also failed: ' + $_.Exception.Message
                Write-Warning $cleanupFailure
                Add-Content -LiteralPath $EvidencePath -Value $cleanupFailure
            }
        }
    }
}
if ($null -ne $failure) {
    Record ('FAIL during ' + $stage + ': ' + $failure.Exception.Message)
    throw $failure
}
if ($MavenOnly) {
    Record 'PASS: normal Maven present/missing model, semantic/error and owned-cleanup witnesses verified. Offline Maven resolution is not network isolation; JDT may request public Gradle version metadata. Generated dependency scratch was removed.'
} else {
    Record ("PASS: supported Java workflow and owned cleanup completed; legacy spontaneous correction timeouts: $spontaneousTimeouts; required idle spontaneous correction timeouts: $idleSpontaneousTimeouts; each retained timeout used exactly one explicit refresh with an exact warning witness. Generated dependency scratch was removed.")
}
