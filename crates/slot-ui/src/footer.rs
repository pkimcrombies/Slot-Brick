use slot_gfx::{Draw, TexId, OUT_H, OUT_W};
use slot_power::Battery;

use crate::battery::{draw_gauge, draw_status_gauge, status_gauge_width, GAUGE_H};
use crate::draw::pixel_aligned_texture;
use crate::plate::HINT_H;
use crate::slot_chrome::MOUTH_H;

const FOOTER_Y: f32 = OUT_H as f32 - MOUTH_H + (MOUTH_H - HINT_H as f32) / 2.0;
const FOOTER_MARGIN: f32 = 24.0;

#[derive(Copy, Clone, Default, PartialEq, Eq, Debug)]
pub struct Printed {
    pub face: Option<TexId>,
    pub w: u32,
}

impl Printed {
    pub fn new(face: TexId, w: u32) -> Self {
        Printed {
            face: Some(face),
            w,
        }
    }
}

/// How far the Wi-Fi symbol sits from the clock, and how big it is drawn.
const WIFI_GAP: f32 = 10.0;
pub const WIFI_PX: f32 = 18.0;

pub fn draw_footer(
    battery: Option<Battery>,
    percent: Printed,
    bolt: Option<TexId>,
    wifi: Option<(TexId, u32, u32)>,
    update: Option<(TexId, u32, u32)>,
    clock: Printed,
    out: &mut Vec<Draw>,
) {
    let y = FOOTER_Y + (HINT_H as f32 - GAUGE_H) / 2.0;
    draw_gauge(FOOTER_MARGIN, y, battery, percent, bolt, out);
    let clock_x = OUT_W as f32 - FOOTER_MARGIN - clock.w as f32;
    printed(clock_x, clock, out);
    // To the left of the clock: the two together say what the Brick is doing right now. A
    // newer Slot, when there is one, is told to the left of those.
    let mut right = clock_x;
    for (tex, w, h) in [wifi, update].into_iter().flatten() {
        right -= WIFI_GAP + w as f32;
        out.push(Draw::Tex {
            x: right.round(),
            y: (FOOTER_Y + (HINT_H as f32 - h as f32) / 2.0).round(),
            w: w as f32,
            h: h as f32,
            tex,
            alpha: 1.0,
        });
    }
}

/// How far a note sits from the battery's percent.
const NOTE_GAP: f32 = 22.0;

/// A note beside the battery's percent (the selected game's play time, on the shelf).
pub fn draw_footer_note(percent: Printed, note: Printed, out: &mut Vec<Draw>) {
    let x = crate::battery::gauge_end(FOOTER_MARGIN, percent) + NOTE_GAP;
    printed(x.round(), note, out);
}

pub(crate) fn draw_printed(x: f32, y: f32, p: Printed, out: &mut Vec<Draw>) {
    if p.w == 0 {
        return;
    }
    let (w, h) = (p.w as f32, HINT_H as f32);
    out.push(match p.face {
        Some(tex) => Draw::Tex {
            x,
            y,
            w,
            h,
            tex,
            alpha: 1.0,
        },
        None => Draw::Rect {
            x,
            y,
            w,
            h,
            colour: [1.0, 1.0, 1.0, 0.08],
        },
    });
}

fn printed(x: f32, p: Printed, out: &mut Vec<Draw>) {
    draw_printed(x, FOOTER_Y, p, out);
}

pub(crate) fn draw_status_text(x: f32, y: f32, p: Printed, scale: [f32; 2], out: &mut Vec<Draw>) {
    let Some(tex) = p.face.filter(|_| p.w > 0) else {
        draw_printed(x, y, p, out);
        return;
    };
    out.push(pixel_aligned_texture(tex, [x, y], [p.w as f32, HINT_H as f32], scale, 1.0));
}

/// Android-style shelf status line, 12px from the top and 24px from both edges.
/// The clock is black at top-left; black battery, connected Wi-Fi and update
/// indicators line up at top-right. The rest of the shelf stays unobstructed.
const STATUS_TOP_Y: f32 = 12.0;
pub const STATUS_MARGIN: f32 = 24.0;

