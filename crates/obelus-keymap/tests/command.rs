//! The switch that draws control as command, in a binary of its own: it is a
//! global, and a test beside the others that turned it on would change what
//! `label` says underneath them.

use obelus_keymap::{control, control_drawn_as_command, draw_control_as_command};

/// What is drawn follows the switch, and only the switch.
///
/// Broken deliberately by having `label` pass `false` for command: the
/// first assertion fails.
#[test]
fn the_screen_draws_control_the_way_the_switch_says() {
    let save = control('s');
    let glyphs = obelus_icons::enabled();

    draw_control_as_command(true);
    assert!(control_drawn_as_command());
    assert_eq!(save.label(), save.label_as(glyphs, true));
    // The file's spelling is not the screen's.
    assert_eq!(save.label_in(false), "Ctrl+s");

    draw_control_as_command(false);
    assert_eq!(save.label(), save.label_as(glyphs, false));
}
