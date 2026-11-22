use std::fs::File;
use std::path::Path;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{Decoder, DecoderOptions, CODEC_TYPE_AAC, CODEC_TYPE_ALAC};
use symphonia::core::errors::Error;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::units::{Time, TimeBase};
use symphonia::default::{get_codecs, get_probe};

pub(crate) fn is_m4a(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("m4a"))
}

pub(crate) fn format(path: &Path) -> Option<Box<dyn FormatReader>> {
    let stream = MediaSourceStream::new(Box::new(File::open(path).ok()?), Default::default());
    let probe = get_probe()
        .format(
            Hint::new().with_extension("m4a"),
            stream,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .ok()?;
    Some(probe.format)
}

pub(crate) struct M4aDecoder {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    track: u32,
    time_base: TimeBase,
    samples: Option<SampleBuffer<i16>>,
    position: usize,
    pub(crate) rate: u32,
    pub(crate) channels: usize,
    pub(crate) duration: u64,
}

impl M4aDecoder {
    pub(crate) fn open(path: &Path) -> Option<Self> {
        let format = format(path)?;
        let track = format.tracks().iter().find(|track| {
            matches!(track.codec_params.codec, CODEC_TYPE_AAC | CODEC_TYPE_ALAC)
        })?;
        let params = &track.codec_params;
        let duration_ticks = params.n_frames;
        let time_base = params.time_base?;
        let track = track.id;
        let decoder = get_codecs().make(params, &DecoderOptions::default()).ok()?;
        let mut reader = Self {
            format,
            decoder,
            track,
            time_base,
            samples: None,
            position: 0,
            rate: 0,
            channels: 0,
            duration: 0,
        };
        // Decode once to get the actual channel count. The container's AAC
        // sample entry can describe a different layout from the codec record.
        reader.next_buffer()?;
        if let Some(ticks) = duration_ticks {
            reader.duration = reader.frames_from_ticks(ticks);
        }
        Some(reader)
    }

    fn frames_from_ticks(&self, ticks: u64) -> u64 {
        let frames = u128::from(ticks) * u128::from(self.time_base.numer) * u128::from(self.rate)
            / u128::from(self.time_base.denom);
        u64::try_from(frames).unwrap_or(0)
    }

    pub(crate) fn seek(&mut self, frame: u64) -> Option<u64> {
        let time = Time::from(frame as f64 / f64::from(self.rate));
        let position = self.format.seek(
            SeekMode::Accurate,
            SeekTo::Time { time, track_id: Some(self.track) },
        ).ok()?;
        self.decoder.reset();
        self.position = self.samples.as_ref().map_or(0, SampleBuffer::len);
        Some(self.frames_from_ticks(position.actual_ts))
    }

    fn next_buffer(&mut self) -> Option<()> {
        loop {
            let packet = self.format.next_packet().ok()?;
            if packet.track_id() != self.track {
                continue;
            }
            let decoded = match self.decoder.decode(&packet) {
                Ok(samples) => samples,
                Err(Error::DecodeError(_)) => continue,
                Err(_) => return None,
            };
            if decoded.frames() == 0 {
                continue;
            }
            let spec = *decoded.spec();
            let channels = spec.channels.count();
            if self.samples.is_none() {
                if !matches!(channels, 1 | 2) || !(8_000..=192_000).contains(&spec.rate) {
                    return None;
                }
                self.rate = spec.rate;
                self.channels = channels;
            } else if channels != self.channels || spec.rate != self.rate {
                return None;
            }
            let capacity = decoded.capacity() as u64;
            let mut buffer = self.samples.take()
                .filter(|previous| previous.capacity() as u64 >= capacity * self.channels as u64)
                .unwrap_or_else(|| SampleBuffer::new(capacity, spec));
            buffer.copy_interleaved_ref(decoded);
            self.samples = Some(buffer);
            self.position = 0;
            return Some(());
        }
    }

    /// Interleaved samples at the source rate, using an existing packet before decoding another.
    pub(crate) fn read(&mut self, output: &mut [i16]) -> usize {
        if self.samples.as_ref().is_none_or(|buffer| self.position == buffer.len())
            && self.next_buffer().is_none()
        {
            return 0;
        }
        let tail = &self.samples.as_ref().expect("decoded packet").samples()[self.position..];
        let written = (tail.len().min(output.len()) / self.channels) * self.channels;
        output[..written].copy_from_slice(&tail[..written]);
        self.position += written;
        written / self.channels
    }
}
