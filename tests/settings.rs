//! The settings view: tabs, a filter, and a control per row.

mod support;

use crossterm::event::KeyCode;
use obelus::{app::App, app::dispatch, command::Command, config, event::Event};

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

/// A tree of its own for one test, with settings in it.
fn tree(name: &str, contents: &str) -> support::Scratch {
    let scratch = support::Scratch::new(&format!("tree-{name}"));
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

/// The settings are a dialog: nothing of obelus's own opens over them.
#[test]
fn nothing_of_obeluss_own_opens_over_the_settings() {
    let _taken = SETTINGS.lock().expect("the lock");
    let scratch = temporary("modal");
    let mut app = open(&settings_file(&scratch));
    let page = support::render(&mut app, 66, 12);
    for key in ['o', 'e', 'p', 'q'] {
        support::press_control(&mut app, key);
    }
    assert!(app.picker().is_none(), "a list opened over the settings");
    assert!(!app.should_quit(), "ctrl+q reached the key table");
    assert_eq!(
        support::text_block(&page),
        support::text_block(&support::render(&mut app, 66, 12)),
        "something opened over the settings"
    );
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
        support::text_block(&dump).contains("Appearance"),
        "the settings are not behind the list:\n{dump}"
    );

    // It filters by typing, which is the whole reason it is this list.
    support::type_text(&mut app, "li");
    let picker = app.picker().expect("the choices");
    assert_eq!(picker.match_count(), 1, "the query narrowed nothing");
    support::press(&mut app, KeyCode::Enter);

    assert!(app.picker().is_none(), "the list stayed open");
    assert_eq!(app.theme_name(), "light", "the theme did not change");
    assert_eq!(
        config::from_toml(&std::fs::read_to_string(&file).expect("the file")).theme,
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
    // the theme nobody picked, and the next escape anywhere in obelus has
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

    assert!(app.config().icons, "the glyphs start on");
    let on = box_of(&mut app);
    support::press(&mut app, KeyCode::Enter);
    assert!(!app.config().icons, "enter did not flip it");
    assert_ne!(
        box_of(&mut app),
        on,
        "the box reads the same whichever way it is set"
    );

    // The arrows do not touch it: they are the tabs'.
    support::press(&mut app, KeyCode::Right);
    assert!(!app.config().icons, "an arrow flipped the switch");
    support::press(&mut app, KeyCode::Left);
    assert!(!app.config().icons, "an arrow flipped the switch");

    obelus::icons::use_glyphs(true);
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
            .rows()
            .iter()
            .map(|shown| shown.setting.name.to_string())
            .collect::<Vec<_>>()
    };
    // Every setting obelus has, in the order the groups are written in.
    assert_eq!(rows(&app).len(), obelus::config::ALL.len());
    assert_eq!(rows(&app)[0], "Colour theme");
    assert_eq!(
        rows(&app).last().map(String::as_str),
        Some("Files a tree ignores")
    );

    // A heading on the first of each group and on nothing else, so the
    // focus never has a row to step over.
    let opens: Vec<Option<&'static str>> = app
        .settings()
        .expect("the settings")
        .rows()
        .iter()
        .map(|shown| shown.opens.map(obelus::config::Group::label))
        .collect();
    assert_eq!(opens[0], Some("Appearance"));
    assert_eq!(opens[1], None, "a second heading inside one group");
    assert_eq!(
        opens.iter().filter(|opens| opens.is_some()).count(),
        obelus::config::Group::ALL.len(),
        "not one heading per group: {opens:?}"
    );

    // And the tabs are the pages: the settings, the keys, the agents.
    assert_eq!(
        obelus::component::settings::Settings::tabs(),
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
    // The "C" of "Colour theme" is the first cell of the name: past the
    // `NN|` the dump writes down its side, past the row's own left-hand
    // padding, and past the indent that puts a setting under its group.
    let name_at = 3 + 1 + usize::from(obelus::component::settings::GROUP_INDENT);
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
    assert_eq!(app.settings().expect("the settings").rows().len(), 1);
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

    // A file written by someone else -- an editor, another obelus -- is what
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
    obelus::icons::use_glyphs(true);
}

/// Settings arriving from another machine are noticed, link and all.
///
/// Which is how anybody keeps settings in git: the file lives in a dotfiles
/// repository and the place obelus looks is a link to it. What a `git pull`
/// rewrites is the file at the far end, so that is the path the change
/// arrives on -- not the one obelus was told about.
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
    let linked = config_home.join("config.toml");
    std::os::unix::fs::symlink(&real, &linked).expect("a link");

    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.config_file_for_test(linked);
    assert_eq!(app.theme_name(), "dark", "it did not read through the link");

    // What another machine's change looks like once git has put it there:
    // the repository's own file, rewritten, and the watcher reporting that
    // path rather than the link's.
    std::fs::write(&real, "theme = \"light\"\n").expect("the file");
    app.handle(Event::FileChanged { path: real });
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
/// had most to explain, and cut off further still on a row the tree had
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
        at(rows[name + 1], "in lists").is_some(),
        "what it does is not under its name:\n{dump}"
    );
    assert!(
        at(rows[name + 1], "in lists") > at(rows[name], "Nerd Font"),
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
    let rows = app.settings().expect("the settings").rows();
    let name = rows[focused.min(rows.len() - 1)].setting.name;
    assert!(
        support::text_block(&dump).contains(name),
        "the entry the keys are on is not on screen: {name}\n{dump}"
    );
}

