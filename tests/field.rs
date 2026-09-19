//! One line with a caret in it.
//!
//! What the three short-question boxes could not do before: reach the
//! middle of what was typed. The keys are the file's keys, because what is
//! in the line is what is in the file -- so these are about the line's own
//! rules, which are the part that is not the file's.

mod support;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use obelus::component::field::Field;

fn press(field: &mut Field, code: KeyCode) -> bool {
    field.handle_key(&KeyEvent::new(code, KeyModifiers::NONE))
}

fn with(field: &mut Field, code: KeyCode, modifiers: KeyModifiers) -> bool {
    field.handle_key(&KeyEvent::new(code, modifiers))
}

fn type_in(field: &mut Field, said: &str) {
    for character in said.chars() {
        press(field, KeyCode::Char(character));
    }
}

/// The caret reaches the middle, which is the whole of what was missing:
/// a name that starts in the box is changed by editing it, not by deleting
/// back to the letter that is wrong.
#[test]
fn the_caret_reaches_the_middle() {
    let mut field = Field::about("working_directory", obelus::component::field::anything);
    assert_eq!(field.said(), "working_directory");
    // The caret starts after it, so a reader who meant to add something
    // types straight away.
    assert_eq!(field.caret().get(), "working_directory".chars().count());

    // Into the middle, a character at a time, and a letter goes in there
    // rather than at the end.
    for _ in 0.."directory".chars().count() {
        assert!(press(&mut field, KeyCode::Left));
    }
    type_in(&mut field, "sub_");
    assert_eq!(field.said(), "working_sub_directory");

    // Home reaches the front, which `push` and `pop` never could.
    assert!(press(&mut field, KeyCode::Home));
    assert_eq!(field.caret().get(), 0);
    type_in(&mut field, "the_");
    assert_eq!(field.said(), "the_working_sub_directory");
}

/// A word at a time, which is the other thing `ctrl` and an arrow mean
/// everywhere a reader has been -- and the reason the line uses the file's
/// own machinery rather than a rule of its own.
#[test]
fn the_caret_walks_words() {
    let mut field = Field::about("one two three", obelus::component::field::anything);
    assert!(with(&mut field, KeyCode::Left, KeyModifiers::CONTROL));
    assert_eq!(field.caret().get(), "one two ".chars().count());

    // And `ctrl+backspace` takes the word in front of it.
    assert!(with(&mut field, KeyCode::Backspace, KeyModifiers::CONTROL));
    assert_eq!(field.said(), "one three");
}

/// What is held, and that typing over it replaces it -- the same rule the
/// file follows, because it is the file's own machinery underneath.
#[test]
fn a_run_can_be_held_and_typed_over() {
    let mut field = Field::new();
    type_in(&mut field, "hello world");
    for _ in 0..5 {
        assert!(with(&mut field, KeyCode::Left, KeyModifiers::SHIFT));
    }
    assert_eq!(field.selected().as_deref(), Some("world"));
    assert_eq!(field.held(), Some(6..11));

    type_in(&mut field, "there");
    assert_eq!(field.said(), "hello there");
    assert_eq!(field.held(), None, "the selection outlived what it was on");
}

/// A line has no line breaks in it and nothing to indent, and says so
/// rather than relying on whoever owns it to take those keys first.
#[test]
fn a_line_stays_one_line() {
    let mut field = Field::new();
    type_in(&mut field, "one");
    for code in [KeyCode::Enter, KeyCode::Tab, KeyCode::BackTab] {
        assert!(
            !press(&mut field, code),
            "{code:?} was taken by a line that has no use for it"
        );
    }
    assert_eq!(field.said(), "one");

    // Up and down have nowhere to go either, which is what lets a list
    // keep them for choosing a row.
    for code in [KeyCode::Up, KeyCode::Down] {
        assert!(!press(&mut field, code), "{code:?} was taken");
    }
}

/// A question that takes only some characters refuses the rest -- and
/// still counts them as seen, or the key would fall through and run a
/// command from inside the question.
#[test]
fn a_line_may_take_only_what_its_question_wants() {
    fn digits(character: char) -> bool {
        character.is_ascii_digit()
    }

    let mut field = Field::taking(digits);
    type_in(&mut field, "12x3y");
    assert_eq!(field.said(), "123");
    assert!(
        press(&mut field, KeyCode::Char('x')),
        "a refused character fell through to whatever is behind the question"
    );
}

