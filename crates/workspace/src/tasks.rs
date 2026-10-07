//! The stdio agent stays responsive while the owned task supervisor runs commands.
use super::{error, Workspace};
use cedar_protocol::{Operation, Payload, RemoteError};
use cedar_tasks::{TaskError, TaskManager, TaskSnapshot};
use std::time::Duration;

fn task_error(error: TaskError) -> RemoteError {
    RemoteError::new(error.code(), error.to_string())
}
fn payload(snapshot: TaskSnapshot) -> Result<Payload, RemoteError> {
    serde_json::to_value(snapshot)
        .map(|snapshot| Payload::RunTask { snapshot })
        .map_err(|e| error("task_serialization", e.to_string()))
}
impl Workspace {
    pub(super) fn handle_task(&mut self, op: Operation) -> Result<Payload, RemoteError> {
        if !self.allow_run {
            return Err(error(
                "run_disabled",
                "Command tasks require explicit workspace execution trust",
            ));
        }
        match op {
            Operation::RunStart {
                program,
                args,
                timeout_secs,
            } => {
                if self.tasks.is_none() {
                    self.tasks = Some(TaskManager::new(&self.root).map_err(task_error)?);
                }
                let manager = self.tasks.as_ref().expect("manager inserted above");
                let id = manager
                    .start(program, args, Duration::from_secs(timeout_secs))
                    .map_err(task_error)?;
                payload(manager.poll(id).map_err(task_error)?)
            }
            Operation::RunPoll { task_id } => payload(
                self.tasks
                    .as_ref()
                    .ok_or_else(|| {
                        error(
                            "unknown_task",
                            "No command task exists in this workspace session",
                        )
                    })?
                    .poll(task_id)
                    .map_err(task_error)?,
            ),
            Operation::RunCancel { task_id } => payload(
                self.tasks
                    .as_ref()
                    .ok_or_else(|| {
                        error(
                            "unknown_task",
                            "No command task exists in this workspace session",
                        )
                    })?
                    .cancel(task_id)
                    .map_err(task_error)?,
            ),
            _ => Err(error("invalid_operation", "Not a command-task operation")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn new_workspaces_do_not_start_task_supervisors_and_untrusted_requests_fail() {
        let root = tempfile::tempdir().unwrap();
        let mut ws = Workspace::open(root.path()).unwrap();
        assert!(ws.tasks.is_none());
        for op in [
            Operation::RunStart {
                program: "nonexistent".into(),
                args: vec![],
                timeout_secs: 1,
            },
            Operation::RunPoll { task_id: 1 },
            Operation::RunCancel { task_id: 1 },
        ] {
            assert_eq!(ws.handle(op).unwrap_err().code, "run_disabled");
            assert!(ws.tasks.is_none());
        }
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn filesystem_operations_remain_available_until_task_cancellation_finishes() {
        let root = tempfile::tempdir().unwrap();
        let mut ws = Workspace::open(root.path()).unwrap();
        ws.set_allow_run(true);
        let started = ws
            .handle(Operation::RunStart {
                program: "sh".into(),
                args: vec!["-c".into(), "printf live; sleep 10".into()],
                timeout_secs: 30,
            })
            .unwrap();
        let Payload::RunTask { snapshot } = started else {
            panic!("wrong payload")
        };
        let id = snapshot["id"].as_u64().unwrap();
        ws.handle(Operation::Write {
            path: "while-running.txt".into(),
            text: "editor stays available".into(),
            expected_revision: None,
        })
        .unwrap();
        assert!(
            matches!(ws.handle(Operation::Read{path:"while-running.txt".into()}).unwrap(),Payload::File{text,..} if text=="editor stays available")
        );
        ws.handle(Operation::RunCancel { task_id: id }).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            let Payload::RunTask { snapshot } =
                ws.handle(Operation::RunPoll { task_id: id }).unwrap()
            else {
                unreachable!()
            };
            if snapshot["state"] == "cancelled" {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "cancel did not finish: {snapshot}"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
