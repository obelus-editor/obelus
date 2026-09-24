//! Key lookup, normalization, and the shape of the shipped table.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use obelus_command::Command;
use obelus_editing::keymap::{Context, KeyChord, Keymap, control};

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
    // A chord nothing wants, which is all this needs. It was `f12` until
    // that became the jump to a definition, and `ctrl+z` before that until
    // it became undo -- a test that borrows an interesting key keeps having
    // to be rewritten when the key is earned.
    let event = press(KeyCode::Char('z'), KeyModifiers::ALT);
    assert_eq!(keymap.lookup(&event, Context::Normal), None);
}

#[test]
fn control_c_copies_the_selection() {
    let keymap = Keymap::new();
    let event = press(KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert_eq!(
        keymap.lookup(&event, Context::Normal),
        Some(Command::SelectionCopy)
    );
}

#[test]
fn release_events_are_discarded() {
    let keymap = Keymap::new();
    let mut event = press(KeyCode::Char('q'), KeyModifiers::CONTROL);
    event.kind = KeyEventKind::Release;

    // A terminal speaking the kitty keyboard protocol reports a press and a
    // release for one keystroke. Matching both fires every binding twice,
    // which reads as "Obelus quit before I let go" rather than as a bug in
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

/// A key held with a modifier Obelus cannot be bound to is a different key,
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

/// Both ways of writing a chord. With glyphs the modifiers stop being
/// prefixes and a key that is a word becomes one column, which is the point:
/// `ctrl+pagedown` is thirteen columns of a right-aligned key column, and
/// every one of them comes off the room the description has.
#[test]
fn a_chord_is_written_with_glyphs_or_spelled_out() {
    let control_f = control('f');
    assert_eq!(control_f.label_in(false), "ctrl+f");
    assert_eq!(
        control_f.label_in(true),
        format!("{} f", obelus_icons::key::CONTROL)
    );

    let page = KeyChord::new(KeyCode::PageDown, KeyModifiers::CONTROL);
    assert_eq!(page.label_in(false), "ctrl+pagedown");
    assert_eq!(
        page.label_in(true),
        format!(
            "{} {}",
            obelus_icons::key::CONTROL,
            obelus_icons::key::PAGE_DOWN
        )
    );
    // Which is the whole argument for the glyphs: five columns instead of
    // thirteen.
    assert!(page.label_in(true).chars().count() < page.label_in(false).chars().count());

    // A function key has a keycap of its own, which is one column where
    // `f10` is three -- and the twelve of them are the first keys a reader
    // looks for.
    let function = obelus_editing::keymap::function(10);
    assert_eq!(function.label_in(false), "f10");
    assert_eq!(
        function.label_in(true),
        obelus_icons::key::function(10)
            .expect("a keycap")
            .to_string()
    );
    assert_eq!(
        obelus_icons::key::function(13),
        None,
        "a keycap was invented for a key most keyboards do not have"
    );
    // Which still reads back as itself: the file is written in the spelled
    // form, and it is the form a reader types by hand.
    assert_eq!(KeyChord::parse("f10"), Some(function));

    // An arrow is a symbol in any font, so it is an arrow either way.
    let jump = KeyChord::new(KeyCode::Left, KeyModifiers::ALT);
    assert_eq!(jump.label_in(false), "alt+\u{2190}");
    assert_eq!(
        jump.label_in(true),
        format!("{} \u{2190}", obelus_icons::key::ALT)
    );

    // And every glyph is followed by a blank column, because a non-`Mono`
    // Nerd Font draws them two cells wide and the second cell is not ours.
    assert!(
        control_f
            .label_in(true)
            .contains(&format!("{} ", obelus_icons::key::CONTROL)),
        "a glyph with nothing after it bleeds over whatever follows"
    );

    // `label` is one of the two, and which one is the switch's business.
    assert_eq!(
        control_f.label(),
        control_f.label_in(obelus_icons::enabled())
    );
}

/// A chord written down reads back as itself.
///
/// The file is the only place a rebinding survives, so a chord that does not
/// survive being written and read is a key the reader binds twice.
#[test]
fn a_chord_survives_being_written_down() {
    for chord in [
        KeyChord::new(KeyCode::Char('p'), KeyModifiers::CONTROL),
        KeyChord::new(KeyCode::Char('a'), KeyModifiers::ALT),
        KeyChord::new(KeyCode::Enter, KeyModifiers::ALT),
        KeyChord::new(KeyCode::Char('A'), KeyModifiers::SHIFT),
        KeyChord::new(KeyCode::PageDown, KeyModifiers::NONE),
        KeyChord::new(KeyCode::F(7), KeyModifiers::CONTROL),
        KeyChord::new(KeyCode::Up, KeyModifiers::NONE),
        KeyChord::new(KeyCode::Char(' '), KeyModifiers::NONE),
    ] {
        let written = chord.label_in(false);
        assert_eq!(
            KeyChord::parse(&written),
            Some(chord),
            "{written:?} did not read back as itself"
        );
    }
    // And what a reader is likely to type by hand.
    assert_eq!(
        KeyChord::parse("Ctrl+P"),
        Some(KeyChord::new(KeyCode::Char('P'), KeyModifiers::CONTROL)),
        "a chord has to be readable however it is capitalised"
    );
    assert_eq!(
        KeyChord::parse("alt+left"),
        Some(KeyChord::new(KeyCode::Left, KeyModifiers::ALT))
    );
    assert_eq!(KeyChord::parse("ctrl+"), None, "half a chord is not one");
    assert_eq!(KeyChord::parse("wat"), None, "a word is not a key");
}

/// A rebinding moves every one of a command's keys, and the file's
/// bindings are changes over the defaults rather than the whole table.
#[test]
fn the_readers_own_bindings_go_over_the_defaults() {
    use obelus_editing::keymap::Context;

    let moved: std::collections::BTreeMap<String, String> = [
        ("close-document".to_string(), "alt+w".to_string()),
        ("choose-theme".to_string(), "alt+y".to_string()),
        ("open-file".to_string(), String::new()),
        ("nonsense.command".to_string(), "ctrl+z".to_string()),
        ("show-change".to_string(), "not a key".to_string()),
        // A key Obelus can read and can never be given: the editor takes
        // the arrows before the table is reached.
        ("go-to-line".to_string(), "up".to_string()),
    ]
    .into_iter()
    .collect();
    let keymap = Keymap::with(&moved);

    // Both of `close-document`'s bindings moved: it is one command with one
    // key, bound in two contexts so that it reaches the list of open files.
    let closes: Vec<_> = keymap
        .bindings()
        .iter()
        .filter(|binding| binding.command == Command::DocumentClose)
        .map(|binding| (binding.context, binding.chord))
        .collect();
    assert_eq!(closes.len(), 2, "a context lost the command: {closes:?}");
    assert!(
        closes
            .iter()
            .all(|(_, chord)| *chord == KeyChord::new(KeyCode::Char('w'), KeyModifiers::ALT)),
        "one of the bindings kept the old key: {closes:?}"
    );

    // A command that had no key gets one where the reader is reading.
    assert_eq!(
        keymap.lookup(
            &press(KeyCode::Char('y'), KeyModifiers::ALT),
            Context::Normal
        ),
        Some(Command::ThemeSelect)
    );
    // A key taken away is taken away.
    assert_eq!(
        keymap.chord_for(Command::FileOpen),
        None,
        "the key the reader removed is still bound"
    );
    // And everything else is the default, including the two the file got
    // wrong: a typo leaves the reader with Obelus, not with holes.
    assert_eq!(
        keymap.chord_for(Command::CommandPalette),
        Keymap::new().chord_for(Command::CommandPalette)
    );
    assert_eq!(
        keymap.chord_for(Command::GitHunk),
        Keymap::new().chord_for(Command::GitHunk),
        "a chord the file spelled wrong took the default with it"
    );
    assert_eq!(
        keymap.chord_for(Command::GoLine),
        Keymap::new().chord_for(Command::GoLine),
        "a key that could never fire was taken out of the file and bound"
    );
}
