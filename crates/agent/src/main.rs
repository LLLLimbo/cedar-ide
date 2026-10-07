use cedar_workspace::Workspace;
use std::io;
use std::path::PathBuf;

fn main() {
    // run owns Workspace and returns before exit, so normal serve errors
    // unwind task/language owners. Never call process::exit inside that scope.
    if let Err(message) = run() {
        eprintln!("cedar-agent: {message}");
        std::process::exit(1);
    }
}
fn run() -> Result<(), String> {
    let mut args = std::env::args_os().skip(1);
    let mut root: Option<PathBuf> = None;
    let mut allow_run = false;
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--root") if root.is_none() => {
                root = Some(PathBuf::from(
                    args.next().ok_or("--root requires a directory")?,
                ));
            }
            Some("--allow-run") if !allow_run => allow_run = true,
            Some("--help" | "-h") => {
                eprintln!("Usage: cedar-agent --root PATH [--allow-run]\n\nServes bounded JSON frames on stdin/stdout. --allow-run permits arbitrary\nprograms with this account's full permissions; this is not a sandbox.");
                return Ok(());
            }
            _ => {
                return Err(format!(
                    "unknown or duplicate argument: {}",
                    arg.to_string_lossy()
                ))
            }
        }
    }
    let root = root.ok_or("--root PATH is required (use --help for usage)")?;
    let mut workspace = Workspace::open(root).map_err(|e| e.to_string())?;
    workspace.set_allow_run(allow_run);
    let stdin = io::stdin();
    let stdout = io::stdout();
    cedar_agent::serve(&mut workspace, &mut stdin.lock(), &mut stdout.lock())
        .map_err(|e| format!("protocol stream closed: {e}"))
}
