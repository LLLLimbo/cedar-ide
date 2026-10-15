# Public source checkpoint

This is the source-only public export of tested development stage `phase77-0.77.0-project-context-r3-export-candidate`.
It is an independent Rust IDE project, not a complete IntelliJ IDEA replacement.
Public commits preserve the phase-by-phase development sequence, but their hashes
differ from the private build checkpoints because generated evidence is omitted.

## Verification evidence

Source, deterministic tests, test fixtures, reproducible verification scripts,
Cargo.lock, CI configuration, and upstream license notices are included.
The only permitted binary source assets are two exact, hash-pinned OFL font faces
(when present), accompanied by their readable manifest and complete license notices.
The one complete JDT completion response needed by a deterministic compile-time
test remains as a data fixture, without unrelated startup logs or temporary URIs.
Raw screenshots, process logs and other JSON/JSONL measurement payloads from the cloud
test machine are deliberately omitted. References to their filenames in the
historical reports describe the original tests; they are not downloadable files
in this public repository. No new benchmark or platform guarantee is implied.
Run the documented test commands to produce fresh evidence on your own machine.

No prebuilt executables, JDK, Java/Kotlin language-server distributions, debug
adapters, credentials, private user projects or tool caches are included.
The locked upstream dependencies keep their respective licenses; bundled
license texts may contain their upstream authors' public copyright information.
When present, the pinned egui 0.31.1 source, exact observation patch and complete
upstream/license provenance are retained byte-for-byte as one dependency.
Java/Kotlin servers still need separate installation and any applicable license.
Native Windows/macOS GUI, authenticated SSH interoperability, full refactoring, integrated
debugging and IntelliJ plugin compatibility are not claimed by this checkpoint.
