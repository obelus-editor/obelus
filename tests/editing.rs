//! Changing a document, and everything that has to stay in step with it.
//!
//! A buffer holds a great deal beside its text — a parse tree, the folds, the
//! blocks hanging between lines, a version five caches key on — and all of it
//! is measured against the text. These are the tests that say so.

mod support;

use obelus::{
    buffer::{Buffer, undo::Doing},
    coordinates::{CharColumn, LineNumber, Span},
};

/// A buffer over a file of the test's own, so an edit has somewhere to
/// happen that is not the repository.
fn opened(name: &str, contents: &str) -> (support::Scratch, Buffer) {
    let scratch = support::Scratch::new(name);
    let path = scratch.path().join("sample.rs");
    std::fs::write(&path, contents).expect("writing the file");
    let buffer = Buffer::open(&path).expect("opening it");
    (scratch, buffer)
}

/// One character, at one place.
fn at(line: usize, column: usize) -> Span {
    Span {
        line: LineNumber::new(line),
        column: CharColumn::new(column),
        end_line: LineNumber::new(line),
        end_column: CharColumn::new(column),
    }
}

#[test]
fn an_edit_moves_the_version_the_caches_are_keyed_on() {
    let (_scratch, mut buffer) = opened("edit-version", "fn main() {}\n");
    let before = buffer.version();

    assert!(buffer.edit(at(0, 3), "x", Doing::Typing));
    assert_eq!(
        buffer.version(),
        before + 1,
        "the version did not move, so five caches still hold the old document"
    );

    // Replacing nothing with nothing is not a change and must not pretend
    // to be one: a version that moves for it invalidates everything for
    // nothing.
    assert!(!buffer.edit(at(0, 3), "", Doing::Typing));
    assert_eq!(buffer.version(), before + 1);
}

#[test]
fn an_edit_is_told_to_the_parse() {
    let (_scratch, mut buffer) = opened("edit-parse", "fn main() { let s = \"ab\"; }\n");
    let kind = |buffer: &Buffer, column: usize| {
        let text = buffer.text();
        let byte = text.byte_of_char(text.char_offset(LineNumber::new(0), CharColumn::new(column)));
        let state = buffer.syntax().expect("a parse");
        let mut highlights = obelus::syntax::highlight::Highlights::default();
        highlights.refresh(
            state,
            text,
            obelus::coordinates::ByteOffset::new(0)..text.byte_length(),
        );
        highlights.kind_at(byte)
    };
    // Inside the string literal to begin with.
    let inside = kind(&buffer, 21);
    assert!(inside.is_some(), "the sample has no highlighting to lose");

    // Widen the literal. If the tree were not told, the bytes after the edit
    // would still be read against the old tree and the character now inside
    // the string would be highlighted as whatever used to be there.
    assert!(buffer.edit(at(0, 21), "cdefgh", Doing::Typing));
    assert_eq!(
        kind(&buffer, 25),
        inside,
        "a character inside the widened string is not string any more"
    );
}

#[test]
fn a_fold_below_an_edit_moves_with_it() {
    let source = "fn one() {\n    body\n}\n\nfn two() {\n    body\n    more\n}\n";
    let (_scratch, mut buffer) = opened("edit-folds", source);
    // Fold the second function, which begins on line 4.
    assert!(
        buffer.toggle_fold(LineNumber::new(4)),
        "the second function does not offer a fold"
    );
    assert!(
        buffer.folds().hides(LineNumber::new(5)),
        "the second function did not fold"
    );

    // A line added above it, which moves every line below by one.
    assert!(buffer.edit(at(0, 0), "// a note\n", Doing::Typing));
    assert!(
        buffer.folds().hides(LineNumber::new(6)),
        "the fold did not move with the lines it was about"
    );
    assert!(
        !buffer.folds().hides(LineNumber::new(5)),
        "the fold stayed where the lines no longer are"
    );
}

