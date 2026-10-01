//! The settings view: tabs, a filter, and a control per row.

mod support;

use crossterm::event::KeyCode;
use obelus_app::{
    app::{App, dispatch},
    event::Event,
};
use obelus_command::Command;
use obelus_component::settings::Shown;

/// Applying a setting touches process-wide state -- the glyph switch is one
/// switch the drawing code can read without a flag threaded into every
/// function -- so these tests take turns. Tests in one file share a process,
/// and every test here applies a setting.
static SETTINGS: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// A settings file of its own for one test.
///
/// The guard is returned rather than just the path, and has to be held: it
/// is what takes the directory away again when the test passes.
fn temporary(name: &str) -> support::Scratch {
    support::Scratch::new(&format!("settings-{name}"))
}

/// Where that test's settings file goes.
fn settings_file(scratch: &support::Scratch) -> std::path::PathBuf {
    scratch.join("config.toml")
}

/// A project of its own for one test, with settings in it.
fn project(name: &str, contents: &str) -> support::Scratch {
    let scratch = support::Scratch::new(&format!("project-{name}"));
    std::fs::create_dir_all(scratch.path().join(".obelus")).expect("the directory");
    std::fs::write(scratch.path().join(".obelus").join("config.toml"), contents).expect("the file");
    scratch
}

fn open(file: &std::path::Path) -> App {
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.config_file_for_test(file.to_path_buf());
    support::lay_out(&mut app, 66, 12);
    dispatch::dispatch(&mut app, Command::ConfigOpen);
    app
}

/// How many settings this Obelus puts on the page.
///
/// Not every setting there is: `font_size` means nothing in a terminal and
/// is not offered in one, the same way the glyph switch is not offered in a
/// window.
fn shown() -> usize {
    obelus_config::ALL
        .iter()
        .filter(|setting| setting.shown())
        .count()
}

/// The settings are a dialog: nothing of Obelus's own opens over them.
///
/// `ctrl+q` used to be among the keys pressed here, and it is not any more:
/// what this rule is about is a key *opening* something over the page, and
/// leaving opens nothing. It keeps the key on purpose now, which
/// `leaving_closes_the_settings_and_asks` is about.
#[test]
fn nothing_of_obeluss_own_opens_over_the_settings() {
    let _taken = SETTINGS.lock().expect("the lock");
    let scratch = temporary("modal");
    let mut app = open(&settings_file(&scratch));
    let page = support::render(&mut app, 66, 12);
    for key in ['o', 'e', 'p'] {
        support::press_control(&mut app, key);
    }
    assert!(app.picker().is_none(), "a list opened over the settings");
    assert_eq!(
        support::text_block(&page),
        support::text_block(&support::render(&mut app, 66, 12)),
        "something opened over the settings"
    );
}

/// And the one key that does not open anything leaves, from in here too.
///
/// With nothing unwritten there is nothing to ask about, so it goes at
/// once -- and the page it was showing is not something to put back,
/// because Obelus is leaving.
#[test]
fn leaving_closes_the_settings_and_asks() {
    let _taken = SETTINGS.lock().expect("the lock");
    let scratch = temporary("modal-quit");
    let mut app = open(&settings_file(&scratch));
    support::render(&mut app, 66, 12);
    assert!(!app.should_quit());

    support::press_control(&mut app, 'q');
    assert!(app.should_quit(), "ctrl+q did nothing from the settings");
}

/// A switch changes the setting, the running program, and the file -- in
/// that order and all at once. A setting that took effect on restart would
/// be a setting nobody can tell they have changed.
#[test]
fn a_switch_takes_effect_and_is_written_down() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let scratch = temporary("switch");
    let file = settings_file(&scratch);
    let mut app = open(&file);

    // Down onto the second row of the appearance tab, which is the glyphs.
    support::press(&mut app, KeyCode::Down);
    assert!(!obelus_icons::enabled(), "the glyphs start off");
    support::press(&mut app, KeyCode::Enter);

    assert!(obelus_icons::enabled(), "the glyphs are still off");
    assert!(app.config().icons, "the setting did not change");
    let written = std::fs::read_to_string(&file).expect("the file was written");
    assert!(
        obelus_config::from_toml(&written).icons,
        "the file does not say so: {written:?}"
    );

    // And again the other way.
    support::press(&mut app, KeyCode::Enter);
    assert!(!obelus_icons::enabled(), "enter did not toggle it back");

    // Space is a character, not a second way to flip it: these pages
    // filter by typing, and "Agent 1" is a name a reader will type.
    support::press(&mut app, KeyCode::Char(' '));
    assert!(!obelus_icons::enabled(), "space flipped the switch");
    assert_eq!(app.settings().expect("the settings").query(), " ");

    // Put the glyphs back for whatever runs next: this is process-wide
    // state, which is the price of a switch the drawing code can read.
    obelus_icons::use_glyphs(false);
}

/// A setting with choices opens the ordinary compact list over the view:
/// the same list the symbol menu is, so it filters by typing and scrolls
/// and marks its selection the way every other list does.
#[test]
fn a_choice_opens_the_list_every_other_choice_uses() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let scratch = temporary("droplist");
    let file = settings_file(&scratch);
    let mut app = open(&file);

    // The theme is the first row.
    support::press(&mut app, KeyCode::Enter);
    let picker = app.picker().expect("the choices");
    assert_eq!(
        picker
            .matches()
            .map(|item| item.label.clone())
            .collect::<Vec<_>>(),
        obelus_theme::builtin::ALL
            .iter()
            .map(|(name, _)| (*name).to_string())
            .collect::<Vec<_>>(),
        "not the theme's choices"
    );
    // Opened on the one in force, so the list starts by saying which.
    assert_eq!(
        picker.selected_item().map(|item| item.label.clone()),
        Some("dark".to_string())
    );
    // And the settings are still there, underneath. Twenty rows rather than
    // twelve: the list is allowed ten of them and there are eleven themes to
    // put in it, so on a screen that short there is nothing left for what it
    // is over -- which is the screen being small, not the list being wrong.
    assert!(app.settings().is_some(), "the view went away");
    let dump = support::render(&mut app, 66, 20);
    assert!(
        support::text_block(&dump).contains("Appearance"),
        "the settings are not behind the list:\n{dump}"
    );

    // It filters by typing, which is the whole reason it is this list.
    let all = app.picker().expect("the choices").match_count();
    support::type_text(&mut app, "light");
    let picker = app.picker().expect("the choices");
    assert!(picker.match_count() < all, "the query narrowed nothing");
    // Narrowed, and not down to one: three of the themes have `light` in
    // their names. What the reader typed is the whole of this one's, which
    // is what puts it at the top -- and asserting a count here instead
    // would be a test that has to be edited every time a theme is added,
    // which is a test about the shelf rather than about the list.
    assert_eq!(
        picker.selected_item().map(|item| item.label.clone()),
        Some("light".to_string()),
        "the closest match is not the one typed"
    );
    support::press(&mut app, KeyCode::Enter);

    assert!(app.picker().is_none(), "the list stayed open");
    assert_eq!(app.theme_name(), "light", "the theme did not change");
    assert_eq!(
        obelus_config::from_toml(&std::fs::read_to_string(&file).expect("the file")).theme,
        "light"
    );

    // Escape closes the list and leaves the setting alone -- and leaves the
    // settings view open, because that is what is behind it.
    support::press(&mut app, KeyCode::Enter);
    support::press(&mut app, KeyCode::Esc);
    assert!(app.picker().is_none(), "escape left the list open");
    assert_eq!(app.theme_name(), "light", "escape chose something");
    assert!(app.settings().is_some(), "escape closed the view as well");
}

/// And walking that list wears each theme as it goes, the same as the theme
/// list does. The colours *are* the choice: a list of two words a reader has
/// to pick between blind says nothing that the two words did not.
#[test]
fn walking_the_theme_list_wears_each_one() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let scratch = temporary("theme-droplist");
    let file = settings_file(&scratch);
    let mut app = open(&file);

    // The page as it stands, to come back to.
    let closed = support::render(&mut app, 66, 12);

    // The theme is the first row, and its list opens on the one in force.
    support::press(&mut app, KeyCode::Enter);
    let dark = support::render(&mut app, 66, 12);
    assert_eq!(
        app.theme_name(),
        "dark",
        "the list did not open on this one"
    );

    // The next row is the other theme, and the screen is wearing it before
    // anything has been chosen.
    support::press(&mut app, KeyCode::Down);
    let light = support::render(&mut app, 66, 12);
    assert_eq!(
        app.theme_name(),
        "light",
        "the row moved and nothing changed"
    );
    assert_ne!(
        support::style_block(&light),
        support::style_block(&dark),
        "the screen is wearing the same colours:\n{light}"
    );

    // And walking away puts back the one that was on. Nothing was chosen,
    // so nothing was decided.
    support::press(&mut app, KeyCode::Esc);
    let after = support::render(&mut app, 66, 12);
    assert_eq!(app.theme_name(), "dark", "the preview stuck");
    assert!(
        !file.exists(),
        "a theme nobody chose was written to the settings file"
    );
    assert_eq!(
        support::style_block(&after),
        support::style_block(&closed),
        "the colours did not come back:\n{after}"
    );

    // And a theme that *was* chosen stays chosen: what the list put back is
    // the theme nobody picked, and the next escape anywhere in Obelus has
    // nothing to do with it.
    support::press(&mut app, KeyCode::Enter);
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);
    assert_eq!(app.theme_name(), "light", "the choice did not take");
    support::press(&mut app, KeyCode::Esc);
    support::press_function(&mut app, 1);
    support::press(&mut app, KeyCode::Esc);
    assert_eq!(
        app.theme_name(),
        "light",
        "escaping a later list put back a theme the reader had chosen"
    );
}

/// A switch is a box, and enter marks it. The arrows are not it -- they
/// walk the tabs, as they do in every other view with tabs on it.
#[test]
fn a_switch_is_a_box_that_enter_marks() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let scratch = temporary("switch-box");
    let file = settings_file(&scratch);
    let mut app = open(&file);
    support::press(&mut app, KeyCode::Down);

    // The box on the row, marked or not. It was a slider, and this looked
    // for where its knob had slid to. Read off the row rather than asked of
    // `ui::tick`: what matters is that the two states differ.
    let box_of = |app: &mut App| {
        let dump = support::render(app, 66, 12);
        support::text_block(&dump)
            .lines()
            .find(|row| row.contains("Nerd Font"))
            .map(|row| support::glyph_after(row, "Nerd Font glyphs"))
            .expect("the row")
    };

    assert!(!app.config().icons, "the glyphs start off");
    let off = box_of(&mut app);
    support::press(&mut app, KeyCode::Enter);
    assert!(app.config().icons, "enter did not flip it");
    assert_ne!(
        box_of(&mut app),
        off,
        "the box reads the same whichever way it is set"
    );

    // The arrows do not touch it: they are the tabs'.
    support::press(&mut app, KeyCode::Right);
    assert!(app.config().icons, "an arrow flipped the switch");
    support::press(&mut app, KeyCode::Left);
    assert!(app.config().icons, "an arrow flipped the switch");

    obelus_icons::use_glyphs(false);
}

/// The tabs are the groups, the arrows walk them -- only the arrows, the way
/// they do in every other view with tabs on it -- and the focus comes back
/// One page of settings, with a heading where one group of them ends and
/// the next begins -- and the tabs are the three shapes of page.
///
/// Broken deliberately by giving each group a tab of its own: a reader
/// looking for "the one about wrapping" had to guess which of four pages
/// somebody had filed it under, and the last of those pages had one row on
/// it.
#[test]
fn every_setting_is_on_one_page_under_a_heading() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let scratch = temporary("tabs");
    let file = settings_file(&scratch);
    let mut app = open(&file);

    let rows = |app: &App| {
        app.settings()
            .expect("the settings")
            .rows(app.agent_offering().as_ref())
            .iter()
            .filter_map(|shown| Some(shown.setting()?.name.to_string()))
            .collect::<Vec<_>>()
    };
    // Every setting this Obelus shows, in the order the groups are written
    // in. Shown rather than every setting there is: a terminal does not
    // offer how big the text is, because its font is its own.
    assert_eq!(rows(&app).len(), shown());
    assert_eq!(rows(&app)[0], "Theme");
    assert_eq!(rows(&app).last().map(String::as_str), Some("Workflow"));

    // A heading on the first of each group and on nothing else, so the
    // focus never has a row to step over.
    let opens: Vec<Option<&'static str>> = app
        .settings()
        .expect("the settings")
        .rows(app.agent_offering().as_ref())
        .iter()
        .map(|shown| match shown {
            Shown::Obelus { opens, .. } => opens.map(obelus_config::Group::label),
            Shown::Agent { .. } | Shown::Silent { .. } => None,
        })
        .collect();
    assert_eq!(opens[0], Some("Appearance"));
    assert_eq!(opens[1], None, "a second heading inside one group");
    assert_eq!(
        opens.iter().filter(|opens| opens.is_some()).count(),
        obelus_config::Group::ALL.len(),
        "not one heading per group: {opens:?}"
    );

    // And the tabs are the pages: the settings, the keys, the agents.
    assert_eq!(
        app.settings().expect("the settings").tabs(),
        ["Settings", "Keys", "Agents"]
    );
    support::press(&mut app, KeyCode::Tab);
    assert!(app.settings().expect("the settings").on_keys());
    support::press(&mut app, KeyCode::Tab);
    assert!(app.settings().expect("the settings").on_agents());
    assert!(
        rows(&app).is_empty(),
        "the agents page has settings rows on it"
    );
    support::press(&mut app, KeyCode::BackTab);
    assert!(app.settings().expect("the settings").on_keys());

    // And the arrows are not one of the keys that walks them any more:
    // the filter is a line with a caret in it, and that is where a caret
    // goes. One way to do it, the way every other tabbed view here works.
    support::press(&mut app, KeyCode::Right);
    assert!(app.settings().expect("the settings").on_keys());
    support::press(&mut app, KeyCode::Left);
    assert!(app.settings().expect("the settings").on_keys());
}

/// A setting whose name is the whole of it keeps the blank under it.
///
/// The blank is what makes an entry an entry -- so that the next name is
/// not read as part of this one -- and a setting with no gloss has nothing
/// else between the two. It is also the one shape where the walk that lays
/// the page out and `Settings::setting_rows`, which the window is settled
/// by, disagreed: the window counted a row the walk never left room for,
/// which is a reader stepping onto an entry nobody drew.
///
/// Deliberate break: `y += tall + u16::from(!row.body.is_empty())` in
/// `placed`, which is what it was while every setting had a gloss.
/// `Nerd Font glyphs` is then drawn hard against `Theme`.
#[test]
fn a_setting_with_no_gloss_keeps_the_blank_under_it() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let scratch = temporary("no-gloss");
    let file = settings_file(&scratch);
    let mut app = open(&file);

    let dump = support::render(&mut app, 66, 12);
    let rows: Vec<&str> = support::text_block(&dump)
        .lines()
        .filter_map(|row| row.split_once('|'))
        .map(|(_, said)| said)
        .collect();
    let at = rows
        .iter()
        .position(|row| row.contains("Theme"))
        .expect("the row the theme is on");
    assert!(
        at + 1 < rows.len(),
        "the theme is the last row of the page:\n{dump}"
    );
    // Nothing on it but the bar's own column, which is every row's.
    assert!(
        !rows[at + 1].chars().any(char::is_alphanumeric),
        "the next setting is hard against the theme:\n{dump}"
    );
}

/// Typing narrows the rows, and the count on the status bar says how many
/// are left. Plainly by substring, because a reader typing "the" means the
/// word.
#[test]
fn typing_narrows_the_settings() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let scratch = temporary("filter");
    let file = settings_file(&scratch);
    let mut app = open(&file);

    support::type_text(&mut app, "theme");
    let dump = support::render(&mut app, 66, 12);
    assert_eq!(
        app.settings()
            .expect("the settings")
            .rows(app.agent_offering().as_ref())
            .len(),
        1,
        "the query narrowed nothing:\n{dump}"
    );
    assert!(
        support::text_block(&dump).contains("Theme"),
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
        .position(|row| row.contains("Theme"))
        .expect("the row that matched");
    // The "T" of "Theme" is the first cell of the name: past the
    // `NN|` the dump writes down its side, past the row's own left-hand
    // padding, and past the indent that puts a setting under its group.
    let name_at = 3 + 1 + usize::from(obelus_component::settings::GROUP_INDENT);
    let letters: Vec<char> = styles[row].chars().skip(name_at).take(12).collect();
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
        support::text_block(&dump).contains("No setting by that name"),
        "an empty screen with no reason:\n{dump}"
    );

    // Backspace brings them back.
    for _ in 0..3 {
        support::press(&mut app, KeyCode::Backspace);
    }
    assert_eq!(
        app.settings()
            .expect("the settings")
            .rows(app.agent_offering().as_ref())
            .len(),
        1
    );
}

