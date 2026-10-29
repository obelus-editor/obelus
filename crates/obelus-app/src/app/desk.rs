//! What only the reader's own machine can do: its clipboard, and the
//! browser a link opens in.
//!
//! **The reader's machine is not always the one Obelus runs on.** Drawn on
//! a window at the other end of a connection, what is copied belongs on the
//! clipboard of the machine the reader is sitting at, and a link opens in
//! *their* browser -- the machine Obelus runs on may have neither, and is
//! certainly not where the reader is looking. So everything that reaches
//! for one goes through [`Desk`], which is the machine Obelus runs on until
//! a front end says otherwise ([`App::desked_by`]).
//!
//! A file dropped on Obelus is not here: what arrives is already a path,
//! and the front end that heard the drop is the one that knows where the
//! file is.

use std::sync::Arc;

use super::App;

/// The reader's own machine, as far as Obelus reaches for it.
pub trait Desk: std::fmt::Debug + Send + Sync {
    /// Puts words on the clipboard.
    ///
    /// # Errors
    ///
    /// Where the clipboard would not take them.
    fn copy(&self, text: &str) -> std::io::Result<()>;

    /// The words on the clipboard, where there are any.
    fn paste(&self) -> Option<String>;

    /// The picture on the clipboard: here, none, or on its way.
    fn picture(&self) -> Picture;

    /// Whether what is on the clipboard is a picture.
    fn holds_a_picture(&self) -> bool;

    /// Opens a link where the reader will see it.
    ///
    /// # Errors
    ///
    /// Where nothing would open it.
    fn open(&self, url: &str) -> std::io::Result<()>;
}

/// The picture on the clipboard, as a paste is answered about it.
///
/// **A picture may be on its way rather than here.** On a machine at the
/// other end of a connection a screenshot is megabytes, and the loop
/// waiting for them to cross was a screen that stopped for as long as they
/// took -- so that answer is that one is coming, and the loop goes on.
#[derive(Debug)]
pub enum Picture {
    /// There is none.
    Nothing,
    /// This one, in this shape.
    Here(String, Vec<u8>),
    /// One is on its way: it arrives as `Event::Pictured`, or what became
    /// of it as `Event::Amiss`.
    Coming,
}

/// The machine Obelus runs on, which is the reader's whenever Obelus is
/// drawn there.
#[derive(Debug, Default)]
pub struct Here;

impl Desk for Here {
    fn copy(&self, text: &str) -> std::io::Result<()> {
        obelus_clipboard::copy(text)
    }

    fn paste(&self) -> Option<String> {
        obelus_clipboard::paste()
    }

    fn picture(&self) -> Picture {
        obelus_clipboard::picture().map_or(Picture::Nothing, |(shape, bytes)| {
            Picture::Here(shape, bytes)
        })
    }

    fn holds_a_picture(&self) -> bool {
        obelus_clipboard::holds_a_picture()
    }

    fn open(&self, url: &str) -> std::io::Result<()> {
        obelus_clipboard::links::open(url)
    }
}

impl App {
    /// Says whose clipboard and browser are the reader's, for Obelus drawn
    /// on a front end at the other end of a connection.
    pub fn desked_by(&mut self, desk: Arc<dyn Desk>) {
        self.desk = desk;
    }

    /// The reader's own machine.
    pub(in crate::app) fn desk(&self) -> &dyn Desk {
        &*self.desk
    }
}
