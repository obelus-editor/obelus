//! A caret steps over what a reader sees as one character, not over what
//! the text counts as one.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use obelus_editing::Editing;
use obelus_text::coordinates::{CharColumn, LineNumber};

/// `a`, a heart asked to be a picture, an `e` with an accent on it, `b`.
/// Seven characters and four things on the screen: a caret may stand at
/// columns 0, 1, 3, 5 and 6, and nowhere else.
const SAID: &str = "a\u{2764}\u{fe0f}e\u{301}b";

fn at(column: usize) -> Editing {
    let mut editing = Editing::new(SAID);
    editing.place(LineNumber::new(0), CharColumn::new(column), u16::MAX);
    editing
}

fn press(editing: &mut Editing, code: KeyCode) {
    editing.handle_key(&KeyEvent::new(code, KeyModifiers::NONE), &(), u16::MAX);
}

fn column(editing: &Editing) -> usize {
    editing.cursor().column.get()
}

/// Right and left step over a picture and its selector, and over a letter
/// and its accent, in one press.
///
/// Deliberate break: a step of one character -- `saturating_add(1)` and
/// `saturating_sub(1)`, which is what the arrows did -- stops between the
/// heart and its selector, and the first assertion fails.
#[test]
fn the_arrows_step_over_a_whole_cluster() {
    let mut editing = at(0);
    let mut stops = Vec::new();
    for _ in 0..4 {
        press(&mut editing, KeyCode::Right);
        stops.push(column(&editing));
    }
    assert_eq!(stops, [1, 3, 5, 6], "stepping right");

    let mut stops = Vec::new();
    for _ in 0..4 {
        press(&mut editing, KeyCode::Left);
        stops.push(column(&editing));
    }
    assert_eq!(stops, [5, 3, 1, 0], "stepping left");
}

/// Backspace and delete take a whole cluster, so no selector is left
/// behind to make the character before it a picture, and no accent is
/// left to land on the letter before it.
///
/// Deliberate break: the same one-character step as above, which takes the
/// selector and leaves the heart -- a line drawing of one -- behind.
#[test]
fn backspace_and_delete_take_a_whole_cluster() {
    let mut editing = at(3);
    press(&mut editing, KeyCode::Backspace);
    assert_eq!(editing.said(), "ae\u{301}b", "backspace after the heart");
    assert_eq!(column(&editing), 1);

    press(&mut editing, KeyCode::Delete);
    assert_eq!(editing.said(), "ab", "delete before the accented e");
}

/// A word step lands at a cluster's edge too, though what a word is is told
/// by each character's kind and an accent is not a letter's kind.
///
/// Deliberate break: leaving the column `word_right` found as it is stops
/// the caret on the accent, in the middle of `é`.
#[test]
fn a_word_step_does_not_stop_inside_a_cluster() {
    let mut editing = Editing::new("e\u{301}x");
    editing.place(LineNumber::new(0), CharColumn::new(0), u16::MAX);
    editing.handle_key(
        &KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL),
        &(),
        u16::MAX,
    );
    assert_ne!(
        column(&editing),
        1,
        "the caret stopped between the e and its accent"
    );
}

/// On a wrapped line, end on a row that ends in a picture stops in front
/// of the picture, and up onto that row from far along the one below stops
/// there too -- not one character back from the row's end, which is
/// between the picture and its selector.
///
/// Ten cells: eight `a`s and a heart fill the first row, and the `b`s,
/// with nowhere to break, go on to the second.
///
/// Deliberate breaks: `end - 1` in the `LineEnd` arm, and `row.end - 1` as
/// the ceiling in `column_in_row` -- each stops at column 9.
#[test]
fn a_wrapped_row_ending_in_a_picture_is_not_ended_inside_it() {
    let width = 10;
    let mut editing = Editing::new("aaaaaaaa\u{2764}\u{fe0f}bbbbbbbb");
    editing.place(LineNumber::new(0), CharColumn::new(0), width);
    editing.handle_key(&KeyEvent::new(KeyCode::End, KeyModifiers::NONE), &(), width);
    assert_eq!(column(&editing), 8, "end on the first row");

    editing.place(LineNumber::new(0), CharColumn::new(18), width);
    editing.handle_key(&KeyEvent::new(KeyCode::Up, KeyModifiers::NONE), &(), width);
    assert_eq!(column(&editing), 8, "up from the end of the second row");
}
