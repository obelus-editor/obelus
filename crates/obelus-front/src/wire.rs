//! A colour, a cell and a mark's palette, as they cross to a window that
//! may be on another machine.
//!
//! **Obelus's own spelling and not ratatui's.** ratatui can write its
//! colours, but reads them back by trying one shape and then another
//! (`untagged`), which needs a format that says what it holds -- and the
//! connection's does not (`obelus_wire`). So these are spelled here, one
//! shape each, and turned back into ratatui's as they arrive. The rest of
//! what crosses -- a place, a size, a cell's modifiers and how it is diffed
//! -- ratatui writes in a way that reads back, and goes as it writes it.
//!
//! **A run of cells in one style crosses once.** A frame's cells are mostly
//! side by side on a row and alike -- a word, a stretch of blank -- and a
//! cell spelled whole for each was sixteen bytes and two allocations a
//! cell, a third of a megabyte and milliseconds each way for a page
//! scrolled. So the cells beside each other on a row that are one character
//! each, in one style and diffed the ordinary way, cross as where the run
//! starts, its style once, and its characters ([`updates`]) -- and are
//! cells again at the other end, so nothing that draws them knows.

use ratatui::{
    buffer::{Cell, CellDiffOption},
    style::{Color, Modifier},
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A colour, one shape for each of ratatui's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Ink {
    Reset,
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    Gray,
    DarkGray,
    LightRed,
    LightGreen,
    LightYellow,
    LightBlue,
    LightMagenta,
    LightCyan,
    White,
    Rgb(u8, u8, u8),
    Indexed(u8),
}

impl From<Color> for Ink {
    fn from(colour: Color) -> Self {
        match colour {
            Color::Reset => Self::Reset,
            Color::Black => Self::Black,
            Color::Red => Self::Red,
            Color::Green => Self::Green,
            Color::Yellow => Self::Yellow,
            Color::Blue => Self::Blue,
            Color::Magenta => Self::Magenta,
            Color::Cyan => Self::Cyan,
            Color::Gray => Self::Gray,
            Color::DarkGray => Self::DarkGray,
            Color::LightRed => Self::LightRed,
            Color::LightGreen => Self::LightGreen,
            Color::LightYellow => Self::LightYellow,
            Color::LightBlue => Self::LightBlue,
            Color::LightMagenta => Self::LightMagenta,
            Color::LightCyan => Self::LightCyan,
            Color::White => Self::White,
            Color::Rgb(red, green, blue) => Self::Rgb(red, green, blue),
            Color::Indexed(index) => Self::Indexed(index),
        }
    }
}

impl From<Ink> for Color {
    fn from(ink: Ink) -> Self {
        match ink {
            Ink::Reset => Self::Reset,
            Ink::Black => Self::Black,
            Ink::Red => Self::Red,
            Ink::Green => Self::Green,
            Ink::Yellow => Self::Yellow,
            Ink::Blue => Self::Blue,
            Ink::Magenta => Self::Magenta,
            Ink::Cyan => Self::Cyan,
            Ink::Gray => Self::Gray,
            Ink::DarkGray => Self::DarkGray,
            Ink::LightRed => Self::LightRed,
            Ink::LightGreen => Self::LightGreen,
            Ink::LightYellow => Self::LightYellow,
            Ink::LightBlue => Self::LightBlue,
            Ink::LightMagenta => Self::LightMagenta,
            Ink::LightCyan => Self::LightCyan,
            Ink::White => Self::White,
            Ink::Rgb(red, green, blue) => Self::Rgb(red, green, blue),
            Ink::Indexed(index) => Self::Indexed(index),
        }
    }
}

/// A cell, with its colours spelled the way they cross.
#[derive(Serialize, Deserialize)]
struct Spelled {
    /// What is in it. A cell nobody has written in crosses as a space,
    /// which ratatui holds to be the same cell.
    symbol: String,
    fg: Ink,
    bg: Ink,
    underline: Ink,
    modifier: Modifier,
    diff: CellDiffOption,
}

impl From<&Cell> for Spelled {
    fn from(cell: &Cell) -> Self {
        Self {
            symbol: cell.symbol().to_string(),
            fg: cell.fg.into(),
            bg: cell.bg.into(),
            underline: cell.underline_color.into(),
            modifier: cell.modifier,
            diff: cell.diff_option,
        }
    }
}

