//! A single transient Java startup owner. The wire handler remains sequential;
//! only launch/initialize/abort runs on this joined, workspace-owned worker.
use super::{java_diagnostics_refresh_supported, LanguageSession};
use crate::{error, java_launch::JavaLaunch, Workspace};
use cedar_language::{
    LspAbortHandle, LspClient, ShutdownOutcome, WindowsCleanupStatus, WindowsRootExit,
};
use cedar_protocol::{Payload, RemoteError};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const STARTUP_ENVELOPE: Duration = Duration::from_secs(75);
const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Default)]
pub(crate) struct JavaStartup {
    last_id: u64,
    record: Option<StartupRecord>,
    // If creating a cleanup thread fails, retain the signalled client here.
    // Its Drop joins only at Workspace destruction, never on Cancel's handler.
    cleanup_fallback: Option<CleanupOwnership>,
    #[cfg(test)]
    fail_cleanup_spawn: bool,
    // An uncertain owner cannot be replaced in the same Workspace/agent.
    restart_blocked: bool,
}

impl std::fmt::Debug for JavaStartup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JavaStartup")
            .field("last_id", &self.last_id)
            .field("pending", &self.pending())
            .field("restart_blocked", &self.restart_blocked)
            .finish()
    }
}

struct StartupRecord {
    id: u64,
    control: Arc<StartupControl>,
    worker: Option<JoinHandle<StartupResult>>,
    terminal: Option<Value>,
}

struct StartupControl {
    cancelled: AtomicBool,
    prepared: AtomicBool,
    worker_failed: AtomicBool,
    #[cfg(test)]
    panic_after_publish: AtomicBool,
    #[cfg(test)]
    panic_after_adoption: AtomicBool,
    adopted: AtomicBool,
    transfer: Mutex<Option<PreparedClient>>,
    wake: OnceLock<thread::Thread>,
    // Publication and cancellation share this short lock. It never surrounds
    // process creation, an LSP gate, a pipe write, a wait, or a join.
    published: Mutex<Option<(u32, LspAbortHandle)>>,
    deadline: Instant,
}

impl StartupControl {
    fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        if let Some((_, handle)) = &*self.published.lock().unwrap() {
            handle.signal();
        }
        self.wake();
    }

    fn wake(&self) {
        if let Some(worker) = self.wake.get() {
            worker.unpark();
        }
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    fn publish(&self, client: &LspClient) {
        let handle = client.abort_handle();
        let mut published = self.published.lock().unwrap();
        if self.is_cancelled() || Instant::now() >= self.deadline {
            handle.signal();
        }
        *published = Some((client.process_id(), handle));
    }

    fn process_id(&self) -> Option<u32> {
        self.published.lock().unwrap().as_ref().map(|(pid, _)| *pid)
    }
}

// Only the startup worker and the sequential Workspace handler can transfer
// this slot. A completed JoinHandle never owns an unconsumed successful client.
struct PreparedClient {
    client: LspClient,
    initialize: Value,
    root_uri: String,
    maven: Option<crate::java_maven::MavenSession>,
}

struct CleanupOwnership {
    client: LspClient,
    predecessor: Option<JoinHandle<StartupResult>>,
}

enum StartupResult {
    Adopted,
    Finished {
        cancelled: bool,
        cleanup_verified: bool,
        error: Option<RemoteError>,
    },
}

impl JavaStartup {
    fn pending(&self) -> bool {
        self.record
            .as_ref()
            .is_some_and(|record| record.worker.is_some() && record.terminal.is_none())
    }
}

impl Drop for JavaStartup {
    fn drop(&mut self) {
        if let Some(record) = &mut self.record {
            record.control.cancel();
            if let Some(worker) = record.worker.take() {
                let _ = worker.join();
            }
            // Defensive ownership recovery if the startup worker unwound after
            // publishing its prepared slot but before consuming cancellation.
            if let Some(prepared) = record
                .control
                .transfer
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .take()
            {
                prepared.client.abort_and_join();
            }
        }
        if let Some(owned) = self.cleanup_fallback.take() {
            if let Some(predecessor) = owned.predecessor {
                let _ = predecessor.join();
            }
            owned.client.abort_and_join();
        }
    }
}

