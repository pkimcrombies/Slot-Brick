//! Slot OS-native H.264 video renderer.
//!
//! MP4 demuxing and H.264 decode run on a bounded background worker. The
//! Music supplies the clock from the video's own soundtrack or a timer for
//! silent files. Frames use Slot's existing GLES2 compositor.

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, SyncSender, TryRecvError},
    Arc,
};

use mp4::{MediaType, Mp4Reader};
use rust_h264::decoder::{ChromaFormat, Frame, OrderedDecoder};
use rust_h264::nal::{parse_annex_b, parse_avcc};
use slot_gfx::{Compositor, Draw, TexId, OUT_H, OUT_W};

const MAX_PIXELS: usize = 854 * 480;
const MAX_FILE_BYTES: u64 = 1 << 30;
const FRAME_AHEAD: f64 = 0.045;

pub(crate) struct VideoFrame {
    pub(crate) pts: f64,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

enum Message {
    Frame(VideoFrame),
    Error(String),
    Finished,
}

/// An instance is owned by Frontend and outlives any individual Music track.
/// No worker ever owns the audio ring or graphics context.
#[derive(Default)]
pub(crate) struct MusicVideo {
    path: Option<PathBuf>,
    cancel: Option<Arc<AtomicBool>>,
    receiver: Option<Receiver<Message>>,
    next: Option<VideoFrame>,
    texture: Option<TexId>,
    shown: Option<(u32, u32)>,
    clock: f64,
    failed: bool,
    finished: bool,
}

impl MusicVideo {
    pub(crate) fn active(&self) -> bool {
        self.path.is_some() && !self.failed && !self.finished
    }

    pub(crate) fn ended(&self) -> bool {
        self.failed || self.finished
    }

    pub(crate) fn stop(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        // Dropping the receiver also unblocks a producer stuck on its
        // bounded send, without ever blocking Slot's audio/render thread.
        self.receiver = None;
        self.next = None;
        self.path = None;
        self.shown = None;
        self.failed = false;
        self.finished = false;
    }

    fn start(&mut self, path: PathBuf, at: f64) {
        self.stop();
        let (sender, receiver) = mpsc::sync_channel(2);
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let worker_path = path.clone();
        match std::thread::Builder::new()
            .name("slot-native-video".into())
            .spawn(move || {
                let result = decode_file(&worker_path, at, &sender, &worker_cancel);
                if !worker_cancel.load(Ordering::Relaxed) {
                    let message = match result {
                        Ok(()) => Message::Finished,
                        Err(err) => Message::Error(err),
                    };
                    // Finishing is ordered after the final frames. stop()
                    // drops the receiver if the user closes the video first.
                    let _ = sender.send(message);
                }
            })
        {
            Ok(_handle) => {
                // A detached thread is cancelled by AtomicBool plus dropping
                // the receiver. Joining it here could freeze music output.
                self.path = Some(path);
                self.cancel = Some(cancel);
                self.receiver = Some(receiver);
                self.clock = at;
            }
            Err(err) => {
                self.path = Some(path);
                self.failed = true;
                eprintln!("slot: native video thread: {err}");
            }
        }
    }

    /// Start on selection or seek. Ordinary clock updates reuse the decoder.
    pub(crate) fn sync(&mut self, selected: Option<(&Path, f64, bool)>) {
        let Some((path, position, restart)) = selected else {
            if self.path.is_some() {
                self.stop();
            }
            return;
        };
        if restart || self.path.as_deref() != Some(path) {
            self.start(path.to_path_buf(), position);
        } else {
            self.clock = position;
        }
    }

