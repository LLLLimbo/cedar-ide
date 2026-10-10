//! Normal shipping-agent saves remain usable when an owned recovery path fails.
//! Two generated fixtures, one connection and ten protocol requests each. No
//! descriptor probe, permission change, execution trust, SSH, or language server.
use super::*;
use cedar_client::Client;
use sha2::{Digest, Sha256};
use std::{
    fs,
    time::{Duration, Instant},
};

const WAIT: Duration = Duration::from_secs(5);
const QUIET_FRAMES: usize = 32;
const PATHS: [&str; 2] = ["first.txt", "second.txt"];
const BASE: [&str; 2] = [
    "first: original generated text\n",
    "second: original text\n",
];
const EXPLICIT: &str = "first: explicit generated save 草稿 🐻\n";
const BATCH: [&str; 2] = [
    "first: generated Save All text Ω 草稿\n",
    "second: generated Save All text é 🐻\n",
];
const BLOCKER: &[u8] = b"owned regular file; never recovery storage\n";
const EXPECTED: [&str; 10] = [
    "Hello", "List", "Read", "Read", "Write", "List", "Write", "List", "Write", "List",
];

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
    // Include both endpoints and affinities; CCursor equality omits affinity.
    selection: (usize, bool, usize, bool),
}

struct Harness {
    client: Option<Client>,
    app: CedarApp,
    commands: Receiver<Command>,
    root: tempfile::TempDir,
    recovery_root: tempfile::TempDir,
    blocker: PathBuf,
    unavailable_error: String,
    operations: Vec<&'static str>,
    writes: [usize; 2],
    acknowledgements: usize,
    quiet_passes: usize,
}

impl Drop for Harness {
    fn drop(&mut self) {
        // A failed assertion still closes only the child owned by this fixture.
        // The success path separately verifies close_and_wait before its receipt.
        if let Some(client) = self.client.take() {
            let _ = client.close_and_wait(WAIT);
        }
    }
}

