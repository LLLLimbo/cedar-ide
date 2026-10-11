//! Exact outer-viewport correction for an already requested footer reveal.
use eframe::egui;

/// Encode the smallest vertical reveal delta as egui's ordinary top-aligned
/// scroll target. The viewport uses the initial content origin, while egui
/// resolves the target against the content minimum after layout.
pub(super) fn footer_target(
    row: egui::Rect,
    viewport: egui::Rect,
    content_min_y: f32,
    viewport_offset_y: f32,
    item_spacing_y: f32,
) -> Option<egui::Rect> {
    let row_size = row.size();
    let viewport_size = viewport.size();
    if ![
        row.left(),
        row.top(),
        row.right(),
        row.bottom(),
        row_size.x,
        row_size.y,
        viewport.left(),
        viewport.top(),
        viewport.right(),
        viewport.bottom(),
        viewport_size.x,
        viewport_size.y,
        content_min_y,
        viewport_offset_y,
        item_spacing_y,
    ]
    .into_iter()
    .all(f32::is_finite)
        || row_size.x <= 0.0
        || row_size.y <= 0.0
        || viewport_size.x <= 0.0
        || viewport_size.y <= 0.0
        || row_size.y > viewport_size.y
    {
        return None;
    }

    let delta = if row.top() < viewport.top() {
        row.top() - viewport.top()
    } else if row.bottom() > viewport.bottom() {
        row.bottom() - viewport.bottom()
    } else {
        return None;
    };
    if !delta.is_finite() || delta == 0.0 {
        return None;
    }
    // TOP resolves to target.top() - content_min_y - item_spacing_y.
    // Include the captured viewport offset so only delta changes it.
    let top = content_min_y + viewport_offset_y + item_spacing_y + delta;
    let bottom = top + row_size.y;
    if !top.is_finite() || !bottom.is_finite() || bottom <= top {
        return None;
    }
    Some(egui::Rect::from_min_max(
        egui::pos2(row.left(), top),
        egui::pos2(row.right(), bottom),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(top: f32, bottom: f32) -> egui::Rect {
        egui::Rect::from_min_max(egui::pos2(24.0, top), egui::pos2(96.0, bottom))
    }

    #[test]
    fn fractional_deficits_keep_the_exact_minimum_reveal_delta() {
        let viewport = rect(12.375, 296.875);
        let content_min_y = -19.625;
        let offset = 32.0;
        let spacing = 3.0;
        for (row, delta) in [(rect(12.25, 32.25), -0.125), (rect(277.0, 297.0), 0.125)] {
            let target = footer_target(row, viewport, content_min_y, offset, spacing).unwrap();
            assert_eq!(target.top(), content_min_y + offset + spacing + delta);
            assert_eq!(target.top() - content_min_y - offset - spacing, delta);
            assert_eq!(target.height(), row.height());
            assert_eq!(target.x_range(), row.x_range());
        }
    }

    #[test]
    fn the_final_content_minimum_is_distinct_from_the_viewport_origin() {
        let initial_origin_y = -19.625;
        let content_min_y = -19.5;
        let offset = 32.0;
        let viewport = rect(initial_origin_y + offset, 296.875);
        let row = rect(277.0, 297.0);
        let target = footer_target(row, viewport, content_min_y, offset, 3.0).unwrap();
        assert_eq!(target.top(), 15.625);
        assert_eq!(target.top() - content_min_y - offset - 3.0, 0.125);
        assert_ne!(target.top(), viewport.top() + 3.0 + 0.125);
    }

    #[test]
    fn contained_or_too_tall_rows_keep_the_original_request() {
        let viewport = rect(12.375, 296.875);
        for row in [
            rect(12.375, 32.375),
            rect(276.875, 296.875),
            viewport,
            rect(12.25, 297.0),
        ] {
            assert_eq!(footer_target(row, viewport, -19.625, 32.0, 3.0), None);
        }
    }

    #[test]
    fn invalid_or_nonfinite_geometry_keeps_the_original_request() {
        let viewport = rect(12.375, 296.875);
        let row = rect(277.0, 297.0);
        let invalid_rects = [
            rect(0.0, 0.0),
            rect(1.0, 0.0),
            egui::Rect::from_min_max(egui::pos2(24.0, 0.0), egui::pos2(24.0, 20.0)),
            rect(f32::NAN, 20.0),
            rect(0.0, f32::INFINITY),
            rect(f32::NEG_INFINITY, 20.0),
            rect(-f32::MAX, f32::MAX),
        ];
        for invalid in invalid_rects {
            assert_eq!(footer_target(invalid, viewport, 0.0, 0.0, 3.0), None);
            assert_eq!(footer_target(row, invalid, 0.0, 0.0, 3.0), None);
        }
        for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(footer_target(row, viewport, invalid, 32.0, 3.0), None);
            assert_eq!(footer_target(row, viewport, -19.625, invalid, 3.0), None);
            assert_eq!(footer_target(row, viewport, -19.625, 32.0, invalid), None);
        }
        assert_eq!(footer_target(row, viewport, f32::MAX, f32::MAX, 3.0), None);
        // Finite inputs may still lose the row's positive height at this scale.
        assert_eq!(footer_target(row, viewport, f32::MAX, 0.0, 3.0), None);
    }
}