/// A copy takes what is held, or the whole line where nothing is -- the
/// rule the file follows with its line and the notes with their note.
#[test]
fn a_copy_takes_what_is_held_or_the_whole_line() {
    let mut field = Field::new();
    type_in(&mut field, "one two");
    assert_eq!(
        field.copied(),
        ("one two".to_string(), "line"),
        "copying nothing is not something a key can usefully do"
    );

    for _ in 0..3 {
        with(&mut field, KeyCode::Left, KeyModifiers::SHIFT);
    }
    assert_eq!(field.copied(), ("two".to_string(), "selection"));

    // And a cut takes it out on the way.
    assert_eq!(field.cut(), ("two".to_string(), "selection"));
    assert_eq!(field.said(), "one ");
    assert_eq!(field.cut(), ("one ".to_string(), "line"));
    assert_eq!(field.said(), "");
}

/// A paste is text arriving all at once, and the line's rules apply to
/// every character of it.
#[test]
fn a_paste_goes_in_under_the_same_rules() {
    fn digits(character: char) -> bool {
        character.is_ascii_digit()
    }

    let mut field = Field::new();
    type_in(&mut field, "ab");
    assert!(press(&mut field, KeyCode::Left));
    field.put("cd");
    assert_eq!(field.said(), "acdb", "a paste did not land at the caret");

    // Line breaks become the blank a reader pasting a wrapped path meant.
    let mut field = Field::new();
    field.put("one\ntwo\tthree");
    assert_eq!(field.said(), "one two three");

    // And a rule refuses what it refuses, however the text arrives.
    let mut field = Field::taking(digits);
    field.put("1a2b3");
    assert_eq!(field.said(), "123");
}

/// And the three boxes that hold one: the caret reaches the middle of what
/// was typed, wherever obelus asks a short question.
mod in_place {
    use obelus::{
        app::App,
        buffer::Buffer,
        command::{Command, dispatch},
        event::{Event, Pointer},
    };

    use super::{KeyCode, KeyEvent, KeyModifiers};
    use crate::support;

    fn open(name: &str) -> (support::Scratch, App) {
        let scratch = support::Scratch::new(name);
        let path = scratch.path().join("one.rs");
        std::fs::write(&path, "fn main() {}\n").expect("writing it");
        let mut app = App::new(vec![Buffer::open(&path).expect("opening it")]);
        app.working_directory_for_test(scratch.path().to_path_buf());
        support::lay_out(&mut app, 76, 18);
        (scratch, app)
    }