pub fn draw_status_bar(
    battery: Option<Battery>,
    percent: Printed,
    bolt: Option<TexId>,
    wifi: Option<(TexId, u32, u32)>,
    update: Option<(TexId, u32, u32)>,
    clock: Printed,
    scale: [f32; 2],
    out: &mut Vec<Draw>,
) {
    draw_status_bar_with_lock(
        battery, percent, bolt, wifi, update, None, clock, scale, out,
    );
}

/// Same status HUD, with an optional Music-only lock glyph centred at the top.
/// The clock, battery, Wi-Fi and update icons remain at their Shelf positions.
pub fn draw_status_bar_with_lock(
    battery: Option<Battery>,
    percent: Printed,
    bolt: Option<TexId>,
    wifi: Option<(TexId, u32, u32)>,
    update: Option<(TexId, u32, u32)>,
    lock: Option<(TexId, u32, u32)>,
    clock: Printed,
    scale: [f32; 2],
    out: &mut Vec<Draw>,
) {
    // Clock uses a dedicated black word texture; don't recolour global HUD text.
    draw_status_text(STATUS_MARGIN, STATUS_TOP_Y, clock, scale, out);

    let battery_right = OUT_W as f32 - STATUS_MARGIN;
    let battery_top = STATUS_TOP_Y + (HINT_H as f32 - GAUGE_H) / 2.0;
    draw_status_gauge(
        battery_right,
        battery_top,
        battery,
        percent,
        bolt,
        [0.0, 0.0, 0.0, 1.0],
        scale,
        out,
    );

    // Keep the system status cluster anchored at the right, exactly as on Shelf.
    // Music Lock has its own centred position and must not push Wi-Fi or battery.
    let mut icon_right = battery_right - status_gauge_width(battery, percent);
    if let Some((tex, w, h)) = lock {
        let (w, h) = (w as f32, h as f32);
        let x = (OUT_W as f32 - w) / 2.0;
        let y = STATUS_TOP_Y + (HINT_H as f32 - h) / 2.0;
        out.push(pixel_aligned_texture(tex, [x, y], [w, h], scale, 1.0));
    }
    if let Some((tex, w, h)) = wifi {
        let (w, h) = (w as f32, h as f32);
        icon_right -= WIFI_GAP + w;
        let y = STATUS_TOP_Y + (HINT_H as f32 - h) / 2.0;
        out.push(pixel_aligned_texture(tex, [icon_right, y], [w, h], scale, 1.0));
    }
    if let Some((tex, w, h)) = update {
        icon_right -= WIFI_GAP + w as f32;
        out.push(Draw::Tex {
            x: icon_right.round(),
            y: (STATUS_TOP_Y + (HINT_H as f32 - h as f32) / 2.0).round(),
            w: w as f32,
            h: h as f32,
            tex,
            alpha: 1.0,
        });
    }
}

#[cfg(test)]
mod status_position_tests {
    use super::*;

    #[test]
    fn clock_starts_in_the_top_left_corner() {
        let mut out = Vec::new();
        let clock = Printed { face: None, w: 50 };
        draw_status_bar(None, Printed::default(), None, None, None, clock, [1.0, 1.0], &mut out);
        assert!(matches!(out.first(), Some(Draw::Rect { x, y, w, h, .. })
            if *x == STATUS_MARGIN && *y == STATUS_TOP_Y
            && *w == 50.0 && *h == HINT_H as f32));
    }

