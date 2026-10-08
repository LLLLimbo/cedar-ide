//! Nonshipping fixed GC diagnostic host. Normal Client arguments and protocol.
//! Never add this opt-in to cedar-agent's CLI or use it as a shipping artifact.
use cedar_workspace::Workspace;
use std::{
    ffi::OsString,
    io::{self, BufRead, Write},
    path::PathBuf,
};

#[derive(Debug, PartialEq, Eq)]
struct Options {
    root: PathBuf,
    allow_run: bool,
}

fn main() {
    // Exactly like the ordinary isolated agent, return out of Workspace's scope
    // before process::exit so owned language processes join during cleanup.
    if let Err(message) = run() {
        eprintln!("cedar-agent-java-gc-diagnostic: {message}");
        std::process::exit(1);
    }
}

fn parse_args(args: impl IntoIterator<Item = OsString>) -> Result<Options, String> {
    let mut args = args.into_iter();
    let mut root = None;
    let mut allow_run = false;
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--root") if root.is_none() => {
                let value = args.next().ok_or("--root requires a directory")?;
                if value.is_empty() || value.to_string_lossy().starts_with("--") {
                    return Err("--root requires a directory, not another option".into());
                }
                root = Some(PathBuf::from(value));
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
    Ok(Options {
        root: root.ok_or("--root PATH is required")?,
        allow_run,
    })
}

fn run() -> Result<(), String> {
    let options = parse_args(std::env::args_os().skip(1))?;
    let stdin = io::stdin();
    let stdout = io::stdout();
    serve(options, &mut stdin.lock(), &mut stdout.lock())
}

fn serve<R: BufRead, W: Write>(
    options: Options,
    reader: &mut R,
    writer: &mut W,
) -> Result<(), String> {
    let mut workspace = Workspace::for_windows_java_gc_diagnostic(options.root)
        .map_err(|error| error.to_string())?;
    // An omitted trust flag must allow Hello, then return ordinary run_disabled
    // protocol errors. The constructor itself never grants execution trust.
    workspace.set_allow_run(options.allow_run);
    cedar_agent::serve(&mut workspace, reader, writer)
        .map_err(|error| format!("protocol stream closed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cedar_protocol::{read_frame, write_frame, Operation, Payload, Request, Response};
    use cedar_workspace::{
        WINDOWS_JAVA_GC_DIAGNOSTIC_MARKER, WINDOWS_JAVA_GC_DIAGNOSTIC_MARKER_CONTENTS,
    };

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn accepts_only_normal_client_root_and_optional_trust() {
        for values in [
            vec!["--root", "space 雪"],
            vec!["--root", "space 雪", "--allow-run"],
            vec!["--allow-run", "--root", "space 雪"],
        ] {
            assert_eq!(
                parse_args(args(&values)).unwrap(),
                Options {
                    root: "space 雪".into(),
                    allow_run: values.contains(&"--allow-run"),
                }
            );
        }
        for values in [
            vec![],
            vec!["--root"],
            vec!["--root", ""],
            vec!["--root", "--root"],
            vec!["--root", "--allow-run"],
            vec!["--root", "--unknown"],
            vec!["--allow-run"],
            vec!["--root", "path", "--root", "other"],
            vec!["--root", "path", "--allow-run", "--allow-run"],
            vec!["--synthetic-root", "path"],
            vec!["--root", "path", "--java-distribution", "distribution"],
            vec!["--root", "path", "--gc-log", "arbitrary"],
            vec!["--root", "path", "--jvm-args", "-Xmx1g"],
            vec!["--root", "path", "--help"],
            vec!["--root", "path", "unexpected"],
        ] {
            assert!(parse_args(args(&values)).is_err(), "{values:?}");
        }
    }

    #[test]
    fn untrusted_host_serves_hello_and_returns_run_disabled_without_launching() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join(WINDOWS_JAVA_GC_DIAGNOSTIC_MARKER),
            WINDOWS_JAVA_GC_DIAGNOSTIC_MARKER_CONTENTS,
        )
        .unwrap();
        let mut input = Vec::new();
        for (id, op) in [
            Operation::Hello,
            Operation::LanguageStartJava {
                java_executable: "must-not-run".into(),
                distribution: "must-not-inspect".into(),
                data_directory: "must-not-inspect".into(),
            },
            Operation::LanguageStart {
                program: "must-not-run".into(),
                args: vec![],
            },
            Operation::LanguageStop,
            Operation::Hello,
        ]
        .into_iter()
        .enumerate()
        {
            write_frame(&mut input, &Request { id: id as u64, op }).unwrap();
        }
        let mut output = Vec::new();
        serve(
            Options {
                root: root.path().into(),
                allow_run: false,
            },
            &mut input.as_slice(),
            &mut output,
        )
        .unwrap();
        let mut reader = output.as_slice();
        for id in 0..5 {
            let response: Response = read_frame(&mut reader).unwrap().unwrap();
            assert_eq!(response.id, id);
            if id == 0 || id == 4 {
                assert!(matches!(response.result.unwrap(), Payload::Hello { .. }));
            } else {
                assert_eq!(response.result.unwrap_err().code, "run_disabled");
            }
        }
        assert!(read_frame::<_, Response>(&mut reader).unwrap().is_none());
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
    }

    #[test]
    fn unmarked_root_cannot_serve_protocol() {
        let root = tempfile::tempdir().unwrap();
        let mut output = Vec::new();
        assert!(serve(
            Options {
                root: root.path().into(),
                allow_run: true
            },
            &mut [].as_slice(),
            &mut output
        )
        .is_err());
        assert!(output.is_empty());
    }
}
