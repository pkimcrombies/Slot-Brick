# Slot-Brick — ongoing development context

> Small handoff for future ChatGPT/AI coding sessions. **Read this first**, then verify the
> current branch and code in GitHub. This records user decisions, not a substitute for tests.
> Keep it brief and update it when a milestone changes. Last reviewed: 2026-11-23. Latest Music control changes (L1/R1 sections, D-pad Left/Right tracks, bottom-right repeat) are committed but not yet rebuilt or device-tested.

## 2026-11-23 Native H264 Music video: implemented and ARM64 verified

- The user requested to stop copying NX Redux `ffplay`/SDL stock-firmware
  dependencies after repeated `libbz2`, OpenSSL 1.1 and `libudev.so.0`
  failures. **Do not reintroduce the external player.**
- Native implementation: `crates/slot/src/music_video.rs` uses pinned
  `mp4=0.14.0` and `rust_h264=0.6.1` Rust crates. Background worker,
  bounded two-frame queue, MP4 H264 demux, SPS/PPS, B-frame PTS ordering,
  nearest sync sample seeking, 8-bit YUV420 to RGBA, fullscreen Slot GLES
  compositor drawing. Existing MP3/M4A decoder continues as sole audio.
  Y toggles video/art on Now Playing; B, section change, song change,
  sleep and physical lock preserve original Music behaviour. No ALSA/EGL
  handoff, SDL, FFmpeg, external libraries or OS changes.
- Confirmed on GitHub Actions native-video verification run
  **45116025887**, source revision
  `bc2627f99b0700adefa5ec6237bc37a3df7c0fbc`:
  309 Slot Rust unit tests; real generated 320x180 H.264 Constrained
  Baseline and Main/B-frame MP4 decode + PTS tests; and ARM64
  `sh brick/docker-build.sh` all passed. Downloaded compiled
  `slot` ELF AArch64, SHA256
  `f0094a3430fc260005b0f598dc2ad3a0d1c7cc966de544095947c36ba01ce79c`.
  Existing `Cargo.lock` was updated in branch by CI.
- Safe device test ZIP delivered:
  `Slot-Brick-Native-Video-ARM64-bc2627f-20261123.zip`;
  contains `Install-Slot-Native-Video.ps1` (valid inside ZIP per
  PowerShell-as-text skill), pinned ARM64 `slot-native-video` ELF,
  README and known-good matching 12s 640x360 H.264/MP3 media test pair.
  The installer only replaces `F:\\Slot\\System\\slot` after verifying
  an original backup under `Slot/System/native-video-backups`, writes
  a manifest for `-Action Revert`, and hash-checks installed binary.
  ZIP may be packaged as `.txt` with identical ZIP bytes for downloads
  blocked by the client, then rename to `.zip` prior to extraction.
  Do not touch boot image, games, saves, existing music, older ffplay
  trial libraries, or firmware.
- **Device playback is not yet proven**: user must test Y toggle,
  audio continuity, seek/suspend, real H264 performance and report
  logs. Codec limited to local non-fragmented 8-bit H264 MP4 up to
  854x480 and 1GiB. `Slot/Logs/slot.log` logs
  `slot: native video:` decoding errors (no new video-player.log).
  Revert installer if rendering or speed is problematic.
- Docs: `docs/music-video-native.md`. A prior CI failure was
  due to missing ffmpeg binary on GitHub runner; a separate unrelated
  flaky cart-art prefetch test was permitted one retry. Final run
  passed all mandatory checks.

## Where we work

- Repository: `pkimcrombies/Slot-Brick` (Rust Slot OS port).
- Active development branch: **`feature/psp-replaces-ps1`**. Do not modify `main`
  unless explicitly asked. Always fetch the current branch HEAD before edits;
  do not assume an earlier conversation's commit SHA is still the tip.
- Device: **TrimUI Brick Pro**, running native Slot OS from a 32 GB SD card.
  Primary working systems: GBA, GB, GBC, NES, SNES. PSP in the old PS1 slot
  is an **experimental, separate workstream**, not the current priority.
- See `CLAUDE.md` for repository conventions and `README.md` for build/packaging.
  The repo and its commit history are authoritative; this file is a conversation handoff.

## User preferences / design constraints

- Keep everything **native Slot (Rust)**. Do not introduce Trimpod, Flutter,
  ClassiPod itself, or a separate music launcher.
- Slot System, cartridge and utility menus use a restrained **black**
  background, off-white labels, muted values, pink selected text, aligned
  columns and consistent row spacing. Avoid bulky highlight bars/panels.
  Preserve physical cartridge previews and functional button hints; Music
  alone retains its separate warm-ivory palette.
