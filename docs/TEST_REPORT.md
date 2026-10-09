# Verification report · Linux agent development bundle / 0.35.0

The previous [0.34 checkpoint](TEST_REPORT_PHASE34_EXPLICIT_DISCONNECT.md) is fully
verified. This slice packages the normal default-feature Linux agent and verifies
the extracted program through local stdio. It adds no protocol capability, Java
platform port, SSH deployment or installation step.

## Required artifact and runtime contract

The build is pinned to Ubuntu 24.04 amd64. The archive contains only the normal
agent, Chinese quick-start, manifest and existing license material. ELF target,
interpreter, direct NEEDED libraries and required version symbols are measured
from the packaged bytes and compared with the manifest. This is not a universal
Linux, musl, ARM64 or old-glibc compatibility claim. Source document Git blob and
SHA-256 identities bind documentation/license bytes to the clean source commit; the CI mapping
records build provenance, not a cryptographic signature or reproducible-build proof.

Tar/gzip verification rejects unsupported paths, duplicate or hidden entries,
links, special files, unexpected modes, excessive expansion, malformed ABI data
and inconsistent manifest/source identities. Extraction requires a fresh directory
and rechecks exact file/directory inventory, contents and permissions afterward.

The required runtime uses the exact extracted agent with execution trust off in
a generated Unicode/space workspace. It checks metadata/capabilities/root, normal
file access, conditional save/readback, search, stale-save and root-escape refusal,
execution rejection, continued file access after errors, two consuming close
acknowledgements and explicit reconnect. It preserves unrelated generated files
and checks only the intended file changed. No user project is read or written.

## Verification status

Local verification passed 1,296 Rust tests across 42 suites (40 ignored), strict
all-target/all-feature Clippy on the host and Windows MSVC target, formatting,
all-feature release and default-feature shipping builds. The Linux archive suite
passed 30 tests, its receipt suite passed nine, and the retained Windows archive
suite passed 35 with one platform skip. An initial local link failure with the
build volume nearly full was preserved; after narrowly scoped removal of obsolete
generated harnesses, the complete verification pipeline passed.

Exact-source CI remains pending. The current cloud development host is Debian 13;
local parser/compile tests cannot establish Ubuntu package runtime acceptance. The actual Ubuntu build, extraction, stdio test and package hashes
must pass the pinned native CI job. Both platforms retain all existing required
gates, including the independently regenerated Windows package.

The Rust test's 30-second limit is a success ceiling checked around calls. Existing
Client RPC deadlines remain unchanged; a failing in-flight call can exceed that
ceiling before returning. Client-call counts include cached Hello and local
capability refusals and are not a count of wire frames. Only successful consuming
close acknowledgements establish local reaping. The CI step's enclosing failure
boundary is not cleanup evidence.

## Limits

No authenticated SSH, Windows-to-Linux connection, network-loss handling, remote
process cleanup or native GUI is exercised by this package test. Linux retains
its current generic-LSP feature set; the Windows-specific typed Java/Maven route
has not been ported. The archive is unsigned and does not bundle JDK, Git, language
servers, a graphical frontend or test drivers. See the
[Linux quick-start](LINUX_AGENT_QUICKSTART.zh-CN.md).
