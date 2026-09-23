//! Markdown, laid out.
//!
//! Values in, rows out: the layout is borrowed, so what is worth pinning
//! down is the translation -- that a heading is a heading, that a fenced
//! block is code, that a bullet is drawn at all, and that no row is wider
//! than the width it was laid out for.
mod support;

use obelus_markdown::render;
use obelus_row::{Ink, Row};

/// Every span of a row, joined.
fn text(row: &Row) -> String {
    row.spans.iter().map(|span| span.text.as_str()).collect()
}

fn inks(rows: &[Row]) -> Vec<Ink> {
    rows.iter()
        .flat_map(|row| row.spans.iter().map(|span| span.ink))
        .collect()
}

#[test]
fn a_heading_a_paragraph_and_a_fence_are_told_apart() {
    let rows = render(
        "# Title\n\nSome *prose* here.\n\n```rust\nfn main() {}\n```\n",
        40,
    );
    let all = inks(&rows);

    assert!(all.contains(&Ink::Heading(1)), "no heading: {rows:?}");
    assert!(all.contains(&Ink::Plain), "no prose: {rows:?}");
    // A fence that names a language is coloured the way that language is,
    // rather than in the one colour that says only "this is code".
    assert!(
        all.iter().any(|ink| matches!(ink, Ink::Syntax(_))),
        "no code: {rows:?}"
    );

    // The heading keeps its words and loses its hashes: the marks are
    // markdown's, not the author's.
    let heading = rows
        .iter()
        .find(|row| row.spans.iter().any(|span| span.ink == Ink::Heading(1)))
        .expect("a heading row");
    assert!(text(heading).contains("Title"));
    assert!(!text(heading).contains('#'), "{heading:?}");

    // And the emphasis survives as emphasis rather than as asterisks.
    let emphasised = rows
        .iter()
        .flat_map(|row| &row.spans)
        .find(|span| span.text.contains("prose"))
        .expect("the emphasised word");
    assert!(emphasised.bold || emphasised.italic, "{emphasised:?}");
    assert!(
        !rows.iter().any(|row| text(row).contains('*')),
        "the asterisks are still there: {rows:?}"
    );
}

#[test]
fn a_list_gets_its_bullets_drawn() {
    let rows = render("- one\n- two\n", 40);
    let drawn: Vec<String> = rows.iter().map(text).collect();

    assert!(
        drawn
            .iter()
            .any(|row| row.contains('\u{2022}') && row.contains("one")),
        "no bullet: {drawn:?}"
    );
    // The bullet is the renderer's mark, not the author's word, and is
    // coloured as one.
    let bullet = rows
        .iter()
        .flat_map(|row| &row.spans)
        .find(|span| span.text.contains('\u{2022}'))
        .expect("a bullet");
    assert_eq!(bullet.ink, Ink::Mark);
}

/// The whole reason for borrowing a markdown renderer instead of walking the
/// syntax tree: wrapping. A row wider than the screen would have to be
/// wrapped again by whoever drew it, and it is the same problem twice.
#[test]
fn nothing_comes_back_wider_than_the_width() {
    let long = "A paragraph long enough that it has to be broken across \
                several rows before it can be drawn into a narrow window, \
                which is the point of asking for a width at all.";
    for width in [20u16, 40, 80] {
        for row in render(long, width) {
            let drawn = text(&row);
            let cells: usize = drawn.chars().count();
            assert!(
                cells <= usize::from(width),
                "a row of {cells} cells came back for a width of {width}: {drawn:?}"
            );
        }
    }
}

#[test]
fn a_rule_is_a_row_with_no_words() {
    let rows = render("above\n\n---\n\nbelow\n", 40);
    assert!(
        rows.iter().any(|row| row.rule && row.spans.is_empty()),
        "no rule: {rows:?}"
    );
}

