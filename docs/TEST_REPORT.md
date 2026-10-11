# Verification report · Single-tab Save and close / 0.51.0 (pending)

Latest accepted checkpoint is [0.50 Copy to new draft](TEST_REPORT_PHASE50_COPY_DRAFT.md#final-verified-acceptance--0500), public `1b25b82935ef26a6d9b99be14fbd9cd16d368490` and CI 38102657165. Historical failures and unknown Git/client failure causes remain preserved. Windows recovery admission remains paused.

This pending slice adds Save and close for one captured editor tab. It reuses the existing conditional Save and exact submission acknowledgement. Acknowledgement alone cannot remove a tab: final-frame checks must retain it after newer edits, cancellation, conflict, unknown outcome or session change. Keep editing cancels only the close intent after submission; the Write may still finish. Discard consent is also bound to the exact captured tab/content/session. Dirty profile forms require explicit Save profile first.

No workspace file is deleted. Existing recovery removal is separately tracked and is not proved by a file-save acknowledgement. This is not Close Others or a global Save and quit redesign.

Local Rust compilation, aggregate tests, releases and process runtime are deliberately deferred to fresh CI because the retained disk reserve is approximately 797 MiB. Local validation is limited to source review, formatting and Python/package checks. Full dual-platform CI, fixed real-agent/controlled-peer ledgers, all three packages and fresh exact-package cloud GUI are mandatory before acceptance. No previous binary substitutes for this source.

## Local source and Python checks

Independent data-loss/acknowledgement-order/input review and process-owner/driver review passed. Review caught and corrected a self-modal interference in the competing-navigation guard; the production-frame test now retains the prompt across three frames before the actual Save button and acknowledgement. This authored regression still requires CI execution. Twenty new pure/frame Rust tests are authored but **not compiled or executed locally**. Source formatting and diff checks are permitted; a successful formatter parse is not a Rust type check.

The new Python validator passes 23 adversarial tests; the retained Copy validator passes 21. Local package families pass Linux desktop 39, Linux agent 30 and Windows bundle 36 (one platform skip). No local aggregate, cross-target compiler check, release binary or new process execution is claimed.

## Required runtime ledger and ownership boundary

The six fixed cases are acknowledged close of an existing file, acknowledged creation/close of a new file, conflict retention, controlled wrong-digest uncertainty, Keep editing after dispatch, and newer editing before acknowledgement. Each issues exactly one Write. Per OS the required total is 27 requests: six Hellos, ten Lists (four ordinary successful-save refreshes), five Reads and six Writes, across six observed reaped owners. Another active dirty tab must remain intact. The controlled peer is marked nonshipping and supplies only the wrong-digest case; its observation does not claim a normal agent produces malformed acknowledgements.

All cases share 115 seconds of active work and a five-second final cleanup reserve within 120 seconds total. Full call admission, immediate child ownership, recovery quiescence/disposal and uncertain-cleanup fixture retention follow the previous Copy proof. No success receipt is printed until every case settles ownership and verifies expected file/recovery contents. This is cooperative observation, not kernel/thread cancellation. The separate driver allows compilation120, selection15, runtime180 and three final five-second reaps: 330 seconds inside a six-minute CI step. Existing production timeouts remain unchanged.
