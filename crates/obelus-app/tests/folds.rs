//! What a file offers to fold, and what a reader does with it.
//!
//! The runs come from the shape of the file rather than from a server, so
//! these read real fixtures and check what Obelus makes of them -- which is
//! also the only way to test folding for the languages no server answers
//! about.

mod support;

use crossterm::event::KeyCode;
use obelus_app::app::App;
use obelus_buffer::{Buffer, folds};
use obelus_command::Command;
use obelus_text::{Text, coordinates::LineNumber};

/// The runs a file offers, as (start, end, where the run stops on its last
/// line).
fn runs(source: &str) -> Vec<(usize, usize, Option<usize>)> {
    folds::of(&Text::from_string(source))
        .iter()
        .map(|fold| (fold.from.get(), fold.to.get(), fold.tail.map(|at| at.get())))
        .collect()
}

/// A run opens where the indentation deepens and closes where it comes
/// back, and when the line it closes on begins with a bracket the run stops
/// just before it -- which is what puts the bracket beside the mark.
#[test]
fn indentation_opens_a_run_and_a_bracket_closes_it() {
    let found = runs("fn main() {\n    if ready {\n        go();\n    }\n}\n");
    assert_eq!(
        found,
        [(0, 4, Some(0)), (1, 3, Some(4))],
        "not the runs the indentation describes"
    );

    // One per line they start on, by construction: a reader has one line to
    // press a key on and one mark to press it by, so two runs starting
    // together would be a mark that folds one of them and no way to say
    // which.
    let starts: Vec<usize> = found.iter().map(|(from, _, _)| *from).collect();
    let mut once = starts.clone();
    once.dedup();
    assert_eq!(starts, once, "two runs start on the same line");
}

/// A block that closes with nothing takes everything down to the last line
/// with anything on it, and the row then has only the mark. `def ready():
/// ...` comes out of the same rule as `if ready { ... }`, without a word
/// about Python in it.
#[test]
fn a_block_that_closes_with_nothing_ends_where_it_runs_out() {
    assert_eq!(
        runs("def shout(name):\n    if not name:\n        return \"\"\n    return name\n\nx = 1\n"),
        [(0, 3, None), (1, 2, None)],
        "the run took the blank line, or stopped early"
    );
}

/// A file with no indentation offers nothing. A TOML file is a list of
/// tables at column zero and so is most markdown: there is no block for a
/// reader to close, and a mark offering to hide "the rest of the file from
/// here" is not the same offer.
#[test]
fn a_file_with_nothing_indented_folds_nowhere() {
    assert!(
        runs("[package]\nname = \"sample\"\n\n[dependencies]\nserde = \"1\"\n").is_empty(),
        "a flat file offered a fold"
    );
}

/// Blank lines inside a block are walked past rather than ending it, and
/// blank lines after one belong to whatever comes next.
#[test]
fn blank_lines_belong_to_what_comes_after_them() {
    assert_eq!(
        runs("fn a() {\n\n    1;\n\n}\n\nfn b() {}\n"),
        [(0, 4, Some(0))],
        "a blank line ended the run, or was swallowed by it"
    );
}

/// The fixture with the runs it offers: a struct, an impl, the method
/// inside it and the `if` inside that, each closing on the brace its last
/// line begins with. The same four a language server answers with for this
/// file, which is the point.
fn nested() -> Buffer {
    support::open_fixture("nested.rs")
}

/// A run inside a folded run keeps its own state. Folding a type over a
/// folded method and opening it again gives the method back folded, because
/// that is where the reader left it.
#[test]
fn a_fold_inside_a_fold_is_where_the_reader_left_it() {
    let mut app = App::new(vec![nested()]);
    support::lay_out(&mut app, 60, 20);

    // The `if`, then the method around it, then the `impl` around that.
    for line in [8, 7, 6] {
        support::press_control(&mut app, 'l');
        support::type_text(&mut app, &line.to_string());
        support::press(&mut app, KeyCode::Enter);
        support::press_alt_key(&mut app, KeyCode::Char('f'));
    }
    let folds = app.current_buffer().expect("a file").folds();
    for line in [5usize, 6, 7] {
        assert!(
            folds.is_folded_at(LineNumber::new(line)),
            "line {} did not fold",
            line + 1
        );
    }

    // Open the outermost. The two inside it are still folded.
    support::press_alt_key(&mut app, KeyCode::Char('f'));
    let folds = app.current_buffer().expect("a file").folds();
    assert!(
        !folds.is_folded_at(LineNumber::new(5)),
        "the impl stayed folded"
    );
    assert!(
        folds.is_folded_at(LineNumber::new(6)),
        "the method lost its fold when the impl opened"
    );
    assert!(
        folds.hides(LineNumber::new(8)),
        "the method opened along with the impl"
    );
}

