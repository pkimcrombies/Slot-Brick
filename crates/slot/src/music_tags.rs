use std::fs::File;
use std::io::{self, Cursor, Read, Seek, SeekFrom};
use std::path::Path;

#[derive(Default)]
pub(crate) struct Tags {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub disc: Option<u16>,
    pub track: Option<u16>,
    /// Release year from ID3 or MP4 tags; no file-created-date substitute.
    pub year: Option<u16>,
}

pub(crate) fn read(path: &Path) -> Tags {
    if crate::music_m4a::is_m4a(path) {
        return read_m4a(path).unwrap_or_default();
    }
    let mut tags = Tags::default();
    if let Ok(mut file) = File::open(path) {
        let _ = id3v2(&mut file, &mut tags);
        if tags.title.is_none()
            || tags.artist.is_none()
            || tags.album.is_none()
            || tags.track.is_none()
        {
            let _ = id3v1(&mut file, &mut tags);
        }
    }
    tags
}

fn read_m4a(path: &Path) -> Option<Tags> {
    use symphonia::core::meta::StandardTagKey::*;

    let mut format = crate::music_m4a::format(path)?;
    let metadata = format.metadata();
    let mut tags = Tags::default();
    for tag in metadata.current()?.tags() {
        match tag.std_key {
            Some(TrackTitle) => tags.title = clean(tag.value.to_string()),
            Some(Artist) => tags.artist = clean(tag.value.to_string()),
            Some(Album) => tags.album = clean(tag.value.to_string()),
            Some(AlbumArtist) => tags.album_artist = clean(tag.value.to_string()),
            Some(DiscNumber) => tags.disc = number(&tag.value.to_string()),
            Some(TrackNumber) => tags.track = number(&tag.value.to_string()),
            Some(ReleaseDate) | Some(Date) | Some(OriginalDate) =>
                tags.year = tags.year.or_else(|| release_year(&tag.value.to_string())),
            _ => {}
        }
    }
    Some(tags)
}
pub(crate) fn id3v2_size(header: &[u8; 10]) -> Option<usize> {
    let version = header[3];
    let flags = header[5];
    if &header[..3] != b"ID3" || !matches!(version, 2..=4) || (version == 2 && flags & 0x40 != 0) {
        return None;
    }
    Some(syncsafe(&header[6..])? as usize)
}

fn id3v2(file: &mut File, tags: &mut Tags) -> Option<()> {
    let mut header = [0; 10];
    file.read_exact(&mut header).ok()?;
    let size = id3v2_size(&header)?;
    let version = header[3];
    let flags = header[5];
    if size as u64 > file.metadata().ok()?.len().saturating_sub(10) {
        return None;
    }
    if version < 4 && flags & 0x80 != 0 {
        // In these versions unsynchronisation can also affect frame headers.
        if size > 16 * 1024 * 1024 {
            return None;
        }
        let mut body = vec![0; size];
        file.read_exact(&mut body).ok()?;
        undo_unsync(&mut body);
        let remaining = body.len();
        frames(&mut Cursor::new(body), version, flags, remaining, tags);
    } else {
        frames(file, version, flags, size, tags);
    }
    Some(())
}

fn frames(
    source: &mut (impl Read + Seek),
    version: u8,
    flags: u8,
    mut remaining: usize,
    tags: &mut Tags,
) {
    if version >= 3 && flags & 0x40 != 0 {
        let mut bytes = [0; 4];
        if remaining < 4 || source.read_exact(&mut bytes).is_err() {
            return;
        }
        let extension = if version == 4 {
            syncsafe(&bytes).and_then(|len| len.checked_sub(4))
        } else {
            Some(u32::from_be_bytes(bytes))
        };
        let Some(extension) = extension
            .map(|len| len as usize)
            .filter(|&len| len <= remaining - 4)
        else {
            return;
        };
        if source.seek(SeekFrom::Current(extension as i64)).is_err() {
            return;
        }
        remaining -= 4 + extension;
    }

    let header_len = if version == 2 { 6 } else { 10 };
    let mut header = [0; 10];
    for _ in 0..4096 {
        if remaining < header_len
            || source.read_exact(&mut header[..header_len]).is_err()
            || header[0] == 0
        {
            break;
        }
        let (id, size) = match version {
            2 => (
                &header[..3],
                Some(u32::from_be_bytes([0, header[3], header[4], header[5]])),
            ),
            3 => (
                &header[..4],
                Some(u32::from_be_bytes(header[4..8].try_into().unwrap())),
            ),
            _ => (&header[..4], syncsafe(&header[4..8])),
        };
        remaining -= header_len;
        let Some(size) = size.map(|n| n as usize).filter(|&n| n <= remaining) else {
            break;
        };
        remaining -= size;
        let format = header[9];
        let readable = match version {
            3 => format & 0xc0 == 0,
            4 => format & 0x0c == 0,
            _ => true,
        };
        let known = matches!(
            id,
            b"TT2"
                | b"TIT2"
                | b"TP1"
                | b"TPE1"
                | b"TAL"
                | b"TALB"
                | b"TP2"
                | b"TPE2"
                | b"TPA"
                | b"TPOS"
                | b"TRK"
                | b"TRCK"
                | b"TYE"
                | b"TYER"
                | b"TDRC"
                | b"TDRL"
                | b"TDOR"
        );
        if !known || !readable || size > 4096 {
            if source.seek(SeekFrom::Current(size as i64)).is_err() {
                break;
            }
            continue;
        }

        let mut payload = vec![0; size];
        if source.read_exact(&mut payload).is_err() {
            break;
        }
        if version == 4 && (flags & 0x80 != 0 || format & 0x02 != 0) {
            undo_unsync(&mut payload);
        }
        let start = match version {
            3 => usize::from(format & 0x20 != 0),
            4 => usize::from(format & 0x40 != 0) + 4 * usize::from(format & 0x01 != 0),
            _ => 0,
        };
        let Some(value) = payload.get(start..).and_then(text) else {
            continue;
        };
        match id {
            b"TT2" | b"TIT2" => {
                tags.title.get_or_insert(value);
            }
            b"TP1" | b"TPE1" => {
                tags.artist.get_or_insert(value);
            }
            b"TAL" | b"TALB" => {
                tags.album.get_or_insert(value);
            }
            b"TP2" | b"TPE2" => {
                tags.album_artist.get_or_insert(value);
            }
            b"TPA" | b"TPOS" => tags.disc = tags.disc.or_else(|| number(&value)),
            b"TRK" | b"TRCK" => tags.track = tags.track.or_else(|| number(&value)),
            b"TYE" | b"TYER" | b"TDRC" | b"TDRL" | b"TDOR" =>
                tags.year = tags.year.or_else(|| release_year(&value)),
            _ => {}
        }
        if tags.title.is_some()
            && tags.artist.is_some()
            && tags.album.is_some()
            && tags.album_artist.is_some()
            && tags.disc.is_some()
            && tags.track.is_some()
            && tags.year.is_some()
        {
            break;
        }
    }
}

