//! Putting characters into cells, and cutting what will not fit.

use super::*;

/// Paints every cell of a region in one style, blanking whatever was there.
///
/// The glyph and the modifiers, and the colours patched over what was
/// there. A style is patched onto a cell everywhere in here -- that is what
/// lets a caller set a background and keep a foreground painted underneath
/// -- and a modifier patched the same way is one nothing can take off: an
/// underline written here by whatever was drawn before survives every
/// drawing over it. A preview of another file wore the underlines of the
/// file it was drawn over, at the columns they were at *there*, which is a
/// file complaining about a line it has never seen.
///
/// Anything drawn over anything else starts with this, so it is the one
/// place that has to take them off.
pub fn fill(cells: &mut CellBuffer, area: Rect, style: Style) {
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            if let Some(cell) = cells.cell_mut((x, y)) {
                cell.set_symbol(" ");
                cell.modifier = ratatui::style::Modifier::empty();
                cell.set_style(style);
            }
        }
    }
}

/// Writes one character, and blanks the cells it covers beyond the first.
///
/// A wide glyph owns the cells after it, and they must hold no symbol at all:
/// the terminal advances two columns for the glyph, so anything left in the
/// second cell shifts the rest of the row. That is the rule in here that
/// breaks silently, which is why there is one copy of it.
///
/// The style is patched onto the cell rather than replacing it, so a caller
/// that only wants to set a foreground can pass one and keep whatever
/// background was painted underneath.
///
/// Returns how many columns were used, never zero: a character the terminal
/// does not advance over still advances this, or a caller stepping through a
/// string would not terminate.
pub fn put(cells: &mut CellBuffer, x: u16, y: u16, character: char, style: Style) -> u16 {
    let mut one = [0u8; 4];
    put_cluster(cells, x, y, character.encode_utf8(&mut one), style)
}

/// The same, for a whole cluster.
///
/// A cluster is one cell however many characters it is made of -- a
/// terminal draws `❤` and U+FE0F after it as one picture two columns wide,
/// and a family joined by U+200D as one picture too -- so the whole of it
/// goes in this cell and the width is the cluster's. The rest of it written
/// into cells of its own is cells the terminal does not advance over, and the
/// rest of the row a column out for each. Whoever walks a string with this
/// walks [`obelus_text::clusters`].
///
/// The width is `obelus-text`'s, which is the one the arithmetic used.
pub fn put_cluster(cells: &mut CellBuffer, x: u16, y: u16, cluster: &str, style: Style) -> u16 {
    let mut characters = cluster.chars();
    let first = characters.next().unwrap_or(' ');
    let alone = characters.next().is_none();
    // `obelus-text`'s answer, which counts a picture the window draws two
    // cells wide -- except for the attachment's stand-in, whose eleven
    // columns are written by whoever puts the label there, one cell at a
    // time.
    let width = match first {
        obelus_text::ATTACHED if alone => first.width().unwrap_or(0),
        _ => obelus_text::cluster_cells(cluster),
    };
    let width = u16::try_from(width).unwrap_or(0);
    if let Some(cell) = cells.cell_mut((x, y)) {
        // A control character is a space whatever comes after it -- the
        // one cluster that starts with one is a line ending.
        match alone || first.is_control() {
            true => cell.set_char(shown(first)),
            false => cell.set_symbol(cluster),
        };
        cell.set_style(style);
    }
    for extra in 1..width {
        if let Some(cell) = cells.cell_mut((x + extra, y)) {
            cell.set_symbol("");
            cell.set_style(style);
        }
    }
    width.max(1)
}

/// What a character looks like in a cell.
///
/// Itself, unless it is a control character, which is a space. A terminal
/// cell holds something that is drawn, and a control character is an
/// instruction rather than a glyph: written into one it is not drawn wrong,
/// it takes the whole frame down, because what puts a frame on the screen
/// asks every cell how wide it is and a control character has no answer.
///
/// This is not Obelus's own text. It is whatever an agent said, whatever a
/// tool put in its output, whatever is in a file somebody opened -- and a
/// reader of code meets a tab in all three. None of them is a reason for
/// Obelus to stop.
///
/// One cell, which is what `put` already counted a control character as, so
/// nothing that walks a row in step with it has to learn a new rule. A tab
/// is therefore one space rather than a jump to the next stop: the place
/// that could do better than that is where the words become rows, and it
/// cannot be done here without the screen and every piece of arithmetic
/// about it disagreeing.
///
/// What is copied is untouched, because a copy is what was said rather than
/// what a terminal could show of it.
const fn shown(character: char) -> char {
    match character.is_control() {
        true => ' ',
        false => character,
    }
}

/// Writes a string, returning the column after it.
pub fn write(cells: &mut CellBuffer, x: u16, y: u16, contents: &str, style: Style) -> u16 {
    let mut column = x;
    for cluster in obelus_text::clusters(contents) {
        column = column.saturating_add(put_cluster(cells, column, y, cluster.text, style));
    }
    column
}

