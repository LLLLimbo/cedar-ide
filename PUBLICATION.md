# Public source checkpoint

This is the source-only public export of tested development stage `phase38-capability-groups-0.38.0`.
It is an independent Rust IDE project, not a complete IntelliJ IDEA replacement.
Public commits preserve the phase-by-phase development sequence, but their hashes
differ from the private build checkpoints because generated evidence is omitted.

## Verification evidence

Source, deterministic tests, test fixtures, reproducible verification scripts,
Cargo.lock, CI configuration, and upstream license notices are included.
The one complete JDT completion response needed by a deterministic compile-time
test remains as a data fixture, without unrelated startup logs or temporary URIs.
Raw screenshots, process logs and other JSON/JSONL measurement payloads from the cloud
test machine are deliberately omitted. References to their filenames in the
historical reports describe the original tests; they are not downloadable files
in this public repository. No new benchmark or platform guarantee is implied.
Run the documented test commands to produce fresh evidence on your own machine.

No prebuilt executables, JDK, Java/Kotlin language-server distributions, debug
adapters, credentials, private user projects or tool caches are included.
The 343 locked upstream dependencies keep their respective licenses; bundled
license texts may contain their upstream authors' public copyright information.
Java/Kotlin servers still need separate installation and any applicable license.
Native Windows/macOS GUI, authenticated SSH interoperability, full refactoring, integrated
debugging and IntelliJ plugin compatibility are not claimed by this checkpoint.
