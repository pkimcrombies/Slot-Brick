use std::ffi::c_void;
use std::fs;
use std::path::{Path, PathBuf};
use std::ptr::NonNull;

use slot_input::{Action, Btn};

use crate::audio::{volume, Ring};
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
    fn slot_mp3_close(mp3: *mut c_void);
}

struct Decoder {
    raw: NonNull<c_void>,
    rate: u32,
    channels: usize,
    duration: u64,
}

// The handle owns a file and decoder with no thread affinity. Calls require exclusive access.
unsafe impl Send for Decoder {}

impl Decoder {
    fn open(path: &Path) -> Option<Self> {
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
            raw,
            rate,
            channels: channels as usize,
            duration,
        };
        (matches!(channels, 1 | 2) && (8_000..=48_000).contains(&rate)).then_some(decoder)
    }

    fn read_stereo(&mut self, samples: &mut [i16; CHUNK * 2]) -> usize {
        let frames = unsafe { slot_mp3_read(self.raw.as_ptr(), samples.as_mut_ptr(), CHUNK as u32) }
            as usize;
        if self.channels == 1 {
            for i in (0..frames).rev() {
                let sample = samples[i];
                samples[i * 2..i * 2 + 2].fill(sample);
            }
        }
        frames
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        unsafe { slot_mp3_close(self.raw.as_ptr()) };
    }
}

struct Playback {
    decoder: Decoder,
    resampler: Resampler,
    submitted: u64,
    output: Vec<i16>,
}

