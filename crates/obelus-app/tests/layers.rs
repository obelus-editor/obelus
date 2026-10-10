//! What is over the file, and what it does and does not let past.
//!
//! Every one of these is driven through `App::handle` and asserted on the
//! application rather than on cells. A golden screen says "this looks like
//! this"; it can never say "no screen can look like both", which is what a
//! rule about stacking needs.
//!
//! Every one of them walks `layers::STACK` rather than a list of its own, so
//! a view added without an answer here is a view these tests will not
//! compile without.

mod support;

use crossterm::event::KeyCode;
use obelus_app::app::{App, dispatch};
use obelus_command::Command;
use obelus_component::layers::Layer;
use support::press;

/// Wide enough for the settings page's two columns, tall enough for a list.
const WIDTH: u16 = 76;
const HEIGHT: u16 = 24;

/// A reader with one file open.
///
/// One and not two, so that choosing from the list of open files lands back
/// on the file the test started with: what is asserted is that the *file*
/// did not change, and a test that also switched documents would be
/// asserting two things and failing for either.
fn reading() -> App {
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    support::lay_out(&mut app, WIDTH, HEIGHT);
    app
}

/// Opens one, however that one is opened.
///
/// Exhaustive on purpose: a layer added without a way to open it here is a
/// layer these tests would silently skip.
fn open(app: &mut App, layer: Layer) {
    match layer {
        Layer::Counts => dispatch::dispatch(app, Command::CountLines),
        Layer::Settings => dispatch::dispatch(app, Command::ConfigOpen),
        Layer::Picker => dispatch::dispatch(app, Command::DocumentList),
        // No command opens this one: it is what enter does on a setting
        // that holds a list of names, and the only such setting is a
        // window's -- so a test running as a terminal has no row to press.
        Layer::Names => app.open_names("fonts"),
        Layer::Prompt => dispatch::dispatch(app, Command::GoLine),
        // No command either: it is what the project going from under the
        // reader puts up. So a project of the test's own, taken away.
        Layer::Gone => {
            static GONE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let tree = std::env::temp_dir().join(format!(
                "obelus-layers-gone-{}-{}",
                std::process::id(),
                GONE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&tree).expect("a tree");
            app.working_directory_for_test(tree.clone());
            std::fs::remove_dir_all(&tree).expect("the tree going");
            app.handle(obelus_app::event::Event::Watched(obelus_watch::Changed {
                path: tree.join("anything"),
            }));
        }
    }
}

/// What a reader is in, after opening one of them.
///
/// The derivation and the opening have to agree, and this is the only test
/// that checks they do: everything below it trusts `layers()` to say what is
/// showing.
#[test]
fn opening_one_puts_the_reader_in_it() {
    for layer in obelus_component::layers::STACK {
        let mut app = reading();
        assert_eq!(app.layers().nearest(), None, "{layer:?}: not a clean start");
        open(&mut app, layer);
        assert_eq!(
            app.layers().nearest(),
            Some(layer),
            "{layer:?} did not open, or opened as something else"
        );
    }
}

/// Escape from any of them comes back to the file, from every starting point.
///
/// This is the invariant the whole arrangement exists for. The reader is
/// never made to work out how many things are stacked up, because there is
/// never more than one thing to leave.
#[test]
fn escape_from_anywhere_comes_back_to_the_file() {
    for layer in obelus_component::layers::STACK {
        let mut app = reading();
        open(&mut app, layer);
        press(&mut app, KeyCode::Esc);
        // Except from the page saying the project has gone, which is the
        // one thing over the file that is not left but answered: the file
        // is in the project that went, and there is nothing to come back
        // to. The page that asks which project is the same, and is not a
        // layer. Broken deliberately by having the page's own key handler
        // put `gone` back to false on escape -- where every other layer
        // takes escape as leaving -- and this fails.
        if layer == Layer::Gone {
            assert_eq!(
                app.layers().nearest(),
                Some(Layer::Gone),
                "escape took the page away with nowhere to go back to"
            );
            continue;
        }
        assert!(
            !app.layers().any(),
            "{layer:?} was still showing after escape"
        );
        assert!(
            app.current_buffer().is_some(),
            "{layer:?}: escape left no file being read"
        );
    }
}

/// No layer lets a key reach the file behind it.
///
/// Broken deliberately by having a layer let through what it does not take
/// (`Hearer::lets_through`), and it is the counts this catches: with a file
/// open and the counts over it, a letter went into the file nobody could
/// see, and so did `backspace`, `delete` and `tab`. The notes were just as
/// unguarded and escaped by luck -- they have a box that swallows characters.
/// Which is the reason every layer is asked rather than the one that looked
/// suspicious: what makes a hole harmless here is an accident of another view's
/// keys, and accidents do not hold still.
///
/// `Enter` is left out on purpose. It means "choose this" in half of these,
/// so a test that pressed it would be asserting about what choosing does.
/// The refusal it would be testing does not read the key at all: it is one
/// condition on what is showing, so a letter getting through and `enter`
/// getting through are the same bug.
#[test]
fn nothing_typed_over_a_layer_reaches_the_file() {
    // The cut below writes whatever it took to a clipboard, and a test that
    // did not ask for Obelus's own would be writing the reader's.
    let _turn = support::clipboard_turn();
    obelus_clipboard::use_provider_for_test(obelus_clipboard::Provider::Kept);
    for layer in obelus_component::layers::STACK {
        let mut app = reading();
        let before = app
            .current_buffer()
            .expect("a file to read")
            .text()
            .rope()
            .to_string();

        open(&mut app, layer);
        for code in [
            KeyCode::Char('x'),
            KeyCode::Backspace,
            KeyCode::Delete,
            KeyCode::Tab,
        ] {
            press(&mut app, code);
        }
        // And the cut, which is bound in a dialog and so is not stopped by
        // what stops a key: it asked a chain of the boxes that had a
        // selection, the counts had none, and the line went out of the file.
        // Broken deliberately by letting the cut go on past the counts.
        support::press_control(&mut app, 'x');

        let after = app
            .current_buffer()
            .expect("a file to read")
            .text()
            .rope()
            .to_string();
        assert_eq!(after, before, "{layer:?} let typing through to the file");
    }
}

/// And no layer lets a motion reach it either.
///
/// The same hole, one door along, and the notes fall into this one: their
/// box swallows a letter but not `ctrl+left`, so a word motion over the
/// notes page walked a cursor in the file behind it. Worse than the typing,
/// because nothing on screen changes and the damage is found later.
#[test]
fn no_motion_over_a_layer_moves_the_hidden_cursor() {
    for layer in obelus_component::layers::STACK {
        let mut app = reading();
        // Somewhere with room to move in both directions, so a motion that
        // does get through has somewhere to go.
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Right);
        let at = |app: &App| {
            let cursor = app.current_buffer().expect("a file to read").cursor();
            (cursor.line, cursor.column)
        };
        let before = at(&app);

        open(&mut app, layer);
        press(&mut app, KeyCode::End);
        support::press_control_key(&mut app, KeyCode::Right);
        support::press_control_key(&mut app, KeyCode::Left);

        assert_eq!(at(&app), before, "{layer:?} moved the cursor behind it");
    }
}

