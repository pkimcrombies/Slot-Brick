#![cfg(feature = "sim")]

use slot_gfx::{Compositor, Draw, FbdevSurface, OUT_H, OUT_W};

#[test]
fn video_shader_converts_planes_and_updates_the_same_texture() {
    let surface = FbdevSurface::offscreen((OUT_W, OUT_H)).expect("offscreen GLES");
    let mut compositor = Compositor::new(&surface).expect("Slot compositor");
    let start = (10, 20);
    let draw = Draw::Video { x: 10.0, y: 20.0, w: 3.0, h: 3.0 };
    let pixels = |compositor: &Compositor, x: usize, y: usize| {
        let rgba = compositor.read_frame();
        let offset = ((start.1 + y) * OUT_W as usize + start.0 + x) * 4;
        [rgba[offset], rgba[offset + 1], rgba[offset + 2]]
    };
    let near = |actual: [u8; 3], expected: [u8; 3]| {
        assert!(actual.into_iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 2),
                "pixel {actual:?}, expected {expected:?}");
    };

    compositor.begin_frame();
    compositor.upload_video_420(
        3, 3,
        [
            &[16, 64, 128, 235, 81, 145, 22, 150, 210],
            &[90, 240, 128, 54],
            &[240, 16, 128, 200],
        ],
    ).expect("three YUV planes");
    compositor.draw_list(&[draw]);
    near(pixels(&compositor, 0, 0), [179, 0, 0]);
    near(pixels(&compositor, 2, 0), [0, 178, 255]);
    near(pixels(&compositor, 2, 2), [255, 196, 77]);

    compositor.begin_frame();
    compositor.upload_video_420(3, 3, [&[235; 9], &[128; 4], &[128; 4]])
        .expect("same-size frame");
    compositor.draw_list(&[draw]);
    near(pixels(&compositor, 0, 0), [255, 255, 255]);
    assert!(compositor.upload_video_420(3, 3, [&[16; 9], &[128; 3], &[128; 4]]).is_err());
}
