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
    // Only expect fallback to Slot for a glyph it actually contains.
    // Open Sans does not guarantee CJK coverage.
    let original = slot_ui::text::in_builtin_font(|| label_font().unwrap());
    let missing = (0x21..=0xffff)
        .filter_map(char::from_u32)
        .find(|&ch| !ch.is_whitespace()
            && chosen.lookup_glyph_index(ch) == 0
            && original.lookup_glyph_index(ch) != 0)
        .expect("test fonts must have at least one glyph exclusive to Slot");
    assert!(std::ptr::eq(font_for(chosen, missing), original),
        "a glyph available only in Slot must use Slot's font");
    // Without an installed supplementary font, a character absent in both
    // faces retains the selected face's missing-glyph behaviour.
    let absent = '\u{10ffff}';
    assert_eq!(chosen.lookup_glyph_index(absent), 0);
    assert_eq!(original.lookup_glyph_index(absent), 0);
    assert!(std::ptr::eq(font_for(chosen, absent), chosen));
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
