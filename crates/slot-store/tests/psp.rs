use slot_store::{read_slot_state, scan, write_slot_state, Platform, SlotState};

#[test]
fn psp_discovers_supported_images_without_treating_them_as_ps1_or_archives() {
    let root = tempfile::tempdir().unwrap();
    let games = root.path().join("Games/PSP");
    std::fs::create_dir_all(&games).unwrap();
    for name in [
        "Alpha.ISO",
        "Beta.cso",
        "Homebrew.pbp",
        "ignored.zip",
        "ignored.7z",
        ".hidden.iso",
    ] {
        std::fs::write(games.join(name), []).unwrap();
    }
    let carts = scan(root.path()).unwrap();
    assert_eq!(
        carts.iter().map(|c| c.stem.as_str()).collect::<Vec<_>>(),
        ["Alpha", "Beta", "Homebrew"]
    );
    assert!(carts.iter().all(|c| c.platform == Platform::Psp));
    assert!(!Platform::Psp.takes_archives());
}

#[test]
fn psp_settings_roundtrip_without_changing_existing_platform_indexes() {
    assert_eq!(Platform::Ps1.index(), 5);
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("Config")).unwrap();
    let mut state = SlotState::default();
    state.cart = Some("Demo".into());
    state.cart_platform = Some(Platform::Psp);
    state.hidden_systems[Platform::Psp.index()] = true;
    write_slot_state(root.path(), &state).unwrap();
    let restored = read_slot_state(root.path());
    assert_eq!(restored.cart_platform, Some(Platform::Psp));
    assert!(!restored.shows(Platform::Psp));
    assert!(restored.shows(Platform::Ps1));
}
