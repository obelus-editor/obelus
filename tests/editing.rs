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

/// Writing a document back, and what happens when the file moved first.
mod saving {
    use crossterm::event::KeyCode;
    use obelus::{
        app::App,
        buffer::Buffer,
        command::{Command, dispatch},
    };

    use super::support;

    fn reading(name: &str, contents: &str) -> (support::Scratch, App, std::path::PathBuf) {
        let scratch = support::Scratch::new(name);
        let path = scratch.path().join("sample.rs");
        std::fs::write(&path, contents).expect("writing the file");
        let mut app = App::new(vec![Buffer::open(&path).expect("opening it")]);
        app.working_directory_for_test(scratch.path().to_path_buf());
        support::lay_out(&mut app, 70, 12);
        (scratch, app, path)
    }

    #[test]
    fn what_was_typed_reaches_the_file() {
        let (_scratch, mut app, path) = reading("save-basic", "fn main() {}\n");
        support::type_text(&mut app, "// ");
        dispatch::dispatch(&mut app, Command::FileSave);

        assert_eq!(
            std::fs::read_to_string(&path).expect("reading it back"),
            "// fn main() {}\n"
        );
        // And it is no longer a document with something to write.
        assert!(!app.current_buffer().expect("a buffer").is_dirty());
    }

    /// A fresh `fs::write` creates with default permissions, which would
    /// quietly disarm a script.
    #[cfg(unix)]
    #[test]
    fn an_executable_file_is_still_executable() {
        use std::os::unix::fs::PermissionsExt as _;

        let (_scratch, mut app, path) = reading("save-mode", "#!/bin/sh\necho hello\n");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("making it executable");

        support::type_text(&mut app, "#");
        dispatch::dispatch(&mut app, Command::FileSave);

        let mode = std::fs::metadata(&path)
            .expect("its data")
            .permissions()
            .mode();
        assert_eq!(
            mode & 0o777,
            0o755,
            "saving took the executable bit off, which disarms a script"
        );
    }

    /// A rename replaces the *name*, and where the settings of a machine
    /// live in a dotfiles repository the name is a link.
    #[cfg(unix)]
    #[test]
    fn a_link_is_still_a_link_and_what_it_points_at_is_written() {
        let scratch = support::Scratch::new("save-link");
        let real = scratch.path().join("elsewhere.rs");
        let link = scratch.path().join("sample.rs");
        std::fs::write(&real, "fn main() {}\n").expect("the real file");
        std::os::unix::fs::symlink(&real, &link).expect("the link");

        let mut app = App::new(vec![Buffer::open(&link).expect("opening the link")]);
        app.working_directory_for_test(scratch.path().to_path_buf());
        support::lay_out(&mut app, 70, 12);
        support::type_text(&mut app, "// ");
        dispatch::dispatch(&mut app, Command::FileSave);

        assert!(
            std::fs::symlink_metadata(&link)
                .expect("the link")
                .file_type()
                .is_symlink(),
            "the link was replaced by an ordinary file"
        );
        assert_eq!(
            std::fs::read_to_string(&real).expect("what it points at"),
            "// fn main() {}\n",
            "the file the link points at was not written"
        );
    }

    /// The watcher exists so a file an agent rewrote comes back by itself.
    /// Over a document somebody has edited that is losing their work.
    #[test]
    fn a_file_that_moves_under_an_edit_is_not_read_over_it() {
        let (_scratch, mut app, path) = reading("save-moved", "mine\n");
        support::type_text(&mut app, "x");

        std::fs::write(&path, "somebody else's\n").expect("rewriting it");
        app.handle(obelus::event::Event::FileChanged { path: path.clone() });

        let buffer = app.current_buffer().expect("a buffer");
        assert_eq!(
            buffer.text().rope().to_string(),
            "xmine\n",
            "the edit was read over"
        );
        assert!(buffer.has_moved(), "the file moving was not noticed");
    }

