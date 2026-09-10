//! The settings view: tabs, a filter, and a control per row.

mod support;

use crossterm::event::KeyCode;
use obelus::{
    app::App,
    command::{Command, dispatch},
    config,
    event::Event,
};

/// Applying a setting touches process-wide state -- the glyph switch is one
/// switch the drawing code can read without a flag threaded into every
/// function -- so these tests take turns. Tests in one file share a process,
/// and every test here applies a setting.
static SETTINGS: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// A directory of its own for one test.
fn temporary(name: &str) -> std::path::PathBuf {
    let directory =
        std::env::temp_dir().join(format!("obelus-settings-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).expect("a directory");
    directory.join("config.toml")
}

fn open(file: &std::path::Path) -> App {
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.config_file_for_test(file.to_path_buf());
    support::lay_out(&mut app, 66, 12);
    dispatch::dispatch(&mut app, Command::ConfigOpen);
    app
}

/// A switch changes the setting, the running program, and the file -- in
/// that order and all at once. A setting that took effect on restart would
/// be a setting nobody can tell they have changed.
#[test]
fn a_switch_takes_effect_and_is_written_down() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let file = temporary("switch");
    let mut app = open(&file);

    // Down onto the second row of the appearance tab, which is the glyphs.
    support::press(&mut app, KeyCode::Down);
    assert!(obelus::icons::enabled(), "the glyphs start on");
    support::press(&mut app, KeyCode::Enter);

    assert!(!obelus::icons::enabled(), "the glyphs are still on");
    assert!(!app.config().icons, "the setting did not change");
    let written = std::fs::read_to_string(&file).expect("the file was written");
    assert!(
        !config::from_toml(&written).icons,
        "the file does not say so: {written:?}"
    );

    // And again the other way.
    support::press(&mut app, KeyCode::Enter);
    assert!(obelus::icons::enabled(), "enter did not toggle it back");

    // Space is a character, not a second way to flip it: these pages
    // filter by typing, and "Agent 1" is a name a reader will type.
    support::press(&mut app, KeyCode::Char(' '));
    assert!(obelus::icons::enabled(), "space flipped the switch");
    assert_eq!(app.settings().expect("the settings").query(), " ");

    // Put the glyphs back for whatever runs next: this is process-wide
    // state, which is the price of a switch the drawing code can read.
    obelus::icons::use_glyphs(true);
}

/// A setting with choices opens the ordinary compact list over the view:
/// the same list the symbol menu is, so it filters by typing and scrolls
/// and marks its selection the way every other list does.
#[test]
fn a_choice_opens_the_list_every_other_choice_uses() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let file = temporary("droplist");
    let mut app = open(&file);

    // The theme is the first row.
    support::press(&mut app, KeyCode::Enter);
    let picker = app.picker().expect("the choices");
    assert_eq!(
        picker
            .matches()
            .map(|item| item.label.clone())
            .collect::<Vec<_>>(),
        ["dark", "light"],
        "not the theme's choices"
    );
    // Opened on the one in force, so the list starts by saying which.
    assert_eq!(
        picker.selected_item().map(|item| item.label.clone()),
        Some("dark".to_string())
    );
    // And the settings are still there, underneath.
    assert!(app.settings().is_some(), "the view went away");
    let dump = support::render(&mut app, 66, 12);
    assert!(
        support::text_block(&dump).contains("appearance"),
        "the settings are not behind the list:\n{dump}"
    );

    // It filters by typing, which is the whole reason it is this list.
    support::type_text(&mut app, "li");
    let picker = app.picker().expect("the choices");
    assert_eq!(picker.match_count(), 1, "the query narrowed nothing");
    support::press(&mut app, KeyCode::Enter);

    assert!(app.picker().is_none(), "the list stayed open");
    assert_eq!(app.theme().name, "light", "the theme did not change");
    assert_eq!(
        config::from_toml(&std::fs::read_to_string(&file).expect("the file")).theme,
        "light"
    );

    // Escape closes the list and leaves the setting alone -- and leaves the
    // settings view open, because that is what is behind it.
    support::press(&mut app, KeyCode::Enter);
    support::press(&mut app, KeyCode::Esc);
    assert!(app.picker().is_none(), "escape left the list open");
    assert_eq!(app.theme().name, "light", "escape chose something");
    assert!(app.settings().is_some(), "escape closed the view as well");
}

