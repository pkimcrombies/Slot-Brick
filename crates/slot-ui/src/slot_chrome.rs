use std::sync::OnceLock;

use slot_gfx::{Draw, TexId, OUT_H, OUT_W};
use slot_store::Cart;
use slot_store::Theme;

use crate::cart::{cart_box, label_colour, label_text, seated_box, SEATED_W};
use crate::footer::Printed;
use crate::icon::icon_box;
use crate::plate::HINT_H;
use crate::shelf::foot_y;

pub const ALERT_PX: f32 = 44.0;

pub const MOUTH_H: f32 = 58.0;
const BAND_Y: f32 = OUT_H as f32 - MOUTH_H;

pub const MOUTH_W: f32 = SEATED_W as f32 + 14.0;
const SLIT_H: f32 = 9.0;
const MOUTH_X: f32 = (OUT_W as f32 - MOUTH_W) / 2.0;
const SLIT_Y: f32 = BAY_Y + 5.0;

pub const LIP_H: f32 = 2.0;

const BAY_W: f32 = MOUTH_W + 18.0;
const BAY_X: f32 = (OUT_W as f32 - BAY_W) / 2.0;
const BAY_Y: f32 = BAND_Y + LIP_H;
const SCOOP_W: f32 = MOUTH_W * 0.88;
const SCOOP_D: f32 = RECESS_H - (SCOOP_Y - BAY_Y);
const SCOOP_Y: f32 = SLIT_Y + SLIT_H;

const RECESS_H: f32 = 42.0;
const RIM_W: f32 = 2.0;
const CX: f32 = OUT_W as f32 / 2.0;
const SCOOP_FLAT: f32 = 4.0;

static THEME: OnceLock<Theme> = OnceLock::new();

pub fn set_theme(theme: Theme) {
    let _ = THEME.set(theme);
}

pub fn theme() -> &'static Theme {
    THEME.get_or_init(Theme::default)
}

fn rgb(c: [u8; 3]) -> [f32; 4] {
    [
        c[0] as f32 / 255.0,
        c[1] as f32 / 255.0,
        c[2] as f32 / 255.0,
        1.0,
    ]
}

pub fn housing() -> [f32; 4] {
    rgb(theme().housing)
}

pub fn opening() -> [f32; 4] {
    rgb(theme().opening)
}

pub fn edge() -> [f32; 4] {
    rgb(theme().edge)
}

pub fn recess() -> [f32; 4] {
    rgb(theme().recess)
}

const LIP_Y: f32 = BAND_Y;

const SEATED_Y: f32 = BAY_Y + 4.0;

fn catch_at(foot: f32, seated_foot: f32) -> f32 {
    (LIP_Y - foot) / (seated_foot - foot)
}

const CATCH_IN: f32 = 0.42;
const CATCH_OUT: f32 = 0.62;
const CREEP: f32 = 0.03;

pub struct SlotChrome<'a> {
    pub cart: &'a Cart,
    pub face: Option<TexId>,
    pub rest: f32,
    pub scale: f32,
    pub seat: f32,
    pub alert: Option<(TexId, f32)>,
    pub dim: f32,
    pub screen: f32,
    pub game: bool,
}

