//! What a block's rows are counted against, when a window changes which
//! characters it draws as pictures.
//!
//! A binary of its own, because the table is the whole process's.

use std::path::Path;

use obelus_buffer::Buffer;
use obelus_text::coordinates::LineNumber;

/// An opened block counts its rows again once the window draws a heart as
/// a picture, two cells wide: ten cells of `a`s and a heart are one row of
/// ten until then, and two rows after.
///
/// Deliberate break: keying the count on the width alone, which hands back
/// the one row it counted before the table changed.
#[test]
fn a_block_counts_its_rows_again_when_the_pictures_change() {
    let mut buffer = Buffer::from_text(Path::new("hearts.txt"), "one\ntwo\n");
    buffer.open_block(LineNumber::new(1), &["aaaaaaaaa\u{2764}".to_string()]);
    let rows = |buffer: &Buffer| {
        buffer
            .block_above(LineNumber::new(1))
            .expect("the block is open")
            .rows(10)
    };
    let before = rows(&buffer);
    obelus_text::draw_as_pictures(&['\u{2764}']);
    let after = rows(&buffer);
    assert_eq!(
        after,
        before + 1,
        "the block kept the rows it counted before"
    );
}
