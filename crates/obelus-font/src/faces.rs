//! Which faces this machine has.
//!
//! fontdb does not ask anybody: it walks the directories fonts are usually
//! kept in, and a face kept anywhere else is not on this machine as far as
//! Obelus can tell. On Linux the directories are fontconfig's own, read out
//! of its configuration, and on Windows the system's and the reader's, which
//! is where an installer puts them. On macOS the walk is a guess, and since
//! macOS 15 a wrong one about the face that matters most: `PingFang SC`, the
//! face the system draws Chinese in and the one cosmic-text falls back to
//! for it, is in
//!
//! ```text
//! /System/Library/PrivateFrameworks/FontServices.framework/Resources/Reserved/PingFangUI.ttc
//! ```
//!
//! which no walk visits. So it was missing from the list a reader chooses
//! from, and the fallback that names it drew Chinese in whatever came after.
//!
//! CoreText knows where every face it will draw with is kept, so on macOS it
//! is asked, and what the walk missed is added to what it found.
//!
//! **A face Obelus cannot draw is not a face it has.** `PingFang SC` turned
//! out to be the reason it is kept where it is: its outlines are in `hvgl`,
//! a table of Apple's that CoreText draws and swash does not. Added as it
//! came, it was the face Chinese was shaped in -- its character map is
//! ordinary -- and then drawn as nothing at all, every character of it,
//! where before the fallback had stepped past the name to a face it could
//! draw. So a face is kept only where it has outlines or pictures swash
//! reads, or where CoreText draws it instead ([`crate::coretext`]) -- which
//! is a question about the face and not about where it was found, and is
//! asked of every face the same way.

use std::collections::HashMap;

use cosmic_text::{
    fontdb,
    skrifa::raw::{FontRef, types::Tag},
};

/// The tables a glyph can be drawn from: outlines, as TrueType and as the two
/// kinds of CFF, and pictures, as Apple's and as Google's.
///
/// Not the older bitmaps, `EBDT`, though swash reads them: cosmic-text asks
/// it for colour and for outlines and never for a plain bitmap, so a face
/// that has nothing else draws as nothing -- which is the face this list is
/// here to let go of.
const DRAWN_FROM: [&[u8; 4]; 5] = [b"glyf", b"CFF ", b"CFF2", b"sbix", b"CBDT"];

/// Adds the faces the walk did not find, where this machine can say, and
/// lets go of the ones that cannot be drawn.
///
/// What it hands back is the faces CoreText draws rather than swash, by the
/// PostScript name CoreText knows each by.
pub(crate) fn settle(db: &mut fontdb::Database) -> HashMap<fontdb::ID, String> {
    let added = of_this_platform(db);
    tracing::info!(added, "the faces the machine named that the walk did not");
    keep_what_can_be_drawn(db, crate::coretext::draws)
}

/// Lets go of the faces neither swash nor CoreText draws, and says which of
/// the rest are CoreText's.
fn keep_what_can_be_drawn(
    db: &mut fontdb::Database,
    coretext_draws: impl Fn(&str) -> bool,
) -> HashMap<fontdb::ID, String> {
    let not_swash: Vec<(fontdb::ID, String)> = db
        .faces()
        .filter(|face| db.with_face_data(face.id, can_be_drawn) == Some(false))
        .map(|face| (face.id, face.post_script_name.clone()))
        .collect();
    let mut by_coretext = HashMap::new();
    let mut undrawable = 0_usize;
    for (id, name) in not_swash {
        match coretext_draws(&name) {
            true => {
                by_coretext.insert(id, name);
            }
            false => {
                db.remove_face(id);
                undrawable += 1;
            }
        }
    }
    tracing::info!(
        by_coretext = by_coretext.len(),
        undrawable,
        faces = db.len(),
        "the faces this machine has"
    );
    by_coretext
}

/// Whether a face has anything swash can draw a glyph from.
pub(crate) fn can_be_drawn(data: &[u8], index: u32) -> bool {
    FontRef::from_index(data, index).is_ok_and(|font| {
        DRAWN_FROM
            .iter()
            .any(|tag| font.table_data(Tag::new(tag)).is_some())
    })
}

