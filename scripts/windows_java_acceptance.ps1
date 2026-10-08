#Requires -Version 7.2
#Requires -PSEdition Core
# Real Windows JDT LS acceptance. External test dependencies are never bundled.
# Run from repository root in an isolated CI/test process, not inside an agent.
# CI supplies an eight-minute process-tree deadline. A direct caller must supply
# its own enclosing deadline; forced cancellation is failure, never cleanup proof.
[CmdletBinding()]
param(
    [string] $Java = "",
    [string] $ScratchRoot = $env:RUNNER_TEMP,
    [string] $EvidencePath = ""
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
# Handle native exit codes explicitly; exception rendering must not echo raw VM output.
$PSNativeCommandUseErrorActionPreference = $false
if (-not $IsWindows) { throw 'This acceptance script requires native Windows.' }
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
$scratch = [IO.Path]::GetFullPath((Join-Path $ScratchRoot ('cedar-windows-java-' + [Guid]::NewGuid().ToString('N'))))
# The runner's native tar has an ANSI command-line boundary. Only extraction
# staging is ASCII; the installed distribution and all runtime fixtures retain
# their Unicode/spaces paths. Do not change machine locale or weaken the probe.
if ($scratch -match '[^\x00-\x7F]') { throw 'Use an ASCII ScratchRoot for native tar staging; runtime Unicode acceptance stays enabled.' }
if ([string]::IsNullOrWhiteSpace($EvidencePath)) { $EvidencePath = Join-Path $ScratchRoot 'cedar-windows-java-acceptance.txt' }
$EvidencePath = [IO.Path]::GetFullPath($EvidencePath)
$CrashEvidencePath = Join-Path (Split-Path $EvidencePath -Parent) 'cedar-java-crash-diagnostics.json'
Set-Content -LiteralPath $EvidencePath -Value 'Cedar Windows Java acceptance; missing dependencies or any failed stage fail this run.'
function Record([string] $Text) {
    Write-Output $Text
    Add-Content -LiteralPath $EvidencePath -Value $Text
}
$failure = $null
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
    if (-not (Test-Path -LiteralPath $env:CEDAR_AGENT_LANGUAGE_VALIDATION_BIN -PathType Leaf)) {
        throw 'Build the separate nonshipping agent language validation fixture before this test.'
    }
    # All protocol/panic/JVM text stays private. Only typed status records survive
    # the collector; a zero-test filter cannot satisfy the witness checks below.
    & cargo test -p cedar-app --all-features --locked real_windows_agent_java_editor_transactions -- --ignored --nocapture --test-threads=1 *> $agentTranscript
    if ($LASTEXITCODE -ne 0) { throw 'Real Windows agent and headless Java acceptance failed.' }
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
                $sessions = @($collected.agent_transcript.evidence.records | Where-Object { $_.kind -ceq 'windows_java_session' })
                $cleanup = @($collected.agent_transcript.evidence.records | Where-Object { $_.kind -ceq 'windows_java_cleanup' })
                $diagnostics = @($collected.agent_transcript.evidence.records | Where-Object { $_.kind -ceq 'windows_java_diagnostics' })
                if ($sessions.Count -ne 3 -or $cleanup.Count -ne 1 -or $diagnostics.Count -ne 6) {
                    throw 'Expected three real agent Java sessions, six diagnostic receipts and one cleanup witness; zero filtered tests cannot pass.'
                }
                $expectedModes = @('initial', 'fresh_data', 'reused_data')
                for ($index = 0; $index -lt 3; $index++) {
                    $record = $sessions[$index]
                    if ($record.session -ne ($index + 1) -or $record.mode -cne $expectedModes[$index] -or
                        -not $record.semantic_checks_passed -or -not $record.source_unchanged -or
                        -not $record.root_identity_verified -or -not $record.jdk_symbol_verified -or
                        -not $record.root_handle_signaled -or $record.root_exit_code -ne 0 -or
                        -not $record.gracefully_exited -or -not $record.versions_2_3_4_synced -or
                        -not $record.correction_change_acknowledged -or
                        $record.correction_change_result -cne 'acknowledged') {
                        throw 'Agent Java session evidence did not prove required semantics, identity and natural zero exit.'
                    }
                }
                for ($index = 0; $index -lt 6; $index++) {
                    $record = $diagnostics[$index]
                    $expectedSession = [Math]::Floor($index / 2) + 1
                    $expectedPhase = if (($index % 2) -eq 0) { 'initial' } else { 'correction' }
                    if ($record.session -ne $expectedSession -or $record.phase -cne $expectedPhase -or
                        $record.result -cne 'matched' -or $record.matching_batches -lt 1 -or
                        $record.counters_saturated -or $record.elapsed_saturated) {
                        throw 'Diagnostic receipts did not prove both exact assertions in each Java session.'
                    }
                }
                if ($cleanup[0].sessions_completed -ne 3 -or -not $cleanup[0].success -or
                    $cleanup[0].primary_failed -or $cleanup[0].cleanup_failed -or
                    $cleanup[0].failure_stage -cne 'none' -or -not $cleanup[0].agent_exit_zero -or
                    -not $cleanup[0].source_unchanged -or -not $cleanup[0].observed_roots_exited -or
                    -not $cleanup[0].synthetic_root_removed) {
                    throw 'Agent Java cleanup evidence did not prove successful completion.'
                }
            }
        }
        catch {
            if ($null -eq $failure) { $stage = 'sanitized crash evidence collection'; $failure = $_ }
            else { Add-Content -LiteralPath $EvidencePath -Value ('Crash evidence collection also failed: ' + $_.Exception.Message) }
        }
        foreach ($name in @('CEDAR_JAVA_ERROR_DIR', 'CEDAR_JDTLS_HOME', 'CEDAR_AGENT_LANGUAGE_VALIDATION_BIN')) {
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
Record 'PASS: direct Java plus real agent/headless editor assertions completed and generated dependency scratch was removed.'
