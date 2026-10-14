# 0.72.0 Files layout candidate

## Scope and visual checkpoint

This stage updates only Files presentation in browse.rs and a read-only loaded
candidate annotation accessor in navigation.rs. Text and Recent layouts, the
backend/protocol, physical request limits, 250 ms submission, IME/input ownership,
stale guards, ordinary Open, and dirty/Undo behavior are unchanged. Version
propagation also updates exact process-test and validator oracles; it does not
weaken their schema, operation counts, timing bounds, canonical paths, CRLF or
immutable-tag byte checks. No dependency was added or updated.

The same three-file Trust-off fixture and 1178x814 Linux native viewport were used
for the earlier 0.71.1 layout and two Files development candidates. The accepted
second Files candidate uses a roughly 734x522 popup, full-width single-line
filename/parent rows, readable muted status/footer text, and an explicitly
requested lower preview of about 160 points. The normal font-derived capacity is
stable across asynchronous results; three results do not shrink the window.
Actual paired tests/main.rs images show the same three-line source with and
without preview, then Hide and cancellation. CJK text is readable in these
images. This is hierarchy/proportion evidence, not IDEA pixel equivalence.

The first native attempt was blocked before Connect by a harness-created 0755
recovery directory. Cedar correctly refused unsafe storage; the startup guard
terminated that attempt. Its evidence is retained and is neither a Files failure
nor a Files pass. Two subsequent independent private-0700 runs exited normally
with no owned app/agent processes or windows left. Final candidate postflight
verified the 1,105 checkout and source-archive files, frozen default-feature pair,
528 manifest entries, unchanged fixture, and 1,166 older evidence files.

The accepted visual candidate still reported package version 0.71.1. Its source
seal was ca482d5e57a4321107b46895a4983f079c1e87e1888fa62eca4ec2e8be0a4fc9,
app SHA-256 65e8acd2c3ece9134340b1bfea31336fb0f9bc459c07cee317e1368326749b88,
and agent SHA-256 c26423ae3a298a5cf05efe0003a0d70abe8a8aafb7e85da2dae14f691b41dc56.
Final 0.72.0 binaries require their own source-bound validation and are not the
executables used for those images. Raw captures and logs are deliberately not
included in the public source checkpoint.

## Focused development results

On that exact visual-candidate source, 184 focused tests passed, with one existing
process acceptance ignored: Browse 91, scoped search 16, search keyboard 43,
browsing shell 17, Text preview 17. App-only strict all-features Clippy, formatting
and diff checks passed. Full same-source workspace checks are a separate gate.

Actual production-frame tests cover normal 1178x814, supported-minimum 780x540,
and resilience-only 700x480 at 13 and 18 points; first visible frame after egui's
invisible sizing pass, pending/first-result/settled states, explicit preview,
Hide, and scrollable long errors. The fixed footer and strict preview clip are
checked without waiting for settlement to conceal movement. At least 12/8/6
complete short rows respectively remain without preview, and preview retains
three result rows and three measured code lines. Heading/footer alignment is
checked from painted galleys, not only response rectangles.

Forced-width decomposed accent, skin-tone modifier, and ZWJ-family cases verify
complete original layout text, actual full hover text, keyboard focus and exact
Read path. Non-ASCII elision no longer reconstructs scalar fragments. The existing
renderer may still cut a visual grapheme or hide a suffix; broad glyph coverage
is not claimed. The original accessibility label is directly retained by source
review, not asserted as a runtime accessibility integration result.

Failed development logs remain preserved: attempted ui.put alignment was still
centered until replaced with a bounded left-aligned child UI. Initial tooltip
tests lacked a real timed pointer trajectory; the egui velocity history had no
movement timestamp, so its post-click tooltip rule correctly suppressed the
hover. A real three-sample approach fixed the fixture. Added OutputEvent assertions
were removed because the guarded modal route did not emit that event; no product
behavior or old regression assertion was changed to make those tests pass.

## Final 0.72.0 local and publication gates

Final-version local validation passed:

- Rust: **2,240 passed, zero failed, 71 ignored across 50 suites**, using default
  libtest parallelism for the workspace/all-targets/all-features gate
- Python: **821 total, 811 passed, ten skipped, zero failed suites**, all 30
  scripts/test_*.py validator suites
- Workspace/all-targets/all-features strict Clippy with `-D warnings`, formatting,
  and aggregate all-features release build: passed with the exact 1.99.0 toolchain
- Installed-system-font CJK opt-in: passed, loading Noto Sans Mono CJK SC face 7
  from the existing 19,484,784-byte font and retaining identical Latin metrics;
  no font download or installation
- Unmodified canonical Browse driver: backend proof plus app v7 receipt,
  **74 fields / 17 operations**
- Unmodified Scoped Search: **57 / 10**; Search Navigation: **57 / 12**;
  File Structure: **45 / 11**, using the explicit mock LSP rather than JDT
- Additional unmodified Report Triage compatibility driver: **55 / 10**

The exact default-feature app/normal-agent pair was built after the aggregate
all-features release build in a separate output namespace, frozen as independent
regular copies, and copied to each driver's required canonical release paths.
The app SHA-256 is d32f9bbecc472abc2d45d45473fae3457f58271d1605bcfb31c39daa4d776e8c;
the agent is 4db92785f6edea8f693e628ebf9aa152b5e786051e4f85f6dd35db670bec9d1a.
The mock is a separate nonshipping test fixture. All 1,106 source files and both
frozen/canonical copies were checked before and after each driver. Canonical debug
artifacts were prewarmed without relaxing the drivers' 120-second compile bounds,
exact path/no-symlink admission, schemas, operation ledgers, or cleanup checks.
Cached validation preceded the proof-input source seal; the final canonical
proof rechecks followed it. This later chronology-only report correction changes
no execution input and preserves the original proof identity.

An initial Python pass correctly rejected a new relative phase-report link that
was not included in the offline package. The guide now follows its existing
repository-report-link convention. The failure is retained; no package file list
or strict link validator was weakened, and all 30 suites were rerun.

Exact-source dual-platform CI and all three distributable package audits remain
separate, pending publication gates. No new native run of the 0.72.0 versioned pair
is claimed: its Files implementation bytes are unchanged from the independently
reviewed 0.71.1-versioned visual candidate described above.

## Remaining limits

The final visual pair reached loaded preview before its first capture: it does
not prove a native pending-preview frame. Earlier native pending-search and
headless pending-preview tests remain separately attributed. Narrow/large-font
native windows, Windows/macOS GUI, real SSH, arbitrary Unicode grapheme rendering,
broad IME/CJK coverage, positive File Structure/JDT GUI and native Ctrl+F12 remain
unverified. File Structure process evidence uses the existing mock LSP rather
than real JDT. Text/Recent were not visually redesigned in this stage.