fn cleanup_verified(outcome: ShutdownOutcome) -> bool {
    if outcome.linux.is_some() {
        return super::linux_cleanup_verified(outcome);
    }
    outcome.windows.is_some_and(|windows| {
        windows.cleanup == WindowsCleanupStatus::Joined
            && windows.errors == cedar_language::WindowsCleanupErrors::default()
            && matches!(
                windows.root_exit,
                WindowsRootExit::BeforeTermination(_) | WindowsRootExit::AfterTermination(_)
            )
    })
}

fn verified_linux_stop_receipt(value: &Value) -> bool {
    let Some(body) = value.get("shutdown").and_then(Value::as_object) else {
        return false;
    };
    let keys = [
        "platform",
        "status",
        "reason",
        "root_exit",
        "cleanup_joined",
        "shutdown_response_received",
        "exit_frame_completed",
    ];
    if value.get("stopped").and_then(Value::as_bool) != Some(true)
        || body.len() != keys.len()
        || keys.iter().any(|key| !body.contains_key(*key))
        || body["platform"] != "linux"
        || body["cleanup_joined"].as_bool() != Some(true)
        || body["shutdown_response_received"].as_bool().is_none()
        || body["exit_frame_completed"].as_bool().is_none()
        || !matches!(
            body["status"].as_str(),
            Some("graceful" | "forced" | "error")
        )
        || !matches!(
            body["reason"].as_str(),
            Some(
                "root_exited"
                    | "grace_expired"
                    | "aborted"
                    | "transport_failure"
                    | "worker_panicked"
            )
        )
    {
        return false;
    }
    let Some(exit) = body["root_exit"].as_object() else {
        return false;
    };
    if exit.len() != 2 {
        return false;
    }
    let code = match exit.get("kind").and_then(Value::as_str) {
        Some("code") => match exit.get("code").and_then(Value::as_u64) {
            Some(code) if code <= 255 => Some(code),
            _ => return false,
        },
        Some("signal") => match exit.get("signal").and_then(Value::as_u64) {
            Some(signal) if (1..=64).contains(&signal) => None,
            _ => return false,
        },
        _ => return false,
    };
    body["status"] != "graceful"
        || (code == Some(0)
            && body["reason"] == "root_exited"
            && body["shutdown_response_received"] == true
            && body["exit_frame_completed"] == true)
}

fn bounded_error(mut error: RemoteError) -> RemoteError {
    const MAX_ERROR_BYTES: usize = 512;
    if error.message.len() > MAX_ERROR_BYTES {
        let mut end = MAX_ERROR_BYTES;
        while !error.message.is_char_boundary(end) {
            end -= 1;
        }
        error.message.truncate(end);
    }
    error
}

fn timeout_error() -> RemoteError {
    error(
        "language_startup_timeout",
        "Java startup exceeded its original deadline",
    )
}

fn cleanup_result(
    client: LspClient,
    cancelled: bool,
    failure: Option<RemoteError>,
) -> StartupResult {
    let verified = cleanup_verified(client.abort_and_join());
    StartupResult::Finished {
        cancelled,
        cleanup_verified: verified,
        error: if verified {
            failure
        } else {
            Some(error("language_cleanup_unverified", "Java startup closed; owned cleanup could not be verified. Check previous server cleanup before explicitly reconnecting and starting again."))
        },
    }
}

// Unwinding a startup worker must not strand a prepared client in shared
// storage without its deadline watcher. Atomic adoption transfers that duty to
// Workspace; an adopted slot is empty and this guard must not signal its client.
struct StartupWorkerGuard(Arc<StartupControl>);
impl Drop for StartupWorkerGuard {
    fn drop(&mut self) {
        if thread::panicking() {
            self.0.worker_failed.store(true, Ordering::SeqCst);
        }
        let prepared = self
            .0
            .transfer
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        if let Some(prepared) = prepared {
            prepared.client.abort_and_join();
        }
    }
}

