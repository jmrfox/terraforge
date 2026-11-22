//! Viewport math for zooming the map preview.

/// Scroll offset along one axis that keeps the image point under `anchor` in place.
///
/// `anchor` is measured from the start of the view. Images smaller than the view are
/// centered in it, so the margin on each side is part of the calculation.
pub fn anchored_scroll_offset(
    offset: f32,
    anchor: f32,
    view_len: f32,
    image_len: f32,
    old_zoom: f32,
    new_zoom: f32,
) -> f32 {
    let margin = |zoom: f32| ((view_len - image_len * zoom) * 0.5).max(0.0);
    let image_point = (offset + anchor - margin(old_zoom)) / old_zoom;
    let new_offset = image_point * new_zoom + margin(new_zoom) - anchor;
    new_offset.clamp(0.0, (image_len * new_zoom - view_len).max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Image coordinate shown at `anchor` for a given offset and zoom.
    fn image_point(offset: f32, anchor: f32, view: f32, image: f32, zoom: f32) -> f32 {
        let margin = ((view - image * zoom) * 0.5).max(0.0);
        (offset + anchor - margin) / zoom
    }

    #[test]
    fn keeps_point_under_cursor_fixed() {
        let (view, image, anchor, offset) = (400.0, 512.0, 130.0, 220.0);
        let before = image_point(offset, anchor, view, image, 2.0);
        let new_offset = anchored_scroll_offset(offset, anchor, view, image, 2.0, 3.0);
        let after = image_point(new_offset, anchor, view, image, 3.0);
        assert!((before - after).abs() < 1e-3, "{before} vs {after}");
    }

    #[test]
    fn keeps_point_fixed_when_image_grows_past_view() {
        let (view, image, anchor) = (800.0, 512.0, 500.0);
        let before = image_point(0.0, anchor, view, image, 1.0);
        let new_offset = anchored_scroll_offset(0.0, anchor, view, image, 1.0, 2.0);
        let after = image_point(new_offset, anchor, view, image, 2.0);
        assert!((before - after).abs() < 1e-3, "{before} vs {after}");
    }

    #[test]
    fn center_anchor_keeps_view_centered() {
        let new_offset = anchored_scroll_offset(312.0, 200.0, 400.0, 512.0, 2.0, 4.0);
        assert!((new_offset - 824.0).abs() < 1e-3, "{new_offset}");
    }

    #[test]
    fn offset_stays_within_scroll_range() {
        assert_eq!(
            anchored_scroll_offset(0.0, 0.0, 400.0, 512.0, 2.0, 1.0),
            0.0
        );
        let end = anchored_scroll_offset(624.0, 400.0, 400.0, 512.0, 2.0, 1.0);
        assert!((end - 112.0).abs() < 1e-3, "{end}");
        assert_eq!(
            anchored_scroll_offset(50.0, 100.0, 800.0, 512.0, 1.0, 0.5),
            0.0
        );
    }
}
