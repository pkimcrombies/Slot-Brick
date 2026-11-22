mod common;

use std::time::{Duration, Instant};

use common::{app_playing_in, app_playing_with, tmp_root_with_carts, CoreSnapshot, StubSnapshot};
use slot::app::{App, Phase};
use slot::emu::Speed;
use slot::session::Session;
use slot_gfx::{Draw, TexId};
use slot_input::{Action, Btn, Millis, RawEvent};
use slot_store::{Core, Platform, StateRing};
use slot_ui::PauseChoice;

fn row(choice: PauseChoice) -> usize {
    PauseChoice::ALL.iter().position(|c| *c == choice).unwrap()
}

fn choose(a: &mut App, choice: PauseChoice) {
    a.apply(Action::QuickMenu);
    for _ in 0..row(choice) {
        a.apply(Action::GbaDown(Btn::Down));
    }
    a.apply(Action::GbaDown(Btn::A));
}

fn states(root: &std::path::Path) -> usize {
    StateRing::new(root, Platform::Gba, Core::Mgba, "Emerald")
        .list()
        .expect("list")
        .len()
}

#[test]
fn a_tap_of_home_in_game_opens_the_menu_on_resume() {
    let d = tmp_root_with_carts(&["Emerald"]);
    let mut a = app_playing_in(d.path(), "Emerald");
    assert_eq!(a.pause_menu(), None);
    a.apply(Action::QuickMenu);
    assert_eq!(a.pause_menu(), Some(row(PauseChoice::Resume)));
    assert!(matches!(a.phase(), Phase::Playing { .. }));
}

#[test]
fn the_menu_moves_and_wraps_at_both_ends() {
    let d = tmp_root_with_carts(&["Emerald"]);
    let mut a = app_playing_in(d.path(), "Emerald");
    let last = PauseChoice::ALL.len() - 1;
    a.apply(Action::QuickMenu);
    a.apply(Action::GbaDown(Btn::Up));
    assert_eq!(a.pause_menu(), Some(last), "up from the top did not wrap");
    a.apply(Action::GbaDown(Btn::Down));
    assert_eq!(a.pause_menu(), Some(0), "down from the bottom did not wrap");
    for i in 1..=last {
        a.apply(Action::GbaDown(Btn::Down));
        assert_eq!(a.pause_menu(), Some(i));
    }
}

#[test]
fn b_home_and_resume_each_go_back_to_the_game() {
    for leave in [
        Action::GbaDown(Btn::B),
        Action::QuickMenu,
        Action::GbaDown(Btn::A),
    ] {
        let d = tmp_root_with_carts(&["Emerald"]);
        let mut a = app_playing_in(d.path(), "Emerald");
        a.apply(Action::QuickMenu);
        a.apply(leave);
        assert_eq!(a.pause_menu(), None, "{leave:?} left the menu up");
        assert!(matches!(a.phase(), Phase::Playing { .. }));
        assert_eq!(states(d.path()), 0, "{leave:?} saved a state");
    }
}

#[test]
fn the_menu_covers_the_game_with_a_black_background() {
    let d = tmp_root_with_carts(&["Emerald"]);
    let mut a = app_playing_in(d.path(), "Emerald");
    let face = TexId::from_raw(970);
    let pink = TexId::from_raw(971);
    a.set_pause_menu_faces(vec![[(face, 100, 40), (pink, 100, 40)]; PauseChoice::ALL.len()]);
    a.apply(Action::QuickMenu);

    let mut out = Vec::new();
    a.draw(&mut out);

    let ground = out
        .iter()
        .rposition(|d| matches!(d, Draw::Rect { colour, .. } if *colour == slot_ui::MENU_BACKGROUND));
    let ground = ground.expect("the menu drew no ground");
    let rows: Vec<_> = out[ground..]
        .iter()
        .filter_map(|d| match *d {
            Draw::Tex { tex, x, .. } if tex == face || tex == pink => Some((tex, x)),
            _ => None,
        })
        .collect();
    assert_eq!(
        rows.len(),
        PauseChoice::ALL.len(),
        "a row is missing over the ground"
    );
    assert_eq!(rows[0].0, pink);
    assert!(rows[1..].iter().all(|&(tex, _)| tex == face));
    assert!(rows.iter().all(|&(_, x)| x + slot_ui::MENU_PAD as f32 == slot_ui::QUICK_EDGE));
    assert!(!out[ground + 1..].iter().any(|draw| matches!(draw, Draw::Rect { colour, .. } if *colour == slot_ui::edge())));
}

