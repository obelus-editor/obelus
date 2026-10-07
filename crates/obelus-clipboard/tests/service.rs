//! The clipboards that are a system service, read back through a paste.
//!
//! Only where there is one, and only when asked for with `--ignored`: the
//! service is the clipboard of whoever is running the suite, and these put
//! words on it. Its own binary so that nothing else in the process is
//! copying at the same moment.
#![cfg(any(target_os = "macos", windows))]

use std::sync::{Mutex, MutexGuard};

use obelus_clipboard::Provider;

/// Whose turn it is: there is one clipboard, and one provider asked for.
static TURN: Mutex<()> = Mutex::new(());

fn taking_turns() -> MutexGuard<'static, ()> {
    // A test that fails poisons this, and the one after it would then fail
    // for that reason rather than its own.
    TURN.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// What another program put on the clipboard is what a paste gives back --
/// and nothing of it reaches a test that asked for the clipboard Obelus
/// keeps.
///
/// The escape sequence as the provider, because that is what a plain
/// Windows machine detects: the one provider that writes and cannot read.
/// And `KEPT` emptied after the copy, so that the words can only have come
/// back from the service.
///
/// Deliberate breaks: `paste` without its question to `native` -- which
/// was the bug, and answers `None` here -- and `the_service_may_be_asked`
/// answering yes for `Provider::Kept`, which hands the last assertion the
/// words.
#[test]
#[ignore = "writes the clipboard of whoever runs it"]
fn a_paste_reads_what_the_service_holds() {
    let _turn = taking_turns();
    obelus_clipboard::use_provider_for_test(Provider::Osc52);
    obelus_clipboard::copy("from somewhere else").expect("copying to the service");
    obelus_clipboard::use_provider_for_test(Provider::Osc52);

    assert_eq!(
        obelus_clipboard::paste().as_deref(),
        Some("from somewhere else")
    );

    obelus_clipboard::use_provider_for_test(Provider::Kept);
    assert_eq!(obelus_clipboard::paste(), None);
}

/// Windows' line breaks arrive as the ones a buffer is written in.
///
/// Every program there copies `\r\n`, and pasted as it came each line gets
/// a `\r` the file did not have.
///
/// Deliberate break: `text` handing the string back without the
/// `replace`.
#[cfg(windows)]
#[test]
#[ignore = "writes the clipboard of whoever runs it"]
fn a_paste_on_windows_breaks_lines_with_a_line_feed() {
    let _turn = taking_turns();
    obelus_clipboard::use_provider_for_test(Provider::Osc52);
    obelus_clipboard::copy("one\r\ntwo\r\n").expect("copying to the service");
    obelus_clipboard::use_provider_for_test(Provider::Osc52);

    assert_eq!(obelus_clipboard::paste().as_deref(), Some("one\ntwo\n"));
}

/// And nothing of the service reaches the other three questions either,
/// for a test that asked for the clipboard Obelus keeps: not what shapes
/// it holds, not the bytes of one, and not a copy.
///
/// The words put there first under the escape sequence, which lets the
/// service be asked, so that each question has something to wrongly find.
///
/// Deliberate breaks, one at a time: the question to
/// `the_service_may_be_asked` taken out of `types` (it lists
/// `text/plain`), of `paste_as` (it hands the words back), and of `copy`
/// (the service then holds the second copy rather than the first).
#[test]
#[ignore = "writes the clipboard of whoever runs it"]
fn a_test_that_asked_for_kept_reaches_no_service() {
    let _turn = taking_turns();
    obelus_clipboard::use_provider_for_test(Provider::Osc52);
    obelus_clipboard::copy("on the service").expect("copying to the service");

    obelus_clipboard::use_provider_for_test(Provider::Kept);
    assert_eq!(obelus_clipboard::types(), Vec::<String>::new());
    assert_eq!(obelus_clipboard::paste_as("text/plain"), None);
    obelus_clipboard::copy("only Obelus's").expect("copying to what Obelus keeps");

    // Asked again, with `KEPT` emptied, so the answer is the service's.
    obelus_clipboard::use_provider_for_test(Provider::Osc52);
    assert_eq!(obelus_clipboard::paste().as_deref(), Some("on the service"));
}
