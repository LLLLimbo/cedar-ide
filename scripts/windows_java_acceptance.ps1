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
    [Environment]::SetEnvironmentVariable($name, $null, 'Process')
}
$env:JAVA_HOME = $jdkRoot
$env:CEDAR_JAVA = $Java
$scratch = Join-Path $ScratchRoot ('cedar windows Java 雪 ' + [Guid]::NewGuid().ToString('N'))
if ([string]::IsNullOrWhiteSpace($EvidencePath)) { $EvidencePath = Join-Path $ScratchRoot 'cedar-windows-java-acceptance.txt' }
$EvidencePath = [IO.Path]::GetFullPath($EvidencePath)
Set-Content -LiteralPath $EvidencePath -Value 'Cedar Windows Java acceptance; missing dependencies or any failed stage fail this run.'
function Record([string] $Text) {
    Write-Output $Text
    Add-Content -LiteralPath $EvidencePath -Value $Text
}
$failure = $null
$stage = 'scratch creation'
$created = $false
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
    & $Java -version 2>&1 | Tee-Object -FilePath $EvidencePath -Append
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
    $distribution = Join-Path $scratch 'JDT distribution 雪'
    New-Item -ItemType Directory -Path $distribution | Out-Null
    $stage = 'verified archive extraction'
    & tar.exe -xzf $archive -C $distribution
    if ($LASTEXITCODE -ne 0) { throw 'Verified JDT archive extraction failed.' }
    if (-not (Test-Path -LiteralPath (Join-Path $distribution 'config_win') -PathType Container)) {
        throw 'Verified distribution lacks config_win.'
    }
    $launchers = @(Get-ChildItem -LiteralPath (Join-Path $distribution 'plugins') -Filter 'org.eclipse.equinox.launcher_*.jar' -File)
    if ($launchers.Count -ne 1) { throw 'Expected exactly one Equinox launcher JAR.' }
    Record ("equinox_launcher=" + $launchers[0].Name)
    # Preserve all upstream notices in the extraction. No JDK/JDT binary is
    # copied into source, release artifacts or the repository's product bundle.
    $stage = 'real Java semantic and lifecycle assertions'
    & cargo run --target $target -p cedar-language --example java_smoke --locked -- $distribution $Java --resolve-imports 2>&1 | Tee-Object -FilePath $EvidencePath -Append
    if ($LASTEXITCODE -ne 0) { throw 'Real Windows Java acceptance failed.' }
}
catch {
    $failure = $_
}
finally {
    # This unique directory is exclusively generated test data. The example
    # joins owned process cleanup before returning; deletion must then succeed.
    if ($created) {
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
Record 'PASS: real Windows Java example completed all assertions and generated dependency scratch was removed.'
