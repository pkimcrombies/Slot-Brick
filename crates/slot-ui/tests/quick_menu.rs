use slot_store::{parse_stamp, FF_SPEEDS};
use slot_ui::{
    date_time_text, quick_caret_face, quick_label_face, quick_value_face, ClockPicker, QuickRow,
    QuickValue, UndoFace, MENU_PAD,
};

fn inked(f: &UndoFace, x: u32, y: u32) -> bool {
    f.rgba[((y * f.w + x) * 4 + 3) as usize] > 128
}

fn ink_columns(f: &UndoFace) -> (u32, u32) {
    let cols: Vec<u32> = (0..f.w)
        .filter(|&x| (0..f.h).any(|y| inked(f, x, y)))
        .collect();
    (
        *cols.first().expect("an empty face"),
        *cols.last().expect("an empty face"),
    )
}

#[test]
fn menu_labels_and_values_use_the_smaller_lowercase_font() {
    let font = slot_ui::text::label_font().unwrap();
    let px = slot_ui::QUICK_LIST_PX;
    let tracking = (px * 0.1).round();
    let label = slot_ui::text::line_width(font, "sort by", px, tracking).ceil() as u32;
    let value = slot_ui::text::line_width(font, "by console", px, tracking).ceil() as u32;
    assert_eq!(quick_label_face(QuickRow::LibrarySort).w, label + 2 * MENU_PAD);
    assert_eq!(quick_value_face("By Console", true).w, value + 2 * MENU_PAD);
}

#[test]
fn music_and_quick_menu_measure_labels_with_the_same_font_and_spacing() {
    let quick = quick_label_face(QuickRow::LibrarySort);
    let (_, [width, _]) = slot_ui::menu_text_face(
        "Sort By",
        slot_ui::QUICK_LIST_PX as u32,
        300,
        [1.0, 1.0],
        slot_ui::MENU_TEXT_INK,
    );
    assert_eq!(width - 4, quick.w - 2 * MENU_PAD);
}

#[test]
fn the_type_keeps_the_menu_padding_around_its_ink() {
    for f in [
        quick_label_face(QuickRow::FastForwardSound),
        quick_label_face(QuickRow::Rumble),
        quick_value_face("Off", false),
        quick_value_face(QuickValue::Speed6.text(), true),
        quick_value_face("SEP 15 16:35", true),
    ] {
        let (first, last) = ink_columns(&f);
        let right = f.w - 1 - MENU_PAD;
        assert!(
            (MENU_PAD..MENU_PAD + 4).contains(&first),
            "ink starts at {first} of {}",
            f.w
        );
        assert!(
            // A lowercase f can extend one pixel past its advance width.
            (right - 4..=right + 1).contains(&last),
            "ink ends at {last} of {}",
            f.w
        );
    }
}

#[test]
fn a_long_label_is_set_as_large_as_a_short_one() {
    let tall = |f: &UndoFace| {
        (0..f.h)
            .filter(|&y| (0..f.w).any(|x| inked(f, x, y)))
            .count()
    };
    let long = tall(&quick_label_face(QuickRow::FastForwardSound));
    let short = tall(&quick_label_face(QuickRow::Rumble));
    assert!(long + 1 >= short, "{long} rows of ink against {short}");
}

#[test]
fn the_fast_forward_row_offers_the_four_ceilings_the_card_can_hold() {
    assert_eq!(FF_SPEEDS, [2, 3, 4, 6]);
    assert_eq!(
        FF_SPEEDS.map(QuickValue::speed),
        [
            Some(QuickValue::Speed2),
            Some(QuickValue::Speed3),
            Some(QuickValue::Speed4),
            Some(QuickValue::Speed6),
        ]
    );
    for other in [1, 5, 7, 9, 16, 28, 255] {
        assert_eq!(
            QuickValue::speed(other),
            None,
            "{other}x is not a value the row has"
        );
    }
}