impl Harness {
    fn new(child_of_file: bool) -> Self {
        let agent = PathBuf::from(
            std::env::var_os("CEDAR_UNAVAILABLE_RECOVERY_AGENT_BIN")
                .expect("explicit normal-agent acceptance binary required"),
        );
        assert!(agent.is_absolute() && agent.is_file());
        let root = tempfile::tempdir().unwrap();
        for (path, text) in PATHS.into_iter().zip(BASE) {
            fs::write(root.path().join(path), text).unwrap();
        }
        let recovery_root = tempfile::tempdir().unwrap();
        let blocker = recovery_root.path().join("owned-file");
        fs::write(&blocker, BLOCKER).unwrap();
        let store = if child_of_file {
            blocker.join("unavailable-store")
        } else {
            blocker.clone()
        };
        let client = Client::spawn_agent(&agent, root.path(), false).unwrap();
        let (worker, commands) = Worker::recording();
        let mut app = CedarApp::empty();
        app.worker = Some(worker);
        app.generation = 1;
        app.state = ConnectionState::Connecting;
        app.connecting_form = Some(ConnectForm {
            local_root: root.path().to_string_lossy().into_owned(),
            ssh: false,
            allow_run: false,
            ..Default::default()
        });
        app.apply_event(Event {
            generation: app.generation,
            id: 0,
            connected: true,
            result: Ok(client.handshake().clone()),
        });
        let mut h = Self {
            client: Some(client),
            app,
            commands,
            root,
            recovery_root,
            blocker,
            unavailable_error: String::new(),
            operations: vec!["Hello"],
            writes: [0; 2],
            acknowledgements: 0,
            quiet_passes: 0,
        };
        h.list();
        assert!(h.app.ready());
        for (index, path) in PATHS.into_iter().enumerate() {
            h.app.open(path.into(), None);
            let command = h.next();
            assert!(matches!(&command.op, Operation::Read { path } if path == PATHS[index]));
            h.idle();
            let event = h.exchange(command);
            assert!(event.connected);
            assert!(matches!(
                &event.result,
                Ok(Payload::File { path, text, revision: actual })
                    if path == PATHS[index] && text == BASE[index] && actual == &revision(BASE[index])
            ));
            h.app.apply_event(event);
            let doc = &h.app.documents[index];
            assert_eq!(doc.text, BASE[index]);
            assert_eq!(doc.saved_text, BASE[index]);
            assert_eq!(doc.revision, Some(revision(BASE[index])));
        }
        assert_eq!(h.app.documents.len(), 2);
        h.app.recovery.start(Ok(store), &h.app.editor_ctx);
        let deadline = Instant::now() + WAIT;
        loop {
            h.app.recovery_tick(&h.app.editor_ctx.clone());
            if let Some(error) = &h.app.recovery.error {
                h.unavailable_error = error.clone();
                break;
            }
            assert!(
                Instant::now() < deadline,
                "initial recovery failure timed out"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        // Failure precedes every edit: no draft write was ever invoked here.
        h.assert_unavailable();
        assert_eq!(h.operations.as_slice(), &EXPECTED[..4]);
        h.idle();
        h
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
        let operation = match &command.op {
            Operation::List { path } if path.is_empty() => "List",
            Operation::Read { path } if PATHS.contains(&path.as_str()) => "Read",
            Operation::Write { path, .. } if PATHS.contains(&path.as_str()) => "Write",
            _ => panic!("only bounded List, Read, and conditional Write are permitted"),
        };
        assert_eq!(EXPECTED.get(self.operations.len()), Some(&operation));
        self.operations.push(operation);
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

    fn assert_unavailable(&self) {
        assert!(!self.app.recovery.loading);
        self.assert_unavailable_state();
    }

    fn assert_unavailable_state(&self) {
        assert!(!self.app.recovery.initialized);
        assert_eq!(
            self.app.recovery.error.as_ref(),
            Some(&self.unavailable_error)
        );
        // No queued operation, possible copy/accepted write, or successful
        // removal may be invented for a store that failed before any edit.
        assert_eq!(self.app.recovery.test_snapshot(), (0, 0, 0));
        assert!(self.app.recovery.drafts.is_empty());
        let identity = self.app.recovery_workspace().unwrap();
        for doc in &self.app.documents {
            assert!(!self.app.recovery.protected(&identity, doc));
            assert_eq!(
                self.app.recovery.status(Some(&identity), Some(doc)),
                ("Recovery needs attention", false)
            );
        }
        assert!(!self.app.execution_trusted() && !self.app.language.running);
        assert_eq!(fs::read(&self.blocker).unwrap(), BLOCKER);
        assert_eq!(fs::read_dir(self.recovery_root.path()).unwrap().count(), 1);
    }

    fn quiet(&mut self) {
        let before = self.snapshot();
        let active = self.app.active_document;
        let operations = self.operations.clone();
        // One shared deadline covers this entire fixed frame cohort; waiting
        // for an asynchronous listing never renews the acceptance clock.
        let deadline = Instant::now() + WAIT;
        for _ in 0..QUIET_FRAMES {
            self.app.recovery.refresh(false);
            assert!(self.app.recovery.loading);
            self.app.recovery.flush();
            self.assert_unavailable_state();
            loop {
                assert!(
                    Instant::now() < deadline,
                    "unavailable recovery refresh timed out"
                );
                self.app.recovery_tick(&self.app.editor_ctx.clone());
                self.assert_unavailable_state();
                assert_eq!(self.snapshot(), before);
                assert_eq!(self.app.active_document, active);
                if !self.app.recovery.loading {
                    // Recovery clears loading only for the requested listing
                    // epoch. A stale listing cannot satisfy this settled check.
                    break;
                }
                std::thread::yield_now();
            }
            self.assert_unavailable();
        }
        assert_eq!(self.snapshot(), before);
        assert_eq!(self.app.active_document, active);
        assert_eq!(self.operations, operations);
        self.quiet_passes += 1;
        self.idle();
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

    fn history_roundtrip(&mut self, batched: bool) {
        for (index, original) in BASE.into_iter().enumerate() {
            let doc = &self.app.documents[index];
            let saved = (doc.saved_text.clone(), doc.revision.clone());
            if batched && index == 0 {
                self.history(index, false);
                assert_eq!(self.app.documents[index].text, EXPLICIT);
            }
            self.history(index, false);
            assert_eq!(self.app.documents[index].text, original);
            self.history(index, true);
            assert_eq!(
                self.app.documents[index].text,
                if index == 0 { EXPLICIT } else { BATCH[index] }
            );
            if batched && index == 0 {
                self.history(index, true);
                assert_eq!(self.app.documents[index].text, BATCH[index]);
            }
            let doc = &self.app.documents[index];
            assert_eq!((doc.saved_text.clone(), doc.revision.clone()), saved);
            assert!(!doc.saving && !doc.save_outcome_unknown());
        }
        self.idle();
    }

    fn acknowledge(&mut self, index: usize, text: &str, base: &str, batch: bool) {
        let command = self.next();
        assert!(matches!(
            &command.op,
            Operation::Write { path, text: submitted, expected_revision }
                if path == PATHS[index] && submitted == text && expected_revision == &Some(revision(base))
        ));
        assert!(matches!(
            self.app.pending.get(&command.id),
            Some(Job::Save { document, snapshot, submission: Some(_) })
                if *document == self.app.documents[index].id && snapshot == text
        ));
        self.writes[index] += 1;
        assert!(self.app.documents[index].saving);
        // Repeated Save/Save All clicks cannot duplicate the pending request.
        if batch {
            self.app.queue_save_all();
            self.app.finish_save_all_frame(&self.app.editor_ctx.clone());
        } else {
            self.app.save();
        }
        self.idle();
        let event = self.exchange(command);
        assert!(event.connected);
        assert!(
            matches!(&event.result, Ok(Payload::Written { revision: ack }) if ack == &revision(text))
        );
        let before = self.snapshot();
        let active = self.app.active_document;
        self.app.apply_event(event);
        assert_eq!(self.snapshot(), before);
        assert_eq!(self.app.active_document, active);
        let doc = &self.app.documents[index];
        assert_eq!(doc.saved_text, text);
        assert_eq!(doc.revision, Some(revision(text)));
        assert!(!doc.dirty() && !doc.saving && !doc.save_outcome_unknown());
        assert_eq!(
            fs::read(self.root.path().join(PATHS[index])).unwrap(),
            text.as_bytes()
        );
        self.acknowledgements += 1;
        self.list();
        assert_eq!(self.snapshot(), before);
        self.assert_unavailable();
        if batch {
            self.app.finish_save_all_frame(&self.app.editor_ctx.clone());
        }
    }

    fn finish(mut self, case: &str) -> serde_json::Value {
        assert_eq!(self.operations, EXPECTED);
        assert_eq!(self.writes, [2, 1]);
        assert_eq!(self.acknowledgements, 3);
        assert_eq!(self.quiet_passes, 4);
        assert!(!self.app.save_all_busy());
        assert!(self
            .app
            .save_all_message()
            .unwrap()
            .contains("2 acknowledged · 0 failed · 0 unknown · 0 unattempted"));
        for (index, text) in BATCH.into_iter().enumerate() {
            let doc = &self.app.documents[index];
            assert_eq!(doc.text, text);
            assert_eq!(doc.saved_text, text);
            assert_eq!(doc.revision, Some(revision(text)));
            assert!(!doc.dirty() && !doc.saving && !doc.save_outcome_unknown());
            assert_eq!(
                fs::read(self.root.path().join(PATHS[index])).unwrap(),
                text.as_bytes()
            );
        }
        self.assert_unavailable();
        self.idle();
        self.client
            .take()
            .unwrap()
            .close_and_wait(WAIT)
            .expect("owned normal agent must actually exit and be reaped");
        let root = self.root.path().to_owned();
        let recovery_root = self.recovery_root.path().to_owned();
        drop(self);
        assert!(!root.exists() && !recovery_root.exists());
        serde_json::json!({
            "case": case, "controlled_peer": false, "execution_trusted": false,
            "requests": 10, "hello": 1, "list": 4, "read": 2, "write": 3,
            "initial_reads": 2, "verification_reads": 0, "per_file_writes": [2, 1],
            "explicit_save_acknowledged": 1, "save_all_acknowledged": 2,
            "successful_ack_refresh_lists": 3, "connections": 1, "reaped": 1,
            "quiet_frames": 4 * QUIET_FRAMES, "full_selections_preserved": true,
            "undo_redo_preserved": true, "source_bytes_verified": true,
            "conditional_revisions_verified": true, "recovery_protected": false,
            "recovery_error_preserved": true, "owned_blocker_preserved": true,
            "queued_recovery_operations": 0, "possible_recovery_copies": 0,
            "acknowledged_recovery_removals": 0,
            "run": 0, "language": 0, "fixtures_removed": true,
        })
    }
}

#[test]
#[ignore = "requires CEDAR_UNAVAILABLE_RECOVERY_AGENT_BIN; bounded normal-agent acceptance"]
fn normal_agent_save_and_save_all_preserve_editor_when_recovery_is_unavailable() {
    let mut receipts = Vec::new();
    for (case, child_of_file) in [("regular_file", false), ("child_beneath_file", true)] {
        let mut h = Harness::new(child_of_file);
        h.edit(0, EXPLICIT);
        h.edit(1, BATCH[1]);
        h.select_all();
        h.quiet();
        h.history_roundtrip(false);
        h.select_all();
        h.app.active_document = Some(h.app.documents[0].id);
        h.app.save();
        h.acknowledge(0, EXPLICIT, BASE[0], false);
        assert_eq!(
            fs::read(h.root.path().join(PATHS[1])).unwrap(),
            BASE[1].as_bytes()
        );
        assert_eq!(h.app.documents[1].saved_text, BASE[1]);
        assert_eq!(h.app.documents[1].revision, Some(revision(BASE[1])));
        h.quiet();
        h.edit(0, BATCH[0]);
        h.select_all();
        h.quiet();
        h.app.queue_save_all();
        h.idle();
        h.app.finish_save_all_frame(&h.app.editor_ctx.clone());
        assert!(h.app.save_all_busy());
        h.acknowledge(0, BATCH[0], EXPLICIT, true);
        h.acknowledge(1, BATCH[1], BASE[1], true);
        h.history_roundtrip(true);
        h.select_all();
        h.quiet();
        receipts.push(h.finish(case));
    }
    assert_eq!(receipts.len(), 2);
    println!(
        "unavailable_recovery_acceptance {}",
        serde_json::json!({
            "schema_version": 1, "tests": 1, "cases": 2, "requests": 20,
            "connections": 2, "reaped": 2, "cases_verified": receipts,
        })
    );
}
