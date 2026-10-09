# Verification report · passive idle transport loss / 0.25.0

This checkpoint observes terminal activity already reported by the existing stdio
reader/writer. An idle frontend can show a lost connection without another user
request. See [scope and limits](IDLE_DISCONNECT.md).

The previous 0.24 public source `e54d77cd943173ef214dcefd0cfc10c9bd58e01a`
passed [exact Ubuntu/Windows CI 37899453166](https://github.com/LLLLimbo/cedar-ide/actions/runs/37899453166).
Its actual Java type-query receipt passed all 18 witnesses; the verified Windows
ZIP contained 531 payloads and a manifest. The historical pre-publication report
is retained [here](TEST_REPORT_PHASE24.md).

## Finite acceptance

- No heartbeat, polling loop, new watcher thread or implicit reconnection.
- Registration and notification races cannot lose queued terminal activity.
- Completed responses, including Write acknowledgements, precede idle loss.
  Unacknowledged writes retain the existing unknown-outcome safeguards.
- Generation fences prevent old events from disconnecting a replacement session.
  Drafts, selection, Undo and owned recovery remain intact.
- Dropping a healthy idle worker releases its mailbox and existing process owner.
- Both-platform normal-agent acceptance uses a controlled local pipe, trust off,
  checks a healthy idle traffic window, explicit reconnection and unchanged source.

## Current verification status

The host aggregate passed 997 Rust tests across 40 suites, with 28 explicit
opt-in tests ignored. Strict host and MSVC all-target/all-feature Clippy passed;
cross-compilation is not Windows execution. Independent frontend state and
concurrency/ownership reviews found no remaining production blocker. The reviews
corrected passive unknown-write wording and callback-capture release under a lock.

Python checks passed: export 2, capability 5, Git fixture 6, frozen Maven cache 11,
resource observer 79 and GC collector 36. Collector passed 77 of 81 with four
platform/tool skips; bundle passed 28 of 29 with one skip; Maven predicates passed
5 of 7 with two skips. Native PowerShell checks remain part of Windows CI.

The all-feature release built successfully. Actual Linux normal-agent controlled
pipe acceptance passed: healthy idle had no additional requests, idle loss arrived
without another operation, exact owned-agent reaping succeeded, and dirty draft,
selection and Undo survived. Explicit headless worker reconnection and stale-event
fencing passed with no Write/Run and unchanged source. All six real-worker process
cases passed, including Write acknowledgement before EOF, repaint notification and
dropping a healthy idle worker. The small test control marker is atomically
published so a partial marker cannot introduce a spurious protocol failure.

The default-feature release frontend and agent also built successfully, and the
normal-agent idle acceptance passed again against that shipping agent. Fresh exact
native CI and package verification are required. No Windows GUI, authenticated SSH, silent
network-stall detection or remote cleanup claim follows from these local tests.