/// A tree can carry settings of its own, and they win where they say
/// anything.
///
/// Which is what a project is for: everybody reading this repository gets
/// its wrapped lines, whatever they have set for themselves elsewhere.
///
/// Broken deliberately by reading the tree's table into a fresh config
/// instead of over the reader's: the theme the reader had chosen came back
/// as the default, and the last assertion failed.
#[test]
fn a_tree_lays_its_own_settings_over_the_readers() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let scratch = tree("over", "wrap = true\n");
    let root = scratch.path().to_path_buf();

    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    // The reader's own, as they would have come from their file.
    app.configure(
        obelus::config::Config {
            theme: "light".to_string(),
            wrap: false,
            ..obelus::config::Config::default()
        },
        Vec::new(),
    );
    app.working_directory_for_test(root.clone());

    assert!(app.config().wrap, "the tree's setting did not take");
    assert_eq!(
        app.theme_name(),
        "light",
        "the tree took away a setting it never named"
    );
}

/// A tree may not choose the agent, or rebind a key.
///
/// A tree is written by whoever wrote the tree. Most of these settings are
/// harmless to hand over; starting a program is not, and neither is moving
/// the keys under somebody's fingers.
///
/// Broken deliberately by giving `agent` and `keys` `Reach::Anywhere`: both
/// arrived from the tree and both assertions failed.
#[test]
fn a_tree_may_not_start_an_agent_or_move_a_key() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let scratch = tree(
        "reach",
        "agent = \"claude-acp\"\n[keys]\nquit = \"ctrl+x\"\n",
    );
    let root = scratch.path().to_path_buf();

    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.configure(obelus::config::Config::default(), Vec::new());
    app.working_directory_for_test(root.clone());

    assert_eq!(app.config().agent, None, "a tree started an agent");
    assert!(
        app.config().keys.is_empty(),
        "a tree moved a key: {:?}",
        app.config().keys
    );
}

/// A setting the tree has is not the reader's to change, and the row says
/// which file has it.
///
/// Broken deliberately by letting `change_setting` write anyway: the switch
/// moved, the file the reader's own settings live in got a line the tree
/// overrides, and the first assertion failed.
#[test]
fn a_setting_the_tree_has_cannot_be_changed_here() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let scratch = tree("pinned", "wrap = true\n");
    let root = scratch.path().to_path_buf();

    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.configure(obelus::config::Config::default(), Vec::new());
    app.working_directory_for_test(root.clone());
    support::lay_out(&mut app, 76, 12);
    dispatch::dispatch(&mut app, Command::ConfigOpen);

    // Onto the wrapped-lines row and try to turn it off.
    support::type_text(&mut app, "wrap");
    support::press(&mut app, KeyCode::Enter);
    assert!(
        app.config().wrap,
        "a setting the tree has was changed from the settings page"
    );

    // And the row says where it comes from.
    let dump = support::render(&mut app, 76, 12);
    assert!(
        support::text_block(&dump).contains(&support::as_shown(".obelus/config.toml")),
        "the row does not say which file has it:\n{dump}"
    );
}

