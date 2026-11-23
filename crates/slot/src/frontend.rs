use std::time::{Duration, Instant};

use slot_gfx::{Compositor, Draw, ScreenEffect, TexId, OUT_H, OUT_W};

const PAPER: &[u8] = include_bytes!("../assets/paper.png");
use slot_input::{InputSource, Millis};
use slot_power::{Platform, Power};
use slot_store::format_stamp;
use slot_ui::{
    arrows_hint_face, badge_face, bezel_face, cart_shadow, chip_face, chip_shadow_face,
    date_time_text, gb_cart_shadow, hhmm, hint_face, icon_face, menu_face, photo_face,
    menu_hint_face_scaled, menu_row_face_scaled, quick_value_face_scaled,
    rebuild_count_face,
    rebuild_title_face, set_clock_hint_face, socket_face, sticker_face, title_face, toast_face,
    wallpaper_face, word_face, word_face_ink_scaled, GbShell, Icon, LinkBadge, PauseChoice, PowerChoice, QuickMenuFaces,
    QuickRow, QuickValue, RebuildScreen, StatusIcon, StickerFields, Toast, UndoFace, ALERT_PX, BOLT_PX,
    HUD_ICON_PX, HUD_INK, LEGEND, QUICK_TEXT_H,
};

use crate::app::{App, LinkRow, Phase};
use crate::build_info::Build;
use crate::cart_faces::CartFaces;
use crate::face_builder::FaceBuilder;
use crate::label_cache::{self, Rebuild};
use crate::link_art_builder::LinkArtBuilder;
use crate::link_screen::{LinkSprites, Sprite};
use crate::link_start::{LinkFail, LinkStep};
use crate::session::Session;
use crate::wallpaper;

// Top-shelf Android Original appearance uses black time, icons, and percentage.
const SHELF_STATUS_INK: [u8; 3] = [0, 0, 0];

const DOZE_TIMEOUT: Duration = Duration::from_secs(180);

#[cfg(feature = "brick")]
fn music_dim_target(music: &crate::music::Music, idle: Duration) -> Option<u8> {
    if music.locked() || (
        music.should_dim()
        && idle >= Duration::from_secs(u64::from(music.dim_timeout_secs()))
    ) {
        Some(music.dim_brightness())
    } else {
        None
    }
}

// The Brick has no lid, so an untouched device would otherwise stay lit until
// the battery is flat. Idle limits before slot dozes (and then suspends).
#[cfg(feature = "brick")]
const IDLE_MENU: Duration = Duration::from_secs(5 * 60);
#[cfg(feature = "brick")]
const IDLE_GAME: Duration = Duration::from_secs(15 * 60);
// After a wake nobody follows up with input (charger plug, bumped key), sleep again soon.
#[cfg(feature = "brick")]
const IDLE_AFTER_WAKE: Duration = Duration::from_secs(90);

const ALERT_INK: [u8; 3] = [0xf0, 0xb4, 0x3c];

/// Fade from the hardware boot logo into Slot, then into each navigated menu.
const BOOT_FADE: Duration = Duration::from_millis(440);
const MENU_FADE: Duration = Duration::from_millis(220);
const SLEEP_FADE: Duration = Duration::from_millis(300);
const WAKE_FADE: Duration = Duration::from_millis(360);
/// Power-off still uses the existing 250 ms confirmation interval.
const SHUTDOWN_FADE: Duration = Duration::from_millis(210);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NavScene {
    Shelf,
    QuickMenu(Option<QuickRow>),
    Music,
    About,
    Clock,
}

impl NavScene {
    fn from_phase(phase: &Phase) -> Option<Self> {
        match phase {
            Phase::Shelf => Some(Self::Shelf),
            // The parent identifies the page. Moving between rows on that page
            // must not replay the fade.
            Phase::QuickMenu { row } => Some(Self::QuickMenu(row.parent())),
            Phase::Music => Some(Self::Music),
            Phase::About => Some(Self::About),
            Phase::SetClock { from_menu: true, .. } => Some(Self::Clock),
            _ => None,
        }
    }
}

/// A single fullscreen black layer fades away over the destination screen.
#[derive(Default)]
struct NavFade {
    scene: Option<NavScene>,
    started: Option<(Instant, Duration)>,
    /// The first actual Slot frame may be Inserting/Playing, not Shelf. Keep
    /// this boot fade independent of later scene changes and game ejection.
    boot_started: Option<Instant>,
    presented: bool,
}

impl NavFade {
    fn begin_first_frame(&mut self, next: Option<NavScene>, now: Instant) {
        if self.presented {
            return;
        }
        self.presented = true;
        self.scene = next;
        self.boot_started = Some(now);
    }

    fn update(&mut self, next: Option<NavScene>, now: Instant) {
        if self.scene == next {
            return;
        }
        let previous = self.scene;
        self.scene = next;
        self.started = next.and_then(|scene| {
            let first_launcher = !self.presented && scene == NavScene::Shelf;
            // The game already has its own eject animation. Coming back to the
            // shelf from a non-menu phase must not overlay an opaque black frame.
            // Menu -> Shelf still fades as before, and first boot still fades in.
            let from_game = self.presented
                && previous.is_none()
                && scene == NavScene::Shelf;
            self.presented = true;
            (!from_game).then_some((
                now,
                if first_launcher { BOOT_FADE } else { MENU_FADE },
            ))
        });
    }

    fn fade_opacity(now: Instant, start: Instant, duration: Duration) -> f32 {
        let progress = (now.saturating_duration_since(start).as_secs_f32()
            / duration.as_secs_f32()).clamp(0.0, 1.0);
        let smooth = progress * progress * (3.0 - 2.0 * progress);
        1.0 - smooth
    }

    fn opacity(&self, now: Instant) -> f32 {
        let navigation = self.started.map_or(0.0, |(start, duration)| {
            Self::fade_opacity(now, start, duration)
        });
        let boot = self.boot_started.map_or(0.0, |start| {
            Self::fade_opacity(now, start, BOOT_FADE)
        });
        navigation.max(boot)
    }

    fn active(&self, now: Instant) -> bool {
        self.started.is_some_and(|(start, duration)| now.saturating_duration_since(start) < duration)
            || self.boot_started.is_some_and(|start| now.saturating_duration_since(start) < BOOT_FADE)
    }
}

/// Power transitions reuse the last visible draw list, so entering Doze or the
/// shutdown screen never exposes an abrupt black frame. Rendering is time-bound.
#[derive(Default)]
struct PowerFade {
    sleep: Option<(Instant, Vec<Draw>)>,
    sleep_black_presented: bool,
    wake: Option<Instant>,
    shutdown: Option<(Instant, Vec<Draw>)>,
}

impl PowerFade {
    fn progress(now: Instant, since: Instant, length: Duration) -> f32 {
        let t = (now.saturating_duration_since(since).as_secs_f32()
            / length.as_secs_f32()).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }

    fn begin_sleep(&mut self, now: Instant, last_frame: &[Draw]) {
        if self.sleep.is_none() {
            self.sleep = Some((now, last_frame.to_vec()));
            self.sleep_black_presented = false;
            self.wake = None;
        }
    }

    /// Render the fade frames plus one final fully black frame, then stop.
    fn sleep_frame_needed(&mut self, now: Instant) -> bool {
        let Some((start, _)) = &self.sleep else { return false };
        if now.saturating_duration_since(*start) < SLEEP_FADE {
            return true;
        }
        if !self.sleep_black_presented {
            self.sleep_black_presented = true;
            return true;
        }
        false
    }

    fn resume(&mut self, now: Instant) {
        self.sleep = None;
        self.sleep_black_presented = false;
        self.wake = Some(now);
    }

    fn wake_opacity(&self, now: Instant) -> f32 {
        self.wake.map_or(0.0, |since| 1.0 - Self::progress(now, since, WAKE_FADE))
    }

    fn wake_active(&self, now: Instant) -> bool {
        self.wake.is_some_and(|since| now.saturating_duration_since(since) < WAKE_FADE)
    }

    fn begin_shutdown(&mut self, now: Instant, last_frame: &[Draw]) {
        if self.shutdown.is_none() {
            self.shutdown = Some((now, last_frame.to_vec()));
        }
    }

    fn shutdown_active(&self, now: Instant) -> bool {
        self.shutdown.as_ref().is_some_and(|(since, _)|
            now.saturating_duration_since(*since) < SHUTDOWN_FADE)
    }
}

pub struct Frontend {
    session: Session,
    start: Instant,
    last: Instant,
    wake_guard: Option<Instant>,
    last_input: Instant,
    last_render: Instant,
    rendered_effects: (f32,f32,f32,Option<f32>),
    nav_fade: NavFade,
    power_fade: PowerFade,
    draws: Vec<Draw>,
    polaroid_texes: Vec<TexId>,
    title_tex: Option<TexId>,
    faces: FaceBuilder,
    /// The label row's words, [dim, lit], rewritten as the label search goes.
    /// The label grid's textures, kept and written over as its pages turn.
    label_grid_tex: LabelGridTex,
    label_adjust_tex: LabelAdjustTex,
    peel_tex: PeelTex,
    cart_faces: CartFaces,
    rebuild: Option<Rebuild>,
    rebuild_faces: RebuildFaces,
    link_art: LinkArtBuilder,
    link_art_done: bool,
    core_asked: Option<String>,
    core_board_tex: Option<TexId>,
    core_lid_tex: Option<TexId>,
    core_built: Option<String>,
    undo_tex: Option<TexId>,
    switcher: Switcher,
    clocks: Clocks,
    about: AboutFace,
    quick_clock: QuickClock,
    quick_menu_scale: [f32; 2],
    music_view: crate::music_view::MusicView,
    overlay_tex: OverlayTex,
    #[cfg(feature = "brick")]
    wifi_tex: Option<TexId>,
    banner_tex: Option<TexId>,
    #[cfg(feature = "brick")]
    auto_sleep: bool,
    #[cfg(feature = "brick")]
    woke_untouched: bool,
}

#[derive(Default)]
struct RebuildFaces {
    title: Option<(TexId, u32, u32)>,
    count: Option<TexId>,
    count_size: (u32, u32),
    shown: Option<usize>,
}

