# Verification report · phase 5 · 2026-10-07

> Public-source note: named raw logs, screenshots and measurement payloads are omitted from this repository. See [verification evidence](../PUBLICATION.md#verification-evidence).

Cedar **0.5.0**, frontend/agent protocol **4**, is an in-development Rust IDE
checkpoint. This phase adds explicit saved task profiles, repairs the ordinary
Windows file-replacement path and strengthens process-transport failure handling
and SSH configuration boundaries. It does not establish IntelliJ IDEA parity.

**Final local aggregate and builds passed.** The final run includes the late
frontend profile and connection-identity fixes: 419 Rust passes and four Python
black-box chains. Publication, exact-commit phase-5 Windows CI, authenticated SSH
and parts of native acceptance remain pending. This report separates final code
checks from native-window evidence; a pending row is not a pass.

The original [phase-4 report](TEST_REPORT_PHASE4.md) is preserved byte-for-byte.
Its contemporaneous pending-CI statement is historical: the later public commit
[`b1ad6f53e325bf19b1fec469394f6eb6ecf2c3ca`](https://github.com/LLLLimbo/cedar-ide/commit/b1ad6f53e325bf19b1fec469394f6eb6ecf2c3ca)
passed both Ubuntu and Windows jobs in
[run 37630631194](https://github.com/LLLLimbo/cedar-ide/actions/runs/37630631194).
That success does not verify the new phase-5 Windows replacement or transport
changes. [Phase 3](TEST_REPORT_PHASE3.md),
[0.3.1](TEST_REPORT_HOTFIX_0_3_1.md), [phase 2](TEST_REPORT_PHASE2.md) and
[phase 1](TEST_REPORT_PHASE1.md) retain their original evidence and limitations.

## Verification snapshot

Verification environment: Linux x86_64, Rust 1.99.0.
This report is sealed with source tag `phase5-0.5.0`. Its public commit and
exact-commit CI result are not known at sealing time; check the repository Actions
record for the subsequently published commit.

| Check | Current evidence | Acceptance boundary |
|---|---|---|
| `cargo fmt --all -- --check` | **PASS** in final aggregate | Final local source |
| Strict workspace Clippy, all targets/features, locked dependencies | **PASS** in final aggregate | Final local source |
| Ordinary workspace tests | **413 passed**, zero failed, 9 opt-in ignored | Final local source |
| Explicit client → separate-agent/fault integration | **6 additional passes**; among the 9 ignored cases above | Final local source; three other opt-ins remain ignored |
| Total final Rust passes | **419** including the 6 explicit cases | No double counting of focused subsets |
| Four Python black-box chains | **PASS**: filesystem, LSP, asynchronous tasks and saved profiles | Real stdio agent, synthetic fixtures |
| App subset | **227 passed, 2 opt-in ignored** | Included in workspace total |
| Linux release workspace build | **PASS, exit 0** | Final local source; binary hashes below |
| Windows MSVC workspace/all-target compilation | **PASS, exit 0**; focused Windows-target strict Clippy also passed | Compilation only; real Windows runtime remains pending |
| Native Linux profile UI, trust off | **PASS**: first Load/editor activation, selection, pretty JSON save, exact argv and external conflict | Repeated on final release bytes identified below |
| Native dirty-window close | Not completed | No native close-guard pass claimed |
| Native profile Run/Cancel/reconnect review | Not performed | Needs explicit approval to enable the synthetic workspace's execution trust |
| Real JDT through stdio agent | **10 checks passed** after shared script changes | Not native GUI or a large-project benchmark |
| OpenSSH 10.0p2 generated options | `ssh -G -F /dev/null` accepted options without a connection | Local argument parsing only |
| Authenticated SSH and real interruption | Not performed; no test keys/server/listener created | Narrow test authorization required |
| Phase-5 real Windows CI | Not run against a published phase-5 commit | Required for new Windows runtime claims |
| Windows/macOS native GUI | Not performed | Remains unvalidated |
| New memory/IDEA comparison benchmark | Not performed | No new performance claim |

Final logs: [verification-phase5-log.txt](../PUBLICATION.md#verification-evidence),
[release-phase5-build.txt](../PUBLICATION.md#verification-evidence),
[windows-phase5-check.txt](../PUBLICATION.md#verification-evidence).
The initial 409-pass aggregate and intermediate 220-pass app run preceded the
last fixes; they are superseded by the final 419-pass result, not additional
independent passes.

Final Linux release SHA-256:

- `cedar`: `35ea2f077696bccdae152fa4b288508c19dacf979c8f449dfbbf9ad4025d9f95`
- `cedar-agent`: `4209ef7bfcfa89b4c467eccad542002f0ca9fc31c1d0cba54c08c919e03a60ae`

These identify local binaries. They are not a signature, a reproducible-build
attestation or a publication result. The trust-off native checks below used
these exact bytes.

## Saved task-profile coverage

The version-1 `cedar.tasks.json` schema rejects unknown, duplicate and missing
fields, positional struct arrays, unsupported versions and duplicate names.
Bounds include a 256 KiB encoded file, 32 profiles, 256 arguments and 64 KiB total
argument text per profile. Strings retain their exact whitespace, empty entries,
Unicode and shell-looking text. Pretty serialization includes its indentation,
escaping and final newline in the output cap; oversized output fails before any
raw-document or undo mutation.

App regression paths include:

- Explicit Load/select/New/Save/Discard/recovery/reconnect with no automatic Run
- Source identity, generation, document/edit version and SHA checks
- Already-open raw drafts, invalid input and genuine-not-found creation
- Delayed load/save results, current typing, closed tabs and stale generations
- Raw/form divergence, revision conflicts and lost connections without replay
- One editor transaction for configuration serialization and normal save ownership
- Same-frame raw Text/Paste conflicts and close guards before serialization
- Reconnect review with the original revision and independent trust setting
- Exact argv dispatch, repeated Run suppression and active-task cancellation paths
- Local Windows execution rejection versus synthetic Windows-frontend/SSH gating
- Minimum/default-height layouts with the maximum 256 argument rows

The new dependency-free Rust example and Python smoke exercise saved configuration
read/save/reload, trust rejection, revision conflicts, literal argv, workspace cwd,
explicit task start/cancel and a sentinel through a real agent. These fixtures are
synthetic. They are not an authenticated SSH session or a native Run-button pass.
See [TASK_PROFILES.md](TASK_PROFILES.md) and
[task_profiles_smoke.py](../scripts/task_profiles_smoke.py).

## Native findings and final release recheck

The debug candidate was operated with execution trust **off** against a synthetic
workspace. It loaded three profiles, selected the literal-argument case, renamed
and saved a profile, preserved exact empty/space/Unicode/shell-looking arguments,
created no sentinel/shell side effect, and displayed a save conflict after an
external file modification. The external disk value survived and the ordinary
configuration document retained the dirty submitted draft.

The debug candidate exposed an initial Load that created the configuration tab
without activating any document. Source changes activate the first loaded tab
while preserving newer navigation; pretty JSON was also added. At 15:10 UTC,
a Linux release candidate recheck passed first-Load editor activation, pretty
JSON save and exact empty/space/CJK/shell-looking argument preservation, still
with trust off and no sentinel. This candidate predates the final connection-
identity correction. Native dirty-window close was not completed and is not
claimed as an acceptance pass; the older dirty window was retained.

Evidence: [initial native QA](../PUBLICATION.md#verification-evidence),
[initial conflict screenshot](../PUBLICATION.md#verification-evidence) and
[release candidate screenshot](../PUBLICATION.md#verification-evidence). The initial
JSON record retains its earlier pending-first-Load wording; the later recheck
is reported here rather than rewriting that candidate's history.

At 15:15 UTC the exact final release was launched in a separate window, leaving
older drafts intact. With trust still off, first Load activated the editor;
selection/name edit/Save wrote readable JSON with the final newline, preserving
empty, space-containing, CJK and shell-looking arguments. No task side effect
appeared. A later external disk edit made Save visibly fail with a conflict;
the external bytes survived while the submitted draft remained dirty.
[Final native evidence](../PUBLICATION.md#verification-evidence),
[save screenshot](../PUBLICATION.md#verification-evidence) and
[conflict screenshot](../PUBLICATION.md#verification-evidence) identify this
repeat. No native execution, cancellation or dirty-close result is inferred.

### Connection-identity correction

The prior colon-concatenated connection key could represent two distinct valid
SSH endpoint/root/agent tuples identically. If Hello also reported the same
canonical root, an endpoint switch could retain the wrong workspace's dirty
buffers. `WorkspaceKey` now uses separate Local/SSH variants and separate
host/port/root/agent fields. Seven no-network regressions cover the collision,
pre-connect and post-Hello dirty protections, clean-buffer reset, changed
canonical roots, and trust-only reconnects. All seven passed in the final
aggregate; no SSH connection or trust grant was required by these tests.

### Real JDT repeat

The refactored shared agent harness was rerun against actual JDT LS. All ten
checks passed: initialization, two synchronized unsaved documents, cross-file
references, class/method outline, Chinese-preserving formatting, stale-version
rejection, formatting idempotence, workspace-boundary navigation rejection,
close/shutdown, and unchanged project source bytes. The fixture was removed.
See [jdt-navigation-phase5.json](../PUBLICATION.md#verification-evidence), which records the
same final agent SHA above. This is real stdio agent/LSP evidence, not native
language UI acceptance, SSH authentication or a performance benchmark.

No approval has yet been recorded for enabling native execution trust in the
synthetic session. Automated trusted fixture tests do not grant that approval.

## Workspace-save and remote-fault evidence

The ordinary workspace save correction is separate from phase-3.1's recovery-
store fix. Existing-file preparation clears Windows temporary attributes and
flushes before the final revision/path recheck; Windows commit uses Rust 1.99
`std::fs::rename`. New files still use no-clobber creation. The implementation
never deletes the destination first, bypasses read-only/deny-delete-sharing or
automatically retries a save. Linux tests and Windows-target compilation cover
the new regressions, but actual Windows behavior awaits exact-commit CI. The
late SHA check still is not an atomic filesystem compare-and-swap. See
[WORKSPACE_SAVE.md](WORKSPACE_SAVE.md).

Transport tests use real child pipes and a portable Rust fault peer: protocol
mismatch, malformed/missing/wrong Hello, incorrect or stale response ID,
truncated/oversized frames, outgoing limits, EOF, stalled input/response, stderr
flood, unsolicited-response pressure and a committed write whose acknowledgement
is lost. Matching application errors keep the connection usable. The tests use
short private deadlines without weakening public operation limits.

The dedicated background reaper provides up to two seconds of orderly-exit
opportunity before attempting direct-child termination/reaping. Linux compiled-
agent tests verify normal EOF/BrokenPipe/malformed-input cleanup of ordinary
owned task groups. A separate forced-agent-termination test demonstrates that
SIGKILL can leave the task running until its own fixture deadline; this is an
intentional negative result, not a remote-cleanup pass. Reconnect does not adopt
old task IDs, resume tasks or replay writes/commands. See
[REMOTE_VALIDATION.md](REMOTE_VALIDATION.md) for the fault matrix and ownership
limits.

SSH option checks enforce strict host keys even for localhost, disable forwarding
and inherited stdio-breaking settings, and narrowly accommodate three option
aliases introduced in OpenSSH 8.7. OpenSSH 7.6 is the argument-set design minimum;
older-client runtime is untested. No authentication test keys, grants, servers or
listeners were created. The proposed isolated pipe-only authentication test is
permission-gated and has not run. Local stdio/option passes cannot replace real
SSH authentication, failure/reconnect and remote-process observation.

## Remaining gates and scope

1. Final aggregate, release build and trust-off native checks passed; later code
   changes require revalidation. Native trust-on and dirty-close checks remain open
2. Record the published phase-5 commit/tag and exact Ubuntu/Windows CI outcome;
   retain failures and distinguish runtime from cross-target compilation
3. With specific approval, exercise native explicit Run/Cancel/reconnect review
   and the bounded authenticated SSH fixture; disclose any remaining gap
4. Keep remote authentication/disconnect/version-capability interoperability a
   core requirement rather than substituting local-backend evidence
5. Keep Windows local execution disabled until Job Objects and cancellable pipes
   are implemented and runtime-verified
6. Continue DAP UI/agent integration only with listener and process-tree security;
   Java/Kotlin project models, current Kotlin compatibility and safe multi-file
   rename remain separate unfinished areas

The phase-3 112.597-second release sample (CJK, recovery enabled, trust off, no
JVM; frontend HWM 117.97 MiB) remains a historical frontend-only measurement.
It does not measure phase 5, establish recovery overhead, or prove an IDEA
comparison. Java/Kotlin JVM samples are separate observations and must not be
summed into a concurrently measured process-tree total. No new memory benchmark
was run for this phase. See [PERFORMANCE.md](PERFORMANCE.md).
