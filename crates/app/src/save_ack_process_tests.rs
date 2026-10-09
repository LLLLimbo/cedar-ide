//! Trust-off Client/frontend acceptance for Written acknowledgement integrity.
//! Every workspace and recovery record is generated and owned by this test.
//! The marked nonshipping peer controls only the first acknowledgement; the
//! release-agent cases use CEDAR_SAVE_ACK_AGENT_BIN without fault injection.
use super::*;
use cedar_client::Client;
use cedar_recovery::{record_id, Store, WorkspaceIdentity};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const WAIT: Duration = Duration::from_secs(5);
const MARKER: &str = ".cedar-interrupted-save-validation";
const MARKER_TEXT: &[u8] = b"cedar-interrupted-save-validation-v1\nsynthetic-data-only\n";
const ACK_MODE: &str = ".cedar-synthetic-save-ack-mode";
const OPERATIONS: &str = ".cedar-interrupted-save-operations";
const COMMITTED: &str = ".cedar-interrupted-save-committed";
const FILE: &str = "draft.txt";
const A: &str = "A: original generated text\n";
const B: &str = "B: submitted generated 草稿 🐻\n";
const C: &str = "C: newer generated text é\n";
const FAULTS: &[&str] = &[
    "synthetic-missing",
    "synthetic-empty",
    "synthetic-oversized",
    "synthetic-noncanonical",
    "synthetic-wrong-digest",
];

fn binary(variable: &str) -> PathBuf {
    let path =
        PathBuf::from(std::env::var_os(variable).expect("explicit acceptance binary required"));
    assert!(path.is_absolute() && path.is_file());
    path
}

