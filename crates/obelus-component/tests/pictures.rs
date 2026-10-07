//! What a hover's rows are laid out against, when a window changes which
//! characters it draws as pictures.
//!
//! A binary of its own, because the table is the whole process's.

use obelus_component::hover::Hover;
use obelus_lsp::hover::Hovered;
use obelus_text::coordinates::{CharColumn, LineNumber};

/// A hover lays itself out again once the window draws a heart as a
/// picture, two cells wide: nine `a`s and a heart are one row of ten cells
/// until then, and do not fit in one after.
///
/// Deliberate break: keying what was laid out on the width alone, which
/// keeps the one row from before the table changed.
#[test]
fn a_hover_is_laid_out_again_when_the_pictures_change() {
    let mut hover = Hover::new(
        Hovered {
            markdown: "aaaaaaaaa\u{2764}".to_string(),
            range: None,
        },
        (LineNumber::new(0), CharColumn::new(0)),
        false,
    );
    hover.settle(10, 10);
    let before = hover.rows().len();
    obelus_text::draw_as_pictures(&['\u{2764}']);
    hover.settle(10, 10);
    assert!(
        hover.rows().len() > before,
        "the hover kept the {before} rows it was laid out in before"
    );
}
