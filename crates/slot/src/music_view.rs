use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use slot_gfx::{Compositor, Draw, TexId, OUT_H, OUT_W};
use slot_store::BRIGHTNESS_MAX;
use slot_ui::{
    menu_marquee_whole_face, menu_marquee_width, menu_text_face,
    pixel_aligned_texture, UndoFace, QUICK_PITCH,
    QUICK_VALUE_EDGE, STATUS_MARGIN,
};

use crate::music::{Music, RepeatMode, Status, Track, ALBUM_VISIBLE_ROWS};
use crate::music_cover::{MusicCover, LIST_THUMBNAIL, THUMBNAIL};

// Music alone uses a soft warm-ivory canvas (#F5F1EA), noticeably
// less stark than pure white. System menus and the launcher keep their
// original dark palette; never change global MENU_* constants.
const MUSIC_BACKGROUND: [f32; 4] = [245.0 / 255.0, 241.0 / 255.0, 234.0 / 255.0, 1.0];
// Reuse a single small bilinearly filtered GPU texture for a smooth diagonal
// backdrop. Regenerate it only if the album colour changes, never per frame.
const PLAYING_GRADIENT_W: u32 = 80;
const PLAYING_GRADIENT_H: u32 = 60;
const MUSIC_TEXT_INK: [u8; 3] = [0x24, 0x24, 0x24];
const MUSIC_MUTED_INK: [u8; 3] = [0x74, 0x6e, 0x6b];
const MUSIC_SELECTION_INK: [u8; 3] = [0xa0, 0x50, 0x69];
// Repeat icons match the black shelf HUD; disabled mode uses a neutral grey.
const REPEAT_ACTIVE_INK: [u8; 3] = [0, 0, 0];
const REPEAT_DISABLED_INK: [u8; 3] = [0x88, 0x88, 0x88];

fn repeat_icon_ink(mode: RepeatMode) -> [u8; 3] {
    if mode == RepeatMode::Off { REPEAT_DISABLED_INK } else { REPEAT_ACTIVE_INK }
}
const MUSIC_PROGRESS_TRACK: [f32; 4] = [0.84, 0.83, 0.82, 1.0];
const MUSIC_COVER_PLACEHOLDER: [f32; 4] = [0.92, 0.91, 0.90, 1.0];
const HUD_LOCK_SIDE: u32 = 18;
// A simple confirmation, not an animation: show the open icon briefly.
const UNLOCK_ICON_HOLD: Duration = Duration::from_millis(600);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum LockGlyph { Closed, Open }

#[derive(Default)]
struct LockFeedback {
    was_locked: bool,
    open_until: Option<Instant>,
}

impl LockFeedback {
    fn update(&mut self, locked: bool, now: Instant) -> Option<LockGlyph> {
        if locked {
            self.was_locked = true;
            self.open_until = None;
            return Some(LockGlyph::Closed);
        }
        if self.was_locked {
            self.open_until = Some(now + UNLOCK_ICON_HOLD);
        }
        self.was_locked = false;
        if self.open_until.is_some_and(|end| now < end) {
            Some(LockGlyph::Open)
        } else {
            self.open_until = None;
            None
        }
    }

    fn pending(&self) -> bool { self.open_until.is_some() }
}


// Match the shared top-left clock, rather than the 32px quick-menu inset.
const EDGE: f32 = STATUS_MARGIN;
// Nine 40px rows fill the usable 640x480 Music canvas; twenty-one
// off-thread 64px thumbnail cache slots keep ten nearby albums warm.
const ROWS: usize = 9;
const LIST_CACHE_RADIUS: usize = 10;
const LIST_CACHE_SLOTS: usize = LIST_CACHE_RADIUS * 2 + 1;
const LIST_TOP: f32 = 100.0;
const ROW_H: f32 = 40.0;
const ALBUM_COVER_SIDE: f32 = 34.0;
const ALBUM_TEXT_X: f32 = EDGE + ALBUM_COVER_SIDE + 12.0;
const LIST_SCROLL_X: f32 = OUT_W as f32 - EDGE + 8.0;
const LIST_SCROLL_TOP: f32 = LIST_TOP + 4.0;
const LIST_SCROLL_HEIGHT: f32 = ROWS as f32 * ROW_H - 8.0;
const TITLE_TOP: f32 = 42.0;
fn list_prefetch_range(cursor: usize, count: usize) -> std::ops::Range<usize> {
    cursor.saturating_sub(LIST_CACHE_RADIUS)
        ..cursor.saturating_add(LIST_CACHE_RADIUS + 1).min(count)
}

// Classic iPod-style album browser: full-width menu rows rather than a
// floating Cover Flow selection panel. Keep Slot's warm-white/rose palette.
const ALBUM_HEADER_BOTTOM: f32 = 108.0;
const ALBUM_LIST_TOP: f32 = 116.0;
const ALBUM_TRACK_ROW_H: f32 = 44.0;
// Match Slot's native two-column settings rhythm, on Music's ivory canvas.
const SETTINGS_FIRST_ROW_Y: f32 = 108.0;
const SETTINGS_ROW_STEP: f32 = QUICK_PITCH;
const SETTINGS_ROW_H: f32 = 44.0;
const SETTINGS_VALUE_X: f32 = QUICK_VALUE_EDGE;
const ALBUM_TRACK_TITLE_X: f32 = EDGE + 44.0;
// Smaller than the 22px Now Playing badge. Keep the mark inline, not in a column.
const TRACK_VIDEO_BADGE_SIDE: u32 = 18;
const TRACK_VIDEO_BADGE_GAP: u32 = 6;
fn track_title_room(has_badge: bool) -> u32 {
    let room = (OUT_W as f32 - ALBUM_TRACK_TITLE_X - EDGE - 12.0) as u32;
    if has_badge { room.saturating_sub(TRACK_VIDEO_BADGE_SIDE + TRACK_VIDEO_BADGE_GAP) }
    else { room }
}
// A gentle tonal step beneath the header, not a separate white panel.
const ALBUM_HEADER_BACKGROUND: [f32; 4] = [239.0 / 255.0, 234.0 / 255.0, 225.0 / 255.0, 1.0];
const ALBUM_ROW_SEPARATOR: [f32; 4] = [0.89, 0.88, 0.87, 1.0];
const ALBUM_WHITE_INK: [u8; 3] = [255, 255, 255];
// ClassiPod-inspired composition, retaining Slot's native 640x480 layout,
// typography, embedded artwork decoder and physical controls.
// Fill the available artwork region while retaining a clean text column.
const PLAYING_COVER_SIDE: f32 = 250.0;
const PLAYING_COVER_X: f32 = EDGE;
const PLAYING_COVER_Y: f32 = 112.0;
// Small monochrome camera-roll badge over the upper-left edge of the cover.
const VIDEO_BADGE_SIDE: u32 = 22;
const VIDEO_BADGE_INSET: f32 = 6.0;
const PLAYING_INFO_X: f32 = EDGE + PLAYING_COVER_SIDE + 22.0;
const PLAYING_INFO_W: u32 = (OUT_W as f32 - PLAYING_INFO_X - EDGE - 4.0) as u32;
// Progress/timestamps occupy the bottom line, with Repeat next to end time.
const PLAYING_PROGRESS_Y: f32 = 414.0;
const PLAYING_TIME_Y: f32 = 432.0;
// Match the scale of the 18px Wi-Fi and 22px battery HUD symbols.
const REPEAT_ICON_SIDE: u32 = 20;
const REPEAT_BADGE_X: f32 = OUT_W as f32 - EDGE - REPEAT_ICON_SIDE as f32;
const REPEAT_BADGE_Y: f32 = PLAYING_TIME_Y + 2.0;
const PLAYING_END_TIME_RIGHT: f32 = REPEAT_BADGE_X - 12.0;
// UXWing play/pause state at the beginning of the elapsed-time row.
const PLAYBACK_ICON_SIDE: u32 = 20;
const PLAYBACK_ICON_X: f32 = EDGE;
const PLAYBACK_ICON_Y: f32 = PLAYING_TIME_Y + 2.0;
const PLAYING_START_TIME_X: f32 = PLAYBACK_ICON_X + PLAYBACK_ICON_SIDE as f32 + 10.0;
// Scroll only titles wider than the available information column.
const TITLE_MARQUEE_PX: u32 = 26;
const TITLE_MARQUEE_START_PAUSE: f32 = 1.3;
const TITLE_MARQUEE_END_PAUSE: f32 = 0.85;
const TITLE_MARQUEE_FORWARD_PX_PER_SEC: f32 = 38.0;
const TITLE_MARQUEE_BACKWARD_PX_PER_SEC: f32 = 40.0;
// Downloaded from UXWing at install time and kept in Slot/System for offline play.
const REPEAT_ICON_FILES: [&str; 3] =
    ["repeat-off.png", "repeat-all.png", "repeat-one.png"];
const PLAYBACK_ICON_FILES: [&str; 2] = ["play.png", "pause.png"];
// UXWing: https://uxwing.com/camera-roll-icon/
const VIDEO_BADGE_FILE: &str = "camera-roll.png";

/// A video badge means a non-empty local companion video with the same
/// directory and filename stem as the playing MP3/M4A. It does not search
/// YouTube or suggest that the current Slot build can decode the video.
fn has_companion_video(audio_path: &Path) -> bool {
    crate::music::companion_video(audio_path).is_some()
}

/// Avoid SD metadata reads during title-marquee frames or repeat updates.
#[derive(Default)]
struct VideoBadgeCache {
    audio_path: Option<PathBuf>,
    available: bool,
}

impl VideoBadgeCache {
    fn available_for(&mut self, audio_path: &Path) -> bool {
        if self.audio_path.as_deref() != Some(audio_path) {
            self.available = has_companion_video(audio_path);
            self.audio_path = Some(audio_path.to_path_buf());
        }
        self.available
    }
}


fn playback_icon_index(status: Status) -> Option<usize> {
    match status {
        Status::Playing => Some(0),
        Status::Paused => Some(1),
        _ => None,
    }
}
// Seven 3D covers are drawn; four extra on each side are decoded ahead
// of time. A 15-slot rolling GPU cache costs at most ~3.75 MiB in
// 256x256 RGBA textures, without keeping an entire music library in RAM.
const FLOW_SLOTS: usize = 7;
const FLOW_EXTRA_COVERS: usize = 4;
const FLOW_CACHE_SLOTS: usize = FLOW_SLOTS + 2 * FLOW_EXTRA_COVERS;
const FLOW_CACHE_RADIUS: usize = FLOW_SLOTS / 2 + FLOW_EXTRA_COVERS;

