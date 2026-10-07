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

## Phase 2: heap / GC profile experiment

Ten additional sequential runs used one copied and SHA-256-pinned smoke binary,
two rounds in reverse profile order. Every run passed the same ten Java
semantic/lifecycle checks. Final pre-shutdown JVM HWM ranges:

| JVM profile | HWM MiB | Total wall seconds |
|---|---:|---:|
| 512 MiB, default G1 | 481.15–540.43 | 11.49–11.67 |
| 256 MiB, default G1 | 475.14–550.08 | 9.62–11.30 |
| 256 MiB, SerialGC | 467.30–535.94 | 9.89–12.32 |
| 128 MiB, SerialGC | 376.46–395.43 | 10.35–11.51 |
| 256 MiB, SerialGC, -Xms32m | 484.78–501.15 | 9.67–10.11 |

Reducing the heap to 256 MiB did **not** consistently reduce observed RSS. The
128 MiB/SerialGC profile separated in this tiny fixture but is experimental;
large project imports and sustained editing may fail or become slow. The
working default remains unchanged. There is no post-shutdown memory sample or
controlled host-load comparison, and these numbers remain JVM-only.

Full stage-by-stage readings and actual JVM command lines are in
`jvm-profile-experiments/REPORT.md` and `fixed-profile-results.json`. Raw JSONL
metadata inherited a hard-coded 512 MiB description from the unchanged test
binary; recorded actual command lines, not that description, identify each
profile. Preliminary runs whose binary changed were excluded.

The phase-1 GUI sample above predates automatic diagnostics, completion UI and
lazy CJK font support. It must not be represented as a phase-2 memory guarantee.
The newer `measure_gui.py` samples the frontend PID directly through Linux
`/proc`, separately reporting child rusage; this avoids mistaking a separately
spawned JVM's high-water memory for frontend memory. It still excludes GPU-service
memory and is an interactive smoke measurement, not a production benchmark.

### Phase-2 release frontend with Chinese text

A separate 132.114-second Linux release session opened `UnicodeDemo.java` with
workspace execution trust off and no language server running. The existing
19,484,784-byte Noto Sans Mono CJK SC collection was loaded on demand. Direct
frontend-PID sampling collected 1,301 readings; maximum observed RSS/HWM was
**121,364 KiB (118.52 MiB)**, with at most 22 threads. Raw data:
`gui-phase2-cjk-resource-sample.json`.

This is the newer CJK-capable frontend sample; the earlier 78.44 MiB phase-1
number must not be used as its footprint. Neither includes a JVM or GPU-service
memory. Both are short interactive samples, not fixed budgets, leak tests or an
IDEA comparison. JVM observations remain reported separately above.
