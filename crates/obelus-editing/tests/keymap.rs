//! Key lookup, normalization, and the shape of the shipped table.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use obelus_command::Command;
use obelus_editing::keymap::{Context, KeyChord, Keymap, Layout, control};

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
    //
    // Unless what wins is the same command, which takes nothing away: a key
    // named twice for one act is that key reaching further, and it is how
    // `ctrl+q` leaves from inside a dialog — where what is bound everywhere
    // is deliberately out of reach, and where leaving is the one act that
    // opens nothing over what is already showing. What this is looking for
    // is a *different* command winning in silence.
    for global in bindings.iter().filter(|b| b.context == Context::Always) {
        for specific in bindings.iter().filter(|b| b.context != Context::Always) {
            assert!(
                global.chord != specific.chord || global.command == specific.command,
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
/// prefixes and become their glyph; the key itself is its word either way,
/// because `Ctrl+End` drawn as two pictures was a chord nobody could read.
///
/// A key that is a word is written the way keys are written everywhere;
/// a letter is left alone, because a shifted letter *is* the capital.
///
/// Broken deliberately by putting the `End` and `F10` glyphs back: the key
/// is a picture again and the two `label_in(true)` below are not words.
#[test]
fn a_chord_is_written_with_glyphs_or_spelled_out() {
    let control_f = control('f');
    assert_eq!(control_f.label_in(false), "Ctrl+f");
    // The letter is the reader's, not Obelus's to tidy: the capital is
    // how a chord with shift on a letter is written, so `Ctrl+F` would be
    // a different key on the screen and a different key in the file.
    assert_eq!(
        KeyChord::new(KeyCode::Char('F'), KeyModifiers::CONTROL).label_in(false),
        "Ctrl+F"
    );
    assert_eq!(
        control_f.label_in(true),
        format!("{} f", obelus_icons::key::CONTROL)
    );

    let page = KeyChord::new(KeyCode::PageDown, KeyModifiers::CONTROL);
    assert_eq!(page.label_in(false), "Ctrl+PageDown");
    assert_eq!(
        page.label_in(true),
        format!("{} PageDown", obelus_icons::key::CONTROL)
    );
    let end = KeyChord::new(KeyCode::End, KeyModifiers::CONTROL);
    assert_eq!(
        end.label_in(true),
        format!("{} End", obelus_icons::key::CONTROL),
        "the key under the modifier is a picture"
    );

    // And a function key is its name, glyphs or not.
    let function = obelus_editing::keymap::function(10);
    assert_eq!(function.label_in(false), "F10");
    assert_eq!(function.label_in(true), "F10");
    // Which still reads back as itself, and so does the way it used to be
    // written: the file is written in the spelled form, a reader types it
    // by hand, and a settings file from before this was capitalised is
    // still a settings file.
    assert_eq!(KeyChord::parse("F10"), Some(function));
    assert_eq!(KeyChord::parse("f10"), Some(function));
    assert_eq!(KeyChord::parse("ctrl+pagedown"), Some(page));

    // An arrow is a symbol in any font, so it is an arrow either way.
    let jump = KeyChord::new(KeyCode::Left, KeyModifiers::ALT);
    assert_eq!(jump.label_in(false), "Alt+\u{2190}");
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
    let (keymap, _unbound) = Keymap::with(Layout::Classic, &moved);

    // All of `close-document`'s bindings moved: it is one command with one
    // key, bound in three contexts so that it reaches the list of open files
    // and a terminal, where it is the program's otherwise and the reader's
    // own key is the reader's own choice.
    let closes: Vec<_> = keymap
        .bindings()
        .iter()
        .filter(|binding| binding.command == Command::DocumentClose)
        .map(|binding| (binding.context, binding.chord))
        .collect();
    assert_eq!(closes.len(), 3, "a context lost the command: {closes:?}");
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

/// Copy and paste answer to the names a desktop knows them by, in both the
/// file and a dialog.
///
/// A desktop's own one chord for copy is turned into a key and sent to
/// whatever has the focus, and which key depends on what it takes that
/// thing for: omarchy's `super+c` is `ctrl+c` for a window and
/// `ctrl+Insert` for something it reads as a terminal. Obelus is both, and
/// is read as either, so it answers both -- and the same table is what `ob`
/// and `obg` are looking things up in, so binding it once is binding it for
/// both.
///
/// Deliberate break: either pair of bindings removed, which leaves its own
/// assertion looking up `None`.
#[test]
fn copy_and_paste_answer_to_the_chords_a_desktop_sends() {
    let keymap = Keymap::new();
    let copy = press(KeyCode::Insert, KeyModifiers::CONTROL);
    let paste = press(KeyCode::Insert, KeyModifiers::SHIFT);

    for context in [Context::Normal, Context::Dialog] {
        assert_eq!(
            keymap.lookup(&copy, context),
            Some(Command::SelectionCopy),
            "ctrl+insert in {context:?}"
        );
        assert_eq!(
            keymap.lookup(&paste, context),
            Some(Command::Paste),
            "shift+insert in {context:?}"
        );
    }

    // And the key it is a modified form of still means what it is named
    // after, which is the one thing these must not have taken.
    assert_eq!(
        keymap.lookup(&press(KeyCode::Insert, KeyModifiers::NONE), Context::Normal),
        Some(Command::ReplaceToggle)
    );
}

/// And those two are the only modified `Insert` a reader may bind.
///
/// `why_not` is asked by the keys page and by the config file as well as by
/// the shipped table, so letting one chord through is letting it through
/// everywhere. Two is what the desktop sends; a third would be Obelus
/// inventing one after all.
///
/// Deliberate break: `why_not` letting any modified `Insert` through, which
/// is the obvious way to write the exception and is what the last two
/// assertions are for.
#[test]
fn only_the_two_chords_a_desktop_sends_are_a_modified_insert() {
    let may = |modifiers| {
        obelus_editing::keymap::why_not(KeyChord::new(KeyCode::Insert, modifiers)).is_none()
    };

    assert!(may(KeyModifiers::CONTROL));
    assert!(may(KeyModifiers::SHIFT));
    assert!(may(KeyModifiers::NONE));
    // Alt asks about the cursor, and there is nothing it would ask here.
    assert!(!may(KeyModifiers::ALT));
    // And two modifiers is the desktop's own, wherever it lands.
    assert!(!may(KeyModifiers::CONTROL | KeyModifiers::SHIFT));
}

/// Rebinding a command with two chords leaves it with one, once per
/// context.
///
/// Copy answers to `ctrl+c` and to `ctrl+Insert`, in the file and in a
/// dialog, so it is in each of those contexts twice -- and to `ctrl+Insert`
/// in a terminal, once. A rebind that made one
/// binding per *appearance* would put the reader's key in twice -- two
/// bindings on one chord in one context, which is the thing
/// `no_chord_is_bound_twice_in_one_context` holds the shipped table to and
/// nothing was holding a rebuilt one to.
///
/// Deliberate break: `rebind` collecting the contexts without asking
/// whether it already has one, which is how it was written when every
/// command had one chord.
#[test]
fn rebinding_a_command_with_two_chords_leaves_it_one_key_per_context() {
    let mut keymap = Keymap::new();
    keymap.rebind(Command::SelectionCopy, Some(control('y')));

    let copies: Vec<_> = keymap
        .bindings()
        .iter()
        .filter(|binding| binding.command == Command::SelectionCopy)
        .collect();
    assert_eq!(copies.len(), 3, "{copies:?}");
    assert!(copies.iter().all(|binding| binding.chord == control('y')));
    // The three it was in, each once.
    let mut contexts: Vec<_> = copies.iter().map(|binding| binding.context).collect();
    contexts.dedup();
    assert_eq!(contexts.len(), 3);

    // And the chord it had is nobody's now, rather than still copying.
    assert_eq!(
        keymap.lookup(
            &press(KeyCode::Insert, KeyModifiers::CONTROL),
            Context::Normal
        ),
        None
    );
}
