//! Making a file that is not there yet.
//!
//! The half of "where should this file be" that a rename is the other half
//! of: the same question on the same row, taking the same kind of answer,
//! and the file at the end of it is made rather than moved. Which is why
//! the words differ -- two prompts saying one thing on one row are one
//! prompt as far as a reader glancing at it is concerned.

mod support;

use crossterm::event::KeyCode;
use obelus_app::app::{App, dispatch};
use obelus_buffer::Buffer;
use obelus_command::Command;

/// Types an answer onto the end of whatever the question opened with.
///
/// Onto the end, because that is how the reader meets it: the directory
/// they are in is already there with the caret after it, and what they add
/// is the name.
fn add(app: &mut App, name: &str) {
    support::type_text(app, name);
    support::press(app, KeyCode::Enter);
}

/// Clears the question first, for an answer that is not under where the
/// reader happens to be.
fn answer(app: &mut App, path: &str) {
    for _ in 0..80 {
        support::press(app, KeyCode::Backspace);
    }
    add(app, path);
}

/// A project with a file open in it.
fn reading(name: &str) -> (support::Scratch, App) {
    let scratch = support::Scratch::new(name);
    scratch.write("src/hint.rs", "fn hint() {}\n");
    let mut app = App::new(vec![
        Buffer::open(&scratch.join("src/hint.rs")).expect("opening it"),
    ]);
    app.working_directory_for_test(scratch.path().to_path_buf());
    app.statuses_for_test(std::collections::HashMap::new());
    support::lay_out(&mut app, 76, 18);
    (scratch, app)
}

/// The two ends of the status row while the question is up: what is being
/// asked, and where the row says the file will go.
///
/// The last row of the screen, which is where a prompt draws and nowhere
/// else: a list open over anything owns that row, and a question is the
/// thing holding the caret. Split on the padding between them, which is
/// what a reader's eye does with it.
fn row(app: &mut App) -> (String, String) {
    let dump = support::render(app, 76, 18);
    let whole = support::text_block(&dump)
        .lines()
        .rfind(|row| row.contains('|'))
        .map(|row| row[row.find('|').expect("a divider") + 1..].trim_end())
        .unwrap_or_default()
        .to_string();
    match whole.rsplit_once("  ") {
        Some((asked, landing)) => (asked.trim().to_string(), landing.trim().to_string()),
        None => (whole.trim().to_string(), String::new()),
    }
}

/// What is being asked, for a test that is about the question alone.
fn asked(app: &mut App) -> String {
    row(app).0
}

/// And where the row says the file will go.
fn landing(app: &mut App) -> String {
    row(app).1
}

/// The file is on disk and open, and everything after that is the path any
/// other file takes.
///
/// On disk at once rather than in a buffer written later, which is the
/// whole of why this is thirty lines: a document that existed only in
/// memory would be a second kind of open file, and the watcher, the
/// server, git, `ctrl+r` and the status row would each have to learn about
/// it.
///
/// Deliberate break: have `make_file` open the path without creating it.
/// `open` fails on a file that is not there, the note says it is not there
/// any more, and nothing is opened at all.
#[test]
fn the_file_is_made_and_opened() {
    let (scratch, mut app) = reading("make-open");
    dispatch::dispatch(&mut app, Command::FileNew);
    add(&mut app, "helper.rs");

    assert!(scratch.join("src/helper.rs").exists(), "it was not made");
    let buffer = app.current_buffer().expect("a file");
    assert_eq!(
        buffer.path(),
        scratch.join("src/helper.rs"),
        "the reader is not in the file they made"
    );
    assert!(
        buffer.text().rope().to_string().is_empty(),
        "a new file with something in it"
    );
    // And it is one of the open documents rather than a view of its own,
    // which is what going through `open` buys.
    assert!(
        app.buffers_for_test()
            .iter()
            .any(|(path, _)| path == &scratch.join("src/helper.rs")),
        "it is not among the open documents"
    );
}