/// A question on the status bar is over the file without covering it.
///
/// The distinction the pointer needs and the keys do not: every line is
/// still on screen, and a line the reader can see is a line they can click.
/// Everything else here takes the screen, so a click that reached the code
/// behind it would move a caret nobody can see.
#[test]
fn only_a_question_leaves_the_file_pointable() {
    for layer in obelus_component::layers::STACK {
        let mut app = reading();
        open(&mut app, layer);
        let covering = app.layers().covering();
        match layer {
            Layer::Prompt => assert!(!covering, "a question is one row, not a screen"),
            _ => assert!(covering, "{layer:?} does not cover the file it is over"),
        }
    }
}

/// A key goes to the nearest layer, not to whichever was asked first.
///
/// The order used to live in the chain itself, so the only way to know it
/// was to read a hundred and thirty lines of `handle_key` and hope the four
/// other places that also held an order agreed. It is now one array, read
/// backwards, and this is the assertion that it is read at all.
///
/// A list over a page is the pair to use because it is one Obelus really
/// has: a setting's choices, and any list opened while a page is showing.
#[test]
fn a_key_goes_to_the_nearest_layer() {
    let mut app = reading();
    dispatch::dispatch(&mut app, Command::TodoOpen);
    dispatch::dispatch(&mut app, Command::DocumentList);
    assert_eq!(
        app.layers().nearest(),
        Some(Layer::Picker),
        "the list did not open over the page"
    );

    press(&mut app, KeyCode::Char('x'));

    assert_eq!(
        app.picker().expect("the list").query(),
        "x",
        "the list did not get the key it was nearest to"
    );
    assert!(
        app.notes().is_some(),
        "the document the list opened over went away under it"
    );
}