fn startup_worker(
    control: Arc<StartupControl>,
    root_uri: String,
    prepare: impl FnOnce() -> Result<JavaLaunch, RemoteError>,
) -> StartupResult {
    let _guard = StartupWorkerGuard(control.clone());
    let _ = control.wake.set(thread::current());
    let before_spawn = |failure| StartupResult::Finished {
        cancelled: control.is_cancelled(),
        cleanup_verified: true,
        error: failure,
    };
    if control.is_cancelled() {
        return before_spawn(None);
    }
    if Instant::now() >= control.deadline {
        return before_spawn(Some(timeout_error()));
    }
    let launch = match prepare() {
        Ok(launch) => launch,
        Err(failure) => return before_spawn(Some(bounded_error(failure))),
    };
    if control.is_cancelled() {
        return before_spawn(None);
    }
    if Instant::now() >= control.deadline {
        return before_spawn(Some(timeout_error()));
    }
    let client = match LspClient::spawn(launch.config, launch.options) {
        Ok(client) => client,
        // Spawn does not provide a typed partial-ownership cleanup report. Do
        // not infer verified teardown from its error string or retry in-place.
        Err(failure) => {
            return StartupResult::Finished {
                cancelled: false,
                cleanup_verified: false,
                error: Some(bounded_error(error(
                    "language_startup_failed",
                    failure.to_string(),
                ))),
            }
        }
    };
    control.publish(&client);
    if control.is_cancelled() {
        return cleanup_result(client, true, None);
    }
    if Instant::now() >= control.deadline {
        return cleanup_result(client, false, Some(timeout_error()));
    }
    let initialized = client.initialize_with_deadline(
        Some(&root_uri),
        launch.initialization_options,
        INITIALIZE_TIMEOUT,
        control.deadline,
    );
    if control.is_cancelled() {
        return cleanup_result(client, true, None);
    }
    if Instant::now() >= control.deadline {
        return cleanup_result(client, false, Some(timeout_error()));
    }
    match initialized {
        Ok(initialize) => {
            if let Some(profile) = launch.maven.as_ref() {
                if let Err(failure) = crate::java_maven::current_pom_matches(profile) {
                    return cleanup_result(client, false, Some(failure));
                }
            }
            {
                let mut transfer = control
                    .transfer
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                *transfer = Some(PreparedClient {
                    client,
                    initialize,
                    root_uri,
                    maven: launch.maven,
                });
                control.prepared.store(true, Ordering::SeqCst);
            }
            #[cfg(test)]
            assert!(
                !control.panic_after_publish.load(Ordering::SeqCst),
                "synthetic panic after prepared publication"
            );
            // Poll can take the prepared client directly and immediately own
            // the authoritative Ready session. Until then this worker remains
            // responsible for cancellation and the original deadline.
            loop {
                let mut transfer = control
                    .transfer
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if control.adopted.load(Ordering::SeqCst) {
                    drop(transfer);
                    #[cfg(test)]
                    assert!(
                        !control.panic_after_adoption.load(Ordering::SeqCst),
                        "synthetic panic after adoption"
                    );
                    return StartupResult::Adopted;
                }
                let cancelled = control.is_cancelled();
                if cancelled || Instant::now() >= control.deadline {
                    let prepared = transfer
                        .take()
                        .expect("unadopted client is owned by startup");
                    drop(transfer);
                    return cleanup_result(
                        prepared.client,
                        cancelled,
                        if cancelled {
                            None
                        } else {
                            Some(timeout_error())
                        },
                    );
                }
                drop(transfer);
                thread::park_timeout(control.deadline.saturating_duration_since(Instant::now()));
            }
        }
        Err(failure) => {
            let failure = if matches!(failure, cedar_language::Error::Timeout(_)) {
                timeout_error()
            } else {
                bounded_error(error("language_error", failure.to_string()))
            };
            cleanup_result(client, false, Some(failure))
        }
    }
}

impl Workspace {
    pub(super) fn owns_java_startup(&self, id: u64) -> bool {
        self.java_startup_record(id).is_ok()
    }

    #[cfg(target_os = "linux")]
    pub(super) fn language_restart_cleanup_blocked(&self) -> bool {
        self.java_startup.restart_blocked
    }

    #[cfg(target_os = "linux")]
    pub(super) fn block_unverified_language_restart(&mut self) {
        self.java_startup.restart_blocked = true;
    }

    pub(super) fn require_language_start_settled(&self) -> Result<(), RemoteError> {
        if self.java_startup.pending() {
            return Err(error(
                "language_start_in_progress",
                "Poll or cancel the current Java startup before changing language sessions",
            ));
        }
        Ok(())
    }