/// macOS asks CoreText for the file every face it has is in.
#[cfg(target_os = "macos")]
fn of_this_platform(db: &mut fontdb::Database) -> usize {
    use objc2_core_foundation::{CFRetained, CFURL};

    let found: std::collections::HashSet<std::path::PathBuf> = db
        .faces()
        .filter_map(|face| match &face.source {
            fontdb::Source::File(path) | fontdb::Source::SharedFile(path, _) => Some(path.clone()),
            fontdb::Source::Binary(_) => None,
        })
        .collect();
    // SAFETY: takes nothing, and hands back an array the caller owns.
    let urls = unsafe { objc2_core_text::CTFontManagerCopyAvailableFontURLs() };
    // SAFETY: an array of `CFURL`s is what it is documented to return.
    let urls = unsafe { CFRetained::cast_unchecked::<objc2_core_foundation::CFArray<CFURL>>(urls) };
    let before = db.len();
    // One file holds several faces, and CoreText names it once per face.
    let mut seen = std::collections::HashSet::new();
    for url in &*urls {
        let Some(path) = url.to_file_path() else {
            continue;
        };
        if found.contains(&path) || !seen.insert(path.clone()) {
            continue;
        }
        if let Err(error) = db.load_font_file(&path) {
            tracing::debug!(?path, %error, "a face CoreText named did not load");
        }
    }
    db.len() - before
}

/// Everywhere else the walk is the system's own answer.
#[cfg(not(target_os = "macos"))]
const fn of_this_platform(_: &mut fontdb::Database) -> usize {
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A face with outlines is kept, and one whose glyphs live in a table
    /// nothing here can read is not.
    ///
    /// The second is the face Obelus carries with its `glyf` renamed, which
    /// is what `PingFangUI.ttc` looks like from here: every table a face
    /// needs to be shaped, and none a glyph can be drawn from.
    ///
    /// Kept where CoreText says it draws it, and handed back as CoreText's;
    /// let go of where it does not.
    ///
    /// Deliberate breaks: `can_be_drawn` answering yes whatever the tables
    /// keeps the renamed face without CoreText, which is the one that drew
    /// Chinese as nothing; and not asking CoreText lets go of the face it
    /// would have drawn, which is `PingFang SC`.
    #[test]
    fn a_face_with_nothing_to_draw_from_is_not_kept() {
        assert!(can_be_drawn(CARRIED, 0));
        let renamed = outlines_called(b"hvgl");
        assert!(!can_be_drawn(&renamed, 0));

        let mut db = fontdb::Database::new();
        db.load_font_data(renamed.clone());
        assert!(keep_what_can_be_drawn(&mut db, |_| false).is_empty());
        assert_eq!(db.len(), 0, "a face nothing draws was kept");

        let mut db = fontdb::Database::new();
        db.load_font_data(renamed);
        let by_coretext = keep_what_can_be_drawn(&mut db, |_| true);
        assert_eq!(db.len(), 1, "a face CoreText draws was let go of");
        let id = db.faces().next().expect("the one face").id;
        assert!(by_coretext.contains_key(&id), "and it is not CoreText's");
    }

    /// Nor is a face whose glyphs are the older bitmaps alone, which swash
    /// can read and cosmic-text never asks it to.
    ///
    /// Deliberate break: putting `EBDT` back in `DRAWN_FROM` keeps it.
    #[test]
    fn a_face_of_plain_bitmaps_is_not_one_that_draws() {
        assert!(!can_be_drawn(&outlines_called(b"EBDT"), 0));
    }

    /// The face Obelus carries.
    const CARRIED: &[u8] = include_bytes!("../fonts/SymbolsNerdFontMono-Regular.ttf");

    /// The face Obelus carries, with its outlines in a table by this name:
    /// every table a face needs to be shaped, and the glyphs somewhere else.
    fn outlines_called(tag: &[u8; 4]) -> Vec<u8> {
        let mut renamed = CARRIED.to_vec();
        // The table directory: a twelve-byte header, and sixteen bytes a
        // table, the tag first.
        let tables = usize::from(u16::from_be_bytes([renamed[4], renamed[5]]));
        let glyf = (0..tables)
            .map(|table| 12 + 16 * table)
            .find(|at| &renamed[*at..*at + 4] == b"glyf")
            .expect("the face Obelus carries has outlines");
        renamed[glyf..glyf + 4].copy_from_slice(tag);
        renamed
    }
}
