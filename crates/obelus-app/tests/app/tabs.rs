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

/// The rows of the list of what is open, by the last part of each label,
/// and the one the reader is on.
fn rows(app: &App) -> (Vec<String>, Option<String>) {
    let picker = app.picker().expect("the list of what is open");
    let name = |label: &str| {
        label
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(label)
            .to_string()
    };
    (
        picker.matches().map(|item| name(&item.label)).collect(),
        picker.selected_item().map(|item| name(&item.label)),
    )
}

/// A tab dragged past another stays where it was put, on the bar and in
/// the list of what is open, and the same move said twice is one move.
///
/// Deliberate breaks: build the tabs from the slots again, ignoring
/// `in_order`, in `App::tabs` -- the bar never moves; and the list's rows
/// from the slots in `App::document_rows` -- the bar moves and the list
/// says the old order.
#[test]
fn a_dragged_tab_stays_where_it_was_put() {
    let scratch = support::Scratch::new("tabs-dragged");
    let one = scratch.write("one.rs", "\n");
    let two = scratch.write("two.rs", "\n");
    let three = scratch.write("three.rs", "\n");
    let (mut app, told) = open(&[one, two, three]);
    app.worktrees_for_test(false);
    support::lay_out(&mut app, 60, 12);

    let (document, beside) = (tab_for(&told, "one.rs"), tab_for(&told, "two.rs"));
    let placed = || Event::Placed {
        document,
        beside,
        after: true,
    };
    app.handle(placed());
    support::lay_out(&mut app, 60, 12);
    assert_eq!(told.last().0, ["two.rs", "one.rs", "three.rs"]);
    // Said again, as a drag says it until the bar has caught up.
    app.handle(placed());
    support::lay_out(&mut app, 60, 12);
    assert_eq!(told.last().0, ["two.rs", "one.rs", "three.rs"]);

    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::DocumentList);
    assert_eq!(rows(&app).0, ["two.rs", "one.rs", "three.rs"]);
}

/// `alt+down` and `alt+up` on the list of what is open move the row the
/// reader is on, and its tab with it, and the reader stays on it.
///
/// Deliberate breaks: leave `moving_key` out of what the list hears, in
/// `App::hear` -- the key walks the selection the way a bare arrow does
/// and nothing moves; and select the row where the list opens rather than
/// the one carried, in `App::moving_key` (`refresh_switching` for
/// `relist_switching`) -- the second press moves the file being read
/// instead.
#[test]
fn alt_and_an_arrow_move_a_row_of_what_is_open() {
    let scratch = support::Scratch::new("tabs-alt-arrows");
    let one = scratch.write("one.rs", "\n");
    let two = scratch.write("two.rs", "\n");
    let three = scratch.write("three.rs", "\n");
    let (mut app, told) = open(&[one, two, three]);
    app.worktrees_for_test(false);
    support::lay_out(&mut app, 60, 12);

    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::DocumentList);
    support::press(&mut app, KeyCode::Down);
    assert_eq!(rows(&app).1.as_deref(), Some("two.rs"));

    support::press_alt_key(&mut app, KeyCode::Down);
    assert_eq!(
        rows(&app),
        (
            vec![
                "one.rs".to_string(),
                "three.rs".to_string(),
                "two.rs".to_string()
            ],
            Some("two.rs".to_string())
        )
    );
    support::press_alt_key(&mut app, KeyCode::Up);
    support::press_alt_key(&mut app, KeyCode::Up);
    assert_eq!(rows(&app).0, ["two.rs", "one.rs", "three.rs"]);
    // Already first: nowhere further up, and still on it.
    support::press_alt_key(&mut app, KeyCode::Up);
    assert_eq!(
        rows(&app),
        (
            vec![
                "two.rs".to_string(),
                "one.rs".to_string(),
                "three.rs".to_string()
            ],
            Some("two.rs".to_string())
        )
    );

    support::lay_out(&mut app, 60, 12);
    assert_eq!(told.last().0, ["two.rs", "one.rs", "three.rs"]);
}

/// With something typed into the list, the rows are in the order of how
/// well they match, so `alt+down` moves nothing -- and does not walk the
/// selection either.
///
/// Deliberate break: drop the query's refusal from `App::moving_key`. The
/// row is moved past whatever ranked under it, which is not the document
/// after it on the bar.
#[test]
fn a_typed_list_moves_nothing() {
    let scratch = support::Scratch::new("tabs-alt-typed");
    let one = scratch.write("one.rs", "\n");
    let two = scratch.write("two.rs", "\n");
    let three = scratch.write("three.rs", "\n");
    let (mut app, told) = open(&[one, two, three]);
    app.worktrees_for_test(false);
    support::lay_out(&mut app, 60, 12);

    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::DocumentList);
    support::type_text(&mut app, "o");
    let before = rows(&app);
    support::press_alt_key(&mut app, KeyCode::Down);
    assert_eq!(rows(&app), before);
    support::press(&mut app, KeyCode::Esc);
    support::lay_out(&mut app, 60, 12);
    assert_eq!(told.last().0, ["one.rs", "two.rs", "three.rs"]);
}