    pub(super) fn require_language_start_available(&mut self) -> Result<(), RemoteError> {
        self.require_language_start_settled()?;
        // A retired Ready may still have its resource-free predecessor marker.
        // Join only an already-finished worker before replacing the record.
        if let Some(record) = self.java_startup.record.as_mut() {
            if record.terminal.is_some()
                && record.worker.as_ref().is_some_and(JoinHandle::is_finished)
            {
                let _ = record.worker.take().unwrap().join();
            }
            if record.worker.is_some() && self.language.is_none() {
                return Err(error(
                    "language_start_in_progress",
                    "The previous Java startup owner is finishing; poll before starting another",
                ));
            }
        }
        if self.java_startup.restart_blocked {
            return Err(error("language_cleanup_unverified", "Check previous server cleanup before explicitly reconnecting and starting another language server; cleanup is unverified"));
        }
        if self.language.is_some() {
            return Err(error(
                "language_running",
                "Stop the current language server before starting another",
            ));
        }
        Ok(())
    }

    pub(super) fn begin_java_startup(
        &mut self,
        java_executable: String,
        distribution: String,
        data_directory: String,
    ) -> Result<Payload, RemoteError> {
        self.require_language_start_available()?;
        // The existing one-shot GC diagnostic host intentionally retains its
        // synchronous, fixed diagnostic recipe; production constructors have no
        // diagnostic profile and never take this branch.
        #[cfg(feature = "windows-java-gc-diagnostic")]
        if self.windows_java_gc_diagnostic.is_some() {
            return Err(error(
                "invalid_java_gc_diagnostic",
                "The fixed GC diagnostic host requires synchronous Java startup",
            ));
        }
        let root = self.root.clone();
        self.begin_java_startup_worker(
            move || {
                crate::java_launch::production(
                    &root,
                    &java_executable,
                    &distribution,
                    &data_directory,
                )
            },
            STARTUP_ENVELOPE,
        )
    }

    pub(super) fn begin_java_maven_startup(
        &mut self,
        java_executable: String,
        distribution: String,
        data_directory: String,
        local_repository: String,
    ) -> Result<Payload, RemoteError> {
        self.require_language_start_available()?;
        let root = self.root.clone();
        self.begin_java_startup_worker(
            move || {
                crate::java_maven::production(
                    &root,
                    &java_executable,
                    &distribution,
                    &data_directory,
                    &local_repository,
                )
            },
            STARTUP_ENVELOPE,
        )
    }

    fn begin_java_startup_worker(
        &mut self,
        prepare: impl FnOnce() -> Result<JavaLaunch, RemoteError> + Send + 'static,
        work_budget: Duration,
    ) -> Result<Payload, RemoteError> {
        self.require_language_start_available()?;
        let accepted_at = Instant::now();
        let root_uri = url::Url::from_directory_path(&self.root)
            .map_err(|_| error("invalid_path", "Cannot create root URI"))?
            .to_string();
        let id = self.java_startup.last_id.checked_add(1).ok_or_else(|| {
            error(
                "language_startup_exhausted",
                "Reconnect before starting another Java session",
            )
        })?;
        self.java_startup.last_id = id;
        let control = Arc::new(StartupControl {
            cancelled: AtomicBool::new(false),
            prepared: AtomicBool::new(false),
            worker_failed: AtomicBool::new(false),
            #[cfg(test)]
            panic_after_publish: AtomicBool::new(false),
            #[cfg(test)]
            panic_after_adoption: AtomicBool::new(false),
            adopted: AtomicBool::new(false),
            transfer: Mutex::new(None),
            wake: OnceLock::new(),
            published: Mutex::new(None),
            deadline: accepted_at + work_budget,
        });
        let worker_control = control.clone();
        let worker = thread::Builder::new()
            .name("cedar-java-startup".into())
            .spawn(move || startup_worker(worker_control, root_uri, prepare))
            .map_err(|failure| error("language_startup_failed", failure.to_string()))?;
        self.java_startup.record = Some(StartupRecord {
            id,
            control,
            worker: Some(worker),
            terminal: None,
        });
        // Begin acknowledges allocation only, even if the worker completes or
        // fails before this handler returns. Poll owns all subsequent states.
        Ok(Payload::Language {
            value: json!({"startup_id":id,"state":"starting","process_id":self.java_startup_record(id)?.control.process_id()}),
        })
    }

    fn java_startup_record(&self, id: u64) -> Result<&StartupRecord, RemoteError> {
        self.java_startup
            .record
            .as_ref()
            .filter(|record| id != 0 && record.id == id)
            .ok_or_else(|| {
                error(
                    "unknown_language_startup",
                    "No matching Java startup exists in this workspace session",
                )
            })
    }

