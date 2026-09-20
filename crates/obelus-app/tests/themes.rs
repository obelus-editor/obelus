//! Themes read from a file.
//!
//! What is compiled in is two of them. What a reader wants is their own
//! colours, and what a desktop that themes every program it has wants is
//! somewhere to write them -- and neither can be served by a palette that
//! only exists inside the binary.

mod support;

use crossterm::event::KeyCode;
use obelus_app::{
    app::{App, dispatch},
    event::Event,
};
use obelus_command::Command;
use obelus_theme::{builtin, written};

/// A tree with a settings file and a themes directory beside it.
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
    let theme = app.theme_called("mine").expect("the theme");
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

/// A tree may hand one over too, and its own goes over the reader's.
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
        app.theme_called("shared").expect("the theme").background,
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
    let theme = app.theme_called("half").expect("the theme");
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
/// does to obelus: it writes the file. Nobody chose anything, the name in
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

    // Somebody else writes it: another obelus, the reader's own editor, or
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

/// The template shipped for omarchy names colours obelus actually has.
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
    .expect("the template shipped beside obelus");

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
    for (key, colour) in wanted {
        let colour = written::hex(&colour).expect("a colour");
        assert!(
            got.contains(&colour),
            "the template sets a colour obelus does not have: {key}"
        );
    }
}