/// The overlay over the game, read off the UI thread, and each system's Overlay row's words.
#[derive(Default)]
struct OverlayTex {
    picture: Option<TexId>,
    /// The overlay being read, and where it will come back.
    reading: Option<(std::path::PathBuf, std::sync::mpsc::Receiver<Option<crate::overlay::Overlay>>)>,
    /// An overlay that could not be used, so it is not read again every frame.
    refused: Option<std::path::PathBuf>,
    texts: [[Option<TexId>; 2]; QuickRow::NAMED],
    /// A name too long for its row, scrolled while the row is lit.
    scrolls: [Option<Scroll>; QuickRow::NAMED],
}

/// A value too long to show whole beside its label: the whole of it drawn once, and the piece
/// of it showing, `room` wide, moved along slowly and back while its row is lit.
struct Scroll {
    whole: slot_ui::UndoFace,
    room: u32,
    scale_x: f32,
    since: std::time::Instant,
    at: Option<u32>,
}

/// How fast a long value scrolls, and how long it rests at either end.
const SCROLL_PX_PER_S: f32 = 40.0;
const SCROLL_REST_S: f32 = 1.2;

impl Scroll {
    /// How far along the piece showing is, `secs` after the row was lit: resting at the start,
    /// along to the end, resting, and back.
    fn offset(&self, secs: f32) -> u32 {
        let span = self.whole.w.saturating_sub(self.room) as f32;
        let speed = SCROLL_PX_PER_S * self.scale_x;
        let travel = span / speed;
        let t = secs % (2.0 * (SCROLL_REST_S + travel));
        let x = if t < SCROLL_REST_S {
            0.0
        } else if t < SCROLL_REST_S + travel {
            (t - SCROLL_REST_S) * speed
        } else if t < 2.0 * SCROLL_REST_S + travel {
            span
        } else {
            span - (t - 2.0 * SCROLL_REST_S - travel) * speed
        };
        x.clamp(0.0, span) as u32
    }

    /// The piece `at` along, faded at a clipped edge and keeping the face's text margins.
    fn piece(&self, at: u32) -> Vec<u8> {
        let (w, h, room) = (self.whole.w, self.whole.h, self.room);
        let pad = (slot_ui::MENU_PAD as f32 * self.scale_x).round() as u32;
        let ink = room.saturating_sub(2 * pad);
        let mut out = vec![0u8; (room * h * 4) as usize];
        for y in 0..h {
            let from = ((y * w + pad + at) * 4) as usize;
            let to = ((y * room + pad) * 4) as usize;
            out[to..to + (ink * 4) as usize].copy_from_slice(&self.whole.rgba[from..from + (ink * 4) as usize]);
        }
        let fade = (14.0 * self.scale_x).round() as u32;
        for x in 0..fade.min(ink) {
            let k = x as f32 / fade as f32;
            for y in 0..h {
                if at > 0 {
                    let i = ((y * room + pad + x) * 4 + 3) as usize;
                    out[i] = (out[i] as f32 * k) as u8;
                }
                if at + room < w {
                    let i = ((y * room + pad + ink - 1 - x) * 4 + 3) as usize;
                    out[i] = (out[i] as f32 * k) as u8;
                }
            }
        }
        out
    }
}

/// `text` as a value face no wider than `room`: whole when it fits, else cut short with "...".
fn shortened_value_face(text: &str, room: u32, scale: [f32; 2]) -> (UndoFace, u32) {
    let (whole, width) = quick_value_face_scaled(text, false, scale);
    if width <= room {
        return (whole, width);
    }
    let chars: Vec<char> = text.chars().collect();
    // The most of it that fits with the dots after it.
    let (mut lo, mut hi) = (0, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let cut: String = chars[..mid].iter().collect::<String>() + "...";
        if quick_value_face_scaled(&cut, false, scale).1 <= room { lo = mid } else { hi = mid - 1 }
    }
    quick_value_face_scaled(&(chars[..lo].iter().collect::<String>() + "..."), false, scale)
}

/// The rows that read a name (each system's Overlay row: the overlay's, or Off; Font: the
/// font's) are made again when it changes.
fn sync_named_texts(app: &mut App, compositor: &mut Compositor, tex: &mut OverlayTex, scale: [f32; 2]) {
    if app.quick_menu().is_none() {
        return;
    }
    let room = slot_ui::quick_value_room();
    let lit_row = app.quick_menu();
    for row in QuickRow::ALL {
        let Some(i) = row.named() else {
            continue;
        };
        if app.named_text_shown(i) != Some(app.named_text(i)) {
            // The name changed: made again, cut short to fit when not lit and scrolled when lit.
            // The Font row names each font in that font.
            let text = app.named_text(i).to_string();
            let own = match (row, app.root()) {
                (QuickRow::Font, Some(root)) => crate::fonts::preview(root, &text),
                _ => None,
            };
            let in_own = |draw: &mut dyn FnMut() -> (UndoFace, u32)| match own {
                Some(font) => slot_ui::text::in_font(font, draw),
                None => draw(),
            };
            let (dim, dim_w) = in_own(&mut || shortened_value_face(&text, room, scale));
            let (whole, whole_w) = in_own(&mut || quick_value_face_scaled(&text, true, scale));
            let (lit, lit_w, scroll) = if whole_w > room {
                let scroll = Scroll {
                    whole,
                    room: (room as f32 * scale[0]).round() as u32,
                    scale_x: scale[0],
                    since: std::time::Instant::now(),
                    at: Some(0),
                };
                let piece = UndoFace { rgba: scroll.piece(0), w: scroll.room, h: scroll.whole.h };
                (piece, room, Some(scroll))
            } else {
                (whole, whole_w, None)
            };
            tex.scrolls[i] = scroll;
            let dim = upload(compositor, &mut tex.texts[i][0], dim);
            let lit = upload(compositor, &mut tex.texts[i][1], lit);
            app.set_named_text_faces(i, text, [(dim, dim_w, QUICK_TEXT_H), (lit, lit_w, QUICK_TEXT_H)]);
        }
        // A long name moves along only while its row is the lit one, starting from its start.
        let (Some(scroll), Some(id)) = (&mut tex.scrolls[i], tex.texts[i][1]) else {
            continue;
        };
        if lit_row != Some(row) {
            scroll.since = std::time::Instant::now();
        }
        let at = scroll.offset(scroll.since.elapsed().as_secs_f32());
        if scroll.at != Some(at) {
            scroll.at = Some(at);
            compositor.update_texture(id, scroll.room, scroll.whole.h, &scroll.piece(at));
        }
    }
}

/// The overlay the game wants: read on a thread of its own when it changes, then uploaded and
/// handed to the app, which fits the game into its window.
fn sync_overlay(app: &mut App, compositor: &mut Compositor, tex: &mut OverlayTex) {
    let wanted = app.overlay_wanted();
    if wanted.as_deref() == app.overlay_loaded() {
        return;
    }
    let Some(wanted) = wanted else {
        // Out of the game: the next one tries again an overlay that could not be used.
        app.take_overlay();
        tex.refused = None;
        return;
    };
    if tex.refused.as_ref() == Some(&wanted) {
        return;
    }
    if tex.reading.as_ref().is_none_or(|(path, _)| *path != wanted) {
        let (tx, rx) = std::sync::mpsc::channel();
        let path = wanted.clone();
        let spawned = std::thread::Builder::new()
            .name("slot-overlay".into())
            .spawn(move || {
                let _ = tx.send(crate::overlay::load(&path));
            });
        if let Err(e) = spawned {
            eprintln!("slot: overlay: {e}");
            tex.refused = Some(wanted);
            return;
        }
        tex.reading = Some((wanted, rx));
        return;
    }
    let Some((path, rx)) = &tex.reading else {
        return;
    };
    let read = match rx.try_recv() {
        Ok(read) => read,
        Err(std::sync::mpsc::TryRecvError::Empty) => return,
        Err(std::sync::mpsc::TryRecvError::Disconnected) => None,
    };
    let path = path.clone();
    tex.reading = None;
    match read {
        Some(overlay) => {
            let id = upload_rgba(compositor, &mut tex.picture, overlay.w, overlay.h, &overlay.rgba);
            eprintln!("slot: overlay {} (window {:?})", path.display(), overlay.window);
            app.set_overlay(path, id, overlay.window);
        }
        None => {
            // Not a PNG Slot can read, or one with no clear window for the game.
            eprintln!("slot: overlay: {} is not one Slot can use", path.display());
            app.take_overlay();
            tex.refused = Some(path);
        }
    }
}

#[derive(Default)]
struct QuickClock {
    dim: Option<TexId>,
    lit: Option<TexId>,
    shown: String,
}

#[derive(Default)]
struct AboutFace {
    tex: Option<TexId>,
    battery: Option<u8>,
}

/// A favorite's star on the shelf: its size at full scale, and its gold.
const STAR_PX: f32 = 40.0;
const STAR_INK: [u8; 3] = [0xff, 0xc8, 0x3c];
/// Its lit facets, and its shaded ones.
const STAR_LIGHT: [u8; 3] = [0xff, 0xec, 0x9a];
const STAR_SHADE: [u8; 3] = [0xd2, 0x86, 0x1c];

#[derive(Default)]
struct Clocks {
    line: Option<TexId>,
    hint: Option<TexId>,
    shelf: Option<TexId>,
    picked: Option<String>,
    shown: String,
    status_scale: [f32; 2],
    wifi_tex: Option<TexId>,
    status_bolt_tex: Option<TexId>,
    battery: String,
    battery_tex: Option<TexId>,
    platform: String,
    platform_tex: Option<TexId>,
    played: Option<String>,
    played_tex: Option<TexId>,
    /// The fanned discs' faces, one texture each, kept for the next game's.
    discs: Vec<Option<TexId>>,
    title_card: Option<String>,
    title_card_tex: Option<TexId>,
}

#[derive(Default)]
struct Switcher {
    open: bool,
    titled: Option<String>,
}