fn flow_prefetch_range(cursor: usize, albums: usize) -> std::ops::Range<usize> {
    cursor.saturating_sub(FLOW_CACHE_RADIUS)
        ..cursor.saturating_add(FLOW_CACHE_RADIUS + 1).min(albums)
}
// ClassiPod Original: 400 ms ease-out-sine, a 0.95 radian Y rotation,
// 22% side scale reduction, and perspective coefficient 0.35 / cover size.
const FLOW_DURATION: Duration = Duration::from_millis(400);
const FLOW_COVER_Y: f32 = 100.0;
const FLOW_FRONT_SIDE: f32 = 218.0;
// With reflections removed, bring the album labels closer to the covers.
const FLOW_ALBUM_TITLE_Y: f32 = 365.0;
const FLOW_ALBUM_ARTIST_Y: f32 = 402.0;
const FLOW_YAW: f32 = 0.95;
const FLOW_PERSPECTIVE: f32 = 0.35 / FLOW_FRONT_SIDE;

/// Navigation state stays in Music; only transient visual interpolation lives
/// here. The renderer requests frames only during the 400ms movement.
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
        let eased = (t * std::f32::consts::FRAC_PI_2).sin();
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
            if (target - current).abs() > 3.0 {
                // An alphabet jump can skip dozens of covers. Snap straight
                // to the selected group rather than spending 400ms decoding
                // and uploading every intermediate carousel window.
                self.travel = None;
            } else {
                self.travel = Some((now, current, target));
            }
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

/// Crop a viewport from a cached full-line RGBA image; never rasterize a
/// glyph in this per-frame path. Clamps requests at each edge.
fn crop_marquee_rgba(whole: &UndoFace, width: u32, at: u32) -> Vec<u8> {
    let mut pixels = vec![0u8; (width * whole.h * 4) as usize];
    let offset = at.min(whole.w.saturating_sub(width));
    let bytes = width.min(whole.w.saturating_sub(offset)) as usize * 4;
    for y in 0..whole.h {
        let from = (y * whole.w + offset) as usize * 4;
        let to = (y * width) as usize * 4;
        pixels[to..to + bytes].copy_from_slice(&whole.rgba[from..from + bytes]);
    }
    pixels
}

#[derive(Default)]
struct TitleMarquee {
    song: Option<PathBuf>,
    title: Option<String>,
    started: Option<Instant>,
    overflowing: bool,
    overflow: f32,
    room: u32,
    // Rasterize the entire line once, then crop from these RGBA pixels.
    // Reuse a single GPU texture for the narrow visible viewport.
    whole: Option<UndoFace>,
    viewport: Option<TexId>,
    last_at: Option<u32>,
    scale: Option<[f32; 2]>,
}

impl TitleMarquee {
    /// A small pause at each end, then a smooth left/right pass. Returning
    /// None keeps the original static title path for titles that already fit.
    fn offset(&mut self, path: &Path, title: &str, room: u32, now: Instant) -> Option<f32> {
        if self.song.as_deref() != Some(path) || self.title.as_deref() != Some(title)
            || self.room != room {
            self.song = Some(path.to_path_buf());
            self.title = Some(title.to_owned());
            self.started = Some(now);
            self.whole = None;
            self.last_at = None;
            self.room = room;
            // Measuring glyph widths is also unnecessary every 33ms.
            self.overflow = (menu_marquee_width(title, TITLE_MARQUEE_PX) - room as f32).max(0.0);
        }
        let overflow = self.overflow;
        self.overflowing = overflow > 1.0;
        if !self.overflowing { return None; }
        let forward = overflow / TITLE_MARQUEE_FORWARD_PX_PER_SEC;
        let backward = overflow / TITLE_MARQUEE_BACKWARD_PX_PER_SEC;
        let cycle = TITLE_MARQUEE_START_PAUSE + forward
            + TITLE_MARQUEE_END_PAUSE + backward;
        let t = now.saturating_duration_since(self.started.unwrap_or(now))
            .as_secs_f32() % cycle;
        let t = t - TITLE_MARQUEE_START_PAUSE;
        if t <= 0.0 { return Some(0.0); }
        if t < forward { return Some(t * TITLE_MARQUEE_FORWARD_PX_PER_SEC); }
        let t = t - forward - TITLE_MARQUEE_END_PAUSE;
        if t <= 0.0 { return Some(overflow); }
        Some((overflow - t * TITLE_MARQUEE_BACKWARD_PX_PER_SEC).max(0.0))
    }

    /// Crop a pre-rendered title at the current offset. On later frames this
    /// is only a compact row-by-row RGBA copy and one texture update.
    fn cropped_face(
        &mut self,
        title: &str,
        offset: f32,
        room: u32,
        scale: [f32; 2],
        compositor: &mut Compositor,
    ) -> Face {
        if self.whole.is_none() || self.scale != Some(scale) {
            self.whole = Some(menu_marquee_whole_face(
                title, TITLE_MARQUEE_PX, scale, MUSIC_SELECTION_INK,
            ));
            self.scale = Some(scale);
            self.last_at = None;
        }
        let whole = self.whole.as_ref().expect("rasterized title");
        let w = (room as f32 * scale[0]).round().max(1.0) as u32;
        let h = whole.h;
        let max_at = whole.w.saturating_sub(w);
        let at = ((offset.max(0.0) * scale[0]).round() as u32).min(max_at);
        if self.viewport.is_none() || self.last_at != Some(at) {
            let rgba = crop_marquee_rgba(whole, w, at);
            if let Some(texture) = self.viewport {
                compositor.update_texture(texture, w, h, &rgba);
            } else {
                self.viewport = Some(compositor.create_texture(w, h, &rgba));
            }
            self.last_at = Some(at);
        }
        Face {
            tex: self.viewport.expect("marquee viewport texture"),
            size: [room, (TITLE_MARQUEE_PX as f32 * 1.4).ceil() as u32],
        }
    }

    fn reset(&mut self) {
        self.song = None;
        self.title = None;
        self.started = None;
        self.overflowing = false;
        self.overflow = 0.0;
        self.room = 0;
        self.whole = None;
        self.last_at = None;
    }
}

struct PlayingBackdrop {
    texture: TexId,
    accent: [u8; 3],
}

pub(crate) struct MusicView {
    cache: Vec<CachedFace>,
    cover: MusicCover,
    album_covers: [MusicCover; LIST_CACHE_SLOTS],
    list_visible: bool,
    flow_covers: [MusicCover; FLOW_CACHE_SLOTS],
    flow_motion: FlowMotion,
    flow_placeholder: Option<TexId>,
    playing_backdrop: Option<PlayingBackdrop>,
    repeat_icons: [MusicIcon; 3],
    title_marquee: TitleMarquee,
    lock_icon: MusicIcon,
    unlock_icon: MusicIcon,
    lock_feedback: LockFeedback,
    lock_glyph: Option<LockGlyph>,
    playback_icons: [MusicIcon; 2],
    video_cover_icon: MusicIcon,
    video_badge: VideoBadgeCache,
    track_video_badges: [VideoBadgeCache; ALBUM_VISIBLE_ROWS],
    track_video_icons: [MusicIcon; 2],
}

impl Default for MusicView {
    fn default() -> Self {
        Self {
            cache: Vec::new(),
            cover: MusicCover::default(),
            album_covers: std::array::from_fn(|_| MusicCover::list_thumbnail()),
            list_visible: false,
            flow_covers: std::array::from_fn(|_| MusicCover::default()),
            flow_motion: FlowMotion::default(),
            flow_placeholder: None,
            playing_backdrop: None,
            repeat_icons: std::array::from_fn(|_| MusicIcon::default()),
            title_marquee: TitleMarquee::default(),
            lock_icon: MusicIcon::default(),
            unlock_icon: MusicIcon::default(),
            lock_feedback: LockFeedback::default(),
            lock_glyph: None,
            playback_icons: std::array::from_fn(|_| MusicIcon::default()),
            video_cover_icon: MusicIcon::default(),
            video_badge: VideoBadgeCache::default(),
            track_video_badges: std::array::from_fn(|_| VideoBadgeCache::default()),
            track_video_icons: std::array::from_fn(|_| MusicIcon::default()),
        }
    }
}

#[derive(Default)]
struct MusicIcon {
    size: [u32; 2],
    texture: Option<TexId>,
}

impl MusicIcon {
    fn load(
        &mut self, music: &Music, file: &str, side: u32, scale: [f32; 2],
        ink: Option<[u8; 3]>, compositor: &mut Compositor,
    ) -> Option<TexId> {
        self.load_with(music, file, side, scale, compositor,
            |bytes, size| decode_music_icon(bytes, size, ink))
    }

    fn load_with(
        &mut self, music: &Music, file: &str, side: u32, scale: [f32; 2],
        compositor: &mut Compositor,
        decode: impl FnOnce(&[u8], [u32; 2]) -> Option<Vec<u8>>,
    ) -> Option<TexId> {
        // Match pixel_aligned_texture so one sampled texel covers one
        // compositor pixel, even at the Brick Pro's 1.6× scale.
        let size = music_icon_size(side, scale);
        if self.size != size {
            self.size = size;
            let pixels = music.icon_root()
                .and_then(|root| std::fs::read(root.join("System/music-icons").join(file)).ok())
                .filter(|bytes| bytes.len() <= 4 << 20)
                .and_then(|bytes| decode(&bytes, size));
            let [w, h] = size;
            self.texture = pixels.map(|pixels| match self.texture {
                Some(id) => {
                    compositor.update_texture(id, w, h, &pixels);
                    id
                }
                None => compositor.create_texture_nearest(w, h, &pixels),
            });
        }
        self.texture
    }
}

impl MusicView {
    fn repeat_icon(&mut self, music: &Music, scale: [f32; 2], compositor: &mut Compositor) -> Option<TexId> {
        let mode = music.repeat_mode();
        let index = match mode {
            RepeatMode::Off => 0,
            RepeatMode::All => 1,
            RepeatMode::One => 2,
        };
        self.repeat_icons[index].load(
            music, REPEAT_ICON_FILES[index], REPEAT_ICON_SIDE, scale,
            Some(repeat_icon_ink(mode)), compositor,
        )
    }

    pub(crate) fn hud_lock_icon(&self) -> Option<(TexId, u32, u32)> {
        let id = match self.lock_glyph {
            Some(LockGlyph::Closed) => self.lock_icon.texture,
            Some(LockGlyph::Open) => self.unlock_icon.texture,
            None => None,
        }?;
        Some((id, HUD_LOCK_SIDE, HUD_LOCK_SIDE))
    }

    /// Refresh only until the open icon disappears, then return to idle redraw.
    pub(crate) fn unlock_feedback_pending(&self) -> bool {
        self.lock_feedback.pending()
    }

    /// Only long titles need the additional low-frequency redraws.
    pub(crate) fn title_marquee_active(&self) -> bool {
        self.title_marquee.overflowing
    }