    #[test]
    fn music_lock_is_centered_and_does_not_shift_the_shared_hud() {
        let percent = Printed { face: None, w: 39 };
        let lock = TexId::from_raw(45);
        let wifi = TexId::from_raw(46);
        let update = TexId::from_raw(47);
        for scale in [[1.0, 1.0], [1.6, 1.6]] {
            for charge in [slot_power::Charge::Discharging, slot_power::Charge::Charging] {
                let battery = Some(Battery { percent: 91, charge });
                let mut regular = Vec::new();
                draw_status_bar(
                    battery, percent, None, Some((wifi, 20, 16)), Some((update, 18, 18)),
                    Printed::default(), scale, &mut regular,
                );
                let mut locked = Vec::new();
                draw_status_bar_with_lock(
                    battery, percent, None, Some((wifi, 20, 16)), Some((update, 18, 18)),
                    Some((lock, 18, 18)), Printed::default(), scale, &mut locked,
                );
                let (x, y, w, h) = locked.iter().find_map(|draw| match draw {
                    Draw::Tex { tex, x, y, w, h, .. } if *tex == lock => Some((*x, *y, *w, *h)),
                    _ => None,
                }).expect("Music Lock must have its own HUD glyph");
                assert!((x + w / 2.0 - OUT_W as f32 / 2.0).abs() <= 0.5 / scale[0] + 0.01,
                    "lock must be centred at scale {scale:?}");
                assert!(y >= STATUS_TOP_Y && y + h <= STATUS_TOP_Y + HINT_H as f32);
                assert!((x * scale[0] - (x * scale[0]).round()).abs() < 0.001);
                locked.retain(|draw| !matches!(draw, Draw::Tex { tex, .. } if *tex == lock));
                assert_eq!(locked, regular, "Music Lock must not move any other HUD element");
            }
        }
    }

    #[test]
    fn disconnected_wifi_slot_is_empty() {
        let mut out = Vec::new();
        draw_status_bar(None, Printed::default(), None, None, None, Printed::default(), [1.0, 1.0], &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn status_icons_align_to_the_display_pixel_grid() {
        let scale = [1.6, 1.6];
        let wifi = TexId::from_raw(12);
        let bolt = TexId::from_raw(13);
        let battery = Battery { percent: 70, charge: slot_power::Charge::Charging };
        let mut out = Vec::new();
        draw_status_bar(
            Some(battery), Printed { face: None, w: 35 }, Some(bolt), Some((wifi, 20, 16)),
            None, Printed::default(), scale, &mut out,
        );
        for (id, icon) in [(wifi, crate::StatusIcon::Wifi), (bolt, crate::StatusIcon::Charging)] {
            let (x, y, w, h) = out.iter().find_map(|&draw| match draw {
                Draw::Tex { x, y, w, h, tex, .. } if tex == id => Some((x, y, w, h)),
                _ => None,
            }).unwrap();
            for value in [x * scale[0], y * scale[1]] {
                assert!((value - value.round()).abs() < 0.001, "icon {id:?} at {value}");
            }
            let face = icon.face(scale);
            assert!((w * scale[0] - face.w as f32).abs() < 0.001);
            assert!((h * scale[1] - face.h as f32).abs() < 0.001);
        }
    }

    #[test]
    fn status_text_uses_one_texel_per_canvas_pixel() {
        let scale = [1.6, 1.6];
        let clock = TexId::from_raw(10);
        let percent = TexId::from_raw(11);
        let (clock_face, clock_width) = crate::word_face_ink_scaled("08:08", [0; 3], scale);
        let (percent_face, percent_width) = crate::word_face_ink_scaled("100%", [0; 3], scale);
        let battery = Battery { percent: 100, charge: slot_power::Charge::Discharging };
        let mut out = Vec::new();
        draw_status_bar(
            Some(battery), Printed::new(percent, percent_width), None, None, None,
            Printed::new(clock, clock_width), scale, &mut out,
        );
        for (id, face) in [(clock, clock_face), (percent, percent_face)] {
            let (x, y, w, h) = out.iter().find_map(|&draw| match draw {
                Draw::Tex { x, y, w, h, tex, .. } if tex == id => Some((x, y, w, h)),
                _ => None,
            }).unwrap();
            for coordinate in [x * scale[0], y * scale[1]] {
                assert!((coordinate - coordinate.round()).abs() < 0.001);
            }
            assert!((w * scale[0] - face.w as f32).abs() < 0.001);
            assert!((h * scale[1] - face.h as f32).abs() < 0.001);
        }
    }
}