impl Frontend {
    pub fn boot(platform: Box<dyn Platform>) -> Self {
        let now = Instant::now();
        let cart_faces = CartFaces::spawn(platform.root().to_path_buf());
        #[cfg(feature = "brick")]
        let session_root = platform.root().to_path_buf();
        let mut session = Session::boot(platform.root().to_path_buf());
        label_cache::forget_old(platform.root());
        let stale = label_cache::stale(platform.root(), session.app().carts());
        crate::boot_times::mark(&format!("  label cache checked ({} new)", stale.len()));
        let rebuild = (!stale.is_empty()).then(|| {
            eprintln!("slot: label cache: {} new labels", stale.len());
            Rebuild::start(platform.root(), stale)
        });
        session
            .app_mut()
            .set_power(Power::new(platform, DOZE_TIMEOUT));
        crate::boot_times::mark("  power ready");
        Frontend {
            session,
            start: now,
            last: now,
            wake_guard: None,
            last_input: now,
            last_render: now,
            rendered_effects: (0.0,0.0,0.0,None),
            nav_fade: NavFade::default(),
            power_fade: PowerFade::default(),
            draws: Vec::new(),
            polaroid_texes: Vec::new(),
            title_tex: None,
            faces: FaceBuilder::spawn(),
            label_grid_tex: LabelGridTex::default(),
            label_adjust_tex: LabelAdjustTex::default(),
            peel_tex: PeelTex::default(),
            cart_faces,
            rebuild,
            rebuild_faces: RebuildFaces::default(),
            link_art: LinkArtBuilder::spawn(),
            link_art_done: false,
            core_asked: None,
            core_board_tex: None,
            core_lid_tex: None,
            core_built: None,
            undo_tex: None,
            switcher: Switcher::default(),
            clocks: Clocks::default(),
            about: AboutFace::default(),
            quick_clock: QuickClock::default(),
            quick_menu_scale: [1.0, 1.0],
            music_view: crate::music_view::MusicView::default(),
            overlay_tex: OverlayTex::default(),
            #[cfg(feature = "brick")]
            wifi_tex: None,
            banner_tex: None,
            #[cfg(feature = "brick")]
            auto_sleep: !platform_root_has(&session_root, "DISABLE_AUTO_SLEEP"),
            #[cfg(feature = "brick")]
            woke_untouched: false,
        }
    }

    fn upload_paper(compositor: &mut Compositor) {
        let mut dec = png::Decoder::new(std::io::Cursor::new(PAPER));
        dec.set_transformations(png::Transformations::normalize_to_color8());
        let Ok(mut reader) = dec.read_info() else {
            return;
        };
        let mut buf = vec![0u8; reader.output_buffer_size()];
        let Ok(info) = reader.next_frame(&mut buf) else {
            return;
        };
        if info.color_type != png::ColorType::Grayscale || info.width != info.height {
            return;
        }
        let rgba: Vec<u8> = buf[..info.buffer_size()]
            .iter()
            .flat_map(|&l| [l, l, l, 255])
            .collect();
        compositor.set_paper(info.width, &rgba);
    }

    /// Everything drawn at startup: the shelf's faces, then the menus'.
    pub fn upload_faces(&mut self, compositor: &mut Compositor) {
        self.upload_shelf_faces(compositor);
        self.upload_menu_faces(compositor);
    }

    /// What the shelf's first frame shows. The rest (upload_menu_faces) can follow it: no menu
    /// opens, and no toast or alert shows, before the next frame's input.
    pub fn upload_shelf_faces(&mut self, compositor: &mut Compositor) {
        Self::upload_paper(compositor);
        crate::boot_times::mark("  paper");
        #[cfg(feature = "brick")]
        slot_ui::upload_slot_art(slot_power::model() == slot_power::Model::BrickPro, |w, h, rgba| {
            compositor.create_texture(w, h, rgba)
        });
        crate::boot_times::mark("  slot art");
        if self.rebuild.is_none() {
            self.cart_faces.sync(self.session.app_mut(), compositor);
        }
        crate::boot_times::mark(&format!("  cart faces synced (label rebuild {})", if self.rebuild.is_some() { "running, skipped" } else { "none" }));
        let icons = Icon::ALL
            .iter()
            .map(|i| {
                let f = icon_face(*i, HUD_ICON_PX, HUD_INK);
                compositor.create_texture(f.w, f.h, &f.rgba)
            })
            .collect();
        self.session.app_mut().set_icon_faces(icons);
        crate::boot_times::mark("  icons");
        let legend = legend_faces(compositor, &LEGEND);
        self.session.app_mut().set_legend_faces(legend);
        let hint = set_clock_hint_face();
        self.clocks.hint = Some(compositor.create_texture(hint.w, hint.h, &hint.rgba));
        crate::boot_times::mark("  legends, clock hint");
        let shadow = cart_shadow();
        let id = compositor.create_texture(shadow.w, shadow.h, &shadow.rgba);
        self.session.app_mut().set_cart_shadow(id);
        let notched = gb_cart_shadow(GbShell::Notched);
        let notched = compositor.create_texture(notched.w, notched.h, &notched.rgba);
        let rounded = gb_cart_shadow(GbShell::Rounded);
        let rounded = compositor.create_texture(rounded.w, rounded.h, &rounded.rgba);
        self.session.app_mut().set_gb_cart_shadows(notched, rounded);
        let [nes, famicom] = [slot_ui::NesShell::Nes, slot_ui::NesShell::Famicom].map(|shell| {
            let face = slot_ui::nes_cart_shadow(shell);
            compositor.create_texture(face.w, face.h, &face.rgba)
        });
        self.session.app_mut().set_nes_cart_shadows(nes, famicom);
        let [us, pal] = [slot_ui::SnesShell::Us, slot_ui::SnesShell::Pal].map(|shell| {
            let face = slot_ui::snes_cart_shadow(shell);
            compositor.create_texture(face.w, face.h, &face.rgba)
        });
        self.session.app_mut().set_snes_cart_shadows(us, pal);
        let disc = slot_ui::disc_shadow();
        let disc = compositor.create_texture(disc.w, disc.h, &disc.rgba);
        let umd = slot_ui::umd_shadow();
        let umd = compositor.create_texture(umd.w, umd.h, &umd.rgba);
        self.session.app_mut().set_disc_shadows(disc, umd);
        let bolt = icon_face(Icon::Charging, BOLT_PX, SHELF_STATUS_INK);
        let bolt_id = compositor.create_texture(bolt.w, bolt.h, &bolt.rgba);
        self.session.app_mut().set_bolt_face(bolt_id);
        // Beside it, while a newer Slot is out.
        let update = icon_face(Icon::Update, slot_ui::WIFI_PX, SHELF_STATUS_INK);
        let update = (compositor.create_texture(update.w, update.h, &update.rgba), update.w, update.h);
        self.session.app_mut().set_update_face(update);
        // A favorite's star, gold with a lit side and a shaded one, outlined so it shows on any
        // plastic.
        let star = slot_ui::star_face(STAR_PX, STAR_INK, STAR_LIGHT, STAR_SHADE);
        let star = (compositor.create_texture(star.w, star.h, &star.rgba), star.w, star.h);
        self.session.app_mut().set_star_face(star);
        crate::boot_times::mark("  shadows and HUD icons");
        self.upload_wallpaper(compositor);
        crate::boot_times::mark("  wallpaper");
    }

    /// What no first frame shows: the power, quick, cart, core and link menus' faces, the
    /// shutdown lines, toasts, the link badges and the alert.
    pub fn upload_menu_faces(&mut self, compositor: &mut Compositor) {
        let link_badges = LinkBadge::FACES
            .iter()
            .map(|b| {
                let (badge, ink) = (
                    b.badge().expect("a face has a glyph"),
                    b.colour().expect("and a colour"),
                );
                let f = badge_face(badge, HUD_ICON_PX, ink);
                compositor.create_texture(f.w, f.h, &f.rgba)
            })
            .collect();
        self.session.app_mut().set_link_badge_faces(link_badges);
        let alert = icon_face(Icon::Alert, ALERT_PX, ALERT_INK);
        let alert = compositor.create_texture(alert.w, alert.h, &alert.rgba);
        self.session.app_mut().set_alert_face(alert);
        let toasts = Toast::all()
            .into_iter()
            .map(|t| {
                let f = toast_face(t);
                compositor.create_texture(f.w, f.h, &f.rgba)
            })
            .collect();
        self.session.app_mut().set_toast_faces(toasts);
        crate::boot_times::mark("  toasts, badges, alert");
        let lines = PowerChoice::ALL
            .iter()
            .map(|c| {
                let f = menu_face(match c {
                    PowerChoice::Restart => {
                        if cfg!(feature = "brick") {
                            "Saving and Exiting"
                        } else {
                            "Restarting"
                        }
                    }
                    PowerChoice::PowerOff => {
                        if cfg!(feature = "brick") {
                            "Saving and Shutting Down"
                        } else {
                            "Powering Down"
                        }
                    }
                });
                (compositor.create_texture(f.w, f.h, &f.rgba), f.w, f.h)
            })
            .collect();
        self.session.app_mut().set_shutdown_faces(lines);
        let menu = PowerChoice::ALL
            .iter()
            .map(|c| {
                let f = menu_face(c.text());
                (compositor.create_texture(f.w, f.h, &f.rgba), f.w, f.h)
            })
            .collect();
        self.session.app_mut().set_power_menu_faces(menu);
        let scale = if cfg!(feature = "brick") { compositor.ui_scale() } else { [1.0, 1.0] };
        let mut up = |(face, width): (UndoFace, u32)| {
            (compositor.create_texture(face.w, face.h, &face.rgba), width, QUICK_TEXT_H)
        };
        let pause = PauseChoice::ALL
            .iter()
            .map(|choice| [false, true].map(|lit| up(menu_row_face_scaled(choice.text(), lit, scale))))
            .collect();
        self.session.app_mut().set_pause_menu_faces(pause);
        let buttons = [false, true]
            .into_iter()
            .map(|on| up(menu_row_face_scaled(slot_ui::swap_ab_text(on), true, scale)))
            .collect();
        self.session.app_mut().set_pause_buttons_faces(buttons);
        self.quick_menu_scale = scale;
        let labels = QuickRow::ALL
            .iter()
            .map(|row| [false, true].map(|lit| up(menu_row_face_scaled(row.label(), lit, scale))))
            .collect();
        let values = QuickValue::ALL
            .iter()
            .map(|v| [false, true].map(|lit| up(quick_value_face_scaled(v.text(), lit, scale))))
            .collect();
        let (tex, width, _) = up(menu_hint_face_scaled("B", "back", scale));
        self.session.app_mut().set_quick_menu_faces(QuickMenuFaces {
            scale,
            labels,
            values,
            clock_back: (tex, width),
        });
        crate::boot_times::mark("  power and quick menus");
        self.upload_cart_menu_faces(compositor);
        crate::boot_times::mark("  cart menu");
        let sockets = slot_store::Core::PICKER
            .iter()
            .map(|c| {
                let f = socket_face(*c);
                compositor.create_texture(f.w, f.h, &f.rgba)
            })
            .collect();
        let chips = slot_store::Core::PICKER
            .iter()
            .map(|c| {
                let f = chip_face(Some(*c));
                compositor.create_texture(f.w, f.h, &f.rgba)
            })
            .collect();
        let blank = chip_face(None);
        let blank = compositor.create_texture(blank.w, blank.h, &blank.rgba);
        let shadow = chip_shadow_face();
        let shadow = compositor.create_texture(shadow.w, shadow.h, &shadow.rgba);
        self.session
            .app_mut()
            .set_core_part_faces(sockets, chips, blank, shadow);
        let legend = [
            hint_face("B", "Cancel"),
            arrows_hint_face("Core"),
            slot_ui::caret_hint_face(true, "Colour"),
            slot_ui::caret_hint_face(false, "Finish"),
            hint_face("A", "Choose"),
        ]
        .into_iter()
        .map(|f| (compositor.create_texture(f.w, f.h, &f.rgba), f.w))
        .collect();
        self.session.app_mut().set_core_legend_faces(legend);
        // Y sits on its own, above the legend rather than in it: it is about the cart's label
        // and not about the core being picked.
        let search = hint_face("Y", "Search");
        let face = (compositor.create_texture(search.w, search.h, &search.rgba), search.w);
        self.session.app_mut().set_core_search_face(face);
        crate::boot_times::mark("  core picker");
        let roles = menu_faces(compositor, LinkRow::ALL.iter().map(|r| r.text()));
        self.session.app_mut().set_link_menu_faces(roles);
        if let Some(linked) = menu_faces(compositor, ["Linked"].into_iter()).pop() {
            self.session.app_mut().set_link_linked_face(linked);
        }
        let legend = [
            hint_face("B", "Cancel"),
            hint_face("SELECT", "Mode"),
            arrows_hint_face("Swap"),
            hint_face("A", "Link"),
            hint_face("A", "OK"),
            hint_face("B", "Back"),
            hint_face("A", "End Link"),
        ]
        .into_iter()
        .map(|f| (compositor.create_texture(f.w, f.h, &f.rgba), f.w))
        .collect();
        self.session.app_mut().set_link_legend_faces(legend);
        let steps = menu_faces(compositor, LinkStep::ALL.iter().map(|s| s.line()));
        self.session.app_mut().set_link_step_faces(steps);
        let fails = menu_faces(compositor, LinkFail::SHOWN.iter().map(|f| f.line()));
        self.session.app_mut().set_link_fail_faces(fails);
        crate::boot_times::mark("  link menus");
    }

