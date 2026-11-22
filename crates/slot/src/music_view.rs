use slot_gfx::{Compositor, Draw, TexId, OUT_H, OUT_W};
use slot_ui::{
    menu_text_face, pixel_aligned_texture, UndoFace, MENU_BACKGROUND, MENU_MUTED_INK,
    MENU_SELECTION_INK, MENU_TEXT_INK, QUICK_EDGE,
};

use crate::music::{Music, Status, Track};
use crate::music_cover::{MusicCover, THUMBNAIL};

const EDGE: f32 = QUICK_EDGE;
const ROWS: usize = 6;
const TITLE_TOP: f32 = 42.0;
const LIST_TOP: f32 = 104.0;
const ROW_H: f32 = 48.0;
const ALBUM_COVER_SIDE: f32 = 40.0;
const ALBUM_TEXT_X: f32 = EDGE + ALBUM_COVER_SIDE + 14.0;
const COVER_SIDE: f32 = 128.0;
const COVER_X: f32 = OUT_W as f32 - EDGE - COVER_SIDE;
const COVER_Y: f32 = 124.0;

#[derive(Clone, Copy)]
struct Face {
    tex: TexId,
    size: [u32; 2],
}

struct CachedFace {
    key: String,
    face: Face,
}

#[derive(Default)]
pub(crate) struct MusicView {
    cache: Vec<CachedFace>,
    cover: MusicCover,
    album_covers: [MusicCover; ROWS],
    album_backdrop: MusicCover,
    fade_texture: Option<TexId>,
}

impl MusicView {
    pub(crate) fn render(
        &mut self,
        music: &Music,
        compositor: &mut Compositor,
        scale: [f32; 2],
    ) -> Vec<Draw> {
        let cover = music.track.as_ref().and_then(|track| {
            self.cover.texture(&music.song(track).path, music.playing_album(), compositor)
        });
        // Reuse a fixed number of cover decoders and GPU textures as the list scrolls.
        // Artwork is decoded off-thread by MusicCover and never blocks the render loop.
        let first = music.cursor().saturating_sub(ROWS - 1);
        let mut album_art = [None; ROWS];
        if music.track.is_none() {
            for (slot, art) in album_art.iter_mut().enumerate() {
                if let Some(album) = music.album_for_row(first + slot) {
                    if let Some(song) = album.songs.first() {
                        *art = self.album_covers[slot].texture(&song.path, album, compositor);
                    }
                }
            }
        }
        // The album track screen keeps its list, with the album's embedded art
        // as a full-height background fading out into the black left side.
        let detail_art = if music.track.is_none() && music.showing_tracks() {
            let album = music.playing_album();
            album.songs.first().and_then(|song| {
                self.album_backdrop.texture(&song.path, album, compositor)
            })
        } else {
            None
        };
        let fade_texture = if detail_art.is_some() {
            Some(*self.fade_texture.get_or_insert_with(|| {
                compositor.create_texture(256, 1, &backdrop_fade_rgba())
            }))
        } else {
            None
        };
        let mut p = Painter {
            cache: &mut self.cache,
            index: 0,
            compositor,
            scale,
            draws: Vec::new(),
        };
        p.rect([0.0, 0.0], [OUT_W as f32, OUT_H as f32], MENU_BACKGROUND);
        if let (Some(art), Some(fade)) = (detail_art, fade_texture) {
            p.album_backdrop(art, fade);
        }
        if let Some(track) = &music.track {
            p.playing(music, track, cover);
        } else {
            p.browser(music, &album_art);
        }
        p.draws
    }
}

struct Painter<'a> {
    cache: &'a mut Vec<CachedFace>,
    index: usize,
    compositor: &'a mut Compositor,
    scale: [f32; 2],
    draws: Vec<Draw>,
}