    /// And a clean one still comes back by itself, which is the whole point
    /// of watching at all.
    #[test]
    fn a_file_that_moves_under_no_edit_still_comes_back() {
        let (_scratch, mut app, path) = reading("save-clean", "mine\n");
        std::fs::write(&path, "somebody else's\n").expect("rewriting it");
        app.handle(obelus::event::Event::FileChanged { path: path.clone() });

        assert_eq!(
            app.current_buffer()
                .expect("a buffer")
                .text()
                .rope()
                .to_string(),
            "somebody else's\n",
            "a clean buffer did not take the new file"
        );
    }

    #[test]
    fn saving_over_a_file_that_moved_takes_two_presses() {
        let (_scratch, mut app, path) = reading("save-overwrite", "mine\n");
        support::type_text(&mut app, "x");
        std::fs::write(&path, "somebody else's\n").expect("rewriting it");
        app.handle(obelus::event::Event::FileChanged { path: path.clone() });

        // The first refuses and says why.
        dispatch::dispatch(&mut app, Command::FileSave);
        // On the status row, not merely set: a note too long for the row is
        // dropped whole rather than half-drawn, and a warning nobody sees
        // is not a warning.
        let dump = support::render(&mut app, 70, 12);
        assert!(
            support::text_block(&dump).contains("changed on disk"),
            "nothing was said about the file moving:\n{dump}"
        );
        assert_eq!(
            std::fs::read_to_string(&path).expect("reading it"),
            "somebody else's\n",
            "the first press wrote over somebody else's file"
        );

        // The second goes through, which is what asking again means.
        dispatch::dispatch(&mut app, Command::FileSave);
        assert_eq!(
            std::fs::read_to_string(&path).expect("reading it"),
            "xmine\n",
            "asking again did not overwrite"
        );
    }

    /// Reloading is the other way out, and takes the file rather than the
    /// edit.
    #[test]
    fn reloading_a_file_that_moved_takes_what_is_on_disk() {
        let (_scratch, mut app, path) = reading("save-reload", "mine\n");
        support::type_text(&mut app, "x");
        std::fs::write(&path, "somebody else's\n").expect("rewriting it");
        app.handle(obelus::event::Event::FileChanged { path: path.clone() });

        dispatch::dispatch(&mut app, Command::FileReload);
        let buffer = app.current_buffer().expect("a buffer");
        assert_eq!(buffer.text().rope().to_string(), "somebody else's\n");
        assert!(!buffer.is_dirty(), "what was read is not what is on disk");
        assert!(!buffer.has_moved(), "the file is still said to have moved");
    }

    #[test]
    fn a_commits_version_is_not_a_file_to_save() {
        let (_scratch, mut app, path) = reading("save-commit", "mine\n");
        let _ = KeyCode::Enter;
        let version =
            Buffer::at_commit(&path, gix::ObjectId::null(gix::hash::Kind::Sha1), "older\n");
        app.open_buffer_for_test(version);
        dispatch::dispatch(&mut app, Command::FileSave);

        assert_eq!(
            std::fs::read_to_string(&path).expect("reading it"),
            "mine\n",
            "a commit's version was written over the file"
        );
        // And told why, rather than "nothing to save". A commit's version
        // can never be dirty -- editing it is refused a layer down -- so
        // without this it would be turned away for the wrong reason, and a
        // reader would go looking for the change they thought they lost.
        assert!(
            app.note()
                .is_some_and(|note| note.contains("commit's version")),
            "turned away for the wrong reason: {:?}",
            app.note()
        );
    }
}

/// Saying what is unwritten, and not throwing it away by accident.
mod saying {
    use crossterm::event::KeyCode;
    use obelus::{
        app::App,
        buffer::Buffer,
        command::{Command, dispatch},
    };

    use super::support;

    fn reading(name: &str, contents: &str) -> (support::Scratch, App, std::path::PathBuf) {
        let scratch = support::Scratch::new(name);
        let path = scratch.path().join("sample.rs");
        std::fs::write(&path, contents).expect("writing the file");
        let mut app = App::new(vec![Buffer::open(&path).expect("opening it")]);
        app.working_directory_for_test(scratch.path().to_path_buf());
        support::lay_out(&mut app, 70, 12);
        (scratch, app, path)
    }