/// The caret is the nearest layer's, which is where the keys are going.
///
/// These were two chains in two orders. The keys walked picker, settings,
/// notes, counts, conversation, question; the caret walked question,
/// picker, conversation, settings, notes, counts. So a question with a list
/// opened over it drew the caret in the question while the typing went to
/// the list, and a conversation with the notes over it drew the caret in
/// the conversation while the typing went to the notes. There is one order
/// now, and this is what says so.
#[test]
fn the_caret_is_where_the_keys_are() {
    let area = ratatui::layout::Rect {
        x: 0,
        y: 0,
        width: WIDTH,
        height: HEIGHT,
    };

    // A list over the notes: the list is nearer, so the caret belongs on
    // its prompt rather than in the note behind it.
    let mut app = reading();
    dispatch::dispatch(&mut app, Command::TodoOpen);
    dispatch::dispatch(&mut app, Command::DocumentList);
    let over = obelus_ui::cursor_position(area, &app).expect("a caret somewhere");
    assert_eq!(
        over.y,
        HEIGHT - 1,
        "the caret was not on the list's own prompt"
    );

    // And a conversation, which is a document rather than a layer, puts it
    // in the box: in the region rather than on the status row, because a
    // message is a paragraph.
    let mut app = reading();
    app.new_conversation();
    assert!(!app.layers().any(), "a conversation is not over anything");
    let alone = obelus_ui::cursor_position(area, &app).expect("a caret somewhere");
    assert!(
        alone.y < HEIGHT - 1,
        "the conversation's caret was on the status row"
    );
}

/// Opening something covers the question that was on the status bar.
///
/// A question is about the thing behind it -- a line to go to, a new name
/// for what the cursor is on -- so once a view has taken that, the question
/// is one nobody can answer. Six openers each decided this for themselves
/// and three of them decided nothing at all, which is why the notes carried
/// a guard against a question that no other view needed.
#[test]
fn opening_anything_covers_the_question() {
    for layer in obelus_component::layers::STACK {
        if layer == Layer::Prompt {
            continue;
        }
        let mut app = reading();
        dispatch::dispatch(&mut app, Command::GoLine);
        assert!(app.layers().has(Layer::Prompt), "no question to cover");

        open(&mut app, layer);
        assert!(
            !app.layers().has(Layer::Prompt),
            "{layer:?} opened over the question and left it there"
        );
    }
}

/// Two pages are never open at once.
///
/// Dispatched rather than pressed, on purpose: the key table has nothing
/// bound in a dialog, so a *key* cannot open a second page. That guard is
/// one enforcement of the rule and not the rule itself -- an agent's
/// question already goes around it, arriving from the connection rather
/// than from a key -- so what is asserted here is the rule.
#[test]
fn two_pages_are_never_open_at_once() {
    let pages = [Layer::Counts, Layer::Settings];
    for first in pages {
        for second in pages {
            let mut app = reading();
            open(&mut app, first);
            open(&mut app, second);
            assert_eq!(
                app.layers().furthest_first().count(),
                1,
                "{first:?} and {second:?} were both showing"
            );
            assert_eq!(
                app.layers().nearest(),
                Some(second),
                "{second:?} did not end up in front of {first:?}"
            );
        }
    }
}

/// And the one nesting that is allowed stays allowed.
///
/// A list opens *over* a page rather than instead of it: a setting's
/// choices, and an agent's own question. This is the one place Obelus
/// stacks two things the reader is in, and it is why the rule is "a view
/// covers what shares its room" rather than "opening covers".
#[test]
fn a_list_opens_over_a_page_rather_than_instead_of_it() {
    let mut app = reading();
    dispatch::dispatch(&mut app, Command::CountLines);
    dispatch::dispatch(&mut app, Command::DocumentList);
    assert_eq!(
        app.layers().furthest_first().collect::<Vec<_>>(),
        [Layer::Counts, Layer::Picker],
        "the list did not open over the page"
    );

    press(&mut app, KeyCode::Esc);
    assert_eq!(
        app.layers().furthest_first().collect::<Vec<_>>(),
        [Layer::Counts],
        "leaving the list took the page with it"
    );
    press(&mut app, KeyCode::Esc);
    assert!(!app.layers().any(), "the page would not be left");
}

