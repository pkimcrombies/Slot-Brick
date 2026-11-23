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
use std::time::{Duration, Instant};

use mp4::{MediaType, Mp4Reader};
use rust_h264::decoder::{ChromaFormat, Frame, OrderedDecoder};
use rust_h264::nal::{parse_annex_b, parse_avcc};
use slot_gfx::{Compositor, Draw, OUT_H, OUT_W};

const MAX_PIXELS: usize = 854 * 480;
const MAX_FILE_BYTES: u64 = 1 << 30;
const FRAME_QUEUE: usize = 5;
// Allow half a 60 Hz refresh of lead time when choosing the next frame.
const FRAME_AHEAD: f64 = 0.008;
const KEEP_FRAME_WITHIN: f64 = 0.050;
const MOVIE_FADE: Duration = Duration::from_millis(180);

pub(crate) struct VideoFrame {
    pub(crate) pts: f64,
    width: u32,
    height: u32,
    planes: [Vec<u8>; 3],
}

enum Message {
    Frame(VideoFrame),
    Error(String),
    Finished,
}

#[derive(Default)]
struct PresentationStats {
    shown: u32,
    late: u32,
    superseded: u32,
    max_delay: f64,
    upload: Duration,
    first_picture: Duration,
}

#[derive(Clone, Copy)]
struct MoviePicture {
    size: (u32, u32),
    since: Instant,
    fading_from: Option<f32>,
}

impl MoviePicture {
    fn new(size: (u32, u32), now: Instant) -> Self {
        Self { size, since: now, fading_from: None }
    }

    fn opacity(&self, now: Instant) -> f32 {
        let t = (now.saturating_duration_since(self.since).as_secs_f32()
            / MOVIE_FADE.as_secs_f32()).min(1.0);
        let eased = t * t * (3.0 - 2.0 * t);
        self.fading_from.map_or(eased, |from| from * (1.0 - eased))
    }

    fn close(self, now: Instant) -> Self {
        Self { since: now, fading_from: Some(self.opacity(now)), ..self }
    }
}

/// An instance is owned by Frontend and outlives any individual Music track.
/// No worker ever owns the audio ring or graphics context.
#[derive(Default)]
pub(crate) struct MusicVideo {
    path: Option<PathBuf>,
    cancel: Option<Arc<AtomicBool>>,
    receiver: Option<Receiver<Message>>,
    next: Option<VideoFrame>,
    picture: Option<MoviePicture>,
    clock: f64,
    started_at: Option<Instant>,
    failed: bool,
    finished: bool,
    stats: PresentationStats,
}

impl MusicVideo {
    pub(crate) fn active(&self) -> bool {
        self.path.is_some() && !self.failed && !self.finished
    }

    pub(crate) fn needs_render(&self) -> bool {
        self.active() || self.picture.is_some_and(|picture| picture.fading_from.is_some())
    }

    pub(crate) fn ended(&self) -> bool {
        self.failed || self.finished
    }

    pub(crate) fn picture_ready(&self) -> bool {
        // A buffered frame can start the soundtrack even if its own PTS
        // is still in the future. This also handles a video with lead-in.
        self.stats.shown > 0 || self.next.is_some()
    }

    pub(crate) fn stop(&mut self) {
        let stats = std::mem::take(&mut self.stats);
        if stats.shown > 0 || stats.late > 0 {
            eprintln!(
                "slot: native video presentation: {} displayed, {} too late, {} superseded, worst delay {:.1} ms, average upload {:.1} ms, first picture {:.1} ms",
                stats.shown, stats.late, stats.superseded, stats.max_delay * 1000.0,
                stats.upload.as_secs_f64() * 1000.0 / f64::from(stats.shown.max(1)),
                stats.first_picture.as_secs_f64() * 1000.0,
            );
        }
        if let Some(cancel) = self.cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        // Dropping the receiver also unblocks a producer stuck on its
        // bounded send, without ever blocking Slot's audio/render thread.
        self.receiver = None;
        self.next = None;
        self.path = None;
        self.picture = None;
        self.started_at = None;
        self.failed = false;
        self.finished = false;
    }

