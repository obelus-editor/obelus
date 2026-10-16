//! A file that is not UTF-8: read the way it is written, written back the
//! same way, and the reader told which way that is.

use crossterm::event::KeyCode;
use obelus_app::app::{App, dispatch};
use obelus_buffer::Buffer;
use obelus_command::Command;

use crate::support;

fn reading(name: &str, bytes: &[u8]) -> (support::Scratch, App, std::path::PathBuf) {
    let scratch = support::Scratch::new(name);
    let path = scratch.path().join("sample.rs");
    std::fs::write(&path, bytes).expect("writing the file");
    let mut app = App::new(vec![Buffer::open(&path).expect("opening it")]);
    app.working_directory_for_test(scratch.path().to_path_buf());
    support::lay_out(&mut app, 70, 12);
    (scratch, app, path)
}

fn text(app: &App) -> String {
    app.current_buffer()
        .expect("a buffer")
        .text()
        .rope()
        .to_string()
}

fn gbk(text: &str) -> Vec<u8> {
    encoding_rs::GBK.encode(text).0.into_owned()
}

const CHINESE: &str = "// 这是一个用中文写的注释，说明这段代码做什么\nfn main() {}\n";

/// A file in GBK opens as the words in it, says on the status row that it
/// is GBK, and goes back to disk in GBK.
///
/// Broken by having `Buffer::save` write the rope's UTF-8 whatever the file
/// was written in: the bytes on disk are not GBK's.
#[test]
fn a_file_in_gbk_is_read_and_written_back_in_gbk() {
    let (_scratch, mut app, path) = reading("encoding-gbk", &gbk(CHINESE));
    assert_eq!(text(&app), CHINESE, "not the words in the file");
    let dump = support::render(&mut app, 70, 12);
    assert!(
        support::text_block(&dump).contains("GBK"),
        "the status row does not say which encoding"
    );

    support::type_text(&mut app, "// ");
    dispatch::dispatch(&mut app, Command::FileSave);
    assert_eq!(
        std::fs::read(&path).expect("reading it back"),
        gbk(&format!("// {CHINESE}")),
        "not written back in GBK"
    );
    assert!(!app.current_buffer().expect("a buffer").is_dirty());
}

/// The encoding is an aside and not a warning: a file in GBK is in GBK for
/// good, and a word that never goes away drawn in the colour of `unsaved`
/// would teach the eye to pass over `unsaved`.
///
/// Broken by drawing the encoding in `status_stale` with the rest of the
/// marker: the two are drawn alike.
#[test]
fn the_encoding_is_not_drawn_as_a_warning() {
    let (_scratch, mut app, _path) = reading("encoding-quiet", &gbk(CHINESE));
    support::type_text(&mut app, "x");
    let dump = support::render(&mut app, 70, 12);
    assert_ne!(
        support::drawn_in(&dump, "GBK"),
        support::drawn_in(&dump, "unsaved"),
        "the encoding is drawn the way unsaved work is"
    );
}

/// UTF-8 says nothing on the status row: it is what nearly every file is,
/// and a word on every one of them would be a word nobody reads.
#[test]
fn utf8_is_not_mentioned() {
    let (_scratch, mut app, _path) = reading("encoding-plain", CHINESE.as_bytes());
    let dump = support::render(&mut app, 70, 12);
    assert!(
        !support::text_block(&dump).contains("UTF-8"),
        "UTF-8 is named on the status row"
    );
}

/// A byte order mark is not a character of the text -- the caret cannot
/// stand on it and the first line's columns do not count it -- and it is
/// put back in front when the file is written.
///
/// Broken by writing UTF-8 without the mark in `Written::encode`: the
/// file comes back without it.
#[test]
fn a_bom_stays_out_of_the_text_and_goes_back_on() {
    let (_scratch, mut app, path) = reading("encoding-bom", b"\xEF\xBB\xBFfn main() {}\n");
    assert_eq!(text(&app), "fn main() {}\n", "the mark is in the text");

    support::type_text(&mut app, "// ");
    dispatch::dispatch(&mut app, Command::FileSave);
    assert_eq!(
        std::fs::read(&path).expect("reading it back"),
        b"\xEF\xBB\xBF// fn main() {}\n",
        "the mark did not go back on"
    );
}

/// A character the file's encoding has no bytes for is not saved, and the
/// status row says which character and which encoding -- rather than a
/// file that says `&#128512;` where the reader typed something else.
///
/// Broken by having `Written::encode` let `encoding_rs` replace what it
/// cannot write: the file is written, with the replacement in it.
#[test]
fn what_the_encoding_cannot_write_is_not_saved() {
    let (_scratch, mut app, path) = reading("encoding-unwritable", &gbk(CHINESE));
    support::type_text(&mut app, "😀");
    dispatch::dispatch(&mut app, Command::FileSave);

    assert_eq!(
        std::fs::read(&path).expect("reading it back"),
        gbk(CHINESE),
        "the file was written"
    );
    assert!(app.current_buffer().expect("a buffer").is_dirty());
    assert!(
        app.note()
            .is_some_and(|said| said.contains("GBK cannot write 😀")),
        "the status row says {:?}",
        app.note()
    );
}

