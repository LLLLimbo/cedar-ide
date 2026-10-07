//! Nonshipping acceptance host. Never add this opt-in to cedar-agent's CLI.
use cedar_workspace::Workspace;
use std::{io, path::PathBuf};

fn main() {
    // Workspace drops before an error exits the process, exactly as in the real
    // agent. Forced-owner-death tests intentionally bypass this normal cleanup.
    if let Err(message) = run() {
        eprintln!("cedar-agent-language-validation: {message}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args_os().skip(1);
    let mut root = None;
    let mut allow_run = false;
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--synthetic-root") if root.is_none() => {
                root = Some(PathBuf::from(
                    args.next().ok_or("--synthetic-root requires a directory")?,
                ));
            }
            Some("--allow-run") if !allow_run => allow_run = true,
            _ => {
                return Err(format!(
                    "unknown or duplicate argument: {}",
                    arg.to_string_lossy()
                ))
            }
        }
    }
    let root = root.ok_or("--synthetic-root PATH is required")?;
    let mut workspace =
        Workspace::for_windows_language_validation(root).map_err(|error| error.to_string())?;
    workspace.set_allow_run(allow_run);
    let stdin = io::stdin();
    let stdout = io::stdout();
    cedar_agent::serve(&mut workspace, &mut stdin.lock(), &mut stdout.lock())
        .map_err(|error| format!("protocol stream closed: {error}"))
}