/// The lines go, the row that stands for them is marked, and what is left
/// of the run's last line comes up beside the mark -- which is the whole of
/// why a range is worth more than a pair of line numbers.
#[test]
fn folding_takes_the_lines_off_the_screen() {
    let mut app = App::new(vec![nested()]);
    support::lay_out(&mut app, 60, 20);
    support::press_control(&mut app, 'l');
    support::type_text(&mut app, "7");
    support::press(&mut app, KeyCode::Enter);
    support::press_alt_key(&mut app, KeyCode::Char('f'));

    let dump = support::render(&mut app, 60, 20);
    let text = support::text_block(&dump);
    assert!(
        !text.contains("is_empty"),
        "a line inside the fold is still on screen"
    );
    assert!(
        text.contains('\u{25b8}'),
        "no mark says the line has more behind it"
    );
    assert!(
        text.contains('\u{25be}'),
        "an open run said nothing about being foldable"
    );
    assert!(text.contains("impl Thing"), "the file above the fold went");
    // The server said the run stops at the brace on its last line, so the
    // brace is what is left of that line, and the row reads as a block.
    assert!(
        text.contains("pub fn shout(&self) -> String { \u{2026} }"),
        "the folded row does not close the block it folded:\n{text}"
    );
}

/// What is left of the run's last line is what the row shows, so a block
/// closes as a block. The run stops just before the brace, which is how the
/// brace is still there to come up beside the mark.
#[test]
fn a_block_closes_as_a_block() {
    let mut app = App::new(vec![nested()]);
    support::lay_out(&mut app, 60, 20);
    support::press_control(&mut app, 'l');
    support::type_text(&mut app, "7");
    support::press(&mut app, KeyCode::Enter);
    support::press_alt_key(&mut app, KeyCode::Char('f'));

    let dump = support::render(&mut app, 60, 20);
    assert!(
        support::text_block(&dump).contains("pub fn shout(&self) -> String { \u{2026} }"),
        "the block does not close:\n{}",
        support::text_block(&dump)
    );
}

/// A block that closes with nothing shows the mark and nothing else. There
/// is no closing line to bring up, and Obelus does not invent one.
#[test]
fn a_block_with_no_bracket_shows_only_the_mark() {
    let mut app = App::new(vec![support::open_fixture("blocks.py")]);
    support::lay_out(&mut app, 60, 20);
    support::press_alt_key(&mut app, KeyCode::Char('f'));

    let dump = support::render(&mut app, 60, 20);
    let text = support::text_block(&dump);
    assert!(
        text.contains("def shout(name): \u{2026}"),
        "the mark is not there:\n{text}"
    );
    assert!(
        !text.contains("return name"),
        "a line of code was dragged up onto the folded row:\n{text}"
    );
}

/// The closing text is the file's and is drawn the colour it would be at
/// home; the mark is Obelus's own and is drawn the way its notes are. A
/// brace that changed colour on its way up the screen would read as
/// something else.
#[test]
fn the_closing_text_keeps_the_colour_it_had() {
    let mut app = App::new(vec![nested()]);
    support::lay_out(&mut app, 70, 20);
    for _ in 0..6 {
        support::press(&mut app, KeyCode::Down);
    }
    support::press_alt_key(&mut app, KeyCode::Char('f'));

    let dump = support::render(&mut app, 70, 20);
    let (row, styles) = support::text_block(&dump)
        .lines()
        .zip(support::style_block(&dump).lines())
        .find(|(row, _)| row.contains("pub fn shout"))
        .expect("the folded row");

    // Counted in characters: the mark is three bytes wide and one cell.
    let cell_of = |needle: char| {
        let bytes = row.rfind(needle).expect("a glyph of the row");
        row[..bytes].chars().count()
    };
    let style_at = |at: usize| styles.chars().nth(at).expect("a style cell");

    assert_eq!(
        style_at(cell_of('}')),
        style_at(cell_of('{')),
        "the closing text did not keep its colour:\n{row}\n{styles}"
    );
    assert_ne!(
        style_at(cell_of('\u{2026}')),
        style_at(cell_of('}')),
        "the mark is drawn as if it were in the file"
    );
}