/// The keys about documents work in a conversation.
///
/// A conversation is a document, so the list of them and the key that closes
/// one are about it as much as about a file. They asked for a *file* being
/// open, which is the same question while every document was one and stopped
/// being the same question the moment one was not: the key went dim and did
/// nothing, and the only way to another document was to know another key.
#[test]
fn the_keys_about_documents_work_in_a_conversation() {
    let mut app = reading();
    app.new_conversation();
    assert!(app.chat().is_some(), "not in a conversation");

    assert!(
        app.offers(Command::DocumentList),
        "the list of open documents is refused from inside one"
    );
    assert!(
        app.offers(Command::DocumentClose),
        "a conversation cannot be closed with the key that closes a document"
    );
    // And what is about a file is still refused, which is the other half:
    // there is nothing here to save, reload, or go to a line of.
    for command in [Command::FileSave, Command::FileReload, Command::GoLine] {
        assert!(
            !app.offers(command),
            "{command:?} was offered in a conversation, which has no file"
        );
    }

    dispatch::dispatch(&mut app, Command::DocumentList);
    assert!(app.picker().is_some(), "the list would not open");
}

/// A conversation is a row of the list, and choosing it goes there.
///
/// It is in the list because it is a document. A row that did nothing when
/// chosen would be a row that lies about being one -- and that is what it
/// did, because what accepted a row only ever switched to a file.
#[test]
fn a_conversation_can_be_switched_to_from_the_list() {
    let mut app = reading();
    app.new_conversation();
    let conversation = app.current_document_for_test().expect("a document");

    // Away to the file, and then back through the list.
    app.open_for_test(std::path::Path::new("tests/fixtures/sample.rs"));
    assert!(app.chat().is_none(), "still in the conversation");

    dispatch::dispatch(&mut app, Command::DocumentList);
    let rows: Vec<String> = app
        .picker()
        .expect("the list")
        .matches()
        .map(|item| item.label.clone())
        .collect();
    assert!(
        rows.len() >= 2,
        "the conversation is not a row of the list: {rows:?}"
    );

    // Narrowed to it by name rather than stepped to by a count of rows: the
    // list opens on whatever is being read, so where the conversation is
    // from there is a fact about how many files happen to be open.
    support::type_text(&mut app, "conversation");
    press(&mut app, KeyCode::Enter);
    assert_eq!(
        app.current_document_for_test(),
        Some(conversation),
        "choosing the conversation's row went nowhere"
    );
    assert!(app.chat().is_some(), "it is not the conversation");
}

/// Closing a row of the list leaves the selection where that row was.
///
/// The list is rebuilt after a close, and a rebuilt list opens on whatever
/// is being read -- the first row, for a reader who opened one file and
/// then went looking -- so closing three in a row walked back up to the top
/// between each. Broken by dropping the `select_row` after the rebuild: the
/// selection lands on `nested.rs`, the file being read, and the first
/// assertion fails.
#[test]
fn closing_a_row_keeps_the_selection_where_it_was() {
    let mut app = reading();
    for name in ["long.rs", "many_lines.rs", "nested.rs"] {
        app.open_for_test(&std::path::Path::new("tests/fixtures").join(name));
    }
    dispatch::dispatch(&mut app, Command::DocumentList);
    let selected = |app: &App| {
        app.picker()
            .and_then(|picker| picker.selected_item())
            .map(|item| item.label.clone())
            .unwrap_or_default()
    };
    // From the top, where the reader is when they are reading the first.
    support::press_control_key(&mut app, KeyCode::Home);
    press(&mut app, KeyCode::Down);
    assert!(selected(&app).ends_with("long.rs"), "{}", selected(&app));

    support::press_control(&mut app, 'w');
    assert!(
        selected(&app).ends_with("many_lines.rs"),
        "closing a row did not leave the selection on the row after it, but on {}",
        selected(&app)
    );

    // And the last row has no row after it, so the one before.
    support::press_control_key(&mut app, KeyCode::End);
    support::press_control(&mut app, 'w');
    assert!(
        selected(&app).ends_with("many_lines.rs"),
        "closing the last row did not leave the selection on the one before, but on {}",
        selected(&app)
    );
}