impl SlotChrome<'_> {
    pub fn draw(&self, out: &mut Vec<Draw>) {
        let seat = self.seat.clamp(0.0, 1.0);
        let dim = self.dim.clamp(0.0, 1.0);
        if dim > 0.0 {
            out.push(Draw::Rect {
                x: 0.0,
                y: 0.0,
                w: OUT_W as f32,
                h: OUT_H as f32,
                colour: [0.0, 0.0, 0.0, dim],
            });
        }

        let chrome = 1.0 - self.screen.clamp(0.0, 1.0);

        draw_slot_back(chrome, out);

        let (cw, ch) = cart_box(self.cart.platform);
        let (cw, ch) = (cw as f32, ch as f32);

        let scale = self.scale.clamp(0.0, 1.0);
        let (w0, h0) = (cw * scale, ch * scale);
        let (sw, sh) = seated_box(self.cart.platform);
        let (sw, sh) = (sw as f32, sh as f32);
        let (foot0, seated_foot) = (foot_y(ch), SEATED_Y + sh);
        let catch = catch_at(foot0, seated_foot);
        let travel = travel(seat, catch);
        let k = (travel / catch).min(1.0);
        let (cw, ch) = (w0 + (sw - w0) * k, h0 + (sh - h0) * k);
        let centre = self.rest + w0 / 2.0;
        let x = centre + (CX - centre) * k - cw / 2.0;
        let y = foot0 + (seated_foot - foot0) * travel - ch;
        let cart_alpha = if seat >= 1.0 { chrome } else { 1.0 };
        out.push(match self.face {
            Some(tex) => Draw::Tex {
                x,
                y,
                w: cw,
                h: ch,
                tex,
                alpha: cart_alpha,
            },
            None => {
                let c = label_colour(&label_text(self.cart));
                Draw::Rect {
                    x,
                    y,
                    w: cw,
                    h: ch,
                    colour: [
                        c[0] as f32 / 255.0,
                        c[1] as f32 / 255.0,
                        c[2] as f32 / 255.0,
                        cart_alpha,
                    ],
                }
            }
        });

        if let Some((tex, alpha)) = self.alert {
            let (w, h) = icon_box(ALERT_PX);
            let (w, h) = (w as f32, h as f32);
            out.push(Draw::Tex {
                x: x + (cw - w) / 2.0,
                y: y + (ch - h) / 2.0,
                w,
                h,
                tex,
                alpha,
            });
        }

        if chrome < 1.0 {
            // Cover the inserted cart behind the *actual* front artwork during
            // the screen fade. The old procedural scoop did not match the
            // Android Original GB slot and left a second outline visible.
            #[cfg(feature = "brick")]
            let artwork_masked = draw_original_gb_transition_mask(out);
            #[cfg(not(feature = "brick"))]
            let artwork_masked = false;
            if !artwork_masked {
                // Match the procedural scoop when the shared artwork is unavailable.
                front_bands(|x, y, w, h, _| {
                    out.push(Draw::Rect {
                        x,
                        y,
                        w,
                        h,
                        colour: [0.0, 0.0, 0.0, 1.0],
                    });
                });
            }
        }

        if self.game && self.screen > 0.0 {
            out.push(Draw::Game);
        }

        draw_slot_front(chrome, out);
    }
}

fn band(x: f32, y: f32, w: f32, h: f32, c: [f32; 4], alpha: f32) -> Draw {
    Draw::Rect {
        x,
        y,
        w,
        h,
        colour: [c[0], c[1], c[2], c[3] * alpha],
    }
}

// Cache vector chrome at twice logical resolution; the cartridge is drawn
// between these layers to preserve insertion and ejection occlusion.
#[cfg(feature = "brick")]
static ART: OnceLock<(TexId, TexId)> = OnceLock::new();

/// Original GBA's full-resolution console art, separate from Brick's default.
#[cfg(feature = "brick")]
static ORIGINAL_GBA_ART: OnceLock<(TexId, TexId)> = OnceLock::new();
// Real Android Original console bars; GBA stays on the existing tested path.
#[cfg(feature = "brick")]
static ORIGINAL_GB_ART: OnceLock<(TexId, TexId)> = OnceLock::new();
/// Black occlusion cover using the exact alpha of the active GB front PNG.
#[cfg(feature = "brick")]
static ORIGINAL_GB_TRANSITION_MASK: OnceLock<TexId> = OnceLock::new();
#[cfg(feature = "brick")]
static ORIGINAL_GBC_ART: OnceLock<(TexId, TexId)> = OnceLock::new();
#[cfg(feature = "brick")]
static ORIGINAL_NES_ART: OnceLock<(TexId, TexId)> = OnceLock::new();
#[cfg(feature = "brick")]
static ORIGINAL_SNES_ART: OnceLock<(TexId, TexId)> = OnceLock::new();
#[cfg(feature = "brick")]
fn original_gba_bar_face(front: bool) -> Option<Vec<u8>> {
    let bytes: &[u8] = if front {
        include_bytes!("../../../assets/android-original/slot_front_gba_light.png")
    } else {
        include_bytes!("../../../assets/android-original/slot_back_gba_light.png")
    };
    let mut decoder = png::Decoder::new(bytes);
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    decoder.set_limits(png::Limits { bytes: 64 << 20 });
    let mut reader = decoder.read_info().ok()?;
    let mut buffer = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buffer).ok()?;
    if info.color_type != png::ColorType::Rgba { return None; }
    // The Android Light GBA art preserves the proper cartridge opening but has
    // a faint cool tint. Neutralise it for the NES-style rose-grey GBA shelf.
    // Keep transparency, recess shadows and fine material texture intact.
    let mut face = crate::art::cover_rgba(
        &buffer[..info.buffer_size()], info.width, info.height, OUT_W, MOUTH_H as u32,
    )?;
    for pixel in face.chunks_exact_mut(4) {
        let grey = ((77u32 * pixel[0] as u32
            + 150u32 * pixel[1] as u32
            + 29u32 * pixel[2] as u32 + 128) >> 8) as u8;
        pixel[0] = grey;
        pixel[1] = grey;
        pixel[2] = grey;
    }
    Some(face)
}

