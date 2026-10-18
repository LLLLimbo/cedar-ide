# Phase 79: 0.79.0 development checkpoint candidate

Status at source sealing: **DEVELOPMENT CANDIDATE; final 0.79.0 validation pending**.
This is a version and documentation consolidation, not a release, main-branch
promotion or a new feature implementation. Its exact parent is
[`817c341eacc54645ce9376f3b6dfa0359782efb5`](https://github.com/LLLLimbo/cedar-ide/commit/817c341eacc54645ce9376f3b6dfa0359782efb5),
with source tree `b4451321cd4d8ca38e3cb27569389f816432e653`.

## Included development work

- Java type results support query-to-result Down, result Up/Down/Home/End,
  explicit Enter opening, and existing draft/selection/Undo protection.
- The welcome area exposes its existing overflow through a visible affordance.
- Find Java type opens a focused view while the full Language tools remain
  available and lifecycle warnings keep their existing meaning.
- Automatic height uses measured focused-view content to accommodate up to
  three complete type entries when space permits. Manual height remains explicit
  and Automatic height restores the measured layout.

The checkpoint changes the workspace version, its exact-version test/driver
literals, and current documentation. It does not change the above production
behavior, dependencies, vendored code, protocol, CI gates, timeouts, package
formats, execution trust or cleanup contracts. The inherited window-close
barrier is described in [Window close](WINDOW_CLOSE.md).

## Verified parent evidence, still version 0.78.0

The parent [CI run 39124400977](https://github.com/LLLLimbo/cedar-ide/actions/runs/39124400977)
passed its four jobs: Ubuntu and Windows aggregate/package verification and
their separate real-JDT Parameter info jobs. The prior local receipt records
2,478 passing Rust tests, 75 ignored, across 50 suites; the 30 Python suites
discovered 843 tests with 10 skips and no failed suite. These counts and that CI
run belong to the parent, not to the new version.

The exact parent Linux desktop artifact 12136060703 was independently checked
against its manifest and source documents. A bounded cloud-Linux native run on
that pair used one application launch and one Java start, with a fresh synthetic
workspace and JDT data directory. At default scale, the first explicit query
showed three complete title/container/location entries in an 1180 x 812 client
area. A real manual resize survived an explicit same-query Search; Automatic
height restored the three entries. Down, End and Enter opened the selected type,
and Back retained the original unsaved draft and marker-line selection. Three
complete entries also remained visible at 780 x 880.

That native run lasted 356 seconds, reported Java exit 0 after explicit Stop,
and ended with application exit 0. Java source hashes and the 538 packaged-file
hashes remained unchanged. JDT modified the synthetic .project and generated
three .class files; the import can trigger its own build. The record does not
prove raw protocol-event counts, global process cleanup, all font/scale/window
combinations or Windows/macOS GUI behavior. Raw private screenshots and logs are
not included in this source export; their separate delivered evidence remains
bound to the original parent package and must not be relabeled as 0.79.0.

The preceding keyboard, welcome and focused-view stages retain their own exact
commits and CI runs:

- [Keyboard: cd476ab](https://github.com/LLLLimbo/cedar-ide/commit/cd476ab54eed513e9ebf8ee7764ce01103128f06),
  [CI 39113797244](https://github.com/LLLLimbo/cedar-ide/actions/runs/39113797244).
- [Welcome: a9331f2](https://github.com/LLLLimbo/cedar-ide/commit/a9331f270ac562c8ad6b4d09ee6ae74e3db9f9f7),
  [CI 39116854118](https://github.com/LLLLimbo/cedar-ide/actions/runs/39116854118).
- [Focused view: 9ecc601](https://github.com/LLLLimbo/cedar-ide/commit/9ecc60177538bd625ff8fbf9e2ae704ccf870ef9),
  [CI 39120046330](https://github.com/LLLLimbo/cedar-ide/actions/runs/39120046330).

## Final checkpoint criteria

At source sealing, the following are **PENDING for 0.79.0**. Their later results
belong in a separate receipt bound to the final source tree/commit, CI run and
package hashes; this report is not rewritten to borrow a later result.

1. Format, strict workspace Clippy, full workspace/all-targets/all-features Rust
   tests, the existing 30 Python suites, and source/export/privacy checks pass
   against the frozen new-version source. Only the ten Cedar workspace package
   versions change in Cargo.lock; dependency identities remain unchanged.
2. Independent review confirms the version/documentation-only diff. The exact
   candidate commit and tree are verified after publication, followed by all
   four original CI jobs, including unchanged canonical and package gates.
3. Linux desktop, Linux agent and Windows development archives identify 0.79.0,
   the exact commit and CI run, and pass the existing package checks. Normal
   default-feature frontend/agent binaries stay paired; old binaries are not
   renamed or relabeled.
4. One bounded cloud-Linux check of the final official desktop pair verifies
   version identity, focused type search, measured/manual/reset height,
   keyboard opening and Back with preserved draft, and normal close with the
   recorded postflight observations.

## Remaining limits

Windows CI does not establish Windows native GUI acceptance. macOS GUI, broad
native IME coverage, authenticated SSH interoperability and a comprehensive
native close/descendant-cleanup matrix are not claimed. The historical
unattributed native zombie is neither a proven failure of this exact source nor
a proven fixed result. Missing earlier raw native evidence is not reconstructed.
The original failed checks and historical pre-validation reports remain part of
the record. A failed final gate prevents claiming this checkpoint verified;
unrelated missing platform coverage remains a stated development limitation.

Completion means a usable, explicitly limited development candidate with exact
source, CI, package and validation identities. It does not imply a tag, release,
main merge, deployment, performance advantage or IntelliJ compatibility.
