# Verification report · checkpoint 9D agent/editor fixture layout / 0.8.14 · 2026-10-08

This checkpoint adds an opt-in real Windows Java acceptance path through the
nonshipping agent host and the actual headless editor transaction/history code.
Normal Windows language capability remains disabled.

## Verified public baseline

Exact public 0.8.12 commit
[`a64d3ac505b51a02b34521e75bceadf01a38b355`](https://github.com/LLLLimbo/cedar-ide/commit/a64d3ac505b51a02b34521e75bceadf01a38b355)
[passed Ubuntu and Windows CI](https://github.com/LLLLimbo/cedar-ide/actions/runs/37714905533).
Windows passed 21 process cases, 16 transport cases, seven isolated-agent
task/bundle cases and three nonshipping agent-language cases.

Direct real JDT passed initial, fresh-data and reused-data sessions, including
source semantics, deferred import resolution and the JDK String symbol witness.
Independent retained root handles observed natural exit0 in all three sessions;
shutdown took 638, 565 and 1,037 ms. Source bytes remained unchanged and the
intended data-directory witnesses were present. This establishes the direct
transport path; it does not establish an editor, GUI or authenticated SSH path.

## 0.8.13 native result and fixture correction

Exact public [`930c212c4955d7391279bb65bc29fb76720759ed`](https://github.com/LLLLimbo/cedar-ide/commit/930c212c4955d7391279bb65bc29fb76720759ed)
[passed Ubuntu CI but failed Windows real-agent acceptance](https://github.com/LLLLimbo/cedar-ide/actions/runs/37718954907).
The direct Java probe again passed all three semantic sessions with natural
exit0. The agent initialized its first Java session in 4,606 ms, but did not
satisfy exact initial source diagnostics. No editor semantic success is claimed.
Cleanup independently observed Java exit0 after 984 ms, agent exit0, unchanged
source and generated-root removal. Cleanup operations completed, while the
overall acceptance receipt remained unsuccessful.

The generated agent fixture placed its Eclipse project above the JDT workspace
data directory. [Eclipse's project-location contract](https://help.eclipse.org/latest/rtopic/org.eclipse.platform.doc.isv/reference/api/org/eclipse/core/resources/IWorkspace.html#validateProjectLocationURI(org.eclipse.core.resources.IProject,java.net.URI))
forbids a project location that contains the platform working directory.
Version 0.8.14 puts the generated project and JDT data in sibling directories
under the same confined synthetic root, matching the passing direct layout.
Document paths change to project with spaces 雪/src/Main.java. Exact source, URI/range,
diagnostic, transaction and lifecycle assertions and deadlines are retained.
The next native run must confirm this correction; no runtime cause or successful
editor result is inferred solely from the static layout rule.

## New agent/editor acceptance

The validation-only constructor requires two exact synthetic-root markers and
explicit allow-run. It validates the selected distribution, relative launcher,
configuration and data locations while preserving the verified ordinary Java
executable spelling. Location checks are not a complete argv allowlist or an
OS sandbox. Normal constructors, shipping CLI and Hello capability gates retain
their existing behavior, including in all-feature builds.

The ignored native test drives three sequential real Java sessions through the
agent protocol: initial, fresh data and reused data. It checks exact source
diagnostics and definition, obtains and resolves a real completion, and invokes
the actual frontend apply path. It requires the primary edit and deferred import
to apply atomically without executing the advisory server command. Actual
Document undo/redo must restore text and cursor state, advance edit versions, and
synchronize versions 2, 3 and 4. Correction diagnostics and unchanged disk bytes
are checked separately. This test does not operate an OS window.

Each session records an independently observed live root identity before stop,
then requires the same retained handle to signal with exit0. The validation
profile performs the same bounded JDK symbol witness before shutdown. Successful
cleanup requires joined request workers and a reaped owned agent; forced cleanup
cannot satisfy graceful acceptance. Synthetic files are removed only after the
required ownership checks succeed. These root observations are not an
independent real-JVM Job-zero or listener-inventory measurement.

Raw transcripts remain in temporary private storage. Public evidence contains
only bounded typed session and cleanup fields through the existing sanitizer.
The PowerShell gate requires all three ordered sessions and one successful
cleanup receipt, so an empty or accidentally filtered cargo test run cannot pass.
The existing direct Java acceptance remains required.

## Verification boundary

Local focused tests cover strict semantic predicates, the real completion fixture
through Document history/version changes, and bounded typed lifecycle receipts.
Independent static review found no remaining implementation or public-evidence
boundary blocker. Exact native CI for this checkpoint remains pending; no
Windows agent/editor success is claimed before that run.

GUI Trust and authenticated SSH remain separate pending boundaries. No private
diagnostic findings are included in this report.