/// Escape closes the view, and the settings are read from the file the next
/// time it opens: what is on the screen is what is in the file.
#[test]
fn the_view_closes_and_the_file_is_what_it_shows() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let scratch = temporary("closes");
    let file = settings_file(&scratch);
    let mut app = open(&file);
    support::press(&mut app, KeyCode::Esc);
    assert!(app.settings().is_none(), "the view stayed open");

    // A file written by someone else -- an editor, another Obelus -- is what
    // a fresh application reads.
    std::fs::write(
        &file,
        "theme = \"light\"\nicons = false\nblame_margin = false\n",
    )
    .expect("writing the file");
    let mut second = App::new(vec![support::open_fixture("sample.rs")]);
    second.config_file_for_test(file.clone());
    assert_eq!(second.theme_name(), "light");
    assert!(!second.config().blame_margin);
    obelus_icons::use_glyphs(false);
}

/// Settings arriving from another machine are noticed, link and all.
///
/// Which is how anybody keeps settings in git: the file lives in a dotfiles
/// repository and the place Obelus looks is a link to it. What a `git pull`
/// rewrites is the file at the far end, so that is the path the change
/// arrives on -- not the one Obelus was told about.
///
/// Broken deliberately by comparing the event's path only against the
/// configured one: the change arrived on the file the link points at,
/// matched nothing, and the window went on showing the theme the reader had
/// already moved on from.
#[cfg(unix)]
#[test]
fn a_change_to_the_file_a_link_points_at_is_a_change_to_the_settings() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let scratch = support::Scratch::new("linked");
    let directory = scratch.path().to_path_buf();
    let repository = directory.join("dotfiles");
    let config_home = directory.join("config");
    std::fs::create_dir_all(&repository).expect("a directory");
    std::fs::create_dir_all(&config_home).expect("a directory");

    let real = repository.join("config.toml");
    std::fs::write(&real, "theme = \"dark\"\n").expect("the file");
    // Spelled the way the disk spells it, which is the way a watcher
    // reports it: on a mac the temporary directory is itself behind a
    // link, `/var` to `/private/var`, and no watcher says `/var`.
    let real = real.canonicalize().expect("the file");
    let linked = config_home.join("config.toml");
    std::os::unix::fs::symlink(&real, &linked).expect("a link");

    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.config_file_for_test(linked);
    assert_eq!(app.theme_name(), "dark", "it did not read through the link");

    // What another machine's change looks like once git has put it there:
    // the repository's own file, rewritten, and the watcher reporting that
    // path rather than the link's.
    std::fs::write(&real, "theme = \"light\"\n").expect("the file");
    app.handle(Event::Watched(obelus_watch::Changed { path: real }));
    assert_eq!(
        app.theme_name(),
        "light",
        "a setting that arrived through the link was not picked up"
    );
}

/// What a setting does goes under its name, indented, and carries onto
/// another row rather than being cut.
///
/// Beside the name the two were competing for one row, and the one that lost
/// was the description -- cut off with an ellipsis on exactly the rows that
/// had most to explain, and cut off further still on a row the project had
/// pinned, where the file's name takes the space as well.
///
/// Broken deliberately by clipping the description to one row instead of
/// wrapping it: the tail of the sentence was nowhere on screen and the last
/// assertion failed.
#[test]
fn what_a_setting_does_goes_under_it_and_wraps() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let scratch = temporary("under");
    let file = settings_file(&scratch);
    let mut app = open(&file);

    let dump = support::render(&mut app, 66, 14);
    let rows: Vec<&str> = support::text_block(&dump).lines().collect();
    let name = rows
        .iter()
        .position(|row| row.contains("Nerd Font glyphs"))
        .expect("the row");

    // Under it, and indented from it.
    let at = |row: &str, needle: &str| row.find(needle).map(|byte| row[..byte].chars().count());
    assert!(
        at(rows[name + 1], "In lists").is_some(),
        "what it does is not under its name:\n{dump}"
    );
    assert!(
        at(rows[name + 1], "In lists") > at(rows[name], "Nerd Font"),
        "it is not indented under the name:\n{dump}"
    );

    // And the whole sentence is there, carried onto as many rows as it
    // takes rather than cut off with an ellipsis.
    let said: String = rows[name + 1..name + 4].concat();
    assert!(
        said.contains("draws a box instead"),
        "the end of the sentence is nowhere:\n{dump}"
    );
    assert!(
        !said.contains('\u{2026}'),
        "it was cut rather than wrapped:\n{dump}"
    );
}

/// Walking to the last setting brings it on screen, however tall the
/// entries are.
///
/// The window is settled by *height* here, like the page of cards: an entry
/// is a name, the rows its description takes and a blank, so a page that
/// counted them as a row each would think four of them fit in four rows --
/// and the reader walking to the last one would be standing on something
/// that is not drawn.
///
/// Broken deliberately by settling with a height of one per entry: the
/// fourth name was nowhere on the screen it is focused on.
#[test]
fn the_last_setting_can_be_walked_to_on_a_short_screen() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let scratch = temporary("short");
    let file = settings_file(&scratch);
    let mut app = open(&file);

    // A region of three rows, which is shorter than the first entry is
    // tall: walking to the second has to move the window or the reader is
    // standing on something nobody drew.
    support::lay_out(&mut app, 66, 7);
    support::press(&mut app, KeyCode::Down);
    let dump = support::render(&mut app, 66, 7);
    let focused = app.settings().expect("the settings").focus();
    let offering = app.agent_offering();
    let rows = app
        .settings()
        .expect("the settings")
        .rows(offering.as_ref());
    let name = rows[focused.min(rows.len() - 1)].label();
    assert!(
        support::text_block(&dump).contains(name),
        "the entry the keys are on is not on screen: {name}\n{dump}"
    );
}

/// A project can carry settings of its own, and they win where they say
/// anything.
///
/// Which is what a project is for: everybody reading this repository gets
/// its wrapped lines, whatever they have set for themselves elsewhere.
///
/// Broken deliberately by reading the project's table into a fresh config
/// instead of over the reader's: the theme the reader had chosen came back
/// as the default, and the last assertion failed.
#[test]
fn a_tree_lays_its_own_settings_over_the_readers() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let scratch = project("over", "wrap = true\n");
    let root = scratch.path().to_path_buf();

    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    // The reader's own, as they would have come from their file.
    app.configure(
        obelus_config::Config {
            theme: "light".to_string(),
            wrap: false,
            ..obelus_config::Config::default()
        },
        Vec::new(),
    );
    app.working_directory_for_test(root.clone());

    assert!(app.config().wrap, "the project's setting did not take");
    assert_eq!(
        app.theme_name(),
        "light",
        "the project took away a setting it never named"
    );
}

/// A project may not choose the agent, or rebind a key.
///
/// A project is written by whoever wrote the project. Most of these settings
/// are harmless to hand over; starting a program is not, and neither is moving
/// the keys under somebody's fingers.
///
/// Broken deliberately by giving `agent` and `keys` `Reach::Anywhere`: both
/// arrived from the project and both assertions failed.
#[test]
fn a_tree_may_not_start_an_agent_or_move_a_key() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let scratch = project(
        "reach",
        "agent = \"claude-acp\"\n[keys]\nquit = \"ctrl+x\"\n",
    );
    let root = scratch.path().to_path_buf();

    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.configure(obelus_config::Config::default(), Vec::new());
    app.working_directory_for_test(root.clone());

    assert_eq!(app.config().agent, None, "a project started an agent");
    assert!(
        app.config().keys.is_empty(),
        "a project moved a key: {:?}",
        app.config().keys
    );
}

/// A setting the project has is not the reader's to change, and the row says
/// which file has it.
///
/// Broken deliberately by letting `change_setting` write anyway: the switch
/// moved, the file the reader's own settings live in got a line the project
/// overrides, and the first assertion failed.
#[test]
fn a_setting_the_tree_has_cannot_be_changed_here() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let scratch = project("pinned", "wrap = true\n");
    let root = scratch.path().to_path_buf();

    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.configure(obelus_config::Config::default(), Vec::new());
    app.working_directory_for_test(root.clone());
    support::lay_out(&mut app, 76, 12);
    dispatch::dispatch(&mut app, Command::ConfigOpen);

    // Onto the wrapped-lines row and try to turn it off.
    support::type_text(&mut app, "wrap");
    support::press(&mut app, KeyCode::Enter);
    assert!(
        app.config().wrap,
        "a setting the project has was changed from the settings page"
    );

    // And the row says where it comes from.
    let dump = support::render(&mut app, 76, 12);
    assert!(
        support::text_block(&dump).contains(&support::as_shown(".obelus/config.toml")),
        "the row does not say which file has it:\n{dump}"
    );
}

/// The project's own page writes to the project's file, and leaves alone what
/// it did not come for.
///
/// The file is written by hand and committed, so it has comments in it and
/// an order somebody chose. Obelus's own file it writes whole; this one it
/// edits.
///
/// Broken deliberately by writing the file from a `toml::Table`: the
/// comments were gone on the first switch, and the last assertion failed.
#[test]
fn the_trees_page_edits_the_trees_file() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let root = project(
        "write",
        "# what this project needs\n\nwrap = true\n# the margin is noisy here\nblame_margin = false\n",
    );

    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.configure(obelus_config::Config::default(), Vec::new());
    app.working_directory_for_test(root.path().to_path_buf());
    support::lay_out(&mut app, 76, 16);
    dispatch::dispatch(&mut app, Command::ConfigProject);
    // Onto `blame_margin` by narrowing to it, which is one row, and turn it
    // on.
    support::type_text(&mut app, "blame");
    support::press(&mut app, KeyCode::Enter);

    let written =
        std::fs::read_to_string(root.join(".obelus").join("config.toml")).expect("the file");
    assert!(
        written.contains("blame_margin = true"),
        "not written: {written:?}"
    );
    assert!(
        written.contains("# what this project needs"),
        "the file's own heading is gone: {written:?}"
    );
    assert!(
        written.contains("# the margin is noisy here"),
        "a comment about a setting is gone: {written:?}"
    );
    // And it took, which is the whole point of writing it.
    assert!(app.config().blame_margin, "the setting did not take");
}

/// Delete takes a setting out of the project's file, and the file keeps its
/// own heading.
///
/// `delete` means on this page what it means on the keys page: take this
/// one out. What was written above the key goes with it -- a comment
/// touching a key is about that key -- but whatever is an empty line away
/// is the file's own and stays.
///
/// Broken deliberately by removing the key without carrying its prefix on:
/// the heading went with the first key and the file lost it.
#[test]
fn delete_takes_a_setting_out_and_leaves_the_heading() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let root = project(
        "Unset",
        "# what this project needs\n\nwrap = true\nblame_margin = true\n",
    );

    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.configure(obelus_config::Config::default(), Vec::new());
    app.working_directory_for_test(root.path().to_path_buf());
    support::lay_out(&mut app, 76, 16);
    dispatch::dispatch(&mut app, Command::ConfigProject);
    support::type_text(&mut app, "wrap");
    support::press(&mut app, KeyCode::Delete);

    let written =
        std::fs::read_to_string(root.join(".obelus").join("config.toml")).expect("the file");
    assert!(!written.contains("wrap"), "still there: {written:?}");
    assert!(
        written.contains("blame_margin = true"),
        "took the wrong one: {written:?}"
    );
    assert!(
        written.contains("# what this project needs"),
        "the heading went with the key: {written:?}"
    );
    // And the reader's own answer is what is in force again.
    assert!(!app.config().wrap, "the project still has it");
}

/// A project that has no settings file gets one the moment something is set.
///
/// Broken deliberately by refusing to write when the file is not there:
/// nothing happened and there was no file, which is a page that cannot be
/// used until somebody makes a file by hand.
#[test]
fn a_tree_with_no_settings_gets_a_file_when_one_is_set() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let root = support::Scratch::new("project-new");

    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.configure(obelus_config::Config::default(), Vec::new());
    app.working_directory_for_test(root.path().to_path_buf());
    support::lay_out(&mut app, 76, 16);
    dispatch::dispatch(&mut app, Command::ConfigProject);
    support::type_text(&mut app, "wrap");
    support::press(&mut app, KeyCode::Enter);

    let written = std::fs::read_to_string(root.join(".obelus").join("config.toml"))
        .expect("no file was made");
    assert!(written.contains("wrap = true"), "{written:?}");
}

/// On the project's page, a setting the project has not got says whose value is
/// showing -- and the two tabs a project may not have say so.
///
/// Broken deliberately by leaving `inherited` `None` for every row: the
/// rows the project does not set looked exactly like the one it does, and a
/// reader could not tell what this project had actually decided.
#[test]
fn the_trees_page_says_which_settings_are_not_its_own() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let root = project("whose", "wrap = true\n");

    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    // The theme they chose, and the margin's names written down at exactly
    // what Obelus would have done anyway: a reader who agrees has still
    // been here, and the column has to say so.
    app.configure(
        obelus_config::Config {
            theme: "light".to_string(),
            blame_margin: obelus_config::Config::default().blame_margin,
            ..obelus_config::Config::default()
        },
        vec!["theme", "blame_margin"],
    );
    app.working_directory_for_test(root.path().to_path_buf());
    // Tall enough for the whole page: every setting is on one now, and this
    // reads two of them against each other.
    support::lay_out(&mut app, 76, 32);
    dispatch::dispatch(&mut app, Command::ConfigProject);

    // The theme is the reader's; the glyphs are nobody's.
    let dump = support::render(&mut app, 76, 32);
    let text = support::text_block(&dump);
    assert!(text.contains("Global"), "{dump}");
    assert!(text.contains("Default"), "{dump}");
    // And the file it would be writing is named on the tab row.
    assert!(
        text.contains(&support::as_shown(".obelus/config.toml")),
        "{dump}"
    );

    // And the setting the project does have says so, in the same column: a
    // column where two of the three layers have a word and the third is
    // blank asks the reader to read an absence.
    let dump = support::render(&mut app, 76, 32);
    let wrap = support::text_block(&dump)
        .lines()
        .find(|row| row.contains("Wrapping"))
        .expect("the row");
    assert!(
        wrap.contains("Project"),
        "a setting the project has does not say so: {wrap:?}"
    );
    // And beside it, one the reader wrote down and the project says nothing
    // about -- written at exactly what Obelus would have done anyway. The
    // column asks whether their file speaks about it, not whether it
    // disagrees: a reader who wrote a line and happened to agree was being
    // told they had never been here.
    let blame = support::text_block(&dump)
        .lines()
        .find(|row| row.contains("Blame"))
        .expect("the row")
        .to_string();
    assert!(
        blame.contains("Global"),
        "a setting written down at its default is not the reader's: {blame:?}"
    );

    // The two tabs a project may not have are not on it at all, which is
    // `the_projects_page_has_one_tab`'s subject: here it is only that
    // walking the tabs stays on this page.
    support::press(&mut app, KeyCode::Tab);
    assert!(
        !app.settings().expect("the settings").on_keys(),
        "a tab walk left the only page the project has"
    );
}

/// A project that acquires settings while Obelus is looking at it is heard.
///
/// Several Obelus processes on one project is the ordinary way to work, and
/// the ordinary project has no settings of its own until somebody gives it
/// some -- from the window next door, or in a pull. Watching only the file
/// that was there at startup is the "read once at startup" mistake with a
/// longer fuse: it looks right until the file is created.
///
/// Broken deliberately by comparing the change against the file the project
/// *has* rather than the one it would have: the event matched nothing, the
/// setting never arrived, and this failed.
#[test]
fn a_tree_that_gains_settings_while_obelus_is_open_is_heard() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let root = support::Scratch::new("project-later");

    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.configure(obelus_config::Config::default(), Vec::new());
    app.working_directory_for_test(root.path().to_path_buf());
    assert!(!app.config().wrap, "the project had settings already");

    // Somebody else writes the project's first settings, and the watcher
    // says so.
    let path = root.join(".obelus").join("config.toml");
    std::fs::create_dir_all(root.join(".obelus")).expect("the directory");
    std::fs::write(&path, "wrap = true\n").expect("the file");
    app.handle(Event::Watched(obelus_watch::Changed { path }));

    assert!(
        app.config().wrap,
        "a project that gained settings was not heard"
    );
}