/// Whatever is over the conversation owns the status row.
///
/// The conversation draws its own while it is what the reader is looking
/// at. The moment something is over it, that row belongs to the thing over
/// it -- its query, its question, its filter -- and a row about the
/// conversation underneath would be two things asking to be read at once.
///
/// It asked whether a *list* was over it, which was every case there was
/// while the conversation was a layer and nothing could be over it but one.
/// As a document, the notes and the settings open over it too.
#[test]
fn what_is_over_a_conversation_owns_the_status_row() {
    let said = |app: &mut App| {
        let dump = support::render(app, WIDTH, HEIGHT);
        support::text_block(&dump)
            .lines()
            .last()
            .unwrap_or_default()
            .to_string()
    };

    // The settings, because their filter is what the row says while they are
    // open -- so whether the row is theirs is something a test can read.
    let mut app = reading();
    app.new_conversation();
    dispatch::dispatch(&mut app, Command::ConfigOpen);
    support::type_text(&mut app, "wrap");
    let covered = said(&mut app);
    assert!(
        covered.contains("wrap"),
        "the settings were over the conversation and the row was not theirs:\n{covered}"
    );
}

/// A window turns its input method on exactly where a character typed
/// would go into some text, and the answer is checked against the
/// character rather than against itself: each place is typed at, and the
/// screen either takes the letter or stays as it was.
///
/// Not where the caret is. An empty filter on the settings has no caret
/// and takes the first letter typed -- which an input method turned on by
/// that letter would already have missed.
///
/// Deliberate break: answering `Layer::Counts` with `true`, and the counts
/// go red here; answering the settings with `true` whatever they are
/// doing, and the key being bound does; asking a file only whether it is
/// one, and the reading does; asking it only for its mode, and the
/// commit's version does.
#[test]
fn the_input_method_is_on_only_where_typing_goes() {
    /// Whether a letter typed here lands anywhere on the screen.
    fn lands(app: &mut App) -> bool {
        let before = support::render(app, WIDTH, HEIGHT);
        // A digit, because the question of a line to go to takes nothing else.
        support::type_text(app, "1");
        let after = support::render(app, WIDTH, HEIGHT);
        support::text_block(&before) != support::text_block(&after)
    }

    // The file, which is written into.
    let mut app = reading();
    assert!(app.takes_text(), "the file is not typed into");
    assert!(lands(&mut app), "a letter did not land in the file");

    for layer in obelus_component::layers::STACK {
        let mut app = reading();
        open(&mut app, layer);
        let expected = match layer {
            Layer::Counts | Layer::Gone => false,
            Layer::Settings | Layer::Names | Layer::Picker | Layer::Prompt => true,
        };
        assert_eq!(app.takes_text(), expected, "{layer:?}");
        assert_eq!(
            lands(&mut app),
            expected,
            "{layer:?} disagrees with typing at it"
        );
    }

    // The settings with nothing typed in their filter, which is where the
    // caret is not and the first letter still goes.
    let mut app = reading();
    open(&mut app, Layer::Settings);
    let area = ratatui::layout::Rect::new(0, 0, WIDTH, HEIGHT);
    assert_eq!(
        obelus_ui::cursor_position(area, &app),
        None,
        "an empty filter has a caret"
    );
    assert!(app.takes_text(), "an empty filter is not typed into");

    // A key being bound, which takes the next key and not a letter.
    let mut app = reading();
    open(&mut app, Layer::Settings);
    press(&mut app, KeyCode::Tab);
    support::type_text(&mut app, "choose-theme");
    press(&mut app, KeyCode::Enter);
    assert!(
        app.settings()
            .is_some_and(|settings| settings.binding().is_some()),
        "no key is being bound"
    );
    assert!(!app.takes_text(), "a key being bound takes text");

    // A reading of a file, which is not the file's bytes.
    let scratch = support::Scratch::new("typing-reading");
    let path = scratch.path().join("sample.md");
    std::fs::write(&path, "# before\n\nafter\n").expect("writing it");
    let mut app = App::new(vec![
        obelus_buffer::Buffer::open(&path).expect("opening it"),
    ]);
    app.working_directory_for_test(scratch.path().to_path_buf());
    support::lay_out(&mut app, WIDTH, HEIGHT);
    assert!(app.takes_text(), "the markdown file is not typed into");
    dispatch::dispatch(&mut app, Command::PreviewToggle);
    assert!(!app.takes_text(), "a reading takes text");
    assert!(!lands(&mut app), "a letter landed in a reading");

    // A commit's version of a file, which nobody can write -- in the mode
    // a file is written in, so it is the content saying no and not the mode.
    let version = obelus_buffer::Buffer::at_commit(
        &path,
        gix::ObjectId::null(gix::hash::Kind::Sha1),
        "# older\n",
    );
    let mut app = App::new(vec![version]);
    app.working_directory_for_test(scratch.path().to_path_buf());
    support::lay_out(&mut app, WIDTH, HEIGHT);
    assert_eq!(
        app.current_buffer().map(obelus_buffer::Buffer::mode),
        Some(obelus_buffer::Mode::Edit),
        "not in the mode a file is written in, so this proves nothing"
    );
    assert!(!app.takes_text(), "a commit's version takes text");
    assert!(!lands(&mut app), "a letter landed in a commit's version");

    // And nothing open at all.
    let mut app = App::new(Vec::new());
    app.working_directory_for_test(std::path::PathBuf::from("/tmp/obelus"));
    support::lay_out(&mut app, WIDTH, HEIGHT);
    assert!(!app.takes_text(), "an empty screen takes text");
}

