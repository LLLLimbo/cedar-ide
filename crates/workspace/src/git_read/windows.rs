//! Git owns only its own job and pipe state, on the isolated agent handler.
//! No user task supervisor or language/JVM owner participates in this route.
use super::*;
use cedar_winprocess::{CaptureProgress, LaunchSpec, Stream, WindowsCommand};

pub(super) fn run(
    recipe: &Recipe,
    arguments: Vec<String>,
) -> Result<RawCommandResult, RemoteError> {
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
                    Ok(Some(_)) => break,
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
    if let Err(e) = owner.cancel_capture_and_complete() {
        failure.get_or_insert(e);
    }
    let exit = owner.wait_exit();
    // Drop finishes this job's ownership before any payload/error is published.
    drop(owner);
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
