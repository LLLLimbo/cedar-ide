#![cfg(any(target_os = "linux", target_os = "macos"))]

use cedar_tasks::{
    TaskError, TaskId, TaskManager, TaskSnapshot, TaskState, MAX_ARGUMENTS, MAX_ARGUMENT_BYTES,
    MAX_COMPLETED_TASKS, MAX_OUTPUT_BYTES_PER_STREAM, MAX_PROGRAM_BYTES, MAX_TIMEOUT,
};
use std::fs;
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};
use tempfile::TempDir;

fn manager() -> (TempDir, TaskManager) {
    let dir = tempfile::tempdir().unwrap();
    let manager = TaskManager::new(dir.path()).unwrap();
    (dir, manager)
}

fn shell(manager: &TaskManager, script: &str, timeout: Duration) -> TaskId {
    manager
        .start("/bin/sh".into(), vec!["-c".into(), script.into()], timeout)
        .unwrap()
}

fn until(
    manager: &TaskManager,
    id: TaskId,
    condition: impl Fn(&TaskSnapshot) -> bool,
) -> TaskSnapshot {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let snapshot = manager.poll(id).unwrap();
        if condition(&snapshot) {
            return snapshot;
        }
        assert!(
            Instant::now() < deadline,
            "task did not reach condition: {snapshot:?}"
        );
        thread::sleep(Duration::from_millis(5));
    }
}

fn done(manager: &TaskManager, id: TaskId) -> TaskSnapshot {
    until(manager, id, |s| s.state.is_terminal())
}

#[test]
fn start_and_poll_are_prompt_and_partial_output_is_live() {
    let (_dir, manager) = manager();
    let started = Instant::now();
    let id = shell(
        &manager,
        "printf ready; sleep 2; printf done",
        Duration::from_secs(5),
    );
    assert!(started.elapsed() < Duration::from_millis(500));
    let started = Instant::now();
    let first = manager.poll(id).unwrap();
    assert!(!first.state.is_terminal());
    assert!(started.elapsed() < Duration::from_millis(500));
    let live = until(&manager, id, |s| s.stdout == "ready");
    assert!(!live.state.is_terminal());
    let started = Instant::now();
    manager.cancel(id).unwrap();
    assert!(started.elapsed() < Duration::from_millis(500));
    assert_eq!(done(&manager, id).state, TaskState::Cancelled);
}

#[test]
fn success_stderr_nonzero_exit_and_null_stdin_are_preserved() {
    let (_dir, manager) = manager();
    let id = shell(
        &manager,
        "printf hello; printf problem >&2; if read x; then exit 99; fi; exit 0",
        Duration::from_secs(2),
    );
    let snapshot = done(&manager, id);
    assert_eq!(snapshot.state, TaskState::Succeeded);
    assert_eq!(snapshot.stdout, "hello");
    assert_eq!(snapshot.stderr, "problem");
    assert_eq!(snapshot.exit_code, Some(0));
    assert!(!snapshot.truncated);
    assert!(snapshot.error.is_none());
    let id = shell(&manager, "exit 7", Duration::from_secs(2));
    let snapshot = done(&manager, id);
    assert_eq!(snapshot.state, TaskState::Failed);
    assert_eq!(snapshot.exit_code, Some(7));
    assert!(snapshot.error.is_none());
}

#[test]
fn executable_arguments_are_literal_and_working_directory_is_explicit() {
    let (dir, manager) = manager();
    let literal = "$(touch UNEXPECTED); spaces 'quotes' & | * 🦀";
    let id = manager
        .start(
            "/bin/echo".into(),
            vec![literal.into()],
            Duration::from_secs(2),
        )
        .unwrap();
    assert_eq!(done(&manager, id).stdout, format!("{literal}\n"));
    assert!(!dir.path().join("UNEXPECTED").exists());
    let id = manager
        .start("/bin/pwd".into(), vec![], Duration::from_secs(2))
        .unwrap();
    assert_eq!(
        done(&manager, id).stdout.trim(),
        dir.path().canonicalize().unwrap().to_str().unwrap()
    );
}

#[test]
fn timeout_and_explicit_cancel_remain_distinguishable() {
    let (_dir, manager) = manager();
    let id = shell(&manager, "exec sleep 10", Duration::from_millis(80));
    let started = Instant::now();
    assert_eq!(done(&manager, id).state, TaskState::TimedOut);
    assert!(started.elapsed() < Duration::from_secs(2));
    let id = shell(
        &manager,
        "printf started; exec sleep 10",
        Duration::from_secs(3),
    );
    until(&manager, id, |s| s.stdout == "started");
    let first = manager.cancel(id).unwrap();
    assert_eq!(first.state, TaskState::Cancelling);
    for _ in 0..100 {
        manager.cancel(id).unwrap();
    }
    assert_eq!(done(&manager, id).state, TaskState::Cancelled);
}

