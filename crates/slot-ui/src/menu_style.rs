use crate::plate::UndoFace;
use crate::text;

pub const MENU_MUTED_INK: [u8; 3] = [0xa7, 0xa2, 0x9f];
pub const MENU_TEXT_INK: [u8; 3] = [0xf0, 0xee, 0xe9];
pub const MENU_BACKGROUND: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
pub const SHELF_PINK: [u8; 3] = [0xe4, 0xcd, 0xcb];
pub const MENU_SELECTION_INK: [u8; 3] = [0xd4, 0x8c, 0x9e];

/// A label with a logical size for the layout and a native-size raster.
pub fn menu_text_face(
    label: &str,
    px: u32,
    room: u32,
    scale: [f32; 2],
    ink: [u8; 3],
) -> (UndoFace, [u32; 2]) {
    menu_line_face(&label.to_lowercase(), px, room, scale, ink)
}

fn menu_line_face(
    label: &str, px: u32, room: u32, [sx, sy]: [f32; 2], ink: [u8; 3],
) -> (UndoFace, [u32; 2]) {
    let Some(font) = text::label_font() else {
        return (
            UndoFace {
                rgba: Vec::new(),
                w: 0,
                h: 0,
            },
            [0; 2],
        );
    };
    let px = px as f32;
    let mut layout = text::menu_line(font, label, room as f32, px);
    let Some(line) = layout.lines.first_mut() else {
        return (
            UndoFace {
                rgba: Vec::new(),
                w: 0,
                h: 0,
            },
            [0; 2],
        );
    };
    if *line
        != label
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    {
        line.push('…');
        while text::line_width(font, line, px, layout.tracking) > room as f32 && *line != "…" {
            line.pop();
            line.pop();
            line.push('…');
        }
    }
    let width = text::line_width(font, line, px, layout.tracking).ceil() as u32 + 4;
    let height = (px * 1.4).ceil() as u32;
    let (w, h) = (
        (width as f32 * sx).round() as u32,
        (height as f32 * sy).round() as u32,
    );
    let mut rgba = vec![0; (w * h * 4) as usize];
    text::in_font(font, || text::draw_centred(&mut rgba, w, h, &layout.scaled(sy), ink));
    (UndoFace { rgba, w, h }, [width, height])
}

/// Natural menu text width before ellipsizing. Used only to decide whether
/// Now Playing needs the animated horizontal title viewport.
pub fn menu_marquee_width(label: &str, px: u32) -> f32 {
    let Some(font) = text::label_font() else { return 0.0 };
    let tracking = (px as f32 * 0.10).round();
    text::line_width(font, &label.to_lowercase(), px as f32, tracking) + 8.0
}

/// Rasterize the entire Now Playing title once per song/scale change.
/// The caller crops a narrow viewport from these pixels on every animation
/// frame rather than rasterizing every glyph again. Cap malformed metadata.
pub fn menu_marquee_whole_face(
    label: &str, px: u32, [sx, sy]: [f32; 2], ink: [u8; 3],
) -> UndoFace {
    let logical_w = menu_marquee_width(label, px).ceil().clamp(1.0, 8192.0);
    let logical_h = (px as f32 * 1.4).ceil();
    let w = (logical_w * sx).round().max(1.0) as u32;
    let h = (logical_h * sy).round().max(1.0) as u32;
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    text::draw_scrolling_line(
        &mut rgba, w, h, &label.to_lowercase(),
        px as f32 * sy, (px as f32 * 0.10).round() * sy,
        4.0 * sx, ink,
    );
    UndoFace { rgba, w, h }
}

/// An unclipped song title rendered *inside* a fixed, width-bounded viewport.
/// Scroll offset is in logical pixels. Only the viewport image is allocated,
/// not a giant texture spanning the full metadata string.
pub fn menu_marquee_face(
    label: &str, px: u32, room: u32, [sx, sy]: [f32; 2],
    ink: [u8; 3], scroll: f32,
) -> (UndoFace, [u32; 2]) {
    let logical_h = (px as f32 * 1.4).ceil() as u32;
    let w = (room as f32 * sx).round().max(1.0) as u32;
    let h = (logical_h as f32 * sy).round().max(1.0) as u32;
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    let physical_px = px as f32 * sy;
    let tracking = (px as f32 * 0.10).round() * sy;
    text::draw_scrolling_line(
        &mut rgba, w, h, &label.to_lowercase(), physical_px,
        tracking, 4.0 * sx - scroll * sx, ink,
    );
    (UndoFace { rgba, w, h }, [room, logical_h])
}

#[cfg(test)]
mod marquee_tests {
    use super::*;

    #[test]
    fn full_title_raster_matches_the_previous_first_viewport() {
        let title = "매우 긴 제목 — this title keeps scrolling smoothly";
        let room = 200;
        let whole = menu_marquee_whole_face(
            title, 26, [1.0, 1.0], MENU_SELECTION_INK,
        );
        let (old, _) = menu_marquee_face(
            title, 26, room, [1.0, 1.0], MENU_SELECTION_INK, 0.0,
        );
        assert!(whole.w > room);
        assert_eq!(whole.h, old.h);
        for y in 0..whole.h {
            let old_start = (y * room * 4) as usize;
            let whole_start = (y * whole.w * 4) as usize;
            let len = (room * 4) as usize;
            assert_eq!(
                &whole.rgba[whole_start..whole_start + len],
                &old.rgba[old_start..old_start + len],
                "cached title crop must retain the same glyph placement"
            );
        }
    }

    #[test]
    fn long_mixed_korean_titles_render_inside_bounded_viewport() {
        let title = "아주 긴 노래 제목 a very long English song title";
        let natural = menu_marquee_width(title, 26);
        assert!(natural > 200.0);
        let (at_start, size) =
            menu_marquee_face(title, 26, 200, [1.0, 1.0], MENU_SELECTION_INK, 0.0);
        let (scrolled, size2) =
            menu_marquee_face(title, 26, 200, [1.0, 1.0], MENU_SELECTION_INK, 135.0);
        assert_eq!(size, [200, 37]);
        assert_eq!(size2, size);
        assert_eq!((at_start.w, at_start.h), (200, 37));
        assert_eq!(at_start.rgba.len(), (200 * 37 * 4) as usize);
        assert_ne!(at_start.rgba, scrolled.rgba, "scrolling reveals a different span of text");
    }
}

pub fn menu_hint_face_scaled(key: &str, label: &str, scale: [f32; 2]) -> (UndoFace, u32) {
    crate::plate::hint_face_scaled(key, label, scale, text::Case::Lower)
}
