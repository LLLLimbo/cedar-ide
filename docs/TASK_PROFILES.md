# Saved command profiles

Cedar can explicitly load, edit, and save `cedar.tasks.json` in the connected workspace root. Local and SSH workspaces use the same ordinary editor/read/revision-checked save path. No configuration operation executes a command.

## Version 1

```json
{"version":1,"profiles":[{"name":"Build","program":"cargo","args":["build"],"timeout_secs":300}]}
```

All fields are required. Unknown, duplicate, or missing object keys, duplicate profile names, unsupported versions, comments, trailing commas, and positional arrays are rejected. Limits:

- 256 KiB UTF-8 encoded file, including whitespace on input and JSON escaping, indentation, and the final newline on output
- 32 profiles; unique, nonempty, trimmed, control-free names up to 128 UTF-8 bytes
- Nonempty executable up to 4,096 UTF-8 bytes; no NUL
- Up to 256 literal arguments, including empty strings; at most 64 KiB combined argument text; no NUL
- Timeout from 1 through 300 seconds

Executable and argument strings are preserved exactly. Whitespace, empty arguments, Unicode, quotes, dollar signs, pipes, and other shell-looking text are not split, trimmed, expanded, or substituted. An executable named `" cargo "` is passed with its surrounding spaces and ordinarily fails to resolve; it is not silently changed to `cargo`.

There is no environment, working-directory override, credential, trust, dependency, autorun, shell-string, or variable-substitution field. The working directory is the connected workspace root. Explicitly choosing a shell as the executable can still run a shell; Cedar never inserts one implicitly. The existing task supervisor's timeout/output/process-tree/resource policies remain authoritative.

Project profiles are plaintext and may be committed to source control. Do not put secrets in arguments. Builds and tests can execute repository code, invoke tools, and download dependencies with the workspace account's permissions.

## Use

1. Open **COMMANDS**. A **Manual command** is available without creating or loading a configuration file
2. Choose **Load** to read `cedar.tasks.json`. If its tab is already open, Load parses that exact editor buffer, including unsaved edits; it never silently replaces that buffer from disk
3. Select a profile, or choose **New profile** after Load. Enter its name and executable, edit ordered literal argument rows, and set the timeout. **Add argument** adds an empty argument; **Remove** removes exactly that row
4. Review the escaped read-only executable/argv preview, workspace host/root, and saved state
5. Choose **Save profile** to commit the whole validated configuration as one editor undo transaction and request its ordinary revision-checked save. Saved JSON uses two-space indentation and one trailing newline for review and Git diffs. **Discard form changes** discards only structured form edits, leaving the raw editor and disk unchanged
6. Choose **Run** explicitly to execute the reviewed command. **Cancel command** requests cancellation and waits for terminal status. Run never saves a profile automatically

Load, selection, New profile, Save, Discard, recovery, and reconnection never run a command. Repeated Run clicks while a start/task is pending do not create another task. No presets or project-discovery commands are implemented in this slice.

Trust is an independent connection setting, never stored in the file. Trust-off connections can load and save profiles. Local Windows execution remains disabled pending verified Job Object/cancellable-pipe support; Windows frontends can still send commands to supported Linux SSH backends. Agent-side trust and platform checks remain authoritative.

## Drafts, conflicts, and reconnects

The ordinary configuration `Document` owns text, its SHA revision, undo history, save acknowledgement, and private recovery. The structured form captures the workspace identity, connection generation, document ID, edit version, and revision from which it was loaded.

- If the raw editor changes after the form was loaded, Run and Save profile stop. Both drafts remain available. Copy any form work you need, choose Discard form changes, and Load the editor buffer to reconcile them
- Save does not switch the active tab. UI actions run after that frame's editor input, so same-frame typing/paste is checked before the form can replace the configuration
- New typing during a save survives the acknowledgement. The submitted text becomes the saved baseline and newer text stays dirty
- A compact input near 256 KiB can still be loaded even when indentation would make its output too large. Such a Save is rejected before changing the raw document, its undo history/revision, or the form draft
- Backend revision conflicts, disappeared files, or connection loss never trigger an automatic retry. The submitted document stays available and recoverable
- Only a genuine `not_found` read creates an unsaved configuration tab. Its save uses `expected_revision: null` to prevent create-over-existing races. Existing files use the captured revision
- Invalid or oversized configuration text remains inspectable in its ordinary tab within the backend's general 1 MiB readable-file limit. It is never replaced with defaults. Files the backend cannot read yield an error and no creation draft
- Unsaved form edits participate in tab close, window close, and workspace-switch guards before serialization. A confirmed discard explicitly permits losing those form edits
- Reconnecting to the same canonical workspace retains the draft but requires **Review retained draft for this connection** or explicit Load before profile Run/Save. Review keeps the original SHA; a subsequent Save still checks for external changes
- Different host, port, root, or agent identities never inherit the previous workspace's profiles. Dirty work blocks switching

To read a newer disk version without disturbing a draft, use the ordinary editor's disk inspection. To replace an open buffer with the latest disk version, first save/copy the needed work, explicitly close its tab, then Load again.

Uncommitted structured form edits are session-only until Save profile serializes them into the ordinary document. Configuration recovery follows the existing private plaintext recovery policy; it restores original revision information with execution trust off. Trust, task IDs, task output, and instructions to resume are never recovered from profiles.

Revision checks preserve Cedar's existing limitation: the final SHA check and filesystem replacement are not an atomic compare-and-swap against arbitrary external writers. See the workspace save documentation for platform behavior.

## Verification

The app includes pure schema/escaping/boundary tests; zero-execution load/select/save tests; raw/form conflicts; delayed load/save and current typing; stale generations/closed documents; reconnect identity/review; trust-off recovery; exact argv dispatch; one-task repeated Run; local Windows versus SSH frontend gating; one-transaction undo; full egui-frame Text/Paste races; and minimum/default-height layouts with 256 argument rows.

```sh
cargo test -p cedar-app
cargo clippy -p cedar-app --all-targets -- -D warnings
```

Synthetic SSH dispatch and platform-gate tests do not prove a real Windows-to-Linux SSH connection. Native window checks, real Windows CI, and actual agent/SSH fixture execution are separate validation steps.
