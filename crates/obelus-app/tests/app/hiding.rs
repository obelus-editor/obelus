//! The files a system keeps out of sight.
//!
//! Its own switch and not part of the one about what a project ignores,
//! because they keep two different things out: one is what the project
//! said, and the other is what the machine says not to show. A reader
//! after `.github/workflows/ci.yml` is not asking to see `target`, and one
//! after a build log is not asking to see `.env`.
//!
//! Which files those are is `ignore`'s answer rather than one of Obelus's:
//! a name beginning with a dot everywhere, and on Windows a file the
//! system has marked as well. So these tests are about the *switch* and
//! use dotted names because that is what holds on the machine they run on
//! -- a test that wrote the rule down would be a second answer to a
//! question the walk is already answering.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use obelus_app::{app::App, event::Event};
use obelus_command::Command;

use crate::support;

/// A project with a hidden file, an ignored one and an ordinary one.
fn scratch(name: &str) -> support::Scratch {
    let scratch = support::Scratch::new(name);
    scratch.write("plain.rs", "fn plain() {}\n");
    scratch.write(".env", "SECRET=1\n");
    scratch.write("skipped.rs", "fn skipped() {}\n");
    scratch.write(".ignore", "skipped.rs\n");
    scratch
}

/// What the walk offers, with each switch set as it is given.
fn found(scratch: &support::Scratch, ignored: bool, hidden: bool, generation: u64) -> Vec<String> {
    let (sender, events) = std::sync::mpsc::channel();
    let latest = obelus_runtime::cancel::Latest::default();
    for _ in 0..generation {
        latest.next();
    }
    obelus_search::spawn_walk(
        scratch.path(),
        latest.claim(generation),
        ignored,
        hidden,
        sender,
    );
    let mut names = Vec::new();
    while let Ok(Event::Search(obelus_search::Event::FilesFound { paths, .. })) = events.recv() {
        names.extend(paths.into_iter().map(|path| path.display().to_string()));
    }
    names.sort();
    names
}

/// Two switches, and each keeps its own thing out.
///
/// The one about what a project ignores does not bring the hidden files
/// with it, and the one about hidden files does not bring the ignored ones
/// -- which is the whole reason there are two.
///
/// Deliberate break: hand `walk` its `obeying` where it is handed `hidden`.
/// One switch then answers for both, and a reader who asked for `.env` is
/// given what `.gitignore` keeps out as well.
#[test]
fn the_two_switches_keep_two_different_things_out() {
    let scratch = scratch("hiding-apart");

    // Neither: the ordinary file and nothing else.
    assert_eq!(found(&scratch, false, false, 1), vec!["plain.rs"]);

    // What the project ignores, and still nothing hidden -- `.ignore`
    // itself is a hidden file and is not offered by this one.
    assert_eq!(
        found(&scratch, true, false, 2),
        vec!["plain.rs", "skipped.rs"]
    );

    // And the hidden ones, with what the project ignores still out.
    assert_eq!(
        found(&scratch, false, true, 3),
        vec![".env", ".ignore", "plain.rs"]
    );

    // Both, which is every file there is.
    assert_eq!(
        found(&scratch, true, true, 4),
        vec![".env", ".ignore", "plain.rs", "skipped.rs"]
    );
}

/// `.git` comes with them, which is the cost of the switch meaning what it
/// says.
///
/// A directory whose files are a database is still a directory of files,
/// and an exception carved out in the walk would be Obelus deciding which
/// of the reader's hidden files they meant. Which was the reason the switch
/// did not exist: it is a cost, and it is the reader's to weigh now.
///
/// Deliberate break: give the walk a `filter_entry` that drops `.git`. The
/// switch then says it offers the files a system keeps out of sight and
/// offers all but one of them.
#[test]
fn the_git_directory_comes_with_them() {
    let scratch = support::Scratch::new("hiding-git");
    scratch.write("plain.rs", "fn plain() {}\n");
    scratch.write(".git/HEAD", "ref: refs/heads/main\n");

    assert_eq!(found(&scratch, false, false, 1), vec!["plain.rs"]);
    assert_eq!(
        found(&scratch, false, true, 2),
        vec![support::as_shown(".git/HEAD"), "plain.rs".to_string()],
        "the switch left one of them out"
    );
}