- Slot launcher and system menus remain minimal **black**. Native Music uses
  **soft warm ivory (#F5F1EA; changed from near-white #F8F7F4 after user
  feedback on 2026-11-22)**, charcoal text and darker dusty-rose selection
  across album lists, album details, Now Playing and the Cover Flow screen.
  The album song-list header is a quiet deeper ivory (#EFEAE1). No changes
  to global menus, game views, texture decoding or cover placeholders.
  Keep Slot's typography and physical buttons; preserve game animations.
- Prefer short, smooth fades. Avoid black flashes, extra loading screens and
  needless redraws/CPU use.
- For each build-worthy change, provide the existing **one-command PowerShell
  installer** workflow: exact Git commit, regression tests, ARM64 build, backup,
  replace and SHA-256 verification. Distribute standalone PowerShell as `.txt`
  for renaming to `.ps1`. **Every time an installer is delivered, the
  supplied PowerShell command MUST include the `.txt` -> `.ps1` rename
  and the execution in the same copy/pasteable command block**. Never require
  the user to rename it manually or give only a bare `.ps1` execution line.
  For example (replace the filename with the actual delivered file):
  ```powershell
  $name = 'Install-Slot-Brick-Example'
  Rename-Item -LiteralPath "$HOME\Downloads\$name.txt" -NewName "$name.ps1" -ErrorAction Stop
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$HOME\Downloads\$name.ps1"
  ```
  Never silently overwrite a working installation.
- Latest code changes on this branch (as of 2026-11-22): `3dfb0d4`
  sleep fade, `f055987` power test expectations, `1e45f95` classic song
  list, `e9b77ac` Cover Flow labels/counter removal, `d66b63c` reused
  GPU album art, `14f850e` soft ivory Music theme. These are commits,
  **not proof of a successful device build or installation**.
- User primarily works on Windows. Previous checkout:
  `C:\Users\pk\Downloads\slot\Slot-Brick-PSP-Feature`; SD usually `F:\`.
  **Treat both as defaults, not guarantees; verify before copying.**
- Don't claim code was compiled or device-tested unless it actually was.
  The user prefers concrete changes and direct answers over speculative mockups.

## Current features / important locations

- Native Music: `crates/slot/src/music.rs`, `music_view.rs`,
  `music_cover.rs`, `music_library.rs`, `music_tags.rs` and `music_m4a.rs`.
  Embedded MP3/M4A art loads asynchronously; album rows have 40px covers.
  **Now Playing navigation and background browsing (2026-11-23, not yet
  ARM64-compiled or device-tested):** the real Music entry defaults to a
  standalone **Now Playing** section, with Albums, Artists and Settings
  alongside it.
  Music opens on Now Playing. **B from Now Playing** opens the track list
  belonging to the currently playing album, with that song highlighted
  (regardless of the last browsed album); B again opens the Albums root
  and highlights the playing album. Without a track B opens Albums root.
  B on an Albums/Artists/Settings root does nothing: only **HOME/Menu**
  (`Action::QuickMenu`) exits Music and returns to Slot's menu. **Start is
  unmapped in Music.** L1/R1 cycle all four Music sections (Now Playing,
  Albums, Artists, Settings) from any page, including during playback.
  On Now Playing with
  an active song, A pauses/resumes, D-pad Left/Right skip
  previous/next track, Up/Down seek +10/-10 seconds, and X cycles repeat.
  Seeking is applied in `Music::update_audio` using the existing playback
  decoder and Ring to flush old queued samples; seeking while paused does not
  resume audio, and pending seek input can accumulate before decoder startup.
  With **no song loaded**, all D-pad directions and A leave the empty Now
  Playing screen unchanged; L1/R1 alone switch sections, while B opens
  Albums root. Button hints such as "x repeat" and "x list" remain absent.
  Music::Track now owns its album ID and song index independently of the
  browser location: audio decoding, repeat, metadata, album cover and track
  count all use the playing album even when the user browses another album.
  Automatically advancing tracks never takes over the current browsing
  screen or selection. Browsing and B do not exit Music or stop audio;
  HOME/Menu still exits Music and stops its music. Playback outside
  the Music phase has NOT been introduced.
  **Shared shelf HUD in Music:** `app.rs` uses the same
  `slot_ui::draw_status_bar` for Music and Shelf. Clock stays at the top-left,
  battery/charging, connected Wi-Fi and update symbol at top-right with the
  same black icons. Album-detail title/artist were shifted down to clear it.
  Do not add a second HUD implementation or put the status bar behind the
  ivory/gradient background. Added unit regression coverage for navigating
  to/from Now Playing without stopping music, browsing another album while
  preserving the original song, and the shared clock position.
  **Music Settings + repeat (Nov 2026, pending Rust/device validation):**
  **Settings is the fourth Music tab** (after Artists), reached by L1/R1,
  not Start. Up/Down select rows, Left/Right/A change values; B is inert
  at this root, just like Albums and Artists. L1/R1 keep cycling tabs.
  Its two-column label/value layout follows Slot System Settings:
  `QUICK_PITCH` (48px) row rhythm, `QUICK_VALUE_EDGE` second column,
  selected labels/values in dusty rose without full-width pink selection
  bars or dividers, and Music's existing warm-ivory background/black HUD.
  The screen and existing settings values remain unchanged.
  Rows: Music-only digital volume cap (default 70%, steps of 10 to 0-100),
  default album browse view (Cover Flow/list), album sort (A-Z/newest tagged
  release date first), Repeat (off/all/one), and **Dim after** (Off/30 seconds/
  1 minute/2 minutes; default 1 minute), and **Dim brightness**
  (physical backlight step 1-9, default 1). **X** on Now Playing cycles
  repeat; the indicator at bottom-right (20×20, matching HUD scale, below progress and time)
  now loads the exact UXWing Repeat Off/All/One transparent PNG icons from
  `Slot/System/music-icons/repeat-{off,all,one}.png`, cached as three GPU
  textures and tinted neutral grey for Off (#888888), black for All/One (#000000). The installer downloads and verifies
  the three assets once, for offline runtime use; an internal glyph fallback
  remains for older cards without icons. The three UXWing pages are
  https://uxwing.com/music-player-no-repeat-symbol-icon/,
  https://uxwing.com/music-player-repeat-symbol-icon/, and
  https://uxwing.com/music-player-repeat-one-time-symbol-icon/.
  Off plays the current album once; All wraps at the last track
  of the current album; One repeats the current song. These preferences
  persist independently at `Slot/Config/music.settings` with a temp-file
  rename. The volume cap is applied to decoded PCM, including when the
  normal device volume is raised; lowering it also clears buffered louder
  samples. It is NOT a certified sound-pressure/dB limit: safe headphone
  levels still depend on device, headphones and listening duration.
  Album sorting uses a separate ordered list of stable album IDs, not
  mutation/reindexing of playing albums or artwork. Release-date tags from
  MP3 ID3v2 (TYER/TDRC/TDRL/TDOR), ID3v1 and M4A (standard Date/ReleaseDate)
  are used (full YYYY-MM-DD where available; year-only tags remain supported);
  missing dates sort last and never use filesystem timestamps.
  Playback controls: A pause/resume, Left/Right previous/next song,
  Up/Down seek +10/-10 seconds, L1/R1 switch all four Music tabs,
  and X changes repeat mode. B opens the playing album's song list
  without stopping playback; HOME/Menu exits Music **unless locked**.
  The hardware volume-key chord acts as Music Lock instead of mute.
  **Music sleep/resume + dim (2026-11-23, pending device validation):**
  Physical backlight dimming is optional, applies only while Music is actively
  playing and the settings screen is closed. Previously it halved the
  normal brightness step (e.g. 6 to 3), which remained fairly bright.
  Now the independent **Dim brightness** setting selects an absolute hardware
  step 1-9 (default 1, the lowest nonzero brightness), capped so it never
  exceeds the normal setting. Both dim settings live in `Config/music.settings`;
  any input restores normal brightness, and no automatic screen-off is
  introduced. The new setting and UI have not been device-validated. Power still triggers the existing 300ms fade and actual kernel
  suspend. Previously `App::doze()` destroyed Music and `wake()` returned to
  the launcher; now a Doze-origin flag preserves the same Music object,
  current browsing location, playing/paused status and track index through
  suspend, and restores `Phase::Music` on wake. `Session::update` stops
  feeding audio while Doze, rolls back unsounded PCM with
  `Music::prepare_suspend(&Ring)`, and discards the ring for ALSA to release.
  After true kernel wake, `Frontend::resume_sleep` reopens the AudioSink
  at its prior sample rate before resuming PCM, so the song can continue
  from the retained decoder position. The normal backlight level is
  restored by `App::wake()` before `Power::on_open()`; no new daemon
  or SD write is required to remember an ordinary suspend. True power loss
  and deliberate poweroff do **not** have persistent music resume.
  Added regression tests for settings persistence, repeat order, volume cap,
  release-date parsing, stable album IDs, Music phase on wake, retained
  selected track, and backlight restoration. **Rust, ARM64, and on-device
  suspend/wake/audio tests have not yet been executed**.
  **Album song list: original iPod Classic music-menu style (2026-11-22).**
  The earlier ClassiPod *Cover Flow selection panel* interpretation was
  rejected: no floating bordered panel, coloured gradient header, large
  artwork backdrop or Cover Flow flip animation. Native
  `music_view.rs::album_tracks` now draws a full-width warm-ivory list with
  a quiet album/artist title bar, seven 44px rows, track numbers **before**
  titles, subtle separators, one full-width dusty-rose selected row, a slim
  scroll indicator for long albums, and a small position count. The old
  `album_gradient` texture/panel logic are removed. It is the same detail
  screen reached from Cover Flow, regular Albums or Artists.
  `music_library::Song` does not have a duration field: never fabricate
  times. Existing Cover Flow, Now Playing, physical input, sleep and menu
  fades must remain unchanged. Brick Pro visual QA and Rust build are pending.
  **Now Playing artwork reuse (2026-11-22):** `MusicView::render` previously
  requested a fresh `MusicCover` whenever a track started, even if a decoded
  and uploaded 256px album cover was already visible in Cover Flow or Albums.
  Now it checks the existing per-slot GPU texture by the album's first-track
  path, reuses the existing `TexId` immediately, or polls an in-flight
  browser decoder rather than duplicating work. Only uncached albums use
  Now Playing's own asynchronous decoder. The existing seven Cover Flow +
  six list slots remain bounded; no unbounded texture map. Texture-key
  regression tests cover both browsing modes and stale/different albums.
  **Dynamic album-colour Now Playing background (2026-11-23):**
  `music_cover.rs` samples one pixel out of 16 from the existing downscaled
  RGBA thumbnail on the **decoder worker**, bins chromatic hues and retains
  a representative RGB tint alongside the cover's GPU texture in each
  bounded `MusicCover` slot. `music_view.rs` reuses that tint immediately
  from Cover Flow or album-list caches, or as soon as pending artwork is
  decoded. The first version used 32 horizontal rectangles with a subtle
  top-to-bottom tint (32% to 6%), which the user confirmed worked but
  wanted a little stronger and diagonal. The new implementation uses one
  **80x60 linearly filtered, cached RGBA gradient texture**, blending album
  colour from **43% at top-left to 10% at bottom-right**. The texture is
  generated/updated only when the sampled album colour changes, never on
  every render frame, and needs just one GPU draw call. No new image reads,
  extra decoders, shaders or per-frame artwork analysis. Music Settings,
  Cover Flow, Albums and Artists retain plain `#F5F1EA`. Near-monochrome/
  missing artwork falls back to ivory. Test the diagonal direction, both
  warm/cool artwork, text contrast, transitions between albums and GPU
  performance on the actual Brick Pro.
  This is **reuse of visible/in-flight GPU artwork**, not a persistent
  all-albums or disk cache: artwork not yet requested must still load.
  Verify first-frame appearance, subsequent songs from the same album, and
  revisiting previously browsed covers on the device.
  Korean glyph fallback via `crates/slot/src/fonts.rs` and
  `crates/slot-ui/src/text.rs`; SD font path:
  `Slot/System/Fonts/.SlotHangul.ttf`. It is optional.
- Album root defaults to **native Cover Flow** on warm ivory. The first
  flat approximation (260ms / screen-plane tilt) was rejected by the user.
  Replacement is based on ClassiPod's **Original** carousel, not Big:
  `adeeteya/Classipod` master:
  `lib/features/music/cover_flow/widgets/cover_flow_carousel.dart`.
  Translate its offset (0.25 screen for first neighbour, then 0.085),
  22% side scale, 0.95 radian Y rotation, perspective 0.35/size,
  distance draw order, and 400ms easeOutSine into native `slot-gfx`
  GLSL/Draw::CoverFlow. No Flutter. After inspecting a real device photo
  on 2026-11-22, the user asked to **remove the mirrored reflections** as
  outdated. Covers keep their original 3D perspective; the extra mirrored
  draw pass, shader UV/fade path, and reflection uniform have been removed.
  Cover Flow now omits the lower `1 / N` album counter entirely and moves
  the centred album title and artist up 24px (to y=365 and y=402), leaving
  carousel positions, animation and album-opening controls untouched.
  Both Albums and Artists use dark charcoal for their selected tab label
  (muted text for the inactive tab); the earlier pink Albums label in
  Cover Flow was inconsistent and is removed. Dusty rose remains for
  song/list selections. In the four-section Music navigation,
  `L1/R1` switch Now Playing/Albums/Artists/Settings. In Albums, `X` toggles
  Cover Flow/list, `A` opens, `Y` plays, and `B` backs out.
  In Cover Flow only, Up/Down move to previous/next album-title initial
  (skipping missing letters and wrapping); Left/Right move one cover.
  Empty Now Playing shows only tabs and the normal shelf HUD. Settings
  retains six rows without bottom help captions.
  The former `x list`/`x covers` hints have been removed.
  Renderer: `slot-gfx/src/draw.rs` and `shaders.rs`; Music layout:
  `music_view.rs`. The `frontend.rs` animation redraw gate must stay
  bounded. First device installer stopped safely during Rust compilation:
  adding `Draw::CoverFlow` required `slot-ui/src/peel.rs::fade_from` to
  include the new sprite variant in its alpha match. The follow-up commit
  adds this exhaustiveness fix without changing layout or shader behavior.
  Hardware and ARM64 compile verification are still pending.
- Now Playing was redesigned (commit `7515789`, later revised): **250px artwork
  on left** (previously 214px), track/artist/album and position on right,
  progress/timestamps lowered to y=414/432, and the 20px UXWing Repeat icon
  aligned at y=434 next to the end timestamp. Keep physical playback controls.
  Only a long Now Playing song title scrolls horizontally; other metadata
  and shorter titles remain static. Device feedback is pending.
- Fades are in `crates/slot/src/frontend.rs` (`NavFade`, `PowerFade`);
  sleep handling also uses `device_app.rs`. Boot 440ms, menu 220ms,
  sleep 300ms, wake 360ms, shutdown 210ms. **Brick sleep-fade fix
  (2026-11-22):** `App::doze()` had called `Power::on_close()` too early,
  physically setting the backlight to zero before the 300ms GPU fade could
  become visible. For Brick only, defer that hardware close until
  `Frontend::sleep_fade_frame()` has presented its last black frame;
  `App::finish_doze_fade()` closes once then, with a mock-backlight test
  to protect mid-fade brightness and wake restoration. The first installer
  caught two old `crates/slot/tests/power.rs` integration assertions that
  expected immediate backlight-off on entering Doze. Those tests exercise
  `Session` without `Frontend`'s real 300ms frame loop, so on Brick they
  now assert the backlight remains lit while fade is pending; the separate
  `app.rs` mock test verifies turning it off when the fade completes.
  Non-Brick lid semantics, the 3.3s suspend delay and wake fade remain unchanged.
  The new Music-resume behaviour must not alter ordinary game/shelf sleep,
  sleep/power-off policies, or the music playback rate.
  Returning from game ejection **must not** add a black navigation fade;
  fix `85b0331` was confirmed smooth.
  First-frame boot fade must also work when Slot starts in `Phase::Inserting`
  (saved cartridge), not only `Phase::Shelf`; track this independent of
  later navigation transitions.
- Shutdown (commit `3daabad`): **no “Saving and Shutting Down” screen** on
  Brick; preserve black fade, plus 70ms GPIO rumble on deliberate power-off
  if Rumble is enabled, *after saves are settled*. Auto/battery shutdown
  stays silent; exit-to-stock keeps its separate behaviour.
  User device verification of this latest change is pending. The initial
  quiet-shutdown installer failed in 3 new tests because the temporary
  test root lacked `Config/` before `write_slot_state`; the subsequent fix
  creates the standard Slot directories first. Do not bypass the tests.
- Initial Cover Flow ARM64 installer failed after tests due to a false-negative
  binary check: it searched for an 8-character Git prefix, although
  `crates/slot/build.rs` embeds `git rev-parse --short HEAD` (normally 7).
  Use the actual `git rev-parse --short HEAD` output for ELF string checks,
  print meaningful per-stage errors, and keep full SHA pinning before install.

## Test/build/install guardrails
- **2026-11-23 thumbnail build failure correction:** User's installer
  stopped safely at regression-tests compilation (exit 101, SD unchanged).
  `music_cover::embedded_cover_sized(path, size: u32)` reused `let size`
  for `music_tags::id3v2_size`, shadowing the thumbnail dimension
  with a `usize`. This produced Rust E0308 at
  `choose_cover_sized(metadata.visuals(), size)`. Renamed that local
  value to `tag_bytes` and preserved the `u32` thumbnail size.
  No production behaviour change intended. Re-run test/build on
  user's Docker toolchain before declaring installation successful.


- **2026-11-23 Music Now Playing smoother title marquee and labels:**
  Music's first tab is visually called **"now"** (tab order and
  navigation unchanged). While playing, the redundant "playing"
  status below "X of Y" is hidden. X-of-Y moves from y=277 to y=326.
  Paused, loading and genuine errors still show at y=358.
  Progress/time and Repeat positions are unchanged.

  Long titles previously recalculated widths, rasterized every glyph
  and uploaded a new 26px image on every ~80ms animation frame.
  New implementation computes width once per song, rasterizes full
  text into a bounded RGBA buffer once, and crops a compact fixed-width
  viewport into one reused GPU texture. Frame requests increase to
  ~30fps (33ms) only while the long-title marquee is active.
  Scroll speeds are 38/40 px/sec forward/back; resting at each end.
  A changed track/scale invalidates the whole-title raster.
  New regression tests cover viewport edge clamping, raster reuse and
  title changes. Audio and album covers are unaffected.
  Real Brick Pro runtime performance and regression tests remain
  unverified until the user's guarded installer runs.

