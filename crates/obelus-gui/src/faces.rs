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

use cosmic_text::fontdb;

/// Adds the faces the walk did not find, where this machine can say.
pub(crate) fn add_the_rest(db: &mut fontdb::Database) {
    let added = of_this_platform(db);
    tracing::info!(added, faces = db.len(), "the faces this machine has");
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
