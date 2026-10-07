# Saved task profile fixture

This is a small, dependency-free Rust program and three explicit profiles.
Use a copy of this folder for destructive tests. A preinstalled Rust toolchain
must be on the workspace host's PATH; profiles do not install or download it.
`--offline` prevents Cargo registry access. The working directory is this folder.

1. Open the copied workspace, load `cedar.tasks.json` and inspect the profiles.
   Loading, editing, selecting and saving must not execute the program or create
   `sentinel.txt`
2. Explicitly trust the synthetic workspace only when ready to execute its code.
   Run **Literal arguments demo**. Its first output line is JSON containing cwd
   and exact argv. Empty strings, spaces, Chinese and shell-looking text must
   remain literal; `not-a-shell` must never be created
3. Run **Cancelable demo**. After its readiness line appears, Cancel and wait
   for the terminal Cancelled status. Editing/saving should remain available
4. **Explicit sentinel demo** intentionally creates `sentinel.txt`, only after
   Run is clicked. Repeating it fails instead of overwriting that sentinel
5. Edit/save/reload a profile, compare the raw JSON editor, and verify that
   conflicting raw-editor or external changes are preserved instead of silently
   overwritten

Profiles contain plaintext command arguments, not a secret store. Execution is
not sandboxed. Windows local execution is intentionally unavailable until its
process-tree cleanup implementation is ready; profiles can still be edited, and
a Windows frontend may run them through a separately configured Linux SSH host.
A successful stdio-agent fixture is not proof of authenticated SSH interoperability.
