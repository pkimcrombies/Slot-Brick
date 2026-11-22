use std::time::{Duration, Instant};

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
const FLOW_SLOTS: usize = 7;
const FLOW_DURATION: Duration = Duration::from_millis(260);
const FLOW_COVER_Y: f32 = 116.0;
const FLOW_FRONT_SIDE: f32 = 212.0;
const FLOW_REFLECT_H: f32 = 40.0;

/// Navigation state stays in Music; only transient visual interpolation lives
/// here. The renderer requests frames only during the 260ms movement.
#[derive(Default)]
struct FlowMotion {
    position: Option<f32>,
    travel: Option<(Instant, f32, f32)>,
}

impl FlowMotion {
    fn at(&self, now: Instant) -> Option<f32> {
        let (start, from, to) = match self.travel {
            Some(travel) => travel,
            None => return self.position,
        };
        let t = (now.saturating_duration_since(start).as_secs_f32()
            / FLOW_DURATION.as_secs_f32()).clamp(0.0, 1.0);
        let eased = t * t * (3.0 - 2.0 * t);
        Some(from + (to - from) * eased)
    }

    fn to(&mut self, cursor: usize, now: Instant) -> f32 {
        let target = cursor as f32;
        if self.position.is_none() {
            self.position = Some(target);
            return target;
        }
        if self.position != Some(target) {
            let current = self.at(now).unwrap_or(target);
            self.position = Some(target);
            self.travel = Some((now, current, target));
        }
        self.at(now).unwrap_or(target)
    }

    fn active(&self, now: Instant) -> bool {
        self.travel.is_some_and(|(at, _, _)| now.saturating_duration_since(at) < FLOW_DURATION)
    }

    fn clear(&mut self) {
        self.position = None;
        self.travel = None;
    }
}

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
    flow_covers: [MusicCover; FLOW_SLOTS],
    flow_motion: FlowMotion,
    flow_reflection_fade: Option<TexId>,
}