## Test/build/install guardrails

- **2026-11-23 Music HUD lock icon + album-list performance overhaul:**
  A downloaded UXWing `lock-icon` PNG is staged as
  `Slot/System/music-icons/lock.png`, decoded/resampled to a black 18px
  native texture and rendered only while Music Lock is active. It sits
  immediately to the left of the battery gauge in the **shared top HUD**,
  with Wi-Fi/update icons shifted left as needed. The former handmade
  padlock in the Music tab row has been removed; all lock, mute and
  physical-switch behaviour remains unchanged. `slot-ui::footer` now
  offers `draw_status_bar_with_lock` while retaining the existing
  `draw_status_bar` signature for the shelf and other screens.

  Albums **list** uses 9 rows of 40px (instead of 6 rows of 48px) and
  smaller 34px drawn covers, with 21 rolling 64px GPU thumbnail slots
  (cursor ±10) rather than 6 reused full 256px images. It requests the
  nine visible albums first, prefetches up to ten on either side, and
  limits concurrent new list-cover decode requests to three. While workers
  are pending, the frontend refreshes at 100ms; when warm it returns
  to normal low-CPU idle rendering. A slender 2px rose scrollbar at the
  right indicates list position and size; long titles use the available
  column without overlapping the rail. Image decoding remains off-thread.
  Optimised 64px PNGs are stored automatically on SD at
  `Slot/Cache/Music/thumb64-v1/*.png`. Filename keys include file
  paths, sizes and modification times of every album candidate track,
  so updates produce new cache entries. Failed disk writes do not block
  artwork display. Previous cache files can be deleted safely.
  **Cover Flow remains 256px**, and Now Playing no longer enlarges tiny
  64px list textures to 250px; it reuses full Cover Flow art or decodes
  one full-quality source. This first-run conversion will still do real
  work, but repeat browsing should use the smaller PNGs. Regressions
  cover cache round trips, invalid PNGs, sizes, modulo window ownership,
  layout and shared-HUD lock positioning. Hardware build/performance
  validation remains pending.


