//! Preserve a deliberate tools-panel height without changing its passive layout.
//! The native egui handle owns interaction; this adapter only reads its response.
use eframe::egui::{self, containers::panel::PanelState};

fn panel_id() -> egui::Id {
    egui::Id::new("tools")
}

// Pinned egui 0.31.1 TopBottomPanel::show_inside_dyn uses this child ID.
// Keep that dependency here; never register another handle or consume its input.
pub(super) fn native_resize_id() -> egui::Id {
    panel_id().with("__resize")
}

#[derive(Clone, Copy)]
pub(super) struct ResizePass {
    reserve: bool,
    capture: bool,
}

fn resize_pass(
    preferred: Option<f32>,
    owned_primary_drag: bool,
    vertical: Option<f32>,
) -> ResizePass {
    let capture = owned_primary_drag
        && vertical.is_some_and(|distance| {
            distance.is_finite() && (preferred.is_some() || distance != 0.0)
        });
    ResizePass {
        reserve: preferred.is_some() || capture,
        capture,
    }
}

fn vertical_displacement(current: Option<egui::Pos2>, origin: Option<egui::Pos2>) -> Option<f32> {
    let (current, origin) = current.zip(origin)?;
    if !current.is_finite() || !origin.is_finite() {
        return None;
    }
    let distance = current.y - origin.y;
    distance.is_finite().then_some(distance)
}

fn captured_height(inner: f32, margin: f32) -> Option<f32> {
    let outer = inner + margin;
    (inner.is_finite() && inner >= 0.0 && outer.is_finite() && outer >= 0.0)
        .then(|| outer.clamp(150.0, 500.0))
}

fn display_height(preferred: f32, available: f32) -> f32 {
    preferred.clamp(150.0, 500.0).min(available.max(0.0))
}

pub(super) fn prepare(ctx: &egui::Context, preferred: Option<f32>) -> ResizePass {
    let response = ctx.read_response(native_resize_id());
    let (origin, primary_only) = ctx.input(|input| {
        let primary_only = input.pointer.primary_down()
            && ![
                egui::PointerButton::Secondary,
                egui::PointerButton::Middle,
                egui::PointerButton::Extra1,
                egui::PointerButton::Extra2,
            ]
            .iter()
            .any(|button| input.pointer.button_down(*button));
        (input.pointer.press_origin(), primary_only)
    });
    let owned_primary_drag = primary_only
        && response
            .as_ref()
            .is_some_and(|response| response.dragged_by(egui::PointerButton::Primary));
    let vertical = vertical_displacement(
        response
            .as_ref()
            .and_then(|response| response.interact_pointer_pos()),
        origin,
    );
    let pass = resize_pass(preferred, owned_primary_drag, vertical);

    if let Some(preferred) = preferred {
        // PanelState contains egui's last content-sized rectangle. Only explicit
        // mode replaces it, anchored to this pass's available bottom edge.
        // A temporary window clamp must not replace the user's preference.
        let mut rect = ctx.available_rect();
        rect.min.y = rect.max.y - display_height(preferred, rect.height());
        ctx.data_mut(|data| data.insert_persisted(panel_id(), PanelState { rect }));
    }
    pass
}

impl ResizePass {
    /// Call first in the panel closure, before content can expand its geometry.
    pub(super) fn reserve_and_capture(self, ui: &mut egui::Ui, frame: &egui::Frame) -> Option<f32> {
        if !self.reserve {
            return None;
        }
        let inner_height = ui.max_rect().height();
        let outer_height = captured_height(inner_height, frame.total_margin().sum().y)?;
        ui.set_min_height(inner_height);
        self.capture.then_some(outer_height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_owned_primary_vertical_intent_enters_explicit_mode() {
        for vertical in [
            None,
            Some(0.0),
            Some(f32::NAN),
            Some(f32::INFINITY),
            Some(f32::NEG_INFINITY),
        ] {
            let pass = resize_pass(None, true, vertical);
            assert!(!pass.reserve && !pass.capture);
        }
        for vertical in [Some(-1.0), Some(1.0)] {
            let passive = resize_pass(None, false, vertical);
            assert!(!passive.reserve && !passive.capture);
            let deliberate = resize_pass(None, true, vertical);
            assert!(deliberate.reserve && deliberate.capture);
        }
    }

    #[test]
    fn explicit_mode_captures_every_owned_drag_pass_even_back_at_origin() {
        for vertical in [Some(0.0), Some(-20.0), Some(20.0)] {
            let dragging = resize_pass(Some(360.0), true, vertical);
            assert!(dragging.reserve && dragging.capture);
            let released = resize_pass(Some(360.0), false, vertical);
            assert!(released.reserve && !released.capture);
        }
    }

    #[test]
    fn malformed_pointer_and_allocation_never_replace_the_preference() {
        let valid = egui::pos2(10.0, 20.0);
        for point in [
            None,
            Some(egui::pos2(f32::NAN, 20.0)),
            Some(egui::pos2(10.0, f32::INFINITY)),
        ] {
            assert_eq!(vertical_displacement(point, Some(valid)), None);
            assert_eq!(vertical_displacement(Some(valid), point), None);
        }
        for vertical in [None, Some(f32::NAN), Some(f32::INFINITY)] {
            let pass = resize_pass(Some(360.0), true, vertical);
            assert!(pass.reserve && !pass.capture);
        }
        for (inner, margin) in [
            (f32::NAN, 24.0),
            (-1.0, 24.0),
            (200.0, f32::INFINITY),
            (20.0, -30.0),
        ] {
            assert_eq!(captured_height(inner, margin), None);
        }
        assert_eq!(captured_height(300.0, 24.0), Some(324.0));
        assert_eq!(captured_height(100.0, 24.0), Some(150.0));
        assert_eq!(captured_height(600.0, 24.0), Some(500.0));
    }

    #[test]
    fn window_clamp_is_only_a_display_height() {
        assert_eq!(display_height(400.0, 900.0), 400.0);
        assert_eq!(display_height(400.0, 250.0), 250.0);
        assert_eq!(display_height(400.0, 900.0), 400.0);
        assert_eq!(display_height(100.0, 900.0), 150.0);
        assert_eq!(display_height(600.0, 900.0), 500.0);
        assert_eq!(display_height(400.0, 100.0), 100.0);
        assert_eq!(display_height(400.0, -10.0), 0.0);
    }
}
