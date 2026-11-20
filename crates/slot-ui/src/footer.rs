use slot_gfx::{Draw, TexId, OUT_H, OUT_W};
use slot_power::Battery;

use crate::battery::{draw_gauge, draw_status_gauge, status_gauge_width, GAUGE_H};
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


/// Android-style shelf status line, 12px from the top and 24px from both edges.
/// The clock is black at top-left; black battery, connected Wi-Fi and update
/// indicators line up at top-right. The rest of the shelf stays unobstructed.
const STATUS_TOP_Y: f32 = 12.0;
const STATUS_MARGIN: f32 = 24.0;

pub fn draw_status_bar(
    battery: Option<Battery>,
    percent: Printed,
    bolt: Option<TexId>,
    wifi: Option<(TexId, u32, u32)>,
    update: Option<(TexId, u32, u32)>,
    clock: Printed,
    out: &mut Vec<Draw>,
) {
    // Clock uses a dedicated black word texture; don't recolour global HUD text.
    draw_printed(STATUS_MARGIN, STATUS_TOP_Y, clock, out);

    let battery_right = OUT_W as f32 - STATUS_MARGIN;
    let battery_top = STATUS_TOP_Y + (HINT_H as f32 - GAUGE_H) / 2.0;
    draw_status_gauge(
        battery_right,
        battery_top,
        battery,
        percent,
        bolt,
        [0.0, 0.0, 0.0, 1.0],
        out,
    );

    let mut icon_right = battery_right - status_gauge_width(battery, percent);
    for (tex, w, h) in [wifi, update].into_iter().flatten() {
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
        draw_status_bar(None, Printed::default(), None, None, None, clock, &mut out);
        assert!(matches!(out.first(), Some(Draw::Rect { x, y, w, h, .. })
            if *x == STATUS_MARGIN && *y == STATUS_TOP_Y
            && *w == 50.0 && *h == HINT_H as f32));
    }

    #[test]
    fn disconnected_wifi_slot_is_empty() {
        let mut out = Vec::new();
        draw_status_bar(None, Printed::default(), None, None, None, Printed::default(), &mut out);
        assert!(out.is_empty());
    }
}