#[test]
fn pause_labels_are_lowercase_and_rasterized_at_display_scale() {
    let (normal, logical) = slot_ui::menu_row_face_scaled("QUIT GAME", false, [1.0, 1.0]);
    assert_ne!(normal.rgba, slot_ui::menu_face("quit game").rgba);

    let (native, native_logical) = slot_ui::menu_row_face_scaled("QUIT GAME", false, [1.6, 1.6]);
    assert_eq!(native_logical, logical);
    assert_eq!(native.w, (logical as f32 * 1.6).round() as u32);
    assert_eq!(native.h, (normal.h as f32 * 1.6).round() as u32);
}

#[test]
fn save_state_saves_and_goes_back_to_the_game() {
    let d = tmp_root_with_carts(&["Emerald"]);
    let mut a = app_playing_in(d.path(), "Emerald");
    choose(&mut a, PauseChoice::SaveState);
    assert_eq!(states(d.path()), 1);
    assert_eq!(a.pause_menu(), None);
    assert!(matches!(a.phase(), Phase::Playing { .. }));
}

#[test]
fn load_state_opens_the_switcher_and_closing_it_goes_back_to_the_game() {
    let d = tmp_root_with_carts(&["Emerald"]);
    let (snapshot, loaded) = StubSnapshot::pair();
    let mut a = app_playing_with(d.path(), "Emerald", snapshot);
    a.apply(Action::SaveState);
    choose(&mut a, PauseChoice::LoadState);
    assert!(matches!(a.phase(), Phase::Polaroids { .. }));
    a.apply(Action::GbaDown(Btn::A));
    assert!(
        loaded.lock().expect("loaded").is_some(),
        "the state was not loaded"
    );
    assert!(matches!(a.phase(), Phase::Playing { .. }));
    assert_eq!(
        a.pause_menu(),
        None,
        "the menu came back over the loaded game"
    );
}

#[test]
fn load_state_with_nothing_saved_is_refused_and_the_menu_stays() {
    let d = tmp_root_with_carts(&["Emerald"]);
    let mut a = app_playing_in(d.path(), "Emerald");
    choose(&mut a, PauseChoice::LoadState);
    assert!(a.refusal_active(a.now()));
    assert_eq!(a.pause_menu(), Some(row(PauseChoice::LoadState)));
}

#[test]
fn reset_game_starts_the_game_over_and_goes_back_to_it() {
    let d = tmp_root_with_carts(&["Emerald"]);
    let core = CoreSnapshot::new();
    for _ in 0..30 {
        core.run_frame();
    }
    let mut a = app_playing_with(d.path(), "Emerald", core.boxed());
    choose(&mut a, PauseChoice::Reset);
    assert_eq!(core.bytes(), [0; 8], "the game did not start over");
    assert_eq!(a.pause_menu(), None);
    assert!(matches!(a.phase(), Phase::Playing { .. }));
    assert_eq!(states(d.path()), 0, "reset saved a state");
}

#[test]
fn quit_game_ejects_the_cart() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    let mut a = app_playing_in(d.path(), "Emerald");
    choose(&mut a, PauseChoice::Quit);
    assert!(matches!(a.phase(), Phase::Ejecting { .. }));
    assert_eq!(a.pause_menu(), None);
}

#[test]
fn quit_game_with_one_cart_is_refused_and_the_menu_stays() {
    let d = tmp_root_with_carts(&["Emerald"]);
    let mut a = app_playing_in(d.path(), "Emerald");
    choose(&mut a, PauseChoice::Quit);
    assert!(matches!(a.phase(), Phase::Playing { .. }));
    assert_eq!(a.pause_menu(), Some(row(PauseChoice::Quit)));
}

#[test]
fn buttons_opens_its_own_screen_without_closing_the_menu() {
    let d = tmp_root_with_carts(&["Emerald"]);
    let mut a = app_playing_in(d.path(), "Emerald");
    choose(&mut a, PauseChoice::Buttons);
    assert!(a.pause_buttons(), "A on Buttons did not open its screen");
    assert_eq!(
        a.pause_menu(),
        Some(row(PauseChoice::Buttons)),
        "the menu closed rather than drilling in"
    );
    assert!(matches!(a.phase(), Phase::Playing { .. }));
}

#[test]
fn a_swaps_ab_from_the_buttons_screen_and_saves_it() {
    let d = tmp_root_with_carts(&["Emerald"]);
    let mut a = app_playing_in(d.path(), "Emerald");
    choose(&mut a, PauseChoice::Buttons);
    assert!(!a.swap_ab());
    a.apply(Action::GbaDown(Btn::A));
    assert!(a.swap_ab());
    assert!(slot_store::read_slot_state(d.path()).swap_ab, "not saved");
    assert!(a.pause_buttons(), "A closed the Buttons screen");
    a.apply(Action::GbaDown(Btn::A));
    assert!(!a.swap_ab(), "a second A did not swap back");
}