/// Walking down from a folded line lands after the run, because the rows in
/// between are not rows of the screen at all.
#[test]
fn the_cursor_steps_over_what_is_folded() {
    let mut app = App::new(vec![nested()]);
    support::lay_out(&mut app, 60, 20);
    support::press_control(&mut app, 'l');
    support::type_text(&mut app, "7");
    support::press(&mut app, KeyCode::Enter);
    support::press_alt_key(&mut app, KeyCode::Char('f'));

    support::press(&mut app, KeyCode::Down);
    assert_eq!(
        app.current_buffer().expect("a file").cursor().line,
        LineNumber::new(12),
        "one step down did not clear the folded method"
    );
}

/// A reader cannot be left standing on a line that is not on screen, so
/// folding around them brings them to the line the run now is.
#[test]
fn folding_over_the_cursor_brings_it_out() {
    let mut app = App::new(vec![nested()]);
    support::lay_out(&mut app, 60, 20);
    support::press_control(&mut app, 'l');
    support::type_text(&mut app, "9");
    support::press(&mut app, KeyCode::Enter);
    support::press_alt_key(&mut app, KeyCode::Char('f'));

    assert_eq!(
        app.current_buffer().expect("a file").cursor().line,
        LineNumber::new(7),
        "the cursor was left inside the fold"
    );
}

/// A line with nothing to fold is a line where the key is dim, and a dim
/// key does nothing and says nothing: `AFoldHere` has already answered, and
/// a note would be a second answer to a settled question.
#[test]
fn a_line_with_nothing_to_fold_is_a_key_that_does_nothing() {
    let mut app = App::new(vec![nested()]);
    support::lay_out(&mut app, 60, 20);
    support::press_control(&mut app, 'l');
    support::type_text(&mut app, "3");
    support::press(&mut app, KeyCode::Enter);
    assert!(
        app.offers(Command::Fold),
        "folding was not offered on a line inside a run"
    );
    support::press_alt_key(&mut app, KeyCode::Char('f'));
    assert_eq!(app.note(), None, "a line inside a run refused to fold");

    support::press_control(&mut app, 'l');
    support::type_text(&mut app, "5");
    support::press(&mut app, KeyCode::Enter);
    assert!(
        !app.offers(Command::Fold),
        "folding was offered on a line with nothing to fold"
    );
    support::press_alt_key(&mut app, KeyCode::Char('f'));
    assert_eq!(app.note(), None, "a dim key answered a settled question");
    let folds = app.current_buffer().expect("a file").folds();
    assert!(
        !folds.is_folded_at(LineNumber::new(4)),
        "the blank line folded something"
    );
    assert!(
        !folds.hides(LineNumber::new(5)),
        "pressing on the blank line hid the impl below it"
    );
}

/// Being taken somewhere opens whatever was hiding it. A reader who asked
/// for a line has said that line is worth seeing; leaving them on the
/// folded row above it, with the status row naming a line that is not on
/// screen, answers a different request.
#[test]
fn arriving_at_a_hidden_line_opens_what_hides_it() {
    let mut app = App::new(vec![nested()]);
    support::lay_out(&mut app, 60, 20);
    support::press_control(&mut app, 'l');
    support::type_text(&mut app, "7");
    support::press(&mut app, KeyCode::Enter);
    support::press_alt_key(&mut app, KeyCode::Char('f'));
    assert!(
        app.current_buffer()
            .expect("a file")
            .folds()
            .hides(LineNumber::new(8)),
        "the method did not fold"
    );

    support::press_control(&mut app, 'l');
    support::type_text(&mut app, "9");
    support::press(&mut app, KeyCode::Enter);

    let buffer = app.current_buffer().expect("a file");
    assert!(
        !buffer.folds().hides(LineNumber::new(8)),
        "the fold stayed shut over the line asked for"
    );
    assert_eq!(
        buffer.cursor().line,
        LineNumber::new(8),
        "the cursor did not arrive"
    );
    let dump = support::render(&mut app, 60, 20);
    assert!(
        support::text_block(&dump).contains("is_empty"),
        "the line asked for is not drawn"
    );
}