/// A switch is a slider, and enter flips it: the knob moves to the other
/// end. The arrows are not it -- they walk the tabs, as they do in every
/// other view with tabs on it.
#[test]
fn a_switch_is_a_slider_that_enter_flips() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let file = temporary("slider");
    let mut app = open(&file);
    support::press(&mut app, KeyCode::Down);

    // Where the knob is drawn on the row.
    let knob = |app: &mut App| {
        let dump = support::render(app, 66, 12);
        support::text_block(&dump)
            .lines()
            .find(|row| row.contains("Nerd Font"))
            .and_then(|row| row.find('\u{25a0}'))
            .expect("the knob")
    };

    assert!(app.config().icons, "the glyphs start on");
    let on = knob(&mut app);
    support::press(&mut app, KeyCode::Enter);
    assert!(!app.config().icons, "enter did not flip it");
    let off = knob(&mut app);
    assert!(off < on, "the knob did not move: {off} then {on}");

    // The arrows do not touch it: they are the tabs'.
    support::press(&mut app, KeyCode::Right);
    assert!(!app.config().icons, "an arrow flipped the switch");
    support::press(&mut app, KeyCode::Left);
    assert!(!app.config().icons, "an arrow flipped the switch");

    obelus::icons::use_glyphs(true);
}

/// The tabs are the groups, the arrows walk them -- only the arrows, the way
/// they do in every other view with tabs on it -- and the focus comes back
/// inside the rows the new tab has.
#[test]
fn the_tabs_are_the_groups() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let file = temporary("tabs");
    let mut app = open(&file);

    let rows = |app: &App| {
        app.settings()
            .expect("the settings")
            .rows()
            .iter()
            .map(|setting| setting.label.to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        rows(&app),
        [
            "Colour theme",
            "Nerd Font glyphs in lists and on the status bar"
        ]
    );

    // Onto the last row, then to the next tab: the focus cannot stay on a
    // row the new tab does not have.
    support::press(&mut app, KeyCode::Down);
    assert_eq!(app.settings().expect("the settings").focus(), 1);
    support::press(&mut app, KeyCode::Right);
    assert_eq!(
        rows(&app),
        [
            "Wrap a line too long for the screen onto the next row",
            "Who last changed the line the cursor is on"
        ]
    );
    assert!(
        app.settings().expect("the settings").focus() < 2,
        "the focus is on a row this tab does not have"
    );

    // Left goes back, and once more reaches the agents -- which is a tab
    // and not a group of settings, so it has no rows of this kind at all.
    support::press(&mut app, KeyCode::Left);
    assert_eq!(
        rows(&app),
        [
            "Colour theme",
            "Nerd Font glyphs in lists and on the status bar"
        ]
    );
    support::press(&mut app, KeyCode::Left);
    assert!(
        app.settings().expect("the settings").on_agents(),
        "the left arrow did not reach the agents"
    );
    assert!(
        rows(&app).is_empty(),
        "the agents page has settings rows on it"
    );

    // Tab is not one of the keys that walks them: one way to do it is the
    // way every other tabbed view here works.
    support::press(&mut app, KeyCode::Tab);
    assert!(app.settings().expect("the settings").on_agents());
}

/// Typing narrows the rows, and the count on the status bar says how many
/// are left. Plainly by substring, because a reader typing "the" means the
/// word.
#[test]
fn typing_narrows_the_settings() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let file = temporary("filter");
    let mut app = open(&file);

    support::type_text(&mut app, "theme");
    let dump = support::render(&mut app, 66, 12);
    assert_eq!(
        app.settings().expect("the settings").rows().len(),
        1,
        "the query narrowed nothing:\n{dump}"
    );
    assert!(
        support::text_block(&dump).contains("Colour theme"),
        "not the row that matched:\n{dump}"
    );
    assert!(
        !support::text_block(&dump).contains("Nerd Font"),
        "a row that did not match is still there:\n{dump}"
    );

    // And the characters that matched carry the background every other list
    // marks a match with: a row in a narrowed list has to say why it is in
    // it. Only those characters -- the rest of the row is not a match.
    let styles: Vec<&str> = support::style_block(&dump)
        .lines()
        .filter(|row| !row.is_empty())
        .collect();
    let row = support::text_block(&dump)
        .lines()
        .filter(|row| !row.is_empty())
        .position(|row| row.contains("Colour theme"))
        .expect("the row that matched");
    // Cell one is the "C" of "Colour theme" -- cell zero is the row's own
    // left-hand padding -- and cell twelve is its last character.
    let letters: Vec<char> = styles[row].chars().skip(3 + 1).take(12).collect();
    let marked: std::collections::HashSet<char> = letters.iter().copied().collect();
    assert!(
        marked.len() > 1,
        "the whole name is one colour, so nothing was marked:\n{dump}"
    );
    // "theme" is the last five characters of the name, so the run is at its
    // end and its first character is not in it.
    assert_ne!(
        letters[0], letters[11],
        "the marked run covers the whole name:\n{dump}"
    );
    assert_eq!(
        letters[7], letters[11],
        "the run is not the five characters that matched:\n{dump}"
    );

    // Nothing matches: the view says so rather than showing an empty screen.
    support::type_text(&mut app, "zzz");
    let dump = support::render(&mut app, 66, 12);
    assert!(
        support::text_block(&dump).contains("no setting by that name"),
        "an empty screen with no reason:\n{dump}"
    );

    // Backspace brings them back.
    for _ in 0..3 {
        support::press(&mut app, KeyCode::Backspace);
    }
    assert_eq!(app.settings().expect("the settings").rows().len(), 1);
}

