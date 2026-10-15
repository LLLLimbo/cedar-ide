//! Exact ui.put bridge parity, using independent native contexts.
use super::*;

#[test]
fn files_ime_show_bridge_preserves_response_cursor_layout_and_paint() {
    let direct = egui::Context::default();
    let bridge = egui::Context::default();
    let id = egui::Id::new("files-ui-put-parity");
    let mut a = "文字😀".to_owned();
    let mut b = a.clone();
    for (step, events) in [
        vec![],
        vec![
            egui::Event::Ime(egui::ImeEvent::Enabled),
            egui::Event::Ime(egui::ImeEvent::Preedit("预备".into())),
        ],
        vec![egui::Event::Ime(egui::ImeEvent::Commit("预备".into()))],
        vec![
            egui::Event::Ime(egui::ImeEvent::Commit("拒绝".into())),
            egui::Event::Ime(egui::ImeEvent::Disabled),
        ],
        vec![egui::Event::Text("text".into())],
        vec![egui::Event::Paste("paste".into())],
        vec![
            egui::Event::PointerMoved(egui::pos2(100.0, 85.0)),
            egui::Event::PointerButton {
                pos: egui::pos2(100.0, 85.0),
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::PointerButton {
                pos: egui::pos2(100.0, 85.0),
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ],
        vec![],
    ]
    .into_iter()
    .enumerate()
    {
        let mut snapshots = Vec::new();
        let mut paints = Vec::new();
        for (ctx, text, use_bridge) in [(&direct, &mut a, false), (&bridge, &mut b, true)] {
            let raw = egui::RawInput {
                time: Some(1.0 + step as f64 * 0.01),
                events: events.clone(),
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(780.0, 540.0),
                )),
                ..Default::default()
            };
            let output = ctx.run(raw, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    if step == 0 {
                        ui.memory_mut(|m| m.request_focus(id));
                    }
                    let rect =
                        egui::Rect::from_min_size(egui::pos2(80.0, 70.0), egui::vec2(410.0, 30.0));
                    let edit = egui::TextEdit::singleline(text)
                        .id(id)
                        .char_limit(4096)
                        .desired_width(f32::INFINITY)
                        .hint_text("Type a filename");
                    let response = if use_bridge {
                        put_files_text_edit(ui, rect, edit).0
                    } else {
                        ui.put(rect, edit)
                    };
                    let next = ui.allocate_response(egui::vec2(40.0, 25.0), egui::Sense::hover());
                    snapshots.push(format!(
                        "{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}",
                        response.id,
                        response.rect,
                        response.interact_rect,
                        response.sense,
                        response.changed(),
                        response.has_focus(),
                        response.gained_focus(),
                        response.lost_focus(),
                        next.rect,
                        egui::TextEdit::load_state(ctx, id)
                            .unwrap()
                            .cursor
                            .char_range()
                    ));
                });
            });
            paints.push(format!(
                "{:?}",
                ctx.tessellate(output.shapes, output.pixels_per_point)
            ));
        }
        assert_eq!(a, b, "native bytes step {step}");
        assert_eq!(snapshots.len(), 2);
        assert_eq!(
            snapshots[0], snapshots[1],
            "response/cursor/layout step {step}"
        );
        assert_eq!(paints[0], paints[1], "paint meshes step {step}");
    }
}