/// A reader on the notes, which are the document: three of them, on the
/// first.
fn on_the_notes(name: &str) -> (support::Scratch, App) {
    let scratch = support::Scratch::new(&format!("layers-notes-{name}"));
    support::make_room_for_notes(scratch.path());
    std::fs::write(
        obelus_todo::path(scratch.path()).expect("a tree that is there"),
        "[[todo]]\nid = \"0123456A\"\nsaid = \"the first\"\ndone = false\n\n\
         [[todo]]\nid = \"0123456B\"\nsaid = \"the second\"\ndone = false\n\n\
         [[todo]]\nid = \"0123456C\"\nsaid = \"the third\"\ndone = false\n",
    )
    .expect("the notes");
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.working_directory_for_test(scratch.path().to_path_buf());
    support::lay_out(&mut app, WIDTH, HEIGHT);
    dispatch::dispatch(&mut app, Command::TodoOpen);
    (scratch, app)
}

/// The keys the notes take with a modifier, and the two that indent.
///
/// The ones a layer is likely to leave alone, which is the point: a key
/// every layer takes for itself proves nothing about what is behind it.
fn pressed_at_what_is_behind(app: &mut App) {
    use crossterm::event::{KeyEvent, KeyModifiers};
    for (code, modifiers) in [
        (KeyCode::Down, KeyModifiers::ALT),
        (KeyCode::Up, KeyModifiers::ALT),
        (KeyCode::Char(' '), KeyModifiers::ALT),
        (KeyCode::Char('a'), KeyModifiers::ALT),
        (KeyCode::Char('o'), KeyModifiers::ALT),
        (KeyCode::Delete, KeyModifiers::ALT),
        (KeyCode::Backspace, KeyModifiers::ALT),
        (KeyCode::Enter, KeyModifiers::ALT),
        (KeyCode::Enter, KeyModifiers::SHIFT),
        (KeyCode::Tab, KeyModifiers::NONE),
        (KeyCode::BackTab, KeyModifiers::SHIFT),
        (KeyCode::Char('x'), KeyModifiers::CONTROL),
        (KeyCode::End, KeyModifiers::CONTROL),
    ] {
        app.handle(obelus_app::event::Event::Key(KeyEvent::new(
            code, modifiers,
        )));
    }
}

/// No layer lets a key reach the notes behind it.
///
/// The notes stopped being a layer when they became a document, and a key a
/// layer did not want fell past it to them: `f1` over the notes and then
/// `alt+down` moved the note behind the list, and `alt+delete` took one
/// away. The file had a guard against exactly this and the notes never got
/// one -- which is why there is no guard now, only an order that stops at
/// whatever is in front.
///
/// Broken deliberately by having a layer let through what it does not take
/// (`Hearer::lets_through`), and the counts, the settings and a list each
/// move, take away or talk about the note behind them.
#[test]
fn nothing_pressed_over_a_layer_reaches_the_notes() {
    let _turn = support::clipboard_turn();
    obelus_clipboard::use_provider_for_test(obelus_clipboard::Provider::Kept);
    for layer in obelus_component::layers::STACK {
        let (scratch, mut app) = on_the_notes(&format!("{layer:?}"));
        let file = obelus_todo::path(scratch.path()).expect("a tree that is there");
        let standing = |app: &App| {
            let notes = app.notes().expect("the notes are what is being read");
            (
                notes.places(),
                notes.selected_note().cloned(),
                notes
                    .writing()
                    .map(obelus_component::composer::Composer::text),
            )
        };
        let before = standing(&app);
        let written = std::fs::read_to_string(&file).expect("the notes");

        open(&mut app, layer);
        if app.layers().nearest() != Some(layer) {
            // A question is about a file, and the notes are not one --
            // said by name, so a layer that stops opening here is noticed
            // rather than passed.
            assert!(
                matches!(layer, Layer::Prompt),
                "{layer:?} did not open over the notes"
            );
            continue;
        }
        pressed_at_what_is_behind(&mut app);

        assert_eq!(
            std::fs::read_to_string(&file).expect("the notes"),
            written,
            "{layer:?} let a key through to the notes file"
        );
        assert!(app.notes().is_some(), "{layer:?}: the notes went away");
        assert_eq!(
            standing(&app),
            before,
            "{layer:?} let a key through to the notes"
        );
    }
}

