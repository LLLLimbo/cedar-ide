# Verification report · Maven model after server exit / 0.33.0

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
preservation, disabled pointer clicks and the actual disabled tooltip. The locked offline local aggregate passed 1,264 tests with zero failures and
36 ignored opt-ins across 41 suites. All 23 Maven-model tests passed, including
five new closure tests and expanded render/Undo cases. Strict all-target/all-feature
Clippy passed on the host and Windows MSVC target; both all-feature and default
shipping release builds passed. Bundle tests ran 36 cases: 35 passed and one
platform skip. Two independent source reviews found no blocking issue.

The existing synthetic startup helper now advances polling time relative to its
current egui frame, allowing repeated explicit starts without moving time backward.
No production deadline or runtime cadence changed. No new JDT process experiment
was needed for this frontend-only state change. Exact-source dual-platform CI,
retained native gates and regenerated package verification remain pending; earlier
Maven pairs do not substitute for this checkpoint's required run.

## Limits

Observed closure is not a cleanup receipt. An outstanding request may still need
to drain under its existing deadline before the ordinary Stop UI becomes available.
This does not add a watchdog, cancel request, heartbeat or automatic recovery.
Existing Maven leaf/offline-cache boundaries remain: offline dependency resolution
is not network isolation or a code sandbox. Native Windows GUI, authenticated SSH,
full project compatibility and upstream Java diagnostic reliability remain subject
to their previously documented limitations.