#[test]
fn the_rows_run_in_the_order_the_user_chose() {
    #[cfg(not(feature = "brick"))]
    assert_eq!(QuickRow::MAIN.map(QuickRow::label), ["Emulation", "Gameplay", "Library", "System"]);
    #[cfg(not(feature = "brick"))]
    assert_eq!(
        QuickRow::SYSTEM.map(QuickRow::label),
        ["Date & Time", "Font", "Rumble", "System Sounds", "Low Battery Shutdown", "Reset Settings", "About"]
    );
    #[cfg(feature = "brick")]
    assert_eq!(QuickRow::MAIN.map(QuickRow::label), ["Music", "Settings"]);
    #[cfg(feature = "brick")]
    assert_eq!(QuickRow::SETTINGS.map(QuickRow::label), ["Emulation", "Gameplay", "Library", "Online", "System"]);
    #[cfg(feature = "brick")]
    assert_eq!(
        QuickRow::SYSTEM.map(QuickRow::label),
        [
            "LEDs",
            "Clock",
            "Font",
            "Rumble",
            "System Sounds",
            "Low Battery Shutdown",
            "Check for Updates",
            "Reset Settings",
            "About"
        ]
    );
    #[cfg(feature = "brick")]
    assert_eq!(QuickRow::ONLINE.map(QuickRow::label), ["Wi-Fi", "Cart Art", "RetroAchievements", "Update"]);
    #[cfg(feature = "brick")]
    assert_eq!(QuickRow::CLOCK.map(QuickRow::label), ["Date & Time", "Auto Clock"]);
    assert_eq!(QuickRow::SCREEN.map(QuickRow::label), ["Game Boy / GBC", "Game Boy Advance", "NES", "SNES", "PlayStation"]);
    assert_eq!(
        QuickRow::SCREEN_GB.map(QuickRow::label),
        ["Shader", "Color Correction", "GB Palettes", "GB Overlay", "GBC Overlay"]
    );
    assert_eq!(QuickRow::SCREEN_GBA.map(QuickRow::label), ["Shader", "Color Correction", "Overlay"]);
    assert_eq!(QuickRow::SCREEN_NES.map(QuickRow::label), ["Shader", "Overlay"]);
    assert_eq!(QuickRow::SCREEN_SNES.map(QuickRow::label), ["Shader", "Overlay"]);
    assert_eq!(QuickRow::SCREEN_PS1.map(QuickRow::label), ["Shader", "Dithering", "Overlay"]);
    // Each Overlay row picks the overlay for its own system.
    use slot_store::Platform;
    let systems: Vec<_> = QuickRow::ALL.into_iter().filter_map(QuickRow::overlay_of).collect();
    assert_eq!(systems, [Platform::Gb, Platform::Gbc, Platform::Gba, Platform::Nes, Platform::Snes, Platform::Ps1]);
    assert_eq!(
        QuickRow::GAME.map(QuickRow::label),
        ["Fast Forward", "Fast Forward Sound", "Rewind", "Turbo Buttons", "Swap A/B", "Saving"]
    );
    assert_eq!(QuickRow::SAVING.map(QuickRow::label), ["Auto Save", "Save on Eject"]);
    // A row with a page under it says A OPEN, not CHANGE.
    for row in QuickRow::ALL {
        if row.child().is_some() {
            assert!(row.opens(), "{row:?} has a page but does not open");
        }
    }
    let mut pages = vec![
        &QuickRow::MAIN[..],
        &QuickRow::SYSTEM[..],
        &QuickRow::LIBRARY[..],
        &QuickRow::SYSTEMS[..],
        &QuickRow::SOUNDS[..],
        &QuickRow::ABOUT[..],
        &QuickRow::SCREEN[..],
        &QuickRow::SCREEN_GB[..],
        &QuickRow::SCREEN_GBA[..],
        &QuickRow::SCREEN_NES[..],
        &QuickRow::SCREEN_SNES[..],
        &QuickRow::SCREEN_PS1[..],
        &QuickRow::GAME[..],
        &QuickRow::SAVING[..],
    ];
    #[cfg(feature = "brick")]
    pages.extend([&QuickRow::SETTINGS[..], &QuickRow::ONLINE[..], &QuickRow::CLOCK[..]]);
    for page in pages {
        for &row in page {
            assert_eq!(row.page(), page, "{row:?}");
        }
    }
}