/// The question opens in the directory the reader is in, because that is
/// where the next file almost always goes.
///
/// Deliberate break: pass an empty string to the prompt in `new_file`. The
/// reader is asked from the project's root and has to type the directory
/// out, which for anything three deep is the whole of the answer.
#[test]
fn the_question_starts_in_the_directory_being_read() {
    let (_scratch, mut app) = reading("make-where");
    dispatch::dispatch(&mut app, Command::FileNew);
    assert_eq!(
        asked(&mut app),
        format!("New file: {}", support::as_shown("src/")),
        "the separator is the platform's, or the row is in two conventions"
    );
}

/// And its own words, which is what tells it from the rename's question.
///
/// Deliberate break: give `PromptKind::NewPath` the label `Call it: `. The
/// two questions are then one row saying one thing about two different
/// acts, and the only way to tell which is running is to remember what was
/// pressed.
#[test]
fn the_question_is_not_the_renames() {
    let (_scratch, mut app) = reading("make-words");
    dispatch::dispatch(&mut app, Command::FileNew);
    let making = asked(&mut app);
    support::press(&mut app, KeyCode::Esc);

    dispatch::dispatch(&mut app, Command::FileRename);
    let moving = asked(&mut app);
    assert_ne!(
        making.split(':').next(),
        moving.split(':').next(),
        "{making:?} and {moving:?} ask the same thing"
    );
}

/// Nothing is open, so there is nowhere to suggest and nothing is
/// suggested.
///
/// `ob some-directory` leaves the reader on a list with no file behind it,
/// and that is the reader most likely to be making the first file in a
/// project. A guess would be Obelus inventing a place.
///
/// Deliberate break: give `new_file`'s `map_or_else` a fallback with
/// something in it. The reader who has opened nothing is asked about a
/// directory nobody named.
#[test]
fn the_question_is_empty_where_nothing_is_open() {
    let scratch = support::Scratch::new("make-nothing");
    let mut app = App::new(Vec::new());
    app.working_directory_for_test(scratch.path().to_path_buf());
    app.statuses_for_test(std::collections::HashMap::new());
    support::lay_out(&mut app, 76, 18);

    dispatch::dispatch(&mut app, Command::FileNew);
    assert_eq!(asked(&mut app), "New file:");
}

/// And empty for a file in the project's own root, which is the same
/// answer arriving a different way.
///
/// Stripping the root off itself leaves a path with nothing in it, and
/// `join` on one of those adds nothing -- where a separator stuck on the
/// end by hand would be `/`, the root of the *disk*, which is where the
/// reader goes if they take the suggestion.
///
/// Deliberate break: build the suggestion with `format!("{shown}/")`
/// instead of `join`. The question opens on a bare `/`, which is the root
/// of the *disk*, and a name typed onto it is a file the reader almost
/// certainly has no business writing.
#[test]
fn the_question_is_empty_for_a_file_in_the_root() {
    let scratch = support::Scratch::new("make-root");
    scratch.write("main.rs", "fn main() {}\n");
    let mut app = App::new(vec![
        Buffer::open(&scratch.join("main.rs")).expect("opening it"),
    ]);
    app.working_directory_for_test(scratch.path().to_path_buf());
    app.statuses_for_test(std::collections::HashMap::new());
    support::lay_out(&mut app, 76, 18);

    dispatch::dispatch(&mut app, Command::FileNew);
    assert_eq!(asked(&mut app), "New file:");
}

/// A file already there is refused rather than emptied.
///
/// Which is the one thing here that cannot be put back, and the same
/// refusal a rename makes onto an occupied path -- in the same words,
/// because it is the same fact.
///
/// Deliberate break: use `File::create` instead of `File::create_new`. The
/// file the reader named is truncated to nothing and opened, and what was
/// in it is gone with no way back.
#[test]
fn a_file_already_there_is_refused() {
    let (scratch, mut app) = reading("make-there");
    dispatch::dispatch(&mut app, Command::FileNew);
    answer(&mut app, "src/hint.rs");

    assert_eq!(app.note(), Some("src/hint.rs is already there"));
    assert_eq!(
        std::fs::read_to_string(scratch.join("src/hint.rs")).expect("still readable"),
        "fn hint() {}\n",
        "the file that was there was written over"
    );
}