/// The tree's own page writes to the tree's file, and leaves alone what it
/// did not come for.
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
    let root = tree(
        "write",
        "# what this project needs\n\nwrap = true\n# the margin is noisy here\nblame_margin = false\n",
    );

    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.configure(obelus::config::Config::default(), Vec::new());
    app.working_directory_for_test(root.path().to_path_buf());
    support::lay_out(&mut app, 76, 16);
    dispatch::dispatch(&mut app, Command::ConfigTree);
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

/// Delete takes a setting out of the tree's file, and the file keeps its
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
    let root = tree(
        "Unset",
        "# what this project needs\n\nwrap = true\nblame_margin = true\n",
    );

    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.configure(obelus::config::Config::default(), Vec::new());
    app.working_directory_for_test(root.path().to_path_buf());
    support::lay_out(&mut app, 76, 16);
    dispatch::dispatch(&mut app, Command::ConfigTree);
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
    assert!(!app.config().wrap, "the tree still has it");
}

/// A tree that has no settings file gets one the moment something is set.
///
/// Broken deliberately by refusing to write when the file is not there:
/// nothing happened and there was no file, which is a page that cannot be
/// used until somebody makes a file by hand.
#[test]
fn a_tree_with_no_settings_gets_a_file_when_one_is_set() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let root = support::Scratch::new("tree-new");

    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.configure(obelus::config::Config::default(), Vec::new());
    app.working_directory_for_test(root.path().to_path_buf());
    support::lay_out(&mut app, 76, 16);
    dispatch::dispatch(&mut app, Command::ConfigTree);
    support::type_text(&mut app, "wrap");
    support::press(&mut app, KeyCode::Enter);

    let written = std::fs::read_to_string(root.join(".obelus").join("config.toml"))
        .expect("no file was made");
    assert!(written.contains("wrap = true"), "{written:?}");
}

/// On the tree's page, a setting the tree has not got says whose value is
/// showing -- and the two tabs a tree may not have say so.
///
/// Broken deliberately by leaving `inherited` `None` for every row: the
/// rows the tree does not set looked exactly like the one it does, and a
/// reader could not tell what this project had actually decided.
#[test]
fn the_trees_page_says_which_settings_are_not_its_own() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let root = tree("whose", "wrap = true\n");

    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    // The theme they chose, and the margin's names written down at exactly
    // what obelus would have done anyway: a reader who agrees has still
    // been here, and the column has to say so.
    app.configure(
        obelus::config::Config {
            theme: "light".to_string(),
            blame_margin: obelus::config::Config::default().blame_margin,
            ..obelus::config::Config::default()
        },
        vec!["theme", "blame_margin"],
    );
    app.working_directory_for_test(root.path().to_path_buf());
    // Tall enough for the whole page: every setting is on one now, and this
    // reads two of them against each other.
    support::lay_out(&mut app, 76, 32);
    dispatch::dispatch(&mut app, Command::ConfigTree);

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

    // And the setting the tree does have says so, in the same column: a
    // column where two of the three layers have a word and the third is
    // blank asks the reader to read an absence.
    let dump = support::render(&mut app, 76, 32);
    let wrap = support::text_block(&dump)
        .lines()
        .find(|row| row.contains("Wrap long lines"))
        .expect("the row");
    assert!(
        wrap.contains("Project"),
        "a setting the tree has does not say so: {wrap:?}"
    );
    // And beside it, one the reader wrote down and the tree says nothing
    // about -- written at exactly what obelus would have done anyway. The
    // column asks whether their file speaks about it, not whether it
    // disagrees: a reader who wrote a line and happened to agree was being
    // told they had never been here.
    let blame = support::text_block(&dump)
        .lines()
        .find(|row| row.contains("Blame in the margin"))
        .expect("the row")
        .to_string();
    assert!(
        blame.contains("Global"),
        "a setting written down at its default is not the reader's: {blame:?}"
    );

    // And the two tabs a tree may not have.
    support::press(&mut app, KeyCode::Tab);
    let dump = support::render(&mut app, 76, 32);
    assert!(
        support::text_block(&dump).contains("may not move the keys"),
        "{dump}"
    );
}