impl Painter<'_> {
    fn album_backdrop(&mut self, (texture, pixels): (TexId, [u32; 2]), fade: TexId) {
        // Cover-fit without distortion: keep the artwork's right edge aligned to
        // the panel. The GPU clips any excess at the top or bottom.
        let side = OUT_H as f32;
        let cover_scale = (side / pixels[0] as f32).max(side / pixels[1] as f32);
        let width = pixels[0] as f32 * cover_scale;
        let height = pixels[1] as f32 * cover_scale;
        self.draws.push(pixel_aligned_texture(
            texture,
            [OUT_W as f32 - width, (OUT_H as f32 - height) / 2.0],
            [width, height],
            self.scale,
            1.0,
        ));
        // When cover-fit overflows left, hide it behind the black reading area.
        let left = OUT_W as f32 - side;
        self.rect([0.0, 0.0], [left, side], MENU_BACKGROUND);
        self.draws.push(pixel_aligned_texture(
            fade, [left, 0.0], [side, side], self.scale, 1.0,
        ));
    }

    fn browser(&mut self, music: &Music, album_art: &[Option<(TexId, [u32; 2])>; ROWS]) {
        if music.is_root() {
            let (albums, artists) = if music.viewing_artists() {
                (MENU_MUTED_INK, MENU_TEXT_INK)
            } else {
                (MENU_TEXT_INK, MENU_MUTED_INK)
            };
            let width = self.text_ink("albums", 24, [EDGE, TITLE_TOP], albums);
            self.text_ink("artists", 24, [EDGE + width as f32 + 22.0, TITLE_TOP], artists);
        } else {
            self.heading(music.label());
        }
        if music.row_count() == 0 {
            let message = if music.scanning() {
                "reading music tags"
            } else {
                "copy mp3 or m4a files to slot/music"
            };
            self.text(message, 20, [EDGE, LIST_TOP]);
            return;
        }
        let cursor = music.cursor();
        let first = cursor.saturating_sub(ROWS - 1);
        for i in first..(first + ROWS).min(music.row_count()) {
            let y = LIST_TOP + (i - first) as f32 * ROW_H;
            let ink = if i == cursor { MENU_SELECTION_INK } else { MENU_TEXT_INK };
            let (label, subtitle) = music.row(i);
            // Albums gain a thumbnail; artist and track rows keep their existing layout.
            let x = if music.album_for_row(i).is_some() {
                if let Some((texture, pixels)) = album_art[i - first] {
                    let size = pixels.map(|n| n as f32 * ALBUM_COVER_SIDE / THUMBNAIL as f32);
                    self.draws.push(pixel_aligned_texture(
                        texture,
                        [
                            EDGE + (ALBUM_COVER_SIDE - size[0]) / 2.0,
                            y + (ROW_H - size[1]) / 2.0,
                        ],
                        size,
                        self.scale,
                        1.0,
                    ));
                }
                ALBUM_TEXT_X
            } else {
                EDGE
            };
            if let Some(subtitle) = subtitle {
                self.text_ink(label, 23, [x, y + 1.0], ink);
                self.text_ink(subtitle, 14, [x, y + 28.0], MENU_MUTED_INK);
            } else {
                self.text_ink(label, 24, [x, y + 6.0], ink);
            }
        }
    }

    fn playing(&mut self, music: &Music, track: &Track, cover: Option<(TexId, [u32; 2])>) {
        self.heading("now playing");
        let song = music.song(track);
        let room = if let Some((texture, pixels)) = cover {
            let size = pixels.map(|n| n as f32 * COVER_SIDE / THUMBNAIL as f32);
            self.draws.push(pixel_aligned_texture(
                texture,
                [COVER_X + (COVER_SIDE - size[0]) / 2.0, COVER_Y + (COVER_SIDE - size[1]) / 2.0],
                size,
                self.scale,
                1.0,
            ));
            (COVER_X - EDGE - 22.0) as u32
        } else {
            (OUT_W as f32 - 2.0 * EDGE - 4.0) as u32
        };
        self.text_with_room(&song.title, 26, [EDGE, 147.0], MENU_SELECTION_INK, room);
        self.text_with_room(&song.artist, 18, [EDGE, 188.0], MENU_MUTED_INK, room);
        let state = match track.status {
            Status::Loading => "loading",
            Status::Playing => "playing",
            Status::Paused => "paused",
            Status::Finished => "finished",
            Status::Unreadable => "could not play this track",
            Status::NoAudio => "audio output unavailable",
        };
        self.text(state, 16, [EDGE, 230.0]);
        if let Some((elapsed, total)) = track.time() {
            if let Some(total) = total {
                let (x, y, width) = (EDGE, 269.0, OUT_W as f32 - 2.0 * EDGE);
                self.rect([x, y], [width, 4.0], [0.54, 0.53, 0.53, 1.0]);
                let fraction = (elapsed / total).clamp(0.0, 1.0) as f32;
                if fraction > 0.0 {
                    self.rect([x, y], [width * fraction, 4.0], [0.96, 0.95, 0.93, 1.0]);
                }
                self.text(
                    &format!("{} / {}", timestamp(elapsed), timestamp(total)),
                    17,
                    [EDGE, 284.0],
                );
            } else {
                self.text(&timestamp(elapsed), 17, [EDGE, 284.0]);
            }
        }
    }

    fn heading(&mut self, title: &str) {
        self.text(title, 26, [EDGE, TITLE_TOP]);
    }

    fn text(&mut self, text: &str, px: u32, at: [f32; 2]) {
        self.text_ink(text, px, at, MENU_TEXT_INK);
    }

    fn text_ink(&mut self, text: &str, px: u32, at: [f32; 2], ink: [u8; 3]) -> u32 {
        let room = OUT_W.saturating_sub((at[0] + EDGE) as u32 + 4);
        self.text_with_room(text, px, at, ink, room)
    }

    fn text_with_room(&mut self, text: &str, px: u32, [x, y]: [f32; 2], ink: [u8; 3], room: u32) -> u32 {
        let key = format!("{px}/{room}/{ink:?}/{text}");
        let face = self.texture(key, |scale| menu_text_face(text, px, room, scale, ink));
        self.place(face, x, y);
        face.size[0]
    }

    fn texture(
        &mut self,
        key: String,
        make: impl FnOnce([f32; 2]) -> (UndoFace, [u32; 2]),
    ) -> Face {
        let i = self.index;
        self.index += 1;
        if let Some(cached) = self.cache.get_mut(i) {
            if cached.key != key {
                let (image, size) = make(self.scale);
                self.compositor
                    .update_texture(cached.face.tex, image.w, image.h, &image.rgba);
                cached.face.size = size;
                cached.key = key;
            }
            return cached.face;
        }
        let (image, size) = make(self.scale);
        let face = Face {
            tex: self
                .compositor
                .create_texture(image.w, image.h, &image.rgba),
            size,
        };
        self.cache.push(CachedFace { key, face });
        face
    }

    fn place(&mut self, face: Face, x: f32, y: f32) {
        if face.size[0] > 0 {
            self.draws.push(pixel_aligned_texture(
                face.tex,
                [x, y],
                face.size.map(|n| n as f32),
                self.scale,
                1.0,
            ));
        }
    }

    fn rect(&mut self, [x, y]: [f32; 2], [w, h]: [f32; 2], colour: [f32; 4]) {
        self.draws.push(Draw::Rect { x, y, w, h, colour });
    }
}

