//! Disc faces for the shelf. Scans supply the print inside Slot's own hub and rim.
use std::sync::OnceLock;

use slot_store::{Cart, Platform};

use crate::cart::{label_colour, label_text, CartFace};
use crate::{art, text};

/// A disc's width and height on the shelf, at full size.
pub const DISC_PX: u32 = 300;
/// Radii are fractions of the face's outer radius. The PS1 hole is 15 mm on a 120 mm CD.
const HOLE: f32 = 0.125;
const RIM: f32 = 0.985;
const PS1_HUB: f32 = 0.21;
const PS1_PRINT_INNER: f32 = 0.235;
const PS1_PRINT_OUTER: f32 = 0.955;
const PS1_EDGE: [u8; 3] = [0x76, 0x78, 0x81];
const UMD_CENTER_Y: f32 = 148.0;
const UMD_HUB_RADIUS: f32 = 65.5;
const UMD_PRINT_INNER: f32 = 68.5;
const UMD_PRINT_OUTER: f32 = 137.5;
const UMD_DISC_RADIUS: f32 = 141.0;
const UMD_HUB: [u8; 3] = [0xf1, 0xf0, 0xed];
const UMD_SHELL_EDGE: [u8; 3] = [0xbc, 0xbc, 0xc0];
const UMD_MARK_BOX: (f32, f32, f32, f32) = (0.29, 0.43, 0.42, 0.14);
const EDGE_SAMPLES: [(f32, f32); 4] = [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)];

const SILVER: [u8; 3] = [0xc9, 0xca, 0xce];
const HUB_GREY: [u8; 3] = [0xe2, 0xe4, 0xe8];
const HUB_ALPHA: f32 = 0.55;
/// The title's box: across the print above the hub.
const TITLE_BOX: (f32, f32, f32, f32) = (0.18, 0.12, 0.64, 0.22);
const UMD_TITLE_BOX: (f32, f32, f32, f32) = (0.22, 0.085, 0.56, 0.18);
const TITLE_LINES: usize = 2;
const TITLE_PX: f32 = 28.0;
const TITLE_MIN_PX: f32 = 12.0;
/// "DISC 2": under the hub.
const NUMBER_BOX: (f32, f32, f32, f32) = (0.3, 0.7, 0.4, 0.1);
const NUMBER_PX: f32 = 18.0;
/// The tag a disc number sits on over a disc's own art.
const TAG: [u8; 3] = [0x1a, 0x18, 0x16];
const TAG_ALPHA: f32 = 0.75;

/// The disc's face: `art` (DISC_PX square, RGBA) printed on it when given, else a disc in the
/// game's own colour with its name across the top, and which disc it is under the hub for a
/// game of several (`number`, from 1).
pub fn disc_face(cart: &Cart, art: Option<Vec<u8>>, number: Option<usize>) -> CartFace {
    let n = DISC_PX;
    let art = art.as_deref().filter(|pixels| pixels.len() == (n * n * 4) as usize);
    if cart.platform == Platform::Psp {
        return umd_face(cart, art);
    }
    let title = label_text(cart);
    let print = label_colour(&title);
    let mut rgba = vec![0u8; (n * n * 4) as usize];
    let r = n as f32 / 2.0;
    for y in 0..n {
        for x in 0..n {
            let o = ((y * n + x) * 4) as usize;
            let template_print = print_pixel(art, o, print);
            // Four samples a pixel, so every edge between the rings is smooth.
            let (mut sum, mut cover) = ([0.0f32; 3], 0.0f32);
            for (sx, sy) in EDGE_SAMPLES {
                let d = ((x as f32 + sx - r).powi(2) + (y as f32 + sy - r).powi(2)).sqrt() / r;
                if !(HOLE..=1.0).contains(&d) {
                    continue;
                }
                let (colour, alpha) = ps1_surface(d, template_print);
                for c in 0..3 {
                    sum[c] += colour[c] as f32 * alpha;
                }
                cover += alpha;
            }
            if cover > 0.0 {
                for c in 0..3 {
                    rgba[o + c] = (sum[c] / cover).round() as u8;
                }
                rgba[o + 3] = (cover / 4.0 * 255.0).round() as u8;
            } else {
                // Match the cut edge in transparent texels so linear filtering
                // doesn't introduce a dark fringe around the rim or hole.
                let dx = x as f32 + 0.5 - r;
                let dy = y as f32 + 0.5 - r;
                let padding = if dx * dx + dy * dy < (HOLE * r).powi(2) {
                    HUB_GREY
                } else {
                    PS1_EDGE
                };
                rgba[o..o + 3].copy_from_slice(&padding);
            }
        }
    }
    match (art, number) {
        (None, number) => {
            print_title(&mut rgba, n, &title, print, TITLE_BOX, TITLE_LINES, TITLE_PX);
            if let Some(number) = number {
                print_title(&mut rgba, n, &format!("DISC {number}"), print, NUMBER_BOX, 1, NUMBER_PX);
            }
        }
        // A scan prints its own number, but which scan is which disc is not known: a tag says.
        (Some(_), Some(number)) => {
            tag(&mut rgba, n, NUMBER_BOX);
            print_title(&mut rgba, n, &format!("DISC {number}"), TAG, NUMBER_BOX, 1, NUMBER_PX);
        }
        (Some(_), None) => {}
    }
    CartFace { rgba, w: n, h: n }
}