- **2026-11-23 physical mute switch / Music Lock correction (hardware
  test pending):** Earlier Music Lock remapped only the
  `VolUp+VolDown` gesture's `Action::MuteToggle`, but ignored the
  Brick Pro's separate physical mute switch, read through
  `/sys/class/gpio/gpio243/value` every ~200ms. The switch set
  `App::brick_mute`, and `App::muted()/output_volume()` still reduced
  Music gain to zero, even though the Music UI showed a lock.
  Updated `App::set_brick_mute` to synchronise the physical switch with
  the Music lock, including if engaged before opening Music, and if
  changed during retained-Music Doze. In Music only,
  `App::muted()` ignores the physical GPIO mute bit, preserving
  the actual `state.muted` or `state.muted_hp` software mute.
  The switch no longer displays the global Volume HUD or writes
  mute/volume settings while Music is open. Normal physical mute
  behaviour outside Music remains unchanged, including after exit.
  With switch ON, a volume-key chord cannot override the physical
  lock; moving switch OFF unlocks. Fixed `remember_volume()` to snapshot
  *software* mute instead of hardware-effective mute, avoiding accidental
  propagation into saved mute flags. New Brick regression tests cover
  GPIO-on-before-entering Music, live switch ON/OFF, audible output
  while locked, volume control suppression, chord/switch precedence,
  ordinary non-Music mute, and switch changes during sleep/wake.
  **Do not claim hardware-proven**: software output is verified by
  tests only; device-level ALSA/hardware mute must be checked on Brick Pro.


- **2026-11-23 installer regression fix:** User's Docker run compiled successfully
  and executed 294 library tests: 285 passed, 8 ignored, 1 failed.
  The failure was `music::tests::ten_second_seek_changes_decoder_position_and_preserves_pause_state`
  at the synthetic MP3 duration assertion (`time().1.unwrap()`):
  the generated MPEG frame stream has no Xing/VBR duration header, and
  dr_mp3 legitimately returns an *unknown* total duration even though
  the frame data supports seeking. Changed only that test assumption to
  accept `None` or a plausible reported duration and keep the actual
  +10/-10 audio seek assertions, pause-state checks, and ring-flush checks.
  The install stopped at regression-tests exit 101 *before* any SD mutation.
  New installer must pin the HEAD after these fixes. Remaining tests, ARM64
  build, and hardware behavior are still pending rerun on user's machine.


- **2026-11-23 Artists letters + Music lock (not yet device-built):**
  The Artists root no longer treats D-pad Left as a tab shortcut to
  Albums. D-pad Left/Right (and ShelfLeft/ShelfRight) jump to the
  previous/next *available* artist initial; empty letters are skipped,
  both ends wrap, and a one-letter/empty artist library preserves its
  position. Up/Down still move one artist, A opens an artist, and L1/R1
  exclusively switch Music tabs. Albums list no longer uses D-pad Right
  for implicit tab navigation. Existing coverflow D-pad behaviour
  (Left/Right covers, Up/Down album letter jumps) stays unchanged.

  **VolUp + VolDown within 200ms** previously emitted MuteToggle and
  changed global mute/volume. Now in Music only it toggles a temporary
  hold lock without muting. The existing pre-chord volume-step snapshot
  is restored on toggle so a lock gesture does not alter loudness.
  Lock applies across all Music tabs and ignores navigation, repeat,
  seeking, playback, volume, brightness and HOME; the same chord
  unlocks it. A small unlabelled padlock appears in the Music tab row.
  While locked, the existing Music dim brightness level (default 1)
  is applied immediately, regardless of pause or configured dim
  timeout; ignored keypresses never restore brightness. While locked,
  Music will not automatically idle-suspend, even when paused.
  Physical PowerTap/PowerHold/Lid transitions remain available for
  sleep/shutdown safety; existing power-menu navigation is preserved.
  On unlock, the prior normal/idle brightness policy resumes. Lock is
  in-memory only: never persisted or carried to a new Music launch,
  but survives kernel sleep/wake within the retained Music session.
  Global mute functionality outside Music remains unchanged.
  Regression tests check letter jumps, empty/same-letter sets,
  volume restoration, ignored locked controls, HOME, sleep/wake,
  and immediate dim policy. Rust/ARM64 and hardware checks pending.


- **2026-11-23 Now Playing controls/layout/marquee (pending validation):**
  Album art enlarged 214->250 logical pixels, x=24, y=112; the metadata
  column remains 300+ pixels wide. Progress moves from y=372 to y=414
  and elapsed/total time to y=432. UXWing Repeat (20px; black for active,
  grey when off) moves from isolated bottom-right to x=596, y=434,
  alongside and 12px to the right of the end time's right edge at x=584.
  Up/Down now seek forward/back 10s, with pending commands applied on
  the audio thread's regular update and the Ring flushed; paused stays
  paused. Left/Right continue skipping songs and L1/R1 cycle sections.
  Long song titles (>available width) now scroll left, pause, scroll
  right, and repeat in a strictly cropped title-only 26px viewport,
  with gentle 80ms redraws while visible. Slot UI uses a bounded
  raster viewport, preserving the existing font/fallback and background.
  Static/short titles and other tabs incur no animation cost.
  Regression tests added for time/repeat/art layout, marquee rendering,
  and real MP3 seek on the audio Ring. Korean font was NOT replaced:
  existing `Slot/System/Fonts/.SlotHangul.ttf` can be swapped on SD
  with a different static TTF. Candidate typefaces are Pretendard,
  Noto Sans KR, Gowun Dodum. Recommend testing one visually and preserving
  the current file until a choice is confirmed. No changes to game assets,
  saves, persistent music settings or background playback.