#[test]
fn a_commits_version_refuses_to_be_edited() {
    let (_scratch, buffer) = opened("edit-commit", "fn main() {}\n");
    let path = buffer.path().to_path_buf();
    let mut version = Buffer::at_commit(&path, gix::ObjectId::null(gix::hash::Kind::Sha1), "old\n");
    let before = version.text().rope().to_string();

    assert!(
        !version.edit(at(0, 0), "x", Doing::Typing),
        "a commit's version let itself be written"
    );
    assert_eq!(version.text().rope().to_string(), before);
}

#[test]
fn undoing_everything_gives_back_the_document_that_was_opened() {
    let source = "fn main() {\n    let x = 1;\n}\n";
    let (_scratch, mut buffer) = opened("undo-round-trip", source);

    // A spread of edits: typing, a deletion, a whole replacement, and one
    // that adds a line.
    buffer.edit(at(1, 12), "23", Doing::Typing);
    buffer.settle_undo();
    buffer.edit(
        Span {
            line: LineNumber::new(1),
            column: CharColumn::new(4),
            end_line: LineNumber::new(1),
            end_column: CharColumn::new(7),
        },
        "const",
        Doing::Whole,
    );
    buffer.settle_undo();
    buffer.edit(at(0, 11), "\n    // note", Doing::Whole);

    assert_ne!(buffer.text().rope().to_string(), source);

    let mut steps = 0;
    while buffer.undo() {
        steps += 1;
        assert!(steps < 20, "undo did not run out");
    }
    assert_eq!(
        buffer.text().rope().to_string(),
        source,
        "undoing everything did not give back what was opened"
    );

    while buffer.redo() {}
    assert_ne!(
        buffer.text().rope().to_string(),
        source,
        "redoing everything did not put the edits back"
    );
}

#[test]
fn a_run_of_typing_comes_back_in_one_step() {
    let (_scratch, mut buffer) = opened("undo-grouping", "fn main() {}\n");

    // Five characters, the way a reader types them: each one where the last
    // one left off.
    for (step, character) in "hello".chars().enumerate() {
        buffer.edit(at(0, 3 + step), &character.to_string(), Doing::Typing);
    }
    assert_eq!(buffer.text().rope().to_string(), "fn hellomain() {}\n");

    assert!(buffer.undo());
    assert_eq!(
        buffer.text().rope().to_string(),
        "fn main() {}\n",
        "one undo did not take the whole run of typing"
    );
    assert!(!buffer.can_undo(), "the run was more than one step");
}

#[test]
fn moving_the_cursor_starts_a_new_step() {
    let (_scratch, mut buffer) = opened("undo-settle", "fn main() {}\n");

    buffer.edit(at(0, 3), "a", Doing::Typing);
    // The reader went somewhere else and came back, which is a decision.
    buffer.settle_undo();
    buffer.edit(at(0, 4), "b", Doing::Typing);

    assert!(buffer.undo());
    assert_eq!(
        buffer.text().rope().to_string(),
        "fn amain() {}\n",
        "the two runs were treated as one"
    );
    assert!(buffer.undo());
    assert_eq!(buffer.text().rope().to_string(), "fn main() {}\n");
}

#[test]
fn a_paste_is_never_swallowed_by_the_typing_around_it() {
    let (_scratch, mut buffer) = opened("undo-whole", "fn main() {}\n");

    buffer.edit(at(0, 3), "a", Doing::Typing);
    buffer.edit(at(0, 4), "PASTED", Doing::Whole);
    buffer.edit(at(0, 10), "b", Doing::Typing);

    assert!(buffer.undo());
    assert_eq!(buffer.text().rope().to_string(), "fn aPASTEDmain() {}\n");
    assert!(buffer.undo());
    assert_eq!(
        buffer.text().rope().to_string(),
        "fn amain() {}\n",
        "the paste did not come back on its own"
    );
}

