#Requires -Version 7.2
#Requires -PSEdition Core
# Package only normal release binaries, then verify the extracted Local route.
# CI provides a three-minute enclosing failure boundary; it is not cleanup proof.
[CmdletBinding()]
param(
    [string] $ScratchRoot = $env:RUNNER_TEMP,
    [string] $SourceCommit = $env:GITHUB_SHA,
    [string] $CiRunUrl = "https://github.com/LLLLimbo/cedar-ide/actions/runs/$($env:GITHUB_RUN_ID)"
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false
if (-not $IsWindows) { throw 'Native Windows is required.' }
if ([string]::IsNullOrWhiteSpace($ScratchRoot)) { throw 'An existing scratch root is required.' }
if ($SourceCommit -cnotmatch '^[0-9a-f]{40}$') { throw 'An exact source SHA is required.' }
if ($CiRunUrl -cnotmatch '^https://github\.com/LLLLimbo/cedar-ide/actions/runs/[1-9][0-9]*$') {
    throw 'The source project CI run URL is required.'
}
$repo = (Get-Location).Path
$scratch = [IO.Path]::GetFullPath((Join-Path $ScratchRoot ('cedar-bundle-' + [Guid]::NewGuid().ToString('N'))))
$output = Join-Path $ScratchRoot 'cedar-windows-development'
if (Test-Path -LiteralPath $output) { throw 'Bundle output directory already exists.' }
[IO.Directory]::CreateDirectory($output) | Out-Null
[IO.Directory]::CreateDirectory($scratch) | Out-Null
$utf8 = [Text.UTF8Encoding]::new($false)
$completed = $false

function Assert-ExtractedBundle([string] $Directory, $Manifest) {
    $expected = @('BUNDLE_MANIFEST.json') + @($Manifest.files | ForEach-Object { $_.path })
    $actual = @(Get-ChildItem -LiteralPath $Directory -Recurse -Force | ForEach-Object {
        if ($_.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Extracted reparse point.' }
        if (-not $_.PSIsContainer) { [IO.Path]::GetRelativePath($Directory, $_.FullName).Replace('\', '/') }
    })
    if (@(Compare-Object ($expected | Sort-Object) ($actual | Sort-Object)).Count -ne 0) {
        throw 'Extracted inventory differs from the package.'
    }
    foreach ($file in $Manifest.files) {
        $path = Join-Path $Directory $file.path
        if ((Get-Item -LiteralPath $path).Length -ne $file.bytes -or
            (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() -cne $file.sha256) {
            throw 'Extracted payload hash differs from the package.'
        }
    }
}

try {
    # Read the version from the checked-in Cargo manifest with the same parser as build.
    $version = & python -c 'import pathlib,tomllib; print(tomllib.loads(pathlib.Path("Cargo.toml").read_text())["workspace"]["package"]["version"])'
    if ($LASTEXITCODE -ne 0 -or $version -cnotmatch '^[0-9]+\.[0-9]+\.[0-9]+$') { throw 'Invalid Cargo version.' }
    $name = "cedar-$version-windows-x86_64-$($SourceCommit.Substring(0,12)).zip"
    $zip = Join-Path $output $name
    $builtText = & python scripts/package_windows_bundle.py build --source-commit $SourceCommit --ci-run-url $CiRunUrl --output $zip
    if ($LASTEXITCODE -ne 0) { throw 'Bundle creation failed.' }
    $built = $builtText | ConvertFrom-Json
    $extracted = Join-Path $scratch 'extracted Cedar 雪 with spaces'
    $verifiedText = & python scripts/package_windows_bundle.py verify $zip --source-commit $SourceCommit --ci-run-url $CiRunUrl --extract-to $extracted
    if ($LASTEXITCODE -ne 0) { throw 'Bundle verification/extraction failed.' }
    $verified = $verifiedText | ConvertFrom-Json
    if ($verified.sha256 -cne $built.sha256 -or $verified.source_commit -cne $SourceCommit -or
        $verified.version -cne $version -or $verified.ci_run_url -cne $CiRunUrl) { throw 'Bundle identity mismatch.' }
    $manifestPath = Join-Path $extracted 'BUNDLE_MANIFEST.json'
    $manifestHash = (Get-FileHash -LiteralPath $manifestPath -Algorithm SHA256).Hash
    $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
    Assert-ExtractedBundle $extracted $manifest

    $workspace = Join-Path $scratch 'synthetic project 雪'
    [IO.Directory]::CreateDirectory((Join-Path $workspace 'portable files 雪')) | Out-Null
    [IO.File]::WriteAllText((Join-Path $workspace '.cedar-portable-probe'), "cedar-portable-probe-v1`n", $utf8)
    $source = Join-Path $workspace 'portable files 雪/hello café.txt'
    [IO.File]::WriteAllText($source, "cedar-portable-original-v1`n", $utf8)
    $probe = Join-Path $extracted 'cedar-client-bundle-probe.exe'
    Copy-Item -LiteralPath (Join-Path $repo 'target/release/cedar-client-bundle-probe.exe') -Destination $probe
    $probeText = & $probe portable $workspace
    if ($LASTEXITCODE -ne 0) { throw 'Extracted bundle file probe failed.' }
    $receipt = $probeText | ConvertFrom-Json
    $checks = @('metadata_verified', 'trust_off', 'list_verified', 'read_verified', 'write_verified',
        'readback_verified', 'search_verified', 'stale_write_rejected', 'execution_rejected',
        'java_rejected', 'maven_rejected', 'workspace_symbols_rejected', 'java_implementations_rejected', 'client_reaped')
    $keys = @('kind', 'schema_version', 'status') + $checks
    if (@(Compare-Object ($keys | Sort-Object) (@($receipt.PSObject.Properties.Name) | Sort-Object)).Count -ne 0 -or
        $receipt.kind -cne 'cedar_windows_bundle_probe' -or $receipt.schema_version -ne 1 -or
        $receipt.status -cne 'success') { throw 'Unexpected bundle probe receipt.' }
    foreach ($check in $checks) {
        if ($receipt.$check -isnot [bool] -or -not $receipt.$check) { throw 'Incomplete bundle probe receipt.' }
    }
    if ([IO.File]::ReadAllText($source, $utf8) -cne "cedar-portable-saved-v1`n") { throw 'Saved bytes differ.' }
    Remove-Item -LiteralPath $probe
    Assert-ExtractedBundle $extracted $manifest
    if ((Get-FileHash -LiteralPath $manifestPath -Algorithm SHA256).Hash -cne $manifestHash -or
        (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash.ToLowerInvariant() -cne $built.sha256) {
        throw 'Package changed during verification.'
    }
    $completed = $true
} finally {
    # Only this invocation's generated scratch tree; ZIP and receipt stay outside it.
    Remove-Item -LiteralPath $scratch -Recurse -Force
    if (Test-Path -LiteralPath $scratch) { throw 'Bundle scratch cleanup failed.' }
}
if (-not $completed) { throw 'Bundle verification did not complete.' }
$result = [ordered]@{
    schema_version = 1
    kind = 'cedar_windows_development_bundle'
    status = 'success'
    version = $version
    source_commit = $SourceCommit
    ci_run_url = $CiRunUrl
    archive = $name
    archive_bytes = $built.bytes
    archive_sha256 = $built.sha256
    file_count = $built.file_count
    unsigned = $true
    unicode_extraction_verified = $true
    payload_unchanged = $true
    synthetic_root_removed = $true
    gui_exercised = $false
    authenticated_ssh_exercised = $false
    probe = $receipt
}
[IO.File]::WriteAllText((Join-Path $output 'BUNDLE_VERIFICATION.json'), ($result | ConvertTo-Json -Depth 5) + "`n", $utf8)
$result | ConvertTo-Json -Depth 5
