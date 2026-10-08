# Two unchanged-recipe Java observation runs

This experiment measures the existing normal agent/Client Java route twice on
one Windows runner. It keeps the production 512 MiB Java heap ceiling, JDK/JDT,
source fixture, capability/trust checks and semantic workload unchanged. It does
not test a different collector or heap size and does not change user defaults.
Native results for this checkpoint are pending.

The earlier two-second post-diagnostics window still contained substantial
activity and working-set growth. A heap ceiling is not a bound on total JVM
resident memory, and a working-set reading does not identify live Java heap.
The new experiment first asks what happens during a longer, explicitly bounded
observation before proposing any memory tuning.

## Fixed workload and budgets

The existing Windows acceptance script runs its previous direct, fixture,
ownership and quick normal-production cases first. It then invokes the new
ignored test `real_windows_normal_agent_java_resource_baseline` twice as separate
processes, using the same prebuilt driver and release agent. Each trial has its
own generated project, fresh external JDT data, marker file, private transcript,
public metrics and sanitized acceptance receipt. The installed JDK/JDT and OS
file cache are reused; this is not a cold-machine benchmark.

Each trial waits exactly 30 seconds after its exact initial diagnostics and
another fixed 30 seconds after corrected diagnostics, before didClose. It does
not keep waiting until a favorable sample appears. The existing 240-second test
watchdog, 270-second observer deadline and twelve-minute enclosing Windows
acceptance step remain. Deadline termination is failure, not cleanup evidence.

The test uses the same four generated files, disabled Maven/Gradle importers and
normal capability-enforcing Client. It still rejects untrusted and generic
startup, verifies JVM identity, diagnostics, definition and confined URI,
completion/deferred import resolution, actual editor Apply/Undo/Redo with
versions 2/3/4, corrected diagnostics, unchanged disk source, Stop outcome,
retained-root exit, client reap and generated-root removal. Each separate
transcript must produce exactly one complete existing production receipt.

## Timing and resource interpretation

The two phases are post-diagnostic observation windows. Neither declares that
indexing, GC or other background activity has finished. The observer reports the
last ten seconds of sampled observations with actual coverage, gaps and missing
values. Short or incomplete observations remain explicitly unknown/partial.
There is no memory/CPU pass threshold or automatic settled-idle label.
The ten-second target ends at the final stable sampling sweep, not at a Rust
marker timestamp: the two clocks have different origins. At the default 200 ms
cadence, window coverage permits up to 400 ms at sampling edges/gaps, including
the trailing phase-observation gap. The report exposes those actual gaps. CPU
integration includes only whole counter-read intervals, without interpolation.

Eight successful-stage latency measurements use monotonic nanoseconds:

- Initialization includes Java startup and verified JVM identity.
- Open-to-diagnostics ends at the exact initial diagnostic assertions.
- Definition includes confined-URI resolution and validation.
- Completion includes candidate validation.
- Deferred import resolution includes edit/identity validation.
- Editor timing spans actual Apply/Undo/Redo and versions 2/3/4 acknowledgements.
- Correction ends at the exact corrected diagnostics.
- Stop ends at the retained-root exit check, excluding didClose and later client reaping.

Failed stages do not invent a successful latency. Individual observations and
ranges are reported; two trials cannot establish a p95. CPU uses the repaired
per-process counter intervals and timing-only uncertainty. Integrated evidence
is still per process; a sum of independent intervals is labeled accordingly.
Resident-memory totals sum one sampling sweep and may double-count shared pages.

## Evidence and comparability

`windows_java_acceptance.ps1` produces:

- `cedar-java-long-idle-trial-1.json` and `cedar-java-long-idle-trial-2.json`
- `cedar-java-long-idle-acceptance-1.json` and `cedar-java-long-idle-acceptance-2.json`
- `cedar-java-long-idle-comparison.json`

The existing acceptance text records source, toolchain, OS/runner and verified
JDK/JDT metadata. The metrics record workload, fixed heap recipe, observation
budgets and input-file fingerprints. Fingerprinting accepts regular input files of at most 512 MiB, reads a bounded
amount and verifies stable file/path identity. Hashing input executable files
is not an attestation of all loaded pages or libraries. The enclosing acceptance script
verifies the pinned JDT archive; the sampler does not independently reverify it.

Comparison requires matching source, workload, dependencies and input binaries.
Each trial's raw stdout/stderr and marker file stay inside generated private
scratch. Only reconstructed fixed labels, numeric metrics/latencies and validated
fingerprints are published. Missing measurements remain unknown; malformed or
inconsistent reports cannot yield a successful comparison.

These two runs describe variability under one unchanged configuration. The
previous brief-run tree maxima of about 826 and 849 MiB came from different runs;
the difference is not evidence of a regression or optimization. This experiment
still excludes GUI rendering and real SSH and does not compare Cedar with IDEA.
Any later tuning needs matched repeated evidence for memory, CPU, interaction
latency and lifecycle behavior. No tuning is part of this checkpoint.