    /// The cart menu's words, each value dim and lit: built once, as the quick menu's are.
    fn upload_cart_menu_faces(&mut self, compositor: &mut Compositor) {
        use slot_ui::{cart_menu_caret_face, cart_menu_label_face, cart_menu_value_face, CartRow};
        let mut up = |f: UndoFace| (compositor.create_texture(f.w, f.h, &f.rgba), f.w, f.h);
        let labels = CartRow::ALL.iter()
            .map(|r| [false, true].map(|selected| up(cart_menu_label_face(*r, selected))))
            .collect();
        let mut values = |names: Vec<&str>| -> Vec<[(TexId, u32, u32); 2]> {
            names
                .into_iter()
                .map(|n| [false, true].map(|lit| up(cart_menu_value_face(n, lit))))
                .collect()
        };
        let mut colour_names: Vec<&str> = slot_ui::colour_choices().iter().map(|(n, _)| *n).collect();
        // A colour picked elsewhere (Cart Studio) shows as Custom, after all the named ones.
        colour_names.push("Custom");
        let colours = values(colour_names);
        let finishes = values(slot_ui::FINISHES.iter().map(|f| slot_ui::finish_name(*f)).collect());
        let shapes = values(
            slot_ui::SHAPES.iter().chain(&slot_ui::SNES_SHAPES).chain(&slot_ui::NES_SHAPES).copied().collect(),
        );
        let carets = [false, true].map(|right| up(cart_menu_caret_face(right)));
        let legend = slot_ui::cart_menu_legend_faces().map(|f| {
            let (tex, w, _) = up(f);
            (tex, w)
        });
        self.session.app_mut().set_cart_menu_faces(slot_ui::CartMenuFaces {
            labels,
            colours,
            finishes,
            shapes,
            carets,
            legend,
        });
    }

    fn upload_wallpaper(&mut self, compositor: &mut Compositor) {
        let app = self.session.app();
        let seed = app.wall_secs().unsigned_abs();
        let Some(rgba) = app
            .root()
            .and_then(|root| wallpaper::pick(root, seed))
            .and_then(|path| wallpaper_face(&path))
        else {
            return;
        };
        let id = compositor.create_texture(OUT_W, OUT_H, &rgba);
        self.session.app_mut().set_wallpaper(id);
    }

    pub fn upload_bezel(&mut self, compositor: &mut Compositor, panel: (u32, u32)) {
        if !slot_gfx::framed(panel) {
            return;
        }
        slot_ui::set_shelf_slack((OUT_W * panel.1 / panel.0).saturating_sub(OUT_H) as f32);
        let Some(path) = self
            .session
            .app()
            .root()
            .and_then(|root| crate::bezel::pick(root, panel))
        else {
            return;
        };
        if let Some(rgba) = bezel_face(&path, panel.0, panel.1) {
            compositor.set_bezel(panel.0, panel.1, &rgba);
            eprintln!("slot: bezel {}", path.display());
        }
    }

    #[cfg(feature = "brick")]
    pub fn power_mode(&self) -> crate::brick_power::Mode {
        use crate::brick_power::Mode;
        if matches!(self.app().phase(),Phase::Doze{..}) {return Mode::Standby}
        if self.rebuild.is_some() || self.app().link_active() || self.session.actually_fast_forwarding() || self.session.actually_rewinding() {return Mode::Boost}
        match self.app().phase() {
            Phase::Playing{..} if self.session.observed_speed()==Some(crate::emu::Speed::Normal)=>Mode::Game,
            Phase::Shelf|Phase::QuickMenu{..}|Phase::Music if self.last_input.elapsed()>Duration::from_secs(2)=>Mode::Menu,
            _=>Mode::Boost,
        }
    }
    #[cfg(feature = "brick")]
    pub fn audio_underruns(&self)->u64 {self.session.audio_ring().underruns()}
    #[cfg(feature = "brick")]
    pub fn needs_render(&self)->bool {
        if self.power_fade.wake_active(Instant::now())
            || self.power_fade.shutdown_active(Instant::now()) {return true}
        // Cover Flow's 260ms animation must not rely on the idle 1s refresh;
        // stop requesting extra frames as soon as the movement completes.
        if matches!(self.app().phase(), Phase::Music)
            && self.music_view.cover_flow_animating() {return true}
        // Completed background decodes are polled every 100ms until the
        // 15-cover prefetch window is warm, then the ordinary idle redraw
        // gate takes over. Otherwise artwork may pop in only on a key press.
        if matches!(self.app().phase(), Phase::Music)
            && (self.music_view.cover_flow_loading()
                || self.music_view.album_list_loading())
            && self.last_render.elapsed() >= Duration::from_millis(100) {return true}
        // Only titles too long for the Now Playing column need a modest
        // 12.5fps redraw. Static titles and other tabs keep the idle gate.
        if matches!(self.app().phase(), Phase::Music)
            && self.music_view.title_marquee_active()
            && self.last_render.elapsed() >= Duration::from_millis(80) {return true}
        // Before the first frame, no transition has actually been presented.
        // The normal initial frame is already requested by the boot/input path;
        // don't defeat the idle redraw gate just because scene is still None.
        if (self.nav_fade.presented
            && self.nav_fade.scene != NavScene::from_phase(self.app().phase()))
            || self.nav_fade.active(Instant::now()) {return true}
        if self.app().wifi.as_ref().map(|w|w.dirty).unwrap_or(false) {return true}
        if self.app().banner_active() {return true}
        if self.render_effects()!=self.rendered_effects {return true}
        if self.rebuild.is_some() || !matches!(self.app().phase(),Phase::Shelf|Phase::QuickMenu{..}|Phase::Music) || self.last_input.elapsed()<Duration::from_secs(2) || self.last_render.elapsed()>=Duration::from_secs(1) {return true}
        let mut probe=Vec::new();self.app().draw(&mut probe);
        probe!=self.draws
    }

    fn render_effects(&self)->(f32,f32,f32,Option<f32>) {
        (self.app().screen_power(),self.app().screen_shake(),self.app().frame_lift(),self.app().frame_split())
    }
    pub fn render(&mut self, compositor: &mut Compositor, window: (u32, u32)) {
        self.last_render=Instant::now();
        self.rendered_effects=self.render_effects();
        compositor.fit(window);
        self.compose(compositor);
        compositor.end_frame(window);
    }

