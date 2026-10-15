//! Whether a window's title bar holds the tabs, which the window is told.
//!
//! Here rather than in the application's binary because the setting is
//! told by applying the settings, and applying them lays the glyph switch
//! and the tab width for the whole process.

use std::sync::{Arc, Mutex};

use obelus_app::app::App;

use crate::support;

/// What a window is told about the bar, each time it is told.
#[derive(Debug, Default)]
struct Told(Mutex<Vec<bool>>);

impl obelus_app::app::Drawing for Told {
    fn bar_tabs(&self, on: bool) {
        self.0.lock().expect("what was said").push(on);
    }

    fn text_size(&self, _: usize) {}

    fn animates(&self, _: bool) {}

    fn use_fonts(&self, _: &[String]) {}

    fn drawn_on(&self, _: ratatui::style::Color) {}

    fn holding(&self, _: ratatui::style::Color, _: ratatui::style::Color) {}

    fn hang(&self, _: Option<&obelus_theme::Wallpaper>) {}

    fn caret_is(
        &self,
        _: obelus_app::app::Caret,
        _: Option<obelus_component::layers::Layer>,
        _: bool,
    ) {
    }
}

/// The window is told whether its bar holds the tabs when it says who it
/// is, and again when the setting changes.
///
/// Deliberate breaks: leave `bar_tabs` out of `App::drawn_by`, and a window
/// started with the setting on keeps the system's bar until the settings
/// next change; out of `apply_config`, and turning it on does nothing until
/// the next start.
#[test]
fn the_window_is_told_whether_its_bar_holds_the_tabs() {
    let _turn = crate::turn();
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    let told = Arc::new(Told::default());
    app.drawn_by(told.clone());
    assert_eq!(told.0.lock().expect("said").as_slice(), [false]);

    app.configure(
        obelus_config::Config {
            bar_tabs: true,
            ..obelus_config::Config::default()
        },
        Vec::new(),
    );
    assert_eq!(told.0.lock().expect("said").last(), Some(&true));
}
