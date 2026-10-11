//! The switch that draws control as command, in a binary of its own: it is a
//! global, and a test beside the others that turned it on would change what
//! `label` says underneath them.

use obelus_keymap::{control, control_drawn_as_command, draw_control_as_command};

/// What is drawn follows the switch, and only the switch.
///
/// Broken deliberately by having `label` pass `false` for command: the
/// assertion that the screen says command fails.
#[test]
fn the_screen_draws_control_the_way_the_switch_says() {
    let save = control('s');
    // Either way of drawing it, because which is the glyph switch's.
    let says = |label: &str, word: &str, glyph: char| label.contains(word) || label.contains(glyph);

    draw_control_as_command(true);
    assert!(control_drawn_as_command());
    assert!(
        says(&save.label(), "Cmd", obelus_icons::key::COMMAND),
        "the screen does not say command: {}",
        save.label()
    );
    // The file's spelling is not the screen's.
    assert_eq!(save.label_in(false), "Ctrl+s");

    draw_control_as_command(false);
    assert!(says(&save.label(), "Ctrl", obelus_icons::key::CONTROL));
}