/// A directory that is not there yet is made, which is what taking a path
/// rather than a name is for.
///
/// Deliberate break: drop the `create_dir_all` in `make_file`.
/// `create_new` fails with "No such file or directory" and a reader
/// starting a module is told to go and make the directory themselves.
#[test]
fn a_directory_that_is_not_there_yet_is_made() {
    let (scratch, mut app) = reading("make-dir");
    dispatch::dispatch(&mut app, Command::FileNew);
    answer(&mut app, "src/lsp/hints.rs");

    assert!(
        scratch.join("src/lsp").is_dir(),
        "the directory was not made"
    );
    assert!(scratch.join("src/lsp/hints.rs").exists(), "it was not made");
}

/// A blank answer leaves the question up and makes nothing.
///
/// Settled by the prompt rather than by the answer: `Field::is_empty` asks
/// whether the line is *blank*, so enter on one of spaces is consumed and
/// the question stays where it is. Which is why the `NewPath` arm carries
/// no guard of its own -- one there would be a second answer to a settled
/// question, and an unreachable one.
///
/// Deliberate break: have `Editing::is_blank` ask `is_empty` rather than
/// `trim().is_empty()`. The spaces are accepted, the answer trims them to
/// nothing, `working_directory.join("")` is the project itself, and a
/// reader clearing the line is told the project is already there.
#[test]
fn a_blank_answer_makes_nothing() {
    let (scratch, mut app) = reading("make-nowhere");
    let before = std::fs::read_dir(scratch.join("src"))
        .expect("a directory")
        .count();

    dispatch::dispatch(&mut app, Command::FileNew);
    answer(&mut app, "   ");

    assert_eq!(app.note(), None, "something was said about nothing");
    assert_eq!(
        asked(&mut app),
        "New file:",
        "the question was answered by a line with nothing in it"
    );
    assert_eq!(
        std::fs::read_dir(scratch.join("src"))
            .expect("a directory")
            .count(),
        before,
        "something was made"
    );
}

/// It is offered with nothing open, because that is when it is most wanted.
///
/// Deliberate break: give it `Requires::AFileOpen`. The row goes dim on the
/// one screen a reader making the first file in a project is looking at,
/// and the key does nothing.
#[test]
fn it_is_offered_with_nothing_open() {
    let scratch = support::Scratch::new("make-offered");
    let mut app = App::new(Vec::new());
    app.working_directory_for_test(scratch.path().to_path_buf());
    app.statuses_for_test(std::collections::HashMap::new());
    support::lay_out(&mut app, 76, 18);
    assert!(app.offers(Command::FileNew));
}

/// The row says which directory the file will land in, as it is typed.
///
/// Which the question itself cannot say: the answer is a path, a path is
/// relative to something, and until this the row never said what. A reader
/// standing in the project's own root was shown a blank line and a file
/// that landed they knew not where.
///
/// Worked out from what is typed rather than from where the reader is, so
/// deleting the suggestion moves it: that is the moment the answer stops
/// meaning what the suggestion meant, and the only one nothing was saying.
///
/// Deliberate break: drop the `render_landing` call beside the prompt's
/// `write`. The row goes back to the question alone and every case below
/// reads the same.
#[test]
fn the_row_says_which_directory_it_will_land_in() {
    let (_scratch, mut app) = reading("landing");
    dispatch::dispatch(&mut app, Command::FileNew);
    assert_eq!(landing(&mut app), support::as_shown("src/"), "as it opens");

    // A name onto the suggestion: the same directory, which is the point
    // of the suggestion.
    support::type_text(&mut app, "helper.rs");
    assert_eq!(landing(&mut app), support::as_shown("src/"), "a name on it");

    // And a directory under it.
    support::type_text(&mut app, "");
    for _ in 0..9 {
        support::press(&mut app, KeyCode::Backspace);
    }
    support::type_text(&mut app, "lsp/hints.rs");
    assert_eq!(
        landing(&mut app),
        support::as_shown("src/lsp/"),
        "a directory under it"
    );
}