/// Closing the document being read goes to the one before it in the order
/// the reader put them in, not in the order they were opened.
///
/// Deliberate break: go to the nearest slot in `App::close`, as it did
/// before the tabs could move. Closing `three.rs`, put second, then goes
/// to `two.rs`, which was opened before it but is now after it.
#[test]
fn closing_goes_to_the_neighbour_the_reader_sees() {
    let scratch = support::Scratch::new("tabs-closed-moved");
    let one = scratch.write("one.rs", "\n");
    let two = scratch.write("two.rs", "\n");
    let three = scratch.write("three.rs", "\n");
    let (mut app, told) = open(&[one.clone(), two, three]);
    support::lay_out(&mut app, 60, 12);

    app.handle(Event::Placed {
        document: tab_for(&told, "three.rs"),
        beside: tab_for(&told, "one.rs"),
        after: true,
    });
    app.handle(Event::Chose(tab_for(&told, "three.rs")));
    support::lay_out(&mut app, 60, 12);
    assert_eq!(
        told.last(),
        (
            vec![
                "one.rs".to_string(),
                "three.rs".to_string(),
                "two.rs".to_string()
            ],
            Some("three.rs".to_string())
        )
    );

    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::DocumentClose);
    assert_eq!(app.current_buffer().map(Buffer::path), Some(one.as_path()));
}

/// The cross on a tab closes that document, not the one being read.
///
/// Deliberate break: close `self.current` rather than `id` in
/// `App::close_by_its_tab`. The file being read goes and the one whose
/// cross was pressed stays.
#[test]
fn a_tab_s_cross_closes_its_own_document() {
    let scratch = support::Scratch::new("tabs-shut");
    let one = scratch.write("one.rs", "fn one() {}\n");
    let two = scratch.write("two.rs", "fn two() {}\n");
    let (mut app, told) = open(&[one.clone(), two]);
    support::lay_out(&mut app, 60, 12);

    app.handle(Event::Shut(tab_for(&told, "two.rs")));
    support::lay_out(&mut app, 60, 12);
    assert_eq!(told.last().0, ["one.rs"], "the wrong tab went");
    assert_eq!(app.current_buffer().map(Buffer::path), Some(one.as_path()));
}

/// The cross on a tab with unwritten work in it asks before anything
/// goes, the way the key that closes does.
///
/// Deliberate break: leave the `is_dirty` question out of
/// `App::close_by_its_tab`. The file closes at once, and what was typed
/// into it with it.
#[test]
fn a_tab_s_cross_asks_before_losing_unwritten_work() {
    let scratch = support::Scratch::new("tabs-shut-unwritten");
    let one = scratch.write("one.rs", "fn one() {}\n");
    let two = scratch.write("two.rs", "fn two() {}\n");
    let (mut app, told) = open(&[one, two]);
    support::lay_out(&mut app, 60, 12);

    support::type_text(&mut app, "x");
    support::lay_out(&mut app, 60, 12);
    app.handle(Event::Shut(tab_for(&told, "one.rs")));
    support::lay_out(&mut app, 60, 12);
    assert_eq!(app.buffers_for_test().len(), 2, "closed without asking");
    assert!(app.picker().is_some(), "nothing was asked");
}

/// The cross on a tab, with the list of what is open up, takes the row
/// out of the list too.
///
/// Deliberate break: leave `relist_switching` out of
/// `App::close_by_its_tab`. The list goes on showing a row for a document
/// that is no longer open.
#[test]
fn a_tab_s_cross_takes_its_row_out_of_the_list() {
    let scratch = support::Scratch::new("tabs-shut-listed");
    let one = scratch.write("one.rs", "fn one() {}\n");
    let two = scratch.write("two.rs", "fn two() {}\n");
    let (mut app, told) = open(&[one, two]);
    support::lay_out(&mut app, 60, 12);

    app.open_document_picker();
    let rows = |app: &App| app.picker().map(|picker| picker.matches().count());
    assert_eq!(rows(&app), Some(2));
    app.handle(Event::Shut(tab_for(&told, "two.rs")));
    assert_eq!(rows(&app), Some(1), "the closed document's row stayed");
}
