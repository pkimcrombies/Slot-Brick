use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::ptr::NonNull;

use slot_input::{Action, Btn};

use crate::audio::{volume, Ring};
use crate::music_library::{Album, Library, Scan, Song};
use crate::music_m4a::{is_m4a, M4aDecoder};
use crate::resample::Resampler;

const CHUNK: usize = 512;
const SEEK_SECONDS: i32 = 10;

unsafe extern "C" {
    fn slot_mp3_open(
        path: *const c_void,
        rate: *mut u32,
        channels: *mut u32,
        duration: *mut u64,
    ) -> *mut c_void;
    fn slot_mp3_read(mp3: *mut c_void, samples: *mut i16, frames: u32) -> u32;
    fn slot_mp3_seek(mp3: *mut c_void, frame: u64, position: *mut u64) -> i32;
    fn slot_mp3_close(mp3: *mut c_void);
}

enum MusicDecoder {
    Mp3(Mp3),
    M4a(Box<M4aDecoder>),
}

struct Mp3(NonNull<c_void>);

// The MP3 handle owns its file and decoder with no thread affinity. Calls require exclusive access.
unsafe impl Send for Mp3 {}

impl Drop for Mp3 {
    fn drop(&mut self) {
        unsafe { slot_mp3_close(self.0.as_ptr()) };
    }
}

struct Decoder {
    decoder: MusicDecoder,
    rate: u32,
    channels: usize,
    duration: u64,
}

impl Decoder {
    fn open(path: &Path) -> Option<Self> {
        if is_m4a(path) {
            let m4a = Box::new(M4aDecoder::open(path)?);
            return Some(Self {
                rate: m4a.rate,
                channels: m4a.channels,
                duration: m4a.duration,
                decoder: MusicDecoder::M4a(m4a),
            });
        }
        #[cfg(unix)]
        let path = {
            use std::os::unix::ffi::OsStrExt;
            std::ffi::CString::new(path.as_os_str().as_bytes())
                .ok()?
                .into_bytes_with_nul()
        };
        #[cfg(windows)]
        let path = {
            use std::os::windows::ffi::OsStrExt;
            path.as_os_str()
                .encode_wide()
                .chain([0])
                .collect::<Vec<_>>()
        };
        let (mut rate, mut channels, mut duration) = (0, 0, 0);
        let raw = NonNull::new(unsafe {
            slot_mp3_open(
                path.as_ptr().cast(),
                &mut rate,
                &mut channels,
                &mut duration,
            )
        })?;
        let decoder = Self {
            decoder: MusicDecoder::Mp3(Mp3(raw)),
            rate,
            channels: channels as usize,
            duration,
        };
        (matches!(channels, 1 | 2) && (8_000..=48_000).contains(&rate)).then_some(decoder)
    }

    fn read_stereo(&mut self, samples: &mut [i16; CHUNK * 2]) -> usize {
        let frames = match &mut self.decoder {
            MusicDecoder::Mp3(raw) => unsafe {
                slot_mp3_read(raw.0.as_ptr(), samples.as_mut_ptr(), CHUNK as u32) as usize
            },
            MusicDecoder::M4a(decoder) => decoder.read(&mut samples[..CHUNK * self.channels]),
        };
        if self.channels == 1 {
            for i in (0..frames).rev() {
                let sample = samples[i];
                samples[i * 2..i * 2 + 2].fill(sample);
            }
        }
        frames
    }

    /// Seek to a source frame, returning the actual position and whether the stream ended.
    fn seek(&mut self, frame: u64) -> Option<(u64, bool)> {
        match &mut self.decoder {
            MusicDecoder::Mp3(raw) => {
                let mut position = 0;
                let result = unsafe { slot_mp3_seek(raw.0.as_ptr(), frame, &mut position) };
                (result >= 0).then_some((position, result == 0))
            }
            MusicDecoder::M4a(decoder) => decoder.seek(frame).map(|position| (position, false)),
        }
    }
}

struct Playback {
    decoder: Decoder,
    resampler: Resampler,
    output_rate: u32,
    submitted: u64,
    output: Vec<i16>,
}

impl Playback {
    fn open(path: &Path, output_rate: u32) -> Option<Self> {
        let decoder = Decoder::open(path)?;
        Some(Self {
            resampler: Resampler::new(f64::from(decoder.rate), f64::from(output_rate)),
            decoder,
            output_rate,
            submitted: 0,
            output: Vec::new(),
        })
    }

    fn seek_by(&mut self, seconds: i32, ring: &Ring) -> Option<bool> {
        let source_rate = u64::from(self.decoder.rate);
        let queued = ring.queued_frames() as u64 * source_rate / u64::from(self.output_rate);
        let current = self.submitted.saturating_sub(queued);
        let target = current.saturating_add_signed(i64::from(seconds) * source_rate as i64);
        let duration = self.decoder.duration;
        let (position, at_end) = if duration != 0 && target >= duration {
            (duration, true)
        } else {
            self.decoder.seek(target)?
        };
        if at_end && duration == 0 {
            self.decoder.duration = position;
        }
        ring.discard();
        self.submitted = position;
        self.resampler = Resampler::new(f64::from(self.decoder.rate), f64::from(self.output_rate));
        Some(at_end)
    }

