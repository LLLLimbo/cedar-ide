# Verification report · checkpoint 9C Unicode launch fix / 0.8.6 · 2026-10-07

> Public-source note: named raw logs, screenshots and measurement payloads are omitted from this repository. See [verification evidence](../PUBLICATION.md#verification-evidence).

This is a scoped direct-Java probe launch correction. Cedar's owned CreateProcessW
transport, UTF-16 literal argv, production agent cwd, trust and normal Windows
IDE capability gate are unchanged. No machine locale, shell wrapper, bootstrap
JAR or ASCII runtime relocation is introduced.

## Actual 0.8.5 result

Public commit
[`68157f38cdad25fb271a0e0c636033215b11e04e`](https://github.com/LLLLimbo/cedar-ide/commit/68157f38cdad25fb271a0e0c636033215b11e04e)
passed Ubuntu CI. Its [Windows job](https://github.com/LLLLimbo/cedar-ide/actions/runs/37696139684/job/113048222001)
passed the prior 21 process, 12 transport and seven isolated-agent/bundle suites.
The archive hash, ASCII extraction, Unicode move and environment correction
succeeded. Java then replaced the Unicode character in its absolute -jar path
with `?` and exited 1 before initialization. Source/cleanup assertions correctly
ran, and the retained root handle recorded the nonzero exit. The later three
agent-language cases were skipped and are not counted as passed.

## Confirmed upstream boundary and supported launch

[OpenJDK21u's launcher](https://github.com/openjdk/jdk21u/blob/master/src/java.base/share/native/launcher/main.c)
converts the Unicode command line through CP_ACP before parsing arguments.
Unrepresentable characters are lost there; Cedar already supplies UTF-16 to
CreateProcessW. Newer 25u/current upstream retains this conversion, so a version
switch alone is not a supported repair. A file.encoding flag or presumed UTF-8
argument file cannot recover already-replaced characters.

The direct probe now uses the verified Unicode distribution as child cwd. The
exact selected launcher is addressed by an ASCII relative plugins path, checked
against its canonical file. Configuration and data use percent-encoded file URLs,
which [Eclipse documents as supported locations](https://help.eclipse.org/latest/topic/org.eclipse.platform.doc.isv/reference/misc/runtime-options.html).
All physical install/project/data paths remain Unicode/spaces. The LSP root and
document URI still identify the original synthetic project.

Read-only inspection of the exact pinned launcher 1.8.0.v20260804-1928 and OSGi
3.24.300.v20260721-1251 bytecode confirmed URI-to-File conversion, UTF-8 CodeSource
installation decoding, and -data to osgi.instance.area mapping. Each initialized
session must find .metadata in the intended physical Unicode data directory.
The first two fresh directories establish creation there; the third reuses it
and relies on the restart semantics, not an independent current-session write
or lock observation. This prevents a literal escaped data path from passing the
fresh-directory checks. The executable
retains its native path; the cwd retains canonical-equivalence/UNC/device checks.
This does not claim arbitrary Unicode Java argv, Unicode JDK-home or long paths.

## Validation

- The exact launch recipe ran through real pinned JDT on Linux: all three
  initial/fresh-data/same-data sessions, lazy imports, source-byte invariants,
  expected metadata locations and cleanup passed in 25,986 ms. This is functional
  evidence for the recipe, not a Windows or performance result.
- Eleven Linux example unit tests cover exact launcher selection, path/URI,
  diagnostic, hover, definition, source invariants and graceful-exit predicates.
- Whole-workspace aggregate: 551 passing Rust executions, zero failed, two
  export regressions and five Python protocol chains passed.
- Strict whole-workspace MSVC cross-clippy and Linux release build passed.
- Independent primary-source review confirmed the upstream boundary and scoped
  recipe; a native Windows run must still establish its platform behavior.

Raw logs: `verification-phase9c-unicode-java-linux.txt`,
`verification-phase9c-unicode-aggregate.txt`,
`verification-phase9c-unicode-windows.txt`,
`verification-phase9c-unicode-release.txt`. Raw machine-specific evidence is
omitted from the public source export.

## Remaining gates

Run exact-commit Windows Java and the three nonshipping agent-language cases
alongside all prior suites. Production agent cwd remains the workspace, so this
direct probe's distribution-cwd recipe does not establish real JDT-through-agent
or editor transaction acceptance. Those need a separately scoped launch profile.
Normal Windows IDE LSP stays disabled. Native GUI, authenticated SSH and broader
process/listener evidence remain unperformed; no approval boundary was bypassed.