/// Dim means "not yours to use here", so the two pages use it the opposite
/// way round.
///
/// On the reader's page a setting the project has taken is unusable, and the
/// whole row says so. On the project's page a setting the project has *not* got
/// is the one thing a reader can do something to -- pressing it is how a
/// setting becomes the project's -- so the row is ordinary there, and only
/// the word saying where the value comes from is dim. Drawn the other way,
/// a fresh project was a page of grey with nothing on it to look at.
///
/// Broken deliberately by dimming a row with an inherited value: the name
/// came out the same colour as the word beside it, and the first assertion
/// failed.
#[test]
fn the_trees_page_does_not_grey_out_what_can_be_set() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let root = project("grey", "wrap = true\n");

    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.configure(obelus_config::Config::default(), Vec::new());
    app.working_directory_for_test(root.path().to_path_buf());
    support::lay_out(&mut app, 76, 16);

    // The style letter of the first character of a run, on the row holding
    // it: two runs in one colour share a letter and two in different
    // colours cannot.
    let letter = |dump: &str, row_with: &str, needle: &str| -> char {
        let rows: Vec<&str> = support::text_block(dump).lines().collect();
        let styles: Vec<&str> = support::style_block(dump).lines().collect();
        let at = rows
            .iter()
            .position(|row| row.contains(row_with))
            .unwrap_or_else(|| panic!("{row_with:?} is not on screen:\n{dump}"));
        let column = rows[at]
            .find(needle)
            .map(|byte| rows[at][..byte].chars().count())
            .unwrap_or_else(|| panic!("{needle:?} is not on that row:\n{dump}"));
        styles[at].chars().nth(column).expect("a style")
    };

    // The project's page: the name is ordinary ink, the word beside it is not.
    dispatch::dispatch(&mut app, Command::ConfigProject);
    // Short of the whole name, so there is a letter of it the query did not
    // match: what a match wears is a background of its own, and a letter
    // carrying that says nothing about the ink this is asking after.
    support::type_text(&mut app, "bla");
    let dump = support::render(&mut app, 76, 16);
    assert_ne!(
        letter(&dump, "Blame", "me"),
        letter(&dump, "Blame", "Default"),
        "the name is as dim as the word saying the value is not the project's:\n{dump}"
    );

    // The reader's page: a setting the project has taken is dim throughout,
    // name and all, because there it really cannot be used.
    dispatch::dispatch(&mut app, Command::ConfigOpen);
    support::type_text(&mut app, "wrap");
    let dump = support::render(&mut app, 76, 16);
    assert_eq!(
        letter(&dump, "Wrapping", "ping"),
        letter(&dump, "Wrapping", &support::as_shown(".obelus/config.toml")),
        "a row the reader cannot use is not dim throughout:\n{dump}"
    );
}

/// The file the project's page writes to is named on the tab row, and only
/// where the tabs have left room for it.
///
/// Broken deliberately by asking whether the name *fits on the row* rather
/// than whether it starts after the tabs end: on a narrow screen it was
/// written over them, and `appearance  r.obelus/config.tomls` is what the
/// reader got.
#[test]
fn the_file_on_the_tab_row_does_not_write_over_the_tabs() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let root = support::Scratch::new("corner");
    // The longer of the two forms, which is the one that will not fit.
    root.write(".obelus/config.toml", "wrap = true\n");

    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.configure(obelus_config::Config::default(), Vec::new());
    app.working_directory_for_test(root.path().to_path_buf());
    support::lay_out(&mut app, 76, 10);
    dispatch::dispatch(&mut app, Command::ConfigProject);

    // The first row, which is the tabs': "Appearance" is a heading down the
    // page now and would find that instead.
    let tabs = |app: &mut App, width: u16| -> String {
        let dump = support::render(app, width, 10);
        support::text_block(&dump)
            .lines()
            .find(|row| !row.trim().is_empty())
            .expect("the tab row")
            .to_string()
    };

    // Wide: the name is there, after the tabs.
    let wide = tabs(&mut app, 76);
    assert!(
        wide.contains(&support::as_shown(".obelus/config.toml")),
        "{wide:?}"
    );
    assert!(
        wide.find("Settings") < wide.find(".obelus"),
        "the name is not after the tabs: {wide:?}"
    );

    // Narrow: the tab is whole, and the name is simply not there. Wide
    // enough for the tab the project's page has and not for the name after
    // it, which is the corner this is about.
    let narrow = tabs(&mut app, 30);
    assert!(
        narrow.contains("Settings"),
        "the tabs were written over: {narrow:?}"
    );
    assert!(
        !narrow.contains(".obelus"),
        "the name was squeezed in anyway: {narrow:?}"
    );
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
    assert!(app.config().icons);
    assert_eq!(app.note(), None, "it complained about not saving");
    obelus_icons::use_glyphs(false);
}

/// Choosing a theme from the theme picker is kept too: a reader who picks
/// one and finds the old one back tomorrow was given a preview rather than
/// a choice.
#[test]
fn the_theme_picker_writes_its_choice_down() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let scratch = temporary("theme-picker");
    let file = settings_file(&scratch);
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.config_file_for_test(file.clone());
    support::lay_out(&mut app, 60, 12);

    dispatch::dispatch(&mut app, Command::ThemeSelect);
    // The list opens on the theme in force, so the next one is the other.
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);

    let chosen = app.theme_name().to_string();
    assert_eq!(
        obelus_config::from_toml(&std::fs::read_to_string(&file).expect("the file")).theme,
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
    let scratch = temporary("ends");
    let file = settings_file(&scratch);
    let mut app = open(&file);
    let focus = |app: &App| app.settings().expect("the settings").focus();

    // Every setting on one page: End reaches the last, Home the first, and
    // neither wraps past its end.
    let last = shown() - 1;
    support::press(&mut app, KeyCode::End);
    assert_eq!(focus(&app), last, "End did not reach the last row");
    support::press(&mut app, KeyCode::End);
    assert_eq!(focus(&app), last, "End walked past the end");
    support::press(&mut app, KeyCode::Home);
    assert_eq!(focus(&app), 0);

    // A page is what the page shows, and paging is clamped rather than
    // wrapped: a page that wrapped past the end would overshoot what the
    // reader was reaching for.
    support::press(&mut app, KeyCode::PageDown);
    let paged = focus(&app);
    assert!(paged > 0, "PageDown did not move");
    support::press(&mut app, KeyCode::PageDown);
    assert!(focus(&app) >= paged, "PageDown wrapped");
    for _ in 0..3 {
        support::press(&mut app, KeyCode::PageUp);
    }
    assert_eq!(focus(&app), 0, "PageUp did not come back");
}

/// The row the focus is on is a row that is drawn.
///
/// The page is drawn into what the tabs and their rule leave off the top
/// and the foot and its rule leave off the bottom -- four rows fewer than
/// the editor -- and the window was settled against two of those four. So
/// the focus could walk two rows past the last one on screen before the
/// page moved under it, and for those two the reader was standing on a row
/// that was nowhere.
///
/// The keys page, because its rows are one row each: there "the row the
/// focus is on" and "a row of the page" are the same count, so a step is a
/// row and the arithmetic has nowhere to hide.
///
/// Deliberate break: `room.1.saturating_sub(2)` back in `settle_rows`, or
/// `settings_room` handing back the editor's own size.
#[test]
fn the_row_the_focus_is_on_is_a_row_that_is_drawn() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let scratch = temporary("focus-drawn");
    let file = settings_file(&scratch);
    let mut app = open(&file);
    support::press(&mut app, KeyCode::Tab);
    assert!(app.settings().expect("the settings").on_keys());

    // Past the foot of the first screenful and well into the second, which
    // is where a window settled against the wrong height leaves the focus
    // behind.
    for step in 0..16 {
        let dump = support::render(&mut app, 76, 14);
        let settings = app.settings().expect("the settings");
        let rows = settings.key_rows();
        let focused = rows[settings.focus().min(rows.len() - 1)].name();
        assert!(
            support::text_block(&dump).contains(focused),
            "step {step}: the focus is on {focused:?}, which is not on screen:\n{dump}"
        );
        support::press(&mut app, KeyCode::Down);
    }
}

/// The agents page is cards, and the same keys walk them: forty of them
/// scroll, the ends are reachable, and a page moves by the cards that fit
/// rather than by a number of rows.
#[test]
fn the_agents_page_is_a_list_of_cards() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let scratch = temporary("cards");
    let file = settings_file(&scratch);
    let mut app = open(&file);
    // Onto the agents tab, which is the last one.
    support::press(&mut app, KeyCode::BackTab);
    assert!(app.settings().expect("the settings").on_agents());

    let agents: Vec<obelus_agent::Agent> = (0..12)
        .map(|index| obelus_agent::Agent {
            id: format!("agent-{index}"),
            name: format!("Agent {index}"),
            version: "1.0.0".to_string(),
            description: "One of several".to_string(),
            authors: vec!["Somebody".to_string()],
            license: "MIT".to_string(),
            website: None,
            icon: None,
            distribution: obelus_agent::Distribution::Node {
                package: format!("agent-{index}@1.0.0"),
                arguments: Vec::new(),
            },
        })
        .collect();
    app.handle(Event::Agent(obelus_agent::Event::Registry {
        agents,
        failure: None,
    }));

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

/// And the cards have the bar every other list in Obelus has.
///
/// The page of settings drew one and the page of cards did not, so the one
/// page in Obelus whose rows are tallest and whose list is longest was the
/// one with nothing saying how much of it there was.
///
/// Both halves, because each passes with the other broken: a page with
/// more cards than screen has a bar down every row of it, and one with a
/// single card has none -- a track with no thumb on it is a control that
/// does not work.
///
/// Deliberate break: take the `scrollbar` call out, or drop the `total >
/// height` around it, or hand the cards the region's own width: a card
/// fills the row it is on, so it blanks the block the bar wrote there and
/// the page comes out with a hole in its bar.
///
/// What this asks of the bar is where it *starts* and that it has no hole
/// in it, rather than how long it is, because how long the page is is not
/// settled: the window is told the editor's height while the page is drawn
/// into what the tabs and the foot leave of it, two rows fewer -- so the
/// last card on screen is drawn a row or two past the rule over the foot,
/// and the page's body is not the bar's own extent. One question, and it
/// is not this one.
#[test]
fn the_cards_have_a_bar_where_there_is_somewhere_to_scroll() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let scratch = temporary("card-bar");
    let file = settings_file(&scratch);
    let mut app = open(&file);
    support::press(&mut app, KeyCode::BackTab);
    assert!(app.settings().expect("the settings").on_agents());

    let listed = |count: usize| -> Vec<obelus_agent::Agent> {
        (0..count)
            .map(|index| obelus_agent::Agent {
                id: format!("agent-{index}"),
                name: format!("Agent {index}"),
                version: "1.0.0".to_string(),
                description: "One of several".to_string(),
                authors: vec!["Somebody".to_string()],
                license: "MIT".to_string(),
                website: None,
                icon: None,
                distribution: obelus_agent::Distribution::Node {
                    package: format!("agent-{index}@1.0.0"),
                    arguments: Vec::new(),
                },
            })
            .collect()
    };

    app.handle(Event::Agent(obelus_agent::Event::Registry {
        agents: listed(12),
        failure: None,
    }));
    // Which rows of the page the bar is on.
    let barred = |dump: &str| -> Vec<bool> {
        body_of(dump)
            .iter()
            .map(|row| row.ends_with('\u{2588}'))
            .collect()
    };

    let dump = support::render(&mut app, 76, 16);
    let column = barred(&dump);
    assert!(column[0], "no bar at the top of the page:\n{dump}");
    let bar = column.iter().take_while(|on| **on).count();
    assert!(
        column[bar..].iter().all(|on| !on),
        "a hole in the bar:\n{dump}"
    );

    // And one card fits, so there is nothing to say.
    app.handle(Event::Agent(obelus_agent::Event::Registry {
        agents: listed(1),
        failure: None,
    }));
    let dump = support::render(&mut app, 76, 16);
    assert!(
        barred(&dump).iter().all(|on| !on),
        "a bar with nowhere to go:\n{dump}"
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
    let scratch = temporary("registry");
    let file = settings_file(&scratch);
    let mut app = open(&file);
    support::press(&mut app, KeyCode::BackTab);
    assert!(app.settings().expect("the settings").on_agents());

    // Nothing has arrived yet: the page says what it is doing.
    let dump = support::render(&mut app, 70, 12);
    assert!(
        support::text_block(&dump).contains("Fetching the list of agents"),
        "not the waiting page:\n{dump}"
    );
    assert_eq!(app.registry_failure(), None);

    // It failed, and there was nothing cached: the page says that instead,
    // because "fetching" would be a lie by now.
    app.handle(Event::Agent(obelus_agent::Event::Registry {
        agents: Vec::new(),
        failure: Some("dns error: no such host".to_string()),
    }));
    let dump = support::render(&mut app, 70, 12);
    assert!(
        support::text_block(&dump).contains("Could not fetch"),
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
    support::press(&mut app, KeyCode::BackTab);

    // An empty answer with nothing wrong leaves whatever list there is:
    // the cached one arriving after the fetched one must not wipe it.
    let one = obelus_agent::Agent {
        id: "one".to_string(),
        name: "The One".to_string(),
        version: "1.0.0".to_string(),
        description: "An agent".to_string(),
        authors: vec!["Someone".to_string()],
        license: "MIT".to_string(),
        website: None,
        icon: None,
        distribution: obelus_agent::Distribution::Node {
            package: "one@1.0.0".to_string(),
            arguments: Vec::new(),
        },
    };
    app.handle(Event::Agent(obelus_agent::Event::Registry {
        agents: vec![one.clone()],
        failure: None,
    }));
    app.handle(Event::Agent(obelus_agent::Event::Registry {
        agents: Vec::new(),
        failure: None,
    }));
    let dump = support::render(&mut app, 70, 12);
    assert!(
        support::text_block(&dump).contains("The One"),
        "an empty answer wiped the list:\n{dump}"
    );

    // And the list arriving later clears it, whichever visit fetched it.
    app.handle(Event::Agent(obelus_agent::Event::Registry {
        agents: vec![one],
        failure: None,
    }));
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
    let scratch = temporary("window");
    let file = settings_file(&scratch);
    let mut app = open(&file);
    support::press(&mut app, KeyCode::BackTab);

    let agents: Vec<obelus_agent::Agent> = (0..12)
        .map(|index| obelus_agent::Agent {
            id: format!("agent-{index}"),
            name: format!("Agent {index}"),
            version: "1.0.0".to_string(),
            description: "One of several".to_string(),
            authors: vec!["Somebody".to_string()],
            license: "MIT".to_string(),
            website: None,
            icon: None,
            distribution: obelus_agent::Distribution::Node {
                package: format!("agent-{index}@1.0.0"),
                arguments: Vec::new(),
            },
        })
        .collect();
    app.handle(Event::Agent(obelus_agent::Event::Registry {
        agents,
        failure: None,
    }));

    // A step, then the frame it produces: the window is settled against the
    // room the page has, which only a frame knows.
    let step = |app: &mut obelus_app::app::App, key: KeyCode| {
        support::press(app, key);
        let dump = support::render(app, 76, 19);
        (app.settings().expect("the settings").top(), dump)
    };

    // Every card is four rows, so three whole ones fit the thirteen this
    // screen leaves the page -- the tabs and their rule off the top, the
    // foot and its rule off the bottom -- counted by their last row,
    // because the fourth card's name is drawn in the row left over.
    // Asserted rather than assumed: the whole point is where the fourth
    // step lands.
    let dump = support::render(&mut app, 76, 19);
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

/// The first agent to install is the one Obelus talks to, and the next one
/// does not take its place.
///
/// The reader pressed the button on a machine with nothing active, so what
/// they want is that agent; a second install is not a request to be
/// switched over, and only one can be active at a time.
///
/// Driven by the event the installing thread sends rather than by pressing
/// the button, because pressing it runs `npm`.
#[test]
fn the_first_agent_installed_is_the_one_in_use() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let scratch = temporary("installed");
    let file = settings_file(&scratch);
    let mut app = open(&file);
    let root = file.with_file_name("agents");
    app.agents_root_for_test(root.clone());

    let agents: Vec<obelus_agent::Agent> = (0..2)
        .map(|index| obelus_agent::Agent {
            id: format!("agent-{index}"),
            name: format!("Agent {index}"),
            version: "1.0.0".to_string(),
            description: "One of two".to_string(),
            authors: vec!["Somebody".to_string()],
            license: "MIT".to_string(),
            website: None,
            icon: None,
            distribution: obelus_agent::Distribution::Node {
                package: format!("agent-{index}@1.0.0"),
                arguments: Vec::new(),
            },
        })
        .collect();
    app.handle(Event::Agent(obelus_agent::Event::Registry {
        agents,
        failure: None,
    }));
    assert_eq!(
        app.config().agent,
        None,
        "one was active before any install"
    );

    // An install that failed activates nothing. There is nothing to talk
    // to, and a card that says both "Failed" and "active" says nothing.
    app.handle(Event::Agent(obelus_agent::Event::Installed {
        id: "agent-0".to_string(),
        failure: Some("npm is not on the path".to_string()),
    }));
    assert_eq!(app.config().agent, None, "a failed install was activated");

    // Nor does one that says it worked and left no record behind. The
    // record is the proof, and this is the state a reader was stuck in
    // once: the settings named an agent, the card said "active", and
    // nothing could be started.
    app.handle(Event::Agent(obelus_agent::Event::Installed {
        id: "agent-0".to_string(),
        failure: None,
    }));
    assert_eq!(
        app.config().agent,
        None,
        "an agent with nothing installed under its name was activated"
    );

    // The first one that works becomes the one in use, and the file says so
    // -- a choice that is gone tomorrow was a preview rather than a choice.
    installed(&root, "agent-0", "1.0.0");
    app.handle(Event::Agent(obelus_agent::Event::Installed {
        id: "agent-0".to_string(),
        failure: None,
    }));
    assert_eq!(app.config().agent.as_deref(), Some("agent-0"));
    assert_eq!(
        obelus_config::from_toml(&std::fs::read_to_string(&file).expect("the file"))
            .agent
            .as_deref(),
        Some("agent-0"),
    );

    // And the next one does not take over.
    installed(&root, "agent-1", "1.0.0");
    app.handle(Event::Agent(obelus_agent::Event::Installed {
        id: "agent-1".to_string(),
        failure: None,
    }));
    assert_eq!(
        app.config().agent.as_deref(),
        Some("agent-0"),
        "the second install took over"
    );
}

/// What a finished install leaves behind: something to run, and the record
/// that says so.
fn installed(root: &std::path::Path, id: &str, version: &str) {
    let home = obelus_agent::home(id, root).expect("a directory for it");
    std::fs::create_dir_all(&home).expect("a directory");
    let program = home.join("run-me");
    std::fs::write(&program, "").expect("a program");
    obelus_agent::remember(id, &program, &[], version, root).expect("the record");
}

/// An agent the settings name and the machine does not have is not active,
/// whatever the settings say: the card offers to install it.
///
/// The state a reader was stuck in. Obelus decided an agent was installed by
/// looking at what `npm` had left lying about, and npm builds its project in an
/// order of its own -- so a run that was killed halfway left a directory
/// that looked finished. The card then read "active" over an agent nothing
/// could start, and the only button on it was the one that turned it off.
#[test]
fn an_agent_that_is_not_installed_is_not_in_use() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let scratch = temporary("stuck");
    let file = settings_file(&scratch);
    // The settings say an agent is in use, from some earlier session.
    std::fs::write(&file, "agent = \"agent-0\"\n").expect("the file");
    let mut app = open(&file);
    let root = file.with_file_name("agents");
    app.agents_root_for_test(root.clone());
    assert_eq!(app.config().agent.as_deref(), Some("agent-0"));

    // And npm's project is all there, exactly as an interrupted install leaves
    // it -- a manifest, and the link to the program.
    let home = obelus_agent::home("agent-0", &root).expect("a directory for it");
    let package = home.join("node_modules").join("agent-0");
    std::fs::create_dir_all(&package).expect("a directory");
    std::fs::write(
        package.join("package.json"),
        "{\"version\":\"1.0.0\",\"bin\":\"agent.js\"}",
    )
    .expect("a manifest");
    let binaries = home.join("node_modules").join(".bin");
    std::fs::create_dir_all(&binaries).expect("a directory");
    std::fs::write(binaries.join("agent-0"), "").expect("a program");

    support::press(&mut app, KeyCode::BackTab);
    app.handle(Event::Agent(obelus_agent::Event::Registry {
        agents: vec![obelus_agent::Agent {
            id: "agent-0".to_string(),
            name: "Agent 0".to_string(),
            version: "1.0.0".to_string(),
            description: "The one that got away".to_string(),
            authors: Vec::new(),
            license: String::new(),
            website: None,
            icon: None,
            distribution: obelus_agent::Distribution::Node {
                package: "agent-0@1.0.0".to_string(),
                arguments: Vec::new(),
            },
        }],
        failure: None,
    }));

    let dump = support::render(&mut app, 76, 16);
    let text = support::text_block(&dump);
    assert!(
        text.contains("Install"),
        "the card does not offer to install it:\n{dump}"
    );
    assert!(
        !text.contains("active"),
        "the card says it is in use:\n{dump}"
    );

    // And once the record is there, it is what it always said it was.
    installed(&root, "agent-0", "1.0.0");
    let dump = support::render(&mut app, 76, 16);
    assert!(
        support::text_block(&dump).contains("active"),
        "a finished install is not in use:\n{dump}"
    );
}

