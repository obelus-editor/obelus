//! What a command about lines would do to a buffer, worked out without
//! doing it.
//!
//! Each of these reads the buffer and says what to put where and what the
//! reader is left holding, and does nothing: the change itself goes through
//! the one door a document changes through (`Buffer::edit`, by way of the
//! application, which keeps everything measured against the text in step).
//! Which is also what lets them be asked about without an application at
//! all.

use obelus_text::coordinates::{CharColumn, LineNumber, Span};

use crate::Buffer;

/// What a command would put in place of what, and what it leaves held.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    /// What it replaces.
    pub span: Span,
    /// What it puts there.
    pub with: String,
    /// What the reader is holding afterwards.
    pub after: After,
}

/// What the reader is holding once a change is made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum After {
    /// These lines, whole.
    Select(Span),
    /// The cursor, here.
    Cursor(LineNumber, CharColumn),
    /// Whatever the change left.
    Kept,
}

/// The lines a command about lines is about: the ones a selection touches,
/// or the one the cursor is on.
fn lines_in_hand(buffer: &Buffer) -> (LineNumber, LineNumber) {
    match buffer.selection() {
        Some(span) => (span.line, span.end_line),
        None => (buffer.cursor().line, buffer.cursor().line),
    }
}

/// The whole of lines `first` to `last`, as a span.
fn lines(buffer: &Buffer, first: LineNumber, last: LineNumber) -> Span {
    Span {
        line: first,
        column: CharColumn::new(0),
        end_line: last,
        end_column: buffer.text().line_length(last),
    }
}

/// The same lines once `with` is in their place: from the start of the first
/// to the end of the last line `with` ends in.
fn after_putting(first: LineNumber, last: LineNumber, with: &str) -> Span {
    let end = with.rsplit('\n').next().unwrap_or_default();
    Span {
        line: first,
        column: CharColumn::new(0),
        end_line: last,
        end_column: CharColumn::new(end.chars().count()),
    }
}

/// The lines the reader is on, one step of indentation deeper or shallower,
/// and held afterwards; nothing where that changes nothing.
///
/// The lines a selection touches, whole, or the line the cursor is on when
/// nothing is selected -- and one change rather than one per line, so that
/// putting a block right takes one `ctrl+z` to put back.
///
/// The selection comes out over the same lines, whole: a reader lining a
/// block up presses this more than once, and a selection that went with the
/// first press would make the second press about something else.
#[must_use]
pub fn shift_indent(buffer: &Buffer, deeper: bool) -> Option<Change> {
    let indent = buffer.indent();
    let text = buffer.text();
    let (first, last) = lines_in_hand(buffer);
    let mut shifted = Vec::new();
    for line in first.get()..=last.get() {
        let line = LineNumber::new(line);
        let was = text.line(line).to_string();
        shifted.push(match deeper {
            // Nothing to put in front of a line with nothing on it: an
            // indent on an empty line is trailing blanks, which is what
            // every other tool in the reader's way then takes back out.
            true if was.trim().is_empty() => was,
            true => indent.clone() + &was,
            false => outdented(&was, &indent),
        });
    }
    let span = lines(buffer, first, last);
    let with = shifted.join("\n");
    if with == text.text_in(span) {
        return None;
    }
    let after = After::Select(after_putting(first, last, &with));
    Some(Change { span, with, after })
}

/// A line with one step of its indentation taken off.
///
/// A tab where the line begins with one, and otherwise up to a step's worth
/// of spaces -- fewer where there are fewer, because a line indented by two
/// spaces in a file of four should come out at the margin rather than stay
/// where it is.
fn outdented(line: &str, indent: &str) -> String {
    if let Some(rest) = line.strip_prefix('\t') {
        return rest.to_string();
    }
    let blanks = line
        .chars()
        .take_while(|character| *character == ' ')
        .count();
    line.chars()
        .skip(blanks.min(indent.chars().count()))
        .collect()
}

/// The lines the reader is on, moved up or down by one; nothing where they
/// are already against that end.
///
/// One change rather than two, so a line walked three rows down is three
/// steps back rather than six -- and the lines go as they are, without being
/// re-indented: a reader moving a line has a place in mind for it, and a line
/// that changed shape on the way is a line they have to look at again.
#[must_use]
pub fn move_lines(buffer: &Buffer, up: bool) -> Option<Change> {
    let text = buffer.text();
    let (first, last) = lines_in_hand(buffer);
    // Nowhere to go: the block is already against the end it is being
    // moved towards.
    if (up && first.get() == 0) || (!up && last >= text.last_line()) {
        return None;
    }
    // The line it swaps with, and the two spans that make it one edit:
    // everything from the first line to the last, in the order the move
    // puts them.
    let (from, to) = match up {
        true => (first.saturating_sub(1), last),
        false => (first, last.saturating_add(1)),
    };
    let rows: Vec<String> = (from.get()..=to.get())
        .map(|line| text.line(LineNumber::new(line)).to_string())
        .collect();
    let with = match up {
        true => rows[1..].join("\n") + "\n" + &rows[0],
        false => {
            let end = rows.len() - 1;
            rows[end].clone() + "\n" + &rows[..end].join("\n")
        }
    };
    // The reader keeps hold of what they moved: the lines under the
    // selection, or the cursor on the line it was on. The last of them is
    // the same line wherever it went, so it ends where it ended.
    let moved = |line: LineNumber| match up {
        true => line.saturating_sub(1),
        false => line.saturating_add(1),
    };
    let after = match buffer.selection() {
        Some(_) => After::Select(Span {
            line: moved(first),
            column: CharColumn::new(0),
            end_line: moved(last),
            end_column: text.line_length(last),
        }),
        None => After::Cursor(moved(buffer.cursor().line), buffer.cursor().column),
    };
    Some(Change {
        span: lines(buffer, from, to),
        with,
        after,
    })
}