/// The key beside the one for what a project ignores, because they are two
/// switches about the same question.
///
/// Deliberate break: take the `.` arm out of `listing_key`. The setting is
/// still there and the palette still offers the key, and pressing it does
/// nothing -- which is the shape the palette exists to avoid.
#[test]
fn the_key_turns_it_on_and_the_palette_says_so() {
    let scratch = scratch("hiding-key");
    let mut app = App::new(Vec::new());
    app.working_directory_for_test(scratch.path().to_path_buf());
    app.statuses_for_test(std::collections::HashMap::new());
    support::lay_out(&mut app, 76, 16);
    obelus_app::app::dispatch::dispatch(&mut app, Command::FileOpen);

    assert!(
        support::usable_here(&mut app).contains(&"Hidden files".to_string()),
        "the palette does not offer it"
    );
    assert!(!app.config().hidden_files, "it starts off");

    let press = |app: &mut App| {
        app.handle(Event::Key(KeyEvent::new(
            KeyCode::Char('.'),
            KeyModifiers::ALT,
        )));
    };
    press(&mut app);
    assert!(app.config().hidden_files, "the key did nothing");
    // And the one beside it is untouched, which is what two switches are
    // for.
    assert!(!app.config().ignored_files, "it turned the other one on");

    // Back again, because a switch a reader cannot put back is not one.
    press(&mut app);
    assert!(!app.config().hidden_files, "it would not go back");
}

/// The tree obeys it too, because it is the same question asked of a
/// directory rather than of the project.
///
/// The tree walks for itself -- one level at a time, so it can say which
/// directories have anything in them -- so a switch threaded only through
/// the flat listing would have the two tabs of one list disagreeing about
/// which files there are.
///
/// Deliberate break: drop the `hidden` from `tree::inside` and leave the
/// walk there at its default. `f1`'s flat tab offers `.env` and its tree
/// tab does not, which is one list with two answers.
#[test]
fn the_tree_obeys_it_as_well() {
    use obelus_search::tree;

    let scratch = scratch("hiding-tree");
    let root = scratch.path();
    let named = |hidden: bool| -> Vec<String> {
        let mut names: Vec<String> = tree::inside(root, root, false, hidden)
            .into_iter()
            .map(|entry| entry.path.display().to_string())
            .collect();
        names.sort();
        names
    };

    assert_eq!(named(false), vec!["plain.rs"]);
    assert_eq!(named(true), vec![".env", ".ignore", "plain.rs"]);
}

/// The search of the project offers both switches, because it walks the
/// files they choose: it obeyed them before it said so, and a reader who had
/// turned them on in the file list found `target` in a search and nothing on
/// screen saying why.
///
/// Deliberate breaks: leave `offering_hidden` out of `refresh_search`, and
/// the palette over the search has no such key; leave `listing_key`
/// refreshing the file list where the search asked, and pressing it fills
/// the search with the project's tree; set them on the file's tab too, and
/// that tab offers a switch about a walk it never takes.
#[test]
fn the_search_of_the_project_offers_them_too() {
    let scratch = scratch("hiding-search");
    let buffer = obelus_buffer::Buffer::open(&scratch.path().join("plain.rs")).expect("plain.rs");
    let mut app = App::new(vec![buffer]);
    app.working_directory_for_test(scratch.path().to_path_buf());
    app.statuses_for_test(std::collections::HashMap::new());
    support::lay_out(&mut app, 76, 16);

    // Not on the file's tab: one file is read, and nothing is walked.
    obelus_app::app::dispatch::dispatch(&mut app, Command::SearchFile);
    let offered = support::usable_here(&mut app);
    assert!(
        !offered.contains(&"Hidden files".to_string()),
        "the file's tab offers it: {offered:?}"
    );
    support::press(&mut app, KeyCode::Esc);

    obelus_app::app::dispatch::dispatch(&mut app, Command::SearchProject);
    let offered = support::usable_here(&mut app);
    assert!(
        offered.contains(&"Hidden files".to_string())
            && offered.contains(&"Ignored files".to_string()),
        "the project's tab does not offer them: {offered:?}"
    );

    support::press_alt_key(&mut app, KeyCode::Char('.'));
    assert!(app.config().hidden_files, "the key did nothing");
    assert!(!app.config().ignored_files, "it turned the other one on");
    let set = support::keys_here(&mut app)
        .into_iter()
        .find(|key| key.does == "Hidden files")
        .and_then(|key| key.set);
    assert_eq!(set, Some(true), "the palette does not say it is on");

    // Still the search, asked again: nothing typed, so nothing found --
    // not the project's tree, which is the file list's answer.
    let picker = app.picker().expect("the search closed");
    assert!(picker.is_searching(), "the search became something else");
    assert_eq!(picker.matches().count(), 0, "the search filled with files");

    support::press_alt_key(&mut app, KeyCode::Char('.'));
    assert!(!app.config().hidden_files, "it would not go back");
}