fn print_pixel(art: Option<&[u8]>, offset: usize, fallback: [u8; 3]) -> [u8; 3] {
    art.map_or(fallback, |pixels| {
        let pixel = &pixels[offset..offset + 4];
        let a = u16::from(pixel[3]);
        std::array::from_fn(|c| {
            ((u16::from(pixel[c]) * a + u16::from(SILVER[c]) * (255 - a) + 127) / 255) as u8
        })
    })
}

fn ps1_surface(radius: f32, print: [u8; 3]) -> ([u8; 3], f32) {
    if radius < PS1_HUB {
        (HUB_GREY, HUB_ALPHA)
    } else if radius < PS1_PRINT_INNER {
        (SILVER, 1.0)
    } else if radius < PS1_PRINT_OUTER {
        (print, 1.0)
    } else if radius < RIM {
        let bevel = (radius - PS1_PRINT_OUTER) / (RIM - PS1_PRINT_OUTER);
        (shade(SILVER, 1.14 - 0.25 * bevel), 1.0)
    } else {
        (PS1_EDGE, 1.0)
    }
}

fn umd_shell() -> &'static [u8] {
    static SHELL: OnceLock<Vec<u8>> = OnceLock::new();
    SHELL.get_or_init(|| {
        // A domed top, straight sides and the broad, gently curved UMD base.
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 300 300" width="300" height="300">
          <path d="M150 1C232 1 299 68 299 150V246
            C299 258 291 268 276 275C235 292 194 299 150 299
            C106 299 65 292 24 275C9 268 1 258 1 246V150C1 68 68 1 150 1Z"
            fill="#eeedeb" stroke="#bcbcc0" stroke-width="1.5"/>
        </svg>"##;
        art::render_svg(svg, DISC_PX, DISC_PX).expect("valid UMD casing")
    })
}

fn umd_face(cart: &Cart, art: Option<&[u8]>) -> CartFace {
    let n = DISC_PX;
    let title = label_text(cart);
    let print = label_colour(&title);
    let mut rgba = umd_shell().to_vec();
    for (i, pixel) in rgba.chunks_exact_mut(4).enumerate() {
        if pixel[3] == 0 {
            pixel[..3].copy_from_slice(&UMD_SHELL_EDGE);
            continue;
        }
        let x = (i % n as usize) as f32 - n as f32 / 2.0;
        let y = (i / n as usize) as f32 - UMD_CENTER_Y;
        let base = [pixel[0], pixel[1], pixel[2]];
        let print = print_pixel(art, i * 4, print);
        let mut sum = [0u16; 3];
        for (sx, sy) in EDGE_SAMPLES {
            let radius = ((x + sx).powi(2) + (y + sy).powi(2)).sqrt();
            let colour = umd_surface(radius, print).unwrap_or(base);
            for c in 0..3 {
                sum[c] += u16::from(colour[c]);
            }
        }
        for c in 0..3 {
            pixel[c] = ((sum[c] + 2) / 4) as u8;
        }
    }
    if art.is_none() {
        print_title(&mut rgba, n, &title, print, UMD_TITLE_BOX, TITLE_LINES, TITLE_PX);
    }
    print_title(&mut rgba, n, "UMD", UMD_HUB, UMD_MARK_BOX, 1, 30.0);
    CartFace { rgba, w: n, h: n }
}

