# Verification report · 2026-10-07

> Public-source note: named raw logs, screenshots and measurement payloads are omitted from this repository. See [verification evidence](../PUBLICATION.md#verification-evidence).

## Result

The first-stage project is buildable and its implemented paths were exercised.
It is **not a complete IntelliJ IDEA replacement**. No untested feature is marked
as passed, and no production-performance equivalence is claimed.

### Automated checks

- `cargo fmt --all -- --check`: PASS
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`: PASS
- `cargo test --workspace --all-targets --all-features --locked`: **82 passed**, zero failed; one explicitly environment-dependent process test is initially ignored
- The ignored client→separate-agent test was then explicitly executed using the compiled agent: **1 additional PASS**
- `cargo build --release --workspace --locked`: PASS (Linux x86_64)
- `cargo check --workspace --all-targets --all-features --target x86_64-pc-windows-msvc --locked`: PASS; compile check only, no Windows runtime or linker verification
- Python black-box stdio smoke: PASS (hello/list/read/write/Unicode/search/conflict/traversal/trust/EOF/malformed frame)
- Python client→agent→Rust mock-LSP process chain: PASS (start/open/change/stale version/completion/definition/hover/diagnostics/close/stop/restart)

Compiler: rustc 1.99.0. Linux environment: x86_64, kernel 6.18.44, glibc 2.41.
The exact aggregate command output is preserved in `verification-log.txt`.

### Native GUI interaction

The built application was launched and operated on a real Linux desktop session:

1. Local project connection and source-folder navigation: PASS
2. Java and Kotlin files in separate tabs: PASS
3. Typing a draft updates dirty state; Ctrl+S writes actual disk bytes: PASS
4. Dirty-tab close presents confirmation; Keep editing preserves text: PASS
5. External disk change followed by Save produces a conflict; both the UI draft and externally changed file are preserved: PASS
6. Window close with unsaved text is intercepted and requires an explicit decision: PASS
7. Optimized build starts a real separate deterministic LSP process, syncs the file, and displays hover/completion responses: PASS
8. Clean window exit with a live LSP completes its shutdown path and exits with status 0: PASS

`native-editor.png` is an actual screenshot of the running application.
`native-lsp-fixture.png` is an actual screenshot of the deterministic mock-LSP
GUI check; its "mock hover" result is deliberately not presented as Java analysis.
The UI and backend semantics also have headless regression tests, including
stale connection generations, typing during asynchronous workspace switches,
save acknowledgements after newer edits, and delayed opens at the 32-tab limit.

### Real Java language server

Official Eclipse JDT LS milestone **1.61.0**, archive build **202609031315**, was
downloaded from Eclipse and checked against its published SHA-256. The server
reports `1.61.0-SNAPSHOT`. Tests used installed OpenJDK **21.0.12.1** and a
throwaway synthetic Eclipse Java project, not any user source code.

A second successful run measured JDT JVM-only RSS/HWM at 254.15 MiB after
initialization, 485.26 MiB after semantic queries and 524.68 MiB before shutdown.
This is separate from the frontend sample and is not a measured concurrent total.
The JVM uses additional native/shared memory beyond its 512 MiB heap cap. Raw
rerun evidence is `crates/language/tests/evidence/jdtls-1.61.0-memory-smoke.jsonl`.

The Rust LSP client successfully initialized, opened the document, obtained a
String-to-int type diagnostic, returned a real variable hover, returned
`greeting : String` among completion candidates, navigated to its declaration,
synchronized a correction through incremental protocol mode, observed the type
error disappear, closed the document and shut down the server. The remaining
unused-local warning is expected and was not mislabeled as a clean diagnostic
list. Cold initialization: 4.768 s; complete synthetic check: 11.234 s.

Reproduce with `cargo run -p cedar-language --example java_smoke -- JDTLS_DIRECTORY`.
The distribution is not bundled. See `LANGUAGE_SERVICES.md` for official sources,
checksum, setup and limits. Raw JSON evidence is included at
`crates/language/tests/evidence/jdtls-1.61.0-smoke.jsonl`.

The real-Java test covers the Rust language library. The agent bridge and GUI
were separately validated using the deterministic peer. It does not establish
large Maven/Gradle project compatibility or a tested Kotlin language server.

### Safety issues found and fixed

- Git status can execute repository clean/process filters. Git now shares the explicit tool-execution trust gate; a malicious-filter regression proves it
- An asynchronous workspace switch could discard typing made after connection began. Completion-time dirty checks preserve it
- Dense in-file searches had quadratic character-offset conversion. Mapping is now linear and results are capped
- Pending asynchronous opens could exceed the tab ceiling. Acceptance rechecks the limit and has a regression
- Reaping a process before signaling its group creates a PID-reuse race. Linux/macOS runner now observes with waitid/WNOWAIT, signals before reaping, and tests preserved wait status
- Windows local tool cleanup did not safely contain descendants/inherited pipes. Run/Git/LSP launching are disabled on local Windows until a verified implementation exists; Linux remote execution remains available

### Not run / deliberately unsupported

- Windows and macOS native window execution, packaging and IME/accessibility validation
- A real authenticated SSH server session; no keys were created and no user server was contacted
- Actual Kotlin server compatibility, production project imports, automatic refactoring/completion edits, debugger, interactive terminal
- Crash/force-quit recovery or long-running multi-day resource-leak testing
- IntelliJ IDEA comparative benchmarks
- External CI execution, signing, publishing, or deployment

The provided CI configuration can exercise Linux/Windows after the project is
placed in an authorized repository. Configuring a workflow is not evidence it
has run.
