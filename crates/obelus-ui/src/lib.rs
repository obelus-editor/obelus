//! Drawing.
//!
//! Nothing in here reads a file, makes a syscall or parses anything. The draw
//! path runs inside `Terminal::draw`, which blocks the main loop on a write to
//! stdout; adding slow work to it is the mistake that actually happens, rather
//! than the write itself being slow.
//!
//! Everything that scrolls says so, in the last column of the region it is
//! in. A file, a preview, a list, a page of settings, a conversation -- the
//! last of those had no bar at all, which left a reader paging through it with
//! nothing on screen answering "how much of this is there, and which part am I
//! looking at". The editor's used to
//! sit one short of it, because the map of where a file has changed had the
//! edge: a list opened over a file made the bar jump sideways, and inside one
//! screen a list with a preview under it had its bar in two columns with a rule
//! between them.
//!
//! The map is *inside* the bar now rather than outside it. They are the same
//! picture at the same scale -- the whole file squeezed into the height of the
//! screen -- so they belong side by side, and the reader reads across them:
//! here is where you are, and here is what has changed.

/// What a colour a server found written down is drawn as.
///
/// A square rather than the whole cell filled in. A terminal cell is about
/// twice as tall as it is wide, so a filled one is an upright bar -- and a
/// bar beside a colour literal reads as a mark on the text rather than as
/// the colour itself. One cell wide, which is what the line was measured
/// with.
pub(crate) const SWATCH: char = '\u{25a0}';

/// How many cells it takes.
///
/// Measured from the glyph rather than written down beside it: the number
/// the line is laid out with and the thing drawn in it have to agree, and
/// two constants that must agree are one that can be changed alone.
#[must_use]
pub fn swatch_cells() -> usize {
    unicode_width::UnicodeWidthChar::width(SWATCH).unwrap_or(1)
}

/// What one cell a file does not contain is drawn as.
///
/// One list per document, because a cell points at one entry and cannot
/// say which of two lists it meant: the colours a server found written
/// down and the hints it worked out are drawn from the same one, in the
/// order the application put them there.
///
/// Here rather than beside either of them: the application builds it and
/// the editor draws it, and a type that lives with one of its two sources
/// would make the other one a guest.
#[derive(Clone, Debug, PartialEq)]
pub enum Drawn {
    /// A colour, as a cell of itself.
    Swatch(ratatui::style::Color),
    /// Something a server would have you read that the file does not say.
    Hint(obelus_lsp::hint::Hinted),
}

use std::path::Path;

use obelus_agent::{Listed, Talking, acp};
use obelus_buffer::{Buffer, TextArea};
use obelus_component::{
    card::Card, chat::Chat, completion::Completion, counts::Counts, hover::Hover, layers,
    prompt::Prompt, settings::Settings, todo::TodoView,
};
use obelus_keymap::Keymap;
use obelus_syntax::highlight::Highlights;
use obelus_text::coordinates::{LineNumber, Span};

use crate::{bars::Whose, image::Images};

/// A path with the reader's own directory written as `~`.
///
/// Here because it is about drawing and not about the filesystem:
/// everything under the home directory would otherwise spend a dozen
/// columns of every row on the same word, taken from the part of the path
/// that says which thing it is. One piece of code, so the welcome
/// screen's own line and the rows of projects under it cannot disagree
/// about how a path is written.
#[must_use]
pub fn with_home_as_tilde(path: &std::path::Path) -> String {
    let said = path.to_string_lossy();
    let Some(home) = std::env::home_dir() else {
        return said.into_owned();
    };
    let home = home.to_string_lossy();
    // Only a whole leading component, so `/home/sunlight` is not written
    // as `~light` for a reader whose directory is `/home/sun`.
    match said.strip_prefix(home.as_ref()) {
        Some("") => "~".to_string(),
        // Either separator: a path on Windows may hold `/` and still be
        // the reader's own directory with something under it.
        Some(rest) if rest.starts_with(std::path::is_separator) => format!("~{rest}"),
        _ => said.into_owned(),
    }
}