    #[test]
    fn the_status_row_says_a_document_is_unwritten() {
        let (_scratch, mut app, _path) = reading("say-status", "fn main() {}\n");
        let clean = support::render(&mut app, 70, 12);
        assert!(!support::text_block(&clean).contains("unsaved"));

        support::type_text(&mut app, "x");
        let dirty = support::render(&mut app, 70, 12);
        assert!(
            support::text_block(&dirty).contains("unsaved"),
            "nothing says the document has changes that are not on disk:\n{dirty}"
        );

        dispatch::dispatch(&mut app, Command::FileSave);
        let saved = support::render(&mut app, 70, 12);
        assert!(
            !support::text_block(&saved).contains("unsaved"),
            "it still says unsaved after being written:\n{saved}"
        );
    }

    #[test]
    fn the_status_row_says_the_file_moved() {
        let (_scratch, mut app, path) = reading("say-moved", "mine\n");
        support::type_text(&mut app, "x");
        std::fs::write(&path, "somebody else's\n").expect("rewriting it");
        app.handle(obelus::event::Event::FileChanged { path });

        let dump = support::render(&mut app, 70, 12);
        assert!(
            support::text_block(&dump).contains("moved"),
            "nothing says the file moved under the edit:\n{dump}"
        );
    }

    #[test]
    fn the_buffer_list_marks_what_is_unwritten() {
        let (_scratch, mut app, _path) = reading("say-list", "fn main() {}\n");
        support::type_text(&mut app, "x");
        dispatch::dispatch(&mut app, Command::BufferList);

        let marked: Vec<bool> = app
            .picker()
            .expect("the list")
            .matches()
            .map(|item| item.marker.is_some())
            .collect();
        assert_eq!(marked, [true], "the row does not say the file is unwritten");
    }

    #[test]
    fn leaving_with_something_unwritten_takes_two_presses() {
        let (_scratch, mut app, _path) = reading("say-quit", "fn main() {}\n");
        support::type_text(&mut app, "x");

        dispatch::dispatch(&mut app, Command::Quit);
        assert!(!app.should_quit(), "it left with an unwritten document");
        let dump = support::render(&mut app, 70, 12);
        assert!(
            support::text_block(&dump).contains("unsaved"),
            "nothing was said about what leaving would lose:\n{dump}"
        );

        dispatch::dispatch(&mut app, Command::Quit);
        assert!(app.should_quit(), "asking again did not leave");
    }

    /// The warning is about the files as they are, not as they were.
    #[test]
    fn typing_after_being_warned_asks_again() {
        let (_scratch, mut app, _path) = reading("say-again", "fn main() {}\n");
        support::type_text(&mut app, "x");
        dispatch::dispatch(&mut app, Command::Quit);
        assert!(!app.should_quit());

        support::type_text(&mut app, "y");
        dispatch::dispatch(&mut app, Command::Quit);
        assert!(
            !app.should_quit(),
            "a warning given before the last change was taken for this one"
        );
    }

    #[test]
    fn leaving_with_nothing_unwritten_goes_at_once() {
        let (_scratch, mut app, _path) = reading("say-clean", "fn main() {}\n");
        let _ = KeyCode::Enter;
        dispatch::dispatch(&mut app, Command::Quit);
        assert!(
            app.should_quit(),
            "it asked about a document nobody changed"
        );
    }
}

/// What a file was written with, it is written back with.
mod bytes {
    use obelus::{
        app::App,
        buffer::Buffer,
        command::{Command, dispatch},
    };

    use super::support;

