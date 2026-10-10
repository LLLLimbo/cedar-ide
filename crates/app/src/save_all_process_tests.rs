//! Explicit Save All through the real stdio Client with execution trust off.
//! Workspaces and recovery stores contain only generated, owned test data.
//! Maven hook state is covered separately by frontend tests; no language server
//! or command is started by this acceptance gate.
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
const MODE: &str = ".cedar-synthetic-save-all-mode";
const OPERATIONS: &str = ".cedar-interrupted-save-operations";
const COMMITTED: &str = ".cedar-interrupted-save-committed";
const PATHS: [&str; 3] = ["first.txt", "pom.xml", "third.txt"];
const BASE: [&str; 3] = [
    "first: original generated text\n",
    "<project>original generated POM</project>\n",
    "third: original generated text\n",
];
const SUBMITTED: [&str; 3] = [
    "first: submitted generated 草稿 🐻\n",
    "<project>submitted generated POM 草稿 🐻</project>\n",
    "third: submitted generated text é\n",
];
const NEWER: &str = "first: newer generated draft Ω 草稿 🐻\n";
const EXTERNAL: &str = "<project>independently changed generated POM</project>\n";

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
    // CCursor equality omits affinity, so record both complete endpoints.
    selection: (usize, bool, usize, bool),
}

struct Harness {
    client: Option<Client>,
    app: CedarApp,
    commands: Receiver<Command>,
    root: tempfile::TempDir,
    recovery_root: Option<tempfile::TempDir>,
    store_path: PathBuf,
    agent: PathBuf,
    controlled: bool,
    operations: Vec<&'static str>,
    writes: [usize; 3],
    acknowledgements: usize,
    connections: usize,
    reaped: usize,
}

impl Drop for Harness {
    fn drop(&mut self) {
        if let Some(client) = self.client.take() {
            let _ = client.close_and_wait(WAIT);
        }
    }
}

impl Harness {
    fn new(mode: Option<&str>) -> Self {
        let root = tempfile::tempdir().unwrap();
        for (path, text) in PATHS.into_iter().zip(BASE) {
            fs::write(root.path().join(path), text).unwrap();
        }
        if let Some(mode) = mode {
            fs::write(root.path().join(MARKER), MARKER_TEXT).unwrap();
            fs::write(root.path().join(MODE), format!("{mode}\n")).unwrap();
        }
        let agent = binary(if mode.is_some() {
            "CEDAR_INTERRUPTED_SAVE_AGENT_BIN"
        } else {
            "CEDAR_SAVE_ALL_AGENT_BIN"
        });
        let recovery_root = tempfile::tempdir().unwrap();
        let store_path = recovery_root.path().join("private-recovery");
        let (_, commands) = Worker::recording();
        let mut h = Self {
            client: None,
            app: CedarApp::empty(),
            commands,
            root,
            recovery_root: Some(recovery_root),
            store_path,
            agent,
            controlled: mode.is_some(),
            operations: Vec::new(),
            writes: [0; 3],
            acknowledgements: 0,
            connections: 0,
            reaped: 0,
        };
        h.connect();
        for (index, path) in PATHS.into_iter().enumerate() {
            h.app.open(path.into(), None);
            let event = h.read(index);
            h.app.apply_event(event);
            let doc = &h.app.documents[index];
            assert_eq!(doc.path, path);
            assert_eq!(doc.text, BASE[index]);
            assert_eq!(doc.saved_text, BASE[index]);
            assert_eq!(doc.revision, Some(revision(BASE[index])));
        }
        assert_eq!(h.app.documents.len(), 3);
        h.app
            .recovery
            .start(Ok(h.store_path.clone()), &h.app.editor_ctx);
        recovery_wait(&mut h.app, |app| app.recovery.initialized);
        for (index, text) in SUBMITTED.into_iter().enumerate() {
            h.edit(index, text);
        }
        h.select_all();
        h.persist();
        h.assert_operations(&["Hello", "List", "Read", "Read", "Read"]);
        h
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
        self.list();
        assert!(self.app.ready());
        self.assert_trust_off();
    }

    fn assert_trust_off(&self) {
        assert!(!self.app.execution_trusted());
        assert!(!self.app.language.running);
    }

    fn next(&self) -> Command {
        self.commands.try_recv().expect("expected frontend request")
    }

