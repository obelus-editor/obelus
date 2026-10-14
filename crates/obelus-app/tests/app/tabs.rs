//! What is open, as the tabs a window draws along its title bar.
//!
//! The application says what they are and the window says where they go,
//! so what is held here is the half that is the application's: what each
//! tab is called, which is being read, that it is said when it changes and
//! not on every frame, and that choosing one is choosing that document.

use std::sync::{Arc, Mutex};

use crossterm::event::KeyCode;
use obelus_app::{
    app::{App, Tab},
    event::Event,
};
use obelus_buffer::Buffer;

use crate::support;

/// What a window is told about the tabs, each time it is told.
#[derive(Debug, Default)]
struct Told(Mutex<Vec<Vec<Tab>>>);

impl obelus_app::app::Drawing for Told {
    fn tabs(&self, tabs: &[Tab]) {
        self.0.lock().expect("what was said").push(tabs.to_vec());
    }

    // The rest are other tests' -- see `settings`.
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

impl Told {
    fn said(&self) -> Vec<Vec<Tab>> {
        self.0.lock().expect("what was said").clone()
    }

    /// The last tabs told, as their names and the one being read.
    fn last(&self) -> (Vec<String>, Option<String>) {
        let said = self.said();
        let tabs = said.last().cloned().unwrap_or_default();
        (
            tabs.iter().map(|tab| tab.name.clone()).collect(),
            tabs.iter()
                .find(|tab| tab.reading)
                .map(|tab| tab.name.clone()),
        )
    }
}

/// An application with these files open, drawn by a window that keeps what
/// it is told.
fn open(paths: &[std::path::PathBuf]) -> (App, Arc<Told>) {
    let buffers = paths
        .iter()
        .map(|path| Buffer::open(path).expect("opening it"))
        .collect();
    let mut app = App::new(buffers);
    let told = Arc::new(Told::default());
    app.drawn_by(told.clone());
    (app, told)
}

/// The document a tab is for, by its name.
fn tab_for(told: &Told, name: &str) -> obelus_buffer::DocumentId {
    told.said()
        .last()
        .and_then(|tabs| tabs.iter().find(|tab| tab.name == name))
        .map(|tab| tab.document)
        .unwrap_or_else(|| panic!("no tab says {name}: {:?}", told.last()))
}

/// The window is told what is open, by name and in order, and which of it
/// is being read -- once, and again only when that changes.
///
/// Deliberate break: tell the window whether or not the tabs changed, in
/// `App::tell_the_tabs`. The second frame then says it again, and a window
/// lays the bar out again for nothing on every keypress.
#[test]
fn the_window_is_told_what_is_open_when_it_changes() {
    let scratch = support::Scratch::new("tabs-told");
    let one = scratch.write("one.rs", "fn one() {}\n");
    let two = scratch.write("two.rs", "fn two() {}\n");
    let (mut app, told) = open(&[one, two]);

    support::lay_out(&mut app, 60, 12);
    let (names, reading) = told.last();
    assert_eq!(names, ["one.rs", "two.rs"]);
    assert_eq!(reading.as_deref(), Some("one.rs"));

    support::lay_out(&mut app, 60, 12);
    assert_eq!(told.said().len(), 1, "told again with nothing changed");

    // A change that is not on disk is on its tab.
    support::type_text(&mut app, "x");
    support::lay_out(&mut app, 60, 12);
    let said = told.said();
    assert_eq!(said.len(), 2, "not told of the change");
    assert!(said[1][0].unwritten, "the change is not on the tab");
}

/// Choosing a tab goes to its document, with the jump recorded so that
/// `alt+left` comes back.
///
/// Deliberate break: leave the `record` out of `App::choose_by_its_tab`.
/// The tab still goes there, and `alt+left` then has nowhere to go back to.
#[test]
fn choosing_a_tab_goes_to_its_document_and_back() {
    let scratch = support::Scratch::new("tabs-chosen");
    let one = scratch.write("one.rs", "fn one() {}\n");
    let two = scratch.write("two.rs", "fn two() {}\n");
    let (mut app, told) = open(&[one.clone(), two.clone()]);
    support::lay_out(&mut app, 60, 12);

    app.handle(Event::Chose(tab_for(&told, "two.rs")));
    support::lay_out(&mut app, 60, 12);
    assert_eq!(
        app.current_buffer().map(Buffer::path),
        Some(two.as_path()),
        "the tab did not go to its file"
    );
    assert_eq!(told.last().1.as_deref(), Some("two.rs"));

    support::press_alt_key(&mut app, KeyCode::Left);
    assert_eq!(
        app.current_buffer().map(Buffer::path),
        Some(one.as_path()),
        "alt+left did not come back"
    );
}

/// A tab chosen with a list open over the file puts the list away, the
/// way a key naming another view does.
///
/// Deliberate break: leave `put_away_the_views` out of
/// `App::choose_by_its_tab`. The file changes under the palette, which
/// stays up over it.
#[test]
fn a_tab_puts_away_the_list_over_the_file() {
    let scratch = support::Scratch::new("tabs-over-a-list");
    let one = scratch.write("one.rs", "fn one() {}\n");
    let two = scratch.write("two.rs", "fn two() {}\n");
    let (mut app, told) = open(&[one.clone(), two.clone()]);
    support::lay_out(&mut app, 60, 12);

    support::press_control(&mut app, 'p');
    assert!(app.picker().is_some(), "the palette did not open");
    app.handle(Event::Chose(tab_for(&told, "two.rs")));
    assert!(app.picker().is_none(), "the palette stayed up");
    assert_eq!(app.current_buffer().map(Buffer::path), Some(two.as_path()));
}

/// A tab chosen while a question on the status row is being typed into
/// leaves the question, and what is typed in it, where they are.
///
/// Deliberate break: leave the `gives_way_to_a_view` refusal out of
/// `App::choose_by_its_tab`. The question stays -- putting views away does
/// not reach it -- and the file it was asked about changes under it, so
/// the answer goes somewhere the reader was not looking when they typed it.
#[test]
fn a_tab_does_not_take_a_question_away() {
    let scratch = support::Scratch::new("tabs-over-a-question");
    let one = scratch.write("one.rs", "fn one() {}\n");
    let two = scratch.write("two.rs", "fn two() {}\n");
    let (mut app, told) = open(&[one.clone(), two]);
    support::lay_out(&mut app, 60, 12);

    app.new_file();
    support::type_text(&mut app, "half");
    app.handle(Event::Chose(tab_for(&told, "two.rs")));
    assert!(
        app.prompt()
            .is_some_and(|prompt| prompt.text().ends_with("half")),
        "the question went"
    );
    assert_eq!(
        app.current_buffer().map(Buffer::path),
        Some(one.as_path()),
        "the tab went to its file under the question"
    );
}

/// A project let go of takes its tabs off the bar: the window is told
/// there is nothing open, though the application that tells it was made
/// again from nothing.
///
/// Deliberate break: leave `told_tabs` out of what
/// `App::let_go_of_the_project` keeps. The new application has told
/// nothing, sees nothing open, takes that for what the window was told,
/// and the old project's tabs stay up over the page that asks which
/// project is next.
#[test]
fn a_project_let_go_of_takes_its_tabs() {
    let scratch = support::Scratch::new("tabs-let-go");
    let one = scratch.write("one.rs", "fn one() {}\n");
    let (mut app, told) = open(&[one]);
    support::lay_out(&mut app, 60, 12);
    assert_eq!(told.last().0, ["one.rs"]);

    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::ProjectClose);
    support::lay_out(&mut app, 60, 12);
    assert!(
        told.last().0.is_empty(),
        "the old tabs stayed: {:?}",
        told.last()
    );
}

/// Two files with one name say which directory each is in; a file whose
/// name is its own says only that.
///
/// Deliberate break: name every file's tab by its name alone. The two tabs
/// both say `mod.rs`, and the reader cannot tell which is which.
#[test]
fn two_files_with_one_name_say_their_directories() {
    let scratch = support::Scratch::new("tabs-named");
    let first = scratch.write("app/mod.rs", "\n");
    let second = scratch.write("ui/mod.rs", "\n");
    let alone = scratch.write("ui/lib.rs", "\n");
    let (mut app, told) = open(&[first, second, alone]);
    support::lay_out(&mut app, 60, 12);
    assert_eq!(told.last().0, ["app/mod.rs", "ui/mod.rs", "lib.rs"]);
}
