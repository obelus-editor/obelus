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
