//! What is drawn with the glyphs on.
//!
//! The glyphs are off until the reader turns them on, so every other binary
//! draws the plain marks and its fixtures hold those. The tests here are
//! about the glyphs themselves -- which picture a row wears -- and there is
//! nothing to say about that with them off.
//!
//! A binary of its own because the switch is process-wide: turned on in a
//! test beside the fixtures, it would move underneath whichever of them was
//! drawing at the time. Nothing in here turns it off.
//!
//! Broken deliberately by leaving the switch where the settings put it:
//! the three about a glyph that should be there go red, which is what they
//! did in the binaries they came from once the glyphs were off by default.
//! The two about a glyph that should *not* be there -- in the query, in the
//! palette -- pass either way, which is why they are here: off, there is no
//! glyph to keep out, and they would pass whatever obelus did.

mod support;

use crossterm::event::KeyCode;
use obelus_app::{app::App, event::Event};
use obelus_buffer::Buffer;
use serde_json::json;
use support::{press, press_control, press_function, type_text};

/// An application with the glyphs on, over the fixture file.
fn app() -> App {
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    // After the application, which applies its settings as it starts and
    // so puts the switch where their default is.
    obelus_icons::use_glyphs(true);
    // A clean tree, whatever the checkout these tests are running in looks
    // like: the file list grows a tab for the changed files when there are
    // any, and a test of how a *row* is drawn should not depend on whether
    // someone is working in the repository.
    app.statuses_for_test(std::collections::HashMap::new());
    app
}

/// The same, over a file of its own, for a test that types into it.
fn editing(name: &str, contents: &str) -> (support::Scratch, App) {
    let scratch = support::Scratch::new(name);
    let path = scratch.path().join("sample.rs");
    std::fs::write(&path, contents).expect("writing the file");
    let mut app = App::new(vec![Buffer::open(&path).expect("opening it")]);
    app.working_directory_for_test(scratch.path().to_path_buf());
    obelus_icons::use_glyphs(true);
    support::lay_out(&mut app, 60, 16);
    (scratch, app)
}

/// A file's glyph goes in its own two columns before the name, so the query
/// never matches it and the matched characters of the name still line up.
#[test]
fn the_file_picker_shows_a_glyph_for_each_file() {
    let mut app = app();
    press_function(&mut app, 1);
    app.handle(Event::Search(obelus_search::Event::FilesFound {
        generation: 1,
        paths: vec![
            "src/app.rs".into(),
            "Cargo.toml".into(),
            "mystery.qqq".into(),
        ],
        ignored: false,
    }));
    // The flat listing, which is what a query is asked against: a file
    // list is a tree until the reader says what they are after.
    type_text(&mut app, "r");

    let dump = support::render(&mut app, 40, 8);
    let text = support::text_block(&dump);

    // The Rust glyph, the config glyph, and the generic one.
    assert!(text.contains('\u{e7a8}'), "no Rust glyph:\n{dump}");
    assert!(text.contains('\u{e615}'), "no config glyph:\n{dump}");
    assert!(text.contains('\u{f15b}'), "no generic glyph:\n{dump}");
}

/// The picture on a row is the one thing that separates two candidates a
/// colour cannot: a module and a keyword are both keyword-coloured.
#[test]
fn the_picture_on_a_row_says_what_the_candidate_is() {
    let (_scratch, mut app) = editing("complete-pictures", "fn main() {\n    p\n}\n");
    support::press(&mut app, crossterm::event::KeyCode::Down);
    support::press(&mut app, crossterm::event::KeyCode::End);
    app.complete_for_test(json!([
        { "label": "path", "kind": 9 },
        { "label": "pub", "kind": 14 },
    ]));

    let dump = support::render(&mut app, 60, 16);
    let rows: Vec<&str> = support::text_block(&dump).lines().collect();
    let picture = |name: &str| {
        rows.iter()
            .find(|row| row.contains(name))
            .and_then(|row| row.chars().find(|cell| *cell as u32 >= 0xf0000))
            .unwrap_or_else(|| panic!("{name} has no picture:\n{dump}"))
    };
    assert_ne!(
        picture("path"),
        picture("pub"),
        "a module and a keyword are drawn alike:\n{dump}"
    );
}