impl MusicView {
    pub(crate) fn cover_flow_animating(&self) -> bool {
        self.flow_motion.active(Instant::now())
    }

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
        if music.track.is_none() && !music.cover_flow() {
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
        // A separate bounded cache for Cover Flow avoids synchronous decoding
        // and never evicts the list's existing thumbnail textures.
        let now = Instant::now();
        let flow = if music.cover_flow() && music.row_count() > 0 {
            Some(self.flow_motion.to(music.cursor(), now))
        } else {
            self.flow_motion.clear();
            None
        };
        let flow_first = flow.map_or(0, |position| position.round() as isize - 3);
        let mut flow_art = [None; FLOW_SLOTS];
        if flow.is_some() {
            for (slot, item) in flow_art.iter_mut().enumerate() {
                let album_index = flow_first + slot as isize;
                if album_index < 0 {
                    continue;
                }
                let album_index = album_index as usize;
                if let Some(album) = music.album_for_row(album_index) {
                    if let Some(song) = album.songs.first() {
                        // Modulo slot ownership remains stable when scrolling
                        // through adjacent album indices.
                        *item = self.flow_covers[album_index % FLOW_SLOTS]
                            .texture(&song.path, album, compositor);
                    }
                }
            }
        }
        let reflection_fade = if flow.is_some() {
            Some(*self.flow_reflection_fade.get_or_insert_with(|| {
                compositor.create_texture(1, 64, &reflection_fade_rgba())
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
        } else if let (Some(position), Some(reflection_fade)) = (flow, reflection_fade) {
            p.cover_flow(music, &flow_art, flow_first, position, reflection_fade);
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
            if !music.viewing_artists() {
                self.text_right("x covers", 16, OUT_W as f32 - EDGE, TITLE_TOP + 7.0, MUSIC_MUTED_INK);
            }
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

    /// Cover Flow's depth is a cheap native 2D projection: nearby covers are
    /// foreshortened and gently tilted, with vertically mirrored reflections.
    /// Far items paint first so the focused album stays in front.
    fn cover_flow(
        &mut self,
        music: &Music,
        art: &[Option<(TexId, [u32; 2])>; FLOW_SLOTS],
        first: isize,
        position: f32,
        reflection_fade: TexId,
    ) {
        let label = self.text_ink("albums", 24, [EDGE, TITLE_TOP], MUSIC_SELECTION_INK);
        self.text_ink("artists", 24, [EDGE + label as f32 + 22.0, TITLE_TOP], MUSIC_MUTED_INK);
        self.text_right("x list", 16, OUT_W as f32 - EDGE, TITLE_TOP + 7.0, MUSIC_MUTED_INK);

        // Sort by distance from the moving centre (far -> near).
        let mut order = [0usize, 1, 2, 3, 4, 5, 6];
        order.sort_by(|&a, &b| {
            let distance_a = ((first + a as isize) as f32 - position).abs();
            let distance_b = ((first + b as isize) as f32 - position).abs();
            distance_b.total_cmp(&distance_a)
        });
        for slot in order {
            let index = first + slot as isize;
            if index < 0 || index as usize >= music.row_count() {
                continue;
            }
            let distance = index as f32 - position;
            let Some((x, y, w, h, alpha, turn)) = flow_geometry(distance) else {
                continue;
            };
            // An artwork-sized paper tile is deliberately shown while asynchronous
            // embedded-art decoding runs (and for albums with no embedded art).
            self.rect([x - 2.0, y - 2.0], [w + 4.0, h + 4.0],
                [0.85, 0.84, 0.83, alpha * 0.65]);
            self.rect([x, y], [w, h], MUSIC_COVER_PLACEHOLDER);
            if let Some((tex, pixels)) = art[slot] {
                let pw = w * pixels[0] as f32 / THUMBNAIL as f32;
                let ph = h * pixels[1] as f32 / THUMBNAIL as f32;
                let px = x + (w - pw) / 2.0;
                let py = y + (h - ph) / 2.0;
                self.draws.push(Draw::Turned {
                    x: px, y: py, w: pw, h: ph, tex, alpha, turn,
                });
                // A negative-height sprite mirrors the cover vertically without
                // a second decoded texture. The small gradient softens the end.
                let reflect_y = py + ph + 4.0;
                self.draws.push(Draw::Tex {
                    x: px, y: reflect_y + FLOW_REFLECT_H, w: pw,
                    h: -FLOW_REFLECT_H, tex, alpha: alpha * 0.19,
                });
                self.draws.push(Draw::Tex {
                    x: px, y: reflect_y, w: pw, h: FLOW_REFLECT_H,
                    tex: reflection_fade, alpha: 1.0,
                });
            }
        }

        let (album, artist) = music.row(music.cursor());
        self.text_center(album, 26, 382.0, MUSIC_TEXT_INK, 544);
        if let Some(artist) = artist {
            self.text_center(artist, 18, 421.0, MUSIC_MUTED_INK, 544);
        }
        self.text_center(
            &format!("{} / {}", music.cursor() + 1, music.row_count()),
            15, 450.0, MUSIC_MUTED_INK, 160,
        );
    }

    fn text_center(&mut self, text: &str, px: u32, y: f32, ink: [u8; 3], room: u32) {
        let key = format!("{px}/{room}/{ink:?}/{text}");
        let face = self.texture(key, |scale| menu_text_face(text, px, room, scale, ink));
        self.place(face, (OUT_W as f32 - face.size[0] as f32) / 2.0, y);
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

/// Returns the projected artwork rectangle and subtle side tilt for a moving
/// album. Culling keeps GPU work bounded, even for large music libraries.
fn flow_geometry(distance: f32) -> Option<(f32, f32, f32, f32, f32, f32)> {
    let abs = distance.abs();
    if abs > 2.7 {
        return None;
    }
    let near = (1.0 - abs).clamp(0.0, 1.0);
    let w = 96.0 + (FLOW_FRONT_SIDE - 96.0) * near;
    let h = 158.0 + (FLOW_FRONT_SIDE - 158.0) * near;
    let span = abs.min(1.0) * 178.0 + (abs - 1.0).max(0.0) * 71.0;
    let x = OUT_W as f32 / 2.0 + distance.signum() * span - w / 2.0;
    let y = FLOW_COVER_Y + (FLOW_FRONT_SIDE - h) / 2.0;
    let alpha = (1.0 - abs * 0.30).clamp(0.18, 1.0);
    let turn = if abs < 0.01 { 0.0 } else { distance.signum() * 0.025 };
    Some((x, y, w, h, alpha, turn))
}

/// Warm-white gradient to dissolve mirrored artwork into the Music canvas.
fn reflection_fade_rgba() -> Vec<u8> {
    (0..64).flat_map(|y| {
        let t = y as f32 / 63.0;
        let eased = t * t * (3.0 - 2.0 * t);
        [248, 247, 244, (255.0 * eased).round() as u8]
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
    fn cover_flow_projection_keeps_a_focused_cover_between_two_smaller_neighbours() {
        let focused = flow_geometry(0.0).unwrap();
        let left = flow_geometry(-1.0).unwrap();
        let right = flow_geometry(1.0).unwrap();
        assert_eq!(focused.2, FLOW_FRONT_SIDE);
        assert!(left.2 < focused.2 && right.2 < focused.2);
        assert!(left.0 + left.2 < focused.0);
        assert!(right.0 > focused.0 + focused.2);
        assert_eq!(flow_geometry(3.0), None);
        assert!(focused.4 > left.4);
        assert!(left.5 < 0.0 && right.5 > 0.0);
    }

    #[test]
    fn cover_flow_animation_eases_and_finishes_without_idle_redraws() {
        let now = Instant::now();
        let mut motion = FlowMotion::default();
        assert_eq!(motion.to(2, now), 2.0);
        assert_eq!(motion.to(3, now), 2.0);
        assert!(motion.active(now + FLOW_DURATION / 2));
        let halfway = motion.at(now + FLOW_DURATION / 2).unwrap();
        assert!(halfway > 2.0 && halfway < 3.0);
        assert_eq!(motion.at(now + FLOW_DURATION), Some(3.0));
        assert!(!motion.active(now + FLOW_DURATION));
        motion.clear();
        assert_eq!(motion.at(now), None);
    }

    #[test]
    fn rapid_cover_flow_navigation_continues_from_the_interpolated_position() {
        let now = Instant::now();
        let mut motion = FlowMotion::default();
        motion.to(2, now);
        motion.to(3, now);
        let midway = now + FLOW_DURATION / 2;
        let before = motion.at(midway).unwrap();
        assert!((2.0..3.0).contains(&before));
        let next = motion.to(4, midway);
        assert!((next - before).abs() < 0.001, "navigation jumped instead of retargeting");
        assert_eq!(motion.at(midway + FLOW_DURATION), Some(4.0));
    }

    #[test]
    fn reflection_dissolves_into_the_warm_white_canvas() {
        let pixels = reflection_fade_rgba();
        assert_eq!(pixels.len(), 64 * 4);
        assert_eq!(&pixels[0..4], &[248, 247, 244, 0]);
        assert_eq!(&pixels[63 * 4..63 * 4 + 4], &[248, 247, 244, 255]);
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
