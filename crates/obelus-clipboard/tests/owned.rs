//! What changes when Obelus owns the clipboard itself.
//!
//! Its own binary because the owner is installed once per process, and the
//! tests in it take turns for the same reason: there is one clipboard, so
//! a fake that two of them were setting up at once would be answering
//! either's question with the other's copy.

use std::sync::{Mutex, MutexGuard};

use obelus_clipboard::{Owner, Provider};

/// An owner that writes down what it was asked, and answers.
#[derive(Default)]
struct Fake {
    /// The shapes of the last copy, in the order they were offered.
    offered: Mutex<Vec<(String, Vec<u8>)>>,
    /// Every name anybody asked for the bytes of.
    asked: Mutex<Vec<String>>,
}

impl Fake {
    fn offered(&self) -> Vec<(String, Vec<u8>)> {
        self.offered.lock().expect("the fake's shapes").clone()
    }

    fn asked(&self) -> Vec<String> {
        self.asked.lock().expect("the fake's questions").clone()
    }
}

impl Owner for &'static Fake {
    fn offer(&self, shapes: Vec<(String, Vec<u8>)>) -> bool {
        *self.offered.lock().expect("the fake's shapes") = shapes;
        true
    }

    fn holding(&self, mime: &str) -> Option<Vec<u8>> {
        self.asked
            .lock()
            .expect("the fake's questions")
            .push(mime.to_string());
        self.offered()
            .into_iter()
            .find(|(name, _)| name == mime)
            .map(|(_, bytes)| bytes)
    }

    fn holds(&self) -> Vec<String> {
        self.offered().into_iter().map(|(name, _)| name).collect()
    }
}

/// The one owner this binary has.
static FAKE: std::sync::OnceLock<&'static Fake> = std::sync::OnceLock::new();

/// Whose turn it is.
static TURN: Mutex<()> = Mutex::new(());

/// The fake, with the clipboard to itself until the guard is dropped.
fn taking_turns() -> (&'static Fake, MutexGuard<'static, ()>) {
    // A test that fails poisons this, and the ones after it would then all
    // fail for that reason rather than their own.
    let turn = TURN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let fake = *FAKE.get_or_init(|| {
        let fake: &'static Fake = Box::leak(Box::new(Fake::default()));
        obelus_clipboard::owned_by(Box::new(fake));
        fake
    });
    // Nothing of this reaches the clipboard of whoever is running the
    // suite, and nothing Obelus kept earlier answers for the owner.
    obelus_clipboard::use_provider_for_test(Provider::Kept);
    *fake.offered.lock().expect("the fake's shapes") = Vec::new();
    fake.asked.lock().expect("the fake's questions").clear();
    (fake, turn)
}

/// An owner takes the copy, and offers the words under every name they go
/// by.
///
/// The three names are the point: a program written against X11 asks for
/// `UTF8_STRING` and one written yesterday asks for
/// `text/plain;charset=utf-8`, and a copy that answered only one of them
/// would paste into some programs and not others -- which is not something
/// a machine without those programs installed can be shown.
///
/// Deliberate break: `copy` going to `to_a_program` without asking the
/// owner (nothing is offered), and `WORDS` cut down to one name (the
/// second assertion).
#[test]
fn a_copy_goes_to_the_owner_under_every_name_words_go_by() {
    let (fake, _turn) = taking_turns();

    obelus_clipboard::copy("fn main() {}").expect("copying through an owner");

    let offered = fake.offered();
    assert!(!offered.is_empty(), "the owner was not asked to take it");
    let names: Vec<&str> = offered.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        names,
        ["text/plain;charset=utf-8", "UTF8_STRING", "text/plain"]
    );
    for (_, bytes) in &offered {
        assert_eq!(bytes, b"fn main() {}");
    }
}

/// And what it holds is what the three questions about the clipboard are
/// answered from.
///
/// Deliberate break: any of the three asking `native::types`, the provider
/// or `KEPT` before the owner -- each of which leaves its own assertion
/// with the empty answer a machine running the suite has.
#[test]
fn what_the_owner_holds_is_what_is_on_the_clipboard() {
    let (fake, _turn) = taking_turns();

    fake.offer(vec![
        ("text/plain;charset=utf-8".to_string(), b"a note".to_vec()),
        ("image/png".to_string(), vec![0x89, b'P', b'N', b'G']),
    ]);

    assert_eq!(
        obelus_clipboard::types(),
        ["text/plain;charset=utf-8", "image/png"]
    );
    assert_eq!(
        obelus_clipboard::paste_as("image/png"),
        Some(vec![0x89, b'P', b'N', b'G'])
    );
    assert_eq!(obelus_clipboard::paste(), Some("a note".to_string()));
}

/// The hand-over on the way out gives away the words and not the shapes.
///
/// Whatever takes the clipboard over is a program that knows nothing about
/// Obelus, and a private shape handed to it is bytes nobody can read.
///
/// Deliberate break: `hand_over` asking for the first shape the owner
/// holds rather than for the words -- which here is the picture.
#[test]
fn the_hand_over_gives_away_the_words() {
    let (fake, _turn) = taking_turns();

    fake.offer(vec![
        ("image/png".to_string(), vec![0x89]),
        ("text/plain;charset=utf-8".to_string(), b"a note".to_vec()),
    ]);
    fake.asked.lock().expect("the fake's questions").clear();

    // `Provider::Kept` is nothing to hand over *to*, which stops this
    // before it reaches a program -- after it has asked for what to hand
    // over, which is what is being read here.
    obelus_clipboard::hand_over();

    assert_eq!(fake.asked(), ["text/plain;charset=utf-8"]);
}