    pub(crate) fn cover_flow_animating(&self) -> bool {
        self.flow_motion.active(Instant::now())
    }

    /// List thumbnails refresh only while the Albums list is visible and
    /// has unfinished workers; idle Music still consumes no extra frames.
    pub(crate) fn album_list_loading(&self) -> bool {
        self.list_visible && self.album_covers.iter().any(MusicCover::is_loading)
    }

    /// Poll pending cover workers promptly while the carousel is on screen.
    /// When all requests settle, return to the usual low-CPU idle redraw gate.
    pub(crate) fn cover_flow_loading(&self) -> bool {
        self.flow_motion.position.is_some()
            && self.flow_covers.iter().any(MusicCover::is_loading)
    }

    /// Both browsing modes retain their own small GPU caches. Returning the
    /// original texture ID avoids decoding/uploading an image that is already
    /// on screen. The caches themselves own and reuse those texture slots.
    fn ready_album_art(&self, first_track: &Path) -> Option<(TexId, [u32; 2])> {
        self.flow_covers.iter().find_map(|cached| cached.ready_for(first_track))
    }

    fn ready_album_tint(&self, first_track: &Path) -> Option<[u8; 3]> {
        self.flow_covers.iter().find_map(|cached| cached.tint_for(first_track))
    }

    /// One texture and no additional artwork reads. Its pixels are computed
    /// only when the sampled album colour changes; GL performs the scaling
    /// and interpolation in a single background draw.
    fn playing_background_texture(
        &mut self,
        accent: Option<[u8; 3]>,
        compositor: &mut Compositor,
    ) -> Option<TexId> {
        let accent = accent?;
        if let Some(backdrop) = &self.playing_backdrop {
            if backdrop.accent == accent {
                return Some(backdrop.texture);
            }
        }
        let pixels = playing_gradient_rgba(accent);
        let texture = if let Some(backdrop) = &mut self.playing_backdrop {
            compositor.update_texture(
                backdrop.texture, PLAYING_GRADIENT_W, PLAYING_GRADIENT_H, &pixels,
            );
            backdrop.accent = accent;
            backdrop.texture
        } else {
            let texture = compositor.create_texture(
                PLAYING_GRADIENT_W, PLAYING_GRADIENT_H, &pixels,
            );
            self.playing_backdrop = Some(PlayingBackdrop { texture, accent });
            texture
        };
        Some(texture)
    }

    pub(crate) fn render(
        &mut self,
        music: &Music,
        compositor: &mut Compositor,
        scale: [f32; 2],
    ) -> Vec<Draw> {
        // Both the artwork and its extracted hue are keyed to the same
        // album request. Colour extraction ran in the background decoder,
        // not on the render thread, and the existing GPU cover is reused.
        let (cover, accent) = if let Some(track) = music.track.as_ref() {
            let album = music.playing_album();
            let first_path = album.songs.first().map(|song| song.path.as_path());
            let ready = first_path.and_then(|path| self.ready_album_art(path));
            if let Some(art) = ready {
                (Some(art), first_path.and_then(|path| self.ready_album_tint(path)))
            } else {
                // A fast press before Cover Flow finishes decoding must use
                // that same in-flight worker, including its sampled colour.
                let pending = first_path.and_then(|path| {
                    self.flow_covers.iter_mut()
                        .find(|cached| cached.requested_for(path))
                        .map(|cached| {
                            let art = cached.texture(path, album, compositor);
                            (art, cached.tint_for(path))
                        })
                });
                pending.unwrap_or_else(|| {
                    let path = &music.song(track).path;
                    let art = self.cover.texture(path, album, compositor);
                    (art, self.cover.tint_for(path))
                })
            }
        } else {
            (None, None)
        };
        // A small thumbnail cache is independent of the seven visible
        // 256px Cover Flow cards. Nine 40px rows are drawn while up to
        // ten more albums in either direction are decoded in background.
        let first = music.cursor().saturating_sub(ROWS - 1);
        let mut album_art = [None; ROWS];
        self.list_visible = !music.showing_now_playing()
            && !music.music_settings_open() && !music.cover_flow()
            && (0..music.row_count()).any(|i| music.album_for_row(i).is_some());
        if self.list_visible {
            let cache_root = music.icon_root();
            // Limit total list-thumbnail decoders to three at a time.
            // A 21-slot window is a cache, not 21 concurrent JPEG readers.
            let mut pending = self.album_covers.iter().filter(|cover| cover.is_loading()).count();
            for (slot, art) in album_art.iter_mut().enumerate() {
                let index = first + slot;
                if let Some(album) = music.album_for_row(index) {
                    if let Some(song) = album.songs.first() {
                        let cover = &mut self.album_covers[index % LIST_CACHE_SLOTS];
                        if pending >= 3 && !cover.requested_for(&song.path) {
                            continue;
                        }
                        let was_loading = cover.is_loading();
                        *art = cover.texture_cached(&song.path, album, compositor, cache_root);
                        if !was_loading && cover.is_loading() { pending += 1; }
                        if was_loading && !cover.is_loading() { pending = pending.saturating_sub(1); }
                    }
                }
            }
            for index in list_prefetch_range(music.cursor(), music.row_count()) {
                if (first..first + ROWS).contains(&index) { continue; }
                if let Some(album) = music.album_for_row(index) {
                    if let Some(song) = album.songs.first() {
                        let cover = &mut self.album_covers[index % LIST_CACHE_SLOTS];
                        if pending >= 3 && !cover.requested_for(&song.path) {
                            continue;
                        }
                        let was_loading = cover.is_loading();
                        let _ = cover.texture_cached(&song.path, album, compositor, cache_root);
                        if !was_loading && cover.is_loading() { pending += 1; }
                        if was_loading && !cover.is_loading() { pending = pending.saturating_sub(1); }
                    }
                }
            }
        }
        // A separate bounded cache for Cover Flow avoids synchronous decoding
        // and never evicts the list's existing thumbnail textures.
        let now = Instant::now();
        let flow = if music.cover_flow() && music.row_count() > 0 {
            Some(self.flow_motion.to(music.cursor(), now))
        } else {
            self.flow_motion.clear();
            None
        };
        let flow_first = flow.map_or(0, |position| position.round() as isize - (FLOW_SLOTS / 2) as isize);
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
                        // Draw the seven visible covers first. Extra cache
                        // requests must not delay artwork in the viewport.
                        *item = self.flow_covers[album_index % FLOW_CACHE_SLOTS]
                            .texture(&song.path, album, compositor);
                    }
                }
            }
        }
        // Keep another four covers warm on each side of the visible seven.
        // These use the same 15 rolling texture handles, so moving into the
        // prefetched window requires no new decode or placeholder flash.
        // Skip visible indices: they were already requested above.
        if flow.is_some() {
            for album_index in flow_prefetch_range(music.cursor(), music.row_count()) {
                let visible = (flow_first..flow_first + FLOW_SLOTS as isize)
                    .contains(&(album_index as isize));
                if visible {
                    continue;
                }
                if let Some(album) = music.album_for_row(album_index) {
                    if let Some(song) = album.songs.first() {
                        let _ = self.flow_covers[album_index % FLOW_CACHE_SLOTS]
                            .texture(&song.path, album, compositor);
                    }
                }
            }
        }
        let flow_placeholder = if flow.is_some() {
            Some(*self.flow_placeholder.get_or_insert_with(|| {
                compositor.create_texture(1, 1, &[232, 230, 227, 255])
            }))
        } else {
            None
        };
        let backdrop = if music.showing_now_playing() && music.track.is_some()
            && !music.music_settings_open() {
            self.playing_background_texture(accent, compositor)
        } else {
            None
        };
        let repeat_icon = if music.showing_now_playing()
            && music.track.is_some() && !music.music_settings_open() {
            self.repeat_icon(music, scale, compositor)
        } else {
            None
        };
        let playback_icon = if music.showing_now_playing() && !music.music_settings_open() {
            music.track.as_ref()
                .and_then(|track| playback_icon_index(track.status))
                .and_then(|index| self.playback_icons[index].load(
                    music, PLAYBACK_ICON_FILES[index], PLAYBACK_ICON_SIDE, scale, Some([0; 3]), compositor,
                ))
        } else {
            None
        };
        let video_badge_icon = if music.showing_now_playing() && !music.music_settings_open() {
            if let Some(track) = music.track.as_ref() {
                if self.video_badge.available_for(&music.song(track).path) {
                    self.video_cover_icon.load(
                        music, VIDEO_BADGE_FILE, VIDEO_BADGE_SIDE, scale, Some([0; 3]), compositor,
                    )
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };
        // Cache only the seven visible paths. Recheck on entering the album
        // or paging to a different set, not on every repaint.
        let mut track_badges = [false; ALBUM_VISIBLE_ROWS];
        let mut track_icons = [None; 2];
        if !music.music_settings_open() && !music.showing_now_playing() && music.showing_tracks() {
            let album = music.browsed_album();
            let (first, end) = music.album_track_window();
            for (slot, index) in (first..end).enumerate() {
                track_badges[slot] = self.track_video_badges[slot]
                    .available_for(&album.songs[index].path);
            }
            if track_badges.iter().any(|&available| available) {
                for (index, icon) in self.track_video_icons.iter_mut().enumerate() {
                    let ink = if index == 0 { [0; 3] } else { ALBUM_WHITE_INK };
                    track_icons[index] = icon.load_with(
                        music, VIDEO_BADGE_FILE, TRACK_VIDEO_BADGE_SIDE, scale, compositor,
                        |bytes, size| decode_track_video_icon(bytes, size, ink),
                    );
                }
            }
        } else {
            self.track_video_badges = std::array::from_fn(|_| VideoBadgeCache::default());
        }
        let title_offset = if music.showing_now_playing() && !music.music_settings_open() {
            if let Some(track) = music.track.as_ref() {
                let song = music.song(track);
                self.title_marquee.offset(
                    &song.path, &song.title, PLAYING_INFO_W, Instant::now(),
                )
            } else {
                self.title_marquee.reset();
                None
            }
        } else {
            self.title_marquee.reset();
            None
        };
        self.lock_glyph = self.lock_feedback.update(music.locked(), Instant::now());
        match self.lock_glyph {
            Some(LockGlyph::Closed) => {
                self.lock_icon.load(music, "lock.png", HUD_LOCK_SIDE, scale, None, compositor);
            }
            Some(LockGlyph::Open) => {
                self.unlock_icon.load(music, "unlock.png", HUD_LOCK_SIDE, scale, None, compositor);
            }
            None => {}
        }
        // Build the viewport before borrowing the text painter. No glyph
        // rasterization occurs on subsequent frames of the same song.
        let title_face = title_offset.map(|offset| {
            let song = music.song(music.track.as_ref().expect("playing title"));
            self.title_marquee.cropped_face(
                &song.title, offset, PLAYING_INFO_W, scale, compositor,
            )
        });
        let mut p = Painter {
            cache: &mut self.cache,
            index: 0,
            compositor,
            scale,
            draws: Vec::new(),
        };
        if let Some(texture) = backdrop {
            p.draws.push(Draw::Tex {
                x: 0.0, y: 0.0,
                w: OUT_W as f32, h: OUT_H as f32,
                tex: texture, alpha: 1.0,
            });
        } else {
            p.rect([0.0, 0.0], [OUT_W as f32, OUT_H as f32], MUSIC_BACKGROUND);
        }
        if music.music_settings_open() {
            p.settings(music);
        } else if music.showing_now_playing() {
            if let Some(track) = &music.track {
                p.playing(music, track, cover, repeat_icon, playback_icon, video_badge_icon, title_face);
            } else {
                p.empty_now_playing();
            }
        } else if music.showing_tracks() {
            p.album_tracks(music, &track_badges, track_icons);
        } else if let (Some(position), Some(placeholder)) = (flow, flow_placeholder) {
            p.cover_flow(music, &flow_art, flow_first, position, placeholder);
        } else {
            p.browser(music, &album_art);
        }
        p.draws
    }
}

