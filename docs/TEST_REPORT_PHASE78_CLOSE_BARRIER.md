# Phase 78: source-only local-owner close barrier candidate

Version: 0.78.0. This is an implementation-scope snapshot, not a passing validation
report. The source starts from the green 0.77 Project-context fixture revision.
Historical phase reports remain historical and are not evidence for this change.

## Scope

- Typed client connection failures retain local ownership provenance while legacy
  String APIs preserve timing. The same process owner is observed after failed
  Hello/validation/cancellation; uncertain setup never becomes no-child proof.
- Worker completion is independent of UI generation and panic-safe. The app owns
  current/retired registration bookkeeping and a bounded sticky error summary.
- Native Close reaches a final stable-pass barrier after existing recovery, task,
  language and mutation checks. Keep editing, reconnect, restore cancellation and
  late/duplicate events retain the owner ledger and exact draft protection.
- The direct-child two-second grace, backend protocol, dependencies, vendor
  patches and CI gates are unchanged. Existing canonical acceptance driver
  behavior and gates are unchanged apart from version literals.

## Intended checks, not execution results

The new source includes deterministic ownership/error cases and full-update raw
Close regressions with recording workers and controlled font delivery. Existing
process suites are adapted to consume owner receipts without relaxing their
cleanup or cancellation assertions. Exact added test names and static review
receipts belong to the separate candidate delivery record.

No local Cargo, rustc, Rust tests, Python test suites, Clippy, release build,
canonical process driver, mechanism experiment or GUI was run for this source
stage. Direct rustfmt parsing/format checks, if recorded separately, establish
format/parseability only and cannot establish type checking or test success.
Official exact-source candidate-branch CI is still required before promotion; no
public main push or release is implied by this source snapshot.

The previously observed native zombie was not attributed by exact startup
identity. The prior isolated process diagnostic was invalid before peer creation
and its supervisor session cleanup was unverified. Those observations are not
claimed as a reproduced product failure or fixed native result here.

See [the intended window-close contract](WINDOW_CLOSE.md).