/// The project's own root is said as `.`, not as nothing.
///
/// Nothing is what the reader was shown when they were standing in it,
/// which is the case this whole tail exists for: a bare name typed into a
/// cleared line lands in the root, and the row said not a word about it.
///
/// Deliberate break: answer `String::new()` for the empty case in
/// `making_in` instead of `Path::new(".").join("")`. The one moment the
/// row has something to say goes back to silence.
#[test]
fn the_projects_own_root_is_said() {
    let (_scratch, mut app) = reading("landing-root");
    dispatch::dispatch(&mut app, Command::FileNew);
    for _ in 0..80 {
        support::press(&mut app, KeyCode::Backspace);
    }
    support::type_text(&mut app, "notes.md");
    assert_eq!(landing(&mut app), support::as_shown("./"));
}

/// Somewhere outside the project keeps its whole path, because there is no
/// shorter way to say it that is still true.
///
/// Deliberate break: have `making_in` use the stripped path whether or not
/// `strip_prefix` succeeded. An absolute answer then reports its own
/// directory as though it were under the project, which is a row saying
/// the file lands somewhere it will not.
#[test]
fn somewhere_outside_the_project_is_said_in_full() {
    let (_scratch, mut app) = reading("landing-away");
    dispatch::dispatch(&mut app, Command::FileNew);
    for _ in 0..80 {
        support::press(&mut app, KeyCode::Backspace);
    }
    let away = std::env::temp_dir().join("obelus-landing");
    support::type_text(&mut app, &away.join("scratch.rs").display().to_string());
    assert_eq!(
        landing(&mut app),
        away.join("").display().to_string(),
        "an answer outside the project"
    );
}

/// The tail is dropped whole where the answer and it do not both fit.
///
/// The answer is what the reader is looking at, and half a directory is
/// worse than none -- which is the rule the rest of this row follows for
/// every aside on it.
///
/// Deliberate break: drop the `start <` guard in `render_landing`. The
/// tail is written over the end of what the reader typed, so the answer on
/// screen is not the answer.
#[test]
fn the_tail_goes_rather_than_crowd_the_answer() {
    let (_scratch, mut app) = reading("landing-narrow");
    dispatch::dispatch(&mut app, Command::FileNew);
    support::type_text(&mut app, "a-rather-long-name-for-a-file.rs");

    // Wide, where they both fit and the tail is there to be lost.
    assert_eq!(landing(&mut app), support::as_shown("src/"), "with room");

    // And narrow, where the answer fills the row. What is on it then has
    // to be the answer and nothing else -- so what the row shows after the
    // label is the beginning of what was typed, rather than the beginning
    // of it with a directory written across the middle.
    let dump = support::render(&mut app, 30, 12);
    let row = support::text_block(&dump)
        .lines()
        .rfind(|row| row.contains('|'))
        .map(|row| row[row.find('|').expect("a divider") + 1..].trim_end())
        .unwrap_or_default();
    let shown = row.strip_prefix(" New file: ").unwrap_or(row);
    let typed = support::as_shown("src/a-rather-long-name-for-a-file.rs");
    assert!(
        typed.starts_with(shown),
        "the tail was written over the answer: the row shows {shown:?}\n{dump}"
    );
}