#[test]
fn a_re_read_forgets_what_could_have_been_put_back() {
    let (scratch, mut buffer) = opened("undo-reload", "one\n");
    buffer.edit(at(0, 3), "x", Doing::Typing);
    assert!(buffer.can_undo());

    std::fs::write(scratch.path().join("sample.rs"), "something else\n").expect("rewriting it");
    assert!(buffer.reload().expect("re-reading it"));
    assert!(
        !buffer.can_undo(),
        "a step about text that has been replaced is still offered"
    );
}

/// Editing as a reader does it: through the keys.
mod keys {
    use crossterm::event::KeyCode;
    use obelus::{
        app::App,
        buffer::Buffer,
        command::{Command, dispatch},
    };

    use super::support;

    fn reading(name: &str, contents: &str) -> (support::Scratch, App) {
        let scratch = support::Scratch::new(name);
        let path = scratch.path().join("sample.rs");
        std::fs::write(&path, contents).expect("writing the file");
        let mut app = App::new(vec![Buffer::open(&path).expect("opening it")]);
        app.working_directory_for_test(scratch.path().to_path_buf());
        support::lay_out(&mut app, 60, 12);
        (scratch, app)
    }

    fn text(app: &App) -> String {
        app.current_buffer()
            .expect("a buffer")
            .text()
            .rope()
            .to_string()
    }

    #[test]
    fn typing_puts_characters_where_the_cursor_is() {
        let (_scratch, mut app) = reading("keys-typing", "fn main() {}\n");
        for _ in 0..3 {
            support::press(&mut app, KeyCode::Right);
        }
        support::type_text(&mut app, "hello");
        assert_eq!(text(&app), "fn hellomain() {}\n");

        support::press(&mut app, KeyCode::Enter);
        assert_eq!(text(&app), "fn hello\nmain() {}\n");
    }

    #[test]
    fn backspace_takes_what_is_behind_and_delete_what_is_in_front() {
        let (_scratch, mut app) = reading("keys-deleting", "abcd\n");
        for _ in 0..2 {
            support::press(&mut app, KeyCode::Right);
        }
        support::press(&mut app, KeyCode::Backspace);
        assert_eq!(text(&app), "acd\n");
        support::press(&mut app, KeyCode::Delete);
        assert_eq!(text(&app), "ad\n");
    }

    /// The one thing every reader expects of them without being told.
    #[test]
    fn backspace_over_a_selection_takes_the_selection() {
        let (_scratch, mut app) = reading("keys-selection", "one two three\n");
        for _ in 0..3 {
            support::press_shift(&mut app, KeyCode::Right);
        }
        support::press(&mut app, KeyCode::Backspace);
        assert_eq!(text(&app), " two three\n");

        // And typing over one replaces it.
        for _ in 0..4 {
            support::press_shift(&mut app, KeyCode::Right);
        }
        support::type_text(&mut app, "X");
        assert_eq!(text(&app), "X three\n");
    }

    /// A line break joins the lines it is between, from either side.
    #[test]
    fn deleting_across_the_end_of_a_line_joins_them() {
        let (_scratch, mut app) = reading("keys-join", "one\ntwo\n");
        support::press(&mut app, KeyCode::End);
        support::press(&mut app, KeyCode::Delete);
        assert_eq!(text(&app), "onetwo\n");

        let (_scratch, mut app) = reading("keys-join-back", "one\ntwo\n");
        support::press(&mut app, KeyCode::Down);
        support::press(&mut app, KeyCode::Backspace);
        assert_eq!(text(&app), "onetwo\n");
    }

    /// Nothing behind the first character and nothing in front of the last.
    #[test]
    fn deleting_off_either_end_does_nothing() {
        let (_scratch, mut app) = reading("keys-ends", "one\n");
        support::press(&mut app, KeyCode::Backspace);
        assert_eq!(text(&app), "one\n");

        support::press_control_key(&mut app, KeyCode::End);
        support::press(&mut app, KeyCode::Delete);
        assert_eq!(text(&app), "one\n");
    }

