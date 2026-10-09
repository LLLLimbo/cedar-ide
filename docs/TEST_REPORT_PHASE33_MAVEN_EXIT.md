# Verification report · Maven model after server exit / 0.33.1

The previous [0.32 checkpoint](TEST_REPORT_PHASE32_SAVE_ACK.md) is fully verified.
This bounded frontend change retires Maven model evidence after an observed
language-server closure. It does not change protocol capabilities or launch policy.

## Required contract

An active Maven model enters a distinct ServerExited state on confirmed closure,
clearing retained model rows and pending adoption ownership. The captured on-disk
POM hash and observation floor remain; dirty POM/Java buffers, cursor selections,
Undo/Redo and saved baselines stay untouched. Repeated closure is idempotent for
model ownership and keeps the explicit exit explanation visible.

The language panel intentionally remains running until its existing explicit Stop
lifecycle is processed. Already-submitted worker requests are not dropped or given
new deadlines. Connected late model replies cannot change state, output or CJK
observation; disconnected replies still report transport loss. Later POM witnesses
cannot replace the exited state with imported or POM-restart status. This prevents
stale presentation and does not establish that processes have been cleaned up.

Check Maven model remains visible and disabled with specific Stop/restart guidance.
No automatic query, reimport, restart, Save, dependency inspection or other worker
command is introduced. Existing Stop success/failure and explicit fresh startup
retain their cleanup and restart-blocking semantics.

## Local verification and pending native acceptance

State and rendered-control regressions cover closure from every model state,
repeated events, pending transport ownership, reply/event order, late reply variants,
later POM acknowledgements, Stop/reset/fresh startup, source and selection/history
preservation, disabled pointer clicks and the actual disabled tooltip. The locked offline local aggregate passed 1,268 tests with zero failures and
36 ignored opt-ins across 41 suites. All 23 Maven-model tests passed, including
five new closure tests and expanded render/Undo cases. Strict all-target/all-feature
Clippy passed on the host and Windows MSVC target; both all-feature and default
shipping release builds passed. Bundle tests ran 36 cases: 35 passed and one
platform skip. The Maven lifecycle and diagnostic follow-up source reviews found no blocking issue.

The existing synthetic startup helper now advances polling time relative to its
current egui frame, allowing repeated explicit starts without moving time backward.
No production deadline or runtime cadence changed. No new JDT process experiment
was needed for this frontend-only state change.

## Preserved native failure and bounded diagnostic follow-up

The exact 0.33.0 run at public b5905c4686978f82f17e92ff1ed4f96d11fd0a67
([CI 37994716814](https://github.com/LLLLimbo/cedar-ide/actions/runs/37994716814))
passed Ubuntu and all 23 Maven model tests on both platforms. Windows failed the
existing synthetic javac acceptance at its first version task: one start,
120 polls, a terminal task observation, and 32.97 seconds total. The receipt did
not capture the terminal state or outcome details. Its unset version string does
not prove empty output; zero frontend Reads means navigation had not begun.
Source integrity, agent reaping and fixture removal passed. The independent
Maven pair passed; ordinary Java and extracted-bundle acceptance were skipped.
That run remains failed, and no package is accepted from it.

The 0.33.1 follow-up changes only test evidence and version/report metadata.
Each explicit task start resets a bounded diagnostic record. The last observed
snapshot records the existing fixed task state, signed and Windows exit codes,
truncation/error-presence flags, retained UTF-8 output lengths, and a fixed
empty/JDK21/other output classification. It never exports raw output, errors,
paths or new version strings. Retained string lengths are not raw pipe-byte counts.
Start and poll RPC timings, maximum poll time and finish elapsed time distinguish
slow calls from repeated live observations. The deadline flag means the existing
nonterminal harness guard rejected a wait; terminal snapshots retain their
original precedence over that guard.

The 30-second task timeout, 45-second finish guard, polling cadence, required
success and cleanup assertions are unchanged. This is diagnostic preparation,
not an identified timeout cause or a runtime fix. Four pure regressions cover
all states, unsigned exit bits, incomplete outcomes, output categories, reset,
timing saturation and exclusion of private-text sentinels. Exact-source dual-OS
CI and every required native/package gate must still pass before acceptance.

## Limits

Observed closure is not a cleanup receipt. An outstanding request may still need
to drain under its existing deadline before the ordinary Stop UI becomes available.
This does not add a watchdog, cancel request, heartbeat or automatic recovery.
Existing Maven leaf/offline-cache boundaries remain: offline dependency resolution
is not network isolation or a code sandbox. Native Windows GUI, authenticated SSH,
full project compatibility and upstream Java diagnostic reliability remain subject
to their previously documented limitations.

## Exact 0.33.1 acceptance

Public source `b5d40a16c9195ce7c289255cc6ed04facb8ad906` passed
[CI 37996976702](https://github.com/LLLLimbo/cedar-ide/actions/runs/37996976702)
on Ubuntu and Windows. Both platforms executed all 23 Maven-model tests and the
four task-evidence regressions. Existing Java, Maven, ownership, file-operation
and extracted Windows package gates passed. The package's 538 payload hashes and
source/run provenance were independently checked. This is not a native Windows
GUI or authenticated SSH result.

The fresh Windows javac workflow passed four starts and four frontend Reads. Its
last-task record describes the final intentional failed compilation (exit 1), not
the initial version request. The previous 0.33 failure remains unexplained; this
pass does not establish a causal fix. Ordinary Java and the required idle workflow
received spontaneous diagnostics in this run; no conditional refresh was used.
