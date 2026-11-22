use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};

use jpeg_decoder::PixelFormat;
use slot_gfx::{Compositor, TexId};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::{MetadataReader, StandardVisualKey, Visual};
use symphonia_metadata::id3v2::Id3v2Reader;

use crate::music_library::Album;
use crate::{cart_art, music_m4a, music_tags};

pub(crate) const THUMBNAIL: u32 = 256;
const MAX_EMBEDDED_BYTES: usize = 16 << 20;
const MAX_PNG_PIXELS: u32 = 8 << 20;

struct Cover {
    rgba: Vec<u8>,
    w: u32,
    h: u32,
}

#[derive(Default)]
pub(crate) struct MusicCover {
    track: Option<PathBuf>,
    size: Option<[u32; 2]>,
    texture: Option<TexId>,
    loading: Option<Load>,
}

struct Load {
    cancel: Arc<AtomicBool>,
    result: mpsc::Receiver<Option<Cover>>,
}

impl MusicCover {
    /// Upload on the render thread, with at most one reader active. A late cover
    /// belongs to the track that requested it and is ignored after a skip.
    pub(crate) fn texture(
        &mut self,
        track: &Path,
        album: &Album,
        compositor: &mut Compositor,
    ) -> Option<(TexId, [u32; 2])> {
        if let Some(load) = &self.loading {
            let expected = self.track.as_deref() == Some(track)
                && !load.cancel.load(Ordering::Relaxed);
            if !expected {
                load.cancel.store(true, Ordering::Relaxed);
            }
            let received = match load.result.try_recv() {
                Ok(cover) => cover,
                Err(mpsc::TryRecvError::Disconnected) => None,
                Err(mpsc::TryRecvError::Empty) => return None,
            };
            self.loading = None;
            if expected {
                if let Some(cover) = received {
                    let Cover { rgba, w, h } = cover;
                    if let Some(tex) = self.texture {
                        compositor.update_texture(tex, w, h, &rgba);
                    } else {
                        self.texture = Some(compositor.create_texture(w, h, &rgba));
                    }
                    self.size = Some([w, h]);
                }
            } else {
                self.track = None;
            }
        }
        if self.track.as_deref() != Some(track) {
            self.track = Some(track.to_path_buf());
            self.size = None;
            self.loading = Load::start(track, album);
        }
        self.texture.zip(self.size)
    }
}

impl Load {
    fn start(track: &Path, album: &Album) -> Option<Self> {
        let paths: Vec<_> = std::iter::once(track.to_path_buf())
            .chain(album.songs.iter().filter(|song| song.path != track).map(|song| song.path.clone()))
            .collect();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let (sender, result) = mpsc::channel();
        std::thread::Builder::new()
            .name("slot-music-cover".into())
            .spawn(move || {
                let cover = paths.iter()
                    .take_while(|_| !worker_cancel.load(Ordering::Relaxed))
                    .find_map(|path| embedded_cover(path));
                let _ = sender.send(cover);
            })
            .ok()?;
        Some(Self { cancel, result })
    }
}

fn embedded_cover(path: &Path) -> Option<Cover> {
    if music_m4a::is_m4a(path) {
        let mut format = music_m4a::format(path)?;
        choose_cover(format.metadata().current()?.visuals())
    } else {
        let mut file = File::open(path).ok()?;
        let mut header = [0; 10];
        file.read_exact(&mut header).ok()?;
        let size = music_tags::id3v2_size(&header)?;
        if size > MAX_EMBEDDED_BYTES || size as u64 + 10 > file.metadata().ok()?.len() {
            return None;
        }
        file.seek(SeekFrom::Start(0)).ok()?;
        let mut stream = MediaSourceStream::new(Box::new(file), Default::default());
        let metadata = Id3v2Reader::new(&Default::default()).read_all(&mut stream).ok()?;
        choose_cover(metadata.visuals())
    }
}

