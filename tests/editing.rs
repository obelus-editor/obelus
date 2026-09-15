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
        let _turn = support::clipboard_turn();
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
        let _turn = support::clipboard_turn();
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
        assert_eq!(
            buffer.on_disk(),
            obelus::buffer::Disk::Written,
            "the file moving was not noticed"
        );
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
    fn saving_over_a_file_that_moved_asks_first() {
        let (_scratch, mut app, path) = reading("save-overwrite", "mine\n");
        support::type_text(&mut app, "x");
        std::fs::write(&path, "somebody else's\n").expect("rewriting it");
        app.handle(obelus::event::Event::FileChanged { path: path.clone() });

        // It stops and asks, rather than writing over somebody else's file.
        dispatch::dispatch(&mut app, Command::FileSave);
        // On the screen, not merely in a field: a question nobody can read
        // is not a question.
        let dump = support::render(&mut app, 70, 12);
        assert!(
            support::text_block(&dump).contains("sample.rs changed on disk"),
            "the question does not say which file moved:\n{dump}"
        );
        assert_eq!(
            std::fs::read_to_string(&path).expect("reading it"),
            "somebody else's\n",
            "it wrote over somebody else's file instead of asking"
        );

        // Each way out says what it loses, because neither is the safe one.
        assert!(
            support::text_block(&dump).contains("loses what was written there"),
            "the ways out do not say what they lose:\n{dump}"
        );

        support::answer(&mut app, "save mine over it");
        assert_eq!(
            std::fs::read_to_string(&path).expect("reading it"),
            "xmine\n",
            "answering did not overwrite"
        );
    }

    /// obelus recognises the file it just wrote, so its own save arriving
    /// back through the watcher is answered by one `stat` rather than by
    /// reading the file again.
    #[test]
    fn a_save_leaves_the_file_recognisable() {
        let (_scratch, mut app, _path) = reading("save-recognise", "mine\n");
        support::type_text(&mut app, "x");
        dispatch::dispatch(&mut app, Command::FileSave);

        assert!(
            !app.current_buffer().expect("a buffer").file_touched(),
            "obelus does not recognise the file it just wrote"
        );
    }

    /// A reader may have four files called `mod.rs` open, and only one of
    /// them is the one about to be written over.
    #[test]
    fn the_conflict_says_which_file_it_is_about() {
        let scratch = support::Scratch::new("save-which");
        let inner = scratch.path().join("inner");
        std::fs::create_dir(&inner).expect("the directory");
        let path = inner.join("sample.rs");
        std::fs::write(&path, "mine\n").expect("the file");
        let mut app = App::new(vec![Buffer::open(&path).expect("opening it")]);
        app.working_directory_for_test(scratch.path().to_path_buf());
        support::lay_out(&mut app, 70, 12);
        support::type_text(&mut app, "x");
        std::fs::write(&path, "somebody else's\n").expect("rewriting it");
        app.handle(obelus::event::Event::FileChanged { path: path.clone() });

        dispatch::dispatch(&mut app, Command::FileSave);
        let dump = support::render(&mut app, 70, 12);
        assert!(
            support::said(&dump).contains("inner/sample.rs changed on disk"),
            "the question does not say which of the files called sample.rs:\n{dump}"
        );
    }

    /// A file somebody took away is a different question: there is nothing
    /// on disk to take instead, and nothing there to write *over*.
    #[test]
    fn saving_a_file_that_was_deleted_asks_a_different_question() {
        let (_scratch, mut app, path) = reading("save-deleted", "mine\n");
        support::type_text(&mut app, "x");
        std::fs::remove_file(&path).expect("deleting it");
        app.handle(obelus::event::Event::FileChanged { path: path.clone() });

        dispatch::dispatch(&mut app, Command::FileSave);
        let dump = support::render(&mut app, 70, 12);
        let text = support::text_block(&dump);
        assert!(
            text.contains("sample.rs was deleted"),
            "it did not say the file was deleted:\n{dump}"
        );
        let ways = support::ways(&app);
        assert!(
            !ways.iter().any(|way| way.contains("on disk")),
            "it offered to take what is on a disk with nothing on it: {ways:?}"
        );

        support::answer(&mut app, "write it back");
        assert_eq!(
            std::fs::read_to_string(&path).expect("reading it"),
            "xmine\n",
            "it did not put the file back"
        );
    }

    /// The other way out accepts the deletion and takes the document with it.
    #[test]
    fn a_deleted_file_can_be_let_go() {
        let (_scratch, mut app, path) = reading("save-letgo", "mine\n");
        support::type_text(&mut app, "x");
        std::fs::remove_file(&path).expect("deleting it");

        dispatch::dispatch(&mut app, Command::FileSave);
        support::answer(&mut app, "close it and let it go");

        assert!(app.current_buffer().is_none(), "it kept the document");
        assert!(
            !std::fs::exists(&path).expect("asking"),
            "letting it go put the file back"
        );
    }

    /// Nobody watched, nobody marked -- and the save still asks. The
    /// watcher is allowed to miss things; this is the place that is not.
    #[test]
    fn a_save_asks_disk_rather_than_the_mark() {
        let (_scratch, mut app, path) = reading("save-unwatched", "mine\n");
        support::type_text(&mut app, "x");
        // Written behind obelus's back, with no `FileChanged` fed in: this
        // is what an overflowed watcher queue or a filesystem that reports
        // nothing looks like from in here.
        std::fs::write(&path, "somebody else's\n").expect("rewriting it");

        dispatch::dispatch(&mut app, Command::FileSave);
        let dump = support::render(&mut app, 70, 12);
        assert!(
            support::text_block(&dump).contains("changed on disk"),
            "it wrote over a change nobody had told it about:\n{dump}"
        );
    }

    /// And a file that was touched without being changed is not a conflict.
    #[test]
    fn a_file_rewritten_with_what_it_had_is_not_a_conflict() {
        let (_scratch, mut app, path) = reading("save-same", "mine\n");
        support::type_text(&mut app, "x");
        // What a formatter that found nothing to change does, and what a
        // checkout of the commit the file was already on does.
        std::fs::write(&path, "mine\n").expect("rewriting it with what it had");
        app.handle(obelus::event::Event::FileChanged { path: path.clone() });

        dispatch::dispatch(&mut app, Command::FileSave);
        assert_eq!(
            std::fs::read_to_string(&path).expect("reading it"),
            "xmine\n",
            "it asked about a file whose bytes nobody changed"
        );
    }

    /// The other way out, which keeps the file and loses the edit.
    #[test]
    fn taking_what_is_on_disk_throws_the_edit_away() {
        let (_scratch, mut app, path) = reading("save-theirs", "mine\n");
        support::type_text(&mut app, "x");
        std::fs::write(&path, "somebody else's\n").expect("rewriting it");
        app.handle(obelus::event::Event::FileChanged { path: path.clone() });

        dispatch::dispatch(&mut app, Command::FileSave);
        support::answer(&mut app, "take what is on disk");

        let buffer = app.current_buffer().expect("a buffer");
        assert_eq!(
            buffer.text().rope().to_string(),
            "somebody else's\n",
            "it kept the edit it was told to throw away"
        );
        assert!(!buffer.is_dirty(), "it is still unwritten after re-reading");
        assert_eq!(
            std::fs::read_to_string(&path).expect("reading it"),
            "somebody else's\n",
            "re-reading wrote something"
        );
    }

    /// And it is a choice rather than an accident: the version it replaced
    /// is one undo away. A re-read would have forgotten it.
    #[test]
    fn taking_what_is_on_disk_can_be_undone() {
        let (_scratch, mut app, path) = reading("save-theirs-undo", "mine\n");
        support::type_text(&mut app, "x");
        std::fs::write(&path, "somebody else's\n").expect("rewriting it");
        app.handle(obelus::event::Event::FileChanged { path: path.clone() });

        dispatch::dispatch(&mut app, Command::FileSave);
        support::answer(&mut app, "take what is on disk");
        assert!(
            !app.current_buffer().expect("a buffer").is_dirty(),
            "what came off disk was called unwritten"
        );

        dispatch::dispatch(&mut app, Command::Undo);
        let buffer = app.current_buffer().expect("a buffer");
        assert_eq!(
            buffer.text().rope().to_string(),
            "xmine\n",
            "undo did not bring back the version that was replaced"
        );
        assert!(
            buffer.is_dirty(),
            "the version that is not on disk was called written"
        );
    }

    /// Cancelling keeps both versions, which is the point of offering it.
    #[test]
    fn cancelling_keeps_the_edit_and_the_file() {
        let (_scratch, mut app, path) = reading("save-cancel", "mine\n");
        support::type_text(&mut app, "x");
        std::fs::write(&path, "somebody else's\n").expect("rewriting it");
        app.handle(obelus::event::Event::FileChanged { path: path.clone() });

        dispatch::dispatch(&mut app, Command::FileSave);
        support::answer(&mut app, "cancel");

        assert_eq!(
            std::fs::read_to_string(&path).expect("reading it"),
            "somebody else's\n",
            "cancelling wrote the file anyway"
        );
        let buffer = app.current_buffer().expect("a buffer");
        assert_eq!(buffer.text().rope().to_string(), "xmine\n");
        assert!(buffer.is_dirty(), "cancelling lost the edit");

        // And it asks again next time, because nothing was settled.
        dispatch::dispatch(&mut app, Command::FileSave);
        assert!(
            support::ways(&app)
                .iter()
                .any(|way| way == "save mine over it"),
            "the second save went through without asking"
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
        assert_eq!(
            buffer.on_disk(),
            obelus::buffer::Disk::Unchanged,
            "the file is still said to have moved"
        );
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

    /// And it has to be visible. The mark spent its first day in the
    /// gutter's grey -- the colour of a line number, picked to recede -- and
    /// a reader looking straight at the list did not see it.
    #[test]
    fn the_mark_is_the_one_the_status_row_uses() {
        let scratch = support::Scratch::new("say-list-golden");
        let one = scratch.path().join("written.rs");
        let two = scratch.path().join("unwritten.rs");
        std::fs::write(&one, "fn one() {}\n").expect("the first file");
        std::fs::write(&two, "fn two() {}\n").expect("the second file");
        let mut app = App::new(vec![Buffer::open(&one).expect("opening the first")]);
        app.working_directory_for_test(scratch.path().to_path_buf());
        support::lay_out(&mut app, 60, 12);
        app.open_buffer_for_test(Buffer::open(&two).expect("opening the second"));
        support::lay_out(&mut app, 60, 12);
        support::type_text(&mut app, "x");

        dispatch::dispatch(&mut app, Command::BufferList);
        let dump = support::render(&mut app, 60, 12);

        // The mark has a column of its own, kept on the rows that have
        // nothing to put in it: a name that sat two columns right of its
        // neighbours because that file is unwritten says the same thing
        // twice, in a way that makes the list harder to read down.
        // In characters, not bytes: the mark is a multi-byte glyph, and a
        // byte offset would call the row it is on two columns wider than it
        // is drawn.
        let column = |name: &str| {
            support::text_block(&dump)
                .lines()
                .find_map(|line| line.find(name).map(|at| line[..at].chars().count()))
                .unwrap_or_else(|| panic!("no row for {name} in:\n{dump}"))
        };
        assert_eq!(
            column("written.rs"),
            column("unwritten.rs"),
            "the marked row's name does not line up with the others:\n{dump}"
        );

        support::check("buffers_unsaved_60x12", &dump);
    }

    #[test]
    fn leaving_with_something_unwritten_asks_first() {
        let (_scratch, mut app, _path) = reading("say-quit", "fn main() {}\n");
        support::type_text(&mut app, "x");

        dispatch::dispatch(&mut app, Command::Quit);
        assert!(!app.should_quit(), "it left with an unwritten document");
        // Which one. With a single file there is room to say, and the
        // reader is about to decide whether to write it.
        let dump = support::render(&mut app, 70, 12);
        assert!(
            support::said(&dump).contains("sample.rs is unsaved"),
            "it did not say which file leaving would lose:\n{dump}"
        );

        support::answer(&mut app, "leave without saving");
        assert!(app.should_quit(), "answering did not leave");
    }

    /// The other way out writes everything first.
    #[test]
    fn leaving_can_write_everything_on_the_way_out() {
        let scratch = support::Scratch::new("say-quit-save");
        let one = scratch.path().join("one.rs");
        let two = scratch.path().join("two.rs");
        std::fs::write(&one, "fn one() {}\n").expect("the first file");
        std::fs::write(&two, "fn two() {}\n").expect("the second file");
        let mut app = App::new(vec![Buffer::open(&one).expect("opening the first")]);
        app.working_directory_for_test(scratch.path().to_path_buf());
        support::lay_out(&mut app, 70, 12);
        support::type_text(&mut app, "x");

        app.open_buffer_for_test(Buffer::open(&two).expect("opening the second"));
        support::lay_out(&mut app, 70, 12);
        support::type_text(&mut app, "y");

        dispatch::dispatch(&mut app, Command::Quit);
        // And with more than one, the count: naming them would be a list
        // nobody reads before pressing enter.
        let dump = support::render(&mut app, 70, 12);
        let said = support::said(&dump);
        assert!(
            said.contains("2 files are unsaved"),
            "it did not say how many files leaving would lose:\n{dump}"
        );
        assert!(
            !said.contains("one.rs"),
            "it named the files instead of counting them:\n{dump}"
        );

        support::answer(&mut app, "save everything and leave");

        assert!(app.should_quit(), "it wrote everything and then stayed");
        assert_eq!(
            std::fs::read_to_string(&one).expect("reading the first"),
            "xfn one() {}\n",
            "the file that was not in front of the reader was not written"
        );
        assert_eq!(
            std::fs::read_to_string(&two).expect("reading the second"),
            "yfn two() {}\n"
        );
    }

    /// A save that failed is the whole reason for asking: leaving anyway
    /// would throw away exactly what the reader just said to keep.
    #[test]
    fn a_save_that_fails_on_the_way_out_stays() {
        let scratch = support::Scratch::new("say-quit-fail");
        let gone = scratch.path().join("gone");
        std::fs::create_dir(&gone).expect("the directory");
        let path = gone.join("sample.rs");
        std::fs::write(&path, "fn main() {}\n").expect("the file");
        let mut app = App::new(vec![Buffer::open(&path).expect("opening it")]);
        app.working_directory_for_test(scratch.path().to_path_buf());
        support::lay_out(&mut app, 70, 12);
        support::type_text(&mut app, "x");
        std::fs::remove_dir_all(&gone).expect("taking the directory away");

        dispatch::dispatch(&mut app, Command::Quit);
        support::answer(&mut app, "save everything and leave");

        assert!(
            !app.should_quit(),
            "it left with the save it promised undone"
        );
        let dump = support::render(&mut app, 70, 12);
        assert!(
            support::text_block(&dump).contains("sample.rs: not saved"),
            "it did not say which file would not go:\n{dump}"
        );
    }

    /// Cancelling stays, and the next attempt asks again: there is no
    /// remembered warning to spend.
    #[test]
    fn cancelling_stays_and_asks_again() {
        let (_scratch, mut app, _path) = reading("say-again", "fn main() {}\n");
        support::type_text(&mut app, "x");

        dispatch::dispatch(&mut app, Command::Quit);
        support::answer(&mut app, "cancel");
        assert!(!app.should_quit(), "cancelling left anyway");

        dispatch::dispatch(&mut app, Command::Quit);
        assert!(
            support::ways(&app)
                .iter()
                .any(|way| way == "leave without saving"),
            "the second attempt left without asking"
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

/// Whether a document differs from what is on disk is a question about
/// where it is in its own history, not about whether anybody has typed.
mod unwritten {
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

    fn unwritten(app: &App) -> bool {
        app.current_buffer().expect("a buffer").is_dirty()
    }

    #[test]
    fn undoing_back_to_what_is_on_disk_is_not_unwritten() {
        let (_scratch, mut app, _path) = reading("undo-clean", "fn main() {}\n");
        support::type_text(&mut app, "x");
        assert!(unwritten(&app), "typing did not make it unwritten");

        dispatch::dispatch(&mut app, Command::Undo);
        assert!(
            !unwritten(&app),
            "it is still marked unwritten after being put back to what is on disk"
        );
    }

    /// Typing something and taking it straight back out leaves the file it
    /// was: the journal has moved on, and the bytes have not.
    ///
    /// The journal alone says otherwise -- a character typed and a
    /// character deleted are two steps forward, not a step back -- so where
    /// the document is the length of the one on disk, the bytes themselves
    /// settle it.
    #[test]
    fn typing_and_deleting_leaves_the_file_as_it_was() {
        let (_scratch, mut app, _path) = reading("undo-balanced", "fn main() {}\n");
        support::type_text(&mut app, "x");
        assert!(unwritten(&app), "typing did not make it unwritten");

        support::press(&mut app, crossterm::event::KeyCode::Backspace);
        assert!(
            !unwritten(&app),
            "the character was taken back out and the file is still called unwritten"
        );
    }

    /// And taking out a *different* character does not: the bytes are what
    /// is asked, not how many of them there are.
    #[test]
    fn deleting_something_else_of_the_same_length_is_still_unwritten() {
        let (_scratch, mut app, _path) = reading("undo-swapped", "fn main() {}\n");
        support::press(&mut app, crossterm::event::KeyCode::End);
        support::type_text(&mut app, "x");
        support::press(&mut app, crossterm::event::KeyCode::Left);
        support::press(&mut app, crossterm::event::KeyCode::Left);
        support::press(&mut app, crossterm::event::KeyCode::Backspace);
        assert_eq!(
            app.current_buffer()
                .expect("a buffer")
                .text()
                .rope()
                .to_string(),
            "fn main() }x\n",
            "not the edit this test meant to make"
        );
        assert!(
            unwritten(&app),
            "a document the same length as the file was taken for the file"
        );
    }

    /// However many it takes. A flag that an edit set and one undo cleared
    /// would call a half-undone document written.
    #[test]
    fn it_takes_as_many_undos_as_it_took_edits() {
        let (_scratch, mut app, _path) = reading("undo-several", "fn main() {}\n");
        support::type_text(&mut app, "one");
        support::press(&mut app, crossterm::event::KeyCode::Enter);
        support::type_text(&mut app, "two");

        dispatch::dispatch(&mut app, Command::Undo);
        assert!(unwritten(&app), "one undo of three edits called it written");
        dispatch::dispatch(&mut app, Command::Undo);
        assert!(
            unwritten(&app),
            "two undos of three edits called it written"
        );
        dispatch::dispatch(&mut app, Command::Undo);
        assert!(!unwritten(&app), "undoing all of it left it unwritten");
    }

    /// Redoing puts it back to a document that is not on disk.
    #[test]
    fn redoing_makes_it_unwritten_again() {
        let (_scratch, mut app, _path) = reading("undo-redo", "fn main() {}\n");
        support::type_text(&mut app, "x");
        dispatch::dispatch(&mut app, Command::Undo);
        dispatch::dispatch(&mut app, Command::Redo);
        assert!(
            unwritten(&app),
            "redoing the edit did not make it unwritten"
        );
    }

    /// Saving moves the mark: what is on disk is what is in front of the
    /// reader, and undoing past *that* is unwritten again.
    #[test]
    fn saving_moves_where_the_document_counts_as_written() {
        let (_scratch, mut app, _path) = reading("undo-saved", "fn main() {}\n");
        support::type_text(&mut app, "x");
        dispatch::dispatch(&mut app, Command::FileSave);
        assert!(!unwritten(&app), "saving did not settle it");

        dispatch::dispatch(&mut app, Command::Undo);
        assert!(
            unwritten(&app),
            "undoing past what was written left it counted as written"
        );

        dispatch::dispatch(&mut app, Command::Redo);
        assert!(
            !unwritten(&app),
            "coming back to what was written did not settle it"
        );
    }

    /// New work after an undo is not the work that was written, even where
    /// it leaves the history the same length.
    #[test]
    fn work_done_after_an_undo_is_not_the_work_that_was_written() {
        let (_scratch, mut app, _path) = reading("undo-branch", "fn main() {}\n");
        support::type_text(&mut app, "x");
        dispatch::dispatch(&mut app, Command::FileSave);

        dispatch::dispatch(&mut app, Command::Undo);
        support::type_text(&mut app, "y");
        assert!(
            unwritten(&app),
            "a document with different text than disk was called written"
        );
    }
}

/// Closing a document with something unwritten in it.
///
/// Its own question because a closed buffer takes its undo with it: there
/// is no other way back to what was in it.
mod closing {
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

    /// Above the ways out, not under them: the prompt row sits below the
    /// rows, and a question found there has been read after its answers.
    #[test]
    fn the_question_is_read_before_its_answers() {
        let (_scratch, mut app, _path) = reading("close-order", "fn main() {}\n");
        support::type_text(&mut app, "x");
        dispatch::dispatch(&mut app, Command::BufferClose);

        let dump = support::render(&mut app, 70, 12);
        let text = support::text_block(&dump);
        let row = |what: &str| {
            text.lines()
                .position(|line| line.contains(what))
                .unwrap_or_else(|| panic!("no row saying {what:?} in:\n{dump}"))
        };
        assert!(
            row("is unsaved") < row("save and close it"),
            "the question is drawn under the answers to it:\n{dump}"
        );
    }

    #[test]
    fn closing_something_unwritten_asks_first() {
        let (_scratch, mut app, _path) = reading("close-ask", "fn main() {}\n");
        support::type_text(&mut app, "x");

        dispatch::dispatch(&mut app, Command::BufferClose);
        assert!(
            app.current_buffer().is_some(),
            "it closed an unwritten document without asking"
        );
        let dump = support::render(&mut app, 70, 12);
        let text = support::text_block(&dump);
        assert!(
            text.contains("sample.rs is unsaved"),
            "the question does not name the file closing would lose:\n{dump}"
        );
    }

    #[test]
    fn closing_can_write_it_first() {
        let (_scratch, mut app, path) = reading("close-save", "fn main() {}\n");
        support::type_text(&mut app, "x");

        dispatch::dispatch(&mut app, Command::BufferClose);
        support::answer(&mut app, "save and close it");

        assert_eq!(
            std::fs::read_to_string(&path).expect("reading it"),
            "xfn main() {}\n",
            "it closed without writing what it said it would write"
        );
        assert!(
            app.current_buffer().is_none(),
            "it wrote it and then stayed"
        );
    }

    #[test]
    fn closing_can_throw_it_away() {
        let (_scratch, mut app, path) = reading("close-discard", "fn main() {}\n");
        support::type_text(&mut app, "x");

        dispatch::dispatch(&mut app, Command::BufferClose);
        support::answer(&mut app, "close it without saving");

        assert!(app.current_buffer().is_none(), "it did not close");
        assert_eq!(
            std::fs::read_to_string(&path).expect("reading it"),
            "fn main() {}\n",
            "it wrote the file it was told to throw away"
        );
    }

    /// A save that did not happen leaves the document open. Closing on the
    /// strength of a write that failed loses exactly what the reader chose
    /// to keep.
    #[test]
    fn closing_stays_open_when_the_write_does_not_go() {
        let scratch = support::Scratch::new("close-fail");
        let gone = scratch.path().join("gone");
        std::fs::create_dir(&gone).expect("the directory");
        let path = gone.join("sample.rs");
        std::fs::write(&path, "fn main() {}\n").expect("the file");
        let mut app = App::new(vec![Buffer::open(&path).expect("opening it")]);
        app.working_directory_for_test(scratch.path().to_path_buf());
        support::lay_out(&mut app, 70, 12);
        support::type_text(&mut app, "x");
        std::fs::remove_dir_all(&gone).expect("taking the directory away");

        dispatch::dispatch(&mut app, Command::BufferClose);
        support::answer(&mut app, "save and close it");

        assert!(
            app.current_buffer().is_some(),
            "it closed a document whose save failed, losing the edit"
        );
    }

    /// A document with nothing unwritten in it closes on the key, with no
    /// question in the way.
    #[test]
    fn closing_something_written_asks_nothing() {
        let (_scratch, mut app, _path) = reading("close-clean", "fn main() {}\n");
        dispatch::dispatch(&mut app, Command::BufferClose);
        assert!(
            app.current_buffer().is_none(),
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

    /// And the caret stays on that row rather than landing on the next one.
    ///
    /// Past the last character of a wrapped row is where the row below
    /// begins: one place with two names, which the text has no way to tell
    /// apart. So `end` stops in front of that character, which is a place
    /// on the row the key was pressed on and nowhere else. Zed clips its
    /// own `line_end` back by a column for the same reason.
    #[test]
    fn end_leaves_the_caret_on_the_row_it_was_pressed_on() {
        let (_scratch, mut app) =
            reading("move-wrap-caret", "alpha beta gamma delta epsilon\n", 16);
        app.configure(
            obelus::config::Config {
                wrap: true,
                ..obelus::config::Config::default()
            },
            Vec::new(),
        );
        let row_of = |app: &mut App| {
            let dump = support::render(app, 16, 12);
            support::cursor_line(&dump)
                .split_once(',')
                .map(|(_, row)| row.parse::<u16>().expect("a row"))
                .unwrap_or_else(|| panic!("the caret is not on screen:\n{dump}"))
        };
        assert_eq!(row_of(&mut app), 0, "not where this test meant to start");

        support::press(&mut app, KeyCode::End);
        assert_eq!(
            row_of(&mut app),
            0,
            "end sent the caret to the start of the row below"
        );
        // In front of the row's last character, which is as far along the
        // row as there is an unambiguous place to be.
        assert_eq!(at(&app).1, 5, "end did not reach the end of the row");

        // And down from there stays on the row below rather than falling
        // through it.
        support::press(&mut app, KeyCode::Down);
        assert_eq!(
            row_of(&mut app),
            1,
            "down from the end of a row skipped one"
        );

        // And home from there is the start of that row.
        support::press(&mut app, KeyCode::Home);
        assert_eq!(
            row_of(&mut app),
            1,
            "home left the caret at the end of the row above"
        );
    }

    /// A row with nowhere to break fills the screen, and then the place
    /// past its last character has no cell at all: a terminal draws its
    /// cursor in a cell, and out past the last column there is none. The
    /// same rule covers it -- stop in front of that character -- and what
    /// makes it worth its own test is that claiming the place left the
    /// caret undrawn, which a reader sees as the cursor vanishing.
    #[test]
    fn end_of_a_row_that_fills_the_screen_stays_on_screen() {
        let (_scratch, mut app) = reading(
            "move-wrap-full",
            "abcdefghijklmnopqrstuvwxyz0123456789\n",
            16,
        );
        app.configure(
            obelus::config::Config {
                wrap: true,
                ..obelus::config::Config::default()
            },
            Vec::new(),
        );
        support::render(&mut app, 16, 12);

        support::press(&mut app, KeyCode::End);
        let dump = support::render(&mut app, 16, 12);
        // "none" is what the dump records for a caret the view could not
        // place, which is what a reader sees as it vanishing.
        assert_ne!(
            support::cursor_line(&dump),
            "none",
            "the caret is not on screen at all:\n{dump}"
        );
        // On the row it was pressed on.
        let rows = support::text_block(&dump);
        let caret = support::cursor_line(&dump)
            .split_once(',')
            .map(|(_, row)| row.parse::<u16>().expect("a row"))
            .expect("the caret is on screen");
        assert_eq!(
            caret, 0,
            "the caret left the row end was pressed on:\n{rows}"
        );
        // In front of its last character, which is as far as the row goes.
        assert_eq!(at(&app).1, 9, "end did not reach the end of the row");
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

/// Laying a file out before writing it.
mod formatting {
    use obelus::lsp::action;

    /// A server saying "nothing to change" is not a server failing, and a
    /// layout obelus cannot read is not half a layout to apply.
    #[test]
    fn only_a_layout_that_can_be_followed_is_followed() {
        assert!(
            action::edits_in(None).is_none(),
            "a reply that failed was read as a layout"
        );
        assert!(
            action::edits_in(Some(serde_json::Value::Null)).is_none(),
            "null is a server with nothing to change"
        );
        assert!(
            action::edits_in(Some(serde_json::json!([]))).is_none(),
            "an empty list is nothing to change"
        );
        assert!(
            action::edits_in(Some(serde_json::json!({ "not": "edits" }))).is_none(),
            "something obelus cannot read was taken for a layout"
        );

        let edits = action::edits_in(Some(serde_json::json!([{
            "range": {
                "start": { "line": 0, "character": 0 },
                "end": { "line": 0, "character": 4 },
            },
            "newText": "  ",
        }])))
        .expect("a layout");
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, "  ");
    }
}

/// Saving with formatting turned on, when nobody can format.
mod format_on_save {
    use obelus::{
        app::App,
        buffer::Buffer,
        command::{Command, dispatch},
    };

    use super::support;

    /// A setting the reader turned on is not a reason to refuse them. With
    /// no server to ask, the file is written as it is rather than waiting
    /// for an answer that is not coming.
    #[test]
    fn a_file_nobody_can_lay_out_is_still_written() {
        let scratch = support::Scratch::new("format-none");
        // An extension no language server obelus knows is started for.
        let path = scratch.path().join("sample.unknownlang");
        std::fs::write(&path, "one\n").expect("writing it");
        let mut app = App::new(vec![Buffer::open(&path).expect("opening it")]);
        app.working_directory_for_test(scratch.path().to_path_buf());
        app.configure(
            obelus::config::Config {
                format_on_save: true,
                ..obelus::config::Config::default()
            },
            Vec::new(),
        );
        support::lay_out(&mut app, 70, 12);

        support::type_text(&mut app, "x");
        dispatch::dispatch(&mut app, Command::FileSave);

        assert_eq!(
            std::fs::read_to_string(&path).expect("reading it back"),
            "xone\n",
            "a file nobody could lay out was never written"
        );
        assert!(!app.current_buffer().expect("a buffer").is_dirty());
    }
}

/// One step in, and one step back out.
mod indenting_a_block {
    use crossterm::event::KeyCode;
    use obelus::{app::App, buffer::Buffer, coordinates::LineNumber};

    use super::support;

    fn editing(name: &str, file: &str, contents: &str) -> (support::Scratch, App) {
        let scratch = support::Scratch::new(name);
        let path = scratch.path().join(file);
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

    /// The whole of the reason this exists: `tab` used to put the indent in
    /// place of the selection, which is the one thing nobody means by it.
    #[test]
    fn tab_over_a_selection_moves_the_lines_in() {
        let (_scratch, mut app) = editing(
            "indent-in",
            "sample.rs",
            "fn a() {\nlet one = 1;\nlet two = 2;\n}\n",
        );
        support::press(&mut app, KeyCode::Down);
        support::press_shift(&mut app, KeyCode::Down);
        support::press(&mut app, KeyCode::Tab);

        assert_eq!(
            text(&app),
            "fn a() {\n    let one = 1;\n    let two = 2;\n}\n",
            "the selected lines did not move in"
        );
    }

    /// And the selection is still over them, so the next press is about the
    /// same lines.
    #[test]
    fn the_lines_stay_selected() {
        let (_scratch, mut app) = editing("indent-again", "sample.rs", "a\nb\nc\n");
        support::press_shift(&mut app, KeyCode::Down);
        support::press(&mut app, KeyCode::Tab);
        support::press(&mut app, KeyCode::Tab);
        assert_eq!(text(&app), "        a\n        b\nc\n");
    }

    /// Back out again, and no further than the margin.
    #[test]
    fn shift_tab_moves_them_back_out() {
        let (_scratch, mut app) = editing(
            "indent-out",
            "sample.rs",
            "fn a() {\n    let one = 1;\n  let two = 2;\n}\n",
        );
        support::press(&mut app, KeyCode::Down);
        support::press_shift(&mut app, KeyCode::Down);
        support::press_shift(&mut app, KeyCode::BackTab);

        // The second line is indented by two in a file of four, so it comes
        // out at the margin rather than staying where it is.
        assert_eq!(text(&app), "fn a() {\nlet one = 1;\nlet two = 2;\n}\n");
    }

    /// With nothing selected it is the line the cursor is on, which is what
    /// makes `shift+tab` worth having on its own.
    #[test]
    fn shift_tab_with_nothing_selected_is_this_line() {
        let (_scratch, mut app) = editing("indent-one", "sample.rs", "fn a() {\n    one();\n}\n");
        support::press(&mut app, KeyCode::Down);
        support::press_shift(&mut app, KeyCode::BackTab);
        assert_eq!(text(&app), "fn a() {\none();\n}\n");
    }

    /// A line with nothing on it gets nothing put in front of it: an indent
    /// there is trailing blanks, which every other tool takes back out.
    #[test]
    fn an_empty_line_is_left_alone() {
        let (_scratch, mut app) = editing("indent-empty", "sample.rs", "a\n\nb\n");
        support::press_shift(&mut app, KeyCode::Down);
        support::press_shift(&mut app, KeyCode::Down);
        support::press(&mut app, KeyCode::Tab);
        assert_eq!(text(&app), "    a\n\n    b\n");
    }

    /// One change, not one per line: a block put right takes one `ctrl+z`.
    #[test]
    fn the_whole_block_is_one_step_back() {
        let (_scratch, mut app) = editing("indent-undo", "sample.rs", "a\nb\nc\n");
        support::press_shift(&mut app, KeyCode::Down);
        support::press_shift(&mut app, KeyCode::Down);
        support::press(&mut app, KeyCode::Tab);
        obelus::command::dispatch::dispatch(&mut app, obelus::command::Command::Undo);
        assert_eq!(text(&app), "a\nb\nc\n");
    }

    /// What a step of indentation is belongs to the file: a file indented
    /// with tabs is a file a reader means to keep indenting with tabs.
    #[test]
    fn a_file_indented_with_tabs_gets_tabs() {
        let (_scratch, mut app) = editing("indent-tabs", "sample.go", "func a() {\n\tone()\n}\n");
        support::press(&mut app, KeyCode::Down);
        support::press_shift(&mut app, KeyCode::Down);
        support::press(&mut app, KeyCode::Tab);
        assert_eq!(text(&app), "func a() {\n\t\tone()\n\t}\n");

        // And so does the tab key with nothing selected.
        support::press(&mut app, KeyCode::Down);
        support::press(&mut app, KeyCode::Down);
        support::press(&mut app, KeyCode::Tab);
        assert!(
            text(&app).ends_with('\t'),
            "the tab key put spaces into a file indented with tabs: {:?}",
            text(&app)
        );
    }

    /// And a file indented with spaces keeps its spaces, however wide.
    #[test]
    fn a_file_indented_with_spaces_gets_spaces() {
        let (_scratch, mut app) =
            editing("indent-spaces", "sample.rs", "fn a() {\n    one();\n}\n");
        support::press(&mut app, KeyCode::Down);
        support::press(&mut app, KeyCode::End);
        support::press(&mut app, KeyCode::Tab);
        let line = app
            .current_buffer()
            .expect("a buffer")
            .text()
            .line(LineNumber::new(1))
            .to_string();
        assert_eq!(line, "    one();    ");
    }
}

/// A word at a time, and a line at a time: the two things `ctrl` already
/// meant for moving, meant for taking out and taking a copy as well.
mod by_the_word_and_the_line {
    use crossterm::event::KeyCode;
    use obelus::{
        app::App,
        buffer::Buffer,
        clipboard,
        command::{Command, dispatch},
    };

    use super::support;

    fn editing(name: &str, contents: &str) -> (support::Scratch, App) {
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

    /// `ctrl` with the arrow keys already steps over a word. A reader who
    /// can step over one expects to be able to take it out.
    #[test]
    fn control_backspace_takes_out_the_word_behind() {
        let (_scratch, mut app) = editing("word-back", "let greeting = hello;\n");
        support::press(&mut app, KeyCode::End);
        support::press_control_key(&mut app, KeyCode::Backspace);
        assert_eq!(text(&app), "let greeting = hello\n");
        support::press_control_key(&mut app, KeyCode::Backspace);
        assert_eq!(text(&app), "let greeting = \n");
    }

    #[test]
    fn control_delete_takes_out_the_word_in_front() {
        let (_scratch, mut app) = editing("word-forward", "let greeting = hello;\n");
        support::press_control_key(&mut app, KeyCode::Delete);
        assert_eq!(text(&app), " greeting = hello;\n");
    }

    /// And at the ends of the document there is no word to take, so nothing
    /// happens rather than something odd.
    #[test]
    fn there_is_nothing_behind_the_start_of_the_document() {
        let (_scratch, mut app) = editing("word-edges", "one two\n");
        support::press_control_key(&mut app, KeyCode::Backspace);
        assert_eq!(text(&app), "one two\n");
        assert!(
            !app.current_buffer().expect("a buffer").is_dirty(),
            "a key with nothing to do made the file unsaved"
        );
    }

    /// Copying with nothing selected copies the line, newline and all, so
    /// that what comes back out is a line rather than the middle of one.
    #[test]
    fn copying_nothing_copies_the_line() {
        let _turn = support::clipboard_turn();
        clipboard::use_provider_for_test(clipboard::Provider::Kept);
        let (_scratch, mut app) = editing("line-copy", "one\ntwo\nthree\n");
        support::press(&mut app, KeyCode::Down);
        dispatch::dispatch(&mut app, Command::SelectionCopy);
        assert_eq!(clipboard::paste().as_deref(), Some("two\n"));
        assert_eq!(text(&app), "one\ntwo\nthree\n", "copying changed the file");
    }

    /// And cutting takes the line away rather than leaving a blank where it
    /// was.
    #[test]
    fn cutting_nothing_cuts_the_line_away() {
        let _turn = support::clipboard_turn();
        clipboard::use_provider_for_test(clipboard::Provider::Kept);
        let (_scratch, mut app) = editing("line-cut", "one\ntwo\nthree\n");
        support::press(&mut app, KeyCode::Down);
        dispatch::dispatch(&mut app, Command::SelectionCut);
        assert_eq!(text(&app), "one\nthree\n");
        assert_eq!(clipboard::paste().as_deref(), Some("two\n"));
    }

    /// A cut line and a paste put it back where it came from, which is what
    /// makes the pair of them a way to move a line.
    #[test]
    fn a_cut_line_pastes_back_as_a_line() {
        let _turn = support::clipboard_turn();
        clipboard::use_provider_for_test(clipboard::Provider::Kept);
        let (_scratch, mut app) = editing("line-move", "one\ntwo\nthree\n");
        support::press(&mut app, KeyCode::Down);
        dispatch::dispatch(&mut app, Command::SelectionCut);
        dispatch::dispatch(&mut app, Command::Paste);
        assert_eq!(text(&app), "one\ntwo\nthree\n");
    }

    /// The last line of a file has no line break after it to take.
    #[test]
    fn cutting_the_last_line_takes_what_there_is() {
        let _turn = support::clipboard_turn();
        clipboard::use_provider_for_test(clipboard::Provider::Kept);
        let (_scratch, mut app) = editing("line-last", "one\ntwo");
        support::press(&mut app, KeyCode::Down);
        dispatch::dispatch(&mut app, Command::SelectionCut);
        assert_eq!(text(&app), "one\n");
    }
}

/// Where the reader has been is a set of line numbers in a document, and an
/// edit moves some of them.
mod going_back {
    use crossterm::event::KeyCode;
    use obelus::{
        app::App,
        buffer::Buffer,
        command::{Command, dispatch},
    };

    use super::support;

    fn editing(name: &str, contents: &str) -> (support::Scratch, App) {
        let scratch = support::Scratch::new(name);
        let path = scratch.path().join("sample.rs");
        std::fs::write(&path, contents).expect("writing the file");
        let mut app = App::new(vec![Buffer::open(&path).expect("opening it")]);
        app.working_directory_for_test(scratch.path().to_path_buf());
        support::lay_out(&mut app, 60, 12);
        (scratch, app)
    }

    /// Ten lines, each saying which it is, so that where a jump lands is
    /// readable rather than counted.
    fn numbered() -> String {
        (0..10).map(|line| format!("line {line}\n")).collect()
    }

    fn at(app: &App) -> usize {
        app.current_buffer().expect("a buffer").cursor().line.get()
    }

    /// Through the key a reader would use: the leap is what the history is
    /// for, and `go-to-line` is the plainest one there is.
    fn leap_to(app: &mut App, line: usize) {
        dispatch::dispatch(app, Command::GoLine);
        support::type_text(app, &(line + 1).to_string());
        support::press(app, KeyCode::Enter);
        assert_eq!(at(app), line, "the leap did not go where it was told");
    }

    #[test]
    fn a_place_below_an_edit_moves_with_it() {
        let (_scratch, mut app) = editing("jump-below", &numbered());
        leap_to(&mut app, 8);
        // Back to the top, and two lines put in above where the jump was.
        dispatch::dispatch(&mut app, Command::GoBack);
        assert_eq!(at(&app), 0, "not where this test meant to start");
        support::press(&mut app, KeyCode::Enter);
        support::press(&mut app, KeyCode::Enter);

        dispatch::dispatch(&mut app, Command::GoForward);
        assert_eq!(
            at(&app),
            10,
            "going forward landed on the line the place used to be on"
        );
    }

    /// And one above it stays where it is.
    #[test]
    fn a_place_above_an_edit_stays_put() {
        let (_scratch, mut app) = editing("jump-above", &numbered());
        leap_to(&mut app, 2);
        leap_to(&mut app, 8);
        // An edit below the first place, which must not move it.
        support::press(&mut app, KeyCode::Enter);

        dispatch::dispatch(&mut app, Command::GoBack);
        assert_eq!(at(&app), 2, "a place above the edit moved anyway");
    }

    /// A place on a line the edit took away is a place that is not there
    /// any more, and the nearest thing left to it is where the edit began.
    #[test]
    fn a_place_inside_an_edit_lands_where_it_began() {
        let (_scratch, mut app) = editing("jump-inside", &numbered());
        leap_to(&mut app, 5);
        dispatch::dispatch(&mut app, Command::GoBack);

        // Take lines three to seven away in one edit.
        for _ in 0..3 {
            support::press(&mut app, KeyCode::Down);
        }
        for _ in 0..4 {
            support::press_shift(&mut app, KeyCode::Down);
        }
        support::press(&mut app, KeyCode::Backspace);

        dispatch::dispatch(&mut app, Command::GoForward);
        assert_eq!(at(&app), 3, "the place did not land where the edit began");
    }
}
