# 0.70 stale-navigation isolation — candidate verification

## Status and accepted baseline

**0.70.0 is a candidate and is not accepted.** Final versioned local aggregate,
strict checks and four current-artifact process proofs passed; exact-source CI,
all three packages and native validation remain pending. The latest accepted bounded baseline
is [0.69.0 Files search-as-you-type](TEST_REPORT_PHASE69_FILES_LIVE_SEARCH.md#accepted-0690-follow-up),
from [public source 2a1b50a](https://github.com/LLLLimbo/cedar-ide/commit/2a1b50a2e220b910251b4afdbe0f8a83f3915303)
and [CI 38458861975](https://github.com/LLLLimbo/cedar-ide/actions/runs/38458861975).
Its four jobs, three packages, Linux **2,200/0/71**, Windows **2,138/0/114** and
24 new Files checks passed. Its Browse v6 contract is **64 strict fields / 16
operations**, retaining the complete older 57-field / fourteen-operation prefix.
These are 0.69 results and do not establish any 0.70 gate.

The 0.69 bounded native type-without-Enter, scope, Preview/Open/Back, dirty Undo
and cancel/reopen journey passed timing and integrity checks. A click replaced
the planned Down step, so it does not validate native Down entry. The fixed
250 ms interval, request counts, broad IME/CJK, Windows/macOS GUI, real SSH and
positive Structure/JDT GUI remain outside that native evidence. Linux Java
Restart cleanup reported **`grace_expired`, signal 9**, not all-graceful shutdown.
The original 0.68 native startup observation remains incomplete, with no product
startup-failure inference. Earlier failures retain their original classifications.

## Narrow behavioral contract

A normal Open reply can create a new inactive document even after its navigation
was superseded. Caching its exact text and revision remains valid. The candidate
allows creation of that new document's ordinary `jump_to` only while the reply's
navigation identity is current and it is not a pending language-navigation
target. Later Files and Recent ordinary Opens must not inherit a canceled Text
line. They must match a fresh ordinary Open of the same bytes.

The change does not clear all pending jumps. Existing valid background jumps
must remain available when their document renders. Current ordinary line Opens,
plain-Open caret behavior, language navigation, dirty text, revision identity,
Undo/Redo and location-history safety retain their existing boundaries. The
[offline interaction contract](FIND_AND_RETURN.md#stale-navigation-isolation-070-candidate)
explains the interrupted user journey. There is no new backend operation,
protocol, dependency, package inventory, provider, trust or transport contract.

## Old-production negative control

Actual production update frames against the accepted 0.69 source reproduced a
Text line-2 Open whose Read was superseded, then cached as an inactive document.
A later ordinary Open through both Files and Recent consumed the canceled line
and landed at scalar **10**. A fresh ordinary Open of the identical fixture bytes
and a new document landed at scalar **39**. Exact cached bytes, dirty Undo and
history remained safe.

The two positive controls passed; the stale-line rejection assertion was the
one intentional failure. This source-derived, supplied-event result establishes
sensitivity to the old behavior. It is not a previous native GUI reproduction,
normal-agent transport run, Windows result or evidence that native input itself
was faulty. Corrected-source frame and process results are recorded separately below.

## Corrected-source focused checks

The first local focused run passed **five tests / zero failures / zero ignored**:
the canceled Text line regression, the current Text target positive control,
newest same-path pending replacement, retained background-buffer jump, and the
existing Files edit/preview/stale-activation check. This development artifact
still reported **0.69.0**; it is not a final versioned 0.70 aggregate or process
receipt.

Files and Recent each reused exact inactive cached bytes with **zero Read** and
matched independently observed ordinary caret/selection at scalar **39**,
including both affinities. The test does not assume an ordinary Open starts on
line 1. A still-current Text line-2 target continued to jump to scalar **10**;
an existing valid background jump remained honored. For one shared in-flight
same-path Read, the newest replacement's line **3** or absence of a line won.
The original dirty bytes, full selection, Undo and history assertions passed.

## Browse v7 process contract

The authored receipt has **74 strict typed fields / 17 operations**. All **64
v6 fields** remain: removing only the ten added fields and the last Read, and
restoring schema version 6, must reconstruct the complete previous contract.
The exact original sixteen-operation ledger remains the prefix. Its nested
values and the earlier **57-field / 14-operation v5** and **50-field /
14-operation v4** projections retain independent checks.

The sole new ledger operation is **Read(`untouched/deep/Needle.rs`)**. The harness
holds a real normal-client response across a newer Files navigation, then delivers
it through the ordinary event handler. This deliberately controls completion
order; it does not inject a server/network delay or reproduce a native race.
It closes the existing clean target and recreates it with a fresh document ID,
checks exact inactive text/revision and absence of the canceled line jump, then
opens it through Files first, then reuses it through Recent. Independent Recent-
first activation is covered by the separate focused frame regression. Both cached
reuses must match the independently
observed fresh ordinary Recent caret from the preserved v6 journey and cause
zero additional operations. Exact Back, dirty selection, Undo and retained state
remain required.

| Added strict field | Required value |
|---|---|
| `stale_open_harness_delayed_completion` | `true` |
| `stale_open_newer_navigation_preserved` | `true` |
| `stale_open_inactive_cache_exact` | `true` |
| `stale_open_inactive_jump_absent` | `true` |
| `stale_open_files_ordinary_position` | `true` |
| `stale_open_recent_ordinary_position` | `true` |
| `stale_open_back_dirty_undo_preserved` | `true` |
| `stale_open_additional_reads` | `1` |
| `stale_open_cached_reuse_operations` | `0` |
| `stale_open_call_timeout_seconds` | `30` |

Strict validation rejects missing, extra, false or wrongly typed observations,
historical schema versions and any changed or additional ledger operation. The
**17/17 pure parser/driver checks passed** for the authored contract; they do not
run Rust or establish application/process behavior.

The new Read keeps its **30-second** call timeout within the unchanged
**240/30/300-second active/cleanup/watchdog** limits. Driver compile/backend/app
bounds and package inventory remain unchanged. The final **0.70.0** current-artifact
normal-agent execution passed with source/fixture and selected-artifact bindings,
Trust off and one owned agent reaped. Its ledger is exactly **Hello 1, List 5,
Read 7, FindFiles 3, Search 1**. Neither frame nor headless process evidence
establishes native timing, Windows GUI, real SSH, broad IME or positive native
language-provider behavior.

## Preserved proof-development failures and corrections

The first pure-validator run retained an old Recent assertion literal expecting
receipt version 6. Updating that expected literal to version 7 preserved its
assertions; the later complete pure suite passed **17/17**. The initial failure
remains separate from that result.

The first application proof compile failed because a new test assertion accessed
the private `Browse.query` field. The correction calls its existing `query()`
accessor. No visibility, production behavior or proof requirement changed; that
compile failure is not a runtime result.

The first early normal-agent execution then failed the new Back observer after
the stale inactive-cache and Files ordinary-caret checks had passed. It expected
the entire pre-Open Back stack to remain after Back, overlooking ordinary
coalescing of an identical departure at the stack top. The failure retained
verified close and its original evidence. It does not establish a production
history failure.

Independent review checked the bounded test-only correction: assert the exact
independent departure at the prior stack top, the entire unchanged post-Files-
Open stack, the one-entry Back pop and exact Forward destination. Recent then
checks its entire appended/popped stacks. Stale-completion history invariance
remains asserted; the production guard, original prefix, operation inventory,
bounds and strict receipt were unchanged. The corrected early run passed
**one test / zero failures** and the strict **v7 / 74-field / 17-operation**
receipt. Those early artifacts still reported **0.69.0**, before version
synchronization; they do not substitute for the final 0.70 results below.

## Final versioned local checks

The **0.70.0** workspace/all-target/all-feature locked Rust aggregate passed
**2,203 tests / zero failures / 71 ignored across 50 suites**, including the three
new focused regression tests and retained background-jump control. Formatting
and strict workspace/all-target/all-feature Clippy passed. All **29 Python
suites** passed: **806 tests total / ten skipped / zero failed suites**.

The current Cargo-JSON-selected default-feature application and matching normal
agent passed all four strict source-bound process proofs:

| Proof | Strict fields | Operations |
|---|---:|---:|
| Browse v7 | 74 | 17 |
| File Structure | 45 | 11 |
| Scoped Search | 57 | 10 |
| Search | 57 | 12 |

Browse preserved the entire earlier contract, added exactly one Read, and
observed zero operations for Files/Recent cached reuse. Trust remained off,
source and fixture bindings passed, and owned-agent cleanup was verified.
File Structure uses the explicit test-server mock, not native real JDT.
These are current selected-artifact executions with canonical validators; the
canonical compile drivers were not run locally. They do not establish exact
remote CI, package or native acceptance.

## Validation gates

| Gate | State and scope |
|---|---|
| Old 0.69 production-frame negative control | Two positive controls passed; one intentional stale-line rejection failure, with scalar 10 versus fresh ordinary scalar 39. |
| Corrected-source focused frame checks | Early **5/0/0 passed** on a development artifact still labelled 0.69.0; the new regressions and retained control also passed in the final **0.70.0** aggregate. |
| Independent source/proof review | **Passed:** source and proof audit, including the independently reviewed complete Back/Forward observer correction. |
| Extended Browse strict parser and compatibility checks | **17/17 passed**; authored v7 contract **74 fields / 17 operations**, exact v6/v5/v4 projections. Pure parser/driver evidence only. |
| Workspace aggregate, strict Clippy, formatting and Python | **0.70.0 passed:** Rust **2,203/0/71 across 50 suites**; strict Clippy and formatting; Python **806 total / ten skipped / zero failed suites across 29 suites**. |
| Current-artifact normal-agent Browse and retained proofs | **0.70.0 passed:** Browse v7 **74/17**, Structure **45/11**, Scoped Search **57/10**, Search **57/12**. Selected-artifact local proofs; canonical compile drivers not run locally. Structure uses a mock. |
| Offline guide/package link closure | **Passed:** all **162 local links/anchors across seven edited documents**, all three existing current-guide closure checks for Windows, Linux agent and Linux desktop, and byte-exact preservation of the original phase 69 preparation text. No inventory changes or package acceptance inferred. |
| Exact-source dual-platform CI and all three packages | Pending. |
| Bounded native stale-navigation journey | Pending. No prior native bug or fixed-native claim. |

All failed and incomplete attempts retain their original result. Acceptance
requires the relevant final-source gates; local evidence does not inherit the
accepted 0.69 release, and Linux evidence does not establish Windows GUI or SSH.
