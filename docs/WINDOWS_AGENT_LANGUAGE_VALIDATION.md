# Windows agent/language acceptance fixture (not an enabled capability)

This checkpoint adds opt-in native tests for the real `cedar_agent::serve` and
`Workspace` language bridge on the existing owned Windows LSP transport. It does
not enable language services in the IDE or ordinary agent. Native runtime
acceptance for this checkpoint is pending until CI runs its exact source revision.
Linux tests and an MSVC cross-target check do not establish Windows behavior.

## Separate host and trust boundary

The `windows-language-validation` Cargo feature builds a separate nonshipping
`cedar-agent-language-validation` binary. Its only root flag is
`--synthetic-root PATH`. The directory must already contain the regular,
non-symlink `.cedar-windows-language-validation` file with exactly these bytes:

```text
cedar-windows-language-validation-v1\n
```

Here `\n` means one newline byte, not the two literal characters. This marker is
an explicit test opt-in, not a security sandbox. Tests use fresh private temporary
roots with Unicode and spaces, and absolute paths to locally built fixtures.
No shell, SSH, GUI Trust action, compiler discovery, downloads, or changes to a
running process's global environment are involved.

`Workspace::for_windows_language_validation` is available only with that feature.
It fixes the host to `IsolatedAgent`, enables only the validation instance's
Windows language startup path, and initially leaves execution disabled.
`--allow-run` is still required before either a task or a language server may
execute. The fixture uses the same wire operations and real sequential serve loop;
there is no new wire grant or configuration flag in normal `cedar-agent`.

Both ordinary constructors (`open` and `with_backend_mode`) always leave Windows
LSP disabled, even when the workspace is marked and the build uses
`--all-features`. Standard `cedar-agent` rejects validation-only CLI flags.
Production `Hello` capability calculation is unchanged. Even the validation host
keeps those production capability claims; these raw-protocol acceptance tests
exercise the explicitly opted-in path without using capability discovery as an
enablement mechanism. The validation executable is absent from CI's shipping
artifact list. Git, legacy synchronous Run, DAP and the in-process Windows host
are not enabled by this fixture.

## Runtime cases

The three ignored Windows integration tests cover:

1. Normal binary and ordinary-constructor gates in an all-feature build, absence
   of language capabilities, and execution rejection without `--allow-run`.
2. Four task/LSP sessions in one real agent: three overlapping startup waves plus
   a language-first wave; real initialize, open and hover requests; file I/O with
   both trees alive; repeated language Stop; independent task cancellation; and
   session restart after cleanup. Stopping the LSP must leave every task process
   alive. Cancelling the task must leave every LSP process alive and able to
   answer another hover request.
3. Peer stdin EOF and forced death of the exact agent Child, each with both owned
   trees known live. No test-owned surrounding Job or PID-based kill can perform
   the cleanup on the implementation's behalf.

For overlapping startup, RunStart returns after asynchronous admission and the
next LanguageStart uses the real language worker. Synthetic task readiness waits
for LSP startup, while LSP initialization waits for task-tree readiness. This
makes their startup lifetimes overlap without sleep-based timing assumptions; it
does not claim that two OS CreateProcess calls necessarily execute simultaneously.
All descendants intentionally retain the capture pipes and stay idle. The driver
opens observation-only handles to each exact root, child and grandchild while
known live, then requires those handles to signal exit. LSP processes additionally
hold exclusive lifetime files; cleanup must release every one without a safety
cap marker. Fixtures have finite lifetime caps, and reaching them fails acceptance.
Task terminal snapshots must follow whole-tree termination.

The existing scheduling limit remains: agent requests are sequential.
LanguageStart can spend up to 60 seconds initializing (the client transport allows
75 seconds), and synchronous queries have their own deadlines. A following Stop,
task cancellation, file operation or peer EOF cannot overtake a blocked handler.
These successful-initialize tests make no responsive-startup-cancellation claim.
Forced process death is a distinct OS ownership test.

## Reproduce on native Windows

Build locally from the source revision to be tested, then run in PowerShell:

```powershell
cargo build --release --workspace --all-features --locked
$env:CEDAR_AGENT_BIN = "$PWD/target/release/cedar-agent.exe"
$env:CEDAR_AGENT_LANGUAGE_VALIDATION_BIN = "$PWD/target/release/cedar-agent-language-validation.exe"
$env:CEDAR_MOCK_LSP_BIN = "$PWD/target/release/cedar-mock-lsp.exe"
$env:CEDAR_WINPROCESS_FIXTURE_BIN = "$PWD/target/release/cedar-winprocess-fixture.exe"
cargo test -p cedar-client --features windows-language-validation --test windows_agent_language --locked -- --ignored --test-threads=1
```

The normal workspace all-feature test run includes constructor, CLI and fixture
trust regressions. CI runs the three native cases explicitly after building the
fixtures. An ignored, skipped or cross-compiled result is not a runtime pass.
Real Windows Java/JDT LS acceptance and any production capability change remain
separate future work. See [the ownership plan](WINDOWS_LANGUAGE_PLAN.md) and
[the transport checkpoint](WINDOWS_LANGUAGE_TRANSPORT.md).