#[test]
fn b_on_the_buttons_screen_backs_out_to_the_list_not_the_game() {
    let d = tmp_root_with_carts(&["Emerald"]);
    let mut a = app_playing_in(d.path(), "Emerald");
    choose(&mut a, PauseChoice::Buttons);
    a.apply(Action::GbaDown(Btn::B));
    assert!(
        !a.pause_buttons(),
        "B did not back out of the Buttons screen"
    );
    assert_eq!(
        a.pause_menu(),
        Some(row(PauseChoice::Buttons)),
        "B on the Buttons screen reached the game instead of the list"
    );
    assert!(matches!(a.phase(), Phase::Playing { .. }));
}

#[test]
fn holding_home_in_the_menu_still_ejects() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    let mut a = app_playing_in(d.path(), "Emerald");
    a.apply(Action::QuickMenu);
    a.apply(Action::Eject);
    assert!(matches!(a.phase(), Phase::Ejecting { .. }));
    assert_eq!(a.pause_menu(), None);
}

#[test]
fn a_double_tap_still_opens_the_switcher() {
    let d = tmp_root_with_carts(&["Emerald"]);
    let mut a = app_playing_in(d.path(), "Emerald");
    a.apply(Action::SaveState);
    a.apply(Action::QuickMenu);
    a.apply(Action::Polaroids);
    assert!(matches!(a.phase(), Phase::Polaroids { .. }));
    a.apply(Action::GbaDown(Btn::B));
    assert!(matches!(a.phase(), Phase::Playing { .. }));
    assert_eq!(a.pause_menu(), None);
}

#[test]
fn game_shortcuts_wait_behind_the_menu() {
    let d = tmp_root_with_carts(&["Emerald"]);
    let mut a = app_playing_in(d.path(), "Emerald");
    a.apply(Action::QuickMenu);
    a.apply(Action::SaveState);
    assert_eq!(states(d.path()), 0, "SELECT+R1 saved behind the menu");
    a.apply(Action::GameMenu);
    assert!(
        !a.game_menu_open(),
        "SELECT+HOME opened Host / Join over the menu"
    );
}

#[test]
fn the_next_game_starts_without_the_menu() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    let mut a = app_playing_in(d.path(), "Emerald");
    choose(&mut a, PauseChoice::Quit);
    for _ in 0..600 {
        a.update(1.0 / 60.0);
        if matches!(a.phase(), Phase::Shelf) {
            break;
        }
    }
    assert!(
        matches!(a.phase(), Phase::Shelf),
        "the eject never finished"
    );
    a.apply(Action::Insert);
    a.on_core_ready();
    for _ in 0..120 {
        a.update(1.0 / 60.0);
    }
    assert!(matches!(a.phase(), Phase::Playing { .. }));
    assert_eq!(a.pause_menu(), None);
}

const FRAME_MS: Millis = 16;

fn step(s: &mut Session, now: &mut Millis, events: Vec<RawEvent>) {
    *now += FRAME_MS;
    s.feed(events, *now);
    s.update(1.0 / 60.0);
}

/// Steps the session while it waits: a core still loading is only started by a later update.
#[track_caller]
fn await_speed(s: &mut Session, now: &mut Millis, want: Speed) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while s.observed_speed() != Some(want) {
        assert!(Instant::now() < deadline, "the core never reached {want:?}");
        step(s, now, vec![]);
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn the_menu_holds_the_core_still_until_resume() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    common::clocked(d.path());
    let mut s = Session::boot(d.path().to_path_buf());
    let mut now = 0;
    s.app_mut().apply(Action::Insert);
    for _ in 0..90 {
        step(&mut s, &mut now, vec![]);
    }
    assert!(s.has_core(), "no core to hold");
    await_speed(&mut s, &mut now, Speed::Normal);

    step(&mut s, &mut now, vec![RawEvent::Down(Btn::Menu)]);
    step(&mut s, &mut now, vec![RawEvent::Up(Btn::Menu)]);
    assert_eq!(
        s.app().pause_menu(),
        Some(0),
        "a tap of HOME did not open the menu"
    );
    await_speed(&mut s, &mut now, Speed::Paused);

    // Past the double-tap window, so A is the menu's and not the switcher's.
    for _ in 0..30 {
        step(&mut s, &mut now, vec![]);
    }
    step(&mut s, &mut now, vec![RawEvent::Down(Btn::A)]);
    assert_eq!(s.app().pause_menu(), None);
    await_speed(&mut s, &mut now, Speed::Normal);
}