    fn java_startup_snapshot(&self, id: u64) -> Result<Payload, RemoteError> {
        let record = self.java_startup_record(id)?;
        Ok(Payload::Language {
            value: match &record.terminal {
                Some(value) => value.clone(),
                None => {
                    json!({"startup_id":id,"state":if record.control.is_cancelled() || record.control.worker_failed.load(Ordering::SeqCst) || Instant::now() >= record.control.deadline {"cancelling"} else {"starting"},"process_id":record.control.process_id()})
                }
            },
        })
    }

    pub(super) fn poll_java_startup(&mut self, id: u64) -> Result<Payload, RemoteError> {
        self.java_startup_record(id)?;
        self.reap_java_startup(id);
        if self.java_startup_record(id)?.terminal.is_some() {
            return self.java_startup_snapshot(id);
        }
        let record = self.java_startup_record(id)?;
        let prepared = {
            let mut transfer = record
                .control
                .transfer
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if !record.control.is_cancelled()
                && !record.control.worker_failed.load(Ordering::SeqCst)
                && Instant::now() < record.control.deadline
            {
                let prepared = transfer.take();
                if prepared.is_some() {
                    record.control.adopted.store(true, Ordering::SeqCst);
                }
                prepared
            } else {
                None
            }
        };
        if let Some(PreparedClient {
            client,
            mut initialize,
            root_uri,
            maven,
        }) = prepared
        {
            let refresh = java_diagnostics_refresh_supported(true, &initialize);
            let imports = super::imports::supported(true, &initialize);
            initialize["cedar_java_diagnostics_refresh"] = json!(refresh);
            initialize["cedar_java_organize_imports"] = json!(imports);
            let maven_model = super::maven::supported(maven.is_some(), &initialize);
            initialize["cedar_java_maven_model"] = json!(maven_model);
            if let Some(profile) = maven.as_ref() {
                initialize["cedar_java_profile"] = json!("maven_leaf");
                initialize["cedar_java_maven_pom_sha256"] = json!(profile.pom_sha256);
            } else if let Some(object) = initialize.as_object_mut() {
                object.remove("cedar_java_profile");
                object.remove("cedar_java_maven_pom_sha256");
            }
            let value = json!({"started":true,"initialize":initialize,"root_uri":root_uri,"process_id":client.process_id()});
            self.language = Some(LanguageSession {
                client,
                startup_id: Some(id),
                production_java: true,
                java_diagnostics_refresh: refresh,
                java_organize_imports: imports,
                java_maven: maven,
                java_maven_model: maven_model,
                opened: HashMap::new(),
                #[cfg(feature = "windows-language-validation")]
                java_validation: None,
            });
            let record = self.java_startup.record.as_mut().unwrap();
            record.terminal = Some(json!({"startup_id":id,"state":"ready","language":value}));
            record.control.wake();
        }
        self.reap_java_startup(id);
        self.java_startup_snapshot(id)
    }

    fn reap_java_startup(&mut self, id: u64) {
        let record = self.java_startup.record.as_mut().unwrap();
        if record.worker.as_ref().is_some_and(JoinHandle::is_finished) {
            let result = match record.worker.take().unwrap().join() {
                Ok(result) => result,
                Err(_)
                    if record.control.adopted.load(Ordering::SeqCst)
                        && (self
                            .language
                            .as_ref()
                            .is_some_and(|session| session.startup_id == Some(id))
                            || record.terminal.as_ref().is_some_and(|value| {
                                matches!(value["state"].as_str(), Some("cancelled" | "failed"))
                            })) =>
                {
                    // Transfer already completed. The failed predecessor owned
                    // no client; preserve the authoritative live Ready session
                    // or the cleanup evidence already recorded by legacy Stop.
                    return;
                }
                Err(_) => StartupResult::Finished {
                    cancelled: false,
                    cleanup_verified: false,
                    error: Some(error(
                        "language_cleanup_unverified",
                        "Java startup worker stopped unexpectedly; check the previous server cleanup before reconnecting and starting again",
                    )),
                },
            };
            match result {
                StartupResult::Adopted => {}
                StartupResult::Finished {
                    cancelled,
                    cleanup_verified,
                    error: failure,
                } => {
                    self.finish_java_startup(id, cancelled, cleanup_verified, failure);
                }
            }
        }
    }