#[test]
fn the_values_read_as_the_menu_prints_them() {
    assert_eq!(
        QuickValue::ALL.map(QuickValue::text),
        [
            "2×",
            "3×",
            "4×",
            "6×",
            "On",
            "Off",
            "LCD3x",
            "Grid",
            "Dot",
            "Simpletex", "Stock", "Low", "Medium", "High", "White", "Blue", "Green", "Amber", "Red", "Purple", "Steady", "Breathe", "Unavailable",
            "1 min", "5 min", "10 min", "3%", "5%", "10%", "A–Z", "By Console", "All Games", "Favorites", "Minutes", "Hours", "Sure?", "Done"
        ]
    );
    assert_eq!(
        slot_store::AUTOSAVE_MINS.map(QuickValue::minutes),
        [Some(QuickValue::Off), Some(QuickValue::Min1), Some(QuickValue::Min5), Some(QuickValue::Min10)]
    );
    assert_eq!(
        slot_store::BATTERY_SHUTDOWNS.map(QuickValue::percent),
        [Some(QuickValue::Off), Some(QuickValue::Percent3), Some(QuickValue::Percent5), Some(QuickValue::Percent10)]
    );
    assert_eq!(QuickValue::flag(true), QuickValue::On);
    assert_eq!(QuickValue::flag(false), QuickValue::Off);
}

#[test]
fn the_date_and_time_read_as_a_month_a_day_and_the_carousels_24_hour_clock() {
    let at = |stamp: &str| date_time_text(parse_stamp(stamp).expect("a stamp"));
    assert_eq!(at("2026-09-15_16-35-00"), "SEP 15 16:35");
    assert_eq!(at("2027-01-05_04-07-59"), "JAN 5 04:07");
}

#[test]
fn a_picker_for_a_set_clock_starts_at_the_local_time_and_its_offset() {
    let utc = parse_stamp("2026-09-15_21-35-42").expect("a stamp");
    let p = ClockPicker::local(utc, -300);
    assert_eq!(p.offset_min(), -300);
    assert_eq!(
        p.secs(),
        utc - 42,
        "the picker does not show the minute it was opened in"
    );
    assert!(p.text().starts_with("2026-09-15 16:35"), "{}", p.text());
}

#[test]
fn the_selected_text_uses_the_darker_menu_pink() {
    let inkiest = |lit| {
        let f = quick_value_face("4×", lit);
        f.rgba
            .chunks(4)
            .max_by_key(|p| p[3])
            .map(|p| [p[0], p[1], p[2]])
            .expect("an empty face")
    };
    assert_eq!(inkiest(true), slot_ui::MENU_SELECTION_INK);
    assert_eq!(inkiest(false), [0x9a, 0x9a, 0xa4]);
    let (label, _) = slot_ui::menu_row_face_scaled("resume", true, [1.0, 1.0]);
    assert!(label.rgba.chunks_exact(4).any(|p| p[..3] == slot_ui::MENU_SELECTION_INK && p[3] == 255));
}

#[test]
fn the_arrows_are_faces_the_height_of_a_value() {
    for right in [false, true] {
        let caret = quick_caret_face(right);
        assert!(
            caret.w > 0 && caret.rgba.chunks(4).any(|p| p[3] > 0),
            "an empty arrow"
        );
        assert_eq!(caret.h, quick_value_face("On", true).h);
    }
}

#[test]
fn built_in_values_fit_in_their_column() {
    for value in QuickValue::ALL {
        assert!(quick_value_face(value.text(), true).w <= slot_ui::quick_value_room(), "{value:?}");
    }
}

#[test]
fn each_row_sits_in_all_at_its_own_index() {
    // Arrays built from QuickRow::ALL are read by QuickRow::index: the two orders must agree.
    for (i, row) in QuickRow::ALL.into_iter().enumerate() {
        assert_eq!(row.index(), i, "{row:?}");
    }
}