/// The two keys that take the whole file at once, and the conditions that
/// keep either of them from being offered when it has nothing to do.
#[test]
fn folding_everything_and_opening_it_again() {
    let mut app = App::new(vec![nested()]);
    support::lay_out(&mut app, 60, 20);

    assert!(app.offers(Command::FoldAll), "nothing offered to fold");
    assert!(
        !app.offers(Command::UnfoldAll),
        "opening everything was offered with nothing folded"
    );

    obelus_app::app::dispatch::dispatch(&mut app, Command::FoldAll);
    let dump = support::render(&mut app, 60, 20);
    let text = support::text_block(&dump);
    assert!(
        text.contains("pub struct Thing"),
        "the line a run starts on went with it"
    );
    assert!(
        !text.contains("pub name"),
        "a line inside a folded run is still drawn"
    );
    assert!(
        !text.contains("is_empty"),
        "a run inside a folded run is still drawn"
    );
    assert!(
        !app.offers(Command::FoldAll),
        "folding everything was offered again with everything folded"
    );
    assert!(app.offers(Command::UnfoldAll), "nothing offered to open");

    obelus_app::app::dispatch::dispatch(&mut app, Command::UnfoldAll);
    let dump = support::render(&mut app, 60, 20);
    let text = support::text_block(&dump);
    assert!(text.contains("pub name"), "opening everything kept a fold");
    assert!(text.contains("is_empty"), "opening everything missed a run");
}

/// The rows the screen has are the rows it draws. A folded run contributes
/// none, so everything below it moves up -- the caret included, which is the
/// half the view cannot see for itself: the terminal is told where to put
/// its cursor, and a count that included the hidden lines would put it
/// below the line it is really on.
#[test]
fn the_caret_sits_on_the_row_the_line_is_drawn_on() {
    /// Which row of the screen the caret was put on.
    fn row_of(dump: &str) -> String {
        support::cursor_line(dump)
            .split(',')
            .nth(1)
            .expect("a row")
            .to_string()
    }

    let mut app = App::new(vec![nested()]);
    support::lay_out(&mut app, 60, 20);
    // Walked rather than jumped: arriving somewhere centres the screen on
    // it, and this is about which row a line is drawn on with the screen
    // left where it was.
    for _ in 0..12 {
        support::press(&mut app, KeyCode::Down);
    }
    let dump = support::render(&mut app, 60, 20);
    assert_eq!(row_of(&dump), "12", "the file is not drawn a line to a row");

    // Fold the method, five lines of which are then off the screen, and step
    // over it.
    for _ in 0..6 {
        support::press(&mut app, KeyCode::Up);
    }
    support::press_alt_key(&mut app, KeyCode::Char('f'));
    support::press(&mut app, KeyCode::Down);
    assert_eq!(
        app.current_buffer().expect("a file").cursor().line,
        LineNumber::new(12),
        "the step did not clear the fold"
    );

    let dump = support::render(&mut app, 60, 20);
    assert_eq!(
        row_of(&dump),
        "7",
        "the caret counted the lines that are not on screen"
    );
}

/// A re-read drops the runs along with what was folded.
///
/// Both belonged to the file that has just been replaced: a run kept across
/// the re-read would hide whichever lines now sit at those numbers, and the
/// list of runs is a claim about a version the reader is no longer looking
/// at. The server is asked again, and until it answers the file does not
/// fold.
#[test]
fn re_reading_the_file_drops_what_was_folded() {
    let scratch = support::Scratch::new("folds");
    let path = scratch.write("one.rs", "fn one() {\n    1;\n}\n");
    let mut buffer = Buffer::open(&path).expect("opening it");

    assert!(buffer.toggle_fold(LineNumber::new(0)), "nothing folded");
    assert!(
        buffer.folds().hides(LineNumber::new(1)),
        "the run did not hide its lines"
    );

    std::fs::write(&path, "// A line nobody folded.\nfn one() {\n    1;\n}\n")
        .expect("rewriting it");
    assert!(buffer.reload().expect("re-reading"), "nothing was re-read");

    assert!(
        !buffer.folds().any_folded(),
        "a fold survived the file it was about"
    );
    // And the new text was asked what it offers: the run now starts a line
    // lower, because a line was put above it.
    assert!(
        buffer.folds().offered_at(LineNumber::new(2)).is_some(),
        "the new text was not asked what it folds"
    );
}

