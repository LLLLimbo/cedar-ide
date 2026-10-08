//! Real stdio Client + frontend Event acceptance. All disk/recovery inputs are
//! generated in fresh temporary roots; the fixture has no execution capability.
//! Run explicitly with CEDAR_INTERRUPTED_SAVE_AGENT_BIN set to the separately
//! built cedar-agent-interrupted-save-validation binary. These tests establish
//! current byte equivalence, never physical file identity or request provenance.
use super::*;
use cedar_client::Client;
use cedar_recovery::{record_id, Draft, Store, WorkspaceIdentity};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const MARKER: &str = ".cedar-interrupted-save-validation";
const MARKER_TEXT: &[u8] = b"cedar-interrupted-save-validation-v1\nsynthetic-data-only\n";
const OPERATIONS: &str = ".cedar-interrupted-save-operations";
const READ_MODE: &str = ".cedar-interrupted-save-read-mode";
const FILE: &str = "draft.txt";
const A: &str = "A: original generated text\n";
const B: &str = "B: submitted generated 草稿 🐻\n";
const C: &str = "C: newer generated text é\n";
const D: &str = "D: independently changed generated disk\n";
const INITIAL: &[&str] = &["Hello", "List", "Read", "Write", "Hello", "List"];

fn agent() -> PathBuf {
    let path = PathBuf::from(
        std::env::var_os("CEDAR_INTERRUPTED_SAVE_AGENT_BIN")
            .expect("set CEDAR_INTERRUPTED_SAVE_AGENT_BIN to the fixture binary"),
    );
    assert!(
        path.is_absolute(),
        "fixture binary must be an absolute path"
    );
    assert!(path.is_file(), "fixture binary must exist");
    path
}

struct Harness {
    // Close child processes before removing their generated workspace.
    client: Option<Client>,
    app: CedarApp,
    commands: Receiver<Command>,
    root: tempfile::TempDir,
    original_revision: String,
}

impl Drop for Harness {
    fn drop(&mut self) {
        if let Some(client) = self.client.take() {
            client.close_and_wait(Duration::from_secs(5)).unwrap();
        }
    }
}

