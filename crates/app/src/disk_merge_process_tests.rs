//! Trust-off normal-agent acceptance for explicit draft-only merge. All disk
//! mutations are generated fixture setup or separately requested Save actions.
use super::*;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const WAIT: Duration = Duration::from_secs(5);
const PATH: &str = "draft 草稿.txt";
const BASE: &str = "alpha 草稿\nshared A\nmiddle 🐻\nshared B\nomega\n";
const LOCAL: &str = "local α\nshared A\nmiddle 🐻\nshared B\nomega\n";
const DISK: &str = "alpha 草稿\nshared A\nmiddle 🐻\nshared B\nremote Ω\n";
const MERGED: &str = "local α\nshared A\nmiddle 🐻\nshared B\nremote Ω\n";
const LATER: &str = "a later external version\n";

fn revision(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

fn binary(name: &str) -> PathBuf {
    let path = PathBuf::from(std::env::var_os(name).expect("explicit acceptance binary required"));
    assert!(path.is_absolute() && path.is_file());
    path
}

fn wait_file(root: &Path, name: &str) -> Vec<u8> {
    let deadline = Instant::now() + WAIT;
    loop {
        if let Ok(bytes) = fs::read(root.join(name)) {
            return bytes;
        }
        assert!(Instant::now() < deadline, "missing relay marker {name}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn response(app: &mut CedarApp, expected_ok: bool) {
    let event = app
        .result_rx
        .recv_timeout(WAIT)
        .expect("normal agent response timed out");
    let WorkerEvent::Response(ref reply) = event else {
        panic!("unexpected idle loss")
    };
    assert!(reply.connected);
    assert_eq!(reply.result.is_ok(), expected_ok);
    if !expected_ok {
        assert!(reply.result.as_ref().unwrap_err().starts_with("conflict:"));
    }
    app.apply_worker_event(event);
}

fn requests(root: &Path) -> Vec<cedar_protocol::Request> {
    fs::read_to_string(root.join("requests"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn assert_no_writes(root: &Path) {
    let operations = requests(root);
    assert_eq!(operations.len(), 5);
    assert!(matches!(operations[0].op, Operation::Hello));
    assert!(matches!(operations[1].op, Operation::List { .. }));
    for request in &operations[2..] {
        assert!(matches!(&request.op, Operation::Read { path } if path == PATH));
    }
}

fn close(app: &mut CedarApp, root: &Path) {
    let pid = String::from_utf8(wait_file(root, "relay-agent-started"))
        .unwrap()
        .trim()
        .parse::<u32>()
        .unwrap();
    fs::write(
        root.join("relay-disconnect.pending"),
        b"close-agent-stdin\n",
    )
    .unwrap();
    fs::rename(
        root.join("relay-disconnect.pending"),
        root.join("relay-disconnect"),
    )
    .unwrap();
    let event = app
        .result_rx
        .recv_timeout(WAIT)
        .expect("controlled normal-agent closure timed out");
    assert!(
        matches!(&event, WorkerEvent::TransportLost { generation, .. } if *generation == app.generation)
    );
    app.apply_worker_event(event);
    assert!(app.state == ConnectionState::Disconnected && app.worker.is_none());
    let receipt: serde_json::Value =
        serde_json::from_slice(&wait_file(root, "relay-agent-reaped")).unwrap();
    assert_eq!(receipt["process_id"], pid);
    assert_eq!(receipt["cleanup_verified"], true);
    assert_eq!(receipt["exit_success"], true);
    let closed = app
        .result_rx
        .recv_timeout(WAIT)
        .expect("local relay cleanup receipt missing");
    assert!(
        matches!(&closed, WorkerEvent::Closed { generation, result: Ok(()) } if *generation == app.generation)
    );
    app.apply_worker_event(closed);
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

#[test]
#[ignore = "requires explicit normal agent and relay binaries; actual process acceptance"]
fn normal_agent_merge_is_read_only_until_explicit_save_and_keeps_revision_conflicts() {
    let agent = binary("CEDAR_DISK_MERGE_AGENT_BIN");
    let peer = binary("CEDAR_DISK_MERGE_PEER_BIN");
    // Three independently owned connections distinguish accepted Save, a later
    // Save conflict, and disk changes during merge verification.
    for scenario in ["save", "save_conflict", "verification_changed"] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("merge workspace 草稿");
        fs::create_dir(&root).unwrap();
        fs::write(
            root.join(".cedar-transport-fixture"),
            b"cedar-transport-fixture-v1\n",
        )
        .unwrap();
        fs::write(root.join("fixture-mode"), "normal_agent_idle_relay").unwrap();
        fs::write(root.join("relay-agent-path"), agent.to_str().unwrap()).unwrap();
        fs::write(root.join(PATH), BASE).unwrap();
        let mut app = CedarApp::empty();
        app.generation = 1;
        app.state = ConnectionState::Connecting;
        app.open_form = false;
        app.connecting_form = Some(ConnectForm {
            ssh: false,
            local_root: root.to_str().unwrap().into(),
            allow_run: false,
            ..Default::default()
        });
        app.worker = Some(Worker::spawn_agent(
            peer.clone(),
            root.clone(),
            app.generation,
            app.result_tx.clone(),
            app.editor_ctx.clone(),
        ));
        response(&mut app, true);
        response(&mut app, true);
        assert!(app.ready() && !app.execution_trusted());
        app.open(PATH.into(), None);
        response(&mut app, true);
        assert_eq!(app.documents[0].text, BASE);
        assert_eq!(
            app.documents[0].revision.as_deref(),
            Some(revision(BASE).as_str())
        );
        frame(&mut app, 0.0, vec![]);
        editor_state::commit(&app.editor_ctx, &mut app.documents[0], LOCAL.into(), 7);
        frame(&mut app, 1.0, vec![]);
        let id = egui::Id::new(("editor", app.documents[0].id));
        let selection =
            egui::text::CCursorRange::two(egui::text::CCursor::new(7), egui::text::CCursor::new(2));
        let mut state = egui::TextEdit::load_state(&app.editor_ctx, id).unwrap();
        state.cursor.set_char_range(Some(selection));
        state.store(&app.editor_ctx, id);
        fs::write(root.join(PATH), DISK).unwrap();
        app.compare_with_disk();
        response(&mut app, true);
        let ctx = app.editor_ctx.clone();
        app.preview_disk_merge(&ctx);
        assert_eq!(app.documents[0].text, LOCAL);
        assert_eq!(app.documents[0].saved_text, BASE);
        if scenario == "verification_changed" {
            fs::write(root.join(PATH), LATER).unwrap();
        }
        app.apply_disk_merge(&ctx);
        response(&mut app, true);
        frame(&mut app, 2.0, vec![]);
        assert_no_writes(&root);
        if scenario == "verification_changed" {
            assert_eq!(app.documents[0].text, LOCAL);
            assert_eq!(app.documents[0].saved_text, BASE);
            assert_eq!(
                app.documents[0].revision.as_deref(),
                Some(revision(BASE).as_str())
            );
            assert_eq!(fs::read_to_string(root.join(PATH)).unwrap(), LATER);
        } else {
            assert_eq!(app.documents[0].text, MERGED);
            assert_eq!(app.documents[0].saved_text, DISK);
            assert_eq!(
                app.documents[0].revision.as_deref(),
                Some(revision(DISK).as_str())
            );
            assert!(app.documents[0].dirty() && app.documents[0].interrupted_save.is_none());
            assert_eq!(fs::read_to_string(root.join(PATH)).unwrap(), DISK);
            app.dismiss_disk_review();
            app.editor_ctx.memory_mut(|m| m.request_focus(id));
            frame(&mut app, 3.0, undo_events(false));
            assert_eq!(app.documents[0].text, LOCAL);
            let restored = egui::TextEdit::load_state(&app.editor_ctx, id)
                .unwrap()
                .cursor
                .char_range()
                .unwrap();
            assert_eq!(restored.primary.index, selection.primary.index);
            assert_eq!(
                restored.primary.prefer_next_row,
                selection.primary.prefer_next_row
            );
            assert_eq!(restored.secondary.index, selection.secondary.index);
            assert_eq!(
                restored.secondary.prefer_next_row,
                selection.secondary.prefer_next_row
            );
            assert_eq!(app.documents[0].saved_text, DISK);
            frame(&mut app, 4.0, undo_events(true));
            assert_eq!(app.documents[0].text, MERGED);
            assert_eq!(app.documents[0].saved_text, DISK);
            assert_no_writes(&root);
            if scenario == "save_conflict" {
                fs::write(root.join(PATH), LATER).unwrap();
            }
            app.save();
            response(&mut app, scenario == "save");
            let audit = requests(&root);
            assert_eq!(
                audit
                    .iter()
                    .filter(|request| matches!(request.op, Operation::Write { .. }))
                    .count(),
                1
            );
            assert!(
                matches!(&audit[5].op,Operation::Write{path,text,expected_revision} if path==PATH && text==MERGED && expected_revision.as_deref()==Some(revision(DISK).as_str()))
            );
            if scenario == "save" {
                response(&mut app, true); // Existing post-save List.
                assert!(!app.documents[0].dirty());
                assert_eq!(fs::read_to_string(root.join(PATH)).unwrap(), MERGED);
            } else {
                assert!(app.documents[0].dirty() && app.documents[0].interrupted_save.is_none());
                assert_eq!(app.documents[0].text, MERGED);
                assert_eq!(app.documents[0].saved_text, DISK);
                assert_eq!(fs::read_to_string(root.join(PATH)).unwrap(), LATER);
            }
        }
        assert!(app.pending.is_empty());
        close(&mut app, &root);
        drop(app);
        temp.close().unwrap();
    }
    println!("disk_merge_acceptance cases=3 trust_off=true preview_inert=true merge_no_write=true separate_disk_baseline=true dirty_result=true full_selection_undo=true redo=true explicit_save_revision=true later_conflict_rejected=true verification_change_rejected=true final_disk_expected=true normal_agent_reaped=true fixtures_removed=true");
}
