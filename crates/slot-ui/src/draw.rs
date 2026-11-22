pub use slot_gfx::{Draw, TexId, OUT_H, OUT_W};

/// Place a texture rasterized at display resolution on the same physical pixel grid.
pub(crate) fn pixel_aligned_texture(
    tex: TexId,
    [x, y]: [f32; 2],
    [w, h]: [f32; 2],
    [sx, sy]: [f32; 2],
    alpha: f32,
) -> Draw {
    Draw::Tex {
        x: (x * sx).round() / sx,
        y: (y * sy).round() / sy,
        w: (w * sx).round().max(1.0) / sx,
        h: (h * sy).round().max(1.0) / sy,
        tex,
        alpha,
    }
}