/// What is highlighted is what is *drawn*, not the first screenful of
/// lines. With a run closed at the top of the screen, the rows below it are
/// lines much further down the file, and a highlight range that stopped at
/// `top + height` would leave every one of them outside it -- which is not
/// a subtle failure: the code below the fold is simply drawn in the plain
/// foreground.
#[test]
fn what_is_below_a_fold_is_still_coloured() {
    let mut app = App::new(vec![support::open_fixture("coloured.rs")]);
    support::lay_out(&mut app, 50, 6);
    support::press_alt_key(&mut app, KeyCode::Char('f'));

    let dump = support::render(&mut app, 50, 6);
    let (row, styles) = support::text_block(&dump)
        .lines()
        .zip(support::style_block(&dump).lines())
        .find(|(row, _)| row.contains("A comment far below"))
        .expect("the comment below the fold");

    // Whatever the theme calls it, a comment is not drawn as plain text --
    // and plain text is what the row beside it, the file's own name in the
    // gutter, is not either. The cell the comment starts on is the one to
    // ask about.
    let bytes = row.find("// A comment").expect("the comment");
    let at = row[..bytes].chars().count();
    let comment = styles.chars().nth(at).expect("a style cell");
    let plain = styles
        .chars()
        .last()
        .expect("the style of the cell past the end of the line");
    assert_ne!(
        comment, plain,
        "the comment below the fold lost its colour:\n{row}\n{styles}"
    );
}

/// Folding over an opened hunk closes it. Its removed lines hang above a
/// line of the file, so hiding that line leaves them undrawn -- and a caret
/// in them is a caret nobody can see, on a row the status bar would still
/// name.
#[test]
fn folding_over_an_opened_hunk_closes_it() {
    use obelus_buffer::{Motion, TextArea};

    let scratch = support::Scratch::new("blockfold");
    let path = scratch.write(
        "outer.rs",
        "fn outer() {\n    let a = 1;\n    let b = 2;\n}\n",
    );
    let mut buffer = Buffer::open(&path).expect("opening it");

    // A hunk opened above line three, with the caret walked into it.
    buffer.open_block(LineNumber::new(2), &["    let gone = 0;".to_string()]);
    buffer.place_cursor(
        LineNumber::new(2),
        obelus_text::coordinates::CharColumn::new(0),
    );
    buffer.move_cursor(
        Motion::Up,
        TextArea {
            width: 40,
            height: 10,
            wrap: false,
        },
    );
    assert!(buffer.in_block().is_some(), "the caret did not walk in");

    assert!(buffer.toggle_fold(LineNumber::new(0)), "nothing folded");
    assert!(
        buffer.folds().hides(LineNumber::new(2)),
        "the run did not hide the line the hunk hangs above"
    );
    assert!(
        buffer.blocks().is_empty(),
        "the hunk is still open over a line that is not on screen"
    );
    assert!(
        buffer.in_block().is_none(),
        "the caret was left in rows nobody draws"
    );
}

/// The scrollbar is a picture of the document at the height of the screen,
/// and a closed run makes the document shorter. Drawn from the file's own
/// line numbers it would say the reader is at the top of something long
/// while the whole of it is in front of them.
#[test]
fn the_scrollbar_measures_what_is_shown() {
    let scratch = support::Scratch::new("bar");
    let mut source = String::from("fn wrapping() {\n");
    for line in 0..30 {
        source.push_str(&format!("    let a{line} = {line};\n"));
    }
    source.push_str("}\n");
    for line in 0..28 {
        source.push_str(&format!("const B{line}: u32 = {line};\n"));
    }
    let path = scratch.write("wrapping.rs", &source);

    let mut app = App::new(vec![Buffer::open(&path).expect("opening")]);
    support::lay_out(&mut app, 50, 10);
    // The track and the thumb are the same block glyph in two colours, so
    // the thumb is counted by style: the last cell of each row, and which
    // of the two styles the topmost row uses.
    let thumb = |dump: &str| {
        let rows: Vec<char> = support::style_block(dump)
            .lines()
            .filter(|row| !row.is_empty())
            .filter_map(|row| row.chars().last())
            .collect();
        let first = rows.first().copied().unwrap_or(' ');
        rows.iter().take_while(|style| **style == first).count()
    };

    // Sixty lines in ten rows: a short thumb.
    let dump = support::render(&mut app, 50, 10);
    let before = thumb(&dump);
    assert!(
        before > 0,
        "no thumb at all:\n{}",
        support::text_block(&dump)
    );

    // Closing the one run takes thirty lines out of the document, so the
    // part in front of the reader is twice what it was.
    support::press_alt_key(&mut app, KeyCode::Char('f'));
    let dump = support::render(&mut app, 50, 10);
    let after = thumb(&dump);
    assert!(
        after > before,
        "the bar still measures the lines nobody can see: {before} rows before, \
         {after} after:\n{}",
        support::text_block(&dump)
    );
}

