# Kotlin language-server validation

> Public-source note: named raw logs, screenshots and measurement payloads are omitted from this repository. See [verification evidence](../PUBLICATION.md#verification-evidence).

Checked 2026-10-07 on Linux x86_64. This is a real stdio integration test through
Cedar's Rust `LspClient`, separate from frontend interaction testing and SSH
transport testing. It does **not** establish IntelliJ feature parity.

## Result

- **Community fallback `fwcd/kotlin-language-server` 1.3.13:** a complete semantic
  edit loop passed: initialize, open, real type diagnostic, typed hover, one
  relevant completion, exact local definition, change, cleared diagnostics,
  close, shutdown request, process reaping and temporary-fixture removal
- **Official `Kotlin/kotlin-lsp` 263.6379.0:** semantic validation is blocked before
  startup. The verified standalone archive requires explicit EULA acceptance,
  but its EULA-display command reports a missing bundled agreement. No agreement
  was accepted and no workaround around that gate was attempted
- **Graceful fallback shutdown is not a pass:** the server answered the shutdown
  request but did not exit within Cedar's five-second grace period. Cedar killed
  and reaped it. The final run also logged an upstream indexing/disposal error

## Official server: current release and blocker

The [official repository](https://github.com/Kotlin/kotlin-lsp) describes an Alpha
server based on IntelliJ IDEA and partially proprietary Air/Fleet components.
Its visible repository has an Apache-2.0 license; that is not evidence that every
component in the binary distribution has that license. Do not redistribute the
binary under Cedar's license or describe it as fully open source.

The [263.6379.0 release](https://github.com/Kotlin/kotlin-lsp/releases/tag/kotlin-lsp%2Fv263.6379.0)
published on October 3 supplied a Linux x64 standalone archive. Its
[published SHA-256](https://download.jetbrains.com/language-server/kotlin-server/263.6379.0/kotlin-server-263.6379.0.tar.gz.sha256)
matched the downloaded bytes:

```text
ab8ca4455dc2fc5fe1a24db2bccc46c104254d2c465155c4251ee65df8f3f7cc
```

Inspected distribution metadata:

- Product version 2026.3 EAP, build 263.6379.0
- Minimum Java version **25**, with bundled JetBrains Runtime
  `25.0.4.1+1-b601.61`; the machine's OpenJDK 21 alone is insufficient
- The old `kotlin-lsp.sh` forwards to `bin/intellij-server` and is deprecated
- `--help` describes `--stdio`, explicit `--eula` acceptance, and
  `--data-sharing=none` (also the stated CLI default)
- `bin/intellij-server --show-eula` returned exit status 12:
  `Bundle is malformed: EULA.txt is missing.`

Only metadata/help/EULA-display inspection was performed. No LSP server listener,
account, license key, EULA acceptance record or semantic session was created.
The next step for this exact release is a corrected official distribution and
review/approval of its actual agreement. A synthetic protocol adapter cannot
remove that dependency. See
[`kotlin-official-263.6379.0-setup.json`](../PUBLICATION.md#verification-evidence).

## Community fallback: provenance and limitations

The [fwcd project](https://github.com/fwcd/kotlin-language-server) explicitly marks
itself deprecated in favor of the official server. Its latest published release
is [1.3.13, January 18, 2025](https://github.com/fwcd/kotlin-language-server/releases/tag/1.3.13),
bundling Kotlin compiler 2.1.0. Its
[source license is MIT](https://raw.githubusercontent.com/fwcd/kotlin-language-server/1.3.13/LICENSE.txt),
and its [build instructions](https://raw.githubusercontent.com/fwcd/kotlin-language-server/1.3.13/BUILDING.md)
specify Java 11+. This run used OpenJDK 21.0.12.1.

The release API exposed one `server.zip` asset and no digest, detached signature
or checksum asset. The locally computed SHA-256 was
`4fe7d71d087b307c7869036171bd9d8c6a4284cd7c25b89098b0a24eb2d9b6d2`.
This is a reproducibility fingerprint, **not** verification against an independently
published checksum. The download came directly from the maintainer's GitHub
release; provenance is recorded in
[`kotlin-fwcd-1.3.13-provenance.json`](../PUBLICATION.md#verification-evidence).

The tested fallback is useful for establishing the integration path. It is not a
recommendation to depend on a deprecated server for current Kotlin, Android,
Kotlin Multiplatform, mixed Java/Kotlin or large Gradle projects without further
validation. Those project types were not exercised.

## Reproduce the real-server smoke

After independently obtaining the fallback distribution, pass its unpacked
`server` directory, containing `lib/server-1.3.13.jar`:

```sh
cargo run --locked -p cedar-language --example kotlin_smoke -- \
  /path/to/fwcd-kotlin-language-server-1.3.13/server /usr/bin/java
```

The example downloads nothing. It launches one JVM in stdio mode with
`-Xms64m -Xmx512m`, creates a temporary Kotlin source directory and isolated server
storage/home, and copies the distribution's existing `kotlin-stdlib-2.1.0.jar`
into a private Maven-layout cache. Upstream's
[backup classpath resolver](https://raw.githubusercontent.com/fwcd/kotlin-language-server/1.3.13/shared/src/main/kotlin/org/javacs/kt/classpath/BackupClassPathResolver.kt)
reads that cache without invoking Maven. No project build files or classpath
scripts are created. Unix runs isolate the relevant home/config/cache variables.
Only Linux execution was validated.

The fixture contains `val broken: Int = "oops"` and an ordinary local `greeting`.
The program asserts the server's real `TYPE_MISMATCH`, String hover, completion
label, and definition URI/line. It changes the buffer to `val broken: Int = 42`
and requires a subsequent batch with no error diagnostics. It sends no
`workspace/executeCommand`, executes no completion commands and applies no
`workspace/applyEdit`. Only synthetic source is opened. JDK/server binaries are
not included in the Cedar deliverable.

Final-run observations, preserved in
[`kotlin-fwcd-1.3.13-smoke.jsonl`](../PUBLICATION.md#verification-evidence):

| Check | Observed result |
|---|---|
| Cold JVM process initialization | 2,949 ms |
| Server identity | Kotlin Language Server 1.3.13 |
| Synchronization | Incremental, `textDocumentSync = 2` |
| Initial diagnostics | Exactly one `TYPE_MISMATCH` at zero-based line 3 |
| Hover | `val greeting: String` |
| Completion at end of token | One candidate, `greeting` |
| Definition | Same fixture URI, zero-based line 1 |
| Diagnostics after in-memory correction | Empty array |
| Direct JVM RSS after initialize | 182.94 MiB |
| Direct JVM RSS after semantic queries | 385.77 MiB |
| Direct JVM RSS after correction | 443.23 MiB |
| Shutdown/cleanup | Request completed; forced reap after 5,000 ms grace; PID gone |
| Total example runtime | 13,002 ms |
| Temporary fixture | Removed |

RSS and high-water RSS come from the direct JVM's `/proc/<pid>/status`, whose
`kB` units are KiB. These numbers exclude the Cedar frontend and agent; they are
not heap usage, PSS or total remote-workspace memory. This is one tiny synthetic
run with fresh process/project/server data, not a cold OS page-cache experiment,
large-project benchmark or comparison against IDEA. The heap cap is not an RSS
cap. Global indexing was left at the server default.

## Known gaps exposed by the experiment

1. The fallback sends **unversioned** diagnostic notifications. The test drains
   pre-edit events before the controlled correction, but that does not prove
   general rapid-edit/stale-diagnostic correctness in an interactive client
2. Completion requested in the middle of the existing `greeting` token returned
   75 noisy candidates and omitted the desired local symbol. End-of-token
   completion succeeded. The failed mid-token result is retained in
   [`kotlin-fwcd-1.3.13-limitations.json`](../PUBLICATION.md#verification-evidence)
3. An initial fixture without an explicit stdlib cache produced missing-builtin
   and unresolved-`println` errors. That attempt failed; it was fixed by supplying
   the server distribution's stdlib, not by weakening the clean-diagnostics check
4. The server's shutdown needed forced reaping and emitted an upstream disposal
   error while background indexing was active. The captured
   [stderr](../PUBLICATION.md#verification-evidence)
   is retained. A successful `shutdown()` return is not proof of graceful exit
5. `completionProvider.resolveProvider` was false. Rename, references, formatting,
   code actions, signature help, semantic tokens and inlay hints were advertised,
   but **not tested or claimed as implemented in Cedar by this validation**
6. No build, debugger, refactoring application, dependency-import workflow,
   authentication, SSH server or frontend completion-popup interaction was tested

Local quality checks passed for the example: rustfmt check, strict Clippy, and
two helper tests. The real-server run is opt-in and is not silently downloaded or
started by ordinary unit tests.
