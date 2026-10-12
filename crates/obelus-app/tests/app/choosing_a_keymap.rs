//! Choosing which keys Obelus starts from: the list a first start puts up,
//! and what choosing from it does.
//!
//! Every settings file here is a temporary one, through
//! `config_file_for_test`: a test that let Obelus find its own would be
//! answering the first start's question in the reader's `~/.config`.

use crossterm::event::KeyCode;
use obelus_app::app::App;
use obelus_command::Command;
use obelus_keymap::{Keymap, Layout};
use obelus_ui::Screen as _;
use support::press;

use crate::support;

/// The layouts the list offers, as it calls them, in the order it offers
/// them.
const CALLED: [&str; 4] = ["Obelus", "Mnemonic", "VS Code", "TextMate"];

/// What the list asks, which is how a test tells it from any other list.
const ASKED: &str = "Which keys Obelus starts from";

/// An application on a scratch project, with a settings file of its own
/// that holds `contents` -- or that is not there at all, for `None`.
fn on(scratch: &support::Scratch, contents: Option<&str>) -> App {
    let file = scratch.join("settings").join("config.toml");
    if let Some(contents) = contents {
        std::fs::create_dir_all(file.parent().expect("a directory")).expect("the directory");
        std::fs::write(&file, contents).expect("the settings");
    }
    let mut app = App::new(Vec::new());
    app.working_directory_for_test(scratch.path().to_path_buf());
    app.config_file_for_test(file);
    app
}

/// Where [`on`] keeps the settings.
fn settings(scratch: &support::Scratch) -> std::path::PathBuf {
    scratch.join("settings").join("config.toml")
}

/// Starts it the way `main` does.
fn start(app: &mut App) {
    let (sender, _events) = std::sync::mpsc::channel();
    app.start(sender);
    support::lay_out(app, 100, 30);
}

/// What the list in front is asking, if there is one.
fn asked(app: &App) -> Option<String> {
    app.picker()
        .and_then(|picker| picker.question().map(str::to_string))
}

/// Picks the layout called `called` from the open list.
fn pick(app: &mut App, called: &str) {
    support::type_text(app, called);
    press(app, KeyCode::Enter);
}

/// Every layout is a row, called what it is called, with two keys it would
/// give -- and the keys are that layout's, not the ones in force. What it
/// is, is what the setting says it is.
///
/// Deliberate breaks: building the examples from `self.keymap` instead of
/// the layout's own table -- every row then says `F1`, and the VS Code row
/// does not say the key VS Code puts the files on; and giving the VS Code
/// row a line of its own again in `about_a_layout`.
#[test]
fn the_list_says_what_each_layout_would_give() {
    let scratch = support::Scratch::new("keymap-rows");
    let mut app = on(&scratch, Some("wrap = true\n"));
    support::lay_out(&mut app, 100, 30);
    app.open_keymap_picker();
    let picker = app.picker().expect("the list");
    let rows: Vec<_> = picker.matches().collect();
    let labels: Vec<&str> = rows.iter().map(|row| row.label.as_str()).collect();
    assert_eq!(labels, CALLED);
    for (row, layout) in rows.iter().zip(Layout::ALL) {
        let keymap = Keymap::laid_out(layout);
        let detail = row.detail.as_deref().expect("what it is");
        let written = obelus_config::key_layout(layout.name()).expect("a layout");
        assert_eq!(row.label, written.name);
        assert!(
            detail.starts_with(written.about),
            "{} says something else here than the setting does: {detail}",
            row.label
        );
        for command in [Command::FileOpen, Command::CommandPalette] {
            let key = keymap.chord_for(command).expect("bound").label();
            assert!(
                detail.contains(&key),
                "{} does not say {key} for {}: {detail}",
                row.label,
                command.name()
            );
        }
        assert!(
            detail.starts_with(char::is_uppercase),
            "copy starts with a capital: {detail}"
        );
    }
}

/// Choosing a layout writes it, moves the keys at once, and moves what the
/// welcome screen says they are.
///
/// Deliberate break: `PickerValue::Setting` returning before
/// `change_setting` for `keys_from` -- nothing is written and the keys stay
/// where they were.
#[test]
fn choosing_a_layout_moves_the_keys_and_the_welcome_screen() {
    let scratch = support::Scratch::new("keymap-choose");
    let mut app = on(&scratch, Some("wrap = true\n"));
    support::lay_out(&mut app, 100, 30);
    let before = support::render(&mut app, 100, 30);
    let obelus = Keymap::laid_out(Layout::Obelus);
    let vscode = Keymap::laid_out(Layout::VsCode);
    let opens = |keymap: &Keymap| keymap.chord_for(Command::FileOpen).expect("bound").label();
    assert!(support::text_block(&before).contains(&opens(&obelus)));

    app.open_keymap_picker();
    assert_eq!(asked(&app).as_deref(), Some(ASKED));
    pick(&mut app, "VS Code");

    assert!(app.picker().is_none(), "the list stayed open");
    assert_eq!(app.config().keys_from, "vscode");
    let written = std::fs::read_to_string(settings(&scratch)).expect("the settings");
    assert!(written.contains("keys_from = \"vscode\""), "{written}");
    assert!(written.contains("wrap = true"), "the rest went: {written}");
    assert_eq!(
        app.keymap().chord_for(Command::FileOpen),
        vscode.chord_for(Command::FileOpen)
    );
    let after = support::render(&mut app, 100, 30);
    assert!(
        support::text_block(&after).contains(&opens(&vscode)),
        "the welcome screen still says the old key:\n{after}"
    );
    assert!(
        !support::text_block(&after).contains(&opens(&obelus)),
        "the welcome screen still says the old key:\n{after}"
    );

    // And the key itself does what the screen now says.
    support::press_control(&mut app, 'p');
    assert!(
        app.picker()
            .is_some_and(|picker| picker.question().is_none()
                && picker
                    .invitation()
                    .is_some_and(|said| said.contains("file"))),
        "ctrl+p is not the files under VS Code's keys: {:?}",
        app.picker().and_then(|picker| picker.invitation())
    );
}