/// The width the cursor counts in has to be the width the view draws in.
///
/// `App::text_area` and `EditorView::render` both work out what comes
/// before and after the text, and a fold column counted by one and not the
/// other puts the caret a cell past the end of the text -- into the
/// scrollbar, where the character it claims to be on is never drawn.
#[test]
fn the_cursor_counts_in_the_width_the_view_draws() {
    let scratch = support::Scratch::new("width");
    let wide: String = std::iter::repeat_n('x', 60).collect();
    let path = scratch.write(
        "wide.rs",
        &format!("fn f() {{\n    let a = \"{wide}\";\n}}\n"),
    );
    let mut app = App::new(vec![Buffer::open(&path).expect("opening")]);
    support::lay_out(&mut app, 40, 10);

    // Down onto the long line, then out along it past the width of the
    // text: the view has to scroll sideways, and the caret has to stay
    // inside the text rather than stepping onto the bar.
    support::press(&mut app, KeyCode::Down);
    for _ in 0..40 {
        support::press(&mut app, KeyCode::Right);
    }
    let dump = support::render(&mut app, 40, 10);
    let cell: u16 = support::cursor_line(&dump)
        .split(',')
        .next()
        .expect("a cell")
        .parse()
        .expect("a number");
    assert!(
        cell < 39,
        "the caret is on the scrollbar's column:\n{}",
        support::text_block(&dump)
    );
}

/// Walking up out of an opened hunk lands on the first line that is shown,
/// not simply the line before it: a run folded away above the hunk would
/// otherwise take the caret with it, onto a line the status bar names and
/// nobody can see.
#[test]
fn walking_up_out_of_a_hunk_clears_what_is_folded() {
    use obelus_buffer::{Motion, TextArea};

    let scratch = support::Scratch::new("upfold");
    let mut source = String::from("fn hidden() {\n");
    for line in 0..6 {
        source.push_str(&format!("    let a{line} = {line};\n"));
    }
    source.push_str("}\nlet after = 1;\n");
    let path = scratch.write("hidden.rs", &source);
    let mut buffer = Buffer::open(&path).expect("opening it");

    // The run hides lines two to eight, and the hunk hangs above line nine
    // -- the first line below the run, so the line *before* it is one of
    // the ones that went.
    assert!(buffer.toggle_fold(LineNumber::new(0)), "nothing folded");
    assert!(
        buffer.folds().hides(LineNumber::new(7)),
        "the run did not hide the line above the hunk"
    );
    buffer.open_block(LineNumber::new(8), &["    let gone = 0;".to_string()]);
    buffer.place_cursor(
        LineNumber::new(8),
        obelus_text::coordinates::CharColumn::new(0),
    );
    let area = TextArea {
        width: 40,
        height: 10,
        wrap: false,
    };
    buffer.move_cursor(Motion::Up, area);
    assert!(buffer.in_block().is_some(), "the caret did not walk in");

    // Up again, out of the top of the block.
    buffer.move_cursor(Motion::Up, area);
    assert!(buffer.in_block().is_none(), "the caret did not walk out");
    assert!(
        !buffer.folds().hides(buffer.cursor().line),
        "the caret left the hunk onto a line that is folded away: {:?}",
        buffer.cursor().line
    );
}

/// A block with nothing in it is not a run. The line below it is no deeper,
/// so there is nothing between the two to hide -- and a mark offering to
/// fold `fn a() {}` into `fn a() { … }`, where the mark stands for no lines
/// at all, is a mark that lies about what it is for.
#[test]
fn an_empty_block_is_not_a_run() {
    assert!(runs("fn a() {\n}\n").is_empty(), "an empty block folded");
    assert!(
        runs("foo(\n)\n").is_empty(),
        "an empty argument list folded"
    );
}

/// What a file offers to fold has to follow every edit -- and working it
/// out is a pass over the whole file, so it is skipped where the edit
/// cannot have moved anything. That shortcut is only worth having if it is
/// never wrong, which is a thing to check against the answer it skipped.
mod after_an_edit {
    use super::{App, Buffer, Command, KeyCode, LineNumber, folds, support};

    /// Every run the file offers, as the fold machinery has them.
    fn offered(app: &App) -> Vec<folds::Fold> {
        let buffer = app.current_buffer().expect("a buffer");
        (0..buffer.text().line_count())
            .filter_map(|line| {
                // The one that *starts* here: `offered_at` answers about
                // the line, and a run covers all of its own.
                buffer
                    .folds()
                    .offered_at(LineNumber::new(line))
                    .filter(|fold| fold.from.get() == line)
            })
            .collect()
    }