/// Choosing an encoding the file's bytes are not is writing it in that
/// encoding from the next save: the way out of the refusal above.
///
/// Broken by having `choose_encoding` only ever read the file again: the
/// file is not UTF-8, so nothing changes and the save is refused again.
#[test]
fn choosing_an_encoding_the_file_is_not_converts_it() {
    let (_scratch, mut app, path) = reading("encoding-convert", &gbk(CHINESE));
    support::type_text(&mut app, "😀");
    choose(&mut app, "UTF-8");
    dispatch::dispatch(&mut app, Command::FileSave);

    assert_eq!(
        std::fs::read_to_string(&path).expect("UTF-8 now"),
        format!("😀{CHINESE}"),
        "not written as UTF-8"
    );
}

/// Choosing an encoding the file's bytes are is reading them again as it:
/// the guess was wrong, and a file of a few accented letters is any of
/// several code pages.
///
/// Broken by having `choose_encoding` only ever write the file in the
/// encoding chosen: the text on screen does not change.
#[test]
fn choosing_an_encoding_the_file_is_reads_it_again() {
    let latin = b"// caf\xe9, na\xefve, r\xe9sum\xe9, d\xe9j\xe0 vu\n";
    let (_scratch, mut app, path) = reading("encoding-read-again", latin);
    let before = text(&app);
    assert!(before.contains("café"), "read as {before:?}");

    choose(&mut app, "windows-1251");
    assert!(
        text(&app).contains("cafй"),
        "not read again as windows-1251: {:?}",
        text(&app)
    );
    // Nothing was written, and there is nothing to write: the file is what
    // it was, read another way.
    assert!(!app.current_buffer().expect("a buffer").is_dirty());
    assert_eq!(std::fs::read(&path).expect("the file"), latin);
}

/// A BOM is put on or taken off by choosing the encoding with or without
/// it, and goes on or comes off when the file is written -- on a file with
/// nothing unwritten in it too, where choosing reads the file again.
///
/// Broken by not asking, after reading the file again, whether what was
/// read is what was chosen: the mark stays, and the row says it was read.
#[test]
fn choosing_with_or_without_the_mark_writes_it_so() {
    let (_scratch, mut app, path) = reading("encoding-mark-off", b"\xEF\xBB\xBFfn main() {}\n");
    choose(&mut app, "UTF-8");
    assert!(app.current_buffer().expect("a buffer").is_dirty());
    dispatch::dispatch(&mut app, Command::FileSave);
    assert_eq!(
        std::fs::read(&path).expect("reading it back"),
        b"fn main() {}\n",
        "the mark was not taken off"
    );

    let (_scratch, mut app, path) = reading("encoding-mark-on", b"fn main() {}\n");
    choose(&mut app, "UTF-8 with BOM");
    dispatch::dispatch(&mut app, Command::FileSave);
    assert_eq!(
        std::fs::read(&path).expect("reading it back"),
        b"\xEF\xBB\xBFfn main() {}\n",
        "the mark was not put on"
    );
}

/// An encoding chosen for the next save is not what is on disk: the file
/// touched with the bytes it already had has not changed, and the file
/// rewritten by somebody else is read the way it is written.
///
/// Broken by reading the disk in the document's encoding whenever it is not
/// UTF-8, as it once was: plain ASCII reads as GBK, so the touch is a
/// change, and UTF-8's Chinese -- here in pairs GBK also takes -- is taken
/// from disk as GBK's.
#[test]
fn an_encoding_not_yet_written_is_not_the_disk_s() {
    let scratch = support::Scratch::new("encoding-pending");
    let path = scratch.join("sample.rs");
    let ascii = "fn main() {}\n";
    std::fs::write(&path, ascii).expect("writing the file");
    let mut buffer = Buffer::open(&path).expect("opening it");
    buffer.write_as(named("GBK"));

    // Somebody else's formatter, which found nothing to change.
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(&path, ascii).expect("the same bytes again");
    assert!(
        !buffer.file_differs(),
        "the same bytes were taken for a change"
    );

    // Twelve bytes of UTF-8 that are also six characters of GBK.
    let theirs = "// 中文注释\nfn main() {}\n";
    std::fs::write(&path, theirs).expect("theirs");
    buffer.take_from_disk().expect("taking theirs");
    assert_eq!(
        buffer.text().rope().to_string(),
        theirs,
        "theirs was misread"
    );
    assert!(
        buffer.written().is_plain(),
        "read as {}",
        buffer.written().name()
    );
}

