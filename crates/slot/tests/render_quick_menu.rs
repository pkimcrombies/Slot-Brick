#![cfg(target_os = "macos")]

mod common;

use std::collections::VecDeque;

use common::{clocked, tmp_root_with_carts};
use slot::frontend::Frontend;
use slot_gfx::{Compositor, HeadlessSurface, OUT_H, OUT_W};
use slot_input::{Btn, InputSource, Millis, RawEvent};
use slot_power::SimPlatform;
use slot_ui::{quick_pitch, quick_top, QuickRow, MENU_SELECTION_INK, QUICK_EDGE, QUICK_VALUE_EDGE};

struct Script(VecDeque<Vec<RawEvent>>);

impl InputSource for Script {
    fn poll(&mut self, _now: Millis) -> Vec<RawEvent> {
        self.0.pop_front().unwrap_or_default()
    }
}

fn tap(f: &mut Frontend, input: &mut Script, btn: Btn) {
    input.0.push_back(vec![RawEvent::Down(btn)]);
    f.advance(input);
    input.0.push_back(vec![RawEvent::Up(btn)]);
    f.advance(input);
}

fn at(px: &[u8], x: usize, y: usize) -> [u8; 3] {
    let o = (y * OUT_W as usize + x) * 4;
    [px[o], px[o + 1], px[o + 2]]
}

fn inked(px: &[u8], xs: std::ops::Range<usize>, top: usize) -> Vec<usize> {
    xs.filter(|&x| (top + 8..top + 39).any(|y| at(px, x, y)[0] > 0x80))
        .collect()
}

fn composed(f: &mut Frontend, c: &mut Compositor, name: &str) -> Vec<u8> {
    f.compose(c);
    let px = c.read_frame();
    if let Ok(dir) = std::env::var("SCRATCH_PNG_DIR") {
        let path = format!("{dir}/quick-menu-{name}.png");
        let file = std::fs::File::create(&path).expect("create png");
        let mut e = png::Encoder::new(std::io::BufWriter::new(file), OUT_W, OUT_H);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        e.write_header()
            .expect("png header")
            .write_image_data(&px)
            .expect("png data");
        println!("wrote {path}");
    }
    px
}

#[test]
fn the_quick_menu_renders_full_screen() {
    let Ok(surface) = HeadlessSurface::new() else {
        return;
    };
    let Ok(mut c) = Compositor::new(&surface) else {
        return;
    };
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    clocked(d.path());
    let mut f = Frontend::boot(Box::new(SimPlatform::at(d.path().to_path_buf())));
    f.upload_faces(&mut c);
    let mut input = Script(VecDeque::new());
    tap(&mut f, &mut input, Btn::Menu);

    let ground = [0, 0, 0];
    let edge = QUICK_EDGE as usize;
    let value_edge = QUICK_VALUE_EDGE as usize;
    let pitch = quick_pitch(QuickRow::MAIN.len());
    for (index, selected) in QuickRow::MAIN.into_iter().enumerate() {
        if index != 0 {
            tap(&mut f, &mut input, Btn::Down);
        }
        let px = composed(&mut f, &mut c, selected.label());
        let top = (quick_top(QuickRow::MAIN.len()) + pitch * index as f32) as usize;
        for x in [1, OUT_W as usize - 2] {
            assert_eq!(at(&px, x, top + 24), ground);
        }
        assert!(
            (top + 8..top + 39).any(|y| (edge..value_edge).any(|x| at(&px, x, y) == MENU_SELECTION_INK)),
            "{selected:?}: selected label did not use the menu pink",
        );
        let label = inked(&px, 0..value_edge, top);
        assert!((edge..=edge + 6).contains(label.first().expect("label")));
        assert!((420..OUT_H as usize).all(|y| (0..OUT_W as usize).all(|x| at(&px, x, y) == ground)));
    }
}

#[test]
fn every_fast_forward_speed_starts_at_the_same_value_column() {
    let Ok(surface) = HeadlessSurface::new() else {
        return;
    };
    let Ok(mut c) = Compositor::new(&surface) else {
        return;
    };
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    clocked(d.path());
    let mut f = Frontend::boot(Box::new(SimPlatform::at(d.path().to_path_buf())));
    f.upload_faces(&mut c);
    let mut input = Script(VecDeque::new());
    tap(&mut f, &mut input, Btn::Menu);
    tap(&mut f, &mut input, Btn::Down);
    tap(&mut f, &mut input, Btn::A);
    tap(&mut f, &mut input, Btn::Left);
    tap(&mut f, &mut input, Btn::Left);

    let top = quick_top(QuickRow::GAME.len()) as usize;
    let edge = QUICK_EDGE as usize;
    let value_edge = QUICK_VALUE_EDGE as usize;
    for name in ["2x", "3x", "4x", "6x"] {
        let px = composed(&mut f, &mut c, name);
        let value = inked(&px, value_edge..OUT_W as usize, top);
        let first_value = *value
            .first()
            .unwrap_or_else(|| panic!("{name}: the Fast Forward row has no value"));
        assert!(
            (value_edge..=value_edge + 6).contains(&first_value),
            "{name} starts at x {first_value}",
        );
        let label = inked(&px, 0..value_edge, top);
        let first = *label
            .first()
            .unwrap_or_else(|| panic!("{name}: the Fast Forward row has no label"));
        assert!(
            (edge..=edge + 6).contains(&first),
            "the label moved to x {first} to make room for {name}"
        );
        tap(&mut f, &mut input, Btn::Right);
    }
}

#[test]
fn the_screen_page_sets_gb_palettes_in_line_with_the_rows_above() {
    let Ok(surface) = HeadlessSurface::new() else {
        return;
    };
    let Ok(mut c) = Compositor::new(&surface) else {
        return;
    };
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    clocked(d.path());
    let mut f = Frontend::boot(Box::new(SimPlatform::at(d.path().to_path_buf())));
    f.upload_faces(&mut c);
    let mut input = Script(VecDeque::new());
    tap(&mut f, &mut input, Btn::Menu);
    tap(&mut f, &mut input, Btn::A);
    tap(&mut f, &mut input, Btn::A);
    for _ in 0..QuickRow::GbPalettes.position() {
        tap(&mut f, &mut input, Btn::Down);
    }
    let px = composed(&mut f, &mut c, "screen-page");

    let edge = QUICK_EDGE as usize;
    let value_edge = QUICK_VALUE_EDGE as usize;
    let pitch = quick_pitch(QuickRow::SCREEN_GB.len());
    for row in QuickRow::SCREEN_GB {
        let top = (quick_top(QuickRow::SCREEN_GB.len()) + pitch * row.position() as f32) as usize;
        let label = inked(&px, 0..value_edge, top);
        let first = *label.first().expect("a row with no label");
        assert!(
            (edge..=edge + 6).contains(&first),
            "{row:?}'s label starts at x {first}"
        );
        let value = inked(&px, value_edge..OUT_W as usize, top);
        let first = *value.first().expect("a row with no value");
        assert!(
            (value_edge..=value_edge + 6).contains(&first),
            "{row:?}'s value starts at x {first}"
        );
    }
}
