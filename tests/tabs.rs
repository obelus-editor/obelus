//! How wide a tab is, which is two things that have to agree.
//!
//! Their own binary because the width is one thing for the whole process --
//! a drawing decision the program shares, the way the glyph switch is --
//! and a test beside them that set a config would move it underneath.

mod support;

use crossterm::event::KeyCode;
use obelus::{app::App, buffer::Buffer, coordinates::LineNumber, text::Text};

fn reading(name: &str, contents: &str, width: usize) -> (support::Scratch, App) {
    let scratch = support::Scratch::new(name);
    let path = scratch.path().join("sample.rs");
    std::fs::write(&path, contents).expect("writing the file");
    let mut app = App::new(vec![Buffer::open(&path).expect("opening it")]);
    app.working_directory_for_test(scratch.path().to_path_buf());
    app.configure(
        obelus::config::Config {
            tab_width: width,
            ..obelus::config::Config::default()
        },
        Vec::new(),
    );
    support::lay_out(&mut app, 70, 12);
    (scratch, app)
}

/// The tabs tests share one global, so they take turns.
static TABS: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn the_tab_key_puts_in_as_many_spaces_as_the_setting_says() {
    let _turn = TABS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
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
    obelus::text::lay_tabs_at(obelus::text::TAB_WIDTH);
}

/// And a tab already in the file is drawn at the same width, or what the
/// key puts in does not line up with what is there.
#[test]
fn a_tab_in_the_file_is_laid_out_at_the_same_width() {
    let _turn = TABS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let text = Text::from_string("\tx\n");

    obelus::text::lay_tabs_at(2);
    assert_eq!(text.line_display_width(LineNumber::new(0)).get(), 3);
    obelus::text::lay_tabs_at(8);
    assert_eq!(
        text.line_display_width(LineNumber::new(0)).get(),
        9,
        "a tab in the file is still drawn at the old width"
    );
    obelus::text::lay_tabs_at(obelus::text::TAB_WIDTH);
}

/// A file that said nothing sensible should not produce a tab nobody
/// can step over.
#[test]
fn a_width_of_nothing_is_not_taken() {
    let _turn = TABS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    obelus::text::lay_tabs_at(0);
    assert!(obelus::text::tab_width() >= 1);
    obelus::text::lay_tabs_at(obelus::text::TAB_WIDTH);
}
