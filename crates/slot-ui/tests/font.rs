use slot_ui::text::{font_for, label_font, set_label_font};

/// A chosen font draws all text, and a character it lacks comes from Slot's own font.
#[test]
fn a_chosen_font_draws_the_text_and_the_built_in_one_fills_its_gaps() {
    let built_in = label_font().expect("Slot's own font") as *const _;
    let hud = || slot_ui::text::in_builtin_font(|| {
        let (clock, width) = slot_ui::word_face_ink_scaled("12:34", [0; 3], [1.6, 1.6]);
        let (battery, battery_width) = slot_ui::word_face_ink_scaled("91%", [0; 3], [1.6, 1.6]);
        (clock.rgba, width, battery.rgba, battery_width)
    });
    let before = hud();
    let toast_before = slot_ui::toast_face(slot_ui::Toast::StateSaved);
    let menu_before = slot_ui::quick_label_face(slot_ui::QuickRow::Music);
    let gohu = include_bytes!("../../../brick/card/Slot/System/Fonts/GohuFont.ttf");
    assert!(set_label_font(gohu));
    let chosen = label_font().expect("the chosen font");
    assert!(
        !std::ptr::eq(chosen, built_in),
        "the chosen font is not in use"
    );
    assert!(
        std::ptr::eq(font_for(chosen, 'É'), chosen),
        "GohuFont has É"
    );
    assert!(
        std::ptr::eq(font_for(chosen, '漢'), built_in),
        "a character GohuFont lacks"
    );
    assert!(std::ptr::eq(font_for(chosen, ' '), chosen));
    assert_eq!(hud(), before, "HUD glyphs and logical widths must be unchanged");
    let toast_after = slot_ui::toast_face(slot_ui::Toast::StateSaved);
    assert_eq!(toast_after.rgba, toast_before.rgba, "HUD toast uses the original font");
    let menu_after = slot_ui::quick_label_face(slot_ui::QuickRow::Music);
    assert_ne!(menu_after.rgba, menu_before.rgba, "menu still follows the selected font");
    assert!(std::ptr::eq(label_font().unwrap(), chosen), "HUD scope restores the selected font");
    assert!(
        !set_label_font(b"not a font"),
        "a second font, or one that is not, is refused"
    );
}