    /// Saving writes the rope rather than the lines, so what a line ends
    /// with is whatever it ended with. `Text::line` strips `\r\n` on the way
    /// out, and a save built from lines would turn a CRLF file into an LF
    /// one without saying so.
    #[test]
    fn a_files_line_endings_survive_being_saved() {
        let scratch = support::Scratch::new("bytes-crlf");
        let path = scratch.path().join("sample.rs");
        std::fs::write(&path, "one\r\ntwo\r\n").expect("writing it");
        let mut app = App::new(vec![Buffer::open(&path).expect("opening it")]);
        app.working_directory_for_test(scratch.path().to_path_buf());
        support::lay_out(&mut app, 70, 12);

        support::type_text(&mut app, "x");
        dispatch::dispatch(&mut app, Command::FileSave);

        assert_eq!(
            std::fs::read_to_string(&path).expect("reading it back"),
            "xone\r\ntwo\r\n",
            "the line endings were changed by saving"
        );
    }

    /// A file with no newline at the end had none for a reason.
    #[test]
    fn a_file_that_ended_without_a_newline_still_does() {
        let scratch = support::Scratch::new("bytes-no-newline");
        let path = scratch.path().join("sample.rs");
        std::fs::write(&path, "no newline here").expect("writing it");
        let mut app = App::new(vec![Buffer::open(&path).expect("opening it")]);
        app.working_directory_for_test(scratch.path().to_path_buf());
        support::lay_out(&mut app, 70, 12);

        support::type_text(&mut app, "x");
        dispatch::dispatch(&mut app, Command::FileSave);

        assert_eq!(
            std::fs::read_to_string(&path).expect("reading it back"),
            "xno newline here",
            "saving put a newline on the end of a file that had none"
        );
    }
}

/// Getting about: a word at a time, and the ends of a wrapped row.
mod moving {
    use crossterm::event::{KeyCode, KeyModifiers};
    use obelus::{app::App, buffer::Buffer};

    use super::support;

    fn reading(name: &str, contents: &str, width: u16) -> (support::Scratch, App) {
        let scratch = support::Scratch::new(name);
        let path = scratch.path().join("sample.rs");
        std::fs::write(&path, contents).expect("writing the file");
        let mut app = App::new(vec![Buffer::open(&path).expect("opening it")]);
        app.working_directory_for_test(scratch.path().to_path_buf());
        support::lay_out(&mut app, width, 12);
        (scratch, app)
    }

    fn at(app: &App) -> (usize, usize) {
        let cursor = app.current_buffer().expect("a buffer").cursor();
        (cursor.line.get(), cursor.column.get())
    }

    fn control(app: &mut App, code: KeyCode) {
        app.handle(obelus::event::Event::Key(crossterm::event::KeyEvent::new(
            code,
            KeyModifiers::CONTROL,
        )));
    }

    /// `foo.bar` is three words, not one: punctuation is not blank, and a
    /// reader stepping through code expects to stop at the dot.
    #[test]
    fn a_word_at_a_time_stops_at_punctuation() {
        let (_scratch, mut app) = reading("move-words", "let x = foo.bar(y);\n", 80);
        let mut stops = Vec::new();
        for _ in 0..9 {
            control(&mut app, KeyCode::Right);
            stops.push(at(&app).1);
        }
        assert_eq!(stops, [3, 5, 7, 11, 12, 15, 16, 17, 19], "{stops:?}");

        // And back the way it came.
        let mut back = Vec::new();
        for _ in 0..4 {
            control(&mut app, KeyCode::Left);
            back.push(at(&app).1);
        }
        assert_eq!(back, [17, 16, 15, 12], "{back:?}");
    }

    /// Nothing to the left on this line means the end of the line above,
    /// which is where the character to the left of column zero really is.
    #[test]
    fn a_word_steps_over_the_end_of_a_line() {
        let (_scratch, mut app) = reading("move-word-lines", "one\ntwo\n", 80);
        control(&mut app, KeyCode::Right);
        control(&mut app, KeyCode::Right);
        assert_eq!(at(&app), (1, 3), "it did not step onto the next line");

        control(&mut app, KeyCode::Left);
        control(&mut app, KeyCode::Left);
        assert_eq!(at(&app), (0, 0));
    }

