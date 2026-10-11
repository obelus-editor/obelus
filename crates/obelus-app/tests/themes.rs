//! Themes read from a file.
//!
//! What is compiled in is a shelf of them, and a shelf is still a shelf:
//! what a reader wants is their own colours, and what a desktop that
//! themes every program it has wants is somewhere to write them -- and
//! neither can be served by a palette that only exists inside the binary,
//! however many are on it.

mod support;

use crossterm::event::KeyCode;
use obelus_app::{
    app::{App, dispatch},
    event::Event,
};
use obelus_command::Command;
use obelus_theme::{builtin, written};

/// A project with a settings file and a themes directory beside it.
fn reader(name: &str) -> support::Scratch {
    let scratch = support::Scratch::new(&format!("theme-{name}"));
    std::fs::create_dir_all(scratch.join("themes")).expect("the directory");
    scratch
}

fn open(scratch: &support::Scratch) -> App {
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.config_file_for_test(scratch.join("config.toml"));
    support::lay_out(&mut app, 70, 14);
    app
}

/// A theme file gives what it wants and inherits the rest.
///
/// Broken deliberately by requiring every colour: thirty-nine of them is a
/// format nobody writes by hand and a template nobody can generate, because
/// the thing generating one knows a dozen colours by a semantic name and
/// nothing at all about which of them a comment should be.
#[test]
fn a_theme_file_says_what_it_wants_and_inherits_the_rest() {
    let scratch = reader("inherit");
    std::fs::write(
        scratch.join("themes/mine.toml"),
        "base = \"light\"\nbackground = \"#121212\"\n\n[syntax]\nkeyword = \"#d35f5f\"\n",
    )
    .expect("the theme");

    let mut app = open(&scratch);
    let theme = app.theme_called("mine").expect("the theme").colours;
    assert_eq!(
        theme.background,
        written::hex("#121212").expect("a colour"),
        "what the file said was not taken"
    );
    assert_eq!(
        theme.syntax.keyword,
        written::hex("#d35f5f").expect("a colour")
    );
    // And everything it did not say comes from the theme it builds on --
    // the light one, because that is what it asked for.
    assert_eq!(theme.gutter, builtin::LIGHT.gutter, "the base was not used");
    assert_eq!(theme.syntax.comment, builtin::LIGHT.syntax.comment);
}

/// The themes on offer are the files and the built-in ones, files first.
#[test]
fn the_list_offers_the_files_beside_the_built_in_ones() {
    let scratch = reader("list");
    std::fs::write(scratch.join("themes/omarchy.toml"), "base = \"dark\"\n").expect("the theme");

    let mut app = open(&scratch);
    dispatch::dispatch(&mut app, Command::ThemeSelect);
    let dump = support::render(&mut app, 70, 14);
    let text = support::text_block(&dump);
    for name in ["omarchy", "dark", "light"] {
        assert!(text.contains(name), "no {name} in the list:\n{dump}");
    }
}

/// Walking to one wears it, choosing it keeps it, and escape puts back what
/// was there -- the same three the built-in ones always had.
#[test]
fn a_file_theme_previews_and_is_kept() {
    let scratch = reader("keep");
    std::fs::write(
        scratch.join("themes/omarchy.toml"),
        "base = \"dark\"\nbackground = \"#121212\"\n",
    )
    .expect("the theme");
    let page = written::hex("#121212").expect("a colour");

    let mut app = open(&scratch);
    dispatch::dispatch(&mut app, Command::ThemeSelect);
    // The list opens on the one in force, and the file's sorts above it.
    support::press(&mut app, KeyCode::Up);
    let _ = support::render(&mut app, 70, 14);
    assert_eq!(
        app.theme().background,
        page,
        "walking to it did not wear it"
    );

    support::press(&mut app, KeyCode::Esc);
    let _ = support::render(&mut app, 70, 14);
    assert_eq!(
        app.theme().background,
        builtin::DARK.background,
        "escape kept the preview"
    );

    dispatch::dispatch(&mut app, Command::ThemeSelect);
    support::press(&mut app, KeyCode::Up);
    support::press(&mut app, KeyCode::Enter);
    assert_eq!(app.theme_name(), "omarchy");
    assert_eq!(app.theme().background, page);
    // And it is written down, so tomorrow is the same colour.
    let written = std::fs::read_to_string(scratch.join("config.toml")).expect("the file");
    assert!(
        written.contains("theme = \"omarchy\""),
        "the choice was not kept: {written}"
    );
}

