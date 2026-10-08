# Observational process-tree resource baseline

`scripts/windows_java_acceptance.ps1` records resources during its existing
normal-agent Java acceptance run. It builds the library test executable before
sampling, then runs that executable once beneath `measure_process_tree.py`.
There is no additional JVM launch, resource threshold, or capability gate.
The original test result and existing typed semantic/lifecycle receipts still
decide acceptance. An incomplete observer report does not change a passing test
into a resource failure.

The report is `cedar-process-tree-baseline.json`, beside
`cedar-windows-java-acceptance.txt` and `cedar-java-crash-diagnostics.json` in the
runner scratch output directory. Run the normal acceptance script with its
documented dependencies and enclosing deadline; the baseline is collected as
part of that command. Do not upload its generated private scratch files.

## What the run measures

The root is the headless `cedar-app` library test executable. Descendants are
classified by verified executable path as the normal shipping `cedar-agent`,
the selected JVM, or other descendants. This exercises real client, agent,
Java diagnostics, completion/import resolution, editor apply/undo/redo and
shutdown. It does not render or measure a GUI.

Windows records resident working set bytes, cumulative CPU deltas, thread counts
and process handle counts for the observed tree every 200 ms. CPU percentages
use one logical core as 100%, so a process or tree can exceed 100%. Aggregates
sum only the measurements within the same sampling sweep. Reported peak RSS is
the maximum such tree sum, never the sum of independently timed process peaks.
Summed resident working sets can double-count shared pages and are not private
or proportional memory. `sweep_ms` exposes the skew of sequential process
queries; samples are not atomic snapshots.

Windows keeps query-only process handles with verified creation times. Parent
creation/exit times must contain the observed child's birth before admitting a
new descendant. Already admitted children remain tracked if they are
reparented. Only numeric report-local instance IDs are published. Sampling may
miss processes that start and finish between sweeps; this is an observational
baseline, not proof that every process ever created was observed.

A supporting Linux `/proc` backend has a generated Python parent/child smoke
test. It verifies process start ticks around reads and treats disappearance,
PID reuse and denied reads as unknown. Its `handles` field explicitly means
open file descriptors, which is not equivalent to Windows handle count. This
supporting test is not a Linux Cedar, GUI or Java resource result.

## Readiness and repeatability

The script supplies `CEDAR_RESOURCE_PHASE_PATH`, an exclusively created file in
its generated scratch directory. Only the ignored production test uses it.
Absent instrumentation, normal tests do not add an idle delay. Fixed markers
contain only a phase and elapsed milliseconds:

1. `starting`: the test starts, including its untrusted-start denial check.
2. `java_initialized`: selected JVM identity verified after initialization.
3. `semantic_ready_idle`: exact initial diagnostic assertions pass. The driver
   waits two seconds without issuing additional editor operations.
4. `query_workload`: normal semantic and editor transactions resume.
5. `cleanup`: language stop, agent reap and fixture cleanup.
6. `complete`: the typed production receipt has been emitted.

The two-second idle interval means “after the exact initial diagnostics.” It
does not assert that background JDT indexing, GC, or other activity has settled.
The sampler reads markers before and after each sweep. It discards CPU deltas
for a new identity, a phase transition, an unstable sweep, or the next interval
after an unstable sweep. Phase summaries exclude sweeps overlapping a marker.
This avoids calling initialization/query CPU idle CPU.

Metadata states the source commit, debug library-test driver, release agent,
fresh generated project/JDT data, pinned JDT version, Python version, logical
CPU count, interval and deadline. The adjacent acceptance text supplies Rust
toolchain/target, OS/runner image, public JDK version and pinned archive hashes.
The OS file cache is uncontrolled: previous acceptance runs have already used
the JDK/JDT binaries. This is a fresh project/data run, not a cold-machine run.
The JVM uses the normal production launch recipe, without a profiling agent.

The sampler, Cargo/compiler, earlier acceptance runs, GUI rendering, SSH and
unrelated system services are excluded. Sampler CPU overhead is excluded from
the totals but can still perturb the measured run. The headless driver itself
is included and is a debug test executable, so it is not a release GUI proxy.

## Schema and incomplete observations

Schema version 1 has fixed metadata, measurement definitions, typed phase
events, a bounded list of samples, per-phase summaries, driver exit status,
timeout status and fixed issue codes. Each sample includes numeric resource
values per instance and role, their same-sweep aggregate, per-metric
completeness, required-role observation and phase stability.

An unseen role has null values. A verified exited identity can contribute zero
live resources. Denied access, missing/raced reads, PID identity mismatch,
enumeration failure or process limits make the affected sweep incomplete;
missing data never becomes an invented zero. CPU without two stable endpoints
is null. Summaries expose the number of valid samples behind each maximum.
Values in incomplete reports are partial observations, not a complete baseline.

The observer limits itself to 64 admitted processes, 1,600 samples, 32,768
enumerated system processes, 8 KiB of markers and a 270-second run deadline.
The existing production test watchdog is 240 seconds and the containing CI
step is twelve minutes. A sampler deadline can terminate only its directly
owned test driver. Its report marks that failure incomplete and makes no
independent claim that agent/JVM cleanup succeeded. Product-owned process jobs
and the existing lifecycle assertions remain responsible for cleanup evidence.

Only fixed roles/statuses, numbers and validated source identifiers enter the
public report. It contains no PIDs, executable paths, command lines, environment
values, exception text, private JDT output or JVM crash logs. Child stdout and
stderr remain in the existing private transcript for the separate allowlisted
crash/acceptance collector. Hostile or unexpected marker fields are rejected.

## Validation and interpretation

Run `python scripts/test_process_tree_baseline.py` for synthetic identity,
reparenting/PID reuse, same-sweep aggregation, missing metrics, phase crossing,
privacy, exit-code propagation and the native Python process-tree test. Native
Windows execution is necessary to validate the Win32 backend and obtain a
Windows baseline; Linux tests or cross-compilation do not establish that result.

This baseline supports future resource work. It does not establish that Cedar
uses fewer resources than IntelliJ IDEA. Any comparison needs equivalent
hardware, OS, JDK, project size, plugins, indexing state, cache conditions,
workload, readiness, sampling and whole-tree inclusion. A headless Cedar test
versus a full IDEA GUI would not be an equivalent comparison.