/// Escape closes the view, and the settings are read from the file the next
/// time it opens: what is on the screen is what is in the file.
#[test]
fn the_view_closes_and_the_file_is_what_it_shows() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let file = temporary("closes");
    let mut app = open(&file);
    support::press(&mut app, KeyCode::Esc);
    assert!(app.settings().is_none(), "the view stayed open");

    // A file written by someone else -- an editor, another obelus -- is what
    // a fresh application reads.
    std::fs::write(&file, "theme = \"light\"\nicons = false\nblame = false\n")
        .expect("writing the file");
    let mut second = App::new(vec![support::open_fixture("sample.rs")]);
    second.config_file_for_test(file.clone());
    assert_eq!(second.theme().name, "light");
    assert!(!second.config().blame);
    obelus::icons::use_glyphs(true);
}

/// An application that was never told where its settings live does not write
/// any: every test is one of those, and the reader's own file is not
/// something a test may touch.
#[test]
fn nothing_is_written_without_being_told_where() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    support::lay_out(&mut app, 66, 12);
    dispatch::dispatch(&mut app, Command::ConfigOpen);
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);
    // It still took effect, and there was nowhere to write it.
    assert!(!app.config().icons);
    assert_eq!(app.note(), None, "it complained about not saving");
    obelus::icons::use_glyphs(true);
}

/// Choosing a theme from the theme picker is kept too: a reader who picks
/// one and finds the old one back tomorrow was given a preview rather than
/// a choice.
#[test]
fn the_theme_picker_writes_its_choice_down() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let file = temporary("theme-picker");
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.config_file_for_test(file.clone());
    support::lay_out(&mut app, 60, 12);

    dispatch::dispatch(&mut app, Command::ThemeSelect);
    // The list opens on the theme in force, so the next one is the other.
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);

    let chosen = app.theme().name.to_string();
    assert_eq!(
        config::from_toml(&std::fs::read_to_string(&file).expect("the file")).theme,
        chosen,
        "the file does not say which theme was chosen"
    );
}

/// The keys that reach the ends of a document reach the ends of a settings
/// page too, and paging moves by what the page shows. A key should not mean
/// one thing in one view and nothing in the next.
#[test]
fn the_ends_and_the_pages_are_reachable() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let file = temporary("ends");
    let mut app = open(&file);
    let focus = |app: &App| app.settings().expect("the settings").focus();

    // Two rows on the appearance tab: End reaches the second, Home the
    // first, and neither wraps past its end.
    support::press(&mut app, KeyCode::End);
    assert_eq!(focus(&app), 1, "End did not reach the last row");
    support::press(&mut app, KeyCode::End);
    assert_eq!(focus(&app), 1, "End walked past the end");
    support::press(&mut app, KeyCode::Home);
    assert_eq!(focus(&app), 0);

    // A page is what the page shows, and paging is clamped rather than
    // wrapped: a page that wrapped past the end would overshoot what the
    // reader was reaching for.
    support::press(&mut app, KeyCode::PageDown);
    assert_eq!(focus(&app), 1, "PageDown did not reach the end");
    support::press(&mut app, KeyCode::PageDown);
    assert_eq!(focus(&app), 1, "PageDown wrapped");
    support::press(&mut app, KeyCode::PageUp);
    assert_eq!(focus(&app), 0, "PageUp did not come back");
}