/// A guess is not held against the file: one somebody converted to UTF-8 is
/// read as UTF-8 when it is read again, and not as the GBK it was guessed
/// to be, which takes most of UTF-8's Chinese without an error.
///
/// Broken by trying the encoding the file was last read in first whoever
/// chose it: the text is mojibake, and the row still says GBK.
#[test]
fn a_guess_gives_way_to_a_file_converted_since() {
    let scratch = support::Scratch::new("encoding-converted");
    let path = scratch.join("sample.rs");
    std::fs::write(&path, gbk(CHINESE)).expect("writing the file");
    let mut buffer = Buffer::open(&path).expect("opening it");
    assert_eq!(buffer.written().name(), "GBK");

    let converted = "// 中文注释\nfn main() {}\n";
    std::fs::write(&path, converted).expect("converting it");
    buffer.reload().expect("reading it again");
    assert_eq!(buffer.text().rope().to_string(), converted, "misread");
    assert!(
        buffer.written().is_plain(),
        "still {}",
        buffer.written().name()
    );
}

/// A choice is: a file of plain ASCII is every encoding at once, and one the
/// reader said is Shift_JIS stays Shift_JIS when somebody rewrites it.
///
/// Broken by guessing again whoever chose the encoding: the file comes back
/// as UTF-8, and a save writes it so.
#[test]
fn a_choice_outlasts_a_rewrite() {
    let scratch = support::Scratch::new("encoding-chosen");
    let path = scratch.join("sample.rs");
    std::fs::write(&path, "fn main() {}\n").expect("writing the file");
    let mut buffer = Buffer::open(&path).expect("opening it");
    buffer
        .read_as(named("Shift_JIS"))
        .expect("reading it as Shift_JIS");

    std::fs::write(&path, "fn main() { 1 }\n").expect("rewriting it");
    buffer.reload().expect("reading it again");
    assert_eq!(buffer.written().name(), "Shift_JIS");
}

/// An agent writing a file nobody has open writes it the way it is
/// written: GBK stays GBK, a character GBK has no bytes for is refused
/// and leaves the file alone, and a mark the agent sent back is not
/// written twice.
///
/// Broken by writing the agent's text as it came: the file is UTF-8. And,
/// apart, by not taking the agent's mark off: the file starts with two.
#[test]
fn an_agent_writes_a_file_the_way_it_is_written() {
    let scratch = support::Scratch::new("encoding-agent");
    let path = scratch.join("other.rs");
    std::fs::write(&path, gbk(CHINESE)).expect("writing the file");
    let mut app = App::new(Vec::new());
    app.working_directory_for_test(scratch.path().to_path_buf());

    let theirs = format!("// 改过了\n{CHINESE}");
    assert!(app.write_for_agent_for_test(&path, &theirs));
    assert_eq!(
        std::fs::read(&path).expect("the file"),
        gbk(&theirs),
        "not GBK"
    );

    assert!(
        !app.write_for_agent_for_test(&path, "😀"),
        "a character GBK cannot write was written"
    );
    assert_eq!(
        std::fs::read(&path).expect("the file"),
        gbk(&theirs),
        "touched"
    );

    let marked = scratch.join("marked.rs");
    std::fs::write(&marked, b"\xEF\xBB\xBFfn main() {}\n").expect("writing it");
    assert!(app.write_for_agent_for_test(&marked, "\u{feff}fn main() { 1 }\n"));
    assert_eq!(
        std::fs::read(&marked).expect("the file"),
        b"\xEF\xBB\xBFfn main() { 1 }\n",
        "the mark was written twice"
    );
}

/// An encoding by the name the list gives it.
fn named(name: &str) -> obelus_encoding::Written {
    obelus_encoding::Written::offered()
        .into_iter()
        .find(|written| written.name() == name)
        .unwrap_or_else(|| panic!("{name} is not offered"))
}

/// Chooses an encoding from the list `choose-encoding` opens, by its name.
fn choose(app: &mut App, name: &str) {
    dispatch::dispatch(app, Command::EncodingSelect);
    let offered = app.picker().expect("the encodings").row_count();
    // To the top, then down to it: the list opens on the file's own.
    for _ in 0..offered {
        support::press(app, KeyCode::Up);
    }
    for _ in 0..offered {
        let here = app
            .picker()
            .and_then(|picker| picker.selected_item())
            .map(|item| item.label.clone());
        if here.as_deref() == Some(name) {
            support::press(app, KeyCode::Enter);
            return;
        }
        support::press(app, KeyCode::Down);
    }
    panic!("{name} is not offered");
}
