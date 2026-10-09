# Verification report · Windows Maven leaf projects / 0.22.0

This checkpoint adds an explicit, trusted Windows Maven leaf profile to the normal
release agent and Client. Ordinary Java remains a separate mode. The model request
reports bounded source/compiler/classpath information from m2e and binds it to the
on-disk root POM captured at startup. Unsaved editor drafts remain independent;
a subsequent accepted POM read/save or backend disk-hash change requires restart.
See [the supported subset and trust limitations](MAVEN_PROJECTS.md).

## Finite acceptance

The new Windows stage prepares an exact, published-digest-checked 83-file Maven
cache from official Maven Central before import. It then runs one generated
present-dependency project and one missing-dependency project through the normal
release agent and Client. The positive case requires actual Maven nature, custom
source folder, compiler 17 settings, exact library identity and Java semantics.
The negative case requires the exact unresolved dependency reference, a matching
offline POM error and continued absence of its JAR/POM. Both require trust/default
and stale-session rejection, changed-POM restart, unchanged inputs, bounded metadata,
retained process identity, joined cleanup and a reaped Client.

Maven dependency resolution is offline. This is not a network-isolated language
server: the stock distribution can request public Gradle version metadata and
trusted configurators can execute code. No user project or credentials are used
by acceptance. Cache preparation has a separate fixed manifest and no redirects;
no artifact payload is included in the development bundle.

The test-only cleanup allowance is 90 seconds, covering the existing 75-second
Stop RPC plus verification. Production grace and RPC deadlines are unchanged.
Phase admission reserves cleanup inside an unchanged absolute 360-second pair
budget; late cleanup or inadequate remaining budget fails acceptance. This fixes
a pre-runtime test-budget inconsistency, not a performance result. Graceful and
forced stop outcomes remain distinct in sanitized receipts.

## Verification status

The final local aggregate passed 916 Rust tests across 40 suites, with 23
explicit opt-in tests retained for native/process stages. Strict host and MSVC
all-target/all-feature Clippy passed. Cache preparation tests passed 10; Maven
receipt tests passed three with the PowerShell execution test skipped locally.
The existing sanitized collector passed 75 of 78 tests with three platform/tool
skips; bundle tests passed 28 of 29 with one skip, and export tests passed two.
The resource observer and numeric collector suites passed 79 and 36 respectively.
The normal default-feature release app/agent build and four actual-agent
protocol, capability, language and task smoke suites passed. Formatting and diff
checks passed; independent source review found no remaining blocking issue.
No Windows Maven runtime or new ZIP is claimed before exact native CI completes. The preceding
[0.21.1 report](TEST_REPORT_PHASE21_RECOVERY.md) preserves the diagnostic recovery
criterion: spontaneous diagnostic timeouts remain separately reported, and an
explicit refresh is not an upstream fix. No GUI Trust, authenticated SSH or local
user-computer verification is included in this checkpoint.

## Initial native checkout failure and bounded repair

The first 0.22 source, public commit
`96b6cc9c72b542e90f73139b6e8aeac0ffad80e5`, failed the early Windows
cache-manifest identity test in [CI run 37875938163](https://github.com/LLLLimbo/cedar-ide/actions/runs/37875938163).
No Maven cache preparation or native Maven import ran. The expected frozen SHA-256
remains `2afceba6a8f6b648a1dbf48cc356b931233cc57bc82b52d02fefdd58e5e876ac`.

A generated repository reproduces Git `core.autocrlf=true` converting that LF
manifest to different CRLF bytes. The native failure log did not include the
actual file digest, so that mechanism is consistent with the failure rather
than directly proven on the failed runner. The follow-up pins LF checkout only
for this exact manifest and adds a real Git regression with an unprotected CRLF
control. All 11 cache tests pass locally. Manifest bytes, digest verification,
production Rust and Cargo versions are unchanged; prior Rust/build results apply
to those identical inputs. The test now exposes only the public manifest digests
if identity fails. A fresh exact native CI run remains required.