    pub fn compose(&mut self, compositor: &mut Compositor) {
        compositor.set_blue_light(self.session.app().blue_light());
        compositor.set_picture(self.session.app().picture_rect());
        compositor.set_screen_effect(match self.session.app().screen_shader() {
            slot_store::Shader::Off => ScreenEffect::None,
            slot_store::Shader::Lcd3x => ScreenEffect::Lcd3x,
            slot_store::Shader::Grid => ScreenEffect::Grid,
            slot_store::Shader::Dot => ScreenEffect::Dot,
            slot_store::Shader::Simpletex => ScreenEffect::Simpletex,
        });
        compositor.set_shake(self.session.app().screen_shake());
        compositor.set_screen_power(self.session.app().screen_power());
        compositor.set_frame_lift(self.session.app().frame_lift());
        compositor.set_frame_split(self.session.app().frame_split());
        let (cw, ch) = self.session.app().canvas();
        compositor.set_game_canvas(cw, ch);
        compositor.set_game_source_rect(self.session.app().source_rect());
        compositor.begin_frame();
        if self.draw_rebuild(compositor) {
            return;
        }
        // Start the fade only once the destination is actually being rendered,
        // so an initial library rebuild cannot consume the boot animation.
        let scene = NavScene::from_phase(self.session.app().phase());
        let now = Instant::now();
        // Begin at the very first rendered frame, irrespective of initial phase.
        // A saved cartridge can start in Inserting rather than Shelf; waiting
        // for Shelf there meant the boot fade never played on the real device.
        self.nav_fade.begin_first_frame(scene, now);
        self.nav_fade.update(scene, now);
        if self.session.app().shutting_down()
            && !matches!(self.session.app().phase(), Phase::Doze { .. }) {
            self.power_fade.begin_shutdown(Instant::now(), &self.draws);
        }
        if let Some(frame) = self.session.frame() {
            compositor.upload_game(&frame);
            crate::latency::taken();
        }
        self.cart_faces.sync(self.session.app_mut(), compositor);
        #[cfg(feature = "brick")]
        if let Some(wifi)=&mut self.session.app_mut().wifi {
            if wifi.dirty {
                let f=wifi.render();
                let tex=match self.wifi_tex {Some(id)=>{compositor.update_texture(id,f.w,f.h,&f.rgba);id},None=>compositor.create_texture(f.w,f.h,&f.rgba)};
                self.wifi_tex=Some(tex);wifi.face=Some(tex);wifi.dirty=false;
            }
        }
        if let Some((id, title, detail, badge)) = self.session.app().banner_to_render() {
            let px = slot_ui::BADGE_PX;
            let badge = badge.and_then(|p| slot_ui::cover(&p, px, px));
            let f = slot_ui::banner_face(&title, &detail, badge.as_deref());
            let tex = match self.banner_tex {
                Some(t) => {
                    compositor.update_texture(t, f.w, f.h, &f.rgba);
                    t
                }
                None => compositor.create_texture(f.w, f.h, &f.rgba),
            };
            self.banner_tex = Some(tex);
            self.session.app_mut().set_banner_face(id, tex);
        }
        sync_clock(self.session.app_mut(), compositor, &mut self.clocks);
        sync_disc_faces(self.session.app_mut(), compositor, &mut self.clocks);
        sync_about(self.session.app_mut(), compositor, &mut self.about);
        sync_quick_clock(self.session.app_mut(), compositor, &mut self.quick_clock, self.quick_menu_scale);
        if matches!(self.session.app().phase(), Phase::Music) {
            if let Some(music) = self.session.app().music() {
                let draws = self.music_view.render(music, compositor, self.quick_menu_scale);
                let lock_icon = self.music_view.hud_lock_icon();
                self.session.app_mut().set_music_draws(draws);
                self.session.app_mut().set_music_lock_face(lock_icon);
            }
        }
        sync_named_texts(self.session.app_mut(), compositor, &mut self.overlay_tex, self.quick_menu_scale);
        sync_overlay(self.session.app_mut(), compositor, &mut self.overlay_tex);
        sync_label_grid(self.session.app_mut(), compositor, &mut self.label_grid_tex);
        sync_label_adjust(self.session.app_mut(), compositor, &mut self.label_adjust_tex);
        sync_peel(self.session.app_mut(), compositor, &mut self.peel_tex);
        sync_core_picker(
            self.session.app_mut(),
            compositor,
            &self.faces,
            &mut self.core_asked,
            &mut self.core_board_tex,
            &mut self.core_lid_tex,
            &mut self.core_built,
        );
        if !self.link_art_done {
            if let Some(art) = self.link_art.take() {
                let mut up = |f: &slot_ui::CartFace| Sprite {
                    tex: compositor.create_texture(f.w, f.h, &f.rgba),
                    w: f.w,
                    h: f.h,
                };
                let sprites = LinkSprites {
                    port: up(&art.port),
                    plug_host: up(&art.plug_host),
                    plug_join: up(&art.plug_join),
                    adapter: up(&art.adapter),
                    arcs_right: [
                        up(&art.arcs_right[0]),
                        up(&art.arcs_right[1]),
                        up(&art.arcs_right[2]),
                    ],
                    arcs_left: [
                        up(&art.arcs_left[0]),
                        up(&art.arcs_left[1]),
                        up(&art.arcs_left[2]),
                    ],
                    clicks: up(&art.clicks),
                    arrow_left: up(&art.arrow_left),
                    arrow_right: up(&art.arrow_right),
                };
                self.session.app_mut().set_link_sprites(sprites);
                self.link_art_done = true;
            }
        }
        sync_switcher(
            self.session.app_mut(),
            compositor,
            Faces {
                pool: &mut self.polaroid_texes,
                title: &mut self.title_tex,
                undo: &mut self.undo_tex,
            },
            &mut self.switcher,
        );
        self.draws.clear();
        self.session.app().draw(&mut self.draws);
        let now = Instant::now();
        if matches!(self.session.app().phase(), Phase::Doze { .. }) {
            if let Some((start, frozen)) = &self.power_fade.sleep {
                self.draws.clone_from(frozen);
                self.draws.push(Draw::Rect {
                    x: 0.0, y: 0.0,
                    w: OUT_W as f32, h: OUT_H as f32,
                    colour: [0.0, 0.0, 0.0, PowerFade::progress(now, *start, SLEEP_FADE)],
                });
            }
        } else if let Some((start, frozen)) = &self.power_fade.shutdown {
            // Crossfade the previous screen into the existing shutdown label.
            // The app's opaque shutdown background must not appear abruptly.
            let progress = PowerFade::progress(now, *start, SHUTDOWN_FADE);
            let labels: Vec<Draw> = self.draws.iter().filter_map(|draw| {
                if let Draw::Tex { x, y, w, h, tex, alpha } = *draw {
                    Some(Draw::Tex { x, y, w, h, tex, alpha: alpha * progress })
                } else {
                    None
                }
            }).collect();
            self.draws.clone_from(frozen);
            self.draws.push(Draw::Rect {
                x: 0.0, y: 0.0, w: OUT_W as f32, h: OUT_H as f32,
                colour: [0.0, 0.0, 0.0, progress],
            });
            self.draws.extend(labels);
        } else {
            let opacity = self.nav_fade.opacity(now).max(self.power_fade.wake_opacity(now));
            if opacity > 0.0 {
                self.draws.push(Draw::Rect {
                    x: 0.0,
                    y: 0.0,
                    w: OUT_W as f32,
                    h: OUT_H as f32,
                    colour: [0.0, 0.0, 0.0, opacity],
                });
            }
        }
        compositor.draw_list(&self.draws);
    }

    pub fn drive_emulator(&mut self) {
        self.session.set_driven(true);
    }

    pub fn step_emulator(&self, present: Duration, timeout: Duration) -> bool {
        self.session.step_emulator(present, timeout)
    }

    pub fn rebuilding_labels(&self) -> bool {
        self.rebuild.is_some()
    }

    fn draw_rebuild(&mut self, compositor: &mut Compositor) -> bool {
        let Some(rebuild) = &self.rebuild else {
            return false;
        };
        if rebuild.finished() {
            self.rebuild = None;
            self.last = Instant::now();
            return false;
        }
        let faces = &mut self.rebuild_faces;
        let title = *faces.title.get_or_insert_with(|| {
            let f = rebuild_title_face("Caching New Labels");
            (compositor.create_texture(f.w, f.h, &f.rgba), f.w, f.h)
        });
        let done = rebuild.done();
        if faces.shown != Some(done) {
            let f = rebuild_count_face(&format!("{done} / {}", rebuild.total()));
            faces.count_size = (f.w, f.h);
            upload(compositor, &mut faces.count, f);
            faces.shown = Some(done);
        }
        let screen = RebuildScreen {
            title: Some(title),
            count: faces
                .count
                .map(|tex| (tex, faces.count_size.0, faces.count_size.1)),
            fraction: done as f32 / rebuild.total().max(1) as f32,
        };
        self.draws.clear();
        screen.draw(&mut self.draws);
        compositor.draw_list(&self.draws);
        true
    }

    pub fn advance(&mut self, input: &mut dyn InputSource) {
        let now = self.now();
        input.set_stick_to_game(self.session.stick_to_game());
        let mut events = input.poll(now);
        self.session.set_stick(input.stick());
        if !events.is_empty() {
            self.last_input=Instant::now();
            #[cfg(feature = "brick")]
            {self.woke_untouched=false;}
        }
        if self.wake_guard.is_some_and(|until| Instant::now() < until) {
            events.retain(|e| !matches!(e, slot_input::RawEvent::Down(slot_input::Btn::Power) | slot_input::RawEvent::Up(slot_input::Btn::Power)));
        }
        if self.rebuild.is_some() {
            return;
        }
        let was_doze = matches!(self.session.app().phase(), Phase::Doze { .. });
        self.session.feed(events, now);
        let dt = self.last.elapsed().as_secs_f32();
        self.last = Instant::now();
        self.session.update(dt);
        #[cfg(feature = "brick")]
        self.sync_music_dim();
        // A second Power press may wake during the pre-suspend preparation.
        // That path does not go through resume_sleep().
        if was_doze && !matches!(self.session.app().phase(), Phase::Doze { .. })
            && !self.session.app().shutting_down() {
            self.power_fade.resume(Instant::now());
        }
    }

    /// Optional Music-only inactivity dim: preserve the user's actual
    /// brightness, never turn the screen fully off, and avoid brightening
    /// during the sleep-to-black animation.
    #[cfg(feature = "brick")]
    fn sync_music_dim(&mut self) {
        if matches!(self.app().phase(), Phase::Doze { .. }) {
            return;
        }
        let dim_level = if matches!(self.app().phase(), Phase::Music) {
            self.app().music().and_then(|music| {
                music_dim_target(music, self.last_input.elapsed())
            })
        } else {
            None
        };
        self.session.app_mut().set_music_dimmed(dim_level);
    }

    /// Called from the Brick's Doze loop to draw a short fade even though
    /// normal frames are suppressed while waiting for kernel suspend.
    #[cfg(feature = "brick")]
    pub fn sleep_fade_frame(&mut self) -> bool {
        if !matches!(self.app().phase(), Phase::Doze { .. }) {
            return false;
        }
        let now = Instant::now();
        self.power_fade.begin_sleep(now, &self.draws);
        let needs_frame = self.power_fade.sleep_frame_needed(now);
        if !needs_frame {
            // The device's backlight has stayed on through the 300ms fade,
            // including the final black frame swapped by device_app. Only
            // now is it safe to turn off the physical LCD illumination.
            self.session.app_mut().finish_doze_fade();
        }
        needs_frame
    }

    #[cfg(feature = "brick")]
    pub fn prepare_sleep(&mut self) -> bool {
        self.session.app_mut().settle_saves();
        self.session.observed_speed().is_none_or(|s| s == crate::emu::Speed::Paused)
    }