/// Decode one Android Original console bar to the Brick's logical 720x58.
#[cfg(feature = "brick")]
fn original_console_bar_face(platform: slot_store::Platform, front: bool) -> Option<Vec<u8>> {
    use slot_store::Platform;
    let bytes: &[u8] = match (platform, front) {
        (Platform::Gb, false) => include_bytes!("../../../assets/android-original/slot_back_gb.png"),
        (Platform::Gb, true) => include_bytes!("../../../assets/android-original/slot_front_gb.png"),
        (Platform::Gbc, false) => include_bytes!("../../../assets/android-original/slot_back_gbc.png"),
        (Platform::Gbc, true) => include_bytes!("../../../assets/android-original/slot_front_gbc.png"),
        (Platform::Nes, false) => include_bytes!("../../../assets/android-original/slot_back_nes.png"),
        (Platform::Nes, true) => include_bytes!("../../../assets/android-original/slot_front_nes.png"),
        (Platform::Snes, false) => include_bytes!("../../../assets/android-original/slot_back_snes.png"),
        (Platform::Snes, true) => include_bytes!("../../../assets/android-original/slot_front_snes.png"),
        (Platform::Gba, _) => return original_gba_bar_face(front),
        _ => return None,
    };
    let mut decoder = png::Decoder::new(bytes);
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    decoder.set_limits(png::Limits { bytes: 64 << 20 });
    let mut reader = decoder.read_info().ok()?;
    let mut buffer = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buffer).ok()?;
    if info.color_type != png::ColorType::Rgba { return None; }
    crate::art::cover_rgba(
        &buffer[..info.buffer_size()], info.width, info.height, OUT_W, MOUTH_H as u32,
    )
}

#[cfg(feature = "brick")]
pub fn upload_slot_art(brick_pro: bool, mut upload: impl FnMut(u32,u32,&[u8])->TexId) {
    if ART.get().is_some() {return;}
    if let (Some(back),Some(front))=(slot_art_face(false),slot_art_face(true)) {
        let _=ART.set((upload(back.w,back.h,&back.rgba),upload(front.w,front.h,&front.rgba)));
    }
    if let (Some(back), Some(front)) = (original_gba_bar_face(false), original_gba_bar_face(true)) {
        let _ = ORIGINAL_GBA_ART.set((
            upload(OUT_W, MOUTH_H as u32, &back),
            upload(OUT_W, MOUTH_H as u32, &front),
        ));
    }
    for (platform, target) in [
        (slot_store::Platform::Gb, &ORIGINAL_GB_ART),
        (slot_store::Platform::Gbc, &ORIGINAL_GBC_ART),
        (slot_store::Platform::Nes, &ORIGINAL_NES_ART),
        (slot_store::Platform::Snes, &ORIGINAL_SNES_ART),
    ] {
        if let (Some(mut back), Some(mut front)) = (
            original_console_bar_face(platform, false),
            original_console_bar_face(platform, true),
        ) {
            if platform == slot_store::Platform::Gb {
                if brick_pro {
                    // Match the Brick Pro housing and leave the dark recess and alpha intact.
                    for pixel in back.chunks_exact_mut(4).chain(front.chunks_exact_mut(4)) {
                        let luma = (f32::from(pixel[0]) + f32::from(pixel[1]) + f32::from(pixel[2])) / 3.0;
                        let strength = ((luma - 85.0) / 80.0).clamp(0.0, 1.0);
                        for (channel, delta) in pixel[..3].iter_mut().zip([13.0, 7.0, 11.0]) {
                            *channel = channel.saturating_add((strength * delta).round() as u8);
                        }
                    }
                }
                // Use the same downsampled pixels as ORIGINAL_GB_ART, so the
                // opening and its antialiased edge always align precisely.
                let mask = black_mask_from_front(&front);
                let _ = ORIGINAL_GB_TRANSITION_MASK
                    .set(upload(OUT_W, MOUTH_H as u32, &mask));
            }
            let _ = target.set((
                upload(OUT_W, MOUTH_H as u32, &back),
                upload(OUT_W, MOUTH_H as u32, &front),
            ));
        }
    }
}