fn revision(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

#[derive(Debug, PartialEq, Eq)]
struct EditorSnapshot {
    document: u64,
    path: String,
    text: String,
    edit_version: u64,
    cursor: (usize, usize),
    jump_to: Option<usize>,
    scroll_to: Option<usize>,
    has_cjk: bool,
    // CCursor equality omits affinity, so compare every field explicitly.
    selection: (usize, bool, usize, bool),
}

struct Harness {
    client: Option<Client>,
    app: CedarApp,
    commands: Receiver<Command>,
    root: tempfile::TempDir,
    agent: PathBuf,
    controlled: bool,
    operations: Vec<&'static str>,
    connections: usize,
    reaped: usize,
}

impl Drop for Harness {
    fn drop(&mut self) {
        // Assertion failures still close our own child. Successful acceptance
        // calls reap explicitly and checks its result and connection count.
        if let Some(client) = self.client.take() {
            let _ = client.close_and_wait(WAIT);
        }
    }
}

impl Harness {
    fn new(mode: Option<&str>) -> Self {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join(FILE), A).unwrap();
        if let Some(mode) = mode {
            fs::write(root.path().join(MARKER), MARKER_TEXT).unwrap();
            fs::write(root.path().join(ACK_MODE), format!("{mode}\n")).unwrap();
        }
        let agent = binary(if mode.is_some() {
            "CEDAR_INTERRUPTED_SAVE_AGENT_BIN"
        } else {
            "CEDAR_SAVE_ACK_AGENT_BIN"
        });
        let (_, commands) = Worker::recording();
        let mut harness = Self {
            client: None,
            app: CedarApp::empty(),
            commands,
            root,
            agent,
            controlled: mode.is_some(),
            operations: Vec::new(),
            connections: 0,
            reaped: 0,
        };
        harness.connect();
        harness.app.open(FILE.into(), None);
        let event = harness.read_event();
        harness.app.apply_event(event);
        assert_eq!(harness.app.documents.len(), 1);
        assert_eq!(harness.app.documents[0].text, A);
        assert_eq!(harness.app.documents[0].saved_text, A);
        assert_eq!(harness.app.documents[0].revision, Some(revision(A)));
        harness.assert_operations(&["Hello", "List", "Read"]);
        harness
    }

    fn reap(&mut self) {
        if let Some(client) = self.client.take() {
            client
                .close_and_wait(WAIT)
                .expect("owned child must be reaped");
            self.reaped += 1;
        }
    }

    fn connect(&mut self) {
        self.reap();
        let client = Client::spawn_agent(&self.agent, self.root.path(), false).unwrap();
        self.connections += 1;
        self.operations.push("Hello");
        let (worker, commands) = Worker::recording();
        self.commands = commands;
        self.app.worker = Some(worker);
        self.app.generation += 1;
        self.app.state = ConnectionState::Connecting;
        self.app.connecting_form = Some(ConnectForm {
            local_root: self.root.path().to_string_lossy().into_owned(),
            ssh: false,
            allow_run: false,
            ..Default::default()
        });
        self.app.apply_event(Event {
            generation: self.app.generation,
            id: 0,
            connected: true,
            result: Ok(client.handshake().clone()),
        });
        self.client = Some(client);
        let command = self.next();
        assert!(matches!(&command.op, Operation::List { path } if path.is_empty()));
        let event = self.exchange(command);
        self.app.apply_event(event);
        assert!(self.app.ready());
        assert!(!self.app.execution_trusted());
        self.assert_idle();
    }

    fn next(&self) -> Command {
        self.commands.try_recv().expect("expected frontend request")
    }

    fn assert_idle(&self) {
        assert!(matches!(
            self.commands.try_recv(),
            Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected)
        ));
    }

    fn exchange(&mut self, command: Command) -> Event {
        self.operations.push(match &command.op {
            Operation::List { .. } => "List",
            Operation::Read { .. } => "Read",
            Operation::Write { .. } => "Write",
            _ => panic!("unexpected operation in save acknowledgement acceptance"),
        });
        let client = self.client.as_mut().unwrap();
        let result = client.request(command.op);
        Event {
            generation: self.app.generation,
            id: command.id,
            connected: client.is_connected(),
            result,
        }
    }

    fn read_event(&mut self) -> Event {
        let command = self.next();
        assert!(matches!(&command.op, Operation::Read { path } if path == FILE));
        self.exchange(command)
    }

    fn deliver_saved_root_listing(&mut self) {
        // A valid Save refreshes the existing default flat Explorer exactly
        // once. Invalid acknowledgements must never reach this success path.
        assert!(!self.controlled);
        assert_eq!(self.app.explorer.mode, explorer_tree::Mode::Flat);
        assert!(self.app.directory.is_empty());
        let command = self.next();
        assert!(matches!(&command.op, Operation::List { path } if path.is_empty()));
        assert!(matches!(
            self.app.pending.get(&command.id),
            Some(Job::List { path }) if path.is_empty()
        ));
        self.assert_idle();
        let event = self.exchange(command);
        assert!(event.connected);
        assert!(matches!(&event.result, Ok(Payload::Entries { .. })));
        self.app.apply_event(event);
        self.assert_idle();
    }

    fn edit(&mut self, text: &str) {
        editor_state::commit(
            &self.app.editor_ctx,
            &mut self.app.documents[0],
            text.into(),
            4,
        );
    }

    fn select(&mut self) {
        let doc = &mut self.app.documents[0];
        let mut state = editor_state::load(&self.app.editor_ctx, doc);
        state.cursor.set_char_range(Some(egui::text::CCursorRange {
            primary: egui::text::CCursor {
                index: 19,
                prefer_next_row: true,
            },
            secondary: egui::text::CCursor {
                index: 2,
                prefer_next_row: false,
            },
        }));
        state.store(&self.app.editor_ctx, egui::Id::new(("editor", doc.id)));
        doc.cursor = model::cursor_location(&doc.text, 19);
        doc.jump_to = Some(1);
        doc.scroll_to = Some(19);
    }

    fn snapshot(&self) -> EditorSnapshot {
        assert_eq!(self.app.documents.len(), 1);
        let doc = &self.app.documents[0];
        assert_eq!(self.app.active_document, Some(doc.id));
        let selection =
            egui::TextEdit::load_state(&self.app.editor_ctx, egui::Id::new(("editor", doc.id)))
                .unwrap()
                .cursor
                .char_range()
                .unwrap();
        EditorSnapshot {
            document: doc.id,
            path: doc.path.clone(),
            text: doc.text.clone(),
            edit_version: doc.edit_version,
            cursor: doc.cursor,
            jump_to: doc.jump_to,
            scroll_to: doc.scroll_to,
            has_cjk: doc.has_cjk,
            selection: (
                selection.primary.index,
                selection.primary.prefer_next_row,
                selection.secondary.index,
                selection.secondary.prefer_next_row,
            ),
        }
    }

    fn save_event(&mut self) -> (Event, interrupted_save::InterruptedSave) {
        self.app.save();
        let command = self.next();
        assert!(matches!(
            &command.op,
            Operation::Write { path, text, expected_revision }
                if path == FILE && text == B && expected_revision == &Some(revision(A))
        ));
        let submission = match self.app.pending.get(&command.id).unwrap() {
            Job::Save {
                submission: Some(token),
                ..
            } => token.clone(),
            _ => panic!("the real Save must capture its bounded submission identity"),
        };
        assert!(self.app.documents[0].saving);
        self.app.save();
        self.assert_idle();
        (self.exchange(command), submission)
    }

    fn assert_unresolved(&self, text: &str, token: &interrupted_save::InterruptedSave) {
        let doc = &self.app.documents[0];
        assert_eq!(doc.text, text);
        assert_eq!(doc.saved_text, A);
        assert_eq!(doc.revision, Some(revision(A)));
        assert_eq!(doc.interrupted_save.as_ref(), Some(token));
        assert!(doc.dirty());
        assert!(!doc.saving);
        assert!(!self.app.execution_trusted());
    }

    fn assert_operations(&self, expected: &[&str]) {
        assert_eq!(self.operations, expected);
        if self.controlled {
            let log = fs::read_to_string(self.root.path().join(OPERATIONS)).unwrap();
            assert_eq!(log.lines().collect::<Vec<_>>(), expected);
        }
        self.assert_idle();
    }

    fn check(&mut self, token: &interrupted_save::InterruptedSave) {
        self.select();
        let before = self.snapshot();
        let text = self.app.documents[0].text.clone();
        self.app.check_interrupted_save();
        self.app.check_interrupted_save();
        let first = self.read_event();
        self.assert_idle();
        self.app.apply_event(first);
        self.app.finish_interrupted_save_check();
        self.assert_unresolved(&text, token);
        assert_eq!(self.snapshot(), before);
        self.app.check_interrupted_save();
        let second = self.read_event();
        self.assert_idle();
        self.app.apply_event(second);
        // Receipt alone stages the second snapshot; adoption is explicit.
        self.assert_unresolved(&text, token);
        assert_eq!(self.snapshot(), before);
        self.app.check_interrupted_save();
        self.assert_idle();
        self.app.finish_interrupted_save_check();
        assert_eq!(self.snapshot(), before);
        assert!(self.app.documents[0].interrupted_save.is_none());
        self.app.check_interrupted_save();
        self.assert_idle();
    }

    fn history(&mut self, redo: bool) {
        let ctx = self.app.editor_ctx.clone();
        let id = self.app.documents[0].id;
        let _ = ctx.run(
            egui::RawInput {
                events: vec![egui::Event::Key {
                    key: egui::Key::Z,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: if redo {
                        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT
                    } else {
                        egui::Modifiers::COMMAND
                    },
                }],
                ..Default::default()
            },
            |ctx| {
                ctx.memory_mut(|memory| memory.request_focus(egui::Id::new(("editor", id))));
                editor_state::history_shortcut(ctx, &mut self.app.documents[0]);
            },
        );
    }

    fn history_roundtrip(&mut self, newer: bool) {
        let base = self.app.documents[0].saved_text.clone();
        let revision = self.app.documents[0].revision.clone();
        let token = self.app.documents[0].interrupted_save.clone();
        self.history(false);
        assert_eq!(self.app.documents[0].text, if newer { B } else { A });
        if newer {
            self.history(false);
            assert_eq!(self.app.documents[0].text, A);
            self.history(true);
            assert_eq!(self.app.documents[0].text, B);
        }
        self.history(true);
        assert_eq!(self.app.documents[0].text, if newer { C } else { B });
        assert_eq!(self.app.documents[0].saved_text, base);
        assert_eq!(self.app.documents[0].revision, revision);
        assert_eq!(self.app.documents[0].interrupted_save, token);
        self.assert_idle();
    }

    fn start_recovery(&mut self, path: &Path) {
        self.app
            .recovery
            .start(Ok(path.into()), &self.app.editor_ctx);
        recovery_wait(&mut self.app, |app| app.recovery.initialized);
    }

    fn persist(&mut self) {
        let identity = self.app.recovery_workspace().unwrap();
        self.app.recovery_tick(&self.app.editor_ctx.clone());
        self.app.recovery.flush();
        recovery_wait(&mut self.app, |app| {
            app.recovery.protected(&identity, &app.documents[0])
        });
    }
}