/// A command's key is moved on the keys page, and the table it changes is
/// the one Obelus is running on.
#[test]
fn a_command_can_be_put_on_another_key() {
    use crossterm::event::KeyModifiers;
    use obelus_command::Command;
    use obelus_editing::keymap::KeyChord;

    let _taken = SETTINGS.lock().expect("the lock");
    let scratch = temporary("bind");
    let file = settings_file(&scratch);
    let mut app = open(&file);
    // The keys tab, which is one across: settings, keys, agents.
    support::press(&mut app, KeyCode::Tab);
    support::type_text(&mut app, "choose-theme");
    let dump = support::render(&mut app, 66, 12);
    assert!(
        support::text_block(&dump).contains("choose-theme"),
        "the keys page does not list the commands:\n{dump}"
    );
    // It has no key at all, which is what makes it worth binding.
    assert_eq!(app.keymap().chord_for(Command::ThemeSelect), None);

    support::press(&mut app, KeyCode::Enter);
    let asking = support::render(&mut app, 66, 12);
    assert!(
        support::text_block(&asking).contains("Press a key"),
        "the row does not say what it is waiting for:\n{asking}"
    );

    support::press_alt_key(&mut app, KeyCode::Char('j'));
    assert_eq!(
        app.keymap().chord_for(Command::ThemeSelect),
        Some(KeyChord::new(KeyCode::Char('j'), KeyModifiers::ALT)),
        "the command is not on the key that was pressed"
    );
    // Written down, so it is still bound tomorrow.
    let written = std::fs::read_to_string(&file).expect("the file");
    assert!(
        // In the spelling the screen shows, which is the only one there
        // is: a file and a screen that wrote a name two ways would be two
        // names.
        written.contains("choose-theme") && written.contains("Alt+j"),
        "the binding is not in the file:\n{written}"
    );
    // And the row says so where it said "press a key".
    let bound = support::render(&mut app, 66, 12);
    assert!(
        support::text_block(&bound).contains("Change the colours"),
        "the row did not go back to saying what the command does:\n{bound}"
    );

    // Live, not merely stored: the key runs the command now.
    support::press(&mut app, KeyCode::Esc);
    support::press_alt_key(&mut app, KeyCode::Char('j'));
    assert!(
        app.picker()
            .is_some_and(|picker| picker.matches().any(|item| item.label.contains("dark"))),
        "the new key did not run the command"
    );
}

/// A key that already means something keeps meaning it, and the row says
/// what has it.
#[test]
fn a_key_that_is_taken_says_so_on_the_row() {
    use crossterm::event::KeyModifiers;
    use obelus_command::Command;

    let _taken = SETTINGS.lock().expect("the lock");
    let scratch = temporary("taken");
    let mut app = open(&settings_file(&scratch));
    support::press(&mut app, KeyCode::Tab);
    support::type_text(&mut app, "choose-theme");
    support::press(&mut app, KeyCode::Enter);

    // `ctrl+p` is the palette's, and it stays the palette's.
    support::press_control(&mut app, 'p');
    let dump = support::render(&mut app, 66, 12);
    assert!(
        support::text_block(&dump).contains("run-command"),
        "the row does not say what has the key:\n{dump}"
    );
    assert_eq!(app.keymap().chord_for(Command::ThemeSelect), None);
    assert_eq!(
        app.keymap().chord_for(Command::CommandPalette),
        Some(obelus_editing::keymap::KeyChord::new(
            KeyCode::Char('p'),
            KeyModifiers::CONTROL
        )),
        "the key was taken from the command that had it"
    );
    // Still waiting, so the reader can press another one.
    assert!(
        app.settings()
            .is_some_and(|settings| settings.binding() == Some(Command::ThemeSelect)),
        "the row gave up on the reader"
    );

    // Escape gives up on the row and not on the page: the nearest thing
    // first, like everywhere else.
    support::press(&mut app, KeyCode::Esc);
    assert!(
        app.settings()
            .is_some_and(|settings| settings.binding().is_none()),
        "escape did not leave the row"
    );
    assert!(app.settings().is_some(), "escape closed the whole page");
}

/// A key that could never fire is refused on the row, and the row goes on
/// waiting.
///
/// Every one of these would leave the reader with a binding that does
/// nothing: the arrows never reach the key table at all -- the editor takes
/// them -- the terminal sends tab for `ctrl+i` whatever was pressed, and a
/// bare letter is what typing will mean.
#[test]
fn a_key_that_could_never_fire_is_refused() {
    use crossterm::event::{KeyEvent, KeyModifiers};
    use obelus_app::event::Event;
    use obelus_command::Command;

    let _taken = SETTINGS.lock().expect("the lock");
    let scratch = temporary("never");
    let mut app = open(&settings_file(&scratch));
    support::press(&mut app, KeyCode::Tab);
    support::type_text(&mut app, "choose-theme");
    support::press(&mut app, KeyCode::Enter);

    for (code, modifiers, why) in [
        (KeyCode::Up, KeyModifiers::NONE, "editor"),
        (KeyCode::PageDown, KeyModifiers::NONE, "editor"),
        (KeyCode::Char('i'), KeyModifiers::CONTROL, "terminal"),
        (KeyCode::Char('z'), KeyModifiers::NONE, "Typing"),
        // The front of the reason, which is what a row this narrow has
        // room for once the key is spelled out rather than drawn.
        (
            KeyCode::Tab,
            KeyModifiers::NONE,
            "every list and box takes this",
        ),
    ] {
        app.handle(Event::Key(KeyEvent::new(code, modifiers)));
        let dump = support::render(&mut app, 66, 12);
        let said = support::text_block(&dump).to_lowercase();
        assert!(
            said.contains(&why.to_lowercase()),
            "the row does not say why {code:?} will not do:\n{dump}"
        );
        assert_eq!(
            app.keymap().chord_for(Command::ThemeSelect),
            None,
            "{code:?} was bound, and it could never fire"
        );
        assert!(
            app.settings()
                .is_some_and(|settings| settings.binding() == Some(Command::ThemeSelect)),
            "the row gave up on the reader after {code:?}"
        );
    }

    // And a key from one of the families is taken, from the same row: the
    // refusals did not leave it in a state where nothing works. `alt+z`,
    // because it only has to be a chord nothing has spoken for -- a test
    // that borrows an interesting key has to be rewritten the day that key
    // is earned, which is what `f12` did here.
    support::press_alt(&mut app, 'z');
    assert!(
        app.keymap().chord_for(Command::ThemeSelect).is_some(),
        "the row would not take a key it should"
    );
}

/// Delete takes a command's key away, which is a decision like any other.
#[test]
fn delete_takes_a_key_away() {
    use obelus_command::Command;

    let _taken = SETTINGS.lock().expect("the lock");
    let scratch = temporary("unbind");
    let file = settings_file(&scratch);
    let mut app = open(&file);
    support::press(&mut app, KeyCode::Tab);
    support::type_text(&mut app, "close-document");
    support::press(&mut app, KeyCode::Enter);
    support::press(&mut app, KeyCode::Delete);

    assert_eq!(app.keymap().chord_for(Command::DocumentClose), None);
    // Both of its bindings: the same command in the list of open files
    // closes the file on the row, and a reader who took its key away meant
    // both.
    assert!(
        !app.keymap()
            .bindings()
            .iter()
            .any(|binding| binding.command == Command::DocumentClose),
        "one of the command's keys survived"
    );
    let written = std::fs::read_to_string(&file).expect("the file");
    assert!(
        written.contains("close-document"),
        "the key taken away is not in the file:\n{written}"
    );
}

/// The settings file opens as a file, and is written first if it is not
/// there yet.
///
/// A reader sent to a path that does not exist has been told nothing. The
/// file Obelus would write is the answer to "what are the settings", and it
/// is what they need in front of them to change one by hand.
#[test]
fn the_settings_file_itself_can_be_read() {
    let _taken = SETTINGS.lock().expect("the lock");
    let scratch = temporary("file");
    let file = settings_file(&scratch);
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.config_file_for_test(file.clone());
    support::lay_out(&mut app, 66, 12);
    assert!(!file.exists(), "the file is there before anything asked");

    dispatch::dispatch(&mut app, Command::ConfigFile);
    assert!(file.exists(), "nothing was written to open");
    assert_eq!(
        app.current_buffer()
            .map(|buffer| buffer.path().to_path_buf()),
        Some(file.clone()),
        "the file being read is not the settings file"
    );
    // And what is on screen is the file, settings and all.
    let dump = support::render(&mut app, 66, 12);
    assert!(
        support::text_block(&dump).contains("theme"),
        "the settings are not on screen:\n{dump}"
    );
}

/// A setting changed in another Obelus is a setting changed here.
///
/// The terminal splits the window; Obelus does not. So several of them on
/// one project is the ordinary way to work, and a settings file read once at
/// startup would leave every other window holding what the reader has
/// already moved on from. The watcher says the file changed, and what is in
/// it is what Obelus is set to -- whichever process wrote it.
#[test]
fn a_setting_changed_by_another_obelus_arrives_here() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let scratch = temporary("shared");
    let file = settings_file(&scratch);
    std::fs::write(&file, "theme = \"light\"\n").expect("a settings file");
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.config_file_for_test(file.clone());
    support::lay_out(&mut app, 66, 12);
    assert_eq!(app.theme_name(), "light", "the file was not read");

    // Another Obelus writes the file. Nothing else says so: the watcher
    // hands over a path, and everything about what changed is in the file.
    std::fs::write(&file, "theme = \"dark\"\n").expect("the other window");
    app.handle(Event::Watched(obelus_watch::Changed { path: file }));
    assert_eq!(
        app.theme_name(),
        "dark",
        "the change in the other window never arrived"
    );
}

