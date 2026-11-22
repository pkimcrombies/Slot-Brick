use crate::draw::{pixel_aligned_texture, Draw, TexId, OUT_W};
use crate::plate::{UndoFace, HINT_H, LEGEND_GAP, LEGEND_Y};
use crate::text;

pub const MENU_ROSE_INK: [u8; 3] = [0xe4, 0xcd, 0xcb];
pub const MENU_MUTED_INK: [u8; 3] = [0xa7, 0xa2, 0x9f];
pub const MENU_TEXT_INK: [u8; 3] = [0xf0, 0xee, 0xe9];
pub const MENU_BACKGROUND: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

pub fn draw_menu_cursor(out: &mut Vec<Draw>, x: f32, centre_y: f32) {
    let [r, g, b] = MENU_ROSE_INK.map(|v| f32::from(v) / 255.0);
    out.push(Draw::Rect {
        x, y: centre_y - 7.0, w: 3.0, h: 14.0, colour: [r, g, b, 1.0],
    });
}

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
    let Some(font) = text::menu_font() else {
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

pub fn menu_hint_face_scaled(key: &str, label: &str, scale: [f32; 2]) -> (UndoFace, u32) {
    let text = format!("{key}  {}", label.to_lowercase());
    // 17 × 1.4 rounds up to the shared 24-pixel hint height.
    let (face, [width, _]) = menu_line_face(&text, 17, OUT_W, scale, MENU_TEXT_INK);
    (face, width)
}

pub fn draw_menu_hints(out: &mut Vec<Draw>, hints: &[(TexId, u32)], x: f32, scale: [f32; 2]) {
    let Some((&(back, width), actions)) = hints.split_first() else { return };
    let mut draw = |tex, x, width| {
        out.push(pixel_aligned_texture(tex, [x, LEGEND_Y], [width as f32, HINT_H as f32], scale, 1.0));
    };
    draw(back, x, width);
    let mut right = OUT_W as f32 - x;
    for &(tex, width) in actions.iter().rev() {
        right -= width as f32;
        draw(tex, right, width);
        right -= LEGEND_GAP;
    }
}