- **2026-11-23 Cover Flow warm artwork cache:** Previously the visible
  7 covers (selected +/-3) were also the entire GPU cache, and entering
  another window recycled those textures and restarted async tag/image
  decoding. Now Cover Flow still renders 7 covers but uses 15 rolling
  `MusicCover` slots (selected +/-7, adding four off-screen on each side).
  The seven visible requests are queued before the additional eight,
  and the album-keyed worker results are only uploaded on the render thread.
  A slot can immediately cancel/drop a stale in-flight request and start
  decoding its new album instead of waiting for the previous worker to
  complete. While the carousel has pending artwork, the frontend polls
  about every 100ms rather than waiting for its normal 1-second idle
  redraw, so ready covers are presented without another button press.
  Once loading settles, no extra frames are requested. New regression
  tests cover prefetch range edges, unique
  modulo slot ownership, and cooperative cancellation. Estimated peak
  cached GPU RGBA memory is ~3.75 MiB for 15x256x256 textures (previous
  7 slots ~1.75 MiB), plus decoder buffers. No persistent on-disk artwork
  cache; first visit still needs decoding and rapid long alphabet jumps
  may display placeholders until the target window warms. Do not block
  the render thread or touch game/launcher artwork. Rust/ARM64 build and
  device-performance validation are pending.


- **2026-11-23 Music minimalism + Cover Flow letter navigation:**
  Empty Now Playing deliberately renders only the Music tabs beneath the
  shared HUD (removed "nothing playing" and the choose-album subtitle).
  Music Settings retains its six rows and ivory/two-column design but
  removes both bottom instructional/safety captions; these remain true
  operating considerations, not permanent screen copy. Cover Flow alone:
  Up goes to the previous available initial, Down to the next initial,
  skipping unused letters and wrapping at the ends (including the `#`
  group); Left/Right still move one album. Alphabet jump targets the first
  displayed album for that letter while keeping A-Z or newest-first
  album sorting unchanged. A sole initial/empty library is unchanged.
  Jumps >3 cover positions snap immediately rather than animating through
  intermediate artwork and wasting image decodes; adjacent Cover Flow
  navigation retains the existing 400ms ease-out animation. Test coverage
  added for missing letters, wrapping, shared initials, release sort and
  far-jump animation. No changes to playback, Music tabs, settings values,
  saved files or video (still deferred).


- **2026-11-23 cartridge Start-menu visual alignment:** The shared
  `slot-ui/src/cart_menu.rs` renderer (GB/GBC/NES/SNES Start) now
  uses Slot's `MENU_BACKGROUND` black canvas, `QUICK_PITCH` 48px
  centered row rhythm, a text-only two-column label/value layout,
  `MENU_TEXT_INK` for ordinary labels, `MENU_MUTED_INK` for ordinary
  values and `MENU_SELECTION_INK` rose for the selected label/value
  and its two adjustment carets. Removes the old filled highlight slab.
  Uses lower-case text with a smaller fallback font for long shell
  shapes/names; keeps the large live cart preview on the left. Footer
  hints remain (B Cancel, directional Change, A Save/Reset, Y Label)
  and grey out Y when offline. `CartMenuFaces.labels` now stores
  normal/selected pairs, pre-uploaded once in `frontend.rs`.
  No `app.rs` control, storage, shell geometry, or label-search
  behaviour changes. GBA's animated core/lid picker remains intentionally
  separate. Added four layout/palette/legend tests; Rust build and
  Brick Pro hardware checks are pending.


- **2026-11-23 Settings as fourth Music tab (not compiled/device-tested):**
  Four-section L1/R1 wrap order is Now Playing, Albums, Artists, Settings.
  Start is unmapped; Settings B is inert; HOME/Menu alone exits Music.
  Up/Down select settings; Left/Right/A modify them. Entering Settings
  resets to the first row; browsing Albums/Artists resets to root.
  `MusicView::settings` uses the same two-column text-only selection
  and 48px pitch as Slot's system QuickMenu, while retaining Music's ivory
  palette and 24px HUD-aligned left margin. Cover Flow decoding is skipped
  while Settings is shown. The six existing settings and audio behavior
  are unchanged. Unit regressions updated for four tabs, persistence,
  Start/B behavior, and ongoing music. Music-video exploration is on hold.


- **2026-11-23 UI refinement:** UXWing repeat symbol reduced from 28x28
  to 20x20, matching the visual scale of battery/Wi-Fi HUD indicators.
  The icon remains at the lower-right, at x=596, y=440 on 640x480,
  with a 20px bottom clearance. Grey Off/black All+One preserved.
  All shuffle feature changes from the interrupted request were
  intentionally removed; no shuffle logic, settings, button handler,
  icon or dependency should ship. Repeat icon remains the only change.


- **2026-11-23 repeat colour refinement:** repeat Off is neutral grey
  `#888888`, All and One are solid black `#000000`, matching the HUD.
  Only the texture tint/fallback colours changed; retain 28x28 at (588,432).
  A unit regression test checks all three ink values.
  No change to on-disk UXWing files, input or playback controls.


- **2026-11-23 Music margin/icon refinement:** Shared Shelf HUD left
  clock at x=24, while Music used `QUICK_EDGE=32`. Exposed the
  existing `STATUS_MARGIN` from `slot_ui` and made Music's
  `EDGE=STATUS_MARGIN=24`, aligning tabs, artwork, list rows and
  playback/progress left edges without changing screen semantics.
  Reduced UXWing repeat texture from 32x32 to 28x28, retaining the
  bottom-right anchor (x=588, y=432 and 20px bottom clearance)
  and smaller RGBA decode/upload. Added layout tests.
  Rust/ARM64 build and device visual verification pending.


- **2026-11-23 UXWing repeat icon layout regression:** installer pinned
  `3d9ae19` stopped before touching SD: 265 passed, 1 failed, 8 ignored
  in `repeat_badge_fits_bottom_right_without_covering_progress_or_time`.
  With `QUICK_EDGE=32`, a 32px icon had y=416, violating the test's
  minimum y>417 below the progress/timestamps. Commit `1637794`
  lowers the icon to y=428 (20px bottom margin), keeping x at 576;
  test asserts both clearances and the exact 428px coordinate.
  Build, other suites and device validation of the revised commit
  remain pending. Previous install aborted with SD unchanged.


- **UXWing repeat assets (2026-11-23):** The Music renderer reads three
  PNG files under `Slot/System/music-icons/` exactly once per
  `MusicView` lifecycle; the `png` decoder is native/offline and the
  RGBA is downscaled to 32x32 before uploading. The three files are not
  checked into Git because downloading them occurs during the Windows
  installer preflight. The installer must validate PNG signatures,
  backup/verify any replaced icons, and copy all three and `slot-native`
  only after successful Rust tests and ARM64 build. The SD `slot`
  wrapper, Games, Saves, Config and Music must not be touched. If UXWing
  download fails, abort before modifying SD. Device visual test pending.


- **2026-11-23 Music Back/exit policy:** B from Now Playing with a
  current track always opens `Page::Album(track.album)` under an Albums
  root, selecting the playing track (not a previously browsed album).
  Root Albums cursor follows sorted album ID, so B again selects that
  album in Albums/Cover Flow. Empty Now Playing B opens Albums root.
  B at Albums/Artists/Settings root never exits. Only `Action::QuickMenu`
  (physical HOME/Menu) exits Music; Start is intentionally unmapped. Updated
  integration and unit regressions in `music.rs` / `quick_menu.rs`.
  Rust tests, ARM64 build and on-device verification are pending.


- **2026-11-23 empty Now Playing navigation:** after a device report,
  removed the old Right/A shortcut to Albums while there is no active
  track. All D-pad/analog navigation and A are inert in this state;
  L1/R1 continue to switch Music sections, and B remains Back.
  Regression test added in `295d65e`. Pending Rust/ARM64/device validation.


- **2026-11-23 section-switch regression:** installer pinned to `5a522cd`
  stopped safely after 260 passed, 1 failed, 8 ignored in
  `music::tests::shoulders_cycle_all_three_sections_without_skipping_tracks`.
  After Y began playback from a track list, pressing R1 from Now Playing
  left the browser inside `Unknown Album` instead of opening the Albums
  root. Fix in `bddd9eb`: L1/R1 always enter the destination section's
  root, regardless of nested browsing state; B from Now Playing still
  restores the originating album track list without stopping audio.
  Added explicit tests for both paths. No SD changes occurred; Rust tests
  and ARM64 build for the fix remain unverified.


