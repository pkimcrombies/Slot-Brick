use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::ptr::NonNull;

use slot_input::{Action, Btn};
use slot_store::BRIGHTNESS_MAX;

use crate::audio::{volume, Ring};
use crate::music_library::{Album, Library, Scan, Song};
use crate::music_m4a::{is_m4a, M4aDecoder};
use crate::resample::Resampler;

const CHUNK: usize = 512;

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
    album: usize,
    index: usize,
    pub(crate) status: Status,
    playback: Option<Playback>,
    ending: bool,
    // Button events are UI-thread only. Seek is applied by update_audio,
    // where the decoder and audio Ring are available together.
    pending_seek_seconds: i32,
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
    dim_brightness: u8,
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
            // Lowest nonzero hardware step; independent of normal brightness.
            dim_brightness: 1,
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
                    ("dim_brightness", level) => {
                        if let Ok(level) = level.parse::<u8>() {
                            if (1..=BRIGHTNESS_MAX).contains(&level) {
                                settings.dim_brightness = level;
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
            "max_volume={}\nalbum_view={}\nalbum_sort={}\nrepeat={}\ndim_timeout={}\ndim_brightness={}\n",
            self.max_volume,
            if self.cover_flow { "covers" } else { "list" },
            if self.album_sort == AlbumSort::Az { "az" } else { "release" },
            self.repeat.label(),
            self.dim_timeout_secs,
            self.dim_brightness,
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
    /// Now Playing is an independent Music section; browsing never stops audio.
    now_playing_view: bool,
    /// Session-only hold switch (VolUp + VolDown chord); never persisted.
    locked: bool,
    discard_audio: bool,
    /// Cover Flow is the default root album browser. X toggles the former list.
    cover_flow: bool,
    settings: MusicSettings,
    settings_view: bool,
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
        // Enter Music on Now Playing. B opens the album browser if nothing plays.
        music.now_playing_view = true;
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
            now_playing_view: false,
            locked: false,
            discard_audio: false,
            cover_flow: true,
            settings: MusicSettings::default(),
            settings_view: false,
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
    /// Optional SD card root (the Slot directory), used for offline Music UI assets.
    pub(crate) fn icon_root(&self) -> Option<&Path> { self.settings_root.as_deref() }
    fn effective_volume(&self, requested: u8) -> u8 {
        requested.min(self.settings.max_volume)
    }
    pub(crate) fn music_settings_open(&self) -> bool { self.settings_view }
    pub(crate) fn locked(&self) -> bool { self.locked }

    /// Only the Music mute chord toggles the hold switch; locking never
    /// pauses, seeks, mutes or changes the playback position.
    pub(crate) fn toggle_lock(&mut self) {
        self.locked = !self.locked;
    }
    pub(crate) fn settings_cursor(&self) -> usize { self.settings_row }
    pub(crate) fn settings_max_volume(&self) -> u8 { self.settings.max_volume }
    pub(crate) fn settings_album_view(&self) -> &'static str {
        if self.settings.cover_flow { "cover flow" } else { "list" }
    }
    pub(crate) fn settings_sort(&self) -> &'static str { self.settings.album_sort.label() }
    pub(crate) fn dim_timeout_secs(&self) -> u16 { self.settings.dim_timeout_secs }
    pub(crate) fn dim_brightness(&self) -> u8 { self.settings.dim_brightness }
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
        self.playing() && !self.settings_view && self.settings.dim_timeout_secs > 0
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
            5 => {
                let level = self.settings.dim_brightness;
                self.settings.dim_brightness = if forward {
                    level.saturating_add(1).min(BRIGHTNESS_MAX)
                } else {
                    level.saturating_sub(1).max(1)
                };
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

    /// Up/Down on Albums Cover Flow jump to the next/previous *available*
    /// title initial (A, B, ...), not just the next card. Skip empty letters
    /// and wrap at the ends. Never reorder albums to make this work: the
    /// current A-Z/release sort and playback identities must remain intact.
    fn jump_album_letter(&mut self, forward: bool) {
        if !self.cover_flow() || self.album_order.is_empty() {
            return;
        }
        let mut starts = std::collections::BTreeMap::<char, usize>::new();
        for (row, &album) in self.album_order.iter().enumerate() {
            let initial = slot_store::initial(&self.library.albums[album].title);
            starts.entry(initial).or_insert(row);
        }
        if starts.len() < 2 {
            return;
        }
        let current_album = self.album_order[self.cursor().min(self.album_order.len() - 1)];
        let current = slot_store::initial(&self.library.albums[current_album].title);
        let letters: Vec<char> = starts.keys().copied().collect();
        let position = letters.iter().position(|&letter| letter == current).unwrap_or(0);
        let next = if forward {
            (position + 1) % letters.len()
        } else {
            (position + letters.len() - 1) % letters.len()
        };
        self.select_row(starts[&letters[next]]);
    }

    /// When browsing the Artists root, D-pad Left/Right jumps between
    /// available starting letters. Keep the artist sort/album membership as
    /// scanned, and preserve the cursor within the Artists tab.
    fn jump_artist_letter(&mut self, forward: bool) {
        if !self.is_root() || !self.viewing_artists() { return; }
        let mut starts = std::collections::BTreeMap::<char, usize>::new();
        for (row, artist) in self.library.artists.iter().enumerate() {
            starts.entry(slot_store::initial(&artist.name)).or_insert(row);
        }
        if starts.len() < 2 { return; }
        let current = slot_store::initial(&self.library.artists[
            self.cursor().min(self.library.artists.len() - 1)
        ].name);
        let letters: Vec<char> = starts.keys().copied().collect();
        let position = letters.iter().position(|&letter| letter == current).unwrap_or(0);
        let next = if forward {
            (position + 1) % letters.len()
        } else {
            (position + letters.len() - 1) % letters.len()
        };
        self.select_row(starts[&letters[next]]);
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
            && !self.now_playing_view && !self.settings_view
    }

    pub(crate) fn showing_now_playing(&self) -> bool { self.now_playing_view }

    /// B from Now Playing opens the album the current track belongs to,
    /// regardless of which artist or album was previously being browsed.
    /// The root cursor follows the sorted album order so a second B
    /// highlights the same album. If nothing is playing, B opens Albums.
    fn browse_playing_album(&mut self) {
        self.now_playing_view = false;
        let root_cursor = self.track.as_ref().and_then(|track| {
            self.album_order.iter().position(|&album| album == track.album)
        }).unwrap_or(0);
        self.browse = vec![Location {
            page: Page::Albums,
            cursor: root_cursor,
        }];
        if let Some(track) = &self.track {
            self.browse.push(Location {
                page: Page::Album(track.album),
                cursor: track.index,
            });
        }
    }

    /// All four Music sections are peers; shoulders wrap in both directions.
    /// Enter Albums/Artists at their roots and Settings at its first row.
    /// Unlike switching sections, B from Now Playing opens the playing album.
    fn navigate_section(&mut self, forward: bool) {
        let current = if self.settings_view {
            3
        } else if self.now_playing_view {
            0
        } else if matches!(self.browse[0].page, Page::Artists) {
            2
        } else {
            1
        };
        let next = (current + if forward { 1 } else { 3 }) % 4;
        self.now_playing_view = next == 0;
        self.settings_view = next == 3;
        if self.settings_view {
            self.settings_row = 0;
        }
        let page = match next {
            1 => Some(Page::Albums),
            2 => Some(Page::Artists),
            _ => None,
        };
        if let Some(page) = page {
            self.browse = vec![Location { page, cursor: 0 }];
        }
    }

    pub(crate) fn showing_tracks(&self) -> bool {
        matches!(self.location().page, Page::Album(_))
    }

    pub(crate) fn song(&self, track: &Track) -> &Song {
        &self.library.albums[track.album].songs[track.index]
    }

    /// The playing album is stable even when the browser opens another album.
    pub(crate) fn playing_album(&self) -> &Album {
        let album = self.track.as_ref().expect("playing track").album;
        &self.library.albums[album]
    }

    /// The browser's current album may differ from the playing album.
    pub(crate) fn browsed_album(&self) -> &Album {
        let Page::Album(album) = self.location().page else {
            unreachable!("track list has a browsed album")
        };
        &self.library.albums[album]
    }

    pub(crate) fn playing_position(&self) -> Option<(usize, usize)> {
        let track = self.track.as_ref()?;
        Some((track.index, self.library.albums[track.album].songs.len()))
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

    /// User-initiated play opens Now Playing but retains the browser stack.
    fn start(&mut self, index: usize) {
        let album = self.selected_album().expect("song belongs to an album");
        // Keep the browser's chosen track, even after returning from playback.
        self.select_row(index);
        self.start_album(album, index);
        self.now_playing_view = true;
    }

    /// Automatic advance and skipping keep the current view unchanged.
    fn start_album(&mut self, album: usize, index: usize) {
        // Preserve a user's independent album-list selection while browsing.
        // The old cursor follows playback only while Now Playing is visible.
        if self.now_playing_view
            && matches!(self.location().page, Page::Album(selected) if selected == album)
        {
            self.select_row(index);
        }
        self.track = Some(Track {
            album,
            index,
            status: Status::Loading,
            playback: None,
            ending: false,
            pending_seek_seconds: 0,
        });
        self.discard_audio = true;
    }

    fn switch(&mut self, index: usize, forward: bool) {
        let Some(album) = self.track.as_ref().map(|track| track.album) else { return };
        if let Some(next) = index
            .checked_add_signed(if forward { 1 } else { -1 })
            .filter(|&i| i < self.library.albums[album].songs.len())
        {
            self.start_album(album, next);
        }
    }

    fn advance_at_end(&mut self, index: usize) {
        let Some(album) = self.track.as_ref().map(|track| track.album) else { return };
        match self.settings.repeat {
            RepeatMode::One => self.start_album(album, index),
            RepeatMode::All if index + 1 == self.library.albums[album].songs.len() => {
                self.start_album(album, 0)
            }
            _ => self.switch(index, true),
        }
    }

    /// True when the browser should return to Slot's menu.
    pub(crate) fn input(&mut self, action: Action) -> bool {
        if action == Action::QuickMenu { return true; }
        // All sections, including Settings, share the same tab navigation.
        // Start is intentionally unmapped; HOME/Menu alone exits Music.
        match action {
            Action::GbaDown(Btn::L1) => {
                self.navigate_section(false);
                return false;
            }
            Action::GbaDown(Btn::R1) => {
                self.navigate_section(true);
                return false;
            }
            _ => {}
        }
        if self.settings_view {
            match action {
                Action::GbaDown(Btn::Up) => {
                    self.settings_row = self.settings_row.saturating_sub(1);
                }
                Action::GbaDown(Btn::Down) => {
                    self.settings_row = (self.settings_row + 1).min(5);
                }
                Action::GbaDown(Btn::A) | Action::GbaDown(Btn::Right) => {
                    self.change_setting(true);
                }
                Action::GbaDown(Btn::Left) => self.change_setting(false),
                // B at a section root is inert; only HOME/Menu leaves Music.
                _ => {}
            }
            return false;
        }
        if self.now_playing_view {
            if action == Action::GbaDown(Btn::B) {
                self.browse_playing_album();
                return false;
            }
            if let Some(track) = &mut self.track {
                let (album, index) = (track.album, track.index);
                match action {
                    Action::GbaDown(Btn::A) => match track.status {
                        Status::Playing => track.status = Status::Paused,
                        Status::Paused => track.status = Status::Playing,
                        Status::Finished | Status::Unreadable | Status::NoAudio => {
                            self.start_album(album, index)
                        }
                        Status::Loading => {}
                    },
                    // Left/right select tracks. Up/down seek by ten seconds
                    // without interrupting play/pause state or changing tabs.
                    Action::GbaDown(Btn::Left) | Action::ShelfLeft => {
                        self.switch(index, false)
                    }
                    Action::GbaDown(Btn::Right) | Action::ShelfRight => {
                        self.switch(index, true)
                    },
                    Action::GbaDown(Btn::Up) | Action::GbaDown(Btn::Down)
                        if matches!(track.status, Status::Playing | Status::Paused | Status::Loading) => {
                        let by = if action == Action::GbaDown(Btn::Up) { 10 } else { -10 };
                        track.pending_seek_seconds =
                            track.pending_seek_seconds.saturating_add(by).clamp(-3600, 3600);
                    },
                    Action::GbaDown(Btn::X) => {
                        self.settings.repeat = self.settings.repeat.step(true);
                        self.persist_settings();
                    }
                    _ => {}
                }
            }
            // Without a track, D-pad input and A do nothing. The B handler
            // above opens Albums; shoulder buttons switch sections earlier.
            return false;
        }
        match action {
            // Cover Flow scrolls albums horizontally; L1/R1 cycle sections
            // before these browsing controls. X restores list browsing.
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
            Action::GbaDown(Btn::Up) if self.cover_flow() => self.jump_album_letter(false),
            Action::GbaDown(Btn::Down) if self.cover_flow() => self.jump_album_letter(true),
            // The album list, Artists and track lists still move one row at a time.
            Action::GbaDown(Btn::Up) => self.select_row(self.cursor().saturating_sub(1)),
            Action::GbaDown(Btn::Down) if self.cursor() + 1 < self.row_count() => {
                self.select_row(self.cursor() + 1)
            }
            // In Artists the D-pad skips alphabet groups, never Music tabs.
            // L1/R1 exclusively change tabs, even in Albums list mode.
            Action::GbaDown(Btn::Left) | Action::ShelfLeft
                if self.is_root() && self.viewing_artists() => self.jump_artist_letter(false),
            Action::GbaDown(Btn::Right) | Action::ShelfRight
                if self.is_root() && self.viewing_artists() => self.jump_artist_letter(true),
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
            // B is strictly navigation inside Music. Only HOME/Menu exits.
            Action::GbaDown(Btn::B) if self.is_root() => {},
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
        let volume = self.effective_volume(level);
        let Some(track) = &mut self.track else { return };
        let album = track.album;
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
        // Resolve queued seeking against the *audible* playback position.
        // This also works while paused, but never implicitly resumes a song.
        if matches!(track.status, Status::Playing | Status::Paused) {
            let seconds = std::mem::take(&mut track.pending_seek_seconds);
            if seconds != 0 {
                if let Some(playback) = track.playback.as_mut() {
                    if let Some(at_end) = playback.seek_by(seconds, ring) {
                        track.ending = at_end;
                    }
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
    fn now_playing_up_down_queue_ten_second_seeks_without_changing_track_or_pause() {
        let album = Album {
            title: "Album".into(), artist: "Artist".into(), release_date: None,
            songs: vec![Song {
                path: PathBuf::from("missing.mp3"), title: "Track".into(),
                artist: "Artist".into(), disc: 1, track: Some(1),
            }],
        };
        let mut music = Music::with_library(Library {
            albums: vec![album], artists: vec![],
        });
        music.open_page(Page::Album(0));
        music.start(0);
        assert_eq!(music.track.as_ref().unwrap().pending_seek_seconds, 0);
        music.input(Action::GbaDown(Btn::Up));
        music.input(Action::GbaDown(Btn::Up));
        music.input(Action::GbaDown(Btn::Down));
        assert_eq!(music.track.as_ref().unwrap().pending_seek_seconds, 10);
        assert_eq!(music.playing_position(), Some((0, 1)));
        // The selected track was Loading; preserve queued input until audio opens.
        assert_eq!(music.track.as_ref().unwrap().status, Status::Loading);
        music.track.as_mut().unwrap().status = Status::Paused;
        music.input(Action::GbaDown(Btn::Down));
        assert_eq!(music.track.as_ref().unwrap().pending_seek_seconds, 0);
        assert_eq!(music.track.as_ref().unwrap().status, Status::Paused);
        music.input(Action::GbaDown(Btn::Up));
        assert_eq!(music.track.as_ref().unwrap().pending_seek_seconds, 10);
        music.input(Action::GbaDown(Btn::Left));
        assert_eq!(music.playing_position(), Some((0, 1)),
            "seeking does not change the track or album");
        music.input(Action::GbaDown(Btn::B));
        assert_eq!(music.track.as_ref().unwrap().pending_seek_seconds, 10,
            "browsing other sections does not flush the queued seek");
        music.input(Action::GbaDown(Btn::L1));
        assert!(music.showing_now_playing());
    }

    #[test]
    fn ten_second_seek_changes_decoder_position_and_preserves_pause_state() {
        let root = tempfile::tempdir().unwrap();
        let album = root.path().join("Music/Album");
        fs::create_dir_all(&album).unwrap();
        let song = album.join("01.mp3");
        silent_mp3(&song, 2);
        // The existing test helper makes only ~0.3s of audio; repeat those
        // valid frames to let +10/-10s be tested against real decoder data.
        let frames = fs::read(&song).unwrap();
        fs::write(&song, frames.repeat(120)).unwrap();
        let mut music = test_music(root.path());
        music.input(Action::GbaDown(Btn::A)); // Album -> tracks
        music.input(Action::GbaDown(Btn::Y)); // start song -> Now Playing
        let ring = Ring::new(0);
        ring.reopen(48_000);
        ring.set_idle(true);
        music.update_audio(&ring, 70);
        assert_eq!(music.track.as_ref().unwrap().status, Status::Playing);
        assert!(music.track.as_ref().unwrap().time().unwrap().1.unwrap() > 30.0);
        music.input(Action::GbaDown(Btn::Up));
        music.update_audio(&ring, 70);
        let elapsed = music.track.as_ref().unwrap().time().unwrap().0;
        assert!((9.0..13.0).contains(&elapsed),
            "Up should seek forward about ten seconds: {elapsed}");
        music.input(Action::GbaDown(Btn::A)); // pause
        assert_eq!(music.track.as_ref().unwrap().status, Status::Paused);
        music.input(Action::GbaDown(Btn::Down));
        music.update_audio(&ring, 70);
        let back = music.track.as_ref().unwrap().time().unwrap().0;
        assert!(back < 3.0, "Down should seek backwards without resuming: {back}");
        assert_eq!(music.track.as_ref().unwrap().status, Status::Paused);
        assert_eq!(ring.queued_frames(), 0,
            "a seek while paused must discard samples from the old position");
        music.input(Action::GbaDown(Btn::A));
        music.update_audio(&ring, 70);
        assert!(music.playing(), "playback can resume from the new seek point");
    }

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
        prefs.dim_brightness = 2;
        prefs.save(dir.path());
        let loaded = MusicSettings::load(dir.path());
        assert_eq!(loaded.max_volume, 40);
        assert!(!loaded.cover_flow);
        assert_eq!(loaded.album_sort, AlbumSort::Release);
        assert_eq!(loaded.repeat, RepeatMode::One);
        assert_eq!(loaded.dim_timeout_secs, 120);
        assert_eq!(loaded.dim_brightness, 2);
        // Existing music.settings files without the new key use level 1.
        std::fs::write(
            MusicSettings::path(dir.path()),
            "max_volume=255\ndim_brightness=0\n",
        ).unwrap();
        let invalid = MusicSettings::load(dir.path());
        assert_eq!(invalid.max_volume, 70);
        assert_eq!(invalid.dim_brightness, 1);
        std::fs::write(MusicSettings::path(dir.path()), "dim_brightness=10\n").unwrap();
        assert_eq!(MusicSettings::load(dir.path()).dim_brightness, 1);
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
        // Opening an album is browsing, not playback. Assert the browsed
        // album first, then explicitly start the selected song.
        music.input(Action::GbaDown(Btn::A));
        assert_eq!(music.browsed_album().title, "Gamma");
        assert!(music.track.is_none());
        music.input(Action::GbaDown(Btn::A));
        assert_eq!(music.playing_album().title, "Gamma");
        assert!(music.showing_now_playing());
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
        // From the Albums root, R1 goes to Artists, then Settings.
        music.input(Action::GbaDown(Btn::Start));
        assert!(!music.music_settings_open(), "Start is no longer a settings shortcut");
        music.input(Action::GbaDown(Btn::R1));
        assert!(!music.music_settings_open());
        music.input(Action::GbaDown(Btn::R1));
        assert!(music.music_settings_open());
        assert_eq!(music.settings_cursor(), 0);
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
        music.input(Action::GbaDown(Btn::Down));
        music.input(Action::GbaDown(Btn::Right));
        assert_eq!(music.dim_brightness(), 2);
        music.input(Action::GbaDown(Btn::B));
        assert!(music.music_settings_open(), "B cannot leave a root tab");
        music.input(Action::GbaDown(Btn::Start));
        assert!(music.music_settings_open(), "Start must remain unmapped");
        music.input(Action::GbaDown(Btn::L1));
        assert!(!music.music_settings_open(), "L1 returns to Artists");
        assert_eq!(music.label(), "artists");

        let loaded = MusicSettings::load(root.path());
        assert_eq!(loaded.max_volume, 60);
        assert_eq!(loaded.album_sort, AlbumSort::Release);
        assert!(!loaded.cover_flow);
        assert_eq!(loaded.repeat, RepeatMode::All);
        assert_eq!(loaded.dim_timeout_secs, 120);
        assert_eq!(loaded.dim_brightness, 2);
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
    fn dim_brightness_stays_within_the_nonzero_hardware_range() {
        let mut music = Music::with_library(Library::default());
        music.settings_row = 5;
        assert_eq!(music.dim_brightness(), 1);
        music.change_setting(false);
        assert_eq!(music.dim_brightness(), 1);
        for _ in 0..20 { music.change_setting(true); }
        assert_eq!(music.dim_brightness(), BRIGHTNESS_MAX);
        music.change_setting(true);
        assert_eq!(music.dim_brightness(), BRIGHTNESS_MAX);
        music.change_setting(false);
        assert_eq!(music.dim_brightness(), BRIGHTNESS_MAX - 1);
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
    fn artists_root_left_right_jump_available_initials_not_albums_tab() {
        use crate::music_library::Artist;
        let names = ["#mix", "Amber", "Ari", "Byron", "Dora", "Zane"];
        let mut music = Music::with_library(Library {
            albums: vec![],
            artists: names.iter().map(|name| Artist {
                name: (*name).into(), albums: vec![],
            }).collect(),
        });
        music.browse[0].page = Page::Artists;
        assert_eq!(music.label(), "artists");
        music.input(Action::GbaDown(Btn::Right));
        assert_eq!(music.row(music.cursor()).0, "Amber");
        music.input(Action::GbaDown(Btn::Down)); // Within A, not a tab switch
        assert_eq!(music.row(music.cursor()).0, "Ari");
        music.input(Action::GbaDown(Btn::Right));
        assert_eq!(music.row(music.cursor()).0, "Byron");
        music.input(Action::ShelfRight);
        assert_eq!(music.row(music.cursor()).0, "Dora",
            "alphabet navigation must skip letters without artists");
        music.input(Action::GbaDown(Btn::Left));
        assert_eq!(music.row(music.cursor()).0, "Byron");
        music.input(Action::GbaDown(Btn::Left));
        assert_eq!(music.row(music.cursor()).0, "Amber");
        music.input(Action::GbaDown(Btn::Left));
        assert_eq!(music.row(music.cursor()).0, "#mix");
        music.input(Action::GbaDown(Btn::Left));
        assert_eq!(music.row(music.cursor()).0, "Zane",
            "previous wraps to the last available group");
        music.input(Action::GbaDown(Btn::Right));
        assert_eq!(music.row(music.cursor()).0, "#mix",
            "next wraps to the first available group");
        assert_eq!(music.label(), "artists", "left/right cannot change Music tab");
        music.input(Action::GbaDown(Btn::L1));
        assert_eq!(music.label(), "albums", "L1 still controls tabs");
    }

    #[test]
    fn artists_alphabet_jumps_ignore_empty_and_one_letter_sets() {
        use crate::music_library::Artist;
        let mut empty = Music::with_library(Library::default());
        empty.browse[0].page = Page::Artists;
        empty.input(Action::GbaDown(Btn::Right));
        assert_eq!(empty.cursor(), 0);
        let mut one = Music::with_library(Library {
            albums: vec![],
            artists: ["Amber", "Ari"].iter().map(|name| Artist {
                name: (*name).into(), albums: vec![],
            }).collect(),
        });
        one.browse[0].page = Page::Artists;
        one.select_row(1);
        one.input(Action::GbaDown(Btn::Left));
        one.input(Action::GbaDown(Btn::Right));
        assert_eq!(one.cursor(), 1, "single-initial artists must not reset cursor");
    }

    #[test]
    fn session_lock_is_not_a_saved_music_setting_or_a_pause_action() {
        let mut music = Music::with_library(Library::default());
        assert!(!music.locked());
        music.toggle_lock();
        assert!(music.locked());
        assert!(!music.playing());
        music.toggle_lock();
        assert!(!music.locked());
    }

    #[test]
    fn cover_flow_up_down_jumps_available_letters_and_wraps() {
        let make_album = |title: &str| Album {
            title: title.into(), artist: "artist".into(), release_date: None,
            songs: vec![Song {
                path: PathBuf::from(format!("{title}.mp3")),
                title: "song".into(), artist: "artist".into(),
                disc: 1, track: Some(1),
            }],
        };
        let mut music = Music::with_library(Library {
            albums: ["#Hits", "Alpha", "Alpine", "Bravo", "Delta"]
                .iter().map(|title| make_album(title)).collect(),
            artists: vec![],
        });
        assert!(music.cover_flow());
        // Use the ordered titles, not the original library storage positions.
        assert_eq!(music.row(music.cursor()).0, "#Hits");
        music.input(Action::GbaDown(Btn::Down));
        assert_eq!(music.row(music.cursor()).0, "Alpha");
        music.input(Action::GbaDown(Btn::Right));
        assert_eq!(music.row(music.cursor()).0, "Alpine",
            "Left/Right still move a single cover");
        music.input(Action::GbaDown(Btn::Down));
        assert_eq!(music.row(music.cursor()).0, "Bravo",
            "skip the remaining A titles and unavailable letters");
        music.input(Action::GbaDown(Btn::Down));
        assert_eq!(music.row(music.cursor()).0, "Delta");
        music.input(Action::GbaDown(Btn::Down));
        assert_eq!(music.row(music.cursor()).0, "#Hits",
            "Down wraps to the first available initial");
        music.input(Action::GbaDown(Btn::Up));
        assert_eq!(music.row(music.cursor()).0, "Delta",
            "Up wraps to the last available initial");
        music.input(Action::GbaDown(Btn::Up));
        assert_eq!(music.row(music.cursor()).0, "Bravo");
        music.input(Action::GbaDown(Btn::X)); // cover flow -> album list
        assert!(!music.cover_flow());
        music.input(Action::GbaDown(Btn::Up));
        assert_eq!(music.row(music.cursor()).0, "Alpine",
            "Up returns to single-row navigation in list mode");
    }

    #[test]
    fn cover_flow_letter_jumps_respect_release_sort_without_reordering_albums() {
        let make_album = |title: &str, date| Album {
            title: title.into(), artist: "artist".into(), release_date: Some(date),
            songs: vec![Song {
                path: PathBuf::from(format!("{title}.mp3")),
                title: "song".into(), artist: "artist".into(),
                disc: 1, track: Some(1),
            }],
        };
        let mut music = Music::with_library(Library {
            albums: vec![
                make_album("Alpha", 20200101), make_album("Beta", 20260101),
                make_album("Alpine", 20250101), make_album("Delta", 20240101),
            ],
            artists: vec![],
        });
        music.settings_row = 2;
        music.change_setting(true);
        assert_eq!(music.settings.album_sort, AlbumSort::Release);
        assert_eq!(music.row(0).0, "Beta");
        // Under release sort, Down still means next alphabetically available
        // letter, while the album rows themselves remain ordered by date.
        music.input(Action::GbaDown(Btn::Down));
        assert_eq!(music.row(music.cursor()).0, "Delta");
        music.input(Action::GbaDown(Btn::Down));
        assert_eq!(music.row(music.cursor()).0, "Alpine",
            "choose the first visible A album, not the oldest or a re-sort");
        assert_eq!(music.row(0).0, "Beta", "release order must be unchanged");
        music.input(Action::GbaDown(Btn::Up));
        assert_eq!(music.row(music.cursor()).0, "Delta");
    }

    #[test]
    fn cover_flow_letter_jumps_are_inert_for_empty_and_one_letter_libraries() {
        let mut empty = Music::with_library(Library::default());
        for action in [Btn::Up, Btn::Down] {
            assert!(!empty.input(Action::GbaDown(action)));
            assert_eq!(empty.cursor(), 0);
        }
        let mk = |name: &str| Album {
            title: name.into(), artist: "artist".into(), release_date: None,
            songs: vec![Song {
                path: PathBuf::from(format!("{name}.mp3")),
                title: "song".into(), artist: "artist".into(),
                disc: 1, track: Some(1),
            }],
        };
        let mut one_letter = Music::with_library(Library {
            albums: vec![mk("Alpha"), mk("Alpine")], artists: vec![],
        });
        one_letter.input(Action::GbaDown(Btn::Right));
        assert_eq!(one_letter.cursor(), 1);
        one_letter.input(Action::GbaDown(Btn::Up));
        one_letter.input(Action::GbaDown(Btn::Down));
        assert_eq!(one_letter.cursor(), 1,
            "do not unexpectedly reset selection if every album shares an initial");
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
        assert!(!music.viewing_artists(),
            "D-pad cannot switch tabs even in the album list");
        music.input(Action::GbaDown(Btn::R1));
        assert!(music.viewing_artists(), "R1 is now the only right tab switch");
        music.input(Action::GbaDown(Btn::L1));
        assert!(!music.cover_flow() && !music.viewing_artists());
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
        // The list view no longer switches tabs with the D-pad. L1/R1
        // alone switch sections; artists root uses left/right for letters.
        music.input(Action::GbaDown(Btn::X));
        assert!(!music.cover_flow());
        music.input(Action::GbaDown(Btn::Right));
        assert_eq!(music.label(), "albums");
        music.input(Action::GbaDown(Btn::R1));
        assert_eq!(music.label(), "artists");
        music.input(Action::GbaDown(Btn::Left));
        assert_eq!(music.label(), "artists", "Left must stay within Artists");
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
        music.input(Action::GbaDown(Btn::Right));
        music.input(Action::GbaDown(Btn::Right));
        let track = music.track.as_ref().unwrap();
        assert_eq!(music.song(track).title, "Hop");
        assert_eq!(music.song(track).artist, "Émilie");
        music.input(Action::GbaDown(Btn::B));
        assert!(music.showing_tracks());
        assert_eq!(music.cursor(), 2, "B highlights the currently playing track");
        assert_eq!(music.browsed_album().title, "Mixtape");
        music.input(Action::GbaDown(Btn::B));
        assert_eq!(music.label(), "albums");
        assert_eq!(music.row(music.cursor()).0, "Mixtape",
            "album root stays selected on the playing album");
        assert!(!music.input(Action::GbaDown(Btn::B)));
        assert_eq!(music.label(), "albums", "B at the root must not leave Music");
    }

    #[test]
    fn empty_now_playing_ignores_dpad_and_a_and_only_shoulders_switch_sections() {
        let root = tempfile::tempdir().unwrap();
        let mut music = Music::open(root.path());
        assert!(music.showing_now_playing());
        assert!(music.track.is_none());
        for action in [
            Action::GbaDown(Btn::Left),
            Action::GbaDown(Btn::Right),
            Action::GbaDown(Btn::Up),
            Action::GbaDown(Btn::Down),
            Action::ShelfLeft,
            Action::ShelfRight,
            Action::GbaDown(Btn::A),
        ] {
            assert!(!music.input(action));
            assert!(music.showing_now_playing(), "{action:?} left empty Now Playing");
            assert!(music.track.is_none());
        }
        music.input(Action::GbaDown(Btn::R1));
        assert!(!music.showing_now_playing());
        assert_eq!(music.label(), "albums");
        music.input(Action::GbaDown(Btn::L1));
        assert!(music.showing_now_playing());
        assert!(music.track.is_none());
    }

    #[test]
    fn music_opens_on_now_playing_and_b_opens_the_album_browser() {
        let root = tempfile::tempdir().unwrap();
        let mut music = Music::open(root.path());
        assert!(music.showing_now_playing());
        assert!(!music.input(Action::GbaDown(Btn::B)));
        assert!(!music.showing_now_playing());
        assert_eq!(music.label(), "albums");
        music.input(Action::GbaDown(Btn::L1));
        assert!(music.showing_now_playing(), "Albums -> Now Playing via L1");
        music.input(Action::GbaDown(Btn::B));
        music.input(Action::GbaDown(Btn::R1));
        assert_eq!(music.label(), "artists");
        music.input(Action::GbaDown(Btn::L1));
        assert_eq!(music.label(), "albums");
    }

    #[test]
    fn shoulders_cycle_all_four_sections_without_skipping_tracks() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("Music");
        std::fs::create_dir_all(&folder).unwrap();
        silent_mp3(&folder.join("01.mp3"), 2);
        silent_mp3(&folder.join("02.mp3"), 2);
        let mut music = test_music(root.path());
        music.input(Action::GbaDown(Btn::Y));
        assert!(music.showing_now_playing());
        assert_eq!(music.playing_position(), Some((0, 2)));

        music.input(Action::GbaDown(Btn::R1)); // now playing -> albums
        assert!(!music.showing_now_playing());
        assert_eq!(music.label(), "albums");
        assert_eq!(music.playing_position(), Some((0, 2)));
        music.input(Action::GbaDown(Btn::R1)); // albums -> artists
        assert_eq!(music.label(), "artists");
        music.input(Action::GbaDown(Btn::R1)); // artists -> settings
        assert!(music.music_settings_open());
        assert!(!music.showing_now_playing());
        assert_eq!(music.playing_position(), Some((0, 2)));
        music.input(Action::GbaDown(Btn::R1)); // settings -> now playing
        assert!(music.showing_now_playing());
        assert!(!music.music_settings_open());
        assert_eq!(music.playing_position(), Some((0, 2)));

        music.input(Action::GbaDown(Btn::L1)); // now playing -> settings
        assert!(music.music_settings_open());
        assert!(!music.showing_now_playing());
        music.input(Action::GbaDown(Btn::L1)); // settings -> artists
        assert_eq!(music.label(), "artists");
        music.input(Action::GbaDown(Btn::L1)); // artists -> albums
        assert_eq!(music.label(), "albums");
        music.input(Action::GbaDown(Btn::L1)); // albums -> now playing
        assert!(music.showing_now_playing());

        music.input(Action::GbaDown(Btn::Right)); // skip to song two
        assert_eq!(music.playing_position(), Some((1, 2)));
        music.input(Action::GbaDown(Btn::Left)); // skip to song one
        assert_eq!(music.playing_position(), Some((0, 2)));
        music.input(Action::GbaDown(Btn::L1));
        assert_eq!(music.playing_position(), Some((0, 2)));
    }

    #[test]
    fn settings_is_a_root_tab_and_never_interrupts_playback() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("Music");
        std::fs::create_dir_all(&folder).unwrap();
        silent_mp3(&folder.join("01.mp3"), 2);
        let mut music = test_music(root.path());
        let ring = Ring::new(0);
        ring.reopen(48_000);
        ring.set_idle(true);
        music.input(Action::GbaDown(Btn::Y));
        music.update_audio(&ring, 60);
        assert!(music.playing());
        let path = music.song(music.track.as_ref().unwrap()).path.clone();
        music.input(Action::GbaDown(Btn::L1));
        assert!(music.music_settings_open(), "L1 from Now Playing wraps to Settings");
        assert_eq!(music.settings_cursor(), 0);
        music.input(Action::GbaDown(Btn::Down));
        assert_eq!(music.settings_cursor(), 1);
        music.input(Action::GbaDown(Btn::B));
        music.input(Action::GbaDown(Btn::Start));
        assert!(music.music_settings_open(), "B and Start stay inside the Settings tab");
        music.update_audio(&ring, 60);
        assert!(music.playing());
        assert_eq!(music.song(music.track.as_ref().unwrap()).path, path);
        music.input(Action::GbaDown(Btn::R1));
        assert!(music.showing_now_playing(), "R1 wraps from Settings to Now Playing");
        assert!(!music.music_settings_open());
        assert!(music.playing());
        assert!(music.input(Action::QuickMenu),
            "HOME/Menu is still the only exit from Music");
    }

    #[test]
    fn section_switch_from_now_playing_enters_albums_root_not_song_list() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("Music");
        std::fs::create_dir_all(&folder).unwrap();
        silent_mp3(&folder.join("01.mp3"), 2);
        let mut music = test_music(root.path());
        music.input(Action::GbaDown(Btn::Y)); // opens the album list, then plays
        assert!(music.showing_now_playing());
        assert!(music.showing_tracks(), "playback retains its browser origin");
        assert_eq!(music.playing_position(), Some((0, 1)));
        music.input(Action::GbaDown(Btn::R1)); // Now Playing -> Albums
        assert!(!music.showing_now_playing());
        assert!(music.is_root(), "section must not open the song list");
        assert_eq!(music.label(), "albums");
        assert_eq!(music.playing_position(), Some((0, 1)), "audio state unchanged");
        music.input(Action::GbaDown(Btn::L1)); // Albums -> Now Playing
        assert!(music.showing_now_playing());
        assert_eq!(music.playing_position(), Some((0, 1)));
    }

    #[test]
    fn b_opens_the_playing_album_and_highlights_current_track_not_browsed_album() {
        let root = tempfile::tempdir().unwrap();
        for album in ["Alpha", "Beta"] {
            let folder = root.path().join("Music").join(album);
            std::fs::create_dir_all(&folder).unwrap();
            silent_mp3(&folder.join("01.mp3"), 2);
            silent_mp3(&folder.join("02.mp3"), 2);
        }
        let mut music = test_music(root.path());
        // Start the second track of Beta (row 1 in the sorted Albums root).
        music.select_row(1);
        music.input(Action::GbaDown(Btn::A));
        music.select_row(1);
        music.input(Action::GbaDown(Btn::A));
        assert!(music.showing_now_playing());
        assert_eq!(music.playing_album().title, "Beta");
        assert_eq!(music.playing_position(), Some((1, 2)));

        // Browse Alpha independently, then return to Now Playing.
        music.input(Action::GbaDown(Btn::R1)); // Albums root
        music.select_row(0);
        music.input(Action::GbaDown(Btn::A));
        assert_eq!(music.browsed_album().title, "Alpha");
        music.input(Action::GbaDown(Btn::L1)); // Now Playing
        assert!(music.showing_now_playing());

        music.input(Action::GbaDown(Btn::B));
        assert!(!music.showing_now_playing());
        assert!(music.showing_tracks());
        assert_eq!(music.browsed_album().title, "Beta",
            "B must use the active track's album, not the browsed album");
        assert_eq!(music.cursor(), 1, "highlight the actual playing song");
        assert_eq!(music.playing_position(), Some((1, 2)));
        music.input(Action::GbaDown(Btn::B));
        assert!(music.is_root());
        assert_eq!(music.label(), "albums");
        assert_eq!(music.row(music.cursor()).0, "Beta",
            "the album root highlights the playing album");
        assert!(!music.input(Action::GbaDown(Btn::B)));
        assert!(music.is_root(), "B never exits Music");
        assert!(music.input(Action::QuickMenu), "only HOME/Menu exits Music");
    }

    #[test]
    fn b_from_empty_now_playing_opens_albums_but_does_not_exit_music() {
        let root = tempfile::tempdir().unwrap();
        let mut music = Music::open(root.path());
        assert!(music.showing_now_playing());
        assert!(music.track.is_none());
        music.input(Action::GbaDown(Btn::B));
        assert_eq!(music.label(), "albums");
        assert!(music.is_root());
        assert!(!music.input(Action::GbaDown(Btn::B)));
        assert_eq!(music.label(), "albums");
        assert!(music.input(Action::QuickMenu));
    }

    #[test]
    fn shoulders_leave_nested_browser_pages_but_keep_the_active_song() {
        let make = |title: &str| Album {
            title: title.into(), artist: "artist".into(), release_date: None,
            songs: vec![Song {
                path: PathBuf::from(format!("{title}.mp3")),
                title: "song".into(), artist: "artist".into(), disc: 1,
                track: Some(1),
            }],
        };
        let mut music = Music::with_library(Library {
            albums: vec![make("Alpha"), make("Beta")],
            artists: vec![],
        });
        music.input(Action::GbaDown(Btn::A)); // inside Alpha track list
        music.input(Action::GbaDown(Btn::A)); // play Alpha
        music.input(Action::GbaDown(Btn::B)); // browse Alpha tracks
        music.input(Action::GbaDown(Btn::R1)); // Artists
        assert_eq!(music.label(), "artists");
        assert_eq!(music.playing_album().title, "Alpha");
        music.input(Action::GbaDown(Btn::R1)); // Settings
        assert!(music.music_settings_open());
        music.input(Action::GbaDown(Btn::R1)); // Now Playing
        assert!(music.showing_now_playing());
        music.input(Action::GbaDown(Btn::R1)); // Albums
        assert_eq!(music.label(), "albums");
        assert_eq!(music.playing_album().title, "Alpha");
    }

    #[test]
    fn browsing_other_albums_does_not_replace_or_interrupt_the_playing_song() {
        let root = tempfile::tempdir().unwrap();
        for name in ["Alpha", "Beta"] {
            let folder = root.path().join("Music").join(name);
            std::fs::create_dir_all(&folder).unwrap();
            silent_mp3(&folder.join("01.mp3"), 2);
        }
        let mut music = test_music(root.path());
        let ring = Ring::new(0);
        ring.reopen(48_000);
        ring.set_idle(true);
        music.input(Action::GbaDown(Btn::A)); // Alpha track list
        music.input(Action::GbaDown(Btn::Y)); // start Alpha -> Now Playing
        assert!(music.showing_now_playing());
        music.update_audio(&ring, 60);
        assert!(music.playing());
        let first_album = music.playing_album().title.clone();
        let first_path = music.song(music.track.as_ref().unwrap()).path.clone();
        music.input(Action::GbaDown(Btn::B)); // back to Alpha's tracks
        assert!(music.showing_tracks());
        music.select_row(0);
        music.update_audio(&ring, 60);
        assert_eq!(music.cursor(), 0, "browser selection must not follow queued audio");
        music.input(Action::GbaDown(Btn::B)); // album root
        music.input(Action::GbaDown(Btn::Down)); // select Beta
        music.input(Action::GbaDown(Btn::A)); // browse Beta without replacing playback
        assert_eq!(music.browsed_album().title, "Beta");
        music.update_audio(&ring, 60);
        assert_eq!(music.playing_album().title, first_album);
        assert_eq!(music.song(music.track.as_ref().unwrap()).path, first_path);
        assert!(music.playing(), "background browsing must keep the decoder alive");
        music.input(Action::GbaDown(Btn::B)); // root
        music.input(Action::GbaDown(Btn::L1)); // Now Playing tab
        assert!(music.showing_now_playing());
        music.input(Action::GbaDown(Btn::A)); // pause same track
        assert!(!music.playing());
        assert_eq!(music.song(music.track.as_ref().unwrap()).path, first_path);
        music.input(Action::GbaDown(Btn::A)); // resume
        assert!(music.playing());
    }

    #[test]
    fn repeat_advances_the_playing_album_even_when_another_album_is_open() {
        let album = |name: &str, count: usize| Album {
            title: name.into(),
            artist: "artist".into(),
            release_date: None,
            songs: (0..count).map(|index| Song {
                path: PathBuf::from(format!("{name}/{index}.mp3")),
                title: format!("{index}"),
                artist: "artist".into(),
                disc: 1,
                track: Some((index + 1) as u16),
            }).collect(),
        };
        let mut music = Music::with_library(Library {
            albums: vec![album("Alpha", 2), album("Beta", 1)],
            artists: vec![],
        });
        music.input(Action::GbaDown(Btn::A)); // Alpha
        music.input(Action::GbaDown(Btn::A)); // song 1 -> Now Playing
        music.input(Action::GbaDown(Btn::B));
        music.input(Action::GbaDown(Btn::B));
        music.input(Action::GbaDown(Btn::Down));
        music.input(Action::GbaDown(Btn::A)); // browsing Beta
        music.settings.repeat = RepeatMode::All;
        music.advance_at_end(0);
        assert_eq!(music.playing_position(), Some((1, 2)));
        assert_eq!(music.playing_album().title, "Alpha");
        assert_eq!(music.browsed_album().title, "Beta");
        assert!(!music.showing_now_playing());
        music.advance_at_end(1);
        assert_eq!(music.playing_position(), Some((0, 2)));
        assert_eq!(music.browsed_album().title, "Beta");
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
        music.input(Action::GbaDown(Btn::Left));
        music.update_audio(&ring, 80);
        assert_eq!(music.cursor(), 0);
        music.input(Action::GbaDown(Btn::B));
        music.update_audio(&ring, 80);
        assert!(music.track.is_some(), "B leaves Now Playing without stopping the song");
        assert!(!music.showing_now_playing());
        assert!(!music.input(Action::GbaDown(Btn::B)));
        assert!(music.is_root());
        assert!(!music.input(Action::GbaDown(Btn::B)));
        assert!(music.is_root());
        assert!(music.input(Action::QuickMenu),
            "HOME/Menu is the only way to leave Music");
    }

    #[test]
    fn dpad_left_right_change_tracks_without_seeking_and_up_down_do_nothing() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("Music");
        fs::create_dir(&folder).unwrap();
        silent_mp3(&folder.join("01.mp3"), 2);
        silent_mp3(&folder.join("02.mp3"), 2);

        let mut music = test_music(root.path());
        let ring = Ring::new(0);
        ring.reopen(48_000);
        music.input(Action::GbaDown(Btn::Y));
        music.update_audio(&ring, 80);
        assert_eq!(music.playing_position(), Some((0, 2)));

        music.input(Action::GbaDown(Btn::Right));
        music.update_audio(&ring, 80);
        assert_eq!(music.playing_position(), Some((1, 2)));
        music.input(Action::GbaDown(Btn::Right));
        assert_eq!(music.playing_position(), Some((1, 2)), "last track is the boundary");
        music.input(Action::GbaDown(Btn::Left));
        music.update_audio(&ring, 80);
        assert_eq!(music.playing_position(), Some((0, 2)));

        let time_before = music.track.as_ref().unwrap().time().unwrap().0;
        music.input(Action::GbaDown(Btn::Up));
        music.input(Action::GbaDown(Btn::Down));
        assert_eq!(music.playing_position(), Some((0, 2)));
        assert_eq!(music.track.as_ref().unwrap().time().unwrap().0, time_before,
            "up/down must neither skip nor seek");
        music.input(Action::GbaDown(Btn::A));
        assert!(!music.playing(), "A still pauses");
        music.input(Action::GbaDown(Btn::A));
        assert!(music.playing(), "A still resumes");
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
        music.input(Action::GbaDown(Btn::Right));
        music.update_audio(&ring, 80);
        assert!(music.playing());
        assert!(ring.queued_frames() > 0);
    }
}