#[cfg(feature = "brick")]
pub fn slot_art_face(front: bool) -> Option<crate::cart::CartFace> {
    use std::fmt::Write;
    let hex=|c: [u8;3]|format!("#{:02x}{:02x}{:02x}",c[0],c[1],c[2]);
    let t=theme();
    let (housing,recess,opening,edge)=(hex(t.housing),hex(t.recess),hex(t.opening),hex(t.edge));
    let near=CX-SCOOP_W/2.0;let far=CX+SCOOP_W/2.0;let right=BAY_X+BAY_W;
    let local=|y:f32|y-BAND_Y;
    let curve=|offset:f32,reverse:bool| {
        let mut path=String::new();
        for i in 0..=256 {
            let u=if reverse {1.0-i as f32/256.0} else {i as f32/256.0};
            let x=near+SCOOP_W*u;
            let y=local(SCOOP_Y)+SCOOP_D*(1.0-(2.0*u-1.0).abs().powf(SCOOP_FLAT))+offset;
            let _=write!(path," L{x:.3},{y:.3}");
        }
        path
    };
    let mut body=String::new();
    if front {
        let _=write!(body,"<path fill='{housing}' d='M0,0 H{BAY_X} V{} H{near} {} H{right} V0 H{OUT_W} V{MOUTH_H} H0 Z'/>",local(SCOOP_Y)+RIM_W,curve(RIM_W,false));
        let _=write!(body,"<path fill='{edge}' d='M{near},{} {} {} Z'/>",local(SCOOP_Y),curve(0.0,false),curve(RIM_W,true));
        let _=write!(body,"<path fill='none' stroke='white' stroke-opacity='.08' stroke-width='.5' d='M{near},{} {}'/>",local(SCOOP_Y)+RIM_W,curve(RIM_W,false));
    } else {
        let _=write!(body,"<rect x='{BAY_X}' y='0' width='{BAY_W}' height='{LIP_H}' fill='{housing}'/><rect x='{MOUTH_X}' y='0' width='{MOUTH_W}' height='{LIP_H}' fill='{edge}'/><rect x='{BAY_X}' y='{}' width='{BAY_W}' height='{RECESS_H}' fill='{recess}'/>",local(BAY_Y));
        let _=write!(body,"<path fill='{opening}' d='M{MOUTH_X},{} H{} V{} H{far} {} H{MOUTH_X} Z'/>",local(SLIT_Y),MOUTH_X+MOUTH_W,local(SCOOP_Y),curve(0.0,true));
        let _=write!(body,"<rect x='{MOUTH_X}' y='{}' width='{MOUTH_W}' height='3' fill='url(#shade)'/>",local(SLIT_Y));
    }
    raster(&body)
}