/// And the glyph wears the name's colour. The icon is part of the name: a
/// file git says has changed is a changed file picture and all, and a glyph
/// left in the plain foreground reads as a second thing on the row.
#[test]
fn a_glyph_is_the_colour_of_the_name_beside_it() {
    use obelus_git::FileStatus;

    let mut app = app();
    let root = app.working_directory().to_path_buf();
    // Two statuses, so the colours are two: a test where every row is the
    // same colour cannot tell the name's colour from the plain one.
    app.statuses_for_test(
        [
            (root.join("new.rs"), FileStatus::New.into()),
            (root.join("old.rs"), FileStatus::Changed.into()),
        ]
        .into_iter()
        .collect(),
    );
    support::lay_out(&mut app, 60, 12);
    press_function(&mut app, 1);
    // The changed listing, which is the one whose rows git has coloured.
    press(&mut app, KeyCode::Tab);

    let dump = support::render(&mut app, 60, 12);
    let text: Vec<&str> = support::text_block(&dump).lines().collect();
    let styles: Vec<&str> = support::style_block(&dump).lines().collect();

    // Where the glyph is and where the name starts, read off the row itself
    // rather than counted out here: what this is about is the two wearing
    // one colour, not which column either is in.
    let colours = |name: &str| {
        let row = text
            .iter()
            .position(|row| row.contains(name))
            .unwrap_or_else(|| panic!("no row for {name}:\n{dump}"));
        let glyph = text[row]
            .char_indices()
            .find(|(_, character)| ('\u{e000}'..='\u{f8ff}').contains(character))
            .map(|(index, _)| text[row][..index].chars().count())
            .unwrap_or_else(|| panic!("no glyph on the row for {name}:\n{dump}"));
        let label = text[row]
            .find(name)
            .map(|index| text[row][..index].chars().count())
            .expect("the name");
        let at = |column: usize| styles[row].chars().nth(column).unwrap_or(' ');
        (at(glyph), at(label))
    };

    let (new_glyph, new_label) = colours("new.rs");
    let (old_glyph, old_label) = colours("old.rs");
    assert_eq!(
        new_glyph, new_label,
        "the glyph is not the colour of the name beside it:\n{dump}"
    );
    assert_eq!(
        old_glyph, old_label,
        "the glyph is not the colour of the name beside it:\n{dump}"
    );
    assert_ne!(
        new_glyph, old_glyph,
        "both glyphs are one colour, so neither is the name's:\n{dump}"
    );
}

/// The glyph is not in the haystack. Nothing a reader types is a private-use
/// codepoint, and having one in there would only skew the scores.
#[test]
fn a_query_matches_the_name_and_not_the_glyph() {
    let mut app = app();
    press_function(&mut app, 1);
    app.handle(Event::Search(obelus_search::Event::FilesFound {
        generation: 1,
        paths: vec!["src/app.rs".into()],
        ignored: false,
    }));

    type_text(&mut app, "app");
    let dump = support::render(&mut app, 40, 8);
    assert!(support::text_block(&dump).contains("src/app.rs"), "{dump}");
    assert!(
        support::legend_block(&dump).contains("bg=#38577f"),
        "the name's matched characters lost their background:\n{dump}"
    );

    // And a query of the glyph itself matches nothing.
    for _ in 0..3 {
        press(&mut app, KeyCode::Backspace);
    }
    type_text(&mut app, "\u{e7a8}");
    let dump = support::render(&mut app, 40, 8);
    assert!(
        !support::text_block(&dump).contains("src/app.rs"),
        "the glyph was matchable:\n{dump}"
    );
}

/// Commands and themes are not files; a glyph for each would be decoration.
#[test]
fn the_command_palette_has_no_glyphs() {
    let mut app = app();
    press_control(&mut app, 'p');
    // One row taller than the list needs, because the screen keeps one for
    // the rule over the status bar.
    let dump = support::render(&mut app, 60, 13);
    let text = support::text_block(&dump);
    assert!(
        !text
            .chars()
            .any(|character| ('\u{e000}'..='\u{f8ff}').contains(&character)),
        "a private-use codepoint reached the palette:\n{dump}"
    );
}