    /// With wrapping on, a row is what a reader reads as a line -- and it
    /// is what `home` looks like it means.
    #[test]
    fn home_and_end_are_about_the_row_when_a_line_wraps() {
        // Narrow enough that the one line takes three rows.
        let (_scratch, mut app) =
            reading("move-wrapped", "alpha beta gamma delta epsilon zeta\n", 16);
        app.configure(
            obelus::config::Config {
                wrap: true,
                ..obelus::config::Config::default()
            },
            Vec::new(),
        );
        support::render(&mut app, 16, 12);

        // Down twice puts the cursor on the third row of the same line.
        support::press(&mut app, KeyCode::Down);
        support::press(&mut app, KeyCode::Down);
        let (line, column) = at(&app);
        assert_eq!(line, 0, "the sample is one line and the cursor left it");
        assert!(column > 0, "the cursor did not reach a later row");

        support::press(&mut app, KeyCode::Home);
        let (_, start) = at(&app);
        assert_ne!(
            start, 0,
            "home went to the top of the whole line rather than the row"
        );

        support::press(&mut app, KeyCode::End);
        let (_, end) = at(&app);
        assert!(
            end < "alpha beta gamma delta epsilon zeta".len(),
            "end went past the row to the end of the line"
        );
        assert!(end > start, "end landed before the row began");
    }

    /// And with wrapping off a row is a line, so the keys mean what they
    /// always meant.
    #[test]
    fn home_and_end_are_about_the_line_when_nothing_wraps() {
        let (_scratch, mut app) = reading("move-unwrapped", "alpha beta gamma\n", 80);
        support::press(&mut app, KeyCode::End);
        assert_eq!(at(&app), (0, 16));
        support::press(&mut app, KeyCode::Home);
        assert_eq!(at(&app), (0, 0));
    }
}

/// Where a new line starts, and where a closing bracket lands.
mod indenting {
    use crossterm::event::KeyCode;
    use obelus::{app::App, buffer::Buffer};

    use super::support;

    fn reading(name: &str, contents: &str) -> (support::Scratch, App) {
        let scratch = support::Scratch::new(name);
        let path = scratch.path().join("sample.rs");
        std::fs::write(&path, contents).expect("writing the file");
        let mut app = App::new(vec![Buffer::open(&path).expect("opening it")]);
        app.working_directory_for_test(scratch.path().to_path_buf());
        support::lay_out(&mut app, 70, 12);
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
    fn a_new_line_keeps_the_indent_of_the_one_above() {
        let (_scratch, mut app) = reading("indent-keep", "    let x = 1;\n");
        support::press(&mut app, KeyCode::End);
        support::press(&mut app, KeyCode::Enter);
        support::type_text(&mut app, "y");
        assert_eq!(text(&app), "    let x = 1;\n    y\n");
    }

    #[test]
    fn a_line_that_opened_something_indents_one_more() {
        let (_scratch, mut app) = reading("indent-open", "    fn main() {\n");
        support::press(&mut app, KeyCode::End);
        support::press(&mut app, KeyCode::Enter);
        support::type_text(&mut app, "body");
        assert_eq!(text(&app), "    fn main() {\n        body\n");
    }

    /// What is in front of the cursor moves down; what is behind it decides
    /// the indent.
    #[test]
    fn returning_inside_a_line_does_not_indent_by_what_came_after() {
        let (_scratch, mut app) = reading("indent-middle", "    foo({ });\n");
        // Just past the brace, so the brace is what is behind the cursor.
        for _ in 0..9 {
            support::press(&mut app, KeyCode::Right);
        }
        support::press(&mut app, KeyCode::Enter);
        assert_eq!(
            text(&app),
            // Eight of indent -- the line's four and one step more for the
            // brace behind the cursor -- and then the space that was in
            // front of it and moved down.
            "    foo({\n         });\n",
            "the indent was decided by what moved down rather than what stayed"
        );
    }

    #[test]
    fn a_closing_bracket_lines_up_with_what_it_closes() {
        let (_scratch, mut app) = reading("indent-close", "fn main() {\n");
        support::press(&mut app, KeyCode::End);
        support::press(&mut app, KeyCode::Enter);
        support::type_text(&mut app, "body");
        support::press(&mut app, KeyCode::Enter);
        support::type_text(&mut app, "}");
        assert_eq!(
            text(&app),
            "fn main() {\n    body\n}\n",
            "the closing bracket sat one step in from what it closes"
        );
    }

    /// Only as the first thing on a line: a bracket in the middle of one is
    /// a bracket, not a decision about indentation.
    #[test]
    fn a_closing_bracket_in_the_middle_of_a_line_is_just_a_bracket() {
        let (_scratch, mut app) = reading("indent-inline", "    foo(bar\n");
        support::press(&mut app, KeyCode::End);
        support::type_text(&mut app, ")");
        assert_eq!(text(&app), "    foo(bar)\n");
    }
}

/// An agent changing a file obelus has open.
mod agents {
    use obelus::{
        app::App,
        buffer::Buffer,
        command::{Command, dispatch},
    };

