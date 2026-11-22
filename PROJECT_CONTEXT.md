# Slot-Brick — ongoing development context

> Small handoff for future ChatGPT/AI coding sessions. **Read this first**, then verify the
> current branch and code in GitHub. This records user decisions, not a substitute for tests.
> Keep it brief and update it when a milestone changes. Last reviewed: 2026-11-23. Navigation/HUD changes committed through `87b293c` (not yet built or tested on device).

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
  for renaming to `.ps1`. Never silently overwrite a working installation.
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
  standalone **Now Playing** section, with Albums and Artists alongside it.
  An empty Now Playing screen offers B/R1 to Albums. Starting a song switches
  to Now Playing, but B returns to the existing browser/album detail without
  stopping or discarding queued music; from Albums root L1 returns to Now
  Playing, and Albums R1 goes to Artists. On Now Playing, A still pauses/
  resumes, Left/Right still seek, L1/R1 still skip, and X cycles repeat.
  Music::Track now owns its album ID and song index independently of the
  browser location: audio decoding, repeat, metadata, album cover and track
  count all use the playing album even when the user browses another album.
  Automatically advancing tracks never takes over the current browsing
  screen or selection. B on the browser's root or HOME/QuickMenu still exits
  Music entirely and stops its music; playback outside the Music phase has
  NOT been introduced.
  **Shared shelf HUD in Music:** `app.rs` uses the same
  `slot_ui::draw_status_bar` for Music and Shelf. Clock stays at the top-left,
  battery/charging, connected Wi-Fi and update symbol at top-right with the
  same black icons. Album-detail title/artist were shifted down to clear it.
  Do not add a second HUD implementation or put the status bar behind the
  ivory/gradient background. Added unit regression coverage for navigating
  to/from Now Playing without stopping music, browsing another album while
  preserving the original song, and the shared clock position.
  **Music Settings + repeat (Nov 2026, pending Rust/device validation):**
  press **Start** from any Music screen to open a full-width classic settings
  page; Up/Down select rows, Left/Right/A change values, B/Start closes.
  Rows: Music-only digital volume cap (default 70%, steps of 10 to 0-100),
  default album browse view (Cover Flow/list), album sort (A-Z/newest tagged
  release date first), Repeat (off/all/one), and **Dim after** (Off/30 seconds/
  1 minute/2 minutes; default 1 minute), and **Dim brightness**
  (physical backlight step 1-9, default 1). **X** on Now Playing cycles
  repeat; a native double-arrow indicator with off/all/1 state appears at the
  top-right. Off plays the current album once; All wraps at the last track
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
  Existing physical play/pause, skip and seek controls are preserved.
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
  song/list selections. `X` toggles original list, `L1/R1`
  Albums/Artists, `A` opens,
  `Y` plays, `B` leaves root. Artists/tracks unchanged.
  Renderer: `slot-gfx/src/draw.rs` and `shaders.rs`; Music layout:
  `music_view.rs`. The `frontend.rs` animation redraw gate must stay
  bounded. First device installer stopped safely during Rust compilation:
  adding `Draw::CoverFlow` required `slot-ui/src/peel.rs::fade_from` to
  include the new sprite variant in its alpha match. The follow-up commit
  adds this exhaustiveness fix without changing layout or shader behavior.
  Hardware and ARM64 compile verification are still pending.
- Now Playing was redesigned (commit `7515789`): **214px artwork on left**,
  track/artist/album and position on right, progress/time at bottom. Keep
  physical playback controls. Device feedback on this redesign is pending.
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

## Next work

1. Verify new **Now Playing / Albums / Artists** navigation and shared top
   HUD first: Music defaults to Now Playing, B returns to the browser without
   stopping music, Albums root L1 returns to Now Playing, A pauses/resumes,
   background track keeps playing when browsing a different album, and the
   clock/battery/Wi-Fi/update layout matches Shelf. Test track auto-advance
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
   Start/B navigation; X cycles off/all/one on Now Playing; 70% default
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