/// A settings file Obelus cannot read is one it will not write over.
///
/// What is in it is the reader's. A file caught mid-write by another Obelus,
/// or hand-edited into something that will not parse, used to read as "no
/// settings at all" -- and the next change in this window wrote the defaults
/// over everything that was in it.
#[test]
fn a_settings_file_that_will_not_read_is_not_written_over() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let scratch = temporary("broken");
    let file = settings_file(&scratch);
    let kept = "theme = \"light\"\nthis file is half written";
    std::fs::write(&file, kept).expect("a settings file");

    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.config_file_for_test(file.clone());
    support::lay_out(&mut app, 66, 12);
    app.handle(Event::Watched(obelus_watch::Changed { path: file.clone() }));

    // A change made here is not saved, and the reader is told why rather
    // than finding out later that their settings went.
    dispatch::dispatch(&mut app, Command::ThemeSelect);
    support::press(&mut app, KeyCode::Enter);
    assert_eq!(
        std::fs::read_to_string(&file).expect("the file"),
        kept,
        "Obelus wrote over a file it could not read"
    );
    let dump = support::render(&mut app, 66, 12);
    assert!(
        support::text_block(&dump).contains("Not saved"),
        "nothing said the change was not kept:\n{dump}"
    );

    // Fixed in the other window, it reads again and saves again. Another
    // theme, so the save has something to change: the one already in the
    // file saves as the file it already is.
    std::fs::write(&file, "theme = \"light\"\n").expect("the other window");
    app.handle(Event::Watched(obelus_watch::Changed { path: file.clone() }));
    dispatch::dispatch(&mut app, Command::ThemeSelect);
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);
    assert_ne!(
        std::fs::read_to_string(&file).expect("the file"),
        "theme = \"light\"\n",
        "it is still refusing to save a file it can read"
    );
}

#[test]
fn writing_the_settings_keeps_what_obelus_does_not_recognise() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let scratch = support::Scratch::new("settings-unknown");
    let file = scratch.path().join("config.toml");
    // A line from a newer Obelus, a setting that has been renamed since, and
    // a comment somebody wrote for themselves. None of it is Obelus's to
    // throw away on the next switch a reader flips.
    std::fs::write(
        &file,
        "# mine, do not eat\nfuture_setting = 3\nblame = false\nwrap = true\n",
    )
    .expect("writing the file");

    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.config_file_for_test(file.clone());
    support::lay_out(&mut app, 76, 16);
    // Any change at all: the file is written whole, and whole used to mean
    // only what Obelus knew about. Onto the reading tab and flip the first
    // switch on it.
    dispatch::dispatch(&mut app, Command::ConfigOpen);
    support::type_text(&mut app, "wrap");
    support::press(&mut app, KeyCode::Enter);

    let written = std::fs::read_to_string(&file).expect("reading it back");
    assert!(
        written.contains("future_setting = 3"),
        "a setting Obelus has never heard of was deleted:\n{written}"
    );
    assert!(
        written.contains("blame = false"),
        "a setting under a name Obelus has stopped using was deleted:\n{written}"
    );
    assert!(
        written.contains("# mine, do not eat"),
        "somebody's comment was deleted:\n{written}"
    );
    // Flipped back to the default, which is written down by taking the
    // line out: the file says what the reader has chosen, not what Obelus
    // would have chosen anyway.
    assert!(
        !obelus_config::from_toml(&written).wrap && !written.contains("wrap"),
        "the switch that was flipped was not written:\n{written}"
    );
}

/// The settings say what their keys do, because two of them cannot be
/// guessed: that the page is narrowed by typing at it, and that a setting
/// the project has set can be taken out again.
///
/// The card is keys alone, though. Typing at a list is not a key -- it
/// answers to every letter, which is why the foot writes it as the word
/// `type` -- and a table of chords read down its left column has no row
/// for one.
///
/// Deliberate break: let `keys_card` take every hint and the card grows a
/// row whose key is a word nobody can press.
#[test]
fn the_settings_say_what_their_keys_do() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let scratch = project("foot", "wrap = true\n");
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.working_directory_for_test(scratch.path().to_path_buf());
    support::lay_out(&mut app, 76, 16);
    dispatch::dispatch(&mut app, Command::ConfigProject);

    let text = support::text_block(&support::render(&mut app, 76, 16)).to_string();
    // "type" is capped as the key and "to filter" is what it does, so the
    // two are looked for apart. Escape is not here at all: it is on the
    // card, because it means the same thing in every view Obelus has.
    for word in ["Change", "type", "To filter", "Unset", "Keys"] {
        assert!(word_on(&text, word), "{word:?} is not at the foot:\n{text}");
    }

    // `ctrl+k` says all of them, at length.
    support::press_control(&mut app, 'k');
    let dump = support::render(&mut app, 76, 16);
    let text = support::text_block(&dump);
    assert!(text.contains("The keys here"), "no card:\n{dump}");
    assert!(
        text.contains("Take this setting out of the project's file"),
        "the card only has the foot's word for it:\n{dump}"
    );
    assert!(
        !text.contains("Type to narrow the list"),
        "the card of keys has a row that is not a key:\n{dump}"
    );

    // And escape closes the card before it leaves the page.
    support::press(&mut app, KeyCode::Esc);
    assert!(app.settings().is_some(), "escape left the settings");
}

/// `unset` is on the project's page and nowhere else, because the reader's own
/// settings have no "Unset" -- one they have not changed is the default.
#[test]
fn the_foot_offers_unset_only_where_it_means_something() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    support::lay_out(&mut app, 76, 16);
    dispatch::dispatch(&mut app, Command::ConfigOpen);
    let text = support::text_block(&support::render(&mut app, 76, 16)).to_string();
    assert!(
        !word_on(&text, "Unset"),
        "the reader's own page offered to unset something:\n{text}"
    );
}

/// Whether a word is on the foot, which is the last row that has anything.
fn word_on(text: &str, word: &str) -> bool {
    text.lines().any(|row| row.contains(word))
}

/// The rest that asks a question is a time, and the times are the few
/// anybody picks -- with zero among them, because "never" is one of the
/// answers people want and a switch beside the list would be a second way
/// to say the slowest one.
#[test]
fn the_time_a_rest_takes_is_the_readers() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let scratch = temporary("dwell");
    let file = settings_file(&scratch);
    let mut app = open(&file);

    // Typed for by name, which is how a reader reaches a setting on a
    // page of them -- and a check that it is on the page at all.
    support::type_text(&mut app, "hover");
    let settings = app.settings().expect("the view");
    assert_eq!(
        settings
            .rows(None)
            .iter()
            .filter_map(|row| Some(row.setting()?.key))
            .collect::<Vec<_>>(),
        ["hover_delay"],
        "the setting is not on the page, or not the only one that word finds"
    );

    support::press(&mut app, KeyCode::Enter);
    let picker = app.picker().expect("the choices");
    assert_eq!(
        picker
            .matches()
            .map(|item| item.label.clone())
            .collect::<Vec<_>>(),
        ["0", "200", "400", "800"],
        "not the times Obelus offers"
    );
    assert_eq!(
        picker.selected_item().map(|item| item.label.clone()),
        Some("400".to_string()),
        "the list did not open on the one in force"
    );

    support::type_text(&mut app, "800");
    support::press(&mut app, KeyCode::Enter);
    assert_eq!(
        obelus_config::from_toml(&std::fs::read_to_string(&file).expect("the file")).hover_delay,
        800,
        "the file does not say what was chosen"
    );
    assert_eq!(app.config().hover_delay, 800, "Obelus is not using it");
}

/// A press on a switch flips it, and a press on the row it is on does not.
///
/// The settings are drawn over the file being read, and until now a press
/// anywhere in them did nothing at all: one line turned the pointer away
/// before every view drawn over a file. The wheel always reached them, which
/// is the shape of the omission.
///
/// Nothing on this page folds, so there is no arrow. What a press reaches
/// is the switch -- a box with a tick in it or without, which is the one
/// thing here that says by its shape that pressing it changes it. A press
/// on the name beside it moves the selection and no more: a setting is not
/// something to change by aiming badly.
///
/// Broken deliberately by handing the press back to nothing, which leaves
/// the switch as it was; or by flipping on a press anywhere in the row,
/// which changes a setting the reader only meant to read.
#[test]
fn a_press_on_a_switch_flips_it() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let scratch = temporary("switch-press");
    let file = settings_file(&scratch);
    let mut app = open(&file);
    let _ = support::render(&mut app, 66, 12);

    // The row the switch is on, found by the words beside it. Every row of
    // the dump says which of the screen's rows it is, which is the number
    // this wants: the block has a line of its own in front of them.
    let dump = support::render(&mut app, 66, 12);
    let y: u16 = support::text_block(&dump)
        .lines()
        .find(|row| row.contains("Nerd Font"))
        .and_then(|row| row.split_once('|'))
        .and_then(|(at, _)| at.trim().parse().ok())
        .expect("the row with the switch on it");
    let area = app.editor_area_for_test();
    assert!(
        y >= area.y && y < area.bottom(),
        "the switch is not in the region the settings are drawn in"
    );

    // A press on the name moves the selection there and leaves the setting
    // alone.
    let was = app.config().icons;
    app.handle(Event::Pointer {
        kind: obelus_app::event::Pointer::Pressed,
        x: area.x + 4,
        y,
    });
    assert_eq!(
        app.config().icons,
        was,
        "a press on the name flipped the switch"
    );

    // And a press on the switch itself flips it.
    app.handle(Event::Pointer {
        kind: obelus_app::event::Pointer::Pressed,
        x: area.right() - 4,
        y,
    });
    assert_ne!(
        app.config().icons,
        was,
        "a press on the switch did not flip it"
    );

    obelus_icons::use_glyphs(false);
}

/// A press on a tab goes to it.
///
/// Every view with tabs draws them with one function, and none of them took
/// a press: the rows of a list start below the tabs, so a press on one fell
/// outside everything the pointer knew about. A tab is the most press-shaped
/// thing on a screen and it is the only thing a tab is for -- there is
/// nothing else a press there could have meant.
///
/// Walked rather than jumped, by the shorter way round. What a tab *costs*
/// is the application's -- a scope asks the search again, a radius walks
/// the history again -- so the press goes down the key's own path; and the
/// tabs wrap, so the short way is at most one step for every list Obelus
/// has, which is what keeps a tab in between from being asked its question
/// on the way past.
///
/// Broken deliberately by letting the press fall through, which leaves the
/// page on the tab it was on.
#[test]
fn a_press_on_a_tab_goes_to_it() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let scratch = temporary("tab-press");
    let file = settings_file(&scratch);
    let mut app = open(&file);
    let dump = support::render(&mut app, 66, 12);

    // The tabs are the page's first row, and the third of them is two away
    // -- which the shorter way round makes one step, because they wrap.
    let names = app.settings().expect("the page").tabs();
    assert_eq!(names.len(), 3, "the page does not have the three tabs");
    assert_eq!(app.settings().expect("the page").tab(), 0);
    let row = support::text_block(&dump)
        .lines()
        .find(|row| row.contains(names[2]))
        .and_then(|row| row.split_once('|'))
        .and_then(|(at, _)| at.trim().parse::<u16>().ok())
        .expect("the tab row");
    let x = support::column_of(
        support::text_block(&dump)
            .lines()
            .find(|row| row.contains(names[2]))
            .expect("the tab row"),
        names[2],
    );

    app.handle(Event::Pointer {
        kind: obelus_app::event::Pointer::Pressed,
        x: u16::try_from(x).expect("a column"),
        y: row,
    });
    assert_eq!(
        app.settings().expect("the page").tab(),
        2,
        "the press did not go to the tab"
    );

    obelus_icons::use_glyphs(false);
}

/// One of the agent's settings, as an agent sends it.
fn offered(id: &str, name: &str, values: &[(&str, &str)]) -> obelus_agent::acp::Setting {
    obelus_agent::acp::Setting {
        id: id.to_string(),
        name: name.to_string(),
        about: Some(format!("What {name} does")),
        values: values
            .iter()
            .map(|(id, name)| obelus_agent::acp::Value {
                id: (*id).to_string(),
                name: (*name).to_string(),
                about: None,
            })
            .collect(),
        current: values
            .first()
            .map(|(id, _)| (*id).to_string())
            .unwrap_or_default(),
        kind: obelus_agent::acp::Kind::Select,
        category: obelus_agent::acp::Category::Other,
        legacy: false,
    }
}

/// An application whose settings name an agent, on the settings page, as it
/// is once the agent has said what it offers.
fn with_an_agent(name: &str, offers: &[obelus_agent::acp::Setting]) -> (support::Scratch, App) {
    with_an_agent_set(name, offers, "")
}

/// The same, with more in the settings file than the agent's name.
///
/// Nothing is installed, so opening the page asks nobody: what the agent
/// offers is handed over the way its answer would be.
fn with_an_agent_set(
    name: &str,
    offers: &[obelus_agent::acp::Setting],
    set: &str,
) -> (support::Scratch, App) {
    let scratch = temporary(name);
    let file = settings_file(&scratch);
    std::fs::write(&file, format!("agent = \"an-agent\"\n{set}")).expect("the settings");
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.agents_root_for_test(scratch.path().join("agents"));
    // Which reads it: `load_config` would go looking at the reader's own
    // real path, which is not this test's to read.
    app.config_file_for_test(file);
    support::lay_out(&mut app, 66, 12);
    dispatch::dispatch(&mut app, Command::ConfigOpen);
    if !offers.is_empty() {
        app.agent_offers_for_test("an-agent", offers.to_vec());
    }
    (scratch, app)
}

/// The active agent's settings are a group on the settings page, under a
/// heading saying when what is in it takes effect.
///
/// A group and not a tab of its own: they are the same shape as Obelus's
/// own -- a name, a line about it, one control -- and a reader looking for
/// the one about thinking should not have to guess which page it is filed
/// under. The heading carries the one thing that is true of this group and
/// no other: these are about the next conversation rather than the one on
/// screen.
///
/// Broken deliberately by returning before the agent's rows are added in
/// `Settings::rows`: the page had the heading nowhere and the filter found
/// nothing.
#[test]
fn the_active_agents_settings_are_a_group_on_the_page() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (_scratch, mut app) = with_an_agent(
        "agent-group",
        &[offered(
            "way",
            "Way of working",
            &[("ask", "Ask first"), ("code", "Write code")],
        )],
    );

    // Narrowed to it, because the agent's group is last and Obelus's own
    // settings are a page and a half on a twelve-row screen.
    support::type_text(&mut app, "way");
    let dump = support::render(&mut app, 66, 12);
    let text = support::text_block(&dump);
    assert!(
        text.contains("an-agent"),
        "no heading for the agent:\n{dump}"
    );
    assert!(
        text.contains("What a new conversation starts on"),
        "the heading does not say when it takes effect:\n{dump}"
    );
    assert!(text.contains("Way of working"), "no row for it:\n{dump}");
    // Nothing chosen yet, which is a state of its own and not a blank.
    assert!(
        text.contains("Agent's own"),
        "the control does not say the agent decides:\n{dump}"
    );
}

/// An agent that has not said what it can be set to says so.
///
/// Not answered is not the same as nothing to answer: a group that was
/// simply not drawn would say the agent has nothing to be set, which is a
/// claim about the agent, and false. The other silence -- still asking --
/// is the agent tests', which have an agent to ask.
///
/// Broken deliberately by giving `agent_offering` no `silence` and letting
/// the group disappear: the page said nothing at all, and an agent with
/// settings looked exactly like one without.
#[test]
fn an_agent_that_has_not_said_what_it_offers_says_so_rather_than_nothing() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (_scratch, mut app) = with_an_agent("agent-silence", &[]);

    // The group is last, so the end of the page is where it is.
    support::press(&mut app, KeyCode::End);
    let dump = support::render(&mut app, 66, 12);
    assert!(
        support::text_block(&dump).contains("Nothing has been heard"),
        "an agent that has said nothing says nothing:\n{dump}"
    );
}