- **Installer validation 2026-11-23:** first pinned Music/Now Playing
  install at `197e134` ran 266 Music/Brick simulation unit tests:
  257 passed, 8 ignored, 1 failed in
  `music::tests::date_sort_orders_newest_first_with_missing_years_last_and_stable_album_ids`.
  The test opened an album then incorrectly called `playing_album()`
  before starting a track. Since Now Playing is independent of browsing,
  browsing must use `browsed_album()`; a second A starts the track.
  Test corrected in `c0b28c8`. The original installer correctly aborted
  before replacing the SD launcher. A fresh pinned installer is required;
  **Rust tests and ARM64 build for the corrected commit remain unverified**.
- **Next installer validation 2026-11-23:** on `59325ab` the complete
  simulation/unit test set passed (258 passed, 8 intentionally ignored);
  pause-menu integration tests passed (19/19) and power tests passed
  (22/22). The quick-menu suite passed 41/42, with one obsolete test
  `music_opens_from_the_menu_and_returns_to_the_same_carousel` expecting
  B to exit Music immediately. The new default Now Playing section
  requires B once to open Albums, B again to exit Music into the same
  HOME menu, and a third B to return to the shelf. Fixed the integration
  test in `4cf8ef4`; no application behavior changed. Installer again
  correctly stopped before the ARM64 build or SD changes. The fixed
  integration test and subsequent ARM64 build remain unverified.

- Prior Docker image: `slot-brick-psp-tools`, target volumes
  `slot-psp-sim` and `slot-psp-arm64`; Cargo home typically
  `/src/.docker-cache/music-cargo`. Installer scripts have the exact invocations.
- Tests: `cargo test --offline --locked -p slot --no-default-features
  --features brick,sim --lib --test quick_menu --test pause_menu --test power`.
  Do not omit power integration tests: the first sleep-fade installer revealed
  two obsolete immediate-backlight assertions. They were corrected in
  `f055987` while preserving power-button and wake checks.
  ARM64 build: `cargo build --offline --locked --profile device
  --target aarch64-unknown-linux-gnu -p slot --no-default-features --features brick`.
- SD launcher: `Slot/System/slot-native`; keep the `Slot/System/slot`
  wrapper intact. Verify SD identity, clean Git tree, commit SHA,
  tests, AArch64 binary, prior hash, backup and final installed hash.
  Do **not** format the SD or modify ROMs, saves, fonts or music as a side effect.
- Known regression: adding navigation fades initially broke
  `frontend::power_render_tests::idle_redraw_gate_preserves_input_effects_and_periodic_refresh`
  by forcing redraws before the first scene was presented. Fix `d4043b6`;
  retain the idle redraw gate. Cargo exit 101 is not enough to diagnose a
  failure—read actual Rust error/test output before changing code.

## 2026-11-23 Last bounded SDL udev bypass experiment

- User requested one final low-risk attempt, then a native Slot OS video
  player path if unsuccessful. Latest player log: after bz2 and OpenSSL
  libraries loaded, SDL_Init reported missing runtime `libudev.so.0`.
- Upstream SDL2 supports `SDL_JOYSTICK_DISABLE_UDEV=1` and
  `SDL_HIDAPI_JOYSTICK_DISABLE_UDEV=1` to bypass optional
  udev-based controller enumeration; Slot parent already polls Y/B/Menu
  for returning from external player. **This is a hypothesis**, not proof
  it will bypass all udev use in the bundled SDL.
- For existing installed binary, standalone installer
  `Install-Slot-Brick-Video-SDL-Udev-Last-Try-20261123.txt`
  replaces only `Slot/System/video-player/ffplay` with a tiny LF-only
  `/bin/sh` wrapper that exports both hints and `exec`s the original
  ARM64 ELF saved as `ffplay.native`. The installer validates AArch64
  ELF architecture and SHA256 of preserved binary; avoids Docker,
  downloads, rebuilds, SD formatting/firmware operations and new
  library patches. Supports `-Action Revert` to restore the exact
  ELF player, byte-for-byte. `sh -n` and dummy executable passthrough
  checks passed offline; **hardware SDL/audio/video not yet tested**.
- Corresponding environment settings are also set in the source
  `device_app.rs` for future native Slot builds, so a future full
  installer need not use a wrapper.
- On failure after one on-device run, *do not* continue layering
  NX Redux dependencies. Restore original player and investigate
  a minimal ffplay alternative built against the Slot OS runtime
  with SDL joystick/udev disabled at build time, or video decoding
  into Slot's existing EGL renderer with no external SDL process.

## 2026-11-23 ffplay SDL initialization failure — stop piecemeal dependency patches

- User has now installed both `libbz2.so.1.0` and private OpenSSL
  1.1 libraries successfully. In `video-player(2).log`, lines 1–17
  are accumulated older libbz2 and libssl failures; the **latest**
  lines 18–29 show repeated:
  `sh: setterm: not found` and
  `Could not initialize SDL - Failed loading libudev.so.0:
  libudev.so.0: cannot open shared object file`.
  The `(Did you set the DISPLAY variable?)` line is ffplay's generic
  SDL_Init error suffix; no evidence an X11 server is required.
- This is now **SDL runtime initialization**, not a missing ELF
  startup dependency, and the MP4 codec has **not been invoked**.
  `setterm` appears nonfatal relative to SDL's precise udev error,
  but should be audited too.
- The bundled NX Redux `ffplay` invokes SDL_Init with
  `SDL_INIT_VIDEO | SDL_INIT_AUDIO | SDL_INIT_TIMER |
  SDL_INIT_JOYSTICK`; SDL can dynamically load libudev
  (outside ELF DT_NEEDED). NX Redux's official installation docs
  state explicitly that it relies on libraries bundled in the
  TrimUI stock firmware. Slot OS is a minimal custom rootfs whose
  Dockerfile includes `libsystemd0` but not legacy `libudev.so.0`.
- **Do not issue another single-library installer.** Before further
  SD changes, inventory recursive ELF DT_NEEDED dependencies for
  ffplay and private libs *plus SDL's dlopen requirements*, identify
  the display backend/input library and ownership handoff, and
  produce one complete, version-pinned runtime built for Slot OS.
  If the NX binary cannot be made compatible, rebuild ffplay/SDL
  against Slot OS or choose a native video backend. Real device
  SDL initialization, video rendering, audio, and Y-return must be
  verified separately. It is not reasonable to call this feature
  installed or working yet. Restore prior Music installer if
  user wishes to opt out; music should otherwise remain unaffected.

## 2026-11-23 OpenSSL runtime package URL correction

- The first `Install-Slot-Brick-Video-OpenSSL11-Fix-20261123.txt`
  stopped before SD writes: `security.debian.org` responded HTTP 404
  for pinned `libssl1.1_1.1.1w-0+deb11u8_arm64.deb`.
  Debian's current security pool no longer reliably serves this
  legacy Bullseye security build.
- Corrected installer:
  `Install-Slot-Brick-Video-OpenSSL11-Fix-v2-20261123.txt`.
  It uses Debian's historical archived security pool at
  `https://archive.debian.org/debian-archive/debian-security/pool/updates/main/o/openssl/`
  and two indexed mirrors (`ftp.snt.utwente.nl/pub/linux/debian-archive/`,
  `debian.snt.utwente.nl/debian-archive/`) as sequential fallbacks.
  Checks exact Debian-published SHA256
  `0fba4e602e499c97c4b623bd56295115667c312ff583221a71b476dd6983a5e0`
  and 1396228-byte length before Docker extraction or any SD write.
- Fix replaces only download/fallback logic; original isolated-library
  placement, AArch64 ELF checks, license file, backups, and rollback
  are unchanged. Package download success remains unverified on the
  user's Windows host until rerun.
- Future full installer: stage all FFplay dependencies and verify
  the complete ELF dependency tree before touching SD, so users don't
  have to install missing libraries one at a time.

## 2026-11-23 ffplay runtime missing OpenSSL 1.1 (second device log)

