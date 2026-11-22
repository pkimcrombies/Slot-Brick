#![cfg(target_os = "macos")]

mod common;

use std::collections::VecDeque;

use common::{clocked, tmp_root_with_carts};
use slot::frontend::Frontend;
use slot_gfx::{Compositor, HeadlessSurface, OUT_H, OUT_W};
use slot_input::{Btn, InputSource, Millis, RawEvent};
use slot_power::SimPlatform;
use slot_ui::{quick_pitch, QuickRow, MENU_ROSE_INK, QUICK_EDGE, QUICK_TOP};

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
    xs.filter(|&x| (top + 8..top + 35).any(|y| at(px, x, y)[0] > 0x80))
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
    let label_edge = QUICK_EDGE as usize;
    let right = OUT_W as usize - label_edge;
    let middle = OUT_W as usize / 2;
    for (i, selected) in QuickRow::MAIN.into_iter().enumerate() {
        if i > 0 {
            tap(&mut f, &mut input, Btn::Down);
        }
        let name = selected.label();
        let px = composed(&mut f, &mut c, name);
        let pitch = quick_pitch(QuickRow::MAIN.len());
        let y = (QUICK_TOP + pitch * (i as f32 + 0.5)) as usize;
        assert_eq!(at(&px, label_edge - 19, y), MENU_ROSE_INK, "{name}: no cursor");
        for x in [1, OUT_W as usize - 2] {
            assert_eq!(at(&px, x, y), ground, "{name}: background at x {x}");
        }
        assert_eq!(
            at(&px, 2, OUT_H as usize - 2),
            ground,
            "{name}: not on the ground"
        );

        for row in QuickRow::MAIN {
            let top = (QUICK_TOP + pitch * row.position() as f32) as usize;
            let label = inked(&px, label_edge..middle, top);
            let first = *label.first().expect("a row with no label");
            assert!(
                (label_edge..=label_edge + 8).contains(&first),
                "{name}: {row:?}'s label starts at x {first}"
            );
            let value = inked(&px, middle..OUT_W as usize, top);
            let last = *value.last().expect("a row with no submenu chevron");
            assert!(
                (right - 8..=right + 1).contains(&last),
                "{name}: {row:?}'s chevron ends at x {last}"
            );
        }
    }

    tap(&mut f, &mut input, Btn::A);
    for _ in 0..QuickRow::DateTime.position() {
        tap(&mut f, &mut input, Btn::Down);
    }
    tap(&mut f, &mut input, Btn::A);
    let px = composed(&mut f, &mut c, "clock");
    assert!(
        (274..320).any(|y| (middle - 180..middle).any(|x| {
            at(&px, x, y) == slot_ui::MENU_TEXT_INK
        })),
        "the clock from the menu does not offer B BACK"
    );
}

#[test]
fn every_fast_forward_speed_sits_on_the_rows_right_edge_and_clears_the_label() {
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

    let top = QUICK_TOP as usize;
    let edge = QUICK_EDGE as usize;
    let right = OUT_W as usize - edge;
    let middle = OUT_W as usize / 2;
    for name in ["2x", "3x", "4x", "6x"] {
        let px = composed(&mut f, &mut c, name);
        let value = inked(&px, middle..OUT_W as usize, top);
        let last = *value
            .last()
            .unwrap_or_else(|| panic!("{name}: the Fast Forward row has no value"));
        assert!(
            (right - 8..=right + 1).contains(&last),
            "{name} ends at x {last}, off the edge every other value keeps"
        );
        assert!(
            inked(&px, middle - 10..middle + 10, top).is_empty(),
            "{name} reaches the middle of the row"
        );
        let label = inked(&px, edge..middle, top);
        let first = *label
            .first()
            .unwrap_or_else(|| panic!("{name}: the Fast Forward row has no label"));
        assert!(
            (edge..=edge + 8).contains(&first),
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
    let right = OUT_W as usize - edge;
    let middle = OUT_W as usize / 2;
    let pitch = quick_pitch(QuickRow::SCREEN_GB.len());
    let y = (QUICK_TOP + pitch * (QuickRow::GbPalettes.position() as f32 + 0.5)) as usize;
    assert_eq!(
        at(&px, edge - 19, y),
        MENU_ROSE_INK,
        "the cursor is not on GB Palettes"
    );
    for row in QuickRow::SCREEN_GB {
        let top = (QUICK_TOP + pitch * row.position() as f32) as usize;
        let label = inked(&px, edge..middle, top);
        let first = *label.first().expect("a row with no label");
        assert!(
            (edge..=edge + 8).contains(&first),
            "{row:?}'s label starts at x {first}"
        );
        let value = inked(&px, middle..OUT_W as usize, top);
        let last = *value.last().expect("a row with no value");
        assert!(
            (right - 8..=right + 1).contains(&last),
            "{row:?}'s value ends at x {last}"
        );
    }
}