impl From<Spelled> for Cell {
    fn from(spelled: Spelled) -> Self {
        let mut cell = Self::default();
        cell.set_symbol(&spelled.symbol);
        cell.fg = spelled.fg.into();
        cell.bg = spelled.bg.into();
        cell.underline_color = spelled.underline.into();
        cell.modifier = spelled.modifier;
        cell.diff_option = spelled.diff;
        cell
    }
}

/// A colour: `#[serde(with = "wire::ink")]`.
pub mod ink {
    use super::{Color, Deserialize, Deserializer, Ink, Serialize, Serializer};

    /// Writes one.
    ///
    /// # Errors
    ///
    /// The format's.
    pub fn serialize<S: Serializer>(colour: &Color, to: S) -> Result<S::Ok, S::Error> {
        Ink::from(*colour).serialize(to)
    }

    /// Reads one.
    ///
    /// # Errors
    ///
    /// The format's.
    pub fn deserialize<'de, D: Deserializer<'de>>(from: D) -> Result<Color, D::Error> {
        Ink::deserialize(from).map(Color::from)
    }
}

/// A cell, boxed: `#[serde(with = "wire::boxed_cell")]`.
pub mod boxed_cell {
    use super::{Cell, Deserialize, Deserializer, Serialize, Serializer, Spelled};

    /// Writes one.
    ///
    /// # Errors
    ///
    /// The format's.
    #[expect(
        clippy::borrowed_box,
        reason = "what serde hands a field's writer is the field"
    )]
    pub fn serialize<S: Serializer>(cell: &Box<Cell>, to: S) -> Result<S::Ok, S::Error> {
        Spelled::from(&**cell).serialize(to)
    }

    /// Reads one.
    ///
    /// # Errors
    ///
    /// The format's.
    pub fn deserialize<'de, D: Deserializer<'de>>(from: D) -> Result<Box<Cell>, D::Error> {
        Spelled::deserialize(from).map(|spelled| Box::new(spelled.into()))
    }
}

/// Cells, a row of them or a region: `#[serde(with = "wire::cells")]`.
pub mod cells {
    use super::{Cell, Deserialize, Deserializer, Serialize, Serializer, Spelled};

    /// Writes them.
    ///
    /// # Errors
    ///
    /// The format's.
    pub fn serialize<S: Serializer>(cells: &[Cell], to: S) -> Result<S::Ok, S::Error> {
        cells
            .iter()
            .map(Spelled::from)
            .collect::<Vec<_>>()
            .serialize(to)
    }

    /// Reads them.
    ///
    /// # Errors
    ///
    /// The format's.
    pub fn deserialize<'de, D: Deserializer<'de>>(from: D) -> Result<Vec<Cell>, D::Error> {
        Vec::<Spelled>::deserialize(from).map(|cells| cells.into_iter().map(Cell::from).collect())
    }
}

/// Bytes, as bytes: `#[serde(with = "wire::bytes")]`.
///
/// Not as the sequence of numbers serde makes of a `Vec<u8>` left to
/// itself, which postcard writes the same way but a call at a time: a
/// picture of megabytes was as many calls each way -- a third of a second
/// in a debug build -- for what is one copy.
pub mod bytes {
    use super::{Deserializer, Serializer};

    /// Writes them.
    ///
    /// # Errors
    ///
    /// The format's.
    pub fn serialize<S: Serializer>(bytes: &[u8], to: S) -> Result<S::Ok, S::Error> {
        to.serialize_bytes(bytes)
    }

    /// Reads them.
    ///
    /// # Errors
    ///
    /// The format's.
    pub fn deserialize<'de, D: Deserializer<'de>>(from: D) -> Result<Vec<u8>, D::Error> {
        from.deserialize_byte_buf(Bytes)
    }

    /// What reads them, whichever way a format hands them over.
    struct Bytes;

    impl<'de> serde::de::Visitor<'de> for Bytes {
        type Value = Vec<u8>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("bytes")
        }

        fn visit_bytes<E: serde::de::Error>(self, bytes: &[u8]) -> Result<Vec<u8>, E> {
            Ok(bytes.to_vec())
        }

        fn visit_byte_buf<E: serde::de::Error>(self, bytes: Vec<u8>) -> Result<Vec<u8>, E> {
            Ok(bytes)
        }

        fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<u8>, A::Error> {
            let mut bytes = Vec::with_capacity(seq.size_hint().unwrap_or(0).min(1 << 20));
            while let Some(byte) = seq.next_element()? {
                bytes.push(byte);
            }
            Ok(bytes)
        }
    }
}

/// A frame's updates, with every run of alike cells crossing once:
/// `#[serde(with = "wire::updates")]`.
pub mod updates {
    use super::{
        Cell, CellDiffOption, Deserialize, Deserializer, Ink, Modifier, Serialize, Serializer,
    };
    use crate::Update;

    /// One thing said about the screen, as it crosses -- with the same
    /// variants in the same order as [`Said`], which is what reads it.
    #[derive(Serialize)]
    enum Saying<'a> {
        Update(&'a Update),
        Run(Run),
    }

    /// The same, read.
    #[derive(Deserialize)]
    enum Said {
        Update(Update),
        Run(Run),
    }

    /// Cells side by side on a row, in one style: where the first is, the
    /// style, and a character for each.
    #[derive(Serialize, Deserialize)]
    struct Run {
        x: u16,
        y: u16,
        fg: Ink,
        bg: Ink,
        underline: Ink,
        modifier: Modifier,
        text: String,
        /// How many cells it is so far, while it is being made: counted as
        /// it grows, because counting the text at every cell made a row of
        /// blank a cost of the row's square.
        #[serde(skip)]
        cells: usize,
    }

    impl Run {
        /// The run a cell starts, where it can be in one: one character,
        /// diffed the ordinary way. A grapheme of several characters, or a
        /// cell diffed some other way, crosses as itself.
        fn of(x: u16, y: u16, cell: &Cell) -> Option<Self> {
            let lone = lone(cell)?;
            Some(Self {
                x,
                y,
                fg: cell.fg.into(),
                bg: cell.bg.into(),
                underline: cell.underline_color.into(),
                modifier: cell.modifier,
                text: lone.to_string(),
                cells: 1,
            })
        }

        /// Takes in the cell at `x`, `y`, where it is the next one along
        /// and alike, and says whether it did.
        fn took(&mut self, x: u16, y: u16, cell: &Cell) -> bool {
            let Some(lone) = lone(cell) else {
                return false;
            };
            let next = usize::from(self.x) + self.cells;
            let alike = Ink::from(cell.fg) == self.fg
                && Ink::from(cell.bg) == self.bg
                && Ink::from(cell.underline_color) == self.underline
                && cell.modifier == self.modifier;
            if y != self.y || usize::from(x) != next || !alike {
                return false;
            }
            self.text.push(lone);
            self.cells += 1;
            true
        }

        /// The cells again, onto the end of `updates`.
        fn cells_onto(self, updates: &mut Vec<Update>) {
            let (fg, bg, underline) = (self.fg.into(), self.bg.into(), self.underline.into());
            for (character, x) in self.text.chars().zip(self.x..) {
                let mut cell = Cell::default();
                cell.set_char(character);
                cell.fg = fg;
                cell.bg = bg;
                cell.underline_color = underline;
                cell.modifier = self.modifier;
                updates.push(Update::Cell {
                    x,
                    y: self.y,
                    cell: Box::new(cell),
                });
            }
        }
    }

    /// The one character a cell holds, where it holds one and is diffed
    /// the ordinary way.
    fn lone(cell: &Cell) -> Option<char> {
        if cell.diff_option != CellDiffOption::None {
            return None;
        }
        let mut characters = cell.symbol().chars();
        let lone = characters.next()?;
        characters.next().is_none().then_some(lone)
    }

    /// Writes them.
    ///
    /// # Errors
    ///
    /// The format's.
    pub fn serialize<S: Serializer>(updates: &[Update], to: S) -> Result<S::Ok, S::Error> {
        let mut saying: Vec<Saying<'_>> = Vec::with_capacity(updates.len() / 4);
        for update in updates {
            if let Update::Cell { x, y, cell } = update {
                if let Some(Saying::Run(run)) = saying.last_mut()
                    && run.took(*x, *y, cell)
                {
                    continue;
                }
                if let Some(run) = Run::of(*x, *y, cell) {
                    saying.push(Saying::Run(run));
                    continue;
                }
            }
            saying.push(Saying::Update(update));
        }
        saying.serialize(to)
    }