- User installed successful `libbz2.so.1.0` patch. New
  `video-player(1).log` progressed from prior libbz2 failures (old lines
  1-8) to repeated `libssl.so.1.1: cannot open shared object file`
  (new lines 9-17). The video/SDL decoder has still **not run**.
- Official Debian bullseye-security `libssl1.1`
  `1.1.1w-0+deb11u8` ARM64 package supplies both
  `libssl.so.1.1` and `libcrypto.so.1.1`; pinned SHA256
  `0fba4e602e499c97c4b623bd56295115667c312ff583221a71b476dd6983a5e0`
  from Debian's arm64 package download page.
- Small follow-up patch:
  `Install-Slot-Brick-Video-OpenSSL11-Fix-20261123.txt`
  downloads/extracts that package under disposable Docker, verifies
  AArch64 ELF headers, installs exactly those two shared objects and
  the copyright text into `Slot/System/video-player/lib`, with
  hash-checked backups and rollback. It preserves Slot OS's newer
  system libraries and user media. The shell script strips Windows
  CR but **retains LF line breaks** (an earlier fix that flattened
  them to `; ` broke POSIX `for ... do` syntax).
- This is still an on-device experiment; another loader/runtime/display
  error is possible, and video playback is not yet verified.
- Next full installer must incorporate this dependency as well as bz2
  and run a full ARM64 ELF needed-library closure check before
  user installation.

## 2026-11-23 libbz2 extraction script CRLF correction

- The first libbz2-only installer downloaded and verified Debian's ARM64
  package, then stopped before writing to SD. Docker's `sh -ec` printed
  `set: Illegal option -` on its first line. The installer had forwarded
  a Windows PowerShell CRLF here-string verbatim as a shell script; the
  trailing carriage return after `set -eu` was treated as an invalid
  POSIX option by dash.
- The corrected `Install-Slot-Brick-Video-libBz2-Fix-v2-20261123.txt`
  removes carriage returns, joins shell statements on one line, and
  retains the same pinned .deb SHA256, ELF check, private-lib-only write,
  verified backups, rollback, and cached download. An offline `sh -n`
  check of the resulting POSIX command succeeded. This does **not**
  establish that Docker/ffplay playback works yet on the user's host.

## 2026-11-23 ffplay missing libbz2 on Slot OS (user device log)

- User installed the experimental video handoff and pressed Y, but video
  never appeared. `Slot/Logs/video-player.log` contained repeated
  `ffplay: error while loading shared libraries: libbz2.so.1.0:
  cannot open shared object file: No such file or directory`.
  This **proves the external process was invoked**, not that EGL/SDL,
  decoding, sound or return-to-Music have worked yet.
- Root cause: previous installer staged the NX Redux ARM64 `ffplay`
  and several `lib*.so` dependencies, but did not include
  `libbz2.so.1.0` from the stock-firmware userspace.
- For the already-installed card, use the **small library-only**
  `Install-Slot-Brick-Video-libBz2-Fix-20261123.txt` installer.
  It downloads official Debian bookworm ARM64
  `libbz2-1.0_1.0.8-5+b1_arm64.deb` from
  `https://deb.debian.org/debian/pool/main/b/bzip2/`
  with SHA-256
  `d3a96ece03326498b39ff093a76800dfcbcb1d4049d6ae6e9f6fa1aa7a590ad6`,
  extracts the dereferenced shared object in existing Docker, and
  installs **only**
  `Slot/System/video-player/lib/libbz2.so.1.0` plus its attribution
  file. Existing files are backed up and verified; SD partition,
  firmware, Slot launcher, games, saves, music, and other libraries
  remain unchanged.
- **A second missing dependency is possible**: this fixes only the
  loader's first confirmed error. On-device re-test with a local
  MP4 is still required. If failure continues, use the newest
  `Slot/Logs/video-player.log` and `Slot/Logs/slot.log` to diagnose.
  Future full video installers should package this verified runtime
  dependency automatically and check ELF dependency closure.

## 2026-11-23 Experimental Y video-player handoff (NOT hardware validated)

- This is an experimental external-player path on the existing
  `feature/psp-replaces-ps1` branch, **not** a native video decoder.
- On Music > Now Playing only, pressing Y when the playing MP3/M4A has
  a nonempty same-stem `.mp4`/`.m4v` sets a one-shot video request.
  If locked, App blocks Y as before. The request does not change the
  normal Album/Artist browser's Y-to-play mapping.
- Music's decoder and song identity remain in RAM. The handoff rewinds
  buffered audio to audible time, records starting seconds, pauses the
  track and explicitly shuts down Slot's ALSA PCM output thread.
  Music resumes its previous playing/paused state on return, seeking
  to NX's patched ffplay `-posfile` position if available. Song switching,
  tabs, repeat, album cache, SD settings remain untouched.
- The device loop temporarily unbinds its EGL context, spawns
  `Slot/System/video-player/ffplay` as an external process with
  `-fs -autoexit -posfile` and the same-stem local file, polling
  physical controls. Y, B, Menu or Power sends SIGTERM to the child
  (600ms to write final position, then SIGKILL fallback); natural
  video completion also returns. Rebind EGL and reopen ALSA, force
  a new UI frame. If EGL cannot be rebound, exit with existing code 66
  so Slot OS restarts safely. The first 300ms are debounced.
  Logs: `Slot/Logs/slot.log`, `Slot/Logs/video-player.log`.
- Packaging path: stage official **NX Redux v2.5.0 Brick Pro** release
  `NXRedux-v2.5.0-brickpro.zip` (pinned SHA256
  `e3ada6dc277762000139e648b64ece0e7e73e415e68e22f045054d84f5851879`),
  extract only the prebuilt patched ARM64 `ffplay` plus its needed
  compatibility libraries into `Slot/System/video-player`. Do not
  install NX Redux, bootloader, firmware, MinUI, or other tools.
- **Important uncertainty:** NX's release is for Brick Pro stock firmware;
  Slot OS is a separate minimal Linux userspace and may lack some
  shared libraries or a compatible SDL2 display backend. Tests and an
  offline ARM64 build of Slot can pass yet the child might exit
  immediately without video, or fail to reacquire display. Treat this
  as a guarded device experiment, not production ready. Do not
  claim compatibility without on-device video + return verification.
- Ship any installer as a downloadable `.txt`, including one complete
  paste-ready PowerShell command for renaming to `.ps1` and executing.
  Preserve the previous regression tests, ARM64 binary check and
  verified backup/rollback. Avoid any SD formatting/repartitioning.

## 2026-11-23 Video badge installer compile correction (still unverified)

- The installer pinned to `31be697` stopped safely at `regression-tests`
  with Rust parsing errors (E0308-like precedence symptoms):
  an `as f32` cast directly before `<` in two
  `youtube_badge_is_small_inside_the_top_left_of_the_cover` assertions
  was interpreted as generic arguments. Parenthesized both casts.
- Prior installer did download and PNG-validate all eight assets; it did
  **not** reach ARM64 build or change the SD. After committing this note,
  generate a fresh installer pinned to the resulting exact branch HEAD,
  with the previous test/build/backup/SHA safeguards and the standard
  copy-paste rename from `.txt` to `.ps1`.
- The existing `embedded_cover` and `SETTINGS_ROW_H` dead-code
  warnings were not compilation blockers and were left untouched.

## 2026-11-23 Music video-available cover badge (build/device verification pending)