    /// Upload frames on the graphics thread using their presentation times.
    pub(crate) fn overlay(&mut self, compositor: &mut Compositor, draws: &mut Vec<Draw>) {
        if self.path.is_none() || self.failed {
            return;
        }
        if let Some(receiver) = &self.receiver {
            for _ in 0..6 {
                let message = if let Some(pending) = self.next.take() {
                    Some(Message::Frame(pending))
                } else {
                    match receiver.try_recv() {
                        Ok(message) => Some(message),
                        Err(TryRecvError::Empty) => None,
                        Err(TryRecvError::Disconnected) => Some(Message::Finished),
                    }
                };
                let Some(message) = message else { break; };
                match message {
                    Message::Frame(frame) => {
                        if frame.pts > self.clock + FRAME_AHEAD {
                            self.next = Some(frame);
                            break;
                        }
                        // If decoding fell behind, skip stale uploads and
                        // drain the queue towards the current video clock.
                        if frame.pts + 0.20 < self.clock && !self.finished {
                            continue;
                        }
                        if let Some(tex) = self.texture {
                            compositor.update_texture(tex, frame.width, frame.height, &frame.rgba);
                        } else {
                            self.texture = Some(compositor.create_texture(
                                frame.width, frame.height, &frame.rgba,
                            ));
                        }
                        self.shown = Some((frame.width, frame.height));
                    }
                    Message::Error(err) => {
                        eprintln!("slot: native video: {err}");
                        self.failed = true;
                        self.shown = None;
                        break;
                    }
                    Message::Finished => {
                        self.finished = true;
                        break;
                    }
                }
            }
        }
        if let (Some(tex), Some((w, h))) = (self.texture, self.shown) {
            let factor = (OUT_W as f32 / w as f32).min(OUT_H as f32 / h as f32);
            let width = w as f32 * factor;
            let height = h as f32 * factor;
            draws.push(Draw::Rect {
                x: 0.0, y: 0.0, w: OUT_W as f32, h: OUT_H as f32,
                colour: [0.0, 0.0, 0.0, 1.0],
            });
            draws.push(Draw::Tex {
                x: (OUT_W as f32 - width) * 0.5,
                y: (OUT_H as f32 - height) * 0.5,
                w: width, h: height, tex, alpha: 1.0,
            });
        }
    }
}

fn seek_sync_sample(
    track: &mp4::Mp4Track,
    count: u32,
    at_seconds: f64,
) -> u32 {
    let target = (at_seconds * track.timescale() as f64).max(0.0) as u64;
    let mut time = 0u64;
    let mut sample = 1u32;
    let mut candidate = 1u32;
    for entry in &track.trak.mdia.minf.stbl.stts.entries {
        if entry.sample_count == 0 || entry.sample_delta == 0 { continue; }
        let span = u64::from(entry.sample_count) * u64::from(entry.sample_delta);
        if target < time.saturating_add(span) {
            candidate = sample.saturating_add(
                ((target.saturating_sub(time)) / u64::from(entry.sample_delta))
                    .min(u64::from(entry.sample_count.saturating_sub(1))) as u32,
            );
            break;
        }
        time = time.saturating_add(span);
        sample = sample.saturating_add(entry.sample_count);
        candidate = sample;
    }
    candidate = candidate.clamp(1, count.max(1));
    match &track.trak.mdia.minf.stbl.stss {
        Some(sync) => sync.entries.iter().copied()
            .take_while(|&id| id <= candidate).last().unwrap_or(1),
        None => candidate,
    }
}

/// Decode bounded local MP4/H.264 access units, carrying true MP4 PTS values
/// through picture reordering. All MP4 data stays on disk except one packet.
fn decode_file(
    path: &Path,
    at: f64,
    sender: &SyncSender<Message>,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let file = File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let size = file.metadata().map_err(|e| e.to_string())?.len();
    if size == 0 || size > MAX_FILE_BYTES {
        return Err("video file is empty or exceeds 1 GiB".into());
    }
    let mut mp4 = Mp4Reader::read_header(BufReader::new(file), size)
        .map_err(|e| format!("MP4 header: {e}"))?;
    if mp4.is_fragmented() {
        return Err("fragmented MP4 is not yet supported".into());
    }
    let (track_id, track) = mp4.tracks().iter()
        .find(|(_, track)| matches!(track.media_type(), Ok(MediaType::H264)))
        .ok_or("no H.264 video track in MP4")?;
    let track_id = *track_id;
    let count = track.sample_count();
    let timescale = track.timescale();
    let width = usize::from(track.width());
    let height = usize::from(track.height());
    if count == 0 || timescale == 0 || width == 0 || height == 0
        || width.checked_mul(height).is_none_or(|pixels| pixels > MAX_PIXELS)
    {
        return Err("unsupported video dimensions or empty sample table (480p limit)".into());
    }
    let stsd = &track.trak.mdia.minf.stbl.stsd;
    let avcc = stsd.avc1.as_ref().ok_or("missing H.264 avc1 header")?.avcc.clone();
    let length_size = usize::from((avcc.length_size_minus_one & 3) + 1);
    if !matches!(length_size, 1 | 2 | 4) {
        return Err("unsupported H.264 sample NAL length".into());
    }
    let first = seek_sync_sample(track, count, at);

    let mut config = Vec::new();
    for nal in avcc.sequence_parameter_sets.iter()
        .chain(avcc.picture_parameter_sets.iter())
    {
        config.extend_from_slice(&[0, 0, 0, 1]);
        config.extend_from_slice(&nal.bytes);
    }
    if config.is_empty() {
        return Err("H.264 file is missing SPS/PPS codec configuration".into());
    }
    let mut decoder = OrderedDecoder::<f64>::new();
    for nal in parse_annex_b(&config) {
        decoder.decode_nal_with_meta(&nal, 0.0)
            .map_err(|e| format!("codec configuration: {e:?}"))?;
    }
    for sample_id in first..=count {
        if cancel.load(Ordering::Relaxed) { return Ok(()); }
        let sample = mp4.read_sample(track_id, sample_id)
            .map_err(|e| format!("sample {sample_id}: {e}"))?
            .ok_or_else(|| format!("missing sample {sample_id}"))?;
        let pts_ticks = i128::from(sample.start_time) + i128::from(sample.rendering_offset);
        let pts = pts_ticks.max(0) as f64 / f64::from(timescale);
        for nal in parse_avcc(&sample.bytes, length_size) {
            if cancel.load(Ordering::Relaxed) { return Ok(()); }
            let frames = decoder.decode_nal_with_meta(&nal, pts)
                .map_err(|e| format!("decode sample {sample_id}: {e:?}"))?;
            for (frame, frame_pts) in frames {
                send_frame(frame, frame_pts, at, sender)?;
            }
        }
    }
    for (frame, frame_pts) in decoder.flush_with_meta() {
        if cancel.load(Ordering::Relaxed) { return Ok(()); }
        send_frame(frame, frame_pts, at, sender)?;
    }
    Ok(())
}

fn send_frame(frame: Frame, pts: f64, begin: f64,
              sender: &SyncSender<Message>) -> Result<(), String> {
    if pts + 0.08 < begin { return Ok(()); }
    let rgba = yuv420_to_rgba(&frame)?;
    sender.send(Message::Frame(VideoFrame {
        pts, width: frame.width, height: frame.height, rgba,
    })).map_err(|_| "viewer closed".to_string())
}

fn yuv420_to_rgba(frame: &Frame) -> Result<Vec<u8>, String> {
    if frame.bit_depth != 8 || frame.chroma_format != ChromaFormat::Yuv420 {
        return Err("only 8-bit YUV420 H.264 is supported in the initial player".into());
    }
    let (w, h) = (frame.width as usize, frame.height as usize);
    let pixels = w.checked_mul(h).ok_or("invalid frame dimensions")?;
    if pixels == 0 || pixels > MAX_PIXELS {
        return Err("decoded frame exceeds the 480p performance limit".into());
    }
    let cw = w.div_ceil(2);
    let ch = h.div_ceil(2);
    if frame.y.len() < pixels || frame.u.len() < cw * ch || frame.v.len() < cw * ch {
        return Err("decoder returned a truncated YUV frame".into());
    }
    let mut rgba = vec![255u8; pixels * 4];
    for y in 0..h {
        for x in 0..w {
            let luma = frame.y[y * w + x] as i32 - 16;
            let chroma = (y / 2) * cw + x / 2;
            let u = frame.u[chroma] as i32 - 128;
            let v = frame.v[chroma] as i32 - 128;
            let c = 298 * luma.max(0);
            let offset = (y * w + x) * 4;
            rgba[offset] = ((c + 409 * v + 128) >> 8).clamp(0, 255) as u8;
            rgba[offset + 1] = ((c - 100 * u - 208 * v + 128) >> 8).clamp(0, 255) as u8;
            rgba[offset + 2] = ((c + 516 * u + 128) >> 8).clamp(0, 255) as u8;
        }
    }
    Ok(rgba)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_yuv420_black_and_white_are_opaque() {
        let frame = Frame {
            width: 2, height: 2,
            y: vec![16, 16, 235, 235],
            u: vec![128], v: vec![128],
            pic_order_cnt: 0,
            chroma_format: ChromaFormat::Yuv420,
            bit_depth: 8,
        };
        let rgba = yuv420_to_rgba(&frame).unwrap();
        assert_eq!(&rgba[0..4], &[0, 0, 0, 255]);
        assert_eq!(&rgba[8..12], &[255, 255, 255, 255]);
    }

    #[test]
    fn unsupported_yuv_depth_rejects_without_crash() {
        let frame = Frame {
            width: 2, height: 2,
            y: vec![0; 8], u: vec![0; 2], v: vec![0; 2],
            pic_order_cnt: 0,
            chroma_format: ChromaFormat::Yuv420,
            bit_depth: 10,
        };
        assert!(yuv420_to_rgba(&frame).is_err());
    }

    #[test]
    fn decodes_real_h264_mp4_fixture_with_presentation_timestamps() {
        let Ok(file) = std::env::var("SLOT_NATIVE_VIDEO_FIXTURE") else {
            // The CI workflow provides a generated local MP4. Generic host
            // tests need not have ffmpeg installed on the developer machine.
            return;
        };
        let (sender, receiver) = mpsc::sync_channel(2);
        let worker = std::thread::spawn(move || {
            let cancel = AtomicBool::new(false);
            decode_file(Path::new(&file), 0.0, &sender, &cancel)
        });
        let mut frames = 0usize;
        let mut last_pts = -1.0f64;
        for message in receiver {
            if let Message::Frame(frame) = message {
                assert_eq!((frame.width, frame.height), (320, 180));
                assert_eq!(frame.rgba.len(), 320 * 180 * 4);
                assert!(frame.pts >= last_pts, "presentation timestamp went backwards");
                last_pts = frame.pts;
                frames += 1;
            }
        }
        worker.join().expect("decoder did not panic")
            .expect("the decoder must consume the real MP4 fixture");
        assert!(frames >= 10, "expected at least ten actual H.264 frames, got {frames}");
        assert!(last_pts > 1.0, "the decoded video clock never advanced");
    }

    #[test]
    fn no_video_stops_worker_and_clears_display() {
        let mut video = MusicVideo::default();
        video.sync(None);
        assert!(!video.active());
        assert!(video.path.is_none());
    }
}