/// A tree that acquires settings while obelus is looking at it is heard.
///
/// Several obelus processes on one project is the ordinary way to work, and
/// the ordinary project has no settings of its own until somebody gives it
/// some -- from the window next door, or in a pull. Watching only the file
/// that was there at startup is the "read once at startup" mistake with a
/// longer fuse: it looks right until the file is created.
///
/// Broken deliberately by comparing the change against the file the tree
/// *has* rather than the one it would have: the event matched nothing, the
/// setting never arrived, and this failed.
#[test]
fn a_tree_that_gains_settings_while_obelus_is_open_is_heard() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let root = support::Scratch::new("tree-later");

    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.configure(obelus::config::Config::default(), Vec::new());
    app.working_directory_for_test(root.path().to_path_buf());
    assert!(!app.config().wrap, "the tree had settings already");

    // Somebody else writes the project's first settings, and the watcher
    // says so.
    let path = root.join(".obelus").join("config.toml");
    std::fs::create_dir_all(root.join(".obelus")).expect("the directory");
    std::fs::write(&path, "wrap = true\n").expect("the file");
    app.handle(Event::FileChanged { path });

    assert!(
        app.config().wrap,
        "a tree that gained settings was not heard"
    );
}

/// Dim means "not yours to use here", so the two pages use it the opposite
/// way round.
///
/// On the reader's page a setting the tree has taken is unusable, and the
/// whole row says so. On the tree's page a setting the tree has *not* got is
/// the one thing a reader can do something to -- pressing it is how a
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
    let root = tree("grey", "wrap = true\n");

    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.configure(obelus::config::Config::default(), Vec::new());
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

    // The tree's page: the name is ordinary ink, the word beside it is not.
    dispatch::dispatch(&mut app, Command::ConfigTree);
    support::type_text(&mut app, "blame");
    let dump = support::render(&mut app, 76, 16);
    assert_ne!(
        letter(&dump, "Blame in the margin", "margin"),
        letter(&dump, "Blame in the margin", "Default"),
        "the name is as dim as the word saying the value is not the tree's:\n{dump}"
    );

    // The reader's page: a setting the tree has taken is dim throughout,
    // name and all, because there it really cannot be used.
    dispatch::dispatch(&mut app, Command::ConfigOpen);
    support::type_text(&mut app, "wrap");
    let dump = support::render(&mut app, 76, 16);
    assert_eq!(
        letter(&dump, "Wrap long lines", "long"),
        letter(
            &dump,
            "Wrap long lines",
            &support::as_shown(".obelus/config.toml")
        ),
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
    app.configure(obelus::config::Config::default(), Vec::new());
    app.working_directory_for_test(root.path().to_path_buf());
    support::lay_out(&mut app, 76, 10);
    dispatch::dispatch(&mut app, Command::ConfigTree);

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
        wide.find("Agents") < wide.find(".obelus"),
        "the name is not after the tabs: {wide:?}"
    );

    // Narrow: the tabs are whole, and the name is simply not there. Wide
    // enough for the three tabs and not for the name after them, which is
    // the corner this is about.
    let narrow = tabs(&mut app, 40);
    assert!(
        narrow.contains("Settings") && narrow.contains("Agents"),
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
    let scratch = temporary("ends");
    let file = settings_file(&scratch);
    let mut app = open(&file);
    let focus = |app: &App| app.settings().expect("the settings").focus();

    // Every setting on one page: End reaches the last, Home the first, and
    // neither wraps past its end.
    let last = obelus::config::ALL.len() - 1;
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

    let agents: Vec<obelus::agent::Agent> = (0..12)
        .map(|index| obelus::agent::Agent {
            id: format!("agent-{index}"),
            name: format!("Agent {index}"),
            version: "1.0.0".to_string(),
            description: "One of several".to_string(),
            authors: vec!["Somebody".to_string()],
            license: "MIT".to_string(),
            website: None,
            icon: None,
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
    let scratch = temporary("registry");
    let file = settings_file(&scratch);
    let mut app = open(&file);
    support::press(&mut app, KeyCode::BackTab);
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
    support::press(&mut app, KeyCode::BackTab);

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
        icon: None,
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
    let scratch = temporary("window");
    let file = settings_file(&scratch);
    let mut app = open(&file);
    support::press(&mut app, KeyCode::BackTab);

    let agents: Vec<obelus::agent::Agent> = (0..12)
        .map(|index| obelus::agent::Agent {
            id: format!("agent-{index}"),
            name: format!("Agent {index}"),
            version: "1.0.0".to_string(),
            description: "One of several".to_string(),
            authors: vec!["Somebody".to_string()],
            license: "MIT".to_string(),
            website: None,
            icon: None,
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

/// The first agent to install is the one obelus talks to, and the next one
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

    let agents: Vec<obelus::agent::Agent> = (0..2)
        .map(|index| obelus::agent::Agent {
            id: format!("agent-{index}"),
            name: format!("Agent {index}"),
            version: "1.0.0".to_string(),
            description: "One of two".to_string(),
            authors: vec!["Somebody".to_string()],
            license: "MIT".to_string(),
            website: None,
            icon: None,
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
    assert_eq!(
        app.config().agent,
        None,
        "one was active before any install"
    );

    // An install that failed activates nothing. There is nothing to talk
    // to, and a card that says both "Failed" and "active" says nothing.
    app.handle(Event::Installed {
        id: "agent-0".to_string(),
        failure: Some("npm is not on the path".to_string()),
    });
    assert_eq!(app.config().agent, None, "a failed install was activated");

    // Nor does one that says it worked and left no record behind. The
    // record is the proof, and this is the state a reader was stuck in
    // once: the settings named an agent, the card said "active", and
    // nothing could be started.
    app.handle(Event::Installed {
        id: "agent-0".to_string(),
        failure: None,
    });
    assert_eq!(
        app.config().agent,
        None,
        "an agent with nothing installed under its name was activated"
    );

    // The first one that works becomes the one in use, and the file says so
    // -- a choice that is gone tomorrow was a preview rather than a choice.
    installed(&root, "agent-0", "1.0.0");
    app.handle(Event::Installed {
        id: "agent-0".to_string(),
        failure: None,
    });
    assert_eq!(app.config().agent.as_deref(), Some("agent-0"));
    assert_eq!(
        config::from_toml(&std::fs::read_to_string(&file).expect("the file"))
            .agent
            .as_deref(),
        Some("agent-0"),
    );

    // And the next one does not take over.
    installed(&root, "agent-1", "1.0.0");
    app.handle(Event::Installed {
        id: "agent-1".to_string(),
        failure: None,
    });
    assert_eq!(
        app.config().agent.as_deref(),
        Some("agent-0"),
        "the second install took over"
    );
}

/// What a finished install leaves behind: something to run, and the record
/// that says so.
fn installed(root: &std::path::Path, id: &str, version: &str) {
    let home = obelus::agent::home(id, root).expect("a directory for it");
    std::fs::create_dir_all(&home).expect("a directory");
    let program = home.join("run-me");
    std::fs::write(&program, "").expect("a program");
    obelus::agent::remember(id, &program, &[], version, root).expect("the record");
}

/// An agent the settings name and the machine does not have is not active,
/// whatever the settings say: the card offers to install it.
///
/// The state a reader was stuck in. obelus decided an agent was installed by
/// looking at what `npm` had left lying about, and npm builds its tree in an
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

    // And npm's tree is all there, exactly as an interrupted install leaves
    // it -- a manifest, and the link to the program.
    let home = obelus::agent::home("agent-0", &root).expect("a directory for it");
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
    app.handle(Event::Registry {
        agents: vec![obelus::agent::Agent {
            id: "agent-0".to_string(),
            name: "Agent 0".to_string(),
            version: "1.0.0".to_string(),
            description: "The one that got away".to_string(),
            authors: Vec::new(),
            license: String::new(),
            website: None,
            icon: None,
            distribution: obelus::agent::Distribution::Node {
                package: "agent-0@1.0.0".to_string(),
                arguments: Vec::new(),
            },
        }],
        failure: None,
    });

    let dump = support::render(&mut app, 76, 16);
    let text = support::text_block(&dump);
    assert!(
        text.contains("install"),
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
/// the one obelus is running on.
#[test]
fn a_command_can_be_put_on_another_key() {
    use crossterm::event::KeyModifiers;
    use obelus::{command::Command, keymap::KeyChord};

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
        written.contains("choose-theme") && written.contains("alt+j"),
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
    use obelus::command::Command;

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
        Some(obelus::keymap::KeyChord::new(
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
    use obelus::{command::Command, event::Event};

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
        (KeyCode::Tab, KeyModifiers::NONE, "takes this one"),
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
    use obelus::command::Command;

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
/// file obelus would write is the answer to "what are the settings", and it
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

/// A setting changed in another obelus is a setting changed here.
///
/// The terminal splits the window; obelus does not. So several of them on
/// one project is the ordinary way to work, and a settings file read once at
/// startup would leave every other window holding what the reader has
/// already moved on from. The watcher says the file changed, and what is in
/// it is what obelus is set to -- whichever process wrote it.
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

    // Another obelus writes the file. Nothing else says so: the watcher
    // hands over a path, and everything about what changed is in the file.
    std::fs::write(&file, "theme = \"dark\"\n").expect("the other window");
    app.handle(Event::FileChanged { path: file });
    assert_eq!(
        app.theme_name(),
        "dark",
        "the change in the other window never arrived"
    );
}

/// A settings file obelus cannot read is one it will not write over.
///
/// What is in it is the reader's. A file caught mid-write by another obelus,
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
    app.handle(Event::FileChanged { path: file.clone() });

    // A change made here is not saved, and the reader is told why rather
    // than finding out later that their settings went.
    dispatch::dispatch(&mut app, Command::ThemeSelect);
    support::press(&mut app, KeyCode::Enter);
    assert_eq!(
        std::fs::read_to_string(&file).expect("the file"),
        kept,
        "obelus wrote over a file it could not read"
    );
    let dump = support::render(&mut app, 66, 12);
    assert!(
        support::text_block(&dump).contains("Not saved"),
        "nothing said the change was not kept:\n{dump}"
    );

    // Fixed in the other window, it reads again and saves again.
    std::fs::write(&file, "theme = \"light\"\n").expect("the other window");
    app.handle(Event::FileChanged { path: file.clone() });
    dispatch::dispatch(&mut app, Command::ThemeSelect);
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
    // A line from a newer obelus, a setting that has been renamed since, and
    // a comment somebody wrote for themselves. None of it is obelus's to
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
    // only what obelus knew about. Onto the reading tab and flip the first
    // switch on it.
    dispatch::dispatch(&mut app, Command::ConfigOpen);
    support::type_text(&mut app, "wrap");
    support::press(&mut app, KeyCode::Enter);

    let written = std::fs::read_to_string(&file).expect("reading it back");
    assert!(
        written.contains("future_setting = 3"),
        "a setting obelus has never heard of was deleted:\n{written}"
    );
    assert!(
        written.contains("blame = false"),
        "a setting under a name obelus has stopped using was deleted:\n{written}"
    );
    assert!(
        written.contains("# mine, do not eat"),
        "somebody's comment was deleted:\n{written}"
    );
    assert!(
        written.contains("wrap = false"),
        "the switch that was flipped was not written:\n{written}"
    );
}

/// The settings say what their keys do, because two of them cannot be
/// guessed: that the page is narrowed by typing at it, and that a setting
/// the tree has set can be taken out again.
#[test]
fn the_settings_say_what_their_keys_do() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let scratch = tree("foot", "wrap = true\n");
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.working_directory_for_test(scratch.path().to_path_buf());
    support::lay_out(&mut app, 76, 16);
    dispatch::dispatch(&mut app, Command::ConfigTree);

    let text = support::text_block(&support::render(&mut app, 76, 16)).to_string();
    // "type" is capped as the key and "to filter" is what it does, so the
    // two are looked for apart. Escape is not here at all: it is on the
    // card, because it means the same thing in every view obelus has.
    for word in ["Change", "type", "to filter", "Unset", "Keys"] {
        assert!(word_on(&text, word), "{word:?} is not at the foot:\n{text}");
    }

    // `f1` says all of them, at length.
    support::press(&mut app, KeyCode::F(1));
    let dump = support::render(&mut app, 76, 16);
    let text = support::text_block(&dump);
    assert!(text.contains("The keys here"), "no card:\n{dump}");
    assert!(
        text.contains("Take this setting out of the tree's file"),
        "the card only has the foot's word for it:\n{dump}"
    );

    // And escape closes the card before it leaves the page.
    support::press(&mut app, KeyCode::Esc);
    assert!(app.settings().is_some(), "escape left the settings");
}

/// `unset` is on the tree's page and nowhere else, because the reader's own
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
            .rows()
            .iter()
            .map(|row| row.setting.key)
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
        "not the times obelus offers"
    );
    assert_eq!(
        picker.selected_item().map(|item| item.label.clone()),
        Some("400".to_string()),
        "the list did not open on the one in force"
    );

    support::type_text(&mut app, "800");
    support::press(&mut app, KeyCode::Enter);
    assert_eq!(
        config::from_toml(&std::fs::read_to_string(&file).expect("the file")).hover_delay,
        800,
        "the file does not say what was chosen"
    );
    assert_eq!(app.config().hover_delay, 800, "obelus is not using it");
}