/// Nor to the conversation behind it.
///
/// Broken deliberately the same way, and `alt+enter` over a list or a page
/// puts a line break in the box behind it; and again by letting the cut go
/// on past the counts, which took the words out of the box.
#[test]
fn nothing_pressed_over_a_layer_reaches_the_conversation() {
    let _turn = support::clipboard_turn();
    obelus_clipboard::use_provider_for_test(obelus_clipboard::Provider::Kept);
    for layer in obelus_component::layers::STACK {
        let mut app = reading();
        app.new_conversation();
        support::type_text(&mut app, "half a thought");
        let standing = |app: &App| {
            let chat = app.chat().expect("the conversation is what is being read");
            (chat.writing().text(), chat.writing().selected(), chat.top())
        };
        let before = standing(&app);

        open(&mut app, layer);
        if app.layers().nearest() != Some(layer) {
            // A question is about a file, and a conversation is not one --
            // said by name, so a layer that stops opening here is noticed
            // rather than passed.
            assert!(
                matches!(layer, Layer::Prompt),
                "{layer:?} did not open over the conversation"
            );
            continue;
        }
        pressed_at_what_is_behind(&mut app);

        assert!(
            app.chat().is_some(),
            "{layer:?}: the conversation went away"
        );
        assert_eq!(
            standing(&app),
            before,
            "{layer:?} let a key through to the conversation"
        );
    }
}

/// A copy over a page with nothing to copy copies nothing.
///
/// Copy is bound in a dialog, so it is not stopped by what stops a key: it
/// asked a chain of the boxes that had a selection, the counts had none,
/// and `ctrl+c` over them copied the line of the file nobody could see.
/// Broken deliberately by letting the copy go on past the counts, and the
/// line is copied.
#[test]
fn a_copy_over_the_counts_copies_nothing() {
    let _turn = support::clipboard_turn();
    obelus_clipboard::use_provider_for_test(obelus_clipboard::Provider::Kept);
    let mut app = reading();
    open(&mut app, Layer::Counts);

    support::press_control(&mut app, 'c');

    assert_eq!(
        app.note(),
        None,
        "a copy over the counts took something from behind them"
    );
}

/// And a cut over a list over the settings comes out of the list.
///
/// The chain asked about the settings before the list, so with a setting's
/// choices open over the page `ctrl+x` took the page's filter -- out of
/// sight behind the list -- and left the list's query as it was. Broken
/// deliberately by putting that chain back, and the settings' filter is
/// what goes.
#[test]
fn a_cut_over_a_list_over_the_settings_comes_out_of_the_list() {
    let _turn = support::clipboard_turn();
    obelus_clipboard::use_provider_for_test(obelus_clipboard::Provider::Kept);
    let mut app = reading();
    open(&mut app, Layer::Settings);
    support::type_text(&mut app, "wrap");
    open(&mut app, Layer::Picker);
    assert_eq!(
        app.layers().furthest_first().collect::<Vec<_>>(),
        [Layer::Settings, Layer::Picker],
        "the list did not open over the settings"
    );
    support::type_text(&mut app, "ab");

    support::press_control(&mut app, 'x');

    assert_eq!(
        app.picker().expect("the list").query(),
        "",
        "the list kept its query"
    );
    assert_eq!(
        app.settings().expect("the settings").query(),
        "wrap",
        "the cut came out of the settings behind the list"
    );
}