fn music_icon_size(side: u32, scale: [f32; 2]) -> [u32; 2] {
    scale.map(|factor| ((side as f32 * factor).round() as u32).max(1))
}

/// Keep the original artwork or convert a monochrome glyph into an ink mask.
fn decode_music_icon(png: &[u8], [width, height]: [u32; 2], ink: Option<[u8; 3]>) -> Option<Vec<u8>> {
    let (w, h) = crate::cart_art::png_size(png)?;
    if w == 0 || h == 0 || w > 1024 || h > 1024 { return None; }
    let (rgba, w, h) = crate::cart_art::decode_rgba(png)?;
    let mut pixels = slot_ui::cover_rgba(&rgba, w, h, width, height)?;
    if let Some(ink) = ink {
        for px in pixels.chunks_exact_mut(4) {
            let darkness = 255 - (px[..3].iter().map(|&channel| u16::from(channel)).sum::<u16>() / 3);
            px[3] = ((u16::from(px[3]) * darkness) / 255) as u8;
            px[..3].copy_from_slice(&ink);
        }
    }
    Some(pixels)
}

// Recolour the original monochrome UXWing camera roll before resizing.
// Preserve transparent holes and antialiased edges; the nearest-filtered
// texture is sized to the compositor's actual pixel density.
fn decode_track_video_icon(png: &[u8], [width, height]: [u32; 2], ink: [u8; 3]) -> Option<Vec<u8>> {
    let (w, h) = crate::cart_art::png_size(png)?;
    if w == 0 || h == 0 || w > 1024 || h > 1024 { return None; }
    let (mut rgba, w, h) = crate::cart_art::decode_rgba(png)?;
    for px in rgba.chunks_exact_mut(4) {
        let darkness = 255 - u16::from(*px[..3].iter().min()?);
        px[3] = ((u16::from(px[3]) * darkness) / 255) as u8;
        px[..3].copy_from_slice(&ink);
    }
    slot_ui::cover_rgba(&rgba, w, h, width, height)
}

struct Painter<'a> {
    cache: &'a mut Vec<CachedFace>,
    index: usize,
    compositor: &'a mut Compositor,
    scale: [f32; 2],
    draws: Vec<Draw>,
}