    fn arrow(app: &mut App, code: KeyCode) {
        app.handle(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
    }

    /// A list's query.
    #[test]
    fn a_list_takes_the_caret_into_its_query() {
        let (_scratch, mut app) = open("field-picker");
        dispatch::dispatch(&mut app, Command::CommandPalette);
        support::type_text(&mut app, "open");
        arrow(&mut app, KeyCode::Left);
        arrow(&mut app, KeyCode::Left);
        support::type_text(&mut app, "XY");
        assert_eq!(
            app.picker().expect("the list").query(),
            "opXYen",
            "the caret could not reach the middle of the query"
        );

        // Home reaches the front, which the tab keys no longer take.
        arrow(&mut app, KeyCode::Home);
        support::type_text(&mut app, "Z");
        assert_eq!(app.picker().expect("the list").query(), "ZopXYen");
    }

    /// And a question on the status bar, which is where a rename is
    /// answered: it starts with the old name in it, so reaching the middle
    /// is the whole of what it is for.
    #[test]
    fn a_question_takes_the_caret_into_its_answer() {
        let (_scratch, mut app) = open("field-prompt");
        dispatch::dispatch(&mut app, Command::GoLine);
        support::type_text(&mut app, "123");
        arrow(&mut app, KeyCode::Left);
        support::type_text(&mut app, "9");
        assert_eq!(
            app.prompt().expect("the question").text(),
            "1293",
            "the caret could not reach the middle of the answer"
        );
    }

    /// And copying out of a box a reader can select in, which is the other
    /// half of the same selection: `ctrl+c` over a list used to copy the
    /// line of the file behind it, and `ctrl+x` used to take that line
    /// away where nobody could see it go.
    #[test]
    fn a_copy_comes_out_of_what_is_being_typed_into() {
        // The turn, and a provider of obelus's own: a copy is kept in one
        // place for the whole process, so a test that takes neither reaches
        // into the machine's real clipboard and empties whatever another
        // test had just put there.
        let _turn = support::clipboard_turn();
        obelus::clipboard::use_provider_for_test(obelus::clipboard::Provider::Kept);
        let (_scratch, mut app) = open("field-copy");
        let before = "fn main() {}\n";
        dispatch::dispatch(&mut app, Command::CommandPalette);
        support::type_text(&mut app, "open");
        for _ in 0..4 {
            app.handle(Event::Key(KeyEvent::new(
                KeyCode::Left,
                KeyModifiers::SHIFT,
            )));
        }

        dispatch::dispatch(&mut app, Command::SelectionCopy);
        assert_eq!(
            app.note().unwrap_or_default(),
            "Copied selection",
            "the copy came out of the file behind the list"
        );

        dispatch::dispatch(&mut app, Command::SelectionCut);
        assert_eq!(
            app.picker().expect("the list").query(),
            "",
            "the cut did not come out of the query"
        );
        assert_eq!(
            app.current_buffer()
                .expect("a file")
                .text()
                .rope()
                .to_string(),
            before,
            "the cut took a line out of the file behind the list"
        );
    }

    /// The whole round trip, through the keys rather than the commands:
    /// select in a box, `ctrl+c`, `ctrl+v`.
    ///
    /// The keys are the part that was missing. A dialog answers no global
    /// key -- that is what keeps one from opening another over it -- so
    /// copy, cut and paste were not bound anywhere a box could hear them,
    /// and pressing them in a list did nothing at all.
    #[test]
    fn copy_and_paste_work_through_the_keys_inside_a_list() {
        use obelus::clipboard::{Provider, use_provider_for_test};

        // The turn first: what obelus keeps when a provider cannot hold a
        // copy is one thing for the whole process, and asking for a
        // provider clears it. Two of these running at once is one test
        // emptying the clipboard another had just copied into.
        let _turn = support::clipboard_turn();
        use_provider_for_test(Provider::Osc52);
        let (_scratch, mut app) = open("field-roundtrip");
        dispatch::dispatch(&mut app, Command::SearchFile);
        support::type_text(&mut app, "thing");
        for _ in 0..5 {
            app.handle(Event::Key(KeyEvent::new(
                KeyCode::Left,
                KeyModifiers::SHIFT,
            )));
        }

        app.handle(Event::Key(KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
        )));
        assert_eq!(
            app.note().unwrap_or_default(),
            "Copied selection",
            "ctrl+c did nothing inside a list"
        );

        // Off what is held first, or the paste puts back exactly what was
        // copied and nothing looks to have happened.
        app.handle(Event::Key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE)));
        app.handle(Event::Key(KeyEvent::new(
            KeyCode::Char('v'),
            KeyModifiers::CONTROL,
        )));
        assert_eq!(
            app.picker().expect("the list").query(),
            "thingthing",
            "ctrl+v did nothing inside a list"
        );
    }

    /// And the same keys reach the settings filter.
    #[test]
    fn the_same_keys_reach_the_settings_filter() {
        use obelus::clipboard::{Provider, use_provider_for_test};

        // The turn first: what obelus keeps when a provider cannot hold a
        // copy is one thing for the whole process, and asking for a
        // provider clears it. Two of these running at once is one test
        // emptying the clipboard another had just copied into.
        let _turn = support::clipboard_turn();
        use_provider_for_test(Provider::Osc52);
        let (_scratch, mut app) = open("field-keys-settings");
        dispatch::dispatch(&mut app, Command::ConfigOpen);
        support::type_text(&mut app, "ab");
        app.handle(Event::Key(KeyEvent::new(
            KeyCode::Left,
            KeyModifiers::SHIFT,
        )));
        app.handle(Event::Key(KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
        )));
        assert_eq!(
            app.note().unwrap_or_default(),
            "Copied selection",
            "ctrl+c did nothing inside the settings"
        );
    }

    /// And the list of open files, which is the one dialog with a command
    /// of its own: while a row of it is on, the keys are looked up in that
    /// list's own context rather than in every dialog's, so what a dialog
    /// answers it has to answer through that.
    #[test]
    fn the_same_keys_reach_the_list_of_open_files() {
        use obelus::clipboard::{Provider, use_provider_for_test};

        // The turn first: what obelus keeps when a provider cannot hold a
        // copy is one thing for the whole process, and asking for a
        // provider clears it. Two of these running at once is one test
        // emptying the clipboard another had just copied into.
        let _turn = support::clipboard_turn();
        use_provider_for_test(Provider::Osc52);
        let (_scratch, mut app) = open("field-keys-buffers");
        dispatch::dispatch(&mut app, Command::DocumentList);
        // A row of it still on, which is what puts the keys in that
        // list's own context: a query that matched nothing would leave it
        // in every other dialog's, and prove nothing.
        support::type_text(&mut app, "one");
        assert!(
            app.picker()
                .and_then(obelus::component::picker::Picker::selected_item)
                .is_some(),
            "the query filtered the list empty, so this is not the buffers context"
        );

        app.handle(Event::Key(KeyEvent::new(
            KeyCode::Left,
            KeyModifiers::SHIFT,
        )));
        app.handle(Event::Key(KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
        )));
        assert_eq!(
            app.note().unwrap_or_default(),
            "Copied selection",
            "ctrl+c did nothing inside the list of open files"
        );
    }

    /// With no file open at all, which is what `ob some-directory` gives
    /// a reader: the box is right there and the keys have to reach it.
    ///
    /// These three asked for a file to be open, which was the right
    /// question while the file was the only thing with a caret in it. A
    /// box a reader is typing into is another, and whether something is
    /// open behind it has nothing to do with copying out of it.
    #[test]
    fn the_keys_reach_a_box_with_no_file_behind_it() {
        use obelus::clipboard::{Provider, use_provider_for_test};

        // The turn first: what obelus keeps when a provider cannot hold a
        // copy is one thing for the whole process, and asking for a
        // provider clears it. Two of these running at once is one test
        // emptying the clipboard another had just copied into.
        let _turn = support::clipboard_turn();
        use_provider_for_test(Provider::Osc52);
        let scratch = support::Scratch::new("field-no-file");
        std::fs::write(scratch.path().join("one.rs"), "fn main() {}\n").expect("writing it");
        let mut app = App::new(Vec::new());
        app.working_directory_for_test(scratch.path().to_path_buf());
        support::lay_out(&mut app, 76, 18);

        dispatch::dispatch(&mut app, Command::SearchProject);
        assert!(app.picker().is_some(), "the search did not open");
        assert!(
            app.offers(Command::SelectionCopy) && app.offers(Command::Paste),
            "the keys are refused over a box the reader is looking at"
        );

        support::type_text(&mut app, "ab");
        app.handle(Event::Key(KeyEvent::new(
            KeyCode::Left,
            KeyModifiers::SHIFT,
        )));
        app.handle(Event::Key(KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
        )));
        assert_eq!(app.note().unwrap_or_default(), "Copied selection");

        app.handle(Event::Key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE)));
        app.handle(Event::Key(KeyEvent::new(
            KeyCode::Char('v'),
            KeyModifiers::CONTROL,
        )));
        assert_eq!(app.picker().expect("the list").query(), "abb");
    }

    /// And out of the box a message is written in, which is the last of
    /// the places a reader types.
    #[test]
    fn a_cut_comes_out_of_the_message_being_written() {
        // The turn, and a provider of obelus's own: a copy is kept in one
        // place for the whole process, so a test that takes neither reaches
        // into the machine's real clipboard and empties whatever another
        // test had just put there.
        let _turn = support::clipboard_turn();
        obelus::clipboard::use_provider_for_test(obelus::clipboard::Provider::Kept);
        let (scratch, mut app) = open("field-chat-cut");
        let before = "fn main() {}\n";
        dispatch::dispatch(&mut app, Command::AgentOpen);
        assert!(app.chat().is_some(), "the chat did not open");

        dispatch::dispatch(&mut app, Command::SelectionCut);
        // Back to the file, which is a document of its own rather than
        // something the conversation is drawn over: what the cut must not
        // have done is take a line out of it while the reader was elsewhere.
        app.open_for_test(&scratch.path().join("one.rs"));
        assert_eq!(
            app.current_buffer()
                .expect("a file")
                .text()
                .rope()
                .to_string(),
            before,
            "the cut took a line out of the file the reader left"
        );
    }

    /// Dragging in a box selects in it, the way dragging in the file
    /// selects in the file.
    ///
    /// The pointer used to stop at every panel: `on_pointer` returned for
    /// anything covering the screen, so no box ever saw a click and a
    /// reader could select with the keyboard and not with the mouse.
    #[test]
    fn dragging_in_a_box_selects_in_it() {
        let (_scratch, mut app) = open("field-drag");
        dispatch::dispatch(&mut app, Command::SearchFile);
        support::type_text(&mut app, "hello world");
        let (y, at) = support::place_of(&mut app, "hello world");

        app.handle(Event::Pointer {
            kind: Pointer::Pressed,
            x: at + 6,
            y,
        });
        app.handle(Event::Pointer {
            kind: Pointer::Dragged,
            x: at + 11,
            y,
        });
        assert_eq!(
            app.picker().expect("the list").query_held(),
            Some(6..11),
            "the drag did not hold the word it crossed"
        );
        assert_eq!(
            app.current_buffer()
                .expect("a file")
                .text()
                .rope()
                .to_string(),
            "fn main() {}\n",
            "the drag reached the file behind the list"
        );
    }

    /// Twice is the word and three times is the whole of it, which is what
    /// a line has instead of a line.
    #[test]
    fn clicking_twice_holds_a_word_and_three_times_holds_the_line() {
        let (_scratch, mut app) = open("field-clicks");
        dispatch::dispatch(&mut app, Command::SearchFile);
        support::type_text(&mut app, "hello world");
        let (y, at) = support::place_of(&mut app, "hello world");

        for _ in 0..2 {
            app.handle(Event::Pointer {
                kind: Pointer::Pressed,
                x: at + 8,
                y,
            });
        }
        assert_eq!(
            app.picker().expect("the list").query_held(),
            Some(6..11),
            "two clicks did not hold the word"
        );

        app.handle(Event::Pointer {
            kind: Pointer::Pressed,
            x: at + 8,
            y,
        });
        assert_eq!(
            app.picker().expect("the list").query_held(),
            Some(0..11),
            "three clicks did not hold the whole line"
        );
    }

    /// And the question on the status bar, which is the nearest of the
    /// three: it is answered over whatever else is showing.
    #[test]
    fn dragging_in_a_question_selects_in_the_answer() {
        let (_scratch, mut app) = open("field-drag-prompt");
        dispatch::dispatch(&mut app, Command::GoLine);
        support::type_text(&mut app, "12345");
        let (y, at) = support::place_of(&mut app, "12345");

        app.handle(Event::Pointer {
            kind: Pointer::Pressed,
            x: at,
            y,
        });
        app.handle(Event::Pointer {
            kind: Pointer::Dragged,
            x: at + 3,
            y,
        });
        assert_eq!(
            app.prompt().expect("the question").held(),
            Some(0..3),
            "the drag did not hold what it crossed"
        );
    }

    /// A paste goes where the reader is typing, not into the file behind
    /// whatever is covering it.
    #[test]
    fn a_paste_lands_where_the_caret_is() {
        let before = "fn main() {}\n";
        for (what, open_it) in [
            ("a list", Command::FileOpen),
            ("a search", Command::SearchFile),
            ("the palette", Command::CommandPalette),
        ] {
            let (_scratch, mut app) = open(&format!("field-paste-{what}").replace(' ', "-"));
            dispatch::dispatch(&mut app, open_it);
            app.handle(Event::Paste("pasted".to_string()));
            assert_eq!(
                app.picker().expect(what).query(),
                "pasted",
                "the paste did not reach {what}"
            );
            assert_eq!(
                app.current_buffer()
                    .expect("a file")
                    .text()
                    .rope()
                    .to_string(),
                before,
                "the paste went into the file behind {what}"
            );
        }

        // And into a question, under the question's own rule about what
        // belongs in it.
        let (_scratch, mut app) = open("field-paste-prompt");
        dispatch::dispatch(&mut app, Command::GoLine);
        app.handle(Event::Paste("4a2".to_string()));
        assert_eq!(app.prompt().expect("the question").text(), "42");
        assert_eq!(
            app.current_buffer()
                .expect("a file")
                .text()
                .rope()
                .to_string(),
            before,
            "the paste went into the file behind the question"
        );
    }
}