    #[cfg(feature = "brick")]
    pub fn resume_sleep(&mut self, input: &mut dyn InputSource) {
        let _ = input.poll(self.now()); // discard the kernel's queued wake key
        self.session.reset_sleep_input();
        self.session.app_mut().wake();
        self.session.reopen_music_audio();
        self.power_fade.resume(Instant::now());
        self.last = Instant::now();
        self.wake_guard = Some(self.last + Duration::from_millis(750));
        self.last_input = self.last;
        self.woke_untouched = true;
        self.session.update(0.0);
    }

    /// Doze after a stretch without input; device_app then suspends as for a power press.
    #[cfg(feature = "brick")]
    pub fn idle_sleep(&mut self) {
        if !self.auto_sleep || self.rebuild.is_some() || self.app().link_active() {return}
        let limit = match self.app().phase() {
            Phase::Shelf | Phase::QuickMenu { .. } => IDLE_MENU,
            // Locked Music deliberately stays in its dimmed, non-suspended
            // state even when paused; the hardware power button still sleeps.
            Phase::Music if !self.app().music().is_some_and(|music|
                music.playing() || music.locked()) => IDLE_MENU,
            Phase::Playing { .. } => IDLE_GAME,
            _ => return,
        };
        let limit = if self.woke_untouched {limit.min(IDLE_AFTER_WAKE)} else {limit};
        if self.last_input.elapsed() >= limit {
            eprintln!("slot: idle for {}s; sleeping", self.last_input.elapsed().as_secs());
            self.woke_untouched = false;
            self.session.app_mut().idle_doze();
        }
    }

    pub fn app(&self) -> &crate::app::App {
        self.session.app()
    }

    pub fn core_settling(&self) -> bool {
        self.session.core_settling()
    }

    pub fn advance_at(&mut self, input: &mut dyn InputSource, now: Millis, dt: f32) {
        let events = input.poll(now);
        self.session.feed(events, now);
        self.session.update(dt);
    }

    pub fn now(&self) -> Millis {
        self.start.elapsed().as_millis() as Millis
    }

    pub fn powering_off(&self) -> bool {
        self.session.app().ready_to_power_off()
    }

    pub fn restarting(&self) -> bool {
        self.session.app().ready_to_restart()
    }

    pub fn restart(&mut self) {
        self.session.app_mut().restart();
    }

    #[cfg(feature = "brick")]
    pub fn exit_to_launcher(&mut self) {
        self.session.app_mut().settle_saves();
        self.session.app().restore_brick_leds();
        let _ = std::process::Command::new("sync").status();
        std::process::exit(0);
    }

    pub fn poweroff(&mut self) {
        self.session.app_mut().poweroff();
    }

    pub fn updating(&self) -> bool {
        self.session.app().updating()
    }

    /// An update is swapped in: saves settled, Slot ends for launch.sh to start the new one.
    #[cfg(feature = "brick")]
    pub fn exit_for_update(&mut self) {
        self.session.app_mut().settle_saves();
        self.session.app().restore_brick_leds();
        let _ = std::process::Command::new("sync").status();
        std::process::exit(crate::update::EXIT_UPDATED);
    }
}

fn menu_faces<'a>(
    compositor: &mut Compositor,
    labels: impl Iterator<Item = &'a str>,
) -> Vec<(TexId, u32, u32)> {
    labels
        .map(|label| {
            let f = menu_face(label);
            (compositor.create_texture(f.w, f.h, &f.rgba), f.w, f.h)
        })
        .collect()
}

fn legend_faces(compositor: &mut Compositor, legend: &[(&str, &str)]) -> Vec<TexId> {
    legend
        .iter()
        .map(|(key, label)| {
            let f = hint_face(key, label);
            compositor.create_texture(f.w, f.h, &f.rgba)
        })
        .collect()
}

struct Faces<'a> {
    pool: &'a mut Vec<TexId>,
    title: &'a mut Option<TexId>,
    undo: &'a mut Option<TexId>,
}

fn sync_switcher(app: &mut App, compositor: &mut Compositor, texes: Faces, state: &mut Switcher) {
    if !matches!(app.phase(), Phase::Polaroids { .. }) {
        state.open = false;
        return;
    }
    if !state.open {
        state.open = true;
        state.titled = None;
        let faces: Vec<_> = app.polaroid_entries().iter().map(photo_face).collect();
        let ids = faces
            .iter()
            .enumerate()
            .map(|(i, f)| match texes.pool.get(i) {
                Some(id) => {
                    compositor.update_texture(*id, f.w, f.h, &f.rgba);
                    *id
                }
                None => {
                    let id = compositor.create_texture_nearest(f.w, f.h, &f.rgba);
                    texes.pool.push(id);
                    id
                }
            })
            .collect();
        app.set_polaroid_faces(ids);

        let label = app
            .undo_label()
            .map(|l| upload(compositor, texes.undo, hint_face("X", l)));
        app.set_undo_face(label);
    }
    if state.titled.as_deref() != app.polaroid_stamp() {
        state.titled = app.polaroid_stamp().map(str::to_string);
        let face = title_face(&app.polaroid_title(&format_stamp(app.wall_secs())));
        let id = upload(compositor, texes.title, face);
        app.set_polaroid_title_face(id);
    }
}

fn sync_clock(app: &mut App, compositor: &mut Compositor, clocks: &mut Clocks) {
    let scale = if cfg!(feature = "brick") { compositor.ui_scale() } else { [1.0, 1.0] };
    let scale_changed = scale != clocks.status_scale;
    if scale_changed {
        clocks.status_scale = scale;
        app.set_status_scale(scale);
        let wifi = StatusIcon::Wifi.face(scale);
        let wifi_id = upload_rgba(compositor, &mut clocks.wifi_tex, wifi.w, wifi.h, &wifi.rgba);
        let [w, h] = StatusIcon::Wifi.size();
        app.set_wifi_face((wifi_id, w, h));
        let bolt = StatusIcon::Charging.face(scale);
        let bolt_id =
            upload_rgba(compositor, &mut clocks.status_bolt_tex, bolt.w, bolt.h, &bolt.rgba);
        app.set_status_bolt_face(bolt_id);
    }
    let picked = app.picker().map(|p| p.text());
    if picked != clocks.picked {
        clocks.picked = picked;
        if let (Some(face), Some(hint)) = (app.picker().map(|p| p.face()), clocks.hint) {
            let line = upload(compositor, &mut clocks.line, face);
            app.set_clock_faces(line, hint);
        }
    }
    let shown = hhmm(app.wall_secs());
    if shown != clocks.shown || scale_changed {
        let (face, w) = word_face_ink_scaled(&shown, SHELF_STATUS_INK, scale);
        clocks.shown = shown;
        let id = upload(compositor, &mut clocks.shelf, face);
        app.set_shelf_clock_face(id, w);
    }
    let platform_shown = app.slot_text().unwrap_or_default();
    if platform_shown != clocks.platform {
        clocks.platform = platform_shown.clone();
        if platform_shown.is_empty() {
            app.clear_shelf_platform();
        } else {
            let face = word_face(&platform_shown);
            let w = face.w;
            let id = upload(compositor, &mut clocks.platform_tex, face);
            app.set_shelf_platform_face(id, w);
        }
    }
    let played = app.played_text();
    if played != clocks.played {
        clocks.played = played.clone();
        let face = played.map(|text| {
            let face = word_face(&text);
            let w = face.w;
            (upload(compositor, &mut clocks.played_tex, face), w)
        });
        app.set_played_face(face);
    }
    let title_card = app.title_card_text();
    if title_card != clocks.title_card {
        clocks.title_card = title_card.clone();
        let face = title_card.map(|text| slot_ui::title_card_face(&text)).filter(|f| f.w > 0).map(|face| {
            let (w, h) = (face.w, face.h);
            (upload(compositor, &mut clocks.title_card_tex, face), w, h)
        });
        app.set_title_card_face(face);
    }
    let battery_shown = app
        .battery()
        .map(|b| format!("{}%", b.percent))
        .unwrap_or_default();
    if battery_shown != clocks.battery || scale_changed {
        clocks.battery = battery_shown.clone();
        if !battery_shown.is_empty() {
            let (face, w) = word_face_ink_scaled(&battery_shown, SHELF_STATUS_INK, scale);
            let id = upload(compositor, &mut clocks.battery_tex, face);
            app.set_battery_percent_face(id, w);
        }
    }
}

/// A game's discs fanned out: each one's face, built here as it is asked for.
fn sync_disc_faces(app: &mut App, compositor: &mut Compositor, clocks: &mut Clocks) {
    let Some(discs) = app.disc_faces_wanted() else {
        return;
    };
    let root = app.root().map(|r| r.to_path_buf());
    if clocks.discs.len() < discs.len() {
        clocks.discs.resize(discs.len(), None);
    }
    let ids = discs
        .iter()
        .zip(clocks.discs.iter_mut())
        .map(|((cart, number), slot)| {
            let art = root.as_deref().and_then(|r| crate::label_cache::label_art(r, cart));
            let face = slot_ui::disc_face(cart, art, Some(*number));
            upload_rgba(compositor, slot, face.w, face.h, &face.rgba)
        })
        .collect();
    app.set_disc_faces(ids);
}

fn sync_quick_clock(app: &mut App, compositor: &mut Compositor, state: &mut QuickClock, scale: [f32; 2]) {
    if app.quick_menu().is_none() {
        return;
    }
    let text = date_time_text(app.wall_secs());
    if text == state.shown {
        return;
    }
    let (dim, dim_w) = quick_value_face_scaled(&text, false, scale);
    let (lit, lit_w) = quick_value_face_scaled(&text, true, scale);
    let dim = upload(compositor, &mut state.dim, dim);
    let lit = upload(compositor, &mut state.lit, lit);
    app.set_quick_clock_faces((dim, dim_w, QUICK_TEXT_H), (lit, lit_w, QUICK_TEXT_H));
    state.shown = text;
}

fn sync_about(app: &mut App, compositor: &mut Compositor, state: &mut AboutFace) {
    if !matches!(app.phase(), Phase::About) {
        return;
    }
    let battery = app.battery().map(|b| b.percent);
    if state.tex.is_some() && state.battery == battery {
        return;
    }
    state.battery = battery;
    let build = Build::current();
    let face = sticker_face(&StickerFields {
        battery,
        serial: &build.serial(),
        dirty_digit: build.dirty_digit(),
    });
    let id = upload(compositor, &mut state.tex, face);
    app.set_sticker_face(id);
}

