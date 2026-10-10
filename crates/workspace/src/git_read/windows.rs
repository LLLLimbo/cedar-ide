//! Git owns only its own job and pipe state, on the isolated agent handler.
//! No user task supervisor or language/JVM owner participates in this route.
use super::*;
use cedar_winprocess::{CaptureProgress, LaunchSpec, Stream, WindowsCommand};

pub(super) fn run(
    recipe: &Recipe,
    arguments: Vec<String>,
) -> Result<RawCommandResult, RemoteError> {
    #[cfg(test)]
    let command = DiagnosticCommand::classify(&arguments);
    #[cfg(test)]
    let mut root_exit_observed = false;
    let mut owner = WindowsCommand::spawn_suspended_with_environment(
        &LaunchSpec {
            executable: recipe.executable.clone(),
            arguments,
            cwd: recipe.root.clone(),
        },
        recipe.environment.iter().map(|(name, value)| (name, value)),
    )
    .map_err(|e| error("command_failed", e.to_string()))?;
    let mut capture = Capture::default();
    let mut timed_out = Instant::now() >= recipe.deadline;
    let mut failure = None;
    if !timed_out {
        if let Err(e) = owner.resume() {
            failure = Some(e);
        } else {
            loop {
                if let Err(e) = capture.round(&mut owner) {
                    failure = Some(e);
                    break;
                }
                if capture.truncated {
                    break;
                }
                match owner.try_exit() {
                    Ok(Some(_)) => {
                        #[cfg(test)]
                        {
                            root_exit_observed = true;
                        }
                        break;
                    }
                    Ok(None) => {}
                    Err(e) => {
                        failure = Some(e);
                        break;
                    }
                }
                if Instant::now() >= recipe.deadline {
                    timed_out = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
    // Root completion is not tree completion. Stop descendants before draining;
    // pending capture operations are always cancelled and joined before waiting.
    if let Err(e) = owner.terminate_tree() {
        failure.get_or_insert(e);
    }
    let drain_deadline = Instant::now() + Duration::from_millis(250);
    while !capture.done() && Instant::now() < drain_deadline {
        if let Err(e) = capture.round(&mut owner) {
            failure.get_or_insert(e);
            break;
        }
        if !capture.done() {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    let truncated = capture.truncated || !capture.done();
    // Preserve the last observed capture state before cancellation can complete
    // pending reads. Root exit means the existing poll before drain observed it,
    // not that later termination/wait cleanup established it.
    #[cfg(test)]
    let diagnostic = CaptureDiagnostic {
        command,
        stdout_bytes: capture.stdout.len(),
        stderr_bytes: capture.stderr.len(),
        cap_hit: capture.truncated,
        stdout_eof: capture.progress.stdout_eof,
        stderr_eof: capture.progress.stderr_eof,
        drain_expired: !capture.done() && Instant::now() >= drain_deadline,
        root_exit_observed,
    };
    if let Err(e) = owner.cancel_capture_and_complete() {
        failure.get_or_insert(e);
    }
    let exit = owner.wait_exit();
    // Drop finishes this job's ownership before any payload/error is published.
    drop(owner);
    #[cfg(test)]
    if let Some(record) = diagnostic.failure_record(failure.is_some() || exit.is_err()) {
        eprintln!("cedar_git_capture_failure {record}");
    }
    let exit = exit.map_err(io_error)?;
    if let Some(failure) = failure {
        return Err(io_error(failure));
    }
    Ok(RawCommandResult {
        stdout: capture.stdout,
        stderr: capture.stderr,
        exit_code: i32::try_from(exit.code).ok(),
        timed_out,
        truncated,
    })
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DiagnosticCommand {
    Version,
    Nonbare,
    Status,
    Diff,
    Other,
}

#[cfg(test)]
impl DiagnosticCommand {
    fn classify(arguments: &[String]) -> Self {
        let mut arguments = arguments.iter().map(String::as_str);
        while let Some(argument) = arguments.next() {
            match argument {
                "--no-pager"
                | "--no-optional-locks"
                | "--no-lazy-fetch"
                | "--no-replace-objects"
                | "--literal-pathspecs" => {}
                value if value.starts_with("--git-dir=") || value.starts_with("--work-tree=") => {}
                "-c" => {
                    if arguments.next().is_none() {
                        return Self::Other;
                    }
                }
                "--version" if arguments.next().is_none() => return Self::Version,
                "config"
                    if arguments
                        .by_ref()
                        .eq(["--local", "--type=bool", "--get", "core.bare"]) =>
                {
                    return Self::Nonbare;
                }
                "status" => return Self::Status,
                "diff" => return Self::Diff,
                _ => return Self::Other,
            }
        }
        Self::Other
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Version => "version",
            Self::Nonbare => "nonbare",
            Self::Status => "status",
            Self::Diff => "diff",
            Self::Other => "other",
        }
    }
}

#[cfg(test)]
struct CaptureDiagnostic {
    command: DiagnosticCommand,
    stdout_bytes: usize,
    stderr_bytes: usize,
    cap_hit: bool,
    stdout_eof: bool,
    stderr_eof: bool,
    drain_expired: bool,
    root_exit_observed: bool,
}

#[cfg(test)]
impl CaptureDiagnostic {
    fn failure_record(&self, owner_error: bool) -> Option<serde_json::Value> {
        let incomplete = !self.stdout_eof || !self.stderr_eof;
        let failure = match (owner_error, self.cap_hit, incomplete) {
            (true, _, _) => "owner_error",
            (false, true, true) => "both",
            (false, true, false) => "cap",
            (false, false, true) => "incomplete",
            (false, false, false) => return None,
        };
        // Fixed keys and scalar values only: no arguments, paths, output, or
        // error messages enter this test-only record.
        Some(serde_json::json!({
            "command": self.command.as_str(),
            "failure": failure,
            "stdout_bytes": self.stdout_bytes.min(MAX_COMMAND_OUTPUT_BYTES),
            "stderr_bytes": self.stderr_bytes.min(MAX_COMMAND_OUTPUT_BYTES),
            "cap_hit": self.cap_hit,
            "stdout_eof": self.stdout_eof,
            "stderr_eof": self.stderr_eof,
            "drain_expired": self.drain_expired,
            "root_exit_observed": self.root_exit_observed,
        }))
    }
}

#[derive(Default)]
struct Capture {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    progress: CaptureProgress,
    truncated: bool,
}
impl Capture {
    fn done(&self) -> bool {
        self.progress.stdout_eof && self.progress.stderr_eof
    }
    fn round(&mut self, owner: &mut WindowsCommand) -> io::Result<()> {
        self.progress = owner.capture_round(|stream, bytes| {
            let target = match stream {
                Stream::Stdout => &mut self.stdout,
                Stream::Stderr => &mut self.stderr,
            };
            let available = MAX_COMMAND_OUTPUT_BYTES - target.len();
            target.extend_from_slice(&bytes[..available.min(bytes.len())]);
            if bytes.len() > available {
                self.truncated = true;
            }
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod diagnostic_tests {
    use super::*;

    #[test]
    fn command_classification_skips_recipe_values_and_never_classifies_a_path() {
        let recipe = Recipe {
            executable: "private-executable".into(),
            root: "private-root/status".into(),
            git_dir: "private-root/diff".into(),
            environment: Vec::new(),
            deadline: Instant::now(),
        };
        for (command, expected) in [
            (vec!["--version"], DiagnosticCommand::Version),
            (
                vec!["config", "--local", "--type=bool", "--get", "core.bare"],
                DiagnosticCommand::Nonbare,
            ),
            (vec!["status", "--", "diff"], DiagnosticCommand::Status),
            (vec!["diff", "--", "--version"], DiagnosticCommand::Diff),
            (vec!["unknown", "status"], DiagnosticCommand::Other),
            (vec!["--version", "private-path"], DiagnosticCommand::Other),
            (
                vec!["config", "--get", "private-setting"],
                DiagnosticCommand::Other,
            ),
            (vec!["--", "status"], DiagnosticCommand::Other),
            (vec![], DiagnosticCommand::Other),
        ] {
            let arguments = recipe.arguments(&command).unwrap();
            assert_eq!(DiagnosticCommand::classify(&arguments), expected);
        }
        for (arguments, expected) in [
            (vec!["-c", "status", "diff"], DiagnosticCommand::Diff),
            (vec!["-c", "--version"], DiagnosticCommand::Other),
            (vec!["-c"], DiagnosticCommand::Other),
            (vec!["--unknown", "status"], DiagnosticCommand::Other),
        ] {
            let arguments = arguments.into_iter().map(str::to_owned).collect::<Vec<_>>();
            assert_eq!(DiagnosticCommand::classify(&arguments), expected);
        }
    }

    #[test]
    fn capture_failure_classification_preserves_cap_eof_and_owner_error_distinctions() {
        for cap_hit in [false, true] {
            for stdout_eof in [false, true] {
                for stderr_eof in [false, true] {
                    for owner_error in [false, true] {
                        let diagnostic = CaptureDiagnostic {
                            command: DiagnosticCommand::Status,
                            stdout_bytes: MAX_COMMAND_OUTPUT_BYTES,
                            stderr_bytes: 0,
                            cap_hit,
                            stdout_eof,
                            stderr_eof,
                            // Missing EOF after a capture error need not mean
                            // that the drain deadline expired.
                            drain_expired: false,
                            root_exit_observed: false,
                        };
                        let record = diagnostic.failure_record(owner_error);
                        let expected = if owner_error {
                            Some("owner_error")
                        } else {
                            match (cap_hit, !stdout_eof || !stderr_eof) {
                                (true, true) => Some("both"),
                                (true, false) => Some("cap"),
                                (false, true) => Some("incomplete"),
                                (false, false) => None,
                            }
                        };
                        assert_eq!(
                            record
                                .as_ref()
                                .map(|value| value["failure"].as_str().unwrap()),
                            expected
                        );
                        if let Some(record) = record {
                            assert_eq!(record["cap_hit"], cap_hit);
                            assert_eq!(record["stdout_eof"], stdout_eof);
                            assert_eq!(record["stderr_eof"], stderr_eof);
                            assert_eq!(record["drain_expired"], false);
                            assert_eq!(record["root_exit_observed"], false);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn capture_failure_serialization_has_only_fixed_scalars_and_capped_byte_counts() {
        for bytes in [
            0,
            MAX_COMMAND_OUTPUT_BYTES - 1,
            MAX_COMMAND_OUTPUT_BYTES,
            usize::MAX,
        ] {
            let diagnostic = CaptureDiagnostic {
                command: DiagnosticCommand::Other,
                stdout_bytes: bytes,
                stderr_bytes: bytes,
                cap_hit: false,
                stdout_eof: true,
                stderr_eof: false,
                drain_expired: true,
                root_exit_observed: true,
            };
            let record = diagnostic.failure_record(false).unwrap();
            let serialized = serde_json::to_string(&record).unwrap();
            let decoded: serde_json::Value = serde_json::from_str(&serialized).unwrap();
            assert_eq!(
                decoded,
                serde_json::json!({
                    "command": "other",
                    "failure": "incomplete",
                    "stdout_bytes": bytes.min(MAX_COMMAND_OUTPUT_BYTES),
                    "stderr_bytes": bytes.min(MAX_COMMAND_OUTPUT_BYTES),
                    "cap_hit": false,
                    "stdout_eof": true,
                    "stderr_eof": false,
                    "drain_expired": true,
                    "root_exit_observed": true,
                })
            );
            assert!(decoded
                .as_object()
                .unwrap()
                .values()
                .all(|value| { value.is_string() || value.is_u64() || value.is_boolean() }));
        }
    }
}
