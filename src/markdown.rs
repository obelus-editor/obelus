//! Markdown, laid out for a terminal.
//!
//! The parse and the layout come from `termimad`, which is the part worth
//! borrowing: it knows how to wrap a paragraph, indent a list, lay out a
//! table and where to put a rule. What obelus does not borrow is the
//! painting -- termimad writes escape sequences to a terminal, and obelus
//! writes cells -- so what comes out of here is *structure*, and the colours
//! are the theme's.
//!
//! Kinds rather than colours, for the same reason the syntax layer caches
//! capture indices rather than styles: switching theme then needs no
//! re-render.

use termimad::{FmtLine, FmtText, MadSkin, minimad::Compound};

use crate::reading::{Ink, Row, Span};

/// Lays markdown out for a width.
///
/// Every row is at most `width` cells, so the caller never has to wrap: the
/// wrapping is where all the difficulty lives, and it is the reason this
/// borrows a markdown renderer instead of walking the syntax tree.
#[must_use]
pub fn render(source: &str, width: u16) -> Vec<Row> {
    // Joined first. The renderer below is line-oriented -- one source line,
    // one row -- and markdown is not: a single newline inside a paragraph is
    // a *soft* break, and the text either side of it is one paragraph that
    // should be laid out to the window. Without this, a README hard-wrapped
    // at eighty columns reads as ragged short lines in a wide window and
    // never reflows in a narrow one.
    let joined = reflow(source);

    // The default skin, for its *measurements*: bullet characters, table
    // borders, the widths it lays out to. Its colours are never read.
    let skin = MadSkin::default();
    let text = FmtText::from(&skin, &joined, Some(usize::from(width.max(1))));

    text.lines
        .iter()
        .map(|line| match line {
            FmtLine::Normal(composite) => Row {
                spans: spans_of(composite),
                rule: false,
            },
            FmtLine::HorizontalRule => Row {
                spans: Vec::new(),
                rule: true,
            },
            // A table, which termimad has laid out into columns of known
            // widths. The cells have to be padded to those widths, or the
            // vertical borders stop lining up with the horizontal ones --
            // and the rules have to be built from the same widths, with
            // corners and crossings rather than a plain line.
            FmtLine::TableRow(row) => Row {
                spans: table_row(row),
                rule: false,
            },
            FmtLine::TableRule(rule) => Row {
                spans: vec![Span {
                    text: table_rule(rule),
                    ink: Ink::Mark,
                    bold: false,
                    italic: false,
                }],
                rule: false,
            },
        })
        .collect()
}

/// The runs of one composite, with whatever the composite's own kind adds.
fn spans_of(composite: &termimad::FmtComposite<'_>) -> Vec<Span> {
    use termimad::CompositeKind;

    let ink = match composite.kind {
        CompositeKind::Header(level) => Ink::Heading(level),
        CompositeKind::Code => Ink::Code,
        CompositeKind::Quote => Ink::Aside,
        CompositeKind::Paragraph
        | CompositeKind::ListItem(_)
        | CompositeKind::ListItemFollowUp(_)
        | CompositeKind::OrderedListItem { .. }
        | CompositeKind::OrderedListItemFollowUp { .. } => Ink::Plain,
    };

    // The bullet or the number, which termimad leaves to the skin to draw:
    // it is in the layout as spacing, not as text, so it is added here.
    let bullet = match composite.kind {
        CompositeKind::ListItem(level) => Some(format!(
            "{}\u{2022} ",
            "  ".repeat(usize::from(level.saturating_sub(1)))
        )),
        CompositeKind::OrderedListItem { level, index } => Some(format!(
            "{}{index}. ",
            "  ".repeat(usize::from(level.saturating_sub(1)))
        )),
        CompositeKind::ListItemFollowUp(level)
        | CompositeKind::OrderedListItemFollowUp { level, .. } => {
            Some("  ".repeat(usize::from(level)))
        }
        // A quote has a mark too, and without it a quoted line is only a
        // different colour -- which says nothing on a screen where every
        // line is some colour.
        CompositeKind::Quote => Some("\u{2503} ".to_string()),
        _ => None,
    };

    let mut spans: Vec<Span> = Vec::new();
    if let Some(bullet) = bullet {
        spans.push(Span {
            text: bullet,
            ink: Ink::Mark,
            bold: false,
            italic: false,
        });
    }
    spans.extend(
        composite
            .compounds
            .iter()
            .map(|compound| span_of(compound, ink)),
    );
    spans
}

/// One compound, which is a run of text with the same emphasis throughout.
fn span_of(compound: &Compound<'_>, ink: Ink) -> Span {
    Span {
        text: compound.src.to_string(),
        // A code span inside a paragraph: the compound knows, and it is more
        // specific than the line it is on.
        ink: if compound.code { Ink::Code } else { ink },
        bold: compound.bold,
        italic: compound.italic,
    }
}

/// One row of a table, padded to its columns and bordered.
fn table_row(row: &termimad::FmtTableRow<'_>) -> Vec<Span> {
    let border = |text: &str| Span {
        text: text.to_string(),
        ink: Ink::Mark,
        bold: false,
        italic: false,
    };

    let mut spans = vec![border("\u{2502}")];
    for cell in &row.cells {
        let width = cell
            .spacing
            .map_or(cell.visible_length, |spacing| spacing.width);
        spans.extend(spans_of(cell));
        // The padding termimad would have written itself. Its own renderer
        // pads while painting; obelus needs the cell to *be* the width, so
        // the border after it lands where the rule says it should.
        let padding = width.saturating_sub(cell.visible_length);
        if padding > 0 {
            spans.push(border(&" ".repeat(padding)));
        }
        spans.push(border("\u{2502}"));
    }
    spans
}