/// Slot art, as an SVG body the width of the screen and the height of the slot's band, drawn at
/// twice the screen's resolution.
#[cfg(feature = "brick")]
fn raster(body: &str) -> Option<crate::cart::CartFace> {
    let svg=format!("<svg xmlns='http://www.w3.org/2000/svg' width='{OUT_W}' height='{MOUTH_H}'><defs><linearGradient id='shade' x2='0' y2='1'><stop stop-color='black' stop-opacity='.25'/><stop offset='1' stop-color='black' stop-opacity='0'/></linearGradient></defs>{body}</svg>");
    let tree=usvg::Tree::from_str(&svg,&usvg::Options::default()).ok()?;
    let (w,h)=(OUT_W*2,(MOUTH_H*2.0) as u32);
    let mut pixels=resvg::tiny_skia::Pixmap::new(w,h)?;
    resvg::render(&tree,resvg::tiny_skia::Transform::from_scale(2.0,2.0),&mut pixels.as_mut());
    let mut rgba=pixels.data().to_vec();
    // Convert premultiplied raster colours for the straight-alpha sprite shader.
    for p in rgba.chunks_exact_mut(4) {
        let a=u32::from(p[3]);
        if a>0 {for c in &mut p[..3] {*c=((u32::from(*c)*255+a/2)/a).min(255) as u8;}}
    }
    Some(crate::cart::CartFace{w,h,rgba})
}
#[cfg(feature = "brick")]
fn draw_art(front: bool, alpha: f32, out: &mut Vec<Draw>) -> bool {
    let Some(&(back,fore))=ART.get() else {return false;};
    if alpha>0.0 {out.push(Draw::Tex{x:0.0,y:BAND_Y,w:OUT_W as f32,h:MOUTH_H,tex:if front {fore}else{back},alpha});}
    true
}

/// Generate a black occlusion layer with the *same alpha* as the visible
/// GB front plate. Preserving partial alpha also preserves antialiased edges.
#[cfg(feature = "brick")]
fn black_mask_from_front(front: &[u8]) -> Vec<u8> {
    let mut mask = front.to_vec();
    for px in mask.chunks_exact_mut(4) {
        px[..3].fill(0);
    }
    mask
}

#[cfg(feature = "brick")]
fn draw_original_gb_transition_mask(out: &mut Vec<Draw>) -> bool {
    let Some(&tex) = ORIGINAL_GB_TRANSITION_MASK.get() else { return false; };
    out.push(Draw::Tex {
        x: 0.0,
        y: BAND_Y,
        w: OUT_W as f32,
        h: MOUTH_H,
        tex,
        alpha: 1.0,
    });
    true
}

#[cfg(feature = "brick")]
fn draw_original_console_art(front: bool, alpha: f32, out: &mut Vec<Draw>) -> bool {
    let Some(&(back, fore)) = ORIGINAL_GB_ART.get() else { return false; };
    if alpha > 0.0 {
        out.push(Draw::Tex {
            x: 0.0, y: BAND_Y, w: OUT_W as f32, h: MOUTH_H,
            tex: if front { fore } else { back }, alpha,
        });
    }
    true
}

fn draw_slot_back(alpha: f32, out: &mut Vec<Draw>) {
    #[cfg(feature = "brick")]
    if draw_original_console_art(false, alpha, out) || draw_art(false, alpha, out) { return; }
    out.push(band(BAY_X, BAND_Y, BAY_W, LIP_H, housing(), alpha));
    out.push(band(MOUTH_X, BAND_Y, MOUTH_W, LIP_H, edge(), alpha));
    out.push(band(BAY_X, BAY_Y, BAY_W, RECESS_H, recess(), alpha));
    out.push(band(MOUTH_X, SLIT_Y, MOUTH_W, SLIT_H, opening(), alpha));
    for_each_scoop_span(|x, w, depth| {
        out.push(band(x, SCOOP_Y, w, depth, opening(), alpha));
    });
}

fn draw_slot_front(alpha: f32, out: &mut Vec<Draw>) {
    #[cfg(feature = "brick")]
    if draw_original_console_art(true, alpha, out) || draw_art(true, alpha, out) { return; }
    front_bands(|x, y, w, h, c| out.push(band(x, y, w, h, c, alpha)));
}