fn choose_cover(visuals: &[Visual]) -> Option<Cover> {
    let front = |picture: &&Visual| picture.usage == Some(StandardVisualKey::FrontCover);
    visuals.iter().filter(front)
        .chain(visuals.iter().filter(|picture| !front(picture)))
        .find_map(|picture| thumbnail(&picture.data))
}

fn thumbnail(data: &[u8]) -> Option<Cover> {
    if data.len() > MAX_EMBEDDED_BYTES {
        return None;
    }
    let (rgba, w, h) = if data.starts_with(&[0xff, 0xd8]) {
        jpeg_rgba(data)?
    } else {
        let (w, h) = cart_art::png_size(data)?;
        if w.checked_mul(h)? > MAX_PNG_PIXELS {
            return None;
        }
        cart_art::decode_rgba(data)?
    };
    if w == 0 || h == 0 {
        return None;
    }
    let long = w.max(h);
    let tw = (w * THUMBNAIL / long).max(1);
    let th = (h * THUMBNAIL / long).max(1);
    let rgba = slot_ui::cover_rgba(&rgba, w, h, tw, th)?;
    Some(Cover { rgba, w: tw, h: th })
}

fn jpeg_rgba(data: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    let mut reader = jpeg_decoder::Decoder::new(data);
    reader.set_max_decoding_buffer_size(32 << 20);
    reader.scale(THUMBNAIL as u16, THUMBNAIL as u16).ok()?;
    let pixels = reader.decode().ok()?;
    let info = reader.info()?;
    let rgba = match info.pixel_format {
        PixelFormat::L8 => pixels.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        PixelFormat::RGB24 => pixels.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        PixelFormat::CMYK32 => pixels.chunks_exact(4).flat_map(|p| {
            let rgb = |c| ((255 - u16::from(c)) * (255 - u16::from(p[3])) / 255) as u8;
            [rgb(p[0]), rgb(p[1]), rgb(p[2]), 255]
        }).collect(),
        PixelFormat::L16 => return None,
    };
    Some((rgba, u32::from(info.width), u32::from(info.height)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::music_library::Library;
    use std::fs;
    use std::time::Duration;

    #[test]
    fn embedded_jpeg_and_png_covers_fit_the_same_square_without_stretching() {
        let root = tempfile::tempdir().unwrap();
        // Generated colour swatches embedded in synthetic audio tracks.
        for (name, sample, height) in [
            ("cover.mp3", include_bytes!("../tests/fixtures/music/cover.mp3").as_slice(), 256),
            ("cover.m4a", include_bytes!("../tests/fixtures/music/cover.m4a").as_slice(), 128),
        ] {
            let path = root.path().join(name);
            fs::write(&path, sample).unwrap();
            let cover = embedded_cover(&path).expect("embedded cover");
            assert_eq!((cover.w, cover.h), (THUMBNAIL, height));
            assert_eq!(cover.rgba.len(), (cover.w * cover.h * 4) as usize);
            let centre = ((cover.h / 2 * cover.w + cover.w / 2) * 4) as usize;
            assert_ne!(cover.rgba[..3], cover.rgba[centre..centre + 3]);
        }
    }

    #[test]
    fn a_track_without_a_cover_can_use_another_track_from_the_album() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("Music");
        fs::create_dir(&folder).unwrap();
        let no_art = folder.join("no-art.m4a");
        fs::write(&no_art, include_bytes!("../tests/fixtures/music/aac.m4a")).unwrap();
        fs::write(folder.join("with-art.m4a"), include_bytes!("../tests/fixtures/music/cover.m4a")).unwrap();
        let album = Library::scan(root.path(), || false).albums.remove(0);
        assert_eq!(album.songs.len(), 2);
        assert!(embedded_cover(&no_art).is_none());
        let load = Load::start(&no_art, &album).unwrap();
        let cover = load.result.recv_timeout(Duration::from_secs(2)).unwrap().unwrap();
        assert_eq!((cover.w, cover.h), (256, 128));
    }
}