    /// Reads them.
    ///
    /// # Errors
    ///
    /// The format's.
    pub fn deserialize<'de, D: Deserializer<'de>>(from: D) -> Result<Vec<Update>, D::Error> {
        let said = Vec::<Said>::deserialize(from)?;
        let mut updates = Vec::with_capacity(said.len() * 4);
        for said in said {
            match said {
                Said::Update(update) => updates.push(update),
                Said::Run(run) => run.cells_onto(&mut updates),
            }
        }
        Ok(updates)
    }
}

/// The colours a mark is inked in: `#[serde(with = "wire::palette")]`.
pub mod palette {
    use obelus_ui::image::Palette;

    use super::{Deserialize, Deserializer, Ink, Serialize, Serializer};

    /// Writes one.
    ///
    /// # Errors
    ///
    /// The format's.
    pub fn serialize<S: Serializer>(palette: &Palette, to: S) -> Result<S::Ok, S::Error> {
        (
            Ink::from(palette.ink),
            Ink::from(palette.paper),
            Ink::from(palette.selected),
        )
            .serialize(to)
    }

    /// Reads one.
    ///
    /// # Errors
    ///
    /// The format's.
    pub fn deserialize<'de, D: Deserializer<'de>>(from: D) -> Result<Palette, D::Error> {
        let (ink, paper, selected) = <(Ink, Ink, Ink)>::deserialize(from)?;
        Ok(Palette {
            ink: ink.into(),
            paper: paper.into(),
            selected: selected.into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use ratatui::buffer::{Cell, CellDiffOption};

    use crate::{Update, link::Down};

    /// A cell at a place, its symbol and its foreground the given ones.
    fn cell(x: u16, y: u16, symbol: &str, fg: ratatui::style::Color) -> Update {
        let mut cell = Cell::default();
        cell.set_symbol(symbol);
        cell.fg = fg;
        Update::Cell {
            x,
            y,
            cell: Box::new(cell),
        }
    }

    /// A frame's updates read back as they were written: runs, and every
    /// way a run is broken -- another style, a gap, another row, a wide
    /// character, a grapheme of several characters, a cell diffed some
    /// other way, and something that is not a cell.
    ///
    /// Broken deliberately by starting each character of a run where the
    /// run starts (`zip(self.x..)` for `zip(std::iter::repeat(self.x))` in
    /// `cells_onto`): every cell of a run landed in its first column.
    #[test]
    fn a_frame_reads_back_as_it_was_written() {
        use ratatui::style::Color;

        let mut skipped = Cell::new("s");
        skipped.diff_option = CellDiffOption::Skip;
        let updates = vec![
            Update::Cleared,
            cell(0, 0, "a", Color::Red),
            cell(1, 0, "b", Color::Red),
            cell(2, 0, "c", Color::Blue),
            cell(4, 0, "d", Color::Blue),
            cell(5, 0, "\u{4e2d}", Color::Blue),
            cell(7, 0, "e", Color::Blue),
            cell(8, 0, "e\u{301}", Color::Blue),
            cell(0, 1, "f", Color::Blue),
            Update::Cell {
                x: 1,
                y: 1,
                cell: Box::new(skipped),
            },
            cell(2, 1, "g", Color::Blue),
            Update::Caret(None),
            cell(3, 1, "h", Color::Blue),
            Update::Frame,
        ];
        let framed = obelus_wire::framed(&Down::Updates(updates.clone())).expect("framed");
        let read: Down = obelus_wire::unframed(&framed[4..]).expect("read");
        assert_eq!(read, Down::Updates(updates));
    }

    /// A row of cells in one style crosses as far fewer bytes than a cell
    /// each would.
    ///
    /// Broken deliberately by letting no cell into a run (`took` answering
    /// `false`): every cell crossed as itself, at the size it always had.
    #[test]
    fn a_row_in_one_style_crosses_small() {
        let row: Vec<Update> = (0..200)
            .map(|x| cell(x, 0, "x", ratatui::style::Color::Rgb(1, 2, 3)))
            .collect();
        let together = obelus_wire::framed(&Down::Updates(row.clone())).expect("framed");
        let alone: usize = row
            .iter()
            .map(|update| obelus_wire::framed(update).expect("framed").len())
            .sum();
        assert!(
            together.len() * 8 < alone,
            "{} bytes together against {alone} alone",
            together.len()
        );
    }
}