/// A choice the agent no longer offers says so on its row, in words and in
/// the colour of something that will not work -- and takes the rows it
/// needs, so the next entry is not drawn over it.
///
/// Deliberate break: answer `None` from `Shown::warning` for an agent's row
/// and the words are gone. Leave the warning out of `own_rows` in the
/// drawing, and the next entry is laid out over it. Leaving it out of
/// `Settings::setting_rows` instead is the next test's.
#[test]
fn a_choice_the_agent_no_longer_offers_says_so_on_its_row() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (_scratch, mut app) = with_an_agent_set(
        "agent-gone",
        &[
            offered(
                "way",
                "Way of working",
                &[("ask", "Ask first"), ("code", "Write code")],
            ),
            offered("thinking", "Way of thinking", &[("fast", "Fast")]),
        ],
        "[agents.an-agent]\nway = \"gone\"\n",
    );

    // Narrowed to the two, so that both are on a page this short.
    support::type_text(&mut app, "way of");
    let cells = support::cells_of(&mut app, 66, 16);
    let screen: Vec<String> = (0..16)
        .map(|y| (0..66).map(|x| cells[(x, y)].symbol()).collect::<String>())
        .collect();
    let dump = screen.join("\n");
    let at = |needle: &str| {
        screen
            .iter()
            .position(|row| row.contains(needle))
            .unwrap_or_else(|| panic!("{needle:?} is not on screen:\n{dump}"))
    };
    // Wrapped to the room there is, so the first of its rows.
    let warned = at("No longer offered by an-agent, so a new conversation");
    // Its own rows, all of them, below what the setting does and above the
    // next name.
    let ends = at("starts on its own");
    let next = screen
        .iter()
        .position(|row| row.contains("Way of thinking") && !row.contains("What"))
        .unwrap_or_else(|| panic!("the next setting is not on screen:\n{dump}"));
    assert!(
        at("What Way of working does") < warned && warned < ends && ends < next,
        "the warning is not between what it does and the next setting:\n{dump}"
    );
    // And in the colour a value that will not work is drawn in.
    let column = screen[warned]
        .find("No longer")
        .map(|byte| screen[warned][..byte].chars().count())
        .expect("the words");
    let row = u16::try_from(warned).expect("a row");
    let column = u16::try_from(column).expect("a column");
    assert_eq!(
        cells[(column, row)].fg,
        app.theme().change_removed,
        "the warning is not in the colour of something that will not work:\n{dump}"
    );
}

/// A warning is counted where the page decides what is on screen, and not
/// only where it is drawn.
///
/// Two walks that must agree: the window settles which entries are showing
/// by `Settings::setting_rows`, and the drawing lays them out by the rows
/// it made. Four choices the agent no longer offers are four entries two
/// rows taller than they look without their warnings -- so on a page this
/// short, one of the walks counting them and the other not is the last of
/// them pushed off the bottom while the reader is standing on it.
///
/// Deliberate break: leave the warning out of `Settings::setting_rows`.
/// The window thinks the last entry fits where it does not, and its name
/// is not on screen.
#[test]
fn a_warning_is_counted_where_the_page_decides_what_is_on_it() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let ways = ["one", "two", "three", "four"];
    let offers: Vec<_> = ways
        .iter()
        .map(|way| offered(way, &format!("Way {way}"), &[("ask", "Ask first")]))
        .collect();
    let set: String = ways
        .iter()
        .map(|way| format!("{way} = \"gone\"\n"))
        .collect();
    let (_scratch, mut app) = with_an_agent_set(
        "agent-gone-counted",
        &offers,
        &format!("[agents.an-agent]\n{set}"),
    );

    support::type_text(&mut app, "way ");
    support::press(&mut app, KeyCode::End);
    let dump = support::render(&mut app, 66, 16);
    assert!(
        support::text_block(&dump).contains("Way four"),
        "the entry the reader is on is not on screen:\n{dump}"
    );
}

/// Choosing what a conversation starts on writes it in the reader's own
/// file, and `delete` puts it back in the agent's hands.
///
/// The third state is the point: a row nobody has touched is not a row set
/// to the first of its values, and the way back to it has to exist or the
/// reader cannot undo what they said.
///
/// Broken deliberately by making `UnsetForAgent` write the value instead of
/// removing it: the row went on naming a value and the file kept it.
#[test]
fn choosing_what_a_conversation_starts_on_is_written_down_and_can_be_undone() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (scratch, mut app) = with_an_agent(
        "agent-choose",
        &[offered(
            "way",
            "Way of working",
            &[("ask", "Ask first"), ("code", "Write code")],
        )],
    );
    let file = settings_file(&scratch);

    support::type_text(&mut app, "way");
    support::press(&mut app, KeyCode::Enter);
    let picker = app.picker().expect("the choices");
    assert_eq!(
        picker
            .matches()
            .map(|item| item.label.clone())
            .collect::<Vec<_>>(),
        ["Agent's own", "Ask first", "Write code"],
        "the agent's own answer is not one of the choices"
    );
    assert_eq!(
        picker.selected_item().map(|item| item.label.clone()),
        Some("Agent's own".to_string()),
        "the list did not open on what the row is on"
    );

    support::type_text(&mut app, "Write");
    support::press(&mut app, KeyCode::Enter);
    assert_eq!(
        obelus_config::from_toml(&std::fs::read_to_string(&file).expect("the file"))
            .agent_default("an-agent", "way"),
        Some("code"),
        "the choice did not reach the file"
    );
    let dump = support::render(&mut app, 66, 12);
    assert!(
        support::text_block(&dump).contains("Write code"),
        "the row does not say what was chosen:\n{dump}"
    );

    // And back again.
    support::press(&mut app, KeyCode::Delete);
    assert_eq!(
        obelus_config::from_toml(&std::fs::read_to_string(&file).expect("the file"))
            .agent_default("an-agent", "way"),
        None,
        "delete left the setting in the file"
    );
    let dump = support::render(&mut app, 66, 12);
    assert!(
        support::text_block(&dump).contains("Agent's own"),
        "the row did not go back to the agent:\n{dump}"
    );
}

/// The project's page has no group for the agent.
///
/// What an agent starts on is the reader's alone, for the reason the agent
/// itself is: a downloaded project that could write it could say that the
/// agent it starts may edit files without being asked. So the group is not
/// there to be pressed -- rather than there and refusing, which is a page
/// that has to be tried before it can be understood.
///
/// Broken deliberately by dropping the `on_project` filter in
/// `Settings::rows`: the group appeared on the project's page, where every
/// other row writes the project's file and this one would have written the
/// reader's.
#[test]
fn the_trees_page_has_no_group_for_the_agent() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (_scratch, mut app) = with_an_agent(
        "agent-project",
        &[offered(
            "way",
            "Way of working",
            &[("ask", "Ask first"), ("code", "Write code")],
        )],
    );
    // It is on the reader's page, so this test is about the other one.
    support::type_text(&mut app, "way");
    let dump = support::render(&mut app, 66, 12);
    assert!(
        support::text_block(&dump).contains("Way of working"),
        "the group is not on the reader's page either:\n{dump}"
    );

    dispatch::dispatch(&mut app, Command::ConfigProject);
    assert!(app.settings().expect("the settings").on_project());
    support::type_text(&mut app, "way");
    let dump = support::render(&mut app, 66, 12);
    assert!(
        !support::text_block(&dump).contains("Way of working"),
        "the agent's settings are on the project's page:\n{dump}"
    );
}

/// The project's page has one tab, because the other two go nowhere.
///
/// Which keys a reader is on and which agent Obelus talks to are the
/// reader's alone -- a downloaded project that could set either would be
/// starting programs and moving `quit` under somebody's fingers. So on this
/// page those two were tabs that said, when reached, that a project may not
/// set them.
///
/// And the rows behind that sentence were still there. The agents page drew
/// its cards over the sentence -- the branch that draws them came first and
/// returned -- so the project's page showed every agent in the registry with
/// a button on each; and on the keys page the focus walked a list nobody
/// could see. Enter on either wrote the reader's own file from the page
/// that is about the project's.
///
/// A tab is for somewhere else to go, and one that goes nowhere is a tab
/// that lies. The foot of every view in Obelus already follows this rule:
/// it lists the keys that do something here and keeps the rest for `F1`.
///
/// Broken deliberately by giving `Page::of` every page whoever is asking:
/// the tabs came back, `tab` walked onto the agents page, and enter there
/// activated whichever agent the invisible focus was on.
#[test]
fn the_projects_page_has_one_tab() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (_scratch, mut app) = with_an_agent("project-tabs", &[]);
    // Something for an invisible focus to land on, if there were one.
    app.handle(Event::Agent(obelus_agent::Event::Registry {
        agents: vec![obelus_agent::Agent {
            id: "another-agent".to_string(),
            name: "Another".to_string(),
            version: "1.0.0".to_string(),
            description: "Not the one in use".to_string(),
            authors: Vec::new(),
            license: "MIT".to_string(),
            website: None,
            icon: None,
            distribution: obelus_agent::Distribution::Node {
                package: "another@1.0.0".to_string(),
                arguments: Vec::new(),
            },
        }],
        failure: None,
    }));

    // The reader's own page has three.
    assert_eq!(
        app.settings().expect("the settings").tabs(),
        ["Settings", "Keys", "Agents"]
    );

    dispatch::dispatch(&mut app, Command::ConfigProject);
    assert_eq!(
        app.settings().expect("the settings").tabs(),
        ["Settings"],
        "the project's page offers somewhere it cannot go"
    );
    let dump = support::render(&mut app, 76, 12);
    let tabs = support::text_block(&dump)
        .lines()
        .nth(1)
        .unwrap_or_default()
        .to_string();
    assert!(
        !tabs.contains("Keys") && !tabs.contains("Agents"),
        "the tab row still offers them:\n{dump}"
    );

    // And the keys that walked the tabs stay on the one page there is,
    // so nothing can be reached from it.
    for _ in 0..3 {
        support::press(&mut app, KeyCode::BackTab);
        support::press(&mut app, KeyCode::Tab);
    }
    let settings = app.settings().expect("the settings");
    assert!(
        !settings.on_keys() && !settings.on_agents(),
        "a tab walk reached a page the project has not got"
    );

    // Which is what keeps the reader's own file out of this page's reach:
    // installing an agent, choosing one and moving a key all write it.
    support::press(&mut app, KeyCode::Enter);
    assert_eq!(
        app.config().agent.as_deref(),
        Some("an-agent"),
        "the project's page changed which agent Obelus talks to"
    );
}

/// Walking the cards rewrites marks, and a frame that changed nothing
/// does not.
///
/// Two different reasons for the first, and both count: a scroll puts
/// every mark on a new row, and a step that scrolls nothing still rewrites
/// two of them, because a mark is drawn onto the background it will sit on
/// and the focused card's background is not the others'.
///
/// This is what decides whether the frame is written with the caret put
/// out. Getting it wrong is visible either way: a frame that hides and
/// shows the caret without needing to is a caret that blinks, and a frame
/// that writes a sixel without hiding it draws the picture under a caret
/// the reader can see.
#[test]
fn walking_the_cards_rewrites_marks_and_a_still_frame_does_not() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let scratch = temporary("marks-move");
    let file = settings_file(&scratch);
    let mut app = open(&file);
    support::press(&mut app, KeyCode::BackTab);
    assert!(app.settings().expect("the settings").on_agents());

    let agents: Vec<obelus_agent::Agent> = (0..12)
        .map(|index| obelus_agent::Agent {
            id: format!("agent-{index}"),
            name: format!("Agent {index}"),
            version: "1.0.0".to_string(),
            description: "One of several".to_string(),
            authors: vec!["Somebody".to_string()],
            license: "MIT".to_string(),
            website: None,
            icon: None,
            distribution: obelus_agent::Distribution::Node {
                package: format!("agent-{index}@1.0.0"),
                arguments: Vec::new(),
            },
        })
        .collect();
    app.handle(Event::Agent(obelus_agent::Event::Registry {
        agents,
        failure: None,
    }));

    let screen = ratatui::layout::Rect::new(0, 0, 76, 16);
    support::render(&mut app, screen.width, screen.height);
    let editor = obelus_ui::editor_room(screen, &app);

    // The page arriving is a page to write, whatever it holds.
    assert!(
        app.picture_moved_for_test(editor),
        "the first page of cards was not a page to write"
    );
    // And asked again with nothing touched, it says so -- which is the
    // half that keeps the caret from blinking on every frame that is not
    // about the cards.
    assert!(
        !app.picture_moved_for_test(editor),
        "a frame that changed nothing still wanted the caret put out"
    );

    // A step inside the window. Nothing scrolls, and two marks are still
    // rewritten: the card the focus left and the card it reached, each on
    // the background it now sits on.
    support::press(&mut app, KeyCode::Down);
    let before = app.settings().expect("the settings").top();
    assert!(
        app.picture_moved_for_test(editor),
        "the focus stepped onto a card without that card's mark being redrawn"
    );
    assert_eq!(
        app.settings().expect("the settings").top(),
        before,
        "this step was meant to be one the window does not follow"
    );
    // And asked again with the focus where it was left, it is still.
    assert!(
        !app.picture_moved_for_test(editor),
        "a frame after the step still wanted the caret put out"
    );

    // And the end of the list, which the window has to follow: every mark
    // on the page lands on a new row.
    support::press(&mut app, KeyCode::End);
    assert!(
        app.picture_moved_for_test(editor),
        "the window scrolled to the last card without moving a mark"
    );
    assert_ne!(
        app.settings().expect("the settings").top(),
        before,
        "End was meant to scroll the window"
    );
}

/// An install reports progress several times a second and moves no mark,
/// so none of those frames is written with the caret put out.
#[test]
fn install_progress_moves_no_mark() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let scratch = temporary("marks-installing");
    let file = settings_file(&scratch);
    let mut app = open(&file);
    support::press(&mut app, KeyCode::BackTab);

    app.handle(Event::Agent(obelus_agent::Event::Registry {
        agents: vec![obelus_agent::Agent {
            id: "agent-0".to_string(),
            name: "Agent 0".to_string(),
            version: "1.0.0".to_string(),
            description: "The only one".to_string(),
            authors: vec!["Somebody".to_string()],
            license: "MIT".to_string(),
            website: None,
            icon: None,
            distribution: obelus_agent::Distribution::Node {
                package: "agent-0@1.0.0".to_string(),
                arguments: Vec::new(),
            },
        }],
        failure: None,
    }));

    let screen = ratatui::layout::Rect::new(0, 0, 76, 16);
    support::render(&mut app, screen.width, screen.height);
    let editor = obelus_ui::editor_room(screen, &app);
    assert!(app.picture_moved_for_test(editor), "no first page");

    for done in 1..4 {
        app.handle(Event::Agent(obelus_agent::Event::Installing {
            id: "agent-0".to_string(),
            progress: obelus_agent::install::Progress {
                done,
                total: Some(4),
                elapsed: std::time::Duration::from_secs(done),
            },
        }));
        assert!(
            !app.picture_moved_for_test(editor),
            "install progress asked for the caret to be put out"
        );
    }
}

/// An install that has said nothing yet turns a mark on its card, and the
/// screen is woken to turn it for exactly as long as the install runs.
///
/// `npm` says nothing until it is done, so a card with no progress is the
/// one this is for: a mark standing still there reads as an install that
/// stopped. Checked by dropping the clause from `wants_animating` (the
/// first `is_waking` fails), by drawing the words without the mark (the
/// mark is not in front of them), and by handing the view a phase of 0
/// rather than the app's (the mark does not move with the clock).
#[test]
fn an_install_turns_its_mark() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let scratch = temporary("install-turns");
    let file = settings_file(&scratch);
    let mut app = open(&file);
    support::press(&mut app, KeyCode::BackTab);

    app.handle(one_agent());
    support::render(&mut app, 76, 16);
    assert!(!app.is_waking(), "something was already waking the screen");

    app.handle(Event::Agent(obelus_agent::Event::Installing {
        id: "agent-0".to_string(),
        progress: obelus_agent::install::Progress {
            done: 0,
            total: None,
            elapsed: std::time::Duration::ZERO,
        },
    }));
    let first = support::render(&mut app, 76, 16);
    assert!(
        app.is_waking(),
        "the mark turns and nothing is waking the screen to turn it"
    );
    let row = first
        .lines()
        .find(|row| row.contains("Installing"))
        .expect("the card says it is installing");
    assert_eq!(
        support::glyph_before(row, "Installing"),
        obelus_ui::spinning(0),
        "no turning mark in front of the words: {row}"
    );

    app.phase_for_test(1);
    let next = support::render(&mut app, 76, 16);
    let row = next
        .lines()
        .find(|row| row.contains("Installing"))
        .expect("the card says it is installing");
    assert_eq!(
        support::glyph_before(row, "Installing"),
        obelus_ui::spinning(1),
        "the mark did not move with the clock: {row}"
    );

    app.handle(Event::Agent(obelus_agent::Event::Installed {
        id: "agent-0".to_string(),
        failure: Some("npm went away".to_string()),
    }));
    support::render(&mut app, 76, 16);
    assert!(
        !app.is_waking(),
        "the screen is still being woken with nothing moving on it"
    );
}