/// A project may hand one over too, and its own goes over the reader's.
///
/// The same order the settings themselves are laid in: what a project says
/// about itself goes over what the reader says about everything. A theme is
/// only colours -- there is no code in one -- so it is handed over on the
/// same terms as a wrapped line.
#[test]
fn a_tree_can_carry_a_theme_of_its_own() {
    let scratch = reader("tree-readers");
    std::fs::write(
        scratch.join("themes/shared.toml"),
        "base = \"dark\"\nbackground = \"#010101\"\n",
    )
    .expect("the reader's");

    let tree = support::Scratch::new("theme-tree");
    std::fs::create_dir_all(tree.join(".obelus/themes")).expect("the directory");
    std::fs::write(tree.join(".obelus/config.toml"), "wrap = true\n").expect("the settings");
    std::fs::write(
        tree.join(".obelus/themes/shared.toml"),
        "base = \"dark\"\nbackground = \"#020202\"\n",
    )
    .expect("the tree's");

    let mut app = open(&scratch);
    app.working_directory_for_test(tree.path().to_path_buf());
    assert_eq!(
        app.theme_called("shared")
            .expect("the theme")
            .colours
            .background,
        written::hex("#020202").expect("a colour"),
        "the reader's file won over the tree's own"
    );
}

/// A file that will not read leaves the colours alone and says so.
///
/// A theme thrown away over one bad line would be a screen the reader
/// cannot use to find the bad line.
#[test]
fn a_theme_that_will_not_read_leaves_the_screen_alone() {
    let scratch = reader("broken");
    std::fs::write(scratch.join("themes/broken.toml"), "base = \"dark\"\nthis(").expect("the file");

    let mut app = open(&scratch);
    assert!(app.theme_called("broken").is_none());
    assert_eq!(app.theme().background, builtin::DARK.background);

    // And one bad *colour* is one line that did nothing, not a theme thrown
    // away: the rest of the file is still colours.
    std::fs::write(
        scratch.join("themes/half.toml"),
        "base = \"dark\"\nbackground = \"blue\"\nforeground = \"#abcdef\"\n",
    )
    .expect("the file");
    let theme = app.theme_called("half").expect("the theme").colours;
    assert_eq!(
        theme.background,
        builtin::DARK.background,
        "a word was read as a colour"
    );
    assert_eq!(theme.foreground, written::hex("#abcdef").expect("a colour"));
}

/// `#abc` is `#aabbcc`, which is what a person types.
#[test]
fn a_colour_is_written_the_way_everybody_writes_one() {
    assert_eq!(written::hex("#abcdef"), written::hex("#abcdef"));
    assert_eq!(written::hex("#fff"), written::hex("#ffffff"));
    assert_eq!(written::hex("#012"), written::hex("#001122"));
    // And nothing else: a named colour would be the terminal's sixteen, and
    // a theme reaching for those cannot promise the same file looks the same
    // twice.
    assert!(written::hex("blue").is_none());
    assert!(written::hex("#12345").is_none());
    assert!(written::hex("#gggggg").is_none());
}