/// A table's rows all come back the same width, and its rule's junctions
/// land on its borders. Cells have to be padded to the widths termimad laid
/// them out to: without that the vertical borders drift left as the text
/// gets shorter, and the rule underneath lines up with nothing.
#[test]
fn a_tables_borders_line_up() {
    let rows = render(
        "| Key | What |\n|-----|------|\n| a | first |\n| bb | x |\n",
        40,
    );
    let drawn: Vec<String> = rows
        .iter()
        .map(text)
        .filter(|row| row.contains('\u{2502}') || row.contains('\u{253c}'))
        .collect();
    assert!(drawn.len() >= 3, "not a table: {drawn:?}");

    let widths: Vec<usize> = drawn.iter().map(|row| row.chars().count()).collect();
    assert!(
        widths.windows(2).all(|pair| pair[0] == pair[1]),
        "the rows are not the same width: {widths:?} in {drawn:?}"
    );

    // And the rule's crossings sit under the rows' bars.
    let rule = drawn
        .iter()
        .find(|row| row.contains('\u{253c}'))
        .expect("a rule");
    let row = drawn
        .iter()
        .find(|row| row.contains('\u{2502}'))
        .expect("a row");
    let bars: Vec<usize> = row
        .chars()
        .enumerate()
        .filter(|(_, character)| *character == '\u{2502}')
        .map(|(at, _)| at)
        .collect();
    let crossings: Vec<usize> = rule
        .chars()
        .enumerate()
        .filter(|(_, character)| matches!(character, '\u{251c}' | '\u{253c}' | '\u{2524}'))
        .map(|(at, _)| at)
        .collect();
    assert_eq!(bars, crossings, "{row:?} against {rule:?}");
}

/// A single newline inside a paragraph is a *soft* break: the text either
/// side of it is one paragraph, to be laid out to the window. The renderer
/// underneath is line-oriented -- one source line, one row -- so without
/// joining them first a README hard-wrapped at eighty columns reads as
/// ragged short lines in a wide window and never reflows in a narrow one.
#[test]
fn a_paragraph_is_joined_before_it_is_wrapped() {
    let source = "A paragraph that was hard-wrapped\nby its author across\nthree source lines.\n";

    // Wide: the three source lines become one row.
    let wide = render(source, 80);
    let rows: Vec<String> = wide
        .iter()
        .map(text)
        .filter(|row| !row.is_empty())
        .collect();
    assert_eq!(rows.len(), 1, "the paragraph did not reflow: {rows:?}");
    assert!(rows[0].contains("hard-wrapped by its author across three"));

    // Narrow: laid out to the window rather than to whatever width the
    // author happened to wrap at.
    let narrow = render(source, 30);
    let rows: Vec<String> = narrow
        .iter()
        .map(text)
        .filter(|row| !row.is_empty())
        .collect();
    assert!(rows.len() > 1, "{rows:?}");
    assert!(rows.iter().all(|row| row.chars().count() <= 30), "{rows:?}");
}

/// Everything markdown needs kept apart stays apart. Each of these is a
/// break the author asked for, and joining any of them makes the next line
/// part of something it is not.
#[test]
fn the_breaks_that_mean_something_survive() {
    // A blank line is a paragraph break.
    let rows: Vec<String> = render("one\n\ntwo\n", 80).iter().map(text).collect();
    assert!(
        rows.iter().any(|row| row.trim() == "one"),
        "the paragraphs were joined: {rows:?}"
    );

    // A heading is one line: prose under it is prose.
    let rows: Vec<String> = render("# Heading\nprose under it\n", 80)
        .iter()
        .map(text)
        .collect();
    assert!(
        rows.iter().any(|row| row.trim() == "Heading"),
        "the heading swallowed the prose: {rows:?}"
    );

    // A fence keeps every break in it: the lines *are* the content.
    let rows: Vec<String> = render("```\nfn main() {}\nlet x = 1;\n```\n", 80)
        .iter()
        .map(text)
        .collect();
    assert!(
        rows.iter()
            .any(|row| row.trim_matches([' ', '\u{2502}']) == "fn main() {}"),
        "the code was reflowed: {rows:?}"
    );

    // Two list items are two items; a continuation of one is part of it.
    let rows: Vec<String> = render("- one\n  continued\n- two\n", 80)
        .iter()
        .map(text)
        .filter(|row| !row.is_empty())
        .collect();
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert!(rows[0].contains("one continued"), "{rows:?}");

    // A table's rows are rows.
    let rows: Vec<String> = render("| a | b |\n|---|---|\n| 1 | 2 |\n", 80)
        .iter()
        .map(text)
        .filter(|row| row.contains('\u{2502}'))
        .collect();
    assert_eq!(rows.len(), 2, "the table was joined into one row: {rows:?}");

    // Two spaces at the end of a line is a hard break: the author asked for
    // it, and the marker is the request rather than the text.
    let rows: Vec<String> = render("before  \nafter\n", 80)
        .iter()
        .map(text)
        .filter(|row| !row.is_empty())
        .collect();
    assert_eq!(rows.len(), 2, "the hard break was joined away: {rows:?}");
    assert_eq!(rows[0].trim_end(), "before");
    assert!(
        !rows[0].ends_with("  "),
        "the marker is still text: {rows:?}"
    );
}

