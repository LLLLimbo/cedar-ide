# One bounded GC diagnostic control

This control asks which collector is actually running and what heap usage and
capacity it reports at collection points. The preceding two long observations
showed low final-window CPU but about 819–840 MiB summed resident working set.
Those values do not identify unused Java heap. No tuning is selected by this
checkpoint, and its native result is pending.

## Fixed nonshipping host

The optional `windows-java-gc-diagnostic` feature builds the separate
`cedar-agent-java-gc-diagnostic` test binary. Its constructor requires exact
synthetic workspace and distribution opt-in markers. Ordinary constructors keep
the profile absent even in all-features builds. The shipping agent CLI and wire
protocol have no diagnostic selector; marker presence alone cannot activate it.
The CI product bundle still includes only the normal cedar/cedar-agent binaries.

The host retains normal capability and execution-trust checks. It never enables
the generic Windows language-validation path or class-file viewer capability.
After the unchanged production launch validation, it permits one trusted
validated launch attempt and inserts exactly this fixed argument before -jar:

```
-Xlog:gc=info:file=cedar-gc-%p.log:uptimemillis,level,tags:filecount=2,filesize=64K
```

Heap ceiling, collector selection, initialization options, Unicode distribution
working directory and the remaining arguments stay as in the production recipe.
There is no arbitrary JVM-argument or log-path parameter. Both marker files and
absence of previous cedar-gc entries are rechecked before the attempt, with a
bounded directory scan. The markers are explicit synthetic opt-in, not a general
filesystem security boundary.

[JDK 21 unified logging](https://docs.oracle.com/en/java/javase/21/docs/specs/man/java.html#enable-logging-with-the-jvm-unified-logging-framework)
supports the selected tags/decorators and PID substitution. Two rotations means
.0 and .1 plus the active file. The 64K rotation size is approximate, so ingestion
has independent strict limits. Logging adds formatting and synchronous file-I/O
overhead; these resource values are not an unchanged shipping baseline.

## Exactly one matched workload

`scripts/windows_java_acceptance.ps1` defaults to no GC control. This checkpoint's
CI explicitly passes `-GcDiagnosticControl`, adding one invocation after the
existing acceptance and two ordinary long trials. It uses the same four generated
Java files, disabled Maven/Gradle importers, fresh project/JDT data, selected, version-recorded installed
JDK and pinned JDT, 30-second initial/correction observation windows and eight stage latencies.
The installed distribution and OS cache have already been used by earlier runs.

The same capability-enforcing Client exercises denied untrusted/generic starts,
real diagnostics, confined definition, completion/import resolution, actual
Apply/Undo/Redo, document versions, correction and unchanged disk source. The
receipt is distinctly windows_java_gc_control / diagnostic_agent_normal_client.
Successful matched-control evidence requires graceful RootExited, root exit 0,
protocol witnesses, joined cleanup and verified client reap. A forced exit cannot
satisfy that natural-shutdown requirement. The existing 240-second test watchdog,
270-second sampler deadline and twelve-minute enclosing CI step remain.

The shipping agent file hash is checked before and after the control. Its
executable is not replaced. No different collector selection, heap reduction, forced GC,
periodic GC, attachment service or listener is introduced.

## Private selection and numeric publication

The test reserves a new fixed private selection file inside the verified JVM
working directory. It records only a bounded typed ownership witness after
checking the retained JVM handle, executable image, creation time, prior-log
absence and natural shutdown. Semantic acceptance is independently required.
The sampler corroborates the same JVM as an owned descendant before closing its
retained handles. Its public result contains only a fixed status and the private
witness's SHA-256, not the PID, creation timestamp or path.

The enclosing control requires that exact digest before the numeric collector
derives the three log names and again after collection. Standalone unbound
collection is labeled selection_binding_verified=false and cannot satisfy this
control. A replaced or changed witness cannot redirect
collection to another JVM. File/link/reparse/identity checks remain in force.
The general crash collector excludes the entire private cedar-gc namespace
before metadata or path-bearing errors, including rejected links and directories.

Only the selected active/.0/.1 files are read: at most 128 KiB each, 384 KiB total,
8,192 lines, 1,024 bytes per line and 2,048 events. Selection input is at most
4 KiB. Oversized, malformed, unstable or unowned input is rejected or explicitly
partial. Raw logs, filenames, paths, PIDs, environment data and exception strings
stay in generated private scratch and are not uploaded.

Public artifacts are:

- cedar-java-gc-control-resources.json: diagnostic identity, timings and sampled resources
- cedar-java-gc-control-acceptance.json: reconstructed semantic and cleanup receipt
- cedar-java-gc-control-numeric.json: collector enum, numeric GC points and file digests

## Meaning and limits

The actual collector comes from an explicit Using header. Supported heap records
contain GC ID, JVM uptime, an allowlisted event kind, used-before, used-after,
heap-capacity-after and pause duration. Heap values are truncated integer MiB
converted to bytes with an explicit 1 MiB quantum. Capacity is the heap capacity
at that collection point, not maximum heap or total process commitment. Used
before collection may exceed capacity afterward when the heap shrinks.

Young-GC post-collection occupancy is not proven live heap. A missing GC event
means not_observed, not zero heap. GC uptime is not silently mapped onto the
sampler's different clock origin or assigned to an idle window. Capacity minus
occupancy is not proven resident memory that could be reclaimed.

A complete semantic run and green CI do not alone prove complete resource
coverage. Resource gaps and partial numeric observations stay explicit. Rejected
ownership/binding fails the control; a safely bound partial report can remain an
inconclusive observation. The control is not repeated to force a preferred result.
Any later option treatment requires its own matched experiment and latency/CPU/
lifecycle tradeoff evidence. No optimization, release resource saving or IDEA
comparison is claimed here.

Format and semantics references: [OpenJDK logging rotation](https://github.com/openjdk/jdk21u/blob/master/src/hotspot/share/logging/logFileOutput.cpp),
[collector header](https://github.com/openjdk/jdk21u/blob/master/src/hotspot/share/memory/universe.cpp),
and [GC timing/heap reporting](https://github.com/openjdk/jdk21u/blob/master/src/hotspot/share/gc/shared/gcTraceTime.cpp).