/// A theme rewritten on disk is a theme the screen is already wearing.
///
/// Which is the whole of what a desktop that themes every program it has
/// does to Obelus: it writes the file. Nobody chose anything, the name in
/// the settings has not moved, and what that name stands for has.
#[test]
fn a_theme_rewritten_on_disk_arrives_here() {
    let scratch = reader("rewritten");
    let file = scratch.join("themes/omarchy.toml");
    std::fs::write(&file, "base = \"dark\"\nbackground = \"#121212\"\n").expect("the theme");
    std::fs::write(scratch.join("config.toml"), "theme = \"omarchy\"\n").expect("the settings");

    let mut app = open(&scratch);
    assert_eq!(
        app.theme().background,
        written::hex("#121212").expect("a colour"),
        "the file was not read at startup"
    );

    // Somebody else writes it: another Obelus, the reader's own editor, or
    // the thing that themes everything on their desktop.
    std::fs::write(&file, "base = \"dark\"\nbackground = \"#241f31\"\n").expect("rewriting");
    app.handle(Event::Watched(obelus_watch::Changed { path: file }));
    assert_eq!(
        app.theme().background,
        written::hex("#241f31").expect("a colour"),
        "the rewritten theme did not arrive"
    );
}

/// And so is one whose whole directory was replaced under it.
///
/// A theme file is often a link into a directory something else owns, and
/// what rewrites it replaces that directory rather than the file: a change
/// arrives on a path inside the directory the link points into, which a
/// watch on the link's own would never have been looking at.
///
/// Unix only, because the link is: making one on Windows wants a privilege
/// an ordinary test run has not got, and a link is the whole of what this
/// covers. Written without the guard, it was not a test that skipped --
/// `std::os::unix` is not there to call, so the file did not compile and
/// the other nine tests in it did not run either.
#[cfg(unix)]
#[test]
fn a_theme_whose_directory_is_replaced_arrives_here() {
    let scratch = reader("replaced");
    let state = support::Scratch::new("theme-state");
    std::fs::create_dir_all(state.join("current/theme")).expect("the directory");
    let real = state.join("current/theme/obelus.toml");
    std::fs::write(&real, "base = \"dark\"\nbackground = \"#121212\"\n").expect("the theme");
    // Spelled the way the disk spells it, which is the way a watcher
    // reports it: on a mac the temporary directory is itself behind a
    // link, `/var` to `/private/var`, and no watcher says `/var`.
    let real = real.canonicalize().expect("the theme");
    std::os::unix::fs::symlink(&real, scratch.join("themes/omarchy.toml")).expect("the link");
    std::fs::write(scratch.join("config.toml"), "theme = \"omarchy\"\n").expect("the settings");

    let mut app = open(&scratch);
    assert_eq!(
        app.theme().background,
        written::hex("#121212").expect("a colour")
    );

    // Staged beside it and moved into place, which is how a theme is
    // swapped whole: the directory the link points into is a different
    // directory afterwards, and the change arrives on a path inside it.
    std::fs::create_dir_all(state.join("next-theme")).expect("the directory");
    std::fs::write(
        state.join("next-theme/obelus.toml"),
        "base = \"dark\"\nbackground = \"#241f31\"\n",
    )
    .expect("the theme");
    std::fs::remove_dir_all(state.join("current/theme")).expect("the old one");
    std::fs::rename(state.join("next-theme"), state.join("current/theme")).expect("the swap");

    app.handle(Event::Watched(obelus_watch::Changed { path: real }));
    assert_eq!(
        app.theme().background,
        written::hex("#241f31").expect("a colour"),
        "the swapped theme did not arrive"
    );
}