mod cells;
mod frame;
mod hints;
mod marked;
mod rules;
mod screen;
mod tabs;
pub use cells::*;
pub use frame::*;
pub use hints::*;
pub use marked::*;
pub use rules::*;
pub use screen::*;
pub use tabs::*;

pub mod bars;
pub mod card;
pub mod chat;
pub mod complete;
pub mod counts;
pub mod editor;
pub mod gone;
pub mod hover;
pub mod image;
pub mod links;
pub mod names;
pub mod picker;
pub mod projects;
pub mod reading;
pub mod settings;
pub mod shapes;
pub mod signature;
pub mod status;
pub mod terminal;
pub mod todo;
pub mod trouble;
pub mod welcome;

use std::ops::Range;

use obelus_component::{
    layers::Layer,
    picker::{Colouring, Picker},
};
use obelus_text::{text_width, widths};
use obelus_theme::Theme;
use ratatui::{
    buffer::Buffer as CellBuffer,
    layout::{Position, Rect, Size},
    style::{Color, Style},
    widgets::Widget as _,
};
use unicode_width::UnicodeWidthChar as _;

/// Held while a test changes, or reads, the one switch that says whether
/// glyphs are drawn.
///
/// The switch is a single atomic for the whole process and cargo runs a
/// crate's tests at once, so a test that flips it and a test that reads it
/// are two tests sharing a variable. The reader saw a flip land between
/// its two looks and compared a badge drawn with glyphs against the mark
/// for a terminal without them -- a failure that appeared about one run in
/// three and only under a full workspace build, which is the shape of
/// thing that gets rerun rather than read.
///
/// A lock and not a rule about which tests may touch it: the two are in
/// different modules and nothing would have stopped a third.
#[cfg(test)]
pub(crate) fn glyphs_held() -> std::sync::MutexGuard<'static, ()> {
    static GLYPHS: std::sync::Mutex<()> = std::sync::Mutex::new(());
    // A test that panicked while holding it has poisoned it and has
    // already failed; the next one wants the lock, not a second failure
    // about the first.
    GLYPHS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::{
        bar_mark, drop_from_left, drop_from_right, tick, truncate_from_left, truncate_from_right,
    };

    /// A bar's mark reaches the end of its track exactly when the last
    /// row does, and never steps back on the way there.
    ///
    /// Deliberate break: scale by the whole list rather than by how far
    /// its top can go -- `total` in place of `furthest` in `bar_reach`.
    /// The mark then stops short of the bottom on the one screenful
    /// anybody checks it against.
    ///
    /// Not checked against what `scrollbar` draws, because `scrollbar`
    /// works it out with this: one function, so there is nothing for the
    /// two of them to disagree about, and a test that compared them
    /// would be asking the rule what it expects.
    #[test]
    fn a_bar_reaches_the_end_of_its_track_when_the_last_row_does() {
        // Ten rows of a forty-row list: a two-row thumb with eight rows
        // of track to travel, over a top that can go thirty.
        assert_eq!(bar_mark(10, 0, 40), 0);
        assert_eq!(bar_mark(10, 30, 40), 8, "the last screenful is the end");
        // And never back, which is the one thing a mark must not do.
        let mut last = 0;
        for top in 0..=30 {
            let mark = bar_mark(10, top, 40);
            assert!(mark >= last, "the mark stepped back at {top}");
            last = mark;
        }
        assert_eq!(bar_mark(10, 0, 10), 0, "a list that fits");
    }

    /// A control character in what is drawn does not take Obelus down.
    ///
    /// A tab in a tool call's output, a stray escape in what an agent said,
    /// a `\r` in a file somebody opened: Obelus put each of them straight
    /// into a cell, and the frame died -- not where it was written, but
    /// later, when what puts a frame on the screen walked the cells asking
    /// each how wide it was. A control character has no answer to that, and
    /// the whole of Obelus went with the question.
    ///
    /// Which is why this draws a real frame. Every golden test in Obelus
    /// reads the cells straight out of the buffer, and the buffer was
    /// perfectly happy: nothing between writing a cell and a terminal
    /// receiving it ever asked.
    ///
    /// Broken deliberately by putting the character in the cell as it
    /// stands, which is what it did.
    #[test]
    fn a_control_character_does_not_take_the_frame_down() {
        use ratatui::{
            Terminal, backend::TestBackend, buffer::Buffer as CellBuffer, layout::Rect,
            style::Style, widgets::Widget,
        };

        struct Odd;

        impl Widget for Odd {
            fn render(self, _area: Rect, cells: &mut CellBuffer) {
                let mut column = 0;
                for character in "a\tb\u{1b}c\rd".chars() {
                    column += super::put(cells, column, 0, character, Style::new());
                }
            }
        }

        let mut terminal = Terminal::new(TestBackend::new(20, 2)).expect("a terminal");
        terminal
            .draw(|frame| frame.render_widget(Odd, frame.area()))
            .expect("the frame went down on a control character");
        // And what is there is the words, with a cell each where the
        // control characters were rather than a hole or a shunted row.
        let drawn: String = (0..7)
            .map(|x| terminal.backend().buffer()[(x, 0)].symbol().to_string())
            .collect();
        assert_eq!(drawn, "a b c d", "the words were not drawn around them");
    }

    /// A box says which way it is set by being a different box.
    ///
    /// The whole of what replaced a slider: a slider said it by where its
    /// knob sat, which is a distance to measure, and this is a glyph to
    /// recognise. So the one thing that must be true of the pair is that
    /// they are not the same glyph -- in a terminal with the font and in
    /// one without, because both are drawn.
    ///
    /// Broken deliberately by giving either pair the same character twice:
    /// every switch Obelus draws goes quiet about its state, and only this
    /// says so -- the views' own tests flip a switch and read the box, and
    /// the settings' one flips the glyphs themselves and so reads one of
    /// each pair.
    #[test]
    fn a_box_is_not_the_same_box_when_it_is_marked() {
        let _held = super::glyphs_held();
        for glyphs in [true, false] {
            obelus_icons::use_glyphs(glyphs);
            assert_ne!(
                tick(true),
                tick(false),
                "the box reads the same either way, with glyphs {glyphs}"
            );
        }
        obelus_icons::use_glyphs(false);
    }

    #[test]
    fn a_path_that_fits_is_left_alone() {
        assert_eq!(drop_from_left("src/app.rs", 20), 0);
        assert_eq!(truncate_from_left("src/app.rs", 20), "src/app.rs");
    }

    /// The file name survives; the directories above it are what goes.
    #[test]
    fn the_end_survives_the_cut() {
        let truncated = truncate_from_left("a/very/deep/path/to/app.rs", 12);
        assert_eq!(truncated, "\u{2026}h/to/app.rs");
        assert_eq!(truncated.chars().count(), 12);
    }

    /// Cells, not characters: a wide glyph costs two, and counting characters
    /// would overrun whatever is drawn after the text.
    #[test]
    fn wide_glyphs_are_counted_by_the_cells_they_take() {
        // Five wide glyphs are ten cells. Six cells hold the ellipsis and two
        // glyphs; a third would need a seventh cell.
        let glyphs = "\u{4f60}\u{597d}\u{4e16}\u{754c}\u{554a}";
        assert_eq!(truncate_from_left(glyphs, 6), "\u{2026}\u{754c}\u{554a}");
        assert_eq!(super::text_width(&truncate_from_left(glyphs, 6)), 5);
    }

    /// The property the rest of the layout depends on: whatever comes back
    /// fits. Anything wider would be drawn over the row's other columns.
    #[test]
    fn the_result_never_exceeds_the_room_it_was_given() {
        let samples = [
            "src/app.rs",
            "a/very/deep/path/to/somewhere/app.rs",
            "\u{4f60}\u{597d}\u{4e16}\u{754c}\u{554a}/mixed/\u{8def}\u{5f84}.rs",
            "\tindented",
            "",
        ];
        for contents in samples {
            for cells in 0..40usize {
                let width = super::text_width(&truncate_from_left(contents, cells));
                assert!(
                    width <= cells || width == 0,
                    "{contents:?} at {cells} cells came back {width} wide"
                );
            }
        }
    }

    /// No room even for the ellipsis, so the caller can draw nothing rather
    /// than a lone `\u{2026}` that says only that something was hidden.
    #[test]
    fn nothing_fits_in_one_cell() {
        assert_eq!(drop_from_left("src/app.rs", 1), 10);
        assert_eq!(truncate_from_left("src/app.rs", 1), "");
        assert_eq!(drop_from_right("src/app.rs", 1), 10);
    }

    /// A sentence keeps its beginning, which is the half that says which
    /// sentence it is.
    #[test]
    fn a_sentence_that_fits_is_left_alone() {
        assert_eq!(drop_from_right("Let a page reach the end", 30), 0);
    }

    /// One cell of what fits goes to the mark, the way it does from the left.
    #[test]
    fn the_beginning_survives_the_cut() {
        let subject = "Stop and ask, instead of counting presses";
        let dropped = drop_from_right(subject, 12);
        let kept: String = subject
            .chars()
            .take(subject.chars().count() - dropped)
            .collect();
        assert_eq!(kept, "Stop and as");
        assert_eq!(super::text_width(&kept) + 1, 12);
    }

    /// Cells here too, and it matters more: a subject written in Chinese is
    /// one character to two columns, so counting characters would cut it at
    /// half the row it was given.
    #[test]
    fn a_wide_sentence_is_counted_by_the_cells_it_takes() {
        let subject = "\u{4fee}\u{590d}\u{4e00}\u{4e2a}\u{95ee}\u{9898}";
        let dropped = drop_from_right(subject, 7);
        let kept: String = subject
            .chars()
            .take(subject.chars().count() - dropped)
            .collect();
        // Six cells for three glyphs, and the seventh for the mark.
        assert_eq!(kept, "\u{4fee}\u{590d}\u{4e00}");
        assert_eq!(super::text_width(&kept), 6);
    }

    /// The string form, and the one cell the mark takes.
    #[test]
    fn a_sentence_comes_back_with_its_tail_marked() {
        assert_eq!(truncate_from_right("Stop and ask", 30), "Stop and ask");
        assert_eq!(
            truncate_from_right("Stop and ask, instead of counting", 12),
            "Stop and as\u{2026}"
        );
        assert_eq!(
            super::text_width(&truncate_from_right("Stop and ask, instead", 12)),
            12
        );
    }

    /// Nothing rather than a lone `\u{2026}`, which is what the other
    /// direction does and says only that something was hidden. The two
    /// helpers this replaced both drew the mark alone here.
    #[test]
    fn no_room_for_the_mark_means_no_mark() {
        assert_eq!(truncate_from_right("Stop and ask", 1), "");
        assert_eq!(truncate_from_right("Stop and ask", 0), "");
    }

    /// The same property the other direction has to hold: what is kept, plus
    /// the cell the mark takes, fits in the room it was given.
    #[test]
    fn what_is_kept_from_the_left_never_exceeds_the_room() {
        let samples = [
            "Stop and ask, instead of counting presses",
            "\u{4fee}\u{590d}\u{4e00}\u{4e2a}\u{95ee}\u{9898}",
            "mixed \u{4e2d}\u{6587} and latin",
            "\tindented",
            "",
        ];
        for contents in samples {
            let total = contents.chars().count();
            for cells in 0..40usize {
                let dropped = drop_from_right(contents, cells);
                if dropped >= total {
                    continue;
                }
                let kept: String = contents.chars().take(total - dropped).collect();
                let width = super::text_width(&kept) + usize::from(dropped > 0);
                assert!(
                    width <= cells,
                    "{contents:?} at {cells} cells kept {width} cells' worth"
                );
                let written = super::text_width(&truncate_from_right(contents, cells));
                assert!(
                    written <= cells,
                    "{contents:?} at {cells} cells came back {written} wide"
                );
            }
        }
    }

    /// A character asked to be drawn as a picture is written with its
    /// selector in one cell, two wide, and a row cut to fit counts it as two.
    ///
    /// Deliberate breaks: writing with `put` alone puts the selector in a
    /// cell of its own, so `x` lands a cell further on; and measuring the cut
    /// with each character's own width counts the heart as one, keeps the
    /// `b` as well, and the row is a cell wider than it was given.
    #[test]
    fn a_picture_is_one_cell_of_two_with_its_selector() {
        use ratatui::{buffer::Buffer as CellBuffer, layout::Rect, style::Style};

        let mut cells = CellBuffer::empty(Rect::new(0, 0, 6, 1));
        let after = super::write(&mut cells, 0, 0, "\u{2764}\u{fe0f}x", Style::new());
        assert_eq!(after, 3, "the heart and the x are three cells");
        assert_eq!(cells[(0, 0)].symbol(), "\u{2764}\u{fe0f}");
        assert_eq!(cells[(1, 0)].symbol(), "");
        assert_eq!(cells[(2, 0)].symbol(), "x");

        // Three cells for `ab❤️`, which is four: one for the ellipsis and
        // two for the heart, so the `b` goes.
        assert_eq!(drop_from_left("ab\u{2764}\u{fe0f}", 3), 2);
        assert_eq!(
            truncate_from_left("ab\u{2764}\u{fe0f}", 3),
            "\u{2026}\u{2764}\u{fe0f}"
        );
        // And room for the ellipsis alone takes the selector with its heart
        // rather than leaving it to say how nothing is drawn.
        assert_eq!(drop_from_left("ab\u{2764}\u{fe0f}", 2), 4);
        assert_eq!(drop_from_right("\u{2764}\u{fe0f}ab", 3), 2);
    }

    /// A family joined by U+200D is written in one cell two wide, counted
    /// as two by everything that finds a cell, and cut whole.
    ///
    /// Deliberate breaks: writing a character at a time puts each person and
    /// each joiner in a cell of its own and the `x` four cells on; and
    /// cutting from the left by characters rather than clusters leaves the
    /// family's joiners and its last two people behind the ellipsis.
    #[test]
    fn a_family_is_one_cell_of_two() {
        use ratatui::{buffer::Buffer as CellBuffer, layout::Rect, style::Style};

        let family = "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}";
        let said = format!("{family}x");
        let mut cells = CellBuffer::empty(Rect::new(0, 0, 8, 1));
        let after = super::write(&mut cells, 0, 0, &said, Style::new());
        assert_eq!(after, 3, "the family and the x are three cells");
        assert_eq!(cells[(0, 0)].symbol(), family);
        assert_eq!(cells[(1, 0)].symbol(), "");
        assert_eq!(cells[(2, 0)].symbol(), "x");

        let counted: Vec<u16> = obelus_text::drawn_widths(&said)
            .map(|(_, cells)| cells)
            .collect();
        assert_eq!(counted, [2, 0, 0, 0, 0, 1]);

        // Room for the ellipsis and the `x` takes the whole family.
        assert_eq!(drop_from_left(&said, 2), 5);
        assert_eq!(drop_from_left(&format!("ab{family}"), 3), 2);
        assert_eq!(drop_from_right(&format!("{family}ab"), 3), 2);
    }

    /// What finds a character from a cell counts the cells `write` drew:
    /// a picture two and its selector none.
    ///
    /// Which is what a click on a conversation's row and the caret put back
    /// on one are found by; they counted the selector as a cell of its own,
    /// so a click on the heart's second cell landed between it and its
    /// selector.
    ///
    /// Deliberate break: counting a selector after a character as the one
    /// cell a zero-width character gets, which is what the conversation did.
    #[test]
    fn a_cell_is_found_by_the_widths_write_draws() {
        use obelus_text::drawn_widths;
        use ratatui::{buffer::Buffer as CellBuffer, layout::Rect, style::Style};

        use super::write;

        let said = "\u{2764}\u{fe0f}x";
        let counted: Vec<u16> = drawn_widths(said).map(|(_, cells)| cells).collect();
        assert_eq!(counted, [2, 0, 1]);
        let mut cells = CellBuffer::empty(Rect::new(0, 0, 6, 1));
        assert_eq!(
            write(&mut cells, 0, 0, said, Style::new()),
            counted.iter().sum::<u16>(),
            "what is counted is not what is drawn"
        );
        // A selector with nothing before it has no cell to go in, and is
        // given the one `put` gives it.
        let counted: Vec<u16> = drawn_widths("\u{fe0f}x").map(|(_, cells)| cells).collect();
        assert_eq!(counted, [1, 1]);
    }
}