impl Harness {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join(MARKER), MARKER_TEXT).unwrap();
        fs::write(root.path().join(FILE), A).unwrap();
        let (_, commands) = Worker::recording();
        let mut harness = Self {
            client: None,
            app: CedarApp::empty(),
            commands,
            root,
            original_revision: String::new(),
        };
        harness.connect();
        harness.app.open(FILE.into(), None);
        harness.deliver_next_read();
        harness.original_revision = harness.app.documents[0].revision.clone().unwrap();
        assert_eq!(harness.app.documents[0].text, A);
        harness.assert_operations(&["Hello", "List", "Read"]);
        harness
    }

    fn connect(&mut self) {
        if let Some(client) = self.client.take() {
            client.close_and_wait(Duration::from_secs(5)).unwrap();
        }
        let client = Client::spawn_agent(&agent(), self.root.path(), false).unwrap();
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

    fn deliver_next_read(&mut self) {
        let event = self.read_event();
        self.app.apply_event(event);
    }

    fn edit(&mut self, text: &str) {
        editor_state::commit(
            &self.app.editor_ctx,
            &mut self.app.documents[0],
            text.into(),
            4,
        );
    }

    fn lost_save_event(&mut self) -> Event {
        self.app.save();
        let command = self.next();
        assert!(matches!(
            &command.op,
            Operation::Write { path, text, expected_revision }
                if path == FILE && text == B
                    && expected_revision.as_deref() == Some(self.original_revision.as_str())
        ));
        let event = self.exchange(command);
        assert!(event.result.is_err(), "Written reply must be lost");
        assert!(!event.connected, "the fixture must actually exit");
        assert_eq!(fs::read_to_string(self.root.path().join(FILE)).unwrap(), B);
        assert_eq!(
            fs::read(self.root.path().join(".cedar-interrupted-save-committed")).unwrap(),
            b"1\n"
        );
        event
    }

    fn lose_save(&mut self, newer: bool) {
        self.edit(B);
        let event = self.lost_save_event();
        if newer {
            self.edit(C);
        }
        self.app.apply_event(event);
        self.assert_unresolved(if newer { C } else { B });
        self.assert_operations(&["Hello", "List", "Read", "Write"]);
        self.connect();
        self.assert_unresolved(if newer { C } else { B });
        self.assert_operations(INITIAL);
    }

    fn assert_unresolved(&self, text: &str) {
        let doc = &self.app.documents[0];
        assert_eq!(doc.text, text);
        assert_eq!(doc.saved_text, A);
        assert_eq!(
            doc.revision.as_deref(),
            Some(self.original_revision.as_str())
        );
        assert!(doc.dirty());
        assert!(doc.interrupted_save.is_some());
        assert!(!doc.saving);
    }

    fn assert_operations(&self, expected: &[&str]) {
        let log = fs::read_to_string(self.root.path().join(OPERATIONS)).unwrap();
        assert_eq!(log.lines().collect::<Vec<_>>(), expected);
    }

    fn assert_reads_since_reconnect(&self, count: usize) {
        let mut expected = INITIAL.to_vec();
        expected.extend(std::iter::repeat_n("Read", count));
        self.assert_operations(&expected);
    }

    fn check(&mut self) {
        self.app.check_interrupted_save();
        // Repeated clicks before either response cannot add another request.
        self.app.check_interrupted_save();
        let first = self.read_event();
        self.assert_idle();
        self.app.apply_event(first);
        self.app.check_interrupted_save();
        let second = self.read_event();
        let observed_revision = match &second.result {
            Ok(Payload::File { revision, .. }) => revision.clone(),
            _ => panic!("expected the actual second disk snapshot"),
        };
        self.assert_idle();
        self.app.apply_event(second);
        self.app.check_interrupted_save();
        self.assert_idle();
        self.app.finish_interrupted_save_check();
        assert_eq!(
            self.app.documents[0].revision.as_deref(),
            Some(observed_revision.as_str())
        );
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
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        app.recovery_tick(&app.editor_ctx.clone());
        assert!(app.recovery.error.is_none(), "{:?}", app.recovery.error);
        if done(app) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "recovery did not acknowledge its mutation"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
#[ignore = "requires CEDAR_INTERRUPTED_SAVE_AGENT_BIN; explicit verify/CI acceptance"]
fn fixture_requires_exact_marker_and_rejects_execution_trust() {
    let root = tempfile::tempdir().unwrap();
    for marker in [None, Some(b"wrong-marker\n".as_slice()), Some(MARKER_TEXT)] {
        if let Some(marker) = marker {
            fs::write(root.path().join(MARKER), marker).unwrap();
        }
        // Correctly marked roots still cannot opt into execution.
        let allow_run = marker == Some(MARKER_TEXT);
        assert!(Client::spawn_agent(&agent(), root.path(), allow_run).is_err());
        assert!(!root.path().join(OPERATIONS).exists());
        assert!(!root.path().join(FILE).exists());
    }
}

#[test]
#[ignore = "requires CEDAR_INTERRUPTED_SAVE_AGENT_BIN; explicit verify/CI acceptance"]
fn committed_lost_reply_checks_are_explicit_bounded_and_preserve_newer_text_and_history() {
    for newer in [false, true] {
        let mut h = Harness::new();
        h.lose_save(newer);
        let before = (
            h.app.documents[0].text.clone(),
            h.app.documents[0].edit_version,
            h.app.documents[0].cursor,
            h.app.documents[0].jump_to,
            h.app.documents[0].scroll_to,
        );
        let editor_id = egui::Id::new(("editor", h.app.documents[0].id));
        let selection = egui::TextEdit::load_state(&h.app.editor_ctx, editor_id)
            .unwrap()
            .cursor
            .char_range();
        h.check();
        h.assert_reads_since_reconnect(2);
        let doc = &h.app.documents[0];
        assert_eq!(
            (
                doc.text.clone(),
                doc.edit_version,
                doc.cursor,
                doc.jump_to,
                doc.scroll_to
            ),
            before
        );
        assert_eq!(
            egui::TextEdit::load_state(&h.app.editor_ctx, editor_id)
                .unwrap()
                .cursor
                .char_range(),
            selection
        );
        assert_eq!(doc.saved_text, B);
        assert!(doc.interrupted_save.is_none());
        assert_eq!(doc.dirty(), newer);
        let adopted = doc.revision.clone().unwrap();
        assert_ne!(adopted, h.original_revision);
        h.app.check_interrupted_save();
        h.assert_idle();
        h.history(false);
        assert_eq!(h.app.documents[0].text, if newer { B } else { A });
        assert_eq!(
            h.app.documents[0].revision.as_deref(),
            Some(adopted.as_str())
        );
        if newer {
            h.history(false);
            assert_eq!(h.app.documents[0].text, A);
            h.history(true);
            assert_eq!(h.app.documents[0].text, B);
        }
        h.history(true);
        assert_eq!(h.app.documents[0].text, if newer { C } else { B });
        h.assert_reads_since_reconnect(2);
        assert_eq!(fs::read_to_string(h.root.path().join(FILE)).unwrap(), B);

        if newer {
            // Only this subsequent explicit Save sends another Write, using
            // the adopted actual disk revision instead of the stale A base.
            h.app.save();
            let command = h.next();
            assert!(
                matches!(&command.op, Operation::Write { text, expected_revision, .. }
                if text == C && expected_revision.as_deref() == Some(adopted.as_str()))
            );
            let event = h.exchange(command);
            assert!(event.connected && matches!(event.result, Ok(Payload::Written { .. })));
            h.app.apply_event(event);
            assert!(!h.app.documents[0].dirty());
            assert_eq!(fs::read_to_string(h.root.path().join(FILE)).unwrap(), C);
            h.assert_operations(&[
                "Hello", "List", "Read", "Write", "Hello", "List", "Read", "Read", "Write",
            ]);
        }
    }
}

#[test]
#[ignore = "requires CEDAR_INTERRUPTED_SAVE_AGENT_BIN; explicit verify/CI acceptance"]
fn divergent_missing_and_different_byte_recreated_files_never_adopt_or_write() {
    for between_reads in [false, true] {
        for change in ["diverged", "missing", "recreated"] {
            let mut h = Harness::new();
            h.lose_save(true);
            if between_reads {
                h.app.check_interrupted_save();
                h.deliver_next_read();
            }
            let file = h.root.path().join(FILE);
            match change {
                "diverged" => fs::write(&file, D).unwrap(),
                "missing" => fs::remove_file(&file).unwrap(),
                "recreated" => {
                    fs::remove_file(&file).unwrap();
                    fs::write(&file, D).unwrap();
                }
                _ => unreachable!(),
            }
            if !between_reads {
                h.app.check_interrupted_save();
            }
            h.deliver_next_read();
            h.app.finish_interrupted_save_check();
            h.assert_idle();
            h.assert_unresolved(C);
            h.assert_reads_since_reconnect(if between_reads { 2 } else { 1 });
            assert_eq!(
                fs::read_to_string(file).ok().as_deref(),
                (change != "missing").then_some(D)
            );
        }
    }
}

#[test]
#[ignore = "requires CEDAR_INTERRUPTED_SAVE_AGENT_BIN; explicit verify/CI acceptance"]
fn identical_byte_recreation_establishes_content_equivalence_only() {
    let mut h = Harness::new();
    h.lose_save(false);
    fs::remove_file(h.root.path().join(FILE)).unwrap();
    fs::write(h.root.path().join(FILE), B).unwrap();
    h.check();
    assert_eq!(h.app.documents[0].saved_text, B);
    assert!(!h.app.documents[0].dirty());
    h.assert_reads_since_reconnect(2);
    // Content-only revisions intentionally cannot identify this physical
    // recreation or prove which request created the matching bytes.
}

#[test]
#[ignore = "requires CEDAR_INTERRUPTED_SAVE_AGENT_BIN; explicit verify/CI acceptance"]
fn edits_before_either_response_or_after_staging_reject_stale_checks() {
    for edit_at in ["first", "second", "staged"] {
        let mut h = Harness::new();
        h.lose_save(false);
        h.app.check_interrupted_save();
        let first = h.read_event();
        if edit_at == "first" {
            h.edit(C);
            h.app.apply_event(first);
        } else {
            h.app.apply_event(first);
            let second = h.read_event();
            if edit_at == "second" {
                h.edit(C);
                h.app.apply_event(second);
            } else {
                h.app.apply_event(second);
                h.edit(C);
            }
        }
        h.app.finish_interrupted_save_check();
        h.assert_unresolved(C);
        h.assert_idle();
        h.assert_reads_since_reconnect(if edit_at == "first" { 1 } else { 2 });
    }
}

#[test]
#[ignore = "requires CEDAR_INTERRUPTED_SAVE_AGENT_BIN; explicit verify/CI acceptance"]
fn old_generation_and_closed_document_replies_cannot_adopt_a_baseline() {
    for reconnect in [false, true] {
        let mut h = Harness::new();
        h.lose_save(false);
        h.app.check_interrupted_save();
        h.deliver_next_read();
        let stale = h.read_event();
        if reconnect {
            h.app.disconnected("synthetic second reconnect".into());
            h.connect();
            h.app.apply_event(stale);
            h.app.finish_interrupted_save_check();
            h.assert_unresolved(B);
            h.assert_operations(&[
                "Hello", "List", "Read", "Write", "Hello", "List", "Read", "Read", "Hello", "List",
            ]);
        } else {
            let old_id = h.app.documents[0].id;
            h.app.remove_tab(old_id);
            h.app.open(FILE.into(), None);
            h.deliver_next_read();
            assert_ne!(h.app.documents[0].id, old_id);
            h.edit(C);
            let before = format!("{:?}", h.app.documents[0]);
            h.app.apply_event(stale);
            h.app.finish_interrupted_save_check();
            assert_eq!(format!("{:?}", h.app.documents[0]), before);
            assert!(h.app.documents[0].interrupted_save.is_none());
            h.assert_reads_since_reconnect(3);
        }
        h.assert_idle();
        assert_eq!(fs::read_to_string(h.root.path().join(FILE)).unwrap(), B);
    }
}

#[test]
#[ignore = "requires CEDAR_INTERRUPTED_SAVE_AGENT_BIN; explicit verify/CI acceptance"]
fn malformed_revision_path_or_content_in_real_frames_preserve_uncertainty() {
    for second in [false, true] {
        for mode in [
            "bad-revision",
            "wrong-digest",
            "nul-content",
            "oversize-content",
            "wrong-path",
        ] {
            let mut h = Harness::new();
            h.lose_save(true);
            h.app.check_interrupted_save();
            if second {
                h.deliver_next_read();
            }
            fs::write(h.root.path().join(READ_MODE), format!("{mode}\n")).unwrap();
            h.deliver_next_read();
            h.app.finish_interrupted_save_check();
            h.assert_unresolved(C);
            h.assert_idle();
            h.assert_reads_since_reconnect(if second { 2 } else { 1 });
            assert_eq!(fs::read_to_string(h.root.path().join(FILE)).unwrap(), B);
        }
    }
}

#[test]
#[ignore = "requires CEDAR_INTERRUPTED_SAVE_AGENT_BIN; explicit verify/CI acceptance"]
fn owned_recovery_uses_the_adopted_base_and_only_owned_clean_copies_are_removed() {
    for newer in [false, true] {
        let mut h = Harness::new();
        let recovery_root = tempfile::tempdir().unwrap();
        let store_path = recovery_root.path().join("private-recovery");
        h.start_recovery(&store_path);
        let identity = h.app.recovery_workspace().unwrap();
        h.edit(B);
        h.persist();
        let event = h.lost_save_event();
        if newer {
            h.edit(C);
        }
        h.app.apply_event(event);
        h.persist();
        h.connect();
        h.check();
        let revision = h.app.documents[0].revision.clone();
        if newer {
            h.persist();
        } else {
            h.app.recovery.flush();
            recovery_wait(&mut h.app, |app| app.recovery.removals_finished());
        }
        h.assert_reads_since_reconnect(2);
        drop(h);
        let store = Store::open(&store_path).unwrap();
        if newer {
            let draft = store.read(&record_id(&identity, FILE).unwrap()).unwrap();
            assert_eq!(draft.text, C);
            assert_eq!(draft.base_text, B);
            assert_eq!(draft.base_revision, revision);
        } else {
            assert!(store.list().unwrap().drafts.is_empty());
        }
    }
}

#[test]
#[ignore = "requires CEDAR_INTERRUPTED_SAVE_AGENT_BIN; explicit verify/CI acceptance"]
fn reconciliation_retains_an_unowned_older_recovery_copy() {
    let mut h = Harness::new();
    let recovery_root = tempfile::tempdir().unwrap();
    let store_path = recovery_root.path().join("private-recovery");
    let identity: WorkspaceIdentity = h.app.recovery_workspace().unwrap();
    let older = Draft {
        workspace: identity.clone(),
        path: FILE.into(),
        text: "older generated recovery draft".into(),
        base_text: A.into(),
        base_revision: Some(h.original_revision.clone()),
        modified_ms: 1,
    };
    Store::open(&store_path).unwrap().write(1, &older).unwrap();
    h.start_recovery(&store_path);
    h.lose_save(false);
    h.app.recovery_tick(&h.app.editor_ctx.clone());
    assert_eq!(
        h.app.recovery.status(Some(&identity), h.app.active()).0,
        "Older recovery waiting"
    );
    h.check();
    h.app.recovery_tick(&h.app.editor_ctx.clone());
    h.app.recovery.flush();
    h.assert_reads_since_reconnect(2);
    drop(h);
    let store = Store::open(store_path).unwrap();
    let retained = store.read(&record_id(&identity, FILE).unwrap()).unwrap();
    assert_eq!(retained.text, older.text);
    assert_eq!(retained.base_text, older.base_text);
    assert_eq!(retained.base_revision, older.base_revision);
}