/// A quote is continued by the next `>` line, with its marker dropped: two
/// quoted lines are one quoted paragraph, and keeping them apart would wrap
/// each of them on its own.
#[test]
fn a_quote_reflows_as_one_paragraph() {
    let source = "> a quotation that was\n> wrapped by its author\n\nprose after it\n";
    let rows: Vec<String> = render(source, 80)
        .iter()
        .map(text)
        .filter(|row| !row.trim().is_empty())
        .collect();

    let quoted = rows
        .iter()
        .find(|row| row.contains("quotation"))
        .expect("the quote");
    assert!(
        quoted.contains("wrapped by its author"),
        "the quote did not reflow: {rows:?}"
    );
    // And it is still marked as a quote rather than becoming prose.
    assert!(quoted.contains('\u{2503}'), "{rows:?}");
    assert!(
        rows.iter().any(|row| row.trim() == "prose after it"),
        "the prose after it was swallowed: {rows:?}"
    );
}

#[test]
fn obelus_can_read_its_own_log() {
    use obelus_reading::log::{Format, format_of};

    // Exactly what the subscriber writes: the process first, because one
    // file holds every session and several of them run at once.
    let pid = std::process::id();
    let written = format!(
        "{pid} 2026-09-14T00:36:43.625057Z  INFO ob: obelus starting version=\"0.1.0\"\n\
         {pid} 2026-09-14T00:36:43.625558Z  INFO obelus_ui::image: pictures protocol=Sixel\n\
         {pid} 2026-09-14T00:36:43.626000Z  WARN obelus_app::app: no keyboard protocol\n"
    );
    assert_eq!(
        format_of(&written),
        Some(Format::Ours),
        "obelus does not recognise the log it writes itself"
    );

    // And which obelus said it survives into the entry, beside the rest of
    // what the line carries by name.
    let entry = Format::Ours
        .read(written.lines().next().expect("a line"))
        .expect("an entry");
    assert!(
        entry
            .fields
            .iter()
            .any(|(name, value)| name == "pid" && value == &pid.to_string()),
        "the process that said it was dropped: {:?}",
        entry.fields
    );
}

/// A fenced block is not prose and is not laid out as prose: what it is made
/// of is what its fence says it is.
mod fences {
    use obelus_row::{Ink, Row};
    use obelus_text::kind::SyntaxKind;

    use super::{render, text};

    /// Where the box begins on a row, if the row has one.
    ///
    /// Not always the first run: a block of code inside a list or a quote
    /// carries that list's indent and that quote's bar down its left, and
    /// the box is drawn in what is left.
    fn box_at(row: &Row, side: &[char]) -> Option<usize> {
        row.spans
            .iter()
            .position(|span| span.text.starts_with(side))
    }

    /// Whether a row is a line of code, which is a row inside the box.
    fn is_code(row: &Row) -> bool {
        box_at(row, &['\u{2502}']).is_some_and(|at| row.spans.len() > at + 2)
    }

    /// Whether a row is the top or the bottom of the box.
    fn is_across(row: &Row) -> bool {
        box_at(row, &['\u{250c}', '\u{2514}']).is_some_and(|at| row.spans.len() == at + 1)
    }

    /// The runs of a row of code, without the box and the room round it --
    /// and without whatever the blocks it is inside draw down its left.
    fn inside(row: &Row) -> &[obelus_row::Span] {
        let at = box_at(row, &['\u{2502}']).unwrap_or(0);
        &row.spans[at + 1..row.spans.len() - 2]
    }

    /// Every ink on a row of code, in the order the runs are in, with the
    /// box dropped. The box itself is what [`a_block_is_boxed`] is about.
    fn inks_of(source: &str, width: u16) -> Vec<Vec<Ink>> {
        render(source, width)
            .iter()
            .filter(|row| is_code(row))
            .map(|row| inside(row).iter().map(|span| span.ink).collect())
            .collect()
    }

