//! Where on the screen a cursor on a page is, and which character of a row
//! a point on the screen is: the drawing's half of the seam that
//! [`obelus_component::page`] is the other half of.
//!
//! A row is counted in characters and a screen in cells, and the two are
//! not the same number: a wide glyph is two cells, and whatever a page
//! draws in front of a row's words -- a mark saying who spoke, an indent --
//! is several. So what column a character of a row is drawn at is known
//! here, given where the row's words begin, and the pointer and the caret
//! cross the seam by the same arithmetic.

/// Which character of a row a cell of the screen is on, for a row whose
/// words begin at cell `begin`.
///
/// Past the end of the row is the row's length, which is the place after
/// its last character.
#[must_use]
pub fn character_under(text: &str, begin: u16, x: u16) -> usize {
    let mut column = begin;
    let mut seen = 0usize;
    for (_, wide) in obelus_text::drawn_widths(text) {
        if x < column + wide {
            return seen;
        }
        column += wide;
        seen += 1;
    }
    seen
}

/// The cell a character of a row is drawn at, for a row whose words begin
/// at cell `begin`.
///
/// The way back across the seam [`character_under`] crosses the other way,
/// for putting the terminal's caret where the cursor is. Past the end of
/// the row is the cell after its last character, which is where a caret at
/// the end of a line belongs.
#[must_use]
pub fn cell_of(text: &str, begin: u16, characters: usize) -> u16 {
    let mut column = begin;
    for (seen, (_, wide)) in obelus_text::drawn_widths(text).enumerate() {
        if seen == characters {
            return column;
        }
        column += wide;
    }
    column
}

/// Says that enter copies the block of code the cursor is in, on the
/// block's own box: along the side of it written on `y`, which ends at
/// cell `ended` and is drawn by `side` -- its bottom, short of the corner,
/// or its top where the bottom is off the screen.
///
/// Not after the last row, which is where enter is said for everything
/// else: the box is the whole width, so beside it is nowhere, and the row
/// under it is the next paragraph as often as it is a blank. And a side of
/// the box is a row with nothing written on it, so the key takes no room
/// from the code.
///
/// One piece of code for every page a block of code is on -- the
/// transcript and a pull request's -- because it is one key saying one
/// thing about one kind of row.
pub(crate) fn offer_copy(
    cells: &mut ratatui::buffer::Buffer,
    side: &[obelus_row::Span],
    y: u16,
    ended: u16,
    dim: ratatui::style::Style,
    theme: &obelus_theme::Theme,
) {
    use crossterm::event::{KeyCode, KeyModifiers};

    // The box is the last run of its row: a quote's bar or a list's indent
    // is in front of it.
    let Some(edge) = side.last().map(|span| crate::text_width(&span.text)) else {
        return;
    };
    let keys = [(
        crate::chat::chord(KeyCode::Enter, KeyModifiers::NONE),
        "Copies the code",
    )];
    let Some(said) = crate::chat::joined(&keys) else {
        return;
    };
    let said = format!(" {said} ");
    // Two of the line between it and the corner, and three at the other
    // end: a key that met the corner would read as the box being open
    // there.
    let wide = crate::text_width(&said);
    let Some(at) = usize::from(ended).checked_sub(wide + 3) else {
        return;
    };
    if at < usize::from(ended).saturating_sub(edge) + 4 {
        return;
    }
    let Ok(at) = u16::try_from(at) else {
        return;
    };
    crate::write(cells, at, y, &said, dim);
    let ground = dim.bg.unwrap_or(theme.background);
    crate::chat::cap_the_keys(at + 1, y, &keys, ground, theme);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A wide character is two cells and one character, both ways.
    ///
    /// Broken deliberately by counting a cell as a character in
    /// `character_under`: the press on the second half of 中 lands on the
    /// character after it.
    #[test]
    fn a_wide_character_is_two_cells_and_one_character() {
        let text = "a中b";
        assert_eq!(cell_of(text, 3, 0), 3);
        assert_eq!(cell_of(text, 3, 1), 4);
        assert_eq!(cell_of(text, 3, 2), 6);
        assert_eq!(cell_of(text, 3, 3), 7, "the end of the row");
        assert_eq!(character_under(text, 3, 5), 1, "the second half of 中");
        assert_eq!(character_under(text, 3, 6), 2);
        assert_eq!(character_under(text, 3, 40), 3, "past the end");
        assert_eq!(character_under(text, 3, 0), 0, "before the words");
    }
}