/// Something already at the path is said as that, however the system spells
/// the refusal.
///
/// `src` and `src/` are the same fact and two errnos: `create_new` answers
/// `AlreadyExists` for the first and `IsADirectory` for the second, and
/// which of them an OS picks is not a difference the reader is being told
/// about. So the path is asked rather than the error read.
///
/// Deliberate break: go back to matching `ErrorKind::AlreadyExists`. The
/// answer with a separator on the end falls through to the shape that
/// names no reason, and one fact has two sentences.
#[test]
fn a_directory_in_the_way_is_said_the_same_way_either_way() {
    for typed in ["src", "src/"] {
        let (_scratch, mut app) = reading(&format!("in-the-way-{}", typed.len()));
        dispatch::dispatch(&mut app, Command::FileNew);
        for _ in 0..80 {
            support::press(&mut app, KeyCode::Backspace);
        }
        support::type_text(&mut app, typed);
        support::press(&mut app, KeyCode::Enter);
        assert_eq!(
            app.note(),
            Some("src is already there"),
            "answering {typed:?}"
        );
    }
}

/// An answer that names the project itself is still named in what is said
/// about it.
///
/// `.` resolves to the project and strips to a path with nothing in it, so
/// the sentence came out with a blank where the name goes -- ` is already
/// there`, which is the shape `write_now` refuses in as many words.
///
/// Deliberate break: drop the fallback in `named`. The note starts with a
/// space and says nothing about what the reader answered.
#[test]
fn an_answer_that_strips_to_nothing_keeps_its_own_words() {
    let (_scratch, mut app) = reading("named-nothing");
    dispatch::dispatch(&mut app, Command::FileNew);
    for _ in 0..80 {
        support::press(&mut app, KeyCode::Backspace);
    }
    support::type_text(&mut app, ".");
    support::press(&mut app, KeyCode::Enter);
    assert_eq!(app.note(), Some(". is already there"));
}

/// A file somewhere along the path is said as that, and said relatively.
///
/// Deliberate break: put `parent.display()` back in place of the relative
/// name. The row is handed an absolute path -- `/tmp/obelus-…/src/hint.rs`
/// -- where every other path on it is written from the project, and on a
/// narrow row the whole sentence is dropped for length.
#[test]
fn a_file_along_the_path_is_said_relatively() {
    let (_scratch, mut app) = reading("along-the-path");
    dispatch::dispatch(&mut app, Command::FileNew);
    for _ in 0..80 {
        support::press(&mut app, KeyCode::Backspace);
    }
    support::type_text(&mut app, "src/hint.rs/inner.rs");
    support::press(&mut app, KeyCode::Enter);
    assert_eq!(
        app.note(),
        Some(format!("{} is not a directory", support::as_shown("src/hint.rs")).as_str()),
        "not the file in the way, said from the project"
    );
}

/// What Obelus says about a path is bounded, because the rest of it is the
/// reader's own text.
///
/// The row a note goes on is shared with the file's name, and one too long
/// for it is dropped whole -- so a note built from three hundred characters
/// of answer is a warning nobody sees.
///
/// Deliberate break: drop the `truncate_from_right` in `named`. The note is
/// three hundred characters long and the status row draws none of it.
#[test]
fn what_is_said_about_a_path_is_bounded() {
    let (_scratch, mut app) = reading("named-long");
    dispatch::dispatch(&mut app, Command::FileNew);
    for _ in 0..80 {
        support::press(&mut app, KeyCode::Backspace);
    }
    support::type_text(&mut app, &format!("{}.rs", "a".repeat(300)));
    support::press(&mut app, KeyCode::Enter);

    let said = app.note().unwrap_or_default().to_string();
    assert!(said.starts_with("Could not make "), "{said:?}");
    assert!(
        said.chars().count() < 60,
        "a note too long for the row: {} characters",
        said.chars().count()
    );
    // And the row actually draws it, which is the whole of the point.
    let dump = support::render(&mut app, 76, 12);
    assert!(
        support::text_block(&dump).contains("Could not make"),
        "the note was dropped for length:\n{dump}"
    );
}

