//! Positions, in whichever units the server agreed to.
//!
//! A server that accepts `utf-8` makes a protocol position the same thing as a
//! tree-sitter point: a line, and bytes into that line. One that will not
//! counts UTF-16 code units, which differ from every other column Obelus has
//! the moment a character outside the basic multilingual plane appears.

use lsp_types::{Position, PositionEncodingKind};
use obelus_text::{
    Text,
    coordinates::{ByteOffset, CharColumn, LineNumber, Utf16Column},
};

/// The protocol position of a place in a document.
#[must_use]
pub fn to_lsp(
    text: &Text,
    line: LineNumber,
    column: CharColumn,
    encoding: &PositionEncodingKind,
) -> Position {
    let character = if is_utf8(encoding) {
        let offset = text.byte_of_char(text.char_offset(line, column));
        text.byte_column(offset)
    } else {
        text.utf16_column(line, column).get()
    };
    Position {
        line: u32::try_from(line.get()).unwrap_or(u32::MAX),
        character: u32::try_from(character).unwrap_or(u32::MAX),
    }
}

/// The place in a document a protocol position names.
#[must_use]
pub fn from_lsp(
    text: &Text,
    position: Position,
    encoding: &PositionEncodingKind,
) -> (LineNumber, CharColumn) {
    let line = text.clamp_line(LineNumber::new(position.line as usize));
    let character = position.character as usize;
    let column = if is_utf8(encoding) {
        let start = text.line_start_byte(line).get();
        text.char_of_byte(ByteOffset::new(start.saturating_add(character)))
            .get()
            .saturating_sub(text.char_offset(line, CharColumn::new(0)).get())
    } else {
        text.column_at_utf16(line, Utf16Column::new(character))
            .get()
    };
    (line, text.clamp_column(line, CharColumn::new(column)))
}

/// Whether positions are counted in bytes.
fn is_utf8(encoding: &PositionEncodingKind) -> bool {
    *encoding == PositionEncodingKind::UTF8
}
