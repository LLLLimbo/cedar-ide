# Cedar native desktop frontend

This is a native Rust application using `eframe`/`egui` 0.31.1 and the Glow renderer. It does not embed a browser or a JVM. The remote workspace agent and any optional language server run separately from the UI.

## Run

From the repository root:

```sh
cargo run -p cedar-app --bin cedar -- /path/to/workspace
```

The optional positional argument prefills the local workspace field. Select **Connect workspace** to open it. For SSH, enter the system OpenSSH destination, port, remote directory, and path to the installed `cedar-agent`. Authenticate and verify the remote host key in an ordinary terminal first. The app never silently accepts unknown SSH host keys.

The application supports:

- Local and SSH connection forms, explicit reconnect, generation-tagged background work, and draft retention after disconnection
- Directory navigation, new-file drafts, 32 editor tabs, and path-based quick open
- UTF-8 editing, line numbers, simple Java/Kotlin/Rust lexical colors, and case-sensitive in-file search
- Revision-checked saves that do not overwrite changes made outside the editor; a disk-version viewer for manual conflict resolution
- Explicit discard confirmation for dirty tabs and normal window-close requests
- Project text search and clickable line results
- Trusted-workspace Git status and bounded, explicit executable/JSON-argv commands
- A manual language-server panel with genuine stdio LSP initialization, open/change/close, hover, definition, completion, and event/diagnostic requests

Use Ctrl/Cmd+P to open a relative path, Ctrl/Cmd+F to find text, Ctrl/Cmd+S to save, and Ctrl/Cmd+W to close a tab. The explorer's **R** button refreshes its directory; **Up** navigates to its parent.

## Language-server workflow

1. Connect with trusted command execution enabled
2. Select **LSP** in the explorer and enter the installed server executable and its JSON argument array
3. Start the server, open an existing file, enter its LSP language ID (`rust`, `java`, or `kotlin`), and select **Sync current file**
4. Place the editor cursor and request Hover, Definition, or Completion
5. Resync after editing; query buttons intentionally stay disabled for an unsynced draft
6. Select **Refresh diagnostics / events** to collect current notifications

Positions are converted to zero-based UTF-16 columns. Results are displayed as read-only JSON, without automatically applying edits. Servers and any required JDK are installed and configured separately on the workspace host. They may index projects or execute repository code with that account's permissions.

## Deliberate limits

This is an independent prototype, not a complete replacement for a mature IDE. There is no debugger, interactive PTY, project model, inline completion menu, rename/refactor engine, persistent settings, autosave, or crash-recovery store. Unsaved buffers survive connection errors within the running process, not a crash or force-quit. Normal close is guarded; commands in progress must finish or time out before quitting.

Files opened by the backend are bounded to 1 MiB and tabs to 32. Pasted text is retained rather than silently truncated, but a draft over the 1 MiB limit must be shortened or copied before saving. In-file search keeps the first 10,000 matches, project search requests 500, and syntax coloring falls back to plain text above 256 KiB. No recursive file index or always-running polling loop is created by the UI.

Git, command execution, and language servers require explicit workspace trust. Local Windows process tools are currently rejected by the backend; a Windows frontend can still use these tools on a POSIX SSH workspace. Plain local file editing does not require command trust.

## Verification

```sh
cargo test -p cedar-app
cargo clippy -p cedar-app --all-targets -- -D warnings
```

Headless tests cover save acknowledgements after newer typing, conflicts and disconnection, stale generations, edits during workspace switches, delayed open requests at the tab limit, dirty tab protection, Unicode search, UTF-16 LSP positions, unsynced-language-query protection, and native UI frame layout at minimum and default window sizes. A headless frame test does not replace an OS-window interaction test.

Framework API reference: <https://docs.rs/eframe/0.31.1/eframe/>.