/// The agents page is cards, and the same keys walk them: forty of them
/// scroll, the ends are reachable, and a page moves by the cards that fit
/// rather than by a number of rows.
#[test]
fn the_agents_page_is_a_list_of_cards() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let file = temporary("cards");
    let mut app = open(&file);
    // Onto the agents tab, which is the last one.
    support::press(&mut app, KeyCode::Left);
    assert!(app.settings().expect("the settings").on_agents());

    let agents: Vec<obelus::agent::Agent> = (0..12)
        .map(|index| obelus::agent::Agent {
            id: format!("agent-{index}"),
            name: format!("Agent {index}"),
            version: "1.0.0".to_string(),
            description: "One of several".to_string(),
            authors: vec!["Somebody".to_string()],
            license: "MIT".to_string(),
            website: None,
            distribution: obelus::agent::Distribution::Node {
                package: format!("agent-{index}@1.0.0"),
                arguments: Vec::new(),
            },
        })
        .collect();
    app.handle(Event::Registry {
        agents,
        failure: None,
    });

    let dump = support::render(&mut app, 76, 16);
    let text = support::text_block(&dump);
    assert!(
        text.contains("Agent 0") && text.contains("One of several"),
        "not a card:\n{dump}"
    );
    // Not all twelve: the page scrolls, and a card is several rows.
    assert!(
        !text.contains("Agent 11"),
        "twelve cards fitted a sixteen-row screen:\n{dump}"
    );

    // End reaches the last card, and it is on screen.
    support::press(&mut app, KeyCode::End);
    let dump = support::render(&mut app, 76, 16);
    assert!(
        support::text_block(&dump).contains("Agent 11"),
        "End did not bring the last card on screen:\n{dump}"
    );
    assert_eq!(app.settings().expect("the settings").focus(), 11);

    // And the filter is the name, not the description: every one of these
    // has the same description, and only one has this name.
    support::type_text(&mut app, "Agent 1");
    let showing = app
        .settings()
        .expect("the settings")
        .agents(&app.listed_agents())
        .len();
    assert_eq!(showing, 3, "not the names that match: Agent 1, 10, 11");
    support::type_text(&mut app, "0");
    assert_eq!(
        app.settings()
            .expect("the settings")
            .agents(&app.listed_agents())
            .len(),
        1
    );
    support::type_text(&mut app, " of several");
    assert_eq!(
        app.settings()
            .expect("the settings")
            .agents(&app.listed_agents())
            .len(),
        0,
        "the description was searched"
    );
}

/// The list is read on a thread -- both the cached copy and the fetched
/// one -- so opening the settings does no file reading and no waiting. A
/// fetch that fails says so on the page and lets the next visit try again:
/// a session that started with no network may have one later.
#[test]
fn a_failed_fetch_says_so_and_is_tried_again() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let file = temporary("registry");
    let mut app = open(&file);
    support::press(&mut app, KeyCode::Left);
    assert!(app.settings().expect("the settings").on_agents());

    // Nothing has arrived yet: the page says what it is doing.
    let dump = support::render(&mut app, 70, 12);
    assert!(
        support::text_block(&dump).contains("fetching the list of agents"),
        "not the waiting page:\n{dump}"
    );
    assert_eq!(app.registry_failure(), None);

    // It failed, and there was nothing cached: the page says that instead,
    // because "fetching" would be a lie by now.
    app.handle(Event::Registry {
        agents: Vec::new(),
        failure: Some("dns error: no such host".to_string()),
    });
    let dump = support::render(&mut app, 70, 12);
    assert!(
        support::text_block(&dump).contains("could not fetch"),
        "the page still says it is fetching:\n{dump}"
    );
    assert_eq!(app.registry_failure(), Some("dns error: no such host"));

    // Reopening the page tries again, which shows as the reason going
    // away: a session that started with no network may have one by now.
    support::press(&mut app, KeyCode::Esc);
    dispatch::dispatch(&mut app, Command::ConfigOpen);
    assert_eq!(
        app.registry_failure(),
        None,
        "reopening the page did not try again"
    );
    support::press(&mut app, KeyCode::Left);

    // An empty answer with nothing wrong leaves whatever list there is:
    // the cached one arriving after the fetched one must not wipe it.
    let one = obelus::agent::Agent {
        id: "one".to_string(),
        name: "The One".to_string(),
        version: "1.0.0".to_string(),
        description: "An agent".to_string(),
        authors: vec!["Someone".to_string()],
        license: "MIT".to_string(),
        website: None,
        distribution: obelus::agent::Distribution::Node {
            package: "one@1.0.0".to_string(),
            arguments: Vec::new(),
        },
    };
    app.handle(Event::Registry {
        agents: vec![one.clone()],
        failure: None,
    });
    app.handle(Event::Registry {
        agents: Vec::new(),
        failure: None,
    });
    let dump = support::render(&mut app, 70, 12);
    assert!(
        support::text_block(&dump).contains("The One"),
        "an empty answer wiped the list:\n{dump}"
    );

    // And the list arriving later clears it, whichever visit fetched it.
    app.handle(Event::Registry {
        agents: vec![one],
        failure: None,
    });
    assert_eq!(app.registry_failure(), None);
    let dump = support::render(&mut app, 70, 12);
    assert!(
        support::text_block(&dump).contains("The One"),
        "the list did not replace the reason:\n{dump}"
    );
}