#[derive(Default)]
struct LabelAdjustTex {
    /// What was last drawn, so a frame that has not moved does no work at all.
    shown: Option<crate::app::AdjustKey>,
    picture: Option<TexId>,
    cart: Option<TexId>,
    caption: Option<TexId>,
    legend: [Option<TexId>; 5],
}

/// The picture being framed by hand, and the cart it is being framed for. Both are read and
/// uploaded once, when the framing screen opens; after that only the words change.
fn sync_label_adjust(app: &mut App, compositor: &mut Compositor, tex: &mut LabelAdjustTex) {
    let key = app.label_adjust_key();
    if key.is_none() || (tex.shown == key && app.label_adjust_built().is_some()) {
        return;
    }
    tex.shown = key;
    let Some((source, words, cart)) = app.label_adjust_view() else {
        return;
    };
    if app.label_adjust_built() == Some((source.as_path(), words.as_str())) {
        return;
    }
    let same_picture = app.label_adjust_built().is_some_and(|(built, _)| built == source);
    let caption = face(compositor, &mut tex.caption, slot_ui::caption_face(&words));
    if same_picture {
        // Only the frame moved: the picture and the cart are the ones already uploaded.
        if let (Some(picture), Some((legend, cart))) = (tex.picture, app.label_adjust_legend()) {
            app.set_label_adjust_faces((source, words), picture, cart, caption, legend);
        }
        return;
    }
    let Some((rgba, w, h)) =
        std::fs::read(&source).ok().and_then(|png| crate::cart_art::label_picture(&png, ADJUST_TEX_MAX))
    else {
        return;
    };
    let picture = upload_rgba(compositor, &mut tex.picture, w, h, &rgba);
    let (face_rgba, _) = slot_ui::cart_cutout_face(&cart);
    let cart_tex = upload_rgba(compositor, &mut tex.cart, face_rgba.w, face_rgba.h, &face_rgba.rgba);
    let mut legend = [(picture, 0u32); 5];
    for (i, hint) in slot_ui::label_adjust_legend_faces().into_iter().enumerate() {
        let (id, lw, _) = face(compositor, &mut tex.legend[i], hint);
        legend[i] = (id, lw);
    }
    app.set_label_adjust_faces((source, words), picture, cart_tex, caption, legend);
}

/// The sticker's curls and its shadow: the same textures every time, written over.
#[derive(Default)]
struct PeelTex {
    curls: Vec<Option<TexId>>,
    shadow: Option<TexId>,
    /// Curls baked but not yet uploaded, and how many of them have gone up.
    pending: Vec<slot_ui::CartFace>,
    sent: usize,
    shadow_id: Option<TexId>,
}

/// How many curls go up to the GPU a frame: a few at a time, so no one frame stalls on them all.
const PEEL_UPLOADS: usize = 8;

/// A label just kept: its curls, baked off the UI thread, go up a few a frame, flat first; the
/// first of them sets it off.
fn sync_peel(app: &mut App, compositor: &mut Compositor, tex: &mut PeelTex) {
    if let Some((curls, shadow)) = app.peel_unbaked() {
        tex.shadow_id = Some(upload_rgba(compositor, &mut tex.shadow, shadow.w, shadow.h, &shadow.rgba));
        tex.curls.resize(curls.len().max(tex.curls.len()), None);
        (tex.pending, tex.sent) = (curls, 0);
    }
    let (Some(shadow), true) = (tex.shadow_id, tex.sent < tex.pending.len()) else {
        return;
    };
    let upto = (tex.sent + PEEL_UPLOADS).min(tex.pending.len());
    for i in tex.sent..upto {
        let c = &tex.pending[i];
        upload_rgba(compositor, &mut tex.curls[i], c.w, c.h, &c.rgba);
    }
    tex.sent = upto;
    let ids = (0..tex.pending.len()).map(|i| if i < upto { tex.curls[i] } else { None }).collect();
    app.set_peel_faces(ids, shadow);
    if upto == tex.pending.len() {
        // All up: the pictures themselves are no longer needed.
        (tex.pending, tex.sent) = (Vec::new(), 0);
    }
}

/// How large the picture being framed is kept: enough to zoom into, and no larger.
const ADJUST_TEX_MAX: u32 = 1024;

#[derive(Default)]
struct LabelGridTex {
    shown: Option<crate::app::GridKey>,
    title: Option<TexId>,
    caption: Option<TexId>,
    legend: [Option<TexId>; 3],
    cells: Vec<Option<TexId>>,
}

/// The label grid's page: its labels as pictures, its heading and its words. The pictures are
/// read again only when the page turns or another label lands on it; the words, which change
/// as the cursor moves and the search runs, are drawn on their own.
fn sync_label_grid(app: &mut App, compositor: &mut Compositor, tex: &mut LabelGridTex) {
    // Worked out without allocating; what follows is not.
    let key = app.label_grid_key();
    if key.is_none() || (tex.shown == key && app.label_grid_built().is_some()) {
        return;
    }
    tex.shown = key;
    let Some((page, paths, words)) = app.label_grid_page() else {
        return;
    };
    let fresh = app.label_grid_built() == Some((page, paths.len()));
    if fresh {
        if app.label_grid_caption_built() != Some(words.as_str()) {
            let caption = face(compositor, &mut tex.caption, slot_ui::caption_face(&words));
            app.set_label_grid_caption(words, caption);
        }
        return;
    }
    let query = app.label_grid_query().unwrap_or_default();
    let heading =
        if query.is_empty() { "FIND A LABEL".to_string() } else { format!("FIND A LABEL \u{b7} {query}") };
    let title = face(compositor, &mut tex.title, slot_ui::title_face(&heading));
    let caption = face(compositor, &mut tex.caption, slot_ui::caption_face(&words));
    let mut legend = [(caption.0, 0u32); 3];
    for (i, hint) in slot_ui::label_grid_legend_faces().into_iter().enumerate() {
        let (id, w, _) = face(compositor, &mut tex.legend[i], hint);
        legend[i] = (id, w);
    }
    tex.cells.resize(slot_ui::PAGE, None);
    let cells = (0..slot_ui::PAGE)
        .map(|i| {
            let png = std::fs::read(paths.get(i)?).ok()?;
            let (rgba, w, h) = crate::cart_art::label_picture(&png, GRID_TEX_MAX)?;
            Some((upload_rgba(compositor, &mut tex.cells[i], w, h, &rgba), w, h))
        })
        .collect();
    app.set_label_grid_faces((page, paths.len()), words, title, caption, cells, legend);
}

/// Uploads a face and keeps its size, which is what the views are drawn from.
fn face(compositor: &mut Compositor, slot: &mut Option<TexId>, f: slot_ui::UndoFace) -> (TexId, u32, u32) {
    let (w, h) = (f.w.max(1), f.h.max(1));
    // No font: a clear pixel, so there is still a texture to point at.
    let rgba = if f.rgba.is_empty() { vec![0; 4] } else { f.rgba };
    let (w, h) = if rgba.len() as u32 == w * h * 4 { (w, h) } else { (1, 1) };
    (upload_rgba(compositor, slot, w, h, &rgba), w, h)
}

/// How big a label is kept for the grid: four to a row on a 640 wide screen.
const GRID_TEX_MAX: u32 = 160;

fn sync_core_picker(
    app: &mut App,
    compositor: &mut Compositor,
    builder: &FaceBuilder,
    asked: &mut Option<String>,
    board: &mut Option<TexId>,
    lid: &mut Option<TexId>,
    built: &mut Option<String>,
) {
    // Keyed by the cart and the plastic it wears, so a colour or finish changed in the picker
    // (or the cart menu) rebuilds the lid it shows.
    // A label written over in place keeps the key the same, so it says so itself.
    if app.take_core_faces_stale() {
        (*asked, *built) = (None, None);
    }
    let cart = app.selected_cart().cloned();
    let highlighted = cart.as_ref().map(crate::face_builder::face_key);
    if let Some(cart) = cart {
        if *asked != highlighted {
            builder.request(cart);
            *asked = highlighted.clone();
        }
    }
    let Some(faces) = builder.take() else {
        return;
    };
    if highlighted.as_deref() != Some(faces.key.as_str()) || *built == highlighted {
        return;
    }
    let board_id = upload_rgba(
        compositor,
        board,
        faces.board.w,
        faces.board.h,
        &faces.board.rgba,
    );
    let lid_id = upload_rgba(compositor, lid, faces.lid.w, faces.lid.h, &faces.lid.rgba);
    app.set_core_board_faces(board_id, lid_id);
    *built = Some(faces.key);
}

fn upload(compositor: &mut Compositor, slot: &mut Option<TexId>, face: slot_ui::UndoFace) -> TexId {
    upload_rgba(compositor, slot, face.w, face.h, &face.rgba)
}

fn upload_rgba(
    compositor: &mut Compositor,
    slot: &mut Option<TexId>,
    w: u32,
    h: u32,
    rgba: &[u8],
) -> TexId {
    match *slot {
        Some(id) => {
            compositor.update_texture(id, w, h, rgba);
            id
        }
        None => {
            let id = compositor.create_texture(w, h, rgba);
            *slot = Some(id);
            id
        }
    }
}

#[cfg(feature = "brick")]
fn platform_root_has(root: &std::path::Path, name: &str) -> bool {
    root.join(name).exists()
}

#[cfg(all(test,feature="brick"))]
mod power_render_tests {
    use super::*;
    fn booted(dir:&std::path::Path)->Frontend {
        crate::root::ensure(dir);
        let mut state=slot_store::SlotState::default();state.clock_set=true;
        slot_store::write_slot_state(dir,&state).unwrap();
        Frontend::boot(Box::new(slot_power::SimPlatform::at(dir.into())))
    }
    #[test]
    fn locked_music_dims_immediately_and_unlock_restores_idle_policy() {
        let dir = tempfile::tempdir().unwrap();
        crate::root::ensure(dir.path());
        let mut music = crate::music::Music::open(dir.path());
        assert_eq!(music_dim_target(&music, Duration::ZERO), None);
        music.toggle_lock();
        assert_eq!(music_dim_target(&music, Duration::ZERO), Some(1),
            "locked Music dims immediately, regardless of playback/idle state");
        assert_eq!(music_dim_target(&music, Duration::from_secs(3600)), Some(1));
        music.toggle_lock();
        assert_eq!(music_dim_target(&music, Duration::ZERO), None,
            "unlock restores normal brightness unless the idle policy applies");
    }

