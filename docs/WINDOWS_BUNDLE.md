# Windows development bundle

The versioned ZIP contains only the normal release `cedar.exe` / `cedar-agent.exe`
pair, Chinese quick-start, Java setup notes and licenses. It has no installer,
service, JDK, JDT LS, test host or diagnostic executable. It is unsigned. Program
files can be extracted into a chosen directory; recovery data still uses the
existing user-local location. This is not a claim of portable user-data storage.

The verified 0.14.0 executables import `VCRUNTIME140.dll` and UCRT API-set DLLs.
The package therefore requires a compatible Microsoft Visual C++ v14 x64 runtime
and Windows UCRT. Version 0.14.1 corrects the included quick-start to state this
prerequisite and link Microsoft's official installer. No runtime DLL is copied,
redistributed or installed by this workflow. CI's hosted machine is not evidence
that the ZIP runs on a clean machine without prerequisites.

`scripts/package_windows_bundle.py` creates an explicit payload inventory from a
clean, exact source checkout. It records the Cargo version, source commit, project
CI URL, and each payload's length and SHA256 in `BUNDLE_MANIFEST.json`. The manifest
does not hash itself. A separate build receipt records the complete ZIP's SHA256.
Hashes establish byte consistency, not an independent signature or a reproducible
compiler-build guarantee. ZIP timestamps and permissions are fixed; compressed
archive bytes are not promised identical across zlib versions.

The native workflow builds the two shipping binaries with default features after
the ordinary correctness suites. Only then does it package them. The packaging
step verifies the archive, extracts it into a new path containing Unicode and
spaces, and checks the extracted inventory. A private copy of the existing
nonshipping bundle probe is added beside the extracted agent solely for the test.
It uses the same `Client::connect(Local)` sibling discovery as the frontend, with
execution trust off, against an exact marked synthetic directory.

The probe checks metadata, listing, reading, conditional writing, readback,
search, stale-write refusal, command/Java trust rejection and owned-client reap.
The wrapper verifies the expected synthetic disk contents, removes the private
probe, and verifies payload hashes and the exact extracted inventory again. Only
the original ZIP and controlled JSON receipt are uploaded. Raw scratch contents
and test binaries are excluded. This establishes the bundle's headless file route;
it does not execute or attest native GUI interaction.

The artifact is named `cedar-windows-development`; its inner ZIP filename contains
the Cargo version and source SHA prefix. The receipt links the full source commit
and exact workflow run. Use a successful run at that exact commit. The existing
loose-binary artifact is retained for compatibility. Native GUI and authenticated
SSH acceptance remain separate open work; no installer, signing or security
settings are changed by this milestone.

The fixed GC experiment completed in 0.13.0, so routine CI no longer invokes its
optional control. Its fixture, strict unit tests and public sanitized evidence
remain available. Required Java semantics, editor transactions, ownership,
concurrency, interruption and normal-route checks still run. Package assembly adds
no background service. Previous headless Java resource figures are documented in
the included quick-start and are not advertised as a GUI or IDEA comparison.
