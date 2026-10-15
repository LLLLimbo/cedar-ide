# 0.76.0 Files native IME acceptance fix candidate

## Scope and native acceptance boundary

This stage closes the Files query/Directory same-field caret seam left outside
0.75's focus-handoff claim. Stable focus and a raw Commit do not prove that native
egui accepted the input. Files now consumes payload-free receipts from the exact
TextEdit invocation, before its own changed-byte handling can retire the epoch.
The existing owner token, current raw batch/pass, context, epoch, continuous owner
focus, final focus and cancellation barrier remain necessary. An observation is
additional evidence, never a replacement for ownership.

The narrow vendored egui patch records branches actually executed, in native
processing order. Its input index belongs to that widget's InputState snapshot,
not the root RawInput batch; Cedar does not match events by index or text payload.
Absence means unobserved, not rejected or accepted. The first owner observation,
including absence, is consumed once, so an extra layout pass cannot overwrite,
replay or renew it. No native acceptance predicate, filtering/order, String
mutation, cursor behavior, changed flag or Undo mechanism is rewritten.

For an owned composition, Enabled/PreeditApplied start a new completion lifecycle;
Accepted establishes completion; empty or standalone CR/LF Commit cancels it.
Rejected duplicates, Disabled and ignored line-break Preedit alone do not erase
an earlier accepted completion in the same lifecycle. Raw terminal intent and
valid stable/final ownership are still required. An accepted insertion branch
can leave bytes equal or insert zero at the native character limit; native
truncation and existing business validation remain authoritative.

Direct edits retain the reviewed 0.75 contract: outside a live composition, an
actually accepted Commit is an independent edit even when a later empty, ignored
or rejected direct terminal follows. Existing barriers still apply. Independent
owned Text/Paste, same-value Text/empty Paste and genuine byte-changing edits,
including Backspace and Undo, retain their separate routes. Selection-only Undo,
focus movement and idle are not fresh committed edits.

A narrowly guarded barrier prevents actual native Accepted/PreeditApplied bytes
under a wrong raw owner or stale epoch from re-entering admission through the
ordinary changed-byte path when no Files composition is live. Rejected-only or
absent receipts do not invent a native mutation. Separately owned ordinary
Text/Paste can still release the existing barrier. Rejected/cancelled composition
preserves the exact native visible bytes and pauses automatic search until a
valid fresh edit/lifecycle or an actually executed clean Find/Enter. Provider
switch/dismiss clears that local pause state.

