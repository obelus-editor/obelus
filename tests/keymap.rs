//! Key lookup, normalization, and the shape of the shipped table.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use obelus::{
    command::Command,
    keymap::{Context, KeyChord, Keymap, control},
};

fn press(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent {
        code,
        modifiers,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
}

#[test]
fn a_global_binding_is_found_from_a_specific_context() {
    let keymap = Keymap::new();
    let event = press(KeyCode::Char('q'), KeyModifiers::CONTROL);

    assert_eq!(keymap.lookup(&event, Context::Normal), Some(Command::Quit));
    assert_eq!(keymap.lookup(&event, Context::Always), Some(Command::Quit));
}

#[test]
fn an_unbound_key_resolves_to_nothing() {
    let keymap = Keymap::new();
    let event = press(KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert_eq!(keymap.lookup(&event, Context::Normal), None);
}

#[test]
fn release_events_are_discarded() {
    let keymap = Keymap::new();
    let mut event = press(KeyCode::Char('q'), KeyModifiers::CONTROL);
    event.kind = KeyEventKind::Release;

    // A terminal speaking the kitty keyboard protocol reports a press and a
    // release for one keystroke. Matching both fires every binding twice,
    // which reads as "obelus quit before I let go" rather than as a bug in
    // key handling.
    assert_eq!(keymap.lookup(&event, Context::Normal), None);
}

#[test]
fn repeats_are_treated_as_presses() {
    let keymap = Keymap::new();
    let mut event = press(KeyCode::Char('q'), KeyModifiers::CONTROL);
    event.kind = KeyEventKind::Repeat;
    assert_eq!(keymap.lookup(&event, Context::Normal), Some(Command::Quit));
}

#[test]
fn the_three_ways_a_terminal_reports_a_shifted_letter_agree() {
    let uppercase_bare = KeyChord::new(KeyCode::Char('A'), KeyModifiers::NONE);
    let uppercase_shifted = KeyChord::new(KeyCode::Char('A'), KeyModifiers::SHIFT);
    let lowercase_shifted = KeyChord::new(KeyCode::Char('a'), KeyModifiers::SHIFT);

    assert_eq!(uppercase_bare, uppercase_shifted);
    assert_eq!(uppercase_bare, lowercase_shifted);

    // Shift still distinguishes a chord when a control key is involved, for
    // the terminals that can tell the two apart at all.
    assert_ne!(
        KeyChord::new(KeyCode::Char('p'), KeyModifiers::CONTROL),
        KeyChord::new(
            KeyCode::Char('p'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT
        )
    );
}

#[test]
fn a_modifier_no_binding_can_rely_on_is_kept_and_so_matches_nothing() {
    // SUPER and HYPER only ever arrive from a terminal speaking the kitty
    // protocol, so a chord carrying one would match in some terminals and not
    // others. It is kept rather than dropped, so such a chord is a binding
    // that never fires — not one that steals `ctrl+q`.
    assert_ne!(
        KeyChord::new(
            KeyCode::Char('q'),
            KeyModifiers::CONTROL | KeyModifiers::SUPER
        ),
        control('q')
    );
}

#[test]
fn no_chord_is_bound_twice_in_one_context() {
    let keymap = Keymap::new();
    let bindings = keymap.bindings();

    for (index, binding) in bindings.iter().enumerate() {
        for earlier in &bindings[..index] {
            assert!(
                earlier.context != binding.context || earlier.chord != binding.chord,
                "{:?} and {:?} are both bound to the same key in {:?}",
                earlier.command,
                binding.command,
                binding.context
            );
        }
    }
}

#[test]
fn no_global_binding_is_shadowed_by_a_context_binding() {
    let keymap = Keymap::new();
    let bindings = keymap.bindings();

    // A shadowed global is not a lookup bug — the specific context is meant to
    // win — but it is always a mistake in the table: the global becomes
    // unreachable in that context with nothing to say so.
    for global in bindings.iter().filter(|b| b.context == Context::Always) {
        for specific in bindings.iter().filter(|b| b.context != Context::Always) {
            assert!(
                global.chord != specific.chord,
                "{:?} shadows the global {:?}",
                specific.command,
                global.command
            );
        }
    }
}

/// A key held with a modifier obelus cannot be bound to is a different key,
/// and matches nothing. Ignoring the modifier instead would quit on
/// `ctrl+super+q` — an answer, and the wrong one, where none was asked for.
#[test]
fn a_modifier_obelus_does_not_know_disqualifies_the_key() {
    let keymap = Keymap::new();
    for extra in [KeyModifiers::SUPER, KeyModifiers::HYPER, KeyModifiers::META] {
        let event = press(KeyCode::Char('q'), KeyModifiers::CONTROL | extra);
        assert_eq!(
            keymap.lookup(&event, Context::Normal),
            None,
            "{extra:?} was ignored rather than respected"
        );
        assert_eq!(KeyChord::from_event(&event), None, "{extra:?}");
    }

    // And the bare chord still works, so the check above is about the
    // modifier and not about the key.
    let event = press(KeyCode::Char('q'), KeyModifiers::CONTROL);
    assert_eq!(keymap.lookup(&event, Context::Normal), Some(Command::Quit));
}