impl Painter<'_> {
    /// A classic iPod-style album song list, not the Cover Flow album flip.
    /// Entering an album from Covers, Albums or Artists opens the same screen.
    /// Keep the content full-width with only a quiet title bar and one
    /// highlighted song, retaining the physical controls in Music::input.
    fn album_tracks(&mut self, music: &Music,
                    badges: &[bool; ALBUM_VISIBLE_ROWS], icons: [Option<TexId>; 2]) {
        let album = music.browsed_album();
        let total = music.row_count();
        let cursor = music.cursor();
        let (first, end) = music.album_track_window();

        // Edge-to-edge header, rather than a framed panel. The same warm
        // background is used for every album, regardless of cover artwork.
        self.rect(
            [0.0, 0.0],
            [OUT_W as f32, ALBUM_HEADER_BOTTOM],
            ALBUM_HEADER_BACKGROUND,
        );
        // Keep both lines clear of the shared shelf status bar at y=12.
        self.text_center(&album.title, 27, 42.0, MUSIC_TEXT_INK, 568);
        self.text_center(&album.artist, 18, 78.0, MUSIC_MUTED_INK, 568);
        self.rect(
            [0.0, ALBUM_HEADER_BOTTOM - 1.0],
            [OUT_W as f32, 1.0],
            ALBUM_ROW_SEPARATOR,
        );

        if total == 0 {
            self.text_center("no tracks", 21, 221.0, MUSIC_MUTED_INK, 400);
            return;
        }

        // The iPod scrolls one continuous list. Track numbers belong before
        // titles, not in a metadata column suggesting unknown durations.
        for index in first..end {
            let y = album_row_y(index, first);
            let selected = index == cursor;
            if selected {
                let rose = MUSIC_SELECTION_INK.map(|n| n as f32 / 255.0);
                self.rect([0.0, y], [OUT_W as f32, ALBUM_TRACK_ROW_H],
                    [rose[0], rose[1], rose[2], 1.0]);
            } else {
                self.rect(
                    [EDGE, y + ALBUM_TRACK_ROW_H - 1.0],
                    [OUT_W as f32 - 2.0 * EDGE, 1.0],
                    ALBUM_ROW_SEPARATOR,
                );
            }

            let ink = if selected { ALBUM_WHITE_INK } else { MUSIC_TEXT_INK };
            let number_ink = if selected { ALBUM_WHITE_INK } else { MUSIC_MUTED_INK };
            let song = &album.songs[index];
            let number = song.track.map(usize::from).unwrap_or(index + 1);
            self.text_with_room(
                &format!("{number:02}"), 17,
                [EDGE, y + 11.0], number_ink, 40,
            );
            let badge = if badges[index - first] { icons[usize::from(selected)] } else { None };
            let room = track_title_room(badge.is_some());
            let width = self.text_with_room(
                &song.title, 22,
                [ALBUM_TRACK_TITLE_X, y + 7.0], ink, room,
            );
            if let Some(tex) = badge {
                let side = TRACK_VIDEO_BADGE_SIDE as f32;
                self.draws.push(pixel_aligned_texture(
                    tex,
                    [ALBUM_TRACK_TITLE_X + width.min(room) as f32 + TRACK_VIDEO_BADGE_GAP as f32,
                     y + (ALBUM_TRACK_ROW_H - side) / 2.0],
                    [side, side], self.scale, 1.0,
                ));
            }
        }

        // Very slim scroll indicator on the outer edge, only for albums
        // longer than the seven visible rows. It doesn't shrink text width.
        if total > ALBUM_VISIBLE_ROWS {
            let rail_y = ALBUM_LIST_TOP + 4.0;
            let rail_h = ALBUM_VISIBLE_ROWS as f32 * ALBUM_TRACK_ROW_H - 8.0;
            let rail_x = OUT_W as f32 - 5.0;
            self.rect([rail_x, rail_y], [2.0, rail_h],
                [0.84, 0.83, 0.82, 1.0]);
            let thumb_h = (rail_h * ALBUM_VISIBLE_ROWS as f32 / total as f32)
                .max(16.0);
            let progress = first as f32 / (total - ALBUM_VISIBLE_ROWS) as f32;
            self.rect(
                [rail_x, rail_y + progress * (rail_h - thumb_h)],
                [2.0, thumb_h], [0.62, 0.36, 0.45, 1.0],
            );
        }
        self.text_center(
            &format!("{} of {total}", cursor + 1),
            15, 453.0, MUSIC_MUTED_INK, 152,
        );
    }

    fn browser(&mut self, music: &Music, album_art: &[Option<(TexId, [u32; 2])>; ROWS]) {
        if music.is_root() {
            self.music_tabs(if music.viewing_artists() { 2 } else { 1 });
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
                    let size = pixels.map(|n| n as f32 * ALBUM_COVER_SIDE / LIST_THUMBNAIL as f32);
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
            let room = (LIST_SCROLL_X - x - 18.0).max(1.0) as u32;
            if let Some(subtitle) = subtitle {
                self.text_with_room(label, 21, [x, y], ink, room);
                self.text_with_room(subtitle, 13, [x, y + 23.0], MUSIC_MUTED_INK, room);
            } else {
                self.text_with_room(label, 22, [x, y + 6.0], ink, room);
            }
        }
        if music.row_count() > ROWS {
            let total = music.row_count();
            let rail = [0.88, 0.86, 0.84, 1.0];
            let thumb = [0.62, 0.32, 0.42, 1.0];
            self.rect([LIST_SCROLL_X, LIST_SCROLL_TOP], [2.0, LIST_SCROLL_HEIGHT], rail);
            let thumb_h = (LIST_SCROLL_HEIGHT * ROWS as f32 / total as f32).max(16.0);
            let progress = first as f32 / (total - ROWS) as f32;
            self.rect(
                [LIST_SCROLL_X, LIST_SCROLL_TOP + progress * (LIST_SCROLL_HEIGHT - thumb_h)],
                [2.0, thumb_h], thumb,
            );
        }
    }

    /// Native translation of ClassiPod's *Original* Cover Flow: the offset
    /// function, scale and 0.95-radian Y rotation follow the Flutter reference,
    /// but the GPU vertex shader performs perspective-correct texturing in Slot.
    /// Far covers are submitted first so the selected art remains in front.
    fn cover_flow(
        &mut self,
        music: &Music,
        art: &[Option<(TexId, [u32; 2])>; FLOW_SLOTS],
        first: isize,
        position: f32,
        placeholder: TexId,
    ) {
        // Match the Artists tab: selected navigation labels are charcoal,
        // while inactive labels stay muted. Rose is for row selection.
        self.music_tabs(1);

        let mut order = [0usize, 1, 2, 3, 4, 5, 6];
        order.sort_by(|&a, &b| {
            let da = (first as f32 + a as f32 - position).abs();
            let db = (first as f32 + b as f32 - position).abs();
            db.total_cmp(&da)
        });
        for slot in order {
            let index = first + slot as isize;
            if index < 0 || index as usize >= music.row_count() {
                continue;
            }
            let distance = index as f32 - position;
            let Some((x, y, size, alpha, yaw)) = flow_geometry(distance) else {
                continue;
            };
            // Keep the no-art tile in the same 3D plane as loaded artwork.
            // Missing art never leaves a flat square floating behind the rack.
            let (tex, pixels) = art[slot].unwrap_or((placeholder, [THUMBNAIL, THUMBNAIL]));
            let ratio = pixels.map(|n| n as f32 / THUMBNAIL as f32);
            let w = size * ratio[0];
            let h = size * ratio[1];
            let px = x + (size - w) * 0.5;
            let py = y + (size - h) * 0.5;
            self.draws.push(Draw::CoverFlow {
                x: px, y: py, w, h, tex, alpha,
                yaw, perspective: FLOW_PERSPECTIVE,
            });
        }

        let (album, artist) = music.row(music.cursor());
        self.text_center(album, 26, FLOW_ALBUM_TITLE_Y, MUSIC_TEXT_INK, 544);
        if let Some(artist) = artist {
            self.text_center(artist, 18, FLOW_ALBUM_ARTIST_Y, MUSIC_MUTED_INK, 544);
        }
    }

    fn text_center(&mut self, text: &str, px: u32, y: f32, ink: [u8; 3], room: u32) {
        let key = format!("{px}/{room}/{ink:?}/{text}");
        let face = self.texture(key, |scale| menu_text_face(text, px, room, scale, ink));
        self.place(face, (OUT_W as f32 - face.size[0] as f32) / 2.0, y);
    }

    /// Three distinct UXWing glyphs are loaded as GPU textures and rendered
    /// beside the end timestamp. The native fallback is used if assets
    /// have not been installed yet (for example, a pre-upgrade SD card).
    fn repeat_badge(&mut self, mode: RepeatMode, icon: Option<TexId>) {
        let (x, y) = (REPEAT_BADGE_X, REPEAT_BADGE_Y);
        if let Some(tex) = icon {
            self.draws.push(pixel_aligned_texture(
                tex, [x, y], [REPEAT_ICON_SIDE as f32; 2], self.scale, 1.0,
            ));
            return;
        }
        let ink = repeat_icon_ink(mode);
        let colour = [
            ink[0] as f32 / 255.0, ink[1] as f32 / 255.0,
            ink[2] as f32 / 255.0, 1.0,
        ];
        // Keep the offline fallback within the smaller 20px icon box too.
        let factor = REPEAT_ICON_SIDE as f32 / 28.0;
        for (dx, dy, w, h) in [
            (2.0, 4.0, 22.0, 2.0), (22.0, 4.0, 2.0, 11.0),
            (19.0, 2.0, 5.0, 2.0), (22.0, 7.0, 5.0, 2.0),
            (4.0, 18.0, 22.0, 2.0), (4.0, 10.0, 2.0, 10.0),
            (4.0, 16.0, 5.0, 2.0), (2.0, 20.0, 5.0, 2.0),
        ] {
            self.rect([x + dx * factor, y + dy * factor],
                [w * factor, h * factor], colour);
        }
        if mode == RepeatMode::One {
            self.text_with_room("1", 11, [x + 8.0, y + 4.0], ink, 12);
        } else if mode == RepeatMode::Off {
            for j in 0..8 {
                self.rect([x + (5.0 + j as f32 * 2.7) * factor,
                    y + (24.0 - j as f32 * 2.3) * factor],
                    [2.0 * factor, 3.0 * factor], colour);
            }
        }
    }

    fn playing(&mut self, music: &Music, track: &Track, cover: Option<(TexId, [u32; 2])>,
               repeat_icon: Option<TexId>, playback_icon: Option<TexId>,
               video_badge_icon: Option<TexId>, title_face: Option<Face>) {
        self.music_tabs(0);
        self.repeat_badge(music.repeat_mode(), repeat_icon);
        let song = music.song(track);
        let album = music.playing_album();

        // Keep the artwork area stable while covers load asynchronously or
        // when the file has no embedded image; metadata never jumps sideways.
        self.rect(
            [PLAYING_COVER_X, PLAYING_COVER_Y],
            [PLAYING_COVER_SIDE, PLAYING_COVER_SIDE],
            MUSIC_COVER_PLACEHOLDER,
        );
        let mut art_top_left = [PLAYING_COVER_X, PLAYING_COVER_Y];
        if let Some((texture, pixels)) = cover {
            let size = pixels.map(|n| n as f32 * PLAYING_COVER_SIDE / THUMBNAIL as f32);
            art_top_left = [
                PLAYING_COVER_X + (PLAYING_COVER_SIDE - size[0]) / 2.0,
                PLAYING_COVER_Y + (PLAYING_COVER_SIDE - size[1]) / 2.0,
            ];
            self.draws.push(pixel_aligned_texture(
                texture, art_top_left, size, self.scale, 1.0,
            ));
        }
        // Draw last so this is a small overlay, not a replacement for the
        // artwork. Keep it relative to the visible cover even if letterboxed.
        if let Some(tex) = video_badge_icon {
            let side = VIDEO_BADGE_SIDE as f32;
            self.draws.push(pixel_aligned_texture(
                tex,
                [art_top_left[0] + VIDEO_BADGE_INSET, art_top_left[1] + VIDEO_BADGE_INSET],
                [side, side], self.scale, 1.0,
            ));
        }

        // Only the title scrolls when wider than its allotted column.
        // The viewport is cropped in RGBA before upload, so Hangul and long
        // Latin text never overpaint the album art or right screen margin.
        if let Some(face) = title_face {
            self.place(face, PLAYING_INFO_X, 128.0);
        } else {
            self.text_with_room(
                &song.title, 26, [PLAYING_INFO_X, 128.0], MUSIC_SELECTION_INK, PLAYING_INFO_W,
            );
        }
        self.text_with_room(
            &song.artist, 19, [PLAYING_INFO_X, 185.0], MUSIC_TEXT_INK, PLAYING_INFO_W,
        );
        self.text_with_room(
            &album.title, 17, [PLAYING_INFO_X, 221.0], MUSIC_MUTED_INK, PLAYING_INFO_W,
        );

        let (current, total) = music.playing_position().expect("active track");
        let position = format!("{} of {}", current + 1, total);
        self.text_with_room(
            &position, 16, [PLAYING_INFO_X, 326.0], MUSIC_MUTED_INK, PLAYING_INFO_W,
        );
        // Playback state appears beside elapsed time; keep only diagnostic
        // loading and error text beneath the track counter.
        let state = match track.status {
            Status::Playing => None,
            Status::Loading => Some("loading"),
            Status::Paused => None,
            Status::Finished => Some("finished"),
            Status::Unreadable => Some("could not play this track"),
            Status::NoAudio => Some("audio output unavailable"),
        };
        if let Some(state) = state {
            self.text_with_room(
                state, 15, [PLAYING_INFO_X, 358.0], MUSIC_MUTED_INK, PLAYING_INFO_W,
            );
        }

        // Progress tracks the decoder position. Up/Down seek ten seconds;
        // Left/Right change tracks; shoulders switch Music sections.
        let width = OUT_W as f32 - 2.0 * EDGE;
        self.rect([EDGE, PLAYING_PROGRESS_Y], [width, 4.0], MUSIC_PROGRESS_TRACK);
        if let Some((elapsed, total)) = track.time() {
            if let Some(tex) = playback_icon {
                self.draws.push(pixel_aligned_texture(
                    tex, [PLAYBACK_ICON_X, PLAYBACK_ICON_Y],
                    [PLAYBACK_ICON_SIDE as f32, PLAYBACK_ICON_SIDE as f32],
                    self.scale, 1.0,
                ));
            }
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
                self.text_ink(&timestamp(elapsed), 17,
                    [PLAYING_START_TIME_X, PLAYING_TIME_Y], MUSIC_MUTED_INK);
                self.text_right(
                    &timestamp(total), 17, PLAYING_END_TIME_RIGHT,
                    PLAYING_TIME_Y, MUSIC_MUTED_INK,
                );
            } else {
                self.text_ink(&timestamp(elapsed), 17,
                    [PLAYING_START_TIME_X, PLAYING_TIME_Y], MUSIC_MUTED_INK);
            }
        }
    }

    /// Same two-column, text-only selection as Slot's system Settings:
    /// no full-width coloured selection bars, cards, dividers or new palette.
    /// Settings is a START-only overlay, not a fourth Music tab.
    fn settings(&mut self, music: &Music) {
        self.heading("settings");
        let volume = format!("{}%", music.settings_max_volume());
        let dim_brightness = format!("{}/{}", music.dim_brightness(), BRIGHTNESS_MAX);
        let values = [
            volume.as_str(),
            music.settings_album_view(),
            music.settings_sort(),
            music.repeat_mode().label(),
            music.settings_dim_timeout(),
            dim_brightness.as_str(),
        ];
        let labels = [
            "volume limit", "album view", "sort albums", "repeat",
            "dim after", "dim brightness",
        ];
        for (index, (label, value)) in labels.iter().zip(values).enumerate() {
            let y = SETTINGS_FIRST_ROW_Y + index as f32 * SETTINGS_ROW_STEP;
            let selected = music.settings_cursor() == index;
            // System Settings highlights the label and value, not the row.
            let label_ink = if selected { MUSIC_SELECTION_INK } else { MUSIC_TEXT_INK };
            let value_ink = if selected { MUSIC_SELECTION_INK } else { MUSIC_MUTED_INK };
            self.text_with_room(label, 23, [EDGE, y + 9.0], label_ink,
                (SETTINGS_VALUE_X - EDGE - 16.0) as u32);
            self.text_with_room(value, 22, [SETTINGS_VALUE_X, y + 9.0], value_ink,
                (OUT_W as f32 - SETTINGS_VALUE_X - EDGE) as u32);
        }
    }

    /// Place the time readout against the right edge of the progress bar.
    fn text_right(&mut self, text: &str, px: u32, right: f32, y: f32, ink: [u8; 3]) {
        let key = format!("{px}/120/{ink:?}/{text}");
        let face = self.texture(key, |scale| menu_text_face(text, px, 120, scale, ink));
        self.place(face, right - face.size[0] as f32, y);
    }

    fn music_tabs(&mut self, selected: usize) {
        let mut x = EDGE;
        for (index, title) in ["now", "albums", "artists"].iter().enumerate() {
            let ink = if index == selected { MUSIC_TEXT_INK } else { MUSIC_MUTED_INK };
            let width = self.text_ink(title, 23, [x, TITLE_TOP], ink);
            x += width as f32 + 20.0;
        }
    }

    /// An idle player is deliberately blank beneath the tabs and HUD.
    /// Music is discoverable through Albums; no placeholder copy is needed.
    fn empty_now_playing(&mut self) {
        self.music_tabs(0);
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

/// A restrained, stronger diagonal colour wash, fading from the upper-left
/// (where the album art sits) towards warm ivory in the lower-right. The
/// screen keeps the same charcoal text and dusty-rose playback controls.
fn playing_background_colour(accent: [u8; 3], x: f32, y: f32) -> [f32; 4] {
    let diagonal = (0.58 * x.clamp(0.0, 1.0)
        + 0.42 * y.clamp(0.0, 1.0)).clamp(0.0, 1.0);
    let smooth = diagonal * diagonal * (3.0 - 2.0 * diagonal);
    // Previous vertical wash: 32% -> 6%. This is a visible but restrained
    // increase, from 43% in the top-left to 10% in the bottom-right.
    let strength = 0.10 + 0.33 * (1.0 - smooth);
    let mut result = MUSIC_BACKGROUND;
    for channel in 0..3 {
        let hue = f32::from(accent[channel]) / 255.0;
        result[channel] = MUSIC_BACKGROUND[channel] * (1.0 - strength)
            + hue * strength;
    }
    result
}

/// Native GPU bilinear interpolation makes the diagonal smooth at 640x480
/// using one tiny 80x60 texture rather than hundreds of independent quads.
fn playing_gradient_rgba(accent: [u8; 3]) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(
        (PLAYING_GRADIENT_W * PLAYING_GRADIENT_H * 4) as usize,
    );
    for y in 0..PLAYING_GRADIENT_H {
        for x in 0..PLAYING_GRADIENT_W {
            let colour = playing_background_colour(
                accent,
                x as f32 / (PLAYING_GRADIENT_W - 1) as f32,
                y as f32 / (PLAYING_GRADIENT_H - 1) as f32,
            );
            for channel in colour {
                rgba.push((channel * 255.0).round().clamp(0.0, 255.0) as u8);
            }
        }
    }
    rgba
}