/// The template shipped for omarchy names colours Obelus actually has.
///
/// A key it spells wrong is a line that does nothing: a theme file gives
/// what it wants and inherits the rest, so a misspelling is indentical to
/// having left the colour out -- silently, and in a file nobody reads
/// except the thing rendering it.
///
/// So every placeholder in it is filled with a colour of its own, and every
/// one of those colours has to turn up somewhere in the theme that comes
/// out.
#[test]
fn the_template_for_omarchy_names_colours_obelus_has() {
    let template = std::fs::read_to_string(
        std::path::Path::new(env!("OBELUS_TREE")).join("contrib/omarchy/obelus.toml.tpl"),
    )
    .expect("the template shipped beside Obelus");

    // A different colour per placeholder, so each can be looked for. `mode`
    // is not one: it names the theme to build on, and is the one place the
    // template says a word rather than a colour.
    let mut said = template.clone();
    let mut wanted = Vec::new();
    let mut at = 0u8;
    while let Some(start) = said.find("{{ ") {
        let end = said[start..].find(" }}").expect("a closed placeholder") + start + 3;
        let key = said[start + 3..end - 3].to_string();
        let filled = match key.as_str() {
            "mode" => "dark".to_string(),
            _ => {
                at += 1;
                let colour = format!("#{at:02x}{at:02x}{at:02x}");
                wanted.push((key, colour.clone()));
                colour
            }
        };
        said.replace_range(start..end, &filled);
    }
    assert!(wanted.len() > 10, "the template filled in almost nothing");

    let table: toml::Table = said.parse().expect("the rendered template is not toml");
    let theme = written::over(&table);
    // Every colour the template put in, somewhere in what came out.
    let mut got = vec![
        theme.background,
        theme.foreground,
        theme.gutter,
        theme.gutter_current,
        theme.scrollbar_track,
        theme.status_foreground,
        theme.status_stale,
        theme.selected_row_background,
        theme.raised_background,
        theme.picker_match_background,
        theme.marked_background,
        theme.selection_background,
        theme.change_added,
        theme.change_modified,
        theme.change_removed,
        theme.change_added_background,
        theme.change_modified_background,
        theme.change_removed_background,
        theme.bracket_background,
    ];
    let syntax = theme.syntax;
    got.extend([
        syntax.attribute,
        syntax.boolean,
        syntax.comment,
        syntax.constant,
        syntax.constructor,
        syntax.escape,
        syntax.function,
        syntax.keyword,
        syntax.label,
        syntax.number,
        syntax.operator,
        syntax.property,
        syntax.punctuation,
        syntax.string,
        syntax.type_name,
        syntax.variable,
        syntax.error,
        syntax.warning,
    ]);
    // And the sixteen a terminal's program names by number, as the file
    // gave them: one it spelt wrong is worked out instead, and its own
    // colour is then nowhere.
    got.extend(theme.terminal.iter().flatten());
    for (key, colour) in wanted {
        let colour = written::hex(&colour).expect("a colour");
        assert!(
            got.contains(&colour),
            "the template sets a colour Obelus does not have: {key}"
        );
    }
}

/// A theme that is a directory, with a picture named from inside it.
fn rain(scratch: &support::Scratch, wallpaper: &str) -> std::path::PathBuf {
    std::fs::create_dir_all(scratch.join("themes/rain")).expect("the directory");
    let file = scratch.join("themes/rain/theme.toml");
    std::fs::write(
        &file,
        format!("base = \"dark\"\nbackground = \"#101820\"\n\n[wallpaper]\n{wallpaper}"),
    )
    .expect("the theme");
    // What is in it is the window's to decode, and a test has no window:
    // that there is a file is all the application asks.
    std::fs::write(scratch.join("themes/rain/rain.jpg"), "not decoded here").expect("the picture");
    file
}

/// A theme may be a directory, and carries the picture its file names,
/// found beside it and laid as it says.
///
/// Deliberate breaks: `found_in` taking only `.toml` files (there is then
/// no `rain` to offer), and the picture's path taken from beside the
/// settings rather than beside the theme (it is then not there, and the
/// theme carries none).
#[test]
fn a_theme_carries_the_picture_its_file_names() {
    use obelus_theme::wallpaper::{Anchor, Edge, Fit};
    let scratch = reader("wallpaper");
    let file = rain(
        &scratch,
        "file = \"rain.jpg\"\nopacity = 70\nfit = \"tile\"\nanchor = \"top-left\"\n",
    );

    let mut app = open(&scratch);
    assert!(
        app.themes().contains(&"rain".to_string()),
        "no rain in {:?}",
        app.themes()
    );
    let worn = app.theme_called("rain").expect("the theme");
    assert_eq!(
        worn.colours.background,
        written::hex("#101820").expect("a colour")
    );
    let hung = worn.wallpaper.expect("no picture under the page");
    assert_eq!(
        hung.file.canonicalize().expect("the picture"),
        scratch
            .join("themes/rain/rain.jpg")
            .canonicalize()
            .expect("the picture"),
    );
    assert_eq!(hung.opacity, 70);
    assert_eq!(hung.fit, Fit::Tile);
    assert_eq!(
        hung.anchor,
        Anchor {
            across: Edge::Start,
            down: Edge::Start
        }
    );
    assert_eq!(app.marks_on_for_test(&file).0, 0, "a good theme was marked");
}

