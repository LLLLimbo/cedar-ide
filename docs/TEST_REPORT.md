# Verification report · Windows Maven leaf projects / 0.22.0

The new Maven profile passed its complete native present/missing acceptance pair.
The overall Windows checkpoint remains red because the final bundle metadata test
failed; the corrected bundle still requires fresh exact verification. The supported subset and execution/cache limitations are described
in [Maven projects](MAVEN_PROJECTS.md). Earlier failed checkpoints and their actual
results remain in the [phase 22 verification history](TEST_REPORT_PHASE22_PROBES.md).

## Latest observed native result

Public commit `6ff525e0e61df684ecb5d96f0436660f414f6e5d` in
[run 37882685195](https://github.com/LLLLimbo/cedar-ide/actions/runs/37882685195)
passed Ubuntu and the existing Windows capability, Java and Git gates. The Maven
present case proved its actual model, exact generated dependency, semantics,
POM-change restart, unchanged inputs and graceful root exit zero. Both cases
classified six lifecycle markers (mask 63), eight metadata files total and zero
foreign repository files, while preserving source/cache hashes and cleanup.

The missing case failed. Its first model response was not ready; processing then
rejected a diagnostic for a foreign document, with an error severity and an
unclassified string code/message. The fixed receipt did not identify that URI or
message. No particular historical marker identity is inferred from those categories.
The missing JAR/POM remained absent and the root exited naturally with verified cleanup.

Ordinary Java also exercised the explicit recovery workflow in this run: the reused
session retained a 60,042 ms spontaneous diagnostic timeout, 595 polls and zero
events. Exactly one fixed refresh was acknowledged and its exact unversioned witness
arrived after 507 ms. The original spontaneous verdict remained false while the
recovery workflow passed. This does not establish general diagnostic freshness or
an upstream fix.

## Narrow classifier candidate

Pinned [JDT Core validation](https://github.com/eclipse-jdt/eclipse.jdt.core/blob/6725c16c24d94c83302346dc384bb915a0f2fe1a/org.eclipse.jdt.core/model/org/eclipse/jdt/internal/core/ClasspathEntry.java#L2548)
and [JDT LS publication](https://github.com/eclipse-jdtls/eclipse.jdt.ls/blob/08eafe6ff60c7159ef88571d47b6a9ef82fef94e/org.eclipse.jdt.ls.core/src/org/eclipse/jdt/ls/core/internal/handlers/WorkspaceDiagnosticsHandler.java#L212)
explain a candidate project-directory marker emitted before the POM diagnostic.
This test-only follow-up recognizes it only in the missing case: exact captured
owned directory URI, string code 964, Error severity, Java source, zero range and
an exact reconstructed message containing the generated dependency JAR path.
Both generated dependency artifacts must actually be absent. Wrong paths, codes,
source, severity, ranges, generic errors and present-case markers remain rejected.

The original unresolved model, exact dependency reference, full-GAV offline POM
error, absence, input integrity and cleanup requirements remain mandatory. No
request, cadence, heap, startup, model or cleanup budget is extended. The sanitized
receipt exports only fixed categories and a boolean witness. This checkpoint's
release predicate requires that witness in the missing case and forbids it in the
present case, so an unexercised candidate cannot be called verified.

## Recovery and verification

The cloud executor refresh removed the unpublished local checkpoint and build
cache. Source was restored from the exact public commit above, with all 813 blobs
verified against the GitHub tree. The unpublished candidate was reconstructed
from its retained scope and pinned-source evidence; its old local commit bytes
were not recovered. Fresh verification passed 922 host Rust tests across 40 suites, with 23 explicit
opt-in tests ignored in the aggregate. Strict host and MSVC all-target/all-feature
Clippy, formatting and diff checks passed. Four new Windows classifier tests
compile under MSVC and await actual native execution. The Maven receipt suite
passed five of seven tests, with PowerShell and native Windows metadata skipped
locally; the collector passed 75 of 78 with three local platform/tool skips. Cache
tests passed 11, export tests two, capability tests five, and bundle tests 28 of 29
with one platform skip. Diagnostic enum sets match the collector whitelist exactly.
Independent review found no remaining blocker. Fresh exact native CI is still
required before acceptance.
No new native pass or bundle is claimed here.

## Observed native Maven acceptance and bundle follow-up

Public commit `af8ef4c28826ff681f8e192f8c06f5c675c6b5c4` in
[run 37889046361](https://github.com/LLLLimbo/cedar-ide/actions/runs/37889046361)
passed the complete normal-agent Maven pair in 25,279 ms. The present model was
imported and the missing model unresolved, each after two queries. The new exact
project-marker witness was false for present and true for missing; the required
full-GAV offline POM witness was also present only in the missing case. Artifact
presence/absence, model and semantic requirements, POM-change restart, immutable
inputs and cleanup all passed. Both roots exited naturally with code zero.
Both repositories had six lifecycle markers, mask 63, eight metadata files and
zero foreign files. This establishes the candidate in this run, without assigning
an identity to the earlier rejected marker.

The existing Java gates passed with three spontaneous correction matches and no
recovery attempts in this run. Default shipping compilation and the Windows Git
smoke (1,402 assertions) passed. The overall Windows job nevertheless failed at
the extracted portable-bundle metadata probe, so no Windows development ZIP was
uploaded or accepted.

That test's exact capability inventory omitted the two intentionally added Maven
capabilities. The narrow follow-up adds those two names while preserving exact
capability equality and protocol/root/OS/architecture/version checks. It also
requires both typed Maven operations to reject with trust off before examining
any supplied path, with a fixed `maven_rejected` receipt checked by the packaging
script. Remaining exact capability inventories were audited: the normal host and
Windows task tests already include Maven; the nonshipping generic-language fixture
intentionally excludes it. No production route, extraction path, trust setting or
ownership behavior changes. Fresh full CI and extracted-bundle verification remain
required before package acceptance.

The bundle follow-up passed 922 host Rust tests across 40 suites (23 explicit
opt-in tests ignored), strict host/MSVC all-target/all-feature Clippy, formatting
and diff checks. Bundle tests passed 28 of 29 with one platform skip and export
tests passed two. Independent review found no blocker. These local checks do not
replace the actual extracted Windows probe.