fn umd_surface(radius: f32, print: [u8; 3]) -> Option<[u8; 3]> {
    let colour = if radius < UMD_HUB_RADIUS {
        UMD_HUB
    } else if radius < UMD_PRINT_INNER {
        SILVER
    } else if radius < UMD_PRINT_OUTER {
        print
    } else if radius < UMD_DISC_RADIUS {
        let bevel = (radius - UMD_PRINT_OUTER) / (UMD_DISC_RADIUS - UMD_PRINT_OUTER);
        shade(SILVER, 1.08 - 0.32 * bevel)
    } else {
        return None;
    };
    Some(colour)
}

/// The disc's outline, black: what a disc on the shelf's edge, drawn see-through, is drawn on.
pub fn disc_shadow() -> CartFace {
    let n = DISC_PX;
    let r = n as f32 / 2.0;
    let mut rgba = Vec::with_capacity((n * n * 4) as usize);
    for y in 0..n {
        for x in 0..n {
            let d = ((x as f32 + 0.5 - r).powi(2) + (y as f32 + 0.5 - r).powi(2)).sqrt();
            let cover = (r - d + 0.5).clamp(0.0, 1.0) * (d - HOLE * r + 0.5).clamp(0.0, 1.0);
            rgba.extend_from_slice(&[0, 0, 0, (cover * 255.0).round() as u8]);
        }
    }
    CartFace { rgba, w: n, h: n }
}

pub fn umd_shadow() -> CartFace {
    let mut rgba = umd_shell().to_vec();
    for pixel in rgba.chunks_exact_mut(4) {
        pixel[..3].fill(0);
    }
    CartFace { rgba, w: DISC_PX, h: DISC_PX }
}

fn print_title(
    rgba: &mut [u8],
    n: u32,
    title: &str,
    print: [u8; 3],
    (bx, by, bw, bh): (f32, f32, f32, f32),
    lines: usize,
    px: f32,
) {
    let Some(font) = text::label_font() else {
        return;
    };
    let (x0, y0) = ((bx * n as f32) as u32, (by * n as f32) as u32);
    let (w, h) = ((bw * n as f32) as u32, (bh * n as f32) as u32);
    let layout = text::fit_box(font, title, w as f32, h as f32, lines, px, TITLE_MIN_PX);
    let mut block = vec![0u8; (w * h * 4) as usize];
    text::draw_centred(&mut block, w, h, &layout, ink(print));
    for y in 0..h {
        for x in 0..w {
            let s = ((y * w + x) * 4) as usize;
            let a = block[s + 3] as u32;
            if a == 0 {
                continue;
            }
            let d = (((y0 + y) * n + x0 + x) * 4) as usize;
            for c in 0..3 {
                rgba[d + c] = ((block[s + c] as u32 * a + rgba[d + c] as u32 * (255 - a)) / 255) as u8;
            }
        }
    }
}

/// A dark, rounded tag across `(bx, by, bw, bh)`, for words printed over a disc's own art.
fn tag(rgba: &mut [u8], n: u32, (bx, by, bw, bh): (f32, f32, f32, f32)) {
    let (x0, y0) = (bx * n as f32, by * n as f32);
    let (w, h) = (bw * n as f32, bh * n as f32);
    let r = h / 2.0;
    for y in y0 as u32..(y0 + h).ceil() as u32 {
        for x in x0 as u32..(x0 + w).ceil() as u32 {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let cx = px.clamp(x0 + r, x0 + w - r);
            let cover = (r - (px - cx).hypot(py - (y0 + r)) + 0.5).clamp(0.0, 1.0) * TAG_ALPHA;
            let o = ((y * n + x) * 4) as usize;
            if cover <= 0.0 || rgba[o + 3] == 0 {
                continue;
            }
            for c in 0..3 {
                rgba[o + c] = (rgba[o + c] as f32 * (1.0 - cover) + TAG[c] as f32 * cover) as u8;
            }
        }
    }
}