    fn start(&mut self, path: PathBuf, at: f64) {
        // On a seek, keep the old picture visible until the new one is
        // ready. Readiness uses the new decoder's presentation stats.
        let previous_picture = self.picture.filter(|_| self.path.as_deref() == Some(path.as_path()));
        self.stop();
        // The decoded planes are compact enough to buffer a few frames
        // through a short CPU decode spike without delaying presentation.
        let (sender, receiver) = mpsc::sync_channel(FRAME_QUEUE);
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
                self.picture = previous_picture;
                self.started_at = Some(Instant::now());
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
                let picture = self.picture;
                self.stop();
                self.picture = picture.map(|picture| picture.close(Instant::now()));
            }
            return;
        };
        if restart || self.path.as_deref() != Some(path) {
            self.start(path.to_path_buf(), position);
        } else {
            self.clock = position;
        }
    }

    fn take_due_frame(&mut self) -> Option<VideoFrame> {
        let receiver = self.receiver.as_ref()?;
        let mut latest = None;
        for _ in 0..6 {
            let message = if let Some(pending) = self.next.take() {
                Message::Frame(pending)
            } else {
                match receiver.try_recv() {
                    Ok(message) => message,
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => Message::Finished,
                }
            };
            match message {
                Message::Frame(frame) => {
                    if frame.pts > self.clock + FRAME_AHEAD {
                        self.next = Some(frame);
                        break;
                    }
                    let delay = self.clock - frame.pts;
                    self.stats.max_delay = self.stats.max_delay.max(delay);
                    if delay > 0.20 {
                        self.stats.late += 1;
                        continue;
                    }
                    if latest.replace(frame).is_some() {
                        self.stats.superseded += 1;
                    }
                    // With time to catch up, give consecutive pictures
                    // their own refresh even if an audio batch made both due.
                    if self.stats.shown > 0 && delay <= KEEP_FRAME_WITHIN {
                        break;
                    }
                }
                Message::Error(err) => {
                    eprintln!("slot: native video: {err}");
                    self.failed = true;
                    return None;
                }
                Message::Finished => {
                    self.finished = true;
                    break;
                }
            }
        }
        latest
    }

    /// Upload at most one frame for each presentation, using compact YUV planes.
    pub(crate) fn overlay(&mut self, compositor: &mut Compositor, draws: &mut Vec<Draw>) {
        let frame = if self.path.is_some() && !self.failed {
            self.take_due_frame()
        } else {
            None
        };
        if let Some(frame) = frame {
            let (w, h) = (frame.width, frame.height);
            let started = Instant::now();
            if let Err(error) = compositor.upload_video_420(w, h, frame.planes.each_ref().map(Vec::as_slice)) {
                eprintln!("slot: native video: {error}");
                self.picture = None;
                self.failed = true;
                return;
            }
            self.stats.upload += started.elapsed();
            self.picture.get_or_insert_with(|| MoviePicture::new((w, h), Instant::now())).size = (w, h);
            self.stats.shown += 1;
            if let Some(started) = self.started_at.take() {
                self.stats.first_picture = started.elapsed();
            }
        }
        let Some(picture) = self.picture else { return };
        let alpha = picture.opacity(Instant::now());
        if alpha == 0.0 {
            if picture.fading_from.is_some() {
                self.picture = None;
            }
            return;
        }
        if alpha == 1.0 {
            draws.clear();
        }
        let (w, h) = picture.size;
        let canvas = (OUT_W as f32, OUT_H as f32);
        let factor = (canvas.0 / w as f32).min(canvas.1 / h as f32);
        let (w, h) = (w as f32 * factor, h as f32 * factor);
        let (x, y) = ((canvas.0 - w) * 0.5, (canvas.1 - h) * 0.5);
        // Only blend the uncovered bars. A full black rectangle would
        // darken the art twice where the movie itself crossfades.
        for [x, y, w, h] in [
            [0.0, 0.0, x, canvas.1], [x + w, 0.0, x, canvas.1],
            [x, 0.0, w, y], [x, y + h, w, y],
        ] {
            if w > 0.0 && h > 0.0 {
                draws.push(Draw::Rect { x, y, w, h, colour: [0.0, 0.0, 0.0, alpha] });
            }
        }
        draws.push(Draw::Video { x, y, w, h, alpha });
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

fn open_mp4(path: &Path) -> Result<Mp4Reader<BufReader<File>>, String> {
    let file = File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let size = file.metadata().map_err(|e| e.to_string())?.len();
    if size == 0 || size > MAX_FILE_BYTES {
        return Err("video file is empty or exceeds 1 GiB".into());
    }
    let mp4 = Mp4Reader::read_header(BufReader::new(file), size)
        .map_err(|e| format!("MP4 header: {e}"))?;
    if mp4.is_fragmented() {
        return Err("fragmented MP4 is not yet supported".into());
    }
    Ok(mp4)
}

fn video_track(mp4: &Mp4Reader<BufReader<File>>) -> Result<&mp4::Mp4Track, String> {
    mp4.tracks().values()
        .filter(|track| matches!(track.media_type(), Ok(MediaType::H264)))
        .min_by_key(|track| track.track_id())
        .ok_or_else(|| "no H.264 video track in MP4".into())
}

pub(crate) fn duration(path: &Path) -> Option<f64> {
    let mp4 = open_mp4(path).ok()?;
    let track = video_track(&mp4).ok()?;
    let ticks = track.trak.mdia.mdhd.duration;
    (ticks > 0 && track.timescale() > 0).then(|| ticks as f64 / f64::from(track.timescale()))
}

#[derive(Default)]
struct DecodeStats {
    samples: u32,
    frames: u32,
    read: Duration,
    decode: Duration,
}

impl Drop for DecodeStats {
    fn drop(&mut self) {
        if self.frames == 0 {
            return;
        }
        let ms = |elapsed: Duration| elapsed.as_secs_f64() * 1000.0 / f64::from(self.frames);
        eprintln!(
            "slot: native video worker: {} samples, {} frames, average {:.1} ms read + {:.1} ms decode per frame",
            self.samples, self.frames, ms(self.read), ms(self.decode),
        );
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
    let mut mp4 = open_mp4(path)?;
    let track = video_track(&mp4)?;
    let track_id = track.track_id();
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
    let mut stats = DecodeStats::default();
    for sample_id in first..=count {
        if cancel.load(Ordering::Relaxed) { return Ok(()); }
        let started = Instant::now();
        let sample = mp4.read_sample(track_id, sample_id)
            .map_err(|e| format!("sample {sample_id}: {e}"))?
            .ok_or_else(|| format!("missing sample {sample_id}"))?;
        stats.samples += 1;
        stats.read += started.elapsed();
        let pts_ticks = i128::from(sample.start_time) + i128::from(sample.rendering_offset);
        let pts = pts_ticks.max(0) as f64 / f64::from(timescale);
        for nal in parse_avcc(&sample.bytes, length_size) {
            if cancel.load(Ordering::Relaxed) { return Ok(()); }
            let started = Instant::now();
            let frames = decoder.decode_nal_with_meta(&nal, pts)
                .map_err(|e| format!("decode sample {sample_id}: {e:?}"))?;
            stats.decode += started.elapsed();
            for (frame, frame_pts) in frames {
                send_frame(frame, frame_pts, at, sender, &mut stats)?;
            }
        }
    }
    for (frame, frame_pts) in decoder.flush_with_meta() {
        if cancel.load(Ordering::Relaxed) { return Ok(()); }
        send_frame(frame, frame_pts, at, sender, &mut stats)?;
    }
    Ok(())
}

fn send_frame(frame: Frame, pts: f64, begin: f64,
              sender: &SyncSender<Message>, stats: &mut DecodeStats) -> Result<(), String> {
    if pts + 0.08 < begin { return Ok(()); }
    let frame = video_frame(frame, pts)?;
    stats.frames += 1;
    sender.send(Message::Frame(frame)).map_err(|_| "viewer closed".to_string())
}

fn video_frame(mut frame: Frame, pts: f64) -> Result<VideoFrame, String> {
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
    frame.y.truncate(pixels);
    frame.u.truncate(cw * ch);
    frame.v.truncate(cw * ch);
    Ok(VideoFrame {
        pts, width: frame.width, height: frame.height, planes: [frame.y, frame.u, frame.v],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn movie_picture_fades_in_and_reverses_without_flashing() {
        let start = Instant::now();
        let picture = MoviePicture::new((640, 360), start);
        let halfway = start + MOVIE_FADE / 2;
        assert_eq!(picture.opacity(start), 0.0);
        assert!((picture.opacity(halfway) - 0.5).abs() < 1e-6);
        assert_eq!(picture.opacity(start + MOVIE_FADE), 1.0);

        let closing = picture.close(halfway);
        assert_eq!(closing.opacity(halfway), picture.opacity(halfway));
        assert!((closing.opacity(halfway + MOVIE_FADE / 2) - 0.25).abs() < 1e-6);
        assert_eq!(closing.opacity(halfway + MOVIE_FADE), 0.0);
    }

    #[test]
    fn presentation_keeps_only_the_newest_due_frame() {
        let (sender, receiver) = mpsc::channel();
        let mut video = MusicVideo {
            receiver: Some(receiver),
            clock: 1.08,
            picture: Some(MoviePicture::new((1, 1), Instant::now())),
            ..MusicVideo::default()
        };
        assert!(!video.picture_ready(), "the picture held during a seek is not a new frame");
        for pts in [1.0, 1.0417, 1.0833, 1.125] {
            sender.send(Message::Frame(VideoFrame {
                pts, width: 1, height: 1, planes: [vec![16], vec![128], vec![128]],
            })).unwrap();
        }

        assert_eq!(video.take_due_frame().unwrap().pts, 1.0833);
        assert!(video.picture_ready(), "a buffered future picture releases the soundtrack");
        assert!(video.take_due_frame().is_none(), "keep future frames for their slot");
        video.clock = 1.12;
        assert_eq!(video.take_due_frame().unwrap().pts, 1.125);
    }

    #[test]
    fn steady_playback_presents_both_frames_after_a_short_delay() {
        let (sender, receiver) = mpsc::channel();
        let mut video = MusicVideo {
            receiver: Some(receiver),
            clock: 1.084,
            picture: Some(MoviePicture::new((1, 1), Instant::now())),
            stats: PresentationStats { shown: 1, ..PresentationStats::default() },
            ..MusicVideo::default()
        };
        for pts in [1.0417, 1.0833] {
            sender.send(Message::Frame(VideoFrame {
                pts, width: 1, height: 1, planes: [vec![16], vec![128], vec![128]],
            })).unwrap();
        }

        assert_eq!(video.take_due_frame().unwrap().pts, 1.0417);
        video.clock += 1.0 / 60.0;
        assert_eq!(video.take_due_frame().unwrap().pts, 1.0833);
        assert_eq!(video.stats.superseded, 0);
    }

    #[test]
    fn video_frame_preserves_decoder_planes() {
        let frame = Frame {
            width: 2, height: 2,
            y: vec![16, 16, 235, 235],
            u: vec![128], v: vec![128],
            pic_order_cnt: 0,
            chroma_format: ChromaFormat::Yuv420,
            bit_depth: 8,
        };
        let packed = video_frame(frame, 2.5).unwrap();
        assert_eq!(packed.pts, 2.5);
        assert_eq!(packed.planes, [vec![16, 16, 235, 235], vec![128], vec![128]]);
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
        assert!(video_frame(frame, 0.0).is_err());
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
                assert_eq!(frame.planes.each_ref().map(Vec::len), [320 * 180, 160 * 90, 160 * 90]);
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
