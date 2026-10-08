//! What a row that is typed into says while it is still working.
//!
//! Its own binary because it needs nothing else: a picker, a theme and a
//! grid. What is held here is the one thing no golden grid covers -- the
//! mark that says the answer to what was typed has not arrived, and that
//! it says so by moving.

use obelus_component::picker::{Picker, PickerItem, PickerLayout, PickerValue};
use obelus_theme::builtin::DARK;
use ratatui::{buffer::Buffer as CellBuffer, layout::Rect};

/// A picker with something typed into it and a row it matches, waiting or
/// not.
///
/// A row, because a list with none says it is waiting on its own line --
/// see [`an_empty_list_says_so_on_its_line`] -- and the row under it is for
/// a list that has rows and is still being filled.
fn waiting(on: bool) -> Picker {
    let found = PickerItem {
        prose: false,
        marker: None,
        icon: None,
        label: "abc".to_string(),
        detail: None,
        trailing: None,
        changed: None,
        value: PickerValue::Nothing,
        enabled: true,
        colours: None,
        status: None,
        depth: 0,
        opens: None,
        kind: None,
        tab: None,
        section: None,
    };
    let mut picker = Picker::new(vec![found], PickerLayout::FullArea);
    picker.set_query("ab");
    if on {
        picker.filling(Some("Still reading\u{2026}".to_string()));
    }
    picker
}

/// The status row, drawn, as one line of text.
fn prompt(picker: &Picker, phase: u32) -> String {
    let area = Rect {
        x: 0,
        y: 0,
        width: 40,
        height: 1,
    };
    let mut cells = CellBuffer::empty(area);
    obelus_ui::status::prompt_row(
        picker,
        area,
        &mut cells,
        ratatui::style::Style::new(),
        &DARK,
        phase,
    );
    (0..area.width)
        .map(|x| cells[(x, 0)].symbol())
        .collect::<String>()
}

/// A row that is working says so after what was typed, not in front of it.
///
/// Deliberate break: put the mark at `area.x` instead of after the words.
/// What the reader is reading is what they typed, and a mark in front of it
/// moves the text under their eyes every time an answer starts or lands.
#[test]
fn the_mark_comes_after_what_was_typed() {
    let line = prompt(&waiting(true), 0);
    let typed = line.find("ab").expect("what was typed is on the row");
    let mark = line
        .find(obelus_ui::spinning(0))
        .expect("the mark is on the row");
    assert!(mark > typed, "the mark is in front of the words: {line:?}");
}

/// And a row with nothing outstanding does not wear one.
///
/// Deliberate break: draw it whatever `is_filling` says. Then every prompt
/// in Obelus carries a mark that means "still going", including the ones
/// that answered before the frame was drawn.
#[test]
fn a_row_that_is_not_waiting_wears_no_mark() {
    let line = prompt(&waiting(false), 0);
    assert!(
        !line.contains(obelus_ui::spinning(0)),
        "a finished list is still turning: {line:?}"
    );
}

/// A list with nothing in it yet says so on its own line, with the mark in
/// front of the words -- and the row under it says nothing more.
///
/// Deliberate breaks: draw the line with `nothing` whatever `is_filling`
/// says, and the line wears no mark; draw the row's mark whether or not
/// the list is empty, and the waiting is said in two places.
#[test]
fn an_empty_list_says_so_on_its_line() {
    use ratatui::widgets::Widget as _;

    let mut picker = Picker::new(Vec::new(), PickerLayout::FullArea);
    picker.while_empty("Still asking");
    picker.filling(Some("Asking\u{2026}".to_string()));
    let area = Rect {
        x: 0,
        y: 0,
        width: 40,
        height: 6,
    };
    let mut cells = CellBuffer::empty(area);
    obelus_ui::picker::PickerView::new(&picker, &DARK, 0, true).render(area, &mut cells);
    let line = (0..area.height)
        .map(|y| {
            (0..area.width)
                .map(|x| cells[(x, y)].symbol())
                .collect::<String>()
        })
        .find(|line| line.contains("Still asking"))
        .expect("the list says why it is empty");
    let mark = line
        .find(obelus_ui::spinning(0))
        .expect("no mark on the list's line");
    assert!(
        mark < line.find("Still asking").unwrap_or(0),
        "the mark is not in front of the words: {line:?}"
    );
    assert!(
        !prompt(&picker, 0).contains(obelus_ui::spinning(0)),
        "the waiting is said on the list and again under it"
    );
}

/// And it turns, or it says the same thing whether anything is happening
/// or not.
///
/// Deliberate break: draw `spinning(0)` rather than `spinning(phase)`.
#[test]
fn the_mark_turns() {
    assert_ne!(
        prompt(&waiting(true), 0),
        prompt(&waiting(true), 1),
        "the row is the same on two frames, so nothing on it is turning"
    );
}