fn front_bands(mut band: impl FnMut(f32, f32, f32, f32, [f32; 4])) {
    let w = OUT_W as f32;
    let right = BAY_X + BAY_W;
    let floor = SCOOP_Y + SCOOP_D + RIM_W;
    band(0.0, BAND_Y, BAY_X, MOUTH_H, housing());
    band(right, BAND_Y, w - right, MOUTH_H, housing());

    let near = CX - SCOOP_W / 2.0;
    band(BAY_X, SCOOP_Y, near - BAY_X, floor - SCOOP_Y, housing());
    let far = CX + SCOOP_W / 2.0;
    band(far, SCOOP_Y, right - far, floor - SCOOP_Y, housing());

    for_each_scoop_span(|x, w, depth| {
        let top = SCOOP_Y + depth + RIM_W;
        band(x, top, w, floor - top, housing());
    });
    band(0.0, floor, w, OUT_H as f32 - floor, housing());
    for_each_scoop_span(|x, w, depth| {
        band(x, SCOOP_Y + depth, w, RIM_W, edge());
    });
}

fn for_each_scoop_span(mut span: impl FnMut(f32, f32, f32)) {
    let hw = SCOOP_W / 2.0;
    let depth = |x: f32| SCOOP_D * (1.0 - (x.abs() / hw).powf(SCOOP_FLAT)).max(0.0);
    let mut x = -hw;
    while x < hw {
        let d = depth(x + 0.5).round();
        let start = x;
        while x < hw && depth(x + 0.5).round() == d {
            x += 1.0;
        }
        span(CX + start, x - start, d);
    }
}

pub fn draw_slot_name(name: Printed, alpha: f32, out: &mut Vec<Draw>) {
    let (Some(tex), true) = (name.face, alpha > 0.0) else {
        return;
    };
    let (w, h) = (name.w as f32, HINT_H as f32);
    let hole = SCOOP_Y + SCOOP_D - SLIT_Y;
    out.push(Draw::Tex {
        x: CX - w / 2.0,
        y: SLIT_Y + (hole - h) / 2.0,
        w,
        h,
        tex,
        alpha,
    });
}

pub fn draw_empty_slot(out: &mut Vec<Draw>) {
    draw_slot_back(1.0, out);
    draw_slot_front(1.0, out);
}

fn travel(seat: f32, catch: f32) -> f32 {
    if seat < CATCH_IN {
        catch * ease(seat / CATCH_IN)
    } else if seat < CATCH_OUT {
        catch + CREEP * (seat - CATCH_IN) / (CATCH_OUT - CATCH_IN)
    } else {
        let caught = catch + CREEP;
        caught + (1.0 - caught) * ease((seat - CATCH_OUT) / (1.0 - CATCH_OUT))
    }
}

pub fn ease(u: f32) -> f32 {
    u * u * u * (u * (u * 6.0 - 15.0) + 10.0)
}

#[cfg(all(test, feature = "brick"))]
mod original_gba_asset_tests {
    use super::*;

    #[test]
    fn original_gba_bars_decode_to_exact_brick_slot_dimensions() {
        for front in [false, true] {
            let image = original_gba_bar_face(front).expect("Android Original PNG must decode");
            assert_eq!(image.len(), (OUT_W * MOUTH_H as u32 * 4) as usize);
            assert!(image.chunks_exact(4).any(|pixel| pixel[3] != 0));
        }
    }

    #[test]
    fn gb_gbc_nes_snes_bars_decode_to_exact_brick_slot_dimensions() {
        for platform in [
            slot_store::Platform::Gb,
            slot_store::Platform::Gbc,
            slot_store::Platform::Nes,
            slot_store::Platform::Snes,
        ] {
            for front in [false, true] {
                let image = original_console_bar_face(platform, front)
                    .expect("Android Original console PNG must decode");
                assert_eq!(image.len(), (OUT_W * MOUTH_H as u32 * 4) as usize);
                assert!(image.chunks_exact(4).any(|pixel| pixel[3] != 0));
            }
        }
        assert!(original_console_bar_face(slot_store::Platform::Ps1, false).is_none());
    }
}

#[cfg(all(test, feature = "brick"))]
mod gb_transition_tests {
    use super::{black_mask_from_front, original_console_bar_face};
    use slot_store::Platform;

