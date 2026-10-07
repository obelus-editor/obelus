//! The characters a window draws as pictures though they are written as
//! text, and how wide that makes them.
//!
//! A binary of its own, because the table is the whole process's: a test
//! beside the others that set it would move every width they measure.

use obelus_text::{
    Text,
    coordinates::{CharColumn, DisplayColumn, LineNumber},
};

/// A heart the window says it draws as a picture is two cells wherever it
/// is counted, and one where the text asks for text; nothing else moves.
///
/// One test rather than several, because they would share the table and
/// cargo runs a binary's tests at once.
///
/// Deliberate breaks: taking the table's arm out of `cells_of` leaves the
/// heart one cell after the window has said otherwise; and letting
/// `drawn_as_a_picture` answer for ASCII makes the `#` two.
#[test]
fn a_character_the_window_draws_as_a_picture_is_two_cells() {
    let heart = '\u{2764}';
    assert!(obelus_text::could_be_a_picture(heart));
    assert!(!obelus_text::could_be_a_picture('#'), "a keycap's #");
    assert!(!obelus_text::could_be_a_picture('a'));
    assert!(
        !obelus_text::could_be_a_picture('\u{8bfb}'),
        "a Chinese character"
    );

    // Until a window says so, a heart is the one cell a terminal gives it.
    assert_eq!(obelus_text::text_width("a\u{2764}b"), 3);

    let before = obelus_text::pictures_version();
    obelus_text::draw_as_pictures(&[heart, '#']);
    assert_ne!(
        obelus_text::pictures_version(),
        before,
        "nothing laid out again"
    );

    assert_eq!(obelus_text::text_width("a\u{2764}b"), 4);
    let text = Text::from_string("a\u{2764}b");
    let line = LineNumber::new(0);
    assert_eq!(
        text.display_column(line, CharColumn::new(2)),
        DisplayColumn::new(3),
        "the b is not after a heart two cells wide"
    );
    // Asked for as a picture it was two already, and asked for as text it
    // is one whatever the window does.
    assert_eq!(obelus_text::text_width("\u{2764}\u{fe0f}"), 2);
    assert_eq!(obelus_text::text_width("\u{2764}\u{fe0e}"), 1);
    // And ASCII is never a picture, whatever the window says.
    assert_eq!(obelus_text::text_width("#"), 1);

    obelus_text::draw_as_pictures(&[]);
    assert_eq!(
        obelus_text::text_width("a\u{2764}b"),
        3,
        "the table was not cleared"
    );
}
