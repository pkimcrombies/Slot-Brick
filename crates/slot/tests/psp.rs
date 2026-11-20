#[test]
fn a_single_psp_game_does_not_auto_launch_on_boot() {
    let root = tempfile::tempdir().unwrap();
    slot::root::ensure(root.path());
    let mut state = slot_store::SlotState::default();
    state.clock_set = true;
    state.cart = Some("Demo".into());
    state.cart_platform = Some(slot_store::Platform::Psp);
    slot_store::write_slot_state(root.path(), &state).unwrap();
    std::fs::write(root.path().join("Games/PSP/Demo.iso"), []).unwrap();
    let app = slot::app::App::boot(root.path());
    assert!(matches!(app.phase(), slot::app::Phase::Shelf));
}

#[test]
fn selecting_a_second_psp_image_with_the_same_stem_preserves_its_path() {
    use slot_input::{Action, Btn};
    let root = tempfile::tempdir().unwrap();
    slot::root::ensure(root.path());
    let mut state = slot_store::SlotState::default();
    state.clock_set = true;
    slot_store::write_slot_state(root.path(), &state).unwrap();
    for name in ["Demo.iso", "Demo.cso"] {
        std::fs::write(root.path().join("Games/PSP").join(name), []).unwrap();
    }
    let mut app = slot::app::App::boot(root.path());
    let first = app.carts().next().unwrap().rom.clone();
    app.apply(Action::GbaDown(Btn::Right));
    app.apply(Action::GbaUp(Btn::Right));
    app.apply(Action::Insert);
    assert_ne!(app.seated_cart().unwrap().rom, first);
}

#[test]
fn slot_can_quit_its_only_psp_game_from_its_pause_menu() {
    use slot_input::{Action, Btn};
    let root = tempfile::tempdir().unwrap();
    slot::root::ensure(root.path());
    let state = slot_store::SlotState { clock_set: true, ..Default::default() };
    slot_store::write_slot_state(root.path(), &state).unwrap();
    std::fs::write(root.path().join("Games/PSP/Demo.iso"), []).unwrap();
    let mut app = slot::app::App::boot(root.path());
    app.apply(Action::Insert);
    app.set_platform(slot_store::Platform::Psp);
    app.on_core_ready();
    app.update(1.0);
    assert!(matches!(app.phase(), slot::app::Phase::Playing { .. }));
    app.apply(Action::QuickMenu);
    app.apply(Action::GbaDown(Btn::Up));
    assert_eq!(app.pause_menu(), Some(3));
    app.apply(Action::GbaDown(Btn::A));
    assert!(matches!(app.phase(), slot::app::Phase::Ejecting { .. }));
}