    #[test]
    fn occlusion_mask_uses_front_art_alpha_not_the_old_scoop() {
        let pixels = [
            220, 215, 200, 255, // painted case
            100, 95, 90, 128,   // antialiased opening edge
            0, 0, 0, 0,         // transparent opening
        ];
        let actual = black_mask_from_front(&pixels);
        assert_eq!(actual, [
            0, 0, 0, 255,
            0, 0, 0, 128,
            0, 0, 0, 0,
        ]);
    }

    #[test]
    fn default_gb_front_contains_both_case_and_transparent_opening() {
        let front = original_console_bar_face(Platform::Gb, true)
            .expect("the default GB front PNG must decode");
        assert!(
            front.chunks_exact(4).any(|px| px[3] == 0),
            "the GB front must have a transparent opening"
        );
        assert!(
            front.chunks_exact(4).any(|px| px[3] == 255),
            "the GB front must have an opaque surrounding case"
        );
    }
}

#[cfg(test)]
mod disc_tests {
    use super::*;

    #[test]
    fn cartridge_and_disc_use_the_same_opening() {
        let render = |platform| {
            let cart = Cart {
                platform,
                stem: "Test".into(),
                rom: Default::default(),
                label: None,
                famicom_label: None,
                title: String::new(),
                code: String::new(),
                shell: None,
            };
            let face = TexId::from_raw(77);
            let mut draws = Vec::new();
            SlotChrome {
                cart: &cart,
                face: Some(face),
                rest: CX - cart_box(platform).0 as f32 / 2.0,
                scale: 1.0,
                seat: 0.5,
                alert: None,
                dim: 0.0,
                screen: 0.0,
                game: false,
            }
            .draw(&mut draws);
            draws.retain(|draw| !matches!(*draw, Draw::Tex { tex, .. } if tex == face));
            draws
        };
        let cartridge = render(slot_store::Platform::Gba);
        for disc in [slot_store::Platform::Ps1, slot_store::Platform::Psp] {
            assert!(render(disc) == cartridge, "the scoop must be identical on {disc:?}");
        }
    }

    #[test]
    fn playstation_discs_use_the_cartridge_catch_and_stay_seated() {
        use slot_store::Platform;

        let face = TexId::from_raw(77);
        for platform in [Platform::Ps1, Platform::Psp] {
            let cart = Cart {
                platform,
                stem: "Disc Test".into(),
                rom: Default::default(),
                label: None,
                famicom_label: None,
                title: String::new(),
                code: String::new(),
                shell: None,
            };
            let mut scene = SlotChrome {
                cart: &cart,
                face: Some(face),
                rest: CX - cart_box(platform).0 as f32 / 2.0,
                scale: 1.0,
                seat: 0.0,
                alert: None,
                dim: 0.0,
                screen: 0.0,
                game: false,
            };
            let mut before = 0.0;
            for seat in [0.0, CATCH_IN, CATCH_OUT, 0.9] {
                scene.seat = seat;
                let mut draws = Vec::new();
                scene.draw(&mut draws);
                let (y, w, h) = draws
                    .iter()
                    .find_map(|draw| match *draw {
                        Draw::Tex { y, w, h, tex, .. } if tex == face => Some((y, w, h)),
                        _ => None,
                    })
                    .expect("still-visible disc should be drawn face-on");
                assert_eq!(w, h);
                assert!(y > before);
                if seat == CATCH_IN {
                    assert!((y + h - LIP_Y).abs() < 0.01, "the disc catches at the lip");
                }
                before = y;
            }
            scene.seat = 1.0;
            for screen in [0.0, 0.5] {
                scene.screen = screen;
                let mut draws = Vec::new();
                scene.draw(&mut draws);
                let (y, w, h, alpha) = draws
                    .iter()
                    .find_map(|draw| match *draw {
                        Draw::Tex { y, w, h, tex, alpha, .. } if tex == face => Some((y, w, h, alpha)),
                        _ => None,
                    })
                    .expect("the disc stays seated through the handoff");
                assert!((y - SEATED_Y).abs() < 0.01);
                assert_eq!((w, h), (SEATED_W as f32, SEATED_W as f32));
                assert_eq!(alpha, 1.0 - screen);
            }
        }
    }

}
