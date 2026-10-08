use super::environment::EnvironmentBlock;
use super::pipes::{CapturePipes, PreparedStdio};
use super::process::ProcessOwner;
use super::stdin::{self, PendingWrite};
use crate::{
    CaptureProgress, LaunchSpec, ProcessExit, StdinCancelOutcome, StdinWriteProgress, Stream,
};
use std::{ffi::OsStr, fmt, io};

/// One owned, atomically job-assigned Windows process and its output captures.
///
/// Supported only in an isolated host with controlled process creation. The
/// exact inheritance list protects this child, not concurrent broad-inheritance
/// spawns elsewhere in its host. This type starts no worker/reader threads.
///
/// Callers must separately enforce execution trust, output retention limits,
/// deadlines and cancellation policy. Startup requires Windows 10/Server 2016+;
/// unsupported Job attributes fail closed without an unconfined fallback.
#[must_use = "Dropping an unresumed owner terminates its suspended child"]
pub struct WindowsCommand {
    process: ProcessOwner,
    capture: CapturePipes,
    stdin: Option<PendingWrite>,
}

impl fmt::Debug for WindowsCommand {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WindowsCommand")
            .finish_non_exhaustive()
    }
}

impl WindowsCommand {
    /// Create the child suspended and already in its owned kill-on-close job.
    /// No application code is intentionally resumed until `resume` succeeds.
    /// The executable and cwd must be existing absolute UTF-8 native paths;
    /// executables must have an explicit .exe extension. No shell/PATH lookup.
    /// Task stdin remains NUL; no parent write endpoint exists in this mode.
    /// The child inherits the calling process's environment.
    pub fn spawn_suspended(spec: &LaunchSpec) -> io::Result<Self> {
        Self::spawn_prepared(spec, PreparedStdio::new()?, None)
    }