    /// The same, as text, and without the box or the room it is set in.
    fn lines_of(source: &str, width: u16) -> Vec<String> {
        render(source, width)
            .iter()
            .filter(|row| !row.spans.is_empty() && !is_across(row))
            .map(|row| match is_code(row) {
                true => inside(row).iter().map(|span| span.text.as_str()).collect(),
                false => text(row),
            })
            .collect()
    }

    /// A block of code is a thing set into the page: a box round it, the
    /// whole width of the reading, and the prose left outside.
    #[test]
    fn a_block_is_boxed() {
        let width = 40;
        let rows = render("before\n\n```rust\nfn main() {}\n```\n", width);
        let lines: Vec<String> = rows.iter().map(text).collect();
        let at = lines
            .iter()
            .position(|line| line.contains("fn main"))
            .unwrap_or_else(|| panic!("the code is not in the rows: {lines:?}"));

        assert!(
            lines[at].starts_with('\u{2502}') && lines[at].ends_with('\u{2502}'),
            "the code is not inside a box: {lines:?}"
        );
        assert!(
            lines[at - 1].starts_with('\u{250c}') && lines[at - 1].ends_with('\u{2510}'),
            "the box has no top: {lines:?}"
        );
        assert!(
            lines[at + 1].starts_with('\u{2514}') && lines[at + 1].ends_with('\u{2518}'),
            "the box has no bottom: {lines:?}"
        );

        // The box is the reading's own width, so its sides line up with
        // whatever is drawn above and below it.
        for line in &lines[at - 1..=at + 1] {
            assert_eq!(
                line.chars().count(),
                usize::from(width),
                "the box is not the width of the reading: {lines:?}"
            );
        }
        assert!(
            lines.iter().any(|line| line.starts_with("before")),
            "the prose was swept into the box too: {lines:?}"
        );
    }

    #[test]
    fn a_named_language_is_coloured_as_that_language() {
        let inks = inks_of("```rust\nfn main() {}\n```\n", 40);
        assert_eq!(
            inks,
            vec![vec![
                Ink::Syntax(SyntaxKind::Keyword),
                Ink::Plain,
                Ink::Syntax(SyntaxKind::Function),
                Ink::Syntax(SyntaxKind::Punctuation),
                Ink::Plain,
                Ink::Syntax(SyntaxKind::Punctuation),
            ]],
            "`fn`, the name and the brackets are not told apart"
        );
    }

    /// The fence's word is the language's name, where a file's is its
    /// extension: the two arrive from different places and mean the same.
    #[test]
    fn the_fence_is_read_by_name_as_well_as_by_extension() {
        for fence in ["rust", "rs"] {
            let inks = inks_of(&format!("```{fence}\nfn main() {{}}\n```\n"), 40);
            assert!(
                inks[0].contains(&Ink::Syntax(SyntaxKind::Keyword)),
                "```{fence} was not read as rust: {inks:?}"
            );
        }
    }

    /// A fence that names nothing keeps the one colour that says only that
    /// it is code -- guessing at a language would colour it wrongly, which
    /// is worse than not colouring it.
    #[test]
    fn a_fence_with_no_language_stays_one_colour() {
        assert_eq!(
            inks_of("```\nfn main() {}\n```\n", 40),
            vec![vec![Ink::Code]]
        );
        assert_eq!(
            inks_of("```nothing-obelus-knows\nfn main() {}\n```\n", 40),
            vec![vec![Ink::Code]]
        );
    }

    /// The fences themselves are markdown's marks, not the author's words,
    /// and the language on the opening one is not a line of the block.
    #[test]
    fn the_fences_and_the_language_are_not_in_the_rows() {
        let lines = lines_of("```rust\nfn main() {}\n```\n", 40);
        assert_eq!(lines, vec!["fn main() {}"]);
    }

    /// Tildes open a fence as backticks do. These used to leak their
    /// language into the rows and lose the code's colour with it.
    #[test]
    fn a_tilde_fence_is_a_fence() {
        let lines = lines_of("~~~rust\nfn main() {}\n~~~\n", 40);
        assert_eq!(lines, vec!["fn main() {}"]);
    }

