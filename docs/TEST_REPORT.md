# Verification report · Save acknowledgement protection / 0.32.0

The previous [0.31.1 checkpoint](TEST_REPORT_PHASE311_HISTORY_CAPTURE.md) is fully
verified. This checkpoint hardens save acknowledgements from an inconsistent
peer; no failure of the normal agent is claimed.

## Required contract

A successful save reply must match the exact lowercase SHA-256 of its captured
submitted String and the original request, generation, workspace, document/path,
saved baseline and submitted-content token. Current typing, selection, active tab
and profile edits are not substituted for submitted contents.

The check runs before baseline adoption, recovery removal, Maven POM observation,
profile success and Explorer refresh. Malformed or inconsistent replies retain
eligible original submission identity for the existing explicit two-Read check.
No automatic Read, Write, retry or replay is introduced. Stale and duplicate
responses remain harmless; completed authoritative acknowledgements remain ordered
before a later transport-loss event.

A missing or ineligible submission identity cannot be reconstructed from the
current draft. A session-only unverifiable-unknown guard preserves dirtiness,
recovery and close protection, blocks Save/profile/merge and survives reconnect.
Check is unavailable, while Copy/Compare and explicit discard remain available.
Undo back to old baseline text does not erase that uncertainty. No persisted
recovery schema, protocol capability or execution permission is added.

## Local verification and required native acceptance

Completed deterministic coverage includes canonical and wrong-content hashes,
UTF-8/CRLF/empty/1 MiB bytes, newer typing, complete selection and Undo/Redo,
request/workspace/baseline ownership, profile/Maven success-hook rejection,
response-before-EOF ordering, fallback state and durable recovery ownership.

Completed cloud Linux process acceptance uses a marked, bounded, nonshipping fault peer and
the normal release agent. Controlled cases include actual commit plus absent,
empty, oversized, noncanonical and wrong-content acknowledgement, as well as a
noncommitted response. Explicit double Reads establish current contents without
proving original-write provenance or physical file identity. Original interrupted
save cases remain required. Native Windows runtime and final package acceptance
are pending a new exact-source CI run; no prior result substitutes for this run.

## Local results

- Full locked offline all-feature aggregate: 1,259 passed, zero failed, 36 ignored
  across 41 suites. Required process opt-ins below were executed separately.
- Eighteen focused acknowledgement tests plus POM/profile success-effect negatives
  passed. Strict host and Windows MSVC all-target/all-feature Clippy passed.
- All-feature and default shipping release builds passed. The process suite used
  the final default-feature agent and a separately built marked fixture.
- Five new process tests passed: 13 controlled transactions, two normal-agent
  saves, and four marker/control rejection cases. Actual ledgers total 15 Writes,
  42 Reads and 17 successfully opened/reaped connections. Invalid acknowledgements
  cause no automatic operation; each valid normal save retains its one existing
  flat-Explorer List refresh. Rejected Client creation does not claim independently
  observed process reaping.
- All nine existing interrupted-save process tests passed. Packaging tests ran
  36 cases: 35 passed, one platform skip. The existing interrupted-save guide is
  now included and its entry-guide link is checked.

The first new process run passed four tests and failed its normal-agent fixture:
it incorrectly expected no command after a valid save, while the existing flat
Explorer intentionally enqueues one root List. The corrected fixture explicitly
requires that exact List/Job/connected Entries response and no further operation;
invalid-ack ledgers remain unchanged. The original failed run is retained. No
production behavior was changed to satisfy that fixture assertion.

## Limits

A consistent hash does not authenticate the peer or independently establish disk
durability. The real agent already emits the expected content hash. Fault injection
is synthetic and cloud-owned; authenticated SSH and native Windows GUI remain
unverified. Existing intermittent Java diagnostic behavior and explicit-refresh
workflow distinctions remain unchanged. Historical resource trials and failures
are preserved; this checkpoint makes no resource-comparison claim.
