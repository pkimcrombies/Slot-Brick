use slot_gfx::{Compositor, Draw, TexId, OUT_H, OUT_W};
use slot_ui::{
    menu_text_face, pixel_aligned_texture, UndoFace, QUICK_EDGE,
};

use crate::music::{Music, Status, Track};
use crate::music_cover::{MusicCover, THUMBNAIL};

// Music alone uses a warm-white canvas. System menus and the launcher keep
// their dark palette; do not change the global MENU_* constants.
const MUSIC_BACKGROUND: [f32; 4] = [248.0 / 255.0, 247.0 / 255.0, 244.0 / 255.0, 1.0];
const MUSIC_TEXT_INK: [u8; 3] = [0x24, 0x24, 0x24];
const MUSIC_MUTED_INK: [u8; 3] = [0x74, 0x6e, 0x6b];
const MUSIC_SELECTION_INK: [u8; 3] = [0xa0, 0x50, 0x69];
const MUSIC_PROGRESS_TRACK: [f32; 4] = [0.84, 0.83, 0.82, 1.0];
const MUSIC_COVER_PLACEHOLDER: [f32; 4] = [0.92, 0.91, 0.90, 1.0];

const EDGE: f32 = QUICK_EDGE;
const ROWS: usize = 6;
const TITLE_TOP: f32 = 42.0;
const LIST_TOP: f32 = 104.0;
const ROW_H: f32 = 48.0;
const ALBUM_COVER_SIDE: f32 = 40.0;
const ALBUM_TEXT_X: f32 = EDGE + ALBUM_COVER_SIDE + 14.0;
// ClassiPod-inspired composition, retaining Slot's native 640x480 layout,
// typography, embedded artwork decoder and physical controls.
const PLAYING_COVER_SIDE: f32 = 214.0;
const PLAYING_COVER_X: f32 = EDGE;
const PLAYING_COVER_Y: f32 = 113.0;
const PLAYING_INFO_X: f32 = EDGE + PLAYING_COVER_SIDE + 24.0;
const PLAYING_INFO_W: u32 = (OUT_W as f32 - PLAYING_INFO_X - EDGE - 4.0) as u32;
const PLAYING_PROGRESS_Y: f32 = 372.0;

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
        // as a full-height background fading out into the warm-white left side.
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
        p.rect([0.0, 0.0], [OUT_W as f32, OUT_H as f32], MUSIC_BACKGROUND);
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
        // When cover-fit overflows left, hide it behind the warm-white reading area.
        let left = OUT_W as f32 - side;
        self.rect([0.0, 0.0], [left, side], MUSIC_BACKGROUND);
        self.draws.push(pixel_aligned_texture(
            fade, [left, 0.0], [side, side], self.scale, 1.0,
        ));
    }

    fn browser(&mut self, music: &Music, album_art: &[Option<(TexId, [u32; 2])>; ROWS]) {
        if music.is_root() {
            let (albums, artists) = if music.viewing_artists() {
                (MUSIC_MUTED_INK, MUSIC_TEXT_INK)
            } else {
                (MUSIC_TEXT_INK, MUSIC_MUTED_INK)
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
            let ink = if i == cursor { MUSIC_SELECTION_INK } else { MUSIC_TEXT_INK };
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
                self.text_ink(subtitle, 14, [x, y + 28.0], MUSIC_MUTED_INK);
            } else {
                self.text_ink(label, 24, [x, y + 6.0], ink);
            }
        }
    }

    fn playing(&mut self, music: &Music, track: &Track, cover: Option<(TexId, [u32; 2])>) {
        self.heading("now playing");
        let song = music.song(track);
        let album = music.playing_album();

        // Keep the artwork area stable while covers load asynchronously or
        // when the file has no embedded image; metadata never jumps sideways.
        self.rect(
            [PLAYING_COVER_X, PLAYING_COVER_Y],
            [PLAYING_COVER_SIDE, PLAYING_COVER_SIDE],
            MUSIC_COVER_PLACEHOLDER,
        );
        if let Some((texture, pixels)) = cover {
            let size = pixels.map(|n| n as f32 * PLAYING_COVER_SIDE / THUMBNAIL as f32);
            self.draws.push(pixel_aligned_texture(
                texture,
                [
                    PLAYING_COVER_X + (PLAYING_COVER_SIDE - size[0]) / 2.0,
                    PLAYING_COVER_Y + (PLAYING_COVER_SIDE - size[1]) / 2.0,
                ],
                size,
                self.scale,
                1.0,
            ));
        }

        // ClassiPod's artwork-left, information-right hierarchy, rendered with
        // Slot's own text faces. All strings remain ellipsized to this column.
        self.text_with_room(
            &song.title, 26, [PLAYING_INFO_X, 128.0], MUSIC_SELECTION_INK, PLAYING_INFO_W,
        );
        self.text_with_room(
            &song.artist, 19, [PLAYING_INFO_X, 185.0], MUSIC_TEXT_INK, PLAYING_INFO_W,
        );
        self.text_with_room(
            &album.title, 17, [PLAYING_INFO_X, 221.0], MUSIC_MUTED_INK, PLAYING_INFO_W,
        );

        let position = format!("{} of {}", music.cursor() + 1, music.row_count());
        self.text_with_room(
            &position, 16, [PLAYING_INFO_X, 277.0], MUSIC_MUTED_INK, PLAYING_INFO_W,
        );
        let state = match track.status {
            Status::Loading => "loading",
            Status::Playing => "playing",
            Status::Paused => "paused",
            Status::Finished => "finished",
            Status::Unreadable => "could not play this track",
            Status::NoAudio => "audio output unavailable",
        };
        self.text_with_room(
            state, 15, [PLAYING_INFO_X, 306.0], MUSIC_MUTED_INK, PLAYING_INFO_W,
        );

        // The transport remains physical-button driven (A, B, left/right,
        // L1/R1). Put the seek indicator at the bottom instead of adding
        // faux touch controls or changing playback behaviour.
        let width = OUT_W as f32 - 2.0 * EDGE;
        self.rect([EDGE, PLAYING_PROGRESS_Y], [width, 4.0], MUSIC_PROGRESS_TRACK);
        if let Some((elapsed, total)) = track.time() {
            if let Some(total) = total {
                let fraction = if total > 0.0 {
                    (elapsed / total).clamp(0.0, 1.0) as f32
                } else {
                    0.0
                };
                if fraction > 0.0 {
                    let rose = MUSIC_SELECTION_INK.map(|n| n as f32 / 255.0);
                    self.rect(
                        [EDGE, PLAYING_PROGRESS_Y], [width * fraction, 4.0],
                        [rose[0], rose[1], rose[2], 1.0],
                    );
                }
                self.text_ink(&timestamp(elapsed), 17, [EDGE, 392.0], MUSIC_MUTED_INK);
                self.text_right(
                    &timestamp(total), 17, OUT_W as f32 - EDGE, 392.0, MUSIC_MUTED_INK,
                );
            } else {
                self.text_ink(&timestamp(elapsed), 17, [EDGE, 392.0], MUSIC_MUTED_INK);
            }
        }
    }

    /// Place the time readout against the right edge of the progress bar.
    fn text_right(&mut self, text: &str, px: u32, right: f32, y: f32, ink: [u8; 3]) {
        let key = format!("{px}/120/{ink:?}/{text}");
        let face = self.texture(key, |scale| menu_text_face(text, px, 120, scale, ink));
        self.place(face, right - face.size[0] as f32, y);
    }

    fn heading(&mut self, title: &str) {
        self.text(title, 26, [EDGE, TITLE_TOP]);
    }

    fn text(&mut self, text: &str, px: u32, at: [f32; 2]) {
        self.text_ink(text, px, at, MUSIC_TEXT_INK);
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

/// A single 256-pixel warm-white-to-transparent overlay (one GPU draw per frame).
/// Preserves legibility of dark text on the left, without rectangular bands.
fn backdrop_fade_rgba() -> Vec<u8> {
    (0..256).flat_map(|x| {
        let t = x as f32 / 255.0;
        let smooth = t * t * (3.0 - 2.0 * t);
        let alpha = (0.04 + 0.96 * (1.0 - smooth)) * 255.0;
        [248, 247, 244, alpha.round() as u8]
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
    fn now_playing_art_and_text_are_separate_at_640_by_480() {
        assert_eq!((OUT_W, OUT_H), (640, 480));
        assert!(PLAYING_COVER_SIDE > 200.0);
        assert!(PLAYING_COVER_X + PLAYING_COVER_SIDE + 12.0 < PLAYING_INFO_X);
        assert!(PLAYING_INFO_W >= 270);
        assert!(PLAYING_INFO_X + PLAYING_INFO_W as f32 <= OUT_W as f32 - EDGE);
        assert!(PLAYING_COVER_Y + PLAYING_COVER_SIDE < PLAYING_PROGRESS_Y);
        assert!(PLAYING_PROGRESS_Y + 48.0 < OUT_H as f32);
    }

    #[test]
    fn album_backdrop_has_a_warm_white_left_and_soft_right_edge() {
        let pixels = backdrop_fade_rgba();
        assert_eq!(pixels.len(), 256 * 4);
        assert_eq!(&pixels[0..3], &[248, 247, 244]);
        assert_eq!(pixels[3], 255);
        assert_eq!(&pixels[255 * 4..255 * 4 + 3], &[248, 247, 244]);
        assert!(pixels[255 * 4 + 3] < 40);
        for x in 1..256 {
            assert!(pixels[(x - 1) * 4 + 3] >= pixels[x * 4 + 3]);
        }
    }

    #[test]
    fn music_has_its_own_light_palette_without_changing_system_menus() {
        assert_eq!(MUSIC_BACKGROUND, [248.0 / 255.0, 247.0 / 255.0, 244.0 / 255.0, 1.0]);
        assert_eq!(MUSIC_TEXT_INK, [0x24, 0x24, 0x24]);
        assert_ne!(MUSIC_BACKGROUND, slot_ui::MENU_BACKGROUND);
        assert_ne!(MUSIC_TEXT_INK, slot_ui::MENU_TEXT_INK);
        assert!(MUSIC_SELECTION_INK[0] < 180, "selected album must remain visible on white");
    }
}