fn recovery_wait(app: &mut CedarApp, done: impl Fn(&CedarApp) -> bool) {
    let deadline = Instant::now() + WAIT;
    loop {
        app.recovery_tick(&app.editor_ctx.clone());
        assert!(app.recovery.error.is_none(), "{:?}", app.recovery.error);
        if done(app) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "recovery acknowledgement timed out"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn recovery_bytes(path: &Path, identity: &WorkspaceIdentity) -> Vec<u8> {
    fs::read(path.join(format!("{}.draft", record_id(identity, FILE).unwrap()))).unwrap()
}

fn request_receipt(h: &Harness, expected_check_reads: usize) -> serde_json::Value {
    let count = |operation| {
        h.operations
            .iter()
            .filter(|name| **name == operation)
            .count()
    };
    let hellos = count("Hello");
    let lists = count("List");
    let reads = count("Read");
    let writes = count("Write");
    assert_eq!(writes, 1);
    assert_eq!(reads, 1 + expected_check_reads);
    assert_eq!(hellos, h.connections);
    let success_refreshes = lists.checked_sub(h.connections).unwrap();
    assert_eq!(success_refreshes, usize::from(!h.controlled));
    assert_eq!(h.reaped, h.connections);
    assert!(h.client.is_none());
    h.assert_idle();
    serde_json::json!({
        "controlled_peer": h.controlled,
        "requests": h.operations.len(),
        "hello": hellos,
        "list": lists,
        "connection_lists": h.connections,
        "successful_ack_refresh_lists": success_refreshes,
        "read": reads,
        "initial_reads": 1,
        "verification_reads": reads - 1,
        "write": writes,
        "connections": h.connections,
        "reaped": h.reaped,
    })
}

fn assert_fault(event: &Event, mode: &str) {
    if mode == "synthetic-missing" {
        assert!(
            !event.connected,
            "the real decoder must close the malformed transport"
        );
        let error = event.result.as_ref().unwrap_err();
        assert!(error.starts_with("transport_read:"), "{error}");
        assert!(error.contains("revision"), "{error}");
    } else {
        assert!(
            event.connected,
            "valid framing must keep the actual Client connected"
        );
        let Ok(Payload::Written { revision: ack }) = &event.result else {
            panic!("expected the controlled Written payload")
        };
        match mode {
            "synthetic-empty" | "synthetic-not-committed" => assert!(ack.is_empty()),
            "synthetic-oversized" => assert_eq!(ack.len(), 65),
            "synthetic-noncanonical" => assert_eq!(*ack, revision(B).to_ascii_uppercase()),
            "synthetic-wrong-digest" => {
                assert_eq!(ack.len(), 64);
                assert!(ack
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
                assert_ne!(*ack, revision(B));
            }
            _ => panic!("unknown test mode"),
        }
    }
}

#[test]
#[ignore = "requires CEDAR_INTERRUPTED_SAVE_AGENT_BIN; explicit verify/CI acceptance"]
fn synthetic_ack_modes_require_exact_marker_and_bounded_control() {
    let mut rejected = 0;
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join(FILE), A).unwrap();
    fs::write(root.path().join(ACK_MODE), b"synthetic-empty\n").unwrap();
    let agent = binary("CEDAR_INTERRUPTED_SAVE_AGENT_BIN");
    // The mode alone grants no access, and trust-on is always rejected.
    assert!(Client::spawn_agent(&agent, root.path(), false).is_err());
    rejected += 1;
    fs::write(root.path().join(MARKER), MARKER_TEXT).unwrap();
    assert!(Client::spawn_agent(&agent, root.path(), true).is_err());
    rejected += 1;
    for mode in ["unknown\n".to_owned(), "x".repeat(65)] {
        fs::write(root.path().join(ACK_MODE), mode).unwrap();
        assert!(Client::spawn_agent(&agent, root.path(), false).is_err());
        rejected += 1;
    }
    assert!(!root.path().join(OPERATIONS).exists());
    assert!(!root.path().join(COMMITTED).exists());
    assert_eq!(fs::read_to_string(root.path().join(FILE)).unwrap(), A);
    let workspace_path = root.path().to_owned();
    drop(root);
    assert!(!workspace_path.exists());
    // Rejected construction returns no Client whose cleanup we could verify.
    // Do not claim an observed reaping result for these rejected handshakes.
    println!(
        "save_ack_acceptance {}",
        serde_json::json!({
            "test": "synthetic_ack_modes_require_exact_marker_and_bounded_control",
            "cases": rejected,
            "controlled_peer": true,
            "accepted_requests": 0,
            "write": 0,
            "verification_reads": 0,
            "successful_connections": 0,
            "observed_reaped_clients": 0,
            "reaping_observed": false,
            "guard_rejections_verified": true,
            "source_preserved": true,
            "fixture_removed": true,
        })
    );
}

#[test]
#[ignore = "requires CEDAR_INTERRUPTED_SAVE_AGENT_BIN; explicit verify/CI acceptance"]
fn committed_faulty_acks_retain_draft_selection_history_recovery_until_two_reads() {
    let mut receipts = Vec::new();
    for &mode in FAULTS {
        for newer in [false, true] {
            let mut h = Harness::new(Some(mode));
            let recovery_root = tempfile::tempdir().unwrap();
            let store_path = recovery_root.path().join("private-recovery");
            h.start_recovery(&store_path);
            let identity = h.app.recovery_workspace().unwrap();
            h.edit(B);
            h.persist();
            let (event, token) = h.save_event();
            assert_fault(&event, mode);
            assert_eq!(fs::read_to_string(h.root.path().join(FILE)).unwrap(), B);
            assert_eq!(fs::read(h.root.path().join(COMMITTED)).unwrap(), b"1\n");
            if newer {
                h.edit(C);
                h.persist();
            }
            h.select();
            let before = h.snapshot();
            let protected = recovery_bytes(&store_path, &identity);
            h.app.apply_event(event);
            h.assert_unresolved(if newer { C } else { B }, &token);
            assert_eq!(h.snapshot(), before);
            h.persist();
            assert_eq!(recovery_bytes(&store_path, &identity), protected);
            h.app.save();
            h.app.save();
            h.assert_operations(&["Hello", "List", "Read", "Write"]);
            h.history_roundtrip(newer);
            h.assert_unresolved(if newer { C } else { B }, &token);

            let mut expected = vec!["Hello", "List", "Read", "Write"];
            if mode == "synthetic-missing" {
                let before = h.snapshot();
                h.connect();
                assert_eq!(h.snapshot(), before);
                assert_eq!(h.reaped, 1);
                expected.extend(["Hello", "List"]);
            }
            assert_eq!(h.app.recovery_workspace(), Some(identity.clone()));
            h.assert_unresolved(if newer { C } else { B }, &token);
            h.assert_operations(&expected);
            h.check(&token);
            expected.extend(["Read", "Read"]);
            h.assert_operations(&expected);
            assert_eq!(h.app.documents[0].saved_text, B);
            assert_eq!(h.app.documents[0].revision, Some(revision(B)));
            assert_eq!(h.app.documents[0].dirty(), newer);
            h.history_roundtrip(newer);
            if newer {
                h.persist();
            } else {
                h.app.recovery.flush();
                recovery_wait(&mut h.app, |app| app.recovery.removals_finished());
            }
            assert_eq!(fs::read_to_string(h.root.path().join(FILE)).unwrap(), B);
            h.assert_operations(&expected);
            h.reap();
            assert_eq!(h.reaped, h.connections);
            receipts.push(request_receipt(&h, 2));
            let workspace_path = h.root.path().to_owned();
            drop(h);
            assert!(!workspace_path.exists());
            let store = Store::open(&store_path).unwrap();
            if newer {
                let draft = store.read(&record_id(&identity, FILE).unwrap()).unwrap();
                assert_eq!(draft.text, C);
                assert_eq!(draft.base_text, B);
                assert_eq!(draft.base_revision, Some(revision(B)));
            } else {
                assert!(store.list().unwrap().drafts.is_empty());
            }
        }
    }
    println!(
        "save_ack_acceptance {}",
        serde_json::json!({
            "test": "committed_faulty_acks_retain_draft_selection_history_recovery_until_two_reads",
            "cases": receipts.len(),
            "controlled_peer": true,
            "request_receipts": receipts,
            "draft_preserved": true,
            "baseline_preserved_until_verification": true,
            "captured_token_retained": true,
            "full_selection_preserved": true,
            "undo_redo_preserved": true,
            "owned_recovery_preserved_until_verification": true,
            "reconciled_recovery_verified": true,
            "source_matches_submitted": true,
            "one_write_per_case": true,
            "two_verification_reads_per_case": true,
            "owned_children_reaped": true,
            "fixtures_removed": true,
        })
    );
}

#[test]
#[ignore = "requires CEDAR_INTERRUPTED_SAVE_AGENT_BIN; explicit verify/CI acceptance"]
fn noncommitted_faulty_ack_checks_original_bytes_without_replaying_save() {
    let mut receipts = Vec::new();
    for newer in [false, true] {
        let mut h = Harness::new(Some("synthetic-not-committed"));
        let recovery_root = tempfile::tempdir().unwrap();
        let store_path = recovery_root.path().join("private-recovery");
        h.start_recovery(&store_path);
        let identity = h.app.recovery_workspace().unwrap();
        h.edit(B);
        let (event, token) = h.save_event();
        assert_fault(&event, "synthetic-not-committed");
        if newer {
            h.edit(C);
        }
        h.persist();
        h.select();
        let before = h.snapshot();
        let protected = recovery_bytes(&store_path, &identity);
        h.app.apply_event(event);
        h.assert_unresolved(if newer { C } else { B }, &token);
        assert_eq!(h.snapshot(), before);
        h.persist();
        assert_eq!(recovery_bytes(&store_path, &identity), protected);
        h.app.save();
        h.assert_operations(&["Hello", "List", "Read", "Write"]);
        assert!(!h.root.path().join(COMMITTED).exists());
        assert_eq!(fs::read_to_string(h.root.path().join(FILE)).unwrap(), A);
        h.check(&token);
        assert_eq!(h.app.documents[0].saved_text, A);
        assert_eq!(h.app.documents[0].revision, Some(revision(A)));
        assert!(h.app.documents[0].dirty());
        h.history_roundtrip(newer);
        h.persist();
        h.assert_operations(&["Hello", "List", "Read", "Write", "Read", "Read"]);
        assert_eq!(fs::read_to_string(h.root.path().join(FILE)).unwrap(), A);
        h.reap();
        assert_eq!(h.reaped, 1);
        receipts.push(request_receipt(&h, 2));
        let workspace_path = h.root.path().to_owned();
        drop(h);
        assert!(!workspace_path.exists());
        let draft = Store::open(&store_path)
            .unwrap()
            .read(&record_id(&identity, FILE).unwrap())
            .unwrap();
        assert_eq!(draft.text, if newer { C } else { B });
        assert_eq!(draft.base_text, A);
        assert_eq!(draft.base_revision, Some(revision(A)));
    }
    println!(
        "save_ack_acceptance {}",
        serde_json::json!({
            "test": "noncommitted_faulty_ack_checks_original_bytes_without_replaying_save",
            "cases": receipts.len(),
            "controlled_peer": true,
            "request_receipts": receipts,
            "draft_preserved": true,
            "original_baseline_preserved": true,
            "full_selection_preserved": true,
            "undo_redo_preserved": true,
            "owned_recovery_preserved": true,
            "source_unchanged": true,
            "one_write_per_case": true,
            "two_verification_reads_per_case": true,
            "owned_children_reaped": true,
            "fixtures_removed": true,
        })
    );
}

#[test]
#[ignore = "requires CEDAR_INTERRUPTED_SAVE_AGENT_BIN; explicit verify/CI acceptance"]
fn typing_during_fault_reconciliation_keeps_token_and_requires_fresh_explicit_check() {
    let mut h = Harness::new(Some("synthetic-empty"));
    h.edit(B);
    let (event, token) = h.save_event();
    assert_fault(&event, "synthetic-empty");
    h.app.apply_event(event);
    h.assert_unresolved(B, &token);
    h.app.check_interrupted_save();
    let first = h.read_event();
    h.edit(C);
    h.select();
    let before = h.snapshot();
    h.app.apply_event(first);
    h.app.finish_interrupted_save_check();
    h.assert_unresolved(C, &token);
    assert_eq!(h.snapshot(), before);
    h.assert_operations(&["Hello", "List", "Read", "Write", "Read"]);
    h.check(&token);
    assert_eq!(h.app.documents[0].text, C);
    assert_eq!(h.app.documents[0].saved_text, B);
    assert!(h.app.documents[0].dirty());
    h.history_roundtrip(true);
    h.assert_operations(&["Hello", "List", "Read", "Write", "Read", "Read", "Read"]);
    assert_eq!(fs::read_to_string(h.root.path().join(FILE)).unwrap(), B);
    h.reap();
    assert_eq!(h.reaped, 1);
    let receipt = request_receipt(&h, 3);
    let workspace_path = h.root.path().to_owned();
    drop(h);
    assert!(!workspace_path.exists());
    println!(
        "save_ack_acceptance {}",
        serde_json::json!({
            "test": "typing_during_fault_reconciliation_keeps_token_and_requires_fresh_explicit_check",
            "cases": 1,
            "controlled_peer": true,
            "request_receipts": [receipt],
            "interrupted_check_reads": 1,
            "successful_check_reads": 2,
            "newer_draft_preserved": true,
            "baseline_preserved_until_fresh_verification": true,
            "captured_token_retained": true,
            "full_selection_preserved": true,
            "undo_redo_preserved": true,
            "recovery_exercised": false,
            "source_matches_submitted": true,
            "one_write_per_case": true,
            "owned_children_reaped": true,
            "fixtures_removed": true,
        })
    );
}

#[test]
#[ignore = "requires CEDAR_SAVE_ACK_AGENT_BIN; explicit verify/CI acceptance"]
fn release_agent_valid_ack_adopts_only_submitted_snapshot_and_owned_recovery() {
    let mut receipts = Vec::new();
    for newer in [false, true] {
        let mut h = Harness::new(None);
        let recovery_root = tempfile::tempdir().unwrap();
        let store_path = recovery_root.path().join("private-recovery");
        h.start_recovery(&store_path);
        let identity = h.app.recovery_workspace().unwrap();
        h.edit(B);
        h.persist();
        let (event, _) = h.save_event();
        assert!(event.connected);
        assert!(
            matches!(&event.result, Ok(Payload::Written { revision: ack }) if ack == &revision(B))
        );
        if newer {
            h.edit(C);
            h.persist();
        }
        h.select();
        let before = h.snapshot();
        h.app.apply_event(event);
        assert_eq!(h.snapshot(), before);
        assert_eq!(h.app.documents[0].saved_text, B);
        assert_eq!(h.app.documents[0].revision, Some(revision(B)));
        assert_eq!(h.app.documents[0].dirty(), newer);
        assert!(!h.app.documents[0].saving);
        assert!(h.app.documents[0].interrupted_save.is_none());
        h.deliver_saved_root_listing();
        assert_eq!(h.snapshot(), before);
        h.app.check_interrupted_save();
        h.history_roundtrip(newer);
        if newer {
            h.persist();
        } else {
            h.app.recovery.flush();
            recovery_wait(&mut h.app, |app| app.recovery.removals_finished());
        }
        h.assert_operations(&["Hello", "List", "Read", "Write", "List"]);
        assert!(!h.app.execution_trusted());
        assert_eq!(fs::read_to_string(h.root.path().join(FILE)).unwrap(), B);
        h.reap();
        assert_eq!(h.reaped, 1);
        receipts.push(request_receipt(&h, 0));
        let workspace_path = h.root.path().to_owned();
        drop(h);
        assert!(!workspace_path.exists());
        let store = Store::open(&store_path).unwrap();
        if newer {
            let draft = store.read(&record_id(&identity, FILE).unwrap()).unwrap();
            assert_eq!(draft.text, C);
            assert_eq!(draft.base_text, B);
            assert_eq!(draft.base_revision, Some(revision(B)));
        } else {
            assert!(store.list().unwrap().drafts.is_empty());
        }
    }
    println!(
        "save_ack_acceptance {}",
        serde_json::json!({
            "test": "release_agent_valid_ack_adopts_only_submitted_snapshot_and_owned_recovery",
            "cases": receipts.len(),
            "controlled_peer": false,
            "request_receipts": receipts,
            "draft_preserved": true,
            "only_submitted_baseline_adopted": true,
            "full_selection_preserved": true,
            "undo_redo_preserved": true,
            "owned_recovery_verified": true,
            "source_matches_submitted": true,
            "one_write_per_case": true,
            "no_verification_reads": true,
            "one_successful_ack_root_list_per_case": true,
            "owned_children_reaped": true,
            "fixtures_removed": true,
        })
    );
}
