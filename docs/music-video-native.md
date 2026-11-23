# Native Music video player — replacement for NX Redux ffplay

Status: **implemented and compiled for native ARM64**, host tests passed;
**no Brick Pro hardware playback validated**.
Target: TrimUI Brick Pro running Slot OS; branch `feature/psp-replaces-ps1`.
Rationale: native Slot OS image intentionally lacks NX Redux stock firmware's
legacy SDL, udev, OpenSSL 1.1, console helpers, etc. SDL hints did not prevent
`ffplay` from dlopening `libudev.so.0`; continued copied-library patches
were explicitly rejected by the user.

## Minimal acceptable experience

- Same-stem `Music/Artist/Song.mp3` + `Song.mp4` detection already exists.
- Y on Now Playing pauses the song and opens the companion MP4 from the
  beginning with its soundtrack. In the movie player, Left and Right
  seek ten seconds, A pauses or resumes, and Y, B or Menu closes it.
  Closing or finishing the movie returns to the song, paused at the
  position it was left. The physical hold switch still blocks Y.
- The movie freezes its clock and soundtrack while the first picture
  loads or the decoder seeks. Silent videos use their own elapsed time.
  Both modes use Slot's existing audio output.
- Use Slot's own EGL ES2 / texture uploads and screen pipeline. Do **not**
  run ffplay, SDL2, a separate display process, or a helper needing
  legacy stock-firmware shared libraries.
- Decode off the main UI/audio thread; bound frame queue and allocations,
  stop/reset on Y switch, suspend, track change, and seek. Error states
  must return to art without crashing the Music player.

## Implemented codec path

- `mp4` Rust crate: demux non-fragmented local MP4 and obtain H.264
  samples, codec configuration (avcC), presentation timestamps and
  keyframe/sample indices. Restrict scope initially to H.264 in MP4
  (not H.265, YouTube streams, other video codecs).
- `rust_h264` Rust crate: decode H.264 AVCC NALs in display order.
  It supports Baseline/Main/High H.264 with AArch64 NEON optimizations,
  Rust >=1.89 (Slot uses 1.96), and does not require SDL or FFmpeg.
  Evaluate decoder correctness and actual device performance; this
  relatively new library has not yet been validated on Brick Pro.
  OpenH264 is an alternate C decoder, but its official public decoder
  feature list specifies Constrained Baseline, making High-profile
  music videos less straightforward.
- The worker passes the decoded YUV420 planes to Slot's compositor. The
  video pass uploads the planes into reusable textures and converts them
  to RGB in its shader while fitting the image to the display.
- Start with a *known-good 480p/30 fps H.264 sidecar* and measure
  CPU, frame drops, elapsed time drift and memory. Fall back to
  360p/24 fps or lower resolution if the CPU cannot keep pace;
  do not imply 1080p support without measurements.

## If Y returns to the album artwork

Check `Slot/Logs/slot.log`. The first Brick Pro test reached the native
decoder and reported `no H.264 video track in MP4`. The installed
`Slot/System/slot-native` matched the ARM64 CI artifact by SHA-256.
The companion was a 1080p VP9 video (`vp09`) inside an MP4 container.

Inspect the video stream with `ffprobe` on a computer. Slot expects
H.264 (`avc1`) in a regular MP4. To convert a VP9 companion for an
initial 360p/24 fps device test, create a new file on the computer:

```sh
ffmpeg -n -i original.mp4 -map 0:v:0 -map '0:a:0?' -sn -dn \
  -vf 'scale=640:360:force_original_aspect_ratio=decrease:force_divisible_by=2,fps=24,setsar=1' \
  -c:v libx264 -profile:v baseline -level:v 3.0 -pix_fmt yuv420p \
  -crf 20 -g 24 -c:a aac -ac 2 -b:a 128k -movflags +faststart converted.mp4
```

Back up the original companion before replacing it. Install the converted
file beside the song with the same basename. Keep the MP4's soundtrack
when converting. Conversion runs on the computer.

For choppy playback, play a representative section and press Y to close
the video. In `Slot/Logs/slot.log`, the worker reports the average time
spent reading and decoding. The presentation summary reports texture
upload time, time to the first picture, displayed frames and frames
that arrived too late.

## Pre-install verification

1. GitHub Actions run
   [45116025887](https://github.com/pkimcrombies/Slot-Brick/actions/runs/45116025887)
   passed the Slot host regressions, including Music Y-toggle, and decoded
   Baseline and Main/B-frame H.264 MP4 fixtures before packaging.
2. Built Slot for ARM64 using the repository's Brick toolchain and checked
   that the AArch64 ELF links only the normal Slot runtime libraries.
3. Prepared a source-commit-pinned installer with SHA256-verified ARM64
   binary, backup, byte-for-byte rollback, and no firmware/system changes.
4. Still required on-device: video startup; 30fps playback and MP3 continuity;
   Y-to-artwork; seeking; track change; physical lock; suspend/wake;
   quit/error fallback; CPU/thermals. Report results honestly.

## Current branch housekeeping

The prior external ffplay handoff was removed from the live Music and Brick
runtime code. Previously installed `video-player` files on the SD card
remain unused and are **not automatically removed**: keep them untouched
until a native-video hardware test passes. An executable installer targets
`Slot/System/slot-native` after saving a checksum-verified original.
`Slot/System/slot` is the shell wrapper that starts it.

References:
- https://docs.rs/crate/rust_h264/0.6.1
- https://docs.rs/mp4/latest/mp4/
- https://github.com/cisco/openh264/wiki/
