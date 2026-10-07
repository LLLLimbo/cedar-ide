# Prepared direct Windows Java acceptance

> Public-source note: named raw logs, screenshots and measurement payloads are omitted from this repository. See [verification evidence](../PUBLICATION.md#verification-evidence).

This prepares the opt-in `cedar-language` example for a real Windows JDT LS run.
It does **not** enable Windows IDE language capabilities, run the workspace agent,
or establish a Windows runtime pass. The broader gate remains in
[WINDOWS_LANGUAGE_PLAN.md](WINDOWS_LANGUAGE_PLAN.md).

## Implemented example checks

`crates/language/examples/java_smoke.rs` now:

- Requires an explicitly supplied, existing absolute `java.exe` on Windows. It
  does not search PATH/PATHEXT, invoke a shell, or accept a `.cmd` wrapper. Linux
  and macOS retain the existing optional Java argument/PATH fallback.
- Selects `config_win` on Windows (`config_linux`/`config_mac` elsewhere), requires
  that directory and exactly one Equinox launcher JAR, and passes literal argv.
  Java-consumed JAR/configuration/data paths on Windows use verified ordinary
  local-drive spellings. Only canonical VerbatimDisk prefixes are stripped;
  canonical equivalence is rechecked, and UNC/device forms are rejected. The
  executable keeps its native path. This avoids the Java 21 java.io UNC-like
  interpretation of Rust's verbatim path prefix; it does not claim UNC/long-path
  coverage.
- Rejects inherited `CLIENT_PORT`, `CLIENT_HOST`, `socket.stream.debug`,
  `JDK_JAVA_OPTIONS`, `JAVA_TOOL_OPTIONS` and `_JAVA_OPTIONS` without printing
  their values or mutating global environment. Prepare a clean test parent.
- Creates project, source and JDT data paths containing spaces and `雪`. The
  fixture has only the Eclipse Java builder, JRE container and synthetic source;
  no Maven/Gradle files, downloaded dependencies or external builders. The direct
  client disables Maven/Gradle imports in initialization settings. This does not
  imply that the workspace agent sends those settings.
- Requires the severity-1 String-to-int diagnostic at the initial string literal,
  a nonempty `String greeting` hover, a greeting completion with a primary edit,
  and exactly one definition at the local declaration URI/range. Both Location
  and LocationLink forms are supported. Equivalent local file-URI serialization
  is normalized (raw/escaped Unicode, percent-escape case, `file:/` versus
  `file:///`, and Windows drive-letter case); remote authorities, UNC, queries,
  fragments and NUL bytes are rejected. The decoded fixture path must match exactly.
- With `--resolve-imports`, requires `resolveProvider=true`, the original
  GregorianCalendar candidate's primary edit and opaque data, deferred import
  retrieval, and unchanged primary edit/label. It never applies those edits or
  executes server commands. Actual editor transactions/Undo/Redo are separate.
- Sends an unsaved version-2 correction that renames the local to `correctedOnly`.
  The fixture explicitly configures unused locals as warnings. Clearing succeeds
  only when there are no errors and the warning identifies `correctedOnly` at its
  exact range; its hover must also report `int correctedOnly`. Thus an old,
  unversioned empty diagnostic is insufficient. Present versions must match
  exactly; overflow and closed transport fail the test.
- Compares source bytes after queries/resolve, after correction, and after
  shutdown or failure Drop, before removing the fixture. JDT metadata/build
  output may change; the invariant is the original source bytes.
- Runs three serialized sessions: initial fresh data, restart with different
  fresh data, then restart using that second data directory. Every session opens
  the original on-disk source and repeats the semantic checks. Each previous
  client is shut down and dropped before the next launch. Reusing the directory
  exercises release of its JDT workspace lock.
- On Windows, retains a query/wait-only process handle immediately after spawn,
  checks that it is live after initialize, records its creation FILETIME and PID,
  and waits on that same handle after shutdown/Drop before reading its exit code.
  Acceptance requires exit code zero after a successful shutdown API call; a
  signaled root with a forced/nonzero exit is cleanup-only and fails acceptance.
  Windows gets a ten-second shutdown grace period (portable behavior keeps three).
  Restart identity compares PID plus creation time, not PID inequality alone.
  No PID-based termination or executable-name process search is used.
- Explicitly removes the temporary fixture and fails if removal fails. The final
  `pass` record appears only after all three sessions and cleanup succeed.

`shutdown_api_succeeded` records the existing shutdown/exit API outcome, whose
configured kill fallback can return success. Therefore Windows separately
requires the retained handle's actual exit code to be zero and records
`gracefully_exited` plus the actual code even on failure. A forced/nonzero exit
does not pass simply because cleanup completed. On portable platforms no
independent exit-code observer is added, so `gracefully_exited` is null. On Windows
the API waits for the owned worker's join/cleanup contract; the retained handle
separately observes root termination. The example has **no independent
Job-zero, descendant, listener or handle-count observer**. Its output records
those omissions and `windows_full_acceptance:false` even when this direct probe
passes. A quiet real JDT server alone cannot prove descendant cleanup.

Linux retains its `/proc` JVM-only RSS/high-water snapshots, now in each session;
other platforms emit `jvm_memory_unavailable`. Earlier single-session timings
and memory captures remain historical and are not directly comparable to this
three-session workload. UNC fixtures are explicitly rejected, not claimed as
covered. Distribution/JDK paths need separate spaces/Unicode placement by the
operator if those argument paths are to be covered too.

## Dependency and invocation contract

Use already obtained, independently verified test dependencies. Do not bundle
them into Cedar. Preserve their included notices/licenses. These pinned official
artifacts were identified during preparation; verify the actual downloaded bytes
before extraction/use:

- JDT LS 1.61.0:
  [jdt-language-server-1.61.0-202609031315.tar.gz](https://download.eclipse.org/jdtls/milestones/1.61.0/jdt-language-server-1.61.0-202609031315.tar.gz)
  with [publisher checksum](https://download.eclipse.org/jdtls/milestones/1.61.0/jdt-language-server-1.61.0-202609031315.tar.gz.sha256)
  `338e7e73d61836651ba2453919a0d34fa763eb4e7c03342092309bffb8934c64`.
- Optional pinned Windows x64 JDK:
  [Temurin 21.0.12.1+1 ZIP](https://github.com/adoptium/temurin21-binaries/releases/download/jdk-21.0.12.1%2B1/OpenJDK21U-jdk_x64_windows_hotspot_21.0.12.1_1.zip)
  with [publisher checksum](https://github.com/adoptium/temurin21-binaries/releases/download/jdk-21.0.12.1%2B1/OpenJDK21U-jdk_x64_windows_hotspot_21.0.12.1_1.zip.sha256.txt)
  `f9d6e191ab098c0d416e7d588a24420a8621cd2f4720dab2459b8b7b2d2d8b4e`.

An explicitly selected installed Java 21+ is also usable, provided its exact
path/build/release metadata is captured. A runner's default Java may be older;
do not assume bare `java` or `JAVA_HOME` identifies the intended JDK. The pinned
JDT milestone may report `1.61.0-SNAPSHOT` in initialize; retain the actual response
rather than substituting the archive version for serverInfo.

Before running, use a dedicated Windows test parent, a private local-drive
scratch extraction or serialized distribution configuration, and an external
watchdog covering the whole run. Three sessions each contain a 60-second
initialize/request budget and two 60-second diagnostic waits, plus semantic
requests and cleanup; a 45-second watchdog is insufficient. The example's
bounded operations do not substitute for an external overall watchdog.

Example PowerShell, after archive/hash verification and scratch extraction:

```powershell
$ErrorActionPreference = 'Stop'
$Jdt = (Resolve-Path 'C:\acceptance with spaces 雪\jdtls').Path
$Java = (Resolve-Path 'C:\acceptance with spaces 雪\jdk\bin\java.exe').Path
$env:JAVA_HOME = Split-Path (Split-Path $Java -Parent) -Parent
foreach ($Name in @('CLIENT_PORT', 'CLIENT_HOST', 'socket.stream.debug',
    'JDK_JAVA_OPTIONS', 'JAVA_TOOL_OPTIONS', '_JAVA_OPTIONS')) {
    $Entry = 'Env:' + $Name
    if (Test-Path -LiteralPath $Entry) { Remove-Item -LiteralPath $Entry }
    if (Test-Path -LiteralPath $Entry) { throw "Failed to remove $Name" }
}
& $Java -version
if ($LASTEXITCODE -ne 0) { throw 'Java metadata command failed' }
Get-Content (Join-Path $env:JAVA_HOME 'release')
cargo run --locked --offline -p cedar-language --example java_smoke -- $Jdt $Java --resolve-imports
if ($LASTEXITCODE -ne 0) { throw 'Direct Java acceptance failed' }
```

Replace the illustrative directories with the actual verified extraction paths.
Do not prequote argument values or launch the server through PowerShell. The
example launches native Java itself. Also record checkout SHA/dirty state, Rust
target/version, runner image/Windows version and actual dependency hashes;
capture stdout JSON-lines and stderr without complete environment dumps.

## Preparation validation and remaining gate

Preparation checks on Linux, with existing cached dependencies and no downloads:

```sh
cargo +1.99.0 test -p cedar-language --example java_smoke --locked --offline
cargo +1.99.0 clippy -p cedar-language --example java_smoke --locked --offline -- -D warnings
cargo +1.99.0 check -p cedar-language --example java_smoke --target x86_64-pc-windows-msvc --locked --offline
```

Ten focused Linux example tests passed. Strict clippy passed for Linux and
`x86_64-pc-windows-msvc`, and Windows example/test cross-checks passed. These are
preparation checks, not native Windows execution.

### Real Linux rerun (2026-10-07 UTC)

The official pinned JDT archive's SHA-256 was verified before extraction.
The probe used OpenJDK `21.0.12.1+1-1-deb13u1-Debian`, the distribution path
`distribution 雪`, and generated Unicode/spaces project/data paths. JDT reported
`JDT Language Server (Standard)` / `1.61.0-SNAPSHOT`.

The first run exposed a URI comparison bug: JDT returned raw `雪` while escaping
spaces, whereas the original helper sent percent-encoded UTF-8. The example
ignored those diagnostics and timed out; failure cleanup still preserved source
bytes and removed the fixture. Strict equivalent-local-URI comparison fixed this
without accepting a different file or weakening range checks. A focused
regression now covers both representations and invalid/external URI rejection.

The corrected run with `--resolve-imports` passed all three sessions in
**27,067 ms**, excluding Cargo compilation:

- Initial session semantics: 6,085 ms
- Fresh-data restart semantics: 6,078 ms
- Same-data restart semantics: 5,705 ms
- Each session passed the exact severity-1 error, greeting completion/hover/local
  definition, deferred GregorianCalendar import resolve, version-2 correction,
  correction-specific severity-2 warning and `int correctedOnly` hover
- All source-byte checks and shutdown API calls passed; the entire fixture was
  removed before the final pass record
- The same-data restart initially emitted an unversioned zero-width cached error
  range. The exact-range check rejected it and waited for the correctly located
  error, then verified the corrected draft

This is one synthetic Linux run with fresh JVMs, not a benchmark or a Windows
result. The local capture `verification-phase9c-java-linux.txt` retains both the
failed diagnosis and successful rerun; raw verification logs are omitted from the
public export as described in [verification evidence](../PUBLICATION.md#verification-evidence).
No Windows runtime, native GUI, SSH, isolated-agent integration or capability-gate
change was performed. Linux shutdown keeps the existing direct-child API and kill
fallback; no independent graceful exit-code/tree/listener assertion is claimed.
Missing dependencies, ignored tests, compile-only results or this Linux pass must
not be presented as Windows acceptance.

Before enabling the IDE capability, still require the same-public-revision
native Windows direct and isolated-agent JDT acceptance, source invariant and
editor transaction checks, task/language independence and owned-agent-death
cleanup, deterministic descendant/blocking-I/O fixtures, full Job-zero/handle/I/O
evidence, and external IPv4/IPv6 listener/command-line observations of known owned
processes. The agent's existing sequential startup bound remains unchanged.


## CI dependency setup and evidence

`scripts/windows_java_acceptance.ps1` requires PowerShell Core 7.2+ and an isolated
Windows test process. It selects the runner's explicit `JAVA_HOME_21_X64/bin/java.exe`
or an explicitly provided absolute native executable, checks JDK metadata and
requires Java 21+. It records the actual vendor/build; the runner JDK is not
misidentified as a pinned Temurin build. Rust host/target, checkout SHA/state and
runner image are also recorded without dumping the inherited environment.

The script downloads only the exact official JDT 1.61.0 archive and verifies
SHA-256 `338e7e73d61836651ba2453919a0d34fa763eb4e7c03342092309bffb8934c64`
before extraction. Each run gets its own spaces/Unicode extraction directory.
Upstream notices remain intact; the JDK and JDT are external test dependencies
and are excluded from Cedar product artifacts. No debugger or alternate-transport
flags are supplied. The script sanitizes the specified Java/socket environment
variables before starting Cargo, in this test parent only.

Every native command's exit code is checked. Download connection/stall limits
are requested where supported; the workflow's eight-minute deadline supplies
the overall bound. Direct callers must impose their own enclosing deadline.
Forced CI cancellation is a failure boundary, never proof that cleanup ran.
The final PASS is written only after semantic assertions and generated dependency
scratch deletion succeed; primary/cleanup failure evidence is preserved.

The workflow uploads `cedar-windows-java-acceptance.txt`, containing public test
metadata and synthetic protocol results, even on failure when the file exists.
A missing artifact or skipped step does not establish acceptance. This script
has been source-reviewed, not executed in PowerShell locally; actual Windows CI
is the verdict for its platform behavior and all Windows-only Rust assertions.

The final integrated 0.8.4 candidate was rerun on Linux with the same verified
external distribution and `--resolve-imports`: all three sessions passed again
in 34,909 ms, with unchanged source bytes and successful fixture removal. This
includes the finalized helper call sites; Windows-only path normalization still
requires its native unit/runtime checks. The differing elapsed time is not a
performance comparison: concurrent build activity and one synthetic run do not
establish benchmark results.


## Windows setup correction after the first native attempt

The 0.8.4 Windows run verified the archive hash but the runner's `tar.exe` could
not open the archive under a Unicode staging path (its error rendered `雪` as
`?`). JDT was not started, and the later agent-language tests were skipped.
The setup now uses an ASCII extraction staging directory, then PowerShell's
Unicode-safe filesystem move places the unchanged distribution in `JDT
distribution 雪`. It verifies staging disappearance, destination/configuration,
one launcher, and identical launcher digest before/after the move. The Java,
project and data-directory Unicode assertions are unchanged. A direct caller
must give an ASCII ScratchRoot for this native tar boundary; no machine locale
or encoding setting is changed.

The same log showed empty Java option variables. PowerShell 7.5 can retain empty
values when `SetEnvironmentVariable` receives `$null`; this is not absence, which
the Rust probe requires. The script and example instructions now remove entries
with the Environment provider and verify that each entry is absent. These are
process-local test-parent changes, not persistent user/machine settings.