/// Position a track row within the visible full-width list.
fn album_row_y(index: usize, first: usize) -> f32 {
    ALBUM_LIST_TOP + (index - first) as f32 * ALBUM_TRACK_ROW_H
}

/// ClassiPod Original spacing, adapted to the 640x480 Brick screen:
/// first neighbour 0.25 * viewport, subsequent covers +0.085 * viewport.
fn flow_geometry(distance: f32) -> Option<(f32, f32, f32, f32, f32)> {
    let abs = distance.abs();
    if abs >= 4.0 {
        return None;
    }
    let size = FLOW_FRONT_SIDE * (1.0 - 0.22 * abs.min(1.0));
    let offset = distance.signum() * OUT_W as f32
        * (0.25 * abs.min(1.0) + 0.085 * (abs - 1.0).clamp(0.0, 3.0));
    let x = (OUT_W as f32 - size) * 0.5 + offset;
    let y = FLOW_COVER_Y + (FLOW_FRONT_SIDE - size) * 0.5;
    let alpha = (4.0 - abs).clamp(0.0, 1.0);
    let yaw = distance.clamp(-1.0, 1.0) * FLOW_YAW;
    Some((x, y, size, alpha, yaw))
}

fn timestamp(seconds: f64) -> String {
    let seconds = seconds as u64;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

#[cfg(test)]
mod album_thumbnail_layout_tests {
    use super::*;

    #[test]
    fn album_list_uses_nine_rows_and_twenty_one_distinct_warm_slots() {
        assert_eq!(ROWS, 9);
        assert_eq!(ROW_H, 40.0);
        assert_eq!(LIST_THUMBNAIL, 64);
        assert_eq!(LIST_CACHE_SLOTS, 21);
        assert_eq!(list_prefetch_range(12, 50), 2..23);
        assert_eq!(list_prefetch_range(0, 50), 0..11);
        assert_eq!(list_prefetch_range(49, 50), 39..50);
        for cursor in 0..50 {
            let slots: std::collections::HashSet<usize> =
                list_prefetch_range(cursor, 50)
                    .map(|index| index % LIST_CACHE_SLOTS).collect();
            assert_eq!(slots.len(), list_prefetch_range(cursor, 50).len(),
                "each album in the warm window must own a unique slot");
        }
        assert!(LIST_TOP + ROWS as f32 * ROW_H <= OUT_H as f32 - 12.0);
        assert!(LIST_SCROLL_X > OUT_W as f32 - EDGE);
        assert!(LIST_SCROLL_X + 2.0 < OUT_W as f32);
    }

    #[test]
    fn album_thumbnails_fit_existing_rows_without_overlapping_text() {
        assert!(ALBUM_COVER_SIDE <= ROW_H);
        assert!(ALBUM_TEXT_X >= EDGE + ALBUM_COVER_SIDE);
        assert!(ALBUM_TEXT_X < OUT_W as f32 - EDGE);
    }

    #[test]
    fn cover_flow_preloads_four_additional_albums_on_each_side() {
        assert_eq!(FLOW_SLOTS, 7, "3D layout remains seven visible covers");
        assert_eq!(FLOW_EXTRA_COVERS, 4);
        assert_eq!(FLOW_CACHE_SLOTS, 15);
        assert_eq!(flow_prefetch_range(10, 40), 3..18);
        assert_eq!(flow_prefetch_range(0, 40), 0..8);
        assert_eq!(flow_prefetch_range(39, 40), 32..40);
        assert_eq!(flow_prefetch_range(0, 0), 0..0);
        assert_eq!(flow_prefetch_range(0, 1), 0..1);
        for cursor in 0..40 {
            let slots: std::collections::HashSet<usize> =
                flow_prefetch_range(cursor, 40)
                    .map(|index| index % FLOW_CACHE_SLOTS).collect();
            assert_eq!(slots.len(), flow_prefetch_range(cursor, 40).len(),
                "warm cache must never overwrite a different album in its own window");
        }
    }

    #[test]
    fn cover_flow_projection_keeps_a_focused_cover_between_two_smaller_neighbours() {
        let focused = flow_geometry(0.0).unwrap();
        let left = flow_geometry(-1.0).unwrap();
        let right = flow_geometry(1.0).unwrap();
        assert_eq!(focused.2, FLOW_FRONT_SIDE);
        assert_eq!(focused.4, 0.0, "selected cover must face forward");
        assert!(left.2 < focused.2 && right.2 < focused.2);
        assert!(left.0 + left.2 / 2.0 < focused.0 + focused.2 / 2.0);
        assert!(right.0 + right.2 / 2.0 > focused.0 + focused.2 / 2.0);
        assert!((right.4 - 0.95).abs() < 0.0001);
        assert!((left.4 + 0.95).abs() < 0.0001);
        assert_eq!(left.3, 1.0, "ClassiPod keeps near neighbours opaque");
        assert!(flow_geometry(3.0).is_some());
        assert_eq!(flow_geometry(4.0), None);
        assert!((right.0 - (320.0 + 160.0 - right.2 / 2.0)).abs() < 0.01);
    }

    #[test]
    fn cover_flow_animation_eases_and_finishes_without_idle_redraws() {
        let now = Instant::now();
        let mut motion = FlowMotion::default();
        assert_eq!(motion.to(2, now), 2.0);
        assert_eq!(motion.to(3, now), 2.0);
        assert!(motion.active(now + FLOW_DURATION / 2));
        let halfway = motion.at(now + FLOW_DURATION / 2).unwrap();
        assert!((halfway - (2.0 + std::f32::consts::FRAC_1_SQRT_2)).abs() < 0.001,
            "ClassiPod scroll uses easeOutSine");
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
    fn cover_flow_long_alphabet_jumps_skip_intermediate_cover_animation() {
        let now = Instant::now();
        let mut motion = FlowMotion::default();
        assert_eq!(motion.to(0, now), 0.0);
        assert_eq!(motion.to(18, now), 18.0,
            "jumping several alphabet groups should land directly on the target");
        assert!(!motion.active(now), "no intermediate covers should animate or decode");
        assert_eq!(motion.to(19, now), 18.0,
            "adjacent Left/Right cover moves still use the original easing");
        assert!(motion.active(now + FLOW_DURATION / 2));
    }

    #[test]
    fn cover_flow_album_labels_sit_above_the_old_reflection_area() {
        // Keep both labels clear of the front album (100 + 218px), while
        // using the newly empty space below the artwork.
        assert!(FLOW_ALBUM_TITLE_Y >= FLOW_COVER_Y + FLOW_FRONT_SIDE + 35.0);
        assert!(FLOW_ALBUM_TITLE_Y < 389.0);
        assert!(FLOW_ALBUM_ARTIST_Y >= FLOW_ALBUM_TITLE_Y + 32.0);
        assert!(FLOW_ALBUM_ARTIST_Y < 426.0);
        assert!(FLOW_ALBUM_ARTIST_Y + 28.0 < OUT_H as f32);
    }

    #[test]
    fn cover_flow_keeps_perspective_and_symmetry_without_reflections() {
        assert!((FLOW_PERSPECTIVE * FLOW_FRONT_SIDE - 0.35).abs() < 0.0001);
        let left = flow_geometry(-1.0).unwrap();
        let right = flow_geometry(1.0).unwrap();
        assert_eq!(left.4, -right.4);
        assert_eq!(left.2, right.2);
    }

    #[test]
    fn now_playing_reuses_cover_flow_or_list_gpu_texture_for_the_same_album() {
        let mut view = MusicView::default();
        let first = Path::new("Music/Album/01.mp3");
        let other = Path::new("Music/Other/01.mp3");
        assert_eq!(view.ready_album_art(first), None);

        // The last displayed Cover Flow image is already decoded and
        // uploaded; Now Playing must use exactly that GPU texture handle.
        let tex = TexId::from_raw(21);
        view.flow_covers[2] = MusicCover::loaded_for_test(first, tex, [256, 192]);
        assert_eq!(view.ready_album_art(first), Some((tex, [256, 192])));
        assert_eq!(view.ready_album_art(other), None);

        // Tiny list thumbnails must never be stretched to the 250px
        // Now Playing cover; it requests a separate full-quality texture.
        view.flow_covers[2] = MusicCover::default();
        let tex = TexId::from_raw(42);
        view.album_covers[0] = MusicCover::loaded_for_test(first, tex, [64, 32]);
        assert_eq!(view.ready_album_art(first), None);
        assert_eq!(view.ready_album_art(other), None);
    }

    #[test]
    fn now_playing_gradient_is_darker_and_diagonal_not_a_vertical_band() {
        let tint = [34, 90, 215];
        let top_left = playing_background_colour(tint, 0.0, 0.0);
        let top_right = playing_background_colour(tint, 1.0, 0.0);
        let bottom_left = playing_background_colour(tint, 0.0, 1.0);
        let bottom_right = playing_background_colour(tint, 1.0, 1.0);
        let red = playing_background_colour([220, 45, 50], 0.0, 0.0);
        assert!(red[0] > red[2], "warm covers yield a warm tint");
        assert!(top_left[2] > top_left[0], "blue artwork remains blue");
        assert!(top_left[0] < top_right[0], "fades from left to right");
        assert!(top_left[0] < bottom_left[0], "also fades top to bottom");
        assert!(bottom_right[0] > top_right[0], "both axes influence colour");
        assert!(bottom_left[0] < top_right[0], "top-to-bottom is not the only axis");
        assert!(top_left[0] > 0.52, "dark text stays readable on the backdrop");
        assert!(bottom_right[0] < MUSIC_BACKGROUND[0],
            "bottom-right still retains a faint album hue");
        assert_eq!(top_left[3], 1.0);
    }

    #[test]
    fn gradient_uses_one_small_opaque_rgba_texture_with_distinct_corners() {
        let rgba = playing_gradient_rgba([34, 90, 215]);
        assert_eq!(rgba.len(), (PLAYING_GRADIENT_W * PLAYING_GRADIENT_H * 4) as usize);
        assert_eq!(rgba[3], 255);
        assert_eq!(rgba[rgba.len() - 1], 255);
        assert_ne!(&rgba[..3], &rgba[rgba.len()-4..rgba.len()-1]);
        // Right neighbour of the top-left already differs along the diagonal.
        assert_ne!(&rgba[..3], &rgba[4 * (PLAYING_GRADIENT_W as usize - 1)..4 * PLAYING_GRADIENT_W as usize - 1]);
    }

    #[test]
    fn colour_lookup_reuses_correct_cover_flow_and_album_list_cache() {
        let mut view = MusicView::default();
        let first = Path::new("Music/Blue/01.mp3");
        let other = Path::new("Music/Red/01.mp3");
        assert_eq!(view.ready_album_tint(first), None);
        view.flow_covers[3] = MusicCover::tinted_for_test(
            first, TexId::from_raw(33), [256, 256], [45, 99, 201],
        );
        assert_eq!(view.ready_album_tint(first), Some([45, 99, 201]));
        assert_eq!(view.ready_album_tint(other), None);
        view.flow_covers[3] = MusicCover::default();
        view.album_covers[1] = MusicCover::tinted_for_test(
            first, TexId::from_raw(34), [64, 48], [210, 95, 60],
        );
        assert_eq!(view.ready_album_tint(first), None,
            "tiny list-thumb colour belongs to list cache, not Now Playing");
        assert_eq!(view.ready_album_tint(other), None);
    }

    #[test]
    fn uxwing_repeat_pngs_decode_to_small_rgba_textures() {
        let mut png_bytes = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut png_bytes, 2, 2);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            enc.write_header().unwrap().write_image_data(&[
                0, 0, 0, 255, 255, 255, 255, 0,
                0, 0, 0, 255, 255, 255, 255, 0,
            ]).unwrap();
        }
        let size = music_icon_size(REPEAT_ICON_SIDE, [1.6; 2]);
        assert_eq!(size, [32, 32]);
        let decoded = decode_music_icon(&png_bytes, size, Some([0; 3])).expect("valid transparent PNG");
        assert_eq!(decoded.len(), 32 * 32 * 4);
        assert!(decode_music_icon(b"not a png", size, None).is_none());
    }

    #[test]
    fn repeat_icons_use_black_for_active_modes_and_grey_for_off() {
        assert_eq!(repeat_icon_ink(RepeatMode::Off), [0x88, 0x88, 0x88]);
        assert_eq!(repeat_icon_ink(RepeatMode::All), [0, 0, 0]);
        assert_eq!(repeat_icon_ink(RepeatMode::One), [0, 0, 0]);
    }

    #[test]
    fn repeat_badge_aligns_with_end_time_without_covering_timeline() {
        assert!(PLAYING_PROGRESS_Y + 10.0 < PLAYING_TIME_Y,
            "timestamps and repeat must sit beneath the progress bar");
        assert_eq!(REPEAT_ICON_SIDE, 20);
        assert_eq!(REPEAT_BADGE_Y, PLAYING_TIME_Y + 2.0,
            "repeat and end timestamp share a centered 24px text row");
        assert!(PLAYING_END_TIME_RIGHT + 8.0 <= REPEAT_BADGE_X,
            "repeat must have a visible gap beside the end timestamp");
        assert!(REPEAT_BADGE_X + REPEAT_ICON_SIDE as f32 <= OUT_W as f32 - EDGE);
        assert!(REPEAT_BADGE_Y + REPEAT_ICON_SIDE as f32 <= OUT_H as f32 - EDGE,
            "repeat and times respect the same bottom screen margin");
        assert_eq!(REPEAT_ICON_FILES, ["repeat-off.png", "repeat-all.png", "repeat-one.png"]);
    }

    #[test]
    fn music_left_margin_matches_the_shelf_clock_and_hud() {
        assert_eq!(STATUS_MARGIN, 24.0);
        assert_eq!(EDGE, STATUS_MARGIN);
        assert_eq!(PLAYING_COVER_X, EDGE);
        assert_eq!(ALBUM_TRACK_TITLE_X, EDGE + 44.0);
        assert_eq!(REPEAT_BADGE_X, 596.0,
            "repeat icon aligns with the opposite HUD edge");
        assert_eq!(REPEAT_BADGE_Y, 434.0,
            "Repeat moves up next to the lowered end time");
    }

    #[test]
    fn uxwing_lock_icon_uses_compact_hud_size() {
        assert_eq!(HUD_LOCK_SIDE, 18);
        let view = MusicView::default();
        assert!(view.hud_lock_icon().is_none());
    }

    #[test]
    fn selected_coloured_padlock_png_keeps_its_own_colours() {
        let mut png_bytes = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut png_bytes, 2, 2);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            enc.write_header().unwrap().write_image_data(&[
                255, 215, 48, 255, 255, 215, 48, 255,
                255, 215, 48, 255, 255, 215, 48, 255,
            ]).unwrap();
        }
        let size = music_icon_size(HUD_LOCK_SIDE, [1.6; 2]);
        assert_eq!(size, [29, 29]);
        let pixels = decode_music_icon(&png_bytes, size, None).expect("valid coloured UXWing PNG");
        assert_eq!(pixels.len(), 29 * 29 * 4);
        assert_eq!(&pixels[0..4], &[255, 215, 48, 255]);
    }

    #[test]
    fn music_lock_icon_switches_immediately_then_open_icon_expires() {
        let now = Instant::now();
        let mut indicator = LockFeedback::default();
        assert_eq!(indicator.update(false, now), None, "no initial unlock flash");
        assert_eq!(indicator.update(true, now), Some(LockGlyph::Closed));
        assert_eq!(indicator.update(false, now), Some(LockGlyph::Open));
        assert_eq!(indicator.update(false, now + Duration::from_millis(500)),
            Some(LockGlyph::Open));
        assert!(indicator.pending(), "request redraw until icon disappears");
        assert_eq!(indicator.update(false, now + UNLOCK_ICON_HOLD), None);
        assert!(!indicator.pending(), "no background refresh after confirmation");
    }

    #[test]
    fn relocking_cancels_any_pending_open_indicator() {
        let now = Instant::now();
        let mut indicator = LockFeedback::default();
        assert_eq!(indicator.update(true, now), Some(LockGlyph::Closed));
        assert_eq!(indicator.update(false, now), Some(LockGlyph::Open));
        assert_eq!(indicator.update(true, now + Duration::from_millis(100)),
            Some(LockGlyph::Closed));
        assert!(!indicator.pending());
        assert_eq!(indicator.update(true, now + UNLOCK_ICON_HOLD),
            Some(LockGlyph::Closed));
    }

    #[test]
    fn youtube_png_badge_preserves_red_colour_and_transparency() {
        let mut png_bytes = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut png_bytes, 2, 2);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            enc.write_header().unwrap().write_image_data(&[
                255, 0, 0, 255, 255, 0, 0, 255,
                255, 0, 0, 255, 255, 0, 0, 255,
            ]).unwrap();
        }
        let size = music_icon_size(VIDEO_BADGE_SIDE, [1.6; 2]);
        assert_eq!(size, [35, 35]);
        let pixels = decode_music_icon(&png_bytes, size, None).expect("valid logo PNG");
        assert_eq!(pixels.len(), 35 * 35 * 4);
        assert_eq!(&pixels[0..4], &[255, 0, 0, 255]);
    }

    #[test]
    fn track_video_badge_has_solid_monochrome_body_and_clear_perforations() {
        let mut png_bytes = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut png_bytes, 2, 2);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            enc.write_header().unwrap().write_image_data(&[
                0, 0, 0, 255, 255, 255, 255, 255,
                0, 0, 0, 0, 0, 0, 0, 128,
            ]).unwrap();
        }
        for ink in [[0, 0, 0], ALBUM_WHITE_INK] {
            let pixels = decode_track_video_icon(&png_bytes, [2, 2], ink).unwrap();
            assert_eq!(&pixels[..3], &ink);
            assert_eq!(pixels[3], 255);
            assert_eq!(pixels[7], 0);
            assert_eq!(pixels[11], 0);
            assert_eq!(pixels[15], 128);
        }
    }

    #[test]
    fn inline_video_badge_reserves_space_only_when_needed() {
        assert!(TRACK_VIDEO_BADGE_SIDE < VIDEO_BADGE_SIDE);
        assert_eq!(track_title_room(false) - track_title_room(true),
            TRACK_VIDEO_BADGE_SIDE + TRACK_VIDEO_BADGE_GAP);
        assert!(ALBUM_TRACK_TITLE_X + track_title_room(true) as f32
            + TRACK_VIDEO_BADGE_GAP as f32 + TRACK_VIDEO_BADGE_SIDE as f32
            <= OUT_W as f32 - EDGE);
        assert_eq!(music_icon_size(TRACK_VIDEO_BADGE_SIDE, [1.6; 2]), [29, 29]);
    }

    #[test]
    fn video_badge_detects_only_matching_non_empty_local_companions() {
        let dir = tempfile::tempdir().unwrap();
        let song = dir.path().join("My Song.mp3");
        let next = dir.path().join("Other Song.m4a");
        std::fs::write(&song, b"music").unwrap();
        std::fs::write(&next, b"music").unwrap();
        std::fs::write(dir.path().join("Different.mp4"), b"video").unwrap();
        assert!(!has_companion_video(&song));
        let video = dir.path().join("My Song.mp4");
        std::fs::write(&video, b"").unwrap();
        assert!(!has_companion_video(&song), "empty video must not show badge");
        std::fs::write(&video, b"video").unwrap();
        assert!(has_companion_video(&song));
        assert!(!has_companion_video(&next));
        std::fs::write(dir.path().join("Other Song.m4v"), b"video").unwrap();
        assert!(has_companion_video(&next));
    }

    #[test]
    fn video_badge_cache_rechecks_when_the_playing_song_changes() {
        let dir = tempfile::tempdir().unwrap();
        let with_video = dir.path().join("With.mp3");
        let without_video = dir.path().join("Without.mp3");
        std::fs::write(dir.path().join("With.mp4"), b"video").unwrap();
        let mut cache = VideoBadgeCache::default();
        assert!(cache.available_for(&with_video));
        assert!(!cache.available_for(&without_video));
        assert!(cache.available_for(&with_video));
    }

    #[test]
    fn camera_roll_badge_is_small_inside_the_top_left_of_the_cover() {
        assert_eq!(VIDEO_BADGE_FILE, "camera-roll.png");
        assert!(VIDEO_BADGE_SIDE <= 24);
        assert!(VIDEO_BADGE_INSET >= 4.0);
        assert!(VIDEO_BADGE_INSET + (VIDEO_BADGE_SIDE as f32)
            < PLAYING_COVER_SIDE / 4.0);
        assert!(PLAYING_COVER_X + VIDEO_BADGE_INSET
            + (VIDEO_BADGE_SIDE as f32) < PLAYING_INFO_X);
    }

    #[test]
    fn playback_status_selects_play_or_pause_and_retains_diagnostics() {
        assert_eq!(playback_icon_index(Status::Playing), Some(0));
        assert_eq!(playback_icon_index(Status::Paused), Some(1));
        for status in [Status::Loading, Status::Finished, Status::Unreadable, Status::NoAudio] {
            assert_eq!(playback_icon_index(status), None);
        }
    }

    #[test]
    fn play_pause_icons_fit_before_elapsed_time_without_shifting_progress() {
        assert_eq!(PLAYBACK_ICON_FILES, ["play.png", "pause.png"]);
        assert_eq!(PLAYBACK_ICON_SIDE, REPEAT_ICON_SIDE);
        assert_eq!(PLAYBACK_ICON_X, EDGE);
        assert_eq!(PLAYBACK_ICON_Y, REPEAT_BADGE_Y);
        assert!(PLAYBACK_ICON_X + PLAYBACK_ICON_SIDE as f32 + 8.0
            <= PLAYING_START_TIME_X);
        assert!(PLAYING_START_TIME_X < PLAYING_END_TIME_RIGHT - 75.0);
    }

    #[test]
    fn uxwing_play_pause_pngs_decode_to_black_icons_without_opaque_background() {
        let mut png_bytes = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut png_bytes, 2, 2);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            enc.write_header().unwrap().write_image_data(&[
                0, 0, 0, 255, 255, 255, 255, 255,
                0, 0, 0, 0, 0, 0, 0, 255,
            ]).unwrap();
        }
        let size = music_icon_size(PLAYBACK_ICON_SIDE, [1.6; 2]);
        assert_eq!(size, [32, 32]);
        let decoded = decode_music_icon(&png_bytes, size, Some([0; 3]))
            .expect("valid transparent playback PNG");
        assert_eq!(decoded.len(), 32 * 32 * 4);
        assert!(decoded.chunks_exact(4).any(|pixel| pixel[3] == 0));
        assert!(decoded.chunks_exact(4).all(|pixel| pixel[..3] == [0, 0, 0]));
    }

    #[test]
    fn now_playing_art_and_text_are_separate_at_640_by_480() {
        assert_eq!((OUT_W, OUT_H), (640, 480));
        assert_eq!(PLAYING_COVER_SIDE, 250.0,
            "artwork expands to use the space above the lowered timeline");
        assert!(PLAYING_COVER_X + PLAYING_COVER_SIDE + 12.0 < PLAYING_INFO_X);
        assert!(PLAYING_INFO_W >= 270);
        assert!(PLAYING_INFO_X + PLAYING_INFO_W as f32 <= OUT_W as f32 - EDGE);
        assert!(PLAYING_COVER_Y + PLAYING_COVER_SIDE < PLAYING_PROGRESS_Y);
        assert!(PLAYING_PROGRESS_Y + 48.0 < OUT_H as f32);
    }

    #[test]
    fn title_marquee_crops_prepared_pixels_and_clamps_out_of_range_scroll() {
        let whole = UndoFace {
            w: 5, h: 2,
            rgba: (0..10u8).flat_map(|n| [n, n, n, 255]).collect(),
        };
        let a = crop_marquee_rgba(&whole, 3, 0);
        let b = crop_marquee_rgba(&whole, 3, 1);
        let c = crop_marquee_rgba(&whole, 3, 99);
        assert_eq!(&a[..12], &[0,0,0,255,1,1,1,255,2,2,2,255]);
        assert_eq!(&b[..12], &[1,1,1,255,2,2,2,255,3,3,3,255]);
        assert_eq!(&c[..12], &[2,2,2,255,3,3,3,255,4,4,4,255]);
        assert_eq!(&c[12..], &[7,7,7,255,8,8,8,255,9,9,9,255]);
    }

    #[test]
    fn now_tab_and_track_counter_have_no_redundant_playing_status() {
        assert_eq!(TITLE_MARQUEE_PX, 26);
        assert_eq!(PLAYING_PROGRESS_Y, 414.0);
        assert!(326.0 > 277.0 && 326.0 < PLAYING_PROGRESS_Y);
    }

    #[test]
    fn title_marquee_pauses_scrolls_and_returns_only_for_long_titles() {
        let now = Instant::now();
        let title = "매우 긴 제목 — this is a long album title that cannot fit the display column";
        let path = Path::new("Music/Artist/01.mp3");
        let mut marquee = TitleMarquee::default();
        assert_eq!(marquee.offset(path, "Short", PLAYING_INFO_W, now), None);
        assert!(!marquee.overflowing);
        assert_eq!(marquee.offset(path, title, PLAYING_INFO_W, now), Some(0.0));
        assert!(marquee.overflowing);
        assert_eq!(marquee.offset(path, title, PLAYING_INFO_W,
            now + Duration::from_millis(700)), Some(0.0));
        let scroll = marquee.offset(path, title, PLAYING_INFO_W,
            now + Duration::from_secs(2)).unwrap();
        assert!(scroll > 0.0, "title must eventually move to reveal hidden glyphs");
        // Keep the cached raster and metrics when only time advances.
        marquee.whole = Some(UndoFace { rgba: vec![0; 16], w: 2, h: 2 });
        let before = marquee.overflow;
        let later = now + Duration::from_secs(3);
        marquee.offset(path, title, PLAYING_INFO_W, later);
        assert!(marquee.whole.is_some(), "same song must not be rasterized again");
        assert_eq!(marquee.overflow, before);
        marquee.offset(path, "Another long English track title to change the identity", PLAYING_INFO_W, later);
        assert!(marquee.whole.is_none(), "new title invalidates old glyph pixels");
        marquee.reset();
        assert!(!marquee.overflowing);
        assert_eq!(marquee.song, None);
    }

    #[test]
    fn progress_and_times_use_bottom_space_without_overlapping_album_art() {
        assert!(PLAYING_COVER_Y + PLAYING_COVER_SIDE + 32.0 < PLAYING_PROGRESS_Y);
        assert!(PLAYING_PROGRESS_Y > 400.0);
        assert!(PLAYING_TIME_Y + 24.0 <= OUT_H as f32 - EDGE);
        assert!(PLAYING_INFO_X > PLAYING_COVER_X + PLAYING_COVER_SIDE);
        assert!(PLAYING_END_TIME_RIGHT < REPEAT_BADGE_X);
        assert!(PLAYING_INFO_W >= 300, "long-title viewport must remain useful");
    }

    #[test]
    fn classic_album_list_spans_the_screen_and_preserves_a_bottom_counter() {
        assert_eq!((OUT_W, OUT_H), (640, 480));
        assert!(ALBUM_LIST_TOP > ALBUM_HEADER_BOTTOM);
        let list_end = album_row_y(ALBUM_VISIBLE_ROWS, 0);
        assert!(list_end < 448.0, "rows overlap the bottom counter");
        assert!(list_end > 400.0, "classic rows should use available screen height");
        assert_eq!(ALBUM_VISIBLE_ROWS, 7);
        assert!(ALBUM_TRACK_TITLE_X > EDGE + 35.0);
        assert!(ALBUM_TRACK_TITLE_X < OUT_W as f32 / 3.0);
    }

    #[test]
    fn classic_album_selection_uses_the_existing_music_palette() {
        assert_eq!(ALBUM_WHITE_INK, [255, 255, 255]);
        assert_eq!(MUSIC_SELECTION_INK, [160, 80, 105]);
        assert_eq!(album_row_y(0, 0), ALBUM_LIST_TOP);
        assert_eq!(album_row_y(6, 0),
            ALBUM_LIST_TOP + 6.0 * ALBUM_TRACK_ROW_H);
        assert_eq!(album_row_y(9, 3),
            ALBUM_LIST_TOP + 6.0 * ALBUM_TRACK_ROW_H);
    }

    #[test]
    fn music_settings_six_rows_fit_above_the_footer() {
        let bottom = SETTINGS_FIRST_ROW_Y + 5.0 * SETTINGS_ROW_STEP + SETTINGS_ROW_H;
        assert_eq!(SETTINGS_ROW_STEP, QUICK_PITCH,
            "Music settings must follow the standard system Settings row pitch");
        assert_eq!(SETTINGS_VALUE_X, QUICK_VALUE_EDGE,
            "Music settings values must use the standard two-column split");
        assert!(bottom < OUT_H as f32 - EDGE,
            "all six settings rows fit without a footer or help captions");
    }

    #[test]
    fn music_has_its_own_light_palette_without_changing_system_menus() {
        assert_eq!(MUSIC_BACKGROUND, [245.0 / 255.0, 241.0 / 255.0, 234.0 / 255.0, 1.0]);
        // A warm off-white should not read as an untinted 255-white canvas.
        assert!(MUSIC_BACKGROUND[0] > MUSIC_BACKGROUND[1]);
        assert!(MUSIC_BACKGROUND[1] > MUSIC_BACKGROUND[2]);
        assert!(MUSIC_BACKGROUND[2] < 0.94);
        // The classic album title bar stays slightly deeper than the canvas.
        for channel in 0..3 {
            assert!(ALBUM_HEADER_BACKGROUND[channel] < MUSIC_BACKGROUND[channel]);
        }
        assert_eq!(MUSIC_TEXT_INK, [0x24, 0x24, 0x24]);
        assert_ne!(MUSIC_BACKGROUND, slot_ui::MENU_BACKGROUND);
        assert_ne!(MUSIC_TEXT_INK, slot_ui::MENU_TEXT_INK);
        assert!(MUSIC_SELECTION_INK[0] < 180, "selected album must remain visible on white");
    }
}