    /// And as a fresh reading of the file gives them.
    fn fresh(app: &App) -> Vec<folds::Fold> {
        folds::of(app.current_buffer().expect("a buffer").text())
    }

    fn editing(name: &str, source: &str) -> (support::Scratch, App) {
        let scratch = support::Scratch::new(name);
        let path = scratch.path().join("sample.rs");
        std::fs::write(&path, source).expect("writing the file");
        let mut app = App::new(vec![Buffer::open(&path).expect("opening it")]);
        app.working_directory_for_test(scratch.path().to_path_buf());
        support::lay_out(&mut app, 60, 12);
        (scratch, app)
    }

    const SOURCE: &str =
        "fn one() {\n    let a = 1;\n    let b = 2;\n}\n\nfn two() {\n    three();\n}\n";

    /// Typing inside a line moves nothing, and the shortcut takes it.
    #[test]
    fn typing_inside_a_line_leaves_the_runs_where_they_were() {
        let (_scratch, mut app) = editing("fold-typing", SOURCE);
        support::press(&mut app, KeyCode::Down);
        support::press(&mut app, KeyCode::End);
        support::type_text(&mut app, "; // a note");
        assert_eq!(offered(&app), fresh(&app));
    }

    /// Typing a bracket at the head of a line changes what closes a run,
    /// which changes where the run stops.
    #[test]
    fn a_bracket_at_the_head_of_a_line_changes_where_a_run_stops() {
        let (_scratch, mut app) = editing("fold-bracket", SOURCE);
        support::press(&mut app, KeyCode::Down);
        support::press(&mut app, KeyCode::Home);
        support::type_text(&mut app, "}");
        assert_eq!(offered(&app), fresh(&app));
    }

    /// A line can keep its indent and still stop closing anything: what a
    /// run ends on is the line that *starts* with a bracket, and putting
    /// something in front of that bracket is not the same line any more.
    #[test]
    fn what_a_line_starts_with_changes_where_a_run_stops() {
        let (_scratch, mut app) = editing(
            "fold-closer",
            "fn one() {\n    if a {\n        b();\n    }\n}\n",
        );
        for _ in 0..3 {
            support::press(&mut app, KeyCode::Down);
        }
        support::press(&mut app, KeyCode::End);
        support::press(&mut app, KeyCode::Left);
        support::type_text(&mut app, "x");
        assert_eq!(
            app.current_buffer()
                .expect("a buffer")
                .text()
                .line(LineNumber::new(3))
                .to_string(),
            "    x}",
            "not the edit this test meant to make"
        );
        assert_eq!(offered(&app), fresh(&app));
    }

    /// Typing a space in front of a line indents it, which is the whole of
    /// what a run is made of.
    #[test]
    fn indenting_a_line_changes_the_runs() {
        let (_scratch, mut app) = editing("fold-indent", SOURCE);
        support::press(&mut app, KeyCode::Down);
        support::press(&mut app, KeyCode::Home);
        support::type_text(&mut app, "    ");
        assert_eq!(offered(&app), fresh(&app));
    }

    /// And a new line is a line every run under it has to count.
    #[test]
    fn splitting_a_line_changes_the_runs() {
        let (_scratch, mut app) = editing("fold-split", SOURCE);
        support::press(&mut app, KeyCode::Down);
        support::press(&mut app, KeyCode::End);
        support::press(&mut app, KeyCode::Enter);
        assert_eq!(offered(&app), fresh(&app));
    }

    /// Emptying a line takes it out of every run's reckoning: a line with
    /// nothing on it says nothing about how deep anything is.
    #[test]
    fn emptying_a_line_changes_the_runs() {
        let (_scratch, mut app) = editing("fold-empty", SOURCE);
        support::press(&mut app, KeyCode::Down);
        support::press(&mut app, KeyCode::End);
        for _ in 0..40 {
            support::press(&mut app, KeyCode::Backspace);
        }
        assert_eq!(offered(&app), fresh(&app));
    }