    fn idle(&self) {
        assert!(matches!(
            self.commands.try_recv(),
            Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected)
        ));
    }

    fn exchange(&mut self, command: Command) -> Event {
        assert!(
            self.operations.len() < 16,
            "fixed acceptance request budget"
        );
        self.operations.push(match &command.op {
            Operation::List { path } if path.is_empty() => "List",
            Operation::Read { path } if PATHS.contains(&path.as_str()) => "Read",
            Operation::Write {
                path,
                text,
                expected_revision,
            } => {
                let index = PATHS
                    .iter()
                    .position(|candidate| *candidate == path.as_str())
                    .unwrap();
                assert_eq!(text, SUBMITTED[index]);
                assert_eq!(expected_revision, &Some(revision(BASE[index])));
                self.writes[index] += 1;
                assert_eq!(self.writes[index], 1, "no replay or duplicate Write");
                "Write"
            }
            _ => panic!("Save All acceptance forbids Run, Language, and unrelated requests"),
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

    fn list(&mut self) {
        let command = self.next();
        assert!(matches!(&command.op, Operation::List { path } if path.is_empty()));
        self.idle();
        let event = self.exchange(command);
        assert!(event.connected && matches!(&event.result, Ok(Payload::Entries { .. })));
        self.app.apply_event(event);
        self.idle();
    }

    fn read(&mut self, index: usize) -> Event {
        let command = self.next();
        assert!(matches!(&command.op, Operation::Read { path } if path == PATHS[index]));
        self.idle();
        self.exchange(command)
    }

    fn write(&mut self, index: usize) -> (Event, interrupted_save::InterruptedSave) {
        let command = self.next();
        assert!(matches!(&command.op, Operation::Write { path, .. } if path == PATHS[index]));
        let Some(Job::Save {
            document,
            snapshot,
            submission: Some(token),
        }) = self.app.pending.get(&command.id)
        else {
            panic!("Save All must use the ordinary captured Save job")
        };
        assert_eq!(*document, self.app.documents[index].id);
        assert_eq!(snapshot, SUBMITTED[index]);
        let token = token.clone();
        assert!(self.app.documents[index].saving);
        self.idle();
        // Additional clicks while this request is in flight must not extend or
        // replace the captured cohort, or emit another Write.
        self.app.queue_save_all();
        self.finish_frame();
        self.idle();
        (self.exchange(command), token)
    }

    fn finish_frame(&mut self) {
        self.app.finish_save_all_frame(&self.app.editor_ctx.clone());
    }

    fn begin(&mut self) {
        self.app.queue_save_all();
        self.idle();
        self.finish_frame();
        assert!(self.app.save_all_busy());
    }

    fn acknowledge(&mut self, index: usize, event: Event) {
        assert!(event.connected);
        assert!(
            matches!(&event.result, Ok(Payload::Written { revision: ack }) if ack == &revision(SUBMITTED[index]))
        );
        let before = self.snapshot();
        let active = self.app.active_document;
        self.app.apply_event(event);
        assert_eq!(self.snapshot(), before);
        assert_eq!(self.app.active_document, active);
        let doc = &self.app.documents[index];
        assert_eq!(doc.saved_text, SUBMITTED[index]);
        assert_eq!(doc.revision, Some(revision(SUBMITTED[index])));
        assert!(!doc.saving && doc.interrupted_save.is_none());
        self.acknowledgements += 1;
        // Keep the ordinary flat Explorer success hook, including its one List.
        self.list();
        assert_eq!(self.snapshot(), before);
        self.finish_frame();
    }

    fn edit(&mut self, index: usize, text: &str) {
        editor_state::commit(
            &self.app.editor_ctx,
            &mut self.app.documents[index],
            text.into(),
            4,
        );
    }

    fn select_all(&mut self) {
        for doc in &mut self.app.documents {
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
    }

    fn snapshot(&self) -> Vec<EditorSnapshot> {
        self.app
            .documents
            .iter()
            .map(|doc| {
                let selection = egui::TextEdit::load_state(
                    &self.app.editor_ctx,
                    egui::Id::new(("editor", doc.id)),
                )
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
            })
            .collect()
    }

    fn persist(&mut self) {
        let identity = self.app.recovery_workspace().unwrap();
        self.app.recovery_tick(&self.app.editor_ctx.clone());
        self.app.recovery.flush();
        recovery_wait(&mut self.app, |app| {
            app.recovery.removals_finished()
                && app
                    .documents
                    .iter()
                    .filter(|doc| doc.dirty())
                    .all(|doc| app.recovery.protected(&identity, doc))
        });
    }

    fn recovery_bytes(&self, index: usize) -> Vec<u8> {
        let identity = self.app.recovery_workspace().unwrap();
        fs::read(self.store_path.join(format!(
            "{}.draft",
            record_id(&identity, PATHS[index]).unwrap()
        )))
        .unwrap()
    }

    fn history(&mut self, index: usize, redo: bool) {
        let ctx = self.app.editor_ctx.clone();
        let id = self.app.documents[index].id;
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
                editor_state::history_shortcut(ctx, &mut self.app.documents[index]);
            },
        );
    }

    fn history_roundtrip(&mut self) {
        for (index, original) in BASE.into_iter().enumerate() {
            let doc = &self.app.documents[index];
            let (base, revision, token) = (
                doc.saved_text.clone(),
                doc.revision.clone(),
                doc.interrupted_save.clone(),
            );
            if index == 0 {
                self.history(index, false);
                assert_eq!(self.app.documents[index].text, SUBMITTED[index]);
            }
            self.history(index, false);
            assert_eq!(self.app.documents[index].text, original);
            self.history(index, true);
            assert_eq!(self.app.documents[index].text, SUBMITTED[index]);
            if index == 0 {
                self.history(index, true);
                assert_eq!(self.app.documents[index].text, NEWER);
            }
            let doc = &self.app.documents[index];
            assert_eq!(doc.saved_text, base);
            assert_eq!(doc.revision, revision);
            assert_eq!(doc.interrupted_save, token);
        }
        self.idle();
    }

    fn assert_operations(&self, expected: &[&str]) {
        assert_eq!(self.operations, expected);
        if self.controlled {
            let log = fs::read_to_string(self.root.path().join(OPERATIONS)).unwrap();
            assert_eq!(log.lines().collect::<Vec<_>>(), expected);
        }
        self.idle();
    }

    fn finish(
        mut self,
        case: &str,
        writes: [usize; 3],
        verification_reads: usize,
        disk: [&str; 3],
    ) {
        assert!(!self.app.save_all_busy());
        self.assert_trust_off();
        self.persist();
        self.reap();
        assert_eq!(self.writes, writes);
        assert_eq!(self.connections, self.reaped);
        let count = |name| {
            self.operations
                .iter()
                .filter(|operation| **operation == name)
                .count()
        };
        assert_eq!(count("Hello"), self.connections);
        assert_eq!(count("List"), self.connections + self.acknowledgements);
        assert_eq!(count("Read"), 3 + verification_reads);
        assert_eq!(count("Write"), writes.iter().sum::<usize>());
        assert_eq!(
            self.operations.len(),
            self.connections * 2
                + self.acknowledgements
                + 3
                + verification_reads
                + writes.iter().sum::<usize>()
        );
        self.assert_operations(&self.operations.clone());
        for (path, text) in PATHS.into_iter().zip(disk) {
            assert_eq!(
                fs::read_to_string(self.root.path().join(path)).unwrap(),
                text
            );
        }
        let identity = self.app.recovery_workspace().unwrap();
        let expected_drafts: Vec<_> = self
            .app
            .documents
            .iter()
            .map(|doc| {
                (
                    doc.path.clone(),
                    doc.dirty(),
                    doc.text.clone(),
                    doc.saved_text.clone(),
                    doc.revision.clone(),
                )
            })
            .collect();
        let failed = usize::from(case == "normal_second_conflict");
        let unknown = usize::from(self.controlled);
        let unattempted = writes.iter().filter(|count| **count == 0).count();
        let summary = self.app.save_all_message().unwrap();
        assert!(summary.contains(&format!(
            "{} acknowledged · {failed} failed · {unknown} unknown · {unattempted} unattempted",
            self.acknowledgements,
        )));
        let receipt = serde_json::json!({
            "test": case, "controlled_peer": self.controlled, "execution_trusted": false,
            "requests": self.operations.len(), "hello": count("Hello"), "list": count("List"),
            "initial_reads": 3, "verification_reads": verification_reads, "read": count("Read"),
            "write": count("Write"), "per_file_writes": self.writes,
            "successful_ack_refresh_lists": self.acknowledgements,
            "acknowledged": self.acknowledgements, "failed": failed, "unknown": unknown,
            "unattempted": unattempted, "batch_summary_verified": true,
            "connections": self.connections, "reaped": self.reaped,
            "conditional_revisions_verified": true, "no_replay": true, "run": 0, "language": 0,
            "drafts_preserved": true, "full_selections_preserved": true, "undo_redo_preserved": true,
            "owned_recovery_verified": true, "source_bytes_verified": true,
            "maven_hook_state_exercised": false, "real_language_started": false,
            "owned_children_reaped": true, "fixtures_removed": true,
        });
        let root = self.root.path().to_owned();
        let recovery_root = self.recovery_root.take().unwrap();
        let recovery_path = recovery_root.path().to_owned();
        let store_path = self.store_path.clone();
        drop(self);
        assert!(!root.exists());
        verify_recovery(&store_path, &identity, &expected_drafts);
        drop(recovery_root);
        assert!(!recovery_path.exists());
        println!("save_all_acceptance {receipt}");
    }
}

type ExpectedDraft = (String, bool, String, String, Option<String>);

fn verify_recovery(path: &Path, identity: &WorkspaceIdentity, expected: &[ExpectedDraft]) {
    let store = Store::open(path).unwrap();
    let listing = store.list().unwrap();
    assert!(listing.issues.is_empty());
    assert_eq!(
        listing.drafts.len(),
        expected.iter().filter(|entry| entry.1).count()
    );
    for (path, dirty, text, base, revision) in expected {
        let id = record_id(identity, path).unwrap();
        if *dirty {
            let draft = store.read(&id).unwrap();
            assert_eq!(&draft.text, text);
            assert_eq!(&draft.base_text, base);
            assert_eq!(&draft.base_revision, revision);
        } else {
            assert!(!listing.drafts.iter().any(|draft| draft.id == id));
        }
    }
}

fn recovery_wait(app: &mut CedarApp, done: impl Fn(&CedarApp) -> bool) {
    let deadline = Instant::now() + WAIT;
    loop {
        app.recovery_tick(&app.editor_ctx.clone());
        assert!(
            app.recovery.error.is_none(),
            "recovery must acknowledge generated drafts"
        );
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

fn first_success(h: &mut Harness) {
    h.begin();
    let (event, _) = h.write(0);
    if h.controlled {
        assert!(
            !h.root.path().join(COMMITTED).exists(),
            "the first Write must not fault"
        );
    }
    h.edit(0, NEWER);
    h.select_all();
    h.persist();
    h.acknowledge(0, event);
    assert_eq!(h.app.documents[0].text, NEWER);
    assert!(h.app.documents[0].dirty());
}

#[test]
#[ignore = "requires CEDAR_SAVE_ALL_AGENT_BIN; explicit default-agent acceptance"]
fn normal_agent_saves_three_conditional_snapshots_and_preserves_newer_draft() {
    let mut h = Harness::new(None);
    first_success(&mut h);
    for index in 1..3 {
        let (event, _) = h.write(index);
        h.acknowledge(index, event);
    }
    assert!(!h.app.save_all_busy());
    h.assert_operations(&[
        "Hello", "List", "Read", "Read", "Read", "Write", "List", "Write", "List", "Write", "List",
    ]);
    h.history_roundtrip();
    h.finish("normal_three_success", [1, 1, 1], 0, SUBMITTED);
}

#[test]
#[ignore = "requires CEDAR_SAVE_ALL_AGENT_BIN; explicit default-agent acceptance"]
fn normal_agent_second_revision_conflict_prevents_third_write() {
    let mut h = Harness::new(None);
    fs::write(h.root.path().join(PATHS[1]), EXTERNAL).unwrap();
    first_success(&mut h);
    let protected = [h.recovery_bytes(1), h.recovery_bytes(2)];
    let (event, _) = h.write(1);
    assert!(event.connected);
    assert!(event.result.as_ref().unwrap_err().starts_with("conflict:"));
    let before = h.snapshot();
    let active = h.app.active_document;
    h.app.apply_event(event);
    h.finish_frame();
    assert!(!h.app.save_all_busy());
    assert_eq!(h.snapshot(), before);
    assert_eq!(h.app.active_document, active);
    for (index, base) in BASE.iter().enumerate().skip(1) {
        let doc = &h.app.documents[index];
        assert_eq!(doc.saved_text, *base);
        assert_eq!(doc.revision, Some(revision(base)));
        assert!(doc.dirty() && !doc.saving && doc.interrupted_save.is_none());
    }
    h.persist();
    assert_eq!([h.recovery_bytes(1), h.recovery_bytes(2)], protected);
    h.assert_operations(&[
        "Hello", "List", "Read", "Read", "Read", "Write", "List", "Write",
    ]);
    h.history_roundtrip();
    h.finish(
        "normal_second_conflict",
        [1, 1, 0],
        0,
        [SUBMITTED[0], EXTERNAL, BASE[2]],
    );
}

#[test]
#[ignore = "requires CEDAR_INTERRUPTED_SAVE_AGENT_BIN; explicit controlled-peer acceptance"]
fn controlled_second_unknown_stops_cohort_and_reconciliation_does_not_resume_it() {
    for mode in ["second-wrong-digest", "second-lost-reply"] {
        let mut h = Harness::new(Some(mode));
        first_success(&mut h);
        let protected = [h.recovery_bytes(1), h.recovery_bytes(2)];
        let (event, token) = h.write(1);
        if mode == "second-lost-reply" {
            assert!(!event.connected && event.result.is_err());
        } else {
            assert!(event.connected);
            assert!(
                matches!(&event.result, Ok(Payload::Written { revision: ack }) if ack.len() == 64 && ack != &revision(SUBMITTED[1]))
            );
        }
        assert_eq!(fs::read(h.root.path().join(COMMITTED)).unwrap(), b"1\n");
        let before = h.snapshot();
        let active = h.app.active_document;
        h.app.apply_event(event);
        h.finish_frame();
        assert!(!h.app.save_all_busy());
        assert_eq!(h.snapshot(), before);
        assert_eq!(h.app.active_document, active);
        let second = &h.app.documents[1];
        assert_eq!(second.interrupted_save.as_ref(), Some(&token));
        assert_eq!(second.saved_text, BASE[1]);
        assert_eq!(second.revision, Some(revision(BASE[1])));
        assert!(second.dirty() && !second.saving);
        let third = &h.app.documents[2];
        assert_eq!(third.saved_text, BASE[2]);
        assert_eq!(third.revision, Some(revision(BASE[2])));
        assert!(third.dirty() && !third.saving && third.interrupted_save.is_none());
        h.persist();
        assert_eq!([h.recovery_bytes(1), h.recovery_bytes(2)], protected);
        h.assert_operations(&[
            "Hello", "List", "Read", "Read", "Read", "Write", "List", "Write",
        ]);
        assert_eq!(
            fs::read_to_string(h.root.path().join(PATHS[1])).unwrap(),
            SUBMITTED[1]
        );
        assert_eq!(
            fs::read_to_string(h.root.path().join(PATHS[2])).unwrap(),
            BASE[2]
        );
        h.history_roundtrip();
        h.select_all();
        let before = h.snapshot();
        if mode == "second-lost-reply" {
            h.connect();
            h.finish_frame();
            h.idle();
            assert_eq!(h.snapshot(), before);
        }
        h.app.active_document = Some(h.app.documents[1].id);
        h.app.check_interrupted_save();
        h.app.check_interrupted_save();
        let first = h.read(1);
        h.app.apply_event(first);
        h.app.finish_interrupted_save_check();
        assert_eq!(h.app.documents[1].interrupted_save.as_ref(), Some(&token));
        let second = h.read(1);
        h.app.apply_event(second);
        assert_eq!(h.app.documents[1].saved_text, BASE[1]);
        h.app.finish_interrupted_save_check();
        h.finish_frame();
        assert_eq!(h.snapshot(), before);
        assert!(h.app.documents[1].interrupted_save.is_none());
        assert_eq!(h.app.documents[1].saved_text, SUBMITTED[1]);
        assert_eq!(h.app.documents[1].revision, Some(revision(SUBMITTED[1])));
        assert!(!h.app.documents[1].dirty());
        h.idle();
        h.finish(mode, [1, 1, 0], 2, [SUBMITTED[0], SUBMITTED[1], BASE[2]]);
    }
}

#[test]
#[ignore = "requires CEDAR_INTERRUPTED_SAVE_AGENT_BIN; explicit controlled-profile guards"]
fn controlled_profile_requires_exact_marker_bounded_mode_and_fixed_paths() {
    let root = tempfile::tempdir().unwrap();
    let agent = binary("CEDAR_INTERRUPTED_SAVE_AGENT_BIN");
    let mut rejected = 0;
    fs::write(root.path().join(PATHS[0]), BASE[0]).unwrap();
    fs::write(root.path().join(MODE), b"second-wrong-digest\n").unwrap();
    assert!(Client::spawn_agent(&agent, root.path(), false).is_err());
    rejected += 1;
    fs::write(root.path().join(MARKER), b"incorrect generated marker\n").unwrap();
    assert!(Client::spawn_agent(&agent, root.path(), false).is_err());
    rejected += 1;
    fs::write(root.path().join(MARKER), MARKER_TEXT).unwrap();
    assert!(Client::spawn_agent(&agent, root.path(), true).is_err());
    rejected += 1;
    for mode in ["unknown\n".to_owned(), "x".repeat(65)] {
        fs::write(root.path().join(MODE), mode).unwrap();
        assert!(Client::spawn_agent(&agent, root.path(), false).is_err());
        rejected += 1;
    }
    fs::write(root.path().join(MODE), b"second-wrong-digest\n").unwrap();
    fs::write(
        root.path().join(".cedar-synthetic-save-ack-mode"),
        b"synthetic-empty\n",
    )
    .unwrap();
    assert!(Client::spawn_agent(&agent, root.path(), false).is_err());
    rejected += 1;
    fs::remove_file(root.path().join(".cedar-synthetic-save-ack-mode")).unwrap();
    // A directory is a portable nonregular-file probe on both OSes; no symlink
    // privilege or user-owned filesystem location is needed.
    for (name, contents) in [
        (MODE, b"second-wrong-digest\n".as_slice()),
        (MARKER, MARKER_TEXT),
    ] {
        fs::remove_file(root.path().join(name)).unwrap();
        fs::create_dir(root.path().join(name)).unwrap();
        assert!(Client::spawn_agent(&agent, root.path(), false).is_err());
        rejected += 1;
        fs::remove_dir(root.path().join(name)).unwrap();
        fs::write(root.path().join(name), contents).unwrap();
    }
    assert!(!root.path().join(OPERATIONS).exists());
    let mut client = Client::spawn_agent(&agent, root.path(), false).unwrap();
    for path in ["other.txt", "../outside.txt"] {
        let error = client
            .request(Operation::Write {
                path: path.into(),
                text: "generated refusal probe".into(),
                expected_revision: Some(revision(BASE[0])),
            })
            .unwrap_err();
        assert!(error.starts_with("fixture_operation_disabled:"));
    }
    let error = client
        .request(Operation::Write {
            path: PATHS[0].into(),
            text: SUBMITTED[0].into(),
            expected_revision: None,
        })
        .unwrap_err();
    assert!(error.starts_with("fixture_operation_disabled:"));
    client.close_and_wait(WAIT).unwrap();
    assert_eq!(
        fs::read_to_string(root.path().join(PATHS[0])).unwrap(),
        BASE[0]
    );
    assert!(!root.path().join("other.txt").exists());
    assert!(!root.path().join(COMMITTED).exists());
    let operations = fs::read_to_string(root.path().join(OPERATIONS)).unwrap();
    let operations: Vec<_> = operations.lines().collect();
    assert_eq!(operations, ["Hello", "Write", "Write", "Write"]);
    let path = root.path().to_owned();
    drop(root);
    assert!(!path.exists());
    println!(
        "save_all_acceptance {}",
        serde_json::json!({
            "test": "controlled_profile_guards", "controlled_peer": true,
            "handshake_rejections": rejected, "rejected_write_attempts": operations.len() - 1,
            "requests": operations.len(), "hello": 1,
            "write": operations.iter().filter(|operation| **operation == "Write").count(),
            "committed_writes": 0, "run": 0, "language": 0,
            "successful_connections": 1, "observed_reaped_clients": 1,
            "rejected_handshake_reaping_observed": false,
            "guard_rejections_verified": true, "source_preserved": true, "fixture_removed": true,
        })
    );
}
