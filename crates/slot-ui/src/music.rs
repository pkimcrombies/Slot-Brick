use crate::plate::UndoFace;
use crate::power_menu::MENU_INK;
use crate::text::{self, Case};

/// A lowercase music label with a logical size for the layout and a native-size raster.
pub fn music_text_face(
    label: &str,
    px: u32,
    room: u32,
    [sx, sy]: [f32; 2],
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
    let mut layout = Case::Lower.fit_line(font, label, room as f32, px, px);
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
            .to_lowercase()
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
    text::draw_centred(&mut rgba, w, h, &layout.scaled(sy), MENU_INK);
    (UndoFace { rgba, w, h }, [width, height])
}