/// A first start -- no settings file at all -- asks, over the welcome
/// screen.
///
/// Deliberate break: taking `ask_for_a_keymap` out of `App::start`.
#[test]
fn a_first_start_asks() {
    let scratch = support::Scratch::new("keymap-first");
    let mut app = on(&scratch, None);
    start(&mut app);
    assert_eq!(asked(&app).as_deref(), Some(ASKED));
}

/// A start with a settings file does not ask, whatever is in it: one that
/// names no layout is a reader who has been here before, and one that will
/// not read must not be answered by writing over it.
///
/// Deliberate break: `unchosen` set to `true` whatever was read -- both
/// starts ask.
#[test]
fn a_start_with_settings_does_not_ask() {
    for (name, contents) in [
        ("keymap-has-settings", "wrap = true\n"),
        ("keymap-unreadable", "not toml = [\n"),
    ] {
        let scratch = support::Scratch::new(name);
        let mut app = on(&scratch, Some(contents));
        start(&mut app);
        assert_ne!(asked(&app).as_deref(), Some(ASKED), "{contents:?}");
    }
}

/// Escape leaves the keys as they were and writes nothing, so the next
/// start asks again.
///
/// Deliberate break: writing `obelus` down when the question is put
/// rather than when it is answered -- the file is there after escape.
#[test]
fn escape_keeps_the_defaults_and_writes_nothing() {
    let scratch = support::Scratch::new("keymap-escape");
    let mut app = on(&scratch, None);
    start(&mut app);
    assert_eq!(asked(&app).as_deref(), Some(ASKED));
    press(&mut app, KeyCode::Esc);
    assert!(app.picker().is_none(), "escape did not let it go");
    assert_eq!(app.config().keys_from, "obelus");
    assert!(
        !settings(&scratch).exists(),
        "escape wrote the settings, so nothing will ask again"
    );
}

/// Choosing the default is written down as well, so the next start does
/// not ask -- and a start after that is a start with settings.
///
/// The default is not a line in the file, so what is kept is the file
/// itself: asked for by whether it is there, which is the question a start
/// asks, rather than by its words -- the template's commented-out
/// `# keys_from = "obelus"` would answer for a line that was never written.
///
/// Deliberate break: `PickerValue::Setting` skipping `change_setting` when
/// the word is the one in force -- nothing is written, and the second start
/// asks again.
#[test]
fn choosing_obelus_is_kept_and_not_asked_again() {
    let scratch = support::Scratch::new("keymap-obelus");
    let mut app = on(&scratch, None);
    start(&mut app);
    // Opened on the one in force, which is the default.
    press(&mut app, KeyCode::Enter);
    assert!(
        settings(&scratch).exists(),
        "choosing the default wrote nothing"
    );

    let mut again = on(&scratch, None);
    start(&mut again);
    assert_ne!(asked(&again).as_deref(), Some(ASKED), "it asked again");
}

/// What went wrong on the way up is put up first, and the question waits
/// for a start that has nothing to say first.
///
/// Deliberate break: asking before `tell_what_went_wrong` regardless of
/// whether it told -- the keymap is over the problems.
#[test]
fn what_went_wrong_comes_first() {
    let scratch = support::Scratch::new("keymap-problems");
    std::fs::create_dir_all(scratch.path().join(".obelus")).expect("the directory");
    std::fs::write(
        scratch.path().join(".obelus").join("config.toml"),
        "workflow = \"feature_branch\"\n",
    )
    .expect("the project's settings");
    let mut app = on(&scratch, None);
    start(&mut app);
    assert_eq!(
        asked(&app).as_deref(),
        Some("What went wrong starting up"),
        "the problems are not in front"
    );
    assert!(!settings(&scratch).exists(), "something was written");
}

