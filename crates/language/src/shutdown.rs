//! Bounded shutdown observations. No server text or diagnostic strings are retained.

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ShutdownOutcome {
    pub shutdown_response_received: bool,
    /// The complete exit frame and stdin closure were acknowledged by the writer.
    /// This does not assert that the server processed the notification.
    pub exit_frame_completed: bool,
    /// Only the Windows owned transport can report joined ownership. The legacy
    /// portable transport has detached I/O workers and always leaves this absent.
    pub windows: Option<WindowsShutdownOutcome>,
}

impl ShutdownOutcome {
    /// Protocol completion, a root exit of zero observed before owner termination,
    /// and joined cleanup with no recorded failures are all required.
    pub fn is_graceful(&self) -> bool {
        self.shutdown_response_received
            && self.exit_frame_completed
            && self.windows.is_some_and(|windows| {
                windows.reason == WindowsShutdownReason::RootExited
                    && !windows.transport_failure_observed
                    && windows.root_exit == WindowsRootExit::BeforeTermination(0)
                    && windows.cleanup == WindowsCleanupStatus::Joined
                    && windows.errors == WindowsCleanupErrors::default()
            })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowsShutdownOutcome {
    /// First material terminal cause; cleanup does not replace an earlier cause.
    pub reason: WindowsShutdownReason,
    /// A transport failure remains material even when root exit was the first
    /// terminal cause. In particular, malformed final output must not turn a
    /// completed protocol exchange and natural root exit into graceful success.
    pub transport_failure_observed: bool,
    pub root_exit: WindowsRootExit,
    pub cleanup: WindowsCleanupStatus,
    pub errors: WindowsCleanupErrors,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsShutdownReason {
    RootExited,
    GraceExpired,
    Aborted,
    TransportFailure,
    WorkerPanicked,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WindowsRootExit {
    /// Observed through the owned root handle before any owner termination call.
    BeforeTermination(u32),
    /// First observed after owner termination began. The code alone cannot show
    /// whether termination caused the exit (even a forced exit may have code 0).
    AfterTermination(u32),
    #[default]
    Unobserved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsCleanupStatus {
    /// The worker was joined after its process and I/O owners were destroyed,
    /// with no recorded cleanup-call errors. This is not an independent Job-zero
    /// observation or a claim that every destructor syscall succeeded.
    Joined,
    /// Ownership destruction and join completed, with recorded cleanup failures.
    JoinedWithErrors,
    /// The worker panicked or a completed ownership report is unavailable.
    Unverified,
}

/// At most one bit per failing operation category, regardless of repeat attempts.
/// These describe observed failures, not all syscalls performed by destructors.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WindowsCleanupErrors {
    pub observe_root: bool,
    pub terminate_tree: bool,
    pub cancel_stdin: bool,
    pub drain_output: bool,
    pub cancel_capture: bool,
    pub wait_root: bool,
    pub worker_panicked: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graceful_requires_protocol_and_observation_before_termination() {
        let mut outcome = ShutdownOutcome {
            shutdown_response_received: true,
            exit_frame_completed: true,
            windows: Some(WindowsShutdownOutcome {
                reason: WindowsShutdownReason::RootExited,
                transport_failure_observed: false,
                root_exit: WindowsRootExit::BeforeTermination(0),
                cleanup: WindowsCleanupStatus::Joined,
                errors: WindowsCleanupErrors::default(),
            }),
        };
        assert!(outcome.is_graceful());
        outcome.windows.as_mut().unwrap().root_exit = WindowsRootExit::AfterTermination(0);
        assert!(!outcome.is_graceful());
        outcome.windows.as_mut().unwrap().root_exit = WindowsRootExit::BeforeTermination(0);
        outcome.windows.as_mut().unwrap().errors.terminate_tree = true;
        assert!(!outcome.is_graceful());
        outcome.windows.as_mut().unwrap().errors = WindowsCleanupErrors::default();
        outcome.exit_frame_completed = false;
        assert!(!outcome.is_graceful());
        outcome.exit_frame_completed = true;
        outcome.windows = None;
        assert!(!outcome.is_graceful());
    }
}