/// An update is an install over an older version, turns the same mark, and
/// says it is an update while it runs.
///
/// The card offered `Update` and the facts said which two versions, so a
/// card that said `Installing` the moment it was pressed would read as an
/// agent that had never been there. Checked by putting `Outdated` before
/// `Installing` where an agent's status is decided (the card goes on
/// offering the button and nothing turns), by leaving `replacing` empty
/// there (the card says `Installing`), and by taking `Installing` out of
/// the arm that writes the two versions (they go).
#[test]
fn an_update_turns_its_mark() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let scratch = temporary("update-turns");
    let file = settings_file(&scratch);
    let mut app = open(&file);
    let root = file.with_file_name("agents");
    app.agents_root_for_test(root.clone());
    installed(&root, "agent-0", "0.9.0");
    support::press(&mut app, KeyCode::BackTab);

    app.handle(one_agent());
    let before = support::render(&mut app, 76, 16);
    assert!(
        before.contains("Update \u{25b8}"),
        "the card was not offering an update:\n{before}"
    );

    app.handle(Event::Agent(obelus_agent::Event::Installing {
        id: "agent-0".to_string(),
        progress: obelus_agent::install::Progress {
            done: 0,
            total: None,
            elapsed: std::time::Duration::ZERO,
        },
    }));
    let during = support::render(&mut app, 76, 16);
    assert!(
        app.is_waking(),
        "the mark turns and nothing is waking the screen to turn it"
    );
    let row = during
        .lines()
        .find(|row| row.contains("Updating"))
        .unwrap_or_else(|| panic!("the card does not say it is updating:\n{during}"));
    assert_eq!(
        support::glyph_before(row, "Updating"),
        obelus_ui::spinning(0),
        "no turning mark in front of the words: {row}"
    );
    assert!(
        during.contains("0.9.0 \u{2192} 1.0.0"),
        "the card forgot which two versions the update is between:\n{during}"
    );
}

/// A registry of one agent, which installs with `npm`.
fn one_agent() -> Event {
    Event::Agent(obelus_agent::Event::Registry {
        agents: vec![obelus_agent::Agent {
            id: "agent-0".to_string(),
            name: "Agent 0".to_string(),
            version: "1.0.0".to_string(),
            description: "The only one".to_string(),
            authors: vec!["Somebody".to_string()],
            license: "MIT".to_string(),
            website: None,
            icon: None,
            distribution: obelus_agent::Distribution::Node {
                package: "agent-0@1.0.0".to_string(),
                arguments: Vec::new(),
            },
        }],
        failure: None,
    })
}

/// The marks arrive one at a time, an event each, and the frame one lands
/// on writes a picture into cells that had a glyph in them.
///
/// Nothing else about the page has moved on that frame -- same cards, same
/// window, same room -- so a layout that watched only where the cards were
/// would let that picture be written under a visible caret.
#[test]
fn a_mark_arriving_is_a_picture_to_write() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let scratch = temporary("marks-arriving");
    let file = settings_file(&scratch);
    let mut app = open(&file);
    support::press(&mut app, KeyCode::BackTab);

    let agents: Vec<obelus_agent::Agent> = (0..3)
        .map(|index| obelus_agent::Agent {
            id: format!("agent-{index}"),
            name: format!("Agent {index}"),
            version: "1.0.0".to_string(),
            description: "One of several".to_string(),
            authors: vec!["Somebody".to_string()],
            license: "MIT".to_string(),
            website: None,
            icon: None,
            distribution: obelus_agent::Distribution::Node {
                package: format!("agent-{index}@1.0.0"),
                arguments: Vec::new(),
            },
        })
        .collect();
    app.handle(Event::Agent(obelus_agent::Event::Registry {
        agents,
        failure: None,
    }));

    let screen = ratatui::layout::Rect::new(0, 0, 76, 16);
    support::render(&mut app, screen.width, screen.height);
    let editor = obelus_ui::editor_room(screen, &app);
    assert!(app.picture_moved_for_test(editor), "no first page");
    assert!(
        !app.picture_moved_for_test(editor),
        "a page that changed nothing still wanted the caret put out"
    );

    // Each mark in turn, and every one of them is a frame to write --
    // including the second and the third, which is where a page that
    // watched only its cards stopped noticing.
    for index in 0..3 {
        app.handle(Event::Agent(obelus_agent::Event::Icon {
            id: format!("agent-{index}"),
            svg: "<svg viewBox=\"0 0 16 16\"><rect width=\"16\" height=\"16\"/></svg>".to_string(),
        }));
        assert!(
            app.picture_moved_for_test(editor),
            "the mark for agent-{index} arrived without a picture to write"
        );
    }
}

/// A filter nobody has typed into has no caret, and one that has been
/// typed into does.
///
/// Two marks for one fact is the reason -- the row the reader is on says
/// where the keys are going -- and the agents page is what makes it matter
/// beyond the look of it: a terminal is handed a picture by writing it at
/// the caret, so a caret sat in an empty box has to be put out and brought
/// back every time a scrolled row moves the marks.
#[test]
fn an_empty_filter_has_no_caret() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let scratch = temporary("empty-filter");
    let file = settings_file(&scratch);
    let mut app = open(&file);

    let screen = ratatui::layout::Rect::new(0, 0, 76, 16);
    support::render(&mut app, screen.width, screen.height);
    assert!(
        obelus_ui::cursor_position(screen, &app).is_none(),
        "the settings opened with a caret in a filter nobody had typed into"
    );

    // And the agents tab, which is the one that draws pictures.
    support::press(&mut app, KeyCode::BackTab);
    support::render(&mut app, screen.width, screen.height);
    assert!(
        app.settings().expect("the settings").on_agents(),
        "not on the agents tab"
    );
    assert!(
        obelus_ui::cursor_position(screen, &app).is_none(),
        "the agents page put a caret in an empty filter"
    );

    // Typing is what asks for one, and it lands after what was typed.
    support::type_text(&mut app, "ag");
    support::render(&mut app, screen.width, screen.height);
    let caret = obelus_ui::cursor_position(screen, &app).expect("no caret to type at");
    assert_eq!(
        caret.y,
        screen.height - 1,
        "the filter's caret left the status row"
    );

    // And taking it back out takes the caret with it.
    support::press(&mut app, KeyCode::Backspace);
    support::press(&mut app, KeyCode::Backspace);
    assert!(
        app.settings().expect("the settings").query().is_empty(),
        "the filter still has something in it"
    );
    support::render(&mut app, screen.width, screen.height);
    assert!(
        obelus_ui::cursor_position(screen, &app).is_none(),
        "the caret stayed behind in an emptied filter"
    );
}

/// What a front end is told, kept for a test to look at.
#[derive(Debug, Default)]
struct Told {
    /// Every size it was told, in order.
    sizes: std::sync::Mutex<Vec<usize>>,
    /// And every ground.
    grounds: std::sync::Mutex<Vec<ratatui::style::Color>>,
    /// And every pair of colours a hold is drawn in.
    holds: std::sync::Mutex<Vec<(ratatui::style::Color, ratatui::style::Color)>>,
}

impl obelus_app::app::Drawing for Told {
    fn text_size(&self, points: usize) {
        self.sizes.lock().expect("what was said").push(points);
    }

    fn drawn_on(&self, ground: ratatui::style::Color) {
        self.grounds.lock().expect("what was said").push(ground);
    }

    fn holding(&self, held: ratatui::style::Color, row: ratatui::style::Color) {
        self.holds.lock().expect("what was said").push((held, row));
    }

    /// Nothing: what this is a test of is the size, and the shape of the
    /// caret is said on every frame -- which is a test of its own, in the
    /// crate that draws one.
    fn animates(&self, _: bool) {}

    fn caret_is(&self, _: obelus_app::app::Caret, _: Option<obelus_component::layers::Layer>) {}

    /// Nor this: which faces a window draws with is its own business, and
    /// what is under test here is the size.
    fn use_fonts(&self, _: &[String]) {}
}

impl Told {
    /// The last size it was told, or nothing if it was never told.
    ///
    /// The last rather than the whole list: reading the settings applies
    /// them more than once -- the reader's answers, then the project's laid
    /// over them -- and how many times is not something this is about.
    fn last(&self) -> Option<usize> {
        self.sizes.lock().expect("what was said").last().copied()
    }

    /// And the last ground, for the same reason.
    fn ground(&self) -> Option<ratatui::style::Color> {
        self.grounds.lock().expect("what was said").last().copied()
    }

    /// How many times a ground was said at all.
    fn grounds_said(&self) -> usize {
        self.grounds.lock().expect("what was said").len()
    }

    /// And the last pair a hold is drawn in.
    fn holds(&self) -> Option<(ratatui::style::Color, ratatui::style::Color)> {
        self.holds.lock().expect("what was said").last().copied()
    }
}

/// The one setting the application cannot act on reaches the thing that
/// can, both when it says who it is and every time the settings change.
///
/// Both halves, because each passes with the other broken: a front end
/// told only at startup draws at the old size for the rest of the session,
/// and one told only on a change starts at whatever it guessed.
///
/// Deliberate break: taking the call out of `apply_config` leaves the
/// second assertion at one entry, and taking it out of `drawn_by` leaves
/// the first empty.
#[test]
fn the_size_of_the_text_reaches_whatever_is_drawing() {
    let _taken = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let scratch = temporary("sizing");
    let file = settings_file(&scratch);
    std::fs::write(&file, "font_size = 20\n").expect("the settings");

    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.config_file_for_test(file.clone());
    let told = std::sync::Arc::new(Told::default());
    app.drawn_by(told.clone());
    assert_eq!(told.last(), Some(20), "the size it starts at");

    // And the reader changes it -- or another Obelus does, which arrives
    // the same way: the file is read again and what it says is applied.
    std::fs::write(&file, "font_size = 24\n").expect("the settings");
    app.config_file_for_test(file);
    assert_eq!(told.last(), Some(24), "the size it was changed to");
}

/// And so does what the page is drawn on, which a window wants for the
/// margin round the grid: a window is not a whole number of cells, and
/// the strip left over is the one part of it no cell says the colour of.
///
/// Both moments again, and for the same reason each half fails on its
/// own: a front end told only at startup paints last week's theme round
/// this week's page, and one told only on a change has no colour at all
/// until the reader touches a setting.
///
/// Deliberate break: take the call out of `set_theme` and the second
/// assertion still holds the light theme's ground; take it out of
/// `drawn_by` and the first has nothing to compare.
#[test]
fn what_the_page_is_drawn_on_reaches_whatever_is_drawing() {
    let _taken = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let scratch = temporary("grounding");
    let file = settings_file(&scratch);
    std::fs::write(&file, "theme = \"light\"\n").expect("the settings");

    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.config_file_for_test(file.clone());
    let told = std::sync::Arc::new(Told::default());
    app.drawn_by(told.clone());
    let light = app.theme().background;
    assert_eq!(told.ground(), Some(light), "the ground it starts on");

    std::fs::write(&file, "theme = \"dark\"\n").expect("the settings");
    app.config_file_for_test(file);
    let dark = app.theme().background;
    assert_ne!(light, dark, "the two themes are drawn on the same colour");
    assert_eq!(told.ground(), Some(dark), "the ground it was changed to");
}

/// And a theme being previewed is what the window is drawn on, and so is
/// the one put back when the reader escapes.
///
/// Neither is a change to the settings -- nobody has chosen anything -- so
/// a ground said only where the settings are applied never heard of
/// either: the cells were in the theme under the selection and the
/// margin and the title bar round them in the one before it.
///
/// Deliberate break: take the call back out of `set_theme` and put it in
/// `apply_config`, where it was. The first assertion then holds the dark
/// theme's ground while the page is in the light one. And take away the
/// `moved` in `set_theme`, and the count goes up by one a frame.
#[test]
fn a_theme_being_previewed_is_what_the_window_is_drawn_on() {
    let _taken = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let scratch = temporary("previewing");
    let file = settings_file(&scratch);
    // Dark, because it is the first of the two: moving down from it walks
    // to light, and moving down from light walks nowhere.
    std::fs::write(&file, "theme = \"dark\"\n").expect("the settings");

    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.config_file_for_test(file);
    let told = std::sync::Arc::new(Told::default());
    app.drawn_by(told.clone());
    let chosen = app.theme().background;

    support::press_control(&mut app, 'p');
    support::type_text(&mut app, "choose-theme");
    support::press(&mut app, KeyCode::Enter);
    // A frame on either side of the arrow: the list's window is settled
    // by one, so before it there are no rows for the arrow to walk, and
    // the preview is worn by one, so before it the page is in the old
    // theme still.
    support::render(&mut app, 60, 12);
    support::press(&mut app, KeyCode::Down);
    support::render(&mut app, 60, 12);
    let previewing = app.theme().background;
    assert_ne!(
        previewing, chosen,
        "the next theme is drawn on the same colour"
    );
    assert_eq!(told.ground(), Some(previewing), "the theme being previewed");
    // And said once. The preview is worn again on every frame the list is
    // open, and a window told something wakes to draw it: saying it every
    // time is a frame asking for the next one for as long as the list is
    // up.
    let said = told.grounds_said();
    support::render(&mut app, 60, 12);
    support::render(&mut app, 60, 12);
    assert_eq!(
        told.grounds_said(),
        said,
        "a frame said the same ground again"
    );

    support::press(&mut app, KeyCode::Esc);
    assert_eq!(told.ground(), Some(chosen), "the theme put back");
}

/// And so do the two colours a hold is drawn in, which a window wants
/// because it finds the hold in the cells rather than being told where it
/// is.
///
/// Which is the whole reason they go this way. A shape would be a claim
/// about a region of one frame, and every view with a list in it would
/// have to remember to make it -- a table of arms, and the next list
/// added is the one that forgets. These are a fact about the theme, so
/// they are said the way the ground is and at the same two moments, and a
/// view is drawn like every other view because it paints the row that
/// colour, which it must do anyway for the terminal.
///
/// Deliberate break: take the call out of `set_theme` and the second
/// assertion still holds the light theme's pair; take it out of
/// `drawn_by` and the first has nothing to compare.
#[test]
fn the_colours_a_hold_is_drawn_in_reach_whatever_is_drawing() {
    let _taken = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let scratch = temporary("holding");
    let file = settings_file(&scratch);
    std::fs::write(&file, "theme = \"light\"\n").expect("the settings");

    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.config_file_for_test(file.clone());
    let told = std::sync::Arc::new(Told::default());
    app.drawn_by(told.clone());
    let light = (
        app.theme().selection_background,
        app.theme().selected_row_background,
    );
    assert_eq!(told.holds(), Some(light), "the pair it starts on");

    std::fs::write(&file, "theme = \"dark\"\n").expect("the settings");
    app.config_file_for_test(file);
    let dark = (
        app.theme().selection_background,
        app.theme().selected_row_background,
    );
    assert_ne!(light, dark, "the two themes hold in the same colours");
    assert_eq!(told.holds(), Some(dark), "the pair it was changed to");
}

/// A settings file that will not read is a problem marked on that file.
///
/// Which is the point of marking it there rather than only saying so: the
/// reader who opens the file to fix it is shown the line, and the count on
/// the status row, the keys that walk problems and the list they are in are
/// the ones they already have -- nothing here is a second kind of mark.
///
/// Deliberate break: `settings_unreadable` saying its piece and stopping,
/// which is what it did before Obelus said anything about its own files.
#[test]
fn a_settings_file_that_will_not_read_is_marked_on_that_file() {
    let _taken = SETTINGS.lock().expect("the lock");
    let scratch = temporary("unreadable");
    let file = settings_file(&scratch);
    std::fs::write(&file, "theme = \"dark\"\nfont_size 15\n").expect("writing a settings file");

    let mut app = App::new(vec![
        obelus_buffer::Buffer::open(&file).expect("opening the settings file"),
    ]);
    app.config_file_for_test(file.clone());
    support::lay_out(&mut app, 72, 24);

    let problems: Vec<_> = app.problems().collect();
    assert_eq!(problems.len(), 1, "{problems:?}");
    let problem = problems[0];
    assert_eq!(
        problem.severity,
        obelus_lsp::trouble::Severity::Error,
        "a file none of whose settings took is not a remark"
    );
    // Obelus's own name, which is what says who noticed -- and what tells
    // this one from a server's when one of them is taken away.
    assert_eq!(problem.source.as_deref(), Some("Obelus"));
    // The second line, counted from zero, and over characters rather than
    // over nothing: the parser stops between two of them.
    assert_eq!(problem.span.line.get(), 1, "{:?}", problem.span);
    assert!(
        problem.span.end_column.get() > problem.span.column.get(),
        "{:?}",
        problem.span
    );
}

