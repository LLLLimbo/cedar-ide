//! Bounded shutdown observations. No server text or diagnostic strings are retained.

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ShutdownOutcome {
    pub shutdown_response_received: bool,
    /// The complete exit frame and stdin closure were acknowledged by the writer.
    /// This does not assert that the server processed the notification.
    pub exit_frame_completed: bool,
    /// Windows-specific Job/process and joined-I/O observation. Other hosts
    /// leave this absent; Linux has its separate ownership observation.
    pub windows: Option<WindowsShutdownOutcome>,
    /// Linux root/process-group and parent-I/O observations; never a Windows Job claim.
    pub linux: Option<LinuxShutdownOutcome>,
}

impl ShutdownOutcome {
    /// Protocol completion, a root exit of zero observed before owner termination,
    /// and joined cleanup with no recorded failures are all required.
    pub fn is_graceful(&self) -> bool {
        self.shutdown_response_received
            && self.exit_frame_completed
            && !(self.windows.is_some() && self.linux.is_some())
            && (self.windows.is_some_and(|windows| {
                windows.reason == WindowsShutdownReason::RootExited
                    && !windows.transport_failure_observed
                    && windows.root_exit == WindowsRootExit::BeforeTermination(0)
                    && windows.cleanup == WindowsCleanupStatus::Joined
                    && windows.errors == WindowsCleanupErrors::default()
            }) || self.linux.is_some_and(|linux| {
                linux.reason == LinuxShutdownReason::RootExited
                    && !linux.transport_failure_observed
                    && linux.root_exit == LinuxRootExit::BeforeTermination(LinuxExitStatus::Code(0))
                    && linux.cleanup == LinuxCleanupStatus::Joined
                    && linux.worker_joined
                    && linux.root_reaped
                    && linux.io_released
                    && linux.errors == LinuxCleanupErrors::default()
            }))
    }
}

/// Linux observation is cached after join or a fixed cleanup observation timeout.
/// Group signaling is not proof that escaped or credential-changed descendants exited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinuxShutdownOutcome {
    pub reason: LinuxShutdownReason,
    pub transport_failure_observed: bool,
    pub root_exit: LinuxRootExit,
    pub cleanup: LinuxCleanupStatus,
    /// Actual owner thread join completed; alone this does not prove cleanup.
    pub worker_joined: bool,
    /// The exclusive owner successfully consumed its original child wait status.
    pub root_reaped: bool,
    /// Cedar-owned stdin/stdout endpoints were destroyed; inherited stderr is excluded.
    pub io_released: bool,
    pub errors: LinuxCleanupErrors,
    /// Joined: owner completion minus first cleanup trigger. Timeout: observed
    /// elapsed at the durable timeout verdict. A later observer never renews it.
    pub cleanup_observation_elapsed_ms: u64,
    pub cleanup_observation_budget_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxShutdownReason {
    RootExited,
    GraceExpired,
    Aborted,
    TransportFailure,
    WorkerPanicked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxExitStatus {
    Code(i32),
    Signal(i32),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LinuxRootExit {
    BeforeTermination(LinuxExitStatus),
    AfterTermination(LinuxExitStatus),
    #[default]
    Unobserved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxCleanupStatus {
    /// Owner joined and no cleanup-call error recorded; inspect root and I/O flags.
    Joined,
    /// Owner joined but cleanup errors remain; this is not verified cleanup.
    JoinedWithErrors,
    /// Owner panic, lost identity, or cleanup observation timeout.
    Unverified,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LinuxCleanupErrors {
    pub observe_root: bool,
    pub terminate_group: bool,
    pub terminate_root: bool,
    pub drain_output: bool,
    pub wait_root: bool,
    pub worker_panicked: bool,
    pub ownership_lost: bool,
    pub cleanup_deadline_expired: bool,
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

    fn linux_graceful() -> ShutdownOutcome {
        ShutdownOutcome {
            shutdown_response_received: true,
            exit_frame_completed: true,
            windows: None,
            linux: Some(LinuxShutdownOutcome {
                reason: LinuxShutdownReason::RootExited,
                transport_failure_observed: false,
                root_exit: LinuxRootExit::BeforeTermination(LinuxExitStatus::Code(0)),
                cleanup: LinuxCleanupStatus::Joined,
                worker_joined: true,
                root_reaped: true,
                io_released: true,
                errors: LinuxCleanupErrors::default(),
                cleanup_observation_elapsed_ms: 1,
                cleanup_observation_budget_ms: 3000,
            }),
        }
    }

    #[test]
    fn linux_graceful_requires_protocol_reap_io_join_and_natural_zero() {
        let good = linux_graceful();
        assert!(good.is_graceful());
        for exit in [
            LinuxRootExit::Unobserved,
            LinuxRootExit::BeforeTermination(LinuxExitStatus::Code(1)),
            LinuxRootExit::BeforeTermination(LinuxExitStatus::Signal(9)),
            LinuxRootExit::AfterTermination(LinuxExitStatus::Code(0)),
            LinuxRootExit::AfterTermination(LinuxExitStatus::Signal(9)),
        ] {
            let mut value = good;
            value.linux.as_mut().unwrap().root_exit = exit;
            assert!(!value.is_graceful());
        }
        for mutate in [
            |v: &mut LinuxShutdownOutcome| v.worker_joined = false,
            |v: &mut LinuxShutdownOutcome| v.root_reaped = false,
            |v: &mut LinuxShutdownOutcome| v.io_released = false,
            |v: &mut LinuxShutdownOutcome| v.transport_failure_observed = true,
            |v: &mut LinuxShutdownOutcome| v.cleanup = LinuxCleanupStatus::Unverified,
            |v: &mut LinuxShutdownOutcome| v.cleanup = LinuxCleanupStatus::JoinedWithErrors,
            |v: &mut LinuxShutdownOutcome| v.reason = LinuxShutdownReason::GraceExpired,
            |v: &mut LinuxShutdownOutcome| v.errors.cleanup_deadline_expired = true,
            |v: &mut LinuxShutdownOutcome| v.errors.ownership_lost = true,
            |v: &mut LinuxShutdownOutcome| v.errors.terminate_group = true,
            |v: &mut LinuxShutdownOutcome| v.errors.drain_output = true,
            |v: &mut LinuxShutdownOutcome| v.errors.wait_root = true,
        ] {
            let mut value = good;
            mutate(value.linux.as_mut().unwrap());
            assert!(!value.is_graceful());
        }
        for protocol in [false, true] {
            let mut value = good;
            value.shutdown_response_received = protocol;
            value.exit_frame_completed = !protocol;
            assert!(!value.is_graceful());
        }
    }

    #[test]
    fn graceful_requires_protocol_and_observation_before_termination() {
        let mut outcome = ShutdownOutcome {
            shutdown_response_received: true,
            exit_frame_completed: true,
            linux: None,
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
