# Android Original UI port (TrimUI Brick Pro)

This is a staged, GBA-first visual port from [slot-for-android](https://github.com/Baggio94/slot-for-android), using [Slot-Brick](https://github.com/LordNordyx/Slot-Brick) as the hardware-compatible runtime.

## Implemented in the first commit

- Display Android's **Original GBA** shelf ground: RGB `196, 198, 228` (lavender).
- Render the **actual Android GBA slot back/front PNGs** at 720×58, downsampled from their 2160×174 masters.
- Keep the slot layers in the existing insertion/ejection draw order, behind and in front of the cartridge.
- Select Android's artwork **only for GBA**; GB/GBC and the other systems retain the Brick slot renderer.
- Keep the existing Brick Pro boot, emulation, controls, saves, power, LEDs and sleep code.

The GBA appearance in this first phase intentionally overrides the normal wallpaper on the GBA shelf. A theme chooser and custom-wallpaper override belong in a later phase.

## Original artwork attribution

The two files in `assets/android-original/` are copied from `Baggio94/slot-for-android`:
- `assets/slot_back_gba.png`
- `assets/slot_front_gba.png`

The Android port is distributed under GNU GPL-3.0, and credits the original Slot authors and artwork contributors. Preserve its license and relevant attribution in redistribution. The theme colours are from `crates/slot-ui/src/menu_style.rs` in that project.

## Verification before SD-card installation

This branch was edited through GitHub without running a cross-compile or booting a Brick Pro. Before installing:

1. Run `cargo test --workspace --features brick` (or the build's documented equivalent).
2. Cross-compile with the established Brick Pro toolchain.
3. Check the GBA shelf, texture alignment, cartridge insertion/ejection, and paused/resumed game.
4. Verify GB and GBC artwork is unaffected.
5. Back up the currently working SD card and executable before deployment.

Do not deploy merely because the code is committed.

## Next phases

- Bring over the real GB and GBC console slot artwork and corresponding green/yellow Original shelf grounds.
- Port Android's top status row, dynamic ink, menu palette, and theme selector.
- Refine carousel movement and cartridge insertion to match Android, adapted to physical buttons.
- Hide unused system shelves without deleting their emulator code.

## Phase 2 — GB / GBC / NES / SNES Original shelves

The second port extends the proven GBA approach to four more systems, using
the original Android console bar PNGs and matching shelf colours:

| Console | Shelf RGB | Artwork |
|---|---|---|
| GB | 202, 211, 168 | assets/slot_{back,front}_gb.png |
| GBC | 243, 227, 160 | assets/slot_{back,front}_gbc.png |
| NES | 228, 205, 203 | assets/home/slot_{back,front}_nes.png |
| SNES | 205, 211, 222 | assets/home/slot_{back,front}_snes.png |

The eight new images are copied from Baggio94/slot-for-android to
`assets/android-original/` and retain their original filenames.
GBA's working artwork and palette remain unchanged. This does not yet port
Android's separate moving NES/SNES slot flaps, or its status bar and menus.

Test the GBA regression and each new console's shelf and insertion/ejection
before considering a release.

## Personal GBA shelf colour

For the grey TrimUI Brick Pro, GBA now deliberately reuses the **NES Original
shelf background** RGB `228, 205, 203` (`#E4CDCB`) rather than Android's
lavender GBA shelf. The **GBA cartridge shape, label and original GBA slot
front/back PNGs remain unchanged**. GB, GBC, NES and SNES each retain their
own existing Original shelf backgrounds and console slot artwork.

This is a one-line palette override and can be reverted independently.

## Visual refinement: clean shelves and neutral GBA slot

- The GBA shelf retains the NES Original rose-grey background (`#E4CDCB`).
- Its rear/front slot bars now use Android's actual light GBA artwork,
  `assets/slot_{back,front}_gba_light.png`, preserving GBA cartridge geometry
  but using light neutral plastic rather than indigo.
- The independent game-title card beneath the cartridge and the transient
  console/platform caption above the slot are no longer rendered on the shelf.
  This does not remove cartridge-label lettering, menu entries or system functionality.
- The Android minimalist white `slot.` boot logo is included as a source asset,
  but installing a frontend executable alone **does not** change the bootloader
  splash. On Brick Pro stock firmware the logo is in an internal boot partition,
  and the repository's stock Brick Logo app is not verified for Brick Pro. Do
  not write that partition without model-specific verification and a backup.
  The optional SD-card-based Slot OS boot image must be verified independently.

Upstream asset paths:
- `assets/slot_back_gba_light.png`
- `assets/slot_front_gba_light.png`
- `android/app/src/main/assets/slot-bootlogo-kpa-960x640.png`

The light GBA slot artwork is desaturated on decode to prevent any
remaining blue/lavender cast. RGB channels are equalised after scaling
while alpha/transparency and fine luminance variation are preserved.


## Android-style shelf status row

- The clock uses black ink at (24, 12), top left.
- The battery gauge, percentage, and charging bolt use black ink at the
  top-right margin, respecting charge and battery state.
- The Wi-Fi icon is only drawn when `brick_wifi::online()` reports a connection.
  Disabled Wi-Fi and enabled-but-disconnected Wi-Fi leave no Wi-Fi icon.
- When available, the update-available icon remains beside the connected Wi-Fi
  indicator, with top-right alignment and the same black ink.
- The old bottom footer (including play-time note) is omitted on the shelf
  to keep the cartridge view clean.
- In-game HUD text, menus, fonts, and Wi-Fi settings screens are unchanged.

Source reference: Baggio94/slot-for-android `crates/slot-ui/src/footer.rs`,
`draw_status_clock` and `STATUS_TOP_Y`. The new Brick version retains its
existing online-state detection and charging behaviour.