/// A line of the picture's table Obelus cannot use is marked where it is,
/// and the picture is laid as though it had not been written; a picture
/// that is not there is marked, and there is none.
///
/// Deliberate breaks: the troubles left out of `theme_called` (nothing is
/// marked), and a misspelt fit refusing the picture (there is then none,
/// where the reader is owed one in the middle while they fix the word).
#[test]
fn a_line_of_a_wallpaper_that_does_nothing_is_marked() {
    use obelus_theme::wallpaper::{Anchor, Fit, OPACITY};
    let scratch = reader("wallpaper-wrong");
    let file = rain(
        &scratch,
        "file = \"rain.jpg\"\nopacity = 150\nfit = \"tiled\"\nanchor = \"middle\"\n",
    );

    let mut app = open(&scratch);
    let hung = app
        .theme_called("rain")
        .expect("the theme")
        .wallpaper
        .expect("the picture was refused over a misspelt word");
    assert_eq!(
        (hung.opacity, hung.fit, hung.anchor),
        (OPACITY, Fit::Cover, Anchor::default())
    );
    assert_eq!(
        app.marks_on_for_test(&file).0,
        3,
        "not every bad line was marked"
    );

    rain(&scratch, "file = \"gone.jpg\"\n");
    let worn = app.theme_called("rain").expect("the theme");
    assert!(
        worn.wallpaper.is_none(),
        "a picture that is not there was hung"
    );
    assert_eq!(
        app.marks_on_for_test(&file).0,
        1,
        "the missing picture was not said"
    );
}

/// A picture's table that names no file is marked, on the table's own
/// name, and there is no picture.
///
/// The mark is made where toml_edit says the key `wallpaper` is, and a
/// mark with nowhere to go is dropped without a word -- so this is what
/// says there is somewhere.
///
/// Deliberate break: the span looked for on the table's value, which a
/// table has none of, rather than its key -- nothing is then marked.
#[test]
fn a_wallpaper_that_names_no_file_is_marked() {
    let scratch = reader("wallpaper-no-file");
    let file = rain(&scratch, "opacity = 50\n");

    let mut app = open(&scratch);
    let worn = app.theme_called("rain").expect("the theme");
    assert!(worn.wallpaper.is_none(), "a picture with no file was hung");
    assert_eq!(
        app.marks_on_for_test(&file).0,
        1,
        "nothing said it names no file"
    );
}

/// Where a file and a directory answer to one name, the directory is the
/// theme, whatever order the disk lists them in -- and the name is offered
/// once.
///
/// Asked of `found_in` itself, because the list a reader is shown drops a
/// name it has seen already: it said "once" with the clash still there,
/// and which of the two was worn was whatever the disk listed first --
/// the directory, on the machine this was written on, so the theme the
/// reader gets passed too.
///
/// Deliberate break: the sort by name alone, as it was, which leaves both
/// in the list in the order `read_dir` gave them.
#[test]
fn a_directory_and_a_file_of_one_name_are_the_directory() {
    let scratch = reader("wallpaper-clash");
    rain(&scratch, "file = \"rain.jpg\"\n");
    std::fs::write(
        scratch.join("themes/rain.toml"),
        "base = \"dark\"\nbackground = \"#ffffff\"\n",
    )
    .expect("the file");

    let found = written::found_in(&scratch.join("themes"));
    let rains: Vec<_> = found.iter().filter(|(name, _)| name == "rain").collect();
    assert_eq!(rains.len(), 1, "one name found {} times", rains.len());
    assert!(
        rains[0].1.ends_with("rain/theme.toml"),
        "the file was taken over the directory: {}",
        rains[0].1.display()
    );

    let mut app = open(&scratch);
    let worn = app.theme_called("rain").expect("the theme");
    assert_eq!(
        worn.colours.background,
        written::hex("#101820").expect("a colour"),
        "the file was taken over the directory"
    );
}