/// And fixing the file takes the mark away.
///
/// A mark that outlived what it was about would be the one thing a mark
/// must not be, which is wrong -- and the settings file is watched, so the
/// moment somebody fixes it Obelus reads it again.
///
/// Deliberate break: `nothing_wrong_with` never called where the file
/// reads, which leaves the old mark beside the new reading.
#[test]
fn fixing_the_settings_file_takes_the_mark_away() {
    let _taken = SETTINGS.lock().expect("the lock");
    let scratch = temporary("fixed");
    let file = settings_file(&scratch);
    std::fs::write(&file, "theme = \"dark\"\nfont_size 15\n").expect("writing a settings file");

    let mut app = App::new(vec![
        obelus_buffer::Buffer::open(&file).expect("opening the settings file"),
    ]);
    app.config_file_for_test(file.clone());
    support::lay_out(&mut app, 72, 24);
    assert_eq!(app.problems().count(), 1, "the broken file was not marked");

    std::fs::write(&file, "theme = \"dark\"\nfont_size = 15\n").expect("fixing the file");
    // The same road the watcher sends Obelus down when somebody else
    // writes that file.
    app.config_file_for_test(file.clone());
    assert_eq!(app.problems().count(), 0, "the mark outlived the mistake");
}

/// A line of a settings file that did nothing is a warning on that line.
///
/// Which is the whole of what a reader gets told about it otherwise:
/// nothing. The file read, so every other line took -- a warning and not an
/// error -- and from the outside a setting Obelus has never heard of looks
/// exactly like one that was obeyed.
///
/// Deliberate break: `apply` going on logging and reporting nothing, which
/// leaves the count at zero.
#[test]
fn a_line_that_did_nothing_is_a_warning_on_that_line() {
    let _taken = SETTINGS.lock().expect("the lock");
    let scratch = temporary("ignored");
    let file = settings_file(&scratch);
    std::fs::write(
        &file,
        "theme = \"dark\"\nshrift = 15\n\n[agents]\ncopilot = \"gpt-5\"\n",
    )
    .expect("writing a settings file");

    let mut app = App::new(vec![
        obelus_buffer::Buffer::open(&file).expect("opening the settings file"),
    ]);
    app.config_file_for_test(file.clone());
    support::lay_out(&mut app, 72, 24);

    let problems: Vec<_> = app.problems().collect();
    assert_eq!(problems.len(), 2, "{problems:?}");
    for problem in &problems {
        assert_eq!(
            problem.severity,
            obelus_lsp::trouble::Severity::Warning,
            "a line that did nothing in a file that read is not an error"
        );
        assert_eq!(problem.source.as_deref(), Some("Obelus"));
    }
    let said: Vec<(usize, &str)> = problems
        .iter()
        .map(|problem| (problem.span.line.get(), problem.message.as_str()))
        .collect();
    assert!(
        said.contains(&(1, "No setting is called shrift")),
        "{said:?}"
    );
    // Under the table it is in, which is the only way to say which of an
    // agent's lines is the one that is wrong.
    assert!(
        said.contains(&(
            4,
            "What agents.copilot is set to is not a table of its settings"
        )),
        "{said:?}"
    );
}

/// And what a project's file may not set is marked on the project's file.
///
/// On that one and not the reader's: two files can name the same setting,
/// and a mark on the wrong one sends the reader to a line that is right.
///
/// Deliberate break: `apply_project` reporting against `settled.path`,
/// which is the reader's own file.
#[test]
fn what_a_project_may_not_set_is_marked_on_the_projects_file() {
    let _taken = SETTINGS.lock().expect("the lock");
    let scratch = project("not-allowed", "theme = \"dark\"\nagent = \"copilot\"\n");
    let theirs = scratch.path().join(".obelus").join("config.toml");

    let mut app = App::new(vec![
        obelus_buffer::Buffer::open(&theirs).expect("opening the project's settings"),
    ]);
    app.working_directory_for_test(scratch.path().to_path_buf());
    let mine = temporary("not-allowed-mine");
    app.config_file_for_test(settings_file(&mine));
    support::lay_out(&mut app, 72, 24);

    let problems: Vec<_> = app.problems().collect();
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(problems[0].message, "A project may not set agent");
    // The second line of the project's file, where `agent` is written.
    assert_eq!(problems[0].span.line.get(), 1);
}

/// A workflow nothing answers to is marked on the line that names it.
///
/// On a project's file, because that is where a workflow is most often
/// chosen. The word was taken as no workflow at all, so the line read as
/// obeyed while the agent was told nothing -- which from the outside is a
/// setting that silently does not work.
///
/// Deliberate break: `apply` taking any word, as it did, which leaves no
/// problem at all.
#[test]
fn a_workflow_nothing_answers_to_is_marked() {
    let _taken = SETTINGS.lock().expect("the lock");
    let scratch = project("no-workflow", "workflow = \"feature_branch\"\n");
    let theirs = scratch.path().join(".obelus").join("config.toml");

    let mut app = App::new(vec![
        obelus_buffer::Buffer::open(&theirs).expect("opening the project's settings"),
    ]);
    app.working_directory_for_test(scratch.path().to_path_buf());
    let mine = temporary("no-workflow-mine");
    app.config_file_for_test(settings_file(&mine));
    support::lay_out(&mut app, 72, 24);

    let problems: Vec<_> = app.problems().collect();
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(problems[0].message, "No workflow is called feature_branch");
    assert_eq!(problems[0].span.line.get(), 0);
    assert_eq!(app.config().workflow, "none", "the word was taken anyway");
}

/// A line of the key table that bound nothing is a warning on that line.
///
/// The three ways it can happen, each with a different thing left to say:
/// a command that has been renamed, a chord Obelus cannot read at all, and
/// a chord it can read and may not be given. A key that silently never
/// fires is the thing the whole `why_not` judgement exists to prevent, and
/// until now the file was the one place it could still happen.
///
/// Deliberate break: `Keymap::with` skipping quietly, which is what it did
/// -- the count drops to zero. And `keys_that_bound_nothing` looking the
/// span up under the bare name rather than under `keys.`, which leaves
/// three problems with nowhere to be.
#[test]
fn a_key_that_bound_nothing_is_a_warning_on_that_line() {
    let _taken = SETTINGS.lock().expect("the lock");
    let scratch = temporary("unbound");
    let file = settings_file(&scratch);
    std::fs::write(
        &file,
        "theme = \"dark\"\n\n[keys]\nopen-file = \"f1\"\nopen-fiel = \"f2\"\n\
         save-file = \"ctrl+shiftier+s\"\nclose-document = \"ctrl+shift+w\"\n",
    )
    .expect("writing a settings file");

    let mut app = App::new(vec![
        obelus_buffer::Buffer::open(&file).expect("opening the settings file"),
    ]);
    app.config_file_for_test(file.clone());
    support::lay_out(&mut app, 72, 24);

    let said: Vec<(usize, String)> = app
        .problems()
        .map(|problem| (problem.span.line.get(), problem.message.clone()))
        .collect();
    assert_eq!(said.len(), 3, "{said:?}");
    // The line that binds is not one of them.
    assert!(!said.iter().any(|(line, _)| *line == 3), "{said:?}");
    assert_eq!(said[0], (4, "No command is called open-fiel".to_string()));
    assert_eq!(
        said[1],
        (
            5,
            "Nothing is bound to save-file: ctrl+shiftier+s is not a key".to_string()
        )
    );
    // The reason the page that binds keys gives, in its own words, because
    // it is the same judgement.
    assert!(
        said[2]
            .1
            .starts_with("Nothing is bound to close-document: "),
        "{said:?}"
    );
    assert_eq!(said[2].0, 6);
}

/// A theme nothing answers to is a warning on the line that names it.
///
/// The colours on screen stay as they are, which is the other half of the
/// same judgement: a reader who cannot read the screen cannot fix the file.
/// So the only way they find out is the mark, and it goes on the line they
/// wrote rather than on a file that does not exist.
///
/// Deliberate break: `apply_config` going back to `if let Some(theme)`,
/// which leaves nothing written down.
#[test]
fn a_theme_nothing_answers_to_is_a_warning_on_its_line() {
    let _taken = SETTINGS.lock().expect("the lock");
    let scratch = temporary("no-theme");
    let file = settings_file(&scratch);
    std::fs::write(&file, "icons = true\ntheme = \"moonlight\"\n").expect("writing the settings");

    let mut app = App::new(vec![
        obelus_buffer::Buffer::open(&file).expect("opening the settings file"),
    ]);
    app.config_file_for_test(file.clone());
    support::lay_out(&mut app, 72, 24);

    let problems: Vec<_> = app.problems().collect();
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(problems[0].message, "No theme is called moonlight");
    assert_eq!(
        problems[0].severity,
        obelus_lsp::trouble::Severity::Warning,
        "a theme that is not there leaves the colours alone"
    );
    // The second line, where the name is written.
    assert_eq!(problems[0].span.line.get(), 1);
}

/// And a theme file that will not parse is an error on that file.
///
/// On the theme's own file and not on the line that names it: the name is
/// right, and the line that is wrong is in the other file.
///
/// Deliberate break: `read_where` handing back no span, which leaves the
/// mark with nowhere to be and the count at zero.
#[test]
fn a_theme_file_that_will_not_read_is_an_error_on_that_file() {
    let _taken = SETTINGS.lock().expect("the lock");
    let scratch = temporary("bad-theme");
    let themes = scratch.path().join("themes");
    std::fs::create_dir_all(&themes).expect("a themes directory");
    let theme = themes.join("moonlight.toml");
    std::fs::write(&theme, "base = \"dark\"\n\n[syntax]\nkeyword \"#ff0000\"\n")
        .expect("writing a theme");

    let file = settings_file(&scratch);
    std::fs::write(&file, "theme = \"moonlight\"\n").expect("writing the settings");

    let mut app = App::new(vec![
        obelus_buffer::Buffer::open(&theme).expect("opening the theme"),
    ]);
    app.config_file_for_test(file);
    support::lay_out(&mut app, 72, 24);

    let problems: Vec<_> = app.problems().collect();
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        problems[0].message.starts_with("This theme will not read"),
        "{problems:?}"
    );
    assert_eq!(problems[0].severity, obelus_lsp::trouble::Severity::Error);
    // The fourth line, where the equals is missing.
    assert_eq!(problems[0].span.line.get(), 3);
}

/// And notes that will not read are an error on the notes' own file.
///
/// A file a reader opens: they write notes into it from the page and edit
/// it by hand, so being told the whole list will not read without being
/// told which line is a reader reading it all themselves. Nothing is shown
/// and nothing is written while it is like that -- an empty page is not
/// what the file says, it is what Obelus can make of it.
///
/// Deliberate break: `the_notes_now` saying its piece and stopping, which
/// is what it did.
#[test]
fn notes_that_will_not_read_are_an_error_on_the_notes_file() {
    let _taken = SETTINGS.lock().expect("the lock");
    let scratch = support::Scratch::new("notes-unreadable");
    support::make_room_for_notes(scratch.path());
    let notes = obelus_git::todo::path(scratch.path());
    std::fs::write(&notes, "[[todo]]\nid = \"ABCDEFGH\"\nsaid \"a note\"\n").expect("the notes");

    let mut app = App::new(vec![
        obelus_buffer::Buffer::open(&notes).expect("opening the notes"),
    ]);
    app.working_directory_for_test(scratch.path().to_path_buf());
    let mine = temporary("notes-unreadable-settings");
    app.config_file_for_test(settings_file(&mine));
    support::lay_out(&mut app, 72, 24);
    dispatch::dispatch(&mut app, Command::TodoOpen);

    let problems: Vec<_> = app.problems().collect();
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        problems[0].message.starts_with("The notes will not read"),
        "{problems:?}"
    );
    assert_eq!(problems[0].severity, obelus_lsp::trouble::Severity::Error);
    // The third line, where the equals is missing.
    assert_eq!(problems[0].span.line.get(), 2);
}

/// Nothing on the page writes in the column its bar is in.
///
/// Two writers reached into it, and both are covered here because each
/// passes with the other broken: a group's heading, which filled the whole
/// region rather than the room left beside the bar, and a value as wide as
/// its own column, which put the arrow that follows it in the column along.
/// The bar came out with a gap at every group and at every long value --
/// and in a window, where those blocks are the bar *saying where it is* and
/// the shape is drawn from them, a bar with a hole in it stopped being a
/// bar at all and came out as a column of squares.
///
/// Deliberate break: `..region` in place of `width: room` where the heading
/// is drawn, or `after + 1` in place of `arrow_at` beside a value.
#[test]
fn nothing_on_the_page_writes_in_the_column_the_bar_is_in() {
    let _taken = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let scratch = temporary("bar-column");
    let file = settings_file(&scratch);
    // A theme whose name is wider than the column a value is given, so the
    // arrow beside it has nowhere of its own left to go.
    std::fs::write(&file, "theme = \"catppuccin-mocha\"\n").expect("the settings");
    let mut app = open(&file);

    let dump = support::render(&mut app, 66, 12);
    for (at, row) in body_of(&dump).iter().enumerate() {
        assert!(
            row.ends_with('\u{2588}'),
            "row {at} of the body is not the bar's: {row:?}\n{dump}"
        );
    }
}

/// The rows of a settings page's body: what is between the rule under the
/// tabs and the rule over the foot, without the dump's own row numbers.
///
/// The whole body, because the first row and the last are where a heading
/// and a foot reach into the bar's column -- and a test that looked only
/// at the run between the first block and the last would see neither.
fn body_of(dump: &str) -> Vec<&str> {
    let rows: Vec<&str> = support::text_block(dump)
        .lines()
        .filter_map(|row| row.split_once('|'))
        .map(|(_, said)| said)
        .collect();
    let rule = |row: &&str| !row.is_empty() && row.chars().all(|cell| cell == '\u{2500}');
    let top = rows.iter().position(rule).expect("a rule under the tabs");
    let foot = rows
        .iter()
        .skip(top + 1)
        .position(rule)
        .expect("a rule over the foot")
        + top
        + 1;
    assert!(foot > top + 1, "the page has a body: {rows:?}");
    rows[top + 1..foot].to_vec()
}

/// A group's name is bold, which is what tells a heading from a setting.
///
/// The word and nothing else stands for a group -- no rule across the page
/// and no second colour -- so the weight is what is left, and it is on the
/// word itself rather than on a row of its own. The same answer a heading
/// gets when Obelus is reading somebody's markdown.
///
/// The cells rather than the dump, because what says a character is bold is
/// a modifier and the dump names a style by its colours.
///
/// Deliberate break: drop the `add_modifier` in `SettingsView::heading`. A
/// group's name is then a setting's name with nothing on it, on a page
/// whose whole shape is names under names.
#[test]
fn a_groups_name_is_bold() {
    let scratch = temporary("heading-bold");
    let file = settings_file(&scratch);
    let mut app = open(&file);

    let dump = support::render(&mut app, 76, 16);
    let rows: Vec<&str> = support::text_block(&dump)
        .lines()
        .filter(|row| row.contains('|'))
        .collect();
    // The first group's heading, and the first setting under it.
    let heading = rows
        .iter()
        .position(|row| row.contains("Appearance"))
        .unwrap_or_else(|| panic!("no heading:\n{dump}"));
    let setting = rows
        .iter()
        .position(|row| row.contains("Theme"))
        .unwrap_or_else(|| panic!("no setting:\n{dump}"));

    let cells = support::cells_of(&mut app, 76, 16);
    let bold = |row: usize, word: &str| {
        let at = support::column_of(rows[row], word);
        cells
            .cell((
                u16::try_from(at).expect("a column"),
                u16::try_from(row).expect("a row"),
            ))
            .expect("a cell")
            .modifier
            .contains(ratatui::style::Modifier::BOLD)
    };
    assert!(
        bold(heading, "Appearance"),
        "the heading is not bold:\n{dump}"
    );
    assert!(
        !bold(setting, "Theme"),
        "a setting is bold as well, so the weight says nothing:\n{dump}"
    );
}