    /// Whatever the reader folded is still folded, which is why this is not
    /// simply re-offered from scratch every time.
    #[test]
    fn what_the_reader_folded_survives_typing() {
        let (_scratch, mut app) = editing("fold-kept", SOURCE);
        obelus_app::app::dispatch::dispatch(&mut app, Command::Fold);
        assert!(
            app.current_buffer().expect("a buffer").folds().any_folded(),
            "nothing was folded to begin with"
        );

        support::press(&mut app, KeyCode::Down);
        support::type_text(&mut app, "x");
        assert!(
            app.current_buffer().expect("a buffer").folds().any_folded(),
            "typing unfolded what the reader had folded"
        );
    }
}

/// The line beside a line is the next one that is *shown*: a caret cannot
/// be put on a line nobody can see, and stepping off the end of the line a
/// closed run hangs on has to clear the whole run.
#[test]
fn an_arrow_steps_over_a_closed_run() {
    let scratch = support::Scratch::new("folds-arrows");
    let path = scratch.write("one.rs", "fn one() {\n    1;\n    2;\n}\nlast\n");
    let mut buffer = Buffer::open(&path).expect("opening it");
    let area = obelus_buffer::TextArea {
        width: 40,
        height: 10,
        wrap: false,
    };

    assert!(buffer.toggle_fold(LineNumber::new(0)), "nothing folded");
    assert!(
        buffer.folds().hides(LineNumber::new(1)),
        "the run did not hide its lines"
    );

    // Off the end of the line the run hangs on, which is past every line
    // it hides.
    buffer.place_cursor(
        LineNumber::new(0),
        obelus_text::coordinates::CharColumn::new(10),
    );
    buffer.move_cursor(obelus_buffer::Motion::Right, area);
    assert_eq!(
        (buffer.cursor().line.get(), buffer.cursor().column.get()),
        (4, 0),
        "the caret landed inside the run"
    );

    // And back the same way.
    buffer.move_cursor(obelus_buffer::Motion::Left, area);
    assert_eq!(
        (buffer.cursor().line.get(), buffer.cursor().column.get()),
        (0, 10),
        "stepping back landed inside the run"
    );
}

/// The mark in the margin folds the line when it is clicked.
///
/// A click to the left of the words used to mean one thing everywhere: the
/// start of that row. Which is right for the numbers -- pointing left of
/// the words is how a whole line is reached -- and wrong for the column
/// beside them. The fold mark is the picture of the key that folds, and a
/// mark like that is clicked everywhere a reader has met one.
///
/// It goes to that line first, because the key asks about the line the
/// caret is on: the reader pointed at a line, so that is the line.
///
/// Broken deliberately by taking the fold column back into "the start of
/// that row", which puts the caret there and leaves the run shut.
#[test]
fn the_fold_mark_folds_the_line_it_is_on() {
    let mut app = App::new(vec![nested()]);
    support::lay_out(&mut app, 60, 20);

    // The row of a line with something to fold, and the column its mark is
    // drawn in: the numbers, and then one cell for the mark.
    let lines = app.current_buffer().expect("a file").text().line_count();
    let mark = obelus_ui::editor::gutter_width(lines);
    assert_eq!(
        obelus_ui::editor::margin_at(mark, lines, false, true),
        obelus_ui::editor::Margin::Folds,
        "that is not the column the fold marks are drawn in"
    );

    // The second line of this file opens a run -- the first is a comment
    // about the file -- so that is the row whose mark is there.
    let area = app.editor_area_for_test();
    app.handle(obelus_app::event::Event::Pointer {
        kind: obelus_app::event::Pointer::Pressed,
        x: area.x + mark,
        y: area.y + 1,
    });
    assert!(
        app.current_buffer().expect("a file").folds().any_folded(),
        "the click on the mark folded nothing"
    );
    assert_eq!(
        app.current_buffer().expect("a file").cursor().line.get(),
        1,
        "the click did not go to the line it was on"
    );

    // And again, it opens.
    app.handle(obelus_app::event::Event::Pointer {
        kind: obelus_app::event::Pointer::Pressed,
        x: area.x + mark,
        y: area.y + 1,
    });
    assert!(
        !app.current_buffer().expect("a file").folds().any_folded(),
        "the same click did not open it again"
    );

    // While the numbers beside it still mean the start of that row.
    app.handle(obelus_app::event::Event::Pointer {
        kind: obelus_app::event::Pointer::Pressed,
        x: area.x,
        y: area.y + 2,
    });
    assert!(
        !app.current_buffer().expect("a file").folds().any_folded(),
        "a click on the numbers folded something"
    );
    assert_eq!(
        app.current_buffer().expect("a file").cursor().line.get(),
        2,
        "a click on the numbers did not reach the line"
    );
}