    fn finish_java_startup(
        &mut self,
        id: u64,
        cancelled: bool,
        verified: bool,
        failure: Option<RemoteError>,
    ) {
        self.java_startup.restart_blocked |= !verified;
        let value = if cancelled && verified {
            json!({"startup_id":id,"state":"cancelled","cleanup_verified":true})
        } else {
            let failure = bounded_error(failure.unwrap_or_else(|| {
                error(
                    "language_cleanup_unverified",
                    "Java startup cleanup could not be verified; check the previous server cleanup before reconnecting and starting again",
                )
            }));
            json!({"startup_id":id,"state":"failed","cleanup_verified":verified,"error":{"code":failure.code,"message":failure.message}})
        };
        self.java_startup.record.as_mut().unwrap().terminal = Some(value);
    }

    fn start_java_cleanup_worker(
        &mut self,
        id: u64,
        client: LspClient,
        cancelled: bool,
        failure: Option<RemoteError>,
    ) -> Result<(), RemoteError> {
        #[cfg(test)]
        let fail_spawn = self.java_startup.fail_cleanup_spawn;
        let record = self.java_startup.record.as_mut().unwrap();
        debug_assert_eq!(record.id, id);
        record.terminal = None;
        // Keep a second reference to the ownership slot until the worker exists.
        // A failed OS thread allocation must not drop/join a client on Cancel's
        // sequential handler. Its already-signalled owner is retained until EOF.
        let owned = Arc::new(Mutex::new(Some(CleanupOwnership {
            client,
            predecessor: record.worker.take(),
        })));
        let worker_owned = owned.clone();
        let spawn = move || {
            thread::Builder::new()
                .name("cedar-java-cleanup".into())
                .spawn(move || {
                    let owned = worker_owned.lock().unwrap().take().unwrap();
                    if let Some(predecessor) = owned.predecessor {
                        let _ = predecessor.join();
                    }
                    cleanup_result(owned.client, cancelled, failure)
                })
        };
        #[cfg(test)]
        let spawned = if fail_spawn {
            // Model OS thread allocation failure: the unstarted closure is
            // dropped, including its reference to the shared ownership slot.
            drop(spawn);
            Err(std::io::Error::other(
                "synthetic cleanup allocation failure",
            ))
        } else {
            spawn()
        };
        #[cfg(not(test))]
        let spawned = spawn();
        match spawned {
            Ok(worker) => {
                record.worker = Some(worker);
                Ok(())
            }
            Err(_) => {
                self.java_startup.cleanup_fallback = owned.lock().unwrap().take();
                self.finish_java_startup(
                    id,
                    false,
                    false,
                    Some(error(
                        "language_cleanup_unverified",
                        "Cannot start Java cleanup owner; check the previous server cleanup before reconnecting and starting again",
                    )),
                );
                Ok(())
            }
        }
    }

    pub(super) fn cancel_java_startup(&mut self, id: u64) -> Result<Payload, RemoteError> {
        let record = self.java_startup_record(id)?;
        if record
            .terminal
            .as_ref()
            .is_some_and(|value| value["state"] != "ready")
        {
            return self.java_startup_snapshot(id);
        }
        record.control.cancel();
        if self
            .language
            .as_ref()
            .is_some_and(|session| session.startup_id == Some(id))
        {
            let session = self.language.take().unwrap();
            self.start_java_cleanup_worker(id, session.client, true, None)?;
        }
        // Deliberately do not join, even an apparently finished worker, here.
        // Poll consumes the result and provides terminal cleanup evidence.
        self.java_startup_snapshot(id)
    }

    pub(super) fn retire_java_startup(
        &mut self,
        id: Option<u64>,
        stopped: &Result<Payload, RemoteError>,
    ) {
        let verified = stopped.as_ref().is_ok_and(|payload| matches!(payload, Payload::Language { value } if
            if cfg!(target_os = "linux") {
                verified_linux_stop_receipt(value)
            } else {
                value["shutdown"]["cleanup_joined"] == true && value["shutdown"]["status"] != "error"
            }
        ));
        // A synchronous typed session has no startup ID, but uncertain Linux
        // cleanup still owns the shared replacement-start safety latch.
        #[cfg(target_os = "linux")]
        if !verified {
            self.block_unverified_language_restart();
        }
        let Some(id) = id else {
            return;
        };
        if self.java_startup_record(id).is_err() {
            return;
        }
        self.finish_java_startup(id, true, verified, stopped.as_ref().err().cloned());
    }
}

#[cfg(test)]
#[path = "language_startup_tests.rs"]
mod tests;