    #[test]
    fn idle_shelf_dozes_after_limit_only() {
        let dir=tempfile::tempdir().unwrap();let mut f=booted(dir.path());
        assert!(matches!(f.app().phase(),Phase::Shelf));
        f.last_input=Instant::now()-Duration::from_secs(4*60);f.idle_sleep();
        assert!(matches!(f.app().phase(),Phase::Shelf),"4 minutes is not yet idle");
        f.last_input=Instant::now()-Duration::from_secs(5*60+1);f.idle_sleep();
        assert!(matches!(f.app().phase(),Phase::Doze{..}));
    }
    #[test]
    fn untouched_wake_sleeps_again_quickly() {
        let dir=tempfile::tempdir().unwrap();let mut f=booted(dir.path());
        f.woke_untouched=true;
        f.last_input=Instant::now()-Duration::from_secs(60);f.idle_sleep();
        assert!(matches!(f.app().phase(),Phase::Shelf));
        f.last_input=Instant::now()-Duration::from_secs(91);f.idle_sleep();
        assert!(matches!(f.app().phase(),Phase::Doze{..}));
    }
    #[test]
    fn auto_sleep_can_be_disabled() {
        let dir=tempfile::tempdir().unwrap();crate::root::ensure(dir.path());
        std::fs::write(dir.path().join("DISABLE_AUTO_SLEEP"),"").unwrap();
        let mut f=booted(dir.path());
        f.last_input=Instant::now()-Duration::from_secs(3600);f.idle_sleep();
        assert!(matches!(f.app().phase(),Phase::Shelf));
    }
    #[test]
    fn idle_redraw_gate_preserves_input_effects_and_periodic_refresh() {
        let dir=tempfile::tempdir().unwrap();
        crate::root::ensure(dir.path());
        let mut state=slot_store::SlotState::default();state.clock_set=true;
        slot_store::write_slot_state(dir.path(),&state).unwrap();
        let mut f=Frontend::boot(Box::new(slot_power::SimPlatform::at(dir.path().into())));
        assert!(matches!(f.app().phase(),Phase::Shelf));
        f.last_input=Instant::now()-Duration::from_secs(3);
        f.last_render=Instant::now();f.rendered_effects=f.render_effects();
        let mut scene=vec![];f.app().draw(&mut scene);f.draws=scene;
        assert!(!f.needs_render(),"unchanged shelf should not redraw");
        // A completed boot fade should also allow the regular idle redraw gate.
        f.nav_fade.update(Some(NavScene::Shelf), Instant::now()-BOOT_FADE);
        assert!(!f.needs_render(),"finished boot fade should not keep redrawing");
        f.last_input=Instant::now();assert!(f.needs_render(),"input must redraw immediately");
        f.last_input=Instant::now()-Duration::from_secs(3);
        f.rendered_effects.0=-1.0;assert!(f.needs_render(),"screen transitions must animate");
        f.rendered_effects=f.render_effects();f.last_render=Instant::now()-Duration::from_secs(2);
        assert!(f.needs_render(),"clock and asynchronous artwork must refresh");
        f.last_render=Instant::now();f.draws.clear();
        assert!(f.needs_render(),"changed drawing commands must redraw");
    }
}

#[cfg(test)]
mod navigation_fade_tests {
    use super::*;

    #[test]
    fn the_first_frame_fades_even_when_starting_with_a_seated_game() {
        let now = Instant::now();
        let mut fade = NavFade::default();
        // An existing cart boots to Inserting: this is not a navigation scene.
        fade.begin_first_frame(None, now);
        fade.update(None, now);
        assert_eq!(fade.opacity(now), 1.0);
        assert!(fade.active(now + BOOT_FADE / 2));
        assert_eq!(fade.opacity(now + BOOT_FADE), 0.0);
        assert!(!fade.active(now + BOOT_FADE));

        // Returning from the game must never replay a black fade.
        fade.update(Some(NavScene::Shelf), now + Duration::from_secs(2));
        assert_eq!(fade.opacity(now + Duration::from_secs(2)), 0.0);
    }

    #[test]
    fn the_boot_fade_survives_early_scene_changes_without_restarting() {
        let now = Instant::now();
        let mut fade = NavFade::default();
        fade.begin_first_frame(Some(NavScene::Shelf), now);
        fade.update(Some(NavScene::Shelf), now);
        fade.update(None, now + BOOT_FADE / 4);
        assert!(fade.opacity(now + BOOT_FADE / 4) > 0.0);
        fade.update(Some(NavScene::Shelf), now + BOOT_FADE / 2);
        assert!(fade.opacity(now + BOOT_FADE / 2) < 1.0);
        assert_eq!(fade.opacity(now + BOOT_FADE), 0.0);
        assert!(!fade.active(now + BOOT_FADE));
    }

    #[test]
    fn selection_movement_does_not_restart_a_menu_fade() {
        let now = Instant::now();
        let mut fade = NavFade::default();
        let screen = NavScene::QuickMenu(QuickRow::Screen.parent());
        let game = NavScene::QuickMenu(QuickRow::Game.parent());
        assert_eq!(screen, game);
        fade.update(Some(screen), now);
        fade.update(Some(game), now + Duration::from_millis(50));
        assert!(fade.opacity(now + Duration::from_millis(100)) < 1.0);
        assert_eq!(fade.opacity(now + MENU_FADE), 0.0);
    }

    #[test]
    fn opening_and_closing_a_page_fades_in_the_destination() {
        let now = Instant::now();
        let mut fade = NavFade::default();
        fade.update(Some(NavScene::Shelf), now);
        assert_eq!(fade.opacity(now), 1.0);
        assert_eq!(fade.opacity(now + BOOT_FADE), 0.0);

        let menu_start = now + BOOT_FADE;
        fade.update(Some(NavScene::QuickMenu(None)), menu_start);
        assert_eq!(fade.opacity(menu_start), 1.0);
        assert!(fade.active(menu_start + Duration::from_millis(100)));
        assert_eq!(fade.opacity(menu_start + MENU_FADE), 0.0);

        let subpage = menu_start + MENU_FADE;
        fade.update(Some(NavScene::QuickMenu(Some(QuickRow::System))), subpage);
        assert_eq!(fade.opacity(subpage), 1.0);
        fade.update(Some(NavScene::Shelf), subpage + MENU_FADE);
        assert_eq!(fade.opacity(subpage + MENU_FADE), 1.0);
        assert_eq!(fade.opacity(subpage + MENU_FADE + MENU_FADE), 0.0);
    }

    #[test]
    fn ejecting_a_game_returns_to_the_shelf_without_a_black_flash() {
        let now = Instant::now();
        let mut fade = NavFade::default();
        fade.update(Some(NavScene::Shelf), now);
        // Playing / Ejecting are not navigation scenes.
        fade.update(None, now + BOOT_FADE);
        let back = now + BOOT_FADE + Duration::from_secs(2);
        fade.update(Some(NavScene::Shelf), back);
        assert_eq!(fade.opacity(back), 0.0);
        assert!(!fade.active(back));

        // Normal menu -> shelf navigation still fades.
        let menu = back + Duration::from_secs(1);
        fade.update(Some(NavScene::QuickMenu(None)), menu);
        assert_eq!(fade.opacity(menu), 1.0);
        fade.update(Some(NavScene::Shelf), menu + MENU_FADE);
        assert_eq!(fade.opacity(menu + MENU_FADE), 1.0);
    }

    #[test]
    fn game_playback_does_not_get_a_menu_fade() {
        let mut fade = NavFade::default();
        let now = Instant::now();
        fade.update(Some(NavScene::Shelf), now);
        fade.update(None, now + Duration::from_millis(10));
        assert!(!fade.active(now + Duration::from_millis(10)));
        assert_eq!(fade.opacity(now + Duration::from_millis(10)), 0.0);
    }
}

#[cfg(test)]
mod power_fade_tests {
    use super::*;

    #[test]
    fn sleep_fade_darkens_the_last_frame_once_then_stops_rendering() {
        let start = Instant::now();
        let mut fade = PowerFade::default();
        let draws = vec![Draw::Rect {
            x: 0.0, y: 0.0, w: 640.0, h: 480.0,
            colour: [1.0, 1.0, 1.0, 1.0],
        }];
        fade.begin_sleep(start, &draws);
        assert_eq!(fade.sleep.as_ref().unwrap().1, draws);
        assert!(fade.sleep_frame_needed(start));
        assert!(fade.sleep_frame_needed(start + SLEEP_FADE / 2));
        assert_eq!(PowerFade::progress(start, start, SLEEP_FADE), 0.0);
        assert_eq!(PowerFade::progress(start + SLEEP_FADE, start, SLEEP_FADE), 1.0);
        assert!(fade.sleep_frame_needed(start + SLEEP_FADE)); // Last solid black frame
        // The caller only closes the hardware backlight on this false return,
        // i.e. one loop iteration after the fully black frame was swapped.
        assert!(!fade.sleep_frame_needed(start + SLEEP_FADE));
    }

    #[test]
    fn wake_fades_in_even_when_the_destination_is_a_game() {
        let start = Instant::now();
        let mut fade = PowerFade::default();
        fade.begin_sleep(start, &[]);
        fade.resume(start + Duration::from_secs(5));
        let resumed = start + Duration::from_secs(5);
        assert!(fade.sleep.is_none());
        assert_eq!(fade.wake_opacity(resumed), 1.0);
        assert!(fade.wake_opacity(resumed + WAKE_FADE / 2) < 1.0);
        assert_eq!(fade.wake_opacity(resumed + WAKE_FADE), 0.0);
        assert!(!fade.wake_active(resumed + WAKE_FADE));
    }

    #[test]
    fn shutdown_crossfade_completes_before_existing_poweroff_deadline() {
        let start = Instant::now();
        let mut fade = PowerFade::default();
        fade.begin_shutdown(start, &[]);
        assert!(SHUTDOWN_FADE < Duration::from_millis(250));
        assert_eq!(PowerFade::progress(start, start, SHUTDOWN_FADE), 0.0);
        assert_eq!(PowerFade::progress(start + SHUTDOWN_FADE, start, SHUTDOWN_FADE), 1.0);
        assert!(!fade.shutdown_active(start + SHUTDOWN_FADE));
    }
}
