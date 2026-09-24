//! `keymap.rs`'s tests, split out for the 400-line file cap.

use super::*;

fn press(code: KeyCode, mods: KeyModifiers) -> String {
    key_name(&KeyEvent::new(code, mods)).unwrap_or_default()
}

#[test]
fn key_names_follow_the_manifest() {
    assert_eq!(press(KeyCode::Char('j'), KeyModifiers::NONE), "j");
    assert_eq!(press(KeyCode::Char('G'), KeyModifiers::SHIFT), "G");
    assert_eq!(
        press(KeyCode::Char(' '), KeyModifiers::CONTROL),
        "Ctrl-Space"
    );
    assert_eq!(press(KeyCode::Enter, KeyModifiers::SHIFT), "Shift-Enter");
    assert_eq!(press(KeyCode::Up, KeyModifiers::ALT), "Alt-Up");
    assert_eq!(
        key_name(&KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE)),
        None
    );
}

#[test]
fn every_command_has_one_binding_row_and_a_unique_id() {
    for b in BINDINGS {
        assert_eq!(Command::from_id(b.command.id()), Some(b.command));
        let rows = BINDINGS.iter().filter(|o| o.command == b.command).count();
        assert_eq!(rows, 1, "{}", b.command.id());
    }
}

#[test]
fn chords_wait_for_their_second_key_and_expire() {
    let t0 = Instant::now();
    let mut chords = Chords::default();
    assert_eq!(
        chords.resolve(Scope::List, None, "g", t0),
        Resolved::Pending
    );
    assert_eq!(
        chords.resolve(Scope::List, None, "g", t0),
        Resolved::Command(Command::ListFirst)
    );
    assert_eq!(
        chords.resolve(Scope::List, None, "d", t0),
        Resolved::Pending
    );
    assert_eq!(
        chords.resolve(Scope::List, None, "j", t0),
        Resolved::Command(Command::ListDown),
        "a broken chord drops its first key"
    );
    chords.resolve(Scope::List, None, "d", t0);
    let late = t0 + CHORD_WINDOW + Duration::from_millis(1);
    assert_eq!(
        chords.resolve(Scope::List, None, "d", late),
        Resolved::Pending,
        "the window passed"
    );
}

#[test]
fn a_sheet_resolves_only_its_own_group() {
    let now = Instant::now();
    let mut chords = Chords::default();
    let j = |c: &mut Chords, g| c.resolve(Scope::Sheet, Some(g), "j", now);
    assert_eq!(
        j(&mut chords, "conflicts"),
        Resolved::Command(Command::ConflictsDown)
    );
    assert_eq!(
        j(&mut chords, "offers"),
        Resolved::Command(Command::OffersDown)
    );
    assert_eq!(
        chords.resolve(Scope::Sheet, Some("offers"), "m", now),
        Resolved::Unbound
    );
}

#[test]
fn a_screen_shares_its_g_with_the_global_chords() {
    let now = Instant::now();
    let mut chords = Chords::default();
    let mut keys = |scope, keys: &[&str]| {
        keys.iter()
            .map(|k| chords.resolve(scope, None, k, now))
            .last()
    };
    assert_eq!(
        keys(Scope::List, &["g", "t"]),
        Some(Resolved::Command(Command::NavTasks))
    );
    assert_eq!(
        keys(Scope::List, &["g", "g"]),
        Some(Resolved::Command(Command::ListFirst))
    );
    assert_eq!(
        keys(Scope::Universal, &["g", "s"]),
        Some(Resolved::Command(Command::NavSettings))
    );
    assert_eq!(
        keys(Scope::Universal, &["g", "g"]),
        Some(Resolved::Pending),
        "no g g here: the second g starts a chord of its own"
    );
    assert_eq!(
        chords.resolve(Scope::Sheet, Some("workspace_menu"), "W", now),
        Resolved::Command(Command::WorkspaceMenuClose),
        "a sheet takes no global keys: W closes the popup"
    );
}