    /// And a fence inside a list item, which used to leak the same way --
    /// while the list went on being numbered around it.
    #[test]
    fn a_fence_inside_a_list_keeps_the_list() {
        let lines = lines_of(
            "1. first\n\n   ```rust\n   let x = 1;\n   ```\n\n2. second\n",
            40,
        );
        assert!(
            lines.iter().any(|line| line.contains("1. first")),
            "{lines:?}"
        );
        assert!(
            lines.iter().any(|line| line.trim() == "let x = 1;"),
            "{lines:?}"
        );
        assert!(
            lines.iter().any(|line| line.contains("2. second")),
            "{lines:?}"
        );
        assert!(
            !lines.iter().any(|line| line.contains("rust")),
            "the fence's language leaked into the rows: {lines:?}"
        );

        // And the box is drawn *inside* the item, under the words of the
        // step it belongs to. A block of code in a list is part of the step
        // it is written under, and one drawn the whole width of the reading
        // has walked out of the list it was in.
        let rows = render(
            "1. first\n\n   ```rust\n   let x = 1;\n   ```\n\n2. second\n",
            40,
        );
        let boxed = rows
            .iter()
            .find(|row| {
                row.spans
                    .iter()
                    .any(|span| span.text.starts_with('\u{250c}'))
            })
            .expect("the top of the box");
        assert!(
            boxed.spans.len() > 1 && boxed.spans[0].text.chars().all(char::is_whitespace),
            "the box is not indented under the step it belongs to: {:?}",
            text(boxed)
        );
    }

    /// A fence is closed by its own kind, and by at least as many of them:
    /// a block about markdown has fences of its own inside it, and a
    /// shorter run of the other character is one of its lines.
    #[test]
    fn a_fence_is_closed_by_its_own_kind() {
        let lines = lines_of("~~~\n```\nstill inside\n```\n~~~\n", 40);
        assert_eq!(
            lines,
            vec!["```", "still inside", "```"],
            "a fence of the other kind closed it"
        );

        // And four backticks are not closed by three.
        let lines = lines_of("````\n```\ninside\n```\n````\n", 40);
        assert_eq!(lines, vec!["```", "inside", "```"]);
    }

    /// A line of code has no words to respect, so it breaks at the width
    /// rather than at a space -- and every character survives the break.
    #[test]
    fn a_long_line_breaks_at_the_width() {
        // Twelve cells across, less the side of the box at each end, is
        // ten for the code.
        let lines = lines_of("```rust\nlet name = other(1, 2, 3);\n```\n", 12);
        assert_eq!(lines, vec!["let name =", " other(1, ", "2, 3);"]);
    }

    /// An unclosed fence is a file somebody is still writing, not a file to
    /// refuse: what is under it is the block.
    #[test]
    fn an_unclosed_fence_runs_to_the_end() {
        let lines = lines_of("```rust\nlet x = 1;\n", 40);
        assert_eq!(lines, vec!["let x = 1;"]);
    }
}

/// The box round a block of code, on screen.
///
/// The rows carry the box; this is what the drawing does with it, and the
/// only way to see that is the cells.
#[test]
fn a_block_of_code_is_drawn_in_a_box() {
    let scratch = support::Scratch::new("reading-code-box");
    let path = scratch.path().join("sample.md");
    // A table as well as the block, because a table's borders are the
    // same furniture: what the box is drawn in is what they are drawn in.
    std::fs::write(
        &path,
        "before\n\n```rust\nfn main() {}\n```\n\n| alpha | beta |\n|---|---|\n| 1 | 2 |\n",
    )
    .expect("writing it");
    let mut app = obelus_app::app::App::new(vec![
        obelus_buffer::Buffer::open(&path).expect("opening it"),
    ]);
    app.working_directory_for_test(scratch.path().to_path_buf());
    support::lay_out(&mut app, 40, 20);
    // A reading is what preview shows: the editor draws the file's own
    // bytes, fences and all.
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::PreviewToggle);

    let dump = support::render(&mut app, 40, 20);
    // Past the row number the dump puts in front of every row, and past
    // the blank line the section itself starts with.
    let cells = |block: &str| -> Vec<String> {
        block
            .lines()
            .filter_map(|row| row.split_once('|').map(|(_, cells)| cells.to_string()))
            .collect()
    };
    let rows = cells(support::text_block(&dump));
    let styles = cells(support::style_block(&dump));
    let at = rows
        .iter()
        .position(|row| row.contains("fn main"))
        .unwrap_or_else(|| panic!("the code is not on screen:\n{dump}"));

    // The box, drawn round the code and held off both edges.
    let code = &rows[at];
    assert!(
        code.starts_with('\u{2502}') && code.trim_end().ends_with('\u{2502}'),
        "the code is not drawn inside a box:\n{dump}"
    );
    assert!(
        rows[at - 1].trim_end().ends_with('\u{2510}')
            && rows[at + 1].trim_end().ends_with('\u{2518}'),
        "the box has no top or no bottom on screen:\n{dump}"
    );
    assert_eq!(
        rows[at].chars().count() - rows[at].trim_end().chars().count(),
        1,
        "the box does not reach the reading's edge, the scrollbar's cell apart:\n{dump}"
    );

    // And it is drawn as the reading's own furniture rather than as code:
    // the same as the borders of a table, which is what it is.
    let table = rows
        .iter()
        .position(|row| row.contains("alpha"))
        .unwrap_or_else(|| panic!("the table is not on screen:\n{dump}"));
    let side = |row: &String| {
        row.chars()
            .position(|cell| cell == '\u{2502}')
            .unwrap_or_else(|| panic!("no border on {row:?}:\n{dump}"))
    };
    let letter = |row: usize, column: usize| styles[row].chars().nth(column).expect("a style");
    assert_eq!(
        letter(at, side(code)),
        letter(table, side(&rows[table])),
        "the box is not drawn as the reading's own furniture:\n{dump}"
    );
}