/// The lines the reader is on with `token` put in front of them as a line
/// comment, or taken off; nothing where they are all blank.
///
/// Off where every line that has anything on it is already commented, and on
/// otherwise: a block half commented is a block somebody was in the middle of
/// commenting, and finishing it is what they meant.
///
/// The token goes at the shallowest indentation of the lines it is about, so
/// that the marks line up under each other rather than stepping in and out
/// with the code. Blank lines are left alone -- a comment on one is trailing
/// blanks.
#[must_use]
pub fn toggle_comment(buffer: &Buffer, token: &str) -> Option<Change> {
    let text = buffer.text();
    let (first, last) = lines_in_hand(buffer);
    let rows: Vec<String> = (first.get()..=last.get())
        .map(|line| text.line(LineNumber::new(line)).to_string())
        .collect();

    // Three passes' worth of questions, asked in one: whether every
    // line with anything on it is already commented, how far in the
    // shallowest of them starts, and whether every mark is followed by
    // a blank -- because where one is not, taking a blank off the rest
    // would eat a character somebody wrote.
    let mut commented = true;
    let mut margin = true;
    let mut indent = usize::MAX;
    for row in rows.iter().filter(|row| !row.trim().is_empty()) {
        let at = row
            .chars()
            .take_while(|character| character.is_whitespace())
            .count();
        indent = indent.min(at);
        let rest: String = row.chars().skip(at).collect();
        match rest.strip_prefix(token) {
            Some(after) => margin &= after.starts_with(' '),
            None => commented = false,
        }
    }
    if indent == usize::MAX {
        return None;
    }

    let with: Vec<String> = rows
        .iter()
        .map(|row| {
            if row.trim().is_empty() {
                return row.clone();
            }
            if !commented {
                let (before, after): (String, String) = (
                    row.chars().take(indent).collect(),
                    row.chars().skip(indent).collect(),
                );
                return format!("{before}{token} {after}");
            }
            let at = row
                .chars()
                .take_while(|character| character.is_whitespace())
                .count();
            let kept: String = row
                .chars()
                .skip(at + token.chars().count() + usize::from(margin))
                .collect();
            row.chars().take(at).collect::<String>() + &kept
        })
        .collect();
    let with = with.join("\n");
    let after = match buffer.selection() {
        Some(_) => After::Select(after_putting(first, last, &with)),
        None => After::Kept,
    };
    Some(Change {
        span: lines(buffer, first, last),
        with,
        after,
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn buffer(text: &str) -> Buffer {
        Buffer::from_text(Path::new("lines.rs"), text)
    }

    /// The selection comes out over the same lines once they are indented,
    /// whole, so a second press is about the same block.
    ///
    /// Broken deliberately by measuring the last line before the change: the
    /// selection stopped four characters short of its end.
    #[test]
    fn an_indented_block_is_held_whole() {
        let mut buffer = buffer("a\nbc\n");
        buffer.select(lines(&buffer, LineNumber::new(0), LineNumber::new(1)));
        let change = shift_indent(&buffer, true).expect("a change");
        assert_eq!(change.with, format!("{0}a\n{0}bc", buffer.indent()));
        let After::Select(held) = change.after else {
            panic!("nothing held")
        };
        assert_eq!(held.end_column.get(), buffer.indent().len() + 2);
    }

    /// A line moved down takes the cursor with it, in the same column, and a
    /// line already at the end goes nowhere.
    ///
    /// Broken deliberately by leaving the cursor's line where it was: the
    /// cursor stayed on the line that came up.
    #[test]
    fn a_moved_line_takes_the_cursor() {
        let mut buffer = buffer("one\ntwo\n");
        buffer.place_cursor(LineNumber::new(0), CharColumn::new(2));
        let change = move_lines(&buffer, false).expect("a change");
        assert_eq!(change.with, "two\none");
        assert_eq!(
            change.after,
            After::Cursor(LineNumber::new(1), CharColumn::new(2))
        );
        buffer.place_cursor(LineNumber::new(2), CharColumn::new(0));
        assert_eq!(move_lines(&buffer, false), None, "past the last line");
    }

    /// A block half commented is finished, at the shallowest indentation, and
    /// a block wholly commented is uncommented.
    ///
    /// Broken deliberately by putting the mark at each line's own indentation:
    /// the marks stepped in with the code.
    #[test]
    fn a_comment_goes_at_the_shallowest_indentation() {
        let mut buffer = buffer("a\n  b\n");
        buffer.select(lines(&buffer, LineNumber::new(0), LineNumber::new(1)));
        let change = toggle_comment(&buffer, "//").expect("a change");
        assert_eq!(change.with, "// a\n//   b");
        let mut buffer = self::buffer("// a\n//   b\n");
        buffer.select(lines(&buffer, LineNumber::new(0), LineNumber::new(1)));
        let change = toggle_comment(&buffer, "//").expect("a change");
        assert_eq!(change.with, "a\n  b");
    }
}