/// Letting go of a project keeps the picture under the page with the rest
/// of the theme, which belongs to the reader and not to the project -- and
/// says nothing to the window, which already has it.
///
/// Deliberate break: `let_go_of_the_project` keeping the colours and not
/// the picture, which leaves the application believing there is none (and
/// tells the window the same picture again when the settings are applied).
#[test]
fn letting_go_of_the_project_keeps_the_picture() {
    let scratch = reader("wallpaper-kept");
    rain(&scratch, "file = \"rain.jpg\"\n");
    std::fs::write(scratch.join("config.toml"), "theme = \"rain\"\n").expect("the settings");

    let mut app = open(&scratch);
    let told = std::sync::Arc::new(Hangs::default());
    app.drawn_by(told.clone());
    assert!(app.wallpaper().is_some(), "no picture to begin with");

    dispatch::dispatch(&mut app, Command::ProjectClose);
    assert_eq!(app.theme_name(), "rain");
    assert!(
        app.wallpaper().is_some(),
        "the picture went with the project"
    );
    assert_eq!(
        told.said().len(),
        1,
        "the window was told again: {:?}",
        told.said()
    );
}

/// What a window is told about the picture under the page.
#[derive(Debug, Default)]
struct Hangs(std::sync::Mutex<Vec<Option<std::path::PathBuf>>>);

impl obelus_app::app::Drawing for Hangs {
    fn hang(&self, wallpaper: Option<&obelus_theme::Wallpaper>) {
        self.0
            .lock()
            .expect("what was said")
            .push(wallpaper.map(|hung| hung.file.clone()));
    }

    // The rest are other tests' -- see `settings`.
    fn text_size(&self, _: usize) {}

    fn animates(&self, _: bool) {}

    fn use_fonts(&self, _: &[String]) {}

    fn drawn_on(&self, _: ratatui::style::Color) {}

    fn holding(&self, _: ratatui::style::Color, _: ratatui::style::Color) {}

    fn caret_is(
        &self,
        _: obelus_app::app::Caret,
        _: Option<obelus_component::layers::Layer>,
        _: bool,
    ) {
    }
}

impl Hangs {
    fn said(&self) -> Vec<Option<std::path::PathBuf>> {
        self.0.lock().expect("what was said").clone()
    }
}

/// The window is told which picture is under the page when it says who it
/// is, when a theme walked to in the list has none, and again when escape
/// puts the old theme back -- and not again on every frame the list is
/// open, because a window told about a picture reads and decodes it.
///
/// Deliberate breaks: the picture left out of what the list keeps to put
/// back (escape then leaves the page with none), and `set_theme` telling
/// the window every time (the list's frames then each say it again).
#[test]
fn the_window_is_told_which_picture_is_under_the_page() {
    let scratch = reader("wallpaper-told");
    rain(&scratch, "file = \"rain.jpg\"\n");
    std::fs::write(scratch.join("config.toml"), "theme = \"rain\"\n").expect("the settings");

    let mut app = open(&scratch);
    let told = std::sync::Arc::new(Hangs::default());
    app.drawn_by(told.clone());
    let picture = app.wallpaper().expect("the picture").file.clone();
    assert_eq!(
        told.said(),
        vec![Some(picture.clone())],
        "not told on the way up"
    );

    // The list opens on rain, and the built-in themes come after the files.
    dispatch::dispatch(&mut app, Command::ThemeSelect);
    support::press(&mut app, KeyCode::Down);
    let _ = support::render(&mut app, 70, 14);
    let _ = support::render(&mut app, 70, 14);
    assert_eq!(
        told.said(),
        vec![Some(picture.clone()), None],
        "walking to dark"
    );

    support::press(&mut app, KeyCode::Esc);
    let _ = support::render(&mut app, 70, 14);
    assert_eq!(
        told.said(),
        vec![Some(picture.clone()), None, Some(picture)],
        "escape did not put the picture back"
    );
}