    /// The clipboard is one thing for the whole process, so the tests that
    /// use it take turns.
    static CLIPBOARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn cutting_takes_the_selection_out_and_pasting_puts_it_back() {
        let _turn = CLIPBOARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // Nothing outside obelus: a suite that used whatever this machine
        // has would reach into the clipboard of whoever ran it, and would
        // pass or fail by what happened to be on it.
        obelus::clipboard::use_provider_for_test(obelus::clipboard::Provider::Kept);

        let (_scratch, mut app) = reading("keys-cut", "one two\n");
        for _ in 0..4 {
            support::press_shift(&mut app, KeyCode::Right);
        }
        dispatch::dispatch(&mut app, Command::SelectionCut);
        assert_eq!(text(&app), "two\n");

        support::press_control_key(&mut app, KeyCode::End);
        dispatch::dispatch(&mut app, Command::Paste);
        assert!(
            text(&app).starts_with("two\none "),
            "the cut text did not come back: {:?}",
            text(&app)
        );
    }

    #[test]
    fn undo_and_redo_walk_the_changes() {
        let (_scratch, mut app) = reading("keys-undo", "fn main() {}\n");
        support::type_text(&mut app, "abc");
        assert_eq!(text(&app), "abcfn main() {}\n");

        dispatch::dispatch(&mut app, Command::Undo);
        assert_eq!(
            text(&app),
            "fn main() {}\n",
            "one undo did not take the run"
        );
        dispatch::dispatch(&mut app, Command::Redo);
        assert_eq!(text(&app), "abcfn main() {}\n");
    }

    /// A pasted function is one change, however many lines it is.
    #[test]
    fn what_the_terminal_pastes_arrives_as_one_change() {
        let (_scratch, mut app) = reading("keys-bracketed", "\n");
        app.handle(obelus::event::Event::Paste(
            "fn one() {}\nfn two() {}\n".to_string(),
        ));
        assert_eq!(text(&app), "fn one() {}\nfn two() {}\n\n");

        dispatch::dispatch(&mut app, Command::Undo);
        assert_eq!(
            text(&app),
            "\n",
            "undoing a paste took it back a line at a time"
        );
    }

    /// The commands say so rather than doing nothing, because a key that is
    /// not offered does nothing at all and the palette row would be grey.
    /// What a cut can be pasted back out of when nothing outside can be
    /// read -- which is OSC 52 always, and every other provider whenever
    /// the program behind it is gone or says nothing.
    #[test]
    fn a_clipboard_that_cannot_be_read_still_pastes_what_obelus_cut() {
        let _turn = CLIPBOARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        obelus::clipboard::use_provider_for_test(obelus::clipboard::Provider::Osc52);

        assert_eq!(
            obelus::clipboard::paste(),
            None,
            "something was on the clipboard before anything was put there"
        );
        obelus::clipboard::copy("what obelus cut").expect("copying");
        assert_eq!(
            obelus::clipboard::paste().as_deref(),
            Some("what obelus cut"),
            "a sequence that cannot be read back left nothing to paste"
        );
    }

    #[test]
    fn a_document_with_nothing_to_undo_does_not_offer_it() {
        let (_scratch, mut app) = reading("keys-offers", "one\n");
        assert!(!app.offers(Command::Undo));
        assert!(!app.offers(Command::Redo));
        // Paste is always offered: what is on a clipboard is not a question
        // that can be answered without asking an outside program.
        assert!(app.offers(Command::Paste));

        support::type_text(&mut app, "x");
        assert!(app.offers(Command::Undo));
        assert!(!app.offers(Command::Redo));
        dispatch::dispatch(&mut app, Command::Undo);
        assert!(app.offers(Command::Redo));
    }
}