/// With nothing open, the status row says it anyway.
///
/// It was drawn only beside a file's name, so the one reader most likely to
/// be told something -- `ob some-directory` opens on the welcome screen,
/// and making the first file in a project is offered right there -- pressed
/// a key, was refused, and watched nothing happen.
///
/// Deliberate break: drop the third arm in `StatusView`'s fallback. The
/// note is still held and the row is still blank.
#[test]
fn the_row_says_it_with_nothing_open() {
    let scratch = support::Scratch::new("said-with-nothing");
    scratch.write("src/hint.rs", "fn hint() {}\n");
    let mut app = App::new(Vec::new());
    app.working_directory_for_test(scratch.path().to_path_buf());
    app.statuses_for_test(std::collections::HashMap::new());
    support::lay_out(&mut app, 76, 20);

    dispatch::dispatch(&mut app, Command::FileNew);
    support::type_text(&mut app, "src");
    support::press(&mut app, KeyCode::Enter);

    let dump = support::render(&mut app, 76, 20);
    let row = support::text_block(&dump)
        .lines()
        .next_back()
        .map(|row| row[row.find('|').expect("a divider") + 1..].trim())
        .unwrap_or_default();
    assert_eq!(row, "src is already there", "\n{dump}");
}

/// A path that leaves the project is refused.
///
/// Which a rename is not: taking a path rather than a name is what moving
/// a file *is*, and the file it moves is one the reader already had. This
/// makes one, and where Obelus makes a file is the project it was opened
/// on -- `../../etc/hosts` typed into a question that opens blank is a
/// slip, not a plan.
///
/// Deliberate break: drop the `folded` guard in `make_file`. The file is
/// made outside the project and opened, and the only thing that ever said
/// so was the directory on the row, a keypress earlier.
#[test]
fn a_path_that_leaves_the_project_is_refused() {
    let (scratch, mut app) = reading("outside");
    // Named after this scratch directory, which carries the process's
    // number: a run that fails leaves the file it should not have made,
    // and a fixed name would fail every run after it for that reason
    // rather than for its own.
    let name = format!(
        "{}-escaped.rs",
        scratch
            .path()
            .file_name()
            .expect("a name")
            .to_string_lossy()
    );
    let outside = scratch.path().parent().expect("a parent").join(&name);
    assert!(!outside.exists(), "it is there before the test runs");

    for typed in [format!("../{name}"), format!("src/../../{name}")] {
        let typed = typed.as_str();
        dispatch::dispatch(&mut app, Command::FileNew);
        for _ in 0..80 {
            support::press(&mut app, KeyCode::Backspace);
        }
        support::type_text(&mut app, typed);
        support::press(&mut app, KeyCode::Enter);
        assert!(
            app.note()
                .is_some_and(|said| said.ends_with("is outside the project")),
            "answering {typed:?}: {:?}",
            app.note()
        );
        assert!(!outside.exists(), "{typed:?} made a file outside it");
    }

    // An absolute one is the same answer: what is asked is where the file
    // lands, not how the reader spelled it.
    let away = std::env::temp_dir().join("obelus-outside.rs");
    dispatch::dispatch(&mut app, Command::FileNew);
    for _ in 0..80 {
        support::press(&mut app, KeyCode::Backspace);
    }
    support::type_text(&mut app, &away.display().to_string());
    support::press(&mut app, KeyCode::Enter);
    assert!(
        app.note()
            .is_some_and(|said| said.ends_with("is outside the project")),
        "an absolute answer: {:?}",
        app.note()
    );
    assert!(!away.exists(), "an absolute answer made a file outside it");

    // And a path that walks out and back in is inside it, because where it
    // lands is what is being asked.
    dispatch::dispatch(&mut app, Command::FileNew);
    for _ in 0..80 {
        support::press(&mut app, KeyCode::Backspace);
    }
    support::type_text(&mut app, "src/../made.rs");
    support::press(&mut app, KeyCode::Enter);
    assert!(scratch.join("made.rs").exists(), "{:?}", app.note());
}
