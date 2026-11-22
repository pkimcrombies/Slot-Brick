# Slot-Brick — ongoing development context

> Small handoff for future ChatGPT/AI coding sessions. **Read this first**, then verify the
> current branch and code in GitHub. This records user decisions, not a substitute for tests.
> Keep it brief and update it when a milestone changes. Last reviewed: 2026-11-23. Latest Music control changes (L1/R1 sections, D-pad Left/Right tracks, bottom-right repeat) are committed but not yet rebuilt or device-tested.

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
  previous/next track, and X cycles repeat. With **no song loaded**, all
  D-pad directions and A leave the empty Now Playing screen unchanged;
  L1/R1 alone switch sections, while B intentionally opens Albums root.
  D-pad Up/Down have no playback action.
  User-requested
  seeking has been removed from the player controls; the low-level seek_by(0)
  remains internally for suspend/resume accuracy. Button hints such as
  "x repeat" and "x list" are intentionally absent on Music screens.
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
  L1/R1 switch the three Music sections, X changes repeat mode.
  No seek gestures. B opens the playing album's song list without
  stopping playback; HOME/Menu is the only exit from Music.
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
  The former `x list`/`x covers` hints have been removed.
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
