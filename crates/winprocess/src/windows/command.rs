use super::pipes::{CapturePipes, PreparedStdio};
use super::process::ProcessOwner;
use crate::{CaptureProgress, LaunchSpec, ProcessExit, Stream};
use std::{fmt, io};

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
    pub fn spawn_suspended(spec: &LaunchSpec) -> io::Result<Self> {
        let PreparedStdio { child, capture } = PreparedStdio::new()?;
        // The process owner borrows stdio only for creation. On failure, child
        // writers close before capture drops and completes any pending I/O.
        let process = match ProcessOwner::create_suspended(spec, &child) {
            Ok(process) => process,
            Err(error) => {
                // Destructured locals otherwise drop in reverse binding order.
                // Close our writers explicitly before capture failure cleanup.
                drop(child);
                drop(capture);
                return Err(error);
            }
        };
        drop(child);
        Ok(Self { process, capture })
    }

    pub fn resume(&mut self) -> io::Result<()> {
        self.process.resume()
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
        let _ = self.capture.cancel_and_complete_pending();
        let _ = self.process.wait_exit();
        // Exceptional stuck kernel operations can delay cleanup; freeing memory
        // that is still referenced by I/O is never a deadline workaround.
    }
}
