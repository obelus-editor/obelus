//! What Obelus says in a language other than English.
//!
//! Not in the application's binary because the language is the process's:
//! applying a setting that names one would speak it to whichever test
//! beside these was drawing. Each test here starts from `App::new`, which
//! applies the defaults -- `auto`, and a test never asks the system, so
//! English -- and goes back to them before it lets go of its turn.

use obelus_app::{
    app::{App, dispatch},
    event::Event,
};
use obelus_command::Command;

use crate::support;

/// Settings that say which language, and nothing else.
fn speaking(language: &str) -> obelus_config::Config {
    obelus_config::Config {
        language: language.to_string(),
        ..obelus_config::Config::default()
    }
}

/// The settings page, drawn.
fn settings_page(app: &mut App) -> String {
    support::lay_out(app, 80, 16);
    dispatch::dispatch(app, Command::ConfigOpen);
    support::text_block(&support::render(app, 80, 16)).to_string()
}

/// Broken deliberately by taking `obelus_i18n::speak` out of `apply_config`:
/// the tabs stayed in English with Chinese chosen.
#[test]
fn the_settings_tabs_are_in_the_language_chosen() {
    let _turn = crate::turn();
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.configure(speaking("zh-CN"), vec!["language"]);
    let page = settings_page(&mut app);
    assert!(
        page.contains("外观") && !page.contains("Appearance"),
        "the tabs are not in Chinese:\n{page}"
    );

    // And back, the way any setting goes back.
    app.configure(speaking("en"), vec!["language"]);
    let page = settings_page(&mut app);
    assert!(
        page.contains("Appearance"),
        "the tabs did not come back:\n{page}"
    );
}

/// A language is offered by its own name, and the system's in the language
/// being spoken.
///
/// Broken deliberately by taking the `language` arm out of `App::called`:
/// the row said `zh-CN` and `auto`, the file's words.
#[test]
fn a_language_is_called_by_its_own_name() {
    let _turn = crate::turn();
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    assert_eq!(
        app.called("language", "auto").as_deref(),
        Some("The system's")
    );
    assert_eq!(app.called("language", "zh-CN").as_deref(), Some("简体中文"));
    app.configure(speaking("zh-CN"), vec!["language"]);
    assert_eq!(app.called("language", "auto").as_deref(), Some("跟随系统"));
    assert_eq!(app.called("language", "en").as_deref(), Some("English"));
    app.configure(speaking("en"), vec!["language"]);
}

/// The news in the plate's edge is measured in columns, so a sentence of
/// Chinese -- two columns a character -- still closes the frame where the
/// rows above it close.
///
/// Broken deliberately twice: by measuring the run set into the edge with
/// `chars().count()` in `welcome::foot`, and by stepping one column a
/// character where the plate is drawn. Either way the edge's corner stood
/// somewhere other than the one above it.
#[test]
fn the_news_in_chinese_fits_the_plate() {
    let _turn = crate::turn();
    let mut app = App::new(Vec::new());
    app.working_directory_for_test(std::path::PathBuf::from("/tmp/obelus"));
    app.configure(speaking("zh-CN"), vec!["language"]);
    app.handle(Event::Released("v99.0.0".to_string()));
    let dump = support::render(&mut app, 64, 20);
    let block = support::text_block(&dump);
    // Where a row's corner is, in columns: a Chinese character is one
    // `char` of the row and two of the screen.
    let corner = |corner: char| {
        let row = block
            .lines()
            .find(|row| row.contains(corner))
            .unwrap_or_default();
        let before = row.split(corner).next().unwrap_or_default();
        (row.to_string(), obelus_text::text_width(before))
    };
    let (top, top_at) = corner('\u{2557}');
    let (edge, edge_at) = corner('\u{255d}');
    assert!(
        edge.contains("v99.0.0 已发布"),
        "the news is not in Chinese: {edge:?}"
    );
    assert_eq!(
        top_at, edge_at,
        "the edge does not close where the top does:\n{top}\n{edge}"
    );
    app.configure(speaking("en"), vec!["language"]);
}