#[test]
fn binary_output_flood_is_capped_and_cannot_starve_control_operations() {
    let (_dir, manager) = manager();
    let id = shell(
        &manager,
        "while :; do printf '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef'; done",
        Duration::from_secs(5),
    );
    let snapshot = done(&manager, id);
    assert_eq!(snapshot.state, TaskState::OutputLimit);
    assert_eq!(snapshot.stdout.len(), MAX_OUTPUT_BYTES_PER_STREAM);
    assert!(snapshot.truncated);
    let id = shell(&manager, "printf usable", Duration::from_secs(2));
    assert_eq!(done(&manager, id).stdout, "usable");
}

#[test]
fn stderr_flood_is_bounded_independently_of_stdout() {
    let (_dir, manager) = manager();
    let id = shell(
        &manager,
        "printf out; while :; do printf '0123456789abcdef0123456789abcdef' >&2; done",
        Duration::from_secs(5),
    );
    let snapshot = done(&manager, id);
    assert_eq!(snapshot.state, TaskState::OutputLimit);
    assert_eq!(snapshot.stdout, "out");
    assert_eq!(snapshot.stderr.len(), MAX_OUTPUT_BYTES_PER_STREAM);
    assert!(snapshot.truncated);
}

#[test]
fn descendant_holding_pipes_is_cleaned_when_the_leader_exits() {
    let (dir, manager) = manager();
    let started = Instant::now();
    let id = shell(
        &manager,
        "(sleep 0.7; printf should-not-exist > descendant-marker) & printf leader-done",
        Duration::from_secs(3),
    );
    let snapshot = done(&manager, id);
    assert_eq!(snapshot.state, TaskState::Succeeded);
    assert_eq!(snapshot.stdout, "leader-done");
    assert!(started.elapsed() < Duration::from_millis(600));
    thread::sleep(Duration::from_millis(800));
    assert!(!dir.path().join("descendant-marker").exists());
}

#[test]
fn completed_cancel_does_not_touch_a_new_active_task() {
    let (_dir, manager) = manager();
    for _ in 0..20 {
        let first = shell(&manager, "exit 0", Duration::from_secs(2));
        let finished = done(&manager, first);
        let second = shell(
            &manager,
            "printf live; exec sleep 10",
            Duration::from_secs(3),
        );
        until(&manager, second, |s| s.stdout == "live");
        for _ in 0..20 {
            assert_eq!(manager.cancel(first).unwrap(), finished);
        }
        assert_eq!(manager.poll(second).unwrap().state, TaskState::Running);
        manager.cancel(second).unwrap();
        assert_eq!(done(&manager, second).state, TaskState::Cancelled);
    }
}

#[test]
fn immediate_exit_racing_repeated_cancel_has_one_stable_terminal_result() {
    let (_dir, manager) = manager();
    for _ in 0..30 {
        let id = shell(&manager, "exit 0", Duration::from_secs(2));
        for _ in 0..20 {
            manager.cancel(id).unwrap();
        }
        let finished = done(&manager, id);
        assert!(matches!(
            finished.state,
            TaskState::Cancelled | TaskState::Succeeded
        ));
        for _ in 0..20 {
            assert_eq!(manager.cancel(id).unwrap(), finished);
        }
    }
}

#[test]
fn concurrent_start_has_exactly_one_accepted_request() {
    let (_dir, manager) = manager();
    let manager = Arc::new(manager);
    let barrier = Arc::new(Barrier::new(9));
    let workers: Vec<_> = (0..8)
        .map(|_| {
            let manager = manager.clone();
            let barrier = barrier.clone();
            thread::spawn(move || {
                barrier.wait();
                manager.start(
                    "/bin/sleep".into(),
                    vec!["3".into()],
                    Duration::from_secs(4),
                )
            })
        })
        .collect();
    barrier.wait();
    let results: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();
    let ids: Vec<_> = results
        .iter()
        .filter_map(|r| r.as_ref().ok().copied())
        .collect();
    assert_eq!(ids.len(), 1);
    for result in results {
        assert!(result == Ok(ids[0]) || result == Err(TaskError::Busy { id: ids[0] }));
    }
    manager.cancel(ids[0]).unwrap();
    assert_eq!(done(&manager, ids[0]).state, TaskState::Cancelled);
}

