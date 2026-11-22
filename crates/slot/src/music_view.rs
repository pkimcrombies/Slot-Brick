use slot_gfx::{Compositor, Draw, TexId, OUT_H, OUT_W};
use slot_ui::{
    centred_hints, edge, menu_hint_face_scaled, music_text_face, pixel_aligned_texture, UndoFace,
    HINT_H, LEGEND_GAP, LEGEND_Y, MENU_BACKGROUND,
};

use crate::music::{Music, Status, Track};

const EDGE: f32 = 32.0;
const CENTER: f32 = OUT_W as f32 / 2.0;
const ROWS: usize = 6;
const LIST_TOP: f32 = 104.0;
const ROW_H: f32 = 48.0;
const ROOM: u32 = OUT_W - 80;

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
}

impl MusicView {
    pub(crate) fn render(
        &mut self,
        music: &Music,
        compositor: &mut Compositor,
        scale: [f32; 2],
    ) -> Vec<Draw> {
        let mut p = Painter {
            cache: &mut self.cache,
            index: 0,
            compositor,
            scale,
            draws: Vec::new(),
        };
        p.rect([0.0, 0.0], [OUT_W as f32, OUT_H as f32], MENU_BACKGROUND);
        if let Some(track) = &music.track {
            p.playing(music, track);
        } else {
            p.browser(music);
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
    fn browser(&mut self, music: &Music) {
        self.text(music.label(), 27, [EDGE, 45.0], false);
        if music.row_count() == 0 {
            let message = if music.scanning() {
                "reading music tags"
            } else {
                "copy mp3 files to slot/music"
            };
            self.text(message, 22, [CENTER, 212.0], true);
            self.hints(&[("B", "back")]);
            return;
        }
        let cursor = music.cursor();
        let first = cursor.saturating_sub(ROWS - 1);
        for i in first..(first + ROWS).min(music.row_count()) {
            let y = LIST_TOP + (i - first) as f32 * ROW_H;
            if i == cursor {
                let mut highlight = edge();
                highlight[3] = 0.24;
                self.rect([0.0, y], [OUT_W as f32, ROW_H], highlight);
                self.rect(
                    [EDGE - 12.0, y + 9.0],
                    [3.0, ROW_H - 18.0],
                    [0.96, 0.95, 0.93, 1.0],
                );
            }
            let (label, subtitle) = music.row(i);
            if let Some(subtitle) = subtitle {
                self.text(label, 21, [EDGE, y + 2.0], false);
                self.text(subtitle, 13, [EDGE, y + 27.0], false);
            } else {
                self.text(label, 24, [EDGE, y + 6.0], false);
            }
        }
        let mut hints = vec![("B", "back")];
        if music.is_root() {
            hints.push(("L1/R1", "view"));
        }
        if music.can_play_album() {
            hints.push(("Y", "play album"));
        }
        hints.push((
            "A",
            if music.showing_tracks() {
                "play"
            } else {
                "open"
            },
        ));
        self.hints(&hints);
    }

    fn playing(&mut self, music: &Music, track: &Track) {
        self.text("now playing", 27, [EDGE, 45.0], false);
        let song = music.song(track);
        self.text(&song.title, 35, [CENTER, 147.0], true);
        self.text(&song.artist, 21, [CENTER, 198.0], true);
        let (state, toggle) = match track.status {
            Status::Loading => ("loading", "pause"),
            Status::Playing => ("playing", "pause"),
            Status::Paused => ("paused", "resume"),
            Status::Finished => ("finished", "replay"),
            Status::Unreadable => ("could not read this mp3", "retry"),
            Status::NoAudio => ("audio output unavailable", "retry"),
        };
        self.text(state, 17, [CENTER, 244.0], true);
        if let Some((elapsed, total)) = track.time() {
            if let Some(total) = total {
                let (x, y, width) = (84.0, 279.0, OUT_W as f32 - 168.0);
                self.rect([x, y], [width, 4.0], [0.54, 0.53, 0.53, 1.0]);
                let fraction = (elapsed / total).clamp(0.0, 1.0) as f32;
                if fraction > 0.0 {
                    self.rect([x, y], [width * fraction, 4.0], [0.96, 0.95, 0.93, 1.0]);
                }
                self.text(
                    &format!("{} / {}", timestamp(elapsed), timestamp(total)),
                    17,
                    [CENTER, 298.0],
                    true,
                );
            } else {
                self.text(&timestamp(elapsed), 17, [CENTER, 298.0], true);
            }
        }
        self.hints(&[("B", "back"), ("L1/R1", "skip"), ("A", toggle)]);
    }

    fn text(&mut self, text: &str, px: u32, [x, y]: [f32; 2], centred: bool) {
        let key = format!("{px}/{text}");
        let face = self.texture(key, |scale| music_text_face(text, px, ROOM, scale));
        let x = if centred {
            x - face.size[0] as f32 / 2.0
        } else {
            x
        };
        self.place(face, x, y);
    }

    fn hints(&mut self, hints: &[(&str, &str)]) {
        let faces: Vec<_> = hints
            .iter()
            .map(|&(key, label)| {
                let face = self.texture(format!("{key}/{label}"), |scale| {
                    let (image, width) = menu_hint_face_scaled(key, label, scale);
                    (image, [width, HINT_H])
                });
                (face.tex, face.size[0])
            })
            .collect();
        for (tex, width, x) in centred_hints(&faces, LEGEND_GAP) {
            self.place(
                Face {
                    tex,
                    size: [width, HINT_H],
                },
                x,
                LEGEND_Y,
            );
        }
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

fn timestamp(seconds: f64) -> String {
    let seconds = seconds as u64;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}