/// Every run that says where it came from came from there.
///
/// One property, and it covers the lot: a heading with its hashes taken
/// off, emphasis with its stars taken off, a code span with its backticks
/// taken off, a list item, a table cell, a line of a fenced block. If the
/// bytes a run points at are not the bytes it is drawing, the pointing is
/// wrong somewhere, and it is wrong in a way nothing on screen would show
/// -- a selection that copies the wrong words.
///
/// The runs a reading adds rather than reads -- the bullet, the quotation
/// bar, the box round a fenced block, a table's borders and padding --
/// point at nothing, which is the other half of the same property.
///
/// Broken deliberately by handing `prose` the wrong base -- every run of
/// every piece after the first then points somewhere else.
///
/// It does *not* catch a run mapped by its start alone rather than broken
/// where the source is. That was tried: the layout never seems to hand
/// back a run spanning a join, so the two ways agree on everything here.
/// The breaking is kept because it is what makes the claim true whatever
/// the layout does, and it is written down here that no test holds it up.
#[test]
fn a_run_that_says_where_it_came_from_came_from_there() {
    let source = "## What I would do\n\n\
        The **cheap** part is moving `closer_for` out of the way, and\n\
        a second line of the same paragraph that has to be joined to it.\n\n\
        - it is a pure function\n\
        - it has nothing to do with `App`\n\n\
        > a quotation, which is marked\n\n\
        ```rust\nfn closer_for(open: char) -> char {\n```\n\n\
        | a | b |\n|---|---|\n| 1 | 2 |\n";
    for width in [30, 48, 76] {
        let rows = render(source, width);
        let mut checked = 0;
        for row in &rows {
            for span in &row.spans {
                let Some(from) = span.from.clone() else {
                    continue;
                };
                checked += 1;
                assert!(
                    from.end <= source.len(),
                    "a run points past the end of the source: {span:?}"
                );
                assert_eq!(
                    &source[from.clone()],
                    span.text,
                    "at width {width}, a run says it came from {from:?} and it did not"
                );
            }
        }
        assert!(
            checked > 10,
            "at width {width}, hardly anything says where it came from: {checked}"
        );
    }
}

/// The marks a reading adds itself point at nothing.
///
/// Broken deliberately by giving the bullet a range of its own: the bytes
/// it would claim say something else entirely, and a selection over it
/// would copy them.
#[test]
fn what_the_reading_adds_points_at_nothing() {
    let source = "- a bullet\n\n> a quotation\n\n```\ncode\n```\n";
    let rows = render(source, 40);
    for row in &rows {
        for span in &row.spans {
            let adds = span.text.contains('\u{2022}')
                || span.text.contains('\u{2503}')
                || span
                    .text
                    .chars()
                    .all(|mark| "\u{2502}\u{2500}\u{250c}\u{2510}\u{2514}\u{2518} ".contains(mark));
            if adds && !span.text.is_empty() {
                assert!(
                    span.from.is_none(),
                    "a mark the reading drew claims to come from the source: {span:?}"
                );
            }
        }
    }
}
