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
- Music Now Playing Y: toggle to video; Y again: album art. Physical lock
  still blocks Y. On missing/unsupported videos, continue playing music.
- Existing MP3/M4A decoder remains sole audio source (no duplicate video
  soundtrack, no ALSA teardown). Video frame presentation follows the
  *audible* music clock. A pauses music and video; Up/Down seeks both;
  track changes reset the video view/decoder.
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
- YUV420 frame -> RGB(A) conversion off the render thread. Upload to a
  **single reused GPU texture** with
  `Compositor::update_texture(tex, w, h, rgba)`; render using
  `Draw::Tex` with aspect-fit to the output screen. Existing Slot
  renderer supplies GLES2 and avoids any extra display backend.
- Start with a *known-good 480p/30 fps H.264 sidecar* and measure
  CPU, frame drops, elapsed time drift and memory. Fall back to
  360p/24 fps or lower resolution if the CPU cannot keep pace;
  do not imply 1080p support without measurements.

## Pre-install verification

1. Compiled and passed the complete Slot host/simulator Rust library tests,
   including Music Y-toggle and YUV420 conversion, in GitHub Actions run
   https://github.com/pkimcrombies/Slot-Brick/actions/runs/45115112228.
2. Built an actual ARM64 Slot binary using the repository's pinned Docker
   toolchain and confirmed AArch64 ELF with only the normal Slot runtime
   libraries. A dedicated additional run generates and decodes Baseline and
   Main/B-frame H.264 MP4 fixtures before packaging.
3. Prepared a source-commit-pinned installer with SHA256-verified ARM64
   binary, backup, byte-for-byte rollback, and no firmware/system changes.
4. Still required on-device: video startup; 30fps playback and MP3 continuity;
   Y-to-artwork; seeking; track change; physical lock; suspend/wake;
   quit/error fallback; CPU/thermals. Report results honestly.

## Current branch housekeeping

The prior external ffplay handoff was removed from the live Music and Brick
runtime code. Previously installed `video-player` files on the SD card
remain unused and are **not automatically removed**: keep them untouched
until a native-video hardware test passes. The installer modifies only the
`Slot/System/slot` executable after saving a checksum-verified original.

References:
- https://docs.rs/crate/rust_h264/0.6.1
- https://docs.rs/mp4/latest/mp4/
- https://github.com/cisco/openh264/wiki/