    fn fill(&mut self, ring: &Ring, level: u8) -> bool {
        let mut samples = [0; CHUNK * 2];
        for _ in 0..8 {
            if ring.queued_frames() >= ring.capacity_frames().div_ceil(2) {
                break;
            }
            let decoded = self.decoder.read_stereo(&mut samples);
            if decoded == 0 {
                return false;
            }
            self.resampler
                .process(&samples[..decoded * 2], &mut self.output);
            volume::apply(&mut self.output, level);
            ring.push(&self.output);
            self.submitted += decoded as u64;
        }
        true
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Status {
    Loading,
    Playing,
    Paused,
    Finished,
    Unreadable,
    NoAudio,
}

pub(crate) struct Track {
    index: usize,
    pub(crate) status: Status,
    playback: Option<Playback>,
    ending: bool,
    seek_seconds: i32,
}

impl Track {
    pub(crate) fn time(&self) -> Option<(f64, Option<f64>)> {
        let playback = self.playback.as_ref()?;
        let rate = f64::from(playback.decoder.rate);
        let total = playback.decoder.duration;
        Some((
            playback.submitted as f64 / rate,
            (total != 0).then(|| total as f64 / rate),
        ))
    }
}

#[derive(Clone, Copy)]
enum Page {
    Albums,
    Artists,
    Artist(usize),
    Album(usize),
}

struct Location {
    page: Page,
    cursor: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum RepeatMode { Off, All, One }

impl RepeatMode {
    pub(crate) fn label(self) -> &'static str {
        match self { Self::Off => "off", Self::All => "all", Self::One => "one" }
    }
    fn step(self, forward: bool) -> Self {
        const MODES: [RepeatMode; 3] = [RepeatMode::Off, RepeatMode::All, RepeatMode::One];
        let at = MODES.iter().position(|&mode| mode == self).unwrap_or(0);
        MODES[(at + if forward { 1 } else { MODES.len() - 1 }) % MODES.len()]
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum AlbumSort { Az, Release }

impl AlbumSort {
    pub(crate) fn label(self) -> &'static str {
        match self { Self::Az => "a-z", Self::Release => "newest first" }
    }
}

#[derive(Clone, Copy, Debug)]
struct MusicSettings {
    max_volume: u8,
    cover_flow: bool,
    album_sort: AlbumSort,
    repeat: RepeatMode,
    dim_timeout_secs: u16,
}

impl Default for MusicSettings {
    fn default() -> Self {
        Self {
            // Digital-gain limit for Music only, not a measured dB ceiling.
            max_volume: 70,
            cover_flow: true,
            album_sort: AlbumSort::Az,
            repeat: RepeatMode::Off,
            // Keep the screen illuminated; gently dim after a minute.
            dim_timeout_secs: 60,
        }
    }
}

impl MusicSettings {
    fn path(root: &Path) -> PathBuf { root.join("Config/music.settings") }

    fn load(root: &Path) -> Self {
        let mut settings = Self::default();
        if let Ok(content) = std::fs::read_to_string(Self::path(root)) {
            for line in content.lines() {
                let Some((key, value)) = line.split_once('=') else { continue };
                match (key.trim(), value.trim()) {
                    ("max_volume", level) => {
                        if let Ok(value) = level.parse::<u8>() {
                            if value <= 100 && value % 10 == 0 {
                                settings.max_volume = value;
                            }
                        }
                    }
                    ("album_view", "list") => settings.cover_flow = false,
                    ("album_view", "covers") => settings.cover_flow = true,
                    ("album_sort", "release") => settings.album_sort = AlbumSort::Release,
                    ("album_sort", "az") => settings.album_sort = AlbumSort::Az,
                    ("repeat", "all") => settings.repeat = RepeatMode::All,
                    ("repeat", "one") => settings.repeat = RepeatMode::One,
                    ("repeat", "off") => settings.repeat = RepeatMode::Off,
                    ("dim_timeout", seconds) => {
                        if let Ok(seconds) = seconds.parse::<u16>() {
                            if [0, 30, 60, 120].contains(&seconds) {
                                settings.dim_timeout_secs = seconds;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        settings
    }

    fn save(self, root: &Path) {
        let path = Self::path(root);
        if let Some(parent) = path.parent() {
            if let Err(err) = std::fs::create_dir_all(parent) {
                eprintln!("slot: music settings directory: {err}");
                return;
            }
        }
        let text = format!(
            "max_volume={}\nalbum_view={}\nalbum_sort={}\nrepeat={}\ndim_timeout={}\n",
            self.max_volume,
            if self.cover_flow { "covers" } else { "list" },
            if self.album_sort == AlbumSort::Az { "az" } else { "release" },
            self.repeat.label(),
            self.dim_timeout_secs,
        );
        let temp = path.with_extension("settings.tmp");
        if let Err(err) = std::fs::write(&temp, text).and_then(|()| std::fs::rename(&temp, &path)) {
            eprintln!("slot: music settings save: {err}");
            let _ = std::fs::remove_file(temp);
        }
    }
}

pub(crate) struct Music {
    library: Library,
    scan: Option<Scan>,
    browse: Vec<Location>,
    pub(crate) track: Option<Track>,
    discard_audio: bool,
    /// Cover Flow is the default root album browser. X toggles the former list.
    cover_flow: bool,
    settings: MusicSettings,
    settings_open: bool,
    settings_row: usize,
    settings_root: Option<PathBuf>,
    /// Stable album identities: sorting never mutates the library itself.
    album_order: Vec<usize>,
    artist_album_order: Vec<Vec<usize>>,
}

impl Music {
    pub(crate) fn open(slot: &Path) -> Self {
        let mut music = Self::with_library(Library::default());
        music.settings = MusicSettings::load(slot);
        music.cover_flow = music.settings.cover_flow;
        music.settings_root = Some(slot.to_path_buf());
        music.scan = Scan::start(slot.to_path_buf());
        music
    }

    fn with_library(library: Library) -> Self {
        let mut music = Self {
            library,
            scan: None,
            browse: vec![Location {
                page: Page::Albums,
                cursor: 0,
            }],
            track: None,
            discard_audio: false,
            cover_flow: true,
            settings: MusicSettings::default(),
            settings_open: false,
            settings_row: 0,
            settings_root: None,
            album_order: Vec::new(),
            artist_album_order: Vec::new(),
        };
        music.sort_albums();
        music
    }

    fn persist_settings(&self) {
        if let Some(root) = &self.settings_root {
            self.settings.save(root);
        }
    }

    /// Sort browser row IDs rather than moving albums. Playing album indexes
    /// and previously loaded Cover Flow artwork retain their identities.
    fn sort_albums(&mut self) {
        let albums = &self.library.albums;
        let mut order: Vec<usize> = (0..albums.len()).collect();
        let sort = |a: &usize, b: &usize| {
            let aa = &albums[*a];
            let bb = &albums[*b];
            let alpha = || aa.title.to_lowercase().cmp(&bb.title.to_lowercase())
                .then_with(|| aa.artist.to_lowercase().cmp(&bb.artist.to_lowercase()));
            match self.settings.album_sort {
                AlbumSort::Az => alpha(),
                AlbumSort::Release => bb.release_date.cmp(&aa.release_date)
                    .then_with(alpha),
            }
        };
        order.sort_by(sort);
        self.artist_album_order = self.library.artists.iter().map(|artist| {
            order.iter().copied().filter(|id| artist.albums.contains(id)).collect()
        }).collect();
        self.album_order = order;
    }

    fn album_at(&self, row: usize) -> Option<usize> {
        match self.location().page {
            Page::Albums => self.album_order.get(row).copied(),
            Page::Artist(artist) => self.artist_album_order.get(artist)?.get(row).copied(),
            Page::Artists | Page::Album(_) => None,
        }
    }

    pub(crate) fn repeat_mode(&self) -> RepeatMode { self.settings.repeat }
    fn effective_volume(&self, requested: u8) -> u8 {
        requested.min(self.settings.max_volume)
    }
    pub(crate) fn music_settings_open(&self) -> bool { self.settings_open }
    pub(crate) fn settings_cursor(&self) -> usize { self.settings_row }
    pub(crate) fn settings_max_volume(&self) -> u8 { self.settings.max_volume }
    pub(crate) fn settings_album_view(&self) -> &'static str {
        if self.settings.cover_flow { "cover flow" } else { "list" }
    }
    pub(crate) fn settings_sort(&self) -> &'static str { self.settings.album_sort.label() }
    pub(crate) fn dim_timeout_secs(&self) -> u16 { self.settings.dim_timeout_secs }
    pub(crate) fn settings_dim_timeout(&self) -> &'static str {
        match self.settings.dim_timeout_secs {
            0 => "off",
            30 => "30 sec",
            60 => "1 min",
            120 => "2 min",
            _ => "1 min",
        }
    }
    pub(crate) fn should_dim(&self) -> bool {
        self.playing() && !self.settings_open && self.settings.dim_timeout_secs > 0
    }

    fn change_setting(&mut self, forward: bool) {
        match self.settings_row {
            0 => {
                let level = self.settings.max_volume;
                self.settings.max_volume = if forward {
                    level.saturating_add(10).min(100)
                } else {
                    level.saturating_sub(10)
                };
                if self.settings.max_volume < level { self.discard_audio = true; }
            }
            1 => {
                self.settings.cover_flow = !self.settings.cover_flow;
                self.cover_flow = self.settings.cover_flow;
            }
            2 => {
                self.settings.album_sort = match self.settings.album_sort {
                    AlbumSort::Az => AlbumSort::Release,
                    AlbumSort::Release => AlbumSort::Az,
                };
                self.sort_albums();
                for location in &mut self.browse {
                    if matches!(location.page, Page::Albums | Page::Artist(_)) {
                        location.cursor = 0;
                    }
                }
            }
            3 => self.settings.repeat = self.settings.repeat.step(forward),
            4 => {
                const VALUES: [u16; 4] = [0, 30, 60, 120];
                let current = VALUES.iter()
                    .position(|&value| value == self.settings.dim_timeout_secs)
                    .unwrap_or(2);
                let next = if forward { (current + 1) % VALUES.len() }
                    else { (current + VALUES.len() - 1) % VALUES.len() };
                self.settings.dim_timeout_secs = VALUES[next];
            }
            _ => {}
        }
        self.persist_settings();
    }

    fn location(&self) -> &Location {
        self.browse.last().expect("the library has a root page")
    }

    fn open_page(&mut self, page: Page) {
        self.browse.push(Location { page, cursor: 0 });
    }

    pub(crate) fn label(&self) -> &str {
        match self.location().page {
            Page::Albums => "albums",
            Page::Artists => "artists",
            Page::Artist(artist) => &self.library.artists[artist].name,
            Page::Album(album) => &self.library.albums[album].title,
        }
    }

    pub(crate) fn is_root(&self) -> bool {
        self.browse.len() == 1
    }

    pub(crate) fn scanning(&self) -> bool {
        self.scan.is_some()
    }

    pub(crate) fn cursor(&self) -> usize {
        self.location().cursor
    }

    fn select_row(&mut self, cursor: usize) {
        self.browse
            .last_mut()
            .expect("the library has a root page")
            .cursor = cursor;
    }

    pub(crate) fn row_count(&self) -> usize {
        match self.location().page {
            Page::Albums => self.library.albums.len(),
            Page::Artists => self.library.artists.len(),
            Page::Artist(artist) => self.library.artists[artist].albums.len(),
            Page::Album(album) => self.library.albums[album].songs.len(),
        }
    }

    pub(crate) fn row(&self, index: usize) -> (&str, Option<&str>) {
        match self.location().page {
            Page::Albums => {
                let album = &self.library.albums[self.album_order[index]];
                (&album.title, Some(&album.artist))
            }
            Page::Artists => (&self.library.artists[index].name, None),
            Page::Artist(artist) => {
                let album = self.artist_album_order[artist][index];
                (&self.library.albums[album].title, None)
            }
            Page::Album(album) => (&self.library.albums[album].songs[index].title, None),
        }
    }

    /// Returns an album only for rows that actually represent albums.
    /// Artist-name and individual-track rows must not display album thumbnails.
    pub(crate) fn album_for_row(&self, index: usize) -> Option<&Album> {
        self.album_at(index).and_then(|id| self.library.albums.get(id))
    }

    pub(crate) fn cover_flow(&self) -> bool {
        self.cover_flow && self.is_root() && !self.viewing_artists()
    }

    pub(crate) fn showing_tracks(&self) -> bool {
        matches!(self.location().page, Page::Album(_))
    }

    pub(crate) fn song(&self, track: &Track) -> &Song {
        &self.playing_album().songs[track.index]
    }

    pub(crate) fn playing_album(&self) -> &Album {
        let Page::Album(album) = self.location().page else {
            unreachable!("playback has an album open")
        };
        &self.library.albums[album]
    }

    fn selected_album(&self) -> Option<usize> {
        let cursor = self.cursor();
        match self.location().page {
            Page::Albums => self.album_order.get(cursor).copied(),
            Page::Artist(artist) => self.artist_album_order.get(artist)?.get(cursor).copied(),
            Page::Album(album) => Some(album),
            Page::Artists => None,
        }
    }

    pub(crate) fn viewing_artists(&self) -> bool {
        matches!(self.location().page, Page::Artists)
    }
    pub(crate) fn playing(&self) -> bool {
        self.track
            .as_ref()
            .is_some_and(|track| matches!(track.status, Status::Loading | Status::Playing))
    }

    fn start(&mut self, index: usize) {
        self.select_row(index);
        self.track = Some(Track {
            index,
            status: Status::Loading,
            playback: None,
            ending: false,
            seek_seconds: 0,
        });
        self.discard_audio = true;
    }

    fn switch(&mut self, index: usize, forward: bool) {
        if let Some(next) = index
            .checked_add_signed(if forward { 1 } else { -1 })
            .filter(|&i| i < self.row_count())
        {
            self.start(next);
        }
    }

    fn advance_at_end(&mut self, index: usize) {
        match self.settings.repeat {
            RepeatMode::One => self.start(index),
            RepeatMode::All if index + 1 == self.row_count() => self.start(0),
            _ => self.switch(index, true),
        }
    }

    /// True when the browser should return to Slot's menu.
    pub(crate) fn input(&mut self, action: Action) -> bool {
        if action == Action::QuickMenu { return true; }
        if action == Action::GbaDown(Btn::Start) {
            self.settings_open = !self.settings_open;
            return false;
        }
        if self.settings_open {
            match action {
                Action::GbaDown(Btn::Up) => {
                    self.settings_row = self.settings_row.saturating_sub(1);
                }
                Action::GbaDown(Btn::Down) => {
                    self.settings_row = (self.settings_row + 1).min(4);
                }
                Action::GbaDown(Btn::A) | Action::GbaDown(Btn::Right) => {
                    self.change_setting(true);
                }
                Action::GbaDown(Btn::Left) => self.change_setting(false),
                Action::GbaDown(Btn::B) => self.settings_open = false,
                _ => {}
            }
            return false;
        }
        if let Some(track) = &mut self.track {
            let index = track.index;
            match action {
                Action::GbaDown(Btn::A) => match track.status {
                    Status::Playing => track.status = Status::Paused,
                    Status::Paused => track.status = Status::Playing,
                    Status::Finished | Status::Unreadable | Status::NoAudio => self.start(index),
                    Status::Loading => {}
                },
                Action::GbaDown(Btn::Left) => {
                    track.seek_seconds = track.seek_seconds.saturating_sub(SEEK_SECONDS);
                }
                Action::GbaDown(Btn::Right) => {
                    track.seek_seconds = track.seek_seconds.saturating_add(SEEK_SECONDS);
                }
                Action::GbaDown(Btn::L1) | Action::ShelfLeft => self.switch(index, false),
                Action::GbaDown(Btn::R1) | Action::ShelfRight => self.switch(index, true),
                Action::GbaDown(Btn::X) => {
                    self.settings.repeat = self.settings.repeat.step(true);
                    self.persist_settings();
                }
                Action::GbaDown(Btn::B) => {
                    self.track = None;
                    self.discard_audio = true;
                }
                _ => {}
            }
            return false;
        }
        match action {
            // Cover Flow scrolls albums horizontally. L1/R1 still switch
            // between the Albums and Artists roots. X restores list browsing.
            Action::GbaDown(Btn::X) if self.is_root() && !self.viewing_artists() => {
                self.cover_flow = !self.cover_flow;
                self.settings.cover_flow = self.cover_flow;
                self.persist_settings();
            }
            Action::GbaDown(Btn::Left) | Action::ShelfLeft if self.cover_flow() => {
                self.select_row(self.cursor().saturating_sub(1));
            }
            Action::GbaDown(Btn::Right) | Action::ShelfRight if self.cover_flow() => {
                if self.cursor() + 1 < self.row_count() {
                    self.select_row(self.cursor() + 1);
                }
            }
            Action::GbaDown(Btn::Up) => self.select_row(self.cursor().saturating_sub(1)),
            Action::GbaDown(Btn::Down) if self.cursor() + 1 < self.row_count() => {
                self.select_row(self.cursor() + 1)
            }
            Action::GbaDown(btn @ (Btn::L1 | Btn::R1 | Btn::Left | Btn::Right)) if self.is_root() => {
                let artists = match btn {
                    Btn::Left => false,
                    Btn::Right => true,
                    _ => !self.viewing_artists(),
                };
                if artists != self.viewing_artists() {
                    self.browse[0] = Location {
                        page: if artists { Page::Artists } else { Page::Albums },
                        cursor: 0,
                    };
                }
            }
            Action::GbaDown(Btn::A) if self.row_count() > 0 => match self.location().page {
                Page::Album(_) => self.start(self.cursor()),
                Page::Artists => self.open_page(Page::Artist(self.cursor())),
                _ => self.open_page(Page::Album(
                    self.selected_album().expect("album row exists"),
                )),
            },
            Action::GbaDown(Btn::Y) => {
                if let Some(album) = self.selected_album() {
                    if !self.showing_tracks() {
                        self.open_page(Page::Album(album));
                    }
                    self.start(0);
                }
            }
            Action::GbaDown(Btn::B) if self.is_root() => return true,
            Action::GbaDown(Btn::B) => {
                self.browse.pop();
            }
            _ => {}
        }
        false
    }

    /// Stop queued audio when entering kernel suspend without losing track,
    /// position, repeat mode, browsing location or play/pause status.
    /// Seeking to the audible position subtracts the ring's queued frames;
    /// on wake the same decoder continues feeding the freshly opened sink.
    pub(crate) fn prepare_suspend(&mut self, ring: &Ring) {
        if let Some(track) = self.track.as_mut() {
            if let Some(playback) = track.playback.as_mut() {
                // Rewind the decoder over any not-yet-audible buffered audio.
                // The returned status is unchanged; paused stays paused.
                let _ = playback.seek_by(0, ring);
            }
            track.ending = false;
        }
        self.discard_audio = true;
        ring.discard();
    }

    pub(crate) fn update_audio(&mut self, ring: &Ring, level: u8) {
        if let Some(library) = self.scan.as_ref().and_then(Scan::take) {
            self.library = library;
            self.scan = None;
            self.sort_albums();
        }
        if std::mem::take(&mut self.discard_audio) {
            ring.discard();
        }
        let Page::Album(album) = self.location().page else {
            return;
        };
        let volume = self.effective_volume(level);
        let Some(track) = &mut self.track else { return };
        if track.status == Status::Loading {
            if ring.sample_rate() == 0 {
                track.status = Status::NoAudio;
                return;
            }
            track.playback = Playback::open(
                &self.library.albums[album].songs[track.index].path,
                ring.sample_rate(),
            );
            track.status = if track.playback.is_some() {
                Status::Playing
            } else {
                Status::Unreadable
            };
        }
        let seek = std::mem::take(&mut track.seek_seconds);
        if seek != 0 {
            if let Some(at_end) = track
                .playback
                .as_mut()
                .and_then(|playback| playback.seek_by(seek, ring))
            {
                track.ending = false;
                if at_end {
                    track.status = Status::Finished;
                } else if track.status == Status::Finished {
                    track.status = Status::Paused;
                }
            }
        }
        if track.status != Status::Playing {
            return;
        }
        if !track.ending {
            track.ending = !track
                .playback
                .as_mut()
                .expect("playing track has a decoder")
                .fill(ring, volume);
        }
        if track.ending && ring.queued_frames() == 0 {
            let index = track.index;
            track.status = Status::Finished;
            self.advance_at_end(index);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_persist_and_bad_values_do_not_override_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let mut prefs = MusicSettings::default();
        assert_eq!(prefs.max_volume, 70);
        prefs.max_volume = 40;
        prefs.cover_flow = false;
        prefs.album_sort = AlbumSort::Release;
        prefs.repeat = RepeatMode::One;
        prefs.dim_timeout_secs = 120;
        prefs.save(dir.path());
        let loaded = MusicSettings::load(dir.path());
        assert_eq!(loaded.max_volume, 40);
        assert!(!loaded.cover_flow);
        assert_eq!(loaded.album_sort, AlbumSort::Release);
        assert_eq!(loaded.repeat, RepeatMode::One);
        assert_eq!(loaded.dim_timeout_secs, 120);
        std::fs::write(MusicSettings::path(dir.path()), "max_volume=255\\n").unwrap();
        assert_eq!(MusicSettings::load(dir.path()).max_volume, 70);
    }

    #[test]
    fn repeat_cycles_off_all_one_and_wraps_or_replays_only_when_selected() {
        let make = |title: &str| Album {
            title: title.to_string(), artist: "artist".into(),
            release_date: None, songs: vec![Song {
                path: PathBuf::from(format!("{title}.mp3")), title: "track".into(),
                artist: "artist".into(), disc: 1, track: Some(1),
            }],
        };
        let mut music = Music::with_library(Library {
            albums: vec![make("A")], artists: Vec::new(),
        });
        music.open_page(Page::Album(0));
        music.start(0);
        assert_eq!(music.repeat_mode(), RepeatMode::Off);
        music.advance_at_end(0);
        assert_eq!(music.track.as_ref().unwrap().status, Status::Loading);
        // A single-track album with repeat off must *not* start over:
        // advance_at_end leaves the previously finished track intact.
        music.track.as_mut().unwrap().status = Status::Finished;
        music.advance_at_end(0);
        assert_eq!(music.track.as_ref().unwrap().status, Status::Finished);
        music.input(Action::GbaDown(Btn::X));
        assert_eq!(music.repeat_mode(), RepeatMode::All);
        music.advance_at_end(0);
        assert_eq!(music.track.as_ref().unwrap().status, Status::Loading);
        music.input(Action::GbaDown(Btn::X));
        assert_eq!(music.repeat_mode(), RepeatMode::One);
        music.advance_at_end(0);
        assert_eq!(music.track.as_ref().unwrap().index, 0);
        music.input(Action::GbaDown(Btn::X));
        assert_eq!(music.repeat_mode(), RepeatMode::Off);
    }

    #[test]
    fn date_sort_orders_newest_first_with_missing_years_last_and_stable_album_ids() {
        let mk = |title: &str, year| Album {
            title: title.into(), artist: "artist".into(), release_date: year,
            songs: vec![Song {
                path: PathBuf::from(format!("{title}.mp3")),
                title: "song".into(), artist: "artist".into(), disc: 1,
                track: Some(1),
            }],
        };
        let mut music = Music::with_library(Library {
            albums: vec![
                mk("Alpha", None),
                mk("Beta", Some(20211012)),
                mk("Gamma", Some(20211104)),
                mk("Delta", Some(20050301)),
            ],
            artists: Vec::new(),
        });
        assert_eq!(music.row(0).0, "Alpha");
        music.settings_row = 2;
        music.change_setting(true);
        assert_eq!(music.row(0).0, "Gamma", "newest full date first");
        assert_eq!(music.row(1).0, "Beta", "same year, earlier month follows");
        assert_eq!(music.row(2).0, "Delta");
        assert_eq!(music.row(3).0, "Alpha", "unknown date last");
        music.input(Action::GbaDown(Btn::A));
        assert_eq!(music.playing_album().title, "Gamma");
        // Changing sort cannot move or replace the album being played.
        music.settings_row = 2;
        music.change_setting(true);
        assert_eq!(music.playing_album().title, "Gamma");
    }

    #[test]
    fn music_settings_page_keeps_playback_and_changes_are_persistent() {
        let root = tempfile::tempdir().unwrap();
        let mut music = Music::with_library(Library::default());
        music.settings_root = Some(root.path().to_path_buf());
        music.input(Action::GbaDown(Btn::Start));
        assert!(music.music_settings_open());
        assert_eq!(music.settings_max_volume(), 70);
        music.input(Action::GbaDown(Btn::Left));
        assert_eq!(music.settings_max_volume(), 60);
        music.input(Action::GbaDown(Btn::Down));
        music.input(Action::GbaDown(Btn::A));
        assert_eq!(music.settings_album_view(), "list");
        music.input(Action::GbaDown(Btn::Down));
        music.input(Action::GbaDown(Btn::Right));
        assert_eq!(music.settings_sort(), "newest first");
        music.input(Action::GbaDown(Btn::Down));
        music.input(Action::GbaDown(Btn::A));
        assert_eq!(music.repeat_mode(), RepeatMode::All);
        music.input(Action::GbaDown(Btn::Down));
        music.input(Action::GbaDown(Btn::Right));
        assert_eq!(music.settings_dim_timeout(), "2 min");
        music.input(Action::GbaDown(Btn::B));
        assert!(!music.music_settings_open());

        let loaded = MusicSettings::load(root.path());
        assert_eq!(loaded.max_volume, 60);
        assert_eq!(loaded.album_sort, AlbumSort::Release);
        assert!(!loaded.cover_flow);
        assert_eq!(loaded.repeat, RepeatMode::All);
        assert_eq!(loaded.dim_timeout_secs, 120);
    }

    #[test]
    fn album_release_date_is_read_from_id3v23_and_v24_tags() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("Music");
        std::fs::create_dir_all(&folder).unwrap();
        for (version, filename, album, frame, value) in [
            (3, "old.mp3", "Older Album", &b"TYER"[..], "1998"),
            (4, "new.mp3", "Newer Album", &b"TDRC"[..], "2024-11-08"),
        ] {
            let frame: &[u8; 4] = frame.try_into().unwrap();
            tagged_mp3(
                &folder.join(filename), version,
                &[(b"TALB", album), (b"TPE1", "Artist"), (frame, value)],
            );
        }
        let library = Library::scan(root.path(), || false);
        assert_eq!(library.albums.len(), 2);
        assert_eq!(library.albums[0].release_date, Some(20241108),
            "alphabetical Newer Album precedes Older Album");
        assert_eq!(library.albums[1].release_date, Some(19980000));
    }

    #[test]
    fn dim_timeout_cycles_off_30_60_120_and_never_turns_off_screen() {
        let mut music = Music::with_library(Library::default());
        assert_eq!(music.settings_dim_timeout(), "1 min");
        music.settings_row = 4;
        music.change_setting(true);
        assert_eq!(music.dim_timeout_secs(), 120);
        music.change_setting(true);
        assert_eq!(music.dim_timeout_secs(), 0);
        assert_eq!(music.settings_dim_timeout(), "off");
        music.change_setting(true);
        assert_eq!(music.dim_timeout_secs(), 30);
        music.change_setting(false);
        assert_eq!(music.dim_timeout_secs(), 0);
        assert!(!music.should_dim());
    }

    #[test]
    fn sleep_does_not_change_playback_status_or_selected_track() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("Music");
        std::fs::create_dir_all(&folder).unwrap();
        silent_mp3(&folder.join("01.mp3"), 2);
        let mut music = test_music(root.path());
        music.input(Action::GbaDown(Btn::Y));
        let ring = Ring::new(0);
        ring.reopen(48_000);
        ring.set_idle(true);
        music.update_audio(&ring, 40);
        let index = music.track.as_ref().unwrap().index;
        let before = music.track.as_ref().unwrap().status;
        music.prepare_suspend(&ring);
        assert_eq!(music.track.as_ref().unwrap().index, index);
        assert_eq!(music.track.as_ref().unwrap().status, before);
        assert_eq!(ring.queued_frames(), 0);
        assert!(music.discard_audio);
        music.update_audio(&ring, 40);
        assert_eq!(music.track.as_ref().unwrap().index, index);
    }

    #[test]
    fn kid_volume_cap_is_enforced_on_top_of_requested_music_volume() {
        let mut music = Music::with_library(Library::default());
        assert_eq!(music.effective_volume(95), 70);
        assert_eq!(music.effective_volume(30), 30);
        music.settings_row = 0;
        music.change_setting(false);
        assert_eq!(music.effective_volume(95), 60);
        music.change_setting(false);
        assert!(music.discard_audio,
            "lowering the cap must discard already-buffered louder samples");
        assert_eq!(music.effective_volume(95), 50);
    }

    #[test]
    fn cover_rows_are_albums_only_in_root_and_artist_album_lists() {
        use crate::music_library::Artist;
        let album = Album {
            title: "alpha".into(),
            artist: "artist".into(),
            release_date: None,
            songs: vec![Song {
                path: std::path::PathBuf::from("alpha.mp3"),
                title: "song".into(),
                artist: "artist".into(),
                disc: 1,
                track: Some(1),
            }],
        };
        let mut music = Music::with_library(Library {
            albums: vec![album],
            artists: vec![Artist {
                name: "artist".into(),
                albums: vec![0],
            }],
        });
        assert_eq!(music.album_for_row(0).unwrap().title, "alpha");
        assert!(music.album_for_row(1).is_none());
        music.browse[0].page = Page::Artists;
        assert!(music.album_for_row(0).is_none());
        music.open_page(Page::Artist(0));
        assert_eq!(music.album_for_row(0).unwrap().title, "alpha");
        music.open_page(Page::Album(0));
        assert!(music.album_for_row(0).is_none());
    }
    #[test]
    fn cover_flow_is_default_and_left_right_browse_albums_without_changing_tabs() {
        let albums = (0..3).map(|i| Album {
            title: format!("album {i}"),
            artist: "artist".into(),
            release_date: None,
            songs: vec![Song {
                path: std::path::PathBuf::from(format!("{i}.mp3")),
                title: "song".into(),
                artist: "artist".into(),
                disc: 1,
                track: Some(1),
            }],
        }).collect();
        let mut music = Music::with_library(Library { albums, artists: vec![] });
        assert!(music.cover_flow());
        music.input(Action::GbaDown(Btn::Right));
        assert_eq!(music.cursor(), 1);
        assert!(!music.viewing_artists());
        music.input(Action::GbaDown(Btn::Left));
        assert_eq!(music.cursor(), 0);
        music.input(Action::GbaDown(Btn::Left));
        assert_eq!(music.cursor(), 0);
        music.input(Action::GbaDown(Btn::X));
        assert!(!music.cover_flow());
        music.input(Action::GbaDown(Btn::Right));
        assert!(music.viewing_artists(), "list mode retains the existing root tab controls");
        music.input(Action::GbaDown(Btn::L1));
        assert!(music.cover_flow() == false && !music.viewing_artists());
    }

    use std::fs;

    fn silent_mp3(path: &Path, channels: u8) {
        // MPEG-1 Layer III, 128 kbit/s at 44.1 kHz. No coded coefficients means silence.
        let mut frame = [0; 417];
        frame[..4].copy_from_slice(&[0xff, 0xfb, 0x90, if channels == 1 { 0xc4 } else { 0x64 }]);
        fs::write(path, frame.repeat(12)).unwrap();
    }

    fn test_music(root: &Path) -> Music {
        Music::with_library(Library::scan(root, || false))
    }

    fn tagged_mp3(path: &Path, version: u8, frames: &[(&[u8; 4], &str)]) {
        let mut body = Vec::new();
        for (id, value) in frames {
            let mut text = if version == 3 {
                let mut utf16 = vec![1, 0xff, 0xfe];
                utf16.extend(value.encode_utf16().flat_map(u16::to_le_bytes));
                utf16
            } else {
                let mut utf8 = vec![3];
                utf8.extend(value.as_bytes());
                utf8
            };
            body.extend(*id);
            body.extend(if version == 3 {
                (text.len() as u32).to_be_bytes()
            } else {
                syncsafe(text.len())
            });
            body.extend([0, 0]);
            body.append(&mut text);
        }
        let mut file = b"ID3".to_vec();
        file.extend([version, 0, 0]);
        file.extend(syncsafe(body.len()));
        file.extend(body);
        fs::write(path, file).unwrap();
    }

    fn syncsafe(len: usize) -> [u8; 4] {
        [21, 14, 7, 0].map(|shift| ((len >> shift) & 127) as u8)
    }

    #[test]
    fn m4a_files_use_itunes_tags_and_decode_mono_aac_and_stereo_alac() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("Music");
        fs::create_dir(&folder).unwrap();
        // Synthetic 440 Hz tones with distinct track and album artist tags.
        for (name, bytes, channels) in [
            ("one.M4A", include_bytes!("../tests/fixtures/music/aac.m4a").as_slice(), 1),
            ("two.m4a", include_bytes!("../tests/fixtures/music/alac.m4a").as_slice(), 2),
        ] {
            let path = folder.join(name);
            fs::write(&path, bytes).unwrap();
            let mut decoder = Decoder::open(&path).expect("supported m4a");
            assert_eq!((decoder.rate, decoder.channels), (44_100, channels));
            assert!(decoder.duration > 0);
            let mut samples = [0; CHUNK * 2];
            let mut has_audio = false;
            let mut total = 0;
            loop {
                let frames = decoder.read_stereo(&mut samples);
                if frames == 0 {
                    break;
                }
                total += frames;
                assert!(total < 44_100);
                has_audio |= samples[..2 * frames].iter().any(|&sample| sample != 0);
                if channels == 1 {
                    assert!(samples[..2 * frames].chunks_exact(2).all(|pair| pair[0] == pair[1]));
                }
            }
            assert!(has_audio && total > CHUNK);
            let (position, ended) = decoder.seek(4_600).expect("m4a forward seek");
            assert!(!ended && position > 0 && position <= 4_600);
            assert!(decoder.read_stereo(&mut samples) > 0);
            assert_eq!(decoder.seek(0), Some((0, false)));
            assert!(decoder.read_stereo(&mut samples) > 0);
        }

        let library = Library::scan(root.path(), || false);
        assert_eq!(library.albums.len(), 1);
        assert_eq!(library.artists[0].name, "Album Artist");
        let album = &library.albums[0];
        assert_eq!(album.title, "Fixture Album");
        assert_eq!(album.songs.len(), 2);
        for song in &album.songs {
            assert_eq!((song.title.as_str(), song.artist.as_str()), ("Fixture Track", "Guest Artist"));
            assert_eq!((song.disc, song.track), (2, Some(3)));
        }
    }

    #[test]
    fn the_music_browser_starts_from_embedded_album_tags() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("Music");
        fs::create_dir(&folder).unwrap();
        let mut tag =
            b"ID3\x03\x00\x00\x00\x00\x00\x15TALB\x00\x00\x00\x0b\x00\x00\x00From a tag".to_vec();
        tag.extend([0xff, 0xfb, 0x90, 0x64]);
        fs::write(folder.join("a file.mp3"), tag).unwrap();
        let music = test_music(root.path());
        assert_eq!(music.label(), "albums");
        assert_eq!(music.row(0).0, "From a tag");
    }

    #[test]
    fn nests_albums_under_album_artists_and_orders_tracks_across_discs() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("Music");
        fs::create_dir_all(folder.join("a separate folder")).unwrap();
        tagged_mp3(
            &folder.join("a.mp3"),
            3,
            &[
                (b"TIT2", "Hop"),
                (b"TPE1", "Émilie"),
                (b"TALB", "Mixtape"),
                (b"TPE2", "Various Artists"),
                (b"TPOS", "2/2"),
                (b"TRCK", "1/1"),
            ],
        );
        tagged_mp3(
            &folder.join("a separate folder/c.mp3"),
            4,
            &[
                (b"TIT2", "Zig"),
                (b"TPE1", "Alex"),
                (b"TALB", "Mixtape"),
                (b"TPE2", "Various Artists"),
                (b"TPOS", "1/2"),
                (b"TRCK", "1/2"),
            ],
        );
        tagged_mp3(
            &folder.join("b.MP3"),
            3,
            &[
                (b"TIT2", "Jump"),
                (b"TPE1", "Alex"),
                (b"TALB", "Mixtape"),
                (b"TPE2", "Various Artists"),
                (b"TPOS", "1/2"),
                (b"TRCK", "2/2"),
            ],
        );
        tagged_mp3(
            &folder.join("same album name.mp3"),
            4,
            &[
                (b"TIT2", "Different"),
                (b"TPE1", "Another Artist"),
                (b"TALB", "Mixtape"),
            ],
        );
        let mut music = test_music(root.path());
        assert_eq!(
            music.row_count(),
            2,
            "albums with the same title use the album artist to distinguish them"
        );
        assert_eq!(music.row(1), ("Mixtape", Some("Various Artists")));
        // This test covers the legacy list navigation. Cover Flow is the new
        // default, where Right advances an album instead of changing tabs.
        music.input(Action::GbaDown(Btn::X));
        assert!(!music.cover_flow());
        music.input(Action::GbaDown(Btn::Right));
        assert_eq!(music.label(), "artists");
        music.input(Action::GbaDown(Btn::Left));
        assert_eq!(music.label(), "albums");
        music.input(Action::GbaDown(Btn::R1));
        assert_eq!(music.label(), "artists");
        assert_eq!(music.row_count(), 2);
        music.input(Action::GbaDown(Btn::Down));
        assert_eq!(music.row(music.cursor()).0, "Various Artists");
        music.input(Action::GbaDown(Btn::A));
        assert_eq!(music.row_count(), 1);
        music.input(Action::GbaDown(Btn::A));
        assert_eq!(music.row_count(), 3);
        assert_eq!(
            (0..3).map(|i| music.row(i).0).collect::<Vec<_>>(),
            ["Zig", "Jump", "Hop"]
        );
        music.input(Action::GbaDown(Btn::Y));
        music.input(Action::GbaDown(Btn::R1));
        music.input(Action::GbaDown(Btn::R1));
        let track = music.track.as_ref().unwrap();
        assert_eq!(music.song(track).title, "Hop");
        assert_eq!(music.song(track).artist, "Émilie");
        music.input(Action::GbaDown(Btn::B));
        assert!(music.showing_tracks());
        assert_eq!(music.cursor(), 2);
        music.input(Action::GbaDown(Btn::B));
        music.input(Action::GbaDown(Btn::B));
        assert_eq!(music.cursor(), 1);
        assert_eq!(music.label(), "artists");
    }

    #[test]
    fn scans_and_plays_only_mp3s_in_the_current_folder() {
        let root = tempfile::tempdir().unwrap();
        let album = root.path().join("Music/An album");
        fs::create_dir_all(&album).unwrap();
        silent_mp3(&album.join("01 - first.MP3"), 1);
        silent_mp3(&album.join("02 - second.mp3"), 2);
        fs::write(album.join("notes.txt"), "ignored").unwrap();
        let mut music = test_music(root.path());
        assert_eq!(music.row_count(), 1);
        music.input(Action::GbaDown(Btn::A));
        assert_eq!(music.label(), "An album");
        assert_eq!(music.row_count(), 2);
        music.input(Action::GbaDown(Btn::Y));

        let ring = Ring::new(0);
        ring.reopen(48_000);
        ring.set_idle(true);
        music.update_audio(&ring, 80);
        assert!(music.playing());
        assert_eq!(
            music.song(music.track.as_ref().unwrap()).title,
            "01 - first"
        );
        assert!(ring.queued_frames() >= 3_200);
        music.input(Action::GbaDown(Btn::A));
        let elapsed = music.track.as_ref().unwrap().time().unwrap().0;
        ring.fill(&mut [1; 7_000]);
        music.update_audio(&ring, 80);
        assert_eq!(ring.queued_frames(), 0);
        assert_eq!(music.track.as_ref().unwrap().time().unwrap().0, elapsed);

        music.input(Action::GbaDown(Btn::A));
        for _ in 0..20 {
            ring.fill(&mut [1; 7_000]);
            music.update_audio(&ring, 80);
            if music.cursor() == 1 {
                break;
            }
        }
        assert_eq!(
            music.cursor(),
            1,
            "the folder advances at the end of the first song"
        );
        music.update_audio(&ring, 80);
        assert!(ring.queued_frames() > 0);
        music.input(Action::GbaDown(Btn::L1));
        music.update_audio(&ring, 80);
        assert_eq!(music.cursor(), 0);
        music.input(Action::GbaDown(Btn::B));
        music.update_audio(&ring, 80);
        assert!(music.track.is_none());
        assert_eq!(ring.queued_frames(), 0);
        assert!(!music.input(Action::GbaDown(Btn::B)));
        assert!(music.is_root());
        assert!(music.input(Action::GbaDown(Btn::B)));
    }

    #[test]
    fn dpad_seeks_without_changing_tracks_and_keeps_paused_audio_paused() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("Music");
        fs::create_dir(&folder).unwrap();
        let first = folder.join("01.mp3");
        silent_mp3(&first, 2);
        fs::write(&first, fs::read(&first).unwrap().repeat(100)).unwrap();
        silent_mp3(&folder.join("02.mp3"), 2);

        let mut music = test_music(root.path());
        let ring = Ring::new(0);
        ring.reopen(48_000);
        music.input(Action::GbaDown(Btn::Y));
        music.update_audio(&ring, 80);
        music.input(Action::GbaDown(Btn::Right));
        music.update_audio(&ring, 80);
        let track = music.track.as_ref().unwrap();
        assert_eq!(track.index, 0);
        assert!((9.9..10.2).contains(&track.time().unwrap().0));

        music.input(Action::GbaDown(Btn::A));
        music.input(Action::GbaDown(Btn::Left));
        music.update_audio(&ring, 80);
        let track = music.track.as_ref().unwrap();
        assert_eq!(track.index, 0);
        assert!(!music.playing());
        assert_eq!(track.time().unwrap().0, 0.0);
        assert_eq!(ring.queued_frames(), 0);

        for _ in 0..4 {
            music.input(Action::GbaDown(Btn::Right));
            music.update_audio(&ring, 80);
        }
        let track = music.track.as_ref().unwrap();
        assert_eq!(track.index, 0);
        assert!(matches!(track.status, Status::Finished));
        let (elapsed, total) = track.time().unwrap();
        assert_eq!(Some(elapsed), total);

        music.input(Action::GbaDown(Btn::Left));
        music.update_audio(&ring, 80);
        let track = music.track.as_ref().unwrap();
        assert!(matches!(track.status, Status::Paused));
        assert!((elapsed - track.time().unwrap().0 - 10.0).abs() < 0.001);
        music.input(Action::GbaDown(Btn::R1));
        music.update_audio(&ring, 80);
        assert_eq!(music.track.as_ref().unwrap().index, 1);
    }

    #[test]
    fn an_unreadable_mp3_can_be_skipped() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("Music");
        fs::create_dir(&folder).unwrap();
        fs::write(folder.join("bad.mp3"), "broken file").unwrap();
        silent_mp3(&folder.join("good.mp3"), 2);
        let mut music = test_music(root.path());
        music.input(Action::GbaDown(Btn::Y));
        let ring = Ring::new(0);
        ring.reopen(48_000);
        music.update_audio(&ring, 80);
        assert!(matches!(
            music.track.as_ref().unwrap().status,
            Status::Unreadable
        ));
        music.input(Action::GbaDown(Btn::R1));
        music.update_audio(&ring, 80);
        assert!(music.playing());
        assert!(ring.queued_frames() > 0);
    }
}
