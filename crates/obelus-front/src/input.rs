//! What a front end tells Obelus: the reader's keys and pointer, the size
//! of the screen, and what the window alone can hear.
//!
//! The events of Obelus's loop that a front end makes and no other part of
//! Obelus does, in a form that crosses. One translation each way, so that a
//! window on the same machine and one on another say the same things in
//! the same words: [`Input::of`] for what the window heard, and
//! [`Input::into_event`] for the loop at the far end.
//!
//! **A resize carries the size.** On one machine the window writes the
//! size where the backend reads it (`Measured`) and the event says only to
//! look; across a connection there is nowhere shared to write it, so the
//! size goes with the event and is written down on arrival, before the loop
//! is told to look.

use crossterm::event::KeyEvent;
use obelus_app::event::{Event, Pointer};
use obelus_buffer::DocumentId;

use crate::Measured;

/// One thing a front end tells Obelus.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Input {
    /// A key, for the key table.
    Key(KeyEvent),
    /// A key a window knows to be a command's.
    Command(KeyEvent),
    /// The screen is this size now.
    Resize {
        /// How many columns.
        columns: u16,
        /// How many rows.
        rows: u16,
        /// How many pixels across, where the front end has pixels.
        width: u16,
        /// And down.
        height: u16,
    },
    /// The faces the window's machine can draw with.
    Fonts {
        /// Every one.
        here: Vec<String>,
        /// What the machine falls back to.
        otherwise: Option<String>,
    },
    /// The reader closed the window.
    Closed,
    /// Shift went down or up.
    Shifted(bool),
    /// The wheel turned.
    Scroll {
        /// How many rows, down where positive.
        rows: i64,
        /// Over which cell, where the front end says.
        at: Option<(u16, u16)>,
    },
    /// The pointer did something.
    Pointer {
        /// What.
        kind: Pointer,
        /// The column.
        x: u16,
        /// The row.
        y: u16,
    },
    /// A tab on the title bar was chosen.
    Chose(DocumentId),
    /// A tab on the title bar was closed.
    Shut(DocumentId),
    /// A tab was dragged beside another.
    Placed {
        /// The tab.
        document: DocumentId,
        /// The one it went beside.
        beside: DocumentId,
        /// Whether after it.
        after: bool,
    },
    /// Words put in, all at once.
    Paste(String),
    /// What an input method is still spelling, and where its caret is.
    Spelling(String, Option<u64>),
    /// Another Obelus asked for this one to come forward.
    Summoned(Option<String>),
}

impl Input {
    /// What a window heard, in these words -- or nothing, for an event that
    /// is not a front end's to say.
    ///
    /// `measured` is where the window wrote the size, which a resize takes
    /// with it.
    #[must_use]
    pub fn of(event: Event, measured: &Measured) -> Option<Self> {
        Some(match event {
            Event::Key(key) => Self::Key(key),
            Event::Command(key) => Self::Command(key),
            Event::Resize => {
                let size = measured.size();
                let pixels = measured.pixels();
                Self::Resize {
                    columns: size.width,
                    rows: size.height,
                    width: pixels.width,
                    height: pixels.height,
                }
            }
            Event::Fonts { here, otherwise } => Self::Fonts { here, otherwise },
            Event::Closed => Self::Closed,
            Event::Shifted(shifted) => Self::Shifted(shifted),
            Event::Scroll { rows, at } => Self::Scroll {
                rows: i64::try_from(rows).unwrap_or(0),
                at,
            },
            Event::Pointer { kind, x, y } => Self::Pointer { kind, x, y },
            Event::Chose(document) => Self::Chose(document),
            Event::Shut(document) => Self::Shut(document),
            Event::Placed {
                document,
                beside,
                after,
            } => Self::Placed {
                document,
                beside,
                after,
            },
            Event::Paste(words) => Self::Paste(words),
            Event::Spelling(spelling, caret) => {
                Self::Spelling(spelling, caret.and_then(|caret| u64::try_from(caret).ok()))
            }
            // Not a path: a path here means nothing on the machine Obelus
            // is on, so a file dropped crosses whole (`client`).
            Event::Dropped(_) => return None,
            Event::Summoned(token) => Self::Summoned(token),
            _ => return None,
        })
    }

    /// The event Obelus's loop hears for this, with a size written down
    /// in `measured` first.
    #[must_use]
    pub fn into_event(self, measured: &Measured) -> Event {
        match self {
            Self::Key(key) => Event::Key(key),
            Self::Command(key) => Event::Command(key),
            Self::Resize {
                columns,
                rows,
                width,
                height,
            } => {
                measured.resized(columns, rows, u32::from(width), u32::from(height));
                Event::Resize
            }
            Self::Fonts { here, otherwise } => Event::Fonts { here, otherwise },
            Self::Closed => Event::Closed,
            Self::Shifted(shifted) => Event::Shifted(shifted),
            Self::Scroll { rows, at } => Event::Scroll {
                rows: isize::try_from(rows).unwrap_or(0),
                at,
            },
            Self::Pointer { kind, x, y } => Event::Pointer { kind, x, y },
            Self::Chose(document) => Event::Chose(document),
            Self::Shut(document) => Event::Shut(document),
            Self::Placed {
                document,
                beside,
                after,
            } => Event::Placed {
                document,
                beside,
                after,
            },
            Self::Paste(words) => Event::Paste(words),
            Self::Spelling(spelling, caret) => Event::Spelling(
                spelling,
                caret.and_then(|caret| usize::try_from(caret).ok()),
            ),
            Self::Summoned(token) => Event::Summoned(token),
        }
    }
}
