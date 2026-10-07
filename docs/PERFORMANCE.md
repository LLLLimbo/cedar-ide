# Resource use and honest benchmarking

> Public-source note: named raw logs, screenshots and measurement payloads are omitted from this repository. See [verification evidence](../PUBLICATION.md#verification-evidence).

The UI uses native egui/Glow and contains no embedded browser/JVM. That does not establish a quantified advantage over another IDE. Java and Kotlin language servers, build daemons, debug adapters and remote agents must be accounted for separately.

Implemented controls:

- Event-driven repaint, no continuous project indexing or permanent UI polling loop
- Workspace I/O off the UI thread
- Files read at most 1 MiB; wire frames at most 8 MiB; editor capped at 32 tabs
- Bounded directory/search walking, results, total snippets and runtime
- Bounded process output and timeouts; no trusted tasks run automatically
- LSP response/event/outbound queues bounded; overflow surfaced
- Plain-text fallback for syntax coloring over 256 KiB; in-file results capped at 10,000

These limits intentionally trade functionality for predictable first-stage behavior. User-pasted drafts are preserved rather than silently truncated and can exceed the save limit. The whole process is not a hard-memory sandbox.

## Reproducible next benchmark

Use the same hardware, OS, display resolution, repository revision, JDK, language server and background processes. Measure cold start, warm idle, opening 10/100 files (respect the current 32-tab limit), search, completion latency and reconnect. Include parent and child RSS/PSS, CPU, disk I/O, startup time and p50/p95 interaction latency. Run several repetitions; report project size, warm-up, compiler mode and GUI driver.

Compare equivalent enabled feature sets. A small editor without full indexing/debugging is not comparable to a fully warmed professional IDE. Keep backend, GUI/GPU, JDT LS/JVM and build daemon numbers separate, then report the complete local and remote totals. Do not describe a debug build or a one-file fixture as a production performance benchmark.

## Recorded smoke samples · 2026-10-07

Linux x86_64/glibc 2.41, optimized release profile. The native UI was opened for
277.33 seconds, with the two tiny Java/Kotlin demo files, manual interactions and
the deterministic mock LSP process. Python `resource.getrusage(RUSAGE_CHILDREN)`
reported **80,320 KiB (78.44 MiB) maximum RSS**, 13.02 s user CPU and 0.62 s system
CPU. This is a smoke-sample measurement, not an idle/peak guarantee; Linux reports
the largest child's high-water RSS, not a concurrent sum across a process tree.
GPU service memory and a real Java/Kotlin JVM are not included. Raw data:
`gui-resource-sample.json`; wrapper: `scripts/measure_gui.py`.

A separate agent-only fixture is reproducible with
`python3 scripts/measure_agent.py target/release/cedar-agent`. It creates 500
synthetic Java files, lists them, reads ten, runs one bounded text search, captures
Linux VmRSS/VmHWM and exits. Raw measurements and fixture sizes are preserved in
`agent-resource-sample.json`. Neither sample ran IntelliJ IDEA or a large imported
production project, so neither establishes comparative savings.

### Java service measured separately

The official JDT LS 1.61.0 / OpenJDK21 rerun used a 512 MiB heap cap. JVM-only
Linux RSS/HWM snapshots were **254.15 MiB** after initialize, **485.26 MiB** after
semantic queries and **524.68 MiB** after the correction. Native/shared JVM
memory is outside the heap cap. These independent measurements must not be
added to the earlier GUI sample as a measured simultaneous total. The real Java
toolchain is substantially heavier than the Rust frontend or bare agent; remote
development moves this cost to the server rather than eliminating it.
