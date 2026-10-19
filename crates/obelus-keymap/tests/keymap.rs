//! Key lookup, normalization, and the shape of the shipped table.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use obelus_command::Command;
use obelus_keymap::{Context, KeyChord, Keymap, Layout, control};

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
/// a letter is written small, with `Shift+` where it was held, because
/// what is written is read without regard to case.
///
/// Broken deliberately by putting the `End` and `F10` glyphs back: the key
/// is a picture again and the two `label_in(true)` below are not words. And
/// by writing the capital rather than `Shift+` and the small letter, which
/// failed on `Ctrl+Shift+f`.
#[test]
fn a_chord_is_written_with_glyphs_or_spelled_out() {
    let control_f = control('f');
    assert_eq!(control_f.label_in(false), "Ctrl+f");
    // The capital is how shift on a letter is held, not how it is
    // written: `Ctrl+F` reads back as `Ctrl+f`, so the shift is said.
    assert_eq!(
        KeyChord::new(KeyCode::Char('F'), KeyModifiers::CONTROL).label_in(false),
        "Ctrl+Shift+f"
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
    let function = obelus_keymap::function(10);
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

/// Control is drawn as command when the reader holds command for it, and
/// written into the file as control whichever way, because `ob` in a
/// terminal on the same machine reads the same file.
///
/// Broken deliberately by writing `Ctrl` in `label_as` whatever `command`
/// said, and by dropping the `cmd+` arm from `parse`: the first two and the
/// last assertion fail.
#[test]
fn control_is_drawn_as_command_and_written_as_control() {
    let save = control('s');
    assert_eq!(save.label_as(false, true), "Cmd+s");
    assert_eq!(
        save.label_as(true, true),
        format!("{} s", obelus_icons::key::COMMAND)
    );
    assert_eq!(save.label_as(false, false), save.label_in(false));
    assert_eq!(save.label_in(false), "Ctrl+s");
    // What the screen says reads back as the key it names, so copying a
    // chord off the foot into the file binds what the reader saw.
    assert_eq!(KeyChord::parse(&save.label_as(false, true)), Some(save));
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
        KeyChord::new(KeyCode::Char('t'), KeyModifiers::ALT | KeyModifiers::SHIFT),
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
    // And what a reader is likely to type by hand. Case is not a key:
    // shift is said with `shift+`, which is how it is written.
    for text in ["Ctrl+P", "CTRL+p", "ctrl+p"] {
        assert_eq!(
            KeyChord::parse(text),
            Some(KeyChord::new(KeyCode::Char('p'), KeyModifiers::CONTROL)),
            "{text}: a chord has to be readable however it is capitalised"
        );
    }
    let shifted = KeyChord::new(
        KeyCode::Char('p'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    assert_eq!(shifted.label_in(false), "Ctrl+Shift+p");
    for text in ["ctrl+shift+p", "Ctrl+Shift+P", "SHIFT+CTRL+p"] {
        assert_eq!(KeyChord::parse(text), Some(shifted), "{text}");
    }
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
    use obelus_keymap::Context;

    let moved: std::collections::BTreeMap<String, Vec<String>> = [
        ("close-document".to_string(), vec!["alt+w".to_string()]),
        ("choose-theme".to_string(), vec!["alt+y".to_string()]),
        ("open-file".to_string(), Vec::new()),
        ("nonsense.command".to_string(), vec!["ctrl+z".to_string()]),
        ("show-change".to_string(), vec!["not a key".to_string()]),
        // A key Obelus can read and can never be given: the editor takes
        // the arrows before the table is reached.
        ("go-to-line".to_string(), vec!["up".to_string()]),
    ]
    .into_iter()
    .collect();
    let (keymap, _unbound) = Keymap::with(Layout::Obelus, &moved);

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
    keymap.rebind(Command::SelectionCopy, &[control('y')]);

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

/// The keys each editor's layout is documented as having, written out by
/// hand rather than read off the table -- on control, which `obg` on a Mac
/// is told for command before a key reaches the table.
///
/// Broken deliberately by putting the TextMate symbols on `ctrl+y` rather
/// than `alt+y` (`ctrl+y` was no longer redo), by dropping the palette's
/// `alt+P`, and by putting the files on `ctrl+t` in both layouts (`ctrl+p`
/// found nothing in VS Code).
#[test]
fn each_editors_layout_has_that_editors_keys() {
    let control = KeyModifiers::CONTROL;
    let alt = KeyModifiers::ALT;
    let shift = KeyModifiers::SHIFT;
    let shared = [
        (KeyCode::Char('p'), control | shift, Command::CommandPalette),
        (KeyCode::Char('p'), alt | shift, Command::CommandPalette),
        (KeyCode::Char('e'), control, Command::DocumentList),
        (KeyCode::Char('g'), control | shift, Command::FileChanged),
        (KeyCode::Char('g'), alt, Command::FileChanged),
        (KeyCode::Char('n'), alt | shift, Command::ConversationSelect),
        (KeyCode::Char('f'), control, Command::SearchFile),
        (KeyCode::Char('f'), control | shift, Command::SearchProject),
        (KeyCode::Char('/'), alt, Command::SearchProject),
        (KeyCode::Char('u'), alt, Command::SymbolOutline),
        (KeyCode::Char('l'), alt, Command::HistoryFile),
        (KeyCode::Char('l'), alt | shift, Command::HistoryProject),
        (KeyCode::Char('.'), alt, Command::SymbolDefinition),
        (KeyCode::Char('u'), control, Command::ShowPullRequests),
        (KeyCode::Char('t'), alt | shift, Command::TodoOpen),
        (KeyCode::Char('z'), control | shift, Command::Redo),
        (KeyCode::Char('y'), control, Command::Redo),
        (KeyCode::Char('z'), control, Command::Undo),
        (KeyCode::Char('s'), control, Command::FileSave),
        (KeyCode::Char('c'), control, Command::SelectionCopy),
        (KeyCode::Char('q'), control, Command::Quit),
        // What the shipped table has on alt, still there.
        (KeyCode::Char('t'), alt, Command::TodoAdd),
        (KeyCode::Char('e'), alt, Command::SymbolTroubles),
        (KeyCode::Char('h'), alt, Command::SymbolHover),
        (KeyCode::Char('d'), alt, Command::GitHunk),
    ];
    let vs_code = [
        (KeyCode::Char('p'), control, Some(Command::FileOpen)),
        (
            KeyCode::Char('o'),
            control | shift,
            Some(Command::SymbolOutline),
        ),
        (KeyCode::Char('t'), control, Some(Command::SearchSymbols)),
        (KeyCode::Char('y'), alt, None),
    ];
    let text_mate = [
        (KeyCode::Char('t'), control, Some(Command::FileOpen)),
        (
            KeyCode::Char('t'),
            control | shift,
            Some(Command::SymbolOutline),
        ),
        (KeyCode::Char('y'), alt, Some(Command::SearchSymbols)),
        (KeyCode::Char('p'), control, None),
    ];
    for (layout, own) in [(Layout::VsCode, vs_code), (Layout::TextMate, text_mate)] {
        let keymap = Keymap::laid_out(layout);
        let keys = shared
            .iter()
            .map(|(code, modifiers, command)| (*code, *modifiers, Some(*command)))
            .chain(own);
        for (code, modifiers, command) in keys {
            assert_eq!(
                keymap.lookup(&press(code, modifiers), Context::Normal),
                command,
                "{} in {layout:?}",
                KeyChord::new(code, modifiers).label_in(false)
            );
        }
    }
}

/// None of the editors' layouts has a function key, over a file or in a
/// terminal, where the function keys reach through to the file's table.
///
/// Broken deliberately by moving `FileReload` in place of `HistoryFile`:
/// `f9` still opened the history.
#[test]
fn the_editors_layouts_have_no_function_keys() {
    for layout in [Layout::VsCode, Layout::TextMate] {
        let keymap = Keymap::laid_out(layout);
        for number in 1..=12 {
            let function = KeyChord::new(KeyCode::F(number), KeyModifiers::NONE);
            assert_eq!(keymap.command_on(function), None, "f{number} in {layout:?}");
            for context in [Context::Normal, Context::Terminal] {
                assert_eq!(
                    keymap.lookup(&press(KeyCode::F(number), KeyModifiers::NONE), context),
                    None,
                    "f{number} in {layout:?} {context:?}"
                );
            }
        }
    }
}

/// Every command the shipped table binds is still bound in every layout,
/// in every context it was bound in -- and no key is two things in one
/// context, and nothing shadows what is bound everywhere.
///
/// Broken deliberately by putting the file's history on `alt+h` (hover lost
/// its key), and by asking where each moved command was bound only after
/// they had all been taken out of the table (every one of them lost its
/// key).
#[test]
fn no_layout_loses_a_command_or_doubles_a_key() {
    let shipped = Keymap::new();
    for layout in Layout::ALL {
        let keymap = Keymap::laid_out(layout);
        for binding in shipped.bindings() {
            assert!(
                keymap
                    .bindings()
                    .iter()
                    .any(|kept| kept.command == binding.command && kept.context == binding.context),
                "{layout:?} lost {} in {:?}",
                binding.command.name(),
                binding.context
            );
        }
        let bindings = keymap.bindings();
        for (index, binding) in bindings.iter().enumerate() {
            for earlier in &bindings[..index] {
                assert!(
                    earlier.context != binding.context || earlier.chord != binding.chord,
                    "{} is {} and {} in {:?} in {layout:?}",
                    binding.chord.label_in(false),
                    earlier.command.name(),
                    binding.command.name(),
                    binding.context
                );
                let (global, specific) = match (earlier.context, binding.context) {
                    (Context::Always, Context::Always) => continue,
                    (Context::Always, _) => (earlier, binding),
                    (_, Context::Always) => (binding, earlier),
                    _ => continue,
                };
                assert!(
                    global.chord != specific.chord || global.command == specific.command,
                    "{} shadows {} in {layout:?}",
                    specific.command.name(),
                    global.command.name()
                );
            }
        }
    }
}

/// A chord with shift on a letter under control needs a window or the
/// keyboard protocol to arrive as itself, so every command on one has a
/// second chord, in the same context, that any terminal can send: alt with
/// anything, or control with a lowercase letter.
///
/// Asked of the chord rather than of how it is written: this test once
/// looked for `Ctrl+` and a capital in the label, and checked nothing at
/// all once the label became `Ctrl+Shift+p`. So it also counts the chords
/// it found, and the editors' layouts must have some.
///
/// Broken deliberately by dropping the outline's `alt+u` from vscode.json.
#[test]
fn every_shifted_chord_has_one_every_terminal_can_send() {
    let letters = || ('a'..='z').map(KeyCode::Char);
    let shifted_control = |chord: KeyChord| {
        letters().any(|letter| {
            chord == KeyChord::new(letter, KeyModifiers::CONTROL | KeyModifiers::SHIFT)
        })
    };
    let plain_control = |chord: KeyChord| {
        letters().any(|letter| chord == KeyChord::new(letter, KeyModifiers::CONTROL))
    };
    for layout in Layout::ALL {
        let keymap = Keymap::laid_out(layout);
        let mut found = 0;
        for binding in keymap.bindings() {
            if !shifted_control(binding.chord) {
                continue;
            }
            found += 1;
            let sendable = keymap.bindings().iter().any(|other| {
                other.command == binding.command
                    && other.context == binding.context
                    && (other.chord.label_in(false).starts_with("Alt+")
                        || plain_control(other.chord))
            });
            assert!(
                sendable,
                "{} is only on {} in {:?}, in {layout:?}",
                binding.command.name(),
                binding.chord.label_in(false),
                binding.context
            );
        }
        if matches!(layout, Layout::VsCode | Layout::TextMate) {
            assert!(found > 0, "{layout:?} has no ctrl+shift chord to check");
        }
    }
}

/// A terminal keeps for Obelus what it kept before, and gives the rest to
/// the program -- including `ctrl+p`, which in the editors' layouts is not
/// the palette.
///
/// Broken deliberately by leaving the terminal's palette on `ctrl+p`.
#[test]
fn a_terminal_in_an_editors_layout_keeps_what_it_kept() {
    let control = KeyModifiers::CONTROL;
    for layout in [Layout::VsCode, Layout::TextMate] {
        let keymap = Keymap::laid_out(layout);
        let terminal = |code, modifiers| keymap.lookup(&press(code, modifiers), Context::Terminal);
        assert_eq!(
            terminal(KeyCode::Char('p'), control | KeyModifiers::SHIFT),
            Some(Command::CommandPalette)
        );
        assert_eq!(
            terminal(KeyCode::Char('p'), KeyModifiers::ALT | KeyModifiers::SHIFT),
            Some(Command::CommandPalette)
        );
        assert_eq!(terminal(KeyCode::Char('p'), control), None);
        assert_eq!(terminal(KeyCode::Char('q'), control), Some(Command::Quit));
        assert_eq!(terminal(KeyCode::Char('v'), control), Some(Command::Paste));
        assert_eq!(
            terminal(KeyCode::Char('w'), control),
            Some(Command::DocumentClose)
        );
        assert_eq!(
            terminal(KeyCode::Insert, KeyModifiers::SHIFT),
            Some(Command::Paste)
        );
        // The alt chords a file has are a shell's in a terminal.
        assert_eq!(terminal(KeyCode::Char('/'), KeyModifiers::ALT), None);
        assert_eq!(terminal(KeyCode::Char('.'), KeyModifiers::ALT), None);
    }
}

/// What a label spells, for pressing it: the modifiers it names and its
/// key, with a capital letter pressed as the letter with shift.
fn pressed_as(label: &str) -> (KeyCode, KeyModifiers) {
    let mut modifiers = KeyModifiers::NONE;
    let mut rest = label;
    for (prefix, modifier) in [
        ("Ctrl+", KeyModifiers::CONTROL),
        ("Alt+", KeyModifiers::ALT),
        ("Shift+", KeyModifiers::SHIFT),
    ] {
        if let Some(tail) = rest.strip_prefix(prefix) {
            modifiers |= modifier;
            rest = tail;
        }
    }
    let code = match rest {
        "Insert" => KeyCode::Insert,
        "Enter" => KeyCode::Enter,
        "Esc" => KeyCode::Esc,
        "\u{2190}" => KeyCode::Left,
        "\u{2191}" => KeyCode::Up,
        "\u{2192}" => KeyCode::Right,
        "\u{2193}" => KeyCode::Down,
        word if word.len() > 1 && word.starts_with('F') => {
            KeyCode::F(word[1..].parse().expect("a function key"))
        }
        word => {
            let character = word.chars().next().expect("a key");
            if character.is_ascii_uppercase() {
                modifiers |= KeyModifiers::SHIFT;
                KeyCode::Char(character.to_ascii_lowercase())
            } else {
                KeyCode::Char(character)
            }
        }
    };
    (code, modifiers)
}

/// A key held with one more modifier than a binding names is not that
/// binding, in any layout: `ctrl+super+q` does not leave, `alt+ctrl+u` is
/// not the outline. And every binding's key, pressed as written, is that
/// binding -- so the first half is not passing because nothing is ever
/// found.
///
/// Broken deliberately by masking `SUPER` off in `KeyChord::from_event`
/// (`ctrl+super+q` left), and by matching in `find` on a chord whose
/// modifiers *contain* the binding's (`ctrl+alt+t` wrote a note).
#[test]
fn one_more_modifier_is_another_key_in_every_layout() {
    for layout in Layout::ALL {
        let keymap = Keymap::laid_out(layout);
        for binding in keymap.bindings() {
            let label = binding.chord.label_in(false);
            assert_eq!(KeyChord::parse(&label), Some(binding.chord), "{label}");
            let (code, modifiers) = pressed_as(&label);
            if binding.context != Context::Always {
                assert_eq!(
                    keymap.bound_here(&press(code, modifiers), binding.context),
                    Some(binding.command),
                    "{label} pressed as written, in {layout:?}"
                );
            }
            // A bare key with one modifier added is a chord of one
            // modifier, which may well be bound: `Insert` and
            // `ctrl+Insert` are two bindings.
            if (modifiers - KeyModifiers::SHIFT).is_empty() {
                continue;
            }
            for extra in [
                KeyModifiers::CONTROL,
                KeyModifiers::ALT,
                KeyModifiers::SUPER,
            ] {
                if modifiers.contains(extra) {
                    continue;
                }
                for context in [
                    Context::Normal,
                    Context::Terminal,
                    Context::Dialog,
                    Context::Documents,
                ] {
                    assert_eq!(
                        keymap.lookup(&press(code, modifiers | extra), context),
                        None,
                        "{label} with {extra:?} in {context:?}, {layout:?}"
                    );
                }
            }
        }
    }
}

/// A reader's own key is theirs whichever layout it is laid over, so
/// switching layouts keeps it -- and the layout's own key for that command
/// goes, rather than firing behind it.
///
/// Broken deliberately by pushing the reader's chord in `Keymap::with`
/// rather than rebinding the command: the layout's own key stayed beside
/// it.
#[test]
fn a_readers_keys_survive_a_change_of_layout() {
    let lines = [("open-file".to_string(), vec!["Alt+z".to_string()])]
        .into_iter()
        .collect();
    for layout in Layout::ALL {
        let (keymap, unbound) = Keymap::with(layout, &lines);
        assert!(unbound.is_empty(), "{unbound:?}");
        assert_eq!(
            keymap.lookup(
                &press(KeyCode::Char('z'), KeyModifiers::ALT),
                Context::Normal
            ),
            Some(Command::FileOpen),
            "{layout:?}"
        );
        let bound = keymap
            .bindings()
            .iter()
            .filter(|binding| binding.command == Command::FileOpen)
            .count();
        assert_eq!(bound, 1, "{layout:?} kept its own key for the files");
    }
}

/// The editors' layouts take command from `obg` on a Mac as the control it
/// arrives as, so what the screen there says of a shifted chord reads back
/// as that chord -- and super is never read as control anywhere, held alone
/// or beside it.
///
/// Broken deliberately by reading `cmd+` as `SUPER` in `KeyChord::parse`
/// (the palette's `Cmd+Shift+p` read as another key), and by masking `SUPER`
/// off in `KeyChord::from_event` (`super+ctrl+q` left).
#[test]
fn command_is_control_in_the_editors_layouts_and_super_never_is() {
    let palette = KeyChord::new(
        KeyCode::Char('p'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    assert_eq!(palette.label_as(false, true), "Cmd+Shift+p");
    assert_eq!(KeyChord::parse("Cmd+Shift+p"), Some(palette));
    assert_eq!(KeyChord::parse("cmd+shift+P"), Some(palette));
    for layout in [Layout::VsCode, Layout::TextMate] {
        let keymap = Keymap::laid_out(layout);
        assert_eq!(keymap.chord_for(Command::CommandPalette), Some(palette));
        for (code, modifiers) in [
            (KeyCode::Char('p'), KeyModifiers::SUPER),
            (
                KeyCode::Char('p'),
                KeyModifiers::SUPER | KeyModifiers::SHIFT,
            ),
            (KeyCode::Char('s'), KeyModifiers::SUPER),
            (
                KeyCode::Char('q'),
                KeyModifiers::SUPER | KeyModifiers::CONTROL,
            ),
        ] {
            for context in [Context::Normal, Context::Terminal, Context::Dialog] {
                assert_eq!(
                    keymap.lookup(&press(code, modifiers), context),
                    None,
                    "{code:?} with {modifiers:?} in {context:?}, {layout:?}"
                );
            }
        }
    }
}

/// What Obelus writes for a key with a mark or a name in it reads back as
/// that key: an arrow after a modifier, and `Insert`.
///
/// Broken deliberately by slicing the modifiers off by byte again
/// (`rest[..5]`), which panicked on `Alt+\u{2191}` -- the arrow is three
/// bytes and the fifth is inside it -- and by taking `insert` out of
/// `parse`, which read `Shift+Insert` as nothing.
#[test]
fn a_chord_with_a_mark_or_a_name_reads_back() {
    for chord in [
        KeyChord::new(KeyCode::Up, KeyModifiers::ALT),
        KeyChord::new(KeyCode::Left, KeyModifiers::ALT),
        KeyChord::new(KeyCode::Insert, KeyModifiers::SHIFT),
        KeyChord::new(KeyCode::Insert, KeyModifiers::CONTROL),
        KeyChord::new(KeyCode::Insert, KeyModifiers::NONE),
    ] {
        let written = chord.label_in(false);
        assert_eq!(KeyChord::parse(&written), Some(chord), "{written}");
    }
    assert_eq!(
        KeyChord::parse("alt+\u{2193}"),
        Some(KeyChord::new(KeyCode::Down, KeyModifiers::ALT))
    );
    assert_eq!(KeyChord::parse("\u{2191}+"), None);
}

/// A line of `[keys]` may name several keys, and the command is on those,
/// in that order, and on no other: the first is the one shown, and every
/// one of them is listed.
///
/// Broken deliberately by binding only the first of a line's keys in
/// `Keymap::with`: `alt+y` did nothing.
#[test]
fn a_command_can_be_on_several_keys() {
    let lines = [(
        "choose-theme".to_string(),
        vec!["alt+z".to_string(), "alt+y".to_string()],
    )]
    .into_iter()
    .collect();
    let (keymap, unbound) = Keymap::with(Layout::Obelus, &lines);
    assert!(unbound.is_empty(), "{unbound:?}");
    let chord = |key: &str| KeyChord::parse(key).expect("a key");
    assert_eq!(
        keymap.chords_for(Command::ThemeSelect),
        [chord("alt+z"), chord("alt+y")]
    );
    assert_eq!(keymap.chord_for(Command::ThemeSelect), Some(chord("alt+z")));
    for key in ['z', 'y'] {
        assert_eq!(
            keymap.lookup(
                &press(KeyCode::Char(key), KeyModifiers::ALT),
                Context::Normal
            ),
            Some(Command::ThemeSelect),
            "alt+{key}"
        );
    }
}

/// One key of a line that will not do is said, and the line's other keys
/// still bind; a line none of whose keys will do leaves the command where
/// the layout put it, rather than on nothing.
///
/// Broken deliberately by giving up on the whole line at its first bad
/// key, as the one-key reading did: `alt+y` was not bound.
#[test]
fn a_bad_key_in_a_line_is_said_and_the_rest_bind() {
    let lines = [
        (
            "choose-theme".to_string(),
            vec![
                "not a key".to_string(),
                "alt+y".to_string(),
                "esc".to_string(),
            ],
        ),
        ("save-file".to_string(), vec!["nonsense".to_string()]),
    ]
    .into_iter()
    .collect();
    let (keymap, unbound) = Keymap::with(Layout::Obelus, &lines);
    let chord = |key: &str| KeyChord::parse(key).expect("a key");
    assert_eq!(keymap.chords_for(Command::ThemeSelect), [chord("alt+y")]);
    let said: Vec<_> = unbound
        .iter()
        .map(|one| (one.name.as_str(), one.text.as_str()))
        .collect();
    assert_eq!(
        said,
        [
            ("choose-theme", "not a key"),
            ("choose-theme", "esc"),
            ("save-file", "nonsense")
        ]
    );
    assert_eq!(keymap.chords_for(Command::FileSave), [chord("ctrl+s")]);
}

/// Every key of a rebinding goes in every context the command was in, in
/// the order given, and an empty list takes them all away.
///
/// Broken deliberately by putting only the first key in each context in
/// `rebind`: close-document's second key did nothing in the list of what is
/// open.
#[test]
fn rebinding_to_several_keys_puts_each_where_the_command_was() {
    let mut keymap = Keymap::new();
    let chord = |key: &str| KeyChord::parse(key).expect("a key");
    keymap.rebind(Command::DocumentClose, &[chord("alt+w"), chord("alt+q")]);
    for context in [Context::Normal, Context::Documents, Context::Terminal] {
        assert_eq!(
            keymap.lookup(&press(KeyCode::Char('q'), KeyModifiers::ALT), context),
            Some(Command::DocumentClose),
            "{context:?}"
        );
    }
    assert_eq!(
        keymap.chords_for(Command::DocumentClose),
        [chord("alt+w"), chord("alt+q")]
    );
    keymap.rebind(Command::DocumentClose, &[]);
    assert!(keymap.chords_for(Command::DocumentClose).is_empty());
}
