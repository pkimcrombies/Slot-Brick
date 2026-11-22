use slot_gfx::{Draw, TexId};
use slot_power::{Battery, Charge};

use crate::footer::{draw_status_text, status_sprite, Printed};
use crate::hud::HUD_INK;
use crate::plate::HINT_H;

pub const BOLT_PX: f32 = 18.0;

pub const GAUGE_W: f32 = 22.0;
pub const GAUGE_H: f32 = 11.0;
const _: () = assert!(GAUGE_H < GAUGE_W);
const NUB_W: f32 = 2.5;
const NUB_H: f32 = 4.0;
pub const WALL: f32 = 1.5;
const GAP: f32 = 7.0;

const BOLT_W: f32 = crate::StatusIcon::Charging.size()[0] as f32;
const BOLT_H: f32 = crate::StatusIcon::Charging.size()[1] as f32;
const BOLT_GAP: f32 = 5.0;
const _: () = assert!(BOLT_GAP > 0.0);

const INK: [f32; 4] = [
    HUD_INK[0] as f32 / 255.0,
    HUD_INK[1] as f32 / 255.0,
    HUD_INK[2] as f32 / 255.0,
    1.0,
];

/// Where the gauge and its percent end, drawn from `x`: what goes beside them starts there.
pub fn gauge_end(x: f32, percent: Printed) -> f32 {
    x + BOLT_W + BOLT_GAP + GAUGE_W + NUB_W + GAP + percent.w as f32
}

pub fn draw_gauge(
    x: f32,
    y: f32,
    battery: Option<Battery>,
    percent: Printed,
    bolt: Option<TexId>,
    out: &mut Vec<Draw>,
) {
    let Some(b) = battery else {
        return;
    };

    let rect = |x: f32, y: f32, w: f32, h: f32, out: &mut Vec<Draw>| {
        out.push(Draw::Rect {
            x,
            y,
            w,
            h,
            colour: INK,
        });
    };

    let cx = x + BOLT_W + BOLT_GAP;

    rect(cx, y, GAUGE_W, WALL, out);
    rect(cx, y + GAUGE_H - WALL, GAUGE_W, WALL, out);
    rect(cx, y, WALL, GAUGE_H, out);
    rect(cx + GAUGE_W - WALL, y, WALL, GAUGE_H, out);
    rect(cx + GAUGE_W, y + (GAUGE_H - NUB_H) / 2.0, NUB_W, NUB_H, out);

    let inner = GAUGE_W - 4.0 * WALL;
    let fill = inner * f32::from(b.percent.min(100)) / 100.0;
    if fill > 0.0 {
        rect(
            cx + 2.0 * WALL,
            y + 2.0 * WALL,
            fill,
            GAUGE_H - 4.0 * WALL,
            out,
        );
    }

    if let (Charge::Charging, Some(tex)) = (b.charge, bolt) {
        out.push(Draw::Tex {
            x,
            y: y + (GAUGE_H - BOLT_H) / 2.0,
            w: BOLT_W,
            h: BOLT_H,
            tex,
            alpha: 1.0,
        });
    }

    if percent.w > 0 {
        let px = cx + GAUGE_W + NUB_W + GAP;
        let py = y + (GAUGE_H - HINT_H as f32) / 2.0;
        out.push(match percent.face {
            Some(tex) => Draw::Tex {
                x: px,
                y: py,
                w: percent.w as f32,
                h: HINT_H as f32,
                tex,
                alpha: 1.0,
            },
            None => Draw::Rect {
                x: px,
                y: py,
                w: percent.w as f32,
                h: HINT_H as f32,
                colour: [1.0, 1.0, 1.0, 0.08],
            },
        });
    }
}


// Android's top-right battery status, with the gauge, charging bolt and
// percentage ending at a fixed right margin. Unlike the legacy footer gauge,
// the bolt takes space only while charging.
const STATUS_BOLT_TEXT_GAP: f32 = 3.0;

pub fn status_gauge_width(battery: Option<Battery>, percent: Printed) -> f32 {
    let Some(b) = battery else { return 0.0 };
    let between = if b.charge == Charge::Charging {
        BOLT_GAP + BOLT_W + STATUS_BOLT_TEXT_GAP
    } else {
        GAP
    };
    GAUGE_W + NUB_W + between + percent.w as f32
}

pub fn draw_status_gauge(
    right: f32,
    y: f32,
    battery: Option<Battery>,
    percent: Printed,
    bolt: Option<TexId>,
    ink: [f32; 4],
    scale: [f32; 2],
    out: &mut Vec<Draw>,
) {
    let Some(b) = battery else { return };
    let x = right - status_gauge_width(battery, percent);
    let rect = |x, y, w, h, out: &mut Vec<Draw>| {
        out.push(Draw::Rect { x, y, w, h, colour: ink });
    };

    rect(x, y, GAUGE_W, WALL, out);
    rect(x, y + GAUGE_H - WALL, GAUGE_W, WALL, out);
    rect(x, y, WALL, GAUGE_H, out);
    rect(x + GAUGE_W - WALL, y, WALL, GAUGE_H, out);
    rect(x + GAUGE_W, y + (GAUGE_H - NUB_H) / 2.0, NUB_W, NUB_H, out);

    let fill = (GAUGE_W - 4.0 * WALL) * f32::from(b.percent.min(100)) / 100.0;
    if fill > 0.0 {
        rect(x + 2.0 * WALL, y + 2.0 * WALL, fill, GAUGE_H - 4.0 * WALL, out);
    }

    if b.charge == Charge::Charging {
        let bx = x + GAUGE_W + NUB_W + BOLT_GAP;
        if let Some(tex) = bolt {
            out.push(status_sprite(
                tex,
                [bx, y + (GAUGE_H - BOLT_H) / 2.0],
                [BOLT_W, BOLT_H],
                scale,
            ));
        }
    }

    if percent.face.is_some() {
        draw_status_text(
            right - percent.w as f32,
            y + (GAUGE_H - HINT_H as f32) / 2.0,
            percent,
            scale,
            out,
        );
    }
}

#[cfg(test)]
mod status_tests {
    use super::*;

    #[test]
    fn status_width_excludes_bolt_when_not_charging() {
        let p = Printed { face: None, w: 35 };
        let discharging = Some(Battery { percent: 70, charge: Charge::Discharging });
        let charging = Some(Battery { percent: 70, charge: Charge::Charging });
        assert_eq!(status_gauge_width(None, p), 0.0);
        assert_eq!(status_gauge_width(discharging, p), GAUGE_W + NUB_W + GAP + 35.0);
        assert_eq!(
            status_gauge_width(charging, p),
            GAUGE_W + NUB_W + BOLT_GAP + BOLT_W + STATUS_BOLT_TEXT_GAP + 35.0
        );
    }

    #[test]
    fn status_gauge_stays_inside_right_margin_and_is_black() {
        let battery = Some(Battery { percent: 70, charge: Charge::Discharging });
        let p = Printed { face: None, w: 35 };
        let right = 696.0;
        let mut out = Vec::new();
        draw_status_gauge(right, 18.5, battery, p, None, [0.0, 0.0, 0.0, 1.0], [1.0, 1.0], &mut out);
        assert!(matches!(out.first(), Some(Draw::Rect { x, y, colour, .. })
            if (*x - (right - status_gauge_width(battery, p))).abs() < 0.01
            && (*y - 18.5).abs() < 0.01
            && *colour == [0.0, 0.0, 0.0, 1.0]));
    }
}
