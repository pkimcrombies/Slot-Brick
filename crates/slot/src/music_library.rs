use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};

use crate::music_tags;

pub(crate) struct Song {
    pub path: PathBuf,
    pub title: String,
    pub artist: String,
    pub disc: u16,
    pub track: Option<u16>,
}

pub(crate) struct Album {
    pub title: String,
    pub artist: String,
    /// Earliest tagged release year among this album's songs (if known).
    pub release_year: Option<u16>,
    pub songs: Vec<Song>,
}

pub(crate) struct Artist {
    pub name: String,
    pub albums: Vec<usize>,
}

#[derive(Default)]
pub(crate) struct Library {
    pub albums: Vec<Album>,
    pub artists: Vec<Artist>,
}

impl Library {
    pub(crate) fn scan(root: &Path, cancelled: impl Fn() -> bool) -> Self {
        let base = root.join("Music");
        let mut albums = BTreeMap::new();
        let mut folders = vec![base.clone()];
        while let Some(folder) = folders.pop() {
            for entry in fs::read_dir(&folder)
                .into_iter()
                .flatten()
                .filter_map(Result::ok)
            {
                if cancelled() {
                    return Self::default();
                }
                let Ok(kind) = entry.file_type() else {
                    continue;
                };
                let path = entry.path();
                if kind.is_dir() {
                    folders.push(path);
                    continue;
                }
                if !kind.is_file()
                    || !(crate::music_m4a::is_m4a(&path)
                        || path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("mp3")))
                {
                    continue;
                }

                let tags = music_tags::read(&path);
                let title = tags.title.unwrap_or_else(|| {
                    path.file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned()
                });
                let artist = tags
                    .artist
                    .as_deref()
                    .or(tags.album_artist.as_deref())
                    .unwrap_or("Unknown Artist")
                    .to_owned();
                let album_artist = tags.album_artist.unwrap_or_else(|| artist.clone());
                let album_title = tags.album.unwrap_or_else(|| {
                    if folder == base {
                        "Unknown Album".into()
                    } else {
                        folder
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned()
                    }
                });
                let album = albums
                    .entry((album_title.to_lowercase(), album_artist.to_lowercase()))
                    .or_insert_with(|| Album {
                        title: album_title,
                        artist: album_artist,
                        release_year: None,
                        songs: Vec::new(),
                    });
                if let Some(year) = tags.year {
                    album.release_year = Some(album.release_year.map_or(year, |old| old.min(year)));
                }
                album.songs.push(Song {
                    path,
                    title,
                    artist,
                    disc: tags.disc.unwrap_or(1),
                    track: tags.track,
                });
            }
        }

        let mut albums: Vec<_> = albums.into_values().collect();
        let mut artists = BTreeMap::new();
        for (index, album) in albums.iter_mut().enumerate() {
            album.songs.sort_by_cached_key(|song| {
                (
                    song.disc,
                    song.track.is_none(),
                    song.track,
                    song.title.to_lowercase(),
                    song.path.clone(),
                )
            });
            let artist = artists
                .entry(album.artist.to_lowercase())
                .or_insert_with(|| Artist {
                    name: album.artist.clone(),
                    albums: Vec::new(),
                });
            artist.albums.push(index);
        }
        Self {
            albums,
            artists: artists.into_values().collect(),
        }
    }
}

pub(crate) struct Scan {
    result: mpsc::Receiver<Library>,
    cancelled: Arc<AtomicBool>,
}

impl Scan {
    pub(crate) fn start(root: PathBuf) -> Option<Self> {
        let (sender, result) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let stop = cancelled.clone();
        std::thread::Builder::new()
            .name("slot-music-library".into())
            .spawn(move || {
                let library = Library::scan(&root, || stop.load(Ordering::Relaxed));
                let _ = sender.send(library);
            })
            .ok()?;
        Some(Self { result, cancelled })
    }

    pub(crate) fn take(&self) -> Option<Library> {
        match self.result.try_recv() {
            Ok(library) => Some(library),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Library::default()),
        }
    }
}

impl Drop for Scan {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}