/// A single 256-pixel black-to-transparent overlay (one GPU draw per frame).
/// Preserves legibility of the left-aligned text without rectangular bands.
fn backdrop_fade_rgba() -> Vec<u8> {
    (0..256).flat_map(|x| {
        let t = x as f32 / 255.0;
        let smooth = t * t * (3.0 - 2.0 * t);
        let alpha = (0.10 + 0.90 * (1.0 - smooth)) * 255.0;
        [0, 0, 0, alpha.round() as u8]
    }).collect()
}

fn timestamp(seconds: f64) -> String {
    let seconds = seconds as u64;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

#[cfg(test)]
mod album_thumbnail_layout_tests {
    use super::*;

    #[test]
    fn album_thumbnails_fit_existing_rows_without_overlapping_text() {
        assert!(ALBUM_COVER_SIDE <= ROW_H);
        assert!(ALBUM_TEXT_X >= EDGE + ALBUM_COVER_SIDE);
        assert!(ALBUM_TEXT_X < OUT_W as f32 - EDGE);
    }

    #[test]
    fn album_backdrop_has_an_opaque_left_and_soft_right_edge() {
        let pixels = backdrop_fade_rgba();
        assert_eq!(pixels.len(), 256 * 4);
        assert_eq!(pixels[3], 255);
        assert!(pixels[255 * 4 + 3] < 40);
        for x in 1..256 {
            assert!(pixels[(x - 1) * 4 + 3] >= pixels[x * 4 + 3]);
        }
    }
}
