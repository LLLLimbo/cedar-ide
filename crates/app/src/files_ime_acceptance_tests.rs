//! Product receipt admission through the real raw hook and App::update.
//! No native private state, event replay, or weaker replacement for old tests.
use super::*;
use egui::{Event, ImeEvent, Key};

fn now(app: &CedarApp) -> f64 {
    app.editor_ctx.input(|input| input.time)
}
fn frame(
    app: &mut CedarApp,
    time: f64,
    events: Vec<Event>,
    discard: bool,
) -> Vec<serde_json::Value> {
    frame_interposed(app, time, events, discard, false, false)
}
fn frame_interposed(
    app: &mut CedarApp,
    time: f64,
    events: Vec<Event>,
    discard: bool,
    remove_prefix: bool,
    reset_view: bool,
) -> Vec<serde_json::Value> {
    let ctx = app.editor_ctx.clone();
    let raw = format!("{events:?}");
    let mut input = egui::RawInput {
        time: Some(time),
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SIZE)),
        events,
        ..Default::default()
    };
    eframe::App::raw_input_hook(app, &ctx, &mut input);
    let hook = app.browse.ime_focus_observation();
    if reset_view {
        app.show_browse(browse::Provider::Files);
    }
    let mut native = eframe::Frame::_new_kittest();
    let mut passes = Vec::new();
    let _ = ctx.run(input, |ctx| {
        if remove_prefix && passes.is_empty() {
            ctx.input_mut(|input| assert!(matches!(input.events.remove(0), Event::Copy)));
        }
        eframe::App::update(app, ctx, &mut native);
        let pass = serde_json::json!({"raw":raw,"time":time,"pass":passes.len(),"hook":hook,"query":app.browse.query(),"scope":app.browse.scope(),"query_response":crate::workspace_access_tests::recorded_observation(app,"browse_query"),"scope_response":crate::workspace_access_tests::recorded_observation(app,"browse_scope"),"controller":app.browse.ime_focus_observation()});
        eprintln!("IME_ACCEPTANCE_TRACE {pass}");
        if discard && passes.is_empty() { ctx.request_discard("receipt admission extra pass"); }
        passes.push(pass);
    });
    passes
}
fn step(app: &mut CedarApp, events: Vec<Event>) {
    frame(app, now(app) + 0.001, events, false);
}
fn ime(event: ImeEvent) -> Event {
    Event::Ime(event)
}
fn fixture(origin: &str) -> (CedarApp, Receiver<Command>) {
    let (mut app, commands) = discovery_fixture();
    enter(&mut app, false);
    files_live::edit(&mut app, "browse_query", "stable");
    files_live::edit(&mut app, "browse_scope", "src");
    files_live::edit(&mut app, origin, "z");
    let t = now(&app) + 0.25;
    frame(&mut app, t, vec![], false);
    let q = app.browse.query().to_owned();
    let s = app.browse.scope().to_owned();
    let request = expect_discovery(&commands, &q, &s);
    reply(
        &mut app,
        request.id,
        Ok(discovery_result(&q, &s, &[], vec![])),
    );
    step(&mut app, vec![]);
    assert!(commands.try_recv().is_err());
    assert!(!app.browse.ime_focus_observation()["composition"]
        .as_bool()
        .unwrap());
    (app, commands)
}
fn outcomes(passes: &[serde_json::Value], origin: &str) -> Vec<String> {
    let field = if origin == "browse_query" {
        "query_response"
    } else {
        "scope_response"
    };
    passes[0][field]["ime_receipts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["outcome"].as_str().unwrap().to_owned())
        .collect()
}
fn settle(app: &mut CedarApp, commands: &Receiver<Command>, t: f64, expected: bool) {
    let q = app.browse.query().to_owned();
    let s = app.browse.scope().to_owned();
    frame(app, t + 0.249, vec![], false);
    assert!(commands.try_recv().is_err(), "full interval");
    frame(app, t + 0.250, vec![], false);
    let emitted: Vec<_> = commands.try_iter().collect();
    eprintln!(
        "IME_ACCEPTANCE_COMMANDS {}",
        serde_json::json!({"time":t+0.250,"query":q,"scope":s,"expected":expected,"commands":emitted.iter().map(|c|serde_json::json!({"id":c.id,"op":c.op})).collect::<Vec<_>>()})
    );
    assert_eq!(
        emitted.len(),
        usize::from(expected),
        "exact automatic command count"
    );
    for command in emitted {
        assert!(
            matches!(command.op,Operation::FindFiles {query,scope,limit} if query==q && scope==s && limit==cedar_protocol::DEFAULT_FILE_DISCOVERY_RESULTS)
        );
    }
    frame(app, t + 2.0, vec![], false);
    assert!(commands.try_recv().is_err(), "single consumption");
    assert_eq!(
        (app.browse.query(), app.browse.scope()),
        (q.as_str(), s.as_str())
    );
}

#[test]
fn files_acceptance_direct_commit_actual_accept_reject_same_value_both_fields() {
    for origin in ["browse_query", "browse_scope"] {
        for accepted in [false, true] {
            for discard in [false, true] {
                let (mut app, commands) = fixture(origin);
                step(
                    &mut app,
                    if accepted {
                        modified_key(Key::A, command_modifiers())
                    } else {
                        key(Key::End)
                    },
                );
                let t = now(&app) + 0.001;
                let passes = frame(
                    &mut app,
                    t,
                    vec![ime(ImeEvent::Commit("z".into())), ime(ImeEvent::Disabled)],
                    discard,
                );
                let result = outcomes(&passes, origin);
                assert_eq!(
                    result[0].starts_with("CommitAccepted"),
                    accepted,
                    "{result:?}"
                );
                assert_eq!(result[0] == "CommitRejectedAnchor", !accepted);
                if discard {
                    assert!(passes.len() > 1);
                    assert!(passes[1][if origin == "browse_query" {
                        "query_response"
                    } else {
                        "scope_response"
                    }]["ime_receipts"]
                        .as_array()
                        .unwrap()
                        .is_empty());
                }
                settle(&mut app, &commands, t, accepted);
            }
        }
    }
}

#[test]
fn files_acceptance_duplicate_reject_retains_same_lifecycle_completion() {
    for origin in ["browse_query", "browse_scope"] {
        for discard in [false, true] {
            let (mut app, commands) = fixture(origin);
            step(&mut app, modified_key(Key::A, command_modifiers()));
            let t = now(&app) + 0.001;
            let passes = frame(
                &mut app,
                t,
                vec![
                    ime(ImeEvent::Commit("z".into())),
                    ime(ImeEvent::Commit("z".into())),
                    ime(ImeEvent::Disabled),
                ],
                discard,
            );
            let r = outcomes(&passes, origin);
            assert!(r[0].starts_with("CommitAccepted"));
            assert_eq!(r[1], "CommitRejectedAnchor");
            settle(&mut app, &commands, t, true);
        }
    }
}

#[test]
fn files_acceptance_direct_accepted_then_empty_or_linebreak_preserves_075_edit() {
    for origin in ["browse_query", "browse_scope"] {
        for terminal in ["", "\r", "\n"] {
            for value in ["changed", "z"] {
                let (mut app, commands) = fixture(origin);
                step(&mut app, modified_key(Key::A, command_modifiers()));
                let t = now(&app) + 0.001;
                let passes = frame(
                    &mut app,
                    t,
                    vec![
                        ime(ImeEvent::Commit(value.into())),
                        ime(ImeEvent::Commit(terminal.into())),
                        ime(ImeEvent::Disabled),
                    ],
                    true,
                );
                let r = outcomes(&passes, origin);
                assert!(r[0].starts_with("CommitAccepted"));
                settle(&mut app, &commands, t, true);
            }
        }
    }
}

#[test]
fn files_acceptance_native_twin_latest_lifecycle_not_any_insertion() {
    for origin in ["browse_query", "browse_scope"] {
        for latest_accepted in [false, true] {
            let (mut app, commands) = fixture(origin);
            step(
                &mut app,
                key(if latest_accepted { Key::End } else { Key::Home }),
            );
            step(&mut app, vec![ime(ImeEvent::Enabled)]);
            step(&mut app, key(Key::Home));
            step(&mut app, vec![ime(ImeEvent::Disabled)]);
            step(&mut app, vec![Event::Text(String::new())]); // independent fresh intent, no native anchor mutation
            let t = now(&app) + 0.001;
            let mut events = vec![ime(ImeEvent::Commit("x".into())), ime(ImeEvent::Enabled)];
            events.extend(key(Key::Home));
            events.push(ime(ImeEvent::Commit("x".into())));
            events.extend(key(Key::End));
            let passes = frame(&mut app, t, events, true);
            let r = outcomes(&passes, origin);
            assert_eq!(r[0].starts_with("CommitAccepted"), !latest_accepted);
            assert_eq!(r[1], "Enabled");
            assert_eq!(r[2].starts_with("CommitAccepted"), latest_accepted);
            assert_eq!(
                if origin == "browse_query" {
                    app.browse.query()
                } else {
                    app.browse.scope()
                },
                "xz"
            );
            settle(&mut app, &commands, t, latest_accepted);
        }
    }
}

#[test]
fn files_acceptance_same_field_disabled_late_commit_stays_paused_then_plain_recovers() {
    for origin in ["browse_query", "browse_scope"] {
        for paste in [false, true] {
            let (mut app, commands) = fixture(origin);
            step(&mut app, modified_key(Key::A, command_modifiers()));
            step(
                &mut app,
                vec![ime(ImeEvent::Enabled), ime(ImeEvent::Preedit("pre".into()))],
            );
            step(&mut app, vec![ime(ImeEvent::Disabled)]);
            let t = now(&app) + 0.001;
            let passes = frame(
                &mut app,
                t,
                vec![ime(ImeEvent::Commit("late".into()))],
                true,
            );
            assert!(
                outcomes(&passes, origin)[0].starts_with("CommitAccepted"),
                "native acceptance alone does not remove barrier"
            );
            settle(&mut app, &commands, t, false);
            let t = now(&app) + 0.001;
            frame(
                &mut app,
                t,
                vec![if paste {
                    Event::Paste("fresh".into())
                } else {
                    Event::Text("fresh".into())
                }],
                false,
            );
            settle(&mut app, &commands, t, true);
        }
    }
}

#[test]
fn files_acceptance_rejected_direct_with_independent_plain_intent_still_searches() {
    for origin in ["browse_query", "browse_scope"] {
        for paste in [false, true] {
            for plain_first in [false, true] {
                let (mut app, commands) = fixture(origin);
                step(&mut app, key(Key::End));
                let plain = if paste {
                    Event::Paste("fresh".into())
                } else {
                    Event::Text("fresh".into())
                };
                let commit = ime(ImeEvent::Commit("no".into()));
                let t = now(&app) + 0.001;
                let passes = frame(
                    &mut app,
                    t,
                    if plain_first {
                        vec![plain, commit]
                    } else {
                        vec![commit, plain]
                    },
                    true,
                );
                assert_eq!(outcomes(&passes, origin), vec!["CommitRejectedAnchor"]);
                settle(&mut app, &commands, t, true);
            }
        }
    }
}

#[test]
fn files_acceptance_direct_rejected_then_accepted_uses_actual_second_event() {
    for origin in ["browse_query", "browse_scope"] {
        let (mut app, commands) = fixture(origin);
        step(&mut app, key(Key::End));
        let mut events = vec![ime(ImeEvent::Commit("rejected".into()))];
        events.extend(key(Key::Home));
        events.push(ime(ImeEvent::Commit("accepted".into())));
        let t = now(&app) + 0.001;
        let passes = frame(&mut app, t, events, true);
        let r = outcomes(&passes, origin);
        assert_eq!(r[0], "CommitRejectedAnchor");
        assert!(r[1].starts_with("CommitAccepted"));
        assert_eq!(
            if origin == "browse_query" {
                app.browse.query()
            } else {
                app.browse.scope()
            },
            "acceptedz"
        );
        settle(&mut app, &commands, t, true);
    }
}

#[test]
fn files_acceptance_multiple_accepted_then_rejected_retains_direct_completion() {
    for origin in ["browse_query", "browse_scope"] {
        let (mut app, commands) = fixture(origin);
        step(&mut app, modified_key(Key::A, command_modifiers()));
        let mut events = vec![ime(ImeEvent::Commit("x".into()))];
        events.extend(key(Key::Home));
        events.push(ime(ImeEvent::Commit("y".into())));
        events.push(ime(ImeEvent::Commit("rejected".into())));
        let t = now(&app) + 0.001;
        let passes = frame(&mut app, t, events, true);
        let r = outcomes(&passes, origin);
        assert!(r[0].starts_with("CommitAccepted"));
        assert!(r[1].starts_with("CommitAccepted"));
        assert_eq!(r[2], "CommitRejectedAnchor");
        assert_eq!(
            if origin == "browse_query" {
                app.browse.query()
            } else {
                app.browse.scope()
            },
            "yx"
        );
        settle(&mut app, &commands, t, true);
    }
}

#[test]
fn files_acceptance_rejected_direct_and_real_backspace_edit_remain_independent() {
    for origin in ["browse_query", "browse_scope"] {
        let (mut app, commands) = fixture(origin);
        step(&mut app, key(Key::End));
        let mut events = vec![ime(ImeEvent::Commit("rejected".into()))];
        events.extend(key(Key::Backspace));
        // Keep a nonempty query for a real backend command even if Query is edited.
        step(&mut app, vec![Event::Text("more".into())]);
        let t = now(&app) + 0.001;
        let passes = frame(&mut app, t, events, true);
        assert_eq!(outcomes(&passes, origin), vec!["CommitRejectedAnchor"]);
        assert_eq!(
            if origin == "browse_query" {
                app.browse.query()
            } else {
                app.browse.scope()
            },
            "zmor"
        );
        settle(&mut app, &commands, t, true);
    }
}

#[test]
fn files_acceptance_direct_receipt_with_native_undo_preserves_actual_final_bytes() {
    for origin in ["browse_query", "browse_scope"] {
        for commit_first in [false, true] {
            for accepted in [false, true] {
                let (mut app, commands) = fixture(origin);
                // Let the native undoer keep z as a real time-separated checkpoint.
                let t = now(&app) + 1.1;
                frame(&mut app, t, vec![], false);
                step(&mut app, vec![Event::Text("tail".into())]);
                step(
                    &mut app,
                    if accepted {
                        key(Key::Home)
                    } else {
                        key(Key::End)
                    },
                );
                let commit = ime(ImeEvent::Commit("X".into()));
                let undo = modified_key(Key::Z, command_modifiers());
                let mut events = if commit_first {
                    vec![commit.clone()]
                } else {
                    undo.clone()
                };
                if commit_first {
                    events.extend(undo)
                } else {
                    events.push(commit)
                }
                let t = now(&app) + 0.001;
                let passes = frame(&mut app, t, events, true);
                let r = outcomes(&passes, origin);
                assert_eq!(r.len(), 1);
                // If Undo ran first, its restored cursor controls the true branch.
                let native_accepted = r[0].starts_with("CommitAccepted");
                let final_text = if origin == "browse_query" {
                    app.browse.query()
                } else {
                    app.browse.scope()
                };
                let actual_byte_edit = final_text != "ztail";
                eprintln!(
                    "IME_ACCEPTANCE_UNDO {}",
                    serde_json::json!({"origin":origin,"commit_first":commit_first,"prepared_anchor_match":accepted,"native_accepted":native_accepted,"final_text":final_text,"actual_byte_edit":actual_byte_edit})
                );
                assert!(
                    actual_byte_edit || native_accepted,
                    "fixture must exercise real Undo or accepted same-value intent"
                );
                settle(&mut app, &commands, t, true);
            }
        }
    }
}

#[test]
fn files_acceptance_wrong_owner_native_bytes_never_authorize_then_plain_recovers() {
    // Scope is drawn after Query: Query can process this batch before the old
    // Scope's focus-loss cleanup removes IME. This is the critical direction.
    let (mut app, commands) = fixture("browse_scope");
    let rect = recorded_response(&app, "browse_query").rect;
    let pos = egui::pos2(rect.left() + 1.0, rect.center().y);
    let mut events = vec![Event::PointerMoved(pos)];
    events.extend(pointer(pos, true));
    events.extend(pointer(pos, false));
    events.push(ime(ImeEvent::Commit("unowned".into())));
    let t = now(&app) + 0.001;
    let passes = frame(&mut app, t, events, true);
    assert!(outcomes(&passes, "browse_query")[0].starts_with("CommitAccepted"));
    assert_eq!(app.browse.query(), "unowned");
    assert_eq!(
        focused(&app),
        Some(recorded_response(&app, "browse_query").id)
    );
    settle(&mut app, &commands, t, false);
    let t = now(&app) + 0.001;
    frame(&mut app, t, vec![Event::Text("fresh".into())], false);
    settle(&mut app, &commands, t, true);
}

#[test]
fn files_acceptance_stale_epoch_native_bytes_never_authorize() {
    let (mut app, commands) = fixture("browse_query");
    step(&mut app, key(Key::Home));
    let t = now(&app) + 0.001;
    // Reopening Files is a real public view transition after raw capture. It
    // retains the stable control ID but retires the raw intent's epoch.
    let passes = frame_interposed(
        &mut app,
        t,
        vec![ime(ImeEvent::Commit("stale".into()))],
        true,
        false,
        true,
    );
    assert!(outcomes(&passes, "browse_query")[0].starts_with("CommitAccepted"));
    assert_eq!(app.browse.query(), "stale");
    settle(&mut app, &commands, t, false);
}

#[test]
fn files_acceptance_widget_local_index_survives_earlier_consumer_prefix_removal() {
    for origin in ["browse_query", "browse_scope"] {
        let (mut app, commands) = fixture(origin);
        step(&mut app, modified_key(Key::A, command_modifiers()));
        let t = now(&app) + 0.001;
        let passes = frame_interposed(
            &mut app,
            t,
            vec![Event::Copy, ime(ImeEvent::Commit("accepted".into()))],
            true,
            true,
            false,
        );
        let field = if origin == "browse_query" {
            "query_response"
        } else {
            "scope_response"
        };
        assert_eq!(
            passes[0][field]["ime_receipts"][0]["input_event_index"], 0,
            "local InputState index is not raw index 1"
        );
        assert!(outcomes(&passes, origin)[0].starts_with("CommitAccepted"));
        settle(&mut app, &commands, t, true);
    }
}

#[test]
fn files_acceptance_pure_plain_pointer_handoff_remains_an_actual_edit() {
    for origin in ["browse_query", "browse_scope"] {
        for paste in [false, true] {
            let (mut app, commands) = fixture(origin);
            let destination = if origin == "browse_query" {
                "browse_scope"
            } else {
                "browse_query"
            };
            let rect = recorded_response(&app, destination).rect;
            let pos = egui::pos2(rect.left() + 1.0, rect.center().y);
            let mut events = vec![Event::PointerMoved(pos)];
            events.extend(pointer(pos, true));
            events.extend(pointer(pos, false));
            events.push(if paste {
                Event::Paste("fresh".into())
            } else {
                Event::Text("fresh".into())
            });
            let t = now(&app) + 0.001;
            let passes = frame(&mut app, t, events, true);
            assert!(outcomes(&passes, destination).is_empty());
            assert!((if destination == "browse_query" {
                app.browse.query()
            } else {
                app.browse.scope()
            })
            .starts_with("fresh"));
            settle(&mut app, &commands, t, true);
        }
    }
}

#[test]
fn files_acceptance_direct_native_acceptance_keeps_256_byte_business_limit() {
    for bytes in [256, 257] {
        let (mut app, commands) = fixture("browse_query");
        step(&mut app, modified_key(Key::A, command_modifiers()));
        let mut text = "😀".repeat(64);
        if bytes == 257 {
            text.push('a');
        }
        assert_eq!(text.len(), bytes);
        let t = now(&app) + 0.001;
        let passes = frame(&mut app, t, vec![ime(ImeEvent::Commit(text.clone()))], true);
        assert!(outcomes(&passes, "browse_query")[0].starts_with("CommitAccepted"));
        assert_eq!(app.browse.query(), text);
        assert!(passes[0]["controller"]["settlement"].is_object());
        assert_eq!(
            cedar_protocol::validate_file_discovery_request(
                &text,
                app.browse.scope(),
                cedar_protocol::DEFAULT_FILE_DISCOVERY_RESULTS
            )
            .is_ok(),
            bytes == 256
        );
        settle(&mut app, &commands, t, bytes == 256);
    }
}

#[test]
fn files_acceptance_direct_accepted_zero_scalar_insertion_still_has_receipt() {
    for origin in ["browse_query", "browse_scope"] {
        let (mut app, commands) = fixture(origin);
        files_live::edit(&mut app, origin, &"a".repeat(4096));
        step(&mut app, vec![ime(ImeEvent::Enabled)]);
        step(&mut app, vec![ime(ImeEvent::Disabled)]);
        step(&mut app, vec![Event::Text(String::new())]);
        let t = now(&app) + 0.001;
        let passes = frame(
            &mut app,
            t,
            vec![ime(ImeEvent::Commit("extra😀".into()))],
            true,
        );
        let r = outcomes(&passes, origin);
        assert_eq!(
            r,
            vec!["CommitAccepted { insertion_start: 4096, insertion_end: 4096 }"]
        );
        assert_eq!(
            if origin == "browse_query" {
                app.browse.query()
            } else {
                app.browse.scope()
            },
            "a".repeat(4096)
        );
        assert!(passes[0]["controller"]["settlement"].is_object());
        settle(&mut app, &commands, t, origin == "browse_scope");
    }
}

#[test]
fn files_acceptance_reverse_direct_handoff_native_acceptance_stays_unowned() {
    let (mut app, commands) = fixture("browse_query");
    let rect = recorded_response(&app, "browse_scope").rect;
    let pos = egui::pos2(rect.left() + 1.0, rect.center().y);
    let mut events = vec![Event::PointerMoved(pos)];
    events.extend(pointer(pos, true));
    events.extend(pointer(pos, false));
    events.push(ime(ImeEvent::Commit("unobserved".into())));
    let t = now(&app) + 0.001;
    let passes = frame(&mut app, t, events, true);
    assert!(outcomes(&passes, "browse_scope")[0].starts_with("CommitAccepted"));
    assert_eq!(
        (app.browse.query(), app.browse.scope()),
        ("z", "unobserved")
    );
    settle(&mut app, &commands, t, false);
}

#[test]
fn files_acceptance_active_handoff_removed_before_widget_is_unobserved() {
    let (mut app, commands) = fixture("browse_query");
    // Native focus-loss cleanup removes IME only when IME was enabled.
    step(&mut app, vec![ime(ImeEvent::Enabled)]);
    let rect = recorded_response(&app, "browse_scope").rect;
    let pos = egui::pos2(rect.left() + 1.0, rect.center().y);
    let mut events = vec![Event::PointerMoved(pos)];
    events.extend(pointer(pos, true));
    events.extend(pointer(pos, false));
    events.push(ime(ImeEvent::Commit("unobserved".into())));
    let t = now(&app) + 0.001;
    let passes = frame(&mut app, t, events, true);
    assert!(outcomes(&passes, "browse_query").is_empty());
    assert!(
        outcomes(&passes, "browse_scope").is_empty(),
        "active Query cleanup removed IME before Scope snapshot"
    );
    assert_eq!((app.browse.query(), app.browse.scope()), ("z", "src"));
    settle(&mut app, &commands, t, false);
}
