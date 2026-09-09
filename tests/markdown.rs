//! Markdown, laid out.
//!
//! Values in, rows out: the layout is borrowed, so what is worth pinning
//! down is the translation -- that a heading is a heading, that a fenced
//! block is code, that a bullet is drawn at all, and that no row is wider
//! than the width it was laid out for.

use obelus::markdown::{Kind, render};

/// Every span of a row, joined.
fn text(row: &obelus::markdown::Row) -> String {
    row.spans.iter().map(|span| span.text.as_str()).collect()
}

fn kinds(rows: &[obelus::markdown::Row]) -> Vec<Kind> {
    rows.iter()
        .flat_map(|row| row.spans.iter().map(|span| span.kind))
        .collect()
}

#[test]
fn a_heading_a_paragraph_and_a_fence_are_told_apart() {
    let rows = render(
        "# Title\n\nSome *prose* here.\n\n```rust\nfn main() {}\n```\n",
        40,
    );
    let all = kinds(&rows);

    assert!(all.contains(&Kind::Heading(1)), "no heading: {rows:?}");
    assert!(all.contains(&Kind::Code), "no code: {rows:?}");
    assert!(all.contains(&Kind::Text), "no prose: {rows:?}");

    // The heading keeps its words and loses its hashes: the marks are
    // markdown's, not the author's.
    let heading = rows
        .iter()
        .find(|row| row.spans.iter().any(|span| span.kind == Kind::Heading(1)))
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
    assert_eq!(bullet.kind, Kind::Decoration);
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
        rows.iter().any(|row| row.trim() == "fn main() {}"),
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