fn id3v1(file: &mut File, tags: &mut Tags) -> io::Result<()> {
    let mut footer = [0; 128];
    file.seek(SeekFrom::End(-128))?;
    file.read_exact(&mut footer)?;
    if &footer[..3] == b"TAG" {
        tags.title = tags.title.take().or_else(|| clean(latin1(&footer[3..33])));
        tags.artist = tags
            .artist
            .take()
            .or_else(|| clean(latin1(&footer[33..63])));
        tags.album = tags.album.take().or_else(|| clean(latin1(&footer[63..93])));
        tags.year = tags.year.or_else(|| release_year(&latin1(&footer[93..97])));
        if tags.track.is_none() && footer[125] == 0 && footer[126] != 0 {
            tags.track = Some(u16::from(footer[126]));
        }
    }
    Ok(())
}

fn text(payload: &[u8]) -> Option<String> {
    let (&encoding, bytes) = payload.split_first()?;
    let value = match encoding {
        0 => latin1(bytes),
        1 | 2 => {
            let (big_endian, bytes) = match bytes {
                [0xfe, 0xff, rest @ ..] => (true, rest),
                [0xff, 0xfe, rest @ ..] => (false, rest),
                _ => (encoding == 2, bytes),
            };
            let words: Vec<_> = bytes
                .chunks_exact(2)
                .map(|pair| {
                    if big_endian {
                        u16::from_be_bytes([pair[0], pair[1]])
                    } else {
                        u16::from_le_bytes([pair[0], pair[1]])
                    }
                })
                .take_while(|&word| word != 0)
                .collect();
            String::from_utf16_lossy(&words)
        }
        3 => String::from_utf8_lossy(bytes).into_owned(),
        _ => return None,
    };
    clean(value)
}

fn latin1(bytes: &[u8]) -> String {
    bytes
        .iter()
        .copied()
        .take_while(|&byte| byte != 0)
        .map(char::from)
        .collect()
}

fn clean(value: String) -> Option<String> {
    let value = value.split('\0').next()?.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

/// Date fields may contain YYYY, YYYY-MM-DD or a timestamp. A year is
/// usable only when explicitly tagged, never inferred from file metadata.
fn release_year(value: &str) -> Option<u16> {
    let year = value.trim().get(..4)?;
    (year.bytes().all(|b| b.is_ascii_digit())).then(|| year.parse::<u16>().ok()).flatten()
        .filter(|&year| (1000..=2100).contains(&year))
}

fn number(value: &str) -> Option<u16> {
    value
        .split('/')
        .next()?
        .trim()
        .parse()
        .ok()
        .filter(|&n| n > 0)
}

fn syncsafe(bytes: &[u8]) -> Option<u32> {
    bytes.iter().try_fold(0, |value, &byte| {
        (byte < 128).then_some((value << 7) | u32::from(byte))
    })
}

fn undo_unsync(bytes: &mut Vec<u8>) {
    let mut previous = None;
    bytes.retain(|&byte| {
        let stuffed = previous == Some(0xff) && byte == 0;
        previous = Some(byte);
        !stuffed
    });
}

#[cfg(test)]
mod release_date_tests {
    use super::*;

    #[test]
    fn parses_tagged_release_years_without_guessing() {
        assert_eq!(release_year("1999"), Some(1999));
        assert_eq!(release_year("2022-09-17"), Some(2022));
        assert_eq!(release_year(" 2015-01-02T00:00:00 "), Some(2015));
        assert_eq!(release_year("unknown"), None);
        assert_eq!(release_year(""), None);
        assert_eq!(release_year("2050s"), Some(2050));
        assert_eq!(release_year("0900"), None);
    }
}