    /// NUL-stdin variant with a complete, explicit child environment.
    /// No variables are implicitly inherited, added, expanded, or removed.
    /// The caller supplies any required system variables and owns policy.
    /// Owned or borrowed key/value pairs are copied without Unicode loss.
    ///
    /// Names must be nonempty and contain neither NUL nor '=' (including
    /// hidden drive entries such as '=C:'); values must be NUL-free. Names
    /// that compare equal under Windows case-insensitive ordinal comparison
    /// are rejected. The sorted, double-NUL UTF-16 block is bounded by
    /// [`crate::MAX_ENVIRONMENT_UTF16_UNITS`], including terminators.
    /// Validation precedes stdio/process creation. The parent's environment
    /// is never modified, and job-before-resume ownership is unchanged.
    pub fn spawn_suspended_with_environment<I, K, V>(
        spec: &LaunchSpec,
        environment: I,
    ) -> io::Result<Self>
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        let environment = EnvironmentBlock::new(environment)?;
        Self::spawn_prepared(spec, PreparedStdio::new()?, Some(&environment))
    }

    /// Explicit piped-stdin variant, with the same job-before-resume invariant.
    /// Adds one bounded overlapped writer; it does not enable any IDE service.
    /// The child inherits the calling process's environment.
    pub fn spawn_suspended_with_piped_stdin(spec: &LaunchSpec) -> io::Result<Self> {
        Self::spawn_prepared(spec, PreparedStdio::with_piped_stdin()?, None)
    }

    fn spawn_prepared(
        spec: &LaunchSpec,
        stdio: PreparedStdio,
        environment: Option<&EnvironmentBlock>,
    ) -> io::Result<Self> {
        let PreparedStdio {
            child,
            capture,
            stdin,
        } = stdio;
        // The process owner borrows stdio only for creation. On failure, child
        // writers close before capture drops and completes any pending I/O.
        let process = match ProcessOwner::create_suspended(spec, &child, environment) {
            Ok(process) => process,
            Err(error) => {
                // Destructured locals otherwise drop in reverse binding order.
                // Close our writers explicitly before capture failure cleanup.
                drop(child);
                drop(stdin);
                drop(capture);
                return Err(error);
            }
        };
        drop(child);
        Ok(Self {
            process,
            capture,
            stdin,
        })
    }

    /// Copy and submit at most MAX_STDIN_WRITE_BYTES to the owned write buffer.
    /// Never waits for the child to read. Account for Written(n) even when the
    /// operation completes synchronously; short counts leave a caller-owned
    /// suffix. Poll Pending before another begin (otherwise WouldBlock).
    /// A submission/completion error can follow partial transmission: framed
    /// protocols must abandon the connection, not retry the entire frame.
    pub fn begin_stdin_write(&mut self, bytes: &[u8]) -> io::Result<StdinWriteProgress> {
        self.stdin.as_mut().ok_or_else(stdin::closed)?.begin(bytes)
    }

    /// Nonblocking completion poll; reports each completed byte count once.
    /// Closed includes the constructor's NUL mode and stopped/closed pipes.
    pub fn poll_stdin_write(&mut self) -> io::Result<StdinWriteProgress> {
        self.stdin
            .as_mut()
            .map_or(Ok(StdinWriteProgress::Closed), PendingWrite::poll)
    }

    /// Close stdin for EOF only when no write completion is outstanding.
    /// WouldBlock leaves ownership untouched; finish polling or explicitly
    /// cancel. Idempotent, and never flushes/waits for child consumption.
    /// A completed write is transport acceptance, not application acknowledgment.
    pub fn close_stdin(&mut self) -> io::Result<()> {
        if self.stdin.as_ref().is_some_and(PendingWrite::is_pending) {
            return Err(stdin::busy());
        }
        drop(self.stdin.take());
        Ok(())
    }

    /// Cancel, establish completion, then close stdin. Cancellation/error can
    /// follow partial transmission; never replay a framed request on this pipe.
    /// This may wait for kernel cancellation completion, not child consumption.
    /// Repeated calls return Idle. It does not terminate the child or stop reads.
    pub fn cancel_stdin_and_complete(&mut self) -> io::Result<StdinCancelOutcome> {
        let Some(mut stdin) = self.stdin.take() else {
            return Ok(StdinCancelOutcome::Idle);
        };
        stdin.cancel_and_complete()
    }

    pub fn resume(&mut self) -> io::Result<()> {
        self.process.resume()
    }

    /// Original process ID returned by successful creation, for diagnostics.
    /// This is not a liveness check or authority for process lookup/termination:
    /// Windows may eventually reuse the number. Observe and clean up through
    /// this owner's process/job handles, including after the root exits.
    pub fn process_id(&self) -> u32 {
        self.process.process_id()
    }

    /// Observe only; the caller must clean up descendants after root exit.
    pub fn try_exit(&mut self) -> io::Result<Option<ProcessExit>> {
        self.process.try_exit()
    }

    /// Deliver at most four 8-KiB chunks per stream. The callback must return
    /// promptly; its borrowed bytes cannot outlive that invocation. Output
    /// retention belongs to the caller, not this fixed-buffer primitive.
    pub fn capture_round(
        &mut self,
        mut sink: impl FnMut(Stream, &[u8]),
    ) -> io::Result<CaptureProgress> {
        self.capture.capture_round(&mut sink)
    }

    /// Request termination of this job tree, using owned handles only.
    /// A successful request alone is not an observed terminal process state.
    pub fn terminate_tree(&mut self) -> io::Result<()> {
        self.process.terminate_tree()
    }

    /// Stop capture and establish completion before reusing/freeing I/O state.
    /// This does not itself terminate the process. Drain wanted final output
    /// within a finite caller budget before invoking this operation.
    pub fn cancel_capture_and_complete(&mut self) -> io::Result<()> {
        self.capture.cancel_and_complete_pending()
    }

    /// Wait for the owned root process. This is intentionally blocking: callers
    /// must drain output while it runs or request termination before waiting.
    /// Task supervisors must never call it on the frontend/UI thread.
    pub fn wait_exit(&mut self) -> io::Result<ProcessExit> {
        self.process.wait_exit()
    }

    /// Duplicate a non-inheritable, query/synchronize-only process handle.
    /// This stable observation handle grants no process-termination authority.
    pub fn observation_handle(&self) -> io::Result<std::os::windows::io::OwnedHandle> {
        self.process.observation_handle()
    }

    /// Kernel job accounting, for supervision and resource-ownership tests.
    pub fn active_processes(&self) -> io::Result<u32> {
        self.process.active_processes()
    }
}

impl Drop for WindowsCommand {
    fn drop(&mut self) {
        // Order is deliberate. Never await pipe EOF while a descendant can
        // retain a writer. Each component also owns its partial-failure cleanup.
        let _ = self.process.terminate_tree();
        let _ = self.cancel_stdin_and_complete();
        let _ = self.capture.cancel_and_complete_pending();
        let _ = self.process.wait_exit();
        // Exceptional stuck kernel operations can delay cleanup; freeing memory
        // that is still referenced by I/O is never a deadline workaround.
    }
}