fn shade(c: [u8; 3], k: f32) -> [u8; 3] {
    c.map(|v| (v as f32 * k).round().clamp(0.0, 255.0) as u8)
}

fn ink(bg: [u8; 3]) -> [u8; 3] {
    let luma = 0.2126 * bg[0] as f32 + 0.7152 * bg[1] as f32 + 0.0722 * bg[2] as f32;
    if luma > 140.0 {
        [0x1a, 0x18, 0x16]
    } else {
        [0xf4, 0xf1, 0xea]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placeholder() -> Cart {
        Cart {
            platform: slot_store::Platform::Ps1,
            stem: "Disc Test".into(),
            rom: Default::default(),
            label: None,
            famicom_label: None,
            title: String::new(),
            code: String::new(),
            shell: None,
        }
    }

    fn alpha(face: &CartFace, x: u32, y: u32) -> u8 {
        face.rgba[((y * face.w + x) * 4 + 3) as usize]
    }

    #[test]
    fn a_disc_is_round_with_a_hole_in_the_middle() {
        let face = disc_face(&placeholder(), None, Some(2));
        let c = DISC_PX / 2;
        assert_eq!(alpha(&face, 0, 0), 0, "a corner is not clear");
        assert_eq!(alpha(&face, c, c), 0, "the hole is not clear");
        assert_eq!(alpha(&face, c, DISC_PX - 10), 255, "the print is not solid");
        assert!(alpha(&face, c, c + (0.2 * c as f32) as u32) < 255, "the hub is not see-through");
    }

    #[test]
    fn ps1_scan_supplies_print_and_the_template_supplies_the_disc() {
        let cart = placeholder();
        let c = DISC_PX / 2;
        let index = |x, y| ((y * DISC_PX + x) * 4) as usize;
        let ink = [44, 73, 178, 255];
        let mut scan = ink.repeat((DISC_PX * DISC_PX) as usize);
        let missing = index(c + 70, c);
        scan[missing + 3] = 0;
        let printed = disc_face(&cart, Some(scan), None);
        let plain = disc_face(&cart, None, None);
        for (x, y) in [(c, c + 23), (c, c + 30), (c, c + 146)] {
            let i = index(x, y);
            assert_eq!(&printed.rgba[i..i + 4], &plain.rgba[i..i + 4]);
        }
        let label = index(c, c + 70);
        assert_eq!(&printed.rgba[label..label + 4], &ink);
        assert_eq!(printed.rgba[missing + 3], 255, "a gap in the scan should not cut through the disc");
        assert_eq!(alpha(&printed, c, c), 0);
        assert_eq!(alpha(&printed, 0, 0), 0);
    }

    #[test]
    fn psp_scan_supplies_the_print_around_a_solid_umd_hub() {
        let mut cart = placeholder();
        cart.platform = Platform::Psp;
        let c = DISC_PX / 2;
        let index = |x, y| ((y * DISC_PX + x) * 4) as usize;
        let ink = [44, 73, 178, 255];
        let mut scan = ink.repeat((DISC_PX * DISC_PX) as usize);
        scan[index(c, c) + 3] = 0;
        let nick = index(c + 90, c);
        scan[nick + 3] = 0;
        let printed = disc_face(&cart, Some(scan), None);
        let plain = disc_face(&cart, None, None);
        assert_eq!(alpha(&printed, c, c), 255);
        assert_eq!(alpha(&printed, 0, 0), 0);
        let shadow = umd_shadow();
        for (x, y, expected) in [(20, 265, 255), (20, 280, 0), (20, 60, 0)] {
            assert_eq!(alpha(&printed, x, y), expected, "UMD casing at ({x}, {y})");
            assert_eq!(alpha(&shadow, x, y), expected, "UMD backing at ({x}, {y})");
        }
        for at in [index(c, c + 25), index(c, c + 145)] {
            assert_eq!(&printed.rgba[at..at + 4], &plain.rgba[at..at + 4]);
        }
        let print = index(c, c + 100);
        assert_eq!(&printed.rgba[print..print + 4], &ink);
        assert_eq!(printed.rgba[nick + 3], 255);
    }
}