/// A table's horizontal rule, with the right corners for where it sits.
fn table_rule(rule: &termimad::FmtTableRule) -> String {
    use termimad::RelativePosition;

    let (left, junction, right) = match rule.position {
        RelativePosition::Top => ('\u{250c}', '\u{252c}', '\u{2510}'),
        RelativePosition::Other => ('\u{251c}', '\u{253c}', '\u{2524}'),
        RelativePosition::Bottom => ('\u{2514}', '\u{2534}', '\u{2518}'),
    };

    let mut line = String::new();
    line.push(left);
    for (index, width) in rule.widths.iter().enumerate() {
        if index > 0 {
            line.push(junction);
        }
        for _ in 0..*width {
            line.push('\u{2500}');
        }
    }
    line.push(right);
    line
}

/// Joins the lines of each paragraph into one line.
///
/// Everything markdown *needs* kept apart stays apart, and the reason each
/// one does is that a blank line, a block's first line, or a hard break says
/// so. What is left -- a line of prose following a line of prose -- is one
/// paragraph that was wrapped by whoever wrote the file, at a width that has
/// nothing to do with the window it is being read in.
///
/// Small on purpose. This is not a markdown parser: it decides only whether
/// the newline at the end of a line survives, and gets the answer from the
/// *next* line's first characters.
#[must_use]
pub fn reflow(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut fenced = false;
    // Whether the previous line was prose that a following line could
    // continue.
    let mut open = false;
    // And whether that prose was quoted, because a quote is continued by
    // another `>` line rather than by a bare one.
    let mut quoted = false;

    for line in source.lines() {
        let trimmed = line.trim_start();

        // Inside a fence nothing is joined: the lines are the content, and
        // their breaks are the whole point of them.
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fenced = !fenced;
            finish(&mut out, &mut open);
            out.push_str(line);
            out.push('\n');
            continue;
        }
        if fenced {
            out.push_str(line);
            out.push('\n');
            continue;
        }

        // A blank line ends a paragraph, and is itself the separator.
        if trimmed.is_empty() {
            finish(&mut out, &mut open);
            out.push('\n');
            continue;
        }

        // A quote is continued by the next `>` line, with its marker
        // dropped: `> one` and `> two` are one quoted paragraph, and keeping
        // them apart would wrap each of them on its own.
        if open && quoted && trimmed.starts_with('>') {
            let rest = trimmed.trim_start_matches('>').trim_start();
            if !rest.is_empty() {
                out.push(' ');
                out.push_str(rest);
                continue;
            }
        }

        if open && !quoted && !starts_a_block(line) {
            // A continuation. One space, because that is what a soft break
            // means, and the leading whitespace of a wrapped line is the
            // author's wrapping rather than their text.
            out.push(' ');
            out.push_str(trimmed);
            continue;
        }

        finish(&mut out, &mut open);
        // A line ending in two spaces or a backslash is a *hard* break: the
        // author asked for the newline, so nothing may continue it -- and
        // the marker is the request, not the text, so it goes.
        if hard_break(line) {
            out.push_str(line.trim_end_matches([' ', '\\']));
            out.push('\n');
            quoted = false;
            continue;
        }
        out.push_str(line);
        // Whether a following line could continue *this* one. A heading is
        // one line by definition, and so are a rule and a table row: prose
        // written under a heading is prose, not part of the heading.
        open = continues(trimmed);
        quoted = trimmed.starts_with('>');
        if !open {
            out.push('\n');
        }
    }
    finish(&mut out, &mut open);
    out
}

/// Ends whatever line is being built.
fn finish(out: &mut String, open: &mut bool) {
    if *open {
        out.push('\n');
        *open = false;
    }
}

/// Whether a following line could be a continuation of this one.
///
/// Prose, a list item and a quote can be wrapped across source lines. A
/// heading, a rule, a table row and a line of indented code cannot: each is
/// one line by definition, and joining the next line onto it would make the
/// next line part of something it is not.
fn continues(trimmed: &str) -> bool {
    if trimmed.starts_with('#') || trimmed.starts_with('|') || is_rule(trimmed) {
        return false;
    }
    true
}

/// Whether a line begins a block of its own rather than continuing prose.
///
/// By its first characters, which is how markdown itself decides. Four
/// spaces is indented code; a bullet, a number, a heading, a quote, a rule,
/// a table row and a tag each begin something.
fn starts_a_block(line: &str) -> bool {
    if line.starts_with("    ") || line.starts_with('\t') {
        return true;
    }
    let trimmed = line.trim_start();
    let bullet = trimmed
        .strip_prefix(['-', '*', '+'])
        .is_some_and(|rest| rest.starts_with(' ') || rest.is_empty());
    let numbered = {
        let digits: String = trimmed.chars().take_while(char::is_ascii_digit).collect();
        !digits.is_empty()
            && trimmed[digits.len()..]
                .strip_prefix(['.', ')'])
                .is_some_and(|rest| rest.starts_with(' '))
    };
    bullet
        || numbered
        || trimmed.starts_with('#')
        || trimmed.starts_with('>')
        || trimmed.starts_with('|')
        || trimmed.starts_with('<')
        || is_rule(trimmed)
}

/// Whether a line is a horizontal rule.
fn is_rule(trimmed: &str) -> bool {
    ["---", "***", "___"]
        .iter()
        .any(|mark| trimmed.starts_with(mark))
        && trimmed
            .chars()
            .all(|character| matches!(character, '-' | '*' | '_' | ' '))
}

/// Whether a line ends with a break the author asked for.
fn hard_break(line: &str) -> bool {
    line.ends_with("  ") || line.ends_with('\\')
}