/// The page asking which project is the whole screen and the more pressing
/// question: the keymap waits for its answer, and comes then.
///
/// Deliberate breaks: taking the chooser out of `ask_for_a_keymap`'s
/// guard -- the list is up under the page at start; and taking
/// `ask_for_a_keymap` out of `settle_on` -- nothing asks once there is a
/// project.
#[test]
fn which_project_comes_first() {
    let scratch = support::Scratch::new("keymap-project");
    let file = settings(&scratch);
    let mut app = App::new(Vec::new());
    app.working_directory_for_test(std::path::PathBuf::from("/tmp/obelus"));
    app.ask_about_these_projects_for_test(vec![obelus_component::chooser::Known {
        path: scratch.path().to_path_buf(),
        shown: scratch.path().display().to_string(),
        last: Some(1),
    }]);
    app.config_file_for_test(file);
    start(&mut app);
    assert!(app.choosing().is_some(), "not asking which project");
    assert!(app.picker().is_none(), "a list is up under the page");

    press(&mut app, KeyCode::Enter);
    assert!(app.choosing().is_none(), "still asking which project");
    assert_eq!(asked(&app).as_deref(), Some(ASKED));
}

/// A start that said what went wrong goes without the question for the
/// whole of its life: choosing a project afterwards does not bring it up.
///
/// Deliberate break: leaving `unchosen` as it was where `start` told --
/// the list came up once the project was chosen.
#[test]
fn a_start_that_told_does_not_ask_later() {
    let scratch = support::Scratch::new("keymap-told");
    let file = settings(&scratch);
    let mut app = App::new(Vec::new());
    app.working_directory_for_test(std::path::PathBuf::from("/tmp/obelus"));
    app.ask_about_these_projects_for_test(vec![obelus_component::chooser::Known {
        path: scratch.path().to_path_buf(),
        shown: scratch.path().display().to_string(),
        last: Some(1),
    }]);
    app.config_file_for_test(file);
    app.amiss("Something went wrong on the way up");
    start(&mut app);
    assert!(
        app.picker()
            .is_some_and(|picker| picker.question() != Some(ASKED)),
        "what went wrong was not put up"
    );

    press(&mut app, KeyCode::Esc);
    assert!(app.choosing().is_some(), "not asking which project");
    press(&mut app, KeyCode::Enter);
    assert!(app.choosing().is_none(), "still asking which project");
    assert_ne!(asked(&app).as_deref(), Some(ASKED), "it asked after all");
}

/// Nobody is at the screen of a headless Obelus to answer.
///
/// Deliberate break: taking `is_headless` out of `ask_for_a_keymap`.
#[test]
fn headless_does_not_ask() {
    let scratch = support::Scratch::new("keymap-headless");
    let mut app = on(&scratch, None);
    app.headless();
    start(&mut app);
    assert!(app.picker().is_none(), "a headless start put a list up");
}

/// A file named on the command line is still asked over, and escape is
/// the way back to it.
///
/// Deliberate break: asking only where `reading_nothing` -- the file
/// start does not ask.
#[test]
fn a_file_is_asked_over() {
    let scratch = support::Scratch::new("keymap-file");
    let path = scratch.join("one.rs");
    std::fs::write(&path, "fn main() {}\n").expect("the file");
    let mut app = App::new(vec![obelus_buffer::Buffer::open(&path).expect("open")]);
    app.working_directory_for_test(scratch.path().to_path_buf());
    app.config_file_for_test(settings(&scratch));
    start(&mut app);
    assert_eq!(asked(&app).as_deref(), Some(ASKED), "not asked over a file");
    press(&mut app, KeyCode::Esc);
    assert!(app.current_buffer().is_some(), "the file went");
}

/// A file that cannot be written is said on the status row, and the keys
/// in force are still the ones chosen for this window.
///
/// Unix only, because what makes the write fail is a directory nobody may
/// write in, and Windows does not refuse a write on that.
///
/// Deliberate break: dropping the `wrong(..)` after `save_to` fails in
/// `change_setting` -- nothing says so.
#[cfg(unix)]
#[test]
fn a_write_that_fails_is_said() {
    use std::os::unix::fs::PermissionsExt as _;

    let scratch = support::Scratch::new("keymap-unwritable");
    // A directory that is there and may not be written in: the file is
    // missing, which is a first start, and nothing can be made beside it.
    let shut = scratch.join("shut");
    std::fs::create_dir_all(&shut).expect("the directory");
    std::fs::set_permissions(&shut, std::fs::Permissions::from_mode(0o555)).expect("shut");
    let mut app = App::new(Vec::new());
    app.working_directory_for_test(scratch.path().to_path_buf());
    app.config_file_for_test(shut.join("config.toml"));
    start(&mut app);
    assert_eq!(asked(&app).as_deref(), Some(ASKED));
    pick(&mut app, "TextMate");
    let said = app.note().map(str::to_string);
    let written = shut.join("config.toml").exists();
    // Opened again before asserting, so a failure leaves nothing behind
    // that the scratch cannot take away.
    std::fs::set_permissions(&shut, std::fs::Permissions::from_mode(0o755)).expect("open");
    assert_eq!(app.config().keys_from, "textmate");
    assert!(
        said.as_deref()
            .is_some_and(|said| said.starts_with("Not saved")),
        "nothing said the choice was not kept: {said:?}"
    );
    assert!(!written, "the directory was written in after all");
}