/// The same, stopping before a column it may not write in.
///
/// [`write`] has no such column: it writes until the cell buffer runs out,
/// which is the whole screen. That is right wherever the caller has already
/// worked out the room -- a row wrapped to its width cannot overrun -- and
/// wrong wherever something is put down *after* such a row, because what
/// follows a row that fills its width begins past the end of it. In the
/// transcript that is a path, a fold's mark and a call's state, and the
/// column they were running into is the scrollbar's.
///
/// `stop` is the first column that may not be written, the way a `Rect`'s
/// right is. A wide glyph that would straddle it is not drawn at all: half
/// of one is a cell the terminal advances over and Obelus did not count.
pub fn write_within(
    cells: &mut CellBuffer,
    x: u16,
    y: u16,
    contents: &str,
    style: Style,
    stop: u16,
) -> u16 {
    let mut column = x;
    for cluster in obelus_text::clusters(contents) {
        // The width `put` will report, worked out before it is asked, so
        // the decision is made before the cell is written rather than
        // after.
        let width = u16::try_from(cluster.cells).unwrap_or(0).max(1);
        if column.saturating_add(width) > stop {
            break;
        }
        column = column.saturating_add(put_cluster(cells, column, y, cluster.text, style));
    }
    column
}

/// How many leading characters to drop so the rest of `contents` fits in
/// `cells`, with one cell left for the ellipsis that marks the cut.
///
/// From the left, because the end is the part worth reading: the file name in
/// a path, the last component of a symbol. The leading directories are the
/// part already known. Measured in cells rather than characters so a path with
/// wide glyphs in it does not overrun whatever comes after it.
///
/// Zero when it already fits. Everything when there is no room even for the
/// ellipsis, so the caller can draw nothing rather than a lone `…`.
#[must_use]
pub fn drop_from_left(contents: &str, cells: usize) -> usize {
    if text_width(contents) <= cells {
        return 0;
    }
    let total = contents.chars().count();
    if cells <= 1 {
        return total;
    }

    let budget = cells - 1;
    // A cluster at a time: the rest of one whose first character did not
    // fit goes with it, because on its own it says how nothing is drawn.
    let mut first = total;
    let mut width = 0usize;
    let measured: Vec<obelus_text::Cluster<'_>> = obelus_text::clusters(contents).collect();
    for cluster in measured.iter().rev() {
        if width + cluster.cells > budget {
            break;
        }
        width += cluster.cells;
        first = cluster.first;
    }
    first
}

/// How many trailing characters to drop so the rest of `contents` fits in
/// `cells`, with one cell left for the ellipsis that marks the cut.
///
/// The mirror of [`drop_from_left`], for a sentence rather than a name. A
/// name is told from its fellows at the end -- the file, the last component
/// of a symbol -- and a sentence at the beginning: a commit subject cut to
/// its last few words has lost the half that said which commit it was.
///
/// Measured in cells for the same reason, which matters more here: a subject
/// written in Chinese is one character to two columns, and counting
/// characters would cut it at half the row.
///
/// Zero when it already fits. Everything when there is no room even for the
/// ellipsis, so the caller can draw nothing rather than a lone `…`.
#[must_use]
pub fn drop_from_right(contents: &str, cells: usize) -> usize {
    if text_width(contents) <= cells {
        return 0;
    }
    let total = contents.chars().count();
    if cells <= 1 {
        return total;
    }

    let budget = cells - 1;
    let mut kept = 0usize;
    let mut width = 0usize;
    for (_, character_width) in widths(contents) {
        if width + character_width > budget {
            break;
        }
        width += character_width;
        kept += 1;
    }
    total - kept
}

/// `contents` with its tail replaced by an ellipsis if it does not fit.
///
/// For a sentence, where the beginning is the part worth keeping. The three
/// places that wanted this had each grown their own: one measured in cells
/// and one in characters, so the same prose cut to the same width came out
/// two different lengths depending on which screen it was on -- and the one
/// counting characters cut a Chinese sentence at half the room it was given.
#[must_use]
pub fn truncate_from_right(contents: &str, cells: usize) -> String {
    let dropped = drop_from_right(contents, cells);
    if dropped == 0 {
        return contents.to_string();
    }
    let total = contents.chars().count();
    if dropped >= total {
        return String::new();
    }
    let mut result: String = contents.chars().take(total - dropped).collect();
    result.push('\u{2026}');
    result
}

/// `contents` with its head replaced by an ellipsis if it does not fit.
#[must_use]
pub fn truncate_from_left(contents: &str, cells: usize) -> String {
    let dropped = drop_from_left(contents, cells);
    if dropped == 0 {
        return contents.to_string();
    }
    let total = contents.chars().count();
    if dropped >= total {
        return String::new();
    }
    let mut result = String::from('\u{2026}');
    result.extend(contents.chars().skip(dropped));
    result
}
