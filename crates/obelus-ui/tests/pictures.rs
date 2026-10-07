//! What the table of characters a window draws as pictures does to what
//! Obelus did not lay out itself.
//!
//! A binary of its own, because the table is the whole process's.

use obelus_ui::terminal::as_given;

/// A heart a program wrote in one column stays one column on the window,
/// though the window draws a heart written as text two cells wide.
///
/// Deliberate break: handing the cell's contents over as they came, which
/// measures two and is drawn over the character after it.
#[test]
fn a_program_s_heart_is_as_wide_as_the_program_was_given_it() {
    obelus_text::draw_as_pictures(&['\u{2764}']);
    let heart = as_given("\u{2764}", false);
    assert_eq!(obelus_text::text_width(&heart), 1, "{heart:?}");

    // One the program was given two columns for is left as it is, and so
    // is everything that is not a picture.
    assert_eq!(as_given("\u{2764}", true), "\u{2764}");
    assert_eq!(as_given("x", false), "x");
    assert_eq!(as_given("\u{4f60}", true), "\u{4f60}");
}
