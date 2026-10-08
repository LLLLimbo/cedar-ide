# Verification report · checkpoint 9C Java executable / 0.8.9 · 2026-10-08

This checkpoint applies an evidence-backed Java launch-profile repair and a
separate Linux broken-pipe fault-injection correction. It does not enable normal
Windows language capability or claim real Java acceptance before native CI.

## Actual 0.8.8 evidence

Exact public commit
[`024a73c3e31a5501d8193491dcc4712e1e7613af`](https://github.com/LLLLimbo/cedar-ide/commit/024a73c3e31a5501d8193491dcc4712e1e7613af)
[ran the bounded diagnostic matrix](https://github.com/LLLLimbo/cedar-ide/actions/runs/37706658606).
Windows Python 3.12.10 passed 40 collector tests with one platform skip. Its
metadata confirmed zero cached DirEntry identity/link fields, real lstat/fstat
identity equality and link count one, and different short/long spellings with
successful canonical handle equivalence and collection.

All three ordinary Java executable cases exited zero: version in ASCII cwd,
version in Unicode cwd, and the Unicode-cwd stdio source probe with all five
markers and all 14 binary input bytes. All three corresponding verbatim-canonical
executable cases exited one with JVM Internal Error. These and the actual JDT
crash shared the same 16 native-frame sequence. Every diagnostic case independently
observed root join, Job zero and completed I/O cleanup. Sanitized collection
completed with four fatal logs and no issues; raw material was not uploaded.

This isolates a launch-representation boundary in the selected JDK 21.0.12.1+1
runtime. It does not identify an exact upstream source line or establish a generic
Windows launcher defect. The real JDT session still failed before semantics. Windows passed all 21
process, 12 transport, seven isolated-agent task/bundle and two stdio cases;
the later three new agent-language cases were skipped after Java failed.

## Narrow Java recipe repair

The Java example now selects the same canonical executable using an ordinary
absolute local-drive spelling, checks recanonicalization equality, and explicitly
limits the JDK installation path to ASCII. Its Windows regression verifies an
ordinary disk prefix, canonical identity, convergence of ordinary/verbatim input
and rejection of Unicode JDK paths. Existing UNC/device negatives remain.

The physical JDT distribution, project and data directories retain Unicode and
spaces. Relative JAR and encoded Eclipse location URLs are unchanged. Generic
WindowsCommand still receives literal UTF-16 paths/argv; production agent cwd,
trust and capability gates are unchanged. No JDK relocation or locale change is
used to make this profile pass.

## Separate Linux lifecycle failure

The same 0.8.8 commit failed one of six explicitly invoked Unix stdio lifecycle
cases. The broken-pipe case timed out with the agent blocked reading stdin and
its task tree still alive. A controlled helper retaining the stdout read end
reproduced this state on the unchanged agent: the first Hello write legitimately
succeeded, and the later disappearance of that reader could not retroactively
produce a write error. A second Hello immediately produced BrokenPipe and owned
task cleanup. A transient inherited descriptor in concurrent CI is consistent
with that mechanism, but the original holder was not observed directly.

Only the broken-pipe setup now retains an owned stdout writer for passive
Linux POLLERR observation. It establishes reader absence before its one
fault-triggering Hello, sharing the original four-second deadline with agent
exit/stderr EOF. A deterministic retained-reader regression preserves the exact
error, task-inactive and leader-reap assertions. libc is a Linux-only test
dependency already present in the lockfile; ordinary stdio setup is unchanged. Production agent error handling and timeout values are unchanged.

## Verification boundary

Final local verification passed 556 Rust tests, including seven explicitly
invoked Unix stdio/lifecycle cases, plus all five Python smoke chains, two export
tests and 39 sanitizer tests (two Windows-only skips). All-feature MSVC clippy
and optimized workspace build passed. Independent review cleared both the
scoped Java representation change and passive broken-pipe fault injection. Ten
normal-parallel repetitions of the two focused broken-pipe tests passed all
20 executions without increasing deadlines; the full seven-case stdio suite
also passed independently against the rebuilt 0.8.9 debug agent. The next
exact Ubuntu/Windows run must pass both the corrected fault-injection test and
actual Java/agent gates. Prior native results do not make this commit green.
GUI Trust, authenticated SSH and full IDE Java/editor integration remain pending.
