//! How many colours a terminal is drawn in, which the settings say and the
//! terminal's front end acts on.
//!
//! Here rather than in the application's binary because the setting is
//! applied to the whole process -- what hands a frame to this terminal
//! reads it -- and a test beside them that set a config would move it
//! underneath.

use std::sync::{Arc, Mutex};

use obelus_app::app::App;
use obelus_theme::depth::{self, Depth};

use crate::support;

/// What a front end is told about the colours, each time it is told.
#[derive(Debug, Default)]
struct Told(Mutex<Vec<Option<Depth>>>);

impl obelus_app::app::Drawing for Told {
    fn colours(&self, depth: Option<Depth>) {
        self.0.lock().expect("what was said").push(depth);
    }

    fn text_size(&self, _: usize) {}

    fn animates(&self, _: bool) {}

    fn bar_tabs(&self, _: bool) {}

    fn use_fonts(&self, _: &[String]) {}

    fn drawn_on(&self, _: ratatui::style::Color) {}

    fn holding(&self, _: ratatui::style::Color, _: ratatui::style::Color) {}

    fn caret_in(&self, _: ratatui::style::Color) {}

    fn hang(&self, _: Option<&obelus_theme::Wallpaper>) {}

    fn caret_is(
        &self,
        _: obelus_app::app::Caret,
        _: Option<obelus_component::layers::Layer>,
        _: bool,
    ) {
    }
}

fn coloured(word: &str) -> obelus_config::Config {
    obelus_config::Config {
        colours: word.to_string(),
        ..obelus_config::Config::default()
    }
}

/// The setting is what this process's terminal draws at, and `auto` is
/// whatever the terminal says.
///
/// Deliberate break: `depth::ask` left out of `apply_config`. The first
/// assertion found nothing asked.
#[test]
fn the_terminal_here_draws_at_what_the_settings_say() {
    let _turn = crate::turn();
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.configure(coloured("256"), Vec::new());
    let palette = depth::asked();
    app.configure(coloured("truecolor"), Vec::new());
    let millions = depth::asked();
    app.configure(coloured("auto"), Vec::new());
    assert_eq!(palette, Some(Depth::Palette));
    assert_eq!(millions, Some(Depth::Millions));
    assert_eq!(depth::asked(), None);
}

/// A terminal drawing an Obelus on another machine is told the setting
/// when it says who it is, and again when the setting changes.
///
/// Deliberate breaks: `colours` left out of `App::drawn_by`, and the
/// terminal draws at its own guess until the settings next change; left out
/// of `apply_config`, and a change on the settings page reaches it only on
/// the next connection.
#[test]
fn a_terminal_elsewhere_is_told_what_the_settings_say() {
    let _turn = crate::turn();
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.configure(coloured("256"), Vec::new());
    let told = Arc::new(Told::default());
    app.drawn_by(told.clone());
    assert_eq!(
        told.0.lock().expect("said").as_slice(),
        [Some(Depth::Palette)]
    );

    app.configure(coloured("auto"), Vec::new());
    assert_eq!(told.0.lock().expect("said").last(), Some(&None));
}