    use super::support;

    fn reading(name: &str, contents: &str) -> (support::Scratch, App, std::path::PathBuf) {
        let scratch = support::Scratch::new(name);
        let path = scratch.path().join("sample.rs");
        std::fs::write(&path, contents).expect("writing the file");
        let mut app = App::new(vec![Buffer::open(&path).expect("opening it")]);
        app.working_directory_for_test(
            scratch
                .path()
                .canonicalize()
                .expect("the scratch directory"),
        );
        support::lay_out(&mut app, 70, 12);
        (scratch, app, path)
    }

    fn asked(app: &mut App, path: &std::path::Path, text: &str) -> bool {
        app.write_for_agent_for_test(path, text)
    }

    fn text(app: &App) -> String {
        app.current_buffer()
            .expect("a buffer")
            .text()
            .rope()
            .to_string()
    }

    /// The whole reason this is allowed: the reader can take it back.
    #[test]
    fn what_an_agent_writes_to_an_open_file_can_be_undone() {
        let (_scratch, mut app, path) = reading("agent-undo", "fn main() {}\n");
        assert!(asked(&mut app, &path, "fn main() { done(); }\n"));
        assert_eq!(text(&app), "fn main() { done(); }\n");

        dispatch::dispatch(&mut app, Command::Undo);
        assert_eq!(
            text(&app),
            "fn main() {}\n",
            "a change the reader did not make could not be taken back"
        );
    }

    /// It goes into the document rather than past it, so the reader is told
    /// there is something unwritten.
    #[test]
    fn an_agents_change_is_unwritten_until_the_reader_saves() {
        let (_scratch, mut app, path) = reading("agent-dirty", "one\n");
        assert!(asked(&mut app, &path, "two\n"));

        assert!(app.current_buffer().expect("a buffer").is_dirty());
        assert_eq!(
            std::fs::read_to_string(&path).expect("the file"),
            "one\n",
            "it went to disk behind the reader"
        );

        dispatch::dispatch(&mut app, Command::FileSave);
        assert_eq!(std::fs::read_to_string(&path).expect("the file"), "two\n");
    }

    /// A file nobody has open has no document to go through, so it is
    /// written.
    #[test]
    fn a_file_that_is_not_open_is_written_to_disk() {
        let (scratch, mut app, _path) = reading("agent-closed", "one\n");
        let other = scratch.path().join("other.rs");
        assert!(asked(&mut app, &other, "made by the agent\n"));
        assert_eq!(
            std::fs::read_to_string(&other).expect("the new file"),
            "made by the agent\n"
        );
    }

    /// An agent inside a reader may change what the reader is looking at,
    /// and nothing else.
    #[test]
    fn a_file_outside_the_tree_is_refused() {
        let (_scratch, mut app, _path) = reading("agent-outside", "one\n");
        let elsewhere = std::env::temp_dir().join("obelus-agent-must-not-write.txt");
        let _ = std::fs::remove_file(&elsewhere);

        assert!(
            !asked(&mut app, &elsewhere, "no"),
            "a write outside the tree was allowed"
        );
        assert!(
            !elsewhere.exists(),
            "a file outside the tree was written anyway"
        );
    }
}
