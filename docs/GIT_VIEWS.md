# Git changes and selected-file diff

Cedar provides an explicitly refreshed Git view through capability-gated
`git_changes` and `git_diff` requests. It reads the repository's disk/index state;
it does not stage files, commit, reset, fetch, apply patches or save editor buffers.
There is no background Git polling. Unsaved editor text is not part of the diff.

## Configuration and supported scope

Select the Git panel, enter the existing absolute Git executable path **on the
workspace host**, then explicitly refresh. For example, use `/usr/bin/git` on a
Linux host or `C:\Program Files\Git\cmd\git.exe` on Windows. Git is neither bundled
nor installed automatically. The selected tool must support Git 2.45 or later and
the actual `--no-lazy-fetch` option, introduced in the [Git 2.45 release](https://github.com/git/git/blob/v2.45.0/Documentation/RelNotes/2.45.0.txt). Windows requires a native `.exe` and the
normal isolated agent; there is no PATH, shell, batch-wrapper or in-process
Windows fallback. Hello advertises implemented operations without launching Git.

The opened workspace must itself be an ordinary repository root with a real
in-root `.git` directory. Ancestor discovery, bare repositories, `.git` files,
linked worktrees, shared metadata and object alternates are outside this slice.
Submodules are ignored. Rename detection is disabled, so renames appear as
deletion plus addition. Untracked files can be opened as files but have no diff
against an invented empty baseline. Conflicts, symlinks and submodules are
status-only. Select a supported tracked file to view staged or unstaged changes.
Binary changes use Git's ordinary summary; binary patches and forced text are
not requested. Unsupported encodings, malformed results and output limits are
reported as errors, never as a clean tree.

Older agents can retain the existing legacy status view. They do not receive new
requests unless they advertise the corresponding capabilities. A capability
claim does not grant execution trust.

## Execution trust and configuration policy

Git status and diff can execute repository-configured clean/process filters.
Both operations therefore require the existing explicit workspace execution
trust before even inspecting the configured executable or repository. Trusted
helpers still have the account's authority and may write files or use the network.
This is a changes viewer with no Git mutation commands, not a filesystem or
network sandbox around repository code.

The fixed recipe disables pager, external diff, textconv, fsmonitor, optional
index refresh, replacement objects and lazy fetching. It uses literal pathspecs
and explicit repository/worktree locations. Inherited `GIT_*` variables are
removed from each child environment, including routing, trace destinations and
Windows stream redirection. Fixed settings disable Git transports and prompts.
No process-global environment is modified.

System/global Git configuration and attributes are suppressed; repository
configuration and attributes remain. As a result, output can differ from your
usual CLI settings, including global `core.autocrlf` or filter definitions.
Cedar does not bypass Git's ownership checks with `safe.directory=*`.

## Request lifetime and display

Each explicit request has a fixed overall deadline and bounded raw capture.
Status paths are parsed from NUL-delimited output with exact identity, and display
labels are escaped separately. A diff request rechecks its eligible status entry
and validates the selected literal file path, including deleted parent paths.
Directories, traversal, `.git` paths and symlink/reparse escapes are rejected.
Concurrent external filesystem changes remain possible; a displayed result is
an observation, not an atomic repository snapshot or permission to overwrite.

The UI ties results to the connection, request, configured program, selected path
and diff kind. Changing these invalidates old replies. Git results never update a
document's saved baseline, Undo history or recovery data. Windows processes use
their own Job and completed pipe cleanup, independently of command-task and Java
owners. Unix retains the bounded process-group implementation and its documented
limits for deliberately escaping descendants; abrupt Unix agent death does not
guarantee child cleanup. Windows' kill-on-close ownership is a separate guarantee.
Kernel cleanup can still exceed a userspace deadline in exceptional cases.

The feature starts no resident Git service. Costs occur on explicit refresh/diff
and depend on repository size and configured helpers. No memory advantage over
another IDE is claimed. Native GUI and authenticated SSH acceptance are separate
from the generated-repository process tests for this feature.

References: [Git command/environment controls](https://git-scm.com/docs/git),
[porcelain status](https://git-scm.com/docs/git-status),
[diff options](https://git-scm.com/docs/git-diff),
[configuration](https://git-scm.com/docs/git-config), and
[repository layouts](https://git-scm.com/docs/gitrepository-layout).


## Native environment verification

Windows name filtering uses the OS ordinal comparison without assuming a
particular Unicode alias table. CI separately checks that comparison against
raw native environment lookup in synthetic children, then exercises actual Git
with both ASCII and Unicode-candidate redirect/trace environments. Disagreement
or unsafe Git behavior blocks acceptance. No raw host environment is exported.


The 0.15.1 Windows runner observed dotless-i candidates as distinct through both
APIs; ASCII/mixed-case and Greek-case controls matched. The earlier unconditional
alias assertion was corrected without changing production filtering. Actual Git
acceptance remains separately required; that run's Git driver was blocked by a
multiple-application discovery argument error, corrected in 0.15.2.

The 0.15.2 Git step was skipped after a recurring Java correction-notification
timeout. Version 0.15.3 collects independent Windows Git evidence after its own
normal-build prerequisite even if Java fails; the overall job remains failed and
no bundle is delivered until every required gate passes.

The 0.15.3 Windows Git probe executed but failed in the missing-promisor fixture. Its generated loose object was read-only; 0.15.4 adjusts only that verified
synthetic object's writable bit before deletion and retains all no-fetch checks.
Actual Windows Git acceptance remains pending the corrected native run.

The corrected 0.15.4 source subsequently passed [exact Ubuntu/Windows CI](https://github.com/LLLLimbo/cedar-ide/actions/runs/37787319950): 1,421 Windows and 1,283 Linux real-Git assertions, including missing-object refusal, unchanged repositories, hostile inherited environments and owned cleanup. The Windows-only forced-agent-death case passed. The rebuilt development bundle passed its separate Unicode extraction and trust-off file-operation probe.