- New first-stage **visual indicator only** on native Music Now Playing:
  a small 22px full-colour UXWing YouTube Color badge
  (https://uxwing.com/youtube-color-icon/) overlays the upper-left corner
  of the current track's 250px album art (6px inset), keeping album art,
  metadata and playback controls unmoved. No badge in the absence of video.
- Current library indexes audio only; **no video player, remote YouTube
  lookup or streaming support exists** in this branch. For this first
  stage, "video available" means a nonempty sidecar video next to the
  currently playing MP3/M4A with the *same filename stem*, ending
  `.mp4` or `.m4v`, e.g.
  `Slot/Music/Artist/Track.mp3` + `Slot/Music/Artist/Track.mp4`.
  This is availability signalling, **not** a guarantee that video will
  play until a player/codec pipeline is implemented. Future video
  integration may replace the detection source.
- MusicView caches per-track video presence: no repeated stat() on
  marquee frames. Only Now Playing requests the badge. Cache and GPU
  icon allocation are bounded; icon decoded and uploaded once.
- Installer must download official UXWing Colour YouTube PNG from
  `https://uxwing.com/wp-content/themes/uxwing/download/brands-and-social-media/youtube-color-icon.png`,
  stage as `Slot/System/music-icons/youtube.png`, PNG-validate,
  back up prior file if any, checksum and install only after full
  regression tests and ARM64 success; preserve Games, Saves, Music and
  Config. Always include the `.txt` to `.ps1` rename in the
  supplied pasteable command.
- Tests added for a matching non-empty local companion, different
  track isolation, cache rekeying, logo RGB preservation, icon size and
  top-left containment. No Docker or actual Brick Pro testing yet.

## 2026-11-23 User-selected lock icon replacement (build/device verification pending)

- The user chose **UXWing Padlock** (https://uxwing.com/padlock-icon/)
  for the locked state and **UXWing Unlocked**
  (https://uxwing.com/unlocked-icon/) for the brief unlocked state,
  replacing Lock Line and Lock Open Line.
- Only the two offline asset download URLs need to change: keep
  `Slot/System/music-icons/lock.png` and `unlock.png`, HUD-centred 18px.
  The native lock-only PNG decoder now preserves their **original colours**
  (necessary to retain the yellow body/keyhole contrast), unlike earlier
  black-tinted line artwork. No changes to lock state,
  600ms hold, input handling, rendering schedule, or other Music icons.
- Installer **must** use the two newly selected UXWing image URLs,
  validate PNG signatures/dimensions, back up and checksum both assets,
  pin the exact current feature-branch commit, and ship as `.txt` with a
  command that renames it to `.ps1` before execution.
- No successful build or Brick Pro device installation was established
  by this documentation/installer-only change.

## 2026-11-23 Brief Music unlock confirmation (build/device verification pending)

- User explicitly prefers **no animation**. When Music unlocks, swap the
  centred closed 18px UXWing Padlock glyph immediately to the matching
  18px UXWing Unlocked icon (https://uxwing.com/unlocked-icon/).
  Hold open glyph for ~600ms, then remove it from HUD. No fade or movement.
  Locking again cancels the open indication instantly and restores the
  closed lock. Unlock works immediately regardless of this visual feedback.
- The closed icon is `Slot/System/music-icons/lock.png` and the open icon
  is `Slot/System/music-icons/unlock.png`. Both retain their original
  transparent PNG colours in one-time cached GPU textures. The MusicView owns a short render deadline; the
  shared Shelf/game status positions and audio/input behaviour stay as-is.
  Ensure any supplied guarded installer downloads, validates, backs up,
  and SHA-verifies the additional `unlock.png` asset before installing.
- New regression tests cover no icon flash on initial unlocked Music,
  closed-to-open transition, 600ms expiry, and immediate cancellation
  when relocked. Docker/ARM64/Brick Pro verification remains pending.

## 2026-11-23 Music HUD and playback glyph update (build/device verification pending)

- A guarded Windows installer caught a `slot-ui` unit regression:
  `music_lock_sits_immediately_left_of_battery_without_moving_the_battery`
  failed (33/34 passed) because raster alignment consumed half a pixel
  of the required 10px lock/battery gap. The installer correctly left SD
  untouched. A follow-up centring request supersedes the adjacent-battery
  placement entirely: Music Lock now uses the **UXWing Padlock** icon
  (https://uxwing.com/padlock-icon/) at 18px, in the centre of the
  shared top HUD. Clock, battery, Wi-Fi and update positions must remain
  identical to the Shelf. A new test checks centre placement and verifies
  the rest of the entire status HUD is byte-for-byte identical at scales
  1.0 and 1.6 for both battery charge states.
- When Music is playing, place the UXWing Play icon
  (https://uxwing.com/play-icon/) left of the elapsed timestamp;
  when paused, show UXWing Pause
  (https://uxwing.com/pause-icon/) instead. Both use a single black 20px
  texture, at x=24/y=434; elapsed time starts at x=54, repeat/end time
  remain unchanged. Remove the separate "paused" and "playing" status
  text beneath X-of-Y but retain loading, finished and genuine error
  messages. Do not change control mappings or actual playback.
- Runtime offline icon paths: `Slot/System/music-icons/lock.png`
  (replace old lock graphic), `play.png`, `pause.png`.
  Existing Repeat Off/All/One assets remain as before. Decode once,
  tint black, reuse GPU textures; do not bundle Flutter or rework HUD
  on game/shelf screens. The Windows installer must stage and validate the
  three UXWing PNG assets before running tests/build and copy them only
  after a successful pinned ARM64 build, with backups and SHA verification.
  No new source commits alone prove a successful Rust build or device
  installation. The old installer is pinned to a previous SHA.
- Previous attempt fixed `E0308` in `music_cover.rs`; subsequent
  cartridge-ui-tests reached Rust tests and failed only the old lock
  placement assertion. New full test suite and ARM64/device verification
  are still pending on the user's Windows Docker toolchain.

## Next work

1. Verify new **Now Playing / Albums / Artists / Settings** navigation and
   shared top HUD first: Music defaults to Now Playing, B returns to the
   browser without stopping music, L1/R1 cycle all four tabs, A pauses/resumes,
   D-pad Left/Right change songs without seeking, and the repeat badge sits
   at bottom-right without overlapping time/progress. There should be no
   button tooltips such as "x repeat" or "x list". Confirm background music
   keeps playing while browsing another album, and the clock/battery/Wi-Fi/
   update layout matches Shelf. Test track auto-advance
   and repeat while browsing (including album identity and selection), menu
   entry/exit, and empty libraries.
   Confirm latest **warm-ivory Music theme**, first-frame boot fade,
   sleep/wake and silent-shutdown changes on-device; fix reported defects first.
2. Verify the redesigned **original iPod Classic-style song list**
   on the 640x480 Brick Pro: full-width rows, quiet top header, seven-row
   scrolling, long title truncation, rose selection and slim scrollbar.
   Also check the existing GPU Cover Flow perspective, rack overlap,
   frame rate and artwork (without reflections). Keep album list via **X**.
3. Validate **Music Settings, repeat, and the new dim timer** on device:
   Settings via L1/R1 (not Start), B inert at Settings root, clean
   two-column menu and uninterrupted playback; X cycles off/all/one
   on Now Playing; 70% default
   volume cap; album sorts and saved preferences; Dim after Off/30sec/1min/2min
   with default 1min, plus Dim brightness 1-9 (default 1) and physical
   display remaining on. Test both the lowest and highest steps and verify
   the dim level never exceeds normal brightness. Confirm any button restores
   normal brightness, settings screen never dims, and volume and games are
   unaffected. No parental-control PIN is present.
   Then test **Power from Now Playing** (while playing and paused):
   the normal fade to full suspend, no music during actual suspend,
   same Music screen/album/position on wake, automatic resume only if
   previously playing, clean ALSA output and correct brightness. Test waking
   early during the 3.3-second pre-suspend delay and subsequent game/shelf
   sleep/wake regressions.
4. Check Now Playing's **stronger diagonal artwork-adaptive
   background** on the Brick Pro: hue strongest at top-left, fading smoothly
   to subtle ivory at bottom-right. Check readability (including dusty-rose
   text), no tint when artwork is monochrome/missing, no incorrect colour
   when swapping albums, and no image-load delay or frame-rate regression.
5. PSP replacement work can resume separately when requested.

**For a new conversation:** ask the assistant to read this file from GitHub,
check the live `feature/psp-replaces-ps1` HEAD and the relevant source files,
then continue from the user's latest request. Update this file after major
decisions or verified milestones; don't copy an entire chat transcript into it.