/// The window of cards moves only when the focus leaves it.
///
/// Worked out from the focus each frame instead, the focused card sits as
/// low on the screen as it will go -- so every step up scrolls, and the
/// page slides under a reader who is nowhere near its edge.
#[test]
fn the_cards_scroll_only_at_an_edge() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let file = temporary("window");
    let mut app = open(&file);
    support::press(&mut app, KeyCode::Left);

    let agents: Vec<obelus::agent::Agent> = (0..12)
        .map(|index| obelus::agent::Agent {
            id: format!("agent-{index}"),
            name: format!("Agent {index}"),
            version: "1.0.0".to_string(),
            description: "One of several".to_string(),
            authors: vec!["Somebody".to_string()],
            license: "MIT".to_string(),
            website: None,
            distribution: obelus::agent::Distribution::Node {
                package: format!("agent-{index}@1.0.0"),
                arguments: Vec::new(),
            },
        })
        .collect();
    app.handle(Event::Registry {
        agents,
        failure: None,
    });

    // A step, then the frame it produces: the window is settled against the
    // room the page has, which only a frame knows.
    let step = |app: &mut obelus::app::App, key: KeyCode| {
        support::press(app, key);
        let dump = support::render(app, 76, 16);
        (app.settings().expect("the settings").top(), dump)
    };

    // Every card is four rows, so three whole ones fit the thirteen this
    // screen leaves the page -- counted by their last row, because the
    // fourth card's name is drawn in the row left over. Asserted rather
    // than assumed: the whole point is where the fourth step lands.
    let dump = support::render(&mut app, 76, 16);
    let whole = support::text_block(&dump)
        .matches("1.0.0 \u{b7} Somebody \u{b7} MIT")
        .count();
    assert_eq!(whole, 3, "not three whole cards:\n{dump}");

    // Down within the window moves nothing.
    for expected in 1..3 {
        let (top, dump) = step(&mut app, KeyCode::Down);
        assert_eq!(top, 0, "the page scrolled at card {expected}:\n{dump}");
        assert!(support::text_block(&dump).contains("Agent 0"));
    }

    // The fourth card is off the window, so the window moves -- by one
    // card, not by a screenful.
    let (top, dump) = step(&mut app, KeyCode::Down);
    assert_eq!(top, 1, "the page did not scroll:\n{dump}");
    let text = support::text_block(&dump);
    assert!(!text.contains("Agent 0"), "it scrolled by less:\n{dump}");
    assert!(text.contains("Agent 3"), "the focused card is off:\n{dump}");

    // Back up. The focused card is inside the window, so the window stays
    // where it is: this is the one a window derived from the focus got
    // wrong, by putting the focused card at the bottom every time.
    for _ in 0..2 {
        let (top, dump) = step(&mut app, KeyCode::Up);
        assert_eq!(top, 1, "going up scrolled from the middle:\n{dump}");
        // On the screen and not only in the number: a view that works the
        // window out from the focus keeps a correct `top` and draws
        // something else.
        let text = support::text_block(&dump);
        assert!(!text.contains("Agent 0"), "going up scrolled:\n{dump}");
        assert!(text.contains("Agent 3"), "going up scrolled:\n{dump}");
    }

    // Now the focus is the window's first card, and one more step moves it.
    let (top, dump) = step(&mut app, KeyCode::Up);
    assert_eq!(top, 0, "the window did not follow the focus up:\n{dump}");
    assert!(support::text_block(&dump).contains("Agent 0"));
}
