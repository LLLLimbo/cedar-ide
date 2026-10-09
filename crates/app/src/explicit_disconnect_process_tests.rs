//! Opt-in explicit Disconnect acceptance through owned, bounded stdio peers.
//! Generated fixtures only: no SSH, execution permission, network, or user files.
use super::*;
use cedar_recovery::{record_id, Store};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const WAIT: Duration = Duration::from_secs(5);
const CANCEL_BOUND: Duration = Duration::from_secs(3);
const SYNTHETIC_ROOT: &str = "/synthetic-explicit-disconnect";
const PATH: &str = "draft 草稿.txt";
const SOURCE: &str = "saved source 草稿 🐻\nsecond line\n";
const DRAFT: &str = "retained unsaved draft 草稿 🐻\nsecond draft line\n";

fn binary(name: &str) -> PathBuf {
    let path = PathBuf::from(std::env::var_os(name).expect("explicit acceptance binary required"));
    assert!(path.is_absolute() && path.is_file());
    path
}

fn revision(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn form(root: &str) -> ConnectForm {
    ConnectForm {
        ssh: false,
        local_root: root.into(),
        allow_run: false,
        ..Default::default()
    }
}

fn fixture(root: &Path, mode: &str) {
    fs::create_dir(root).unwrap();
    fs::write(
        root.join(".cedar-transport-fixture"),
        b"cedar-transport-fixture-v1\n",
    )
    .unwrap();
    fs::write(root.join("fixture-mode"), mode).unwrap();
    fs::write(root.join(PATH), SOURCE).unwrap();
    fs::write(
        root.join("hello.json"),
        serde_json::to_vec(&Payload::Hello {
            protocol: cedar_protocol::PROTOCOL_VERSION,
            root: SYNTHETIC_ROOT.into(),
            agent: None,
        })
        .unwrap(),
    )
    .unwrap();
}

fn wait_file(root: &Path, name: &str) -> Vec<u8> {
    let deadline = Instant::now() + WAIT;
    loop {
        if let Ok(bytes) = fs::read(root.join(name)) {
            return bytes;
        }
        assert!(
            Instant::now() < deadline,
            "controlled fixture marker timed out"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn pid(root: &Path, name: &str) -> u32 {
    String::from_utf8(wait_file(root, name))
        .unwrap()
        .trim()
        .parse()
        .unwrap()
}

fn operations(root: &Path) -> Vec<String> {
    fs::read_to_string(root.join("requests"))
        .unwrap()
        .lines()
        .enumerate()
        .map(|(index, line)| {
            let request: cedar_protocol::Request = serde_json::from_str(line).unwrap();
            assert_eq!(request.id, index as u64 + 1);
            match request.op {
                Operation::Hello => "hello",
                Operation::List { path } if path.is_empty() => "list",
                Operation::Read { path } if path == PATH || path == "fixture.txt" => "read",
                _ => panic!("explicit Disconnect emitted a forbidden operation"),
            }
            .into()
        })
        .collect()
}

fn response(app: &mut CedarApp) {
    let event = app
        .result_rx
        .recv_timeout(WAIT)
        .expect("peer response timed out");
    assert!(matches!(&event, WorkerEvent::Response(reply)
        if reply.generation == app.generation && reply.connected && reply.result.is_ok()));
    app.apply_worker_event(event);
}

fn start(
    app: &mut CedarApp,
    root: &Path,
    peer: &Path,
    connection: ConnectForm,
    close_timeout: Option<Duration>,
) {
    assert!(app.worker.is_none());
    app.generation += 1;
    app.state = ConnectionState::Connecting;
    app.connecting_form = Some(connection);
    app.pending.clear();
    app.agent_info = None;
    app.worker = Some(match close_timeout {
        Some(timeout) => Worker::spawn_agent_with_close_timeout(
            peer.into(),
            root.into(),
            app.generation,
            app.result_tx.clone(),
            app.editor_ctx.clone(),
            timeout,
        ),
        None => Worker::spawn_agent(
            peer.into(),
            root.into(),
            app.generation,
            app.result_tx.clone(),
            app.editor_ctx.clone(),
        ),
    });
    response(app); // Hello schedules the ordinary initial List.
    response(app);
    assert!(app.ready() && app.pending.is_empty());
    assert!(!app.execution_trusted());
}

fn begin_disconnect(app: &mut CedarApp) {
    let generation = app.generation;
    assert!(app.disconnect_problem().is_none());
    app.disconnect_idle();
    assert!(app.state == ConnectionState::Disconnecting);
    assert_eq!(app.generation, generation);
    assert!(app.worker.is_none() && app.pending.is_empty() && app.agent_info.is_none());
    assert!(!app.ready());
    assert_eq!(app.notice, disconnect::WAITING);
    // A repeat click cannot claim completion or replace the in-flight owner.
    app.disconnect_idle();
    assert!(app.state == ConnectionState::Disconnecting);
    assert_eq!(app.generation, generation);
}

fn finish_disconnect(app: &mut CedarApp) {
    let event = app
        .result_rx
        .recv_timeout(WAIT)
        .expect("owned close receipt timed out");
    assert!(
        matches!(&event, WorkerEvent::Closed { generation, result: Ok(()) }
        if *generation == app.generation)
    );
    app.apply_worker_event(event);
    assert!(app.state == ConnectionState::Disconnected);
    assert!(app.worker.is_none() && app.pending.is_empty());
}

fn normal_agent_receipt(root: &Path, agent_pid: u32) {
    assert_eq!(wait_file(root, "relay-owner-eof"), b"true\n");
    let receipt: serde_json::Value =
        serde_json::from_slice(&wait_file(root, "relay-agent-reaped")).unwrap();
    assert_eq!(receipt["process_id"], agent_pid);
    assert_eq!(receipt["cleanup_verified"], true);
    assert_eq!(receipt["exit_success"], true);
    assert!(!root.join("safety-expired").exists());
}

fn frame(app: &mut CedarApp, time: f64, events: Vec<egui::Event>) {
    let ctx = app.editor_ctx.clone();
    let mut native = eframe::Frame::_new_kittest();
    let _ = ctx.run(
        egui::RawInput {
            time: Some(time),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 700.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| eframe::App::update(app, ctx, &mut native),
    );
}

fn undo_events(redo: bool) -> Vec<egui::Event> {
    let modifiers = if redo {
        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT
    } else {
        egui::Modifiers::COMMAND
    };
    [true, false]
        .into_iter()
        .map(|pressed| egui::Event::Key {
            key: egui::Key::Z,
            physical_key: Some(egui::Key::Z),
            pressed,
            repeat: false,
            modifiers,
        })
        .collect()
}

fn selection(app: &CedarApp) -> egui::text::CCursorRange {
    egui::TextEdit::load_state(
        &app.editor_ctx,
        egui::Id::new(("editor", app.documents[0].id)),
    )
    .unwrap()
    .cursor
    .char_range()
    .unwrap()
}

fn persist(app: &mut CedarApp) {
    let workspace = app.recovery_workspace().unwrap();
    let ctx = app.editor_ctx.clone();
    app.recovery_tick(&ctx);
    app.recovery.flush();
    let deadline = Instant::now() + WAIT;
    loop {
        app.recovery_tick(&ctx);
        assert!(app.recovery.error.is_none());
        if app.recovery.protected(&workspace, &app.documents[0]) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "generated recovery persistence timed out"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn retain_unknown_draft(app: &mut CedarApp) {
    let mut doc = Document::new(1, PATH.into(), SOURCE.into(), revision(SOURCE.as_bytes()));
    doc.text = DRAFT.into();
    doc.edit_version = 1;
    doc.interrupted_save = interrupted_save::InterruptedSave::capture(app, &doc);
    assert!(doc.interrupted_save.is_some());
    app.documents.push(doc);
    app.active_document = Some(1);
    app.next_document = 2;
}

fn assert_draft(app: &CedarApp) {
    assert_eq!(app.documents.len(), 1);
    let doc = &app.documents[0];
    assert_eq!(doc.text, DRAFT);
    assert_eq!(doc.saved_text, SOURCE);
    assert_eq!(
        doc.revision.as_deref(),
        Some(revision(SOURCE.as_bytes()).as_str())
    );
    assert!(doc.dirty() && !doc.saving && doc.interrupted_save.is_some());
    assert_eq!(app.active_document, Some(doc.id));
}

fn late_read(app: &mut CedarApp, generation: u64, id: u64) {
    app.apply_worker_event(WorkerEvent::Response(Event {
        generation,
        id,
        connected: true,
        result: Ok(Payload::File {
            path: "fixture.txt".into(),
            text: "late generated text must not be adopted".into(),
            revision: "late-generated-revision".into(),
        }),
    }));
}

#[test]
#[ignore = "requires explicit normal agent and peer binaries; bounded process acceptance"]
fn normal_agent_explicit_disconnect_reconnect_retains_native_draft_and_recovery() {
    let agent = binary("CEDAR_EXPLICIT_DISCONNECT_AGENT_BIN");
    let peer = binary("CEDAR_EXPLICIT_DISCONNECT_PEER_BIN");
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("disconnect workspace 草稿");
    let recovery_root = temp.path().join("private recovery");
    fixture(&root, "normal_agent_disconnect_relay");
    fs::write(root.join("relay-agent-path"), agent.to_str().unwrap()).unwrap();
    let source_hash = revision(&fs::read(root.join(PATH)).unwrap());
    let mut app = CedarApp::empty();
    app.open_form = false;
    app.recovery
        .start(Ok(recovery_root.clone()), &app.editor_ctx);
    start(&mut app, &root, &peer, form(root.to_str().unwrap()), None);
    let first_agent = pid(&root, "relay-agent-started");
    app.open(PATH.into(), None);
    response(&mut app);
    assert_eq!(app.documents[0].text, SOURCE);
    frame(&mut app, 0.0, vec![]);
    editor_state::commit(&app.editor_ctx, &mut app.documents[0], DRAFT.into(), 8);
    frame(&mut app, 1.0, vec![]);
    app.documents[0].interrupted_save =
        interrupted_save::InterruptedSave::capture(&app, &app.documents[0]);
    let unknown_save = app.documents[0].interrupted_save.clone();
    assert!(unknown_save.is_some());
    let workspace = app.recovery_workspace().unwrap();
    let workspace_key = app.workspace_key.clone();
    let accepted_root = app.root.clone();
    let editor_id = egui::Id::new(("editor", app.documents[0].id));
    let mut selected =
        egui::text::CCursorRange::two(egui::text::CCursor::new(8), egui::text::CCursor::new(2));
    selected.primary.prefer_next_row = false;
    selected.secondary.prefer_next_row = true;
    let mut editor = egui::TextEdit::load_state(&app.editor_ctx, editor_id).unwrap();
    editor.cursor.set_char_range(Some(selected));
    editor.store(&app.editor_ctx, editor_id);
    app.editor_ctx
        .memory_mut(|memory| memory.request_focus(editor_id));
    app.location_history.back.push(location_history::Location {
        generation: app.generation,
        document: app.documents[0].id,
        edit_version: app.documents[0].edit_version,
        selection: egui::text::CCursorRange::one(egui::text::CCursor::new(1)),
    });
    app.run_state.output = "retained generated command history".into();
    persist(&mut app);
    let old_generation = app.generation;
    assert_eq!(operations(&root), ["hello", "list", "read"]);
    begin_disconnect(&mut app);
    assert_eq!(app.workspace_key, workspace_key);
    assert_eq!(app.root, accepted_root);
    assert_eq!(app.recovery_workspace(), Some(workspace.clone()));
    assert!(app.recovery.protected(&workspace, &app.documents[0]));
    assert!(location_history::same_selection(selection(&app), selected));
    assert_eq!(app.location_history.back.len(), 1);
    finish_disconnect(&mut app);
    normal_agent_receipt(&root, first_agent);
    assert!(!app.unverified_local_close);
    assert_draft(&app);
    assert_eq!(app.documents[0].interrupted_save, unknown_save);
    assert_eq!(app.run_state.output, "retained generated command history");
    frame(&mut app, 2.0, vec![]);
    assert!(location_history::same_selection(selection(&app), selected));
    let ctx = app.editor_ctx.clone();
    assert!(app.history_step(location_history::Direction::Back, &ctx));
    assert_eq!(selection(&app).primary.index, 1);
    assert!(app.history_step(location_history::Direction::Forward, &ctx));
    assert!(location_history::same_selection(selection(&app), selected));
    app.editor_ctx
        .memory_mut(|memory| memory.request_focus(editor_id));
    frame(&mut app, 3.0, undo_events(false));
    assert_eq!(app.documents[0].text, SOURCE);
    frame(&mut app, 4.0, undo_events(true));
    assert_draft(&app);
    persist(&mut app);
    assert_eq!(operations(&root), ["hello", "list", "read"]);
    assert_eq!(revision(&fs::read(root.join(PATH)).unwrap()), source_hash);

    // A second explicitly requested connection starts only after the first
    // direct-child receipt and the relay's owned normal-agent wait receipt.
    for name in [
        "relay-owner-eof",
        "relay-agent-reaped",
        "relay-agent-started",
        "started",
        "requests",
    ] {
        fs::remove_file(root.join(name)).unwrap();
    }
    start(&mut app, &root, &peer, form(root.to_str().unwrap()), None);
    let second_agent = pid(&root, "relay-agent-started");
    assert!(app.generation > old_generation);
    late_read(&mut app, old_generation, 3);
    for result in [
        Ok(()),
        Err("transport_cleanup_unverified: old receipt".into()),
    ] {
        app.apply_worker_event(WorkerEvent::Closed {
            generation: old_generation,
            result,
        });
        assert!(app.ready() && !app.unverified_local_close);
    }
    app.apply_worker_event(WorkerEvent::TransportLost {
        generation: old_generation,
        message: "old generated loss".into(),
    });
    assert!(app.ready());
    assert_draft(&app);
    assert_eq!(app.documents[0].interrupted_save, unknown_save);
    app.save();
    assert!(app.error.as_deref().unwrap().contains("interrupted save"));
    assert_eq!(operations(&root), ["hello", "list"]);
    begin_disconnect(&mut app);
    finish_disconnect(&mut app);
    normal_agent_receipt(&root, second_agent);
    assert_draft(&app);
    persist(&mut app);
    assert_eq!(operations(&root), ["hello", "list"]);
    assert_eq!(revision(&fs::read(root.join(PATH)).unwrap()), source_hash);
    drop(app);
    let store = Store::open(&recovery_root).unwrap();
    let saved = store.read(&record_id(&workspace, PATH).unwrap()).unwrap();
    assert_eq!(saved.text, DRAFT);
    assert_eq!(saved.base_text, SOURCE);
    assert_eq!(saved.base_revision.as_deref(), Some(source_hash.as_str()));
    drop(store);
    temp.close().unwrap();
    println!("explicit_disconnect_acceptance normal_agent_connections=2 normal_agents_reaped=2 local_closes_verified=2 trust_off=true no_write_run_or_replay=true source_hash_unchanged=true draft_full_selection_undo_retained=true recovery_verified=true unknown_save_retained=true offline_history_retained=true stale_events_ignored=true fixture_removed=true");
}

#[test]
#[ignore = "requires CEDAR_EXPLICIT_DISCONNECT_PEER_BIN; bounded process acceptance"]
fn explicit_disconnect_cancels_stalled_read_and_suppresses_queued_list() {
    let peer = binary("CEDAR_EXPLICIT_DISCONNECT_PEER_BIN");
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("stalled read");
    let replacement = temp.path().join("replacement");
    fixture(&root, "stalled_read");
    fixture(&replacement, "capability_peer");
    let source_hash = revision(&fs::read(root.join(PATH)).unwrap());
    let mut app = CedarApp::empty();
    start(&mut app, &root, &peer, form(SYNTHETIC_ROOT), None);
    retain_unknown_draft(&mut app);
    let unknown_save = app.documents[0].interrupted_save.clone();
    let old_generation = app.generation;
    app.open("fixture.txt".into(), None);
    let request: cedar_protocol::Request =
        serde_json::from_slice(&wait_file(&root, "ready-3")).unwrap();
    assert!(matches!(request.op, Operation::Read { .. }));
    let read_id = *app.pending.keys().next().unwrap();
    app.list("queued-only".into());
    assert_eq!(app.pending.len(), 2);
    let began = Instant::now();
    begin_disconnect(&mut app);
    let cancelled = app
        .result_rx
        .recv_timeout(CANCEL_BOUND)
        .expect("Read cancellation timed out");
    assert!(began.elapsed() < CANCEL_BOUND);
    assert!(matches!(&cancelled, WorkerEvent::Response(reply)
        if reply.generation == old_generation && reply.id == read_id && !reply.connected
        && reply.result.as_ref().is_err_and(|error| error.starts_with("transport_cancelled:"))));
    app.apply_worker_event(cancelled);
    assert!(app.state == ConnectionState::Disconnecting);
    late_read(&mut app, old_generation, read_id);
    assert!(app.state == ConnectionState::Disconnecting);
    assert_draft(&app);
    finish_disconnect(&mut app);
    wait_file(&root, "eof");
    assert_eq!(operations(&root), ["hello", "list", "read"]);
    start(&mut app, &replacement, &peer, form(SYNTHETIC_ROOT), None);
    late_read(&mut app, old_generation, read_id);
    app.apply_worker_event(WorkerEvent::Closed {
        generation: old_generation,
        result: Err("transport_cleanup_unverified: stale".into()),
    });
    assert!(app.ready() && !app.unverified_local_close);
    assert_draft(&app);
    assert_eq!(app.documents[0].interrupted_save, unknown_save);
    app.save();
    assert!(app.error.as_deref().unwrap().contains("interrupted save"));
    assert_eq!(operations(&replacement), ["hello", "list"]);
    begin_disconnect(&mut app);
    finish_disconnect(&mut app);
    wait_file(&replacement, "eof");
    assert_eq!(operations(&replacement), ["hello", "list"]);
    assert_eq!(revision(&fs::read(root.join(PATH)).unwrap()), source_hash);
    assert_eq!(
        revision(&fs::read(replacement.join(PATH)).unwrap()),
        source_hash
    );
    drop(app);
    temp.close().unwrap();
    println!("explicit_disconnect_stalled_read_acceptance cancellation_bounded=true queued_list_suppressed=true late_read_not_adopted=true stale_close_ignored=true local_closes_verified=2 unknown_save_retained=true no_write_run_or_replay=true source_hashes_unchanged=true fixture_removed=true");
}

#[cfg(target_os = "linux")]
struct ProcessObservation {
    pid: u32,
    start_ticks: u64,
}

#[cfg(target_os = "linux")]
impl ProcessObservation {
    fn stat(pid: u32) -> std::io::Result<(u64, char)> {
        let stat = fs::read_to_string(format!("/proc/{pid}/stat"))?;
        let fields: Vec<_> = stat
            .rsplit_once(')')
            .expect("invalid process stat")
            .1
            .split_whitespace()
            .collect();
        Ok((
            fields[19].parse().unwrap(),
            fields[0].chars().next().unwrap(),
        ))
    }

    fn open(pid: u32) -> Self {
        let (start_ticks, state) = Self::stat(pid).unwrap();
        assert!(pid != 0 && start_ticks != 0 && !matches!(state, 'Z' | 'X' | 'x'));
        Self { pid, start_ticks }
    }

    fn exited(&self) -> bool {
        match Self::stat(self.pid) {
            Ok((start_ticks, state)) => {
                start_ticks != self.start_ticks || matches!(state, 'Z' | 'X' | 'x')
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
            Err(_) => panic!("exact fixture exit observation failed"),
        }
    }

    fn wait_exit(&self) {
        let deadline = Instant::now() + WAIT;
        while !self.exited() {
            assert!(
                Instant::now() < deadline,
                "same fixture did not exit within cleanup bound"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        // Start ticks plus a previous live observation establish the original
        // peer's exit only. This is not an owned wait/reaping receipt.
    }
}

#[cfg(windows)]
struct ProcessObservation {
    handle: std::os::windows::io::OwnedHandle,
    pid: u32,
    created: u64,
}

#[cfg(windows)]
impl ProcessObservation {
    fn open(pid: u32) -> Self {
        use std::os::windows::io::FromRawHandle;
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
        };
        // SAFETY: This PID is published by our just-started private fixture.
        // Retain observation-only rights before initiating any local close.
        let raw = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                0,
                pid,
            )
        };
        assert!(!raw.is_null(), "fixture observation handle unavailable");
        let mut observed = Self {
            // SAFETY: OpenProcess returned a newly owned native handle.
            handle: unsafe { std::os::windows::io::OwnedHandle::from_raw_handle(raw) },
            pid,
            created: 0,
        };
        observed.created = observed.creation_time();
        assert!(observed.created != 0 && !observed.exited());
        observed
    }

    fn creation_time(&self) -> u64 {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::{
            Foundation::FILETIME,
            System::Threading::{GetProcessId, GetProcessTimes},
        };
        let mut times = [FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        }; 4];
        // SAFETY: Retained query-only handle and four distinct writable outputs.
        unsafe {
            assert_eq!(GetProcessId(self.handle.as_raw_handle()), self.pid);
            assert_ne!(
                GetProcessTimes(
                    self.handle.as_raw_handle(),
                    &mut times[0],
                    &mut times[1],
                    &mut times[2],
                    &mut times[3]
                ),
                0
            );
        }
        (u64::from(times[0].dwHighDateTime) << 32) | u64::from(times[0].dwLowDateTime)
    }

    fn exited(&self) -> bool {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::{
            Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT},
            System::Threading::WaitForSingleObject,
        };
        assert_eq!(self.creation_time(), self.created);
        // SAFETY: Retained SYNCHRONIZE handle, zero-time observation only.
        match unsafe { WaitForSingleObject(self.handle.as_raw_handle(), 0) } {
            WAIT_OBJECT_0 => true,
            WAIT_TIMEOUT => false,
            _ => panic!("fixture wait observation failed"),
        }
    }

    fn wait_exit(&self) {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::{
            Foundation::WAIT_OBJECT_0,
            System::Threading::{GetExitCodeProcess, WaitForSingleObject},
        };
        assert_eq!(self.creation_time(), self.created);
        // SAFETY: Same retained handle, bounded wait, no PID-based signalling.
        assert_eq!(
            unsafe { WaitForSingleObject(self.handle.as_raw_handle(), WAIT.as_millis() as u32) },
            WAIT_OBJECT_0
        );
        let mut code = 0;
        // SAFETY: The signaled handle proves termination independently of code.
        assert_ne!(
            unsafe { GetExitCodeProcess(self.handle.as_raw_handle(), &mut code) },
            0
        );
        assert!(self.exited());
    }
}

#[cfg(any(target_os = "linux", windows))]
#[test]
#[ignore = "requires CEDAR_EXPLICIT_DISCONNECT_PEER_BIN; bounded process acceptance"]
fn explicit_disconnect_close_timeout_keeps_warning_after_replacement() {
    let peer = binary("CEDAR_EXPLICIT_DISCONNECT_PEER_BIN");
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("unresponsive close");
    let replacement = temp.path().join("replacement");
    fixture(&root, "disconnect_close_timeout");
    fixture(&replacement, "capability_peer");
    let source_hash = revision(&fs::read(root.join(PATH)).unwrap());
    let mut app = CedarApp::empty();
    start(
        &mut app,
        &root,
        &peer,
        form(SYNTHETIC_ROOT),
        Some(Duration::from_millis(20)),
    );
    wait_file(&root, "close-timeout-ready");
    let observed = ProcessObservation::open(pid(&root, "started"));
    retain_unknown_draft(&mut app);
    let old_generation = app.generation;
    let began = Instant::now();
    begin_disconnect(&mut app);
    let event = app
        .result_rx
        .recv_timeout(CANCEL_BOUND)
        .expect("close observation did not finish");
    assert!(began.elapsed() < CANCEL_BOUND);
    assert!(
        matches!(&event, WorkerEvent::Closed { generation, result: Err(error) }
        if *generation == old_generation && error.starts_with("transport_close:"))
    );
    assert!(
        !observed.exited(),
        "timeout must precede the unchanged reaper's cleanup"
    );
    app.apply_worker_event(event);
    assert!(app.state == ConnectionState::CleanupUnverified);
    assert!(app.unverified_local_close);
    assert_eq!(app.error.as_deref(), Some(disconnect::UNVERIFIED));
    assert_draft(&app);
    // A fabricated/duplicate success cannot erase an already selected outcome.
    app.apply_worker_event(WorkerEvent::Closed {
        generation: old_generation,
        result: Ok(()),
    });
    assert!(app.state == ConnectionState::CleanupUnverified && app.unverified_local_close);
    assert_eq!(operations(&root), ["hello", "list"]);

    start(&mut app, &replacement, &peer, form(SYNTHETIC_ROOT), None);
    assert!(app.ready() && app.unverified_local_close);
    for result in [Ok(()), Err("transport_close: stale timeout".into())] {
        app.apply_worker_event(WorkerEvent::Closed {
            generation: old_generation,
            result,
        });
        assert!(app.ready() && app.unverified_local_close);
    }
    late_read(&mut app, old_generation, 3);
    assert_draft(&app);
    app.save();
    assert!(app.error.as_deref().unwrap().contains("interrupted save"));
    assert_eq!(operations(&replacement), ["hello", "list"]);
    begin_disconnect(&mut app);
    finish_disconnect(&mut app);
    assert!(app.unverified_local_close);
    wait_file(&replacement, "eof");
    observed.wait_exit();
    assert_eq!(operations(&root), ["hello", "list"]);
    assert_eq!(operations(&replacement), ["hello", "list"]);
    assert_eq!(revision(&fs::read(root.join(PATH)).unwrap()), source_hash);
    assert_eq!(
        revision(&fs::read(replacement.join(PATH)).unwrap()),
        source_hash
    );
    // This app-level timeout deliberately has no successful cleanup receipt.
    // The Client transport test separately waits on its authoritative reaper.
    assert!(!root.join("eof").exists() && !root.join("safety-expired").exists());
    #[cfg(windows)]
    drop(observed);
    drop(app);
    temp.close().unwrap();
    println!("explicit_disconnect_timeout_acceptance close_observation_ms=20 no_premature_success=true cleanup_verified=false timed_out_peer_exit_observed=true warning_retained_after_replacement=true replacement_close_verified=true unknown_save_retained=true no_write_run_or_replay=true source_hashes_unchanged=true fixture_removed=true");
}
