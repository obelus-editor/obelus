//! Where the frame drew its links, for a click that follows one.
//!
//! What a click follows has to be what was on the screen under it: a link's
//! words can be cut short by what a row says at its end, and the cells in
//! front of a row's words are nobody's. Worked out a second time from the
//! characters, the click opened a link from the margin beside it. So the
//! view that draws a link says where it drew it, and the frame hands that
//! back with the bars ([`crate::Left`]).
//!
//! Kept here rather than in what the link is part of: a conversation's
//! transcript used to keep them, written into it while it was being drawn,
//! which made drawing a thing that changed what it drew. Scratch for one
//! frame on the drawing thread, the way [`crate::bars`] is: [`said`] is only
//! listening inside [`collect`], so a view drawn by a test on its own records
//! nothing and needs nothing.

use std::{cell::RefCell, ops::Range};

/// A link the frame drew.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Drawn {
    /// The row of the screen.
    pub row: u16,
    /// The cells across it.
    pub cells: Range<u16>,
    /// Where it goes.
    pub to: String,
}

thread_local! {
    /// The links drawn so far this frame, while a frame is listening.
    static SAID: RefCell<Option<Vec<Drawn>>> = const { RefCell::new(None) };
}

/// Draws a frame and hands back the links it drew.
pub(crate) fn collect<T>(draw: impl FnOnce() -> T) -> (T, Vec<Drawn>) {
    SAID.with(|said| *said.borrow_mut() = Some(Vec::new()));
    let drawn = draw();
    let links = SAID
        .with(|said| said.borrow_mut().take())
        .unwrap_or_default();
    (drawn, links)
}

/// A link's words were drawn across these cells of a row of the screen.
pub(crate) fn said(row: u16, cells: Range<u16>, to: &str) {
    SAID.with(|said| {
        if let Some(said) = said.borrow_mut().as_mut() {
            said.push(Drawn {
                row,
                cells,
                to: to.to_string(),
            });
        }
    });
}

/// Where the link drawn at this cell goes, if one was drawn there.
#[must_use]
pub fn at(links: &[Drawn], x: u16, y: u16) -> Option<&str> {
    links
        .iter()
        .find(|link| link.row == y && link.cells.contains(&x))
        .map(|link| link.to.as_str())
}