impl Playback {
    fn open(path: &Path, output_rate: u32) -> Option<Self> {
        let decoder = Decoder::open(path)?;
        Some(Self {
            resampler: Resampler::new(f64::from(decoder.rate), f64::from(output_rate)),
            decoder,
            submitted: 0,
            output: Vec::new(),
        })
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

pub(crate) struct Entry {
    path: PathBuf,
    pub(crate) name: String,
    pub(crate) folder: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
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

pub(crate) struct Music {
    base: PathBuf,
    folder: PathBuf,
    pub(crate) entries: Vec<Entry>,
    pub(crate) cursor: usize,
    pub(crate) track: Option<Track>,
    discard_audio: bool,
}

impl Music {
    pub(crate) fn open(slot: &Path) -> Self {
        let base = slot.join("Music");
        let mut music = Self {
            folder: base.clone(),
            base,
            entries: Vec::new(),
            cursor: 0,
            track: None,
            discard_audio: false,
        };
        music.list();
        music
    }

    fn list(&mut self) {
        self.entries = fs::read_dir(&self.folder)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let ty = entry.file_type().ok()?;
                let path = entry.path();
                if !ty.is_dir() && !(ty.is_file() && path.extension()?.eq_ignore_ascii_case("mp3"))
                {
                    return None;
                }
                let name = if ty.is_dir() {
                    path.file_name()?
                } else {
                    path.file_stem()?
                };
                Some(Entry {
                    name: name.to_string_lossy().into_owned(),
                    path,
                    folder: ty.is_dir(),
                })
            })
            .collect();
        self.entries.sort_by_cached_key(|entry| {
            (!entry.folder, entry.name.to_lowercase(), entry.name.clone())
        });
        self.cursor = 0;
    }

    pub(crate) fn folder_name(&self) -> &str {
        self.folder
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("music")
    }

    pub(crate) fn is_root(&self) -> bool {
        self.folder == self.base
    }

    pub(crate) fn track_name(&self, track: &Track) -> &str {
        &self.entries[track.index].name
    }

    pub(crate) fn has_tracks(&self) -> bool {
        self.entries.iter().any(|entry| !entry.folder)
    }

    pub(crate) fn playing(&self) -> bool {
        self.track
            .as_ref()
            .is_some_and(|track| matches!(track.status, Status::Loading | Status::Playing))
    }

    fn start(&mut self, index: usize) {
        self.cursor = index;
        self.track = Some(Track {
            index,
            status: Status::Loading,
            playback: None,
            ending: false,
        });
        self.discard_audio = true;
    }

    fn switch(&mut self, index: usize, forward: bool) {
        let adjacent = if forward {
            (index + 1..self.entries.len()).find(|&i| !self.entries[i].folder)
        } else {
            (0..index).rev().find(|&i| !self.entries[i].folder)
        };
        if let Some(next) = adjacent {
            self.start(next);
        }
    }

    /// True when the browser should return to Slot's menu.
    pub(crate) fn input(&mut self, action: Action) -> bool {
        if action == Action::QuickMenu {
            return true;
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
                Action::GbaDown(Btn::L1 | Btn::Left) | Action::ShelfLeft => {
                    self.switch(index, false)
                }
                Action::GbaDown(Btn::R1 | Btn::Right) | Action::ShelfRight => {
                    self.switch(index, true)
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
            Action::GbaDown(Btn::Up) => self.cursor = self.cursor.saturating_sub(1),
            Action::GbaDown(Btn::Down) if self.cursor + 1 < self.entries.len() => self.cursor += 1,
            Action::GbaDown(Btn::A) => {
                if let Some(entry) = self.entries.get(self.cursor) {
                    if entry.folder {
                        self.folder = entry.path.clone();
                        self.list();
                    } else {
                        self.start(self.cursor);
                    }
                }
            }
            Action::GbaDown(Btn::Y) => {
                if let Some(index) = self.entries.iter().position(|entry| !entry.folder) {
                    self.start(index);
                }
            }
            Action::GbaDown(Btn::B) if self.is_root() => return true,
            Action::GbaDown(Btn::B) => {
                let from = self.folder.clone();
                self.folder.pop();
                self.list();
                self.cursor = self
                    .entries
                    .iter()
                    .position(|entry| entry.path == from)
                    .unwrap_or(0);
            }
            _ => {}
        }
        false
    }

    pub(crate) fn update_audio(&mut self, ring: &Ring, level: u8) {
        if std::mem::take(&mut self.discard_audio) {
            ring.discard();
        }
        let Some(track) = &mut self.track else { return };
        if track.status == Status::Loading {
            if ring.sample_rate() == 0 {
                track.status = Status::NoAudio;
                return;
            }
            track.playback = Playback::open(&self.entries[track.index].path, ring.sample_rate());
            track.status = if track.playback.is_some() {
                Status::Playing
            } else {
                Status::Unreadable
            };
        }
        if track.status != Status::Playing {
            return;
        }
        if !track.ending {
            track.ending = !track
                .playback
                .as_mut()
                .expect("playing track has a decoder")
                .fill(ring, level);
        }
        if track.ending && ring.queued_frames() == 0 {
            let index = track.index;
            track.status = Status::Finished;
            self.switch(index, true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn silent_mp3(path: &Path, channels: u8) {
        // MPEG-1 Layer III, 128 kbit/s at 44.1 kHz. No coded coefficients means silence.
        let mut frame = [0; 417];
        frame[..4].copy_from_slice(&[0xff, 0xfb, 0x90, if channels == 1 { 0xc4 } else { 0x64 }]);
        fs::write(path, frame.repeat(12)).unwrap();
    }

    #[test]
    fn scans_and_plays_only_mp3s_in_the_current_folder() {
        let root = tempfile::tempdir().unwrap();
        let album = root.path().join("Music/An album");
        fs::create_dir_all(&album).unwrap();
        silent_mp3(&album.join("01 - first.MP3"), 1);
        silent_mp3(&album.join("02 - second.mp3"), 2);
        fs::write(album.join("notes.txt"), "ignored").unwrap();
        let mut music = Music::open(root.path());
        assert!(music.entries[0].folder);
        music.input(Action::GbaDown(Btn::A));
        assert_eq!(music.folder_name(), "An album");
        assert_eq!(music.entries.len(), 2);
        music.input(Action::GbaDown(Btn::Y));

        let ring = Ring::new(0);
        ring.reopen(48_000);
        ring.set_idle(true);
        music.update_audio(&ring, 80);
        assert!(music.playing());
        assert_eq!(
            music.track_name(music.track.as_ref().unwrap()),
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
            if music.cursor == 1 {
                break;
            }
        }
        assert_eq!(
            music.cursor, 1,
            "the folder advances at the end of the first song"
        );
        music.update_audio(&ring, 80);
        assert!(ring.queued_frames() > 0);
        music.input(Action::GbaDown(Btn::L1));
        music.update_audio(&ring, 80);
        assert_eq!(music.cursor, 0);
        music.input(Action::GbaDown(Btn::B));
        music.update_audio(&ring, 80);
        assert!(music.track.is_none());
        assert_eq!(ring.queued_frames(), 0);
        assert!(!music.input(Action::GbaDown(Btn::B)));
        assert!(music.is_root());
        assert!(music.input(Action::GbaDown(Btn::B)));
    }

    #[test]
    fn an_unreadable_mp3_can_be_skipped() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("Music");
        fs::create_dir(&folder).unwrap();
        fs::write(folder.join("bad.mp3"), "broken file").unwrap();
        silent_mp3(&folder.join("good.mp3"), 2);
        let mut music = Music::open(root.path());
        music.input(Action::GbaDown(Btn::A));
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
