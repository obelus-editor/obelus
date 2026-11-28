//! How wide a tab is, which is two things that have to agree.
//!
//! Not in the application's binary because the width is one thing for the
//! whole process --
//! a drawing decision the program shares, the way the glyph switch is --
//! and a test beside them that set a config would move it underneath.

use crossterm::event::KeyCode;
use obelus_app::app::App;
use obelus_buffer::Buffer;
use obelus_text::{Text, coordinates::LineNumber};

use crate::support;

fn reading(name: &str, contents: &str, width: usize) -> (support::Scratch, App) {
    let scratch = support::Scratch::new(name);
    let path = scratch.path().join("sample.rs");
    std::fs::write(&path, contents).expect("writing the file");
    let mut app = App::new(vec![Buffer::open(&path).expect("opening it")]);
    app.working_directory_for_test(scratch.path().to_path_buf());
    app.configure(
        obelus_config::Config {
            tab_width: width,
            ..obelus_config::Config::default()
        },
        Vec::new(),
    );
    support::lay_out(&mut app, 70, 12);
    (scratch, app)
}

#[test]
fn the_tab_key_puts_in_as_many_spaces_as_the_setting_says() {
    let _turn = crate::turn();
    let (_scratch, mut app) = reading("tabs-typed", "x\n", 2);
    support::press(&mut app, KeyCode::Tab);
    assert_eq!(
        app.current_buffer()
            .expect("a buffer")
            .text()
            .rope()
            .to_string(),
        "  x\n"
    );

    let (_scratch, mut app) = reading("tabs-typed-8", "x\n", 8);
    support::press(&mut app, KeyCode::Tab);
    assert_eq!(
        app.current_buffer()
            .expect("a buffer")
            .text()
            .rope()
            .to_string(),
        "        x\n"
    );
    obelus_text::lay_tabs_at(obelus_text::TAB_WIDTH);
}

/// And a tab already in the file is drawn at the same width, or what the
/// key puts in does not line up with what is there.
#[test]
fn a_tab_in_the_file_is_laid_out_at_the_same_width() {
    let _turn = crate::turn();
    let text = Text::from_string("\tx\n");

    obelus_text::lay_tabs_at(2);
    assert_eq!(text.line_display_width(LineNumber::new(0)).get(), 3);
    obelus_text::lay_tabs_at(8);
    assert_eq!(
        text.line_display_width(LineNumber::new(0)).get(),
        9,
        "a tab in the file is still drawn at the old width"
    );
    obelus_text::lay_tabs_at(obelus_text::TAB_WIDTH);
}

/// A file that said nothing sensible should not produce a tab nobody
/// can step over.
#[test]
fn a_width_of_nothing_is_not_taken() {
    let _turn = crate::turn();
    obelus_text::lay_tabs_at(0);
    assert!(obelus_text::tab_width() >= 1);
    obelus_text::lay_tabs_at(obelus_text::TAB_WIDTH);
}

/// And a conversation already open is laid out again at the new width,
/// though nothing in it has moved.
///
/// The wrapping measures a tab by the setting, and the rows were kept by
/// the width of the screen, the pictures and the language alone -- so a
/// conversation open while the reader changed it went on wrapped at the old
/// width until something in it changed, and with each thing said kept on
/// its own, the things that did not change went on so for good.
///
/// Broken deliberately by leaving the tab out of `laid_against`.
#[test]
fn a_conversation_open_while_the_width_changes_is_laid_out_at_the_new_one() {
    let _turn = crate::turn();
    let mut chat = obelus_component::chat::Chat::new();
    // Seven cells at a tab of two, twenty-five at a tab of eight: one row
    // in twelve columns, and then three.
    chat.note("a\tb\tc\td");

    obelus_text::lay_tabs_at(2);
    let narrow = chat.rows(12).len();
    obelus_text::lay_tabs_at(8);
    let wide = chat.rows(12).len();
    obelus_text::lay_tabs_at(obelus_text::TAB_WIDTH);
    assert!(
        wide > narrow,
        "the tabs went wider and the rows stayed as they were: {narrow} then {wide}"
    );
}