The full 250 ms interval, already-completed commit deadline across later focus
transfer, one physical FindFiles flight, latest replacement and retired/stale
reply isolation remain. There is no automatic Read, preview, Open, activation,
save, retry or polling. Project Text, Recent, protocol, literal matching and
validation limits are unchanged. The self-contained packaged interaction guide
is [Find and return](FIND_AND_RETURN.md#files-native-ime-acceptance-076-candidate).

## Completed focused development evidence

These results belong to the separate sealed candidate E, whose package version
is still **0.75.0**. They are not final 0.76.0 full-workspace or release evidence.
The candidate's 1,213-file source-input aggregate has SHA-256
`6681164a64b7a384ccc598634a87b8b6e11214d48c47d5277372963827b55bca`.

- The unchanged original **40-case** investigation matrix, **18 new product
  tests** and **23 native supplemental tests** passed twice. Repeat executions
  establish repeatability, not additional distinct test coverage.
- One independent-context TextEdit bridge-parity check and all **48 original
  retained Files tests** passed. The retained input suite recorded **212 passed /
  one existing ignored**; 24 overlap with the Files suite. The full scoped union
  is **318 distinct passing Rust tests plus one existing ignored test**.
- Python collector/export/bundle suites ran 154 tests: **153 passed, one existing
  host-dependent Windows junction skip**. These are portable fixture checks,
  not native Windows builds or acceptance of shipping archives.
- Workspace and vendor formatting, strict app all-targets/all-features Clippy,
  the actual source-export smoke and independent review passed. Clippy is
  app-scoped; its log retains 33 upstream egui dependency warnings.

All 302 original matrix native traces match the frozen observational prototype:
raw events, pass/time, field bytes, focus, complete native responses/receipts and
cursor observations, documents and preview visibility. All 14 expected rejected
matrix cases emit zero automatic commands; all 14 expected positive cases emit
one. New acceptance traces contain 57 one-command and 16 zero-command settlements,
with no extra command after the later settling frame. Seven direct/mixed/Undo
preservation groups match a separately rebuilt prototype control for all 320
native traces and 38 command records.

New checks cover both fields, same-value accepted/rejected direct commits, extra
passes, multiple terminals/latest lifecycle, cancellation and late Commit,
ordinary recovery and Undo, wrong raw owner, stale epoch, an earlier consumer
changing widget-local event indexes, pointer handoffs, 256/257-byte validation,
zero insertion at the 4,096-character native limit, and genuinely unobserved
input after active native IME cleanup. The bridge checks response, cursor,
geometry and tessellated paint against a separate native context.

Original red evidence remains separate. Two new ownership/cleanup fixtures first
made incorrect native-byte/cleanup assumptions; their failed files/logs remain,
and only those new assumptions were corrected. One ownership prototype-control
attempt reused a fresh cached candidate artifact and is explicitly excluded.
Its replacement required a non-fresh rebuild and demonstrated the intended two
ownership failures at one automatic command versus zero, with nine preservation
controls passing. No original investigation/retained oracle or production limit
was relaxed to obtain the result.

## Vendor identity, licensing and distribution

The relative Cargo patch selects the same **egui 0.31.1** dependency. Its official
crate archive SHA-256 is
`25dd34cec49ab55d85ebf70139cb1ccd29c977ef6b6ba4fe85489d6877ee9ef3`,
with upstream tag 0.31.1 and archive VCS commit
`1669e52a7ccfc3489c1b0999b9ed48894a0b3887`. All 92 archive members remain;
only text_edit/builder.rs, mod.rs and output.rs differ. The exact three-file
observation patch remains byte-identical to the reviewed prototype.

The source export includes 98 vendor files: those 92 members, both unmodified
MIT/Apache-2.0 texts, upstream-member manifest, license-provenance record, exact
patch and Cedar modification notice. The complete upstream licenses were verified
against both the official tag and archive commit. They are also copied once into
the existing binary-bundle inventory under third-party-licenses/egui-0.31.1.
This is one logical dependency and one notice row, not an extra dependency or an
upstream release. See [third-party notices](../THIRD_PARTY_NOTICES.md#vendored-egui-0311)
and the [Cedar modification notice](../vendor/egui-0.31.1/CEDAR-NOTICE.md).

The 343 prior notice rows and 518 prior license files remain unchanged. Only the
two exact egui license files are added. The source exporter preserves pinned
vendor bytes while retaining UTF-8/NUL/sensitive-content checks and the existing
font whitelist. Windows/Linux agent/Linux desktop binary payload allowlists are
unchanged: source, patch and provenance records do not enter binary bundles.
Focused fixture tests cover missing/corrupt licenses, altered vendor identities,
extra/missing members, symlinks and export-byte preservation.

The development export smoke rehashed all 1,213 exported files against its sealed
candidate. Host locked/offline metadata agrees with old 075 after normalizing
workspace paths and the declared egui registry-to-path identity. Windows-filtered
metadata remains blocked for both old 075 and candidate by uncached clipboard-win
5.4.1; an earlier all-platform attempt lacked android-activity 0.6.1. Neither is
claimed as a complete cross-platform dependency resolution or build.

## Final source and pending acceptance

The final source is independently reconstructed from sealed candidate E before
version propagation. The workspace version and its ten Cedar lock entries become
**0.76.0**. Existing driver/process-test version guards and their version-fixture
literals receive only that same exact version substitution; no canonical runner,
assertion, ledger, timeout or workflow behavior is changed. Pinned upstream egui
stays 0.31.1; historical reports retain their
original versions and evidence. Current docs describe this pre-seal snapshot.
The final source must remain immutable after sealing: later results belong to
external receipts bound to that exact source, not backfilled edits here.

Pending post-seal gates include final-version full Rust/Python, strict workspace
Clippy, all-features release, the last default shipping release, exact default/
all-feature executable pins, source/export checks, exact-public-source CI and
all three shipping-package audits. **The five original canonical drivers are
NOT RUN locally.** Only their original official workflow execution on the exact
public source may establish that canonical acceptance; focused tests, alternate
runners and old-version results cannot substitute for it. No local commit,
publication, release artifact or future test outcome is preclaimed by this report.

Synthetic raw events through real egui controls provide deterministic native
branch evidence. **Real OS IME and a new GUI journey remain untested**; Windows/
macOS GUI, broad CJK coverage and platform input-method behavior remain unverified.
The recording worker proves emitted operations, not real backend traversal or
external data transmission. No OS input method is installed for this work.