#[test]
fn history_evicts_oldest_and_task_ids_are_not_reused_across_managers() {
    let (_dir, manager) = manager();
    let mut ids = Vec::new();
    for _ in 0..MAX_COMPLETED_TASKS + 3 {
        let id = shell(&manager, "exit 0", Duration::from_secs(2));
        done(&manager, id);
        ids.push(id);
    }
    for &id in &ids[..3] {
        assert_eq!(manager.poll(id), Err(TaskError::UnknownTask { id }));
        assert_eq!(manager.cancel(id), Err(TaskError::UnknownTask { id }));
    }
    for &id in &ids[3..] {
        assert_eq!(manager.poll(id).unwrap().state, TaskState::Succeeded);
    }
    let (_other_dir, other) = self::manager();
    let other_id = shell(&other, "exec sleep 2", Duration::from_secs(3));
    assert!(other_id > *ids.last().unwrap());
    assert_eq!(
        other.cancel(ids[3]),
        Err(TaskError::UnknownTask { id: ids[3] })
    );
    other.cancel(other_id).unwrap();
    done(&other, other_id);
}

#[test]
fn manager_drop_cancels_and_reaps_normal_children_and_their_group() {
    let (dir, manager) = manager();
    let id = shell(
        &manager,
        "(sleep 0.7; printf bad > orphan-marker) & printf started; sleep 10",
        Duration::from_secs(20),
    );
    until(&manager, id, |s| s.stdout == "started");
    let started = Instant::now();
    drop(manager);
    assert!(started.elapsed() < Duration::from_secs(2));
    thread::sleep(Duration::from_millis(800));
    assert!(!dir.path().join("orphan-marker").exists());
}

#[test]
fn unicode_split_across_live_reads_is_not_permanently_corrupted() {
    let (_dir, manager) = manager();
    let id = shell(
        &manager,
        r"printf '\360\237'; sleep 0.15; printf '\246\200'; printf '\377' >&2",
        Duration::from_secs(2),
    );
    let partial = until(&manager, id, |s| !s.stdout.is_empty());
    assert!(!partial.state.is_terminal());
    assert_eq!(partial.stdout, "�");
    let finished = done(&manager, id);
    assert_eq!(finished.stdout, "🦀");
    assert_eq!(finished.stderr, "�");
    assert_eq!(finished.state, TaskState::Succeeded);
    assert!(!finished.truncated);
}

#[test]
fn spawn_failure_is_retained_and_never_retried() {
    let (dir, manager) = manager();
    let missing = dir
        .path()
        .join("missing-executable")
        .to_string_lossy()
        .into_owned();
    let id = manager
        .start(missing, vec![], Duration::from_secs(2))
        .unwrap();
    let snapshot = done(&manager, id);
    assert_eq!(snapshot.state, TaskState::SpawnFailed);
    assert!(snapshot.error.is_some());
    assert_eq!(manager.cancel(id).unwrap(), snapshot);
    let id = shell(
        &manager,
        "printf x >> invocation-count; exit 7",
        Duration::from_secs(2),
    );
    assert_eq!(done(&manager, id).state, TaskState::Failed);
    for _ in 0..20 {
        manager.poll(id).unwrap();
        manager.cancel(id).unwrap();
    }
    assert_eq!(
        fs::read_to_string(dir.path().join("invocation-count")).unwrap(),
        "x"
    );
}

#[test]
fn command_validation_and_unknown_id_do_not_create_pending_work() {
    let (_dir, manager) = manager();
    for (program, args) in [
        (String::new(), vec![]),
        ("x".repeat(MAX_PROGRAM_BYTES + 1), vec![]),
        ("a\0b".into(), vec![]),
        ("echo".into(), vec!["a\0b".into()]),
        ("echo".into(), vec![String::new(); MAX_ARGUMENTS + 1]),
        ("echo".into(), vec!["x".repeat(MAX_ARGUMENT_BYTES + 1)]),
    ] {
        assert_eq!(
            manager.start(program, args, Duration::from_secs(1)),
            Err(TaskError::InvalidCommand)
        );
    }
    for timeout in [Duration::ZERO, MAX_TIMEOUT + Duration::from_nanos(1)] {
        assert_eq!(
            manager.start("/bin/echo".into(), vec![], timeout),
            Err(TaskError::InvalidTimeout)
        );
    }
    assert_eq!(manager.poll(0), Err(TaskError::UnknownTask { id: 0 }));
    assert_eq!(manager.cancel(0), Err(TaskError::UnknownTask { id: 0 }));
    let id = shell(&manager, "exit 0", Duration::from_secs(2));
    assert_eq!(done(&manager, id).state, TaskState::Succeeded);
}
