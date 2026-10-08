# Verification report · checkpoint 9C collector portability / 0.8.8 · 2026-10-08

This checkpoint corrects the diagnostic collector's Windows filesystem handling.
The JVM crash remains unresolved; no language capability gate is changed.

## Exact native failure

Public 0.8.7 commit
[`877da5ec812eb3b9a40695aab61030935040b795`](https://github.com/LLLLimbo/cedar-ide/commit/877da5ec812eb3b9a40695aab61030935040b795)
passed Ubuntu. Its [Windows job](https://github.com/LLLLimbo/cedar-ide/actions/runs/37705346976/job/113078288940)
failed the collector tests with 17 failures and four errors before the Rust/native
process and Java stages. No six-case JVM matrix or real-Java artifact was
produced. Earlier native process/transport passes belong to their earlier exact
commits; they are not counted as execution in this failed run.

Python documents that Windows DirEntry.stat caches zero st_ino, st_dev and
st_nlink fields. The collector incorrectly interpreted ordinary files' zero
link count as an unsafe hardlink. Fresh no-follow lstat is required; accepting
zero link counts would remove a safety check and is not the correction.

A second failure compared GetFinalPathNameByHandleW's expanded pathname with the
runner's short 8.3 temporary-directory spelling. Canonical spelling must be
compared after rejecting reparse components, while actual file/root identity,
single-link, size and modification checks continue to guard the read. Literal
spelling equality is not Windows file identity. Neither issue explains the JVM
crash because the diagnostic processes never ran.

## Scope and verification

Only the diagnostic filesystem handling, its regression tests and checkpoint
metadata change. The raw-output privacy boundary remains: private generated
scratch is the only raw sink; public evidence contains reconstructed allowed
fields and typed statuses. No environment, register, memory or minidump data is
uploaded. Native tests report bounded Python/filesystem metadata without raw
runner paths, so the next exact CI can establish actual Windows behavior.

Local aggregate verification passed 555 Rust tests, all five Python protocol/
profile smoke chains, and two export tests. The sanitizer passed 39 tests; two
native Windows integration checks were skipped on Linux. MSVC all-target,
all-feature clippy and optimized all-feature workspace build passed. Reprocessing
the retained real Linux JDT transcript produced complete sanitized evidence.
Native Windows collector and JVM execution are still required. GUI Trust, authenticated
SSH and normal Windows language capability remain outside this checkpoint.

Sources: [DirEntry.stat Windows fields](https://docs.python.org/3/library/os.html#os.DirEntry.stat),
[realpath and Windows short names](https://docs.python.org/3/library/os.path.html#os.path.realpath).
