# Slot-Brick — ongoing development context

> Small handoff for future ChatGPT/AI coding sessions. **Read this first**, then verify the
> current branch and code in GitHub. This records user decisions, not a substitute for tests.
> Keep it brief and update it when a milestone changes. Last reviewed: 2026-11-22.

## Where we work

- Repository: `pkimcrombies/Slot-Brick` (Rust Slot OS port).
- Active development branch: **`feature/psp-replaces-ps1`**. Do not modify `main`
  unless explicitly asked. At this handoff the branch HEAD is `3daabadea1b111265d76ca98242a403faff8a600`;
  always fetch the actual HEAD rather than trusting this SHA.
- Device: **TrimUI Brick Pro**, running native Slot OS from a 32 GB SD card.
  Primary working systems: GBA, GB, GBC, NES, SNES. PSP in the old PS1 slot
  is an **experimental, separate workstream**, not the current priority.
- See `CLAUDE.md` for repository conventions and `README.md` for build/packaging.
  The repo and its commit history are authoritative; this file is a conversation handoff.

## User preferences / design constraints

- Keep everything **native Slot (Rust)**. Do not introduce Trimpod, Flutter,
  ClassiPod itself, or a separate music launcher.
- Minimal black backgrounds, Slot's existing typography, white text, subtle
  rose/pink selection, no unnecessary chrome. Cartridge/game animations must stay intact.
- Prefer short, smooth fades. Avoid black flashes, extra loading screens and
  needless redraws/CPU use.
- For each build-worthy change, provide the existing **one-command PowerShell
  installer** workflow: exact Git commit, regression tests, ARM64 build, backup,
  replace and SHA-256 verification. Distribute standalone PowerShell as `.txt`
  for renaming to `.ps1`. Never silently overwrite a working installation.
- User primarily works on Windows. Previous checkout:
  `C:\Users\pk\Downloads\slot\Slot-Brick-PSP-Feature`; SD usually `F:\`.
  **Treat both as defaults, not guarantees; verify before copying.**
- Don't claim code was compiled or device-tested unless it actually was.
  The user prefers concrete changes and direct answers over speculative mockups.

## Current features / important locations

- Native Music: `crates/slot/src/music.rs`, `music_view.rs`,
  `music_cover.rs`, `music_library.rs` and `music_m4a.rs`.
  Embedded MP3/M4A art loads asynchronously; album rows have 40px covers.
  Album detail has large artwork on the right fading toward black on the left.
  Korean glyph fallback via `crates/slot/src/fonts.rs` and
  `crates/slot-ui/src/text.rs`; SD font path:
  `Slot/System/Fonts/.SlotHangul.ttf`. It is optional.
- Now Playing was redesigned (commit `7515789`): **214px artwork on left**,
  track/artist/album and position on right, progress/time at bottom. Keep
  physical playback controls. Device feedback on this redesign is pending.
- Fades are in `crates/slot/src/frontend.rs` (`NavFade`, `PowerFade`);
  sleep handling also uses `device_app.rs`. Boot 440ms, menu 220ms,
  sleep 300ms, wake 360ms, shutdown 210ms. Returning from game ejection
  **must not** add a black navigation fade; fix `85b0331` was confirmed smooth.
- Shutdown (commit `3daabad`): **no “Saving and Shutting Down” screen** on
  Brick; preserve black fade, plus 70ms GPIO rumble on deliberate power-off
  if Rumble is enabled, *after saves are settled*. Auto/battery shutdown
  stays silent; exit-to-stock keeps its separate behaviour.
  User device verification of this latest change is pending.

## Test/build/install guardrails

- Prior Docker image: `slot-brick-psp-tools`, target volumes
  `slot-psp-sim` and `slot-psp-arm64`; Cargo home typically
  `/src/.docker-cache/music-cargo`. Installer scripts have the exact invocations.
- Tests: `cargo test --offline --locked -p slot --no-default-features
  --features brick,sim --lib --test quick_menu --test pause_menu`.
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

1. Confirm latest **Now Playing**, sleep/wake and silent-shutdown changes
   on the actual device; fix reported defects first.
2. Then build **native Cover Flow** inspired by
   `adeeteya/Classipod`: centered selected album, angled neighbouring
   covers, subtle reflection, album/artist underneath, smooth button-driven
   horizontal movement. Preserve the current list as an alternative.
   ClassiPod is *visual reference only*, not code to port wholesale.
3. PSP replacement work can resume separately when requested.

**For a new conversation:** ask the assistant to read this file from GitHub,
check the live `feature/psp-replaces-ps1` HEAD and the relevant source files,
then continue from the user's latest request. Update this file after major
decisions or verified milestones; don't copy an entire chat transcript into it.
