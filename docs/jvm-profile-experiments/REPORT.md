# JDT LS small-fixture memory experiments

> Public-source note: named raw logs, screenshots and measurement payloads are omitted from this repository. See [verification evidence](../../PUBLICATION.md#verification-evidence).

## Conclusion

Keep the existing stable 512 MiB/default-GC profile unchanged on this evidence. The 256 MiB profiles do not show a reliably lower final pre-shutdown HWM than the control; their ranges overlap substantially. The 128 MiB/SerialGC profile passed both pinned runs with lower observed HWM, but should remain an experimental option requiring representative-project and sustained-session validation. A small heap limit is not a limit on total JVM resident memory.

## Method

- Existing java_smoke executable, copied once and never rebuilt for the reported comparison
- SHA-256: 1cc18cebe11170a209c7327f3e8b4f462e6aaa492e257c68d142742fb3f92a40
- Executable: java_smoke-pinned
- Eclipse JDT LS 1.61.0; /usr/bin/java OpenJDK 21.0.12.1 (build 21.0.12.1+1-1-deb13u1-Debian), Linux x86_64
- The JVM default collector was verified to be G1. Each wrapper replaces exactly one -Xmx512m argument and execs the JVM, preserving PID accuracy for /proc measurements
- Five profiles, two sequential rounds; the second round reverses the profile order. No JVM profile runs overlap, but unrelated host work was not controlled, and OS/filesystem caches were not flushed
- Fresh temporary Eclipse project, server data directory and one synthetic Java source per run; Maven and Gradle import disabled
- Identical ten checks in every run: initialize, didOpen, semantic type-error diagnostics, nonempty hover, completion containing greeting, nonempty definition, didChange, type error cleared, didClose, shutdown
- All 10 reported runs passed with exit code 0. Corrected diagnostics retained one warning, so error cleared does not mean zero diagnostics
- Memory is the direct language-server JVM only, from /proc/PID/status. RSS is a snapshot and HWM is the high-water mark through that phase. No sample is taken after shutdown starts; this is not a measured whole-process-lifetime peak
- Reported wall seconds include the smoke process and its shutdown, not just server initialization
- The raw smoke metadata hard-codes jvm_max_heap_mib=512. For these wrapper runs, actual_cmdline in each summary JSON is authoritative; the raw logs are preserved unchanged
- Preliminary files without the fixed- prefix are excluded because their executable was not pinned; one rebuild occurred between those runs
- No frontend, remote agent, user project, or total-system memory is included. These numbers must not be added to a separately measured frontend RSS to claim a concurrent total

## Profile-level results

| JVM profile | Runs passed | Final sampled HWM (MiB), runs 1 / 2 | Wall seconds, runs 1 / 2 |
|---|---:|---:|---:|
| -Xmx512m (default G1) | 2/2 | 481.15 / 540.43 | 11.494 / 11.673 |
| -Xmx256m (default G1) | 2/2 | 550.08 / 475.14 | 9.624 / 11.299 |
| -Xmx256m -XX:+UseSerialGC | 2/2 | 535.94 / 467.30 | 12.322 / 9.887 |
| -Xmx128m -XX:+UseSerialGC | 2/2 | 395.43 / 376.46 | 10.352 / 11.508 |
| -Xms32m -Xmx256m -XX:+UseSerialGC | 2/2 | 501.15 / 484.78 | 10.108 / 9.672 |

## Raw per-stage measurements

RSS and HWM are MiB; elapsed is milliseconds from immediately before starting the language-server process.

| Profile / run | Phase | Elapsed ms | RSS MiB | HWM MiB |
|---|---|---:|---:|---:|
| heap512-default / 1 | after_initialize | 5157 | 233.660156 | 233.660156 |
| heap512-default / 1 | after_semantic_queries | 7838 | 414.070312 | 414.070312 |
| heap512-default / 1 | after_correction_before_shutdown | 8358 | 481.152344 | 481.152344 |
| heap512-default / 2 | after_initialize | 5180 | 225.695312 | 229.691406 |
| heap512-default / 2 | after_semantic_queries | 8025 | 501.882812 | 501.882812 |
| heap512-default / 2 | after_correction_before_shutdown | 8548 | 540.429688 | 540.429688 |
| heap256-default / 1 | after_initialize | 3529 | 234.394531 | 234.394531 |
| heap256-default / 1 | after_semantic_queries | 6031 | 481.933594 | 481.933594 |
| heap256-default / 1 | after_correction_before_shutdown | 6488 | 550.078125 | 550.078125 |
| heap256-default / 2 | after_initialize | 5183 | 235.191406 | 242.652344 |
| heap256-default / 2 | after_semantic_queries | 7682 | 417.574219 | 417.574219 |
| heap256-default / 2 | after_correction_before_shutdown | 8173 | 475.140625 | 475.140625 |
| heap256-serial / 1 | after_initialize | 3832 | 244.648438 | 249.882812 |
| heap256-serial / 1 | after_semantic_queries | 7064 | 440.175781 | 440.175781 |
| heap256-serial / 1 | after_correction_before_shutdown | 8638 | 535.941406 | 535.941406 |
| heap256-serial / 2 | after_initialize | 3760 | 245.511719 | 245.511719 |
| heap256-serial / 2 | after_semantic_queries | 6235 | 394.058594 | 394.171875 |
| heap256-serial / 2 | after_correction_before_shutdown | 6785 | 467.300781 | 467.300781 |
| heap128-serial / 1 | after_initialize | 3820 | 266.113281 | 266.113281 |
| heap128-serial / 1 | after_semantic_queries | 6522 | 389.523438 | 389.980469 |
| heap128-serial / 1 | after_correction_before_shutdown | 7004 | 394.406250 | 395.425781 |
| heap128-serial / 2 | after_initialize | 4048 | 251.335938 | 251.335938 |
| heap128-serial / 2 | after_semantic_queries | 7205 | 369.035156 | 369.195312 |
| heap128-serial / 2 | after_correction_before_shutdown | 8014 | 376.464844 | 376.464844 |
| heap256-serial-xms32 / 1 | after_initialize | 3819 | 222.937500 | 222.937500 |
| heap256-serial-xms32 / 1 | after_semantic_queries | 6432 | 438.046875 | 438.046875 |
| heap256-serial-xms32 / 1 | after_correction_before_shutdown | 6997 | 501.148438 | 501.148438 |
| heap256-serial-xms32 / 2 | after_initialize | 3526 | 218.183594 | 218.183594 |
| heap256-serial-xms32 / 2 | after_semantic_queries | 6001 | 417.542969 | 417.542969 |
| heap256-serial-xms32 / 2 | after_correction_before_shutdown | 6588 | 484.781250 | 484.781250 |

## Exact launch configuration

Profile-specific JVM options are shown above. Each raw per-run summary contains the entire observed /proc/PID/cmdline, including the unique temporary server-data directory. Common arguments were:

```text
-Declipse.application=org.eclipse.jdt.ls.core.id1
-Dosgi.bundles.defaultStartLevel=4
-Declipse.product=org.eclipse.jdt.ls.core.product
-Dlog.level=WARNING
--add-modules=ALL-SYSTEM
--add-opens java.base/java.util=ALL-UNNAMED
--add-opens java.base/java.lang=ALL-UNNAMED
-jar /path/to/language-tools/jdtls-1.61.0/plugins/org.eclipse.equinox.launcher_1.8.0.v20260804-1928.jar
-configuration /path/to/language-tools/jdtls-1.61.0/config_linux
-data <fresh-temporary-fixture>/jdt-data
```

## Scope and remaining validation

Two brief runs per profile establish repeatable completion of this specific synthetic workflow, not broad stability. This does not test large/multi-module projects, Maven or Gradle import, prolonged indexing, long editing sessions, multiple open projects, sustained CPU consumption, pauses, or out-of-memory recovery. RSS variability in the 512 and 256 MiB profiles is too large for a strong optimization claim from these samples. No production/default configuration was changed.

## Evidence files

- fixed-profile-results.json: complete aggregate with actual command lines, all raw stage counters, selected semantic payloads and stderr
- fixed-<profile>-run<N>.jsonl: unchanged smoke output
- fixed-<profile>-run<N>.summary.json: exact JVM arguments, measured stages, wall time, binary hash and pass result
- fixed-<profile>-run<N>.stderr: stderr for each run
- java_smoke-pinned and java_smoke-pinned.sha256: pinned executable and digest
- <profile>.sh: exact Java launcher wrappers
- java-256-default-flags.txt and java-256-serial-flags.txt: verified JVM ergonomic/default flags
